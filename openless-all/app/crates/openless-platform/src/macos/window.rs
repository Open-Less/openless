//! macOS 窗口焦点操作。
//!
//! AppKit 的 `makeKeyAndOrderFront:` 必须在主线程调用；直接调用时若 AppKit 抛异常
//! 会穿过 Rust 栈导致进程崩溃。这里统一用 `objc2::exception::catch` 兜住。

use std::ffi::c_void;

/// 把窗口置前并设为 key window。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用发生在主线程。
pub unsafe fn make_key_and_order_front(window: *mut c_void) -> Result<(), String> {
    use objc2::runtime::AnyObject;

    if window.is_null() {
        return Err("null NSWindow".to_string());
    }
    let ns = window as *mut AnyObject;
    // SAFETY: 闭包只做一次无返回值的 ObjC 消息发送，不运行 Rust 析构；
    // 即便 AppKit 抛异常，catch 也会在闭包帧边界内接住。
    let result = unsafe {
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let nil: *mut AnyObject = std::ptr::null_mut();
            let _: () = objc2::msg_send![ns, makeKeyAndOrderFront: nil];
        }))
    };
    result.map_err(|error| format!("{error:?}"))
}

/// 把 OpenLess 自己激活到前台（`NSApplication activateIgnoringOtherApps`）。
///
/// # Safety
/// AppKit 要求在主线程调用；宿主经 `run_on_main_thread` 派发。
pub unsafe fn activate_app() {
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    let Some(cls) = AnyClass::get("NSApplication") else {
        return;
    };
    let ns_app: *mut AnyObject = unsafe { objc2::msg_send![cls, sharedApplication] };
    if ns_app.is_null() {
        return;
    }
    let _: () = unsafe { objc2::msg_send![ns_app, activateIgnoringOtherApps: Bool::YES] };
}

/// 让窗口可以通过拖动背景移动（`setMovableByWindowBackground` + `setMovable`）。
///
/// AppKit 在 drag margin 不一致时会抛 `NSException`；不接住会穿过 Rust 栈直接
/// abort。这里统一 catch：失败只记日志，拖动失效但进程存活。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用在主线程。
pub unsafe fn make_draggable(window: *mut c_void) -> Result<(), String> {
    use objc2::runtime::{AnyObject, Bool};

    if window.is_null() {
        return Err("null NSWindow".to_string());
    }
    let ns = window as *mut AnyObject;
    let result = unsafe {
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let _: () = objc2::msg_send![ns, setMovableByWindowBackground: Bool::YES];
            let _: () = objc2::msg_send![ns, setMovable: Bool::YES];
        }))
    };
    result.map_err(|error| format!("{error:?}"))
}

/// 显示窗口但**不抢焦点**（`orderFrontRegardless`）。
///
/// Spotlight / Raycast 式行为：窗口可见但不成为 key window，调用方的应用仍在前台，
/// 随后的 AX 读取或 Cmd+C 才能作用在"用户原来的应用"上。
///
/// `level` 给 `Some` 时同时设置窗口层级（如胶囊用 25 = floating 之上）。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用在主线程。
pub unsafe fn show_without_activating(window: *mut c_void, level: Option<i64>) -> Result<(), String> {
    use objc2::runtime::AnyObject;

    if window.is_null() {
        return Err("null NSWindow".to_string());
    }
    let ns = window as *mut AnyObject;
    unsafe {
        if let Some(level) = level {
            let _: () = objc2::msg_send![ns, setLevel: level];
        }
        let _: () = objc2::msg_send![ns, orderFrontRegardless];
    }
    Ok(())
}

/// 给面板加 `NSWindowStyleMaskNonactivatingPanel` 位，使其点击时不激活 App。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用在主线程。
pub unsafe fn set_nonactivating_panel_style(window: *mut c_void) {
    const NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL: usize = 1 << 7;

    if window.is_null() {
        return;
    }
    let ns = window as *mut objc2::runtime::AnyObject;
    let current: usize = unsafe { objc2::msg_send![ns, styleMask] };
    let _: () = unsafe {
        objc2::msg_send![ns, setStyleMask: current | NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL]
    };
}

/// 设置 `collectionBehavior`（跨 Space / 全屏辅助窗口等位组合）。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用在主线程。
pub unsafe fn set_collection_behavior(window: *mut c_void, behavior: usize) {
    if window.is_null() {
        return;
    }
    let ns = window as *mut objc2::runtime::AnyObject;
    let _: () = unsafe { objc2::msg_send![ns, setCollectionBehavior: behavior] };
}

/// 读取当前 `collectionBehavior`，用于漂移检测。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用在主线程。
pub unsafe fn collection_behavior(window: *mut c_void) -> usize {
    if window.is_null() {
        return 0;
    }
    let ns = window as *mut objc2::runtime::AnyObject;
    unsafe { objc2::msg_send![ns, collectionBehavior] }
}

/// 应用当前是否在前台（`NSApplication isActive`）。
///
/// # Safety
/// `window` 无需参数；调用必须在主线程（AppKit 要求）。
pub unsafe fn app_is_active() -> bool {
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    let Some(cls) = AnyClass::get("NSApplication") else {
        return false;
    };
    let ns_app: *mut AnyObject = unsafe { objc2::msg_send![cls, sharedApplication] };
    if ns_app.is_null() {
        return false;
    }
    let active: Bool = unsafe { objc2::msg_send![ns_app, isActive] };
    active.as_bool()
}

/// 只把窗口设为 key window，不激活 App（`makeKeyWindow`）。
///
/// # Safety
/// `window` 必须是有效的 `NSWindow*`，且调用在主线程。
pub unsafe fn make_key_window(window: *mut c_void) {
    if window.is_null() {
        return;
    }
    let ns = window as *mut objc2::runtime::AnyObject;
    let _: () = unsafe { objc2::msg_send![ns, makeKeyWindow] };
}
