#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Volcengine SAUC bigmodel streaming ASR client.
//!
//! Sends PCM frames, combines recognition updates and waits for the protocol's
//! final response. A `definite=true` utterance commits that segment only; it does
//! not end the stream or prevent later audio from being sent.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderValue;
use tokio_tungstenite::tungstenite::{handshake::client::Request as WebSocketRequest, Message};
use uuid::Uuid;

use crate::config::{TaskSpawner, TokioTaskSpawner};

use super::frame::{self, Flags, MessageType, Serialization};
use super::stream_send::{send_ws_message_with_timeout, PacedSendClock, SharedWriter, WsStream};
use super::{AudioConsumer, DictionaryHotword, RawTranscript};
use crate::ports::{TextStreamChunk, TextStreamSink};

/// Official "bigmodel streaming ASR API" (bidirectional streaming, optimized)
/// endpoint: https://www.volcengine.com/docs/6561/1354869
/// Both auth modes share this endpoint; only the handshake auth headers differ.
const ENDPOINT_APP_ID_TOKEN: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
const ENDPOINT_API_KEY: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
/// Agent Plan uses a dedicated subscription endpoint with API-key authentication.
/// https://docs.volcengine.com/docs/82379/2516286
const ENDPOINT_AGENT_PLAN: &str = "wss://openspeech.bytedance.com/api/v3/plan/sauc/bigmodel_async";
/// 200 ms of 16 kHz / 16-bit / mono PCM.
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 6_400;
/// 16 kHz · 16-bit · mono = 32 000 bytes/sec → 32 bytes/ms.
const BYTES_PER_MS: f64 = 32.0;
const BYTES_PER_MS_U64: u64 = 32;
const FRAME_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const HOTWORD_CAP: usize = 80;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);

/// On a poor network the TLS/WebSocket handshake can hang until the OS-level TCP
/// timeout (tens of seconds), leaving the user stuck on "Starting" with no voice
/// input. The coordinator's global timeout covers only `await_final_result`, not
/// `open_session`, so the handshake needs its own cap here: fail fast and retry on
/// timeout instead of freezing.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A single network blip (connection reset / transient DNS failure) used to fail the
/// whole dictation; a few retries make blips recoverable. `AuthRejected` (bad
/// credentials) is excluded — retrying never helps, it only delays the error.
const CONNECT_MAX_ATTEMPTS: usize = 3;
const CONNECT_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// Volcengine ASR authentication mode.
///
/// - `AppIdToken`: legacy voice-console apps, authenticated with the
///   `X-Api-App-Key` + `X-Api-Access-Key` header pair.
/// - `ApiKey`: normal service API key or the Agent Plan-specific API key,
///   authenticated with a single `X-Api-Key` header.
///
/// On the normal service, both modes share the WebSocket endpoint and binary frame
/// protocol; only the handshake auth headers differ. Agent Plan selects its dedicated
/// endpoint per service and always uses ApiKey auth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VolcengineAuthMode {
    AppIdToken,
    ApiKey,
}

impl VolcengineAuthMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "api_key" => Self::ApiKey,
            _ => Self::AppIdToken,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AppIdToken => "app_id_token",
            Self::ApiKey => "api_key",
        }
    }

    /// Whether the credentials required by the current mode are complete (uniform
    /// trim semantics).
    ///
    /// `secret` semantics depend on the mode: AppIdToken = Access Token (legacy voice
    /// console), ApiKey = normal service or Agent Plan ASR API key. `app_id` is only
    /// required non-empty in AppIdToken mode.
    ///
    /// Every entry point that judges credential completeness per mode (`open_session`,
    /// `volcengine_configured`, `ensure_asr_credentials`) should reuse this method so
    /// the three rules cannot drift.
    pub fn auth_ok(&self, app_id: &str, secret: &str) -> bool {
        let app_id_ok = match self {
            Self::AppIdToken => !app_id.trim().is_empty(),
            Self::ApiKey => true,
        };
        app_id_ok && !secret.trim().is_empty()
    }
}

/// Service selection is separate from the standard service's authentication mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VolcengineService {
    #[default]
    Standard,
    AgentPlan,
}

