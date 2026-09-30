//! Batch Whisper ASR client — collects PCM in a buffer, then POSTs a WAV file
//! to any OpenAI-compatible `/audio/transcriptions` endpoint on session end.

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

const PCM_SAMPLE_RATE_HZ: u64 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;

/// Safe char-count upper bound for Whisper's `prompt` parameter.
///
/// The OpenAI / Groq Audio Transcriptions APIs accept `prompt` up to 244
/// tokens. The BPE tokenizer's chars-per-token varies by language: English
/// ~4 chars/token, Japanese and Chinese ~1 char/token at worst. Cap at 240
/// chars so CJK users fit safely.
pub const PROMPT_CHAR_BUDGET: usize = 240;

/// Separator (ASCII). Stable in Whisper's tokenizer for every language.
const PROMPT_SEPARATOR: &str = ", ";

/// Default endpoint and model for the ZenMux aggregator (issue #837). Matches
/// the frontend `ASR_PRESETS` `zenmux` entry; `read_whisper_credentials`
/// falls back here when active is zenmux and the user left it empty.
pub const ZENMUX_DEFAULT_ENDPOINT: &str = "https://zenmux.ai/api/v1";
pub const ZENMUX_DEFAULT_MODEL: &str = "qwen/qwen3-asr-flash";

/// `/audio/transcriptions` request-body encoding is decided by the shared Core; Tauri only implements transport.
pub use crate::provider_rules::AsrRequestFormat;

pub struct WhisperBatchASR {
    api_key: String,
    base_url: String,
    model: String,
    /// Optional prompt (vocabulary hints etc.). Never send empty or
    /// whitespace-only prompts. `None` = no prompt (existing behavior).
    prompt: Option<String>,
    /// For providers that cap file length despite OpenAI compatibility. None sends in one shot as before.
    max_chunk_duration_ms: Option<u64>,
    /// Request `response_format=verbose_json` so per-segment metadata
    /// (no_speech_prob / avg_logprob / compression_ratio) can filter
    /// hallucinations. OpenAI / Groq Whisper support it. SenseVoice /
    /// TeleSpeech etc. (SiliconFlow) have no response_format parameter, so
    /// keep false and send plain `json` to avoid breaking them.
    verbose_json: bool,
    /// Request-body encoding. Defaults to `Multipart`; OpenRouter uses `OpenRouterJson`.
    request_format: AsrRequestFormat,
    /// ZenMux `language` field (ISO 639-1, e.g. `zh`). None = omit so the
    /// server auto-detects. Only applies to `ZenMuxJson` encoding.
    language: Option<String>,
    /// ZenMux `enable_itn` field (number/unit normalization). Defaults to
    /// true, matching ZenMux docs and Chinese ASR expectations; only applies
    /// to `ZenMuxJson` encoding.
    enable_itn: bool,
    /// First-class `hotwords` parameter (JSON-array string). StepFun and
    /// similar providers ignore `prompt` silently but expose a dedicated
    /// hotwords field — the dictionary only takes effect through it.
    /// Empty = omit.
    hotwords: Vec<String>,
    /// Custom endpoint path (default `/audio/transcriptions`; MiniMax etc. use `/speech_to_text`).
    endpoint_path: Option<String>,
    buffer: Mutex<Vec<u8>>,
}

impl WhisperBatchASR {
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        prompt: Option<String>,
        max_chunk_duration_ms: Option<u64>,
        verbose_json: bool,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            prompt,
            max_chunk_duration_ms,
            verbose_json,
            request_format: AsrRequestFormat::Multipart,
            language: None,
            enable_itn: true,
            hotwords: Vec::new(),
            endpoint_path: None,
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Sets a custom endpoint path (e.g. `"/speech_to_text"`).
    pub fn with_endpoint_path(mut self, path: impl Into<String>) -> Self {
        self.endpoint_path = Some(path.into());
        self
    }

    /// Sets the request-body encoding (default `Multipart`). OpenRouter needs
    /// `OpenRouterJson`. A builder avoids changing the existing `new()` call-site signatures.
    pub fn with_request_format(mut self, request_format: AsrRequestFormat) -> Self {
        self.request_format = request_format;
        self
    }

    /// Sets the first-class hotwords list (default empty = omit). Only applies
    /// to Multipart encoding; the `prompt` vs hotwords choice is made by wiring
    /// (see coordinator's `whisper_uses_hotwords`).
    pub fn with_hotwords(mut self, hotwords: Vec<String>) -> Self {
        self.hotwords = hotwords;
        self
    }

    /// Sets the ZenMux `language` field (ISO 639-1 code). None = omit
    /// (auto-detect). Only applies to `ZenMuxJson` encoding.
    pub fn with_language(mut self, language: Option<String>) -> Self {
        self.language = language;
        self
    }

