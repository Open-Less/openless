use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;
use crate::cloud_providers::{build_cloud_channel_provider, CloudPolisherProvider};
use crate::llm_gemini::GeminiProvider;
use crate::llm_protocol::{LlmRequestFormat, StreamEvent, TextEventStream};
use crate::polish::{ActiveLLMProvider, LLMError, OpenAICompatibleLLMProvider};
use crate::{CredentialStore, PreferencesStore, ProviderSlot};

mod tools;

const OUTPUT_BUDGET: usize = 8_192;
const UNKNOWN_OUTPUT_BUDGET: usize = 2_048;
const MAX_SSE_BYTES: usize = 16 * 1024 * 1024;
const STREAM_WAIT: Duration = Duration::from_secs(90);
const STREAM_TOTAL: Duration = Duration::from_secs(300);

pub struct ConfiguredAskModels {
    credentials: Arc<dyn CredentialStore>,
    preferences: Arc<PreferencesStore>,
}

impl ConfiguredAskModels {
    pub fn new(credentials: Arc<dyn CredentialStore>, preferences: Arc<PreferencesStore>) -> Self {
        Self {
            credentials,
            preferences,
        }
    }
}

impl AskModelFactory for ConfiguredAskModels {
    fn prepare(
        &self,
        _deep_thinking: bool,
    ) -> BoxFuture<'_, Result<Arc<dyn AskModel>, BackendError>> {
        Box::pin(async move {
            let preferences = self.preferences.get();
            let suite = crate::shared_types::effective_pipeline_mode(
                preferences.multimodal_pipeline_enabled,
                preferences.pipeline_mode,
            ) == crate::shared_types::PipelineMode::Deepseek;
            let route = if suite {
                crate::deepseek_suite::invocation(false)
            } else {
                crate::provider_resolution::resolve_session_provider(
                    &self.credentials,
                    ProviderSlot::Llm,
                    &preferences.active_llm_provider,
                )
                .await
                .map_err(|error| {
                    if error.code == BackendErrorCode::InvalidState {
                        ask_error(BackendErrorCode::Provider, "askModelNotConfigured")
                    } else {
                        error
                    }
                })?
            };
            if matches!(
                route.provider_type.as_str(),
                crate::polish::CODEX_OAUTH_PROVIDER_ID | crate::siwc::PROVIDER_ID
            ) {
                return Err(ask_error(
                    BackendErrorCode::Unsupported,
                    "askModelProtocolUnsupported",
                ));
            }
            let provider =
                build_cloud_channel_provider(self.credentials.as_ref(), &route, suite, false)
                    .await
                    .map_err(|error| {
                        if error.code == BackendErrorCode::Provider {
                            ask_error(BackendErrorCode::Provider, "askModelNotConfigured")
                        } else {
                            error
                        }
                    })?;
            let provider = match provider {
                CloudPolisherProvider::OpenAi(ActiveLLMProvider::OpenAI(provider)) => {
                    let config = provider.ask_config();
                    config.protocol.validate_headers(&config.extra_headers)?;
                    Wire::Compatible(provider)
                }
                CloudPolisherProvider::Gemini(provider) => Wire::Gemini(provider),
                CloudPolisherProvider::OpenAi(ActiveLLMProvider::Codex(_)) => {
                    return Err(ask_error(
                        BackendErrorCode::Unsupported,
                        "askModelProtocolUnsupported",
                    ));
                }
            };
            Ok(Arc::new(PreparedModel::new(route.provider_id, provider)) as Arc<dyn AskModel>)
        })
    }
}

enum Wire {
    Compatible(OpenAICompatibleLLMProvider),
    Gemini(GeminiProvider),
}

#[derive(Clone, Copy)]
enum Thinking {
    Unverified,
    OpenAi,
    DeepSeek,
    ClaudeBudget(u32),
    ClaudeAdaptive,
    GeminiBudget { normal: i32 },
    GeminiLevel,
}

struct PreparedModel {
    wire: Wire,
    label: AskModelLabel,
    capabilities: AskModelCapabilities,
    thinking: Thinking,
}

