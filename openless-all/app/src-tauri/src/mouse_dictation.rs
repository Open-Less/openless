//! Global Mouse4 / Mouse5 dictation triggers via a dedicated mouse hook.
//!
//! Keyboard combos use `global-hotkey` / `RegisterHotKey`, which cannot register
//! mouse buttons. This module mirrors the MacDictationKey / side-aware pattern:
//! install a native listener and emit [`ComboHotkeyEvent`] into the shared
//! combo bridge so Hold / Toggle edge semantics stay identical.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{OnceLock, RwLock};
use std::time::Instant;

use crate::combo_hotkey::{ComboHotkeyError, ComboHotkeyEvent};
use crate::shortcut_binding::binding_requires_mouse_hook;
use crate::types::ShortcutBinding;

static ACTIVE_MOUSE: OnceLock<RwLock<Option<MouseMonitorState>>> = OnceLock::new();

struct MouseMonitorState {
    primary: String,
    /// Normalized generic modifier tags: ctrl / alt / shift / super.
    modifiers: Vec<String>,
    tx: Sender<ComboHotkeyEvent>,
    held: AtomicBool,
}

impl MouseMonitorState {
    fn handle_edge(&self, pressed: bool, modifiers_match: bool) {
        if pressed {
            if modifiers_match && !self.held.swap(true, Ordering::SeqCst) {
                send_edge(self, ComboHotkeyEvent::Pressed { at: Instant::now() });
            }
        } else {
            // Release belongs to the accepted press even if modifiers changed.
            release_held(self);
        }
    }
}

pub struct MouseDictationMonitor {
    #[cfg(all(target_os = "windows", not(test)))]
    _hook: platform::HookThread,
}

impl MouseDictationMonitor {
    /// A bridge failure drops the installed hook before returning to the transaction.
    pub fn start_with_bridge(
        binding: ShortcutBinding,
        bridge: impl FnOnce(std::sync::mpsc::Receiver<ComboHotkeyEvent>) -> Result<(), String>,
    ) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let monitor = Self::start(binding, tx).map_err(|error| error.to_string())?;
        bridge(rx)?;
        Ok(monitor)
    }

    pub fn is_running(&self) -> bool {
        #[cfg(all(target_os = "windows", not(test)))]
        {
            self._hook.is_running()
        }
        #[cfg(any(not(target_os = "windows"), test))]
        {
            true
        }
    }

    pub fn start(
        binding: ShortcutBinding,
        tx: Sender<ComboHotkeyEvent>,
    ) -> Result<Self, ComboHotkeyError> {
        #[cfg(all(not(target_os = "windows"), not(test)))]
        {
            let _ = (binding, tx);
            return Err(ComboHotkeyError::RegisterFailed(
                "Mouse4/Mouse5 global hotkeys are currently supported on Windows only".into(),
            ));
        }

        #[cfg(any(target_os = "windows", test))]
        {
            let state = state_from_binding(binding, tx)?;
            let slot = ACTIVE_MOUSE.get_or_init(|| RwLock::new(None));
            let mut guard = slot
                .write()
                .map_err(|e| ComboHotkeyError::RegisterFailed(e.to_string()))?;
            if guard.is_some() {
                return Err(ComboHotkeyError::RegisterFailed(
                    "mouse monitor already active".into(),
                ));
            }
            #[cfg(all(target_os = "windows", not(test)))]
            let hook = platform::HookThread::start().map_err(ComboHotkeyError::RegisterFailed)?;
            *guard = Some(state);
            Ok(Self {
                #[cfg(all(target_os = "windows", not(test)))]
                _hook: hook,
            })
        }
    }

    pub fn update_binding(&self, binding: ShortcutBinding) -> Result<(), ComboHotkeyError> {
        let slot = ACTIVE_MOUSE
            .get()
            .ok_or_else(|| ComboHotkeyError::RegisterFailed("mouse monitor inactive".into()))?;
        let mut guard = slot
            .write()
            .map_err(|e| ComboHotkeyError::RegisterFailed(e.to_string()))?;
        let Some(existing) = guard.as_mut() else {
            return Err(ComboHotkeyError::RegisterFailed(
                "mouse monitor inactive".into(),
            ));
        };
        let next = state_from_binding(binding, existing.tx.clone())?;
        release_held(existing);
        existing.primary = next.primary;
        existing.modifiers = next.modifiers;
        Ok(())
    }
}

