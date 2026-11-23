use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use super::{
    esc_exclusive, install_error, reset_shared_held_state, send_cancel_or_log,
    send_combo_abort_or_log, send_or_log, start_listener_thread, update_shared_binding,
    update_shared_modifier_shortcuts, HotkeyAdapter, HotkeyCombinedEdge, HotkeyEvent, Shared,
    StartupTx,
};
use crate::types::{
    HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyMode, HotkeyTrigger,
};

pub fn start_adapter(
    binding: HotkeyBinding,
    tx: Sender<HotkeyEvent>,
    cancel_tx: Sender<()>,
    combo_tx: Sender<HotkeyCombinedEdge>,
) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
    let listener = start_listener_thread(
        binding,
        tx,
        cancel_tx,
        combo_tx,
        "openless-hotkey-mac-event-tap",
        "hotkey hook 启动超时",
        run_listen_loop,
    )?;
    Ok(Box::new(MacHotkeyAdapter {
        shared: listener.shared,
        handles: listener.startup,
    }))
}

/// Refs needed to stop the Mac CFRunLoop / CGEventTap from outside the listener
/// thread. Filled in by `run_listen_loop` once the tap is created and the runloop
/// reference is captured; consumed by `MacHotkeyAdapter::shutdown` when the
/// monitor is dropped (so a binding swap or app shutdown doesn't leak the
/// listener thread + tap). Cf. audit 3.1.1.
struct MacShutdownHandles {
    tap: std::sync::Mutex<Option<CfMachPortRef>>,
    runloop: std::sync::Mutex<Option<CfRunLoopRef>>,
}

// SAFETY: CfMachPortRef / CfRunLoopRef are CoreFoundation handles; the only
// operations we perform on them across threads are CGEventTapEnable and
// CFRunLoopStop, both of which Apple documents as safe to call from any
// thread.
unsafe impl Send for MacShutdownHandles {}
unsafe impl Sync for MacShutdownHandles {}

struct MacHotkeyAdapter {
    shared: Arc<Shared>,
    handles: Arc<MacShutdownHandles>,
}

impl HotkeyAdapter for MacHotkeyAdapter {
    fn kind(&self) -> HotkeyAdapterKind {
        HotkeyAdapterKind::MacEventTap
    }

    fn update_binding(&self, binding: HotkeyBinding) {
        update_shared_binding(&self.shared, binding);
    }

    fn update_modifier_shortcuts(
        &self,
        qa_trigger: Option<HotkeyTrigger>,
        selection_polish_trigger: Option<HotkeyTrigger>,
        translation_trigger: Option<HotkeyTrigger>,
    ) {
        update_shared_modifier_shortcuts(
            &self.shared,
            qa_trigger,
            selection_polish_trigger,
            translation_trigger,
        );
    }

    fn reset_held_state(&self) {
        reset_shared_held_state(&self.shared);
    }
    fn set_recording_active(&self, active: bool) {
        self.shared.recording_active.store(active, Ordering::SeqCst);
        if !active {
            self.shared.recording_fn_held.store(false, Ordering::SeqCst);
        }
    }

    fn shutdown(&self) {
        // Order: disable the tap first so the OS stops dispatching events to us, then stop the
        // runloop so the listener thread returns from CFRunLoopRun() and exits. take() makes
        // this idempotent.
        let tap = self.handles.tap.lock().ok().and_then(|mut g| g.take());
        if let Some(tap) = tap {
            unsafe { CGEventTapEnable(tap, false) };
        }
        let runloop = self.handles.runloop.lock().ok().and_then(|mut g| g.take());
        if let Some(rl) = runloop {
            unsafe { CFRunLoopStop(rl) };
        }
    }
}

// ── Raw CG/CF FFI ──────────────────────────────────────────────────────

#[repr(C)]
struct OpaqueCgEvent(c_void);
// SAFETY: NSEvent's public signature uses an opaque __CGEvent pointer.
unsafe impl objc2::encode::RefEncode for OpaqueCgEvent {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("__CGEvent", &[]));
}
type CgEventRef = *mut OpaqueCgEvent;

#[repr(C)]
struct OpaqueCfMachPort(c_void);
type CfMachPortRef = *mut OpaqueCfMachPort;

#[repr(C)]
struct OpaqueCfRunLoop(c_void);
type CfRunLoopRef = *mut OpaqueCfRunLoop;

#[repr(C)]
struct OpaqueCfRunLoopSource(c_void);
type CfRunLoopSourceRef = *mut OpaqueCfRunLoopSource;

type CfStringRef = *const c_void;
type CfAllocatorRef = *const c_void;

type CgEventMask = u64;
type CgEventType = u32;
type CgEventTapLocation = u32;
type CgEventTapPlacement = u32;
type CgEventTapOptions = u32;
type CgEventField = u32;
type CgEventFlags = u64;

