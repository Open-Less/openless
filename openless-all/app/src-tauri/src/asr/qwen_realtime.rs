//! Alibaba Cloud Bailian Qwen3-ASR-Flash realtime client.
//!
//! Speaks the OpenAI Realtime-style WebSocket protocol
//! (`/api-ws/v1/realtime?model=...`) — the protocol line `bailian.rs` left as
//! a follow-up. 与经典 `/api-ws/v1/inference` 不同：音频以 base64 JSON 事件
//! （`input_audio_buffer.append`）发送，服务端以 `server_vad` 自动断句，每句
//! 产生一个 `conversation.item.input_audio_transcription.completed`。
//!
//! 2026-07 线上实测确认的关键行为：
//! - `session.finish` 会先冲刷 VAD 尚未关闭的尾段（补发 completed）再回
//!   `session.finished`，说到一半松手不会丢尾巴；
//! - 纯静音 + finish 正常返回 `session.finished`（连接检查可用，无经典协议
//!   的 EmptyAudio 问题）；
//! - `session.update` 省略 `input_audio_transcription.language` 时自动检测语种；
//! - 业务空间专属域名（`wss://{WorkspaceId}.cn-beijing.maas.aliyuncs.com`）
//!   同样承载此路径，经典 inference 路径则只在公共网关。

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
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
use uuid::Uuid;

use super::{AudioConsumer, RawTranscript};

pub const PROVIDER_ID: &str = "bailian-qwen3-realtime";
pub const DEFAULT_ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime";
pub const DEFAULT_MODEL: &str = "qwen3-asr-flash-realtime";

/// 100 ms of 16 kHz / 16-bit / mono PCM，与 recorder 输出及官方示例一致。
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 3_200;
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
const SESSION_READY_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// WebSocket 建连（TCP + TLS + HTTP upgrade）本身的上限。没有它 `connect_async` 会无限
/// 等，而 `open_session` 是在串行的 hotkey bridge 线程上 `block_on` 等的 —— 卡住就意味着
/// 热键彻底失灵（开不了也停不了，只能退出重开）。详见 stepfun_realtime.rs 同名常量。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// server_vad 断句静默阈值。官方默认 400ms；取 500ms 降低说话中途换气被切断的概率。
const VAD_SILENCE_DURATION_MS: u32 = 500;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

#[derive(Clone, Debug)]
pub struct Qwen3RealtimeCredentials {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
}

impl Qwen3RealtimeCredentials {
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

    /// 连接 URL：`{endpoint}?model={model}`。用户若已在 endpoint 里带了
    /// `model=` 查询参数则原样使用，不重复拼接。
    pub fn connect_url(&self) -> String {
        let endpoint = self.normalized_endpoint();
        if endpoint.contains("model=") {
            return endpoint;
        }
        let sep = if endpoint.contains('?') { '&' } else { '?' };
        format!(
            "{}{}model={}",
            endpoint.trim_end_matches('/'),
            sep,
            self.normalized_model()
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Qwen3ASRError {
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
    Finish(oneshot::Sender<Result<(), Qwen3ASRError>>),
}

#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    audio_scratch: Vec<u8>,
    bytes_received: u64,
    /// 已经真正写入 WebSocket 的 PCM 字节数。恢复录音时旧音频会一次性进入发送队列，
    /// 需要用它估算队列还要按实时节奏排空多久，避免固定 12s 收尾超时提前中断。
    bytes_sent: u64,
    session_started: bool,
    session_finished: bool,
    session_start_error: Option<String>,
    runtime: Option<Handle>,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, Qwen3ASRError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    /// VAD 断句后按到达顺序累积的已完成句段（completed.transcript）。
    completed_segments: Vec<String>,
    /// 当前未完成句段的最新 interim 文本；completed 到达后清空。
    /// 服务端在句段开放期把累积文本放 `stash`、精修期放 `text`，取非空者。
    partial_text: String,
}

pub struct Qwen3RealtimeASR {
    credentials: Qwen3RealtimeCredentials,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, Qwen3ASRError>>>>,
    session_started: Arc<Notify>,
    session_finished: Arc<Notify>,
}

impl Qwen3RealtimeASR {
    pub fn new(credentials: Qwen3RealtimeCredentials) -> Self {
        Self {
            credentials,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            session_started: Arc::new(Notify::new()),
            session_finished: Arc::new(Notify::new()),
        }
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), Qwen3ASRError> {
        if self.credentials.api_key.trim().is_empty() {
            return Err(Qwen3ASRError::CredentialsMissing);
        }

        let url = self.credentials.connect_url();
        let mut request = url
            .into_client_request()
            .map_err(|e| Qwen3ASRError::ConnectionFailed(e.to_string()))?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.credentials.api_key.trim()))
                .map_err(|e| Qwen3ASRError::ConnectionFailed(e.to_string()))?,
        );
        request
            .headers_mut()
            .insert("OpenAI-Beta", HeaderValue::from_static("realtime=v1"));