impl Drop for MouseDictationMonitor {
    fn drop(&mut self) {
        if let Some(slot) = ACTIVE_MOUSE.get() {
            if let Ok(mut guard) = slot.write() {
                if let Some(state) = guard.as_mut() {
                    release_held(state);
                }
                *guard = None;
            }
        }
    }
}

fn state_from_binding(
    binding: ShortcutBinding,
    tx: Sender<ComboHotkeyEvent>,
) -> Result<MouseMonitorState, ComboHotkeyError> {
    if !binding_requires_mouse_hook(&binding) {
        return Err(ComboHotkeyError::UnsupportedKey(binding.primary));
    }
    let primary = normalize_mouse_primary(&binding.primary)
        .ok_or_else(|| ComboHotkeyError::UnsupportedKey(binding.primary.clone()))?;
    let mut modifiers = Vec::new();
    for raw in &binding.modifiers {
        modifiers.push(
            normalize_generic_modifier(raw)
                .ok_or_else(|| ComboHotkeyError::UnsupportedModifier(raw.clone()))?,
        );
    }
    modifiers.sort();
    modifiers.dedup();
    Ok(MouseMonitorState {
        primary,
        modifiers,
        tx,
        held: AtomicBool::new(false),
    })
}

fn normalize_mouse_primary(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "MOUSE4" => Some("Mouse4".into()),
        "MOUSE5" => Some("Mouse5".into()),
        _ => None,
    }
}

fn normalize_generic_modifier(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some("ctrl".into()),
        "alt" | "option" | "opt" => Some("alt".into()),
        "shift" => Some("shift".into()),
        "cmd" | "command" if cfg!(target_os = "windows") => Some("ctrl".into()),
        "cmd" | "command" | "super" | "meta" | "win" => Some("super".into()),
        _ => None,
    }
}

fn release_held(state: &MouseMonitorState) {
    if state.held.swap(false, Ordering::SeqCst) {
        send_edge(state, ComboHotkeyEvent::Released { at: Instant::now() });
    }
}

fn with_active<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&MouseMonitorState) -> R,
{
    let slot = ACTIVE_MOUSE.get()?;
    let guard = slot.read().ok()?;
    guard.as_ref().map(f)
}

fn send_edge(state: &MouseMonitorState, evt: ComboHotkeyEvent) {
    if let Err(err) = state.tx.send(evt) {
        log::warn!("[mouse-dictation] event send failed: {err}");
    }
}

fn modifiers_match(required: &[String]) -> bool {
    #[cfg(all(target_os = "windows", not(test)))]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        // Match hotkey.rs: use raw VK codes rather than VIRTUAL_KEY helpers.
        const VK_SHIFT: i32 = 0x10;
        const VK_CONTROL: i32 = 0x11;
        const VK_MENU: i32 = 0x12;
        const VK_LWIN: i32 = 0x5B;
        const VK_RWIN: i32 = 0x5C;
        let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL) } < 0;
        let alt = unsafe { GetAsyncKeyState(VK_MENU) } < 0;
        let shift = unsafe { GetAsyncKeyState(VK_SHIFT) } < 0;
        let meta =
            unsafe { GetAsyncKeyState(VK_LWIN) } < 0 || unsafe { GetAsyncKeyState(VK_RWIN) } < 0;
        match_modifier_states(required, [ctrl, alt, shift, meta])
    }
    #[cfg(any(not(target_os = "windows"), test))]
    {
        // Tests synthesize edges without real modifier state; require empty modifiers.
        match_modifier_states(required, [false; 4])
    }
}

fn match_modifier_states(required: &[String], down: [bool; 4]) -> bool {
    // Exact matching also rejects unexpected modifiers for bare mouse bindings.
    ["ctrl", "alt", "shift", "super"]
        .into_iter()
        .zip(down)
        .all(|(tag, pressed)| pressed == required.iter().any(|required| required == tag))
}

