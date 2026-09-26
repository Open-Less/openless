use super::*;
use crate::cloud_sync_e2ee_documents::*;
use crate::cloud_sync_e2ee_protocol::{
    crypto::DerivedKey,
    types::{DocumentKind, LogicalDocument},
};
use crate::credentials::{CredentialKey, SecretValue, SyncCredentials};
use crate::{BackendError, BackendErrorCode};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use zeroize::Zeroizing;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("openless-sync-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct TestCredentials {
    value: Arc<Mutex<SyncCredentials>>,
    failures: Arc<Mutex<VecDeque<bool>>>,
    read_barrier: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    bound_gate: Arc<Mutex<Option<Arc<SyncWriteGate>>>>,
}
fn unsupported<T: Send + 'static>() -> BoxFuture<'static, Result<T, BackendError>> {
    Box::pin(async {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "fixture unused",
        ))
    })
}
impl CredentialStore for TestCredentials {
    fn bind_sync_gate(&self, gate: Arc<SyncWriteGate>) -> Result<(), BackendError> {
        let mut bound = self.bound_gate.lock().unwrap();
        if bound
            .as_ref()
            .is_some_and(|existing| !Arc::ptr_eq(existing, &gate))
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "different sync gate",
            ));
        }
        *bound = Some(gate);
        Ok(())
    }
    fn status(
        &self,
        _: crate::shared_types::UserPreferences,
    ) -> BoxFuture<'static, Result<crate::shared_types::CredentialsStatus, BackendError>> {
        unsupported()
    }
    fn read(
        &self,
        _: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        unsupported()
    }
    fn write(
        &self,
        _: CredentialKey,
        _: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported()
    }
    fn remove(&self, _: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported()
    }
    fn export_sync_credentials_readonly(
        &self,
    ) -> BoxFuture<'static, Result<SyncCredentials, BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            if this.bound_gate.lock().unwrap().is_none() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "unbound sync gate",
                ));
            }
            let value = this.value.lock().unwrap().clone();
            let barrier = this.read_barrier.lock().unwrap().clone();
            if let Some(barrier) = barrier {
                barrier.wait().await;
                barrier.wait().await;
            }
            Ok(value)
        })
    }
    fn export_sync_credentials(
        &self,
        permit: &ExclusivePermit,
    ) -> BoxFuture<'static, Result<SyncCredentials, BackendError>> {
        let valid = self
            .bound_gate
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|gate| permit.belongs_to(gate));
        let value = self.value.lock().unwrap().clone();
        Box::pin(async move {
            if !valid {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "unbound sync gate",
                ));
            }
            Ok(value)
        })
    }
    fn replace_sync_credentials(
        &self,
        value: SyncCredentials,
        permit: &ExclusivePermit,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let valid = self
            .bound_gate
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|gate| permit.belongs_to(gate));
        let this = self.clone();
        Box::pin(async move {
            if !valid {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "unbound sync gate",
                ));
            }
            if this.failures.lock().unwrap().pop_front().unwrap_or(false) {
                return Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "fixture credential failure",
                ));
            }
            *this.value.lock().unwrap() = value;
            Ok(())
        })
    }
}
struct Extensions {
    values: Mutex<BTreeMap<String, SecretJson>>,
    protector: Arc<dyn JournalProtector>,
    key_requested: AtomicBool,
    ui_revision: Mutex<Option<String>>,
}
impl ProtectedExtensionStore for Extensions {
    fn read_scope(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>> {
        Box::pin(async move {
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(&state::scope_id(&scope)?)
                .cloned())
        })
    }
    fn write_scope(
        &self,
        scope: SyncScope,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            self.values
                .lock()
                .unwrap()
                .insert(state::scope_id(&scope)?, value);
            Ok(())
        })
    }
    fn read_device(
        &self,
        key: DeviceExtensionKey,
    ) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>> {
        Box::pin(async move { Ok(self.values.lock().unwrap().get(key.storage_name()).cloned()) })
    }
    fn write_device(
        &self,
        key: DeviceExtensionKey,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            self.values
                .lock()
                .unwrap()
                .insert(key.storage_name().into(), value);
            if key == DeviceExtensionKey::UiPreferences {
                *self.ui_revision.lock().unwrap() = Some(uuid::Uuid::new_v4().to_string());
            }
            Ok(())
        })
    }
    fn read_ui_revision(&self) -> BoxFuture<'_, DocumentResult<Option<String>>> {
        Box::pin(async move { Ok(self.ui_revision.lock().unwrap().clone()) })
    }
    fn journal_protector(&self) -> BoxFuture<'_, DocumentResult<Arc<dyn JournalProtector>>> {
        Box::pin(async move {
            self.key_requested.store(true, Ordering::Release);
            Ok(Arc::clone(&self.protector))
        })
    }
}
#[derive(Clone, Default)]
struct TestEffects {
    targets: Arc<Mutex<Vec<crate::shared_types::UserPreferences>>>,
    failures: Arc<Mutex<VecDeque<bool>>>,
    wait: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    gate: Arc<Mutex<Option<std::sync::Weak<SyncWriteGate>>>>,
}
impl crate::config::RestoreRuntimeEffects for TestEffects {
    fn apply_target(
        &self,
        target: crate::shared_types::UserPreferences,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let this = self.clone();
        Box::pin(async move {
            let gate = this
                .gate
                .lock()
                .unwrap()
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .unwrap();
            assert!(
                gate.try_exclusive().is_err(),
                "effects must remain inside the exclusive restore fence"
            );
            assert!(
                gate.begin_mutation().is_err(),
                "ordinary writes must remain blocked during effects"
            );
            this.targets.lock().unwrap().push(target);
            let barrier = this.wait.lock().unwrap().clone();
            if let Some(barrier) = barrier {
                barrier.wait().await;
                barrier.wait().await;
            }
            if this.failures.lock().unwrap().pop_front().unwrap_or(false) {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture effects failure",
                ))
            } else {
                Ok(())
            }
        })
    }
}

