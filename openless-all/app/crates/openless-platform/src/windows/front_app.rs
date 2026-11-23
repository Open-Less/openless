//! Windows 前台应用身份与窗口激活。
//!
//! Windows 没有 bundle id，"前台是谁"只能靠窗口标题；这既是 LLM 提示里的应用名，
//! 也是选区/粘贴目标校验的一部分。

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, IsIconic,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

/// 窗口标题（空标题返回 `None`）。
pub fn window_title(raw_hwnd: usize) -> Option<String> {
    if raw_hwnd == 0 {
        return None;
    }
    let hwnd = HWND(raw_hwnd as *mut std::ffi::c_void);
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        (!title.is_empty()).then_some(title)
    }
}

/// 前台应用标识 `(窗口标题, None)`。
///
/// Windows 没有 bundle id 概念，返回 `None` 而不是伪造一个。
pub fn current_front_app_parts() -> (Option<String>, Option<String>) {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return (None, None);
    }
    (window_title(hwnd.0 as usize), None)
}

/// 把指定窗口恢复到前台（必要时从最小化恢复）。
///
/// Windows 的前台抢占规则可能拒绝 `SetForegroundWindow`，所以返回值只代表"调用发出去了"；
/// 调用方必须重新捕获焦点指纹确认，不能仅凭这里的结果就粘贴。
pub fn activate_window(raw_hwnd: usize) -> bool {
    if raw_hwnd == 0 {
        return false;
    }
    let hwnd = HWND(raw_hwnd as *mut std::ffi::c_void);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_handles_are_rejected_without_panicking() {
        assert_eq!(window_title(0), None);
        assert!(!activate_window(0));
    }
}
