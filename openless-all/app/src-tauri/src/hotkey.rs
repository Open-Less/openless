//! 全局热键监听：发送按下 / 抬起 / 取消三类边沿事件。
//!
//! - macOS：原生 CGEventTap（core-foundation + core-graphics FFI），与 Swift
//!   `OpenLessHotkey/HotkeyMonitor.swift` 同源。
//! - Windows：原生 `WH_KEYBOARD_LL` low-level keyboard hook，保留 modifier-only
//!   trigger（如右 Control / 右 Alt）的真实语义。
//!
//! Only raw edges with generation and monotonic timestamps are produced; openless-core interprets
//! their semantics.
//!
//! Esc (cancel) and combo abort (a normal key pressed while the trigger is held) **must not** share
//! the `HotkeyEvent` channel; they use a separate `Sender<HotkeyCombinedEdge>`. The
//! Pressed/Released bridge thread runs serial block_on (fixing the latch races in #468/#475), so a
//! Pressed/Released event synchronously completes `begin_session` (mic open + ASR handshake) or the
//! whole transcription + polish flow. If cancel/abort shared that queue, they would only run after
//! the flow finishes, appearing hundreds of ms late. The dedicated channel + consumer thread
//! handles cancel/abort immediately (`cancel_session` / `handle_trigger_combined` are pure
//! synchronous fast paths: set flags + release resources, no await).

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use crate::types::HotkeyTrigger;
use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed {
        at: Instant,
        press_id: u64,
    },
    Released {
        at: Instant,
        press_id: u64,
    },
    // Combo abort is not in this enum: it goes through the separate `combo_abort` channel to avoid
    // being queued behind the Pressed → begin_session synchronous mic-open flow (see module docs).
    /// Shift (or a future config-selected modifier) press edge. Can occur at any time during
    /// recording; the upper layer switches to the translation output pipeline. See issue #4.
    TranslationModifierPressed,
    QaShortcutPressed,
    SelectionPolishShortcutPressed,
    SelectionPolishShortcutReleased,
    /// Fn pressed while recording mode is active (the browser never delivers Fn keydown to the
    /// web layer, so the recorder cannot capture it; CGEventTap detects it in recording mode and
    /// reports it so the frontend ShortcutRecorder can commit an Fn binding).
    FnRecordingPressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyCombinedEdge {
    pub at: Instant,
    pub press_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn shared_with_held_latches() -> Shared {
        Shared {
            binding: RwLock::new(HotkeyBinding::default()),
            trigger_held: AtomicBool::new(true),
            trigger_press_id: AtomicU64::new(42),
            trigger_companion_seen: AtomicU64::new(42),
            qa_trigger: RwLock::new(None),
            qa_trigger_held: AtomicBool::new(true),
            selection_polish_trigger: RwLock::new(None),
            selection_polish_trigger_held: AtomicBool::new(true),
            translation_trigger: RwLock::new(None),
            translation_trigger_held: AtomicBool::new(true),
            translation_modifier_held: AtomicBool::new(true),
            recording_active: AtomicBool::new(false),
            recording_fn_held: AtomicBool::new(false),
        }
    }

    #[test]
    fn reset_shared_held_state_clears_all_shortcut_latches() {
        let shared = shared_with_held_latches();
        reset_shared_held_state(&shared);

        assert!(!shared.trigger_held.load(Ordering::SeqCst));
        assert_eq!(shared.trigger_press_id.load(Ordering::SeqCst), 0);
        assert_eq!(shared.trigger_companion_seen.load(Ordering::SeqCst), 0);
        assert!(!shared.qa_trigger_held.load(Ordering::SeqCst));
        assert!(!shared.selection_polish_trigger_held.load(Ordering::SeqCst));
        assert!(!shared.translation_trigger_held.load(Ordering::SeqCst));
        assert!(!shared.translation_modifier_held.load(Ordering::SeqCst));
    }

    #[test]
    fn update_binding_resets_only_dictation_latch() {
        let shared = shared_with_held_latches();
        let next = HotkeyBinding {
            trigger: HotkeyTrigger::LeftControl,
            mode: crate::types::HotkeyMode::Hold,
            keys: None,
        };

        update_shared_binding(&shared, next.clone());

        assert_eq!(*shared.binding.read(), next);
        assert!(!shared.trigger_held.load(Ordering::SeqCst));
        assert_eq!(shared.trigger_press_id.load(Ordering::SeqCst), 0);
        assert_eq!(shared.trigger_companion_seen.load(Ordering::SeqCst), 0);
        assert!(shared.qa_trigger_held.load(Ordering::SeqCst));
        assert!(shared.selection_polish_trigger_held.load(Ordering::SeqCst));
        assert!(shared.translation_trigger_held.load(Ordering::SeqCst));
        assert!(shared.translation_modifier_held.load(Ordering::SeqCst));
    }

    #[test]
    fn update_modifier_shortcuts_resets_only_modifier_latches() {
        let shared = shared_with_held_latches();

        update_shared_modifier_shortcuts(
            &shared,
            Some(HotkeyTrigger::RightCommand),
            Some(HotkeyTrigger::RightControl),
            Some(HotkeyTrigger::LeftOption),
        );

        assert_eq!(*shared.qa_trigger.read(), Some(HotkeyTrigger::RightCommand));
        assert_eq!(
            *shared.selection_polish_trigger.read(),
            Some(HotkeyTrigger::RightControl)
        );
        assert_eq!(
            *shared.translation_trigger.read(),
            Some(HotkeyTrigger::LeftOption)
        );
        assert!(shared.trigger_held.load(Ordering::SeqCst));
        assert!(!shared.qa_trigger_held.load(Ordering::SeqCst));
        assert!(!shared.selection_polish_trigger_held.load(Ordering::SeqCst));
        assert!(!shared.translation_trigger_held.load(Ordering::SeqCst));
        assert!(shared.translation_modifier_held.load(Ordering::SeqCst));
    }
}

