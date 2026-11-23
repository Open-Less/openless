//! Native capsule display policy: follow the pointer by default, select the primary or
//! foreground-window display when requested, and mirror only when the user explicitly chooses
//! all attached displays.
//!
//! The primary `capsule` / `capsule-rail` windows stay the source of truth: they own the
//! card modes, the layout cache and the event routing. Legacy mirror labels are kept
//! for opt-in mirrors; cursor-only mode creates no additional windows.
//!
//! Pure placement / topology helpers in this module are platform-agnostic and unit-tested
//! on every CI host. Native mirror creation supports macOS and Windows.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::types::CapsuleDisplayMode;
#[cfg(not(mobile))]
use tauri::Monitor;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

// 显示器几何、放置、镜像选择已迁到平台层；宿主只负责 tauri Monitor → MonitorRect 的转换。
pub(crate) use openless_platform::capsule::geometry::{
    capsule_placement, mirror_targets, monitor_at_point, MonitorRect, MAX_CAPSULE_MIRRORS,
};

/// tauri 显示器信息 → 平台层 `MonitorRect`。
#[cfg(not(mobile))]
pub(crate) fn monitor_rect_from_tauri(monitor: &tauri::Monitor) -> MonitorRect {
    let position = monitor.position();
    let size = monitor.size();
    let work = monitor.work_area();
    MonitorRect::from_parts(
        position.x,
        position.y,
        size.width as i32,
        size.height as i32,
        work.position.x,
        work.position.y,
        work.size.width as i32,
        work.size.height as i32,
        monitor.scale_factor().max(0.1),
        openless_platform::capsule::geometry::monitor_identity(monitor.name().map(String::as_str)),
    )
}

pub(crate) const CAPSULE_MIRROR_PREFIX: &str = "capsule-mirror-";
pub(crate) const RAIL_MIRROR_PREFIX: &str = "capsule-rail-mirror-";

/// Follow the mouse, not the capsule HWND (which can still be on the old screen).
/// Fall back to the primary display if the cursor cannot be queried.
#[cfg(target_os = "windows")]
pub(crate) fn target_monitor_rect<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
) -> Option<MonitorRect> {
    target_monitor_rect_for_mode(window, CapsuleDisplayMode::CursorScreen, None)
}

#[cfg(target_os = "windows")]
pub(crate) fn target_monitor_rect_for_mode<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
    mode: CapsuleDisplayMode,
    frozen: Option<MonitorRect>,
) -> Option<MonitorRect> {
    let monitors = all_monitor_rects(window);
    let primary = primary_monitor_rect(window);
    match mode {
        CapsuleDisplayMode::PrimaryScreen => primary.or_else(|| monitors.first().copied()),
        CapsuleDisplayMode::ForegroundWindowScreen => {
            resolve_frozen_monitor(frozen, &monitors, primary)
        }
        CapsuleDisplayMode::CursorScreen | CapsuleDisplayMode::AllScreens => {
            let mut point = windows::Win32::Foundation::POINT::default();
            // SAFETY: GetCursorPos writes only to this initialized POINT.
            if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.is_ok()
            {
                if let Some(monitor) = monitor_at_point(&monitors, point.x, point.y) {
                    return Some(monitor);
                }
            }
            primary.or_else(|| monitors.first().copied())
        }
    }
}

#[cfg(target_os = "windows")]
fn primary_monitor_rect<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Option<MonitorRect> {
    window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor_rect_from_tauri(&monitor))
}

/// Resolve a monitor captured at session start against the current topology.
/// Returning the current rect preserves updated work-area/DPI data while the stable monitor name
/// (when available) or outer bounds identify the same display. A disconnected target falls back to
/// the current primary display.
pub(crate) fn resolve_frozen_monitor(
    frozen: Option<MonitorRect>,
    monitors: &[MonitorRect],
    primary: Option<MonitorRect>,
) -> Option<MonitorRect> {
    if let Some(frozen) = frozen {
        if let Some(current) = monitors.iter().copied().find(|monitor| {
            if frozen.identity != 0 && monitor.identity != 0 {
                return monitor.identity == frozen.identity;
            }
            (monitor.left, monitor.top, monitor.right, monitor.bottom)
                == (frozen.left, frozen.top, frozen.right, frozen.bottom)
        }) {
            return Some(current);
        }
    }
    primary.or_else(|| monitors.first().copied())
}

