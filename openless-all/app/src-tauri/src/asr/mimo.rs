//! Batch MiMo ASR client — collects PCM in a buffer, then POSTs a WAV file
//! to Xiaomi MiMo's `/v1/chat/completions` endpoint on session end.
//!
//! MiMo ASR uses OpenAI Chat Completions compatible format, but with audio
//! input instead of text. The audio is sent as base64-encoded data URL.
//!
//! Reference: https://platform.xiaomimimo.com/docs/zh-CN/api/audio/Speech-Recognition

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

const PCM_SAMPLE_RATE_HZ: u64 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;

pub const PROVIDER_ID: &str = "mimo";
pub const DEFAULT_ENDPOINT: &str = "https://api.xiaomimimo.com/v1";
pub const DEFAULT_MODEL: &str = "mimo-v2.5-asr";

/// MiMo ASR 请求体编码方式。
///
/// MiMo 使用 OpenAI Chat Completions 格式，音频以 base64 data URL 形式
/// 放在 `messages[].content[].input_audio.data` 字段中。
#[derive(Clone, Debug)]
pub struct MiMoBatchASR {
    api_key: String,
    base_url: String,
    model: String,
    /// 语言 hint：`auto`（默认）/ `zh` / `en`。
    language: String,
    buffer: Mutex<Vec<u8>>,
}

impl MiMoBatchASR {
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        language: Option<String>,
    ) -> Self {
        Self {
            api_key,
            base_url: normalize_endpoint(&base_url),
            model: normalize_model(&model),
            language: language.unwrap_or_else(|| "auto".to_string()),
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Stop collecting audio, encode the buffer as WAV, and POST to the
    /// MiMo ASR endpoint.
    ///
    /// 失败时**保留** PCM buffer，让上层有机会重试或在历史中至少留一个失败记录。
    pub async fn transcribe(&self) -> Result<RawTranscript> {
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm).await;
        // 仅在成功路径上才清 buffer。
        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(&self, pcm: &[u8]) -> Result<RawTranscript> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("MiMo API key missing");
        }

        let duration_ms = pcm_duration_ms(pcm);
        let text = self.transcribe_chunk(pcm).await?;

        Ok(RawTranscript { text, duration_ms })
    }

    async fn transcribe_chunk(&self, pcm: &[u8]) -> Result<String> {
        let samples: Vec<i16> = pcm
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let wav_base64 = base64::engine::general_purpose::STANDARD.encode(&wav);

        // 构造 data URL
        let data_url = format!("data:audio/wav;base64,{wav_base64}");

        // 构造请求体（OpenAI Chat Completions 格式）
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_audio",
                            "input_audio": {
                                "data": data_url
                            }
                        }
                    ]
                }
            ],
            "asr_options": {
                "language": self.language
            },
            "stream": false
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let client = reqwest::Client::new();

        let resp = client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("MiMo ASR HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("MiMo ASR API error {}: {}", status, body);
        }

        let json: serde_json::Value = resp.json().await.context("parse MiMo ASR response")?;

        // 从 Chat Completion 响应中提取识别文本
        let text = json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|content| content.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        Ok(text)
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for MiMoBatchASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// 标准化 endpoint：空字符串或空白时使用默认值。
fn normalize_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        DEFAULT_ENDPOINT.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 标准化 model：空字符串或空白时使用默认值。
fn normalize_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        trimmed.to_string()
    }
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    (pcm.len() as u64 / PCM_BYTES_PER_SAMPLE as u64) * 1000 / PCM_SAMPLE_RATE_HZ
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;

    #[test]
    fn normalize_endpoint_returns_default_for_empty() {
        assert_eq!(normalize_endpoint(""), DEFAULT_ENDPOINT);
        assert_eq!(normalize_endpoint("  "), DEFAULT_ENDPOINT);
    }

    #[test]
    fn normalize_endpoint_preserves_non_empty() {
        assert_eq!(
            normalize_endpoint("https://custom.api.com/v1"),
            "https://custom.api.com/v1"
        );
    }

    #[test]
    fn normalize_model_returns_default_for_empty() {
        assert_eq!(normalize_model(""), DEFAULT_MODEL);
        assert_eq!(normalize_model("  "), DEFAULT_MODEL);
    }

    #[test]
    fn normalize_model_preserves_non_empty() {
        assert_eq!(normalize_model("mimo-v2.5-asr"), "mimo-v2.5-asr");
    }

    #[test]
    fn pcm_duration_ms_calculates_correctly() {
        // 1 秒的 16kHz 16-bit mono PCM = 32000 字节
        let pcm = vec![0u8; 32_000];
        assert_eq!(pcm_duration_ms(&pcm), 1000);
    }

    #[test]
    fn pcm_duration_ms_handles_empty() {
        let pcm = vec![];
        assert_eq!(pcm_duration_ms(&pcm), 0);
    }

    #[test]
    fn consume_pcm_chunk_accumulates() {
        let asr = MiMoBatchASR::new(
            "test-key".to_string(),
            String::new(),
            String::new(),
            None,
        );

        let pcm1 = vec![0u8; 100];
        let pcm2 = vec![0u8; 200];
        asr.consume_pcm_chunk(&pcm1);
        asr.consume_pcm_chunk(&pcm2);

        assert_eq!(asr.buffer.lock().len(), 300);
    }

    #[test]
    fn cancel_clears_buffer() {
        let asr = MiMoBatchASR::new(
            "test-key".to_string(),
            String::new(),
            String::new(),
            None,
        );

        asr.consume_pcm_chunk(&vec![0u8; 100]);
        assert_eq!(asr.buffer.lock().len(), 100);

        asr.cancel();
        assert!(asr.buffer.lock().is_empty());
    }
}
