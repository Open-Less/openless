#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Volcengine SAUC bigmodel streaming ASR client.
//!
//! Direct port of the Swift `VolcengineStreamingASR`. Battle-tested protocol
//! quirks are preserved verbatim — see comments tagged with `[asr]` for the
//! original learnings (especially the "definite=true is NOT stream end" bug).

use std::sync::atomic::{AtomicUsize, Ordering};
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
use uuid::Uuid;

use super::frame::{self, Flags, MessageType, Serialization};
use super::{AudioConsumer, DictionaryHotword, RawTranscript};

const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
/// 200 ms of 16 kHz / 16-bit / mono PCM.
const TARGET_AUDIO_CHUNK_BYTES: usize = 6_400;
/// 16 kHz · 16-bit · mono = 32 000 bytes/sec → 32 bytes/ms.
const BYTES_PER_MS: f64 = 32.0;
const HOTWORD_CAP: usize = 80;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);

/// 弱网下 TLS/WebSocket 握手可能一直挂到 OS 级 TCP 超时（几十秒），期间用户卡在
/// 「Starting」无法语音输入。协调器的全局超时只覆盖 `await_final_result`，**不**覆盖
/// `open_session`，所以这里必须自己给握手设上限：超时即快速失败并重试，而不是冻结。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 单次网络抖动（连接被重置 / 瞬时 DNS 失败）以前会直接让整次听写失败。重试几次让
/// 抖动可恢复。`AuthRejected`（凭据被拒）不在重试之列——重试也不会变好，只会拖慢报错。
const CONNECT_MAX_ATTEMPTS: usize = 3;
const CONNECT_RETRY_BACKOFF: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct VolcengineCredentials {
    pub app_id: String,
    pub access_token: String,
    pub resource_id: String,
}

impl VolcengineCredentials {
    pub fn default_resource_id() -> &'static str {
        "volc.seedasr.sauc.duration"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VolcengineASRError {
    #[error("credentials missing")]
    CredentialsMissing,
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    /// WebSocket 握手阶段服务端返回 401 / 403：凭据被拒。
    /// 区分自 `ConnectionFailed`（DNS/TLS/网络层失败）—— 前者通常是 App ID / Access
    /// Token / Resource ID 错或账号没开通 bigmodel；后者是网络断 / 防火墙 / DNS。
    /// 文案简短，原因在文档里说明，capsule 不堆长引导。
    #[error("凭据被拒（{0}）")]
    AuthRejected(u16),
    /// WebSocket 握手阶段服务端返回 429：请求过多 / 账号被限流。
    /// 单独归类而非落入 `ConnectionFailed` —— 后者会被 `connect_with_retry` 当网络抖动
    /// 立即重试 3 次，反而加剧限流、且文案含糊指向「网络失败」误导用户。此类与
    /// `AuthRejected` 一样**短路不重试**：立即回带明确文案，让用户知道是限流不是断网。
    #[error("请求过多，账号被限流（{0}）")]
    RateLimited(u16),
    #[error("no final result")]
    NoFinalResult,
    #[error("final result timed out")]
    FinalResultTimeout,
    #[error("decode failed: {0}")]
    DecodeFailed(String),
}

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

/// Sync state shared across the receive loop, the public API, and the
/// audio-consumer fast path.
#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    next_sequence: i32,
    bytes_sent: usize,
    frames_sent: usize,
    is_connected: bool,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, VolcengineASRError>>>,
    runtime: Option<Handle>,
    start: Option<Instant>,
    /// 最近一次 partial（非 final）的累积 transcript。服务端在 final 帧到达前
    /// 关闭连接 / 网络中断时，作为 fallback 回给上层，避免「用户的话已经识别出来
    /// 但没拿到 final」就丢光。
    last_partial_text: String,
}

