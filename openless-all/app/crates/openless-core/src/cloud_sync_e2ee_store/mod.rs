//! Native repository adapter for encrypted sync.
//! Complete logical restoration is protected by an encrypted crash journal.

mod capture;
pub mod extensions;
pub mod gate;
mod journal;
mod native;
mod state;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::cloud_sync_e2ee_documents::{
    ConflictChoice, DocumentError, DocumentKind, DocumentResult, ExportedDocuments, RestoreBackend,
    RestoreContext, RestoreLease, SecretJson, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::types::{
    DocumentSet, LogicalDocument, Revision, SourceDevice,
};
use crate::local_migration_bundle::{
    decode_bundle, export_validated_bundle, BundleArtifact, BundleError, BundleExportOptions,
    BundleInspection, BundleMergePreview, BundlePassword, RestoreMode,
};
use crate::{BackendRepositories, CredentialStore};

pub use extensions::{DeviceExtensionKey, ProtectedExtensionStore};
pub use gate::{ChangeOrigin, ExclusivePermit, MutationPermit, SyncChange, SyncWriteGate};

#[derive(Clone)]
pub struct CoreSyncStore {
    inner: Arc<Inner>,
}

struct Inner {
    repositories: BackendRepositories,
    credentials: Arc<dyn CredentialStore>,
    data_dir: PathBuf,
    device: SourceDevice,
    gate: Arc<SyncWriteGate>,
    extensions: Arc<dyn ProtectedExtensionStore>,
    tasks: Arc<dyn crate::TaskSpawner>,
    restoring: AtomicBool,
    metadata: tokio::sync::Mutex<()>,
    runtime_idle: std::sync::Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    runtime_effects: std::sync::Mutex<Option<Arc<dyn crate::config::RestoreRuntimeEffects>>>,
    android_migration:
        std::sync::OnceLock<Arc<dyn crate::local_migration_bundle::AndroidMigrationState>>,
}

impl CoreSyncStore {
    pub fn new(
        repositories: BackendRepositories,
        credentials: Arc<dyn CredentialStore>,
        data_dir: PathBuf,
        device: SourceDevice,
        gate: Arc<SyncWriteGate>,
        extensions: Arc<dyn ProtectedExtensionStore>,
        tasks: Arc<dyn crate::TaskSpawner>,
    ) -> DocumentResult<Self> {
        let registered = gate::open_for_data_dir(&data_dir)?;
        if !Arc::ptr_eq(&registered, &gate) {
            return Err(DocumentError::InvalidDocument);
        }
        credentials
            .bind_sync_gate(Arc::clone(&gate))
            .map_err(|_| DocumentError::Locked)?;
        repositories
            .activity
            .bind_sync_device(&device.id)
            .map_err(|_| DocumentError::CaptureFailed)?;
        Ok(Self {
            inner: Arc::new(Inner {
                repositories,
                credentials,
                data_dir,
                device,
                gate,
                extensions,
                tasks,
                restoring: AtomicBool::new(false),
                metadata: tokio::sync::Mutex::new(()),
                runtime_idle: std::sync::Mutex::new(None),
                runtime_effects: std::sync::Mutex::new(None),
                android_migration: std::sync::OnceLock::new(),
            }),
        })
    }

    pub fn generation(&self) -> DocumentResult<Revision> {
        self.inner.gate.generation()
    }

    pub fn bind_android_migration_state(
        &self,
        state: Arc<dyn crate::local_migration_bundle::AndroidMigrationState>,
    ) -> DocumentResult<()> {
        self.inner
            .android_migration
            .set(state)
            .map_err(|_| DocumentError::InvalidDocument)
    }
    pub fn device(&self) -> SourceDevice {
        self.inner.device.clone()
    }
    pub fn custom_server_origin(&self) -> Option<String> {
        self.inner
            .repositories
            .preferences
            .get()
            .sync_custom_server_origin
    }
    pub(crate) fn set_custom_server_origin(
        &self,
        origin: Option<String>,
    ) -> Result<(), crate::BackendError> {
        self.inner
            .repositories
            .preferences
            .update(|prefs| prefs.sync_custom_server_origin = origin)
    }

