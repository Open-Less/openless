//! Run the production mouse monitor without loading Tauri's ASR native DLLs.
mod types {
    pub use openless_core::shared_types::ShortcutBinding;
}

mod shortcut_binding {
    pub use openless_core::binding_requires_mouse_hook;
}

// Only the shared event/error DTOs are used, never the stub's registration methods.
#[allow(dead_code)]
#[path = "../../src/mobile_stubs/combo_hotkey.rs"]
mod combo_hotkey;

#[allow(dead_code)]
#[path = "../../src/mouse_dictation.rs"]
mod mouse_dictation;
