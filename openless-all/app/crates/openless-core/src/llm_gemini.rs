#![allow(clippy::too_many_arguments)]

//! Native Google Gemini generateContent / streamGenerateContent client.
//!
//! Constraints that rule out reusing `polish.rs::OpenAICompatibleLLMProvider`:
//! 1. **Thinking-mode control** — Gemini's native `thinkingConfig` is more
//!    direct than the OpenAI-compatible shim's provider-private fields;
//!    OpenLess only exposes a channel-level switch, not a per-model table.
//! 2. **Auth** — native uses the `x-goog-api-key` header (Bearer is not
//!    recognized); OpenAICompatibleLLMProvider hardcodes Bearer Authorization.
//! 3. **Request/response shape** — native `contents` uses `role: user|model`
//!    with no chat-completions system role; the system prompt goes through
//!    `systemInstruction`.
//!
//! Prompt assembly (system_prompt / user_prompt / qa system_prompt) reuses
//! `polish.rs::compose_*` pub(crate) builders so the two LLM clients can't
//! drift. `clean_polish_output` is reused too — the leading self-narration
//! prefix banned by polish prompts is only stripped on the native path
//! through it.

use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use crate::polish::{
    clean_polish_output, compose_qa_system_prompt, compose_translate_prompts,
    llm_error_from_reqwest, safe_str_slice, LLMError,
};
use crate::shared_types::{ChineseScriptPreference, OutputLanguagePreference, QaChatMessage};
use crate::types::PolishMode;

const DEFAULT_TEMPERATURE: f32 = 0.3;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
const BODY_PREVIEW_LIMIT: usize = 200;

#[derive(Clone, Debug)]
pub struct GeminiConfig {
    pub api_key: String,
    pub model: String,
    /// e.g. `https://generativelanguage.googleapis.com/v1beta`. A trailing
    /// `/` is allowed; the backend appends `{base_url}/models/{model}:generateContent`.
    pub base_url: String,
    pub temperature: f32,
    pub request_timeout_secs: u64,
    /// true = omit the thinking-disabling thinkingConfig and let the model
    /// think by its own default; false = send Gemini's native channel-level
    /// minimal thinking config.
    pub thinking_enabled: bool,
}

impl GeminiConfig {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
            temperature: DEFAULT_TEMPERATURE,
            request_timeout_secs: DEFAULT_REQUEST_TIMEOUT_SECS,
            thinking_enabled: false,
        }
    }

    pub fn with_thinking_enabled(mut self, enabled: bool) -> Self {
        self.thinking_enabled = enabled;
        self
    }
}

