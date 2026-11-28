//! Production native keyboard dispatch and pure gesture tests, without Tauri/model runtimes.
#![allow(dead_code)]
mod types {
    pub use openless_core::shared_types::{
        HotkeyAdapterKind, HotkeyBinding, HotkeyCapability, HotkeyInstallError, HotkeyMode,
        HotkeyTrigger, ShortcutBinding,
    };
}
#[cfg(target_os = "macos")]
#[path = "../../src/shortcut_binding.rs"]
mod shortcut_binding;
#[cfg(target_os = "macos")]
mod macos_dictation_key { pub const PRIMARY: &str = "MacDictationKey"; }
#[cfg(not(target_os = "macos"))]
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
#[cfg(target_os = "macos")]
extern crate self as tauri;
#[cfg(target_os = "macos")]
pub mod async_runtime { pub use tokio::spawn; }
#[cfg(target_os = "macos")]
#[path = "support/translation_ask_host.rs"]
mod ask_window;
// Other platforms retain the stock route; no window/device runtime is needed.
#[cfg(not(target_os = "macos"))]
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
#[path = "../../src/capsule_style_menu.rs"]
mod capsule_style_menu;

#[cfg(target_os = "macos")]
#[tokio::test]
async fn skip_auto_style_held_recording_key_uses_real_ask_keyboard_route() {
    let _guard = side_aware_combo::TEST_MONITOR_LOCK.lock().await;
    let desktop = std::sync::Arc::new(ask_window::Desktop::default());
    ask_window::install_test_runtime(&desktop);
    ask_window::hotkey::set_binding(
        &types::ShortcutBinding { primary: "RightOption".into(), modifiers: vec![] },
        types::HotkeyMode::Hold,
    );
    hotkey::platform::tests::skip_auto_style_mac_held_ask_controller();
    // Drop desktop: subsequent cases revert to the stock route.
}

#[cfg(target_os = "macos")]
#[test]
fn style_menu_native_callback_preserves_key_pairs_across_open_close_and_cancel() {
    hotkey::platform::tests::style_menu_real_callback_preserves_key_pairs_across_open_close_and_cancel();
}
