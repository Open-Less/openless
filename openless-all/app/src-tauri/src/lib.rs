#![cfg_attr(
    target_os = "linux",
    allow(dead_code, unused_imports, unused_variables)
)]
//! OpenLess Tauri backend.
//!
//! Modules mirror the original Swift libraries (one purpose per file):
//! - hotkey: global hotkey monitor
//! - recorder: microphone capture (16 kHz mono Int16 PCM)
//! - asr: streaming ASR providers (Volcengine SAUC bigmodel)
//! - polish: OpenAI-compatible chat completions client
//! - insertion: cursor-position text insertion (AX / paste)
//! - persistence: history + preferences + credentials vault
//! - coordinator: dictation state machine glue
//! - commands: Tauri IPC surface

mod asr;
mod audio_mute;
mod cli;
mod coding_agent;
mod combo_hotkey;
mod commands;
mod coordinator;
mod coordinator_state;
mod correction;
mod global_hotkey_runtime;
mod hotkey;
mod insertion;
#[cfg(target_os = "linux")]
mod linux_fcitx;
mod llm_gemini;
mod net;
mod permissions;
mod persistence;
mod polish;
mod qa_hotkey;
mod recorder;
mod selection;
mod shortcut_binding;
mod types;
mod unicode_keystroke;
mod windows_ime_ipc;
mod windows_ime_profile;
mod windows_ime_protocol;
mod windows_ime_session;

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

const LOG_ROTATE_LIMIT_BYTES: u64 = 10 * 1024 * 1024;
#[cfg(target_os = "macos")]
const OPENLESS_BUNDLE_ID: &str = "com.openless.app";

/// 第一次 show 时把 QA 浮窗摆到屏幕底部居中；之后的 show 不再 reposition，
/// 让用户拖动后的位置在 hide → show 之间得以保持。详见 issue #118 v2。
static QA_WINDOW_POSITIONED: AtomicBool = AtomicBool::new(false);
static TRAY_MICROPHONE_WATCHER_STOPPING: AtomicBool = AtomicBool::new(false);
use tauri::menu::{
    CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder,
};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    RunEvent, Runtime,
};

