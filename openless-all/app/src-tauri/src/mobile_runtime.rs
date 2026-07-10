//! Minimal Tauri mobile runtime — single main window, no tray/hotkey/updater.

use std::sync::Arc;

use tauri::{AppHandle, Manager, RunEvent};

use crate::coordinator::Coordinator;

pub fn run() {
    let coordinator = Arc::new(Coordinator::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(coordinator.clone())
        .setup(move |app| {
            crate::init_file_logger();
            log::info!("=== OpenLess mobile 启动 ===");
            initialize_android_ndk_context_for_audio();

            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
            }
            if let Some(qa) = app.get_webview_window("qa") {
                let _ = qa.hide();
            }

            coordinator.bind_app(app.handle().clone());
            #[cfg(target_os = "android")]
            {
                crate::android::register_android_coordinator(coordinator.clone());
                coordinator.apply_android_overlay_on_startup();
            }
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

#[cfg(target_os = "android")]
fn initialize_android_ndk_context_for_audio() {
    static INIT: std::sync::Once = std::sync::Once::new();

    INIT.call_once(|| {
        let Some(context) = tao::platform::android::prelude::main_android_context() else {
            log::warn!("[android] tao Android context unavailable; audio backend may fail");
            return;
        };

        let result = std::panic::catch_unwind(|| unsafe {
            ndk_context::initialize_android_context(context.java_vm, context.context_jobject);
        });

        if result.is_ok() {
            log::info!("[android] initialized ndk-context for audio backend");
        } else {
            log::warn!("[android] ndk-context was already initialized or rejected initialization");
        }
    });
}

#[cfg(not(target_os = "android"))]
fn initialize_android_ndk_context_for_audio() {}
