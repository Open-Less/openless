//! Tauri-only host operations used by the compatibility coordinator.
//!
//! The shared backend never sees this module. It owns the late-bound
//! [`tauri::AppHandle`] and keeps window, main-thread and managed-state access
//! out of the coordinator's business paths.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::types::{CapsulePayload, CapsuleState, CapsuleStyle};

static CAPSULE_SUPPRESSED_BY_TOGGLE_LOGGED: AtomicBool = AtomicBool::new(false);
static CAPSULE_FIRST_SHOW_LOGGED: AtomicBool = AtomicBool::new(false);
static CAPSULE_NO_ACTIVATE_FALLBACK_WARNED: AtomicBool = AtomicBool::new(false);
static CAPSULE_WINDOW_MISSING_LOGGED: AtomicBool = AtomicBool::new(false);

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

    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
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

#[cfg(target_os = "linux")]
fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    true
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
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

struct CapsuleWindowState {
    layout: Mutex<Option<CapsuleLayoutState>>,
    cursor_passthrough: AtomicBool,
    style: AtomicU8,
    fallback_card_visible: AtomicBool,
    fallback_presentation_id: AtomicU64,
    deferred_payload: Mutex<Option<CapsulePayload>>,
    last_payload: Mutex<Option<CapsulePayload>>,
    layout_watch_active: AtomicBool,
    layout_watch_epoch: AtomicU64,
}

impl Default for CapsuleWindowState {
    fn default() -> Self {
        Self {
            layout: Mutex::new(None),
            cursor_passthrough: AtomicBool::new(true),
            style: AtomicU8::new(0),
            fallback_card_visible: AtomicBool::new(false),
            fallback_presentation_id: AtomicU64::new(0),
            deferred_payload: Mutex::new(None),
            last_payload: Mutex::new(None),
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
            window.set_ignore_cursor_events(passthrough)?;
            self.state
                .cursor_passthrough
                .store(passthrough, Ordering::SeqCst);
        }
        Ok(())
    }

    pub(crate) fn invalidate_layout(&self) {
        *self.state.layout.lock() = None;
    }

    pub(crate) fn hide(&self) -> tauri::Result<()> {
        self.stop_layout_watch();
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
            crate::position_capsule_bottom_center_with_style(
                &window,
                translation,
                self.state.cached_style(),
            )?;
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
        if self.state.layout.lock().as_ref() == Some(&next) {
            return;
        }
        if crate::position_capsule_bottom_center_with_style(window, translation_active, style)
            .is_ok()
        {
            *self.state.layout.lock() = Some(next);
        }
    }

    pub(crate) fn show_for_recording(&self, reassert_spaces: bool) {
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
                    let payload = capsule.state.last_payload.lock().clone();
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
        let payload = self.state.last_payload.lock().clone();
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
        *self.state.last_payload.lock() = Some(payload.clone());
        let Some(window) = self.window() else {
            return;
        };
        let fallback_card_active = self.state.defer_if_fallback_active(payload);

        #[cfg(target_os = "linux")]
        {
            let _ = (
                window,
                payload,
                show_capsule,
                style,
                fallback_card_active,
                reassert_spaces,
            );
            return;
        }

        #[cfg(not(target_os = "linux"))]
        {
            let action = capsule_window_action(fallback_card_active, show_capsule, payload.state);
            if action == CapsuleWindowAction::PreserveFallbackCard {
                log::debug!(
                    "[capsule] native window update deferred: insert fallback card owns the window"
                );
                return;
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
                    let _ = window.hide();
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

    #[cfg(any(target_os = "macos", target_os = "linux"))]
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

    pub(crate) fn show_less_computer(&self) {
        if let Some(app) = self.app() {
            crate::show_less_computer_window(&app);
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
}
