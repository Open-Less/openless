//! Shared use-cases for operating on previously captured text or audio.
//!
//! Hosts own file selection and optional foreground-application capture. The
//! core owns style selection, immutable provider snapshots, cancellation and
//! attribution so Tauri and egui cannot drift in their business behaviour.

use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::config::TaskSpawner;
use crate::credentials::{CredentialStore, ProviderSlot};
use crate::dictation_context::{
    DictationContext, DictationProviderInvocations, DictationStartOptions, ProviderInvocation,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{TextPolisher, TextStreamChunk, TextStreamSink, TranscriptionEngine};
use crate::style_pack_store::StylePackStore;
use crate::types::SessionId;
use crate::{DictionaryStore, PreferencesStore, UserPreferences};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepolishRequest {
    pub raw_text: String,
    pub style_pack_id: Option<String>,
    pub front_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsrCallLabel {
    pub provider: String,
    pub model: Option<String>,
}

impl AsrCallLabel {
    pub fn new(provider: impl Into<String>, model: Option<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.filter(|model| !model.trim().is_empty()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetranscriptionResult {
    pub text: String,
    pub duration_ms: u64,
    pub asr: AsrCallLabel,
}

#[derive(Debug, Clone)]
pub struct RetranscriptionFailure {
    pub error: BackendError,
    pub attempted_asr: Option<AsrCallLabel>,
}

impl RetranscriptionFailure {
    pub fn is_terminal(&self) -> bool {
        is_terminal_foundry_error(&self.error)
    }

    pub fn into_message(self) -> String {
        self.error.message
    }
}

pub trait AuxiliaryApi: Send + Sync {
    fn repolish(
        &self,
        request: RepolishRequest,
    ) -> BoxFuture<'static, Result<String, BackendError>>;

    fn retranscribe_pcm(
        &self,
        pcm: Vec<u8>,
    ) -> BoxFuture<'static, Result<RetranscriptionResult, RetranscriptionFailure>>;
}

pub(crate) struct AuxiliaryService {
    preferences: Arc<PreferencesStore>,
    style_packs: Arc<StylePackStore>,
    vocabulary: Arc<DictionaryStore>,
    credential_store: Arc<dyn CredentialStore>,
    polisher: Arc<dyn TextPolisher>,
    transcription: Arc<dyn TranscriptionEngine>,
    task_spawner: Arc<dyn TaskSpawner>,
}

impl AuxiliaryService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        preferences: Arc<PreferencesStore>,
        style_packs: Arc<StylePackStore>,
        vocabulary: Arc<DictionaryStore>,
        credential_store: Arc<dyn CredentialStore>,
        polisher: Arc<dyn TextPolisher>,
        transcription: Arc<dyn TranscriptionEngine>,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            preferences,
            style_packs,
            vocabulary,
            credential_store,
            polisher,
            transcription,
            task_spawner,
        }
    }

    async fn capture_repolish_context(
        &self,
        style_pack_id: Option<&str>,
        front_app: Option<String>,
    ) -> Result<Arc<DictationContext>, BackendError> {
        let preferences = self.preferences.get();
        let llm = crate::provider_resolution::resolve_session_provider(
            &self.credential_store,
            ProviderSlot::Llm,
            &preferences.active_llm_provider,
        )
        .await?;
        let omni = crate::provider_resolution::resolve_session_provider(
            &self.credential_store,
            ProviderSlot::Omni,
            &preferences.active_omni_provider,
        )
        .await?;
        self.capture_context(
            preferences,
            style_pack_id,
            front_app,
            DictationProviderInvocations::new(
                ProviderInvocation::for_provider("auxiliary-unused-asr"),
                llm,
                omni,
            ),
        )
    }

    async fn capture_retranscription_context(&self) -> Result<Arc<DictationContext>, BackendError> {
        let preferences = self.preferences.get();
        let asr = crate::provider_resolution::resolve_session_provider(
            &self.credential_store,
            ProviderSlot::Asr,
            &preferences.active_asr_provider,
        )
        .await?;
        self.capture_context(
            preferences,
            None,
            None,
            DictationProviderInvocations::new(
                asr,
                ProviderInvocation::for_provider("auxiliary-unused-llm"),
                ProviderInvocation::for_provider("auxiliary-unused-omni"),
            ),
        )
    }

    fn capture_context(
        &self,
        preferences: UserPreferences,
        style_pack_id: Option<&str>,
        front_app: Option<String>,
        providers: DictationProviderInvocations,
    ) -> Result<Arc<DictationContext>, BackendError> {
        let style_pack = match style_pack_id.filter(|id| !id.trim().is_empty()) {
            Some(id) => self.style_packs.get(id)?,
            None => self
                .style_packs
                .get_or_default_active(&preferences.active_style_pack_id)?,
        };
        let hotwords = self
            .vocabulary
            .list()?
            .into_iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.phrase)
            .collect();
        let options = DictationStartOptions {
            style_pack_id: Some(style_pack.id.clone()),
            front_app,
            ..DictationStartOptions::default()
        };
        let mut context = DictationContext::capture(
            &preferences,
            &style_pack,
            providers,
            hotwords,
            Vec::new(),
            &options,
        );
        context.pipeline_mode = crate::shared_types::effective_pipeline_mode(
            preferences.multimodal_pipeline_enabled,
            preferences.pipeline_mode,
        );
        Ok(Arc::new(context))
    }

    fn clone_for_future(&self) -> Self {
        Self {
            preferences: Arc::clone(&self.preferences),
            style_packs: Arc::clone(&self.style_packs),
            vocabulary: Arc::clone(&self.vocabulary),
            credential_store: Arc::clone(&self.credential_store),
            polisher: Arc::clone(&self.polisher),
            transcription: Arc::clone(&self.transcription),
            task_spawner: Arc::clone(&self.task_spawner),
        }
    }
}

impl AuxiliaryApi for AuxiliaryService {
    fn repolish(
        &self,
        request: RepolishRequest,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        let service = self.clone_for_future();
        Box::pin(async move {
            let context = service
                .capture_repolish_context(request.style_pack_id.as_deref(), request.front_app)
                .await?;
            if !context.uses_llm_polisher() {
                return Ok(request.raw_text);
            }
            let output = service
                .polisher
                .polish(
                    SessionId::new(),
                    context,
                    request.raw_text,
                    Arc::new(DiscardTextStream),
                )
                .await?;
            Ok(output.text)
        })
    }

    fn retranscribe_pcm(
        &self,
        pcm: Vec<u8>,
    ) -> BoxFuture<'static, Result<RetranscriptionResult, RetranscriptionFailure>> {
        let service = self.clone_for_future();
        Box::pin(async move {
            if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
                return Err(RetranscriptionFailure {
                    error: BackendError::new(
                        BackendErrorCode::InvalidArgument,
                        "PCM must contain 16-bit little-endian samples",
                    ),
                    attempted_asr: None,
                });
            }
            let context = service
                .capture_retranscription_context()
                .await
                .map_err(|error| RetranscriptionFailure {
                    error,
                    attempted_asr: None,
                })?;
            let fallback_label =
                AsrCallLabel::new(context.asr.provider_type.clone(), context.asr.model.clone());
            let session_id = SessionId::new();
            let session = service
                .transcription
                .start(session_id, context, Arc::new(DiscardTextStream))
                .await
                .map_err(|mut error| {
                    error.retryable = !is_terminal_foundry_error(&error);
                    RetranscriptionFailure {
                        error,
                        attempted_asr: Some(fallback_label.clone()),
                    }
                })?;
            let attempted_asr = session
                .asr_call_label()
                .unwrap_or_else(|| fallback_label.clone());
            session.consume_pcm_chunk(&pcm);
            let mut guard = RetranscriptionCancelGuard::new(
                Arc::clone(&session),
                Arc::clone(&service.task_spawner),
            );
            let result = session.finish().await;
            guard.disarm();
            match result {
                Ok(output) => Ok(RetranscriptionResult {
                    text: output.text,
                    duration_ms: output.duration_ms,
                    asr: attempted_asr,
                }),
                Err(mut error) => {
                    error.retryable = !is_terminal_foundry_error(&error);
                    Err(RetranscriptionFailure {
                        error,
                        attempted_asr: Some(attempted_asr),
                    })
                }
            }
        })
    }
}

