#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FoundryNativeModelState {
    pub alias: String,
    pub cached: bool,
    pub size_bytes: Option<u64>,
    pub display_name: Option<String>,
}

/// CPU 回退期间向调用方报告的最小状态。调用方只决定如何展示，不参与模型选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoundryFallbackNotice {
    SwitchingToCpu,
    DownloadingCpu,
}

impl FoundryFallbackNotice {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::SwitchingToCpu => "检测到 GPU 识别异常，正在切换 CPU…",
            Self::DownloadingCpu => "正在下载 CPU 模型，首次使用可能较慢…",
        }
    }
}

pub(crate) type FoundryFallbackNoticeCallback =
    Arc<dyn Fn(FoundryFallbackNotice) + Send + Sync + 'static>;

/// 一条 Foundry ASR route 的进程内代数。
///
/// route 在录音/重转录会话创建时分配；后续设置变更或新会话只能使旧 route 失效，
/// 不能在真正开始转写时把旧会话重新绑定到最新代数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FoundryRouteEpoch(u64);

/// 一段录音的 Foundry 转写结果。仅供本地 ASR provider 消费，不扩展 IPC 协议。
#[derive(Debug, Clone, Default)]
pub(crate) struct FoundryTranscriptionOutcome {
    pub texts: Vec<String>,
    pub used_cpu_fallback: bool,
    pub gpu_model_id: Option<String>,
    pub cpu_model_id: Option<String>,
    pub primary_recovery: Option<FoundryPrimaryRecoveryToken>,
}

/// 一次成功 CPU 回退后恢复原始 primary variant 所需的进程内令牌。
///
/// 令牌绑定 route epoch；旧会话的异步恢复不能覆盖后续录音或显式模型操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundryPrimaryRecoveryToken {
    alias: String,
    primary_model_id: String,
    route_epoch: FoundryRouteEpoch,
}

impl FoundryPrimaryRecoveryToken {
    pub(crate) fn new(
        alias: impl Into<String>,
        primary_model_id: impl Into<String>,
        route_epoch: FoundryRouteEpoch,
    ) -> Self {
        Self {
            alias: alias.into(),
            primary_model_id: primary_model_id.into(),
            route_epoch,
        }
    }

    pub(crate) const fn route_epoch(&self) -> FoundryRouteEpoch {
        self.route_epoch
    }
}

/// 单次录音回退临时 CPU 模型的运行时 lease。
///
/// 取消清理必须带上该 lease，避免旧录音的异步清理误卸载下一段录音重新加载的同一 CPU variant。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FoundryTemporaryCpuFallbackLease(u64);

/// CPU 回退已经尝试但不能完成时的终态标记。
///
/// Coordinator 据此跳过面对瞬态网络错误设计的静默重试，避免重新命中同一 CUDA 路径。
#[derive(Debug, thiserror::Error)]
#[error("Foundry CUDA CPU fallback failed; gpu_error={gpu_error}; cpu_error={cpu_error}")]
pub(crate) struct FoundryCpuFallbackTerminalError {
    gpu_error: String,
    cpu_error: String,
}

pub(crate) fn is_terminal_foundry_fallback_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<FoundryCpuFallbackTerminalError>()
            .is_some()
    })
}

/// Foundry GPU→CPU 回退终态错误面向用户的精简文案（PR #945 review P2-2）。
///
/// 重转录/听写/QA 三处消费点共用，避免文案分叉；原始 GPU/CPU SDK 错误
/// 只出现在日志与 err 字段，不直接展示给用户。仅 Windows 存在 Foundry
/// provider，非 Windows 目标无需编译（避免 dead_code 警告）。
#[cfg(target_os = "windows")]
pub(crate) const FOUNDRY_FALLBACK_TERMINAL_USER_MESSAGE: &str =
    "本地识别失败: GPU 识别异常，且 CPU 回退未能完成（详情见日志）";

