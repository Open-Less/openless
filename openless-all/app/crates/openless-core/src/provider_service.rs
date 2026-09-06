//! Core-owned provider management operations.
//!
//! The runtime engines in [`crate::cloud_providers`] already own the actual
//! ASR/LLM/Omni protocols.  This module is the management seam around those
//! engines: it resolves a channel, reads its credentials through the typed
//! [`CredentialStore`] port, validates connectivity, and lists models.  Hosts
//! must not duplicate these rules.

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::cloud_providers::{SharedCloudTextPolisher, SharedCloudTranscriptionEngine};
use crate::credentials::{
    ChannelKind, CredentialKey, CredentialNamespace, CredentialStore, ProviderSlot,
    ASR_API_KEY_ACCOUNT, ASR_ENDPOINT_ACCOUNT, ASR_MODEL_ACCOUNT, LLM_API_KEY_ACCOUNT,
    LLM_ENDPOINT_ACCOUNT, LLM_EXTRA_HEADERS_ACCOUNT, LLM_MODEL_ACCOUNT, OMNI_API_KEY_ACCOUNT,
    OMNI_ENDPOINT_ACCOUNT, OMNI_EXTRA_HEADERS_ACCOUNT, OMNI_MODEL_ACCOUNT,
};
use crate::dictation_context::{DictationContext, ProviderInvocation};
use crate::domains::{
    ProviderApi, ProviderCheckResult, ProviderKind, ProviderModelsResult, ProviderRequest,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{TextPolisher, TextStreamChunk, TextStreamSink, TranscriptionEngine};
use crate::provider_rules::{
    api_key_required, default_asr_endpoint, default_asr_model, default_llm_endpoint,
    default_omni_endpoint, parse_extra_headers, provider_descriptor, validation_probe_for,
    AuthRequirement, ValidationProbe,
};
use crate::provider_transport::{
    ProviderCancellation, ProviderTransport, ProviderTransportError, ProviderTransportRequest,
    ReqwestProviderTransport,
};
use crate::shared_types::PipelineMode;
use crate::types::SessionId;
use crate::{encode_dictation_wav, TaskSpawner};

const MODEL_LIST_MAX_BYTES: usize = 2 * 1024 * 1024;
const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(15);

/// Shared implementation of [`ProviderApi`] for every non-UI host.
#[derive(Clone)]
pub struct ProviderService {
    credentials: Arc<dyn CredentialStore>,
    task_spawner: Arc<dyn TaskSpawner>,
    transport: Arc<dyn ProviderTransport>,
}

impl ProviderService {
    pub fn new(credentials: Arc<dyn CredentialStore>, task_spawner: Arc<dyn TaskSpawner>) -> Self {
        Self::new_with_transport(
            credentials,
            task_spawner,
            Arc::new(ReqwestProviderTransport::new()),
        )
    }

    /// Construct the service with an explicit model-list transport.
    ///
    /// Production hosts should normally use [`Self::new`].  Tests and hosts
    /// with a different networking policy can inject a transport without
    /// changing provider resolution or response parsing semantics.
    pub fn new_with_transport(
        credentials: Arc<dyn CredentialStore>,
        task_spawner: Arc<dyn TaskSpawner>,
        transport: Arc<dyn ProviderTransport>,
    ) -> Self {
        Self {
            credentials,
            task_spawner,
            transport,
        }
    }

    async fn resolve(&self, request: ProviderRequest) -> Result<ResolvedProvider, BackendError> {
        let (namespace, slot, channel_kind) = match request.kind {
            ProviderKind::Asr => (
                CredentialNamespace::Asr,
                ProviderSlot::Asr,
                ChannelKind::Asr,
            ),
            ProviderKind::Llm => (
                CredentialNamespace::Llm,
                ProviderSlot::Llm,
                ChannelKind::Llm,
            ),
            ProviderKind::Omni => (
                CredentialNamespace::Omni,
                ProviderSlot::Omni,
                ChannelKind::Llm,
            ),
        };
        if request.kind == ProviderKind::Omni && request.channel_id.is_some() {
            return Err(invalid_request("omni provider does not support channel id"));
        }

        let channel_is_explicit = request.channel_id.is_some();
        let provider_id = match request.channel_id {
            Some(id) if !id.trim().is_empty() => id,
            Some(_) => return Err(invalid_request("provider channel id must not be blank")),
            None => {
                let id = self.credentials.active_provider(slot).await?;
                if id.trim().is_empty() {
                    return Err(provider_error("provider channel is not configured"));
                }
                id
            }
        };

        let provider_type = if request.kind == ProviderKind::Omni {
            provider_id.clone()
        } else {
            let channels = self.credentials.list_channels(channel_kind).await?;
            let channel = channels
                .into_iter()
                .find(|channel| channel.id == provider_id);
            if channel_is_explicit && channel.is_none() {
                return Err(provider_error("provider channel is not configured"));
            }
            channel
                .map(|channel| channel.provider_type)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| provider_id.clone())
        };
        if provider_type.trim().is_empty() {
            return Err(invalid_request("provider type must not be blank"));
        }

        let (model_account, key_account, endpoint_account, extra_headers_account) =
            match request.kind {
                ProviderKind::Asr => (
                    ASR_MODEL_ACCOUNT,
                    ASR_API_KEY_ACCOUNT,
                    ASR_ENDPOINT_ACCOUNT,
                    None,
                ),
                ProviderKind::Llm => (
                    LLM_MODEL_ACCOUNT,
                    LLM_API_KEY_ACCOUNT,
                    LLM_ENDPOINT_ACCOUNT,
                    Some(LLM_EXTRA_HEADERS_ACCOUNT),
                ),
                ProviderKind::Omni => (
                    OMNI_MODEL_ACCOUNT,
                    OMNI_API_KEY_ACCOUNT,
                    OMNI_ENDPOINT_ACCOUNT,
                    Some(OMNI_EXTRA_HEADERS_ACCOUNT),
                ),
            };
        let model = self.read(namespace, &provider_id, model_account).await?;
        let api_key = self.read(namespace, &provider_id, key_account).await?;
        let endpoint = self.read(namespace, &provider_id, endpoint_account).await?;
        let extra_headers = match extra_headers_account {
            Some(account) => self.read(namespace, &provider_id, account).await?,
            None => None,
        };

        Ok(ResolvedProvider {
            kind: request.kind,
            provider_id,
            provider_type,
            model,
            api_key,
            endpoint,
            extra_headers,
        })
    }

    async fn read(
        &self,
        namespace: CredentialNamespace,
        provider_id: &str,
        account: &str,
    ) -> Result<Option<String>, BackendError> {
        let key = CredentialKey::new(namespace, Some(provider_id.to_string()), account)?;
        self.credentials
            .read(key)
            .await
            .map(|value| value.map(crate::SecretValue::into_exposed))
    }

    async fn validate_inner(
        &self,
        request: ProviderRequest,
        cancellation: ProviderCancellation,
    ) -> Result<ProviderCheckResult, BackendError> {
        if cancellation.is_cancelled() {
            return Err(cancelled_request());
        }
        let resolved = self.resolve(request).await?;
        self.validate_resolved(resolved, cancellation).await?;
        Ok(ProviderCheckResult { ok: true })
    }

    async fn validate_resolved(
        &self,
        resolved: ResolvedProvider,
        cancellation: ProviderCancellation,
    ) -> Result<(), BackendError> {
        if cancellation.is_cancelled() {
            return Err(cancelled_request());
        }
        ensure_supported_kind(&resolved)?;
        validate_configuration(&resolved)?;
        let probe = validation_probe_for(
            resolved.kind,
            &resolved.provider_type,
            resolved.model.as_deref(),
        );
        if probe == ValidationProbe::AsrNonSilent {
            return tokio::select! {
                _ = wait_for_cancellation(cancellation) => Err(cancelled_request()),
                result = validate_dashscope_probe(&resolved) => result,
            };
        }
        let context = Arc::new(resolved.context());
        let session_id = SessionId::new();
        match resolved.kind {
            ProviderKind::Asr => {
                let engine = SharedCloudTranscriptionEngine::with_task_spawner(
                    Arc::clone(&self.credentials),
                    Arc::clone(&self.task_spawner),
                );
                let session = tokio::select! {
                    _ = wait_for_cancellation(cancellation.clone()) => return Err(cancelled_request()),
                    result = engine.start(session_id, context, Arc::new(DiscardTextStream)) => {
                        result.map_err(sanitize_validation_error)?
                    }
                };
                // A 500 ms 16 kHz mono silence probe exercises the same
                // request/handshake path without storing user audio.
                let pcm = vec![0_u8; 16_000];
                let wav = encode_dictation_wav(&pcm)?;
                session.consume_pcm_chunk(&wav[44..]);
                let finish = tokio::select! {
                    _ = wait_for_cancellation(cancellation) => {
                        let _ = session.cancel().await;
                        return Err(cancelled_request());
                    }
                    result = session.finish() => result.map(|_| ()),
                };
                if let Err(error) = finish {
                    let accepted = (probe == ValidationProbe::StepfunNoSpeech
                        && stepfun_no_speech_is_valid(&error))
                        || (probe == ValidationProbe::AsrSilenceAllowsNoFinal
                            && provider_no_final_is_valid(&error));
                    if !accepted {
                        return Err(sanitize_validation_error(error));
                    }
                }
            }
            ProviderKind::Llm => {
                let polisher = SharedCloudTextPolisher::new(Arc::clone(&self.credentials));
                let polish = polisher.polish(
                    session_id,
                    context,
                    "验证连接".to_string(),
                    Arc::new(DiscardTextStream),
                );
                tokio::select! {
                    _ = wait_for_cancellation(cancellation) => {
                        let _ = polisher.cancel(session_id).await;
                        return Err(cancelled_request());
                    }
                    result = polish => result.map_err(sanitize_validation_error)?,
                };
            }
            ProviderKind::Omni => {
                let validation = crate::cloud_providers::validate_shared_omni_provider(
                    Arc::clone(&self.credentials),
                    context,
                );
                tokio::select! {
                    _ = wait_for_cancellation(cancellation) => return Err(cancelled_request()),
                    result = validation => result.map_err(sanitize_validation_error)?,
                };
            }
        }
        Ok(())
    }

    async fn list_models_inner(
        &self,
        request: ProviderRequest,
        cancellation: ProviderCancellation,
    ) -> Result<ProviderModelsResult, BackendError> {
        let resolved = self.resolve(request).await?;
        ensure_supported_kind(&resolved)?;
        if let Some(models) = static_models(&resolved) {
            if cancellation.is_cancelled() {
                return Err(cancelled_request());
            }
            self.validate_resolved(resolved, cancellation).await?;
            return Ok(ProviderModelsResult { models });
        }
        validate_configuration(&resolved)?;
        let models = fetch_models(&resolved, Arc::clone(&self.transport), cancellation).await?;
        Ok(ProviderModelsResult { models })
    }

    /// Cancelable variant used by hosts that expose an explicit in-flight
    /// provider management cancellation action.  The legacy [`ProviderApi`]
    /// method uses a fresh token and remains source-compatible.
    pub fn list_models_with_cancellation(
        &self,
        request: ProviderRequest,
        cancellation: ProviderCancellation,
    ) -> BoxFuture<'static, Result<ProviderModelsResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.list_models_inner(request, cancellation).await })
    }

    pub fn validate_with_cancellation(
        &self,
        request: ProviderRequest,
        cancellation: ProviderCancellation,
    ) -> BoxFuture<'static, Result<ProviderCheckResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move { service.validate_inner(request, cancellation).await })
    }
}

