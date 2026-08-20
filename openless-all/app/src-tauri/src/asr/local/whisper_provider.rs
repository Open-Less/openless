//! macOS 本地 Whisper Large-v3 Turbo：录音结束后整段 batch 解码。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::asr::RawTranscript;

pub const MODEL_ID: &str = "whisper-large-v3-turbo";
const QUANTIZED_MODEL_FILE: &str = "ggml-large-v3-turbo-q5_0.bin";

pub fn model_path_for_model(model_id: &str) -> Result<PathBuf> {
    let id = crate::asr::local::ModelId::from_str(model_id)
        .filter(|id| id.is_whisper())
        .ok_or_else(|| anyhow::anyhow!("未知的本地 Whisper 模型: {model_id}"))?;
    let dir = crate::asr::local::models::model_dir(id)?;
    let file_name = id
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("本地 Whisper 模型没有文件名: {model_id}"))?;
    let path = model_path_in_dir(id, &dir, file_name);
    Ok(path)
}

fn model_path_in_dir(id: crate::asr::local::ModelId, dir: &Path, file_name: &str) -> PathBuf {
    let path = dir.join(file_name);
    if id == crate::asr::local::ModelId::WhisperLargeV3Turbo && !path.exists() {
        let quantized = dir.join(QUANTIZED_MODEL_FILE);
        if quantized.exists() {
            return quantized;
        }
    }
    path
}

pub fn model_ready_for_model(model_id: &str) -> bool {
    model_path_for_model(model_id)
        .map(|path| path.is_file())
        .unwrap_or(false)
}

pub struct LocalWhisperCache {
    inner: Mutex<Option<CachedEngine>>,
}

struct CachedEngine {
    model_id: String,
    engine: Arc<WhisperEngine>,
    last_used: Instant,
}

impl Default for LocalWhisperCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalWhisperCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub fn get_or_load(&self, model_id: &str, path: &Path) -> Result<Arc<WhisperEngine>> {
        {
            let mut slot = self.inner.lock();
            if let Some(cached) = slot.as_mut() {
                if cached.model_id == model_id {
                    cached.last_used = Instant::now();
                    return Ok(Arc::clone(&cached.engine));
                }
                slot.take();
            }
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Whisper 模型路径不是有效的 UTF-8"))?;
        log::info!("[local-whisper] loading model from {}", path.display());
        let context =
            WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
                .map_err(|error| anyhow::anyhow!("加载 Whisper 模型失败: {error}"))?;
        let engine = Arc::new(WhisperEngine {
            context: Mutex::new(context),
        });
        self.inner.lock().replace(CachedEngine {
            model_id: model_id.to_string(),
            engine: Arc::clone(&engine),
            last_used: Instant::now(),
        });
        Ok(engine)
    }

    pub fn touch(&self) {
        if let Some(cached) = self.inner.lock().as_mut() {
            cached.last_used = Instant::now();
        }
    }

    pub fn release_if_idle(&self, threshold: Duration) -> bool {
        let mut slot = self.inner.lock();
        match slot.as_ref() {
            Some(cached) if cached.last_used.elapsed() >= threshold => {
                slot.take();
                true
            }
            _ => false,
        }
    }

    pub fn release_now(&self) {
        self.inner.lock().take();
    }

    pub fn loaded_model_id(&self) -> Option<String> {
        self.inner
            .lock()
            .as_ref()
            .map(|cached| cached.model_id.clone())
    }
}

#[cfg(test)]
mod model_path_tests {
    use super::model_path_in_dir;
    use crate::asr::local::ModelId;

    #[test]
    fn turbo_path_falls_back_to_q5_but_q5_path_does_not_fall_back() {
        let dir = std::env::temp_dir().join(format!(
            "openless-whisper-path-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let turbo = dir.join("ggml-large-v3-turbo.bin");
        let q5 = dir.join("ggml-large-v3-turbo-q5_0.bin");
        std::fs::write(&turbo, b"turbo").unwrap();

        assert_eq!(
            model_path_in_dir(
                ModelId::WhisperLargeV3TurboQ5,
                &dir,
                "ggml-large-v3-turbo-q5_0.bin"
            ),
            q5
        );

        std::fs::remove_file(&turbo).unwrap();
        std::fs::write(&q5, b"q5").unwrap();

        assert_eq!(
            model_path_in_dir(
                ModelId::WhisperLargeV3Turbo,
                &dir,
                "ggml-large-v3-turbo.bin"
            ),
            q5
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub struct WhisperEngine {
    context: Mutex<WhisperContext>,
}

impl WhisperEngine {
    pub(crate) fn transcribe(&self, audio: &[f32], language: &str) -> Result<String> {
        let mut context = self.context.lock();
        let mut state = context
            .create_state()
            .map_err(|error| anyhow::anyhow!("创建 Whisper 状态失败: {error}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        match language {
            "auto" | "" => params.set_language(None),
            language => params.set_language(Some(language)),
        }
        if language == "zh" {
            params.set_initial_prompt("以下是普通话的句子。");
        }
        params.set_translate(false);
        params.set_no_timestamps(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        state
            .full(params, audio)
            .map_err(|error| anyhow::anyhow!("Whisper batch 解码失败: {error}"))?;

        let count = state
            .full_n_segments()
            .map_err(|error| anyhow::anyhow!("读取 Whisper 分段数失败: {error}"))?;
        let mut text = String::new();
        for index in 0..count {
            text.push_str(
                &state
                    .full_get_segment_text(index)
                    .map_err(|error| anyhow::anyhow!("读取 Whisper 分段失败: {error}"))?,
            );
        }
        Ok(text.trim().to_string())
    }
}

pub struct LocalWhisperAsr {
    engine: Arc<WhisperEngine>,
    language: String,
    buffer: Mutex<Vec<u8>>,
}

impl LocalWhisperAsr {
    pub fn new(engine: Arc<WhisperEngine>, language: String) -> Self {
        Self {
            engine,
            language,
            buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn buffer_duration_ms(&self) -> u64 {
        (self.buffer.lock().len() as u64 / 2) * 1000 / 16_000
    }

    pub async fn transcribe(self: Arc<Self>) -> Result<RawTranscript> {
        let pcm = std::mem::take(&mut *self.buffer.lock());
        let duration_ms = (pcm.len() as u64 / 2) * 1000 / 16_000;
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }
        let audio = pcm_to_f32(&pcm);
        let engine = Arc::clone(&self.engine);
        let language = self.language.clone();
        // `spawn_blocking` 无法被 tokio::time::timeout 中止；调用方取消或超时后只会
        // 放弃等待结果，native Whisper 解码仍可能继续运行。Coordinator 会先驱逐
        // cache，再让后续会话加载新的 WhisperContext，避免复用仍在解码的旧锁。
        let text =
            tauri::async_runtime::spawn_blocking(move || engine.transcribe(&audio, &language))
                .await
                .context("Whisper batch 解码任务异常")??;
        Ok(RawTranscript { text, duration_ms })
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for LocalWhisperAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

fn pcm_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::pcm_to_f32;

    #[test]
    fn converts_recorder_pcm_to_whisper_samples() {
        let bytes = [0x00, 0x80, 0x00, 0x40, 0xff, 0x7f];
        let samples = pcm_to_f32(&bytes);
        assert_eq!(samples.len(), 3);
        assert!((samples[0] + 1.0).abs() < f32::EPSILON);
        assert!((samples[1] - 0.5).abs() < 0.0001);
        assert!(samples[2] > 0.99);
    }
}