const SESSION_EVENT_TAP: CgEventTapLocation = 1;
const HEAD_INSERT: CgEventTapPlacement = 0;
const TAP_OPTION_DEFAULT: CgEventTapOptions = 0;

const KEY_DOWN: CgEventType = 10;
const KEY_UP: CgEventType = 11;
const FLAGS_CHANGED: CgEventType = 12;
/// Brightness / volume / keyboard-backlight and similar macOS function-layer
/// actions are delivered as system-defined events instead of KEY_DOWN.
const SYSTEM_DEFINED: CgEventType = 14;
const TAP_DISABLED_BY_TIMEOUT: CgEventType = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: CgEventType = 0xFFFF_FFFF;

const KEYBOARD_EVENT_KEYCODE: CgEventField = 9;

const FLAG_MASK_SHIFT: CgEventFlags = 0x0002_0000;
const FLAG_MASK_CONTROL: CgEventFlags = 0x0004_0000;
const FLAG_MASK_ALTERNATE: CgEventFlags = 0x0008_0000;
const FLAG_MASK_COMMAND: CgEventFlags = 0x0010_0000;
const FLAG_MASK_SECONDARY_FN: CgEventFlags = 0x0080_0000;

const ESC_KEYCODE: i64 = 53;
// IOKit hidsystem/IOLLEvent.h + ev_keymap.h. systemDefined subtype 8 carries
// auxiliary control keys in data1's high 16 bits; 0..=23 are the scanned
// brightness / volume / media / illumination family. Globe/Menu is 25 and
// must stay excluded, otherwise a plain Fn tap could cancel itself.
const NX_SUBTYPE_AUX_CONTROL_BUTTONS: i16 = 8;
const NX_NUM_SCANNED_SPECIAL_KEYS: u16 = 24;
/// Under Auto / Toggle, Fn is a dual-purpose key: a short press triggers dictation, a
/// clearly long press is left to the macOS function layer. Matches Auto mode's existing
/// 350ms short/long press boundary.
const FN_TAP_MAX_DURATION: std::time::Duration = std::time::Duration::from_millis(350);

type CgEventTapCallBack = extern "C" fn(
    proxy: *mut c_void,
    event_type: CgEventType,
    event: CgEventRef,
    user_info: *mut c_void,
) -> CgEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: CgEventTapLocation,
        place: CgEventTapPlacement,
        options: CgEventTapOptions,
        events_of_interest: CgEventMask,
        callback: CgEventTapCallBack,
        user_info: *mut c_void,
    ) -> CfMachPortRef;
    fn CGEventTapEnable(tap: CfMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CgEventRef, field: CgEventField) -> i64;
    fn CGEventGetFlags(event: CgEventRef) -> CgEventFlags;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: CfAllocatorRef,
        port: CfMachPortRef,
        order: isize,
    ) -> CfRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CfRunLoopRef;
    fn CFRunLoopAddSource(rl: CfRunLoopRef, source: CfRunLoopSourceRef, mode: CfStringRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CfRunLoopRef);
    static kCFRunLoopCommonModes: CfStringRef;
}

struct CallbackContext {
    shared: Arc<Shared>,
    tx: Sender<HotkeyEvent>,
    /// Esc-only channel, see module docs — must not share tx's serial bridge.
    cancel_tx: Sender<()>,
    /// Combo-abort-only channel, see module docs — must not share tx's serial bridge.
    combo_tx: Sender<HotkeyCombinedEdge>,
    /// Under Auto / Toggle, Fn dispatch is deferred until release: short press dispatches,
    /// long press or function-layer combos are dropped.
    fn_pressed_at: parking_lot::Mutex<Option<std::time::Instant>>,
    /// (tap, runloop) refs shared with MacHotkeyAdapter. Tap re-enable on
    /// TAP_DISABLED_BY_TIMEOUT goes through handles.tap; adapter shutdown uses both locks.
    handles: Arc<MacShutdownHandles>,
}

unsafe impl Send for CallbackContext {}
unsafe impl Sync for CallbackContext {}