impl ProviderApi for ProviderService {
    fn validate(
        &self,
        request: ProviderRequest,
    ) -> BoxFuture<'static, Result<ProviderCheckResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            service
                .validate_inner(request, ProviderCancellation::new())
                .await
        })
    }

    fn list_models(
        &self,
        request: ProviderRequest,
    ) -> BoxFuture<'static, Result<ProviderModelsResult, BackendError>> {
        let service = self.clone();
        Box::pin(async move {
            service
                .list_models_inner(request, ProviderCancellation::new())
                .await
        })
    }
}

#[derive(Debug, Clone)]
struct ResolvedProvider {
    kind: ProviderKind,
    provider_id: String,
    provider_type: String,
    model: Option<String>,
    api_key: Option<String>,
    endpoint: Option<String>,
    extra_headers: Option<String>,
}

impl ResolvedProvider {
    fn context(&self) -> DictationContext {
        let mut context = DictationContext::default();
        let invocation = ProviderInvocation {
            provider_id: self.provider_id.clone(),
            provider_type: self.provider_type.clone(),
            model: self.model.clone().filter(|value| !value.trim().is_empty()),
            language: None,
            prompt: None,
            runtime: None,
            keep_loaded_secs: None,
        };
        match self.kind {
            ProviderKind::Asr => context.asr = invocation,
            ProviderKind::Llm => context.llm = invocation,
            ProviderKind::Omni => {
                context.pipeline_mode = PipelineMode::Multimodal;
                context.omni = invocation;
            }
        }
        context
    }
}

