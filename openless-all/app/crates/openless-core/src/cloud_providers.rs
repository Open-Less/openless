//! Shared cloud provider construction and session lifecycle.
//!
//! Hosts supply only a [`CredentialStore`]. Provider selection, credential
//! account routing, protocol defaults, cancellation and output semantics stay
//! in core so Tauri and Linux cannot drift.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use parking_lot::{Mutex, RwLock};

use crate::asr::{
    BailianCredentials, BailianRealtimeASR, DashScopeMultimodalASR, DictionaryHotword,
    ElevenLabsBatchASR, MimoBatchASR, Qwen3RealtimeASR, Qwen3RealtimeCredentials,
    StepfunRealtimeASR, StepfunRealtimeCredentials, VolcengineCredentials, VolcengineStreamingASR,
    WhisperBatchASR, XfyunCredentials, XfyunStreamingASR,
};
use crate::config::{TaskSpawner, TokioTaskSpawner};
use crate::credentials::{
    CredentialKey, CredentialNamespace, CredentialStore, ASR_ADVANCED_CONFIG_ACCOUNT,
    ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT, ASR_VOCABULARY_ID_ACCOUNT,
    LLM_API_KEY_ACCOUNT, LLM_ENDPOINT_ACCOUNT, LLM_EXTRA_HEADERS_ACCOUNT, LLM_TEMPERATURE_ACCOUNT,
    OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_EXTRA_HEADERS_ACCOUNT, OMNI_MODEL_ACCOUNT,
    OMNI_TEMPERATURE_ACCOUNT, VOLCENGINE_ACCESS_KEY_ACCOUNT, VOLCENGINE_API_KEY_ACCOUNT,
    VOLCENGINE_APP_KEY_ACCOUNT, VOLCENGINE_AUTH_MODE_ACCOUNT, VOLCENGINE_RESOURCE_ID_ACCOUNT,
    XFYUN_API_KEY_ACCOUNT, XFYUN_APP_ID_ACCOUNT,
};
use crate::dictation_context::{DictationAudioSource, DictationContext};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{
    ActiveRecording, AudioConsumer, AudioRecorder, DictationEngine, EngineFailure,
    EngineFailureStage, EngineProgress, EngineProgressSink, EngineResult, EngineStage,
    PolishOutput, RecordingProgressSink, TextPolisher, TextStreamChunk, TextStreamSink,
    TranscriptOutput, TranscriptionEngine, TranscriptionSession,
};
use crate::provider_rules::{
    default_asr_endpoint, default_asr_model, default_llm_endpoint, default_llm_model,
    default_omni_endpoint, default_omni_model, parse_extra_headers, provider_descriptor,
    ValidationProbe,
};
use crate::types::SessionId;

pub const SHARED_CLOUD_ASR_PROVIDER_TYPES: &[&str] = &[
    "volcengine",
    "elevenlabs",
    "bailian",
    "bailian-qwen3-realtime",
    "bailian-fun-asr-flash",
    "siliconflow",
    "stepfun",
    "zhipu",
    "groq",
    "whisper",
    "openrouter",
    "zenmux",
    "openai-compatible",
    "xiaomi-mimo-asr",
    "iflytek",
];

pub const SHARED_CLOUD_LLM_PROVIDER_TYPES: &[&str] = &[
    "ark",
    "deepseek",
    "siliconflow",
    "atlascloud",
    "openai",
    "gemini",
    crate::polish::CODEX_OAUTH_PROVIDER_ID,
    "mimo",
    "cometapi",
    "openrouterFree",
    "alibabaCoding",
    "codingPlanX",
    "minimax",
    "stepfun",
    "custom",
];

pub const SHARED_OMNI_PROVIDER_TYPES: &[&str] = &["openai", "gemini", "dashscope-omni", "custom"];

#[derive(Clone)]
pub struct SharedCloudTranscriptionEngine {
    credentials: Arc<dyn CredentialStore>,
    task_spawner: Arc<dyn TaskSpawner>,
}

impl SharedCloudTranscriptionEngine {
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self::with_task_spawner(credentials, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: Arc<dyn CredentialStore>,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            credentials,
            task_spawner,
        }
    }
}

#[derive(Clone)]
enum CloudTranscriptionSessionKind {
    Volcengine(Arc<VolcengineStreamingASR>),
    Whisper(Arc<WhisperBatchASR>),
    Mimo(Arc<MimoBatchASR>),
    DashScope(Arc<DashScopeMultimodalASR>),
    ElevenLabs(Arc<ElevenLabsBatchASR>),
    Bailian(Arc<BailianRealtimeASR>),
    QwenRealtime(Arc<Qwen3RealtimeASR>),
    StepfunRealtime(Arc<StepfunRealtimeASR>),
    Xfyun(Arc<XfyunStreamingASR>),
}

struct CloudTranscriptionSession {
    kind: CloudTranscriptionSessionKind,
    asr_call_label: crate::AsrCallLabel,
    finished: AtomicBool,
}

impl TranscriptionEngine for SharedCloudTranscriptionEngine {
    fn start(
        &self,
        _session_id: SessionId,
        context: Arc<DictationContext>,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let credentials = Arc::clone(&self.credentials);
        let task_spawner = Arc::clone(&self.task_spawner);
        Box::pin(async move {
            let (kind, asr_call_label) = build_cloud_transcription_session(
                credentials.as_ref(),
                &context,
                task_spawner,
                partials,
            )
            .await?;
            Ok(Arc::new(CloudTranscriptionSession {
                kind,
                asr_call_label,
                finished: AtomicBool::new(false),
            }) as Arc<dyn TranscriptionSession>)
        })
    }
}

