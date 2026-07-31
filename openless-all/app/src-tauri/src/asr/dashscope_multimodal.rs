//! 阿里云百炼（DashScope）多模态生成同步接口的批量 ASR 客户端。
//!
//! `fun-asr-flash` 与 `qwen-audio-3.0-asr-flash` 系列是**非实时录音文件识别**
//! 模型，走 DashScope 私有的
//! `multimodal-generation/generation` HTTP 接口，既不是实时 WebSocket 双工
//! （见 `bailian.rs`），也不是 OpenAI 兼容的 `/audio/transcriptions`
//! （见 `whisper.rs`）。因此单独成一路批量客户端：录音结束后把整段 PCM 编成
//! WAV、base64 进 JSON body、POST 一次拿整段文本。
//!
//! 结构与 `mimo.rs`（同为「攒 PCM → POST 一段音频 → 解析私有 JSON」）一致，
//! 复用其 `split_pcm_by_duration` / `join_transcript_chunks` 分片与拼接逻辑，
//! 只有请求信封与响应解析不同。

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;
use serde_json::Value;

use crate::asr::mimo::{join_transcript_chunks, split_pcm_by_duration};
use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

// fun-asr-flash 单条音频上限 5 分钟；但真正的硬约束是 base64 进 JSON 的请求体
// 体积。沿用 mimo 验证过的 180s 预算（16k/16-bit/mono WAV base64 后约 7.7MB），
// 稳稳落在时长和常见网关体积上限之内。超长录音按此切分后逐段识别再拼接。
const DASHSCOPE_MAX_CHUNK_DURATION_MS: u64 = 180_000;

pub const PROVIDER_ID: &str = "bailian-fun-asr-flash";
pub const DEFAULT_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";
pub const DEFAULT_MODEL: &str = "fun-asr-flash-2026-06-15";
pub const QWEN_AUDIO_MODEL: &str = "qwen-audio-3.0-asr-flash";

pub struct DashScopeMultimodalASR {
    api_key: String,
    base_url: String,
    model: String,
    buffer: Mutex<Vec<u8>>,
}

impl DashScopeMultimodalASR {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn buffer_duration_ms(&self) -> u64 {
        crate::asr::pcm::pcm_duration_ms(&self.buffer.lock())
    }

    pub async fn transcribe(&self) -> Result<RawTranscript> {
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm).await;
        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(&self, pcm: &[u8]) -> Result<RawTranscript> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("DashScope API key missing");
        }

        let duration_ms = crate::asr::pcm::pcm_duration_ms(pcm);
        let chunks = split_pcm_by_duration(pcm, DASHSCOPE_MAX_CHUNK_DURATION_MS);
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
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let body = dashscope_multimodal_body(&self.model, &wav);
        let url = generation_url(&self.base_url)?;
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .header("Content-Type", "application/json")
            // multimodal-generation 默认可 SSE 流式；显式关掉走一次性 JSON 响应。
            .header("X-DashScope-SSE", "disable")
            .json(&body)
            .send()
            .await
            .context("DashScope ASR HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DashScope ASR API error {}: {}", status, body);
        }

        let json: Value = resp.json().await.context("parse DashScope ASR response")?;
        Ok(extract_dashscope_text(&json).trim().to_string())
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

pub fn is_supported_model(model: &str) -> bool {
    let model = model.trim();
    model == DEFAULT_MODEL || model == QWEN_AUDIO_MODEL
}

impl crate::recorder::AudioConsumer for DashScopeMultimodalASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// 归一化到 multimodal-generation 的完整 endpoint。
///
/// preset 默认下发的就是完整地址，命中首个分支直接用；用户若只填了业务空间
/// 专属域名根（`https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com`）则补上标准
/// 路径。其余情况保守地把标准后缀拼到用户给的路径后面。
pub fn generation_url(base_url: &str) -> Result<String> {
    const CANONICAL_PATH: &str = "/api/v1/services/aigc/multimodal-generation/generation";
    let trimmed = base_url.trim();
    let parsed = reqwest::Url::parse(trimmed).context("parse DashScope base URL")?;
    let path = parsed.path().trim_end_matches('/');
    if path.ends_with("/multimodal-generation/generation") {
        let mut url = parsed.clone();
        url.set_path(path);
        return Ok(url.to_string());
    }
    let mut url = parsed.clone();
    if path.is_empty() {
        url.set_path(CANONICAL_PATH);
    } else {
        url.set_path(&format!("{path}{CANONICAL_PATH}"));
    }
    Ok(url.to_string())
}

pub fn dashscope_multimodal_body(model: &str, wav: &[u8]) -> Value {
    let audio_data = format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(wav)
    );
    // qwen-audio-3.0-asr-flash 还支持 vocabulary 与 language_hints；当前批量客户端
    // 尚未将这两项设置映射到请求体，暂时保持自动语言检测且不传热词。
    serde_json::json!({
        "model": model,
        "input": {
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "input_audio",
                    "input_audio": { "data": audio_data },
                }],
            }],
        },
        "parameters": {
            "format": "wav",
            "sample_rate": "16000",
        },
    })
}

