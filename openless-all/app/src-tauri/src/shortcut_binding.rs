//! Shared parsing/validation for user-configurable shortcut bindings.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

use crate::types::{HotkeyTrigger, ShortcutBinding};

#[derive(Debug, thiserror::Error)]
pub enum ShortcutBindingError {
    #[error("不支持的修饰键: {0}")]
    UnsupportedModifier(String),
    #[error("不支持的主键: {0}")]
    UnsupportedKey(String),
}

const SIDE_MODIFIER_TAGS: &[&str] = &[
    "cmd-left",
    "cmd-right",
    "ctrl-left",
    "ctrl-right",
    "alt-left",
    "alt-right",
    "shift-left",
    "shift-right",
    "super-left",
    "super-right",
];

pub fn is_side_specific_modifier_tag(raw: &str) -> bool {
    SIDE_MODIFIER_TAGS.contains(&normalize_side_modifier_tag(raw).as_str())
}

pub fn binding_requires_side_aware_hook(binding: &ShortcutBinding) -> bool {
    !binding.modifiers.is_empty()
        && binding
            .modifiers
            .iter()
            .any(|tag| is_side_specific_modifier_tag(tag))
}

pub const SIDE_SPECIFIC_NON_DICTATION_MSG: &str =
    "Side-specific modifier shortcuts are only supported for dictation start/stop.";

pub fn reject_side_specific_non_dictation(binding: &ShortcutBinding) -> Result<(), String> {
    if binding_requires_side_aware_hook(binding) {
        return Err(SIDE_SPECIFIC_NON_DICTATION_MSG.to_string());
    }
    Ok(())
}

fn physical_modifier_class(raw: &str) -> String {
    let tag = normalize_side_modifier_tag(raw);
    if is_side_specific_modifier_tag(&tag) {
        if tag.starts_with("cmd-") || tag.starts_with("super-") {
            return "Super".to_string();
        }
        if tag.starts_with("ctrl-") {
            return "Control".to_string();
        }
        if tag.starts_with("alt-") {
            return "Alt".to_string();
        }
        if tag.starts_with("shift-") {
            return "Shift".to_string();
        }
    }
    physical_class_from_generic_tag(&normalize_modifier_tag(raw))
}

fn physical_class_from_generic_tag(tag: &str) -> String {
    match tag {
        "ctrl" | "control" => "Control".to_string(),
        "alt" | "option" | "opt" => "Alt".to_string(),
        "shift" => "Shift".to_string(),
        #[cfg(target_os = "windows")]
        "cmd" | "command" => "Control".to_string(),
        #[cfg(target_os = "windows")]
        "super" | "meta" | "win" => "Super".to_string(),
        #[cfg(not(target_os = "windows"))]
        "cmd" | "command" | "super" | "meta" | "win" => "Super".to_string(),
        other => other.to_string(),
    }
}

fn physical_modifier_set(binding: &ShortcutBinding) -> std::collections::BTreeSet<String> {
    binding
        .modifiers
        .iter()
        .map(|raw| physical_modifier_class(raw))
        .collect()
}

/// Returns true when two bindings would compete for the same physical shortcut.
pub fn bindings_overlap(left: &ShortcutBinding, right: &ShortcutBinding) -> bool {
    let left_legacy = legacy_modifier_trigger(left);
    let right_legacy = legacy_modifier_trigger(right);
    match (left_legacy, right_legacy) {
        (Some(left), Some(right)) => left == right,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => {
            if normalize_primary(&left.primary) != normalize_primary(&right.primary) {
                return false;
            }
            let left_side = binding_requires_side_aware_hook(left);
            let right_side = binding_requires_side_aware_hook(right);
            if left_side && right_side {
                let left_mods: std::collections::BTreeSet<String> = left
                    .modifiers
                    .iter()
                    .map(|raw| normalize_side_modifier_tag(raw))
                    .collect();
                let right_mods: std::collections::BTreeSet<String> = right
                    .modifiers
                    .iter()
                    .map(|raw| normalize_side_modifier_tag(raw))
                    .collect();
                return left_mods == right_mods;
            }
            physical_modifier_set(left) == physical_modifier_set(right)
        }
    }
}

pub fn validate_binding(binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    if legacy_modifier_trigger(binding).is_some() {
        return Ok(());
    }
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Ok(());
    }
    if binding_requires_side_aware_hook(binding) {
        parse_primary(&binding.primary)?;
        for raw in &binding.modifiers {
            if !is_side_specific_modifier_tag(raw) {
                return Err(ShortcutBindingError::UnsupportedModifier(raw.clone()));
            }
        }
        return Ok(());
    }
    parse_global_hotkey(binding)?;
    Ok(())
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

pub fn legacy_modifier_trigger(binding: &ShortcutBinding) -> Option<HotkeyTrigger> {
    if !binding.modifiers.is_empty() {
        return None;
    }
    match normalize_primary(&binding.primary).as_str() {
        "rightoption" | "rightalt" => Some(HotkeyTrigger::RightOption),
        "leftoption" | "leftalt" => Some(HotkeyTrigger::LeftOption),
        "rightcontrol" | "rightctrl" => Some(HotkeyTrigger::RightControl),
        "leftcontrol" | "leftctrl" => Some(HotkeyTrigger::LeftControl),
        "rightcommand" | "rightcmd" | "rightsuper" | "rightmeta" => {
            Some(HotkeyTrigger::RightCommand)
        }
        "leftcommand" | "leftcmd" | "leftsuper" | "leftmeta" => Some(HotkeyTrigger::LeftCommand),
        "leftshift" => Some(HotkeyTrigger::LeftShift),
        "rightshift" => Some(HotkeyTrigger::RightShift),
        "shiftleft" => Some(HotkeyTrigger::LeftShift),
        "shiftright" => Some(HotkeyTrigger::RightShift),
        "fn" | "function" => Some(HotkeyTrigger::Fn),
        "mediaplaypause" | "mediaplay" | "playpause" => Some(HotkeyTrigger::MediaPlayPause),
        _ => None,
    }
}

