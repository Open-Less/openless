//! Framework-independent provider adapters shared by every host.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::StreamExt;

use crate::credentials::SecretValue;
use crate::dictation_context::DictationContext;
use crate::errors::{BackendError, BackendErrorCode};
use crate::ports::{
    AudioConsumer, TextPolisher, TextStreamSink, TranscriptOutput, TranscriptionEngine,
    TranscriptionSession,
};
use crate::types::SessionId;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_PCM_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct OpenAiTranscriptionConfig {
    pub endpoint: url::Url,
    pub model: String,
    pub api_key: Option<SecretValue>,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub timeout: Duration,
    pub max_response_bytes: usize,
    pub max_pcm_bytes: usize,
}

impl OpenAiTranscriptionConfig {
    pub fn new(endpoint: url::Url, model: impl Into<String>, api_key: Option<SecretValue>) -> Self {
        Self {
            endpoint,
            model: model.into(),
            api_key,
            language: None,
            prompt: None,
            timeout: DEFAULT_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_pcm_bytes: DEFAULT_MAX_PCM_BYTES,
        }
    }

    fn validate(&self) -> Result<(), BackendError> {
        validate_http_endpoint(&self.endpoint)?;
        validate_non_blank("transcription model", &self.model)?;
        validate_limits(
            self.timeout,
            self.max_response_bytes,
            Some(self.max_pcm_bytes),
        )
    }
}

impl std::fmt::Debug for OpenAiTranscriptionConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiTranscriptionConfig")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("language", &self.language)
            .field("prompt", &self.prompt)
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("max_pcm_bytes", &self.max_pcm_bytes)
            .finish()
    }
}

pub struct OpenAiBatchTranscriptionEngine {
    client: reqwest::Client,
    config: Arc<OpenAiTranscriptionConfig>,
}

impl OpenAiBatchTranscriptionEngine {
    pub fn new(config: OpenAiTranscriptionConfig) -> Result<Self, BackendError> {
        config.validate()?;
        let client = reqwest::Client::builder()
            .build()
            .map_err(request_build_error)?;
        Ok(Self {
            client,
            config: Arc::new(config),
        })
    }
}

impl TranscriptionEngine for OpenAiBatchTranscriptionEngine {
    fn start(
        &self,
        _session_id: SessionId,
        context: Arc<DictationContext>,
        _partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        let session = OpenAiBatchTranscriptionSession {
            client: self.client.clone(),
            config: Arc::clone(&self.config),
            context,
            pcm: Arc::new(Mutex::new(Vec::new())),
            overflowed: Arc::new(AtomicBool::new(false)),
            finished: AtomicBool::new(false),
            cancellation: Arc::new(RequestCancellation::default()),
        };
        Box::pin(async move { Ok(Arc::new(session) as Arc<dyn TranscriptionSession>) })
    }
}

struct OpenAiBatchTranscriptionSession {
    client: reqwest::Client,
    config: Arc<OpenAiTranscriptionConfig>,
    context: Arc<DictationContext>,
    pcm: Arc<Mutex<Vec<u8>>>,
    overflowed: Arc<AtomicBool>,
    finished: AtomicBool,
    cancellation: Arc<RequestCancellation>,
}

impl AudioConsumer for OpenAiBatchTranscriptionSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if self.cancellation.cancelled.load(Ordering::Acquire)
            || self.finished.load(Ordering::Acquire)
        {
            return;
        }
        let mut buffer = self.pcm.lock().expect("transcription PCM lock poisoned");
        if buffer.len().saturating_add(pcm.len()) > self.config.max_pcm_bytes {
            self.overflowed.store(true, Ordering::Release);
            return;
        }
        buffer.extend_from_slice(pcm);
    }
}

