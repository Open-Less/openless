//! Tauri-only host operations used by the compatibility coordinator.
//!
//! The shared backend never sees this module. It owns the late-bound
//! [`tauri::AppHandle`] and keeps window, main-thread and managed-state access
//! out of the coordinator's business paths.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

#[cfg(target_os = "windows")]
#[path = "capsule_hit_test.rs"]
mod capsule_hit_test;
#[cfg(target_os = "windows")]
use capsule_hit_test::{install_capsule_hit_test_proc, CapsuleHitTestRect};

#[path = "capsule_snapshot.rs"]
mod capsule_snapshot;
use capsule_snapshot::CapsuleSnapshotState;
use openless_core::BackendEvent;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::types::{CapsuleDisplayMode, CapsulePayload, CapsuleState, CapsuleStyle};

/// The Tauri event-loop thread, recorded by [`TauriCoordinatorHost::bind`].
#[cfg(target_os = "windows")]
static MAIN_THREAD: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

static CAPSULE_SUPPRESSED_BY_TOGGLE_LOGGED: AtomicBool = AtomicBool::new(false);
static CAPSULE_FIRST_SHOW_LOGGED: AtomicBool = AtomicBool::new(false);
static CAPSULE_NO_ACTIVATE_FALLBACK_WARNED: AtomicBool = AtomicBool::new(false);
static CAPSULE_WINDOW_MISSING_LOGGED: AtomicBool = AtomicBool::new(false);

const HIT_TEST_MODE_CAPSULE: u8 = 0;
const HIT_TEST_MODE_CARD: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
struct CapsuleTranscriptRailPosition {
    width: f64,
    height: f64,
    /// Rail top relative to the capsule body HWND top, in logical/native pixels.
    top_offset: f64,
    gap: f64,
}

/// Keep the standalone rail attached to the actual capsule body content. The
/// capsule HWND contains transparent headroom for each style, so subtracting
/// rail height from the HWND top puts Classic and Typeless rails too high.
fn capsule_transcript_rail_position(
    style: CapsuleStyle,
    translation_active: bool,
    transcript_visible: bool,
) -> Option<CapsuleTranscriptRailPosition> {
    if !transcript_visible && (!translation_active || style == CapsuleStyle::Siri) {
        return None;
    }
    match style {
        CapsuleStyle::Siri => {
            let body_top = 0.0;
            let height = 40.0;
            let gap = 8.0;
            Some(CapsuleTranscriptRailPosition {
                width: 460.0,
                height,
                top_offset: body_top - gap - height,
                gap,
            })
        }
        CapsuleStyle::Classic => {
            const HOST_HEIGHT: f64 = 172.0;
            const BODY_BOTTOM_INSET: f64 = 16.0;
            const BODY_HEIGHT: f64 = 32.0;
            const RAIL_HEIGHT: f64 = 52.0;
            const RAIL_GAP: f64 = 8.0;
            const BADGE_HEIGHT: f64 = 28.0;
            const BADGE_GAP: f64 = 8.0;
            let body_top = HOST_HEIGHT - BODY_BOTTOM_INSET - BODY_HEIGHT;
            let gap = RAIL_GAP;
            let height = if transcript_visible { RAIL_HEIGHT } else { 0.0 }
                + if translation_active {
                    BADGE_HEIGHT + if transcript_visible { BADGE_GAP } else { 0.0 }
                } else {
                    0.0
                };
            let top_offset = body_top - RAIL_GAP - height;
            Some(CapsuleTranscriptRailPosition {
                width: 460.0,
                height,
                top_offset,
                gap,
            })
        }
        CapsuleStyle::Typeless => {
            const ZOOM: f64 = 0.447;
            let host_height = if translation_active { 65.0 } else { 57.0 };
            let body_bottom = host_height;
            let body_top =
                body_bottom - (64.0 * ZOOM) - if translation_active { 20.0 * ZOOM } else { 0.0 };
            let rail_height = if transcript_visible { 52.0 * ZOOM } else { 0.0 };
            let height = rail_height + if translation_active { 20.0 * ZOOM } else { 0.0 };
            let gap = 0.0;
            Some(CapsuleTranscriptRailPosition {
                width: 206.0,
                height,
                top_offset: body_top - gap - rail_height,
                gap,
            })
        }
    }
}

/// Places and shows (or hides) a transcript rail for the capsule whose outer origin is
/// `capsule_origin`. Used for the primary rail and for every monitor's rail mirror, so the
/// rail always sits on the same capsule it belongs to.
#[cfg(target_os = "windows")]
fn position_rail_for_capsule<R: tauri::Runtime>(
    app: &AppHandle<R>,
    rail: &tauri::WebviewWindow<R>,
    capsule_origin: tauri::PhysicalPosition<i32>,
    scale: f64,
    style: CapsuleStyle,
    translation_active: bool,
    visible: bool,
) -> tauri::Result<()> {
    let rail_visible_before = rail.is_visible().unwrap_or(false);
    let Some(position) = capsule_transcript_rail_position(style, translation_active, visible)
    else {
        rail.hide()?;
        return Ok(());
    };
    rail.set_ignore_cursor_events(true)?;
    rail.set_size(tauri::LogicalSize::new(position.width, position.height))?;
    rail.set_position(tauri::PhysicalPosition::new(
        capsule_origin.x,
        capsule_origin.y + (position.top_offset * scale).round() as i32,
    ))?;
    // Positioning happens on every streaming frame, but presentation must happen only on a
    // hidden-to-visible transition. Re-showing the dedicated rail for each transcript character
    // restarts its WebView entrance animation even when the main capsule stays visible.
    if should_present_capsule_window(rail_visible_before) {
        show_capsule_window_for_recording(app, rail, false);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn capsule_control_hit_rect(
    style: CapsuleStyle,
    transcript_visible: bool,
    translation_active: bool,
    window_width: i32,
    window_height: i32,
) -> Option<CapsuleHitTestRect> {
    if style == CapsuleStyle::Siri || window_width <= 0 || window_height <= 0 {
        return None;
    }
    let (
        logical_host_width,
        logical_host_height,
        logical_body_width,
        logical_body_height,
        logical_bottom_inset,
    ) = match style {
        // Classic transcript is hosted by the separate click-through rail HWND on
        // Windows. The capsule HWND therefore owns only its 172px host, regardless
        // of whether the rail is visible.
        CapsuleStyle::Classic => (460.0, 172.0, 116.0, 32.0, 16.0),
        // Typeless content is scaled by zoom: 64px CSS × 0.447. The native window may be
        // 57px or 65px high when the dedicated translation row is present.
        CapsuleStyle::Typeless => (
            206.0,
            if translation_active { 65.0 } else { 57.0 },
            232.0 * 0.447,
            64.0 * 0.447,
            0.0,
        ),
        CapsuleStyle::Siri => unreachable!(),
    };
    let _ = transcript_visible;
    let scale =
        (window_width as f64 / logical_host_width).min(window_height as f64 / logical_host_height);
    // Windows clips the native input region as well as CSS. Include the language
    // picker only while translation is active; the transcript remains click-through.
    let picker_visible = style == CapsuleStyle::Classic && translation_active;
    let interactive_width = if picker_visible { 260.0 } else { logical_body_width };
    let interactive_height = logical_body_height + if picker_visible { 36.0 } else { 0.0 };
    let body_width = (interactive_width * scale).round() as i32;
    let body_height = (interactive_height * scale).round() as i32;
    let bottom_inset = (logical_bottom_inset * scale).round() as i32;
    let control_left = ((window_width as f64 - body_width as f64) / 2.0).round() as i32;
    let control_bottom = window_height - bottom_inset;
    Some(CapsuleHitTestRect {
        left: control_left.max(0),
        right: (control_left + body_width).min(window_width),
        top: (control_bottom - body_height).max(0),
        bottom: control_bottom.max(0),
    })
}

#[cfg(target_os = "windows")]
fn capsule_card_hit_rect(window_width: i32, window_height: i32) -> CapsuleHitTestRect {
    CapsuleHitTestRect {
        left: 0,
        right: window_width.max(0),
        top: 0,
        bottom: window_height.max(0),
    }
}

#[cfg(target_os = "windows")]
fn set_capsule_input_region(
    hwnd: windows::Win32::Foundation::HWND,
    rect: CapsuleHitTestRect,
) -> tauri::Result<()> {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ};

    if hwnd.0.is_null() {
        return Err(tauri::Error::Anyhow(anyhow::anyhow!(
            "cannot set input region for a null HWND"
        )));
    }

    let radius = ((rect.bottom - rect.top) / 2).max(1);
    let region =
        unsafe { CreateRoundRectRgn(rect.left, rect.top, rect.right, rect.bottom, radius, radius) };
    if region.0.is_null() {
        return Err(tauri::Error::Anyhow(anyhow::anyhow!(
            "CreateRoundRectRgn returned a null region"
        )));
    }
    let result = unsafe { SetWindowRgn(hwnd, region, BOOL(1)) };
    if result == 0 {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
        return Err(tauri::Error::Anyhow(anyhow::anyhow!("SetWindowRgn failed")));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn clear_capsule_input_region(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Gdi::{SetWindowRgn, HRGN};

    if hwnd.0.is_null() {
        return;
    }
    unsafe {
        let _ = SetWindowRgn(hwnd, HRGN::default(), BOOL(1));
    }
}

#[cfg(target_os = "windows")]
fn configure_capsule_hit_test<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    style: CapsuleStyle,
    transcript_visible: bool,
    translation_active: bool,
) -> tauri::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    let Ok(handle) = window.window_handle() else {
        return Ok(());
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = windows::Win32::Foundation::HWND(raw.hwnd.get() as *mut _);
    if hwnd.0.is_null() {
        return Err(tauri::Error::Anyhow(anyhow::anyhow!(
            "capsule window returned a null HWND"
        )));
    }
    let mut client = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut client).map_err(|error| tauri::Error::Anyhow(error.into()))?;
    }
    let Some(rect) = capsule_control_hit_rect(
        style,
        transcript_visible,
        translation_active,
        client.right - client.left,
        client.bottom - client.top,
    ) else {
        return Ok(());
    };
    set_capsule_input_region(hwnd, rect)?;
    if let Err(error) = install_capsule_hit_test_proc(hwnd, rect) {
        clear_capsule_input_region(hwnd);
        return Err(tauri::Error::Anyhow(error.into()));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn configure_card_hit_test<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    let Ok(handle) = window.window_handle() else {
        return Ok(());
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = windows::Win32::Foundation::HWND(raw.hwnd.get() as *mut _);
    if hwnd.0.is_null() {
        return Err(tauri::Error::Anyhow(anyhow::anyhow!(
            "card window returned a null HWND"
        )));
    }
    let mut client = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut client).map_err(|error| tauri::Error::Anyhow(error.into()))?;
    }
    let rect = capsule_card_hit_rect(client.right - client.left, client.bottom - client.top);
    clear_capsule_input_region(hwnd);
    install_capsule_hit_test_proc(hwnd, rect)
        .map_err(|error| tauri::Error::Anyhow(error.into()))
}

