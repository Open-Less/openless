//! 腾讯云实时语音识别 WebSocket 客户端。
//!
//! 官方文档：https://cloud.tencent.com/document/api/1093/48982
//!
//! 协议要点：
//! - 端点：`wss://asr.cloud.tencent.com/asr/v2/<appid>`；
//! - 鉴权：查询参数按字典序拼接后，以 SecretKey 做 HMAC-SHA1，再 Base64；
//! - 音频：16 kHz / 16-bit / 单声道 PCM，每 200ms 发送 6400 bytes；
//! - 收尾：发送文本消息 `{"type":"end"}`，等待 `final=1`；
//! - 默认模型：`Hy-ASR-3.0-preview`（腾讯云当前最新混元 ASR Preview）。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use parking_lot::Mutex as ParkingMutex;
use serde_json::Value;
use sha1::Sha1;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use crate::config::{TaskSpawner, TokioTaskSpawner};
use crate::ports::{TextStreamChunk, TextStreamSink};

use super::{AudioConsumer, RawTranscript};

pub const PROVIDER_ID: &str = "tencent-cloud";
pub const DEFAULT_ENDPOINT: &str = "wss://asr.cloud.tencent.com/asr/v2";
pub const DEFAULT_MODEL: &str = "Hy-ASR-3.0-preview";
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 6_400;

const BYTES_PER_MS: u64 = 32;
const MAX_AUDIO_SEND_RATE: u64 = 2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const FRAME_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(15);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

enum SendItem {
    Audio(Vec<u8>),
    End(oneshot::Sender<Result<(), TencentCloudASRError>>),
}

#[derive(Clone, Debug)]
pub struct TencentCloudCredentials {
    pub app_id: String,
    pub secret_id: String,
    pub secret_key: String,
    pub model: String,
}

impl TencentCloudCredentials {
    pub fn auth_ok(&self) -> bool {
        !self.app_id.trim().is_empty()
            && !self.secret_id.trim().is_empty()
            && !self.secret_key.trim().is_empty()
    }

