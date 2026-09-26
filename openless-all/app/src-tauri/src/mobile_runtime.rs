//! Minimal Tauri mobile runtime — single main window, no tray/hotkey/updater.

use std::sync::Arc;

use tauri::{AppHandle, Manager, RunEvent};

use crate::coordinator::Coordinator;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_fs::init());

    // Coordinator is created inside setup (after Android storage roots are ready).
    // Managing state in setup is supported by Tauri 2 and avoids constructing
    // PreferencesStore against /data/local/tmp before JNI Context exists.
    builder
        .setup(|app| {
            #[cfg(target_os = "android")]
            {
                if let Err(error) = crate::persistence::init_android_storage_roots() {
                    eprintln!("[android-storage] ERROR init failed: {error:#}");
                }
            }

            crate::init_file_logger();
            log::info!("=== OpenLess mobile 启动 ===");
            initialize_android_ndk_context_for_audio();

            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
            }
            if let Some(qa) = app.get_webview_window("qa") {
                let _ = qa.hide();
            }

            let coordinator = Arc::new(Coordinator::new());
            app.manage(coordinator.clone());
            let core_backend = coordinator.backend();
            app.manage(Arc::clone(&core_backend));
            coordinator.tauri_host().bind(app.handle().clone());
            let startup = tauri::async_runtime::block_on(core_backend.start())?;
            if !startup.backend.running {
                return Err("OpenLess Core did not reach the running state".into());
            }
            crate::tauri_events::start(app.handle().clone(), Arc::clone(&core_backend));
            #[cfg(target_os = "android")]
            {
                crate::android::register_android_backend(core_backend);
                crate::android::register_android_coordinator(coordinator.clone());
                crate::android::register_android_app_handle(app.handle().clone());
                coordinator.apply_android_overlay_on_startup();
            }
            Ok(())
        })
        .invoke_handler(crate::app_invoke_handler_mobile!())
        .build(tauri::generate_context!())
        .expect("error while building tauri mobile application")
        .run(|app, event| match event {
            // Tao's Android backend treats "window destroyed" as "the whole
            // app should exit" and calls std::process::exit() once this
            // event isn't prevented (tauri-runtime-wry's on Destroyed
            // handler, then tao::platform_impl::android exiting on
            // ControlFlow::Exit). That exit() runs process-wide C++ static
            // destructors (libhwui/libminikin included) while the
            // Kotlin-side OpenLessImeService is still live in this same
            // process, which produced repeated "destroyed mutex" native
            // aborts unrelated to any Activity visibility. The backend must
            // outlive this window, so always prevent the exit here.
            RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
            }
            RunEvent::Exit => {
                // With ExitRequested now always prevented above, reaching
                // this point at all means something forced the exit despite
                // that (or a future code path calls AppHandle::exit()/
                // restart() directly) — worth recording as its own distinct
                // restart-cause bucket, separate from an OS-level process
                // kill, since it means Tauri itself decided to tear down.
                #[cfg(target_os = "android")]
                {
                    let _ = crate::android::jni::android::with_android_env(|env, context| {
                        crate::android::jni::android::start_service_action(
                            env,
                            context,
                            "com.openless.app.OpenLessRuntimeService",
                            "com.openless.app.action.RUNTIME_EXITED",
                        )
                    });
                }
                if let Some(coordinator) = app.try_state::<Arc<Coordinator>>() {
                    coordinator.stop_hotkey_listener();
                    let backend = coordinator.backend();
                    tauri::async_runtime::spawn(async move {
                        let _ = backend.shutdown().await;
                    });
                }
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
