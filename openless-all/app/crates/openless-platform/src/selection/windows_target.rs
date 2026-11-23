//! Windows 选区插入目标的指纹。
//!
//! 粘贴前必须确认"焦点还在当初捕获选区的那个控件上"。仅凭顶层 HWND 不够：
//! 同一应用里切换编辑面板会保留 HWND，所以同时记录前台窗口、焦点子控件，
//! 以及它们各自的进程/线程标识。

use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
};

/// 捕获时的焦点指纹。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsSelectionTarget {
    pub foreground_window: usize,
    pub focused_window: usize,
    pub foreground_process_id: u32,
    pub foreground_thread_id: u32,
    pub focused_process_id: u32,
    pub focused_thread_id: u32,
}

/// 捕获当前前台窗口与焦点子控件。
pub fn capture() -> Option<WindowsSelectionTarget> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return None;
        }

        let mut foreground_process_id = 0;
        let foreground_thread_id =
            GetWindowThreadProcessId(foreground, Some(&mut foreground_process_id));
        if foreground_process_id == 0 || foreground_thread_id == 0 {
            return None;
        }

        let mut gui_info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let focused = if GetGUIThreadInfo(foreground_thread_id, &mut gui_info).is_ok()
            && !gui_info.hwndFocus.0.is_null()
        {
            gui_info.hwndFocus
        } else {
            foreground
        };
        let mut focused_process_id = 0;
        let focused_thread_id = GetWindowThreadProcessId(focused, Some(&mut focused_process_id));
        if focused_process_id == 0 || focused_thread_id == 0 {
            return None;
        }

        Some(WindowsSelectionTarget {
            foreground_window: foreground.0 as usize,
            focused_window: focused.0 as usize,
            foreground_process_id,
            foreground_thread_id,
            focused_process_id,
            focused_thread_id,
        })
    }
}

/// 当前焦点是否仍是捕获时那个控件。
pub fn matches(captured: WindowsSelectionTarget, current: WindowsSelectionTarget) -> bool {
    captured == current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(seed: usize) -> WindowsSelectionTarget {
        WindowsSelectionTarget {
            foreground_window: seed,
            focused_window: seed + 1,
            foreground_process_id: seed as u32,
            foreground_thread_id: seed as u32 + 1,
            focused_process_id: seed as u32 + 2,
            focused_thread_id: seed as u32 + 3,
        }
    }

    #[test]
    fn identical_fingerprints_match() {
        assert!(matches(target(1), target(1)));
    }

    #[test]
    fn switching_the_focused_child_is_a_change() {
        let captured = target(1);
        let mut current = target(1);
        current.focused_window += 10;
        assert!(!matches(captured, current));
    }
}
