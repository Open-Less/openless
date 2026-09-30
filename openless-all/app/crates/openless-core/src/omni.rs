//! Model channel of the multimodal (Omni) recognition pipeline (issue #902).
//!
//! Produces text from a system prompt, user text, and optional audio in one call.
//! Credentials use the independent `omni` namespace.
//!
//! Channels:
//! - OpenAI-compatible chat completions: the user content carries base64 WAV
//!   in an `input_audio` part;
//! - Gemini native generateContent: an `inlineData(audio/wav)` part
//!   (reuses `llm_gemini.rs`).

use std::collections::HashMap;

use base64::Engine;
use serde_json::{json, Value};

use crate::polish::{
    append_utf8_sse_chunk, apply_openai_compatible_thinking_control, chat_completions_url,
    extract_assistant_content, finish_utf8_sse_chunks, http_client_builder,
    openai_model_omits_custom_temperature, safe_str_slice, send_with_transient_retry, LLMError,
};

pub const OMNI_GEMINI_PROVIDER_ID: &str = "gemini";
/// Omni request default timeout (seconds). Longer than plain text polish:
/// base64 WAV upload plus audio-model generation.
const OMNI_DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 90;
const BODY_PREVIEW_LIMIT: usize = 200;

#[derive(Clone, Debug)]
pub struct OmniConfig {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub extra_headers: HashMap<String, String>,
    pub temperature: Option<f32>,
    pub thinking_enabled: bool,
}

impl OmniConfig {
    pub fn is_gemini(&self) -> bool {
        self.provider_id.trim() == OMNI_GEMINI_PROVIDER_ID
            || self.base_url.contains("generativelanguage.googleapis.com")
    }

    /// DashScope Omni requires a MIME-free data URL and a separate audio format.
    /// Match the parsed hostname so unrelated compatible endpoints retain bare Base64.
    fn audio_requires_data_url(&self) -> bool {
        reqwest::Url::parse(self.base_url.trim())
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| host.contains("dashscope") || host.contains("aliyuncs"))
    }
}

/// Build-time snapshot of one Omni call (provider id + model) for history attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmniCallLabel {
    pub provider: String,
    pub model: String,
}

/// OpenAI-compatible chat completions channel (`input_audio` audio part).
pub struct OpenAICompatibleOmni {
    config: OmniConfig,
    client: reqwest::Client,
}

impl OpenAICompatibleOmni {
    pub fn new(config: OmniConfig) -> Self {
        // Same as OpenAICompatibleLLMProvider: cache the connection pool by
        // (timeout, proxy-bypass) to reuse TLS handshakes across sentences.
        // Toggling the proxy switch clears and rebuilds the net cache.
        let timeout = OMNI_DEFAULT_REQUEST_TIMEOUT_SECS;
        let no_proxy =
            crate::net::should_bypass_proxy(&config.base_url, crate::net::use_system_proxy());
        let base_url = config.base_url.clone();
        let client = crate::net::cached_client((timeout, no_proxy), || {
            http_client_builder(&base_url, timeout)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        });
        Self { config, client }
    }

    fn omni_body(&self, stream: bool, messages: Vec<Value>) -> Value {
        let mut body = json!({
            "model": self.config.model,
            "stream": stream,
            "messages": messages,
        });
        if let Some(temperature) = self.config.temperature {
            // OpenAI 官方 gpt-5 / gpt-6 只接受默认 temperature=1（#857 / #1101），同润色路径。
            if !(self.config.provider_id.trim() == "openai"
                && openai_model_omits_custom_temperature(&self.config.model))
            {
                body["temperature"] = json!(temperature);
            }
        }
        // DashScope Omni requests text-only output.
        if self.config.audio_requires_data_url() {
            body["modalities"] = json!(["text"]);
        }
        apply_openai_compatible_thinking_control(
            &mut body,
            &self.config.provider_id,
            &self.config.base_url,
            &self.config.model,
            self.config.thinking_enabled,
        );
        body
    }

