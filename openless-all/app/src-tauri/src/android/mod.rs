//! Android platform integration (JNI, overlay, accessibility, insert).

pub mod accessibility;
#[cfg(target_os = "android")]
pub mod insert;
pub mod insert_tiers;
pub mod jni;
pub mod native_bridge;
pub mod overlay;
pub mod shizuku;
#[cfg(target_os = "android")]
pub mod updater;
pub mod updater_logic;
pub use crate::types::android_types as types;

pub use accessibility::{
    get_android_accessibility_status, is_accessibility_enabled, paste_via_accessibility,
    paste_via_accessibility_with_result, request_android_accessibility_permission,
    AndroidAccessibilityPermissionResult,
};
#[cfg(target_os = "android")]
pub use insert::android_insert_with_strategy;
pub use native_bridge::{
    hide_overlay, is_overlay_visible, notify_capsule_state, refresh_overlay_if_visible,
    refresh_overlay_layout, register_android_backend, register_android_coordinator,
    replace_overlay, show_overlay,
};
pub use overlay::{
    get_android_overlay_status, hide_android_overlay, refresh_android_overlay_if_visible,
    refresh_android_overlay_layout, replace_android_overlay, request_android_overlay_permission,
    show_android_overlay, AndroidOverlayPermissionResult,
};
pub use shizuku::{
    get_android_shizuku_status, open_shizuku_app, paste_via_shizuku_with_result,
    recover_android_accessibility, request_android_shizuku_permission, AndroidShizukuOpenResult,
    AndroidShizukuPermissionResult,
};
