//! Minimal Tauri mobile runtime — single main window, no tray/hotkey/updater.

use std::sync::Arc;

use tauri::{AppHandle, Manager, RunEvent};

use crate::coordinator::Coordinator;
use crate::commands::{self, MicrophoneMonitorState};

pub fn run() {
    let coordinator = Arc::new(Coordinator::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(coordinator.clone())
        .manage(MicrophoneMonitorState::new(None))
        .setup(move |app| {
            crate::init_file_logger();
            log::info!("=== OpenLess mobile 启动 ===");

            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
            }

            coordinator.bind_app(app.handle().clone());
            Ok(())
        })
        .invoke_handler(crate::app_invoke_handler_mobile!())
        .build(tauri::generate_context!())
        .expect("error while building tauri mobile application")
        .run(|app, event| match event {
            RunEvent::Exit => {
                let coordinator = app.state::<Arc<Coordinator>>();
                coordinator.stop_hotkey_listener();
            }
            _ => {}
        });
}

#[allow(dead_code)]
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