    /// Sets the ZenMux `enable_itn` (number normalization) switch, default
    /// true. Only applies to `ZenMuxJson` encoding.
    pub fn with_enable_itn(mut self, enable_itn: bool) -> Self {
        self.enable_itn = enable_itn;
        self
    }

    /// Stop collecting audio, encode the buffer as WAV, and POST to the
    /// Whisper transcriptions endpoint.
    ///
    /// On failure **keep** the PCM buffer so the caller can retry or at least
    /// record a failure in history; current buffered audio duration in ms.
    /// The coordinator reads this before transcribe() to compute the
    /// Whisper / OpenRouter dynamic timeout. Does not consume the buffer.
    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(&self.buffer.lock())
    }

    /// The buffer must survive a failed transcribe: draining it up front
    /// would lose the recording on credential or network errors.
    pub async fn transcribe(&self) -> Result<RawTranscript> {
        // Clone instead of take: ~30s of 16 kHz 16-bit audio is ~960 KB,
        // called once per session — acceptable.
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm).await;
        // Clear the buffer only on success. On failure the PCM stays: the
        // coordinator gets Err, but a re-triggered stop can send again.
        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(&self, pcm: &[u8]) -> Result<RawTranscript> {
        let duration_ms = pcm_duration_ms(pcm);
        let chunks = split_pcm_by_duration(pcm, self.max_chunk_duration_ms);
        let mut texts = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            texts.push(self.transcribe_chunk(chunk).await?);
        }

        Ok(RawTranscript {
            text: join_transcript_chunks(&texts),
            duration_ms,
        })
    }

    async fn transcribe_chunk(&self, pcm: &[u8]) -> Result<String> {
        let samples: Vec<i16> = pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let url = transcription_url(&self.base_url, self.endpoint_path.as_deref())?;
        let client = crate::net::http();

        let request = match self.request_format {
            AsrRequestFormat::Multipart => {
                let wav_part = reqwest::multipart::Part::bytes(wav)
                    .file_name("audio.wav")
                    .mime_str("audio/wav")
                    .context("set MIME type")?;
                let mut form = reqwest::multipart::Form::new()
                    .part("file", wav_part)
                    .text("model", self.model.clone());

                // Only verbose_json-capable providers (OpenAI / Groq) get a
                // request for segment metadata, with temperature pinned to 0.
                // Don't send it to providers without support (SiliconFlow
                // SenseVoice / TeleSpeech etc.) to avoid 4xx on unknown params.
                if self.verbose_json {
                    form = form
                        .text("response_format", "verbose_json")
                        .text("temperature", "0");
                }

                // Never send an empty `prompt`: some OpenAI-compatible
                // implementations error on it (Groq tolerates it, skip
                // defensively). `trim()` also excludes whitespace-only values.
                if let Some(prompt) = self.prompt.as_ref() {
                    let trimmed = prompt.trim();
                    if !trimmed.is_empty() {
                        form = form.text("prompt", trimmed.to_string());
                    }
                }

                // First-class hotwords (StepFun shape: a parseable JSON array
                // string like `["hotword1","hotword2"]`). Filter out blank
                // entries before encoding; if all are blank, omit the field.
                let hotwords: Vec<&str> = self
                    .hotwords
                    .iter()
                    .map(|w| w.trim())
                    .filter(|w| !w.is_empty())
                    .collect();
                if !hotwords.is_empty() {
                    if let Ok(encoded) = serde_json::to_string(&hotwords) {
                        form = form.text("hotwords", encoded);
                    }
                }

                // `openai-compatible` allows an empty API key (LAN endpoints
                // without auth): omit the Authorization header so an empty
                // Bearer doesn't get rejected with 401.
                let mut request = client.post(&url);
                if !self.api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", self.api_key));
                }
                request.multipart(form)
            }
            AsrRequestFormat::OpenRouterJson => {
                // OpenRouter /audio/transcriptions: application/json with
                // standard base64 (padded) audio. Omit the multipart-only
                // prompt/response_format fields to avoid 4xx on unknown
                // fields; verbose_json stays off for this protocol.
                let body = serde_json::json!({
                    "model": self.model,
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                        "format": "wav",
                    },
                });
                let mut request = client.post(&url);
                if !self.api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", self.api_key));
                }
                request.json(&body)
            }
            AsrRequestFormat::ZenMuxJson => {
                // ZenMux /audio/transcriptions (issue #837): application/json
                // with standard base64 audio. Language follows the OpenLess
                // working-language mapping; enable_itn comes from the settings
                // toggle and is always sent explicitly. No multipart-only fields.
                let mut body = serde_json::json!({
                    "model": self.model,
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                        "format": "wav",
                    },
                    "enable_itn": self.enable_itn,
                });
                if let Some(language) = self.language.as_ref() {
                    if !language.trim().is_empty() {
                        body["language"] = serde_json::Value::String(language.trim().to_string());
                    }
                }
                let mut request = client.post(&url);
                if !self.api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", self.api_key));
                }
                request.json(&body)
            }
        };

        let resp = request
            .send()
            .await
            .context("Whisper HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Whisper API error {}: {}", status, body);
        }

        let json: serde_json::Value = resp.json().await.context("parse Whisper response")?;
        if let Some(base_resp) = json.get("base_resp") {
            let status_code = base_resp
                .get("status_code")
                .and_then(|c| c.as_i64())
                .unwrap_or(0);
            if status_code != 0 {
                let msg = base_resp
                    .get("status_msg")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                anyhow::bail!("MiniMax API error {}: {}", status_code, msg);
            }
        }
        if self.verbose_json {
            // verbose_json: assemble the body from segment metadata, dropping
            // hallucinations. Falls back to plain text when segments are absent.
            Ok(extract_confident_text(&json))
        } else {
            // GLM-ASR etc. occasionally return placeholder text consisting
            // only of `#` (or `##`...) for silent/weak audio (issue #787).
            // Normalize to an empty transcript so the existing empty-transcript
            // guard keeps placeholders out of user input.
            let text = json["text"].as_str().unwrap_or("").trim();
            if is_placeholder_heading(text) {
                Ok(String::new())
            } else {
                Ok(text.to_string())
            }
        }
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl super::AudioConsumer for WhisperBatchASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// Assemble verbose_json output, dropping hallucinated segments.
///
/// Whisper generates plausible-but-unsaid text on silent / quiet / noisy
/// audio (known hallucination defect): lead-in silence or mic noise becomes
/// stray words. Each verbose_json segment carries `no_speech_prob` /
/// `avg_logprob` / `compression_ratio`; use them to drop segments that are
/// clearly not real speech.
///
/// Drop when any of:
/// - `no_speech_prob > 0.6` and `avg_logprob < -0.5`: high silence
///   probability with low confidence, silence turned into words.
/// - `compression_ratio > 2.4`: the same phrase repeated (Whisper's standard
///   threshold).
/// - `avg_logprob < -1.0`: very low confidence, noise turned into words.
///
/// Thresholds are conservative because dropping real speech is the worst
/// outcome. Without a `segments` field (e.g. the provider ignored
/// verbose_json) fall back to `text` as before. Missing metadata fields
/// count as "keep" (unwrap_or defaults), so providers without these metrics
/// pass through unchanged.
fn extract_confident_text(json: &serde_json::Value) -> String {
    let Some(segments) = json.get("segments").and_then(|s| s.as_array()) else {
        let text = json["text"].as_str().unwrap_or("").trim();
        if is_placeholder_heading(text) {
            return String::new();
        }
        return text.to_string();
    };

    let mut kept = String::new();
    for seg in segments {
        let text = seg.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if text.trim().is_empty() {
            continue;
        }
        let no_speech = seg
            .get("no_speech_prob")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let avg_logprob = seg
            .get("avg_logprob")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let compression = seg
            .get("compression_ratio")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        let is_hallucination =
            (no_speech > 0.6 && avg_logprob < -0.5) || compression > 2.4 || avg_logprob < -1.0;
        if is_hallucination {
            log::warn!(
                "[whisper] 丢弃疑似幻听段落: no_speech={:.2} avg_logprob={:.2} compression={:.2} text={:?}",
                no_speech,
                avg_logprob,
                compression,
                text.trim()
            );
            continue;
        }
        kept.push_str(text);
    }

    let kept = kept.trim().to_string();
    if kept.is_empty() {
        // Every segment was judged a hallucination (the clip is almost all
        // silence). Falling back to the raw text would reintroduce it, so
        // return an empty string; callers treat that as "nothing said".
        return String::new();
    }
    kept
}