fn ensure_supported_kind(resolved: &ResolvedProvider) -> Result<(), BackendError> {
    let supported = provider_descriptor(resolved.kind, &resolved.provider_type)
        .is_some_and(|descriptor| descriptor.validation_probe != ValidationProbe::Unsupported);
    if supported {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "provider validation is not available for this native or unknown provider",
        ))
    }
}

fn validate_configuration(resolved: &ResolvedProvider) -> Result<(), BackendError> {
    let descriptor = provider_descriptor(resolved.kind, &resolved.provider_type)
        .ok_or_else(|| provider_error("provider descriptor is not configured"))?;
    let api_key = resolved.api_key.as_deref().unwrap_or_default();
    if api_key_required(
        resolved.kind,
        &resolved.provider_type,
        resolved.endpoint.as_deref(),
    ) && api_key.trim().is_empty()
        && !matches!(
            descriptor.auth_requirement,
            AuthRequirement::Volcengine | AuthRequirement::Xfyun
        )
    {
        let label = match resolved.kind {
            ProviderKind::Asr => "ASR",
            ProviderKind::Llm => "LLM",
            ProviderKind::Omni => "Omni",
        };
        return Err(provider_error(format!("{label} API key is not configured")));
    }
    let model = resolved
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(descriptor.default_model.as_deref());
    if model.is_none()
        && !matches!(
            descriptor.auth_requirement,
            AuthRequirement::None | AuthRequirement::Volcengine | AuthRequirement::Xfyun
        )
    {
        return Err(invalid_request("provider model is not configured"));
    }
    if !matches!(descriptor.auth_requirement, AuthRequirement::OAuth) {
        let endpoint = resolved
            .endpoint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(descriptor.default_endpoint.as_deref());
        if endpoint.is_none()
            && !matches!(
                descriptor.auth_requirement,
                AuthRequirement::None | AuthRequirement::Volcengine | AuthRequirement::Xfyun
            )
        {
            return Err(provider_error("provider endpoint is not configured"));
        }
        if let Some(endpoint) = endpoint {
            validate_provider_endpoint(endpoint, resolved.kind == ProviderKind::Asr)?;
        }
        if let Some(headers) = resolved.extra_headers.as_deref() {
            parse_extra_headers(headers)?;
        }
    }
    Ok(())
}

fn static_models(resolved: &ResolvedProvider) -> Option<Vec<String>> {
    provider_descriptor(resolved.kind, &resolved.provider_type)
        .map(|descriptor| descriptor.static_models)
        .filter(|models| !models.is_empty())
}

fn validate_provider_endpoint(endpoint: &str, allow_websocket: bool) -> Result<(), BackendError> {
    let url =
        url::Url::parse(endpoint).map_err(|_| invalid_request("provider endpoint is invalid"))?;
    if url.host_str().is_none()
        || !matches!(url.scheme(), "http" | "https")
            && !(allow_websocket && matches!(url.scheme(), "ws" | "wss"))
    {
        return Err(invalid_request("provider endpoint is invalid"));
    }
    Ok(())
}

const DASHSCOPE_ASR_VALIDATE_SAMPLE_URL: &str =
    "https://dashscope.oss-cn-beijing.aliyuncs.com/samples/audio/paraformer/hello_world_female2.wav";

