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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use parking_lot::Mutex as ParkingMutex;
use serde_json::Value;
use sha1::Sha1;
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use super::{AudioConsumer, RawTranscript};

pub const PROVIDER_ID: &str = "tencent-cloud";
pub const DEFAULT_ENDPOINT: &str = "wss://asr.cloud.tencent.com/asr/v2";
pub const DEFAULT_MODEL: &str = "Hy-ASR-3.0-preview";
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 6_400;

const BYTES_PER_MS: u64 = 32;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(15);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

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
    #[error("连接失败: {0}")]
    ConnectionFailed(String),
    #[error("凭据被拒或服务未开通（{0}）")]
    AuthRejected(String),
    #[error("账户不可用（{0}）")]
    AccountUnavailable(String),
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
    bytes_sent: u64,
    started: bool,
    finished: bool,
    runtime: Option<Handle>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, TencentCloudASRError>>>,
    final_segments: BTreeMap<i64, String>,
    partial_segments: BTreeMap<i64, String>,
    last_result_text: String,
}

pub struct TencentCloudStreamingASR {
    credentials: TencentCloudCredentials,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, TencentCloudASRError>>>>,
    handshake_tx: ParkingMutex<Option<oneshot::Sender<Result<(), TencentCloudASRError>>>>,
    audio_tx: ParkingMutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    pending_sends: Arc<AtomicUsize>,
    send_done: Arc<Notify>,
}

impl TencentCloudStreamingASR {
    pub fn new(credentials: TencentCloudCredentials) -> Self {
        Self {
            credentials,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            handshake_tx: ParkingMutex::new(None),
            audio_tx: ParkingMutex::new(None),
            pending_sends: Arc::new(AtomicUsize::new(0)),
            send_done: Arc::new(Notify::new()),
        }
    }