impl VolcengineService {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value.trim() {
            "" | "standard" => Ok(Self::Standard),
            "agent_plan" => Ok(Self::AgentPlan),
            _ => Err("volcengineServiceInvalid"),
        }
    }

    pub fn auth_mode(self, configured: VolcengineAuthMode) -> VolcengineAuthMode {
        match self {
            Self::Standard => configured,
            Self::AgentPlan => VolcengineAuthMode::ApiKey,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VolcengineCredentials {
    pub service: VolcengineService,
    pub auth_mode: VolcengineAuthMode,
    /// App ID (AppIdToken mode; empty in ApiKey mode).
    pub app_id: String,
    /// Access Token (AppIdToken mode) or API Key (ApiKey mode).
    pub access_token: String,
    pub resource_id: String,
}

impl VolcengineCredentials {
    pub fn default_resource_id() -> &'static str {
        "volc.seedasr.sauc.duration"
    }

    /// Uses the default Resource ID when unconfigured or whitespace-only; keeps the
    /// original value of a non-empty configuration.
    pub fn resolve_resource_id(configured: Option<String>) -> String {
        configured
            .filter(|resource_id| !resource_id.trim().is_empty())
            .unwrap_or_else(|| Self::default_resource_id().to_string())
    }

    /// Whether the credentials satisfy the current auth mode (uniform trim semantics,
    /// see [`VolcengineAuthMode::auth_ok`]).
    pub fn auth_ok(&self) -> bool {
        self.service
            .auth_mode(self.auth_mode.clone())
            .auth_ok(&self.app_id, &self.access_token)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VolcengineASRError {
    #[error("credentials missing")]
    CredentialsMissing,
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    /// WebSocket handshake returned 401 / 403: credentials rejected. Distinguished
    /// from `ConnectionFailed` (DNS/TLS/network-layer failure) — the former usually
    /// means a wrong App ID / Access Token / Resource ID or bigmodel not enabled for
    /// the account; the latter means network down / firewall / DNS. The message is
    /// short; the reasons are documented rather than piling long guidance into the
    /// capsule.
    // Keep an explicit HTTP marker so provider validation can retain the status
    // when it removes untrusted provider response details.
    #[error("凭据被拒（HTTP {0}）")]
    AuthRejected(u16),
    /// WebSocket handshake returned 429: too many requests / account throttled.
    /// Classified separately instead of falling into `ConnectionFailed` — the latter
    /// would be retried immediately 3 times by `connect_with_retry` as a network blip,
    /// worsening the throttle and showing a vague "network failure" message. Like
    /// `AuthRejected`, this short-circuits without retry and returns a clear message
    /// so the user knows it is throttling, not a disconnect.
    #[error("请求过多，账号被限流（HTTP {0}）")]
    RateLimited(u16),
    #[error("no final result")]
    NoFinalResult,
    #[error("final result timed out")]
    FinalResultTimeout,
    #[error("decode failed: {0}")]
    DecodeFailed(String),
}

enum SendItem {
    Audio {
        seq: i32,
        chunk: Vec<u8>,
    },
    End {
        final_seq: i32,
        done: oneshot::Sender<Result<(), VolcengineASRError>>,
    },
}

/// Sync state shared across the receive loop, the public API, and the
/// audio-consumer fast path.
#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    next_sequence: i32,
    bytes_received: u64,
    bytes_written: u64,
    frames_written: usize,
    is_connected: bool,
    ending: bool,
    final_frame_sent: bool,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    send_error: Option<VolcengineASRError>,
    cancel_tx: Option<watch::Sender<bool>>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, VolcengineASRError>>>,
    start: Option<Instant>,
    /// Accumulated transcript of the latest partial (non-final). Returned to the
    /// caller as a fallback when the server closes the connection or the network
    /// drops before the final frame, so recognized text is not lost just because no
    /// final arrived.
    last_partial_text: String,
}

pub struct VolcengineStreamingASR {
    credentials: VolcengineCredentials,
    #[cfg(test)]
    endpoint_override: Option<String>,
    frame_send_timeout: Duration,
    task_spawner: Arc<dyn TaskSpawner>,
    hotwords: Vec<DictionaryHotword>,
    state: ParkingMutex<SyncState>,
    /// Guards the WebSocket write half so concurrent `send` calls serialize.
    /// Stored as Arc so spawned send tasks can hold their own clone — independent
    /// of the lifetime of any particular `&self` borrow.
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, VolcengineASRError>>>>,
    partial_sink: ParkingMutex<Option<Arc<dyn TextStreamSink>>>,
}

impl VolcengineStreamingASR {
    pub fn new(credentials: VolcengineCredentials, hotwords: Vec<DictionaryHotword>) -> Self {
        Self::with_task_spawner(credentials, hotwords, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: VolcengineCredentials,
        hotwords: Vec<DictionaryHotword>,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self {
            credentials,
            #[cfg(test)]
            endpoint_override: None,
            frame_send_timeout: FRAME_SEND_TIMEOUT,
            task_spawner,
            hotwords,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            partial_sink: ParkingMutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint_override = Some(endpoint.into());
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
        st.send_error.is_none() && st.final_frame_sent && st.bytes_written == st.bytes_received
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.partial_sink.lock() = Some(sink);
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), VolcengineASRError> {
        let creds = &self.credentials;
        // Route through VolcengineCredentials::auth_ok (trim semantics) so this stays
        // the same rule as the overview-page credential status check and the dictation
        // preflight.
        if !creds.auth_ok() || creds.resource_id.trim().is_empty() {
            return Err(VolcengineASRError::CredentialsMissing);
        }

        let connect_id = Uuid::new_v4().to_string();
        let ws = self.connect_with_retry(&connect_id).await?;
        let (write, read) = ws.split();

        let (tx, rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);

        // Reset sync state for the new session.
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.next_sequence = 1;
            st.is_connected = true;
            st.final_tx = Some(tx);
            st.send_tx = Some(send_tx);
            st.cancel_tx = Some(cancel_tx);
            st.start = Some(Instant::now());
        }
        *self.final_rx.lock() = Some(rx);
        *self.writer.lock().await = Some(write);

        // Spawn the single FIFO audio worker: drains queued audio with 2x realtime
        // pacing before sending the NegativeSequence terminal frame.
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
                    SendItem::Audio { seq, chunk } => {
                        let Some(slot) = clock.wait_slot(&mut cancel_rx).await else {
                            break;
                        };
                        let chunk_len = chunk.len();
                        let frame = frame::build(
                            MessageType::AudioOnlyRequest,
                            Flags::PositiveSequence,
                            Serialization::None,
                            &chunk,
                            Some(seq),
                        );
                        if let Err(ws_err) = send_ws_message_with_timeout(
                            &writer_for_worker,
                            Message::Binary(frame.into()),
                            frame_send_timeout,
                            &mut cancel_rx,
                        )
                        .await
                        {
                            let err = VolcengineASRError::ConnectionFailed(
                                ws_err.into_message("websocket not open"),
                            );
                            log::error!("[asr] audio frame seq={} send 失败: {}", seq, err);
                            if let Some(this) = weak_self_for_worker.upgrade() {
                                this.record_send_failure(err);
                            }
                            break;
                        }
                        if let Some(this) = weak_self_for_worker.upgrade() {
                            let mut st = this.state.lock();
                            st.bytes_written = st.bytes_written.saturating_add(chunk_len as u64);
                            st.frames_written = st.frames_written.saturating_add(1);
                        }
                        clock.mark_sent(slot, chunk_len, BYTES_PER_MS_U64, 2);
                    }
                    SendItem::End { final_seq, done } => {
                        let frame = frame::build(
                            MessageType::AudioOnlyRequest,
                            Flags::NegativeSequence,
                            Serialization::None,
                            &[],
                            Some(final_seq),
                        );
                        let result = send_ws_message_with_timeout(
                            &writer_for_worker,
                            Message::Binary(frame.into()),
                            frame_send_timeout,
                            &mut cancel_rx,
                        )
                        .await
                        .map_err(|ws_err| {
                            VolcengineASRError::ConnectionFailed(
                                ws_err.into_message("websocket not open"),
                            )
                        });
                        match &result {
                            Ok(()) => {
                                if let Some(this) = weak_self_for_worker.upgrade() {
                                    this.state.lock().final_frame_sent = true;
                                }
                            }
                            Err(err) => {
                                log::error!("[asr] final frame send 失败: {err}");
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

        // Send the first frame: full client request with seq=1.
        let payload_json = self.build_first_frame_payload(&connect_id);
        let payload_bytes = serde_json::to_vec(&payload_json)
            .map_err(|e| VolcengineASRError::DecodeFailed(e.to_string()))?;
        let first_seq = self.allocate_positive_seq();
        let frame = frame::build(
            MessageType::FullClientRequest,
            Flags::PositiveSequence,
            Serialization::Json,
            &payload_bytes,
            Some(first_seq),
        );
        if let Err(err) = send_binary(&self.writer, frame).await {
            self.cancel();
            return Err(err);
        }

        // Spawn the receive loop. Holds a Weak<Self> so it doesn't keep
        // the struct alive forever if callers drop their Arcs.
        let weak_self = Arc::downgrade(self);
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            let mut read = read;
            while let Some(msg) = read.next().await {
                let Some(this) = weak_self.upgrade() else {
                    break;
                };
                match msg {
                    Ok(Message::Binary(data)) => {
                        if !this.handle_frame(&data) {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        // Server closed without sending final -> fall back to the
                        // latest partial so recognized text is not lost.
                        this.fallback_to_partial_or_error(VolcengineASRError::NoFinalResult);
                        break;
                    }
                    Ok(_) => { /* ignore text/ping/pong */ }
                    Err(e) => {
                        log::error!("[asr] receive loop error: {}", e);
                        // On network interruption, also fall back to the partial so
                        // the user at least keeps the recognized portion.
                        this.fallback_to_partial_or_error(VolcengineASRError::ConnectionFailed(
                            e.to_string(),
                        ));
                        break;
                    }
                }
                if !this.state.lock().is_connected {
                    break;
                }
            }
        }));

        Ok(())
    }

    /// Build the WebSocket handshake request (endpoint + auth headers). Rebuilt
    /// per connect attempt because `connect_async` consumes the request and
    /// `http::Request` is not `Clone`.
    fn build_connect_request(
        &self,
        connect_id: &str,
        request_id: &str,
    ) -> Result<WebSocketRequest, VolcengineASRError> {
        let auth_mode = self
            .credentials
            .service
            .auth_mode(self.credentials.auth_mode.clone());
        #[cfg(test)]
        if let Some(ref endpoint) = self.endpoint_override {
            let mut request = endpoint
                .as_str()
                .into_client_request()
                .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?;
            let headers = request.headers_mut();
            headers.insert(
                "X-Api-Connect-Id",
                HeaderValue::from_str(connect_id)
                    .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
            );
            headers.insert(
                "X-Api-Request-Id",
                HeaderValue::from_str(request_id)
                    .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
            );
            return Ok(request);
        }
        let endpoint = match (self.credentials.service, &auth_mode) {
            (VolcengineService::AgentPlan, _) => ENDPOINT_AGENT_PLAN,
            (_, VolcengineAuthMode::AppIdToken) => ENDPOINT_APP_ID_TOKEN,
            (_, VolcengineAuthMode::ApiKey) => ENDPOINT_API_KEY,
        };
        let mut request = endpoint
            .into_client_request()
            .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?;
        let headers = request.headers_mut();

        // Headers per auth mode:
        // - AppIdToken: X-Api-App-Key + X-Api-Access-Key (legacy voice console)
        // - ApiKey: X-Api-Key (normal service or Agent Plan ASR API key, single header)
        match auth_mode {
            VolcengineAuthMode::AppIdToken => {
                headers.insert(
                    "X-Api-App-Key",
                    HeaderValue::from_str(&self.credentials.app_id)
                        .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
                );
                headers.insert(
                    "X-Api-Access-Key",
                    HeaderValue::from_str(&self.credentials.access_token)
                        .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
                );
            }
            VolcengineAuthMode::ApiKey => {
                headers.insert(
                    "X-Api-Key",
                    HeaderValue::from_str(&self.credentials.access_token)
                        .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
                );
            }
        }

        headers.insert(
            "X-Api-Resource-Id",
            HeaderValue::from_str(&self.credentials.resource_id)
                .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
        );
        headers.insert(
            "X-Api-Connect-Id",
            HeaderValue::from_str(connect_id)
                .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
        );
        // The official auth table (docs/6561/1354869) requires two more headers:
        // X-Api-Request-Id (task ID; officially a random UUID is recommended, generated
        // independently per connect attempt) and X-Api-Sequence (send sequence, fixed
        // at -1).
        headers.insert(
            "X-Api-Request-Id",
            HeaderValue::from_str(request_id)
                .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
        );
        headers.insert("X-Api-Sequence", HeaderValue::from_static("-1"));
        Ok(request)
    }

    /// Connect with a per-attempt timeout and bounded retries so a poor network
    /// (hung handshake or a transient blip) doesn't kill the whole dictation.
    /// `AuthRejected` / `RateLimited` short-circuit — bad credentials never heal on
    /// retry, and hammering a rate-limited account only makes the throttle worse.
    async fn connect_with_retry(&self, connect_id: &str) -> Result<WsStream, VolcengineASRError> {
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let request_id = Uuid::new_v4().to_string();
            let request = self.build_connect_request(connect_id, &request_id)?;
            log::info!(
                "[asr] Volcengine connect endpoint={} connect_id={} request_id={}",
                request.uri(),
                connect_id,
                request_id
            );
            match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request)).await {
                Ok(Ok((ws, response))) => {
                    log::info!(
                        "[asr] Volcengine connected connect_id={} log_id={}",
                        connect_id,
                        response
                            .headers()
                            .get("X-Tt-Logid")
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("-")
                    );
                    return Ok(ws);
                }
                Ok(Err(e)) => {
                    if let tokio_tungstenite::tungstenite::Error::Http(response) = &e {
                        // Failed handshakes also carry a provider trace ID. Log
                        // only status and trace IDs, never auth headers or bodies.
                        log::warn!(
                            "[asr] Volcengine handshake rejected status={} connect_id={} request_id={} log_id={}",
                            response.status().as_u16(),
                            connect_id,
                            request_id,
                            response
                                .headers()
                                .get("X-Tt-Logid")
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or("-")
                        );
                    }
                    let classified = classify_connect_error(e);
                    if is_non_retryable(&classified) || attempt >= CONNECT_MAX_ATTEMPTS {
                        return Err(classified);
                    }
                    log::warn!(
                        "[asr] 连接尝试 {attempt}/{CONNECT_MAX_ATTEMPTS} 失败: {classified}；重试中"
                    );
                }
                Err(_) => {
                    if attempt >= CONNECT_MAX_ATTEMPTS {
                        return Err(VolcengineASRError::ConnectionFailed(format!(
                            "连接超时（{} ms）",
                            CONNECT_TIMEOUT.as_millis()
                        )));
                    }
                    log::warn!(
                        "[asr] 连接尝试 {attempt}/{CONNECT_MAX_ATTEMPTS} 超时（{} ms）；重试中",
                        CONNECT_TIMEOUT.as_millis()
                    );
                }
            }
            tokio::time::sleep(CONNECT_RETRY_BACKOFF * attempt as u32).await;
        }
    }

    pub async fn send_last_frame(&self) -> Result<(), VolcengineASRError> {
        let (done_tx, done_rx) = oneshot::channel();
        {
            let mut st = self.state.lock();
            if let Some(ref err) = st.send_error {
                return Err(err.clone());
            }
            let Some(tx) = st.send_tx.take() else {
                return Err(VolcengineASRError::ConnectionFailed(
                    "websocket closed before final frame".into(),
                ));
            };
            st.ending = true;
            if !st.pending_audio.is_empty() {
                let leftover = std::mem::take(&mut st.pending_audio);
                let seq = st.next_sequence;
                st.next_sequence += 1;
                if tx
                    .send(SendItem::Audio {
                        seq,
                        chunk: leftover,
                    })
                    .is_err()
                {
                    return Err(st.send_error.clone().unwrap_or_else(|| {
                        VolcengineASRError::ConnectionFailed("audio send queue closed".into())
                    }));
                }
            }
            let final_seq = -st.next_sequence;
            st.next_sequence += 1;
            if tx
                .send(SendItem::End {
                    final_seq,
                    done: done_tx,
                })
                .is_err()
            {
                return Err(st.send_error.clone().unwrap_or_else(|| {
                    VolcengineASRError::ConnectionFailed("audio send queue closed".into())
                }));
            }
        }

        match done_rx.await {
            Ok(inner) => inner?,
            Err(_) => {
                return Err(self.state.lock().send_error.clone().unwrap_or_else(|| {
                    VolcengineASRError::ConnectionFailed(
                        "audio send worker stopped before final frame".into(),
                    )
                }));
            }
        }

        let (written_frames, written_bytes, received_bytes) = {
            let st = self.state.lock();
            (st.frames_written, st.bytes_written, st.bytes_received)
        };
        let duration_ms = (written_bytes as f64 / BYTES_PER_MS) as u64;
        log::info!(
            "[asr] 发送总结：{} written frames, {}/{} bytes (~{} ms)",
            written_frames,
            written_bytes,
            received_bytes,
            duration_ms
        );
        Ok(())
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, VolcengineASRError> {
        self.await_final_result_with_timeout(FINAL_RESULT_TIMEOUT)
            .await
    }

    pub async fn await_final_result_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<RawTranscript, VolcengineASRError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(VolcengineASRError::NoFinalResult);
        };
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(VolcengineASRError::NoFinalResult),
            Err(_) => {
                log::error!(
                    "[asr] final result timed out after {} ms",
                    timeout.as_millis()
                );
                self.cancel();
                Err(VolcengineASRError::FinalResultTimeout)
            }
        }
    }

    pub fn cancel(&self) {
        {
            let mut st = self.state.lock();
            st.is_connected = false;
            st.pending_audio.clear();
            st.send_tx = None;
            if let Some(tx) = st.cancel_tx.as_ref() {
                let _ = tx.send(true);
            }
        }
        // Close the writer asynchronously so the receive loop sees EOF. The
        // host-provided spawner also handles synchronous teardown callers.
        let writer = Arc::clone(&self.writer);
        self.task_spawner.spawn(Box::pin(async move {
            if let Some(mut w) = writer.lock().await.take() {
                let _ = w.close().await;
            }
        }));
        self.signal_error(VolcengineASRError::NoFinalResult);
    }

    fn record_send_failure(&self, err: VolcengineASRError) {
        self.resolve_with_partial_or_error(err, true);
    }

    // ---- internals ----

    fn build_first_frame_payload(&self, connect_id: &str) -> Value {
        let mut request = json!({
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "show_utterances": true,
            "enable_speaker_info": true,
        });
        if let Some(context) = hotword_context(&self.hotwords) {
            request["context"] = Value::String(context);
            let enabled_count = self.hotwords.iter().filter(|h| h.enabled).count();
            log::info!("[asr] hotwords injected: {}", enabled_count);
        }
        json!({
            "user": { "uid": connect_id },
            "audio": {
                "format": "pcm",
                "rate": 16000,
                "bits": 16,
                "channel": 1,
                "codec": "raw",
            },
            "request": request,
        })
    }

    fn allocate_positive_seq(&self) -> i32 {
        let mut st = self.state.lock();
        let s = st.next_sequence;
        st.next_sequence += 1;
        s
    }

    /// Returns `false` once the session has terminated (caller should stop reading).
    fn handle_frame(&self, data: &[u8]) -> bool {
        let Some(parsed) = frame::parse(data) else {
            log::error!("[asr] 帧解析失败 raw={}", hex_prefix(data, 32));
            return true;
        };

        if parsed.message_type == Some(MessageType::ErrorMessage) {
            let code = parsed.error_code.unwrap_or(0);
            log::error!("[asr] Volcengine error frame code={code}");
            let err = VolcengineASRError::ConnectionFailed(format!("Volcengine ASR error {code}"));
            {
                let mut st = self.state.lock();
                if st.send_error.is_none() {
                    st.send_error = Some(err.clone());
                }
                st.is_connected = false;
                st.send_tx = None;
                if let Some(tx) = st.cancel_tx.as_ref() {
                    let _ = tx.send(true);
                }
            }
            self.signal_error(err);
            return false;
        }

        if parsed.message_type != Some(MessageType::FullServerResponse) {
            return true;
        }

        let json: Value = match serde_json::from_slice(&parsed.payload) {
            Ok(v) => v,
            Err(_) => return true,
        };
        let Some(result) = normalized_result(&json) else {
            return true;
        };

        // Trust only the frame-header flags (lastPacket / negativeSequence) for the
        // stream-end signal. utterance.definite=true was previously mistaken for end
        // of stream — but it only means "this segment's audio is settled"; the user
        // may still be talking. Closing the receive loop on the first definite=true
        // lost everything said afterwards (measured: 9 seconds lost).
        let has_final = parsed.is_final();
        let mut full_text = result
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(utterances) = result.get("utterances").and_then(|v| v.as_array()) {
            // --- Speaker filtering: keep only the primary speaker (longest talk time) ---
            // 1. Tally each speaker's total talk time.
            let mut speaker_durations: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            for u in utterances.iter() {
                if let (Some(speaker), Some(start), Some(end)) = (
                    u.get("speaker").and_then(|s| s.as_str()),
                    u.get("start_time").and_then(|t| t.as_u64()),
                    u.get("end_time").and_then(|t| t.as_u64()),
                ) {
                    let dur = end.saturating_sub(start);
                    *speaker_durations.entry(speaker.to_string()).or_insert(0) += dur;
                }
            }

            // 2. Pick the speaker with the longest talk time (the primary speaker).
            let primary_speaker: Option<String> = speaker_durations
                .iter()
                .max_by_key(|(_, &dur)| dur)
                .map(|(s, _)| s.clone());

            // 3. Join only the primary speaker's text; with no speaker tags at all,
            // fall back to joining everything.
            let pieces: Vec<&str> = if let Some(ref primary) = primary_speaker {
                utterances
                    .iter()
                    .filter(|u| {
                        u.get("speaker")
                            .and_then(|s| s.as_str())
                            .map(|s| s == primary.as_str())
                            .unwrap_or(true) // utterances without a speaker field are kept
                    })
                    .filter_map(|u| u.get("text").and_then(|t| t.as_str()))
                    .collect()
            } else {
                utterances
                    .iter()
                    .filter_map(|u| u.get("text").and_then(|t| t.as_str()))
                    .collect()
            };

            if !pieces.is_empty() {
                full_text = pieces.join("");
                if let Some(ref primary) = primary_speaker {
                    let filtered_count = utterances.len().saturating_sub(pieces.len());
                    if filtered_count > 0 {
                        log::info!(
                            "[asr] speaker filter: primary={}, kept={}, filtered={}",
                            primary,
                            pieces.len(),
                            filtered_count
                        );
                    }
                }
            }
        }

        // Offset zero replaces the current transcript, so publish the full snapshot,
        // including recognition corrections rather than only an appended suffix.
        if !full_text.is_empty() {
            if !has_final {
                self.state.lock().last_partial_text = full_text.clone();
            }
            if let Some(sink) = self.partial_sink.lock().clone() {
                let _ = sink.publish(TextStreamChunk {
                    text: full_text.clone(),
                    offset: 0,
                });
            }
        }

        if has_final {
            let duration_ms = self
                .state
                .lock()
                .start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let transcript = RawTranscript {
                text: full_text,
                duration_ms,
            };
            self.signal_success(transcript);
            {
                let mut st = self.state.lock();
                st.is_connected = false;
                st.send_tx = None;
                if let Some(tx) = st.cancel_tx.as_ref() {
                    let _ = tx.send(true);
                }
            }
            return false;
        }
        true
    }

    fn signal_success(&self, transcript: RawTranscript) {
        let tx = self.state.lock().final_tx.take();
        if let Some(tx) = tx {
            let _ = tx.send(Ok(transcript));
        }
    }

    fn signal_error(&self, err: VolcengineASRError) {
        let tx = self.state.lock().final_tx.take();
        if let Some(tx) = tx {
            let _ = tx.send(Err(err));
        }
    }

    /// Called on server close / network interruption: returns cached partial text as
    /// a fallback transcript if available, errors otherwise. Works with
    /// `last_partial_text` to guarantee "at least no loss of already-recognized
    /// speech".
    fn fallback_to_partial_or_error(&self, err: VolcengineASRError) {
        self.resolve_with_partial_or_error(err, false);
    }

    fn resolve_with_partial_or_error(&self, err: VolcengineASRError, clear_pending: bool) {
        let (tx, outcome, partial_chars) = {
            let mut st = self.state.lock();
            if st.send_error.is_none() {
                st.send_error = Some(err.clone());
            }
            st.is_connected = false;
            st.send_tx = None;
            if clear_pending {
                st.pending_audio.clear();
            }
            if let Some(cancel_tx) = st.cancel_tx.as_ref() {
                let _ = cancel_tx.send(true);
            }
            let partial = st.last_partial_text.clone();
            let duration_ms = st
                .start
                .map(|s| s.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let tx = st.final_tx.take();
            if !partial.is_empty() {
                let chars = partial.chars().count();
                (
                    tx,
                    Ok(RawTranscript {
                        text: partial,
                        duration_ms,
                    }),
                    Some(chars),
                )
            } else {
                (tx, Err(err.clone()), None)
            }
        };
        if let Some(chars) = partial_chars {
            log::warn!("[asr] {}; 使用 partial 兜底（{} 字）", err, chars);
        }
        if let Some(tx) = tx {
            let _ = tx.send(outcome);
        }
    }
}

