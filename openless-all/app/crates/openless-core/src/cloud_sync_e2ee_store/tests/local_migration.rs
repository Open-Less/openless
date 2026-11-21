use super::*;
use crate::local_migration_bundle::{
    AndroidMigrationLease, AndroidMigrationSnapshot, AndroidMigrationState,
};

fn portable(theme: &str) -> Value {
    json!({"schemaVersion":1,"learningPrivacyVersion":1,"preferences":{"themeMode":theme,"hapticEnabled":true,"hapticAmplitude":55,"hapticDurationMs":12,"englishSuggestionsEnabled":true,"debugFeaturesEnabled":false},"strokeFrequency":{"word":"1,2"},"pinyinLearnedPhrases":{"xx":"word,2"}})
}

#[derive(Clone)]
struct AndroidState(Arc<Mutex<AndroidData>>);
struct AndroidData {
    value: Value,
    revision: String,
    leased: bool,
    recovering: bool,
    failures: VecDeque<usize>,
    finish_failures: usize,
}
impl AndroidState {
    fn new(value: Value) -> Self {
        Self(Arc::new(Mutex::new(AndroidData {
            value,
            revision: uuid::Uuid::new_v4().to_string(),
            leased: false,
            recovering: false,
            failures: VecDeque::new(),
            finish_failures: 0,
        })))
    }
    fn value(&self) -> Value {
        self.0.lock().unwrap().value.clone()
    }
}
impl AndroidMigrationState for AndroidState {
    fn capture(&self) -> DocumentResult<AndroidMigrationSnapshot> {
        let data = self.0.lock().unwrap();
        if data.leased || data.recovering {
            return Err(DocumentError::RuntimeBusy);
        }
        Ok(AndroidMigrationSnapshot {
            revision: data.revision.clone(),
            value: SecretJson::new(data.value.clone()),
        })
    }
    fn acquire(&self, expected: Option<&str>) -> DocumentResult<Box<dyn AndroidMigrationLease>> {
        let mut data = self.0.lock().unwrap();
        if data.leased {
            return Err(DocumentError::RuntimeBusy);
        }
        if expected.is_some_and(|expected| expected != data.revision || data.recovering) {
            return Err(DocumentError::StalePreview);
        }
        data.leased = true;
        Ok(Box::new(self.clone()))
    }
}
impl AndroidMigrationLease for AndroidState {
    fn capture(&self) -> DocumentResult<SecretJson> {
        Ok(SecretJson::new(self.value()))
    }
    fn replace(&mut self, value: &SecretJson) -> DocumentResult<()> {
        let mut data = self.0.lock().unwrap();
        let fail = data.failures.pop_front();
        for (index, key) in ["preferences", "strokeFrequency", "pinyinLearnedPhrases"]
            .iter()
            .enumerate()
        {
            data.value[key] = value.expose()[key].clone();
            data.revision = uuid::Uuid::new_v4().to_string();
            if fail == Some(index) {
                return Err(DocumentError::CaptureFailed);
            }
        }
        Ok(())
    }
    fn finish(&mut self, recovery_required: bool) -> DocumentResult<()> {
        let mut data = self.0.lock().unwrap();
        data.leased = false;
        if data.finish_failures > 0 {
            data.finish_failures -= 1;
            data.recovering = true;
            return Err(DocumentError::CaptureFailed);
        }
        data.recovering = recovery_required;
        Ok(())
    }
}

fn fixture(theme: &str) -> (Fixture, AndroidState) {
    let fixture = Fixture::new();
    let state = AndroidState::new(portable(theme));
    fixture
        .store
        .bind_android_migration_state(Arc::new(state.clone()))
        .unwrap();
    (fixture, state)
}