#[cfg(target_os = "windows")]
fn clear_capsule_input_region_for_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return Ok(());
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Ok(());
    };
    let hwnd = windows::Win32::Foundation::HWND(raw.hwnd.get() as *mut _);
    if !hwnd.0.is_null() {
        clear_capsule_input_region(hwnd);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapsuleShowStrategy {
    NoActivate,
    FallbackShow,
}

fn capsule_show_strategy_for_platform() -> CapsuleShowStrategy {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        CapsuleShowStrategy::NoActivate
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        CapsuleShowStrategy::FallbackShow
    }
}

fn capsule_state_log_name(state: CapsuleState) -> &'static str {
    match state {
        CapsuleState::Idle => "idle",
        CapsuleState::Recording => "recording",
        CapsuleState::Transcribing => "transcribing",
        CapsuleState::Polishing => "polishing",
        CapsuleState::Done => "done",
        CapsuleState::Cancelled => "cancelled",
        CapsuleState::Error => "error",
    }
}

pub(crate) fn show_capsule_window_for_recording<R: tauri::Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    reassert_spaces: bool,
) {
    #[cfg(target_os = "macos")]
    if crate::desktop_resources::active() {
        if let Ok(raw) = window.ns_window() {
            unsafe {
                use objc2::{msg_send, runtime::AnyObject};
                let panel = raw as *mut AnyObject;
                let _: () = msg_send![panel, orderFrontRegardless];
            }
        }
        return;
    }
    let mut needs_fallback = true;
    if capsule_show_strategy_for_platform() == CapsuleShowStrategy::NoActivate {
        needs_fallback = !show_capsule_window_no_activate(app, window, reassert_spaces);
        if needs_fallback && !CAPSULE_NO_ACTIVATE_FALLBACK_WARNED.swap(true, Ordering::SeqCst) {
            log::warn!("[capsule] no-activate show failed; falling back to window.show()");
        }
    }

    if needs_fallback {
        if let Err(error) = window.show() {
            log::warn!("[capsule] show fallback failed: {error}");
        }
    }
}

#[cfg(target_os = "windows")]
fn set_capsule_window_topmost_no_activate<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    show: bool,
) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };

    let Ok(handle) = window.window_handle() else {
        log::warn!(
            "[capsule] no_activate failed: window_handle() unavailable — Win32 presentation skipped"
        );
        return false;
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        log::warn!(
            "[capsule] no_activate failed: non-Win32 RawWindowHandle — Win32 presentation skipped"
        );
        return false;
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);
    if hwnd.0.is_null() {
        log::warn!("[capsule] no_activate failed: Win32 handle is null");
        return false;
    }

    if show {
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    }
    let position_result = if show {
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    } else {
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
    };
    if let Err(error) = position_result {
        log::warn!("[capsule] no_activate failed: SetWindowPos returned {error}");
        return false;
    }
    true
}

#[cfg(target_os = "windows")]
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    set_capsule_window_topmost_no_activate(window, true)
}

#[cfg(target_os = "macos")]
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    reassert_spaces: bool,
) -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(handle) = window.ns_window() else {
        return false;
    };
    let ns_window = handle as *mut AnyObject;
    if ns_window.is_null() {
        return false;
    }

    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    const BEHAVIOR: usize = CAN_JOIN_ALL_SPACES | STATIONARY | FULL_SCREEN_AUXILIARY;
    unsafe {
        let _: () = msg_send![ns_window, setLevel: 25i64];
        if reassert_spaces {
            let current: usize = msg_send![ns_window, collectionBehavior];
            if current != BEHAVIOR {
                log::warn!(
                    "[capsule] collectionBehavior drifted to {current} (expected {BEHAVIOR}); re-registering"
                );
            }
            let low = STATIONARY | FULL_SCREEN_AUXILIARY;
            let _: () = msg_send![ns_window, setCollectionBehavior: low];
        } else {
            let _: () = msg_send![ns_window, setCollectionBehavior: BEHAVIOR];
        }
        let _: () = msg_send![ns_window, orderFrontRegardless];
    }
    if reassert_spaces {
        let app = app.clone();
        let window = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            let _ = app.run_on_main_thread(move || {
                let Ok(handle) = window.ns_window() else {
                    return;
                };
                let ns_window = handle as *mut AnyObject;
                if ns_window.is_null() {
                    return;
                }
                unsafe {
                    let _: () = msg_send![ns_window, setCollectionBehavior: BEHAVIOR];
                }
            });
        });
    }
    true
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    false
}

/// Hide a capsule mirror through both Tauri and Win32 on Windows.
///
/// Mirrors are shown with a native `SetWindowPos(..., SWP_SHOWWINDOW)` path.
/// Use the matching native hide path so a successful Tauri hide cannot leave
/// the topmost mirror HWND visible after the target set becomes empty.
#[cfg(target_os = "windows")]
fn hide_capsule_mirror<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), &'static str> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SW_HIDE,
    };

    window.hide().map_err(|_| "tauri_hide_failed")?;
    let handle = window
        .window_handle()
        .map_err(|_| "win32_handle_unavailable")?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Err("non_win32_handle");
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);
    if hwnd.0.is_null() {
        return Err("null_win32_handle");
    }

    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        )
    }
    .map_err(|_| "win32_hide_failed")?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn hide_capsule_mirror<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), &'static str> {
    window.hide().map_err(|_| "tauri_hide_failed")
}

#[cfg(target_os = "windows")]
fn hide_capsule_window_if_present() {
    use std::iter::once;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    };

    let title: Vec<u16> = "OpenLess Capsule".encode_utf16().chain(once(0)).collect();
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        Ok(hwnd) => hwnd,
        Err(_) => return,
    };
    if hwnd == HWND::default() || hwnd.0.is_null() {
        return;
    }

    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        )
    };
}