    pub(crate) async fn legacy_baseline_matches(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> DocumentResult<bool> {
        let state = state::ScopeState::decode(
            &scope,
            self.inner.extensions.read_scope(scope.clone()).await?,
        )?;
        Ok(state.baseline_revision == revision && state.baseline.as_ref() == Some(&documents))
    }

    pub fn changes(&self) -> tokio::sync::watch::Receiver<SyncChange> {
        self.inner.gate.subscribe()
    }

    /// Core binds its shared dictation/insertion/agent session-state probe before startup recovery.
    pub fn bind_runtime_idle_probe(
        &self,
        probe: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> DocumentResult<()> {
        let mut slot = self
            .inner
            .runtime_idle
            .lock()
            .map_err(|_| DocumentError::RecoveryRequired)?;
        if slot.is_some() {
            return Err(DocumentError::InvalidDocument);
        }
        *slot = Some(probe);
        Ok(())
    }

    /// Install a real, absolute Host effects adapter before allowing restores.
    pub fn bind_runtime_effects(
        &self,
        effects: Arc<dyn crate::config::RestoreRuntimeEffects>,
    ) -> DocumentResult<()> {
        let mut slot = self
            .inner
            .runtime_effects
            .lock()
            .map_err(|_| DocumentError::RecoveryRequired)?;
        if slot.is_some() {
            return Err(DocumentError::InvalidDocument);
        }
        *slot = Some(effects);
        Ok(())
    }

    fn runtime_effects(&self) -> DocumentResult<Arc<dyn crate::config::RestoreRuntimeEffects>> {
        self.inner
            .runtime_effects
            .lock()
            .map_err(|_| DocumentError::RecoveryRequired)?
            .clone()
            .ok_or(DocumentError::Unsupported)
    }

    async fn apply_runtime_effects(&self, permit: &ExclusivePermit) -> DocumentResult<()> {
        if !permit.belongs_to(&self.inner.gate) {
            return Err(DocumentError::InvalidDocument);
        }
        let effects = self.runtime_effects()?;
        let target = self.inner.repositories.preferences.get();
        effects
            .apply_target(target)
            .await
            .map_err(|_| DocumentError::CaptureFailed)
    }

    /// Ordinary runtime starts must check this while holding Core's shared runtime lock.
    pub fn is_recovering(&self) -> bool {
        self.inner.restoring.load(Ordering::Acquire)
            || self.inner.gate.recovery_required().unwrap_or(true)
    }
    pub fn ensure_runtime_available(&self) -> DocumentResult<()> {
        if self.is_recovering() {
            Err(DocumentError::RecoveryRequired)
        } else {
            Ok(())
        }
    }

    /// Local-only eligibility sample. Callers recheck this stamp after credential reads.
    pub(crate) fn setup_prompt_state(
        &self,
    ) -> DocumentResult<Option<(crate::shared_types::UserPreferences, (Revision, u64))>> {
        if self.is_recovering() {
            return Ok(None);
        }
        let probe = self
            .inner
            .runtime_idle
            .lock()
            .map_err(|_| DocumentError::RecoveryRequired)?
            .clone();
        let Some(probe) = probe else {
            return Ok(None);
        };
        if !probe() {
            return Ok(None);
        }
        let stamp = match self.inner.gate.coherent_generation() {
            Ok(stamp) => stamp,
            Err(DocumentError::SourceChanged | DocumentError::RecoveryRequired) => return Ok(None),
            Err(error) => return Err(error),
        };
        let preferences = self.inner.repositories.preferences.get();
        if self.is_recovering()
            || !probe()
            || self.inner.gate.coherent_generation().ok() != Some(stamp)
        {
            return Ok(None);
        }
        Ok(Some((preferences, stamp)))
    }

    pub async fn export_scope(&self, scope: SyncScope) -> DocumentResult<ExportedDocuments> {
        self.validate_local_scope(&scope)?;
        let _metadata = self.inner.metadata.lock().await;
        self.capture_readonly(&scope).await
    }

    pub async fn restore_scope(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
    ) -> DocumentResult<ExportedDocuments> {
        self.restore_scope_with_android_revision(desired, context, None)
            .await
    }

    async fn restore_scope_with_android_revision(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
        android_revision: Option<String>,
    ) -> DocumentResult<ExportedDocuments> {
        self.validate_local_scope(&context.scope)?;
        let scope = context.scope.clone();
        let desired = native::canonicalize_native_documents(desired)?;
        let plan = crate::cloud_sync_e2ee_documents::prepare_sync_restore(desired, context)?
            .with_android_revision(android_revision);
        let store = self.clone();
        // Detach only the owned, journalled operation: dropping an IPC waiter cannot cancel it.
        self.spawn_owned(async move {
            let _metadata = store.inner.metadata.lock().await;
            let _runtime = store.begin_runtime_restore()?;
            store.runtime_effects()?;
            let protector = store.inner.extensions.journal_protector().await?;
            let journal = store.journal();
            crate::cloud_sync_e2ee_documents::apply_sync_restore(
                plan,
                &store,
                &journal,
                protector.as_ref(),
            )
            .await?;
            let permit = store.inner.gate.try_exclusive()?;
            store.capture_locked(&scope, &permit).await
        })
        .await
    }

    pub async fn record_baseline(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> DocumentResult<()> {
        self.validate_local_scope(&scope)?;
        let store = self.clone();
        self.spawn_owned(async move {
            let _metadata = store.inner.metadata.lock().await;
            store.baseline_locked(&scope, documents, revision).await
        })
        .await
    }

    pub async fn recover_registered(&self) -> DocumentResult<()> {
        let scopes = self.inner.gate.registered_recovery_scopes()?;
        if scopes.is_empty() && !self.inner.gate.recovery_required()? {
            return Ok(());
        }
        let store = self.clone();
        self.spawn_owned(async move {
            use crate::cloud_sync_e2ee_documents::JournalStore;
            let _metadata = store.inner.metadata.lock().await;
            let _runtime = store.begin_runtime_restore()?;
            // Recovery needs the same real Host adapter before reading protected journals
            // or prompting for keys; unsupported recovery must leave the pending bytes intact.
            store.runtime_effects()?;
            let journal = store.journal();
            for scope in scopes {
                if journal.load(scope.clone()).await?.is_none() {
                    let permit = store.inner.gate.try_exclusive_recovery()?;
                    let scope_id = state::scope_id(&scope)?;
                    permit.recover_without_journal(&scope_id)?;
                    permit.forget_completed_scope_without_journal(&scope_id)?;
                } else {
                    let protector = store.inner.extensions.journal_protector().await?;
                    crate::cloud_sync_e2ee_documents::recover_sync_restore(
                        scope,
                        &store,
                        &journal,
                        protector.as_ref(),
                    )
                    .await?;
                }
            }
            if store.inner.gate.recovery_required()? {
                let permit = store.inner.gate.try_exclusive_recovery()?;
                store.reconcile_native(&permit).await?;
                store.apply_runtime_effects(&permit).await?;
                permit.accept_reconciled_capture()?;
            }
            Ok(())
        })
        .await
    }

    /// Runs an owned operation on the host task spawner and awaits its outcome.
    /// Dropping the awaiting caller cannot cancel the operation: the spawned
    /// task owns the sender side of the channel.
    async fn spawn_owned<T, F>(&self, operation: F) -> DocumentResult<T>
    where
        T: Send + 'static,
        F: std::future::Future<Output = DocumentResult<T>> + Send + 'static,
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.inner.tasks.spawn(Box::pin(async move {
            let _ = sender.send(operation.await);
        }));
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(DocumentError::RecoveryRequired),
        }
    }

    fn validate_local_scope(&self, scope: &SyncScope) -> DocumentResult<()> {
        crate::cloud_sync_e2ee_documents::validate_scope(scope)?;
        if scope.device_id != self.inner.device.id {
            return Err(DocumentError::InvalidDocument);
        }
        Ok(())
    }
    fn journal(&self) -> journal::FileJournalStore {
        journal::FileJournalStore::new(
            self.inner.data_dir.join("encrypted-sync"),
            Arc::clone(&self.inner.gate),
        )
    }
    fn begin_runtime_restore(&self) -> DocumentResult<RuntimeRestore> {
        self.inner
            .restoring
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| DocumentError::SourceChanged)?;
        let guard = RuntimeRestore(self.clone());
        let probe = self
            .inner
            .runtime_idle
            .lock()
            .map_err(|_| DocumentError::RecoveryRequired)?
            .clone()
            .ok_or(DocumentError::Unsupported)?;
        if !probe() {
            return Err(DocumentError::RuntimeBusy);
        }
        Ok(guard)
    }

