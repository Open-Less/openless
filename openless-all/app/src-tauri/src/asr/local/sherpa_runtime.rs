//! sherpa-onnx 本地 ASR runtime（M1 骨架）。
//!
//! 设计与 `foundry_runtime.rs` 对齐：runtime 是模型/会话/生命周期的单一持有者，
//! 不感知 `Coordinator` / `Recorder` / UI / Tauri 事件。失败统一通过
//! `anyhow::Error` 上抛，由上层翻译为用户可见文案。
//!
//! M1 阶段：
//! - 全平台编译通过（避免 macOS / Linux CI 红线）
//! - 不引入 `sherpa-onnx` crate（M2 才加 Windows-only 依赖）
//! - `ensure_loaded` / `transcribe_pcm` / `release_now` 全部桩实现
//! - 仅维持 active_model / runtime_ready 这种「状态门面」，便于前端联调

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tokio::sync::Mutex as AsyncMutex;

use crate::asr::local::sherpa::{
    self, SherpaCatalogModel, SherpaFamily, SherpaPreparePhase, SherpaPrepareProgressPayload,
    SherpaRuntimeStatus, PROVIDER_ID,
};

#[cfg(target_os = "windows")]
use sherpa_onnx::{
    OfflineParaformerModelConfig, OfflineQwen3ASRModelConfig, OfflineRecognizer,
    OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, OfflineWhisperModelConfig,
};

/// 模型加载状态。M1 阶段不持有任何 native handle；
/// M2 引入 sherpa-onnx crate 后再补 `recognizer: Arc<OfflineRecognizer>` 之类的字段。
#[derive(Clone)]
struct LoadedModel {
    alias: String,
    #[cfg(target_os = "windows")]
    recognizer: Arc<OfflineRecognizer>,
}

#[derive(Default)]
struct RuntimeState {
    loaded: Option<LoadedModel>,
}

/// 跨会话单例。生命周期由 `AsyncMutex` 串行化，确保 ensure_loaded / release 不会并发。
pub struct SherpaOnnxRuntime {
    lifecycle: AsyncMutex<()>,
    cancel_prepare: AtomicBool,
    state: Mutex<RuntimeState>,
}

impl Default for SherpaOnnxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SherpaOnnxRuntime {
    pub fn new() -> Self {
        Self {
            lifecycle: AsyncMutex::new(()),
            cancel_prepare: AtomicBool::new(false),
            state: Mutex::new(RuntimeState::default()),
        }
    }

    /// 返回当前 runtime 是否真的具备推理能力。M1 永远是 false；
    /// M2 接入 sherpa-onnx 后改为编译期 `#[cfg(target_os = "windows")]` 真值。
    #[allow(dead_code)]
    pub fn is_available(&self) -> bool {
        cfg!(target_os = "windows")
    }

    pub async fn status_snapshot(&self, active_model: &str) -> SherpaRuntimeStatus {
        let loaded_model_id = self
            .state
            .lock()
            .loaded
            .as_ref()
            .map(|loaded| loaded.alias.clone());
        SherpaRuntimeStatus {
            provider_id: PROVIDER_ID.into(),
            available: self.is_available(),
            runtime_ready: loaded_model_id.is_some(),
            active_model: active_model.to_string(),
            loaded_model_id,
            error: None,
        }
    }

    /// M1：返回静态 catalog。M3 接入下载管理后会合并本地缓存状态。
    #[allow(dead_code)]
    pub async fn catalog_snapshot(&self) -> Result<Vec<SherpaCatalogModel>> {
        let mut catalog = sherpa::static_catalog_models();
        for model in &mut catalog {
            let dir = sherpa::model_dir_for_alias(&model.alias)?;
            model.cached = sherpa::required_files_for_alias(&model.alias)
                .map(|files| {
                    files.iter().all(|file| {
                        let path = dir.join(file);
                        sherpa::required_path_is_valid(&model.alias, file, &path)
                    })
                })
                .unwrap_or(false);
            model.downloaded_bytes =
                crate::asr::local::sherpa_download::downloaded_bytes(&model.alias);
            model.file_size_mb = model_dir_size_mb(&dir);
        }
        Ok(catalog)
    }