struct Fixture {
    _temp: Temp,
    store: CoreSyncStore,
    repo: BackendRepositories,
    credentials: TestCredentials,
    extensions: Arc<Extensions>,
    scope: SyncScope,
    effects: TestEffects,
}
impl Fixture {
    fn new() -> Self {
        let temp = Temp::new();
        let gate = gate::open_for_data_dir(&temp.0).unwrap();
        let repo = BackendRepositories::open(&temp.0).unwrap();
        let credentials = TestCredentials {
            value: Arc::new(Mutex::new(SyncCredentials {
                channels: vec![],
                credentials: vec![],
            })),
            failures: Arc::new(Mutex::new(VecDeque::new())),
            read_barrier: Arc::new(Mutex::new(None)),
            bound_gate: Arc::new(Mutex::new(None)),
        };
        let extensions = Arc::new(Extensions {
            values: Mutex::new(BTreeMap::from([(
                "sync-ui-preferences".into(),
                ui_preferences("en", "medium"),
            )])),
            protector: Arc::new(CryptoJournalProtector::new(Arc::new(
                DerivedKey::from_secret_bytes(Zeroizing::new([37; 32])),
            ))),
            key_requested: AtomicBool::new(false),
            ui_revision: Mutex::new(Some(uuid::Uuid::new_v4().to_string())),
        });
        let device = SourceDevice {
            id: "fixture-device".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            app_version: "2.0.0-Beta.3".into(),
        };
        let scope = SyncScope {
            service_origin: "https://sync.example.test".into(),
            owner_github_id: "42".into(),
            vault_id: uuid::Uuid::new_v4().to_string(),
            key_id: uuid::Uuid::new_v4().to_string(),
            device_id: device.id.clone(),
        };
        let store = CoreSyncStore::new(
            repo.clone(),
            Arc::new(credentials.clone()),
            temp.0.clone(),
            device,
            gate,
            extensions.clone(),
        )
        .unwrap();
        store.bind_runtime_idle_probe(Arc::new(|| true)).unwrap();
        let effects = TestEffects::default();
        *effects.gate.lock().unwrap() = Some(Arc::downgrade(&store.inner.gate));
        store
            .bind_runtime_effects(Arc::new(effects.clone()))
            .unwrap();
        Self {
            _temp: temp,
            store,
            repo,
            credentials,
            extensions,
            scope,
            effects,
        }
    }
    fn reopened(self) -> Self {
        let Self {
            _temp,
            store,
            repo,
            credentials,
            extensions,
            scope,
            effects,
        } = self;
        let device = store.device();
        drop(store);
        drop(repo);
        // Stand-in for reopening the persistent OS vault, not retaining the old process gate.
        credentials.bound_gate.lock().unwrap().take();
        let gate = gate::open_for_data_dir(&_temp.0).unwrap();
        let repo = BackendRepositories::open(&_temp.0).unwrap();
        let store = CoreSyncStore::new(
            repo.clone(),
            Arc::new(credentials.clone()),
            _temp.0.clone(),
            device,
            gate,
            extensions.clone(),
        )
        .unwrap();
        store.bind_runtime_idle_probe(Arc::new(|| true)).unwrap();
        *effects.gate.lock().unwrap() = Some(Arc::downgrade(&store.inner.gate));
        store
            .bind_runtime_effects(Arc::new(effects.clone()))
            .unwrap();
        Self {
            _temp,
            store,
            repo,
            credentials,
            extensions,
            scope,
            effects,
        }
    }

