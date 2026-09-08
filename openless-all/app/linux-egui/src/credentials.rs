use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::credentials::{
    ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT, LLM_API_KEY_ACCOUNT,
    LLM_ENDPOINT_ACCOUNT, LLM_MODEL_ACCOUNT, OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT,
    OMNI_MODEL_ACCOUNT, VOLCENGINE_ACCESS_KEY_ACCOUNT, VOLCENGINE_API_KEY_ACCOUNT,
    VOLCENGINE_APP_KEY_ACCOUNT, VOLCENGINE_AUTH_MODE_ACCOUNT, VOLCENGINE_RESOURCE_ID_ACCOUNT,
    XFYUN_API_KEY_ACCOUNT, XFYUN_APP_ID_ACCOUNT,
};
#[cfg(any(target_os = "linux", test))]
use openless_core::credentials_legacy::LegacyCredentials;
use openless_core::{
    BackendError, BackendErrorCode, ChannelKind, ChannelMutation, ChannelMutationResult,
    ChannelSummary, CredentialKey, CredentialMetadata, CredentialNamespace, CredentialStore,
    CredentialsStatus, ProviderSlot, SecretValue, UserPreferences,
};
use serde::{Deserialize, Serialize};

const METADATA_VERSION: u32 = 1;
#[cfg(target_os = "linux")]
const KEYRING_SERVICE: &str = "top.openless.linux";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCredentialMetadata {
    version: u32,
    #[serde(default)]
    metadata: CredentialMetadata,
    #[serde(default)]
    keys: Vec<CredentialKey>,
    #[serde(default)]
    legacy_migrated: bool,
}

/// Linux credential adapter: secrets live in Secret Service/keyring, while
/// non-secret channel ordering and the list of configured keys live in the app
/// data directory.  Secret values are never serialized to the metadata file.
#[derive(Clone)]
pub struct LinuxCredentialStore {
    metadata_path: PathBuf,
    state: Arc<Mutex<PersistedCredentialMetadata>>,
}

