#![cfg_attr(target_os = "linux", allow(dead_code))]
//! 全局热键监听：发送按下 / 抬起 / 取消三类边沿事件。
//!
//! - macOS：原生 CGEventTap（core-foundation + core-graphics FFI），与 Swift
//!   `OpenLessHotkey/HotkeyMonitor.swift` 同源。
//! - Windows：原生 `WH_KEYBOARD_LL` low-level keyboard hook，保留 modifier-only
//!   trigger（如右 Control / 右 Alt）的真实语义。
//! - Linux：fcitx5 插件提供热键事件（DBus 信号 `DictationKeyEvent`）。
//!
//! 仅产出"边沿"事件，toggle vs hold 由 Coordinator 解释。
//!
//! Esc（取消）与组合键撤销（触发键按住期间叠加了普通键）**都不走** `HotkeyEvent`
//! 通道，而是独立的 `Sender<u64>`：Pressed/Released 的 bridge 线程为了修 #468/#475 的
//! latch 竞态改成了串行 block_on，Pressed / Released 会在 bridge 线程上同步跑完
//! `begin_session`（开麦 + ASR 握手）或整个转写 + 润色流程 —— 若取消 / 撤销与它们同
//! 队列，事件只能排队等流程跑完，观感就是「晚几百毫秒才生效」。独立通道 + 专用消费
//! 线程保证取消 / 撤销随到随处理（`cancel_session` / `handle_trigger_combined` 都是纯
//! 同步快路径：置旗标 + 清资源，不 await）。

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use crate::types::HotkeyTrigger;
use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed { at: Instant, press_id: u64 },
    Released { at: Instant },
    // 组合键撤销不在此枚举里：走独立的 `combo_abort` 通道，避免被上面 Pressed →
    // begin_session 的同步开麦流程堵在队列里（见模块注释）。
    /// Shift（或未来配置项指定的修饰键）按下边沿。可在录音过程中任何时刻产生；
    /// 上层据此切换到翻译输出管线。详见 issue #4。
    TranslationModifierPressed,
    QaShortcutPressed,
    SelectionPolishShortcutPressed,
    SelectionPolishShortcutReleased,
    /// 录制态按下 Fn（浏览器不向网页层下发 Fn 的 keydown，无法通过 recorder 捕获；
    /// 由 CGEventTap 在录制态检测后上报，供前端 ShortcutRecorder 提交 Fn 绑定）。
    FnRecordingPressed,
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
    /// 快捷键录制态开关。激活期间监听器上报 Fn 按下边沿（`FnRecordingPressed`），
    /// 供前端 recorder 提交 Fn 绑定（浏览器不向网页层下发 Fn keydown）。
    fn set_recording_active(&self, _active: bool) {}
    /// 本次按住期间，监听器是否已经看到触发键被叠加了普通键。上层的「仲裁窗口」
    /// 按下后先等一小会儿再读它，命中就整条按下作废（麦克风都不用开）。
    /// 没有键盘监听器的平台（Linux/fcitx5）恒为 false。
    fn trigger_combined_since_press(&self, _press_id: u64) -> bool {
        false
    }
    fn shutdown(&self) {}
}

