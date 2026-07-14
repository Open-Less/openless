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
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::Value;
use std::time::Duration;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

pub const PROVIDER_ID: &str = "elevenlabs";
pub const DEFAULT_ENDPOINT: &str = "https://api.elevenlabs.io/v1";
// `scribe_v2` is the current recommended Scribe model (higher accuracy, 99
// languages, ~40% cheaper). `scribe_v1` is deprecated and is removed from the
// API on 2026-07-09, so it would be a poor default to ship.
pub const DEFAULT_MODEL: &str = "scribe_v2";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

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

    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(&self.buffer.lock())
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
        let resolved = crate::endpoint_security::resolve_http_endpoint(&url)
            .await
            .context("resolve ElevenLabs endpoint")?;

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
            .text("tag_audio_events", "false")
            .text("timestamps_granularity", "none");

        // Never forward the custom credential header or audio to a redirect
        // target. Unlike standard Authorization headers, `xi-api-key` is not
        // guaranteed to be stripped by HTTP clients on cross-origin redirects.
        let mut client_builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(transcribe_timeout(duration_ms as f64 / 1000.0));
        if let Some(resolved) = &resolved {
            client_builder = client_builder.resolve_to_addrs(&resolved.host, &resolved.addrs);
        }
        let client = client_builder
            .build()
            .context("build ElevenLabs ASR HTTP client")?;
        let resp = client
            .post(&url)
            .header("xi-api-key", self.api_key.trim())
            .multipart(form)
            .send()
            .await
            .context("ElevenLabs ASR HTTP request failed")?;

        let status = resp.status();
        let body = read_response_limited(resp).await?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&body);
            if let Some(code) = safe_error_code(&body) {
                anyhow::bail!("ElevenLabs ASR API error {} (code: {})", status, code);
            }
            anyhow::bail!("ElevenLabs ASR API error {}", status);
        }

        let json: Value = serde_json::from_slice(&body).context("parse ElevenLabs ASR response")?;
        Ok(RawTranscript {
            text: extract_text(&json)?.trim().to_string(),
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
    crate::endpoint_security::validate_http_endpoint(base_url)
        .map_err(anyhow::Error::msg)
        .context("validate ElevenLabs endpoint")?;
    let parsed = reqwest::Url::parse(base_url.trim()).context("parse ElevenLabs base URL")?;
    let loopback_http = parsed.scheme() == "http"
        && parsed.host_str().is_some_and(|host| {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
    if parsed.scheme() != "https" && !loopback_http {
        anyhow::bail!("ElevenLabs endpoint must use HTTPS (HTTP is allowed only for loopback)");
    }
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

async fn read_response_limited(response: reqwest::Response) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read ElevenLabs ASR response")?;
        append_response_chunk(&mut body, &chunk)?;
    }
    Ok(body)
}

fn append_response_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<()> {
    if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
        anyhow::bail!("ElevenLabs ASR response too large");
    }
    body.extend_from_slice(chunk);
    Ok(())
}

/// Batch transcription gets a fixed network allowance plus time proportional
/// to the recording, while retaining a practical minimum for short clips.
pub fn transcribe_timeout(audio_secs: f64) -> Duration {
    Duration::from_secs(30.max((audio_secs * 0.5).ceil() as u64 + 20))
}

/// Pull the transcript out of the Scribe response.
///
/// Single-channel responses carry a top-level `text`; multichannel responses
/// carry `transcripts: [{ text, ... }]` instead. Concatenate the latter so a
/// stray multichannel result still yields usable text.
pub fn extract_text(json: &Value) -> Result<String> {
    if let Some(text) = json.get("text") {
        return text
            .as_str()
            .map(str::to_string)
            .context("ElevenLabs ASR response `text` must be a string");
    }
    if let Some(transcripts) = json.get("transcripts") {
        let transcripts = transcripts
            .as_array()
            .context("ElevenLabs ASR response `transcripts` must be an array")?;
        return transcripts
            .iter()
            .map(|item| {
                item.get("text")
                    .and_then(|text| text.as_str())
                    .context("ElevenLabs ASR transcript missing string `text`")
            })
            .collect::<Result<Vec<_>>>()
            .map(|texts| texts.join(" "));
    }
    anyhow::bail!("ElevenLabs ASR response missing `text` or `transcripts`")
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    super::pcm::pcm_duration_ms(pcm)
}

/// Return only locally-known, non-sensitive error codes.
///
/// The response body is untrusted, especially for custom endpoints. Never
/// propagate arbitrary fields into errors because callers log and display the
/// resulting message. Unknown codes deliberately collapse to the HTTP status.
fn safe_error_code(body: &str) -> Option<&'static str> {
    let json: Value = serde_json::from_str(body).ok()?;
    match json.pointer("/detail/code").and_then(Value::as_str) {
        Some("audio_too_short") => Some("audio_too_short"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
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
    fn url_rejects_insecure_non_loopback_endpoint() {
        let error = speech_to_text_url("http://api.example.com/v1").unwrap_err();
        assert!(format!("{error:#}").to_ascii_lowercase().contains("https"));

        assert!(speech_to_text_url("http://127.0.0.1:8080/v1").is_ok());
        assert!(speech_to_text_url("http://localhost:8080/v1").is_ok());
        assert!(speech_to_text_url("http://[::1]:8080/v1").is_ok());
    }

    #[test]
    fn url_rejects_sensitive_network_targets() {
        assert!(speech_to_text_url("https://169.254.169.254/v1").is_err());
        assert!(speech_to_text_url("https://100.64.0.1/v1").is_err());
        assert!(speech_to_text_url("https://metadata.google.internal/v1").is_err());
    }

    #[test]
    fn timeout_scales_with_recording_duration() {
        assert_eq!(transcribe_timeout(0.0), Duration::from_secs(30));
        assert_eq!(transcribe_timeout(20.0), Duration::from_secs(30));
        assert_eq!(transcribe_timeout(120.0), Duration::from_secs(80));
    }

    #[test]
    fn extract_text_reads_single_channel() {
        let json = serde_json::json!({ "language_code": "en", "text": "hello world" });
        assert_eq!(extract_text(&json).unwrap(), "hello world");
    }

    #[test]
    fn extract_text_joins_multichannel_transcripts() {
        let json = serde_json::json!({
            "transcripts": [ { "text": "left" }, { "text": "right" } ]
        });
        assert_eq!(extract_text(&json).unwrap(), "left right");
    }

    #[test]
    fn extract_text_rejects_missing_transcript_field() {
        let json = serde_json::json!({ "language_code": "en" });
        assert!(extract_text(&json).is_err());
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
            assert!(request_text.contains("scribe_v2"));
            assert!(request_text.contains("name=\"file\""));
            assert!(request_text.contains("name=\"tag_audio_events\"\r\n\r\nfalse\r\n"));
            assert!(request_text.contains("name=\"timestamps_granularity\"\r\n\r\nnone\r\n"));
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

    #[test]
    fn response_limit_accepts_one_megabyte_and_rejects_the_next_byte() {
        let mut body = Vec::new();
        append_response_chunk(&mut body, &vec![b'a'; MAX_RESPONSE_BYTES]).unwrap();
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);

        let error = append_response_chunk(&mut body, b"a")
            .unwrap_err()
            .to_string();
        assert!(error.contains("response too large"));
    }

    #[tokio::test]
    async fn transcribe_redacts_untrusted_error_response_body() {
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
            let _request = read_http_request(&mut stream);
            let body = r#"{"detail":{"code":"audio_too_short","message":"SENSITIVE_API_KEY_VALUE","request_id":"SENSITIVE_REQUEST_ID","transcript":"SENSITIVE_TRANSCRIPT"}}"#;
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        let asr = ElevenLabsBatchASR::new(
            "synthetic-test-key".to_string(),
            format!("http://{}/v1", addr),
            DEFAULT_MODEL.to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        let error = asr.transcribe().await.unwrap_err().to_string();

        assert!(error.contains("400 Bad Request"));
        assert!(error.contains("audio_too_short"));
        assert!(!error.contains("SENSITIVE_API_KEY_VALUE"));
        assert!(!error.contains("SENSITIVE_REQUEST_ID"));
        assert!(!error.contains("SENSITIVE_TRANSCRIPT"));
        assert!(!error.contains("message"));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn transcribe_does_not_follow_redirects_with_credentials() {
        let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_addr = redirect_listener.local_addr().unwrap();
        let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        target_listener.set_nonblocking(true).unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let followed = Arc::new(AtomicBool::new(false));
        let target_followed = Arc::clone(&followed);

        let redirect_server = thread::spawn(move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let _request = read_http_request(&mut stream);
            let response = format!(
                "HTTP/1.1 302 Found\r\nlocation: http://{}/v1/speech-to-text\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                target_addr
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });
        let target_server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match target_listener.accept() {
                    Ok((mut stream, _)) => {
                        target_followed.store(true, Ordering::SeqCst);
                        write_json_response(
                            &mut stream,
                            r#"{"language_code":"en","text":"redirect followed"}"#,
                        );
                        break;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept redirect target request failed: {err}"),
                }
            }
        });

        let asr = ElevenLabsBatchASR::new(
            "synthetic-test-key".to_string(),
            format!("http://{}/v1", redirect_addr),
            DEFAULT_MODEL.to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        let error = asr.transcribe().await.unwrap_err().to_string();

        assert!(error.contains("302 Found"));
        redirect_server.join().unwrap();
        target_server.join().unwrap();
        assert!(!followed.load(Ordering::SeqCst));
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
