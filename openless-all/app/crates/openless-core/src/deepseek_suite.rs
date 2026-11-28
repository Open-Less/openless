//! Dedicated credentials and routing for the DeepSeek pipeline.
use crate::credentials::{CredentialKey, CredentialNamespace, CredentialStore};
use crate::dictation_context::ProviderInvocation;
use crate::errors::{BackendError, BackendErrorCode};
use crate::shared_types::{CredentialsStatus, PipelineMode, UserPreferences};
use serde::{Deserialize, Serialize};

pub const CONFIG_ACCOUNT: &str = "deepseek_suite.config";
pub const CHANNEL_ID: &str = "__deepseek_suite__";
pub const ENDPOINT: &str = "https://api.deepseek.com/v1";
pub const MODEL: &str = "deepseek-flash";
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeepSeekSuiteConfig {
    pub api_key: String,
    pub voice: String,
}
impl Default for DeepSeekSuiteConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            voice: "stella".into(),
        }
    }
}
impl DeepSeekSuiteConfig {
    pub fn parse(raw: &str) -> Result<Self, BackendError> {
        let value: Self = serde_json::from_str(raw).map_err(|_| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                "Invalid DeepSeek suite configuration",
            )
        })?;
        if !matches!(value.voice.as_str(), "stella" | "tide") {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "Unknown DeepSeek voice",
            ));
        }
        Ok(value)
    }
    pub async fn load(store: &dyn CredentialStore) -> Result<Self, BackendError> {
        let key = CredentialKey::new(CredentialNamespace::Application, None, CONFIG_ACCOUNT)?;
        match store.read(key).await? {
            Some(raw) => Self::parse(raw.expose_secret()),
            None => Ok(Self::default()),
        }
    }
    pub fn configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}
pub fn invocation(asr: bool) -> ProviderInvocation {
    let mut provider = ProviderInvocation::new(CHANNEL_ID, "deepseek");
    provider.model = Some(if asr { "deepseek-asr" } else { MODEL }.into());
    provider
}
pub async fn credentials_status(
    store: &dyn CredentialStore,
    preferences: UserPreferences,
) -> Result<CredentialsStatus, BackendError> {
    if preferences.pipeline_mode != PipelineMode::Deepseek {
        return store.status(preferences).await;
    }
    let configured = DeepSeekSuiteConfig::load(store).await?.configured();
    Ok(CredentialsStatus {
        pipeline_mode: PipelineMode::Deepseek,
        active_asr_provider: "deepseek".into(),
        active_llm_provider: "deepseek".into(),
        asr_configured: configured,
        llm_configured: configured,
        ark_configured: configured,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryCredentialStore, SecretValue};
    #[tokio::test]
    async fn suite_configuration_is_independent_of_traditional_channels() {
        let store = InMemoryCredentialStore::default();
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some("deepseek".into()),
                    "ark.api_key",
                )
                .unwrap(),
                SecretValue::new("traditional-key"),
            )
            .await
            .unwrap();
        assert!(!DeepSeekSuiteConfig::load(&store)
            .await
            .unwrap()
            .configured());
        let key =
            CredentialKey::new(CredentialNamespace::Application, None, CONFIG_ACCOUNT).unwrap();
        store
            .write(
                key,
                SecretValue::new(r#"{"apiKey":"suite-key","voice":"tide"}"#),
            )
            .await
            .unwrap();
        let suite = DeepSeekSuiteConfig::load(&store).await.unwrap();
        assert_eq!(suite.api_key, "suite-key");
        assert_eq!(suite.voice, "tide");
        let prefs = UserPreferences {
            pipeline_mode: PipelineMode::Deepseek,
            ..Default::default()
        };
        let status = credentials_status(&store, prefs).await.unwrap();
        assert!(status.asr_configured && status.llm_configured && !status.omni_configured);
        assert_eq!(status.pipeline_mode, PipelineMode::Deepseek);
        assert_eq!(invocation(true).provider_id, CHANNEL_ID);
    }
    #[test]
    fn selected_pipeline_survives_a_store_reopen() {
        let directory =
            std::env::temp_dir().join(format!("openless-pipeline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("preferences.json");
        for mode in [
            PipelineMode::Traditional,
            PipelineMode::Multimodal,
            PipelineMode::Deepseek,
        ] {
            let store = crate::PreferencesStore::open(&path).unwrap();
            store
                .set(UserPreferences {
                    pipeline_mode: mode,
                    multimodal_pipeline_enabled: true,
                    ..Default::default()
                })
                .unwrap();
            drop(store);
            assert_eq!(
                crate::PreferencesStore::open(&path)
                    .unwrap()
                    .get()
                    .pipeline_mode,
                mode
            );
        }
        let _ = std::fs::remove_dir_all(directory);
    }
    #[test]
    fn all_three_pipeline_choices_survive_preferences_roundtrip() {
        for mode in [
            PipelineMode::Traditional,
            PipelineMode::Multimodal,
            PipelineMode::Deepseek,
        ] {
            let prefs = UserPreferences {
                pipeline_mode: mode,
                multimodal_pipeline_enabled: true,
                ..Default::default()
            };
            let restored: UserPreferences =
                serde_json::from_value(serde_json::to_value(prefs).unwrap()).unwrap();
            assert_eq!(
                crate::shared_types::effective_pipeline_mode(
                    restored.multimodal_pipeline_enabled,
                    restored.pipeline_mode
                ),
                mode
            );
        }
        assert_eq!(
            crate::shared_types::effective_pipeline_mode(false, PipelineMode::Deepseek),
            PipelineMode::Deepseek
        );
    }
}