#[cfg(target_os = "windows")]
// PR #945 review P2-1：transcribe_audio_file 死代码已删除；此 allow 仍需保留，
// 覆盖 ensure_loaded 等其他 pre-existing 死代码（删除它们涉及公共 API 形状变更，
// 留作后续跟进）。
#[allow(dead_code)]
mod imp {
    use super::{FoundryPrimaryRecoveryToken, FoundryRouteEpoch};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const PROGRESS_EMIT_MIN_INTERVAL_MS: u64 = 100;

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or_default()
    }

    use anyhow::{Context, Result};
    use foundry_local_sdk::{DeviceType, FoundryLocalConfig, FoundryLocalManager, Model};
    use parking_lot::Mutex;
    use tokio::sync::Mutex as AsyncMutex;

    use super::{
        FoundryCpuFallbackTerminalError, FoundryFallbackNotice, FoundryFallbackNoticeCallback,
        FoundryTemporaryCpuFallbackLease, FoundryTranscriptionOutcome,
    };
    use crate::asr::local::foundry::{
        FoundryPrepareProgressPayload, FoundryRuntimeStatus, PROVIDER_ID,
    };
    use crate::asr::local::foundry_native::{self, RuntimeSource};

    type FoundryPrepareProgressCallback =
        Arc<dyn Fn(FoundryPrepareProgressPayload) + Send + Sync + 'static>;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FoundryExecutionDevice {
        Cpu,
        Gpu,
        Other,
    }

    impl FoundryExecutionDevice {
        fn from_model(model: &Model) -> Self {
            match model
                .info()
                .runtime
                .as_ref()
                .map(|runtime| &runtime.device_type)
            {
                Some(DeviceType::CPU) => Self::Cpu,
                Some(DeviceType::GPU) => Self::Gpu,
                _ => Self::Other,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FoundryVariantDescriptor {
        id: String,
        version: u64,
        device: FoundryExecutionDevice,
    }

    impl FoundryVariantDescriptor {
        fn new(id: impl Into<String>, version: u64, device: FoundryExecutionDevice) -> Self {
            Self {
                id: id.into(),
                version,
                device,
            }
        }
    }

    fn select_cpu_variant_id(variants: &[FoundryVariantDescriptor]) -> Option<String> {
        variants
            .iter()
            .filter(|variant| variant.device == FoundryExecutionDevice::Cpu)
            .max_by(|left, right| {
                left.version
                    .cmp(&right.version)
                    .then_with(|| left.id.cmp(&right.id))
            })
            .map(|variant| variant.id.clone())
    }

    fn is_cuda_cudnn_failure(error: &str) -> bool {
        let error = error.to_ascii_lowercase();
        error.contains("cudnn_fe")
            || error.contains("cudnn_backend_api_failed")
            || error.contains("failed to initialize cudnn frontend")
            || error.contains("cudnn_engines_precompiled64_9.dll")
    }

    fn is_cuda_fallback_candidate(
        device: FoundryExecutionDevice,
        execution_provider: Option<&str>,
        error: &str,
    ) -> bool {
        device == FoundryExecutionDevice::Gpu
            && execution_provider
                .is_some_and(|provider| provider.eq_ignore_ascii_case("CUDAExecutionProvider"))
            && is_cuda_cudnn_failure(error)
    }

    fn may_reuse_loaded_model(
        loaded_alias: &str,
        requested_alias: &str,
        temporary_cpu_fallback: bool,
    ) -> bool {
        loaded_alias == requested_alias && !temporary_cpu_fallback
    }

    fn should_release_temporary_cpu_fallback(
        loaded_lease: Option<FoundryTemporaryCpuFallbackLease>,
        cancelled_lease: FoundryTemporaryCpuFallbackLease,
    ) -> bool {
        loaded_lease == Some(cancelled_lease)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FoundryCpuLoadCompletion {
        UseCpu,
        RestorePrimary,
        Cancelled,
    }

    fn cpu_load_completion(
        load_succeeded: bool,
        cancel_requested: bool,
    ) -> FoundryCpuLoadCompletion {
        if cancel_requested {
            FoundryCpuLoadCompletion::Cancelled
        } else if load_succeeded {
            FoundryCpuLoadCompletion::UseCpu
        } else {
            FoundryCpuLoadCompletion::RestorePrimary
        }
    }

    #[derive(Clone)]
    struct LoadedModel {
        alias: String,
        model_id: String,
        model: Arc<Model>,
        device: FoundryExecutionDevice,
        execution_provider: Option<String>,
        temporary_cpu_fallback_lease: Option<FoundryTemporaryCpuFallbackLease>,
    }

    impl LoadedModel {
        fn new(
            alias: impl Into<String>,
            model: Arc<Model>,
            temporary_cpu_fallback_lease: Option<FoundryTemporaryCpuFallbackLease>,
        ) -> Self {
            let execution_provider = model
                .info()
                .runtime
                .as_ref()
                .map(|runtime| runtime.execution_provider.clone());
            Self {
                alias: alias.into(),
                model_id: model.id().to_string(),
                device: FoundryExecutionDevice::from_model(&model),
                execution_provider,
                model,
                temporary_cpu_fallback_lease,
            }
        }

        fn is_temporary_cpu_fallback(&self) -> bool {
            self.temporary_cpu_fallback_lease.is_some()
        }
    }

    #[derive(Clone)]
    struct PrimaryModel {
        alias: String,
        model_id: String,
        model: Arc<Model>,
        device: FoundryExecutionDevice,
        execution_provider: Option<String>,
    }

    impl PrimaryModel {
        fn from_loaded(loaded: &LoadedModel) -> Self {
            Self {
                alias: loaded.alias.clone(),
                model_id: loaded.model_id.clone(),
                model: Arc::clone(&loaded.model),
                device: loaded.device,
                execution_provider: loaded.execution_provider.clone(),
            }
        }
    }

    #[derive(Default)]
    struct RuntimeState {
        manager: Option<&'static FoundryLocalManager>,
        loaded: Option<LoadedModel>,
        primary_by_alias: HashMap<String, PrimaryModel>,
    }

    #[derive(Debug, Clone)]
    struct FoundryCpuSwitch {
        model_id: String,
        cache_hit: bool,
    }

    /// 将「按分片转写、识别 CUDA 错误、一次 CPU 回退、清理临时模型」收敛到一个
    /// 可替换的执行接口中。生产路径使用 SDK adapter；测试路径使用脚本化 fake，完全不需 GPU。
    #[allow(async_fn_in_trait)]
    trait FoundryExecutionAdapter {
        fn alias(&self) -> &str;
        fn execution_device(&self) -> FoundryExecutionDevice;
        fn execution_provider(&self) -> Option<&str>;
        fn model_id(&self) -> &str;
        async fn transcribe(&mut self, audio_path: &Path, timeout: Duration) -> Result<String>;
        async fn switch_to_cpu(
            &mut self,
            notices: &FoundryFallbackNoticeCallback,
        ) -> Result<FoundryCpuSwitch>;
        async fn finish(&mut self) -> Result<()>;
    }

    async fn transcribe_recording_with_adapter<A: FoundryExecutionAdapter>(
        adapter: &mut A,
        audio_paths: &[PathBuf],
        audio_timeout: Duration,
        notices: &FoundryFallbackNoticeCallback,
    ) -> Result<FoundryTranscriptionOutcome> {
        let result = async {
            let mut outcome = FoundryTranscriptionOutcome {
                texts: Vec::with_capacity(audio_paths.len()),
                used_cpu_fallback: false,
                gpu_model_id: (adapter.execution_device() == FoundryExecutionDevice::Gpu)
                    .then(|| adapter.model_id().to_string()),
                cpu_model_id: None,
                primary_recovery: None,
            };
            let mut fallback_gpu_error = None;
            let mut fallback_started_at = None;
            let mut deadline = Instant::now()
                .checked_add(audio_timeout)
                .context("Foundry Local Whisper transcription timeout is too large")?;

            for (index, audio_path) in audio_paths.iter().enumerate() {
                let timeout = remaining_transcription_timeout(deadline, Instant::now())
                    .with_context(|| {
                        format!(
                            "Foundry Local Whisper total timeout exhausted before chunk {}/{}",
                            index + 1,
                            audio_paths.len()
                        )
                    })?;
                match adapter.transcribe(audio_path, timeout).await {
                    Ok(text) => outcome.texts.push(text),
                    Err(error)
                        if !outcome.used_cpu_fallback
                            && is_cuda_fallback_candidate(
                                adapter.execution_device(),
                                adapter.execution_provider(),
                                &format!("{error:#}"),
                            ) =>
                    {
                        let gpu_error = format!("{error:#}");
                        let fallback_started = Instant::now();
                        let alias = adapter.alias().to_string();
                        let gpu_model_id = adapter.model_id().to_string();
                        log::warn!(
                            "[foundry-asr] event=cuda_fallback_detected alias={} gpu_model={} chunk={}/{} error_category=cudnn_cuda",
                            alias,
                            gpu_model_id,
                            index + 1,
                            audio_paths.len()
                        );
                        notices(FoundryFallbackNotice::SwitchingToCpu);
                        let cpu = match adapter.switch_to_cpu(notices).await {
                            Ok(cpu) => cpu,
                            Err(cpu_error) => {
                                log::error!(
                                    "[foundry-asr] event=cuda_fallback_failed alias={} gpu_model={} fallback_stage=cpu_prepare duration_ms={} error_category=cpu_prepare",
                                    alias,
                                    gpu_model_id,
                                    fallback_started.elapsed().as_millis()
                                );
                                return Err(anyhow::Error::new(FoundryCpuFallbackTerminalError {
                                    gpu_error: gpu_error.clone(),
                                    cpu_error: format!("{cpu_error:#}"),
                                }));
                            }
                        };
                        log::warn!(
                            "[foundry-asr] event=cuda_fallback_cpu_ready alias={} gpu_model={} cpu_model={} cpu_cache_hit={} duration_ms={}",
                            alias,
                            gpu_model_id,
                            cpu.model_id,
                            cpu.cache_hit,
                            fallback_started.elapsed().as_millis()
                        );
                        outcome.used_cpu_fallback = true;
                        outcome.cpu_model_id = Some(cpu.model_id);
                        fallback_gpu_error = Some(gpu_error);
                        fallback_started_at = Some(fallback_started);
                        // CPU 是一次恢复路径：首次下载/加载不耗尽原 GPU 的推理预算，
                        // 因此给尚未完成的分片一段新的同规格推理窗口。
                        deadline = Instant::now()
                            .checked_add(audio_timeout)
                            .context("Foundry CPU fallback timeout is too large")?;
                        let retry_timeout =
                            remaining_transcription_timeout(deadline, Instant::now())?;
                        let text = match adapter.transcribe(audio_path, retry_timeout).await {
                            Ok(text) => text,
                            Err(cpu_error) => {
                                log::error!(
                                    "[foundry-asr] event=cuda_fallback_failed alias={} gpu_model={} cpu_model={} fallback_stage=cpu_inference duration_ms={} error_category=cpu_inference",
                                    alias,
                                    gpu_model_id,
                                    outcome.cpu_model_id.as_deref().unwrap_or("unknown"),
                                    fallback_started.elapsed().as_millis()
                                );
                                return Err(anyhow::Error::new(FoundryCpuFallbackTerminalError {
                                    gpu_error: fallback_gpu_error
                                        .clone()
                                        .unwrap_or_else(|| "unknown CUDA error".to_string()),
                                    cpu_error: format!("{cpu_error:#}"),
                                }));
                            }
                        };
                        outcome.texts.push(text);
                    }
                    Err(error) if outcome.used_cpu_fallback => {
                        log::error!(
                            "[foundry-asr] event=cuda_fallback_failed alias={} gpu_model={} cpu_model={} fallback_stage=cpu_inference duration_ms={} error_category=cpu_inference",
                            adapter.alias(),
                            outcome.gpu_model_id.as_deref().unwrap_or("unknown"),
                            outcome.cpu_model_id.as_deref().unwrap_or("unknown"),
                            fallback_started_at
                                .map(|started| started.elapsed().as_millis())
                                .unwrap_or_default()
                        );
                        return Err(anyhow::Error::new(FoundryCpuFallbackTerminalError {
                            gpu_error: fallback_gpu_error
                                .clone()
                                .unwrap_or_else(|| "unknown CUDA error".to_string()),
                            cpu_error: format!("{error:#}"),
                        }));
                    }
                    Err(error) => return Err(error),
                }
            }
            if outcome.used_cpu_fallback {
                log::info!(
                    "[foundry-asr] event=cuda_fallback_completed alias={} gpu_model={} cpu_model={} duration_ms={}",
                    adapter.alias(),
                    outcome.gpu_model_id.as_deref().unwrap_or("unknown"),
                    outcome.cpu_model_id.as_deref().unwrap_or("unknown"),
                    fallback_started_at
                        .map(|started| started.elapsed().as_millis())
                        .unwrap_or_default()
                );
            }
            Ok(outcome)
        }
        .await;

        // 临时 CPU 模型无论结果如何都应释放；清理失败不能覆盖已拿到的转写文本，
        // 但会保留 runtime state，令下一次默认准备路径继续负责回收它。
        if let Err(error) = adapter.finish().await {
            log::warn!("[foundry-asr] release temporary CPU fallback model failed: {error:#}");
        }
        result
    }

    fn remaining_transcription_timeout(deadline: Instant, now: Instant) -> Result<Duration> {
        deadline
            .checked_duration_since(now)
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| anyhow::anyhow!("Foundry Local Whisper total timeout exhausted"))
    }

    struct FoundrySdkExecution<'a> {
        runtime: &'a FoundryLocalRuntime,
        manager: &'static FoundryLocalManager,
        alias: &'a str,
        language_hint: Option<String>,
        primary: PrimaryModel,
        loaded: LoadedModel,
        using_temporary_cpu_fallback: bool,
    }

    impl FoundrySdkExecution<'_> {
        async fn restore_after_failed_cpu_switch(
            &self,
            previous: &LoadedModel,
            error: anyhow::Error,
        ) -> Result<FoundryCpuSwitch> {
            if let Err(restore_error) = self
                .runtime
                .restore_loaded_model(self.manager, previous)
                .await
            {
                return Err(error.context(format!(
                    "CPU fallback also failed to restore GPU model {}: {restore_error:#}",
                    previous.model_id
                )));
            }
            Err(error)
        }
    }

    impl FoundryExecutionAdapter for FoundrySdkExecution<'_> {
        fn alias(&self) -> &str {
            self.alias
        }

        fn execution_device(&self) -> FoundryExecutionDevice {
            self.loaded.device
        }

        fn execution_provider(&self) -> Option<&str> {
            self.loaded.execution_provider.as_deref()
        }

        fn model_id(&self) -> &str {
            &self.loaded.model_id
        }

        async fn transcribe(&mut self, audio_path: &Path, timeout: Duration) -> Result<String> {
            let mut client = self.loaded.model.create_audio_client();
            if let Some(language_hint) = self.language_hint.as_deref() {
                client = client.language(language_hint);
            }
            let model_id = self.loaded.model_id.clone();
            let result = tokio::time::timeout(timeout, client.transcribe(audio_path))
                .await
                .with_context(|| {
                    format!(
                        "transcribe audio with Foundry model {model_id} timed out after {} seconds",
                        timeout.as_secs()
                    )
                })?
                .with_context(|| format!("transcribe audio with Foundry model {model_id}"))?;
            Ok(result.text)
        }

        async fn switch_to_cpu(
            &mut self,
            notices: &FoundryFallbackNoticeCallback,
        ) -> Result<FoundryCpuSwitch> {
            // 在任何 await 之前分配 lease：取消方可以用当时的 lease 上界安全清理尚未
            // 完成加载的临时模型，同时不会触及随后新录音分配的更高 lease。
            let lease = self.runtime.next_temporary_cpu_fallback_lease();
            self.runtime.check_prepare_cancelled()?;
            let cpu_model = self
                .runtime
                .cpu_variant_model(self.manager, self.alias)
                .await?;
            self.runtime.check_prepare_cancelled()?;
            let cpu_model_id = cpu_model.id().to_string();
            let cached = cpu_model
                .is_cached()
                .await
                .with_context(|| format!("check Foundry CPU model cache {cpu_model_id}"))?;
            log::info!(
                "[foundry-asr] event=cpu_variant_selected alias={} gpu_model={} cpu_model={} cpu_cache_hit={}",
                self.alias,
                self.loaded.model_id,
                cpu_model_id,
                cached
            );
            if !cached {
                notices(FoundryFallbackNotice::DownloadingCpu);
                log::info!(
                    "[foundry-asr] event=cpu_download_started alias={} cpu_model={}",
                    self.alias,
                    cpu_model_id
                );
                cpu_model
                    .download_builder()
                    .cancel(Arc::clone(&self.runtime.cancel_prepare))
                    .run()
                    .await
                    .with_context(|| format!("download Foundry CPU model {cpu_model_id}"))?;
                log::info!(
                    "[foundry-asr] event=cpu_download_completed alias={} cpu_model={}",
                    self.alias,
                    cpu_model_id
                );
            }
            self.runtime.check_prepare_cancelled()?;

            let previous = self.loaded.clone();
            if let Err(error) = FoundryLocalRuntime::unload_model(&previous).await {
                return Err(error.context(format!(
                    "unload GPU model {} before CPU fallback",
                    previous.model_id
                )));
            }
            self.runtime.clear_loaded_if_model_id(&previous.model_id);

            // 先把带 lease 的临时模型记入 runtime state，再等待 load。若外层因取消 drop
            // 当前 future，取消清理任务将在 lifecycle 锁释放后看到这份 state 并卸载它。
            let loaded = LoadedModel::new(self.alias, Arc::clone(&cpu_model), Some(lease));
            {
                let mut state = self.runtime.state.lock();
                state.manager = Some(self.manager);
                state.loaded = Some(loaded.clone());
            }

            log::info!(
                "[foundry-asr] event=cpu_load_started alias={} cpu_model={}",
                self.alias,
                cpu_model_id
            );
            let load_result = cpu_model
                .load()
                .await
                .with_context(|| format!("load Foundry CPU model {cpu_model_id}"));
            let cancellation_error = self.runtime.check_prepare_cancelled().err();
            match cpu_load_completion(load_result.is_ok(), cancellation_error.is_some()) {
                FoundryCpuLoadCompletion::UseCpu => {}
                FoundryCpuLoadCompletion::RestorePrimary => {
                    let error = load_result.expect_err("failed CPU load must carry an error");
                    if let Err(cleanup_error) = FoundryLocalRuntime::unload_model(&loaded).await {
                        return Err(error.context(format!(
                            "temporary CPU model {cpu_model_id} also failed to unload; preserving temporary runtime state: {cleanup_error:#}"
                        )));
                    }
                    self.runtime.clear_loaded_if_model_id(&loaded.model_id);
                    return self.restore_after_failed_cpu_switch(&previous, error).await;
                }
                FoundryCpuLoadCompletion::Cancelled => {
                    let error = cancellation_error
                        .expect("cancelled CPU load completion must carry a cancellation error");
                    if let Err(cleanup_error) = FoundryLocalRuntime::unload_model(&loaded).await {
                        return Err(error.context(format!(
                            "cancelled temporary CPU model {cpu_model_id} also failed to unload; preserving temporary runtime state: {cleanup_error:#}"
                        )));
                    }
                    self.runtime.clear_loaded_if_model_id(&loaded.model_id);
                    return Err(error);
                }
            }

            self.loaded = loaded;
            self.using_temporary_cpu_fallback = true;
            log::warn!(
                "[foundry-asr] event=cpu_load_completed alias={} gpu_model={} cpu_model={} cpu_cache_hit={}",
                self.alias,
                previous.model_id,
                self.loaded.model_id,
                cached
            );
            Ok(FoundryCpuSwitch {
                model_id: cpu_model_id,
                cache_hit: cached,
            })
        }

        async fn finish(&mut self) -> Result<()> {
            if self.using_temporary_cpu_fallback {
                FoundryLocalRuntime::unload_model(&self.loaded).await?;
                self.runtime.clear_loaded_if_model_id(&self.loaded.model_id);
                self.using_temporary_cpu_fallback = false;
            }
            Ok(())
        }
    }

    pub struct FoundryLocalRuntime {
        /// 串行化 runtime 内所有「物理状态」操作（下载/加载/卸载/推理），防止
        /// release/delete/prepare 与在途转写交错破坏 SDK 状态。
        ///
        /// 锁粒度 trade-off（PR #945 review P1-3）：`transcribe_audio_files` 整段录音
        /// 单次持锁，期间 `release_now`/`delete_model`/`prepare` 都会等待；首次 CPU
        /// 回退下载可能数百 MB、持续数十秒。该等待有界于转写 timeout 预算，且取消
        /// 仍可中断（`cancel_prepare` + `check_prepare_cancelled`）。若未来要缩小粒度，
        /// 可让下载阶段不持锁、下载完成后重新校验 route epoch 再持锁加载/推理。
        lifecycle: AsyncMutex<()>,
        cancel_prepare: Arc<AtomicBool>,
        temporary_cpu_fallback_sequence: AtomicU64,
        route_epoch: AtomicU64,
        state: Mutex<RuntimeState>,
    }

    impl Default for FoundryLocalRuntime {
        fn default() -> Self {
            Self::new()
        }
    }

    impl FoundryLocalRuntime {
        pub fn new() -> Self {
            Self {
                lifecycle: AsyncMutex::new(()),
                cancel_prepare: Arc::new(AtomicBool::new(false)),
                temporary_cpu_fallback_sequence: AtomicU64::new(0),
                route_epoch: AtomicU64::new(0),
                state: Mutex::new(RuntimeState::default()),
            }
        }

        pub async fn status_snapshot(
            &self,
            active_model: &str,
            runtime_source: &str,
        ) -> FoundryRuntimeStatus {
            let loaded_model_id = self
                .state
                .lock()
                .loaded
                .as_ref()
                .map(|loaded| loaded.model_id.clone());
            let native_ready = foundry_native::runtime_ready();
            FoundryRuntimeStatus {
                provider_id: PROVIDER_ID.into(),
                available: true,
                runtime_ready: native_ready,
                runtime_source: foundry_native::normalize_runtime_source_str(runtime_source),
                active_model: active_model.to_string(),
                loaded_model_id,
                endpoint: None,
                error: None,
            }
        }

        /// 激活事务的 prepare 回执：LoadedModel 只在 SDK load 成功后发布。
        /// SDK model_id 可包含具体设备后缀，必须以保存的 alias 匹配用户请求。
        pub(crate) fn is_loaded_for(&self, alias: &str) -> bool {
            self.state
                .lock()
                .loaded
                .as_ref()
                .is_some_and(|loaded| loaded.alias == alias)
        }

        pub async fn ensure_loaded(&self, alias: &str, runtime_source: &str) -> Result<String> {
            self.ensure_loaded_with_progress(alias, runtime_source, |_| {})
                .await
        }

        pub async fn ensure_loaded_with_progress<F>(
            &self,
            alias: &str,
            runtime_source: &str,
            progress: F,
        ) -> Result<String>
        where
            F: Fn(FoundryPrepareProgressPayload) + Send + Sync + 'static,
        {
            self.advance_route_epoch();
            let _lifecycle = self.lifecycle.lock().await;
            self.cancel_prepare.store(false, Ordering::SeqCst);
            let progress: FoundryPrepareProgressCallback = Arc::new(progress);
            // 节流：SDK 的 percent 回调频率不可控（可能远高于前端可感知的
            // 刷新率），percent 类事件 ≥150ms 才转发，避免进度浮层抽搐；
            // phase 事件（percent=None，如 runtime/model/load 的阶段切换与
            // finished/failed）不受限，保证阶段提示不丢。
            let raw = Arc::clone(&progress);
            let last_emit = Arc::new(AtomicU64::new(0));
            let progress: FoundryPrepareProgressCallback = Arc::new(move |payload| {
                if payload.percent.is_some() {
                    let now = now_millis();
                    if now - last_emit.load(Ordering::Relaxed) < PROGRESS_EMIT_MIN_INTERVAL_MS {
                        return;
                    }
                    last_emit.store(now, Ordering::Relaxed);
                }
                raw(payload);
            });
            let runtime_source = foundry_native::normalize_runtime_source(runtime_source);
            Ok(self
                .ensure_loaded_locked(alias, runtime_source, progress)
                .await?
                .model_id)
        }

        pub fn request_cancel_prepare(&self) {
            self.advance_route_epoch();
            self.cancel_prepare.store(true, Ordering::SeqCst);
        }

        /// 仅取消仍属于指定 route 的 ASR 操作，并返回该操作当前持有的临时 CPU lease。
        ///
        /// route 校验与代数推进使用 CAS，避免旧 provider 在新录音刚开始时把共享
        /// `cancel_prepare` 标志写给新录音。清理 lease 从 state 读取精确值，不使用
        /// runtime 全局序列上界，避免旧取消误卸载新录音的 CPU 模型。
        pub(crate) fn request_cancel_transcription(
            &self,
            expected_epoch: FoundryRouteEpoch,
        ) -> Option<FoundryTemporaryCpuFallbackLease> {
            let state = self.state.lock();
            let current = self.route_epoch.load(Ordering::SeqCst);
            if current != expected_epoch.0
                || self
                    .route_epoch
                    .compare_exchange(
                        current,
                        current.wrapping_add(1),
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    )
                    .is_err()
            {
                return None;
            }
            self.cancel_prepare.store(true, Ordering::SeqCst);
            state
                .loaded
                .as_ref()
                .and_then(|loaded| loaded.temporary_cpu_fallback_lease)
        }

        #[cfg(test)]
        pub(crate) fn cancel_prepare_requested_for_tests(&self) -> bool {
            self.cancel_prepare.load(Ordering::SeqCst)
        }

        /// 整段录音（所有分片 + CPU 回退的首次下载/加载）在单次 lifecycle 锁持有内
        /// 完成。锁期间 `release_now`/`delete_model`/`prepare` 会等待，首次 CPU 回退
        /// 下载可达数百 MB；该等待有界于 `audio_timeout`，取消仍可中断（见
        /// `FoundryLocalRuntime::lifecycle` 字段注释的 trade-off，PR #945 review P1-3）。
        pub(crate) async fn transcribe_audio_files(
            &self,
            route_epoch: FoundryRouteEpoch,
            alias: &str,
            runtime_source: &str,
            language_hint: Option<&str>,
            audio_paths: &[PathBuf],
            audio_timeout: Duration,
            notices: FoundryFallbackNoticeCallback,
        ) -> Result<FoundryTranscriptionOutcome> {
            let _lifecycle = self.lifecycle.lock().await;
            self.cancel_prepare.store(false, Ordering::SeqCst);
            let runtime_source = foundry_native::normalize_runtime_source(runtime_source);
            let loaded = self
                .ensure_loaded_locked(alias, runtime_source, Arc::new(|_| {}))
                .await?;
            let manager = self.manager()?;
            let primary = PrimaryModel::from_loaded(&loaded);
            let mut execution = FoundrySdkExecution {
                runtime: self,
                manager,
                alias,
                language_hint: normalized_language_hint(language_hint),
                primary,
                loaded,
                using_temporary_cpu_fallback: false,
            };
            let mut outcome = transcribe_recording_with_adapter(
                &mut execution,
                audio_paths,
                audio_timeout,
                &notices,
            )
            .await?;
            if outcome.used_cpu_fallback {
                outcome.primary_recovery = Some(FoundryPrimaryRecoveryToken::new(
                    alias,
                    execution.primary.model_id.clone(),
                    route_epoch,
                ));
            }
            Ok(outcome)
        }

        pub async fn release_now(&self) -> Result<()> {
            self.advance_route_epoch();
            let wait_started = Instant::now();
            let _lifecycle = self.lifecycle.lock().await;
            let waited_ms = wait_started.elapsed().as_millis();
            if waited_ms >= 100 {
                // 长时间等待说明有在途转写/下载持锁（PR #945 review P1-3），
                // 记日志便于真机定位「点释放模型无响应」的阻塞点。
                log::info!(
                    "[foundry-asr] release_now waited {waited_ms} ms for lifecycle lock (in-flight transcribe/download)"
                );
            }
            self.release_now_locked().await
        }

        /// 为新录音或重转录会话分配 route，并立即使旧恢复/释放任务失效。
        pub(crate) fn begin_route(&self) -> FoundryRouteEpoch {
            self.advance_route_epoch()
        }

        pub(crate) fn route_epoch_snapshot(&self) -> FoundryRouteEpoch {
            FoundryRouteEpoch(self.route_epoch.load(Ordering::SeqCst))
        }

        /// 使已调度的恢复/释放任务失效；用于 alias 或 runtime source 切换。
        pub fn invalidate_route(&self) {
            self.advance_route_epoch();
        }

        pub(crate) async fn release_if_route_epoch(
            &self,
            expected_epoch: FoundryRouteEpoch,
        ) -> Result<bool> {
            let _lifecycle = self.lifecycle.lock().await;
            if self.route_epoch_snapshot() != expected_epoch {
                return Ok(false);
            }
            self.release_now_locked().await?;
            self.advance_route_epoch();
            Ok(true)
        }

        /// 等到 native lifecycle 锁之后再次校验 Host 的使用代次。仅在排队前检查会
        /// 留下 TOCTOU：等待旧推理释放锁期间，新会话/设置页已经开始加载新模型。
        pub(crate) async fn release_if_generation(
            &self,
            generation: &AtomicU64,
            expected: u64,
        ) -> Result<bool> {
            let _lifecycle = self.lifecycle.lock().await;
            if generation.load(Ordering::Acquire) != expected {
                return Ok(false);
            }
            self.release_now_locked().await?;
            self.advance_route_epoch();
            Ok(true)
        }

        /// 取消当前录音时仅清理精确匹配的临时 CPU 模型；正常 alias 模型仍遵循用户已有的
        /// 保活设置。该方法会等待在途下载/加载/推理释放 lifecycle 锁。
        pub async fn release_temporary_cpu_fallback(
            &self,
            cancelled_lease: FoundryTemporaryCpuFallbackLease,
        ) -> Result<()> {
            let _lifecycle = self.lifecycle.lock().await;
            let temporary_cpu = self.loaded_model_snapshot().filter(|loaded| {
                should_release_temporary_cpu_fallback(
                    loaded.temporary_cpu_fallback_lease,
                    cancelled_lease,
                )
            });
            if let Some(loaded) = temporary_cpu {
                Self::unload_model(&loaded).await?;
                self.clear_loaded_if_model_id(&loaded.model_id);
                log::info!(
                    "[foundry-asr] event=cpu_fallback_released alias={} cpu_model={}",
                    loaded.alias,
                    loaded.model_id
                );
            }
            Ok(())
        }

        pub fn storage_configuration_locked(&self) -> bool {
            self.state.lock().manager.is_some()
        }

        pub async fn model_dir_for_alias(&self, alias: &str) -> Result<PathBuf> {
            let _lifecycle = self.lifecycle.lock().await;
            if self.state.lock().manager.is_none() {
                return crate::persistence::foundry_model_cache_root();
            }
            let manager = self.manager()?;
            let model = manager
                .catalog()
                .get_model(alias)
                .await
                .with_context(|| format!("get Foundry model {alias}"))?;
            model
                .path()
                .await
                .with_context(|| format!("get Foundry model path {alias}"))
        }

        pub async fn inspect_models(
            &self,
            aliases: &[String],
        ) -> Result<Vec<super::FoundryNativeModelState>> {
            let _lifecycle = self.lifecycle.lock().await;
            if !foundry_native::runtime_ready() {
                return Ok(aliases
                    .iter()
                    .map(|alias| super::FoundryNativeModelState {
                        alias: alias.clone(),
                        cached: false,
                        size_bytes: None,
                        display_name: None,
                    })
                    .collect());
            }
            let manager = self.manager()?;
            let mut states = Vec::with_capacity(aliases.len());
            for alias in aliases {
                let model = manager
                    .catalog()
                    .get_model(alias)
                    .await
                    .with_context(|| format!("get Foundry catalog model {alias}"))?;
                let info = model.info();
                let cached = model.is_cached().await.unwrap_or(info.cached);
                states.push(super::FoundryNativeModelState {
                    alias: alias.clone(),
                    cached,
                    size_bytes: info
                        .file_size_mb
                        .map(|size| size.saturating_mul(1024 * 1024)),
                    display_name: info.display_name.clone(),
                });
            }
            Ok(states)
        }

        pub async fn delete_model(&self, alias: &str) -> Result<()> {
            self.advance_route_epoch();
            let _lifecycle = self.lifecycle.lock().await;
            let manager = self.manager()?;
            let model = manager
                .catalog()
                .get_model(alias)
                .await
                .with_context(|| format!("get Foundry model {alias}"))?;
            let loaded = self
                .loaded_model_snapshot()
                .filter(|loaded| loaded.alias == alias);
            if let Some(loaded) = loaded.as_ref() {
                Self::unload_model(loaded).await?;
                self.clear_loaded_if_model_id(&loaded.model_id);
            }
            model
                .remove_from_cache()
                .await
                .with_context(|| format!("remove Foundry model cache {alias}"))?;
            self.state.lock().primary_by_alias.remove(alias);
            Ok(())
        }

        async fn ensure_loaded_locked(
            &self,
            alias: &str,
            runtime_source: RuntimeSource,
            progress: FoundryPrepareProgressCallback,
        ) -> Result<LoadedModel> {
            if let Some(loaded) = self.cached_loaded_model(alias) {
                progress.as_ref()(FoundryPrepareProgressPayload::finished(
                    alias,
                    "Foundry model already loaded",
                ));
                return Ok(loaded);
            }

            let previous_loaded = self.loaded_for_replacement(alias);

            self.check_prepare_cancelled()?;
            foundry_native::ensure_runtime(runtime_source, {
                let progress = Arc::clone(&progress);
                let alias = alias.to_string();
                move |label, percent| {
                    progress.as_ref()(FoundryPrepareProgressPayload::runtime(
                        alias.clone(),
                        label.to_string(),
                        percent,
                    ));
                }
            })
            .await
            .context("download Foundry Local native runtime")?;
            self.check_prepare_cancelled()?;
            let manager = self.manager()?;
            progress.as_ref()(FoundryPrepareProgressPayload::runtime(
                alias,
                "Foundry Local runtime components",
                0.0,
            ));
            let runtime_progress = Arc::clone(&progress);
            let runtime_alias = alias.to_string();
            manager
                .download_and_register_eps_with_progress(
                    None,
                    move |ep_name: &str, percent: f64| {
                        let label = if ep_name.trim().is_empty() {
                            "Foundry Local runtime components".to_string()
                        } else {
                            format!("Foundry Local runtime component: {ep_name}")
                        };
                        runtime_progress.as_ref()(FoundryPrepareProgressPayload::runtime(
                            runtime_alias.clone(),
                            label,
                            percent,
                        ));
                    },
                )
                .await
                .context("download/register Foundry execution providers")?;
            progress.as_ref()(FoundryPrepareProgressPayload::runtime(
                alias,
                "Foundry Local runtime components",
                100.0,
            ));
            self.check_prepare_cancelled()?;

            let preferred_primary = self.preferred_primary_model(manager, alias).await;
            let mut model = match preferred_primary {
                Some(model) => model,
                None => manager
                    .catalog()
                    .get_model(alias)
                    .await
                    .with_context(|| format!("get Foundry model {alias}"))?,
            };
            let using_recorded_primary = self
                .primary_model_snapshot(alias)
                .is_some_and(|primary| primary.model_id == model.id());

            let model_label = model_display_label(alias);
            if !model
                .is_cached()
                .await
                .context("check Foundry model cache")?
            {
                progress.as_ref()(FoundryPrepareProgressPayload::model(
                    alias,
                    model_label.clone(),
                    0.0,
                ));
                let model_progress = Arc::clone(&progress);
                let model_alias = alias.to_string();
                let model_label_for_progress = model_label.clone();
                model
                    .download(Some(move |percent: f64| {
                        model_progress.as_ref()(FoundryPrepareProgressPayload::model(
                            model_alias.clone(),
                            model_label_for_progress.clone(),
                            percent,
                        ));
                    }))
                    .await
                    .with_context(|| format!("download Foundry model {alias}"))?;
                progress.as_ref()(FoundryPrepareProgressPayload::model(
                    alias,
                    model_label.clone(),
                    100.0,
                ));
            } else {
                progress.as_ref()(FoundryPrepareProgressPayload::model(
                    alias,
                    format!("{model_label} already downloaded"),
                    100.0,
                ));
            }

            self.check_prepare_cancelled()?;
            progress.as_ref()(FoundryPrepareProgressPayload::load(
                alias,
                model_label.clone(),
                0.0,
            ));
            let model_id = model.id().to_string();
            if previous_loaded
                .as_ref()
                .is_some_and(|previous| previous.model_id == model_id)
            {
                progress.as_ref()(FoundryPrepareProgressPayload::load(
                    alias,
                    model_label.clone(),
                    100.0,
                ));
                let loaded = LoadedModel::new(alias, model, None);
                self.set_primary_loaded(manager, loaded.clone());
                progress.as_ref()(FoundryPrepareProgressPayload::finished(
                    alias,
                    format!("{model_label} ready"),
                ));
                return Ok(loaded);
            }

            let unloaded_previous = if let Some(previous) = previous_loaded.as_ref() {
                Self::unload_model(previous).await?;
                self.clear_loaded_if_model_id(&previous.model_id);
                Some(previous.clone())
            } else {
                None
            };
            if let Err(error) = self.check_prepare_cancelled() {
                self.rollback_prepare_error(manager, unloaded_previous.as_ref(), alias, error)
                    .await?;
            }
            if let Err(error) = model
                .load()
                .await
                .with_context(|| format!("load Foundry model {alias}"))
            {
                if using_recorded_primary {
                    let failed_primary_id = model.id().to_string();
                    self.clear_primary_if_model_id(alias, &failed_primary_id);
                    let alias_model = manager
                        .catalog()
                        .get_model(alias)
                        .await
                        .with_context(|| format!("reselect Foundry model {alias}"))?;
                    if alias_model.id() == failed_primary_id {
                        self.rollback_prepare_error(
                            manager,
                            unloaded_previous.as_ref(),
                            alias,
                            error,
                        )
                        .await?;
                    }
                    log::warn!(
                        "[foundry-asr] recorded primary {} failed to load; reselected {} for alias {}",
                        failed_primary_id,
                        alias_model.id(),
                        alias
                    );
                    model = alias_model;
                    if !model
                        .is_cached()
                        .await
                        .context("check reselected Foundry model cache")?
                    {
                        model.download(None::<fn(f64)>).await.with_context(|| {
                            format!("download reselected Foundry model {alias}")
                        })?;
                    }
                    if let Err(reselect_error) = model
                        .load()
                        .await
                        .with_context(|| format!("load reselected Foundry model {alias}"))
                    {
                        self.rollback_prepare_error(
                            manager,
                            unloaded_previous.as_ref(),
                            alias,
                            reselect_error,
                        )
                        .await?;
                    }
                } else {
                    self.rollback_prepare_error(manager, unloaded_previous.as_ref(), alias, error)
                        .await?;
                }
            }
            if self.cancel_prepare.load(Ordering::SeqCst) {
                if let Err(error) = model
                    .unload()
                    .await
                    .with_context(|| format!("unload cancelled Foundry model {alias}"))
                {
                    self.rollback_prepare_error(manager, unloaded_previous.as_ref(), alias, error)
                        .await?;
                }
                self.rollback_prepare_error(
                    manager,
                    unloaded_previous.as_ref(),
                    alias,
                    anyhow::anyhow!("Foundry Local Whisper prepare cancelled"),
                )
                .await?;
            }
            progress.as_ref()(FoundryPrepareProgressPayload::load(
                alias,
                model_label.clone(),
                100.0,
            ));

            let loaded = LoadedModel::new(alias, model, None);
            self.set_primary_loaded(manager, loaded.clone());
            progress.as_ref()(FoundryPrepareProgressPayload::finished(
                alias,
                format!("{model_label} ready"),
            ));
            Ok(loaded)
        }

        async fn release_now_locked(&self) -> Result<()> {
            if let Some(loaded) = self.loaded_model_snapshot() {
                Self::unload_model(&loaded).await?;
                self.clear_loaded_if_model_id(&loaded.model_id);
            }
            Ok(())
        }

        /// 成功 CPU 回退后按原 route 恢复精确 primary variant。
        ///
        /// 新准备/转写会在等待 lifecycle 锁之前推进 epoch，因此旧恢复任务不会覆盖新会话。
        pub async fn restore_primary_for_keep_alive(
            &self,
            token: &FoundryPrimaryRecoveryToken,
        ) -> Result<bool> {
            let _lifecycle = self.lifecycle.lock().await;
            if !self.route_is_current(token) {
                return Ok(false);
            }

            if let Some(loaded) = self.loaded_model_snapshot() {
                if loaded.model_id == token.primary_model_id && !loaded.is_temporary_cpu_fallback()
                {
                    return Ok(true);
                }
                if loaded.is_temporary_cpu_fallback() {
                    Self::unload_model(&loaded).await?;
                    self.clear_loaded_if_model_id(&loaded.model_id);
                } else {
                    return Ok(false);
                }
            }

            let Some(primary) = self
                .primary_model_snapshot(&token.alias)
                .filter(|primary| primary.model_id == token.primary_model_id)
            else {
                return Ok(false);
            };
            let manager = self.manager()?;
            if primary.device != FoundryExecutionDevice::Gpu
                || primary
                    .execution_provider
                    .as_deref()
                    .is_none_or(|provider| !provider.eq_ignore_ascii_case("CUDAExecutionProvider"))
            {
                self.clear_primary_if_model_id(&token.alias, &token.primary_model_id);
                return Ok(false);
            }
            if let Err(error) = manager
                .catalog()
                .get_model_variant(&token.primary_model_id)
                .await
            {
                self.clear_primary_if_model_id(&token.alias, &token.primary_model_id);
                log::warn!(
                    "[foundry-asr] primary recovery variant disappeared alias={} model={}: {error:#}",
                    token.alias,
                    token.primary_model_id
                );
                return Ok(false);
            }
            let model = Arc::clone(&primary.model);
            if let Err(error) = model.load().await.with_context(|| {
                format!("restore Foundry primary model {}", token.primary_model_id)
            }) {
                self.clear_primary_if_model_id(&token.alias, &token.primary_model_id);
                return Err(error);
            }
            let loaded = LoadedModel::new(&primary.alias, model, None);
            if !self.route_is_current(token) {
                if let Err(error) = Self::unload_model(&loaded).await {
                    let mut state = self.state.lock();
                    state.manager = Some(manager);
                    state.loaded = Some(loaded);
                    return Err(error.context("unload stale restored Foundry primary model"));
                }
                return Ok(false);
            }
            self.set_primary_loaded(manager, loaded);
            Ok(true)
        }

        /// 仅当恢复令牌仍代表当前 route 时释放 primary；用于保活截止任务。
        pub async fn release_primary_if_current(
            &self,
            token: &FoundryPrimaryRecoveryToken,
        ) -> Result<bool> {
            let _lifecycle = self.lifecycle.lock().await;
            if !self.route_is_current(token) {
                return Ok(false);
            }
            let Some(loaded) = self.loaded_model_snapshot().filter(|loaded| {
                loaded.model_id == token.primary_model_id && !loaded.is_temporary_cpu_fallback()
            }) else {
                return Ok(false);
            };
            Self::unload_model(&loaded).await?;
            self.clear_loaded_if_model_id(&loaded.model_id);
            self.advance_route_epoch();
            Ok(true)
        }

        async fn restore_loaded_model(
            &self,
            manager: &'static FoundryLocalManager,
            loaded: &LoadedModel,
        ) -> Result<()> {
            loaded
                .model
                .load()
                .await
                .with_context(|| format!("restore Foundry model {}", loaded.model_id))?;
            if loaded.is_temporary_cpu_fallback() {
                let mut state = self.state.lock();
                state.manager = Some(manager);
                state.loaded = Some(loaded.clone());
            } else {
                self.set_primary_loaded(manager, loaded.clone());
            }
            Ok(())
        }

        async fn rollback_prepare_error(
            &self,
            manager: &'static FoundryLocalManager,
            previous: Option<&LoadedModel>,
            alias: &str,
            error: anyhow::Error,
        ) -> Result<()> {
            if let Some(previous) = previous {
                if let Err(restore_error) = self.restore_loaded_model(manager, previous).await {
                    return Err(error).with_context(|| {
                        format!(
                            "prepare Foundry model {alias} failed; also failed to restore previous Foundry model {}: {restore_error:#}",
                            previous.model_id
                        )
                    });
                }
            }
            Err(error)
        }

        async fn cpu_variant_model(
            &self,
            manager: &'static FoundryLocalManager,
            alias: &str,
        ) -> Result<Arc<Model>> {
            let catalog = manager.catalog();
            let model = catalog
                .get_model(alias)
                .await
                .with_context(|| format!("get Foundry model variants for {alias}"))?;
            let variants = model
                .variants()
                .into_iter()
                .map(|variant| {
                    FoundryVariantDescriptor::new(
                        variant.id(),
                        variant.info().version,
                        FoundryExecutionDevice::from_model(&variant),
                    )
                })
                .collect::<Vec<_>>();
            let cpu_variant_id = select_cpu_variant_id(&variants).with_context(|| {
                format!("Foundry model {alias} has no CPU variant for CUDA fallback")
            })?;
            catalog
                .get_model_variant(&cpu_variant_id)
                .await
                .with_context(|| format!("get Foundry CPU model variant {cpu_variant_id}"))
        }

        async fn preferred_primary_model(
            &self,
            manager: &'static FoundryLocalManager,
            alias: &str,
        ) -> Option<Arc<Model>> {
            let primary = self.primary_model_snapshot(alias)?;
            match manager.catalog().get_model_variant(&primary.model_id).await {
                Ok(model) => Some(model),
                Err(error) => {
                    log::warn!(
                        "[foundry-asr] recorded primary variant unavailable alias={} model={}: {error:#}",
                        alias,
                        primary.model_id
                    );
                    self.clear_primary_if_model_id(alias, &primary.model_id);
                    None
                }
            }
        }

        fn cached_loaded_model(&self, alias: &str) -> Option<LoadedModel> {
            self.state
                .lock()
                .loaded
                .as_ref()
                .filter(|loaded| {
                    may_reuse_loaded_model(&loaded.alias, alias, loaded.is_temporary_cpu_fallback())
                })
                .cloned()
        }

        fn manager(&self) -> Result<&'static FoundryLocalManager> {
            if let Some(manager) = self.state.lock().manager {
                return Ok(manager);
            }

            let manager = FoundryLocalManager::create(self.manager_config())
                .context("initialize Foundry Local manager")?;
            self.state.lock().manager = Some(manager);
            Ok(manager)
        }

        fn manager_config(&self) -> FoundryLocalConfig {
            // OpenLess owns Windows App Runtime installation; keep the SDK bootstrapper non-interactive.
            let mut config =
                FoundryLocalConfig::new("openless").additional_setting("Bootstrap", "false");
            if let Ok(dir) = crate::persistence::foundry_app_data_root() {
                config = config.app_data_dir(dir.to_string_lossy().to_string());
            }
            if let Ok(dir) = crate::persistence::foundry_model_cache_root() {
                config = config.model_cache_dir(dir.to_string_lossy().to_string());
            }
            if let Ok(dir) = crate::persistence::foundry_logs_root() {
                config = config.logs_dir(dir.to_string_lossy().to_string());
            }
            let runtime_dir = foundry_native::runtime_dir().ok();
            let candidates = foundry_native_dir_candidates(runtime_dir.as_deref());
            if let Some(native_dir) = select_foundry_native_dir(candidates) {
                config = config.library_path(native_dir.to_string_lossy().to_string());
            }
            config
        }

        fn loaded_model_snapshot(&self) -> Option<LoadedModel> {
            self.state.lock().loaded.clone()
        }

        fn primary_model_snapshot(&self, alias: &str) -> Option<PrimaryModel> {
            self.state.lock().primary_by_alias.get(alias).cloned()
        }

        fn set_primary_loaded(&self, manager: &'static FoundryLocalManager, loaded: LoadedModel) {
            debug_assert!(!loaded.is_temporary_cpu_fallback());
            let primary = PrimaryModel::from_loaded(&loaded);
            let mut state = self.state.lock();
            state.manager = Some(manager);
            state
                .primary_by_alias
                .insert(primary.alias.clone(), primary);
            state.loaded = Some(loaded);
        }

        fn clear_primary_if_model_id(&self, alias: &str, model_id: &str) {
            let mut state = self.state.lock();
            if state
                .primary_by_alias
                .get(alias)
                .is_some_and(|primary| primary.model_id == model_id)
            {
                state.primary_by_alias.remove(alias);
            }
        }

        fn loaded_for_replacement(&self, alias: &str) -> Option<LoadedModel> {
            self.state
                .lock()
                .loaded
                .as_ref()
                .filter(|loaded| {
                    !may_reuse_loaded_model(
                        &loaded.alias,
                        alias,
                        loaded.is_temporary_cpu_fallback(),
                    )
                })
                .cloned()
        }

        fn clear_loaded_if_model_id(&self, model_id: &str) {
            let mut state = self.state.lock();
            if state
                .loaded
                .as_ref()
                .is_some_and(|loaded| loaded.model_id == model_id)
            {
                state.loaded.take();
            }
        }

        async fn unload_model(loaded: &LoadedModel) -> Result<()> {
            loaded
                .model
                .unload()
                .await
                .with_context(|| format!("unload Foundry model {}", loaded.model_id))?;
            Ok(())
        }

        fn check_prepare_cancelled(&self) -> Result<()> {
            if self.cancel_prepare.load(Ordering::SeqCst) {
                anyhow::bail!("Foundry Local Whisper prepare cancelled");
            }
            Ok(())
        }

        fn next_temporary_cpu_fallback_lease(&self) -> FoundryTemporaryCpuFallbackLease {
            FoundryTemporaryCpuFallbackLease(
                self.temporary_cpu_fallback_sequence
                    .fetch_add(1, Ordering::SeqCst)
                    .wrapping_add(1),
            )
        }

        fn advance_route_epoch(&self) -> FoundryRouteEpoch {
            FoundryRouteEpoch(
                self.route_epoch
                    .fetch_add(1, Ordering::SeqCst)
                    .wrapping_add(1),
            )
        }

        fn route_is_current(&self, token: &FoundryPrimaryRecoveryToken) -> bool {
            self.route_epoch_snapshot() == token.route_epoch
        }
    }

    fn model_display_label(alias: &str) -> String {
        alias.to_string()
    }

    fn normalized_language_hint(language_hint: Option<&str>) -> Option<String> {
        language_hint
            .map(str::trim)
            .filter(|hint| !hint.is_empty())
            .map(str::to_string)
    }

    fn foundry_native_dir_candidates(runtime_dir: Option<&Path>) -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        if let Some(runtime_dir) = runtime_dir {
            candidates.push(runtime_dir.to_path_buf());
        }

        candidates
    }

    fn select_foundry_native_dir(candidates: Vec<PathBuf>) -> Option<PathBuf> {
        candidates
            .into_iter()
            .find(|dir| dir.join("Microsoft.AI.Foundry.Local.Core.dll").exists())
    }

    #[cfg(test)]
    mod lifecycle_tests {
        #[tokio::test]
        async fn release_generation_is_rechecked_after_waiting_for_native_lifecycle() {
            let runtime = super::FoundryLocalRuntime::new();
            let generation = std::sync::atomic::AtomicU64::new(1);
            let guard = runtime.lifecycle.lock().await;
            let release = runtime.release_if_generation(&generation, 1);
            tokio::pin!(release);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(1), &mut release)
                    .await
                    .is_err()
            );
            generation.store(2, std::sync::atomic::Ordering::Release);
            drop(guard);
            assert!(
                !release.await.unwrap(),
                "stale cleanup must leave the newly-owned runtime intact"
            );
        }

        use super::{
            cpu_load_completion, foundry_native_dir_candidates, is_cuda_cudnn_failure,
            is_cuda_fallback_candidate, may_reuse_loaded_model, normalized_language_hint,
            select_cpu_variant_id, select_foundry_native_dir,
            should_release_temporary_cpu_fallback, transcribe_recording_with_adapter,
            FoundryCpuLoadCompletion, FoundryCpuSwitch, FoundryExecutionAdapter,
            FoundryExecutionDevice, FoundryFallbackNotice, FoundryFallbackNoticeCallback,
            FoundryLocalRuntime, FoundryVariantDescriptor,
        };
        use anyhow::Result;
        use std::{
            collections::VecDeque,
            fs,
            path::{Path, PathBuf},
            sync::Arc,
            time::Duration,
        };

        enum ScriptedTranscription {
            Text(&'static str),
            Error(&'static str),
        }

        enum ScriptedCpuSwitch {
            Success {
                model_id: &'static str,
                download_required: bool,
            },
            Error(&'static str),
        }

        struct ScriptedExecution {
            device: FoundryExecutionDevice,
            execution_provider: Option<&'static str>,
            model_id: String,
            transcriptions: VecDeque<ScriptedTranscription>,
            cpu_switch: Option<ScriptedCpuSwitch>,
            transcribe_devices: Vec<FoundryExecutionDevice>,
            transcribe_timeouts: Vec<Duration>,
            cpu_switch_delay: Duration,
            switch_count: usize,
            finish_count: usize,
            released_temporary_cpu: bool,
        }

        impl ScriptedExecution {
            fn gpu(
                transcriptions: impl IntoIterator<Item = ScriptedTranscription>,
                cpu_switch: ScriptedCpuSwitch,
            ) -> Self {
                Self {
                    device: FoundryExecutionDevice::Gpu,
                    execution_provider: Some("CUDAExecutionProvider"),
                    model_id: "whisper-medium-gpu:4".to_string(),
                    transcriptions: transcriptions.into_iter().collect(),
                    cpu_switch: Some(cpu_switch),
                    transcribe_devices: Vec::new(),
                    transcribe_timeouts: Vec::new(),
                    cpu_switch_delay: Duration::ZERO,
                    switch_count: 0,
                    finish_count: 0,
                    released_temporary_cpu: false,
                }
            }

            fn with_cpu_switch_delay(mut self, delay: Duration) -> Self {
                self.cpu_switch_delay = delay;
                self
            }
        }

        impl FoundryExecutionAdapter for ScriptedExecution {
            fn alias(&self) -> &str {
                "whisper-medium"
            }

            fn execution_device(&self) -> FoundryExecutionDevice {
                self.device
            }

            fn execution_provider(&self) -> Option<&str> {
                self.execution_provider
            }

            fn model_id(&self) -> &str {
                &self.model_id
            }

            async fn transcribe(
                &mut self,
                _audio_path: &Path,
                timeout: Duration,
            ) -> Result<String> {
                self.transcribe_devices.push(self.device);
                self.transcribe_timeouts.push(timeout);
                match self
                    .transcriptions
                    .pop_front()
                    .expect("test script must provide every transcription result")
                {
                    ScriptedTranscription::Text(text) => Ok(text.to_string()),
                    ScriptedTranscription::Error(error) => anyhow::bail!("{error}"),
                }
            }

            async fn switch_to_cpu(
                &mut self,
                notices: &FoundryFallbackNoticeCallback,
            ) -> Result<FoundryCpuSwitch> {
                self.switch_count += 1;
                if !self.cpu_switch_delay.is_zero() {
                    tokio::time::sleep(self.cpu_switch_delay).await;
                }
                match self
                    .cpu_switch
                    .take()
                    .expect("CPU switch may only be attempted once")
                {
                    ScriptedCpuSwitch::Success {
                        model_id,
                        download_required,
                    } => {
                        if download_required {
                            notices(FoundryFallbackNotice::DownloadingCpu);
                        }
                        self.device = FoundryExecutionDevice::Cpu;
                        self.execution_provider = Some("CPUExecutionProvider");
                        self.model_id = model_id.to_string();
                        Ok(FoundryCpuSwitch {
                            model_id: model_id.to_string(),
                            cache_hit: !download_required,
                        })
                    }
                    ScriptedCpuSwitch::Error(error) => anyhow::bail!("{error}"),
                }
            }

            async fn finish(&mut self) -> Result<()> {
                self.finish_count += 1;
                self.released_temporary_cpu = self.device == FoundryExecutionDevice::Cpu;
                Ok(())
            }
        }

        fn audio_paths(count: usize) -> Vec<PathBuf> {
            (1..=count)
                .map(|index| PathBuf::from(format!("chunk-{index}.wav")))
                .collect()
        }

        fn notices() -> (
            FoundryFallbackNoticeCallback,
            Arc<std::sync::Mutex<Vec<FoundryFallbackNotice>>>,
        ) {
            let received = Arc::new(std::sync::Mutex::new(Vec::new()));
            let callback_received = Arc::clone(&received);
            let callback: FoundryFallbackNoticeCallback = Arc::new(move |notice| {
                callback_received.lock().unwrap().push(notice);
            });
            (callback, received)
        }

        #[tokio::test]
        async fn cuda_failure_retries_the_failed_chunk_once_on_cpu_and_keeps_cpu_for_later_chunks()
        {
            let mut execution = ScriptedExecution::gpu(
                [
                    ScriptedTranscription::Text("first"),
                    ScriptedTranscription::Error("CUDNN_FE failure 11: CUDNN_BACKEND_API_FAILED"),
                    ScriptedTranscription::Text("second"),
                    ScriptedTranscription::Text("third"),
                ],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: true,
                },
            );
            let (callback, received) = notices();

            let outcome = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(3),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap();

            assert_eq!(outcome.texts, ["first", "second", "third"]);
            assert!(outcome.used_cpu_fallback);
            assert_eq!(
                outcome.gpu_model_id.as_deref(),
                Some("whisper-medium-gpu:4")
            );
            assert_eq!(
                outcome.cpu_model_id.as_deref(),
                Some("whisper-medium-cpu:4")
            );
            assert_eq!(execution.switch_count, 1);
            assert_eq!(
                execution.transcribe_devices,
                [
                    FoundryExecutionDevice::Gpu,
                    FoundryExecutionDevice::Gpu,
                    FoundryExecutionDevice::Cpu,
                    FoundryExecutionDevice::Cpu,
                ]
            );
            assert_eq!(execution.finish_count, 1);
            assert!(execution.released_temporary_cpu);
            assert_eq!(
                *received.lock().unwrap(),
                [
                    FoundryFallbackNotice::SwitchingToCpu,
                    FoundryFallbackNotice::DownloadingCpu,
                ]
            );
        }

        #[tokio::test]
        async fn cpu_retry_receives_a_fresh_inference_budget_after_model_preparation() {
            let mut execution = ScriptedExecution::gpu(
                [
                    ScriptedTranscription::Error("CUDNN_BACKEND_API_FAILED during GPU inference"),
                    ScriptedTranscription::Text("recovered"),
                ],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: true,
                },
            )
            .with_cpu_switch_delay(Duration::from_millis(100));
            let (callback, _) = notices();

            let outcome = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_millis(150),
                &callback,
            )
            .await
            .unwrap();

            assert_eq!(outcome.texts, ["recovered"]);
            assert_eq!(execution.transcribe_timeouts.len(), 2);
            assert!(
                execution.transcribe_timeouts[1] >= Duration::from_millis(120),
                "CPU retry should retain a fresh dynamic inference budget after preparation"
            );
        }

        #[tokio::test]
        async fn non_cuda_failure_keeps_the_existing_error_path_without_cpu_fallback() {
            let mut execution = ScriptedExecution::gpu(
                [ScriptedTranscription::Error("network request timed out")],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: false,
                },
            );
            let (callback, received) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(error.to_string().contains("network request timed out"));
            assert!(!super::super::is_terminal_foundry_fallback_error(&error));
            assert_eq!(execution.switch_count, 0);
            assert_eq!(execution.finish_count, 1);
            assert!(!execution.released_temporary_cpu);
            assert!(received.lock().unwrap().is_empty());
        }

        #[tokio::test]
        async fn cuda_signature_on_a_non_gpu_variant_does_not_trigger_cpu_fallback() {
            let mut execution = ScriptedExecution::gpu(
                [ScriptedTranscription::Error(
                    "CUDNN_FE failure 11: CUDNN_BACKEND_API_FAILED",
                )],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: false,
                },
            );
            execution.device = FoundryExecutionDevice::Cpu;
            execution.model_id = "whisper-medium-cpu:4".to_string();
            let (callback, _) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(!super::super::is_terminal_foundry_fallback_error(&error));
            assert_eq!(execution.switch_count, 0);
            assert_eq!(execution.finish_count, 1);
        }

        #[tokio::test]
        async fn cudnn_signature_on_a_non_cuda_gpu_does_not_trigger_cpu_fallback() {
            let mut execution = ScriptedExecution::gpu(
                [ScriptedTranscription::Error(
                    "CUDNN_FE failure 11: CUDNN_BACKEND_API_FAILED",
                )],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: false,
                },
            );
            execution.execution_provider = Some("WebGpuExecutionProvider");
            let (callback, _) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(error.to_string().contains("CUDNN_FE failure"));
            assert_eq!(execution.switch_count, 0);
            assert_eq!(execution.finish_count, 1);
        }

        #[tokio::test]
        async fn unavailable_cpu_variant_is_a_terminal_fallback_error_without_a_second_gpu_attempt()
        {
            let mut execution = ScriptedExecution::gpu(
                [ScriptedTranscription::Error(
                    "Failed to initialize CUDNN Frontend",
                )],
                ScriptedCpuSwitch::Error("Foundry model whisper-medium has no CPU variant"),
            );
            let (callback, _) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(super::super::is_terminal_foundry_fallback_error(&error));
            assert!(error
                .to_string()
                .contains("Failed to initialize CUDNN Frontend"));
            assert!(error.to_string().contains("has no CPU variant"));
            assert_eq!(execution.switch_count, 1);
            assert_eq!(execution.transcribe_devices, [FoundryExecutionDevice::Gpu]);
            assert_eq!(execution.finish_count, 1);
        }

        #[tokio::test]
        async fn cpu_download_failure_is_terminal_and_does_not_retry_the_gpu_chunk() {
            let mut execution = ScriptedExecution::gpu(
                [ScriptedTranscription::Error(
                    "CUDNN_BACKEND_API_FAILED during GPU inference",
                )],
                ScriptedCpuSwitch::Error("download Foundry CPU model whisper-medium-cpu:4 failed"),
            );
            let (callback, _) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(super::super::is_terminal_foundry_fallback_error(&error));
            assert!(error.to_string().contains("download Foundry CPU model"));
            assert_eq!(execution.switch_count, 1);
            assert_eq!(execution.transcribe_devices, [FoundryExecutionDevice::Gpu]);
            assert_eq!(execution.finish_count, 1);
        }

        #[tokio::test]
        async fn cpu_transcription_failure_is_terminal_and_still_releases_the_cpu_model() {
            let mut execution = ScriptedExecution::gpu(
                [
                    ScriptedTranscription::Error(
                        "Could not locate cudnn_engines_precompiled64_9.dll",
                    ),
                    ScriptedTranscription::Error("CPU model inference failed"),
                ],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: false,
                },
            );
            let (callback, _) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(super::super::is_terminal_foundry_fallback_error(&error));
            assert!(error.to_string().contains("CPU model inference failed"));
            assert_eq!(execution.switch_count, 1);
            assert_eq!(
                execution.transcribe_devices,
                [FoundryExecutionDevice::Gpu, FoundryExecutionDevice::Cpu]
            );
            assert_eq!(execution.finish_count, 1);
            assert!(execution.released_temporary_cpu);
        }

        #[tokio::test]
        async fn cpu_download_or_load_cancellation_is_terminal_and_leaves_no_cpu_route() {
            for stage in ["CPU download", "CPU load"] {
                let mut execution = ScriptedExecution::gpu(
                    [ScriptedTranscription::Error(
                        "CUDNN_BACKEND_API_FAILED during GPU inference",
                    )],
                    ScriptedCpuSwitch::Error(match stage {
                        "CPU download" => {
                            "Foundry Local Whisper prepare cancelled during CPU download"
                        }
                        "CPU load" => "Foundry Local Whisper prepare cancelled during CPU load",
                        _ => unreachable!(),
                    }),
                );
                let (callback, _) = notices();

                let error = transcribe_recording_with_adapter(
                    &mut execution,
                    &audio_paths(1),
                    Duration::from_secs(30),
                    &callback,
                )
                .await
                .unwrap_err();

                assert!(super::super::is_terminal_foundry_fallback_error(&error));
                assert!(error.to_string().contains(stage));
                assert_eq!(execution.finish_count, 1);
                assert!(!execution.released_temporary_cpu);
            }
        }

        #[test]
        fn cpu_load_completion_never_restores_primary_after_cancellation() {
            assert_eq!(
                cpu_load_completion(true, true),
                FoundryCpuLoadCompletion::Cancelled
            );
            assert_eq!(
                cpu_load_completion(false, true),
                FoundryCpuLoadCompletion::Cancelled
            );
            assert_eq!(
                cpu_load_completion(false, false),
                FoundryCpuLoadCompletion::RestorePrimary
            );
            assert_eq!(
                cpu_load_completion(true, false),
                FoundryCpuLoadCompletion::UseCpu
            );
        }

        #[tokio::test]
        async fn cpu_inference_cancellation_releases_the_temporary_cpu_route_without_text() {
            let mut execution = ScriptedExecution::gpu(
                [
                    ScriptedTranscription::Error("CUDNN_BACKEND_API_FAILED during GPU inference"),
                    ScriptedTranscription::Error("Foundry Local Whisper transcription cancelled"),
                ],
                ScriptedCpuSwitch::Success {
                    model_id: "whisper-medium-cpu:4",
                    download_required: false,
                },
            );
            let (callback, _) = notices();

            let error = transcribe_recording_with_adapter(
                &mut execution,
                &audio_paths(1),
                Duration::from_secs(30),
                &callback,
            )
            .await
            .unwrap_err();

            assert!(super::super::is_terminal_foundry_fallback_error(&error));
            assert!(error.to_string().contains("transcription cancelled"));
            assert_eq!(execution.finish_count, 1);
            assert!(execution.released_temporary_cpu);
        }

        #[test]
        fn cuda_cudnn_failure_classifier_requires_a_stable_cuda_signature() {
            assert!(is_cuda_cudnn_failure(
                "CUDNN_FE failure 11: CUDNN_BACKEND_API_FAILED"
            ));
            assert!(is_cuda_cudnn_failure("Failed to initialize CUDNN Frontend"));
            assert!(is_cuda_cudnn_failure(
                "Could not locate cudnn_engines_precompiled64_9.dll"
            ));
            assert!(!is_cuda_cudnn_failure("audio file could not be decoded"));
            assert!(!is_cuda_cudnn_failure("request timed out"));
        }

        #[test]
        fn cuda_fallback_requires_the_cuda_execution_provider() {
            let cudnn_error = "CUDNN_FE failure 11: CUDNN_BACKEND_API_FAILED";

            assert!(is_cuda_fallback_candidate(
                FoundryExecutionDevice::Gpu,
                Some("CUDAExecutionProvider"),
                cudnn_error,
            ));
            assert!(!is_cuda_fallback_candidate(
                FoundryExecutionDevice::Gpu,
                Some("WebGpuExecutionProvider"),
                cudnn_error,
            ));
            assert!(!is_cuda_fallback_candidate(
                FoundryExecutionDevice::Gpu,
                Some("OpenVINOExecutionProvider"),
                cudnn_error,
            ));
            assert!(!is_cuda_fallback_candidate(
                FoundryExecutionDevice::Cpu,
                Some("CPUExecutionProvider"),
                cudnn_error,
            ));
            assert!(!is_cuda_fallback_candidate(
                FoundryExecutionDevice::Other,
                None,
                cudnn_error,
            ));
        }

        #[test]
        fn cpu_variant_selection_uses_device_type_and_highest_version() {
            let variants = [
                FoundryVariantDescriptor::new(
                    "whisper-medium-cuda-gpu:4",
                    4,
                    FoundryExecutionDevice::Gpu,
                ),
                FoundryVariantDescriptor::new(
                    "whisper-medium-generic-cpu:3",
                    3,
                    FoundryExecutionDevice::Cpu,
                ),
                FoundryVariantDescriptor::new(
                    "whisper-medium-generic-cpu:4",
                    4,
                    FoundryExecutionDevice::Cpu,
                ),
            ];

            assert_eq!(
                select_cpu_variant_id(&variants).as_deref(),
                Some("whisper-medium-generic-cpu:4")
            );
        }

        #[test]
        fn temporary_cpu_model_is_not_reused_by_the_next_recording() {
            assert!(may_reuse_loaded_model(
                "whisper-medium",
                "whisper-medium",
                false
            ));
            assert!(!may_reuse_loaded_model(
                "whisper-medium",
                "whisper-medium",
                true
            ));
            assert!(!may_reuse_loaded_model(
                "whisper-small",
                "whisper-medium",
                false
            ));
        }

        #[test]
        fn cancelled_recording_cleanup_matches_only_its_cpu_fallback_lease() {
            let runtime = FoundryLocalRuntime::new();
            let cancelled_lease = runtime.next_temporary_cpu_fallback_lease();
            let newer_lease = runtime.next_temporary_cpu_fallback_lease();

            assert!(should_release_temporary_cpu_fallback(
                Some(cancelled_lease),
                cancelled_lease
            ));
            assert!(!should_release_temporary_cpu_fallback(
                Some(newer_lease),
                cancelled_lease
            ));
        }

        #[test]
        fn stale_transcription_cancel_cannot_touch_the_current_route() {
            let runtime = FoundryLocalRuntime::new();
            let old_route = runtime.begin_route();
            let current_route = runtime.begin_route();

            assert_eq!(
                runtime.request_cancel_transcription(old_route),
                None,
                "a stale provider must not cancel the current route"
            );
            assert_eq!(runtime.route_epoch_snapshot(), current_route);
            assert!(!runtime.cancel_prepare_requested_for_tests());
        }

        #[test]
        fn current_transcription_cancel_invalidates_its_route() {
            let runtime = FoundryLocalRuntime::new();
            let route = runtime.begin_route();

            assert_eq!(runtime.request_cancel_transcription(route), None);
            assert_ne!(runtime.route_epoch_snapshot(), route);
            assert!(runtime.cancel_prepare_requested_for_tests());
        }

        #[test]
        fn a_new_route_invalidates_an_old_primary_recovery_token() {
            let runtime = FoundryLocalRuntime::new();
            let epoch = runtime.advance_route_epoch();
            let token = super::super::FoundryPrimaryRecoveryToken {
                alias: "whisper-medium".to_string(),
                primary_model_id: "whisper-medium-cuda-gpu:4".to_string(),
                route_epoch: epoch,
            };

            assert!(runtime.route_is_current(&token));
            runtime.advance_route_epoch();
            assert!(!runtime.route_is_current(&token));
        }

        #[test]
        fn runtime_has_async_lifecycle_gate() {
            let runtime = FoundryLocalRuntime::new();

            assert!(runtime.lifecycle.try_lock().is_ok());
        }

        #[test]
        fn runtime_normalizes_language_hint_before_audio_client() {
            assert_eq!(
                normalized_language_hint(Some(" zh ")),
                Some("zh".to_string())
            );
            assert_eq!(normalized_language_hint(Some("")), None);
            assert_eq!(normalized_language_hint(None), None);
        }

        #[test]
        fn runtime_finds_downloaded_foundry_native_runtime_dir() {
            let root = std::env::temp_dir().join(format!(
                "openless-foundry-native-test-{}",
                uuid::Uuid::new_v4()
            ));
            let native_dir = root.join("runtime");
            fs::create_dir_all(&native_dir).unwrap();
            fs::write(
                native_dir.join("Microsoft.AI.Foundry.Local.Core.dll"),
                b"placeholder",
            )
            .unwrap();

            let candidates = foundry_native_dir_candidates(Some(native_dir.as_path()));

            assert_eq!(select_foundry_native_dir(candidates).unwrap(), native_dir);

            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::FoundryLocalRuntime;

#[cfg(not(target_os = "windows"))]
pub struct FoundryLocalRuntime;

#[cfg(not(target_os = "windows"))]
impl Default for FoundryLocalRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "windows"))]
impl FoundryLocalRuntime {
    pub fn new() -> Self {
        Self
    }

