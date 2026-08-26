//! 本地 Qwen3-ASR 在 dictation 路径上的适配器。
//!
//! 与 `WhisperBatchASR` 形状对齐：实现 `AudioConsumer` 缓冲 PCM，stop 时
//! MLX 后端整段 batch 解码；C 后端保持流式解码，并通过 `local-asr-token`
//! 向前端发送稳定 token。
//!
//! engine 现在由 `LocalAsrCache` 提供——Coordinator 在 build_local_qwen3 里
//! 取已缓存的引擎再传进来，避免每次会话都重加载 1.2GB+ 模型。

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::LocalQwenEngine;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::asr::RawTranscript;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use anyhow::{Context, Result};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use parking_lot::Mutex;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use tauri::{AppHandle, Emitter};

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub struct LocalQwenAsr {
    engine: Arc<LocalQwenEngine>,
    operation_id: u64,
    cancelled: Arc<AtomicBool>,
    buffer: Mutex<Vec<u8>>,
    app: AppHandle,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
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

    /// 当前缓冲音频时长（毫秒）。Coordinator 在 transcribe() 调用前读取，
    /// 用来给本地 Qwen ASR 计算动态超时（max(15, ceil(audio_s × 0.6) + 10)）。
    /// 不消费缓冲。
    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(self.buffer.lock().len())
    }

    /// stop 时调用：MLX 整段 batch；C 保持历史流式 token 与尾部静音收尾行为。
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl crate::recorder::AudioConsumer for LocalQwenAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn i16_le_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|c| {
            let v = i16::from_le_bytes([c[0], c[1]]);
            v as f32 / 32768.0
        })
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pcm_duration_ms(byte_len: usize) -> u64 {
    (byte_len as u64 / 2) * 1000 / 16_000
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn token_emission_enabled(cancelled: &AtomicBool) -> bool {
    !cancelled.load(Ordering::Acquire)
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
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
