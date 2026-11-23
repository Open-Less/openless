//! macOS 胶囊窗口控制：按 DOM 量测出的矩形做点击穿透。
//!
//! 与 Windows 的区别：AppKit 的 `setIgnoresMouseEvents` 是按窗口整体设置的，
//! 所以我们要先把屏幕坐标换算成窗口内坐标，判断是否落在任一交互区域内。
//!
//! 宿主传入 `NSWindow*`（不透明指针）与矩形集合；tauri 窗口对象、主线程调度、
//! 事件订阅都留在宿主。

use std::ffi::c_void;

/// 交互区域：DOM 量测出的**逻辑**矩形（原点在窗口内容区左上角，y 向下）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl InputRect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left && x < self.left + self.width && y >= self.top && y < self.top + self.height
    }

    /// 非有限值（NaN/Inf）或负尺寸视为无效矩形，直接忽略。
    pub fn valid(&self) -> bool {
        self.left.is_finite()
            && self.top.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

/// 判断某个点（窗口内逻辑坐标，y 向下）是否需要接收鼠标事件。
pub fn should_receive_mouse(regions: &[InputRect], x: f64, y: f64) -> bool {
    regions
        .iter()
        .filter(|rect| rect.valid())
        .any(|rect| rect.contains(x, y))
}

#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::c_void;

    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};
    use objc2_foundation::{NSPoint, NSRect};

    /// 把 `NSWindow*` 的 `ignoresMouseEvents` 设为 `pass`。
    ///
    /// `window` 必须是 NSWindow 指针；调用方负责保证它在主线程上且仍然有效。
    ///
    /// # Safety
    /// `window` 必须指向一个有效的 NSWindow 对象，且在主线程调用。
    pub unsafe fn set_ignores_mouse_events(window: *mut c_void, pass: bool) {
        if window.is_null() {
            return;
        }
        let native = window as *mut AnyObject;
        let _: () = unsafe { msg_send![native, setIgnoresMouseEvents: Bool::new(pass)] };
    }

    /// 读取当前光标在窗口内容区内的逻辑坐标（原点左上、y 向下）。
    ///
    /// # Safety
    /// `window` 必须指向有效的 NSWindow 对象，且在主线程调用。
    pub unsafe fn cursor_in_window(window: *mut c_void) -> Option<(f64, f64)> {
        if window.is_null() {
            return None;
        }
        let native = window as *mut AnyObject;
        let view: *mut AnyObject = unsafe { msg_send![native, contentView] };
        if view.is_null() {
            return None;
        }
        // AppKit 把屏幕坐标直接换算到这个窗口的逻辑坐标，避开了 tauri 的全局光标
        // 与窗口位置使用不同缩放因子的问题。
        let screen: NSPoint = unsafe { msg_send![objc2::class!(NSEvent), mouseLocation] };
        let point: NSPoint = unsafe { msg_send![native, convertPointFromScreen: screen] };
        let local: NSPoint = unsafe {
            msg_send![view, convertPoint: point fromView: std::ptr::null::<AnyObject>()]
        };
        let bounds: NSRect = unsafe { msg_send![view, bounds] };
        let flipped: Bool = unsafe { msg_send![view, isFlipped] };
        let x = local.x - bounds.origin.x;
        let y = if flipped.as_bool() {
            local.y - bounds.origin.y
        } else {
            bounds.origin.y + bounds.size.height - local.y
        };
        Some((x, y))
    }
}

#[cfg(target_os = "macos")]
pub use ffi::{cursor_in_window, set_ignores_mouse_events};

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f64, top: f64, width: f64, height: f64) -> InputRect {
        InputRect {
            left,
            top,
            width,
            height,
        }
    }

    #[test]
    fn only_measured_controls_receive_the_mouse() {
        let regions = [rect(10.0, 100.0, 32.0, 32.0), rect(400.0, 100.0, 32.0, 32.0)];
        assert!(should_receive_mouse(&regions, 20.0, 110.0));
        assert!(!should_receive_mouse(&regions, 220.0, 110.0));
        assert!(!should_receive_mouse(&regions, 20.0, 20.0));
    }

    #[test]
    fn non_finite_or_negative_rects_are_ignored() {
        assert!(!rect(f64::NAN, 0.0, 20.0, 20.0).valid());
        assert!(!rect(0.0, 0.0, -1.0, 20.0).valid());
        assert!(!should_receive_mouse(&[rect(f64::NAN, 0.0, 20.0, 20.0)], 1.0, 1.0));
    }

    #[test]
    fn bounds_are_half_open() {
        let regions = [rect(0.0, 0.0, 10.0, 10.0)];
        assert!(should_receive_mouse(&regions, 0.0, 0.0));
        assert!(should_receive_mouse(&regions, 9.99, 9.99));
        assert!(!should_receive_mouse(&regions, 10.0, 5.0));
    }
}
