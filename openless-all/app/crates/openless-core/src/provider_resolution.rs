use std::sync::Arc;

use crate::credentials::{
    first_enabled_channel, ChannelKind, CredentialKey, CredentialNamespace, CredentialStore,
    ProviderChannelId, ProviderSlot, ProviderType,
};
use crate::dictation_context::ProviderInvocation;
use crate::errors::{BackendError, BackendErrorCode};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{
        ChannelMutation, ChannelSummary, CredentialDirectory, CredentialMetadata,
        CredentialMetadataStore, InMemoryCredentialStore, SecretValue,
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
}
