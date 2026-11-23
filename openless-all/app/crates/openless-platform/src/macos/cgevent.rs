//! macOS 按键注入底层（CoreGraphics）。
//!
//! 与文本写入共用同一套 CGEvent 投递：直接调 FFI，避免 enigo 在非主线程触发的
//! TSM 主线程断言。

use std::ffi::c_void;

use openless_core::shared_types::PasteShortcut;

use crate::common::error::PlatformError;

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
const KCG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x0010_0000;
/// kVK_ANSI_C
const KEY_C: CGKeyCode = 8;
/// kVK_ANSI_V
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

/// Cmd+C：抓选区时让目标应用复制。
pub(crate) fn send_copy_shortcut() -> Result<(), PlatformError> {
    post_command_key(KEY_C)
}

/// Cmd+V：粘贴。
pub(crate) fn send_paste_shortcut(_shortcut: PasteShortcut) -> Result<(), PlatformError> {
    post_command_key(KEY_V)
}

fn post_command_key(key: CGKeyCode) -> Result<(), PlatformError> {
    unsafe {
        let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
        // source 为 NULL 时仍然可以投递事件（Apple 文档允许），不当作失败。
        let down = CGEventCreateKeyboardEvent(source, key, true);
        let up = CGEventCreateKeyboardEvent(source, key, false);
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
            return Err(PlatformError::SendFailed(
                "CGEventCreateKeyboardEvent returned null".into(),
            ));
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