impl TranscriptionSession for OpenAiBatchTranscriptionSession {
    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        let already_finished = self.finished.swap(true, Ordering::AcqRel);
        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        let context = Arc::clone(&self.context);
        let pcm = Arc::clone(&self.pcm);
        let overflowed = Arc::clone(&self.overflowed);
        let cancellation = Arc::clone(&self.cancellation);
        Box::pin(async move {
            if already_finished {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "transcription session has already been finalized",
                ));
            }
            ensure_not_cancelled(&cancellation)?;
            if overflowed.load(Ordering::Acquire) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "recording exceeded the configured in-memory audio limit",
                ));
            }
            let pcm = std::mem::take(&mut *pcm.lock().expect("transcription PCM lock poisoned"));
            if pcm.is_empty() {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "recording contains no audio",
                ));
            }
            let duration_ms = (pcm.len() as u64).saturating_mul(1000)
                / (u64::from(crate::audio::DICTATION_SAMPLE_RATE) * 2);
            let wav = crate::audio::encode_dictation_wav(&pcm)?;
            let file = reqwest::multipart::Part::bytes(wav)
                .file_name("openless-dictation.wav")
                .mime_str("audio/wav")
                .map_err(request_build_error)?;
            let mut form = reqwest::multipart::Form::new()
                .text(
                    "model",
                    context
                        .asr
                        .model
                        .clone()
                        .unwrap_or_else(|| config.model.clone()),
                )
                .part("file", file);
            if let Some(language) = context
                .asr
                .language
                .as_deref()
                .and_then(|value| non_blank(Some(value)))
                .or_else(|| non_blank(config.language.as_deref()))
            {
                form = form.text("language", language.to_string());
            }
            if let Some(prompt) = context
                .asr
                .prompt
                .as_deref()
                .and_then(|value| non_blank(Some(value)))
                .or_else(|| non_blank(config.prompt.as_deref()))
            {
                form = form.text("prompt", prompt.to_string());
            }
            let mut request = client
                .post(config.endpoint.clone())
                .timeout(config.timeout)
                .multipart(form);
            if let Some(api_key) = configured_secret(config.api_key.as_ref()) {
                request = request.bearer_auth(api_key);
            }
            let response = run_cancellable(
                Arc::clone(&cancellation),
                read_response(request.send(), config.max_response_bytes),
            )
            .await?;
            let payload: TranscriptionResponse =
                serde_json::from_slice(&response).map_err(|error| {
                    provider_error(
                        format!("invalid transcription response JSON: {error}"),
                        false,
                    )
                })?;
            let text = payload.text.trim().to_string();
            if text.is_empty() {
                return Err(provider_error(
                    "transcription provider returned empty text".to_string(),
                    false,
                ));
            }
            Ok(TranscriptOutput { text, duration_ms })
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.cancellation.cancel();
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug, serde::Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Clone)]
pub struct OpenAiChatPolisherConfig {
    pub endpoint: url::Url,
    pub model: String,
    pub api_key: Option<SecretValue>,
    pub system_prompt: String,
    pub temperature: f32,
    pub timeout: Duration,
    pub max_response_bytes: usize,
}

impl OpenAiChatPolisherConfig {
    pub fn new(
        endpoint: url::Url,
        model: impl Into<String>,
        api_key: Option<SecretValue>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            endpoint,
            model: model.into(),
            api_key,
            system_prompt: system_prompt.into(),
            temperature: 0.3,
            timeout: DEFAULT_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }

    fn validate(&self) -> Result<(), BackendError> {
        validate_http_endpoint(&self.endpoint)?;
        validate_non_blank("chat model", &self.model)?;
        validate_non_blank("polish system prompt", &self.system_prompt)?;
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "chat temperature must be finite and between 0 and 2",
            ));
        }
        validate_limits(self.timeout, self.max_response_bytes, None)
    }
}

impl std::fmt::Debug for OpenAiChatPolisherConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiChatPolisherConfig")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("system_prompt", &self.system_prompt)
            .field("temperature", &self.temperature)
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

pub struct OpenAiChatPolisher {
    client: reqwest::Client,
    config: Arc<OpenAiChatPolisherConfig>,
    active: Arc<Mutex<HashMap<SessionId, Arc<RequestCancellation>>>>,
}

