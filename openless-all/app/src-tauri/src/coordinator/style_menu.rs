//! Real hotkey provenance and the no-focus capsule picker bridge.
use super::*;
use crate::capsule_style_menu::{self as menu, Context, Event, Focus};

#[cfg(target_os = "windows")]
#[path = "style_menu/windows_focus.rs"]
mod windows_focus;
#[cfg(target_os = "windows")]
static WINDOWS_FOCUS: std::sync::OnceLock<Mutex<Option<windows_focus::Owner>>> = std::sync::OnceLock::new();

#[cfg(not(target_os = "windows"))]
fn foreground() -> Option<Focus> {
    #[cfg(target_os = "macos")]
    {
        let pid = crate::ask_window::foreground_pid()?;
        let window = crate::selection::macos_front_window_id(pid as i32)?;
        Some(Focus((u64::from(pid) << 32) | u64::from(window),
            crate::ask_window::active_space_epoch()))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    { None }
}

#[derive(Default)]
#[cfg(not(target_os = "windows"))]
struct FocusState {
    serial: u64,
    native: Option<Focus>,
    target: Option<crate::selection::SelectionInsertionTarget>,
}

#[cfg(not(target_os = "windows"))]
impl FocusState {
    fn current(&mut self) -> Option<(Focus, bool)> {
        let native = foreground()?;
        let window_changed = self.native != Some(native);
        let control_changed = !window_changed && self.target.as_ref()
            .is_some_and(|target| !crate::selection::capsule_menu_input_target_is_current(target));
        if window_changed || control_changed {
            self.serial = self.serial.checked_add(1).expect("style menu focus sequence exhausted");
            self.native = Some(native);
            self.target = None;
        }
        if self.target.is_none() {
            self.target = crate::selection::capture_capsule_menu_input_target();
        }
        // A verifiable window still binds mouse-only ownership when AX/UIA is
        // unavailable; it must not keep that owner across an app/window change.
        Some((Focus(self.serial, native.1), self.target.is_some()))
    }
}

pub(super) fn configure_style_menu(inner: &Arc<Inner>) {
    let source = Arc::downgrade(inner);
    let sink = Arc::downgrade(inner);
    let epoch = Arc::downgrade(inner);
    #[cfg(not(target_os = "windows"))]
    let focus = Mutex::new(FocusState::default());
    #[cfg(target_os = "windows")]
    let focus = windows_focus::Owner::start().map_err(|error|
        log::warn!("[capsule] focus owner unavailable: {error}")).ok();
    #[cfg(target_os = "windows")]
    { *WINDOWS_FOCUS.get_or_init(|| Mutex::new(None)).lock() = focus.clone(); }
    #[cfg(target_os = "windows")]
    let focus_revision = focus.clone();
    let source = Arc::new(move || {
        let inner = source.upgrade()?;
        let snapshot = inner.backend.snapshot();
        if !matches!(snapshot.dictation.phase,
            openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording) {
            return None;
        }
        if matches!(inner.backend.dictation_output_target(), Some(
            openless_core::DictationOutputTarget::AskComposer
                | openless_core::DictationOutputTarget::Qa
                | openless_core::DictationOutputTarget::VoiceEdit
        )) || inner.selection_polish_capsule_active.load(Ordering::SeqCst) {
            return None;
        }
        // Query native focus first; then read current Core readiness and the
        // capsule surface, so a slow AX/UIA query cannot replay Starting over
        // an owner that has already mounted in this same style epoch.
        #[cfg(not(target_os = "windows"))]
        let (focus, target_verified) = focus.lock().current()
            .map(|(focus, verified)| (Some(focus), verified)).unwrap_or((None, false));
        let (target, style_epoch, picker_enabled) = inner.backend.dictation_style_target_with_epoch()?;
        #[cfg(target_os = "windows")]
        let (focus, target_verified) = focus.as_ref()
            .and_then(|focus| focus.current(windows_focus::Scope { session: target.session_id, epoch: style_epoch }))
            .map(|(serial, process, verified)| (Some(Focus(serial, process)), verified))
            .unwrap_or((None, false));
        if snapshot.dictation.session_id != Some(target.session_id) { return None; }
        let surface_available = inner.host.style_menu_surface_available(target.session_id);
        let keyboard_available = cfg!(any(target_os = "macos", target_os = "windows"))
            && !inner.shortcut_recording_active.load(Ordering::SeqCst)
            && inner.hotkey.try_lock().is_some_and(|monitor| monitor.is_some())
            && surface_available;
        Some(Context {
            session_id: target.session_id, style_epoch, mutable: target.mutable && surface_available,
            skip_auto: target.skip_auto_style,
            picker_enabled,
            // Window/control identity is the insertion adapter's existing
            // native proof (AX on macOS, HWND/UIA on Windows). Missing permission
            // or an unverifiable input target yields mouse-only ownership.
            keyboard_available: keyboard_available && target_verified, focus,
            blocked_keys: menu::blocked_keys(&hotkey_runtime_target(&inner)),
        })
    });
    let sink = Arc::new(move |event| {
        sink.upgrade().is_some_and(|inner| inner.host.queue_style_menu_event(event))
    });
    let epoch = Arc::new(move || epoch.upgrade()
        .map(|inner| inner.backend.dictation_style_epoch()).unwrap_or(u64::MAX));
    #[cfg(not(target_os = "windows"))]
    menu::configure_fenced(source, sink, epoch);
    #[cfg(target_os = "windows")]
    {
        let observer = focus_revision.clone();
        menu::configure_focus_fenced(source, sink, epoch, Arc::new(move ||
            focus_revision.as_ref().map(|focus| focus.revision()).unwrap_or((u64::MAX, u64::MAX))));
        if let Some(owner) = observer { owner.set_observer(Arc::new(menu::refresh)); }
    }
}

/// Only the two native hotkey consumers call this. Programmatic style changes
/// and ordinary start options never pass through this provenance boundary.
pub(super) fn handle_skip_auto_style_hotkey(inner: &Arc<Inner>, session_id: openless_core::SessionId) {
    if let Ok((target, epoch)) = inner.backend.toggle_dictation_style_with_epoch(session_id) {
        #[cfg(target_os = "windows")]
        {
            let owner = WINDOWS_FOCUS.get().and_then(|slot| slot.lock().clone());
            let weak = Arc::downgrade(inner);
            if let Some(owner) = owner {
                let _ = owner.submit(windows_focus::Scope { session: session_id, epoch }, move || {
                    if let Some(inner) = weak.upgrade() {
                        if let Some(request) = menu::hotkey_toggled(&target, epoch) {
                            let _ = inner.host.queue_style_menu_event(Event::Request(request));
                        }
                    }
                });
            }
        }
        #[cfg(not(target_os = "windows"))]
        if let Some(request) = menu::hotkey_toggled(&target, epoch) {
            let _ = inner.host.queue_style_menu_event(Event::Request(request));
        }
    }
}