pub struct VolcengineStreamingASR {
    credentials: VolcengineCredentials,
    hotwords: Vec<DictionaryHotword>,
    state: ParkingMutex<SyncState>,
    /// Guards the WebSocket write half so concurrent `send` calls serialize.
    /// Stored as Arc so spawned send tasks can hold their own clone — independent
    /// of the lifetime of any particular `&self` borrow.
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, VolcengineASRError>>>>,
    /// 单 worker 模式：consume_pcm_chunk 把 (seq, chunk) 入队这个 channel，
    /// open_session 里 spawn 出的唯一 worker 串行 recv + send_binary，
    /// 保证 seq 顺序严格等于实际发送顺序。session 结束时 take() 掉这个 sender，
    /// worker 的 recv() 返回 None 自动退出。
    audio_tx: ParkingMutex<Option<mpsc::UnboundedSender<(i32, Vec<u8>)>>>,
    /// 队列里 + worker 在飞的 audio 帧总数。consume +N，worker send 完一帧 -1。
    /// send_last_frame 必须等它降到 0 才能安全发末帧，否则末帧可能被服务端先收到
    /// 而把后续 chunk 当成「stream 已结束」之后的多余数据丢弃 → 尾句丢失。
    pending_sends: Arc<AtomicUsize>,
    send_done: Arc<Notify>,
}

impl VolcengineStreamingASR {
    pub fn new(credentials: VolcengineCredentials, hotwords: Vec<DictionaryHotword>) -> Self {
        Self {
            credentials,
            hotwords,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            audio_tx: ParkingMutex::new(None),
            pending_sends: Arc::new(AtomicUsize::new(0)),
            send_done: Arc::new(Notify::new()),
        }
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), VolcengineASRError> {
        if self.credentials.app_id.is_empty()
            || self.credentials.access_token.is_empty()
            || self.credentials.resource_id.is_empty()
        {
            return Err(VolcengineASRError::CredentialsMissing);
        }

        let connect_id = Uuid::new_v4().to_string();
        let ws = self.connect_with_retry(&connect_id).await?;
        let (write, read) = ws.split();

        let (tx, rx) = oneshot::channel();

        // Reset sync state for the new session.
        {
            let mut st = self.state.lock();
            st.pending_audio.clear();
            st.next_sequence = 1;
            st.bytes_sent = 0;
            st.frames_sent = 0;
            st.is_connected = true;
            st.final_tx = Some(tx);
            st.runtime = Some(Handle::current());
            st.start = Some(Instant::now());
            st.last_partial_text.clear();
        }
        self.pending_sends.store(0, Ordering::SeqCst);
        *self.final_rx.lock() = Some(rx);
        *self.writer.lock().await = Some(write);

        // 起一个唯一的 audio worker：consume_pcm_chunk 把 (seq, chunk) 推到 audio_tx，
        // worker 这边 FIFO recv 然后串行 send_binary。session 结束后调用方
        // (cancel / handle_frame error / fallback_to_partial_or_error) 会 take 掉
        // self.audio_tx，channel 关闭，worker 自然退出。
        let (audio_tx, mut audio_rx) = mpsc::unbounded_channel::<(i32, Vec<u8>)>();
        *self.audio_tx.lock() = Some(audio_tx);
        let writer_for_worker = Arc::clone(&self.writer);
        let pending_for_worker = Arc::clone(&self.pending_sends);
        let notify_for_worker = Arc::clone(&self.send_done);
        tokio::spawn(async move {
            while let Some((seq, chunk)) = audio_rx.recv().await {
                let frame = frame::build(
                    MessageType::AudioOnlyRequest,
                    Flags::PositiveSequence,
                    Serialization::None,
                    &chunk,
                    Some(seq),
                );
                if let Err(e) = send_binary(&writer_for_worker, frame).await {
                    log::error!("[asr] audio frame seq={} send 失败: {}", seq, e);
                }
                if pending_for_worker.fetch_sub(1, Ordering::SeqCst) == 1 {
                    notify_for_worker.notify_waiters();
                }
            }
        });

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
        send_binary(&self.writer, frame).await?;

        // Spawn the receive loop. Holds a Weak<Self> so it doesn't keep
        // the struct alive forever if callers drop their Arcs.
        let weak_self = Arc::downgrade(self);
        tokio::spawn(async move {
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
                        // 服务端没发 final 就关连接 → 用最近一次 partial 兜底，不丢已识别的文字。
                        this.fallback_to_partial_or_error(VolcengineASRError::NoFinalResult);
                        break;
                    }
                    Ok(_) => { /* ignore text/ping/pong */ }
                    Err(e) => {
                        log::error!("[asr] receive loop error: {}", e);
                        // 网络中断同样回退到 partial，让用户至少拿到已经识别的部分。
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
        });

        Ok(())
    }

