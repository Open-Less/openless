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
mod commands;
mod coordinator;
mod hotkey;
mod insertion;
mod permissions;
mod persistence;
mod polish;
mod recorder;
mod types;

#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, LogicalPosition, Manager, RunEvent, Runtime};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_file_logger();
    log::info!("=== OpenLess 启动 ===");

    let coordinator = Arc::new(coordinator::Coordinator::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(coordinator.clone())
        .setup(move |app| {
            // Capsule 启动时定位到屏幕底部居中并隐藏；coordinator 按需显示。
            // 与 Swift `CapsuleWindowController.repositionToBottomCenter` 同语义。
            if let Some(capsule) = app.get_webview_window("capsule") {
                if let Err(e) = position_capsule_bottom_center(&capsule) {
                    log::warn!("[capsule] position failed: {e}");
                }
                let _ = capsule.hide();
            }

            // 主窗口磨砂：macOS 用 NSVisualEffectView，Windows 用 Mica。
            // 没这一层的话 transparent: true 让窗口透明 → 背后只是空，不是磨砂。
            if let Some(main) = app.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{
                        apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState,
                    };
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
                    if let Err(e) = apply_mica(&main, None) {
                        log::warn!("[main] mica failed: {e}");
                    }
                }
            }

            show_main_window(app.handle());

            // 启动时主动弹 Accessibility 授权框（与 Swift `AppDelegate` 行为一致）。
            // 用户首次必看到系统提示；已授权则静默返回。
            #[cfg(target_os = "macos")]
            {
                let status = permissions::request_accessibility();
                log::info!("[startup] Accessibility status = {:?}", status);
            }

            // 菜单栏图标 — 与 Swift `MenuBarController` 同语义：
            // 左键点 → 显示/聚焦主窗口；菜单含「显示主窗口」「退出」。
            let toggle = MenuItemBuilder::with_id("toggle", "显示主窗口").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "退出 OpenLess").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&toggle, &quit]).build()?;

            // 与 Swift `StatusBarIcon.swift` 行为一致：用全彩 AppIcon，**不**走 template 模式
            // （走 template 会被 macOS 染成单色 → 看起来像个黑方块）。
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(false)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // Spin up hotkey listener; coordinator owns the lifecycle.
            let app_handle = app.handle().clone();
            coordinator.bind_app(app_handle);
            coordinator.start_hotkey_listener();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::get_hotkey_status,
            commands::get_hotkey_capability,
            commands::get_credentials,
            commands::set_credential,
            commands::list_history,
            commands::delete_history_entry,
            commands::clear_history,
            commands::list_vocab,
            commands::add_vocab,
            commands::remove_vocab,
            commands::set_vocab_enabled,
            commands::start_dictation,
            commands::stop_dictation,
            commands::cancel_dictation,
            commands::repolish,
            commands::set_default_polish_mode,
            commands::set_style_enabled,
            commands::check_accessibility_permission,
            commands::request_accessibility_permission,
            commands::check_microphone_permission,
            commands::request_microphone_permission,
            commands::open_system_settings,
            commands::trigger_microphone_prompt,
            commands::read_credential,
            commands::set_active_llm_provider,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show_main_window(app),
            RunEvent::WindowEvent { label, event, .. } => {
                if label == "main" {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_main_window(app);
                    }
                }
            }
            RunEvent::Exit => {
                let coordinator = app.state::<Arc<coordinator::Coordinator>>();
                coordinator.stop_hotkey_listener();
            }
            _ => {}
        });
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

fn log_dir_path() -> std::path::PathBuf {
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
    let _ = app.run_on_main_thread(|| {
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
            let main_win: *mut AnyObject = msg_send![ns_app, mainWindow];
            if main_win.is_null() {
                return;
            }
            let _: () = msg_send![main_win, makeKeyWindow];
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

/// 把 capsule 窗口移到屏幕底部居中，与 Swift `CapsuleWindowController.repositionToBottomCenter` 同效。
/// 留 80pt 给 macOS Dock；Windows 任务栏一般在底部 48pt 以内，整体也合适。
fn position_capsule_bottom_center<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(m) => m,
        None => return Ok(()),
    };
    let scale = monitor.scale_factor();
    let size = monitor.size();
    let logical_w = size.width as f64 / scale;
    let logical_h = size.height as f64 / scale;
    let cap_w = 220.0_f64;
    let cap_h = 96.0_f64;
    let x = ((logical_w - cap_w) / 2.0).max(0.0);
    let y = (logical_h - cap_h - 80.0).max(0.0);
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}
