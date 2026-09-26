use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::*;
use crate::cloud_sync_e2ee_protocol::{
    crypto::DerivedKey,
    types::{DocumentKind, Revision},
};

struct State {
    data: ValidatedSyncDocuments,
    generation: Revision,
    failures: VecDeque<bool>,
    pending: bool,
    journal_ready: bool,
    receipt: Option<RestoreReceipt>,
    writes: usize,
}

#[derive(Clone)]
struct Backend(Arc<Mutex<State>>);
struct Lease(Backend);

impl RestoreBackend for Backend {
    fn acquire(
        &self,
        _scope: SyncScope,
        expected: Option<Revision>,
    ) -> BoxFuture<'_, DocumentResult<Box<dyn RestoreLease>>> {
        Box::pin(async move {
            if expected.is_some_and(|g| g != self.0.lock().unwrap().generation) {
                return Err(DocumentError::StalePreview);
            }
            Ok(Box::new(Lease(self.clone())) as Box<dyn RestoreLease>)
        })
    }
}

fn captured(state: &State) -> ExportSnapshot {
    let mut snapshot = super::tests::snapshot();
    snapshot.generation = state.generation;
    snapshot.base_revision = state.data.observed_revision();
    snapshot.preferences = SecretJson::default();
    snapshot.ui_preferences = SecretJson::default();
    for doc in &state.data.documents().documents {
        match doc.kind {
            DocumentKind::Preferences => {
                snapshot
                    .preferences
                    .expose_mut()
                    .as_object_mut()
                    .unwrap()
                    .insert(doc.id.clone(), doc.value.clone());
            }
            DocumentKind::UiPreferences => {
                snapshot
                    .ui_preferences
                    .expose_mut()
                    .as_object_mut()
                    .unwrap()
                    .insert(doc.id.clone(), doc.value.clone());
            }
            DocumentKind::Channels => snapshot
                .channels
                .push(serde_json::from_value(doc.value.clone()).unwrap()),
            DocumentKind::ProviderCredentials => snapshot
                .provider_credentials
                .push(serde_json::from_value(doc.value.clone()).unwrap()),
            DocumentKind::Dictionary => {
                snapshot.dictionary.push(SecretJson::new(doc.value.clone()))
            }
            DocumentKind::Corrections => snapshot
                .corrections
                .push(SecretJson::new(doc.value.clone())),
            DocumentKind::VocabularyPresets => snapshot
                .vocabulary_presets
                .push(serde_json::from_value(doc.value.clone()).unwrap()),
            DocumentKind::StylePacks => snapshot
                .style_packs
                .push(serde_json::from_value(doc.value.clone()).unwrap()),
            DocumentKind::History => snapshot.history.push(SecretJson::new(doc.value.clone())),
            DocumentKind::Activity => snapshot
                .activity
                .push(serde_json::from_value(doc.value.clone()).unwrap()),
            DocumentKind::DeviceProfile if doc.id == snapshot.source_device.id => {
                let profile: DeviceProfileRecord =
                    serde_json::from_value(doc.value.clone()).unwrap();
                for (key, value) in profile.preferences.expose().as_object().unwrap() {
                    snapshot
                        .preferences
                        .expose_mut()
                        .as_object_mut()
                        .unwrap()
                        .insert(key.clone(), value.clone());
                }
                snapshot.channels.extend(profile.channels);
                snapshot
                    .provider_credentials
                    .extend(profile.provider_credentials);
                snapshot.window_positions = profile.window_positions;
            }
            DocumentKind::DeviceProfile => snapshot.retained_documents.push(doc.clone()),
        }
    }
    snapshot.tombstones = state.data.documents().tombstones.clone();
    snapshot
}

