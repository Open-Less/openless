use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

use futures_util::future::BoxFuture;

use crate::errors::{BackendError, BackendErrorCode};
use crate::shared_types::{CredentialsStatus, UserPreferences};

macro_rules! provider_identifier {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, BackendError> {
                let value = value.into();
                let value = value.trim();
                if value.is_empty() {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        concat!($label, " must not be blank"),
                    ));
                }
                Ok(Self(value.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = <String as serde::Deserialize>::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

provider_identifier!(ProviderChannelId, "provider channel id");
provider_identifier!(ProviderType, "provider type");

pub const ASR_API_KEY_ACCOUNT: &str = "asr.api_key";
pub const ASR_ENDPOINT_ACCOUNT: &str = "asr.endpoint";
pub const ASR_MODEL_ACCOUNT: &str = "asr.model";
pub const ASR_VOCABULARY_ID_ACCOUNT: &str = "asr.vocabulary_id";
pub const ASR_ADVANCED_CONFIG_ACCOUNT: &str = "asr.advanced_config";
pub const VOLCENGINE_APP_KEY_ACCOUNT: &str = "volcengine.app_key";
pub const VOLCENGINE_ACCESS_KEY_ACCOUNT: &str = "volcengine.access_key";
pub const VOLCENGINE_RESOURCE_ID_ACCOUNT: &str = "volcengine.resource_id";
pub const VOLCENGINE_AUTH_MODE_ACCOUNT: &str = "volcengine.auth_mode";
pub const VOLCENGINE_API_KEY_ACCOUNT: &str = "volcengine.api_key";
pub const XFYUN_APP_ID_ACCOUNT: &str = "xfyun.app_id";
pub const XFYUN_API_KEY_ACCOUNT: &str = "xfyun.api_key";
pub const LLM_API_KEY_ACCOUNT: &str = "ark.api_key";
pub const LLM_MODEL_ACCOUNT: &str = "ark.model_id";
pub const LLM_ENDPOINT_ACCOUNT: &str = "ark.endpoint";
pub const LLM_EXTRA_HEADERS_ACCOUNT: &str = "ark.extra_headers";
pub const LLM_TEMPERATURE_ACCOUNT: &str = "ark.temperature";
pub const OMNI_API_KEY_ACCOUNT: &str = "omni.api_key";
pub const OMNI_ENDPOINT_ACCOUNT: &str = "omni.endpoint";
pub const OMNI_MODEL_ACCOUNT: &str = "omni.model";
pub const OMNI_EXTRA_HEADERS_ACCOUNT: &str = "omni.extra_headers";
pub const OMNI_TEMPERATURE_ACCOUNT: &str = "omni.temperature";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Asr,
    Llm,
}

impl ChannelKind {
    pub fn parse(value: &str) -> Result<Self, BackendError> {
        match value {
            "asr" => Ok(Self::Asr),
            "llm" => Ok(Self::Llm),
            other => Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                format!("unknown channel kind: {other}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSlot {
    Asr,
    Llm,
    Omni,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSummary {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub order: u32,
    pub last_test: Option<ChannelTestSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelTestSummary {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelMutation {
    /// Commit a prepared local runtime and its channel in one metadata revision.
    ActivateLocalAsr {
        id: Option<String>,
        provider_type: String,
    },
    Create {
        kind: ChannelKind,
        provider_type: String,
        name: String,
    },
    SetProviderType {
        kind: ChannelKind,
        id: String,
        provider_type: String,
    },
    DeleteIfBlank {
        kind: ChannelKind,
        id: String,
    },
    Rename {
        kind: ChannelKind,
        id: String,
        name: String,
    },
    Delete {
        kind: ChannelKind,
        id: String,
    },
    SetEnabled {
        kind: ChannelKind,
        id: String,
        enabled: bool,
    },
    Reorder {
        kind: ChannelKind,
        ids: Vec<String>,
    },
    RecordTest {
        kind: ChannelKind,
        id: String,
        ok: bool,
        latency_ms: Option<u32>,
        at: i64,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelMutationResult {
    Applied,
    Activated(String),
    Created(String),
    DeletedIfBlank(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialNamespace {
    Asr,
    Llm,
    Omni,
    Marketplace,
    Application,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialKey {
    pub namespace: CredentialNamespace,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub account: String,
}

impl CredentialKey {
    pub fn new(
        namespace: CredentialNamespace,
        provider_id: Option<String>,
        account: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let account = account.into();
        if account.trim().is_empty()
            || provider_id
                .as_deref()
                .is_some_and(|provider| provider.trim().is_empty())
        {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "credential account and provider id must not be blank",
            ));
        }
        Ok(Self {
            namespace,
            provider_id,
            account,
        })
    }
}

/// Secret value with deliberately redacted diagnostics and no serde support.
///
/// Hosts may expose the value only from an explicitly authorised settings
/// surface. Core snapshots, events and errors can never serialize this type.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn into_exposed(self) -> String {
        self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

pub trait CredentialStore: Send + Sync {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>>;

    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>>;

    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>>;

    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>>;

    fn list_channels(
        &self,
        _kind: ChannelKind,
    ) -> BoxFuture<'static, Result<Vec<ChannelSummary>, BackendError>> {
        unsupported_credentials()
    }

    fn mutate_channel(
        &self,
        _mutation: ChannelMutation,
    ) -> BoxFuture<'static, Result<ChannelMutationResult, BackendError>> {
        unsupported_credentials()
    }

    fn active_provider(
        &self,
        _slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        unsupported_credentials()
    }

    fn set_active_provider(
        &self,
        _slot: ProviderSlot,
        _provider_id: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported_credentials()
    }
}

/// Persistence-only seam for non-secret channel metadata.
///
/// Implementations project their existing secure payload into
/// [`CredentialMetadata`]; mutation, ordering and active-channel policy stay in
/// [`CredentialDirectory`].
pub trait CredentialMetadataStore: Send + Sync {
    fn load_metadata(&self) -> BoxFuture<'static, Result<CredentialMetadata, BackendError>>;

    fn save_metadata(
        &self,
        metadata: CredentialMetadata,
    ) -> BoxFuture<'static, Result<(), BackendError>>;

    fn channel_has_secrets(
        &self,
        kind: ChannelKind,
        channel_id: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>>;
}

/// Core-owned channel directory. Hosts provide storage; this type owns every
/// channel mutation and persists one metadata revision per successful change.
#[derive(Clone)]
pub struct CredentialDirectory {
    store: std::sync::Arc<dyn CredentialMetadataStore>,
    mutation_gate: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl CredentialDirectory {
    pub fn new(store: std::sync::Arc<dyn CredentialMetadataStore>) -> Self {
        Self {
            store,
            mutation_gate: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> Result<Vec<ChannelSummary>, BackendError> {
        Ok(self.store.load_metadata().await?.list_channels(kind))
    }

    pub async fn active_provider(&self, slot: ProviderSlot) -> Result<String, BackendError> {
        Ok(self.store.load_metadata().await?.active_provider(slot))
    }

    pub async fn mutate_channel(
        &self,
        mutation: ChannelMutation,
    ) -> Result<ChannelMutationResult, BackendError> {
        // Vault load/save are separate asynchronous effects. Serialize the
        // complete read-modify-write, including across cloned directories, so
        // a delayed validation/reorder cannot overwrite a newly created channel.
        let _guard = self.mutation_gate.lock().await;
        let mut metadata = self.store.load_metadata().await?;
        let has_credentials = match &mutation {
            ChannelMutation::DeleteIfBlank { kind, id } => {
                self.store.channel_has_secrets(*kind, id.clone()).await?
            }
            _ => false,
        };
        let result = metadata.apply_channel_mutation(mutation, |_| has_credentials)?;
        if !matches!(result, ChannelMutationResult::DeletedIfBlank(false)) {
            self.store.save_metadata(metadata).await?;
        }
        Ok(result)
    }

    pub async fn set_active_provider(
        &self,
        slot: ProviderSlot,
        provider_id: String,
    ) -> Result<(), BackendError> {
        let _guard = self.mutation_gate.lock().await;
        let mut metadata = self.store.load_metadata().await?;
        let revision = metadata.revision();
        metadata.select_active_provider(slot, provider_id)?;
        if metadata.revision() == revision {
            Ok(())
        } else {
            self.store.save_metadata(metadata).await
        }
    }
}

/// Non-secret provider/channel metadata that can be persisted by any host.
/// Secret values remain in the platform credential vault and are represented
/// here only through the `has_credentials` callback used for blank cleanup.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMetadata {
    #[serde(default)]
    channels: HashMap<ChannelKind, Vec<ChannelSummary>>,
    #[serde(default)]
    active_providers: HashMap<ProviderSlot, String>,
    #[serde(default)]
    revision: u64,
}

impl CredentialMetadata {
    pub fn from_parts(
        asr_channels: Vec<ChannelSummary>,
        llm_channels: Vec<ChannelSummary>,
        active_asr: impl Into<String>,
        active_llm: impl Into<String>,
        active_omni: impl Into<String>,
        revision: u64,
    ) -> Self {
        let mut channels = HashMap::new();
        channels.insert(ChannelKind::Asr, asr_channels);
        channels.insert(ChannelKind::Llm, llm_channels);
        let mut active_providers = HashMap::new();
        active_providers.insert(ProviderSlot::Asr, active_asr.into());
        active_providers.insert(ProviderSlot::Llm, active_llm.into());
        active_providers.insert(ProviderSlot::Omni, active_omni.into());
        Self {
            channels,
            active_providers,
            revision,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn list_channels(&self, kind: ChannelKind) -> Vec<ChannelSummary> {
        let mut channels = self.channels.get(&kind).cloned().unwrap_or_default();
        normalize_channel_order(&mut channels);
        channels
    }

    pub fn active_provider(&self, slot: ProviderSlot) -> String {
        self.active_providers
            .get(&slot)
            .cloned()
            .unwrap_or_default()
    }

    fn assign_active_provider(&mut self, slot: ProviderSlot, provider_id: String) {
        if self.active_providers.get(&slot) != Some(&provider_id) {
            self.active_providers.insert(slot, provider_id);
            self.revision = self.revision.saturating_add(1);
        }
    }

    pub fn select_active_provider(
        &mut self,
        slot: ProviderSlot,
        provider_id: String,
    ) -> Result<(), BackendError> {
        let provider_id = provider_id.trim();
        if provider_id.is_empty() {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "provider channel id must not be blank",
            ));
        }
        let kind = match slot {
            ProviderSlot::Asr => Some(ChannelKind::Asr),
            ProviderSlot::Llm => Some(ChannelKind::Llm),
            ProviderSlot::Omni => None,
        };
        let mut reordered = false;
        if let Some(kind) = kind {
            let channels = self.channels.entry(kind).or_default();
            if let Some(selected) = channels.iter().find(|channel| channel.id == provider_id) {
                if !selected.enabled {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "the selected provider channel is disabled",
                    ));
                }
                normalize_channel_order(channels);
                let previous = channels
                    .iter()
                    .map(|channel| channel.id.clone())
                    .collect::<Vec<_>>();
                let mut ids = vec![provider_id.to_string()];
                ids.extend(
                    channels
                        .iter()
                        .filter(|channel| channel.id != provider_id)
                        .map(|channel| channel.id.clone()),
                );
                for (order, id) in ids.iter().enumerate() {
                    if let Some(channel) = channels.iter_mut().find(|channel| &channel.id == id) {
                        channel.order = order as u32;
                    }
                }
                normalize_channel_order(channels);
                reordered = channels
                    .iter()
                    .map(|channel| channel.id.as_str())
                    .ne(previous.iter().map(String::as_str));
            }
        }
        let revision = self.revision;
        self.assign_active_provider(slot, provider_id.to_string());
        if reordered && self.revision == revision {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(())
    }

    pub fn apply_channel_mutation(
        &mut self,
        mutation: ChannelMutation,
        has_credentials: impl Fn(&str) -> bool,
    ) -> Result<ChannelMutationResult, BackendError> {
        let mutation_kind = match &mutation {
            ChannelMutation::ActivateLocalAsr { .. } => ChannelKind::Asr,
            ChannelMutation::Create { kind, .. }
            | ChannelMutation::SetProviderType { kind, .. }
            | ChannelMutation::DeleteIfBlank { kind, .. }
            | ChannelMutation::Rename { kind, .. }
            | ChannelMutation::Delete { kind, .. }
            | ChannelMutation::SetEnabled { kind, .. }
            | ChannelMutation::Reorder { kind, .. }
            | ChannelMutation::RecordTest { kind, .. } => *kind,
        };
        let slot = slot_for_kind(mutation_kind);
        let active = self.active_provider(slot);
        let active_was_managed = matches!(&mutation, ChannelMutation::ActivateLocalAsr { .. })
            || active.is_empty()
            || self
                .channels
                .get(&mutation_kind)
                .is_some_and(|channels| channels.iter().any(|channel| channel.id == active));
        let (kind, result) = match mutation {
            ChannelMutation::ActivateLocalAsr { id, provider_type } => {
                if provider_type.trim().is_empty() {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "local ASR provider type must not be blank",
                    ));
                }
                let channels = self.channels.entry(ChannelKind::Asr).or_default();
                normalize_channel_order(channels);
                let index = match id {
                    Some(id) => Some(
                        channels
                            .iter()
                            .position(|channel| channel.id == id)
                            .ok_or_else(|| unknown_channel(ChannelKind::Asr, &id))?,
                    ),
                    None => channels
                        .iter()
                        .position(|channel| channel.provider_type == provider_type),
                };
                let mut channel = match index {
                    Some(index) => {
                        if channels[index].provider_type != provider_type {
                            return Err(BackendError::new(
                                BackendErrorCode::InvalidState,
                                "local ASR channel changed during activation",
                            ));
                        }
                        channels.remove(index)
                    }
                    None => ChannelSummary {
                        id: allocate_channel_id(channels, &provider_type),
                        provider_type,
                        name: String::new(),
                        enabled: true,
                        order: 0,
                        last_test: None,
                    },
                };
                channel.enabled = true;
                let id = channel.id.clone();
                channels.insert(0, channel);
                for (order, channel) in channels.iter_mut().enumerate() {
                    channel.order = order as u32;
                }
                (ChannelKind::Asr, ChannelMutationResult::Activated(id))
            }
            ChannelMutation::Create {
                kind,
                provider_type,
                name,
            } => {
                let provider_type = provider_type.trim();
                if provider_type.is_empty() {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "provider type must not be blank",
                    ));
                }
                let channels = self.channels.entry(kind).or_default();
                let id = allocate_channel_id(channels, provider_type);
                let order = channels
                    .iter()
                    .filter(|channel| channel.enabled)
                    .map(|channel| channel.order)
                    .max()
                    .map(|order| order.saturating_add(1))
                    .unwrap_or(0);
                channels.push(ChannelSummary {
                    id: id.clone(),
                    name: name.trim().to_string(),
                    provider_type: provider_type.to_string(),
                    enabled: true,
                    order,
                    last_test: None,
                });
                (kind, ChannelMutationResult::Created(id))
            }
            ChannelMutation::SetProviderType {
                kind,
                id,
                provider_type,
            } => {
                let provider_type = provider_type.trim();
                if provider_type.is_empty() {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "provider type must not be blank",
                    ));
                }
                let channel = find_channel_mut(&mut self.channels, kind, &id)?;
                channel.provider_type = provider_type.to_string();
                channel.last_test = None;
                (kind, ChannelMutationResult::Applied)
            }
            ChannelMutation::DeleteIfBlank { kind, id } => {
                let channels = self.channels.entry(kind).or_default();
                let before = channels.len();
                channels.retain(|channel| {
                    channel.id != id
                        || !channel.name.trim().is_empty()
                        || has_credentials(&channel.id)
                });
                (
                    kind,
                    ChannelMutationResult::DeletedIfBlank(channels.len() != before),
                )
            }
            ChannelMutation::Rename { kind, id, name } => {
                find_channel_mut(&mut self.channels, kind, &id)?.name = name.trim().to_string();
                (kind, ChannelMutationResult::Applied)
            }
            ChannelMutation::Delete { kind, id } => {
                let channels = self.channels.entry(kind).or_default();
                let before = channels.len();
                channels.retain(|channel| channel.id != id);
                if channels.len() == before {
                    return Err(unknown_channel(kind, &id));
                }
                (kind, ChannelMutationResult::Applied)
            }
            ChannelMutation::SetEnabled { kind, id, enabled } => {
                let channels = self.channels.entry(kind).or_default();
                let target_order = if enabled {
                    channels
                        .iter()
                        .filter(|channel| channel.id != id && channel.enabled)
                        .map(|channel| channel.order)
                        .max()
                } else {
                    channels
                        .iter()
                        .filter(|channel| channel.id != id)
                        .map(|channel| channel.order)
                        .max()
                }
                .map(|order| order.saturating_add(1))
                .unwrap_or(0);
                let channel = channels
                    .iter_mut()
                    .find(|channel| channel.id == id)
                    .ok_or_else(|| unknown_channel(kind, &id))?;
                channel.enabled = enabled;
                channel.order = target_order;
                (kind, ChannelMutationResult::Applied)
            }
            ChannelMutation::Reorder { kind, ids } => {
                let channels = self.channels.entry(kind).or_default();
                normalize_channel_order(channels);
                let mut ordered_ids = Vec::with_capacity(channels.len());
                for id in ids {
                    if channels.iter().any(|channel| channel.id == id) && !ordered_ids.contains(&id)
                    {
                        ordered_ids.push(id);
                    }
                }
                for channel in channels.iter() {
                    if !ordered_ids.contains(&channel.id) {
                        ordered_ids.push(channel.id.clone());
                    }
                }
                for (order, id) in ordered_ids.iter().enumerate() {
                    if let Some(channel) = channels.iter_mut().find(|channel| &channel.id == id) {
                        channel.order = order as u32;
                    }
                }
                (kind, ChannelMutationResult::Applied)
            }
            ChannelMutation::RecordTest {
                kind,
                id,
                ok,
                latency_ms,
                at,
                error,
            } => {
                find_channel_mut(&mut self.channels, kind, &id)?.last_test =
                    Some(ChannelTestSummary {
                        ok,
                        latency_ms,
                        at,
                        error,
                    });
                (kind, ChannelMutationResult::Applied)
            }
        };
        normalize_channel_order(self.channels.entry(kind).or_default());
        if !matches!(result, ChannelMutationResult::DeletedIfBlank(false)) {
            self.revision = self.revision.saturating_add(1);
            if active_was_managed {
                self.sync_active(kind);
            }
        }
        Ok(result)
    }

    fn sync_active(&mut self, kind: ChannelKind) {
        let slot = slot_for_kind(kind);
        let active = self
            .channels
            .get(&kind)
            .and_then(|channels| channels.iter().find(|channel| channel.enabled))
            .map(|channel| channel.id.clone())
            .unwrap_or_default();
        self.active_providers.insert(slot, active);
    }
}

fn slot_for_kind(kind: ChannelKind) -> ProviderSlot {
    match kind {
        ChannelKind::Asr => ProviderSlot::Asr,
        ChannelKind::Llm => ProviderSlot::Llm,
    }
}

#[derive(Default)]
pub struct InMemoryCredentialStore {
    values: RwLock<HashMap<CredentialKey, SecretValue>>,
    status: RwLock<CredentialsStatus>,
    metadata: RwLock<CredentialMetadata>,
}

impl InMemoryCredentialStore {
    pub fn set_status(&self, status: CredentialsStatus) {
        *self
            .status
            .write()
            .expect("credential status lock poisoned") = status;
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        let mut status = self
            .status
            .read()
            .expect("credential status lock poisoned")
            .clone();
        status.pipeline_mode = crate::shared_types::effective_pipeline_mode(
            preferences.multimodal_pipeline_enabled,
            preferences.pipeline_mode,
        );
        if status.pipeline_mode != crate::shared_types::PipelineMode::Multimodal {
            status.omni_configured = false;
        }
        Box::pin(async move { Ok(status) })
    }

    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        let value = self
            .values
            .read()
            .expect("credential values lock poisoned")
            .get(&key)
            .cloned();
        Box::pin(async move { Ok(value) })
    }

    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.values
            .write()
            .expect("credential values lock poisoned")
            .insert(key, value);
        Box::pin(async { Ok(()) })
    }

    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        self.values
            .write()
            .expect("credential values lock poisoned")
            .remove(&key);
        Box::pin(async { Ok(()) })
    }

    fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> BoxFuture<'static, Result<Vec<ChannelSummary>, BackendError>> {
        let channels = self
            .metadata
            .read()
            .expect("credential metadata lock poisoned")
            .list_channels(kind);
        Box::pin(async move { Ok(channels) })
    }

    fn mutate_channel(
        &self,
        mutation: ChannelMutation,
    ) -> BoxFuture<'static, Result<ChannelMutationResult, BackendError>> {
        let values = self.values.read().expect("credential values lock poisoned");
        let result = self
            .metadata
            .write()
            .expect("credential metadata lock poisoned")
            .apply_channel_mutation(mutation, |id| {
                values
                    .keys()
                    .any(|key| key.provider_id.as_deref() == Some(id))
            });
        Box::pin(async move { result })
    }

    fn active_provider(
        &self,
        slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let provider = self
            .metadata
            .read()
            .expect("credential metadata lock poisoned")
            .active_provider(slot);
        Box::pin(async move { Ok(provider) })
    }

    fn set_active_provider(
        &self,
        slot: ProviderSlot,
        provider_id: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self
            .metadata
            .write()
            .expect("credential metadata lock poisoned")
            .select_active_provider(slot, provider_id);
        Box::pin(async move { result })
    }
}