/// Detect transcripts that are pure `#` placeholder text.
///
/// GLM-ASR (zhipu) occasionally transcribes an entire silent / quiet clip as
/// a single `#` (or `##`, `###`...) — degenerate placeholder output, not a
/// real transcript (issue #787). Only match text made up entirely of one or
/// more `#`: real content containing other characters (`C#`, `# hello`) is
/// unaffected. Input is `trim()`ed first; surrounding whitespace doesn't
/// affect the check.
fn is_placeholder_heading(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|c| c == '#')
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    super::pcm::pcm_duration_ms(pcm)
}

pub fn split_pcm_by_duration(pcm: &[u8], max_chunk_duration_ms: Option<u64>) -> Vec<&[u8]> {
    let Some(max_chunk_duration_ms) = max_chunk_duration_ms else {
        return vec![pcm];
    };
    if max_chunk_duration_ms == 0 {
        return vec![pcm];
    }

    let samples_per_chunk = PCM_SAMPLE_RATE_HZ * max_chunk_duration_ms / 1000;
    let bytes_per_chunk = samples_per_chunk as usize * PCM_BYTES_PER_SAMPLE;
    if bytes_per_chunk == 0 || pcm.len() <= bytes_per_chunk {
        return vec![pcm];
    }

    pcm.chunks(bytes_per_chunk).collect()
}