    fn context(&self, export: &ExportedDocuments) -> RestoreContext {
        RestoreContext {
            scope: self.scope.clone(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            observed_revision: Revision::new(3),
            local_generation: export.generation,
            target_device: self.store.device(),
        }
    }
}
fn set_doc(set: &mut DocumentSet, kind: DocumentKind, id: &str, value: Value) {
    if let Some(old) = set
        .documents
        .iter_mut()
        .find(|doc| doc.kind == kind && doc.id == id)
    {
        old.value = value;
    } else {
        set.documents.push(LogicalDocument {
            id: id.into(),
            kind,
            schema_version: 1,
            value,
        });
    }
}
fn validate(set: DocumentSet) -> ValidatedSyncDocuments {
    validate_sync_documents(set, Revision::new(3)).unwrap()
}

#[tokio::test]
async fn disabled_fresh_start_never_requests_local_key() {
    let fixture = Fixture::new();
    fixture.store.recover_registered().await.unwrap();
    assert!(!fixture.extensions.key_requested.load(Ordering::Acquire));
}

#[tokio::test]
async fn complete_native_restore_keeps_all_history_order_unknown_preferences_and_activity_provenance(
) {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "futureSetting",
        json!({"nested":[1,2,3]}),
    );
    set_doc(
        &mut desired,
        DocumentKind::UiPreferences,
        "locale",
        json!("zh-CN"),
    );
    for index in 0..205 {
        let id = format!("history-{index:03}");
        set_doc(
            &mut desired,
            DocumentKind::History,
            &id,
            json!({"id":id,"createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":format!("fixture {index}"),"finalText":format!("fixture {index}"),"mode":"raw","insertStatus":"notRequested","hasAudioRecording":false,"sortIndex":204-index}),
        );
    }
    for (source, count) in [("fixture-device", 4), ("other-device", 7)] {
        set_doc(
            &mut desired,
            DocumentKind::Activity,
            &format!("{source}:2026-09-26"),
            json!({"sourceDeviceId":source,"date":"2026-09-26","count":count,"chars":10,"durationMs":20}),
        );
    }
    let expected = native::canonicalize_native_documents(validate(desired)).unwrap();
    let result = fixture
        .store
        .restore_scope(expected.clone(), fixture.context(&before))
        .await
        .unwrap();
    assert_eq!(
        result.documents.documents().documents,
        expected.documents().documents
    );
    assert_eq!(result.generation.get(), before.generation.get() + 1);
    let history = fixture.repo.history.list().unwrap();
    assert_eq!(history.len(), 205);
    assert_eq!(history[0].id, "history-204");
    assert_eq!(fixture.repo.activity.snapshot().unwrap()[0].count, 11);
    let raw: Value =
        serde_json::from_slice(&std::fs::read(fixture._temp.0.join("preferences.json")).unwrap())
            .unwrap();
    assert!(
        raw.get("futureSetting").is_none(),
        "unknown remote preference stays in encrypted extension"
    );
    let again = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert!(again
        .documents
        .documents()
        .documents
        .iter()
        .any(|doc| doc.id == "futureSetting"));
    assert!(fixture
        .store
        .inner
        .gate
        .registered_recovery_scopes()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn real_files_roll_back_after_credential_failure_and_recover_if_rollback_also_fails() {
    for failures in [vec![true, false], vec![true, true, false]] {
        let fixture = Fixture::new();
        let before = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        let mut desired = before.documents.documents().clone();
        set_doc(
            &mut desired,
            DocumentKind::Preferences,
            "themeMode",
            json!("dark"),
        );
        fixture
            .credentials
            .failures
            .lock()
            .unwrap()
            .extend(failures.clone());
        let result = fixture
            .store
            .restore_scope(validate(desired), fixture.context(&before))
            .await;
        assert_eq!(
            result.unwrap_err(),
            if failures.len() == 2 {
                DocumentError::RestoreRolledBack
            } else {
                DocumentError::RecoveryRequired
            }
        );
        if failures.len() == 3 {
            assert!(fixture.store.is_recovering());
            let journal = std::fs::read_dir(fixture._temp.0.join("encrypted-sync"))
                .unwrap()
                .filter_map(Result::ok)
                .find(|entry| entry.file_name().to_string_lossy().starts_with("restore-"))
                .unwrap();
            let bytes = std::fs::read(journal.path()).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("themeMode"));
            fixture.store.recover_registered().await.unwrap();
        }
        let after = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        assert_eq!(
            after.documents.documents().documents,
            before.documents.documents().documents
        );
        assert_eq!(after.generation, before.generation);
        assert!(!fixture.store.is_recovering());
    }
}

#[tokio::test]
async fn optimistic_capture_does_not_block_writes_and_discards_a_mixed_snapshot() {
    let fixture = Fixture::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *fixture.credentials.read_barrier.lock().unwrap() = Some(barrier.clone());
    let store = fixture.store.clone();
    let scope = fixture.scope.clone();
    let capture = tokio::spawn(async move { store.export_scope(scope).await });
    barrier.wait().await;
    fixture
        .repo
        .preferences
        .update(|prefs| prefs.show_capsule = !prefs.show_capsule)
        .unwrap();
    barrier.wait().await;
    assert_eq!(
        capture.await.unwrap().unwrap_err(),
        DocumentError::SourceChanged
    );
}

#[test]
fn every_preference_save_preserves_top_level_and_nested_unknown_values() {
    let fixture = Fixture::new();
    let path = fixture._temp.0.join("preferences.json");
    let mut raw: Value = serde_json::to_value(fixture.repo.preferences.get()).unwrap();
    raw["futureSetting"] = json!({"x":9});
    raw["customStylePrompts"]["futureMode"] = json!("keep");
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    fixture
        .repo
        .preferences
        .update(|prefs| prefs.show_capsule = !prefs.show_capsule)
        .unwrap();
    fixture
        .repo
        .preferences
        .set_preserving_current_style_preferences(fixture.repo.preferences.get())
        .unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["futureSetting"], raw["futureSetting"]);
    assert_eq!(saved["customStylePrompts"]["futureMode"], "keep");
}

#[test]
fn unknown_nested_rows_abort_ordinary_mutation_without_writing_or_dirty_generation() {
    let fixture = Fixture::new();
    let generation = fixture.store.generation().unwrap();
    let path = fixture._temp.0.join("style-packs.json");
    let mut styles: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    styles[0]["examples"] = json!([{"input":"a","output":"b","future":{"x":1}}]);
    let bytes = serde_json::to_vec(&styles).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let id = styles[0]["id"].as_str().unwrap();
    assert_eq!(
        fixture
            .repo
            .style_packs
            .set_enabled(id, false)
            .unwrap_err()
            .code,
        BackendErrorCode::Unsupported
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(fixture.store.generation().unwrap(), generation);
    let activity = fixture._temp.0.join("activity.json");
    let bytes =
        br#"{"2026-09-26":{"count":1,"sourceContributions":{"device":{"count":1,"future":2}}}}"#;
    std::fs::write(&activity, bytes).unwrap();
    assert_eq!(
        fixture
            .repo
            .activity
            .bump("2026-09-26", 1, 2)
            .unwrap_err()
            .code,
        BackendErrorCode::Unsupported
    );
    assert_eq!(std::fs::read(activity).unwrap(), bytes);
}

#[tokio::test]
async fn baseline_updates_do_not_erase_newer_local_edits_or_tombstones() {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    fixture
        .repo
        .preferences
        .update(|prefs| prefs.show_capsule = !prefs.show_capsule)
        .unwrap();
    let current = fixture.repo.preferences.get().show_capsule;
    fixture
        .store
        .record_baseline(
            fixture.scope.clone(),
            before.documents.documents().clone(),
            Revision::new(3),
        )
        .await
        .unwrap();
    assert_eq!(fixture.repo.preferences.get().show_capsule, current);
    let after = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert!(after.generation > before.generation);
}

fn png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO3c3b0AAAAASUVORK5CYII=").unwrap()
}