    pub async fn ensure_loaded(&self, alias: &str) -> Result<String> {
        self.ensure_loaded_with_progress(alias, |_| {}).await
    }

    pub async fn ensure_loaded_with_progress<F>(&self, alias: &str, progress: F) -> Result<String>
    where
        F: Fn(SherpaPrepareProgressPayload) + Send + Sync + 'static,
    {
        let _lifecycle = self.lifecycle.lock().await;
        self.cancel_prepare.store(false, Ordering::SeqCst);
        validate_alias(alias)?;
        if let Some(loaded) = self.cached_loaded_model(alias) {
            progress(SherpaPrepareProgressPayload::new(
                SherpaPreparePhase::Finished,
                alias,
                "Sherpa-Onnx model already loaded",
                Some(100.0),
                None,
            ));
            return Ok(loaded.alias);
        }
        self.check_prepare_cancelled()?;
        let dir = sherpa::model_dir_for_alias(alias)?;
        ensure_required_files(alias, &dir)?;
        progress(SherpaPrepareProgressPayload::new(
            SherpaPreparePhase::Model,
            alias,
            "Sherpa-Onnx local model files",
            Some(100.0),
            None,
        ));
        self.check_prepare_cancelled()?;
        progress(SherpaPrepareProgressPayload::new(
            SherpaPreparePhase::Load,
            alias,
            "Load Sherpa-Onnx model",
            Some(0.0),
            None,
        ));
        let loaded = load_model(alias, &dir).await?;
        self.check_prepare_cancelled()?;
        progress(SherpaPrepareProgressPayload::new(
            SherpaPreparePhase::Load,
            alias,
            "Load Sherpa-Onnx model",
            Some(100.0),
            None,
        ));
        self.state.lock().loaded = Some(loaded.clone());
        progress(SherpaPrepareProgressPayload::new(
            SherpaPreparePhase::Finished,
            alias,
            "Sherpa-Onnx model ready",
            Some(100.0),
            None,
        ));
        Ok(alias.to_string())
    }

    /// M1：永远返回空串，配合 mock pipeline 让用户的话不被「丢失也不被乱写」。
    /// 真实接入见 M2 `OfflineRecognizer::decode`。
    #[allow(dead_code)]
    pub async fn transcribe_pcm(
        &self,
        alias: &str,
        pcm: &[u8],
        language_hint: Option<&str>,
        audio_timeout: std::time::Duration,
    ) -> Result<String> {
        if pcm.is_empty() {
            return Ok(String::new());
        }
        let loaded_alias = self.ensure_loaded(alias).await?;
        let loaded = self
            .state
            .lock()
            .loaded
            .clone()
            .filter(|loaded| loaded.alias == loaded_alias)
            .context("sherpa-onnx model not loaded")?;
        transcribe_loaded_model(
            loaded,
            pcm.to_vec(),
            language_hint.map(str::to_string),
            audio_timeout,
        )
        .await
    }

    pub fn request_cancel_prepare(&self) {
        self.cancel_prepare.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn cancel_prepare_requested_for_tests(&self) -> bool {
        self.cancel_prepare.load(Ordering::SeqCst)
    }

    pub async fn release_now(&self) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        self.state.lock().loaded = None;
        Ok(())
    }

    pub fn model_dir_for_alias(alias: &str) -> Result<PathBuf> {
        sherpa::model_dir_for_alias(alias)
    }

    pub async fn delete_model(&self, alias: &str) -> Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        validate_alias(alias)?;
        {
            let mut state = self.state.lock();
            if state.loaded.as_ref().map(|loaded| loaded.alias.as_str()) == Some(alias) {
                state.loaded = None;
            }
        }
        let dir = sherpa::model_dir_for_alias(alias)?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("remove sherpa-onnx model dir {}", dir.display()))?;
        }
        Ok(())
    }

    fn cached_loaded_model(&self, alias: &str) -> Option<LoadedModel> {
        self.state
            .lock()
            .loaded
            .as_ref()
            .filter(|loaded| loaded.alias == alias)
            .cloned()
    }

    fn check_prepare_cancelled(&self) -> Result<()> {
        if self.cancel_prepare.load(Ordering::SeqCst) {
            anyhow::bail!("sherpa-onnx prepare cancelled");
        }
        Ok(())
    }
}

