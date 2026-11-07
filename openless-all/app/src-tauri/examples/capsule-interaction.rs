//! Isolated GUI acceptance for the production capsule and native input ownership.
//! The backend represents only a fixture session and never records or inserts text.
#![allow(dead_code, unexpected_cfgs)]
#[cfg(target_os = "macos")]
#[path = "../src/macos_capsule_input.rs"]
mod macos_capsule_input;
#[cfg(target_os = "macos")]
mod fixture {
    use objc2::{
        class, msg_send,
        runtime::{AnyObject, Bool},
    };
    use objc2_foundation::NSRect;
    use serde_json::{json, Value};
    use std::{
        io::{self, BufRead},
        sync::Mutex,
    };
    use tauri::Emitter;
    static TARGET: Mutex<String> = Mutex::new(String::new());
    fn recording() -> Value {
        json!({"state":"recording","level":0.4,"translation":true,"sessionId":"fixture-recording","capsuleStyle":"classic","warming":false,"transcript":"测试实时原文 123","revision":1,"payloadRevision":1,"sequence":1})
    }
    #[tauri::command]
    fn get_startup_snapshot(debug: Option<Value>) -> Value {
        if let Some(debug) = debug {
            println!("GEOMETRY {debug}");
        } else {
            println!("CALL startup");
        }
        json!({"contractVersion":"2.0.0","backend":{"running":true}})
    }
    #[tauri::command]
    fn get_settings() -> Value {
        println!("CALL settings");
        json!({"themeMode":"light","capsuleStyle":"classic","capsuleTranscriptEnabled":true,"capsuleTranscriptFontSize":14,"translationTargetLanguage":"English"})
    }
    #[tauri::command]
    fn get_capsule_snapshot() -> Value {
        println!("CALL snapshot");
        recording()
    }
    #[tauri::command]
    fn get_capsule_auxiliary_snapshot() -> Value {
        json!({"suggestions":[],"fallback":null})
    }
    #[tauri::command]
    fn get_platform_capabilities() -> Value {
        json!({"platform":"desktop","os":"mac","supportsDesktopHotkey":true})
    }
    #[tauri::command]
    fn set_capsule_transcript_visible(visible: bool) {
        let _ = visible;
    }
    #[tauri::command]
    fn stop_dictation() {
        println!("ACTION confirm");
    }
    #[tauri::command]
    fn cancel_dictation() {
        println!("ACTION cancel");
    }
    #[tauri::command]
    fn get_dictation_translation_target() -> Value {
        let mut target = TARGET.lock().unwrap();
        if target.is_empty() {
            *target = "English".into();
        }
        json!({"sessionId":"fixture-recording","targetLanguage":*target,"workingLanguages":["简体中文"],"mutable":true})
    }
    #[tauri::command]
    fn select_dictation_translation_target(target_language: String, session_id: String) -> Value {
        assert_eq!(session_id, "fixture-recording");
        println!("ACTION language={target_language}");
        *TARGET.lock().unwrap() = target_language;
        get_dictation_translation_target()
    }
    #[tauri::command]
    async fn set_capsule_language_menu(
        window: tauri::WebviewWindow,
        open: bool,
        session_id: String,
    ) -> Result<(), String> {
        assert_eq!(session_id, "fixture-recording");
        let owner = window.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || unsafe {
                let native = owner.ns_window().unwrap() as *mut AnyObject;
                let mut frame: NSRect = msg_send![native, frame];
                // The production host reserves headroom while keeping the pill's bottom fixed.
                frame.size.height = if open { 492. } else { 172. };
                let _: () = msg_send![native,setFrame:frame display:Bool::YES];
                let _ = send.send(());
            })
            .map_err(|e| e.to_string())?;
        receive.await.map_err(|e| e.to_string())?;
        println!("MENU {open}");
        Ok(())
    }
    #[tauri::command]
    fn set_capsule_interactive_regions(
        window: tauri::WebviewWindow,
        rects: Value,
    ) -> Result<(), String> {
        println!("REGIONS {rects}");
        super::macos_capsule_input::set_capsule_interactive_regions(
            window,
            serde_json::from_value(rects).map_err(|e| e.to_string())?,
        )
    }
    #[tauri::command]
    fn fixture_geometry(payload: Value) {
        println!("GEOMETRY {payload}");
    }
    fn geometry(window: &tauri::WebviewWindow) {
        let _=window.eval(r#"window.__TAURI_INTERNALS__.invoke('get_startup_snapshot',{debug:{buttons:[...document.querySelectorAll('button,[role=option],[role=listbox]')].filter(e=>{if(e.getAttribute('role')!=='option')return true;const r=e.getBoundingClientRect(),p=e.closest('[role=listbox]').getBoundingClientRect();return r.top>=p.top&&r.bottom<=p.bottom&&r.width>0;}).map(e=>{let r=e.getBoundingClientRect(),s=getComputedStyle(e);return {text:e.textContent,label:e.getAttribute('aria-label'),selected:e.getAttribute('aria-selected')==='true',role:e.getAttribute('role'),class:e.className,left:r.left,top:r.top,width:r.width,height:r.height,visibility:s.visibility,clip:s.clipPath.startsWith('shape('),radius:s.borderRadius}}),width:innerWidth,height:innerHeight}})"#);
    }
    pub fn main() {
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.openless.capsule-interaction-fixture".into();
        context.config_mut().app.windows.clear();
        context.config_mut().app.with_global_tauri = true;
        context.config_mut().app.security.csp = None;
        context.config_mut().build.dev_url = None;
        tauri::Builder::default()
            .invoke_handler(tauri::generate_handler![
                get_startup_snapshot,
                get_settings,
                get_capsule_snapshot,
                get_capsule_auxiliary_snapshot,
                get_platform_capabilities,
                set_capsule_transcript_visible,
                stop_dictation,
                cancel_dictation,
                get_dictation_translation_target,
                select_dictation_translation_target,
                set_capsule_language_menu,
                set_capsule_interactive_regions,
                fixture_geometry
            ])
            .setup(|app| {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                let screen_index = std::env::args()
                    .nth(1)
                    .and_then(|value| value.parse::<isize>().ok())
                    .unwrap_or(0);
                let label = if std::env::args().any(|arg| arg == "--mirror") {
                    "capsule-mirror-fixture"
                } else {
                    "capsule"
                };
                let window = tauri::WebviewWindowBuilder::new(
                    app,
                    label,
                    tauri::WebviewUrl::App("index.html?window=capsule&os=mac".into()),
                )
                .title("OpenLess real capsule interaction fixture")
                .inner_size(460., 172.)
                .position(350., 150.)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .visible_on_all_workspaces(true)
                .shadow(false)
                .focused(false)
                .focusable(false)
                .accept_first_mouse(true)
                .build()?;
                unsafe {
                    let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
                    let count = core_foundation::array::CFArrayGetCount(screens as _);
                    if screen_index < 0 || screen_index >= count {
                        println!(
                            "SKIP screen {screen_index} unavailable; {count} attached display(s)"
                        );
                        app.handle().exit(77);
                        return Ok(());
                    }
                    let screen =
                        core_foundation::array::CFArrayGetValueAtIndex(screens as _, screen_index)
                            as *mut AnyObject;
                    let visible: NSRect = msg_send![screen, visibleFrame];
                    let scale: f64 = msg_send![screen, backingScaleFactor];
                    println!("SCREEN {screen_index} SCALE {scale}");
                    let native = window.ns_window().unwrap() as *mut AnyObject;
                    let _: () = msg_send![native,setLevel:30isize];
                    let mut frame: NSRect = msg_send![native, frame];
                    frame.origin.x =
                        visible.origin.x + (visible.size.width - frame.size.width) / 2.;
                    frame.origin.y = visible.origin.y + 30.;
                    let _: () = msg_send![native,setFrame:frame display:Bool::YES];
                }
                let handle = app.handle().clone();
                let input_window = window.clone();
                std::thread::spawn(move || {
                    for line in io::stdin().lock().lines().map_while(Result::ok) {
                        match line.as_str() {
                            "geometry" => geometry(&input_window),
                            "reset" => {
                                let _ = handle.emit_to("capsule", "capsule:state", recording());
                            }
                            "quit" => {
                                handle.exit(0);
                                break;
                            }
                            _ => {}
                        }
                    }
                });
                Ok(())
            })
            .run(context)
            .unwrap();
    }
}
#[cfg(target_os = "macos")]
fn main() {
    fixture::main();
}
#[cfg(not(target_os = "macos"))]
fn main() {}
