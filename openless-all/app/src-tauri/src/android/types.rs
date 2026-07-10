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
#[serde(rename_all = "snake_case")]
pub enum AndroidOverlayCancelSwipeDirection {
    Up,
    Down,
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

pub fn default_android_overlay_cancel_swipe_direction() -> AndroidOverlayCancelSwipeDirection {
    AndroidOverlayCancelSwipeDirection::Up
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidOverlaySettingsAction {
    None,
    RefreshLayout,
    Transition {
        from: AndroidOverlayTrigger,
        to: AndroidOverlayTrigger,
    },
}

pub fn classify_android_overlay_settings_change(
    previous: &super::UserPreferences,
    next: &super::UserPreferences,
) -> AndroidOverlaySettingsAction {
    let trigger_changed =
        previous.android_overlay_trigger.normalized() != next.android_overlay_trigger.normalized();
    let size_changed = normalize_android_overlay_size_dp(previous.android_overlay_size_dp)
        != normalize_android_overlay_size_dp(next.android_overlay_size_dp);

    if trigger_changed {
        return AndroidOverlaySettingsAction::Transition {
            from: previous.android_overlay_trigger.normalized(),
            to: next.android_overlay_trigger.normalized(),
        };
    }

    if size_changed {
        return AndroidOverlaySettingsAction::RefreshLayout;
    }

    AndroidOverlaySettingsAction::None
}

#[cfg(test)]
mod android_overlay_tests {
    use super::*;
    use crate::types::UserPreferences;

    fn overlay_prefs(
        trigger: AndroidOverlayTrigger,
        size_dp: u32,
        activation: AndroidOverlayActivationMode,
    ) -> UserPreferences {
        let mut prefs = UserPreferences::default();
        prefs.android_overlay_trigger = trigger;
        prefs.android_overlay_size_dp = size_dp;
        prefs.android_overlay_activation_mode = activation;
        prefs
    }

    #[test]
    fn size_only_change_returns_refresh_layout() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            96,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::RefreshLayout,
        );
    }

    #[test]
    fn trigger_only_change_returns_transition() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Background,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::Transition {
                from: AndroidOverlayTrigger::Background,
                to: AndroidOverlayTrigger::Always,
            },
        );
    }

    #[test]
    fn trigger_and_size_change_returns_transition_only() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Background,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            96,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::Transition {
                from: AndroidOverlayTrigger::Background,
                to: AndroidOverlayTrigger::Always,
            },
        );
    }

    #[test]
    fn activation_only_change_returns_none() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::LongPress,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::None,
        );
    }

    #[test]
    fn out_of_bounds_size_200_to_120_returns_none_after_normalize() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Always,
            200,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            120,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::None,
        );
    }

    #[test]
    fn out_of_bounds_size_below_min_normalizes_to_same_returns_none() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Always,
            30,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            48,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::None,
        );
    }

    #[test]
    fn identical_normalized_size_returns_none() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::None,
        );
    }

    #[test]
    fn keyboard_trigger_normalizes_to_background_for_transition() {
        let previous = overlay_prefs(
            AndroidOverlayTrigger::Keyboard,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        let next = overlay_prefs(
            AndroidOverlayTrigger::Always,
            72,
            AndroidOverlayActivationMode::Tap,
        );
        assert_eq!(
            classify_android_overlay_settings_change(&previous, &next),
            AndroidOverlaySettingsAction::Transition {
                from: AndroidOverlayTrigger::Background,
                to: AndroidOverlayTrigger::Always,
            },
        );
    }
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
