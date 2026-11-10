//! Linux 宿主：GTK 窗口 + WebKitGTK 渲染**官方 React 界面**（不装 Tauri）。
//!
//! 桥的形状与 `src/lib/harmonyBridge.ts` 完全一致：
//!   JS → native : window.webkit.messageHandlers.openless.postMessage(JSON)
//!   native → JS : window.__openlessResolve(id, JSON)
//!
//! 静态资源用一个只在 127.0.0.1 上监听的小 HTTP 服务提供（WebKit 拒绝 file://
//! 下的 ES module / fetch），这也是 Tauri/Cog 之类的常规做法。
#[cfg(feature = "capsule")]
mod capsule;
mod events;
mod host;
mod hotkeys;

use gtk::prelude::*;
use javascriptcore::ValueExt;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use serde_json::{json, Value};
use webkit2gtk::{SettingsExt, UserContentManager, UserContentManagerExt, WebViewExt};

struct Host {
    backend: Option<std::sync::Arc<openless_core::OpenLessBackend>>,
    runtime: Option<std::sync::Arc<tokio::runtime::Runtime>>,
    settings_runtime: Option<std::sync::Arc<dyn openless_core::SettingsRuntime>>,
}

fn main() {
    let host = init_host();
    let dist = std::env::var("OPENLESS_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dist"));
    let port = match serve_dir(dist.clone()) {
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

    let app = gtk::Application::new(Some("com.openless.linuxweb"), Default::default());
    let host = std::sync::Arc::new(host);
    app.connect_activate(move |app| build_window(app, &url, host.clone()));
    app.run();
}

fn build_window(app: &gtk::Application, url: &str, host: std::sync::Arc<Host>) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("OpenLess")
        .default_width(1180)
        .default_height(820)
        .build();

    let manager = UserContentManager::new();
    manager.register_script_message_handler("openless");
    // 在页面任何脚本之前打上引擎标记：WebKitGTK 在合成器上有几处做不到
    // Chromium/WebView2 那样的性能（最明显的是全页 `filter: blur()` 过渡），
    // 前端据此降级动画，而不是让用户看到抖动。

/// 排障用帧探针（`OPENLESS_FRAME_PROBE=1`）：监听页面切换的相位属性变化，
/// 记录切换期间每帧间隔，最差帧通过 `log_client_error` 打到宿主 stderr。
/// 只在设了这个环境变量时注入，正式运行不产生任何开销。
const FRAME_PROBE: &str = r#"
(() => {
  const marks = new Map();
  const watch = () => {
    const surfaces = document.querySelectorAll('[data-transition-phase]');
    surfaces.forEach((node) => {
      const phase = node.getAttribute('data-transition-phase');
      if (phase === 'settled') return;
      const started = performance.now();
      if (marks.has(phase)) return;
      marks.set(phase, started);
      let last = started;
      let worst = 0;
      // 同时盯住左侧导航与滚动容器的矩形：整窗平移/抖动会让导航也跟着动，
      // 只有内容变则是局部重排（典型原因：滚动条出现/消失改变可用宽度）。
      const nav = document.querySelector('nav');
      const base = nav ? nav.getBoundingClientRect() : null;
      let navShift = 0;
      let widthShift = 0;
      const tick = (now) => {
        worst = Math.max(worst, now - last);
        last = now;
        if (base && nav) {
          const rect = nav.getBoundingClientRect();
          navShift = Math.max(navShift, Math.abs(rect.left - base.left), Math.abs(rect.top - base.top));
          widthShift = Math.max(widthShift, Math.abs(rect.width - base.width));
        }
        if (now - started < 420) {
          requestAnimationFrame(tick);
        } else {
          marks.delete(phase);
          window.webkit.messageHandlers.openless.postMessage(JSON.stringify({
            id: 'probe-' + Math.random().toString(36).slice(2),
            command: 'log_client_error',
            args: JSON.stringify({ message: 'frame-probe ' + phase + ' worst=' + worst.toFixed(1) + 'ms navShift=' + navShift.toFixed(2) + ' navWidthShift=' + widthShift.toFixed(2) }),
          }));
        }
      };
      requestAnimationFrame(tick);
    });
  };
  new MutationObserver(watch).observe(document.documentElement, {
    subtree: true,
    attributes: true,
    attributeFilter: ['data-transition-phase'],
  });
  watch();
})();
"#;