/// fun-asr-flash 的响应信封与标准多模态接口不同，且不同模型版本字段路径略有
/// 差异（`output.text` / `output.output.sentence.text` / 标准 `choices`）。
/// 这里按已知路径逐一兜底提取，取到第一个非空文本即返回，避免因单一路径假设
/// 而在某个版本上静默丢字。
pub fn extract_dashscope_text(json: &Value) -> String {
    let output = json.get("output");

    // 1) output.text —— fun-asr-flash 文档主路径
    if let Some(text) = output.and_then(|o| o.get("text")).and_then(Value::as_str) {
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }

    // 2) output.output.sentence.text —— 文档给出的另一种嵌套形态
    if let Some(text) = output
        .and_then(|o| o.get("output"))
        .and_then(|o| o.get("sentence"))
        .and_then(|s| s.get("text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }

    // 3) output.sentence.text
    if let Some(text) = output
        .and_then(|o| o.get("sentence"))
        .and_then(|s| s.get("text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }

    // 4) 标准多模态 output.choices[0].message.content（字符串或 [{text}] 数组）
    if let Some(content) = output
        .and_then(|o| o.get("choices"))
        .and_then(|c| c.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    {
        if let Some(text) = content.as_str() {
            return text.trim().to_string();
        }
        if let Some(items) = content.as_array() {
            return items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_string();
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn generation_url_from_full_endpoint_is_unchanged() {
        assert_eq!(generation_url(DEFAULT_ENDPOINT).unwrap(), DEFAULT_ENDPOINT);
        assert_eq!(
            generation_url("https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation/").unwrap(),
            DEFAULT_ENDPOINT
        );
    }

    #[test]
    fn generation_url_from_workspace_host_gets_canonical_path() {
        assert_eq!(
            generation_url("https://ws-xxx.cn-beijing.maas.aliyuncs.com").unwrap(),
            "https://ws-xxx.cn-beijing.maas.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
        );
    }

    #[test]
    fn body_uses_multimodal_generation_shape() {
        for model in [DEFAULT_MODEL, QWEN_AUDIO_MODEL] {
            let body = dashscope_multimodal_body(model, b"wav");
            assert_eq!(body["model"], model);
            let audio = &body["input"]["messages"][0]["content"][0];
            assert_eq!(audio["type"], "input_audio");
            assert!(audio["input_audio"]["data"]
                .as_str()
                .unwrap()
                .starts_with("data:audio/wav;base64,"));
            assert_eq!(body["parameters"]["format"], "wav");
            assert_eq!(body["parameters"]["sample_rate"], "16000");
            assert!(body["parameters"].get("vocabulary_id").is_none());
        }
    }

    #[test]
    fn extract_text_prefers_output_text() {
        let json = serde_json::json!({ "output": { "text": "  你好世界  " } });
        assert_eq!(extract_dashscope_text(&json), "你好世界");
    }

    #[test]
    fn extract_text_falls_back_to_nested_sentence() {
        let json = serde_json::json!({
            "output": { "output": { "sentence": { "text": "嵌套句" } } }
        });
        assert_eq!(extract_dashscope_text(&json), "嵌套句");
    }

    #[test]
    fn extract_text_falls_back_to_choices_content_array() {
        let json = serde_json::json!({
            "output": {
                "choices": [{
                    "message": { "content": [{ "text": "第一段" }, { "text": "第二段" }] }
                }]
            }
        });
        assert_eq!(extract_dashscope_text(&json), "第一段第二段");
    }

    #[test]
    fn extract_text_empty_when_no_known_path() {
        let json = serde_json::json!({ "request_id": "abc", "output": {} });
        assert_eq!(extract_dashscope_text(&json), "");
    }

    #[tokio::test]
    async fn posts_multimodal_generation_request() {
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
                            "timed out waiting for DashScope ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept DashScope ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with(
                "POST /api/v1/services/aigc/multimodal-generation/generation HTTP/1.1"
            ));
            assert!(lower.contains("authorization: bearer sk-test"));
            assert!(lower.contains("content-type: application/json"));
            assert!(request_text.contains(r#""model":"fun-asr-flash-2026-06-15""#));
            assert!(request_text.contains(r#""type":"input_audio""#));
            assert!(request_text.contains("data:audio/wav;base64,"));
            assert!(!request_text.contains("vocabulary_id"));
            write_json_response(
                &mut stream,
                r#"{"output":{"text":"你好百炼"},"request_id":"r1"}"#,
            );
        });

        let asr = DashScopeMultimodalASR::new(
            "sk-test".to_string(),
            format!(
                "http://{}/api/v1/services/aigc/multimodal-generation/generation",
                addr
            ),
            DEFAULT_MODEL.to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        assert_eq!(asr.buffer_duration_ms(), 1_000);
        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "你好百炼");
        assert_eq!(transcript.duration_ms, 1_000);
        server.join().unwrap();
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        let mut expected_len = None;
        loop {
            let read = stream.read(&mut buf).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if expected_len.is_none() {
                expected_len = parse_expected_request_len(&request);
            }
            if expected_len.is_some_and(|len| request.len() >= len) {
                break;
            }
        }
        request
    }

    fn parse_expected_request_len(request: &[u8]) -> Option<usize> {
        let header_end = request.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_len = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })?;
        Some(header_end + content_len)
    }

    fn write_json_response(stream: &mut std::net::TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }
}
