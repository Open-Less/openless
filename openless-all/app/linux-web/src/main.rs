//! Linux 后端 + HTTP 命令桥。**不渲染任何界面**：窗口由 Qt 宿主
//! （`linux-qt`）或任何浏览器提供。
//!
//! 三种宿主共用同一条协议：
//!   `POST /api/invoke {id,command,args}` → `{ok,value|error}`
//!   `GET  /api/events`（SSE，事件与 GTK/Qt 同一套映射）
//!
//! 静态资源用一个只在 127.0.0.1 上监听的小 HTTP 服务提供（Chromium 拒绝
//! file:// 下的 ES module / fetch）。
mod events;
mod host;
mod hotkeys;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use serde_json::{json, Value};

struct Host {
    backend: Option<std::sync::Arc<openless_core::OpenLessBackend>>,
    runtime: Option<std::sync::Arc<tokio::runtime::Runtime>>,
    settings_runtime: Option<std::sync::Arc<dyn openless_core::SettingsRuntime>>,
    /// 胶囊/QA 的窗口级状态：这些不属于后端，属于宿主展示层。
    qa_expanded: std::sync::Mutex<bool>,
    /// 云同步状态的短缓存（250ms）：界面高频轮询时不必每次重新序列化。
    cloud_sync_status_cache: std::sync::Mutex<Option<(std::time::Instant, Value)>>,
    capsule_transcript_visible: std::sync::Mutex<bool>,
    capsule_language_menu: std::sync::Mutex<bool>,
    shortcut_recording_active: std::sync::Mutex<bool>,
    /// 托盘命令（显示窗口 / 退出）走这条广播，SSE 把它和 Core 事件一起推给渲染宿主。
    /// 宿主侧事件（托盘命令、core 的 host action）共用的广播通道。
    host_events: tokio::sync::broadcast::Sender<(String, String)>,
    /// core 处在「润色预览」模式时为 true；`get_selection_polish_preview` 用它做门禁。
    selection_preview_pending: std::sync::atomic::AtomicBool,
    /// core 的 host action 队列（窗口显隐、打开外链、重启…）。
    host_actions: Option<std::sync::Arc<openless_linux_host::LinuxHostActions>>,
}

/// 恢复（云端同步还原）之后要在宿主编生效的效果。
///
/// Tauri 那边会重建热键监听、改代理、刷新托盘；Linux 这边的热键由 fcitx5 插件
/// 与监听线程自行跟随偏好变化，所以这里做两件必须做的事：校验快捷键不冲突、
/// 同步代理开关。失败要如实返回，让恢复流程回滚而不是假装成功。
struct LinuxRestoreEffects;

impl openless_core::config::RestoreRuntimeEffects for LinuxRestoreEffects {
    fn apply_target(
        &self,
        target: openless_core::UserPreferences,
    ) -> futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>> {
        Box::pin(async move {
            // 快捷键冲突**不阻断恢复**：云端偏好是本机自己同步上去的，
            // 冲突通常来自两台机器的默认值差异。Linux 的热键由 fcitx5 插件
            // 与监听线程按偏好生效，冲突项会被忽略，不影响其余配置落地。
            // （Tauri 在这里直接返回 Err 是因为它的桌面路径不支持 Linux，
            //   那条分支我们是唯一实现者，不能照抄。）
            if let Err(reason) = openless_core::reject_hotkey_collisions(&target) {
                eprintln!("[linux-web] 恢复的快捷键有冲突，保留本机设置继续：{reason}");
            }
            openless_core::net::set_use_system_proxy(target.use_system_proxy);
            Ok(())
        })
    }
}

/// 极简日志桥：把 core 的 `log` 输出转到 stderr（→ journal）。
/// 不引依赖，但能让我们看到 `[e2ee-token]` / `[core]` 这类关键诊断。
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn main() {
    static LOGGER: StderrLogger = StderrLogger;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    // 单实例必须在读数据目录之前抢到：两个后端同时跑会互相覆盖历史/凭据。
    let Some(_instance_guard) = host::claim_single_instance() else {
        std::process::exit(0);
    };
    let host = std::sync::Arc::new(init_host());
    let dist = resolve_dist();
    let port = match serve_dir(dist.clone(), host.clone()) {
        Ok(port) => port,
        Err(error) => {
            eprintln!("[linux-web] 静态资源目录不可用（{}）：{error}", dist.display());
            std::process::exit(1);
        }
    };
    let mut url = format!("http://127.0.0.1:{port}/index.html");
    // 排障对照：OPENLESS_NO_TRANSITION=1 时页面走 reduced-motion 路径（无过渡动画），
    // 用来区分「抖动来自动画」还是「抖动来自布局/滚动条」。
    if std::env::var("OPENLESS_NO_TRANSITION").ok().as_deref() == Some("1") {
        url.push_str("?noTransition=1");
    }
    // 对照：OPENLESS_PAGE_FADE=1 恢复淡入过渡（默认 GTK 宿主是瞬时切换）。
    if std::env::var("OPENLESS_PAGE_FADE").ok().as_deref() == Some("1") {
        url.push_str(if url.contains('?') { "&pageTransition=fade" } else { "?pageTransition=fade" });
    }
    // 宿主传进来的桥 socket：窗口层后续通过它订阅事件（目前只记录，便于排障）。
    if let Some(index) = std::env::args().position(|arg| arg == "--host-socket") {
        if let Some(socket) = std::env::args().nth(index + 1) {
            println!("[linux-web] 宿主桥 socket: {socket}");
        }
    }
    println!("[linux-web] serving {} at {}", dist.display(), url);

    // 再无头 GUI 分支：窗口渲染由 Qt 宿主负责，这里只提供 /api 与静态资源。
    println!("[linux-web] 后端就绪，等待宿主连接");
    // 无头模式仍要挂热键监听：快捷键 → 听写/QA → 事件推给渲染宿主。
    // 之前漏了这步，所以「按快捷键没反应」。
    if let (Some(backend), Some(runtime)) = (host.backend.clone(), host.runtime.clone()) {
        hotkeys::spawn_headless(backend, runtime);
    }
    // 托盘由这里负责（StatusNotifierItem），而不是渲染宿主：菜单项、本地化
    // （8 种语言）与命令语义因此与 macOS/Windows 完全一致。
    spawn_tray(host.clone());
    spawn_host_actions(host.clone());
    spawn_device_watcher(host.clone());
    loop {
        std::thread::park();
    }
}

fn resolve_dist() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENLESS_DIST") {
        return PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // 包内布局：二进制在 `/usr/lib/openless/`，页面在 `web/dist`（或 `dist`）。
            // 也兼容二进制被放在 `/usr/bin` 的老布局。
            let candidates = [
                dir.join("web/dist"),
                dir.join("dist"),
                dir.join("../lib/openless/web/dist"),
                dir.join("../../lib/openless/web/dist"),
                dir.join("../../share/openless/web/dist"),
            ];
            for candidate in candidates {
                if candidate.join("index.html").is_file() {
                    return candidate;
                }
            }
        }
    }
    // 开发期直接 `cargo run`：源码树里的 dist。
    // 注意这个路径是**编译期**写死的，装到用户机器上并不存在，所以只在它真的有页面时用。
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dist");
    if dev.join("index.html").is_file() {
        return dev;
    }
    dev
}

