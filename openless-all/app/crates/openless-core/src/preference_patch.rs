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
    serde_json::from_value(document).map_err(invalid)
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