fn transcription_url(base_url: &str, endpoint_path: Option<&str>) -> Result<String> {
    let parsed = reqwest::Url::parse(base_url.trim()).context("parse Whisper base URL")?;
    let mut url = parsed.clone();
    let path = parsed.path().trim_end_matches('/');
    let next_path = if let Some(target) = endpoint_path {
        let target = target.trim();
        if path.ends_with(target) {
            path.to_string()
        } else {
            let prefix = path
                .strip_suffix("/audio/transcriptions")
                .or_else(|| path.strip_suffix("/chat/completions"))
                .unwrap_or(path);
            format!(
                "{prefix}{}",
                if target.starts_with('/') {
                    target.to_string()
                } else {
                    format!("/{target}")
                }
            )
        }
    } else if path.ends_with("/audio/transcriptions") || path.ends_with("/speech_to_text") {
        path.to_string()
    } else if path.ends_with("/audio") {
        format!("{path}/transcriptions")
    } else if let Some(prefix) = path.strip_suffix("/chat/completions") {
        format!("{prefix}/audio/transcriptions")
    } else if parsed.host_str().is_some_and(|host| {
        ["minimaxi.com", "minimax.chat"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    }) {
        format!("{path}/speech_to_text")
    } else {
        format!("{path}/audio/transcriptions")
    };
    url.set_path(&next_path);
    Ok(url.to_string())
}

pub fn join_transcript_chunks(chunks: &[String]) -> String {
    let mut joined = String::new();
    for chunk in chunks.iter().map(|chunk| chunk.trim()) {
        if chunk.is_empty() {
            continue;
        }
        if needs_chunk_separator(&joined, chunk) {
            joined.push(' ');
        }
        joined.push_str(chunk);
    }
    joined
}

fn needs_chunk_separator(current: &str, next: &str) -> bool {
    let Some(prev) = current.chars().last() else {
        return false;
    };
    let Some(first) = next.chars().next() else {
        return false;
    };

    if is_closing_punctuation(first) || is_opening_punctuation(prev) {
        return false;
    }
    if is_cjk(prev) && (is_cjk(first) || is_opening_punctuation(first)) {
        return false;
    }
    if is_cjk(first) && is_closing_punctuation(prev) {
        return false;
    }
    if is_cjk_punctuation(prev) && is_cjk(first) {
        return false;
    }
    true
}

fn is_opening_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '(' | '[' | '{' | '"' | '\'' | '（' | '「' | '『' | '《' | '“' | '‘'
    )
}

fn is_closing_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ',' | '.'
            | '!'
            | '?'
            | ':'
            | ';'
            | ')'
            | ']'
            | '}'
            | '"'
            | '\''
            | '，'
            | '。'
            | '、'
            | '！'
            | '？'
            | '：'
            | '；'
            | '）'
            | '」'
            | '』'
            | '》'
            | '”'
            | '’'
            | '…'
    )
}

fn is_cjk_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '，' | '。'
            | '、'
            | '！'
            | '？'
            | '：'
            | '；'
            | '（'
            | '）'
            | '「'
            | '」'
            | '『'
            | '』'
            | '《'
            | '》'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '…'
            | '—'
    )
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
    )
}

/// Build the Whisper `prompt` parameter from the enabled user-dictionary phrases.
///
/// Whisper accepts vocabulary hints / style context via `prompt`: it curbs
/// spelling drift for proper nouns and jargon and biases the ASR stage
/// toward correct spellings (including kanji choice). The dictionary
/// previously only reached Volcengine ASR and the polish LLM, not
/// Whisper-compatible providers (whisper / siliconflow / zhipu / groq); this
/// function delivers the same entries to Whisper.
///
/// # Contract
///
/// - Whitespace-only phrases are skipped
/// - Entries are joined with `, `
/// - A trailing `.` marks "end of sentence" so the model doesn't treat the
///   prompt as a continuation and mix it into the transcript head
/// - Entries exceeding `PROMPT_CHAR_BUDGET` are **skipped** and iteration
///   continues (no mid-way break), so one long leading entry cannot discard
///   every later one; entry order is preserved and fit is maximized
/// - Returns `None` for empty input or zero valid phrases, so callers need
///   not distinguish "no prompt" from an empty prompt
///
/// Entries that don't fit the budget are dropped **silently**: the user sees
/// them in the vocabulary and assumes they work, but they never reach the
/// ASR. Log one line when that happens so it's diagnosable.
///
/// This runs on every dictation, so log only when the dropped set **changed**;
/// `app` is pinned to Info level (`lib.rs`), so debug would be invisible.
fn log_dropped_phrases_when_changed(included: &[&str], dropped: &[&str]) {
    static LAST_DROPPED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

    let fingerprint = (!dropped.is_empty()).then(|| dropped.join(", "));
    let Ok(mut last) = LAST_DROPPED.lock() else {
        return;
    };
    if *last == fingerprint {
        return;
    }
    *last = fingerprint;
    if dropped.is_empty() {
        return;
    }
    log::info!(
        "[asr-vocab] prompt budget {} chars: kept {} phrase(s), dropped {}: {:?}",
        PROMPT_CHAR_BUDGET,
        included.len(),
        dropped.len(),
        dropped
    );
}

