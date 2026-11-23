//! Windows 按键注入底层（SendInput）。
//!
//! 这是平台内的"系统调用层"，供文本写入与选区捕获共用；它不构成对外能力，
//! 对外的按键能力见 crate 顶层的 `input` 门面。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_C, VK_CONTROL,
};

use crate::common::error::PlatformError;

/// 发送一个虚拟键的按下/抬起事件。
pub(crate) fn send_vk(vk: VIRTUAL_KEY, key_up: bool) -> Result<(), PlatformError> {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(input)
}

pub(crate) fn press_vk(vk: VIRTUAL_KEY) -> Result<(), PlatformError> {
    send_vk(vk, false)?;
    send_vk(vk, true)
}

/// 发送一个键组合：按下修饰键 → 点主键 → 逆序松开修饰键。
///
/// 任何一步失败都会把已按下的修饰键松开，避免系统里留下卡住的键。
pub(crate) fn send_chord(
    modifiers: &[VIRTUAL_KEY],
    primary: VIRTUAL_KEY,
) -> Result<(), PlatformError> {
    let mut pressed = 0usize;
    for modifier in modifiers {
        if let Err(error) = send_vk(*modifier, false) {
            for modifier in modifiers[..pressed].iter().rev() {
                let _ = send_vk(*modifier, true);
            }
            return Err(error);
        }
        pressed += 1;
    }

    let mut first_error = press_vk(primary).err();
    for modifier in modifiers.iter().rev() {
        if let Err(error) = send_vk(*modifier, true) {
            first_error.get_or_insert(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Ctrl+C：抓选区时让目标应用把选区复制到剪贴板。
pub(crate) fn send_copy_shortcut() -> Result<(), PlatformError> {
    send_chord(&[VK_CONTROL], VK_C)
}

/// 投递一个 UTF-16 码元（Unicode 键入）。
pub(crate) fn send_unicode_unit(unit: u16, key_up: bool) -> Result<(), PlatformError> {
    let flags = if key_up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(input)
}

fn send(input: INPUT) -> Result<(), PlatformError> {
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        Err(PlatformError::SendFailed(
            std::io::Error::last_os_error().to_string(),
        ))
    }
}
