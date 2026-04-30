//! Volcengine SAUC bigmodel streaming ASR client.
//!
//! Direct port of the Swift `VolcengineStreamingASR`. Battle-tested protocol
//! quirks are preserved verbatim — see comments tagged with `[asr]` for the
//! original learnings (especially the "definite=true is NOT stream end" bug).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex as ParkingMutex;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::runtime::Handle;
use tokio::sync::{oneshot, Mutex as AsyncMutex, Notify};
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

#[derive(Clone, Debug)]
pub struct VolcengineCredentials {
    pub app_id: String,
    pub access_token: String,
    pub resource_id: String,
}

impl VolcengineCredentials {
    pub fn default_resource_id() -> &'static str {
        "volc.bigasr.sauc.duration"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VolcengineASRError {
    #[error("credentials missing")]
    CredentialsMissing,
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("authentication failed")]
    AuthenticationFailed,
    #[error("no final result")]
    NoFinalResult,
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
    /// 在飞的 audio 帧 spawn 数。consume_pcm_chunk +1，spawn 内 send 完成 -1。
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
            pending_sends: Arc::new(AtomicUsize::new(0)),
            send_done: Arc::new(Notify::new()),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.state.lock().is_connected
    }

    pub async fn open_session(self: &Arc<Self>) -> Result<(), VolcengineASRError> {
        if self.credentials.app_id.is_empty()
            || self.credentials.access_token.is_empty()
            || self.credentials.resource_id.is_empty()
        {
            return Err(VolcengineASRError::CredentialsMissing);
        }

        let connect_id = Uuid::new_v4().to_string();
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
            HeaderValue::from_str(&connect_id)
                .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?,
        );

        let (ws, _resp) = connect_async(request)
            .await
            .map_err(|e| VolcengineASRError::ConnectionFailed(e.to_string()))?;
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
        let rx = self.final_rx.lock().take();
        let Some(rx) = rx else {
            return Err(VolcengineASRError::NoFinalResult);
        };
        match rx.await {
            Ok(result) => result,
            Err(_) => Err(VolcengineASRError::NoFinalResult),
        }
    }

    pub fn cancel(&self) {
        let runtime = {
            let mut st = self.state.lock();
            st.is_connected = false;
            st.pending_audio.clear();
            st.runtime.clone()
        };
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
            // 优先用 utterances 拼接的文本（包含全部分段，不论 definite 与否）
            let pieces: Vec<&str> = utterances
                .iter()
                .filter_map(|u| u.get("text").and_then(|t| t.as_str()))
                .collect();
            if !pieces.is_empty() {
                full_text = pieces.join("");
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
    }
}

impl AudioConsumer for VolcengineStreamingASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let runtime = {
            let mut st = self.state.lock();
            if !st.is_connected {
                return;
            }
            st.pending_audio.extend_from_slice(pcm);
            st.runtime.clone()
        };

        let Some(runtime) = runtime else {
            return;
        };

        // Drain as many full chunks as we have, spawning one send per chunk.
        loop {
            let chunk_and_seq = {
                let mut st = self.state.lock();
                if st.pending_audio.len() < TARGET_AUDIO_CHUNK_BYTES {
                    None
                } else {
                    let chunk: Vec<u8> =
                        st.pending_audio.drain(..TARGET_AUDIO_CHUNK_BYTES).collect();
                    let seq = st.next_sequence;
                    st.next_sequence += 1;
                    st.bytes_sent += chunk.len();
                    st.frames_sent += 1;
                    Some((chunk, seq))
                }
            };

            let Some((chunk, seq)) = chunk_and_seq else {
                break;
            };

            let writer = Arc::clone(&self.writer);
            // pending_sends + Notify 让 send_last_frame 知道何时所有 chunk 都已发出。
            self.pending_sends.fetch_add(1, Ordering::SeqCst);
            let pending = Arc::clone(&self.pending_sends);
            let notify = Arc::clone(&self.send_done);
            runtime.spawn(async move {
                let frame = frame::build(
                    MessageType::AudioOnlyRequest,
                    Flags::PositiveSequence,
                    Serialization::None,
                    &chunk,
                    Some(seq),
                );
                if let Err(e) = send_binary(&writer, frame).await {
                    // 把丢帧错误顶到日志里，定位"为什么服务端只收到 100ms"
                    log::error!("[asr] audio frame seq={} send 失败: {}", seq, e);
                }
                if pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                    notify.notify_waiters();
                }
            });
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
            "volc.bigasr.sauc.duration"
        );
    }
}
