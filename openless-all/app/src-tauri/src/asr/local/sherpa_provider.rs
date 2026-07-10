#![allow(dead_code, unused_imports, unused_variables)]
//! sherpa-onnx 本地 ASR provider（Windows offline batch + online streaming）。
//!
//! 形状与 `foundry_provider.rs` 对齐：
//! - 作为 `Recorder::AudioConsumer` 持续吃 PCM
//! - 录音结束后 `transcribe(timeout)` 返回 `RawTranscript`
//! - `cancel()` 让任何 in-flight transcription 提前结束，并清理已缓存 PCM
//!
//! Offline 模型停止录音后把整段 16kHz mono s16le PCM 交给
//! `SherpaOnnxRuntime::transcribe_pcm`。Online 模型在独立 worker 中实时消费 PCM，
//! partial token 通过回调上抛，停止录音后返回 final `RawTranscript`。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use parking_lot::Mutex;

use crate::asr::RawTranscript;

use super::sherpa;
use super::sherpa_runtime::{SherpaOnlineSession, SherpaOnnxRuntime};

pub struct SherpaOnnxAsr {
    runtime: Arc<SherpaOnnxRuntime>,
    model_alias: String,
    language_hint: Option<String>,
    mode: SherpaProviderMode,
    cancel_generation: AtomicU64,
}

enum SherpaProviderMode {
    Offline { buffer: Mutex<Vec<u8>> },
    Online { worker: Mutex<Option<OnlineWorker>> },
}

struct OnlineWorker {
    tx: Sender<OnlineWorkerMessage>,
    result_rx: Mutex<Option<Receiver<Result<String>>>>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
    audio_bytes: AtomicU64,
    cancelled: Arc<AtomicBool>,
}

enum OnlineWorkerMessage {
    Pcm(Vec<u8>),
    Finish,
    Cancel,
}

pub type SherpaTokenHandler = Arc<dyn Fn(String) + Send + Sync + 'static>;

impl SherpaOnnxAsr {
    pub fn new(
        runtime: Arc<SherpaOnnxRuntime>,
        model_alias: String,
        language_hint: Option<String>,
    ) -> Self {
        Self {
            runtime,
            model_alias,
            language_hint: normalize_language_hint(language_hint),
            mode: SherpaProviderMode::Offline {
                buffer: Mutex::new(Vec::new()),
            },
            cancel_generation: AtomicU64::new(0),
        }
    }

    pub async fn new_for_model(
        runtime: Arc<SherpaOnnxRuntime>,
        model_alias: String,
        language_hint: Option<String>,
        token_handler: Option<SherpaTokenHandler>,
    ) -> Result<Self> {
        if sherpa::alias_is_online(&model_alias) {
            let session = runtime.create_online_session(&model_alias).await?;
            Ok(Self {
                runtime,
                model_alias,
                language_hint: normalize_language_hint(language_hint),
                mode: SherpaProviderMode::Online {
                    worker: Mutex::new(Some(OnlineWorker::spawn(session, token_handler))),
                },
                cancel_generation: AtomicU64::new(0),
            })
        } else {
            Ok(Self::new(runtime, model_alias, language_hint))
        }
    }

    #[allow(dead_code)]
    pub fn model_alias(&self) -> &str {
        &self.model_alias
    }

    #[allow(dead_code)]
    pub fn language_hint(&self) -> Option<&str> {
        self.language_hint.as_deref()
    }

    pub async fn transcribe(&self, audio_timeout: Duration) -> Result<RawTranscript> {
        match &self.mode {
            SherpaProviderMode::Offline { buffer } => {
                self.transcribe_offline(buffer, audio_timeout).await
            }
            SherpaProviderMode::Online { worker } => {
                self.transcribe_online(worker, audio_timeout).await
            }
        }
    }

    async fn transcribe_offline(
        &self,
        buffer: &Mutex<Vec<u8>>,
        audio_timeout: Duration,
    ) -> Result<RawTranscript> {
        let cancel_generation = self.cancel_generation.load(Ordering::SeqCst);
        let pcm = buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let duration_ms = pcm_duration_ms(&pcm);
        let result = self
            .runtime
            .transcribe_pcm(&self.model_alias, &pcm, self.language_hint(), audio_timeout)
            .await;

        if self.cancel_generation.load(Ordering::SeqCst) != cancel_generation {
            anyhow::bail!("sherpa-onnx transcription cancelled");
        }

        // 与 Foundry 行为对齐：进入推理后清 buffer，避免下一轮重复消费。
        buffer.lock().clear();

        let text = result?;
        Ok(RawTranscript {
            text: trim_transcript_text(&text),
            duration_ms,
        })
    }

