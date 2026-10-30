//! iOS generic-password vault. The entire document (including directory metadata)
//! lives in SecItem, never in the preferences/data directory.
use super::*;
use openless_core::credentials::*;
use openless_core::provider_rules::{self, CredentialConfiguration};
use serde::Serialize;

pub(crate) trait VaultStorage: Send + Sync {
    fn load(&self) -> Result<Option<Vec<u8>>, BackendError>;
    fn save(&self, bytes: &[u8]) -> Result<(), BackendError>;
}

#[cfg(not(test))]
struct KeychainStorage {
    account: String,
}
#[cfg(not(test))]
impl VaultStorage for KeychainStorage {
    fn load(&self) -> Result<Option<Vec<u8>>, BackendError> {
        match security_framework::passwords::get_generic_password(
            "app.openless.ios.corehost.credentials.v1",
            &self.account,
        ) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.code() == -25300 => Ok(None), // errSecItemNotFound only
            Err(_) => Err(error(
                BackendErrorCode::Platform,
                "credential Keychain read failed",
            )),
        }
    }
    fn save(&self, bytes: &[u8]) -> Result<(), BackendError> {
        security_framework::passwords::set_generic_password(
            "app.openless.ios.corehost.credentials.v1",
            &self.account,
            bytes,
        )
        .map_err(|_| {
            error(
                BackendErrorCode::Platform,
                "credential Keychain write failed",
            )
        })
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct VaultDocument {
    metadata: CredentialMetadata,
    values: Vec<(CredentialKey, String)>,
}
impl Drop for VaultDocument {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for (_, value) in &mut self.values {
            value.zeroize();
        }
    }
}
struct Vault {
    storage: Arc<dyn VaultStorage>,
    document: Mutex<VaultDocument>,
}
impl Vault {
    fn update<T>(
        &self,
        change: impl FnOnce(&mut VaultDocument) -> Result<T, BackendError>,
    ) -> Result<T, BackendError> {
        let mut document = self.document.lock().unwrap();
        let mut next = document.clone();
        let result = change(&mut next)?;
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&next)
                .map_err(|_| error(BackendErrorCode::Internal, "invalid vault document"))?,
        );
        self.storage.save(&bytes)?;
        *document = next;
        Ok(result)
    }
}
impl CredentialMetadataStore for Vault {
    fn load_metadata(&self) -> BoxFuture<'static, Result<CredentialMetadata, BackendError>> {
        let metadata = self.document.lock().unwrap().metadata.clone();
        Box::pin(async move { Ok(metadata) })
    }
    fn save_metadata(
        &self,
        metadata: CredentialMetadata,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.update(|doc| {
            doc.metadata = metadata;
            Ok(())
        });
        Box::pin(async move { result })
    }
    fn channel_has_secrets(
        &self,
        _kind: ChannelKind,
        id: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>> {
        let found = self
            .document
            .lock()
            .unwrap()
            .values
            .iter()
            .any(|(key, _)| key.provider_id.as_deref() == Some(&id));
        Box::pin(async move { Ok(found) })
    }
}