impl AudioConsumer for CloudTranscriptionSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        match &self.kind {
            CloudTranscriptionSessionKind::Volcengine(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::Whisper(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::Mimo(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::DashScope(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::ElevenLabs(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::Bailian(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::QwenRealtime(provider) => {
                provider.consume_pcm_chunk(pcm)
            }
            CloudTranscriptionSessionKind::StepfunRealtime(provider) => {
                provider.consume_pcm_chunk(pcm)
            }
            CloudTranscriptionSessionKind::Xfyun(provider) => provider.consume_pcm_chunk(pcm),
        }
    }
}

impl TranscriptionSession for CloudTranscriptionSession {
    fn asr_call_label(&self) -> Option<crate::AsrCallLabel> {
        Some(self.asr_call_label.clone())
    }

    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        if self.finished.swap(true, Ordering::AcqRel) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "cloud transcription session has already been finalized",
                ))
            });
        }
        let kind = self.kind.clone();
        Box::pin(async move {
            let transcript = match kind {
                CloudTranscriptionSessionKind::Volcengine(provider) => {
                    let _ = provider.send_last_frame().await;
                    timeout_transcription(Duration::from_secs(120), provider.await_final_result())
                        .await?
                }
                CloudTranscriptionSessionKind::Whisper(provider) => {
                    let timeout = crate::provider_rules::whisper_transcribe_timeout(
                        provider.buffer_duration_ms() as f64 / 1000.0,
                    );
                    timeout_transcription(timeout, provider.transcribe()).await?
                }
                CloudTranscriptionSessionKind::Mimo(provider) => {
                    timeout_transcription(Duration::from_secs(120), provider.transcribe()).await?
                }
                CloudTranscriptionSessionKind::DashScope(provider) => {
                    let timeout =
                        provider.transcribe_timeout(provider.buffer_duration_ms() as f64 / 1000.0);
                    timeout_transcription(timeout, provider.transcribe()).await?
                }
                CloudTranscriptionSessionKind::ElevenLabs(provider) => {
                    let timeout = crate::asr::elevenlabs::transcribe_timeout(
                        provider.buffer_duration_ms() as f64 / 1000.0,
                    );
                    timeout_transcription(timeout, provider.transcribe()).await?
                }
                CloudTranscriptionSessionKind::Bailian(provider) => {
                    let _ = provider.send_last_frame().await;
                    timeout_transcription(Duration::from_secs(120), provider.await_final_result())
                        .await?
                }
                CloudTranscriptionSessionKind::QwenRealtime(provider) => {
                    let _ = provider.send_last_frame().await;
                    timeout_transcription(Duration::from_secs(120), provider.await_final_result())
                        .await?
                }
                CloudTranscriptionSessionKind::StepfunRealtime(provider) => {
                    let _ = provider.send_last_frame().await;
                    timeout_transcription(Duration::from_secs(120), provider.await_final_result())
                        .await?
                }
                CloudTranscriptionSessionKind::Xfyun(provider) => {
                    let _ = provider.send_last_frame().await;
                    timeout_transcription(Duration::from_secs(120), provider.await_final_result())
                        .await?
                }
            };
            Ok(TranscriptOutput {
                text: transcript.text,
                duration_ms: transcript.duration_ms,
            })
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        match &self.kind {
            CloudTranscriptionSessionKind::Volcengine(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Whisper(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Mimo(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::DashScope(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::ElevenLabs(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Bailian(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::QwenRealtime(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::StepfunRealtime(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Xfyun(provider) => provider.cancel(),
        }
        Box::pin(async { Ok(()) })
    }
}

async fn timeout_transcription<T, E>(
    timeout: Duration,
    operation: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, BackendError>
where
    E: std::fmt::Display,
{
    match tokio::time::timeout(timeout, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(BackendError::new(
            BackendErrorCode::Provider,
            format!("ASR provider failed: {error}"),
        )),
        Err(_) => Err(
            BackendError::new(BackendErrorCode::Provider, "ASR provider timed out").retryable(true),
        ),
    }
}

async fn build_cloud_transcription_session(
    credentials: &dyn CredentialStore,
    context: &DictationContext,
    task_spawner: Arc<dyn TaskSpawner>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<(CloudTranscriptionSessionKind, crate::AsrCallLabel), BackendError> {
    use crate::asr::volcengine::VolcengineAuthMode;
    use crate::provider_rules::{ActiveAsrProviderKind, BailianEndpointProtocol};

    let channel_id = context.asr.provider_id.trim();
    let provider_type = context.asr.provider_type.trim();
    if channel_id.is_empty() || provider_type.is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "ASR channel id and provider type must not be empty",
        ));
    }
    if provider_descriptor(crate::ProviderKind::Asr, provider_type)
        .is_none_or(|descriptor| descriptor.validation_probe == ValidationProbe::Unsupported)
    {
        return Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "shared cloud ASR provider is not supported",
        ));
    }
    let stored_model = read_channel_credential(
        credentials,
        CredentialNamespace::Asr,
        channel_id,
        ASR_MODEL_ACCOUNT,
    )
    .await?;
    let model = context
        .asr
        .model
        .clone()
        .or(stored_model)
        .unwrap_or_default();
    let effective = crate::provider_rules::resolve_effective_asr_provider(provider_type, &model)
        .map_err(|message| BackendError::new(BackendErrorCode::InvalidArgument, message))?;
    let api_key = read_channel_credential(
        credentials,
        CredentialNamespace::Asr,
        channel_id,
        ASR_API_KEY_ACCOUNT,
    )
    .await?
    .unwrap_or_default();
    let endpoint = read_channel_credential(
        credentials,
        CredentialNamespace::Asr,
        channel_id,
        ASR_ENDPOINT_ACCOUNT,
    )
    .await?
    .unwrap_or_default();
    let advanced_config_raw = read_channel_credential(
        credentials,
        CredentialNamespace::Asr,
        channel_id,
        ASR_ADVANCED_CONFIG_ACCOUNT,
    )
    .await?;
    let advanced_config = crate::provider_rules::advanced_asr_config_for(
        provider_type,
        advanced_config_raw.as_deref(),
    );

    let (kind, label_model) = match crate::provider_rules::active_asr_provider_kind(&effective) {
        ActiveAsrProviderKind::Bailian => {
            require_configured(&api_key, "ASR API key")?;
            let stored_endpoint = non_blank_owned(endpoint)
                .unwrap_or_else(|| crate::asr::bailian::DEFAULT_ENDPOINT.to_string());
            let endpoint = if provider_type == crate::asr::bailian::PROVIDER_ID {
                crate::provider_rules::derive_bailian_endpoint(
                    &stored_endpoint,
                    BailianEndpointProtocol::ClassicRealtime,
                )
                .unwrap_or(stored_endpoint)
            } else {
                stored_endpoint
            };
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::bailian::DEFAULT_MODEL.to_string());
            let vocabulary_id = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                ASR_VOCABULARY_ID_ACCOUNT,
            )
            .await?
            .and_then(non_blank_owned);
            let provider = Arc::new(BailianRealtimeASR::with_task_spawner(
                BailianCredentials {
                    api_key,
                    endpoint,
                    model: effective_model.clone(),
                    vocabulary_id,
                },
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider.open_session().await.map_err(map_asr_error)?;
            (
                CloudTranscriptionSessionKind::Bailian(provider),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::Qwen3Realtime => {
            require_configured(&api_key, "ASR API key")?;
            let stored_endpoint = non_blank_owned(endpoint)
                .unwrap_or_else(|| crate::asr::qwen_realtime::DEFAULT_ENDPOINT.to_string());
            let endpoint = if provider_type == crate::asr::bailian::PROVIDER_ID {
                crate::provider_rules::derive_bailian_endpoint(
                    &stored_endpoint,
                    BailianEndpointProtocol::QwenRealtime,
                )
                .unwrap_or(stored_endpoint)
            } else {
                stored_endpoint
            };
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::qwen_realtime::DEFAULT_MODEL.to_string());
            let provider = Arc::new(Qwen3RealtimeASR::with_task_spawner(
                Qwen3RealtimeCredentials {
                    api_key,
                    endpoint,
                    model: effective_model.clone(),
                },
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider.open_session().await.map_err(map_asr_error)?;
            (
                CloudTranscriptionSessionKind::QwenRealtime(provider),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::StepfunRealtime => {
            require_configured(&api_key, "ASR API key")?;
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::stepfun_realtime::DEFAULT_MODEL.to_string());
            let provider = Arc::new(StepfunRealtimeASR::with_task_spawner(
                StepfunRealtimeCredentials {
                    api_key,
                    endpoint,
                    model: effective_model.clone(),
                    prompt: context.asr.prompt.clone(),
                },
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider.open_session().await.map_err(map_asr_error)?;
            (
                CloudTranscriptionSessionKind::StepfunRealtime(provider),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::Mimo => {
            require_configured(&api_key, "ASR API key")?;
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::mimo::DEFAULT_MODEL.to_string());
            (
                CloudTranscriptionSessionKind::Mimo(Arc::new(MimoBatchASR::new(
                    api_key,
                    non_blank_owned(endpoint)
                        .unwrap_or_else(|| crate::asr::mimo::DEFAULT_ENDPOINT.to_string()),
                    effective_model.clone(),
                ))),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::DashScopeMultimodal => {
            require_configured(&api_key, "ASR API key")?;
            let model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::dashscope_multimodal::DEFAULT_MODEL.to_string());
            let stored_endpoint = non_blank_owned(endpoint)
                .unwrap_or_else(|| crate::asr::dashscope_multimodal::DEFAULT_ENDPOINT.to_string());
            let endpoint = if provider_type == crate::asr::bailian::PROVIDER_ID {
                let protocol =
                    match crate::provider_rules::dashscope_batch_protocol_for_model(&model) {
                        Some(crate::provider_rules::DashScopeBatchProtocol::AsyncTranscription) => {
                            BailianEndpointProtocol::AsyncTranscription
                        }
                        _ => BailianEndpointProtocol::Multimodal,
                    };
                crate::provider_rules::derive_bailian_endpoint(&stored_endpoint, protocol)
                    .unwrap_or(stored_endpoint)
            } else {
                stored_endpoint
            };
            (
                CloudTranscriptionSessionKind::DashScope(Arc::new(DashScopeMultimodalASR::new(
                    api_key,
                    endpoint,
                    model.clone(),
                ))),
                Some(model),
            )
        }
        ActiveAsrProviderKind::ElevenLabs => {
            require_configured(&api_key, "ASR API key")?;
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::elevenlabs::DEFAULT_MODEL.to_string());
            (
                CloudTranscriptionSessionKind::ElevenLabs(Arc::new(ElevenLabsBatchASR::new(
                    api_key,
                    non_blank_owned(endpoint)
                        .unwrap_or_else(|| crate::asr::elevenlabs::DEFAULT_ENDPOINT.to_string()),
                    effective_model.clone(),
                ))),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::WhisperCompatible => {
            if crate::provider_rules::api_key_required(
                crate::ProviderKind::Asr,
                provider_type,
                Some(&endpoint),
            ) {
                require_configured(&api_key, "ASR API key")?;
            }
            let default_endpoint = default_asr_endpoint(provider_type).unwrap_or("");
            let default_model = default_asr_model(provider_type).unwrap_or("whisper-1");
            let effective_model =
                non_blank_owned(model).unwrap_or_else(|| default_model.to_string());
            let mut provider = WhisperBatchASR::new(
                api_key,
                non_blank_owned(endpoint).unwrap_or_else(|| default_endpoint.to_string()),
                effective_model.clone(),
                context.asr.prompt.clone(),
                crate::provider_rules::batch_asr_chunk_limit_ms(provider_type, advanced_config),
                crate::provider_rules::whisper_supports_verbose_json(
                    provider_type,
                    advanced_config,
                ),
            )
            .with_request_format(crate::provider_rules::whisper_request_format(provider_type));
            if crate::provider_rules::whisper_uses_hotwords(provider_type) {
                provider = provider.with_hotwords(context.polish.hotwords.clone());
            }
            if provider_type == "zenmux" {
                let language = context.asr.language.clone().or_else(|| {
                    context
                        .polish
                        .working_languages
                        .first()
                        .and_then(|language| crate::provider_rules::zenmux_language_code(language))
                });
                provider = provider
                    .with_language(language)
                    .with_enable_itn(advanced_config.enable_itn);
            }
            (
                CloudTranscriptionSessionKind::Whisper(Arc::new(provider)),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::Volcengine => {
            let auth_mode = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                VOLCENGINE_AUTH_MODE_ACCOUNT,
            )
            .await?
            .map(|value| VolcengineAuthMode::parse(&value))
            .unwrap_or(VolcengineAuthMode::AppIdToken);
            let app_id = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                VOLCENGINE_APP_KEY_ACCOUNT,
            )
            .await?
            .unwrap_or_default();
            let secret_account = match auth_mode {
                VolcengineAuthMode::AppIdToken => VOLCENGINE_ACCESS_KEY_ACCOUNT,
                VolcengineAuthMode::ApiKey => VOLCENGINE_API_KEY_ACCOUNT,
            };
            let access_token = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                secret_account,
            )
            .await?
            .unwrap_or_default();
            if !auth_mode.auth_ok(&app_id, &access_token) {
                return Err(credential_missing("Volcengine credentials"));
            }
            let resource_id = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                VOLCENGINE_RESOURCE_ID_ACCOUNT,
            )
            .await?;
            let credentials = VolcengineCredentials {
                auth_mode,
                app_id,
                access_token,
                resource_id: VolcengineCredentials::resolve_resource_id(resource_id),
            };
            let hotwords = context
                .polish
                .hotwords
                .iter()
                .cloned()
                .map(|phrase| DictionaryHotword {
                    phrase,
                    enabled: true,
                })
                .collect();
            let label =
                crate::provider_rules::volc_resource_history_label(&credentials.resource_id);
            let provider = Arc::new(VolcengineStreamingASR::with_task_spawner(
                credentials,
                hotwords,
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider.open_session().await.map_err(map_asr_error)?;
            (CloudTranscriptionSessionKind::Volcengine(provider), label)
        }
        ActiveAsrProviderKind::Xfyun => {
            let app_id = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                XFYUN_APP_ID_ACCOUNT,
            )
            .await?
            .unwrap_or_default();
            let api_key = read_channel_credential(
                credentials,
                CredentialNamespace::Asr,
                channel_id,
                XFYUN_API_KEY_ACCOUNT,
            )
            .await?
            .unwrap_or_default();
            require_configured(&app_id, "Xfyun application id")?;
            require_configured(&api_key, "Xfyun API key")?;
            let provider = Arc::new(XfyunStreamingASR::with_task_spawner(
                XfyunCredentials { app_id, api_key },
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider.open_session().await.map_err(map_asr_error)?;
            (CloudTranscriptionSessionKind::Xfyun(provider), None)
        }
    };
    Ok((kind, crate::AsrCallLabel::new(effective, label_model)))
}

async fn read_channel_credential(
    credentials: &dyn CredentialStore,
    namespace: CredentialNamespace,
    channel_id: &str,
    account: &str,
) -> Result<Option<String>, BackendError> {
    let key = CredentialKey::new(namespace, Some(channel_id.to_string()), account)?;
    credentials
        .read(key)
        .await
        .map(|value| value.map(crate::SecretValue::into_exposed))
}

fn require_configured(value: &str, label: &str) -> Result<(), BackendError> {
    if value.trim().is_empty() {
        Err(credential_missing(label))
    } else {
        Ok(())
    }
}

fn credential_missing(label: &str) -> BackendError {
    BackendError::new(
        BackendErrorCode::Provider,
        format!("{label} is not configured"),
    )
}

fn non_blank_owned(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn map_asr_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        BackendErrorCode::Provider,
        format!("ASR provider failed: {error}"),
    )
}

pub struct SharedCloudTextPolisher {
    credentials: Arc<dyn CredentialStore>,
    active: Arc<Mutex<HashMap<SessionId, Arc<ProviderCancellation>>>>,
}

impl SharedCloudTextPolisher {
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            credentials,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Default)]
struct ProviderCancellation {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl ProviderCancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

struct PolishRegistration {
    session_id: SessionId,
    cancellation: Arc<ProviderCancellation>,
    active: Arc<Mutex<HashMap<SessionId, Arc<ProviderCancellation>>>>,
}

impl Drop for PolishRegistration {
    fn drop(&mut self) {
        let mut active = self.active.lock();
        if active
            .get(&self.session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cancellation))
        {
            active.remove(&self.session_id);
        }
    }
}

enum CloudPolisherProvider {
    OpenAi(crate::polish::ActiveLLMProvider),
    Gemini(crate::llm_gemini::GeminiProvider),
}

impl TextPolisher for SharedCloudTextPolisher {
    fn polish(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        raw_text: String,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
        let cancellation = Arc::new(ProviderCancellation::default());
        {
            let mut active = self.active.lock();
            match active.entry(session_id) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Arc::clone(&cancellation));
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Box::pin(async {
                        Err(BackendError::new(
                            BackendErrorCode::Busy,
                            "polish request already exists for this session",
                        ))
                    });
                }
            }
        }
        let registration = PolishRegistration {
            session_id,
            cancellation: Arc::clone(&cancellation),
            active: Arc::clone(&self.active),
        };
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            let _registration = registration;
            if raw_text.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "polish input must not be empty",
                ));
            }
            let provider =
                build_cloud_polisher_provider(credentials.as_ref(), context.as_ref()).await?;
            tokio::select! {
                _ = cancellation.cancelled() => Err(cancelled_provider_error()),
                result = run_cloud_polish(
                    provider,
                    Arc::clone(&context),
                    raw_text,
                    partials,
                    Arc::clone(&cancellation),
                ) => result,
            }
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let cancellation = self.active.lock().get(&session_id).cloned();
        Box::pin(async move {
            if let Some(cancellation) = cancellation {
                cancellation.cancel();
            }
            Ok(())
        })
    }
}

async fn run_cloud_polish(
    provider: CloudPolisherProvider,
    context: Arc<DictationContext>,
    raw_text: String,
    partials: Arc<dyn TextStreamSink>,
    cancellation: Arc<ProviderCancellation>,
) -> Result<PolishOutput, BackendError> {
    if cancellation.is_cancelled() {
        return Err(cancelled_provider_error());
    }
    let call_label = match &provider {
        CloudPolisherProvider::OpenAi(provider) => provider.call_label(),
        CloudPolisherProvider::Gemini(_) => crate::polish::LlmCallLabel {
            provider: context.llm.provider_id.clone(),
            model: context
                .llm
                .model
                .clone()
                .or_else(|| default_llm_model(&context.llm.provider_type).map(str::to_string))
                .unwrap_or_default(),
        },
    };
    let prior_turns: Vec<(String, String)> = context
        .polish
        .prior_turns
        .iter()
        .rev()
        .map(|turn| (turn.raw_text.clone(), turn.polished_text.clone()))
        .collect();
    let style_system_prompt = if context.polish.translation_active {
        crate::build_polish_translate_system_prompt(
            &context.polish.style_system_prompt,
            &context.polish.translation_target_language,
        )
    } else {
        context.polish.style_system_prompt.clone()
    };
    let polished = match &provider {
        CloudPolisherProvider::OpenAi(provider)
            if !context.polish.translation_active && provider.supports_streaming_polish() =>
        {
            let offset = Arc::new(AtomicU64::new(0));
            let publish_error: Arc<Mutex<Option<BackendError>>> = Arc::new(Mutex::new(None));
            let sink = Arc::clone(&partials);
            let stream_offset = Arc::clone(&offset);
            let stream_error = Arc::clone(&publish_error);
            let on_delta = move |delta: &str| {
                if stream_error.lock().is_some() {
                    return;
                }
                let offset =
                    stream_offset.fetch_add(delta.chars().count() as u64, Ordering::AcqRel);
                if let Err(error) = sink.publish(TextStreamChunk {
                    text: delta.to_string(),
                    offset,
                }) {
                    *stream_error.lock() = Some(error);
                }
            };
            let stream_error_for_cancel = Arc::clone(&publish_error);
            let cancellation_for_stream = Arc::clone(&cancellation);
            let should_cancel = move || {
                cancellation_for_stream.is_cancelled() || stream_error_for_cancel.lock().is_some()
            };
            let result = provider
                .polish_streaming(
                    &raw_text,
                    context.polish.mode,
                    &context.polish.hotwords,
                    &style_system_prompt,
                    &context.polish.working_languages,
                    context.polish.chinese_script_preference,
                    context.polish.output_language_preference,
                    context.polish.front_app.as_deref(),
                    context.polish.cursor_context.as_deref(),
                    &prior_turns,
                    on_delta,
                    should_cancel,
                )
                .await;
            if let Some(error) = publish_error.lock().take() {
                return Err(error);
            }
            result.map_err(map_llm_error)?
        }
        CloudPolisherProvider::OpenAi(provider) => provider
            .polish(
                &raw_text,
                context.polish.mode,
                &context.polish.hotwords,
                &style_system_prompt,
                &context.polish.working_languages,
                context.polish.chinese_script_preference,
                context.polish.output_language_preference,
                context.polish.front_app.as_deref(),
                context.polish.cursor_context.as_deref(),
                &prior_turns,
            )
            .await
            .map_err(map_llm_error)?,
        CloudPolisherProvider::Gemini(provider) => provider
            .polish(
                &raw_text,
                context.polish.mode,
                &context.polish.hotwords,
                &style_system_prompt,
                &context.polish.working_languages,
                context.polish.chinese_script_preference,
                context.polish.output_language_preference,
                context.polish.front_app.as_deref(),
                context.polish.cursor_context.as_deref(),
                &prior_turns,
            )
            .await
            .map_err(map_llm_error)?,
    };
    if cancellation.is_cancelled() {
        return Err(cancelled_provider_error());
    }
    let mut output = if context.polish.translation_active {
        match crate::split_polish_translate_output(&polished) {
            Some((source_text, text)) => PolishOutput {
                text,
                source_text,
                llm_call_label: None,
            },
            None => {
                log::warn!(
                    "[cloud-provider] polish+translate response missing markers; using plain translation"
                );
                let text = match &provider {
                    CloudPolisherProvider::OpenAi(provider) => provider
                        .translate_to(
                            &raw_text,
                            &context.polish.translation_target_language,
                            &context.polish.working_languages,
                            context.polish.chinese_script_preference,
                            context.polish.output_language_preference,
                            context.polish.front_app.as_deref(),
                        )
                        .await
                        .map_err(map_llm_error)?,
                    CloudPolisherProvider::Gemini(provider) => provider
                        .translate_to(
                            &raw_text,
                            &context.polish.translation_target_language,
                            &context.polish.working_languages,
                            context.polish.chinese_script_preference,
                            context.polish.output_language_preference,
                            context.polish.front_app.as_deref(),
                        )
                        .await
                        .map_err(map_llm_error)?,
                };
                PolishOutput::text(text)
            }
        }
    } else {
        PolishOutput::text(polished)
    };
    if output.text.trim().is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::Provider,
            "LLM provider returned empty polish output",
        ));
    }
    output.llm_call_label = Some(call_label);
    Ok(output)
}

async fn build_cloud_polisher_provider(
    credentials: &dyn CredentialStore,
    context: &DictationContext,
) -> Result<CloudPolisherProvider, BackendError> {
    let channel_id = context.llm.provider_id.trim();
    let provider_type = context.llm.provider_type.trim();
    if channel_id.is_empty() || provider_type.is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "LLM channel id and provider type must not be empty",
        ));
    }
    let model = context
        .llm
        .model
        .clone()
        .and_then(non_blank_owned)
        .or_else(|| default_llm_model(provider_type).map(str::to_string))
        .ok_or_else(|| {
            BackendError::new(BackendErrorCode::Provider, "LLM model is not configured")
        })?;
    if provider_type == crate::polish::CODEX_OAUTH_PROVIDER_ID {
        let provider = crate::polish::CodexOAuthLLMProvider::new(
            crate::polish::CodexOAuthConfig::new(model)
                .with_thinking_enabled(context.polish.llm_thinking_enabled),
        );
        return Ok(CloudPolisherProvider::OpenAi(
            crate::polish::ActiveLLMProvider::Codex(provider),
        ));
    }

    let api_key = read_channel_credential(
        credentials,
        CredentialNamespace::Llm,
        channel_id,
        LLM_API_KEY_ACCOUNT,
    )
    .await?
    .unwrap_or_default();
    let configured_endpoint = read_channel_credential(
        credentials,
        CredentialNamespace::Llm,
        channel_id,
        LLM_ENDPOINT_ACCOUNT,
    )
    .await?;
    if crate::provider_rules::api_key_required(
        crate::ProviderKind::Llm,
        provider_type,
        configured_endpoint.as_deref(),
    ) {
        require_configured(&api_key, "LLM API key")?;
    }
    let endpoint = configured_endpoint
        .and_then(non_blank_owned)
        .or_else(|| default_llm_endpoint(provider_type).map(str::to_string))
        .ok_or_else(|| {
            BackendError::new(BackendErrorCode::Provider, "LLM endpoint is not configured")
        })?;
    crate::endpoint_security::validate_http_endpoint(&endpoint)
        .map_err(|error| map_llm_error(error.to_string()))?;
    if provider_type == "gemini" {
        let provider = crate::llm_gemini::GeminiProvider::new(
            crate::llm_gemini::GeminiConfig::new(api_key, model, endpoint.trim_end_matches('/'))
                .with_thinking_enabled(context.polish.llm_thinking_enabled),
        );
        return Ok(CloudPolisherProvider::Gemini(provider));
    }

    let base_url = endpoint
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/')
        .to_string();
    let temperature = read_channel_credential(
        credentials,
        CredentialNamespace::Llm,
        channel_id,
        LLM_TEMPERATURE_ACCOUNT,
    )
    .await?
    .as_deref()
    .map(parse_temperature)
    .transpose()?
    .flatten();
    let extra_headers = read_channel_credential(
        credentials,
        CredentialNamespace::Llm,
        channel_id,
        LLM_EXTRA_HEADERS_ACCOUNT,
    )
    .await?
    .as_deref()
    .map(parse_extra_headers)
    .transpose()?
    .unwrap_or_default();
    let config = crate::polish::OpenAICompatibleConfig::new(
        provider_type,
        "OpenLess LLM",
        base_url,
        api_key,
        model,
    )
    .with_thinking_enabled(context.polish.llm_thinking_enabled)
    .with_temperature(crate::polish::openai_compatible_temperature_for_provider(
        provider_type,
        temperature,
    ))
    .with_extra_headers(extra_headers);
    Ok(CloudPolisherProvider::OpenAi(
        crate::polish::ActiveLLMProvider::OpenAI(crate::polish::OpenAICompatibleLLMProvider::new(
            config,
        )),
    ))
}

fn parse_temperature(value: &str) -> Result<Option<f32>, BackendError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let temperature: f32 = value.parse().map_err(|_| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            "LLM temperature must be a number between 0 and 2",
        )
    })?;
    if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "LLM temperature must be a number between 0 and 2",
        ));
    }
    Ok(Some(temperature))
}

