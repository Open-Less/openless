//! 全局热键监听：发送按下 / 抬起 / 取消三类边沿事件。
//!
//! - macOS：原生 CGEventTap（core-foundation + core-graphics FFI），与 Swift
//!   `OpenLessHotkey/HotkeyMonitor.swift` 同源。**不能用 `rdev`**：rdev 在每个
//!   事件回调里同步调 `TSMGetInputSourceProperty`，macOS 14+ 强制断言主线程，
//!   非主线程触发 `dispatch_assert_queue_fail` → SIGTRAP abort（已踩坑）。
//! - Windows：原生 `WH_KEYBOARD_LL` low-level keyboard hook，保留 modifier-only
//!   trigger（如右 Control / 右 Alt）的真实语义，不再把平台能力藏在 `rdev` 抽象里。
//! - Linux / 其他：继续 best-effort 走 `rdev::listen`。
//!
//! 仅产出"边沿"事件，toggle vs hold 由 Coordinator 解释。

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError};

#[derive(Clone, Copy, Debug)]
pub enum HotkeyEvent {
    Pressed,
    Released,
    Cancelled,
}

pub trait HotkeyAdapter: Send + Sync {
    fn kind(&self) -> HotkeyAdapterKind;
    fn update_binding(&self, binding: HotkeyBinding);
    fn shutdown(&self) {}
}

struct Shared {
    binding: RwLock<HotkeyBinding>,
    /// 触发键当前是否处于"按住"状态。OS 自动重复事件用此去重。
    trigger_held: AtomicBool,
}

pub struct HotkeyMonitor {
    adapter: Box<dyn HotkeyAdapter>,
}

impl HotkeyMonitor {
    /// Spawn the listener thread and **wait synchronously** for it to confirm
    /// the OS-level hook installed so the caller can surface an actual adapter
    /// status instead of silently dropping events.
    pub fn start(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
    ) -> Result<Self, HotkeyInstallError> {
        Ok(Self {
            adapter: platform::start_adapter(binding, tx)?,
        })
    }

    pub fn update_binding(&self, binding: HotkeyBinding) {
        self.adapter.update_binding(binding);
    }

    pub fn kind(&self) -> HotkeyAdapterKind {
        self.adapter.kind()
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

type StartupTx<T> = mpsc::Sender<Result<T, HotkeyInstallError>>;

struct ListenerThread<T> {
    shared: Arc<Shared>,
    startup: T,
}

fn start_listener_thread<T, F>(
    binding: HotkeyBinding,
    tx: Sender<HotkeyEvent>,
    thread_name: &str,
    startup_timeout_message: &'static str,
    run_listen_loop: F,
) -> Result<ListenerThread<T>, HotkeyInstallError>
where
    T: Send + 'static,
    F: FnOnce(Arc<Shared>, Sender<HotkeyEvent>, StartupTx<T>) + Send + 'static,
{
    let shared = Arc::new(Shared {
        binding: RwLock::new(binding),
        trigger_held: AtomicBool::new(false),
    });

    let thread_shared = Arc::clone(&shared);
    let (status_tx, status_rx) = mpsc::channel::<Result<T, HotkeyInstallError>>();
    std::thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || run_listen_loop(thread_shared, tx, status_tx))
        .map_err(|e| install_error("spawn_failed", format!("hotkey 线程启动失败: {e}")))?;

    match status_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(startup)) => Ok(ListenerThread { shared, startup }),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(install_error("startup_timeout", startup_timeout_message)),
    }
}

