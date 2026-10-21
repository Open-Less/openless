//! Real-time Gemini transcription, shared by Traditional and Smart dictation.

mod protocol;
mod session;
#[cfg(test)]
mod tests;
pub(crate) mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot, watch};

use super::gemini::TranscriptionMode;
use super::{AudioConsumer, RawTranscript};
use crate::config::TaskSpawner;
use crate::ports::TextStreamSink;

pub const MODEL: &str = "gemini-3.5-transcribe-live";
const CHUNK_BYTES: usize = 3_200;
const MAX_AUDIO_BYTES: usize = 32_000 * 600;
// AudioConsumer is synchronous: history retries replay a complete recording in one call.
// Bound the queue by the model's audio limit, with room for a tail frame and stop marker.
const QUEUE_FRAMES: usize = MAX_AUDIO_BYTES / CHUNK_BYTES + 2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const FINISH_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-recording credentials and structured transcription hints; never debug-log this value.
#[derive(Clone)]
pub struct GeminiLiveConfig {
    pub api_key: String,
    pub base_url: String,
    pub languages: Vec<String>,
    pub hotwords: Vec<String>,
    pub mode: TranscriptionMode,
    pub extra_headers: HashMap<String, String>,
}

enum Command {
    Audio(Vec<u8>),
    Finish { duration_ms: u64, silence: bool },
}

struct AudioState {
    sender: Option<mpsc::Sender<Command>>,
    pending: Vec<u8>,
    bytes: usize,
    silence: bool,
}

/// Owns one recording. A single worker owns the socket, so cancellation drops both halves.
pub struct GeminiLiveASR {
    audio: Mutex<AudioState>,
    result: Mutex<Option<oneshot::Receiver<Result<RawTranscript>>>>,
    shutdown: watch::Sender<Option<&'static str>>,
}

impl GeminiLiveASR {
    /// Finish setup before accepting audio. Dropping a pending handshake closes its socket.
    pub async fn connect(
        config: GeminiLiveConfig,
        partials: Arc<dyn TextStreamSink>,
        task_spawner: Arc<dyn TaskSpawner>,
    ) -> Result<Arc<Self>> {
        if config.api_key.trim().is_empty() {
            bail!("Gemini Live API key is required");
        }
        let socket = tokio::time::timeout(CONNECT_TIMEOUT, session::connect(&config))
            .await
            .context("Gemini Live setup timed out")??;
        let (sender, receiver) = mpsc::channel(QUEUE_FRAMES);
        let (result_tx, result_rx) = oneshot::channel();
        let (shutdown, cancelled) = watch::channel(None);
        let provider = Arc::new(Self {
            audio: Mutex::new(AudioState {
                sender: Some(sender),
                pending: Vec::new(),
                bytes: 0,
                silence: true,
            }),
            result: Mutex::new(Some(result_rx)),
            shutdown,
        });
        task_spawner.spawn(Box::pin(async move {
            let result = session::run(socket, receiver, cancelled, partials).await;
            let _ = result_tx.send(result);
        }));
        Ok(provider)
    }

    /// Queue the trailing PCM and stop marker in order, then await authoritative text.
    pub async fn finish(&self) -> Result<RawTranscript> {
        let result = self
            .result
            .lock()
            .take()
            .context("Gemini Live session already finished")?;
        let _cleanup = CancelOnDrop(self.shutdown.clone());
        {
            let mut audio = self.audio.lock();
            if let Some(sender) = audio.sender.take() {
                if !audio.pending.len().is_multiple_of(2) {
                    self.fail("Gemini Live audio ended with an incomplete PCM sample");
                } else {
                    let tail = std::mem::take(&mut audio.pending);
                    if !tail.is_empty() && sender.try_send(Command::Audio(tail)).is_err() {
                        self.fail("Gemini Live audio queue is unavailable");
                    }
                    let duration_ms = super::pcm::pcm_duration_ms_from_bytes(audio.bytes as u64);
                    let _ = sender
                        .try_send(Command::Finish {
                            duration_ms,
                            silence: audio.silence,
                        })
                        .map_err(|_| self.fail("Gemini Live audio queue is unavailable"));
                }
            }
        }
        result
            .await
            .context("Gemini Live transcription worker stopped")?
    }

    /// Cancellation is synchronous and also interrupts a blocked socket write.
    pub fn cancel(&self) {
        self.fail("Gemini Live transcription cancelled");
        let mut audio = self.audio.lock();
        audio.sender.take();
        audio.pending.clear();
    }

    fn fail(&self, reason: &'static str) {
        self.shutdown.send_if_modified(|current| {
            if current.is_some() {
                false
            } else {
                *current = Some(reason);
                true
            }
        });
    }
}

impl AudioConsumer for GeminiLiveASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        let mut audio = self.audio.lock();
        let Some(sender) = audio.sender.clone() else {
            return;
        };
        if pcm.is_empty() || self.shutdown.borrow().is_some() {
            return;
        }
        if pcm.len() > MAX_AUDIO_BYTES.saturating_sub(audio.bytes) {
            self.fail("Gemini Live recordings are limited to 10 minutes");
            return;
        }
        audio.bytes += pcm.len();
        audio.silence &= pcm.iter().all(|byte| *byte == 0);
        // Keep at most one incomplete frame outside the bounded worker queue.
        for chunk in pcm.chunks(CHUNK_BYTES) {
            audio.pending.extend_from_slice(chunk);
            if audio.pending.len() >= CHUNK_BYTES {
                let tail = audio.pending.split_off(CHUNK_BYTES);
                let frame = std::mem::replace(&mut audio.pending, tail);
                if sender.try_send(Command::Audio(frame)).is_err() {
                    self.fail(
                        "Gemini Live audio queue overflowed or disconnected; retry the recording",
                    );
                    return;
                }
            }
        }
    }
}

impl Drop for GeminiLiveASR {
    fn drop(&mut self) {
        self.fail("Gemini Live transcription cancelled");
    }
}

/// A dropped finish future must stop its detached worker even if the session is retained.
struct CancelOnDrop(watch::Sender<Option<&'static str>>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0
            .send_replace(Some("Gemini Live transcription cancelled"));
    }
}