pub fn build_prompt_from_phrases(phrases: &[String]) -> Option<String> {
    let mut included: Vec<&str> = Vec::new();
    let mut dropped: Vec<&str> = Vec::new();
    let mut total_chars: usize = 0;

    for phrase in phrases {
        let trimmed = phrase.trim();
        if trimmed.is_empty() {
            continue;
        }
        let phrase_chars = trimmed.chars().count();
        let added = if included.is_empty() {
            phrase_chars
        } else {
            PROMPT_SEPARATOR.chars().count() + phrase_chars
        };
        // Reserve one char for the trailing ".".
        if total_chars + added + 1 > PROMPT_CHAR_BUDGET {
            dropped.push(trimmed);
            continue;
        }
        included.push(trimmed);
        total_chars += added;
    }

    log_dropped_phrases_when_changed(&included, &dropped);

    if included.is_empty() {
        return None;
    }
    let mut s = included.join(PROMPT_SEPARATOR);
    s.push('.');
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::AudioConsumer;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn build_prompt_returns_none_for_empty_input() {
        assert_eq!(build_prompt_from_phrases(&[]), None);
    }

    #[test]
    fn build_prompt_returns_none_when_all_phrases_blank() {
        let phrases = vec!["".to_string(), "   ".to_string(), "\t\n".to_string()];
        assert_eq!(build_prompt_from_phrases(&phrases), None);
    }

    #[test]
    fn build_prompt_single_phrase() {
        let phrases = vec!["梁山泊".to_string()];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊.".to_string())
        );
    }

    #[test]
    fn build_prompt_joins_with_comma_and_appends_period() {
        let phrases = vec![
            "梁山泊".to_string(),
            "片沼ほとり".to_string(),
            "TRC".to_string(),
        ];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊, 片沼ほとり, TRC.".to_string())
        );
    }

    #[test]
    fn build_prompt_trims_each_phrase() {
        let phrases = vec!["  梁山泊  ".to_string(), "\tTRC\n".to_string()];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊, TRC.".to_string())
        );
    }

    #[test]
    fn build_prompt_skips_blank_entries_in_middle() {
        let phrases = vec![
            "alpha".to_string(),
            "".to_string(),
            "   ".to_string(),
            "beta".to_string(),
        ];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("alpha, beta.".to_string())
        );
    }

    #[test]
    fn build_prompt_truncates_overflow_but_keeps_short_entries_after_long_one() {
        // A 250-char leading phrase exceeds the budget alone and is skipped;
        // the following short entries are kept. Verifies the no-mid-break contract.
        let long = "あ".repeat(250);
        let phrases = vec![long.clone(), "梁山泊".to_string(), "TRC".to_string()];
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(!prompt.contains(&long), "long phrase must be dropped");
        assert!(prompt.contains("梁山泊"));
        assert!(prompt.contains("TRC"));
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn build_prompt_respects_char_budget() {
        // 6 chars x 50 entries = 300 chars (more with separators) -> overflow entries are dropped.
        let phrases: Vec<String> = (0..50).map(|i| format!("word{:02}", i)).collect();
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(
            prompt.chars().count() <= PROMPT_CHAR_BUDGET,
            "prompt length {} exceeds budget {}",
            prompt.chars().count(),
            PROMPT_CHAR_BUDGET
        );
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn build_prompt_includes_first_entries_when_truncating_in_order() {
        // Order guarantee: earliest entries fill first; later ones drop.
        let phrases: Vec<String> = (0..100).map(|i| format!("entry{:03}", i)).collect();
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(prompt.contains("entry000"));
        assert!(prompt.contains("entry001"));
        // 100 entries x 8+ chars surely exceeds the budget -> the tail is dropped
        assert!(!prompt.contains("entry099"));
    }

    #[test]
    fn split_pcm_by_duration_keeps_default_as_single_chunk() {
        let pcm = vec![0u8; 96_000];
        assert_eq!(split_pcm_by_duration(&pcm, None), vec![pcm.as_slice()]);
    }

    #[test]
    fn split_pcm_by_duration_uses_sample_boundaries() {
        let pcm = vec![0u8; 32_000 * 65];
        let chunks = split_pcm_by_duration(&pcm, Some(30_000));

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 32_000 * 30);
        assert_eq!(chunks[1].len(), 32_000 * 30);
        assert_eq!(chunks[2].len(), 32_000 * 5);
    }

    #[test]
    fn split_pcm_by_duration_zero_limit_falls_back_to_single_chunk() {
        let pcm = vec![0u8; 96_000];
        assert_eq!(split_pcm_by_duration(&pcm, Some(0)), vec![pcm.as_slice()]);
    }

    #[test]
    fn transcription_url_accepts_base_audio_or_full_endpoint() {
        assert_eq!(
            transcription_url("https://open.bigmodel.cn/api/paas/v4", None).unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions"
        );
        assert_eq!(
            transcription_url("https://open.bigmodel.cn/api/paas/v4/audio", None).unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions"
        );
        assert_eq!(
            transcription_url(
                "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
                None
            )
            .unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions"
        );
        assert_eq!(
            transcription_url(
                "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions?api-version=2026-01-01",
                None
            )
            .unwrap(),
            "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions?api-version=2026-01-01"
        );
        // MiniMax & speech_to_text tests
        assert_eq!(
            transcription_url("https://api.minimaxi.com/v1", None).unwrap(),
            "https://api.minimaxi.com/v1/speech_to_text"
        );
        assert_eq!(
            transcription_url("https://api.minimax.chat/v1", None).unwrap(),
            "https://api.minimax.chat/v1/speech_to_text"
        );
        assert_eq!(
            transcription_url("https://custom-proxy.com/v1/speech_to_text", None).unwrap(),
            "https://custom-proxy.com/v1/speech_to_text"
        );
        assert_eq!(
            transcription_url("https://custom-proxy.com/v1", Some("/speech_to_text")).unwrap(),
            "https://custom-proxy.com/v1/speech_to_text"
        );
        assert_eq!(
            transcription_url(
                "https://custom-proxy.com/v1/speech_to_text",
                Some("/speech_to_text")
            )
            .unwrap(),
            "https://custom-proxy.com/v1/speech_to_text"
        );
    }

    #[test]
    fn minimax_routing_does_not_capture_unrelated_custom_endpoints() {
        for endpoint in [
            "https://notminimaxi.com/v1",
            "https://minimax.proxy.example/v1",
            "https://proxy.example/minimax/v1",
        ] {
            assert_eq!(
                transcription_url(endpoint, None).unwrap(),
                format!("{endpoint}/audio/transcriptions")
            );
        }
        assert_eq!(
            transcription_url(
                "https://proxy.example/v1/audio/transcriptions",
                Some("/speech_to_text")
            )
            .unwrap(),
            "https://proxy.example/v1/speech_to_text"
        );
    }

    #[test]
    fn join_transcript_chunks_skips_empty_chunks() {
        let chunks = vec![" hello ".to_string(), "".to_string(), "world".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "hello world");
    }

    #[test]
    fn join_transcript_chunks_keeps_cjk_together() {
        let chunks = vec!["你好".to_string(), "世界".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "你好世界");
    }

    #[test]
    fn join_transcript_chunks_separates_mixed_script_boundaries() {
        let chunks = vec!["中文".to_string(), "English".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "中文 English");

        let chunks = vec!["OpenLess".to_string(), "中文".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "OpenLess 中文");
    }

    #[test]
    fn join_transcript_chunks_handles_punctuation_boundaries() {
        let chunks = vec!["hello,".to_string(), "world".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "hello, world");

        let chunks = vec!["hello".to_string(), ",".to_string(), "world".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "hello, world");

        let chunks = vec!["foo.".to_string(), "bar".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "foo. bar");

        let chunks = vec!["(".to_string(), "hello".to_string(), ")".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "(hello)");
    }

    #[test]
    fn join_transcript_chunks_handles_cjk_punctuation_boundaries() {
        let chunks = vec!["你好".to_string(), "，世界".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "你好，世界");

        let chunks = vec!["中文".to_string(), "。".to_string(), "下一句".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "中文。下一句");

        let chunks = vec!["他说".to_string(), "：".to_string(), "你好".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "他说：你好");

        let chunks = vec!["中文。".to_string(), "OpenAI".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "中文。 OpenAI");

        let chunks = vec!["「".to_string(), "中文".to_string(), "」".to_string()];
        assert_eq!(join_transcript_chunks(&chunks), "「中文」");
    }

    #[test]
    fn extract_confident_text_drops_hallucinated_segment() {
        let json = serde_json::json!({
            "text": "本当の発話 幻聴",
            "segments": [
                {"text": "本当の発話", "no_speech_prob": 0.01, "avg_logprob": -0.2, "compression_ratio": 1.2},
                {"text": "幻聴", "no_speech_prob": 0.9, "avg_logprob": -0.8, "compression_ratio": 1.1},
            ]
        });
        assert_eq!(extract_confident_text(&json), "本当の発話");
    }

    #[test]
    fn extract_confident_text_keeps_all_confident_segments() {
        let json = serde_json::json!({
            "text": "ignored",
            "segments": [
                {"text": "前半", "no_speech_prob": 0.0, "avg_logprob": -0.1, "compression_ratio": 1.0},
                {"text": "後半", "no_speech_prob": 0.0, "avg_logprob": -0.2, "compression_ratio": 1.0},
            ]
        });
        assert_eq!(extract_confident_text(&json), "前半後半");
    }

    #[test]
    fn extract_confident_text_falls_back_to_text_without_segments() {
        let json = serde_json::json!({ "text": "  素の文字起こし  " });
        assert_eq!(extract_confident_text(&json), "素の文字起こし");
    }

    #[test]
    fn extract_confident_text_missing_metrics_keeps_segment() {
        // When the provider returns no metrics, "keep" = leave the segment as-is (harmless no-op).
        let json = serde_json::json!({
            "text": "x",
            "segments": [ {"text": "保留される"} ]
        });
        assert_eq!(extract_confident_text(&json), "保留される");
    }

    #[test]
    fn placeholder_heading_detects_pure_hash_runs_only() {
        // issue #787: GLM-ASR occasionally returns placeholder text of only `#`.
        assert!(is_placeholder_heading("#"));
        assert!(is_placeholder_heading("##"));
        assert!(is_placeholder_heading("###"));
        assert!(is_placeholder_heading("  ##  "));
        // Real content with other characters is unaffected.
        assert!(!is_placeholder_heading("C#"));
        assert!(!is_placeholder_heading("# 你好"));
        assert!(!is_placeholder_heading("#hash"));
        // Empty / whitespace input doesn't match.
        assert!(!is_placeholder_heading(""));
        assert!(!is_placeholder_heading("   "));
    }

    #[tokio::test]
    async fn single_placeholder_chunk_transcribes_to_empty() {
        // A single-chunk response of `#` normalizes to an empty transcript.
        for text in ["#", "##", "###"] {
            let (base_url, server) = start_whisper_test_server(vec![text]);
            let asr = WhisperBatchASR::new(
                "key".to_string(),
                base_url,
                "model".to_string(),
                None,
                None,
                false,
            );
            let pcm = vec![0u8; 32_000 * 2];
            asr.consume_pcm_chunk(&pcm);

            let transcript = asr.transcribe().await.unwrap();
            assert_eq!(transcript.text, "");
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn placeholder_chunk_is_dropped_when_joining() {
        // A placeholder chunk among real chunks is dropped; real chunks are kept.
        let (base_url, server) = start_whisper_test_server(vec!["你好", "#", "世界"]);
        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "model".to_string(),
            None,
            Some(30_000),
            false,
        );
        let pcm = vec![0u8; 32_000 * 65];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "你好世界");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn all_placeholder_chunks_transcribe_to_empty() {
        // All chunks are `#` -> the whole transcript is empty (exercises the
        // coordinator's empty-transcript guard).
        let (base_url, server) = start_whisper_test_server(vec!["#", "##", "###"]);
        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "model".to_string(),
            None,
            Some(30_000),
            false,
        );
        let pcm = vec![0u8; 32_000 * 65];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn transcribe_posts_single_request_without_chunk_limit() {
        let (base_url, server) = start_whisper_test_server(vec!["one"]);
        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "model".to_string(),
            None,
            None,
            false,
        );
        let pcm = vec![0u8; 32_000 * 65];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "one");
        assert_eq!(transcript.duration_ms, 65_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn transcribe_splits_requests_when_chunk_limit_is_set() {
        let (base_url, server) = start_whisper_test_server(vec!["你好", "world", "尾"]);
        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "model".to_string(),
            None,
            Some(30_000),
            false,
        );
        let pcm = vec![0u8; 32_000 * 65];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "你好 world 尾");
        assert_eq!(transcript.duration_ms, 65_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn openrouter_format_posts_json_with_base64_audio() {
        // issue #582: OpenRouterJson posts application/json with
        // input_audio.data (base64) instead of multipart; the response is
        // still parsed as {text}.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
            assert!(lower.contains("content-type: application/json"));
            assert!(lower.contains("authorization: bearer key"));
            // Body is JSON: contains input_audio.data + format:"wav", not multipart.
            assert!(request_text.contains("input_audio"));
            assert!(request_text.contains(r#""format":"wav""#));
            assert!(!lower.contains("multipart/form-data"));
            write_json_response(&mut stream, r#"{"text":"openrouter ok"}"#);
        });
        let base_url = format!("http://{}", addr);

        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "openai/whisper-large-v3-turbo".to_string(),
            None,
            None,
            false,
        )
        .with_request_format(AsrRequestFormat::OpenRouterJson);
        let pcm = vec![0u8; 32_000 * 2];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "openrouter ok");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn zenmux_format_posts_json_with_language_and_itn() {
        // issue #837: ZenMuxJson posts application/json with
        // input_audio.data (base64); language is sent when mapped and
        // enable_itn is always sent explicitly; the response is parsed as {text}.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
            assert!(lower.contains("content-type: application/json"));
            assert!(lower.contains("authorization: bearer key"));
            assert!(request_text.contains("input_audio"));
            assert!(request_text.contains(r#""format":"wav""#));
            assert!(request_text.contains(r#""language":"zh""#));
            assert!(request_text.contains(r#""enable_itn":true"#));
            assert!(!lower.contains("multipart/form-data"));
            write_json_response(&mut stream, r#"{"text":"zenmux ok"}"#);
        });
        let base_url = format!("http://{}", addr);

        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "qwen/qwen3-asr-flash".to_string(),
            None,
            Some(30_000),
            false,
        )
        .with_request_format(AsrRequestFormat::ZenMuxJson)
        .with_language(Some("zh".to_string()))
        .with_enable_itn(true);
        let pcm = vec![0u8; 32_000 * 2];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "zenmux ok");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn zenmux_format_omits_language_when_unset_and_sends_false_itn() {
        // With no language mapping (None), the field is omitted; enable_itn=false is sent explicitly.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.contains(r#""enable_itn":false"#));
            assert!(!request_text.contains(r#""language""#));
            write_json_response(&mut stream, r#"{"text":"zenmux no-lang ok"}"#);
        });
        let base_url = format!("http://{}", addr);

        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "qwen/qwen3-asr-flash".to_string(),
            None,
            None,
            false,
        )
        .with_request_format(AsrRequestFormat::ZenMuxJson)
        .with_language(None)
        .with_enable_itn(false);
        let pcm = vec![0u8; 32_000 * 2];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "zenmux no-lang ok");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn empty_api_key_omits_authorization_header() {
        // openai-compatible allows an empty API key (LAN no-auth endpoints):
        // the request must not carry an empty Bearer header, which servers 401.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
            assert!(!request_text.to_ascii_lowercase().contains("authorization:"));
            write_json_response(&mut stream, r#"{"text":"no-auth ok"}"#);
        });
        let base_url = format!("http://{}", addr);

        let asr = WhisperBatchASR::new(
            String::new(),
            base_url,
            "qwen3-asr".to_string(),
            None,
            None,
            false,
        );
        let pcm = vec![0u8; 32_000 * 2];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "no-auth ok");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn hotwords_sent_as_json_array_field_without_prompt() {
        // StepFun shape: the dictionary goes through the first-class `hotwords`
        // (JSON array string) instead of `prompt`; blank entries are filtered
        // before encoding.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
            assert!(request_text.contains(r#"name="hotwords""#));
            assert!(request_text.contains(r#"["阶跃星辰","OpenLess"]"#));
            assert!(!request_text.contains(r#"name="prompt""#));
            write_json_response(&mut stream, r#"{"text":"hotwords ok"}"#);
        });
        let base_url = format!("http://{}", addr);

        let asr = WhisperBatchASR::new(
            "key".to_string(),
            base_url,
            "stepaudio-2.5-asr".to_string(),
            None,
            None,
            false,
        )
        .with_hotwords(vec![
            "阶跃星辰".to_string(),
            "   ".to_string(),
            "OpenLess".to_string(),
        ]);
        let pcm = vec![0u8; 32_000 * 2];
        asr.consume_pcm_chunk(&pcm);

        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "hotwords ok");
        server.join().unwrap();
    }

    fn start_whisper_test_server(texts: Vec<&'static str>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for text in texts {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "timed out waiting for ASR test request"
                            );
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("accept ASR test request failed: {err}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_http_request(&mut stream);
                let request_text = String::from_utf8_lossy(&request);
                let request_text_lower = request_text.to_ascii_lowercase();
                assert!(request_text.starts_with("POST /audio/transcriptions HTTP/1.1"));
                assert!(request_text_lower.contains("authorization: bearer key"));
                assert!(request_text.contains("model"));
                write_json_response(&mut stream, &format!(r#"{{"text":"{}"}}"#, text));
            }
        });
        (format!("http://{}", addr), server)
    }

    fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut buf = [0u8; 8192];
        let mut request = Vec::new();
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let header_text = String::from_utf8_lossy(&request[..header_end + 4]);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .or_else(|| line.strip_prefix("Content-Length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        request
    }

    fn write_json_response(stream: &mut TcpStream, body: &str) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    }
}
