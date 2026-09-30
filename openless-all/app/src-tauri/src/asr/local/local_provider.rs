//! Adapter that plugs the local Qwen3-ASR into the dictation path.
//!
//! Same shape as `WhisperBatchASR`: implements `AudioConsumer` to buffer PCM;
//! on stop, the MLX backend decodes the whole recording as one batch while the
//! C backend keeps streaming decode, emitting stable tokens to the frontend
//! via `local-asr-token`.
//!
//! The engine is now provided by `LocalAsrCache` — the Coordinator fetches the
//! cached engine in build_local_qwen3 and passes it in, avoiding reloading the
//! 1.2GB+ model on every session.

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Arc;

#[cfg(target_os = "macos")]
use super::LocalQwenEngine;
#[cfg(target_os = "macos")]
use crate::asr::RawTranscript;
#[cfg(target_os = "macos")]
use anyhow::{Context, Result};
#[cfg(target_os = "macos")]
use parking_lot::Mutex;
#[cfg(target_os = "macos")]
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "macos")]
pub struct LocalQwenAsr {
    engine: Arc<LocalQwenEngine>,
    operation_id: u64,
    cancelled: Arc<AtomicBool>,
    buffer: Mutex<Vec<u8>>,
    app: AppHandle,
}

#[cfg(target_os = "macos")]
impl LocalQwenAsr {
    pub fn new(app: AppHandle, engine: Arc<LocalQwenEngine>) -> Self {
        let operation_id = engine.next_operation_id();
        Self {
            engine,
            operation_id,
            cancelled: Arc::new(AtomicBool::new(false)),
            buffer: Mutex::new(Vec::new()),
            app,
        }
    }

    /// Duration (ms) of buffered audio. The coordinator reads it before
    /// calling transcribe() to compute the dynamic timeout for local Qwen ASR
    /// (max(15, ceil(audio_s × 0.6) + 10)). Does not consume the buffer.
    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(self.buffer.lock().len())
    }

    /// Called on stop: MLX decodes the whole recording as one batch; the C
    /// backend keeps its historical streaming tokens and trailing-silence
    /// finalization behavior.
    pub async fn transcribe(self: Arc<Self>) -> Result<RawTranscript> {
        self.cancelled.store(false, Ordering::Release);
        let pcm_bytes = std::mem::take(&mut *self.buffer.lock());
        if pcm_bytes.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }
        let duration_ms = pcm_duration_ms(pcm_bytes.len());
        let samples_f32 = i16_le_bytes_to_f32(&pcm_bytes);
        let engine = Arc::clone(&self.engine);
        let operation_id = self.operation_id;
        let app = self.app.clone();
        let cancelled = Arc::clone(&self.cancelled);
        let worker_cancelled = Arc::clone(&cancelled);
        let text = tauri::async_runtime::spawn_blocking(move || {
            engine.transcribe_dictation_with_handler(
                operation_id,
                &worker_cancelled,
                samples_f32,
                move |piece: &str| {
                    if !token_emission_enabled(&cancelled) {
                        return;
                    }
                    if let Err(error) = app.emit("local-asr-token", piece.to_string()) {
                        log::warn!("[local-asr] emit token failed: {error}");
                    }
                },
            )
        })
        .await
        .context("transcribe spawn_blocking join 失败")?
        .context("本地 Qwen3-ASR 解码失败")?;

        Ok(RawTranscript { text, duration_ms })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.buffer.lock().clear();
        self.engine.cancel_operation(self.operation_id);
    }
}

#[cfg(target_os = "macos")]
impl crate::recorder::AudioConsumer for LocalQwenAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

#[cfg(target_os = "macos")]
fn i16_le_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|c| {
            let v = i16::from_le_bytes([c[0], c[1]]);
            v as f32 / 32768.0
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn pcm_duration_ms(byte_len: usize) -> u64 {
    (byte_len as u64 / 2) * 1000 / 16_000
}

#[cfg(target_os = "macos")]
fn token_emission_enabled(cancelled: &AtomicBool) -> bool {
    !cancelled.load(Ordering::Acquire)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn duration_uses_only_original_pcm_bytes() {
        assert_eq!(pcm_duration_ms(32_000), 1_000);
    }

    #[test]
    fn cancellation_closes_the_token_emission_gate() {
        let cancelled = AtomicBool::new(false);
        assert!(token_emission_enabled(&cancelled));

        cancelled.store(true, Ordering::Release);

        assert!(!token_emission_enabled(&cancelled));
    }
}
