//! Xiaomi MiMo ASR client.
//!
//! MiMo ASR uses the official OpenAI-compatible `/chat/completions` endpoint
//! with `messages[].content[].input_audio`, not Whisper's
//! `/audio/transcriptions` protocol.

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;
use serde_json::Value;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

const PCM_SAMPLE_RATE_HZ: u64 = 16_000;
const PCM_BYTES_PER_SAMPLE: usize = 2;
// 官方限制：Base64 后的音频数据不能超过 10MB。180s 的 16k/16-bit/mono WAV
// Base64 后约 7.7MB，给 JSON/data-url 前缀和厂商侧 MB 口径差异留余量。
const MIMO_MAX_CHUNK_DURATION_MS: u64 = 180_000;
pub const PROVIDER_ID: &str = "xiaomi-mimo-asr";
pub const DEFAULT_ENDPOINT: &str = "https://api.xiaomimimo.com/v1";
pub const DEFAULT_MODEL: &str = "mimo-v2.5-asr";

pub struct MimoBatchASR {
    api_key: String,
    base_url: String,
    model: String,
    buffer: Mutex<Vec<u8>>,
}

impl MimoBatchASR {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            buffer: Mutex::new(Vec::new()),
        }
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
            anyhow::bail!("MiMo API key missing");
        }

        let duration_ms = pcm_duration_ms(pcm);
        let chunks = split_pcm_by_duration(pcm, MIMO_MAX_CHUNK_DURATION_MS);
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
        let body = mimo_chat_body(&self.model, &wav);
        let url = mimo_chat_completions_url(&self.base_url)?;
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .json(&body)
            .send()
            .await
            .context("MiMo ASR HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("MiMo ASR API error {}: {}", status, body);
        }

        let json: Value = resp.json().await.context("parse MiMo ASR response")?;
        Ok(extract_mimo_text(&json).trim().to_string())
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for MimoBatchASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

pub fn mimo_chat_completions_url(base_url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(base_url.trim()).context("parse MiMo base URL")?;
    let mut url = parsed.clone();
    let path = parsed.path().trim_end_matches('/');
    let next_path = if path.ends_with("/chat/completions") {
        path.to_string()
    } else if path.ends_with("/chat") {
        format!("{path}/completions")
    } else {
        format!("{path}/chat/completions")
    };
    url.set_path(&next_path);
    Ok(url.to_string())
}

pub fn mimo_chat_body(model: &str, wav: &[u8]) -> Value {
    let audio_data = format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(wav)
    );
    serde_json::json!({
        "model": model,
        "stream": false,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {
                    "data": audio_data,
                    "format": "wav",
                },
            }],
        }],
    })
}

pub fn extract_mimo_text(json: &Value) -> String {
    let Some(content) = json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    else {
        return String::new();
    };

    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    content
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .and_then(|text| text.as_str())
                        .or_else(|| item.get("content").and_then(|text| text.as_str()))
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    super::pcm::pcm_duration_ms(pcm)
}

fn split_pcm_by_duration(pcm: &[u8], max_chunk_duration_ms: u64) -> Vec<&[u8]> {
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

fn join_transcript_chunks(chunks: &[String]) -> String {
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

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

fn is_cjk_punctuation(c: char) -> bool {
    matches!(
        c,
        '。' | '，' | '、' | '！' | '？' | '；' | '：' | '」' | '』' | '）' | '》'
    )
}

fn is_closing_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() && !matches!(c, '(' | '[' | '{' | '"' | '\'') || is_cjk_punctuation(c)
}

fn is_opening_punctuation(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '"' | '\'' | '「' | '『' | '（' | '《')
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
    fn mimo_url_targets_chat_completions() {
        assert_eq!(
            mimo_chat_completions_url("https://api.xiaomimimo.com/v1").unwrap(),
            "https://api.xiaomimimo.com/v1/chat/completions"
        );
        assert_eq!(
            mimo_chat_completions_url("https://api.xiaomimimo.com/v1/chat/completions").unwrap(),
            "https://api.xiaomimimo.com/v1/chat/completions"
        );
    }

    #[test]
    fn mimo_body_uses_official_input_audio_shape() {
        let body = mimo_chat_body(DEFAULT_MODEL, b"wav");
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["stream"], false);
        let audio = &body["messages"][0]["content"][0];
        assert_eq!(audio["type"], "input_audio");
        assert_eq!(audio["input_audio"]["format"], "wav");
        assert!(audio["input_audio"]["data"]
            .as_str()
            .unwrap()
            .starts_with("data:audio/wav;base64,"));
    }

    #[test]
    fn mimo_response_extracts_message_content() {
        let json = serde_json::json!({
            "choices": [{ "message": { "content": "你好 MiMo" } }]
        });
        assert_eq!(extract_mimo_text(&json), "你好 MiMo");
    }

    #[test]
    fn mimo_response_accepts_content_array_text() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "content": [
                        { "type": "text", "text": "你好" },
                        { "type": "text", "text": "MiMo" }
                    ]
                }
            }]
        });
        assert_eq!(extract_mimo_text(&json), "你好MiMo");
    }

    #[tokio::test]
    async fn mimo_posts_chat_completion_audio_request() {
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
                            "timed out waiting for MiMo ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept MiMo ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with("POST /chat/completions HTTP/1.1"));
            assert!(lower.contains("authorization: bearer key"));
            assert!(lower.contains("content-type: application/json"));
            assert!(request_text.contains(r#""model":"mimo-v2.5-asr""#));
            assert!(request_text.contains(r#""type":"input_audio""#));
            assert!(request_text.contains("data:audio/wav;base64,"));
            write_json_response(
                &mut stream,
                r#"{"choices":[{"message":{"content":"mimo ok"}}]}"#,
            );
        });

        let asr = MimoBatchASR::new(
            "key".to_string(),
            format!("http://{}", addr),
            DEFAULT_MODEL.to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "mimo ok");
        assert_eq!(transcript.duration_ms, 1_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn mimo_splits_audio_before_base64_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for (index, text) in ["hello", "world"].into_iter().enumerate() {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "timed out waiting for MiMo ASR chunk request"
                            );
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("accept MiMo ASR chunk request failed: {err}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_http_request(&mut stream);
                let request_text = String::from_utf8_lossy(&request);
                assert!(request_text.starts_with("POST /chat/completions HTTP/1.1"));
                assert!(request_text.contains(r#""model":"mimo-v2.5-asr""#));
                assert!(request_text.contains("data:audio/wav;base64,"));
                assert!(
                    request_text.len() < 10 * 1024 * 1024,
                    "chunk {index} exceeded MiMo 10MB base64 request budget"
                );
                write_json_response(
                    &mut stream,
                    &format!(r#"{{"choices":[{{"message":{{"content":"{text}"}}}}]}}"#),
                );
            }
        });

        let asr = MimoBatchASR::new(
            "key".to_string(),
            format!("http://{}", addr),
            DEFAULT_MODEL.to_string(),
        );
        let pcm = vec![0u8; 32_000 * 181];
        asr.consume_pcm_chunk(&pcm);
        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "hello world");
        assert_eq!(transcript.duration_ms, 181_000);
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
