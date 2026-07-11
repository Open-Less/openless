#![allow(dead_code, unused_variables)] // Task 6 接入 coordinator 后这些路径会变成运行时路径。

#[cfg(target_os = "windows")]
use std::fs::{self, OpenOptions};
#[cfg(target_os = "windows")]
use std::io::Write;
#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::sync::Arc;

#[cfg(target_os = "windows")]
use anyhow::Context;
use anyhow::Result;
use parking_lot::Mutex;
#[cfg(target_os = "windows")]
use uuid::Uuid;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

#[cfg(target_os = "windows")]
use super::foundry_runtime::FoundryLocalRuntime;

/// Foundry Local Whisper 属于 Whisper 系模型，原生解码窗口约 30s。每次 SDK
/// 请求保持在窗口内，再由 OpenLess 合并分片文本，避免长听写只返回第一段。
const FOUNDRY_WHISPER_CHUNK_LIMIT_MS: u64 = 30_000;

pub struct FoundryLocalWhisperAsr {
    #[cfg(target_os = "windows")]
    runtime: Arc<FoundryLocalRuntime>,
    model_alias: String,
    runtime_source: String,
    language_hint: Option<String>,
    buffer: Mutex<Vec<u8>>,
    cancel_generation: AtomicU64,
}

impl FoundryLocalWhisperAsr {
    #[cfg(target_os = "windows")]
    pub fn new(
        runtime: Arc<FoundryLocalRuntime>,
        model_alias: String,
        runtime_source: String,
        language_hint: Option<String>,
    ) -> Self {
        Self {
            runtime,
            model_alias,
            runtime_source,
            language_hint: normalize_language_hint(language_hint),
            buffer: Mutex::new(Vec::new()),
            cancel_generation: AtomicU64::new(0),
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn new(model_alias: String, language_hint: Option<String>) -> Self {
        Self {
            model_alias,
            runtime_source: "auto".into(),
            language_hint: normalize_language_hint(language_hint),
            buffer: Mutex::new(Vec::new()),
            cancel_generation: AtomicU64::new(0),
        }
    }

    pub fn model_alias(&self) -> &str {
        &self.model_alias
    }

    pub fn language_hint(&self) -> Option<&str> {
        self.language_hint.as_deref()
    }

    /// 当前缓冲音频时长（毫秒）。Coordinator 在 transcribe() 调用前读取，
    /// 用来给 Foundry Local Whisper 计算动态超时。不消费缓冲。
    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(&self.buffer.lock())
    }

    pub async fn transcribe(&self, audio_timeout: std::time::Duration) -> Result<RawTranscript> {
        let cancel_generation = self.cancel_generation.load(Ordering::SeqCst);
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm, audio_timeout).await;
        if self.cancel_generation.load(Ordering::SeqCst) != cancel_generation {
            anyhow::bail!("Foundry Local Whisper transcription cancelled");
        }
        if foundry_transcribe_attempt_consumes_buffer(&result) {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(
        &self,
        pcm: &[u8],
        audio_timeout: std::time::Duration,
    ) -> Result<RawTranscript> {
        let duration_ms = pcm_duration_ms(pcm);

        #[cfg(not(target_os = "windows"))]
        {
            let _ = pcm;
            anyhow::bail!(
                "Foundry Local Whisper is only available on Windows: {}",
                self.model_alias
            );
        }

        #[cfg(target_os = "windows")]
        {
            let chunks = crate::asr::whisper::split_pcm_by_duration(
                pcm,
                Some(FOUNDRY_WHISPER_CHUNK_LIMIT_MS),
            );
            let deadline = std::time::Instant::now()
                .checked_add(audio_timeout)
                .ok_or_else(|| anyhow::anyhow!("Foundry Local Whisper timeout is too large"))?;
            if chunks.len() > 1 {
                log::info!(
                    "[foundry-asr] splitting {:.2}s audio into {} chunks (limit={}ms)",
                    duration_ms as f64 / 1000.0,
                    chunks.len(),
                    FOUNDRY_WHISPER_CHUNK_LIMIT_MS
                );
            }

            let mut texts = Vec::with_capacity(chunks.len());
            for (index, chunk) in chunks.iter().enumerate() {
                let wav_file = TempWavFile::create(chunk)?;
                let remaining_timeout =
                    remaining_transcribe_timeout(deadline, std::time::Instant::now())
                        .with_context(|| {
                            format!(
                                "Foundry Local Whisper total timeout exhausted before chunk {}/{}",
                                index + 1,
                                chunks.len()
                            )
                        })?;
                let text = self
                    .runtime
                    .transcribe_audio_file(
                        &self.model_alias,
                        &self.runtime_source,
                        self.language_hint(),
                        wav_file.path(),
                        remaining_timeout,
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "transcribe Foundry Local Whisper chunk {}/{} with model {}",
                            index + 1,
                            chunks.len(),
                            self.model_alias
                        )
                    })?;
                texts.push(trim_transcript_text(&text));
            }

            Ok(RawTranscript {
                text: crate::asr::whisper::join_transcript_chunks(&texts),
                duration_ms,
            })
        }
    }