impl OpenAiChatPolisher {
    pub fn new(config: OpenAiChatPolisherConfig) -> Result<Self, BackendError> {
        config.validate()?;
        Ok(Self {
            client: reqwest::Client::builder()
                .build()
                .map_err(request_build_error)?,
            config: Arc::new(config),
            active: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

async fn send_chat_completion(
    client: &reqwest::Client,
    config: &OpenAiChatPolisherConfig,
    cancellation: Arc<RequestCancellation>,
    payload: serde_json::Value,
) -> Result<String, BackendError> {
    let mut request = client
        .post(config.endpoint.clone())
        .timeout(config.timeout)
        .json(&payload);
    if let Some(api_key) = configured_secret(config.api_key.as_ref()) {
        request = request.bearer_auth(api_key);
    }
    let response = run_cancellable(
        cancellation,
        read_response(request.send(), config.max_response_bytes),
    )
    .await?;
    let payload: ChatCompletionResponse = serde_json::from_slice(&response)
        .map_err(|error| provider_error(format!("invalid chat response JSON: {error}"), false))?;
    let content = payload
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .unwrap_or_default();
    Ok(crate::output_cleaning::clean_polish_output(&content))
}

impl TextPolisher for OpenAiChatPolisher {
    fn polish(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        raw_text: String,
        _partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<crate::ports::PolishOutput, BackendError>> {
        let cancellation = Arc::new(RequestCancellation::default());
        {
            let mut active = self.active.lock().expect("chat cancellation lock poisoned");
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
        let registration = ActiveRequestRegistration {
            session_id,
            cancellation: Arc::clone(&cancellation),
            active: Arc::clone(&self.active),
        };
        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        Box::pin(async move {
            let _registration = registration;
            ensure_not_cancelled(&cancellation)?;
            validate_non_blank("polish input", &raw_text)?;
            let model = context
                .llm
                .model
                .clone()
                .unwrap_or_else(|| config.model.clone());
            let call_label = crate::polish::LlmCallLabel {
                provider: context.llm.provider_id.clone(),
                model: model.clone(),
            };
            let (captured_prompt, user_prompt) = context.effective_polish_prompts(&raw_text);
            let system_prompt = if captured_prompt.trim().is_empty() {
                config.system_prompt.clone()
            } else {
                captured_prompt
            };
            let mut messages = Vec::with_capacity(context.polish.prior_turns.len() * 2 + 2);
            messages.push(serde_json::json!({
                "role": "system",
                "content": system_prompt,
            }));
            for turn in context.polish.prior_turns.iter().rev() {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": crate::prompts::user_prompt(&turn.raw_text),
                }));
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": turn.polished_text,
                }));
            }
            messages.push(serde_json::json!({
                "role": "user",
                "content": user_prompt,
            }));
            let payload = serde_json::json!({
                "model": model,
                "temperature": config.temperature,
                "messages": messages,
            });
            let cleaned =
                send_chat_completion(&client, &config, Arc::clone(&cancellation), payload).await?;
            let mut output = if context.polish.translation_active {
                if let Some((source_text, text)) =
                    crate::prompt_compose::split_polish_translate_output(&cleaned)
                {
                    crate::ports::PolishOutput {
                        text,
                        source_text,
                        llm_call_label: None,
                    }
                } else {
                    log::warn!(
                        "polish-and-translate response missing markers; retrying plain translation"
                    );
                    ensure_not_cancelled(&cancellation)?;
                    let (system_prompt, user_prompt) =
                        crate::prompt_compose::compose_translate_prompts(
                            &raw_text,
                            &context.polish.translation_target_language,
                            &context.polish.working_languages,
                            context.polish.chinese_script_preference,
                            context.polish.front_app.as_deref(),
                        );
                    let fallback_payload = serde_json::json!({
                        "model": model,
                        "temperature": config.temperature,
                        "messages": [
                            { "role": "system", "content": system_prompt },
                            { "role": "user", "content": user_prompt },
                        ],
                    });
                    let text = send_chat_completion(
                        &client,
                        &config,
                        Arc::clone(&cancellation),
                        fallback_payload,
                    )
                    .await?;
                    crate::ports::PolishOutput::text(text)
                }
            } else {
                crate::ports::PolishOutput::text(cleaned)
            };
            if output.text.is_empty() {
                return Err(provider_error(
                    "chat provider returned empty polish text".to_string(),
                    false,
                ));
            }
            output.llm_call_label = Some(call_label);
            Ok(output)
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        if let Some(cancellation) = self
            .active
            .lock()
            .expect("chat cancellation lock poisoned")
            .get(&session_id)
            .cloned()
        {
            cancellation.cancel();
        }
        Box::pin(async { Ok(()) })
    }
}

#[derive(Debug, serde::Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Debug, serde::Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, serde::Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Default)]
struct RequestCancellation {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl RequestCancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
}

struct ActiveRequestRegistration {
    session_id: SessionId,
    cancellation: Arc<RequestCancellation>,
    active: Arc<Mutex<HashMap<SessionId, Arc<RequestCancellation>>>>,
}

impl Drop for ActiveRequestRegistration {
    fn drop(&mut self) {
        let mut active = self.active.lock().expect("chat cancellation lock poisoned");
        if active
            .get(&self.session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cancellation))
        {
            active.remove(&self.session_id);
        }
    }
}

async fn run_cancellable<T>(
    cancellation: Arc<RequestCancellation>,
    request: impl std::future::Future<Output = Result<T, BackendError>>,
) -> Result<T, BackendError> {
    let notified = cancellation.notify.notified();
    tokio::pin!(notified);
    ensure_not_cancelled(&cancellation)?;
    tokio::select! {
        biased;
        _ = &mut notified => Err(cancelled_provider_error()),
        result = request => result,
    }
}

async fn read_response(
    response: impl std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    max_bytes: usize,
) -> Result<Vec<u8>, BackendError> {
    let response = response.await.map_err(reqwest_provider_error)?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(provider_error(
            "provider response exceeded the configured size limit".to_string(),
            false,
        ));
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(reqwest_provider_error)?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(provider_error(
                "provider response exceeded the configured size limit".to_string(),
                false,
            ));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&body)
            .chars()
            .filter(|character| !character.is_control() || character.is_whitespace())
            .take(512)
            .collect::<String>();
        return Err(provider_error(
            format!("provider returned HTTP {status}: {preview}"),
            status.is_server_error() || status.as_u16() == 429,
        ));
    }
    Ok(body)
}