pub fn binding_from_legacy_trigger(trigger: HotkeyTrigger) -> ShortcutBinding {
    let primary = match trigger {
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => "RightOption",
        HotkeyTrigger::LeftOption => "LeftOption",
        HotkeyTrigger::RightControl => "RightControl",
        HotkeyTrigger::LeftControl => "LeftControl",
        HotkeyTrigger::RightCommand => "RightCommand",
        HotkeyTrigger::LeftCommand => "LeftCommand",
        HotkeyTrigger::LeftShift => "LeftShift",
        HotkeyTrigger::RightShift => "RightShift",
        HotkeyTrigger::Fn => "Fn",
        HotkeyTrigger::MediaPlayPause => "MediaPlayPause",
        HotkeyTrigger::Custom => "RightOption",
    };
    ShortcutBinding {
        primary: primary.into(),
        modifiers: Vec::new(),
    }
}

pub fn normalize_side_modifier_tag(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "super-left" => "cmd-left".into(),
        "super-right" => "cmd-right".into(),
        tag => tag.to_string(),
    }
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

fn normalize_primary(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn parse_primary(raw: &str) -> Result<Code, ShortcutBindingError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combo_and_single_key() {
        let combo = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let parsed = parse_global_hotkey(&combo).expect("combo parses");
        #[cfg(target_os = "windows")]
        assert!(parsed.mods.contains(Modifiers::CONTROL));
        #[cfg(not(target_os = "windows"))]
        assert!(parsed.mods.contains(Modifiers::SUPER));
        assert!(parsed.mods.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::KeyD);

        let single = ShortcutBinding {
            primary: "F8".into(),
            modifiers: vec![],
        };
        let parsed = parse_global_hotkey(&single).expect("single key parses");
        assert!(parsed.mods.is_empty());
        assert_eq!(parsed.key, Code::F8);
    }

    #[test]
    fn detects_legacy_modifier_only() {
        let binding = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };
        assert_eq!(
            legacy_modifier_trigger(&binding),
            Some(HotkeyTrigger::RightControl)
        );
        let left_cmd = ShortcutBinding {
            primary: "LeftCommand".into(),
            modifiers: vec![],
        };
        assert_eq!(
            legacy_modifier_trigger(&left_cmd),
            Some(HotkeyTrigger::LeftCommand)
        );
    }

    #[test]
    fn side_specific_combo_requires_side_aware_hook() {
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd-left".into(), "shift-right".into()],
        };
        assert!(binding_requires_side_aware_hook(&binding));
        assert!(validate_binding(&binding).is_ok());
    }

    #[test]
    fn generic_combo_uses_global_hotkey_path() {
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        assert!(!binding_requires_side_aware_hook(&binding));
        assert!(parse_global_hotkey(&binding).is_ok());
    }

    #[test]
    fn super_side_aliases_normalize_to_cmd() {
        assert_eq!(normalize_side_modifier_tag("super-left"), "cmd-left");
        assert_eq!(normalize_side_modifier_tag("Super-Right"), "cmd-right");
        assert!(is_side_specific_modifier_tag("super-left"));
    }

    #[test]
    fn accepts_shifted_printable_aliases() {
        let cases = [
            ("?", Code::Slash),
            ("!", Code::Digit1),
            (":", Code::Semicolon),
            ("+", Code::Equal),
            ("_", Code::Minus),
            ("{", Code::BracketLeft),
            ("|", Code::Backslash),
        ];
        for (primary, expected) in cases {
            let binding = ShortcutBinding {
                primary: primary.into(),
                modifiers: vec!["shift".into()],
            };
            let parsed = parse_global_hotkey(&binding).expect("shifted printable parses");
            assert_eq!(parsed.key, expected);
            assert!(parsed.mods.contains(Modifiers::SHIFT));
        }
    }

    fn combo(primary: &str, modifiers: Vec<&str>) -> ShortcutBinding {
        ShortcutBinding {
            primary: primary.into(),
            modifiers: modifiers.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn side_combo_overlaps_generic_combo_with_same_physical_modifiers() {
        #[cfg(target_os = "windows")]
        {
            assert!(!bindings_overlap(
                &combo("D", vec!["cmd-left"]),
                &combo("D", vec!["cmd"]),
            ));
            assert!(bindings_overlap(
                &combo("D", vec!["cmd-left"]),
                &combo("D", vec!["super"]),
            ));
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(bindings_overlap(
                &combo("D", vec!["cmd-left"]),
                &combo("D", vec!["cmd"]),
            ));
        }
    }

    #[test]
    fn side_combo_does_not_overlap_generic_combo_with_extra_modifier() {
        assert!(!bindings_overlap(
            &combo("D", vec!["cmd-left"]),
            &combo("D", vec!["cmd", "shift"]),
        ));
    }

    #[test]
    fn side_combos_with_different_sides_do_not_overlap() {
        assert!(!bindings_overlap(
            &combo("D", vec!["cmd-left"]),
            &combo("D", vec!["cmd-right"]),
        ));
    }

    #[test]
    fn rejects_side_specific_non_dictation_shortcut() {
        let binding = combo("D", vec!["cmd-left"]);
        assert!(binding_requires_side_aware_hook(&binding));
        assert_eq!(
            reject_side_specific_non_dictation(&binding).unwrap_err(),
            SIDE_SPECIFIC_NON_DICTATION_MSG,
        );
        assert!(reject_side_specific_non_dictation(&combo("D", vec!["cmd"])).is_ok());
    }
}
