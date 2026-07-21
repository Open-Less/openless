//! Deepgram Live WebSocket ASR Provider.
//!
//! Protocol Specification (Deepgram Live Streaming):
//! - WebSocket Endpoint: `wss://api.deepgram.com/v1/listen`
//! - Auth Header: `Authorization: Token <API_KEY>` (or `Bearer <API_KEY>`)
//! - Query Parameters: `model=nova-3`, `language=zh` (or `en-US`), `smart_format=true`, `encoding=linear16`, `sample_rate=16000`, `channels=1`, `interim_results=true`, `endpointing=true`
//! - Binary Frames: Raw 16 kHz / 16-bit / mono PCM bytes.
//! - Server Event "Results": `{"type": "Results", "is_final": true|false, "speech_final": true|false, "channel": {"alternatives": [{"transcript": "..."}]}}`
//! - Client Close: Text frame `{"type": "CloseStream"}` or empty binary frame.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::qwen_realtime::join_segments;
use super::{AudioConsumer, RawTranscript};

pub const PROVIDER_ID: &str = "deepgram";
pub const DEFAULT_ENDPOINT: &str = "wss://api.deepgram.com/v1/listen";
pub const DEFAULT_MODEL: &str = "nova-3";

/// 100 ms of 16 kHz / 16-bit / mono PCM (3200 bytes).
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 3_200;
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
const SESSION_READY_TIMEOUT: Duration = Duration::from_secs(8);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

#[derive(Clone, Debug)]
pub struct DeepgramCredentials {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub language: Option<String>,
}

impl DeepgramCredentials {
    pub fn normalized_model(&self) -> String {
        let model = self.model.trim();
        if model.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model.to_string()
        }
    }

    pub fn connect_url(&self) -> String {
        let endpoint = self.endpoint.trim();
        let base_ws = if endpoint.is_empty() {
            DEFAULT_ENDPOINT.to_string()
        } else if endpoint.starts_with("wss://") || endpoint.starts_with("ws://") {
            endpoint.trim_end_matches('/').to_string()
        } else {
            let Ok(mut url) = url::Url::parse(endpoint) else {
                return DEFAULT_ENDPOINT.to_string();
            };
            if url.set_scheme("wss").is_err() {
                return DEFAULT_ENDPOINT.to_string();
            }
            url.to_string().trim_end_matches('/').to_string()
        };

        let model = self.normalized_model();
        let lang = self
            .language
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("zh");

        let query = format!(
            "model={model}&language={lang}&smart_format=true&encoding=linear16&sample_rate=16000&channels=1&interim_results=true&endpointing=true"
        );

        if base_ws.contains('?') {
            format!("{base_ws}&{query}")
        } else {
            format!("{base_ws}?{query}")
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeepgramError {
    #[error("credentials missing")]
    CredentialsMissing,
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("send failed: {0}")]
    SendFailed(String),
    #[error("task failed: {0}")]
    TaskFailed(String),
    #[error("no final result")]
    NoFinalResult,
    #[error("final result timed out")]
    FinalResultTimeout,
}

enum SendItem {
    Audio(Vec<u8>),
    CloseStream,
}

#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    audio_scratch: Vec<u8>,
    bytes_received: u64,
    session_started: bool,
    session_finished: bool,
    session_start_error: Option<String>,
    runtime: Option<Handle>,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, DeepgramError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    completed_segments: Vec<String>,
    interim_transcript: String,
    finishing: bool,
}

pub struct DeepgramRealtimeASR {
    credentials: DeepgramCredentials,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, DeepgramError>>>>,
    session_started: Arc<Notify>,
    session_finished: Arc<Notify>,
}

impl DeepgramRealtimeASR {
    pub fn new(credentials: DeepgramCredentials) -> Self {
        Self {
            credentials,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            session_started: Arc::new(Notify::new()),
            session_finished: Arc::new(Notify::new()),
        }
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), DeepgramError> {
        if self.credentials.api_key.trim().is_empty() {
            return Err(DeepgramError::CredentialsMissing);
        }

        let url = self.credentials.connect_url();
        let mut request = url
            .into_client_request()
            .map_err(|e| DeepgramError::ConnectionFailed(e.to_string()))?;

        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Token {}", self.credentials.api_key.trim()))
                .map_err(|e| DeepgramError::ConnectionFailed(e.to_string()))?,
        );

