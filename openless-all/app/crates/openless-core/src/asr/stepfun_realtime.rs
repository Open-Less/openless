//! Step Audio realtime ASR client for StepFun
//! (`wss://api.stepfun.com/v1/realtime/asr/stream`).
//!
//! Like `qwen_realtime.rs`, an OpenAI Realtime-style WS, but with four key
//! differences (each verified against the live API on 2026-07-16):
//!
//! - **The model is passed in `session.update`**
//!   (`session.audio.input.transcription.model`), not as a URL query; session config
//!   uses the nested `audio.input.{format,transcription,turn_detection}` shape.
//! - **`delta`'s `text`/`stash` concatenate**: `text` is the confirmed prefix and
//!   `stash` the unsettled tail; the current segment's full text = `text + stash`
//!   (Qwen treats the two as mutually exclusive, taking whichever is non-empty).
//! - **No server-side finish event**: `session.finish` replies with
//!   `transcript.response.error` (unsupported); in server_vad mode
//!   `input_audio_buffer.commit` is silently ignored. The only reliable close is to
//!   **send >= silence_duration_ms of silence to force VAD to close the segment** —
//!   ~0.4s after speech_stopped the segment's `completed` arrives, then the client
//!   disconnects itself.
//! - The `prompt` field is accepted inside the transcription config (the batch
//!   /audio/transcriptions endpoint silently ignores prompt and only honors
//!   hotwords — the two channels bias vocabulary in opposite ways).

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::config::{TaskSpawner, TokioTaskSpawner};

use super::qwen_realtime::join_segments;
use super::{AudioConsumer, RawTranscript};
use crate::ports::{TextStreamChunk, TextStreamSink};

/// Internal effective id (`resolve_effective_asr_provider` routes here from `stepfun`
/// by model name); not shown in the settings-page preset list.
pub const PROVIDER_ID: &str = "stepfun-realtime";
pub const DEFAULT_ENDPOINT: &str = "wss://api.stepfun.com/v1/realtime/asr/stream";
pub const DEFAULT_MODEL: &str = "stepaudio-2.5-asr-stream";
/// Fixed path under the base URL for the realtime WS (derives the wss URL from the
/// https base shared with the batch endpoint).
const REALTIME_PATH: &str = "/realtime/asr/stream";

/// 100 ms of 16 kHz / 16-bit / mono PCM, matching the recorder output.
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 3_200;
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
const SESSION_READY_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound on the WebSocket handshake itself (TCP + TLS + HTTP upgrade).
///
/// Without it, `connect_async` waits forever: a mid-handshake disconnect, a corporate
/// gateway / hotel portal silently dropping packets, or a server black-holing the
/// connection means this await never returns. `open_session` is awaited with
/// `block_on` on the hotkey bridge thread (made serial to fix the #468/#475 latch
/// race), so once stuck, both press and release queue up unhandled — the user sees
/// "hotkeys suddenly completely dead, recording can neither start nor stop, must
/// restart the app", with no hint why.
///
/// Distinct from SESSION_READY_TIMEOUT, which covers "waiting for session.updated
/// after connecting"; this one covers connecting itself, previously unguarded. Value
/// matches the volcengine side.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// server_vad segmentation silence threshold, aligned with qwen_realtime (500ms
/// lowers the chance of cutting on a breath).
const VAD_SILENCE_DURATION_MS: u32 = 500;
/// Length of the silence tail appended at close: must exceed VAD_SILENCE_DURATION_MS
/// with network margin, otherwise VAD never closes the segment and the last
/// sentence's completed never arrives (the protocol has no finish event).
const SILENCE_TAIL_MS: u64 = 700;
/// Grace period after the silence tail for "no open segments": a pure-silence session
/// (connection check, accidental press) has no speech_started at all; when the grace
/// expires it returns successfully with empty text.
const FINISH_GRACE: Duration = Duration::from_millis(1_200);
/// Poll interval during the grace period. The finish criterion must be checked
/// repeatedly — a single check that fails leaves no second chance (see the grace task
/// in `send_last_frame`).
const FINISH_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Hard deadline counted from the tail frame write. Backstop for a server that never
/// closes the last segment, capping the worst-case wait at this instead of
/// FINAL_RESULT_TIMEOUT (12s).
const FINISH_HARD_DEADLINE: Duration = Duration::from_millis(3_000);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

#[derive(Clone, Debug)]
pub struct StepfunRealtimeCredentials {
    pub api_key: String,
    /// Three accepted forms: empty (default gateway), the `https://api.stepfun.com/v1`
    /// base shared with the batch endpoint (wss path derived automatically), or a full
    /// `wss://` URL (used as-is).
    pub endpoint: String,
    pub model: String,
    /// User dictionary assembled into a prompt (the realtime protocol accepts
    /// transcription.prompt; the batch endpoint is the opposite, only hotwords).
    /// None = not sent.
    pub prompt: Option<String>,
}