impl LinuxCredentialStore {
    pub fn open(data_dir: &Path) -> Result<Self, BackendError> {
        if data_dir.as_os_str().is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "Linux credential metadata directory must not be empty",
            ));
        }
        let metadata_path = data_dir.join("credential-metadata.json");
        let state = read_metadata(&metadata_path)?;
        Ok(Self {
            metadata_path,
            state: Arc::new(Mutex::new(state)),
        })
    }

    fn update_metadata<T>(
        &self,
        update: impl FnOnce(&mut PersistedCredentialMetadata) -> Result<T, BackendError>,
    ) -> Result<T, BackendError> {
        let mut state = self
            .state
            .lock()
            .expect("credential metadata lock poisoned");
        let mut next = state.clone();
        let result = update(&mut next)?;
        persist_metadata(&self.metadata_path, &next)?;
        *state = next;
        Ok(result)
    }

    pub(crate) fn set_active_provider_immediate(
        &self,
        slot: ProviderSlot,
        provider_id: &str,
    ) -> Result<(), BackendError> {
        let provider_id = provider_id.trim().to_string();
        if provider_id.is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "active provider id must not be blank",
            ));
        }
        self.update_metadata(|state| state.metadata.select_active_provider(slot, provider_id))
    }

    fn mutate_channel_with(
        &self,
        mutation: ChannelMutation,
        mut remove: impl FnMut(&CredentialKey) -> Result<(), BackendError>,
    ) -> Result<ChannelMutationResult, BackendError> {
        self.update_metadata(|state| {
            let removal = match &mutation {
                ChannelMutation::Delete { kind, id }
                | ChannelMutation::DeleteIfBlank { kind, id } => {
                    Some((namespace_for_kind(*kind), id.clone()))
                }
                _ => None,
            };
            let keys = &state.keys;
            let result = state.metadata.apply_channel_mutation(mutation, |id| {
                keys.iter().any(|key| {
                    removal
                        .as_ref()
                        .is_some_and(|(namespace, _)| key.namespace == *namespace)
                        && key.provider_id.as_deref() == Some(id)
                })
            })?;
            if let Some((namespace, id)) = removal {
                if result != ChannelMutationResult::DeletedIfBlank(false) {
                    let belongs_to_channel = |key: &CredentialKey| {
                        key.namespace == namespace
                            && key.provider_id.as_deref() == Some(id.as_str())
                    };
                    // Delete vault entries before publishing the new directory. A
                    // failed deletion keeps the original channel and key list so
                    // retry can finish; already-missing entries are idempotent.
                    // Namespace is part of identity: ASR and LLM may share an ID.
                    for key in state.keys.iter().filter(|key| belongs_to_channel(key)) {
                        remove(key)?;
                    }
                    state.keys.retain(|key| !belongs_to_channel(key));
                }
            }
            Ok(result)
        })
    }

    /// Copy 1.x sources once at production startup. Reading and parsing are
    /// separate from committing, so locked/missing chunks never look like an
    /// empty successful migration. Old sources remain available for rollback.
    pub(crate) fn migrate_legacy(&self, home_dir: &Path) -> Result<(), BackendError> {
        #[cfg(target_os = "linux")]
        self.migrate_legacy_with(
            Some(home_dir),
            read_legacy_secret,
            read_legacy_file,
            read_secret,
            write_secret,
        )?;
        #[cfg(not(target_os = "linux"))]
        let _ = home_dir;
        Ok(())
    }

    #[cfg(any(target_os = "linux", test))]
    fn migrate_legacy_with(
        &self,
        home_dir: Option<&Path>,
        mut legacy_read: impl FnMut(&str) -> Result<Option<SecretValue>, BackendError>,
        legacy_file: impl FnOnce(&Path) -> Result<Option<SecretValue>, BackendError>,
        read: impl FnMut(&CredentialKey) -> Result<Option<SecretValue>, BackendError>,
        write: impl FnMut(&CredentialKey, &SecretValue) -> Result<(), BackendError>,
    ) -> Result<(), BackendError> {
        use openless_core::credentials_legacy::{
            decode_legacy_credentials, read_legacy_accounts, read_legacy_vault_payload,
        };
        // Resolve identity from the host's explicit configuration, never from
        // ambient HOME. In particular, an integration test's temporary data_dir
        // does not grant access to the signed-in user's old credential sources.
        let Some(home_dir) = home_dir else {
            return Ok(());
        };
        if self
            .state
            .lock()
            .expect("credential metadata lock poisoned")
            .legacy_migrated
        {
            return Ok(());
        }
        let source = if let Some(payload) = read_legacy_vault_payload(&mut legacy_read)? {
            Some(decode_legacy_credentials(payload.expose_secret())?)
        } else if let Some(payload) = legacy_file(home_dir)? {
            Some(decode_legacy_credentials(payload.expose_secret())?)
        } else {
            read_legacy_accounts(legacy_read)?
        };
        if let Some(source) = source {
            self.import_legacy_with(source, read, write)?;
        }
        Ok(())
    }

    #[cfg(any(target_os = "linux", test))]
    fn import_legacy_with(
        &self,
        source: LegacyCredentials,
        mut read: impl FnMut(&CredentialKey) -> Result<Option<SecretValue>, BackendError>,
        mut write: impl FnMut(&CredentialKey, &SecretValue) -> Result<(), BackendError>,
    ) -> Result<(), BackendError> {
        self.update_metadata(|state| {
            if state.legacy_migrated {
                return Ok(());
            }
            let mut asr = state.metadata.list_channels(ChannelKind::Asr);
            let mut llm = state.metadata.list_channels(ChannelKind::Llm);
            let occupied = |key: &CredentialKey| {
                let Some(id) = key.provider_id.as_deref() else {
                    return state.keys.contains(key);
                };
                let managed = match key.namespace {
                    CredentialNamespace::Asr => asr.iter().any(|channel| channel.id == id),
                    CredentialNamespace::Llm => llm.iter().any(|channel| channel.id == id),
                    CredentialNamespace::Omni => {
                        state.metadata.active_provider(ProviderSlot::Omni) == id
                    }
                    _ => false,
                };
                managed
                    || state.keys.iter().any(|candidate| {
                        candidate.namespace == key.namespace
                            && candidate.provider_id == key.provider_id
                    })
            };
            let imported_keys: Vec<_> = source
                .secrets
                .into_iter()
                .filter(|(key, _)| !occupied(key))
                .collect();
            // Hold the metadata lock through vault writes. Settings mutations
            // cannot race the import and replace a newer user choice. On partial
            // failure the marker/key index stay uncommitted; a retry reads any
            // already-written destination entry instead of overwriting it.
            for (key, value) in imported_keys {
                if read(&key)?.is_none() {
                    write(&key, &value)?;
                }
                state.keys.push(key);
            }
            for (kind, channels) in [(ChannelKind::Asr, &mut asr), (ChannelKind::Llm, &mut llm)] {
                for mut channel in source.metadata.list_channels(kind) {
                    if !channels.iter().any(|existing| existing.id == channel.id) {
                        channel.order = channels.len() as u32;
                        channels.push(channel);
                    }
                }
            }
            let active = |slot| {
                non_empty(state.metadata.active_provider(slot))
                    .unwrap_or_else(|| source.metadata.active_provider(slot))
            };
            state.metadata = CredentialMetadata::from_parts(
                asr,
                llm,
                active(ProviderSlot::Asr),
                active(ProviderSlot::Llm),
                active(ProviderSlot::Omni),
                state.metadata.revision().saturating_add(1),
            );
            // persist_metadata is the commit point for channels, keys AND this
            // marker. A failed rename leaves no false success marker on disk.
            state.legacy_migrated = true;
            Ok(())
        })
    }
}

