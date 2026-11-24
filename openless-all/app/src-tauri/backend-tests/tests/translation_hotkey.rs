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
#[path = "../../src/mobile_stubs/combo_hotkey.rs"]
mod combo_hotkey;
#[path = "../../src/hotkey.rs"]
mod hotkey;
#[path = "../../src/recording_companion.rs"]
mod recording_companion;
#[path = "../../src/side_aware_combo.rs"]
mod side_aware_combo;