use crate::types::PolishMode;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let foundry_local_runtime = Arc::new(asr::local::FoundryLocalRuntime::new());
    let sherpa_onnx_runtime = Arc::new(asr::local::SherpaOnnxRuntime::new());
    let sherpa_download_manager =
        Arc::new(asr::local::sherpa_download::SherpaDownloadManager::new());
    #[cfg(target_os = "windows")]
    let coordinator = Arc::new(coordinator::Coordinator::new_with_local_runtimes(
        Arc::clone(&foundry_local_runtime),
        Arc::clone(&sherpa_onnx_runtime),
    ));
    #[cfg(not(target_os = "windows"))]
    let coordinator = Arc::new(coordinator::Coordinator::new());
    #[cfg(target_os = "windows")]
    if let Err(error) = coordinator.sync_active_asr_provider_from_preferences() {
        log::warn!("[startup] sync active ASR provider from preferences failed: {error}");
    }
    let local_asr_download_manager = Arc::new(asr::local::DownloadManager::new());

    tauri::Builder::default()
        // 单实例锁：第二个进程启动时立即退出，激活信号转给已运行实例的主窗口。
        // 否则两份 OpenLess（如 /Applications/ + dev build）会各自抓全局热键，
        // 导致按一次键、两个进程同时跑流水线、文本被插入两遍。见 issue #50。
        //
        // 第二个进程的 argv 还有一个用处：作为 Linux 下的「触发器入口」。
        // 桌面环境快捷键执行 `openless --toggle-dictation` 时，第二个进程被本插件
        // 拦截 → argv 直接转给主实例 coordinator。详见 issue #420 / `cli.rs`。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(intent) = cli::parse_cli_intent(&argv) {
                log::info!(
                    "[single-instance] another instance launched with intent={intent:?}, dispatching"
                );
                dispatch_cli_intent(app, intent);
                return;
            }
            // 静默启动模式下：第二次启动（Win11 的「登录时重新打开应用」、autostart 双触发、
            // 或用户手动再点图标）也不弹主窗口，否则 start_minimized=true 在 Win11 上整体失效。
            // 用户想看主窗口走托盘菜单 / 托盘左键。issue #468。
            if let Some(coordinator) = app
                .try_state::<Arc<coordinator::Coordinator>>()
                .map(|s| Arc::clone(&*s))
            {
                if coordinator.prefs().get().start_minimized {
                    log::info!(
                        "[single-instance] start_minimized=true → skipping show on relaunch"
                    );
                    return;
                }
            }
            log::info!(
                "[single-instance] another instance launched, focusing existing main window"
            );
            show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // 跨平台开机自启：mac 写 LaunchAgent plist，linux 写 ~/.config/autostart/*.desktop，
        // windows 写 HKCU\Software\Microsoft\Windows\CurrentVersion\Run。前端 toggle 直接
        // 调插件 isEnabled / enable / disable，不维持本地 prefs，让 OS 当唯一真相。issue #194。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(coordinator.clone())
        .manage(local_asr_download_manager.clone())
        .manage(sherpa_download_manager.clone())
        .manage(foundry_local_runtime.clone())
        .manage(sherpa_onnx_runtime.clone())
        .manage(commands::MicrophoneMonitorState::new(None))
        .manage(commands::TrayMicrophoneMenuState::new(Vec::new()))
        .setup(move |app| {
            init_file_logger();
            log::info!("=== OpenLess 启动 ===");

            // Capsule 启动时定位到屏幕底部居中并隐藏；coordinator 按需显示。
            // 与 Swift `CapsuleWindowController.repositionToBottomCenter` 同语义。
            if let Some(capsule) = app.get_webview_window("capsule") {
                if let Err(e) = position_capsule_bottom_center(&capsule, false) {
                    log::warn!("[capsule] position failed: {e}");
                }
                let _ = capsule.hide();
            }

            // QA 浮窗（issue #118）：紧贴胶囊上方 8pt、屏幕底部居中、380×440。
            // 启动时 hide()，等 coordinator 在 open_qa_panel 时再 show + 首次定位。
            // tauri.conf.json 里需要声明 label="qa" 的窗口（前端 agent 负责）；
            // 这里 get_webview_window 返回 None 时直接跳过，不影响主流程。
            if let Some(qa) = app.get_webview_window("qa") {
                if let Err(e) = position_qa_window(&qa) {
                    log::warn!("[qa] position failed: {e}");
                }
                #[cfg(target_os = "macos")]
                make_qa_window_draggable_macos(&qa);
                let _ = qa.hide();
            } else {
                log::info!("[qa] qa 窗口未在 tauri.conf.json 中声明，前端 agent 会补上");
            }

            // Less Computer 语音 Agent 浮窗（macOS only）。启动时隐藏；coordinator
            // 在 Less Computer 会话开始时再 show + 定位。非 macOS 上该窗口虽在
            // tauri.conf.json 声明，但前端不渲染入口、后端不 emit，保持隐藏惰性。
            if let Some(lc) = app.get_webview_window("less-computer") {
                let _ = lc.hide();
            }

            // 主窗口磨砂：macOS 用 NSVisualEffectView，Windows 用 Mica。
            // 没这一层的话 transparent: true 让窗口透明 → 背后只是空，不是磨砂。
            //
            // decorations 留给运行时分平台决定：macOS 默认 true 用系统红黄绿；
            // Windows 这里关掉 native chrome 让 React 端 WinTitleBar 接管。
            if let Some(main) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };
                    if let Err(e) = main.set_decorations(true) {
                        log::warn!("[main] enable native decorations failed: {e}");
                    }
                    if let Err(e) = apply_vibrancy(
                        &main,
                        NSVisualEffectMaterial::HudWindow,
                        Some(NSVisualEffectState::Active),
                        Some(20.0),
                    ) {
                        log::warn!("[main] vibrancy failed: {e}");
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    use window_vibrancy::apply_mica;
                    // Windows 走 Tauri decorations:true 原生 Win11 标题栏 / 关闭按钮 /
                    // 拖动 / 圆角 / resize border。保留 apply_mica 给原生 chrome 提供
                    // 磨砂材质，配合 WindowChrome 半透明 background 让 sidebar 透出玻璃感。
                    if let Err(e) = apply_mica(&main, None) {
                        log::warn!("[main] mica failed: {e}");
                    }
                    // Win11 22H2+: 把原生标题栏底色调成白色，与应用 sidebar 视觉统一。
                    // 老版 Windows 静默失败，不阻塞。
                    apply_windows_caption_color(&main);
                }
                // 静默启动开关：prefs.start_minimized = true → 不弹主窗口，
                // 用户从菜单栏 / 托盘点击访问。开机自启时尤其有用，避免每次
                // 登录都被主窗口打扰。OPENLESS_SHOW_MAIN_ON_START=1 仍保留
                // 老的强制 show 路径（手动 dispatch 测试 / dev 用），优先级高
                // 于 prefs。
                let force_show =
                    std::env::var("OPENLESS_SHOW_MAIN_ON_START").ok().as_deref() == Some("1");
                let suppress_show = !force_show && coordinator.prefs().get().start_minimized;
                if suppress_show {
                    log::info!("[main] start_minimized=true → 跳过初始 show，等用户点托盘");
                } else {
                    #[cfg(target_os = "linux")]
                    {
                        // Workaround for Linux Wayland WebKitGTK compositing:
                        // `visible:false` → `show()` can leave the webview surface
                        // without a valid input region. The ±1px nudge forces
                        // GTK size-allocate → input surface reattach.
                        // Ref: tauri#9394, cc-switch linux_fix.rs
                        let main_clone = main.clone();
                        let _ = main_clone.set_focus();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            let _ = main_clone.set_focus();
                            if let Ok(orig) = main_clone.inner_size() {
                                let bumped = tauri::PhysicalSize::new(
                                    orig.width.saturating_add(1),
                                    orig.height,
                                );
                                let _ = main_clone.set_size(bumped);
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                let _ = main_clone.set_size(orig);
                                log::info!("[main] Linux nudge: focus + surface reactivation done");
                                // Reconcile: compositor may have coalesced the two
                                // set_size calls, leaving the window at width+1.
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                if let Ok(after) = main_clone.inner_size() {
                                    // Only correct the ±1px nudge artifact — if the
                                    // compositor or user resized the window significantly
                                    // during this window, don't clobber that change.
                                    let dw = if after.width > orig.width { after.width - orig.width } else { orig.width - after.width };
                                    let dh = if after.height > orig.height { after.height - orig.height } else { orig.height - after.height };
                                    if dw <= 1 && dh <= 1 && (dw > 0 || dh > 0) {
                                        let _ = main_clone.set_size(orig);
                                    }
                                }
                            }
                        });
                    }
                    if let Err(e) = main.show() {
                        log::warn!("[main] initial show failed: {e}");
                    }
                }
            }

            // 启动时主动弹 Accessibility 授权框（与 Swift `AppDelegate` 行为一致）。
            // 用户首次必看到系统提示；已授权则静默返回。
            #[cfg(target_os = "macos")]
            {
                let status = permissions::request_accessibility();
                log::info!("[startup] Accessibility status = {:?}", status);
            }

            // AppImage / 便携版：fcitx5 插件缺了就从 bundled resources 自动安装
            // 到 ~/.local/ 下面。不会覆盖系统已有的插件。
            #[cfg(target_os = "linux")]
            crate::linux_fcitx::ensure_plugin_installed(app.handle());

            // 菜单栏图标 — 与 Swift `MenuBarController` 同语义：
            // 左键点 → 显示/聚焦主窗口；菜单含「显示主窗口」「退出」。
            let tray_menu = build_tray_menu(app, &coordinator)?;
            let menu = tray_menu.menu;

            // 与 Swift `StatusBarIcon.swift` 行为一致：用全彩 AppIcon，**不**走 template 模式
            // （走 template 会被 macOS 染成单色 → 看起来像个黑方块）。
            if let Some(icon) = app.default_window_icon() {
                {
                    let state = app.state::<commands::TrayMicrophoneMenuState>();
                    *state.lock() = tray_menu.microphone_items;
                }
                let _tray = TrayIconBuilder::with_id("main-tray")
                    .icon(icon.clone())
                    .icon_as_template(false)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| match event.id.as_ref() {
                        "toggle" => show_main_window(app),
                        "quit" => app.exit(0),
                        id => {
                            if handle_style_tray_menu_event(app, id) {
                                return;
                            }
                            handle_microphone_tray_menu_event(app, id);
                        }
                    })
                    .on_tray_icon_event(move |tray, event| match event {
                        TrayIconEvent::Enter { .. } => {
                            if let Err(err) = refresh_tray_microphone_menu(tray.app_handle()) {
                                log::warn!("[tray] refresh microphone menu on hover failed: {err}");
                            }
                        }
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            ..
                        } => show_main_window(tray.app_handle()),
                        _ => {}
                    })
                    .build(app)?;
                start_tray_microphone_watcher(app.handle().clone());
            } else {
                log::warn!("[startup] default window icon missing; tray icon disabled");
            }

            // Spin up hotkey listener; coordinator owns the lifecycle.
            let app_handle = app.handle().clone();
            coordinator.bind_app(app_handle);
            coordinator.start_hotkey_listener();
            // QA / custom combo hotkeys use `global-hotkey` (Carbon on macOS).
            // Start those after RunEvent::Ready, when the AppKit event loop is live.
            if std::env::var("OPENLESS_SHOW_MAIN_ON_START").ok().as_deref() == Some("1") {
                show_main_window(app.handle());
            }

            // 首次启动也可能带 CLI flag（用户双击 .desktop 之前先用 CLI 起一遍）。
            // 等 coordinator 准备好后再 dispatch；GUI 仍然照常起来。
            let first_run_args: Vec<String> = std::env::args().collect();
            if let Some(intent) = cli::parse_cli_intent(&first_run_args) {
                log::info!("[startup] first-run CLI intent={intent:?}, dispatching");
                dispatch_cli_intent(app.handle(), intent);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::get_default_style_system_prompts,
            commands::set_settings,
            commands::get_update_channel,
            commands::set_update_channel,
            commands::fetch_latest_beta_release,
            commands::app_check_update_with_channel,
            commands::check_network,
            commands::get_hotkey_status,
            commands::get_hotkey_capability,
            commands::set_shortcut_recording_active,
            commands::get_windows_ime_status,
            commands::list_microphone_devices,
            commands::start_microphone_level_monitor,
            commands::stop_microphone_level_monitor,
            commands::get_credentials,
            commands::set_credential,
            commands::list_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::read_audio_recording,
            commands::retranscribe_history_entry,
            commands::marketplace_list,
            commands::marketplace_detail,
            commands::marketplace_install,
            commands::marketplace_upload,
            commands::marketplace_like,
            commands::marketplace_my_likes,
            commands::marketplace_my_packs,
            commands::marketplace_delete,
            commands::github_device_flow_start,
            commands::github_device_flow_poll,
            commands::list_vocab,
            commands::add_vocab,
            commands::remove_vocab,
            commands::set_vocab_enabled,
            commands::list_correction_rules,
            commands::add_correction_rule,
            commands::remove_correction_rule,
            commands::set_correction_rule_enabled,
            commands::list_vocab_presets,
            commands::save_vocab_presets,
            commands::start_dictation,
            commands::stop_dictation,
            commands::cancel_dictation,
            coding_agent::commands::coding_agent_detect,
            coding_agent::commands::coding_agent_run_test,
            coding_agent::commands::coding_agent_cancel_test,
            coding_agent::commands::coding_agent_command_risk,
            commands::handle_window_hotkey_event,
            #[cfg(debug_assertions)]
            commands::inject_hotkey_click_for_dev,
            commands::repolish,
            commands::list_style_packs,
            commands::create_style_pack_from_template,
            commands::save_style_pack,
            commands::preview_style_pack_runtime,
            commands::set_active_style_pack,
            commands::set_style_pack_enabled,
            commands::reset_builtin_style_pack,
            commands::delete_style_pack,
            commands::import_style_pack_from_zip,
            commands::export_style_pack_to_zip,
            commands::set_default_polish_mode,
            commands::set_style_enabled,
            commands::check_accessibility_permission,
            commands::request_accessibility_permission,
            commands::check_microphone_permission,
            commands::request_microphone_permission,
            commands::open_system_settings,
            commands::trigger_microphone_prompt,
            commands::read_credential,
            commands::set_active_asr_provider,
            commands::set_active_llm_provider,
            commands::get_qa_hotkey_label,
            commands::set_qa_hotkey,
            commands::validate_shortcut_binding,
            commands::set_dictation_hotkey,
            commands::set_translation_hotkey,
            commands::set_switch_style_hotkey,
            commands::set_open_app_hotkey,
            commands::qa_window_dismiss,
            commands::qa_window_pin,
            commands::less_computer_window_dismiss,
            commands::less_computer_window_resize,
            commands::less_computer_approve,
            commands::validate_combo_hotkey,
            commands::set_combo_hotkey,
            commands::validate_provider_credentials,
            commands::list_provider_models,
            commands::local_asr_get_settings,
            commands::local_asr_storage_settings,
            commands::local_asr_set_models_base_dir,
            commands::local_asr_set_active_model,
            commands::local_asr_set_mirror,
            commands::local_asr_list_models,
            commands::local_asr_fetch_remote_info,
            commands::local_asr_download_model,
            commands::local_asr_cancel_download,
            commands::local_asr_delete_model,
            commands::local_asr_model_dir,
            commands::local_asr_reveal_model_dir,
            commands::local_asr_reveal_models_root,
            commands::local_asr_test_model,
            commands::local_asr_engine_status,
            commands::local_asr_release_engine,
            commands::local_asr_preload,
            commands::local_asr_set_keep_loaded_secs,
            commands::foundry_local_asr_status,
            commands::foundry_local_asr_catalog,
            commands::foundry_local_asr_set_model,
            commands::foundry_local_asr_set_language_hint,
            commands::foundry_local_asr_set_runtime_source,
            commands::foundry_local_asr_prepare,
            commands::foundry_local_asr_cancel_prepare,
            commands::foundry_local_asr_release,
            commands::foundry_local_asr_model_dir,
            commands::foundry_local_asr_delete_model,
            commands::foundry_local_asr_reveal_model_dir,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_status,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_catalog,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_fetch_remote_info,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_download_model,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_cancel_download,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_set_model,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_set_language_hint,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_prepare,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_cancel_prepare,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_release,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_model_dir,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_delete_model,
            #[cfg(target_os = "windows")]
            commands::sherpa_onnx_asr_reveal_model_dir,
            commands::export_error_log,
            restart_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::Ready => {
                let coordinator = app.state::<Arc<coordinator::Coordinator>>();
                // 同步启动 QA hotkey listener。和 dictation hotkey 平行，互不抢状态。
                coordinator.start_qa_hotkey_listener();
                // 启动「快速 Agent」双热键监听（功能默认关闭，启用后才注册）。
                coordinator.start_coding_agent_hotkey_listener();
                // 启动自定义组合键监听器。当 trigger == Custom 时替代 modifier-only 监听器。
                coordinator.start_combo_hotkey_listener();
                coordinator.start_translation_hotkey_listener();
                coordinator.start_switch_style_hotkey_listener();
                coordinator.start_open_app_hotkey_listener();
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show_main_window(app),
            RunEvent::WindowEvent { label, event, .. } => {
                if label == "main" {
                    if let tauri::WindowEvent::CloseRequested { ref api, .. } = event {
                        api.prevent_close();
                        hide_main_window(app);
                    }
                }
            }
            RunEvent::Exit => {
                TRAY_MICROPHONE_WATCHER_STOPPING.store(true, Ordering::Relaxed);
                let coordinator = app.state::<Arc<coordinator::Coordinator>>();
                coordinator.stop_hotkey_listener();
                coordinator.stop_qa_hotkey_listener();
                coordinator.stop_coding_agent_hotkey_listener();
                coordinator.stop_combo_hotkey_listener();
                coordinator.stop_translation_hotkey_listener();
                coordinator.stop_switch_style_hotkey_listener();
                coordinator.stop_open_app_hotkey_listener();
            }
            _ => {}
        });
}

