//! Cross-platform text insertion at the current cursor position.
//!
//! Strategy:
//! 1. Always copy the text to the clipboard first (so the user can manually
//!    `Cmd+V` / `Ctrl+V` if simulation fails).
//! 2. On macOS, simulate Cmd+V via raw `CGEventPost` FFI — **不能用 enigo**：
//!    enigo 在 macOS 上的 keycode_to_string 会同步调 `TSMGetInputSourceProperty`，
//!    macOS 14+ 强制断言主线程，从 tokio worker 线程调就 SIGTRAP（已踩坑）。
//!    Swift 原版 `TextInserter.simulatePaste()` 用的就是 CGEventCreateKeyboardEvent
//!    → CGEventPost，跟我们这里完全同源。
//! 3. 其他平台 (Windows/Linux) 仍用 enigo。

use crate::types::InsertStatus;

pub struct TextInserter;

impl TextInserter {
    pub fn new() -> Self {
        Self
    }

    /// Insert `text` at the current cursor position.
    pub fn insert(&self, text: &str) -> InsertStatus {
        if text.is_empty() {
            return InsertStatus::CopiedFallback;
        }
        if !copy_to_clipboard(text) {
            return InsertStatus::Failed;
        }
        if let Err(err) = simulate_paste() {
            log::warn!("[insertion] simulated paste failed: {}", err);
            return InsertStatus::CopiedFallback;
        }
        insertion_success_status()
    }
}

impl Default for TextInserter {
    fn default() -> Self {
        Self::new()
    }
}

fn copy_to_clipboard(text: &str) -> bool {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(err) => {
            log::error!("[insertion] clipboard init failed: {}", err);
            return false;
        }
    };
    if let Err(err) = clipboard.set_text(text.to_string()) {
        log::error!("[insertion] clipboard set_text failed: {}", err);
        return false;
    }
    true
}

#[cfg(target_os = "macos")]
fn simulate_paste() -> Result<(), String> {
    macos::post_cmd_v()
}

#[cfg(not(target_os = "macos"))]
fn simulate_paste() -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let modifier = Key::Control;
    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| e.to_string())?;
    let press_v = enigo.key(Key::Unicode('v'), Direction::Click);
    let release_modifier = enigo.key(modifier, Direction::Release);
    if let Err(e) = release_modifier {
        return Err(e.to_string());
    }
    press_v.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn insertion_success_status() -> InsertStatus {
    InsertStatus::Inserted
}

#[cfg(not(target_os = "macos"))]
fn insertion_success_status() -> InsertStatus {
    // Windows/Linux 的 Ctrl+V 只能证明粘贴快捷键已发送，不能证明目标控件已接收。
    InsertStatus::CopiedFallback
}

// ─────────────────────────── macOS native CGEvent paste ───────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    #[repr(C)]
    struct OpaqueCGEvent(c_void);
    type CGEventRef = *mut OpaqueCGEvent;

    #[repr(C)]
    struct OpaqueCGEventSource(c_void);
    type CGEventSourceRef = *mut OpaqueCGEventSource;

    type CGEventTapLocation = u32;
    type CGEventSourceStateID = i32;
    type CGKeyCode = u16;
    type CGEventFlags = u64;

    const KCG_HID_EVENT_TAP: CGEventTapLocation = 0;
    const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: CGEventSourceStateID = 1;
    const KCG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x00100000;
    /// Virtual keycode for "V" on US/ANSI layouts (kVK_ANSI_V).
    const KEY_V: CGKeyCode = 9;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: CGEventSourceStateID) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: CGKeyCode,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
        fn CGEventPost(tap: CGEventTapLocation, event: CGEventRef);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    /// 与 Swift `TextInserter.simulatePaste()` 同源:
    ///   下 V + 加 Cmd flag → post → 上 V + 加 Cmd flag → post
    /// 全部走 C 层 CGEvent，不会触发 enigo 那条 TSM 主线程断言路径。
    pub fn post_cmd_v() -> Result<(), String> {
        unsafe {
            let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
            // 即使 source 是空也能 post（Apple 文档允许 NULL source），所以不当致命错误。
            let down = CGEventCreateKeyboardEvent(source, KEY_V, true);
            let up = CGEventCreateKeyboardEvent(source, KEY_V, false);
            if down.is_null() || up.is_null() {
                if !source.is_null() {
                    CFRelease(source as *const c_void);
                }
                if !down.is_null() {
                    CFRelease(down as *const c_void);
                }
                if !up.is_null() {
                    CFRelease(up as *const c_void);
                }
                return Err("CGEventCreateKeyboardEvent returned null".into());
            }
            CGEventSetFlags(down, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventSetFlags(up, KCG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(KCG_HID_EVENT_TAP, down);
            CGEventPost(KCG_HID_EVENT_TAP, up);

            CFRelease(down as *const c_void);
            CFRelease(up as *const c_void);
            if !source.is_null() {
                CFRelease(source as *const c_void);
            }
        }
        Ok(())
    }
}