fn validate_alias(alias: &str) -> Result<()> {
    if sherpa::model_alias_is_known(alias) {
        Ok(())
    } else {
        anyhow::bail!("unknown sherpa-onnx model alias: {alias}");
    }
}

fn ensure_required_files(alias: &str, dir: &Path) -> Result<()> {
    for file in sherpa::required_files_for_alias(alias)? {
        let path = dir.join(file);
        if !sherpa::required_path_is_valid(alias, file, &path) {
            anyhow::bail!(
                "sherpa-onnx model file missing: {}. Place model files under {}",
                file,
                dir.display()
            );
        }
    }
    Ok(())
}

fn model_dir_size_mb(dir: &Path) -> Option<u64> {
    if !dir.exists() {
        return None;
    }
    let mut bytes = 0u64;
    accumulate_dir_size(dir, &mut bytes);
    Some(bytes / 1024 / 1024)
}

fn accumulate_dir_size(dir: &Path, bytes: &mut u64) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => accumulate_dir_size(&path, bytes),
            Ok(file_type) if file_type.is_file() => {
                if let Ok(meta) = entry.metadata() {
                    *bytes += meta.len();
                }
            }
            _ => {}
        }
    }
}

#[cfg(target_os = "windows")]
async fn load_model(alias: &str, dir: &Path) -> Result<LoadedModel> {
    let alias = alias.to_string();
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let recognizer = create_offline_recognizer(&alias, &dir)?;
        Ok(LoadedModel {
            alias,
            recognizer: Arc::new(recognizer),
        })
    })
    .await
    .map_err(|e| anyhow::anyhow!("sherpa-onnx load join failed: {e:#}"))?
}

#[cfg(not(target_os = "windows"))]
async fn load_model(alias: &str, _dir: &Path) -> Result<LoadedModel> {
    Ok(LoadedModel {
        alias: alias.to_string(),
    })
}

#[cfg(target_os = "windows")]
fn create_offline_recognizer(alias: &str, dir: &Path) -> Result<OfflineRecognizer> {
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.num_threads = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 4) as i32)
        .unwrap_or(2);
    config.model_config.provider = Some("cpu".into());
    match model_family(alias)? {
        SherpaFamily::SenseVoice => {
            config.model_config.tokens = Some(path_to_string(&dir.join("tokens.txt"))?);
            config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
                model: Some(path_to_string(&dir.join("model.int8.onnx"))?),
                language: Some("auto".into()),
                use_itn: true,
            };
        }
        SherpaFamily::Paraformer => {
            config.model_config.tokens = Some(path_to_string(&dir.join("tokens.txt"))?);
            config.model_config.paraformer = OfflineParaformerModelConfig {
                model: Some(path_to_string(&dir.join("model.int8.onnx"))?),
            };
        }
        SherpaFamily::Whisper => {
            config.model_config.tokens = Some(path_to_string(&dir.join("tokens.txt"))?);
            config.model_config.whisper = OfflineWhisperModelConfig {
                encoder: Some(path_to_string(&dir.join("encoder.int8.onnx"))?),
                decoder: Some(path_to_string(&dir.join("decoder.int8.onnx"))?),
                language: Some("auto".into()),
                task: Some("transcribe".into()),
                tail_paddings: 0,
                enable_token_timestamps: false,
                enable_segment_timestamps: false,
            };
        }
        SherpaFamily::Qwen3Asr => {
            config.model_config.qwen3_asr = OfflineQwen3ASRModelConfig {
                conv_frontend: Some(path_to_string(&dir.join("conv_frontend.onnx"))?),
                encoder: Some(path_to_string(&dir.join("encoder.int8.onnx"))?),
                decoder: Some(path_to_string(&dir.join("decoder.int8.onnx"))?),
                tokenizer: Some(path_to_string(&dir.join("tokenizer"))?),
                ..Default::default()
            };
            config.model_config.num_threads = 3;
        }
        SherpaFamily::Zipformer => anyhow::bail!("zipformer is not supported by offline batch M2"),
    }
    OfflineRecognizer::create(&config)
        .ok_or_else(|| anyhow::anyhow!("create sherpa-onnx offline recognizer failed"))
}

