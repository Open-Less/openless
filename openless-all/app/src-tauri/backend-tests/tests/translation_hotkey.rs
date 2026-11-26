//! Production native keyboard dispatch and pure gesture tests, without Tauri/model runtimes.
#![allow(dead_code)]
mod types {
    pub use openless_core::shared_types::{
        HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError, HotkeyMode,
        HotkeyTrigger, ShortcutBinding,
    };
}
mod shortcut_binding {
    pub use openless_core::{is_side_specific_modifier_tag, normalize_side_modifier_tag};
    pub fn parse_primary(primary: &str) -> Result<(), openless_core::ShortcutBindingError> {
        openless_core::validate_shortcut_binding(&crate::types::ShortcutBinding {
            primary: primary.into(),
            modifiers: vec![],
        })
    }
}
mod selection {
    // Selection capture is unrelated to keyboard classification in this test harness.
    pub fn prefetch_selection_workspace_capture() {}
}
// This harness leaves Ask unconfigured so the production keyboard dispatcher
// continues through the dictation/translation route under test.
mod ask_window {
    pub fn active_space_epoch() -> u64 { 0 }
    pub fn foreground_pid() -> Option<u32> { None }
    pub mod hotkey {
        pub enum Disposition { Stock, Pass, Consume }
        pub fn keyboard_edge(_: u16, _: bool, _: bool, _: bool) -> Disposition { Disposition::Stock }
        pub fn reset_physical_state() {}
        pub fn can_buffer_space() -> bool { false }
        pub fn fn_is_held() -> bool { false }
        pub fn abort_fn_companion() {}
    }
}
mod unicode_keystroke {
    pub fn is_secure_input_enabled() -> bool { false }
}
#[path = "../../src/mobile_stubs/combo_hotkey.rs"]
mod combo_hotkey;
#[path = "../../src/hotkey.rs"]
mod hotkey;
#[path = "../../src/recording_companion.rs"]
mod recording_companion;
#[path = "../../src/side_aware_combo.rs"]
mod side_aware_combo;
