//! Mobile stub — global mouse dictation triggers are unavailable on Android/iOS.

use std::sync::mpsc::Sender;

use crate::hold_source_tracker::TriggerSource;
use crate::hotkey::HotkeyEvent;

pub struct MouseDictationConfig {
    pub middle_enabled: bool,
    pub side_enabled: bool,
}

pub struct MouseDictationMonitor;

impl MouseDictationMonitor {
    pub fn start(
        _config: MouseDictationConfig,
        _tx: Sender<(HotkeyEvent, TriggerSource)>,
    ) -> Result<Self, String> {
        Err("mouse dictation is not available on mobile".into())
    }

    pub fn update_config(_config: MouseDictationConfig) {}
}

#[cfg(target_os = "windows")]
pub mod platform {
    pub fn ensure_hook_thread() -> Result<(), String> {
        Err("mouse dictation is not available on mobile".into())
    }

    pub fn dispatch_button_number(_button: i64, _pressed: bool) {}
}

#[cfg(target_os = "macos")]
pub mod platform {
    pub fn dispatch_button_number(_button: i64, _pressed: bool) {}
}