impl PreparedModel {
    fn new(channel: String, wire: Wire) -> Self {
        let (service, model, protocol, thinking, images, context_window, configured_output) =
            match &wire {
                Wire::Compatible(provider) => {
                    let config = provider.ask_config();
                    let (service, protocol) = provider.usage_metadata();
                    let (thinking, images, context_window) =
                        compatible_caps(&service, &config.model, &config.protocol);
                    (
                        service_key(service, &config.base_url),
                        config.model.clone(),
                        protocol,
                        thinking,
                        images,
                        context_window,
                        Some(config.protocol.max_tokens as usize),
                    )
                }
                Wire::Gemini(provider) => {
                    let config = provider.ask_config();
                    let base = url::Url::parse(&config.base_url).ok();
                    let official = base.as_ref().is_some_and(|url| {
                        crate::prompt_cache::service_id(url.as_str()) == "gemini"
                            && matches!(url.path().trim_end_matches('/'), "/v1beta")
                    });
                    let (thinking, context_window) = match (official, config.model.as_str()) {
                        (true, "gemini-2.5-flash" | "gemini-2.5-flash-lite") => {
                            (Thinking::GeminiBudget { normal: 0 }, Some(1_048_576))
                        }
                        (true, "gemini-2.5-pro") => {
                            (Thinking::GeminiBudget { normal: 128 }, Some(1_048_576))
                        }
                        (true, "gemini-3-flash-preview" | "gemini-3.5-flash-lite") => {
                            (Thinking::GeminiLevel, Some(1_048_576))
                        }
                        _ => (Thinking::Unverified, None),
                    };
                    (
                        service_key(
                            if official { "gemini" } else { "custom" }.into(),
                            &config.base_url,
                        ),
                        config.model.clone(),
                        "gemini_generate_content".to_string(),
                        thinking,
                        context_window.is_some(),
                        context_window,
                        None,
                    )
                }
            };
        let output_budget = if context_window.is_some() {
            OUTPUT_BUDGET
        } else {
            UNKNOWN_OUTPUT_BUDGET
        };
        let image_token_reserve = if service == "openai" && model == "gpt-4o-mini" {
            2_833 + 8 * 5_667
        } else if images {
            16_384
        } else {
            0
        };
        Self {
            wire,
            label: AskModelLabel {
                service,
                channel,
                model,
                protocol,
            },
            capabilities: AskModelCapabilities {
                context_window,
                output_tokens: configured_output
                    .map_or(output_budget, |max| max.min(output_budget)),
                image_input: images,
                image_token_reserve,
                deep_thinking: !matches!(thinking, Thinking::Unverified),
            },
            thinking,
        }
    }
}

fn service_key(service: String, endpoint: &str) -> String {
    if service != "custom" {
        return service;
    }
    let Ok(url) = url::Url::parse(endpoint.trim()) else {
        return service;
    };
    let origin = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/');
    let digest = Sha256::digest(format!("{origin}{path}"));
    format!("custom.{}", hex::encode(&digest[..12]))
}

fn compatible_caps(
    service: &str,
    model: &str,
    protocol: &crate::llm_protocol::LlmProtocolConfig,
) -> (Thinking, bool, Option<usize>) {
    use LlmRequestFormat::*;
    match (service, protocol.format, model) {
        ("openai", ChatCompletions | Responses, "gpt-4.1" | "gpt-4.1-mini" | "gpt-4.1-nano") => {
            (Thinking::Unverified, true, Some(1_047_576))
        }
        ("openai", ChatCompletions | Responses, "gpt-4o" | "gpt-4o-mini") => {
            (Thinking::Unverified, true, Some(128_000))
        }
        ("openai", ChatCompletions | Responses, "gpt-5" | "gpt-5-mini" | "gpt-5-nano") => {
            (Thinking::OpenAi, true, Some(400_000))
        }
        ("openai", ChatCompletions | Responses, "gpt-5.4" | "gpt-5.4-mini" | "gpt-5.4-nano") => (
            Thinking::OpenAi,
            true,
            Some(if model == "gpt-5.4" {
                1_050_000
            } else {
                400_000
            }),
        ),
        (
            "deepseek",
            ChatCompletions,
            "deepseek-v4-flash" | "deepseek-v4-pro" | "deepseek-flash" | "deepseek-pro",
        ) => (Thinking::DeepSeek, false, Some(1_000_000)),
        ("anthropic", Messages, "claude-sonnet-4-6" | "claude-opus-4-6") => {
            (Thinking::ClaudeAdaptive, true, Some(1_000_000))
        }
        (
            "anthropic",
            Messages,
            "claude-sonnet-4-5"
            | "claude-haiku-4-5"
            | "claude-opus-4-5"
            | "claude-sonnet-4-5-20250929"
            | "claude-opus-4-5-20251101"
            | "claude-haiku-4-5-20251001",
        ) => {
            let valid = protocol.thinking_budget >= 1024
                && (protocol.thinking_budget as usize)
                    < (protocol.max_tokens as usize).min(OUTPUT_BUDGET);
            let thinking = if valid {
                Thinking::ClaudeBudget(protocol.thinking_budget)
            } else {
                Thinking::Unverified
            };
            (thinking, true, Some(200_000))
        }
        _ => (Thinking::Unverified, false, None),
    }
}