    async fn transcribe_online(
        &self,
        worker_slot: &Mutex<Option<OnlineWorker>>,
        audio_timeout: Duration,
    ) -> Result<RawTranscript> {
        let cancel_generation = self.cancel_generation.load(Ordering::SeqCst);
        let Some(worker) = worker_slot.lock().take() else {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        };
        let duration_ms = pcm_duration_ms_from_bytes(worker.audio_bytes.load(Ordering::SeqCst));
        let started = Instant::now();
        let result = worker.finish(audio_timeout).await;
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if self.cancel_generation.load(Ordering::SeqCst) != cancel_generation {
            self.runtime.record_streaming_result(
                &self.model_alias,
                duration_ms,
                elapsed_ms,
                Some("sherpa-onnx streaming transcription cancelled".into()),
            );
            anyhow::bail!("sherpa-onnx streaming transcription cancelled");
        }
        match &result {
            Ok(_) => self.runtime.record_streaming_result(
                &self.model_alias,
                duration_ms,
                elapsed_ms,
                None,
            ),
            Err(error) => self.runtime.record_streaming_result(
                &self.model_alias,
                duration_ms,
                elapsed_ms,
                Some(format!("{error:#}")),
            ),
        }
        let text = result?;
        Ok(RawTranscript {
            text: trim_transcript_text(&text),
            duration_ms,
        })
    }

    pub fn cancel(&self) {
        self.cancel_generation.fetch_add(1, Ordering::SeqCst);
        self.runtime.request_cancel_prepare();
        match &self.mode {
            SherpaProviderMode::Offline { buffer } => buffer.lock().clear(),
            SherpaProviderMode::Online { worker } => {
                if let Some(worker) = worker.lock().take() {
                    worker.cancel();
                }
            }
        }
    }
}

impl crate::recorder::AudioConsumer for SherpaOnnxAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        match &self.mode {
            SherpaProviderMode::Offline { buffer } => buffer.lock().extend_from_slice(pcm),
            SherpaProviderMode::Online { worker } => {
                if let Some(worker) = worker.lock().as_ref() {
                    worker.send_pcm(pcm);
                }
            }
        }
    }
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    crate::asr::pcm::pcm_duration_ms(pcm)
}

fn pcm_duration_ms_from_bytes(bytes: u64) -> u64 {
    crate::asr::pcm::pcm_duration_ms_from_bytes(bytes)
}

fn trim_transcript_text(text: &str) -> String {
    text.trim().to_string()
}