struct MicrophoneTrayMenu {
    submenu: Submenu<tauri::Wry>,
    items: Vec<commands::TrayMicrophoneMenuItem>,
}

struct StyleTrayMenu {
    submenu: Submenu<tauri::Wry>,
}

struct TrayMenu {
    menu: Menu<tauri::Wry>,
    microphone_items: Vec<commands::TrayMicrophoneMenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrayPolishModeMenuEntry {
    id: String,
    label: &'static str,
    mode: PolishMode,
    checked: bool,
}

fn tray_style_menu_enabled() -> bool {
    cfg!(target_os = "windows")
}

fn tray_polish_mode_menu_entries(selected: PolishMode) -> Vec<TrayPolishModeMenuEntry> {
    [
        (PolishMode::Raw, "style-raw"),
        (PolishMode::Light, "style-light"),
        (PolishMode::Structured, "style-structured"),
        (PolishMode::Formal, "style-formal"),
    ]
    .into_iter()
    .map(|(mode, id)| TrayPolishModeMenuEntry {
        id: id.to_string(),
        label: mode.display_name(),
        mode,
        checked: mode == selected,
    })
    .collect()
}

fn parse_tray_polish_mode_id(id: &str) -> Option<PolishMode> {
    match id {
        "style-raw" => Some(PolishMode::Raw),
        "style-light" => Some(PolishMode::Light),
        "style-structured" => Some(PolishMode::Structured),
        "style-formal" => Some(PolishMode::Formal),
        _ => None,
    }
}

fn build_tray_menu<M: Manager<tauri::Wry>>(
    app: &M,
    coordinator: &Arc<coordinator::Coordinator>,
) -> tauri::Result<TrayMenu> {
    let toggle = MenuItemBuilder::with_id("toggle", "显示主窗口").build(app)?;
    let microphone_menu = build_microphone_tray_menu(app, coordinator)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 OpenLess").build(app)?;
    let mut builder = MenuBuilder::new(app);
    let style_menu = if tray_style_menu_enabled() {
        Some(build_style_tray_menu(app, coordinator)?)
    } else {
        None
    };
    if let Some(style_menu) = &style_menu {
        builder = builder.item(&style_menu.submenu);
    }
    let menu = builder
        .items(&[&toggle, &microphone_menu.submenu, &quit])
        .build()?;
    Ok(TrayMenu {
        menu,
        microphone_items: microphone_menu.items,
    })
}

fn build_style_tray_menu<M: Manager<tauri::Wry>>(
    app: &M,
    coordinator: &Arc<coordinator::Coordinator>,
) -> tauri::Result<StyleTrayMenu> {
    let prefs = coordinator.prefs().get();
    let selected = coordinator
        .style_packs()
        .get_or_default_active(&prefs.active_style_pack_id)
        .map(|pack| pack.base_mode)
        .unwrap_or(prefs.default_mode);
    let mut submenu = SubmenuBuilder::with_id(app, "style", "输出风格");
    for entry in tray_polish_mode_menu_entries(selected) {
        let item = CheckMenuItemBuilder::with_id(&entry.id, entry.label)
            .checked(entry.checked)
            .build(app)?;
        submenu = submenu.item(&item);
    }
    Ok(StyleTrayMenu {
        submenu: submenu.build()?,
    })
}

fn build_microphone_tray_menu<M: Manager<tauri::Wry>>(
    app: &M,
    coordinator: &Arc<coordinator::Coordinator>,
) -> tauri::Result<MicrophoneTrayMenu> {
    let selected = coordinator.prefs().get().microphone_device_name;
    let mut items = Vec::new();
    let mut submenu = SubmenuBuilder::with_id(app, "microphone", "选择麦克风");
    let devices = match recorder::list_input_devices() {
        Ok(devices) => devices,
        Err(err) => {
            log::warn!("[tray] list microphone devices failed: {err}");
            Vec::new()
        }
    };
    let selected_available =
        selected.trim().is_empty() || devices.iter().any(|device| device.name == selected);

    let default_item = CheckMenuItemBuilder::with_id("mic-default", "系统默认麦克风")
        .checked(selected.trim().is_empty() || !selected_available)
        .build(app)?;
    submenu = submenu.item(&default_item);
    items.push(commands::TrayMicrophoneMenuItem {
        id: "mic-default".to_string(),
        device_name: String::new(),
        item: default_item,
    });

    if devices.is_empty() {
        let empty = MenuItemBuilder::with_id("mic-empty", "未发现麦克风")
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&empty);
    } else {
        for (index, device) in devices.into_iter().enumerate() {
            let id = format!("mic-device-{index}");
            let label = if device.is_default {
                format!("{}（系统默认）", device.name)
            } else {
                device.name.clone()
            };
            let item = CheckMenuItemBuilder::with_id(&id, label)
                .checked(selected == device.name)
                .build(app)?;
            submenu = submenu.item(&item);
            items.push(commands::TrayMicrophoneMenuItem {
                id,
                device_name: device.name,
                item,
            });
        }
    }

    Ok(MicrophoneTrayMenu {
        submenu: submenu.build()?,
        items,
    })
}

pub(crate) fn refresh_tray_microphone_menu(app: &AppHandle) -> tauri::Result<()> {
    let coordinator = app.state::<Arc<coordinator::Coordinator>>();
    let tray_menu = build_tray_menu(app, &coordinator)?;
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_menu(Some(tray_menu.menu))?;
    }
    let state = app.state::<commands::TrayMicrophoneMenuState>();
    *state.lock() = tray_menu.microphone_items;
    Ok(())
}

fn microphone_device_signature() -> Option<Vec<(String, bool)>> {
    match recorder::list_input_devices() {
        Ok(devices) => Some(
            devices
                .into_iter()
                .map(|device| (device.name, device.is_default))
                .collect(),
        ),
        Err(err) => {
            log::warn!("[tray] watch microphone devices failed: {err}");
            None
        }
    }
}

fn start_tray_microphone_watcher(app: AppHandle) {
    TRAY_MICROPHONE_WATCHER_STOPPING.store(false, Ordering::Relaxed);
    if let Err(err) = std::thread::Builder::new()
        .name("openless-tray-mic-watch".into())
        .spawn(move || {
            let mut last_signature = microphone_device_signature();
            while !TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1500));
                if TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                    break;
                }
                let signature = microphone_device_signature();
                if signature == last_signature {
                    continue;
                }
                last_signature = signature;
                let app = app.clone();
                let refresh_app = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(err) = refresh_tray_microphone_menu(&refresh_app) {
                        log::warn!(
                            "[tray] refresh microphone menu after device change failed: {err}"
                        );
                    }
                    let _ = refresh_app.emit("microphone:devices-changed", serde_json::json!({}));
                });
            }
        })
    {
        log::warn!("[tray] start microphone watcher failed: {err}");
    }
}

fn handle_microphone_tray_menu_event(app: &AppHandle, id: &str) {
    let tray_items = app.state::<commands::TrayMicrophoneMenuState>();
    let items = tray_items.lock();
    let Some(selected) = items.iter().find(|item| item.id == id) else {
        return;
    };

    let coord = app.state::<Arc<coordinator::Coordinator>>();
    let mut prefs = coord.prefs().get();
    prefs.microphone_device_name = selected.device_name.clone();
    if let Err(err) = coord.prefs().set(prefs.clone()) {
        log::warn!("[tray] save microphone preference failed: {err}");
        return;
    }
    let _ = app.emit("prefs:changed", &prefs);

    commands::sync_tray_microphone_selection(&items, &selected.device_name);
}