async fn validate_dashscope_probe(resolved: &ResolvedProvider) -> Result<(), BackendError> {
    let api_key = resolved
        .api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| provider_error("ASR API key is not configured"))?;
    let model = resolved
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_asr_model(&resolved.provider_type))
        .ok_or_else(|| invalid_request("ASR model is not configured"))?;
    crate::provider_rules::validate_dashscope_multimodal_model(model).map_err(invalid_request)?;
    let protocol = crate::provider_rules::dashscope_batch_protocol_for_model(model)
        .unwrap_or(crate::provider_rules::DashScopeBatchProtocol::Multimodal);
    let stored_endpoint = resolved
        .endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_asr_endpoint(&resolved.provider_type))
        .ok_or_else(|| provider_error("ASR endpoint is not configured"))?;
    let endpoint = if resolved.provider_type == "bailian" {
        let endpoint_protocol = match protocol {
            crate::provider_rules::DashScopeBatchProtocol::Multimodal => {
                crate::provider_rules::BailianEndpointProtocol::Multimodal
            }
            crate::provider_rules::DashScopeBatchProtocol::AsyncTranscription => {
                crate::provider_rules::BailianEndpointProtocol::AsyncTranscription
            }
        };
        crate::provider_rules::derive_bailian_endpoint(stored_endpoint, endpoint_protocol)
            .map_err(invalid_request)?
    } else {
        stored_endpoint.to_string()
    };
    validate_provider_endpoint(&endpoint, false)?;
    let provider = crate::asr::DashScopeMultimodalASR::new(
        api_key.to_string(),
        endpoint.clone(),
        model.to_string(),
    );
    if protocol == crate::provider_rules::DashScopeBatchProtocol::AsyncTranscription {
        return tokio::time::timeout(
            Duration::from_secs(120),
            provider.transcribe_async_url_with_timeout(
                DASHSCOPE_ASR_VALIDATE_SAMPLE_URL,
                Duration::from_secs(60),
            ),
        )
        .await
        .map_err(|_| {
            BackendError::new(BackendErrorCode::Provider, "ASR provider timed out").retryable(true)
        })?
        .map(|_| ())
        .map_err(|error| provider_error(format!("ASR provider failed: {error}")));
    }

    let url = crate::asr::dashscope_multimodal::generation_url(&endpoint)
        .map_err(|_| invalid_request("ASR endpoint is invalid"))?;
    let body = crate::asr::dashscope_multimodal::dashscope_multimodal_body_from_uri(
        model,
        DASHSCOPE_ASR_VALIDATE_SAMPLE_URL,
    );
    let response = crate::net::credential_http()
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("X-DashScope-SSE", "disable")
        .json(&body)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                BackendError::new(BackendErrorCode::Provider, "ASR provider timed out")
                    .retryable(true)
            } else {
                provider_error("ASR provider network request failed")
            }
        })?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(provider_error(format!(
            "providerHttpStatus:{}",
            response.status().as_u16()
        )))
    }
}

fn stepfun_no_speech_is_valid(error: &BackendError) -> bool {
    let message = error.message.to_ascii_lowercase();
    error.code == BackendErrorCode::Provider
        && message.contains("400")
        && message.contains("no speech")
}

fn provider_no_final_is_valid(error: &BackendError) -> bool {
    error.code == BackendErrorCode::Provider
        && error
            .message
            .to_ascii_lowercase()
            .contains("no final result")
}

fn sanitize_validation_error(error: BackendError) -> BackendError {
    if error.code != BackendErrorCode::Provider {
        return error;
    }
    let message = error.message.as_str();
    if message.ends_with("is not configured") {
        return error;
    }
    let status = ["status ", "API error ", "HTTP "]
        .iter()
        .find_map(|marker| {
            let tail = message.split_once(marker)?.1;
            let digits = tail
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            digits
                .parse::<u16>()
                .ok()
                .filter(|status| (100..600).contains(status))
        });
    let message = status
        .map(|status| format!("providerHttpStatus:{status}"))
        .unwrap_or_else(|| "provider validation failed".to_string());
    BackendError::new(BackendErrorCode::Provider, message).retryable(error.retryable)
}

async fn fetch_models(
    resolved: &ResolvedProvider,
    transport: Arc<dyn ProviderTransport>,
    cancellation: ProviderCancellation,
) -> Result<Vec<String>, BackendError> {
    let endpoint = resolved
        .endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_llm_endpoint(&resolved.provider_type))
        .or_else(|| default_omni_endpoint(&resolved.provider_type))
        .ok_or_else(|| provider_error("provider endpoint is not configured"))?;
    let url = models_url(endpoint)?;
    let is_gemini =
        crate::net::sanitized_url_for_logs(&url).contains("generativelanguage.googleapis.com");
    let mut request_headers = Vec::new();
    if let Some(api_key) = resolved
        .api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if is_gemini {
            request_headers.push(("x-goog-api-key".to_string(), api_key.to_string()));
        } else {
            request_headers.push(("Authorization".to_string(), format!("Bearer {api_key}")));
        }
    }
    if let Some(extra_headers) = resolved.extra_headers.as_deref() {
        for (name, value) in parse_extra_headers(extra_headers)? {
            request_headers.push((name, value));
        }
    }
    let response = transport
        .execute(
            ProviderTransportRequest {
                url,
                headers: request_headers,
                timeout: MODEL_LIST_TIMEOUT,
                max_response_bytes: MODEL_LIST_MAX_BYTES,
            },
            cancellation,
        )
        .await
        .map_err(map_transport_error)?;
    if !(200..300).contains(&response.status) {
        return Err(BackendError::new(
            BackendErrorCode::Provider,
            format!("providerHttpStatus:{}", response.status),
        ));
    }
    if response.body.len() > MODEL_LIST_MAX_BYTES {
        return Err(provider_error("provider model response is too large"));
    }
    parse_model_list(&response.body, is_gemini)
}

