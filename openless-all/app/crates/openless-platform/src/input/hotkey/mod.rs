//! 全局热键监听。
//!
//! 平台后端：
//! - **macOS**：CGEventTap（纯修饰键、Fn、侧键、Esc 独占）；
//! - **Windows**：`WH_KEYBOARD_LL` 低级键盘钩子；
//! - 其余平台：安装直接失败，调用方据此给用户明确提示，而不是默默不工作。
//!
//! 两个宿主行为（预取选区、清理伴生键）通过 [`HostHooks`] 注入，平台层不反向依赖宿主。

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
pub use crate::types::HotkeyAdapterKind;
use crate::types::{HotkeyBinding, HotkeyCapability, HotkeyInstallError};

static CAPSULE_MENU_OPEN: AtomicBool = AtomicBool::new(false);

pub fn set_capsule_menu_open(open: bool) {
    CAPSULE_MENU_OPEN.store(open, std::sync::atomic::Ordering::SeqCst);
}

pub fn capsule_menu_open() -> bool {
    CAPSULE_MENU_OPEN.load(std::sync::atomic::Ordering::SeqCst)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed {
        at: Instant,
        press_id: u64,
    },
    Released {
        at: Instant,
        press_id: u64,
        /// Native per-press verdict, independent of the asynchronous abort bridge.
        combined: bool,
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
            adapter: start_adapter(binding, tx, cancel_tx, combo_tx)?,
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

pub fn next_press_id() -> u64 {
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

pub fn esc_exclusive() -> bool {
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


// ─────────────────────────── 宿主行为钩子 ───────────────────────────

/// 热键监听需要宿主提供的两个行为。平台层不反向依赖宿主，只通过这里向下调用。
#[derive(Debug, Clone, Copy)]
pub struct HostHooks {
    /// 按下听写热键时预取选区（避免与录音启动串行等待）。
    pub prefetch_selection: fn(),
    /// 松开热键时清理录音伴生键的按住状态。
    pub reset_companion_held: fn(),
    /// 录音过程中的伴随键事件；返回 true 表示该键已被消费（吞掉）。
    pub recording_companion_event: fn(&str, bool) -> bool,
    /// 系统是否处于 Secure Event Input（凭据输入框）。macOS 合成按键此时会被静默丢弃。
    pub secure_input_enabled: fn() -> bool,
}

static HOST_HOOKS: std::sync::OnceLock<HostHooks> = std::sync::OnceLock::new();

/// 注册宿主钩子。宿主启动时调用一次；重复调用只有第一次生效。
pub fn install_host_hooks(hooks: HostHooks) {
    let _ = HOST_HOOKS.set(hooks);
}

fn prefetch_selection() {
    if let Some(hooks) = HOST_HOOKS.get() {
        (hooks.prefetch_selection)();
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn reset_companion_held() {
    if let Some(hooks) = HOST_HOOKS.get() {
        (hooks.reset_companion_held)();
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn secure_input_enabled() -> bool {
    match HOST_HOOKS.get() {
        Some(hooks) => (hooks.secure_input_enabled)(),
        None => false,
    }
}

pub fn recording_companion_event(key: &str, pressed: bool) -> bool {
    match HOST_HOOKS.get() {
        Some(hooks) => (hooks.recording_companion_event)(key, pressed),
        None => false,
    }
}


// ─────────────────────────── 平台后端 ───────────────────────────

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as backend;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as backend;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use unsupported as backend;

/// 启动一个热键监听适配器。
pub fn start_adapter(
    binding: openless_core::shared_types::HotkeyBinding,
    tx: std::sync::mpsc::Sender<HotkeyEvent>,
    cancel_tx: std::sync::mpsc::Sender<()>,
    combo_tx: std::sync::mpsc::Sender<HotkeyCombinedEdge>,
) -> Result<Box<dyn HotkeyAdapter>, openless_core::shared_types::HotkeyInstallError> {
    backend::start_adapter(binding, tx, cancel_tx, combo_tx)
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
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    recording_fn_held: AtomicBool,
}
