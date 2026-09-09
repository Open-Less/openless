//! Process-owned capture of the Mac Dictation key. Ordinary F5 is never captured.
use crate::combo_hotkey::ComboHotkeyEvent;
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub const PRIMARY: &str = "MacDictationKey";
const MODIFIERS: u64 = (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 23);
fn effective_flags(flags: u64, fn_down: bool) -> u64 {
    if fn_down {
        flags
    } else {
        flags & !(1 << 23)
    }
}
#[derive(Default)]
struct KeyState {
    held: bool,
}
#[derive(Debug, PartialEq)]
enum Edge {
    Pass,
    Repeat,
    Press,
    Release,
}
impl KeyState {
    fn event(&mut self, key: i64, down: bool, repeat: bool, flags: u64) -> Edge {
        if key != 176 {
            return Edge::Pass;
        }
        if self.held {
            if down {
                return Edge::Repeat;
            }
            self.held = false;
            return Edge::Release;
        }
        if down && !repeat && flags & MODIFIERS == 0 {
            self.held = true;
            return Edge::Press;
        }
        Edge::Pass
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void,
        context: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enabled: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFMachPortInvalidate(port: *mut c_void);
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(runloop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRemoveSource(runloop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: bool) -> i32;
    fn CFRelease(value: *const c_void);
    static kCFRunLoopDefaultMode: *const c_void;
}
struct Context {
    state: KeyState,
    tx: Sender<ComboHotkeyEvent>,
    stop: Arc<AtomicBool>,
    tap: *mut c_void,
}

fn recover_disabled_tap(ctx: &mut Context, enable: impl FnOnce(*mut c_void)) {
    if ctx.state.held {
        let _ = ctx
            .tx
            .send(ComboHotkeyEvent::Released { at: Instant::now() });
    }
    ctx.state.held = false;
    enable(ctx.tap);
}

extern "C" fn callback(
    _: *mut c_void,
    kind: u32,
    event: *mut c_void,
    data: *mut c_void,
) -> *mut c_void {
    if data.is_null() {
        return event;
    }
    let ctx = unsafe { &mut *(data as *mut Context) };
    if kind == 0xffff_fffe || kind == 0xffff_ffff {
        // Fail open for a held key, then restore the selected trigger in place.
        recover_disabled_tap(ctx, |tap| unsafe { CGEventTapEnable(tap, true) });
        log::warn!("[dictation-key] native interception was disabled and has been re-enabled");
        return event;
    }
    if event.is_null() || !matches!(kind, 10 | 11) {
        return event;
    }
    let key = unsafe { CGEventGetIntegerValueField(event, 9) };
    if key != 176 {
        return event;
    }
    // Never start a new capture in a password field / Secure Event Input session.
    if !ctx.state.held && crate::unicode_keystroke::is_secure_input_enabled() {
        return event;
    }
    let (flags, repeat) = unsafe {
        (
            CGEventGetFlags(event),
            CGEventGetIntegerValueField(event, 8) != 0,
        )
    };
    let fn_down = unsafe { CGEventSourceKeyState(0, 63) };
    let edge = ctx
        .state
        .event(key, kind == 10, repeat, effective_flags(flags, fn_down));
    let message = match edge {
        Edge::Press => Some(ComboHotkeyEvent::Pressed { at: Instant::now() }),
        Edge::Release => Some(ComboHotkeyEvent::Released { at: Instant::now() }),
        _ => None,
    };
    if let Some(message) = message {
        if ctx.tx.send(message).is_err() {
            ctx.stop.store(true, Ordering::SeqCst);
            return event;
        }
    }
    if edge == Edge::Pass {
        event
    } else {
        std::ptr::null_mut()
    }
}

pub struct Monitor {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Monitor {
    pub fn start(tx: Sender<ComboHotkeyEvent>) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("openless-dictation-key".into())
            .spawn(move || unsafe {
                let mut context = Context {
                    state: KeyState::default(),
                    tx,
                    stop: thread_stop.clone(),
                    tap: std::ptr::null_mut(),
                };
                let tap = CGEventTapCreate(
                    0,
                    0,
                    0,
                    (1 << 10) | (1 << 11),
                    callback,
                    &mut context as *mut _ as *mut c_void,
                );
                if tap.is_null() {
                    let _ = ready_tx.send(Err("macDictationKeyPermission".to_string()));
                    return;
                }
                context.tap = tap;
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    CFMachPortInvalidate(tap);
                    CFRelease(tap);
                    let _ = ready_tx.send(Err("macDictationKeyUnavailable".to_string()));
                    return;
                }
                let runloop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(runloop, source, kCFRunLoopDefaultMode);
                CGEventTapEnable(tap, true);
                if ready_tx.send(Ok(())).is_ok() {
                    while !thread_stop.load(Ordering::SeqCst) {
                        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, false);
                    }
                }
                CGEventTapEnable(tap, false);
                CFRunLoopRemoveSource(runloop, source, kCFRunLoopDefaultMode);
                CFMachPortInvalidate(tap);
                CFRelease(source);
                CFRelease(tap);
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            result => {
                stop.store(true, Ordering::SeqCst);
                let _ = thread.join();
                Err(match result {
                    Ok(Err(e)) => e,
                    _ => "macDictationKeyUnavailable".into(),
                })
            }
        }
    }
    pub fn active(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn function_layer_flag_is_distinct_from_physical_fn() {
        assert_eq!(effective_flags(1 << 23, false), 0);
        assert_eq!(effective_flags(1 << 23, true), 1 << 23);
        assert_eq!(effective_flags((1 << 19) | (1 << 23), false), 1 << 19);
    }
    #[test]
    fn ordinary_and_modified_keys_pass_through() {
        let mut s = KeyState::default();
        for key in [96, 90, 59, 58, 63] {
            assert_eq!(s.event(key, true, false, 0), Edge::Pass);
        }
        for bit in [17, 18, 19, 20, 23] {
            assert_eq!(s.event(176, true, false, 1 << bit), Edge::Pass);
        }
        assert_eq!(s.event(176, false, false, 0), Edge::Pass);
    }
    #[test]
    fn repeat_and_release_pairing() {
        let mut s = KeyState::default();
        assert_eq!(s.event(176, true, true, 0), Edge::Pass);
        assert_eq!(s.event(176, true, false, 0), Edge::Press);
        assert_eq!(s.event(176, true, true, 0), Edge::Repeat);
        assert_eq!(s.event(176, false, false, MODIFIERS), Edge::Release);
        assert_eq!(s.event(176, false, false, 0), Edge::Pass);
        assert_eq!(s.event(176, true, false, 0), Edge::Press);
    }
    #[test]
    fn disabled_tap_releases_held_key_and_is_reenabled() {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut context = Context {
            state: KeyState { held: true },
            tx,
            stop: stop.clone(),
            tap: std::ptr::null_mut(),
        };
        let mut reenabled = false;

        recover_disabled_tap(&mut context, |_| reenabled = true);

        assert!(reenabled);
        assert!(!context.state.held);
        assert!(!stop.load(Ordering::SeqCst));
        assert!(matches!(
            rx.try_recv(),
            Ok(ComboHotkeyEvent::Released { .. })
        ));
    }
}