#[test]
fn failed_style_save_never_changes_live_cache() {
    let fixture = Fixture::new();
    let before = fixture.repo.style_packs.get("builtin.light").unwrap();
    let mut next = before.clone();
    next.name = "new title".into();
    let generation = fixture.store.generation().unwrap();
    crate::persistence::fail_next_atomic_write(&fixture._temp.0.join("style-packs.json"), false);
    assert!(fixture.repo.style_packs.update(next).is_err());
    assert_eq!(
        fixture.repo.style_packs.get("builtin.light").unwrap(),
        before
    );
    assert_eq!(fixture.store.generation().unwrap(), generation);
    assert!(!fixture.store.is_recovering());
}

#[tokio::test]
async fn uncertain_style_icon_and_import_commits_keep_their_referenced_assets() {
    for import in [false, true] {
        let fixture = Fixture::new();
        let pack = fixture
            .repo
            .style_packs
            .update_icon("builtin.light", Some(&png()))
            .unwrap();
        let original = pack.icon_path.unwrap();
        let archive = fixture
            .repo
            .style_packs
            .export_zip_bytes("builtin.light")
            .unwrap();
        crate::persistence::fail_next_atomic_write(&fixture._temp.0.join("style-packs.json"), true);
        let error = if import {
            fixture
                .repo
                .style_packs
                .import_from_zip_bytes(&archive)
                .unwrap_err()
        } else {
            fixture
                .repo
                .style_packs
                .update_icon("builtin.light", Some(&png()))
                .unwrap_err()
        };
        assert_eq!(error.code, BackendErrorCode::OutcomeUnknown);
        assert!(fixture.store.is_recovering());
        let persisted: Vec<crate::style_packs::StylePack> = serde_json::from_slice(
            &std::fs::read(fixture._temp.0.join("style-packs.json")).unwrap(),
        )
        .unwrap();
        for pack in &persisted {
            if let Some(path) = &pack.icon_path {
                assert!(std::path::Path::new(path).is_file());
            }
        }
        assert!(std::path::Path::new(&original).is_file());
        fixture.store.recover_registered().await.unwrap();
        assert!(!fixture.store.is_recovering());
        assert_eq!(fixture.repo.style_packs.list().unwrap(), persisted);
    }
}

#[tokio::test]
async fn runtime_probe_rejects_restore_before_any_file_changes() {
    let fixture = Fixture::new();
    *fixture.store.inner.runtime_idle.lock().unwrap() = Some(Arc::new(|| false));
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    let result = fixture
        .store
        .restore_scope(validate(desired), fixture.context(&before))
        .await;
    assert_eq!(result.unwrap_err(), DocumentError::RuntimeBusy);
    assert!(!fixture.store.is_recovering());
    assert_eq!(fixture.store.generation().unwrap(), before.generation);
    assert!(!fixture.extensions.key_requested.load(Ordering::Acquire));
}

#[tokio::test]
async fn activity_restore_beyond_rolling_retention_keeps_every_source_record() {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    let first = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    for index in 0..733 {
        let date = (first + chrono::Days::new(index)).to_string();
        set_doc(
            &mut desired,
            DocumentKind::Activity,
            &format!("fixture-device:{date}"),
            json!({"sourceDeviceId":"fixture-device","date":date,"count":1,"chars":2,"durationMs":3}),
        );
    }
    fixture
        .store
        .restore_scope(validate(desired), fixture.context(&before))
        .await
        .unwrap();
    assert_eq!(fixture.repo.activity.snapshot().unwrap().len(), 733);
}