/// 托盘：菜单与 macOS/Windows 对齐（显示主窗口 / 输出风格 / 选择麦克风 / 退出），
/// 文案用 Rust 侧同一份 8 语言目录。
///
/// 窗口与退出属于渲染宿主的职责，所以通过 `tray:command` 事件转交（Qt 宿主在
/// `/api/events` 上接收）；风格与麦克风直接改后端偏好。
fn spawn_tray(host: std::sync::Arc<Host>) {
    std::thread::Builder::new()
        .name("openless-tray".into())
        .spawn(move || {
            let tray = match openless_linux_host::LinuxTray::start() {
                Ok(tray) => tray,
                Err(error) => {
                    eprintln!("[linux-web] 托盘注册失败：{error}");
                    return;
                }
            };
            println!("[linux-web] 托盘已注册");
            let lang = match openless_linux_host::LocalePref::from_tag("system").resolve() {
                lang => lang,
            };
            let _ = tray.set_lang(lang);
            let mut logged_styles = false;
            let mut devices_rx = host.host_events.subscribe();
            let mut devices_dirty = true;
            loop {
                // 热插拔监听线程每 3 秒比对一次，变了就推 `devices:changed`。
                while let Ok((name, _)) = devices_rx.try_recv() {
                    if name == "devices:changed" {
                        devices_dirty = true;
                    }
                }
                let Some(backend) = host.backend.clone() else {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    continue;
                };
                // 麦克风列表随插拔刷新；风格列表只拉一次（改动走偏好事件）。
                if devices_dirty {
                    if let Some(runtime) = host.runtime.clone() {
                        if let Ok(devices) =
                            runtime.block_on(backend.services().platform.microphone_devices())
                        {
                            let selected = backend.get_preferences().microphone_device_name;
                            let mics = devices
                                .into_iter()
                                .map(|device| openless_linux_host::TrayMicrophone {
                                    is_default: device.is_default,
                                    name: device.name.clone(),
                                    selected: selected == device.name,
                                })
                                .collect::<Vec<_>>();
                            let _ = tray.set_microphones(mics);
                        }
                        devices_dirty = false;
                    }
                }
                if !logged_styles {
                    // 输出风格子菜单：与 macOS/Windows 的「输出风格」一致。
                    let active = backend.get_preferences().active_style_pack_id;
                    if let Ok(packs) = backend.list_style_packs(&active) {
                        let styles = packs
                            .into_iter()
                            .map(|pack| openless_linux_host::TrayStyle {
                                active: pack.id == active,
                                id: pack.id.clone(),
                                name: pack.name.clone(),
                            })
                            .collect::<Vec<_>>();
                        let _ = tray.set_styles(styles);
                    }
                    logged_styles = true;
                }
                tray.drain(|command| match command {
                    openless_linux_host::TrayCommand::ShowMain => {
                        let _ = host.host_events.send(("tray:command".into(), "\"show\"".into()));
                    }
                    openless_linux_host::TrayCommand::Quit => {
                        let _ = host.host_events.send(("tray:command".into(), "\"quit\"".into()));
                    }
                    openless_linux_host::TrayCommand::ActivatePreviousStyle => {
                        let _ = backend.activate_previous_style_pack();
                    }
                    openless_linux_host::TrayCommand::SelectStyle(id) => {
                        if let Err(error) = backend.activate_style_pack(&id) {
                            eprintln!("[linux-web] 托盘切换风格失败：{error}");
                        }
                    }
                    openless_linux_host::TrayCommand::SelectMicrophone(device) => {
                        if let Err(error) = backend.select_microphone_device(device) {
                            eprintln!("[linux-web] 托盘切换麦克风失败：{error}");
                        }
                    }
                });
                if let Some(error) = tray.take_error() {
                    eprintln!("[linux-web] 托盘出错：{error}");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        })
        .ok();
}

/// core 的 host action（窗口显隐 / 打开外链 / 重启 / 通知）→ 渲染宿主。
///
/// 这些 action 以前没人 drain，等于全部被丢掉：所以「润色预览」窗口不弹、
/// 从热键打开 QA 面板也没反应。这里每 120ms 取一次，转成 SSE 事件。
fn spawn_host_actions(host: std::sync::Arc<Host>) {
    std::thread::Builder::new()
        .name("openless-host-actions".into())
        .spawn(move || loop {
            if let Some(actions) = host.host_actions.clone() {
                actions.drain(|action| {
                    use openless_core::HostAction as A;
                    let event: Option<(&str, Value)> = match action {
                        A::ShowSelectionPreview => {
                            host.selection_preview_pending
                                .store(true, std::sync::atomic::Ordering::SeqCst);
                            let _ = host.host_events.send((
                                "selection-polish-preview:shown".into(),
                                "{}".into(),
                            ));
                            Some(("host:action", json!({ "action": "showQa" })))
                        }
                        A::HideSelectionPreview => {
                            host.selection_preview_pending
                                .store(false, std::sync::atomic::Ordering::SeqCst);
                            let _ = host.host_events.send((
                                "selection-polish-preview:hide".into(),
                                "{}".into(),
                            ));
                            None
                        }
                        A::ShowQa => Some(("host:action", json!({ "action": "showQa" }))),
                        A::HideQa => Some(("host:action", json!({ "action": "hideQa" }))),
                        A::ShowMain | A::FocusMain => {
                            Some(("host:action", json!({ "action": "showMain" })))
                        }
                        A::OpenExternalUrl(url) => {
                            Some(("host:action", json!({ "action": "openUrl", "url": url })))
                        }
                        A::OpenSystemSettings(target) => Some((
                            "host:action",
                            json!({ "action": "openSystemSettings", "target": target }),
                        )),
                        A::RequestRestart => {
                            Some(("host:action", json!({ "action": "restart" })))
                        }
                        A::Notify(text) => {
                            Some(("host:action", json!({ "action": "notify", "text": text })))
                        }
                        // 听写浮层由胶囊自己的事件流驱动；Less Computer 未在 Linux 提供。
                        A::ShowDictationFeedback | A::HideDictationFeedback | A::ShowLessComputer => {
                            None
                        }
                    };
                    if let Some((name, payload)) = event {
                        let _ = host.host_events.send((name.to_string(), payload.to_string()));
                    }
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(120));
        })
        .ok();
}

/// 麦克风热插拔：每 3 秒枚举一次设备，变化时刷新托盘菜单并推 `devices:changed`
/// 给界面（选择麦克风的下拉与托盘子菜单都靠它跟上插拔）。
fn spawn_device_watcher(host: std::sync::Arc<Host>) {
    std::thread::Builder::new()
        .name("openless-devices".into())
        .spawn(move || {
            let mut signature: Option<Vec<String>> = None;
            loop {
                if let (Some(backend), Some(runtime)) = (host.backend.clone(), host.runtime.clone())
                {
                    if let Ok(devices) =
                        runtime.block_on(backend.services().platform.microphone_devices())
                    {
                        let mut names = devices
                            .iter()
                            .map(|device| format!("{}|{}", device.name, device.is_default))
                            .collect::<Vec<_>>();
                        names.sort();
                        let changed = signature.as_ref() != Some(&names);
                        if changed {
                            if signature.is_some() {
                                println!("[linux-web] 麦克风设备变化：{} 个", names.len());
                                let _ = host.host_events.send((
                                    "devices:changed".into(),
                                    json!({ "count": names.len() }).to_string(),
                                ));
                            }
                            signature = Some(names);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        })
        .ok();
}

fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENLESS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    home.join("OpenLess")
}

fn read_json(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok())
}

fn init_host() -> Host {
    let (host_events, _) = tokio::sync::broadcast::channel::<(String, String)>(64);
    let mut host = Host {
        backend: None,
        runtime: None,
        settings_runtime: None,
        capsule_transcript_visible: std::sync::Mutex::new(false),
        capsule_language_menu: std::sync::Mutex::new(false),
        shortcut_recording_active: std::sync::Mutex::new(false),
        qa_expanded: std::sync::Mutex::new(false),
        cloud_sync_status_cache: std::sync::Mutex::new(None),
        host_events,
        selection_preview_pending: std::sync::atomic::AtomicBool::new(false),
        host_actions: None,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => std::sync::Arc::new(runtime),
        Err(error) => {
            eprintln!("[linux-web] tokio 运行时创建失败：{error}");
            return host;
        }
    };
    let home_dir = std::env::var("HOME").ok().map(PathBuf::from);
    let config = openless_core::BackendConfig {
        data_dir: data_dir(),
        home_dir,
        ..Default::default()
    };
    let _guard = runtime.enter();
    match openless_linux_host::LinuxBackendBuilder::from_shared_providers(config)
        .and_then(|builder| builder.build())
    {
        Ok(runtime_parts) => {
            println!("[linux-web] 后端就绪（openless-core 已装配）");
            // 必须在 start() **之前**绑定恢复效果：启动流程里的
            // `recover_registered()` 会用它在崩溃/中断后完成上一次恢复的收尾，
            // 晚绑定会让状态卡在 recovery_required，之后所有设置改动都被拒绝。
            if let Err(error) = runtime_parts
                .backend
                .bind_restore_runtime_effects(std::sync::Arc::new(LinuxRestoreEffects))
            {
                eprintln!("[linux-web] 恢复效果绑定失败：{error}");
            }
            // 必须显式 `start()`：core 的 `running` 标志不置位时，
            // 所有会改状态的命令（start_dictation 等）都会返回
            // "backend is not started" —— 表现就是「按快捷键没反应」。
            match runtime.block_on(runtime_parts.backend.start()) {
                Ok(_) => println!("[linux-web] 后端已启动"),
                Err(error) => eprintln!("[linux-web] 后端启动失败：{error}"),
            }
            // E2EE 云同步的会话只在「真正连一次」之后才会恢复：纯 `status()`
            // 在冷启动永远是 signed_out（界面据此提示「请重新登录」）。
            // 这里启动后连一次（未启用同步时只是握手，不会上传），
            // 让状态、账号、hasCloudSnapshot 等字段从一开始就是真实的。
            // 与 Tauri 对齐：启动后只做 ensure_runtime_ready（恢复围栏检查）
            // 和 remote_input 配置；**不主动 sync_now**。
            match runtime_parts.backend.ensure_runtime_ready() {
                Ok(()) => {}
                Err(error) => eprintln!("[linux-web] 运行时就绪检查未通过：{error}"),
            }
            {
                let preferences = runtime_parts.backend.get_preferences();
                let remote = std::sync::Arc::clone(&runtime_parts.backend);
                runtime.spawn(async move {
                    if let Err(error) = remote
                        .services()
                        .remote_input
                        .configure(openless_core::RemoteInputConfig {
                            enabled: preferences.remote_input_enabled,
                            port: preferences.remote_input_port,
                        })
                        .await
                    {
                        if error.code != openless_core::BackendErrorCode::Unsupported {
                            eprintln!("[linux-web] 远程输入启动失败：{error}");
                        }
                    }
                });
            }
            host.backend = Some(runtime_parts.backend);
            host.settings_runtime = Some(runtime_parts.settings_runtime);
            host.host_actions = Some(runtime_parts.host_actions);
        }
        Err(error) => eprintln!("[linux-web] 后端装配失败：{error}"),
    }
    host.runtime = Some(runtime);
    host
}


/// 读偏好 → 改一个字段 → 用 core 的 update_settings 写回（revision 冲突检测）。
fn persist_preferences(
    host: &Host,
    backend: &openless_core::OpenLessBackend,
    mutate: impl FnOnce(&mut openless_core::UserPreferences),
) -> Result<(), String> {
    let runtime_settings = host
        .settings_runtime
        .as_deref()
        .ok_or_else(|| "设置运行时未就绪".to_string())?;
    let mut draft = backend.get_preferences();
    mutate(&mut draft);
    let revision = backend.snapshot().preferences_revision;
    backend
        .update_settings(
            draft,
            openless_core::SettingsUpdateOptions {
                expected_preferences_revision: Some(revision),
                ..openless_core::SettingsUpdateOptions::STRICT
            },
            runtime_settings,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn shortcut_arg(args: &Value, optional: bool) -> Result<Option<openless_core::shared_types::ShortcutBinding>, String> {
    let raw = args.get("binding").cloned().unwrap_or(Value::Null);
    if raw.is_null() {
        return if optional {
            Ok(None)
        } else {
            Err("缺少参数 binding".to_string())
        };
    }
    let binding: openless_core::shared_types::ShortcutBinding =
        serde_json::from_value(raw).map_err(|error| error.to_string())?;
    openless_core::validate_shortcut_binding(&binding)
        .map_err(|error| format!("{error:?}"))?;
    Ok(Some(binding))
}

/// 在数据目录下按 `<session>.wav` 找归档录音（录音目录名可能随版本变，所以扫一遍）。
fn find_recording(data_dir: &std::path::Path, session_id: &str) -> Option<PathBuf> {
    let wanted = format!("{session_id}.wav");
    let mut stack = vec![data_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(wanted.as_str()) {
                return Some(path);
            }
        }
    }
    None
}

/// 不引入 base64 crate：录音回放只需要标准字母表 + padding。
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 解析 `set_combo_hotkey` / `validate_combo_hotkey` 的 binding 参数。
fn combo_binding(
    args: &Value,
) -> Result<openless_core::shared_types::ComboBinding, String> {
    serde_json::from_value(args.get("binding").cloned().unwrap_or(Value::Null))
        .map_err(|error| error.to_string())
}

/// 组合键校验：规则来自 `openless_core::validate_shortcut_binding`（与设置页、
/// Tauri 侧同一套），这里额外拒绝「只有修饰键」的组合——那是修饰键触发的领域，
/// 不该作为组合键保存。
fn validate_combo(
    binding: &openless_core::shared_types::ComboBinding,
) -> Result<(), String> {
    const MODIFIERS: [&str; 8] = [
        "shift", "ctrl", "control", "alt", "option", "super", "meta", "win",
    ];
    let primary = binding.primary.trim();
    if primary.is_empty() {
        return Err("组合键缺少主键".to_string());
    }
    if MODIFIERS.iter().any(|name| primary.eq_ignore_ascii_case(name)) {
        return Err(format!("「{primary}」是修饰键，需要再配一个主键"));
    }
    let shortcut = openless_core::shared_types::ShortcutBinding {
        primary: binding.primary.clone(),
        modifiers: binding.modifiers.clone(),
    };
    // 与 Tauri 的 `validate_combo_hotkey` 同序：先拒绝裸 Shift（那是修饰键触发的
    // 领域），再走 core 的通用校验。
    openless_core::reject_bare_shift_dictation_shortcut(&shortcut)
        .map_err(|error| error.to_string())?;
    openless_core::validate_shortcut_binding(&shortcut).map_err(|error| error.to_string())
}

fn arg_bool(args: &Value, key: &str) -> Result<bool, String> {
    args.get(key)
        .and_then(|value| value.as_bool())
        .ok_or_else(|| format!("缺少参数 {key}"))
}

/// `PolishMode` 的 serde 名字（raw / light / structured / formal）。
fn polish_mode(args: &Value) -> Result<openless_core::PolishMode, String> {
    let raw = args
        .get("mode")
        .cloned()
        .ok_or_else(|| "缺少参数 mode".to_string())?;
    serde_json::from_value(raw).map_err(|error| format!("未知的润色模式：{error}"))
}

/// 从 CDN 拉取更新清单（与 Tauri updater 同一份数据源）。
///
/// 返回给前端的形状贴近 Tauri 的 `Update`：`version` / `notes` / `pubDate` / `url`，
/// 外加 `currentVersion` 与 `newer`（前端据此决定是否提示）。
async fn fetch_update_manifest(
    channel: openless_linux_host::update_check::UpdateChannel,
) -> Result<Option<Value>, String> {
    let (path, suffix) = match channel {
        openless_linux_host::update_check::UpdateChannel::Stable => ("stable", ""),
        openless_linux_host::update_check::UpdateChannel::Beta => ("beta", "-beta"),
    };
    let url = format!(
        "https://dl.aeoform.com/updates/{path}/latest-linux-x86_64{suffix}.json"
    );
    let response = openless_core::net::send_with_retry(|| {
        openless_core::net::http()
            .get(&url)
            .timeout(std::time::Duration::from_secs(15))
    })
    .await
    .map_err(|error| error.to_string())?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("更新清单: HTTP {}", response.status()));
    }
    let body: Value = response.json().await.map_err(|error| error.to_string())?;
    let Some(version) = body.get("version").and_then(|value| value.as_str()) else {
        return Ok(None);
    };
    let current = openless_linux_host::update_check::app_version();
    Ok(Some(json!({
        "version": version,
        "notes": body.get("notes").cloned().unwrap_or(Value::Null),
        "pubDate": body.get("pub_date").cloned().unwrap_or(Value::Null),
        "url": format!("https://github.com/Open-Less/openless/releases/tag/v{version}"),
        "currentVersion": current,
        "newer": openless_linux_host::update_check::is_newer(current, version),
    })))
}

/// QA 面板形态切换：宿主只记录，窗口层（layer-shell）按它调整尺寸。
fn set_qa_expanded(host: &Host, expanded: bool) {
    *host.qa_expanded.lock().unwrap() = expanded;
}

impl Host {
    /// 云同步状态：250ms 内的重复请求直接返回缓存。
    fn cloud_sync_status_cached(&self, backend: &openless_core::OpenLessBackend) -> Value {
        const TTL: std::time::Duration = std::time::Duration::from_millis(250);
        let mut slot = self
            .cloud_sync_status_cache
            .lock()
            .expect("cloud sync status cache lock poisoned");
        if let Some((at, value)) = slot.as_ref() {
            if at.elapsed() < TTL {
                return value.clone();
            }
        }
        let value = match backend.cloud_sync_e2ee_status() {
            Ok(status) => json_value(status),
            Err(error) => json!({ "error": error.to_string() }),
        };
        *slot = Some((std::time::Instant::now(), value.clone()));
        value
    }
}

/// 需要 GTK 主线程执行的动作（`Host` 本身不能持有 GTK 对象）。

/// 把键盘焦点交回主窗口。
fn focus_main_window(_host: &Host) {
    // 窗口归 Qt 宿主所有；前端要「回到主窗口」时由宿主自行处理。
}

fn json_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("缺少参数 {key}"))
}

fn channel_kind(kind: Option<&str>) -> Option<openless_core::ChannelKind> {
    match kind {
        Some("asr") => Some(openless_core::ChannelKind::Asr),
        Some("llm") => Some(openless_core::ChannelKind::Llm),
        _ => None,
    }
}

fn dispatch(host: &Host, command: &str, _args: &Value) -> Result<Value, String> {
    let dir = data_dir();
    let backend = host.backend.as_deref();
    match command {
        "get_startup_snapshot" => Ok(json!({
            "contractVersion": "2.0.0",
            "backend": { "running": backend.is_some() }
        })),
        "get_settings" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(serde_json::to_value(backend.get_preferences()).unwrap_or(Value::Null))
        }
        "get_settings_snapshot" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(json!({
                "preferences": serde_json::to_value(backend.get_preferences()).unwrap_or(Value::Null),
                "revision": backend.snapshot().preferences_revision,
            }))
        }
        "get_credentials" => {
            let (backend, runtime) = match (backend, host.runtime.as_ref()) {
                (Some(backend), Some(runtime)) => (backend, runtime),
                _ => return Err("后端未就绪".to_string()),
            };
            let status = runtime
                .block_on(backend.get_credentials_status())
                .map_err(|error| error.to_string())?;
            Ok(serde_json::to_value(status).unwrap_or(Value::Null))
        }
        // 设置写回：走 core 的 update_settings（带 revision 冲突检测），
        // 运行时效果（开机自启 / 热键 / 渠道切换）由 LinuxSettingsRuntime 负责。
        "set_settings" | "save_settings" | "update_setting_fields" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host
                .settings_runtime
                .as_deref()
                .ok_or_else(|| "设置运行时未就绪".to_string())?;
            let edits: std::collections::BTreeMap<String, Value> = _args
                .get("edits")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default();
            let revision = backend.snapshot().preferences_revision;
            let draft = openless_core::preference_patch::patch_preferences(
                &backend.get_preferences(),
                &edits,
            )
            .map_err(|error| error.to_string())?;
            backend
                .update_settings(
                    draft,
                    openless_core::SettingsUpdateOptions {
                        expected_preferences_revision: Some(revision),
                        ..openless_core::SettingsUpdateOptions::STRICT
                    },
                    runtime,
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "preferences": serde_json::to_value(backend.get_preferences()).unwrap_or(Value::Null),
                "revision": backend.snapshot().preferences_revision,
            }))
        }
        "list_history" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let entries = backend.list_history().map_err(|error| error.to_string())?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "get_activity_stats" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let days = backend
                .list_activity()
                .map_err(|error| error.to_string())?;
            serde_json::to_value(days).map_err(|error| error.to_string())
        }
        "list_provider_descriptors" => {
            let kind = match _args.get("kind").and_then(|value| value.as_str()) {
                Some("asr") => openless_core::ProviderKind::Asr,
                Some("llm") => openless_core::ProviderKind::Llm,
                Some("omni") => openless_core::ProviderKind::Omni,
                _ => return Err("kind 必须是 asr / llm".to_string()),
            };
            serde_json::to_value(openless_core::provider_rules::provider_descriptors(kind))
                .map_err(|error| error.to_string())
        }
        // ── 渠道 ───────────────────────────────────────────────────────
        "list_channels" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let channels = runtime
                .block_on(backend.list_channels(kind))
                .map_err(|error| error.to_string())?;
            serde_json::to_value(channels).map_err(|error| error.to_string())
        }
        "create_channel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let name = _args
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("新渠道")
                .to_string();
            let provider_type = _args
                .get("providerType")
                .or_else(|| _args.get("provider_type"))
                .and_then(|value| value.as_str())
                .unwrap_or("custom")
                .to_string();
            let channel = runtime
                .block_on(backend.create_channel(kind, name, provider_type))
                .map_err(|error| error.to_string())?;
            serde_json::to_value(channel).map_err(|error| error.to_string())
        }
        "delete_channel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let id = _args
                .get("id")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 id".to_string())?
                .to_string();
            runtime
                .block_on(backend.delete_channel(kind, id))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "deleted": true }))
        }
        "set_channel_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone();
            let runtime = runtime.ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let id = _args
                .get("id")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 id".to_string())?
                .to_string();
            let enabled = _args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            runtime
                .block_on(backend.set_channel_enabled(kind, id, enabled))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "enabled": enabled }))
        }
        // ── 凭据读写（映射表在 core: credential_accounts::credential_key）──
        "read_credential" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let account = _args
                .get("account")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 account".to_string())?;
            let provider = _args
                .get("provider")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let key = openless_core::credential_accounts::credential_key(account, provider)
                .map_err(|error| error.to_string())?;
            let value = runtime
                .block_on(backend.read_credential(key))
                .map_err(|error| error.to_string())?;
            Ok(json!(value.map(|secret| secret.into_exposed())))
        }
        "set_credential" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let account = _args
                .get("account")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 account".to_string())?;
            let provider = _args
                .get("provider")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let value = _args
                .get("value")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let key = openless_core::credential_accounts::credential_key(account, provider)
                .map_err(|error| error.to_string())?;
            runtime
                .block_on(backend.set_credential(
                    key,
                    openless_core::SecretValue::new(value),
                ))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "saved": true }))
        }
        "validate_provider_credentials" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = _args
                .get("kind")
                .and_then(|value| value.as_str())
                .and_then(|kind| match kind {
                    "asr" => Some(openless_core::ProviderKind::Asr),
                    "llm" => Some(openless_core::ProviderKind::Llm),
                    "omni" => Some(openless_core::ProviderKind::Omni),
                    _ => None,
                })
                .ok_or_else(|| "kind 必须是 asr / llm / omni".to_string())?;
            let channel_id = _args
                .get("channelId")
                .or_else(|| _args.get("channel_id"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let thinking_enabled = backend.get_preferences().llm_thinking_enabled;
            let result = runtime
                .block_on(backend.services().provider.validate(openless_core::ProviderRequest {
                    kind,
                    channel_id,
                    thinking_enabled,
                }))
                .map_err(|error| {
                    error
                        .details
                        .as_ref()
                        .and_then(|details| details.get("siwc"))
                        .map(|details| format!("siwc:{details}"))
                        .unwrap_or(error.message)
                })?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        }
        // ── 词库 / 纠错规则（core 全是同步接口）────────────────────────
        "list_vocab" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            serde_json::to_value(backend.list_vocabulary().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        }
        "add_vocab" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let phrase = required_str(_args, "phrase")?;
            let note = _args.get("note").and_then(|value| value.as_str()).map(str::to_string);
            serde_json::to_value(
                backend.add_vocabulary(phrase.to_string(), note).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "remove_vocab" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend
                .remove_vocabulary(&required_str(_args, "id")?)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "removed": true }))
        }
        "update_vocab" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend
                .update_vocabulary_phrase(
                    required_str(_args, "id")?,
                    required_str(_args, "phrase")?.to_string(),
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({ "updated": true }))
        }
        "list_vocab_presets" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            serde_json::to_value(
                backend.list_vocabulary_presets().map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "save_vocab_presets" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let store: openless_core::VocabPresetStore =
                serde_json::from_value(_args.get("store").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            backend
                .save_vocabulary_presets(&store)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "saved": true }))
        }
        "list_correction_rules" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            serde_json::to_value(
                backend.list_correction_rules().map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "add_correction_rule" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let rule = backend
                .add_correction_rule(
                    required_str(_args, "pattern")?.to_string(),
                    required_str(_args, "replacement")?.to_string(),
                )
                .map_err(|error| error.to_string())?;
            serde_json::to_value(rule).map_err(|error| error.to_string())
        }
        "remove_correction_rule" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend
                .remove_correction_rule(&required_str(_args, "id")?)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "removed": true }))
        }
        "set_correction_rule_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let enabled = _args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            backend
                .set_correction_rule_enabled(&required_str(_args, "id")?, enabled)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "enabled": enabled }))
        }
        "accept_pending_correction" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend
                .accept_pending_correction(&required_str(_args, "id")?)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "accepted": true }))
        }
        "reject_pending_correction" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend.reject_pending_correction(&required_str(_args, "id")?);
            Ok(json!({ "rejected": true }))
        }
        "dismiss_vocab_suggestions" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend.dismiss_pending_corrections();
            Ok(json!({ "dismissed": true }))
        }
        // ── 风格包 ─────────────────────────────────────────────────────
        "list_style_packs" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let active = backend.get_preferences().active_style_pack_id;
            serde_json::to_value(
                backend
                    .list_style_packs(&active)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "save_style_pack" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let pack: openless_core::StylePack =
                serde_json::from_value(_args.get("stylePack").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            serde_json::to_value(
                backend
                    .update_style_pack(pack)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "delete_style_pack" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend
                .remove_style_pack(&required_str(_args, "id")?)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "removed": true }))
        }
        "set_style_pack_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let enabled = _args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            serde_json::to_value(
                backend
                    .set_style_pack_enabled(&required_str(_args, "id")?, enabled)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "reset_builtin_style_pack" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            serde_json::to_value(
                backend
                    .reset_builtin_style_pack(&required_str(_args, "id")?)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "preview_style_pack_runtime" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let pack: openless_core::StylePack =
                serde_json::from_value(_args.get("stylePack").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            serde_json::to_value(backend.preview_style_pack_runtime(&pack))
                .map_err(|error| error.to_string())
        }
        "get_default_style_system_prompts" => {
            serde_json::to_value(openless_core::StyleSystemPrompts::default())
                .map_err(|error| error.to_string())
        }
        "import_style_pack_from_zip" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let path = std::path::PathBuf::from(required_str(_args, "zipPath")?);
            serde_json::to_value(
                backend
                    .import_style_pack_path(&path)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())
        }
        "export_style_pack_to_zip" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let path = std::path::PathBuf::from(required_str(_args, "targetPath")?);
            backend
                .export_style_pack_path(&required_str(_args, "id")?, &path)
                .map_err(|error| error.to_string())?;
            Ok(json!(path.to_string_lossy()))
        }
        "set_style_pack_hotkeys" => {
            // 与 Tauri 一致：写回偏好里的 style_pack_hotkeys（冲突检测由 core 负责）。
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime_settings = host
                .settings_runtime
                .as_deref()
                .ok_or_else(|| "设置运行时未就绪".to_string())?;
            let hotkeys: Vec<openless_core::shared_types::StylePackHotkey> =
                serde_json::from_value(_args.get("hotkeys").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let mut draft = backend.get_preferences();
            draft.style_pack_hotkeys = hotkeys;
            let revision = backend.snapshot().preferences_revision;
            backend
                .update_settings(
                    draft,
                    openless_core::SettingsUpdateOptions {
                        expected_preferences_revision: Some(revision),
                        ..openless_core::SettingsUpdateOptions::STRICT
                    },
                    runtime_settings,
                )
                .map_err(|error| error.to_string())?;
            Ok(json!({ "saved": true }))
        }
        // ── 听写 / 录音 ────────────────────────────────────────────────
        "start_dictation" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            runtime
                .block_on(backend.start_dictation())
                .map_err(|error| error.to_string())?;
            Ok(json!({ "started": true }))
        }
        "stop_dictation" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            runtime
                .block_on(backend.stop_dictation())
                .map_err(|error| error.to_string())?;
            Ok(json!({ "stopped": true }))
        }
        "cancel_dictation" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let session = backend.snapshot().dictation.session_id;
            runtime
                .block_on(backend.cancel_dictation(session))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "cancelled": true }))
        }
        "list_microphone_devices" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let devices = runtime
                .block_on(backend.services().platform.microphone_devices())
                .map_err(|error| error.to_string())?;
            serde_json::to_value(devices).map_err(|error| error.to_string())
        }
        "record_channel_test" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|v| v.as_str()))
                .ok_or_else(|| "kind 必须是 asr / llm".to_string())?;
            let id = required_str(_args, "id")?.to_string();
            let ok = _args.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            let latency_ms = _args
                .get("latencyMs")
                .or_else(|| _args.get("latency_ms"))
                .and_then(|v| v.as_u64())
                .map(|value| value as u32);
            let error = _args
                .get("error")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            host.runtime
                .clone()
                .ok_or_else(|| "运行时未就绪".to_string())?
                .block_on(backend.record_channel_test(kind, id, ok, latency_ms, error))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "recorded": true }))
        }
        // ── 热键 ──────────────────────────────────────────────────────
        "set_dictation_hotkey" | "set_qa_hotkey" | "set_quick_note_hotkey"
        | "set_selection_polish_hotkey" | "set_translation_hotkey"
        | "set_switch_style_hotkey" | "set_open_app_hotkey" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let optional = command != "set_dictation_hotkey" && command != "set_translation_hotkey";
            let binding = shortcut_arg(_args, optional)?;
            persist_preferences(host, backend, |prefs| match command {
                "set_dictation_hotkey" => {
                    if let Some(binding) = binding.clone() {
                        prefs.dictation_hotkey = binding;
                    }
                }
                "set_qa_hotkey" => prefs.qa_hotkey = binding.clone(),
                "set_quick_note_hotkey" => prefs.quick_note_hotkey = binding.clone(),
                "set_selection_polish_hotkey" => prefs.selection_polish_hotkey = binding.clone(),
                "set_translation_hotkey" => {
                    if let Some(binding) = binding.clone() {
                        prefs.translation_hotkey = binding;
                    }
                }
                "set_switch_style_hotkey" => prefs.switch_style_hotkey = binding.clone(),
                "set_open_app_hotkey" => prefs.open_app_hotkey = binding.clone(),
                _ => {}
            })?;
            Ok(json!({ "saved": true }))
        }
        // ── 云同步（sync / async 混用：同步直接调，async 用 block_on）─────
        "cloud_sync_e2ee_status" => {
            // 只读缓存 + 250ms 去重：界面会高频轮询这个命令（实测 12 分钟
            // 1.8 万次），既不做网络调用，也不重复序列化。
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(host.cloud_sync_status_cached(backend))
        }
        "cloud_sync_e2ee_cancel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(json_value(
                backend
                    .cloud_sync_e2ee_cancel(required_str(_args, "taskId")?.to_string())
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_claim_setup_prompt"
        | "cloud_sync_e2ee_lock"
        | "cloud_sync_e2ee_sync_now"
        | "cloud_sync_e2ee_sign_out"
        | "cloud_sync_e2ee_sign_in_with_token"
        | "cloud_sync_e2ee_begin_sign_in"
        | "cloud_sync_e2ee_get_ui_preferences"
        | "cloud_sync_e2ee_get_ui_preferences_snapshot"
        | "cloud_sync_status"
        | "cloud_sync_restore" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            match command {
                "cloud_sync_e2ee_claim_setup_prompt" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_claim_setup_prompt())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_lock" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_lock())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_sync_now" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_sync_now())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_sign_out" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_sign_out())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_sign_in_with_token" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_sign_in_with_token())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_begin_sign_in" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_begin_sign_in())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_get_ui_preferences" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_get_ui_preferences())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_e2ee_get_ui_preferences_snapshot" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_get_ui_preferences_snapshot())
                        .map_err(|error| error.to_string())?,
                )),
                "cloud_sync_status" => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_status())
                        .map_err(|error| error.to_string())?,
                )),
                _ => Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_restore())
                        .map_err(|error| error.to_string())?,
                )),
            }
        }
        "cloud_sync_e2ee_prepare_enable" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let consent = required_str(_args, "consentVersion")?.to_string();
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_prepare_enable(consent))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_create" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let password = required_str(_args, "password")?.to_string();
            let confirmation = required_str(_args, "passwordConfirmation")?.to_string();
            let remember = _args.get("rememberKey").and_then(|v| v.as_bool()).unwrap_or(false);
            let consent = required_str(_args, "consentVersion")?.to_string();
            let observed_revision = required_str(_args, "observedRevision")?.to_string();
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_create(
                        password,
                        confirmation,
                        remember,
                        consent,
                        observed_revision,
                    ))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_unlock" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let password = required_str(_args, "password")?.to_string();
            let remember = _args.get("rememberKey").and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_unlock(password, remember))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_set_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let enabled = _args
                .get("enabled")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| "缺少参数 enabled".to_string())?;
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_set_enabled(enabled))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_preview_restore" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let revision = required_str(_args, "observedRevision")?.to_string();
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_preview_restore(revision))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_change_password" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let current = required_str(_args, "currentPassword")?.to_string();
            let next = required_str(_args, "newPassword")?.to_string();
            let confirmation = required_str(_args, "confirmation")?.to_string();
            let remember = _args.get("rememberKey").and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_change_password(
                        current,
                        next,
                        confirmation,
                        remember,
                    ))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_delete_remote" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let vault = required_str(_args, "expectedVaultId")?.to_string();
            let revision = required_str(_args, "observedRevision")?.to_string();
            let confirmed = _args.get("confirmed").and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_delete_remote(vault, revision, confirmed))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_configure_server" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let origin = _args.get("origin").and_then(|v| v.as_str()).map(str::to_string);
            let token = required_str(_args, "token")?.to_string();
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_configure_server(origin, token))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_e2ee_poll_sign_in" | "cloud_sync_e2ee_cancel_sign_in" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let session = required_str(_args, "authorizationSessionId")?.to_string();
            if command == "cloud_sync_e2ee_poll_sign_in" {
                Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_poll_sign_in(session))
                        .map_err(|error| error.to_string())?,
                ))
            } else {
                Ok(json_value(
                    runtime
                        .block_on(backend.cloud_sync_e2ee_cancel_sign_in(session))
                        .map_err(|error| error.to_string())?,
                ))
            }
        }
        "cloud_sync_e2ee_set_ui_preferences_checked" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let locale = required_str(_args, "locale")?.to_string();
            let font_scale = required_str(_args, "fontScale")?.to_string();
            let expected = _args
                .get("expectedRevision")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_e2ee_set_ui_preferences_checked(
                        locale,
                        font_scale,
                        expected,
                    ))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "cloud_sync_delete" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let revision = _args
                .get("baseRevision")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| "缺少参数 baseRevision".to_string())?;
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_delete(revision))
                    .map_err(|error| error.to_string())?,
            ))
        }
        // ── 杂项：剪贴板 / 网络 / 本地 IP / 日志 / 重启 ────────────────
        "copy_text_to_clipboard" => {
            let text = _args
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if !text.is_empty() {
                openless_linux_host::fcitx5_copy_to_clipboard(text)
                    .map_err(|error| error.to_string())?;
                println!("[linux-web] 已复制到剪贴板（{} 字符）", text.chars().count());
            }
            Ok(json!({ "copied": !text.is_empty() }))
        }
        "check_network" => {
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let (state, latency) =
                runtime.block_on(openless_linux_host::linux_access::probe_network());
            Ok(json!({
                "online": matches!(state, openless_linux_host::linux_access::CheckState::Ready),
                "latencyMs": latency,
            }))
        }
        "list_local_ips" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let ips = runtime
                .block_on(backend.services().remote_input.list_local_ips())
                .map_err(|error| error.message)?;
            Ok(json_value(ips))
        }
        // 诊断页的两个按钮：导出日志 + 探测光标上下文。
        "export_error_log" => {
            // 宿主没有原生保存对话框：文件名给后端，直接落到 ~/Downloads。
            let mut file_name = required_str(_args, "fileName")?.to_string();
            file_name.retain(|c| c != '/' && c != '\\');
            if file_name.trim().is_empty() {
                file_name = String::from("openless.log");
            }
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let downloads = PathBuf::from(home).join("Downloads");
            std::fs::create_dir_all(&downloads)
                .map_err(|error| format!("创建 Downloads 目录失败：{error}"))?;
            let destination = downloads.join(file_name);
            let dir = data_dir();
            openless_linux_host::export_error_logs(&dir, &destination)
                .map_err(|error| format!("{error}"))?;
            Ok(json!({ "written": destination.display().to_string() }))
        }
        "debug_read_cursor_context" => {
            // 用 fcitx5 的 surrounding text 探一下焦点应用能不能读到文本。
            let started = std::time::Instant::now();
            match openless_linux_host::fcitx5_surrounding_text() {
                Ok(Some(surrounding)) => Ok(json!({
                    "status": "ok",
                    "reason": null,
                    "window": {
                        "text": surrounding.text,
                        "cursor": surrounding.cursor,
                    },
                    "appName": surrounding.program,
                    "bundleId": null,
                    "elapsedMs": started.elapsed().as_millis() as u64,
                })),
                Ok(None) => Ok(json!({
                    "status": "unsupported",
                    "reason": "应用没有提供光标上下文（fcitx5 surrounding text 不可用）",
                    "window": null,
                    "appName": null,
                    "bundleId": null,
                    "elapsedMs": started.elapsed().as_millis() as u64,
                })),
                Err(error) => Ok(json!({
                    "status": "unavailable",
                    "reason": error.to_string(),
                    "window": null,
                    "appName": null,
                    "bundleId": null,
                    "elapsedMs": started.elapsed().as_millis() as u64,
                })),
            }
        }
        "log_client_error" => {
            // 页面报错只进日志，不回传任何用户内容。
            let message = _args
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("(no message)");
            eprintln!("[linux-web][client] {message}");
            Ok(Value::Null)
        }
        "is_wayland_cli_mode" => Ok(json!(
            std::env::var_os("WAYLAND_DISPLAY").is_some()
        )),
        "restart_app" => {
            // 与 Tauri 版同语义：退出由 systemd/autostart 拉起；这里直接退出，
            // 进程管理交给单元文件（`app-openless@autostart.service` 的 Restart 策略）。
            println!("[linux-web] restart requested");
            std::process::exit(0);
        }
        // ── 录音回放 / 重新转录 ───────────────────────────────────────
        "read_audio_recording" => {
            let session_id = required_str(_args, "sessionId")?.to_string();
            let path = find_recording(&data_dir(), &session_id)
                .ok_or_else(|| "找不到这次录音".to_string())?;
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            Ok(json!(format!(
                "data:audio/wav;base64,{}",
                base64_encode(&bytes)
            )))
        }
        "retranscribe_recording" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let session_id = required_str(_args, "sessionId")?.to_string();
            let entry = backend
                .list_history()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|entry| entry.id == session_id)
                .ok_or_else(|| "history entry not found".to_string())?;
            if backend
                .snapshot()
                .dictation
                .session_id
                .is_some_and(|active| active.to_string() == session_id)
            {
                return Err("录音还在进行，先停掉再重新转录".to_string());
            }
            if entry.has_audio_recording != Some(true) {
                return Err("这条历史没有归档录音".to_string());
            }
            let path = find_recording(&data_dir(), &session_id)
                .ok_or_else(|| "找不到这次录音".to_string())?;
            let wav = std::fs::read(&path).map_err(|error| error.to_string())?;
            // 归档 wav 是 16k/mono/16bit + 固定 44 字节标准头（见 asr::wav）。
            if wav.len() <= 44 {
                return Err("录音为空或损坏".to_string());
            }
            let pcm = wav[44..].to_vec();
            let started = std::time::Instant::now();
            let retranscription = runtime
                .block_on(backend.services().auxiliary.retranscribe_pcm(pcm))
                .map_err(|error| error.into_message())?;
            let text = retranscription.text;
            let asr = retranscription.asr;
            if text.trim().is_empty() {
                return Err("重新转录仍未识别到语音".to_string());
            }
            let elapsed = started.elapsed().as_millis() as u64;
            let _ = asr; // AsrCallLabel 没有 Serialize：前端用不到它
            Ok(json!({
                "text": text,
                "elapsedMs": elapsed,
            }))
        }
        // ── 服务用量 ──────────────────────────────────────────────────
        "get_service_usage" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let recorder = backend
                .services()
                .usage_recorder
                .clone()
                .ok_or_else(|| "服务用量不可用".to_string())?;
            let (records, dropped) = recorder.snapshot().map_err(|error| error.to_string())?;
            Ok(json!({ "records": records, "droppedOrFailed": dropped }))
        }
        "clear_service_usage" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let recorder = backend
                .services()
                .usage_recorder
                .clone()
                .ok_or_else(|| "服务用量不可用".to_string())?;
            recorder.clear().map_err(|error| error.to_string())?;
            Ok(json!({ "cleared": true }))
        }
        // ── 自定义组合键（校验规则在 Tauri 侧，Linux 先只落盘）─────────
        "set_combo_hotkey" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let binding = combo_binding(_args)?;
            validate_combo(&binding)?;
            let binding = Some(binding);
            persist_preferences(host, backend, |prefs| {
                prefs.custom_combo_hotkey = binding;
            })?;
            Ok(json!({ "saved": true }))
        }
        // ── 风格包激活（Tauri 里还会刷托盘菜单，这里同样刷一次）─────
        "set_active_style_pack" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let id = required_str(_args, "id")?.to_string();
            let pack = backend
                .activate_style_pack(&id)
                .map_err(|error| error.to_string())?;
            // 托盘菜单不缓存「当前风格包」（`LinuxTray` 在下一次风格/麦克风/语言
            // 变更时重建），所以这里不需要额外刷新调用。
            Ok(json_value(pack))
        }
        "read_style_pack_icon" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let id = required_str(_args, "id")?.to_string();
            Ok(json_value(
                backend
                    .read_style_pack_icon(&id)
                    .map_err(|error| error.to_string())?,
            ))
        }
        "reset_accessibility_permission_and_restart_app" => {
            // Linux 没有辅助功能授权这个概念（Tauri 那条是 macOS 专用的
            // TCC 重置）。这里明确返回「不适用」，而不是假装成功。
            Err("Linux 没有辅助功能授权流程".to_string())
        }
        // ── 组合键校验（规则在 core 的 validate_shortcut_binding）──────
        "validate_combo_hotkey" => {
            let binding = combo_binding(_args)?;
            validate_combo(&binding)?;
            Ok(json!({ "valid": true }))
        }
        "cloud_sync_e2ee_apply_restore" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let preview_id = required_str(_args, "previewId")?.to_string();
            let mode: openless_core::cloud_sync_e2ee::RestoreMode =
                serde_json::from_value(_args.get("mode").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let choices: Vec<openless_core::cloud_sync_e2ee::SyncConflictChoice> =
                serde_json::from_value(
                    _args.get("conflictChoices").cloned().unwrap_or_else(|| json!([])),
                )
                .map_err(|error| error.to_string())?;
            Ok(json_value(
                runtime
                    .block_on(
                        backend.cloud_sync_e2ee_apply_restore(preview_id, mode, choices),
                    )
                    .map_err(|error| error.to_string())?,
            ))
        }
        // ── 历史 / 词表 / 风格包 / 频道的变更命令 ─────────────────────
        "clear_history" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            if backend.snapshot().dictation.session_id.is_some() {
                return Err("录音进行中，先停止再清空历史".to_string());
            }
            let entries = backend.list_history().map_err(|error| error.to_string())?;
            backend.clear_history().map_err(|error| error.to_string())?;
            // 归档录音一并删掉，否则数据目录会留下无主 wav。
            for entry in entries {
                if let Some(path) = find_recording(&data_dir(), &entry.id) {
                    let _ = std::fs::remove_file(path);
                }
            }
            Ok(json!({ "cleared": true }))
        }
        "delete_history_entry" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let id = required_str(_args, "id")?.to_string();
            if backend
                .snapshot()
                .dictation
                .session_id
                .is_some_and(|active| active.to_string() == id)
            {
                return Err("这条正在录音，先停止或取消再删除".to_string());
            }
            backend.delete_history(&id).map_err(|error| error.to_string())?;
            if let Some(path) = find_recording(&data_dir(), &id) {
                let _ = std::fs::remove_file(path);
            }
            Ok(json!({ "deleted": true }))
        }
        "set_vocab_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let id = required_str(_args, "id")?.to_string();
            let enabled = arg_bool(_args, "enabled")?;
            backend
                .set_vocabulary_enabled(&id, enabled)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "enabled": enabled }))
        }
        "set_style_enabled" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let mode = polish_mode(_args)?;
            let enabled = arg_bool(_args, "enabled")?;
            let pack_id = openless_core::style_packs::builtin_style_pack_id(mode);
            backend
                .set_style_pack_enabled(pack_id, enabled)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "enabled": enabled }))
        }
        "set_default_polish_mode" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let mode = polish_mode(_args)?;
            let pack_id = openless_core::style_packs::builtin_style_pack_id(mode);
            let pack = backend
                .activate_style_pack(pack_id)
                .map_err(|error| error.to_string())?;
            Ok(json_value(pack))
        }
        "set_style_pack_icon" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let id = required_str(_args, "id")?.to_string();
            let png: Option<Vec<u8>> = match _args.get("png") {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    serde_json::from_value(value.clone())
                        .map_err(|error| format!("png 参数不是字节数组：{error}"))?,
                ),
            };
            Ok(json_value(
                backend
                    .set_style_pack_icon(&id, png.as_deref())
                    .map_err(|error| error.to_string())?,
            ))
        }
        "rename_channel" | "reorder_channels" | "set_channel_provider_type" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|value| value.as_str()))
                .ok_or_else(|| "未知的频道类型".to_string())?;
            let result = match command {
                "rename_channel" => runtime.block_on(backend.rename_channel(
                    kind,
                    required_str(_args, "id")?.to_string(),
                    required_str(_args, "name")?.to_string(),
                )),
                "reorder_channels" => runtime.block_on(backend.reorder_channels(
                    kind,
                    serde_json::from_value(
                        _args.get("ids").cloned().unwrap_or_else(|| json!([])),
                    )
                    .map_err(|error| error.to_string())?,
                )),
                _ => runtime.block_on(backend.set_channel_provider_type(
                    kind,
                    required_str(_args, "id")?.to_string(),
                    required_str(_args, "providerType")?.to_string(),
                )),
            };
            result.map_err(|error| error.to_string())?;
            Ok(json!({ "ok": true }))
        }
        "list_provider_models" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let kind: openless_core::ProviderKind = match required_str(_args, "kind")? {
                "asr" => openless_core::ProviderKind::Asr,
                "llm" => openless_core::ProviderKind::Llm,
                "omni" => openless_core::ProviderKind::Omni,
                other => return Err(format!("未知的供应商类型：{other}")),
            };
            let thinking_enabled = backend.get_preferences().llm_thinking_enabled;
            Ok(json_value(
                runtime
                    .block_on(backend.services().provider.list_models(
                        openless_core::ProviderRequest {
                            kind,
                            thinking_enabled,
                            channel_id: _args
                                .get("channelId")
                                .and_then(|value| value.as_str())
                                .map(str::to_string),
                        },
                    ))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "validate_shortcut_binding" => {
            let binding = shortcut_arg(_args, false)?
                .ok_or_else(|| "缺少 binding".to_string())?;
            openless_core::validate_shortcut_binding(&binding)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "valid": true }))
        }
        // ── 偏好类小命令 + 少数纯搬运 ────────────────────────────────
        "get_qa_hotkey_label" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let prefs = backend.get_preferences();
            if prefs.qa_companion_key.is_empty() {
                return Ok(json!(""));
            }
            Ok(json!(format!(
                "{} + {}",
                prefs.dictation_hotkey.display_label(),
                prefs.qa_companion_key
            )))
        }
        "get_update_channel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let prefs = backend.get_preferences();
            // 与 Tauri 的 effective_update_channel 同规则：没显式设过时，
            // 版本号带 `-`（例如 2.0.2-Beta.3）就走 beta。
            let channel = if prefs.update_channel_explicit {
                prefs.update_channel
            } else if env!("CARGO_PKG_VERSION").contains('-') {
                openless_core::shared_types::UpdateChannel::Beta
            } else {
                openless_core::shared_types::UpdateChannel::Stable
            };
            Ok(json_value(channel))
        }
        "set_update_channel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let channel: openless_core::shared_types::UpdateChannel =
                serde_json::from_value(_args.get("channel").cloned().unwrap_or(Value::Null))
                    .map_err(|error| format!("未知的更新通道：{error}"))?;
            persist_preferences(host, backend, |prefs| {
                prefs.update_channel = channel;
                prefs.update_channel_explicit = true;
            })?;
            Ok(json_value(channel))
        }
        "open_system_settings" => {
            // Linux 没有 macOS 那种按面板直达的 URL 方案；能用的是桌面的设置入口。
            // 拿不到就静默失败（前端只是打开设置，不该因此报错打断流程）。
            let pane = _args.get("pane").and_then(|value| value.as_str()).unwrap_or("");
            let program = if pane == "microphone" { "pavucontrol" } else { "systemsettings" };
            let _ = std::process::Command::new(program)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            Ok(json!({ "opened": program }))
        }
        "delete_channel_if_blank" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let kind = channel_kind(_args.get("kind").and_then(|value| value.as_str()))
                .ok_or_else(|| "未知的频道类型".to_string())?;
            let deleted = runtime
                .block_on(backend.delete_channel_if_blank(
                    kind,
                    required_str(_args, "id")?.to_string(),
                ))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "deleted": deleted }))
        }
        "create_style_pack_from_template" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let template: openless_core::StylePack =
                serde_json::from_value(_args.get("template").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            Ok(json_value(
                backend
                    .create_style_pack(template)
                    .map_err(|error| error.to_string())?,
            ))
        }
        "get_expression_profile" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(json_value(
                backend
                    .expression_profile_directions()
                    .map_err(|error| error.to_string())?,
            ))
        }
        "clear_expression_profile" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            backend
                .clear_expression_profile()
                .map_err(|error| error.to_string())?;
            Ok(json!({ "cleared": true }))
        }
        "get_dictation_translation_target" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(json_value(backend.dictation_translation_target()))
        }
        "select_dictation_translation_target" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let session = required_str(_args, "sessionId")?;
            let session = uuid::Uuid::parse_str(session)
                .map(openless_core::SessionId::from_uuid)
                .map_err(|error| format!("session id 不合法：{error}"))?;
            let target = required_str(_args, "targetLanguage")?.to_string();
            Ok(json_value(
                backend
                    .select_dictation_translation_target(session, target)
                    .map_err(|error| error.to_string())?,
            ))
        }
        // ── 云同步上传 / 学习词 / 麦克风权限 / 远程输入 ────────────────
        "cloud_sync_upload" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let base_revision = _args
                .get("baseRevision")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| "缺少参数 baseRevision".to_string())?;
            let ui_preferences: openless_core::CloudSyncUiPreferences =
                serde_json::from_value(_args.get("uiPreferences").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            Ok(json_value(
                runtime
                    .block_on(backend.cloud_sync_upload(base_revision, ui_preferences))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "add_learned_vocab" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let phrase = required_str(_args, "phrase")?.to_string();
            backend
                .add_learned_vocabulary(phrase)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "added": true }))
        }
        "check_microphone_permission" | "request_microphone_permission" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let platform = &backend.services().platform;
            if command == "request_microphone_permission" {
                // Linux 没有系统级授权弹窗：申请即尝试打开一次采集设备。
                let _ = runtime.block_on(platform.request_microphone_permission());
            }
            match runtime.block_on(platform.microphone_permission()) {
                Ok(snapshot) => Ok(json!({ "microphone": json_value(snapshot.microphone) })),
                Err(error) => Err(error.to_string()),
            }
        }
        "get_remote_input_status" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let remote = &backend.services().remote_input;
            let status = remote.status().map_err(|error| error.message)?;
            let pin = runtime
                .block_on(remote.read_pairing_pin())
                .map_err(|error| error.message)?;
            Ok(json!({
                "status": json_value(status),
                // SecretValue 故意没有 Serialize（避免把密钥直接扔进任何序列化路径），
                // 这里取它的明文展示形式。
                "pairingPin": pin.expose_secret(),
            }))
        }
        "set_remote_locale" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let locale = required_str(_args, "locale")?.to_string();
            runtime
                .block_on(backend.services().remote_input.set_locale(locale))
                .map_err(|error| error.message)?;
            Ok(json!({ "ok": true }))
        }
        "regenerate_remote_pin" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let remote = &backend.services().remote_input;
            runtime
                .block_on(remote.regenerate_pairing_pin())
                .map_err(|error| error.message)?;
            let pin = runtime
                .block_on(remote.read_pairing_pin())
                .map_err(|error| error.message)?;
            Ok(json!(pin.expose_secret()))
        }
        // ── 当前频道选择 / 更新检查 ──────────────────────────────────
        "set_active_asr_provider" | "set_active_llm_provider" | "set_active_omni_provider" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let slot = if command == "set_active_asr_provider" {
                openless_core::ProviderSlot::Asr
            } else if command == "set_active_llm_provider" {
                openless_core::ProviderSlot::Llm
            } else {
                openless_core::ProviderSlot::Omni
            };
            // 前端参数名是 `provider`（不是 id）。
            let provider = required_str(_args, "provider")?.to_string();
            Ok(json_value(
                runtime
                    .block_on(backend.set_active_provider(slot, provider))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "app_check_update_with_channel" | "fetch_latest_beta_release" => {
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let channel = if command == "fetch_latest_beta_release" {
                openless_linux_host::update_check::UpdateChannel::Beta
            } else {
                match _args.get("channel").and_then(|value| value.as_str()) {
                    Some("beta") => openless_linux_host::update_check::UpdateChannel::Beta,
                    Some("stable") | None => {
                        openless_linux_host::update_check::UpdateChannel::Stable
                    }
                    Some(other) => return Err(format!("未知的更新通道：{other}")),
                }
            };
            // 走 CDN 清单（`dl.aeoform.com/updates/...`），不再读 GitHub atom：
            // 上游把 updater 清单搬到了 CDN，atom 那条只剩发布说明的含义。
            let manifest = runtime
                .block_on(fetch_update_manifest(channel))
                .map_err(|error| error.to_string())?;
            Ok(json_value(manifest))
        }
        // ── 重新润色 / 麦克风电平监视 / 热键状态 ─────────────────────
        "repolish" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let raw_text = required_str(_args, "rawText")?.to_string();
            let mode = polish_mode(_args)?;
            let style_pack_id = _args
                .get("stylePackId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let _ = mode; // 显式 style_pack_id 优先，与 Tauri 一致（mode 只为兼容老调用方）
            Ok(json_value(
                runtime
                    .block_on(backend.services().auxiliary.repolish(
                        openless_core::RepolishRequest {
                            raw_text,
                            style_pack_id,
                            // Linux 没有「取最前窗口」的端口（Tauri 用平台 API），
                            // 这里如实留空，不编造上下文。
                            front_app: None,
                        },
                    ))
                    .map_err(|error| error.to_string())?,
            ))
        }
        // ── 选区润色的预览 / 确认 / 取消 / 撤销 ───────────────────────────
        // 与 Tauri 的 `*_selection_polish_preview` 同一套语义：预览模式下面板
        // 拉一次负载，用户确认后把（可能改过的）文本替换回选区。
        "get_selection_polish_preview" => {
            if !host
                .selection_preview_pending
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                // 没有待处理的润色请求：不要把残留快照当成新请求。
                return Ok(Value::Null);
            }
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let snapshot = runtime
                .block_on(backend.services().selection.snapshot())
                .map_err(|error| error.to_string())?;
            match (snapshot.preview_text, snapshot.source_text) {
                (Some(text), Some(source_text)) => {
                    Ok(json!({ "text": text, "sourceText": source_text }))
                }
                _ => Ok(Value::Null),
            }
        }
        "confirm_selection_polish_preview" => {
            host.selection_preview_pending
                .store(false, std::sync::atomic::Ordering::SeqCst);
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let text = required_str(_args, "text")?.to_string();
            let snapshot = runtime
                .block_on(backend.services().selection.snapshot())
                .map_err(|error| error.to_string())?;
            let session_id = snapshot
                .session_id
                .ok_or_else(|| "selection preview is not active".to_string())?;
            runtime
                .block_on(backend.services().selection.confirm(session_id, Some(text)))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "confirmed": true }))
        }
        "cancel_selection_polish_preview" => {
            host.selection_preview_pending
                .store(false, std::sync::atomic::Ordering::SeqCst);
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            runtime
                .block_on(backend.services().selection.cancel(None))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "cancelled": true }))
        }
        "revert_selection_voice_preview" => {
            // 撤销上一次选区替换（语音编辑与润色共用同一个 selection session）。
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let snapshot = runtime
                .block_on(backend.services().selection.snapshot())
                .map_err(|error| error.to_string())?;
            let session_id = snapshot
                .session_id
                .ok_or_else(|| "selection preview is not active".to_string())?;
            runtime
                .block_on(backend.services().selection.revert(session_id))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "reverted": true }))
        }
        "apply_quick_note_repolish" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let session_id = required_str(_args, "sessionId")?.to_string();
            let text = required_str(_args, "text")?.to_string();
            let style_pack_id = _args
                .get("stylePackId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            Ok(json_value(
                backend
                    .apply_history_repolish(&session_id, text, style_pack_id)
                    .map_err(|error| error.to_string())?,
            ))
        }
        "start_microphone_level_monitor" => {
            // Tauri 这条是空实现（电平由录音链自己推）；这里同样不额外起采集，
            // 免得和录音抢设备。前端拿到的电平来自 `dictation:level` 事件。
            Ok(json!({ "started": false }))
        }
        "stop_microphone_level_monitor" => Ok(json!({ "stopped": true })),
        "get_hotkey_status" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let status = runtime
                .block_on(backend.services().platform.hotkey_status())
                .map_err(|error| error.to_string())?;
            Ok(json_value(status))
        }
        "set_desktop_onboarding_finished" => {
            // 引导完成状态落在偏好里（Tauri 用单独的状态文件；Linux 没有那次
            // 迁移历史，选项就是偏好，少一个文件少一处不同步）。
            let finished = arg_bool(_args, "finished")?;
            // 与 Tauri 落同一个文件（`desktop-onboarding.json`，camelCase 字段），
            // 这样迁移/共享数据目录时两边读到的是同一份状态。
            let path = data_dir().join("desktop-onboarding.json");
            let payload = serde_json::to_vec_pretty(&json!({ "finished": finished }))
                .map_err(|error| error.to_string())?;
            let temp = path.with_extension("json.tmp");
            std::fs::write(&temp, payload).map_err(|error| error.to_string())?;
            std::fs::rename(&temp, &path).map_err(|error| error.to_string())?;
            Ok(json!({ "finished": finished }))
        }
        // ── QA / 胶囊窗口级命令 ──────────────────────────────────────
        "qa_get_snapshot" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            Ok(json_value(
                runtime
                    .block_on(backend.services().qa.snapshot())
                    .map_err(|error| error.to_string())?,
            ))
        }
        "qa_toggle_recording" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            runtime
                .block_on(backend.services().qa.toggle_recording())
                .map_err(|error| error.to_string())?;
            Ok(json!({ "ok": true }))
        }
        "qa_window_dismiss" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            runtime
                .block_on(backend.services().qa.dismiss())
                .map_err(|error| error.to_string())?;
            Ok(json!({ "dismissed": true }))
        }
        "qa_set_edit_instruction_mode" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let enabled = arg_bool(_args, "enabled")?;
            runtime
                .block_on(backend.services().qa.set_edit_instruction_mode(enabled))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "enabled": enabled }))
        }
        "qa_submit_text" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let text = required_str(_args, "text")?.to_string();
            let expected = match _args.get("expectedSessionId").and_then(|value| value.as_str()) {
                Some(raw) => Some(
                    uuid::Uuid::parse_str(raw)
                        .map(openless_core::SessionId::from_uuid)
                        .map_err(|error| format!("session id 不合法：{error}"))?,
                ),
                None => None,
            };
            // 带上下文提交：避免把回答贴到已经换掉的会话里。
            runtime
                .block_on(backend.services().qa.submit_text_in_context(text, expected))
                .map_err(|error| error.to_string())?;
            Ok(json!({ "ok": true }))
        }
        "qa_window_set_expanded" => {
            let expanded = arg_bool(_args, "expanded")?;
            // 窗口尺寸由宿主决定（与 Tauri 同语义：只改变展示形态，不动会话状态）。
            set_qa_expanded(host, expanded);
            Ok(json!({ "expanded": expanded }))
        }
        "get_capsule_snapshot" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let dictation = backend.snapshot().dictation;
            Ok(json!({
                "sessionId": dictation.session_id.map(|id| id.to_string()),
                "phase": json_value(dictation.phase),
                "level": dictation.level,
                "elapsedMs": dictation.elapsed_ms,
                "transcriptVisible": *host.capsule_transcript_visible.lock().unwrap(),
            }))
        }
        "set_capsule_transcript_visible" => {
            let visible = arg_bool(_args, "visible")?;
            *host.capsule_transcript_visible.lock().unwrap() = visible;
            Ok(json!({ "transcriptVisible": visible }))
        }
        "set_capsule_language_menu" => {
            // 语言菜单是胶囊内的临时浮层；宿主只记录开合状态供快照查询。
            let open = arg_bool(_args, "open")?;
            *host.capsule_language_menu.lock().unwrap() = open;
            Ok(json!({ "open": open }))
        }
        "chat_panel_focus_keyboard" => {
            // 把键盘焦点交回主窗口（页面里的聊天面板需要输入）。
            focus_main_window(host);
            Ok(json!({ "focused": true }))
        }
        "set_shortcut_recording_active" => {
            let active = arg_bool(_args, "active")?;
            *host.shortcut_recording_active.lock().unwrap() = active;
            Ok(json!({ "active": active }))
        }
        "dismiss_insert_fallback_card" | "report_insert_fallback_card_height" => {
            // 插入兜底卡片在 Linux 宿主里不存在（没有那条插入链路），
            // 这几条只用于页面内部状态同步，这里如实记为无操作。
            Ok(json!(null))
        }
        // ── 钥匙串（云同步的 E2EE 密钥就存在这里）─────────────────────
        "keyring_state" => {
            let (keyring, input) = openless_linux_host::linux_access::probe_session();
            Ok(json!({
                "keyring": json_value(keyring),
                "input": json_value(input),
            }))
        }
        "unlock_keyring" => {
            // 只由用户动作触发解锁（会自动弹 kwallet 对话框）；
            // 自动启动路径永远不解锁，避免启动时弹窗。
            let timeout = openless_linux_host::linux_access::VAULT_UNLOCK_TIMEOUT;
            match openless_linux_host::linux_access::unlock_default_vault(timeout) {
                Ok(()) => Ok(json!({ "unlocked": true })),
                Err(error) => Err(error.to_string()),
            }
        }
        // Linux 没有 macOS 的辅助功能授权，也没有 Windows 的 IME 状态：
        // 返回「已授权 / 非 Windows」而不是报错，否则设置页会显示成故障。
        "check_accessibility_permission" | "request_accessibility_permission" => {
            Ok(json!("granted"))
        }
        // 输入法后端状态（Linux 版）：前端用的还是 `WindowsImeStatus` 那个结构，
        // 但 Linux 上「后端」指的是 fcitx5 插件，所以这里给的是真实的三项：
        // 插件是否装了、守护进程在不在、D-Bus 服务是否有 owner。
        "get_ime_backend_status" => {
            let plugin_installed = openless_linux_host::fcitx5_plugin_installed();
            let daemon_running = openless_linux_host::fcitx5_available();
            Ok(json!({
                "backend": "fcitx5",
                "pluginInstalled": plugin_installed,
                "daemonRunning": daemon_running,
                "ready": plugin_installed && daemon_running,
            }))
        }
        // Linux 没有 TSF：结构必须和前端的 `WindowsImeStatus` 完全一致，
        // 少了 `state` 会让设置页的 `state !== 'notWindows'` 恒真，白白显示一行
        // 「Windows 输入法后端：undefined」。
        "get_windows_ime_status" => Ok(json!({
            "state": "notWindows",
            "usingTsfBackend": false,
            "message": null,
            "dllPath": null,
        })),
        "get_hotkey_capability" => {
            // Linux 的能力描述 core 里已经写好了（adapter=Fcitx5 + 各触发键 +
            // evdev 权限提示），宿主不要再拼一份，否则两边会漂。
            Ok(json_value(openless_core::shared_types::HotkeyCapability::current()))
        }
        // ── 风格市场 + GitHub 设备码登录（全部走 core 的 MarketplaceApi）──
        "marketplace_list" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            // 三个字段都可选，所以缺省就是「不带过滤」，不需要 Default 实现。
            let query = openless_core::MarketplaceQuery {
                query: _args.get("query").and_then(|v| v.as_str()).map(str::to_string),
                sort: _args.get("sort").and_then(|v| v.as_str()).map(str::to_string),
                limit: _args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
            };
            Ok(json_value(
                runtime
                    .block_on(backend.services().marketplace.list(query))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "marketplace_detail" | "marketplace_install" | "marketplace_like"
        | "marketplace_delete" | "marketplace_my_likes" | "marketplace_my_packs"
        | "marketplace_auth_status" | "marketplace_logout"
        | "github_device_flow_start" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let api = &backend.services().marketplace;
            let pack_id = _args
                .get("packId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let outcome = match command {
                "marketplace_detail" => {
                    let id = pack_id.ok_or_else(|| "缺少 packId".to_string())?;
                    json_value(runtime.block_on(api.detail(id)).map_err(|e| e.to_string())?)
                }
                "marketplace_install" => {
                    let id = pack_id.ok_or_else(|| "缺少 packId".to_string())?;
                    json_value(runtime.block_on(api.install(id)).map_err(|e| e.to_string())?)
                }
                "marketplace_like" => {
                    let id = pack_id.ok_or_else(|| "缺少 packId".to_string())?;
                    json_value(
                        runtime.block_on(api.toggle_like(id)).map_err(|e| e.to_string())?,
                    )
                }
                "marketplace_delete" => {
                    let id = pack_id.ok_or_else(|| "缺少 packId".to_string())?;
                    runtime.block_on(api.delete(id)).map_err(|e| e.to_string())?;
                    Value::Null
                }
                "marketplace_my_likes" => json_value(
                    runtime.block_on(api.my_likes()).map_err(|e| e.to_string())?,
                ),
                "marketplace_my_packs" => json_value(
                    runtime.block_on(api.my_packs()).map_err(|e| e.to_string())?,
                ),
                "marketplace_auth_status" => json_value(
                    runtime.block_on(api.auth_status()).map_err(|e| e.to_string())?,
                ),
                "marketplace_logout" => {
                    runtime.block_on(api.logout()).map_err(|e| e.to_string())?;
                    Value::Null
                }
                // 字段名必须和前端 `GithubDeviceStartResponse` 一致：
                // core 给的是 `intervalSecs` / `expiresInSecs`，前端读的是
                // `interval` / `expiresIn`。名字对不上时 expiresAt 会变成 NaN，
                // 轮询间隔退化成 0 —— 实测 15 分钟里打了 7752 次 poll。
                _ => {
                    let start = |runtime: &std::sync::Arc<tokio::runtime::Runtime>| {
                        runtime.block_on(api.start_device_flow())
                    };
                    // 上一次登录被取消后，core 会拒绝立刻重新发起；先清一下再重试。
                    let flow = match start(&runtime) {
                        Ok(flow) => flow,
                        Err(error) => {
                            let message = error.to_string();
                            if message.contains("取消") || message.contains("cancel") {
                                let _ = runtime.block_on(api.cancel_device_flow(None));
                                start(&runtime).map_err(|retry| retry.to_string())?
                            } else {
                                return Err(message);
                            }
                        }
                    };
                    json!({
                        "flowId": flow.flow_id,
                        "userCode": flow.user_code,
                        "verificationUri": flow.verification_uri,
                        "interval": flow.interval_secs,
                        "expiresIn": flow.expires_in_secs,
                    })
                }
            };
            Ok(outcome)
        }
        "marketplace_download" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let pack_id = required_str(_args, "packId")?.to_string();
            let target = required_str(_args, "targetPath")?.to_string();
            let bytes = runtime
                .block_on(backend.services().marketplace.download_archive(pack_id))
                .map_err(|error| error.to_string())?;
            if let Some(parent) = PathBuf::from(&target).parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::write(&target, &bytes).map_err(|error| error.to_string())?;
            Ok(json!({ "written": target, "bytes": bytes.len() }))
        }
        "marketplace_upload" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let pack_id = required_str(_args, "packId")?.to_string();
            let origin = _args
                .get("originPackId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            Ok(json_value(
                runtime
                    .block_on(backend.services().marketplace.upload(pack_id, origin))
                    .map_err(|error| error.to_string())?,
            ))
        }
        "github_device_flow_poll" | "github_device_flow_cancel" => {
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let api = &backend.services().marketplace;
            // 前端 `githubDeviceFlowCancel(undefined)` 会传 `flowId: null`，
            // 语义是「取消当前流程」，所以这里是可选参数。
            let flow_id = _args
                .get("flowId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            if command == "github_device_flow_poll" {
                let flow_id = flow_id.ok_or_else(|| "缺少参数 flowId".to_string())?;
                let polled = runtime
                    .block_on(api.poll_device_flow(flow_id))
                    .map_err(|error| error.to_string())?;
                // 刚完成授权：云同步缓存的是**旧 token**，清掉会话让它用新
                // token 重连，否则界面会一直显示「请重新登录」。
                if matches!(polled, openless_core::OAuthPollResult::Authorized { .. }) {
                    // spawn 需要 'static 所有权，这里拿一份 Arc。
                    if let Some(arc) = host.backend.as_ref().map(std::sync::Arc::clone) {
                        runtime.spawn(async move {
                            // **不要**在这里调 `cloud_sync_e2ee_sign_out()`：
                            // core 的 sign_out 会把 Marketplace 的 GitHub token
                            // 一起删掉并打 tombstone —— 等于刚登录就立刻登出，
                            // 界面于是永远提示「请重新登录」。
                            // `sync_now` 自己会发现连接里的 token 与当前
                            // marketplace token 不一致，从而强制重连。
                            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                            match arc.cloud_sync_e2ee_sync_now().await {
                                Ok(status) => println!(
                                    "[linux-web] 登录后云同步已重连：auth={:?} account={:?}",
                                    status.auth_state, status.account
                                ),
                                Err(error) => {
                                    println!("[linux-web] 登录后云同步重连失败：{error}")
                                }
                            }
                        });
                    }
                }
                Ok(json_value(polled))
            } else {
                runtime
                    .block_on(api.cancel_device_flow(flow_id))
                    .map_err(|error| error.to_string())?;
                Ok(json!(null))
            }
        }
        "get_history" | "history_list" => {
            let history = read_json(&dir.join("history.json")).unwrap_or_else(|| json!([]));
            Ok(json!({ "entries": history, "revision": 0 }))
        }
        other => Err(format!("未实现的命令：{other}")),
    }
}

