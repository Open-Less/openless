use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::time::Duration;

use futures_util::future::BoxFuture;
use openless_core::credentials::{
    ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT, LLM_API_KEY_ACCOUNT,
    LLM_ENDPOINT_ACCOUNT, LLM_MODEL_ACCOUNT, OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT,
    OMNI_MODEL_ACCOUNT, TENCENT_CLOUD_APP_ID_ACCOUNT, TENCENT_CLOUD_SECRET_ID_ACCOUNT,
    TENCENT_CLOUD_SECRET_KEY_ACCOUNT, VOLCENGINE_ACCESS_KEY_ACCOUNT, VOLCENGINE_API_KEY_ACCOUNT,
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

/// 一个 Omni 渠道的非敏感记录（与云端 `ChannelRecord` 的字段对齐）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredOmniChannel {
    id: String,
    provider_type: String,
    name: String,
    enabled: bool,
    order: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCredentialMetadata {
    version: u32,
    /// 渠道的非敏感配置（云端文档 `ChannelRecord.configuration`）。
    /// 恢复时会原样写回；导出时带上，保证 replace → capture 完全一致
    /// —— `replace_and_verify` 是逐字段比对的，丢了它就会回滚。
    #[serde(default)]
    channel_configuration: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Omni（多模态）渠道。core 的 `CredentialMetadata` 只有 Asr/Llm 两类渠道，
    /// 而云端文档里有 Omni；不单独存就会在「替换 → 重新捕获」时少一半渠道，
    /// 恢复因此回滚。这里自己维护一份。
    #[serde(default)]
    omni_channels: Vec<StoredOmniChannel>,
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
    siwc: Arc<openless_core::siwc::SiwcService>,
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
            siwc: crate::siwc::service(data_dir.to_path_buf()),
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


    /// Omni 的账号（`omni.api_key` 等）在前端读写时**不带 provider**，
    /// 而库里是按活跃 omni provider 存的。这里把 provider 为空的 Omni key
    /// 解析到当前活跃 provider，否则读写会落到一个不存在的 key 上
    /// （表现就是「多模态的密钥不显示」）。
    fn resolve_omni_key(&self, mut key: CredentialKey) -> CredentialKey {
        if key.namespace == CredentialNamespace::Omni && key.provider_id.is_none() {
            let active = self
                .state
                .lock()
                .expect("credential metadata lock poisoned")
                .metadata
                .active_provider(ProviderSlot::Omni);
            if !active.trim().is_empty() {
                key.provider_id = Some(active);
            }
        }
        key
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
    fn siwc(&self) -> Option<Arc<openless_core::siwc::SiwcService>> {
        Some(self.siwc.clone())
    }

    // ── 端到端加密云同步的密钥存储 ───────────────────────────────────
    // core 的默认实现是「fail closed」（没有适配器就报错），所以 Linux 必须在
    // 自己的凭据库里实现，否则云同步永远返回 secure_storage_denied。
    fn read_sync_secret(
        &self,
        account: openless_core::credentials::SyncSecretAccount,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        Box::pin(async move { read_sync_secret_value(&account) })
    }

    fn write_sync_secret(
        &self,
        account: openless_core::credentials::SyncSecretAccount,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move { write_sync_secret_value(&account, &value) })
    }

    fn remove_sync_secret(
        &self,
        account: openless_core::credentials::SyncSecretAccount,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move { remove_sync_secret_value(&account) })
    }


    // ── 云同步：导出/替换整套凭据 ────────────────────────────────────
    // 账号命名与其它平台完全一致（asr.api_key / ark.api_key / …），所以在
    // Linux 上创建的快照可以被 macOS/Windows 还原，反之亦然。
    fn export_sync_credentials_readonly(
        &self,
    ) -> BoxFuture<'static, Result<openless_core::credentials::SyncCredentials, BackendError>> {
        let state = self
            .state
            .lock()
            .expect("credential metadata lock poisoned")
            .clone();
        Box::pin(async move {
            use openless_core::credentials::{
                SyncChannel, SyncCredentialRecord, SyncCredentials, SyncNamespace,
            };
            eprintln!("[linux-creds] capture start: channels={}", state.metadata.list_channels(ChannelKind::Asr).len() + state.metadata.list_channels(ChannelKind::Llm).len());
            let mut snapshot = SyncCredentials {
                channels: Vec::new(),
                credentials: Vec::new(),
            };
            for (kind, namespace, slot) in [
                (ChannelKind::Asr, SyncNamespace::Asr, ProviderSlot::Asr),
                (ChannelKind::Llm, SyncNamespace::Llm, ProviderSlot::Llm),
            ] {
                let active = state.metadata.active_provider(slot);
                for channel in state.metadata.list_channels(kind) {
                    snapshot.channels.push(SyncChannel {
                        id: channel.id.clone(),
                        namespace,
                        provider_type: channel.provider_type.clone(),
                        name: channel.name.clone(),
                        enabled: channel.enabled,
                        order: channel.order,
                        active: channel.id == active,
                        configuration: state
                            .channel_configuration
                            .get(&format!("{}:{}", namespace.as_str(), channel.id))
                            .cloned()
                            .unwrap_or_default(),
                    });
                    let mut accounts = std::collections::BTreeMap::new();
                    for key in state.keys.iter().filter(|key| {
                        key.namespace == credential_namespace(namespace)
                            && key.provider_id.as_deref() == Some(channel.id.as_str())
                    }) {
                        if let Some(secret) = read_secret(key)? {
                            accounts.insert(key.account.clone(), secret.expose_secret().to_string());
                        }
                    }
                    snapshot.credentials.push(SyncCredentialRecord {
                        channel_id: channel.id.clone(),
                        namespace,
                        accounts,
                    });
                }
            }
            // Omni 渠道：优先用元数据里保存的记录；没有就从 key 里推导。
            let active_omni = state.metadata.active_provider(ProviderSlot::Omni);
            let mut omni_channels = state.omni_channels.clone();
            if omni_channels.is_empty() {
                let mut ids: Vec<String> = state
                    .keys
                    .iter()
                    .filter(|key| key.namespace == CredentialNamespace::Omni)
                    .filter_map(|key| key.provider_id.clone())
                    .collect();
                if !active_omni.is_empty() {
                    ids.push(active_omni.clone());
                }
                ids.sort();
                ids.dedup();
                for (order, id) in ids.into_iter().enumerate() {
                    omni_channels.push(StoredOmniChannel {
                        provider_type: id.clone(),
                        name: id.clone(),
                        enabled: true,
                        order: order as u32,
                        id,
                    });
                }
            }
            for channel in &omni_channels {
                snapshot.channels.push(SyncChannel {
                    id: channel.id.clone(),
                    namespace: SyncNamespace::Omni,
                    provider_type: channel.provider_type.clone(),
                    name: channel.name.clone(),
                    enabled: channel.enabled,
                    order: channel.order,
                    active: channel.id == active_omni,
                    configuration: state
                        .channel_configuration
                        .get(&format!("omni:{}", channel.id))
                        .cloned()
                        .unwrap_or_default(),
                });
                let mut accounts = std::collections::BTreeMap::new();
                for key in state.keys.iter().filter(|key| {
                    key.namespace == CredentialNamespace::Omni
                        && key.provider_id.as_deref() == Some(channel.id.as_str())
                }) {
                    if let Some(secret) = read_secret(key)? {
                        accounts.insert(key.account.clone(), secret.expose_secret().to_string());
                    }
                }
                snapshot.credentials.push(SyncCredentialRecord {
                    channel_id: channel.id.clone(),
                    namespace: SyncNamespace::Omni,
                    accounts,
                });
            }
            eprintln!(
                "[linux-creds] capture done: channels={} (base={}) credentials={}",
                snapshot.channels.len(),
                snapshot.channels.len() - omni_channels.len(),
                snapshot.credentials.len()
            );
            snapshot.validate()?;
            Ok(snapshot)
        })
    }

    fn export_sync_credentials(
        &self,
        _permit: &openless_core::credentials::ExclusivePermit,
    ) -> BoxFuture<'static, Result<openless_core::credentials::SyncCredentials, BackendError>> {
        self.export_sync_credentials_readonly()
    }

    fn replace_sync_credentials(
        &self,
        snapshot: openless_core::credentials::SyncCredentials,
        _permit: &openless_core::credentials::ExclusivePermit,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let state_handle = Arc::clone(&self.state);
        let metadata_path = self.metadata_path.clone();
        // 同步范围之外的 key（Marketplace / Application）必须保留：
        // `read()` 用这份索引判断可见性，整份覆盖会让 GitHub token 立刻消失。
        let preserved_keys: Vec<CredentialKey> = state_handle
            .lock()
            .expect("credential metadata lock poisoned")
            .keys
            .iter()
            .filter(|key| {
                !matches!(
                    key.namespace,
                    CredentialNamespace::Asr
                        | CredentialNamespace::Llm
                        | CredentialNamespace::Omni
                )
            })
            .cloned()
            .collect();
        Box::pin(async move {
            use openless_core::credentials::{
                ChannelSummary, CredentialKey, CredentialMetadata, SyncNamespace,
            };
            eprintln!(
                "[linux-creds] replace start: channels={} credentials={}",
                snapshot.channels.len(),
                snapshot.credentials.len()
            );
            snapshot.validate()?;
            let mut asr = Vec::new();
            let mut llm = Vec::new();
            let mut omni_channels: Vec<StoredOmniChannel> = Vec::new();
            let mut active = std::collections::HashMap::new();
            for channel in &snapshot.channels {
                let summary = ChannelSummary {
                    id: channel.id.clone(),
                    name: channel.name.clone(),
                    provider_type: channel.provider_type.clone(),
                    enabled: channel.enabled,
                    order: channel.order,
                    last_test: None,
                };
                match channel.namespace {
                    SyncNamespace::Asr => asr.push(summary),
                    SyncNamespace::Llm => llm.push(summary),
                    SyncNamespace::Omni => omni_channels.push(StoredOmniChannel {
                        id: channel.id.clone(),
                        provider_type: channel.provider_type.clone(),
                        name: channel.name.clone(),
                        enabled: channel.enabled,
                        order: channel.order,
                    }),
                }
                if channel.active {
                    active.insert(channel.namespace.as_str().to_string(), channel.id.clone());
                }
            }
            let mut channel_configuration = std::collections::BTreeMap::new();
            for channel in &snapshot.channels {
                if !channel.configuration.is_empty() {
                    channel_configuration.insert(
                        format!("{}:{}", channel.namespace.as_str(), channel.id),
                        channel.configuration.clone(),
                    );
                }
            }
            let mut keys: Vec<CredentialKey> = preserved_keys.clone();
            for record in &snapshot.credentials {
                for (account, value) in &record.accounts {
                    let key = CredentialKey::new(
                        credential_namespace(record.namespace),
                        Some(record.channel_id.clone()),
                        account.clone(),
                    )
                    .map_err(|error| {
                        eprintln!(
                            "[linux-creds] restore failed at key build: ns={:?} channel={} account={} err={:?}",
                            record.namespace, record.channel_id, account, error.code
                        );
                        error
                    })?;
                    write_secret(&key, &SecretValue::new(value.clone())).map_err(|error| {
                        eprintln!(
                            "[linux-creds] restore failed writing secret: channel={} account={} err={:?} {}",
                            record.channel_id, account, error.code, error.message
                        );
                        error
                    })?;
                    keys.push(key);
                }
            }
            // 云端文档可能没有标记 active omni（我们的 capture 在元数据为空时
            // 全部给 false）。这种情况下按「有 api_key 的第一个」补一个，
            // 否则 `activeOmniProvider` 是空的，界面里 omni 卡片会显示成未配置。
            let active_omni = active.get("omni").cloned().unwrap_or_else(|| {
                let with_key = snapshot
                    .credentials
                    .iter()
                    .filter(|record| record.namespace == SyncNamespace::Omni)
                    .find(|record| record.accounts.contains_key("omni.api_key"))
                    .map(|record| record.channel_id.clone());
                with_key
                    .or_else(|| omni_channels.first().map(|channel| channel.id.clone()))
                    .unwrap_or_default()
            });
            let metadata = CredentialMetadata::from_parts(
                asr,
                llm,
                active.get("asr").cloned().unwrap_or_default(),
                active.get("llm").cloned().unwrap_or_default(),
                active_omni,
                0,
            );
            let mut state = state_handle
                .lock()
                .expect("credential metadata lock poisoned");
            state.metadata = metadata;
            state.omni_channels = omni_channels;
            state.channel_configuration = channel_configuration;
            state.keys = keys;
            let result = persist_metadata(&metadata_path, &state);
            eprintln!("[linux-creds] replace done: keys={} ok={:?}", state.keys.len(), result.is_ok());
            result
        })
    }

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
            let active_channel = |kind, slot, fallback: String| {
                let active = state.metadata.selected_provider(slot);
                if state.metadata.list_channels(kind).is_empty() {
                    non_empty(active).unwrap_or(fallback)
                } else {
                    active
                }
            };
            let active_asr_provider = active_channel(
                ChannelKind::Asr,
                ProviderSlot::Asr,
                preferences.active_asr_provider,
            );
            let active_llm_provider = active_channel(
                ChannelKind::Llm,
                ProviderSlot::Llm,
                preferences.active_llm_provider,
            );
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
            let service_key = CredentialKey::new(
                CredentialNamespace::Asr,
                Some(volcengine_provider.to_string()),
                openless_core::credentials::VOLCENGINE_SERVICE_ACCOUNT,
            )?;
            #[cfg(target_os = "linux")]
            let volcengine_service = if has(
                CredentialNamespace::Asr,
                volcengine_provider,
                openless_core::credentials::VOLCENGINE_SERVICE_ACCOUNT,
            ) {
                tokio::task::spawn_blocking(move || read_secret(&service_key))
                    .await
                    .map_err(join_error)??
                    .map(SecretValue::into_exposed)
            } else {
                None
            };
            #[cfg(not(target_os = "linux"))]
            let volcengine_service = {
                let _ = service_key;
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
                volcengine_service,
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
                tencent_cloud_app_id: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    TENCENT_CLOUD_APP_ID_ACCOUNT,
                ),
                tencent_cloud_secret_id: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    TENCENT_CLOUD_SECRET_ID_ACCOUNT,
                ),
                tencent_cloud_secret_key: has(
                    CredentialNamespace::Asr,
                    &active_asr_provider,
                    TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
                ),
                llm_api_key: has(
                    CredentialNamespace::Llm,
                    &active_llm_provider,
                    LLM_API_KEY_ACCOUNT,
                ),
                llm_endpoint: llm_endpoint
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
                llm_api_key_required: openless_core::provider_rules::api_key_required(
                    openless_core::ProviderKind::Llm,
                    &llm_provider_type,
                    llm_endpoint.as_deref(),
                ),
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
            // Linux ships no local inference engine, so every native/local ASR
            // provider id is reported unconfigured rather than gated on a Qwen
            // runtime that is never present.
            let local_asr_configured = match asr_provider_type.as_str() {
                "local-qwen3"
                | "local-qwen3-c"
                | "local-qwen3-mlx"
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
        let key = self.resolve_omni_key(key);
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
                tokio::task::spawn_blocking(move || require_committed_value(read_secret(&key)?))
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
        let key = self.resolve_omni_key(key);
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
        let key = self.resolve_omni_key(key);
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

#[cfg(all(test, target_os = "linux"))]
mod vault_timeout_tests {
    use super::*;

    /// 钥匙串在等用户输密码时不会返回：调用方必须先拿到一句「已锁定」而不是
    /// 一起挂住。这里用一个永不返回的闭包验证超时会被翻译成 retryable 错误。
    #[test]
    fn a_hung_secret_service_operation_times_out_into_a_locked_error() {
        let started = std::time::Instant::now();
        let result = run_with_vault_timeout_for::<(), _>(
            "contract-hang",
            Duration::from_millis(120),
            || {
                std::thread::sleep(Duration::from_secs(30));
                Ok(())
            },
        );
        let error = result.expect_err("a hung vault operation must not report success");
        assert_eq!(error.message, crate::linux_access::VAULT_LOCKED_MESSAGE);
        assert!(error.retryable, "a locked vault is recoverable, not fatal");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the caller must return promptly instead of waiting for the vault"
        );
    }

    #[test]
    fn a_fast_operation_returns_its_own_result() {
        let value =
            run_with_vault_timeout_for::<u8, _>("contract-fast", Duration::from_secs(5), || Ok(7))
                .expect("a fast operation passes straight through");
        assert_eq!(value, 7);
    }
}

