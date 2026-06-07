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
        HotkeyTrigger::Fn => "Fn",
        HotkeyTrigger::MediaPlayPause => "MediaPlayPause",
        HotkeyTrigger::Custom => "RightOption",
    };
    ShortcutBinding {
        primary: primary.into(),
        modifiers: Vec::new(),
    }
}
