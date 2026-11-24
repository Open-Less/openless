//! Windows 胶囊窗口控制：命中测试、点击穿透、不抢焦点。
//!
//! 全部以 `HWND` 为参数，宿主在窗口创建之后把句柄传进来。
//! 子类化用的是窗口自有 subclass，重复安装不会破坏其它回调链。

use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HTTRANSPARENT, MA_NOACTIVATE, WM_MOUSEACTIVATE, WM_NCDESTROY, WM_NCHITTEST,
};

const SUBCLASS_ID: usize = 1;

/// 命中区域（相对窗口左上角的客户区坐标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitRect {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

impl HitRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// 点是否落在区域内。
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

struct HitTestState {
    rect: HitRect,
    no_activate: bool,
}

unsafe extern "system" fn hit_test_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(hit_test_subclass_proc), subclass_id);
        drop(Box::from_raw(data as *mut HitTestState));
        return DefSubclassProc(hwnd, msg, wparam, lparam);
    }

    let state = &*(data as *const HitTestState);

    if msg == WM_MOUSEACTIVATE && state.no_activate {
        return LRESULT(MA_NOACTIVATE as isize);
    }

    if msg == WM_NCHITTEST {
        let control = state.rect;
        // lparam 是屏幕坐标（低 16 位 x，高 16 位 y，有符号）。
        let screen_x = (lparam.0 as i32 & 0xffff) as u16 as i16 as i32;
        let screen_y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            let client_x = screen_x - rect.left;
            let client_y = screen_y - rect.top;
            if !control.contains(client_x, client_y) {
                // 区域外透明：点击穿透到下层窗口。
                return LRESULT(HTTRANSPARENT as isize);
            }
        }
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// 安装/更新命中测试区域（胶囊本体，不抢焦点）。
///
/// 参数是裸 `HWND`（以 `usize` 传递），宿主不需要认识 windows crate 类型。
pub fn install_hit_test(raw_hwnd: usize, rect: HitRect) -> windows::core::Result<()> {
    install(raw_hwnd, rect, false)
}

/// 安装/更新命中测试区域（卡片，允许激活）。
pub fn install_card_hit_test(raw_hwnd: usize, rect: HitRect) -> windows::core::Result<()> {
    install(raw_hwnd, rect, true)
}

/// 不替换子类链，只改激活策略。
pub fn set_no_activate(raw_hwnd: usize, no_activate: bool) {
    if raw_hwnd == 0 {
        return;
    }
    let hwnd = HWND(raw_hwnd as *mut std::ffi::c_void);
    let mut data = 0;
    unsafe {
        if GetWindowSubclass(hwnd, Some(hit_test_subclass_proc), SUBCLASS_ID, Some(&mut data))
            .as_bool()
        {
            (*(data as *mut HitTestState)).no_activate = no_activate;
        }
    }
}

