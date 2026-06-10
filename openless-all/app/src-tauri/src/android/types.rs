//! Android-specific preference types and status payloads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AndroidInsertStrategy {
    Auto,
    Ime,
    Accessibility,
    Clipboard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AndroidOverlayTrigger {
    Background,
    Keyboard,
    Always,
}

impl AndroidOverlayTrigger {
    pub fn normalized(self) -> Self {
        match self {
            AndroidOverlayTrigger::Keyboard => AndroidOverlayTrigger::Background,
            trigger => trigger,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AndroidOverlayActivationMode {
    Tap,
    LongPress,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AndroidOverlayLeftSwipeAction {
    Translation,
    StylePack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AndroidAccessibilityState {
    Enabled,
    NotEnabled,
    NotAndroid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAccessibilityStatus {
    pub state: AndroidAccessibilityState,
    pub enabled: bool,
    pub message: String,
}

pub fn default_android_insert_strategy() -> AndroidInsertStrategy {
    AndroidInsertStrategy::Accessibility
}

pub fn default_android_overlay_trigger() -> AndroidOverlayTrigger {
    AndroidOverlayTrigger::Background
}

pub fn default_android_overlay_activation_mode() -> AndroidOverlayActivationMode {
    AndroidOverlayActivationMode::Tap
}

pub fn default_android_overlay_left_swipe_action() -> AndroidOverlayLeftSwipeAction {
    AndroidOverlayLeftSwipeAction::Translation
}

pub fn default_android_overlay_size_dp() -> u32 {
    72
}

pub fn normalize_android_insert_strategy(strategy: AndroidInsertStrategy) -> AndroidInsertStrategy {
    match strategy {
        AndroidInsertStrategy::Auto | AndroidInsertStrategy::Ime => {
            AndroidInsertStrategy::Accessibility
        }
        strategy => strategy,
    }
}

pub fn normalize_android_overlay_size_dp(size_dp: u32) -> u32 {
    size_dp.clamp(48, 120)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AndroidOverlayPermissionState {
    Granted,
    NotGranted,
    NotAndroid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AndroidOverlayStatus {
    pub permission: AndroidOverlayPermissionState,
    pub overlay_visible: bool,
    pub message: String,
}