pub trait HotkeyAdapter: Send + Sync {
    fn kind(&self) -> HotkeyAdapterKind;
    fn update_binding(&self, binding: HotkeyBinding);
    fn update_modifier_shortcuts(
        &self,
        qa_trigger: Option<HotkeyTrigger>,
        selection_polish_trigger: Option<HotkeyTrigger>,
        translation_trigger: Option<HotkeyTrigger>,
    );
    fn reset_held_state(&self);
    /// Shortcut-recording mode switch. While active the listener reports Fn press edges
    /// (`FnRecordingPressed`) so the frontend recorder can commit an Fn binding (the browser never
    /// delivers Fn keydown to the web layer).
    fn set_recording_active(&self, _active: bool) {}
    fn shutdown(&self) {}
}

struct Shared {
    binding: RwLock<HotkeyBinding>,
    /// Whether the trigger key is currently held. Deduplicates OS auto-repeat events.
    trigger_held: AtomicBool,
    /// Global generation of the current trigger press. Generated by the listener so late events on
    /// the separate abort channel are not mistaken for the next press.
    trigger_press_id: AtomicU64,
    /// Press generation in which a companion (normal) key was seen; 0 means none seen yet.
    trigger_companion_seen: AtomicU64,
    qa_trigger: RwLock<Option<HotkeyTrigger>>,
    qa_trigger_held: AtomicBool,
    selection_polish_trigger: RwLock<Option<HotkeyTrigger>>,
    selection_polish_trigger_held: AtomicBool,
    translation_trigger: RwLock<Option<HotkeyTrigger>>,
    translation_trigger_held: AtomicBool,
    /// Whether Shift (translation modifier) is currently held. Used to detect the down edge on
    /// FLAGS_CHANGED (only false → true emits TranslationModifierPressed). See issue #4.
    translation_modifier_held: AtomicBool,
    /// Whether shortcut recording is active (ShortcutRecorder capturing). While active the
    /// CGEventTap reports Fn press edges (`FnRecordingPressed`) for the frontend recorder.
    recording_active: AtomicBool,
    /// Whether Fn is held in recording mode; detects the down edge on FLAGS_CHANGED (dedup).
    recording_fn_held: AtomicBool,
}

pub struct HotkeyMonitor {
    adapter: Box<dyn HotkeyAdapter>,
}

impl HotkeyMonitor {
    /// Spawn the listener thread and **wait synchronously** for it to confirm
    /// the OS-level hook installed so the caller can surface an actual adapter
    /// status instead of silently dropping events.
    ///
    /// `cancel_tx`: sends `()` as soon as Esc is pressed. Separate from `tx` (see module docs) —
    /// it must not share the serial Pressed/Released bridge, or cancels queue behind Processing.
    /// `combo_tx`: sends a timestamped press edge when a normal key is pressed while the trigger is
    /// held. Separate from `tx` (see module docs) — otherwise the abort queues behind
    /// `begin_session`'s mic open and the capsule lingers hundreds of ms.
    pub fn start(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<HotkeyCombinedEdge>,
    ) -> Result<Self, HotkeyInstallError> {
        Ok(Self {
            adapter: platform::start_adapter(binding, tx, cancel_tx, combo_tx)?,
        })
    }

    pub fn update_binding(&self, binding: HotkeyBinding) {
        self.adapter.update_binding(binding);
    }

    pub fn update_modifier_shortcuts(
        &self,
        qa_trigger: Option<HotkeyTrigger>,
        selection_polish_trigger: Option<HotkeyTrigger>,
        translation_trigger: Option<HotkeyTrigger>,
    ) {
        self.adapter.update_modifier_shortcuts(
            qa_trigger,
            selection_polish_trigger,
            translation_trigger,
        );
    }

    pub fn kind(&self) -> HotkeyAdapterKind {
        self.adapter.kind()
    }

    pub fn reset_held_state(&self) {
        self.adapter.reset_held_state();
    }

    pub fn set_recording_active(&self, active: bool) {
        self.adapter.set_recording_active(active);
    }

    pub fn capability() -> HotkeyCapability {
        HotkeyCapability::current()
    }
}

impl Drop for HotkeyMonitor {
    fn drop(&mut self) {
        self.adapter.shutdown();
    }
}

fn install_error(code: &str, message: impl Into<String>) -> HotkeyInstallError {
    HotkeyInstallError {
        code: code.into(),
        message: message.into(),
    }
}

fn send_or_log(tx: &Sender<HotkeyEvent>, evt: HotkeyEvent) {
    if let Err(e) = tx.send(evt) {
        log::warn!("[hotkey] 事件发送失败: {e}");
    }
}

fn send_cancel_or_log(tx: &Sender<()>) {
    if let Err(e) = tx.send(()) {
        log::warn!("[hotkey] 取消事件发送失败: {e}");
    }
}

fn send_combo_abort_or_log(tx: &Sender<HotkeyCombinedEdge>, press_id: u64) {
    if let Err(e) = tx.send(HotkeyCombinedEdge {
        at: Instant::now(),
        press_id,
    }) {
        log::warn!("[hotkey] 组合键撤销事件发送失败: {e}");
    }
}