impl CredentialStore for LinuxCredentialStore {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        let state = self
            .state
            .lock()
            .expect("credential metadata lock poisoned")
            .clone();
        Box::pin(async move {
            let active_asr_provider = non_empty(state.metadata.active_provider(ProviderSlot::Asr))
                .unwrap_or(preferences.active_asr_provider);
            let active_llm_provider = non_empty(state.metadata.active_provider(ProviderSlot::Llm))
                .unwrap_or(preferences.active_llm_provider);
            let active_omni_provider =
                non_empty(state.metadata.active_provider(ProviderSlot::Omni))
                    .unwrap_or(preferences.active_omni_provider);
            let provider_type = |kind, active: &str| {
                state
                    .metadata
                    .list_channels(kind)
                    .into_iter()
                    .find(|channel| channel.id == active)
                    .map(|channel| channel.provider_type)
                    .unwrap_or_else(|| active.to_string())
            };
            let asr_provider_type = provider_type(ChannelKind::Asr, &active_asr_provider);
            let llm_provider_type = provider_type(ChannelKind::Llm, &active_llm_provider);
            let has = |namespace, provider: &str, account: &str| {
                state.keys.iter().any(|key| {
                    key.namespace == namespace
                        && key.provider_id.as_deref().is_none_or(|id| id == provider)
                        && key.account == account
                })
            };
            let volcengine_provider = if asr_provider_type == "volcengine" {
                active_asr_provider.as_str()
            } else {
                "volcengine"
            };
            let endpoint_key = CredentialKey::new(
                CredentialNamespace::Llm,
                Some(active_llm_provider.clone()),
                LLM_ENDPOINT_ACCOUNT,
            )?;
            #[cfg(target_os = "linux")]
            let llm_endpoint = if has(
                CredentialNamespace::Llm,
                &active_llm_provider,
                LLM_ENDPOINT_ACCOUNT,
            ) {
                tokio::task::spawn_blocking(move || read_secret(&endpoint_key))
                    .await
                    .map_err(join_error)??
                    .map(SecretValue::into_exposed)
            } else {
                None
            };
            #[cfg(not(target_os = "linux"))]
            let llm_endpoint: Option<String> = {
                let _ = endpoint_key;
                None
            };
            let auth_mode_key = CredentialKey::new(
                CredentialNamespace::Asr,
                Some(volcengine_provider.to_string()),
                VOLCENGINE_AUTH_MODE_ACCOUNT,
            )?;
            #[cfg(target_os = "linux")]
            let volcengine_auth_mode = if has(
                CredentialNamespace::Asr,
                volcengine_provider,
                VOLCENGINE_AUTH_MODE_ACCOUNT,
            ) {
                tokio::task::spawn_blocking(move || read_secret(&auth_mode_key))
                    .await
                    .map_err(join_error)??
                    .map(SecretValue::into_exposed)
            } else {
                None
            };
            #[cfg(not(target_os = "linux"))]
            let volcengine_auth_mode = {
                let _ = auth_mode_key;
                None
            };
            let configuration = openless_core::provider_rules::CredentialConfiguration {
                asr_api_key: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    ASR_API_KEY_ACCOUNT,
                ),
                asr_endpoint: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    ASR_ENDPOINT_ACCOUNT,
                ),
                asr_model: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    ASR_MODEL_ACCOUNT,
                ),
                volcengine_auth_mode,
                volcengine_app_key: has(
                    CredentialNamespace::Asr,
                    volcengine_provider,
                    VOLCENGINE_APP_KEY_ACCOUNT,
                ),
                volcengine_access_key: has(
                    CredentialNamespace::Asr,
                    volcengine_provider,
                    VOLCENGINE_ACCESS_KEY_ACCOUNT,
                ),
                volcengine_api_key: has(
                    CredentialNamespace::Asr,
                    volcengine_provider,
                    VOLCENGINE_API_KEY_ACCOUNT,
                ),
                volcengine_resource_id: has(
                    CredentialNamespace::Asr,
                    volcengine_provider,
                    VOLCENGINE_RESOURCE_ID_ACCOUNT,
                ),
                xfyun_app_id: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    XFYUN_APP_ID_ACCOUNT,
                ),
                xfyun_api_key: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    XFYUN_API_KEY_ACCOUNT,
                ),
                llm_api_key: has(
                    CredentialNamespace::Llm,
                    &active_llm_provider,
                    LLM_API_KEY_ACCOUNT,
                ),
                llm_endpoint: llm_endpoint
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
                llm_endpoint_matches_default: llm_endpoint.as_deref().is_some_and(|endpoint| {
                    openless_core::provider_rules::default_llm_endpoint(&llm_provider_type)
                        .is_some_and(|default| {
                            openless_core::provider_rules::equivalent_endpoint(endpoint, default)
                        })
                }),
                llm_model: has(
                    CredentialNamespace::Llm,
                    &active_llm_provider,
                    LLM_MODEL_ACCOUNT,
                ),
                codex_oauth: false,
                omni_api_key: has(
                    CredentialNamespace::Omni,
                    &active_omni_provider,
                    OMNI_API_KEY_ACCOUNT,
                ),
                omni_endpoint: has(
                    CredentialNamespace::Omni,
                    &active_omni_provider,
                    OMNI_ENDPOINT_ACCOUNT,
                ),
                omni_model: has(
                    CredentialNamespace::Omni,
                    &active_omni_provider,
                    OMNI_MODEL_ACCOUNT,
                ),
            };
            let local_asr_configured = match asr_provider_type.as_str() {
                "local-qwen3" | "local-qwen3-c" => Some(crate::backend::qwen_engine_available()),
                "local-qwen3-mlx"
                | "local-whisper"
                | "apple-speech"
                | "foundry-local-whisper"
                | "sherpa-onnx-local" => Some(false),
                _ => None,
            };
            let asr_configured = openless_core::provider_rules::asr_configured(
                &asr_provider_type,
                &configuration,
                local_asr_configured,
            );
            let llm_configured =
                openless_core::provider_rules::llm_configured(&llm_provider_type, &configuration);
            Ok(CredentialsStatus {
                active_asr_provider,
                active_llm_provider,
                pipeline_mode: openless_core::shared_types::effective_pipeline_mode(
                    preferences.multimodal_pipeline_enabled,
                    preferences.pipeline_mode,
                ),
                asr_configured,
                llm_configured,
                omni_configured: openless_core::provider_rules::omni_configured(
                    &active_omni_provider,
                    &configuration,
                ),
                volcengine_configured: openless_core::provider_rules::volcengine_configured(
                    &configuration,
                ),
                ark_configured: llm_configured,
            })
        })
    }

    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        // The committed key index controls visibility. A vault write may have
        // succeeded before its metadata rename failed; such an uncommitted entry
        // must not become usable through a freshly recreated channel with the
        // same ID. Migration retries read the destination directly to recover it.
        let committed = self
            .state
            .lock()
            .expect("credential metadata lock poisoned")
            .keys
            .contains(&key);
        Box::pin(async move {
            if !committed {
                return Ok(None);
            }
            #[cfg(target_os = "linux")]
            {
                tokio::task::spawn_blocking(move || read_secret(&key))
                    .await
                    .map_err(join_error)?
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = key;
                unsupported_keyring()
            }
        })
    }

    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let store = self.clone();
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                let remove = value.expose_secret().trim().is_empty();
                tokio::task::spawn_blocking(move || {
                    store.update_metadata(|state| {
                        if remove {
                            remove_secret(&key)?;
                        } else {
                            write_secret(&key, &value)?;
                        }
                        if remove {
                            state.keys.retain(|candidate| candidate != &key);
                        } else if !state.keys.contains(&key) {
                            state.keys.push(key);
                        }
                        Ok(())
                    })
                })
                .await
                .map_err(join_error)?
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (store, key, value);
                unsupported_keyring()
            }
        })
    }

    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        let store = self.clone();
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                tokio::task::spawn_blocking(move || {
                    store.update_metadata(|state| {
                        remove_secret(&key)?;
                        state.keys.retain(|candidate| candidate != &key);
                        Ok(())
                    })
                })
                .await
                .map_err(join_error)?
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (store, key);
                unsupported_keyring()
            }
        })
    }

    fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> BoxFuture<'static, Result<Vec<ChannelSummary>, BackendError>> {
        let channels = self
            .state
            .lock()
            .expect("credential metadata lock poisoned")
            .metadata
            .list_channels(kind);
        Box::pin(async move { Ok(channels) })
    }

    fn mutate_channel(
        &self,
        mutation: ChannelMutation,
    ) -> BoxFuture<'static, Result<ChannelMutationResult, BackendError>> {
        let store = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || store.mutate_channel_with(mutation, remove_secret))
                .await
                .map_err(join_error)?
        })
    }

    fn active_provider(
        &self,
        slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let provider = self
            .state
            .lock()
            .expect("credential metadata lock poisoned")
            .metadata
            .active_provider(slot);
        Box::pin(async move { Ok(provider) })
    }

    fn set_active_provider(
        &self,
        slot: ProviderSlot,
        provider_id: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let store = self.clone();
        Box::pin(async move { store.set_active_provider_immediate(slot, &provider_id) })
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn namespace_for_kind(kind: ChannelKind) -> CredentialNamespace {
    match kind {
        ChannelKind::Asr => CredentialNamespace::Asr,
        ChannelKind::Llm => CredentialNamespace::Llm,
    }
}

fn read_metadata(path: &Path) -> Result<PersistedCredentialMetadata, BackendError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let mut state: PersistedCredentialMetadata =
                serde_json::from_slice(&bytes).map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Persistence,
                        format!("invalid Linux credential metadata: {error}"),
                    )
                })?;
            if state.version > METADATA_VERSION {
                return Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "Linux credential metadata is newer than this application",
                ));
            }
            state.version = METADATA_VERSION;
            Ok(state)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistedCredentialMetadata {
                version: METADATA_VERSION,
                ..PersistedCredentialMetadata::default()
            })
        }
        Err(error) => Err(BackendError::new(
            BackendErrorCode::Persistence,
            format!("failed to read Linux credential metadata: {error}"),
        )),
    }
}