fn handle_style_tray_menu_event(app: &AppHandle, id: &str) -> bool {
    let Some(mode) = parse_tray_polish_mode_id(id) else {
        return false;
    };
    let coord = app.state::<Arc<coordinator::Coordinator>>();
    if let Err(err) = commands::activate_builtin_style_mode(&coord, app, mode) {
        log::warn!("[tray] activate builtin style mode failed: {err}");
        return true;
    }
    if let Err(err) = refresh_tray_microphone_menu(app) {
        log::warn!("[tray] refresh style menu after polish mode change failed: {err}");
    }
    true
}

/// 把 Win11 原生标题栏底色刷成白色，与应用 sidebar 视觉统一。需要 Win11 22H2+
/// (Build 22621+) 才支持 `DWMWA_CAPTION_COLOR`(35)；老 Windows 上 DwmSetWindowAttribute
/// 返回错误，仅打 warn 不阻塞启动。
#[cfg(target_os = "windows")]
fn apply_windows_caption_color<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR};

    let handle = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle,
        Ok(other) => {
            log::warn!("[main] unexpected raw window handle for caption color: {other:?}");
            return;
        }
        Err(e) => {
            log::warn!("[main] read raw window handle for caption color failed: {e}");
            return;
        }
    };
    let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);

    // COLORREF 0x00BBGGRR 编码——选用 rgb(245,245,247) 跟 WindowChrome 的 glass linear-gradient
    // 起始色一致，减小原生 caption bar 跟应用磨砂玻璃的色差（用户反馈：纯白 caption + 半透灰 glass
    // 色差很丑）。R=0xF5 G=0xF5 B=0xF7 → COLORREF = 0x00F7F5F5。
    let glass_match: u32 = 0x00F7F5F5;
    unsafe {
        if let Err(e) = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &glass_match as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&glass_match) as u32,
        ) {
            log::warn!("[main] set caption color failed (likely pre-22H2 Win): {e}");
        }
    }
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    // macOS：自动更新会让新装的 .app 带 com.apple.quarantine（无论 Tauri updater
    // 怎么解包，下载流由 LaunchServices 接管，输出物可能仍带 xattr）。如果不
    // strip，重启后 Gatekeeper 会拦着说"OpenLess 已损坏 / 来自未识别开发者"，
    // 用户必须自己开终端跑 xattr -cr 才能继续用 — 违反了"自动更新对用户应该零摩擦"。
    //
    // 在 restart 前阻塞地清一次 xattr。失败容忍（PATH 异常、xattr 不存在、磁盘
    // 只读等边角情况），不让它阻塞重启本身。
    #[cfg(target_os = "macos")]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = exe
            .ancestors()
            .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        {
            let _ = std::process::Command::new("/usr/bin/xattr")
                .arg("-cr")
                .arg(bundle)
                .status();
            log::info!("[updater] stripped xattr on {:?} before restart", bundle);
        }
    }
    #[cfg(target_os = "macos")]
    reset_tcc_for_beta_restart();
    app.restart();
}

#[cfg(target_os = "macos")]
fn reset_tcc_for_beta_restart() {
    if !is_beta_build() {
        log::info!("[updater] skipping TCC reset before stable restart");
        return;
    }

    // Beta builds are currently ad-hoc signed. Their code hash changes across builds, so
    // old TCC rows can leave System Settings checked while AXIsProcessTrusted() is false.
    reset_tcc_service_for_beta_restart("Accessibility");
    reset_tcc_service_for_beta_restart("Microphone");
}

#[cfg(target_os = "macos")]
fn is_beta_build() -> bool {
    env!("CARGO_PKG_VERSION").contains('-')
}

#[cfg(target_os = "macos")]
fn reset_tcc_service_for_beta_restart(service: &str) {
    match std::process::Command::new("/usr/bin/tccutil")
        .args(["reset", service, OPENLESS_BUNDLE_ID])
        .status()
    {
        Ok(status) if status.success() => {
            log::info!("[updater] reset TCC {service} before beta restart");
        }
        Ok(status) => {
            log::warn!("[updater] reset TCC {service} before beta restart exited with {status}");
        }
        Err(e) => {
            log::warn!("[updater] reset TCC {service} before beta restart failed: {e}");
        }
    }
}

/// 把日志同时写到 stderr + ~/Library/Logs/OpenLess/openless.log（match Swift `Log.swift`）。
fn init_file_logger() {
    use simplelog::{
        ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TermLogger, TerminalMode,
        WriteLogger,
    };
    let log_dir = log_dir_path();
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = log_dir.join("openless.log");
    if let Err(e) = rotate_log_if_too_large(&log_file) {
        eprintln!("[logger] WARN 日志轮转失败: {e}");
    }
    let config = ConfigBuilder::new().set_time_format_rfc3339().build();
    let mut loggers: Vec<Box<dyn simplelog::SharedLogger>> = vec![TermLogger::new(
        LevelFilter::Info,
        config.clone(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )];
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        loggers.push(WriteLogger::new(LevelFilter::Info, config, file));
    }
    let _ = CombinedLogger::init(loggers);
}

fn rotate_log_if_too_large(path: &std::path::Path) -> std::io::Result<()> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() <= LOG_ROTATE_LIMIT_BYTES {
        return Ok(());
    }

    let archive = path.with_file_name("openless.log.1");
    match std::fs::remove_file(&archive) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::rename(path, archive)
}

pub fn log_dir_path() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("OpenLess");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return std::path::PathBuf::from(local)
                .join("OpenLess")
                .join("Logs");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("OpenLess")
                .join("logs");
        }
    }
    std::env::temp_dir().join("OpenLess")
}

pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    activate_window_mode(app);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    activate_app(app);
}

/// 把 CLI intent 路由到 coordinator。两个入口共用：
/// 1. 首次启动（lib.rs setup 末尾）
/// 2. single-instance 回调（第二个进程被拦截后转发 argv）
///
/// 异步动作（start_dictation / stop_dictation 是 async）通过 tauri 自带 runtime spawn，
/// 不阻塞回调线程。所有动作都按 coordinator 当前状态自检：
/// - ToggleDictation 在 Idle → start，在 Listening → stop，Starting/Processing/Inserting 忽略并记日志
/// - ToggleQa 直接转发到 handle_qa_hotkey_pressed（语义等同于按一次 QA 热键）
/// - CancelDictation 直接调 cancel（cancel 本身在非 Listening 时也安全）
fn dispatch_cli_intent<R: Runtime>(app: &AppHandle<R>, intent: cli::CliIntent) {
    let coordinator = app
        .try_state::<Arc<coordinator::Coordinator>>()
        .map(|s| Arc::clone(&*s));
    let Some(coordinator) = coordinator else {
        log::warn!("[cli] coordinator not yet managed; dropping intent={intent:?}");
        return;
    };
    match intent {
        cli::CliIntent::ToggleDictation => {
            let coord = Arc::clone(&coordinator);
            tauri::async_runtime::spawn(async move {
                let phase = coord.dictation_phase_for_cli();
                use coordinator_state::SessionPhase;
                match phase {
                    SessionPhase::Idle => {
                        log::info!("[cli] toggle-dictation: Idle → start_dictation");
                        if let Err(e) = coord.start_dictation().await {
                            log::warn!("[cli] start_dictation failed: {e}");
                        }
                    }
                    SessionPhase::Listening => {
                        log::info!("[cli] toggle-dictation: Listening → stop_dictation");
                        if let Err(e) = coord.stop_dictation().await {
                            log::warn!("[cli] stop_dictation failed: {e}");
                        }
                    }
                    SessionPhase::Starting => {
                        // 复用 stop_dictation 自身的 Starting → pending_stop 处理，
                        // 与按一次主热键的行为对齐（issue #51）。
                        log::info!("[cli] toggle-dictation: Starting → stop_dictation (pending)");
                        if let Err(e) = coord.stop_dictation().await {
                            log::warn!("[cli] stop_dictation failed: {e}");
                        }
                    }
                    other => {
                        log::info!("[cli] toggle-dictation ignored (phase={other:?})");
                    }
                }
            });
        }
        cli::CliIntent::ToggleQa => {
            let coord = Arc::clone(&coordinator);
            tauri::async_runtime::spawn(async move {
                log::info!("[cli] toggle-qa: dispatching to qa hotkey handler");
                coord.cli_toggle_qa_panel().await;
            });
        }
        cli::CliIntent::CancelDictation => {
            log::info!("[cli] cancel-dictation: invoking cancel");
            coordinator.cancel_dictation();
        }
    }
}

pub(crate) fn request_microphone_from_foreground<R: Runtime>(
    app: &AppHandle<R>,
) -> permissions::PermissionStatus {
    show_main_window(app);
    wait_for_app_activation(app);
    permissions::request_microphone()
}

fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    activate_menu_bar_mode(app);
}

#[cfg(target_os = "macos")]
fn activate_window_mode<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let _ = app.set_dock_visibility(true);
    let _ = app.show();
}

#[cfg(not(target_os = "macos"))]
fn activate_window_mode<R: Runtime>(_app: &AppHandle<R>) {}

#[cfg(target_os = "macos")]
fn activate_menu_bar_mode<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let _ = app.set_dock_visibility(false);
}

#[cfg(not(target_os = "macos"))]
fn activate_menu_bar_mode<R: Runtime>(_app: &AppHandle<R>) {}