fn ensure_not_cancelled(cancellation: &RequestCancellation) -> Result<(), BackendError> {
    if cancellation.cancelled.load(Ordering::Acquire) {
        Err(cancelled_provider_error())
    } else {
        Ok(())
    }
}

fn validate_http_endpoint(endpoint: &url::Url) -> Result<(), BackendError> {
    if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "provider endpoint must be an absolute HTTP or HTTPS URL",
        ));
    }
    Ok(())
}

fn validate_non_blank(label: &str, value: &str) -> Result<(), BackendError> {
    if value.trim().is_empty() {
        Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            format!("{label} must not be blank"),
        ))
    } else {
        Ok(())
    }
}

fn validate_limits(
    timeout: Duration,
    max_response_bytes: usize,
    max_pcm_bytes: Option<usize>,
) -> Result<(), BackendError> {
    if timeout.is_zero() || max_response_bytes == 0 || max_pcm_bytes.is_some_and(|limit| limit == 0)
    {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "provider timeout and size limits must be greater than zero",
        ));
    }
    Ok(())
}

fn configured_secret(secret: Option<&SecretValue>) -> Option<&str> {
    secret
        .map(SecretValue::expose_secret)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn request_build_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        BackendErrorCode::Internal,
        format!("failed to build provider request: {error}"),
    )
}

fn reqwest_provider_error(error: reqwest::Error) -> BackendError {
    if error.is_timeout() {
        provider_error("provider request timed out".to_string(), true)
    } else {
        provider_error(format!("provider request failed: {error}"), true)
    }
}

fn provider_error(message: String, retryable: bool) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, message).retryable(retryable)
}