        let (ws, _resp) = connect_async(request)
            .await
            .map_err(|e| DeepgramError::ConnectionFailed(e.to_string()))?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.runtime = Some(Handle::current());
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
            st.send_tx = Some(send_tx);
            st.session_started = true;
        }
        *self.final_rx.lock() = Some(final_rx);
        self.session_started.notify_waiters();

        let writer_for_worker = Arc::clone(&self.writer);
        let weak_self_for_worker = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(item) = send_rx.recv().await {
                let res = match item {
                    SendItem::Audio(chunk) => {
                        send_binary(&writer_for_worker, chunk).await
                    }
                    SendItem::CloseStream => {
                        send_text(&writer_for_worker, json!({"type": "CloseStream"}).to_string()).await
                    }
                };
                if let Err(error) = res {
                    log::error!("[deepgram-asr] send worker failed: {error}");
                    if let Some(this) = weak_self_for_worker.upgrade() {
                        this.finish_error(error);
                    }
                    break;
                }
            }
        });

        let weak_self = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut read = read;
            while let Some(msg) = read.next().await {
                let Some(this) = weak_self.upgrade() else {
                    break;
                };
                match msg {
                    Ok(Message::Text(text)) => {
                        if !this.handle_text_message(&text) {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        this.finish_success();
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("[deepgram-asr] receive loop error: {e}");
                        this.finish_with_partial_or_error(DeepgramError::ConnectionFailed(
                            e.to_string(),
                        ));
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn send_last_frame(self: &Arc<Self>) -> Result<(), DeepgramError> {
        let result = tokio::time::timeout(FINAL_RESULT_TIMEOUT, async {
            let finished = self.session_finished.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();

            let send_tx = {
                let mut st = self.state.lock();
                if !st.pending_audio.is_empty() {
                    let pending = std::mem::take(&mut st.pending_audio);
                    st.audio_scratch.extend_from_slice(&pending);
                }
                let tail = std::mem::take(&mut st.audio_scratch);
                let send_tx = st.send_tx.clone();
                st.finishing = true;
                if let Some(tx) = &send_tx {
                    if !tail.is_empty() {
                        let _ = tx.send(SendItem::Audio(tail));
                    }
                    let _ = tx.send(SendItem::CloseStream);
                }
                send_tx
            };

            if send_tx.is_none() {
                return Ok(());
            }

            if !self.state.lock().session_finished {
                finished.await;
            }
            Ok(())
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => {
                self.finish_with_partial_or_error(DeepgramError::FinalResultTimeout);
                Ok(())
            }
        }
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, DeepgramError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(DeepgramError::NoFinalResult);
        };
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, rx)
            .await
            .map_err(|_| DeepgramError::FinalResultTimeout)?
            .map_err(|_| DeepgramError::NoFinalResult)?
    }

    pub fn cancel(&self) {
        let mut st = self.state.lock();
        st.pending_audio.clear();
        st.audio_scratch.clear();
        st.send_tx.take();
        st.final_tx.take();
        st.session_finished = true;
        drop(st);
        let writer = Arc::clone(&self.writer);
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                let _ = close_writer(&writer).await;
            });
        } else {
            std::thread::spawn(move || {
                if let Ok(rt) = tokio::runtime::Runtime::new() {
                    rt.block_on(async move {
                        let _ = close_writer(&writer).await;
                    });
                }
            });
        }
    }

    fn handle_text_message(&self, text: &str) -> bool {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[deepgram-asr] invalid json event: {e}");
                return true;
            }
        };
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "Results" => {
                let is_final = value
                    .get("is_final")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let speech_final = value
                    .get("speech_final")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                let transcript = value
                    .get("channel")
                    .and_then(|c| c.get("alternatives"))
                    .and_then(|a| a.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|alt| alt.get("transcript"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();

                let mut st = self.state.lock();
                if is_final || speech_final {
                    if !transcript.is_empty() {
                        st.completed_segments.push(transcript.to_string());
                    }
                    st.interim_transcript.clear();
                } else if !transcript.is_empty() {
                    st.interim_transcript = transcript.to_string();
                }
                true
            }
            "Error" => {
                let err_msg = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Deepgram streaming error");
                self.finish_with_partial_or_error(DeepgramError::TaskFailed(err_msg.to_string()));
                false
            }
            _ => true,
        }
    }

    fn finish_success(&self) {
        let (tx, text, duration_ms) = {
            let mut st = self.state.lock();
            if st.session_finished {
                return;
            }
            st.session_finished = true;
            st.send_tx.take();

            let mut segments = std::mem::take(&mut st.completed_segments);
            if !st.interim_transcript.is_empty() {
                segments.push(std::mem::take(&mut st.interim_transcript));
            }
            let text = join_segments(&segments);
            let duration_ms = if st.bytes_received > 0 {
                st.bytes_received / BYTES_PER_MS
            } else {
                st.start
                    .map(|start| start.elapsed().as_millis() as u64)
                    .unwrap_or_default()
            };
            (st.final_tx.take(), text, duration_ms)
        };
        if let Some(tx) = tx {
            let _ = tx.send(Ok(RawTranscript { text, duration_ms }));
        }
        self.session_finished.notify_waiters();
        self.close_on_runtime();
    }

    fn finish_with_partial_or_error(&self, error: DeepgramError) {
        let has_partial = {
            let st = self.state.lock();
            !st.completed_segments.is_empty() || !st.interim_transcript.trim().is_empty()
        };
        if has_partial {
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_error(&self, error: DeepgramError) {
        let tx = {
            let mut st = self.state.lock();
            if st.session_finished {
                return;
            }
            st.session_finished = true;
            st.send_tx.take();
            st.final_tx.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(Err(error));
        }
        self.session_finished.notify_waiters();
        self.close_on_runtime();
    }

    fn close_on_runtime(&self) {
        let writer = Arc::clone(&self.writer);
        if let Some(handle) = self.state.lock().runtime.clone() {
            handle.spawn(async move {
                let _ = close_writer(&writer).await;
            });
        }
    }
}

impl AudioConsumer for DeepgramRealtimeASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        let (send_tx, chunks) = {
            let mut st = self.state.lock();
            st.bytes_received = st.bytes_received.saturating_add(pcm.len() as u64);
            if !st.session_started {
                st.pending_audio.extend_from_slice(pcm);
                return;
            }
            st.audio_scratch.extend_from_slice(pcm);
            let chunks = drain_audio_chunks(&mut st.audio_scratch);
            (st.send_tx.clone(), chunks)
        };
        if let Some(tx) = send_tx {
            for chunk in chunks {
                let _ = tx.send(SendItem::Audio(chunk));
            }
        }
    }
}

