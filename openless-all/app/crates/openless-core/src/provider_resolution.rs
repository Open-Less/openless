use std::sync::Arc;

use crate::credentials::{
    first_enabled_channel, ChannelKind, CredentialKey, CredentialNamespace, CredentialStore,
    ProviderChannelId, ProviderSlot, ProviderType, OMNI_MODEL_ACCOUNT,
};
use crate::dictation_context::ProviderInvocation;
use crate::errors::{BackendError, BackendErrorCode};
use crate::shared_types::{effective_pipeline_mode, PipelineMode, UserPreferences};

pub(crate) async fn resolve_session_provider(
    credential_store: &Arc<dyn CredentialStore>,
    slot: ProviderSlot,
    preference_fallback: &str,
) -> Result<ProviderInvocation, BackendError> {
    let channel_kind = match slot {
        ProviderSlot::Asr => Some(ChannelKind::Asr),
        ProviderSlot::Llm => Some(ChannelKind::Llm),
        ProviderSlot::Omni => None,
    };
    let selected_channel = if let Some(kind) = channel_kind {
        match credential_store.list_channels(kind).await {
            Ok(channels) if !channels.is_empty() => {
                Some(first_enabled_channel(&channels).cloned().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "no provider channel is enabled",
                    )
                })?)
            }
            Ok(_) => None,
            Err(error) if error.code == BackendErrorCode::Unsupported => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let (provider_id, provider_type) = if let Some(channel) = selected_channel {
        // Resolve both fields from one channel snapshot, rather than trusting a
        // separately persisted active value that may be stale after a restart.
        (channel.id, channel.provider_type)
    } else {
        let legacy_id = match credential_store.active_provider(slot).await {
            Ok(provider) if !provider.trim().is_empty() => provider,
            Ok(_) => preference_fallback.to_string(),
            Err(error) if error.code == BackendErrorCode::Unsupported => {
                preference_fallback.to_string()
            }
            Err(error) => return Err(error),
        };
        (legacy_id.clone(), legacy_id)
    };
    let provider_id = ProviderChannelId::new(provider_id)?;
    let provider_type = ProviderType::new(provider_type)?;
    let (namespace, channel_id, account) = match slot {
        ProviderSlot::Asr => (
            CredentialNamespace::Asr,
            Some(provider_id.as_str().to_string()),
            "asr.model",
        ),
        ProviderSlot::Llm => (
            CredentialNamespace::Llm,
            Some(provider_id.as_str().to_string()),
            "ark.model_id",
        ),
        ProviderSlot::Omni => (CredentialNamespace::Omni, None, "omni.model"),
    };
    let model_key = CredentialKey::new(namespace, channel_id, account)?;
    let model = match credential_store.read(model_key).await {
        Ok(value) => value
            .map(crate::credentials::SecretValue::into_exposed)
            .filter(|value| !value.trim().is_empty()),
        Err(error) if error.code == BackendErrorCode::Unsupported => None,
        Err(error) => return Err(error),
    };
    Ok(ProviderInvocation {
        provider_id: provider_id.into_inner(),
        provider_type: provider_type.into_inner(),
        model,
        language: None,
        prompt: None,
        runtime: None,
        keep_loaded_secs: None,
    })
}

/// Text dependency for selection polish and selection voice.
///
/// Multimodal mode uses the Omni channel. Traditional mode uses the LLM slot.
/// The two configurations stay independent: a missing LLM must not block Omni.
pub(crate) async fn resolve_selection_text_provider(
    credential_store: &Arc<dyn CredentialStore>,
    preferences: &UserPreferences,
) -> Result<ProviderInvocation, BackendError> {
    let mode = effective_pipeline_mode(
        preferences.multimodal_pipeline_enabled,
        preferences.pipeline_mode,
    );
    match mode {
        PipelineMode::Deepseek => Ok(crate::deepseek_suite::invocation(false)),
        PipelineMode::Multimodal => {
            let mut invocation = resolve_session_provider(
                credential_store,
                ProviderSlot::Omni,
                &preferences.active_omni_provider,
            )
            .await?;
            // Current vaults store `omni.model` on the active provider. The
            // shared resolver still reads the legacy unscoped key.
            if invocation
                .model
                .as_deref()
                .is_none_or(|model| model.trim().is_empty())
            {
                invocation.model =
                    read_scoped_omni_model(credential_store, &invocation.provider_id).await?;
            }
            Ok(invocation)
        }
        PipelineMode::Traditional => {
            resolve_session_provider(
                credential_store,
                ProviderSlot::Llm,
                &preferences.active_llm_provider,
            )
            .await
        }
    }
}