/// Map a captured external foreground HWND to the monitor containing that window.
#[cfg(target_os = "windows")]
pub(crate) fn monitor_rect_for_hwnd<R: tauri::Runtime>(
    app: &AppHandle<R>,
    raw_hwnd: usize,
) -> Option<MonitorRect> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let hwnd = HWND(raw_hwnd as *mut c_void);
    if hwnd.0.is_null() {
        return None;
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with the required structure size and is written by Win32.
    if !unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() } {
        return None;
    }
    let rect = info.rcMonitor;
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(monitor_rect_from_tauri)
        .find(|candidate| {
            (
                candidate.left,
                candidate.top,
                candidate.right,
                candidate.bottom,
            ) == (rect.left, rect.top, rect.right, rect.bottom)
        })
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
            identity: 0,
        });
    }
    window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| monitor_rect_from_tauri(&m))
}

/// Every monitor currently connected, in the OS enumeration order.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn all_monitor_rects<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Vec<MonitorRect> {
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(monitor_rect_from_tauri)
        .collect()
}

/// Shared target selection for disposable overlays without a hidden WebView.
pub(crate) fn display_targets(
    mode: CapsuleDisplayMode,
    monitors: &[MonitorRect],
    primary: Option<MonitorRect>,
    cursor: Option<(i32, i32)>,
    frozen: Option<MonitorRect>,
) -> Vec<MonitorRect> {
    let target = match mode {
        CapsuleDisplayMode::PrimaryScreen => primary.or_else(|| monitors.first().copied()),
        CapsuleDisplayMode::ForegroundWindowScreen => {
            resolve_frozen_monitor(frozen, monitors, primary)
        }
        CapsuleDisplayMode::CursorScreen | CapsuleDisplayMode::AllScreens => cursor
            .and_then(|(x, y)| monitor_at_point(monitors, x, y))
            .or(primary)
            .or_else(|| monitors.first().copied()),
    };
    let Some(target) = target else {
        return Vec::new();
    };
    let mut targets = vec![target];
    targets.extend(mirror_targets(mode, monitors, target));
    targets
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

    fn monitor(
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        taskbar: i32,
        scale: f64,
    ) -> MonitorRect {
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
            identity: 0,
        }
    }

    fn monitor_with_identity(
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        taskbar: i32,
        scale: f64,
        identity: u64,
    ) -> MonitorRect {
        let mut monitor = monitor(left, top, width, height, taskbar, scale);
        monitor.identity = identity;
        monitor
    }

    #[test]
    fn disposable_capsule_obeys_each_display_mode() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        let left = monitor(-1920, 0, 1920, 1080, 40, 1.0);
        let right = monitor(1920, 0, 2560, 1440, 80, 1.5);
        let monitors = [primary, left, right];
        let select = |mode| {
            display_targets(
                mode,
                &monitors,
                Some(primary),
                Some((-100, 20)),
                Some(right),
            )
        };
        assert_eq!(select(CapsuleDisplayMode::CursorScreen), vec![left]);
        assert_eq!(select(CapsuleDisplayMode::PrimaryScreen), vec![primary]);
        assert_eq!(
            select(CapsuleDisplayMode::ForegroundWindowScreen),
            vec![right]
        );
        assert_eq!(
            select(CapsuleDisplayMode::AllScreens),
            vec![left, primary, right]
        );
        assert_eq!(
            display_targets(
                CapsuleDisplayMode::ForegroundWindowScreen,
                &[primary],
                Some(primary),
                Some((-100, 20)),
                Some(right)
            ),
            vec![primary]
        );
        assert_eq!(
            display_targets(
                CapsuleDisplayMode::CursorScreen,
                &monitors,
                Some(primary),
                None,
                None
            ),
            vec![primary]
        );
        assert!(display_targets(CapsuleDisplayMode::AllScreens, &[], None, None, None).is_empty());
    }

    #[test]
    fn frozen_monitor_keeps_current_work_area_and_falls_back_when_disconnected() {
        let primary = monitor(0, 0, 1920, 1080, 40, 1.0);
        let captured = monitor(-1920, 0, 1920, 1080, 40, 1.0);
        let refreshed = monitor(-1920, 0, 1920, 1080, 80, 1.25);

        assert_eq!(
            resolve_frozen_monitor(Some(captured), &[refreshed, primary], Some(primary)),
            Some(refreshed)
        );
        assert_eq!(
            resolve_frozen_monitor(Some(captured), &[primary], Some(primary)),
            Some(primary)
        );
        assert_eq!(
            resolve_frozen_monitor(None, &[primary], Some(primary)),
            Some(primary)
        );
    }

    #[test]
    fn frozen_monitor_identity_survives_resolution_and_layout_changes() {
        let primary = monitor_with_identity(0, 0, 1920, 1080, 40, 1.0, 11);
        let captured = monitor_with_identity(-1920, 0, 1920, 1080, 40, 1.0, 22);
        let refreshed = monitor_with_identity(-2560, -120, 2560, 1440, 80, 1.5, 22);

        assert_eq!(
            resolve_frozen_monitor(Some(captured), &[primary, refreshed], Some(primary)),
            Some(refreshed)
        );
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
