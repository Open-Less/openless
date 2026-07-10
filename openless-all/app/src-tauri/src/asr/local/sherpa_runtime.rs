#![allow(dead_code, unused_imports, unused_variables)]
//! sherpa-onnx 本地 ASR runtime（Windows offline batch + online streaming）。
//!
//! 设计与 `foundry_runtime.rs` 对齐：runtime 是模型/会话/生命周期的单一持有者，
//! 不感知 `Coordinator` / `Recorder` / UI / Tauri 事件。失败统一通过
//! `anyhow::Error` 上抛，由上层翻译为用户可见文案。
//!
//! 当前 Windows 路径接入 `sherpa-onnx` 的 `OfflineRecognizer` 和
//! `OnlineRecognizer`，支持模型加载、缓存、整段 PCM 转写、online 分块解码和释放。
//! 非 Windows 仍只保留可编译的状态门面。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tokio::sync::Mutex as AsyncMutex;

use crate::asr::local::sherpa::{
    self, SherpaCatalogModel, SherpaFamily, SherpaMode, SherpaPreparePhase,
    SherpaPrepareProgressPayload, SherpaRuntimeStatus, PROVIDER_ID,
};

#[cfg(target_os = "windows")]
use sherpa_onnx::{
    OfflineParaformerModelConfig, OfflineQwen3ASRModelConfig, OfflineRecognizer,
    OfflineRecognizerConfig, OfflineSenseVoiceModelConfig, OfflineWhisperModelConfig,
    OnlineRecognizer, OnlineRecognizerConfig,
};

/// Offline 模型加载状态。Windows 持有 native `OfflineRecognizer`；其他平台仅保留 alias
/// 以维持跨平台编译与状态查询形状。
#[derive(Clone)]
struct LoadedOfflineModel {
    alias: String,
    #[cfg(target_os = "windows")]
    recognizer: Arc<OfflineRecognizer>,
}

/// Online 模型加载状态。每次听写会话会从 recognizer 创建独立 `OnlineStream`。
#[derive(Clone)]
struct LoadedOnlineModel {
    alias: String,
    #[cfg(target_os = "windows")]
    recognizer: Arc<OnlineRecognizer>,
}

#[derive(Default)]
struct RuntimeState {
    offline_loaded: Option<LoadedOfflineModel>,
    online_loaded: Option<LoadedOnlineModel>,
    diagnostics: RuntimeDiagnostics,
}

#[derive(Clone, Default)]
struct RuntimeDiagnostics {
    last_prepare_ms: Option<u64>,
    last_transcribe_ms: Option<u64>,
    last_audio_ms: Option<u64>,
    last_error: Option<String>,
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

    /// 返回当前 runtime 是否真的具备推理能力。当前仅 Windows 接入
    /// `sherpa-onnx` offline recognizer。
    #[allow(dead_code)]
    pub fn is_available(&self) -> bool {
        cfg!(target_os = "windows")
    }

    pub async fn status_snapshot(&self, active_model: &str) -> SherpaRuntimeStatus {
        let state = self.state.lock();
        let loaded_model_id = active_loaded_model_id(&state, active_model);
        let diagnostics = state.diagnostics.clone();
        SherpaRuntimeStatus {
            provider_id: PROVIDER_ID.into(),
            available: self.is_available(),
            runtime_ready: loaded_model_id.is_some(),
            active_model: active_model.to_string(),
            loaded_model_id,
            error: diagnostics.last_error.clone(),
            last_prepare_ms: diagnostics.last_prepare_ms,
            last_transcribe_ms: diagnostics.last_transcribe_ms,
            last_audio_ms: diagnostics.last_audio_ms,
            last_error: diagnostics.last_error,
        }
    }