    /// Build the WebSocket handshake request (endpoint + auth headers). Rebuilt
    /// per connect attempt because `connect_async` consumes the request and
    /// `http::Request` is not `Clone`.
    fn build_connect_request(
        &self,
        connect_id: &str,
    ) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, VolcengineASRError>
    {
        let mut request = ENDPOINT
            .into_client_request()
            .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?;
        let headers = request.headers_mut();
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
            let request = self.build_connect_request(connect_id)?;
            match tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request)).await {
                Ok(Ok((ws, _resp))) => return Ok(ws),
                Ok(Err(e)) => {
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
        // 等所有 fire-and-forget 发送完成。否则末帧（NegativeSequence）可能比尾部
        // chunk 先到服务端，被识别为「流已结束」之后再到的 chunk 全部丢弃 = 尾句吞掉。
        // 给一个 800ms 上限避免极端网络下永远等。
        let drain_deadline = Instant::now() + std::time::Duration::from_millis(800);
        while self.pending_sends.load(Ordering::SeqCst) > 0 {
            let remaining = drain_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                log::warn!(
                    "[asr] send_last_frame: pending {} 帧未发送完，超时强制继续",
                    self.pending_sends.load(Ordering::SeqCst)
                );
                break;
            }
            // notified() 返回 future，被 timeout 包住 → 等待发送完成或超时
            let _ = tokio::time::timeout(remaining, self.send_done.notified()).await;
        }

        // Drain leftover audio (if any) into one final positive-sequence frame.
        let leftover = {
            let mut st = self.state.lock();
            if st.pending_audio.is_empty() {
                None
            } else {
                Some(std::mem::take(&mut st.pending_audio))
            }
        };

        if let Some(buf) = leftover {
            let seq = self.allocate_positive_seq();
            let len = buf.len();
            let frame = frame::build(
                MessageType::AudioOnlyRequest,
                Flags::PositiveSequence,
                Serialization::None,
                &buf,
                Some(seq),
            );
            {
                let mut st = self.state.lock();
                st.bytes_sent += len;
                st.frames_sent += 1;
            }
            send_binary(&self.writer, frame).await?;
        }

        // Final frame: negativeSequence + negative seq number signals stream end.
        // 末帧用 negativeSequence + 负序号收尾，告诉服务端"流到此结束"。
        let final_seq = {
            let mut st = self.state.lock();
            let s = -st.next_sequence;
            st.next_sequence += 1;
            s
        };
        let frame = frame::build(
            MessageType::AudioOnlyRequest,
            Flags::NegativeSequence,
            Serialization::None,
            &[],
            Some(final_seq),
        );
        send_binary(&self.writer, frame).await?;

        let (total_bytes, total_frames) = {
            let st = self.state.lock();
            (st.bytes_sent, st.frames_sent)
        };
        let duration_ms = (total_bytes as f64 / BYTES_PER_MS) as u64;
        log::info!(
            "[asr] 发送总结：{} audio frames, {} bytes (~{} ms)",
            total_frames,
            total_bytes,
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
        let runtime = {
            let mut st = self.state.lock();
            st.is_connected = false;
            st.pending_audio.clear();
            st.runtime.clone()
        };
        // Drop audio sender → worker.recv() 返回 None → worker 退出，不再 hold writer。
        *self.audio_tx.lock() = None;
        if let Some(runtime) = runtime {
            // Close the writer asynchronously so the receive loop sees EOF.
            let writer = Arc::clone(&self.writer);
            runtime.spawn(async move {
                if let Some(mut w) = writer.lock().await.take() {
                    let _ = w.close().await;
                }
            });
        }
        self.signal_error(VolcengineASRError::NoFinalResult);
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
            let body = String::from_utf8_lossy(&parsed.payload).to_string();
            let code = parsed.error_code.unwrap_or(0);
            log::error!(
                "[asr] error frame code={} body={}",
                code,
                body.chars().take(200).collect::<String>()
            );
            self.signal_error(VolcengineASRError::ConnectionFailed(format!(
                "ASR error {}: {}",
                code, body
            )));
            self.state.lock().is_connected = false;
            *self.audio_tx.lock() = None;
            return false;
        }

        if parsed.message_type != Some(MessageType::FullServerResponse) {
            return true;
        }

        if let Ok(payload_str) = std::str::from_utf8(&parsed.payload) {
            log::info!(
                "[asr] server JSON: {}",
                payload_str.chars().take(400).collect::<String>()
            );
        }

        let json: Value = match serde_json::from_slice(&parsed.payload) {
            Ok(v) => v,
            Err(_) => return true,
        };
        let Some(result) = normalized_result(&json) else {
            return true;
        };

        // 流结束信号只信帧头 flags（lastPacket / negativeSequence）。
        // 之前误把 utterance.definite=true 当成流结束——但那只代表"这一段语音已固化"，
        // 用户可能还在继续说。结果一收到第一个 definite=true 就关掉接收，
        // 后面用户讲的内容全部丢失（实测丢了 9 秒）。
        let has_final = parsed.is_final();
        let mut full_text = result
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(utterances) = result.get("utterances").and_then(|v| v.as_array()) {
            // --- 声纹过滤：只保留主要说话人（说话时长最长的） ---
            // 1. 统计每个 speaker 的说话时长
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

            // 2. 找到说话时长最长的 speaker（即"主要说话人"）
            let primary_speaker: Option<String> = speaker_durations
                .iter()
                .max_by_key(|(_, &dur)| dur)
                .map(|(s, _)| s.clone());

            // 3. 只拼接主要说话人的文本；如果没有任何 speaker 标签，回退到全量拼接
            let pieces: Vec<&str> = if let Some(ref primary) = primary_speaker {
                utterances
                    .iter()
                    .filter(|u| {
                        u.get("speaker")
                            .and_then(|s| s.as_str())
                            .map(|s| s == primary.as_str())
                            .unwrap_or(true) // 无 speaker 字段的 utterance 保留
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

        // 缓存最新的 partial transcript：服务端在 final 帧前断连时 fallback 用。
        // 仅在非空且不是 final 时更新（final 走另一条路径）。
        if !has_final && !full_text.is_empty() {
            self.state.lock().last_partial_text = full_text.clone();
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
            self.state.lock().is_connected = false;
            *self.audio_tx.lock() = None;
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

    /// 服务端 close / 网络中断时调用：如果有缓存的 partial 文本，作为 transcript
    /// 兜底返回；否则才报错。配合 `last_partial_text` 实现「至少不丢用户已识别出的话」。
    fn fallback_to_partial_or_error(&self, err: VolcengineASRError) {
        let (partial, duration_ms) = {
            let st = self.state.lock();
            (
                st.last_partial_text.clone(),
                st.start
                    .map(|s| s.elapsed().as_millis() as u64)
                    .unwrap_or(0),
            )
        };
        if !partial.is_empty() {
            log::warn!(
                "[asr] {}; 使用 partial 兜底（{} 字）",
                err,
                partial.chars().count()
            );
            self.signal_success(RawTranscript {
                text: partial,
                duration_ms,
            });
        } else {
            self.signal_error(err);
        }
        self.state.lock().is_connected = false;
        *self.audio_tx.lock() = None;
    }
}

impl AudioConsumer for VolcengineStreamingASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        // 单 worker 串行 send 模式：在 state 锁内 drain 并分配 seq（seq 单调），
        // 然后把 (seq, chunk) push 进 mpsc。worker 端按入队顺序 send，
        // 哪怕跨多个 consume 调用、多个 spawn 也不会再有 writer 锁竞争。
        let chunks: Vec<(i32, Vec<u8>)> = {
            let mut st = self.state.lock();
            if !st.is_connected {
                return;
            }
            st.pending_audio.extend_from_slice(pcm);

            let mut out: Vec<(i32, Vec<u8>)> = Vec::new();
            while st.pending_audio.len() >= TARGET_AUDIO_CHUNK_BYTES {
                let chunk: Vec<u8> = st.pending_audio.drain(..TARGET_AUDIO_CHUNK_BYTES).collect();
                let seq = st.next_sequence;
                st.next_sequence += 1;
                st.bytes_sent += chunk.len();
                st.frames_sent += 1;
                out.push((seq, chunk));
            }
            out
        };

        if chunks.is_empty() {
            return;
        }
        let Some(tx) = self.audio_tx.lock().as_ref().cloned() else {
            return;
        };

        for entry in chunks {
            // pending_sends 必须在 tx.send 之前 +1：否则 worker 可能先 recv + 发送 +
            // 减 1，把 usize 计数器 underflow。
            self.pending_sends.fetch_add(1, Ordering::SeqCst);
            if tx.send(entry).is_err() {
                // worker 已退出（cancel / 错误路径里 audio_tx 被 take）。
                // 撤销刚才的 +1，避免 send_last_frame 的 wait 永远等不到 0。
                if self.pending_sends.fetch_sub(1, Ordering::SeqCst) == 1 {
                    self.send_done.notify_waiters();
                }
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
    sink.send(Message::Binary(data))
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

/// 把 tokio-tungstenite 的 connect 错误分类：握手收到 HTTP 401 / 403 → `AuthRejected`
/// （凭据被拒，要 user 检查 App ID / Access Token / 账号资源开通状态）；429 →
/// `RateLimited`（请求过多 / 限流，重试只会火上浇油，短路报明确文案）；其它 → 通用
/// `ConnectionFailed`（DNS / TLS / 网络层）。让 capsule 文案能跟泛泛 HTTP error 区分。
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

/// 握手错误是否「重试也无益」，`connect_with_retry` 据此短路。凭据被拒（401/403）
/// 与限流（429）都属此类：前者重试不会变对，后者重试只会加剧限流。其余（网络层）
/// 才值得在抖动时重试。
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

    /// 构造一个握手阶段返回给定 HTTP 状态码的 tungstenite 错误，用于分类测试。
    fn http_ws_error(status: u16) -> tokio_tungstenite::tungstenite::Error {
        use tokio_tungstenite::tungstenite::http::Response;
        let resp = Response::builder()
            .status(status)
            .body(None)
            .expect("build test http response");
        tokio_tungstenite::tungstenite::Error::Http(resp)
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
        // 通用网络失败仍应重试（抖动可恢复）——不能被误判成短路。
        assert!(!is_non_retryable(&VolcengineASRError::ConnectionFailed(
            "dns fail".into()
        )));
        assert!(!is_non_retryable(&VolcengineASRError::FinalResultTimeout));
    }

    #[test]
    fn classify_401_403_still_auth_rejected() {
        // 回归：新增 429 分类不影响既有 401 / 403 → AuthRejected。
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
        // 文案必须明确指向「限流/请求过多」，不是含糊的「网络失败」。
        let msg = VolcengineASRError::RateLimited(429).to_string();
        assert!(msg.contains("限流") || msg.contains("请求过多"), "文案: {msg}");
        assert!(!msg.contains("网络"), "限流文案不应误导为网络失败: {msg}");
    }

    #[tokio::test]
    async fn await_final_result_returns_error_when_final_frame_never_arrives() {
        let asr = VolcengineStreamingASR::new(
            VolcengineCredentials {
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
}