impl RestoreLease for Lease {
    fn capture(&mut self) -> BoxFuture<'_, DocumentResult<ExportSnapshot>> {
        Box::pin(async move { Ok(captured(&self.0 .0.lock().unwrap())) })
    }
    fn replace(&mut self, desired: ValidatedSyncDocuments) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            let mut state = self.0 .0.lock().unwrap();
            state.writes += 1;
            if state.failures.pop_front().unwrap_or(false) {
                // A first file became visible; the following credential write failed.
                let value = desired
                    .documents()
                    .documents
                    .iter()
                    .find(|doc| doc.id == "themeMode")
                    .unwrap()
                    .value
                    .clone();
                state
                    .data
                    .set
                    .documents
                    .iter_mut()
                    .find(|doc| doc.id == "themeMode")
                    .unwrap()
                    .value = value;
                return Err(DocumentError::CaptureFailed);
            }
            state.data = desired;
            Ok(())
        })
    }
    fn reload(&mut self) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn mark_recovery_pending(&mut self, _operation: &str) -> DocumentResult<()> {
        self.0 .0.lock().unwrap().pending = true;
        Ok(())
    }
    fn mark_journal_ready(&mut self, _operation: &str) -> DocumentResult<()> {
        self.0 .0.lock().unwrap().journal_ready = true;
        Ok(())
    }
    fn recover_without_journal(&mut self) -> DocumentResult<()> {
        let mut state = self.0 .0.lock().unwrap();
        if state.pending && state.journal_ready {
            return Err(DocumentError::RecoveryRequired);
        }
        state.pending = false;
        Ok(())
    }
    fn completed_restore(&self, operation: &str) -> DocumentResult<Option<RestoreReceipt>> {
        Ok(self
            .0
             .0
            .lock()
            .unwrap()
            .receipt
            .as_ref()
            .filter(|r| r.operation_id == operation)
            .cloned())
    }
    fn finish_restore(
        &mut self,
        operation: &str,
        committed: bool,
    ) -> DocumentResult<RestoreReceipt> {
        let mut state = self.0 .0.lock().unwrap();
        if let Some(receipt) = &state.receipt {
            return Ok(receipt.clone());
        }
        if committed {
            state.generation = state.generation.checked_next().unwrap();
        }
        let receipt = RestoreReceipt {
            operation_id: operation.into(),
            generation: state.generation,
            committed,
            journal_cleanup_pending: false,
        };
        state.pending = false;
        state.receipt = Some(receipt.clone());
        Ok(receipt)
    }
}

#[derive(Default)]
struct Storage {
    value: Mutex<Option<SealedJournal>>,
    saves: AtomicUsize,
    fail_before: AtomicUsize,
    fail_after: AtomicUsize,
    fail_clear: AtomicBool,
}
impl JournalStore for Storage {
    fn load(&self, _scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SealedJournal>>> {
        Box::pin(async { Ok(self.value.lock().unwrap().clone()) })
    }
    fn save(&self, _scope: SyncScope, value: SealedJournal) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            let number = self.saves.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_before.load(Ordering::SeqCst) == number {
                return Err(DocumentError::JournalUnavailable);
            }
            *self.value.lock().unwrap() = Some(value);
            if self.fail_after.load(Ordering::SeqCst) == number {
                return Err(DocumentError::JournalUnavailable);
            }
            Ok(())
        })
    }
    fn clear(&self, _scope: SyncScope) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async {
            if self.fail_clear.load(Ordering::SeqCst) {
                Err(DocumentError::JournalUnavailable)
            } else {
                *self.value.lock().unwrap() = None;
                Ok(())
            }
        })
    }
}

fn context() -> RestoreContext {
    RestoreContext {
        scope: SyncScope {
            service_origin: "https://sync.example.test".into(),
            owner_github_id: "42".into(),
            vault_id: "12345678-1234-4234-8234-123456789abc".into(),
            key_id: "22345678-1234-4234-8234-123456789abc".into(),
            device_id: "device-a".into(),
        },
        operation_id: "32345678-1234-4234-8234-123456789abc".into(),
        observed_revision: Revision::new(3),
        local_generation: Revision::new(7),
        target_device: super::tests::source(),
    }
}

fn setup(failures: &[bool]) -> (Backend, RestorePlan, Storage, CryptoJournalProtector) {
    let data = export_snapshot(super::tests::channel_snapshot())
        .unwrap()
        .documents;
    let mut desired = data.documents().clone();
    desired
        .documents
        .iter_mut()
        .find(|doc| doc.id == "themeMode")
        .unwrap()
        .value = json!("dark");
    let desired = validate_sync_documents(desired, Revision::new(3)).unwrap();
    let plan = prepare_sync_restore(desired, context()).unwrap();
    let backend = Backend(Arc::new(Mutex::new(State {
        data,
        generation: Revision::new(7),
        failures: failures.iter().copied().collect(),
        pending: false,
        journal_ready: false,
        receipt: None,
        writes: 0,
    })));
    let protector = CryptoJournalProtector::new(Arc::new(DerivedKey::from_secret_bytes(
        Zeroizing::new([37; 32]),
    )));
    (backend, plan, Storage::default(), protector)
}

fn theme(backend: &Backend) -> Value {
    backend
        .0
        .lock()
        .unwrap()
        .data
        .documents()
        .documents
        .iter()
        .find(|doc| doc.id == "themeMode")
        .unwrap()
        .value
        .clone()
}

#[tokio::test]
async fn restore_verifies_all_logical_data_and_advances_one_generation() {
    let (backend, plan, storage, protector) = setup(&[]);
    let receipt = apply_sync_restore(plan, &backend, &storage, &protector)
        .await
        .unwrap();
    assert!(receipt.committed);
    assert_eq!(receipt.generation.get(), 8);
    assert_eq!(theme(&backend), "dark");
    assert!(!backend.0.lock().unwrap().pending);
    assert!(storage.value.lock().unwrap().is_none());
}

