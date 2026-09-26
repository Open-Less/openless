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

pub struct MouseDictationMonitor;

impl MouseDictationMonitor {
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
            *slot
                .write()
                .map_err(|e| ComboHotkeyError::RegisterFailed(e.to_string()))? = Some(state);

            #[cfg(target_os = "windows")]
            platform::ensure_hook_thread().map_err(ComboHotkeyError::RegisterFailed)?;

            Ok(Self)
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
        release_held(existing);
        let next = state_from_binding(binding, existing.tx.clone())?;
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
        modifiers.push(normalize_generic_modifier(raw).ok_or_else(|| {
            ComboHotkeyError::UnsupportedModifier(raw.clone())
        })?);
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
    #[cfg(target_os = "windows")]
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
        for tag in required {
            let down = match tag.as_str() {
                "ctrl" => ctrl,
                "alt" => alt,
                "shift" => shift,
                "super" => meta,
                _ => false,
            };
            if !down {
                return false;
            }
        }
        // Reject unexpected modifiers so Ctrl+Mouse4 does not fire for bare Mouse4 binds.
        let unexpected = [
            ("ctrl", ctrl),
            ("alt", alt),
            ("shift", shift),
            ("super", meta),
        ];
        for (tag, down) in unexpected {
            if down && !required.iter().any(|r| r == tag) {
                return false;
            }
        }
        true
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Tests synthesize edges without real modifier state; require empty modifiers.
        required.is_empty()
    }
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
        if pressed {
            if !modifiers_match(&state.modifiers) {
                return;
            }
            if !state.held.swap(true, Ordering::SeqCst) {
                send_edge(state, ComboHotkeyEvent::Pressed { at: Instant::now() });
            }
        } else if state.held.swap(false, Ordering::SeqCst) {
            send_edge(state, ComboHotkeyEvent::Released { at: Instant::now() });
        }
    });
}

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, WH_MOUSE_LL,
        WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
    };

    static MOUSE_HOOK: OnceLock<Mutex<Option<isize>>> = OnceLock::new();
    static HOOK_THREAD_STARTED: OnceLock<()> = OnceLock::new();

    pub fn ensure_hook_thread() -> Result<(), String> {
        if HOOK_THREAD_STARTED.get().is_some() {
            return Ok(());
        }
        std::thread::Builder::new()
            .name("openless-mouse-hook".into())
            .spawn(|| {
                if let Err(err) = install_hook() {
                    log::error!("[mouse-dictation] hook install failed: {err}");
                    return;
                }
                let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                unsafe {
                    while windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                        &mut msg,
                        None,
                        0,
                        0,
                    )
                    .0
                        > 0
                    {
                        let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                        let _ = windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                    }
                }
                uninstall_hook();
            })
            .map_err(|e| format!("spawn mouse hook thread: {e}"))?;
        let _ = HOOK_THREAD_STARTED.set(());
        Ok(())
    }

    fn install_hook() -> Result<(), String> {
        let slot = MOUSE_HOOK.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            return Ok(());
        }
        unsafe {
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0)
                .map_err(|e| format!("mouse hook install failed: {e}"))?;
            *guard = Some(hook.0 as isize);
        }
        Ok(())
    }

    fn uninstall_hook() {
        if let Some(slot) = MOUSE_HOOK.get() {
            if let Ok(mut guard) = slot.lock() {
                if let Some(hook) = guard.take() {
                    unsafe {
                        let _ = UnhookWindowsHookEx(HHOOK(hook as *mut core::ffi::c_void));
                    }
                }
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

    #[repr(C)]
    #[derive(Copy, Clone)]
    #[allow(non_snake_case)]
    struct MSLLHOOKSTRUCT {
        pt: windows::Win32::Foundation::POINT,
        mouseData: u32,
        flags: u32,
        time: u32,
        extraInfo: usize,
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
    fn press_release_emits_combo_edges() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();
        let (tx, rx) = mpsc::channel();
        let _monitor = MouseDictationMonitor::start(mouse4_binding(), tx).unwrap();

        handle_button("Mouse4", true);
        assert!(matches!(rx.recv().unwrap(), ComboHotkeyEvent::Pressed { .. }));
        handle_button("Mouse4", false);
        assert!(matches!(rx.recv().unwrap(), ComboHotkeyEvent::Released { .. }));

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
        assert!(matches!(rx.recv().unwrap(), ComboHotkeyEvent::Pressed { .. }));
        drop(monitor);
        assert!(matches!(rx.recv().unwrap(), ComboHotkeyEvent::Released { .. }));

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
        assert!(matches!(rx.recv().unwrap(), ComboHotkeyEvent::Pressed { .. }));

        clear_active_monitor();
    }
}