fn run_listen_loop(
    shared: Arc<Shared>,
    tx: Sender<HotkeyEvent>,
    cancel_tx: Sender<()>,
    combo_tx: Sender<HotkeyCombinedEdge>,
    status_tx: StartupTx<Arc<MacShutdownHandles>>,
) {
    let mask: CgEventMask = (1u64 << FLAGS_CHANGED)
        | (1u64 << KEY_DOWN)
        | (1u64 << KEY_UP)
        | (1u64 << SYSTEM_DEFINED);
    let handles = Arc::new(MacShutdownHandles {
        tap: std::sync::Mutex::new(None),
        runloop: std::sync::Mutex::new(None),
    });
    let context = Box::into_raw(Box::new(CallbackContext {
        shared,
        tx,
        cancel_tx,
        combo_tx,
        fn_pressed_at: parking_lot::Mutex::new(None),
        handles: Arc::clone(&handles),
    }));

    unsafe {
        let tap = CGEventTapCreate(
            SESSION_EVENT_TAP,
            HEAD_INSERT,
            TAP_OPTION_DEFAULT,
            mask,
            tap_callback,
            context as *mut c_void,
        );
        if tap.is_null() {
            log::warn!(
                "[hotkey] CGEventTapCreate 失败 — Accessibility 权限未授予。Coordinator 会重试。"
            );
            let _ = Box::from_raw(context);
            let _ = status_tx.send(Err(install_error(
                "accessibility_denied",
                "hotkey hook 安装失败（辅助功能权限未授予）",
            )));
            return;
        }
        *handles.tap.lock().unwrap() = Some(tap);

        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        let runloop = CFRunLoopGetCurrent();
        *handles.runloop.lock().unwrap() = Some(runloop);
        CFRunLoopAddSource(runloop, source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);

        log::info!("[hotkey] CGEventTap 已启动");
        let _ = status_tx.send(Ok(handles));
        // CFRunLoopRun blocks until CFRunLoopStop is called (triggered by
        // MacHotkeyAdapter::shutdown). After it returns, the listener thread drops the context
        // and exits naturally.
        CFRunLoopRun();
        let _ = Box::from_raw(context);
    }
}

extern "C" fn tap_callback(
    _proxy: *mut c_void,
    event_type: CgEventType,
    event: CgEventRef,
    user_info: *mut c_void,
) -> CgEventRef {
    if user_info.is_null() {
        return event;
    }
    let ctx = unsafe { &*(user_info as *const CallbackContext) };

    match event_type {
        TAP_DISABLED_BY_TIMEOUT | TAP_DISABLED_BY_USER_INPUT => {
            reset_companion_held();
            if let Some(tap) = *ctx.handles.tap.lock().unwrap() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }
        FLAGS_CHANGED => handle_flags_changed(ctx, event),
        KEY_DOWN => {
            let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
            if keycode == ESC_KEYCODE && super::capsule_menu_open() { return event; }
            if !ctx.shared.recording_active.load(Ordering::SeqCst)
                && crate::input::side_combo::platform::dispatch_keycode(
                    keycode,
                    false,
                    unsafe { CGEventGetFlags(event) },
                    true,
                )
            {
                crate::input::side_combo::handle_companion_key_down();
                note_companion_key_down(ctx);
                return std::ptr::null_mut();
            }
            handle_key_down(ctx, event);
            // While a session is active, consume Esc exclusively: returning null deletes the
            // event (active tap) so the host app never sees it, avoiding double effect between
            // "cancel session" and the host's Esc semantics. See esc_exclusive docs.
            if keycode == ESC_KEYCODE && esc_exclusive() {
                return std::ptr::null_mut();
            }
        }
        KEY_UP => {
            let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
            if crate::input::side_combo::platform::dispatch_keycode(
                keycode,
                false,
                unsafe { CGEventGetFlags(event) },
                false,
            ) {
                return std::ptr::null_mut();
            }
        }
        // Fn+brightness / volume / keyboard backlight and similar do not produce KEY_DOWN but
        // systemDefined. While the Fn trigger is held, treat these as function-layer combos —
        // dictation must not start at the same time.
        SYSTEM_DEFINED => {
            handle_system_defined_event(ctx, || system_defined_event_payload(event));
        }
        _ => {}
    }
    event
}