    /// 返回静态 catalog，并合并本地缓存状态与已下载字节数。
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
        let started = Instant::now();
        let result = self
            .ensure_loaded_with_progress_inner(alias, progress)
            .await;
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        match &result {
            Ok(loaded_alias) => {
                log::info!(
                    "[sherpa-asr] prepare finished model={} elapsed_ms={}",
                    loaded_alias,
                    elapsed_ms
                );
                self.record_prepare_result(elapsed_ms, None);
            }
            Err(error) => {
                let message = format!("{error:#}");
                log::warn!(
                    "[sherpa-asr] prepare failed model={} elapsed_ms={} error={}",
                    alias,
                    elapsed_ms,
                    message
                );
                self.record_prepare_result(elapsed_ms, Some(message));
            }
        }
        result
    }

    async fn ensure_loaded_with_progress_inner<F>(&self, alias: &str, progress: F) -> Result<String>
    where
        F: Fn(SherpaPrepareProgressPayload) + Send + Sync + 'static,
    {
        let _lifecycle = self.lifecycle.lock().await;
        self.cancel_prepare.store(false, Ordering::SeqCst);
        validate_alias(alias)?;
        if let Some(loaded_alias) = self.cached_loaded_alias(alias) {
            progress(SherpaPrepareProgressPayload::new(
                SherpaPreparePhase::Finished,
                alias,
                "Sherpa-Onnx model already loaded",
                Some(100.0),
                None,
            ));
            return Ok(loaded_alias);
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
        match loaded {
            LoadedModel::Offline(loaded) => self.state.lock().offline_loaded = Some(loaded),
            LoadedModel::Online(loaded) => self.state.lock().online_loaded = Some(loaded),
        }
        progress(SherpaPrepareProgressPayload::new(
            SherpaPreparePhase::Finished,
            alias,
            "Sherpa-Onnx model ready",
            Some(100.0),
            None,
        ));
        Ok(alias.to_string())
    }

    /// Windows 下用已加载的 `OfflineRecognizer` 做整段 PCM batch 转写；非 Windows
    /// 保持空实现，避免把 sherpa provider 暴露为可用推理能力。
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
        if sherpa::mode_for_alias(alias)? != SherpaMode::Offline {
            anyhow::bail!("sherpa-onnx model {alias} is online-only; use streaming API");
        }
        let audio_ms = pcm_duration_ms(pcm);
        let loaded_alias = self.ensure_loaded(alias).await?;
        let loaded = self
            .state
            .lock()
            .offline_loaded
            .clone()
            .filter(|loaded| loaded.alias == loaded_alias)
            .context("sherpa-onnx offline model not loaded")?;
        let started = Instant::now();
        let result = transcribe_loaded_model(
            loaded,
            pcm.to_vec(),
            language_hint.map(str::to_string),
            audio_timeout,
        )
        .await;
        let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        match &result {
            Ok(text) => {
                log::info!(
                    "[sherpa-asr] transcribe finished model={} audio_ms={} elapsed_ms={} text_chars={}",
                    alias,
                    audio_ms,
                    elapsed_ms,
                    text.chars().count()
                );
                self.record_transcribe_result(audio_ms, elapsed_ms, None);
            }
            Err(error) => {
                let message = format!("{error:#}");
                log::warn!(
                    "[sherpa-asr] transcribe failed model={} audio_ms={} elapsed_ms={} error={}",
                    alias,
                    audio_ms,
                    elapsed_ms,
                    message
                );
                self.record_transcribe_result(audio_ms, elapsed_ms, Some(message));
            }
        }
        result
    }

    /// 创建独立 online 解码 session。调用者负责按 Recorder PCM chunk 喂入，
    /// 并在停止录音时调用 `finish()` 刷出 final text。
    pub async fn create_online_session(&self, alias: &str) -> Result<SherpaOnlineSession> {
        if sherpa::mode_for_alias(alias)? != SherpaMode::Online {
            anyhow::bail!("sherpa-onnx model {alias} is not an online streaming model");
        }
        let loaded_alias = self.ensure_loaded(alias).await?;
        let loaded = self
            .state
            .lock()
            .online_loaded
            .clone()
            .filter(|loaded| loaded.alias == loaded_alias)
            .context("sherpa-onnx online model not loaded")?;
        create_online_session_from_loaded(loaded)
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
        let mut state = self.state.lock();
        state.offline_loaded = None;
        state.online_loaded = None;
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
            if state
                .offline_loaded
                .as_ref()
                .map(|loaded| loaded.alias.as_str())
                == Some(alias)
            {
                state.offline_loaded = None;
            }
            if state
                .online_loaded
                .as_ref()
                .map(|loaded| loaded.alias.as_str())
                == Some(alias)
            {
                state.online_loaded = None;
            }
        }
        let dir = sherpa::model_dir_for_alias(alias)?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("remove sherpa-onnx model dir {}", dir.display()))?;
        }
        Ok(())
    }

    fn cached_loaded_alias(&self, alias: &str) -> Option<String> {
        let state = self.state.lock();
        if state
            .offline_loaded
            .as_ref()
            .filter(|loaded| loaded.alias == alias)
            .is_some()
        {
            return Some(alias.to_string());
        }
        if state
            .online_loaded
            .as_ref()
            .filter(|loaded| loaded.alias == alias)
            .is_some()
        {
            return Some(alias.to_string());
        }
        None
    }

    fn check_prepare_cancelled(&self) -> Result<()> {
        if self.cancel_prepare.load(Ordering::SeqCst) {
            anyhow::bail!("sherpa-onnx prepare cancelled");
        }
        Ok(())
    }

    fn record_prepare_result(&self, elapsed_ms: u64, error: Option<String>) {
        let mut state = self.state.lock();
        state.diagnostics.last_prepare_ms = Some(elapsed_ms);
        state.diagnostics.last_error = error;
    }

    fn record_transcribe_result(&self, audio_ms: u64, elapsed_ms: u64, error: Option<String>) {
        let mut state = self.state.lock();
        state.diagnostics.last_audio_ms = Some(audio_ms);
        state.diagnostics.last_transcribe_ms = Some(elapsed_ms);
        state.diagnostics.last_error = error;
    }

    pub fn record_streaming_result(
        &self,
        alias: &str,
        audio_ms: u64,
        elapsed_ms: u64,
        error: Option<String>,
    ) {
        match &error {
            Some(message) => log::warn!(
                "[sherpa-asr] streaming finished model={} audio_ms={} elapsed_ms={} error={}",
                alias,
                audio_ms,
                elapsed_ms,
                message
            ),
            None => log::info!(
                "[sherpa-asr] streaming finished model={} audio_ms={} elapsed_ms={}",
                alias,
                audio_ms,
                elapsed_ms
            ),
        }
        self.record_transcribe_result(audio_ms, elapsed_ms, error);
    }
}

