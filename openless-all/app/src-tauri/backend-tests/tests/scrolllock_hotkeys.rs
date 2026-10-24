//! Execute the production Windows side-aware mapper without Tauri's native ASR DLLs.
#![cfg(target_os = "windows")]

#[path = "../../../crates/openless-core/tests/scrolllock_hotkeys.rs"]
mod core_contract;

mod types {
    pub use openless_core::shared_types::{
        HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError, HotkeyTrigger,
        ShortcutBinding,
    };
}

mod shortcut_binding {
    pub use openless_core::{is_side_specific_modifier_tag, normalize_side_modifier_tag};

    // The monitor needs input validation only; the native parser has its own app tests.
    pub fn parse_primary(primary: &str) -> Result<(), openless_core::ShortcutBindingError> {
        openless_core::validate_shortcut_binding(&crate::types::ShortcutBinding {
            primary: primary.into(),
            modifiers: vec![],
        })
    }
}

// Reuse only the event/error DTOs, not these stubs' registration methods.
#[allow(dead_code)]
#[path = "../../src/mobile_stubs/hotkey.rs"]
mod hotkey;

#[allow(dead_code)]
#[path = "../../src/mobile_stubs/combo_hotkey.rs"]
mod combo_hotkey;

#[allow(dead_code)]
#[path = "../../src/recording_companion.rs"]
mod recording_companion;

#[allow(dead_code)]
#[path = "../../src/side_aware_combo.rs"]
mod side_aware_combo;
