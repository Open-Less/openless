//! Windows 窗口表现原语（圆角区域、DWM 配色、按标题查找、顶层窗口隐藏）。
//!
//! 全部以裸 `HWND`（`usize`）为参数，宿主负责从 tauri 窗口对象取出句柄；
//! 平台层不依赖任何窗口框架。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, DeleteObject, SetWindowRgn, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClientRect, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    IsWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
};

use super::capsule_window::HitRect;

fn hwnd(raw: usize) -> Option<HWND> {
    (raw != 0).then_some(HWND(raw as *mut std::ffi::c_void))
}

/// 句柄是否是仍然有效的窗口。
pub fn is_window(raw: usize) -> bool {
    hwnd(raw).is_some_and(|handle| unsafe { IsWindow(handle).as_bool() })
}

/// 客户区尺寸 `(width, height)`。
pub fn client_size(raw: usize) -> Option<(i32, i32)> {
    let handle = hwnd(raw)?;
    let mut rect = RECT::default();
    unsafe { GetClientRect(handle, &mut rect) }.ok()?;
    Some((rect.right - rect.left, rect.bottom - rect.top))
}

/// 窗口的屏幕矩形。
pub fn window_rect(raw: usize) -> Option<HitRect> {
    let handle = hwnd(raw)?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(handle, &mut rect) }.ok()?;
    Some(HitRect::new(rect.left, rect.top, rect.right, rect.bottom))
}

/// 窗口所属进程 id。
pub fn process_id_of_window(raw: usize) -> Option<u32> {
    let handle = hwnd(raw)?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(handle, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// 该窗口是否是当前前台窗口。
pub fn is_foreground(raw: usize) -> bool {
    let Some(handle) = hwnd(raw) else {
        return false;
    };
    unsafe { GetForegroundWindow() == handle }
}

/// 把窗口区域裁成圆角矩形（命中测试与视觉一致）。
///
/// `radius` 为 `None` 时按高度自动取半径（胶囊的胶囊形）。
pub fn set_round_rect_region(raw: usize, rect: HitRect, radius: Option<i32>) -> Result<(), String> {
    let handle = hwnd(raw).ok_or_else(|| "null HWND".to_string())?;
    let radius = radius.unwrap_or(((rect.bottom - rect.top) / 2).max(1));
    let region =
        unsafe { CreateRoundRectRgn(rect.left, rect.top, rect.right, rect.bottom, radius, radius) };
    if region.0.is_null() {
        return Err("CreateRoundRectRgn returned a null region".to_string());
    }
    if unsafe { SetWindowRgn(handle, region, BOOL(1)) } == 0 {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(region.0));
        }
        return Err("SetWindowRgn failed".to_string());
    }
    Ok(())
}

/// 清除窗口区域裁剪（恢复整窗矩形）。
pub fn clear_window_region(raw: usize) {
    let Some(handle) = hwnd(raw) else {
        return;
    };
    use windows::Win32::Graphics::Gdi::HRGN;
    unsafe {
        let _ = SetWindowRgn(handle, HRGN::default(), BOOL(1));
    }
}

/// 按标题查找顶层窗口。
pub fn find_window_by_title(title: &str) -> Option<usize> {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) }
        .ok()
        .map(|hwnd| hwnd.0 as usize)
        .filter(|raw| *raw != 0)
}

/// 彻底隐藏一个置顶窗口：`ShowWindow(SW_HIDE)` + 取消 topmost。
///
/// 胶囊/镜像窗口是原生 SetWindowPos 显示的，隐藏也必须走原生路径，
/// 否则 tauri 的 hide 成功但 HWND 仍然可见。
pub fn hide_window_topmost(raw: usize) -> Result<(), String> {
    let handle = hwnd(raw).ok_or_else(|| "null HWND".to_string())?;
    let _ = unsafe { ShowWindow(handle, SW_HIDE) };
    unsafe {
        SetWindowPos(
            handle,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        )
    }
    .map_err(|error| error.to_string())
}

fn set_dwm_attribute<T>(handle: HWND, attribute: DWMWINDOWATTRIBUTE, value: &T, label: &str) {
    let result = unsafe {
        DwmSetWindowAttribute(
            handle,
            attribute,
            value as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(value) as u32,
        )
    };
    if let Err(error) = result {
        log::warn!("[window] set {label} failed (likely pre-22H2 Win): {error}");
    }
}

/// 同步原生标题栏配色（Win11 22H2+）。
///
/// 旧版本 Windows 会返回错误，只记日志、不阻塞启动。
pub fn apply_caption_theme(raw: usize, dark: bool) {
    let Some(handle) = hwnd(raw) else {
        return;
    };
    // COLORREF 0x00BBGGRR，与前端 tokens 对齐。
    let immersive_dark: i32 = i32::from(dark);
    let caption_color: u32 = if dark { 0x0022_1914 } else { 0x00F7_F5F5 };
    let text_color: u32 = if dark { 0x00FB_F7F4 } else { 0x002A_170F };
    let border_color: u32 = if dark { 0x002B_201A } else { 0x00E8_E8E8 };

    set_dwm_attribute(handle, DWMWA_USE_IMMERSIVE_DARK_MODE, &immersive_dark, "immersive dark mode");
    set_dwm_attribute(handle, DWMWA_CAPTION_COLOR, &caption_color, "caption color");
    set_dwm_attribute(handle, DWMWA_TEXT_COLOR, &text_color, "text color");
    set_dwm_attribute(handle, DWMWA_BORDER_COLOR, &border_color, "border color");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_handles_are_rejected_without_panicking() {
        assert!(!is_window(0));
        assert_eq!(client_size(0), None);
        assert_eq!(window_rect(0), None);
        assert!(!is_foreground(0));
        assert!(hide_window_topmost(0).is_err());
        assert_eq!(find_window_by_title("__openless_no_such_window__"), None);
    }
}

/// 以"置顶 + 不激活"的方式显示或隐藏窗口。
///
/// 胶囊窗口用 `SW_SHOWNOACTIVATE` 显示，避免抢焦点导致后续 AX/选区读取读到 OpenLess 自己。
pub fn set_topmost_no_activate(raw: usize, show: bool) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };

    let handle = hwnd(raw).ok_or_else(|| "null HWND".to_string())?;
    if show {
        let _ = unsafe { ShowWindow(handle, SW_SHOWNOACTIVATE) };
    }
    let flags = if show {
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW
    } else {
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE
    };
    unsafe { SetWindowPos(handle, HWND_TOPMOST, 0, 0, 0, 0, flags) }
        .map_err(|error| format!("SetWindowPos: {error}"))
}

/// 按标题隐藏一个置顶窗口（用于上一版本遗留的胶囊窗口）。
pub fn hide_window_by_title(title: &str) -> bool {
    match find_window_by_title(title) {
        Some(raw) => hide_window_topmost(raw).is_ok(),
        None => false,
    }
}

/// 移动/缩放窗口但不激活、不改 Z 序（用于把卡片窗口摆到计算出的位置）。
pub fn move_window_no_activate(raw: usize, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER};

    let handle = hwnd(raw).ok_or_else(|| "null HWND".to_string())?;
    unsafe { SetWindowPos(handle, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER) }
        .map_err(|error| format!("SetWindowPos: {error}"))
}