#[tokio::test]
async fn data_only_export_keeps_explicit_portable_state_without_a_host_adapter() {
    let source = Fixture::new();
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions {
            portable_android_preferences: Some(portable("dark")),
            ..BundleExportOptions::default()
        })
        .await
        .unwrap();
    let decoded = decode_bundle(&artifact.bytes, None, Revision::new(0)).unwrap();
    assert_eq!(
        android_settings(&decoded.documents),
        Some(&portable("dark"))
    );
}

fn choices(preview: &BundleMergePreview, side: ConflictSide) -> Vec<ConflictChoice> {
    preview
        .conflicts
        .iter()
        .map(|conflict| ConflictChoice {
            conflict_id: conflict.conflict_id.clone(),
            side,
        })
        .collect()
}

#[tokio::test]
async fn android_and_core_roll_back_at_each_android_write_and_recover_failed_rollback() {
    let (source, _) = fixture("dark");
    source
        .repo
        .preferences
        .update(|prefs| prefs.theme_mode = crate::contract::ThemeMode::Dark)
        .unwrap();
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions::default())
        .await
        .unwrap();
    for stage in 0..3 {
        for fail_rollback in [false, true] {
            let (target, state) = fixture("light");
            let before = state.value();
            let core_before = target.repo.preferences.get();
            let preview = target
                .store
                .preview_local_bundle(&artifact.bytes, None, RestoreMode::Replace)
                .await
                .unwrap();
            state.0.lock().unwrap().failures.push_back(stage);
            if fail_rollback {
                state.0.lock().unwrap().failures.push_back(stage);
            }
            let result = target
                .store
                .import_local_bundle(
                    &artifact.bytes,
                    None,
                    RestoreMode::Replace,
                    &choices(&preview, ConflictSide::Remote),
                    Some(preview.observed_generation),
                    preview.observed_android_revision,
                )
                .await;
            assert_eq!(
                result.unwrap_err(),
                if fail_rollback {
                    BundleError::RecoveryRequired
                } else {
                    BundleError::RestoreRolledBack
                }
            );
            let target = if fail_rollback {
                assert!(target.store.is_recovering());
                assert!(state.0.lock().unwrap().recovering);
                let reopened = target.reopened();
                reopened.store.recover_registered().await.unwrap();
                reopened
            } else {
                target
            };
            assert_eq!(state.value(), before);
            assert_eq!(
                serde_json::to_value(target.repo.preferences.get()).unwrap(),
                serde_json::to_value(core_before).unwrap()
            );
            assert!(!target.store.is_recovering());
            assert!(!state.0.lock().unwrap().recovering);
        }
    }
}