impl AskModel for PreparedModel {
    fn label(&self) -> &AskModelLabel {
        &self.label
    }

    fn capabilities(&self) -> AskModelCapabilities {
        self.capabilities
    }

    fn supports_search_tool(&self) -> bool {
        match &self.wire {
            Wire::Compatible(_) => true,
            Wire::Gemini(_) => self.label.service == "gemini",
        }
    }

    fn stream(
        &self,
        request: AskModelRequest,
        cancellation: AskCancellation,
        progress: AskModelSink,
    ) -> BoxFuture<'_, Result<AskModelCompletion, BackendError>> {
        Box::pin(async move {
            if request.output_tokens == 0 || request.output_tokens > self.capabilities.output_tokens
            {
                return Err(ask_error(
                    BackendErrorCode::InvalidArgument,
                    "askOutputLimitInvalid",
                ));
            }
            if request.deep_thinking && !self.capabilities.deep_thinking {
                return Err(ask_error(
                    BackendErrorCode::Unsupported,
                    "askDeepThinkingUnsupported",
                ));
            }
            if !self.capabilities.image_input
                && request
                    .messages
                    .iter()
                    .flat_map(|message| &message.parts)
                    .any(|part| matches!(part, AskModelPart::Image(_)))
            {
                return Err(ask_error(
                    BackendErrorCode::Unsupported,
                    "askImageUnsupported",
                ));
            }
            let (builder, format, gemini) = match &self.wire {
                Wire::Compatible(provider) => {
                    let config = provider.ask_config();
                    let format = config.protocol.format;
                    let messages = compatible_messages(format, &request);
                    let builder = provider
                        .ask_request(messages, |body| {
                            tune_compatible(
                                body,
                                format,
                                &self.label.service,
                                self.thinking,
                                &request,
                                config,
                            );
                            tools::configure(body, format, &request.search);
                        })
                        .map_err(public_model_error)?;
                    (builder, format, false)
                }
                Wire::Gemini(provider) => {
                    let mut body =
                        gemini_body(&request, self.thinking, provider.ask_config().temperature);
                    tools::configure_gemini(&mut body, &request.search);
                    (
                        provider.ask_request(&body),
                        LlmRequestFormat::ChatCompletions,
                        true,
                    )
                }
            };
            let search = matches!(request.search, AskModelSearch::Available);
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(cancelled()),
                result = tokio::time::timeout(STREAM_TOTAL, stream_with_tools(builder, format, gemini, search, progress)) => {
                    result.map_err(|_| public_model_error(LLMError::Timeout))?
                }
            }
        })
    }
}

