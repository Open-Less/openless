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

use crate::asr::tencent_cloud::TencentCloudASRError;
use crate::asr::{
    BailianCredentials, BailianRealtimeASR, DashScopeMultimodalASR, DictionaryHotword,
    ElevenLabsBatchASR, GeminiBatchASR, MimoBatchASR, Qwen3RealtimeASR, Qwen3RealtimeCredentials,
    StepfunRealtimeASR, StepfunRealtimeCredentials, TencentCloudCredentials,
    TencentCloudStreamingASR, VolcengineCredentials, VolcengineStreamingASR, WhisperBatchASR,
    XfyunCredentials, XfyunStreamingASR,
};
use crate::config::{TaskSpawner, TokioTaskSpawner};
use crate::credentials::{
    CredentialKey, CredentialNamespace, CredentialStore, ASR_ADVANCED_CONFIG_ACCOUNT,
    ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT, ASR_VOCABULARY_ID_ACCOUNT,
    LLM_API_KEY_ACCOUNT, LLM_ENDPOINT_ACCOUNT, LLM_EXTRA_HEADERS_ACCOUNT, LLM_TEMPERATURE_ACCOUNT,
    OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_EXTRA_HEADERS_ACCOUNT, OMNI_MODEL_ACCOUNT,
    OMNI_TEMPERATURE_ACCOUNT, TENCENT_CLOUD_APP_ID_ACCOUNT, TENCENT_CLOUD_SECRET_ID_ACCOUNT,
    TENCENT_CLOUD_SECRET_KEY_ACCOUNT, VOLCENGINE_ACCESS_KEY_ACCOUNT, VOLCENGINE_API_KEY_ACCOUNT,
    VOLCENGINE_APP_KEY_ACCOUNT, VOLCENGINE_AUTH_MODE_ACCOUNT, VOLCENGINE_RESOURCE_ID_ACCOUNT,
    XFYUN_API_KEY_ACCOUNT, XFYUN_APP_ID_ACCOUNT,
};
use crate::dictation_context::{DictationAudioSource, DictationContext};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{
    ActiveRecording, AudioConsumer, AudioRecorder, CaptureStartRequest, DictationEngine,
    EngineFailure, EngineFailureStage, EngineProgress, EngineProgressSink, EngineResult,
    EngineStage, PendingAudioCapture, PolishOutput, PreparedTranscription, RecordingProgressSink,
    TextPolisher, TextStreamChunk, TextStreamSink, TranscriptOutput, TranscriptionEngine,
    TranscriptionSession,
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
    "orcarouter",
    "zenmux",
    "minimax",
    "openai-compatible",
    "xiaomi-mimo-asr",
    "iflytek",
    "tencent-cloud",
    "gemini",
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
    "requesty",
    "api-route",
    "orcarouter",
    "alibabaCoding",
    "codingPlanX",
    "minimax",
    "stepfun",
    "opencode",
    "tencentTokenHub",
    "lmstudio",
    "custom",
    "custom_responses",
    "custom_messages",
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

const ASR_SNAPSHOT_ACCOUNTS: &[&str] = &[
    ASR_MODEL_ACCOUNT,
    ASR_API_KEY_ACCOUNT,
    ASR_ENDPOINT_ACCOUNT,
    ASR_ADVANCED_CONFIG_ACCOUNT,
    ASR_VOCABULARY_ID_ACCOUNT,
    crate::credentials::VOLCENGINE_SERVICE_ACCOUNT,
    VOLCENGINE_AUTH_MODE_ACCOUNT,
    VOLCENGINE_APP_KEY_ACCOUNT,
    VOLCENGINE_ACCESS_KEY_ACCOUNT,
    VOLCENGINE_API_KEY_ACCOUNT,
    VOLCENGINE_RESOURCE_ID_ACCOUNT,
    XFYUN_APP_ID_ACCOUNT,
    XFYUN_API_KEY_ACCOUNT,
    TENCENT_CLOUD_APP_ID_ACCOUNT,
    TENCENT_CLOUD_SECRET_ID_ACCOUNT,
    TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
];

#[derive(Clone)]
struct CloudTranscriptionPreparation {
    context: Arc<DictationContext>,
    provider_type: String,
    effective_provider: String,
    bailian_protocol: crate::provider_rules::BailianProtocol,
    kind: crate::provider_rules::ActiveAsrProviderKind,
    model: String,
    api_key: String,
    endpoint: String,
    advanced_config: crate::provider_rules::AdvancedAsrConfig,
    credentials: HashMap<&'static str, Option<String>>,
}

impl CloudTranscriptionPreparation {
    fn value(&self, account: &str) -> Option<String> {
        self.credentials.get(account).cloned().flatten()
    }
}

struct PreparedCloudTranscription {
    preparation: CloudTranscriptionPreparation,
    task_spawner: Arc<dyn TaskSpawner>,
}

impl PreparedTranscription for PreparedCloudTranscription {
    fn start(
        &self,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let preparation = self.preparation.clone();
        let task_spawner = Arc::clone(&self.task_spawner);
        Box::pin(async move {
            let (kind, asr_call_label) = build_cloud_transcription_session(
                &preparation,
                Arc::clone(&task_spawner),
                partials,
            )
            .await?;
            Ok(Arc::new(CloudTranscriptionSession {
                kind,
                asr_call_label,
                finished: AtomicBool::new(false),
                task_spawner,
            }) as Arc<dyn TranscriptionSession>)
        })
    }
}

#[derive(Clone)]
enum CloudTranscriptionSessionKind {
    Volcengine(Arc<VolcengineStreamingASR>),
    Whisper(Arc<WhisperBatchASR>),
    Mimo(Arc<MimoBatchASR>),
    DashScope(Arc<DashScopeMultimodalASR>),
    ElevenLabs(Arc<ElevenLabsBatchASR>),
    Gemini(Arc<GeminiBatchASR>),
    GeminiLive(Arc<crate::asr::gemini_live::GeminiLiveASR>),
    Bailian(Arc<BailianRealtimeASR>),
    QwenRealtime(Arc<Qwen3RealtimeASR>),
    StepfunRealtime(Arc<StepfunRealtimeASR>),
    Xfyun(Arc<XfyunStreamingASR>),
    TencentCloud(Arc<TencentCloudStreamingASR>),
}

struct CloudTranscriptionSession {
    kind: CloudTranscriptionSessionKind,
    asr_call_label: crate::AsrCallLabel,
    finished: AtomicBool,
    task_spawner: Arc<dyn TaskSpawner>,
}

impl TranscriptionEngine for SharedCloudTranscriptionEngine {
    fn prepare(
        self: Arc<Self>,
        _session_id: SessionId,
        context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<Arc<dyn PreparedTranscription>, BackendError>> {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            let preparation =
                prepare_cloud_transcription(credentials.as_ref(), Arc::clone(&context)).await?;
            Ok(Arc::new(PreparedCloudTranscription {
                preparation,
                task_spawner: Arc::clone(&self.task_spawner),
            }) as Arc<dyn PreparedTranscription>)
        })
    }

    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let engine = Arc::new(self.clone());
        Box::pin(async move {
            let prepared = engine.prepare(session_id, context).await?;
            prepared.start(partials).await
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
            CloudTranscriptionSessionKind::Gemini(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::GeminiLive(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::Bailian(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::QwenRealtime(provider) => {
                provider.consume_pcm_chunk(pcm)
            }
            CloudTranscriptionSessionKind::StepfunRealtime(provider) => {
                provider.consume_pcm_chunk(pcm)
            }
            CloudTranscriptionSessionKind::Xfyun(provider) => provider.consume_pcm_chunk(pcm),
            CloudTranscriptionSessionKind::TencentCloud(provider) => {
                provider.consume_pcm_chunk(pcm)
            }
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
        let task_spawner = Arc::clone(&self.task_spawner);
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
                CloudTranscriptionSessionKind::Gemini(provider) => {
                    let worker = Arc::clone(&provider);
                    let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
                    task_spawner.spawn(Box::pin(async move {
                        let _ = result_sender.send(worker.transcribe().await);
                    }));
                    // Verify may drop finish on cancellation; the worker still owns file cleanup.
                    let mut cleanup = crate::asr::gemini::CancelOnDrop {
                        provider: Some(provider),
                        task_spawner,
                    };
                    let result = result_receiver.await;
                    cleanup.provider = None;
                    result
                        .map_err(|_| {
                            BackendError::new(
                                BackendErrorCode::Internal,
                                "Gemini transcription task failed",
                            )
                        })?
                        .map_err(map_asr_error)?
                }
                CloudTranscriptionSessionKind::GeminiLive(provider) => {
                    provider.finish().await.map_err(map_asr_error)?
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
                CloudTranscriptionSessionKind::TencentCloud(provider) => {
                    provider
                        .send_last_frame()
                        .await
                        .map_err(map_tencent_asr_error)?;
                    provider
                        .await_final_result()
                        .await
                        .map_err(map_tencent_asr_error)?
                }
            };
            Ok(TranscriptOutput {
                text: transcript.text,
                duration_ms: transcript.duration_ms,
            })
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        if let CloudTranscriptionSessionKind::Gemini(provider) = &self.kind {
            let provider = Arc::clone(provider);
            return Box::pin(async move {
                provider.cancel().await;
                Ok(())
            });
        }
        match &self.kind {
            CloudTranscriptionSessionKind::Volcengine(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Whisper(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Mimo(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::DashScope(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::ElevenLabs(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::GeminiLive(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Bailian(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::QwenRealtime(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::StepfunRealtime(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Xfyun(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::TencentCloud(provider) => provider.cancel(),
            CloudTranscriptionSessionKind::Gemini(_) => unreachable!(),
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

async fn read_asr_channel_credentials(
    credentials: &dyn CredentialStore,
    channel_id: &str,
) -> Result<HashMap<&'static str, Option<String>>, BackendError> {
    let mut values = HashMap::new();
    for account in ASR_SNAPSHOT_ACCOUNTS {
        values.insert(
            *account,
            read_channel_credential(credentials, CredentialNamespace::Asr, channel_id, account)
                .await?,
        );
    }
    Ok(values)
}

pub(crate) async fn asr_channel_configured(
    credentials: &dyn CredentialStore,
    asr: &crate::dictation_context::ProviderInvocation,
) -> Result<bool, BackendError> {
    let values = read_asr_channel_credentials(credentials, &asr.provider_id).await?;
    let value = |account: &str| {
        values
            .get(account)
            .and_then(|value| value.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let configuration = crate::provider_rules::CredentialConfiguration {
        asr_api_key: value(ASR_API_KEY_ACCOUNT).is_some(),
        asr_endpoint: value(ASR_ENDPOINT_ACCOUNT).is_some(),
        asr_model: asr
            .model
            .as_deref()
            .is_some_and(|model| !model.trim().is_empty())
            || value(ASR_MODEL_ACCOUNT).is_some(),
        volcengine_service: value(crate::credentials::VOLCENGINE_SERVICE_ACCOUNT)
            .map(str::to_string),
        volcengine_auth_mode: value(VOLCENGINE_AUTH_MODE_ACCOUNT).map(str::to_string),
        volcengine_app_key: value(VOLCENGINE_APP_KEY_ACCOUNT).is_some(),
        volcengine_access_key: value(VOLCENGINE_ACCESS_KEY_ACCOUNT).is_some(),
        volcengine_api_key: value(VOLCENGINE_API_KEY_ACCOUNT).is_some(),
        xfyun_app_id: value(XFYUN_APP_ID_ACCOUNT).is_some(),
        xfyun_api_key: value(XFYUN_API_KEY_ACCOUNT).is_some(),
        tencent_cloud_app_id: value(TENCENT_CLOUD_APP_ID_ACCOUNT).is_some(),
        tencent_cloud_secret_id: value(TENCENT_CLOUD_SECRET_ID_ACCOUNT).is_some(),
        tencent_cloud_secret_key: value(TENCENT_CLOUD_SECRET_KEY_ACCOUNT).is_some(),
        ..Default::default()
    };
    Ok(crate::provider_rules::asr_configured(
        &asr.provider_type,
        &configuration,
        None,
    ))
}

async fn prepare_cloud_transcription(
    credentials: &dyn CredentialStore,
    context: Arc<DictationContext>,
) -> Result<CloudTranscriptionPreparation, BackendError> {
    let channel_id = context.asr.provider_id.trim().to_string();
    let provider_type = context.asr.provider_type.trim().to_string();
    if channel_id.is_empty() || provider_type.is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "ASR channel id and provider type must not be empty",
        ));
    }
    if provider_descriptor(crate::ProviderKind::Asr, &provider_type)
        .is_none_or(|descriptor| descriptor.validation_probe == ValidationProbe::Unsupported)
    {
        return Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "shared cloud ASR provider is not supported",
        ));
    }
    let values = read_asr_channel_credentials(credentials, &channel_id).await?;
    let value = |account: &str| values.get(account).cloned().flatten();
    let model = context
        .asr
        .model
        .clone()
        .or_else(|| value(ASR_MODEL_ACCOUNT))
        .unwrap_or_default();
    let bailian_protocol = crate::provider_rules::BailianProtocol::from_config(
        &provider_type,
        value(ASR_ADVANCED_CONFIG_ACCOUNT).as_deref(),
    )
    .map_err(|message| BackendError::new(BackendErrorCode::InvalidArgument, message))?;
    let effective_provider = bailian_protocol
        .resolve_provider(&provider_type, &model)
        .map_err(|message| BackendError::new(BackendErrorCode::InvalidArgument, message))?;
    let advanced_config = crate::provider_rules::advanced_asr_config_for(
        &provider_type,
        value(ASR_ADVANCED_CONFIG_ACCOUNT).as_deref(),
    );
    let preparation = CloudTranscriptionPreparation {
        context,
        provider_type,
        kind: crate::provider_rules::active_asr_provider_kind(&effective_provider),
        effective_provider,
        bailian_protocol,
        api_key: value(ASR_API_KEY_ACCOUNT).unwrap_or_default(),
        endpoint: value(ASR_ENDPOINT_ACCOUNT).unwrap_or_default(),
        advanced_config,
        model,
        credentials: values,
    };
    validate_cloud_transcription_preparation(&preparation)?;
    Ok(preparation)
}

fn validate_cloud_transcription_preparation(
    preparation: &CloudTranscriptionPreparation,
) -> Result<(), BackendError> {
    use crate::asr::volcengine::VolcengineAuthMode;
    use crate::provider_rules::ActiveAsrProviderKind;

    match preparation.kind {
        ActiveAsrProviderKind::Bailian
        | ActiveAsrProviderKind::Qwen3Realtime
        | ActiveAsrProviderKind::StepfunRealtime
        | ActiveAsrProviderKind::Mimo
        | ActiveAsrProviderKind::DashScopeMultimodal
        | ActiveAsrProviderKind::ElevenLabs
        | ActiveAsrProviderKind::Gemini => {
            require_configured(&preparation.api_key, "ASR API key")?;
        }
        ActiveAsrProviderKind::WhisperCompatible => {
            if crate::provider_rules::api_key_required(
                crate::ProviderKind::Asr,
                &preparation.provider_type,
                Some(&preparation.endpoint),
            ) {
                require_configured(&preparation.api_key, "ASR API key")?;
            }
        }
        ActiveAsrProviderKind::Volcengine => {
            let service = crate::asr::volcengine::VolcengineService::parse(
                preparation
                    .value(crate::credentials::VOLCENGINE_SERVICE_ACCOUNT)
                    .as_deref()
                    .unwrap_or_default(),
            )
            .map_err(|message| BackendError::new(BackendErrorCode::InvalidArgument, message))?;
            let configured = preparation
                .value(VOLCENGINE_AUTH_MODE_ACCOUNT)
                .map(|value| VolcengineAuthMode::parse(&value))
                .unwrap_or(VolcengineAuthMode::AppIdToken);
            let auth_mode = service.auth_mode(configured);
            let app_id = preparation
                .value(VOLCENGINE_APP_KEY_ACCOUNT)
                .unwrap_or_default();
            let secret_account = match auth_mode {
                VolcengineAuthMode::AppIdToken => VOLCENGINE_ACCESS_KEY_ACCOUNT,
                VolcengineAuthMode::ApiKey => VOLCENGINE_API_KEY_ACCOUNT,
            };
            let secret = preparation.value(secret_account).unwrap_or_default();
            if !auth_mode.auth_ok(&app_id, &secret) {
                return Err(credential_missing("Volcengine credentials"));
            }
        }
        ActiveAsrProviderKind::Xfyun => {
            require_configured(
                &preparation.value(XFYUN_APP_ID_ACCOUNT).unwrap_or_default(),
                "Xfyun application id",
            )?;
            require_configured(
                &preparation.value(XFYUN_API_KEY_ACCOUNT).unwrap_or_default(),
                "Xfyun API key",
            )?;
        }
        ActiveAsrProviderKind::TencentCloud => {
            require_configured(
                &preparation
                    .value(TENCENT_CLOUD_APP_ID_ACCOUNT)
                    .unwrap_or_default(),
                "Tencent Cloud AppID",
            )?;
            require_configured(
                &preparation
                    .value(TENCENT_CLOUD_SECRET_ID_ACCOUNT)
                    .unwrap_or_default(),
                "Tencent Cloud SecretID",
            )?;
            require_configured(
                &preparation
                    .value(TENCENT_CLOUD_SECRET_KEY_ACCOUNT)
                    .unwrap_or_default(),
                "Tencent Cloud SecretKey",
            )?;
        }
    }
    Ok(())
}

async fn build_cloud_transcription_session(
    preparation: &CloudTranscriptionPreparation,
    task_spawner: Arc<dyn TaskSpawner>,
    partials: Arc<dyn TextStreamSink>,
) -> Result<(CloudTranscriptionSessionKind, crate::AsrCallLabel), BackendError> {
    use crate::asr::volcengine::VolcengineAuthMode;
    use crate::provider_rules::{ActiveAsrProviderKind, BailianEndpointProtocol};

    let provider_type = preparation.provider_type.as_str();
    let model = preparation.model.clone();
    let api_key = preparation.api_key.clone();
    let endpoint = preparation.endpoint.clone();
    let context = &preparation.context;
    let (kind, label_model) = match preparation.kind {
        ActiveAsrProviderKind::Bailian => {
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
            let vocabulary_id = preparation
                .value(ASR_VOCABULARY_ID_ACCOUNT)
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
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| default_asr_model(provider_type).unwrap().to_string());
            let endpoint = non_blank_owned(endpoint)
                .unwrap_or_else(|| default_asr_endpoint(provider_type).unwrap().to_string());
            let provider = if provider_type == crate::asr::mimo::ORCAROUTER_PROVIDER_ID {
                MimoBatchASR::new_orcarouter(api_key, endpoint, effective_model.clone())
            } else {
                MimoBatchASR::new(api_key, endpoint, effective_model.clone())
            };
            (
                CloudTranscriptionSessionKind::Mimo(Arc::new(provider)),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::DashScopeMultimodal => {
            let model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::dashscope_multimodal::DEFAULT_MODEL.to_string());
            let stored_endpoint = non_blank_owned(endpoint)
                .unwrap_or_else(|| crate::asr::dashscope_multimodal::DEFAULT_ENDPOINT.to_string());
            let endpoint = if provider_type == crate::asr::bailian::PROVIDER_ID {
                let protocol = match preparation.bailian_protocol.batch_protocol(&model) {
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
                CloudTranscriptionSessionKind::DashScope(Arc::new(
                    DashScopeMultimodalASR::new(api_key, endpoint, model.clone())
                        .with_protocol(preparation.bailian_protocol),
                )),
                Some(model),
            )
        }
        ActiveAsrProviderKind::ElevenLabs => {
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
        ActiveAsrProviderKind::Gemini => {
            let effective_model = non_blank_owned(model)
                .unwrap_or_else(|| crate::asr::gemini::DEFAULT_MODEL.to_string());
            crate::provider_rules::validate_gemini_asr_model(&effective_model)
                .map_err(|message| BackendError::new(BackendErrorCode::InvalidArgument, message))?;
            let endpoint = non_blank_owned(endpoint)
                .unwrap_or_else(|| crate::asr::gemini::DEFAULT_ENDPOINT.to_string());
            let hotwords = context.polish.hotwords.clone();
            let kind = if effective_model == crate::asr::gemini_live::MODEL {
                let config = crate::asr::gemini_live::GeminiLiveConfig {
                    api_key,
                    base_url: endpoint,
                    languages: context.asr.language.clone().into_iter().collect(),
                    hotwords,
                    mode: crate::asr::gemini::TranscriptionMode::Verbatim,
                    extra_headers: HashMap::new(),
                };
                CloudTranscriptionSessionKind::GeminiLive(
                    crate::asr::gemini_live::GeminiLiveASR::connect(config, partials, task_spawner)
                        .await
                        .map_err(map_asr_error)?,
                )
            } else {
                CloudTranscriptionSessionKind::Gemini(Arc::new(GeminiBatchASR::new(
                    api_key,
                    endpoint,
                    effective_model.clone(),
                    context.asr.language.clone(),
                    hotwords,
                )))
            };
            (kind, Some(effective_model))
        }
        ActiveAsrProviderKind::WhisperCompatible => {
            let default_endpoint = default_asr_endpoint(provider_type).unwrap_or("");
            let default_model = default_asr_model(provider_type).unwrap_or("whisper-1");
            let effective_model =
                non_blank_owned(model).unwrap_or_else(|| default_model.to_string());
            let mut provider = WhisperBatchASR::new(
                api_key,
                non_blank_owned(endpoint).unwrap_or_else(|| default_endpoint.to_string()),
                effective_model.clone(),
                context.asr.prompt.clone(),
                crate::provider_rules::batch_asr_chunk_limit_ms(
                    provider_type,
                    preparation.advanced_config,
                ),
                crate::provider_rules::whisper_supports_verbose_json(
                    provider_type,
                    preparation.advanced_config,
                ),
            )
            .with_request_format(crate::provider_rules::whisper_request_format(provider_type));
            if provider_type == "minimax" {
                provider = provider.with_endpoint_path("/speech_to_text");
            }
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
                    .with_enable_itn(preparation.advanced_config.enable_itn);
            }
            (
                CloudTranscriptionSessionKind::Whisper(Arc::new(provider)),
                Some(effective_model),
            )
        }
        ActiveAsrProviderKind::Volcengine => {
            let service = crate::asr::volcengine::VolcengineService::parse(
                preparation
                    .value(crate::credentials::VOLCENGINE_SERVICE_ACCOUNT)
                    .as_deref()
                    .unwrap_or_default(),
            )
            .map_err(|message| BackendError::new(BackendErrorCode::InvalidArgument, message))?;
            let auth_mode = preparation
                .value(VOLCENGINE_AUTH_MODE_ACCOUNT)
                .map(|value| VolcengineAuthMode::parse(&value))
                .unwrap_or(VolcengineAuthMode::AppIdToken);
            let auth_mode = service.auth_mode(auth_mode);
            let app_id = preparation
                .value(VOLCENGINE_APP_KEY_ACCOUNT)
                .unwrap_or_default();
            let secret_account = match auth_mode {
                VolcengineAuthMode::AppIdToken => VOLCENGINE_ACCESS_KEY_ACCOUNT,
                VolcengineAuthMode::ApiKey => VOLCENGINE_API_KEY_ACCOUNT,
            };
            let access_token = preparation.value(secret_account).unwrap_or_default();
            let resource_id = preparation.value(VOLCENGINE_RESOURCE_ID_ACCOUNT);
            let credentials = VolcengineCredentials {
                service,
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
            let app_id = preparation.value(XFYUN_APP_ID_ACCOUNT).unwrap_or_default();
            let api_key = preparation.value(XFYUN_API_KEY_ACCOUNT).unwrap_or_default();
            let provider = Arc::new(XfyunStreamingASR::with_task_spawner(
                XfyunCredentials { app_id, api_key },
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider.open_session().await.map_err(map_asr_error)?;
            (CloudTranscriptionSessionKind::Xfyun(provider), None)
        }
        ActiveAsrProviderKind::TencentCloud => {
            let app_id = preparation
                .value(TENCENT_CLOUD_APP_ID_ACCOUNT)
                .unwrap_or_default();
            let secret_id = preparation
                .value(TENCENT_CLOUD_SECRET_ID_ACCOUNT)
                .unwrap_or_default();
            let secret_key = preparation
                .value(TENCENT_CLOUD_SECRET_KEY_ACCOUNT)
                .unwrap_or_default();
            let model = non_blank_owned(preparation.model.clone())
                .unwrap_or_else(|| crate::asr::tencent_cloud::DEFAULT_MODEL.to_string());
            let provider = Arc::new(TencentCloudStreamingASR::with_task_spawner(
                TencentCloudCredentials {
                    app_id,
                    secret_id,
                    secret_key,
                    model: model.clone(),
                },
                Arc::clone(&task_spawner),
            ));
            provider.set_partial_sink(Arc::clone(&partials));
            provider
                .open_session()
                .await
                .map_err(map_tencent_asr_error)?;
            (
                CloudTranscriptionSessionKind::TencentCloud(provider),
                Some(model),
            )
        }
    };
    Ok((
        kind,
        crate::AsrCallLabel::new(preparation.effective_provider.clone(), label_model),
    ))
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

fn map_tencent_asr_error(error: TencentCloudASRError) -> BackendError {
    let retryable = matches!(
        &error,
        TencentCloudASRError::ConnectionFailed
            | TencentCloudASRError::RateLimited
            | TencentCloudASRError::ServiceUnavailable(_)
            | TencentCloudASRError::NoFinalResult
            | TencentCloudASRError::FinalResultTimeout
    );
    BackendError::new(
        BackendErrorCode::Provider,
        format!("Tencent Cloud ASR failed: {error}"),
    )
    .retryable(retryable)
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
            let mut provider =
                build_cloud_polisher_provider(credentials.as_ref(), context.as_ref()).await?;
            if let CloudPolisherProvider::OpenAi(crate::polish::ActiveLLMProvider::OpenAI(
                provider,
            )) = &mut provider
            {
                provider.set_dictation_session(session_id);
            }
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

struct PolishStreamPublisher {
    sink: Arc<dyn TextStreamSink>,
    cancellation: Arc<ProviderCancellation>,
    state: Mutex<PolishStreamState>,
}

#[derive(Default)]
struct PolishStreamState {
    translation: Option<crate::prompt_compose::PolishTranslationStream>,
    offset: u64,
    error: Option<BackendError>,
}

impl PolishStreamPublisher {
    fn new(
        sink: Arc<dyn TextStreamSink>,
        cancellation: Arc<ProviderCancellation>,
        translation: Option<crate::prompt_compose::PolishTranslationStream>,
    ) -> Self {
        Self {
            sink,
            cancellation,
            state: Mutex::new(PolishStreamState {
                translation,
                ..PolishStreamState::default()
            }),
        }
    }

    fn publish(&self, delta: &str) {
        let mut state = self.state.lock();
        if self.cancellation.is_cancelled() || state.error.is_some() {
            return;
        }
        let text = match state.translation.as_mut() {
            Some(translation) => translation.push(delta),
            None => delta.to_owned(),
        };
        if text.is_empty() {
            return;
        }
        let offset = state.offset;
        state.offset += text.chars().count() as u64;
        if let Err(error) = self.sink.publish(TextStreamChunk { text, offset }) {
            state.error = Some(error);
        }
    }

    fn should_cancel(&self) -> bool {
        self.cancellation.is_cancelled() || self.state.lock().error.is_some()
    }

    fn take_error(&self) -> Option<BackendError> {
        self.state.lock().error.take()
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
    let stream = PolishStreamPublisher::new(
        Arc::clone(&partials),
        Arc::clone(&cancellation),
        context
            .polish
            .translation_active
            .then(crate::prompt_compose::PolishTranslationStream::default),
    );
    let on_delta = |delta: &str| stream.publish(delta);
    let should_cancel = || stream.should_cancel();
    let result = match &provider {
        CloudPolisherProvider::OpenAi(provider @ crate::polish::ActiveLLMProvider::Codex(_))
            if !context.polish.translation_active =>
        {
            provider
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
                    context.polish.edit_plan_input,
                )
                .await
        }
        CloudPolisherProvider::OpenAi(provider) => {
            provider
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
                    context.polish.edit_plan_input,
                    on_delta,
                    should_cancel,
                )
                .await
        }
        CloudPolisherProvider::Gemini(provider) if !context.polish.translation_active => {
            provider
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
                    context.polish.edit_plan_input,
                )
                .await
        }
        CloudPolisherProvider::Gemini(provider) => {
            provider
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
                    context.polish.edit_plan_input,
                    on_delta,
                    should_cancel,
                )
                .await
        }
    };
    if let Some(error) = stream.take_error() {
        return Err(error);
    }
    let polished = result.map_err(map_polish_error)?;
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
                // No target section was published, so fallback starts at offset zero.
                let stream = PolishStreamPublisher::new(
                    Arc::clone(&partials),
                    Arc::clone(&cancellation),
                    Some(crate::prompt_compose::PolishTranslationStream::plain()),
                );
                let on_delta = |delta: &str| stream.publish(delta);
                let should_cancel = || stream.should_cancel();
                let result = match &provider {
                    CloudPolisherProvider::OpenAi(provider) => {
                        provider
                            .translate_to_streaming(
                                &raw_text,
                                &context.polish.translation_target_language,
                                &context.polish.working_languages,
                                context.polish.chinese_script_preference,
                                context.polish.output_language_preference,
                                context.polish.front_app.as_deref(),
                                on_delta,
                                should_cancel,
                            )
                            .await
                    }
                    CloudPolisherProvider::Gemini(provider) => {
                        provider
                            .translate_to_streaming(
                                &raw_text,
                                &context.polish.translation_target_language,
                                &context.polish.working_languages,
                                context.polish.chinese_script_preference,
                                context.polish.output_language_preference,
                                context.polish.front_app.as_deref(),
                                on_delta,
                                should_cancel,
                            )
                            .await
                    }
                };
                if let Some(error) = stream.take_error() {
                    return Err(error);
                }
                let text = result.map_err(map_polish_error)?;
                PolishOutput::text(text.trim().to_owned())
            }
        }
    } else {
        PolishOutput::text(polished)
    };
    if cancellation.is_cancelled() {
        return Err(cancelled_provider_error());
    }
    if output.text.trim().is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::Provider,
            "LLM provider returned empty polish output",
        ));
    }
    output.llm_call_label = Some(call_label);
    Ok(output)
}

/// Whether the LLM channel a session would polish through is configured enough
/// to send a request (issue #1032). Mirrors the checks in
/// `build_cloud_polisher_provider` without constructing a provider. A failed
/// credential read counts as configured so the existing error path still runs.
pub(crate) async fn llm_channel_configured(
    credentials: &dyn CredentialStore,
    channel_id: &str,
    provider_type: &str,
    model: Option<&str>,
) -> bool {
    let channel_id = channel_id.trim();
    let provider_type = provider_type.trim();
    if channel_id.is_empty() || provider_type.is_empty() {
        return false;
    }
    let has_model = model.map(str::trim).is_some_and(|value| !value.is_empty())
        || default_llm_model(provider_type).is_some();
    if !has_model {
        return false;
    }
    if provider_type == crate::polish::CODEX_OAUTH_PROVIDER_ID {
        return true;
    }
    let Ok(api_key) = read_channel_credential(
        credentials,
        CredentialNamespace::Llm,
        channel_id,
        LLM_API_KEY_ACCOUNT,
    )
    .await
    else {
        return true;
    };
    let Ok(endpoint) = read_channel_credential(
        credentials,
        CredentialNamespace::Llm,
        channel_id,
        LLM_ENDPOINT_ACCOUNT,
    )
    .await
    else {
        return true;
    };
    let api_key_blank = api_key.is_none_or(|value| value.trim().is_empty());
    if api_key_blank
        && crate::provider_rules::api_key_required(
            crate::ProviderKind::Llm,
            provider_type,
            endpoint.as_deref(),
        )
    {
        return false;
    }
    endpoint
        .and_then(non_blank_owned)
        .or_else(|| default_llm_endpoint(provider_type).map(str::to_string))
        .is_some()
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

    let protocol =
        crate::llm_protocol::LlmProtocolConfig::load(credentials, channel_id, provider_type)
            .await?;
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
        endpoint,
        api_key,
        model,
    )
    .with_protocol(protocol)
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

pub fn parse_temperature(value: &str) -> Result<Option<f32>, BackendError> {
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

/// A polish failure, marked retryable when the network never answered
/// (connection failure or timeout) — unlike a rejected key, exhausted quota or
/// malformed reply. The dictation pipeline uses it to tell "offline" apart
/// when the transcript came from a local model.
fn map_polish_error(error: crate::polish::LLMError) -> BackendError {
    let transient = matches!(
        error,
        crate::polish::LLMError::Network(_) | crate::polish::LLMError::Timeout
    );
    map_llm_error(error).retryable(transient)
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
    build_omni_provider_with_task_spawner(credentials, context, Arc::new(TokioTaskSpawner)).await
}

/// Audio entry points pass the host scheduler through to worker and drop cleanup.
async fn build_omni_provider_with_task_spawner(
    credentials: &dyn CredentialStore,
    context: &DictationContext,
    task_spawner: Arc<dyn TaskSpawner>,
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
    if provider_type == "gemini" && model.trim() == crate::asr::gemini_live::MODEL {
        crate::asr::gemini_live::transport::endpoint(&base_url).map_err(map_omni_error)?;
    } else {
        crate::endpoint_security::validate_http_endpoint(&base_url)
            .map_err(|error| map_omni_error(error.to_string()))?;
    }
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
    let provider = crate::omni::OmniProvider::with_task_spawner(config, task_spawner)
        .with_transcription_hints(&context.polish.working_languages, &context.polish.hotwords);
    validate_omni_context(&provider, context)?;
    Ok(provider)
}

/// Dedicated transcription cannot apply a translation prompt.
fn validate_omni_context(
    provider: &crate::omni::OmniProvider,
    context: &DictationContext,
) -> Result<(), BackendError> {
    if provider.is_transcription_only() && context.polish.translation_active {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "Gemini Transcribe does not support translation in Multimodal mode; use Traditional mode with a language model",
        ));
    }
    Ok(())
}

/// Validate an Omni provider using the same construction and request path as
/// dictation. Dedicated transcription uses synthetic silence; general-purpose
/// models use a text probe, which does not prove audio support (issue #1118).
pub async fn validate_shared_omni_provider(
    credentials: Arc<dyn CredentialStore>,
    context: Arc<DictationContext>,
    task_spawner: Arc<dyn TaskSpawner>,
) -> Result<(), BackendError> {
    let provider =
        build_omni_provider_with_task_spawner(credentials.as_ref(), context.as_ref(), task_spawner)
            .await?;
    if provider.is_transcription_only() {
        let wav = crate::asr::wav::encode_wav_16k_mono(&vec![0; 8_000]);
        provider
            .complete("", "", Some(&wav))
            .await
            .map_err(map_omni_error)?;
        // Completed silence legitimately produces no transcript.
        return Ok(());
    }
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

/// First-stage Omni for Selection Voice / Less Computer (#1119 product B):
/// audio + prompt → instruction/command text. Downstream intent / EditPlan /
/// Agent submit still use their existing LLM (or Agent) paths.
///
/// Empty Omni output is returned as `Ok("")` so callers can apply product
/// policy (e.g. Less Computer Dictate → `Empty` outcome without a chat error;
/// Selection Voice / Agent still treat emptiness as a provider failure).
pub(crate) async fn complete_omni_instruction_from_wav(
    credentials: &dyn CredentialStore,
    context: &DictationContext,
    wav_bytes: &[u8],
    system_prompt: &str,
    task_spawner: Arc<dyn TaskSpawner>,
) -> Result<String, BackendError> {
    let provider =
        build_omni_provider_with_task_spawner(credentials, context, task_spawner).await?;
    let result = provider
        .complete(system_prompt, "", Some(wav_bytes))
        .await
        .map_err(map_omni_error)?;
    Ok(result.trim().to_string())
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
    task_spawner: Arc<dyn TaskSpawner>,
    sessions: Arc<Mutex<HashMap<SessionId, Arc<OmniSession>>>>,
    pending_captures: Arc<Mutex<HashMap<SessionId, Box<dyn PendingAudioCapture>>>>,
}

impl SharedOmniDictationEngine {
    pub fn new(credentials: Arc<dyn CredentialStore>, recorder: Arc<dyn AudioRecorder>) -> Self {
        Self::with_task_spawner(credentials, recorder, Arc::new(TokioTaskSpawner))
    }

    /// Reuse the host runtime for transcription tasks, including cleanup after caller drop.
    pub fn with_task_spawner(
        credentials: Arc<dyn CredentialStore>,
        recorder: Arc<dyn AudioRecorder>,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            credentials,
            recorder,
            task_spawner,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_captures: Arc::new(Mutex::new(HashMap::new())),
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
    received_bytes: AtomicU64,
    live: Mutex<Option<Arc<crate::asr::gemini_live::GeminiLiveASR>>>,
}

impl OmniPcm {
    fn snapshot(&self) -> Vec<u8> {
        self.bytes.lock().clone()
    }

    fn duration_ms(&self) -> u64 {
        crate::asr::pcm::pcm_duration_ms_from_bytes(self.received_bytes.load(Ordering::Acquire))
    }

    /// Live owns its bounded audio queue; only batch models need a second complete PCM copy.
    fn append(&self, bytes: &mut Vec<u8>, pcm: &[u8]) {
        self.received_bytes
            .fetch_add(pcm.len() as u64, Ordering::AcqRel);
        if let Some(live) = self.live.lock().as_ref() {
            live.consume_pcm_chunk(pcm);
        } else {
            bytes.extend_from_slice(pcm);
        }
    }
}

impl AudioConsumer for OmniPcm {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let mut bytes = self.bytes.lock();
        self.append(&mut bytes, pcm);
    }
}

impl OmniSession {
    /// Session registration precedes setup so cancel can interrupt a pending Live handshake.
    async fn start_live(&self, session_id: SessionId) -> Result<(), BackendError> {
        if self.context.read().recording.transcribe_after_stop {
            return Ok(());
        }
        let partials = Arc::new(OmniLivePreview {
            session_id,
            progress: Arc::clone(&self.progress),
            cancellation: Arc::clone(&self.cancellation),
        });
        let live = tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => return Err(cancelled_omni_error()),
            result = self.provider.start_live_transcription(partials) => result.map_err(map_omni_error)?,
        };
        *self.pcm.live.lock() = live;
        if self.cancellation.is_cancelled() {
            if let Some(live) = self.pcm.live.lock().as_ref() {
                live.cancel();
            }
            return Err(cancelled_omni_error());
        }
        Ok(())
    }
}

struct OmniLivePreview {
    session_id: SessionId,
    progress: Arc<dyn EngineProgressSink>,
    cancellation: Arc<ProviderCancellation>,
}

/// Remove a registered Live session even when its caller drops a start/finish future.
struct OmniSessionGuard {
    sessions: Arc<Mutex<HashMap<SessionId, Arc<OmniSession>>>>,
    session_id: SessionId,
    session: Arc<OmniSession>,
    armed: bool,
}

impl OmniSessionGuard {
    fn new(
        sessions: &Arc<Mutex<HashMap<SessionId, Arc<OmniSession>>>>,
        session_id: SessionId,
        session: &Arc<OmniSession>,
    ) -> Self {
        Self {
            sessions: Arc::clone(sessions),
            session_id,
            session: Arc::clone(session),
            armed: true,
        }
    }
}

impl Drop for OmniSessionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.session.cancellation.cancel();
            remove_omni_session(&self.sessions, self.session_id, &self.session);
        }
    }
}

impl TextStreamSink for OmniLivePreview {
    fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
        if self.cancellation.is_cancelled() {
            return Err(cancelled_omni_error());
        }
        self.progress.publish(
            self.session_id,
            EngineProgress::TranscriptDelta(crate::TranscriptDelta {
                text: chunk.text,
                offset: chunk.offset,
                is_final: false,
            }),
        )
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
    fn arm_audio_capture(
        &self,
        request: CaptureStartRequest,
    ) -> Result<Box<dyn PendingAudioCapture>, BackendError> {
        self.recorder.arm_capture(request)
    }

    fn start_with_audio_capture(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
        capture: Box<dyn PendingAudioCapture>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let credentials = Arc::clone(&self.credentials);
        let sessions = Arc::clone(&self.sessions);
        let pending_captures = Arc::clone(&self.pending_captures);
        let task_spawner = Arc::clone(&self.task_spawner);
        Box::pin(async move {
            let mut capture = Some(capture);
            let duplicate_pending = {
                let mut pending = pending_captures.lock();
                if let std::collections::hash_map::Entry::Vacant(entry) = pending.entry(session_id)
                {
                    entry.insert(capture.take().expect("Omni capture was not consumed"));
                    false
                } else {
                    true
                }
            };
            if duplicate_pending {
                let _ = capture
                    .take()
                    .expect("duplicate Omni capture was lost")
                    .discard()
                    .await;
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "Omni provisional capture already exists",
                ));
            }
            let provider = match build_omni_provider_with_task_spawner(
                credentials.as_ref(),
                &context,
                task_spawner,
            )
            .await
            {
                Ok(provider) => Arc::new(provider),
                Err(error) => {
                    let pending_capture = pending_captures.lock().remove(&session_id);
                    if let Some(capture) = pending_capture {
                        let _ = capture.discard().await;
                    }
                    return Err(error);
                }
            };
            let Some(capture) = pending_captures.lock().remove(&session_id) else {
                return Err(cancelled_omni_error());
            };
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
            let duplicate = {
                let mut active = sessions.lock();
                match active.entry(session_id) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(Arc::clone(&session));
                        false
                    }
                    std::collections::hash_map::Entry::Occupied(_) => true,
                }
            };
            if duplicate {
                let _ = capture.discard().await;
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "Omni dictation session already exists",
                ));
            }
            let mut cleanup = OmniSessionGuard::new(&sessions, session_id, &session);
            if let Err(error) = session.start_live(session_id).await {
                let _ = capture.discard().await;
                remove_omni_session(&sessions, session_id, &session);
                return Err(error);
            }
            if context.audio_source == DictationAudioSource::External {
                let _ = capture.discard().await;
                if session.cancellation.is_cancelled() {
                    return Err(cancelled_omni_error());
                }
                cleanup.armed = false;
                return Ok(());
            }
            if session.cancellation.is_cancelled() {
                let _ = capture.discard().await;
                remove_omni_session(&sessions, session_id, &session);
                return Err(cancelled_omni_error());
            }
            let consumer: Arc<dyn AudioConsumer> = pcm;
            let level_progress: Arc<dyn RecordingProgressSink> = Arc::new(OmniRecordingProgress {
                session_id,
                progress,
            });
            let recording = match capture
                .commit(session_id, Arc::clone(&context), consumer, level_progress)
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
            cleanup.armed = false;
            Ok(())
        })
    }

    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        progress: Arc<dyn EngineProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let credentials = Arc::clone(&self.credentials);
        let recorder = Arc::clone(&self.recorder);
        let sessions = Arc::clone(&self.sessions);
        let task_spawner = Arc::clone(&self.task_spawner);
        Box::pin(async move {
            let provider = Arc::new(
                build_omni_provider_with_task_spawner(credentials.as_ref(), &context, task_spawner)
                    .await?,
            );
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
            let mut cleanup = OmniSessionGuard::new(&sessions, session_id, &session);
            if let Err(error) = session.start_live(session_id).await {
                remove_omni_session(&sessions, session_id, &session);
                return Err(error);
            }
            if context.audio_source == DictationAudioSource::External {
                if session.cancellation.is_cancelled() {
                    remove_omni_session(&sessions, session_id, &session);
                    return Err(cancelled_omni_error());
                }
                cleanup.armed = false;
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
            cleanup.armed = false;
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
            let _cleanup = OmniSessionGuard::new(&sessions, session_id, &session);
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
            let duration_ms = session.pcm.duration_ms();
            if session.cancellation.is_cancelled() {
                remove_omni_session(&sessions, session_id, &session);
                return Err(EngineFailure::from(cancelled_omni_error()));
            }
            let live = session.pcm.live.lock().clone();
            let is_live = live.is_some();
            // Live can revise the transcript until finalization has drained the worker.
            let stage = if is_live {
                EngineStage::Transcribing
            } else {
                EngineStage::Polishing
            };
            progress
                .publish(session_id, EngineProgress::Stage(stage))
                .map_err(EngineFailure::from)?;
            let wav = if live.is_some() {
                Vec::new()
            } else {
                crate::asr::wav::encode_wav_16k_mono(
                    &session
                        .pcm
                        .snapshot()
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
                        .collect::<Vec<_>>(),
                )
            };
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
                biased;
                _ = cancellation.cancelled() => Err(cancelled_omni_error()),
                result = async {
                    if let Some(live) = live {
                        live.finish().await.map(|transcript| transcript.text).map_err(map_asr_error)
                    } else {
                        provider.complete_streaming(
                            &system_prompt, "", Some(&wav), on_delta, should_cancel,
                        ).await.map_err(map_omni_error)
                    }
                } => result,
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
            if session.cancellation.is_cancelled() {
                return Err(EngineFailure::from(cancelled_omni_error()));
            }
            if is_live {
                progress
                    .publish(session_id, EngineProgress::Stage(EngineStage::Polishing))
                    .map_err(EngineFailure::from)?;
            }
            if !context.recording.archive_successful_recording {
                if let Some(archive) = archive.as_ref() {
                    if archive.is_available() {
                        let _ = archive.discard().await;
                    }
                }
            }
            if session.cancellation.is_cancelled() {
                return Err(EngineFailure::from(cancelled_omni_error()));
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
            validate_omni_context(&session.provider, &context)?;
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
            session.pcm.append(&mut bytes, pcm);
            (
                session.pcm.duration_ms(),
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
        let pending_capture = self.pending_captures.lock().remove(&session_id);
        let sessions = Arc::clone(&self.sessions);
        Box::pin(async move {
            if let Some(capture) = pending_capture {
                capture.discard().await?;
            }
            let Some(session) = session else {
                return Ok(());
            };
            let _cleanup = OmniSessionGuard::new(&sessions, session_id, &session);
            session.cancellation.cancel();
            if let Some(live) = session.pcm.live.lock().as_ref() {
                live.cancel();
            }
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
    if let Some(live) = expected.pcm.live.lock().as_ref() {
        live.cancel();
    }
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
    #[tokio::test]
    async fn manual_bailian_protocol_is_snapshotted_for_recording() {
        use super::*;
        use crate::provider_rules::BailianProtocol;
        let credentials = crate::InMemoryCredentialStore::default();
        for (name, expected) in [
            ("dashscope-realtime", "bailian"),
            ("qwen-realtime", "bailian-qwen3-realtime"),
            ("multimodal", "bailian-fun-asr-flash"),
            ("qwen-multimodal", "bailian-fun-asr-flash"),
            ("async-transcription", "bailian-fun-asr-flash"),
        ] {
            let raw = format!(r#"{{"bailianProtocol":"{name}"}}"#);
            for (account, value) in [
                (ASR_API_KEY_ACCOUNT, "fixture-key"),
                (ASR_ADVANCED_CONFIG_ACCOUNT, raw.as_str()),
            ] {
                credentials
                    .write(
                        CredentialKey::new(
                            CredentialNamespace::Asr,
                            Some(name.to_string()),
                            account,
                        )
                        .unwrap(),
                        crate::SecretValue::new(value),
                    )
                    .await
                    .unwrap();
            }
            let mut context = DictationContext::default();
            context.asr.provider_id = name.to_string();
            context.asr.provider_type = "bailian".into();
            context.asr.model = Some("unknown-model".into());
            let prepared = prepare_cloud_transcription(&credentials, Arc::new(context))
                .await
                .unwrap();
            assert_eq!(prepared.effective_provider, expected);
            assert_eq!(prepared.model, "unknown-model");
            // A later settings change must not mutate an already prepared recording.
            credentials
                .write(
                    CredentialKey::new(
                        CredentialNamespace::Asr,
                        Some(name.to_string()),
                        ASR_ADVANCED_CONFIG_ACCOUNT,
                    )
                    .unwrap(),
                    crate::SecretValue::new("{}"),
                )
                .await
                .unwrap();
            assert_eq!(
                prepared.bailian_protocol,
                BailianProtocol::from_config("bailian", Some(&raw)).unwrap()
            );
        }
    }
    use super::*;
    use crate::{InMemoryCredentialStore, ProviderInvocation, SecretValue};

    struct ObservedTranslationStream {
        chunks: Mutex<Vec<TextStreamChunk>>,
        arrived: tokio::sync::Semaphore,
    }

    impl ObservedTranslationStream {
        fn new() -> Self {
            Self {
                chunks: Mutex::new(Vec::new()),
                arrived: tokio::sync::Semaphore::new(0),
            }
        }
    }

    impl TextStreamSink for ObservedTranslationStream {
        fn publish(&self, chunk: TextStreamChunk) -> Result<(), BackendError> {
            self.chunks.lock().push(chunk);
            self.arrived.add_permits(1);
            Ok(())
        }
    }

    async fn assert_translation_streams_for_protocol(
        gemini: bool,
        format: crate::llm_protocol::LlmRequestFormat,
        preset: &str,
        cancel: bool,
        fallback: bool,
    ) {
        use crate::llm_protocol::LlmRequestFormat;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn check_request(
            socket: &mut tokio::net::TcpStream,
            gemini: bool,
            format: LlmRequestFormat,
            prefix: &str,
        ) {
            let mut request = Vec::new();
            let mut buf = [0_u8; 4096];
            loop {
                let read = socket.read(&mut buf).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buf[..read]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|length| length.trim().parse().ok())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        let body: serde_json::Value =
                            serde_json::from_slice(&request[end + 4..]).unwrap();
                        if gemini {
                            assert!(headers.contains(":streamGenerateContent?alt=sse"));
                        } else {
                            assert_eq!(body["stream"], true);
                            let suffix = match format {
                                LlmRequestFormat::ChatCompletions => "chat/completions",
                                LlmRequestFormat::Responses => "responses",
                                LlmRequestFormat::Messages => "messages",
                            };
                            assert!(
                                headers.starts_with(&format!("POST {prefix}/{suffix}?tenant=1 "))
                            );
                            let lower = headers.to_ascii_lowercase();
                            if format == LlmRequestFormat::Messages {
                                assert!(lower.contains("x-api-key: fixture-key"));
                                assert!(lower.contains("anthropic-version: 2023-06-01"));
                                assert!(!lower.contains("authorization:"));
                                assert!(body["system"]
                                    .as_str()
                                    .is_some_and(|text| !text.is_empty()));
                            } else {
                                assert!(lower.contains("authorization: bearer fixture-key"));
                            }
                        }
                        break;
                    }
                }
            }
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let prefix = if preset == "opencode" {
            "/zen/go/v1"
        } else {
            "/gateway/v1"
        };
        let endpoint = if gemini {
            format!("http://{}", listener.local_addr().unwrap())
        } else {
            format!(
                "http://{}{prefix}/chat/completions?tenant=1",
                listener.local_addr().unwrap()
            )
        };
        let (source_sent, source_ready) = tokio::sync::oneshot::channel();
        let (advance, next) = tokio::sync::oneshot::channel();
        let (finish, finishing) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            check_request(&mut socket, gemini, format, prefix).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
            let event = |text: &str| {
                let value = if gemini {
                    serde_json::json!({"candidates":[{"content":{"parts":[{"text":text}]}}]})
                } else {
                    match format {
                        LlmRequestFormat::ChatCompletions => {
                            serde_json::json!({"choices":[{"delta":{"content":text}}]})
                        }
                        LlmRequestFormat::Responses => {
                            serde_json::json!({"type":"response.output_text.delta","delta":text})
                        }
                        LlmRequestFormat::Messages => {
                            serde_json::json!({"type":"content_block_delta","delta":{"type":"text_delta","text":text}})
                        }
                    }
                };
                let data = format!("data: {value}\n\n");
                format!("{:X}\r\n{data}\r\n", data.len())
            };
            let end = if gemini {
                ""
            } else {
                match format {
                    LlmRequestFormat::ChatCompletions => "data: [DONE]\n\n",
                    LlmRequestFormat::Responses => "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
                    LlmRequestFormat::Messages => "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n",
                }
            };
            let terminal = if end.is_empty() {
                String::new()
            } else {
                format!("{:X}\r\n{end}\r\n", end.len())
            };
            if !gemini {
                let hidden = "[[OPENLESS_TRANSLATION]]do not expose reasoning";
                let value = match format {
                    LlmRequestFormat::ChatCompletions => {
                        serde_json::json!({"choices":[{"delta":{"reasoning_content":hidden}}]})
                    }
                    LlmRequestFormat::Responses => {
                        serde_json::json!({"type":"response.reasoning_summary_text.delta","delta":hidden})
                    }
                    LlmRequestFormat::Messages => {
                        serde_json::json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":hidden}})
                    }
                };
                let frame = format!("data: {value}\n\n");
                socket
                    .write_all(format!("{:X}\r\n{frame}\r\n", frame.len()).as_bytes())
                    .await
                    .unwrap();
            }
            if fallback {
                socket
                    .write_all(event("malformed combined output").as_bytes())
                    .await
                    .unwrap();
                socket.write_all(terminal.as_bytes()).await.unwrap();
                socket.write_all(b"0\r\n\r\n").await.unwrap();
                socket.shutdown().await.unwrap();
                (socket, _) = listener.accept().await.unwrap();
                check_request(&mut socket, gemini, format, prefix).await;
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
            } else {
                socket
                    .write_all(
                        event("[[OPENLESS_POLISHED_SOURCE]]\n整理后的源文。\n[[OPENLESS_TRANS")
                            .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            socket.flush().await.unwrap();
            source_sent.send(()).unwrap();
            if next.await.is_err() {
                return;
            }
            socket
                .write_all(event(if fallback { "Hello" } else { "LATION]]\nHello" }).as_bytes())
                .await
                .unwrap();
            socket.flush().await.unwrap();
            if finishing.await.is_err() {
                return;
            }
            let _ = socket.write_all(event(" 🌍\n").as_bytes()).await;
            let _ = socket.write_all(terminal.as_bytes()).await;
            let _ = socket.write_all(b"0\r\n\r\n").await;
        });

        let credentials = Arc::new(InMemoryCredentialStore::default());
        write_channel_secret(
            credentials.as_ref(),
            CredentialNamespace::Llm,
            "translation",
            LLM_ENDPOINT_ACCOUNT,
            &endpoint,
        )
        .await;
        write_channel_secret(
            credentials.as_ref(),
            CredentialNamespace::Llm,
            "translation",
            LLM_API_KEY_ACCOUNT,
            "fixture-key",
        )
        .await;
        if !gemini {
            let request_format = match format {
                LlmRequestFormat::ChatCompletions => "chat_completions",
                LlmRequestFormat::Responses => "responses",
                LlmRequestFormat::Messages => "messages",
            };
            write_channel_secret(
                credentials.as_ref(),
                CredentialNamespace::Llm,
                "translation",
                crate::llm_protocol::REQUEST_FORMAT_ACCOUNT,
                request_format,
            )
            .await;
        }
        let polisher = Arc::new(SharedCloudTextPolisher::new(credentials));
        let mut context = DictationContext {
            llm: ProviderInvocation::new("translation", if gemini { "gemini" } else { preset }),
            ..DictationContext::default()
        };
        context.llm.model = Some("fixture-model".into());
        context.polish.translation_active = true;
        context.polish.translation_target_language = "English".into();
        let sink = Arc::new(ObservedTranslationStream::new());
        let session_id = SessionId::new();
        crate::testing::logging::start();
        let run = polisher.polish(
            session_id,
            Arc::new(context),
            "原始语音".into(),
            sink.clone(),
        );
        let run = tokio::spawn(run);
        tokio::time::timeout(Duration::from_secs(3), source_ready)
            .await
            .unwrap()
            .unwrap();
        assert!(
            sink.chunks.lock().is_empty(),
            "source and partial markers must stay hidden"
        );
        assert!(!run.is_finished());
        advance.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), sink.arrived.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        assert_eq!(
            *sink.chunks.lock(),
            [TextStreamChunk {
                text: "Hello".into(),
                offset: 0
            }]
        );
        assert!(
            !run.is_finished(),
            "the first translation delta must arrive before completion"
        );
        if cancel {
            polisher.cancel(session_id).await.unwrap();
            let error = tokio::time::timeout(Duration::from_secs(3), run)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(error.code, BackendErrorCode::Cancelled);
            let _ = finish.send(());
            server.await.unwrap();
            assert_eq!(
                sink.chunks.lock().len(),
                1,
                "cancellation must suppress later translation deltas"
            );
        } else {
            finish.send(()).unwrap();
            let output = tokio::time::timeout(Duration::from_secs(3), run)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            server.await.unwrap();
            assert_eq!(output.text, "Hello 🌍");
            assert_eq!(
                output.source_text.as_deref(),
                if fallback {
                    None
                } else {
                    Some("整理后的源文。")
                }
            );
            assert_eq!(
                *sink.chunks.lock(),
                [
                    TextStreamChunk {
                        text: "Hello".into(),
                        offset: 0
                    },
                    TextStreamChunk {
                        text: " 🌍".into(),
                        offset: 5
                    },
                ]
            );
        }
        if !gemini {
            let logs = crate::testing::logging::matching(&format!("session_id={session_id} "));
            let requests: Vec<_> = logs
                .iter()
                .filter(|line| line.contains("request_id="))
                .collect();
            assert_eq!(requests.len(), if fallback { 2 } else { 1 });
            let request_ids: std::collections::HashSet<_> = requests
                .iter()
                .map(|line| {
                    line.split_whitespace()
                        .find(|field| field.starts_with("request_id="))
                        .unwrap()
                })
                .collect();
            assert_eq!(request_ids.len(), requests.len());
            if !cancel {
                for request_id in request_ids {
                    let records = crate::testing::logging::matching(request_id);
                    assert!(records
                        .iter()
                        .any(|line| line.contains("[llm] stream done")));
                    assert!(!records
                        .iter()
                        .any(|line| line.contains("原始语音") || line.contains("Hello 🌍")));
                }
            }
        }
    }

    async fn assert_translation_streams_before_completion(
        gemini: bool,
        cancel: bool,
        fallback: bool,
    ) {
        assert_translation_streams_for_protocol(
            gemini,
            crate::llm_protocol::LlmRequestFormat::ChatCompletions,
            "openai-compatible",
            cancel,
            fallback,
        )
        .await;
    }

    #[tokio::test]
    async fn translation_streams_safely_across_channel_protocols_and_zen() {
        for format in crate::llm_protocol::LlmRequestFormat::ALL {
            for preset in ["custom", "opencode"] {
                for fallback in [false, true] {
                    assert_translation_streams_for_protocol(false, format, preset, false, fallback)
                        .await;
                }
            }
            assert_translation_streams_for_protocol(false, format, "custom", true, false).await;
        }
    }

    #[tokio::test]
    async fn openai_translation_streams_target_before_completion() {
        assert_translation_streams_before_completion(false, false, false).await;
    }

    #[tokio::test]
    async fn gemini_translation_streams_target_before_completion() {
        assert_translation_streams_before_completion(true, false, false).await;
    }

    #[tokio::test]
    async fn cancelling_translation_stops_the_provider_before_completion() {
        assert_translation_streams_before_completion(false, true, false).await;
    }

    #[tokio::test]
    async fn translation_without_markers_streams_the_plain_translation_fallback() {
        assert_translation_streams_before_completion(false, false, true).await;
    }

    #[test]
    fn tencent_cloud_errors_keep_retryability_and_stable_public_messages() {
        use crate::asr::tencent_cloud::TencentCloudASRError;

        for error in [
            TencentCloudASRError::ConnectionFailed,
            TencentCloudASRError::RateLimited,
            TencentCloudASRError::NoFinalResult,
            TencentCloudASRError::FinalResultTimeout,
            TencentCloudASRError::ServiceUnavailable(5000),
        ] {
            let error = map_tencent_asr_error(error);
            assert_eq!(error.code, BackendErrorCode::Provider);
            assert!(error.retryable, "{error:?}");
            assert!(error.details.is_none());
        }
        for error in [
            TencentCloudASRError::CredentialsMissing,
            TencentCloudASRError::AuthRejected(4002),
            TencentCloudASRError::AccountUnavailable(4004),
            TencentCloudASRError::TaskFailed(4001),
        ] {
            let error = map_tencent_asr_error(error);
            assert_eq!(error.code, BackendErrorCode::Provider);
            assert!(!error.retryable, "{error:?}");
            assert!(error.details.is_none());
        }
    }

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
    async fn ark_polisher_builder_requires_keys_only_for_official_endpoints() {
        for endpoint in [
            "https://ark.cn-beijing.volces.com/api/v3",
            "https://ark.cn-beijing.volces.com/api/plan/v3",
            "https://ark.cn-beijing.volces.com/api/coding/v3",
            "http://127.0.0.1:8080/v1",
        ] {
            // Key rejection is independent of protocol; exercise each key state once,
            // then retain successful construction coverage for all three protocols.
            for (format, key) in [
                ("chat_completions", None),
                ("chat_completions", Some("")),
                ("chat_completions", Some(" \t\n")),
                ("chat_completions", Some("fixture-key")),
                ("responses", Some("fixture-key")),
                ("messages", Some("fixture-key")),
            ] {
                let store = InMemoryCredentialStore::default();
                write_channel_secret(
                    &store,
                    CredentialNamespace::Llm,
                    "ark-channel",
                    LLM_ENDPOINT_ACCOUNT,
                    endpoint,
                )
                .await;
                write_channel_secret(
                    &store,
                    CredentialNamespace::Llm,
                    "ark-channel",
                    crate::llm_protocol::REQUEST_FORMAT_ACCOUNT,
                    format,
                )
                .await;
                if let Some(key) = key {
                    write_channel_secret(
                        &store,
                        CredentialNamespace::Llm,
                        "ark-channel",
                        LLM_API_KEY_ACCOUNT,
                        key,
                    )
                    .await;
                }
                let mut llm = ProviderInvocation::new("ark-channel", "ark");
                llm.model = Some("fixture-model".to_string());
                let context = DictationContext {
                    llm,
                    ..DictationContext::default()
                };
                let result = build_cloud_polisher_provider(&store, &context).await;
                if !endpoint.starts_with("http://127.0.0.1")
                    && key.is_none_or(|value| value.trim().is_empty())
                {
                    let error = match result {
                        Err(error) => error,
                        Ok(_) => {
                            panic!("official endpoint must require an API key: {endpoint}")
                        }
                    };
                    assert_eq!(error.code, BackendErrorCode::Provider);
                    assert_eq!(error.message, "LLM API key is not configured");
                } else {
                    assert!(result.is_ok(), "{endpoint}");
                }
            }
        }
    }

    #[tokio::test]
    async fn lmstudio_generation_still_requires_a_model() {
        let context = DictationContext {
            llm: ProviderInvocation::new("lmstudio-channel", "lmstudio"),
            ..DictationContext::default()
        };
        match build_cloud_polisher_provider(&InMemoryCredentialStore::default(), &context).await {
            Err(error) => assert_eq!(error.message, "LLM model is not configured"),
            Ok(_) => panic!("LM Studio must not generate without a selected model"),
        }
    }

    #[tokio::test]
    async fn asr_channel_configured_matches_provider_credential_requirements() {
        for (provider, accounts) in [
            ("whisper", vec![ASR_API_KEY_ACCOUNT]),
            ("bailian", vec![ASR_API_KEY_ACCOUNT]),
            (
                "volcengine",
                vec![VOLCENGINE_APP_KEY_ACCOUNT, VOLCENGINE_ACCESS_KEY_ACCOUNT],
            ),
            ("iflytek", vec![XFYUN_APP_ID_ACCOUNT, XFYUN_API_KEY_ACCOUNT]),
            (
                "tencent-cloud",
                vec![
                    TENCENT_CLOUD_APP_ID_ACCOUNT,
                    TENCENT_CLOUD_SECRET_ID_ACCOUNT,
                    TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
                ],
            ),
        ] {
            let store = InMemoryCredentialStore::default();
            let invocation = ProviderInvocation::new("cloud-channel", provider);
            assert!(
                !asr_channel_configured(&store, &invocation).await.unwrap(),
                "{provider}"
            );
            for account in accounts {
                assert!(!asr_channel_configured(&store, &invocation).await.unwrap());
                write_channel_secret(
                    &store,
                    CredentialNamespace::Asr,
                    "cloud-channel",
                    account,
                    "fixture-key",
                )
                .await;
            }
            assert!(
                asr_channel_configured(&store, &invocation).await.unwrap(),
                "{provider}"
            );
            let context = Arc::new(DictationContext {
                asr: invocation,
                ..Default::default()
            });
            assert!(
                prepare_cloud_transcription(&store, context).await.is_ok(),
                "{provider}"
            );
        }
    }

    #[tokio::test]
    async fn asr_configuration_does_not_hide_credential_read_errors() {
        let invocation = ProviderInvocation::new("channel", "whisper");
        let error = asr_channel_configured(&crate::UnsupportedCredentialStore, &invocation)
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Unsupported);
    }

    #[tokio::test]
    async fn llm_channel_configured_matches_polisher_requirements() {
        let store = InMemoryCredentialStore::default();
        // Keyed provider with defaults: unconfigured until an API key is saved (issue #1032).
        assert!(!llm_channel_configured(&store, "ark", "ark", None).await);
        write_channel_secret(
            &store,
            CredentialNamespace::Llm,
            "ark",
            LLM_API_KEY_ACCOUNT,
            "fixture-key",
        )
        .await;
        assert!(llm_channel_configured(&store, "ark", "ark", None).await);

        // Keyless local provider: configured once a model is selected, matching the builder.
        assert!(
            !llm_channel_configured(&store, "lmstudio-channel", "lmstudio", None).await,
            "LM Studio without a model is unconfigured"
        );
        assert!(
            llm_channel_configured(&store, "lmstudio-channel", "lmstudio", Some("local-model"))
                .await
        );

        // Empty identifiers are never configured.
        assert!(!llm_channel_configured(&store, "", "ark", None).await);
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
    async fn cloud_asr_preparation_freezes_channel_credentials_before_network_start() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let header_end;
            loop {
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0, "client closed before sending its request");
                request.extend_from_slice(&chunk[..count]);
                if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    header_end = end + 4;
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0, "client closed before sending its body");
                request.extend_from_slice(&chunk[..count]);
            }
            let body = br#"{"choices":[{"message":{"content":"frozen"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
            request
        });

        let store = Arc::new(InMemoryCredentialStore::default());
        write_channel_secret(
            store.as_ref(),
            CredentialNamespace::Asr,
            "frozen-channel",
            ASR_API_KEY_ACCOUNT,
            "frozen-key",
        )
        .await;
        write_channel_secret(
            store.as_ref(),
            CredentialNamespace::Asr,
            "frozen-channel",
            ASR_ENDPOINT_ACCOUNT,
            &endpoint,
        )
        .await;
        let engine = Arc::new(SharedCloudTranscriptionEngine::new(store.clone()));
        let context = Arc::new(DictationContext {
            asr: ProviderInvocation::new("frozen-channel", "xiaomi-mimo-asr"),
            ..DictationContext::default()
        });
        let prepared = Arc::clone(&engine)
            .prepare(SessionId::new(), context)
            .await
            .unwrap();
        write_channel_secret(
            store.as_ref(),
            CredentialNamespace::Asr,
            "frozen-channel",
            ASR_API_KEY_ACCOUNT,
            "changed-key",
        )
        .await;

        let session = prepared
            .start(Arc::new(IgnoreTextStreamSink))
            .await
            .unwrap();
        session.consume_pcm_chunk(&[0, 0]);
        assert_eq!(session.finish().await.unwrap().text, "frozen");
        let request = String::from_utf8(server.await.unwrap())
            .unwrap()
            .to_ascii_lowercase();
        assert!(request.contains("authorization: bearer frozen-key"));
        assert!(!request.contains("changed-key"));
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