pub struct GeminiProvider {
    config: GeminiConfig,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(config: GeminiConfig) -> Self {
        // Reuse a cached client keyed by timeout so the connection pool survives
        // across utterances instead of re-handshaking every polish. The proxy
        // switch clears the cache in net::set_use_system_proxy, so rebuild
        // here under the new policy.
        let timeout = config.request_timeout_secs;
        let no_proxy =
            crate::net::should_bypass_proxy(&config.base_url, crate::net::use_system_proxy());
        let client = crate::net::cached_client((timeout, no_proxy), || {
            let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(timeout));
            if no_proxy {
                builder = builder.no_proxy();
            }
            builder.build().unwrap_or_else(|_| reqwest::Client::new())
        });
        Self { config, client }
    }

    pub async fn polish(
        &self,
        raw_text: &str,
        mode: PolishMode,
        hotwords: &[String],
        style_system_prompt: &str,
        working_languages: &[String],
        chinese_script_preference: ChineseScriptPreference,
        output_language_preference: OutputLanguagePreference,
        front_app: Option<&str>,
        cursor_context: Option<&str>,
        prior_turns: &[(String, String)],
        edit_plan_input: bool,
    ) -> Result<String, LLMError> {
        let (system_prompt, user_prompt) = crate::prompt_compose::compose_polish_prompts_for_input(
            raw_text,
            mode,
            hotwords,
            style_system_prompt,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
            cursor_context,
            !prior_turns.is_empty(),
            edit_plan_input,
        );

        let contents = build_polish_history_contents(prior_turns, &user_prompt);
        let body = self.build_generate_body(&system_prompt, contents);
        let url = generate_content_url(&self.config.base_url, &self.config.model);

        log::info!(
            "[llm] POST {} provider=gemini model={} prior_turns={}",
            crate::net::sanitized_url_for_logs(&url),
            self.config.model,
            prior_turns.len()
        );

        let body_text = self.send_unary(&url, &body).await?;
        let raw = extract_assistant_content(&body_text)?;
        Ok(clean_polish_output(&raw))
    }

    pub async fn translate_to(
        &self,
        raw_text: &str,
        target_language: &str,
        working_languages: &[String],
        chinese_script_preference: ChineseScriptPreference,
        _output_language_preference: OutputLanguagePreference,
        front_app: Option<&str>,
    ) -> Result<String, LLMError> {
        let (system_prompt, user_prompt) = compose_translate_prompts(
            raw_text,
            target_language,
            working_languages,
            chinese_script_preference,
            front_app,
        );

        let contents = vec![user_content(&user_prompt)];
        let body = self.build_generate_body(&system_prompt, contents);
        let url = generate_content_url(&self.config.base_url, &self.config.model);

        log::info!(
            "[llm] POST {} provider=gemini model={} translate=true",
            crate::net::sanitized_url_for_logs(&url),
            self.config.model
        );

        let body_text = self.send_unary(&url, &body).await?;
        let raw = extract_assistant_content(&body_text)?;
        Ok(clean_polish_output(&raw))
    }

    pub async fn polish_streaming<F, C>(
        &self,
        raw_text: &str,
        mode: PolishMode,
        hotwords: &[String],
        style_system_prompt: &str,
        working_languages: &[String],
        chinese_script_preference: ChineseScriptPreference,
        output_language_preference: OutputLanguagePreference,
        front_app: Option<&str>,
        cursor_context: Option<&str>,
        prior_turns: &[(String, String)],
        edit_plan_input: bool,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let (system_prompt, user_prompt) = crate::prompt_compose::compose_polish_prompts_for_input(
            raw_text,
            mode,
            hotwords,
            style_system_prompt,
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
            cursor_context,
            !prior_turns.is_empty(),
            edit_plan_input,
        );
        let body = self.build_generate_body(
            &system_prompt,
            build_polish_history_contents(prior_turns, &user_prompt),
        );
        let url = stream_generate_content_url(&self.config.base_url, &self.config.model);
        self.send_streaming(&url, &body, on_delta, should_cancel)
            .await
    }

    pub async fn translate_to_streaming<F, C>(
        &self,
        raw_text: &str,
        target_language: &str,
        working_languages: &[String],
        chinese_script_preference: ChineseScriptPreference,
        _output_language_preference: OutputLanguagePreference,
        front_app: Option<&str>,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let (system_prompt, user_prompt) = compose_translate_prompts(
            raw_text,
            target_language,
            working_languages,
            chinese_script_preference,
            front_app,
        );
        let body = self.build_generate_body(&system_prompt, vec![user_content(&user_prompt)]);
        let url = stream_generate_content_url(&self.config.base_url, &self.config.model);
        self.send_streaming(&url, &body, on_delta, should_cancel)
            .await
    }

    /// Gemini Omni call with optional WAV audio in an `inlineData` part.
    /// The Omni layer encodes PCM to WAV; absent audio uses a text-only request.
    /// Both paths use the independent Omni credential namespace.
    pub(crate) async fn complete_omni(
        &self,
        system_prompt: &str,
        user_text: &str,
        wav_bytes: Option<&[u8]>,
    ) -> Result<String, LLMError> {
        let contents = omni_gemini_contents(user_text, wav_bytes);
        let body = self.build_generate_body(system_prompt, contents);
        let url = generate_content_url(&self.config.base_url, &self.config.model);

        log::info!(
            "[omni] POST {} provider=gemini model={} audio={}",
            crate::net::sanitized_url_for_logs(&url),
            self.config.model,
            wav_bytes.is_some()
        );

        let body_text = self.send_unary(&url, &body).await?;
        let raw = extract_assistant_content(&body_text)?;
        Ok(clean_polish_output(&raw))
    }

    /// Streaming answers for selection voice QA. Native Gemini SSE:
    /// `:streamGenerateContent?alt=sse`; each `data: {...}` frame carries the
    /// delta in `candidates[0].content.parts[0].text`. There is no `[DONE]`
    /// sentinel at stream end; the stream just terminates.
    pub async fn answer_chat_streaming<F, C>(
        &self,
        messages: &[QaChatMessage],
        working_languages: &[String],
        chinese_script_preference: ChineseScriptPreference,
        output_language_preference: OutputLanguagePreference,
        front_app: Option<&str>,
        on_delta: F,
        should_cancel: C,
    ) -> Result<String, LLMError>
    where
        F: Fn(&str) + Send + Sync,
        C: Fn() -> bool + Send + Sync,
    {
        let system_prompt = compose_qa_system_prompt(
            working_languages,
            chinese_script_preference,
            output_language_preference,
            front_app,
        );

        let contents = qa_messages_to_contents(messages);
        let body = self.build_generate_body(&system_prompt, contents);
        let url = stream_generate_content_url(&self.config.base_url, &self.config.model);

        log::info!(
            "[llm] POST {} provider=gemini model={} chat_turns={} stream=true",
            crate::net::sanitized_url_for_logs(&url),
            self.config.model,
            messages.len()
        );

        self.send_streaming(&url, &body, on_delta, should_cancel)
            .await
    }

    /// `generationConfig` injection: temperature + channel-level thinkingConfig.
    fn build_generate_body(&self, system_prompt: &str, contents: Vec<Value>) -> Value {
        let mut generation_config = json!({ "temperature": self.config.temperature });
        if !self.config.thinking_enabled {
            generation_config["thinkingConfig"] = disabled_thinking_config();
        }
        json!({
            "systemInstruction": system_instruction(system_prompt),
            "contents": contents,
            "generationConfig": generation_config,
        })
    }

    async fn send_unary(&self, url: &str, body: &Value) -> Result<String, LLMError> {
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json");
        if !self.config.api_key.trim().is_empty() {
            request = request.header("x-goog-api-key", self.config.api_key.as_str());
        }
        let request = request.json(body);

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => return Err(llm_error_from_reqwest(e)),
        };

        let status = response.status();
        let body_text = response.text().await.map_err(llm_error_from_reqwest)?;

        let preview_end = BODY_PREVIEW_LIMIT.min(body_text.len());
        let preview = safe_str_slice(&body_text, preview_end);
        log::info!("[llm] HTTP {} body={}", status.as_u16(), preview);

        if !status.is_success() {
            return Err(LLMError::InvalidResponse {
                status: status.as_u16(),
                body: preview.to_string(),
            });
        }

        Ok(body_text)
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
            request = request.header("x-goog-api-key", self.config.api_key.as_str());
        }
        let request = request.json(body);

        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => return Err(llm_error_from_reqwest(e)),
        };

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.map_err(llm_error_from_reqwest)?;
            let preview_end = BODY_PREVIEW_LIMIT.min(body_text.len());
            let preview = safe_str_slice(&body_text, preview_end);
            log::error!("[llm] HTTP {} body={}", status.as_u16(), preview);
            return Err(LLMError::InvalidResponse {
                status: status.as_u16(),
                body: preview.to_string(),
            });
        }

        let mut response = response;
        // Byte-level buffering — `reqwest::chunk()` may split in the middle
        // of a multi-byte UTF-8 character (CJK / emoji); calling from_utf8 on
        // each chunk independently would treat a valid SSE stream as
        // "non-utf8 SSE chunk" and fail (verified gap found by PR #398
        // pr_agent). SSE frame delimiters `\n\n` are pure ASCII (0x0A) and
        // never fall inside a multi-byte character, so locating complete
        // events by bytes and decoding whole events is always safe.
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut full_text = String::new();
        loop {
            // Same cancel flag as polish.rs streaming — break immediately on
            // user cancel / popover close instead of draining the HTTP body
            // and burning quota.
            if should_cancel() {
                log::info!("[llm] gemini stream cancelled by caller; breaking SSE loop");
                break;
            }
            let chunk_opt = response.chunk().await.map_err(llm_error_from_reqwest)?;
            let Some(chunk) = chunk_opt else { break };
            byte_buffer.extend_from_slice(&chunk);

            for event in drain_complete_sse_events(&mut byte_buffer) {
                for line in event.lines() {
                    let Some(payload) = line
                        .strip_prefix("data: ")
                        .or_else(|| line.strip_prefix("data:"))
                    else {
                        continue;
                    };
                    let payload = payload.trim();
                    if payload.is_empty() {
                        continue;
                    }
                    let v: Value = match serde_json::from_str(payload) {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!(
                                "[llm] gemini SSE parse skip: {e}; payload preview: {}",
                                safe_str_slice(payload, 80)
                            );
                            continue;
                        }
                    };
                    // Gemini SSE: candidates[0].content.parts[*].text
                    if let Some(parts) = v["candidates"][0]["content"]["parts"].as_array() {
                        for part in parts {
                            if let Some(delta) = part["text"].as_str() {
                                if !delta.is_empty() {
                                    full_text.push_str(delta);
                                    on_delta(delta);
                                }
                            }
                        }
                    }
                }
            }
        }

        log::info!(
            "[llm] HTTP 200 gemini stream done; total chars={}",
            full_text.chars().count()
        );

        if full_text.is_empty() {
            return Err(LLMError::InvalidResponse {
                status: 200,
                body: "empty stream".to_string(),
            });
        }
        Ok(full_text)
    }
}