#[cfg(not(target_os = "windows"))]
fn hide_capsule_window_if_present() {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapsuleWindowAction {
    PreserveFallbackCard,
    ShowCapsule,
    HideCapsule,
}

fn capsule_window_action(
    fallback_card_active: bool,
    show_capsule: bool,
    state: CapsuleState,
    qa_voice_active: bool,
) -> CapsuleWindowAction {
    if qa_voice_active {
        CapsuleWindowAction::HideCapsule
    } else if fallback_card_active {
        CapsuleWindowAction::PreserveFallbackCard
    } else if show_capsule && !matches!(state, CapsuleState::Idle) {
        CapsuleWindowAction::ShowCapsule
    } else {
        CapsuleWindowAction::HideCapsule
    }
}

fn should_present_capsule_window(window_visible_before: bool) -> bool {
    !window_visible_before
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CapsuleLayoutState {
    translation_active: bool,
    transcript_visible: bool,
    style: CapsuleStyle,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    work_x: i32,
    work_y: i32,
    work_width: u32,
    work_height: u32,
    scale_bits: u64,
    /// Hash of every connected monitor (Windows). Plugging a display in or out changes it
    /// even when the primary monitor is untouched, so cached placements are not reused.
    topology: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapsuleSnapshot {
    #[serde(flatten)]
    pub(crate) payload: CapsulePayload,
    pub(crate) transcript: String,
    pub(crate) sequence: u64,
    pub(crate) revision: u64,
    pub(crate) payload_revision: u64,
}

struct CapsuleMenuFrame {
    label: String,
    session_id: openless_core::SessionId,
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
}

fn capsule_menu_geometry(y: i32, height: u32, work_top: i32, scale: f64) -> (i32, u32) {
    let extra = ((320.0 * scale).round() as i32).min(y.saturating_sub(work_top).max(0));
    (y - extra, height.saturating_add(extra as u32))
}

struct CapsuleWindowState {
    layout: Mutex<Option<CapsuleLayoutState>>,
    capsule_visible: AtomicBool,
    qa_voice_active: AtomicBool,
    translation_menu: Mutex<Option<CapsuleMenuFrame>>,
    cursor_passthrough: AtomicBool,
    hit_test_mode: AtomicU8,
    transcript_visible: AtomicBool,
    style: AtomicU8,
    display_mode: AtomicU8,
    foreground_monitor: Mutex<Option<crate::capsule_mirrors::MonitorRect>>,
    foreground_session_id: Mutex<Option<String>>,
    foreground_target_captured: AtomicBool,
    fallback_card_visible: AtomicBool,
    fallback_presentation_id: AtomicU64,
    fallback_payload: Mutex<Option<crate::types::InsertFallbackCardPayload>>,
    deferred_payload: Mutex<Option<CapsulePayload>>,
    snapshot: Mutex<CapsuleSnapshotState>,
    layout_watch_active: AtomicBool,
    layout_watch_epoch: AtomicU64,
    /// Native display topology and presentation cache; preference changes invalidate it.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    mirror_fingerprint: Mutex<Option<u64>>,
}

impl Default for CapsuleWindowState {
    fn default() -> Self {
        Self {
            layout: Mutex::new(None),
            capsule_visible: AtomicBool::new(false),
            qa_voice_active: AtomicBool::new(false),
            translation_menu: Mutex::new(None),
            cursor_passthrough: AtomicBool::new(true),
            hit_test_mode: AtomicU8::new(HIT_TEST_MODE_CAPSULE),
            transcript_visible: AtomicBool::new(false),
            style: AtomicU8::new(0),
            display_mode: AtomicU8::new(0),
            foreground_monitor: Mutex::new(None),
            foreground_session_id: Mutex::new(None),
            foreground_target_captured: AtomicBool::new(false),
            fallback_card_visible: AtomicBool::new(false),
            fallback_presentation_id: AtomicU64::new(0),
            fallback_payload: Mutex::new(None),
            deferred_payload: Mutex::new(None),
            snapshot: Mutex::new(CapsuleSnapshotState::default()),
            layout_watch_active: AtomicBool::new(false),
            layout_watch_epoch: AtomicU64::new(0),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            mirror_fingerprint: Mutex::new(None),
        }
    }
}

const DISPLAY_MODE_CURSOR_SCREEN: u8 = 0;
const DISPLAY_MODE_PRIMARY_SCREEN: u8 = 1;
const DISPLAY_MODE_FOREGROUND_WINDOW_SCREEN: u8 = 2;
const DISPLAY_MODE_ALL_SCREENS: u8 = 3;

pub(crate) fn normalized_display_mode(mode: CapsuleDisplayMode) -> CapsuleDisplayMode {
    #[cfg(not(target_os = "windows"))]
    if matches!(
        mode,
        CapsuleDisplayMode::PrimaryScreen | CapsuleDisplayMode::ForegroundWindowScreen
    ) {
        return CapsuleDisplayMode::CursorScreen;
    }
    mode
}

fn display_mode_tag(mode: CapsuleDisplayMode) -> u8 {
    match mode {
        CapsuleDisplayMode::CursorScreen => DISPLAY_MODE_CURSOR_SCREEN,
        CapsuleDisplayMode::PrimaryScreen => DISPLAY_MODE_PRIMARY_SCREEN,
        CapsuleDisplayMode::ForegroundWindowScreen => DISPLAY_MODE_FOREGROUND_WINDOW_SCREEN,
        CapsuleDisplayMode::AllScreens => DISPLAY_MODE_ALL_SCREENS,
    }
}

fn display_mode_from_tag(tag: u8) -> CapsuleDisplayMode {
    match tag {
        DISPLAY_MODE_PRIMARY_SCREEN => CapsuleDisplayMode::PrimaryScreen,
        DISPLAY_MODE_FOREGROUND_WINDOW_SCREEN => CapsuleDisplayMode::ForegroundWindowScreen,
        DISPLAY_MODE_ALL_SCREENS => CapsuleDisplayMode::AllScreens,
        _ => CapsuleDisplayMode::CursorScreen,
    }
}

impl CapsuleWindowState {
    fn cache_display_mode(&self, mode: CapsuleDisplayMode) {
        let mode = normalized_display_mode(mode);
        let tag = display_mode_tag(mode);
        let changed = self.display_mode.swap(tag, Ordering::SeqCst) != tag;
        if changed {
            self.clear_foreground_target();
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                *self.mirror_fingerprint.lock() = None;
            }
        }
    }

    fn cached_display_mode(&self) -> CapsuleDisplayMode {
        display_mode_from_tag(self.display_mode.load(Ordering::SeqCst))
    }

    fn foreground_target_matches_session(&self, session_id: &str) -> bool {
        self.foreground_target_captured.load(Ordering::SeqCst)
            && self.foreground_session_id.lock().as_deref() == Some(session_id)
    }

    fn begin_foreground_target_capture(
        &self,
        session_id: &str,
        monitor: Option<crate::capsule_mirrors::MonitorRect>,
    ) {
        if self.foreground_target_matches_session(session_id) {
            return;
        }
        *self.foreground_session_id.lock() = Some(session_id.to_string());
        self.foreground_target_captured
            .store(true, Ordering::SeqCst);
        *self.foreground_monitor.lock() = monitor;
    }

    fn clear_foreground_target(&self) {
        self.foreground_target_captured
            .store(false, Ordering::SeqCst);
        *self.foreground_session_id.lock() = None;
        *self.foreground_monitor.lock() = None;
    }

    fn foreground_monitor(&self) -> Option<crate::capsule_mirrors::MonitorRect> {
        *self.foreground_monitor.lock()
    }

    fn cache_style(&self, style: CapsuleStyle) {
        self.style.store(
            match style {
                CapsuleStyle::Siri => 0,
                CapsuleStyle::Classic => 1,
                CapsuleStyle::Typeless => 2,
            },
            Ordering::Relaxed,
        );
    }

    fn cached_style(&self) -> CapsuleStyle {
        match self.style.load(Ordering::Relaxed) {
            1 => CapsuleStyle::Classic,
            2 => CapsuleStyle::Typeless,
            _ => CapsuleStyle::Siri,
        }
    }

    fn begin_fallback_card(&self) -> u64 {
        self.deferred_payload.lock().take();
        self.fallback_card_visible.store(true, Ordering::SeqCst);
        self.fallback_presentation_id
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1)
    }

    fn dismiss_fallback_card(&self) -> (bool, Option<CapsulePayload>) {
        let was_visible = self.fallback_card_visible.swap(false, Ordering::SeqCst);
        let deferred = was_visible
            .then(|| self.deferred_payload.lock().take())
            .flatten();
        (was_visible, deferred)
    }

    fn defer_if_fallback_active(&self, payload: &CapsulePayload) -> bool {
        let active = self.fallback_card_visible.load(Ordering::SeqCst);
        if active {
            *self.deferred_payload.lock() = Some(payload.clone());
        }
        active
    }

    fn active_fallback_presentation_id(&self) -> Option<u64> {
        self.fallback_card_visible
            .load(Ordering::SeqCst)
            .then(|| self.fallback_presentation_id.load(Ordering::SeqCst))
    }

    fn fallback_presentation_is_current(&self, presentation_id: u64) -> bool {
        self.active_fallback_presentation_id() == Some(presentation_id)
    }
}

/// Narrow Tauri window capability used by the compatibility coordinator.
///
/// The coordinator may schedule semantic capsule operations, but it never
/// receives an [`AppHandle`] or [`tauri::WebviewWindow`]. Keeping those handles
/// private prevents window code from becoming an accidental business API.
#[derive(Clone)]
pub(crate) struct TauriCapsuleWindow {
    app: AppHandle,
    #[cfg(target_os = "windows")]
    host: TauriCoordinatorHost,
    state: Arc<CapsuleWindowState>,
    qa_context: Arc<crate::qa_adapter::TauriQaHostContext>,
}

impl TauriCapsuleWindow {
    fn window(&self) -> Option<tauri::WebviewWindow> {
        self.app.get_webview_window("capsule")
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn target_monitor_for_layout(
        &self,
        window: &tauri::WebviewWindow,
    ) -> Option<crate::capsule_mirrors::MonitorRect> {
        #[cfg(target_os = "windows")]
        {
            return crate::capsule_mirrors::target_monitor_rect_for_mode(
                window,
                self.state.cached_display_mode(),
                self.state.foreground_monitor(),
            );
        }
        #[cfg(target_os = "macos")]
        {
            crate::capsule_mirrors::target_monitor_rect(window)
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn capture_foreground_monitor(&self) -> Option<crate::capsule_mirrors::MonitorRect> {
        self.qa_context
            .is_panel_visible()
            .then(|| self.qa_context.external_focus_target())
            .flatten()
            .or_else(crate::coordinator::capture_focus_target)
            .and_then(|hwnd| crate::capsule_mirrors::monitor_rect_for_hwnd(&self.app, hwnd))
    }

    #[cfg(target_os = "windows")]
    fn prepare_display_target(&self, payload: &CapsulePayload, _window: &tauri::WebviewWindow) {
        const NON_DICTATION_SESSION: &str = "__openless_non_dictation_foreground__";

        if payload.selection_polish {
            return;
        }
        match payload.state {
            CapsuleState::Recording => {
                if self.state.cached_display_mode() == CapsuleDisplayMode::ForegroundWindowScreen {
                    let session_id = payload
                        .session_id
                        .as_deref()
                        .unwrap_or(NON_DICTATION_SESSION);
                    if !self.state.foreground_target_matches_session(session_id) {
                        let monitor = self.capture_foreground_monitor();
                        self.state
                            .begin_foreground_target_capture(session_id, monitor);
                    }
                }
            }
            // Keep Done/Cancelled/Error on the recording target for their visible terminal frame;
            // Idle clears it after the terminal animation has been hidden.
            CapsuleState::Idle => self.state.clear_foreground_target(),
            CapsuleState::Done | CapsuleState::Cancelled | CapsuleState::Error => {}
            CapsuleState::Transcribing | CapsuleState::Polishing => {}
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn prepare_display_target(&self, payload: &CapsulePayload, _window: &tauri::WebviewWindow) {
        if !payload.selection_polish
            && matches!(
                payload.state,
                CapsuleState::Idle
                    | CapsuleState::Done
                    | CapsuleState::Cancelled
                    | CapsuleState::Error
            )
        {
            self.state.clear_foreground_target();
        }
    }

    fn rail_window(&self) -> Option<tauri::WebviewWindow> {
        self.app.get_webview_window("capsule-rail")
    }

    fn hide_transcript_overlay(&self) {
        if let Some(window) = self.rail_window() {
            let _ = window.hide();
        }
    }

    #[cfg(target_os = "windows")]
    fn position_transcript_overlay(
        &self,
        capsule: &tauri::WebviewWindow,
        style: CapsuleStyle,
        translation_active: bool,
        visible: bool,
    ) -> tauri::Result<()> {
        let Some(rail) = self.rail_window() else {
            return Ok(());
        };
        let scale = capsule.scale_factor().unwrap_or(1.0);
        let main_position = capsule.outer_position()?;
        position_rail_for_capsule(
            &self.app,
            &rail,
            main_position,
            scale,
            style,
            translation_active,
            visible,
        )
    }

    pub(crate) fn is_available_for(&self, state: CapsuleState) -> bool {
        let available = self.window().is_some();
        if !available && !CAPSULE_WINDOW_MISSING_LOGGED.swap(true, Ordering::SeqCst) {
            log::warn!(
                "[capsule] capsule webview window not found — show path skipped (state={})",
                capsule_state_log_name(state)
            );
        }
        available
    }

    pub(crate) fn run_on_main_thread<F>(&self, task: F) -> Result<(), String>
    where
        F: FnOnce(Self) + Send + 'static,
    {
        let capsule = self.clone();
        self.app
            .run_on_main_thread(move || task(capsule))
            .map_err(|error| error.to_string())
    }

    fn restore_translation_menu(&self, owner: Option<(&str, openless_core::SessionId)>) -> Result<bool, String> {
        let frame = {
            let mut slot = self.state.translation_menu.lock();
            if let Some((label, session_id)) = owner {
                if !slot.as_ref().is_some_and(|frame| frame.label == label && frame.session_id == session_id) { return Ok(false); }
            }
            slot.take()
        };
        let Some(frame) = frame else { return Ok(false); };
        #[cfg(not(mobile))]
        crate::hotkey::set_capsule_menu_open(false);
        self.invalidate_layout();
        if let Some(window) = self.app.get_webview_window(&frame.label) {
            window.set_size(frame.size).map_err(|error| error.to_string())?;
            window.set_position(frame.position).map_err(|error| error.to_string())?;
            #[cfg(target_os = "windows")]
            configure_capsule_hit_test(&window, self.state.cached_style(),
                self.state.transcript_visible.load(Ordering::SeqCst), true).map_err(|error| error.to_string())?;
        }
        Ok(true)
    }

    /// Reserve room for the shared popup above the pill without moving its bottom anchor.
    pub(crate) fn set_translation_menu_open(&self, label: String, session_id: openless_core::SessionId, open: bool) -> Result<(), String> {
        if !open {
            if self.restore_translation_menu(Some((&label, session_id)))? && self.state.capsule_visible.load(Ordering::SeqCst) {
                self.start_layout_watch();
                self.refresh_style();
                self.sync_capsule_mirrors();
            }
            return Ok(());
        }
        if self.state.qa_voice_active.load(Ordering::SeqCst) { return Err("qa_owns_voice_surface".into()); }
        if self.state.translation_menu.lock().as_ref().is_some_and(|frame| frame.label == label && frame.session_id == session_id) { return Ok(()); }
        self.restore_translation_menu(None)?;
        let window = self.app.get_webview_window(&label).ok_or("capsule_window_unavailable")?;
        let position = window.outer_position().map_err(|error| error.to_string())?;
        let size = window.inner_size().map_err(|error| error.to_string())?;
        let scale = window.scale_factor().map_err(|error| error.to_string())?;
        let work_top = window.current_monitor().map_err(|error| error.to_string())?
            .map(|monitor| monitor.work_area().position.y).unwrap_or(position.y - (320.0 * scale) as i32);
        let (top, height) = capsule_menu_geometry(position.y, size.height, work_top, scale);
        *self.state.translation_menu.lock() = Some(CapsuleMenuFrame {label, session_id, position, size});
        #[cfg(not(mobile))]
        crate::hotkey::set_capsule_menu_open(true);
        self.stop_layout_watch();
        self.hide_transcript_overlay();
        let result = (|| -> tauri::Result<()> {
            window.set_position(tauri::PhysicalPosition::new(position.x, top))?;
            window.set_size(tauri::PhysicalSize::new(size.width, height))?;
            #[cfg(not(mobile))]
            window.set_ignore_cursor_events(false)?;
            #[cfg(target_os = "windows")]
            configure_card_hit_test(&window)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = self.restore_translation_menu(None);
            self.start_layout_watch();
            return Err(error.to_string());
        }
        Ok(())
    }

    pub(crate) fn set_size(&self, width: f64, height: f64) -> tauri::Result<()> {
        // Cards temporarily borrow this window; their geometry belongs to the card until released.
        self.stop_layout_watch();
        if let Some(window) = self.window() {
            window.set_size(tauri::LogicalSize::new(width, height))?;
        }
        Ok(())
    }

    #[cfg(not(mobile))]
    pub(crate) fn set_cursor_passthrough(&self, passthrough: bool) -> tauri::Result<()> {
        if let Some(window) = self.window() {
            #[cfg(target_os = "windows")]
            if passthrough {
                clear_capsule_input_region_for_window(&window)?;
            } else if self.state.hit_test_mode.load(Ordering::SeqCst) == HIT_TEST_MODE_CARD {
                configure_card_hit_test(&window)?;
            } else {
                let translation_active = self
                    .state
                    .snapshot
                    .lock()
                    .payload
                    .as_ref()
                    .is_some_and(|payload| payload.translation);
                configure_capsule_hit_test(
                    &window,
                    self.state.cached_style(),
                    self.state.transcript_visible.load(Ordering::SeqCst),
                    translation_active,
                )?;
            }
            window.set_ignore_cursor_events(passthrough)?;
            self.state
                .cursor_passthrough
                .store(passthrough, Ordering::SeqCst);
        }
        self.sync_capsule_mirrors();
        Ok(())
    }

    /// Rebuild the real client hit region after a card resize. The card owns the
    /// shared HWND, so its whole current client area must be interactive; using
    /// the previous capsule region would make Copy/Dismiss intermittently miss.
    pub(crate) fn refresh_card_hit_test(&self) -> tauri::Result<()> {
        if self.state.hit_test_mode.load(Ordering::SeqCst) != HIT_TEST_MODE_CARD {
            return Ok(());
        }
        #[cfg(target_os = "windows")]
        if let Some(window) = self.window() {
            configure_card_hit_test(&window)?;
        }
        Ok(())
    }

    #[cfg(not(mobile))]
    pub(crate) fn set_card_hit_test_mode(&self) -> tauri::Result<()> {
        if crate::desktop_resources::active() {
            if let Some(coordinator) = self.app.try_state::<Arc<crate::coordinator::Coordinator>>()
            {
                coordinator.tauri_host().stop_native_capsule();
            }
        }
        crate::desktop_resources::ensure_card_window(&self.app)?;
        self.state
            .hit_test_mode
            .store(HIT_TEST_MODE_CARD, Ordering::SeqCst);
        self.hide_transcript_overlay();
        if let Some(window) = self.window() {
            window.set_ignore_cursor_events(false)?;
            self.state.cursor_passthrough.store(false, Ordering::SeqCst);
            #[cfg(target_os = "windows")]
            configure_card_hit_test(&window)?;
        }
        self.sync_capsule_mirrors();
        Ok(())
    }

    #[cfg(not(mobile))]
    pub(crate) fn restore_capsule_hit_test_mode(&self) -> tauri::Result<()> {
        self.state
            .hit_test_mode
            .store(HIT_TEST_MODE_CAPSULE, Ordering::SeqCst);
        self.set_cursor_passthrough(true)?;
        // Restoring a card must also restore the capsule geometry/rail state that
        // was current before the card took ownership of the window.
        self.invalidate_layout();
        if let Some(window) = self.window() {
            let translation = self
                .state
                .snapshot
                .lock()
                .payload
                .as_ref()
                .is_some_and(|payload| payload.translation);
            self.maybe_position_capsule_bottom_center(
                &window,
                translation,
                self.state.cached_style(),
            );
        }
        Ok(())
    }

    pub(crate) fn invalidate_layout(&self) {
        *self.state.layout.lock() = None;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            *self.state.mirror_fingerprint.lock() = None;
        }
    }

    /// Apply the user's display-target choice on the native main thread.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(crate) fn sync_capsule_mirrors(&self) {
        let _ = self.run_on_main_thread(|capsule| capsule.apply_capsule_mirrors());
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub(crate) fn sync_capsule_mirrors(&self) {}

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn apply_capsule_mirrors(&self) {
        if self.state.translation_menu.lock().is_some() { return; }
        use crate::capsule_mirrors::{
            capsule_placement, ensure_capsule_mirror, mirror_label, mirror_targets,
            CAPSULE_MIRROR_PREFIX, MAX_CAPSULE_MIRRORS, RAIL_MIRROR_PREFIX,
        };

        let Some(primary) = self.window() else {
            return;
        };
        let mode = self.state.cached_display_mode();
        let Some(primary_rect) = self.target_monitor_for_layout(&primary) else {
            return;
        };
        let monitors = crate::capsule_mirrors::all_monitor_rects(&primary);
        let card_mode = self.state.hit_test_mode.load(Ordering::SeqCst) == HIT_TEST_MODE_CARD;
        // Card modes borrow the primary HWND; mirrors must not show a stale capsule over them.
        let visible = self.state.capsule_visible.load(Ordering::SeqCst)
            && !card_mode
            && !self.state.fallback_card_visible.load(Ordering::SeqCst);
        let style = self.state.cached_style();
        let translation_active = self
            .state
            .snapshot
            .lock()
            .payload
            .as_ref()
            .is_some_and(|payload| payload.translation);
        let transcript_visible = self.state.transcript_visible.load(Ordering::SeqCst);
        let passthrough = self.state.cursor_passthrough.load(Ordering::SeqCst);

        let style_tag = match style {
            CapsuleStyle::Siri => 0u8,
            CapsuleStyle::Classic => 1,
            CapsuleStyle::Typeless => 2,
        };
        let fingerprint = crate::capsule_mirrors::mirror_fingerprint(
            visible,
            passthrough,
            style_tag,
            translation_active,
            transcript_visible,
            primary_rect,
            &monitors,
        );
        if *self.state.mirror_fingerprint.lock() == Some(fingerprint) {
            return;
        }

        let app = self.app.clone();
        let targets = if visible {
            mirror_targets(mode, &monitors, primary_rect)
        } else {
            Vec::new()
        };

        // Reuse a bounded set of webviews. Recreating WKWebViews on every
        // recording/mode switch accumulates native resources on macOS.
        let hidden_payload = self
            .state
            .snapshot
            .lock()
            .payload
            .clone()
            .map(|mut payload| {
                payload.state = CapsuleState::Idle;
                payload
            });
        let mut complete = true;
        for index in targets.len()..MAX_CAPSULE_MIRRORS {
            for prefix in [CAPSULE_MIRROR_PREFIX, RAIL_MIRROR_PREFIX] {
                if let Some(window) = app.get_webview_window(&mirror_label(prefix, index)) {
                    let hide_result = if prefix == CAPSULE_MIRROR_PREFIX {
                        hide_capsule_mirror(&window).map_err(|error| error.to_string())
                    } else {
                        window.hide().map_err(|error| error.to_string())
                    };
                    if let Err(error) = hide_result {
                        log::warn!("[capsule] mirror hide failed: {error}");
                        complete = false;
                    }
                    // Stop hidden shader/exit animations; future live frames are
                    // routed only to visible mirror windows.
                    if let Some(payload) = &hidden_payload {
                        let _ = app.emit_to(window.label(), "capsule:state", payload);
                    }
                }
            }
        }

        let bounds = crate::capsule_window_bounds_for_style_with_transcript_and_translation(
            style,
            cfg!(target_os = "macos") && transcript_visible,
            translation_active,
        );
        let bottom_gap = crate::capsule_bottom_gap(style);

        // Only a fully applied sync may be cached; failed operations are retried.
        for (index, monitor) in targets.iter().enumerate() {
            let capsule_label = mirror_label(CAPSULE_MIRROR_PREFIX, index);
            let mirror = match ensure_capsule_mirror(&app, &capsule_label) {
                Ok(window) => window,
                Err(error) => {
                    log::warn!("[capsule] mirror {capsule_label} create failed: {error}");
                    complete = false;
                    continue;
                }
            };
            let (phys_w, phys_h, x, y) =
                capsule_placement(bounds.width, bounds.height, bottom_gap, *monitor);
            #[cfg(target_os = "macos")]
            let _ = (phys_w, phys_h);
            // Geometry failures must not be fingerprint-cached: a skipped retry would leave
            // the mirror stuck at the wrong size/position after a monitor reconnect.
            #[cfg(target_os = "windows")]
            let size_result = mirror.set_size(tauri::PhysicalSize::new(phys_w, phys_h));
            #[cfg(target_os = "macos")]
            let size_result = mirror.set_size(tauri::LogicalSize::new(bounds.width, bounds.height));
            if let Err(error) = size_result {
                log::warn!("[capsule] mirror {capsule_label} set_size failed: {error}");
                complete = false;
                continue;
            }
            #[cfg(target_os = "windows")]
            let position_result = mirror.set_position(tauri::PhysicalPosition::new(x, y));
            #[cfg(target_os = "macos")]
            let position_result = mirror.set_position(tauri::LogicalPosition::new(
                x as f64 / monitor.scale,
                y as f64 / monitor.scale,
            ));
            if let Err(error) = position_result {
                log::warn!("[capsule] mirror {capsule_label} set_position failed: {error}");
                complete = false;
                continue;
            }
            #[cfg(target_os = "windows")]
            let hit_test = if passthrough {
                clear_capsule_input_region_for_window(&mirror)
            } else {
                configure_capsule_hit_test(&mirror, style, transcript_visible, translation_active)
            };
            #[cfg(target_os = "windows")]
            if let Err(error) = hit_test {
                log::warn!("[capsule] mirror {capsule_label} hit-test setup failed: {error}");
                complete = false;
            }
            if let Err(error) = mirror.set_ignore_cursor_events(passthrough) {
                log::warn!("[capsule] mirror {capsule_label} passthrough failed: {error}");
                complete = false;
            }
            show_capsule_window_for_recording(&app, &mirror, false);
            if !mirror.is_visible().unwrap_or(false) {
                log::warn!("[capsule] mirror {capsule_label} remained hidden after show");
                complete = false;
            }

            #[cfg(target_os = "windows")]
            {
                let rail_label = mirror_label(RAIL_MIRROR_PREFIX, index);
                match crate::capsule_mirrors::ensure_rail_mirror(&app, &rail_label) {
                    Ok(rail) => {
                        if let Err(error) = position_rail_for_capsule(
                            &app,
                            &rail,
                            tauri::PhysicalPosition::new(x, y),
                            monitor.scale,
                            style,
                            translation_active,
                            transcript_visible,
                        ) {
                            log::warn!("[capsule] mirror {rail_label} positioning failed: {error}");
                            complete = false;
                        }
                    }
                    Err(error) => {
                        log::warn!("[capsule] mirror {rail_label} create failed: {error}");
                        complete = false;
                    }
                }
            }
        }

        *self.state.mirror_fingerprint.lock() = complete.then_some(fingerprint);
    }

    pub(crate) fn hide(&self) -> tauri::Result<()> {
        let _ = self.restore_translation_menu(None);
        self.state.capsule_visible.store(false, Ordering::SeqCst);
        self.stop_layout_watch();
        self.hide_transcript_overlay();
        if let Some(window) = self.window() {
            #[cfg(not(mobile))]
            {
                if crate::desktop_resources::active() {
                    self.state
                        .hit_test_mode
                        .store(HIT_TEST_MODE_CAPSULE, Ordering::SeqCst);
                }
                crate::desktop_resources::hide_capsule_window(&window)?;
            }
            #[cfg(mobile)]
            window.hide()?;
        }
        self.sync_capsule_mirrors();
        Ok(())
    }

    pub(crate) fn position_vocab_card(
        &self,
        width: f64,
        height: f64,
        edge_margin: f64,
    ) -> tauri::Result<()> {
        let Some(window) = self.window() else {
            return Ok(());
        };
        let Some(monitor) = window.current_monitor()? else {
            return Ok(());
        };
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let position = monitor.position();
        let monitor_width = size.width as f64 / scale;
        let monitor_height = size.height as f64 / scale;
        let monitor_x = position.x as f64 / scale;
        let monitor_y = position.y as f64 / scale;
        window.set_position(tauri::LogicalPosition::new(
            monitor_x + monitor_width - width - edge_margin,
            monitor_y + monitor_height - height - 80.0,
        ))
    }

    pub(crate) fn position_fallback_card(&self, width: f64, height: f64) -> tauri::Result<()> {
        let Some(window) = self.window() else {
            return Ok(());
        };
        let Some(monitor) = window.current_monitor()? else {
            return Ok(());
        };
        let scale = monitor.scale_factor();
        let size = monitor.size();
        let position = monitor.position();
        let monitor_width = size.width as f64 / scale;
        let monitor_height = size.height as f64 / scale;
        let monitor_x = position.x as f64 / scale;
        let monitor_y = position.y as f64 / scale;
        window.set_position(tauri::LogicalPosition::new(
            monitor_x + (monitor_width - width) / 2.0,
            monitor_y + monitor_height - height - 80.0,
        ))
    }

    pub(crate) fn position_capsule_bottom_center(&self, translation: bool) -> tauri::Result<()> {
        if let Some(window) = self.window() {
            crate::position_capsule_bottom_center_with_style_and_transcript(
                &window,
                translation,
                self.state.cached_style(),
                self.state.transcript_visible.load(Ordering::SeqCst),
            )?;
        }
        Ok(())
    }

    /// Restore the capsule window from the cached style and latest payload.
    /// Card dismissal must not guess Siri geometry or discard the rail state.
    pub(crate) fn restore_capsule_geometry(&self) -> tauri::Result<()> {
        self.invalidate_layout();
        if let Some(window) = self.window() {
            let translation = self
                .state
                .snapshot
                .lock()
                .payload
                .as_ref()
                .is_some_and(|payload| payload.translation);
            self.maybe_position_capsule_bottom_center(
                &window,
                translation,
                self.state.cached_style(),
            );
        }
        Ok(())
    }

    pub(crate) fn set_transcript_visible(&self, visible: bool) -> tauri::Result<()> {
        if !self.state.capsule_visible.load(Ordering::SeqCst) {
            self.state.transcript_visible.store(false, Ordering::SeqCst);
            self.hide_transcript_overlay();
            return Ok(());
        }
        self.state
            .transcript_visible
            .store(visible, Ordering::SeqCst);
        if self.state.hit_test_mode.load(Ordering::SeqCst) == HIT_TEST_MODE_CARD {
            // Card geometry owns this shared window until dismissal. The card webview may
            // report transcriptVisible=false, but that must not restore capsule bounds.
            self.hide_transcript_overlay();
            return Ok(());
        }
        self.state.layout.lock().take();
        if let Some(window) = self.window() {
            let translation = self
                .state
                .snapshot
                .lock()
                .payload
                .as_ref()
                .is_some_and(|payload| payload.translation);
            self.maybe_position_capsule_bottom_center(
                &window,
                translation,
                self.state.cached_style(),
            );
            #[cfg(target_os = "windows")]
            if self.state.hit_test_mode.load(Ordering::SeqCst) != HIT_TEST_MODE_CARD
                && !self.state.cursor_passthrough.load(Ordering::SeqCst)
            {
                configure_capsule_hit_test(
                    &window,
                    self.state.cached_style(),
                    visible,
                    translation,
                )?;
            }
        }
        Ok(())
    }

    fn layout_snapshot(
        &self,
        window: &tauri::WebviewWindow,
        translation_active: bool,
        style: CapsuleStyle,
    ) -> Option<CapsuleLayoutState> {
        #[cfg(target_os = "windows")]
        {
            // Use the same selected display as placement, including mixed-DPI layouts.
            if let Some(mon) = self.target_monitor_for_layout(window) {
                let monitors = crate::capsule_mirrors::all_monitor_rects(window);
                return Some(CapsuleLayoutState {
                    translation_active,
                    transcript_visible: self.state.transcript_visible.load(Ordering::SeqCst),
                    style,
                    monitor_x: mon.left,
                    monitor_y: mon.top,
                    monitor_width: (mon.right - mon.left).max(0) as u32,
                    monitor_height: (mon.bottom - mon.top).max(0) as u32,
                    work_x: mon.work_left,
                    work_y: mon.work_top,
                    work_width: (mon.work_right - mon.work_left).max(0) as u32,
                    work_height: (mon.work_bottom - mon.work_top).max(0) as u32,
                    scale_bits: mon.scale.to_bits(),
                    topology: crate::capsule_mirrors::topology_signature(&monitors),
                });
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(mon) = crate::capsule_target_monitor(window) {
                return Some(CapsuleLayoutState {
                    translation_active,
                    transcript_visible: self.state.transcript_visible.load(Ordering::SeqCst),
                    style,
                    monitor_x: mon.physical_x,
                    monitor_y: mon.physical_y,
                    monitor_width: mon.physical_width,
                    monitor_height: mon.physical_height,
                    work_x: mon.work_x,
                    work_y: mon.work_y,
                    work_width: mon.work_width,
                    work_height: mon.work_height,
                    scale_bits: mon.scale.to_bits(),
                    topology: 0,
                });
            }
        }
        let monitor = window.current_monitor().ok().flatten()?;
        Some(CapsuleLayoutState {
            translation_active,
            transcript_visible: self.state.transcript_visible.load(Ordering::SeqCst),
            style,
            monitor_x: monitor.position().x,
            monitor_y: monitor.position().y,
            monitor_width: monitor.size().width,
            monitor_height: monitor.size().height,
            work_x: monitor.work_area().position.x,
            work_y: monitor.work_area().position.y,
            work_width: monitor.work_area().size.width,
            work_height: monitor.work_area().size.height,
            scale_bits: monitor.scale_factor().to_bits(),
            topology: 0,
        })
    }

    fn maybe_position_capsule_bottom_center(
        &self,
        window: &tauri::WebviewWindow,
        translation_active: bool,
        style: CapsuleStyle,
    ) {
        if self.state.translation_menu.lock().is_some() { return; }
        let Some(next) = self.layout_snapshot(window, translation_active, style) else {
            return;
        };
        let layout_changed = self.state.layout.lock().as_ref() != Some(&next);
        if layout_changed {
            #[cfg(target_os = "windows")]
            let position_result = self
                .target_monitor_for_layout(window)
                .map(|monitor| {
                    crate::position_capsule_on_monitor(window, translation_active, style, monitor)
                })
                .unwrap_or(Ok(()));
            #[cfg(not(target_os = "windows"))]
            let position_result = crate::position_capsule_bottom_center_with_style_and_transcript(
                window,
                translation_active,
                style,
                self.state.transcript_visible.load(Ordering::SeqCst),
            );
            if position_result.is_ok() {
                *self.state.layout.lock() = Some(next);
            }
        }
        #[cfg(target_os = "windows")]
        if self.state.capsule_visible.load(Ordering::SeqCst) {
            if let Err(error) = self.position_transcript_overlay(
                window,
                style,
                translation_active,
                self.state.transcript_visible.load(Ordering::SeqCst),
            ) {
                log::warn!("[capsule] transcript overlay positioning failed: {error}");
            }
        } else {
            self.hide_transcript_overlay();
        }
        self.sync_capsule_mirrors();
        #[cfg(target_os = "windows")]
        if layout_changed
            && self.state.hit_test_mode.load(Ordering::SeqCst) != HIT_TEST_MODE_CARD
            && !self.state.cursor_passthrough.load(Ordering::SeqCst)
        {
            if let Err(error) = configure_capsule_hit_test(
                window,
                style,
                self.state.transcript_visible.load(Ordering::SeqCst),
                translation_active,
            ) {
                log::warn!("[capsule] configure client-band hit testing failed: {error}");
            }
        }
    }

    pub(crate) fn show_for_recording(&self, reassert_spaces: bool) {
        if self.state.qa_voice_active.load(Ordering::SeqCst) { let _ = self.hide(); return; }
        self.state.capsule_visible.store(true, Ordering::SeqCst);
        if let Some(window) = self.window() {
            show_capsule_window_for_recording(&self.app, &window, reassert_spaces);
        }
        self.sync_capsule_mirrors();
    }

    fn stop_layout_watch(&self) {
        self.state
            .layout_watch_active
            .store(false, Ordering::SeqCst);
        self.state.layout_watch_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Audio callbacks stop during remote processing, but Dock and monitor changes must still apply.
    fn start_layout_watch(&self) {
        if self.state.layout_watch_active.swap(true, Ordering::SeqCst) {
            return;
        }
        let epoch = self
            .state
            .layout_watch_epoch
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        let state = Arc::clone(&self.state);
        // The Windows timer must not own a Tauri handle: both cloning it on a Tokio
        // worker and dropping it when the timer exits can race the event loop's Rc.
        #[cfg(target_os = "windows")]
        let host = self.host.clone();
        #[cfg(not(target_os = "windows"))]
        let capsule = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if state.layout_watch_epoch.load(Ordering::SeqCst) != epoch {
                    break;
                }
                #[cfg(target_os = "windows")]
                {
                    let host_for_main = host.clone();
                    host.on_main_thread(move || {
                        if let Some(capsule) = host_for_main.capsule_window() {
                            capsule.refresh_watched_layout(epoch);
                        }
                    });
                }
                #[cfg(not(target_os = "windows"))]
                let _ = capsule.run_on_main_thread(move |capsule| {
                    capsule.refresh_watched_layout(epoch);
                });
            }
        });
    }

    fn refresh_watched_layout(&self, epoch: u64) {
        // A stopped/restarted watch can still have an old tick queued on the main thread.
        if self.state.layout_watch_epoch.load(Ordering::SeqCst) != epoch {
            return;
        }
        let Some(window) = self.window() else {
            self.stop_layout_watch();
            return;
        };
        if !window.is_visible().unwrap_or(false) {
            self.stop_layout_watch();
            return;
        }
        let payload = self.state.snapshot.lock().payload.clone();
        if let Some(payload) = payload {
            self.maybe_position_capsule_bottom_center(
                &window,
                payload.translation,
                self.state.cached_style(),
            );
        }
    }

    fn update_cursor_for_payload(&self, payload: &CapsulePayload, style: CapsuleStyle) {
        #[cfg(not(mobile))]
        {
            let interactive = style != CapsuleStyle::Siri
                && !payload.selection_polish
                && matches!(
                    payload.state,
                    CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
                );
            let want_passthrough = !interactive;
            if self.state.cursor_passthrough.load(Ordering::SeqCst) != want_passthrough {
                if let Err(error) = self.set_cursor_passthrough(want_passthrough) {
                    log::warn!("[capsule] set_ignore_cursor_events failed: {error}");
                }
            }
        }
    }

    fn refresh_style(&self) {
        let Some(window) = self.window() else {
            return;
        };
        if !window.is_visible().unwrap_or(false)
            || self.state.fallback_card_visible.load(Ordering::SeqCst)
            || !self.state.layout_watch_active.load(Ordering::SeqCst)
        {
            return;
        }
        let payload = self.state.snapshot.lock().payload.clone();
        if let Some(payload) = payload {
            let style = self.state.cached_style();
            self.maybe_position_capsule_bottom_center(&window, payload.translation, style);
            self.update_cursor_for_payload(&payload, style);
        }
    }

    pub(crate) fn apply_capsule_payload(
        &self,
        payload: &CapsulePayload,
        show_capsule: bool,
        style: CapsuleStyle,
        reassert_spaces: bool,
    ) {
        if payload.state != CapsuleState::Recording || !payload.translation {
            let _ = self.restore_translation_menu(None);
        }
        self.state.cache_style(style);
        let Some(window) = self.window() else {
            return;
        };
        let window_visible_before = window.is_visible().unwrap_or(false);
        self.prepare_display_target(payload, &window);
        let fallback_card_active = self.state.defer_if_fallback_active(payload);

        {
            let action = capsule_window_action(fallback_card_active, show_capsule, payload.state, self.state.qa_voice_active.load(Ordering::SeqCst));
            if action == CapsuleWindowAction::PreserveFallbackCard {
                log::debug!(
                    "[capsule] native window update deferred: insert fallback card owns the window"
                );
                return;
            }

            match action {
                CapsuleWindowAction::ShowCapsule => {
                    self.state.capsule_visible.store(true, Ordering::SeqCst);
                }
                CapsuleWindowAction::HideCapsule => {
                    self.state.capsule_visible.store(false, Ordering::SeqCst);
                    self.hide_transcript_overlay();
                }
                CapsuleWindowAction::PreserveFallbackCard => unreachable!(),
            }

            if action == CapsuleWindowAction::ShowCapsule && !window_visible_before {
                // The OS can move a hidden window (e.g. when a monitor is unplugged), so a
                // placement cached while it was visible must not be reused on the next show.
                self.invalidate_layout();
            }
            self.maybe_position_capsule_bottom_center(&window, payload.translation, style);
            self.update_cursor_for_payload(payload, style);
            self.sync_capsule_mirrors();

            match action {
                CapsuleWindowAction::PreserveFallbackCard => unreachable!(),
                CapsuleWindowAction::ShowCapsule => {
                    self.start_layout_watch();
                    if !CAPSULE_FIRST_SHOW_LOGGED.swap(true, Ordering::SeqCst) {
                        log::info!(
                            "[capsule] first show this session: show_capsule=true visible=true state={}",
                            capsule_state_log_name(payload.state)
                        );
                    }
                    if should_present_capsule_window(window_visible_before) {
                        show_capsule_window_for_recording(&self.app, &window, reassert_spaces);
                    } else {
                        // On Windows, leave an already visible WebView untouched. Reasserting
                        // HWND_TOPMOST for every streaming update can make the compositor replay
                        // its entrance animation, which users perceive as the capsule reopening
                        // for each character.
                        #[cfg(not(target_os = "windows"))]
                        {
                            show_capsule_window_for_recording(&self.app, &window, reassert_spaces);
                        }
                    }
                    #[cfg(target_os = "macos")]
                    crate::restore_main_window_key_if_active(&self.app);
                }
                CapsuleWindowAction::HideCapsule => {
                    self.stop_layout_watch();
                    if !show_capsule
                        && !matches!(payload.state, CapsuleState::Idle)
                        && !CAPSULE_SUPPRESSED_BY_TOGGLE_LOGGED.swap(true, Ordering::SeqCst)
                    {
                        log::info!(
                            "[capsule] suppressed by user toggle: show_capsule=false visible=true state={}",
                            capsule_state_log_name(payload.state)
                        );
                    }
                    hide_capsule_window_if_present();
                    let _ = self.hide();
                    if matches!(
                        payload.state,
                        CapsuleState::Done | CapsuleState::Cancelled | CapsuleState::Error
                    ) {
                        self.state.clear_foreground_target();
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn restore_main_window_key_if_active(&self) {
        crate::restore_main_window_key_if_active(&self.app);
    }
}

#[derive(Clone)]
pub(crate) struct TauriCoordinatorHost {
    app: crate::core_adapters::AppHandleSlot,
    capsule: Arc<CapsuleWindowState>,
    #[cfg(not(mobile))]
    native: Arc<Mutex<Option<Arc<crate::native_capsule::NativeCapsule>>>>,
    qa_context: Arc<crate::qa_adapter::TauriQaHostContext>,
}

impl TauriCoordinatorHost {
    pub(crate) fn new(
        app: crate::core_adapters::AppHandleSlot,
        qa_context: Arc<crate::qa_adapter::TauriQaHostContext>,
    ) -> Self {
        Self {
            app,
            capsule: Arc::new(CapsuleWindowState::default()),
            #[cfg(not(mobile))]
            native: Arc::new(Mutex::new(None)),
            qa_context,
        }
    }

    pub(crate) fn bind(&self, app: AppHandle) {
        // `bind` runs in Tauri's `setup`, on the event-loop thread.
        #[cfg(target_os = "windows")]
        let _ = MAIN_THREAD.set(std::thread::current().id());
        #[cfg(not(mobile))]
        if crate::desktop_resources::active() {
            match crate::native_capsule::NativeCapsule::new(app.clone()) {
                Ok(native) => {
                    *self.native.lock() = Some(Arc::new(native));
                }
                Err(error) => {
                    log::error!("[native-capsule] {error}; dictation remains available");
                }
            }
        }
        *self.app.lock() = Some(app);
    }

    /// Runs `task` on the event-loop thread.
    ///
    /// Tauri handles (`AppHandle`, windows, `TauriCapsuleWindow`) share the event loop through a
    /// non-atomic `Rc`, so cloning or dropping one off the main thread races the main thread's
    /// refcount and eventually frees the event loop while it runs (heap corruption 0xc0000374).
    /// This posts through the bound handle by reference instead of cloning it on the caller.
    #[cfg(target_os = "windows")]
    pub(crate) fn on_main_thread<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if MAIN_THREAD.get() == Some(&std::thread::current().id()) {
            task();
            return;
        }
        // On a worker thread `run_on_main_thread` only queues through the event-loop proxy, so
        // holding the slot lock here cannot re-enter it.
        if let Some(app) = self.app.lock().as_ref() {
            let _ = app.run_on_main_thread(task);
        }
    }

    #[cfg(not(mobile))]
    pub(crate) fn present_native_capsule(
        &self,
        payload: &CapsulePayload,
        epoch: u64,
        previous: Option<CapsuleState>,
    ) -> bool {
        if !crate::desktop_resources::active() {
            return false;
        }
        if let Some(app) = self.app() {
            let preferences = app
                .state::<Arc<crate::coordinator::Coordinator>>()
                .backend()
                .get_preferences();
            if payload.state == CapsuleState::Recording && previous != Some(CapsuleState::Recording)
            {
                if preferences.audio_cue_on_record
                    && !preferences.mute_during_recording
                    && !payload.selection_polish
                {
                    crate::native_audio_cue::play_start();
                }
            } else if payload.state != CapsuleState::Recording
                && previous == Some(CapsuleState::Recording)
            {
                crate::native_audio_cue::stop();
            }
        }
        if let Some(native) = self.native.lock().as_ref() {
            if self.capsule.fallback_card_visible.load(Ordering::SeqCst)
                || self.capsule.hit_test_mode.load(Ordering::SeqCst) == HIT_TEST_MODE_CARD
            {
                native.stop();
            } else {
                native.present(payload, epoch, self.capsule.snapshot.lock().text.clone());
            }
        }
        true
    }

    #[cfg(not(mobile))]
    pub(crate) fn set_native_capsule_locale(&self, locale: String) {
        if let Some(native) = self.native.lock().as_ref() {
            native.set_locale(locale);
        }
    }

    #[cfg(not(mobile))]
    pub(crate) fn refresh_native_capsule_theme(&self) {
        if let Some(native) = self.native.lock().as_ref() {
            native.refresh_theme();
        }
    }

    #[cfg(not(mobile))]
    pub(crate) fn stop_native_capsule(&self) {
        if let Some(native) = self.native.lock().as_ref() {
            native.stop();
        }
        crate::native_audio_cue::stop();
    }

    fn app(&self) -> Option<AppHandle> {
        self.app.lock().clone()
    }

    pub(crate) fn is_bound(&self) -> bool {
        self.app.lock().is_some()
    }

    pub(crate) fn capsule_window(&self) -> Option<TauriCapsuleWindow> {
        self.app().map(|app| TauriCapsuleWindow {
            app,
            #[cfg(target_os = "windows")]
            host: self.clone(),
            state: Arc::clone(&self.capsule),
            qa_context: Arc::clone(&self.qa_context),
        })
    }

    pub(crate) fn set_qa_voice_active(&self, active: bool) {
        if self.capsule.qa_voice_active.swap(active, Ordering::SeqCst) == active { return; }
        if active {
            if let Some(capsule) = self.capsule_window() {
                let _ = capsule.run_on_main_thread(|capsule| { let _ = capsule.hide(); });
            }
        }
    }

    pub(crate) fn cached_capsule_style(&self) -> CapsuleStyle {
        self.capsule.cached_style()
    }

    pub(crate) fn capsule_snapshot(&self) -> Option<CapsuleSnapshot> {
        let snapshot = self.capsule.snapshot.lock().clone();
        if snapshot.pending_payload_revision.is_some()
            || snapshot.payload_revision != snapshot.revision
        {
            // A rail ready replay must never observe a payload from one
            // revision together with transcript/session data from another.
            return None;
        }
        let mut payload = snapshot.payload?;
        payload.session_id = snapshot.session_id.clone();
        Some(CapsuleSnapshot {
            payload,
            transcript: snapshot.text,
            sequence: snapshot.sequence,
            revision: snapshot.revision,
            payload_revision: snapshot.payload_revision,
        })
    }

    /// Commit the replay frame before the webview/main-thread work is queued.
    /// The rail snapshot is an IPC replay source, so it must not wait for a
    /// window callback to become internally consistent.
    pub(crate) fn capsule_revision(&self) -> u64 {
        self.capsule.snapshot.lock().revision
    }

    pub(crate) fn commit_capsule_payload(
        &self,
        payload: &CapsulePayload,
        captured_revision: u64,
    ) -> bool {
        self.capsule
            .snapshot
            .lock()
            .commit_capsule_payload(payload, captured_revision)
    }

    /// Update replay state without waiting for the native window callback.
    pub(crate) fn record_backend_event(&self, event: &BackendEvent) {
        self.capsule.snapshot.lock().record_backend_event(event);
        #[cfg(not(mobile))]
        if crate::desktop_resources::active()
            && matches!(
                event.kind,
                openless_core::BackendEventKind::TranscriptDelta(_)
            )
        {
            // Streaming text may arrive after capture stops, with no further level frames.
            let (session_id, text) = {
                let snapshot = self.capsule.snapshot.lock();
                (snapshot.session_id.clone(), snapshot.text.clone())
            };
            if let (Some(session_id), Some(native)) = (session_id, self.native.lock().as_ref()) {
                native.update_transcript(&session_id, text);
            }
        }
    }

    pub(crate) fn cache_capsule_display_mode(&self, mode: CapsuleDisplayMode) {
        self.capsule.cache_display_mode(mode);
    }

    pub(crate) fn cache_capsule_style(&self, style: CapsuleStyle) {
        self.capsule.cache_style(style);
        if let Some(capsule) = self.capsule_window() {
            let _ = capsule.run_on_main_thread(|capsule| capsule.refresh_style());
        }
    }

    pub(crate) fn begin_insert_fallback_card(&self) -> u64 {
        self.capsule.begin_fallback_card()
    }

    pub(crate) fn dismiss_insert_fallback_card(&self) -> (bool, Option<CapsulePayload>) {
        self.capsule.dismiss_fallback_card()
    }

    pub(crate) fn defer_capsule_if_fallback_active(&self, payload: &CapsulePayload) -> bool {
        self.capsule.defer_if_fallback_active(payload)
    }

    pub(crate) fn auxiliary_fallback_snapshot(
        &self,
    ) -> Option<crate::types::InsertFallbackCardPayload> {
        self.capsule.fallback_payload.lock().clone()
    }

    pub(crate) fn active_insert_fallback_presentation_id(&self) -> Option<u64> {
        self.capsule.active_fallback_presentation_id()
    }

    pub(crate) fn insert_fallback_presentation_is_current(&self, presentation_id: u64) -> bool {
        self.capsule
            .fallback_presentation_is_current(presentation_id)
    }

    pub(crate) fn run_on_main_thread<F>(&self, task: F) -> Result<(), String>
    where
        F: FnOnce() + Send + 'static,
    {
        let app = self
            .app()
            .ok_or_else(|| "Tauri AppHandle is not bound".to_string())?;
        app.run_on_main_thread(task)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn spawn<F>(&self, future: F) -> tauri::async_runtime::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tauri::async_runtime::spawn(future)
    }

    pub(crate) fn spawn_blocking<F, R>(&self, task: F) -> tauri::async_runtime::JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        tauri::async_runtime::spawn_blocking(task)
    }

    pub(crate) fn block_on<F: Future>(&self, future: F) -> F::Output {
        tauri::async_runtime::block_on(future)
    }

    pub(crate) fn show_less_computer(&self) -> Result<(), String> {
        let app = self
            .app()
            .ok_or_else(|| "Tauri AppHandle is not bound yet".to_string())?;
        crate::show_less_computer_window(&app)
    }

    pub(crate) fn set_voice_edit_interactive(&self, interactive: bool) {
        if let Some(app) = self.app() {
            crate::set_voice_edit_interactive(&app, interactive);
        }
    }

    pub(crate) fn show_voice_edit(&self) {
        if let Some(app) = self.app() {
            crate::show_voice_edit_window(&app);
        }
    }

    pub(crate) fn hide_voice_edit(&self) {
        if let Some(app) = self.app() {
            crate::hide_voice_edit_window(&app);
        }
    }

    pub(crate) fn hide_less_computer(&self) {
        if let Some(app) = self.app() {
            crate::hide_less_computer_window(&app);
            crate::hide_less_computer_glow(&app);
        }
    }

    pub(crate) fn hide_less_computer_glow(&self) {
        if let Some(app) = self.app() {
            crate::hide_less_computer_glow(&app);
        }
    }

    pub(crate) fn show_less_computer_glow(&self) {
        if let Some(app) = self.app() {
            crate::show_less_computer_glow(&app);
        }
    }

    pub(crate) fn show_main_window(&self) {
        let Some(app) = self.app() else {
            return;
        };
        let app_for_main = app.clone();
        let _ = app.run_on_main_thread(move || crate::show_main_window(&app_for_main));
    }

    pub(crate) fn refresh_tray_microphone_menu(&self) {
        let Some(app) = self.app() else {
            return;
        };
        let app_for_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Err(error) = crate::refresh_tray_microphone_menu(&app_for_main) {
                log::warn!("[tray] refresh style menu after switch style hotkey failed: {error}");
            }
        });
    }

    pub(crate) fn activate_style_pack_by_id(
        &self,
        coordinator: &crate::coordinator::Coordinator,
        pack_id: &str,
    ) -> Result<crate::types::StylePack, String> {
        let app = self
            .app()
            .ok_or_else(|| "Tauri AppHandle is not bound".to_string())?;
        crate::commands::activate_style_pack_by_id(coordinator, &app, pack_id)
    }

    pub(crate) fn emit_insert_fallback(&self, payload: &crate::types::InsertFallbackCardPayload) {
        *self.capsule.fallback_payload.lock() = Some(payload.clone());
        if let Some(app) = self.app() {
            let _ = app.emit_to("capsule", "insert:fallback", payload);
        }
    }

    pub(crate) fn clear_insert_fallback(&self) {
        self.capsule.fallback_payload.lock().take();
        if let Some(app) = self.app() {
            let _ = app.emit_to(
                "capsule",
                "insert:fallback",
                None::<crate::types::InsertFallbackCardPayload>,
            );
        }
    }

    pub(crate) fn emit_capsule_state_to_capsule(&self, payload: &crate::types::CapsulePayload) {
        if let Some(app) = self.app() {
            let _ = app.emit_to("capsule", "capsule:state", payload);
            let _ = app.emit_to("capsule-rail", "capsule:state", payload);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            for prefix in [
                crate::capsule_mirrors::CAPSULE_MIRROR_PREFIX,
                crate::capsule_mirrors::RAIL_MIRROR_PREFIX,
            ] {
                for label in crate::capsule_mirrors::existing_mirror_labels(&app, prefix) {
                    if app
                        .get_webview_window(&label)
                        .is_some_and(|window| window.is_visible().unwrap_or(false))
                    {
                        let _ = app.emit_to(label.as_str(), "capsule:state", payload);
                    }
                }
            }
        }
    }

    pub(crate) fn emit_capsule_state_to_main(&self, payload: &crate::types::CapsulePayload) {
        if let Some(app) = self.app() {
            let _ = app.emit_to("main", "capsule:state", payload);
        }
    }

    #[cfg(not(mobile))]
    pub(crate) fn emit_fn_shortcut_pressed(&self) {
        if let Some(app) = self.app() {
            let _ = app.emit("fn-shortcut-pressed", ());
        }
    }

    #[cfg(all(not(mobile), target_os = "windows"))]
    pub(crate) fn show_selection_voice_intent_prompt(&self) {
        if let Some(app) = self.app() {
            crate::show_selection_voice_intent_prompt(&app);
        }
    }

    #[cfg(all(not(mobile), target_os = "windows"))]
    pub(crate) fn hide_selection_voice_intent_prompt(&self) {
        if let Some(app) = self.app() {
            crate::hide_selection_voice_intent_prompt(&app);
        }
    }

    #[cfg(not(mobile))]
    pub(crate) fn stop_microphone_preview(&self, owner: &str) {
        let Some(app) = self.app() else {
            return;
        };
        let state = app.state::<crate::commands::MicrophoneMonitorState>();
        let _operation = state.operation.lock();
        let recorder = state.recorder.lock().take();
        if let Some(recorder) = recorder {
            log::info!("[recorder] stopping microphone preview monitor before {owner}");
            recorder.stop();
        }
    }

    pub(crate) async fn switch_to_ascii(
        &self,
    ) -> Result<
        Option<crate::unicode_keystroke::PreviousInputSource>,
        crate::unicode_keystroke::TisError,
    > {
        let app = self.app().ok_or_else(|| {
            crate::unicode_keystroke::TisError::MainThreadDispatch(
                "Tauri AppHandle is not bound".to_string(),
            )
        })?;
        crate::unicode_keystroke::switch_to_ascii(&app).await
    }

    pub(crate) async fn restore_input_source(
        &self,
        previous: Option<crate::unicode_keystroke::PreviousInputSource>,
    ) -> Result<(), crate::unicode_keystroke::TisError> {
        let app = self.app().ok_or_else(|| {
            crate::unicode_keystroke::TisError::MainThreadDispatch(
                "Tauri AppHandle is not bound".to_string(),
            )
        })?;
        crate::unicode_keystroke::restore_input_source(&app, previous).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_mode_changes_invalidate_mirrors_without_changing_style() {
        let state = CapsuleWindowState::default();
        assert_eq!(
            state.cached_display_mode(),
            CapsuleDisplayMode::CursorScreen
        );
        for mode in [
            CapsuleDisplayMode::AllScreens,
            CapsuleDisplayMode::PrimaryScreen,
            CapsuleDisplayMode::ForegroundWindowScreen,
            CapsuleDisplayMode::CursorScreen,
        ] {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                *state.mirror_fingerprint.lock() = Some(123);
            }
            let previous_mode = state.cached_display_mode();
            state.cache_display_mode(mode);
            #[cfg(target_os = "windows")]
            assert_eq!(state.cached_display_mode(), mode);
            #[cfg(not(target_os = "windows"))]
            let expected_mode = if matches!(
                mode,
                CapsuleDisplayMode::PrimaryScreen | CapsuleDisplayMode::ForegroundWindowScreen
            ) {
                CapsuleDisplayMode::CursorScreen
            } else {
                mode
            };
            #[cfg(not(target_os = "windows"))]
            assert_eq!(state.cached_display_mode(), expected_mode);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if state.cached_display_mode() != previous_mode {
                assert!(state.mirror_fingerprint.lock().is_none());
            } else {
                assert_eq!(*state.mirror_fingerprint.lock(), Some(123));
            }
            assert_eq!(state.cached_style(), CapsuleStyle::Siri);
        }
    }

    #[test]
    fn foreground_target_capture_is_one_shot_until_session_reset() {
        let state = CapsuleWindowState::default();
        let monitor = crate::capsule_mirrors::MonitorRect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
            work_left: -1920,
            work_top: 0,
            work_right: 0,
            work_bottom: 1040,
            scale: 1.0,
            identity: 0,
        };
        state.begin_foreground_target_capture("session-a", Some(monitor));
        state.begin_foreground_target_capture("session-a", None);
        assert_eq!(state.foreground_monitor(), Some(monitor));
        assert!(state.foreground_target_matches_session("session-a"));
        assert!(!state.foreground_target_matches_session("session-b"));
        assert!(state.foreground_target_captured.load(Ordering::SeqCst));
        state.clear_foreground_target();
        assert_eq!(state.foreground_monitor(), None);
        assert!(!state.foreground_target_captured.load(Ordering::SeqCst));
        state.begin_foreground_target_capture("session-b", None);
        assert!(state.foreground_target_matches_session("session-b"));
        assert_eq!(state.foreground_monitor(), None);
    }

    #[test]
    fn capsule_style_cache_preserves_every_wire_variant() {
        let state = CapsuleWindowState::default();
        assert_eq!(state.cached_style(), CapsuleStyle::Siri);
        for style in [
            CapsuleStyle::Classic,
            CapsuleStyle::Typeless,
            CapsuleStyle::Siri,
        ] {
            state.cache_style(style);
            assert_eq!(state.cached_style(), style);
        }
    }

    #[test]
    fn capsule_layout_cache_notices_work_area_and_style_changes() {
        let initial = CapsuleLayoutState {
            translation_active: false,
            transcript_visible: false,
            style: CapsuleStyle::Classic,
            monitor_x: 0,
            monitor_y: 0,
            monitor_width: 1920,
            monitor_height: 1080,
            work_x: 0,
            work_y: 0,
            work_width: 1920,
            work_height: 1040,
            scale_bits: 1.0_f64.to_bits(),
            topology: 7,
        };
        assert_ne!(
            initial,
            CapsuleLayoutState {
                work_height: 1080,
                ..initial
            }
        );
        // Plugging a monitor in/out must invalidate the cached placement even when the
        // primary monitor itself is unchanged.
        assert_ne!(
            initial,
            CapsuleLayoutState {
                topology: 8,
                ..initial
            }
        );
        assert_ne!(
            initial,
            CapsuleLayoutState {
                style: CapsuleStyle::Typeless,
                ..initial
            }
        );
    }

    #[test]
    fn capsule_is_presented_only_after_a_hidden_to_visible_transition() {
        assert!(should_present_capsule_window(false));
        assert!(!should_present_capsule_window(true));
    }

    #[test]
    fn capsule_show_strategy_matches_platform_activation_contract() {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        assert_eq!(
            capsule_show_strategy_for_platform(),
            CapsuleShowStrategy::NoActivate
        );

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(
            capsule_show_strategy_for_platform(),
            CapsuleShowStrategy::FallbackShow
        );
    }

    #[test]
    fn dedicated_qa_voice_hides_all_shared_capsule_presentations() {
        for state in [CapsuleState::Recording, CapsuleState::Transcribing, CapsuleState::Polishing, CapsuleState::Done, CapsuleState::Error, CapsuleState::Idle] {
            for fallback in [true, false] {
                assert_eq!(capsule_window_action(fallback, true, state, true), CapsuleWindowAction::HideCapsule);
            }
        }
        assert_eq!(capsule_window_action(false, true, CapsuleState::Recording, false), CapsuleWindowAction::ShowCapsule);
    }

    #[test]
    fn language_menu_expands_upward_and_preserves_the_bottom_at_each_scale() {
        for (y,height,top,scale) in [(600,172,24,1.0), (1200,344,48,2.0), (-200,172,-1000,1.0), (100,172,24,1.0)] {
            let (expanded_y, expanded_height) = super::capsule_menu_geometry(y,height,top,scale);
            assert!(expanded_y >= top);
            assert_eq!(expanded_y + expanded_height as i32, y + height as i32);
            assert!(expanded_height >= height);
        }
    }

    #[test]
    fn fallback_card_owns_native_window_until_dismissed() {
        for state in [
            CapsuleState::Idle,
            CapsuleState::Recording,
            CapsuleState::Polishing,
            CapsuleState::Done,
        ] {
            assert_eq!(
                capsule_window_action(true, true, state, false),
                CapsuleWindowAction::PreserveFallbackCard
            );
        }
    }

    #[test]
    fn capsule_window_action_follows_visibility_without_fallback_card() {
        assert_eq!(
            capsule_window_action(false, true, CapsuleState::Recording, false),
            CapsuleWindowAction::ShowCapsule
        );
        assert_eq!(
            capsule_window_action(false, true, CapsuleState::Idle, false),
            CapsuleWindowAction::HideCapsule
        );
        assert_eq!(
            capsule_window_action(false, false, CapsuleState::Recording, false),
            CapsuleWindowAction::HideCapsule
        );
    }

    #[test]
    fn capsule_transcript_rail_position_uses_body_content_for_all_styles() {
        let siri = capsule_transcript_rail_position(CapsuleStyle::Siri, false, true).unwrap();
        assert_eq!(
            (siri.width, siri.height, siri.top_offset, siri.gap),
            (460.0, 40.0, -48.0, 8.0)
        );
        assert_eq!(
            capsule_transcript_rail_position(CapsuleStyle::Siri, true, true),
            Some(siri)
        );

        let classic = capsule_transcript_rail_position(CapsuleStyle::Classic, false, true).unwrap();
        assert_eq!(
            (
                classic.width,
                classic.height,
                classic.top_offset,
                classic.gap
            ),
            (460.0, 52.0, 64.0, 8.0)
        );
        assert_eq!(
            capsule_transcript_rail_position(CapsuleStyle::Classic, true, true),
            Some(CapsuleTranscriptRailPosition {
                width: 460.0,
                height: 88.0,
                top_offset: 28.0,
                gap: 8.0,
            })
        );

        let typeless =
            capsule_transcript_rail_position(CapsuleStyle::Typeless, false, true).unwrap();
        assert_eq!(typeless.width, 206.0);
        assert_eq!(typeless.gap, 0.0);
        assert!((typeless.height - 52.0 * 0.447).abs() < f64::EPSILON);
        assert!((typeless.top_offset - (57.0 - 64.0 * 0.447 - 52.0 * 0.447)).abs() < f64::EPSILON);

        let typeless_translation =
            capsule_transcript_rail_position(CapsuleStyle::Typeless, true, true).unwrap();
        assert!(
            (typeless_translation.top_offset - (65.0 - 20.0 * 0.447 - 64.0 * 0.447 - 52.0 * 0.447))
                .abs()
                < 1e-12,
            "unexpected typeless translation rail offset: {}",
            typeless_translation.top_offset
        );

        for style in [
            CapsuleStyle::Siri,
            CapsuleStyle::Classic,
            CapsuleStyle::Typeless,
        ] {
            assert_eq!(capsule_transcript_rail_position(style, false, false), None);
        }
        let classic_badge =
            capsule_transcript_rail_position(CapsuleStyle::Classic, true, false).unwrap();
        assert_eq!(
            (classic_badge.height, classic_badge.top_offset),
            (28.0, 88.0)
        );
        let typeless_badge =
            capsule_transcript_rail_position(CapsuleStyle::Typeless, true, false).unwrap();
        assert!((typeless_badge.height - 20.0 * 0.447).abs() < 1e-12);
        assert!((typeless_translation.height - 72.0 * 0.447).abs() < 1e-12);
        assert_eq!(
            capsule_transcript_rail_position(CapsuleStyle::Siri, true, false),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_hit_test_uses_x_and_y_and_translation_bounds() {
        assert_eq!(
            capsule_control_hit_rect(CapsuleStyle::Classic, true, false, 460, 172),
            Some(CapsuleHitTestRect {
                left: 172,
                right: 288,
                top: 124,
                bottom: 156,
            })
        );
        assert_eq!(
            capsule_control_hit_rect(CapsuleStyle::Classic, false, false, 460, 172),
            Some(CapsuleHitTestRect {
                left: 172,
                right: 288,
                top: 124,
                bottom: 156,
            })
        );
        let typeless = capsule_control_hit_rect(CapsuleStyle::Typeless, true, true, 206, 65)
            .expect("translation typeless hit rect");
        assert_eq!((typeless.left, typeless.right), (51, 155));
        assert_eq!((typeless.top, typeless.bottom), (36, 65));
        assert!(typeless.left > 0 && typeless.right < 206);
        assert!(typeless.top > 0 && typeless.bottom <= 65);
        let typeless_compact =
            capsule_control_hit_rect(CapsuleStyle::Typeless, false, false, 206, 57)
                .expect("compact typeless hit rect");
        assert_eq!((typeless_compact.top, typeless_compact.bottom), (28, 57));
        assert_eq!(
            capsule_control_hit_rect(CapsuleStyle::Siri, false, false, 460, 180),
            None
        );
        assert_eq!(
            capsule_card_hit_rect(320, 140),
            CapsuleHitTestRect {
                left: 0,
                right: 320,
                top: 0,
                bottom: 140,
            }
        );
    }
}
