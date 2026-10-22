use serde::{Deserialize, Serialize};

use crate::errors::BackendError;
use crate::shared_types::{
    HotkeyMode, ShortcutBinding, StylePackHotkey, UserPreferences, WindowsInsertionMode,
};

pub(crate) fn ensure_testing_features_enabled(
    preferences: &UserPreferences,
) -> Result<(), BackendError> {
    if preferences.testing_features_enabled {
        Ok(())
    } else {
        Err(BackendError::new(
            crate::BackendErrorCode::PermissionDenied,
            "Enable features under testing in Advanced settings first",
        )
        .retryable(false))
    }
}

/// Preserve historical custom names on load, while canonicalizing catalog aliases.
pub(crate) fn canonical_translation_target(value: &str) -> String {
    crate::language_catalog::native_language_name(value).unwrap_or_else(|| value.trim().to_string())
}

pub(crate) fn ordered_translation_targets(values: Vec<String>) -> Vec<String> {
    let mut targets = Vec::new();
    for value in values {
        let value = canonical_translation_target(&value);
        if !value.is_empty() && !targets.contains(&value) {
            targets.push(value);
        }
    }
    targets
}

pub(crate) fn migrate_translation_targets(
    targets: Option<Vec<String>>,
    selected: &str,
) -> (Vec<String>, String) {
    let selected = canonical_translation_target(selected);
    let mut targets =
        ordered_translation_targets(targets.unwrap_or_else(|| vec![selected.clone()]));
    // A nonempty legacy selection remains authoritative when an older client
    // round-trips a newly introduced list it does not know how to update.
    if !targets.is_empty() && !selected.is_empty() && !targets.contains(&selected) {
        targets.push(selected.clone());
    }
    let selected = if targets.is_empty() {
        String::new()
    } else {
        selected
    };
    (targets, selected)
}

/// A missing list is a legacy scalar-only update, not permission to erase targets.
/// Unknown names already stored by an older client remain usable, but cannot be newly added.
pub(crate) fn reconcile_translation_settings(
    previous: &UserPreferences,
    next: &mut UserPreferences,
) -> Result<(), BackendError> {
    let (previous_targets, previous_selected) = migrate_translation_targets(
        if previous.translation_target_languages.is_empty()
            && !previous.translation_target_languages_present
        {
            None
        } else {
            Some(previous.translation_target_languages.clone())
        },
        &previous.translation_target_language,
    );
    // A legacy client can echo the initially empty list while selecting its first language.
    // An empty list replacing stored choices remains an explicit deletion.
    let input = if previous_targets.is_empty() && next.translation_target_languages.is_empty() {
        next.translation_target_language_input.as_deref()
            .unwrap_or(&next.translation_target_language)
    } else {
        &next.translation_target_language
    };
    let selected = canonical_translation_target(input);
    let mut targets = if next.translation_target_languages_present {
        ordered_translation_targets(next.translation_target_languages.clone())
    } else {
        previous_targets.clone()
    };
    // Scalar-only clients (including existing Rust callers editing a snapshot) select
    // a target without replacing the ordered collection. Explicit [] always clears.
    if (!next.translation_target_languages_present
        || (targets == previous_targets && selected != previous_selected))
        && !selected.is_empty()
        && !targets.contains(&selected)
    {
        targets.push(selected.clone());
    }
    for target in &targets {
        if crate::language_catalog::native_language_name(target).is_none()
            && !previous_targets.contains(target)
        {
            return Err(BackendError::new(
                crate::errors::BackendErrorCode::InvalidArgument,
                format!("unknown translation target: {target}"),
            ));
        }
    }
    let selected = if !selected.is_empty() && !targets.contains(&selected) {
        targets.first().cloned().unwrap_or_default()
    } else {
        selected
    };
    let (targets, selected) = migrate_translation_targets(Some(targets), &selected);
    next.translation_target_languages = targets;
    next.translation_target_language = selected;
    next.translation_target_languages_present = true;
    next.translation_target_language_input = None;
    Ok(())
}