struct Shared {
    binding: RwLock<HotkeyBinding>,
    /// 触发键当前是否处于"按住"状态。OS 自动重复事件用此去重。
    trigger_held: AtomicBool,
    /// 当前触发键按下的全局代次。代次由监听器生成，避免独立撤销通道的迟到事件
    /// 误认成下一次按下。
    trigger_press_id: AtomicU64,
    /// 已经看到普通键的按下代次；0 表示本次按下尚未看到普通键。
    trigger_companion_seen: AtomicU64,
    qa_trigger: RwLock<Option<HotkeyTrigger>>,
    qa_trigger_held: AtomicBool,
    selection_polish_trigger: RwLock<Option<HotkeyTrigger>>,
    selection_polish_trigger_held: AtomicBool,
    translation_trigger: RwLock<Option<HotkeyTrigger>>,
    translation_trigger_held: AtomicBool,
    /// Shift（翻译修饰键）当前是否按住。用于在 FLAGS_CHANGED 上识别 down 边沿
    /// （只在 false → true 时往上层发 TranslationModifierPressed）。详见 issue #4。
    translation_modifier_held: AtomicBool,
    /// 快捷键录制是否激活（ShortcutRecorder 处于录入态）。激活期间 CGEventTap 上报
    /// Fn 按下边沿（`FnRecordingPressed`），供前端 recorder 提交 Fn 绑定。
    recording_active: AtomicBool,
    /// 录制态 Fn 是否已按住，用于在 FLAGS_CHANGED 上识别 down 边沿（去重）。
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
    /// `cancel_tx`：Esc 按下即发一个 `()`。独立于 `tx`，见模块注释——不能与
    /// Pressed/Released 挤同一条串行 bridge，否则 Processing 期间取消排不上队。
    /// `combo_tx`：触发键按住期间叠加了普通键就发一个 press id。独立于 `tx`，见模块
    /// 注释——不能与 Pressed/Released 挤同一条串行 bridge，否则撤销要等
    /// `begin_session` 开完麦才排得上队，胶囊晚几百毫秒才消失。
    pub fn start(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<u64>,
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

    pub fn trigger_combined_since_press(&self, press_id: u64) -> bool {
        self.adapter.trigger_combined_since_press(press_id)
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

fn send_combo_abort_or_log(tx: &Sender<u64>, press_id: u64) {
    if let Err(e) = tx.send(press_id) {
        log::warn!("[hotkey] 组合键撤销事件发送失败: {e}");
    }
}

static NEXT_PRESS_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_press_id() -> u64 {
    NEXT_PRESS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// 会话激活期间（胶囊显示录音/转写/润色中）Esc 由 OpenLess 独占：tap/hook 吞掉
/// keydown 不透传宿主应用。否则一次 Esc 双重生效——既取消 OpenLess 会话，又触发
/// 宿主应用自己的 Esc 语义（如取消 Claude 正在生成的回复）。对照输入法的行为：
/// 组合窗激活时 Esc 只取消候选词、宿主应用收不到。keyup 不吞：宿主应用几乎都在
/// keydown 上响应 Esc，孤儿 keyup 无害，且窗口期内会话结束时吞 up 不吞 down 反而
/// 会造成不成对。由 coordinator 的 emit_capsule 在胶囊状态变化时更新。
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
    combo_tx: Sender<u64>,
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
            Sender<u64>,
            StartupTx<T>,
        ) + Send + 'static,
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
            // 绑定未变化（如 supervisor 每 5s 周期性重新应用同一绑定）：不要碰 held latch。
            // 否则会在长按期间把「已按住」清成 false，松手时 `!is_active && was_held` 不成立、
            // 不再发 Released —— hold 模式（Less Computer 按住说话）录音停不下来、要再按一次。
            // 复现：长按 >5s 跨过一次 supervisor 轮询即触发。
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

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;

    use super::{
        esc_exclusive, install_error, reset_shared_held_state, send_cancel_or_log,
        send_combo_abort_or_log, send_or_log,
        start_listener_thread, update_shared_binding, update_shared_modifier_shortcuts,
        HotkeyAdapter, HotkeyEvent, Shared, StartupTx,
    };
    use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyTrigger};

    pub fn start_adapter(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<u64>,
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

        fn trigger_combined_since_press(&self, press_id: u64) -> bool {
            self.shared
                .trigger_companion_seen
                .load(Ordering::SeqCst)
                == press_id
        }

        fn shutdown(&self) {
            // 顺序：先 disable tap 让 OS 不再向我们派发事件，然后 stop runloop
            // 让 listener 线程从 CFRunLoopRun() 返回退出。take() 保证幂等。
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
    const TAP_DISABLED_BY_TIMEOUT: CgEventType = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER_INPUT: CgEventType = 0xFFFF_FFFF;

    const KEYBOARD_EVENT_KEYCODE: CgEventField = 9;

    const FLAG_MASK_SHIFT: CgEventFlags = 0x0002_0000;
    const FLAG_MASK_CONTROL: CgEventFlags = 0x0004_0000;
    const FLAG_MASK_ALTERNATE: CgEventFlags = 0x0008_0000;
    const FLAG_MASK_COMMAND: CgEventFlags = 0x0010_0000;
    const FLAG_MASK_SECONDARY_FN: CgEventFlags = 0x0080_0000;

    const ESC_KEYCODE: i64 = 53;

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
        /// Esc 专用通道，见模块注释——不与 tx 挤同一条串行 bridge。
        cancel_tx: Sender<()>,
        /// 组合键撤销专用通道，见模块注释——不与 tx 挤同一条串行 bridge。
        combo_tx: Sender<u64>,
        /// 与 MacHotkeyAdapter 共享的 (tap, runloop) refs。tap re-enable on
        /// TAP_DISABLED_BY_TIMEOUT 走 handles.tap；adapter shutdown 也走这两个 lock。
        handles: Arc<MacShutdownHandles>,
    }

    unsafe impl Send for CallbackContext {}
    unsafe impl Sync for CallbackContext {}

    fn run_listen_loop(
        shared: Arc<Shared>,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<u64>,
        status_tx: StartupTx<Arc<MacShutdownHandles>>,
    ) {
        let mask: CgEventMask = (1u64 << FLAGS_CHANGED)
            | (1u64 << KEY_DOWN)
            | (1u64 << KEY_UP);
        let handles = Arc::new(MacShutdownHandles {
            tap: std::sync::Mutex::new(None),
            runloop: std::sync::Mutex::new(None),
        });
        let context = Box::into_raw(Box::new(CallbackContext {
            shared,
            tx,
            cancel_tx,
            combo_tx,
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
            // CFRunLoopRun 阻塞直到 CFRunLoopStop 被调用（由 MacHotkeyAdapter::shutdown
            // 触发）。返回后 listener 线程清理 context 并自然退出。
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
                // 会话激活期间独占消费 Esc：返回 null 删除事件（active tap），宿主应用
                // 收不到，避免「取消会话」与宿主 Esc 语义双重生效。见 esc_exclusive 注释。
                if keycode == ESC_KEYCODE && esc_exclusive() {
                    return std::ptr::null_mut();
                }
            }
            KEY_UP => {
                let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
                crate::side_aware_combo::platform::dispatch_keycode(keycode, false, 0, false);
            }
            _ => {}
        }
        event
    }

    fn handle_flags_changed(ctx: &CallbackContext, event: CgEventRef) {
        let flags = unsafe { CGEventGetFlags(event) };

        // 录制态：浏览器不向网页层下发 Fn keydown（系统修饰键被隐藏），无法通过
        // ShortcutRecorder 捕获。这里用 CGEventTap 检测 Fn 按下边沿（keycode 63 +
        // kCGEventFlagMaskSecondaryFn），上报 `FnRecordingPressed`，供上层转给前端
        // recorder 提交 Fn 绑定。
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
        }

        // Shift 是翻译模式修饰键 — 与触发键的 keycode 检查独立，任何时刻按 Shift 都生效。
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

        let trigger = ctx.shared.binding.read().trigger;
        if trigger == HotkeyTrigger::Custom {
            return;
        }
        let expected_keycode = trigger_to_keycode(trigger);
        if keycode != expected_keycode {
            return;
        }
        let mask = trigger_to_flag_mask(trigger);
        let is_active = (flags & mask) != 0;
        let was_held = ctx.shared.trigger_held.load(Ordering::SeqCst);

        if is_active && !was_held {
            ctx.shared.trigger_held.store(true, Ordering::SeqCst);
            let press_id = super::next_press_id();
            ctx.shared
                .trigger_press_id
                .store(press_id, Ordering::SeqCst);
            ctx.shared
                .trigger_companion_seen
                .store(0, Ordering::SeqCst);
            send_or_log(
                &ctx.tx,
                HotkeyEvent::Pressed {
                    at: std::time::Instant::now(),
                    press_id,
                },
            );
        } else if !is_active && was_held {
            ctx.shared.trigger_held.store(false, Ordering::SeqCst);
            send_or_log(&ctx.tx, HotkeyEvent::Released { at: std::time::Instant::now() });
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
        if keycode == ESC_KEYCODE {
            note_companion_key_down(ctx);
            send_cancel_or_log(&ctx.cancel_tx);
            return;
        }
        note_companion_key_down(ctx);
    }

    /// 触发键按住期间按下任意普通键 = 用户在打组合键（Option+任意字母/数字键、Option+Tab…），
    /// 不是想说话 —— 往组合键撤销通道发一次让上层撤销这次按下。
    ///
    /// 走 `combo_tx` 而不是 `tx`：撤销必须在按下 Q 的那一帧就生效（胶囊立刻消失），
    /// 而 `tx` 那头的 bridge 此刻多半正卡在这次按下自己的 `begin_session` 里。见模块注释。
    ///
    /// macOS 的修饰键走 FLAGS_CHANGED、不会进 KEY_DOWN，所以叠加 Shift（翻译修饰键）
    /// 或 Cmd 不会被误判成组合键；只有真正的字符 / 功能键才算。OS 自动重复与「按住
    /// 触发键连按多个键」由 companion latch 收敛成一次。
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
            mpsc::Receiver<u64>,
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
                    handles: Arc::new(MacShutdownHandles {
                        tap: std::sync::Mutex::new(None),
                        runloop: std::sync::Mutex::new(None),
                    }),
                },
                rx,
                combo_rx,
            )
        }

