//! Window-owned hit-test state; repeated installs must preserve other subclasses.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HTTRANSPARENT, WM_NCDESTROY, WM_NCHITTEST,
};

const SUBCLASS_ID: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CapsuleHitTestRect {
    pub(crate) left: i32,
    pub(crate) right: i32,
    pub(crate) top: i32,
    pub(crate) bottom: i32,
}

unsafe extern "system" fn capsule_hit_test_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(capsule_hit_test_subclass_proc), subclass_id);
        drop(Box::from_raw(data as *mut CapsuleHitTestRect));
        return DefSubclassProc(hwnd, msg, wparam, lparam);
    }

    if msg == WM_NCHITTEST {
        let control = *(data as *const CapsuleHitTestRect);
        let screen_x = (lparam.0 as i32 & 0xffff) as u16 as i16 as i32;
        let screen_y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as i32;
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            let client_x = screen_x - rect.left;
            let client_y = screen_y - rect.top;
            if client_x < control.left
                || client_x >= control.right
                || client_y < control.top
                || client_y >= control.bottom
            {
                return LRESULT(HTTRANSPARENT as isize);
            }
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Called on the window's owning thread by the capsule/card layout paths.
pub(crate) fn install_capsule_hit_test_proc(
    hwnd: HWND,
    rect: CapsuleHitTestRect,
) -> windows::core::Result<()> {
    if hwnd.0.is_null() {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_INVALIDARG,
        ));
    }

    let mut data = 0;
    unsafe {
        // A subclass can remain in the chain even when another callback is on top.
        if GetWindowSubclass(
            hwnd,
            Some(capsule_hit_test_subclass_proc),
            SUBCLASS_ID,
            Some(&mut data),
        )
        .as_bool()
        {
            *(data as *mut CapsuleHitTestRect) = rect;
            return Ok(());
        }
        let data = Box::into_raw(Box::new(rect));
        if !SetWindowSubclass(
            hwnd,
            Some(capsule_hit_test_subclass_proc),
            SUBCLASS_ID,
            data as usize,
        )
        .as_bool()
        {
            drop(Box::from_raw(data));
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_FAIL,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW,
        RegisterClassW, SendMessageW, SetWindowLongPtrW, UnregisterClassW, GWLP_USERDATA,
        GWLP_WNDPROC, HTCLIENT, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSW,
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

    fn rect() -> CapsuleHitTestRect {
        CapsuleHitTestRect {
            left: 10,
            right: 20,
            top: 30,
            bottom: 40,
        }
    }

    #[test]
    fn reinstall_refreshes_geometry_and_keeps_message_forwarding() {
        let window = TestWindow::new();
        install_capsule_hit_test_proc(window.hwnd, rect()).unwrap();
        assert_eq!(window.hit_test(15, 35), HTCLIENT as isize);
        assert_eq!(window.hit_test(0, 0), HTTRANSPARENT as isize);
        install_capsule_hit_test_proc(
            window.hwnd,
            CapsuleHitTestRect {
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
        install_capsule_hit_test_proc(window.hwnd, rect()).unwrap();
        window.add_upper_subclass();
        window.send_probe();
        install_capsule_hit_test_proc(window.hwnd, rect()).unwrap();
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
    fn destroy_through_upper_subclass_reaches_original_cleanup() {
        let mut window = TestWindow::new();
        install_capsule_hit_test_proc(window.hwnd, rect()).unwrap();
        window.add_upper_subclass();
        window.destroy();
        assert!(window.probe.destroyed.get());
    }

    #[test]
    fn rectangles_belong_to_their_window_and_do_not_survive_recreation() {
        let mut first = TestWindow::new();
        let second = TestWindow::new();
        install_capsule_hit_test_proc(first.hwnd, rect()).unwrap();
        install_capsule_hit_test_proc(
            second.hwnd,
            CapsuleHitTestRect {
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
        install_capsule_hit_test_proc(recreated.hwnd, rect()).unwrap();
        assert_eq!(recreated.hit_test(15, 35), HTCLIENT as isize);
        second.send_probe();
    }

    #[test]
    fn destroyed_window_rejects_installation() {
        let mut window = TestWindow::new();
        let hwnd = window.hwnd;
        window.destroy();
        assert!(install_capsule_hit_test_proc(hwnd, rect()).is_err());
    }

    #[test]
    fn null_window_is_rejected() {
        assert!(install_capsule_hit_test_proc(HWND::default(), rect()).is_err());
    }
}
