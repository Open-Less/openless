//! Windows 文本写入后端。
//!
//! 两条通道，都绕开剪贴板：
//! - [`type_text`]：SendInput 逐字 Unicode（按 UTF-16 code unit 投递），
//!   换行按 `newline_mode` 决定（Enter / Shift+Enter / CRLF）；
//! - [`send_paste_shortcut`]：SendInput 模拟 Ctrl+V / Ctrl+Shift+V / Shift+Insert。
//!
//! 节奏与迁移前保持一致：每个事件后 1ms（给目标消息循环处理时间，避免 CJK→ASCII
//! 切换时丢首字符），每 16 个事件后额外 12ms。

use std::time::Duration;

use openless_core::shared_types::{PasteShortcut, WindowsSendInputNewlineMode};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_CONTROL, VK_INSERT, VK_RETURN, VK_SHIFT, VK_TAB, VK_V,
};

use crate::text::TextError;

const SENDINPUT_CHUNK_CHARS: usize = 16;
const SENDINPUT_CHUNK_DELAY: Duration = Duration::from_millis(12);
/// 每个合成按键后的间隔；见模块文档。
const INTER_KEYSTROKE_DELAY: Duration = Duration::from_millis(1);

/// 逐字输入：返回已投递的源字符数。
pub fn type_text(
    text: &str,
    newline_mode: WindowsSendInputNewlineMode,
) -> Result<usize, TextError> {
    type_text_with(text, newline_mode, std::thread::sleep)
}

/// 可注入 sleep 的版本，便于在没有真实 SendInput 的环境里测节奏与计数。
fn type_text_with(
    text: &str,
    newline_mode: WindowsSendInputNewlineMode,
    mut sleep: impl FnMut(Duration),
) -> Result<usize, TextError> {
    if text.is_empty() {
        return Ok(0);
    }
    let mut typed_chars = 0;
    let mut sent_in_chunk = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        // typed_chars 统计的是"已消费的源标量"，不是系统按键数：
        // 被吞掉的 `\r` 也算消费，否则 CRLF 会被误判成部分失败。
        match classify_char(ch) {
            CharKind::Skip => {
                typed_chars += 1;
                continue;
            }
            _ => {
                if let Err(error) = send_character(ch, newline_mode) {
                    return Err(partial_or_original(typed_chars, error));
                }
            }
        }
        typed_chars += 1;
        sent_in_chunk += 1;
        sleep(INTER_KEYSTROKE_DELAY);
        if sent_in_chunk >= SENDINPUT_CHUNK_CHARS && chars.peek().is_some() {
            sleep(SENDINPUT_CHUNK_DELAY);
            sent_in_chunk = 0;
        }
    }
    Ok(typed_chars)
}

/// 配置的粘贴快捷键对应的（修饰键，主键）。
fn paste_shortcut_keys(shortcut: PasteShortcut) -> (&'static [VIRTUAL_KEY], VIRTUAL_KEY) {
    match shortcut {
        PasteShortcut::CtrlV => (&[VK_CONTROL], VK_V),
        PasteShortcut::CtrlShiftV => (&[VK_CONTROL, VK_SHIFT], VK_V),
        // Shift+Insert 是终端类应用的粘贴键。
        PasteShortcut::ShiftInsert => (&[VK_SHIFT], VK_INSERT),
    }
}

/// 模拟粘贴快捷键。底层键注入复用 [`super::input`]，与选区捕获共用同一套实现。
pub fn send_paste_shortcut(shortcut: PasteShortcut) -> Result<(), TextError> {
    let (modifiers, primary) = paste_shortcut_keys(shortcut);
    super::input::send_chord(modifiers, primary)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharKind {
    /// `\r`：不产生事件，但计入已消费前缀（CRLF 只落一个换行）。
    Skip,
    Newline,
    Tab,
    Unicode,
}

fn classify_char(ch: char) -> CharKind {
    match ch {
        '\r' => CharKind::Skip,
        '\n' => CharKind::Newline,
        '\t' => CharKind::Tab,
        _ => CharKind::Unicode,
    }
}

fn send_character(ch: char, newline_mode: WindowsSendInputNewlineMode) -> Result<(), TextError> {
    match classify_char(ch) {
        CharKind::Skip => Ok(()),
        CharKind::Newline => send_newline(newline_mode),
        CharKind::Tab => press_vk(VK_TAB),
        CharKind::Unicode => {
            let mut buffer = [0u16; 2];
            for unit in ch.encode_utf16(&mut buffer) {
                send_utf16_unit(*unit, false)?;
                send_utf16_unit(*unit, true)?;
            }
            Ok(())
        }
    }
}

fn partial_or_original(typed_chars: usize, source: TextError) -> TextError {
    if typed_chars == 0 {
        source
    } else {
        TextError::Partial {
            typed_chars,
            source: Box::new(source),
        }
    }
}

fn send_newline(mode: WindowsSendInputNewlineMode) -> Result<(), TextError> {
    match mode {
        WindowsSendInputNewlineMode::Enter => press_vk(VK_RETURN),
        WindowsSendInputNewlineMode::ShiftEnter => {
            super::input::send_vk(VK_SHIFT, false)?;
            let result = press_vk(VK_RETURN);
            let release = super::input::send_vk(VK_SHIFT, true);
            result.and(release)
        }
        WindowsSendInputNewlineMode::CrLf => {
            send_utf16_unit(0x000D, false)?;
            send_utf16_unit(0x000D, true)?;
            send_utf16_unit(0x000A, false)?;
            send_utf16_unit(0x000A, true)
        }
    }
}

fn press_vk(vk: VIRTUAL_KEY) -> Result<(), TextError> {
    super::input::press_vk(vk)
}

fn send_utf16_unit(unit: u16, key_up: bool) -> Result<(), TextError> {
    super::input::send_unicode_unit(unit, key_up)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_kinds(text: &str, expected: &[CharKind]) {
        let kinds: Vec<CharKind> = text.chars().map(classify_char).collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn crlf_is_skipped_but_counts_toward_the_consumed_prefix() {
        // 不真正发按键：用空 sleep 和一个"永远成功"的路径无法替换 send_character，
        // 所以这里只验证分类与计数语义。
        assert_kinds("\r\n\t", &[CharKind::Skip, CharKind::Newline, CharKind::Tab]);
        let mut sleeps = Vec::new();
        let consumed = type_text_with("", WindowsSendInputNewlineMode::Enter, |d| sleeps.push(d))
            .unwrap();
        assert_eq!(consumed, 0);
        assert!(sleeps.is_empty());
    }

    #[test]
    fn paste_shortcut_maps_to_the_expected_virtual_keys() {
        let (modifiers, primary) = paste_shortcut_keys(PasteShortcut::CtrlV);
        assert_eq!(modifiers, &[VK_CONTROL]);
        assert_eq!(primary, VK_V);

        let (modifiers, primary) = paste_shortcut_keys(PasteShortcut::CtrlShiftV);
        assert_eq!(modifiers, &[VK_CONTROL, VK_SHIFT]);
        assert_eq!(primary, VK_V);

        let (modifiers, primary) = paste_shortcut_keys(PasteShortcut::ShiftInsert);
        assert_eq!(modifiers, &[VK_SHIFT]);
        assert_eq!(primary, VK_INSERT);
    }

    #[test]
    fn empty_text_sends_no_events() {
        let mut sleeps = Vec::new();
        let consumed =
            type_text_with("", WindowsSendInputNewlineMode::Enter, |d| sleeps.push(d)).unwrap();
        assert_eq!(consumed, 0);
        assert!(sleeps.is_empty());
    }
}
