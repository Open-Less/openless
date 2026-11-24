//! Alibaba Cloud Bailian / DashScope realtime ASR client.
//!
//! Uses the classic DashScope realtime recognition WebSocket protocol
//! (`/api-ws/v1/inference`) because it accepts raw 16 kHz mono PCM frames and
//! matches OpenLess' recorder output directly. The Qwen OpenAI Realtime line is
//! a different protocol and is intentionally left for a follow-up provider.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use serde_json::{json, Value};
use tokio::net::{lookup_host, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::client_async_tls;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderValue;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::config::{TaskSpawner, TokioTaskSpawner};

use super::stream_send::{send_ws_message_with_timeout, PacedSendClock, SharedWriter, WsStream};
use super::{AudioConsumer, RawTranscript};
use crate::ports::{TextStreamChunk, TextStreamSink};

pub const PROVIDER_ID: &str = "bailian";
pub const DEFAULT_ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference/";
pub const DEFAULT_MODEL: &str = "fun-asr-realtime";

/// 100 ms of 16 kHz / 16-bit / mono PCM.
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 3_200;
const TARGET_AUDIO_CHUNK_BYTES_8K: usize = 1_600;
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
const FRAME_SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// Cap on the WebSocket handshake (TCP + TLS + HTTP upgrade) itself. Without it
/// `connect_async` waits forever, and `open_session` is awaited with `block_on` on
/// the serial hotkey bridge thread — a hang means hotkeys are completely dead
/// (recording can neither start nor stop; the app must be restarted). See the same
/// constant in stepfun_realtime.rs.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// TCP cap per candidate address. On IPv6 black holes the whole 5s budget must not be
/// spent on the first AAAA.
const PER_ADDR_TCP_TIMEOUT: Duration = Duration::from_millis(1500);

fn default_port_for_request(
    request: &tokio_tungstenite::tungstenite::handshake::client::Request,
) -> Result<u16, Box<WsError>> {
    let default_port = match request.uri().scheme_str() {
        Some("ws") => 80,
        Some("wss") => 443,
        _ => {
            return Err(WsError::Url(
                tokio_tungstenite::tungstenite::error::UrlError::UnsupportedUrlScheme,
            )
            .into())
        }
    };
    Ok(request.uri().port_u16().unwrap_or(default_port))
}

fn order_connect_addrs(mut addrs: Vec<std::net::SocketAddr>) -> Vec<std::net::SocketAddr> {
    addrs.sort_by_key(|addr| u8::from(!addr.ip().is_ipv4()));
    addrs
}

async fn connect_ws_to_addrs(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    addrs: Vec<std::net::SocketAddr>,
) -> Result<
    (
        WsStream,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    Box<WsError>,
> {
    if addrs.is_empty() {
        return Err(WsError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no addresses for websocket endpoint",
        ))
        .into());
    }

    let mut last_err = None;
    for addr in addrs {
        match tokio::time::timeout(PER_ADDR_TCP_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => match client_async_tls(request.clone(), stream).await {
                Ok(connection) => return Ok(connection),
                Err(error) => last_err = Some(error),
            },
            Ok(Err(error)) => last_err = Some(WsError::Io(error)),
            Err(_) => {
                last_err = Some(WsError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "tcp connect timeout ({})",
                        if addr.ip().is_ipv4() { "v4" } else { "v6" }
                    ),
                )))
            }
        }
    }

    Err(last_err
        .unwrap_or_else(|| {
            WsError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "no tcp candidate",
            ))
        })
        .into())
}

async fn connect_ws_prefer_ipv4(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
) -> Result<
    (
        WsStream,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    Box<WsError>,
> {
    let port = default_port_for_request(&request)?;
    let host = request.uri().host().unwrap_or("").to_string();
    let addrs = lookup_host((host.as_str(), port))
        .await
        .map_err(WsError::Io)?
        .collect::<Vec<_>>();
    connect_ws_to_addrs(request, order_connect_addrs(addrs)).await
}

#[derive(Clone, Debug)]
pub struct BailianCredentials {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub vocabulary_id: Option<String>,
}

impl BailianCredentials {
    pub fn normalized_endpoint(&self) -> String {
        if self.endpoint.trim().is_empty() {
            return DEFAULT_ENDPOINT.to_string();
        }
        self.endpoint.trim().to_string()
    }

    pub fn normalized_model(&self) -> String {
        let model = self.model.trim();
        if model.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model.to_string()
        }
    }
}

