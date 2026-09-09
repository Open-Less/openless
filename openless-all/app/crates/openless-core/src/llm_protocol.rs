//! 渠道级文本协议：请求格式、鉴权和正文事件由 Core 统一解释。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::credentials::{CredentialKey, CredentialNamespace, CredentialStore};
use crate::polish::{LLMError, OpenAICompatibleConfig};
use crate::{BackendError, BackendErrorCode};

pub const REQUEST_FORMAT_ACCOUNT: &str = "ark.request_format";
pub const MESSAGES_THINKING_ACCOUNT: &str = "ark.messages_thinking";
pub const MAX_TOKENS_ACCOUNT: &str = "ark.max_tokens";
pub const THINKING_BUDGET_ACCOUNT: &str = "ark.thinking_budget";
pub const CONFIG_ACCOUNTS: [&str; 4] = [
    REQUEST_FORMAT_ACCOUNT,
    MESSAGES_THINKING_ACCOUNT,
    MAX_TOKENS_ACCOUNT,
    THINKING_BUDGET_ACCOUNT,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmRequestFormat {
    #[default]
    ChatCompletions,
    Responses,
    Messages,
}

impl LlmRequestFormat {
    pub const ALL: [Self; 3] = [Self::ChatCompletions, Self::Responses, Self::Messages];

    pub fn default_for(provider: &str) -> Self {
        match provider {
            "custom_responses" => Self::Responses,
            "custom_messages" => Self::Messages,
            _ => Self::ChatCompletions,
        }
    }

    pub fn selectable(provider: &str) -> bool {
        !matches!(provider, "gemini" | "codex_oauth" | "tencentTokenHub")
    }

    pub fn parse(value: &str) -> Result<Self, BackendError> {
        match value.trim() {
            "chat_completions" => Ok(Self::ChatCompletions),
            "responses" => Ok(Self::Responses),
            "messages" => Ok(Self::Messages),
            _ => Err(config_error("llmRequestFormatInvalid")),
        }
    }

    pub fn url(self, endpoint: &str) -> Result<String, LLMError> {
        endpoint_url(
            endpoint,
            match self {
                Self::ChatCompletions => "/chat/completions",
                Self::Responses => "/responses",
                Self::Messages => "/messages",
            },
        )
    }

    pub fn headers(self, api_key: &str) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        if self == Self::Messages {
            headers.push(("anthropic-version".into(), "2023-06-01".into()));
        }
        if !api_key.trim().is_empty() {
            headers.push(if self == Self::Messages {
                ("x-api-key".into(), api_key.to_string())
            } else {
                ("Authorization".into(), format!("Bearer {api_key}"))
            });
        }
        headers
    }
}