pub(crate) fn validate_session_translation_target(
    preferences: &UserPreferences,
    target: &str,
) -> Result<String, BackendError> {
    let target = canonical_translation_target(target);
    let mut targets = ordered_translation_targets(preferences.translation_target_languages.clone());
    if !preferences.translation_target_languages_present {
        let legacy = canonical_translation_target(&preferences.translation_target_language);
        if !legacy.is_empty() && !targets.contains(&legacy) {
            targets.push(legacy);
        }
    }
    if target.is_empty() || !targets.contains(&target) {
        return Err(BackendError::new(
            crate::errors::BackendErrorCode::InvalidArgument,
            "session translation target must be a configured nonempty target",
        ));
    }
    Ok(target)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsCollisionPolicy {
    Reject,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateOptions {
    pub preserve_current_style: bool,
    pub collision_policy: SettingsCollisionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_preferences_revision: Option<u64>,
}

impl SettingsUpdateOptions {
    pub const STRICT: Self = Self {
        preserve_current_style: false,
        collision_policy: SettingsCollisionPolicy::Reject,
        expected_preferences_revision: None,
    };

    pub const SETTINGS_DOCUMENT: Self = Self {
        preserve_current_style: true,
        collision_policy: SettingsCollisionPolicy::Reconcile,
        expected_preferences_revision: None,
    };

    pub const fn at_revision(mut self, revision: u64) -> Self {
        self.expected_preferences_revision = Some(revision);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyRuntimeTarget {
    pub dictation: ShortcutBinding,
    pub dictation_mode: HotkeyMode,
    pub qa: Option<ShortcutBinding>,
    #[serde(default)]
    pub qa_companion_key: String,
    pub quick_note: Option<ShortcutBinding>,
    pub translation: ShortcutBinding,
    pub switch_style: Option<ShortcutBinding>,
    pub open_app: Option<ShortcutBinding>,
    pub selection_polish: Option<ShortcutBinding>,
    pub coding_agent_enabled: bool,
    pub coding_agent_voice: Option<ShortcutBinding>,
    #[serde(default)]
    pub coding_agent_panel: Option<ShortcutBinding>,
    #[serde(default)]
    pub coding_agent_quick: Option<ShortcutBinding>,
    pub style_packs: Vec<StylePackHotkey>,
}

impl From<&UserPreferences> for HotkeyRuntimeTarget {
    fn from(preferences: &UserPreferences) -> Self {
        Self {
            dictation: preferences.dictation_hotkey.clone(),
            dictation_mode: preferences.hotkey.mode,
            qa: if cfg!(target_os = "linux") { preferences.qa_hotkey.clone() } else { None },
            qa_companion_key: preferences.qa_companion_key.clone(),
            quick_note: preferences.quick_note_hotkey.clone(),
            translation: preferences.translation_hotkey.clone(),
            switch_style: preferences.switch_style_hotkey.clone(),
            open_app: preferences.open_app_hotkey.clone(),
            selection_polish: preferences.selection_polish_hotkey.clone(),
            coding_agent_enabled: preferences.less_computer_available(),
            coding_agent_voice: preferences.coding_agent_voice_hotkey.clone(),
            coding_agent_panel: preferences.coding_agent_panel_hotkey.clone(),
            coding_agent_quick: preferences.coding_agent_quick_hotkey.clone(),
            style_packs: preferences.style_pack_hotkeys.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsKeyboardRuntimeTarget {
    pub openless_language_profile_enabled: bool,
}

impl From<&UserPreferences> for WindowsKeyboardRuntimeTarget {
    fn from(preferences: &UserPreferences) -> Self {
        Self {
            openless_language_profile_enabled: !matches!(
                preferences.windows_insertion_mode,
                WindowsInsertionMode::SendInput | WindowsInsertionMode::Paste
            ) || preferences
                .windows_show_openless_in_keyboard_list,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsValueChange<T> {
    pub previous: T,
    pub next: T,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsEffectPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkeys: Option<SettingsValueChange<HotkeyRuntimeTarget>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_asr_provider: Option<SettingsValueChange<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_at_login: Option<SettingsValueChange<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_keyboard: Option<SettingsValueChange<WindowsKeyboardRuntimeTarget>>,
}

impl SettingsEffectPlan {
    pub fn between(previous: &UserPreferences, next: &UserPreferences) -> Self {
        fn changed<T: PartialEq>(previous: T, next: T) -> Option<SettingsValueChange<T>> {
            (previous != next).then_some(SettingsValueChange { previous, next })
        }

        Self {
            hotkeys: changed(previous.into(), next.into()),
            active_asr_provider: changed(
                previous.active_asr_provider.clone(),
                next.active_asr_provider.clone(),
            ),
            launch_at_login: changed(previous.launch_at_login, next.launch_at_login),
            windows_keyboard: changed(previous.into(), next.into()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.hotkeys.is_none()
            && self.active_asr_provider.is_none()
            && self.launch_at_login.is_none()
            && self.windows_keyboard.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsEffectKind {
    WindowsKeyboard,
    ActiveAsrProvider,
    LaunchAtLogin,
    Hotkeys,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsEffectReceipt {
    pub applied: Vec<SettingsEffectKind>,
}

#[derive(Debug, Clone)]
pub struct SettingsEffectFailure {
    pub error: BackendError,
    pub receipt: SettingsEffectReceipt,
}

impl SettingsEffectFailure {
    pub fn before_side_effect(error: BackendError) -> Self {
        Self {
            error,
            receipt: SettingsEffectReceipt::default(),
        }
    }

    pub fn after_side_effect(error: BackendError, receipt: SettingsEffectReceipt) -> Self {
        Self { error, receipt }
    }
}

/// Platform adapter for the settings transaction.
///
/// `prepare` and `commit` both run before preferences are persisted. Adapters
/// must consume only the explicit targets in `SettingsEffectPlan`; they must not
/// read a staged settings document. `restore` must be idempotent and restore only
/// the effects named by the receipt.
pub trait SettingsRuntime: Send + Sync {
    fn prepare(
        &self,
        _plan: &SettingsEffectPlan,
    ) -> Result<SettingsEffectReceipt, SettingsEffectFailure> {
        Ok(SettingsEffectReceipt::default())
    }

    fn commit(
        &self,
        _plan: &SettingsEffectPlan,
        _receipt: &mut SettingsEffectReceipt,
    ) -> Result<(), SettingsEffectFailure> {
        Ok(())
    }

    fn restore(
        &self,
        _plan: &SettingsEffectPlan,
        _receipt: &SettingsEffectReceipt,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopSettingsRuntime;

impl SettingsRuntime for NoopSettingsRuntime {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateOutcome {
    pub preferences: UserPreferences,
    pub reconciled_hotkey_count: usize,
    pub effects: SettingsEffectPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylePackRemovalOutcome {
    pub effects: SettingsEffectPlan,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences(mode: WindowsInsertionMode, show: bool) -> UserPreferences {
        UserPreferences {
            windows_insertion_mode: mode,
            windows_sendinput_insertion_only: mode == WindowsInsertionMode::SendInput,
            windows_show_openless_in_keyboard_list: show,
            ..UserPreferences::default()
        }
    }

    #[test]
    fn windows_keyboard_effect_tracks_the_effective_profile_state() {
        for (mode, show, enabled) in [
            (WindowsInsertionMode::Tsf, true, true),
            (WindowsInsertionMode::Tsf, false, true),
            (WindowsInsertionMode::SendInput, true, true),
            (WindowsInsertionMode::SendInput, false, false),
            (WindowsInsertionMode::Paste, true, true),
            (WindowsInsertionMode::Paste, false, false),
        ] {
            assert_eq!(
                WindowsKeyboardRuntimeTarget::from(&preferences(mode, show))
                    .openless_language_profile_enabled,
                enabled,
                "mode={mode:?} show={show}"
            );
        }

        let send_input_hidden = preferences(WindowsInsertionMode::SendInput, false);
        let paste_hidden = preferences(WindowsInsertionMode::Paste, false);
        assert!(
            SettingsEffectPlan::between(&send_input_hidden, &paste_hidden)
                .windows_keyboard
                .is_none()
        );

        let tsf_with_hidden_pref = preferences(WindowsInsertionMode::Tsf, false);
        let change = SettingsEffectPlan::between(&paste_hidden, &tsf_with_hidden_pref)
            .windows_keyboard
            .expect("returning to TSF must re-enable its language profile");
        assert!(!change.previous.openless_language_profile_enabled);
        assert!(change.next.openless_language_profile_enabled);
        assert!(!tsf_with_hidden_pref.windows_show_openless_in_keyboard_list);
    }
}

#[cfg(test)]
mod translation_targets_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_list_roundtrip_allows_the_first_legacy_selection_but_explicit_clear_still_wins() {
        let previous = UserPreferences::default();
        let mut document = serde_json::to_value(&previous).unwrap();
        document["translationTargetLanguage"] = json!("ko");
        let mut next: UserPreferences = serde_json::from_value(document).unwrap();
        reconcile_translation_settings(&previous, &mut next).unwrap();
        assert_eq!(next.translation_target_language, "한국어");
        assert_eq!(next.translation_target_languages, vec!["한국어"]);
        let previous = next;
        let mut document = serde_json::to_value(&previous).unwrap();
        document["translationTargetLanguages"] = json!([]);
        let mut cleared: UserPreferences = serde_json::from_value(document).unwrap();
        reconcile_translation_settings(&previous, &mut cleared).unwrap();
        assert!(cleared.translation_target_language.is_empty());
        assert!(cleared.translation_target_languages.is_empty());
    }

    #[test]
    fn legacy_selected_target_seeds_list_and_preserves_custom_names() {
        for (input, expected) in [
            (" en ", "English"),
            (" My historical language ", "My historical language"),
        ] {
            let preferences: UserPreferences = serde_json::from_value(json!({
                "translationTargetLanguage": input,
            }))
            .unwrap();
            assert_eq!(preferences.translation_target_languages, vec![expected]);
            assert_eq!(preferences.translation_target_language, expected);
            assert!(!preferences.translation_target_languages_present);
        }
        let defaults: UserPreferences = serde_json::from_value(json!({})).unwrap();
        assert!(defaults.translation_target_languages.is_empty());
        assert!(defaults.translation_target_language.is_empty());
    }

    #[test]
    fn explicit_targets_canonicalize_dedupe_keep_order_and_validate_selection() {
        let preferences: UserPreferences = serde_json::from_value(json!({
            "translationTargetLanguages": ["ja", " en ", "English", "", "繁體中文", "zh-TW"],
            "translationTargetLanguage": "en",
        }))
        .unwrap();
        assert_eq!(
            preferences.translation_target_languages,
            vec!["日本語", "English", "繁体中文"]
        );
        assert_eq!(preferences.translation_target_language, "English");
        assert!(preferences.translation_target_languages_present);
        let invalid_selection: UserPreferences = serde_json::from_value(json!({
            "translationTargetLanguages": ["ja", "en"],
            "translationTargetLanguage": "fr",
        }))
        .unwrap();
        assert_eq!(invalid_selection.translation_target_language, "Français");
        assert_eq!(
            invalid_selection.translation_target_languages,
            vec!["日本語", "English", "Français"]
        );
    }

    #[test]
    fn explicit_empty_list_clears_selected_target_and_marker_is_not_wire_data() {
        let preferences: UserPreferences = serde_json::from_value(json!({
            "translationTargetLanguages": [],
            "translationTargetLanguage": "en",
            "translationTargetLanguagesPresent": false,
        }))
        .unwrap();
        assert!(preferences.translation_target_languages_present);
        assert!(preferences.translation_target_languages.is_empty());
        assert!(preferences.translation_target_language.is_empty());
        let value = serde_json::to_value(preferences).unwrap();
        assert!(value.get("translationTargetLanguagesPresent").is_none());
        let forged: UserPreferences = serde_json::from_value(json!({
            "translationTargetLanguage": "en",
            "translationTargetLanguagesPresent": true,
        }))
        .unwrap();
        assert!(!forged.translation_target_languages_present);
    }

    #[test]
    fn unknown_new_additions_fail_but_historical_targets_survive() {
        let previous: UserPreferences = serde_json::from_value(json!({
            "translationTargetLanguage": "Historical custom target",
        }))
        .unwrap();
        let mut next = previous.clone();
        reconcile_translation_settings(&previous, &mut next).unwrap();
        assert_eq!(
            next.translation_target_languages,
            vec!["Historical custom target"]
        );
        assert_eq!(
            validate_session_translation_target(&next, "Historical custom target").unwrap(),
            "Historical custom target"
        );
        next.translation_target_languages
            .push("Unknown new target".into());
        assert_eq!(
            reconcile_translation_settings(&previous, &mut next)
                .unwrap_err()
                .code,
            crate::errors::BackendErrorCode::InvalidArgument
        );
    }

    #[test]
    fn old_client_scalar_updates_preserve_order_and_disabled_selection() {
        let previous: UserPreferences = serde_json::from_value(json!({
            "translationTargetLanguages": ["en", "ja"],
            "translationTargetLanguage": "en",
        }))
        .unwrap();
        for (scalar, selected, targets) in [
            ("en", "English", vec!["English", "日本語"]),
            ("ja", "日本語", vec!["English", "日本語"]),
            ("fr", "Français", vec!["English", "日本語", "Français"]),
            ("", "", vec!["English", "日本語"]),
        ] {
            let mut next: UserPreferences = serde_json::from_value(json!({
                "translationTargetLanguage": scalar,
            }))
            .unwrap();
            reconcile_translation_settings(&previous, &mut next).unwrap();
            assert_eq!(next.translation_target_languages, targets);
            assert_eq!(next.translation_target_language, selected);
        }
    }
}