    pub fn cancel(&self) {
        self.cancel_generation.fetch_add(1, Ordering::SeqCst);
        #[cfg(target_os = "windows")]
        self.runtime.request_cancel_prepare();
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for FoundryLocalWhisperAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    crate::asr::pcm::pcm_duration_ms(pcm)
}

fn remaining_transcribe_timeout(
    deadline: std::time::Instant,
    now: std::time::Instant,
) -> Result<std::time::Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| anyhow::anyhow!("Foundry Local Whisper total timeout exhausted"))
}

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let samples: Vec<i16> = pcm
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    encode_wav_16k_mono(&samples)
}

#[cfg(target_os = "windows")]
struct TempWavFile {
    path: PathBuf,
}

#[cfg(target_os = "windows")]
impl TempWavFile {
    fn create(pcm: &[u8]) -> Result<Self> {
        let dir = foundry_temp_dir();
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let path = dir.join(format!("foundry-whisper-{}.wav", Uuid::new_v4()));
        let wav = pcm_to_wav(pcm);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("create {}", path.display()))?;

        if let Err(err) = file.write_all(&wav) {
            drop(file);
            remove_partial_temp_wav(&path);
            return Err(err).with_context(|| format!("write {}", path.display()));
        }
        if let Err(err) = file.sync_all() {
            drop(file);
            remove_partial_temp_wav(&path);
            return Err(err).with_context(|| format!("sync {}", path.display()));
        }

        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(target_os = "windows")]