fn cancelled_provider_error() -> BackendError {
    BackendError::new(
        BackendErrorCode::Cancelled,
        "provider request was cancelled",
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;
    use crate::ports::{TextStreamChunk, TranscriptOutput};

    struct IgnoreTextStream;

    impl TextStreamSink for IgnoreTextStream {
        fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
            Ok(())
        }
    }

    fn spawn_http_response(
        status: &str,
        body: &'static str,
    ) -> (url::Url, std::sync::mpsc::Receiver<Vec<u8>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let status = status.to_string();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let mut expected_length = None;
            loop {
                let count = socket.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if expected_length.is_none() {
                    if let Some(header_end) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&request[..header_end]);
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                line.split_once(':').and_then(|(name, value)| {
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                            })
                            .unwrap_or(0);
                        expected_length = Some(header_end + 4 + content_length);
                    }
                }
                if expected_length.is_some_and(|length| request.len() >= length) {
                    break;
                }
            }
            request_tx.send(request).unwrap();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).unwrap();
        });
        (
            url::Url::parse(&format!("http://{address}/v1/test")).unwrap(),
            request_rx,
        )
    }

    fn spawn_http_responses(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (url::Url, std::sync::mpsc::Receiver<Vec<Vec<u8>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut requests = Vec::with_capacity(responses.len());
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                let mut expected_length = None;
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    if expected_length.is_none() {
                        if let Some(header_end) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&request[..header_end]);
                            let content_length = headers
                                .lines()
                                .find_map(|line| {
                                    line.split_once(':').and_then(|(name, value)| {
                                        name.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse::<usize>().ok())
                                            .flatten()
                                    })
                                })
                                .unwrap_or(0);
                            expected_length = Some(header_end + 4 + content_length);
                        }
                    }
                    if expected_length.is_some_and(|length| request.len() >= length) {
                        break;
                    }
                }
                requests.push(request);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).unwrap();
            }
            request_tx.send(requests).unwrap();
        });
        (
            url::Url::parse(&format!("http://{address}/v1/test")).unwrap(),
            request_rx,
        )
    }

    fn request_json(request: &[u8]) -> serde_json::Value {
        let header_end = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("HTTP request must contain a header terminator");
        serde_json::from_slice(&request[header_end + 4..]).expect("request body must be JSON")
    }

    #[tokio::test]
    async fn batch_transcription_sends_wav_and_parses_text_without_exposing_key() {
        let (endpoint, request) = spawn_http_response("200 OK", r#"{"text":" fixture raw "}"#);
        let config = OpenAiTranscriptionConfig::new(
            endpoint,
            "fixture-asr",
            Some(SecretValue::new("super-secret-key")),
        );
        assert!(!format!("{config:?}").contains("super-secret-key"));
        let engine = OpenAiBatchTranscriptionEngine::new(config).unwrap();
        let mut context = DictationContext::default();
        context.asr.model = None;
        let session = engine
            .start(
                SessionId::new(),
                Arc::new(context),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap();
        session.consume_pcm_chunk(&[1, 0, 2, 0]);
        let transcript: TranscriptOutput = session.finish().await.unwrap();
        assert_eq!(transcript.text, "fixture raw");
        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        let request = String::from_utf8_lossy(&request);
        assert!(request.contains("fixture-asr"));
        assert!(request.contains("RIFF"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer super-secret-key"));
    }

    #[tokio::test]
    async fn cancelled_batch_session_never_opens_a_network_request() {
        let config = OpenAiTranscriptionConfig::new(
            url::Url::parse("http://127.0.0.1:1/v1/audio/transcriptions").unwrap(),
            "fixture-asr",
            None,
        );
        let engine = OpenAiBatchTranscriptionEngine::new(config).unwrap();
        let session = engine
            .start(
                SessionId::new(),
                Arc::new(DictationContext::default()),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap();
        session.consume_pcm_chunk(&[1, 0]);
        session.cancel().await.unwrap();
        assert_eq!(
            session.finish().await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );
    }

    #[tokio::test]
    async fn chat_polisher_parses_and_cleans_the_shared_completion_contract() {
        let (endpoint, request) = spawn_http_response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"```text\npolished text\n```"}}]}"#,
        );
        let polisher = OpenAiChatPolisher::new(OpenAiChatPolisherConfig::new(
            endpoint,
            "fixture-chat",
            None,
            "Polish the input",
        ))
        .unwrap();
        let result = polisher
            .polish(
                SessionId::new(),
                Arc::new(DictationContext::default()),
                "raw text".to_string(),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap();
        assert_eq!(result.text, "polished text");
        let request =
            String::from_utf8_lossy(&request.recv_timeout(Duration::from_secs(5)).unwrap())
                .to_string();
        assert!(request.contains("fixture-chat"));
        assert!(request.contains("raw text"));
    }

    #[tokio::test]
    async fn duplicate_chat_polish_keeps_the_original_cancellation_route() {
        let polisher = OpenAiChatPolisher::new(OpenAiChatPolisherConfig::new(
            url::Url::parse("http://127.0.0.1:1/v1/chat/completions").unwrap(),
            "fixture-chat",
            None,
            "Polish the input",
        ))
        .unwrap();
        let session_id = SessionId::new();
        let original = polisher.polish(
            session_id,
            Arc::new(DictationContext::default()),
            "original".to_string(),
            Arc::new(IgnoreTextStream),
        );

        let duplicate = polisher
            .polish(
                session_id,
                Arc::new(DictationContext::default()),
                "duplicate".to_string(),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap_err();
        assert_eq!(duplicate.code, BackendErrorCode::Busy);

        polisher.cancel(session_id).await.unwrap();
        assert_eq!(
            original.await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );
    }

    #[tokio::test]
    async fn chat_request_preserves_prompt_history_order_and_secret_boundary() {
        let (endpoint, request) = spawn_http_response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"polished current"}}]}"#,
        );
        let polisher = OpenAiChatPolisher::new(OpenAiChatPolisherConfig::new(
            endpoint,
            "fixture-chat",
            Some(SecretValue::new("body-must-not-contain-this-secret")),
            "fallback system prompt",
        ))
        .unwrap();
        let mut context = DictationContext::default();
        context.polish.style_system_prompt = "STYLE-CONTRACT".to_string();
        context.polish.front_app = Some("Visual Studio Code".to_string());
        context.polish.cursor_context = Some("before <OPENLESS_CURSOR> after".to_string());
        context.polish.prior_turns = vec![
            crate::dictation_context::PolishHistoryTurn {
                raw_text: "newest prior raw".to_string(),
                polished_text: "newest prior answer".to_string(),
            },
            crate::dictation_context::PolishHistoryTurn {
                raw_text: "oldest prior raw".to_string(),
                polished_text: "oldest prior answer".to_string(),
            },
        ];

        let result = polisher
            .polish(
                SessionId::new(),
                Arc::new(context),
                "current raw".to_string(),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap();
        assert_eq!(result.text, "polished current");

        let request = request.recv_timeout(Duration::from_secs(5)).unwrap();
        let body = request_json(&request);
        let body_text = serde_json::to_string(&body).unwrap();
        assert!(!body_text.contains("body-must-not-contain-this-secret"));
        assert_eq!(body["model"], "fixture-chat");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 6);
        assert_eq!(
            messages
                .iter()
                .map(|message| message["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["system", "user", "assistant", "user", "assistant", "user"]
        );
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains("STYLE-CONTRACT"));
        assert!(system.contains("Visual Studio Code"));
        assert!(system.contains("<cursor_context>"));
        assert!(system.contains("不可信用户文本"));
        assert!(messages[1]["content"]
            .as_str()
            .unwrap()
            .contains("oldest prior raw"));
        assert_eq!(messages[2]["content"], "oldest prior answer");
        assert!(messages[3]["content"]
            .as_str()
            .unwrap()
            .contains("newest prior raw"));
        assert_eq!(messages[4]["content"], "newest prior answer");
        let current = messages[5]["content"].as_str().unwrap();
        assert!(current.contains("<raw_transcript>"));
        assert!(current.contains("current raw"));
    }

    #[tokio::test]
    async fn translation_missing_markers_retries_with_plain_translation_prompt() {
        let (endpoint, requests) = spawn_http_responses(vec![
            (
                "200 OK",
                r#"{"choices":[{"message":{"content":"malformed combined output"}}]}"#,
            ),
            (
                "200 OK",
                r#"{"choices":[{"message":{"content":"translated fallback"}}]}"#,
            ),
        ]);
        let polisher = OpenAiChatPolisher::new(OpenAiChatPolisherConfig::new(
            endpoint,
            "fixture-chat",
            None,
            "Polish the input",
        ))
        .unwrap();
        let mut context = DictationContext::default();
        context.polish.translation_active = true;
        context.polish.translation_target_language = "English".to_string();

        let result = polisher
            .polish(
                SessionId::new(),
                Arc::new(context),
                "raw text".to_string(),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap();

        assert_eq!(result.text, "translated fallback");
        assert_eq!(result.source_text, None);
        let requests = requests.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(requests.len(), 2);
        let fallback_request = String::from_utf8_lossy(&requests[1]);
        assert!(
            fallback_request.contains("中译英"),
            "fallback request did not contain the English translation contract: {fallback_request}"
        );
        assert!(
            fallback_request.contains("raw text"),
            "fallback request did not contain source text: {fallback_request}"
        );
    }

    #[tokio::test]
    async fn combined_translation_preserves_polished_source_for_history() {
        let (endpoint, request) = spawn_http_response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"[[OPENLESS_POLISHED_SOURCE]]\nsource polished\n[[OPENLESS_TRANSLATION]]\ntarget translated"}}]}"#,
        );
        let polisher = OpenAiChatPolisher::new(OpenAiChatPolisherConfig::new(
            endpoint,
            "fixture-chat",
            None,
            "Polish the input",
        ))
        .unwrap();
        let mut context = DictationContext::default();
        context.polish.translation_active = true;
        context.polish.translation_target_language = "English".to_string();

        let result = polisher
            .polish(
                SessionId::new(),
                Arc::new(context),
                "raw text".to_string(),
                Arc::new(IgnoreTextStream),
            )
            .await
            .unwrap();

        assert_eq!(result.text, "target translated");
        assert_eq!(result.source_text.as_deref(), Some("source polished"));
        request.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