fn persist_metadata(path: &Path, state: &PersistedCredentialMetadata) -> Result<(), BackendError> {
    let parent = path.parent().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::Persistence,
            "Linux credential metadata has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Persistence,
            format!("failed to create Linux credential directory: {error}"),
        )
    })?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Persistence,
            format!("failed to serialize Linux credential metadata: {error}"),
        )
    })?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Persistence,
            format!("failed to stage Linux credential metadata: {error}"),
        )
    })?;
    #[cfg(windows)]
    if path.exists() {
        std::fs::remove_file(path).map_err(|error| {
            BackendError::new(
                BackendErrorCode::Persistence,
                format!("failed to replace test credential metadata: {error}"),
            )
        })?;
    }
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        BackendError::new(
            BackendErrorCode::Persistence,
            format!("failed to commit Linux credential metadata: {error}"),
        )
    })
}

#[cfg(target_os = "linux")]
fn keyring_entry(key: &CredentialKey) -> Result<keyring::Entry, BackendError> {
    let account = serde_json::to_string(key).map_err(|error| {
        BackendError::new(
            BackendErrorCode::Internal,
            format!("failed to encode credential key: {error}"),
        )
    })?;
    keyring::Entry::new(KEYRING_SERVICE, &account).map_err(keyring_error)
}

