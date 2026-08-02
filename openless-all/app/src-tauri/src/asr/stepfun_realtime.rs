//! 阶跃星辰 StepAudio 实时 ASR 客户端（`wss://api.stepfun.com/v1/realtime/asr/stream`）。
//!
//! 与 `qwen_realtime.rs` 同为 OpenAI Realtime 风格 WS，但四处关键差异
//! （2026-07-16 真实接口逐项实测确认）：
//!
//! - **模型在 `session.update` 里传**（`session.audio.input.transcription.model`），
//!   不是 URL query；session 配置是 `audio.input.{format,transcription,turn_detection}`
//!   的嵌套形状。
//! - **`delta` 的 `text`/`stash` 是拼接关系**：`text` 是已确定前缀、`stash` 是
//!   未定尾巴，当前句段全文 = `text + stash`（Qwen 是两者互斥取非空）。
//! - **没有服务端结束事件**：`session.finish` 回 `transcript.response.error`（不支持）；
//!   server_vad 模式下 `input_audio_buffer.commit` 被静默忽略。唯一可靠的收尾是
//!   **补送 ≥silence_duration_ms 的静音帧逼 VAD 关段**——speech_stopped 后 ~0.4s
//!   吐出该句段的 `completed`，随后客户端自行断开。
//! - `prompt` 字段在 transcription 配置里被接受（批式 /audio/transcriptions 则
//!   静默忽略 prompt、只认 hotwords——两条通道词汇偏置方式相反）。

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

use super::qwen_realtime::join_segments;
use super::{AudioConsumer, RawTranscript};

/// 内部 effective id（`resolve_effective_asr_provider` 按模型名从 `stepfun`
/// 路由到这里），不出现在设置页 preset 列表里。
pub const PROVIDER_ID: &str = "stepfun-realtime";
pub const DEFAULT_ENDPOINT: &str = "wss://api.stepfun.com/v1/realtime/asr/stream";
pub const DEFAULT_MODEL: &str = "stepaudio-2.5-asr-stream";
/// 实时 WS 在 base URL 下的固定路径（从批式共用的 https base 派生 wss URL 用）。
const REALTIME_PATH: &str = "/realtime/asr/stream";

/// 100 ms of 16 kHz / 16-bit / mono PCM，与 recorder 输出一致。
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 3_200;
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);
const SESSION_READY_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// WebSocket 建连（TCP + TLS + HTTP upgrade）本身的上限。
///
/// 没有它，`connect_async` 会无限等：握手打到一半断网、公司网关 / 酒店门户静默丢包、
/// 服务端黑洞掉连接，这个 await 就永远不返回。而 `open_session` 是在 hotkey bridge
/// 线程上 `block_on` 等的（那条 bridge 为修 #468/#475 的 latch 竞态改成了串行），
/// 一旦卡住，按下 / 松开全都排在队里没人处理 —— 用户的表现是「热键突然完全失灵，
/// 录音开不了也停不了，只能退出重开」，而且没有任何提示。
///
/// 与 SESSION_READY_TIMEOUT 的区别：那个管的是「连上之后等 session.updated 回应」，
/// 这个管的是「连上」本身，之前完全没人管。取值与 volcengine 侧一致。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// server_vad 断句静默阈值，与 qwen_realtime 取齐（500ms 降低换气误切概率）。
const VAD_SILENCE_DURATION_MS: u32 = 500;
/// 收尾补送的静音时长：必须 > VAD_SILENCE_DURATION_MS 并留网络余量，
/// 否则 VAD 不关段、最后一句永远等不到 completed（协议无 finish 事件）。
const SILENCE_TAIL_MS: u64 = 700;
/// 静音送出后等待「无未关句段」的宽限期：纯静音会话（连接检查、误触）没有任何
/// speech_started，宽限到点即以空文本成功返回。
const FINISH_GRACE: Duration = Duration::from_millis(1_200);
/// 宽限期内的复查间隔。收尾判据必须**反复**查——只查一次的话，那一次不通过就
/// 再没有第二次机会（详见 `send_last_frame` 里的宽限任务）。
const FINISH_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 自尾帧写出起算的收尾硬上限。服务端始终不关最后一个句段时的兜底，把最坏等待
/// 从 FINAL_RESULT_TIMEOUT（12s）压到这里。
const FINISH_HARD_DEADLINE: Duration = Duration::from_millis(3_000);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

#[derive(Clone, Debug)]
pub struct StepfunRealtimeCredentials {
    pub api_key: String,
    /// 允许三种形态：空（默认网关）、批式共用的 `https://api.stepfun.com/v1`
    /// （自动派生 wss 路径）、完整 `wss://` URL（原样使用）。
    pub endpoint: String,
    pub model: String,
    /// 用户词典拼成的 prompt（实时协议接受 transcription.prompt；批式则相反，
    /// 只认 hotwords）。None = 不发。
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

