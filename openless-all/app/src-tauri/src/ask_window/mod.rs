use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "ask-anything";
const PAGE: &str = "ask-window/index.html";
static OPEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Opens the standalone Ask surface without creating a conversation or starting a recording.
#[tauri::command]
pub async fn ask_window_open(app: AppHandle) -> Result<(), String> {
    // Native window creation runs from the async command, not a Windows event handler.
    let _opening = OPEN.lock().await;
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App(PAGE.into()))
            .title("OpenLess — Ask Anything")
            .inner_size(920.0, 800.0)
            .center()
            .visible(false)
            .build()
            .map_err(|error| error.to_string())?,
    };
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}