// ─────────────────────── internal helpers ───────────────────────

fn user_content(text: &str) -> Value {
    json!({ "role": "user", "parts": [{ "text": text }] })
}

fn model_content(text: &str) -> Value {
    json!({ "role": "model", "parts": [{ "text": text }] })
}

fn system_instruction(system_prompt: &str) -> Value {
    json!({ "parts": [{ "text": system_prompt }] })
}

/// Drains all complete events from the byte buffer, delimited by SSE frame
/// separators (`\n\n` or `\r\n\r\n`); incomplete trailing bytes stay in the
/// buffer for the next chunk.
///
/// Invariant: every byte of both delimiters is ASCII (0x0A / 0x0D) and can
/// never appear inside a UTF-8 multi-byte character, so
/// 1. byte-level delimiter search is 100% safe;
/// 2. from_utf8 over the complete event range (event_start..delim_start)
///    can never fail from a chunk boundary splitting a multi-byte character;
/// 3. CRLF and LF never both match at the same offset (\r\n\r\n contains no
///    \n\n), so picking the earliest delimiter is unambiguous.
///
/// Fixes both SSE gaps reported by PR #398 pr_agent:
/// (a) the old code ran from_utf8 per network chunk and killed the stream
///     whenever CJK / emoji spanned a chunk boundary;
/// (b) the old code only recognized `\n\n`, so servers using CRLF-style
///     framing (some HTTP/2 intermediaries and CDNs normalize line endings)
///     looked like an empty stream — the docs don't mandate LF-only, so both
///     must be accepted.
fn drain_complete_sse_events(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut events = Vec::new();
    loop {
        let crlf = buffer.windows(4).position(|w| w == b"\r\n\r\n");
        let lf = buffer.windows(2).position(|w| w == b"\n\n");
        let (end, delim_len) = match (crlf, lf) {
            (Some(c), Some(l)) => {
                if c <= l {
                    (c, 4)
                } else {
                    (l, 2)
                }
            }
            (Some(c), None) => (c, 4),
            (None, Some(l)) => (l, 2),
            (None, None) => break,
        };
        let event_str = match std::str::from_utf8(&buffer[..end]) {
            Ok(s) => s.to_string(),
            Err(e) => {
                // The complete event itself is invalid UTF-8 (rare; likely
                // dirty upstream data): drop this event instead of killing
                // the stream.
                log::warn!("[llm] gemini SSE event has invalid UTF-8 (skipping): {e}");
                buffer.drain(..end + delim_len);
                continue;
            }
        };
        events.push(event_str);
        buffer.drain(..end + delim_len);
    }
    events
}