/// 更换格式只替换已知的末端路径，不破坏网关前缀及查询参数。
pub fn endpoint_url(endpoint: &str, suffix: &str) -> Result<String, LLMError> {
    let mut url = url::Url::parse(endpoint.trim())
        .map_err(|_| LLMError::ParseError("invalid LLM endpoint".into()))?;
    let path = url.path().trim_end_matches('/');
    let prefix = ["/chat/completions", "/responses", "/messages", "/models"]
        .iter()
        .find_map(|suffix| path.strip_suffix(suffix))
        .unwrap_or(path);
    url.set_path(&format!("{prefix}{suffix}"));
    Ok(url.to_string())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagesThinking {
    #[default]
    Adaptive,
    Budget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProtocolConfig {
    pub format: LlmRequestFormat,
    pub messages_thinking: MessagesThinking,
    pub max_tokens: u32,
    pub thinking_budget: u32,
}

impl Default for LlmProtocolConfig {
    fn default() -> Self {
        Self {
            format: LlmRequestFormat::ChatCompletions,
            messages_thinking: MessagesThinking::Adaptive,
            max_tokens: 8192,
            thinking_budget: 1024,
        }
    }
}

impl LlmProtocolConfig {
    pub fn validate_headers(
        &self,
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<(), BackendError> {
        if self.format == LlmRequestFormat::Messages
            && headers.keys().any(|name| {
                name.eq_ignore_ascii_case("x-api-key")
                    || name.eq_ignore_ascii_case("anthropic-version")
            })
        {
            return Err(config_error("llmProtocolHeaderConflict"));
        }
        Ok(())
    }

    pub async fn load(
        store: &dyn CredentialStore,
        channel: &str,
        provider: &str,
    ) -> Result<Self, BackendError> {
        let mut config = Self {
            format: LlmRequestFormat::default_for(provider),
            ..Self::default()
        };
        if !LlmRequestFormat::selectable(provider) {
            return Ok(config);
        }
        for account in CONFIG_ACCOUNTS {
            let key =
                CredentialKey::new(CredentialNamespace::Llm, Some(channel.to_string()), account)?;
            if let Some(value) = store.read(key).await? {
                config.apply(account, value.expose_secret())?;
            }
        }
        config.validate()?;
        Ok(config)
    }

    pub fn apply(&mut self, account: &str, value: &str) -> Result<(), BackendError> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(());
        }
        match account {
            REQUEST_FORMAT_ACCOUNT => self.format = LlmRequestFormat::parse(value)?,
            MESSAGES_THINKING_ACCOUNT => {
                self.messages_thinking = match value {
                    "adaptive" => MessagesThinking::Adaptive,
                    "budget" => MessagesThinking::Budget,
                    _ => return Err(config_error("llmThinkingModeInvalid")),
                }
            }
            MAX_TOKENS_ACCOUNT => self.max_tokens = positive_tokens(value)?,
            THINKING_BUDGET_ACCOUNT => {
                self.thinking_budget = positive_tokens(value)?;
                if self.thinking_budget < 1024 {
                    return Err(config_error("llmThinkingBudgetInvalid"));
                }
            }
            _ => return Err(config_error("llmRequestFormatInvalid")),
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), BackendError> {
        if self.format == LlmRequestFormat::Messages
            && (self.max_tokens == 0
                || self.messages_thinking == MessagesThinking::Budget
                    && (self.thinking_budget < 1024 || self.thinking_budget >= self.max_tokens))
        {
            return Err(config_error("llmThinkingBudgetInvalid"));
        }
        Ok(())
    }
}

fn positive_tokens(value: &str) -> Result<u32, BackendError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| config_error("llmTokenLimitInvalid"))
}

fn config_error(message: &str) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, message)
}

pub(crate) fn request_body(
    config: &OpenAICompatibleConfig,
    stream: bool,
    messages: Vec<Value>,
) -> Value {
    let mut body = match config.protocol.format {
        LlmRequestFormat::ChatCompletions => {
            unreachable!("Chat Completions retains its provider rules")
        }
        LlmRequestFormat::Responses => {
            let mut body =
                json!({"model": config.model, "stream": stream, "store": false, "input": messages});
            let model = config
                .model
                .trim()
                .strip_prefix("openai/")
                .unwrap_or(config.model.trim())
                .to_ascii_lowercase();
            // 已知普通模型不接受 reasoning；未知网关模型按所选兼容协议声明参数。
            if !(model.starts_with("gpt-4")
                || model.starts_with("gpt-3.5")
                || model.starts_with("chatgpt-4"))
            {
                let effort = if model.starts_with("gpt-5-pro")
                    || model.contains("-pro") && model.starts_with("gpt-5.")
                {
                    "high"
                } else if config.thinking_enabled {
                    "medium"
                } else {
                    "low"
                };
                body["reasoning"] = json!({"effort": effort});
            }
            body
        }
        LlmRequestFormat::Messages => {
            let mut system = Vec::new();
            let mut turns = Vec::new();
            for message in messages {
                if matches!(message["role"].as_str(), Some("system" | "developer")) {
                    if let Some(text) = message["content"].as_str() {
                        system.push(text.to_string());
                    }
                } else {
                    turns.push(message);
                }
            }
            let mut body = json!({"model": config.model, "stream": stream, "messages": turns, "max_tokens": config.protocol.max_tokens});
            if !system.is_empty() {
                body["system"] = json!(system.join("\n\n"));
            }
            if config.thinking_enabled {
                body["thinking"] =
                    if config.protocol.messages_thinking == MessagesThinking::Adaptive {
                        json!({"type": "adaptive"})
                    } else {
                        json!({"type": "enabled", "budget_tokens": config.protocol.thinking_budget})
                    };
            }
            body
        }
    };
    if body.get("reasoning").is_none()
        && !(config.protocol.format == LlmRequestFormat::Messages && config.thinking_enabled)
    {
        if let Some(temperature) = config.temperature {
            body["temperature"] = json!(temperature);
        }
    }
    body
}