#[cfg(any(target_os = "linux", test))]
fn require_committed_value(
    value: Option<SecretValue>,
) -> Result<Option<SecretValue>, BackendError> {
    value.map(Some).ok_or_else(|| BackendError::new(
        BackendErrorCode::Persistence,
        "A saved credential is missing from the active Secret Service vault. Restore the original wallet before editing this channel; its metadata has been retained.",
    ))
}

/// 单次 Secret Service 操作的硬上限。
///
/// zbus / `secret_service` 在「钥匙串在等用户输密码」时**不会**返回：`Unlock` 会
/// 一直等 prompt 的 `Completed` 信号。以前这个等待没有任何上限，于是开机后钥匙串
/// 没解锁时，凭据相关的调用会挂在那里，用户看到的却是「什么都没发生」——没有报错、
/// 也没有解锁窗口。
///
/// 这里把操作丢进独立线程并限时等待：到点就报「钥匙串已锁定」（retryable），
/// 让上层把这句话显示出来，用户去设置页点「解锁钥匙串」。
#[cfg(target_os = "linux")]
const VAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(8);

#[cfg(target_os = "linux")]
fn run_with_vault_timeout<T, F>(label: &'static str, work: F) -> Result<T, BackendError>
where
    F: FnOnce() -> Result<T, BackendError> + Send + 'static,
    T: Send + 'static,
{
    run_with_vault_timeout_for(label, VAULT_OPERATION_TIMEOUT, work)
}

#[cfg(target_os = "linux")]
fn run_with_vault_timeout_for<T, F>(
    label: &'static str,
    timeout: Duration,
    work: F,
) -> Result<T, BackendError>
where
    F: FnOnce() -> Result<T, BackendError> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name(format!("openless-vault-{label}"))
        .spawn(move || {
            let _ = tx.send(work());
        });
    if spawned.is_err() {
        return Err(BackendError::new(
            BackendErrorCode::Internal,
            "failed to start the credential vault worker",
        ));
    }
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => {
            log::warn!(
                "Secret Service did not answer within {timeout:?} (likely locked); surfacing a locked-vault error"
            );
            Err(crate::linux_access::vault_locked_error())
        }
    }
}