/// Contents sequence for multi-turn polish.
/// Input contract: `prior_turns` matches polish.rs (newest-first); chat
/// chronological order is oldest-first, hence `iter().rev()` here.
fn build_polish_history_contents(
    prior_turns: &[(String, String)],
    user_prompt: &str,
) -> Vec<Value> {
    let mut contents: Vec<Value> = Vec::with_capacity(prior_turns.len() * 2 + 1);
    for (raw, polished) in prior_turns.iter().rev() {
        contents.push(user_content(&crate::polish::prompts::user_prompt(raw)));
        contents.push(model_content(polished));
    }
    contents.push(user_content(user_prompt));
    contents
}

/// One user turn for a Gemini multimodal call: the text part is always
/// first, the audio part optional. `wav_bytes` holds encoded WAV file bytes,
/// sent base64 via `inlineData(audio/wav)`.
fn omni_gemini_contents(user_text: &str, wav_bytes: Option<&[u8]>) -> Vec<Value> {
    let mut parts = vec![json!({ "text": user_text })];
    if let Some(wav) = wav_bytes {
        let data = base64::engine::general_purpose::STANDARD.encode(wav);
        parts.push(json!({
            "inlineData": {
                "mimeType": "audio/wav",
                "data": data,
            }
        }));
    }
    vec![json!({ "role": "user", "parts": parts })]
}