    fn resolved_model(&self) -> &str {
        let model = self.model.trim();
        if model.is_empty() {
            DEFAULT_MODEL
        } else {
            model
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TencentCloudASRError {
    #[error("credentials missing")]
    CredentialsMissing,
    #[error("connection failed")]
    ConnectionFailed,
    #[error("credentials rejected or service unavailable ({0})")]
    AuthRejected(i64),
    #[error("account unavailable ({0})")]
    AccountUnavailable(i64),
    #[error("rate limited")]
    RateLimited,
    #[error("service unavailable ({0})")]
    ServiceUnavailable(i64),
    #[error("request failed ({0})")]
    TaskFailed(i64),
    #[error("no final result")]
    NoFinalResult,
    #[error("final result timed out")]
    FinalResultTimeout,
}

#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    bytes_sent: u64,
    started: bool,
    finished: bool,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, TencentCloudASRError>>>,
    final_segments: BTreeMap<i64, String>,
    partial_segments: BTreeMap<i64, String>,
    last_result_text: String,
}

pub struct TencentCloudStreamingASR {
    credentials: TencentCloudCredentials,
    endpoint: String,
    task_spawner: Arc<dyn TaskSpawner>,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, TencentCloudASRError>>>>,
    handshake_tx: ParkingMutex<Option<oneshot::Sender<Result<(), TencentCloudASRError>>>>,
    send_tx: ParkingMutex<Option<mpsc::UnboundedSender<SendItem>>>,
    partial_sink: ParkingMutex<Option<Arc<dyn TextStreamSink>>>,
}

impl TencentCloudStreamingASR {
    pub fn new(credentials: TencentCloudCredentials) -> Self {
        Self::with_task_spawner(credentials, Arc::new(TokioTaskSpawner))
    }

    pub fn with_task_spawner(
        credentials: TencentCloudCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Self {
        Self::with_endpoint(credentials, task_spawner, DEFAULT_ENDPOINT.to_string())
    }

    fn with_endpoint(
        credentials: TencentCloudCredentials,
        task_spawner: Arc<dyn TaskSpawner>,
        endpoint: String,
    ) -> Self {
        Self {
            credentials,
            endpoint,
            task_spawner,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            handshake_tx: ParkingMutex::new(None),
            send_tx: ParkingMutex::new(None),
            partial_sink: ParkingMutex::new(None),
        }
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.partial_sink.lock() = Some(sink);
    }

    pub fn connect_url(&self) -> String {
        connect_url_at(
            &self.endpoint,
            &self.credentials,
            chrono::Utc::now().timestamp(),
            random_nonce(),
            Uuid::new_v4().to_string(),
        )
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), TencentCloudASRError> {
        if !self.credentials.auth_ok() {
            return Err(TencentCloudASRError::CredentialsMissing);
        }

        let request = self
            .connect_url()
            .into_client_request()
            .map_err(|_| TencentCloudASRError::ConnectionFailed)?;
        let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| TencentCloudASRError::ConnectionFailed)?
            .map_err(|_| TencentCloudASRError::ConnectionFailed)?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        let (handshake_tx, handshake_rx) = oneshot::channel();
        {
            let mut state = self.state.lock();
            *state = SyncState::default();
            state.final_tx = Some(final_tx);
        }
        *self.final_rx.lock() = Some(final_rx);
        *self.send_tx.lock() = Some(send_tx);
        *self.handshake_tx.lock() = Some(handshake_tx);

        let writer = Arc::clone(&self.writer);
        let weak_self = Arc::downgrade(self);
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            let mut next_audio_send_at = None;
            while let Some(item) = send_rx.recv().await {
                let (result, done) = match item {
                    SendItem::Audio(chunk) => {
                        if let Some(send_at) = next_audio_send_at {
                            tokio::time::sleep_until(send_at).await;
                        }
                        let interval_ms =
                            (chunk.len() as u64 / BYTES_PER_MS / MAX_AUDIO_SEND_RATE).max(1);
                        let result = send_binary(&writer, chunk).await;
                        next_audio_send_at =
                            Some(tokio::time::Instant::now() + Duration::from_millis(interval_ms));
                        (result, None)
                    }
                    SendItem::End(done) => {
                        (send_text(&writer, r#"{"type":"end"}"#).await, Some(done))
                    }
                };
                if let Err(error) = &result {
                    log::error!("[tencent-cloud-asr] websocket send failed");
                    if let Some(this) = weak_self.upgrade() {
                        this.finish_error(error.clone());
                    }
                }
                if let Some(done) = done {
                    let _ = done.send(result.clone());
                }
                if result.is_err() {
                    break;
                }
            }
        }));

        let weak_self = Arc::downgrade(self);
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            let mut read = read;
            while let Some(message) = read.next().await {
                let Some(this) = weak_self.upgrade() else {
                    break;
                };
                match message {
                    Ok(Message::Text(text)) => {
                        if !this.handle_text_message(&text) {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        this.finish_on_close();
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        log::error!("[tencent-cloud-asr] receive loop failed");
                        this.finish_with_partial_or_error(TencentCloudASRError::ConnectionFailed);
                        break;
                    }
                }
            }
        }));