fn active_loaded_model_id(state: &RuntimeState, active_model: &str) -> Option<String> {
    state
        .offline_loaded
        .as_ref()
        .filter(|loaded| loaded.alias == active_model)
        .map(|loaded| loaded.alias.clone())
        .or_else(|| {
            state
                .online_loaded
                .as_ref()
                .filter(|loaded| loaded.alias == active_model)
                .map(|loaded| loaded.alias.clone())
        })
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

fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    crate::asr::pcm::pcm_duration_ms(pcm)
}

enum LoadedModel {
    Offline(LoadedOfflineModel),
    Online(LoadedOnlineModel),
}

#[cfg(target_os = "windows")]
async fn load_model(alias: &str, dir: &Path) -> Result<LoadedModel> {
    let alias = alias.to_string();
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || match sherpa::mode_for_alias(&alias)? {
        SherpaMode::Offline => {
            let recognizer = create_offline_recognizer(&alias, &dir)?;
            Ok(LoadedModel::Offline(LoadedOfflineModel {
                alias,
                recognizer: Arc::new(recognizer),
            }))
        }
        SherpaMode::Online => {
            let recognizer = create_online_recognizer(&alias, &dir)?;
            Ok(LoadedModel::Online(LoadedOnlineModel {
                alias,
                recognizer: Arc::new(recognizer),
            }))
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("sherpa-onnx load join failed: {e:#}"))?
}

#[cfg(not(target_os = "windows"))]
async fn load_model(alias: &str, _dir: &Path) -> Result<LoadedModel> {
    match sherpa::mode_for_alias(alias)? {
        SherpaMode::Offline => Ok(LoadedModel::Offline(LoadedOfflineModel {
            alias: alias.to_string(),
        })),
        SherpaMode::Online => Ok(LoadedModel::Online(LoadedOnlineModel {
            alias: alias.to_string(),
        })),
    }
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

#[cfg(target_os = "windows")]
fn create_online_recognizer(alias: &str, dir: &Path) -> Result<OnlineRecognizer> {
    let mut config = OnlineRecognizerConfig::default();
    config.model_config.num_threads = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 4) as i32)
        .unwrap_or(2);
    config.model_config.provider = Some("cpu".into());
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = 2.4;
    config.rule2_min_trailing_silence = 1.2;
    config.rule3_min_utterance_length = 20.0;
    config.decoding_method = Some("greedy_search".into());
    match model_family(alias)? {
        SherpaFamily::Zipformer => {
            config.model_config.tokens = Some(path_to_string(&dir.join("tokens.txt"))?);
            config.model_config.transducer.encoder = Some(path_to_string(
                &dir.join("encoder-epoch-99-avg-1.int8.onnx"),
            )?);
            config.model_config.transducer.decoder =
                Some(path_to_string(&dir.join("decoder-epoch-99-avg-1.onnx"))?);
            config.model_config.transducer.joiner = Some(path_to_string(
                &dir.join("joiner-epoch-99-avg-1.int8.onnx"),
            )?);
        }
        family => anyhow::bail!("sherpa-onnx family {family:?} is not supported by online ASR"),
    }
    OnlineRecognizer::create(&config)
        .ok_or_else(|| anyhow::anyhow!("create sherpa-onnx online recognizer failed"))
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
    loaded: LoadedOfflineModel,
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
    _loaded: LoadedOfflineModel,
    _pcm: Vec<u8>,
    _language_hint: Option<String>,
    _audio_timeout: std::time::Duration,
) -> Result<String> {
    Ok(String::new())
}