/// QA chat messages → Gemini contents: the assistant role is renamed to
/// model. QaChatMessage.role is `"user" | "assistant"` on the polish.rs
/// OpenAI path; `assistant` maps to Gemini's `model`, everything else passes
/// through unchanged.
fn qa_messages_to_contents(messages: &[QaChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let role = if m.role == "assistant" {
                "model"
            } else {
                "user"
            };
            json!({ "role": role, "parts": [{ "text": m.content }] })
        })
        .collect()
}

/// Thinking-off / minimal-thinking request for the native Gemini channel.
///
/// OpenLess keeps no per-model adaptation table: when thinking is enabled,
/// no thinkingConfig is sent; when disabled, it sends the official
/// `thinkingBudget = 0` that expresses "no thinking". If a specific model
/// doesn't support the field or can't fully disable thinking, the Gemini API
/// itself handles that.
fn disabled_thinking_config() -> Value {
    json!({ "thinkingBudget": 0 })
}

fn generate_content_url(base_url: &str, model: &str) -> String {
    let trimmed = base_url.trim();
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        let fallback = trimmed.trim_end_matches('/');
        return format!("{fallback}/models/{model}:generateContent");
    };
    let path = url.path().trim_end_matches('/');
    url.set_path(&format!("{path}/models/{model}:generateContent"));
    url.to_string()
}

fn stream_generate_content_url(base_url: &str, model: &str) -> String {
    let trimmed = base_url.trim();
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        let fallback = trimmed.trim_end_matches('/');
        return format!("{fallback}/models/{model}:streamGenerateContent?alt=sse");
    };
    let path = url.path().trim_end_matches('/');
    url.set_path(&format!("{path}/models/{model}:streamGenerateContent"));
    let existing_query = url
        .query_pairs()
        .filter(|(key, _)| key != "alt")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in existing_query {
            query.append_pair(&key, &value);
        }
        query.append_pair("alt", "sse");
    }
    url.to_string()
}

