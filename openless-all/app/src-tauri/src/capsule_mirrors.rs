//! Native capsule display policy: follow the pointer by default, mirror only
//! when the user explicitly chooses all attached displays.
//!
//! The primary `capsule` / `capsule-rail` windows stay the source of truth: they own the
//! card modes, the layout cache and the event routing. Legacy mirror labels are kept
//! for opt-in mirrors; cursor-only mode creates no additional windows.
//!
//! Pure placement / topology helpers in this module are platform-agnostic and unit-tested
//! on every CI host. Native mirror creation supports macOS and Windows.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::{AppHandle, Manager, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use crate::types::CapsuleDisplayMode;

pub(crate) const CAPSULE_MIRROR_PREFIX: &str = "capsule-mirror-";
pub(crate) const RAIL_MIRROR_PREFIX: &str = "capsule-rail-mirror-";

/// Legacy slot count used to hide any previously created mirror windows.
pub(crate) const MAX_CAPSULE_MIRRORS: usize = 7;

/// Physical description of one monitor in virtual-desktop coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MonitorRect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
    pub(crate) work_left: i32,
    pub(crate) work_top: i32,
    pub(crate) work_right: i32,
    pub(crate) work_bottom: i32,
    /// Effective DPI scale (1.0 = 96 dpi).
    pub(crate) scale: f64,
}

impl MonitorRect {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn from_tauri(monitor: &Monitor) -> Self {
        let position = monitor.position();
        let size = monitor.size();
        let work = monitor.work_area();
        let left = position.x;
        let top = position.y;
        let work_left = work.position.x;
        let work_top = work.position.y;
        Self {
            left,
            top,
            right: left + size.width as i32,
            bottom: top + size.height as i32,
            work_left,
            work_top,
            work_right: work_left + work.size.width as i32,
            work_bottom: work_top + work.size.height as i32,
            scale: monitor.scale_factor().max(0.1),
        }
    }

    /// Work area (taskbar excluded), falling back to the full monitor when the OS reports none.
    fn work_rect(&self) -> (i32, i32, i32, i32) {
        if self.work_right > self.work_left && self.work_bottom > self.work_top {
            (self.work_left, self.work_top, self.work_right, self.work_bottom)
        } else {
            (self.left, self.top, self.right, self.bottom)
        }
    }
}

/// Follow the mouse, not the capsule HWND (which can still be on the old screen).
/// Fall back to the primary display if the cursor cannot be queried.
#[cfg(target_os = "windows")]
pub(crate) fn target_monitor_rect<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
) -> Option<MonitorRect> {
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};
    let monitors = all_monitor_rects(window);
    let mut point = POINT::default();
    // SAFETY: GetCursorPos writes only to this initialized POINT.
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        if let Some(monitor) = monitor_at_point(&monitors, point.x, point.y) {
            return Some(monitor);
        }
    }
    window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| MonitorRect::from_tauri(&monitor))
}

pub(crate) fn monitor_at_point(monitors: &[MonitorRect], x: i32, y: i32) -> Option<MonitorRect> {
    monitors
        .iter()
        .copied()
        .find(|m| x >= m.left && x < m.right && y >= m.top && y < m.bottom)
}

/// Match the macOS pointer-based primary placement without querying the old window position.
#[cfg(target_os = "macos")]
pub(crate) fn target_monitor_rect<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
) -> Option<MonitorRect> {
    if let Some(m) = crate::capsule_target_monitor(window) {
        return Some(MonitorRect {
            left: m.physical_x,
            top: m.physical_y,
            right: m.physical_x + m.physical_width as i32,
            bottom: m.physical_y + m.physical_height as i32,
            work_left: m.work_x,
            work_top: m.work_y,
            work_right: m.work_x + m.work_width as i32,
            work_bottom: m.work_y + m.work_height as i32,
            scale: m.scale,
        });
    }
    window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| MonitorRect::from_tauri(&m))
}

/// Every monitor currently connected, in the OS enumeration order.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn all_monitor_rects<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Vec<MonitorRect> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(MonitorRect::from_tauri)
        .collect()
}

/// Physical placement of a capsule at the bottom center of `monitor`'s work area.
///
/// `logical_width` / `logical_height` / `logical_bottom_gap` are DPI-independent sizes; they
/// are converted with the monitor's own scale, so mixed-DPI setups each get a correctly sized
/// capsule. Returns `(physical_width, physical_height, x, y)`.
pub(crate) fn capsule_placement(
    logical_width: f64,
    logical_height: f64,
    logical_bottom_gap: f64,
    monitor: MonitorRect,
) -> (u32, u32, i32, i32) {
    let scale = monitor.scale;
    let phys_w = (logical_width * scale).round() as i32;
    let phys_h = (logical_height * scale).round() as i32;
    let (work_l, work_t, work_r, work_b) = monitor.work_rect();
    let x = work_l + ((work_r - work_l - phys_w) / 2).max(0);
    let y = work_b - phys_h - (logical_bottom_gap * scale).round() as i32;
    let (x, y) = crate::clamp_to_monitor(x, y, phys_w, phys_h, work_l, work_t, work_r, work_b);
    (phys_w.max(1) as u32, phys_h.max(1) as u32, x, y)
}