        match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => {
                self.cancel();
                Err(error)
            }
            Ok(Err(_)) => {
                self.cancel();
                Err(TencentCloudASRError::ConnectionFailed)
            }
            Err(_) => {
                self.cancel();
                Err(TencentCloudASRError::ConnectionFailed)
            }
        }
    }

    pub async fn send_last_frame(&self) -> Result<(), TencentCloudASRError> {
        self.flush_pending_audio();
        let (done_tx, done_rx) = oneshot::channel();
        let sender = self
            .send_tx
            .lock()
            .as_ref()
            .cloned()
            .ok_or(TencentCloudASRError::ConnectionFailed)?;
        sender
            .send(SendItem::End(done_tx))
            .map_err(|_| TencentCloudASRError::ConnectionFailed)?;
        done_rx
            .await
            .map_err(|_| TencentCloudASRError::ConnectionFailed)?
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, TencentCloudASRError> {
        self.await_final_result_with_timeout(FINAL_RESULT_TIMEOUT)
            .await
    }

    pub async fn await_final_result_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<RawTranscript, TencentCloudASRError> {
        let Some(receiver) = self.final_rx.lock().take() else {
            return Err(TencentCloudASRError::NoFinalResult);
        };
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(TencentCloudASRError::NoFinalResult),
            Err(_) => {
                log::error!(
                    "[tencent-cloud-asr] final result timed out after {} ms",
                    timeout.as_millis()
                );
                self.cancel();
                Err(TencentCloudASRError::FinalResultTimeout)
            }
        }
    }

    pub fn cancel(&self) {
        self.state.lock().pending_audio.clear();
        *self.handshake_tx.lock() = None;
        *self.send_tx.lock() = None;
        let writer = Arc::clone(&self.writer);
        self.task_spawner.spawn(Box::pin(async move {
            let mut guard = writer.lock().await;
            if let Some(mut ws) = guard.take() {
                let _ = ws.close().await;
            }
        }));
        self.signal_error(TencentCloudASRError::NoFinalResult);
    }

    fn flush_pending_audio(&self) {
        let leftover = {
            let mut state = self.state.lock();
            if state.pending_audio.is_empty() {
                return;
            }
            let leftover = std::mem::take(&mut state.pending_audio);
            state.bytes_sent += leftover.len() as u64;
            leftover
        };
        self.enqueue_audio(leftover);
    }

    fn enqueue_audio(&self, chunk: Vec<u8>) {
        let Some(sender) = self.send_tx.lock().as_ref().cloned() else {
            return;
        };
        if sender.send(SendItem::Audio(chunk)).is_err() {
            self.signal_error(TencentCloudASRError::ConnectionFailed);
        }
    }

    fn handle_text_message(&self, text: &str) -> bool {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                log::warn!("[tencent-cloud-asr] invalid json event: {error}");
                return true;
            }
        };
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            let error = classify_server_error(code);
            if let Some(sender) = self.handshake_tx.lock().take() {
                let _ = sender.send(Err(error.clone()));
            }
            self.finish_error(error);
            return false;
        }

        self.mark_started();
        if let Some(result) = value.get("result") {
            self.record_result(result);
        }
        if value.get("final").and_then(Value::as_i64) == Some(1) {
            self.finish_success();
            return false;
        }
        true
    }

    fn mark_started(&self) {
        let mut state = self.state.lock();
        if !state.started {
            state.started = true;
        }
        drop(state);
        if let Some(sender) = self.handshake_tx.lock().take() {
            let _ = sender.send(Ok(()));
        }
    }

    fn record_result(&self, result: &Value) {
        let Some(text) = result.get("voice_text_str").and_then(Value::as_str) else {
            return;
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let index = result.get("index").and_then(Value::as_i64).unwrap_or(0);
        let slice_type = result
            .get("slice_type")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        let mut state = self.state.lock();
        state.last_result_text = text.to_string();
        let mut delta = None;
        if slice_type == 2 {
            state.final_segments.insert(index, text.to_string());
            state.partial_segments.remove(&index);
        } else {
            let previous = state
                .partial_segments
                .get(&index)
                .map(String::as_str)
                .unwrap_or("");
            delta = text
                .strip_prefix(previous)
                .filter(|suffix| !suffix.is_empty())
                .map(str::to_string);
            state.partial_segments.insert(index, text.to_string());
        }
        drop(state);
        if let Some(delta) = delta {
            if let Some(sink) = self.partial_sink.lock().clone() {
                let _ = sink.publish(TextStreamChunk {
                    text: delta,
                    offset: 0,
                });
            }
        }
    }

    fn finish_on_close(&self) {
        if let Some(sender) = self.handshake_tx.lock().take() {
            let _ = sender.send(Err(TencentCloudASRError::ConnectionFailed));
            return;
        }
        self.finish_with_partial_or_error(TencentCloudASRError::NoFinalResult);
    }

    fn finish_with_partial_or_error(&self, error: TencentCloudASRError) {
        let has_text = {
            let state = self.state.lock();
            !state.last_result_text.trim().is_empty()
                || !state.final_segments.is_empty()
                || !state.partial_segments.is_empty()
        };
        if has_text {
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_success(&self) {
        let (sender, text, duration_ms) = {
            let mut state = self.state.lock();
            if state.finished {
                return;
            }
            state.finished = true;
            state.pending_audio.clear();
            let mut segments = state.final_segments.clone();
            for (index, text) in &state.partial_segments {
                segments.entry(*index).or_insert_with(|| text.clone());
            }
            let text = if segments.is_empty() {
                state.last_result_text.clone()
            } else {
                super::mimo::join_transcript_chunks(
                    &segments.into_values().collect::<Vec<String>>(),
                )
            };
            (state.final_tx.take(), text, state.bytes_sent / BYTES_PER_MS)
        };
        *self.send_tx.lock() = None;
        if let Some(sender) = sender {
            let _ = sender.send(Ok(RawTranscript { text, duration_ms }));
        }
        self.close_writer();
    }

    fn signal_error(&self, error: TencentCloudASRError) {
        let sender = {
            let mut state = self.state.lock();
            if state.finished {
                return;
            }
            state.finished = true;
            state.final_tx.take()
        };
        *self.send_tx.lock() = None;
        if let Some(sender) = sender {
            let _ = sender.send(Err(error));
        }
    }

    fn finish_error(&self, error: TencentCloudASRError) {
        if let Some(sender) = self.handshake_tx.lock().take() {
            let _ = sender.send(Err(error.clone()));
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

impl AudioConsumer for TencentCloudStreamingASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let chunks = {
            let mut state = self.state.lock();
            if !state.started || state.finished {
                return;
            }
            state.pending_audio.extend_from_slice(pcm);
            let mut chunks = Vec::new();
            while state.pending_audio.len() >= TARGET_AUDIO_CHUNK_BYTES {
                let chunk = state
                    .pending_audio
                    .drain(..TARGET_AUDIO_CHUNK_BYTES)
                    .collect::<Vec<u8>>();
                state.bytes_sent += chunk.len() as u64;
                chunks.push(chunk);
            }
            chunks
        };
        for chunk in chunks {
            self.enqueue_audio(chunk);
        }
    }
}

async fn send_binary(writer: &SharedWriter, data: Vec<u8>) -> Result<(), TencentCloudASRError> {
    let mut guard = writer.lock().await;
    let Some(ws) = guard.as_mut() else {
        return Err(TencentCloudASRError::ConnectionFailed);
    };
    tokio::time::timeout(FRAME_SEND_TIMEOUT, ws.send(Message::Binary(data)))
        .await
        .map_err(|_| TencentCloudASRError::ConnectionFailed)?
        .map_err(|_| TencentCloudASRError::ConnectionFailed)
}

async fn send_text(writer: &SharedWriter, data: &str) -> Result<(), TencentCloudASRError> {
    let mut guard = writer.lock().await;
    let Some(ws) = guard.as_mut() else {
        return Err(TencentCloudASRError::ConnectionFailed);
    };
    tokio::time::timeout(FRAME_SEND_TIMEOUT, ws.send(Message::Text(data.to_string())))
        .await
        .map_err(|_| TencentCloudASRError::ConnectionFailed)?
        .map_err(|_| TencentCloudASRError::ConnectionFailed)
}

fn connect_url_at(
    base_endpoint: &str,
    credentials: &TencentCloudCredentials,
    timestamp: i64,
    nonce: u32,
    voice_id: String,
) -> String {
    let endpoint = format!(
        "{}/{}",
        base_endpoint.trim_end_matches('/'),
        credentials.app_id.trim()
    );
    let parsed = url::Url::parse(&endpoint).expect("static Tencent Cloud endpoint parses");
    let host_and_path = format!(
        "{}{}",
        parsed.host_str().expect("Tencent Cloud endpoint has host"),
        parsed.path()
    );
    let params = BTreeMap::from([
        (
            "engine_model_type".to_string(),
            credentials.resolved_model().to_string(),
        ),
        ("expired".to_string(), (timestamp + 86_400).to_string()),
        ("filter_dirty".to_string(), "0".to_string()),
        ("filter_empty_result".to_string(), "1".to_string()),
        ("filter_modal".to_string(), "1".to_string()),
        ("filter_punc".to_string(), "0".to_string()),
        ("needvad".to_string(), "1".to_string()),
        ("nonce".to_string(), nonce.to_string()),
        (
            "secretid".to_string(),
            credentials.secret_id.trim().to_string(),
        ),
        ("timestamp".to_string(), timestamp.to_string()),
        ("voice_format".to_string(), "1".to_string()),
        ("voice_id".to_string(), voice_id),
    ]);
    let query = query_string(&params);
    let signing_text = format!("{host_and_path}?{query}");
    let signature = compute_signature(&signing_text, &credentials.secret_key);
    format!("{endpoint}?{query}&signature={}", url_encode(&signature))
}

fn query_string(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<String>>()
        .join("&")
}

fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub fn compute_signature(signing_text: &str, secret_key: &str) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret_key.trim().as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(signing_text.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

fn random_nonce() -> u32 {
    let bytes = Uuid::new_v4().into_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000_000 + 1
}

fn classify_server_error(code: i64) -> TencentCloudASRError {
    match code {
        4002 | 4003 => TencentCloudASRError::AuthRejected(code),
        4004 | 4005 => TencentCloudASRError::AccountUnavailable(code),
        4006 => TencentCloudASRError::RateLimited,
        4009 => TencentCloudASRError::ConnectionFailed,
        5000..=5002 => TencentCloudASRError::ServiceUnavailable(code),
        _ => TencentCloudASRError::TaskFailed(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct DelayedFirstSpawner {
        spawned: AtomicUsize,
        release: Arc<tokio::sync::Semaphore>,
    }

    impl TaskSpawner for DelayedFirstSpawner {
        fn spawn(&self, task: futures_util::future::BoxFuture<'static, ()>) {
            if self.spawned.fetch_add(1, Ordering::SeqCst) == 0 {
                let release = Arc::clone(&self.release);
                tokio::spawn(async move {
                    release.acquire_owned().await.unwrap().forget();
                    task.await;
                });
            } else {
                tokio::spawn(task);
            }
        }
    }

    fn credentials() -> TencentCloudCredentials {
        TencentCloudCredentials {
            app_id: "1259220000".into(),
            secret_id: "test-secret-id".into(),
            secret_key: "key".into(),
            model: DEFAULT_MODEL.into(),
        }
    }

    #[test]
    fn hmac_sha1_signature_matches_known_vector() {
        assert_eq!(
            compute_signature("The quick brown fox jumps over the lazy dog", "key"),
            "3nybhbi3iqa8ino29wqQcBydtNk="
        );
    }

    #[test]
    fn connect_url_uses_sorted_signed_query_and_encodes_signature() {
        let url = connect_url_at(
            DEFAULT_ENDPOINT,
            &credentials(),
            1_700_000_000,
            42,
            "voice-id".into(),
        );
        assert!(url.starts_with(
            "wss://asr.cloud.tencent.com/asr/v2/1259220000?engine_model_type=Hy-ASR-3.0-preview&expired=1700086400"
        ));
        assert!(url.contains("&nonce=42&secretid=test-secret-id&timestamp=1700000000"));
        assert!(url.contains("&voice_format=1&voice_id=voice-id&signature="));
        assert!(!url.ends_with('='));
    }

    #[test]
    fn stable_segments_replace_partials_and_keep_order() {
        let asr = TencentCloudStreamingASR::new(credentials());
        asr.record_result(&serde_json::json!({
            "slice_type": 1,
            "index": 1,
            "voice_text_str": "世界"
        }));
        asr.record_result(&serde_json::json!({
            "slice_type": 2,
            "index": 0,
            "voice_text_str": "你好"
        }));
        let state = asr.state.lock();
        assert_eq!(
            state.final_segments.get(&0).map(String::as_str),
            Some("你好")
        );
        assert_eq!(
            state.partial_segments.get(&1).map(String::as_str),
            Some("世界")
        );
    }

    #[test]
    fn server_errors_are_classified_for_actions() {
        assert!(matches!(
            classify_server_error(4002),
            TencentCloudASRError::AuthRejected(_)
        ));
        assert!(matches!(
            classify_server_error(4006),
            TencentCloudASRError::RateLimited
        ));
        assert!(matches!(
            classify_server_error(5000),
            TencentCloudASRError::ServiceUnavailable(5000)
        ));
    }

    #[test]
    fn auth_ok_requires_all_three_keys() {
        assert!(credentials().auth_ok());
        assert!(!TencentCloudCredentials {
            app_id: "app".into(),
            secret_id: "id".into(),
            secret_key: String::new(),
            model: DEFAULT_MODEL.into(),
        }
        .auth_ok());
    }

    #[tokio::test]
    async fn websocket_session_sends_all_audio_before_end_and_returns_final_text() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/asr/v2", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success" }).to_string(),
            ))
            .await
            .unwrap();

            let first = ws.next().await.unwrap().unwrap();
            let second = ws.next().await.unwrap().unwrap();
            let end = ws.next().await.unwrap().unwrap();
            assert!(matches!(first, Message::Binary(bytes) if bytes.len() == 6_400));
            assert!(matches!(second, Message::Binary(bytes) if bytes.len() == 100));
            assert_eq!(end, Message::Text(r#"{"type":"end"}"#.to_string()));

            ws.send(Message::Text(
                serde_json::json!({
                    "code": 0,
                    "message": "success",
                    "result": { "slice_type": 2, "index": 0, "voice_text_str": "你好" }
                })
                .to_string(),
            ))
            .await
            .unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success", "final": 1 }).to_string(),
            ))
            .await
            .unwrap();
        });
        let asr = Arc::new(TencentCloudStreamingASR::with_endpoint(
            credentials(),
            Arc::new(TokioTaskSpawner),
            endpoint,
        ));

        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; 6_500]);
        asr.send_last_frame().await.unwrap();
        let transcript = asr.await_final_result().await.unwrap();

        assert_eq!(transcript.text, "你好");
        assert_eq!(transcript.duration_ms, 203);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn end_waits_for_queued_audio_even_when_writer_start_is_delayed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/asr/v2", listener.local_addr().unwrap());
        let (first_tx, mut first_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success" }).to_string(),
            ))
            .await
            .unwrap();
            first_tx.send(ws.next().await.unwrap().unwrap()).unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Binary(bytes) if bytes.len() == 100
            ));
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(r#"{"type":"end"}"#.to_string())
            );
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success", "final": 1 }).to_string(),
            ))
            .await
            .unwrap();
        });
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let asr = Arc::new(TencentCloudStreamingASR::with_endpoint(
            credentials(),
            Arc::new(DelayedFirstSpawner {
                spawned: AtomicUsize::new(0),
                release: Arc::clone(&release),
            }),
            endpoint,
        ));
        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; 6_500]);
        let finishing = {
            let asr = Arc::clone(&asr);
            tokio::spawn(async move { asr.send_last_frame().await })
        };

        assert!(
            tokio::time::timeout(Duration::from_millis(1_800), &mut first_rx)
                .await
                .is_err(),
            "end must not bypass audio queued on a delayed writer"
        );
        release.add_permits(1);
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), &mut first_rx)
                .await
                .unwrap()
                .unwrap(),
            Message::Binary(bytes) if bytes.len() == 6_400
        ));
        finishing.await.unwrap().unwrap();
        assert!(asr.await_final_result().await.unwrap().text.is_empty());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn buffered_audio_replay_stays_within_tencent_rate_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/asr/v2", listener.local_addr().unwrap());
        let (elapsed_tx, elapsed_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success" }).to_string(),
            ))
            .await
            .unwrap();
            let mut received_at = Vec::new();
            for _ in 0..3 {
                assert!(matches!(
                    ws.next().await.unwrap().unwrap(),
                    Message::Binary(bytes) if bytes.len() == TARGET_AUDIO_CHUNK_BYTES
                ));
                received_at.push(tokio::time::Instant::now());
            }
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(r#"{"type":"end"}"#.to_string())
            );
            elapsed_tx
                .send(received_at[2].duration_since(received_at[0]))
                .unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success", "final": 1 }).to_string(),
            ))
            .await
            .unwrap();
        });
        let asr = Arc::new(TencentCloudStreamingASR::with_endpoint(
            credentials(),
            Arc::new(TokioTaskSpawner),
            endpoint,
        ));

        asr.open_session().await.unwrap();
        asr.consume_pcm_chunk(&vec![1; TARGET_AUDIO_CHUNK_BYTES * 3]);
        asr.send_last_frame().await.unwrap();
        asr.await_final_result().await.unwrap();

        assert!(
            elapsed_rx.await.unwrap() >= Duration::from_millis(180),
            "buffered audio must be paced instead of sent as one burst"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn websocket_rejection_does_not_expose_server_message() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/asr/v2", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({
                    "code": 4002,
                    "message": "SENSITIVE_SERVER_RESPONSE"
                })
                .to_string(),
            ))
            .await
            .unwrap();
        });
        let asr = Arc::new(TencentCloudStreamingASR::with_endpoint(
            credentials(),
            Arc::new(TokioTaskSpawner),
            endpoint,
        ));

        let error = asr.open_session().await.unwrap_err();

        assert!(matches!(error, TencentCloudASRError::AuthRejected(4002)));
        assert!(!error.to_string().contains("SENSITIVE_SERVER_RESPONSE"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn cancel_closes_the_socket_and_unblocks_waiters() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/asr/v2", listener.local_addr().unwrap());
        let (closed_tx, closed_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                serde_json::json!({ "code": 0, "message": "success" }).to_string(),
            ))
            .await
            .unwrap();
            let closed = matches!(
                ws.next().await,
                Some(Ok(Message::Close(_))) | None | Some(Err(_))
            );
            closed_tx.send(closed).unwrap();
        });
        let asr = Arc::new(TencentCloudStreamingASR::with_endpoint(
            credentials(),
            Arc::new(TokioTaskSpawner),
            endpoint,
        ));
        asr.open_session().await.unwrap();

        asr.cancel();

        assert!(tokio::time::timeout(Duration::from_secs(2), closed_rx)
            .await
            .unwrap()
            .unwrap());
        assert!(matches!(
            asr.send_last_frame().await,
            Err(TencentCloudASRError::ConnectionFailed)
        ));
        assert!(matches!(
            asr.await_final_result().await,
            Err(TencentCloudASRError::NoFinalResult)
        ));
        server.await.unwrap();
    }
}
