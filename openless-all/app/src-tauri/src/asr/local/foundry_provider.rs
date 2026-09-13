#![allow(dead_code, unused_variables)]
//! Foundry Local Whisper 的录音缓冲与分片转写适配。
//! Windows 路径执行原生推理；其他目标保留类型兼容分支。

#[cfg(target_os = "windows")]
use std::fs::{self, OpenOptions};
#[cfg(target_os = "windows")]
use std::io::Write;
#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
#[cfg(target_os = "windows")]
use super::foundry_runtime::FoundryRouteEpoch;
use super::foundry_runtime::{FoundryFallbackNoticeCallback, FoundryPrimaryRecoveryToken};

/// Foundry Local Whisper 属于 Whisper 系模型，原生解码窗口约 30s。每次 SDK
/// 请求保持在窗口内，再由 OpenLess 合并分片文本，避免长听写只返回第一段。
const FOUNDRY_WHISPER_CHUNK_LIMIT_MS: u64 = 30_000;

#[must_use = "primary_recovery must be passed to Foundry release scheduling"]
pub(crate) struct FoundryProviderTranscription {
    pub raw: RawTranscript,
    pub used_cpu_fallback: bool,
    pub primary_recovery: Option<FoundryPrimaryRecoveryToken>,
}

pub struct FoundryLocalWhisperAsr {
    #[cfg(target_os = "windows")]
    runtime: Arc<FoundryLocalRuntime>,
    #[cfg(target_os = "windows")]
    route_epoch: FoundryRouteEpoch,
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
        let route_epoch = runtime.begin_route();
        Self {
            runtime,
            route_epoch,
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

    /// 当前缓冲音频时长（毫秒）。Coordinator 在发起转写前读取，
    /// 用来给 Foundry Local Whisper 计算动态超时。不消费缓冲。
    pub fn buffer_duration_ms(&self) -> u64 {
        pcm_duration_ms(&self.buffer.lock())
    }

    /// 转写当前录音，并在 Foundry 的一次性 GPU→CPU 回退期间同步最小 UI 提示。
    ///
    /// 返回值包含 primary recovery token；所有调用方都必须把它交给 Coordinator 的释放调度，
    /// 避免成功重转录只保留文本、却丢失模型生命周期信息。
    pub(crate) async fn transcribe_with_fallback_notice(
        &self,
        audio_timeout: std::time::Duration,
        notices: FoundryFallbackNoticeCallback,
    ) -> Result<FoundryProviderTranscription> {
        let cancel_generation = self.cancel_generation.load(Ordering::SeqCst);
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(FoundryProviderTranscription {
                raw: RawTranscript {
                    text: String::new(),
                    duration_ms: 0,
                },
                used_cpu_fallback: false,
                primary_recovery: None,
            });
        }

        let result = self.transcribe_inner(&pcm, audio_timeout, notices).await;
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
        notices: FoundryFallbackNoticeCallback,
    ) -> Result<FoundryProviderTranscription> {
        let duration_ms = pcm_duration_ms(pcm);

        #[cfg(not(target_os = "windows"))]
        {
            let _ = pcm;
            let _ = notices;
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
            if chunks.len() > 1 {
                log::info!(
                    "[foundry-asr] splitting {:.2}s audio into {} chunks (limit={}ms)",
                    duration_ms as f64 / 1000.0,
                    chunks.len(),
                    FOUNDRY_WHISPER_CHUNK_LIMIT_MS
                );
            }

            // 所有临时 WAV 必须在单次 runtime 调用结束后才释放：GPU 失败时，runtime 才能让
            // CPU 重试失败分片并继续后续分片，保持整段录音的一致执行路线。
            let wav_files = chunks
                .iter()
                .map(|chunk| TempWavFile::create(chunk))
                .collect::<Result<Vec<_>>>()?;
            let audio_paths = wav_files
                .iter()
                .map(|wav_file| wav_file.path().to_path_buf())
                .collect::<Vec<_>>();
            let outcome = self
                .runtime
                .transcribe_audio_files(
                    self.route_epoch,
                    &self.model_alias,
                    &self.runtime_source,
                    self.language_hint(),
                    &audio_paths,
                    audio_timeout,
                    notices,
                )
                .await
                .with_context(|| {
                    format!(
                        "transcribe Foundry Local Whisper recording ({} chunks) with model {}",
                        chunks.len(),
                        self.model_alias
                    )
                })?;
            let texts = outcome
                .texts
                .iter()
                .map(|text| trim_transcript_text(text))
                .collect::<Vec<_>>();

            Ok(FoundryProviderTranscription {
                raw: RawTranscript {
                    text: crate::asr::whisper::join_transcript_chunks(&texts),
                    duration_ms,
                },
                used_cpu_fallback: outcome.used_cpu_fallback,
                primary_recovery: outcome.primary_recovery,
            })
        }
    }

    pub fn cancel(&self) {
        self.cancel_generation.fetch_add(1, Ordering::SeqCst);
        #[cfg(target_os = "windows")]
        {
            // 旧 provider 不能取消新 route；runtime 同时返回当前 route 的精确 CPU lease，
            // 避免跨会话取消共享的 prepare 标志或误卸载新录音的临时模型。
            if let Some(cancelled_lease) =
                self.runtime.request_cancel_transcription(self.route_epoch)
            {
                let runtime = Arc::clone(&self.runtime);
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = runtime
                        .release_temporary_cpu_fallback(cancelled_lease)
                        .await
                    {
                        log::warn!(
                            "[foundry-asr] cancel cleanup for temporary CPU fallback failed: {error:#}"
                        );
                    }
                });
            }
        }
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
    fn foundry_provider_binds_a_new_route_when_the_recording_is_created() {
        let runtime = std::sync::Arc::new(super::FoundryLocalRuntime::new());
        let first = super::FoundryLocalWhisperAsr::new(
            std::sync::Arc::clone(&runtime),
            "whisper-small".into(),
            "auto".into(),
            None,
        );
        let second = super::FoundryLocalWhisperAsr::new(
            std::sync::Arc::clone(&runtime),
            "whisper-medium".into(),
            "auto".into(),
            None,
        );

        assert_ne!(first.route_epoch, second.route_epoch);
        assert_eq!(runtime.route_epoch_snapshot(), second.route_epoch);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn foundry_provider_route_is_not_rebased_by_a_later_setting_change() {
        let runtime = std::sync::Arc::new(super::FoundryLocalRuntime::new());
        let provider = super::FoundryLocalWhisperAsr::new(
            std::sync::Arc::clone(&runtime),
            "whisper-small".into(),
            "auto".into(),
            None,
        );
        let recording_route = provider.route_epoch;

        runtime.invalidate_route();

        assert_ne!(runtime.route_epoch_snapshot(), recording_route);
        assert_eq!(provider.route_epoch, recording_route);
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
