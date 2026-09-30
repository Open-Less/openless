//! Apply field edits through the shared Core patch implementation.
pub use openless_core::preference_patch::patch_preferences;
use openless_core::preference_patch::update_fields;
use openless_core::{BackendError, SettingsUpdateOutcome};
use serde_json::Value;
use std::collections::BTreeMap;

impl crate::LinuxHost {
    pub fn update_preference_fields(
        &self,
        edits: &BTreeMap<String, Value>,
    ) -> Result<SettingsUpdateOutcome, BackendError> {
        update_fields(
            edits,
            || {
                (
                    self.snapshot().preferences_revision,
                    self.backend().get_preferences(),
                )
            },
            |draft, revision| self.update_settings_strict(draft, revision),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::preference_patch::revision_conflict;
    use openless_core::{BackendErrorCode, UserPreferences};

    #[test]
    fn patches_preserve_other_fields_and_reject_unknown_or_invalid_values() {
        let current = UserPreferences {
            active_style_pack_id: "external-style".into(),
            ..Default::default()
        };
        let next = patch_preferences(
            &current,
            &BTreeMap::from([
                ("/showCapsule".into(), Value::Bool(false)),
                ("/hotkey/mode".into(), Value::String("hold".into())),
            ]),
        )
        .unwrap();
        assert_eq!(next.active_style_pack_id, current.active_style_pack_id);
        assert!(!next.show_capsule);
        for (key, value) in [
            ("/typo", Value::Bool(false)),
            ("/remoteInputPort", Value::from(100_000)),
        ] {
            assert!(patch_preferences(&current, &BTreeMap::from([(key.into(), value)])).is_err());
        }
    }

    #[test]
    fn only_revision_conflicts_are_retried() {
        let mut error = BackendError::new(BackendErrorCode::Busy, "native resource busy");
        assert!(!revision_conflict(&error));
        error.details = Some(
            serde_json::json!({"expectedPreferencesRevision": 1, "actualPreferencesRevision": 2}),
        );
        assert!(revision_conflict(&error));
        error.code = BackendErrorCode::Platform;
        assert!(!revision_conflict(&error));
    }
    #[test]
    fn omitted_fields_persist_after_a_real_core_revision_conflict() {
        use std::sync::Arc;
        let dir =
            std::env::temp_dir().join(format!("openless-field-patch-{}", uuid::Uuid::new_v4()));
        let backend = Arc::new(
            openless_core::OpenLessBackend::new(
                openless_core::BackendConfig {
                    data_dir: dir.clone(),
                    ..Default::default()
                },
                openless_core::BackendDependencies::unsupported(),
            )
            .unwrap(),
        );
        let host = crate::LinuxHost::with_settings_runtime(
            backend.clone(),
            Arc::new(openless_core::NoopSettingsRuntime),
        );
        let selection = backend
            .create_style_pack(openless_core::StylePack {
                name: "Selection patch".into(),
                prompt: "Polish".into(),
                ..Default::default()
            })
            .unwrap();
        let initial = backend.get_preferences();
        let mut changed = initial.clone();
        changed.streaming_insert_save_clipboard = !initial.streaming_insert_save_clipboard;
        changed.show_capsule = !initial.show_capsule;
        changed.capsule_style = openless_core::shared_types::CapsuleStyle::Classic;
        changed.selection_polish_output_mode =
            openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm;
        changed.polish_context_window_minutes = 17;
        changed.audio_recording_max_entries = Some(19);
        changed.coding_agent_enabled = true;
        changed.coding_agent_provider = "codex-cli".into();
        changed.coding_agent_permission_mode = "plan".into();
        changed.coding_agent_model = Some("fixture".into());
        changed.coding_agent_workdir = Some("/tmp/work".into());
        changed.coding_agent_exe = Some("/bin/codex".into());
        changed.remote_input_default_mode = "hold".into();
        changed.working_languages = vec!["ja".into()];
        changed.translation_target_language = "ko".into();
        changed.selection_polish_style_pack_id = selection.id.clone();
        changed.style_pack_hotkeys = vec![openless_core::shared_types::StylePackHotkey {
            pack_id: selection.id,
            binding: openless_core::shared_types::ShortcutBinding {
                primary: "F10".into(),
                modifiers: vec!["alt".into()],
            },
        }];
        let initial = serde_json::to_value(initial).unwrap();
        let changed = serde_json::to_value(changed).unwrap();
        for (key, value) in changed.as_object().unwrap() {
            if initial[key] == *value {
                continue;
            }
            let patch = BTreeMap::from([(format!("/{key}"), value.clone())]);
            let mut attempts = 0;
            update_fields(
                &patch,
                || {
                    (
                        host.snapshot().preferences_revision,
                        backend.get_preferences(),
                    )
                },
                |draft, revision| {
                    attempts += 1;
                    if attempts == 1 {
                        host.update_preference_fields(&BTreeMap::from([(
                            "/microphoneDeviceName".into(),
                            Value::String(key.clone()),
                        )]))?;
                    }
                    host.update_settings_strict(draft, revision)
                },
            )
            .unwrap();
            assert_eq!(
                attempts, 2,
                "{key} must retry exactly the revision conflict"
            );
            let saved = serde_json::to_value(backend.get_preferences()).unwrap();
            assert_eq!(saved[key], *value, "{key}");
            assert_eq!(saved["microphoneDeviceName"], key.as_str());
        }
        drop(host);
        drop(backend);
        let reopened = openless_core::OpenLessBackend::new(
            openless_core::BackendConfig {
                data_dir: dir.clone(),
                ..Default::default()
            },
            openless_core::BackendDependencies::unsupported(),
        )
        .unwrap();
        let saved = serde_json::to_value(reopened.get_preferences()).unwrap();
        for (key, value) in changed.as_object().unwrap() {
            if initial[key] != *value {
                assert_eq!(saved[key], *value, "persisted {key}");
            }
        }
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
