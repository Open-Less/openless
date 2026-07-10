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
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

use super::{AudioConsumer, RawTranscript};

pub const PROVIDER_ID: &str = "bailian";
pub const DEFAULT_ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference/";
pub const DEFAULT_MODEL: &str = "fun-asr-realtime";

/// 100 ms of 16 kHz / 16-bit / mono PCM.
pub const TARGET_AUDIO_CHUNK_BYTES: usize = 3_200;
const BYTES_PER_MS: u64 = 32;
const FINAL_RESULT_TIMEOUT: Duration = Duration::from_secs(12);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

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

#[derive(Debug, thiserror::Error)]
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
    Audio(Vec<u8>),
    Finish(oneshot::Sender<Result<(), BailianASRError>>),
}

#[derive(Default)]
struct SyncState {
    task_id: String,
    pending_audio: Vec<u8>,
    audio_scratch: Vec<u8>,
    bytes_received: u64,
    task_started: bool,
    task_finished: bool,
    runtime: Option<Handle>,
    start: Option<Instant>,
    final_tx: Option<oneshot::Sender<Result<RawTranscript, BailianASRError>>>,
    send_tx: Option<mpsc::UnboundedSender<SendItem>>,
    /// sentence_id → text，按 sentence_id 排序拼接得到最终文本。
    /// 同一 sentence_id 的后到结果覆盖前一个，消除累积文本导致的重复。
    final_segments: BTreeMap<i64, String>,
    /// sentence_id → interim 文本，sentence_end == false 时更新，
    /// 收到同 sentence_id 的 final 结果时将内容移入 final_segments。
    partial_segments: BTreeMap<i64, String>,
    last_result_text: String,
}

pub struct BailianRealtimeASR {
    credentials: BailianCredentials,
    state: ParkingMutex<SyncState>,
    writer: SharedWriter,
    final_rx: ParkingMutex<Option<oneshot::Receiver<Result<RawTranscript, BailianASRError>>>>,
    task_started: Arc<Notify>,
}

