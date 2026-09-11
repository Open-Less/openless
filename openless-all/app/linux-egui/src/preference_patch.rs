//! Field edits are applied to the latest revision, so a settings modal cannot
//! overwrite a hotkey, active style or remote-input change made in the background.
use openless_core::{BackendError, BackendErrorCode, SettingsUpdateOutcome, UserPreferences};
use serde_json::Value;
use std::collections::BTreeMap;

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
    serde_json::from_value(document).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, error.to_string())
}

impl crate::LinuxHost {
    /// Release native shortcuts through the settings transaction before removing
    /// their pack. A failed removal restores the previous preference document.
    pub fn remove_style_pack(&self, id: &str) -> Result<(), BackendError> {
        let previous = self.backend().get_preferences();
        let bindings: Vec<_> = previous
            .style_pack_hotkeys
            .iter()
            .filter(|p| p.pack_id != id)
            .cloned()
            .collect();
        self.update_preference_fields(&BTreeMap::from([(
            "/stylePackHotkeys".into(),
            serde_json::to_value(bindings).map_err(invalid)?,
        )]))?;
        if let Err(error) = self.backend().remove_style_pack(id) {
            self.update_preference_fields(&BTreeMap::from([(
                "/stylePackHotkeys".into(),
                serde_json::to_value(previous.style_pack_hotkeys).map_err(invalid)?,
            )]))?;
            return Err(error);
        }
        Ok(())
    }
    pub fn update_preference_fields(
        &self,
        edits: &BTreeMap<String, Value>,
    ) -> Result<SettingsUpdateOutcome, BackendError> {
        for _ in 0..3 {
            let snapshot = self.snapshot();
            let draft = patch_preferences(&self.backend().get_preferences(), edits)?;
            match self.update_settings_strict(draft, snapshot.preferences_revision) {
                Err(error) if error.code == BackendErrorCode::Busy => continue,
                outcome => return outcome,
            }
        }
        Err(BackendError::new(
            BackendErrorCode::Busy,
            "settings changed while saving; please retry",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_patch_preserves_concurrent_unrelated_changes() {
        let mut preferences = UserPreferences::default();
        preferences.active_style_pack_id = "newly-selected-style".into();
        let changed = patch_preferences(
            &preferences,
            &BTreeMap::from([("/showCapsule".into(), Value::Bool(false))]),
        )
        .unwrap();
        assert_eq!(changed.active_style_pack_id, "newly-selected-style");
        assert!(!changed.show_capsule);
    }
    #[test]
    fn unknown_fields_and_invalid_types_are_rejected() {
        let preferences = UserPreferences::default();
        assert!(patch_preferences(
            &preferences,
            &BTreeMap::from([("/typo".into(), Value::Bool(false))])
        )
        .is_err());
        assert!(patch_preferences(
            &preferences,
            &BTreeMap::from([("/remoteInputPort".into(), Value::from(100_000))])
        )
        .is_err());
    }
}
