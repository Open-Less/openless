//! Mobile stub — side-specific combo hotkeys are unavailable on Android/iOS.

use std::sync::mpsc::Sender;

use crate::combo_hotkey::{ComboHotkeyError, ComboHotkeyEvent};
use crate::types::ShortcutBinding;

#[derive(Debug, Clone, Copy)]
pub enum SideModifier {
    CmdLeft,
    CmdRight,
    CtrlLeft,
    CtrlRight,
    AltLeft,
    AltRight,
    ShiftLeft,
    ShiftRight,
}

pub struct SideAwareComboMonitor;

impl SideAwareComboMonitor {
    pub fn start(
        _binding: ShortcutBinding,
        _tx: Sender<ComboHotkeyEvent>,
    ) -> Result<Self, ComboHotkeyError> {
        Err(ComboHotkeyError::RegisterFailed(
            "Side-specific combo hotkeys are not available on mobile".into(),
        ))
    }
}

pub fn handle_side_modifier(_side: SideModifier, _pressed: bool) {}

pub fn handle_primary_key(_primary: &str, _pressed: bool) {}

#[cfg(target_os = "macos")]
pub mod platform {
    pub fn dispatch_keycode(_keycode: i64, _flags_changed: bool, _flags: u64, _pressed: bool) {}
}

#[cfg(target_os = "windows")]
pub mod platform {
    pub fn dispatch_vk(_vk_code: u32, _pressed: bool) {}
}