static NEXT_PRESS_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_press_id() -> u64 {
    // Only needs to be unique across listeners; edge ordering is expressed by each edge's Instant.
    NEXT_PRESS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// While a session is active (capsule showing recording/transcribing/polishing), Esc is owned
/// exclusively by OpenLess: the tap/hook swallows the keydown and does not forward it to the host
/// app. Otherwise one Esc would take effect twice — canceling the OpenLess session and triggering
/// the host app's own Esc behavior (e.g. canceling a Claude reply being generated). This mirrors
/// IME behavior: while the composition window is active, Esc only cancels candidates and the host
/// app never sees it. keyup is not swallowed: host apps almost all respond to Esc on keydown, an
/// orphan keyup is harmless, and swallowing up-but-not-down within the window would unpair the
/// events. Updated by the coordinator's emit_capsule on capsule state changes.
static ESC_EXCLUSIVE: AtomicBool = AtomicBool::new(false);

pub fn set_esc_exclusive(active: bool) {
    ESC_EXCLUSIVE.store(active, std::sync::atomic::Ordering::SeqCst);
}

fn esc_exclusive() -> bool {
    ESC_EXCLUSIVE.load(std::sync::atomic::Ordering::SeqCst)
}
type StartupTx<T> = mpsc::Sender<Result<T, HotkeyInstallError>>;

struct ListenerThread<T> {
    shared: Arc<Shared>,
    startup: T,
}

fn start_listener_thread<T, F>(
    binding: HotkeyBinding,
    tx: Sender<HotkeyEvent>,
    cancel_tx: Sender<()>,
    combo_tx: Sender<HotkeyCombinedEdge>,
    thread_name: &str,
    startup_timeout_message: &'static str,
    run_listen_loop: F,
) -> Result<ListenerThread<T>, HotkeyInstallError>
where
    T: Send + 'static,
    F: FnOnce(
            Arc<Shared>,
            Sender<HotkeyEvent>,
            Sender<()>,
            Sender<HotkeyCombinedEdge>,
            StartupTx<T>,
        ) + Send
        + 'static,
{
    let shared = Arc::new(Shared {
        binding: RwLock::new(binding),
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
    });

    let thread_shared = Arc::clone(&shared);
    let (status_tx, status_rx) = mpsc::channel::<Result<T, HotkeyInstallError>>();
    std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || run_listen_loop(thread_shared, tx, cancel_tx, combo_tx, status_tx))
        .map_err(|e| install_error("spawn_failed", format!("hotkey 线程启动失败: {e}")))?;

    match status_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(startup)) => Ok(ListenerThread { shared, startup }),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(install_error("startup_timeout", startup_timeout_message)),
    }
}

fn update_shared_binding(shared: &Shared, binding: HotkeyBinding) {
    {
        let mut current = shared.binding.write();
        if *current == binding {
            // Binding unchanged (e.g. the supervisor re-applies the same binding every 5s): do not
            // touch the held latch. Otherwise a long press would get its "held" flag cleared to
            // false mid-press, so on release `!is_active && was_held` fails and no Released is
            // emitted — hold mode (Less Computer push-to-talk) keeps recording and needs another
            // press. Repro: hold >5s across one supervisor poll.
            return;
        }
        *current = binding;
    }
    shared
        .trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .trigger_press_id
        .store(0, std::sync::atomic::Ordering::SeqCst);
    shared
        .trigger_companion_seen
        .store(0, std::sync::atomic::Ordering::SeqCst);
}

fn update_shared_modifier_shortcuts(
    shared: &Shared,
    qa_trigger: Option<HotkeyTrigger>,
    selection_polish_trigger: Option<HotkeyTrigger>,
    translation_trigger: Option<HotkeyTrigger>,
) {
    *shared.qa_trigger.write() = qa_trigger;
    *shared.selection_polish_trigger.write() = selection_polish_trigger;
    *shared.translation_trigger.write() = translation_trigger;
    shared
        .qa_trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .selection_polish_trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .translation_trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
}

fn reset_shared_held_state(shared: &Shared) {
    shared
        .trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .trigger_companion_seen
        .store(0, std::sync::atomic::Ordering::SeqCst);
    shared
        .trigger_press_id
        .store(0, std::sync::atomic::Ordering::SeqCst);
    shared
        .qa_trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .selection_polish_trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .translation_trigger_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
    shared
        .translation_modifier_held
        .store(false, std::sync::atomic::Ordering::SeqCst);
}

// ─────────────────────────── macOS implementation ───────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    //! 没有原生全局热键后端的桌面构建：安装直接失败，调用方据此给用户明确提示，
    //! 而不是默默不工作。真实实现见 macOS（CGEventTap）与 Windows（WH_KEYBOARD_LL）。

    use super::{
        HotkeyAdapter, HotkeyBinding, HotkeyCombinedEdge, HotkeyEvent, HotkeyInstallError,
    };
    use std::sync::mpsc::Sender;

    pub fn start_adapter(
        _binding: HotkeyBinding,
        _tx: Sender<HotkeyEvent>,
        _cancel_tx: Sender<()>,
        _combo_tx: Sender<HotkeyCombinedEdge>,
    ) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
        Err(HotkeyInstallError {
            code: "hotkey_unsupported".to_string(),
            message: "当前平台不提供原生全局热键后端".to_string(),
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
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
                if let Some(tap) = *ctx.handles.tap.lock().unwrap() {
                    unsafe { CGEventTapEnable(tap, true) };
                }
                return event;
            }
            FLAGS_CHANGED => handle_flags_changed(ctx, event),
            KEY_DOWN => {
                handle_key_down(ctx, event);
                let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
                crate::side_aware_combo::platform::dispatch_keycode(keycode, false, 0, true);
                // While a session is active, consume Esc exclusively: returning null deletes the
                // event (active tap) so the host app never sees it, avoiding double effect between
                // "cancel session" and the host's Esc semantics. See esc_exclusive docs.
                if keycode == ESC_KEYCODE && esc_exclusive() {
                    return std::ptr::null_mut();
                }
            }
            KEY_UP => {
                let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
                crate::side_aware_combo::platform::dispatch_keycode(keycode, false, 0, false);
            }
            // Fn+brightness / volume / keyboard backlight and similar do not produce KEY_DOWN but
            // systemDefined. While the Fn trigger is held, treat these as function-layer combos —
            // dictation must not start at the same time.
            SYSTEM_DEFINED => {
                if let Some((subtype, data1)) = system_defined_event_payload(event) {
                    note_fn_function_layer_event(ctx, subtype, data1);
                }
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
        crate::side_aware_combo::platform::dispatch_keycode(keycode, true, flags, false);
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
                send_or_log(&ctx.tx, HotkeyEvent::Released { at: now, press_id });
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
                send_or_log(&ctx.tx, HotkeyEvent::Released { at: now, press_id });
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
                crate::selection::prefetch_selection_workspace_capture();
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
        crate::side_aware_combo::handle_companion_key_down();
        if keycode == ESC_KEYCODE {
            note_companion_key_down(ctx);
            send_cancel_or_log(&ctx.cancel_tx);
            return;
        }
        note_companion_key_down(ctx);
    }

    fn system_defined_event_payload(event: CgEventRef) -> Option<(i16, isize)> {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};

        if event.is_null() {
            return None;
        }
        let cls = AnyClass::get("NSEvent")?;
        let ns_event: *mut AnyObject =
            unsafe { msg_send![cls, eventWithCGEvent: event.cast::<c_void>()] };
        if ns_event.is_null() {
            return None;
        }
        let subtype: i16 = unsafe { msg_send![ns_event, subtype] };
        let data1: isize = unsafe { msg_send![ns_event, data1] };
        Some((subtype, data1))
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
            note_fn_function_layer_event(
                &ctx,
                NX_SUBTYPE_AUX_CONTROL_BUTTONS,
                (2_i64 << 16) as isize,
            );
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
}