#[cfg(target_os = "macos")]
fn activate_app<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.run_on_main_thread(|| {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject, Bool};

        unsafe {
            let Some(cls) = AnyClass::get("NSApplication") else {
                return;
            };
            let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
            if !ns_app.is_null() {
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: Bool::YES];
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn activate_app<R: Runtime>(_app: &AppHandle<R>) {}

/// 展示胶囊后调用：若 OpenLess 已是前台 app，用 makeKeyWindow 还原主窗口焦点。
/// 不调 NSApp.activate，不抢其他 app 焦点，符合 CLAUDE.md 约束。
#[cfg(target_os = "macos")]
pub(crate) fn restore_main_window_key_if_active<R: Runtime>(app: &AppHandle<R>) {
    let main = app.get_webview_window("main");
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject, Bool};
        unsafe {
            let Some(cls) = AnyClass::get("NSApplication") else {
                return;
            };
            let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
            if ns_app.is_null() {
                return;
            }
            let is_active: Bool = msg_send![ns_app, isActive];
            if !is_active.as_bool() {
                return;
            }
            let Some(main) = main else {
                return;
            };
            match main.ns_window() {
                Ok(handle) => {
                    let main_win = handle as *mut AnyObject;
                    if !main_win.is_null() {
                        let _: () = msg_send![main_win, makeKeyWindow];
                    }
                }
                Err(e) => log::warn!("[main] ns_window unavailable for key restore: {e}"),
            };
        }
    });
}

#[cfg(target_os = "macos")]
fn wait_for_app_activation<R: Runtime>(app: &AppHandle<R>) {
    let (tx, rx) = mpsc::channel();
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject, Bool};

        unsafe {
            let Some(cls) = AnyClass::get("NSApplication") else {
                let _ = tx.send(());
                return;
            };
            let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
            if !ns_app.is_null() {
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: Bool::YES];
            }
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(800));
    std::thread::sleep(Duration::from_millis(150));
}

#[cfg(not(target_os = "macos"))]
fn wait_for_app_activation<R: Runtime>(_app: &AppHandle<R>) {}

/// QA 浮窗的目标尺寸（issue #118）。胶囊默认 220×96 + Dock 80pt + 8pt gap，
/// 算下来 QA 窗口顶部坐标 = h - 80 - 96 - 8 - 280。
const QA_WINDOW_WIDTH: f64 = 380.0;
const QA_WINDOW_HEIGHT: f64 = 440.0;
/// 胶囊与 QA 窗口的间距，与设计稿一致。
const QA_WINDOW_GAP_TO_CAPSULE: f64 = 8.0;
/// 给 macOS Dock 留的下边距（与 capsule 同源）。
const DOCK_BOTTOM_PADDING_FOR_QA: f64 = 80.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LogicalMonitorFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn logical_monitor_frame(
    physical_x: i32,
    physical_y: i32,
    physical_width: u32,
    physical_height: u32,
    scale: f64,
) -> LogicalMonitorFrame {
    let scale = scale.max(0.1);
    LogicalMonitorFrame {
        x: physical_x as f64 / scale,
        y: physical_y as f64 / scale,
        width: physical_width as f64 / scale,
        height: physical_height as f64 / scale,
    }
}

fn bottom_center_position(
    frame: LogicalMonitorFrame,
    window_width: f64,
    window_height: f64,
    bottom_offset: f64,
) -> (f64, f64) {
    let x = frame.x + ((frame.width - window_width) / 2.0).max(0.0);
    let y = frame.y + (frame.height - bottom_offset - window_height).max(0.0);
    (x, y)
}

fn bottom_visual_position(
    frame: LogicalMonitorFrame,
    window_width: f64,
    visual_height: f64,
    bottom_padding: f64,
    bottom_inset: f64,
) -> (f64, f64) {
    let x = frame.x + ((frame.width - window_width) / 2.0).max(0.0);
    let y = frame.y + (frame.height - visual_height - bottom_padding - bottom_inset).max(0.0);
    (x, y)
}

/// 把窗口左上角 `(x, y)`（同 area 同坐标系，physical px）夹到给定矩形内，
/// **保证整窗（含自身 w×h）落在 area 内可见**。area 为工作区时即可避开任务栏。
///
/// 纯函数，无 Win32 依赖，便于单测多显示器 / 负原点 / 异常 DPI 输入。issue #470：
/// 此前 Windows 分支只夹上边（`y.max(mon.top)`），左/右/下未夹，多屏负坐标下胶囊
/// 可能被算到屏外却无任何观测。这里四边都夹。
///
/// area 比窗口还小时（`area_right - w < area_left`），`max_x` 退化为 `area_left`，
/// `clamp` 把左上角收回 area 左上角，保证至少左上角可见、不溢出为负超界。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn clamp_to_monitor(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    area_left: i32,
    area_top: i32,
    area_right: i32,
    area_bottom: i32,
) -> (i32, i32) {
    // 右/下边界 = area 右下角减去窗口自身尺寸，确保整窗可见。
    // 用 saturating_sub 防 area_right/area_bottom 为极小（含 i32::MIN 近邻）时减法溢出。
    let max_x = area_right.saturating_sub(w).max(area_left);
    let max_y = area_bottom.saturating_sub(h).max(area_top);
    let clamped_x = x.clamp(area_left, max_x);
    let clamped_y = y.clamp(area_top, max_y);
    (clamped_x, clamped_y)
}

/// 把 QA 浮窗放到屏幕底部居中、紧贴胶囊上方。tauri 启动期 + show 之前都会调一次，
/// 防止用户切换显示器后位置错乱。
fn position_qa_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let pos = monitor.position();
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let capsule_height = capsule_height_for_qa();
    let (x, y) = bottom_center_position(
        frame,
        QA_WINDOW_WIDTH,
        QA_WINDOW_HEIGHT,
        DOCK_BOTTOM_PADDING_FOR_QA + capsule_height + QA_WINDOW_GAP_TO_CAPSULE,
    );
    window.set_size(tauri::LogicalSize::new(QA_WINDOW_WIDTH, QA_WINDOW_HEIGHT))?;
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// 显示 QA 窗口并发一条状态事件（前端订阅 `qa:state`）。
/// `content_kind` 是不透明字符串（"loading" / "answer" / "idle" 等），
/// 让前端 React 视图自行决定渲染哪一种。
///
/// ## 跨端焦点契约（#164 / #466，三端**有意**不同，勿盲目对齐）
/// - **macOS**：`orderFrontRegardless` —— 窗口可见但不成为 key window，frontmost
///   始终是用户原 app，AX / Cmd+C fallback 能直接读到选区。**全程不抢焦点**。
/// - **Windows**：`show_qa_window_no_activate` 实际是 `show()` + `set_focus()`，
///   出现的那一帧会短暂抢前台。这是 #466 对 #164 的**有意取舍**：WebView2 子窗口有
///   独立 focus 模型，不主动抓焦点则 QA webview 收不到键盘事件 → ESC 到不了 React
///   监听、X 按钮 first-click 被 OS 当激活点击吃掉。代价由 `coordinator/qa.rs` 的
///   focus-dance 补偿：抓选区前用 `qa_focus_target` 把焦点临时还给用户原 app，
///   `simulate_copy` 跑完再 `refocus_qa_window` 收回。**移除 set_focus 会同时回归
///   #164 的反面（ESC/X 失效），别删。** 详见 `show_qa_window_no_activate` 内注释。
/// - **Linux**：`window.show()`。qa 窗口静态配置 `focus: false`（tauri.conf.json），
///   Tauri 将其建成非激活窗口，因此 show() **不抢焦点**，与 macOS 契约一致。
pub(crate) fn show_qa_window<R: tauri::Runtime>(app: &AppHandle<R>, content_kind: &str) {
    let Some(window) = app.get_webview_window("qa") else {
        log::info!("[qa] show 跳过：qa 窗口不存在 (content_kind={content_kind})");
        return;
    };
    // 仅首次 show 时居中；之后保留用户拖动后的位置。
    if !QA_WINDOW_POSITIONED.load(Ordering::Relaxed) {
        if let Err(e) = position_qa_window(&window) {
            log::warn!("[qa] position before first show failed: {e}");
        }
        QA_WINDOW_POSITIONED.store(true, Ordering::Relaxed);
    }
    // macOS：不用 window.show()（它会 makeKeyAndOrderFront 把 OpenLess 推成 frontmost，
    // 之后 capture_selection 的 AX read / Cmd+C fallback 都跑在 OpenLess 自己的 webview 上
    // → 抓不到原 app 选区）。改用 orderFrontRegardless 让窗口可见但**不**成为 key window，
    // frontmost 仍是用户原 app，AX 还能读到选区。这是 Spotlight / Raycast 的标准做法。
    //
    // ⚠️ 关键：NSWindow 任何操作必须在主线程，macOS 26 是硬断言（违反直接 SIGTRAP）。
    // show_qa_window 经常从 tokio worker 调（qa_hotkey_bridge_loop），所以裸 ObjC msg_send
    // 必须用 `app.run_on_main_thread` dispatch 到主线程。详见 issue #118 v2。
    #[cfg(target_os = "macos")]
    {
        let window_clone = window.clone();
        let _ = app.run_on_main_thread(move || {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            match window_clone.ns_window() {
                Ok(handle) => {
                    let ns = handle as *mut AnyObject;
                    if ns.is_null() {
                        log::warn!("[qa] ns_window null; falling back to window.show()");
                        let _ = window_clone.show();
                    } else {
                        unsafe {
                            let _: () = msg_send![ns, orderFrontRegardless];
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[qa] ns_window unavailable: {e}; falling back to window.show()");
                    let _ = window_clone.show();
                }
            }
        });
    }
    #[cfg(target_os = "windows")]
    if !show_qa_window_no_activate(&window) {
        log::warn!("[qa] show_no_activate failed; falling back to window.show()");
        if let Err(e) = window.show() {
            log::warn!("[qa] show fallback failed: {e}");
        }
    }
    // Linux：qa 窗口静态配置 focus:false → Tauri 建成非激活窗口，window.show() 不抢
    // 焦点，与 macOS「不抢焦点」契约一致（无需 Windows 那套 set_focus + focus-dance）。
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    if let Err(e) = window.show() {
        log::warn!("[qa] show failed: {e}");
    }
    let _ = app.emit_to(
        "qa",
        "qa:state",
        serde_json::json!({ "kind": content_kind }),
    );
}

/// QA 浮窗的拖动修复（macOS）。
///
/// 配置 `focus: false` 让 Tauri 把窗口创建为 nonactivating panel 风格（避免抢前台 app
/// 焦点）。代价是 AppKit 的 `performWindowDragWithEvent:` 在 nonactivating 窗口上无效，
/// 所以 `data-tauri-drag-region` 和 `WebviewWindow::start_dragging()` 都拖不动。
///
/// 解法是把 NSWindow 的 `movableByWindowBackground` 打开——这条路径不依赖窗口是否成为
/// key window，跟 Spotlight / Raycast 的浮窗是同一手法。设一次就够，整个生命周期保持。
#[cfg(target_os = "macos")]
fn make_qa_window_draggable_macos<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};
    let Ok(handle) = window.ns_window() else {
        log::warn!("[qa] ns_window unavailable; drag fix skipped");
        return;
    };
    let ns_window = handle as *mut AnyObject;
    if ns_window.is_null() {
        log::warn!("[qa] ns_window null; drag fix skipped");
        return;
    }
    unsafe {
        let _: () = msg_send![ns_window, setMovableByWindowBackground: Bool::YES];
        let _: () = msg_send![ns_window, setMovable: Bool::YES];
    }
    log::info!("[qa] NSWindow movableByWindowBackground=YES");
}