fn handle_flags_changed(ctx: &CallbackContext, event: CgEventRef) {
    let flags = unsafe { CGEventGetFlags(event) };

    // Recording mode: the browser never delivers Fn keydown to the web layer (the system
    // modifier is hidden), so ShortcutRecorder cannot capture it. Here the CGEventTap detects
    // the Fn press edge (keycode 63 + kCGEventFlagMaskSecondaryFn), reports
    // `FnRecordingPressed`, and the upper layer forwards it to the frontend recorder to commit
    // an Fn binding.
    if ctx.shared.recording_active.load(Ordering::SeqCst) {
        let fn_active = (flags & FLAG_MASK_SECONDARY_FN) != 0;
        let fn_was_held = ctx.shared.recording_fn_held.load(Ordering::SeqCst);
        if fn_active && !fn_was_held {
            ctx.shared.recording_fn_held.store(true, Ordering::SeqCst);
            log::info!("[hotkey] 录制态检测到 Fn↓ → 上报 FnRecordingPressed");
            send_or_log(&ctx.tx, HotkeyEvent::FnRecordingPressed);
        } else if !fn_active && fn_was_held {
            ctx.shared.recording_fn_held.store(false, Ordering::SeqCst);
            log::info!("[hotkey] 录制态 Fn↑（松开）");
        }
        // Recording mode only hands Fn to ShortcutRecorder. Do not also update the dictation
        // trigger latch, otherwise after the frontend saves Fn and exits recording mode, the
        // same physical release is mistaken for a dictation short press.
        return;
    }

    // Shift is the translation-mode modifier — independent of the trigger keycode check; a
    // Shift press takes effect at any time.
    let shift_active = (flags & FLAG_MASK_SHIFT) != 0;
    let shift_was_held = ctx.shared.translation_modifier_held.load(Ordering::SeqCst);
    if shift_active && !shift_was_held {
        ctx.shared
            .translation_modifier_held
            .store(true, Ordering::SeqCst);
        send_or_log(&ctx.tx, HotkeyEvent::TranslationModifierPressed);
    } else if !shift_active && shift_was_held {
        ctx.shared
            .translation_modifier_held
            .store(false, Ordering::SeqCst);
    }

    let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
    crate::input::side_combo::platform::dispatch_keycode(keycode, true, flags, false);
    handle_optional_modifier_trigger(
        ctx,
        keycode,
        flags,
        *ctx.shared.qa_trigger.read(),
        &ctx.shared.qa_trigger_held,
        HotkeyEvent::QaShortcutPressed,
        None,
    );
    handle_optional_modifier_trigger(
        ctx,
        keycode,
        flags,
        *ctx.shared.selection_polish_trigger.read(),
        &ctx.shared.selection_polish_trigger_held,
        HotkeyEvent::SelectionPolishShortcutPressed,
        Some(HotkeyEvent::SelectionPolishShortcutReleased),
    );
    handle_optional_modifier_trigger(
        ctx,
        keycode,
        flags,
        *ctx.shared.translation_trigger.read(),
        &ctx.shared.translation_trigger_held,
        HotkeyEvent::TranslationModifierPressed,
        None,
    );

    handle_dictation_trigger_flags_changed(ctx, keycode, flags, std::time::Instant::now());
}

fn fn_uses_tap_only_semantics(trigger: HotkeyTrigger, mode: HotkeyMode) -> bool {
    trigger == HotkeyTrigger::Fn && matches!(mode, HotkeyMode::Auto | HotkeyMode::Toggle)
}

/// Handles flagsChanged edges for a modifier-only dictation trigger.
///
/// Under Auto / Toggle, Fn cannot use the "press = trigger" rule of other modifiers: it is
/// also a macOS function-layer modifier. Record the press here, and only after release —
/// confirmed as a short press with no function keys — emit the Pressed / Released pair. Long
/// presses and Fn+F1/volume/brightness system actions never start dictation, so the capsule
/// never flashes.
fn handle_dictation_trigger_flags_changed(
    ctx: &CallbackContext,
    keycode: i64,
    flags: CgEventFlags,
    now: std::time::Instant,
) {
    let (trigger, mode) = {
        let binding = ctx.shared.binding.read();
        (binding.trigger, binding.mode)
    };
    if trigger == HotkeyTrigger::Custom || keycode != trigger_to_keycode(trigger) {
        return;
    }

    let is_active = (flags & trigger_to_flag_mask(trigger)) != 0;
    let was_held = ctx.shared.trigger_held.load(Ordering::SeqCst);
    let tap_only_fn = fn_uses_tap_only_semantics(trigger, mode);

    if is_active && !was_held {
        ctx.shared.trigger_held.store(true, Ordering::SeqCst);
        let press_id = super::next_press_id();
        ctx.shared
            .trigger_press_id
            .store(press_id, Ordering::SeqCst);
        ctx.shared.trigger_companion_seen.store(0, Ordering::SeqCst);
        if tap_only_fn {
            *ctx.fn_pressed_at.lock() = Some(now);
            log::debug!("[hotkey] Fn↓ 等待松开后判定短按 / 功能层操作");
        } else {
            send_or_log(&ctx.tx, HotkeyEvent::Pressed { at: now, press_id });
        }
        return;
    }

    if !is_active && was_held {
        ctx.shared.trigger_held.store(false, Ordering::SeqCst);
        let press_id = ctx.shared.trigger_press_id.swap(0, Ordering::SeqCst);
        if !tap_only_fn {
            send_or_log(&ctx.tx, HotkeyEvent::Released { at: now, press_id,
                combined: ctx.shared.trigger_companion_seen.load(Ordering::SeqCst) == press_id });
            return;
        }

        let pressed_at = ctx.fn_pressed_at.lock().take();
        let companion_seen = press_id != 0
            && ctx.shared.trigger_companion_seen.load(Ordering::SeqCst) == press_id;
        let held_for = pressed_at.map(|at| now.saturating_duration_since(at));
        if !companion_seen && held_for.is_some_and(|duration| duration < FN_TAP_MAX_DURATION) {
            let pressed_at = pressed_at.expect("checked above");
            send_or_log(
                &ctx.tx,
                HotkeyEvent::Pressed {
                    at: pressed_at,
                    press_id,
                },
            );
            send_or_log(&ctx.tx, HotkeyEvent::Released { at: now, press_id,
                combined: ctx.shared.trigger_companion_seen.load(Ordering::SeqCst) == press_id });
        } else {
            log::info!(
                "[hotkey] Fn 操作未触发听写（held_ms={}, companion_seen={companion_seen}）",
                held_for.map(|duration| duration.as_millis()).unwrap_or(0)
            );
        }
    } else if !is_active && trigger == HotkeyTrigger::Fn {
        // Recording mode / binding updates reset the held latch deliberately; a physical
        // release still needs to clear the stale timestamp.
        ctx.fn_pressed_at.lock().take();
    }
}