    async fn reconcile_native(&self, permit: &ExclusivePermit) -> DocumentResult<()> {
        // No UI mirror or sync key is required to reconcile an interrupted ordinary save.
        let repo = &self.inner.repositories;
        repo.preferences
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repo.vocabulary
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repo.correction_rules
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        repo.history
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let styles = repo
            .style_packs
            .sync_snapshot(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        for style in styles {
            if let Some(path) = style
                .pack
                .expose()
                .get("iconPath")
                .and_then(serde_json::Value::as_str)
            {
                crate::persistence::ensure_durable_file(std::path::Path::new(path))
                    .map_err(|_| DocumentError::RecoveryRequired)?;
            }
        }
        for name in [
            "preferences.json",
            "history.json",
            "activity.json",
            "dictionary.json",
            "correction-rules.json",
            "style-packs.json",
            "vocab-presets.json",
        ] {
            let path = self.inner.data_dir.join(name);
            if path.exists() {
                crate::persistence::ensure_durable_file(&path)
                    .map_err(|_| DocumentError::RecoveryRequired)?;
            }
        }
        repo.activity
            .sync_records(permit)
            .map_err(|_| DocumentError::CaptureFailed)?;
        crate::vocabulary::list_vocab_presets(&self.inner.data_dir)
            .map_err(|_| DocumentError::CaptureFailed)?;
        let credentials = self
            .inner
            .credentials
            .export_sync_credentials(permit)
            .await
            .map_err(|_| DocumentError::CaptureFailed)?;
        crate::cloud_sync_e2ee_documents::validate_credential_set(
            &credentials.channels,
            &credentials.credentials,
        )
    }

    pub async fn set_ui_preferences(&self, value: SecretJson) -> DocumentResult<()> {
        self.set_ui_preferences_inner(value, None).await
    }

    pub async fn set_ui_preferences_checked(
        &self,
        value: SecretJson,
        expected_revision: Option<String>,
    ) -> DocumentResult<()> {
        self.set_ui_preferences_inner(value, Some(expected_revision))
            .await
    }

    async fn set_ui_preferences_inner(
        &self,
        value: SecretJson,
        expected_revision: Option<Option<String>>,
    ) -> DocumentResult<()> {
        let map = value
            .expose()
            .as_object()
            .ok_or(DocumentError::InvalidDocument)?;
        if map.len() != 2 || !map.contains_key("locale") || !map.contains_key("fontScale") {
            return Err(DocumentError::InvalidDocument);
        }
        let locale = map["locale"]
            .as_str()
            .ok_or(DocumentError::InvalidDocument)?;
        let font = map["fontScale"]
            .as_str()
            .ok_or(DocumentError::InvalidDocument)?;
        if ![
            "system", "zh-CN", "zh-TW", "en", "ja", "ko", "es", "fr", "de",
        ]
        .contains(&locale)
            || !["small", "medium", "large"].contains(&font)
        {
            return Err(DocumentError::InvalidDocument);
        }
        let store = self.clone();
        self.spawn_owned(async move {
            let _metadata = store.inner.metadata.lock().await;
            let permit = store.inner.gate.begin_mutation()?;
            if let Some(expected) = expected_revision {
                let current = match store.inner.extensions.read_ui_revision().await {
                    Ok(value) => value,
                    Err(error) => {
                        permit.abort_unmodified()?;
                        return Err(error);
                    }
                };
                if current != expected {
                    permit.abort_unmodified()?;
                    return Err(DocumentError::StalePreview);
                }
            }
            let previous = match store
                .inner
                .extensions
                .read_device(DeviceExtensionKey::UiPreferences)
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    permit.abort_unmodified()?;
                    return Err(error);
                }
            };
            if previous.as_ref() == Some(&value) {
                return permit.abort_unmodified();
            }
            store
                .inner
                .extensions
                .write_device(DeviceExtensionKey::UiPreferences, value)
                .await?;
            permit.commit(ChangeOrigin::User).map(|_| ())
        })
        .await
    }
}