/// Mirror only on explicit opt-in, excluding the display already hosting the primary.
pub(crate) fn mirror_targets(
    mode: CapsuleDisplayMode,
    monitors: &[MonitorRect],
    primary: MonitorRect,
) -> Vec<MonitorRect> {
    if mode != CapsuleDisplayMode::AllScreens {
        return Vec::new();
    }
    monitors
        .iter()
        .copied()
        .filter(|m| {
            (m.left, m.top, m.right, m.bottom)
                != (primary.left, primary.top, primary.right, primary.bottom)
        })
        .take(MAX_CAPSULE_MIRRORS)
        .collect()
}

/// Stable hash of the monitor layout. Changes when a monitor is connected, disconnected,
/// moved, re-scaled or re-ordered, so cached placements can be invalidated.
pub(crate) fn topology_signature(monitors: &[MonitorRect]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for monitor in monitors {
        (
            monitor.left,
            monitor.top,
            monitor.right,
            monitor.bottom,
            monitor.work_left,
            monitor.work_top,
            monitor.work_right,
            monitor.work_bottom,
            monitor.scale.to_bits(),
        )
            .hash(&mut hasher);
    }
    hasher.finish()
}

/// Fingerprint of everything the mirror windows depend on. When it is unchanged since the
/// last sync, the native windows are already in the right state and the sync is skipped.
pub(crate) fn mirror_fingerprint(
    visible: bool,
    passthrough: bool,
    style_tag: u8,
    translation_active: bool,
    transcript_visible: bool,
    primary: MonitorRect,
    monitors: &[MonitorRect],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    (
        visible,
        passthrough,
        style_tag,
        translation_active,
        transcript_visible,
    )
        .hash(&mut hasher);
    topology_signature(&[primary]).hash(&mut hasher);
    topology_signature(monitors).hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn mirror_label(prefix: &str, index: usize) -> String {
    format!("{prefix}{index}")
}

/// Labels of existing mirror windows whose label starts with `prefix`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn existing_mirror_labels<R: tauri::Runtime>(
    app: &AppHandle<R>,
    prefix: &str,
) -> Vec<String> {
    app.webview_windows()
        .keys()
        .filter(|label| label.starts_with(prefix))
        .cloned()
        .collect()
}

/// Returns the capsule mirror with `label`, creating it hidden when it does not exist yet.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn ensure_capsule_mirror<R: tauri::Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App("index.html?window=capsule".into()),
    )
    .title("OpenLess Capsule Mirror")
    .inner_size(460.0, 180.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .accept_first_mouse(true)
    .build()?;
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::cocoa::appkit::NSWindowCollectionBehavior;
        use tauri_nspanel::WebviewWindowExt;
        let panel = match window.to_panel() {
            Ok(panel) => panel,
            Err(error) => {
                let _ = window.close();
                return Err(error);
            }
        };
        panel.set_style_mask(1 << 7); // Non-activating, like the primary capsule.
        panel.set_level(25);
        panel.set_collection_behaviour(
            NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces,
        );
    }
    Ok(window)
}