// ── 静态资源 ────────────────────────────────────────────────────────────────

fn serve_dir(root: PathBuf, host: std::sync::Arc<Host>) -> std::io::Result<u16> {
    if !root.join("index.html").exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} 里没有 index.html", root.display()),
        ));
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            let host = host.clone();
            std::thread::spawn(move || {
                let _ = handle_request(stream, &root, &host);
            });
        }
    });
    Ok(port)
}

/// 静态文件 + 命令桥。
///
/// 除了把 `dist/` 发出去，还提供两个接口，让**任何**渲染宿主（Qt/CEF/浏览器）
/// 都能驱动同一个 Rust 后端，而不必各自实现一遍 135 条命令：
///
/// - `POST /api/invoke`  `{id,command,args}` → `{ok,value|error}`
/// - `GET  /api/events`  SSE，推 `{name,payload}`（映射与 GTK 宿主同一份）
fn handle_request(mut stream: TcpStream, root: &Path, host: &Host) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let path = target.split('?').next().unwrap_or("/").to_string();

    if path == "/api/events" {
        return stream_events(stream, host);
    }
    if path == "/api/invoke" && method == "POST" {
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;
        let reply = (|| -> Result<Value, String> {
            let message: Value = serde_json::from_slice(&body)
                .map_err(|error| format!("请求体不是 JSON：{error}"))?;
            let command = message
                .get("command")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "缺少 command".to_string())?;
            let args = message.get("args").cloned().unwrap_or_else(|| json!({}));
            dispatch(host, command, &args)
        })();
        // 全量命令日志：前端只显示友好文案，排障必须能看到到底调了什么。
        println!("[cmd] {}", message_command(&body));
        let payload = match reply {
            Ok(value) => json!({ "ok": true, "value": value }),
            Err(error) => {
                // 前端只显示友好文案，真实错误码在这里留一份，排障时查 journal。
                println!(
                    "[cmd] {} -> error: {}",
                    message_command(&body),
                    error
                );
                json!({ "ok": false, "error": error })
            }
        };
        let bytes = serde_json::to_vec(&payload).unwrap_or_default();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
            bytes.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(&bytes)?;
        return Ok(());
    }
    if path == "/api/ping" {
        let bytes = serde_json::to_vec(&json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
            .unwrap_or_default();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
            bytes.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(&bytes)?;
        return Ok(());
    }

    let mut file_path = root.join(path.trim_start_matches('/'));
    if path.ends_with('/') || file_path.is_dir() {
        file_path.push("index.html");
    }
    let body = std::fs::read(&file_path).unwrap_or_default();
    let (status, mime) = if body.is_empty() {
        ("404 Not Found", "text/plain")
    } else {
        ("200 OK", mime_for(&file_path))
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)?;
    Ok(())
}