/// Dispatch a Mouse4 / Mouse5 edge into the active monitor (if any).
pub fn handle_button(primary: &str, pressed: bool) {
    let Some(normalized) = normalize_mouse_primary(primary) else {
        return;
    };
    with_active(|state| {
        if state.primary != normalized {
            return;
        }
        state.handle_edge(pressed, modifiers_match(&state.modifiers));
    });
}

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        HC_ACTION, HHOOK, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_QUIT, WM_XBUTTONDOWN,
        WM_XBUTTONUP, XBUTTON1, XBUTTON2,
    };

    pub struct HookThread {
        thread_id: u32,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl HookThread {
        pub fn is_running(&self) -> bool {
            self.thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
        }

        pub fn start() -> Result<Self, String> {
            Self::start_with_install(|| unsafe {
                SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0)
                    .map_err(|error| format!("mouse hook install failed: {error}"))
            })
        }

        fn start_with_install(
            install: impl FnOnce() -> Result<HHOOK, String> + Send + 'static,
        ) -> Result<Self, String> {
            let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
            let thread = std::thread::Builder::new()
                .name("openless-mouse-hook".into())
                .spawn(move || unsafe {
                    let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                    // Create the queue before publishing its thread id to Drop.
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let hook = match install() {
                        Ok(hook) => hook,
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                            return;
                        }
                    };
                    if ready_tx.send(Ok(GetCurrentThreadId())).is_ok() {
                        while windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                            &mut msg, None, 0, 0,
                        )
                        .0 > 0
                        {
                            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                            let _ = windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                        }
                    }
                    let _ = UnhookWindowsHookEx(hook);
                })
                .map_err(|e| format!("spawn mouse hook thread: {e}"))?;
            // A timeout drops the rendezvous receiver; the worker then unhooks itself.
            let thread_id = ready_rx
                .recv_timeout(std::time::Duration::from_secs(3))
                .map_err(|error| format!("mouse hook startup failed: {error}"))??;
            Ok(Self {
                thread_id,
                thread: Some(thread),
            })
        }
    }

    impl Drop for HookThread {
        fn drop(&mut self) {
            let Some(thread) = self.thread.take() else {
                return;
            };
            if !thread.is_finished() {
                if let Err(error) =
                    unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }
                {
                    log::error!("[mouse-dictation] stop hook failed: {error}");
                    return;
                }
            }
            // MouseDictationMonitor released the callback's state lock before this join.
            if thread.join().is_err() {
                log::error!("[mouse-dictation] hook thread panicked");
            }
        }
    }

    unsafe extern "system" fn low_level_mouse_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 && lparam.0 != 0 {
            let msg = wparam.0 as u32;
            let mouse = std::ptr::read(lparam.0 as *const MSLLHOOKSTRUCT);
            if matches!(msg, WM_XBUTTONDOWN | WM_XBUTTONUP) {
                let hi = ((mouse.mouseData >> 16) & 0xFFFF) as u16;
                let primary = if hi == XBUTTON1 as u16 {
                    Some("Mouse4")
                } else if hi == XBUTTON2 as u16 {
                    Some("Mouse5")
                } else {
                    None
                };
                if let Some(primary) = primary {
                    handle_button(primary, msg == WM_XBUTTONDOWN);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn installation_failure_is_reported_and_can_retry() {
            for attempt in 0..2 {
                let error =
                    HookThread::start_with_install(move || Err(format!("install {attempt}")))
                        .err()
                        .expect("installation must fail");
                assert_eq!(error, format!("install {attempt}"));
            }
        }

        #[test]
        #[ignore = "installs real Windows hooks; run explicitly on an interactive desktop"]
        fn native_hook_can_stop_and_restart() {
            for _ in 0..3 {
                drop(HookThread::start().expect("native hook startup"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Mutex};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn clear_active_monitor() {
        if let Some(slot) = ACTIVE_MOUSE.get() {
            if let Ok(mut guard) = slot.write() {
                if let Some(state) = guard.as_mut() {
                    release_held(state);
                }
                *guard = None;
            }
        }
    }

    fn mouse4_binding() -> ShortcutBinding {
        ShortcutBinding {
            primary: "Mouse4".into(),
            modifiers: vec![],
        }
    }

    #[test]
    fn failed_bridge_drops_monitor_and_allows_retry() {
        let _lock = TEST_LOCK.lock().unwrap();
        let error = MouseDictationMonitor::start_with_bridge(mouse4_binding(), |_| {
            Err("spawn bridge failed".into())
        })
        .err()
        .expect("bridge failure must propagate");
        assert_eq!(error, "spawn bridge failed");
        let mut receiver = None;
        let monitor = MouseDictationMonitor::start_with_bridge(mouse4_binding(), |rx| {
            receiver = Some(rx);
            Ok(())
        })
        .unwrap();
        handle_button("Mouse4", true);
        let receiver = receiver.unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ComboHotkeyEvent::Pressed { .. }
        ));
        drop(monitor);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ComboHotkeyEvent::Released { .. }
        ));
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn modifiers_match_exactly_and_use_keyboard_aliases() {
        let required = vec!["ctrl".into()];
        assert!(match_modifier_states(
            &required,
            [true, false, false, false]
        ));
        assert!(!match_modifier_states(&required, [false; 4]));
        assert!(!match_modifier_states(
            &required,
            [true, false, true, false]
        ));
        assert!(!match_modifier_states(&[], [true, false, false, false]));
        assert_eq!(
            normalize_generic_modifier("cmd").as_deref(),
            Some(if cfg!(target_os = "windows") {
                "ctrl"
            } else {
                "super"
            })
        );
    }

    #[test]
    fn edges_deduplicate_and_release_after_modifiers_change() {
        let (tx, rx) = mpsc::channel();
        let state = state_from_binding(mouse4_binding(), tx).unwrap();
        state.handle_edge(true, false);
        assert!(rx.try_recv().is_err());
        state.handle_edge(true, true);
        state.handle_edge(true, true);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ComboHotkeyEvent::Pressed { .. }
        ));
        assert!(rx.try_recv().is_err());
        state.handle_edge(false, false);
        state.handle_edge(false, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ComboHotkeyEvent::Released { .. }
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn invalid_update_preserves_held_binding_and_valid_update_releases_once() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, rx) = mpsc::channel();
        let monitor = MouseDictationMonitor::start(mouse4_binding(), tx).unwrap();
        handle_button("Mouse4", true);
        let _ = rx.try_recv().unwrap();
        assert!(monitor
            .update_binding(ShortcutBinding {
                primary: "Mouse5".into(),
                modifiers: vec!["ctrl-left".into()],
            })
            .is_err());
        assert!(rx.try_recv().is_err());
        monitor
            .update_binding(ShortcutBinding {
                primary: "Mouse5".into(),
                modifiers: vec![],
            })
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            ComboHotkeyEvent::Released { .. }
        ));
        handle_button("Mouse4", false);
        assert!(rx.try_recv().is_err());
        drop(monitor);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn invalid_start_does_not_reserve_singleton() {
        let _lock = TEST_LOCK.lock().unwrap();
        let (tx, _rx) = mpsc::channel();
        assert!(MouseDictationMonitor::start(
            ShortcutBinding {
                primary: "Mouse3".into(),
                modifiers: vec![],
            },
            tx.clone()
        )
        .is_err());
        for _ in 0..3 {
            drop(MouseDictationMonitor::start(mouse4_binding(), tx.clone()).unwrap());
        }
    }

    #[test]
    fn press_release_emits_combo_edges() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();
        let (tx, rx) = mpsc::channel();
        let _monitor = MouseDictationMonitor::start(mouse4_binding(), tx).unwrap();

        handle_button("Mouse4", true);
        assert!(matches!(
            rx.recv().unwrap(),
            ComboHotkeyEvent::Pressed { .. }
        ));
        handle_button("Mouse4", false);
        assert!(matches!(
            rx.recv().unwrap(),
            ComboHotkeyEvent::Released { .. }
        ));

        clear_active_monitor();
    }

    #[test]
    fn wrong_button_ignored() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();
        let (tx, rx) = mpsc::channel();
        let _monitor = MouseDictationMonitor::start(mouse4_binding(), tx).unwrap();

        handle_button("Mouse5", true);
        assert!(rx.try_recv().is_err());

        clear_active_monitor();
    }

    #[test]
    fn dropping_monitor_emits_release_when_held() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();
        let (tx, rx) = mpsc::channel();
        let monitor = MouseDictationMonitor::start(mouse4_binding(), tx).unwrap();

        handle_button("Mouse4", true);
        assert!(matches!(
            rx.recv().unwrap(),
            ComboHotkeyEvent::Pressed { .. }
        ));
        drop(monitor);
        assert!(matches!(
            rx.recv().unwrap(),
            ComboHotkeyEvent::Released { .. }
        ));

        clear_active_monitor();
    }

    #[test]
    fn update_binding_switches_primary() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();
        let (tx, rx) = mpsc::channel();
        let monitor = MouseDictationMonitor::start(mouse4_binding(), tx).unwrap();

        monitor
            .update_binding(ShortcutBinding {
                primary: "Mouse5".into(),
                modifiers: vec![],
            })
            .unwrap();
        handle_button("Mouse4", true);
        assert!(rx.try_recv().is_err());
        handle_button("Mouse5", true);
        assert!(matches!(
            rx.recv().unwrap(),
            ComboHotkeyEvent::Pressed { .. }
        ));

        clear_active_monitor();
    }
}