pub struct SherpaOnlineSession {
    alias: String,
    #[cfg(target_os = "windows")]
    recognizer: Arc<OnlineRecognizer>,
    #[cfg(target_os = "windows")]
    stream: sherpa_onnx::OnlineStream,
    committed_text: String,
    last_partial_text: String,
    last_emitted_text: String,
}

impl SherpaOnlineSession {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn accept_pcm_chunk<F>(&mut self, pcm: &[u8], on_token: &F) -> Result<()>
    where
        F: Fn(&str),
    {
        if pcm.is_empty() {
            return Ok(());
        }
        let samples = pcm_s16le_to_f32(pcm)?;
        self.accept_samples(&samples, on_token)
    }

    pub fn finish<F>(&mut self, on_token: &F) -> Result<String>
    where
        F: Fn(&str),
    {
        self.finish_inner(on_token)
    }

    #[cfg(target_os = "windows")]
    fn accept_samples<F>(&mut self, samples: &[f32], on_token: &F) -> Result<()>
    where
        F: Fn(&str),
    {
        self.stream.accept_waveform(16_000, samples);
        self.drain_ready(on_token);
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    fn accept_samples<F>(&mut self, _samples: &[f32], _on_token: &F) -> Result<()>
    where
        F: Fn(&str),
    {
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn finish_inner<F>(&mut self, on_token: &F) -> Result<String>
    where
        F: Fn(&str),
    {
        self.stream.input_finished();
        self.drain_ready(on_token);
        self.capture_result(on_token, false);
        Ok(self.final_text())
    }

    #[cfg(not(target_os = "windows"))]
    fn finish_inner<F>(&mut self, _on_token: &F) -> Result<String>
    where
        F: Fn(&str),
    {
        Ok(self.final_text())
    }

    #[cfg(target_os = "windows")]
    fn drain_ready<F>(&mut self, on_token: &F)
    where
        F: Fn(&str),
    {
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
            self.capture_result(on_token, true);
        }
    }

    #[cfg(target_os = "windows")]
    fn capture_result<F>(&mut self, on_token: &F, allow_endpoint_reset: bool)
    where
        F: Fn(&str),
    {
        let Some(result) = self.recognizer.get_result(&self.stream) else {
            return;
        };
        let text = result.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.emit_delta(&text, on_token);
        self.last_partial_text = text.clone();
        if allow_endpoint_reset && self.recognizer.is_endpoint(&self.stream) {
            append_segment(&mut self.committed_text, &text);
            self.last_partial_text.clear();
            self.last_emitted_text.clear();
            self.recognizer.reset(&self.stream);
        }
    }

    fn emit_delta<F>(&mut self, text: &str, on_token: &F)
    where
        F: Fn(&str),
    {
        if text == self.last_emitted_text {
            return;
        }
        if let Some(delta) = text.strip_prefix(&self.last_emitted_text) {
            if !delta.is_empty() {
                on_token(delta);
            }
        } else {
            on_token(text);
        }
        self.last_emitted_text = text.to_string();
    }

    fn final_text(&self) -> String {
        let mut text = self.committed_text.clone();
        if !self.last_partial_text.is_empty() {
            append_segment(&mut text, &self.last_partial_text);
        }
        text.trim().to_string()
    }
}

fn append_segment(text: &mut String, segment: &str) {
    let segment = segment.trim();
    if segment.is_empty() {
        return;
    }
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    text.push_str(segment);
}

#[cfg(target_os = "windows")]
fn create_online_session_from_loaded(loaded: LoadedOnlineModel) -> Result<SherpaOnlineSession> {
    let stream = loaded.recognizer.create_stream();
    Ok(SherpaOnlineSession {
        alias: loaded.alias,
        recognizer: loaded.recognizer,
        stream,
        committed_text: String::new(),
        last_partial_text: String::new(),
        last_emitted_text: String::new(),
    })
}

#[cfg(not(target_os = "windows"))]
fn create_online_session_from_loaded(loaded: LoadedOnlineModel) -> Result<SherpaOnlineSession> {
    Ok(SherpaOnlineSession {
        alias: loaded.alias,
        committed_text: String::new(),
        last_partial_text: String::new(),
        last_emitted_text: String::new(),
    })
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
    async fn new_runtime_reports_offline_batch_status_shape() {
        let runtime = SherpaOnnxRuntime::new();
        let status = runtime.status_snapshot("sense-voice-small-zh").await;

        assert_eq!(status.provider_id, PROVIDER_ID);
        assert_eq!(status.available, cfg!(target_os = "windows"));
        assert!(!status.runtime_ready);
        assert_eq!(status.active_model, "sense-voice-small-zh");
        assert_eq!(status.loaded_model_id, None);
        assert_eq!(status.error, None);
        assert_eq!(status.last_prepare_ms, None);
        assert_eq!(status.last_transcribe_ms, None);
        assert_eq!(status.last_audio_ms, None);
        assert_eq!(status.last_error, None);
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn status_snapshot_only_marks_active_model_ready() {
        let runtime = SherpaOnnxRuntime::new();
        runtime.state.lock().offline_loaded = Some(LoadedOfflineModel {
            alias: "sense-voice-small-zh".into(),
        });

        let active = runtime.status_snapshot("sense-voice-small-zh").await;
        let inactive = runtime.status_snapshot("paraformer-zh").await;

        assert!(active.runtime_ready);
        assert_eq!(
            active.loaded_model_id.as_deref(),
            Some("sense-voice-small-zh")
        );
        assert!(!inactive.runtime_ready);
        assert_eq!(inactive.loaded_model_id, None);
    }

    #[tokio::test]
    async fn ensure_loaded_rejects_unknown_alias() {
        let runtime = SherpaOnnxRuntime::new();
        let result = runtime.ensure_loaded("unknown-sherpa-model").await;
        assert!(result.is_err());
        let status = runtime.status_snapshot("sense-voice-small-zh").await;
        assert!(status.last_prepare_ms.is_some());
        assert!(status
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("unknown-sherpa-model"));
        assert_eq!(status.error, status.last_error);
    }

    #[tokio::test]
    async fn status_snapshot_reports_runtime_diagnostics() {
        let runtime = SherpaOnnxRuntime::new();
        runtime.record_prepare_result(12, None);
        runtime.record_transcribe_result(1_250, 34, Some("decode failed".into()));

        let status = runtime.status_snapshot("paraformer-zh").await;

        assert_eq!(status.last_prepare_ms, Some(12));
        assert_eq!(status.last_audio_ms, Some(1_250));
        assert_eq!(status.last_transcribe_ms, Some(34));
        assert_eq!(status.error.as_deref(), Some("decode failed"));
        assert_eq!(status.last_error.as_deref(), Some("decode failed"));
    }

    #[test]
    fn request_cancel_prepare_marks_runtime_cancelled() {
        let runtime = SherpaOnnxRuntime::new();
        assert!(!runtime.cancel_prepare_requested_for_tests());
        runtime.request_cancel_prepare();
        assert!(runtime.cancel_prepare_requested_for_tests());
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
        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("model.int8.onnx"));
        assert!(message.contains(&dir.display().to_string()));
    }

    #[test]
    fn model_dir_size_mb_counts_nested_files() {
        let dir = std::env::temp_dir().join(format!(
            "openless-sherpa-runtime-size-test-{}",
            uuid::Uuid::new_v4()
        ));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("top.bin"), vec![1u8; 1024 * 1024]).unwrap();
        std::fs::write(nested.join("child.bin"), vec![2u8; 1024 * 1024]).unwrap();

        let size = model_dir_size_mb(&dir);

        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(size, Some(2));
    }

