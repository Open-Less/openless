//! iFlytek (Xfyun Open Platform) realtime speech transcription (RTASR) streaming
//! client.
//!
//! Official docs: https://www.xfyun.cn/doc/asr/rtasr/API.html
//!
//! Protocol notes (standard edition):
//! - Endpoint: `wss://rtasr.xfyun.cn/v1/ws`; auth via query params
//!   `appid` + `ts` + `signa`, where `signa = Base64(HmacSHA1(MD5(appid + ts), apiKey))`.
//! - Audio: 16 kHz / 16-bit / mono PCM, exactly matching the OpenLess recorder output.
//! - Recommended: send 1280 bytes every 40ms; sending faster may trigger engine errors.
//! - End of upload: send the binary message `{"end": true}`.
//! - Results: the server returns `{"action":"result","data":"<json string>"}` as text
//!   messages; `data.cn.st.type` is `0` (final) / `1` (interim); after all results the
//!   server disconnects.
//!
//! Known limits: standard RTASR has no request-level hotwords (personalized hotwords
//! can only be uploaded in the Xfyun console); dialects/minor languages need console
//! enablement plus a `lang` param; initially fixed to Mandarin Chinese (`cn`).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest as Md5Digest, Md5};
use parking_lot::Mutex as ParkingMutex;
use serde_json::Value;
use sha1::Sha1;
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{TaskSpawner, TokioTaskSpawner};

use super::stream_send::{send_ws_message_with_timeout, PacedSendClock, SharedWriter};
use super::{AudioConsumer, RawTranscript};
use crate::ports::{TextStreamChunk, TextStreamSink};

pub const PROVIDER_ID: &str = "iflytek";
pub const DEFAULT_ENDPOINT: &str = "wss://rtasr.xfyun.cn/v1/ws";
/// RTASR docs recommendation: 1280 bytes every 40ms (16k/16-bit/mono = 32000 B/s).
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 1_280;
/// 16 kHz / 16-bit / mono = 32000 bytes/sec -> 32 bytes/ms.
const BYTES_PER_MS: u64 = 32;
const FRAME_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
/// Cap on the WebSocket handshake (TCP + TLS + HTTP upgrade) so a poor network cannot
/// hang the hotkey thread.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Cap on waiting for `action=started` during the handshake. Auth failures
/// (10105/10110) should fail fast within this window instead of surfacing only at
/// finish time.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Default language: Mandarin Chinese. Dialects/minor languages require console
/// enablement and a corresponding `lang` param.
const DEFAULT_LANG: &str = "cn";

enum SendItem {
    Audio(Vec<u8>),
    End(oneshot::Sender<Result<(), XfyunASRError>>),
}

#[derive(Clone, Debug)]
pub struct XfyunCredentials {
    /// Xfyun Open Platform app ID.
    pub app_id: String,
    /// API key for the realtime speech transcription service.
    pub api_key: String,
}

