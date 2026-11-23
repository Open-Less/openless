use std::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    MSG, WH_KEYBOARD_LL, WM_QUIT,
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
            let vk_code = crate::input::side_combo::normalize_windows_vk(
                keyboard.vkCode,
                keyboard.scanCode,
                keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
            );
            if dispatch_keyboard_event(ctx, vk_code, wparam.0) {
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
    if vk_code == VK_ESCAPE && super::capsule_menu_open() { return false; }
    if vk_code == VK_ESCAPE && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) {
        crate::input::side_combo::handle_companion_key_down();
        note_companion_key_down(ctx);
        send_cancel_or_log(&ctx.cancel_tx);
        // While a session is active, consume Esc exclusively (return true → LRESULT(1)
        // swallows it) so the host app never sees it, avoiding double effect between
        // "cancel session" and the host's Esc semantics. See esc_exclusive docs.
        return esc_exclusive();
    }

    if crate::input::side_combo::platform::dispatch_vk(vk_code, pressed) {
        if pressed {
            crate::input::side_combo::handle_companion_key_down();
            note_companion_key_down(ctx);
        }
        return true;
    }

    if pressed && !is_modifier_vk(vk_code) {
        crate::input::side_combo::handle_companion_key_down();
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
                        combined: ctx.shared.trigger_companion_seen.load(Ordering::SeqCst) == press_id,
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
                    super::prefetch_selection();
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
        use crate::input::hotkey::HotkeyMonitor;
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
    fn windows_release_carries_combo_verdict_even_when_abort_bridge_has_not_drained() {
        let (ctx, rx, abort_rx) = callback_context_with_combo(shared(HotkeyTrigger::RightControl));
        dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYDOWN);
        let press_id = match rx.recv().unwrap() {
            HotkeyEvent::Pressed { press_id, .. } => press_id,
            other => panic!("expected press, got {other:?}"),
        };
        dispatch_keyboard_event(&ctx, 0x41, WM_KEYDOWN);
        dispatch_keyboard_event(&ctx, VK_RCONTROL, WM_KEYUP);
        // Intentionally read Released before the independently queued Combined.
        assert!(matches!(rx.recv().unwrap(), HotkeyEvent::Released {
            press_id: released_id, combined: true, ..
        } if released_id == press_id));
        assert_eq!(abort_rx.recv().unwrap().press_id, press_id);
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
        use crate::input::side_combo::SideAwareComboMonitor;
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
        use crate::input::side_combo::SideAwareComboMonitor;
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