fn handle_optional_modifier_trigger(
    ctx: &CallbackContext,
    keycode: i64,
    flags: CgEventFlags,
    trigger: Option<HotkeyTrigger>,
    held: &std::sync::atomic::AtomicBool,
    press_event: HotkeyEvent,
    release_event: Option<HotkeyEvent>,
) {
    let Some(trigger) = trigger else {
        return;
    };
    if trigger == HotkeyTrigger::Custom || keycode != trigger_to_keycode(trigger) {
        return;
    }
    let active = (flags & trigger_to_flag_mask(trigger)) != 0;
    let was_held = held.load(Ordering::SeqCst);
    if active && !was_held {
        held.store(true, Ordering::SeqCst);
        if matches!(press_event, HotkeyEvent::SelectionPolishShortcutPressed) {
            prefetch_selection();
        }
        send_or_log(&ctx.tx, press_event);
    } else if !active && was_held {
        held.store(false, Ordering::SeqCst);
        if let Some(release_event) = release_event {
            send_or_log(&ctx.tx, release_event);
        }
    }
}

fn handle_key_down(ctx: &CallbackContext, event: CgEventRef) {
    let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
    crate::input::side_combo::handle_companion_key_down();
    if keycode == ESC_KEYCODE {
        note_companion_key_down(ctx);
        send_cancel_or_log(&ctx.cancel_tx);
        return;
    }
    note_companion_key_down(ctx);
}

fn handle_system_defined_event(
    ctx: &CallbackContext,
    payload: impl FnOnce() -> Option<(i16, isize)>,
) {
    // System controls only affect dictation while a configured Fn press is held.
    if !ctx.shared.trigger_held.load(Ordering::SeqCst)
        || ctx.shared.binding.read().trigger != HotkeyTrigger::Fn
    {
        return;
    }
    if let Some((subtype, data1)) = payload() {
        note_fn_function_layer_event(ctx, subtype, data1);
    }
}

fn system_defined_event_payload(event: CgEventRef) -> Option<(i16, isize)> {
    if event.is_null() {
        return None;
    }
    if unsafe { libc::pthread_main_np() } != 0 {
        return system_defined_event_payload_on_main(event);
    }
    // NSEvent conversion can enter HIToolbox's main-thread-only input-source
    // machinery. Complete classification before the listener handles Fn release.
    let address = event as usize;
    let mut payload = None;
    dispatch2::DispatchQueue::main().exec_sync(|| {
        payload = system_defined_event_payload_on_main(address as CgEventRef);
    });
    payload
}

fn system_defined_event_payload_on_main(event: CgEventRef) -> Option<(i16, isize)> {
    use objc2::msg_send;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyClass, AnyObject};

    debug_assert_ne!(unsafe { libc::pthread_main_np() }, 0);
    autoreleasepool(|_| {
        let cls = AnyClass::get("NSEvent")?;
        let ns_event: *mut AnyObject = unsafe { msg_send![cls, eventWithCGEvent: event] };
        if ns_event.is_null() {
            return None;
        }
        let event_type: usize = unsafe { msg_send![ns_event, type] };
        if event_type != SYSTEM_DEFINED as usize {
            return None;
        }
        let subtype: i16 = unsafe { msg_send![ns_event, subtype] };
        let data1: isize = unsafe { msg_send![ns_event, data1] };
        Some((subtype, data1))
    })
}

fn is_auxiliary_function_key_event(subtype: i16, data1: isize) -> bool {
    if subtype != NX_SUBTYPE_AUX_CONTROL_BUTTONS {
        return false;
    }
    let key_type = ((data1 as u64 >> 16) & 0xffff) as u16;
    key_type < NX_NUM_SCANNED_SPECIAL_KEYS
}

fn note_fn_function_layer_event(ctx: &CallbackContext, subtype: i16, data1: isize) {
    if is_auxiliary_function_key_event(subtype, data1)
        && ctx.shared.binding.read().trigger == HotkeyTrigger::Fn
    {
        note_companion_key_down(ctx);
    }
}

