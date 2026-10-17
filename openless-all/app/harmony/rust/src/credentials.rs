use futures_util::future::BoxFuture;
use openless_core::{
    credentials::*, BackendError, BackendErrorCode, CredentialsStatus, UserPreferences,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn failure(e: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Persistence, e.to_string())
}
type Dirty = Arc<Mutex<HashMap<CredentialKey, Option<SecretValue>>>>;
type Values = Arc<Mutex<HashMap<CredentialKey, SecretValue>>>;
struct DiskMetadata {
    path: PathBuf,
    values: Values,
}
impl CredentialMetadataStore for DiskMetadata {
    fn load_metadata(&self) -> BoxFuture<'static, Result<CredentialMetadata, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            match std::fs::read(path) {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(failure),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok(CredentialMetadata::default())
                }
                Err(e) => Err(failure(e)),
            }
        })
    }
    fn save_metadata(
        &self,
        metadata: CredentialMetadata,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let temporary = path.with_extension("tmp");
            std::fs::write(&temporary, serde_json::to_vec(&metadata).map_err(failure)?)
                .map_err(failure)?;
            std::fs::rename(temporary, path).map_err(failure)
        })
    }
    fn channel_has_secrets(
        &self,
        _kind: ChannelKind,
        id: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>> {
        let values = self.values.clone();
        Box::pin(async move {
            Ok(values.lock().map_err(failure)?.iter().any(|(key, value)| {
                key.provider_id.as_deref() == Some(&id) && !value.expose_secret().is_empty()
            }))
        })
    }
}
pub struct HarmonyCredentials {
    directory: CredentialDirectory,
    metadata: Arc<DiskMetadata>,
    values: Values,
    dirty: Dirty,
}
impl HarmonyCredentials {
    pub fn new(path: PathBuf) -> Self {
        let values = Arc::new(Mutex::new(HashMap::new()));
        let metadata = Arc::new(DiskMetadata {
            path,
            values: values.clone(),
        });
        Self {
            directory: CredentialDirectory::new(metadata.clone()),
            metadata,
            values,
            dirty: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
impl HarmonyCredentials {
    pub fn hydrate(&self, key: CredentialKey, value: SecretValue) -> Result<(), BackendError> {
        self.values.lock().map_err(failure)?.insert(key, value);
        Ok(())
    }
    pub fn dirty_records(&self) -> Result<serde_json::Value, BackendError> {
        Ok(serde_json::Value::Array(self.dirty.lock().map_err(failure)?.iter().map(|(key,value)|serde_json::json!({"account":key.account,"provider":key.provider_id,"value":value.as_ref().map(|v|v.expose_secret())})).collect()))
    }
    pub fn acknowledge_dirty(&self) -> Result<(), BackendError> {
        self.dirty.lock().map_err(failure)?.clear();
        Ok(())
    }
}
impl CredentialStore for HarmonyCredentials {
    fn status(
        &self,
        prefs: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        let metadata = self.metadata.clone();
        let values = self.values.clone();
        Box::pin(async move {
            let meta = metadata.load_metadata().await?;
            let asr = meta.active_provider(ProviderSlot::Asr);
            let llm = meta.active_provider(ProviderSlot::Llm);
            let omni = meta.active_provider(ProviderSlot::Omni);
            let values = values.lock().map_err(failure)?;
            let get = |provider: &str, account: &str| {
                values
                    .iter()
                    .find(|(key, _)| {
                        key.provider_id.as_deref().unwrap_or("") == provider
                            && key.account == account
                    })
                    .map(|(_, v)| v.expose_secret().to_owned())
            };
            let has = |provider: &str, account: &str| {
                get(provider, account).is_some_and(|v| !v.trim().is_empty())
            };
            let config = openless_core::provider_rules::CredentialConfiguration {
                asr_api_key: has(&asr, "asr.api_key"),
                asr_endpoint: has(&asr, "asr.endpoint"),
                asr_model: has(&asr, "asr.model"),
                volcengine_service: get(&asr, "volcengine.service"),
                volcengine_auth_mode: get(&asr, "volcengine.auth_mode"),
                volcengine_app_key: has(&asr, "volcengine.app_key"),
                volcengine_access_key: has(&asr, "volcengine.access_key"),
                volcengine_api_key: has(&asr, "volcengine.api_key"),
                volcengine_resource_id: has(&asr, "volcengine.resource_id"),
                xfyun_app_id: has(&asr, "xfyun.app_id"),
                xfyun_api_key: has(&asr, "xfyun.api_key"),
                tencent_cloud_app_id: has(&asr, "tencent_cloud.app_id"),
                tencent_cloud_secret_id: has(&asr, "tencent_cloud.secret_id"),
                tencent_cloud_secret_key: has(&asr, "tencent_cloud.secret_key"),
                llm_api_key: has(&llm, "ark.api_key"),
                llm_endpoint: has(&llm, "ark.endpoint"),
                llm_model: has(&llm, "ark.model_id"),
                llm_api_key_required: true,
                omni_api_key: has(&omni, "omni.api_key"),
                omni_endpoint: has(&omni, "omni.endpoint"),
                omni_model: has(&omni, "omni.model"),
                ..Default::default()
            };
            let provider_type = |kind, id: &str| {
                meta.list_channels(kind)
                    .into_iter()
                    .find(|c| c.id == id)
                    .map(|c| c.provider_type)
                    .unwrap_or_else(|| id.into())
            };
            let pipeline_mode = openless_core::shared_types::effective_pipeline_mode(
                prefs.multimodal_pipeline_enabled,
                prefs.pipeline_mode,
            );
            let asr_configured = openless_core::provider_rules::asr_configured(
                &provider_type(ChannelKind::Asr, &asr),
                &config,
                Some(false),
            );
            let llm_configured = openless_core::provider_rules::llm_configured(
                &provider_type(ChannelKind::Llm, &llm),
                &config,
            );
            Ok(CredentialsStatus {
                active_asr_provider: asr,
                active_llm_provider: llm,
                pipeline_mode,
                asr_configured,
                llm_configured,
                omni_configured: openless_core::provider_rules::omni_configured(&omni, &config),
                volcengine_configured: openless_core::provider_rules::volcengine_configured(
                    &config,
                ),
                ark_configured: llm_configured,
            })
        })
    }
    fn read(
        &self,
        mut key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        let values = self.values.clone();
        let original_key = key.clone();
        let directory = self.directory.clone();
        Box::pin(async move {
            if key.provider_id.is_none()
                && matches!(
                    key.namespace,
                    CredentialNamespace::Asr | CredentialNamespace::Llm | CredentialNamespace::Omni
                )
            {
                key.provider_id = Some(
                    directory
                        .active_provider(match key.namespace {
                            CredentialNamespace::Llm => ProviderSlot::Llm,
                            CredentialNamespace::Omni => ProviderSlot::Omni,
                            _ => ProviderSlot::Asr,
                        })
                        .await?,
                );
            }
            let values = values.lock().map_err(failure)?;
            Ok(values
                .get(&key)
                .or_else(|| values.get(&original_key))
                .cloned())
        })
    }
    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let values = self.values.clone();
        let dirty = self.dirty.clone();
        Box::pin(async move {
            dirty
                .lock()
                .map_err(failure)?
                .insert(key.clone(), Some(value.clone()));
            values.lock().map_err(failure)?.insert(key, value);
            Ok(())
        })
    }
    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        let values = self.values.clone();
        let dirty = self.dirty.clone();
        Box::pin(async move {
            dirty.lock().map_err(failure)?.insert(key.clone(), None);
            values.lock().map_err(failure)?.remove(&key);
            Ok(())
        })
    }
    fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> BoxFuture<'static, Result<Vec<ChannelSummary>, BackendError>> {
        let d = self.directory.clone();
        Box::pin(async move { d.list_channels(kind).await })
    }
    fn mutate_channel(
        &self,
        m: ChannelMutation,
    ) -> BoxFuture<'static, Result<ChannelMutationResult, BackendError>> {
        let d = self.directory.clone();
        let values = self.values.clone();
        let dirty = self.dirty.clone();
        let removed = match &m {
            ChannelMutation::Delete { id, .. } | ChannelMutation::DeleteIfBlank { id, .. } => {
                Some(id.clone())
            }
            _ => None,
        };
        Box::pin(async move {
            let result = d.mutate_channel(m).await?;
            if let Some(id) =
                removed.filter(|_| !matches!(result, ChannelMutationResult::DeletedIfBlank(false)))
            {
                let mut values = values.lock().map_err(failure)?;
                let keys: Vec<_> = values
                    .keys()
                    .filter(|key| key.provider_id.as_deref() == Some(&id))
                    .cloned()
                    .collect();
                let mut dirty = dirty.lock().map_err(failure)?;
                for key in keys {
                    values.remove(&key);
                    dirty.insert(key, None);
                }
            }
            Ok(result)
        })
    }
    fn active_provider(&self, s: ProviderSlot) -> BoxFuture<'static, Result<String, BackendError>> {
        let d = self.directory.clone();
        Box::pin(async move { d.active_provider(s).await })
    }
    fn set_active_provider(
        &self,
        s: ProviderSlot,
        id: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let d = self.directory.clone();
        Box::pin(async move { d.set_active_provider(s, id).await })
    }
}
