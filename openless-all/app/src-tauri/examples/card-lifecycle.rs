//! macOS smoke fixture: real correction-card frontend and production window policy.
//! Run through scripts/test-card-lifecycle-macos.py so the whole bundle is signed.
#![allow(dead_code, unexpected_cfgs)]

#[cfg(target_os = "macos")]
#[path = "../src/desktop_resources.rs"]
mod desktop_resources;

#[cfg(target_os = "macos")]
fn show_main_window<R: tauri::Runtime>(_: &tauri::AppHandle<R>) {
    unreachable!("the fixture never creates the main window");
}

#[cfg(target_os = "macos")]
mod fixture {
    use super::desktop_resources;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::{Manager, RunEvent};

    static CYCLE: AtomicUsize = AtomicUsize::new(0);
    static DRAIN_TICKS: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static WEBVIEWS: std::cell::RefCell<Vec<objc2::rc::Weak<objc2::runtime::AnyObject>>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    #[tauri::command]
    fn get_startup_snapshot() -> serde_json::Value {
        serde_json::json!({"contractVersion": "2.0.0", "backend": {"running": true}})
    }

    #[tauri::command]
    fn get_capsule_auxiliary_snapshot() -> serde_json::Value {
        let cycle = CYCLE.load(Ordering::SeqCst);
        let ttl = if cycle % 3 == 2 { 1200 } else { 60_000 };
        serde_json::json!({
            "suggestions": [{
                "id": "fixture-correction", "pattern": "fixtureword", "replacement": "fixture word",
                "expiresAtMs": chrono::Utc::now().timestamp_millis() + ttl
            }],
            "fallback": null
        })
    }

    fn close_card(app: tauri::AppHandle, action: &str) {
        println!("CARD_ACTION {} {}", CYCLE.load(Ordering::SeqCst), action);
        let window = app.get_webview_window("capsule").expect("card exists");
        desktop_resources::hide_capsule_window(&window).unwrap();
    }

    #[tauri::command]
    fn reject_pending_correction(app: tauri::AppHandle, id: String) {
        assert_eq!(id, "fixture-correction");
        let action = if CYCLE.load(Ordering::SeqCst) % 3 == 2 {
            "timeout"
        } else {
            "reject"
        };
        close_card(app, action);
    }

    #[tauri::command]
    fn accept_pending_correction(app: tauri::AppHandle, id: String) {
        assert_eq!(id, "fixture-correction");
        close_card(app, "accept");
    }

    fn show_card(app: &tauri::AppHandle) {
        desktop_resources::ensure_card_window(app).unwrap();
        let window = app.get_webview_window("capsule").unwrap();
        window
            .with_webview(|webview| unsafe {
                let object = objc2::rc::Retained::from_raw(
                    webview.inner() as *mut objc2::runtime::AnyObject
                )
                .unwrap();
                // Tauri 2.11.2 WithWebview hands out three +1 raw handles via
                // Retained::into_raw. Consume all three instead of retaining
                // them again, or the instrumentation itself keeps cards alive.
                let _controller = objc2::rc::Retained::from_raw(
                    webview.controller() as *mut objc2::runtime::AnyObject
                )
                .unwrap();
                let _window = objc2::rc::Retained::from_raw(
                    webview.ns_window() as *mut objc2::runtime::AnyObject
                )
                .unwrap();
                WEBVIEWS
                    .with_borrow_mut(|views| views.push(objc2::rc::Weak::from_retained(&object)));
            })
            .unwrap();
        window
            .set_size(tauri::LogicalSize::new(320., 108.))
            .unwrap();
        window
            .set_position(tauri::LogicalPosition::new(200., 200.))
            .unwrap();
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2::{msg_send, runtime::AnyObject};
            let raw = window.ns_window().unwrap() as *mut AnyObject;
            let _: () = msg_send![raw, orderFrontRegardless];
        }
        println!("CARD_SHOW {}", CYCLE.load(Ordering::SeqCst));
    }

    pub(super) fn run() {
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.openless.card-lifecycle-test".into();
        desktop_resources::configure(&mut context, true);
        let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_startup_snapshot, get_capsule_auxiliary_snapshot, reject_pending_correction, accept_pending_correction
        ])
        .setup(|app| {
            show_card(app.handle());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    let tick = handle.clone();
                    handle.run_on_main_thread(move || {
                        let cycle = CYCLE.load(Ordering::SeqCst);
                        if cycle == 9 {
                            let alive = WEBVIEWS.with_borrow(|views| views.iter().enumerate().filter_map(|(i, view)| view.load().map(|_| i)).collect::<Vec<_>>());
                            if !alive.is_empty() {
                                let attempt = DRAIN_TICKS.fetch_add(1, Ordering::SeqCst);
                                if attempt % 5 == 0 { println!("CARD_DRAIN {:?}", alive); }
                                assert!(attempt < 25, "closed cards retained their WKWebView after 5 seconds: {:?}", alive);
                                return;
                            }
                            println!("CARD_WEBVIEWS_RELEASED 9");
                                let normal = tauri::WebviewWindowBuilder::new(&tick, "normal-window-probe", tauri::WebviewUrl::App("index.html".into()))
                                    .visible(false).build().unwrap();
                                unsafe {
                                    use objc2::{msg_send, runtime::{AnyObject, Bool}, ClassType};
                                    let raw = normal.ns_window().unwrap() as *mut AnyObject;
                                    let panel: Bool = msg_send![raw, isKindOfClass: objc2_app_kit::NSPanel::class()];
                                    assert!(!panel.as_bool(), "card hook affected a later normal window");
                                }
                                normal.destroy().unwrap();
                                println!("CARD_NORMAL_WINDOW_OK");
                                println!("CARD_PASS 9");
                                tick.exit(0);

                            return;
                        }

                        if let Some(window) = tick.get_webview_window("capsule") {
                            if cycle % 3 != 2 && (cycle != 0 || std::env::var_os("OPENLESS_CARD_MANUAL_FIRST").is_none()) {
                                let index = if cycle % 3 == 0 { 1 } else { 0 };
                                let script = format!(
                                    "if (document.body.textContent.includes('fixtureword')) {{ const b = document.querySelectorAll('button'); if (b.length === 2) b[{index}].click(); }}"
                                );
                                window.eval(&script).unwrap();
                            }
                        } else {
                            let next = CYCLE.fetch_add(1, Ordering::SeqCst) + 1;
                            println!("CARD_DESTROYED {}", cycle);
                            if next < 9 { show_card(&tick); }

                        }
                    }).unwrap();
                }
            });
            Ok(())
        })
        .build(context)
        .expect("fixture builds");
        app.run(|_, event| {
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    fixture::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("This fixture requires macOS.");
}