        /// 不关心组合键撤销的用例：撤销通道的接收端就地丢弃（这些用例不会往它发东西）。
        fn callback_context(shared: Arc<Shared>) -> (CallbackContext, mpsc::Receiver<HotkeyEvent>) {
            let (ctx, rx, _combo_rx) = callback_context_with_combo(shared);
            (ctx, rx)
        }

        fn drain_combo(rx: &mpsc::Receiver<u64>) -> usize {
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

        // Option+任意字母/数字键这类组合键：按住期间的普通键按下只撤销一次，
        // 且必须真的按住了触发键。
        #[test]
        fn mac_companion_key_down_aborts_trigger_once_per_hold() {
            let shared = shared(HotkeyTrigger::LeftOption);
            let (ctx, rx, combo_rx) = callback_context_with_combo(Arc::clone(&shared));

            // 没按住触发键时的普通打字：与听写无关，不发事件。
            note_companion_key_down(&ctx);
            assert_eq!(drain_combo(&combo_rx), 0);

            shared
                .trigger_press_id
                .store(1, Ordering::SeqCst);
            shared.trigger_held.store(true, Ordering::SeqCst);
            // OS 自动重复 / 按住触发键连按多个键，都只撤销一次。
            note_companion_key_down(&ctx);
            note_companion_key_down(&ctx);
            assert_eq!(drain_combo(&combo_rx), 1);

            // 下一次 Pressed 边沿会重置 latch（handle_flags_changed 里做），下一轮组合键
            // 才能再次撤销 —— 否则第二次组合键会被当成正常听写。
            shared
                .trigger_companion_seen
                .store(0, Ordering::SeqCst);
            note_companion_key_down(&ctx);
            assert_eq!(drain_combo(&combo_rx), 1);

            // 撤销全程不碰 Pressed/Released 那条串行通道 —— 它此刻正卡在 begin_session 里。
            assert!(drain(&rx).is_empty());
        }
    }
}