fn cancelled_provider_error() -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, "LLM polish request cancelled")
}

fn map_llm_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        BackendErrorCode::Provider,
        format!("LLM provider failed: {error}"),
    )
}

pub struct SharedAuxiliaryTextPolisher {
    credentials: Arc<dyn CredentialStore>,
    traditional: Arc<dyn TextPolisher>,
    omni_active: Arc<Mutex<HashMap<SessionId, Arc<ProviderCancellation>>>>,
}

impl SharedAuxiliaryTextPolisher {
    pub fn new(credentials: Arc<dyn CredentialStore>, traditional: Arc<dyn TextPolisher>) -> Self {
        Self {
            credentials,
            traditional,
            omni_active: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl TextPolisher for SharedAuxiliaryTextPolisher {
    fn polish(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        raw_text: String,
        _partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
        if context.pipeline_mode != crate::shared_types::PipelineMode::Multimodal {
            return self
                .traditional
                .polish(session_id, context, raw_text, _partials);
        }
        let cancellation = Arc::new(ProviderCancellation::default());
        {
            let mut active = self.omni_active.lock();
            match active.entry(session_id) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(Arc::clone(&cancellation));
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Box::pin(async {
                        Err(BackendError::new(
                            BackendErrorCode::Busy,
                            "auxiliary Omni polish request already exists for this session",
                        ))
                    });
                }
            }
        }
        let registration = PolishRegistration {
            session_id,
            cancellation: Arc::clone(&cancellation),
            active: Arc::clone(&self.omni_active),
        };
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            let _registration = registration;
            if raw_text.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "polish input must not be empty",
                ));
            }
            let provider = build_omni_provider(credentials.as_ref(), &context).await?;
            let mut system_prompt = context.polish.style_system_prompt.clone();
            if !context.polish.hotwords.is_empty() {
                system_prompt.push_str(&format!(
                    "\n\n# 词典/热词\n以下专有名词必须严格按给定写法准确识别：{}。",
                    context.polish.hotwords.join("、")
                ));
            }
            if !context.polish.working_languages.is_empty() {
                system_prompt.push_str(&format!(
                    "\n\n# 工作语言\n用户主要在以下语言间工作：{}。",
                    context.polish.working_languages.join("、")
                ));
            }
            let result = tokio::select! {
                _ = cancellation.cancelled() => Err(cancelled_omni_error()),
                result = provider.complete(&system_prompt, &raw_text, None) => {
                    result.map_err(map_omni_error)
                }
            }?;
            if result.trim().is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::Provider,
                    "Omni provider returned empty polish output",
                ));
            }
            Ok(PolishOutput::text(result))
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let cancellation = self.omni_active.lock().get(&session_id).cloned();
        let traditional = Arc::clone(&self.traditional);
        Box::pin(async move {
            if let Some(cancellation) = cancellation {
                cancellation.cancel();
            }
            traditional.cancel(session_id).await
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn answer_qa_with_context(
    credentials: Arc<dyn CredentialStore>,
    context: Arc<DictationContext>,
    messages: Vec<crate::QaMessage>,
    audio_wav: Option<Vec<u8>>,
    session_id: SessionId,
    progress: Arc<dyn crate::QaProgressSink>,
    cancelled: Arc<AtomicBool>,
) -> Result<String, BackendError> {
    let messages = messages
        .into_iter()
        .map(|message| crate::shared_types::QaChatMessage {
            role: message.role,
            content: message.content,
            selection_text: message.selection_text,
        })
        .collect::<Vec<_>>();
    let publish_error: Arc<Mutex<Option<BackendError>>> = Arc::new(Mutex::new(None));
    let progress_for_delta = Arc::clone(&progress);
    let publish_error_for_delta = Arc::clone(&publish_error);
    let on_delta = move |chunk: &str| {
        if publish_error_for_delta.lock().is_some() {
            return;
        }
        if let Err(error) = progress_for_delta.publish(
            session_id,
            crate::QaProgress::AnswerDelta(chunk.to_string()),
        ) {
            *publish_error_for_delta.lock() = Some(error);
        }
    };
    let publish_error_for_cancel = Arc::clone(&publish_error);
    let should_cancel =
        move || cancelled.load(Ordering::Acquire) || publish_error_for_cancel.lock().is_some();

    let result = if audio_wav.is_some()
        || context.pipeline_mode == crate::shared_types::PipelineMode::Multimodal
    {
        let provider = build_omni_provider(credentials.as_ref(), &context).await?;
        let system_prompt = crate::compose_qa_system_prompt(
            &context.polish.working_languages,
            context.polish.chinese_script_preference,
            context.polish.output_language_preference,
            context.polish.front_app.as_deref(),
        );
        let user_text = messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        provider
            .complete_streaming(
                &system_prompt,
                &user_text,
                audio_wav.as_deref(),
                on_delta,
                should_cancel,
            )
            .await
            .map_err(map_omni_error)
    } else {
        match build_cloud_polisher_provider(credentials.as_ref(), &context).await? {
            CloudPolisherProvider::OpenAi(provider) => provider
                .answer_chat_streaming(
                    &messages,
                    &context.polish.working_languages,
                    context.polish.chinese_script_preference,
                    context.polish.output_language_preference,
                    context.polish.front_app.as_deref(),
                    on_delta,
                    should_cancel,
                )
                .await
                .map_err(map_llm_error),
            CloudPolisherProvider::Gemini(provider) => provider
                .answer_chat_streaming(
                    &messages,
                    &context.polish.working_languages,
                    context.polish.chinese_script_preference,
                    context.polish.output_language_preference,
                    context.polish.front_app.as_deref(),
                    on_delta,
                    should_cancel,
                )
                .await
                .map_err(map_llm_error),
        }
    };
    if let Some(error) = publish_error.lock().take() {
        return Err(error);
    }
    result
}

async fn build_omni_provider(
    credentials: &dyn CredentialStore,
    context: &DictationContext,
) -> Result<crate::omni::OmniProvider, BackendError> {
    let provider_id = context.omni.provider_id.trim();
    let provider_type = context.omni.provider_type.trim();
    if provider_id.is_empty() || provider_type.is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "Omni provider id and type must not be empty",
        ));
    }
    let api_key = read_channel_credential(
        credentials,
        CredentialNamespace::Omni,
        provider_id,
        OMNI_API_KEY_ACCOUNT,
    )
    .await?
    .unwrap_or_default();
    require_configured(&api_key, "Omni API key")?;
    let model = context
        .omni
        .model
        .clone()
        .and_then(non_blank_owned)
        .or(read_channel_credential(
            credentials,
            CredentialNamespace::Omni,
            provider_id,
            OMNI_MODEL_ACCOUNT,
        )
        .await?
        .and_then(non_blank_owned))
        .or_else(|| default_omni_model(provider_type).map(str::to_string))
        .ok_or_else(|| {
            BackendError::new(BackendErrorCode::Provider, "Omni model is not configured")
        })?;
    let base_url = read_channel_credential(
        credentials,
        CredentialNamespace::Omni,
        provider_id,
        OMNI_ENDPOINT_ACCOUNT,
    )
    .await?
    .and_then(non_blank_owned)
    .or_else(|| default_omni_endpoint(provider_type).map(str::to_string))
    .ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::Provider,
            "Omni endpoint is not configured",
        )
    })?;
    crate::endpoint_security::validate_http_endpoint(&base_url)
        .map_err(|error| map_omni_error(error.to_string()))?;
    let extra_headers = read_channel_credential(
        credentials,
        CredentialNamespace::Omni,
        provider_id,
        OMNI_EXTRA_HEADERS_ACCOUNT,
    )
    .await?
    .as_deref()
    .map(parse_extra_headers)
    .transpose()?
    .unwrap_or_default();
    let temperature = read_channel_credential(
        credentials,
        CredentialNamespace::Omni,
        provider_id,
        OMNI_TEMPERATURE_ACCOUNT,
    )
    .await?
    .as_deref()
    .map(parse_temperature)
    .transpose()?
    .flatten();
    let config = crate::omni::OmniConfig {
        provider_id: provider_type.to_string(),
        base_url,
        api_key,
        model,
        extra_headers,
        temperature: crate::polish::openai_compatible_temperature_for_provider(
            provider_type,
            temperature,
        ),
        thinking_enabled: context.polish.llm_thinking_enabled,
    };
    Ok(crate::omni::OmniProvider::new(config))
}

