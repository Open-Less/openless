//! Tauri-only host operations used by the compatibility coordinator.
//!
//! The shared backend never sees this module. It owns the late-bound
//! [`tauri::AppHandle`] and keeps window, main-thread and managed-state access
//! out of the coordinator's business paths.

#[cfg(target_os = "windows")]
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[path = "capsule_snapshot.rs"]
mod capsule_snapshot;
use capsule_snapshot::CapsuleSnapshotState;
use openless_core::BackendEvent;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::types::{CapsulePayload, CapsuleState, CapsuleStyle};

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
            const BODY_HEIGHT: f64 = 52.0;
            const RAIL_HEIGHT: f64 = 52.0;
            const RAIL_GAP: f64 = 8.0;
            const BADGE_HEIGHT: f64 = 22.0;
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

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
struct CapsuleHitTestEntry {
    previous_proc: isize,
    control_left: i32,
    control_right: i32,
    control_top: i32,
    control_bottom: i32,
}

#[cfg(target_os = "windows")]
static CAPSULE_HIT_TEST_ENTRIES: OnceLock<Mutex<HashMap<isize, CapsuleHitTestEntry>>> =
    OnceLock::new();

#[cfg(target_os = "windows")]
fn capsule_hit_test_entries() -> &'static Mutex<HashMap<isize, CapsuleHitTestEntry>> {
    CAPSULE_HIT_TEST_ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CapsuleHitTestRect {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
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
        CapsuleStyle::Classic => (460.0, 172.0, 196.0, 52.0, 16.0),
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
    let body_width = (logical_body_width * scale).round() as i32;
    let body_height = (logical_body_height * scale).round() as i32;
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
unsafe extern "system" fn capsule_hit_test_window_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, GetWindowRect, SetWindowLongPtrW, GWLP_WNDPROC,
        HTTRANSPARENT, WM_NCDESTROY, WM_NCHITTEST,
    };

    if msg == WM_NCDESTROY {
        let previous_proc = capsule_hit_test_entries()
            .lock()
            .remove(&(hwnd.0 as isize))
            .map(|entry| entry.previous_proc)
            .unwrap_or_default();
        if previous_proc != 0 {
            let previous: windows::Win32::UI::WindowsAndMessaging::WNDPROC =
                std::mem::transmute(previous_proc);
            let _ = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, previous_proc);
            return CallWindowProcW(previous, hwnd, msg, wparam, lparam);
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    if msg == WM_NCHITTEST {
        let key = hwnd.0 as isize;
        let entry = capsule_hit_test_entries().lock().get(&key).copied();
        if let Some(entry) = entry {
            let screen_x = (lparam.0 as i32 & 0xffff) as u16 as i16 as i32;
            let screen_y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let client_x = screen_x - rect.left;
                let client_y = screen_y - rect.top;
                if client_x < entry.control_left
                    || client_x >= entry.control_right
                    || client_y < entry.control_top
                    || client_y >= entry.control_bottom
                {
                    return windows::Win32::Foundation::LRESULT(HTTRANSPARENT as isize);
                }
            }
        }
    }

    let previous_proc = capsule_hit_test_entries()
        .lock()
        .get(&(hwnd.0 as isize))
        .map(|entry| entry.previous_proc)
        .unwrap_or_default();
    if previous_proc != 0 {
        let previous: windows::Win32::UI::WindowsAndMessaging::WNDPROC =
            std::mem::transmute(previous_proc);
        CallWindowProcW(previous, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[cfg(target_os = "windows")]
fn install_capsule_hit_test_proc(
    hwnd: windows::Win32::Foundation::HWND,
    rect: CapsuleHitTestRect,
) -> tauri::Result<()> {
    use windows::Win32::Foundation::{GetLastError, SetLastError, ERROR_SUCCESS};
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_WNDPROC};

    if hwnd.0.is_null() {
        return Err(tauri::Error::Anyhow(anyhow::anyhow!(
            "cannot install hit-test proc for a null HWND"
        )));
    }

    let key = hwnd.0 as isize;
    let mut entries = capsule_hit_test_entries().lock();
    if let Some(entry) = entries.get_mut(&key) {
        entry.control_left = rect.left;
        entry.control_right = rect.right;
        entry.control_top = rect.top;
        entry.control_bottom = rect.bottom;
        return Ok(());
    }
    unsafe { SetLastError(ERROR_SUCCESS) };
    let previous = unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            capsule_hit_test_window_proc as usize as isize,
        )
    };
    if previous == 0 && unsafe { GetLastError() } != ERROR_SUCCESS {
        return Err(tauri::Error::Anyhow(anyhow::anyhow!(
            "SetWindowLongPtrW(GWLP_WNDPROC) failed"
        )));
    }
    entries.insert(
        key,
        CapsuleHitTestEntry {
            previous_proc: previous,
            control_left: rect.left,
            control_right: rect.right,
            control_top: rect.top,
            control_bottom: rect.bottom,
        },
    );
    Ok(())
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
        return Err(error);
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
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };

    let Ok(handle) = window.window_handle() else {
        log::warn!(
            "[capsule] no_activate failed: window_handle() unavailable — Win32 show skipped"
        );
        return false;
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        log::warn!("[capsule] no_activate failed: non-Win32 RawWindowHandle — Win32 show skipped");
        return false;
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);
    if hwnd.0.is_null() {
        log::warn!("[capsule] no_activate failed: Win32 handle is null");
        return false;
    }

    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    if let Err(error) = unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    } {
        log::warn!("[capsule] no_activate failed: SetWindowPos returned {error}");
        return false;
    }
    true
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
) -> CapsuleWindowAction {
    if fallback_card_active {
        CapsuleWindowAction::PreserveFallbackCard
    } else if show_capsule && !matches!(state, CapsuleState::Idle) {
        CapsuleWindowAction::ShowCapsule
    } else {
        CapsuleWindowAction::HideCapsule
    }
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

struct CapsuleWindowState {
    layout: Mutex<Option<CapsuleLayoutState>>,
    capsule_visible: AtomicBool,
    cursor_passthrough: AtomicBool,
    hit_test_mode: AtomicU8,
    transcript_visible: AtomicBool,
    style: AtomicU8,
    fallback_card_visible: AtomicBool,
    fallback_presentation_id: AtomicU64,
    deferred_payload: Mutex<Option<CapsulePayload>>,
    snapshot: Mutex<CapsuleSnapshotState>,
    layout_watch_active: AtomicBool,
    layout_watch_epoch: AtomicU64,
}

impl Default for CapsuleWindowState {
    fn default() -> Self {
        Self {
            layout: Mutex::new(None),
            capsule_visible: AtomicBool::new(false),
            cursor_passthrough: AtomicBool::new(true),
            hit_test_mode: AtomicU8::new(HIT_TEST_MODE_CAPSULE),
            transcript_visible: AtomicBool::new(false),
            style: AtomicU8::new(0),
            fallback_card_visible: AtomicBool::new(false),
            fallback_presentation_id: AtomicU64::new(0),
            deferred_payload: Mutex::new(None),
            snapshot: Mutex::new(CapsuleSnapshotState::default()),
            layout_watch_active: AtomicBool::new(false),
            layout_watch_epoch: AtomicU64::new(0),
        }
    }
}

impl CapsuleWindowState {
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
    state: Arc<CapsuleWindowState>,
}

impl TauriCapsuleWindow {
    fn window(&self) -> Option<tauri::WebviewWindow> {
        self.app.get_webview_window("capsule")
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
        let Some(position) = capsule_transcript_rail_position(style, translation_active, visible)
        else {
            rail.hide()?;
            return Ok(());
        };
        let scale = capsule.scale_factor().unwrap_or(1.0);
        let main_position = capsule.outer_position()?;
        rail.set_ignore_cursor_events(true)?;
        rail.set_size(tauri::LogicalSize::new(position.width, position.height))?;
        rail.set_position(tauri::PhysicalPosition::new(
            main_position.x,
            main_position.y + (position.top_offset * scale).round() as i32,
        ))?;
        // The rail is a sibling overlay and must never steal focus from the
        // application under the capsule, just like the main capsule HWND.
        show_capsule_window_for_recording(&self.app, &rail, false);
        Ok(())
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
    }

    pub(crate) fn hide(&self) -> tauri::Result<()> {
        self.state.capsule_visible.store(false, Ordering::SeqCst);
        self.stop_layout_watch();
        self.hide_transcript_overlay();
        if let Some(window) = self.window() {
            window.hide()?;
        }
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
            if let Some(mon) = crate::foreground_window_monitor() {
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
        })
    }

    fn maybe_position_capsule_bottom_center(
        &self,
        window: &tauri::WebviewWindow,
        translation_active: bool,
        style: CapsuleStyle,
    ) {
        let Some(next) = self.layout_snapshot(window, translation_active, style) else {
            return;
        };
        let layout_changed = self.state.layout.lock().as_ref() != Some(&next);
        if layout_changed
            && crate::position_capsule_bottom_center_with_style_and_transcript(
                window,
                translation_active,
                style,
                self.state.transcript_visible.load(Ordering::SeqCst),
            )
            .is_ok()
        {
            *self.state.layout.lock() = Some(next);
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
        self.state.capsule_visible.store(true, Ordering::SeqCst);
        if let Some(window) = self.window() {
            show_capsule_window_for_recording(&self.app, &window, reassert_spaces);
        }
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
        let capsule = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if capsule.state.layout_watch_epoch.load(Ordering::SeqCst) != epoch {
                    break;
                }
                let _ = capsule.run_on_main_thread(move |capsule| {
                    if capsule.state.layout_watch_epoch.load(Ordering::SeqCst) != epoch {
                        return;
                    }
                    let Some(window) = capsule.window() else {
                        capsule.stop_layout_watch();
                        return;
                    };
                    if !window.is_visible().unwrap_or(false) {
                        capsule.stop_layout_watch();
                        return;
                    }
                    let payload = capsule.state.snapshot.lock().payload.clone();
                    if let Some(payload) = payload {
                        capsule.maybe_position_capsule_bottom_center(
                            &window,
                            payload.translation,
                            capsule.state.cached_style(),
                        );
                    }
                });
            }
        });
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
        self.state.cache_style(style);
        let Some(window) = self.window() else {
            return;
        };
        let fallback_card_active = self.state.defer_if_fallback_active(payload);

        {
            let action = capsule_window_action(fallback_card_active, show_capsule, payload.state);
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

            self.maybe_position_capsule_bottom_center(&window, payload.translation, style);
            self.update_cursor_for_payload(payload, style);

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
                    show_capsule_window_for_recording(&self.app, &window, reassert_spaces);
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
}