fn update_shared_binding(shared: &Shared, binding: HotkeyBinding) {
    *shared.binding.write() = binding;
    shared
        .trigger_held
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
        install_error, send_or_log, start_listener_thread, update_shared_binding, HotkeyAdapter,
        HotkeyEvent, Shared, StartupTx,
    };
    use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyTrigger};

    pub fn start_adapter(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
    ) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
        let listener = start_listener_thread(
            binding,
            tx,
            "openless-hotkey-mac-event-tap",
            "hotkey hook 启动超时",
            run_listen_loop,
        )?;
        let _ = listener.startup;
        Ok(Box::new(MacHotkeyAdapter {
            shared: listener.shared,
        }))
    }

    struct MacHotkeyAdapter {
        shared: Arc<Shared>,
    }

    impl HotkeyAdapter for MacHotkeyAdapter {
        fn kind(&self) -> HotkeyAdapterKind {
            HotkeyAdapterKind::MacEventTap
        }

        fn update_binding(&self, binding: HotkeyBinding) {
            update_shared_binding(&self.shared, binding);
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
    const FLAGS_CHANGED: CgEventType = 12;
    const TAP_DISABLED_BY_TIMEOUT: CgEventType = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER_INPUT: CgEventType = 0xFFFF_FFFF;

    const KEYBOARD_EVENT_KEYCODE: CgEventField = 9;

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
        static kCFRunLoopCommonModes: CfStringRef;
    }

    struct CallbackContext {
        shared: Arc<Shared>,
        tx: Sender<HotkeyEvent>,
        tap: std::sync::Mutex<Option<CfMachPortRef>>,
    }

    unsafe impl Send for CallbackContext {}
    unsafe impl Sync for CallbackContext {}

    fn run_listen_loop(shared: Arc<Shared>, tx: Sender<HotkeyEvent>, status_tx: StartupTx<()>) {
        let mask: CgEventMask = (1u64 << FLAGS_CHANGED) | (1u64 << KEY_DOWN);
        let context = Box::into_raw(Box::new(CallbackContext {
            shared,
            tx,
            tap: std::sync::Mutex::new(None),
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
            *(*context).tap.lock().unwrap() = Some(tap);

            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            let runloop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(runloop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);

            log::info!("[hotkey] CGEventTap 已启动");
            let _ = status_tx.send(Ok(()));
            CFRunLoopRun();
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
                if let Some(tap) = *ctx.tap.lock().unwrap() {
                    unsafe { CGEventTapEnable(tap, true) };
                }
                return event;
            }
            FLAGS_CHANGED => handle_flags_changed(ctx, event),
            KEY_DOWN => handle_key_down(ctx, event),
            _ => {}
        }
        event
    }

    fn handle_flags_changed(ctx: &CallbackContext, event: CgEventRef) {
        let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
        let trigger = ctx.shared.binding.read().trigger;
        let expected_keycode = trigger_to_keycode(trigger);
        if keycode != expected_keycode {
            return;
        }
        let flags = unsafe { CGEventGetFlags(event) };
        let mask = trigger_to_flag_mask(trigger);
        let is_active = (flags & mask) != 0;
        let was_held = ctx.shared.trigger_held.load(Ordering::SeqCst);

        if is_active && !was_held {
            ctx.shared.trigger_held.store(true, Ordering::SeqCst);
            send_or_log(&ctx.tx, HotkeyEvent::Pressed);
        } else if !is_active && was_held {
            ctx.shared.trigger_held.store(false, Ordering::SeqCst);
            send_or_log(&ctx.tx, HotkeyEvent::Released);
        }
    }

    fn handle_key_down(ctx: &CallbackContext, event: CgEventRef) {
        let keycode = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
        if keycode == ESC_KEYCODE {
            send_or_log(&ctx.tx, HotkeyEvent::Cancelled);
        }
    }

    fn trigger_to_keycode(trigger: HotkeyTrigger) -> i64 {
        match trigger {
            HotkeyTrigger::LeftControl => 59,
            HotkeyTrigger::RightControl => 62,
            HotkeyTrigger::LeftOption => 58,
            HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => 61,
            HotkeyTrigger::RightCommand => 54,
            HotkeyTrigger::Fn => 63,
        }
    }

    fn trigger_to_flag_mask(trigger: HotkeyTrigger) -> CgEventFlags {
        match trigger {
            HotkeyTrigger::LeftControl | HotkeyTrigger::RightControl => FLAG_MASK_CONTROL,
            HotkeyTrigger::RightCommand => FLAG_MASK_COMMAND,
            HotkeyTrigger::LeftOption | HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => {
                FLAG_MASK_ALTERNATE
            }
            HotkeyTrigger::Fn => FLAG_MASK_SECONDARY_FN,
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
        install_error, send_or_log, start_listener_thread, update_shared_binding, HotkeyAdapter,
        HotkeyEvent, Shared, StartupTx,
    };
    use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyTrigger};

    const WM_KEYDOWN: usize = 0x0100;
    const WM_KEYUP: usize = 0x0101;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const WM_SYSKEYUP: usize = 0x0105;

    const VK_ESCAPE: u32 = 0x1B;
    const VK_LCONTROL: u32 = 0xA2;
    const VK_RCONTROL: u32 = 0xA3;
    const VK_RMENU: u32 = 0xA5;
    const VK_RWIN: u32 = 0x5C;
    const LLKHF_INJECTED: u32 = 0x0000_0010;

    static HOOK_CONTEXT: AtomicPtr<CallbackContext> = AtomicPtr::new(std::ptr::null_mut());

    pub fn start_adapter(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
    ) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
        let listener = start_listener_thread(
            binding,
            tx,
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
        hook: std::sync::Mutex<Option<HHOOK>>,
    }

    unsafe impl Send for CallbackContext {}
    unsafe impl Sync for CallbackContext {}

    fn run_listen_loop(shared: Arc<Shared>, tx: Sender<HotkeyEvent>, status_tx: StartupTx<u32>) {
        let thread_id = unsafe { GetCurrentThreadId() };
        let context = Box::into_raw(Box::new(CallbackContext {
            shared,
            tx,
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
                if keyboard.flags.0 & LLKHF_INJECTED == 0 {
                    dispatch_keyboard_event(ctx, keyboard.vkCode, wparam.0);
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

    fn dispatch_keyboard_event(ctx: &CallbackContext, vk_code: u32, message: usize) {
        if vk_code == VK_ESCAPE && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN) {
            send_or_log(&ctx.tx, HotkeyEvent::Cancelled);
            return;
        }

        let trigger = ctx.shared.binding.read().trigger;
        if vk_code != trigger_to_vk_code(trigger) {
            return;
        }

        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let was_held = ctx.shared.trigger_held.swap(true, Ordering::SeqCst);
                if !was_held {
                    send_or_log(&ctx.tx, HotkeyEvent::Pressed);
                }
            }
            WM_KEYUP | WM_SYSKEYUP => {
                let was_held = ctx.shared.trigger_held.swap(false, Ordering::SeqCst);
                if was_held {
                    send_or_log(&ctx.tx, HotkeyEvent::Released);
                }
            }
            _ => {}
        }
    }

    fn trigger_to_vk_code(trigger: HotkeyTrigger) -> u32 {
        // Windows only gives us a small set of modifier virtual keys that can be
        // used as reliable modifier-only global triggers, so the cross-platform
        // trigger list intentionally collapses a few aliases onto the same
        // physical Windows key:
        // - LeftOption reuses RightAlt / VK_RMENU
        // - Fn reuses RightControl / VK_RCONTROL
        match trigger {
            HotkeyTrigger::RightControl => VK_RCONTROL,
            HotkeyTrigger::LeftControl => VK_LCONTROL,
            HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => VK_RMENU,
            HotkeyTrigger::RightCommand => VK_RWIN,
            HotkeyTrigger::LeftOption => VK_RMENU,
            HotkeyTrigger::Fn => VK_RCONTROL,
        }
    }
}

// ─────────────────────────── Linux / other implementation ───────────────────────────

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use std::time::Duration;

    use rdev::{listen, Event, EventType, Key};

    use super::{
        install_error, start_listener_thread, update_shared_binding, HotkeyAdapter, HotkeyEvent,
        Shared, StartupTx,
    };
    use crate::types::{HotkeyAdapterKind, HotkeyBinding, HotkeyInstallError, HotkeyTrigger};

    pub fn start_adapter(
        binding: HotkeyBinding,
        tx: Sender<HotkeyEvent>,
    ) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
        let listener = start_listener_thread(
            binding,
            tx,
            "openless-hotkey-rdev",
            "hotkey hook 启动超时",
            run_listen_loop,
        )?;
        let _ = listener.startup;
        Ok(Box::new(RdevHotkeyAdapter {
            shared: listener.shared,
        }))
    }

    struct RdevHotkeyAdapter {
        shared: Arc<Shared>,
    }

    impl HotkeyAdapter for RdevHotkeyAdapter {
        fn kind(&self) -> HotkeyAdapterKind {
            HotkeyAdapterKind::Rdev
        }

        fn update_binding(&self, binding: HotkeyBinding) {
            update_shared_binding(&self.shared, binding);
        }
    }

    fn run_listen_loop(shared: Arc<Shared>, tx: Sender<HotkeyEvent>, status_tx: StartupTx<()>) {
        let status_sent = Arc::new(AtomicBool::new(false));
        let ready_status_sent = Arc::clone(&status_sent);
        let ready_status_tx = status_tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(350));
            if !ready_status_sent.swap(true, Ordering::SeqCst) {
                let _ = ready_status_tx.send(Ok(()));
            }
        });
        let cb_shared = Arc::clone(&shared);
        let result = listen(move |event: Event| {
            dispatch_event(&cb_shared, &tx, event);
        });
        if let Err(err) = result {
            if !status_sent.swap(true, Ordering::SeqCst) {
                let _ = status_tx.send(Err(install_error(
                    "listen_failed",
                    format!("rdev::listen 启动失败: {err:?}"),
                )));
            }
            log::error!("[hotkey] rdev::listen 启动失败: {:?}", err);
        }
    }

    fn dispatch_event(shared: &Shared, tx: &Sender<HotkeyEvent>, event: Event) {
        let trigger = shared.binding.read().trigger;
        match event.event_type {
            EventType::KeyPress(key) => {
                if key == Key::Escape {
                    let _ = tx.send(HotkeyEvent::Cancelled);
                    return;
                }
                if key == trigger_to_rdev_key(trigger) {
                    let was_held = shared.trigger_held.swap(true, Ordering::SeqCst);
                    if !was_held {
                        let _ = tx.send(HotkeyEvent::Pressed);
                    }
                }
            }
            EventType::KeyRelease(key) => {
                if key == trigger_to_rdev_key(trigger) {
                    let was_held = shared.trigger_held.swap(false, Ordering::SeqCst);
                    if was_held {
                        let _ = tx.send(HotkeyEvent::Released);
                    }
                }
            }
            _ => {}
        }
    }

    fn trigger_to_rdev_key(trigger: HotkeyTrigger) -> Key {
        match trigger {
            HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => Key::AltGr,
            HotkeyTrigger::LeftOption => Key::Alt,
            HotkeyTrigger::RightControl => Key::ControlRight,
            HotkeyTrigger::LeftControl => Key::ControlLeft,
            HotkeyTrigger::RightCommand => Key::MetaRight,
            HotkeyTrigger::Fn => Key::Function,
        }
    }
}