struct RuntimeRestore(CoreSyncStore);
impl Drop for RuntimeRestore {
    fn drop(&mut self) {
        self.0.inner.restoring.store(false, Ordering::Release);
    }
}

struct NativeLease {
    store: CoreSyncStore,
    scope: SyncScope,
    scope_id: String,
    permit: ExclusivePermit,
    android: Option<Box<dyn crate::local_migration_bundle::AndroidMigrationLease>>,
}
impl Drop for NativeLease {
    fn drop(&mut self) {
        if let Some(android) = &mut self.android {
            let _ = android.finish(self.store.inner.gate.recovery_required().unwrap_or(true));
        }
    }
}
impl RestoreBackend for CoreSyncStore {
    fn acquire(
        &self,
        scope: SyncScope,
        expected_generation: Option<Revision>,
    ) -> BoxFuture<'_, DocumentResult<Box<dyn RestoreLease>>> {
        Box::pin(async move {
            self.validate_local_scope(&scope)?;
            let permit = if expected_generation.is_some() {
                self.inner.gate.try_exclusive()?
            } else {
                self.inner.gate.try_exclusive_recovery()?
            };
            if expected_generation
                .is_some_and(|expected| permit.generation().ok() != Some(expected))
            {
                return Err(DocumentError::StalePreview);
            }
            let scope_id = state::scope_id(&scope)?;
            Ok(Box::new(NativeLease {
                store: self.clone(),
                scope,
                scope_id,
                permit,
                android: None,
            }) as Box<dyn RestoreLease>)
        })
    }
}