    fn build_messages(
        &self,
        system_prompt: &str,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
    ) -> Vec<Value> {
        let user_content = match wav_bytes {
            Some(wav) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(wav);
                // DashScope Omni uses data:;base64 plus the separate format field.
                let data = if self.config.audio_requires_data_url() {
                    format!("data:;base64,{encoded}")
                } else {
                    encoded
                };
                let mut parts = vec![json!({
                    "type": "input_audio",
                    "input_audio": { "data": data, "format": "wav" },
                })];
                if !user_text.trim().is_empty() {
                    parts.push(json!({ "type": "text", "text": user_text }));
                }
                Value::Array(parts)
            }
            None => json!(user_text),
        };
        vec![
            json!({ "role": "system", "content": system_prompt }),
            json!({ "role": "user", "content": user_content }),
        ]
    }

    async fn send_unary(&self, url: &str, body: &Value) -> Result<String, LLMError> {
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");
        if !self.config.api_key.trim().is_empty() {
            request = request.header("Authorization", format!("Bearer {}", self.config.api_key));
        }
        for (key, value) in &self.config.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }
        let request = request.json(body);
        let response = send_with_transient_retry(request).await?;
        let status = response.status();
        let body_text = response
            .text()
            .await
            .map_err(crate::polish::llm_error_from_reqwest)?;
        let preview_end = BODY_PREVIEW_LIMIT.min(body_text.len());
        let preview = safe_str_slice(&body_text, preview_end);
        log::info!("[omni] HTTP {} body={}", status.as_u16(), preview);
        if !status.is_success() {
            return Err(LLMError::InvalidResponse {
                status: status.as_u16(),
                body: preview.to_string(),
            });
        }
        extract_assistant_content(&body_text)
    }

    async fn send_streaming<F, C>(
        &self,
        url: &str,
        body: &Value,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream");
        if !self.config.api_key.trim().is_empty() {
            request = request.header("Authorization", format!("Bearer {}", self.config.api_key));
        }
        for (key, value) in &self.config.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }
        let request = request.json(body);
        let response = send_with_transient_retry(request).await?;
        let status = response.status();
        if !status.is_success() {
            let body_text = response
                .text()
                .await
                .map_err(crate::polish::llm_error_from_reqwest)?;
            let preview_end = BODY_PREVIEW_LIMIT.min(body_text.len());
            let preview = safe_str_slice(&body_text, preview_end);
            log::error!("[omni] streaming HTTP {} body={}", status.as_u16(), preview);
            return Err(LLMError::InvalidResponse {
                status: status.as_u16(),
                body: preview.to_string(),
            });
        }

        // Shared UTF-8 decoding normalizes CRLF to LF; one frame = several
        // lines separated by `\n\n`, each line `data: {...}` / `data: [DONE]`.
        let mut response = response;
        let mut buffer = String::new();
        let mut utf8_pending: Vec<u8> = Vec::new();
        let mut full_text = String::new();
        let mut cancelled = false;
        loop {
            if should_cancel() {
                log::info!("[omni] stream cancelled by caller; breaking SSE loop");
                cancelled = true;
                break;
            }
            let chunk_opt = response
                .chunk()
                .await
                .map_err(crate::polish::llm_error_from_reqwest)?;
            let Some(chunk) = chunk_opt else { break };
            append_utf8_sse_chunk(&mut buffer, &mut utf8_pending, &chunk)?;
            while let Some(idx) = buffer.find("\n\n") {
                let event = buffer[..idx].to_string();
                buffer.drain(..idx + 2);
                for line in event.lines() {
                    let Some(payload) = line
                        .strip_prefix("data: ")
                        .or_else(|| line.strip_prefix("data:"))
                    else {
                        continue;
                    };
                    let payload = payload.trim();
                    if payload.is_empty() || payload == "[DONE]" {
                        continue;
                    }
                    let value: Value = match serde_json::from_str(payload) {
                        Ok(value) => value,
                        Err(error) => {
                            log::warn!(
                                "[omni] SSE parse skip: {error}; payload preview: {}",
                                safe_str_slice(payload, 80)
                            );
                            continue;
                        }
                    };
                    if let Some(delta) = value["choices"][0]["delta"]["content"].as_str() {
                        if !delta.is_empty() {
                            full_text.push_str(delta);
                            on_delta(delta);
                        }
                    }
                }
            }
        }
        if !cancelled {
            finish_utf8_sse_chunks(&mut buffer, &mut utf8_pending)?;
        }
        log::info!(
            "[omni] stream done; total chars={}",
            full_text.chars().count()
        );
        if full_text.is_empty() {
            return Err(LLMError::InvalidResponse {
                status: 200,
                body: "empty omni stream".to_string(),
            });
        }
        Ok(full_text)
    }

    pub(crate) async fn complete(
        &self,
        system_prompt: &str,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
    ) -> Result<String, LLMError> {
        let messages = self.build_messages(system_prompt, user_text, wav_bytes);
        let body = self.omni_body(false, messages);
        let url = chat_completions_url(&self.config.base_url);
        log::info!(
            "[omni] POST {} provider={} model={} audio={}",
            crate::net::sanitized_url_for_logs(&url),
            self.config.provider_id,
            self.config.model,
            wav_bytes.is_some()
        );
        self.send_unary(&url, &body).await
    }

    pub(crate) async fn complete_streaming<F, C>(
        &self,
        system_prompt: &str,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let messages = self.build_messages(system_prompt, user_text, wav_bytes);
        let body = self.omni_body(true, messages);
        let url = chat_completions_url(&self.config.base_url);
        log::info!(
            "[omni] POST {} provider={} model={} audio={} stream=true",
            crate::net::sanitized_url_for_logs(&url),
            self.config.provider_id,
            self.config.model,
            wav_bytes.is_some()
        );
        self.send_streaming(&url, &body, on_delta, should_cancel)
            .await
    }
}