/// Bailian realtime ASR uses the DashScope WebSocket gateway, which accepts only
/// ws:// or wss:// endpoints. Users often paste the `https://` compatibility-mode /
/// dedicated-domain URL from the Bailian console — a different HTTP protocol whose
/// WebSocket handshake always fails, with an unreadable underlying error
/// ("URL scheme not supported"). Intercept at validation entry points so the frontend
/// can show an actionable hint via the `bailianEndpointSchemeInvalid` error code.
pub fn endpoint_scheme_is_websocket(endpoint: &str) -> bool {
    let lower = endpoint.trim().to_ascii_lowercase();
    lower.starts_with("wss://") || lower.starts_with("ws://")
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum BailianASRError {
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
    Audio { payload: Vec<u8>, input_bytes: u64 },
    Finish(oneshot::Sender<Result<(), BailianASRError>>),
}

#[derive(Default)]
struct SyncState {
    task_id: String,
    pending_audio: Vec<u8>,
    audio_scratch: Vec<u8>,
    downsample_remainder: Vec<u8>,
    bytes_received: u64,
    bytes_sent_input: u64,
    task_started: bool,
    task_finished: bool,
    finish_sent: bool,
    ending: bool,
    send_error: Option<BailianASRError>,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, BailianASRError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    cancel_tx: Option<watch::Sender<bool>>,
    /// sentence_id -> text; joined in sentence_id order to form the final text. A
    /// later result for the same sentence_id overwrites the previous one, eliminating
    /// the duplication caused by cumulative text.
    final_segments: BTreeMap<i64, String>,
    /// sentence_id -> interim text, updated while sentence_end == false; when the
    /// final result for that sentence_id arrives, the content moves into
    /// final_segments.
    partial_segments: BTreeMap<i64, String>,
    last_result_text: String,
}

pub struct BailianRealtimeASR {
    credentials: BailianCredentials,
    frame_send_timeout: Duration,
    task_spawner: Arc<dyn TaskSpawner>,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, BailianASRError>>>>,
    task_started: Arc<Notify>,
    partial_sink: ParkingMutex<Option<Arc<dyn TextStreamSink>>>,
}

impl BailianRealtimeASR {
    pub fn new(credentials: BailianCredentials) -> Self {
        Self::with_task_spawner(credentials, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: BailianCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            credentials,
            frame_send_timeout: FRAME_SEND_TIMEOUT,
            task_spawner,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            task_started: Arc::new(Notify::new()),
            partial_sink: ParkingMutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_frame_send_timeout(mut self, timeout: Duration) -> Self {
        self.frame_send_timeout = timeout;
        self
    }

    fn is_8k_model(&self) -> bool {
        model_is_8k(&self.credentials.normalized_model())
    }

    pub fn audio_coverage(&self) -> (u64, u64) {
        let st = self.state.lock();
        (st.bytes_sent_input, st.bytes_received)
    }

    pub fn is_stream_complete(&self) -> bool {
        let st = self.state.lock();
        let max_tail_discard = if self.is_8k_model() { 2 } else { 0 };
        st.send_error.is_none()
            && st.finish_sent
            && st.bytes_received.saturating_sub(st.bytes_sent_input) <= max_tail_discard
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.partial_sink.lock() = Some(sink);
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), BailianASRError> {
        if self.credentials.api_key.trim().is_empty() {
            return Err(BailianASRError::CredentialsMissing);
        }

        let task_id = Uuid::new_v4().simple().to_string();
        let endpoint = self.credentials.normalized_endpoint();
        let mut request = endpoint
            .into_client_request()
            .map_err(|e| BailianASRError::ConnectionFailed(e.to_string()))?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("bearer {}", self.credentials.api_key.trim()))
                .map_err(|e| BailianASRError::ConnectionFailed(e.to_string()))?,
        );

        let (ws, _resp) = tokio::time::timeout(CONNECT_TIMEOUT, connect_ws_prefer_ipv4(request))
            .await
            .map_err(|_| {
                BailianASRError::ConnectionFailed(format!(
                    "连接超时（{} ms）",
                    CONNECT_TIMEOUT.as_millis()
                ))
            })?
            .map_err(|e| BailianASRError::ConnectionFailed(e.to_string()))?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.task_id = task_id.clone();
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
            st.send_tx = Some(send_tx);
            st.cancel_tx = Some(cancel_tx);
        }
        *self.final_rx.lock() = Some(final_rx);

        let writer_for_worker = Arc::clone(&self.writer);
        let weak_self_for_worker = Arc::downgrade(self);
        let task_id_for_worker = task_id.clone();
        let frame_send_timeout = self.frame_send_timeout;
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            let mut clock = PacedSendClock::new();
            while let Some(item) = send_rx.recv().await {
                match item {
                    SendItem::Audio {
                        payload,
                        input_bytes,
                    } => {
                        let Some(scheduled_start) = clock.wait_slot(&mut cancel_rx).await else {
                            break;
                        };
                        if let Err(ws_err) = send_ws_message_with_timeout(
                            &writer_for_worker,
                            Message::Binary(payload.into()),
                            frame_send_timeout,
                            &mut cancel_rx,
                        )
                        .await
                        {
                            let err = BailianASRError::SendFailed(
                                ws_err.into_message("websocket writer not available"),
                            );
                            log::error!("[bailian-asr] audio frame send failed: {err}");
                            if let Some(this) = weak_self_for_worker.upgrade() {
                                this.record_send_failure(err);
                            }
                            // Abort before finish-task can overtake incomplete audio.
                            break;
                        }
                        if let Some(this) = weak_self_for_worker.upgrade() {
                            let mut st = this.state.lock();
                            st.bytes_sent_input = st.bytes_sent_input.saturating_add(input_bytes);
                        }
                        clock.mark_sent(scheduled_start, input_bytes as usize, BYTES_PER_MS, 2);
                    }
                    SendItem::Finish(done) => {
                        let result = send_ws_message_with_timeout(
                            &writer_for_worker,
                            Message::Text(finish_task_message(&task_id_for_worker).into()),
                            frame_send_timeout,
                            &mut cancel_rx,
                        )
                        .await
                        .map_err(|ws_err| {
                            BailianASRError::SendFailed(
                                ws_err.into_message("websocket writer not available"),
                            )
                        });
                        match &result {
                            Ok(()) => {
                                if let Some(this) = weak_self_for_worker.upgrade() {
                                    this.state.lock().finish_sent = true;
                                }
                            }
                            Err(err) => {
                                log::error!("[bailian-asr] finish-task send failed: {err}");
                                if let Some(this) = weak_self_for_worker.upgrade() {
                                    this.record_send_failure(err.clone());
                                }
                            }
                        }
                        let _ = done.send(result);
                        break;
                    }
                }
            }
        }));

        if let Err(error) = send_text(
            &self.writer,
            run_task_message(
                &task_id,
                &self.credentials.normalized_model(),
                self.credentials.vocabulary_id.as_deref(),
            ),
        )
        .await
        {
            self.cancel();
            return Err(error);
        }

        let weak_self = Arc::downgrade(self);
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
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
                        this.finish_with_partial_or_error(BailianASRError::NoFinalResult);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("[bailian-asr] receive loop error: {e}");
                        this.finish_with_partial_or_error(BailianASRError::ConnectionFailed(
                            e.to_string(),
                        ));
                        break;
                    }
                }
            }
        }));

        Ok(())
    }

    pub async fn send_last_frame(&self) -> Result<(), BailianASRError> {
        let started = self.task_started.notified();
        tokio::pin!(started);
        started.as_mut().enable();
        let ready = {
            let st = self.state.lock();
            st.task_started || st.task_finished || st.send_error.is_some()
        };
        if !ready {
            tokio::time::timeout(Duration::from_secs(5), started)
                .await
                .map_err(|_| BailianASRError::FinalResultTimeout)?;
        }
        let model = self.credentials.normalized_model();
        let input_scale = if model_is_8k(&model) { 2u64 } else { 1u64 };
        let (done_tx, done_rx) = oneshot::channel();
        {
            let mut st = self.state.lock();
            if let Some(ref error) = st.send_error {
                return Err(error.clone());
            }
            let Some(send_tx) = st.send_tx.take() else {
                return Err(BailianASRError::SendFailed(
                    "websocket session is not open".to_string(),
                ));
            };
            st.ending = true;
            if model_is_8k(&model) {
                clear_downsample_tail(&mut st.downsample_remainder);
            }
            if !st.pending_audio.is_empty() {
                let pending = std::mem::take(&mut st.pending_audio);
                st.audio_scratch.extend_from_slice(&pending);
            }
            let mut tail_chunks = drain_audio_chunks_for_model(&mut st.audio_scratch, &model);
            if !st.audio_scratch.is_empty() {
                tail_chunks.push(std::mem::take(&mut st.audio_scratch));
            }
            for payload in tail_chunks {
                let input_bytes = (payload.len() as u64).saturating_mul(input_scale);
                if send_tx
                    .send(SendItem::Audio {
                        payload,
                        input_bytes,
                    })
                    .is_err()
                {
                    return Err(st.send_error.clone().unwrap_or_else(|| {
                        BailianASRError::SendFailed("send worker closed".to_string())
                    }));
                }
            }
            if send_tx.send(SendItem::Finish(done_tx)).is_err() {
                return Err(st.send_error.clone().unwrap_or_else(|| {
                    BailianASRError::SendFailed("send worker closed".to_string())
                }));
            }
        }
        match done_rx.await {
            Ok(inner) => inner,
            Err(_) => {
                Err(self.state.lock().send_error.clone().unwrap_or_else(|| {
                    BailianASRError::SendFailed("finish ack dropped".to_string())
                }))
            }
        }
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, BailianASRError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(BailianASRError::NoFinalResult);
        };
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, rx)
            .await
            .map_err(|_| BailianASRError::FinalResultTimeout)?
            .map_err(|_| BailianASRError::NoFinalResult)?
    }

    pub fn cancel(&self) {
        let mut st = self.state.lock();
        st.pending_audio.clear();
        st.audio_scratch.clear();
        st.downsample_remainder.clear();
        st.send_tx.take();
        st.final_tx.take();
        st.task_finished = true;
        if let Some(tx) = st.cancel_tx.as_ref() {
            let _ = tx.send(true);
        }
        drop(st);
        self.task_started.notify_waiters();
        let writer = Arc::clone(&self.writer);
        self.task_spawner.spawn(Box::pin(async move {
            let _ = close_writer(&writer).await;
        }));
    }

    fn handle_text_message(&self, text: &str) -> bool {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[bailian-asr] invalid json event: {e}");
                return true;
            }
        };
        let event = value
            .get("header")
            .and_then(|h| h.get("event"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event {
            "task-started" => {
                self.mark_task_started();
                true
            }
            "result-generated" => {
                self.record_result(&value);
                true
            }
            "task-finished" => {
                self.finish_success();
                false
            }
            "task-failed" => {
                let message = value
                    .get("header")
                    .and_then(|h| h.get("error_message"))
                    .and_then(Value::as_str)
                    .unwrap_or("task failed")
                    .to_string();
                self.finish_error(BailianASRError::TaskFailed(message));
                false
            }
            _ => true,
        }
    }

    fn mark_task_started(&self) {
        let model = self.credentials.normalized_model();
        let input_scale = if model_is_8k(&model) { 2u64 } else { 1u64 };
        let (send_tx, chunks) = {
            let mut st = self.state.lock();
            st.task_started = true;
            if !st.pending_audio.is_empty() {
                let pending = std::mem::take(&mut st.pending_audio);
                st.audio_scratch.extend_from_slice(&pending);
            }
            let send_tx = st.send_tx.clone();
            let chunks = drain_audio_chunks_for_model(&mut st.audio_scratch, &model);
            (send_tx, chunks)
        };
        if let Some(tx) = send_tx {
            for payload in chunks {
                let input_bytes = (payload.len() as u64).saturating_mul(input_scale);
                let _ = tx.send(SendItem::Audio {
                    payload,
                    input_bytes,
                });
            }
        }
        self.task_started.notify_waiters();
    }

    fn record_result(&self, value: &Value) {
        let sentence = value
            .get("payload")
            .and_then(|p| p.get("output"))
            .and_then(|o| o.get("sentence"));
        let Some(sentence) = sentence else {
            return;
        };

        // Skip heartbeat events (no recognized text).
        if sentence
            .get("heartbeat")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return;
        }

        let Some(text) = sentence.get("text").and_then(Value::as_str) else {
            return;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        // Use the API-documented sentence_end as the finality signal. end_time > 0 is
        // only a compatibility fallback when the sentence_end field is entirely
        // absent, because DashScope interim results also carry a positive end_time
        // (growing with the audio); falling back unconditionally would misjudge
        // interim results as final and re-create the cumulative-text duplication.
        let sentence_end_val = sentence.get("sentence_end");
        let sentence_end = sentence_end_val.and_then(Value::as_bool).unwrap_or(false);
        let end_time = sentence
            .get("end_time")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let is_sentence_final = match sentence_end_val {
            Some(_) => sentence_end,
            None => end_time > 0,
        };

        let sentence_id = sentence
            .get("sentence_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        let snapshot = {
            let mut st = self.state.lock();
            st.last_result_text = trimmed.to_string();
            if is_sentence_final {
                st.final_segments.insert(sentence_id, trimmed.to_string());
                st.partial_segments.remove(&sentence_id);
            } else {
                st.partial_segments.insert(sentence_id, trimmed.to_string());
            }
            let mut segments = st.partial_segments.clone();
            segments.extend(st.final_segments.clone());
            merge_segments(&segments.into_values().collect::<Vec<_>>())
        };
        if let Some(sink) = self.partial_sink.lock().clone() {
            let _ = sink.publish(TextStreamChunk {
                text: snapshot,
                offset: 0,
            });
        }
    }

    fn finish_success(&self) {
        let (tx, text, duration_ms) = {
            let mut st = self.state.lock();
            if st.task_finished {
                return;
            }
            st.task_finished = true;
            st.send_tx.take();
            if let Some(cancel_tx) = st.cancel_tx.as_ref() {
                let _ = cancel_tx.send(true);
            }
            let text = if st.final_segments.is_empty() {
                st.last_result_text.clone()
            } else {
                let segments: Vec<String> = st.final_segments.values().cloned().collect();
                merge_segments(&segments)
            };
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
        self.task_started.notify_waiters();
        self.close_on_runtime();
    }

    fn finish_with_partial_or_error(&self, error: BailianASRError) {
        let has_partial = {
            let mut st = self.state.lock();
            if st.send_error.is_none() {
                st.send_error = Some(error.clone());
            }
            !st.last_result_text.trim().is_empty()
                || !st.final_segments.is_empty()
                || !st.partial_segments.is_empty()
        };
        if has_partial {
            // Consistent with Volcengine: when the connection breaks on a short session
            // with an existing partial, keep it available via await_final_result while
            // retaining st.send_error for coverage/long-recording checks.
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn record_send_failure(&self, error: BailianASRError) {
        {
            let mut st = self.state.lock();
            if st.send_error.is_none() {
                st.send_error = Some(error.clone());
            }
            st.send_tx.take();
            st.pending_audio.clear();
            st.audio_scratch.clear();
        }
        self.finish_with_partial_or_error(error);
    }

    fn finish_error(&self, error: BailianASRError) {
        let tx = {
            let mut st = self.state.lock();
            if st.send_error.is_none() {
                st.send_error = Some(error.clone());
            }
            if st.task_finished {
                return;
            }
            st.task_finished = true;
            st.send_tx.take();
            if let Some(cancel_tx) = st.cancel_tx.as_ref() {
                let _ = cancel_tx.send(true);
            }
            st.final_tx.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(Err(error));
        }
        self.task_started.notify_waiters();
        self.close_on_runtime();
    }

    fn close_on_runtime(&self) {
        let writer = Arc::clone(&self.writer);
        self.task_spawner.spawn(Box::pin(async move {
            let _ = close_writer(&writer).await;
        }));
    }
}

impl AudioConsumer for BailianRealtimeASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        let model = self.credentials.normalized_model();
        let input_scale = if model_is_8k(&model) { 2u64 } else { 1u64 };
        let (send_tx, chunks) = {
            let mut st = self.state.lock();
            if st.ending {
                return;
            }
            st.bytes_received = st.bytes_received.saturating_add(pcm.len() as u64);
            if st.task_finished || st.send_error.is_some() {
                return;
            }
            let audio = if model_is_8k(&model) {
                st.downsample_remainder.extend_from_slice(pcm);
                let complete_len = st.downsample_remainder.len() / 4 * 4;
                let audio = downsample_pcm_16k_to_8k(&st.downsample_remainder[..complete_len]);
                st.downsample_remainder.drain(..complete_len);
                audio
            } else {
                pcm.to_vec()
            };
            if !st.task_started {
                st.pending_audio.extend_from_slice(&audio);
                return;
            }
            st.audio_scratch.extend_from_slice(&audio);
            let chunks = drain_audio_chunks_for_model(&mut st.audio_scratch, &model);
            (st.send_tx.clone(), chunks)
        };
        if let Some(tx) = send_tx {
            for payload in chunks {
                let input_bytes = (payload.len() as u64).saturating_mul(input_scale);
                if tx
                    .send(SendItem::Audio {
                        payload,
                        input_bytes,
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
fn drain_audio_chunks(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    drain_audio_chunks_with_target(buffer, TARGET_AUDIO_CHUNK_BYTES)
}

fn drain_audio_chunks_for_model(buffer: &mut Vec<u8>, model: &str) -> Vec<Vec<u8>> {
    let target = if model_is_8k(model) {
        TARGET_AUDIO_CHUNK_BYTES_8K
    } else {
        TARGET_AUDIO_CHUNK_BYTES
    };
    drain_audio_chunks_with_target(buffer, target)
}

fn drain_audio_chunks_with_target(buffer: &mut Vec<u8>, target: usize) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    while buffer.len() >= target {
        chunks.push(buffer.drain(..target).collect());
    }
    chunks
}

fn model_is_8k(model: &str) -> bool {
    model.contains("-8k-") || model.starts_with("paraformer-8k-")
}

fn downsample_pcm_16k_to_8k(pcm: &[u8]) -> Vec<u8> {
    // 16 kHz -> 8 kHz: average adjacent samples (first-order low-pass). Compared with
    // plain decimation (dropping every other sample), averaging suppresses aliasing of
    // the 4-8 kHz band into 0-4 kHz, making recognition more stable. i32 summation
    // avoids i16 overflow; output sample count is halved.
    let mut out = Vec::with_capacity(pcm.len() / 2);
    for pair in pcm.as_chunks::<4>().0.iter() {
        let left = i16::from_le_bytes([pair[0], pair[1]]) as i32;
        let right = i16::from_le_bytes([pair[2], pair[3]]) as i32;
        let sample = ((left + right) / 2) as i16;
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn clear_downsample_tail(remainder: &mut Vec<u8>) {
    // Averaging downsample needs sample pairs: a remainder smaller than one pair at
    // close-out (<= 1 16k sample, approx 0.0625ms) cannot be paired and is discarded,
    // keeping later chunk alignment intact.
    remainder.clear();
}

/// Joins text segments with overlap detection: when the start of the next segment
/// overlaps the end of the previous one, only the non-overlapping tail is appended,
/// preventing cumulative-text duplication from API replays or duplicate events.
///
/// Minimum overlap length is 2 chars to avoid single-char coincidences.
/// e.g. ["AB CD", "CD EF"] -> "AB CD EF"
fn merge_segments(segments: &[String]) -> String {
    let mut result = String::new();
    for seg in segments {
        if result.is_empty() {
            result = seg.clone();
            continue;
        }
        let result_chars: Vec<char> = result.chars().collect();
        let seg_chars: Vec<char> = seg.chars().collect();
        let max_overlap = result_chars.len().min(seg_chars.len());
        let mut overlap = 0;
        for n in (2..=max_overlap).rev() {
            if result_chars[result_chars.len() - n..] == seg_chars[..n] {
                overlap = n;
                break;
            }
        }
        let tail: String = seg_chars[overlap..].iter().collect();
        result.push_str(&tail);
    }
    result
}

fn run_task_message(task_id: &str, model: &str, vocabulary_id: Option<&str>) -> String {
    let mut parameters = json!({
        "sample_rate": if model_is_8k(model) { 8000 } else { 16000 },
        "format": "pcm"
    });
    if let Some(vocabulary_id) = vocabulary_id.map(str::trim).filter(|id| !id.is_empty()) {
        parameters["vocabulary_id"] = Value::String(vocabulary_id.to_string());
    }

    json!({
        "header": {
            "action": "run-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": {
            "task_group": "audio",
            "task": "asr",
            "function": "recognition",
            "model": model,
            "parameters": parameters,
            "input": {}
        }
    })
    .to_string()
}

fn finish_task_message(task_id: &str) -> String {
    json!({
        "header": {
            "action": "finish-task",
            "task_id": task_id,
            "streaming": "duplex"
        },
        "payload": { "input": {} }
    })
    .to_string()
}

async fn send_text(writer: &SharedWriter, text: String) -> Result<(), BailianASRError> {
    let mut guard = writer.lock().await;
    let Some(ws) = guard.as_mut() else {
        return Err(BailianASRError::ConnectionFailed(
            "websocket writer not available".to_string(),
        ));
    };
    ws.send(Message::Text(text.into()))
        .await
        .map_err(|e| BailianASRError::SendFailed(e.to_string()))
}

async fn close_writer(writer: &SharedWriter) -> Result<(), BailianASRError> {
    let mut guard = writer.lock().await;
    if let Some(mut ws) = guard.take() {
        let _ = ws.close().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_snapshots_keep_prefix_and_recognition_corrections() {
        let asr = create_test_asr();
        let sink = Arc::new(super::super::TranscriptCapture::default());
        asr.set_partial_sink(sink.clone());
        for (id, text, final_result) in [
            (0, "你", false),
            (0, "你好", false),
            (0, "您好", false),
            (0, "您好。", true),
            (1, "世", false),
            (1, "世界", false),
            (1, "世界！", true),
        ] {
            asr.record_result(&make_result_event(id, text, final_result));
        }
        sink.assert_snapshots(&[
            "你",
            "你好",
            "您好",
            "您好。",
            "您好。世",
            "您好。世界",
            "您好。世界！",
        ]);
    }

    // ---- helpers ----

    fn make_result_event(sentence_id: i64, text: &str, is_final: bool) -> Value {
        json!({
            "payload": {
                "output": {
                    "sentence": {
                        "sentence_id": sentence_id,
                        "text": text,
                        "sentence_end": is_final,
                        // end_time is always positive, matching the real DashScope
                        // API: interim and final both carry positive end_time.
                        "end_time": 1000 + sentence_id * 100
                    }
                }
            }
        })
    }

    fn make_heartbeat_event() -> Value {
        json!({
            "payload": {
                "output": {
                    "sentence": {
                        "heartbeat": true
                    }
                }
            }
        })
    }

    fn create_test_asr() -> BailianRealtimeASR {
        BailianRealtimeASR::new(BailianCredentials {
            api_key: "sk-test".to_string(),
            endpoint: String::new(),
            model: String::new(),
            vocabulary_id: None,
        })
    }

    #[test]
    fn websocket_default_ports_match_uri_scheme() {
        let ws = "ws://localhost/path".into_client_request().unwrap();
        let wss = "wss://localhost/path".into_client_request().unwrap();
        let explicit = "ws://localhost:9000/path".into_client_request().unwrap();

        assert_eq!(default_port_for_request(&ws).unwrap(), 80);
        assert_eq!(default_port_for_request(&wss).unwrap(), 443);
        assert_eq!(default_port_for_request(&explicit).unwrap(), 9000);
    }

    #[test]
    fn websocket_default_port_rejects_non_websocket_scheme() {
        let request = "https://localhost/path".into_client_request().unwrap();
        let explicit_port = "https://localhost:443/path".into_client_request().unwrap();
        assert!(matches!(
            default_port_for_request(&request)
                .as_ref()
                .map_err(|error| error.as_ref()),
            Err(WsError::Url(
                tokio_tungstenite::tungstenite::error::UrlError::UnsupportedUrlScheme
            ))
        ));
        assert!(matches!(
            default_port_for_request(&explicit_port)
                .as_ref()
                .map_err(|error| error.as_ref()),
            Err(WsError::Url(
                tokio_tungstenite::tungstenite::error::UrlError::UnsupportedUrlScheme
            ))
        ));
    }

    #[test]
    fn order_connect_addrs_puts_ipv4_first() {
        let v6: std::net::SocketAddr = "[2408:400a::1]:443".parse().unwrap();
        let v4: std::net::SocketAddr = "8.152.159.24:443".parse().unwrap();
        let ordered = order_connect_addrs(vec![v6, v4]);
        assert!(ordered[0].ip().is_ipv4());
        assert!(ordered[1].ip().is_ipv6());
    }

    #[tokio::test]
    async fn websocket_handshake_failure_falls_back_to_next_address() {
        let rejected_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rejected_addr = rejected_listener.local_addr().unwrap();
        let rejected_task = tokio::spawn(async move {
            let (stream, _) = rejected_listener.accept().await.unwrap();
            drop(stream);
        });

        let accepted_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let accepted_addr = accepted_listener.local_addr().unwrap();
        let accepted_task = tokio::spawn(async move {
            let (stream, _) = accepted_listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap();
        });

        let request = format!("ws://localhost:{}/path", accepted_addr.port())
            .into_client_request()
            .unwrap();
        let (stream, _) = connect_ws_to_addrs(request, vec![rejected_addr, accepted_addr])
            .await
            .unwrap();
        drop(stream);

        rejected_task.await.unwrap();
        accepted_task.await.unwrap();
    }

    #[tokio::test]
    async fn websocket_connects_to_ipv6_candidate_when_available() {
        let Ok(listener) = tokio::net::TcpListener::bind("[::1]:0").await else {
            return;
        };
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap();
        });

        let request = format!("ws://[::1]:{}/path", addr.port())
            .into_client_request()
            .unwrap();
        let (stream, _) = connect_ws_to_addrs(request, vec![addr]).await.unwrap();
        drop(stream);
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn websocket_connect_with_no_candidates_fails_immediately() {
        let request = "ws://localhost/path".into_client_request().unwrap();
        let result = connect_ws_to_addrs(request, Vec::new()).await;
        assert!(
            matches!(result.as_ref().map_err(|error| error.as_ref()), Err(WsError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
        );
    }

    // ---- merge_segments ----

    #[test]
    fn merge_segments_dedupes_overlap() {
        let segments = vec!["你好吗".to_string(), "好吗我们".to_string()];
        assert_eq!(merge_segments(&segments), "你好吗我们");
    }

    #[test]
    fn merge_segments_no_overlap() {
        let segments = vec!["今天".to_string(), "天气真好".to_string()];
        assert_eq!(merge_segments(&segments), "今天天气真好");
    }

    #[test]
    fn merge_segments_single_segment() {
        let segments = vec!["仅一段".to_string()];
        assert_eq!(merge_segments(&segments), "仅一段");
    }

    #[test]
    fn merge_segments_empty_input() {
        let segments: Vec<String> = vec![];
        assert_eq!(merge_segments(&segments), "");
    }

    #[test]
    fn merge_segments_full_overlap() {
        let segments = vec!["重复重复".to_string(), "重复重复".to_string()];
        assert_eq!(merge_segments(&segments), "重复重复");
    }

    #[test]
    fn merge_segments_three_segments_chain() {
        let segments = vec![
            "今天天气".to_string(),
            "天气真不错".to_string(),
            "不错我们出去玩".to_string(),
        ];
        assert_eq!(merge_segments(&segments), "今天天气真不错我们出去玩");
    }

    // ---- record_result: heartbeat ----

    #[test]
    fn heartbeat_event_is_skipped() {
        let asr = create_test_asr();
        asr.record_result(&make_heartbeat_event());
        let st = asr.state.lock();
        assert!(st.last_result_text.is_empty());
        assert!(st.final_segments.is_empty());
        assert!(st.partial_segments.is_empty());
    }

    // ---- record_result: partial / interim ----

    #[test]
    fn partial_results_same_sentence_id_overwrite() {
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "你", false));
        asr.record_result(&make_result_event(1, "你好", false));
        asr.record_result(&make_result_event(1, "你好吗", false));
        let st = asr.state.lock();
        assert!(st.final_segments.is_empty(), "no final yet");
        assert_eq!(st.partial_segments.len(), 1);
        assert_eq!(st.partial_segments.get(&1).unwrap(), "你好吗");
    }

    #[test]
    fn final_result_replaces_partial_same_sentence_id() {
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "你", false));
        asr.record_result(&make_result_event(1, "你好", false));
        asr.record_result(&make_result_event(1, "你好吗", true));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 1);
        assert_eq!(st.final_segments.get(&1).unwrap(), "你好吗");
        assert!(st.partial_segments.is_empty(), "partial not cleaned up");
    }

    #[test]
    fn interim_with_positive_end_time_not_mistaken_for_final() {
        // In the real DashScope API, interim results carry both sentence_end: false
        // and a positive end_time; verify this is not misjudged as final.
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "中间结果", false));
        let st = asr.state.lock();
        assert!(
            st.final_segments.is_empty(),
            "interim with end_time > 0 but sentence_end=false should not be final"
        );
        assert_eq!(st.partial_segments.len(), 1);
        assert_eq!(st.partial_segments.get(&1).unwrap(), "中间结果");
    }

    // ---- record_result: duplicate final ----

    #[test]
    fn duplicate_final_event_not_duplicated() {
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "你好吗", true));
        asr.record_result(&make_result_event(1, "你好吗", true));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 1);
        assert_eq!(st.final_segments.get(&1).unwrap(), "你好吗");
    }

    #[test]
    fn duplicate_final_event_updated_text() {
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "旧版本", true));
        asr.record_result(&make_result_event(1, "新版本", true));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 1);
        assert_eq!(st.final_segments.get(&1).unwrap(), "新版本");
    }

    // ---- record_result: sentence_id == 0 ----

    #[test]
    fn final_with_zero_sentence_id_not_dropped() {
        let asr = create_test_asr();
        let event = json!({
            "payload": {
                "output": {
                    "sentence": {
                        "text": "短句",
                        "sentence_end": true,
                        "end_time": 500
                    }
                }
            }
        });
        asr.record_result(&event);
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 1);
        assert!(st.final_segments.contains_key(&0));
        assert_eq!(st.final_segments.get(&0).unwrap(), "短句");
    }

    // ---- record_result: multiple sentence IDs ----

    #[test]
    fn multiple_sentence_ids_tracked_separately() {
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "第一句", true));
        asr.record_result(&make_result_event(2, "第二句", true));
        asr.record_result(&make_result_event(3, "第三句", true));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 3);
        // BTreeMap keeps insertion order
        let texts: Vec<&String> = st.final_segments.values().collect();
        assert_eq!(texts, vec!["第一句", "第二句", "第三句"]);
    }

    #[test]
    fn mixed_partial_and_final_interleaved() {
        let asr = create_test_asr();
        // sentence 1: interim updates
        asr.record_result(&make_result_event(1, "我", false));
        asr.record_result(&make_result_event(1, "我想", false));
        // sentence 2: interim updates
        asr.record_result(&make_result_event(2, "测", false));
        asr.record_result(&make_result_event(2, "测试", false));
        // sentence 1 final
        asr.record_result(&make_result_event(1, "我想吃饭", true));
        // sentence 2 final
        asr.record_result(&make_result_event(2, "测试一下", true));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 2);
        assert_eq!(st.final_segments.get(&1).unwrap(), "我想吃饭");
        assert_eq!(st.final_segments.get(&2).unwrap(), "测试一下");
        assert!(st.partial_segments.is_empty(), "partials not cleaned up");
    }

    // ---- finish_success ----

    #[test]
    fn finish_success_assembles_segments_with_merge_guard() {
        let asr = create_test_asr();
        asr.record_result(&make_result_event(1, "今天天气", true));
        asr.record_result(&make_result_event(2, "天气真不错", true));
        asr.record_result(&make_result_event(3, "不错吧", true));

        let (tx, mut rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.final_tx = Some(tx);
            st.bytes_received = 10000;
        }

        asr.finish_success();
        let result = rx.try_recv().unwrap().unwrap();
        assert_eq!(result.text, "今天天气真不错吧");
    }

    #[test]
    fn finish_success_fallback_to_last_result_text() {
        let asr = create_test_asr();
        // interim only, no final segments
        asr.record_result(&make_result_event(1, "中间结果", false));

        let (tx, mut rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.final_tx = Some(tx);
            st.bytes_received = 10000;
        }

        asr.finish_success();
        let result = rx.try_recv().unwrap().unwrap();
        assert_eq!(result.text, "中间结果");
    }

    // ---- existing tests kept ----

    #[test]
    fn endpoint_scheme_accepts_websocket_only() {
        assert!(endpoint_scheme_is_websocket(DEFAULT_ENDPOINT));
        assert!(endpoint_scheme_is_websocket("ws://localhost:9000/api-ws"));
        assert!(endpoint_scheme_is_websocket(
            "  WSS://dashscope.aliyuncs.com/api-ws/v1/inference/  "
        ));
        // The Bailian console's https compatibility-mode / dedicated-domain URLs are
        // not WebSocket gateways.
        assert!(!endpoint_scheme_is_websocket(
            "https://llm-xxx.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
        ));
        assert!(!endpoint_scheme_is_websocket(
            "http://dashscope.aliyuncs.com/api-ws/v1/inference/"
        ));
        assert!(!endpoint_scheme_is_websocket(""));
    }

    #[test]
    fn credentials_apply_default_endpoint_and_model() {
        let creds = BailianCredentials {
            api_key: "sk-test".to_string(),
            endpoint: "".to_string(),
            model: "".to_string(),
            vocabulary_id: None,
        };
        assert_eq!(creds.normalized_endpoint(), DEFAULT_ENDPOINT);
        assert_eq!(creds.normalized_model(), DEFAULT_MODEL);
    }

    #[test]
    fn run_task_message_uses_pcm_16k() {
        let value: Value =
            serde_json::from_str(&run_task_message("abc", DEFAULT_MODEL, None)).unwrap();
        assert_eq!(value["header"]["action"], "run-task");
        assert_eq!(value["payload"]["model"], DEFAULT_MODEL);
        assert_eq!(value["payload"]["parameters"]["sample_rate"], 16000);
        assert_eq!(value["payload"]["parameters"]["format"], "pcm");
        assert!(value["payload"]["parameters"]["vocabulary_id"].is_null());
    }

    #[test]
    fn run_task_message_uses_pcm_8k_for_8k_model() {
        let value: Value =
            serde_json::from_str(&run_task_message("abc", "fun-asr-flash-8k-realtime", None))
                .unwrap();
        assert_eq!(value["payload"]["parameters"]["sample_rate"], 8000);
    }

    #[test]
    fn downsample_16k_pcm_to_8k_averages_each_sample_pair() {
        let pcm = [
            2_i16.to_le_bytes(),
            4_i16.to_le_bytes(),
            6_i16.to_le_bytes(),
            8_i16.to_le_bytes(),
        ]
        .concat();
        let downsampled = downsample_pcm_16k_to_8k(&pcm);
        let samples = downsampled
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert_eq!(samples, vec![3, 7]);
    }

    #[test]
    fn downsample_flush_drops_lone_tail_sample() {
        // 3 16k samples: the complete pair (1,2) is averaged; the leftover 3rd sample
        // cannot be paired and is discarded at close (at most 0.0625ms lost,
        // imperceptible).
        let mut remainder = [
            1_i16.to_le_bytes(),
            2_i16.to_le_bytes(),
            3_i16.to_le_bytes(),
        ]
        .concat();
        let complete_len = remainder.len() / 4 * 4;
        let downsampled = downsample_pcm_16k_to_8k(&remainder[..complete_len]);
        remainder.drain(..complete_len);
        clear_downsample_tail(&mut remainder);

        let samples = downsampled
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert_eq!(samples, vec![(1 + 2) / 2]);
        assert!(remainder.is_empty());
    }

    #[test]
    fn downsample_keeps_phase_across_recorder_chunks() {
        let asr = BailianRealtimeASR::new(BailianCredentials {
            api_key: "sk-test".to_string(),
            endpoint: String::new(),
            model: "fun-asr-flash-8k-realtime".to_string(),
            vocabulary_id: None,
        });
        let first = [
            1_i16.to_le_bytes(),
            2_i16.to_le_bytes(),
            3_i16.to_le_bytes(),
        ]
        .concat();
        asr.consume_pcm_chunk(&first);
        asr.consume_pcm_chunk(&4_i16.to_le_bytes());

        let state = asr.state.lock();
        let samples = state
            .pending_audio
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        assert_eq!(samples, vec![1, 3]);
    }

    #[test]
    fn run_task_message_includes_optional_vocabulary_id() {
        let value: Value =
            serde_json::from_str(&run_task_message("abc", DEFAULT_MODEL, Some(" vocab-123 ")))
                .unwrap();
        assert_eq!(value["payload"]["parameters"]["vocabulary_id"], "vocab-123");
    }

    #[test]
    fn drain_audio_chunks_keeps_tail_buffered() {
        let mut buffer = vec![1u8; TARGET_AUDIO_CHUNK_BYTES * 2 + 17];
        let chunks = drain_audio_chunks(&mut buffer);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), TARGET_AUDIO_CHUNK_BYTES);
        assert_eq!(chunks[1].len(), TARGET_AUDIO_CHUNK_BYTES);
        assert_eq!(buffer.len(), 17);
    }

    #[tokio::test]
    async fn buffered_audio_paces_and_tracks_coverage_across_worker() {
        let frames = Arc::new(ParkingMutex::new(Vec::<(Instant, Vec<u8>)>::new()));
        let finish_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let frames_srv = Arc::clone(&frames);
        let finish_seen_srv = Arc::clone(&finish_seen);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/api-ws/v1/inference/",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(msg)) = ws.next().await {
                match msg {
                    Message::Text(text) => {
                        let v: Value = serde_json::from_str(&text).unwrap();
                        let task_id = v["header"]["task_id"].as_str().unwrap_or("").to_string();
                        match v["header"]["action"].as_str().unwrap_or_default() {
                            "run-task" => {
                                ws.send(Message::text(
                                    json!({
                                        "header": {"event": "task-started", "task_id": task_id},
                                        "payload": {}
                                    })
                                    .to_string(),
                                ))
                                .await
                                .unwrap();
                            }
                            "finish-task" => {
                                finish_seen_srv.store(true, std::sync::atomic::Ordering::Release);
                                ws.send(Message::text(
                                    json!({
                                        "header": {"event": "result-generated", "task_id": task_id},
                                        "payload": {
                                            "output": {
                                                "sentence": {
                                                    "sentence_id": 0,
                                                    "text": "百炼完成。",
                                                    "sentence_end": true,
                                                    "end_time": 1000
                                                }
                                            }
                                        }
                                    })
                                    .to_string(),
                                ))
                                .await
                                .unwrap();
                                ws.send(Message::text(
                                    json!({
                                        "header": {"event": "task-finished", "task_id": task_id},
                                        "payload": {}
                                    })
                                    .to_string(),
                                ))
                                .await
                                .unwrap();
                                let _ = ws.close(None).await;
                                break;
                            }
                            _ => {}
                        }
                    }
                    Message::Binary(data) => {
                        frames_srv.lock().push((Instant::now(), data.to_vec()));
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });

        let asr = Arc::new(BailianRealtimeASR::new(BailianCredentials {
            api_key: "sk-test".into(),
            endpoint,
            model: "fun-asr-flash-8k-realtime".into(),
            vocabulary_id: None,
        }));
        asr.open_session().await.unwrap();

        // Feed 16 kHz PCM that downsamples cleanly + 2 lone tail bytes (<= 2 B allowance).
        let input_len = TARGET_AUDIO_CHUNK_BYTES * 5 + 2;
        asr.consume_pcm_chunk(&vec![0x22u8; input_len]);
        asr.send_last_frame().await.unwrap();
        let res = asr.await_final_result().await.unwrap();
        assert_eq!(res.text, "百炼完成。");
        assert!(finish_seen.load(std::sync::atomic::Ordering::Acquire));
        assert!(asr.is_stream_complete());
        assert_eq!(
            asr.audio_coverage(),
            ((TARGET_AUDIO_CHUNK_BYTES * 5) as u64, input_len as u64)
        );

        let recorded = frames.lock();
        assert_eq!(recorded.len(), 5);
        let span = recorded
            .last()
            .unwrap()
            .0
            .saturating_duration_since(recorded[0].0);
        assert!(
            span >= Duration::from_millis(140),
            "expected >= 2x pacing across 4 intervals (~200ms), got {span:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn send_last_frame_fails_when_closed_or_write_times_out() {
        let unopened = Arc::new(BailianRealtimeASR::new(BailianCredentials {
            api_key: "sk-test".into(),
            endpoint: String::new(),
            model: DEFAULT_MODEL.into(),
            vocabulary_id: None,
        }));
        unopened.mark_task_started();
        assert!(unopened.send_last_frame().await.is_err());
        assert!(!unopened.is_stream_complete());

        let finish_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finish_seen_srv = Arc::clone(&finish_seen);
        let frame0_seen = Arc::new(Notify::new());
        let frame0_seen_srv = Arc::clone(&frame0_seen);
        let shutdown = Arc::new(Notify::new());
        let shutdown_srv = Arc::clone(&shutdown);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/api-ws/v1/inference/",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            loop {
                tokio::select! {
                    msg = ws.next() => match msg {
                        Some(Ok(Message::Text(text))) => {
                            let v: Value = serde_json::from_str(&text).unwrap();
                            let task_id = v["header"]["task_id"].as_str().unwrap_or("").to_string();
                            match v["header"]["action"].as_str().unwrap_or_default() {
                                "run-task" => {
                                    ws.send(Message::text(
                                        json!({
                                            "header": {"event": "task-started", "task_id": task_id},
                                            "payload": {}
                                        })
                                        .to_string(),
                                    ))
                                    .await
                                    .unwrap();
                                }
                                "finish-task" => {
                                    finish_seen_srv.store(true, std::sync::atomic::Ordering::Release);
                                }
                                _ => {}
                            }
                        }
                        Some(Ok(Message::Binary(_))) => {
                            frame0_seen_srv.notify_waiters();
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    },
                    _ = shutdown_srv.notified() => {
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
        });

        let asr = Arc::new(
            BailianRealtimeASR::new(BailianCredentials {
                api_key: "sk-test".into(),
                endpoint,
                model: DEFAULT_MODEL.into(),
                vocabulary_id: None,
            })
            .with_frame_send_timeout(Duration::from_millis(60)),
        );
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![0x33u8; TARGET_AUDIO_CHUNK_BYTES]);
        tokio::time::timeout(Duration::from_secs(2), frame0_seen.notified())
            .await
            .unwrap();

        let writer_guard = asr.writer.lock().await;
        asr.consume_pcm_chunk(&vec![0x44u8; TARGET_AUDIO_CHUNK_BYTES]);
        assert!(asr.send_last_frame().await.is_err());
        drop(writer_guard);

        assert!(!finish_seen.load(std::sync::atomic::Ordering::Acquire));
        assert!(!asr.is_stream_complete());
        assert_eq!(
            asr.audio_coverage(),
            (
                TARGET_AUDIO_CHUNK_BYTES as u64,
                (TARGET_AUDIO_CHUNK_BYTES * 2) as u64
            )
        );
        shutdown.notify_waiters();
        server.await.unwrap();
    }
}