impl AudioConsumer for VolcengineStreamingASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let mut st = self.state.lock();
        if st.ending || (!st.is_connected && st.bytes_received == 0 && st.send_tx.is_none()) {
            return;
        }
        st.bytes_received = st.bytes_received.saturating_add(pcm.len() as u64);
        if !st.is_connected || st.send_error.is_some() {
            return;
        }
        let Some(tx) = st.send_tx.clone() else {
            return;
        };
        st.pending_audio.extend_from_slice(pcm);

        let mut chunks: Vec<(i32, Vec<u8>)> = Vec::new();
        while st.pending_audio.len() >= TARGET_AUDIO_CHUNK_BYTES {
            let chunk: Vec<u8> = st.pending_audio.drain(..TARGET_AUDIO_CHUNK_BYTES).collect();
            let seq = st.next_sequence;
            st.next_sequence += 1;
            chunks.push((seq, chunk));
        }

        for (seq, chunk) in chunks {
            if tx.send(SendItem::Audio { seq, chunk }).is_err() {
                log::warn!("[asr] audio queue closed; dropping subsequent frames");
                return;
            }
        }
    }
}

async fn send_binary(writer: &SharedWriter, data: Vec<u8>) -> Result<(), VolcengineASRError> {
    let mut guard = writer.lock().await;
    let Some(sink) = guard.as_mut() else {
        return Err(VolcengineASRError::ConnectionFailed(
            "websocket not open".into(),
        ));
    };
    sink.send(Message::Binary(data.into()))
        .await
        .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))
}

