//! iFlytek（讯飞开放平台）实时语音转写（RTASR）流式客户端。
//!
//! 官方文档：https://www.xfyun.cn/doc/asr/rtasr/API.html
//!
//! 协议要点（标准版）：
//! - 端点：`wss://rtasr.xfyun.cn/v1/ws`，鉴权走查询参数
//!   `appid` + `ts` + `signa`，其中 `signa = Base64(HmacSHA1(MD5(appid + ts), apiKey))`；
//! - 音频：16 kHz / 16-bit / 单声道 PCM，与 OpenLess recorder 输出完全一致；
//! - 建议每 40ms 发送 1280 字节，发送过快可能触发引擎报错；
//! - 上传结束：发送二进制消息 `{"end": true}`；
//! - 结果：服务端以 text message 返回 `{"action":"result","data":"<json 字符串>"}`，
//!   `data.cn.st.type` 为 `0`（最终结果）/ `1`（中间结果），全部结果发完后服务端断开连接。
//!
//! 已知限制：标准版 RTASR 没有请求参数级热词（个性化热词只能在讯飞控制台上传）；
//! 方言/小语种需在控制台开通后通过 `lang` 参数指定，首期固定中文普通话（`cn`）。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use md5::{Digest as Md5Digest, Md5};
use parking_lot::Mutex as ParkingMutex;
use serde_json::Value;
use sha1::Sha1;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::config::{TaskSpawner, TokioTaskSpawner};

use super::{AudioConsumer, RawTranscript};
use crate::ports::{TextStreamChunk, TextStreamSink};

pub const PROVIDER_ID: &str = "iflytek";
pub const DEFAULT_ENDPOINT: &str = "wss://rtasr.xfyun.cn/v1/ws";
/// RTASR 文档建议：每 40ms 发送 1280 字节（16k/16-bit/mono = 32000 B/s）。
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 1_280;
/// 16 kHz · 16-bit · mono = 32 000 bytes/sec → 32 bytes/ms。
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
/// WebSocket 建连（TCP + TLS + HTTP upgrade）上限，避免弱网下握手挂死热键线程。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 握手阶段等待 `action=started` 的上限。鉴权失败（10105/10110）应在此窗口内快速失败，
/// 而不是把错误拖到收尾阶段才暴露。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// 默认语种：中文普通话。方言/小语种需在讯飞控制台开通后传对应 `lang` 参数。
const DEFAULT_LANG: &str = "cn";

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

#[derive(Clone, Debug)]
pub struct XfyunCredentials {
    /// 讯飞开放平台应用 ID。
    pub app_id: String,
    /// 实时语音转写服务对应的 APIKey（接口密钥）。
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
    /// 握手阶段服务端返回 10105 / 10110：AppID / APIKey 错误、IP 白名单未配置、
    /// 或账号未开通实时语音转写服务。
    #[error("凭据被拒或未开通服务（{0}）")]
    AuthRejected(String),
    /// 10800：超过授权连接数 / 并发受限。
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
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, XfyunASRError>>>,
    /// seg_id → 最终（type=0）分段文本。同一 seg_id 的后到结果覆盖前一个。
    final_segments: BTreeMap<i64, String>,
    /// seg_id → 最近一次中间（type=1）分段文本，服务端在 final 前断连时兜底用。
    partial_segments: BTreeMap<i64, String>,
    last_result_text: String,
}