#[tokio::test]
async fn all_eleven_kinds_restore_stable_ids_credentials_icons_presets_and_foreign_profiles() {
    use base64::Engine;
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    set_doc(
        &mut desired,
        DocumentKind::UiPreferences,
        "locale",
        json!("ja"),
    );
    set_doc(
        &mut desired,
        DocumentKind::Channels,
        "llm:stable-llm",
        json!({"id":"stable-llm","namespace":"llm","providerType":"requesty","name":"Fixture API","enabled":true,"order":0,"active":true}),
    );
    set_doc(
        &mut desired,
        DocumentKind::ProviderCredentials,
        "llm:stable-llm",
        json!({"channelId":"stable-llm","namespace":"llm","accounts":{"ark.api_key":"fixture-private-key","ark.endpoint":"https://router.requesty.ai/v1","ark.model_id":"fixture-model"}}),
    );
    set_doc(
        &mut desired,
        DocumentKind::Dictionary,
        "stable-word",
        json!({"id":"stable-word","phrase":"OpenLess fixture","note":"fixture note","enabled":true,"hits":2,"createdAt":"2026-09-26T00:00:00Z","sortIndex":0}),
    );
    set_doc(
        &mut desired,
        DocumentKind::Corrections,
        "stable-rule",
        json!({"id":"stable-rule","pattern":"opnless","replacement":"OpenLess","enabled":true,"createdAt":"2026-09-26T00:00:00Z","source":"learned","sortIndex":0}),
    );
    set_doc(
        &mut desired,
        DocumentKind::VocabularyPresets,
        "custom:stable-preset",
        json!({"id":"stable-preset","origin":"custom","name":"Private words","phrases":["one","two"],"enabled":true,"order":0}),
    );
    let builtin = crate::vocabulary::builtin_vocab_presets()
        .into_iter()
        .next()
        .unwrap()
        .id;
    set_doc(
        &mut desired,
        DocumentKind::VocabularyPresets,
        &format!("override:{builtin}"),
        json!({"id":builtin,"origin":"override","name":"Override words","phrases":["three"],"enabled":true,"order":0}),
    );
    let icon = base64::engine::general_purpose::STANDARD.encode(png());
    let style = desired
        .documents
        .iter_mut()
        .find(|doc| doc.kind == DocumentKind::StylePacks && doc.id == "builtin.light")
        .unwrap();
    style.value["icon"] = json!({"mime":"image/png","base64":icon});
    set_doc(
        &mut desired,
        DocumentKind::History,
        "stable-history",
        json!({"id":"stable-history","createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":"Fixture note","finalText":"Fixture note","mode":"raw","insertStatus":"notRequested","hasAudioRecording":false,"sortIndex":0}),
    );
    set_doc(
        &mut desired,
        DocumentKind::Activity,
        "foreign-device:2026-09-26",
        json!({"sourceDeviceId":"foreign-device","date":"2026-09-26","count":2,"chars":4,"durationMs":8}),
    );
    set_doc(
        &mut desired,
        DocumentKind::DeviceProfile,
        "foreign-device",
        json!({"device":{"id":"foreign-device","os":"windows","arch":"x86_64","appVersion":"2.0.0-Beta.3"},"preferences":{"microphoneDeviceName":"foreign microphone","codingAgentExe":"C:\\fixture\\do-not-run.exe"},"channels":[{"id":"foreign-local","namespace":"asr","providerType":"foundry-local-whisper","name":"Foreign local model","enabled":true,"order":0,"active":true}],"providerCredentials":[{"channelId":"foreign-local","namespace":"asr","accounts":{"asr.model":"fixture-local-model"}}],"windowPositions":[]}),
    );
    let desired = native::canonicalize_native_documents(validate(desired)).unwrap();
    let result = fixture
        .store
        .restore_scope(desired.clone(), fixture.context(&before))
        .await
        .unwrap();
    assert_eq!(
        result.documents.documents().documents,
        desired.documents().documents
    );
    let kinds = result
        .documents
        .documents()
        .documents
        .iter()
        .map(|doc| doc.kind)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds.len(), 11);
    let credentials = fixture.credentials.value.lock().unwrap();
    assert_eq!(credentials.channels.len(), 1);
    assert_eq!(credentials.channels[0].id, "stable-llm");
    assert_eq!(
        credentials.credentials[0].accounts["ark.api_key"],
        "fixture-private-key"
    );
    drop(credentials);
    assert_ne!(
        fixture.repo.preferences.get().microphone_device_name,
        "foreign microphone"
    );
    assert_eq!(fixture.repo.vocabulary.list().unwrap()[0].id, "stable-word");
    assert_eq!(
        fixture.repo.correction_rules.list().unwrap()[0].id,
        "stable-rule"
    );
    let presets = crate::vocabulary::list_vocab_presets(&fixture._temp.0).unwrap();
    assert_eq!(presets.custom[0].id, "stable-preset");
    assert_eq!(presets.overrides[0].phrases, vec!["three"]);
    assert!(fixture
        .repo
        .style_packs
        .icon_data_url("builtin.light")
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn deletion_ledger_and_upload_baseline_are_isolated_by_account_vault_scope() {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Dictionary,
        "deleted-word",
        json!({"id":"deleted-word","phrase":"remove me","note":null,"enabled":true,"hits":0,"createdAt":"2026-09-26T00:00:00Z","sortIndex":0}),
    );
    let seeded = fixture
        .store
        .restore_scope(validate(desired), fixture.context(&before))
        .await
        .unwrap();
    fixture
        .store
        .record_baseline(
            fixture.scope.clone(),
            seeded.documents.documents().clone(),
            Revision::new(3),
        )
        .await
        .unwrap();
    fixture.repo.vocabulary.remove("deleted-word").unwrap();
    let deleted = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert!(deleted
        .documents
        .documents()
        .tombstones
        .iter()
        .any(|mark| mark.id == "deleted-word"));
    let mut other = fixture.scope.clone();
    other.owner_github_id = "43".into();
    other.vault_id = uuid::Uuid::new_v4().to_string();
    let fresh = fixture.store.export_scope(other).await.unwrap();
    assert!(fresh.documents.documents().tombstones.is_empty());
    fixture
        .repo
        .preferences
        .update(|prefs| prefs.show_capsule = !prefs.show_capsule)
        .unwrap();
    fixture
        .store
        .record_baseline(
            fixture.scope.clone(),
            deleted.documents.documents().clone(),
            Revision::new(4),
        )
        .await
        .unwrap();
    let later = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert!(later
        .documents
        .documents()
        .tombstones
        .iter()
        .any(|mark| mark.id == "deleted-word"));
    assert!(later.generation > deleted.generation);
}

#[tokio::test]
async fn ui_compare_and_swap_rejects_an_old_mirror_after_restore_and_preserves_generation() {
    let fixture = Fixture::new();
    let old = fixture.extensions.read_ui_revision().await.unwrap();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::UiPreferences,
        "locale",
        json!("ja"),
    );
    let restored = fixture
        .store
        .restore_scope(validate(desired), fixture.context(&before))
        .await
        .unwrap();
    let result = fixture
        .store
        .set_ui_preferences_checked(ui_preferences("en", "large"), old)
        .await;
    assert_eq!(result.unwrap_err(), DocumentError::StalePreview);
    assert_eq!(fixture.store.generation().unwrap(), restored.generation);
    assert_eq!(
        fixture
            .extensions
            .read_device(DeviceExtensionKey::UiPreferences)
            .await
            .unwrap()
            .unwrap()
            .expose()["locale"],
        "ja"
    );
    assert_eq!(
        fixture
            .store
            .set_ui_preferences_checked(ui_preferences("de", "small"), None)
            .await
            .unwrap_err(),
        DocumentError::StalePreview
    );
    fixture
        .store
        .set_ui_preferences_checked(
            ui_preferences("ja", "large"),
            fixture.extensions.read_ui_revision().await.unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.store.generation().unwrap().get(),
        restored.generation.get() + 1
    );
}