fn normalize_language_hint(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

impl OnlineWorker {
    fn spawn(mut session: SherpaOnlineSession, token_handler: Option<SherpaTokenHandler>) -> Self {
        let alias = session.alias().to_string();
        let (tx, rx) = mpsc::channel::<OnlineWorkerMessage>();
        let (result_tx, result_rx) = mpsc::channel::<Result<String>>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let join_handle = std::thread::Builder::new()
            .name(format!("openless-sherpa-online-{alias}"))
            .spawn(move || {
                let emit = |piece: &str| {
                    if piece.is_empty() || worker_cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(handler) = token_handler.as_ref() {
                        handler(piece.to_string());
                    }
                };
                let result = loop {
                    match rx.recv() {
                        Ok(OnlineWorkerMessage::Pcm(pcm)) => {
                            if worker_cancelled.load(Ordering::SeqCst) {
                                break Err(anyhow::anyhow!("sherpa-onnx streaming cancelled"));
                            }
                            if let Err(error) = session.accept_pcm_chunk(&pcm, &emit) {
                                break Err(error);
                            }
                            if worker_cancelled.load(Ordering::SeqCst) {
                                break Err(anyhow::anyhow!("sherpa-onnx streaming cancelled"));
                            }
                        }
                        Ok(OnlineWorkerMessage::Finish) => {
                            if worker_cancelled.load(Ordering::SeqCst) {
                                break Err(anyhow::anyhow!("sherpa-onnx streaming cancelled"));
                            }
                            break session.finish(&emit);
                        }
                        Ok(OnlineWorkerMessage::Cancel) | Err(_) => {
                            worker_cancelled.store(true, Ordering::SeqCst);
                            break Err(anyhow::anyhow!("sherpa-onnx streaming cancelled"));
                        }
                    }
                };
                let _ = result_tx.send(result);
            })
            .expect("spawn sherpa online worker");

        Self {
            tx,
            result_rx: Mutex::new(Some(result_rx)),
            join_handle: Mutex::new(Some(join_handle)),
            audio_bytes: AtomicU64::new(0),
            cancelled,
        }
    }

    fn send_pcm(&self, pcm: &[u8]) {
        if pcm.is_empty() || self.cancelled.load(Ordering::SeqCst) {
            return;
        }
        self.audio_bytes
            .fetch_add(pcm.len() as u64, Ordering::SeqCst);
        if self
            .tx
            .send(OnlineWorkerMessage::Pcm(pcm.to_vec()))
            .is_err()
        {
            log::warn!("[sherpa-asr] online worker is not accepting PCM");
        }
    }

    async fn finish(self, audio_timeout: Duration) -> Result<String> {
        let _ = self.tx.send(OnlineWorkerMessage::Finish);
        let result_rx = self
            .result_rx
            .lock()
            .take()
            .ok_or_else(|| anyhow::anyhow!("sherpa-onnx streaming result already taken"))?;
        let join_handle = self.join_handle.lock().take();
        let result = tokio::time::timeout(audio_timeout, async move {
            tokio::task::spawn_blocking(move || {
                result_rx.recv().map_err(|error| {
                    anyhow::anyhow!("sherpa-onnx streaming worker closed: {error}")
                })?
            })
            .await
            .map_err(|error| anyhow::anyhow!("sherpa-onnx streaming join failed: {error:#}"))?
        })
        .await;
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                self.cancelled.store(true, Ordering::SeqCst);
                let _ = self.tx.send(OnlineWorkerMessage::Cancel);
                anyhow::bail!("sherpa-onnx streaming transcribe timeout");
            }
        };
        if let Some(join_handle) = join_handle {
            let _ = join_handle.join();
        }
        result
    }

    fn cancel(self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.tx.send(OnlineWorkerMessage::Cancel);
        if let Some(join_handle) = self.join_handle.lock().take() {
            let _ = join_handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;

    fn make_provider() -> SherpaOnnxAsr {
        SherpaOnnxAsr::new(
            Arc::new(SherpaOnnxRuntime::new()),
            "sense-voice-small-zh".into(),
            Some("  ZH  ".into()),
        )
    }

    #[test]
    fn normalize_language_hint_trims_and_lowercases() {
        let provider = make_provider();
        assert_eq!(provider.language_hint(), Some("zh"));
    }

    #[test]
    fn empty_language_hint_normalizes_to_none() {
        let provider = SherpaOnnxAsr::new(
            Arc::new(SherpaOnnxRuntime::new()),
            "paraformer-zh".into(),
            Some("   ".into()),
        );
        assert!(provider.language_hint().is_none());
    }

    #[test]
    fn consume_pcm_chunk_extends_buffer() {
        let provider = make_provider();
        provider.consume_pcm_chunk(&[1, 2, 3, 4]);
        provider.consume_pcm_chunk(&[5, 6]);
        match &provider.mode {
            SherpaProviderMode::Offline { buffer } => assert_eq!(buffer.lock().len(), 6),
            SherpaProviderMode::Online { .. } => panic!("expected offline provider"),
        }
    }

    #[tokio::test]
    async fn empty_buffer_transcribe_returns_empty_transcript() {
        let provider = make_provider();
        let result = provider.transcribe(Duration::from_secs(5)).await.unwrap();
        assert!(result.text.is_empty());
        assert_eq!(result.duration_ms, 0);
    }

    #[tokio::test]
    async fn transcribe_clears_buffer_on_runtime_error() {
        let provider = SherpaOnnxAsr::new(
            Arc::new(SherpaOnnxRuntime::new()),
            "unknown-sherpa-model".into(),
            None,
        );
        provider.consume_pcm_chunk(&vec![0u8; 32_000]);
        let result = provider.transcribe(Duration::from_secs(5)).await;
        assert!(result.is_err());
        match &provider.mode {
            SherpaProviderMode::Offline { buffer } => assert!(buffer.lock().is_empty()),
            SherpaProviderMode::Online { .. } => panic!("expected offline provider"),
        }
    }

    #[test]
    fn cancel_clears_buffer_and_bumps_generation() {
        let runtime = Arc::new(SherpaOnnxRuntime::new());
        let provider = SherpaOnnxAsr::new(
            Arc::clone(&runtime),
            "sense-voice-small-zh".into(),
            Some("  ZH  ".into()),
        );
        provider.consume_pcm_chunk(&[1, 2, 3, 4]);
        let before = provider.cancel_generation.load(Ordering::SeqCst);
        provider.cancel();
        let after = provider.cancel_generation.load(Ordering::SeqCst);
        assert!(after > before);
        assert!(runtime.cancel_prepare_requested_for_tests());
        match &provider.mode {
            SherpaProviderMode::Offline { buffer } => assert!(buffer.lock().is_empty()),
            SherpaProviderMode::Online { .. } => panic!("expected offline provider"),
        }
    }
}