// ─────────────────────────── Windows implementation ───────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use std::cell::Cell;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_QUIT,
    };

    use super::{
        esc_exclusive, install_error, reset_shared_held_state, send_cancel_or_log,
        send_combo_abort_or_log, send_or_log, start_listener_thread, update_shared_binding,
        update_shared_modifier_shortcuts, HotkeyAdapter, HotkeyCombinedEdge, HotkeyEvent, Shared,
        StartupTx,
    };
    use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyTrigger};

    const WM_KEYDOWN: usize = 0x0100;
    const WM_KEYUP: usize = 0x0101;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const WM_SYSKEYUP: usize = 0x0105;

    const VK_ESCAPE: u32 = 0x1B;
    const VK_SHIFT: u32 = 0x10;
    const VK_CONTROL: u32 = 0x11;
    const VK_MENU: u32 = 0x12;
    const VK_CAPITAL: u32 = 0x14;
    const VK_LSHIFT: u32 = 0xA0;
    const VK_RSHIFT: u32 = 0xA1;
    const VK_LCONTROL: u32 = 0xA2;
    const VK_RCONTROL: u32 = 0xA3;
    const VK_LMENU: u32 = 0xA4;
    const VK_RMENU: u32 = 0xA5;
    const VK_RWIN: u32 = 0x5C;
    const VK_LWIN: u32 = 0x5B;
    const VK_MEDIA_PLAY_PAUSE: u32 = 0xB3;
    thread_local! {
        // The WH_KEYBOARD_LL callback runs on the thread that installed the hook. Main dictation
        // and Less Computer each have their own listener thread; a process-global pointer would be
        // overwritten by the second monitor and dangle after it exits. Each thread owns its own
        // context; unhook/clear the slot before dropping the Box.
        static HOOK_CONTEXT: Cell<*mut CallbackContext> = const { Cell::new(std::ptr::null_mut()) };
    }

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
            "openless-hotkey-win-ll-hook",
            "Windows hotkey hook 启动超时",
            run_listen_loop,
        )?;
        Ok(Box::new(WindowsHotkeyAdapter {
            shared: listener.shared,
            thread_id: listener.startup,
        }))
    }

    struct WindowsHotkeyAdapter {
        shared: Arc<Shared>,
        thread_id: u32,
    }

    impl HotkeyAdapter for WindowsHotkeyAdapter {
        fn kind(&self) -> HotkeyAdapterKind {
            HotkeyAdapterKind::WindowsLowLevel
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
        }

        fn shutdown(&self) {
            unsafe {
                if let Err(err) = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0))
                {
                    log::warn!("[hotkey] Windows hook 退出消息发送失败: {err}");
                }
            }
        }
    }

    struct CallbackContext {
        shared: Arc<Shared>,
        tx: Sender<HotkeyEvent>,
        /// Esc-only channel, see module docs — must not share tx's serial bridge.
        cancel_tx: Sender<()>,
        /// Combo-abort-only channel, see module docs — must not share tx's serial bridge.
        combo_tx: Sender<HotkeyCombinedEdge>,
        hook: std::sync::Mutex<Option<HHOOK>>,
    }

    fn run_listen_loop(
        shared: Arc<Shared>,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<HotkeyCombinedEdge>,
        status_tx: StartupTx<u32>,
    ) {
        let thread_id = unsafe { GetCurrentThreadId() };
        let context = Box::into_raw(Box::new(CallbackContext {
            shared,
            tx,
            cancel_tx,
            combo_tx,
            hook: std::sync::Mutex::new(None),
        }));
        HOOK_CONTEXT.set(context);

        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0);
            match hook {
                Ok(hook) => {
                    *(*context).hook.lock().unwrap() = Some(hook);
                    log::info!("[hotkey] Windows low-level keyboard hook 已启动");
                    if status_tx.send(Ok(thread_id)).is_err() {
                        // After the starter timed out and gave up, nobody holds the monitor; do
                        // not leave an orphaned hook behind.
                        let _ = UnhookWindowsHookEx(hook);
                        HOOK_CONTEXT.set(std::ptr::null_mut());
                        let _ = Box::from_raw(context);
                        return;
                    }
                }
                Err(err) => {
                    HOOK_CONTEXT.set(std::ptr::null_mut());
                    let _ = Box::from_raw(context);
                    let _ = status_tx.send(Err(install_error(
                        "hook_install_failed",
                        format!("Windows low-level keyboard hook 安装失败: {err}"),
                    )));
                    return;
                }
            }

            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, None, 0, 0).0;
                if result == -1 {
                    log::error!("[hotkey] Windows GetMessageW 返回错误，hook 线程退出");
                    break;
                }
                if result == 0 {
                    log::warn!("[hotkey] Windows hook 消息循环收到退出消息");
                    break;
                }
                let _ = TranslateMessage(&message);
                let _ = DispatchMessageW(&message);
            }

            if let Some(hook) = (*context).hook.lock().unwrap().take() {
                let _ = UnhookWindowsHookEx(hook);
            }
            // The listener thread may exit while the trigger is still held (config reload, app
            // shutdown, or an abnormal end of the hook message loop). Clear the internal latches
            // first so the next listener does not inherit stale pressed state when reusing the
            // shared state.
            super::reset_shared_held_state(&(*context).shared);
            HOOK_CONTEXT.set(std::ptr::null_mut());
            let _ = Box::from_raw(context);
        }
    }

    unsafe extern "system" fn low_level_keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32 && lparam.0 != 0 {
            if let Some(ctx) = callback_context() {
                let keyboard = *(lparam.0 as *const KBDLLHOOKSTRUCT);
                // Synthetic input (SendInput/keybd_event) and real keyboards share the same
                // dispatch path. As long as the event's virtual key matches the current
                // configuration, the existing edge dedup and combo-abort logic still decide
                // whether OpenLess fires; no filtering by synthetic-input source here.
                if dispatch_keyboard_event(ctx, keyboard.vkCode, wparam.0) {
                    return LRESULT(1);
                }
            }
        }

        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe fn callback_context<'a>() -> Option<&'a CallbackContext> {
        let ptr = HOOK_CONTEXT.get();
        if ptr.is_null() {
            None
        } else {
            Some(&*ptr)
        }
    }

    fn dispatch_keyboard_event(ctx: &CallbackContext, vk_code: u32, message: usize) -> bool {
        // Shortcut recording receives real keyboard events in the frontend; the main
        // dictation/Agent hooks must not swallow keys.
        if ctx.shared.recording_active.load(Ordering::SeqCst) {
            return false;
        }
        let pressed = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        if vk_code == VK_ESCAPE && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) {
            crate::side_aware_combo::handle_companion_key_down();
            note_companion_key_down(ctx);
            send_cancel_or_log(&ctx.cancel_tx);
            // While a session is active, consume Esc exclusively (return true → LRESULT(1)
            // swallows it) so the host app never sees it, avoiding double effect between
            // "cancel session" and the host's Esc semantics. See esc_exclusive docs.
            return esc_exclusive();
        }

        crate::side_aware_combo::platform::dispatch_vk(vk_code, pressed);

        if pressed && !is_modifier_vk(vk_code) {
            crate::side_aware_combo::handle_companion_key_down();
            note_companion_key_down(ctx);
        }

        // Shift (either side) = translation-mode modifier. A press takes effect at any time during
        // recording. See issue #4.
        if matches!(vk_code, VK_SHIFT | VK_LSHIFT | VK_RSHIFT) {
            match message {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let was_held = ctx
                        .shared
                        .translation_modifier_held
                        .swap(true, Ordering::SeqCst);
                    if !was_held {
                        send_or_log(&ctx.tx, HotkeyEvent::TranslationModifierPressed);
                    }
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    ctx.shared
                        .translation_modifier_held
                        .store(false, Ordering::SeqCst);
                }
                _ => {}
            }
            // Shift must still fall through to the configured-trigger matching below, otherwise a
            // left/right Shift hold-to-talk key would only produce the translation notification,
            // never Pressed/Released, becoming a dead key that can still be saved.
        }

        handle_optional_modifier_trigger(
            ctx,
            vk_code,
            message,
            *ctx.shared.qa_trigger.read(),
            &ctx.shared.qa_trigger_held,
            HotkeyEvent::QaShortcutPressed,
            None,
        );
        handle_optional_modifier_trigger(
            ctx,
            vk_code,
            message,
            *ctx.shared.selection_polish_trigger.read(),
            &ctx.shared.selection_polish_trigger_held,
            HotkeyEvent::SelectionPolishShortcutPressed,
            Some(HotkeyEvent::SelectionPolishShortcutReleased),
        );
        handle_optional_modifier_trigger(
            ctx,
            vk_code,
            message,
            *ctx.shared.translation_trigger.read(),
            &ctx.shared.translation_trigger_held,
            HotkeyEvent::TranslationModifierPressed,
            None,
        );

        let trigger = ctx.shared.binding.read().trigger;
        if trigger == HotkeyTrigger::Custom {
            return false;
        }
        if vk_code != trigger_to_vk_code(trigger) {
            return false;
        }

        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let was_held = ctx.shared.trigger_held.swap(true, Ordering::SeqCst);
                if !was_held {
                    let press_id = super::next_press_id();
                    ctx.shared
                        .trigger_press_id
                        .store(press_id, Ordering::SeqCst);
                    ctx.shared.trigger_companion_seen.store(0, Ordering::SeqCst);
                    log::info!("[hotkey] Windows trigger pressed vk={vk_code}");
                    send_or_log(
                        &ctx.tx,
                        HotkeyEvent::Pressed {
                            at: std::time::Instant::now(),
                            press_id,
                        },
                    );
                }
            }
            WM_KEYUP | WM_SYSKEYUP => {
                let was_held = ctx.shared.trigger_held.swap(false, Ordering::SeqCst);
                if was_held {
                    let press_id = ctx.shared.trigger_press_id.swap(0, Ordering::SeqCst);
                    log::info!("[hotkey] Windows trigger released vk={vk_code}");
                    send_or_log(
                        &ctx.tx,
                        HotkeyEvent::Released {
                            at: std::time::Instant::now(),
                            press_id,
                        },
                    );
                }
            }
            _ => {}
        }
        true
    }

    /// A non-modifier key pressed while the trigger is held = the user is typing a combo
    /// (Alt+Tab / Alt+F4, ...), not speaking — send one abort on the combo channel so the upper
    /// layer cancels this press. Why `combo_tx` instead of `tx`: see module docs (the `tx` side is
    /// most likely still inside this very press's begin_session). Modifiers themselves do not
    /// count as "other keys", matching the macOS side (modifiers go through FLAGS_CHANGED, never
    /// KEY_DOWN): overlaying Shift (translation modifier) or Ctrl does not abort dictation.
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
        log::info!("[hotkey] Windows 触发键与其他键组合按下 —— 撤销本次触发");
        send_combo_abort_or_log(&ctx.combo_tx, press_id);
    }

    fn is_modifier_vk(vk_code: u32) -> bool {
        matches!(
            vk_code,
            VK_SHIFT
                | VK_LSHIFT
                | VK_RSHIFT
                | VK_CONTROL
                | VK_LCONTROL
                | VK_RCONTROL
                | VK_MENU
                | VK_LMENU
                | VK_RMENU
                | VK_LWIN
                | VK_RWIN
                | VK_CAPITAL
        )
    }

    fn handle_optional_modifier_trigger(
        ctx: &CallbackContext,
        vk_code: u32,
        message: usize,
        trigger: Option<HotkeyTrigger>,
        held: &std::sync::atomic::AtomicBool,
        press_event: HotkeyEvent,
        release_event: Option<HotkeyEvent>,
    ) {
        let Some(trigger) = trigger else {
            return;
        };
        if trigger == HotkeyTrigger::Custom || vk_code != trigger_to_vk_code(trigger) {
            return;
        }
        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let was_held = held.swap(true, Ordering::SeqCst);
                if !was_held {
                    if matches!(press_event, HotkeyEvent::SelectionPolishShortcutPressed) {
                        crate::selection::prefetch_selection_workspace_capture();
                    }
                    send_or_log(&ctx.tx, press_event);
                }
            }
            WM_KEYUP | WM_SYSKEYUP => {
                let was_held = held.swap(false, Ordering::SeqCst);
                if was_held {
                    if let Some(release_event) = release_event {
                        send_or_log(&ctx.tx, release_event);
                    }
                }
            }
            _ => {}
        }
    }

    fn trigger_to_vk_code(trigger: HotkeyTrigger) -> u32 {
        // The Windows low-level hook distinguishes left/right Alt, so LeftOption / RightOption
        // must keep their physical side. The few remaining cross-platform aliases still fold onto
        // Windows-available physical keys:
        // - Fn reuses RightControl / VK_RCONTROL
        match trigger {
            HotkeyTrigger::RightControl => VK_RCONTROL,
            HotkeyTrigger::LeftControl => VK_LCONTROL,
            HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => VK_RMENU,
            HotkeyTrigger::RightCommand => VK_RWIN,
            HotkeyTrigger::LeftCommand => VK_LWIN,
            HotkeyTrigger::LeftShift => VK_LSHIFT,
            HotkeyTrigger::RightShift => VK_RSHIFT,
            HotkeyTrigger::LeftOption => VK_LMENU,
            HotkeyTrigger::Fn => VK_RCONTROL,
            HotkeyTrigger::MediaPlayPause => VK_MEDIA_PLAY_PAUSE,
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
            let (combo_abort_tx, combo_abort_rx) = mpsc::channel();
            (
                CallbackContext {
                    shared,
                    tx,
                    cancel_tx,
                    combo_tx: combo_abort_tx,
                    hook: std::sync::Mutex::new(None),
                },
                rx,
                combo_abort_rx,
            )
        }

        /// Test cases that don't care about combo aborts: the abort channel's receiver drops
        /// them in place (these cases never send on it).
        fn callback_context(shared: Arc<Shared>) -> (CallbackContext, mpsc::Receiver<HotkeyEvent>) {
            let (ctx, rx, _combo_abort_rx) = callback_context_with_combo(shared);
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
        fn windows_hook_context_belongs_to_its_listener_thread() {
            let (ctx, _) = callback_context(shared(HotkeyTrigger::RightControl));
            let mut ctx = Box::new(ctx);
            HOOK_CONTEXT.set(&mut *ctx);
            let other_thread_has_no_context =
                std::thread::spawn(|| unsafe { super::callback_context().is_none() })
                    .join()
                    .unwrap();
            HOOK_CONTEXT.set(std::ptr::null_mut());
            assert!(
                other_thread_has_no_context,
                "another listener must never see this hook's context"
            );
        }

        #[test]
        fn windows_shift_trigger_keeps_both_recording_edges() {
            for (trigger, key) in [
                (HotkeyTrigger::LeftShift, VK_LSHIFT),
                (HotkeyTrigger::RightShift, VK_RSHIFT),
            ] {
                let (ctx, rx) = callback_context(shared(trigger));
                assert!(dispatch_keyboard_event(&ctx, key, WM_KEYDOWN));
                assert!(dispatch_keyboard_event(&ctx, key, WM_KEYUP));
                let events: Vec<_> = rx
                    .try_iter()
                    .filter(|event| !matches!(event, HotkeyEvent::TranslationModifierPressed))
                    .collect();
                assert!(matches!(
                    events.as_slice(),
                    [HotkeyEvent::Pressed { .. }, HotkeyEvent::Released { .. }]
                ));
            }
        }

        #[test]
        fn windows_shortcut_recording_passes_bound_keys_to_the_frontend() {
            let state = shared(HotkeyTrigger::LeftControl);
            state.recording_active.store(true, Ordering::SeqCst);
            let (ctx, rx) = callback_context(Arc::clone(&state));
            assert!(!dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYDOWN));
            assert!(!dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYUP));
            assert!(rx.try_recv().is_err());
            state.recording_active.store(false, Ordering::SeqCst);
            assert!(dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYUP));
            assert_eq!(rx.try_iter().count(), 2);
        }

        #[test]
        #[ignore = "interactive Windows smoke: injects captured Ctrl keys into real hooks"]
        fn windows_multiple_native_monitors_survive_independent_shutdown() {
            use crate::hotkey::HotkeyMonitor;
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
            };

            let start = |trigger| {
                let (tx, rx) = mpsc::channel();
                let (cancel_tx, _) = mpsc::channel();
                let (combo_tx, _) = mpsc::channel();
                let monitor = HotkeyMonitor::start(
                    HotkeyBinding {
                        trigger,
                        mode: crate::types::HotkeyMode::Hold,
                        keys: None,
                    },
                    tx,
                    cancel_tx,
                    combo_tx,
                )
                .unwrap();
                (monitor, rx)
            };
            let press_and_release = |key: u32, rx: &mpsc::Receiver<HotkeyEvent>| {
                let input = |flags| INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(key as u16),
                            dwFlags: flags,
                            ..Default::default()
                        },
                    },
                };
                // Both Ctrl keys are swallowed by their own hooks; no text reaches the target app.
                assert_eq!(
                    unsafe {
                        SendInput(
                            &[input(Default::default()), input(KEYEVENTF_KEYUP)],
                            std::mem::size_of::<INPUT>() as i32,
                        )
                    },
                    2
                );
                let pressed = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
                let released = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
                assert!(matches!((pressed, released), (
                    HotkeyEvent::Pressed { press_id: left, .. },
                    HotkeyEvent::Released { press_id: right, .. }
                ) if left != 0 && left == right));
            };
            let (dictation, dictation_rx) = start(HotkeyTrigger::RightControl);
            let (agent, agent_rx) = start(HotkeyTrigger::LeftControl);
            press_and_release(VK_RCONTROL, &dictation_rx);
            assert!(agent_rx.try_recv().is_err());
            press_and_release(VK_LCONTROL, &agent_rx);
            assert!(dictation_rx.try_recv().is_err());
            drop(agent);
            assert_eq!(
                agent_rx.recv_timeout(std::time::Duration::from_secs(2)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
            press_and_release(VK_RCONTROL, &dictation_rx);
            let (replacement, replacement_rx) = start(HotkeyTrigger::LeftControl);
            press_and_release(VK_LCONTROL, &replacement_rx);
            drop(dictation);
            assert_eq!(
                dictation_rx.recv_timeout(std::time::Duration::from_secs(2)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
            press_and_release(VK_LCONTROL, &replacement_rx);
            drop(replacement);
            assert_eq!(
                replacement_rx.recv_timeout(std::time::Duration::from_secs(2)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        }

        #[test]
        fn windows_modifier_edges_are_deduped_from_mock_hook_events() {
            let shared = shared(HotkeyTrigger::RightControl);
            let (ctx, rx) = callback_context(shared);

            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP));

            let events = drain(&rx);
            assert_eq!(edge_names(events.clone()), vec!["pressed", "released"]);
            let [HotkeyEvent::Pressed {
                press_id: pressed_id,
                ..
            }, HotkeyEvent::Released {
                press_id: released_id,
                ..
            }] = events.as_slice()
            else {
                panic!("expected one paired Windows press/release generation")
            };
            assert_ne!(*pressed_id, 0);
            assert_eq!(pressed_id, released_id);
        }

        #[test]
        fn windows_unrelated_key_does_not_trigger_configured_modifier() {
            let shared = shared(HotkeyTrigger::RightControl);
            let (ctx, rx) = callback_context(shared);

            assert!(!dispatch_keyboard_event(&ctx, 0x41, WM_KEYDOWN));
            assert!(!dispatch_keyboard_event(&ctx, 0x41, WM_KEYUP));
            assert!(drain(&rx).is_empty());
        }

        #[test]
        fn windows_modifier_edges_ignore_unrelated_keys_and_reemit_after_release() {
            let shared = shared(HotkeyTrigger::RightControl);
            let (ctx, rx) = callback_context(shared);

            assert!(!dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN));

            assert_eq!(
                edge_names(drain(&rx)),
                vec!["pressed", "released", "pressed"]
            );
        }

        #[test]
        fn windows_optional_modifier_shortcuts_use_independent_latches() {
            let shared = shared(HotkeyTrigger::RightControl);
            *shared.qa_trigger.write() = Some(HotkeyTrigger::RightCommand);
            *shared.translation_trigger.write() = Some(HotkeyTrigger::LeftOption);
            let (ctx, rx) = callback_context(shared);

            dispatch_keyboard_event(&ctx, VK_RWIN, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_RWIN, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_LMENU, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_LSHIFT, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_LSHIFT, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_RWIN, WM_KEYUP);
            dispatch_keyboard_event(&ctx, VK_RWIN, WM_KEYDOWN);

            assert_eq!(
                drain(&rx),
                vec![
                    HotkeyEvent::QaShortcutPressed,
                    HotkeyEvent::TranslationModifierPressed,
                    HotkeyEvent::TranslationModifierPressed,
                    HotkeyEvent::QaShortcutPressed,
                ]
            );
        }

        #[test]
        fn windows_right_control_routes_only_to_selection_polish_action() {
            let shared = shared(HotkeyTrigger::Custom);
            *shared.selection_polish_trigger.write() = Some(HotkeyTrigger::RightControl);
            let (ctx, rx) = callback_context(shared);

            dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP);
            dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN);

            assert_eq!(
                drain(&rx),
                vec![
                    HotkeyEvent::SelectionPolishShortcutPressed,
                    HotkeyEvent::SelectionPolishShortcutReleased,
                    HotkeyEvent::SelectionPolishShortcutPressed,
                ]
            );
        }

        #[test]
        fn windows_option_triggers_keep_left_and_right_alt_separate() {
            let left_shared = shared(HotkeyTrigger::LeftOption);
            let (left_ctx, left_rx) = callback_context(left_shared);

            assert!(!dispatch_keyboard_event(&left_ctx, VK_RMENU, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&left_ctx, VK_LMENU, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&left_ctx, VK_LMENU, WM_KEYUP));
            assert_eq!(edge_names(drain(&left_rx)), vec!["pressed", "released"]);

            let right_option_shared = shared(HotkeyTrigger::RightOption);
            let (right_option_ctx, right_option_rx) = callback_context(right_option_shared);
            assert!(!dispatch_keyboard_event(
                &right_option_ctx,
                VK_LMENU,
                WM_KEYDOWN
            ));
            assert!(dispatch_keyboard_event(
                &right_option_ctx,
                VK_RMENU,
                WM_KEYDOWN
            ));
            assert_eq!(edge_names(drain(&right_option_rx)), vec!["pressed"]);

            let right_alt_shared = shared(HotkeyTrigger::RightAlt);
            let (right_alt_ctx, right_alt_rx) = callback_context(right_alt_shared);
            assert!(!dispatch_keyboard_event(
                &right_alt_ctx,
                VK_LMENU,
                WM_KEYDOWN
            ));
            assert!(dispatch_keyboard_event(
                &right_alt_ctx,
                VK_RMENU,
                WM_KEYDOWN
            ));
            assert_eq!(edge_names(drain(&right_alt_rx)), vec!["pressed"]);
        }

        // Combos like Alt+any letter/digit key: a normal key press aborts this trigger;
        // modifier overlays (Shift = translation mode) do not.
        #[test]
        fn windows_companion_key_down_aborts_trigger_but_modifiers_do_not() {
            let shared = shared(HotkeyTrigger::LeftOption);
            let (ctx, _rx, combo_abort_rx) = callback_context_with_combo(shared);

            dispatch_keyboard_event(&ctx, VK_LMENU, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_LSHIFT, WM_KEYDOWN);
            assert_eq!(drain_combo(&combo_abort_rx), 0);

            dispatch_keyboard_event(&ctx, 0x41, WM_KEYDOWN); // A
            dispatch_keyboard_event(&ctx, 0x41, WM_KEYDOWN); // OS auto-repeat
            assert_eq!(drain_combo(&combo_abort_rx), 1);
        }

        #[test]
        fn windows_escape_while_trigger_held_is_also_a_companion_key() {
            let shared = shared(HotkeyTrigger::LeftOption);
            let (ctx, _rx, combo_abort_rx) = callback_context_with_combo(shared);

            dispatch_keyboard_event(&ctx, VK_LMENU, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_ESCAPE, WM_KEYDOWN);

            assert_eq!(drain_combo(&combo_abort_rx), 1);
        }

        #[test]
        fn windows_shift_side_combo_receives_pressed_via_dispatch_keyboard_event() {
            use crate::side_aware_combo::SideAwareComboMonitor;
            use crate::types::ShortcutBinding;

            let (combo_tx, combo_rx) = mpsc::channel();
            let (abort_tx, _abort_rx) = mpsc::channel();
            let binding = ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["shift-left".into()],
            };
            let monitor =
                SideAwareComboMonitor::start(binding, combo_tx, abort_tx).expect("start monitor");

            let shared = shared(HotkeyTrigger::Custom);
            let (ctx, hotkey_rx) = callback_context(shared);

            dispatch_keyboard_event(&ctx, VK_LSHIFT, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, 0x44, WM_KEYDOWN);

            assert!(matches!(
                combo_rx.recv().unwrap(),
                HotkeyEvent::Pressed { .. }
            ));
            assert!(hotkey_rx
                .try_iter()
                .any(|evt| evt == HotkeyEvent::TranslationModifierPressed));

            drop(monitor);
        }

        #[test]
        fn windows_modifier_chord_uses_existing_companion_abort_semantics() {
            use crate::side_aware_combo::SideAwareComboMonitor;
            use crate::types::ShortcutBinding;

            let (tx, rx) = mpsc::channel();
            let (abort_tx, abort_rx) = mpsc::channel();
            let binding = ShortcutBinding {
                primary: "ModifierChord".into(),
                modifiers: vec!["ctrl-left".into(), "cmd-left".into()],
            };
            let monitor =
                SideAwareComboMonitor::start(binding, tx, abort_tx).expect("start monitor");

            let shared = shared(HotkeyTrigger::Custom);
            let (ctx, _main_rx) = callback_context(shared);

            dispatch_keyboard_event(&ctx, VK_LCONTROL, WM_KEYDOWN);
            assert!(rx.try_recv().is_err());
            dispatch_keyboard_event(&ctx, VK_LWIN, WM_KEYDOWN);
            let press_id = match rx.recv().unwrap() {
                HotkeyEvent::Pressed { press_id, .. } => press_id,
                other => panic!("expected modifier chord Pressed, got {other:?}"),
            };

            dispatch_keyboard_event(&ctx, 0x44, WM_KEYDOWN);
            assert!(matches!(
                abort_rx.recv().unwrap(),
                HotkeyCombinedEdge {
                    press_id: combined_id,
                    ..
                } if combined_id == press_id
            ));
            dispatch_keyboard_event(&ctx, 0x44, WM_KEYDOWN);
            assert!(abort_rx.try_recv().is_err());

            dispatch_keyboard_event(&ctx, VK_LWIN, WM_KEYUP);
            assert!(matches!(
                rx.recv().unwrap(),
                HotkeyEvent::Released {
                    press_id: released_id,
                    ..
                } if released_id == press_id
            ));

            drop(monitor);
        }
    }
}