fn install(raw_hwnd: usize, rect: HitRect, no_activate: bool) -> windows::core::Result<()> {
    if raw_hwnd == 0 {
        return Err(windows::core::Error::from_hresult(E_INVALIDARG));
    }
    let hwnd = HWND(raw_hwnd as *mut std::ffi::c_void);
    let mut data = 0;
    unsafe {
        // 子类可能仍在链上（只是被别的回调覆盖），此时原地更新状态。
        if GetWindowSubclass(hwnd, Some(hit_test_subclass_proc), SUBCLASS_ID, Some(&mut data))
            .as_bool()
        {
            *(data as *mut HitTestState) = HitTestState { rect, no_activate };
            return Ok(());
        }
        let data = Box::into_raw(Box::new(HitTestState { rect, no_activate }));
        if !SetWindowSubclass(hwnd, Some(hit_test_subclass_proc), SUBCLASS_ID, data as usize)
            .as_bool()
        {
            drop(Box::from_raw(data));
            return Err(windows::core::Error::from_hresult(E_FAIL));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rect_uses_half_open_bounds() {
        let rect = HitRect::new(10, 20, 110, 60);
        assert!(rect.contains(10, 20));
        assert!(rect.contains(109, 59));
        assert!(!rect.contains(110, 59));
        assert!(!rect.contains(109, 60));
        assert!(!rect.contains(9, 20));
    }

    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW,
        RegisterClassW, SendMessageW, SetWindowLongPtrW, UnregisterClassW, GWLP_USERDATA,
        GWLP_WNDPROC, HTCLIENT, HWND_MESSAGE, MA_ACTIVATE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
        WNDCLASSW,
    };

    const PROBE_MESSAGE: u32 = WM_APP + 42;
    const PROBE_RESULT: isize = 12345;
    static NEXT_CLASS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Default)]
    struct Probe {
        messages: Cell<u32>,
        destroyed: Cell<bool>,
        upper_calls: Cell<u32>,
        upper_previous: Cell<isize>,
    }

    unsafe extern "system" fn original_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let data = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Probe;
        if !data.is_null() {
            let probe = &*data;
            match msg {
                PROBE_MESSAGE => {
                    probe.messages.set(probe.messages.get() + 1);
                    return LRESULT(PROBE_RESULT);
                }
                WM_NCDESTROY => probe.destroyed.set(true),
                WM_NCHITTEST => return LRESULT(HTCLIENT as isize),
                WM_MOUSEACTIVATE => return LRESULT(MA_ACTIVATE as isize),
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    unsafe extern "system" fn upper_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let probe = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Probe);
        if msg == PROBE_MESSAGE {
            probe.upper_calls.set(probe.upper_calls.get() + 1);
            // Bound the failing case instead of overflowing the test process stack.
            if probe.upper_calls.get() > 4 {
                return LRESULT(-1);
            }
        }
        CallWindowProcW(
            std::mem::transmute::<isize, windows::Win32::UI::WindowsAndMessaging::WNDPROC>(
                probe.upper_previous.get(),
            ),
            hwnd,
            msg,
            wparam,
            lparam,
        )
    }

    struct TestWindow {
        hwnd: HWND,
        class: Vec<u16>,
        probe: Box<Probe>,
    }

    impl TestWindow {
        fn new() -> Self {
            let class: Vec<u16> = format!(
                "OpenLessHitTest{}",
                NEXT_CLASS.fetch_add(1, Ordering::Relaxed)
            )
            .encode_utf16()
            .chain(Some(0))
            .collect();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(original_proc),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            assert_ne!(unsafe { RegisterClassW(&wc) }, 0);
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    PCWSTR(class.as_ptr()),
                    PCWSTR::null(),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    100,
                    100,
                    HWND_MESSAGE,
                    None,
                    HINSTANCE::default(),
                    None,
                )
            }
            .expect("CreateWindowExW");
            let probe = Box::<Probe>::default();
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*probe as *const Probe as isize);
            }
            Self { hwnd, class, probe }
        }

        fn add_upper_subclass(&self) {
            let previous = unsafe {
                SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, upper_proc as *const () as isize)
            };
            assert_ne!(previous, 0);
            self.probe.upper_previous.set(previous);
        }

        fn send_probe(&self) {
            assert_eq!(
                unsafe { SendMessageW(self.hwnd, PROBE_MESSAGE, WPARAM(0), LPARAM(0)) }.0,
                PROBE_RESULT,
            );
        }

        fn mouse_activate(&self) -> isize {
            unsafe { SendMessageW(self.hwnd, WM_MOUSEACTIVATE, WPARAM(0), LPARAM(0)) }.0
        }

        fn hit_test(&self, x: i32, y: i32) -> isize {
            let mut bounds = RECT::default();
            unsafe { GetWindowRect(self.hwnd, &mut bounds) }.unwrap();
            let x = (bounds.left + x) as u16 as u32;
            let y = (bounds.top + y) as u16 as u32;
            unsafe {
                SendMessageW(
                    self.hwnd,
                    WM_NCHITTEST,
                    WPARAM(0),
                    LPARAM((x | (y << 16)) as isize),
                )
            }
            .0
        }

        fn destroy(&mut self) {
            unsafe { DestroyWindow(self.hwnd) }.expect("DestroyWindow");
            self.hwnd = HWND::default();
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            if !self.hwnd.0.is_null() {
                self.destroy();
            }
            unsafe { UnregisterClassW(PCWSTR(self.class.as_ptr()), HINSTANCE::default()) }
                .expect("UnregisterClassW");
        }
    }

    fn rect() -> HitRect {
        HitRect {
            left: 10,
            right: 20,
            top: 30,
            bottom: 40,
        }
    }

    #[test]
    fn reinstall_refreshes_geometry_and_keeps_message_forwarding() {
        let window = TestWindow::new();
        install_hit_test(window.hwnd, rect()).unwrap();
        assert_eq!(window.hit_test(15, 35), HTCLIENT as isize);
        assert_eq!(window.hit_test(0, 0), HTTRANSPARENT as isize);
        install_hit_test(
            window.hwnd,
            HitRect {
                left: 0,
                right: 5,
                top: 0,
                bottom: 5,
            },
        )
        .unwrap();
        assert_eq!(window.hit_test(15, 35), HTTRANSPARENT as isize);
        assert_eq!(window.hit_test(1, 1), HTCLIENT as isize);
        window.send_probe();
        assert_eq!(window.probe.messages.get(), 1);
    }

    #[test]
    fn upper_subclass_survives_forwarding_and_reinstall() {
        let window = TestWindow::new();
        install_hit_test(window.hwnd, rect()).unwrap();
        window.add_upper_subclass();
        window.send_probe();
        install_hit_test(window.hwnd, rect()).unwrap();
        assert_eq!(
            unsafe { GetWindowLongPtrW(window.hwnd, GWLP_WNDPROC) },
            upper_proc as *const () as isize
        );
        window.send_probe();
        assert_eq!(window.probe.messages.get(), 2);
        assert_eq!(window.probe.upper_calls.get(), 2);
        assert_eq!(window.hit_test(0, 0), HTTRANSPARENT as isize);
    }

    #[test]
    fn card_mode_preserves_upper_subclasses_and_releases_no_activate_on_restore() {
        let window = TestWindow::new();
        install_card_hit_test(window.hwnd, rect()).unwrap();
        window.add_upper_subclass();
        assert_eq!(window.mouse_activate(), MA_NOACTIVATE as isize);
        install_card_hit_test(window.hwnd, rect()).unwrap();
        assert_eq!(window.mouse_activate(), MA_NOACTIVATE as isize);
        window.send_probe();
        assert_eq!(window.probe.upper_calls.get(), 1);

        set_no_activate(window.hwnd, false);
        assert_eq!(window.mouse_activate(), MA_ACTIVATE as isize);
        assert_eq!(window.hit_test(15, 35), HTCLIENT as isize);

        install_card_hit_test(window.hwnd, rect()).unwrap();
        install_hit_test(window.hwnd, rect()).unwrap();
        assert_eq!(window.mouse_activate(), MA_ACTIVATE as isize);
    }

    #[test]
    fn activation_policy_belongs_to_each_window_and_is_cleared_on_destruction() {
        let mut card = TestWindow::new();
        let capsule = TestWindow::new();
        install_card_hit_test(card.hwnd, rect()).unwrap();
        install_hit_test(capsule.hwnd, rect()).unwrap();
        assert_eq!(card.mouse_activate(), MA_NOACTIVATE as isize);
        assert_eq!(capsule.mouse_activate(), MA_ACTIVATE as isize);
        card.destroy();

        let recreated = TestWindow::new();
        install_hit_test(recreated.hwnd, rect()).unwrap();
        assert_eq!(recreated.mouse_activate(), MA_ACTIVATE as isize);
        assert_eq!(capsule.mouse_activate(), MA_ACTIVATE as isize);
    }

    #[test]
    fn destroy_through_upper_subclass_reaches_original_cleanup() {
        let mut window = TestWindow::new();
        install_hit_test(window.hwnd, rect()).unwrap();
        window.add_upper_subclass();
        window.destroy();
        assert!(window.probe.destroyed.get());
    }

    #[test]
    fn rectangles_belong_to_their_window_and_do_not_survive_recreation() {
        let mut first = TestWindow::new();
        let second = TestWindow::new();
        install_hit_test(first.hwnd, rect()).unwrap();
        install_hit_test(
            second.hwnd,
            HitRect {
                left: 0,
                right: 5,
                top: 0,
                bottom: 5,
            },
        )
        .unwrap();
        assert_eq!(first.hit_test(15, 35), HTCLIENT as isize);
        assert_eq!(second.hit_test(15, 35), HTTRANSPARENT as isize);
        first.destroy();
        assert!(first.probe.destroyed.get());
        let recreated = TestWindow::new();
        assert!(!unsafe {
            GetWindowSubclass(
                recreated.hwnd,
                Some(capsule_hit_test_subclass_proc),
                SUBCLASS_ID,
                None,
            )
        }
        .as_bool());
        install_hit_test(recreated.hwnd, rect()).unwrap();
        assert_eq!(recreated.hit_test(15, 35), HTCLIENT as isize);
        second.send_probe();
    }

    #[test]
    fn destroyed_window_rejects_installation() {
        let mut window = TestWindow::new();
        let hwnd = window.hwnd;
        window.destroy();
        assert!(install_hit_test(hwnd, rect()).is_err());
    }

    #[test]
    fn null_window_is_rejected() {
        assert!(install_hit_test(HWND::default(), rect()).is_err());
    }
}