fn response_error(message: &str) -> LLMError {
    LLMError::ParseError(message.to_string())
}

fn check_stop_reason(value: &Value) -> Result<bool, LLMError> {
    if let Some(reason) = value.as_str() {
        if !matches!(reason, "end_turn" | "stop_sequence") {
            return Err(response_error("llmResponseIncomplete"));
        }
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn extract_text(format: LlmRequestFormat, text: &str) -> Result<String, LLMError> {
    if format == LlmRequestFormat::ChatCompletions {
        return crate::polish::extract_assistant_content(text);
    }
    let value: Value =
        serde_json::from_str(text).map_err(|_| response_error("invalid LLM JSON"))?;
    if !value["error"].is_null() {
        return Err(response_error("llmStreamError"));
    }
    let mut output = String::new();
    match format {
        LlmRequestFormat::Responses => {
            if value["status"] != "completed" {
                return Err(response_error("llmResponseIncomplete"));
            }
            if let Some(items) = value["output"].as_array() {
                for item in items {
                    if item["type"] == "message" && item["role"] == "assistant" {
                        append_blocks(&mut output, &item["content"], "output_text");
                    }
                }
            }
        }
        LlmRequestFormat::Messages => {
            if !check_stop_reason(&value["stop_reason"])? {
                return Err(response_error("llmResponseIncomplete"));
            }
            append_blocks(&mut output, &value["content"], "text");
        }
        LlmRequestFormat::ChatCompletions => unreachable!(),
    }
    if output.is_empty() {
        return Err(response_error("empty LLM response"));
    }
    Ok(crate::polish::clean_polish_output(&output))
}

fn append_blocks(output: &mut String, blocks: &Value, kind: &str) {
    if let Some(blocks) = blocks.as_array() {
        for block in blocks {
            if block["type"] == kind {
                if let Some(text) = block["text"].as_str() {
                    output.push_str(text);
                }
            }
        }
    }
}

pub(crate) enum StreamEvent {
    Text(String),
    Done,
    Ignore,
}

/// 共用 SSE 分帧；保留未完整的 UTF-8 字节，不能逐个 HTTP chunk 有损解码。
pub(crate) struct TextEventStream {
    format: LlmRequestFormat,
    buffer: String,
    pending: Vec<u8>,
    messages_complete: bool,
    pub done: bool,
}

impl TextEventStream {
    pub fn new(format: LlmRequestFormat) -> Self {
        Self {
            format,
            buffer: String::new(),
            pending: Vec::new(),
            messages_complete: false,
            done: false,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), LLMError> {
        crate::polish::append_utf8_sse_chunk(&mut self.buffer, &mut self.pending, chunk)?;
        // 在完整字符串上替换，兼容 CR 与 LF 分属不同网络块。
        if self.buffer.contains("\r\n") {
            self.buffer = self.buffer.replace("\r\n", "\n");
        }
        Ok(())
    }

    pub fn next(&mut self) -> Result<Option<StreamEvent>, LLMError> {
        if self.done {
            return Ok(None);
        }
        let Some(end) = self.buffer.find("\n\n") else {
            return Ok(None);
        };
        let event = self.buffer[..end].to_string();
        self.buffer.drain(..end + 2);
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            return Ok(Some(StreamEvent::Ignore));
        }
        if self.format == LlmRequestFormat::ChatCompletions && data.trim() == "[DONE]" {
            self.done = true;
            return Ok(Some(StreamEvent::Done));
        }
        let value: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) if self.format == LlmRequestFormat::ChatCompletions => {
                return Ok(Some(StreamEvent::Ignore))
            }
            Err(_) => return Err(response_error("invalid LLM SSE JSON")),
        };
        let kind = value["type"]
            .as_str()
            .or_else(|| {
                event
                    .lines()
                    .find_map(|line| line.strip_prefix("event:").map(str::trim))
            })
            .unwrap_or("");
        if kind == "error" || !value["error"].is_null() {
            return Err(response_error("llmStreamError"));
        }
        let text = match self.format {
            LlmRequestFormat::ChatCompletions => value["choices"][0]["delta"]["content"].as_str(),
            LlmRequestFormat::Responses => match kind {
                "response.output_text.delta" => value["delta"].as_str(),
                "response.completed" => {
                    if value["response"]["status"]
                        .as_str()
                        .is_some_and(|s| s != "completed")
                    {
                        return Err(response_error("llmResponseIncomplete"));
                    }
                    self.done = true;
                    None
                }
                "response.failed" | "response.incomplete" => {
                    return Err(response_error("llmResponseIncomplete"))
                }
                _ => None,
            },
            LlmRequestFormat::Messages => match kind {
                "content_block_start" if value["content_block"]["type"] == "text" => {
                    value["content_block"]["text"].as_str()
                }
                "content_block_delta" if value["delta"]["type"] == "text_delta" => {
                    value["delta"]["text"].as_str()
                }
                "message_delta" => {
                    if check_stop_reason(&value["delta"]["stop_reason"])? {
                        self.messages_complete = true;
                    }
                    None
                }
                "message_stop" => {
                    if !self.messages_complete {
                        return Err(response_error("llmResponseIncomplete"));
                    }
                    self.done = true;
                    None
                }
                _ => None,
            },
        };
        Ok(Some(
            if let Some(text) = text.filter(|text| !text.is_empty()) {
                StreamEvent::Text(text.to_string())
            } else if self.done {
                StreamEvent::Done
            } else {
                StreamEvent::Ignore
            },
        ))
    }

    pub fn finish(&mut self) -> Result<(), LLMError> {
        crate::polish::finish_utf8_sse_chunks(&mut self.buffer, &mut self.pending)?;
        if self.format != LlmRequestFormat::ChatCompletions && !self.done {
            return Err(response_error("llmResponseIncomplete"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{InMemoryCredentialStore, SecretValue};

    #[tokio::test]
    async fn protocol_defaults_overrides_and_channels_are_independent() {
        let store = InMemoryCredentialStore::default();
        for (provider, expected) in [
            ("custom", LlmRequestFormat::ChatCompletions),
            ("custom_responses", LlmRequestFormat::Responses),
            ("custom_messages", LlmRequestFormat::Messages),
            ("deepseek", LlmRequestFormat::ChatCompletions),
        ] {
            assert_eq!(
                LlmProtocolConfig::load(&store, "a", provider)
                    .await
                    .unwrap()
                    .format,
                expected
            );
        }
        let key = CredentialKey::new(
            CredentialNamespace::Llm,
            Some("a".into()),
            REQUEST_FORMAT_ACCOUNT,
        )
        .unwrap();
        store
            .write(key.clone(), SecretValue::new("messages"))
            .await
            .unwrap();
        assert_eq!(
            LlmProtocolConfig::load(&store, "a", "openai")
                .await
                .unwrap()
                .format,
            LlmRequestFormat::Messages
        );
        assert_eq!(
            LlmProtocolConfig::load(&store, "b", "openai")
                .await
                .unwrap()
                .format,
            LlmRequestFormat::ChatCompletions
        );
        store.write(key, SecretValue::new("invalid")).await.unwrap();
        assert!(LlmProtocolConfig::load(&store, "a", "openai")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn tokenhub_is_fixed_to_chat_completions() {
        let store = InMemoryCredentialStore::default();
        store
            .write(
                CredentialKey::new(
                    CredentialNamespace::Llm,
                    Some("tokenhub".into()),
                    REQUEST_FORMAT_ACCOUNT,
                )
                .unwrap(),
                SecretValue::new("messages"),
            )
            .await
            .unwrap();

        assert!(!LlmRequestFormat::selectable("tencentTokenHub"));
        assert_eq!(
            LlmProtocolConfig::load(&store, "tokenhub", "tencentTokenHub")
                .await
                .unwrap()
                .format,
            LlmRequestFormat::ChatCompletions
        );
    }

    #[test]
    fn urls_and_auth_follow_format_without_losing_gateway_paths() {
        for descriptor in crate::provider_rules::provider_descriptors(crate::ProviderKind::Llm) {
            if LlmRequestFormat::selectable(descriptor.provider_type.as_str()) {
                assert_eq!(descriptor.supported_request_formats, LlmRequestFormat::ALL);
                assert_eq!(
                    descriptor.default_request_format,
                    Some(LlmRequestFormat::default_for(
                        descriptor.provider_type.as_str()
                    ))
                );
            } else {
                assert!(descriptor.supported_request_formats.is_empty());
                assert!(descriptor.default_request_format.is_none());
            }
        }
        for format in LlmRequestFormat::ALL {
            let suffix = match format {
                LlmRequestFormat::ChatCompletions => "chat/completions",
                LlmRequestFormat::Responses => "responses",
                LlmRequestFormat::Messages => "messages",
            };
            for old in [
                "",
                "/chat/completions/",
                "/responses",
                "/messages/",
                "/models",
            ] {
                let base = format!("https://example.com/gateway/v1{old}?tenant=1#local");
                assert_eq!(
                    format.url(&base).unwrap(),
                    format!("https://example.com/gateway/v1/{suffix}?tenant=1#local")
                );
                assert_eq!(
                    endpoint_url(&base, "/models").unwrap(),
                    "https://example.com/gateway/v1/models?tenant=1#local"
                );
            }
            let headers = format.headers("test-key");
            if format == LlmRequestFormat::Messages {
                assert!(headers.contains(&("x-api-key".into(), "test-key".into())));
                assert!(!headers.iter().any(|(name, _)| name == "Authorization"));
            } else {
                assert_eq!(
                    headers,
                    vec![("Authorization".into(), "Bearer test-key".into())]
                );
            }
        }
        let headers =
            crate::provider_rules::parse_extra_headers(r#"{"X-API-Key":"override"}"#).unwrap();
        assert!(LlmProtocolConfig::default()
            .validate_headers(&headers)
            .is_ok());
        assert!(LlmProtocolConfig {
            format: LlmRequestFormat::Messages,
            ..Default::default()
        }
        .validate_headers(&headers)
        .is_err());
    }

    #[test]
    fn request_shapes_and_thinking_do_not_leak_between_protocols() {
        let messages = vec![
            json!({"role":"system","content":"rules"}),
            json!({"role":"user","content":"old"}),
            json!({"role":"assistant","content":"answer"}),
            json!({"role":"user","content":"new"}),
        ];
        let mut config = OpenAICompatibleConfig::new(
            "deepseek",
            "test",
            "https://example.com/v1",
            "key",
            "gateway-model",
        )
        .with_temperature(Some(0.5));
        config.protocol.format = LlmRequestFormat::Responses;
        for enabled in [false, true] {
            config.thinking_enabled = enabled;
            let body = request_body(&config, true, messages.clone());
            assert_eq!(body["input"], json!(messages));
            assert_eq!(body["store"], false);
            assert_eq!(
                body["reasoning"]["effort"],
                if enabled { "medium" } else { "low" }
            );
            for absent in [
                "messages",
                "thinking",
                "enable_thinking",
                "reasoning_effort",
                "temperature",
            ] {
                assert!(body.get(absent).is_none());
            }
        }
        config.model = "gpt-4o".into();
        let body = request_body(&config, false, messages.clone());
        assert!(body.get("reasoning").is_none());
        assert_eq!(body["temperature"], 0.5);
        config.model = "gpt-5-pro".into();
        assert_eq!(
            request_body(&config, false, messages.clone())["reasoning"]["effort"],
            "high"
        );
        config.protocol.format = LlmRequestFormat::Messages;
        for enabled in [false, true] {
            config.thinking_enabled = enabled;
            for mode in [MessagesThinking::Adaptive, MessagesThinking::Budget] {
                config.protocol.messages_thinking = mode;
                let body = request_body(&config, false, messages.clone());
                assert_eq!(body["system"], "rules");
                assert_eq!(body["messages"], json!(&messages[1..]));
                assert_eq!(body["max_tokens"], 8192);
                if enabled {
                    assert_eq!(
                        body["thinking"]["type"],
                        if mode == MessagesThinking::Adaptive {
                            "adaptive"
                        } else {
                            "enabled"
                        }
                    );
                } else {
                    assert!(body.get("thinking").is_none());
                }
                assert_eq!(body.get("temperature").is_none(), enabled);
                if enabled && mode == MessagesThinking::Budget {
                    assert_eq!(body["thinking"]["budget_tokens"], 1024);
                }
                for absent in ["input", "reasoning", "reasoning_effort", "enable_thinking"] {
                    assert!(body.get(absent).is_none());
                }
            }
        }
    }

    #[test]
    fn budgets_are_validated_and_non_streaming_extracts_only_complete_text() {
        let mut config = LlmProtocolConfig {
            format: LlmRequestFormat::Messages,
            messages_thinking: MessagesThinking::Budget,
            ..Default::default()
        };
        for value in ["0", "-1", "1.5", "4294967296"] {
            assert!(config.apply(MAX_TOKENS_ACCOUNT, value).is_err());
        }
        assert!(config.apply(THINKING_BUDGET_ACCOUNT, "1023").is_err());
        config.apply(THINKING_BUDGET_ACCOUNT, "8192").unwrap();
        assert!(config.validate().is_err());
        config.apply(MAX_TOKENS_ACCOUNT, "10000").unwrap();
        config.validate().unwrap();
        for (format, body) in [
            (
                LlmRequestFormat::Responses,
                json!({"status":"completed", "output":[{"type":"reasoning","summary":"secret"},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"你"},{"type":"output_text","text":"好"}]}]}),
            ),
            (
                LlmRequestFormat::Messages,
                json!({"stop_reason":"end_turn", "content":[{"type":"thinking","thinking":"secret"},{"type":"text","text":"你"},{"type":"text","text":"好"}]}),
            ),
        ] {
            assert_eq!(extract_text(format, &body.to_string()).unwrap(), "你好");
        }
        assert!(extract_text(
            LlmRequestFormat::Responses,
            r#"{"status":"incomplete","output":[]}"#
        )
        .is_err());
        assert!(extract_text(
            LlmRequestFormat::Messages,
            r#"{"stop_reason":"max_tokens","content":[{"type":"text","text":"partial"}]}"#
        )
        .is_err());
    }

    #[test]
    fn sse_handles_every_byte_boundary_and_requires_successful_termination() {
        for (format, fixture) in [
            (LlmRequestFormat::Responses, "event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"你好🙂\"}\r\n\r\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"secret\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"),
            (LlmRequestFormat::Messages, "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"你好🙂\"}}\r\n\r\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"secret\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n"),
        ] {
            for size in 1..=fixture.len() {
                let mut stream = TextEventStream::new(format);
                let mut text = String::new();
                for chunk in fixture.as_bytes().chunks(size) {
                    stream.push(chunk).unwrap();
                    while let Some(event) = stream.next().unwrap() {
                        if let StreamEvent::Text(delta) = event { text.push_str(&delta); }
                    }
                }
                stream.finish().unwrap();
                assert_eq!(text, "你好🙂");
            }
            assert!(TextEventStream::new(format).finish().is_err());
        }
        for (format, event) in [
            (LlmRequestFormat::Responses, r#"{"type":"response.failed"}"#),
            (
                LlmRequestFormat::Responses,
                r#"{"type":"response.incomplete"}"#,
            ),
            (
                LlmRequestFormat::Messages,
                r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#,
            ),
            (
                LlmRequestFormat::Messages,
                r#"{"type":"error","error":{"message":"secret"}}"#,
            ),
        ] {
            let mut stream = TextEventStream::new(format);
            stream
                .push(format!("data: {event}\n\n").as_bytes())
                .unwrap();
            let error = stream
                .next()
                .err()
                .expect("must reject unsuccessful streams");
            assert!(!error.to_string().contains("secret"));
        }

        let mut stream = TextEventStream::new(LlmRequestFormat::Messages);
        stream
            .push(
                b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n",
            )
            .unwrap();
        assert!(matches!(stream.next().unwrap(), Some(StreamEvent::Text(_))));
        let error = match stream.next() {
            Err(error) => error,
            Ok(_) => panic!("message_stop without stop_reason must fail"),
        };
        assert!(error.to_string().contains("llmResponseIncomplete"));
    }
}
