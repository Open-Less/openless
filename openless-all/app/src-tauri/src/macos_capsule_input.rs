//! Native click-through follows measured DOM controls, not the transparent WebView bounds.
use parking_lot::Mutex;
use serde::Deserialize;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}
impl InputRect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left
            && x <= self.left + self.width
            && y >= self.top
            && y <= self.top + self.height
    }
    fn valid(&self) -> bool {
        [self.left, self.top, self.width, self.height]
            .iter()
            .all(|n| n.is_finite())
            && self.width > 0.0
            && self.height > 0.0
            && self.width <= 4096.0
            && self.height <= 4096.0
    }
}

static REGIONS: OnceLock<Arc<Mutex<Vec<InputRect>>>> = OnceLock::new();
#[tauri::command]
pub(crate) fn set_capsule_interactive_regions(
    window: tauri::WebviewWindow,
    rects: Vec<InputRect>,
) -> Result<(), String> {
    if window.label() != "capsule" {
        return Err("capsule window required".into());
    }
    if rects.len() > 128 || rects.iter().any(|rect| !rect.valid()) {
        return Err("invalid input regions".into());
    }
    let mut start = false;
    let regions = REGIONS.get_or_init(|| {
        start = true;
        Arc::new(Mutex::new(Vec::new()))
    });
    *regions.lock() = rects;
    if start {
        let regions = Arc::clone(regions);
        tauri::async_runtime::spawn(async move {
            loop {
                let visible = window.is_visible().unwrap_or(false);
                if visible {
                    if let (Ok(cursor), Ok(origin), Ok(scale)) = (
                        window.cursor_position(),
                        window.inner_position(),
                        window.scale_factor(),
                    ) {
                        let x = (cursor.x - origin.x as f64) / scale;
                        let y = (cursor.y - origin.y as f64) / scale;
                        let pass = !regions.lock().iter().any(|rect| rect.contains(x, y));
                        // Host phase transitions can also reset ignoresMouseEvents.
                        let _ = window.set_ignore_cursor_events(pass);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(if visible {
                    12
                } else {
                    250
                }))
                .await;
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_and_transparent_space_are_not_input_regions() {
        let buttons = [
            InputRect {
                left: 10.,
                top: 100.,
                width: 32.,
                height: 32.,
            },
            InputRect {
                left: 400.,
                top: 100.,
                width: 32.,
                height: 32.,
            },
        ];
        assert!(buttons.iter().any(|r| r.contains(20., 110.)));
        assert!(!buttons.iter().any(|r| r.contains(220., 110.)));
        assert!(!buttons.iter().any(|r| r.contains(20., 20.)));
        assert!(!InputRect {
            left: f64::NAN,
            top: 0.,
            width: 20.,
            height: 20.
        }
        .valid());
    }
}