    pub async fn status_snapshot(
        &self,
        active_model: &str,
        runtime_source: &str,
    ) -> super::foundry::FoundryRuntimeStatus {
        let mut status = super::foundry::FoundryRuntimeStatus::unavailable(
            active_model.to_string(),
            "Foundry Local Whisper is only available on Windows",
        );
        status.runtime_source = super::foundry_native::normalize_runtime_source_str(runtime_source);
        status
    }

    pub async fn ensure_loaded(
        &self,
        alias: &str,
        _runtime_source: &str,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Foundry Local Whisper is only available on Windows: {alias}");
    }

    pub async fn ensure_loaded_with_progress<F>(
        &self,
        alias: &str,
        _runtime_source: &str,
        _progress: F,
    ) -> anyhow::Result<String>
    where
        F: Fn(super::foundry::FoundryPrepareProgressPayload) + Send + Sync + 'static,
    {
        anyhow::bail!("Foundry Local Whisper is only available on Windows: {alias}");
    }

    pub fn request_cancel_prepare(&self) {}

    pub(crate) fn begin_route(&self) -> FoundryRouteEpoch {
        FoundryRouteEpoch(0)
    }

    pub fn invalidate_route(&self) {}

    pub(crate) async fn transcribe_audio_files(
        &self,
        _route_epoch: FoundryRouteEpoch,
        alias: &str,
        _runtime_source: &str,
        _language_hint: Option<&str>,
        _audio_paths: &[std::path::PathBuf],
        _audio_timeout: std::time::Duration,
        _notices: FoundryFallbackNoticeCallback,
    ) -> anyhow::Result<FoundryTranscriptionOutcome> {
        anyhow::bail!("Foundry Local Whisper is only available on Windows: {alias}");
    }