/// SSE：把后端事件推给页面（Qt/CEF/浏览器都能直接用）。
/// 从请求体里取出命令名（仅用于错误日志）。
fn message_command(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.get("command").and_then(|c| c.as_str()).map(str::to_string))
        .unwrap_or_default()
}

fn stream_events(mut stream: TcpStream, host: &Host) -> std::io::Result<()> {
    let (Some(backend), Some(runtime)) = (host.backend.clone(), host.runtime.clone()) else {
        let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n");
        return Ok(());
    };
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
    )?;
    stream.write_all(b": connected\n\n")?;
    stream.flush()?;
    let mut subscription = backend.subscribe();
    let mut host_events = host.host_events.subscribe();
    let mut last_cloud_sync: Option<String> = None;
    loop {
        // 托盘命令与 host action 也走这条流：渲染宿主不需要第二种传输。
        let host_event = runtime.block_on(async {
            tokio::select! {
                event = subscription.recv() => Ok(event),
                forwarded = host_events.recv() => Err(forwarded),
            }
        });
        if let Err(forwarded) = host_event {
            let Ok((name, payload)) = forwarded else { continue };
            let line = json!({ "name": name, "payload": payload });
            if write!(stream, "data: {line}\n\n").is_err() || stream.flush().is_err() {
                break;
            }
            continue;
        }
        let event = match host_event.expect("checked above") {
            Ok(event) => event,
            Err(_) => {
                // 滞后：让页面重新拉快照。
                if stream
                    .write_all(b"data: {\"name\":\"resync\",\"payload\":\"{}\"}\n\n")
                    .is_err()
                {
                    break;
                }
                let _ = stream.flush();
                continue;
            }
        };
        let Some((name, payload)) = events::map_event(&event) else {
            continue;
        };
        let payload_text = payload.to_string();
        if name.starts_with("cloud-sync-e2ee:") {
            // 内容没变就不重发：前端收到 state 事件会重新拉 status，
            // 重复事件会把它变成自激循环（实测能打到每秒几十次）。
            if last_cloud_sync.as_deref() == Some(payload_text.as_str()) {
                continue;
            }
            last_cloud_sync = Some(payload_text.clone());
        }
        let line = json!({ "name": name, "payload": payload_text });
        if write!(stream, "data: {line}\n\n").is_err() || stream.flush().is_err() {
            break;
        }
    }
    Ok(())
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}