impl CredentialMetadataStore for InMemoryCredentialStore {
    fn load_metadata(&self) -> BoxFuture<'static, Result<CredentialMetadata, BackendError>> {
        let metadata = self
            .metadata
            .read()
            .expect("credential metadata lock poisoned")
            .clone();
        Box::pin(async move { Ok(metadata) })
    }

    fn save_metadata(
        &self,
        metadata: CredentialMetadata,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        *self
            .metadata
            .write()
            .expect("credential metadata lock poisoned") = metadata;
        Box::pin(async { Ok(()) })
    }

    fn channel_has_secrets(
        &self,
        _kind: ChannelKind,
        channel_id: String,
    ) -> BoxFuture<'static, Result<bool, BackendError>> {
        let has_secrets = self
            .values
            .read()
            .expect("credential values lock poisoned")
            .keys()
            .any(|key| key.provider_id.as_deref() == Some(channel_id.as_str()));
        Box::pin(async move { Ok(has_secrets) })
    }
}

fn find_channel_mut<'a>(
    all_channels: &'a mut HashMap<ChannelKind, Vec<ChannelSummary>>,
    kind: ChannelKind,
    id: &str,
) -> Result<&'a mut ChannelSummary, BackendError> {
    all_channels
        .entry(kind)
        .or_default()
        .iter_mut()
        .find(|channel| channel.id == id)
        .ok_or_else(|| unknown_channel(kind, id))
}