#[tokio::test]
async fn stale_android_preview_is_rejected_before_any_core_write() {
    let (source, _) = fixture("dark");
    let (target, state) = fixture("light");
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions::default())
        .await
        .unwrap();
    let preview = target
        .store
        .preview_local_bundle(&artifact.bytes, None, RestoreMode::Merge)
        .await
        .unwrap();
    state.0.lock().unwrap().revision = "changed".into();
    let before = target.repo.preferences.get();
    assert_eq!(
        target
            .store
            .import_local_bundle(
                &artifact.bytes,
                None,
                RestoreMode::Merge,
                &choices(&preview, ConflictSide::Remote),
                Some(preview.observed_generation),
                preview.observed_android_revision
            )
            .await
            .unwrap_err(),
        BundleError::StalePreview
    );
    assert_eq!(
        serde_json::to_value(target.repo.preferences.get()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(state.value(), portable("light"));
}

#[tokio::test]
async fn android_choices_and_learning_follow_the_selected_mode() {
    for mode in [RestoreMode::Merge, RestoreMode::Replace] {
        for side in [ConflictSide::Local, ConflictSide::Remote] {
            let (source, _) = fixture("dark");
            let (target, state) = fixture("light");
            state.0.lock().unwrap().value["strokeFrequency"]["local-only"] = json!("3,4");
            let artifact = source
                .store
                .export_local_bundle(BundleExportOptions::default())
                .await
                .unwrap();
            let preview = target
                .store
                .preview_local_bundle(&artifact.bytes, None, mode)
                .await
                .unwrap();
            target
                .store
                .import_local_bundle(
                    &artifact.bytes,
                    None,
                    mode,
                    &choices(&preview, side),
                    Some(preview.observed_generation),
                    preview.observed_android_revision,
                )
                .await
                .unwrap();
            assert_eq!(
                state.value()["preferences"]["themeMode"],
                if side == ConflictSide::Local {
                    "light"
                } else {
                    "dark"
                }
            );
            assert_eq!(
                state.value()["strokeFrequency"].get("local-only").is_some(),
                mode == RestoreMode::Merge || side == ConflictSide::Local
            );
        }
    }
}

#[tokio::test]
async fn recovery_replays_android_before_or_after_from_the_durable_decision() {
    for (committed, has_android_state) in
        [(false, true), (true, true), (false, false), (true, false)]
    {
        let (target, state) = fixture("light");
        let captured = if has_android_state {
            target.store.capture_local_bundle().await.unwrap().0
        } else {
            target
                .store
                .export_scope(target.store.local_bundle_scope())
                .await
                .unwrap()
        };
        let before = captured.documents.documents().clone();
        let mut after = before.clone();
        set_doc(
            &mut after,
            DocumentKind::Preferences,
            "themeMode",
            json!("dark"),
        );
        if has_android_state {
            set_doc(
                &mut after,
                DocumentKind::UiPreferences,
                "androidPortableSettings",
                portable("dark"),
            );
        }
        let scope = target.store.local_bundle_scope();
        let operation = uuid::Uuid::new_v4().to_string();
        let permit = target.store.inner.gate.try_exclusive().unwrap();
        let local_before = target.store.capture_local_rollback_state(&permit).unwrap();
        let journal = json!({"schemaVersion":1,"scope":scope,"operationId":operation,"observedRevision":captured.documents.observed_revision(),"phase":if committed {"committed"} else {"prepared"},"before":before,"after":after,"localBefore":local_before});
        permit
            .mark_restore_pending(&operation, &state::scope_id(&scope).unwrap(), &scope)
            .unwrap();
        let ciphertext = target
            .extensions
            .protector
            .seal(&scope, &operation, &serde_json::to_vec(&journal).unwrap())
            .unwrap();
        target
            .store
            .journal()
            .save(
                scope.clone(),
                SealedJournal {
                    operation_id: operation.clone(),
                    ciphertext,
                },
            )
            .await
            .unwrap();
        permit
            .mark_journal_ready(&operation, &state::scope_id(&scope).unwrap())
            .unwrap();
        drop(permit);
        state.0.lock().unwrap().value = portable("system");
        state.0.lock().unwrap().recovering = has_android_state;
        let target = target.reopened();
        target.store.recover_registered().await.unwrap();
        assert_eq!(
            state.value(),
            portable(if !has_android_state {
                "system"
            } else if committed {
                "dark"
            } else {
                "light"
            })
        );
        assert!(!target.store.is_recovering());
    }
}

fn usage(set: &mut DocumentSet, id: &str, text: &str, count: u32) {
    set_doc(
        set,
        DocumentKind::History,
        id,
        json!({"id":id,"createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":text,"finalText":text,"mode":"raw","insertStatus":"notRequested","hasAudioRecording":false,"sortIndex":0}),
    );
    set_doc(
        set,
        DocumentKind::Activity,
        "fixture-device:2026-09-26",
        json!({"sourceDeviceId":"fixture-device","date":"2026-09-26","count":count,"chars":10,"durationMs":20}),
    );
}

fn legacy_usage_bundle(bytes: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Read, Write};
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let mut manifest: Value = serde_json::from_str(&manifest).unwrap();
    let mut documents = String::new();
    archive
        .by_name("documents.json")
        .unwrap()
        .read_to_string(&mut documents)
        .unwrap();
    let mut set: DocumentSet = serde_json::from_str(&documents).unwrap();
    usage(&mut set, "sender-history", "SENDER_PRIVATE_TEXT", 99);
    set.tombstones
        .push(crate::cloud_sync_e2ee_protocol::types::Tombstone {
            id: "receiver-history".into(),
            kind: DocumentKind::History,
            deleted_at: "2026-09-26T00:00:00Z".into(),
            base_revision: Revision::new(0),
        });
    let documents = serde_json::to_vec(&set).unwrap();
    manifest["documents"]["bytes"] = json!(documents.len());
    manifest["documents"]["sha256"] = json!(hex::encode(Sha256::digest(&documents)));
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
        ("documents.json", documents),
    ] {
        output
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

#[tokio::test]
async fn old_usage_records_and_deletions_are_skipped_in_both_restore_modes() {
    let source = Fixture::new();
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions::default())
        .await
        .unwrap();
    let legacy = legacy_usage_bundle(&artifact.bytes);
    for mode in [RestoreMode::Merge, RestoreMode::Replace] {
        let target = Fixture::new();
        let before = target
            .store
            .export_scope(target.scope.clone())
            .await
            .unwrap();
        let mut set = before.documents.documents().clone();
        usage(&mut set, "receiver-history", "RECEIVER_PRIVATE_TEXT", 41);
        target
            .store
            .restore_scope(validate(set), target.context(&before))
            .await
            .unwrap();
        let history = target.repo.history.list().unwrap();
        let activity = target.repo.activity.snapshot().unwrap();
        let preview = target
            .store
            .preview_local_bundle(&legacy, None, mode)
            .await
            .unwrap();
        assert_eq!(preview.inspection.skipped_history, 1);
        assert_eq!(preview.inspection.skipped_activity, 1);
        assert_eq!(preview.inspection.skipped_tombstones, 1);
        target
            .store
            .import_local_bundle(
                &legacy,
                None,
                mode,
                &choices(&preview, ConflictSide::Remote),
                Some(preview.observed_generation),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(target.repo.history.list().unwrap()).unwrap(),
            serde_json::to_value(history).unwrap()
        );
        assert_eq!(
            serde_json::to_value(target.repo.activity.snapshot().unwrap()).unwrap(),
            serde_json::to_value(activity).unwrap()
        );
    }
}