    /// 连接 URL：`wss://` 开头原样用；`http(s)://` base（与批式共用的凭据槽）
    /// 换 scheme 并补 `/realtime/asr/stream` 路径；解析失败/空值回默认网关。
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
        /// `send_last_frame` 用它确认尾帧已由写 worker 实际写入 WebSocket。
        written_tx: Option<oneshot::Sender<Result<(), String>>>,
    },
}

#[derive(Default)]
struct SyncState {
    pending_audio: Vec<u8>,
    audio_scratch: Vec<u8>,
    bytes_received: u64,
    /// 最近一次非静音 PCM 由写 worker 成功写入 WebSocket 的时刻。
    last_non_silent_audio_written_at: Option<Instant>,
    /// 最近一次服务端 completed 事件到达的时刻。
    last_completed_at: Option<Instant>,
    session_started: bool,
    session_finished: bool,
    session_start_error: Option<String>,
    runtime: Option<Handle>,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, StepfunASRError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    /// VAD 断句后按到达顺序累积的已完成句段（completed.transcript）。
    completed_segments: Vec<String>,
    /// 当前开放句段的 interim 全文 = delta.text（已确定前缀）+ delta.stash
    /// （未定尾巴）；completed 到达后清空。
    partial_text: String,
    /// speech_started 开、completed 关的未收尾句段计数。收尾判据：finishing
    /// 且归零（静音尾帧已逼 VAD 关掉所有段）。
    open_segments: u32,
    /// send_last_frame 已冲刷尾音频 + 静音帧，进入等待句段归零阶段。
    finishing: bool,
    /// 尾帧尚未由写 worker 确认写入 WebSocket，不能被旧句段的 completed 抢先收尾。
    tail_write_pending: bool,
}

pub struct StepfunRealtimeASR {
    credentials: StepfunRealtimeCredentials,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, StepfunASRError>>>>,
    session_started: Arc<Notify>,
    session_finished: Arc<Notify>,
}

impl StepfunRealtimeASR {
    pub fn new(credentials: StepfunRealtimeCredentials) -> Self {
        Self {
            credentials,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            session_started: Arc::new(Notify::new()),
            session_finished: Arc::new(Notify::new()),
        }
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
            st.runtime = Some(Handle::current());
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
            st.send_tx = Some(send_tx);
        }
        *self.final_rx.lock() = Some(final_rx);

        let writer_for_worker = Arc::clone(&self.writer);
        let weak_self_for_worker = Arc::downgrade(self);
        tokio::spawn(async move {
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
        });

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

    /// 冲刷尾音频 + 补送静音帧逼 VAD 关掉所有开放句段，等全部 completed 到齐。
    ///
    /// 协议没有 finish 事件（见模块注释），完成判据由客户端状态机给出：
    /// `finishing && open_segments == 0`。纯静音会话（无任何 speech_started）
    /// 由 FINISH_GRACE 宽限兜底，以空文本成功返回。
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
                // 只有**真实录音**部分算「有声」。补的静音是收尾工具，若把它一起
                // 算进去，`last_non_silent_audio_written_at` 会被推到最后一个
                // completed 之后，`has_audio_after_last_completed` 从此恒为真。
                let contains_non_silent_audio = contains_non_silent_pcm(&tail);
                // 尾音频和静音帧合并成一次 append，减少一次写。
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

            // 宽限任务：尾帧已写出，客户端不再发任何音频，剩下的只是等服务端把尾帧
            // 里的音频吐成 completed。等够 FINISH_GRACE 且没有未关句段即可收尾。
            //
            // 必须**轮询**：此处曾经只在 FINISH_GRACE 到点查一次，那一次不通过就再
            // 没有第二次机会——而尾帧之后服务端可能一个事件都不再发（松手前已停顿
            // ≥VAD 阈值时，最后一段的 completed 早在尾帧之前就到了），会话于是一路
            // 干等到 FINAL_RESULT_TIMEOUT。实测约 13% 的听写因此白等 12 秒。
            // （那 13% 的直接成因是 `contains_non_silent_pcm` 把底噪当语音，见该函数；
            // 轮询 + 硬上限是第二道防线，保证任何收尾失败都不会再退化成 12 秒。）
            //
            // 收尾前提仍是「无未关句段、且最后一个 completed 之后没有未确认的真实
            // 音频」——那条保护是对的，不能为了修卡顿丢掉。真正的兜底是
            // FINISH_HARD_DEADLINE：服务端始终不关最后一段时，把最坏等待压到 3s，
            // 而不是一路耗到 FINAL_RESULT_TIMEOUT。
            let weak = Arc::downgrade(self);
            tokio::spawn(async move {
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
            });

