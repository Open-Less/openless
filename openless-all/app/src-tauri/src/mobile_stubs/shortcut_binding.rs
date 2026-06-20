//! Mobile stub — shortcut binding validation is unavailable on mobile.

use crate::types::{HotkeyTrigger, ShortcutBinding};

#[derive(Debug, thiserror::Error)]
pub enum ShortcutBindingError {
    #[error("快捷键在移动端不可用")]
    Unavailable,
}

pub fn validate_binding(_binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    Err(ShortcutBindingError::Unavailable)
}

pub fn parse_global_hotkey(_binding: &ShortcutBinding) -> Result<(), ShortcutBindingError> {
    Err(ShortcutBindingError::Unavailable)
}

pub fn is_side_specific_modifier_tag(_raw: &str) -> bool {
    false
}

pub fn binding_requires_side_aware_hook(_binding: &ShortcutBinding) -> bool {
    false
}

pub const SIDE_SPECIFIC_NON_DICTATION_MSG: &str =
    "Side-specific modifier shortcuts are only supported for dictation start/stop.";

pub fn reject_side_specific_non_dictation(_binding: &ShortcutBinding) -> Result<(), String> {
    Ok(())
}

pub fn bindings_overlap(_left: &ShortcutBinding, _right: &ShortcutBinding) -> bool {
    false
}

pub fn normalize_side_modifier_tag(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

pub fn legacy_modifier_trigger(_binding: &ShortcutBinding) -> Option<HotkeyTrigger> {
    None
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
