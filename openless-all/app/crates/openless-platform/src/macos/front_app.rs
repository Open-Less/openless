//! macOS 前台应用身份。
//!
//! 文本通道的安全门要回答两个问题：
//! 1. "现在最前面的是哪个应用"（AX 读取失败的兜底与日志）；
//! 2. "**这个 pid** 是哪个应用"（判断手里这个 AX 元素属于谁 —— 必须按元素自己的 pid 查，
//!    不能用前台应用顶替，否则切换应用的窗口期会把新应用的控件当成旧应用放行）。
//!
//! 这三个字段（name / bundle / pid）必须来自**同一个** `NSRunningApplication` 快照：
//! 分开查询可能在焦点切换时拼出两个应用的信息。

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

unsafe fn ns_string_to_rust(ns_string: *mut AnyObject) -> Option<String> {
    if ns_string.is_null() {
        return None;
    }
    let utf8: *const std::os::raw::c_char = unsafe { msg_send![ns_string, UTF8String] };
    if utf8.is_null() {
        return None;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(utf8) };
    let s = cstr.to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 一次 `NSWorkspace.frontmostApplication` 快照：(name, bundle, pid)。
pub fn current_front_app_snapshot() -> (Option<String>, Option<String>, Option<i32>) {
    unsafe {
        let Some(cls) = AnyClass::get("NSWorkspace") else {
            return (None, None, None);
        };
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return (None, None, None);
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return (None, None, None);
        }
        let name_obj: *mut AnyObject = msg_send![app, localizedName];
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        let pid: i32 = msg_send![app, processIdentifier];
        unsafe {
            (
                ns_string_to_rust(name_obj),
                ns_string_to_rust(bundle_obj),
                (pid > 0).then_some(pid),
            )
        }
    }
}

/// 前台应用的结构化身份 `(localizedName, bundleIdentifier)`。
pub fn current_front_app_parts() -> (Option<String>, Option<String>) {
    let (name, bundle, _) = current_front_app_snapshot();
    (name, bundle)
}

/// 前台应用 pid。
pub fn current_front_app_pid() -> Option<i32> {
    current_front_app_snapshot().2
}

/// **指定 pid** 的 bundle id（不是"当前前台是谁"）。
pub fn bundle_id_for_pid(pid: i32) -> Option<String> {
    unsafe {
        let cls = AnyClass::get("NSRunningApplication")?;
        let app: *mut AnyObject = msg_send![cls, runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return None;
        }
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        unsafe { ns_string_to_rust(bundle_obj) }
    }
}

/// 把指定 pid 的应用激活到前台（NSRunningApplication activate，忽略其它应用）。
///
/// 尽力而为：Electron/自绘窗口类应用可能需要更久才恢复 key window，调用方应轮询确认。
pub fn activate_app_by_pid(pid: i32) {
    unsafe {
        let Some(cls) = AnyClass::get("NSRunningApplication") else {
            return;
        };
        let app: *mut AnyObject = msg_send![cls, runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return;
        }
        let _: bool = msg_send![app, activateWithOptions: 1u64]; // IgnoringOtherApps
    }
}
