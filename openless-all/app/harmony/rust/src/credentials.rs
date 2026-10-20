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
fn read_metadata(path: &std::path::Path) -> Result<CredentialMetadata, BackendError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(failure),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CredentialMetadata::default()),
        Err(e) => Err(failure(e)),
    }
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
        Box::pin(async move { read_metadata(&path) })
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
    pub fn hydrate(&self, mut key: CredentialKey, value: SecretValue) -> Result<(), BackendError> {
        let legacy_omni = key.namespace == CredentialNamespace::Omni && key.provider_id.is_none();
        if legacy_omni {
            key.provider_id =
                Some(read_metadata(&self.metadata.path)?.active_provider(ProviderSlot::Omni));
        }
        let mut values = self.values.lock().map_err(failure)?;
        if legacy_omni {
            // A provider-scoped Asset entry always wins over a legacy alias,
            // independent of the order returned by the secure vault.
            values.entry(key).or_insert(value);
        } else {
            values.insert(key, value);
        }
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
        mut key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let values = self.values.clone();
        let dirty = self.dirty.clone();
        let directory = self.directory.clone();
        Box::pin(async move {
            let legacy = key.clone();
            if key.namespace == CredentialNamespace::Omni && key.provider_id.is_none() {
                key.provider_id = Some(directory.active_provider(ProviderSlot::Omni).await?);
            }
            let mut writes = dirty.lock().map_err(failure)?;
            if key != legacy {
                writes.insert(legacy, None);
            }
            writes.insert(key.clone(), Some(value.clone()));
            drop(writes);
            values.lock().map_err(failure)?.insert(key, value);
            Ok(())
        })
    }
    fn remove(&self, mut key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        let values = self.values.clone();
        let dirty = self.dirty.clone();
        let directory = self.directory.clone();
        Box::pin(async move {
            let legacy = key.clone();
            if key.namespace == CredentialNamespace::Omni && key.provider_id.is_none() {
                key.provider_id = Some(directory.active_provider(ProviderSlot::Omni).await?);
            }
            let mut writes = dirty.lock().map_err(failure)?;
            if key != legacy {
                writes.insert(legacy, None);
            }
            writes.insert(key.clone(), None);
            drop(writes);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omni_recording_accepts_restored_settings_credentials_without_a_network_request() {
        let root = std::env::temp_dir().join(format!(
            "openless-harmony-omni-recording-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let host = crate::build_host(root.clone(), root.join("cache"), runtime).unwrap();
        crate::dispatch(
            &host,
            "set_active_omni_provider",
            serde_json::json!({"provider":"dashscope-omni"}),
        )
        .unwrap();
        crate::dispatch(
            &host,
            "__hydrate_credential",
            serde_json::json!({"account":"omni.api_key","value":"fixture-only-not-a-real-key"}),
        )
        .unwrap();
        let mut prefs = host.backend.get_preferences();
        prefs.multimodal_pipeline_enabled = true;
        prefs.pipeline_mode = openless_core::shared_types::PipelineMode::Multimodal;
        {
            let _entered = host.runtime.enter();
            host.backend
                .update_settings(
                    prefs,
                    openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                    &openless_core::settings::NoopSettingsRuntime,
                )
                .unwrap();
        }
        let session = crate::dispatch(&host, "start_dictation", serde_json::json!({})).unwrap();
        let id = serde_json::from_value(session.clone()).unwrap();
        host.backend.feed_external_pcm(id, &[1, 0, 2, 0]).unwrap();
        crate::dispatch(
            &host,
            "cancel_dictation",
            serde_json::json!({"session":session}),
        )
        .unwrap();
        host.runtime.block_on(host.backend.shutdown()).unwrap();
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn omni_unscoped_settings_bind_to_active_provider_and_restore_legacy_assets() {
        let root = std::env::temp_dir().join(format!(
            "openless-harmony-omni-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let store = HarmonyCredentials::new(root.join("channels.json"));
            store
                .set_active_provider(ProviderSlot::Omni, "dashscope-omni".into())
                .await
                .unwrap();
            let unscoped =
                CredentialKey::new(CredentialNamespace::Omni, None, "omni.api_key").unwrap();
            let scoped = CredentialKey::new(
                CredentialNamespace::Omni,
                Some("dashscope-omni".into()),
                "omni.api_key",
            )
            .unwrap();
            assert!(
                !store
                    .status(UserPreferences::default())
                    .await
                    .unwrap()
                    .omni_configured
            );
            store
                .write(unscoped.clone(), SecretValue::new("fixture-current"))
                .await
                .unwrap();
            assert!(
                store
                    .status(UserPreferences::default())
                    .await
                    .unwrap()
                    .omni_configured
            );
            assert_eq!(
                store
                    .read(scoped.clone())
                    .await
                    .unwrap()
                    .unwrap()
                    .expose_secret(),
                "fixture-current"
            );
            let dirty = store.dirty_records().unwrap();
            assert!(dirty
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["provider"] == "dashscope-omni" && row["value"].is_string()));
            assert!(dirty
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["provider"].is_null() && row["value"].is_null()));
            for legacy_first in [true, false] {
                let restored = HarmonyCredentials::new(root.join("channels.json"));
                if legacy_first {
                    restored
                        .hydrate(unscoped.clone(), SecretValue::new("fixture-legacy"))
                        .unwrap();
                }
                restored
                    .hydrate(scoped.clone(), SecretValue::new("fixture-current"))
                    .unwrap();
                if !legacy_first {
                    restored
                        .hydrate(unscoped.clone(), SecretValue::new("fixture-legacy"))
                        .unwrap();
                }
                assert!(
                    restored
                        .status(UserPreferences::default())
                        .await
                        .unwrap()
                        .omni_configured
                );
                assert_eq!(
                    restored
                        .read(scoped.clone())
                        .await
                        .unwrap()
                        .unwrap()
                        .expose_secret(),
                    "fixture-current"
                );
            }
            let legacy = HarmonyCredentials::new(root.join("channels.json"));
            legacy
                .hydrate(unscoped.clone(), SecretValue::new("fixture-legacy"))
                .unwrap();
            assert!(
                legacy
                    .status(UserPreferences::default())
                    .await
                    .unwrap()
                    .omni_configured
            );
            assert!(legacy.read(scoped.clone()).await.unwrap().is_some());
            store.remove(unscoped.clone()).await.unwrap();
            assert!(store.read(scoped).await.unwrap().is_none());
            assert!(
                !store
                    .status(UserPreferences::default())
                    .await
                    .unwrap()
                    .omni_configured
            );
            store
                .set_active_provider(ProviderSlot::Omni, "gemini".into())
                .await
                .unwrap();
            assert!(store.read(unscoped).await.unwrap().is_none());
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
