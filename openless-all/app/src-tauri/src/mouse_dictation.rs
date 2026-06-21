//! Global mouse-button dictation triggers (middle / side buttons).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{OnceLock, RwLock};

use crate::hold_source_tracker::TriggerSource;
use crate::hotkey::HotkeyEvent;

static ACTIVE_MOUSE: OnceLock<RwLock<Option<MouseMonitorState>>> = OnceLock::new();

struct MouseMonitorState {
    middle_enabled: bool,
    side_enabled: bool,
    tx: Sender<(HotkeyEvent, TriggerSource)>,
    middle_held: AtomicBool,
    side_held: AtomicBool,
}

pub struct MouseDictationConfig {
    pub middle_enabled: bool,
    pub side_enabled: bool,
}

pub struct MouseDictationMonitor;

impl MouseDictationMonitor {
    pub fn start(
        config: MouseDictationConfig,
        tx: Sender<(HotkeyEvent, TriggerSource)>,
    ) -> Result<Self, String> {
        if !config.middle_enabled && !config.side_enabled {
            return Err("mouse dictation disabled".into());
        }
        let slot = ACTIVE_MOUSE.get_or_init(|| RwLock::new(None));
        *slot.write().map_err(|e| e.to_string())? = Some(MouseMonitorState {
            middle_enabled: config.middle_enabled,
            side_enabled: config.side_enabled,
            tx,
            middle_held: AtomicBool::new(false),
            side_held: AtomicBool::new(false),
        });
        Ok(Self)
    }

    pub fn update_config(config: MouseDictationConfig) {
        if let Some(slot) = ACTIVE_MOUSE.get() {
            if let Ok(mut guard) = slot.write() {
                if let Some(state) = guard.as_mut() {
                    if state.middle_enabled && !config.middle_enabled {
                        release_held_if_held(state, &state.middle_held, TriggerSource::MouseMiddle);
                    }
                    if state.side_enabled && !config.side_enabled {
                        release_held_if_held(state, &state.side_held, TriggerSource::MouseSide);
                    }
                    state.middle_enabled = config.middle_enabled;
                    state.side_enabled = config.side_enabled;
                }
            }
        }
    }
}

impl Drop for MouseDictationMonitor {
    fn drop(&mut self) {
        if let Some(slot) = ACTIVE_MOUSE.get() {
            if let Ok(mut guard) = slot.write() {
                if let Some(state) = guard.as_mut() {
                    release_held_sources(state, true, true);
                }
                *guard = None;
            }
        }
    }
}

fn release_held_if_held(
    state: &MouseMonitorState,
    held: &AtomicBool,
    source: TriggerSource,
) {
    if held.swap(false, Ordering::SeqCst) {
        send_edge(state, HotkeyEvent::Released, source);
    }
}

fn release_held_sources(state: &MouseMonitorState, middle: bool, side: bool) {
    if middle {
        release_held_if_held(state, &state.middle_held, TriggerSource::MouseMiddle);
    }
    if side {
        release_held_if_held(state, &state.side_held, TriggerSource::MouseSide);
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

fn send_edge(state: &MouseMonitorState, evt: HotkeyEvent, source: TriggerSource) {
    if let Err(err) = state.tx.send((evt, source)) {
        log::warn!("[mouse-dictation] event send failed: {err}");
    }
}

pub fn handle_button(button: MouseButton, pressed: bool) {
    with_active(|state| {
        let (enabled, held, source) = match button {
            MouseButton::Middle => (
                state.middle_enabled,
                &state.middle_held,
                TriggerSource::MouseMiddle,
            ),
            MouseButton::Side => (
                state.side_enabled,
                &state.side_held,
                TriggerSource::MouseSide,
            ),
        };
        if !enabled {
            return;
        }
        if pressed {
            if !held.swap(true, Ordering::SeqCst) {
                send_edge(state, HotkeyEvent::Pressed, source);
            }
        } else if held.swap(false, Ordering::SeqCst) {
            send_edge(state, HotkeyEvent::Released, source);
        }
    });
}

#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Middle,
    Side,
}

#[cfg(target_os = "windows")]
pub mod platform {
    use super::*;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, WH_MOUSE_LL,
        WM_MBUTTONDOWN, WM_MBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
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

    pub fn install_hook() -> Result<(), String> {
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

    pub fn uninstall_hook() {
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
            match msg {
                WM_MBUTTONDOWN => handle_button(MouseButton::Middle, true),
                WM_MBUTTONUP => handle_button(MouseButton::Middle, false),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let hi = ((mouse.mouseData >> 16) & 0xFFFF) as u16;
                    if hi == XBUTTON1 as u16 || hi == XBUTTON2 as u16 {
                        let pressed = msg == WM_XBUTTONDOWN;
                        handle_button(MouseButton::Side, pressed);
                    }
                }
                _ => {}
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct MSLLHOOKSTRUCT {
        pt: windows::Win32::Foundation::POINT,
        mouseData: u32,
        flags: u32,
        time: u32,
        extraInfo: usize,
    }
}

#[cfg(target_os = "macos")]
pub mod platform {
    use super::*;

    pub fn dispatch_button_number(button_number: i64, pressed: bool) {
        // macOS CGEvent buttonNumber: 0=left, 1=right, 2=middle, 3/4=side buttons
        let button = match button_number {
            2 => Some(MouseButton::Middle),
            3 | 4 => Some(MouseButton::Side),
            _ => None,
        };
        if let Some(button) = button {
            handle_button(button, pressed);
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
                if let Some(state) = guard.take() {
                    release_held_sources(&state, true, true);
                }
            }
        }
    }

    #[test]
    fn disabling_held_middle_emits_release() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();

        let (tx, rx) = mpsc::channel();
        let _monitor = MouseDictationMonitor::start(
            MouseDictationConfig {
                middle_enabled: true,
                side_enabled: false,
            },
            tx,
        )
        .unwrap();

        handle_button(MouseButton::Middle, true);
        assert_eq!(
            rx.recv().unwrap(),
            (HotkeyEvent::Pressed, TriggerSource::MouseMiddle)
        );

        MouseDictationMonitor::update_config(MouseDictationConfig {
            middle_enabled: false,
            side_enabled: false,
        });
        assert_eq!(
            rx.recv().unwrap(),
            (HotkeyEvent::Released, TriggerSource::MouseMiddle)
        );

        clear_active_monitor();
    }

    #[test]
    fn dropping_monitor_emits_release_for_held_sources() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear_active_monitor();

        let (tx, rx) = mpsc::channel();
        let monitor = MouseDictationMonitor::start(
            MouseDictationConfig {
                middle_enabled: true,
                side_enabled: true,
            },
            tx,
        )
        .unwrap();

        handle_button(MouseButton::Middle, true);
        handle_button(MouseButton::Side, true);
        assert_eq!(
            rx.recv().unwrap(),
            (HotkeyEvent::Pressed, TriggerSource::MouseMiddle)
        );
        assert_eq!(
            rx.recv().unwrap(),
            (HotkeyEvent::Pressed, TriggerSource::MouseSide)
        );

        drop(monitor);

        let mut releases = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            releases.push(evt);
        }
        assert_eq!(releases.len(), 2);
        assert!(releases.contains(&(HotkeyEvent::Released, TriggerSource::MouseMiddle)));
        assert!(releases.contains(&(HotkeyEvent::Released, TriggerSource::MouseSide)));

        clear_active_monitor();
    }
}