    #[tokio::test]
    async fn release_now_clears_loaded_model() {
        let runtime = SherpaOnnxRuntime::new();
        #[cfg(not(target_os = "windows"))]
        {
            runtime.state.lock().offline_loaded = Some(LoadedOfflineModel {
                alias: "sense-voice-small-zh".into(),
            });
            runtime.state.lock().online_loaded = Some(LoadedOnlineModel {
                alias: sherpa::DEFAULT_ONLINE_MODEL_ALIAS.into(),
            });
        }
        runtime.release_now().await.unwrap();

        let status = runtime.status_snapshot("paraformer-zh").await;
        assert!(!status.runtime_ready);
        assert_eq!(status.loaded_model_id, None);
        assert!(runtime.state.lock().offline_loaded.is_none());
        assert!(runtime.state.lock().online_loaded.is_none());
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

    #[tokio::test]
    async fn transcribe_pcm_rejects_online_model_alias() {
        let runtime = SherpaOnnxRuntime::new();
        let result = runtime
            .transcribe_pcm(
                sherpa::DEFAULT_ONLINE_MODEL_ALIAS,
                &[0, 0],
                None,
                std::time::Duration::from_secs(5),
            )
            .await;

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("online-only"));
    }

    #[tokio::test]
    async fn create_online_session_rejects_offline_model_alias() {
        let runtime = SherpaOnnxRuntime::new();
        let result = runtime
            .create_online_session(sherpa::DEFAULT_MODEL_ALIAS)
            .await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        assert!(format!("{error:#}").contains("not an online streaming model"));
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

    #[test]
    fn append_segment_inserts_space_between_segments() {
        let mut text = String::new();
        append_segment(&mut text, "你好");
        append_segment(&mut text, "world");
        assert_eq!(text, "你好 world");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn sherpa_onnx_crate_exposes_online_recognizer_api_for_streaming_phase() {
        let mut config = sherpa_onnx::OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some("encoder.onnx".into());
        config.model_config.transducer.decoder = Some("decoder.onnx".into());
        config.model_config.transducer.joiner = Some("joiner.onnx".into());
        config.model_config.tokens = Some("tokens.txt".into());
        config.enable_endpoint = true;
        config.decoding_method = Some("greedy_search".into());

        assert_eq!(
            config.model_config.transducer.encoder.as_deref(),
            Some("encoder.onnx")
        );
        assert!(config.enable_endpoint);
        assert_eq!(config.decoding_method.as_deref(), Some("greedy_search"));
    }
}