#[test]
fn preference_array_extensions_follow_stable_identity_across_ordinary_updates() {
    let fixture = Fixture::new();
    let path = fixture._temp.0.join("preferences.json");
    let mut raw = serde_json::to_value(fixture.repo.preferences.get()).unwrap();
    raw["stylePackHotkeys"] = json!([{"packId":"builtin.light","binding":{"primary":"L","modifiers":["ctrl"],"futureBinding":true},"futureHotkey":{"x":1}}]);
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    fixture
        .repo
        .preferences
        .update(|prefs| prefs.show_capsule = !prefs.show_capsule)
        .unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["stylePackHotkeys"][0]["futureHotkey"], json!({"x":1}));
    assert_eq!(
        saved["stylePackHotkeys"][0]["binding"]["futureBinding"],
        true
    );
}

#[test]
fn pending_restore_constructors_never_salvage_or_normalize_original_files() {
    let temp = Temp::new();
    let gate = gate::open_for_data_dir(&temp.0).unwrap();
    let permit = gate.try_exclusive().unwrap();
    let scope = SyncScope {
        service_origin: "https://sync.example.test".into(),
        owner_github_id: "42".into(),
        vault_id: uuid::Uuid::new_v4().to_string(),
        key_id: uuid::Uuid::new_v4().to_string(),
        device_id: "fixture-device".into(),
    };
    permit
        .mark_restore_pending(
            &uuid::Uuid::new_v4().to_string(),
            &state::scope_id(&scope).unwrap(),
            &scope,
        )
        .unwrap();
    let preferences = temp.0.join("preferences.json");
    let styles = temp.0.join("style-packs.json");
    std::fs::write(
        &preferences,
        br#"{"defaultMode":"unknown-future-mode","streamingInsert":false}"#,
    )
    .unwrap();
    std::fs::write(&styles, b"[]").unwrap();
    let before = std::fs::read(&preferences).unwrap();
    let _prefs = crate::preferences::PreferencesStore::open(&preferences).unwrap();
    let style_store = crate::style_pack_store::StylePackStore::at_data_dir(&temp.0).unwrap();
    assert!(style_store.list().unwrap().is_empty());
    assert_eq!(std::fs::read(preferences).unwrap(), before);
    assert_eq!(std::fs::read(styles).unwrap(), b"[]");
    assert_eq!(std::fs::read_dir(&temp.0).unwrap().count(), 3);
}

#[tokio::test]
async fn ui_revision_cas_rejects_a_pre_restore_writer_even_when_values_are_identical() {
    let fixture = Fixture::new();
    let old = fixture.extensions.read_ui_revision().await.unwrap();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let desired =
        validate_sync_documents(before.documents.documents().clone(), Revision::new(3)).unwrap();
    fixture
        .store
        .restore_scope(desired, fixture.context(&before))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .extensions
            .read_device(DeviceExtensionKey::UiPreferences)
            .await
            .unwrap()
            .unwrap(),
        ui_preferences("en", "medium")
    );
    assert_ne!(fixture.extensions.read_ui_revision().await.unwrap(), old);
    assert_eq!(
        fixture
            .store
            .set_ui_preferences_checked(ui_preferences("en", "large"), old)
            .await
            .unwrap_err(),
        DocumentError::StalePreview
    );
}

#[tokio::test]
async fn restoring_history_never_replaces_existing_local_media_availability_with_remote_flags() {
    let fixture = Fixture::new();
    let record:crate::types::DictationSession=serde_json::from_value(json!({"id":"local-recording","createdAt":"2026-09-26T00:00:00Z","source":"quick_note","rawTranscript":"fixture","finalText":"fixture","mode":"raw","insertStatus":"notRequested","hasAudioRecording":true})).unwrap();
    fixture
        .repo
        .history
        .append_with_retention(record, 0, None)
        .unwrap();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    assert_eq!(
        before
            .documents
            .documents()
            .documents
            .iter()
            .find(|doc| doc.kind == DocumentKind::History)
            .unwrap()
            .value["hasAudioRecording"],
        false
    );
    let desired =
        validate_sync_documents(before.documents.documents().clone(), Revision::new(3)).unwrap();
    fixture
        .store
        .restore_scope(desired, fixture.context(&before))
        .await
        .unwrap();
    assert_eq!(
        fixture.repo.history.list().unwrap()[0].has_audio_recording,
        Some(true)
    );
}