fn credentials(secret: &str) -> SyncCredentials {
    let mut result = SyncCredentials {
        channels: Vec::new(),
        credentials: Vec::new(),
    };
    for (namespace, provider, endpoint, model, key) in [
        (
            "asr",
            "custom-openai-whisper",
            "asr.endpoint",
            "asr.model",
            "asr.api_key",
        ),
        (
            "llm",
            "openai",
            "ark.endpoint",
            "ark.model_id",
            "ark.api_key",
        ),
        (
            "omni",
            "qwen-omni",
            "omni.endpoint",
            "omni.model",
            "omni.api_key",
        ),
    ] {
        result.channels.push(serde_json::from_value(json!({"id":provider,"namespace":namespace,"providerType":provider,"name":provider,"enabled":true,"order":0,"active":true})).unwrap());
        result.credentials.push(serde_json::from_value(json!({"channelId":provider,"namespace":namespace,"accounts":{(endpoint):"https://example.test/v1",(model):"model",(key):secret}})).unwrap());
    }
    result
}

#[tokio::test]
async fn all_namespaces_restore_public_settings_idempotently_without_keys() {
    let source = Fixture::new();
    *source.credentials.value.lock().unwrap() = credentials("sender-secret");
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions::default())
        .await
        .unwrap();
    for mode in [RestoreMode::Merge, RestoreMode::Replace] {
        let target = Fixture::new();
        for _ in 0..2 {
            let preview = target
                .store
                .preview_local_bundle(&artifact.bytes, None, mode)
                .await
                .unwrap();
            target
                .store
                .import_local_bundle(
                    &artifact.bytes,
                    None,
                    mode,
                    &choices(&preview, ConflictSide::Remote),
                    Some(preview.observed_generation),
                    None,
                )
                .await
                .unwrap();
            let restored = target.credentials.value.lock().unwrap();
            assert_eq!(restored.channels.len(), 3);
            for record in &restored.credentials {
                assert_eq!(record.accounts.len(), 2);
                assert!(record
                    .accounts
                    .values()
                    .any(|value| value == "https://example.test/v1"));
                assert!(record.accounts.values().any(|value| value == "model"));
            }
        }
    }
}