fn model_family(alias: &str) -> Result<SherpaFamily> {
    sherpa::MODELS
        .iter()
        .find(|model| model.alias == alias)
        .map(|model| model.family)
        .context("unknown sherpa-onnx model family")
}

#[cfg(target_os = "windows")]
fn path_to_string(path: &Path) -> Result<String> {
    Ok(path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))?
        .to_string())
}

#[cfg(target_os = "windows")]
async fn transcribe_loaded_model(
    loaded: LoadedModel,
    pcm: Vec<u8>,
    language_hint: Option<String>,
    audio_timeout: std::time::Duration,
) -> Result<String> {
    tokio::time::timeout(audio_timeout, async move {
        tokio::task::spawn_blocking(move || {
            let samples = pcm_s16le_to_f32(&pcm)?;
            let stream = loaded.recognizer.create_stream();
            if let Some(language) = language_hint.as_deref().filter(|value| !value.is_empty()) {
                if stream.has_option("language") {
                    stream.set_option("language", language);
                }
            }
            stream.accept_waveform(16_000, &samples);
            loaded.recognizer.decode(&stream);
            let result = stream
                .get_result()
                .ok_or_else(|| anyhow::anyhow!("sherpa-onnx returned no result"))?;
            Ok(result.text)
        })
        .await
        .map_err(|e| anyhow::anyhow!("sherpa-onnx transcribe join failed: {e:#}"))?
    })
    .await
    .map_err(|_| anyhow::anyhow!("sherpa-onnx transcribe timeout"))?
}

#[cfg(not(target_os = "windows"))]
async fn transcribe_loaded_model(
    _loaded: LoadedModel,
    _pcm: Vec<u8>,
    _language_hint: Option<String>,
    _audio_timeout: std::time::Duration,
) -> Result<String> {
    Ok(String::new())
}

fn pcm_s16le_to_f32(pcm: &[u8]) -> Result<Vec<f32>> {
    if pcm.len() % 2 != 0 {
        anyhow::bail!("PCM buffer length is not aligned to i16 samples");
    }
    Ok(pcm
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_runtime_reports_skeleton_shape() {
        let runtime = SherpaOnnxRuntime::new();
        let status = runtime.status_snapshot("sense-voice-small-zh").await;

        assert_eq!(status.provider_id, PROVIDER_ID);
        assert_eq!(status.available, cfg!(target_os = "windows"));
        assert!(!status.runtime_ready);
        assert_eq!(status.active_model, "sense-voice-small-zh");
        assert_eq!(status.loaded_model_id, None);
        assert_eq!(status.error, None);
    }

    #[tokio::test]
    async fn ensure_loaded_rejects_unknown_alias() {
        let runtime = SherpaOnnxRuntime::new();
        let result = runtime.ensure_loaded("unknown-sherpa-model").await;
        assert!(result.is_err());
    }

    #[test]
    fn ensure_required_files_reports_missing_model_files() {
        let dir = std::env::temp_dir().join(format!(
            "openless-sherpa-runtime-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let result = ensure_required_files("paraformer-zh", &dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn release_now_clears_loaded_model() {
        let runtime = SherpaOnnxRuntime::new();
        runtime.release_now().await.unwrap();

        let status = runtime.status_snapshot("paraformer-zh").await;
        assert!(!status.runtime_ready);
        assert_eq!(status.loaded_model_id, None);
    }

    #[tokio::test]
    async fn transcribe_pcm_returns_empty_for_empty_input() {
        let runtime = SherpaOnnxRuntime::new();
        let text = runtime
            .transcribe_pcm(
                "sense-voice-small-zh",
                &[],
                Some("zh"),
                std::time::Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert!(text.is_empty());
    }

    #[test]
    fn pcm_s16le_to_f32_converts_samples() {
        let samples = pcm_s16le_to_f32(&[0, 0, 0xff, 0x7f, 0x00, 0x80]).unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0], 0.0);
        assert!(samples[1] > 0.99);
        assert_eq!(samples[2], -1.0);
    }

    #[test]
    fn pcm_s16le_to_f32_rejects_odd_length() {
        assert!(pcm_s16le_to_f32(&[0]).is_err());
    }
}