impl XfyunCredentials {
    pub fn auth_ok(&self) -> bool {
        !self.app_id.trim().is_empty() && !self.api_key.trim().is_empty()
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum XfyunASRError {
    #[error("credentials missing")]
    CredentialsMissing,
    #[error("连接失败: {0}")]
    ConnectionFailed(String),
    /// Handshake returned 10105 / 10110: wrong AppID / APIKey, IP allowlist not
    /// configured, or the realtime transcription service not enabled.
    #[error("凭据被拒或未开通服务（{0}）")]
    AuthRejected(String),
    /// 10800: over the licensed connection count / concurrency limited.
    #[error("并发受限（{0}）")]
    RateLimited(String),
    #[error("识别失败: {0}")]
    TaskFailed(String),
    #[error("no final result")]
    NoFinalResult,
    #[error("final result timed out")]
    FinalResultTimeout,
}

#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    bytes_received: u64,
    bytes_written: u64,
    started: bool,
    ending: bool,
    end_sent: bool,
    finished: bool,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, XfyunASRError>>>,
    /// FIFO send queue shared across audio frames and the final end sentinel.
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    /// First send worker error, preserved so send_last_frame fails reliably instead
    /// of sending an out-of-order end marker after an incomplete stream.
    send_error: Option<XfyunASRError>,
    cancel_tx: Option<watch::Sender<bool>>,
    /// seg_id -> final (type=0) segment text. A later result for the same seg_id
    /// overwrites the previous one.
    final_segments: BTreeMap<i64, String>,
    /// seg_id -> latest interim (type=1) segment text, used as a fallback when the
    /// server disconnects before sending final.
    partial_segments: BTreeMap<i64, String>,
    last_result_text: String,
}

pub struct XfyunStreamingASR {
    credentials: XfyunCredentials,
    endpoint: String,
    frame_send_timeout: Duration,
    task_spawner: Arc<dyn TaskSpawner>,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, XfyunASRError>>>>,
    /// Handshake result channel: the receive loop sends Ok on `action=started` and Err
    /// on error. `open_session` awaits it to fail fast on auth rejection.
    handshake_tx: ParkingMutex<Option<oneshot::Sender<Result<(), XfyunASRError>>>>,
    partial_sink: ParkingMutex<Option<Arc<dyn TextStreamSink>>>,
}

impl XfyunStreamingASR {
    pub fn new(credentials: XfyunCredentials) -> Self {
        Self::with_task_spawner(credentials, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: XfyunCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            credentials,
            endpoint: DEFAULT_ENDPOINT.to_string(),
            frame_send_timeout: FRAME_SEND_TIMEOUT,
            task_spawner,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            handshake_tx: ParkingMutex::new(None),
            partial_sink: ParkingMutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    #[cfg(test)]
    pub(crate) fn with_frame_send_timeout(mut self, timeout: Duration) -> Self {
        self.frame_send_timeout = timeout;
        self
    }

    pub fn audio_coverage(&self) -> (u64, u64) {
        let st = self.state.lock();
        (st.bytes_written, st.bytes_received)
    }

    pub fn is_stream_complete(&self) -> bool {
        let st = self.state.lock();
        st.send_error.is_none() && st.end_sent && st.bytes_written == st.bytes_received
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.partial_sink.lock() = Some(sink);
    }

    /// Builds the WebSocket URL with auth params:
    /// `wss://rtasr.xfyun.cn/v1/ws?appid=..&ts=..&signa=..&lang=cn`.
    pub fn connect_url(&self) -> String {
        connect_url_at(&self.endpoint, &self.credentials)
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), XfyunASRError> {
        if !self.credentials.auth_ok() {
            return Err(XfyunASRError::CredentialsMissing);
        }

        let request = self
            .connect_url()
            .into_client_request()
            .map_err(|e| XfyunASRError::ConnectionFailed(e.to_string()))?;
        let (ws, _resp) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| {
                XfyunASRError::ConnectionFailed(format!(
                    "连接超时（{} ms）",
                    CONNECT_TIMEOUT.as_millis()
                ))
            })?
            .map_err(|e| XfyunASRError::ConnectionFailed(e.to_string()))?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        let (handshake_tx, handshake_rx) = oneshot::channel();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
            st.send_tx = Some(send_tx);
            st.cancel_tx = Some(cancel_tx);
        }
        *self.final_rx.lock() = Some(final_rx);
        *self.handshake_tx.lock() = Some(handshake_tx);

        // Send worker: drains audio chunks in strict FIFO order with realtime pacing
        // before processing the end sentinel, so `{"end": true}` never overtakes audio.
        let writer_for_worker = Arc::clone(&self.writer);
        let weak_self_for_worker = Arc::downgrade(self);
        let frame_send_timeout = self.frame_send_timeout;
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            let mut clock = PacedSendClock::new();
            loop {
                let item = tokio::select! {
                    maybe_item = send_rx.recv() => match maybe_item {
                        Some(item) => item,
                        None => break,
                    },
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow_and_update() {
                            break;
                        }
                        continue;
                    }
                };

                match item {
                    SendItem::Audio(chunk) => {
                        let Some(scheduled_start) = clock.wait_slot(&mut cancel_rx).await else {
                            break;
                        };
                        let chunk_len = chunk.len();
                        if let Err(ws_err) = send_ws_message_with_timeout(
                            &writer_for_worker,
                            Message::Binary(chunk.into()),
                            frame_send_timeout,
                            &mut cancel_rx,
                        )
                        .await
                        {
                            let err = XfyunASRError::ConnectionFailed(
                                ws_err.into_message("websocket not open"),
                            );
                            log::error!("[xfyun-asr] audio frame send failed: {err}");
                            if let Some(this) = weak_self_for_worker.upgrade() {
                                this.record_send_failure(err);
                            }
                            // Stop immediately on failure so SendItem::End is never written
                            // over an incomplete audio stream.
                            break;
                        }
                        if let Some(this) = weak_self_for_worker.upgrade() {
                            this.state.lock().bytes_written += chunk_len as u64;
                        }
                        clock.mark_sent(scheduled_start, chunk_len, BYTES_PER_MS, 1);
                    }
                    SendItem::End(ack_tx) => {
                        let result = send_ws_message_with_timeout(
                            &writer_for_worker,
                            Message::Binary(br#"{"end": true}"#.to_vec().into()),
                            frame_send_timeout,
                            &mut cancel_rx,
                        )
                        .await
                        .map_err(|ws_err| {
                            XfyunASRError::ConnectionFailed(
                                ws_err.into_message("websocket not open"),
                            )
                        });
                        match &result {
                            Ok(()) => {
                                if let Some(this) = weak_self_for_worker.upgrade() {
                                    this.state.lock().end_sent = true;
                                }
                            }
                            Err(err) => {
                                log::error!("[xfyun-asr] end frame send failed: {err}");
                                if let Some(this) = weak_self_for_worker.upgrade() {
                                    this.record_send_failure(err.clone());
                                }
                            }
                        }
                        let _ = ack_tx.send(result);
                        break;
                    }
                }
            }
        }));

        // Receive loop: handles started / result / error, plus server disconnect.
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
                        // The server disconnects on its own after all results; also
                        // covers interruptions like 37005 (15s without audio) —
                        // finish_on_close falls back to whatever arrived.
                        this.finish_on_close();
                        break;
                    }
                    Ok(_) => { /* ignore binary/ping/pong */ }
                    Err(e) => {
                        log::error!("[xfyun-asr] receive loop error: {e}");
                        this.finish_with_partial_or_error(XfyunASRError::ConnectionFailed(
                            e.to_string(),
                        ));
                        break;
                    }
                }
            }
        }));

        // Wait for the handshake result: auth errors / rejected connections fail fast
        // here, without waiting for the user to finish speaking.
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => {
                self.cancel();
                Err(e)
            }
            Ok(Err(_)) => {
                self.cancel();
                Err(XfyunASRError::ConnectionFailed(
                    "握手通道提前关闭".to_string(),
                ))
            }
            Err(_) => {
                self.cancel();
                Err(XfyunASRError::ConnectionFailed(format!(
                    "握手超时（{} ms）",
                    HANDSHAKE_TIMEOUT.as_millis()
                )))
            }
        }
    }

    pub async fn send_last_frame(&self) -> Result<(), XfyunASRError> {
        let (end_tx, end_rx) = oneshot::channel();
        {
            let mut st = self.state.lock();
            if let Some(ref error) = st.send_error {
                return Err(error.clone());
            }
            let Some(tx) = st.send_tx.take() else {
                return Err(XfyunASRError::ConnectionFailed("websocket not open".into()));
            };
            st.ending = true;
            if !st.pending_audio.is_empty() {
                let leftover = std::mem::take(&mut st.pending_audio);
                if tx.send(SendItem::Audio(leftover)).is_err() {
                    return Err(st.send_error.clone().unwrap_or_else(|| {
                        XfyunASRError::ConnectionFailed("audio send queue closed".into())
                    }));
                }
            }
            if tx.send(SendItem::End(end_tx)).is_err() {
                return Err(st.send_error.clone().unwrap_or_else(|| {
                    XfyunASRError::ConnectionFailed("audio send queue closed".into())
                }));
            }
        }

        match end_rx.await {
            Ok(result) => result,
            Err(_) => {
                let error = self.state.lock().send_error.clone().unwrap_or_else(|| {
                    XfyunASRError::ConnectionFailed(
                        "audio send worker stopped before end frame".into(),
                    )
                });
                Err(error)
            }
        }
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, XfyunASRError> {
        self.await_final_result_with_timeout(FINAL_RESULT_TIMEOUT)
            .await
    }

    pub async fn await_final_result_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<RawTranscript, XfyunASRError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(XfyunASRError::NoFinalResult);
        };
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(XfyunASRError::NoFinalResult),
            Err(_) => {
                log::error!(
                    "[xfyun-asr] final result timed out after {} ms",
                    timeout.as_millis()
                );
                self.cancel();
                Err(XfyunASRError::FinalResultTimeout)
            }
        }
    }

    pub fn cancel(&self) {
        {
            let mut st = self.state.lock();
            st.pending_audio.clear();
            st.send_tx = None;
            if let Some(tx) = st.cancel_tx.as_ref() {
                let _ = tx.send(true);
            }
        }
        // Release the handshake channel: if open_session is still waiting for started,
        // it immediately receives Err and returns.
        *self.handshake_tx.lock() = None;
        self.close_writer();
        self.signal_error(XfyunASRError::NoFinalResult);
    }

    // ---- internals ----

    fn handle_text_message(&self, text: &str) -> bool {
        let value: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[xfyun-asr] invalid json event: {e}");
                return true;
            }
        };
        let action = value
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match action {
            "started" => {
                self.mark_started();
                true
            }
            "result" => {
                self.record_result(&value);
                true
            }
            "error" => {
                let code = value
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let desc = value
                    .get("desc")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                log::error!("[xfyun-asr] server error code={code} desc={desc}");
                let error = classify_server_error(&code, &desc);
                // Error during the handshake: hand it directly to open_session's
                // handshake waiter.
                let handed = self.handshake_tx.lock().take();
                if let Some(tx) = handed {
                    let _ = tx.send(Err(error.clone()));
                }
                self.finish_error(error);
                false
            }
            _ => true,
        }
    }

    fn mark_started(&self) {
        self.state.lock().started = true;
        let tx = self.handshake_tx.lock().take();
        if let Some(tx) = tx {
            let _ = tx.send(Ok(()));
        }
    }

    fn record_result(&self, value: &Value) {
        let Some(data_str) = value.get("data").and_then(Value::as_str) else {
            return;
        };
        let Ok(data) = serde_json::from_str::<Value>(data_str) else {
            log::warn!("[xfyun-asr] result data is not valid JSON");
            return;
        };
        let Some(seg_id) = data.get("seg_id").and_then(Value::as_i64) else {
            return;
        };
        let Some(st) = data.get("cn").and_then(|c| c.get("st")) else {
            return;
        };
        // The docs type the `type` field as a string ("0" final / "1" interim).
        // Defensively accept the numeric form (0/1) too: if the server returned a
        // number and only strings were accepted, all final results would degrade to
        // interim and the close-out fallback would keep only the last sentence —
        // losing the earlier ones.
        let is_final = st
            .get("type")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s == "0")
                    .or_else(|| v.as_i64().map(|n| n == 0))
            })
            .unwrap_or(false);
        let text = extract_words(st);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        let snapshot = {
            let mut state = self.state.lock();
            state.last_result_text = trimmed.to_string();
            if is_final {
                state.final_segments.insert(seg_id, trimmed.to_string());
                state.partial_segments.remove(&seg_id);
            } else {
                state.partial_segments.insert(seg_id, trimmed.to_string());
            }
            let mut segments = state.partial_segments.clone();
            segments.extend(state.final_segments.clone());
            super::mimo::join_transcript_chunks(&segments.into_values().collect::<Vec<_>>())
        };
        if let Some(sink) = self.partial_sink.lock().clone() {
            let _ = sink.publish(TextStreamChunk {
                text: snapshot,
                offset: 0,
            });
        }
    }

    /// Server disconnected: normal path (all results already sent) or abnormal
    /// interruption. When closed before the handshake completes, hand the error to
    /// open_session's waiter first.
    fn finish_on_close(&self) {
        let handshake = self.handshake_tx.lock().take();
        if let Some(tx) = handshake {
            let _ = tx.send(Err(XfyunASRError::ConnectionFailed(
                "连接在握手完成前被关闭".to_string(),
            )));
            return;
        }
        self.finish_with_partial_or_error(XfyunASRError::NoFinalResult);
    }

    /// If any recognized content (final or interim) exists, return it as a fallback;
    /// otherwise error — consistent with the Volcengine / Bailian policy of "not
    /// losing recognized text when the server disconnects before final".
    fn finish_with_partial_or_error(&self, error: XfyunASRError) {
        self.finish_with_partial_or_error_inner(error, false);
    }

    fn finish_with_partial_or_error_inner(&self, error: XfyunASRError, is_send_failure: bool) {
        enum Outcome {
            AlreadyFinished,
            Partial {
                tx: Option<oneshot::Sender<Result<RawTranscript, XfyunASRError>>>,
                text: String,
                duration_ms: u64,
            },
            Error {
                tx: Option<oneshot::Sender<Result<RawTranscript, XfyunASRError>>>,
                error: XfyunASRError,
            },
        }
        // Resolve state, send_error, finished flag, and final_tx in one critical section so
        // concurrent cancel() cannot intercept final_tx ahead of partial salvage.
        let outcome = {
            let mut st = self.state.lock();
            if is_send_failure && st.send_error.is_none() {
                st.send_error = Some(error.clone());
            }
            if st.finished {
                st.send_tx = None;
                if is_send_failure {
                    st.pending_audio.clear();
                }
                Outcome::AlreadyFinished
            } else {
                st.finished = true;
                st.pending_audio.clear();
                st.send_tx = None;
                if let Some(cancel_tx) = st.cancel_tx.as_ref() {
                    let _ = cancel_tx.send(true);
                }
                let has_partial =
                    !st.last_result_text.trim().is_empty() || !st.partial_segments.is_empty();
                let tx = st.final_tx.take();
                if has_partial {
                    let text = if st.final_segments.is_empty() {
                        st.last_result_text.clone()
                    } else {
                        let segments: Vec<String> = st.final_segments.values().cloned().collect();
                        super::mimo::join_transcript_chunks(&segments)
                    };
                    let audio_bytes = st.bytes_written.max(st.bytes_received);
                    let duration_ms = if audio_bytes > 0 {
                        audio_bytes / BYTES_PER_MS
                    } else {
                        st.start
                            .map(|start| start.elapsed().as_millis() as u64)
                            .unwrap_or_default()
                    };
                    Outcome::Partial {
                        tx,
                        text,
                        duration_ms,
                    }
                } else {
                    Outcome::Error { tx, error }
                }
            }
        };
        match outcome {
            Outcome::AlreadyFinished => {}
            Outcome::Partial {
                tx,
                text,
                duration_ms,
            } => {
                if let Some(tx) = tx {
                    let _ = tx.send(Ok(RawTranscript { text, duration_ms }));
                }
                self.close_writer();
            }
            Outcome::Error { tx, error } => {
                if let Some(handshake_tx) = self.handshake_tx.lock().take() {
                    let _ = handshake_tx.send(Err(error.clone()));
                }
                if let Some(tx) = tx {
                    let _ = tx.send(Err(error));
                }
                self.close_writer();
            }
        }
    }

    fn signal_error(&self, error: XfyunASRError) {
        let tx = {
            let mut st = self.state.lock();
            if st.finished {
                return;
            }
            st.finished = true;
            st.send_tx = None;
            if let Some(cancel_tx) = st.cancel_tx.as_ref() {
                let _ = cancel_tx.send(true);
            }
            st.final_tx.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(Err(error));
        }
    }

    fn record_send_failure(&self, error: XfyunASRError) {
        self.finish_with_partial_or_error_inner(error, true);
    }

    fn finish_error(&self, error: XfyunASRError) {
        // If an error occurs before the handshake completes (receive-loop network
        // interruption, server error message), it must reach open_session's handshake
        // waiter — otherwise that side idles until HANDSHAKE_TIMEOUT (5s). Sessions
        // past the handshake take None here and skip idempotently.
        let handshake = self.handshake_tx.lock().take();
        if let Some(tx) = handshake {
            let _ = tx.send(Err(error.clone()));
        }
        self.signal_error(error);
        self.close_writer();
    }

    fn close_writer(&self) {
        let writer = Arc::clone(&self.writer);
        self.task_spawner.spawn(Box::pin(async move {
            let mut guard = writer.lock().await;
            if let Some(mut ws) = guard.take() {
                let _ = ws.close().await;
            }
        }));
    }
}