fn unknown_channel(kind: ChannelKind, id: &str) -> BackendError {
    BackendError::new(
        BackendErrorCode::InvalidArgument,
        format!("unknown {kind:?} channel: {id}"),
    )
}

fn allocate_channel_id(channels: &[ChannelSummary], provider_type: &str) -> String {
    if !channels.iter().any(|channel| channel.id == provider_type) {
        return provider_type.to_string();
    }
    for suffix in 2..u32::MAX {
        let candidate = format!("{provider_type}-{suffix}");
        if !channels.iter().any(|channel| channel.id == candidate) {
            return candidate;
        }
    }
    unreachable!("channel id space exhausted")
}

fn normalize_channel_order(channels: &mut [ChannelSummary]) {
    channels.sort_by_key(|channel| (!channel.enabled, channel.order, channel.id.clone()));
    for (order, channel) in channels.iter_mut().enumerate() {
        channel.order = order as u32;
    }
}

pub struct UnsupportedCredentialStore;

impl CredentialStore for UnsupportedCredentialStore {
    fn status(
        &self,
        preferences: UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        Box::pin(async move {
            Ok(CredentialsStatus {
                pipeline_mode: crate::shared_types::effective_pipeline_mode(
                    preferences.multimodal_pipeline_enabled,
                    preferences.pipeline_mode,
                ),
                ..CredentialsStatus::default()
            })
        })
    }