/// Validate an Omni provider using the same construction and request path as
/// the production dictation pipeline.  The probe is intentionally text-only:
/// it exercises credential, endpoint, model and protocol resolution without
/// retaining user audio.
pub async fn validate_shared_omni_provider(
    credentials: Arc<dyn CredentialStore>,
    context: Arc<DictationContext>,
) -> Result<(), BackendError> {
    let provider = build_omni_provider(credentials.as_ref(), context.as_ref()).await?;
    let result = provider
        .complete("验证连接", "ping", None)
        .await
        .map_err(map_omni_error)?;

    if result.trim().is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::Provider,
            "Omni provider returned empty validation output",
        ));
    }

    Ok(())
}

fn cancelled_omni_error() -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, "Omni request cancelled")
}

fn map_omni_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        BackendErrorCode::Provider,
        format!("Omni provider failed: {error}"),
    )
}

pub struct SharedOmniDictationEngine {
    credentials: Arc<dyn CredentialStore>,
    recorder: Arc<dyn AudioRecorder>,
    sessions: Arc<Mutex<HashMap<SessionId, Arc<OmniSession>>>>,
}

impl SharedOmniDictationEngine {
    pub fn new(credentials: Arc<dyn CredentialStore>, recorder: Arc<dyn AudioRecorder>) -> Self {
        Self {
            credentials,
            recorder,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct OmniSession {
    context: RwLock<Arc<DictationContext>>,
    provider: Arc<crate::omni::OmniProvider>,
    pcm: Arc<OmniPcm>,
    recording: Mutex<Option<Box<dyn ActiveRecording>>>,
    cancellation: Arc<ProviderCancellation>,
    finishing: AtomicBool,
    progress: Arc<dyn EngineProgressSink>,
}

#[derive(Default)]
struct OmniPcm {
    bytes: Mutex<Vec<u8>>,
}

impl OmniPcm {
    fn snapshot(&self) -> Vec<u8> {
        self.bytes.lock().clone()
    }

    fn duration_ms(&self) -> u64 {
        (self.bytes.lock().len() as u64).saturating_mul(1_000) / 32_000
    }
}

impl AudioConsumer for OmniPcm {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.bytes.lock().extend_from_slice(pcm);
    }
}

struct OmniRecordingProgress {
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
}

impl RecordingProgressSink for OmniRecordingProgress {
    fn publish_level(&self, elapsed_ms: u64, level: f32) -> Result<(), BackendError> {
        self.progress.publish(
            self.session_id,
            EngineProgress::RecordingLevel {
                elapsed_ms,
                level: level.clamp(0.0, 1.0),
            },
        )
    }
}

impl DictationEngine for SharedOmniDictationEngine {
    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let credentials = Arc::clone(&self.credentials);
        let recorder = Arc::clone(&self.recorder);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let provider = Arc::new(build_omni_provider(credentials.as_ref(), &context).await?);
            let pcm = Arc::new(OmniPcm::default());
            let cancellation = Arc::new(ProviderCancellation::default());
            let session = Arc::new(OmniSession {
                context: RwLock::new(Arc::clone(&context)),
                provider,
                pcm: Arc::clone(&pcm),
                recording: Mutex::new(None),
                cancellation,
                finishing: AtomicBool::new(false),
                progress: Arc::clone(&progress),
            });
            {
                let mut active = sessions.lock();
                match active.entry(session_id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(Arc::clone(&session));
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {
                        return Err(BackendError::new(
                            BackendErrorCode::Busy,
                            "Omni dictation session already exists",
                        ));
                    }
                }
            }
            if context.audio_source == DictationAudioSource::External {
                if session.cancellation.is_cancelled() {
                    remove_omni_session(&sessions, session_id, &session);
                    return Err(cancelled_omni_error());
                }
                return Ok(());
            }
            let consumer: Arc<dyn AudioConsumer> = pcm;
            let level_progress: Arc<dyn RecordingProgressSink> = Arc::new(OmniRecordingProgress {
                session_id,
                progress,
            });
            let recording = match recorder
                .start(session_id, context, consumer, level_progress)
                .await
            {
                Ok(recording) => recording,
                Err(error) => {
                    remove_omni_session(&sessions, session_id, &session);
                    return Err(error);
                }
            };
            if session.cancellation.is_cancelled() {
                let _ = recording.stop().await;
                remove_omni_session(&sessions, session_id, &session);
                return Err(cancelled_omni_error());
            }
            *session.recording.lock() = Some(recording);
            if session.cancellation.is_cancelled() {
                let recording = session.recording.lock().take();
                if let Some(recording) = recording {
                    let _ = recording.stop().await;
                }
                remove_omni_session(&sessions, session_id, &session);
                return Err(cancelled_omni_error());
            }
            Ok(())
        })
    }

    fn finish(
        &self,
        session_id: SessionId,
        progress: Arc<dyn EngineProgressSink>,
    ) -> BoxFuture<'static, Result<EngineResult, EngineFailure>> {
        let session = self.sessions.lock().get(&session_id).cloned();
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let Some(session) = session else {
                return Err(EngineFailure::from(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Omni dictation session is not active",
                )));
            };
            if session.finishing.swap(true, Ordering::AcqRel) {
                return Err(EngineFailure::from(BackendError::new(
                    BackendErrorCode::Busy,
                    "Omni dictation session is already finishing",
                )));
            }
            let context = Arc::clone(&session.context.read());
            let recording = session.recording.lock().take();
            if context.audio_source == DictationAudioSource::Microphone && recording.is_none() {
                return Err(EngineFailure::from(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Omni recording is not ready",
                )));
            }
            let archive = recording.as_ref().and_then(|recording| recording.archive());
            let duration_ms = session.pcm.duration_ms();
            if let Some(recording) = recording {
                if let Err(error) = recording.stop().await {
                    remove_omni_session(&sessions, session_id, &session);
                    let mut failure = EngineFailure::new(error, EngineFailureStage::Transcribing);
                    failure.duration_ms = Some(duration_ms);
                    failure.has_audio_recording = archive.as_ref().map(|item| item.is_available());
                    return Err(failure);
                }
            }
            if session.cancellation.is_cancelled() {
                remove_omni_session(&sessions, session_id, &session);
                return Err(EngineFailure::from(cancelled_omni_error()));
            }
            progress
                .publish(session_id, EngineProgress::Stage(EngineStage::Polishing))
                .map_err(EngineFailure::from)?;
            let wav = crate::asr::wav::encode_wav_16k_mono(
                &session
                    .pcm
                    .snapshot()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                    .collect::<Vec<_>>(),
            );
            let system_prompt = build_omni_prompt(&context);
            let offset = Arc::new(AtomicU64::new(0));
            let publish_error: Arc<Mutex<Option<BackendError>>> = Arc::new(Mutex::new(None));
            let sink = Arc::clone(&progress);
            let stream_offset = Arc::clone(&offset);
            let stream_error = Arc::clone(&publish_error);
            let on_delta = move |delta: &str| {
                if stream_error.lock().is_some() {
                    return;
                }
                let offset =
                    stream_offset.fetch_add(delta.chars().count() as u64, Ordering::AcqRel);
                if let Err(error) = sink.publish(
                    session_id,
                    EngineProgress::PolishDelta(crate::PolishDelta {
                        text: delta.to_string(),
                        offset,
                        is_final: false,
                    }),
                ) {
                    *stream_error.lock() = Some(error);
                }
            };
            let cancellation_for_stream = Arc::clone(&session.cancellation);
            let stream_error_for_cancel = Arc::clone(&publish_error);
            let should_cancel = move || {
                cancellation_for_stream.is_cancelled() || stream_error_for_cancel.lock().is_some()
            };
            let provider = Arc::clone(&session.provider);
            let cancellation = Arc::clone(&session.cancellation);
            let started = Instant::now();
            let result = tokio::select! {
                _ = cancellation.cancelled() => Err(cancelled_omni_error()),
                result = provider.complete_streaming(
                    &system_prompt,
                    "",
                    Some(&wav),
                    on_delta,
                    should_cancel,
                ) => result.map_err(map_omni_error),
            };
            let polish_ms = Some(started.elapsed().as_millis() as u64);
            if let Some(error) = publish_error.lock().take() {
                remove_omni_session(&sessions, session_id, &session);
                let mut failure = EngineFailure::new(error, EngineFailureStage::Polishing);
                failure.duration_ms = Some(duration_ms);
                failure.polish_ms = polish_ms;
                failure.has_audio_recording = archive.as_ref().map(|item| item.is_available());
                return Err(failure);
            }
            let output = match result {
                Ok(output) if !output.trim().is_empty() => output.trim().to_string(),
                Ok(_) => {
                    let error = BackendError::new(
                        BackendErrorCode::Provider,
                        "Omni provider returned empty output",
                    );
                    remove_omni_session(&sessions, session_id, &session);
                    let mut failure = EngineFailure::new(error, EngineFailureStage::Polishing);
                    failure.duration_ms = Some(duration_ms);
                    failure.polish_ms = polish_ms;
                    failure.has_audio_recording = archive.as_ref().map(|item| item.is_available());
                    return Err(failure);
                }
                Err(error) => {
                    remove_omni_session(&sessions, session_id, &session);
                    let mut failure = EngineFailure::new(error, EngineFailureStage::Polishing);
                    failure.duration_ms = Some(duration_ms);
                    failure.polish_ms = polish_ms;
                    failure.has_audio_recording = archive.as_ref().map(|item| item.is_available());
                    return Err(failure);
                }
            };
            if !context.recording.archive_successful_recording {
                if let Some(archive) = archive.as_ref() {
                    if archive.is_available() {
                        let _ = archive.discard().await;
                    }
                }
            }
            progress
                .publish(
                    session_id,
                    EngineProgress::PolishDelta(crate::PolishDelta {
                        text: output.clone(),
                        offset: 0,
                        is_final: true,
                    }),
                )
                .map_err(EngineFailure::from)?;
            let has_audio_recording = archive.as_ref().map(|item| item.is_available());
            remove_omni_session(&sessions, session_id, &session);
            Ok(EngineResult {
                raw_text: output.clone(),
                asr_transcript: None,
                polished_text: output,
                polish_source: None,
                asr_call_label: None,
                llm_call_label: None,
                duration_ms,
                polish_failed: false,
                asr_ms: None,
                polish_ms,
                has_audio_recording,
            })
        })
    }

    fn update_context(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self.sessions.lock().get(&session_id).cloned();
        Box::pin(async move {
            let session = session.ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Omni dictation session is not active",
                )
            })?;
            if session.finishing.load(Ordering::Acquire) || session.cancellation.is_cancelled() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Omni dictation context can only change before finalization",
                ));
            }
            *session.context.write() = context;
            Ok(())
        })
    }

    fn feed_audio(&self, session_id: SessionId, pcm: &[u8]) -> Result<(), BackendError> {
        if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "external PCM must contain complete signed 16-bit samples",
            ));
        }
        let session = self
            .sessions
            .lock()
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Omni dictation session is not active",
                )
            })?;
        if session.context.read().audio_source != DictationAudioSource::External {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "Omni dictation session does not use external audio",
            ));
        }
        let (elapsed_ms, level) = {
            let mut bytes = session.pcm.bytes.lock();
            if session.finishing.load(Ordering::Acquire) || session.cancellation.is_cancelled() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Omni dictation session no longer accepts external audio",
                ));
            }
            bytes.extend_from_slice(pcm);
            (
                (bytes.len() as u64).saturating_mul(1_000) / 32_000,
                crate::external_audio::pcm_i16_le_rms(pcm),
            )
        };
        session.progress.publish(
            session_id,
            EngineProgress::RecordingLevel { elapsed_ms, level },
        )
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self.sessions.lock().get(&session_id).cloned();
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            let Some(session) = session else {
                return Ok(());
            };
            session.cancellation.cancel();
            let recording = session.recording.lock().take();
            if let Some(recording) = recording {
                recording.stop().await?;
            }
            remove_omni_session(&sessions, session_id, &session);
            Ok(())
        })
    }
}

