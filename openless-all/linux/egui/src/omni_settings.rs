use openless_core::credentials::{
    OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_EXTRA_HEADERS_ACCOUNT, OMNI_MODEL_ACCOUNT,
    OMNI_TEMPERATURE_ACCOUNT,
};
use openless_core::{
    BackendError, CredentialKey, CredentialNamespace, OpenLessBackend, ProviderKind, ProviderSlot,
    SecretValue,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OmniField {
    Model,
    Endpoint,
    ApiKey,
    Temperature,
    ExtraHeaders,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OmniAction {
    Field(OmniField, String),
    Provider(String),
    Save,
    ClearSecrets,
    Models,
    Validate,
    Reload,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OmniPreset {
    pub id: String,
    pub label_key: String,
    pub endpoint: String,
    pub model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OmniSettings {
    pub provider: String,
    pub presets: Vec<OmniPreset>,
    pub model: String,
    pub endpoint: String,
    pub api_key: String,
    pub temperature: String,
    pub extra_headers: String,
    pub models: Vec<String>,
    pub loaded: bool,
    pub busy: bool,
    pub switching: bool,
    pub revision: u64,
    pub dirty: bool,
    pub notice: Option<String>,
}
impl Default for OmniSettings {
    fn default() -> Self {
        Self {
            provider: "custom".into(),
            presets: openless_core::provider_rules::provider_descriptors(ProviderKind::Omni)
                .into_iter()
                .map(|d| OmniPreset {
                    id: d.provider_type.as_str().into(),
                    label_key: d.label_key,
                    endpoint: d.default_endpoint.unwrap_or_default(),
                    model: d.default_model.unwrap_or_default(),
                })
                .collect(),
            model: String::new(),
            endpoint: String::new(),
            api_key: String::new(),
            temperature: String::new(),
            extra_headers: String::new(),
            models: Vec::new(),
            loaded: false,
            busy: false,
            switching: false,
            revision: 0,
            dirty: false,
            notice: None,
        }
    }
}
impl OmniSettings {
    pub fn validate(&self) -> Result<(), BackendError> {
        if self.provider == "custom" {
            openless_core::parse_temperature(&self.temperature)?;
            openless_core::provider_rules::parse_extra_headers(&self.extra_headers)?;
        }
        Ok(())
    }
    pub fn edit(&mut self, field: OmniField, value: String) {
        match field {
            OmniField::Model => self.model = value,
            OmniField::Endpoint => {
                self.endpoint = value;
                self.models.clear();
            }
            OmniField::ApiKey => self.api_key = value,
            OmniField::Temperature => self.temperature = value,
            OmniField::ExtraHeaders => self.extra_headers = value,
        }
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
        self.notice = None;
    }
    pub fn merge_receipt(&self, submitted: &Self, mut stored: Self) -> Self {
        if self.provider == stored.provider && self.revision != submitted.revision {
            if self.model != submitted.model {
                stored.model = self.model.clone();
            }
            if self.endpoint != submitted.endpoint {
                stored.endpoint = self.endpoint.clone();
                stored.models.clear();
            }
            if self.api_key != submitted.api_key {
                stored.api_key = self.api_key.clone();
            }
            if self.temperature != submitted.temperature {
                stored.temperature = self.temperature.clone();
            }
            if self.extra_headers != submitted.extra_headers {
                stored.extra_headers = self.extra_headers.clone();
            }
            stored.revision = self.revision;
            stored.dirty = true;
        }
        stored
    }
    pub async fn load(backend: &OpenLessBackend) -> Result<Self, BackendError> {
        let mut state = Self::default();
        let provider = backend.get_preferences().active_omni_provider;
        if state.presets.iter().any(|p| p.id == provider) {
            state.provider = provider;
        }
        state.model = read(backend, &state.provider, OMNI_MODEL_ACCOUNT).await?;
        state.endpoint = read(backend, &state.provider, OMNI_ENDPOINT_ACCOUNT).await?;
        if state.provider == "custom" {
            state.temperature = read(backend, &state.provider, OMNI_TEMPERATURE_ACCOUNT).await?;
        }
        // 与 Tauri 的 `CredentialField` 一致：把已存的密钥读进表单（输入框按密码
        // 掩码显示），否则用户每次打开都以为「密钥没保存」，测试完刷新一次就变空。
        state.api_key = read(backend, &state.provider, OMNI_API_KEY_ACCOUNT).await?;
        if state.provider == "custom" {
            state.extra_headers =
                read(backend, &state.provider, OMNI_EXTRA_HEADERS_ACCOUNT).await?;
        }
        state.loaded = true;
        Ok(state)
    }
    pub async fn save(&self, backend: &OpenLessBackend) -> Result<(), BackendError> {
        self.validate()?;
        put(backend, &self.provider, OMNI_MODEL_ACCOUNT, &self.model).await?;
        put(
            backend,
            &self.provider,
            OMNI_ENDPOINT_ACCOUNT,
            &self.endpoint,
        )
        .await?;
        if !self.api_key.trim().is_empty() {
            put(backend, &self.provider, OMNI_API_KEY_ACCOUNT, &self.api_key).await?;
        }
        if self.provider == "custom" {
            put(
                backend,
                &self.provider,
                OMNI_TEMPERATURE_ACCOUNT,
                &self.temperature,
            )
            .await?;
            if !self.extra_headers.trim().is_empty() {
                put(
                    backend,
                    &self.provider,
                    OMNI_EXTRA_HEADERS_ACCOUNT,
                    &self.extra_headers,
                )
                .await?;
            }
        }
        Ok(())
    }
    pub async fn select(backend: &OpenLessBackend, id: &str) -> Result<(), BackendError> {
        let descriptor = openless_core::provider_rules::provider_descriptor(ProviderKind::Omni, id)
            .ok_or_else(|| {
                BackendError::new(
                    openless_core::BackendErrorCode::InvalidArgument,
                    "unknown Omni provider",
                )
            })?;
        backend
            .set_active_provider(ProviderSlot::Omni, id.into())
            .await?;
        if id != "custom" {
            if let Some(endpoint) = descriptor.default_endpoint {
                put(backend, id, OMNI_ENDPOINT_ACCOUNT, &endpoint).await?;
            }
            if let Some(model) = descriptor.default_model {
                put(backend, id, OMNI_MODEL_ACCOUNT, &model).await?;
            }
        }
        Ok(())
    }
    pub async fn clear_secrets(&self, backend: &OpenLessBackend) -> Result<(), BackendError> {
        for account in [OMNI_API_KEY_ACCOUNT, OMNI_EXTRA_HEADERS_ACCOUNT] {
            put(backend, &self.provider, account, "").await?;
        }
        Ok(())
    }
}
fn key(provider: Option<String>, account: &str) -> Result<CredentialKey, BackendError> {
    CredentialKey::new(CredentialNamespace::Omni, provider, account)
}
async fn read(
    backend: &OpenLessBackend,
    provider: &str,
    account: &str,
) -> Result<String, BackendError> {
    // Use the same provider-scoped key as the production Omni pipeline.
    // Legacy import is handled by the credential store, not by UI fallbacks.
    let value = backend
        .read_credential(key(Some(provider.into()), account)?)
        .await?;
    Ok(value.map(SecretValue::into_exposed).unwrap_or_default())
}
async fn put(
    backend: &OpenLessBackend,
    provider: &str,
    account: &str,
    value: &str,
) -> Result<(), BackendError> {
    if value.trim().is_empty() {
        backend
            .remove_credential(key(Some(provider.into()), account)?)
            .await?;
    } else {
        backend
            .set_credential(
                key(Some(provider.into()), account)?,
                SecretValue::new(value.trim()),
            )
            .await?;
    }
    Ok(())
}