fn parse_model_list(body: &[u8], is_gemini: bool) -> Result<Vec<String>, BackendError> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| provider_error("provider model response is invalid JSON"))?;
    let models = if is_gemini {
        value
            .get("models")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| provider_error("provider model response is missing models"))?
            .iter()
            .filter(|item| {
                item.get("supportedGenerationMethods")
                    .and_then(serde_json::Value::as_array)
                    .map(|methods| {
                        methods
                            .iter()
                            .any(|method| method.as_str() == Some("generateContent"))
                    })
                    .unwrap_or(true)
            })
            .filter_map(|item| item.get("name").and_then(serde_json::Value::as_str))
            .map(|name| {
                name.strip_prefix("models/")
                    .unwrap_or(name)
                    .trim()
                    .to_string()
            })
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
    } else {
        value
            .get("data")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| provider_error("provider model response is missing data"))?
            .iter()
            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let mut models = models;
    models.sort();
    models.dedup();
    Ok(models)
}

fn models_url(endpoint: &str) -> Result<String, BackendError> {
    let mut url = url::Url::parse(endpoint.trim())
        .map_err(|_| invalid_request("provider endpoint is invalid"))?;
    let path = url.path().trim_end_matches('/');
    let next_path = if path.ends_with("/models") {
        path.to_string()
    } else if let Some(prefix) = path.strip_suffix("/chat/completions") {
        format!("{prefix}/models")
    } else {
        format!("{path}/models")
    };
    url.set_path(&next_path);
    Ok(url.to_string())
}

fn map_transport_error(error: ProviderTransportError) -> BackendError {
    match error {
        ProviderTransportError::Timeout => {
            BackendError::new(BackendErrorCode::Provider, "provider request timed out")
                .retryable(true)
        }
        ProviderTransportError::Connection => BackendError::new(
            BackendErrorCode::Provider,
            "provider network connection failed",
        )
        .retryable(true),
        ProviderTransportError::Cancelled => {
            BackendError::new(BackendErrorCode::Cancelled, "provider request cancelled")
        }
        ProviderTransportError::ResponseTooLarge => {
            provider_error("provider model response is too large")
        }
        ProviderTransportError::Request => {
            BackendError::new(BackendErrorCode::Provider, "provider request failed")
        }
    }
}

fn invalid_request(message: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, message)
}

fn provider_error(message: impl Into<String>) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, message)
}

fn cancelled_request() -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, "provider request cancelled")
}

async fn wait_for_cancellation(cancellation: ProviderCancellation) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

struct DiscardTextStream;

impl TextStreamSink for DiscardTextStream {
    fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{
        ChannelMutation, ChannelMutationResult, InMemoryCredentialStore, SecretValue,
    };
    use crate::provider_transport::{ProviderCancellation, ProviderTransportError};
    use crate::testing::FakeProviderTransport;
    use std::io::{Read, Write};

    fn spawn_http_response(
        status: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> (String, std::sync::mpsc::Receiver<Vec<u8>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 8192];
            loop {
                let read = stream.read(&mut buffer).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            request_tx.send(request).unwrap();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        (format!("http://{address}/v1"), request_rx)
    }

    async fn create_channel_with_values(
        credentials: &Arc<InMemoryCredentialStore>,
        kind: ChannelKind,
        provider_type: &str,
        values: &[(&str, &str)],
    ) -> String {
        let result = credentials
            .mutate_channel(ChannelMutation::Create {
                kind,
                provider_type: provider_type.to_string(),
                name: "fixture".to_string(),
            })
            .await
            .unwrap();
        let id = match result {
            ChannelMutationResult::Created(id) => id,
            other => panic!("unexpected mutation result: {other:?}"),
        };
        let namespace = match kind {
            ChannelKind::Asr => CredentialNamespace::Asr,
            ChannelKind::Llm => CredentialNamespace::Llm,
        };
        for (account, value) in values {
            credentials
                .write(
                    CredentialKey::new(namespace, Some(id.clone()), *account).unwrap(),
                    SecretValue::new(*value),
                )
                .await
                .unwrap();
        }
        id
    }

    #[tokio::test]
    async fn openai_compatible_asr_without_key_reaches_the_configured_endpoint() {
        let (endpoint, request) =
            spawn_http_response("200 OK", "application/json", r#"{"text":"ok"}"#);
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let channel = create_channel_with_values(
            &credentials,
            ChannelKind::Asr,
            "openai-compatible",
            &[
                (ASR_ENDPOINT_ACCOUNT, endpoint.as_str()),
                (ASR_MODEL_ACCOUNT, "local-asr"),
            ],
        )
        .await;
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));

        service
            .validate(ProviderRequest {
                kind: ProviderKind::Asr,
                channel_id: Some(channel),
            })
            .await
            .unwrap();

        let request = String::from_utf8_lossy(&request.recv().unwrap()).to_ascii_lowercase();
        assert!(request.starts_with("post /v1/audio/transcriptions "));
        assert!(!request.contains("authorization:"));
    }