impl TauriCoordinatorHost {
    pub(crate) fn new(app: crate::core_adapters::AppHandleSlot) -> Self {
        Self {
            app,
            capsule: Arc::new(CapsuleWindowState::default()),
        }
    }

    pub(crate) fn bind(&self, app: AppHandle) {
        *self.app.lock() = Some(app);
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
            state: Arc::clone(&self.capsule),
        })
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

    #[cfg(target_os = "macos")]
    pub(crate) fn local_qwen_asr(
        &self,
        engine: std::sync::Arc<crate::asr::local::LocalQwenEngine>,
    ) -> anyhow::Result<std::sync::Arc<crate::asr::local::LocalQwenAsr>> {
        let app = self
            .app()
            .ok_or_else(|| anyhow::anyhow!("AppHandle 未绑定"))?;
        Ok(std::sync::Arc::new(crate::asr::local::LocalQwenAsr::new(
            app, engine,
        )))
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
        if let Some(app) = self.app() {
            let _ = app.emit_to("capsule", "insert:fallback", payload);
        }
    }

    pub(crate) fn clear_insert_fallback(&self) {
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
        let recorder = state.lock().take();
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
        };
        assert_ne!(
            initial,
            CapsuleLayoutState {
                work_height: 1080,
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
    fn fallback_card_owns_native_window_until_dismissed() {
        for state in [
            CapsuleState::Idle,
            CapsuleState::Recording,
            CapsuleState::Polishing,
            CapsuleState::Done,
        ] {
            assert_eq!(
                capsule_window_action(true, true, state),
                CapsuleWindowAction::PreserveFallbackCard
            );
        }
    }

    #[test]
    fn capsule_window_action_follows_visibility_without_fallback_card() {
        assert_eq!(
            capsule_window_action(false, true, CapsuleState::Recording),
            CapsuleWindowAction::ShowCapsule
        );
        assert_eq!(
            capsule_window_action(false, true, CapsuleState::Idle),
            CapsuleWindowAction::HideCapsule
        );
        assert_eq!(
            capsule_window_action(false, false, CapsuleState::Recording),
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
            (460.0, 52.0, 44.0, 8.0)
        );
        assert_eq!(
            capsule_transcript_rail_position(CapsuleStyle::Classic, true, true),
            Some(CapsuleTranscriptRailPosition {
                width: 460.0,
                height: 82.0,
                top_offset: 14.0,
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
            (22.0, 74.0)
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
                left: 132,
                right: 328,
                top: 104,
                bottom: 156,
            })
        );
        assert_eq!(
            capsule_control_hit_rect(CapsuleStyle::Classic, false, false, 460, 172),
            Some(CapsuleHitTestRect {
                left: 132,
                right: 328,
                top: 104,
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
