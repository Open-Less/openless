use std::sync::Arc;

use crate::credentials::{
    ChannelKind, CredentialKey, CredentialNamespace, CredentialStore, ProviderChannelId,
    ProviderSlot, ProviderType,
};
use crate::dictation_context::ProviderInvocation;
use crate::errors::{BackendError, BackendErrorCode};

pub(crate) async fn resolve_session_provider(
    credential_store: &Arc<dyn CredentialStore>,
    slot: ProviderSlot,
    preference_fallback: &str,
) -> Result<ProviderInvocation, BackendError> {
    let provider_id = match credential_store.active_provider(slot).await {
        Ok(provider) if !provider.trim().is_empty() => provider,
        Ok(_) => preference_fallback.to_string(),
        Err(error) if error.code == BackendErrorCode::Unsupported => {
            preference_fallback.to_string()
        }
        Err(error) => return Err(error),
    };
    let channel_kind = match slot {
        ProviderSlot::Asr => Some(ChannelKind::Asr),
        ProviderSlot::Llm => Some(ChannelKind::Llm),
        ProviderSlot::Omni => None,
    };
    let provider_type = if let Some(kind) = channel_kind {
        match credential_store.list_channels(kind).await {
            Ok(channels) => match channels
                .into_iter()
                .find(|channel| channel.id == provider_id)
            {
                Some(channel) if channel.enabled => channel.provider_type,
                Some(_) => {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "the selected provider channel is disabled",
                    ));
                }
                None => provider_id.clone(),
            },
            Err(error) if error.code == BackendErrorCode::Unsupported => provider_id.clone(),
            Err(error) => return Err(error),
        }
    } else {
        provider_id.clone()
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
    use crate::credentials::{ChannelMutation, CredentialDirectory, InMemoryCredentialStore};

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