    #[tokio::test]
    async fn custom_llm_without_key_reaches_its_explicit_endpoint() {
        let (endpoint, request) = spawn_http_response(
            "200 OK",
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
        );
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let channel = create_channel_with_values(
            &credentials,
            ChannelKind::Llm,
            "custom",
            &[
                (LLM_ENDPOINT_ACCOUNT, endpoint.as_str()),
                (LLM_MODEL_ACCOUNT, "local-llm"),
            ],
        )
        .await;
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));

        service
            .validate(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(channel),
            })
            .await
            .unwrap();

        let request = String::from_utf8_lossy(&request.recv().unwrap()).to_ascii_lowercase();
        assert!(request.starts_with("post /v1/chat/completions "));
        assert!(!request.contains("authorization:"));
    }

    #[tokio::test]
    async fn provider_validation_never_returns_an_untrusted_error_body() {
        let (endpoint, _request) = spawn_http_response(
            "401 Unauthorized",
            "application/json",
            r#"{"error":"response-secret"}"#,
        );
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let channel = create_channel_with_values(
            &credentials,
            ChannelKind::Llm,
            "custom",
            &[
                (LLM_ENDPOINT_ACCOUNT, endpoint.as_str()),
                (LLM_MODEL_ACCOUNT, "local-llm"),
            ],
        )
        .await;
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));

        let error = service
            .validate(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(channel),
            })
            .await
            .unwrap_err();

        assert_eq!(error.message, "providerHttpStatus:401");
        assert!(!format!("{error:?}").contains("response-secret"));
    }

    #[tokio::test]
    async fn static_model_list_runs_the_real_provider_probe_first() {
        let (endpoint, request) =
            spawn_http_response("200 OK", "application/json", r#"{"output":{}}"#);
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let channel = create_channel_with_values(
            &credentials,
            ChannelKind::Asr,
            "bailian-fun-asr-flash",
            &[
                (ASR_API_KEY_ACCOUNT, "fixture-key"),
                (ASR_ENDPOINT_ACCOUNT, endpoint.as_str()),
                (ASR_MODEL_ACCOUNT, "fun-asr-flash-2026-06-15"),
            ],
        )
        .await;
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));

        let result = service
            .list_models(ProviderRequest {
                kind: ProviderKind::Asr,
                channel_id: Some(channel),
            })
            .await
            .unwrap();

        assert_eq!(
            result.models,
            vec!["qwen-audio-3.0-asr-flash", "fun-asr-flash-2026-06-15"]
        );
        let request = request
            .recv_timeout(Duration::from_secs(2))
            .expect("static list must perform a protocol probe");
        let request = String::from_utf8_lossy(&request);
        assert!(request.contains(DASHSCOPE_ASR_VALIDATE_SAMPLE_URL));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer fixture-key"));
    }

    #[tokio::test]
    async fn stepfun_no_speech_400_proves_credentials_and_protocol_are_valid() {
        let (endpoint, _request) = spawn_http_response(
            "400 Bad Request",
            "application/json",
            r#"{"error":{"message":"no speech found","type":"request_params_invalid"}}"#,
        );
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let channel = create_channel_with_values(
            &credentials,
            ChannelKind::Asr,
            "stepfun",
            &[
                (ASR_API_KEY_ACCOUNT, "fixture-key"),
                (ASR_ENDPOINT_ACCOUNT, endpoint.as_str()),
                (ASR_MODEL_ACCOUNT, "stepaudio-2.5-asr"),
            ],
        )
        .await;
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));

        service
            .validate(ProviderRequest {
                kind: ProviderKind::Asr,
                channel_id: Some(channel),
            })
            .await
            .unwrap();
    }

    #[test]
    fn no_speech_probe_does_not_hide_other_bad_requests() {
        let accepted = BackendError::new(
            BackendErrorCode::Provider,
            "ASR provider failed: Whisper API error 400: no speech found",
        );
        let rejected = BackendError::new(
            BackendErrorCode::Provider,
            "ASR provider failed: Whisper API error 400: response_format is invalid",
        );
        assert!(stepfun_no_speech_is_valid(&accepted));
        assert!(!stepfun_no_speech_is_valid(&rejected));

        let no_final = BackendError::new(
            BackendErrorCode::Provider,
            "ASR provider failed: no final result",
        );
        assert!(provider_no_final_is_valid(&no_final));
        assert!(!provider_no_final_is_valid(&rejected));
    }

    async fn service_with_channel() -> (ProviderService, Arc<InMemoryCredentialStore>) {
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let created = credentials
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Llm,
                provider_type: "openai".to_string(),
                name: "test".to_string(),
            })
            .await
            .unwrap();
        let id = match created {
            ChannelMutationResult::Created(id) => id,
            other => panic!("unexpected mutation result: {other:?}"),
        };
        credentials
            .set_active_provider(ProviderSlot::Llm, id.clone())
            .await
            .unwrap();
        credentials
            .write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some(id.clone()),
                    LLM_API_KEY_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("test-key"),
            )
            .await
            .unwrap();
        let credential_store: Arc<dyn CredentialStore> = credentials.clone();
        let service = ProviderService::new(credential_store, Arc::new(crate::TokioTaskSpawner));
        (service, credentials)
    }

    #[tokio::test]
    async fn channel_resolution_does_not_cross_channels() {
        let (service, credentials) = service_with_channel().await;
        let error = service
            .list_models(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some("missing".to_string()),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert!(!format!("{error:?}").contains("test-key"));
        let _ = credentials;
    }

    #[tokio::test]
    async fn omni_channel_is_rejected_before_credential_access() {
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));
        let error = service
            .validate(ProviderRequest {
                kind: ProviderKind::Omni,
                channel_id: Some("channel".to_string()),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::InvalidArgument);
    }

    #[tokio::test]
    async fn channel_credentials_are_scoped_and_active_resolution_is_explicit() {
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let first = credentials
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Llm,
                provider_type: "openai".to_string(),
                name: "first".to_string(),
            })
            .await
            .unwrap();
        let second = credentials
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Llm,
                provider_type: "gemini".to_string(),
                name: "second".to_string(),
            })
            .await
            .unwrap();
        let first_id = match first {
            ChannelMutationResult::Created(id) => id,
            _ => panic!("first channel was not created"),
        };
        let second_id = match second {
            ChannelMutationResult::Created(id) => id,
            _ => panic!("second channel was not created"),
        };
        for (id, key) in [(&first_id, "first-secret"), (&second_id, "second-secret")] {
            credentials
                .write(
                    CredentialKey::new(
                        CredentialNamespace::Llm,
                        Some(id.clone()),
                        LLM_API_KEY_ACCOUNT,
                    )
                    .unwrap(),
                    SecretValue::new(key),
                )
                .await
                .unwrap();
        }
        credentials
            .set_active_provider(ProviderSlot::Llm, first_id.clone())
            .await
            .unwrap();
        let credential_store: Arc<dyn CredentialStore> = credentials.clone();
        let service = ProviderService::new(credential_store, Arc::new(crate::TokioTaskSpawner));

        let first_resolved = service
            .resolve(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(first_id.clone()),
            })
            .await
            .unwrap();
        let second_resolved = service
            .resolve(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(second_id.clone()),
            })
            .await
            .unwrap();
        let active_resolved = service
            .resolve(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: None,
            })
            .await
            .unwrap();

        assert_eq!(first_resolved.provider_type, "openai");
        assert_eq!(second_resolved.provider_type, "gemini");
        assert_eq!(first_resolved.api_key.as_deref(), Some("first-secret"));
        assert_eq!(second_resolved.api_key.as_deref(), Some("second-secret"));
        assert_eq!(active_resolved.provider_id, first_id);
    }

    #[test]
    fn model_url_preserves_query_and_changes_only_path() {
        let url = models_url("https://example.com/v1/chat/completions?token=query-secret#fragment")
            .unwrap();
        assert_eq!(
            url,
            "https://example.com/v1/models?token=query-secret#fragment"
        );
    }

    #[test]
    fn openai_model_response_is_sorted_deduplicated_and_redacted() {
        let models = parse_model_list(
            br#"{"data":[{"id":"gpt-z"},{"id":""},{"id":"gpt-a"},{"id":"gpt-z"}]}"#,
            false,
        )
        .unwrap();
        assert_eq!(models, vec!["gpt-a", "gpt-z"]);
    }

    #[test]
    fn gemini_model_response_filters_unsupported_methods() {
        let models = parse_model_list(
            br#"{"models":[{"name":"models/gemini-z","supportedGenerationMethods":["generateContent"]},{"name":"models/embedding","supportedGenerationMethods":["embedContent"]},{"name":"gemini-a"}]}"#,
            true,
        )
        .unwrap();
        assert_eq!(models, vec!["gemini-a", "gemini-z"]);
    }

    #[test]
    fn invalid_model_response_is_a_provider_error_without_body() {
        let error = parse_model_list(br#"{"error":"secret-key"}"#, false).unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert!(!format!("{error:?}").contains("secret-key"));
    }

    async fn service_with_fake_transport() -> (ProviderService, Arc<FakeProviderTransport>, String)
    {
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let created = credentials
            .mutate_channel(ChannelMutation::Create {
                kind: ChannelKind::Llm,
                provider_type: "openai".to_string(),
                name: "transport fixture".to_string(),
            })
            .await
            .unwrap();
        let id = match created {
            ChannelMutationResult::Created(id) => id,
            other => panic!("unexpected mutation result: {other:?}"),
        };
        credentials
            .set_active_provider(ProviderSlot::Llm, id.clone())
            .await
            .unwrap();
        for (account, value) in [
            (LLM_API_KEY_ACCOUNT, "provider-secret"),
            (
                LLM_ENDPOINT_ACCOUNT,
                "https://example.test/v1?token=url-secret",
            ),
            (LLM_EXTRA_HEADERS_ACCOUNT, r#"{"x-tenant":"header-secret"}"#),
        ] {
            credentials
                .write(
                    CredentialKey::new(CredentialNamespace::Llm, Some(id.clone()), account)
                        .unwrap(),
                    SecretValue::new(value),
                )
                .await
                .unwrap();
        }
        let transport = Arc::new(FakeProviderTransport::default());
        let credential_store: Arc<dyn CredentialStore> = credentials;
        let service = ProviderService::new_with_transport(
            credential_store,
            Arc::new(crate::TokioTaskSpawner),
            transport.clone(),
        );
        (service, transport, id)
    }

    #[tokio::test]
    async fn fake_transport_parses_models_and_redacts_request_debug() {
        let (service, transport, channel) = service_with_fake_transport().await;
        transport.push_response(
            200,
            br#"{"data":[{"id":"gpt-z"},{"id":"gpt-a"},{"id":"gpt-z"}]}"#,
        );

        let result = service
            .list_models(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(channel),
            })
            .await
            .unwrap();
        assert_eq!(result.models, vec!["gpt-a", "gpt-z"]);

        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Bearer provider-secret"));
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "x-tenant" && value == "header-secret"));
        let debug = format!("{request:?}");
        for secret in ["provider-secret", "header-secret", "url-secret"] {
            assert!(!debug.contains(secret), "transport debug leaked {secret}");
        }
        assert_eq!(
            request.url,
            "https://example.test/v1/models?token=url-secret"
        );
    }

    #[tokio::test]
    async fn fake_transport_maps_status_timeout_cancel_size_and_invalid_json() {
        let (service, transport, channel) = service_with_fake_transport().await;
        for (status, expected) in [
            (401, "providerHttpStatus:401"),
            (403, "providerHttpStatus:403"),
            (429, "providerHttpStatus:429"),
            (500, "providerHttpStatus:500"),
            (302, "providerHttpStatus:302"),
        ] {
            transport.push_response(status, br#"{"data":[]}"#);
            let error = service
                .list_models(ProviderRequest {
                    kind: ProviderKind::Llm,
                    channel_id: Some(channel.clone()),
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, BackendErrorCode::Provider);
            assert_eq!(error.message, expected);
            assert!(!error.retryable);
        }

        transport.push_response(200, br#"not-json secret-body"#);
        let error = service
            .list_models(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(channel.clone()),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert!(!format!("{error:?}").contains("secret-body"));

        transport.push_response(200, vec![b'x'; MODEL_LIST_MAX_BYTES + 1]);
        let error = service
            .list_models(ProviderRequest {
                kind: ProviderKind::Llm,
                channel_id: Some(channel.clone()),
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert!(error.message.contains("too large"));

        for (transport_error, code, retryable) in [
            (
                ProviderTransportError::Timeout,
                BackendErrorCode::Provider,
                true,
            ),
            (
                ProviderTransportError::Connection,
                BackendErrorCode::Provider,
                true,
            ),
            (
                ProviderTransportError::Request,
                BackendErrorCode::Provider,
                false,
            ),
            (
                ProviderTransportError::ResponseTooLarge,
                BackendErrorCode::Provider,
                false,
            ),
            (
                ProviderTransportError::Cancelled,
                BackendErrorCode::Cancelled,
                false,
            ),
        ] {
            transport.push_error(transport_error);
            let error = service
                .list_models(ProviderRequest {
                    kind: ProviderKind::Llm,
                    channel_id: Some(channel.clone()),
                })
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, retryable);
        }
        assert_eq!(transport.requests().len(), 12);
    }

    #[tokio::test]
    async fn cancellation_token_stops_fake_transport_before_dispatch() {
        let (service, transport, channel) = service_with_fake_transport().await;
        transport.push_response(200, br#"{"data":[{"id":"never-used"}]}"#);
        let cancellation = ProviderCancellation::new();
        cancellation.cancel();
        let error = service
            .list_models_with_cancellation(
                ProviderRequest {
                    kind: ProviderKind::Llm,
                    channel_id: Some(channel),
                },
                cancellation,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert_eq!(transport.requests().len(), 1);
    }

    #[tokio::test]
    async fn cancellation_stops_static_validation_before_network_dispatch() {
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let channel = create_channel_with_values(
            &credentials,
            ChannelKind::Asr,
            "bailian-fun-asr-flash",
            &[
                (ASR_API_KEY_ACCOUNT, "fixture-key"),
                (ASR_ENDPOINT_ACCOUNT, "http://127.0.0.1:9/v1"),
                (ASR_MODEL_ACCOUNT, "fun-asr-flash-2026-06-15"),
            ],
        )
        .await;
        let service = ProviderService::new(credentials, Arc::new(crate::TokioTaskSpawner));
        let cancellation = ProviderCancellation::new();
        cancellation.cancel();

        let error = service
            .list_models_with_cancellation(
                ProviderRequest {
                    kind: ProviderKind::Asr,
                    channel_id: Some(channel),
                },
                cancellation,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
    }

    #[test]
    fn static_model_lists_match_legacy_provider_order_without_duplicates() {
        let expected = [
            (
                "bailian",
                vec![
                    "fun-asr-realtime",
                    "fun-asr-flash-8k-realtime",
                    "qwen3-asr-flash-realtime",
                    "qwen3-asr-flash-realtime-2026-02-10",
                    "qwen3-asr-flash-realtime-2025-10-27",
                    "qwen-audio-3.0-asr-flash",
                    "fun-asr-flash-2026-06-15",
                    "qwen3-asr-flash",
                    "fun-asr",
                    "fun-asr-2025-11-07",
                    "fun-asr-2025-08-25",
                    "fun-asr-mtl",
                    "fun-asr-mtl-2025-08-25",
                    "paraformer-v2",
                ],
            ),
            (
                "bailian-qwen3-realtime",
                vec![
                    "qwen3-asr-flash-realtime",
                    "qwen3-asr-flash-realtime-2026-02-10",
                    "qwen3-asr-flash-realtime-2025-10-27",
                ],
            ),
            ("xiaomi-mimo-asr", vec!["mimo-v2.5-asr"]),
            (
                "bailian-fun-asr-flash",
                vec!["qwen-audio-3.0-asr-flash", "fun-asr-flash-2026-06-15"],
            ),
            ("elevenlabs", vec!["scribe_v2"]),
        ];
        for (provider_type, expected_models) in expected {
            let resolved = ResolvedProvider {
                kind: ProviderKind::Asr,
                provider_id: provider_type.to_string(),
                provider_type: provider_type.to_string(),
                model: None,
                api_key: None,
                endpoint: None,
                extra_headers: None,
            };
            let actual = static_models(&resolved).expect("provider should have static models");
            assert_eq!(actual, expected_models);
            let unique = actual.iter().collect::<std::collections::HashSet<_>>();
            assert_eq!(unique.len(), actual.len());
        }
    }
}