#[cfg(target_os = "linux")]
fn read_legacy_secret(account: &str) -> Result<Option<SecretValue>, BackendError> {
    let entry = keyring::Entry::new(
        openless_core::credentials_legacy::LEGACY_CREDENTIAL_SERVICE,
        account,
    )
    .map_err(keyring_error)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(SecretValue::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    }
}

#[cfg(any(target_os = "linux", test))]
fn read_legacy_file(home_dir: &Path) -> Result<Option<SecretValue>, BackendError> {
    let path = home_dir.join(".openless").join("credentials.json");
    match std::fs::read_to_string(path) {
        Ok(payload) => Ok(Some(SecretValue::new(payload))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(BackendError::new(
            BackendErrorCode::Persistence,
            "failed to read legacy credential file",
        )),
    }
}

#[cfg(target_os = "linux")]
fn read_secret(key: &CredentialKey) -> Result<Option<SecretValue>, BackendError> {
    match keyring_entry(key)?.get_password() {
        Ok(value) => Ok(Some(SecretValue::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    }
}

#[cfg(target_os = "linux")]
fn write_secret(key: &CredentialKey, value: &SecretValue) -> Result<(), BackendError> {
    keyring_entry(key)?
        .set_password(value.expose_secret())
        .map_err(keyring_error)
}

#[cfg(target_os = "linux")]
fn remove_secret(key: &CredentialKey) -> Result<(), BackendError> {
    match keyring_entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    }
}

#[cfg(not(target_os = "linux"))]
fn remove_secret(_: &CredentialKey) -> Result<(), BackendError> {
    unsupported_keyring()
}

#[cfg(target_os = "linux")]
fn keyring_error(error: keyring::Error) -> BackendError {
    BackendError::new(
        BackendErrorCode::Persistence,
        format!("Linux credential vault operation failed: {error}"),
    )
}

fn join_error(error: tokio::task::JoinError) -> BackendError {
    BackendError::new(
        BackendErrorCode::Internal,
        format!("Linux credential task failed: {error}"),
    )
}

#[cfg(not(target_os = "linux"))]
fn unsupported_keyring<T>() -> Result<T, BackendError> {
    Err(BackendError::new(
        BackendErrorCode::Unsupported,
        "Linux Secret Service credential adapter is unavailable on this target",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_store() -> LinuxCredentialStore {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-credential-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        LinuxCredentialStore::open(&root).unwrap()
    }

    fn create_channel(store: &LinuxCredentialStore, kind: ChannelKind) -> String {
        let ChannelMutationResult::Created(id) = store
            .mutate_channel_with(
                ChannelMutation::Create {
                    kind,
                    provider_type: "openai-compatible".into(),
                    name: String::new(),
                },
                |_| panic!("creation must not delete secrets"),
            )
            .unwrap()
        else {
            panic!("expected channel creation");
        };
        id
    }

    #[test]
    fn deleting_a_channel_removes_its_credential_keys() {
        let store = temporary_store();
        let id = create_channel(&store, ChannelKind::Asr);
        let llm_id = create_channel(&store, ChannelKind::Llm);
        assert_eq!(id, llm_id);
        let asr_key = CredentialKey::new(
            CredentialNamespace::Asr,
            Some(id.clone()),
            ASR_API_KEY_ACCOUNT,
        )
        .unwrap();
        let asr_endpoint = CredentialKey::new(
            CredentialNamespace::Asr,
            Some(id.clone()),
            ASR_ENDPOINT_ACCOUNT,
        )
        .unwrap();
        let llm_key =
            CredentialKey::new(CredentialNamespace::Llm, Some(llm_id), LLM_API_KEY_ACCOUNT)
                .unwrap();
        let mut vault = std::collections::HashMap::from([
            (asr_key.clone(), SecretValue::new("fixture-asr")),
            (
                asr_endpoint.clone(),
                SecretValue::new("https://asr.example/v1"),
            ),
            (llm_key.clone(), SecretValue::new("fixture-llm")),
        ]);
        store
            .update_metadata(|state| {
                state.keys = vault.keys().cloned().collect();
                Ok(())
            })
            .unwrap();
        store
            .mutate_channel_with(
                ChannelMutation::Delete {
                    kind: ChannelKind::Asr,
                    id: id.clone(),
                },
                |key| {
                    vault.remove(key);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(store.state.lock().unwrap().keys, vec![llm_key.clone()]);
        assert_eq!(vault.len(), 1);
        assert!(vault.contains_key(&llm_key));
        assert_eq!(create_channel(&store, ChannelKind::Asr), id);
        assert!(!vault.contains_key(&asr_key));
        assert!(!vault.contains_key(&asr_endpoint));
        let reopened = LinuxCredentialStore::open(store.metadata_path.parent().unwrap()).unwrap();
        assert_eq!(reopened.state.lock().unwrap().keys, vec![llm_key]);
        let _ = std::fs::remove_dir_all(store.metadata_path.parent().unwrap());
    }

    #[test]
    fn failed_channel_deletion_remains_retryable_and_blank_checks_are_namespace_scoped() {
        let store = temporary_store();
        let id = create_channel(&store, ChannelKind::Asr);
        create_channel(&store, ChannelKind::Llm);
        let asr_key = CredentialKey::new(
            CredentialNamespace::Asr,
            Some(id.clone()),
            ASR_API_KEY_ACCOUNT,
        )
        .unwrap();
        let llm_key = CredentialKey::new(
            CredentialNamespace::Llm,
            Some(id.clone()),
            LLM_API_KEY_ACCOUNT,
        )
        .unwrap();
        let asr_endpoint = CredentialKey::new(
            CredentialNamespace::Asr,
            Some(id.clone()),
            ASR_ENDPOINT_ACCOUNT,
        )
        .unwrap();
        let mut vault = std::collections::HashMap::from([
            (asr_key.clone(), SecretValue::new("fixture-key")),
            (
                asr_endpoint.clone(),
                SecretValue::new("https://asr.example/v1"),
            ),
        ]);
        store
            .update_metadata(|state| {
                state.keys = vec![asr_key.clone(), asr_endpoint.clone(), llm_key];
                Ok(())
            })
            .unwrap();
        let mutation = ChannelMutation::Delete {
            kind: ChannelKind::Asr,
            id: id.clone(),
        };
        let before = std::fs::read(&store.metadata_path).unwrap();
        assert!(store
            .mutate_channel_with(mutation.clone(), |key| {
                if key == &asr_endpoint {
                    return Err(BackendError::new(
                        BackendErrorCode::Persistence,
                        "locked test vault",
                    ));
                }
                vault.remove(key);
                Ok(())
            })
            .is_err());
        assert!(!vault.contains_key(&asr_key));
        assert!(vault.contains_key(&asr_endpoint));
        assert_eq!(std::fs::read(&store.metadata_path).unwrap(), before);
        assert_eq!(
            store
                .state
                .lock()
                .unwrap()
                .metadata
                .list_channels(ChannelKind::Asr)
                .len(),
            1
        );
        store
            .mutate_channel_with(mutation, |key| {
                vault.remove(key);
                Ok(())
            })
            .unwrap();
        assert!(vault.is_empty());
        assert_eq!(create_channel(&store, ChannelKind::Asr), id);
        assert_eq!(
            store
                .mutate_channel_with(
                    ChannelMutation::DeleteIfBlank {
                        kind: ChannelKind::Asr,
                        id
                    },
                    |_| panic!("blank channel has no secret to remove")
                )
                .unwrap(),
            ChannelMutationResult::DeletedIfBlank(true)
        );
        let _ = std::fs::remove_dir_all(store.metadata_path.parent().unwrap());
    }

    const LEGACY: &str =
        include_str!("../../crates/openless-core/tests/fixtures/credentials-legacy-v2.json");

    #[test]
    fn legacy_migration_without_host_home_never_reads_credential_sources() {
        let store = temporary_store();
        store
            .migrate_legacy_with(
                None,
                |_| panic!("no home was supplied; legacy vault must not be read"),
                |_| panic!("no home was supplied; legacy file must not be read"),
                |_| panic!("no home was supplied; destination vault must not be read"),
                |_, _| panic!("no home was supplied; destination vault must not be written"),
            )
            .unwrap();
        assert!(!store.metadata_path.exists());
        assert!(!store.state.lock().unwrap().legacy_migrated);
    }

    #[test]
    fn legacy_source_precedence_preserves_files_and_retries_unavailable_chunks() {
        use openless_core::credentials_legacy::LEGACY_CREDENTIAL_ACCOUNT;
        let store = temporary_store();
        let manifest = r#"{"openless_credentials_storage":"chunked","version":1,"chunks":1}"#;
        assert!(store
            .migrate_legacy_with(
                Some(store.metadata_path.parent().unwrap()),
                |account| Ok(
                    (account == LEGACY_CREDENTIAL_ACCOUNT).then(|| SecretValue::new(manifest))
                ),
                |_| panic!("incomplete vault must not fall back to an older file"),
                |_| Ok(None),
                |_, _| panic!("incomplete vault must not write destination"),
            )
            .is_err());
        assert!(!store.state.lock().unwrap().legacy_migrated);
        store
            .migrate_legacy_with(
                Some(store.metadata_path.parent().unwrap()),
                |account| {
                    Ok(match account {
                        LEGACY_CREDENTIAL_ACCOUNT => Some(SecretValue::new(manifest)),
                        "credentials.v1.chunk.0" => Some(SecretValue::new(LEGACY)),
                        _ => None,
                    })
                },
                |_| panic!("complete vault has precedence over a stale file"),
                |_| Ok(None),
                |_, _| Ok(()),
            )
            .unwrap();
        assert!(store.state.lock().unwrap().legacy_migrated);
        let _ = std::fs::remove_dir_all(store.metadata_path.parent().unwrap());

        let store = temporary_store();
        let legacy_home = store.metadata_path.parent().unwrap().join("supplied-home");
        let legacy_path = legacy_home.join(".openless").join("credentials.json");
        std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        std::fs::write(&legacy_path, LEGACY).unwrap();
        store
            .migrate_legacy_with(
                Some(&legacy_home),
                |_| Ok(None),
                |home_dir| {
                    assert_eq!(home_dir, legacy_home);
                    read_legacy_file(home_dir)
                },
                |_| Ok(None),
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(&legacy_path).unwrap(), LEGACY);
        assert_eq!(store.state.lock().unwrap().keys.len(), 24);
        let _ = std::fs::remove_dir_all(store.metadata_path.parent().unwrap());
    }

    #[tokio::test]
    async fn uncommitted_vault_entries_are_not_visible_to_runtime_readers() {
        let store = temporary_store();
        let key = CredentialKey::new(
            CredentialNamespace::Asr,
            Some("shared".into()),
            ASR_API_KEY_ACCOUNT,
        )
        .unwrap();
        // This must return from the committed index before touching the platform
        // keyring, including on Windows where the Linux keyring is unavailable.
        assert!(store.read(key).await.unwrap().is_none());
    }

    #[test]
    fn legacy_import_retries_partial_writes_and_preserves_existing_configuration() {
        use openless_core::credentials_legacy::decode_legacy_credentials;
        let store = temporary_store();
        let current = ChannelSummary {
            id: "shared".into(),
            name: "Current settings".into(),
            provider_type: "custom".into(),
            enabled: true,
            order: 0,
            last_test: None,
        };
        store
            .update_metadata(|state| {
                state.metadata =
                    CredentialMetadata::from_parts(vec![], vec![current], "", "shared", "", 1);
                Ok(())
            })
            .unwrap();
        let vault = std::cell::RefCell::new(std::collections::HashMap::new());
        let read = |key: &CredentialKey| Ok(vault.borrow().get(key).cloned());
        let mut writes = 0;
        assert!(store
            .import_legacy_with(
                decode_legacy_credentials(LEGACY).unwrap(),
                read,
                |key, value| {
                    writes += 1;
                    if writes == 2 {
                        return Err(BackendError::new(
                            BackendErrorCode::Persistence,
                            "locked test vault",
                        ));
                    }
                    vault.borrow_mut().insert(key.clone(), value.clone());
                    Ok(())
                }
            )
            .is_err());
        assert!(!store.state.lock().unwrap().legacy_migrated);
        assert!(store.state.lock().unwrap().keys.is_empty());
        assert_eq!(vault.borrow().len(), 1);
        let mut writes = 0;
        store
            .import_legacy_with(
                decode_legacy_credentials(LEGACY).unwrap(),
                read,
                |key, value| {
                    writes += 1;
                    vault.borrow_mut().insert(key.clone(), value.clone());
                    Ok(())
                },
            )
            .unwrap();
        // The existing LLM card is authoritative, even where the user explicitly
        // cleared a secret. ASR with the same ID is an independent namespace.
        assert!(vault
            .borrow()
            .keys()
            .all(|key| key.namespace != CredentialNamespace::Llm));
        assert_eq!(vault.borrow().len(), 19);
        assert_eq!(writes, 18);
        let reopened = LinuxCredentialStore::open(store.metadata_path.parent().unwrap()).unwrap();
        let state = reopened.state.lock().unwrap();
        assert!(state.legacy_migrated);
        assert_eq!(
            state.metadata.list_channels(ChannelKind::Llm)[0].name,
            "Current settings"
        );
        assert_eq!(
            state.metadata.list_channels(ChannelKind::Llm)[0].provider_type,
            "custom"
        );
        assert_eq!(state.metadata.active_provider(ProviderSlot::Asr), "shared");
        assert_eq!(
            state.metadata.active_provider(ProviderSlot::Omni),
            "bailian"
        );
        drop(state);
        reopened
            .import_legacy_with(
                decode_legacy_credentials(LEGACY).unwrap(),
                |_| panic!("completed import must not read vault"),
                |_, _| panic!("completed import must not write vault"),
            )
            .unwrap();
        assert!(!std::fs::read_to_string(&store.metadata_path)
            .unwrap()
            .contains("fixture-"));
        let _ = std::fs::remove_dir_all(store.metadata_path.parent().unwrap());
    }

    #[test]
    fn legacy_import_marks_success_only_after_metadata_commit() {
        use openless_core::credentials_legacy::decode_legacy_credentials;
        let store = temporary_store();
        std::fs::create_dir_all(&store.metadata_path).unwrap();
        let vault = std::cell::RefCell::new(std::collections::HashMap::new());
        let read = |key: &CredentialKey| Ok(vault.borrow().get(key).cloned());
        assert!(store
            .import_legacy_with(
                decode_legacy_credentials(LEGACY).unwrap(),
                read,
                |key, value| {
                    vault.borrow_mut().insert(key.clone(), value.clone());
                    Ok(())
                }
            )
            .is_err());
        assert_eq!(vault.borrow().len(), 24);
        assert!(!store.state.lock().unwrap().legacy_migrated);
        std::fs::remove_dir(&store.metadata_path).unwrap();
        store
            .import_legacy_with(decode_legacy_credentials(LEGACY).unwrap(), read, |_, _| {
                panic!("retry must preserve previously written secrets")
            })
            .unwrap();
        assert_eq!(store.state.lock().unwrap().keys.len(), 24);
        assert!(store.state.lock().unwrap().legacy_migrated);
        let _ = std::fs::remove_dir_all(store.metadata_path.parent().unwrap());
    }

    #[tokio::test]
    async fn metadata_round_trips_without_secret_values() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-credential-metadata-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = LinuxCredentialStore::open(&root).unwrap();
        store
            .set_active_provider(ProviderSlot::Asr, "local-qwen".into())
            .await
            .unwrap();
        store
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Asr,
                provider_type: "openai-compatible".into(),
                name: "Primary".into(),
            })
            .await
            .unwrap();

        let reopened = LinuxCredentialStore::open(&root).unwrap();
        assert_eq!(
            reopened.active_provider(ProviderSlot::Asr).await.unwrap(),
            "local-qwen"
        );
        assert_eq!(
            reopened
                .list_channels(ChannelKind::Asr)
                .await
                .unwrap()
                .len(),
            1
        );
        let persisted = std::fs::read_to_string(root.join("credential-metadata.json")).unwrap();
        assert!(!persisted.contains("secret"));
        assert!(!persisted.contains("password"));
        let preferences = UserPreferences {
            multimodal_pipeline_enabled: false,
            pipeline_mode: openless_core::shared_types::PipelineMode::Multimodal,
            ..Default::default()
        };
        assert_eq!(
            reopened.status(preferences).await.unwrap().pipeline_mode,
            openless_core::shared_types::PipelineMode::Traditional
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