#[tokio::test]
async fn constructing_store_binds_the_exact_gate_required_by_real_credential_operations() {
    let fixture = Fixture::new();
    let bound = fixture
        .credentials
        .bound_gate
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    assert!(Arc::ptr_eq(&bound, &fixture.store.inner.gate));
    *fixture.credentials.bound_gate.lock().unwrap() = None;
    assert!(fixture
        .credentials
        .export_sync_credentials_readonly()
        .await
        .is_err());
    fixture.credentials.bind_sync_gate(bound).unwrap();
    fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_device_path_restore_restores_original_local_grant_without_exporting_it() {
    for fail_rollback in [false, true] {
        let fixture = Fixture::new();
        fixture
            .repo
            .preferences
            .update(|prefs| {
                prefs.coding_agent_enabled = true;
                prefs.coding_agent_exe = Some("/fixture/original-agent".into());
            })
            .unwrap();
        let before = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        assert!(!serde_json::to_string(before.documents.documents())
            .unwrap()
            .contains("codingAgentEnabled"));
        let mut desired = before.documents.documents().clone();
        desired
            .documents
            .iter_mut()
            .find(|doc| doc.kind == DocumentKind::DeviceProfile && doc.id == "fixture-device")
            .unwrap()
            .value["preferences"]["codingAgentExe"] = json!("/fixture/never-run-imported-agent");
        fixture
            .credentials
            .failures
            .lock()
            .unwrap()
            .extend(if fail_rollback {
                vec![true, true, false]
            } else {
                vec![true, false]
            });
        assert!(fixture
            .store
            .restore_scope(validate(desired), fixture.context(&before))
            .await
            .is_err());
        if fail_rollback {
            fixture.store.recover_registered().await.unwrap();
        }
        let prefs = fixture.repo.preferences.get();
        assert!(prefs.coding_agent_enabled);
        assert_eq!(
            prefs.coding_agent_exe.as_deref(),
            Some("/fixture/original-agent")
        );
    }
}

#[tokio::test]
async fn deleted_history_rollback_restores_all_local_audio_markers_and_keeps_wav_files() {
    for restart_recovery in [false, true] {
        let mut fixture = Fixture::new();
        let recording_dir = fixture._temp.0.join("recordings");
        std::fs::create_dir_all(&recording_dir).unwrap();
        let mut expected = std::collections::BTreeMap::new();
        let mut recording = None;
        for marker in [Some(true), Some(false), None] {
            let id = uuid::Uuid::new_v4().to_string();
            if marker == Some(true) {
                let path = recording_dir.join(format!("{id}.wav"));
                std::fs::write(&path, b"fixture local recording remains untouched").unwrap();
                recording = Some(path);
            }
            let row:crate::types::DictationSession=serde_json::from_value(json!({"id":id,"createdAt":"2026-09-26T00:00:00Z","rawTranscript":"fixture","finalText":"fixture","mode":"raw","insertStatus":"inserted","hasAudioRecording":marker})).unwrap();
            fixture
                .repo
                .history
                .append_with_retention(row, 0, None)
                .unwrap();
            expected.insert(id, marker);
        }
        let before = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        assert!(before
            .documents
            .documents()
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::History)
            .all(|doc| doc.value["hasAudioRecording"] == false));
        let mut desired = before.documents.documents().clone();
        desired
            .documents
            .retain(|doc| doc.kind != DocumentKind::History);
        for id in expected.keys() {
            desired
                .tombstones
                .push(crate::cloud_sync_e2ee_protocol::types::Tombstone {
                    id: id.clone(),
                    kind: DocumentKind::History,
                    deleted_at: "2026-09-26T01:00:00Z".into(),
                    base_revision: Revision::new(0),
                });
        }
        fixture
            .credentials
            .failures
            .lock()
            .unwrap()
            .extend(if restart_recovery {
                vec![true, true, false]
            } else {
                vec![true, false]
            });
        let result = fixture
            .store
            .restore_scope(validate(desired), fixture.context(&before))
            .await;
        assert_eq!(
            result.unwrap_err(),
            if restart_recovery {
                DocumentError::RecoveryRequired
            } else {
                DocumentError::RestoreRolledBack
            }
        );
        if restart_recovery {
            assert!(fixture.store.is_recovering());
            fixture = fixture.reopened();
            fixture.store.recover_registered().await.unwrap();
        }
        let actual: std::collections::BTreeMap<_, _> = fixture
            .repo
            .history
            .list()
            .unwrap()
            .into_iter()
            .map(|record| (record.id, record.has_audio_recording))
            .collect();
        assert_eq!(actual,expected,"rollback must preserve true, false and unknown receiver-only markers after rows were removed");
        assert_eq!(
            std::fs::read(recording.unwrap()).unwrap(),
            b"fixture local recording remains untouched"
        );
        let exported = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        assert!(exported
            .documents
            .documents()
            .documents
            .iter()
            .filter(|doc| doc.kind == DocumentKind::History)
            .all(|doc| doc.value["hasAudioRecording"] == false));
        assert_eq!(fixture.store.generation().unwrap(), before.generation);
        assert!(!fixture.store.is_recovering());
    }
}

#[tokio::test]
async fn remote_history_audio_true_cannot_create_local_media_availability() {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::History,
        "foreign-recording",
        json!({"id":"foreign-recording","createdAt":"2026-09-26T00:00:00Z","rawTranscript":"fixture","finalText":"fixture","mode":"raw","insertStatus":"inserted","hasAudioRecording":true,"sortIndex":0}),
    );
    assert_eq!(
        validate_sync_documents(desired, Revision::new(3)).unwrap_err(),
        DocumentError::ExcludedField
    );
    assert!(fixture
        .repo
        .history
        .read_entry("foreign-recording")
        .unwrap()
        .is_none());
    assert!(!fixture.store.is_recovering());
}

#[tokio::test]
async fn restore_requires_bound_runtime_effects_before_local_writes() {
    let fixture = Fixture::new();
    *fixture.store.inner.runtime_effects.lock().unwrap() = None;
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    assert_eq!(
        fixture
            .store
            .restore_scope(validate(desired), fixture.context(&before))
            .await
            .unwrap_err(),
        DocumentError::Unsupported
    );
    assert_eq!(fixture.store.generation().unwrap(), before.generation);
    assert!(!fixture.store.is_recovering());
    assert!(!fixture.extensions.key_requested.load(Ordering::Acquire));
    assert!(fixture.effects.targets.lock().unwrap().is_empty());
}