async fn read_scoped_omni_model(
    credential_store: &Arc<dyn CredentialStore>,
    provider_id: &str,
) -> Result<Option<String>, BackendError> {
    let key = CredentialKey::new(
        CredentialNamespace::Omni,
        Some(provider_id.to_string()),
        OMNI_MODEL_ACCOUNT,
    )?;
    match credential_store.read(key).await {
        Ok(value) => Ok(value
            .map(crate::credentials::SecretValue::into_exposed)
            .filter(|model| !model.trim().is_empty())),
        Err(error) if error.code == BackendErrorCode::Unsupported => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{
        ChannelMutation, ChannelMutationResult, ChannelSummary, CredentialDirectory,
        CredentialMetadata, CredentialMetadataStore, InMemoryCredentialStore, SecretValue,
    };

    async fn restored_store(enabled: bool, stale: &str) -> Arc<InMemoryCredentialStore> {
        let store = Arc::new(InMemoryCredentialStore::default());
        store
            .save_metadata(CredentialMetadata::from_parts(
                vec![
                    ChannelSummary {
                        id: "old-local".into(),
                        name: String::new(),
                        provider_type: "local-qwen3-mlx".into(),
                        enabled: false,
                        order: 2,
                        last_test: None,
                    },
                    ChannelSummary {
                        id: "cloud-account".into(),
                        name: String::new(),
                        provider_type: "tencent-cloud".into(),
                        enabled,
                        order: 0,
                        last_test: None,
                    },
                ],
                vec![],
                stale,
                "",
                "",
                0,
            ))
            .await
            .unwrap();
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Asr,
                    Some("cloud-account".into()),
                    "asr.model",
                )
                .unwrap(),
                SecretValue::new("Hy-ASR-3.0-preview"),
            )
            .await
            .unwrap();
        store
    }

    #[tokio::test]
    async fn restored_channel_selection_resolves_actual_cloud_id_protocol_and_model() {
        for stale in ["local-qwen3-mlx", "old-local", "missing", ""] {
            let credential_store: Arc<dyn CredentialStore> = restored_store(true, stale).await;
            let resolved =
                resolve_session_provider(&credential_store, ProviderSlot::Asr, "local-qwen3-mlx")
                    .await
                    .unwrap();
            assert_eq!(resolved.provider_id, "cloud-account");
            assert_eq!(resolved.provider_type, "tencent-cloud");
            assert_eq!(resolved.model.as_deref(), Some("Hy-ASR-3.0-preview"));
        }
    }

    #[tokio::test]
    async fn all_disabled_channels_block_legacy_preference_fallback() {
        let credential_store: Arc<dyn CredentialStore> =
            restored_store(false, "local-qwen3-mlx").await;
        let error =
            resolve_session_provider(&credential_store, ProviderSlot::Asr, "local-qwen3-mlx")
                .await
                .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::InvalidState);
    }

    #[tokio::test]
    async fn reordered_asr_channel_wins_over_stale_preference() {
        let store = Arc::new(InMemoryCredentialStore::default());
        let directory = CredentialDirectory::new(store.clone());
        for provider_type in ["foundry-local-whisper", "volcengine", "volcengine"] {
            directory
                .mutate_channel(ChannelMutation::Create {
                    kind: ChannelKind::Asr,
                    provider_type: provider_type.into(),
                    name: provider_type.into(),
                })
                .await
                .unwrap();
        }
        directory
            .mutate_channel(ChannelMutation::Reorder {
                kind: ChannelKind::Asr,
                ids: vec![
                    "volcengine-2".into(),
                    "volcengine".into(),
                    "foundry-local-whisper".into(),
                ],
            })
            .await
            .unwrap();
        assert_eq!(
            directory.active_provider(ProviderSlot::Asr).await.unwrap(),
            "volcengine-2"
        );

        let credential_store: Arc<dyn CredentialStore> = store;
        for stale_preference in ["foundry-local-whisper", "volcengine"] {
            let resolved =
                resolve_session_provider(&credential_store, ProviderSlot::Asr, stale_preference)
                    .await
                    .unwrap();
            assert_eq!(resolved.provider_id, "volcengine-2");
            assert_eq!(resolved.provider_type, "volcengine");
        }
    }

    #[tokio::test]
    async fn empty_asr_selection_uses_legacy_preference_fallback() {
        let credential_store: Arc<dyn CredentialStore> =
            Arc::new(InMemoryCredentialStore::default());
        let resolved = resolve_session_provider(&credential_store, ProviderSlot::Asr, "volcengine")
            .await
            .unwrap();
        assert_eq!(resolved.provider_id, "volcengine");
        assert_eq!(resolved.provider_type, "volcengine");
    }

    #[tokio::test]
    async fn selection_text_provider_follows_the_active_pipeline() {
        use crate::shared_types::UserPreferences;

        let store = Arc::new(InMemoryCredentialStore::default());
        let directory = CredentialDirectory::new(store.clone());
        let created = directory
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Llm,
                provider_type: "openai-compatible".into(),
                name: "Selection LLM".into(),
            })
            .await
            .unwrap();
        let ChannelMutationResult::Created(llm_id) = created else {
            panic!("LLM channel should be created");
        };
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some(llm_id.clone()),
                    crate::credentials::LLM_MODEL_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("llm-model"),
            )
            .await
            .unwrap();
        directory
            .set_active_provider(ProviderSlot::Omni, "dashscope-omni".into())
            .await
            .unwrap();
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Omni,
                    Some("dashscope-omni".into()),
                    crate::credentials::OMNI_API_KEY_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("omni-key"),
            )
            .await
            .unwrap();
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Omni,
                    Some("dashscope-omni".into()),
                    crate::credentials::OMNI_MODEL_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("omni-model"),
            )
            .await
            .unwrap();

        let credential_store: Arc<dyn CredentialStore> = store.clone();
        let mut preferences = UserPreferences {
            multimodal_pipeline_enabled: true,
            pipeline_mode: crate::shared_types::PipelineMode::Traditional,
            active_llm_provider: "stale-llm".into(),
            active_omni_provider: "stale-omni".into(),
            ..Default::default()
        };

        let traditional = resolve_selection_text_provider(&credential_store, &preferences)
            .await
            .unwrap();
        assert_eq!(traditional.provider_id, llm_id);
        assert_eq!(traditional.provider_type, "openai-compatible");
        assert_eq!(traditional.model.as_deref(), Some("llm-model"));

        preferences.multimodal_pipeline_enabled = false;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        let gated = resolve_selection_text_provider(&credential_store, &preferences)
            .await
            .unwrap();
        assert_eq!(gated.provider_id, llm_id);
        assert_eq!(gated.provider_type, "openai-compatible");

        preferences.multimodal_pipeline_enabled = true;
        preferences.pipeline_mode = crate::shared_types::PipelineMode::Multimodal;
        let multimodal = resolve_selection_text_provider(&credential_store, &preferences)
            .await
            .unwrap();
        assert_eq!(multimodal.provider_id, "dashscope-omni");
        assert_eq!(multimodal.provider_type, "dashscope-omni");
        assert_eq!(multimodal.model.as_deref(), Some("omni-model"));

        directory
            .mutate_channel(ChannelMutation::SetEnabled {
                kind: ChannelKind::Llm,
                id: llm_id,
                enabled: false,
            })
            .await
            .unwrap();
        let llm_error = resolve_session_provider(&credential_store, ProviderSlot::Llm, "ark")
            .await
            .unwrap_err();
        assert_eq!(llm_error.code, BackendErrorCode::InvalidState);
        let multimodal = resolve_selection_text_provider(&credential_store, &preferences)
            .await
            .unwrap();
        assert_eq!(multimodal.provider_id, "dashscope-omni");
        assert_eq!(multimodal.provider_type, "dashscope-omni");
        assert_eq!(multimodal.model.as_deref(), Some("omni-model"));
    }
}