impl RestoreLease for NativeLease {
    fn finish_local_state(&mut self) -> DocumentResult<()> {
        if let Some(android) = &mut self.android {
            android
                .finish(false)
                .map_err(|_| DocumentError::RecoveryRequired)?;
            self.android = None;
        }
        Ok(())
    }
    fn prepare_target(
        &mut self,
        target: &ValidatedSyncDocuments,
        revision: Option<&str>,
    ) -> DocumentResult<()> {
        if self.scope != self.store.local_bundle_scope() {
            return if revision.is_none() {
                Ok(())
            } else {
                Err(DocumentError::Unsupported)
            };
        }
        if let Some(value) = android_settings(target) {
            crate::local_migration_bundle::validate_portable_android_preferences(value)
                .map_err(|_| DocumentError::InvalidDocument)?;
            let state = self
                .store
                .inner
                .android_migration
                .get()
                .ok_or(DocumentError::Unsupported)?;
            self.android = Some(state.acquire(revision)?);
        } else if revision.is_some() {
            return Err(DocumentError::InvalidDocument);
        }
        Ok(())
    }
    fn capture(
        &mut self,
    ) -> BoxFuture<'_, DocumentResult<crate::cloud_sync_e2ee_documents::ExportSnapshot>> {
        Box::pin(async move {
            let state = state::ScopeState::decode(
                &self.scope,
                self.store
                    .inner
                    .extensions
                    .read_scope(self.scope.clone())
                    .await?,
            )?;
            let mut snapshot = self
                .store
                .capture_native(&self.scope, &self.permit, &state)
                .await?;
            if let Some(android) = &self.android {
                let value = android.capture()?;
                snapshot.retained_documents.retain(|doc| {
                    !(doc.kind == DocumentKind::UiPreferences
                        && doc.id == crate::local_migration_bundle::ANDROID_SETTINGS_ID)
                });
                snapshot
                    .retained_documents
                    .push(android_document(value.into_value()));
            }
            Ok(snapshot)
        })
    }
    fn capture_rollback_state(&mut self) -> BoxFuture<'_, DocumentResult<SecretJson>> {
        Box::pin(async move { self.store.capture_local_rollback_state(&self.permit) })
    }
    fn restore_rollback_state(&mut self, value: &SecretJson) -> BoxFuture<'_, DocumentResult<()>> {
        let value = value.clone();
        Box::pin(async move {
            self.store
                .restore_local_rollback_state(&value, &self.permit)?;
            // Excluded local grants/mirrors are restored after the logical rollback.
            // The Host must converge to that final target before the fence is released.
            self.store.apply_runtime_effects(&self.permit).await
        })
    }
    fn preflight(
        &mut self,
        documents: &ValidatedSyncDocuments,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        let documents = documents.clone();
        Box::pin(async move {
            self.store.runtime_effects()?;
            self.store
                .validate_native(&self.scope, &documents, &self.permit)
                .await
        })
    }
    fn replace(&mut self, documents: ValidatedSyncDocuments) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            self.store.runtime_effects()?;
            self.store
                .replace_native(&self.scope, &documents, &self.permit)
                .await?;
            if let (Some(android), Some(value)) = (&mut self.android, android_settings(&documents))
            {
                android.replace(&SecretJson::new(value.clone()))?;
            }
            Ok(())
        })
    }
    fn reload(&mut self) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move { self.store.apply_runtime_effects(&self.permit).await })
    }
    fn mark_recovery_pending(&mut self, operation_id: &str) -> DocumentResult<()> {
        self.permit
            .mark_restore_pending(operation_id, &self.scope_id, &self.scope)
    }
    fn mark_journal_ready(&mut self, operation_id: &str) -> DocumentResult<()> {
        self.permit.mark_journal_ready(operation_id, &self.scope_id)
    }
    fn recover_without_journal(&mut self) -> DocumentResult<()> {
        self.permit.recover_without_journal(&self.scope_id)
    }
    fn completed_restore(
        &self,
        operation_id: &str,
    ) -> DocumentResult<Option<crate::cloud_sync_e2ee_documents::RestoreReceipt>> {
        self.permit
            .completed_restore(operation_id, &self.scope_id)
            .map(|value| value.map(receipt))
    }
    fn finish_restore(
        &mut self,
        operation_id: &str,
        committed: bool,
    ) -> DocumentResult<crate::cloud_sync_e2ee_documents::RestoreReceipt> {
        self.permit
            .finish_restore(operation_id, &self.scope_id, committed)
            .map(receipt)
    }
}
fn receipt(value: gate::GateRestoreReceipt) -> crate::cloud_sync_e2ee_documents::RestoreReceipt {
    crate::cloud_sync_e2ee_documents::RestoreReceipt {
        operation_id: value.operation_id,
        generation: value.generation,
        committed: value.committed,
        journal_cleanup_pending: false,
    }
}

fn android_settings(documents: &ValidatedSyncDocuments) -> Option<&serde_json::Value> {
    documents
        .documents()
        .documents
        .iter()
        .find(|doc| {
            doc.kind == DocumentKind::UiPreferences
                && doc.id == crate::local_migration_bundle::ANDROID_SETTINGS_ID
        })
        .map(|doc| &doc.value)
}

fn android_document(value: serde_json::Value) -> LogicalDocument {
    LogicalDocument {
        id: crate::local_migration_bundle::ANDROID_SETTINGS_ID.into(),
        kind: DocumentKind::UiPreferences,
        schema_version: crate::cloud_sync_e2ee_documents::DOCUMENT_SCHEMA_VERSION,
        value,
    }
}

fn local_bundle_diff(
    current: &ValidatedSyncDocuments,
    incoming: &ValidatedSyncDocuments,
    mode: RestoreMode,
) -> DocumentResult<crate::cloud_sync_e2ee_documents::MergePreview> {
    match mode {
        RestoreMode::Merge => {
            crate::cloud_sync_e2ee_documents::diff_sync_documents(None, current, incoming)
        }
        RestoreMode::Replace => {
            crate::cloud_sync_e2ee_documents::diff_local_replace_documents(current, incoming)
        }
    }
}

fn merge_android_learning(
    current: &ValidatedSyncDocuments,
    desired: &mut ValidatedSyncDocuments,
) -> Result<(), BundleError> {
    let Some(local) = android_settings(current) else {
        return Ok(());
    };
    let Some(target) = desired.set.documents.iter_mut().find(|doc| {
        doc.kind == DocumentKind::UiPreferences
            && doc.id == crate::local_migration_bundle::ANDROID_SETTINGS_ID
    }) else {
        return Ok(());
    };
    for key in ["strokeFrequency", "pinyinLearnedPhrases"] {
        let incoming = target.value[key]
            .as_object_mut()
            .ok_or(BundleError::InvalidSchema)?;
        for (id, value) in local[key].as_object().ok_or(BundleError::InvalidSchema)? {
            if incoming.len() >= 20_000 {
                break;
            }
            incoming.entry(id.clone()).or_insert_with(|| value.clone());
        }
    }
    crate::local_migration_bundle::validate_portable_android_preferences(&target.value)
}

