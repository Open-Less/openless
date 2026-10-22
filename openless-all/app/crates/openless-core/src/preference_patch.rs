//! Apply field edits to the latest Core revision through its strict transaction.
use std::collections::BTreeMap;

use crate::{BackendError, BackendErrorCode, SettingsUpdateOutcome, UserPreferences};
use serde_json::Value;

pub fn patch_preferences(
    current: &UserPreferences,
    edits: &BTreeMap<String, Value>,
) -> Result<UserPreferences, BackendError> {
    let mut document = serde_json::to_value(current).map_err(invalid)?;
    for (pointer, value) in edits {
        let destination = document
            .pointer_mut(pointer)
            .ok_or_else(|| invalid(format!("unknown preference: {pointer}")))?;
        *destination = value.clone();
    }
    if let Some(mode) = edits.get("/capsuleDisplayMode") {
        document["capsuleDisplayModeV2"] = mode.clone();
    }
    let mut patched = serde_json::from_value(document).map_err(invalid)?;
    crate::settings::reconcile_translation_settings(current, &mut patched)?;
    Ok(patched)
}

fn invalid(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, error.to_string())
}

pub fn revision_conflict(error: &BackendError) -> bool {
    error.code == BackendErrorCode::Busy
        && error.details.as_ref().is_some_and(|details| {
            details["expectedPreferencesRevision"].is_u64()
                && details["actualPreferencesRevision"].is_u64()
        })
}

pub fn update_fields(
    edits: &BTreeMap<String, Value>,
    mut read: impl FnMut() -> (u64, UserPreferences),
    mut save: impl FnMut(UserPreferences, u64) -> Result<SettingsUpdateOutcome, BackendError>,
) -> Result<SettingsUpdateOutcome, BackendError> {
    for attempt in 0..3 {
        let (revision, current) = read();
        let draft = patch_preferences(&current, edits)?;
        match save(draft, revision) {
            Err(error) if attempt < 2 && revision_conflict(&error) => continue,
            outcome => return outcome,
        }
    }
    unreachable!("the final attempt always returns")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_drafts_survive_rebasing_and_serialization_before_save() {
        let current = UserPreferences::default();
        let edits = BTreeMap::from([("/translationTargetLanguage".into(), Value::from("ko"))]);
        let draft = patch_preferences(&current, &edits).unwrap();
        assert_eq!(draft.translation_target_language, "한국어");
        assert_eq!(draft.translation_target_languages, vec!["한국어"]);
        let loaded: UserPreferences = serde_json::from_value(serde_json::to_value(&draft).unwrap()).unwrap();
        assert_eq!(loaded.translation_target_language, draft.translation_target_language);
        assert_eq!(loaded.translation_target_languages, draft.translation_target_languages);
        let cleared = patch_preferences(&loaded, &BTreeMap::from([
            ("/translationTargetLanguages".into(), serde_json::json!([])),
        ])).unwrap();
        assert!(cleared.translation_target_language.is_empty());
        assert!(cleared.translation_target_languages.is_empty());
    }

    #[test]
    fn capsule_display_edits_establish_v2_without_opt_in_from_unrelated_edits() {
        let mut current = UserPreferences::default();
        let unrelated = BTreeMap::from([("/showCapsule".into(), Value::Bool(false))]);
        assert_eq!(
            patch_preferences(&current, &unrelated)
                .unwrap()
                .capsule_display_mode_v2,
            None
        );
        for mode in [
            "primaryScreen",
            "foregroundWindowScreen",
            "allScreens",
            "cursorScreen",
        ] {
            current = patch_preferences(
                &current,
                &BTreeMap::from([("/capsuleDisplayMode".into(), Value::from(mode))]),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(&current).unwrap()["capsuleDisplayModeV2"],
                mode
            );
            let next = patch_preferences(&current, &unrelated).unwrap();
            assert_eq!(next.capsule_display_mode, current.capsule_display_mode);
            assert_eq!(
                next.capsule_display_mode_v2,
                current.capsule_display_mode_v2
            );
        }
    }

    #[test]
    fn independent_field_edits_survive_conflicting_revisions() {
        let current = UserPreferences::default();
        let edits = BTreeMap::from([("/showCapsule".into(), Value::Bool(!current.show_capsule))]);
        let mut external = current.clone();
        external.microphone_device_name = "changed elsewhere".into();
        let result = patch_preferences(&external, &edits).unwrap();
        assert_eq!(result.microphone_device_name, "changed elsewhere");
        assert_ne!(result.show_capsule, current.show_capsule);
        assert!(patch_preferences(
            &current,
            &BTreeMap::from([("/unknown".into(), Value::Bool(true))])
        )
        .is_err());
        assert!(patch_preferences(
            &current,
            &BTreeMap::from([("/remoteInputPort".into(), Value::from(100000))])
        )
        .is_err());
        let busy = BackendError::new(BackendErrorCode::Busy, "native failure");
        assert!(!revision_conflict(&busy));
    }
}
