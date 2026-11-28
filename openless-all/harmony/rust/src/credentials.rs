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
    siwc: Arc<openless_core::siwc::SiwcService>,
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
            siwc: crate::siwc::service(),
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
    /// Provider ids that already have at least one non-empty saved `omni.*`
    /// credential. This reads only the hydrated vault map that the ArkTS host
    /// replays from the system Asset keystore: it never consults the provider
    /// catalogue, never performs I/O or network work, and never exposes a secret.
    ///
    /// "Started configuring" is deliberately broader than `omni_configured`:
    /// an endpoint-only or model-only draft still counts, and provider ids with
    /// no current descriptor are retained so a hand-edited or newer channel is
    /// not silently dropped from the list.
    pub fn configured_omni_providers(&self) -> Result<Vec<String>, BackendError> {
        let values = self.values.lock().map_err(failure)?;
        let mut providers: Vec<String> = Vec::new();
        for (key, value) in values.iter() {
            if key.namespace != CredentialNamespace::Omni {
                continue;
            }
            let Some(raw) = key.provider_id.as_deref() else {
                continue;
            };
            let provider = raw.trim();
            if provider.is_empty() || value.expose_secret().trim().is_empty() {
                continue;
            }
            if !providers.iter().any(|seen| seen == provider) {
                providers.push(provider.to_string());
            }
        }
        providers.sort();
        Ok(providers)
    }
}
impl CredentialStore for HarmonyCredentials {
    fn siwc(&self) -> Option<Arc<openless_core::siwc::SiwcService>> {
        Some(self.siwc.clone())
    }

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
            let provider_type = |kind, id: &str| {
                meta.list_channels(kind)
                    .into_iter()
                    .find(|c| c.id == id)
                    .map(|c| c.provider_type)
                    .unwrap_or_else(|| id.into())
            };
            let asr_type = provider_type(ChannelKind::Asr, &asr);
            let llm_type = provider_type(ChannelKind::Llm, &llm);
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
                llm_api_key_required: openless_core::provider_rules::api_key_required(
                    openless_core::ProviderKind::Llm,
                    &llm_type,
                    get(&llm, "ark.endpoint").as_deref(),
                ),
                omni_api_key: has(&omni, "omni.api_key"),
                omni_endpoint: has(&omni, "omni.endpoint"),
                omni_model: has(&omni, "omni.model"),
                ..Default::default()
            };
            let pipeline_mode = openless_core::shared_types::effective_pipeline_mode(
                prefs.multimodal_pipeline_enabled,
                prefs.pipeline_mode,
            );
            let local_asr_runtime = openless_core::provider_rules::provider_descriptor(
                openless_core::ProviderKind::Asr,
                &asr_type,
            )
            .is_some_and(|descriptor| {
                descriptor.auth_requirement == openless_core::provider_rules::AuthRequirement::None
            })
            .then_some(false);
            let asr_configured = openless_core::provider_rules::asr_configured(
                &asr_type,
                &config,
                local_asr_runtime,
            );
            let llm_configured = openless_core::provider_rules::llm_configured(&llm_type, &config);
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

    #[test]
    fn cloud_asr_and_custom_omni_channels_report_configured_while_local_asr_stays_false() {
        let root = std::env::temp_dir().join(format!(
            "openless-harmony-cred-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let host = crate::build_host(root.clone(), root.join("cache"), runtime).unwrap();

        // Cloud ASR with custom channel id (bailian) must report asrConfigured=true.
        let asr_id = crate::dispatch(
            &host,
            "create_channel",
            serde_json::json!({"kind":"asr","providerType":"bailian","name":"My Bailian"}),
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        crate::dispatch(
            &host,
            "set_credential",
            serde_json::json!({"account":"asr.api_key","provider":asr_id,"value":"fixture-asr-key"}),
        )
        .unwrap();
        crate::dispatch(
            &host,
            "set_active_asr_provider",
            serde_json::json!({"provider":asr_id}),
        )
        .unwrap();
        let status = crate::dispatch(&host, "get_credentials", serde_json::json!({})).unwrap();
        assert_eq!(status["asrConfigured"], true);

        // Iflytek ASR requires xfyun.app_id + xfyun.api_key.
        let xfyun_id = crate::dispatch(
            &host,
            "create_channel",
            serde_json::json!({"kind":"asr","providerType":"iflytek","name":"My Xfyun"}),
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        crate::dispatch(
            &host,
            "set_active_asr_provider",
            serde_json::json!({"provider":xfyun_id}),
        )
        .unwrap();
        assert_eq!(
            crate::dispatch(&host, "get_credentials", serde_json::json!({})).unwrap()
                ["asrConfigured"],
            false
        );
        crate::dispatch(
            &host,
            "set_credential",
            serde_json::json!({"account":"xfyun.app_id","provider":xfyun_id,"value":"fixture-app"}),
        )
        .unwrap();
        crate::dispatch(
            &host,
            "set_credential",
            serde_json::json!({"account":"xfyun.api_key","provider":xfyun_id,"value":"fixture-key"}),
        )
        .unwrap();
        assert_eq!(
            crate::dispatch(&host, "get_credentials", serde_json::json!({})).unwrap()
                ["asrConfigured"],
            true
        );

        // Local ASR providers remain unavailable on HarmonyOS.
        let local_id = crate::dispatch(
            &host,
            "create_channel",
            serde_json::json!({"kind":"asr","providerType":"local-whisper","name":"Local"}),
        )
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
        crate::dispatch(
            &host,
            "set_active_asr_provider",
            serde_json::json!({"provider":local_id}),
        )
        .unwrap();
        assert_eq!(
            crate::dispatch(&host, "get_credentials", serde_json::json!({})).unwrap()
                ["asrConfigured"],
            false
        );

        // Omni provider (fixed provider id, not a ChannelKind) reports omniConfigured=true.
        crate::dispatch(
            &host,
            "set_credential",
            serde_json::json!({"account":"omni.api_key","provider":"gemini","value":"fixture-omni-key"}),
        )
        .unwrap();
        crate::dispatch(
            &host,
            "set_active_omni_provider",
            serde_json::json!({"provider":"gemini"}),
        )
        .unwrap();
        assert_eq!(
            crate::dispatch(&host, "get_credentials", serde_json::json!({})).unwrap()
                ["omniConfigured"],
            true
        );

        host.runtime.block_on(host.backend.shutdown()).unwrap();
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn omni_list_store(tag: &str) -> (std::path::PathBuf, HarmonyCredentials) {
        let root = std::env::temp_dir().join(format!(
            "openless-harmony-omni-list-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = HarmonyCredentials::new(root.join("channels.json"));
        (root, store)
    }

    fn omni_key(provider: Option<&str>, account: &str) -> CredentialKey {
        CredentialKey::new(
            CredentialNamespace::Omni,
            provider.map(str::to_owned),
            account,
        )
        .unwrap()
    }

    #[test]
    fn configured_omni_providers_reports_saved_non_empty_omni_ids_only() {
        let (root, store) = omni_list_store("ids");
        store
            .hydrate(
                omni_key(Some("openai"), "omni.api_key"),
                SecretValue::new("fixture-secret-openai"),
            )
            .unwrap();
        // Endpoint-only and model-only drafts still count as started configuration.
        store
            .hydrate(
                omni_key(Some("custom"), "omni.endpoint"),
                SecretValue::new("https://fixture.example/v1"),
            )
            .unwrap();
        store
            .hydrate(
                omni_key(Some("gemini"), "omni.model"),
                SecretValue::new("gemini-fixture"),
            )
            .unwrap();
        // Empty and whitespace-only values are not configured.
        store
            .hydrate(
                omni_key(Some("empty"), "omni.api_key"),
                SecretValue::new(""),
            )
            .unwrap();
        store
            .hydrate(
                omni_key(Some("blank"), "omni.api_key"),
                SecretValue::new("   "),
            )
            .unwrap();
        // Other namespaces must never leak into the omni list.
        store
            .hydrate(
                CredentialKey::new(
                    CredentialNamespace::Asr,
                    Some("volcengine".into()),
                    "asr.api_key",
                )
                .unwrap(),
                SecretValue::new("fixture-asr-key"),
            )
            .unwrap();
        store
            .hydrate(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some("openai".into()),
                    "ark.api_key",
                )
                .unwrap(),
                SecretValue::new("fixture-llm-key"),
            )
            .unwrap();
        store
            .hydrate(
                CredentialKey::new(
                    CredentialNamespace::Marketplace,
                    Some("openai".into()),
                    openless_core::MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("fixture-github-token"),
            )
            .unwrap();

        let ids = store.configured_omni_providers().unwrap();
        assert_eq!(
            ids,
            vec![
                "custom".to_string(),
                "gemini".to_string(),
                "openai".to_string()
            ],
            "sorted unique ids, one per provider with a saved omni field"
        );
        let wire = serde_json::to_string(&ids).unwrap();
        for secret in [
            "fixture-secret-openai",
            "https://fixture.example/v1",
            "gemini-fixture",
            "fixture-asr-key",
            "fixture-llm-key",
            "fixture-github-token",
        ] {
            assert!(
                !wire.contains(secret),
                "provider list must never carry credential material"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configured_omni_providers_deduplicates_fields_and_keeps_unknown_ids() {
        let (root, store) = omni_list_store("dedupe");
        for account in [
            "omni.api_key",
            "omni.endpoint",
            "omni.model",
            "omni.temperature",
        ] {
            store
                .hydrate(
                    omni_key(Some("my-private-omni"), account),
                    SecretValue::new("fixture"),
                )
                .unwrap();
        }
        assert_eq!(
            store.configured_omni_providers().unwrap(),
            vec!["my-private-omni".to_string()],
            "a provider without a current descriptor survives and appears once"
        );

        let (empty_root, empty_store) = omni_list_store("empty");
        assert!(
            empty_store.configured_omni_providers().unwrap().is_empty(),
            "no saved omni field is an empty list, not an error"
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(empty_root).unwrap();
    }

    #[test]
    fn configured_omni_providers_resolve_legacy_unscoped_assets_to_active_provider() {
        let (root, store) = omni_list_store("legacy");
        // A pre-provider-scoped Asset record is bound to the active omni provider
        // when the host hydrates the vault, exactly like the runtime read path.
        let metadata = CredentialMetadata::from_parts(vec![], vec![], "", "", "dashscope-omni", 1);
        std::fs::write(
            root.join("channels.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        store
            .hydrate(
                omni_key(None, "omni.api_key"),
                SecretValue::new("fixture-legacy"),
            )
            .unwrap();
        assert_eq!(
            store.configured_omni_providers().unwrap(),
            vec!["dashscope-omni".to_string()]
        );

        // Without an active omni provider there is no identity to report; the read
        // must stay empty instead of fabricating a provider id.
        let (orphan_root, orphan_store) = omni_list_store("orphan");
        orphan_store
            .hydrate(
                omni_key(None, "omni.api_key"),
                SecretValue::new("fixture-orphan"),
            )
            .unwrap();
        assert!(orphan_store.configured_omni_providers().unwrap().is_empty());
        std::fs::remove_dir_all(orphan_root).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_configured_omni_providers_command_returns_ids_without_secrets() {
        let root = std::env::temp_dir().join(format!(
            "openless-harmony-omni-list-command-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let host = crate::build_host(root.clone(), root.join("cache"), runtime).unwrap();
        for (provider, account, value) in [
            ("openai", "omni.api_key", "fixture-openai-key"),
            ("custom", "omni.endpoint", "https://fixture.example/v1"),
            ("empty", "omni.api_key", ""),
        ] {
            crate::dispatch(
                &host,
                "set_credential",
                serde_json::json!({
                    "account": account,
                    "provider": provider,
                    "value": value
                }),
            )
            .unwrap();
        }
        let listed = crate::dispatch(
            &host,
            "list_configured_omni_providers",
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(listed, serde_json::json!(["custom", "openai"]));
        let wire = listed.to_string();
        assert!(!wire.contains("fixture-openai-key"));
        assert!(!wire.contains("fixture.example"));

        host.runtime.block_on(host.backend.shutdown()).unwrap();
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }
}