impl CoreSyncStore {
    /// Export the complete local state without binding the user file to a cloud account.
    pub async fn export_local_bundle(
        &self,
        options: BundleExportOptions,
    ) -> Result<BundleArtifact, BundleError> {
        let (captured, _) = self.capture_local_bundle().await?;
        let options = BundleExportOptions {
            portable_android_preferences: android_settings(&captured.documents)
                .cloned()
                .or(options.portable_android_preferences),
            ..options
        };
        export_validated_bundle(&captured.documents, options)
    }

    /// Inspecting a bundle is deliberately independent of the native stores.
    pub fn inspect_local_bundle(bytes: &[u8]) -> Result<BundleInspection, BundleError> {
        crate::local_migration_bundle::inspect_bundle(bytes)
    }

    async fn capture_local_bundle(
        &self,
    ) -> Result<(ExportedDocuments, Option<String>), BundleError> {
        let platform = self.inner.android_migration.get();
        let android = platform.map(|state| state.capture()).transpose()?;
        let mut captured = self.export_scope(self.local_bundle_scope()).await?;
        if let Some(snapshot) = &android {
            crate::local_migration_bundle::validate_portable_android_preferences(
                snapshot.value.expose(),
            )?;
            captured.documents.set.documents.retain(|doc| {
                !(doc.kind == DocumentKind::UiPreferences
                    && doc.id == crate::local_migration_bundle::ANDROID_SETTINGS_ID)
            });
            captured
                .documents
                .set
                .documents
                .push(android_document(snapshot.value.expose().clone()));
            captured.documents = crate::cloud_sync_e2ee_documents::revalidate_sync_documents(
                captured.documents.set.clone(),
                captured.documents.observed_revision(),
            )?;
            if platform.expect("captured platform").capture()?.revision != snapshot.revision {
                return Err(BundleError::StalePreview);
            }
        }
        Ok((captured, android.map(|snapshot| snapshot.revision)))
    }

