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
    DocumentError, DocumentResult, ExportedDocuments, RestoreBackend, RestoreContext, RestoreLease,
    SecretJson, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::types::{DocumentSet, Revision, SourceDevice};
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
    restoring: AtomicBool,
    metadata: tokio::sync::Mutex<()>,
    runtime_idle: std::sync::Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    runtime_effects: std::sync::Mutex<Option<Arc<dyn crate::config::RestoreRuntimeEffects>>>,
}

impl CoreSyncStore {
    pub fn new(
        repositories: BackendRepositories,
        credentials: Arc<dyn CredentialStore>,
        data_dir: PathBuf,
        device: SourceDevice,
        gate: Arc<SyncWriteGate>,
        extensions: Arc<dyn ProtectedExtensionStore>,
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
                restoring: AtomicBool::new(false),
                metadata: tokio::sync::Mutex::new(()),
                runtime_idle: std::sync::Mutex::new(None),
                runtime_effects: std::sync::Mutex::new(None),
            }),
        })
    }

    pub fn generation(&self) -> DocumentResult<Revision> {
        self.inner.gate.generation()
    }
    pub fn device(&self) -> SourceDevice {
        self.inner.device.clone()
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
        self.validate_local_scope(&context.scope)?;
        let scope = context.scope.clone();
        let desired = native::canonicalize_native_documents(desired)?;
        let plan = crate::cloud_sync_e2ee_documents::prepare_sync_restore(desired, context)?;
        let store = self.clone();
        // Detach only the owned, journalled operation: dropping an IPC waiter cannot cancel it.
        tokio::spawn(async move {
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
        .map_err(|_| DocumentError::RecoveryRequired)?
    }

    pub async fn record_baseline(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> DocumentResult<()> {
        self.validate_local_scope(&scope)?;
        let store = self.clone();
        tokio::spawn(async move {
            let _metadata = store.inner.metadata.lock().await;
            store.baseline_locked(&scope, documents, revision).await
        })
        .await
        .map_err(|_| DocumentError::RecoveryRequired)?
    }

    pub async fn recover_registered(&self) -> DocumentResult<()> {
        let scopes = self.inner.gate.registered_recovery_scopes()?;
        if scopes.is_empty() && !self.inner.gate.recovery_required()? {
            return Ok(());
        }
        let store = self.clone();
        tokio::spawn(async move {
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
        .map_err(|_| DocumentError::RecoveryRequired)?
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
        tokio::spawn(async move {
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
        .map_err(|_| DocumentError::RecoveryRequired)?
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
            }) as Box<dyn RestoreLease>)
        })
    }
}

impl RestoreLease for NativeLease {
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
            self.store
                .capture_native(&self.scope, &self.permit, &state)
                .await
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
                .await
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

#[cfg(test)]
mod tests;