impl StepfunRealtimeCredentials {
    pub fn normalized_model(&self) -> String {
        let model = self.model.trim();
        if model.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model.to_string()
        }
    }

    /// Connect URL: a `wss://` prefix is used as-is; an `http(s)://` base (the
    /// credential slot shared with the batch endpoint) gets its scheme switched and
    /// the `/realtime/asr/stream` path appended; unparseable or empty falls back to
    /// the default gateway.
    pub fn connect_url(&self) -> String {
        let endpoint = self.endpoint.trim();
        if endpoint.is_empty() {
            return DEFAULT_ENDPOINT.to_string();
        }
        if endpoint.starts_with("wss://") || endpoint.starts_with("ws://") {
            return endpoint.trim_end_matches('/').to_string();
        }
        let Ok(mut url) = url::Url::parse(endpoint) else {
            return DEFAULT_ENDPOINT.to_string();
        };
        if url.set_scheme("wss").is_err() {
            return DEFAULT_ENDPOINT.to_string();
        }
        let path = url.path().trim_end_matches('/').to_string();
        if !path.ends_with(REALTIME_PATH) {
            url.set_path(&format!("{path}{REALTIME_PATH}"));
        }
        url.set_query(None);
        url.set_fragment(None);
        url.to_string()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StepfunASRError {
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
    Audio {
        chunk: Vec<u8>,
        contains_non_silent_audio: bool,
        /// Lets `send_last_frame` confirm the tail frame was actually written to the
        /// WebSocket by the write worker.
        written_tx: Option<oneshot::Sender<Result<(), String>>>,
    },
}

#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    audio_scratch: Vec<u8>,
    bytes_received: u64,
    /// Last time the write worker successfully wrote non-silent PCM to the WebSocket.
    last_non_silent_audio_written_at: Option<Instant>,
    /// Last time a server-side completed event arrived.
    last_completed_at: Option<Instant>,
    session_started: bool,
    session_finished: bool,
    session_start_error: Option<String>,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, StepfunASRError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    /// Completed segments (completed.transcript) accumulated in arrival order after
    /// VAD segmentation.
    completed_segments: Vec<String>,
    // Per connection: ignore updates/repeats for completed item IDs.
    completed_item_ids: std::collections::HashSet<String>,
    /// Current open segment's interim full text = delta.text (confirmed prefix) +
    /// delta.stash (unsettled tail); cleared when completed arrives.
    partial_text: String,
    /// Unsettled segment count: opened by speech_started, closed by completed. Finish
    /// criterion: finishing and zero (the silence tail has forced VAD to close all
    /// segments).
    open_segments: u32,
    /// send_last_frame has flushed the tail audio + silence frames and is waiting for
    /// segment count to reach zero.
    finishing: bool,
    /// Tail frame not yet confirmed written to the WebSocket by the write worker; an
    /// older segment's completed must not finish the session first.
    tail_write_pending: bool,
}

pub struct StepfunRealtimeASR {
    credentials: StepfunRealtimeCredentials,
    task_spawner: Arc<dyn TaskSpawner>,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, StepfunASRError>>>>,
    session_started: Arc<Notify>,
    session_finished: Arc<Notify>,
    partial_sink: ParkingMutex<Option<Arc<dyn TextStreamSink>>>,
}