/// A normal key pressed while the trigger is held = the user is typing a combo
/// (Option+letter/digit, Option+Tab, ...), not speaking — send one abort on the combo channel
/// so the upper layer cancels this press.
///
/// Uses `combo_tx` instead of `tx`: the abort must take effect the same frame the key goes
/// down (capsule disappears immediately), while the bridge on the `tx` side is most likely
/// still inside this very press's `begin_session`. See module docs.
///
/// macOS modifiers go through FLAGS_CHANGED and never enter KEY_DOWN, so overlaying Shift
/// (translation modifier) or Cmd is not mistaken for a combo; only real character/function
/// keys count. OS auto-repeat and "several keys pressed while holding the trigger" are
/// collapsed into one abort by the companion latch.
fn note_companion_key_down(ctx: &CallbackContext) {
    if !ctx.shared.trigger_held.load(Ordering::SeqCst) {
        return;
    }
    let press_id = ctx.shared.trigger_press_id.load(Ordering::SeqCst);
    if press_id == 0
        || ctx
            .shared
            .trigger_companion_seen
            .compare_exchange(0, press_id, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return;
    }
    let deferred_fn = {
        let binding = ctx.shared.binding.read();
        fn_uses_tap_only_semantics(binding.trigger, binding.mode)
    };
    if deferred_fn {
        // Under Auto / Toggle, Fn has not dispatched Pressed to the coordinator yet; just
        // record the companion — the whole press is discarded on release. Sending an abort
        // here would leave a pending id that never sees a Pressed.
        log::info!("[hotkey] Fn 功能层组合按下 —— 本次短按候选作废");
        return;
    }
    log::info!("[hotkey] 触发键与其他键组合按下 —— 撤销本次触发");
    send_combo_abort_or_log(&ctx.combo_tx, press_id);
}

fn trigger_to_keycode(trigger: HotkeyTrigger) -> i64 {
    match trigger {
        HotkeyTrigger::LeftControl => 59,
        HotkeyTrigger::RightControl => 62,
        HotkeyTrigger::LeftOption => 58,
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => 61,
        HotkeyTrigger::RightCommand => 54,
        HotkeyTrigger::LeftCommand => 55,
        HotkeyTrigger::LeftShift => 56,
        HotkeyTrigger::RightShift => 60,
        HotkeyTrigger::Fn => 63,
        HotkeyTrigger::MediaPlayPause => 0,
        HotkeyTrigger::Custom => unreachable!("custom combo hotkeys use ComboHotkeyMonitor"),
    }
}

