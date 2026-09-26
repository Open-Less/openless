use std::{path::Path, sync::Arc};

use futures_util::future::BoxFuture;
use sha2::{Digest, Sha256};

use crate::cloud_sync_e2ee_documents::{
    CryptoJournalProtector, DocumentError, DocumentResult, ExportedDocuments, JournalProtector,
    RestoreContext, SecretJson, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::types::{DocumentSet, Revision, SourceDevice};
use crate::cloud_sync_e2ee_store::{
    extensions::{DeviceExtensionKey, ProtectedExtensionStore},
    gate::SyncChange,
    CoreSyncStore,
};

use super::{document_error, local::LocalStorage, service::SyncServiceData, SyncResult};

pub(crate) fn build(
    config: super::EncryptedSyncConfig,
    data_dir: &Path,
    repositories: crate::BackendRepositories,
    credentials: Arc<dyn crate::CredentialStore>,
    marketplace: Arc<crate::marketplace::MarketplaceService>,
    github_client_id: String,
    events: crate::events::BackendEventPublisher,
) -> SyncResult<(super::EncryptedSyncService, Arc<CoreSyncStore>)> {
    let origin = url::Url::parse(&config.service_origin)
        .map_err(|_| super::error("unsupported_protocol"))?;
    if origin.scheme() != "https"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
        || data_dir.as_os_str().is_empty()
    {
        return Err(super::error("unsupported_protocol"));
    }
    let origin = origin.origin().ascii_serialization();
    let root = data_dir.join("encrypted-sync");
    std::fs::create_dir_all(&root).map_err(|_| super::error("local_storage_unavailable"))?;
    let gate =
        crate::cloud_sync_e2ee_store::gate::open_for_data_dir(data_dir).map_err(document_error)?;
    let device_id = load_device_id(&root)?;
    let device = SourceDevice {
        id: device_id.clone(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        app_version: config.app_version,
    };
    let local = LocalStorage::new(
        root.join("protected"),
        origin.clone(),
        device_id,
        credentials.clone(),
    );
    let store = Arc::new(
        CoreSyncStore::new(
            repositories,
            credentials,
            data_dir.into(),
            device,
            gate,
            Arc::new(local.clone()),
        )
        .map_err(document_error)?,
    );
    let service = super::EncryptedSyncService::new(
        super::SyncServiceConfig {
            origin,
            github_client_id,
        },
        marketplace,
        local,
        store.clone(),
        events,
    );
    Ok((service, store))
}

pub(crate) fn load_device_id(root: &Path) -> SyncResult<String> {
    let device_path = root.join("device-id");
    let device_id = match std::fs::read_to_string(&device_path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Never invent another keyring/AAD binding for existing ciphertext.
            for entry in
                std::fs::read_dir(root).map_err(|_| super::error("local_storage_unavailable"))?
            {
                let entry = entry.map_err(|_| super::error("local_storage_unavailable"))?;
                let path = entry.path();
                if path.is_dir()
                    && std::fs::read_dir(&path)
                        .map_err(|_| super::error("local_storage_unavailable"))?
                        .next()
                        .is_some()
                {
                    return Err(super::error("recovery_required"));
                }
            }
            let id = uuid::Uuid::new_v4().to_string();
            if super::local::durable_create(&device_path, id.as_bytes())? {
                id
            } else {
                std::fs::read_to_string(&device_path)
                    .map_err(|_| super::error("local_storage_unavailable"))?
            }
        }
        Err(_) => return Err(super::error("local_storage_unavailable")),
    };
    crate::cloud_sync_e2ee_protocol::types::UuidV4::parse(&device_id)
        .map_err(|_| super::error("recovery_required"))?;
    Ok(device_id)
}

impl ProtectedExtensionStore for LocalStorage {
    fn read_scope(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>> {
        Box::pin(async move {
            self.read(&scope_bucket(&scope)?, "extensions")
                .await
                .map_err(|_| DocumentError::JournalUnavailable)
        })
    }

    fn write_scope(
        &self,
        scope: SyncScope,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            self.write(&scope_bucket(&scope)?, "extensions", value)
                .await
                .map_err(|_| DocumentError::JournalUnavailable)
        })
    }

    fn read_device(
        &self,
        key: DeviceExtensionKey,
    ) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>> {
        Box::pin(async move {
            if key == DeviceExtensionKey::UiPreferences {
                self.read_ui()
                    .await
                    .map(|value| value.map(|envelope| envelope.value))
                    .map_err(|_| DocumentError::JournalUnavailable)
            } else {
                self.read("device", key.storage_name())
                    .await
                    .map_err(|_| DocumentError::JournalUnavailable)
            }
        })
    }

    fn write_device(
        &self,
        key: DeviceExtensionKey,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            if key == DeviceExtensionKey::UiPreferences {
                let envelope = super::local::UiEnvelope {
                    schema_version: 1,
                    revision: uuid::Uuid::new_v4().to_string(),
                    value,
                };
                self.write("device", key.storage_name(), envelope)
                    .await
                    .map_err(|_| DocumentError::JournalUnavailable)
            } else {
                self.write("device", key.storage_name(), value)
                    .await
                    .map_err(|_| DocumentError::JournalUnavailable)
            }
        })
    }

    fn read_ui_revision(&self) -> BoxFuture<'_, DocumentResult<Option<String>>> {
        Box::pin(async move {
            self.read_ui()
                .await
                .map(|value| value.map(|envelope| envelope.revision))
                .map_err(|_| DocumentError::JournalUnavailable)
        })
    }

    fn journal_protector(&self) -> BoxFuture<'_, DocumentResult<Arc<dyn JournalProtector>>> {
        Box::pin(async move {
            let key = self.key().await.map_err(|_| DocumentError::Locked)?;
            Ok(Arc::new(CryptoJournalProtector::new(key)) as Arc<dyn JournalProtector>)
        })
    }
}

fn scope_bucket(scope: &SyncScope) -> DocumentResult<String> {
    // Changing the cloud encryption password keeps the same locally protected
    // baseline/tombstone bucket; account or vault changes never share it.
    let bytes = serde_json::to_vec(&[
        &scope.service_origin,
        &scope.owner_github_id,
        &scope.vault_id,
        &scope.device_id,
    ])
    .map_err(|_| DocumentError::InvalidDocument)?;
    Ok(format!("scope:{:x}", Sha256::digest(bytes)))
}

impl SyncServiceData for CoreSyncStore {
    fn export(&self, scope: SyncScope) -> BoxFuture<'_, SyncResult<ExportedDocuments>> {
        Box::pin(async move { self.export_scope(scope).await.map_err(document_error) })
    }
    fn restore(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
    ) -> BoxFuture<'_, SyncResult<ExportedDocuments>> {
        Box::pin(async move {
            self.restore_scope(desired, context)
                .await
                .map_err(document_error)
        })
    }
    fn recover(&self) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move { self.recover_registered().await.map_err(document_error) })
    }
    fn baseline(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move {
            self.record_baseline(scope, documents, revision)
                .await
                .map_err(document_error)
        })
    }
    fn generation(&self) -> SyncResult<Revision> {
        CoreSyncStore::generation(self).map_err(document_error)
    }
    fn device(&self) -> SourceDevice {
        CoreSyncStore::device(self)
    }
    fn changes(&self) -> tokio::sync::watch::Receiver<SyncChange> {
        CoreSyncStore::changes(self)
    }
}