impl BailianRealtimeASR {
    pub fn new(credentials: BailianCredentials) -> Self {
        Self {
            credentials,
            state: ParkingMutex::new(SyncState::default()),
            writer: Arc::new(AsyncMutex::new(None)),
            final_rx: ParkingMutex::new(None),
            task_started: Arc::new(Notify::new()),
        }
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

        let (ws, _resp) = connect_async(request)
            .await
            .map_err(|e| BailianASRError::ConnectionFailed(e.to_string()))?;
        let (write, read) = ws.split();
        *self.writer.lock().await = Some(write);

        let (final_tx, final_rx) = oneshot::channel();
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<SendItem>();
        {
            let mut st = self.state.lock();
            *st = SyncState::default();
            st.task_id = task_id.clone();
            st.runtime = Some(Handle::current());
            st.start = Some(Instant::now());
            st.final_tx = Some(final_tx);
            st.send_tx = Some(send_tx);
        }
        *self.final_rx.lock() = Some(final_rx);

        let writer_for_worker = Arc::clone(&self.writer);
        let task_id_for_worker = task_id.clone();
        tokio::spawn(async move {
            while let Some(item) = send_rx.recv().await {
                match item {
                    SendItem::Audio(chunk) => {
                        if let Err(e) = send_binary(&writer_for_worker, chunk).await {
                            log::error!("[bailian-asr] audio frame send failed: {e}");
                        }
                    }
                    SendItem::Finish(done) => {
                        let result =
                            send_text(&writer_for_worker, finish_task_message(&task_id_for_worker))
                                .await
                                .map_err(|e| BailianASRError::SendFailed(e.to_string()));
                        let _ = done.send(result);
                    }
                }
            }
        });

        send_text(
            &self.writer,
            run_task_message(
                &task_id,
                &self.credentials.normalized_model(),
                self.credentials.vocabulary_id.as_deref(),
            ),
        )
        .await?;

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
        });

        Ok(())
    }

    pub async fn send_last_frame(&self) -> Result<(), BailianASRError> {
        let started = self.task_started.notified();
        tokio::pin!(started);
        started.as_mut().enable();
        let ready = {
            let st = self.state.lock();
            st.task_started || st.task_finished
        };
        if !ready {
            tokio::time::timeout(Duration::from_secs(5), started)
                .await
                .map_err(|_| BailianASRError::FinalResultTimeout)?;
        }
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
            let _ = send_tx.send(SendItem::Audio(chunk));
        }
        let (done_tx, done_rx) = oneshot::channel();
        send_tx
            .send(SendItem::Finish(done_tx))
            .map_err(|_| BailianASRError::SendFailed("send worker closed".to_string()))?;
        done_rx
            .await
            .map_err(|_| BailianASRError::SendFailed("finish ack dropped".to_string()))?
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
        st.send_tx.take();
        st.final_tx.take();
        st.task_finished = true;
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
        let (send_tx, chunks) = {
            let mut st = self.state.lock();
            st.task_started = true;
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

        // 跳过 heartbeat 事件（不含识别文本）
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

        // 使用 API 文档标注的 sentence_end 作为 finality 判断。
        // end_time > 0 仅在 sentence_end 字段完全不存在时作为兼容 fallback，
        // 因为 DashScope 的 interim 结果也包含正数的 end_time（随音频推进增长），
        // 直接 fallback 会导致 interim 结果被误判为 final，重现累积文本重复。
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

        let mut st = self.state.lock();
        st.last_result_text = trimmed.to_string();

        if is_sentence_final {
            // 所有 final 结果（含 sentence_id == 0）都存入 final_segments。
            // BTreeMap 覆盖语义保证同一 sentence_id 不会重复追加。
            st.final_segments.insert(sentence_id, trimmed.to_string());
            // 清理该句的 interim 缓存
            st.partial_segments.remove(&sentence_id);
        } else {
            // interim 结果暂存 partial，同一 sentence_id 后到覆盖前到
            st.partial_segments.insert(sentence_id, trimmed.to_string());
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
        self.close_on_runtime();
    }

    fn finish_with_partial_or_error(&self, error: BailianASRError) {
        let has_partial = {
            let st = self.state.lock();
            !st.last_result_text.trim().is_empty()
                || !st.final_segments.is_empty()
                || !st.partial_segments.is_empty()
        };
        if has_partial {
            // 与 Volcengine 保持一致：连接异常但已有 partial 时优先兜底返回，避免丢失用户已识别出的内容。
            self.finish_success();
        } else {
            self.finish_error(error);
        }
    }

    fn finish_error(&self, error: BailianASRError) {
        let tx = {
            let mut st = self.state.lock();
            if st.task_finished {
                return;
            }
            st.task_finished = true;
            st.send_tx.take();
            st.final_tx.take()
        };
        if let Some(tx) = tx {
            let _ = tx.send(Err(error));
        }
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

impl AudioConsumer for BailianRealtimeASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        let (send_tx, chunks) = {
            let mut st = self.state.lock();
            st.bytes_received = st.bytes_received.saturating_add(pcm.len() as u64);
            if !st.task_started {
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

/// 带重叠检测的文本段拼接：如果后一段的开头与前一段的末尾存在重叠，
/// 只追加不重叠的尾部，避免因 API 重放或重复事件导致的累积文本重复。
///
/// 最小重叠长度为 2 个字符，避免单字巧合匹配（如"今天"+"天气"）。
/// 例如 ["你好吗", "好吗我们"] → "你好吗我们"
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
        "sample_rate": 16000,
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
    ws.send(Message::Text(text))
        .await
        .map_err(|e| BailianASRError::SendFailed(e.to_string()))
}

async fn send_binary(writer: &SharedWriter, data: Vec<u8>) -> Result<(), BailianASRError> {
    let mut guard = writer.lock().await;
    let Some(ws) = guard.as_mut() else {
        return Err(BailianASRError::ConnectionFailed(
            "websocket writer not available".to_string(),
        ));
    };
    ws.send(Message::Binary(data))
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

    // ---- helpers ----

    fn make_result_event(sentence_id: i64, text: &str, is_final: bool) -> Value {
        json!({
            "payload": {
                "output": {
                    "sentence": {
                        "sentence_id": sentence_id,
                        "text": text,
                        "sentence_end": is_final,
                        // end_time 始终为正数，匹配 DashScope 真实 API 行为：
                        // interim 和 final 都携带正数的 end_time。
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
        // DashScope 真实 API 中 interim 结果同时带有 sentence_end: false
        // 和正数的 end_time，验证这不会被误判为 final。
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
}