/// Unified multimodal-channel entry point: routes by config to the Gemini
/// native or OpenAI-compatible client.
pub enum OmniProvider {
    Gemini {
        provider: crate::llm_gemini::GeminiProvider,
        label: OmniCallLabel,
    },
    OpenAI(OpenAICompatibleOmni),
}

impl OmniProvider {
    pub fn new(config: OmniConfig) -> Self {
        if config.is_gemini() {
            let label = OmniCallLabel {
                provider: config.provider_id.clone(),
                model: config.model.clone(),
            };
            let gemini_config = crate::llm_gemini::GeminiConfig::new(
                config.api_key.clone(),
                config.model.clone(),
                config.base_url.clone(),
            )
            .with_thinking_enabled(config.thinking_enabled);
            let mut gemini_config = gemini_config;
            if let Some(temperature) = config.temperature {
                gemini_config.temperature = temperature;
            }
            Self::Gemini {
                provider: crate::llm_gemini::GeminiProvider::new(gemini_config),
                label,
            }
        } else {
            Self::OpenAI(OpenAICompatibleOmni::new(config))
        }
    }

    pub fn call_label(&self) -> OmniCallLabel {
        match self {
            Self::Gemini { label, .. } => label.clone(),
            Self::OpenAI(provider) => OmniCallLabel {
                provider: provider.config.provider_id.clone(),
                model: provider.config.model.clone(),
            },
        }
    }

    /// One-shot call: audio + prompt produce final text in a single step;
    /// without audio it's plain text (reused by text pipelines).
    pub async fn complete(
        &self,
        system_prompt: &str,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
    ) -> Result<String, LLMError> {
        match self {
            Self::Gemini { provider, .. } => {
                provider
                    .complete_omni(system_prompt, user_text, wav_bytes)
                    .await
            }
            Self::OpenAI(provider) => provider.complete(system_prompt, user_text, wav_bytes).await,
        }
    }