fn hex_prefix(data: &[u8], n: usize) -> String {
    data.iter()
        .take(n)
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join("")
}

fn normalized_result(json: &Value) -> Option<&Value> {
    if let Some(obj) = json.get("result") {
        if obj.is_object() {
            return Some(obj);
        }
        if let Some(arr) = obj.as_array() {
            if let Some(first) = arr.first() {
                return Some(first);
            }
        }
    }
    if json.get("text").and_then(|v| v.as_str()).is_some() {
        return Some(json);
    }
    None
}

/// Classifies tokio-tungstenite connect errors: HTTP 401 / 403 during the handshake
/// -> `AuthRejected` (credentials rejected; user should check App ID / Access Token /
/// account resource enablement); 429 -> `RateLimited` (too many requests; retrying
/// only adds fuel, so short-circuit with a clear message); anything else -> generic
/// `ConnectionFailed` (DNS / TLS / network layer). Lets the capsule message be
/// distinguished from a generic HTTP error.
fn classify_connect_error(err: tokio_tungstenite::tungstenite::Error) -> VolcengineASRError {
    use tokio_tungstenite::tungstenite::Error as WsError;
    if let WsError::Http(resp) = &err {
        let status = resp.status().as_u16();
        if status == 401 || status == 403 {
            return VolcengineASRError::AuthRejected(status);
        }
        if status == 429 {
            return VolcengineASRError::RateLimited(status);
        }
    }
    VolcengineASRError::ConnectionFailed(err.to_string())
}