    pub fn connect_url(&self) -> String {
        connect_url_at(
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
            .map_err(|error| TencentCloudASRError::ConnectionFailed(error.to_string()))?;
        let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| {
                TencentCloudASRError::ConnectionFailed(format!(
                    "连接超时（{} ms）",
                    CONNECT_TIMEOUT.as_millis()
                ))
            })?
            .map_err(|error| TencentCloudASRError::ConnectionFailed(error.to_string()))?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (handshake_tx, handshake_rx) = oneshot::channel();
        {
            let mut state = self.state.lock();
            *state = SyncState::default();
            state.runtime = Some(Handle::current());
            state.final_tx = Some(final_tx);
        }
        *self.final_rx.lock() = Some(final_rx);
        *self.audio_tx.lock() = Some(audio_tx);
        *self.handshake_tx.lock() = Some(handshake_tx);
        self.pending_sends.store(0, Ordering::SeqCst);

        let writer = Arc::clone(&self.writer);
        let pending_sends = Arc::clone(&self.pending_sends);
        let send_done = Arc::clone(&self.send_done);
        tokio::spawn(async move {
            while let Some(chunk) = audio_rx.recv().await {
                if let Err(error) = send_binary(&writer, chunk).await {
                    log::error!("[tencent-cloud-asr] audio frame send failed: {error}");
                }
                if pending_sends.fetch_sub(1, Ordering::SeqCst) == 1 {
                    send_done.notify_waiters();
                }
            }
        });

        let weak_self = Arc::downgrade(self);
        tokio::spawn(async move {
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
                    Err(error) => {
                        log::error!("[tencent-cloud-asr] receive loop error: {error}");
                        this.finish_with_partial_or_error(TencentCloudASRError::ConnectionFailed(
                            error.to_string(),
                        ));
                        break;
                    }
                }
            }
        });

        match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => {
                self.cancel();
                Err(error)
            }
            Ok(Err(_)) => {
                self.cancel();
                Err(TencentCloudASRError::ConnectionFailed(
                    "握手通道提前关闭".to_string(),
                ))
            }
            Err(_) => {
                self.cancel();
                Err(TencentCloudASRError::ConnectionFailed(format!(
                    "握手超时（{} ms）",
                    HANDSHAKE_TIMEOUT.as_millis()
                )))
            }
        }
    }

    pub async fn send_last_frame(&self) -> Result<(), TencentCloudASRError> {
        self.flush_pending_audio();
        self.wait_for_pending_sends().await;
        send_text(&self.writer, r#"{"type":"end"}"#).await
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
        let runtime = {
            let mut state = self.state.lock();
            state.pending_audio.clear();
            state.runtime.clone()
        };
        *self.handshake_tx.lock() = None;
        *self.audio_tx.lock() = None;
        if let Some(runtime) = runtime {
            let writer = Arc::clone(&self.writer);
            runtime.spawn(async move {
                let mut guard = writer.lock().await;
                if let Some(mut ws) = guard.take() {
                    let _ = ws.close().await;
                }
            });
        }
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

    async fn wait_for_pending_sends(&self) {
        let deadline = Instant::now() + Duration::from_millis(1_500);
        while self.pending_sends.load(Ordering::SeqCst) > 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                log::warn!(
                    "[tencent-cloud-asr] {} audio frames still pending at end",
                    self.pending_sends.load(Ordering::SeqCst)
                );
                break;
            }
            let _ = tokio::time::timeout(remaining, self.send_done.notified()).await;
        }
    }

    fn enqueue_audio(&self, chunk: Vec<u8>) {
        let Some(sender) = self.audio_tx.lock().as_ref().cloned() else {
            return;
        };
        self.pending_sends.fetch_add(1, Ordering::SeqCst);
        if sender.send(chunk).is_err() {
            if self.pending_sends.fetch_sub(1, Ordering::SeqCst) == 1 {
                self.send_done.notify_waiters();
            }
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
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            let error = classify_server_error(code, message);
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
            state.runtime = Some(Handle::current());
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
        if slice_type == 2 {
            state.final_segments.insert(index, text.to_string());
            state.partial_segments.remove(&index);
        } else {
            state.partial_segments.insert(index, text.to_string());
        }
    }

    fn finish_on_close(&self) {
        if let Some(sender) = self.handshake_tx.lock().take() {
            let _ = sender.send(Err(TencentCloudASRError::ConnectionFailed(
                "连接在握手完成前被关闭".to_string(),
            )));
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
        *self.audio_tx.lock() = None;
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
        *self.audio_tx.lock() = None;
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
        if let Some(runtime) = self.state.lock().runtime.clone() {
            runtime.spawn(async move {
                let mut guard = writer.lock().await;
                if let Some(mut ws) = guard.take() {
                    let _ = ws.close().await;
                }
            });
        }
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
        return Err(TencentCloudASRError::ConnectionFailed(
            "websocket not open".to_string(),
        ));
    };
    ws.send(Message::Binary(data))
        .await
        .map_err(|error| TencentCloudASRError::ConnectionFailed(error.to_string()))
}

async fn send_text(writer: &SharedWriter, data: &str) -> Result<(), TencentCloudASRError> {
    let mut guard = writer.lock().await;
    let Some(ws) = guard.as_mut() else {
        return Err(TencentCloudASRError::ConnectionFailed(
            "websocket not open".to_string(),
        ));
    };
    ws.send(Message::Text(data.to_string()))
        .await
        .map_err(|error| TencentCloudASRError::ConnectionFailed(error.to_string()))
}

fn connect_url_at(
    credentials: &TencentCloudCredentials,
    timestamp: i64,
    nonce: u32,
    voice_id: String,
) -> String {
    let endpoint = format!("{}/{}", DEFAULT_ENDPOINT, credentials.app_id.trim());
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

fn classify_server_error(code: i64, message: &str) -> TencentCloudASRError {
    let detail = format!("{code} {message}");
    match code {
        4002 | 4003 => TencentCloudASRError::AuthRejected(detail),
        4004 | 4005 => TencentCloudASRError::AccountUnavailable(detail),
        4006 => TencentCloudASRError::RateLimited(detail),
        _ => TencentCloudASRError::TaskFailed(detail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let url = connect_url_at(&credentials(), 1_700_000_000, 42, "voice-id".into());
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
    fn server_errors_are_classified_for_actionable_messages() {
        assert!(matches!(
            classify_server_error(4002, "auth failed"),
            TencentCloudASRError::AuthRejected(_)
        ));
        assert!(matches!(
            classify_server_error(4006, "concurrency exceeded"),
            TencentCloudASRError::RateLimited(_)
        ));
    }
}
