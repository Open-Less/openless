//! Read-only conversion of the 1.x desktop vault into the shared credential contract.
//!
//! The host owns Secret Service/file access and the destination transaction. This
//! decoder knows the old account names and JSON shape, but never deletes a source
//! or marks a migration complete. Secret-bearing input/output deliberately has no
//! `Debug` or `Serialize` implementation, including parse errors.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::credentials::*;
use crate::{BackendError, BackendErrorCode, MARKETPLACE_GITHUB_TOKEN_ACCOUNT};

pub const LEGACY_CREDENTIAL_SERVICE: &str = "com.openless.app";
pub const LEGACY_CREDENTIAL_ACCOUNT: &str = "credentials.v1";

pub struct LegacyCredentials {
    pub metadata: CredentialMetadata,
    pub secrets: Vec<(CredentialKey, SecretValue)>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct LegacyRoot {
    version: u32,
    active: LegacyActive,
    providers: LegacyProviders,
    omni: LegacyOmni,
    metadata_revision: u64,
    marketplace: LegacyMarketplace,
}

#[derive(Deserialize)]
#[serde(default)]
struct LegacyActive {
    asr: String,
    llm: String,
}

impl Default for LegacyActive {
    fn default() -> Self {
        // These are the old Linux/macOS defaults; Windows' local Foundry preset
        // must not be introduced merely because this pure decoder is tested there.
        Self {
            asr: "volcengine".into(),
            llm: "ark".into(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct LegacyProviders {
    asr: BTreeMap<String, LegacyEntry>,
    llm: BTreeMap<String, LegacyEntry>,
}

#[derive(Deserialize)]
#[serde(default)]
struct LegacyOmni {
    active: String,
    providers: BTreeMap<String, LegacyEntry>,
}

impl Default for LegacyOmni {
    fn default() -> Self {
        Self {
            active: "custom".into(),
            providers: BTreeMap::new(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyMarketplace {
    github_access_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacyEntry {
    provider_type: Option<String>,
    display_name: Option<String>,
    order: Option<u32>,
    enabled: bool,
    last_test: Option<ChannelTestSummary>,
    api_key: Option<String>,
    #[serde(rename = "baseURL")]
    base_url: Option<String>,
    model: Option<String>,
    app_key: Option<String>,
    access_key: Option<String>,
    resource_id: Option<String>,
    auth_mode: Option<String>,
    volcengine_api_key: Option<String>,
    vocabulary_id: Option<String>,
    advanced_config: Option<String>,
    xfyun_app_id: Option<String>,
    xfyun_api_key: Option<String>,
    temperature: Option<f64>,
    extra_headers: Option<BTreeMap<String, String>>,
}

impl Default for LegacyEntry {
    fn default() -> Self {
        Self {
            provider_type: None,
            display_name: None,
            order: None,
            enabled: true,
            last_test: None,
            api_key: None,
            base_url: None,
            model: None,
            app_key: None,
            access_key: None,
            resource_id: None,
            auth_mode: None,
            volcengine_api_key: None,
            vocabulary_id: None,
            advanced_config: None,
            xfyun_app_id: None,
            xfyun_api_key: None,
            temperature: None,
            extra_headers: None,
        }
    }
}

impl LegacyEntry {
    fn has_content(&self) -> bool {
        [
            &self.display_name,
            &self.api_key,
            &self.base_url,
            &self.model,
            &self.app_key,
            &self.access_key,
            &self.resource_id,
            &self.auth_mode,
            &self.volcengine_api_key,
            &self.vocabulary_id,
            &self.advanced_config,
            &self.xfyun_app_id,
            &self.xfyun_api_key,
        ]
        .into_iter()
        .any(|value| value.as_deref().is_some_and(|value| !value.is_empty()))
            || self.temperature.is_some()
            || self
                .extra_headers
                .as_ref()
                .is_some_and(|headers| !headers.is_empty())
    }
}

#[derive(Deserialize)]
struct ChunkManifest {
    openless_credentials_storage: String,
    version: u32,
    generation: Option<String>,
    chunks: usize,
}

#[derive(Deserialize)]
struct StorageHeader {
    openless_credentials_storage: Option<String>,
}

fn invalid_legacy() -> BackendError {
    // serde's diagnostic may quote a rejected string value. Never interpolate it
    // here: old JSON contains tokens, including arbitrary secret extra headers.
    BackendError::new(
        BackendErrorCode::Persistence,
        "invalid legacy credential payload",
    )
}

/// Read either a direct old JSON entry or both generations of chunk manifests.
/// Missing/locked chunks are errors, not an empty vault: the host must retry
/// rather than commit an incomplete migration or fall back to a stale file.
pub fn read_legacy_vault_payload(
    mut read: impl FnMut(&str) -> Result<Option<SecretValue>, BackendError>,
) -> Result<Option<SecretValue>, BackendError> {
    let Some(entry) = read(LEGACY_CREDENTIAL_ACCOUNT)? else {
        return Ok(None);
    };
    let header: StorageHeader =
        serde_json::from_str(entry.expose_secret()).map_err(|_| invalid_legacy())?;
    if header.openless_credentials_storage.is_none() {
        // Full validation is done by decode_legacy_credentials; malformed JSON
        // must not silently trigger another source with different active channels.
        return Ok(Some(entry));
    }
    let manifest: ChunkManifest =
        serde_json::from_str(entry.expose_secret()).map_err(|_| invalid_legacy())?;
    if manifest.openless_credentials_storage != "chunked"
        || manifest.version != 1
        || manifest.chunks == 0
        || manifest.chunks > 4096
    {
        return Err(invalid_legacy());
    }
    let mut payload = String::new();
    for index in 0..manifest.chunks {
        let account = match &manifest.generation {
            Some(generation) => format!("credentials.v1.chunk.{generation}.{index}"),
            None => format!("credentials.v1.chunk.{index}"),
        };
        let chunk = read(&account)?.ok_or_else(invalid_legacy)?;
        payload.push_str(chunk.expose_secret());
    }
    Ok(Some(SecretValue::new(payload)))
}

/// Decode the actual desktop v1/v2 schema, preserving channel IDs independently
/// of protocol/provider types. A provider-less v1 entry uses its original map key
/// as both values, exactly as the original desktop migration did.
pub fn decode_legacy_credentials(payload: &str) -> Result<LegacyCredentials, BackendError> {
    let header: StorageHeader = serde_json::from_str(payload).map_err(|_| invalid_legacy())?;
    if header.openless_credentials_storage.is_some() {
        return Err(invalid_legacy());
    }
    let root: LegacyRoot = serde_json::from_str(payload).map_err(|_| invalid_legacy())?;
    if root.version > 2 {
        return Err(invalid_legacy());
    }
    let mut secrets = Vec::new();
    let (asr_channels, active_asr) = decode_channels(
        root.providers.asr,
        root.active.asr,
        CredentialNamespace::Asr,
        &mut secrets,
    )?;
    let (llm_channels, active_llm) = decode_channels(
        root.providers.llm,
        root.active.llm,
        CredentialNamespace::Llm,
        &mut secrets,
    )?;
    for (id, entry) in root.omni.providers {
        decode_entry(id, entry, CredentialNamespace::Omni, &mut secrets)?;
    }
    if let Some(token) = root
        .marketplace
        .github_access_token
        .filter(|value| !value.is_empty())
    {
        secrets.push((
            CredentialKey::new(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT,
            )?,
            SecretValue::new(token),
        ));
    }
    Ok(LegacyCredentials {
        metadata: CredentialMetadata::from_parts(
            asr_channels,
            llm_channels,
            active_asr,
            active_llm,
            root.omni.active,
            root.metadata_revision,
        ),
        secrets,
    })
}

fn decode_channels(
    entries: BTreeMap<String, LegacyEntry>,
    active: String,
    namespace: CredentialNamespace,
    secrets: &mut Vec<(CredentialKey, SecretValue)>,
) -> Result<(Vec<ChannelSummary>, String), BackendError> {
    let mut entries: Vec<_> = entries.into_iter().collect();
    // v1 did not persist order. Keep its active entry first; deterministic IDs
    // retain that ordering across interrupted migrations and upgrades.
    entries.sort_by_key(|(id, entry)| (id != &active, !entry.has_content(), id.clone()));
    let mut channels = Vec::new();
    for (index, (id, entry)) in entries.into_iter().enumerate() {
        let provider_type = entry.provider_type.clone().unwrap_or_else(|| id.clone());
        ProviderChannelId::new(id.clone())?;
        ProviderType::new(provider_type.clone())?;
        channels.push(ChannelSummary {
            id: id.clone(),
            name: entry.display_name.clone().unwrap_or_default(),
            provider_type,
            enabled: entry.enabled,
            order: entry.order.unwrap_or(index as u32),
            last_test: entry.last_test.clone(),
        });
        decode_entry(id, entry, namespace, secrets)?;
    }
    channels.sort_by_key(|channel| (channel.order, channel.id.clone()));
    let active = if channels.is_empty()
        || channels
            .iter()
            .any(|channel| channel.id == active && channel.enabled)
    {
        active
    } else {
        channels
            .iter()
            .find(|channel| channel.enabled)
            .map(|channel| channel.id.clone())
            .unwrap_or_default()
    };
    Ok((channels, active))
}

fn decode_entry(
    id: String,
    entry: LegacyEntry,
    namespace: CredentialNamespace,
    secrets: &mut Vec<(CredentialKey, SecretValue)>,
) -> Result<(), BackendError> {
    let (api_key, endpoint, model, extra_headers, temperature) = match namespace {
        CredentialNamespace::Asr => (
            ASR_API_KEY_ACCOUNT,
            ASR_ENDPOINT_ACCOUNT,
            ASR_MODEL_ACCOUNT,
            None,
            None,
        ),
        CredentialNamespace::Llm => (
            LLM_API_KEY_ACCOUNT,
            LLM_ENDPOINT_ACCOUNT,
            LLM_MODEL_ACCOUNT,
            Some(LLM_EXTRA_HEADERS_ACCOUNT),
            Some(LLM_TEMPERATURE_ACCOUNT),
        ),
        CredentialNamespace::Omni => (
            OMNI_API_KEY_ACCOUNT,
            OMNI_ENDPOINT_ACCOUNT,
            OMNI_MODEL_ACCOUNT,
            Some(OMNI_EXTRA_HEADERS_ACCOUNT),
            Some(OMNI_TEMPERATURE_ACCOUNT),
        ),
        _ => return Err(invalid_legacy()),
    };
    let mut fields = vec![
        (api_key, entry.api_key),
        (endpoint, entry.base_url),
        (model, entry.model),
    ];
    if namespace == CredentialNamespace::Asr {
        fields.extend([
            (VOLCENGINE_APP_KEY_ACCOUNT, entry.app_key),
            (VOLCENGINE_ACCESS_KEY_ACCOUNT, entry.access_key),
            (VOLCENGINE_RESOURCE_ID_ACCOUNT, entry.resource_id),
            (VOLCENGINE_AUTH_MODE_ACCOUNT, entry.auth_mode),
            (VOLCENGINE_API_KEY_ACCOUNT, entry.volcengine_api_key),
            (ASR_VOCABULARY_ID_ACCOUNT, entry.vocabulary_id),
            (ASR_ADVANCED_CONFIG_ACCOUNT, entry.advanced_config),
            (XFYUN_APP_ID_ACCOUNT, entry.xfyun_app_id),
            (XFYUN_API_KEY_ACCOUNT, entry.xfyun_api_key),
        ]);
    }
    if let Some(account) = extra_headers {
        if let Some(headers) = entry.extra_headers.filter(|headers| !headers.is_empty()) {
            fields.push((
                account,
                Some(serde_json::to_string(&headers).map_err(|_| invalid_legacy())?),
            ));
        }
    }
    if let Some(account) = temperature {
        if let Some(value) = entry
            .temperature
            .filter(|value| value.is_finite() && (0.0..=2.0).contains(value))
        {
            fields.push((account, Some(value.to_string())));
        }
    }
    for (account, value) in fields {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            secrets.push((
                CredentialKey::new(namespace, Some(id.clone()), account)?,
                SecretValue::new(value),
            ));
        }
    }
    Ok(())
}

/// Before the JSON vault, 1.x stored each account directly under the old service.
/// Those accounts route to the historical active Linux presets, not new channel
/// UUIDs. No entries means no source, so a fresh install need not write a marker.
pub fn read_legacy_accounts(
    mut read: impl FnMut(&str) -> Result<Option<SecretValue>, BackendError>,
) -> Result<Option<LegacyCredentials>, BackendError> {
    let mut secrets = Vec::new();
    for (namespace, provider, accounts) in [
        (
            CredentialNamespace::Asr,
            "volcengine",
            &[
                VOLCENGINE_APP_KEY_ACCOUNT,
                VOLCENGINE_ACCESS_KEY_ACCOUNT,
                VOLCENGINE_RESOURCE_ID_ACCOUNT,
                VOLCENGINE_AUTH_MODE_ACCOUNT,
                VOLCENGINE_API_KEY_ACCOUNT,
                ASR_API_KEY_ACCOUNT,
                ASR_ENDPOINT_ACCOUNT,
                ASR_MODEL_ACCOUNT,
                ASR_VOCABULARY_ID_ACCOUNT,
                ASR_ADVANCED_CONFIG_ACCOUNT,
                XFYUN_APP_ID_ACCOUNT,
                XFYUN_API_KEY_ACCOUNT,
            ][..],
        ),
        (
            CredentialNamespace::Llm,
            "ark",
            &[LLM_API_KEY_ACCOUNT, LLM_MODEL_ACCOUNT, LLM_ENDPOINT_ACCOUNT][..],
        ),
        (
            CredentialNamespace::Omni,
            "custom",
            &[
                OMNI_API_KEY_ACCOUNT,
                OMNI_ENDPOINT_ACCOUNT,
                OMNI_MODEL_ACCOUNT,
            ][..],
        ),
    ] {
        for account in accounts {
            if let Some(value) = read(account)?.filter(|value| !value.expose_secret().is_empty()) {
                secrets.push((
                    CredentialKey::new(namespace, Some(provider.into()), *account)?,
                    value,
                ));
            }
        }
    }
    if secrets.is_empty() {
        return Ok(None);
    }
    let channels = |namespace, id: &str| {
        if secrets.iter().any(|(key, _)| key.namespace == namespace) {
            vec![ChannelSummary {
                id: id.into(),
                name: String::new(),
                provider_type: id.into(),
                enabled: true,
                order: 0,
                last_test: None,
            }]
        } else {
            Vec::new()
        }
    };
    Ok(Some(LegacyCredentials {
        metadata: CredentialMetadata::from_parts(
            channels(CredentialNamespace::Asr, "volcengine"),
            channels(CredentialNamespace::Llm, "ark"),
            "volcengine",
            "ark",
            "custom",
            0,
        ),
        secrets,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY: &str = include_str!("../tests/fixtures/credentials-legacy-v2.json");

    #[test]
    fn desktop_v2_fixture_preserves_namespaces_channels_and_every_account() {
        let parsed = decode_legacy_credentials(LEGACY).unwrap();
        let asr = parsed.metadata.list_channels(ChannelKind::Asr);
        assert_eq!(asr[0].id, "shared");
        assert_eq!(asr[0].provider_type, "openai-compatible");
        assert_eq!(asr[0].name, "旧语音渠道");
        assert_eq!(asr[0].last_test.as_ref().unwrap().latency_ms, Some(23));
        assert!(!asr[1].enabled);
        assert_eq!(
            parsed.metadata.list_channels(ChannelKind::Llm)[0].provider_type,
            "deepseek"
        );
        assert_eq!(parsed.metadata.active_provider(ProviderSlot::Asr), "shared");
        assert_eq!(parsed.metadata.active_provider(ProviderSlot::Llm), "shared");
        assert_eq!(
            parsed.metadata.active_provider(ProviderSlot::Omni),
            "bailian"
        );
        assert_eq!(parsed.metadata.revision(), 7);
        let secret = |namespace, id, account| {
            let key = CredentialKey::new(namespace, id, account).unwrap();
            parsed
                .secrets
                .iter()
                .find(|(candidate, _)| candidate == &key)
                .unwrap()
                .1
                .expose_secret()
        };
        assert_eq!(
            secret(
                CredentialNamespace::Asr,
                Some("shared".into()),
                ASR_API_KEY_ACCOUNT
            ),
            "fixture-asr-key"
        );
        assert_eq!(
            secret(
                CredentialNamespace::Llm,
                Some("shared".into()),
                LLM_API_KEY_ACCOUNT
            ),
            "fixture-llm-key"
        );
        assert_eq!(
            secret(
                CredentialNamespace::Omni,
                Some("bailian".into()),
                OMNI_API_KEY_ACCOUNT
            ),
            "fixture-omni-key"
        );
        assert_eq!(
            secret(
                CredentialNamespace::Marketplace,
                None,
                MARKETPLACE_GITHUB_TOKEN_ACCOUNT
            ),
            "fixture-github-token"
        );
        assert_eq!(
            secret(
                CredentialNamespace::Llm,
                Some("shared".into()),
                LLM_EXTRA_HEADERS_ACCOUNT
            ),
            r#"{"X-Tenant":"fixture-header-secret"}"#
        );
        assert_eq!(
            secret(
                CredentialNamespace::Omni,
                Some("bailian".into()),
                OMNI_TEMPERATURE_ACCOUNT
            ),
            "0.2"
        );
        assert_eq!(parsed.secrets.len(), 24);
        assert!(!serde_json::to_string(&parsed.metadata)
            .unwrap()
            .contains("fixture-"));
    }

    #[test]
    fn stable_and_generation_chunks_decode_the_same_desktop_fixture() {
        for generation in [None, Some("old-generation")] {
            let split = LEGACY.find("\"omni\"").unwrap();
            let chunks = [&LEGACY[..split], &LEGACY[split..]];
            let header = serde_json::json!({ "openless_credentials_storage": "chunked", "version": 1, "generation": generation, "chunks": 2 }).to_string();
            let payload = read_legacy_vault_payload(|account| {
                if account == LEGACY_CREDENTIAL_ACCOUNT {
                    return Ok(Some(SecretValue::new(&header)));
                }
                for (index, chunk) in chunks.iter().enumerate() {
                    let expected = match generation {
                        Some(generation) => format!("credentials.v1.chunk.{generation}.{index}"),
                        None => format!("credentials.v1.chunk.{index}"),
                    };
                    if account == expected {
                        return Ok(Some(SecretValue::new(*chunk)));
                    }
                }
                Ok(None)
            })
            .unwrap()
            .unwrap();
            assert_eq!(payload.expose_secret(), LEGACY);
            assert_eq!(
                decode_legacy_credentials(payload.expose_secret())
                    .unwrap()
                    .secrets
                    .len(),
                24
            );
        }
        assert!(
            read_legacy_vault_payload(|account| Ok((account == LEGACY_CREDENTIAL_ACCOUNT).then(
                || SecretValue::new(
                    r#"{"openless_credentials_storage":"chunked","version":1,"chunks":1}"#
                )
            )))
            .is_err()
        );
    }

    #[test]
    fn v1_payload_and_pre_json_accounts_retain_historical_routing() {
        let parsed = decode_legacy_credentials(r#"{"version":1,"active":{"asr":"missing","llm":"ark"},"providers":{"asr":{"blank":{},"volcengine":{"accessKey":"fixture-token"}},"llm":{"ark":{"apiKey":"fixture-key"}}}}"#).unwrap();
        assert_eq!(
            parsed.metadata.active_provider(ProviderSlot::Asr),
            "volcengine"
        );
        assert_eq!(
            parsed.metadata.list_channels(ChannelKind::Asr)[0].provider_type,
            "volcengine"
        );
        let parsed = read_legacy_accounts(|account| {
            Ok([
                VOLCENGINE_ACCESS_KEY_ACCOUNT,
                LLM_API_KEY_ACCOUNT,
                OMNI_API_KEY_ACCOUNT,
            ]
            .contains(&account)
            .then(|| SecretValue::new("fixture-account")))
        })
        .unwrap()
        .unwrap();
        for (namespace, id, account) in [
            (
                CredentialNamespace::Asr,
                "volcengine",
                VOLCENGINE_ACCESS_KEY_ACCOUNT,
            ),
            (CredentialNamespace::Llm, "ark", LLM_API_KEY_ACCOUNT),
            (CredentialNamespace::Omni, "custom", OMNI_API_KEY_ACCOUNT),
        ] {
            assert!(parsed
                .secrets
                .iter()
                .any(|(key, _)| key.namespace == namespace
                    && key.provider_id.as_deref() == Some(id)
                    && key.account == account));
        }
        assert!(read_legacy_accounts(|_| Ok(None)).unwrap().is_none());
    }

    #[test]
    fn malformed_secret_values_never_appear_in_errors() {
        for payload in [
            r#"{"version":"fixture-secret"}"#,
            r#"{"providers":{"asr":{"id":{"enabled":"fixture-secret"}}}}"#,
            r#"{"openless_credentials_storage":"chunked","version":1}"#,
        ] {
            let error = decode_legacy_credentials(payload).err().unwrap();
            assert!(!format!("{error:?}").contains("fixture-secret"));
        }
    }
}