#[tokio::test]
async fn runtime_effects_are_awaited_under_the_exclusive_and_runtime_fences() {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *fixture.effects.wait.lock().unwrap() = Some(barrier.clone());
    let store = fixture.store.clone();
    let context = fixture.context(&before);
    let restore =
        tokio::spawn(async move { store.restore_scope(validate(desired), context).await });
    barrier.wait().await;
    assert!(!restore.is_finished());
    assert!(fixture.store.is_recovering());
    assert!(fixture.store.ensure_runtime_available().is_err());
    assert!(fixture
        .repo
        .preferences
        .update(|prefs| prefs.show_capsule = false)
        .is_err());
    barrier.wait().await;
    let result = restore.await.unwrap().unwrap();
    assert_eq!(result.generation.get(), before.generation.get() + 1);
    assert!(!fixture.store.is_recovering());
    assert_eq!(fixture.effects.targets.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn effects_failure_rolls_back_and_final_local_grants_are_applied_again() {
    for startup_recovery in [false, true] {
        let mut fixture = Fixture::new();
        fixture
            .repo
            .preferences
            .update(|prefs| {
                prefs.coding_agent_enabled = true;
                prefs.coding_agent_exe = Some("/fixture/original".into());
            })
            .unwrap();
        let before = fixture
            .store
            .export_scope(fixture.scope.clone())
            .await
            .unwrap();
        let mut desired = before.documents.documents().clone();
        desired
            .documents
            .iter_mut()
            .find(|doc| doc.kind == DocumentKind::DeviceProfile && doc.id == "fixture-device")
            .unwrap()
            .value["preferences"]["codingAgentExe"] = json!("/fixture/new-program");
        fixture
            .effects
            .failures
            .lock()
            .unwrap()
            .extend(if startup_recovery {
                vec![true, true, false, false]
            } else {
                vec![true, false, false]
            });
        assert_eq!(
            fixture
                .store
                .restore_scope(validate(desired), fixture.context(&before))
                .await
                .unwrap_err(),
            if startup_recovery {
                DocumentError::RecoveryRequired
            } else {
                DocumentError::RestoreRolledBack
            }
        );
        if startup_recovery {
            assert!(fixture.store.is_recovering());
            fixture = fixture.reopened();
            fixture.store.recover_registered().await.unwrap();
        }
        let applied = fixture.effects.targets.lock().unwrap();
        assert!(!applied[0].coding_agent_enabled);
        assert_eq!(
            applied[0].coding_agent_exe.as_deref(),
            Some("/fixture/new-program")
        );
        let final_target = applied.last().unwrap();
        assert!(final_target.coding_agent_enabled);
        assert_eq!(
            final_target.coding_agent_exe.as_deref(),
            Some("/fixture/original")
        );
        assert_eq!(fixture.store.generation().unwrap(), before.generation);
        assert!(!fixture.store.is_recovering());
    }
}

#[tokio::test]
async fn launch_at_login_is_excluded_and_preserves_the_receivers_local_setting() {
    let fixture = Fixture::new();
    fixture
        .repo
        .preferences
        .update(|prefs| prefs.launch_at_login = true)
        .unwrap();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let encoded = serde_json::to_string(before.documents.documents()).unwrap();
    assert!(!encoded.contains("launchAtLogin"));
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    fixture
        .store
        .restore_scope(validate(desired), fixture.context(&before))
        .await
        .unwrap();
    assert!(fixture.repo.preferences.get().launch_at_login);
    assert!(
        fixture
            .effects
            .targets
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .launch_at_login
    );
}

#[tokio::test]
async fn pending_recovery_without_effects_never_reads_key_or_changes_its_durable_markers() {
    let fixture = Fixture::new();
    let before = fixture
        .store
        .export_scope(fixture.scope.clone())
        .await
        .unwrap();
    let mut desired = before.documents.documents().clone();
    set_doc(
        &mut desired,
        DocumentKind::Preferences,
        "themeMode",
        json!("dark"),
    );
    fixture
        .credentials
        .failures
        .lock()
        .unwrap()
        .extend([true, true]);
    assert_eq!(
        fixture
            .store
            .restore_scope(validate(desired), fixture.context(&before))
            .await
            .unwrap_err(),
        DocumentError::RecoveryRequired
    );
    let directory = fixture._temp.0.join("encrypted-sync");
    let journal = std::fs::read_dir(&directory)
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("restore-"))
        .unwrap()
        .path();
    let generation = std::fs::read(directory.join("generation.json")).unwrap();
    let sealed = std::fs::read(&journal).unwrap();
    let effects_calls = fixture.effects.targets.lock().unwrap().len();
    *fixture.store.inner.runtime_effects.lock().unwrap() = None;
    fixture
        .extensions
        .key_requested
        .store(false, Ordering::Release);
    assert_eq!(
        fixture.store.recover_registered().await.unwrap_err(),
        DocumentError::Unsupported
    );
    assert!(!fixture.extensions.key_requested.load(Ordering::Acquire));
    assert_eq!(
        std::fs::read(directory.join("generation.json")).unwrap(),
        generation
    );
    assert_eq!(std::fs::read(journal).unwrap(), sealed);
    assert_eq!(fixture.effects.targets.lock().unwrap().len(), effects_calls);
    assert!(fixture.store.is_recovering());
}

#[test]
fn setup_prompt_store_probe_requires_idle_runtime_and_no_active_writer() {
    let fixture = Fixture::new();
    assert!(fixture.store.setup_prompt_state().unwrap().is_some());
    let writing = fixture.store.inner.gate.begin_mutation().unwrap();
    assert!(fixture.store.setup_prompt_state().unwrap().is_none());
    writing.abort_unmodified().unwrap();
    *fixture.store.inner.runtime_idle.lock().unwrap() = Some(Arc::new(|| false));
    assert!(fixture.store.setup_prompt_state().unwrap().is_none());
    *fixture.store.inner.runtime_idle.lock().unwrap() = Some(Arc::new(|| true));
    fixture.store.inner.restoring.store(true, Ordering::Release);
    assert!(fixture.store.setup_prompt_state().unwrap().is_none());
    fixture
        .store
        .inner
        .restoring
        .store(false, Ordering::Release);
    assert!(fixture.store.setup_prompt_state().unwrap().is_some());
}
