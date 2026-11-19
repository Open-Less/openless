use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "ask-anything";
const PAGE: &str = "ask-window/index.html";
const WIDTH: f64 = 1920.0;
const HEIGHT: f64 = 800.0;
static OPEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Opens the standalone Ask surface without creating a conversation or starting a recording.
#[tauri::command]
pub async fn ask_window_open(app: AppHandle) -> Result<(), String> {
    // Native window creation runs from the async command, not a Windows event handler.
    let _opening = OPEN.lock().await;
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => {
            let monitor = app.primary_monitor().ok().flatten();
            let scale = monitor
                .as_ref()
                .map(|monitor| {
                    let work: LogicalSize<f64> =
                        monitor.work_area().size.to_logical(monitor.scale_factor());
                    (work.width / WIDTH).min(work.height / HEIGHT).min(1.0)
                })
                .unwrap_or(1.0);
            let window = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App(PAGE.into()))
                .title("OpenLess — Ask Anything")
                .inner_size(WIDTH * scale, HEIGHT * scale)
                .decorations(false)
                .resizable(false)
                .center()
                .visible(false)
                .build()
                .map_err(|error| error.to_string())?;
            if let (Some(monitor), Ok(size)) = (monitor, window.outer_size()) {
                let work = monitor.work_area();
                // Center in the same work area used to fit the window, excluding the taskbar or Dock.
                let x = work.position.x + (work.size.width.saturating_sub(size.width) / 2) as i32;
                let y = work.position.y + (work.size.height.saturating_sub(size.height) / 2) as i32;
                let _ = window.set_position(PhysicalPosition::new(x, y));
            }
            window
        }
    };
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}