fn is_terminal_foundry_error(error: &BackendError) -> bool {
    error.details.as_ref().is_some_and(|details| {
        details.get("terminal").and_then(serde_json::Value::as_str) == Some("foundry_fallback")
    })
}

struct DiscardTextStream;

impl TextStreamSink for DiscardTextStream {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

struct RetranscriptionCancelGuard {
    session: Option<Arc<dyn crate::ports::TranscriptionSession>>,
    task_spawner: Arc<dyn TaskSpawner>,
}

impl RetranscriptionCancelGuard {
    fn new(
        session: Arc<dyn crate::ports::TranscriptionSession>,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            session: Some(session),
            task_spawner,
        }
    }

    fn disarm(&mut self) {
        self.session.take();
    }
}

impl Drop for RetranscriptionCancelGuard {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        self.task_spawner.spawn(Box::pin(async move {
            let _ = session.cancel().await;
        }));
    }
}

pub(crate) struct UnsupportedAuxiliaryApi;

impl AuxiliaryApi for UnsupportedAuxiliaryApi {
    fn repolish(
        &self,
        _request: RepolishRequest,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        Box::pin(async {
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "auxiliary text processing is not configured",
            ))
        })
    }

    fn retranscribe_pcm(
        &self,
        _pcm: Vec<u8>,
    ) -> BoxFuture<'static, Result<RetranscriptionResult, RetranscriptionFailure>> {
        Box::pin(async {
            Err(RetranscriptionFailure {
                error: BackendError::new(
                    BackendErrorCode::Unsupported,
                    "auxiliary transcription is not configured",
                ),
                attempted_asr: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialKey, InMemoryCredentialStore, SecretValue};
    use crate::style_packs::{BUILTIN_STYLE_PACK_FORMAL_ID, BUILTIN_STYLE_PACK_RAW_ID};
    use crate::testing::{FixtureTextPolisher, FixtureTranscriptionEngine};
    use crate::CredentialsStatus;
    use crate::TokioTaskSpawner;

    fn service_with_credentials(
        polisher: Arc<dyn TextPolisher>,
        transcription: Arc<dyn TranscriptionEngine>,
        credential_store: Arc<dyn CredentialStore>,
    ) -> (AuxiliaryService, std::path::PathBuf) {
        let dictionary_path = std::env::temp_dir().join(format!(
            "openless-core-auxiliary-{}.json",
            uuid::Uuid::new_v4()
        ));
        (
            AuxiliaryService::new(
                Arc::new(PreferencesStore::in_memory()),
                Arc::new(StylePackStore::in_memory()),
                Arc::new(DictionaryStore::at_path(dictionary_path.clone())),
                credential_store,
                polisher,
                transcription,
                Arc::new(TokioTaskSpawner),
            ),
            dictionary_path,
        )
    }

    fn service(
        polisher: Arc<dyn TextPolisher>,
        transcription: Arc<dyn TranscriptionEngine>,
    ) -> (AuxiliaryService, std::path::PathBuf) {
        service_with_credentials(
            polisher,
            transcription,
            Arc::new(InMemoryCredentialStore::default()),
        )
    }

    struct AsrOnlyCredentialStore;

    impl AsrOnlyCredentialStore {
        fn unsupported<T>() -> BoxFuture<'static, Result<T, BackendError>> {
            Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "test credential operation is unsupported",
                ))
            })
        }
    }

    impl CredentialStore for AsrOnlyCredentialStore {
        fn status(
            &self,
            _preferences: UserPreferences,
        ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
            Self::unsupported()
        }

        fn read(
            &self,
            _key: CredentialKey,
        ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
            Self::unsupported()
        }

        fn write(
            &self,
            _key: CredentialKey,
            _value: SecretValue,
        ) -> BoxFuture<'static, Result<(), BackendError>> {
            Self::unsupported()
        }

        fn remove(&self, _key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
            Self::unsupported()
        }

        fn active_provider(
            &self,
            slot: ProviderSlot,
        ) -> BoxFuture<'static, Result<String, BackendError>> {
            assert_eq!(
                slot,
                ProviderSlot::Asr,
                "retranscription queried a non-ASR slot"
            );
            Self::unsupported()
        }
    }

    #[tokio::test]
    async fn untouched_builtin_raw_repolish_is_a_true_passthrough() {
        let (service, path) = service(
            Arc::new(FixtureTextPolisher::successful("must not be used")),
            Arc::new(FixtureTranscriptionEngine::successful("unused", 0)),
        );

        let result = service
            .repolish(RepolishRequest {
                raw_text: "原样输出".into(),
                style_pack_id: Some(BUILTIN_STYLE_PACK_RAW_ID.into()),
                front_app: Some("Editor".into()),
            })
            .await
            .unwrap();

        assert_eq!(result, "原样输出");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn repolish_uses_the_explicit_style_without_changing_active_preferences() {
        let (service, path) = service(
            Arc::new(FixtureTextPolisher::successful("指定风格结果")),
            Arc::new(FixtureTranscriptionEngine::successful("unused", 0)),
        );
        let before = service.preferences.get().active_style_pack_id;

        let result = service
            .repolish(RepolishRequest {
                raw_text: "原文".into(),
                style_pack_id: Some(BUILTIN_STYLE_PACK_FORMAL_ID.into()),
                front_app: None,
            })
            .await
            .unwrap();

        assert_eq!(result, "指定风格结果");
        assert_eq!(service.preferences.get().active_style_pack_id, before);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn retranscription_feeds_pcm_and_returns_the_frozen_provider_label() {
        let transcription = Arc::new(FixtureTranscriptionEngine::successful("重转文本", 125));
        let (service, path) = service(
            Arc::new(FixtureTextPolisher::successful("unused")),
            transcription.clone(),
        );
        let expected_provider = service.preferences.get().active_asr_provider;

        let result = service.retranscribe_pcm(vec![1, 0, 2, 0]).await.unwrap();

        assert_eq!(result.text, "重转文本");
        assert_eq!(result.duration_ms, 125);
        assert_eq!(result.asr.provider, expected_provider);
        assert_eq!(transcription.pcm(), vec![1, 0, 2, 0]);
        assert_eq!(transcription.cancel_count(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn retranscription_does_not_resolve_llm_or_omni_credentials() {
        let (service, path) = service_with_credentials(
            Arc::new(FixtureTextPolisher::successful("unused")),
            Arc::new(FixtureTranscriptionEngine::successful("仅 ASR", 80)),
            Arc::new(AsrOnlyCredentialStore),
        );

        let result = service.retranscribe_pcm(vec![1, 0]).await.unwrap();

        assert_eq!(result.text, "仅 ASR");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn malformed_pcm_is_rejected_before_starting_an_adapter() {
        let transcription = Arc::new(FixtureTranscriptionEngine::successful("unused", 0));
        let (service, path) = service(
            Arc::new(FixtureTextPolisher::successful("unused")),
            transcription.clone(),
        );

        let failure = service.retranscribe_pcm(vec![1]).await.unwrap_err();

        assert_eq!(failure.error.code, BackendErrorCode::InvalidArgument);
        assert!(transcription.pcm().is_empty());
        let _ = std::fs::remove_file(path);
    }
}
