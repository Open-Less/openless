//! 快捷键解析：主键字符串 → `global-hotkey` 的 `Code`，完整绑定 → `HotKey`。
//! 侧键组合校验、组合键注册、鼠标听写共用同一份实现。

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use openless_core::shared_types::ShortcutBinding;
use openless_core::{is_side_specific_modifier_tag, ShortcutBindingError};

pub fn parse_primary(raw: &str) -> Result<Code, ShortcutBindingError> {
    let raw = if raw == " " { "Space" } else { raw };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ShortcutBindingError::UnsupportedKey("(空)".into()));
    }
    if trimmed.chars().count() == 1 {
        let ch = trimmed.chars().next().unwrap();
        if let Some(code) = char_to_code(ch) {
            return Ok(code);
        }
    }
    let upper = trimmed.to_ascii_uppercase();
    let named = match upper.as_str() {
        "ENTER" | "RETURN" => Code::Enter,
        "TAB" => Code::Tab,
        "ESC" | "ESCAPE" => Code::Escape,
        "SPACE" => Code::Space,
        "SCROLLLOCK" => Code::ScrollLock,
        "BACKSPACE" => Code::Backspace,
        "DELETE" | "DEL" => Code::Delete,
        "HOME" => Code::Home,
        "END" => Code::End,
        "PAGEUP" => Code::PageUp,
        "PAGEDOWN" => Code::PageDown,
        "ARROWUP" | "UP" => Code::ArrowUp,
        "ARROWDOWN" | "DOWN" => Code::ArrowDown,
        "ARROWLEFT" | "LEFT" => Code::ArrowLeft,
        "ARROWRIGHT" | "RIGHT" => Code::ArrowRight,
        "F1" => Code::F1,
        "F2" => Code::F2,
        "F3" => Code::F3,
        "F4" => Code::F4,
        "F5" => Code::F5,
        "F6" => Code::F6,
        "F7" => Code::F7,
        "F8" => Code::F8,
        "F9" => Code::F9,
        "F10" => Code::F10,
        "F11" => Code::F11,
        "F12" => Code::F12,
        "F13" => Code::F13,
        "F14" => Code::F14,
        "F15" => Code::F15,
        "F16" => Code::F16,
        "F17" => Code::F17,
        "F18" => Code::F18,
        "F19" => Code::F19,
        "F20" => Code::F20,
        _ => return Err(ShortcutBindingError::UnsupportedKey(trimmed.to_string())),
    };
    Ok(named)
}

fn char_to_code(ch: char) -> Option<Code> {
    let c = ch.to_ascii_uppercase();
    let code = match c {
        'A' => Code::KeyA,
        'B' => Code::KeyB,
        'C' => Code::KeyC,
        'D' => Code::KeyD,
        'E' => Code::KeyE,
        'F' => Code::KeyF,
        'G' => Code::KeyG,
        'H' => Code::KeyH,
        'I' => Code::KeyI,
        'J' => Code::KeyJ,
        'K' => Code::KeyK,
        'L' => Code::KeyL,
        'M' => Code::KeyM,
        'N' => Code::KeyN,
        'O' => Code::KeyO,
        'P' => Code::KeyP,
        'Q' => Code::KeyQ,
        'R' => Code::KeyR,
        'S' => Code::KeyS,
        'T' => Code::KeyT,
        'U' => Code::KeyU,
        'V' => Code::KeyV,
        'W' => Code::KeyW,
        'X' => Code::KeyX,
        'Y' => Code::KeyY,
        'Z' => Code::KeyZ,
        '0' | ')' => Code::Digit0,
        '1' | '!' => Code::Digit1,
        '2' | '@' => Code::Digit2,
        '3' | '#' => Code::Digit3,
        '4' | '$' => Code::Digit4,
        '5' | '%' => Code::Digit5,
        '6' | '^' => Code::Digit6,
        '7' | '&' => Code::Digit7,
        '8' | '*' => Code::Digit8,
        '9' | '(' => Code::Digit9,
        ';' | ':' => Code::Semicolon,
        ',' | '<' => Code::Comma,
        '.' | '>' => Code::Period,
        '/' | '?' => Code::Slash,
        '\\' | '|' => Code::Backslash,
        '[' | '{' => Code::BracketLeft,
        ']' | '}' => Code::BracketRight,
        '\'' | '"' => Code::Quote,
        '`' | '~' => Code::Backquote,
        '-' | '_' => Code::Minus,
        '=' | '+' => Code::Equal,
        ' ' => Code::Space,
        _ => return None,
    };
    Some(code)
}

pub fn parse_global_hotkey(binding: &ShortcutBinding) -> Result<HotKey, ShortcutBindingError> {
    let mut mods = Modifiers::empty();
    for raw in &binding.modifiers {
        let tag = normalize_modifier_tag(raw);
        let bit = match tag.as_str() {
            "cmd" | "command" | "super" | "meta" | "win" => Modifiers::SUPER,
            "ctrl" | "control" => Modifiers::CONTROL,
            "alt" | "option" | "opt" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            other => return Err(ShortcutBindingError::UnsupportedModifier(other.to_string())),
        };
        mods |= bit;
    }
    let code = parse_primary(&binding.primary)?;
    let mods = if mods.is_empty() { None } else { Some(mods) };
    Ok(HotKey::new(mods, code))
}

fn normalize_modifier_tag(raw: &str) -> String {
    let tag = raw.trim().to_ascii_lowercase();
    if is_side_specific_modifier_tag(&tag) {
        return tag;
    }
    #[cfg(target_os = "windows")]
    {
        if matches!(tag.as_str(), "cmd" | "command") {
            return "ctrl".to_string();
        }
    }
    tag
}