#[tokio::test]
async fn credential_failure_rolls_back_the_already_written_file_without_false_success() {
    let (backend, plan, storage, protector) = setup(&[true, false]);
    assert_eq!(
        apply_sync_restore(plan, &backend, &storage, &protector)
            .await
            .unwrap_err(),
        DocumentError::RestoreRolledBack
    );
    assert_eq!(theme(&backend), "system");
    assert_eq!(backend.0.lock().unwrap().generation.get(), 7);
    assert!(storage.value.lock().unwrap().is_none());
}

#[tokio::test]
async fn rollback_failure_leaves_only_encrypted_material_and_recovery_resumes_it() {
    let (backend, plan, storage, protector) = setup(&[true, true]);
    assert_eq!(
        apply_sync_restore(plan, &backend, &storage, &protector)
            .await
            .unwrap_err(),
        DocumentError::RecoveryRequired
    );
    let bytes = storage
        .value
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .ciphertext
        .clone();
    assert!(!bytes
        .windows(b"fixture-secret-original".len())
        .any(|part| part == b"fixture-secret-original"));
    assert!(backend.0.lock().unwrap().pending);
    let result = recover_sync_restore(context().scope, &backend, &storage, &protector)
        .await
        .unwrap();
    assert!(matches!(result, RecoveryOutcome::RolledBack(_)));
    assert_eq!(theme(&backend), "system");
}

#[tokio::test]
async fn unknown_commit_journal_write_is_resolved_by_its_durable_decision() {
    for persisted in [false, true] {
        let (backend, plan, storage, protector) = setup(&[]);
        if persisted {
            storage.fail_after.store(2, Ordering::SeqCst);
        } else {
            storage.fail_before.store(2, Ordering::SeqCst);
        }
        assert_eq!(
            apply_sync_restore(plan, &backend, &storage, &protector)
                .await
                .unwrap_err(),
            DocumentError::RecoveryRequired
        );
        let recovered = recover_sync_restore(context().scope, &backend, &storage, &protector)
            .await
            .unwrap();
        assert_eq!(
            matches!(recovered, RecoveryOutcome::Committed(_)),
            persisted
        );
        assert_eq!(theme(&backend), if persisted { "dark" } else { "system" });
    }
}

#[tokio::test]
async fn obsolete_committed_journal_never_replays_over_new_user_data() {
    let (backend, plan, storage, protector) = setup(&[]);
    storage.fail_clear.store(true, Ordering::SeqCst);
    assert!(
        apply_sync_restore(plan, &backend, &storage, &protector)
            .await
            .unwrap()
            .journal_cleanup_pending
    );
    backend
        .0
        .lock()
        .unwrap()
        .data
        .set
        .documents
        .iter_mut()
        .find(|doc| doc.id == "themeMode")
        .unwrap()
        .value = json!("light");
    let writes = backend.0.lock().unwrap().writes;
    storage.fail_clear.store(false, Ordering::SeqCst);
    recover_sync_restore(context().scope, &backend, &storage, &protector)
        .await
        .unwrap();
    assert_eq!(theme(&backend), "light");
    assert_eq!(backend.0.lock().unwrap().writes, writes);
}

#[tokio::test]
async fn wrong_scope_or_ciphertext_cannot_mutate_recovery_data() {
    let (backend, plan, storage, protector) = setup(&[true, true]);
    let _ = apply_sync_restore(plan, &backend, &storage, &protector).await;
    let before = theme(&backend);
    let writes = backend.0.lock().unwrap().writes;
    let mut wrong = context().scope;
    wrong.owner_github_id = "43".into();
    assert_eq!(
        recover_sync_restore(wrong, &backend, &storage, &protector)
            .await
            .unwrap_err(),
        DocumentError::Locked
    );
    {
        let mut stored = storage.value.lock().unwrap();
        let data = &mut stored.as_mut().unwrap().ciphertext;
        let last = data.len() - 1;
        data[last] ^= 1;
    }
    assert_eq!(
        recover_sync_restore(context().scope, &backend, &storage, &protector)
            .await
            .unwrap_err(),
        DocumentError::Locked
    );
    assert_eq!(theme(&backend), before);
    assert_eq!(backend.0.lock().unwrap().writes, writes);
}

#[tokio::test]
async fn journal_prepare_failure_never_starts_a_data_write() {
    let (backend, plan, storage, protector) = setup(&[]);
    storage.fail_before.store(1, Ordering::SeqCst);
    assert_eq!(
        apply_sync_restore(plan, &backend, &storage, &protector)
            .await
            .unwrap_err(),
        DocumentError::JournalUnavailable
    );
    assert_eq!(backend.0.lock().unwrap().writes, 0);
    assert!(matches!(
        recover_sync_restore(context().scope, &backend, &storage, &protector)
            .await
            .unwrap(),
        RecoveryOutcome::NoPending
    ));
    assert!(!backend.0.lock().unwrap().pending);
}