fn compatible_messages(format: LlmRequestFormat, request: &AskModelRequest) -> Vec<Value> {
    let mut messages = vec![json!({"role": "system", "content": request.instructions})];
    messages.extend(request.messages.iter().map(|message| {
        let role = match message.role {
            AskModelRole::User => "user",
            AskModelRole::Assistant => "assistant",
        };
        let content = match message.parts.as_slice() {
            [AskModelPart::Text(text)] => json!(nonempty_text(text)),
            parts => Value::Array(parts.iter().filter_map(|part| match part {
                AskModelPart::Text(text) if text.trim().is_empty() => None,
                AskModelPart::Text(text) => Some(match format {
                    LlmRequestFormat::Responses => json!({"type": "input_text", "text": text}),
                    _ => json!({"type": "text", "text": text}),
                }),
                AskModelPart::Image(image) => {
                    let data = image_base64(image);
                    let mime = image.mime.media_type();
                    Some(match format {
                        LlmRequestFormat::ChatCompletions => json!({
                            "type": "image_url", "image_url": {"url": format!("data:{mime};base64,{data}")}
                        }),
                        LlmRequestFormat::Responses => json!({
                            "type": "input_image", "image_url": format!("data:{mime};base64,{data}"), "detail": "auto"
                        }),
                        LlmRequestFormat::Messages => json!({
                            "type": "image", "source": {"type": "base64", "media_type": mime, "data": data}
                        }),
                    })
                }
            }).collect()),
        };
        json!({"role": role, "content": content})
    }));
    messages
}

fn tune_compatible(
    body: &mut Value,
    format: LlmRequestFormat,
    service: &str,
    thinking: Thinking,
    request: &AskModelRequest,
    config: &crate::polish::OpenAICompatibleConfig,
) {
    if let Some(object) = body.as_object_mut() {
        for key in [
            "reasoning",
            "reasoning_effort",
            "thinking",
            "enable_thinking",
            "chat_template_kwargs",
        ] {
            object.remove(key);
        }
    }
    let output_key = match format {
        LlmRequestFormat::ChatCompletions if service == "openai" => "max_completion_tokens",
        LlmRequestFormat::ChatCompletions | LlmRequestFormat::Messages => "max_tokens",
        LlmRequestFormat::Responses => "max_output_tokens",
    };
    body[output_key] = json!(request.output_tokens);
    if format == LlmRequestFormat::ChatCompletions && crate::prompt_cache::supports_usage(config) {
        body["stream_options"] = json!({"include_usage": true});
    }
    let deep = request.deep_thinking;
    match thinking {
        Thinking::OpenAi => {
            body.as_object_mut()
                .expect("request body is an object")
                .remove("temperature");
            let effort = if deep { "medium" } else { "low" };
            if format == LlmRequestFormat::Responses {
                body["reasoning"] = json!({"effort": effort});
            } else {
                body["reasoning_effort"] = json!(effort);
            }
        }
        Thinking::DeepSeek => {
            body["thinking"] = json!({"type": if deep { "enabled" } else { "disabled" }});
        }
        Thinking::ClaudeAdaptive if deep => {
            body.as_object_mut()
                .expect("request body is an object")
                .remove("temperature");
            body["thinking"] = json!({"type": "adaptive"});
            body["output_config"] = json!({"effort": "high"});
        }
        Thinking::ClaudeBudget(budget) if deep => {
            body.as_object_mut()
                .expect("request body is an object")
                .remove("temperature");
            body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
        }
        _ => {}
    }
}

fn gemini_body(request: &AskModelRequest, thinking: Thinking, temperature: f32) -> Value {
    let contents = request
        .messages
        .iter()
        .map(|message| {
            let role = match message.role {
                AskModelRole::User => "user",
                AskModelRole::Assistant => "model",
            };
            let parts = message
                .parts
                .iter()
                .map(|part| match part {
                    AskModelPart::Text(text) => json!({"text": nonempty_text(text)}),
                    AskModelPart::Image(image) => json!({"inlineData": {
                        "mimeType": image.mime.media_type(), "data": image_base64(image),
                    }}),
                })
                .collect::<Vec<_>>();
            json!({"role": role, "parts": parts})
        })
        .collect::<Vec<_>>();
    let mut generation = json!({"maxOutputTokens": request.output_tokens});
    match thinking {
        Thinking::GeminiBudget { normal } => {
            let budget = if request.deep_thinking { 4096 } else { normal };
            generation["temperature"] = json!(temperature);
            generation["thinkingConfig"] = json!({"thinkingBudget": budget});
        }
        Thinking::GeminiLevel => {
            generation["thinkingConfig"] =
                json!({"thinkingLevel": if request.deep_thinking { "HIGH" } else { "LOW" }});
        }
        _ => {
            generation["temperature"] = json!(temperature);
        }
    }
    json!({
        "systemInstruction": {"parts": [{"text": request.instructions}]},
        "contents": contents,
        "generationConfig": generation,
    })
}