// ─────────────────────────── Windows implementation ───────────────────────────

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::atomic::Ordering;
    use std::sync::atomic::{AtomicPtr, Ordering as AtomicOrdering};
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
        send_combo_abort_or_log, send_or_log,
        start_listener_thread, update_shared_binding, update_shared_modifier_shortcuts,
        HotkeyAdapter, HotkeyEvent, Shared, StartupTx,
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
    static HOOK_CONTEXT: AtomicPtr<CallbackContext> = AtomicPtr::new(std::ptr::null_mut());

    pub fn start_adapter(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<u64>,
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

        fn trigger_combined_since_press(&self, press_id: u64) -> bool {
            self.shared
                .trigger_companion_seen
                .load(Ordering::SeqCst)
                == press_id
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
        /// Esc 专用通道，见模块注释——不与 tx 挤同一条串行 bridge。
        cancel_tx: Sender<()>,
        /// 组合键撤销专用通道，见模块注释——不与 tx 挤同一条串行 bridge。
        combo_tx: Sender<u64>,
        hook: std::sync::Mutex<Option<HHOOK>>,
    }

    unsafe impl Send for CallbackContext {}
    unsafe impl Sync for CallbackContext {}

    fn run_listen_loop(
        shared: Arc<Shared>,
        tx: Sender<HotkeyEvent>,
        cancel_tx: Sender<()>,
        combo_tx: Sender<u64>,
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
        HOOK_CONTEXT.store(context, AtomicOrdering::SeqCst);

        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0);
            match hook {
                Ok(hook) => {
                    *(*context).hook.lock().unwrap() = Some(hook);
                    log::info!("[hotkey] Windows low-level keyboard hook 已启动");
                    let _ = status_tx.send(Ok(thread_id));
                }
                Err(err) => {
                    HOOK_CONTEXT.store(std::ptr::null_mut(), AtomicOrdering::SeqCst);
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
            // 监听线程可能在触发键仍处于按下状态时退出（配置重载、应用关闭或
            // hook 消息循环异常结束）。先清理内部锁存，避免下一次监听器复用
            // 共享状态时把旧的按下状态带过去。
            super::reset_shared_held_state(&(*context).shared);
            HOOK_CONTEXT.store(std::ptr::null_mut(), AtomicOrdering::SeqCst);
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
                // 合成输入（SendInput/keybd_event）与真实键盘统一走同一条分发路径。
                // 只要事件的虚拟键值匹配当前配置，现有的边沿去重和组合键撤销逻辑
                // 仍然负责决定是否触发 OpenLess；这里不再按合成输入来源过滤。
                if dispatch_keyboard_event(ctx, keyboard.vkCode, wparam.0) {
                    return LRESULT(1);
                }
            }
        }

        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe fn callback_context<'a>() -> Option<&'a CallbackContext> {
        let ptr = HOOK_CONTEXT.load(AtomicOrdering::SeqCst);
        if ptr.is_null() {
            None
        } else {
            Some(&*ptr)
        }
    }

    fn dispatch_keyboard_event(ctx: &CallbackContext, vk_code: u32, message: usize) -> bool {
        let pressed = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        if vk_code == VK_ESCAPE && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) {
            note_companion_key_down(ctx);
            send_cancel_or_log(&ctx.cancel_tx);
            // 会话激活期间独占消费 Esc（返回 true → LRESULT(1) 吞掉），宿主应用收不到，
            // 避免「取消会话」与宿主 Esc 语义双重生效。见 esc_exclusive 注释。
            return esc_exclusive();
        }

        crate::side_aware_combo::platform::dispatch_vk(vk_code, pressed);

        if pressed && !is_modifier_vk(vk_code) {
            note_companion_key_down(ctx);
        }

        // Shift（任一侧）= 翻译模式修饰键。在录音过程中任意时刻按下都生效。详见 issue #4。
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
            return false;
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
                    ctx.shared
                        .trigger_companion_seen
                        .store(0, Ordering::SeqCst);
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
                    log::info!("[hotkey] Windows trigger released vk={vk_code}");
                    send_or_log(&ctx.tx, HotkeyEvent::Released { at: std::time::Instant::now() });
                }
            }
            _ => {}
        }
        true
    }

    /// 触发键按住期间按下任意非修饰键 = 用户在打组合键（Alt+Tab / Alt+F4…），不是想
    /// 说话 —— 往组合键撤销通道发一次让上层撤销这次按下。走 `combo_tx` 而不是 `tx` 的
    /// 理由见模块注释（`tx` 那头此刻多半正卡在这次按下自己的 begin_session 里）。
    /// 修饰键本身不算「其他键」，与 macOS 侧（修饰键走 FLAGS_CHANGED、不进 KEY_DOWN）
    /// 保持同一语义：叠加 Shift（翻译修饰键）或 Ctrl 不会撤销听写。
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
        // Windows 低层 hook 能区分左右 Alt，LeftOption / RightOption 必须保留物理侧。
        // 其他少量跨平台别名仍按 Windows 可用物理键折叠：
        // - Fn 复用 RightControl / VK_RCONTROL
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
            mpsc::Receiver<u64>,
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

        /// 不关心组合键撤销的用例：撤销通道的接收端就地丢弃（这些用例不会往它发东西）。
        fn callback_context(shared: Arc<Shared>) -> (CallbackContext, mpsc::Receiver<HotkeyEvent>) {
            let (ctx, rx, _combo_abort_rx) = callback_context_with_combo(shared);
            (ctx, rx)
        }

        fn drain_combo(rx: &mpsc::Receiver<u64>) -> usize {
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
        fn windows_modifier_edges_are_deduped_from_mock_hook_events() {
            let shared = shared(HotkeyTrigger::RightControl);
            let (ctx, rx) = callback_context(shared);

            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP));
            assert!(dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP));

            assert_eq!(
                edge_names(drain(&rx)),
                vec!["pressed", "released"]
            );
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
            assert_eq!(
                edge_names(drain(&left_rx)),
                vec!["pressed", "released"]
            );

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

        // Alt+任意字母/数字键这类组合键：普通键按下撤销本次触发；
        // 修饰键叠加（Shift = 翻译模式）不算。
        #[test]
        fn windows_companion_key_down_aborts_trigger_but_modifiers_do_not() {
            let shared = shared(HotkeyTrigger::LeftOption);
            let (ctx, _rx, combo_abort_rx) = callback_context_with_combo(shared);

            dispatch_keyboard_event(&ctx, VK_LMENU, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, VK_LSHIFT, WM_KEYDOWN);
            assert_eq!(drain_combo(&combo_abort_rx), 0);

            dispatch_keyboard_event(&ctx, 0x41, WM_KEYDOWN); // A
            dispatch_keyboard_event(&ctx, 0x41, WM_KEYDOWN); // OS 自动重复
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
            use crate::combo_hotkey::ComboHotkeyEvent;
            use crate::side_aware_combo::SideAwareComboMonitor;
            use crate::types::ShortcutBinding;

            let (combo_tx, combo_rx) = mpsc::channel();
            let binding = ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["shift-left".into()],
            };
            let monitor = SideAwareComboMonitor::start(binding, combo_tx).expect("start monitor");

            let shared = shared(HotkeyTrigger::Custom);
            let (ctx, hotkey_rx) = callback_context(shared);

            dispatch_keyboard_event(&ctx, VK_LSHIFT, WM_KEYDOWN);
            dispatch_keyboard_event(&ctx, 0x44, WM_KEYDOWN);

            assert!(matches!(combo_rx.recv().unwrap(), ComboHotkeyEvent::Pressed { .. }));
            assert!(
                hotkey_rx
                    .try_iter()
                    .any(|evt| evt == HotkeyEvent::TranslationModifierPressed)
            );

            drop(monitor);
        }
    }
}