    /// Streaming output. The OpenAI-compatible channel calls back per SSE
    /// delta; the Gemini channel v1 returns in one shot and delivers the
    /// full text via a single `on_delta` (matches the approved "Gemini
    /// falls back to one-shot" plan).
    pub async fn complete_streaming<F, C>(
        &self,
        system_prompt: &str,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        match self {
            Self::Gemini { provider, .. } => {
                let text = provider
                    .complete_omni(system_prompt, user_text, wav_bytes)
                    .await?;
                on_delta(&text);
                Ok(text)
            }
            Self::OpenAI(provider) => {
                provider
                    .complete_streaming(
                        system_prompt,
                        user_text,
                        wav_bytes,
                        on_delta,
                        should_cancel,
                    )
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> OmniConfig {
        OmniConfig {
            provider_id: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            model: "gpt-4o-audio-preview".into(),
            extra_headers: HashMap::new(),
            temperature: Some(0.3),
            thinking_enabled: false,
        }
    }

    #[test]
    fn build_messages_embeds_wav_as_input_audio_part() {
        let provider = OpenAICompatibleOmni::new(config());
        let messages = provider.build_messages("system-prompt", "", Some(&[1u8, 2, 3, 4]));
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "system-prompt");
        assert_eq!(messages[1]["role"], "user");
        let parts = messages[1]["content"].as_array().expect("audio parts");
        assert_eq!(parts[0]["type"], "input_audio");
        assert_eq!(parts[0]["input_audio"]["format"], "wav");
        let data = parts[0]["input_audio"]["data"]
            .as_str()
            .expect("base64 data");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("valid base64");
        assert_eq!(decoded, vec![1u8, 2, 3, 4]);
        // No extra text part when user_text is empty.
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn build_messages_text_only_when_no_audio() {
        let provider = OpenAICompatibleOmni::new(config());
        let messages = provider.build_messages("system", "你好", None);
        assert_eq!(messages[1]["content"], "你好");
    }

    #[test]
    fn build_messages_appends_text_part_alongside_audio() {
        let provider = OpenAICompatibleOmni::new(config());
        let messages = provider.build_messages("system", "翻译成中文", Some(&[0u8; 8]));
        let parts = messages[1]["content"].as_array().expect("audio parts");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1]["type"], "text");
        assert_eq!(parts[1]["text"], "翻译成中文");
    }

    #[test]
    fn build_messages_wraps_audio_as_data_url_for_dashscope() {
        let mut dashscope = config();
        dashscope.base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1".into();
        let provider = OpenAICompatibleOmni::new(dashscope);
        let messages = provider.build_messages("system-prompt", "", Some(&[1u8, 2, 3, 4]));
        let parts = messages[1]["content"].as_array().expect("audio parts");
        let data = parts[0]["input_audio"]["data"]
            .as_str()
            .expect("audio data");
        let payload = data
            .strip_prefix("data:;base64,")
            .expect("official DashScope Omni data-url prefix");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("valid base64");
        assert_eq!(decoded, vec![1u8, 2, 3, 4]);
    }

    #[test]
    fn build_messages_wraps_audio_as_data_url_for_bailian_dedicated_endpoint() {
        let mut maas = config();
        maas.base_url =
            "https://llm-example.cn-beijing.maas.aliyuncs.com/compatible-mode/v1".into();
        let provider = OpenAICompatibleOmni::new(maas);
        let messages = provider.build_messages("system-prompt", "", Some(&[1u8, 2, 3, 4]));
        let parts = messages[1]["content"].as_array().expect("audio parts");
        assert!(parts[0]["input_audio"]["data"]
            .as_str()
            .expect("audio data")
            .starts_with("data:;base64,"));
    }

    #[test]
    fn omni_audio_data_url_matches_dashscope_hosts_only() {
        assert!(!config().audio_requires_data_url());
        let mut dashscope = config();
        dashscope.base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1".into();
        assert!(dashscope.audio_requires_data_url());
        let mut maas = config();
        maas.base_url = "https://LLM-EXAMPLE.cn-beijing.maas.aliyuncs.com/v1".into();
        assert!(maas.audio_requires_data_url());
        // Keywords outside the host (a path that happens to contain dashscope) must not trigger.
        let mut path_only = config();
        path_only.base_url = "https://example.com/proxy/dashscope/v1".into();
        assert!(!path_only.audio_requires_data_url());
        let mut empty = config();
        empty.base_url = String::new();
        assert!(!empty.audio_requires_data_url());
    }

    #[test]
    fn omni_audio_data_url_ignores_non_host_url_components() {
        for base_url in [
            "https://dashscope@api.example.com/v1",
            "https://user:aliyuncs@api.example.com/v1",
            "https://api.example.com?provider=dashscope",
            "https://api.example.com#aliyuncs",
        ] {
            let mut other = config();
            other.base_url = base_url.into();
            assert!(!other.audio_requires_data_url(), "{base_url}");
            let provider = OpenAICompatibleOmni::new(other);
            let messages = provider.build_messages("system", "", Some(&[1u8, 2, 3, 4]));
            assert_eq!(messages[1]["content"][0]["input_audio"]["data"], "AQIDBA==");
        }
    }

    #[test]
    fn omni_body_has_stream_model_and_temperature() {
        let provider = OpenAICompatibleOmni::new(config());
        let body = provider.omni_body(true, vec![json!({"role": "user", "content": "x"})]);
        assert_eq!(body["stream"], true);
        assert_eq!(body["model"], "gpt-4o-audio-preview");
        // temperature is stored as f32 (0.3f32 serializes as 0.30000001192092896); compare with tolerance.
        assert!((body["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert!(body.get("modalities").is_none());
    }

    #[test]
    fn omni_body_sets_text_modalities_for_dashscope() {
        let mut dashscope = config();
        dashscope.base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1".into();
        let provider = OpenAICompatibleOmni::new(dashscope);
        let body = provider.omni_body(true, vec![json!({"role": "user", "content": "x"})]);
        assert_eq!(body["modalities"], json!(["text"]));
    }

    #[test]
    fn omni_body_omits_temperature_for_openai_gpt6_api_ids() {
        for model in ["gpt-6-astra", "gpt-6-sol", "gpt-6-luna"] {
            let mut omni = config();
            omni.model = model.into();
            let provider = OpenAICompatibleOmni::new(omni);
            let body = provider.omni_body(false, vec![json!({"role": "user", "content": "x"})]);

            assert!(
                body.get("temperature").is_none(),
                "{model} must not receive temperature (issue #1101)"
            );
        }
    }

    #[test]
    fn omni_gemini_routing_uses_provider_id_or_base_url() {
        assert!(!config().is_gemini());
        let mut gemini = config();
        gemini.provider_id = "gemini".into();
        assert!(gemini.is_gemini());
        let mut via_url = config();
        via_url.base_url = "https://generativelanguage.googleapis.com/v1beta".into();
        assert!(via_url.is_gemini());
    }
}