    fn read(
        &self,
        _key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        unsupported_credentials()
    }

    fn write(
        &self,
        _key: CredentialKey,
        _value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported_credentials()
    }

    fn remove(&self, _key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported_credentials()
    }
}

fn unsupported_credentials<T>() -> BoxFuture<'static, Result<T, BackendError>> {
    Box::pin(async {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "credential store is not configured",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, order: u32, enabled: bool) -> ChannelSummary {
        ChannelSummary {
            id: id.to_string(),
            name: String::new(),
            provider_type: id.to_string(),
            enabled,
            order,
            last_test: None,
        }
    }

    #[test]
    fn channel_ids_and_active_order_match_the_legacy_directory_contract() {
        let mut metadata = CredentialMetadata::default();

        let first = metadata
            .apply_channel_mutation(
                ChannelMutation::Create {
                    kind: ChannelKind::Llm,
                    provider_type: "deepseek".to_string(),
                    name: "primary".to_string(),
                },
                |_| false,
            )
            .unwrap();
        let second = metadata
            .apply_channel_mutation(
                ChannelMutation::Create {
                    kind: ChannelKind::Llm,
                    provider_type: "deepseek".to_string(),
                    name: "backup".to_string(),
                },
                |_| false,
            )
            .unwrap();

        assert_eq!(
            first,
            ChannelMutationResult::Created("deepseek".to_string())
        );
        assert_eq!(
            second,
            ChannelMutationResult::Created("deepseek-2".to_string())
        );
        assert_eq!(metadata.active_provider(ProviderSlot::Llm), "deepseek");
        assert_eq!(metadata.revision(), 2);

        metadata
            .apply_channel_mutation(
                ChannelMutation::Reorder {
                    kind: ChannelKind::Llm,
                    ids: vec!["deepseek-2".to_string(), "deepseek".to_string()],
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(metadata.active_provider(ProviderSlot::Llm), "deepseek-2");

        metadata
            .apply_channel_mutation(
                ChannelMutation::SetEnabled {
                    kind: ChannelKind::Llm,
                    id: "deepseek-2".to_string(),
                    enabled: false,
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(metadata.active_provider(ProviderSlot::Llm), "deepseek");
        assert_eq!(
            metadata
                .list_channels(ChannelKind::Llm)
                .into_iter()
                .map(|channel| (channel.id, channel.enabled))
                .collect::<Vec<_>>(),
            vec![
                ("deepseek".to_string(), true),
                ("deepseek-2".to_string(), false)
            ]
        );

        metadata
            .apply_channel_mutation(
                ChannelMutation::Delete {
                    kind: ChannelKind::Llm,
                    id: "deepseek".to_string(),
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(metadata.active_provider(ProviderSlot::Llm), "");
        metadata
            .apply_channel_mutation(
                ChannelMutation::SetEnabled {
                    kind: ChannelKind::Llm,
                    id: "deepseek-2".to_string(),
                    enabled: true,
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(metadata.active_provider(ProviderSlot::Llm), "deepseek-2");
    }

    #[test]
    fn partial_reorder_preserves_unlisted_channel_order_and_blank_delete_is_safe() {
        let mut metadata = CredentialMetadata::from_parts(
            vec![],
            vec![
                summary("a", 0, true),
                summary("b", 1, true),
                summary("c", 2, true),
            ],
            "",
            "a",
            "custom",
            7,
        );

        metadata
            .apply_channel_mutation(
                ChannelMutation::Reorder {
                    kind: ChannelKind::Llm,
                    ids: vec!["c".to_string(), "a".to_string()],
                },
                |_| false,
            )
            .unwrap();
        assert_eq!(
            metadata
                .list_channels(ChannelKind::Llm)
                .into_iter()
                .map(|channel| channel.id)
                .collect::<Vec<_>>(),
            vec!["c".to_string(), "a".to_string(), "b".to_string()]
        );

        assert_eq!(
            metadata
                .apply_channel_mutation(
                    ChannelMutation::DeleteIfBlank {
                        kind: ChannelKind::Llm,
                        id: "a".to_string(),
                    },
                    |id| id == "a",
                )
                .unwrap(),
            ChannelMutationResult::DeletedIfBlank(false)
        );
        assert_eq!(metadata.revision(), 8);
    }

    #[tokio::test]
    async fn in_memory_store_round_trips_secrets_without_exposing_debug_or_serde() {
        let store = InMemoryCredentialStore::default();
        let key = CredentialKey::new(
            CredentialNamespace::Asr,
            Some("fixture".to_string()),
            "api_key",
        )
        .unwrap();
        let secret = SecretValue::new("do-not-log-this");
        assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");

        store.write(key.clone(), secret).await.unwrap();
        assert_eq!(
            store
                .read(key.clone())
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "do-not-log-this"
        );
        store.remove(key.clone()).await.unwrap();
        assert!(store.read(key).await.unwrap().is_none());
    }

    #[test]
    fn credential_keys_reject_blank_identifiers() {
        assert_eq!(
            CredentialKey::new(CredentialNamespace::Llm, None, " ")
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidArgument
        );
        assert_eq!(
            CredentialKey::new(CredentialNamespace::Llm, Some(" ".to_string()), "api_key")
                .unwrap_err()
                .code,
            BackendErrorCode::InvalidArgument
        );
    }

    #[test]
    fn provider_identifiers_are_distinct_validated_wire_values() {
        let channel = ProviderChannelId::new(" channel-a ").unwrap();
        let provider = ProviderType::new(" openai ").unwrap();
        assert_eq!(channel.as_str(), "channel-a");
        assert_eq!(provider.as_str(), "openai");
        assert_eq!(serde_json::to_string(&channel).unwrap(), r#""channel-a""#);
        assert!(serde_json::from_str::<ProviderChannelId>(r#"" ""#).is_err());
        assert!(serde_json::from_str::<ProviderType>(r#""""#).is_err());
    }

    #[tokio::test]
    async fn credential_directory_owns_mutation_while_repository_only_persists() {
        let repository = std::sync::Arc::new(InMemoryCredentialStore::default());
        let metadata_store: std::sync::Arc<dyn CredentialMetadataStore> = repository.clone();
        let directory = CredentialDirectory::new(metadata_store);

        let created = directory
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Asr,
                provider_type: "openai-compatible".into(),
                name: "Local".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            created,
            ChannelMutationResult::Created("openai-compatible".into())
        );
        assert_eq!(
            directory.active_provider(ProviderSlot::Asr).await.unwrap(),
            "openai-compatible"
        );
        let second = directory
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Asr,
                provider_type: "openai-compatible".into(),
                name: "Backup".into(),
            })
            .await
            .unwrap();
        let ChannelMutationResult::Created(second) = second else {
            panic!("second channel was not created");
        };
        directory
            .set_active_provider(ProviderSlot::Asr, second.clone())
            .await
            .unwrap();
        assert_eq!(
            directory.active_provider(ProviderSlot::Asr).await.unwrap(),
            second
        );
        assert_eq!(
            directory.list_channels(ChannelKind::Asr).await.unwrap()[0].name,
            "Backup"
        );
        assert_eq!(repository.load_metadata().await.unwrap().revision(), 3);
    }

    #[tokio::test]
    async fn credential_directory_serializes_concurrent_read_modify_write() {
        struct YieldingMetadataStore(std::sync::Arc<InMemoryCredentialStore>);

        impl CredentialMetadataStore for YieldingMetadataStore {
            fn load_metadata(
                &self,
            ) -> BoxFuture<'static, Result<CredentialMetadata, BackendError>> {
                let repository = self.0.clone();
                Box::pin(async move {
                    let snapshot = repository.load_metadata().await?;
                    // Match the real vault's async I/O boundary: two callers
                    // can read the same revision before either one saves it.
                    tokio::task::yield_now().await;
                    Ok(snapshot)
                })
            }

            fn save_metadata(
                &self,
                metadata: CredentialMetadata,
            ) -> BoxFuture<'static, Result<(), BackendError>> {
                self.0.save_metadata(metadata)
            }

            fn channel_has_secrets(
                &self,
                kind: ChannelKind,
                id: String,
            ) -> BoxFuture<'static, Result<bool, BackendError>> {
                self.0.channel_has_secrets(kind, id)
            }
        }

        let repository = std::sync::Arc::new(InMemoryCredentialStore::default());
        let directory =
            CredentialDirectory::new(std::sync::Arc::new(YieldingMetadataStore(repository)));
        let other = directory.clone();
        let create = |name: &str| ChannelMutation::Create {
            kind: ChannelKind::Llm,
            provider_type: "openai".into(),
            name: name.into(),
        };
        let (first, second) = tokio::join!(
            directory.mutate_channel(create("first")),
            other.mutate_channel(create("second")),
        );
        assert_ne!(
            first.unwrap(),
            second.unwrap(),
            "concurrent creates need distinct channel ids"
        );
        let channels = directory.list_channels(ChannelKind::Llm).await.unwrap();
        assert_eq!(
            channels.len(),
            2,
            "neither channel may be lost by a stale save"
        );
        let (renamed, activated) = tokio::join!(
            directory.mutate_channel(ChannelMutation::Rename {
                kind: ChannelKind::Llm,
                id: channels[0].id.clone(),
                name: "renamed".into(),
            }),
            other.set_active_provider(ProviderSlot::Llm, channels[1].id.clone()),
        );
        renamed.unwrap();
        activated.unwrap();
        assert_eq!(
            directory.active_provider(ProviderSlot::Llm).await.unwrap(),
            channels[1].id
        );
        assert!(directory
            .list_channels(ChannelKind::Llm)
            .await
            .unwrap()
            .iter()
            .any(|channel| channel.name == "renamed"));
    }
}
