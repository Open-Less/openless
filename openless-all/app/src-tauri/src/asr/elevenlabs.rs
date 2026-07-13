//! ElevenLabs Scribe ASR client.
//!
//! ElevenLabs Speech-to-Text uses `POST /v1/speech-to-text` with
//! `multipart/form-data` (`model_id` + `file`) and the `xi-api-key` header —
//! *not* OpenAI's Bearer `/audio/transcriptions` protocol — so it needs its
//! own client rather than riding on `WhisperBatchASR`.
//!
//! ElevenLabs accepts files up to 5 GB per request, so unlike MiMo (10 MB
//! base64 limit) there is no need to split the buffer: a single dictation
//! session always fits in one request.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde_json::Value;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

pub const PROVIDER_ID: &str = "elevenlabs";
pub const DEFAULT_ENDPOINT: &str = "https://api.elevenlabs.io/v1";
pub const DEFAULT_MODEL: &str = "scribe_v1";

pub struct ElevenLabsBatchASR {
    api_key: String,
    base_url: String,
    model: String,
    buffer: Mutex<Vec<u8>>,
}

impl ElevenLabsBatchASR {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            buffer: Mutex::new(Vec::new()),
        }
    }

    pub async fn transcribe(&self) -> Result<RawTranscript> {
        // clone rather than take: only on a successful response do we clear the
        // buffer, so a credential / network failure keeps the user's audio for
        // a retry (mirrors WhisperBatchASR / MimoBatchASR).
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
            anyhow::bail!("ElevenLabs API key missing");
        }

        let duration_ms = pcm_duration_ms(pcm);
        let samples: Vec<i16> = pcm
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let url = speech_to_text_url(&self.base_url)?;

        let wav_part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .context("set MIME type")?;
        // `tag_audio_events=false`: by default Scribe emits bracketed non-speech
        // events like "(laughter)" / "(高音)" into `text`; for dictation those
        // pollute the inserted text, so disable them.
        let form = reqwest::multipart::Form::new()
            .part("file", wav_part)
            .text("model_id", self.model.clone())
            .text("tag_audio_events", "false");

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("xi-api-key", self.api_key.trim())
            .multipart(form)
            .send()
            .await
            .context("ElevenLabs ASR HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("ElevenLabs ASR API error {}: {}", status, body);
        }

        let json: Value = resp.json().await.context("parse ElevenLabs ASR response")?;
        Ok(RawTranscript {
            text: extract_text(&json).trim().to_string(),
            duration_ms,
        })
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for ElevenLabsBatchASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// Build the `/v1/speech-to-text` endpoint from a configured base URL.
///
/// Accepts either the API root (`https://api.elevenlabs.io/v1`) or the full
/// endpoint already ending in `/speech-to-text`, so re-saving the resolved URL
/// is idempotent.
pub fn speech_to_text_url(base_url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(base_url.trim()).context("parse ElevenLabs base URL")?;
    let mut url = parsed.clone();
    let path = parsed.path().trim_end_matches('/');
    let next_path = if path.ends_with("/speech-to-text") {
        path.to_string()
    } else {
        format!("{path}/speech-to-text")
    };
    url.set_path(&next_path);
    Ok(url.to_string())
}

/// Pull the transcript out of the Scribe response.
///
/// Single-channel responses carry a top-level `text`; multichannel responses
/// carry `transcripts: [{ text, ... }]` instead. Concatenate the latter so a
/// stray multichannel result still yields usable text.
pub fn extract_text(json: &Value) -> String {
    if let Some(text) = json.get("text").and_then(|t| t.as_str()) {
        return text.to_string();
    }
    if let Some(transcripts) = json.get("transcripts").and_then(|t| t.as_array()) {
        return transcripts
            .iter()
            .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
    }
    String::new()
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    super::pcm::pcm_duration_ms(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn url_targets_speech_to_text() {
        assert_eq!(
            speech_to_text_url("https://api.elevenlabs.io/v1").unwrap(),
            "https://api.elevenlabs.io/v1/speech-to-text"
        );
        assert_eq!(
            speech_to_text_url("https://api.elevenlabs.io/v1/").unwrap(),
            "https://api.elevenlabs.io/v1/speech-to-text"
        );
        assert_eq!(
            speech_to_text_url("https://api.elevenlabs.io/v1/speech-to-text").unwrap(),
            "https://api.elevenlabs.io/v1/speech-to-text"
        );
    }

    #[test]
    fn extract_text_reads_single_channel() {
        let json = serde_json::json!({ "language_code": "en", "text": "hello world" });
        assert_eq!(extract_text(&json), "hello world");
    }

    #[test]
    fn extract_text_joins_multichannel_transcripts() {
        let json = serde_json::json!({
            "transcripts": [ { "text": "left" }, { "text": "right" } ]
        });
        assert_eq!(extract_text(&json), "left right");
    }

    #[test]
    fn extract_text_missing_field_is_empty() {
        let json = serde_json::json!({ "language_code": "en" });
        assert_eq!(extract_text(&json), "");
    }

    #[tokio::test]
    async fn transcribe_posts_multipart_speech_to_text_request() {
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
                            "timed out waiting for ElevenLabs ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept ElevenLabs ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with("POST /v1/speech-to-text HTTP/1.1"));
            assert!(lower.contains("xi-api-key: key"));
            assert!(lower.contains("content-type: multipart/form-data"));
            assert!(request_text.contains("name=\"model_id\""));
            assert!(request_text.contains("scribe_v1"));
            assert!(request_text.contains("name=\"file\""));
            assert!(request_text.contains("name=\"tag_audio_events\""));
            write_json_response(
                &mut stream,
                r#"{"language_code":"en","text":"elevenlabs ok"}"#,
            );
        });

        let asr = ElevenLabsBatchASR::new(
            "key".to_string(),
            format!("http://{}/v1", addr),
            DEFAULT_MODEL.to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "elevenlabs ok");
        assert_eq!(transcript.duration_ms, 1_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn transcribe_empty_buffer_skips_request() {
        let asr = ElevenLabsBatchASR::new(
            "key".to_string(),
            "http://127.0.0.1:1/v1".to_string(),
            DEFAULT_MODEL.to_string(),
        );
        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "");
        assert_eq!(transcript.duration_ms, 0);
    }

    fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
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

    fn write_json_response(stream: &mut TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }
}
