//! Native click-through follows measured DOM controls, not the transparent WebView bounds.

use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tauri::Manager;

/// 交互区域：直接复用平台层类型（字段公开，可从 DOM 量测结果构造）。
pub(crate) use openless_platform::macos::capsule_window::InputRect;
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

type Regions = Arc<Mutex<HashMap<String, Vec<InputRect>>>>;
static REGIONS: OnceLock<Regions> = OnceLock::new();
fn owns_capsule_input(label: &str) -> bool {
    label == "capsule" || label.starts_with("capsule-mirror-")
}
#[tauri::command]
pub(crate) fn set_capsule_interactive_regions(
    window: tauri::WebviewWindow,
    rects: Vec<InputRect>,
) -> Result<(), String> {
    if !owns_capsule_input(window.label()) {
        return Err("capsule window required".into());
    }
    if rects.len() > 128 || rects.iter().any(|rect| !rect.valid()) {
        return Err("invalid input regions".into());
    }
    let mut start = false;
    let regions = REGIONS.get_or_init(|| {
        start = true;
        Arc::new(Mutex::new(HashMap::new()))
    });
    // Remove departed mirrors whenever a live document publishes a fresh frame.
    let live = window.app_handle().webview_windows();
    {
        let mut current = regions.lock();
        current.retain(|label, _| live.contains_key(label));
        current.insert(window.label().to_string(), rects);
    }
    update_native_input(&window, Arc::clone(regions));
    if start {
        let regions = Arc::clone(regions);
        let app = window.app_handle().clone();
        tauri::async_runtime::spawn(async move {
            loop {
                // Each mirror owns its own DOM geometry and coordinate space.
                let labels: Vec<_> = regions.lock().keys().cloned().collect();
                let mut visible = false;
                for label in labels {
                    if let Some(window) = app
                        .get_webview_window(&label)
                        .filter(|w| w.is_visible().unwrap_or(false))
                    {
                        visible = true;
                        update_native_input(&window, Arc::clone(&regions));
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

fn update_native_input(window: &tauri::WebviewWindow, regions: Regions) {
    let owner = window.clone();
    let _ = window.run_on_main_thread(move || unsafe {
        // Keep a press routed to its original window through mouse-up. Opening
        // a language menu resizes the host before the DOM publishes new rectangles.
        let pressed = openless_platform::macos::capsule_window::pressed_mouse_buttons();
        if pressed != 0 {
            return;
        }
        let Ok(pointer) = owner.ns_window() else {
            return;
        };
        let window = pointer as *mut std::ffi::c_void;
        // 坐标换算与点击穿透判定都在平台层（AppKit 把屏幕点直接换算到窗口逻辑坐标，
        // 避开 tauri 全局光标与窗口位置缩放因子不一致的问题）。
        let Some((x, y)) = openless_platform::macos::capsule_window::cursor_in_window(window) else {
            return;
        };
        let receive = regions
            .lock()
            .get(owner.label())
            .is_some_and(|rects| {
                openless_platform::macos::capsule_window::should_receive_mouse(rects, x, y)
            });
        openless_platform::macos::capsule_window::set_ignores_mouse_events(window, !receive);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primary_and_mirror_capsules_own_input_but_other_windows_do_not() {
        assert!(owns_capsule_input("capsule"));
        assert!(owns_capsule_input("capsule-mirror-42"));
        for label in ["main", "qa", "capsule-rail", "less-computer"] {
            assert!(!owns_capsule_input(label));
        }
    }
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