impl AudioConsumer for XfyunStreamingASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let mut st = self.state.lock();
        if !st.started || st.ending {
            return;
        }
        st.bytes_received = st.bytes_received.saturating_add(pcm.len() as u64);
        if st.finished || st.send_error.is_some() {
            return;
        }
        let Some(tx) = st.send_tx.clone() else {
            return;
        };
        st.pending_audio.extend_from_slice(pcm);
        let mut chunks = Vec::new();
        while st.pending_audio.len() >= TARGET_AUDIO_CHUNK_BYTES {
            let chunk: Vec<u8> = st.pending_audio.drain(..TARGET_AUDIO_CHUNK_BYTES).collect();
            chunks.push(chunk);
        }
        for chunk in chunks {
            if tx.send(SendItem::Audio(chunk)).is_err() {
                log::warn!("[xfyun-asr] audio queue closed; dropping subsequent frames");
                return;
            }
        }
    }
}

fn connect_url_at(endpoint: &str, credentials: &XfyunCredentials) -> String {
    let ts = chrono::Utc::now().timestamp().to_string();
    let signa = compute_signa(&credentials.app_id, &credentials.api_key, &ts);
    let mut url = url::Url::parse(endpoint).expect("endpoint parses");
    url.query_pairs_mut()
        .append_pair("appid", credentials.app_id.trim())
        .append_pair("ts", &ts)
        .append_pair("signa", &signa)
        .append_pair("lang", DEFAULT_LANG);
    url.to_string()
}