/// 排障用页面轮换（`OPENLESS_PAGE_SWEEP=1`）：每 900ms 点一个侧栏项，
/// 让帧探针有真实的页面切换可测。只在设了这个环境变量时注入。
const PAGE_SWEEP: &str = r#"
(() => {
  const tick = () => {
    const buttons = Array.from(document.querySelectorAll('button.ol-nav-btn'));
    if (!buttons.length) return void setTimeout(tick, 400);
    let index = 0;
    setInterval(() => {
      index = (index + 1) % buttons.length;
      buttons[index].click();
    }, 900);
  };
  setTimeout(() => {
    window.webkit.messageHandlers.openless.postMessage(JSON.stringify({
      id: 'sweep-engine',
      command: 'log_client_error',
      args: JSON.stringify({ message: 'engine=' + (document.documentElement.dataset.olEngine || 'none') }),
    }));
    tick();
  }, 3000);
})();
"#;

    if std::env::var("OPENLESS_PAGE_SWEEP").ok().as_deref() == Some("1") {
        manager.add_script(&webkit2gtk::UserScript::new(
            PAGE_SWEEP,
            webkit2gtk::UserContentInjectedFrames::TopFrame,
            webkit2gtk::UserScriptInjectionTime::End,
            &[],
            &[],
        ));
    }
    if std::env::var("OPENLESS_FRAME_PROBE").ok().as_deref() == Some("1") {
        manager.add_script(&webkit2gtk::UserScript::new(
            FRAME_PROBE,
            webkit2gtk::UserContentInjectedFrames::TopFrame,
            webkit2gtk::UserScriptInjectionTime::End,
            &[],
            &[],
        ));
    }
    manager.add_script(&webkit2gtk::UserScript::new(
        "window.__openlessEngine='webkitgtk';\
         document.documentElement.dataset.olEngine='webkitgtk';",
        webkit2gtk::UserContentInjectedFrames::TopFrame,
        webkit2gtk::UserScriptInjectionTime::Start,
        &[],
        &[],
    ));

    let view = webkit2gtk::WebView::builder()
        .user_content_manager(&manager)
        .build();

    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_enable_webgl(true);
        settings.set_enable_developer_extras(true);
    }
    // 与产品一致的浅色底；页面自己画卡片。
    view.set_background_color(&gtk::gdk::RGBA::new(0.96, 0.96, 0.95, 1.0));

    let host_for_ipc = host.clone();
    let view_for_ipc = view.clone();
    manager.connect_script_message_received(Some("openless"), move |_m, result| {
        let Some(value) = result.js_value() else { return };
        let raw = value.to_str().to_string();
        let Ok(message) = serde_json::from_str::<Value>(&raw) else {
            eprintln!("[linux-web] 无法解析桥消息: {raw}");
            return;
        };
        let id = message["id"].as_str().unwrap_or_default().to_string();
        let command = message["command"].as_str().unwrap_or_default().to_string();
        let args: Value = message["args"]
            .as_str()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_else(|| json!({}));
        let (ok, payload) = match dispatch(&host_for_ipc, &command, &args) {
            Ok(value) => (true, json!({ "ok": true, "value": value })),
            Err(error) => (false, json!({ "ok": false, "error": error })),
        };
        let js = format!(
            "window.__openlessResolve({}, {});",
            json!(id),
            json!(payload.to_string())
        );
        let _ = ok;
        view_for_ipc.evaluate_javascript(
            &js,
            None::<&str>,
            None::<&str>,
            None::<&webkit2gtk::gio::Cancellable>,
            |_| {},
        );
    });

    // 排障用：OPENLESS_WEB_TOPMOST=1 让窗口浮在最前（截图/对比时用）。
    if std::env::var("OPENLESS_WEB_TOPMOST").ok().as_deref() == Some("1") {
        window.set_keep_above(true);
    }
    window.set_child(Some(&view));
    view.load_uri(url);
    view.connect_load_changed(|_view, event| {
        if event == webkit2gtk::LoadEvent::Finished {
            println!("[linux-web] 页面已加载完成");
        }
    });
    window.show_all();
    // 托盘与窗口显隐：窗口关掉只是隐藏，进程继续常驻（与 Tauri 版行为一致）。
    window.connect_delete_event(|window, _| {
        window.hide();
        gtk::glib::Propagation::Stop
    });
    host::start_tray(window.clone());
    // 后端就绪后立刻开始把事件推给页面（听写状态 / 历史变更 / QA 电平等）。
    {
        let backend = host.backend.clone();
        let runtime = host.runtime.clone();
        if let (Some(backend), Some(runtime)) = (backend.clone(), runtime.clone()) {
            events::spawn_backend_events(backend.clone(), runtime.clone(), view.clone());
            hotkeys::spawn(backend.clone(), runtime.clone(), window.clone());
            #[cfg(feature = "capsule")]
            {
                // 胶囊是独立的 layer surface：不抢焦点、贴底居中，页面走 ?capsule=1。
                let capsule = std::sync::Arc::new(capsule::build(
                    app,
                    &format!("{url}?window=capsule"),
                    CAPSULE_WIDTH,
                    CAPSULE_HEIGHT,
                    CAPSULE_BOTTOM_MARGIN,
                ));
                // 排障：OPENLESS_CAPSULE_ALWAYS=1 时启动就显示胶囊（截图/对齐用）。
                if std::env::var("OPENLESS_CAPSULE_ALWAYS").ok().as_deref() == Some("1") {
                    capsule.window.show_all();
                }
                capsule::attach_events(capsule, runtime, backend);
            }
        }
    }
}