fn drain_audio_chunks(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    while buffer.len() >= TARGET_AUDIO_CHUNK_BYTES {
        chunks.push(buffer.drain(..TARGET_AUDIO_CHUNK_BYTES).collect());
    }
    chunks
}

async fn send_binary(writer: &SharedWriter, data: Vec<u8>) -> Result<(), DeepgramError> {
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut guard = writer.lock().await;
        let Some(ws) = guard.as_mut() else {
            return Err(DeepgramError::ConnectionFailed(
                "websocket writer not available".to_string(),
            ));
        };
        ws.send(Message::Binary(data))
            .await
            .map_err(|e| DeepgramError::SendFailed(e.to_string()))
    })
    .await
    .map_err(|_| DeepgramError::SendFailed("websocket write timed out".to_string()))?
}

async fn send_text(writer: &SharedWriter, text: String) -> Result<(), DeepgramError> {
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut guard = writer.lock().await;
        let Some(ws) = guard.as_mut() else {
            return Err(DeepgramError::ConnectionFailed(
                "websocket writer not available".to_string(),
            ));
        };
        ws.send(Message::Text(text))
            .await
            .map_err(|e| DeepgramError::SendFailed(e.to_string()))
    })
    .await
    .map_err(|_| DeepgramError::SendFailed("websocket write timed out".to_string()))?
}

async fn close_writer(writer: &SharedWriter) -> Result<(), DeepgramError> {
    let mut guard = writer.lock().await;
    if let Some(mut ws) = guard.take() {
        let _ = ws.close().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_asr() -> DeepgramRealtimeASR {
        DeepgramRealtimeASR::new(DeepgramCredentials {
            api_key: "test-key".to_string(),
            endpoint: String::new(),
            model: "nova-3".to_string(),
            language: Some("zh".to_string()),
        })
    }

    #[test]
    fn connect_url_formats_query_params() {
        let creds = DeepgramCredentials {
            api_key: "k".to_string(),
            endpoint: String::new(),
            model: "nova-3".to_string(),
            language: Some("zh".to_string()),
        };
        assert!(creds.connect_url().contains("model=nova-3"));
        assert!(creds.connect_url().contains("language=zh"));
        assert!(creds.connect_url().contains("smart_format=true"));
    }

    #[test]
    fn handles_results_events() {
        let asr = create_test_asr();
        let event = json!({
            "type": "Results",
            "is_final": true,
            "speech_final": true,
            "channel": {
                "alternatives": [{ "transcript": "你好世界" }]
            }
        })
        .to_string();

        assert!(asr.handle_text_message(&event));
        assert_eq!(asr.state.lock().completed_segments, vec!["你好世界"]);
    }
}