        let (ws, _resp) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| {
                Qwen3ASRError::ConnectionFailed(format!(
                    "连接超时（{} ms）",
                    CONNECT_TIMEOUT.as_millis()
                ))
            })?
            .map_err(|e| Qwen3ASRError::ConnectionFailed(e.to_string()))?;
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
            // Qwen realtime 按实时流消费音频。正常录音时 recorder 本来就约每 100ms
            // 送来一帧；恢复录音则会把之前保存的 PCM 一次性灌进队列。若不节流，这批
            // 帧会在几毫秒内全部写进 WebSocket，服务端会把它当成超实时突发而漏掉旧段。
            let mut next_audio_send_at: Option<tokio::time::Instant> = None;
            while let Some(item) = send_rx.recv().await {
                match item {
                    SendItem::Audio(chunk) => {
                        let now = tokio::time::Instant::now();
                        let frame_started_at = match next_audio_send_at {
                            Some(deadline) if deadline > now => {
                                tokio::time::sleep_until(deadline).await;
                                deadline
                            }
                            _ => now,
                        };
                        if let Err(e) =
                            send_text(&writer_for_worker, append_audio_message(&chunk)).await
                        {
                            log::error!("[qwen3-asr] audio frame send failed: {e}");
                            if let Some(this) = weak_self_for_worker.upgrade() {
                                this.finish_error(e);
                            }
                            break;
                        }
                        if let Some(this) = weak_self_for_worker.upgrade() {
                            let mut st = this.state.lock();
                            st.bytes_sent = st.bytes_sent.saturating_add(chunk.len() as u64);
                        }
                        // 以上一帧的计划时刻为基准，避免把每次 WebSocket write 的耗时
                        // 累加进节拍；网络偶发卡顿时则从当前时刻重新起算，不追赶突发。
                        next_audio_send_at = Some(
                            frame_started_at + realtime_audio_duration(chunk.len() as u64),
                        );
                    }
                    SendItem::Finish(done) => {
                        let result = send_text(&writer_for_worker, finish_session_message())
                            .await
                            .map_err(|e| Qwen3ASRError::SendFailed(e.to_string()));
                        let _ = done.send(result);
                    }
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
                        this.fail_session_start(
                            "websocket closed before session configuration completed",
                        );
                        this.finish_with_partial_or_error(Qwen3ASRError::NoFinalResult);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::error!("[qwen3-asr] receive loop error: {e}");
                        this.fail_session_start(&e.to_string());
                        this.finish_with_partial_or_error(Qwen3ASRError::ConnectionFailed(
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
        if let Err(error) = send_text(&self.writer, session_update_message()).await {
            self.cancel();
            return Err(error);
        }
        let ready_result = if !self.state.lock().session_started {
            tokio::time::timeout(SESSION_READY_TIMEOUT, started)
                .await
                .map_err(|_| Qwen3ASRError::FinalResultTimeout)
        } else {
            Ok(())
        };
        if let Err(error) = ready_result {
            self.cancel();
            return Err(error);
        }
        if let Some(error) = self.state.lock().session_start_error.clone() {
            self.cancel();
            return Err(Qwen3ASRError::TaskFailed(error));
        }

        Ok(())
    }

    pub async fn send_last_frame(&self) -> Result<(), Qwen3ASRError> {
        let (finish_timeout, pending_audio_bytes) = {
            let st = self.state.lock();
            let pending = st.bytes_received.saturating_sub(st.bytes_sent);
            (final_result_timeout(pending), pending)
        };
        if pending_audio_bytes > TARGET_AUDIO_CHUNK_BYTES as u64 {
            log::info!(
                "[qwen3-asr] draining {pending_audio_bytes} queued audio bytes at realtime cadence before finish"
            );
        }
        let result = tokio::time::timeout(finish_timeout, async {
            let finished = self.session_finished.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();
            let (send_tx, tail_chunks) = {
                let mut st = self.state.lock();
                let send_tx = st.send_tx.clone();
                if !st.pending_audio.is_empty() {
                    let pending = std::mem::take(&mut st.pending_audio);
                    st.audio_scratch.extend_from_slice(&pending);
                }
                let tail = if st.audio_scratch.is_empty() {
                    Vec::new()
                } else {
                    vec![std::mem::take(&mut st.audio_scratch)]
                };
                (send_tx, tail)
            };
            let Some(send_tx) = send_tx else {
                return Ok(());
            };
            for chunk in tail_chunks {
                send_tx
                    .send(SendItem::Audio(chunk))
                    .map_err(|_| Qwen3ASRError::SendFailed("send worker closed".to_string()))?;
            }
            let (done_tx, done_rx) = oneshot::channel();
            send_tx
                .send(SendItem::Finish(done_tx))
                .map_err(|_| Qwen3ASRError::SendFailed("send worker closed".to_string()))?;
            done_rx
                .await
                .map_err(|_| Qwen3ASRError::SendFailed("finish ack dropped".to_string()))??;
            if !self.state.lock().session_finished {
                finished.await;
            }
            Ok(())
        })
        .await
        .map_err(|_| Qwen3ASRError::FinalResultTimeout)
        .and_then(|result| result);
        if result.is_err() {
            self.cancel();
        }
        result
    }

    pub async fn await_final_result(&self) -> Result<RawTranscript, Qwen3ASRError> {
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(Qwen3ASRError::NoFinalResult);
        };
        tokio::time::timeout(FINAL_RESULT_TIMEOUT, rx)
            .await
            .map_err(|_| Qwen3ASRError::FinalResultTimeout)?
            .map_err(|_| Qwen3ASRError::NoFinalResult)?
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
                log::warn!("[qwen3-asr] invalid json event: {e}");
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
            "conversation.item.input_audio_transcription.text" => {
                self.record_partial(&value);
                true
            }
            "conversation.item.input_audio_transcription.completed" => {
                self.record_completed(&value);
                true
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
                self.finish_error(Qwen3ASRError::TaskFailed(format!("{item_id}: {message}")));
                false
            }
            "session.finished" => {
                self.finish_success();
                false
            }
            "error" => {
                let message = value
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("realtime session error")
                    .to_string();
                self.finish_with_partial_or_error(Qwen3ASRError::TaskFailed(message));
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

    fn record_partial(&self, value: &Value) {
        // 句段开放期服务端把累积文本放 `stash`，随后的精修 pass 放 `text`；
        // 两者互斥出现，取非空者作为当前句段的 interim 文本。
        let text = value
            .get("text")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                value
                    .get("stash")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
            });
        if let Some(text) = text {
            self.state.lock().partial_text = text.trim().to_string();
        }
    }

    fn record_completed(&self, value: &Value) {
        let Some(transcript) = value.get("transcript").and_then(Value::as_str) else {
            return;
        };
        let trimmed = transcript.trim();
        let mut st = self.state.lock();
        if !trimmed.is_empty() {
            st.completed_segments.push(trimmed.to_string());
        }
        st.partial_text.clear();
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
            // session.finished 前若还有未 completed 的 interim 尾巴（理论上
            // finish 会冲刷出 completed，防御性兜底），拼在最后。
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

    fn finish_with_partial_or_error(&self, error: Qwen3ASRError) {
        let has_partial = {
            let st = self.state.lock();
            !st.completed_segments.is_empty() || !st.partial_text.trim().is_empty()
        };
        if has_partial {
            // 与 Bailian / Volcengine 保持一致：连接异常但已有结果时兜底返回。
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_error(&self, error: Qwen3ASRError) {
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

impl AudioConsumer for Qwen3RealtimeASR {
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

/// PCM 为 16kHz / 16-bit / mono，即每毫秒 32 字节。向上取整以免尾帧得到 0ms。
fn realtime_audio_duration(bytes: u64) -> Duration {
    Duration::from_millis(bytes.saturating_add(BYTES_PER_MS - 1) / BYTES_PER_MS)
}

/// 固定的服务端收尾窗口之外，为尚未发出的音频保留完整实时播放时长。
fn final_result_timeout(pending_audio_bytes: u64) -> Duration {
    FINAL_RESULT_TIMEOUT.saturating_add(realtime_audio_duration(pending_audio_bytes))
}

/// VAD 句段拼接：CJK 之间直接相连；拉丁词之间补空格，避免英文句段黏连。
/// `stepfun_realtime` 的多句段收尾复用同一套拼接逻辑，故 `pub(crate)`。
pub(crate) fn join_segments(segments: &[String]) -> String {
    let mut joined = String::new();
    for seg in segments.iter().map(|s| s.trim()) {
        if seg.is_empty() {
            continue;
        }
        if let (Some(prev), Some(next)) = (joined.chars().last(), seg.chars().next()) {
            if next.is_ascii_alphanumeric()
                && (prev.is_ascii_alphanumeric()
                    || matches!(prev, '.' | ',' | '?' | '!' | ':' | ';'))
            {
                joined.push(' ');
            }
        }
        joined.push_str(seg);
    }
    joined
}

fn session_update_message() -> String {
    // language 省略 => 服务端自动检测语种（2026-07 实测可用）。
    json!({
        "type": "session.update",
        "event_id": event_id(),
        "session": {
            "modalities": ["text"],
            "input_audio_format": "pcm",
            "sample_rate": 16000,
            "turn_detection": {
                "type": "server_vad",
                "silence_duration_ms": VAD_SILENCE_DURATION_MS,
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

fn finish_session_message() -> String {
    json!({ "type": "session.finish", "event_id": event_id() }).to_string()
}

fn event_id() -> String {
    format!("event_{}", Uuid::new_v4())
}

pub fn endpoint_scheme_is_secure_websocket(endpoint: &str) -> bool {
    url::Url::parse(endpoint.trim())
        .map(|url| url.scheme() == "wss")
        .unwrap_or(false)
}

async fn send_text(writer: &SharedWriter, text: String) -> Result<(), Qwen3ASRError> {
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut guard = writer.lock().await;
        let Some(ws) = guard.as_mut() else {
            return Err(Qwen3ASRError::ConnectionFailed(
                "websocket writer not available".to_string(),
            ));
        };
        ws.send(Message::Text(text))
            .await
            .map_err(|e| Qwen3ASRError::SendFailed(e.to_string()))
    })
    .await
    .map_err(|_| Qwen3ASRError::SendFailed("websocket write timed out".to_string()))?
}

async fn close_writer(writer: &SharedWriter) -> Result<(), Qwen3ASRError> {
    let mut guard = writer.lock().await;
    if let Some(mut ws) = guard.take() {
        let _ = ws.close().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_asr() -> Qwen3RealtimeASR {
        Qwen3RealtimeASR::new(Qwen3RealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: String::new(),
            model: String::new(),
        })
    }

    // ---- credentials / URL ----

    #[test]
    fn credentials_apply_default_endpoint_and_model() {
        let creds = Qwen3RealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: String::new(),
            model: String::new(),
        };
        assert_eq!(creds.normalized_endpoint(), DEFAULT_ENDPOINT);
        assert_eq!(creds.normalized_model(), DEFAULT_MODEL);
        assert_eq!(
            creds.connect_url(),
            "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime"
        );
    }

    #[test]
    fn connect_url_keeps_existing_model_query() {
        let creds = Qwen3RealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=custom".to_string(),
            model: "ignored".to_string(),
        };
        assert_eq!(
            creds.connect_url(),
            "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=custom"
        );
    }

    #[test]
    fn connect_url_supports_dedicated_workspace_domain() {
        let creds = Qwen3RealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: "wss://llm-xxx.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime/".to_string(),
            model: String::new(),
        };
        assert_eq!(
            creds.connect_url(),
            "wss://llm-xxx.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime"
        );
    }

    // ---- message builders ----

    #[test]
    fn session_update_uses_pcm_16k_and_server_vad() {
        let value: Value = serde_json::from_str(&session_update_message()).unwrap();
        assert_eq!(value["type"], "session.update");
        assert_eq!(value["session"]["input_audio_format"], "pcm");
        assert_eq!(value["session"]["sample_rate"], 16000);
        assert_eq!(value["session"]["turn_detection"]["type"], "server_vad");
        // language 省略走服务端自动检测
        assert!(value["session"]["input_audio_transcription"].is_null());
        assert!(value["event_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("event_")));
    }

    #[test]
    fn append_message_base64_encodes_audio() {
        let value: Value = serde_json::from_str(&append_audio_message(b"\x01\x02\x03")).unwrap();
        assert_eq!(value["type"], "input_audio_buffer.append");
        assert_eq!(value["audio"], "AQID");
        assert!(value["event_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("event_")));
    }

    #[test]
    fn finish_message_shape() {
        let value: Value = serde_json::from_str(&finish_session_message()).unwrap();
        assert_eq!(value["type"], "session.finish");
        assert!(value["event_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("event_")));
    }

    #[test]
    fn client_event_ids_are_unique() {
        let update: Value = serde_json::from_str(&session_update_message()).unwrap();
        let append: Value = serde_json::from_str(&append_audio_message(b"audio")).unwrap();
        let finish: Value = serde_json::from_str(&finish_session_message()).unwrap();
        assert_ne!(update["event_id"], append["event_id"]);
        assert_ne!(append["event_id"], finish["event_id"]);
    }

    // ---- event handling ----

    fn text_event(text: &str, stash: &str) -> Value {
        json!({
            "type": "conversation.item.input_audio_transcription.text",
            "text": text,
            "stash": stash,
        })
    }

    fn completed_event(transcript: &str) -> Value {
        json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "transcript": transcript,
        })
    }

    #[test]
    fn partial_prefers_text_falls_back_to_stash() {
        let asr = create_test_asr();
        // 句段开放期：text 为空、stash 有累积文本
        asr.record_partial(&text_event("", "今天"));
        assert_eq!(asr.state.lock().partial_text, "今天");
        // 精修期：text 有值优先
        asr.record_partial(&text_event("今天天气", "旧stash"));
        assert_eq!(asr.state.lock().partial_text, "今天天气");
        // 两者皆空不覆盖已有 partial
        asr.record_partial(&text_event("", ""));
        assert_eq!(asr.state.lock().partial_text, "今天天气");
    }

    #[test]
    fn completed_appends_segment_and_clears_partial() {
        let asr = create_test_asr();
        asr.record_partial(&text_event("", "第一句"));
        asr.handle_text_message(&completed_event("第一句话说完了。").to_string());
        asr.handle_text_message(&completed_event("第二句也说完了。").to_string());
        let st = asr.state.lock();
        assert_eq!(
            st.completed_segments,
            vec!["第一句话说完了。", "第二句也说完了。"]
        );
        assert!(st.partial_text.is_empty());
    }

    #[test]
    fn finish_success_joins_segments_with_trailing_partial() {
        let asr = create_test_asr();
        asr.record_completed(&completed_event("第一句。"));
        asr.record_partial(&text_event("", "没说完的尾巴"));

        let (tx, mut rx) = oneshot::channel();
        {
            let mut st = asr.state.lock();
            st.final_tx = Some(tx);
            st.bytes_received = 32_000;
        }
        asr.finish_success();
        let result = rx.try_recv().unwrap().unwrap();
        assert_eq!(result.text, "第一句。没说完的尾巴");
        assert_eq!(result.duration_ms, 1_000);
    }

    #[test]
    fn session_finished_event_stops_loop_with_result() {
        let asr = create_test_asr();
        asr.record_completed(&completed_event("你好。"));
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        let keep_going = asr.handle_text_message(&json!({"type": "session.finished"}).to_string());
        assert!(!keep_going);
        assert_eq!(rx.try_recv().unwrap().unwrap().text, "你好。");
    }

    #[test]
    fn error_event_with_partial_returns_partial() {
        let asr = create_test_asr();
        asr.record_completed(&completed_event("已识别内容。"));
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        let keep_going = asr.handle_text_message(
            &json!({"type": "error", "error": {"message": "boom"}}).to_string(),
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
        assert!(matches!(err, Qwen3ASRError::TaskFailed(m) if m == "boom"));
    }

    #[test]
    fn transcription_failure_returns_error_instead_of_silent_success() {
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        let keep_going = asr.handle_text_message(
            &json!({
                "type": "conversation.item.input_audio_transcription.failed",
                "item_id": "item_123",
                "error": { "message": "transcription rejected" },
            })
            .to_string(),
        );
        assert!(!keep_going);
        let err = rx.try_recv().unwrap().unwrap_err();
        assert!(
            matches!(err, Qwen3ASRError::TaskFailed(message) if message == "item_123: transcription rejected")
        );
    }

    #[test]
    fn append_write_failure_returns_error_instead_of_partial_success() {
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        asr.finish_error(Qwen3ASRError::SendFailed("websocket write timed out".to_string()));
        let err = rx.try_recv().unwrap().unwrap_err();
        assert!(matches!(err, Qwen3ASRError::SendFailed(message) if message == "websocket write timed out"));
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
    fn empty_session_finishes_with_empty_text() {
        // 连接检查场景：纯静音无任何 completed，finish 后应返回空文本成功。
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        asr.handle_text_message(&json!({"type": "session.finished"}).to_string());
        assert_eq!(rx.try_recv().unwrap().unwrap().text, "");
    }

    // ---- join_segments ----

    #[test]
    fn join_segments_cjk_concatenates_directly() {
        let segments = vec!["第一句。".to_string(), "第二句。".to_string()];
        assert_eq!(join_segments(&segments), "第一句。第二句。");
    }

    #[test]
    fn join_segments_latin_inserts_space() {
        let segments = vec!["hello".to_string(), "world".to_string()];
        assert_eq!(join_segments(&segments), "hello world");
    }

    #[test]
    fn join_segments_latin_punctuation_inserts_space() {
        for punctuation in ['.', ',', '?', '!'] {
            let segments = vec![format!("Hello{punctuation}"), "World".to_string()];
            assert_eq!(
                join_segments(&segments),
                format!("Hello{punctuation} World")
            );
        }
    }

    #[test]
    fn qwen3_endpoint_requires_wss() {
        assert!(endpoint_scheme_is_secure_websocket(DEFAULT_ENDPOINT));
        assert!(!endpoint_scheme_is_secure_websocket(
            "ws://localhost:9000/realtime"
        ));
        assert!(!endpoint_scheme_is_secure_websocket(
            "https://api.example.com/realtime"
        ));
    }

    #[test]
    fn join_segments_skips_empty() {
        let segments = vec!["".to_string(), "内容".to_string(), "  ".to_string()];
        assert_eq!(join_segments(&segments), "内容");
    }

    // ---- audio buffering ----

    #[test]
    fn audio_buffered_before_session_created() {
        let asr = create_test_asr();
        asr.consume_pcm_chunk(&[0u8; 100]);
        let st = asr.state.lock();
        assert_eq!(st.pending_audio.len(), 100);
        assert_eq!(st.bytes_received, 100);
    }

    #[test]
    fn drain_audio_chunks_keeps_tail_buffered() {
        let mut buffer = vec![1u8; TARGET_AUDIO_CHUNK_BYTES * 2 + 17];
        let chunks = drain_audio_chunks(&mut buffer);
        assert_eq!(chunks.len(), 2);
        assert_eq!(buffer.len(), 17);
    }

    #[test]
    fn realtime_pacing_uses_pcm_duration() {
        assert_eq!(
            realtime_audio_duration(TARGET_AUDIO_CHUNK_BYTES as u64),
            Duration::from_millis(100)
        );
        assert_eq!(realtime_audio_duration(1), Duration::from_millis(1));
    }

    #[test]
    fn finish_timeout_includes_queued_replay_duration() {
        assert_eq!(
            final_result_timeout(64_000),
            FINAL_RESULT_TIMEOUT + Duration::from_secs(2)
        );
    }
}
