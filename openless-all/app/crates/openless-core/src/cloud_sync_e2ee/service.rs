#[path = "setup_prompt.rs"]
mod setup_prompt;

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::cloud_sync_e2ee_documents::{
    self as documents, ExportedDocuments, RestoreContext, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::{
    crypto::{self, DerivedKey, EncryptContext, NormalizedPassword},
    transport::{Metadata, MetadataResult, OperationStatus, ProxyPolicy, SyncSession, Transport},
    types::*,
};
use crate::events::{BackendEventKind, BackendEventPublisher};
use crate::marketplace::MarketplaceService;
use crate::{BackendError, SecretValue};

use super::{
    document_error,
    dto::*,
    error,
    local::{ClientSettings, LocalStorage},
    protocol_error, SyncResult,
};

/// The repository adapter performs the protected, exclusive restore transaction
/// and verifies actual read-back before returning. It never runs UI callbacks.
pub(crate) trait SyncServiceData: Send + Sync {
    fn export(&self, scope: SyncScope) -> BoxFuture<'_, SyncResult<ExportedDocuments>>;
    fn restore(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
    ) -> BoxFuture<'_, SyncResult<ExportedDocuments>>;
    fn recover(&self) -> BoxFuture<'_, SyncResult<()>>;
    fn baseline(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> BoxFuture<'_, SyncResult<()>>;
    fn generation(&self) -> SyncResult<Revision>;
    fn device(&self) -> SourceDevice;
    fn changes(
        &self,
    ) -> tokio::sync::watch::Receiver<crate::cloud_sync_e2ee_store::gate::SyncChange>;
}

pub(crate) struct SyncServiceConfig {
    pub origin: String,
    pub github_client_id: String,
}

struct Connection {
    transport: Transport,
    session: SyncSession,
    github_token: SecretValue,
    expires_at: Instant,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Baseline {
    revision: Revision,
    vault_id: UuidV4,
    documents: DocumentSet,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    record: String,
    operation_id: String,
    vault_id: String,
    key_id: Option<String>,
    local_generation: Revision,
    deleted: bool,
    remember_key: bool,
    key_preference_epoch: u64,
    base_revision: Revision,
    old_key_id: Option<String>,
}

struct PreviewState {
    public: RestorePreview,
    remote: ValidatedSyncDocuments,
    local: ExportedDocuments,
    baseline: Option<ValidatedSyncDocuments>,
    vault_id: String,
    key_id: String,
}

struct Runtime {
    initialized: bool,
    settings: ClientSettings,
    connection: Option<Connection>,
    metadata: Option<Metadata>,
    snapshot: Option<SnapshotUpload>,
    key: Option<Arc<DerivedKey>>,
    baseline: Option<Baseline>,
    preview: Option<PreviewState>,
    retry_at: Option<Instant>,
}

struct Shared {
    config: SyncServiceConfig,
    marketplace: Arc<MarketplaceService>,
    data: Arc<dyn SyncServiceData>,
    local: LocalStorage,
    events: BackendEventPublisher,
    runtime: tokio::sync::Mutex<Runtime>,
    status: Mutex<EncryptedSyncStatus>,
    cancelled: Arc<AtomicBool>,
    auto_started: AtomicBool,
    shutdown: AtomicBool,
    auto_suspended: AtomicBool,
    sequence: AtomicU64,
    wake: tokio::sync::Notify,
}

#[derive(Clone)]
pub(crate) struct EncryptedSyncService(Arc<Shared>);

impl EncryptedSyncService {
    pub(crate) fn new(
        config: SyncServiceConfig,
        marketplace: Arc<MarketplaceService>,
        local: LocalStorage,
        data: Arc<dyn SyncServiceData>,
        events: BackendEventPublisher,
    ) -> Self {
        let status = EncryptedSyncStatus::initial(config.origin.clone());
        Self(Arc::new(Shared {
            config,
            marketplace,
            data,
            local,
            events,
            runtime: tokio::sync::Mutex::new(Runtime {
                initialized: false,
                settings: ClientSettings::default(),
                connection: None,
                metadata: None,
                snapshot: None,
                key: None,
                baseline: None,
                preview: None,
                retry_at: None,
            }),
            status: Mutex::new(status),
            cancelled: Arc::new(AtomicBool::new(false)),
            auto_started: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            auto_suspended: AtomicBool::new(false),
            sequence: AtomicU64::new(0),
            wake: tokio::sync::Notify::new(),
        }))
    }

    pub(crate) fn status(&self) -> EncryptedSyncStatus {
        let mut value = self
            .0
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        value.sequence = self.0.sequence.load(Ordering::Acquire).to_string();
        match self.0.data.generation() {
            Ok(generation) => value.local_generation = generation.as_str().into(),
            Err(_) => {
                value.recovery_required = true;
                value.sync_state = SyncState::RecoveryRequired;
            }
        }
        value
    }

    fn update(&self, update: impl FnOnce(&mut EncryptedSyncStatus)) {
        let status = {
            let mut status = self.0.status.lock().unwrap_or_else(|e| e.into_inner());
            update(&mut status);
            status.sequence = self
                .0
                .sequence
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1)
                .to_string();
            status.clone()
        };
        self.0.events.publish(
            None,
            BackendEventKind::CloudSyncStateChanged(EncryptedSyncEvent {
                sequence: status.sequence.clone(),
                account_id: status.account.as_ref().map(|v| v.github_id.clone()),
                vault_id: status.vault_id.clone(),
                task_id: status.task_id.clone(),
                status,
            }),
        );
    }

    fn begin(&self) {
        self.update(|s| {
            self.0.cancelled.store(false, Ordering::Release);
            s.task_id = Some(uuid::Uuid::new_v4().to_string());
            s.sync_state = SyncState::Syncing;
            s.last_error = None;
        });
    }

    fn check_cancelled(&self) -> SyncResult<()> {
        if self.0.cancelled.load(Ordering::Acquire) || self.0.shutdown.load(Ordering::Acquire) {
            Err(error("cancelled"))
        } else {
            Ok(())
        }
    }

    async fn current_account(&self, runtime: &Runtime) -> SyncResult<()> {
        self.check_cancelled()?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        if self
            .0
            .marketplace
            .read_access_token()
            .await
            .map_err(|_| error("sign_in_required"))?
            != connection.github_token
        {
            return Err(error("account_changed"));
        }
        self.check_cancelled()
    }

    fn finish<T>(&self, runtime: &mut Runtime, result: &SyncResult<T>) {
        let failure = result.as_ref().err().map(|e| {
            let code = e
                .details
                .as_ref()
                .and_then(|v| v.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("sync_failed")
                .to_owned();
            let retry_after_seconds = e
                .details
                .as_ref()
                .and_then(|v| v.get("retryAfterSeconds"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok());
            SyncFailure {
                code,
                retry_after_seconds,
            }
        });
        if let Some(seconds) = failure.as_ref().and_then(|v| v.retry_after_seconds) {
            runtime.retry_at = Some(Instant::now() + Duration::from_secs(u64::from(seconds)));
        }
        self.update(|s| {
            if let Ok(generation) = self.0.data.generation() {
                s.local_generation = generation.as_str().into();
            }
            s.task_id = None;
            s.enabled = runtime.settings.enabled;
            s.key_state = if runtime.key.is_some() {
                KeyState::Unlocked
            } else {
                KeyState::Locked
            };
            s.consent_version = runtime.settings.consent_version.clone();
            s.last_successful_sync_at = runtime.settings.last_success.clone();
            s.last_synced_local_generation = runtime.settings.last_generation.clone();
            if let Some(failure) = failure {
                s.sync_state = match failure.code.as_str() {
                    "sign_in_required" | "account_changed" => {
                        s.auth_state = AuthState::Expired;
                        SyncState::SignInRequired
                    }
                    "unlock_required" | "invalid_password_or_ciphertext" => {
                        SyncState::UnlockRequired
                    }
                    "outcome_unknown" => SyncState::OutcomeUnknown,
                    "recovery_required" => {
                        s.recovery_required = true;
                        SyncState::RecoveryRequired
                    }
                    "conflict" | "restore_review_required" => SyncState::Conflict,
                    "cancelled" => {
                        if s.enabled {
                            SyncState::Pending
                        } else {
                            SyncState::Disabled
                        }
                    }
                    _ => SyncState::Failed,
                };
                s.last_error = Some(failure);
            } else {
                s.last_error = None;
                s.sync_state = if runtime.preview.is_some() {
                    SyncState::Conflict
                } else if !s.enabled {
                    SyncState::Disabled
                } else if s.auth_state != AuthState::SignedIn {
                    SyncState::SignInRequired
                } else if runtime.key.is_none() {
                    SyncState::UnlockRequired
                } else if s.pending_operation_id.is_some() {
                    SyncState::OutcomeUnknown
                } else if s.last_synced_local_generation.as_deref()
                    != Some(s.local_generation.as_str())
                {
                    SyncState::Pending
                } else {
                    SyncState::Ready
                };
            }
        });
    }

    async fn initialize(&self, runtime: &mut Runtime) -> SyncResult<()> {
        // Recovery precedes all ordinary mutations and backend-ready reporting.
        // It also handles a dropped IPC future without requiring a process restart.
        self.0.data.recover().await?;
        if runtime.initialized {
            return Ok(());
        }
        if self.0.local.initialized()? {
            runtime.settings = self
                .0
                .local
                .read("device", "client")
                .await?
                .ok_or_else(|| error("recovery_required"))?;
            if runtime.settings.version != 1 {
                return Err(error("recovery_required"));
            }
        }
        if self.0.local.locked_out().await? {
            runtime.settings.remember_key = false;
            runtime.key = None;
            runtime.preview = None;
        }
        runtime.initialized = true;
        Ok(())
    }

    async fn save_settings(&self, runtime: &Runtime) -> SyncResult<()> {
        self.0
            .local
            .write("device", "client", runtime.settings.clone())
            .await
    }

    async fn connect(&self, runtime: &mut Runtime) -> SyncResult<()> {
        self.initialize(runtime).await?;
        self.check_cancelled()?;
        if runtime.retry_at.is_some_and(|at| at > Instant::now()) {
            return Err(error("rate_limited"));
        }
        // Capture once after recovery has applied the current preferences. Both
        // cache validation and client construction use this exact policy.
        let proxy_policy =
            ProxyPolicy::for_origin(&self.0.config.origin, crate::net::use_system_proxy());
        self.connect_with_proxy_policy(runtime, proxy_policy).await
    }

    async fn connect_with_proxy_policy(
        &self,
        runtime: &mut Runtime,
        proxy_policy: ProxyPolicy,
    ) -> SyncResult<()> {
        let valid = if let Some(connection) = &runtime.connection {
            connection.expires_at > Instant::now() + Duration::from_secs(30)
                && connection.transport.proxy_policy() == proxy_policy
                && self.0.marketplace.read_access_token().await.ok().as_ref()
                    == Some(&connection.github_token)
        } else {
            false
        };
        if valid {
            return Ok(());
        }
        #[cfg(not(test))]
        let transport = Transport::new(
            &self.0.config.origin,
            &self.0.config.github_client_id,
            proxy_policy,
        )
        .await
        .map_err(protocol_error)?;
        #[cfg(test)]
        let transport = if self.0.config.origin.starts_with("http://127.0.0.1:") {
            Transport::for_test_with_proxy_policy(
                &self.0.config.origin,
                &self.0.config.github_client_id,
                proxy_policy,
            )
            .await
            .map_err(protocol_error)?
        } else {
            Transport::new(
                &self.0.config.origin,
                &self.0.config.github_client_id,
                proxy_policy,
            )
            .await
            .map_err(protocol_error)?
        };
        let (token, account) = self
            .0
            .marketplace
            .sync_identity()
            .await
            .map_err(|_| error("sign_in_required"))?;
        self.check_cancelled()?;
        let session = transport
            .exchange(token.expose_secret(), &account.github_id)
            .await
            .map_err(protocol_error)?;
        if session.account().github_id != account.github_id
            || transport.service_origin().trim_end_matches('/')
                != self.0.config.origin.trim_end_matches('/')
        {
            return Err(error("account_changed"));
        }
        let backup_retention_days = transport.capabilities().max_backup_retention_days;
        if self
            .0
            .marketplace
            .read_access_token()
            .await
            .map_err(|_| error("sign_in_required"))?
            != token
        {
            return Err(error("account_changed"));
        }
        let owner = account.github_id.as_str().to_string();
        if runtime.settings.owner_id.as_deref() != Some(&owner) {
            if runtime.settings.owner_id.is_some() {
                runtime.settings.enabled = false;
                runtime.settings.remember_key = false;
                self.save_settings(runtime).await?;
            }
            self.forget_unlock_material(runtime).await?;
            runtime.settings.enabled = false;
            runtime.settings.consent_version = None;
            runtime.settings.remember_key = false;
            runtime.settings.last_success = None;
            runtime.settings.last_generation = None;
            runtime.settings.vault_id = None;
            runtime.settings.key_id = None;
            runtime.key = None;
            runtime.baseline = None;
            runtime.preview = None;
            runtime.metadata = None;
            runtime.snapshot = None;
        }
        runtime.settings.owner_id = Some(owner.clone());
        let expires_at = Instant::now() + Duration::from_secs(u64::from(session.expires_in()));
        runtime.connection = Some(Connection {
            transport,
            session,
            github_token: token,
            expires_at,
        });
        runtime.baseline = self.0.local.read(&owner, "baseline").await?;
        let pending: Option<Pending> = self.0.local.read(&owner, "pending").await?;
        self.update(|s| {
            s.account = Some(SyncAccount {
                github_id: owner,
                login: account.login,
            });
            s.auth_state = AuthState::SignedIn;
            s.backup_retention_days = Some(backup_retention_days);
            s.pending_operation_id = pending.map(|p| p.operation_id);
            s.has_cloud_snapshot = None;
        });
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn connect_with_proxy_setting_for_test(
        &self,
        use_system_proxy: bool,
    ) -> SyncResult<()> {
        let mut runtime = self.0.runtime.lock().await;
        self.initialize(&mut runtime).await?;
        self.check_cancelled()?;
        let policy = ProxyPolicy::for_origin(&self.0.config.origin, use_system_proxy);
        self.connect_with_proxy_policy(&mut runtime, policy).await
    }

    async fn refresh_metadata(&self, runtime: &mut Runtime) -> SyncResult<()> {
        self.current_account(runtime).await?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let metadata = match connection
            .transport
            .metadata(&connection.session, None)
            .await
            .map_err(protocol_error)?
        {
            MetadataResult::Modified(value) => *value,
            MetadataResult::NotModified => return Err(error("invalid_response")),
        };
        self.current_account(runtime).await?;
        let value = metadata.value();
        let owner = self.owner(runtime)?.to_owned();
        let seen: Option<Revision> = self.0.local.read(&owner, "last-seen-revision").await?;
        if seen.is_some_and(|seen| value.revision < seen)
            || runtime
                .baseline
                .as_ref()
                .is_some_and(|base| value.revision < base.revision)
        {
            return Err(error("revision_rollback"));
        }
        self.0
            .local
            .write(&owner, "last-seen-revision", value.revision)
            .await?;
        let key_binding_changed = runtime.key.is_some()
            && (runtime.settings.vault_id.as_deref()
                != value.vault_id.as_ref().map(UuidV4::as_str)
                || runtime.settings.key_id.as_deref() != value.key_id.as_ref().map(UuidV4::as_str));
        if value.state != VaultState::Active
            || key_binding_changed
            || runtime.snapshot.as_ref().is_some_and(|s| {
                Some(&s.key_id) != value.key_id.as_ref()
                    || Some(&s.vault_id) != value.vault_id.as_ref()
            })
        {
            runtime.key = None;
            runtime.snapshot = None;
            runtime.preview = None;
        }
        if value.state == VaultState::Deleted {
            runtime.settings.enabled = false;
        }
        self.update(|s| {
            s.remote_revision = Some(value.revision.as_str().into());
            s.vault_id = value.vault_id.as_ref().map(|v| v.as_str().into());
            s.key_id = value.key_id.as_ref().map(|v| v.as_str().into());
            s.has_cloud_snapshot = Some(value.state == VaultState::Active);
        });
        runtime.metadata = Some(metadata);
        Ok(())
    }

    fn metadata<'a>(&self, runtime: &'a Runtime) -> SyncResult<&'a Metadata> {
        runtime
            .metadata
            .as_ref()
            .ok_or_else(|| error("metadata_required"))
    }
    fn owner<'a>(&self, runtime: &'a Runtime) -> SyncResult<&'a str> {
        runtime
            .settings
            .owner_id
            .as_deref()
            .ok_or_else(|| error("sign_in_required"))
    }
    fn unlocked(&self, runtime: &Runtime) -> SyncResult<Arc<DerivedKey>> {
        runtime.key.clone().ok_or_else(|| error("unlock_required"))
    }

    async fn download(&self, runtime: &mut Runtime) -> SyncResult<SnapshotUpload> {
        self.current_account(runtime).await?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let snapshot = connection
            .transport
            .snapshot(&connection.session, self.metadata(runtime)?)
            .await
            .map_err(protocol_error)?;
        self.current_account(runtime).await?;
        Ok(snapshot)
    }

    async fn decrypt(
        &self,
        runtime: &Runtime,
        snapshot: SnapshotUpload,
        key: Arc<DerivedKey>,
    ) -> SyncResult<ValidatedSyncDocuments> {
        let metadata = self.metadata(runtime)?.value().clone();
        let owner = GithubId::parse(self.owner(runtime)?).map_err(protocol_error)?;
        tokio::task::spawn_blocking(move || {
            let set = crypto::decrypt_snapshot(&snapshot, &metadata, &owner, &key)
                .map_err(protocol_error)?;
            documents::validate_sync_documents(set, metadata.revision).map_err(document_error)
        })
        .await
        .map_err(|_| error("crypto_worker_failed"))?
    }

    async fn remember(
        &self,
        runtime: &mut Runtime,
        snapshot: &SnapshotUpload,
        key: Arc<DerivedKey>,
        remember: bool,
    ) -> SyncResult<()> {
        self.0
            .local
            .remember(
                self.owner(runtime)?,
                snapshot.vault_id.as_str(),
                snapshot.key_id.as_str(),
                &key,
                remember,
            )
            .await?;
        runtime.key = Some(key);
        runtime.settings.remember_key = remember;
        runtime.settings.key_preference_epoch = runtime
            .settings
            .key_preference_epoch
            .checked_add(1)
            .ok_or_else(|| error("recovery_required"))?;
        runtime.settings.vault_id = Some(snapshot.vault_id.as_str().into());
        runtime.settings.key_id = Some(snapshot.key_id.as_str().into());
        self.save_settings(runtime).await?;
        self.0.local.set_lockout(false).await
    }

    async fn try_remembered(&self, runtime: &mut Runtime) -> SyncResult<()> {
        if runtime.key.is_some()
            || !runtime.settings.remember_key
            || self.0.local.locked_out().await?
        {
            return Ok(());
        }
        let metadata = self.metadata(runtime)?.value();
        if metadata.state != VaultState::Active {
            return Ok(());
        }
        let vault = metadata
            .vault_id
            .as_ref()
            .ok_or_else(|| error("invalid_response"))?
            .as_str();
        let key_id = metadata
            .key_id
            .as_ref()
            .ok_or_else(|| error("invalid_response"))?
            .as_str();
        if runtime.settings.vault_id.as_deref() != Some(vault)
            || runtime.settings.key_id.as_deref() != Some(key_id)
        {
            return Ok(());
        }
        if let Some(key) = self
            .0
            .local
            .remembered(self.owner(runtime)?, vault, key_id)
            .await?
        {
            let snapshot = self.download(runtime).await?;
            let key = Arc::new(key);
            // Merely retrieving/deriving a key cannot set keyState=unlocked.
            self.decrypt(runtime, snapshot.clone(), key.clone()).await?;
            self.current_account(runtime).await?;
            runtime.snapshot = Some(snapshot);
            runtime.key = Some(key);
        }
        Ok(())
    }

    pub(crate) async fn prepare_enable(
        &self,
        consent_version: String,
    ) -> SyncResult<EnablePreparation> {
        if consent_version != CONSENT_VERSION {
            return Err(error("consent_required"));
        }
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            runtime.settings.consent_version = Some(consent_version);
            runtime.settings.prompted = true;
            self.save_settings(&runtime).await?;
            self.try_remembered(&mut runtime).await?;
            let step = if self.metadata(&runtime)?.value().state != VaultState::Active {
                EnableStep::Create
            } else if runtime.key.is_none() {
                EnableStep::Unlock
            } else if runtime.baseline.as_ref().is_some_and(|b| {
                Some(&b.vault_id)
                    == self
                        .metadata(&runtime)
                        .ok()
                        .and_then(|m| m.value().vault_id.as_ref())
            }) {
                EnableStep::Ready
            } else {
                EnableStep::RestoreReview
            };
            Ok(step)
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|next_step| EnablePreparation {
            next_step,
            status: self.status(),
        })
    }

    pub(crate) async fn unlock(
        &self,
        password: String,
        remember_key: bool,
    ) -> SyncResult<EncryptedSyncStatus> {
        let password = SecretInput::new(password);
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.0.auto_suspended.store(false, Ordering::Release);
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
                return Err(error("consent_required"));
            }
            let snapshot = self.download(&mut runtime).await?;
            let encoded = snapshot.clone();
            let metadata = self.metadata(&runtime)?.value().clone();
            let owner = GithubId::parse(self.owner(&runtime)?).map_err(protocol_error)?;
            let (key, documents) = tokio::task::spawn_blocking(move || {
                let (key, set) =
                    crypto::decrypt_snapshot_with_password(&encoded, &metadata, &owner, password)
                        .map_err(protocol_error)?;
                let documents = documents::validate_sync_documents(set, metadata.revision)
                    .map_err(document_error)?;
                Ok::<_, BackendError>((Arc::new(key), documents))
            })
            .await
            .map_err(|_| error("crypto_worker_failed"))??;
            self.current_account(&runtime).await?;
            if let (Some(vault), Some(key_id)) =
                (&runtime.settings.vault_id, &runtime.settings.key_id)
            {
                if vault != snapshot.vault_id.as_str() || key_id != snapshot.key_id.as_str() {
                    self.0
                        .local
                        .forget(self.owner(&runtime)?, vault, key_id)
                        .await?;
                }
            }
            self.remember(&mut runtime, &snapshot, key, remember_key)
                .await?;
            runtime.snapshot = Some(snapshot);
            if !runtime.baseline.as_ref().is_some_and(|b| {
                Some(&b.vault_id)
                    == self
                        .metadata(&runtime)
                        .ok()
                        .and_then(|m| m.value().vault_id.as_ref())
            }) {
                self.make_preview(&mut runtime, documents).await?;
            }
            Ok(())
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn create(
        &self,
        password: String,
        confirmation: String,
        remember_key: bool,
        consent_version: String,
        observed_revision: String,
    ) -> SyncResult<EncryptedSyncStatus> {
        let password = SecretInput::new(password);
        let confirmation = SecretInput::new(confirmation);
        if consent_version != CONSENT_VERSION {
            return Err(error("consent_required"));
        }
        let observed = Revision::parse(&observed_revision).map_err(protocol_error)?;
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.0.auto_suspended.store(false, Ordering::Release);
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.reconcile_for_review(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            let metadata = self.metadata(&runtime)?.value();
            if metadata.state == VaultState::Active || metadata.revision != observed {
                return Err(error("revision_conflict"));
            }
            if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
                return Err(error("consent_required"));
            }
            self.retire_reviewed_pending(&mut runtime, None).await?;
            let context = EncryptContext {
                owner_github_id: GithubId::parse(self.owner(&runtime)?).map_err(protocol_error)?,
                vault_id: UuidV4::random().map_err(protocol_error)?,
                key_id: UuidV4::random().map_err(protocol_error)?,
                base_revision: observed,
                operation_id: UuidV4::random().map_err(protocol_error)?,
                kind: UploadKind::Create,
                kdf: Kdf::fixed(Salt::random().map_err(protocol_error)?),
            };
            let captured = self
                .0
                .data
                .export(self.scope(&runtime, context.vault_id.as_str(), context.key_id.as_str())?)
                .await?;
            let documents = captured.documents.documents().clone();
            let (snapshot, key) = tokio::task::spawn_blocking(move || {
                let password = NormalizedPassword::confirmed_new(password, confirmation)
                    .map_err(protocol_error)?;
                let key =
                    Arc::new(crypto::derive_key(&password, &context.kdf).map_err(protocol_error)?);
                let snapshot =
                    crypto::encrypt_snapshot(&documents, &context, &key).map_err(protocol_error)?;
                Ok::<_, BackendError>((snapshot, key))
            })
            .await
            .map_err(|_| error("crypto_worker_failed"))??;
            self.current_account(&runtime).await?;
            self.remember(&mut runtime, &snapshot, key, remember_key)
                .await?;
            self.upload(&mut runtime, snapshot, captured, None).await?;
            self.check_cancelled()?;
            runtime.settings.enabled = true;
            self.save_settings(&runtime).await?;
            Ok(())
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    async fn upload(
        &self,
        runtime: &mut Runtime,
        snapshot: SnapshotUpload,
        captured: ExportedDocuments,
        old_key_id: Option<String>,
    ) -> SyncResult<()> {
        self.current_account(runtime).await?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let operation = connection
            .transport
            .prepare_upload(
                &connection.session,
                self.metadata(runtime)?,
                &snapshot,
                runtime.snapshot.as_ref(),
            )
            .map_err(protocol_error)?;
        let pending = Pending {
            record: String::from_utf8(operation.to_pending_record().map_err(protocol_error)?)
                .map_err(|_| error("invalid_response"))?,
            operation_id: snapshot.operation_id.as_str().into(),
            vault_id: snapshot.vault_id.as_str().into(),
            key_id: Some(snapshot.key_id.as_str().into()),
            local_generation: captured.generation,
            deleted: false,
            remember_key: runtime.settings.remember_key,
            key_preference_epoch: runtime.settings.key_preference_epoch,
            base_revision: snapshot.base_revision,
            old_key_id,
        };
        let owner = self.owner(runtime)?.to_string();
        self.0
            .local
            .write(
                &owner,
                &format!("proposal:{}", pending.operation_id),
                captured.documents.documents().clone(),
            )
            .await?;
        self.0
            .local
            .write(&owner, "pending", pending.clone())
            .await?;
        self.update(|s| s.pending_operation_id = Some(pending.operation_id.clone()));
        self.current_account(runtime).await?;
        // Once submitted, preserve the exact operation until a validated receipt.
        let receipt = connection
            .transport
            .submit(&connection.session, &operation)
            .await;
        match receipt {
            Ok(receipt) => {
                self.accept_receipt(runtime, &pending, &receipt.receipt)
                    .await?;
                runtime.snapshot = Some(snapshot);
                if receipt.replayed {
                    // A replay proves that operation committed, not that its
                    // revision is still the server's current head.
                    self.update(|s| s.has_cloud_snapshot = None);
                }
                Ok(())
            }
            Err(e) => {
                if definite_rejection(&e) {
                    self.discard_rejected(runtime, &pending).await?;
                    if snapshot.kind != UploadKind::Snapshot {
                        self.0
                            .local
                            .forget(&owner, snapshot.vault_id.as_str(), snapshot.key_id.as_str())
                            .await?;
                    }
                    if snapshot.kind == UploadKind::Create {
                        runtime.key = None;
                        runtime.settings.remember_key = false;
                        runtime.settings.key_id = None;
                        runtime.settings.vault_id = None;
                        self.save_settings(runtime).await?;
                    }
                    return Err(protocol_error(e));
                }
                Err(error("outcome_unknown"))
            }
        }
    }

    async fn reconcile(&self, runtime: &mut Runtime) -> SyncResult<()> {
        let owner = self.owner(runtime)?.to_owned();
        let Some(pending): Option<Pending> = self.0.local.read(&owner, "pending").await? else {
            return Ok(());
        };
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let operation = connection
            .transport
            .restore_pending(&connection.session, pending.record.as_bytes())
            .map_err(protocol_error)?;
        if operation.operation_id().as_str() != pending.operation_id {
            return Err(error("recovery_required"));
        }
        self.current_account(runtime).await?;
        let receipt = match connection
            .transport
            .operation(&connection.session, &operation)
            .await
            .map_err(protocol_error)?
        {
            OperationStatus::Committed(receipt) => receipt,
            OperationStatus::Pending(pending) => {
                let mut result = error("outcome_unknown");
                result.details = Some(
                    serde_json::json!({"reason":"outcome_unknown","retryAfterSeconds":pending.retry_after_seconds}),
                );
                return Err(result);
            }
            OperationStatus::NotFound => {
                // 404 does not prove failure. Only an identical body/op ID can be retried.
                self.current_account(runtime).await?;
                match connection
                    .transport
                    .submit(&connection.session, &operation)
                    .await
                {
                    Ok(receipt) => receipt.receipt,
                    Err(e) if definite_rejection(&e) => {
                        // This is an older unknown operation: rejection of a
                        // retry cannot prove that the original never committed.
                        let _ = self.refresh_metadata(runtime).await;
                        return Err(error("outcome_unknown"));
                    }
                    Err(_) => return Err(error("outcome_unknown")),
                }
            }
        };
        self.accept_receipt(runtime, &pending, &receipt).await
    }

    async fn accept_receipt(
        &self,
        runtime: &mut Runtime,
        pending: &Pending,
        receipt: &OperationReceipt,
    ) -> SyncResult<()> {
        let owner = self.owner(runtime)?.to_string();
        if receipt.operation_id.as_str() != pending.operation_id
            || receipt.vault_id.as_str() != pending.vault_id
        {
            return Err(error("invalid_response"));
        }
        if pending.deleted {
            runtime.settings.enabled = false;
            runtime.key = None;
            runtime.preview = None;
            runtime.baseline = None;
            self.0.local.remove(&owner, "baseline").await?;
            if let Some(key_id) = &runtime.settings.key_id {
                self.0
                    .local
                    .forget(&owner, &pending.vault_id, key_id)
                    .await?;
            }
            runtime.settings.remember_key = false;
            runtime.settings.vault_id = None;
            runtime.settings.key_id = None;
        } else {
            if runtime.settings.vault_id.as_deref() != Some(pending.vault_id.as_str())
                || runtime.settings.key_id.as_ref() != pending.key_id.as_ref()
            {
                // Receipt reconciliation can complete a password rotation while
                // memory still holds the old key from the failed upload turn.
                runtime.key = None;
                runtime.snapshot = None;
                runtime.preview = None;
            }
            let documents: DocumentSet = self
                .0
                .local
                .read(&owner, &format!("proposal:{}", pending.operation_id))
                .await?
                .ok_or_else(|| error("recovery_required"))?;
            documents::validate_sync_documents(documents.clone(), receipt.committed_revision)
                .map_err(document_error)?;
            let baseline = Baseline {
                revision: receipt.committed_revision,
                vault_id: receipt.vault_id.clone(),
                documents,
            };
            self.0
                .local
                .write(&owner, "baseline", baseline.clone())
                .await?;
            self.0
                .data
                .baseline(
                    self.scope(
                        runtime,
                        &pending.vault_id,
                        pending
                            .key_id
                            .as_deref()
                            .ok_or_else(|| error("recovery_required"))?,
                    )?,
                    baseline.documents.clone(),
                    baseline.revision,
                )
                .await?;
            runtime.baseline = Some(baseline);
            runtime.settings.vault_id = Some(pending.vault_id.clone());
            runtime.settings.key_id = pending.key_id.clone();
            if runtime.settings.key_preference_epoch == pending.key_preference_epoch
                && !self.0.local.locked_out().await?
            {
                runtime.settings.remember_key = pending.remember_key;
            }
            if let Some(old) = &pending.old_key_id {
                if Some(old) != pending.key_id.as_ref() {
                    self.0.local.forget(&owner, &pending.vault_id, old).await?;
                }
            }
        }
        runtime.settings.last_generation = Some(pending.local_generation.as_str().into());
        runtime.settings.last_success = Some(receipt.committed_at.clone());
        let seen: Option<Revision> = self.0.local.read(&owner, "last-seen-revision").await?;
        self.0
            .local
            .write(
                &owner,
                "last-seen-revision",
                seen.map_or(receipt.committed_revision, |seen| {
                    seen.max(receipt.committed_revision)
                }),
            )
            .await?;
        self.save_settings(runtime).await?;
        // Deleting the pending record is last; a crash at any earlier step replays safely.
        self.0.local.remove(&owner, "pending").await?;
        // This is an unreferenced encrypted scratch copy after pending removal.
        // Its cleanup failure cannot undo a confirmed commit or restore old keys.
        let _ = self
            .0
            .local
            .remove(&owner, &format!("proposal:{}", pending.operation_id))
            .await;
        self.update(|s| {
            s.pending_operation_id = None;
            s.remote_revision = Some(receipt.committed_revision.as_str().into());
            s.has_cloud_snapshot = Some(!pending.deleted);
            s.vault_id = (!pending.deleted).then(|| pending.vault_id.clone());
            s.key_id = if pending.deleted {
                None
            } else {
                pending.key_id.clone()
            };
        });
        Ok(())
    }

    pub(crate) fn cancel(&self, task_id: &str) -> SyncResult<EncryptedSyncStatus> {
        {
            // The identity check and flag update share begin/finish's status
            // lock. A delayed cancel can never poison the next task's flag.
            let status = self.0.status.lock().unwrap_or_else(|e| e.into_inner());
            if status.task_id.as_deref() != Some(task_id) {
                return Err(error("stale_task"));
            }
            self.0.cancelled.store(true, Ordering::Release);
        }
        Ok(self.status())
    }

    pub(crate) async fn lock(&self) -> SyncResult<EncryptedSyncStatus> {
        self.0.auto_suspended.store(true, Ordering::Release);
        self.0.cancelled.store(true, Ordering::Release);
        let mut runtime = self.0.runtime.lock().await;
        runtime.key = None;
        runtime.preview = None;
        runtime.settings.remember_key = false;
        let result = async {
            self.0.local.set_lockout(true).await?;
            self.initialize(&mut runtime).await?;
            runtime.settings.remember_key = false;
            runtime.settings.key_preference_epoch = runtime
                .settings
                .key_preference_epoch
                .checked_add(1)
                .ok_or_else(|| error("recovery_required"))?;
            // Persist the refusal to auto-unlock before asking the OS to delete
            // a key; a denied deletion must not unlock again on restart.
            self.save_settings(&runtime).await?;
            self.forget_unlock_material(&runtime).await
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn set_enabled(&self, enabled: bool) -> SyncResult<EncryptedSyncStatus> {
        if !enabled {
            self.0.auto_suspended.store(true, Ordering::Release);
            self.0.cancelled.store(true, Ordering::Release);
        } else {
            self.0.auto_suspended.store(false, Ordering::Release);
        }
        let mut runtime = self.0.runtime.lock().await;
        self.initialize(&mut runtime).await?;
        let result = async {
            if enabled {
                if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
                    return Err(error("consent_required"));
                }
                if runtime.preview.is_some() || runtime.baseline.is_none() {
                    return Err(error("restore_review_required"));
                }
                self.unlocked(&runtime)?;
                self.0.cancelled.store(false, Ordering::Release);
                self.current_account(&runtime).await?;
            }
            runtime.settings.enabled = enabled;
            self.save_settings(&runtime).await?;
            if enabled {
                self.0.wake.notify_one();
            }
            Ok(())
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    fn scope(&self, runtime: &Runtime, vault: &str, key: &str) -> SyncResult<SyncScope> {
        Ok(SyncScope {
            service_origin: self.0.config.origin.clone(),
            owner_github_id: self.owner(runtime)?.into(),
            vault_id: vault.into(),
            key_id: key.into(),
            device_id: self.0.data.device().id.clone(),
        })
    }

    fn active_scope(&self, runtime: &Runtime) -> SyncResult<SyncScope> {
        let meta = self.metadata(runtime)?.value();
        self.scope(
            runtime,
            meta.vault_id
                .as_ref()
                .ok_or_else(|| error("cloud_deleted"))?
                .as_str(),
            meta.key_id
                .as_ref()
                .ok_or_else(|| error("cloud_deleted"))?
                .as_str(),
        )
    }

    fn baseline_documents(&self, runtime: &Runtime) -> SyncResult<Option<ValidatedSyncDocuments>> {
        runtime
            .baseline
            .as_ref()
            .filter(|b| {
                Some(&b.vault_id)
                    == runtime
                        .metadata
                        .as_ref()
                        .and_then(|m| m.value().vault_id.as_ref())
            })
            .map(|b| {
                documents::validate_sync_documents(b.documents.clone(), b.revision)
                    .map_err(document_error)
            })
            .transpose()
    }

    async fn make_preview(
        &self,
        runtime: &mut Runtime,
        remote: ValidatedSyncDocuments,
    ) -> SyncResult<RestorePreview> {
        let scope = self.active_scope(runtime)?;
        let local = self.0.data.export(scope.clone()).await?;
        let baseline = self.baseline_documents(runtime)?;
        let merged = documents::diff_sync_documents(baseline.as_ref(), &local.documents, &remote)
            .map_err(document_error)?;
        let conflicts = merged
            .conflicts()
            .iter()
            .map(|conflict| SyncConflictItem {
                id: conflict.conflict_id.clone(),
                kind: wire_name(&conflict.kind),
                reason: wire_name(&conflict.reason),
            })
            .collect();
        let mut counts = BTreeMap::new();
        for doc in &remote.documents().documents {
            *counts.entry(wire_name(&doc.kind)).or_insert(0) += 1;
        }
        let public = RestorePreview {
            preview_id: uuid::Uuid::new_v4().to_string(),
            unconfirmed_operation_id: self
                .reviewable_pending(runtime)
                .await?
                .map(|p| p.operation_id),
            observed_revision: remote.observed_revision().as_str().into(),
            local_generation: local.generation.as_str().into(),
            counts,
            device_settings_to_review: remote
                .documents()
                .documents
                .iter()
                .filter(|d| d.kind == DocumentKind::DeviceProfile)
                .map(|_| "device_profile".to_string())
                .take(1)
                .collect(),
            conflicts,
        };
        runtime.preview = Some(PreviewState {
            public: public.clone(),
            remote,
            local,
            baseline,
            vault_id: scope.vault_id.clone(),
            key_id: scope.key_id.clone(),
        });
        self.0.events.publish(
            None,
            BackendEventKind::CloudSyncConflictDetected(EncryptedSyncConflictEvent {
                sequence: self
                    .0
                    .sequence
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1)
                    .to_string(),
                account_id: scope.owner_github_id,
                vault_id: scope.vault_id,
                task_id: self.status().task_id,
                preview: public.clone(),
            }),
        );
        Ok(public)
    }

    pub(crate) async fn preview_restore(
        &self,
        observed_revision: String,
    ) -> SyncResult<RestorePreview> {
        let observed = Revision::parse(&observed_revision).map_err(protocol_error)?;
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.reconcile_for_review(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            if self.metadata(&runtime)?.value().revision != observed {
                return Err(error("stale_preview"));
            }
            let key = self.unlocked(&runtime)?;
            let snapshot = self.download(&mut runtime).await?;
            let remote = self.decrypt(&runtime, snapshot.clone(), key).await?;
            runtime.snapshot = Some(snapshot);
            self.current_account(&runtime).await?;
            self.make_preview(&mut runtime, remote).await
        }
        .await;
        self.finish(&mut runtime, &result);
        result
    }

    pub(crate) async fn apply_restore(
        &self,
        preview_id: String,
        mode: RestoreMode,
        choices: Vec<SyncConflictChoice>,
    ) -> SyncResult<EncryptedSyncStatus> {
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.current_account(&runtime).await?;
            self.unlocked(&runtime)?;
            self.refresh_metadata(&mut runtime).await?;
            let preview = runtime
                .preview
                .as_ref()
                .ok_or_else(|| error("stale_preview"))?;
            let scope = self.active_scope(&runtime)?;
            if preview.public.preview_id != preview_id
                || self.metadata(&runtime)?.value().revision.as_str()
                    != preview.public.observed_revision
                || self.0.data.generation()?.as_str() != preview.public.local_generation
                || scope.vault_id != preview.vault_id
                || scope.key_id != preview.key_id
            {
                return Err(error("stale_preview"));
            }
            let desired = match mode {
                RestoreMode::Replace => preview.remote.clone(),
                RestoreMode::Merge => {
                    let choices: Vec<documents::ConflictChoice> = choices
                        .into_iter()
                        .map(|c| documents::ConflictChoice {
                            conflict_id: c.id,
                            side: match c.side {
                                ConflictSide::Local => documents::ConflictSide::Local,
                                ConflictSide::Cloud => documents::ConflictSide::Remote,
                            },
                        })
                        .collect();
                    documents::diff_sync_documents(
                        preview.baseline.as_ref(),
                        &preview.local.documents,
                        &preview.remote,
                    )
                    .map_err(document_error)?
                    .resolve(&choices)
                    .map_err(document_error)?
                }
            };
            let remote = preview.remote.clone();
            let context = RestoreContext {
                scope: scope.clone(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                observed_revision: remote.observed_revision(),
                local_generation: preview.local.generation,
                target_device: self.0.data.device(),
            };
            let reviewed_operation = preview.public.unconfirmed_operation_id.clone();
            self.current_account(&runtime).await?;
            self.retire_reviewed_pending(&mut runtime, reviewed_operation.as_deref())
                .await?;
            // After journal prepare, cancellation must finish commit/rollback.
            let applied = self.0.data.restore(desired, context).await?;
            let baseline = Baseline {
                revision: remote.observed_revision(),
                vault_id: UuidV4::parse(&scope.vault_id).map_err(protocol_error)?,
                documents: remote.documents().clone(),
            };
            self.0
                .local
                .write(&scope.owner_github_id, "baseline", baseline.clone())
                .await?;
            self.0
                .data
                .baseline(scope.clone(), baseline.documents.clone(), baseline.revision)
                .await?;
            runtime.baseline = Some(baseline);
            runtime.preview = None;
            runtime.settings.last_generation =
                if same_documents(applied.documents.documents(), remote.documents()) {
                    Some(applied.generation.as_str().into())
                } else {
                    None
                };
            if !self.0.cancelled.load(Ordering::Acquire) {
                runtime.settings.enabled = true;
            }
            self.save_settings(&runtime).await?;
            let ui_preferences = applied
                .documents
                .documents()
                .documents
                .iter()
                .filter(|d| d.kind == DocumentKind::UiPreferences)
                .filter_map(|d| d.value.as_str().map(|v| (d.id.clone(), v.to_owned())))
                .collect();
            self.0.events.publish(
                None,
                BackendEventKind::CloudSyncRestoreCompleted(EncryptedSyncRestoreEvent {
                    sequence: self
                        .0
                        .sequence
                        .fetch_add(1, Ordering::AcqRel)
                        .saturating_add(1)
                        .to_string(),
                    account_id: scope.owner_github_id,
                    vault_id: scope.vault_id,
                    task_id: self.status().task_id,
                    local_generation: applied.generation.as_str().into(),
                    ui_preferences,
                }),
            );
            self.check_cancelled()?;
            // A local conflict choice can differ from the current cloud head.
            // Use the same CAS/merge path to upload it, never claim it synced early.
            self.run_once(&mut runtime).await
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn sync_now(&self) -> SyncResult<EncryptedSyncStatus> {
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = self.run_once(&mut runtime).await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    async fn run_once(&self, runtime: &mut Runtime) -> SyncResult<()> {
        self.connect(runtime).await?;
        self.reconcile(runtime).await?;
        if !runtime.settings.enabled {
            return Ok(());
        }
        if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
            return Err(error("consent_required"));
        }
        self.refresh_metadata(runtime).await?;
        if self.metadata(runtime)?.value().state != VaultState::Active {
            self.save_settings(runtime).await?;
            return Err(error("cloud_deleted"));
        }
        self.try_remembered(runtime).await?;
        let key = self.unlocked(runtime)?;
        let snapshot = self.download(runtime).await?;
        let remote = self.decrypt(runtime, snapshot.clone(), key.clone()).await?;
        runtime.snapshot = Some(snapshot.clone());
        let baseline = self.baseline_documents(runtime)?;
        if baseline.is_none() {
            self.make_preview(runtime, remote).await?;
            return Err(error("restore_review_required"));
        }
        let scope = self.active_scope(runtime)?;
        let mut captured = self.0.data.export(scope.clone()).await?;
        let merge = documents::diff_sync_documents(baseline.as_ref(), &captured.documents, &remote)
            .map_err(document_error)?;
        if !merge.conflicts().is_empty() {
            self.make_preview(runtime, remote).await?;
            return Err(error("conflict"));
        }
        let desired = merge.resolve(&[]).map_err(document_error)?;
        self.current_account(runtime).await?;
        if !same_documents(captured.documents.documents(), desired.documents()) {
            let context = RestoreContext {
                scope: scope.clone(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                observed_revision: remote.observed_revision(),
                local_generation: captured.generation,
                target_device: self.0.data.device(),
            };
            captured = self.0.data.restore(desired, context).await?;
            self.emit_restored(&scope, &captured);
        }
        if same_documents(captured.documents.documents(), remote.documents()) {
            let baseline = Baseline {
                revision: remote.observed_revision(),
                vault_id: snapshot.vault_id.clone(),
                documents: remote.documents().clone(),
            };
            self.0
                .local
                .write(&scope.owner_github_id, "baseline", baseline.clone())
                .await?;
            self.0
                .data
                .baseline(scope, baseline.documents.clone(), baseline.revision)
                .await?;
            runtime.baseline = Some(baseline);
            runtime.settings.last_generation = Some(captured.generation.as_str().into());
            runtime.settings.last_success = Some(chrono::Utc::now().to_rfc3339());
            self.save_settings(runtime).await?;
            return Ok(());
        }
        self.current_account(runtime).await?;
        let documents = captured.documents.documents().clone();
        let context = EncryptContext {
            owner_github_id: snapshot.owner_github_id.clone(),
            vault_id: snapshot.vault_id.clone(),
            key_id: snapshot.key_id.clone(),
            base_revision: self.metadata(runtime)?.value().revision,
            operation_id: UuidV4::random().map_err(protocol_error)?,
            kind: UploadKind::Snapshot,
            kdf: snapshot.kdf.clone(),
        };
        let encrypted = tokio::task::spawn_blocking(move || {
            crypto::encrypt_snapshot(&documents, &context, &key).map_err(protocol_error)
        })
        .await
        .map_err(|_| error("crypto_worker_failed"))??;
        self.upload(runtime, encrypted, captured, None).await
    }

    pub(crate) async fn change_password(
        &self,
        current_password: String,
        new_password: String,
        confirmation: String,
        remember_key: bool,
    ) -> SyncResult<EncryptedSyncStatus> {
        let current_password = SecretInput::new(current_password);
        let new_password = SecretInput::new(new_password);
        let confirmation = SecretInput::new(confirmation);
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.run_once(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            let previous = self.download(&mut runtime).await?;
            let metadata = self.metadata(&runtime)?.value().clone();
            if !runtime.baseline.as_ref().is_some_and(|baseline| {
                baseline.revision == metadata.revision && baseline.vault_id == previous.vault_id
            }) {
                // Another device advanced the head after run_once merged it.
                // Never encrypt the older local image against the newer CAS token.
                return Err(error("revision_conflict"));
            }
            let scope = self.active_scope(&runtime)?;
            let captured = self.0.data.export(scope).await?;
            let prior = previous.clone();
            let content = captured.documents.documents().clone();
            let (snapshot, key) = tokio::task::spawn_blocking(move || {
                let (_, _) = crypto::decrypt_snapshot_with_password(
                    &prior,
                    &metadata,
                    &prior.owner_github_id,
                    current_password,
                )
                .map_err(protocol_error)?;
                let password = NormalizedPassword::confirmed_new(new_password, confirmation)
                    .map_err(protocol_error)?;
                let context = EncryptContext {
                    owner_github_id: prior.owner_github_id.clone(),
                    vault_id: prior.vault_id.clone(),
                    key_id: UuidV4::random().map_err(protocol_error)?,
                    base_revision: metadata.revision,
                    operation_id: UuidV4::random().map_err(protocol_error)?,
                    kind: UploadKind::PasswordChange,
                    kdf: Kdf::fixed(Salt::random().map_err(protocol_error)?),
                };
                let key =
                    Arc::new(crypto::derive_key(&password, &context.kdf).map_err(protocol_error)?);
                let snapshot =
                    crypto::encrypt_snapshot(&content, &context, &key).map_err(protocol_error)?;
                Ok::<_, BackendError>((snapshot, key))
            })
            .await
            .map_err(|_| error("crypto_worker_failed"))??;
            self.current_account(&runtime).await?;
            let old_key = runtime.key.clone();
            let old_settings = runtime.settings.clone();
            // Both scoped key entries survive an unknown result; delete the old
            // remembered key only after the password-change receipt is verified.
            self.remember(&mut runtime, &snapshot, key, remember_key)
                .await?;
            runtime.snapshot = Some(previous.clone());
            let result = self
                .upload(
                    &mut runtime,
                    snapshot,
                    captured,
                    Some(previous.key_id.as_str().into()),
                )
                .await;
            if result.is_err() {
                let epoch = runtime.settings.key_preference_epoch;
                let remember = runtime.settings.remember_key;
                runtime.key = old_key;
                runtime.settings = old_settings;
                runtime.settings.key_preference_epoch = epoch;
                runtime.settings.remember_key = remember;
                self.save_settings(&runtime).await?;
            }
            result
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn delete_remote(
        &self,
        expected_vault_id: String,
        observed_revision: String,
        confirmed: bool,
    ) -> SyncResult<EncryptedSyncStatus> {
        if !confirmed {
            return Err(error("delete_confirmation_required"));
        }
        let observed = Revision::parse(&observed_revision).map_err(protocol_error)?;
        let expected = UuidV4::parse(&expected_vault_id).map_err(protocol_error)?;
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.reconcile_for_review(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            let metadata = self.metadata(&runtime)?;
            if metadata.value().revision != observed
                || metadata.value().vault_id.as_ref() != Some(&expected)
                || metadata.value().state != VaultState::Active
            {
                return Err(error("revision_conflict"));
            }
            self.retire_reviewed_pending(&mut runtime, None).await?;
            let metadata = self.metadata(&runtime)?;
            let connection = runtime
                .connection
                .as_ref()
                .ok_or_else(|| error("sign_in_required"))?;
            let operation = connection
                .transport
                .prepare_delete(
                    &connection.session,
                    metadata,
                    UuidV4::random().map_err(protocol_error)?,
                )
                .map_err(protocol_error)?;
            let pending = Pending {
                record: String::from_utf8(operation.to_pending_record().map_err(protocol_error)?)
                    .map_err(|_| error("invalid_response"))?,
                operation_id: operation.operation_id().as_str().into(),
                vault_id: expected_vault_id,
                key_id: None,
                local_generation: self.0.data.generation()?,
                deleted: true,
                remember_key: false,
                key_preference_epoch: runtime.settings.key_preference_epoch,
                base_revision: observed,
                old_key_id: None,
            };
            self.0
                .local
                .write(self.owner(&runtime)?, "pending", pending.clone())
                .await?;
            self.update(|s| s.pending_operation_id = Some(pending.operation_id.clone()));
            self.current_account(&runtime).await?;
            let receipt = match connection
                .transport
                .submit(&connection.session, &operation)
                .await
            {
                Ok(receipt) => receipt,
                Err(e) if definite_rejection(&e) => {
                    self.discard_rejected(&runtime, &pending).await?;
                    return Err(protocol_error(e));
                }
                Err(_) => return Err(error("outcome_unknown")),
            };
            self.accept_receipt(&mut runtime, &pending, &receipt.receipt)
                .await
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn sign_out(&self) -> SyncResult<EncryptedSyncStatus> {
        use crate::domains::MarketplaceApi;
        self.0.auto_suspended.store(true, Ordering::Release);
        self.0.cancelled.store(true, Ordering::Release);
        // Preserve the account API's existing fail-closed guarantee immediately,
        // even while an earlier sync task is draining its native I/O worker.
        self.0.marketplace.invalidate_authentication();
        let mut runtime = self.0.runtime.lock().await;
        runtime.key = None;
        runtime.preview = None;
        runtime.settings.enabled = false;
        runtime.settings.remember_key = false;
        let connection = runtime.connection.take();
        // OAuth logout is independent of encrypted journal/key cleanup. Always
        // attempt it; denied sync-key deletion cannot keep GitHub authorized.
        let logout_result = self
            .0
            .marketplace
            .logout()
            .await
            .map_err(|_| error("secure_storage_denied"));
        let cleanup_result = async {
            self.0.local.set_lockout(true).await?;
            self.initialize(&mut runtime).await?;
            runtime.settings.enabled = false;
            runtime.settings.remember_key = false;
            runtime.settings.key_preference_epoch = runtime
                .settings
                .key_preference_epoch
                .checked_add(1)
                .ok_or_else(|| error("recovery_required"))?;
            self.save_settings(&runtime).await?;
            self.forget_unlock_material(&runtime).await
        }
        .await;
        if let Some(Connection {
            transport,
            session,
            github_token,
            ..
        }) = connection
        {
            drop(github_token);
            // Local sign-out remains possible offline; the discarded session
            // also has a short server-enforced expiry.
            let _ = tokio::time::timeout(Duration::from_secs(5), transport.revoke_session(session))
                .await;
        }
        runtime.settings.enabled = false;
        runtime.settings.remember_key = false;
        runtime.metadata = None;
        runtime.snapshot = None;
        runtime.baseline = None;
        self.update(|s| {
            s.account = None;
            s.auth_state = AuthState::SignedOut;
            s.has_cloud_snapshot = None;
            s.vault_id = None;
            s.key_id = None;
            s.remote_revision = None;
            s.pending_operation_id = None;
        });
        let result = cleanup_result.and(logout_result);
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn begin_sign_in(&self) -> SyncResult<EncryptedSyncSignIn> {
        use crate::domains::MarketplaceApi;
        let flow = self
            .0
            .marketplace
            .start_device_flow()
            .await
            .map_err(|_| error("sign_in_required"))?;
        if flow.verification_uri != "https://github.com/login/device" {
            return Err(error("invalid_response"));
        }
        let expires = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::seconds(
                i64::try_from(flow.expires_in_secs).map_err(|_| error("invalid_response"))?,
            ))
            .ok_or_else(|| error("invalid_response"))?;
        Ok(EncryptedSyncSignIn {
            authorization_session_id: flow.flow_id,
            user_code: flow.user_code,
            verification_uri: flow.verification_uri,
            expires_at: expires.to_rfc3339(),
            interval_seconds: flow.interval_secs,
        })
    }

    async fn forget_unlock_material(&self, runtime: &Runtime) -> SyncResult<()> {
        let Some(owner) = &runtime.settings.owner_id else {
            return Ok(());
        };
        if let (Some(vault), Some(key)) = (&runtime.settings.vault_id, &runtime.settings.key_id) {
            self.0.local.forget(owner, vault, key).await?;
        }
        // An interrupted password rotation can have two remembered keys. Lock
        // and sign-out clear both while retaining the encrypted replay record.
        if let Some(pending) = self.0.local.read::<Pending>(owner, "pending").await? {
            for key in [pending.key_id.as_ref(), pending.old_key_id.as_ref()]
                .into_iter()
                .flatten()
            {
                self.0.local.forget(owner, &pending.vault_id, key).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn poll_sign_in(
        &self,
        session: String,
    ) -> SyncResult<EncryptedSyncSignInResult> {
        use crate::domains::MarketplaceApi;
        use crate::domains::OAuthPollResult;
        match self
            .0
            .marketplace
            .poll_device_flow(session)
            .await
            .map_err(|_| error("sign_in_required"))?
        {
            OAuthPollResult::Authorized { .. } => {
                let (_, account) = self
                    .0
                    .marketplace
                    .sync_identity()
                    .await
                    .map_err(|_| error("sign_in_required"))?;
                Ok(EncryptedSyncSignInResult::SignedIn {
                    account: SyncAccount {
                        github_id: account.github_id.as_str().into(),
                        login: account.login,
                    },
                })
            }
            OAuthPollResult::Pending => Ok(EncryptedSyncSignInResult::Pending { slow_down: false }),
            OAuthPollResult::SlowDown => Ok(EncryptedSyncSignInResult::Pending { slow_down: true }),
            OAuthPollResult::Error { message }
                if message == "OAuth 设备码已过期，请重新发起登录" =>
            {
                Ok(EncryptedSyncSignInResult::Expired)
            }
            OAuthPollResult::Error { message }
                if message.contains("拒绝") || message.contains("取消") =>
            {
                Ok(EncryptedSyncSignInResult::Denied)
            }
            OAuthPollResult::Error { .. } => Err(error("sign_in_required")),
        }
    }

    pub(crate) async fn cancel_sign_in(&self, session: String) -> SyncResult<()> {
        use crate::domains::MarketplaceApi;
        self.0.marketplace.cancel_device_flow(Some(session)).await
    }

    pub(crate) async fn start(&self, spawner: Arc<dyn crate::TaskSpawner>) -> SyncResult<()> {
        if self.0.auto_started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        {
            let mut runtime = self.0.runtime.lock().await;
            let result = self.initialize(&mut runtime).await;
            self.finish(&mut runtime, &result);
            if result.is_err() {
                self.0.auto_started.store(false, Ordering::Release);
            }
            result?;
        }
        let service = self.clone();
        spawner.spawn(Box::pin(async move {
            let mut changes = service.0.data.changes();
            // One startup trigger. Only a bounded local-busy retry may schedule
            // another attempt without a user change; no network polling timer.
            service.auto_sync(true).await;
            loop {
                let mut forced = tokio::select! {
                    changed = changes.changed() => { if changed.is_err() { break; } false },
                    _ = service.0.wake.notified() => true,
                };
                if service.0.shutdown.load(Ordering::Acquire) { break; }
                let first = Instant::now();
                let mut deadline = tokio::time::Instant::now() + Duration::from_millis(750);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep_until(deadline) => break,
                        changed = changes.changed() => {
                            if changed.is_err() { return; }
                            let elapsed = first.elapsed();
                            if elapsed >= Duration::from_secs(5) { break; }
                            deadline = tokio::time::Instant::now() + Duration::from_millis(750).min(Duration::from_secs(5) - elapsed);
                        },
                        _ = service.0.wake.notified() => { if service.0.shutdown.load(Ordering::Acquire) { return; } forced = true; },
                    }
                }
                let change = *changes.borrow_and_update();
                if forced || matches!(change.origin, crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User) { service.auto_sync(false).await; }
            }
        }));
        Ok(())
    }

    async fn auto_sync(&self, startup: bool) {
        // A LocalOnly writer can overlap the debounce/capture boundary without
        // emitting another dirty generation when it finishes. Retain this
        // trigger briefly instead of losing it after SourceChanged. This budget
        // applies only to local contention, never transport/CAS/unknown results.
        let mut delays = [250, 750, 1_500, 3_000].into_iter();
        let mut retry_sequence = None;
        loop {
            let mut runtime = self.0.runtime.lock().await;
            if self.0.shutdown.load(Ordering::Acquire)
                || self.0.auto_suspended.load(Ordering::Acquire)
                || !runtime.settings.enabled
                || runtime.preview.is_some()
            {
                return;
            }
            if retry_sequence.is_some_and(|sequence| {
                self.0.cancelled.load(Ordering::Acquire)
                    || self.0.sequence.load(Ordering::Acquire) != sequence
            }) {
                return;
            }
            if runtime.key.is_none() && !(startup && runtime.settings.remember_key) {
                return;
            }
            if runtime.retry_at.is_some_and(|at| at > Instant::now()) {
                return;
            }
            self.begin();
            let result = self.run_once(&mut runtime).await;
            let local_busy = result.as_ref().err().is_some_and(|failure| {
                failure.code == crate::BackendErrorCode::Busy
                    && matches!(
                        failure.message.as_str(),
                        "sync_documents_source_changed" | "runtime_busy"
                    )
            });
            self.finish(&mut runtime, &result);
            let completed_sequence = self.0.sequence.load(Ordering::Acquire);
            // Pause/lock/manual work must be able to acquire the runtime during
            // the delay. Every retry revalidates the current service state.
            drop(runtime);
            if !local_busy || self.0.cancelled.load(Ordering::Acquire) {
                return;
            }
            let Some(delay) = delays.next() else {
                return;
            };
            // A later manual operation owns its own outcome. In particular an
            // old local retry must never reconcile that operation's unknown PUT.
            retry_sequence = Some(completed_sequence);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.0.shutdown.store(true, Ordering::Release);
        self.0.cancelled.store(true, Ordering::Release);
        self.0.wake.notify_waiters();
        let mut runtime = self.0.runtime.lock().await;
        runtime.key = None;
        runtime.preview = None;
        runtime.connection = None;
    }

    async fn discard_rejected(&self, runtime: &Runtime, pending: &Pending) -> SyncResult<()> {
        let owner = self.owner(runtime)?;
        self.0.local.remove(owner, "pending").await?;
        let _ = self
            .0
            .local
            .remove(owner, &format!("proposal:{}", pending.operation_id))
            .await;
        self.update(|status| status.pending_operation_id = None);
        Ok(())
    }

    async fn reconcile_for_review(&self, runtime: &mut Runtime) -> SyncResult<()> {
        match self.reconcile(runtime).await {
            Err(e) if e.message == "outcome_unknown" => Ok(()),
            result => result,
        }
    }

    async fn reviewable_pending(&self, runtime: &Runtime) -> SyncResult<Option<Pending>> {
        let pending: Option<Pending> = self.0.local.read(self.owner(runtime)?, "pending").await?;
        if let Some(pending) = &pending {
            // Once a newer head is observed, the original If-Match can no
            // longer commit. Until then, only exact replay is safe.
            if self.metadata(runtime)?.value().revision <= pending.base_revision {
                return Err(error("outcome_unknown"));
            }
        }
        Ok(pending)
    }

    async fn retire_reviewed_pending(
        &self,
        runtime: &mut Runtime,
        expected_id: Option<&str>,
    ) -> SyncResult<()> {
        let pending = self.reviewable_pending(runtime).await?;
        if expected_id.is_some() && pending.as_ref().map(|p| p.operation_id.as_str()) != expected_id
        {
            return Err(error("stale_preview"));
        }
        if let Some(pending) = pending {
            self.current_account(runtime).await?;
            let head = self.metadata(runtime)?.value();
            for key_id in [pending.key_id.as_ref(), pending.old_key_id.as_ref()]
                .into_iter()
                .flatten()
            {
                let active = head.vault_id.as_ref().map(UuidV4::as_str)
                    == Some(pending.vault_id.as_str())
                    && head.key_id.as_ref().map(UuidV4::as_str) == Some(key_id.as_str());
                if !active {
                    self.0
                        .local
                        .forget(self.owner(runtime)?, &pending.vault_id, key_id)
                        .await?;
                }
            }
            // Explicit restore/create/delete supersedes the old CAS only after
            // review. Preserve the uncertain operation as protected evidence;
            // never mislabel it failed or committed without its receipt.
            self.0
                .local
                .write(
                    self.owner(runtime)?,
                    &format!("reviewed-uncertain:{}", pending.operation_id),
                    pending.clone(),
                )
                .await?;
            self.0.local.remove(self.owner(runtime)?, "pending").await?;
            self.update(|status| status.pending_operation_id = None);
        }
        Ok(())
    }

    pub(crate) async fn ui_preferences(&self) -> SyncResult<Option<crate::CloudSyncUiPreferences>> {
        Ok(self.ui_preferences_snapshot().await?.preferences)
    }

    pub(crate) async fn ui_preferences_snapshot(
        &self,
    ) -> SyncResult<EncryptedUiPreferencesSnapshot> {
        let Some(envelope) = self.0.local.read_ui().await? else {
            return Ok(EncryptedUiPreferencesSnapshot {
                preferences: None,
                revision: None,
            });
        };
        let preferences = serde_json::from_value(serde_json::json!({ "locale": envelope.value.expose().get("locale"), "fontScale": envelope.value.expose().get("fontScale") }))
            .map_err(|_| error("recovery_required"))?;
        Ok(EncryptedUiPreferencesSnapshot {
            preferences: Some(preferences),
            revision: Some(envelope.revision),
        })
    }

    fn emit_restored(&self, scope: &SyncScope, applied: &ExportedDocuments) {
        let ui_preferences = applied
            .documents
            .documents()
            .documents
            .iter()
            .filter(|d| d.kind == DocumentKind::UiPreferences)
            .filter_map(|d| d.value.as_str().map(|v| (d.id.clone(), v.to_owned())))
            .collect();
        self.0.events.publish(
            None,
            BackendEventKind::CloudSyncRestoreCompleted(EncryptedSyncRestoreEvent {
                sequence: self
                    .0
                    .sequence
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1)
                    .to_string(),
                account_id: scope.owner_github_id.clone(),
                vault_id: scope.vault_id.clone(),
                task_id: self.status().task_id,
                local_generation: applied.generation.as_str().into(),
                ui_preferences,
            }),
        );
    }
}

fn wire_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn same_documents(left: &DocumentSet, right: &DocumentSet) -> bool {
    // Device/time metadata describes an export, not a user edit.
    left.documents == right.documents && left.tombstones == right.tombstones
}

fn definite_rejection(error: &crate::cloud_sync_e2ee_protocol::Error) -> bool {
    // Only for the first submit of a freshly generated operation ID. A retry of
    // an older unknown request cannot use a CAS rejection as proof of failure.
    matches!(
        error,
        crate::cloud_sync_e2ee_protocol::Error::Api {
            status: 400..=499,
            ..
        }
    )
}
