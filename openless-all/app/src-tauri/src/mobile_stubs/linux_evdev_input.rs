//! Mobile stub — Linux evdev input is unavailable on Android/iOS.

use crate::hold_source_tracker::TriggerSource;
use crate::hotkey::HotkeyEvent;
use crate::mouse_dictation::MouseDictationConfig;
use crate::types::ShortcutBinding;

pub struct LinuxEvdevConfig {
    pub side_combo: Option<ShortcutBinding>,
    pub mouse: MouseDictationConfig,
}

pub struct LinuxEvdevMonitor;

impl LinuxEvdevMonitor {
    pub fn start(
        _config: LinuxEvdevConfig,
        _dictation_tx: std::sync::mpsc::Sender<(HotkeyEvent, TriggerSource)>,
    ) -> Result<Self, String> {
        Err("linux evdev is not available on mobile".into())
    }
}

pub fn status_message() -> Option<String> {
    None
}

pub fn set_status_message(_message: Option<String>) {}

pub fn is_available() -> bool {
    false
}
