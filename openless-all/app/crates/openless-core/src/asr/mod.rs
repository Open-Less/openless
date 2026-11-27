//! Cross-platform ASR protocol implementations shared by every host.

pub mod bailian;
pub mod dashscope_multimodal;
pub mod elevenlabs;
mod frame;
pub mod gemini;
pub mod gemini_live;
pub mod mimo;
pub mod pcm;
pub mod qwen_realtime;
pub mod stepfun_realtime;
pub mod tencent_cloud;
pub mod volcengine;
pub mod wav;
pub mod whisper;
pub mod xfyun;

pub use crate::ports::AudioConsumer;
pub use bailian::{BailianCredentials, BailianRealtimeASR};
pub use dashscope_multimodal::DashScopeMultimodalASR;
pub use elevenlabs::ElevenLabsBatchASR;
pub use gemini::GeminiBatchASR;
pub use mimo::MimoBatchASR;
pub use qwen_realtime::{Qwen3RealtimeASR, Qwen3RealtimeCredentials};
pub use stepfun_realtime::{StepfunRealtimeASR, StepfunRealtimeCredentials};
pub use tencent_cloud::{TencentCloudCredentials, TencentCloudStreamingASR};
pub use volcengine::{VolcengineCredentials, VolcengineStreamingASR};
pub use whisper::WhisperBatchASR;
pub use xfyun::{XfyunCredentials, XfyunStreamingASR};

/// What a provider yielded after the stream or batch request completed.
pub type RawTranscript = crate::ports::TranscriptOutput;

pub(super) mod stream_send {
    use std::sync::Arc;
    use std::time::Duration;

    use futures_util::SinkExt;
    use tokio::net::TcpStream;
    use tokio::sync::{watch, Mutex as AsyncMutex};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

    pub(in crate::asr) type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
    pub(in crate::asr) type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
    pub(in crate::asr) type SharedWriter = Arc<AsyncMutex<Option<WsSink>>>;

    pub(in crate::asr) enum WsSendError {
        NotOpen,
        Cancelled,
        Timeout(u128),
        Transport(String),
    }

    impl WsSendError {
        pub(in crate::asr) fn into_message(self, closed_msg: &str) -> String {
            match self {
                Self::NotOpen => closed_msg.to_string(),
                Self::Cancelled => "session cancelled".to_string(),
                Self::Timeout(ms) => format!("audio frame send timed out after {ms} ms"),
                Self::Transport(msg) => msg,
            }
        }
    }

    /// Tracks non-drifting per-frame send slots for streaming ASR workers.
    pub(in crate::asr) struct PacedSendClock {
        next_send_at: Option<tokio::time::Instant>,
    }

    impl PacedSendClock {
        pub(in crate::asr) fn new() -> Self {
            Self { next_send_at: None }
        }

        /// Sleeps until the next scheduled slot unless `cancel_rx` fires first.
        pub(in crate::asr) async fn wait_slot(
            &self,
            cancel_rx: &mut watch::Receiver<bool>,
        ) -> Option<tokio::time::Instant> {
            if *cancel_rx.borrow_and_update() {
                return None;
            }
            let now = tokio::time::Instant::now();
            let Some(deadline) = self.next_send_at else {
                return Some(now);
            };
            if deadline <= now {
                return Some(now);
            }
            loop {
                if *cancel_rx.borrow_and_update() {
                    return None;
                }
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => {
                        return (!*cancel_rx.borrow_and_update()).then_some(deadline);
                    }
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow_and_update() {
                            return None;
                        }
                    }
                }
            }
        }

        pub(in crate::asr) fn mark_sent(
            &mut self,
            scheduled_start: tokio::time::Instant,
            byte_len: usize,
            bytes_per_ms: u64,
            rate_multiplier: u64,
        ) {
            let denom = bytes_per_ms.saturating_mul(rate_multiplier).max(1);
            let micros = (byte_len as u64)
                .saturating_mul(1_000)
                .div_ceil(denom)
                .max(1);
            let interval = Duration::from_micros(micros);
            self.next_send_at = Some((scheduled_start + interval).max(tokio::time::Instant::now()));
        }
    }

    pub(in crate::asr) async fn send_ws_message_with_timeout(
        writer: &SharedWriter,
        msg: Message,
        timeout: Duration,
        cancel_rx: &mut watch::Receiver<bool>,
    ) -> Result<(), WsSendError> {
        if *cancel_rx.borrow_and_update() {
            return Err(WsSendError::Cancelled);
        }
        tokio::select! {
            result = tokio::time::timeout(timeout, async {
                let mut guard = writer.lock().await;
                let Some(ws) = guard.as_mut() else {
                    return Err(WsSendError::NotOpen);
                };
                ws.send(msg)
                    .await
                    .map_err(|e| WsSendError::Transport(e.to_string()))
            }) => {
                match result {
                    Ok(inner) => inner,
                    Err(_) => Err(WsSendError::Timeout(timeout.as_millis())),
                }
            }
            changed = cancel_rx.changed() => {
                let _ = changed;
                Err(WsSendError::Cancelled)
            }
        }
    }
}

/// User-defined hotword used to bias providers that expose that capability.
#[derive(Debug, Clone)]
pub struct DictionaryHotword {
    pub phrase: String,
    pub enabled: bool,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct TranscriptCapture(std::sync::Mutex<Vec<crate::ports::TextStreamChunk>>);
#[cfg(test)]
impl crate::ports::TextStreamSink for TranscriptCapture {
    fn publish(&self, chunk: crate::ports::TextStreamChunk) -> Result<(), crate::BackendError> {
        self.0.lock().unwrap().push(chunk);
        Ok(())
    }
}
#[cfg(test)]
impl TranscriptCapture {
    pub(crate) fn assert_snapshots(&self, expected: &[&str]) {
        let chunks = self.0.lock().unwrap();
        assert!(chunks.iter().all(|chunk| chunk.offset == 0));
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
}