            if !self.state.lock().session_finished {
                finished.await;
            }
            Ok(())
        })
        .await;
        match result {
            Ok(inner) => inner,
            Err(_) => {
                // 超时兜底：有部分结果就带出去，没有才报错——与断连路径一致。
                // 走到这里说明上面的宽限任务没能收尾（它自己有 FINISH_HARD_DEADLINE
                // 兜底，正常不该到这一步），而用户已经白等了整整 12 秒——必须留痕。
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
            // `transcript.response.error` 是 StepFun 特有的请求级错误事件
            // （实测发不支持的 session.finish 时返回），与通用 `error` 同处理。
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
        // `text` 是已确定前缀、`stash` 是未定尾巴，二者拼接即当前句段全文
        // （与 Qwen 的互斥语义不同，见模块注释）。两者皆空不覆盖已有 partial。
        let confirmed = value.get("text").and_then(Value::as_str).unwrap_or("");
        let stash = value.get("stash").and_then(Value::as_str).unwrap_or("");
        let combined = format!("{confirmed}{stash}");
        let combined = combined.trim();
        if !combined.is_empty() {
            self.state.lock().partial_text = combined.to_string();
        }
    }

    /// 返回 false 表示会话已收尾、读循环可退出。
    fn record_completed(&self, value: &Value) -> bool {
        let transcript = value
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or("");
        let should_finish = {
            let mut st = self.state.lock();
            let trimmed = transcript.trim();
            if !trimmed.is_empty() {
                st.completed_segments.push(trimmed.to_string());
            }
            st.partial_text.clear();
            st.open_segments = st.open_segments.saturating_sub(1);
            st.last_completed_at = Some(Instant::now());
            st.finishing
                && !st.tail_write_pending
                && !has_audio_after_last_completed(&st)
                && st.open_segments == 0
        };
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
            // 收尾时若还有未 completed 的 interim 尾巴（静音帧理应冲出 completed，
            // 防御性兜底），拼在最后。
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
            // 与 Bailian / Qwen / Volcengine 一致：异常但已有结果时兜底返回。
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
        if let Some(handle) = self.state.lock().runtime.clone() {
            handle.spawn(async move {
                let _ = close_writer(&writer).await;
            });
        }
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

/// 采样振幅超过满量程 ~1% 才算「有声」。
const NON_SILENT_PEAK: i16 = 328;
/// 至少这么多采样超阈值才判定这一帧含真实语音。单点尖峰（键盘、电流噪声）不足以
/// 把收尾判据拉住。
const NON_SILENT_MIN_SAMPLES: usize = 8;

/// 这段 s16le PCM 里是否有真实语音。
///
/// 曾经写成 `any(|byte| byte != 0)`——但麦克风底噪（实测 RMS≈0.0008）每个采样都
/// 非零，于是**任何**一帧都被判成「非静音」，`last_non_silent_audio_written_at`
/// 被无意义地一路刷新，收尾判据永远不成立。这里改成按振幅判。
fn contains_non_silent_pcm(pcm: &[u8]) -> bool {
    pcm.chunks_exact(2)
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

/// 硬截止只允许在已有当前句段可返回文本，或确认没有未结算音频时收尾。
///
/// 如果服务端已经确认开始了一段语音、却还没有发出任何 transcript，继续等到
/// FINAL_RESULT_TIMEOUT，让迟到的 completed 有机会到达；超时后由
/// `finish_with_partial_or_error` 返回显式错误，而不是静默成功返回空文本。
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
    // language 省略 => 服务端自动检测语种。
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
    use futures_util::{SinkExt, StreamExt};

    fn create_test_asr() -> StepfunRealtimeASR {
        StepfunRealtimeASR::new(StepfunRealtimeCredentials {
            api_key: "sk-test".to_string(),
            endpoint: String::new(),
            model: String::new(),
            prompt: None,
        })
    }

    /// 语音级 s16le PCM：振幅远超 NON_SILENT_PEAK。
    fn speech_pcm(bytes: usize) -> Vec<u8> {
        std::iter::repeat(6_000i16.to_le_bytes())
            .flatten()
            .take(bytes)
            .collect()
    }

    /// 底噪级 s16le PCM：每个字节都非零（旧的 `byte != 0` 判据会误判成语音），
    /// 但振幅低于 NON_SILENT_PEAK，实际是静音。
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

        // 批式共用的 https base（preset 默认值）→ 派生 wss 完整路径。
        creds.endpoint = "https://api.stepfun.com/v1".to_string();
        assert_eq!(creds.connect_url(), DEFAULT_ENDPOINT);
        creds.endpoint = "https://api.stepfun.com/v1/".to_string();
        assert_eq!(creds.connect_url(), DEFAULT_ENDPOINT);

        // 完整 wss URL 原样使用。
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
        // StepFun 语义：text 前缀 + stash 尾巴（实测 2026-07），非 Qwen 的互斥取一。
        let asr = create_test_asr();
        asr.handle_text_message(&delta_event("", "今天。"));
        assert_eq!(asr.state.lock().partial_text, "今天。");
        asr.handle_text_message(&delta_event("今天天气不错，", "我们来测试。"));
        assert_eq!(asr.state.lock().partial_text, "今天天气不错，我们来测试。");
        // 两者皆空不覆盖已有 partial。
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
            &json!({"type": "transcript.response.error", "error": {"message": "boom"}})
                .to_string(),
        );
        assert!(!keep_going);
        assert_eq!(rx.try_recv().unwrap().unwrap().text, "已识别内容。");
    }

    #[test]
    fn error_event_without_partial_returns_error() {
        let asr = create_test_asr();
        let (tx, mut rx) = oneshot::channel();
        asr.state.lock().final_tx = Some(tx);
        asr.handle_text_message(&json!({"type": "error", "error": {"message": "boom"}}).to_string());
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
        // 连接检查 / 误触场景：无任何句段，finish_success 返回空文本成功。
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
        // 底噪：每个字节都非零，旧的 `byte != 0` 判据会误判成语音。
        assert!(!contains_non_silent_pcm(&room_tone_pcm(3_200)));
        assert!(contains_non_silent_pcm(&speech_pcm(3_200)));
        // 纯静音与单点尖峰都不算有声。
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
            // 保持连接直到客户端处理 completed 并主动关闭，避免测试服务端析构抢先
            // 触发客户端的连接错误。
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

    /// 假网关：握手后回 `session.updated`，收到第一个音频 append 时发出指定事件，
    /// 之后**保持静默**直到客户端关闭连接——模拟 StepFun「没有 finish 事件」的现实。
    async fn spawn_fake_gateway(events_on_first_audio: Vec<String>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/v1/realtime/asr/stream",
            listener.local_addr().unwrap()
        );
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            // 首条必然是 session.update。
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

    /// 回归：松手前已停顿 ≥VAD 阈值 —— 最后一段的 completed 在尾帧**之前**就到了，
    /// 服务端此后不会再发任何事件。
    ///
    /// 修复前：尾帧里残留的底噪被 `byte != 0` 判成语音，把
    /// `last_non_silent_audio_written_at` 推到 completed 之后，一次性宽限检查因此
    /// 不通过、又没有第二次机会 → 干等满 12 秒 FINAL_RESULT_TIMEOUT。实测约 13%
    /// 的听写走到这条路径上。
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
        // 一整帧语音，触发服务端回 speech_started + completed。
        asr.consume_pcm_chunk(&speech_pcm(TARGET_AUDIO_CHUNK_BYTES));
        // 关键时序：必须等 completed **先于**尾帧到达，否则走的是
        // `record_completed` 的健康快路径，复现不出这个 bug。
        tokio::time::timeout(Duration::from_secs(5), async {
            while asr.state.lock().last_completed_at.is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake gateway should have completed the segment");
        // 不足一帧的底噪残留：留在 audio_scratch 里，收尾时跟静音帧拼成尾帧一起
        // 写出。旧判据把它当语音，于是尾帧把「最后一次语音」推到 completed 之后。
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

    /// 回归：服务端始终不关最后一个句段（只有 speech_started + delta，没有
    /// completed）。硬上限必须兜住，而不是退化成 12 秒。
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
        // 未 completed 的 interim 尾巴仍然带出去，不能因为收尾超时就丢字。
        assert_eq!(
            asr.await_final_result().await.unwrap().text,
            "这句话没有收到 completed"
        );
    }

    // 服务端收下 TCP 却永不完成 WebSocket 握手（断网、公司网关 / 酒店门户静默丢包、
    // 服务端黑洞）时，open_session 必须超时返回错误，而不是把调用方永远挂住 ——
    // 它是在串行的 hotkey bridge 线程上 block_on 等的，一旦挂住，按下 / 松开全都排在
    // 队列里没人处理，用户表现为「热键彻底失灵，录音开不了也停不了，只能退出重开」。
    #[tokio::test]
    async fn open_session_times_out_when_handshake_never_completes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // 收下连接后什么都不回，连接一直挂着。
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
        // 上限给足余量，只为证明它真的有界（没有 CONNECT_TIMEOUT 时这里会永远不返回）。
        assert!(
            started.elapsed() < CONNECT_TIMEOUT * 3,
            "超时应在 CONNECT_TIMEOUT 量级返回，实际耗时 {:?}",
            started.elapsed()
        );
    }
}