/// 当前鼠标位置（虚拟桌面坐标）。
pub fn cursor_position() -> Option<(i32, i32)> {
    let mut point = windows::Win32::Foundation::POINT::default();
    // SAFETY: GetCursorPos 只写入这个已初始化的 POINT。
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }
        .ok()
        .map(|_| (point.x, point.y))
}

/// 窗口所在显示器的外框（虚拟桌面坐标）；窗口无效时返回 `None`。
pub fn monitor_bounds_for_hwnd(raw_hwnd: usize) -> Option<HitRect> {
    use std::ffi::c_void;

    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let hwnd = HWND(raw_hwnd as *mut c_void);
    if hwnd.0.is_null() {
        return None;
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` 已按 Win32 要求设置结构大小，仅由系统写入。
    if !unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() } {
        return None;
    }
    let rect = info.rcMonitor;
    Some(HitRect::new(rect.left, rect.top, rect.right, rect.bottom))
}

/// 前台窗口句柄（无前台窗口时 `None`）。
pub fn foreground_window() -> Option<usize> {
    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    (!hwnd.0.is_null()).then_some(hwnd.0 as usize)
}

/// 前台窗口的进程 id。
pub fn foreground_process_id() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// 当前进程 id。
pub fn current_process_id() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }
}

/// 把焦点还给某个窗口（必要时从最小化恢复）。返回是否成功。
pub fn restore_focus_to_window(raw_target: usize) -> bool {
    use std::ffi::c_void;

    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let hwnd = HWND(raw_target as *mut c_void);
    if hwnd.0.is_null() {
        return false;
    }
    if !unsafe { IsWindow(hwnd) }.as_bool() {
        return false;
    }
    if unsafe { GetForegroundWindow() } == hwnd {
        return true;
    }
    if unsafe { IsIconic(hwnd) }.as_bool() {
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    let _ = unsafe { SetForegroundWindow(hwnd) };
    std::thread::sleep(std::time::Duration::from_millis(60));
    (unsafe { GetForegroundWindow() }) == hwnd
}