fn extract_assistant_content(body: &str) -> Result<String, LLMError> {
    let json: Value = serde_json::from_str(body)
        .map_err(|e| LLMError::ParseError(format!("not valid JSON: {}", e)))?;
    let candidates = json
        .get("candidates")
        .and_then(|v| v.as_array())
        .ok_or_else(|| LLMError::ParseError("missing candidates array".into()))?;
    let first = candidates
        .first()
        .ok_or_else(|| LLMError::ParseError("candidates array is empty".into()))?;
    let parts = first
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .ok_or_else(|| LLMError::ParseError("missing content.parts".into()))?;
    // Concatenate all part.text. With thinking on, the model may emit several
    // segments; joining per segment avoids future single-part vs multi-part
    // differences breaking this.
    let mut buf = String::new();
    for part in parts {
        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
            buf.push_str(t);
        }
    }
    if buf.is_empty() {
        return Err(LLMError::ParseError(
            "candidates[0].content.parts[*].text 为空".into(),
        ));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_thinking_config_uses_channel_level_budget_zero() {
        assert_eq!(disabled_thinking_config(), json!({ "thinkingBudget": 0 }));
    }

    #[test]
    fn generate_content_url_handles_trailing_slash_in_base_url() {
        let a = generate_content_url("https://x/v1beta", "gemini-2.5-flash");
        let b = generate_content_url("https://x/v1beta/", "gemini-2.5-flash");
        assert_eq!(
            a,
            "https://x/v1beta/models/gemini-2.5-flash:generateContent"
        );
        assert_eq!(
            b,
            "https://x/v1beta/models/gemini-2.5-flash:generateContent"
        );
    }

    #[test]
    fn generate_content_url_preserves_query_and_fragment() {
        assert_eq!(
            generate_content_url(
                "https://example.com/v1beta?token=query-secret#client-fragment",
                "gemini-2.5-flash"
            ),
            "https://example.com/v1beta/models/gemini-2.5-flash:generateContent?token=query-secret#client-fragment"
        );
    }

    #[test]
    fn stream_generate_content_url_appends_alt_sse() {
        let a = stream_generate_content_url("https://x/v1beta", "gemini-2.5-flash");
        assert_eq!(
            a,
            "https://x/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn stream_generate_content_url_preserves_existing_query() {
        let url = stream_generate_content_url(
            "https://example.com/v1beta?token=query-secret#client-fragment",
            "gemini-2.5-flash",
        );
        assert_eq!(
            url,
            "https://example.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?token=query-secret&alt=sse#client-fragment"
        );
    }

    #[test]
    fn extract_assistant_content_concatenates_multiple_parts() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":"hello "},{"text":"world"}]}}]}"#;
        assert_eq!(extract_assistant_content(body).unwrap(), "hello world");
    }

    #[test]
    fn extract_assistant_content_empty_array_errors() {
        let body = r#"{"candidates":[]}"#;
        assert!(extract_assistant_content(body).is_err());
    }

    #[test]
    fn build_polish_history_contents_orders_oldest_to_newest_and_uses_model_role() {
        // prior_turns arrives newest-first (same contract as
        // polish.rs::build_polish_history_messages); reverse into chat
        // chronological oldest-first order for Gemini.
        // Polished history on the assistant role must map to Gemini's `model` role.
        let prior = vec![
            ("raw-newest".into(), "polished-newest".into()),
            ("raw-mid".into(), "polished-mid".into()),
            ("raw-oldest".into(), "polished-oldest".into()),
        ];
        let contents = build_polish_history_contents(&prior, "USER_NOW");
        // 3x(user/model) + 1 current user = 7
        assert_eq!(contents.len(), 7);
        assert_eq!(contents[0]["role"], "user");
        assert!(contents[0]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("raw-oldest"));
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "polished-oldest");
        assert_eq!(contents[5]["role"], "model");
        assert_eq!(contents[5]["parts"][0]["text"], "polished-newest");
        assert_eq!(contents[6]["role"], "user");
        assert_eq!(contents[6]["parts"][0]["text"], "USER_NOW");
    }

    #[test]
    fn qa_messages_assistant_role_is_remapped_to_model() {
        let messages = vec![
            QaChatMessage {
                role: "user".into(),
                content: "选区是什么意思".into(),
                selection_text: None,
            },
            QaChatMessage {
                role: "assistant".into(),
                content: "这是一段示例文本".into(),
                selection_text: None,
            },
            QaChatMessage {
                role: "user".into(),
                content: "继续问".into(),
                selection_text: None,
            },
        ];
        let contents = qa_messages_to_contents(&messages);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[2]["role"], "user");
    }

    #[test]
    fn build_generate_body_disabled_includes_channel_level_thinking_budget_zero() {
        let cfg = GeminiConfig::new("k", "any-gemini-model", "https://x/v1beta");
        let provider = GeminiProvider::new(cfg);
        let body = provider.build_generate_body("SYS", vec![user_content("hi")]);
        assert_eq!(
            body["generationConfig"]["thinkingConfig"],
            json!({ "thinkingBudget": 0 })
        );
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "SYS");
        assert_eq!(body["contents"][0]["role"], "user");
    }

    #[test]
    fn build_generate_body_thinking_enabled_omits_thinking_config() {
        let cfg = GeminiConfig::new("k", "gemini-2.5-flash", "https://x/v1beta")
            .with_thinking_enabled(true);
        let provider = GeminiProvider::new(cfg);
        let body = provider.build_generate_body("SYS", vec![user_content("hi")]);
        assert!(
            body["generationConfig"].get("thinkingConfig").is_none(),
            "开启思考模式时不下发关闭思考的 thinkingConfig"
        );
    }

    #[test]
    fn drain_complete_sse_events_splits_full_event_at_delimiter() {
        let mut buf = b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: incompl".to_vec();
        let events = drain_complete_sse_events(&mut buf);
        assert_eq!(events, vec!["data: {\"a\":1}", "data: {\"b\":2}"]);
        // The incomplete tail stays in the buffer for the next chunk.
        assert_eq!(buf, b"data: incompl");
    }

    #[test]
    fn drain_complete_sse_events_handles_multibyte_split_across_chunks() {
        // Regression for the PR #398 pr_agent UTF-8 SSE gap:
        // The sample text is e4 bd a0 e5 a5 bd in UTF-8 (6 bytes). Simulate
        // reqwest::chunk() cutting after e4 bd (a third of the way into the
        // first character). The old code immediately ran from_utf8(&chunk),
        // errored, and killed the whole stream; the new code accumulates
        // bytes until a complete event (\n\n) arrives, then decodes — must
        // be lossless.
        let event_bytes = b"data: {\"text\":\"\xe4\xbd\xa0\xe5\xa5\xbd\"}\n\n";
        let cut = 17; // cuts after e4 bd, before a0 — inside a multi-byte character
        assert!(cut < event_bytes.len() && event_bytes[cut] == 0xa0);

        let mut buf = Vec::new();
        buf.extend_from_slice(&event_bytes[..cut]);
        let events_round_1 = drain_complete_sse_events(&mut buf);
        assert!(
            events_round_1.is_empty(),
            "尚未收到 \\n\\n，不能产生 event；同时 buffer 不应因半截多字节字符报错"
        );

        buf.extend_from_slice(&event_bytes[cut..]);
        let events_round_2 = drain_complete_sse_events(&mut buf);
        assert_eq!(events_round_2.len(), 1, "拼齐后应产生 1 个完整 event");
        assert!(
            events_round_2[0].contains("你好"),
            "中文必须在拼齐后完好解出；旧实现这里会丢字"
        );
        assert!(buf.is_empty(), "处理完后 buffer 应清空");
    }

    #[test]
    fn drain_complete_sse_events_handles_crlf_delimiter() {
        // Regression for the PR #398 pr_agent advisory: some servers/CDNs
        // delimit SSE frames with \r\n\r\n, and the old implementation only
        // recognized \n\n, treating the whole stream as empty. The new one
        // searches \r\n\r\n and \n\n byte-wise and takes the earliest offset.
        // Rust str::lines() strips \r inside an event, so line handling needs
        // no change.
        let mut buf = b"data: {\"a\":1}\r\n\r\ndata: {\"b\":2}\r\n\r\n".to_vec();
        let events = drain_complete_sse_events(&mut buf);
        assert_eq!(events, vec!["data: {\"a\":1}", "data: {\"b\":2}"]);
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_complete_sse_events_picks_earliest_delimiter_when_mixed() {
        // The same buffer holds both LF- and CRLF-style frames — process in
        // order of appearance so no event is lost.
        let mut buf = b"data: lf-event\n\ndata: crlf-event\r\n\r\nrest".to_vec();
        let events = drain_complete_sse_events(&mut buf);
        assert_eq!(events, vec!["data: lf-event", "data: crlf-event"]);
        assert_eq!(buf, b"rest");
    }

    #[test]
    fn drain_complete_sse_events_skips_invalid_utf8_event_without_failing_stream() {
        // Edge case: the complete event's own byte sequence is invalid UTF-8
        // (dirty upstream data). The old implementation's `?` would fail and
        // kill the stream; the new one degrades to warn + skip.
        let mut buf: Vec<u8> = b"data: ok\n\n".to_vec();
        buf.extend_from_slice(&[0xff, 0xfe, b'\n', b'\n']); // invalid event
        buf.extend_from_slice(b"data: ok2\n\n");
        let events = drain_complete_sse_events(&mut buf);
        assert_eq!(events, vec!["data: ok", "data: ok2"]);
        assert!(buf.is_empty());
    }
}