/// 隐藏 QA 窗口。供 commands::qa_window_dismiss / coordinator session 收尾共用。
pub(crate) fn hide_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("qa") {
        let _ = window.hide();
    }
}

// ───────────────────────── Less Computer 浮窗 ─────────────────────────
//
// Less Computer 语音 Agent 的聊天浮窗（窗口 label = "less-computer"）。
// 仅 macOS：和 coordinator / 前端对 Less Computer 的 gating 一致（Windows/Linux
// 不注册热键、前端 detectOS 不渲染入口），所以这些窗口操作全部 `#[cfg(macos)]`，
// 其它平台是 no-op，避免在非目标平台动 NSWindow / 弹一个空浮窗。

/// Less Computer 浮窗宽度（高度由前端按内容自适应，经 `less_computer_window_resize`
/// 回传，Rust 端按 bottom-anchored 重新摆放，让内容增长向上撑开）。
#[cfg(target_os = "macos")]
const LESS_COMPUTER_WINDOW_WIDTH: f64 = 400.0;
#[cfg(target_os = "macos")]
const LESS_COMPUTER_WINDOW_MIN_HEIGHT: f64 = 120.0;
#[cfg(target_os = "macos")]
const LESS_COMPUTER_WINDOW_MAX_HEIGHT: f64 = 520.0;

