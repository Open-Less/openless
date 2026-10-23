//! Narrow Host injection seam for opt-in limited domain data. Deliberately has
//! no upload method: the existing remote protocol exposes a singleton full vault.

use crate::cloud_sync_e2ee_documents::{
    AllowedSyncScope, ConfirmedVocabularySelection, DocumentError, DocumentResult,
    LimitedSyncDocuments, RestoreContext, Revision, SyncScope,
};
use crate::cloud_sync_e2ee_store::CoreSyncStore;
use crate::{marketplace::MarketplaceService, BackendError};
use futures_util::future::BoxFuture;
use std::sync::Arc;

/// Verified immutable numeric identity. Display login and OAuth secrets are not exposed.
/// Obtain a fresh value from MarketplaceService; do not persist it as authorization.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSyncAccount {
    github_id: String,
}
impl VerifiedSyncAccount {
    pub fn github_id(&self) -> &str {
        &self.github_id
    }
}

/// Inject the actual account authority, without constructing/enabling full sync.
/// Implementations must revalidate sign-out/account switch on every call; never use a
/// cached display login. Hosts normally inject their existing MarketplaceService.
pub trait SyncAccountProvider: Send + Sync {
    fn verified_sync_account(&self) -> BoxFuture<'_, Result<VerifiedSyncAccount, BackendError>>;
}
impl MarketplaceService {
    pub async fn verified_sync_account(&self) -> Result<VerifiedSyncAccount, BackendError> {
        let (_, account) = self.sync_identity().await?;
        Ok(VerifiedSyncAccount {
            github_id: account.github_id.as_str().into(),
        })
    }
}
impl SyncAccountProvider for MarketplaceService {
    fn verified_sync_account(&self) -> BoxFuture<'_, Result<VerifiedSyncAccount, BackendError>> {
        Box::pin(MarketplaceService::verified_sync_account(self))
    }
}

#[derive(Debug, Clone)]
pub struct LimitedSyncExport {
    pub generation: Revision,
    pub documents: LimitedSyncDocuments,
}

#[derive(Clone)]
pub struct LimitedSyncStore {
    store: CoreSyncStore,
    accounts: Arc<dyn SyncAccountProvider>,
}
impl LimitedSyncStore {
    /// Compose the actual Core protected local store without constructing or enabling the
    /// full-snapshot service. Choose one policy per repository instance: its activity/vault
    /// gate binds one device identity. The limited root has a separate durable device ID,
    /// wrapping-key AAD, extension buckets and journal identity from desktop full sync.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        config: super::EncryptedSyncConfig,
        data_dir: &std::path::Path,
        repositories: crate::BackendRepositories,
        credentials: Arc<dyn crate::CredentialStore>,
        tasks: Arc<dyn crate::TaskSpawner>,
        allowed: AllowedSyncScope,
        accounts: Arc<dyn SyncAccountProvider>,
    ) -> Result<Self, BackendError> {
        if allowed != AllowedSyncScope::ConfirmedVocabularyModeStyleReferences {
            return Err(super::error("unsupported_protocol"));
        }
        let (_, _, store) = super::adapter::build_store(
            config,
            data_dir,
            repositories,
            credentials,
            tasks,
            super::LIMITED_LOCAL_ROOT,
        )?;
        Self::new((*store).clone(), allowed, accounts).map_err(super::document_error)
    }

    /// Inject an already composed, dedicated limited-policy store. Do not reuse a full
    /// service's local device identity; use `build` for the standard protected composition.
    pub fn new(
        store: CoreSyncStore,
        allowed: AllowedSyncScope,
        accounts: Arc<dyn SyncAccountProvider>,
    ) -> DocumentResult<Self> {
        if allowed != AllowedSyncScope::ConfirmedVocabularyModeStyleReferences {
            return Err(DocumentError::Unsupported);
        }
        Ok(Self { store, accounts })
    }

    pub fn generation(&self) -> DocumentResult<Revision> {
        self.store.generation()
    }
    pub fn device(&self) -> crate::cloud_sync_e2ee_documents::SourceDevice {
        self.store.device()
    }
    pub fn bind_runtime_idle_probe(
        &self,
        probe: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> DocumentResult<()> {
        self.store.bind_runtime_idle_probe(probe)
    }
    pub fn bind_runtime_effects(
        &self,
        effects: Arc<dyn crate::config::RestoreRuntimeEffects>,
    ) -> DocumentResult<()> {
        self.store.bind_runtime_effects(effects)
    }
    pub async fn recover_registered(&self) -> DocumentResult<()> {
        self.store.recover_registered().await
    }
    pub fn is_recovering(&self) -> bool {
        self.store.is_recovering()
    }

    async fn check_account(&self, scope: &SyncScope) -> DocumentResult<()> {
        let account = self
            .accounts
            .verified_sync_account()
            .await
            .map_err(|_| DocumentError::Locked)?;
        if account.github_id() != scope.owner_github_id {
            return Err(DocumentError::InvalidReference);
        }
        Ok(())
    }

    /// Coherent local capture, projected before being returned to Host. No sync-vault I/O;
    /// the account provider may perform authentication I/O. Capture still requires real
    /// credential gate/batch hooks, never an empty-vault fallback.
    pub async fn export(
        &self,
        scope: SyncScope,
        confirmed: ConfirmedVocabularySelection,
    ) -> DocumentResult<LimitedSyncExport> {
        self.check_account(&scope).await?;
        let full = self.store.export_scope(scope.clone()).await?;
        self.check_account(&scope).await?;
        Ok(LimitedSyncExport {
            generation: full.generation,
            documents: LimitedSyncDocuments::project(&full, &scope, &confirmed)?,
        })
    }

    /// Explicit restore/merge result, not last-writer-wins. Caller supplies the generation
    /// reviewed in its preview; all later local mutations cause StalePreview. Existing Core
    /// exclusive gate, encrypted before/after journal, recovery and rollback remain authoritative.
    pub async fn restore(
        &self,
        desired: LimitedSyncDocuments,
        context: RestoreContext,
        confirmed: ConfirmedVocabularySelection,
    ) -> DocumentResult<LimitedSyncExport> {
        self.check_account(&context.scope).await?;
        let full = self.store.export_scope(context.scope.clone()).await?;
        if full.generation != context.local_generation {
            return Err(DocumentError::StalePreview);
        }
        let overlaid =
            desired.overlay(&full, &context.scope, context.observed_revision, &confirmed)?;
        self.check_account(&context.scope).await?;
        let scope = context.scope.clone();
        let restored = self.store.restore_scope(overlaid, context).await?;
        Ok(LimitedSyncExport {
            generation: restored.generation,
            documents: desired.project_restored(&restored, &scope)?,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) struct TestAccount(pub(crate) &'static str);
    impl SyncAccountProvider for TestAccount {
        fn verified_sync_account(
            &self,
        ) -> BoxFuture<'_, Result<VerifiedSyncAccount, BackendError>> {
            Box::pin(async {
                Ok(VerifiedSyncAccount {
                    github_id: self.0.into(),
                })
            })
        }
    }
}