#[tokio::test]
async fn redacted_shares_keep_receiver_secrets_and_their_endpoint_binding() {
    let source = Fixture::new();
    let target = Fixture::new();
    *source.credentials.value.lock().unwrap() = credentials("sender-secret");
    *target.credentials.value.lock().unwrap() = credentials("receiver-secret");
    for record in &mut source.credentials.value.lock().unwrap().credentials {
        for (account, value) in &mut record.accounts {
            if account.ends_with(".endpoint") {
                *value = "https://untrusted.test/v1".into();
            }
        }
    }
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions::default())
        .await
        .unwrap();
    let preview = target
        .store
        .preview_local_bundle(&artifact.bytes, None, RestoreMode::Replace)
        .await
        .unwrap();
    target
        .store
        .import_local_bundle(
            &artifact.bytes,
            None,
            RestoreMode::Replace,
            &choices(&preview, ConflictSide::Remote),
            Some(preview.observed_generation),
            None,
        )
        .await
        .unwrap();
    for record in &target.credentials.value.lock().unwrap().credentials {
        assert!(record
            .accounts
            .values()
            .any(|value| value == "receiver-secret"));
        assert!(record
            .accounts
            .values()
            .any(|value| value == "https://example.test/v1"));
        assert!(!record
            .accounts
            .values()
            .any(|value| value == "https://untrusted.test/v1"));
    }
}