impl Drop for TempWavFile {
    fn drop(&mut self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                log::warn!(
                    "[foundry-asr] 清理临时 WAV 失败 {}: {err}",
                    self.path.display()
                );
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn remove_partial_temp_wav(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            log::warn!(
                "[foundry-asr] 清理未完成的临时 WAV 失败 {}: {err}",
                path.display()
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn foundry_temp_dir() -> PathBuf {
    std::env::temp_dir()
        .join("OpenLess")
        .join("foundry-local-asr")
}

fn normalize_language_hint(language_hint: Option<String>) -> Option<String> {
    language_hint
        .map(|hint| hint.trim().to_string())
        .filter(|hint| !hint.is_empty())
}

fn trim_transcript_text(text: &str) -> String {
    text.trim().to_string()
}

fn foundry_transcribe_attempt_consumes_buffer<T>(result: &Result<T>) -> bool {
    let _ = result;
    true
}

#[cfg(test)]
mod tests {
    use crate::recorder::AudioConsumer;

    #[cfg(target_os = "windows")]
    fn test_provider() -> (
        super::FoundryLocalWhisperAsr,
        std::sync::Arc<super::FoundryLocalRuntime>,
    ) {
        use std::sync::Arc;

        let runtime = Arc::new(super::FoundryLocalRuntime::new());
        (
            super::FoundryLocalWhisperAsr::new(
                Arc::clone(&runtime),
                "whisper-small".into(),
                "auto".into(),
                Some(" zh ".into()),
            ),
            runtime,
        )
    }

    #[cfg(not(target_os = "windows"))]
    fn test_provider() -> super::FoundryLocalWhisperAsr {
        super::FoundryLocalWhisperAsr::new("whisper-small".into(), Some(" zh ".into()))
    }

    #[test]
    fn foundry_provider_duration_uses_16k_i16_pcm() {
        let pcm = vec![0u8; 32_000];

        assert_eq!(super::pcm_duration_ms(&pcm), 1000);
    }

    #[test]
    fn foundry_provider_wav_ignores_odd_trailing_byte() {
        let pcm = [0x01, 0x00, 0xff, 0x7f, 0xee];
        let wav = super::pcm_to_wav(&pcm);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 4);
        assert_eq!(&wav[44..], &[0x01, 0x00, 0xff, 0x7f]);
    }

    #[test]
    fn foundry_provider_splits_long_pcm_at_whisper_window() {
        let pcm = vec![0u8; 32_000 * 65];
        let chunks = crate::asr::whisper::split_pcm_by_duration(
            &pcm,
            Some(super::FOUNDRY_WHISPER_CHUNK_LIMIT_MS),
        );

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 32_000 * 30);
        assert_eq!(chunks[1].len(), 32_000 * 30);
        assert_eq!(chunks[2].len(), 32_000 * 5);
    }

    #[test]
    fn foundry_chunk_timeout_uses_remaining_total_budget() {
        let started = std::time::Instant::now();
        let deadline = started + std::time::Duration::from_secs(85);

        assert_eq!(
            super::remaining_transcribe_timeout(
                deadline,
                started + std::time::Duration::from_secs(30),
            )
            .unwrap(),
            std::time::Duration::from_secs(55)
        );
        assert!(super::remaining_transcribe_timeout(deadline, deadline).is_err());
    }

    #[test]
    fn foundry_provider_reports_buffer_duration_without_consuming() {
        #[cfg(target_os = "windows")]
        let (provider, _) = test_provider();
        #[cfg(not(target_os = "windows"))]
        let provider = test_provider();

        provider.consume_pcm_chunk(&vec![0u8; 32_000]);

        assert_eq!(provider.buffer_duration_ms(), 1000);
        assert_eq!(provider.buffer.lock().len(), 32_000);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn foundry_provider_temp_wav_drop_removes_file() {
        let pcm = [0x01, 0x00, 0xff, 0x7f];
        let path = {
            let temp = super::TempWavFile::create(&pcm).unwrap();
            let path = temp.path().to_path_buf();

            assert!(path.exists());

            path
        };

        assert!(!path.exists());
    }

    #[test]
    fn foundry_provider_normalizes_language_hint_and_text() {
        assert_eq!(
            super::normalize_language_hint(Some(" zh ".into())),
            Some("zh".into())
        );
        assert_eq!(super::normalize_language_hint(Some(" ".into())), None);
        assert_eq!(super::trim_transcript_text("  hello\r\n"), "hello");
    }

    #[test]
    fn foundry_transcribe_attempt_consumes_buffer_even_on_error() {
        let result: anyhow::Result<()> = Err(anyhow::anyhow!("transient runtime error"));

        assert!(super::foundry_transcribe_attempt_consumes_buffer(&result));
    }

    #[test]
    fn foundry_provider_cancel_clears_buffer() {
        #[cfg(target_os = "windows")]
        let (provider, _) = test_provider();
        #[cfg(not(target_os = "windows"))]
        let provider = test_provider();

        provider.consume_pcm_chunk(&[1, 0, 2, 0]);
        provider.cancel();

        assert!(provider.buffer.lock().is_empty());
        assert_eq!(
            provider
                .cancel_generation
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(provider.model_alias(), "whisper-small");
        assert_eq!(provider.language_hint(), Some("zh"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn foundry_provider_cancel_requests_runtime_prepare_cancel() {
        let (provider, runtime) = test_provider();

        provider.cancel();

        assert!(runtime.cancel_prepare_requested_for_tests());
    }
}