    pub async fn release_now(&self) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn release_temporary_cpu_fallback(
        &self,
        _lease: FoundryTemporaryCpuFallbackLease,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn storage_configuration_locked(&self) -> bool {
        false
    }

    pub async fn model_dir_for_alias(&self, alias: &str) -> anyhow::Result<std::path::PathBuf> {
        anyhow::bail!("Foundry Local Whisper is only available on Windows: {alias}");
    }

    pub async fn inspect_models(
        &self,
        aliases: &[String],
    ) -> anyhow::Result<Vec<FoundryNativeModelState>> {
        Ok(aliases
            .iter()
            .map(|alias| FoundryNativeModelState {
                alias: alias.clone(),
                cached: false,
                size_bytes: None,
                display_name: None,
            })
            .collect())
    }

    pub async fn delete_model(&self, alias: &str) -> anyhow::Result<()> {
        anyhow::bail!("Foundry Local Whisper is only available on Windows: {alias}");
    }
}

#[cfg(test)]
mod tests {
    use super::FoundryLocalRuntime;

    #[tokio::test]
    async fn new_runtime_reports_native_audio_status_shape() {
        let runtime = FoundryLocalRuntime::new();
        let status = runtime.status_snapshot("whisper-small", "auto").await;

        assert_eq!(status.provider_id, crate::asr::local::foundry::PROVIDER_ID);
        assert_eq!(status.active_model, "whisper-small");
        assert_eq!(status.loaded_model_id, None);
        assert_eq!(status.endpoint, None);
        if status.available {
            assert_eq!(status.error, None);
        } else {
            assert!(status.error.is_some());
        }
    }

    #[tokio::test]
    async fn new_runtime_release_now_has_real_async_unload_contract() {
        let runtime = FoundryLocalRuntime::new();

        runtime.release_now().await.unwrap();

        let status = runtime.status_snapshot("whisper-small", "auto").await;
        assert_eq!(status.loaded_model_id, None);
    }
}