#[cfg(target_os = "linux")]
fn keyring_entry(key: &CredentialKey) -> Result<keyring::Entry, BackendError> {
    crate::linux_access::ensure_persistent_vault()?;
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
    crate::linux_access::ensure_persistent_vault()?;
    let entry = keyring::Entry::new(
        openless_core::credentials_legacy::LEGACY_CREDENTIAL_SERVICE,
        account,
    )
    .map_err(keyring_error)?;
    run_with_vault_timeout("legacy-read", move || match entry.get_password() {
        Ok(value) => Ok(Some(SecretValue::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    })
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
    let entry = keyring_entry(key)?;
    run_with_vault_timeout("read", move || match entry.get_password() {
        Ok(value) => Ok(Some(SecretValue::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    })
}

#[cfg(target_os = "linux")]
fn write_secret(key: &CredentialKey, value: &SecretValue) -> Result<(), BackendError> {
    let entry = keyring_entry(key)?;
    let secret = value.expose_secret().to_string();
    run_with_vault_timeout("write", move || {
        entry.set_password(&secret).map_err(keyring_error)
    })
}

#[cfg(target_os = "linux")]
fn remove_secret(key: &CredentialKey) -> Result<(), BackendError> {
    let entry = keyring_entry(key)?;
    run_with_vault_timeout("remove", move || match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    })
}

/// `SyncNamespace` ↔ `CredentialNamespace`（枚举名一致，只是分属两个模块）。
#[cfg(any(target_os = "linux", test))]
fn credential_namespace(
    namespace: openless_core::credentials::SyncNamespace,
) -> openless_core::credentials::CredentialNamespace {
    use openless_core::credentials::{CredentialNamespace, SyncNamespace};
    match namespace {
        SyncNamespace::Asr => CredentialNamespace::Asr,
        SyncNamespace::Llm => CredentialNamespace::Llm,
        SyncNamespace::Omni => CredentialNamespace::Omni,
    }
}

/// 同步（E2EE）密钥单独走一条 keyring 账号空间：账号名由 core 生成
/// （`cloud-sync.e2ee.local.<digest>` / `cloud-sync.e2ee.key.<digest>`），
/// 与 provider 凭据的 key 形状完全不同，所以不会互相覆盖。
#[cfg(target_os = "linux")]
fn sync_secret_entry(
    account: &openless_core::credentials::SyncSecretAccount,
) -> Result<keyring::Entry, BackendError> {
    crate::linux_access::ensure_persistent_vault()?;
    keyring::Entry::new(KEYRING_SERVICE, account.as_str()).map_err(keyring_error)
}

#[cfg(target_os = "linux")]
fn read_sync_secret_value(
    account: &openless_core::credentials::SyncSecretAccount,
) -> Result<Option<SecretValue>, BackendError> {
    let entry = sync_secret_entry(account)?;
    run_with_vault_timeout("sync-read", move || match entry.get_password() {
        Ok(value) => Ok(Some(SecretValue::new(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    })
}

#[cfg(target_os = "linux")]
fn write_sync_secret_value(
    account: &openless_core::credentials::SyncSecretAccount,
    value: &SecretValue,
) -> Result<(), BackendError> {
    let entry = sync_secret_entry(account)?;
    let secret = value.expose_secret().to_string();
    run_with_vault_timeout("sync-write", move || {
        entry.set_password(&secret).map_err(keyring_error)
    })
}

#[cfg(target_os = "linux")]
fn remove_sync_secret_value(
    account: &openless_core::credentials::SyncSecretAccount,
) -> Result<(), BackendError> {
    let entry = sync_secret_entry(account)?;
    run_with_vault_timeout("sync-remove", move || match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    })
}

#[cfg(not(target_os = "linux"))]
fn read_sync_secret_value(
    _: &openless_core::credentials::SyncSecretAccount,
) -> Result<Option<SecretValue>, BackendError> {
    unsupported_keyring()
}

#[cfg(not(target_os = "linux"))]
fn write_sync_secret_value(
    _: &openless_core::credentials::SyncSecretAccount,
    _: &SecretValue,
) -> Result<(), BackendError> {
    unsupported_keyring()
}

#[cfg(not(target_os = "linux"))]
fn remove_sync_secret_value(
    _: &openless_core::credentials::SyncSecretAccount,
) -> Result<(), BackendError> {
    unsupported_keyring()
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

    #[test]
    fn a_committed_key_missing_from_the_current_vault_is_not_an_empty_form() {
        let error = require_committed_value(None).unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Persistence);
        assert!(error.message.contains("original wallet"));
    }

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
    async fn disabled_channels_do_not_restore_stale_preferences_in_status() {
        let store = temporary_store();
        let ChannelMutationResult::Created(id) = store
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Asr,
                provider_type: "tencent-cloud".into(),
                name: "Cloud".into(),
            })
            .await
            .unwrap()
        else {
            panic!("expected created channel")
        };
        store
            .mutate_channel(ChannelMutation::SetEnabled {
                kind: ChannelKind::Asr,
                id,
                enabled: false,
            })
            .await
            .unwrap();
        let reopened = LinuxCredentialStore::open(store.metadata_path.parent().unwrap()).unwrap();
        let status = reopened
            .status(UserPreferences {
                active_asr_provider: "local-qwen3-mlx".into(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(status.active_asr_provider.is_empty());
        assert!(reopened
            .active_provider(ProviderSlot::Asr)
            .await
            .unwrap()
            .is_empty());
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
        let ChannelMutationResult::Created(created) = store
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Asr,
                provider_type: "openai-compatible".into(),
                name: "Primary".into(),
            })
            .await
            .unwrap()
        else {
            panic!("expected created channel")
        };

        let reopened = LinuxCredentialStore::open(&root).unwrap();
        assert_eq!(
            reopened.active_provider(ProviderSlot::Asr).await.unwrap(),
            created
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

#[cfg(test)]
mod sync_credentials_roundtrip {
    use super::*;
    use openless_core::credentials::ExclusivePermit;

    /// 云同步恢复失败（`sync_restore_rolled_back`）时用它定位：先把 store 的
    /// 凭据导出，再用同一份快照做一次 replace，最后重新导出比对。
    #[test]
    fn export_replace_roundtrip_keeps_channels_and_secrets() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-sync-roundtrip-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = LinuxCredentialStore::open(&root).unwrap();
        let created = store
            .mutate_channel_with(
                ChannelMutation::Create {
                    kind: ChannelKind::Llm,
                    provider_type: "openai-compatible".into(),
                    name: "test".into(),
                },
                |_| Ok(()),
            )
            .unwrap();
        let ChannelMutationResult::Created(channel_id) = created else {
            panic!("expected create");
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(store.write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some(channel_id.clone()),
                    "ark.api_key",
                )
                .unwrap(),
                SecretValue::new("sk-test-roundtrip"),
            ))
            .unwrap();

        let gate = openless_core::cloud_sync_e2ee_store::gate::SyncWriteGate::open(
            root.join("generation.json"),
        )
        .unwrap();
        let permit: ExclusivePermit = gate.try_exclusive().unwrap();
        let snapshot = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(store.export_sync_credentials_readonly())
            .expect("export must succeed");
        assert!(!snapshot.channels.is_empty(), "export must contain channels");
        assert!(!snapshot.credentials.is_empty(), "export must contain credentials");

        runtime
            .block_on(store.replace_sync_credentials(snapshot.clone(), &permit))
            .expect("replace must succeed");

        let again = runtime
            .block_on(store.export_sync_credentials_readonly())
            .expect("re-export must succeed");
        assert_eq!(
            snapshot.channels.len(),
            again.channels.len(),
            "channel count must survive replace"
        );
        assert_eq!(
            snapshot.credentials, again.credentials,
            "credentials must round-trip byte for byte (restore verification relies on it)"
        );
        assert_eq!(
            snapshot.channels, again.channels,
            "channel records must round-trip field by field (including configuration)"
        );
    }
}

#[cfg(test)]
mod sync_credentials_preserve {
    use super::*;
    use openless_core::credentials::ExclusivePermit;

    /// 恢复（replace）不能把同步范围之外的 key 弄丢 —— Marketplace 的 GitHub
    /// token 就在那里。丢了之后的症状是「恢复完成却要求重新登录」。
    #[test]
    fn replace_keeps_marketplace_token_visible() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-sync-preserve-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = LinuxCredentialStore::open(&root).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let token_key = CredentialKey::new(
            CredentialNamespace::Marketplace,
            None,
            "marketplace.github.token",
        )
        .unwrap();
        runtime
            .block_on(store.write(token_key.clone(), SecretValue::new("gho_test")))
            .unwrap();

        let gate = openless_core::cloud_sync_e2ee_store::gate::SyncWriteGate::open(
            root.join("generation.json"),
        )
        .unwrap();
        let permit: ExclusivePermit = gate.try_exclusive().unwrap();
        let empty = openless_core::credentials::SyncCredentials {
            channels: Vec::new(),
            credentials: Vec::new(),
        };
        runtime
            .block_on(store.replace_sync_credentials(empty, &permit))
            .unwrap();

        let value = runtime
            .block_on(store.read(token_key))
            .unwrap()
            .expect("Marketplace token must stay visible");
        assert_eq!(value.expose_secret(), "gho_test");
    }
}