fn nonempty_text(text: &str) -> &str {
    if text.trim().is_empty() {
        "(Attachment-only question)"
    } else {
        text
    }
}

fn image_base64(image: &AskImage) -> String {
    base64::engine::general_purpose::STANDARD.encode(&image.bytes)
}

#[cfg(test)]
async fn stream(
    builder: reqwest::RequestBuilder,
    format: LlmRequestFormat,
    gemini: bool,
    progress: AskModelSink,
) -> Result<AskModelCompletion, BackendError> {
    stream_with_tools(builder, format, gemini, false, progress).await
}

async fn stream_with_tools(
    builder: reqwest::RequestBuilder,
    format: LlmRequestFormat,
    gemini: bool,
    search: bool,
    progress: AskModelSink,
) -> Result<AskModelCompletion, BackendError> {
    let start = tokio::time::Instant::now();
    let mut response = tokio::time::timeout(STREAM_WAIT, builder.send())
        .await
        .map_err(|_| public_model_error(LLMError::Timeout))?
        .map_err(|error| public_model_error(crate::polish::llm_error_from_reqwest(error)))?;
    if !response.status().is_success() {
        let status = response.status();
        if search && matches!(status.as_u16(), 400 | 422 | 501) {
            return Err(ask_error(
                BackendErrorCode::Provider,
                "askModelRejectedTools",
            ));
        }
        return Err(
            ask_error(BackendErrorCode::Provider, "askModelRequestFailed")
                .retryable(status.is_server_error() || status.as_u16() == 429),
        );
    }
    let mut parser = TextEventStream::new(format).for_ask(gemini);
    let mut tools = tools::SearchStream::new(format, gemini, search);
    let mut usage = ReportedTokens::default();
    let mut bytes = 0usize;
    let mut has_answer = false;
    while !parser.done {
        let wait = if has_answer {
            STREAM_WAIT
        } else {
            STREAM_WAIT.saturating_sub(start.elapsed())
        };
        let Some(chunk) = tokio::time::timeout(wait, response.chunk())
            .await
            .map_err(|_| public_model_error(LLMError::Timeout))?
            .map_err(|error| public_model_error(crate::polish::llm_error_from_reqwest(error)))?
        else {
            break;
        };
        bytes += chunk.len();
        if bytes > MAX_SSE_BYTES {
            return Err(ask_error(BackendErrorCode::Provider, "askStreamTooLarge"));
        }
        parser.push(&chunk).map_err(public_model_error)?;
        loop {
            let event = parser.next_with_observer(|value, kind| tools.observe(value, kind));
            let reported = parser.reported_tokens();
            if reported != usage {
                usage = reported;
                progress(AskModelProgress::Usage(usage));
            }
            match event.map_err(public_model_error)? {
                Some(StreamEvent::Text(delta)) => {
                    has_answer = true;
                    progress(AskModelProgress::Text(delta));
                }
                Some(StreamEvent::Thinking) => progress(AskModelProgress::Thinking),
                Some(StreamEvent::Done | StreamEvent::Ignore) => {}
                None => break,
            }
        }
    }
    parser.finish().map_err(public_model_error)?;
    Ok(tools
        .take()
        .map_or(AskModelCompletion::Complete, AskModelCompletion::ToolCall))
}

fn public_model_error(error: LLMError) -> BackendError {
    let (message, retryable) = match error {
        LLMError::Timeout => ("askModelTimeout", true),
        LLMError::Network(_) => ("askModelNetworkError", true),
        LLMError::ParseError(reason) if reason == "llmResponseIncomplete" => {
            ("askModelIncomplete", true)
        }
        LLMError::ParseError(reason) if reason == "askInvalidToolCall" => {
            ("askInvalidToolCall", false)
        }
        LLMError::ParseError(reason) if reason == "llmContentFiltered" => {
            ("askModelContentFiltered", false)
        }
        _ => ("askModelStreamError", true),
    };
    ask_error(BackendErrorCode::Provider, message).retryable(retryable)
}

#[cfg(test)]
mod tests;