/// 胶囊尺寸与贴底间距（与 Tauri 的胶囊窗口一致：宽 460、底距 12）。
#[cfg(feature = "capsule")]
const CAPSULE_WIDTH: i32 = 460;
#[cfg(feature = "capsule")]
const CAPSULE_HEIGHT: i32 = 228;
#[cfg(feature = "capsule")]
const CAPSULE_BOTTOM_MARGIN: i32 = 12;

// ── 命令分发 ────────────────────────────────────────────────────────────────

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
    let mut host = Host { backend: None, runtime: None, settings_runtime: None };
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
    match openless_linux_egui::LinuxBackendBuilder::from_shared_providers(config)
        .and_then(|builder| builder.build())
    {
        Ok(runtime_parts) => {
            println!("[linux-web] 后端就绪（openless-core 已装配）");
            host.backend = Some(runtime_parts.backend);
            host.settings_runtime = Some(runtime_parts.settings_runtime);
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
            let backend = backend.ok_or_else(|| "后端未就绪".to_string())?;
            Ok(json_value(
                backend.cloud_sync_e2ee_status().map_err(|error| error.to_string())?,
            ))
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
                openless_linux_egui::fcitx5_copy_to_clipboard(text)
                    .map_err(|error| error.to_string())?;
            }
            Ok(json!({ "copied": !text.is_empty() }))
        }
        "check_network" => {
            let runtime = host.runtime.clone().ok_or_else(|| "运行时未就绪".to_string())?;
            let (state, latency) =
                runtime.block_on(openless_linux_egui::linux_access::probe_network());
            Ok(json!({
                "online": matches!(state, openless_linux_egui::linux_access::CheckState::Ready),
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
            let binding: Option<openless_core::shared_types::ComboBinding> =
                serde_json::from_value(_args.get("binding").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
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
        "get_history" | "history_list" => {
            let history = read_json(&dir.join("history.json")).unwrap_or_else(|| json!([]));
            Ok(json!({ "entries": history, "revision": 0 }))
        }
        other => Err(format!("未实现的命令：{other}")),
    }
}

// ── 静态资源 ────────────────────────────────────────────────────────────────

fn serve_dir(root: PathBuf) -> std::io::Result<u16> {
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
            std::thread::spawn(move || {
                let _ = handle_request(stream, &root);
            });
        }
    });
    Ok(port)
}

fn handle_request(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
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