/// 把 Less Computer 浮窗按给定高度（clamp 到 [min,max]）摆到屏幕底部居中、
/// 紧贴胶囊上方。bottom 对齐胶囊顶部，所以高度变化时窗口向上生长。
#[cfg(target_os = "macos")]
fn position_less_computer_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    height: f64,
) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let pos = monitor.position();
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let height = height.clamp(
        LESS_COMPUTER_WINDOW_MIN_HEIGHT,
        LESS_COMPUTER_WINDOW_MAX_HEIGHT,
    );
    let capsule_height = capsule_height_for_qa();
    let (x, y) = bottom_center_position(
        frame,
        LESS_COMPUTER_WINDOW_WIDTH,
        height,
        DOCK_BOTTOM_PADDING_FOR_QA + capsule_height + QA_WINDOW_GAP_TO_CAPSULE,
    );
    window.set_size(tauri::LogicalSize::new(LESS_COMPUTER_WINDOW_WIDTH, height))?;
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// 显示 Less Computer 浮窗（不抢前台 app 焦点，与 QA 同手法）。`macos` 专用。
#[cfg(target_os = "macos")]
pub(crate) fn show_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("less-computer") else {
        log::info!("[less-computer] show 跳过：窗口不存在");
        return;
    };
    if let Err(e) = position_less_computer_window(&window, LESS_COMPUTER_WINDOW_MIN_HEIGHT) {
        log::warn!("[less-computer] position before show failed: {e}");
    }
    let window_clone = window.clone();
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        match window_clone.ns_window() {
            Ok(handle) => {
                let ns = handle as *mut AnyObject;
                if ns.is_null() {
                    log::warn!("[less-computer] ns_window null; falling back to window.show()");
                    let _ = window_clone.show();
                } else {
                    unsafe {
                        let _: () = msg_send![ns, orderFrontRegardless];
                    }
                }
            }
            Err(e) => {
                log::warn!("[less-computer] ns_window unavailable: {e}; falling back to show()");
                let _ = window_clone.show();
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 隐藏 Less Computer 浮窗。供 dismiss 命令 / session 收尾共用。
#[cfg(target_os = "macos")]
pub(crate) fn hide_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("less-computer") {
        let _ = window.hide();
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 显示全屏彩虹描边浮层：盖满当前显示器、点击穿透、置顶。Agent 工作时点亮整屏边缘。
#[cfg(target_os = "macos")]
pub(crate) fn show_less_computer_glow<R: tauri::Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("less-computer-glow") else {
        return;
    };
    // 盖满当前（否则主）显示器，含菜单栏/Dock 区域。关键：用「逻辑坐标」(物理/缩放) ——
    // Retina 上 monitor.size() 是物理像素(2x)，直接 set_size 会把窗口铺成两倍、错位、不贴边。
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    // 逻辑坐标的「铺满整屏」矩形 (x, y, w, h)。f64 元组可 Copy：既在 show 前先铺一次，
    // 也在主线程 realize（orderFront）后再铺一次（见下，修首次半屏 bug）。
    let bounds: Option<(f64, f64, f64, f64)> = monitor.map(|m| {
        let scale = m.scale_factor();
        let size = m.size();
        let pos = m.position();
        (
            pos.x as f64 / scale,
            pos.y as f64 / scale,
            size.width as f64 / scale,
            size.height as f64 / scale,
        )
    });
    if let Some((x, y, w, h)) = bounds {
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
        let _ = window.set_size(tauri::LogicalSize::new(w, h));
    }
    // 点击穿透：纯视觉浮层，绝不拦截鼠标。
    let _ = window.set_ignore_cursor_events(true);
    let window_clone = window.clone();
    let _ = app.run_on_main_thread(move || {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        match window_clone.ns_window() {
            Ok(handle) => {
                let ns = handle as *mut AnyObject;
                if ns.is_null() {
                    let _ = window_clone.show();
                } else {
                    unsafe {
                        // 抬到菜单栏(24)/Dock 之上，让描边能真正贴到屏幕最外缘（含顶部菜单栏区域）。
                        let _: () = msg_send![ns, setLevel: 25i64];
                        // 所有 Space 都显示、不参与窗口循环、全屏 app 上也叠加。
                        let _: () = msg_send![ns, setCollectionBehavior: 273u64];
                        let _: () = msg_send![ns, setIgnoresMouseEvents: true];
                        let _: () = msg_send![ns, orderFrontRegardless];
                    }
                }
            }
            Err(_) => {
                let _ = window_clone.show();
            }
        }
        // 首次使用彩虹边框只画半屏并卡住：glow 窗口 conf 初始 800×600 且 visible:false，
        // 首次 show 前从未 realize —— current_monitor() 取不到 / show 前的 set_size 没贴住整屏，
        // webview 首帧按 800×600 画出半屏描边。这里在 realize（orderFront）之后**再铺满一次**，
        // 强制 webview 按整屏重排重绘。后续使用窗口已 realize，show 前那次就够、不闪。
        if let Some((x, y, w, h)) = bounds {
            let _ = window_clone.set_position(tauri::LogicalPosition::new(x, y));
            let _ = window_clone.set_size(tauri::LogicalSize::new(w, h));
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_less_computer_glow<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 隐藏全屏彩虹描边浮层。
#[cfg(target_os = "macos")]
pub(crate) fn hide_less_computer_glow<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("less-computer-glow") {
        let _ = window.hide();
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_less_computer_glow<R: tauri::Runtime>(_app: &AppHandle<R>) {}

/// 前端按内容测高后回传。以「当前窗口底边」为锚向上生长——只改高度、保住用户拖动后的位置，
/// 不再重新居中（否则一改内容就把拖走的框拉回屏幕底部中间）。`macos` 专用。
#[cfg(target_os = "macos")]
pub(crate) fn resize_less_computer_window<R: tauri::Runtime>(app: &AppHandle<R>, height: f64) {
    let Some(window) = app.get_webview_window("less-computer") else {
        return;
    };
    let height = height.clamp(
        LESS_COMPUTER_WINDOW_MIN_HEIGHT,
        LESS_COMPUTER_WINDOW_MAX_HEIGHT,
    );
    let scale = window.scale_factor().unwrap_or(1.0);
    match (window.outer_position(), window.outer_size()) {
        (Ok(pos), Ok(size)) => {
            let x = pos.x as f64 / scale;
            let cur_top = pos.y as f64 / scale;
            let cur_h = size.height as f64 / scale;
            let bottom = cur_top + cur_h;
            let monitor_top = window
                .current_monitor()
                .ok()
                .flatten()
                .map(|m| {
                    let p = m.position();
                    let s = m.size();
                    logical_monitor_frame(p.x, p.y, s.width, s.height, m.scale_factor()).y
                })
                .unwrap_or(f64::NEG_INFINITY);
            let new_y = (bottom - height).max(monitor_top);
            let _ = window.set_size(tauri::LogicalSize::new(LESS_COMPUTER_WINDOW_WIDTH, height));
            let _ = window.set_position(tauri::LogicalPosition::new(x, new_y));
        }
        // 拿不到当前位置（极少见）→ 退回首屏居中摆放。
        _ => {
            if let Err(e) = position_less_computer_window(&window, height) {
                log::warn!("[less-computer] resize fallback failed: {e}");
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn resize_less_computer_window<R: tauri::Runtime>(_app: &AppHandle<R>, _height: f64) {}

/// 抓完选区后把焦点重新交回 QA 浮窗（Windows focus-dance 下半场）。begin_qa_session
/// 在 capture_selection 跑完时调；非 Windows 平台是 no-op。issue #466。
#[cfg(target_os = "windows")]
pub(crate) fn refocus_qa_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("qa") {
        let _ = show_qa_window_no_activate(&window);
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn refocus_qa_window<R: tauri::Runtime>(_app: &AppHandle<R>) {}

#[cfg(target_os = "windows")]
fn show_qa_window_no_activate<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    // 函数名沿用历史命名，实际行为已切到「show + focus」—— 让 QA webview 真正拿到键盘
    // 焦点，ESC 才能到 React 监听、X 按钮 first-click 才不会被 OS 当作激活点击吃掉。
    //
    // 走 Tauri 的 show() / set_focus() 而不是 Win32 SetForegroundWindow + SetFocus
    // 的原因（pr_agent 关注点二轮回应）：
    //   - 直接 SetFocus(host_hwnd) 不保证 WebView2 child 收键盘事件，WebView2 子窗口
    //     有自己的 focus 模型。Tauri 内部走 webview 专用路径，能把焦点真正送到 webview。
    //   - SetForegroundWindow 在 Win11 focus-stealing prevention 下可能被拒。Tauri
    //     2.x 在跨平台 abstraction 里做了兜底（按 SPI 临时调整 / attach input queue）。
    //
    // 对 issue #164 "QA 浮窗不抢前台 app 焦点"的取舍：浮窗出现时会短暂成为前台，
    // 但 begin_qa_session 抓选区前 focus-dance 会把焦点临时还给用户原 app（见
    // coordinator.rs 同 issue 注释），抓完再 refocus_qa_window 收回 —— 选区路径
    // 仍能正常工作，issue #164 在「QA 出现的那一帧」短暂被违背是 #466 修复的代价。
    if window.show().is_err() {
        return false;
    }
    let _ = window.set_focus();
    true
}

/// 输入目标显示器的物理矩形（虚拟桌面坐标）+ DPI 缩放。
#[cfg(target_os = "windows")]
pub(crate) struct ForegroundMonitor {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
    /// 工作区矩形（physical px，去掉任务栏）。多端一致：胶囊优先夹到工作区内，
    /// 避免压住任务栏。取不到时回退为整屏矩形。issue #470。
    pub(crate) work_left: i32,
    pub(crate) work_top: i32,
    pub(crate) work_right: i32,
    pub(crate) work_bottom: i32,
    /// 该显示器的有效 DPI 缩放（1.0 = 96dpi）。
    pub(crate) scale: f64,
}

/// 用 Win32 定位「当前前台窗口（= 用户正在输入的 App）」所在的显示器。
/// 多显示器下用它把胶囊摆到「正在输入的那块屏」。`window.current_monitor()`
/// 返回的是胶囊窗口自己所在的显示器，因此不能用它来跟随输入位置。
#[cfg(target_os = "windows")]
pub(crate) fn foreground_window_monitor() -> Option<ForegroundMonitor> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let hwnd = GetForegroundWindow();
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmon.is_invalid() {
            return None;
        }
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return None;
        }
        let mut dpi_x: u32 = 96;
        let mut dpi_y: u32 = 96;
        // 取不到时退回 96dpi 继续，不让定位整体失败。
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        Some(ForegroundMonitor {
            left: mi.rcMonitor.left,
            top: mi.rcMonitor.top,
            right: mi.rcMonitor.right,
            bottom: mi.rcMonitor.bottom,
            work_left: mi.rcWork.left,
            work_top: mi.rcWork.top,
            work_right: mi.rcWork.right,
            work_bottom: mi.rcWork.bottom,
            scale: (dpi_x as f64 / 96.0).max(0.1),
        })
    }
}

/// 把 capsule 窗口移到屏幕底部居中，与 Swift `CapsuleWindowController.repositionToBottomCenter` 同效。
/// 留 80pt 给 macOS Dock；Windows 任务栏一般在底部 48pt 以内，整体也合适。
pub(crate) fn position_capsule_bottom_center<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    translation_active: bool,
) -> tauri::Result<()> {
    let bounds = capsule_window_bounds(translation_active);

    // Windows：跟随「正在输入的 App」所在显示器摆放，避免多显示器下胶囊
    // 总是固定出现在主屏 / 胶囊自己那块屏。
    #[cfg(target_os = "windows")]
    {
        if let Some(mon) = foreground_window_monitor() {
            let scale = mon.scale;
            let phys_w = (bounds.width * scale).round() as i32;
            let phys_h = (bounds.height * scale).round() as i32;
            window.set_size(PhysicalSize::new(
                phys_w.max(1) as u32,
                phys_h.max(1) as u32,
            ))?;

            let mon_w = mon.right - mon.left;
            let x = mon.left + ((mon_w - phys_w) / 2).max(0);
            // 与既有行为一致：「距底部 visual高度 + 80 + inset」，按 physical px 计算。
            let offset_from_bottom =
                (capsule_visual_height(translation_active) + 80.0 + bounds.bottom_inset) * scale;
            let y = ((mon.bottom as f64) - offset_from_bottom).round() as i32;

            // #470：四边都夹到「工作区」内（去掉任务栏），保证整窗可见。GetMonitorInfoW
            // 取不到 rcWork 时（理论上不会，rcWork 总随 rcMonitor 一同填）退回整屏矩形。
            let (work_l, work_t, work_r, work_b) =
                if mon.work_right > mon.work_left && mon.work_bottom > mon.work_top {
                    (mon.work_left, mon.work_top, mon.work_right, mon.work_bottom)
                } else {
                    (mon.left, mon.top, mon.right, mon.bottom)
                };
            let (clamped_x, clamped_y) =
                clamp_to_monitor(x, y, phys_w, phys_h, work_l, work_t, work_r, work_b);
            log::debug!(
                "[capsule] win position: mon=({},{})..({},{}) work=({},{})..({},{}) scale={:.2} size=({}x{}) -> raw=({},{}) clamped=({},{})",
                mon.left, mon.top, mon.right, mon.bottom,
                work_l, work_t, work_r, work_b,
                scale, phys_w, phys_h, x, y, clamped_x, clamped_y
            );
            window.set_position(PhysicalPosition::new(clamped_x, clamped_y))?;
            return Ok(());
        }
        // 仅当 Win32 取不到前台显示器时，落回下面的 current_monitor 逻辑。
    }

    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    window.set_size(LogicalSize::new(bounds.width, bounds.height))?;

    let scale = monitor.scale_factor();
    let size = monitor.size();
    let pos = monitor.position();
    let frame = logical_monitor_frame(pos.x, pos.y, size.width, size.height, scale);
    let (x, y) = bottom_visual_position(
        frame,
        bounds.width,
        capsule_visual_height(translation_active),
        80.0,
        bounds.bottom_inset,
    );
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CapsuleWindowBounds {
    width: f64,
    height: f64,
    bottom_inset: f64,
}

fn capsule_window_bounds(translation_active: bool) -> CapsuleWindowBounds {
    #[cfg(target_os = "windows")]
    {
        const WINDOWS_CAPSULE_PILL_WIDTH: f64 = 196.0;
        const WINDOWS_CAPSULE_SIDE_INSET: f64 = 12.0;
        CapsuleWindowBounds {
            // Keep the existing Windows hitbox width, but express it as
            // pill width (196) + symmetric 12px side insets for shadow room.
            width: WINDOWS_CAPSULE_PILL_WIDTH + WINDOWS_CAPSULE_SIDE_INSET * 2.0,
            height: if translation_active { 118.0 } else { 84.0 },
            bottom_inset: 12.0,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // macOS / Linux：固定 220×110，与 1.2.11 行为一致 — 录音 / 翻译徽章
        // 共用同一个窗口尺寸，避免按 Shift 后窗口高度变化导致胶囊整体下移。
        let _ = translation_active;
        CapsuleWindowBounds {
            width: 220.0,
            height: 110.0,
            bottom_inset: 0.0,
        }
    }
}

fn capsule_visual_height(_translation_active: bool) -> f64 {
    #[cfg(target_os = "windows")]
    {
        52.0
    }

    #[cfg(not(target_os = "windows"))]
    {
        96.0
    }
}

fn capsule_height_for_qa() -> f64 {
    capsule_visual_height(false)
}

#[cfg(test)]
mod tests {
    use super::{
        bottom_center_position, bottom_visual_position, capsule_height_for_qa,
        capsule_visual_height, capsule_window_bounds, clamp_to_monitor, logical_monitor_frame,
        parse_tray_polish_mode_id, rotate_log_if_too_large, tray_polish_mode_menu_entries,
        tray_style_menu_enabled, LogicalMonitorFrame, LOG_ROTATE_LIMIT_BYTES,
    };
    use crate::types::PolishMode;
    use std::io::Write;

    #[test]
    fn tray_style_menu_is_windows_only() {
        #[cfg(target_os = "windows")]
        assert!(tray_style_menu_enabled());

        #[cfg(not(target_os = "windows"))]
        assert!(!tray_style_menu_enabled());
    }

    #[test]
    fn tray_style_menu_lists_builtin_modes_in_expected_order() {
        let entries = tray_polish_mode_menu_entries(PolishMode::Structured);

        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.id.as_str(), entry.label, entry.mode, entry.checked))
                .collect::<Vec<_>>(),
            vec![
                ("style-raw", "原文", PolishMode::Raw, false),
                ("style-light", "轻度润色", PolishMode::Light, false),
                ("style-structured", "清晰结构", PolishMode::Structured, true),
                ("style-formal", "正式表达", PolishMode::Formal, false),
            ]
        );
    }

    #[test]
    fn tray_style_menu_id_parsing_accepts_only_style_items() {
        assert_eq!(
            parse_tray_polish_mode_id("style-raw"),
            Some(PolishMode::Raw)
        );
        assert_eq!(
            parse_tray_polish_mode_id("style-light"),
            Some(PolishMode::Light)
        );
        assert_eq!(
            parse_tray_polish_mode_id("style-structured"),
            Some(PolishMode::Structured)
        );
        assert_eq!(
            parse_tray_polish_mode_id("style-formal"),
            Some(PolishMode::Formal)
        );
        assert_eq!(parse_tray_polish_mode_id("toggle"), None);
        assert_eq!(parse_tray_polish_mode_id("mic-default"), None);
    }

    #[test]
    fn capsule_window_bounds_leave_room_for_windows_shadow() {
        let bounds = capsule_window_bounds(false);
        #[cfg(target_os = "windows")]
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (220.0, 84.0, 12.0)
        );

        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (220.0, 110.0, 0.0)
        );
    }

    #[test]
    fn capsule_window_bounds_expand_for_translation_badge() {
        let bounds = capsule_window_bounds(true);
        #[cfg(target_os = "windows")]
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (220.0, 118.0, 12.0)
        );

        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            (bounds.width, bounds.height, bounds.bottom_inset),
            (220.0, 110.0, 0.0)
        );
    }

    #[test]
    fn capsule_visual_height_matches_frontend_pill() {
        #[cfg(target_os = "windows")]
        assert_eq!(capsule_visual_height(true), 52.0);

        #[cfg(not(target_os = "windows"))]
        assert_eq!(capsule_visual_height(true), 96.0);
    }

    #[test]
    fn qa_anchor_uses_normal_capsule_height_source() {
        #[cfg(target_os = "windows")]
        assert_eq!(capsule_height_for_qa(), 52.0);

        #[cfg(not(target_os = "windows"))]
        assert_eq!(capsule_height_for_qa(), 96.0);
    }

    #[test]
    fn logical_monitor_frame_preserves_negative_origin() {
        let frame = logical_monitor_frame(-2560, 720, 5120, 2880, 2.0);

        assert_eq!(
            frame,
            LogicalMonitorFrame {
                x: -1280.0,
                y: 360.0,
                width: 2560.0,
                height: 1440.0,
            }
        );
    }

    #[test]
    fn bottom_center_position_keeps_window_on_left_monitor() {
        let frame = LogicalMonitorFrame {
            x: -1440.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
        };

        let pos = bottom_center_position(frame, 380.0, 440.0, 184.0);

        assert_eq!(pos, (-910.0, 276.0));
    }

    #[test]
    fn bottom_visual_position_keeps_capsule_on_upper_monitor() {
        let frame = LogicalMonitorFrame {
            x: 0.0,
            y: -900.0,
            width: 1440.0,
            height: 900.0,
        };

        let pos = bottom_visual_position(frame, 220.0, 96.0, 80.0, 0.0);

        assert_eq!(pos, (610.0, -176.0));
    }

    // ---- #470: capsule 四边 clamp（纯函数，合成多显示器 / 负原点 / 1.5x DPI 输入）----

    #[test]
    fn clamp_to_monitor_leaves_on_screen_position_untouched() {
        // 1080p 主屏正中偏下，整窗本就可见 → 原样返回。
        let (x, y) = clamp_to_monitor(800, 900, 264, 126, 0, 0, 1920, 1040);
        assert_eq!((x, y), (800, 900));
    }

    #[test]
    fn clamp_to_monitor_pulls_back_off_screen_right_and_bottom() {
        // x/y 算到了屏幕右下外侧 → 收回到「右下角减去窗口尺寸」，整窗仍可见。
        let (x, y) = clamp_to_monitor(2000, 1200, 264, 126, 0, 0, 1920, 1040);
        assert_eq!((x, y), (1920 - 264, 1040 - 126));
        // 整窗右/下边界都落在 area 内。
        assert!(x + 264 <= 1920);
        assert!(y + 126 <= 1040);
    }

    #[test]
    fn clamp_to_monitor_pulls_back_when_right_edge_overflows_inside_area() {
        // 左上角 x=1800 本在 area 内，但 x+w=2064 越过右边界 1920 →
        // 应被左移到「右边界 - 窗口宽」，整窗右缘恰好贴住 area_right。
        let (x, _y) = clamp_to_monitor(1800, 900, 264, 126, 0, 0, 1920, 1040);
        assert_eq!(x, 1920 - 264);
        assert!(x + 264 <= 1920);
    }

    #[test]
    fn clamp_to_monitor_pushes_into_negative_origin_left_monitor() {
        // 副屏在主屏左侧（负 X 原点），落点算到了副屏左外侧 → 夹回 area_left。
        // 1.5x DPI 下尺寸偏大，但 area 仍宽于窗口，左上角夹到 (-2560, top)。
        let (x, y) = clamp_to_monitor(-3000, -100, 294, 138, -2560, 0, 0, 1440);
        assert_eq!(x, -2560);
        assert_eq!(y, 0);
        // 右/下仍在 area 内。
        assert!(x >= -2560 && x + 294 <= 0);
        assert!(y >= 0 && y + 138 <= 1440);
    }

    #[test]
    fn clamp_to_monitor_respects_work_area_above_taskbar() {
        // 工作区底部 = 1040（任务栏占了 1040..1080）。落点本在任务栏区域（y=1030），
        // 应被夹到「工作区底 - 窗口高」之上，胶囊整窗不压任务栏。
        let (_x, y) = clamp_to_monitor(800, 1030, 264, 126, 0, 0, 1920, 1040);
        assert_eq!(y, 1040 - 126);
        assert!(y + 126 <= 1040);
    }

    #[test]
    fn clamp_to_monitor_degrades_gracefully_when_window_wider_than_area() {
        // 病态输入：area 比窗口还窄（罕见，但要保证不 panic、不溢出为负超界）。
        // max_x 钳到 area_left，clamp 把左上角收回 area_left。
        let (x, y) = clamp_to_monitor(500, 500, 800, 600, 0, 0, 400, 300);
        assert_eq!((x, y), (0, 0));
    }

    #[test]
    fn oversized_log_rotates_to_single_archive() {
        let dir = std::env::temp_dir().join(format!("openless-log-rotate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("openless.log");
        let archive = dir.join("openless.log.1");

        {
            let mut file = std::fs::File::create(&log).unwrap();
            file.set_len(LOG_ROTATE_LIMIT_BYTES + 1).unwrap();
            file.write_all(b"x").unwrap();
        }
        std::fs::write(&archive, b"old").unwrap();

        rotate_log_if_too_large(&log).unwrap();

        assert!(!log.exists());
        assert!(archive.exists());
        assert!(std::fs::metadata(&archive).unwrap().len() > LOG_ROTATE_LIMIT_BYTES);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn small_log_does_not_rotate() {
        let dir = std::env::temp_dir().join(format!("openless-log-small-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("openless.log");
        let archive = dir.join("openless.log.1");
        std::fs::write(&log, b"small").unwrap();

        rotate_log_if_too_large(&log).unwrap();

        assert!(log.exists());
        assert!(!archive.exists());
        assert_eq!(std::fs::read(&log).unwrap(), b"small");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_log_does_not_rotate() {
        let dir = std::env::temp_dir().join(format!("openless-log-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("openless.log");
        let archive = dir.join("openless.log.1");

        rotate_log_if_too_large(&log).unwrap();

        assert!(!log.exists());
        assert!(!archive.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