    pub async fn preview_local_bundle(
        &self,
        bytes: &[u8],
        password: Option<BundlePassword>,
        mode: RestoreMode,
    ) -> Result<BundleMergePreview, BundleError> {
        let (captured, observed_android_revision) = self.capture_local_bundle().await?;
        let generation = captured.generation;
        let current = captured.documents;
        let decoded = decode_bundle(bytes, password.as_ref(), generation)?;
        let inspection = decoded.inspection.clone();
        let incoming = self.prepare_bundle_documents(&current, decoded)?;
        let removed_documents = if mode == RestoreMode::Replace {
            current
                .documents()
                .documents
                .iter()
                // The native restore materializes missing preferences and built-in styles.
                .filter(|doc| {
                    !matches!(
                        doc.kind,
                        DocumentKind::Preferences | DocumentKind::UiPreferences
                    ) && !(doc.kind == DocumentKind::StylePacks
                        && doc.value["pack"]["kind"] == "builtin")
                })
                .filter(|doc| {
                    !incoming
                        .documents()
                        .documents
                        .iter()
                        .any(|other| other.kind == doc.kind && other.id == doc.id)
                })
                .map(|doc| crate::cloud_sync_e2ee_documents::DocumentKey {
                    kind: doc.kind,
                    id: doc.id.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let preview = local_bundle_diff(&current, &incoming, mode)?;
        Ok(BundleMergePreview {
            inspection,
            observed_generation: generation,
            observed_android_revision,
            removed_documents,
            conflicts: preview.conflicts().to_vec(),
        })
    }

    /// Apply a previously inspected bundle under the same crash-safe gate used by sync.
    /// The synthetic scope is internal state only; it never appears in the user bundle.
    pub async fn import_local_bundle(
        &self,
        bytes: &[u8],
        password: Option<BundlePassword>,
        mode: RestoreMode,
        choices: &[ConflictChoice],
        expected_generation: Option<Revision>,
        expected_android_revision: Option<String>,
    ) -> Result<BundleInspection, BundleError> {
        let scope = self.local_bundle_scope();
        let (captured, android_revision) = self.capture_local_bundle().await?;
        if android_revision != expected_android_revision {
            return Err(BundleError::StalePreview);
        }
        let current = captured.documents.clone();
        let generation = captured.generation;
        if expected_generation.is_some_and(|expected| expected != generation) {
            return Err(BundleError::StalePreview);
        }
        let decoded = decode_bundle(bytes, password.as_ref(), generation)?;
        let inspection = decoded.inspection.clone();
        let bundle_documents = self.prepare_bundle_documents(&current, decoded)?;
        let mut desired = local_bundle_diff(&current, &bundle_documents, mode)?.resolve(choices)?;
        if mode == RestoreMode::Merge {
            merge_android_learning(&current, &mut desired)?;
        }
        let portable_android_preferences = android_settings(&current).cloned();
        let backup_options = if inspection.credentials_present {
            BundleExportOptions {
                mode: crate::local_migration_bundle::BundleMode::PersonalMigration,
                credential_policy:
                    crate::local_migration_bundle::CredentialPolicy::PasswordEncrypted,
                password: password.clone(),
                portable_android_preferences,
                ..BundleExportOptions::default()
            }
        } else {
            BundleExportOptions {
                portable_android_preferences,
                ..BundleExportOptions::default()
            }
        };
        let backup = export_validated_bundle(&captured.documents, backup_options)?;
        self.write_local_bundle_backup(&backup)?;
        let context = RestoreContext {
            scope,
            operation_id: uuid::Uuid::new_v4().to_string(),
            observed_revision: current.observed_revision(),
            local_generation: generation,
            target_device: self.inner.device.clone(),
        };
        self.restore_scope_with_android_revision(desired, context, android_revision)
            .await
            .map_err(BundleError::from)?;
        Ok(inspection)
    }

    fn prepare_bundle_documents(
        &self,
        current: &ValidatedSyncDocuments,
        mut decoded: crate::local_migration_bundle::DecodedBundle,
    ) -> Result<ValidatedSyncDocuments, BundleError> {
        use crate::cloud_sync_e2ee_documents::DocumentKind;

        decoded
            .documents
            .set
            .documents
            .retain(|doc| !matches!(doc.kind, DocumentKind::History | DocumentKind::Activity));
        decoded
            .documents
            .set
            .tombstones
            .retain(|doc| !matches!(doc.kind, DocumentKind::History | DocumentKind::Activity));
        decoded.documents.set.documents.extend(
            current
                .documents()
                .documents
                .iter()
                .filter(|doc| matches!(doc.kind, DocumentKind::History | DocumentKind::Activity))
                .cloned(),
        );
        decoded.documents.set.tombstones.extend(
            current
                .documents()
                .tombstones
                .iter()
                .filter(|doc| matches!(doc.kind, DocumentKind::History | DocumentKind::Activity))
                .cloned(),
        );
        if android_settings(&decoded.documents).is_none() {
            if let Some(value) = android_settings(current) {
                decoded
                    .documents
                    .set
                    .documents
                    .push(android_document(value.clone()));
            }
        }
        if !matches!(
            decoded.inspection.mode,
            crate::local_migration_bundle::BundleMode::ShareRedacted
        ) && decoded.inspection.credentials_present
        {
            let mut set = decoded.documents.documents().clone();
            set.source_device = current.documents().source_device.clone();
            set.tombstones
                .retain(|tombstone| tombstone.kind != DocumentKind::DeviceProfile);
            set.documents
                .retain(|document| document.kind != DocumentKind::DeviceProfile);
            set.documents.extend(
                current
                    .documents()
                    .documents
                    .iter()
                    .filter(|document| document.kind == DocumentKind::DeviceProfile)
                    .cloned(),
            );
            return crate::cloud_sync_e2ee_documents::revalidate_sync_documents(
                set,
                current.observed_revision(),
            )
            .map_err(BundleError::from);
        }

        use serde_json::json;
        use std::collections::{BTreeMap, BTreeSet};

        let mut set = decoded.documents.documents().clone();
        set.source_device = current.documents().source_device.clone();
        set.tombstones.retain(|tombstone| {
            !matches!(
                tombstone.kind,
                DocumentKind::ProviderCredentials | DocumentKind::DeviceProfile
            )
        });
        let imported_channels: BTreeSet<String> = set
            .documents
            .iter()
            .filter(|document| document.kind == DocumentKind::Channels)
            .map(|document| document.id.clone())
            .collect();
        let current_channels: BTreeMap<String, LogicalDocument> = current
            .documents()
            .documents
            .iter()
            .filter(|document| document.kind == DocumentKind::Channels)
            .map(|document| (document.id.clone(), document.clone()))
            .collect();
        let imported_active_namespaces: BTreeSet<String> = set
            .documents
            .iter()
            .filter(|document| {
                document.kind == DocumentKind::Channels
                    && document
                        .value
                        .get("active")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
            })
            .filter_map(|document| {
                document
                    .value
                    .get("namespace")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        let current_credentials: BTreeMap<String, LogicalDocument> = current
            .documents()
            .documents
            .iter()
            .filter(|document| document.kind == DocumentKind::ProviderCredentials)
            .map(|document| (document.id.clone(), document.clone()))
            .collect();
        let mut additions = Vec::new();
        set.tombstones.retain(|tombstone| {
            !(tombstone.kind == DocumentKind::Channels
                && current_channels.contains_key(&tombstone.id)
                && !imported_channels.contains(&tombstone.id))
        });

        for channel in set
            .documents
            .iter_mut()
            .filter(|document| document.kind == DocumentKind::Channels)
        {
            let existing_channel = current_channels.get(&channel.id);
            let compatible_channel = existing_channel.is_some_and(|current| {
                current.value.get("namespace") == channel.value.get("namespace")
                    && current.value.get("providerType") == channel.value.get("providerType")
            });
            if let Some(credentials) = current_credentials.get(&channel.id) {
                if compatible_channel {
                    let record: crate::cloud_sync_e2ee_documents::ProviderCredentialRecord =
                        serde_json::from_value(credentials.value.clone())
                            .map_err(|_| BundleError::InvalidSchema)?;
                    let has_secret = record.accounts.keys().any(|account| {
                        !crate::cloud_sync_e2ee_documents::registry::public_credential_account(
                            record.namespace,
                            account,
                        )
                    });
                    if has_secret {
                        // Public configuration from a share is untrusted when it is
                        // paired with a retained secret. Keep the existing provider
                        // endpoint/model instead of sending that secret elsewhere.
                        channel
                            .value
                            .as_object_mut()
                            .ok_or(BundleError::InvalidSchema)?
                            .remove("configuration");
                    }
                    let mut credential = credentials.clone();
                    let credential_object = credential
                        .value
                        .as_object_mut()
                        .ok_or(BundleError::InvalidSchema)?;
                    let accounts = credential_object
                        .get_mut("accounts")
                        .and_then(serde_json::Value::as_object_mut)
                        .ok_or(BundleError::InvalidSchema)?;
                    if !has_secret {
                        accounts.retain(|account, _| {
                            !crate::cloud_sync_e2ee_documents::registry::public_credential_account(
                                record.namespace,
                                account,
                            )
                        });
                        if let Some(configuration) = channel
                            .value
                            .get("configuration")
                            .and_then(serde_json::Value::as_object)
                        {
                            for (account, value) in configuration {
                                accounts.insert(account.clone(), value.clone());
                            }
                        }
                    }
                    additions.push(credential);
                    continue;
                }
                // A changed provider identity must never inherit a receiver secret.
                channel
                    .value
                    .as_object_mut()
                    .ok_or(BundleError::InvalidSchema)?
                    .insert("enabled".into(), serde_json::Value::Bool(false));
                channel
                    .value
                    .as_object_mut()
                    .ok_or(BundleError::InvalidSchema)?
                    .insert("active".into(), serde_json::Value::Bool(false));
            }
            let channel_value = channel
                .value
                .as_object_mut()
                .ok_or(BundleError::InvalidSchema)?;
            let namespace = channel_value
                .get("namespace")
                .cloned()
                .ok_or(BundleError::InvalidSchema)?;
            let channel_id = channel_value
                .get("id")
                .cloned()
                .ok_or(BundleError::InvalidSchema)?;
            additions.push(LogicalDocument {
                id: channel.id.clone(),
                kind: DocumentKind::ProviderCredentials,
                schema_version: channel.schema_version,
                value: json!({
                    "channelId": channel_id,
                    "namespace": namespace,
                    "accounts": channel_value.get("configuration").cloned().unwrap_or_else(|| json!({})),
                }),
            });
        }

        // Replace imports must not delete receiver-only channels or their
        // credentials merely because a redacted share cannot carry them.
        for (id, mut channel) in current_channels {
            if !imported_channels.contains(&id) {
                if imported_active_namespaces.contains(
                    channel
                        .value
                        .get("namespace")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                ) {
                    channel
                        .value
                        .as_object_mut()
                        .ok_or(BundleError::InvalidSchema)?
                        .insert("active".into(), serde_json::Value::Bool(false));
                }
                if let Some(credentials) = current_credentials.get(&id) {
                    set.documents.push(channel);
                    set.documents.push(credentials.clone());
                }
            }
        }
        additions.extend(
            current
                .documents()
                .documents
                .iter()
                .filter(|document| document.kind == DocumentKind::DeviceProfile)
                .cloned(),
        );
        set.documents.extend(additions);
        crate::cloud_sync_e2ee_documents::revalidate_sync_documents(
            set,
            current.observed_revision(),
        )
        .map_err(BundleError::from)
    }

    fn local_bundle_scope(&self) -> SyncScope {
        SyncScope {
            service_origin: "https://local.openless.invalid".into(),
            owner_github_id: "1".into(),
            vault_id: "00000000-0000-4000-8000-000000000001".into(),
            key_id: "00000000-0000-4000-8000-000000000002".into(),
            device_id: self.inner.device.id.clone(),
        }
    }

    fn write_local_bundle_backup(&self, artifact: &BundleArtifact) -> Result<(), BundleError> {
        let directory = self.inner.data_dir.join("local-migration-backups");
        std::fs::create_dir_all(&directory).map_err(|_| BundleError::BackupFailed)?;
        let id = uuid::Uuid::new_v4().to_string();
        let temporary = directory.join(format!("{id}.tmp"));
        let target = directory.join(format!("{id}.omb"));
        let result = (|| -> Result<(), BundleError> {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|_| BundleError::BackupFailed)?;
            std::io::Write::write_all(&mut file, &artifact.bytes)
                .map_err(|_| BundleError::BackupFailed)?;
            file.sync_all().map_err(|_| BundleError::BackupFailed)?;
            std::fs::rename(&temporary, &target).map_err(|_| BundleError::BackupFailed)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests;