pub struct XfyunStreamingASR {
    credentials: XfyunCredentials,
    task_spawner: Arc<dyn TaskSpawner>,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, XfyunASRError>>>>,
    /// 握手结果通道：receive loop 收到 `action=started` 后发 Ok，收到 error 发 Err。
    /// `open_session` 等待它以在鉴权失败时快速失败。
    handshake_tx: ParkingMutex<Option<oneshot::Sender<Result<(), XfyunASRError>>>>,
    /// 音频发送队列：consume_pcm_chunk 入队，唯一 worker 串行 send，保证时序。
    audio_tx: ParkingMutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    /// 队列里 + worker 在飞的 audio 帧总数。send_last_frame 必须等它归零再发
    /// `{"end": true}`，否则末帧先到、尾部音频被服务端当「end 之后的数据」丢弃。
    pending_sends: Arc<AtomicUsize>,
    send_done: Arc<Notify>,
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
            task_spawner,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            handshake_tx: ParkingMutex::new(None),
            audio_tx: ParkingMutex::new(None),
            pending_sends: Arc::new(AtomicUsize::new(0)),
            send_done: Arc::new(Notify::new()),
            partial_sink: ParkingMutex::new(None),
        }
    }

    pub fn set_partial_sink(&self, sink: Arc<dyn TextStreamSink>) {
        *self.partial_sink.lock() = Some(sink);
    }

    /// 构建带鉴权参数的 WebSocket 地址：
    /// `wss://rtasr.xfyun.cn/v1/ws?appid=..&ts=..&signa=..&lang=cn`。
    pub fn connect_url(&self) -> String {
        connect_url(&self.credentials)
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
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (handshake_tx, handshake_rx) = oneshot::channel();
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
        }
        *self.final_rx.lock() = Some(final_rx);
        *self.audio_tx.lock() = Some(audio_tx);
        *self.handshake_tx.lock() = Some(handshake_tx);
        self.pending_sends.store(0, Ordering::SeqCst);

        // 音频 worker：FIFO recv + 串行 send_binary，保证 chunk 顺序。
        let writer_for_worker = Arc::clone(&self.writer);
        let pending_for_worker = Arc::clone(&self.pending_sends);
        let notify_for_worker = Arc::clone(&self.send_done);
        let task_spawner = Arc::clone(&self.task_spawner);
        task_spawner.spawn(Box::pin(async move {
            while let Some(chunk) = audio_rx.recv().await {
                if let Err(e) = send_binary(&writer_for_worker, chunk).await {
                    log::error!("[xfyun-asr] audio frame send failed: {e}");
                }
                if pending_for_worker.fetch_sub(1, Ordering::SeqCst) == 1 {
                    notify_for_worker.notify_waiters();
                }
            }
        }));

        // receive loop：处理 started / result / error，以及服务端断开。
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
                        // 服务端在全部结果发完后主动断开；也覆盖 37005（15s 无音频）
                        // 等中断场景 —— finish_on_close 会按已有内容兜底。
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

        // 等待握手结果：鉴权错误 / 连接被拒在这里快速失败，不等用户说完话。
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
        // 等所有在途音频帧发完，再发 `{"end": true}`（上限 800ms 防极端网络下永远等）。
        let drain_deadline = Instant::now() + Duration::from_millis(800);
        while self.pending_sends.load(Ordering::SeqCst) > 0 {
            let remaining = drain_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                log::warn!(
                    "[xfyun-asr] send_last_frame: pending {} 帧未发送完，超时强制继续",
                    self.pending_sends.load(Ordering::SeqCst)
                );
                break;
            }
            let _ = tokio::time::timeout(remaining, self.send_done.notified()).await;
        }

        // 冲刷尾部不足一块的残余音频。
        let leftover = {
            let mut st = self.state.lock();
            if st.pending_audio.is_empty() {
                None
            } else {
                Some(std::mem::take(&mut st.pending_audio))
            }
        };
        if let Some(buf) = leftover {
            let len = buf.len() as u64;
            self.state.lock().bytes_sent += len;
            let Some(tx) = self.audio_tx.lock().as_ref().cloned() else {
                return Err(XfyunASRError::ConnectionFailed("websocket not open".into()));
            };
            self.pending_sends.fetch_add(1, Ordering::SeqCst);
            if tx.send(buf).is_err() {
                self.pending_sends.fetch_sub(1, Ordering::SeqCst);
            }
        }

        // 等尾部音频也发完，再发结束标识（内容与文档一致，必须走 binary message）。
        let drain_deadline = Instant::now() + Duration::from_millis(800);
        while self.pending_sends.load(Ordering::SeqCst) > 0 {
            let remaining = drain_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                log::warn!(
                    "[xfyun-asr] send_last_frame: tail {} 帧未发送完，超时强制结束",
                    self.pending_sends.load(Ordering::SeqCst)
                );
                break;
            }
            let _ = tokio::time::timeout(remaining, self.send_done.notified()).await;
        }

        send_binary(&self.writer, br#"{"end": true}"#.to_vec()).await
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
        self.state.lock().pending_audio.clear();
        // 释放握手通道：open_session 若仍在等 started，会立刻收到 Err 返回。
        *self.handshake_tx.lock() = None;
        // 关闭音频队列 → worker 的 recv() 返回 None → 退出，不再 hold writer。
        *self.audio_tx.lock() = None;
        let writer = Arc::clone(&self.writer);
        self.task_spawner.spawn(Box::pin(async move {
            let mut guard = writer.lock().await;
            if let Some(mut ws) = guard.take() {
                let _ = ws.close().await;
            }
        }));
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
                // 握手阶段就报错：把错误直接交给 open_session 的 handshake 等待方。
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
        // `type` 字段文档为字符串（"0" 最终 / "1" 中间）。防御性兼容数字形态
        // （0/1）：若服务端以数字返回而只认字符串，多句最终结果会被整体降级成
        // 中间结果，收尾 fallback 只剩最后一句 —— 前句丢失。
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

        let mut state = self.state.lock();
        state.last_result_text = trimmed.to_string();
        let mut delta = None;
        if is_final {
            // 最终结果：以 seg_id 去重覆盖，收尾按 seg_id 顺序拼接。
            state.final_segments.insert(seg_id, trimmed.to_string());
            state.partial_segments.remove(&seg_id);
        } else {
            let previous = state
                .partial_segments
                .get(&seg_id)
                .map(String::as_str)
                .unwrap_or("");
            delta = trimmed
                .strip_prefix(previous)
                .filter(|suffix| !suffix.is_empty())
                .map(str::to_string);
            state.partial_segments.insert(seg_id, trimmed.to_string());
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

    /// 服务端断开连接：正常路径（全部结果已发完）或异常中断。
    /// 握手尚未完成就被关闭时，优先把错误交给 open_session 的等待方。
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

    /// 有已识别内容（最终或中间结果）就兜底返回，否则报错 —— 与 Volcengine / Bailian
    /// 的「服务端在 final 前断连不丢已识别文字」策略保持一致。
    fn finish_with_partial_or_error(&self, error: XfyunASRError) {
        let has_partial = {
            let st = self.state.lock();
            !st.last_result_text.trim().is_empty() || !st.partial_segments.is_empty()
        };
        if has_partial {
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_success(&self) {
        let (tx, text, duration_ms) = {
            let mut st = self.state.lock();
            if st.finished {
                return;
            }
            st.finished = true;
            st.pending_audio.clear();
            let text = if st.final_segments.is_empty() {
                st.last_result_text.clone()
            } else {
                let segments: Vec<String> = st.final_segments.values().cloned().collect();
                super::mimo::join_transcript_chunks(&segments)
            };
            let duration_ms = if st.bytes_sent > 0 {
                st.bytes_sent / BYTES_PER_MS
            } else {
                st.start
                    .map(|start| start.elapsed().as_millis() as u64)
                    .unwrap_or_default()
            };
            (st.final_tx.take(), text, duration_ms)
        };
        *self.audio_tx.lock() = None;
        if let Some(tx) = tx {
            let _ = tx.send(Ok(RawTranscript { text, duration_ms }));
        }
        self.close_writer();
    }

    fn signal_error(&self, error: XfyunASRError) {
        let tx = {
            let mut st = self.state.lock();
            if st.finished {
                return;
            }
            st.finished = true;
            st.final_tx.take()
        };
        *self.audio_tx.lock() = None;
        if let Some(tx) = tx {
            let _ = tx.send(Err(error));
        }
    }

    fn finish_error(&self, error: XfyunASRError) {
        // 握手尚未完成就出错（如 receive loop 网络中断、服务端 error 消息）时，
        // 必须把错误交给 open_session 的 handshake 等待方 —— 否则它只能空等
        // HANDSHAKE_TIMEOUT（5s）才返回。已过握手的会话这里 take 到 None，幂等跳过。
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
        let chunks: Vec<Vec<u8>> = {
            let mut st = self.state.lock();
            if !st.started || st.finished {
                return;
            }
            st.pending_audio.extend_from_slice(pcm);
            let mut out = Vec::new();
            while st.pending_audio.len() >= TARGET_AUDIO_CHUNK_BYTES {
                let chunk: Vec<u8> = st.pending_audio.drain(..TARGET_AUDIO_CHUNK_BYTES).collect();
                st.bytes_sent += chunk.len() as u64;
                out.push(chunk);
            }
            out
        };
        if chunks.is_empty() {
            return;
        }
        let Some(tx) = self.audio_tx.lock().as_ref().cloned() else {
            return;
        };
        for chunk in chunks {
            // pending_sends 必须先 +1 再入队：否则 worker 可能先 recv + 发送 + 减 1，
            // 把 usize 计数器 underflow。
            self.pending_sends.fetch_add(1, Ordering::SeqCst);
            if tx.send(chunk).is_err() {
                if self.pending_sends.fetch_sub(1, Ordering::SeqCst) == 1 {
                    self.send_done.notify_waiters();
                }
                log::warn!("[xfyun-asr] audio queue closed; dropping subsequent frames");
                return;
            }
        }
    }
}

async fn send_binary(writer: &SharedWriter, data: Vec<u8>) -> Result<(), XfyunASRError> {
    let mut guard = writer.lock().await;
    let Some(ws) = guard.as_mut() else {
        return Err(XfyunASRError::ConnectionFailed(
            "websocket not open".to_string(),
        ));
    };
    ws.send(Message::Binary(data))
        .await
        .map_err(|e| XfyunASRError::ConnectionFailed(e.to_string()))
}

fn connect_url(credentials: &XfyunCredentials) -> String {
    let ts = chrono::Utc::now().timestamp().to_string();
    let signa = compute_signa(&credentials.app_id, &credentials.api_key, &ts);
    let mut url = url::Url::parse(DEFAULT_ENDPOINT).expect("static endpoint parses");
    url.query_pairs_mut()
        .append_pair("appid", credentials.app_id.trim())
        .append_pair("ts", &ts)
        .append_pair("signa", &signa)
        .append_pair("lang", DEFAULT_LANG);
    url.to_string()
}

/// `signa = Base64(HmacSHA1(MD5(appid + ts), apiKey))`，与讯飞开放平台文档公式一致。
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

/// 从 `cn.st` 节点提取全部词：`rt[].ws[].cw[]` 取第一个候选的 `w` 依次拼接。
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

/// 把讯飞错误码归类为对用户可读的类别：鉴权/授权问题与并发限制单独分类，
/// 其余归通用识别失败（避免 capsule 文案笼统指向「网络失败」）。
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
    fn signa_matches_official_documentation_example() {
        // 官方文档示例：appid=595f23df，ts=1512041814，apiKey=d9f4aa7ea6d94faca62cd88a28fd5234
        // → signa = IrrzsJeOFk1NGfJHW6SkHUoN9CU=
        let signa = compute_signa("595f23df", "d9f4aa7ea6d94faca62cd88a28fd5234", "1512041814");
        assert_eq!(signa, "IrrzsJeOFk1NGfJHW6SkHUoN9CU=");
    }

    #[test]
    fn md5_hex_matches_known_vector() {
        // MD5("595f23df1512041814") = 0829d4012497c14a30e7e72aeebe565e（文档示例）
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
        // 防御性：服务端若以数字 0/1 返回 type（而非文档字符串 "0"/"1"），
        // 最终结果仍要落 final_segments，不能整体降级成中间结果丢句。
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
        // 握手前出错（如网络中断）必须唤醒 open_session 的 handshake 等待方，
        // 否则其空等 HANDSHAKE_TIMEOUT。已过握手的会话 take 到 None，幂等。
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
        // 已 take：重复调用不 panic、不重复通知。
        asr.finish_error(XfyunASRError::ConnectionFailed("again".into()));
        assert!(asr.handshake_tx.lock().is_none());
    }
}