/// Whether a handshake error is pointless to retry; `connect_with_retry` short-circuits
/// on these. Rejected credentials (401/403) and throttling (429) both qualify: the
/// former never becomes correct on retry, the latter only gets worse. Network-layer
/// errors are the ones worth retrying on a blip.
fn is_non_retryable(err: &VolcengineASRError) -> bool {
    matches!(
        err,
        VolcengineASRError::AuthRejected(_) | VolcengineASRError::RateLimited(_)
    )
}

fn hotword_context(entries: &[DictionaryHotword]) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    for entry in entries {
        if !entry.enabled {
            continue;
        }
        let trimmed = entry.phrase.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.iter().any(|w| w.eq_ignore_ascii_case(trimmed)) {
            continue;
        }
        seen.push(trimmed.to_string());
        if seen.len() >= HOTWORD_CAP {
            break;
        }
    }
    if seen.is_empty() {
        return None;
    }
    let words: Vec<Value> = seen.into_iter().map(|w| json!({ "word": w })).collect();
    let payload = json!({ "hotwords": words });
    serde_json::to_string(&payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_snapshots_keep_prefix_and_recognition_corrections() {
        let asr = VolcengineStreamingASR::new(
            VolcengineCredentials {
                service: VolcengineService::Standard,
                auth_mode: VolcengineAuthMode::AppIdToken,
                app_id: "test".into(),
                access_token: "test".into(),
                resource_id: VolcengineCredentials::default_resource_id().into(),
            },
            Vec::new(),
        );
        let sink = Arc::new(super::super::TranscriptCapture::default());
        asr.set_partial_sink(sink.clone());
        for text in ["你", "你好", "您好", "您好。世界"] {
            let payload =
                serde_json::to_vec(&serde_json::json!({"result": {"text": text}})).unwrap();
            let bytes = frame::build(
                MessageType::FullServerResponse,
                frame::Flags::None,
                frame::Serialization::Json,
                &payload,
                None,
            );
            assert!(asr.handle_frame(&bytes));
        }
        sink.assert_snapshots(&["你", "你好", "您好", "您好。世界"]);
    }

    #[test]
    fn simulated_two_minute_stream_keeps_routing_after_definite_segments() {
        let asr = VolcengineStreamingASR::new(
            VolcengineCredentials {
                service: VolcengineService::Standard,
                auth_mode: VolcengineAuthMode::AppIdToken,
                app_id: "test".into(),
                access_token: "test".into(),
                resource_id: VolcengineCredentials::default_resource_id().into(),
            },
            Vec::new(),
        );
        let sink = Arc::new(super::super::TranscriptCapture::default());
        asr.set_partial_sink(sink.clone());
        let mut snapshots = Vec::new();
        for packet in 1..=650 {
            let text = "段".repeat(packet);
            let payload = serde_json::to_vec(&json!({
                "audio_info": { "duration": 200 * packet },
                "result": {
                    "text": text,
                    "utterances": [{ "text": text, "definite": packet % 20 == 0 }],
                },
            }))
            .unwrap();
            let bytes = frame::build(
                MessageType::FullServerResponse,
                Flags::PositiveSequence,
                Serialization::Json,
                &payload,
                Some(packet as i32),
            );
            assert!(asr.handle_frame(&bytes));
            snapshots.push(text);
        }
        let expected = snapshots.iter().map(String::as_str).collect::<Vec<_>>();
        sink.assert_snapshots(&expected);
    }

    #[test]
    fn hotword_context_dedupes_case_insensitively_and_caps() {
        let mut entries = vec![
            DictionaryHotword {
                phrase: "Foo".into(),
                enabled: true,
            },
            DictionaryHotword {
                phrase: "foo".into(),
                enabled: true,
            },
            DictionaryHotword {
                phrase: "  ".into(),
                enabled: true,
            },
            DictionaryHotword {
                phrase: "Bar".into(),
                enabled: false,
            },
            DictionaryHotword {
                phrase: "Baz".into(),
                enabled: true,
            },
        ];
        for i in 0..200 {
            entries.push(DictionaryHotword {
                phrase: format!("w{}", i),
                enabled: true,
            });
        }
        let ctx = hotword_context(&entries).expect("should produce JSON");
        assert!(ctx.contains("\"hotwords\""));
        assert!(ctx.contains("Foo"));
        assert!(ctx.contains("Baz"));
        assert!(!ctx.contains("Bar"));
        let count = ctx.matches("\"word\"").count();
        assert!(count <= HOTWORD_CAP);
    }

    #[test]
    fn hotword_context_returns_none_when_all_disabled() {
        let entries = vec![DictionaryHotword {
            phrase: "Foo".into(),
            enabled: false,
        }];
        assert!(hotword_context(&entries).is_none());
    }

    #[test]
    fn default_resource_id_is_sauc_duration() {
        assert_eq!(
            VolcengineCredentials::default_resource_id(),
            "volc.seedasr.sauc.duration"
        );
    }

    #[test]
    fn resource_id_resolution_defaults_only_missing_or_blank_values() {
        let default_resource_id = "volc.seedasr.sauc.duration";
        let cases = [
            (None, default_resource_id),
            (Some(""), default_resource_id),
            (Some("   "), default_resource_id),
            (Some("\t\r\n"), default_resource_id),
            (
                Some("volc.bigasr.sauc.duration"),
                "volc.bigasr.sauc.duration",
            ),
            (Some(" custom.resource.id "), " custom.resource.id "),
        ];

        for (configured, expected) in cases {
            assert_eq!(
                VolcengineCredentials::resolve_resource_id(configured.map(str::to_string)),
                expected
            );
        }
    }

    #[test]
    fn auth_mode_from_str_roundtrips() {
        assert_eq!(
            VolcengineAuthMode::parse("api_key"),
            VolcengineAuthMode::ApiKey
        );
        assert_eq!(
            VolcengineAuthMode::parse("app_id_token"),
            VolcengineAuthMode::AppIdToken
        );
        assert_eq!(
            VolcengineAuthMode::parse(""),
            VolcengineAuthMode::AppIdToken
        ); // default fallback
        assert_eq!(VolcengineAuthMode::ApiKey.as_str(), "api_key");
        assert_eq!(VolcengineAuthMode::AppIdToken.as_str(), "app_id_token");
    }

    #[test]
    fn auth_ok_matches_mode_requirements() {
        let app_id_token = VolcengineAuthMode::AppIdToken;
        let api_key = VolcengineAuthMode::ApiKey;
        // AppIdToken: both app_id and secret must be non-empty.
        assert!(app_id_token.auth_ok("app", "token"));
        assert!(!app_id_token.auth_ok("", "token"));
        assert!(!app_id_token.auth_ok("app", ""));
        // Whitespace-only counts as unconfigured (uniform trim semantics, matching
        // volcengine_configured / preflight).
        assert!(!app_id_token.auth_ok("   ", "   "));
        // ApiKey: only the API key is required; app_id may be empty.
        assert!(api_key.auth_ok("", "key"));
        assert!(api_key.auth_ok("app", "key"));
        assert!(!api_key.auth_ok("app", ""));
        assert!(!api_key.auth_ok("", "   "));
    }

    #[test]
    fn build_connect_request_selects_endpoint_and_headers_per_mode() {
        let cases = [
            (
                VolcengineService::Standard,
                VolcengineAuthMode::AppIdToken,
                ENDPOINT_APP_ID_TOKEN,
                true,  // both headers (X-Api-App-Key / X-Api-Access-Key)
                false, // must not carry X-Api-Key
            ),
            (
                VolcengineService::Standard,
                VolcengineAuthMode::ApiKey,
                ENDPOINT_API_KEY,
                false, // must not carry the header pair
                true,  // single header X-Api-Key
            ),
            (
                VolcengineService::AgentPlan,
                VolcengineAuthMode::AppIdToken,
                ENDPOINT_AGENT_PLAN,
                false,
                true,
            ),
            (
                VolcengineService::AgentPlan,
                VolcengineAuthMode::ApiKey,
                ENDPOINT_AGENT_PLAN,
                false,
                true,
            ),
        ];
        for (service, mode, endpoint, expects_app_headers, expects_api_key) in cases {
            let asr = VolcengineStreamingASR::new(
                VolcengineCredentials {
                    service,
                    auth_mode: mode.clone(),
                    app_id: "app".into(),
                    access_token: "secret".into(),
                    resource_id: VolcengineCredentials::default_resource_id().into(),
                },
                vec![],
            );
            let req = asr
                .build_connect_request("connect-id", "request-id")
                .unwrap();
            assert_eq!(
                req.uri().to_string(),
                endpoint,
                "端点应随鉴权模式切换（mode={mode:?}）"
            );
            let headers = req.headers();
            assert_eq!(
                headers.contains_key("X-Api-App-Key"),
                expects_app_headers,
                "mode={mode:?} X-Api-App-Key"
            );
            assert_eq!(
                headers.contains_key("X-Api-Access-Key"),
                expects_app_headers,
                "mode={mode:?} X-Api-Access-Key"
            );
            assert_eq!(
                headers.contains_key("X-Api-Key"),
                expects_api_key,
                "mode={mode:?} X-Api-Key"
            );
            // Both modes must carry the resource and connect-id headers.
            assert!(headers.contains_key("X-Api-Resource-Id"));
            assert_eq!(headers.get("X-Api-Connect-Id").unwrap(), "connect-id");
            // The remaining headers required by the official auth table
            // (docs/6561/1354869).
            assert_eq!(headers.get("X-Api-Request-Id").unwrap(), "request-id");
            assert_ne!(
                headers.get("X-Api-Request-Id"),
                headers.get("X-Api-Connect-Id"),
                "任务 ID 不应复用会话连接 ID"
            );
            assert_eq!(headers.get("X-Api-Sequence").unwrap(), "-1");
        }
        // Regression: both auth modes share the same official endpoint
        // (docs/6561/1354869); ApiKey mode previously used the /api/v3/plan/... path,
        // causing 45000010 AuthenticationError.
        assert_eq!(ENDPOINT_API_KEY, ENDPOINT_APP_ID_TOKEN);
        assert_eq!(
            ENDPOINT_API_KEY,
            "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async"
        );
    }

    /// Builds a tungstenite error whose handshake phase returns the given HTTP status,
    /// for classification tests.
    fn http_ws_error(status: u16) -> tokio_tungstenite::tungstenite::Error {
        use tokio_tungstenite::tungstenite::http::Response;
        let resp = Response::builder()
            .status(status)
            .body(None)
            .expect("build test http response");
        tokio_tungstenite::tungstenite::Error::Http(Box::new(resp))
    }

    #[test]
    fn classify_429_is_rate_limited_not_connection_failed() {
        let classified = classify_connect_error(http_ws_error(429));
        assert!(
            matches!(classified, VolcengineASRError::RateLimited(429)),
            "429 应归类为 RateLimited 而非 ConnectionFailed，避免被当网络抖动重试"
        );
    }

    #[test]
    fn rate_limited_is_non_retryable_like_auth_rejected() {
        assert!(
            is_non_retryable(&VolcengineASRError::RateLimited(429)),
            "限流不可重试：重试只会加剧限流"
        );
        assert!(
            is_non_retryable(&VolcengineASRError::AuthRejected(401)),
            "凭据被拒不可重试"
        );
    }

    #[test]
    fn network_errors_stay_retryable() {
        // Generic network failures must stay retryable (blips recover) — they must not
        // be misjudged as short-circuit.
        assert!(!is_non_retryable(&VolcengineASRError::ConnectionFailed(
            "dns fail".into()
        )));
        assert!(!is_non_retryable(&VolcengineASRError::FinalResultTimeout));
    }

    #[test]
    fn classify_401_403_still_auth_rejected() {
        // Regression: adding the 429 classification does not affect the existing
        // 401 / 403 -> AuthRejected.
        assert!(matches!(
            classify_connect_error(http_ws_error(401)),
            VolcengineASRError::AuthRejected(401)
        ));
        assert!(matches!(
            classify_connect_error(http_ws_error(403)),
            VolcengineASRError::AuthRejected(403)
        ));
    }

    #[test]
    fn rate_limited_message_mentions_throttling_not_network() {
        // The message must clearly point to throttling / too many requests, not a
        // vague "network failure".
        let msg = VolcengineASRError::RateLimited(429).to_string();
        assert!(
            msg.contains("限流") || msg.contains("请求过多"),
            "文案: {msg}"
        );
        assert!(!msg.contains("网络"), "限流文案不应误导为网络失败: {msg}");
    }

    #[tokio::test]
    async fn await_final_result_returns_error_when_final_frame_never_arrives() {
        let asr = VolcengineStreamingASR::new(
            VolcengineCredentials {
                service: VolcengineService::Standard,
                auth_mode: VolcengineAuthMode::AppIdToken,
                app_id: "app".into(),
                access_token: "token".into(),
                resource_id: VolcengineCredentials::default_resource_id().into(),
            },
            Vec::new(),
        );
        let (tx, rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        *asr.final_rx.lock() = Some(rx);

        let result = asr
            .await_final_result_with_timeout(std::time::Duration::from_millis(10))
            .await;

        assert!(matches!(
            result,
            Err(VolcengineASRError::FinalResultTimeout)
        ));
    }

    async fn spawn_loopback_ws<F, Fut>(handler: F) -> (String, tokio::task::JoinHandle<()>)
    where
        F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            handler(ws).await;
        });
        (format!("ws://{addr}"), handle)
    }

    fn loopback_volcengine(endpoint: String) -> Arc<VolcengineStreamingASR> {
        Arc::new(
            VolcengineStreamingASR::new(
                VolcengineCredentials {
                    service: VolcengineService::Standard,
                    auth_mode: VolcengineAuthMode::ApiKey,
                    app_id: String::new(),
                    access_token: "test-key".into(),
                    resource_id: VolcengineCredentials::default_resource_id().into(),
                },
                Vec::new(),
            )
            .with_endpoint(endpoint),
        )
    }

    fn volc_final_response(text: &str) -> Vec<u8> {
        let payload = serde_json::to_vec(&json!({ "result": { "text": text } })).unwrap();
        frame::build(
            MessageType::FullServerResponse,
            Flags::NegativeSequence,
            Serialization::Json,
            &payload,
            Some(-1),
        )
    }

    #[tokio::test]
    async fn audio_frame_write_failure_stops_worker_and_fails_send_last_frame() {
        let received_frames = Arc::new(ParkingMutex::new(Vec::<frame::ParsedFrame>::new()));
        let first_audio_seen = Arc::new(tokio::sync::Notify::new());
        let shutdown_srv = Arc::new(tokio::sync::Notify::new());
        let received_srv = Arc::clone(&received_frames);
        let first_audio_srv = Arc::clone(&first_audio_seen);
        let shutdown_srv_task = Arc::clone(&shutdown_srv);

        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            loop {
                tokio::select! {
                    msg = ws.next() => match msg {
                        Some(Ok(Message::Binary(data))) => {
                            let parsed = frame::parse(&data).unwrap();
                            let is_audio = parsed.message_type == Some(MessageType::AudioOnlyRequest)
                                && !parsed.is_final();
                            received_srv.lock().push(parsed);
                            if is_audio {
                                first_audio_srv.notify_waiters();
                            }
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
            VolcengineStreamingASR::new(
                VolcengineCredentials {
                    service: VolcengineService::Standard,
                    auth_mode: VolcengineAuthMode::ApiKey,
                    app_id: String::new(),
                    access_token: "test-key".into(),
                    resource_id: VolcengineCredentials::default_resource_id().into(),
                },
                Vec::new(),
            )
            .with_endpoint(endpoint)
            .with_frame_send_timeout(Duration::from_millis(60)),
        );
        asr.open_session().await.unwrap();

        asr.consume_pcm_chunk(&vec![0x11u8; TARGET_AUDIO_CHUNK_BYTES]);
        tokio::time::timeout(Duration::from_secs(2), first_audio_seen.notified())
            .await
            .unwrap();
        assert_eq!(
            asr.audio_coverage(),
            (
                TARGET_AUDIO_CHUNK_BYTES as u64,
                TARGET_AUDIO_CHUNK_BYTES as u64
            )
        );
        assert_eq!(asr.state.lock().frames_written, 1);

        let writer_guard = asr.writer.lock().await;
        asr.consume_pcm_chunk(&vec![0x22u8; TARGET_AUDIO_CHUNK_BYTES + 200]);
        let err = asr
            .send_last_frame()
            .await
            .expect_err("must fail on write timeout");
        assert!(matches!(err, VolcengineASRError::ConnectionFailed(_)));
        drop(writer_guard);

        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            asr.audio_coverage(),
            (
                TARGET_AUDIO_CHUNK_BYTES as u64,
                (TARGET_AUDIO_CHUNK_BYTES * 2 + 200) as u64
            )
        );
        assert_eq!(asr.state.lock().frames_written, 1);
        assert!(!asr.is_stream_complete());
        assert!(
            received_frames.lock().iter().all(|f| !f.is_final()),
            "NegativeSequence must never be transmitted after failed audio write"
        );

        shutdown_srv.notify_waiters();
        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn buffered_audio_is_paced_and_precedes_negative_sequence() {
        let frames = Arc::new(ParkingMutex::new(
            Vec::<(Instant, frame::ParsedFrame)>::new(),
        ));
        let frames_srv = Arc::clone(&frames);
        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            while let Some(Ok(Message::Binary(data))) = ws.next().await {
                let parsed = frame::parse(&data).unwrap();
                let is_final = parsed.is_final();
                frames_srv.lock().push((Instant::now(), parsed));
                if is_final {
                    ws.send(Message::Binary(volc_final_response("火山完成").into()))
                        .await
                        .unwrap();
                    let _ = ws.close(None).await;
                    break;
                }
            }
        })
        .await;

        let asr = loopback_volcengine(endpoint);
        asr.open_session().await.unwrap();
        let payload = vec![0x33u8; TARGET_AUDIO_CHUNK_BYTES * 3 + 320];
        asr.consume_pcm_chunk(&payload);
        asr.send_last_frame().await.unwrap();

        let transcript = asr.await_final_result().await.unwrap();
        assert_eq!(transcript.text, "火山完成");
        assert!(asr.is_stream_complete());
        assert_eq!(
            asr.audio_coverage(),
            (payload.len() as u64, payload.len() as u64)
        );
        server_handle.await.unwrap();

        let recorded = frames.lock();
        // FullClientRequest + 3 chunks + 1 leftover + 1 NegativeSequence = 6 frames.
        assert_eq!(recorded.len(), 6);
        assert_eq!(
            recorded[0].1.message_type,
            Some(MessageType::FullClientRequest)
        );
        for i in 1..=4 {
            assert_eq!(
                recorded[i].1.message_type,
                Some(MessageType::AudioOnlyRequest)
            );
            assert!(!recorded[i].1.is_final());
        }
        assert!(recorded[5].1.is_final());
        // 3 full chunks = 3 * 100ms intervals at 2x realtime -> total span across frames 1..=4 >= 220ms.
        let pacing_span = recorded[4].0.saturating_duration_since(recorded[1].0);
        assert!(
            pacing_span >= Duration::from_millis(220),
            "expected ~300 ms 2x realtime pacing, got {pacing_span:?}"
        );
    }

    #[tokio::test]
    async fn cancel_wakes_paced_send_worker_promptly() {
        let frames = Arc::new(ParkingMutex::new(0usize));
        let first_audio = Arc::new(tokio::sync::Notify::new());
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let frames_srv = Arc::clone(&frames);
        let first_audio_srv = Arc::clone(&first_audio);
        let shutdown_srv = Arc::clone(&shutdown);

        let (endpoint, server_handle) = spawn_loopback_ws(move |mut ws| async move {
            loop {
                tokio::select! {
                    msg = ws.next() => match msg {
                        Some(Ok(Message::Binary(data))) => {
                            let parsed = frame::parse(&data).unwrap();
                            if parsed.message_type == Some(MessageType::AudioOnlyRequest) {
                                *frames_srv.lock() += 1;
                                first_audio_srv.notify_waiters();
                            }
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
        })
        .await;

        let asr = loopback_volcengine(endpoint);
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![0x55u8; TARGET_AUDIO_CHUNK_BYTES * 40]);
        tokio::time::timeout(Duration::from_secs(2), first_audio.notified())
            .await
            .unwrap();

        let started = Instant::now();
        asr.cancel();
        let res = tokio::time::timeout(Duration::from_millis(250), asr.await_final_result())
            .await
            .expect("cancel must wake await_final_result promptly");
        assert!(matches!(res, Err(VolcengineASRError::NoFinalResult)));
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(*frames.lock() <= 3);

        shutdown.notify_waiters();
        server_handle.await.unwrap();
    }

    #[tokio::test]
    async fn record_send_failure_preserves_partial_across_concurrent_cancel() {
        let asr = VolcengineStreamingASR::new(
            VolcengineCredentials {
                service: VolcengineService::Standard,
                auth_mode: VolcengineAuthMode::AppIdToken,
                app_id: "app".into(),
                access_token: "token".into(),
                resource_id: VolcengineCredentials::default_resource_id().into(),
            },
            Vec::new(),
        );
        let (tx, rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.is_connected = true;
            st.last_partial_text = "已识别前缀".into();
            st.final_tx = Some(tx);
        }
        *asr.final_rx.lock() = Some(rx);

        asr.record_send_failure(VolcengineASRError::ConnectionFailed("broken".into()));
        asr.cancel();

        let transcript = asr
            .await_final_result()
            .await
            .expect("partial transcript must survive concurrent cancel");
        assert_eq!(transcript.text, "已识别前缀");
        assert!(!asr.is_stream_complete());
    }
}