#[cfg(test)]
fn connect_url(credentials: &XfyunCredentials) -> String {
    connect_url_at(DEFAULT_ENDPOINT, credentials)
}

/// `signa = Base64(HmacSHA1(MD5(appid + ts), apiKey))`, matching the Xfyun Open
/// Platform docs formula.
pub fn compute_signa(app_id: &str, api_key: &str, ts: &str) -> String {
    let base = format!("{app_id}{ts}");
    let md5_hex = md5_hex(base.as_bytes());
    let mut mac = Hmac::<Sha1>::new_from_slice(api_key.trim().as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(md5_hex.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

fn md5_hex(input: &[u8]) -> String {
    let digest = Md5::digest(input);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// Extracts all words from the `cn.st` node: joins the first candidate's `w` across
/// `rt[].ws[].cw[]` in order.
fn extract_words(st: &Value) -> String {
    let mut out = String::new();
    let Some(rt) = st.get("rt").and_then(Value::as_array) else {
        return out;
    };
    for sentence in rt {
        let Some(ws) = sentence.get("ws").and_then(Value::as_array) else {
            continue;
        };
        for word in ws {
            let Some(cw) = word.get("cw").and_then(Value::as_array) else {
                continue;
            };
            for candidate in cw {
                if let Some(w) = candidate.get("w").and_then(Value::as_str) {
                    out.push_str(w);
                    break;
                }
            }
        }
    }
    out
}

/// Classifies iFlytek error codes into user-readable categories: auth/licensing and
/// concurrency limits get their own variants; everything else is a generic task
/// failure (keeps the capsule message from vaguely blaming the network).
fn classify_server_error(code: &str, desc: &str) -> XfyunASRError {
    match code {
        "10105" | "10110" => XfyunASRError::AuthRejected(format!("{code} {desc}")),
        "10800" => XfyunASRError::RateLimited(format!("{code} {desc}")),
        _ => XfyunASRError::TaskFailed(format!("{code} {desc}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_snapshots_keep_prefix_and_recognition_corrections() {
        let asr = XfyunStreamingASR::new(XfyunCredentials {
            app_id: "test".into(),
            api_key: "test".into(),
        });
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
            let data = serde_json::json!({"seg_id": id, "cn": {"st": {"type": if final_result {"0"} else {"1"}, "rt": [{"ws": [{"cw": [{"w": text}]}]}]}}});
            asr.record_result(&serde_json::json!({"data": data.to_string()}));
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

    #[test]
    fn signa_matches_official_documentation_example() {
        // Official docs example: appid=595f23df, ts=1512041814, apiKey=d9f4aa7ea6d94faca62cd88a28fd5234
        // → signa = IrrzsJeOFk1NGfJHW6SkHUoN9CU=
        let signa = compute_signa("595f23df", "d9f4aa7ea6d94faca62cd88a28fd5234", "1512041814");
        assert_eq!(signa, "IrrzsJeOFk1NGfJHW6SkHUoN9CU=");
    }

    #[test]
    fn md5_hex_matches_known_vector() {
        // MD5("595f23df1512041814") = 0829d4012497c14a30e7e72aeebe565e (documentation example)
        assert_eq!(
            md5_hex(b"595f23df1512041814"),
            "0829d4012497c14a30e7e72aeebe565e"
        );
    }

    #[test]
    fn connect_url_contains_encoded_auth_params() {
        let creds = XfyunCredentials {
            app_id: "595f23df".into(),
            api_key: "d9f4aa7ea6d94faca62cd88a28fd5234".into(),
        };
        let url = connect_url(&creds);
        assert!(url.starts_with("wss://rtasr.xfyun.cn/v1/ws?"));
        assert!(url.contains("appid=595f23df"));
        assert!(url.contains("ts="));
        assert!(url.contains("signa="));
        assert!(url.contains("lang=cn"));
    }

    #[test]
    fn extract_words_joins_candidates_in_order() {
        let data = serde_json::json!({
            "cn": {
                "st": {
                    "bg": "820",
                    "ed": "0",
                    "rt": [
                        {"ws": [
                            {"cw": [{"w": "啊", "wp": "n"}], "wb": 0, "we": 0},
                            {"cw": [{"w": "喂", "wp": "n"}], "wb": 0, "we": 0},
                            {"cw": [{"w": "！", "wp": "p"}], "wb": 0, "we": 0},
                            {"cw": [{"w": "你好", "wp": "n"}], "wb": 0, "we": 0}
                        ]}
                    ],
                    "type": "1"
                }
            },
            "seg_id": 5
        });
        let text = extract_words(&data["cn"]["st"]);
        assert_eq!(text, "啊喂！你好");
    }

    #[test]
    fn record_result_stores_final_and_partial_separately() {
        let asr = XfyunStreamingASR::new(XfyunCredentials {
            app_id: "app".into(),
            api_key: "key".into(),
        });
        asr.record_result(&serde_json::json!({
            "action": "result",
            "data": "{\"cn\":{\"st\":{\"rt\":[{\"ws\":[{\"cw\":[{\"w\":\"中间\"}]}]}],\"type\":\"1\"}},\"seg_id\":1}"
        }));
        {
            let st = asr.state.lock();
            assert!(st.final_segments.is_empty());
            assert_eq!(st.partial_segments.get(&1).unwrap(), "中间");
        }
        asr.record_result(&serde_json::json!({
            "action": "result",
            "data": "{\"cn\":{\"st\":{\"rt\":[{\"ws\":[{\"cw\":[{\"w\":\"最终\"}]}]}],\"type\":\"0\"}},\"seg_id\":1}"
        }));
        {
            let st = asr.state.lock();
            assert_eq!(st.final_segments.get(&1).unwrap(), "最终");
            assert!(
                st.partial_segments.is_empty(),
                "final 应清除同 seg 的 partial"
            );
        }
    }

    #[test]
    fn record_result_accepts_numeric_type_for_final() {
        // Defensively: if the server returns type as numbers 0/1 instead of the doc'd
        // strings "0"/"1", final results must still land in final_segments instead of
        // degrading to interim and losing sentences.
        let asr = XfyunStreamingASR::new(XfyunCredentials {
            app_id: "app".into(),
            api_key: "key".into(),
        });
        asr.record_result(&serde_json::json!({
            "action": "result",
            "data": "{\"cn\":{\"st\":{\"rt\":[{\"ws\":[{\"cw\":[{\"w\":\"第一句\"}]}]}],\"type\":0}},\"seg_id\":1}"
        }));
        asr.record_result(&serde_json::json!({
            "action": "result",
            "data": "{\"cn\":{\"st\":{\"rt\":[{\"ws\":[{\"cw\":[{\"w\":\"第二句\"}]}]}],\"type\":0}},\"seg_id\":2}"
        }));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 2);
        assert_eq!(st.final_segments.get(&1).unwrap(), "第一句");
        assert_eq!(st.final_segments.get(&2).unwrap(), "第二句");
        assert!(st.partial_segments.is_empty());
    }

    #[test]
    fn duplicate_final_segment_overwrites_not_duplicates() {
        let asr = XfyunStreamingASR::new(XfyunCredentials {
            app_id: "app".into(),
            api_key: "key".into(),
        });
        let event = |text: &str| {
            serde_json::json!({
                "action": "result",
                "data": format!(
                    "{{\"cn\":{{\"st\":{{\"rt\":[{{\"ws\":[{{\"cw\":[{{\"w\":\"{text}\"}}]}}]}}],\"type\":\"0\"}}}},\"seg_id\":2}}"
                )
            })
        };
        asr.record_result(&event("第一版"));
        asr.record_result(&event("第二版"));
        let st = asr.state.lock();
        assert_eq!(st.final_segments.len(), 1);
        assert_eq!(st.final_segments.get(&2).unwrap(), "第二版");
    }

    #[test]
    fn auth_and_license_errors_classify_as_auth_rejected() {
        assert!(matches!(
            classify_server_error("10105", "illegal access|illegal client_ip"),
            XfyunASRError::AuthRejected(_)
        ));
        assert!(matches!(
            classify_server_error("10110", "no license"),
            XfyunASRError::AuthRejected(_)
        ));
    }

    #[test]
    fn concurrency_limit_classifies_as_rate_limited() {
        assert!(matches!(
            classify_server_error("10800", "over max connect limit"),
            XfyunASRError::RateLimited(_)
        ));
    }

    #[test]
    fn generic_errors_stay_task_failed() {
        assert!(matches!(
            classify_server_error("37005", "no audio data"),
            XfyunASRError::TaskFailed(_)
        ));
    }

    #[test]
    fn credentials_require_both_fields_trimmed() {
        let ok = XfyunCredentials {
            app_id: "app".into(),
            api_key: "key".into(),
        };
        assert!(ok.auth_ok());
        assert!(!XfyunCredentials {
            app_id: "".into(),
            api_key: "key".into()
        }
        .auth_ok());
        assert!(!XfyunCredentials {
            app_id: "app".into(),
            api_key: "  ".into()
        }
        .auth_ok());
    }

    #[test]
    fn finish_error_notifies_pending_handshake_waiter() {
        // An error before the handshake (e.g. network interruption) must wake
        // open_session's handshake waiter, otherwise it idles out HANDSHAKE_TIMEOUT.
        // Sessions past the handshake take None — idempotent.
        let asr = XfyunStreamingASR::new(XfyunCredentials {
            app_id: "app".into(),
            api_key: "key".into(),
        });
        let (tx, mut rx) = oneshot::channel();
        *asr.handshake_tx.lock() = Some(tx);

        asr.finish_error(XfyunASRError::ConnectionFailed("boom".into()));

        match rx.try_recv() {
            Ok(Err(XfyunASRError::ConnectionFailed(_))) => {}
            other => panic!("handshake 等待方应收到错误，实际: {other:?}"),
        }
        // Already taken: repeated calls neither panic nor notify again.
        asr.finish_error(XfyunASRError::ConnectionFailed("again".into()));
        assert!(asr.handshake_tx.lock().is_none());
    }

    async fn spawn_loopback_ws<F, Fut>(handler: F) -> (String, tokio::task::JoinHandle<()>)
    where
        F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback listener");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept client");
            let ws = tokio_tungstenite::accept_async(stream)
                .await
                .expect("accept ws handshake");
            handler(ws).await;
        });
        (format!("ws://{addr}"), handle)
    }

    fn loopback_asr(endpoint: String) -> Arc<XfyunStreamingASR> {
        Arc::new(
            XfyunStreamingASR::new(XfyunCredentials {
                app_id: "test-app".into(),
                api_key: "test-key".into(),
            })
            .with_endpoint(endpoint),
        )
    }

    fn xfyun_result_json(text: &str, is_final: bool) -> String {
        let data = serde_json::json!({
            "seg_id": 0,
            "cn": {
                "st": {
                    "type": if is_final { "0" } else { "1" },
                    "rt": [{"ws": [{"cw": [{"w": text}]}]}]
                }
            }
        });
        serde_json::json!({
            "action": "result",
            "code": "0",
            "data": data.to_string()
        })
        .to_string()
    }

    #[tokio::test]
    async fn buffered_audio_is_paced_and_precedes_end_frame() {
        let frames = Arc::new(ParkingMutex::new(Vec::<(Instant, Vec<u8>)>::new()));
        let frames_srv = Arc::clone(&frames);
        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                .await
                .unwrap();
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Binary(data) = msg {
                    let bytes = data.to_vec();
                    let is_end = bytes == br#"{"end": true}"#;
                    frames_srv.lock().push((Instant::now(), bytes));
                    if is_end {
                        ws.send(Message::Text(xfyun_result_json("测试完成", true).into()))
                            .await
                            .unwrap();
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
        })
        .await;

        let asr = loopback_asr(endpoint);
        asr.open_session().await.expect("open session");

        let mut payload = vec![0x11u8; TARGET_AUDIO_CHUNK_BYTES * 5 + 400];
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte = (idx % 251) as u8;
        }
        asr.consume_pcm_chunk(&payload);
        asr.send_last_frame().await.expect("send_last_frame");

        let transcript = asr.await_final_result().await.expect("final transcript");
        assert_eq!(transcript.text, "测试完成");
        server_handle.await.expect("server join");

        let recorded = frames.lock();
        assert_eq!(recorded.len(), 7, "5 chunks + 1 leftover + 1 end marker");
        for i in 0..5 {
            assert_eq!(recorded[i].1.len(), TARGET_AUDIO_CHUNK_BYTES);
        }
        assert_eq!(recorded[5].1.len(), 400);
        assert_eq!(recorded[6].1.as_slice(), br#"{"end": true}"#);

        let total_pacing_elapsed = recorded[5].0.saturating_duration_since(recorded[0].0);
        assert!(
            total_pacing_elapsed >= Duration::from_millis(140),
            "expected ~200 ms pacing across 6 audio frames, got {total_pacing_elapsed:?}"
        );
        for i in 0..4 {
            let delta = recorded[i + 1].0.saturating_duration_since(recorded[i].0);
            assert!(
                delta >= Duration::from_millis(25),
                "expected ~40 ms pacing between frame {i} and {}, got {delta:?}",
                i + 1
            );
        }
    }

    #[tokio::test]
    async fn end_frame_waits_for_delayed_send_worker_instead_of_overtaking_audio() {
        let frames = Arc::new(ParkingMutex::new(Vec::<Vec<u8>>::new()));
        let frames_srv = Arc::clone(&frames);
        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                .await
                .unwrap();
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Binary(data) = msg {
                    let bytes = data.to_vec();
                    let is_end = bytes == br#"{"end": true}"#;
                    frames_srv.lock().push(bytes);
                    if is_end {
                        ws.send(Message::Text(xfyun_result_json("长缓冲完成", true).into()))
                            .await
                            .unwrap();
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
        })
        .await;

        let asr = loopback_asr(endpoint);
        asr.open_session().await.expect("open session");

        // 25 full chunks + 320 B tail spans ~1,000 ms of realtime pacing (> old 800 ms cap).
        let payload = vec![0x22u8; TARGET_AUDIO_CHUNK_BYTES * 25 + 320];
        asr.consume_pcm_chunk(&payload);
        asr.send_last_frame().await.expect("send_last_frame");

        let transcript = asr.await_final_result().await.expect("final transcript");
        assert_eq!(transcript.text, "长缓冲完成");
        server_handle.await.expect("server join");

        let recorded = frames.lock();
        assert_eq!(recorded.len(), 27, "25 chunks + leftover + end marker");
        assert_eq!(recorded.last().unwrap().as_slice(), br#"{"end": true}"#);
        let audio_bytes: usize = recorded[..26].iter().map(Vec::len).sum();
        assert_eq!(audio_bytes, payload.len());
    }

    #[tokio::test]
    async fn audio_frame_send_failure_stops_worker_and_fails_send_last_frame_without_end() {
        let frames = Arc::new(ParkingMutex::new(Vec::<Vec<u8>>::new()));
        let frame0_seen = Arc::new(tokio::sync::Notify::new());
        let shutdown_srv = Arc::new(tokio::sync::Notify::new());

        let frames_srv = Arc::clone(&frames);
        let frame0_seen_srv = Arc::clone(&frame0_seen);
        let shutdown_srv_task = Arc::clone(&shutdown_srv);

        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                .await
                .unwrap();
            loop {
                tokio::select! {
                    msg = ws.next() => match msg {
                        Some(Ok(Message::Binary(data))) => {
                            let bytes = data.to_vec();
                            frames_srv.lock().push(bytes);
                            frame0_seen_srv.notify_waiters();
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    },
                    _ = shutdown_srv_task.notified() => {
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
        })
        .await;

        let asr = Arc::new(
            XfyunStreamingASR::new(XfyunCredentials {
                app_id: "test-app".into(),
                api_key: "test-key".into(),
            })
            .with_endpoint(endpoint)
            .with_frame_send_timeout(Duration::from_millis(60)),
        );
        asr.open_session().await.expect("open session");

        asr.consume_pcm_chunk(&vec![0x33u8; TARGET_AUDIO_CHUNK_BYTES]);
        tokio::time::timeout(Duration::from_secs(2), frame0_seen.notified())
            .await
            .expect("frame 0 arrives");

        // Case A: fail during send_last_frame without partial text -> both send_last_frame and
        // await_final_result error, and `{"end": true}` is never transmitted even after lock release.
        let writer_guard = asr.writer.lock().await;
        asr.consume_pcm_chunk(&vec![0x44u8; TARGET_AUDIO_CHUNK_BYTES + 256]);
        let send_err = asr
            .send_last_frame()
            .await
            .expect_err("stalled frame must fail send_last_frame");
        assert!(matches!(send_err, XfyunASRError::ConnectionFailed(_)));
        drop(writer_guard);

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(asr.await_final_result().await.is_err());
        {
            let recorded = frames.lock();
            assert_eq!(recorded.len(), 1);
            assert!(
                recorded.iter().all(|f| f.as_slice() != br#"{"end": true}"#),
                "end marker must not be sent after audio frame failure"
            );
        }
        shutdown_srv.notify_waiters();
        server_handle.await.expect("server join");

        // Case B: when interim text already arrived before frame write failure, send_last_frame
        // still returns Err (so long recordings replay) while await_final_result salvages partial.
        let frames_b = Arc::new(ParkingMutex::new(Vec::<Vec<u8>>::new()));
        let frame0_b = Arc::new(tokio::sync::Notify::new());
        let shutdown_b = Arc::new(tokio::sync::Notify::new());
        let frames_b_srv = Arc::clone(&frames_b);
        let frame0_b_srv = Arc::clone(&frame0_b);
        let shutdown_b_srv = Arc::clone(&shutdown_b);

        let (endpoint_b, server_handle_b) = spawn_loopback_ws(move |mut ws| async move {
            ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                .await
                .unwrap();
            loop {
                tokio::select! {
                    msg = ws.next() => match msg {
                        Some(Ok(Message::Binary(data))) => {
                            frames_b_srv.lock().push(data.to_vec());
                            ws.send(Message::Text(xfyun_result_json("部分文本", false).into()))
                                .await
                                .unwrap();
                            frame0_b_srv.notify_waiters();
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    },
                    _ = shutdown_b_srv.notified() => {
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
        })
        .await;

        let asr_b = Arc::new(
            XfyunStreamingASR::new(XfyunCredentials {
                app_id: "test-app".into(),
                api_key: "test-key".into(),
            })
            .with_endpoint(endpoint_b)
            .with_frame_send_timeout(Duration::from_millis(60)),
        );
        struct PartialSignalSink {
            notify: Arc<tokio::sync::Notify>,
            texts: ParkingMutex<Vec<String>>,
        }
        impl TextStreamSink for PartialSignalSink {
            fn publish(&self, chunk: TextStreamChunk) -> Result<(), crate::BackendError> {
                self.texts.lock().push(chunk.text);
                self.notify.notify_waiters();
                Ok(())
            }
        }
        let partial_seen = Arc::new(tokio::sync::Notify::new());
        let sink_b = Arc::new(PartialSignalSink {
            notify: Arc::clone(&partial_seen),
            texts: ParkingMutex::new(Vec::new()),
        });
        asr_b.set_partial_sink(sink_b.clone());
        asr_b.open_session().await.expect("open session B");

        asr_b.consume_pcm_chunk(&vec![0x55u8; TARGET_AUDIO_CHUNK_BYTES]);
        tokio::time::timeout(Duration::from_secs(2), frame0_b.notified())
            .await
            .expect("frame 0 B arrives");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if sink_b.texts.lock().last().map(String::as_str) == Some("部分文本") {
                    break;
                }
                partial_seen.notified().await;
            }
        })
        .await
        .expect("partial snapshot recorded");

        let writer_guard_b = asr_b.writer.lock().await;
        asr_b.consume_pcm_chunk(&vec![0x66u8; TARGET_AUDIO_CHUNK_BYTES]);
        assert!(asr_b.send_last_frame().await.is_err());
        drop(writer_guard_b);

        let salvaged = asr_b
            .await_final_result()
            .await
            .expect("short session fallback salvages partial");
        assert_eq!(salvaged.text, "部分文本");
        assert!(frames_b
            .lock()
            .iter()
            .all(|f| f.as_slice() != br#"{"end": true}"#));
        shutdown_b.notify_waiters();
        server_handle_b.await.expect("server join B");
    }

    #[tokio::test]
    async fn cancel_wakes_paced_send_worker_promptly() {
        let frames = Arc::new(ParkingMutex::new(Vec::<Vec<u8>>::new()));
        let frame0_seen = Arc::new(tokio::sync::Notify::new());
        let shutdown_srv = Arc::new(tokio::sync::Notify::new());
        let frames_srv = Arc::clone(&frames);
        let frame0_seen_srv = Arc::clone(&frame0_seen);
        let shutdown_srv_task = Arc::clone(&shutdown_srv);

        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                .await
                .unwrap();
            loop {
                tokio::select! {
                    msg = ws.next() => match msg {
                        Some(Ok(Message::Binary(data))) => {
                            frames_srv.lock().push(data.to_vec());
                            frame0_seen_srv.notify_waiters();
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    },
                    _ = shutdown_srv_task.notified() => {
                        let _ = ws.close(None).await;
                        break;
                    }
                }
            }
        })
        .await;

        let asr = loopback_asr(endpoint);
        asr.open_session().await.expect("open session");

        // 100 chunks = ~4,000 ms of paced audio queued; cancel must interrupt pacing promptly.
        asr.consume_pcm_chunk(&vec![0x77u8; TARGET_AUDIO_CHUNK_BYTES * 100]);
        tokio::time::timeout(Duration::from_secs(2), frame0_seen.notified())
            .await
            .expect("frame 0 arrives");

        let started_cancel = Instant::now();
        asr.cancel();
        let res = tokio::time::timeout(Duration::from_millis(250), asr.await_final_result())
            .await
            .expect("cancel resolves await_final_result promptly");
        assert!(matches!(res, Err(XfyunASRError::NoFinalResult)));
        assert!(started_cancel.elapsed() < Duration::from_millis(250));
        assert!(asr.send_last_frame().await.is_err());

        tokio::time::sleep(Duration::from_millis(80)).await;
        let received_count = frames.lock().len();
        assert!(
            received_count <= 5,
            "expected worker to stop promptly after cancel, received {received_count} frames"
        );
        assert!(frames
            .lock()
            .iter()
            .all(|f| f.as_slice() != br#"{"end": true}"#));

        shutdown_srv.notify_waiters();
        server_handle.await.expect("server join");
    }
}