/// Returns the transcript-rail mirror with `label`, creating it hidden when needed.
#[cfg(target_os = "windows")]
pub(crate) fn ensure_rail_mirror<R: tauri::Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App("index.html?window=capsule-rail&os=win".into()),
    )
    .title("OpenLess Transcript Rail Mirror")
    .inner_size(460.0, 52.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .accept_first_mouse(false)
    .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(left: i32, top: i32, width: i32, height: i32, taskbar: i32, scale: f64) -> MonitorRect {
        MonitorRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
            work_left: left,
            work_top: top,
            work_right: left + width,
            work_bottom: top + height - taskbar,
            scale,
        }
    }

    #[test]
    fn multi_monitor_recording_has_no_duplicate_capsules() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        let left = monitor(-1920, 0, 1920, 1080, 40, 1.0);
        let right = monitor(1920, 0, 2560, 1440, 40, 1.5);

        let targets = mirror_targets(
            CapsuleDisplayMode::CursorScreen,
            &[left, primary, right],
            primary,
        );

        assert!(targets.is_empty());
        assert_eq!(
            mirror_targets(
                CapsuleDisplayMode::AllScreens,
                &[left, primary, right],
                primary
            ),
            vec![left, right]
        );
        assert_eq!(
            mirror_targets(
                CapsuleDisplayMode::AllScreens,
                &[left, primary, right],
                right
            ),
            vec![left, primary]
        );
        assert!(mirror_targets(CapsuleDisplayMode::AllScreens, &[primary], primary).is_empty());
        assert_eq!(
            monitor_at_point(&[left, primary, right], -20, 50),
            Some(left)
        );
        assert_eq!(
            monitor_at_point(&[left, primary, right], 2000, 50),
            Some(right)
        );
        assert_eq!(
            monitor_at_point(&[left, primary, right], 10, 50),
            Some(primary)
        );
        assert_eq!(monitor_at_point(&[left, primary, right], 99999, 50), None);
    }

    #[test]
    fn mirror_targets_is_empty_on_single_monitor() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        assert!(mirror_targets(CapsuleDisplayMode::CursorScreen, &[primary], primary).is_empty());
    }

    #[test]
    fn cursor_only_never_mirrors_and_all_screens_is_bounded() {
        let primary = monitor(0, 0, 1920, 1080, 0, 1.0);
        let mut monitors = vec![primary];
        for i in 1..=(MAX_CAPSULE_MIRRORS as i32 + 3) {
            monitors.push(monitor(i * 1920, 0, 1920, 1080, 0, 1.0));
        }
        assert!(mirror_targets(CapsuleDisplayMode::CursorScreen, &monitors, primary).is_empty());
        assert_eq!(
            mirror_targets(CapsuleDisplayMode::AllScreens, &monitors, primary).len(),
            MAX_CAPSULE_MIRRORS
        );
    }

    #[test]
    fn capsule_placement_bottom_centers_on_a_monitor_left_of_primary() {
        // Negative virtual-desktop origin: the monitor sits left of the primary one.
        let left = monitor(-1920, 0, 1920, 1080, 40, 1.0);

        let (w, h, x, y) = capsule_placement(460.0, 180.0, 12.0, left);

        assert_eq!((w, h), (460, 180));
        // work area x = -1920..0, centered: -1920 + (1920 - 460) / 2 = -1190
        assert_eq!(x, -1190);
        // work area bottom = 1040, minus height and the 12px gap
        assert_eq!(y, 1040 - 180 - 12);
    }

    #[test]
    fn capsule_placement_scales_each_monitor_by_its_own_dpi() {
        let hidpi = monitor(1920, 0, 2880, 1620, 60, 1.5);

        let (w, h, x, y) = capsule_placement(460.0, 180.0, 12.0, hidpi);

        assert_eq!((w, h), (690, 270));
        // centered in 2880px work area: 1920 + (2880 - 690) / 2 = 3015
        assert_eq!(x, 3015);
        // work bottom = 1560, minus 270px height and 18px (12 logical * 1.5) gap
        assert_eq!(y, 1560 - 270 - 18);
    }

    #[test]
    fn capsule_placement_keeps_zero_gap_for_typeless_style() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        let (_, _, _, y) = capsule_placement(206.0, 57.0, 0.0, primary);
        assert_eq!(y, 1040 - 57);
    }

    #[test]
    fn topology_signature_changes_when_a_monitor_is_disconnected_and_restored() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        let external = monitor(-1920, 0, 1920, 1080, 40, 1.0);

        let both = topology_signature(&[primary, external]);
        let only_primary = topology_signature(&[primary]);
        let restored = topology_signature(&[primary, external]);

        assert_ne!(both, only_primary);
        assert_eq!(both, restored);
    }

    #[test]
    fn mirror_fingerprint_tracks_visibility_and_topology() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        let external = monitor(1920, 0, 1920, 1080, 40, 1.0);
        let layout = [primary, external];

        let shown = mirror_fingerprint(true, false, 1, false, false, primary, &layout);
        let hidden = mirror_fingerprint(false, false, 1, false, false, primary, &layout);
        let unplugged = mirror_fingerprint(true, false, 1, false, false, primary, &[primary]);

        assert_ne!(shown, hidden);
        assert_ne!(shown, unplugged);
        assert_eq!(
            shown,
            mirror_fingerprint(true, false, 1, false, false, primary, &layout)
        );
    }

    #[test]
    fn mirror_labels_are_prefixed_and_indexed() {
        assert_eq!(mirror_label(CAPSULE_MIRROR_PREFIX, 2), "capsule-mirror-2");
        assert_eq!(mirror_label(RAIL_MIRROR_PREFIX, 1), "capsule-rail-mirror-1");
    }
}