// ─────────────────────────── Linux / other implementation ───────────────────────────

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod platform {
    use std::sync::mpsc::Sender;

    use super::{HotkeyAdapter, HotkeyEvent};
    use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyTrigger};

    /// Linux 统一使用 fcitx5 插件作为热键源（Wayland / X11 均可）。
    ///
    /// 实际的热键事件由 `linux_fcitx::start_dictation_signal_listener` 接收
    /// fcitx5 插件的 DBus 信号并转发到 `Sender<HotkeyEvent>`。
    pub fn start_adapter(
        _binding: HotkeyBinding,
        _tx: Sender<HotkeyEvent>,
        _cancel_tx: Sender<()>,
        _combo_tx: Sender<u64>,
    ) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
        log::info!("[hotkey] Linux — fcitx5 plugin handles hotkeys");
        Ok(Box::new(PlaceholderAdapter {
            _tx,
            _cancel_tx,
            _combo_tx,
        }))
    }

    /// Linux 占位 adapter：实现接口但不监听键盘。
    /// 热键事件由 fcitx5 插件的 `DictationKeyEvent` DBus 信号提供。
    /// 组合键撤销由 fcitx5 插件通过 `DictationKeyCombined` 信号上报。
    struct PlaceholderAdapter {
        _tx: Sender<HotkeyEvent>,
        _cancel_tx: Sender<()>,
        _combo_tx: Sender<u64>,
    }

    impl HotkeyAdapter for PlaceholderAdapter {
        fn kind(&self) -> HotkeyAdapterKind {
            HotkeyAdapterKind::Fcitx5
        }

        fn update_binding(&self, _binding: HotkeyBinding) {
            // fcitx5 插件热键由 sync_binding_to_plugin 单独同步。
        }

        fn update_modifier_shortcuts(
            &self,
            qa_trigger: Option<HotkeyTrigger>,
            selection_polish_trigger: Option<HotkeyTrigger>,
            translation_trigger: Option<HotkeyTrigger>,
        ) {
            crate::linux_fcitx::sync_qa_binding(qa_trigger);
            // 选区润色触发键：fcitx5 插件通过 SelectionPolishEvent 信号回传
            //（插件端需 `scripts/inject-fcitx5-plugin.sh` 重装新版 .so）。
            crate::linux_fcitx::sync_selection_polish_binding(selection_polish_trigger);
            crate::linux_fcitx::sync_translation_binding(translation_trigger);
        }

        fn reset_held_state(&self) {}
    }
}