pub(crate) struct IosCredentialStore {
    vault: Arc<Vault>,
    directory: CredentialDirectory,
}
impl IosCredentialStore {
    #[cfg(not(test))]
    pub fn keychain(developer_mode: bool) -> Result<Self, BackendError> {
        // App sandbox paths can change on reinstall. The OS already scopes
        // this item to the app; mode names are stable across app reloads.
        let account = if developer_mode { "developer" } else { "standard" }.into();
        Self::open(Arc::new(KeychainStorage { account }))
    }
    pub(crate) fn open(storage: Arc<dyn VaultStorage>) -> Result<Self, BackendError> {
        let bytes = zeroize::Zeroizing::new(storage.load()?.unwrap_or_default());
        let document = if bytes.is_empty() {
            VaultDocument::default()
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|_| error(BackendErrorCode::Platform, "credential vault is corrupt"))?
        };
        let vault = Arc::new(Vault {
            storage,
            document: Mutex::new(document),
        });
        Ok(Self {
            directory: CredentialDirectory::new(vault.clone()),
            vault,
        })
    }
}
impl CredentialStore for IosCredentialStore {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        let doc = self.vault.document.lock().unwrap();
        let asr = doc.metadata.active_provider(ProviderSlot::Asr);
        let llm = doc.metadata.active_provider(ProviderSlot::Llm);
        let omni = doc.metadata.active_provider(ProviderSlot::Omni);
        let provider_type = |kind, id: &str| {
            doc.metadata
                .list_channels(kind)
                .into_iter()
                .find(|c| c.id == id)
                .map(|c| c.provider_type)
                .unwrap_or_else(|| id.into())
        };
        let asr_type = provider_type(ChannelKind::Asr, &asr);
        let llm_type = provider_type(ChannelKind::Llm, &llm);
        let read = |ns, id: &str, account: &str| {
            doc.values
                .iter()
                .find(|(k, _)| {
                    k.namespace == ns
                        && k.provider_id.as_deref() == Some(id)
                        && k.account == account
                })
                .map(|(_, v)| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let has = |ns, id: &str, account: &str| read(ns, id, account).is_some();
        let a = CredentialNamespace::Asr;
        let l = CredentialNamespace::Llm;
        let o = CredentialNamespace::Omni;
        let configuration = CredentialConfiguration {
            asr_api_key: has(a, &asr, ASR_API_KEY_ACCOUNT),
            asr_endpoint: has(a, &asr, ASR_ENDPOINT_ACCOUNT),
            asr_model: has(a, &asr, ASR_MODEL_ACCOUNT),
            volcengine_service: read(a, &asr, VOLCENGINE_SERVICE_ACCOUNT),
            volcengine_auth_mode: read(a, &asr, VOLCENGINE_AUTH_MODE_ACCOUNT),
            volcengine_app_key: has(a, &asr, VOLCENGINE_APP_KEY_ACCOUNT),
            volcengine_access_key: has(a, &asr, VOLCENGINE_ACCESS_KEY_ACCOUNT),
            volcengine_api_key: has(a, &asr, VOLCENGINE_API_KEY_ACCOUNT),
            volcengine_resource_id: has(a, &asr, VOLCENGINE_RESOURCE_ID_ACCOUNT),
            xfyun_app_id: has(a, &asr, XFYUN_APP_ID_ACCOUNT),
            xfyun_api_key: has(a, &asr, XFYUN_API_KEY_ACCOUNT),
            tencent_cloud_app_id: has(a, &asr, TENCENT_CLOUD_APP_ID_ACCOUNT),
            tencent_cloud_secret_id: has(a, &asr, TENCENT_CLOUD_SECRET_ID_ACCOUNT),
            tencent_cloud_secret_key: has(a, &asr, TENCENT_CLOUD_SECRET_KEY_ACCOUNT),
            llm_api_key: has(l, &llm, LLM_API_KEY_ACCOUNT),
            llm_endpoint: has(l, &llm, LLM_ENDPOINT_ACCOUNT),
            llm_model: has(l, &llm, LLM_MODEL_ACCOUNT),
            llm_api_key_required: provider_rules::api_key_required(
                openless_core::domains::ProviderKind::Llm,
                &llm_type,
                read(l, &llm, LLM_ENDPOINT_ACCOUNT).as_deref(),
            ),
            omni_api_key: has(o, &omni, OMNI_API_KEY_ACCOUNT),
            omni_endpoint: has(o, &omni, OMNI_ENDPOINT_ACCOUNT),
            omni_model: has(o, &omni, OMNI_MODEL_ACCOUNT),
            ..Default::default()
        };
        let pipeline_mode = openless_core::shared_types::effective_pipeline_mode(
            preferences.multimodal_pipeline_enabled,
            preferences.pipeline_mode,
        );
        let status = CredentialsStatus {
            active_asr_provider: asr,
            active_llm_provider: llm,
            asr_configured: provider_rules::asr_configured(
                &asr_type,
                &configuration,
                (asr_type == "ios-sensevoice").then_some(true),
            ),
            llm_configured: provider_rules::llm_configured(&llm_type, &configuration),
            omni_configured: pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal
                && provider_rules::omni_configured(&omni, &configuration),
            pipeline_mode,
            ..Default::default()
        };
        Box::pin(async move { Ok(status) })
    }
    fn read(
        &self,
        mut key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        let doc = self.vault.document.lock().unwrap();
        // Core's Omni context asks for an unscoped model; engines/services ask
        // for a provider-scoped key. Resolve both through the same active slot.
        if key.provider_id.is_none() {
            let slot = match key.namespace {
                CredentialNamespace::Asr => Some(ProviderSlot::Asr),
                CredentialNamespace::Llm => Some(ProviderSlot::Llm),
                CredentialNamespace::Omni => Some(ProviderSlot::Omni),
                _ => None,
            };
            if let Some(slot) = slot {
                key.provider_id = Some(doc.metadata.active_provider(slot));
            }
        }
        let value = doc
            .values
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| SecretValue::new(v.clone()));
        Box::pin(async move { Ok(value) })
    }
    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.vault.update(|doc| {
            doc.values.retain(|(k, _)| *k != key);
            doc.values.push((key, value.into_exposed()));
            Ok(())
        });
        Box::pin(async move { result })
    }
    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.vault.update(|doc| {
            doc.values.retain(|(k, _)| *k != key);
            Ok(())
        });
        Box::pin(async move { result })
    }
    fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> BoxFuture<'static, Result<Vec<ChannelSummary>, BackendError>> {
        let directory = self.directory.clone();
        Box::pin(async move { directory.list_channels(kind).await })
    }
    fn mutate_channel(
        &self,
        mutation: ChannelMutation,
    ) -> BoxFuture<'static, Result<ChannelMutationResult, BackendError>> {
        let directory = self.directory.clone();
        Box::pin(async move { directory.mutate_channel(mutation).await })
    }
    fn active_provider(
        &self,
        slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let directory = self.directory.clone();
        Box::pin(async move { directory.active_provider(slot).await })
    }
    fn set_active_provider(
        &self,
        slot: ProviderSlot,
        id: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let directory = self.directory.clone();
        Box::pin(async move { directory.set_active_provider(slot, id).await })
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryVault(pub Mutex<Option<Vec<u8>>>);
#[cfg(test)]
impl VaultStorage for MemoryVault {
    fn load(&self) -> Result<Option<Vec<u8>>, BackendError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, bytes: &[u8]) -> Result<(), BackendError> {
        *self.0.lock().unwrap() = Some(bytes.to_vec());
        Ok(())
    }
}
