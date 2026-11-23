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
