//! AssemblyAI Realtime Streaming ASR Provider (v3 WebSocket API).
//!
//! Protocol Specification (AssemblyAI Streaming v3):
//! - WebSocket Endpoint: `wss://streaming.assemblyai.com/v3/ws?speech_model=universal-3-5-pro&sample_rate=16000`
//! - Auth Header: `Authorization: <API_KEY>` (NO `Bearer` prefix).
//! - Binary Frames: 16 kHz / 16-bit / mono PCM audio chunks.
//! - Server Event "Begin": `{"type": "Begin", "id": "..."}`
//! - Server Event "Turn": `{"type": "Turn", "end_of_turn": true|false, "transcript": "..."}`
//! - Client Terminate: Text frame `{"type": "Terminate"}`
//! - Server Event "Termination": `{"type": "Termination", "audio_duration_seconds": ...}`
//! - Server Event "Error": `{"type": "Error", "error": "..."}`

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

pub const PROVIDER_ID: &str = "assemblyai";
pub const DEFAULT_ENDPOINT: &str = "wss://streaming.assemblyai.com/v3/ws";
pub const DEFAULT_MODEL: &str = "universal-3-5-pro";

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
pub struct AssemblyAICredentials {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
}

impl AssemblyAICredentials {
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
        if base_ws.contains('?') {
            format!("{base_ws}&speech_model={model}&sample_rate=16000")
        } else {
            format!("{base_ws}?speech_model={model}&sample_rate=16000")
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssemblyAIError {
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
    Terminate,
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
    final_tx: Option<oneshot::Sender<Result<RawTranscript, AssemblyAIError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    completed_turns: Vec<String>,
    interim_transcript: String,
    finishing: bool,
}

pub struct AssemblyAIRealtimeASR {
    credentials: AssemblyAICredentials,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, AssemblyAIError>>>>,
    session_started: Arc<Notify>,
    session_finished: Arc<Notify>,
}

impl AssemblyAIRealtimeASR {
    pub fn new(credentials: AssemblyAICredentials) -> Self {
        Self {
            credentials,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            session_started: Arc::new(Notify::new()),
            session_finished: Arc::new(Notify::new()),
        }
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), AssemblyAIError> {
        if self.credentials.api_key.trim().is_empty() {
            return Err(AssemblyAIError::CredentialsMissing);
        }

        let url = self.credentials.connect_url();
        let mut request = url
            .into_client_request()
            .map_err(|e| AssemblyAIError::ConnectionFailed(e.to_string()))?;

        // AssemblyAI requires Authorization: <API_KEY> with NO Bearer prefix
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(self.credentials.api_key.trim())
                .map_err(|e| AssemblyAIError::ConnectionFailed(e.to_string()))?,
        );

        let (ws, _resp) = connect_async(request)
            .await
            .map_err(|e| AssemblyAIError::ConnectionFailed(e.to_string()))?;
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
        }
        *self.final_rx.lock() = Some(final_rx);

        let writer_for_worker = Arc::clone(&self.writer);
        let weak_self_for_worker = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(item) = send_rx.recv().await {
                let res = match item {
                    SendItem::Audio(chunk) => {
                        send_binary(&writer_for_worker, chunk).await
                    }
                    SendItem::Terminate => {
                        send_text(&writer_for_worker, json!({"type": "Terminate"}).to_string()).await
                    }
                };
                if let Err(error) = res {
                    log::error!("[assemblyai-asr] send worker failed: {error}");
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
                        this.fail_session_start("WebSocket closed before session began");
                        this.finish_with_partial_or_error(AssemblyAIError::NoFinalResult);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("[assemblyai-asr] receive loop error: {e}");
                        this.fail_session_start(&e.to_string());
                        this.finish_with_partial_or_error(AssemblyAIError::ConnectionFailed(
                            e.to_string(),
                        ));
                        break;
                    }
                }
            }
        });

        let started = self.session_started.notified();
        tokio::pin!(started);
        started.as_mut().enable();

        let ready_result = if !self.state.lock().session_started {
            tokio::time::timeout(SESSION_READY_TIMEOUT, started)
                .await
                .map_err(|_| AssemblyAIError::FinalResultTimeout)
        } else {
            Ok(())
        };
        if let Err(error) = ready_result {
            self.cancel();
            return Err(error);
        }
        if let Some(error) = self.state.lock().session_start_error.clone() {
            self.cancel();
            return Err(AssemblyAIError::TaskFailed(error));
        }

        Ok(())
    }

    pub async fn send_last_frame(self: &Arc<Self>) -> Result<(), AssemblyAIError> {
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
                    let _ = tx.send(SendItem::Terminate);
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
                self.finish_with_partial_or_error(AssemblyAIError::FinalResultTimeout);
                Ok(())
            }
        }
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, AssemblyAIError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(AssemblyAIError::NoFinalResult);
        };
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, rx)
            .await
            .map_err(|_| AssemblyAIError::FinalResultTimeout)?
            .map_err(|_| AssemblyAIError::NoFinalResult)?
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
                log::warn!("[assemblyai-asr] invalid json event: {e}");
                return true;
            }
        };
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "Begin" => {
                self.mark_session_started();
                true
            }
            "Turn" => {
                let transcript = value
                    .get("transcript")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let end_of_turn = value
                    .get("end_of_turn")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                let mut st = self.state.lock();
                if end_of_turn {
                    if !transcript.is_empty() {
                        st.completed_turns.push(transcript.to_string());
                    }
                    st.interim_transcript.clear();
                } else if !transcript.is_empty() {
                    st.interim_transcript = transcript.to_string();
                }
                true
            }
            "Termination" => {
                self.finish_success();
                false
            }
            "Error" => {
                let err_msg = value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("AssemblyAI streaming error");
                self.finish_with_partial_or_error(AssemblyAIError::TaskFailed(err_msg.to_string()));
                false
            }
            _ => true,
        }
    }

    fn mark_session_started(&self) {
        let (send_tx, chunks) = {
            let mut st = self.state.lock();
            st.session_started = true;
            if !st.pending_audio.is_empty() {
                let pending = std::mem::take(&mut st.pending_audio);
                st.audio_scratch.extend_from_slice(&pending);
            }
            let send_tx = st.send_tx.clone();
            let chunks = drain_audio_chunks(&mut st.audio_scratch);
            (send_tx, chunks)
        };
        if let Some(tx) = send_tx {
            for chunk in chunks {
                let _ = tx.send(SendItem::Audio(chunk));
            }
        }
        self.session_started.notify_waiters();
    }

    fn fail_session_start(&self, error: &str) {
        let mut st = self.state.lock();
        if !st.session_started && st.session_start_error.is_none() {
            st.session_start_error = Some(error.to_string());
            self.session_started.notify_waiters();
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

            let mut turns = std::mem::take(&mut st.completed_turns);
            if !st.interim_transcript.is_empty() {
                turns.push(std::mem::take(&mut st.interim_transcript));
            }
            let text = join_segments(&turns);
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

    fn finish_with_partial_or_error(&self, error: AssemblyAIError) {
        let has_partial = {
            let st = self.state.lock();
            !st.completed_turns.is_empty() || !st.interim_transcript.trim().is_empty()
        };
        if has_partial {
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_error(&self, error: AssemblyAIError) {
        self.fail_session_start(&error.to_string());
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

impl AudioConsumer for AssemblyAIRealtimeASR {
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

async fn send_binary(writer: &SharedWriter, data: Vec<u8>) -> Result<(), AssemblyAIError> {
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut guard = writer.lock().await;
        let Some(ws) = guard.as_mut() else {
            return Err(AssemblyAIError::ConnectionFailed(
                "websocket writer not available".to_string(),
            ));
        };
        ws.send(Message::Binary(data))
            .await
            .map_err(|e| AssemblyAIError::SendFailed(e.to_string()))
    })
    .await
    .map_err(|_| AssemblyAIError::SendFailed("websocket write timed out".to_string()))?
}

async fn send_text(writer: &SharedWriter, text: String) -> Result<(), AssemblyAIError> {
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut guard = writer.lock().await;
        let Some(ws) = guard.as_mut() else {
            return Err(AssemblyAIError::ConnectionFailed(
                "websocket writer not available".to_string(),
            ));
        };
        ws.send(Message::Text(text))
            .await
            .map_err(|e| AssemblyAIError::SendFailed(e.to_string()))
    })
    .await
    .map_err(|_| AssemblyAIError::SendFailed("websocket write timed out".to_string()))?
}

async fn close_writer(writer: &SharedWriter) -> Result<(), AssemblyAIError> {
    let mut guard = writer.lock().await;
    if let Some(mut ws) = guard.take() {
        let _ = ws.close().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_asr() -> AssemblyAIRealtimeASR {
        AssemblyAIRealtimeASR::new(AssemblyAICredentials {
            api_key: "test-api-key".to_string(),
            endpoint: String::new(),
            model: String::new(),
        })
    }

    #[test]
    fn connect_url_formats_query_params() {
        let creds = AssemblyAICredentials {
            api_key: "k".to_string(),
            endpoint: String::new(),
            model: "universal-3-5-pro".to_string(),
        };
        assert_eq!(
            creds.connect_url(),
            "wss://streaming.assemblyai.com/v3/ws?speech_model=universal-3-5-pro&sample_rate=16000"
        );
    }

    #[test]
    fn handles_begin_and_turn_events() {
        let asr = create_test_asr();
        assert!(asr.handle_text_message(r#"{"type":"Begin","id":"sess-123"}"#));
        assert!(asr.state.lock().session_started);

        assert!(asr.handle_text_message(r#"{"type":"Turn","end_of_turn":false,"transcript":"Hello"}"#));
        assert_eq!(asr.state.lock().interim_transcript, "Hello");

        assert!(asr.handle_text_message(r#"{"type":"Turn","end_of_turn":true,"transcript":"Hello world."}"#));
        assert_eq!(asr.state.lock().completed_turns, vec!["Hello world."]);
        assert!(asr.state.lock().interim_transcript.is_empty());
    }

    #[test]
    fn handles_termination_event() {
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        asr.handle_text_message(r#"{"type":"Turn","end_of_turn":true,"transcript":"Final sentence."}"#);
        
        let keep_going = asr.handle_text_message(r#"{"type":"Termination","audio_duration_seconds":2.5}"#);
        assert!(!keep_going);

        let res = rx.try_recv().unwrap().unwrap();
        assert_eq!(res.text, "Final sentence.");
    }
}