impl StepfunRealtimeASR {
    pub fn new(credentials: StepfunRealtimeCredentials) -> Self {
        Self::with_task_spawner(credentials, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: StepfunRealtimeCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            credentials,
            task_spawner,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            session_started: Arc::new(Notify::new()),
            session_finished: Arc::new(Notify::new()),
            partial_sink: ParkingMutex::new(None),
        }
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.partial_sink.lock() = Some(sink);
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), StepfunASRError> {
        if self.credentials.api_key.trim().is_empty() {
            return Err(StepfunASRError::CredentialsMissing);
        }

        let url = self.credentials.connect_url();
        let mut request = url
            .into_client_request()
            .map_err(|e| StepfunASRError::ConnectionFailed(e.to_string()))?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.credentials.api_key.trim()))
                .map_err(|e| StepfunASRError::ConnectionFailed(e.to_string()))?,
        );

        let (ws, _resp) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| {
                StepfunASRError::ConnectionFailed(format!(
                    "连接超时（{} ms）",
                    CONNECT_TIMEOUT.as_millis()
                ))
            })?
            .map_err(|e| StepfunASRError::ConnectionFailed(e.to_string()))?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
            st.send_tx = Some(send_tx);
        }
        *self.final_rx.lock() = Some(final_rx);

        let writer_for_worker = Arc::clone(&self.writer);
        let weak_self_for_worker = Arc::downgrade(self);
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            while let Some(SendItem::Audio {
                chunk,
                contains_non_silent_audio,
                written_tx,
            }) = send_rx.recv().await
            {
                match send_text(&writer_for_worker, append_audio_message(&chunk)).await {
                    Ok(()) => {
                        if contains_non_silent_audio {
                            if let Some(this) = weak_self_for_worker.upgrade() {
                                this.mark_non_silent_audio_written();
                            }
                        }
                        if let Some(tx) = written_tx {
                            let _ = tx.send(Ok(()));
                        }
                    }
                    Err(error) => {
                        if let Some(tx) = written_tx {
                            let _ = tx.send(Err(error.to_string()));
                        }
                        log::error!("[stepfun-asr] audio frame send failed: {error}");
                        if let Some(this) = weak_self_for_worker.upgrade() {
                            this.finish_error(error);
                        }
                        break;
                    }
                }
            }
        }));

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
                        this.fail_session_start(
                            "websocket closed before session configuration completed",
                        );
                        this.finish_with_partial_or_error(StepfunASRError::NoFinalResult);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("[stepfun-asr] receive loop error: {e}");
                        this.fail_session_start(&e.to_string());
                        this.finish_with_partial_or_error(StepfunASRError::ConnectionFailed(
                            e.to_string(),
                        ));
                        break;
                    }
                }
            }
        }));

        let started = self.session_started.notified();
        tokio::pin!(started);
        started.as_mut().enable();
        let update = session_update_message(
            &self.credentials.normalized_model(),
            self.credentials.prompt.as_deref(),
        );
        if let Err(error) = send_text(&self.writer, update).await {
            self.cancel();
            return Err(error);
        }
        let ready_result = if !self.state.lock().session_started {
            tokio::time::timeout(SESSION_READY_TIMEOUT, started)
                .await
                .map_err(|_| StepfunASRError::FinalResultTimeout)
        } else {
            Ok(())
        };
        if let Err(error) = ready_result {
            self.cancel();
            return Err(error);
        }
        if let Some(error) = self.state.lock().session_start_error.clone() {
            self.cancel();
            return Err(StepfunASRError::TaskFailed(error));
        }

        Ok(())
    }

    /// Flushes the tail audio and appends silence frames to force VAD to close all
    /// open segments, then waits for every completed to arrive.
    ///
    /// The protocol has no finish event (see module docs), so the finish criterion
    /// comes from the client state machine: `finishing && open_segments == 0`.
    /// Pure-silence sessions (no speech_started at all) are covered by the
    /// FINISH_GRACE backstop and return successfully with empty text.
    pub async fn send_last_frame(self: &Arc<Self>) -> Result<(), StepfunASRError> {
        let result = tokio::time::timeout(FINAL_RESULT_TIMEOUT, async {
            let finished = self.session_finished.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();
            let (send_tx, tail, contains_non_silent_audio) = {
                let mut st = self.state.lock();
                let send_tx = st.send_tx.clone();
                if !st.pending_audio.is_empty() {
                    let pending = std::mem::take(&mut st.pending_audio);
                    st.audio_scratch.extend_from_slice(&pending);
                }
                let mut tail = std::mem::take(&mut st.audio_scratch);
                // Only the real recording counts as "voiced". The appended silence is a
                // close-out tool; counting it would push last_non_silent_audio_written_at
                // past the last completed, making has_audio_after_last_completed
                // permanently true.
                let contains_non_silent_audio = contains_non_silent_pcm(&tail);
                // Merge tail audio and silence frames into one append to save a write.
                tail.resize(tail.len() + (SILENCE_TAIL_MS * BYTES_PER_MS) as usize, 0);
                st.finishing = true;
                st.tail_write_pending = send_tx.is_some();
                (send_tx, tail, contains_non_silent_audio)
            };
            let Some(send_tx) = send_tx else {
                return Ok(());
            };
            let (written_tx, written_rx) = oneshot::channel();
            send_tx
                .send(SendItem::Audio {
                    chunk: tail,
                    contains_non_silent_audio,
                    written_tx: Some(written_tx),
                })
                .map_err(|_| {
                    StepfunASRError::SendFailed("audio writer is not available".to_string())
                })?;
            written_rx
                .await
                .map_err(|_| {
                    StepfunASRError::SendFailed(
                        "audio writer stopped before the tail frame was sent".to_string(),
                    )
                })?
                .map_err(StepfunASRError::SendFailed)?;
            self.state.lock().tail_write_pending = false;

            // Grace task: the tail frame is written and the client sends no more audio;
            // all that remains is waiting for the server to turn the tail audio into
            // completed events. Once FINISH_GRACE has elapsed with no open segments,
            // finish.
            //
            // Must poll: this used to check once at FINISH_GRACE expiry, and a single
            // failed check left no second chance — after the tail frame the server may
            // send no further events at all (when the pause before release already
            // exceeded the VAD threshold, the last segment's completed arrived before
            // the tail), so the session idled until FINAL_RESULT_TIMEOUT. Measured at
            // ~13% of dictations waiting the full 12 seconds this way. (The direct
            // cause of that 13% was contains_non_silent_pcm treating room tone as
            // speech, see that function; polling + the hard deadline are the second
            // line of defense ensuring any finish failure never degrades to 12s again.)
            //
            // The finish precondition is still "no open segments, and no unconfirmed
            // real audio after the last completed" — that guard is correct and must not
            // be dropped to fix the stall. The real backstop is FINISH_HARD_DEADLINE:
            // when the server never closes the last segment, worst-case wait is 3s
            // instead of idling until FINAL_RESULT_TIMEOUT.
            let weak = Arc::downgrade(self);
            let task_spawner = Arc::clone(&self.task_spawner);
            task_spawner.spawn(Box::pin(async move {
                let since_tail = Instant::now();
                loop {
                    tokio::time::sleep(FINISH_POLL_INTERVAL).await;
                    let Some(this) = weak.upgrade() else {
                        return;
                    };
                    let waited = since_tail.elapsed();
                    let expired = waited >= FINISH_HARD_DEADLINE;
                    let should_finish = {
                        let st = this.state.lock();
                        if st.session_finished {
                            return;
                        }
                        if expired {
                            should_force_finish_at_hard_deadline(&st)
                        } else {
                            waited >= FINISH_GRACE
                                && st.open_segments == 0
                                && !has_audio_after_last_completed(&st)
                        }
                    };
                    if should_finish {
                        if expired {
                            log::warn!(
                                "[stepfun-asr] server did not settle within {FINISH_HARD_DEADLINE:?} \
                                 (open segments); finishing with what we have"
                            );
                        }
                        this.finish_success();
                        return;
                    }
                }
            }));

            if !self.state.lock().session_finished {
                finished.await;
            }
            Ok(())
        })
        .await;
        match result {
            Ok(inner) => inner,
            Err(_) => {
                // Timeout backstop: return the partial result if one exists, error
                // otherwise — same as the disconnect path. Reaching here means the
                // grace task above failed to finish (it has its own FINISH_HARD_DEADLINE
                // backstop and normally should not get here), and the user has already
                // waited the full 12 seconds — this must be logged.
                log::warn!(
                    "[stepfun-asr] finish stalled for {:?}, falling back to partial transcript",
                    FINAL_RESULT_TIMEOUT
                );
                self.finish_with_partial_or_error(StepfunASRError::FinalResultTimeout);
                Ok(())
            }
        }
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, StepfunASRError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(StepfunASRError::NoFinalResult);
        };
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, rx)
            .await
            .map_err(|_| StepfunASRError::FinalResultTimeout)?
            .map_err(|_| StepfunASRError::NoFinalResult)?
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
        self.task_spawner.spawn(Box::pin(async move {
            let _ = close_writer(&writer).await;
        }));
    }

    fn handle_text_message(&self, text: &str) -> bool {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[stepfun-asr] invalid json event: {e}");
                return true;
            }
        };
        let event = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event {
            "session.updated" => {
                self.mark_session_started();
                true
            }
            "input_audio_buffer.speech_started" => {
                self.state.lock().open_segments += 1;
                true
            }
            "conversation.item.input_audio_transcription.delta" => {
                self.record_partial(&value);
                true
            }
            "conversation.item.input_audio_transcription.completed" => {
                self.record_completed(&value)
            }
            "conversation.item.input_audio_transcription.failed" => {
                let item_id = value
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown item");
                let message = value
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| value.get("message").and_then(Value::as_str))
                    .unwrap_or("audio transcription failed");
                self.finish_error(StepfunASRError::TaskFailed(format!("{item_id}: {message}")));
                false
            }
            // `transcript.response.error` is a StepFun-specific request-level error
            // event (observed when sending the unsupported session.finish); handled
            // like the generic `error`.
            "error" | "transcript.response.error" => {
                let message = value
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| value.get("message").and_then(Value::as_str))
                    .unwrap_or("realtime session error")
                    .to_string();
                self.finish_with_partial_or_error(StepfunASRError::TaskFailed(message));
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
                let _ = tx.send(SendItem::Audio {
                    contains_non_silent_audio: contains_non_silent_pcm(&chunk),
                    chunk,
                    written_tx: None,
                });
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

    fn record_partial(&self, value: &Value) {
        // `text` is the confirmed prefix and `stash` the unsettled tail; concatenated
        // they form the current segment's full text (unlike Qwen's mutually exclusive
        // semantics, see module docs). Empty deltas do not overwrite an existing
        // partial.
        let confirmed = value.get("text").and_then(Value::as_str).unwrap_or("");
        let stash = value.get("stash").and_then(Value::as_str).unwrap_or("");
        let combined = format!("{confirmed}{stash}");
        let combined = combined.trim();
        if !combined.is_empty() {
            let snapshot = {
                let mut state = self.state.lock();
                if state.session_finished
                    || value
                        .get("item_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| state.completed_item_ids.contains(id))
                {
                    return;
                }
                state.partial_text = combined.to_string();
                let mut segments = state.completed_segments.clone();
                segments.push(state.partial_text.clone());
                join_segments(&segments)
            };
            if let Some(sink) = self.partial_sink.lock().clone() {
                let _ = sink.publish(TextStreamChunk {
                    text: snapshot,
                    offset: 0,
                });
            }
        }
    }

    /// Returns false when the session has finished and the read loop may exit.
    fn record_completed(&self, value: &Value) -> bool {
        let transcript = value
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or("");
        let (should_finish, snapshot) = {
            let mut st = self.state.lock();
            if st.session_finished {
                return !st.session_finished;
            }
            if let Some(id) = value.get("item_id").and_then(Value::as_str) {
                if !st.completed_item_ids.insert(id.to_owned()) {
                    return !st.session_finished;
                }
            }
            let trimmed = transcript.trim();
            if !trimmed.is_empty() {
                st.completed_segments.push(trimmed.to_string());
            }
            st.partial_text.clear();
            st.open_segments = st.open_segments.saturating_sub(1);
            st.last_completed_at = Some(Instant::now());
            (
                st.finishing
                    && !st.tail_write_pending
                    && !has_audio_after_last_completed(&st)
                    && st.open_segments == 0,
                join_segments(&st.completed_segments),
            )
        };
        if let Some(sink) = self.partial_sink.lock().clone() {
            let _ = sink.publish(TextStreamChunk {
                text: snapshot,
                offset: 0,
            });
        }
        if should_finish {
            self.finish_success();
            return false;
        }
        true
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
            // At close, if an interim tail is still un-completed (the silence frames
            // should flush it out; defensive backstop), append it last.
            if !st.partial_text.is_empty() {
                segments.push(std::mem::take(&mut st.partial_text));
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

    fn finish_with_partial_or_error(&self, error: StepfunASRError) {
        let has_result = {
            let st = self.state.lock();
            has_transcript(&st)
        };
        if has_result {
            // Consistent with Bailian / Qwen / Volcengine: on error with an existing
            // result, fall back to returning it.
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_error(&self, error: StepfunASRError) {
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
        self.task_spawner.spawn(Box::pin(async move {
            let _ = close_writer(&writer).await;
        }));
    }

    fn mark_non_silent_audio_written(&self) {
        self.state.lock().last_non_silent_audio_written_at = Some(Instant::now());
    }
}

impl AudioConsumer for StepfunRealtimeASR {
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
                let _ = tx.send(SendItem::Audio {
                    contains_non_silent_audio: contains_non_silent_pcm(&chunk),
                    chunk,
                    written_tx: None,
                });
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

/// A sample counts as "voiced" only above ~1% of full-scale amplitude.
const NON_SILENT_PEAK: i16 = 328;
/// At least this many samples above threshold to call the frame real speech. A single
/// spike (keyboard, electrical noise) must not hold the finish criterion.
const NON_SILENT_MIN_SAMPLES: usize = 8;

/// Whether this s16le PCM contains real speech.
///
/// Used to be `any(|byte| byte != 0)` — but microphone room tone (measured
/// RMS approx 0.0008) makes every sample non-zero, so every frame was judged
/// "non-silent", last_non_silent_audio_written_at was refreshed pointlessly, and the
/// finish criterion could never hold. Amplitude-based instead.
fn contains_non_silent_pcm(pcm: &[u8]) -> bool {
    pcm.as_chunks::<2>()
        .0
        .iter()
        .filter(|sample| {
            i16::from_le_bytes([sample[0], sample[1]]).saturating_abs() > NON_SILENT_PEAK
        })
        .take(NON_SILENT_MIN_SAMPLES)
        .count()
        == NON_SILENT_MIN_SAMPLES
}

fn has_transcript(state: &SyncState) -> bool {
    !state.completed_segments.is_empty() || !state.partial_text.trim().is_empty()
}

/// At the hard deadline, finish only if there is current-segment text to return, or
/// no audio remains unsettled.
///
/// If the server confirmed a speech segment started but emitted no transcript yet,
/// keep waiting until FINAL_RESULT_TIMEOUT so a late completed can arrive; after the
/// timeout `finish_with_partial_or_error` returns an explicit error rather than
/// silently succeeding with empty text.
fn should_force_finish_at_hard_deadline(state: &SyncState) -> bool {
    if state.open_segments > 0 {
        return !state.partial_text.trim().is_empty();
    }
    !has_audio_after_last_completed(state)
}

fn has_audio_after_last_completed(state: &SyncState) -> bool {
    match (
        state.last_non_silent_audio_written_at,
        state.last_completed_at,
    ) {
        (Some(last_audio), Some(last_completed)) => last_audio > last_completed,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn session_update_message(model: &str, prompt: Option<&str>) -> String {
    // language omitted => server auto-detects.
    let mut transcription = json!({ "model": model });
    if let Some(prompt) = prompt {
        let trimmed = prompt.trim();
        if !trimmed.is_empty() {
            transcription["prompt"] = json!(trimmed);
        }
    }
    json!({
        "type": "session.update",
        "event_id": event_id(),
        "session": {
            "audio": {
                "input": {
                    "format": {
                        "type": "pcm",
                        "codec": "pcm_s16le",
                        "rate": 16000,
                        "bits": 16,
                        "channel": 1,
                    },
                    "transcription": transcription,
                    "turn_detection": {
                        "type": "server_vad",
                        "silence_duration_ms": VAD_SILENCE_DURATION_MS,
                    },
                },
            },
        },
    })
    .to_string()
}

fn append_audio_message(pcm: &[u8]) -> String {
    json!({
        "type": "input_audio_buffer.append",
        "event_id": event_id(),
        "audio": base64::engine::general_purpose::STANDARD.encode(pcm),
    })
    .to_string()
}

fn event_id() -> String {
    format!("event_{}", Uuid::new_v4())
}

async fn send_text(writer: &SharedWriter, text: String) -> Result<(), StepfunASRError> {
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut guard = writer.lock().await;
        let Some(ws) = guard.as_mut() else {
            return Err(StepfunASRError::ConnectionFailed(
                "websocket writer not available".to_string(),
            ));
        };
        ws.send(Message::Text(text))
            .await
            .map_err(|e| StepfunASRError::SendFailed(e.to_string()))
    })
    .await
    .map_err(|_| StepfunASRError::SendFailed("websocket write timed out".to_string()))?
}

async fn close_writer(writer: &SharedWriter) -> Result<(), StepfunASRError> {
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
        for text in ["你", "你好", "您好"] {
            asr.record_partial(&serde_json::json!({"text": text}));
        }
        asr.record_completed(&serde_json::json!({"transcript": "您好。"}));
        asr.record_partial(&serde_json::json!({"stash": "世界"}));
        asr.record_completed(&serde_json::json!({"transcript": "世界！"}));
        sink.assert_snapshots(&["你", "你好", "您好", "您好。", "您好。世界", "您好。世界！"]);
    }

    use futures_util::{SinkExt, StreamExt};

    #[test]
    fn completed_item_ignores_late_partial_and_duplicate_but_accepts_next_item() {
        let asr = create_test_asr();
        let sink = Arc::new(super::super::TranscriptCapture::default());
        asr.set_partial_sink(sink.clone());
        asr.record_partial(&serde_json::json!({"item_id":"a", "text":"你好"}));
        asr.record_completed(&serde_json::json!({"item_id":"a", "transcript":"你好。"}));
        asr.record_partial(&serde_json::json!({"item_id":"a", "text":"旧文字"}));
        asr.record_completed(&serde_json::json!({"item_id":"a", "transcript":"你好。"}));
        asr.record_partial(&serde_json::json!({"item_id":"b", "text":"世界"}));
        sink.assert_snapshots(&["你好", "你好。", "你好。世界"]);
    }

    fn create_test_asr() -> StepfunRealtimeASR {
        StepfunRealtimeASR::new(StepfunRealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: String::new(),
            model: String::new(),
            prompt: None,
        })
    }

    /// Speech-level s16le PCM: amplitude far above NON_SILENT_PEAK.
    fn speech_pcm(bytes: usize) -> Vec<u8> {
        std::iter::repeat(6_000i16.to_le_bytes())
            .flatten()
            .take(bytes)
            .collect()
    }

    /// Room-tone-level s16le PCM: every byte non-zero (the old `byte != 0` check would
    /// misjudge it as speech), but amplitude below NON_SILENT_PEAK — actually silence.
    fn room_tone_pcm(bytes: usize) -> Vec<u8> {
        std::iter::repeat(0x0101i16.to_le_bytes())
            .flatten()
            .take(bytes)
            .collect()
    }

    // ---- credentials / URL ----

    #[test]
    fn connect_url_defaults_and_derives_from_https_base() {
        let mut creds = StepfunRealtimeCredentials {
            api_key: "k".to_string(),
            endpoint: String::new(),
            model: String::new(),
            prompt: None,
        };
        assert_eq!(creds.connect_url(), DEFAULT_ENDPOINT);
        assert_eq!(creds.normalized_model(), DEFAULT_MODEL);

        // https base shared with the batch endpoint (preset default) -> derive the
        // full wss path.
        creds.endpoint = "https://api.stepfun.com/v1".to_string();
        assert_eq!(creds.connect_url(), DEFAULT_ENDPOINT);
        creds.endpoint = "https://api.stepfun.com/v1/".to_string();
        assert_eq!(creds.connect_url(), DEFAULT_ENDPOINT);

        // Full wss URL used as-is.
        creds.endpoint = "wss://gateway.example.com/v1/realtime/asr/stream".to_string();
        assert_eq!(
            creds.connect_url(),
            "wss://gateway.example.com/v1/realtime/asr/stream"
        );
    }

    // ---- message builders ----

    #[test]
    fn session_update_uses_stepfun_nested_shape() {
        let value: Value =
            serde_json::from_str(&session_update_message(DEFAULT_MODEL, Some("OpenLess.")))
                .unwrap();
        assert_eq!(value["type"], "session.update");
        let input = &value["session"]["audio"]["input"];
        assert_eq!(input["format"]["codec"], "pcm_s16le");
        assert_eq!(input["format"]["rate"], 16000);
        assert_eq!(input["transcription"]["model"], DEFAULT_MODEL);
        assert_eq!(input["transcription"]["prompt"], "OpenLess.");
        assert_eq!(input["turn_detection"]["type"], "server_vad");
    }

    #[test]
    fn session_update_omits_blank_prompt() {
        let value: Value =
            serde_json::from_str(&session_update_message(DEFAULT_MODEL, Some("  "))).unwrap();
        assert!(value["session"]["audio"]["input"]["transcription"]["prompt"].is_null());
        let value: Value =
            serde_json::from_str(&session_update_message(DEFAULT_MODEL, None)).unwrap();
        assert!(value["session"]["audio"]["input"]["transcription"]["prompt"].is_null());
    }

    // ---- event handling ----

    fn delta_event(text: &str, stash: &str) -> String {
        json!({
            "type": "conversation.item.input_audio_transcription.delta",
            "text": text,
            "stash": stash,
        })
        .to_string()
    }

    fn completed_event(transcript: &str) -> String {
        json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "transcript": transcript,
        })
        .to_string()
    }

    fn speech_started_event() -> String {
        json!({ "type": "input_audio_buffer.speech_started" }).to_string()
    }

    #[test]
    fn delta_concatenates_confirmed_text_with_stash() {
        // StepFun semantics: text prefix + stash tail (measured 2026-07), not Qwen's
        // pick-the-non-empty-one.
        let asr = create_test_asr();
        asr.handle_text_message(&delta_event("", "今天。"));
        assert_eq!(asr.state.lock().partial_text, "今天。");
        asr.handle_text_message(&delta_event("今天天气不错，", "我们来测试。"));
        assert_eq!(asr.state.lock().partial_text, "今天天气不错，我们来测试。");
        // Empty deltas do not overwrite an existing partial.
        asr.handle_text_message(&delta_event("", ""));
        assert_eq!(asr.state.lock().partial_text, "今天天气不错，我们来测试。");
    }

    #[test]
    fn completed_closes_open_segment_and_clears_partial() {
        let asr = create_test_asr();
        asr.handle_text_message(&speech_started_event());
        assert_eq!(asr.state.lock().open_segments, 1);
        asr.handle_text_message(&delta_event("第一句", ""));
        let keep_going = asr.handle_text_message(&completed_event("第一句话说完了。"));
        assert!(keep_going, "未进入 finishing 时读循环应继续");
        let st = asr.state.lock();
        assert_eq!(st.completed_segments, vec!["第一句话说完了。"]);
        assert!(st.partial_text.is_empty());
        assert_eq!(st.open_segments, 0);
    }

    #[test]
    fn finishing_completes_when_last_open_segment_closes() {
        let asr = create_test_asr();
        asr.handle_text_message(&speech_started_event());
        let (tx, mut rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.final_tx = Some(tx);
            st.finishing = true;
            st.bytes_received = 32_000;
        }
        let keep_going = asr.handle_text_message(&completed_event("最后一句。"));
        assert!(!keep_going, "最后一段 completed 后读循环应退出");
        let result = rx.try_recv().unwrap().unwrap();
        assert_eq!(result.text, "最后一句。");
        assert_eq!(result.duration_ms, 1_000);
    }

    #[test]
    fn finishing_waits_while_segments_still_open() {
        let asr = create_test_asr();
        asr.handle_text_message(&speech_started_event());
        asr.handle_text_message(&speech_started_event());
        asr.state.lock().finishing = true;
        let keep_going = asr.handle_text_message(&completed_event("第一段。"));
        assert!(keep_going, "还有开放句段时不能提前收尾");
        assert!(!asr.state.lock().session_finished);
    }

    #[test]
    fn stepfun_request_error_event_returns_partial_when_available() {
        let asr = create_test_asr();
        asr.handle_text_message(&speech_started_event());
        asr.handle_text_message(&completed_event("已识别内容。"));
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        let keep_going = asr.handle_text_message(
            &json!({"type": "transcript.response.error", "error": {"message": "boom"}}).to_string(),
        );
        assert!(!keep_going);
        assert_eq!(rx.try_recv().unwrap().unwrap().text, "已识别内容。");
    }

    #[test]
    fn error_event_without_partial_returns_error() {
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        asr.handle_text_message(
            &json!({"type": "error", "error": {"message": "boom"}}).to_string(),
        );
        let err = rx.try_recv().unwrap().unwrap_err();
        assert!(matches!(err, StepfunASRError::TaskFailed(m) if m == "boom"));
    }

    #[test]
    fn hard_deadline_waits_for_unsettled_segment_without_transcript() {
        let mut state = SyncState {
            open_segments: 1,
            completed_segments: vec!["previous".to_string()],
            ..SyncState::default()
        };
        assert!(!should_force_finish_at_hard_deadline(&state));

        state.partial_text = "interim".to_string();
        assert!(should_force_finish_at_hard_deadline(&state));

        state.partial_text.clear();
        state.open_segments = 0;
        state.last_non_silent_audio_written_at = Some(Instant::now());
        assert!(!should_force_finish_at_hard_deadline(&state));

        state.last_completed_at = Some(Instant::now());
        assert!(should_force_finish_at_hard_deadline(&state));
    }

    #[test]
    fn only_session_updated_marks_session_ready() {
        let asr = create_test_asr();
        asr.handle_text_message(&json!({"type": "session.created"}).to_string());
        assert!(!asr.state.lock().session_started);
        asr.handle_text_message(&json!({"type": "session.updated"}).to_string());
        assert!(asr.state.lock().session_started);
    }

    #[test]
    fn empty_finishing_session_yields_empty_text() {
        // Connection check / accidental press: no segments at all; finish_success
        // returns empty text successfully.
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.final_tx = Some(tx);
            st.finishing = true;
        }
        asr.finish_success();
        assert_eq!(rx.try_recv().unwrap().unwrap().text, "");
    }

    #[test]
    fn multi_segment_transcripts_join_like_qwen() {
        let asr = create_test_asr();
        asr.handle_text_message(&speech_started_event());
        asr.handle_text_message(&completed_event("第一句。"));
        asr.handle_text_message(&speech_started_event());
        let (tx, mut rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.final_tx = Some(tx);
            st.finishing = true;
        }
        asr.handle_text_message(&completed_event("第二句。"));
        assert_eq!(rx.try_recv().unwrap().unwrap().text, "第一句。第二句。");
    }

    // ---- audio buffering ----

    #[test]
    fn audio_buffered_before_session_ready() {
        let asr = create_test_asr();
        asr.consume_pcm_chunk(&[0u8; 100]);
        let st = asr.state.lock();
        assert_eq!(st.pending_audio.len(), 100);
        assert_eq!(st.bytes_received, 100);
    }

    #[test]
    fn silence_detection_ignores_room_tone_but_catches_speech() {
        // Room tone: every byte non-zero, the old `byte != 0` check would call it speech.
        assert!(!contains_non_silent_pcm(&room_tone_pcm(3_200)));
        assert!(contains_non_silent_pcm(&speech_pcm(3_200)));
        // Pure silence and a single-point spike both count as silence.
        assert!(!contains_non_silent_pcm(&[0u8; 3_200]));
        let mut spike = room_tone_pcm(3_200);
        spike[0..2].copy_from_slice(&20_000i16.to_le_bytes());
        assert!(!contains_non_silent_pcm(&spike));
    }

    #[test]
    fn only_audio_written_after_completed_remains_pending() {
        let completed_at = Instant::now();
        let mut state = SyncState {
            last_non_silent_audio_written_at: Some(completed_at),
            last_completed_at: Some(completed_at + Duration::from_millis(1)),
            ..SyncState::default()
        };
        assert!(!has_audio_after_last_completed(&state));

        state.last_non_silent_audio_written_at = Some(completed_at + Duration::from_millis(2));
        assert!(has_audio_after_last_completed(&state));
    }

    #[test]
    fn completed_does_not_finish_while_tail_write_is_pending() {
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        {
            let mut state = asr.state.lock();
            state.final_tx = Some(tx);
            state.finishing = true;
            state.tail_write_pending = true;
            state.open_segments = 1;
            state.last_non_silent_audio_written_at = Some(Instant::now());
        }

        let keep_going = asr.handle_text_message(&completed_event("上一段。"));

        assert!(keep_going, "尾帧未写入时，旧 completed 不能提前关闭会话");
        assert!(!asr.state.lock().session_finished);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn non_silent_audio_waits_for_delayed_vad_before_finishing() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/v1/realtime/asr/stream",
            listener.local_addr().unwrap()
        );
        let (release_events_tx, release_events_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            ws.send(Message::Text(
                json!({ "type": "session.updated" }).to_string(),
            ))
            .await
            .unwrap();

            release_events_rx.await.unwrap();
            ws.send(Message::Text(speech_started_event()))
                .await
                .unwrap();
            ws.send(Message::Text(completed_event("delayed speech")))
                .await
                .unwrap();
            // Keep the connection open until the client processes the completed and
            // closes itself, so the test server is not dropped first and turned into a
            // client-side connection error.
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                while let Some(message) = ws.next().await {
                    match message {
                        Ok(Message::Close(_)) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            })
            .await;
        });

        let asr = Arc::new(StepfunRealtimeASR::new(StepfunRealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint,
            model: DEFAULT_MODEL.to_string(),
            prompt: None,
        }));
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&speech_pcm(TARGET_AUDIO_CHUNK_BYTES));

        let asr_for_finish = Arc::clone(&asr);
        let finish = tokio::spawn(async move { asr_for_finish.send_last_frame().await });
        tokio::time::sleep(FINISH_GRACE + Duration::from_millis(100)).await;
        assert!(
            !finish.is_finished(),
            "non-silent audio must not finish before the server observes its VAD segment"
        );

        release_events_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), finish)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            asr.await_final_result().await.unwrap().text,
            "delayed speech"
        );
        server.await.unwrap();
    }

    /// Fake gateway: replies with `session.updated` after the handshake, emits the
    /// given events on the first audio append, then stays silent until the client
    /// closes — simulating StepFun's "no finish event" reality.
    async fn spawn_fake_gateway(events_on_first_audio: Vec<String>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/v1/realtime/asr/stream",
            listener.local_addr().unwrap()
        );
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            // The first message is necessarily session.update.
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            ws.send(Message::Text(
                json!({ "type": "session.updated" }).to_string(),
            ))
            .await
            .unwrap();
            let mut fired = false;
            while let Some(Ok(message)) = ws.next().await {
                match message {
                    Message::Text(_) if !fired => {
                        fired = true;
                        for event in &events_on_first_audio {
                            ws.send(Message::Text(event.clone())).await.unwrap();
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        endpoint
    }

    /// Regression: the pause before release already exceeded the VAD threshold — the
    /// last segment's completed arrived before the tail frame, and the server sends
    /// nothing after that.
    ///
    /// Before the fix: room tone left in the tail was judged speech by `byte != 0`,
    /// pushing last_non_silent_audio_written_at past the completed, so the one-shot
    /// grace check failed with no second chance -> the full 12s FINAL_RESULT_TIMEOUT.
    /// Measured on ~13% of dictations.
    #[tokio::test]
    async fn finishes_promptly_when_last_completed_arrived_before_the_tail() {
        let endpoint = spawn_fake_gateway(vec![
            speech_started_event(),
            completed_event("说完就松手了。"),
        ])
        .await;

        let asr = Arc::new(StepfunRealtimeASR::new(StepfunRealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint,
            model: DEFAULT_MODEL.to_string(),
            prompt: None,
        }));
        asr.open_session().await.unwrap();
        // One full frame of speech, making the server reply speech_started + completed.
        asr.consume_pcm_chunk(&speech_pcm(TARGET_AUDIO_CHUNK_BYTES));
        // Key timing: completed must arrive before the tail frame, otherwise the healthy
        // fast path in `record_completed` runs and the bug does not reproduce.
        tokio::time::timeout(Duration::from_secs(5), async {
            while asr.state.lock().last_completed_at.is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake gateway should have completed the segment");
        // Sub-frame room-tone residue: stays in audio_scratch and is concatenated with
        // the silence frames into the tail at close. The old check treated it as
        // speech, pushing "last speech" past the completed.
        asr.consume_pcm_chunk(&room_tone_pcm(1_600));

        let started = Instant::now();
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, asr.send_last_frame())
            .await
            .expect("send_last_frame must not hang until the final-result timeout")
            .unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < FINISH_HARD_DEADLINE,
            "should settle on the grace path, took {elapsed:?}"
        );
        assert_eq!(
            asr.await_final_result().await.unwrap().text,
            "说完就松手了。"
        );
    }

    /// Regression: the server never closes the last segment (only speech_started +
    /// delta, no completed). The hard deadline must bound the wait instead of
    /// degrading to 12 seconds.
    #[tokio::test]
    async fn hard_deadline_bounds_the_wait_when_segment_never_closes() {
        let endpoint = spawn_fake_gateway(vec![
            speech_started_event(),
            delta_event("这句话没有收到 completed", ""),
        ])
        .await;

        let asr = Arc::new(StepfunRealtimeASR::new(StepfunRealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint,
            model: DEFAULT_MODEL.to_string(),
            prompt: None,
        }));
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&speech_pcm(TARGET_AUDIO_CHUNK_BYTES));

        let started = Instant::now();
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, asr.send_last_frame())
            .await
            .expect("hard deadline must fire well before the final-result timeout")
            .unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed >= FINISH_HARD_DEADLINE && elapsed < FINISH_HARD_DEADLINE * 2,
            "should settle at the hard deadline, took {elapsed:?}"
        );
        // The un-completed interim tail is still returned; a finish timeout must not
        // drop text.
        assert_eq!(
            asr.await_final_result().await.unwrap().text,
            "这句话没有收到 completed"
        );
    }

    // When the server accepts TCP but never completes the WebSocket handshake
    // (disconnect, corporate gateway / hotel portal silently dropping packets, server
    // black-holing), open_session must time out with an error instead of hanging the
    // caller forever — it runs block_on on the serial hotkey bridge thread, and a hang
    // queues press / release events unhandled: hotkeys appear totally dead, recording
    // can neither start nor stop, and the app must be restarted.
    #[tokio::test]
    async fn open_session_times_out_when_handshake_never_completes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Accepts the connection then never replies, leaving it hanging.
        let _server = tokio::spawn(async move {
            let _accepted = listener.accept().await;
            std::future::pending::<()>().await;
        });

        let asr = Arc::new(StepfunRealtimeASR::new(StepfunRealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: format!("ws://{addr}"),
            model: String::new(),
            prompt: None,
        }));

        let started = Instant::now();
        let err = asr
            .open_session()
            .await
            .expect_err("握手不完成时必须超时失败，不能永远挂着");
        assert!(
            matches!(&err, StepfunASRError::ConnectionFailed(msg) if msg.contains("连接超时")),
            "应报连接超时，实际: {err:?}"
        );
        // Generous upper bound, only to prove the wait is truly bounded (without
        // CONNECT_TIMEOUT this would never return).
        assert!(
            started.elapsed() < CONNECT_TIMEOUT * 3,
            "超时应在 CONNECT_TIMEOUT 量级返回，实际耗时 {:?}",
            started.elapsed()
        );
    }
}