#[tokio::test]
async fn cloud_restore_does_not_require_or_write_the_android_adapter() {
    let (target, state) = fixture("light");
    let before = target
        .store
        .export_scope(target.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    target
        .store
        .restore_scope(validate(desired), target.context(&before))
        .await
        .unwrap();
    assert_eq!(state.value(), portable("light"));
}

#[tokio::test]
async fn both_modes_require_exact_conflict_choices_before_writes() {
    for mode in [RestoreMode::Merge, RestoreMode::Replace] {
        let (source, _) = fixture("dark");
        let (target, state) = fixture("light");
        let artifact = source
            .store
            .export_local_bundle(BundleExportOptions::default())
            .await
            .unwrap();
        let preview = target
            .store
            .preview_local_bundle(&artifact.bytes, None, mode)
            .await
            .unwrap();
        let valid = choices(&preview, ConflictSide::Local);
        assert!(!valid.is_empty());
        let mut missing = valid.clone();
        missing.pop();
        let mut duplicate = valid.clone();
        duplicate.push(valid[0].clone());
        let mut unknown = valid.clone();
        unknown.push(ConflictChoice {
            conflict_id: "unknown".into(),
            side: ConflictSide::Remote,
        });
        for invalid in [missing, duplicate, unknown] {
            assert_eq!(
                target
                    .store
                    .import_local_bundle(
                        &artifact.bytes,
                        None,
                        mode,
                        &invalid,
                        Some(preview.observed_generation),
                        preview.observed_android_revision.clone()
                    )
                    .await
                    .unwrap_err(),
                BundleError::Document(DocumentError::ConflictChoiceRequired)
            );
            assert_eq!(
                target.store.generation().unwrap(),
                preview.observed_generation
            );
            assert_eq!(state.value(), portable("light"));
        }
    }
}

#[tokio::test]
async fn replace_removes_receiver_only_configuration_but_not_explicit_local_conflicts() {
    for mode in [RestoreMode::Merge, RestoreMode::Replace] {
        let (source, _) = fixture("dark");
        let (target, state) = fixture("light");
        let before = target
            .store
            .export_scope(target.store.local_bundle_scope())
            .await
            .unwrap();
        let mut local = before.documents.documents().clone();
        set_doc(
            &mut local,
            DocumentKind::Preferences,
            "futureSetting",
            json!("retained"),
        );
        set_doc(
            &mut local,
            DocumentKind::UiPreferences,
            "futureUi",
            json!("retained"),
        );
        set_doc(
            &mut local,
            DocumentKind::Dictionary,
            "receiver-only",
            json!({"id":"receiver-only","phrase":"Keep only in merge","enabled":true,"hits":0,"createdAt":"2026-09-26T00:00:00Z","sortIndex":0}),
        );
        target
            .store
            .restore_scope(
                validate(local),
                RestoreContext {
                    scope: target.store.local_bundle_scope(),
                    ..target.context(&before)
                },
            )
            .await
            .unwrap();
        let artifact = source
            .store
            .export_local_bundle(BundleExportOptions::default())
            .await
            .unwrap();
        let preview = target
            .store
            .preview_local_bundle(&artifact.bytes, None, mode)
            .await
            .unwrap();
        assert_eq!(
            preview
                .removed_documents
                .iter()
                .any(|doc| doc.id == "receiver-only"),
            mode == RestoreMode::Replace
        );
        assert!(!preview
            .removed_documents
            .iter()
            .any(|doc| doc.id == "futureSetting" || doc.id == "futureUi"));
        target
            .store
            .import_local_bundle(
                &artifact.bytes,
                None,
                mode,
                &choices(&preview, ConflictSide::Local),
                Some(preview.observed_generation),
                preview.observed_android_revision,
            )
            .await
            .unwrap();
        assert_eq!(
            target
                .repo
                .vocabulary
                .list()
                .unwrap()
                .iter()
                .any(|entry| entry.id == "receiver-only"),
            mode == RestoreMode::Merge
        );
        assert_eq!(state.value()["preferences"]["themeMode"], "light");
    }
}

#[tokio::test]
async fn failed_android_fence_release_keeps_journal_and_recovery_does_not_replay_committed_data() {
    let (source, _) = fixture("dark");
    let (target, state) = fixture("light");
    let artifact = source
        .store
        .export_local_bundle(BundleExportOptions::default())
        .await
        .unwrap();
    let preview = target
        .store
        .preview_local_bundle(&artifact.bytes, None, RestoreMode::Replace)
        .await
        .unwrap();
    state.0.lock().unwrap().finish_failures = 2;
    assert_eq!(
        target
            .store
            .import_local_bundle(
                &artifact.bytes,
                None,
                RestoreMode::Replace,
                &choices(&preview, ConflictSide::Remote),
                Some(preview.observed_generation),
                preview.observed_android_revision
            )
            .await
            .unwrap_err(),
        BundleError::RecoveryRequired
    );
    assert_eq!(
        target
            .store
            .inner
            .gate
            .registered_recovery_scopes()
            .unwrap()
            .len(),
        1
    );
    assert!(state.0.lock().unwrap().recovering);
    target
        .repo
        .preferences
        .update(|prefs| prefs.theme_mode = crate::contract::ThemeMode::Light)
        .unwrap();
    target.store.recover_registered().await.unwrap();
    assert_eq!(
        target.repo.preferences.get().theme_mode,
        crate::contract::ThemeMode::Light
    );
    assert_eq!(state.value(), portable("dark"));
    assert!(!state.0.lock().unwrap().recovering);
    assert!(target
        .store
        .inner
        .gate
        .registered_recovery_scopes()
        .unwrap()
        .is_empty());
}