fn remove_omni_session(
    sessions: &Arc<Mutex<HashMap<SessionId, Arc<OmniSession>>>>,
    session_id: SessionId,
    expected: &Arc<OmniSession>,
) {
    let mut sessions = sessions.lock();
    if sessions
        .get(&session_id)
        .is_some_and(|current| Arc::ptr_eq(current, expected))
    {
        sessions.remove(&session_id);
    }
}

fn build_omni_prompt(context: &DictationContext) -> String {
    let mut snapshot = context.clone();
    snapshot.polish.translation_active = false;
    let mut prompt = snapshot.effective_polish_system_prompt();
    if context.polish.translation_active {
        prompt.push_str(&format!(
            "\n\n# 翻译\n把识别和整理后的最终正文翻译成「{}」。只输出译文，不要输出源文、标记或解释。",
            context.polish.translation_target_language
        ));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryCredentialStore, ProviderInvocation, SecretValue};

    struct IgnoreTextStreamSink;

    impl TextStreamSink for IgnoreTextStreamSink {
        fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
            Ok(())
        }
    }

    struct IgnoreEngineProgressSink;

    impl EngineProgressSink for IgnoreEngineProgressSink {
        fn publish(
            &self,
            _session_id: SessionId,
            _progress: EngineProgress,
        ) -> Result<(), BackendError> {
            Ok(())
        }
    }

    async fn write_channel_secret(
        store: &InMemoryCredentialStore,
        namespace: CredentialNamespace,
        channel_id: &str,
        account: &str,
        value: &str,
    ) {
        store
            .write(
                CredentialKey::new(namespace, Some(channel_id.to_string()), account).unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cloud_asr_rejects_unknown_protocol_instead_of_falling_back_to_volcengine() {
        let credentials: Arc<dyn CredentialStore> = Arc::new(InMemoryCredentialStore::default());
        let engine = SharedCloudTranscriptionEngine::new(credentials);
        let context = DictationContext {
            asr: ProviderInvocation::new("channel", "unknown-provider"),
            ..DictationContext::default()
        };

        let error = match engine
            .start(
                SessionId::new(),
                Arc::new(context),
                Arc::new(IgnoreTextStreamSink),
            )
            .await
        {
            Ok(_) => panic!("unknown provider must not enter a production builder"),
            Err(error) => error,
        };
        assert_eq!(error.code, BackendErrorCode::Unsupported);
    }

    #[tokio::test]
    async fn cloud_asr_reads_only_the_channel_frozen_in_the_context() {
        let store = Arc::new(InMemoryCredentialStore::default());
        write_channel_secret(
            store.as_ref(),
            CredentialNamespace::Asr,
            "other-channel",
            ASR_API_KEY_ACCOUNT,
            "must-not-be-used",
        )
        .await;
        let credentials: Arc<dyn CredentialStore> = store.clone();
        let engine = SharedCloudTranscriptionEngine::new(credentials);
        let context = DictationContext {
            asr: ProviderInvocation::new("selected-channel", "xiaomi-mimo-asr"),
            ..DictationContext::default()
        };

        let error = match engine
            .start(
                SessionId::new(),
                Arc::new(context.clone()),
                Arc::new(IgnoreTextStreamSink),
            )
            .await
        {
            Ok(_) => panic!("credentials from another channel must not be used"),
            Err(error) => error,
        };
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert!(!error.to_string().contains("must-not-be-used"));

        write_channel_secret(
            store.as_ref(),
            CredentialNamespace::Asr,
            "selected-channel",
            ASR_API_KEY_ACCOUNT,
            "selected-secret",
        )
        .await;
        let session = engine
            .start(
                SessionId::new(),
                Arc::new(context),
                Arc::new(IgnoreTextStreamSink),
            )
            .await
            .unwrap();
        assert_eq!(
            session.asr_call_label().unwrap().provider,
            "xiaomi-mimo-asr"
        );
        session.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn cloud_polisher_reports_missing_credentials_without_opening_the_network() {
        let credentials: Arc<dyn CredentialStore> = Arc::new(InMemoryCredentialStore::default());
        let polisher = SharedCloudTextPolisher::new(credentials);
        let context = DictationContext {
            llm: ProviderInvocation::new("llm-channel", "deepseek"),
            ..DictationContext::default()
        };

        let error = polisher
            .polish(
                SessionId::new(),
                Arc::new(context),
                "raw text".to_string(),
                Arc::new(IgnoreTextStreamSink),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Provider);
        assert_eq!(error.message, "LLM API key is not configured");
    }

    #[tokio::test]
    async fn duplicate_polish_session_keeps_the_original_cancellation_route() {
        let credentials: Arc<dyn CredentialStore> = Arc::new(InMemoryCredentialStore::default());
        let polisher = SharedCloudTextPolisher::new(credentials);
        let session_id = SessionId::new();
        let original = Arc::new(ProviderCancellation::default());
        polisher
            .active
            .lock()
            .insert(session_id, Arc::clone(&original));

        let error = polisher
            .polish(
                session_id,
                Arc::new(DictationContext::default()),
                "raw".to_string(),
                Arc::new(IgnoreTextStreamSink),
            )
            .await
            .expect_err("duplicate polish session must be rejected");

        assert_eq!(error.code, BackendErrorCode::Busy);
        let active = polisher.active.lock();
        assert!(Arc::ptr_eq(active.get(&session_id).unwrap(), &original));
    }

    #[tokio::test]
    async fn duplicate_omni_start_is_busy_and_keeps_the_original_external_audio_route() {
        let store = Arc::new(InMemoryCredentialStore::default());
        write_channel_secret(
            store.as_ref(),
            CredentialNamespace::Omni,
            "openai",
            OMNI_API_KEY_ACCOUNT,
            "omni-secret",
        )
        .await;
        let credentials: Arc<dyn CredentialStore> = store;
        let recorder: Arc<dyn AudioRecorder> = Arc::new(crate::testing::FixtureAudioRecorder::new(
            Vec::new(),
            Vec::new(),
        ));
        let engine = SharedOmniDictationEngine::new(credentials, recorder);
        let session_id = SessionId::new();
        let mut omni = ProviderInvocation::new("openai", "openai");
        omni.model = Some("gpt-4o-audio-preview".to_string());
        let context = DictationContext {
            audio_source: DictationAudioSource::External,
            pipeline_mode: crate::shared_types::PipelineMode::Multimodal,
            omni,
            ..DictationContext::default()
        };
        let context = Arc::new(context);
        let progress: Arc<dyn EngineProgressSink> = Arc::new(IgnoreEngineProgressSink);

        engine
            .start(session_id, Arc::clone(&context), Arc::clone(&progress))
            .await
            .unwrap();
        let error = engine
            .start(session_id, context, progress)
            .await
            .expect_err("duplicate Omni start must be rejected");

        assert_eq!(error.code, BackendErrorCode::Busy);
        engine.feed_audio(session_id, &[0, 0]).unwrap();
        engine.cancel(session_id).await.unwrap();
    }

    #[tokio::test]
    async fn omni_provider_reads_only_the_provider_frozen_in_the_context() {
        let store = InMemoryCredentialStore::default();
        for (account, frozen, other) in [
            (OMNI_API_KEY_ACCOUNT, "frozen-secret", "other-secret"),
            (
                OMNI_ENDPOINT_ACCOUNT,
                "https://frozen.example.com/v1",
                "https://other.example.com/v1",
            ),
            (OMNI_MODEL_ACCOUNT, "frozen-model", "other-model"),
            (
                OMNI_EXTRA_HEADERS_ACCOUNT,
                r#"{"x-tenant":"frozen"}"#,
                r#"{"x-tenant":"other"}"#,
            ),
            (OMNI_TEMPERATURE_ACCOUNT, "0.4", "1.2"),
        ] {
            write_channel_secret(
                &store,
                CredentialNamespace::Omni,
                "frozen-provider",
                account,
                frozen,
            )
            .await;
            write_channel_secret(
                &store,
                CredentialNamespace::Omni,
                "other-provider",
                account,
                other,
            )
            .await;
        }
        let context = DictationContext {
            omni: ProviderInvocation::new("frozen-provider", "custom-omni"),
            ..DictationContext::default()
        };

        let provider = build_omni_provider(&store, &context).await.unwrap();
        assert_eq!(provider.call_label().provider, "custom-omni");
        assert_eq!(provider.call_label().model, "frozen-model");

        write_channel_secret(
            &store,
            CredentialNamespace::Omni,
            "frozen-provider",
            OMNI_EXTRA_HEADERS_ACCOUNT,
            r#"{"Authorization":"frozen-secret"}"#,
        )
        .await;
        let error = match build_omni_provider(&store, &context).await {
            Ok(_) => panic!("reserved Omni headers must be rejected"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(!message.contains("frozen-secret"));
        assert!(!message.contains("other-secret"));
    }

    #[test]
    fn shared_provider_config_rejects_unsafe_headers_and_temperature() {
        assert_eq!(parse_temperature(" 0.3 ").unwrap(), Some(0.3));
        assert!(parse_temperature("NaN").is_err());
        assert!(parse_temperature("2.1").is_err());
        assert!(parse_extra_headers(r#"{"x-trace":"ok"}"#).is_ok());
        assert!(parse_extra_headers(r#"{"Authorization":"secret"}"#).is_err());
    }
}