fn trigger_to_flag_mask(trigger: HotkeyTrigger) -> CgEventFlags {
    match trigger {
        HotkeyTrigger::LeftControl | HotkeyTrigger::RightControl => FLAG_MASK_CONTROL,
        HotkeyTrigger::LeftCommand | HotkeyTrigger::RightCommand => FLAG_MASK_COMMAND,
        HotkeyTrigger::LeftShift | HotkeyTrigger::RightShift => FLAG_MASK_SHIFT,
        HotkeyTrigger::LeftOption | HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => {
            FLAG_MASK_ALTERNATE
        }
        HotkeyTrigger::Fn => FLAG_MASK_SECONDARY_FN,
        HotkeyTrigger::MediaPlayPause => 0,
        HotkeyTrigger::Custom => unreachable!("custom combo hotkeys use ComboHotkeyMonitor"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::RwLock;
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::mpsc;

    fn shared(trigger: HotkeyTrigger) -> Arc<Shared> {
        Arc::new(Shared {
            binding: RwLock::new(HotkeyBinding {
                trigger,
                mode: crate::types::HotkeyMode::Toggle,
                keys: None,
            }),
            trigger_held: AtomicBool::new(false),
            trigger_press_id: AtomicU64::new(0),
            trigger_companion_seen: AtomicU64::new(0),
            qa_trigger: RwLock::new(None),
            qa_trigger_held: AtomicBool::new(false),
            selection_polish_trigger: RwLock::new(None),
            selection_polish_trigger_held: AtomicBool::new(false),
            translation_trigger: RwLock::new(None),
            translation_trigger_held: AtomicBool::new(false),
            translation_modifier_held: AtomicBool::new(false),
            recording_active: AtomicBool::new(false),
            recording_fn_held: AtomicBool::new(false),
        })
    }

    fn callback_context_with_combo(
        shared: Arc<Shared>,
    ) -> (
        CallbackContext,
        mpsc::Receiver<HotkeyEvent>,
        mpsc::Receiver<HotkeyCombinedEdge>,
    ) {
        let (tx, rx) = mpsc::channel();
        let (cancel_tx, _cancel_rx) = mpsc::channel();
        let (combo_tx, combo_rx) = mpsc::channel();
        (
            CallbackContext {
                shared,
                tx,
                cancel_tx,
                combo_tx,
                fn_pressed_at: parking_lot::Mutex::new(None),
                handles: Arc::new(MacShutdownHandles {
                    tap: std::sync::Mutex::new(None),
                    runloop: std::sync::Mutex::new(None),
                }),
            },
            rx,
            combo_rx,
        )
    }

    /// Test cases that don't care about combo aborts: the abort channel's receiver drops
    /// them in place (these cases never send on it).
    fn callback_context(shared: Arc<Shared>) -> (CallbackContext, mpsc::Receiver<HotkeyEvent>) {
        let (ctx, rx, _combo_rx) = callback_context_with_combo(shared);
        (ctx, rx)
    }

    fn drain_combo(rx: &mpsc::Receiver<HotkeyCombinedEdge>) -> usize {
        rx.try_iter().count()
    }

    fn drain(rx: &mpsc::Receiver<HotkeyEvent>) -> Vec<HotkeyEvent> {
        rx.try_iter().collect()
    }

    fn edge_names(events: Vec<HotkeyEvent>) -> Vec<&'static str> {
        events
            .into_iter()
            .filter_map(|event| match event {
                HotkeyEvent::Pressed { .. } => Some("pressed"),
                HotkeyEvent::Released { .. } => Some("released"),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn mac_optional_modifier_edges_are_deduped_from_mock_flags() {
        let shared = shared(HotkeyTrigger::RightControl);
        let (ctx, rx) = callback_context(Arc::clone(&shared));

        handle_optional_modifier_trigger(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::RightCommand),
            trigger_to_flag_mask(HotkeyTrigger::RightCommand),
            Some(HotkeyTrigger::RightCommand),
            &shared.qa_trigger_held,
            HotkeyEvent::QaShortcutPressed,
            None,
        );
        handle_optional_modifier_trigger(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::RightCommand),
            trigger_to_flag_mask(HotkeyTrigger::RightCommand),
            Some(HotkeyTrigger::RightCommand),
            &shared.qa_trigger_held,
            HotkeyEvent::QaShortcutPressed,
            None,
        );
        handle_optional_modifier_trigger(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::RightCommand),
            0,
            Some(HotkeyTrigger::RightCommand),
            &shared.qa_trigger_held,
            HotkeyEvent::QaShortcutPressed,
            None,
        );
        handle_optional_modifier_trigger(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::RightCommand),
            trigger_to_flag_mask(HotkeyTrigger::RightCommand),
            Some(HotkeyTrigger::RightCommand),
            &shared.qa_trigger_held,
            HotkeyEvent::QaShortcutPressed,
            None,
        );

        assert_eq!(
            drain(&rx),
            vec![
                HotkeyEvent::QaShortcutPressed,
                HotkeyEvent::QaShortcutPressed,
            ]
        );
    }

    // Combos like Option+any letter/digit key: a normal key press while held aborts only
    // once, and the trigger must actually be held.
    #[test]
    fn mac_companion_key_down_aborts_trigger_once_per_hold() {
        let shared = shared(HotkeyTrigger::LeftOption);
        let (ctx, rx, combo_rx) = callback_context_with_combo(Arc::clone(&shared));

        // Normal typing without the trigger held: unrelated to dictation, no event sent.
        note_companion_key_down(&ctx);
        assert_eq!(drain_combo(&combo_rx), 0);

        shared.trigger_press_id.store(1, Ordering::SeqCst);
        shared.trigger_held.store(true, Ordering::SeqCst);
        // OS auto-repeat / multiple keys pressed while holding the trigger abort only once.
        note_companion_key_down(&ctx);
        note_companion_key_down(&ctx);
        assert_eq!(drain_combo(&combo_rx), 1);

        // The next Pressed edge resets the latch (in handle_flags_changed), so only the next
        // combo can abort again — otherwise a second combo would be treated as dictation.
        shared.trigger_companion_seen.store(0, Ordering::SeqCst);
        note_companion_key_down(&ctx);
        assert_eq!(drain_combo(&combo_rx), 1);

        // The abort never touches the serial Pressed/Released channel — it is stuck in
        // begin_session at this moment.
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn mac_fn_auto_short_tap_dispatches_only_after_release() {
        let shared = shared(HotkeyTrigger::Fn);
        shared.binding.write().mode = HotkeyMode::Auto;
        let (ctx, rx, combo_rx) = callback_context_with_combo(Arc::clone(&shared));
        let pressed_at = std::time::Instant::now();

        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            FLAG_MASK_SECONDARY_FN,
            pressed_at,
        );
        assert!(drain(&rx).is_empty(), "Fn down must stay a tap candidate");

        let released_at = pressed_at + std::time::Duration::from_millis(120);
        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            0,
            released_at,
        );

        let events = drain(&rx);
        assert!(matches!(events.as_slice(), (
            [HotkeyEvent::Pressed { press_id: left, .. }, HotkeyEvent::Released { press_id: right, .. }]
        ) if *left != 0 && left == right));
        assert_eq!(drain_combo(&combo_rx), 0);
    }

    #[test]
    fn mac_fn_auto_long_hold_is_reserved_for_system_functions() {
        let shared = shared(HotkeyTrigger::Fn);
        shared.binding.write().mode = HotkeyMode::Auto;
        let (ctx, rx, combo_rx) = callback_context_with_combo(Arc::clone(&shared));
        let pressed_at = std::time::Instant::now();

        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            FLAG_MASK_SECONDARY_FN,
            pressed_at,
        );
        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            0,
            pressed_at + FN_TAP_MAX_DURATION,
        );

        assert!(drain(&rx).is_empty());
        assert_eq!(drain_combo(&combo_rx), 0);
    }

    #[test]
    fn mac_idle_input_source_switch_does_not_decode_appkit_events() {
        for trigger in [HotkeyTrigger::Fn, HotkeyTrigger::RightOption] {
            let shared = shared(trigger);
            let (ctx, rx) = callback_context(shared);
            handle_system_defined_event(&ctx, || {
                panic!("idle input-source switching must not convert CGEvent to NSEvent");
            });
            assert!(drain(&rx).is_empty());
        }
    }

    #[test]
    fn mac_system_controls_do_not_decode_appkit_during_non_fn_holds() {
        let shared = shared(HotkeyTrigger::RightOption);
        shared.trigger_held.store(true, Ordering::SeqCst);
        let (ctx, rx) = callback_context(shared);
        handle_system_defined_event(&ctx, || {
            panic!("non-Fn dictation must not decode unrelated system controls");
        });
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn mac_fn_function_layer_event_suppresses_tap_without_pending_abort() {
        let shared = shared(HotkeyTrigger::Fn);
        shared.binding.write().mode = HotkeyMode::Toggle;
        let (ctx, rx, combo_rx) = callback_context_with_combo(Arc::clone(&shared));
        let pressed_at = std::time::Instant::now();

        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            FLAG_MASK_SECONDARY_FN,
            pressed_at,
        );
        // subtype/data1 encoding for brightness-up (NX_KEYTYPE_BRIGHTNESS_UP=2).
        handle_system_defined_event(&ctx, || {
            Some((NX_SUBTYPE_AUX_CONTROL_BUTTONS, (2_i64 << 16) as isize))
        });
        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            0,
            pressed_at + std::time::Duration::from_millis(80),
        );

        assert!(drain(&rx).is_empty());
        assert_eq!(drain_combo(&combo_rx), 0);
    }

    #[test]
    fn mac_system_defined_event_does_not_abort_non_fn_dictation() {
        let shared = shared(HotkeyTrigger::LeftOption);
        let (ctx, rx, combo_rx) = callback_context_with_combo(Arc::clone(&shared));
        shared.trigger_press_id.store(7, Ordering::SeqCst);
        shared.trigger_held.store(true, Ordering::SeqCst);

        note_fn_function_layer_event(
            &ctx,
            NX_SUBTYPE_AUX_CONTROL_BUTTONS,
            (2_i64 << 16) as isize,
        );

        assert_eq!(shared.trigger_companion_seen.load(Ordering::SeqCst), 0);
        assert!(drain(&rx).is_empty());
        assert_eq!(drain_combo(&combo_rx), 0);
    }

    #[test]
    fn mac_globe_system_event_is_not_mistaken_for_a_function_layer_key() {
        assert!(!is_auxiliary_function_key_event(
            NX_SUBTYPE_AUX_CONTROL_BUTTONS,
            (25_i64 << 16) as isize,
        ));
        assert!(!is_auxiliary_function_key_event(0, (2_i64 << 16) as isize));
    }

    #[test]
    fn mac_fn_hold_mode_keeps_press_to_talk_semantics() {
        let shared = shared(HotkeyTrigger::Fn);
        shared.binding.write().mode = HotkeyMode::Hold;
        let (ctx, rx) = callback_context(Arc::clone(&shared));
        let pressed_at = std::time::Instant::now();

        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            FLAG_MASK_SECONDARY_FN,
            pressed_at,
        );
        assert_eq!(edge_names(drain(&rx)), vec!["pressed"]);

        handle_dictation_trigger_flags_changed(
            &ctx,
            trigger_to_keycode(HotkeyTrigger::Fn),
            0,
            pressed_at + std::time::Duration::from_secs(1),
        );
        assert_eq!(edge_names(drain(&rx)), vec!["released"]);
    }
}
