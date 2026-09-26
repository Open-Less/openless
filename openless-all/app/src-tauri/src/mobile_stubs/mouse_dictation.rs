//! Mobile stub — global mouse dictation triggers are unavailable on Android/iOS.

use std::sync::mpsc::Sender;

use crate::combo_hotkey::{ComboHotkeyError, ComboHotkeyEvent};
use crate::types::ShortcutBinding;

pub struct MouseDictationMonitor;

impl MouseDictationMonitor {
    pub fn start(
        _binding: ShortcutBinding,
        _tx: Sender<ComboHotkeyEvent>,
    ) -> Result<Self, ComboHotkeyError> {
        Err(ComboHotkeyError::RegisterFailed(
            "mouse dictation is not available on mobile".into(),
        ))
    }

    pub fn update_binding(&self, _binding: ShortcutBinding) -> Result<(), ComboHotkeyError> {
        Err(ComboHotkeyError::RegisterFailed(
            "mouse dictation is not available on mobile".into(),
        ))
    }
}

pub fn handle_button(_primary: &str, _pressed: bool) {}
