//! Exercise the public Core boundary used by native update_setting_fields,
//! including its real settings transaction and on-disk store (no preview IPC).
use std::{collections::BTreeMap, path::Path, sync::Arc};

use openless_core::preference_patch::update_fields;
use openless_core::testing::{FixtureDictationEngine, FixtureTextInserter, RecordingHostActions};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, InMemoryCredentialStore,
    InsertOutcome, NoopSettingsRuntime, OpenLessBackend, SettingsUpdateOptions,
    SettingsUpdateOutcome, TokioTaskSpawner,
};
use serde_json::{json, Value};

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new() -> Self {
        Self(
            std::env::temp_dir().join(format!("openless-settings-fields-{}", uuid::Uuid::new_v4())),
        )
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn backend(path: &Path) -> OpenLessBackend {
    OpenLessBackend::new(
        BackendConfig {
            data_dir: path.to_path_buf(),
            ..BackendConfig::default()
        },
        BackendDependencies {
            host_actions: Arc::new(RecordingHostActions::default()),
            text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
            dictation_engine: Arc::new(FixtureDictationEngine::successful("", "")),
            task_spawner: Arc::new(TokioTaskSpawner),
            credential_store: Arc::new(InMemoryCredentialStore::default()),
            services: openless_core::domains::BackendServices::unsupported(),
            local_asr_runtime: None,
            voice_activation_runtime: None,
            marketplace_config: None,
            selection_runtime: None,
            selection_polisher: None,
            qa_runtime: None,
        },
    )
    .unwrap()
}

// Same public Core read/patch/transaction sequence as commands/settings.rs::
// persist_setting_fields; only the platform side-effect runtime is a no-op.
fn update_setting_fields(
    backend: &OpenLessBackend,
    edits: &BTreeMap<String, Value>,
) -> Result<SettingsUpdateOutcome, BackendError> {
    update_fields(
        edits,
        || {
            (
                backend.snapshot().preferences_revision,
                backend.get_preferences(),
            )
        },
        |prefs, revision| {
            backend.update_settings(
                prefs,
                SettingsUpdateOptions::SETTINGS_DOCUMENT.at_revision(revision),
                &NoopSettingsRuntime,
            )
        },
    )
}

fn count_edit(value: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([("/translationShiftTapCount".into(), value)])
}

#[test]
fn native_field_updates_persist_first_choice_and_explicit_default_after_reopen() {
    let directory = TestDirectory::new();
    let mut core = backend(&directory.0);
    let untouched = core.get_preferences();
    assert_eq!(untouched.translation_shift_tap_count(), 1);
    assert!(untouched.translation_shift_tap_count.is_none());
    assert!(serde_json::to_value(&untouched)
        .unwrap()
        .get("translationShiftTapCount")
        .is_none());

    // An unrelated native save must not turn the implicit default into an explicit choice.
    update_setting_fields(
        &core,
        &BTreeMap::from([("/showCapsule".into(), json!(false))]),
    )
    .unwrap();
    let disk: Value =
        serde_json::from_slice(&std::fs::read(directory.0.join("preferences.json")).unwrap())
            .unwrap();
    assert!(disk.get("translationShiftTapCount").is_none());

    for count in [2, 3, 1] {
        let revision = core.snapshot().preferences_revision;
        let saved = update_setting_fields(&core, &count_edit(json!(count))).unwrap();
        assert_eq!(saved.preferences.translation_shift_tap_count(), count);
        assert_eq!(core.snapshot().preferences_revision, revision + 1);
        assert_eq!(
            core.get_preferences()
                .translation_shift_tap_count
                .unwrap()
                .get(),
            count
        );
        let disk: Value =
            serde_json::from_slice(&std::fs::read(directory.0.join("preferences.json")).unwrap())
                .unwrap();
        assert_eq!(disk["translationShiftTapCount"], count);
        drop(core);
        core = backend(&directory.0);
        assert_eq!(
            core.get_preferences()
                .translation_shift_tap_count
                .unwrap()
                .get(),
            count
        );
        assert!(!core.get_preferences().show_capsule);
    }
}

#[test]
fn invalid_field_updates_leave_preferences_revision_and_disk_unchanged() {
    let directory = TestDirectory::new();
    let core = backend(&directory.0);
    for configured in [false, true] {
        if configured {
            update_setting_fields(&core, &count_edit(json!(2))).unwrap();
        }
        let before = serde_json::to_value(core.get_preferences()).unwrap();
        let revision = core.snapshot().preferences_revision;
        let disk = std::fs::read(directory.0.join("preferences.json")).ok();
        for (pointer, value) in [
            ("/translationShiftTapCount", json!(0)),
            ("/translationShiftTapCount", json!(256)),
            ("/translationShiftTapCount", json!("2")),
            ("/translationShiftTapCount/child", json!(2)),
            ("/unknownPreference", json!(2)),
        ] {
            let edits = BTreeMap::from([
                (
                    "/showCapsule".into(),
                    json!(!core.get_preferences().show_capsule),
                ),
                (pointer.into(), value),
            ]);
            assert_eq!(
                update_setting_fields(&core, &edits).unwrap_err().code,
                BackendErrorCode::InvalidArgument
            );
            assert_eq!(
                serde_json::to_value(core.get_preferences()).unwrap(),
                before
            );
            assert_eq!(core.snapshot().preferences_revision, revision);
            assert_eq!(
                std::fs::read(directory.0.join("preferences.json")).ok(),
                disk
            );
        }
    }
}

#[test]
fn first_optional_choice_retries_a_real_revision_conflict_without_losing_other_edits() {
    let directory = TestDirectory::new();
    let core = backend(&directory.0);
    let mut attempts = 0;
    update_fields(
        &count_edit(json!(3)),
        || (core.snapshot().preferences_revision, core.get_preferences()),
        |prefs, revision| {
            attempts += 1;
            if attempts == 1 {
                update_setting_fields(
                    &core,
                    &BTreeMap::from([(
                        "/microphoneDeviceName".into(),
                        json!("concurrent fixture"),
                    )]),
                )
                .unwrap();
            }
            core.update_settings(
                prefs,
                SettingsUpdateOptions::SETTINGS_DOCUMENT.at_revision(revision),
                &NoopSettingsRuntime,
            )
        },
    )
    .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(core.snapshot().preferences_revision, 2);
    assert_eq!(core.get_preferences().translation_shift_tap_count(), 3);
    assert_eq!(
        core.get_preferences().microphone_device_name,
        "concurrent fixture"
    );
}
