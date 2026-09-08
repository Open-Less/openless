//! Thin Tauri implementations of the framework-independent core ports.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures_util::future::BoxFuture;
use openless_core::{
    ActiveRecording, AudioConsumer as CoreAudioConsumer, AudioRecorder, AudioRecorderRouter,
    BackendError, BackendErrorCode, DictationContext, DictationEngine, EditObservationAdapter,
    EditObservationSink, ExternalAudioRecorder, HostAction, HostActions, InsertOutcome,
    InsertWriteResult, RecordingArchive, RecordingProgressSink, SessionId,
    TextInserter as CoreTextInserter, TextInsertionSession, TextPolisher, TextStreamChunk,
    TextStreamSink, TranscriptOutput, TranscriptionEngine, TranscriptionSession,
};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::recorder::{AudioConsumer as LegacyAudioConsumer, Recorder, RecorderError};

pub(crate) type AppHandleSlot = Arc<Mutex<Option<AppHandle>>>;

pub(crate) fn app_handle_slot() -> AppHandleSlot {
    Arc::new(Mutex::new(None))
}

/// Late-bound Core backend shared with adapters that are constructed before
/// `OpenLessBackend::new` returns. The weak reference keeps Core ownership
/// explicit without making an adapter query Tauri managed state through
/// `AppHandle` or creating a backend/adapter reference cycle.
pub(crate) type BackendSlot = Arc<Mutex<Option<std::sync::Weak<openless_core::OpenLessBackend>>>>;

pub(crate) fn backend_slot() -> BackendSlot {
    Arc::new(Mutex::new(None))
}

/// Native audio callbacks and resource destructors do not inherit Tokio's
/// thread-local context. Always enqueue on Tauri's shared executor instead of
/// asking the callback thread for a runtime and silently losing its cleanup.
struct TauriTaskSpawner;

impl openless_core::TaskSpawner for TauriTaskSpawner {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        tauri::async_runtime::spawn(task);
    }
}

#[derive(Clone)]
pub(crate) struct TauriNativeAsrDependencies {
    foundry: Arc<crate::asr::local::FoundryLocalRuntime>,
    sherpa: Arc<crate::asr::local::SherpaOnnxRuntime>,
    #[cfg(target_os = "windows")]
    foundry_generation: Arc<AtomicU64>,
    #[cfg(target_os = "windows")]
    sherpa_generation: Arc<AtomicU64>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    qwen_cache: Arc<crate::asr::local::LocalAsrCache>,
    #[cfg(target_os = "macos")]
    whisper_cache: Arc<crate::asr::local::LocalWhisperCache>,
}

impl TauriNativeAsrDependencies {
    #[cfg(target_os = "windows")]
    pub(crate) fn new(
        foundry: Arc<crate::asr::local::FoundryLocalRuntime>,
        sherpa: Arc<crate::asr::local::SherpaOnnxRuntime>,
    ) -> Self {
        Self {
            foundry,
            sherpa,
            foundry_generation: Arc::new(AtomicU64::new(0)),
            sherpa_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn new() -> Self {
        Self {
            foundry: Arc::new(crate::asr::local::FoundryLocalRuntime::new()),
            sherpa: Arc::new(crate::asr::local::SherpaOnnxRuntime::new()),
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            qwen_cache: Arc::new(crate::asr::local::LocalAsrCache::new()),
            #[cfg(target_os = "macos")]
            whisper_cache: Arc::new(crate::asr::local::LocalWhisperCache::new()),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(crate) fn qwen_cache(&self) -> Arc<crate::asr::local::LocalAsrCache> {
        Arc::clone(&self.qwen_cache)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn whisper_cache(&self) -> Arc<crate::asr::local::LocalWhisperCache> {
        Arc::clone(&self.whisper_cache)
    }
}

pub(crate) fn backend_dependencies(
    app: AppHandleSlot,
    backend: BackendSlot,
    native_asr_dependencies: TauriNativeAsrDependencies,
    preferences: Arc<openless_core::PreferencesStore>,
    hotkey_status: Arc<Mutex<openless_core::HotkeyStatus>>,
    qa_host_context: Arc<crate::qa_adapter::TauriQaHostContext>,
) -> openless_core::BackendDependencies {
    // The Tauri host owns the Tokio executor used by shared core providers.
    // Keep this spawner explicit so core never falls back to constructing a
    // runtime during cancellation or background cleanup.
    let task_spawner: Arc<dyn openless_core::TaskSpawner> = Arc::new(TauriTaskSpawner);
    let model_store = crate::persistence::models_root()
        .ok()
        .and_then(|root| openless_core::ModelStoreConfig::new(root).ok())
        .and_then(|config| openless_core::ModelStore::new(config).ok())
        .map(Arc::new);
    let credential_store: Arc<dyn openless_core::CredentialStore> = Arc::new(
        crate::commands::SystemCredentialStore::new(model_store.clone()),
    );
    let local_asr_runtime = Arc::new(TauriLocalAsrRuntimeAdapter::new(
        native_asr_dependencies.clone(),
        preferences,
    ));
    let transcription = Arc::new(openless_core::TranscriptionRouter::default());
    let production_asr: Arc<dyn TranscriptionEngine> = Arc::new(
        openless_core::SharedCloudTranscriptionEngine::with_task_spawner(
            Arc::clone(&credential_store),
            Arc::clone(&task_spawner),
        ),
    );
    for provider_type in openless_core::SHARED_CLOUD_ASR_PROVIDER_TYPES {
        transcription
            .register(*provider_type, Arc::clone(&production_asr))
            .expect("built-in ASR provider ids are non-empty");
    }
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let native_asr: Arc<dyn TranscriptionEngine> = Arc::new(TauriNativeTranscriptionEngine::new(
        native_asr_dependencies,
        model_store.clone(),
        Arc::clone(&backend),
    ));
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let _ = native_asr_dependencies;
    #[cfg(target_os = "windows")]
    for provider_type in [
        crate::asr::local::foundry::PROVIDER_ID,
        "foundry-local",
        "foundry-whisper",
        crate::asr::local::sherpa::PROVIDER_ID,
        "sherpa-onnx",
    ] {
        transcription
            .register(provider_type, Arc::clone(&native_asr))
            .expect("native ASR provider ids are non-empty");
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    for provider_type in [
        crate::asr::local::PROVIDER_ID,
        crate::asr::local::LOCAL_QWEN3_MLX_PROVIDER_ID,
        crate::asr::local::LOCAL_QWEN3_C_PROVIDER_ID,
    ] {
        transcription
            .register(provider_type, Arc::clone(&native_asr))
            .expect("native ASR provider ids are non-empty");
    }
    #[cfg(target_os = "macos")]
    for provider_type in [
        crate::asr::local::LOCAL_WHISPER_PROVIDER_ID,
        crate::asr::local::APPLE_SPEECH_PROVIDER_ID,
    ] {
        transcription
            .register(provider_type, Arc::clone(&native_asr))
            .expect("native ASR provider ids are non-empty");
    }
    let polisher = Arc::new(openless_core::TextPolisherRouter::default());
    let production_polisher: Arc<dyn TextPolisher> = Arc::new(
        openless_core::SharedCloudTextPolisher::new(Arc::clone(&credential_store)),
    );
    for provider_type in openless_core::SHARED_CLOUD_LLM_PROVIDER_TYPES {
        polisher
            .register(*provider_type, Arc::clone(&production_polisher))
            .expect("built-in LLM provider ids are non-empty");
    }
    let polisher: Arc<dyn TextPolisher> = polisher;
    let auxiliary_transcription: Arc<dyn TranscriptionEngine> = transcription.clone();
    let auxiliary_polisher: Arc<dyn TextPolisher> =
        Arc::new(openless_core::SharedAuxiliaryTextPolisher::new(
            Arc::clone(&credential_store),
            Arc::clone(&polisher),
        ));
    let host_recorder: Arc<dyn AudioRecorder> = Arc::new(TauriAudioRecorder {
        app: Arc::clone(&app),
        backend: Arc::clone(&backend),
    });
    let recorder =
        AudioRecorderRouter::new(Arc::clone(&host_recorder), ExternalAudioRecorder::default());
    let traditional = Arc::new(openless_core::PipelineDictationEngine::new(
        Arc::new(recorder),
        transcription,
        Arc::clone(&polisher),
    ));
    let dictation = Arc::new(openless_core::DictationEngineRouter::new(traditional));
    let production_omni: Arc<dyn DictationEngine> = Arc::new(
        openless_core::SharedOmniDictationEngine::new(Arc::clone(&credential_store), host_recorder),
    );
    for provider_type in openless_core::SHARED_OMNI_PROVIDER_TYPES {
        dictation
            .register_omni(*provider_type, Arc::clone(&production_omni))
            .expect("built-in Omni provider ids are non-empty");
    }
    let mut dependencies = openless_core::BackendDependencies::unsupported();
    if let Some(model_store) = model_store {
        dependencies.services.configure_model_store(model_store);
    }
    dependencies
        .services
        .configure_auxiliary_runtime(auxiliary_polisher, auxiliary_transcription);
    dependencies.services.provider = Arc::new(openless_core::ProviderService::new(
        Arc::clone(&credential_store),
        Arc::clone(&task_spawner),
    ));
    dependencies
        .services
        .configure_coding_agent_process(Arc::new(
            crate::coding_agent::TauriCodingAgentProcessAdapter,
        ));
    dependencies.qa_runtime = Some(Arc::new(crate::qa_adapter::TauriQaRuntimeAdapter::new(
        Arc::clone(&app),
        backend,
        Arc::clone(&credential_store),
        Arc::clone(&qa_host_context),
    )));
    #[cfg(not(mobile))]
    {
        let runtime = Arc::new(TauriRemoteInputRuntimeAdapter::new(Arc::clone(&app)));
        dependencies.services.remote_input = Arc::new(
            openless_core::RemoteInputService::new(runtime, 8443, "zh-CN")
                .expect("built-in remote input defaults are valid"),
        );
    }
    dependencies.services.platform =
        Arc::new(TauriPlatformApi::new(Arc::clone(&app), hotkey_status));
    dependencies.services.host_context = Arc::new(TauriHostContextAdapter);
    dependencies.services.edit_observation = Arc::new(TauriEditObservationAdapter::default());
    dependencies.local_asr_runtime = Some(local_asr_runtime);
    #[cfg(not(mobile))]
    {
        dependencies.selection_runtime =
            Some(Arc::new(TauriSelectionRuntime::new(Arc::clone(&app))));
        dependencies.selection_polisher = Some(polisher);
    }
    dependencies.text_inserter = Arc::new(TauriTextInserter::new(Arc::clone(&app)));
    dependencies.host_actions = Arc::new(TauriHostActions::new(app, qa_host_context));
    dependencies.dictation_engine = dictation;
    dependencies.credential_store = credential_store;
    dependencies.task_spawner = task_spawner;
    dependencies
}

fn local_asr_backend_error(code: BackendErrorCode, error: impl std::fmt::Display) -> BackendError {
    BackendError::new(code, error.to_string())
}

fn native_local_asr_model(
    target: &openless_core::LocalAsrTarget,
) -> Result<crate::asr::local::ModelId, BackendError> {
    crate::asr::local::ModelId::from_wire_id(target.model_id()).ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            format!("unknown generic local ASR model: {}", target.model_id()),
        )
    })
}

struct TauriLocalAsrRuntimeAdapter {
    native: TauriNativeAsrDependencies,
    preferences: Arc<openless_core::PreferencesStore>,
    foundry_rebind_pending: Arc<AtomicBool>,
}

impl TauriLocalAsrRuntimeAdapter {
    fn new(
        native: TauriNativeAsrDependencies,
        preferences: Arc<openless_core::PreferencesStore>,
    ) -> Self {
        Self {
            native,
            preferences,
            foundry_rebind_pending: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(target_os = "windows")]
    fn invalidate_release(&self, runtime: openless_core::LocalAsrRuntime) {
        match runtime {
            openless_core::LocalAsrRuntime::Foundry => &self.native.foundry_generation,
            openless_core::LocalAsrRuntime::SherpaOnnx => &self.native.sherpa_generation,
            openless_core::LocalAsrRuntime::Generic => return,
        }
        .fetch_add(1, Ordering::AcqRel);
    }
}

impl openless_core::ModelRuntimeAdapter for TauriLocalAsrRuntimeAdapter {
    fn engine_available(&self, runtime: openless_core::LocalAsrRuntime) -> bool {
        match runtime {
            openless_core::LocalAsrRuntime::Generic => {
                cfg!(any(target_os = "macos", target_os = "linux"))
            }
            openless_core::LocalAsrRuntime::Foundry
            | openless_core::LocalAsrRuntime::SherpaOnnx => cfg!(target_os = "windows"),
        }
    }

    fn supports_model(&self, target: &openless_core::LocalAsrTarget) -> bool {
        match target.runtime {
            openless_core::LocalAsrRuntime::Generic => {
                #[cfg(target_os = "macos")]
                {
                    true
                }
                #[cfg(target_os = "linux")]
                {
                    openless_core::LocalAsrModelId::from_wire_id(target.model_id())
                        .is_some_and(openless_core::LocalAsrModelId::is_qwen)
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                {
                    false
                }
            }
            openless_core::LocalAsrRuntime::Foundry
            | openless_core::LocalAsrRuntime::SherpaOnnx => cfg!(target_os = "windows"),
        }
    }

    fn inspect_native_models(
        &self,
        targets: Vec<openless_core::LocalAsrTarget>,
    ) -> BoxFuture<'static, Result<Vec<openless_core::NativeModelState>, BackendError>> {
        let foundry = Arc::clone(&self.native.foundry);
        Box::pin(async move {
            let foundry_targets = targets
                .into_iter()
                .filter(|target| target.runtime == openless_core::LocalAsrRuntime::Foundry)
                .collect::<Vec<_>>();
            if foundry_targets.is_empty() {
                return Ok(Vec::new());
            }
            let aliases = foundry_targets
                .iter()
                .map(|target| target.model_id().to_string())
                .collect::<Vec<_>>();
            let states = foundry.inspect_models(&aliases).await.map_err(|error| {
                local_asr_backend_error(BackendErrorCode::Platform, format!("{error:#}"))
            })?;
            Ok(states
                .into_iter()
                .filter_map(|state| {
                    foundry_targets
                        .iter()
                        .find(|target| target.model_id() == state.alias)
                        .cloned()
                        .map(|target| openless_core::NativeModelState {
                            target,
                            installed: state.cached,
                            size_bytes: state.size_bytes,
                            display_name: state.display_name,
                        })
                })
                .collect())
        })
    }

    fn native_model_dir(
        &self,
        target: openless_core::LocalAsrTarget,
        fallback: PathBuf,
    ) -> BoxFuture<'static, Result<PathBuf, BackendError>> {
        if target.runtime != openless_core::LocalAsrRuntime::Foundry {
            return Box::pin(async move { Ok(fallback) });
        }
        let foundry = Arc::clone(&self.native.foundry);
        Box::pin(async move {
            foundry
                .model_dir_for_alias(target.model_id())
                .await
                .map_err(|error| {
                    local_asr_backend_error(BackendErrorCode::Platform, format!("{error:#}"))
                })
        })
    }

    fn delete_native_model(
        &self,
        target: openless_core::LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let foundry = Arc::clone(&self.native.foundry);
        Box::pin(async move {
            if target.runtime != openless_core::LocalAsrRuntime::Foundry {
                return Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "only Foundry uses native model deletion",
                ));
            }
            foundry
                .delete_model(target.model_id())
                .await
                .map_err(|error| {
                    local_asr_backend_error(BackendErrorCode::Platform, format!("{error:#}"))
                })
        })
    }

    fn rebind_storage(
        &self,
        _models_root: PathBuf,
    ) -> BoxFuture<'static, Result<openless_core::StorageRebind, BackendError>> {
        let restart_required = self.native.foundry.storage_configuration_locked();
        self.foundry_rebind_pending
            .store(restart_required, Ordering::Release);
        Box::pin(async move {
            Ok(if restart_required {
                openless_core::StorageRebind::RestartRequired
            } else {
                openless_core::StorageRebind::Applied
            })
        })
    }

    fn runtime_status(
        &self,
        settings: openless_core::LocalAsrSettings,
        _model_dir: PathBuf,
    ) -> BoxFuture<'static, Result<openless_core::LocalAsrRuntimeStatus, BackendError>> {
        let foundry = Arc::clone(&self.native.foundry);
        let sherpa = Arc::clone(&self.native.sherpa);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let qwen_cache = Arc::clone(&self.native.qwen_cache);
        #[cfg(target_os = "macos")]
        let whisper_cache = Arc::clone(&self.native.whisper_cache);
        Box::pin(async move {
            match settings.runtime {
                openless_core::LocalAsrRuntime::Generic => {
                    #[cfg(target_os = "macos")]
                    let loaded =
                        if openless_core::LocalAsrModelId::from_wire_id(&settings.active_model)
                            .is_some_and(openless_core::LocalAsrModelId::is_whisper)
                        {
                            whisper_cache.loaded_model_id()
                        } else {
                            qwen_cache.loaded_model_id()
                        };
                    #[cfg(target_os = "linux")]
                    let loaded = qwen_cache.loaded_model_id();
                    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                    let loaded: Option<String> = None;
                    Ok(openless_core::LocalAsrRuntimeStatus {
                        runtime: settings.runtime,
                        provider_id: settings.provider_id,
                        available: settings.engine_available,
                        loaded: loaded.is_some(),
                        active_model: settings.active_model,
                        model_id: loaded,
                        keep_loaded_secs: settings.keep_loaded_secs,
                        runtime_source: None,
                        endpoint: None,
                        operation: None,
                        error: None,
                        last_error: None,
                        last_prepare_ms: None,
                        last_transcribe_ms: None,
                        last_audio_ms: None,
                    })
                }
                openless_core::LocalAsrRuntime::Foundry => {
                    let status = foundry
                        .status_snapshot(
                            &settings.active_model,
                            settings.runtime_source.unwrap_or_default().as_str(),
                        )
                        .await;
                    Ok(openless_core::LocalAsrRuntimeStatus {
                        runtime: settings.runtime,
                        provider_id: status.provider_id,
                        available: status.available,
                        loaded: status.runtime_ready,
                        active_model: status.active_model,
                        model_id: status.loaded_model_id,
                        keep_loaded_secs: settings.keep_loaded_secs,
                        runtime_source: Some(openless_core::FoundryRuntimeSource::from_legacy(
                            &status.runtime_source,
                        )),
                        endpoint: status.endpoint,
                        operation: None,
                        error: status.error,
                        last_error: None,
                        last_prepare_ms: None,
                        last_transcribe_ms: None,
                        last_audio_ms: None,
                    })
                }
                openless_core::LocalAsrRuntime::SherpaOnnx => {
                    let status = sherpa.status_snapshot(&settings.active_model).await;
                    Ok(openless_core::LocalAsrRuntimeStatus {
                        runtime: settings.runtime,
                        provider_id: status.provider_id,
                        available: status.available,
                        loaded: status.runtime_ready,
                        active_model: status.active_model,
                        model_id: status.loaded_model_id,
                        keep_loaded_secs: settings.keep_loaded_secs,
                        runtime_source: None,
                        endpoint: None,
                        operation: None,
                        error: status.error,
                        last_error: status.last_error,
                        last_prepare_ms: status.last_prepare_ms,
                        last_transcribe_ms: status.last_transcribe_ms,
                        last_audio_ms: status.last_audio_ms,
                    })
                }
            }
        })
    }

    fn prepare(
        &self,
        target: openless_core::LocalAsrTarget,
        runtime_source: openless_core::FoundryRuntimeSource,
        model_dir: PathBuf,
        progress: openless_core::ModelPrepareProgressSink,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        #[cfg(target_os = "windows")]
        self.invalidate_release(target.runtime);
        let foundry = Arc::clone(&self.native.foundry);
        let sherpa = Arc::clone(&self.native.sherpa);
        let foundry_rebind_pending = Arc::clone(&self.foundry_rebind_pending);
        Box::pin(async move {
            let loaded = match target.runtime {
                openless_core::LocalAsrRuntime::Foundry => {
                    if foundry_rebind_pending.load(Ordering::Acquire) {
                        return Err(BackendError::new(
                            BackendErrorCode::InvalidState,
                            "Foundry storage changed; restart is required",
                        ));
                    }
                    let progress = Arc::clone(&progress);
                    foundry
                        .ensure_loaded_with_progress(
                            target.model_id(),
                            runtime_source.as_str(),
                            move |payload| {
                                let phase = match payload.phase {
                                    crate::asr::local::foundry::FoundryPreparePhase::Runtime => {
                                        openless_core::LocalAsrPreparePhase::Runtime
                                    }
                                    crate::asr::local::foundry::FoundryPreparePhase::Model => {
                                        openless_core::LocalAsrPreparePhase::Model
                                    }
                                    crate::asr::local::foundry::FoundryPreparePhase::Load => {
                                        openless_core::LocalAsrPreparePhase::Load
                                    }
                                    crate::asr::local::foundry::FoundryPreparePhase::Finished => {
                                        openless_core::LocalAsrPreparePhase::Finished
                                    }
                                    crate::asr::local::foundry::FoundryPreparePhase::Failed => {
                                        openless_core::LocalAsrPreparePhase::Failed
                                    }
                                };
                                progress(openless_core::LocalAsrPrepareProgress {
                                    runtime: openless_core::LocalAsrRuntimeKind::Foundry,
                                    phase,
                                    model_alias: payload.model_alias,
                                    label: payload.label,
                                    percent: payload.percent,
                                    error: payload.error,
                                });
                            },
                        )
                        .await
                        .map_err(|error| {
                            local_asr_backend_error(
                                BackendErrorCode::Platform,
                                format!("{error:#}"),
                            )
                        })
                }
                openless_core::LocalAsrRuntime::SherpaOnnx => {
                    let progress = Arc::clone(&progress);
                    sherpa
                        .ensure_loaded_with_progress(
                            target.model_id(),
                            model_dir.clone(),
                            move |payload| {
                                let phase = match payload.phase {
                                    crate::asr::local::sherpa::SherpaPreparePhase::Runtime => {
                                        openless_core::LocalAsrPreparePhase::Runtime
                                    }
                                    crate::asr::local::sherpa::SherpaPreparePhase::Model => {
                                        openless_core::LocalAsrPreparePhase::Model
                                    }
                                    crate::asr::local::sherpa::SherpaPreparePhase::Load => {
                                        openless_core::LocalAsrPreparePhase::Load
                                    }
                                    crate::asr::local::sherpa::SherpaPreparePhase::Finished => {
                                        openless_core::LocalAsrPreparePhase::Finished
                                    }
                                    crate::asr::local::sherpa::SherpaPreparePhase::Failed => {
                                        openless_core::LocalAsrPreparePhase::Failed
                                    }
                                };
                                progress(openless_core::LocalAsrPrepareProgress {
                                    runtime: openless_core::LocalAsrRuntimeKind::SherpaOnnx,
                                    phase,
                                    model_alias: payload.model_alias,
                                    label: payload.label,
                                    percent: payload.percent,
                                    error: payload.error,
                                });
                            },
                        )
                        .await
                        .map_err(|error| {
                            local_asr_backend_error(
                                BackendErrorCode::Platform,
                                format!("{error:#}"),
                            )
                        })
                }
                // Generic 模型文件已由 Core ModelStore 校验完整性。与 Foundry/Sherpa
                // 不同，它没有额外的 native runtime 安装阶段；真正加载留给 preload，
                // 此时才能拿到本次激活的 MLX/C provider，不能偷读尚未提交的旧偏好。
                openless_core::LocalAsrRuntime::Generic => {
                    let model = native_local_asr_model(&target)?;
                    if cfg!(target_os = "macos") || (cfg!(target_os = "linux") && model.is_qwen()) {
                        Ok(target.model_id().to_string())
                    } else {
                        Err(BackendError::new(
                            BackendErrorCode::Unsupported,
                            "generic local ASR model is unavailable on this platform",
                        ))
                    }
                }
            }?;
            let _ = model_dir;
            Ok(loaded)
        })
    }

    fn cancel_prepare(
        &self,
        runtime: openless_core::LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = match runtime {
            openless_core::LocalAsrRuntime::Foundry => {
                self.native.foundry.request_cancel_prepare();
                Ok(())
            }
            openless_core::LocalAsrRuntime::SherpaOnnx => {
                self.native.sherpa.request_cancel_prepare();
                Ok(())
            }
            openless_core::LocalAsrRuntime::Generic => Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "generic local ASR preload has no separate prepare cancellation",
            )),
        };
        Box::pin(async move { result })
    }

    fn release(
        &self,
        runtime: openless_core::LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        #[cfg(target_os = "windows")]
        self.invalidate_release(runtime);
        let foundry = Arc::clone(&self.native.foundry);
        let sherpa = Arc::clone(&self.native.sherpa);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let qwen_cache = Arc::clone(&self.native.qwen_cache);
        #[cfg(target_os = "macos")]
        let whisper_cache = Arc::clone(&self.native.whisper_cache);
        Box::pin(async move {
            match runtime {
                openless_core::LocalAsrRuntime::Generic => {
                    #[cfg(any(target_os = "macos", target_os = "linux"))]
                    qwen_cache.release_now();
                    #[cfg(target_os = "macos")]
                    whisper_cache.release_now();
                    Ok(())
                }
                openless_core::LocalAsrRuntime::Foundry => {
                    foundry.release_now().await.map_err(|error| {
                        local_asr_backend_error(BackendErrorCode::Platform, format!("{error:#}"))
                    })
                }
                openless_core::LocalAsrRuntime::SherpaOnnx => {
                    sherpa.release_now().await.map_err(|error| {
                        local_asr_backend_error(BackendErrorCode::Platform, format!("{error:#}"))
                    })
                }
            }
        })
    }

    fn claim_lease(&self, lease: openless_core::LocalAsrRuntimeLease) {
        if lease.target.runtime != openless_core::LocalAsrRuntime::Generic {
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if native_local_asr_model(&lease.target).is_ok_and(|model| model.is_qwen()) {
            self.native
                .qwen_cache
                .claim_lease(lease.target.model_id(), lease.generation);
        }
        #[cfg(target_os = "macos")]
        if native_local_asr_model(&lease.target).is_ok_and(|model| model.is_whisper()) {
            self.native
                .whisper_cache
                .claim_lease(lease.target.model_id(), lease.generation);
        }
    }

    fn release_lease(
        &self,
        lease: openless_core::LocalAsrRuntimeLease,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        if lease.target.runtime != openless_core::LocalAsrRuntime::Generic {
            return self.release(lease.target.runtime);
        }
        // Generic 在 macOS 下有两个独立 cache。由 cache 同锁校验模型及激活代次，
        // 不能整体 release(Generic)，也不能先查询 ID 再清空以免释放新模型。
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if native_local_asr_model(&lease.target).is_ok_and(|model| model.is_qwen()) {
            self.native
                .qwen_cache
                .release_lease(lease.target.model_id(), lease.generation);
        }
        #[cfg(target_os = "macos")]
        if native_local_asr_model(&lease.target).is_ok_and(|model| model.is_whisper()) {
            self.native
                .whisper_cache
                .release_lease(lease.target.model_id(), lease.generation);
        }
        Box::pin(async { Ok(()) })
    }

    fn preload(
        &self,
        target: openless_core::LocalAsrTarget,
        model_dir: PathBuf,
        provider_type: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.preload_for_lease(target, model_dir, provider_type, None)
    }

    fn preload_lease(
        &self,
        lease: openless_core::LocalAsrRuntimeLease,
        model_dir: PathBuf,
        provider_type: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.preload_for_lease(
            lease.target,
            model_dir,
            provider_type,
            Some(lease.generation),
        )
    }

    fn test_model(
        &self,
        target: openless_core::LocalAsrTarget,
        model_dir: PathBuf,
    ) -> BoxFuture<'static, Result<openless_core::LocalAsrTestResult, BackendError>> {
        let preferences = Arc::clone(&self.preferences);
        Box::pin(async move {
            if target.runtime != openless_core::LocalAsrRuntime::Generic {
                return Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "native model smoke test is only available for generic local ASR",
                ));
            }
            let backend = crate::asr::local::qwen_backend_for_provider(
                &preferences.get().active_asr_provider,
            );
            let result = crate::asr::local::test_run::run_test(
                native_local_asr_model(&target)?,
                backend,
                model_dir,
            )
            .await
            .map_err(|error| {
                local_asr_backend_error(BackendErrorCode::Platform, format!("{error:#}"))
            })?;
            Ok(openless_core::LocalAsrTestResult {
                target,
                backend: result.backend,
                expected_text: result.expected_text,
                transcribed_text: result.transcribed_text,
                audio_ms: result.audio_ms,
                load_ms: result.load_ms,
                transcribe_ms: result.transcribe_ms,
            })
        })
    }

    fn invalidate_route(&self, runtime: openless_core::LocalAsrRuntime) {
        if runtime == openless_core::LocalAsrRuntime::Foundry {
            self.native.foundry.invalidate_route();
        }
    }
}

impl TauriLocalAsrRuntimeAdapter {
    fn preload_for_lease(
        &self,
        target: openless_core::LocalAsrTarget,
        model_dir: PathBuf,
        provider_type: String,
        _activation_generation: Option<u64>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        #[cfg(target_os = "windows")]
        let foundry = Arc::clone(&self.native.foundry);
        #[cfg(target_os = "windows")]
        let sherpa = Arc::clone(&self.native.sherpa);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let qwen_cache = Arc::clone(&self.native.qwen_cache);
        #[cfg(target_os = "macos")]
        let whisper_cache = Arc::clone(&self.native.whisper_cache);
        Box::pin(async move {
            if target.runtime != openless_core::LocalAsrRuntime::Generic {
                // Windows 的 prepare 已完成加载；统一 preload 阶段只验证回执。
                // preload 也可被单独调用，所以必须核对真实已加载 alias，不能虚报
                // 成功，也不能像旧实现那样用 Unsupported 推翻成功的 prepare。
                #[cfg(target_os = "windows")]
                {
                    let ready = match target.runtime {
                        openless_core::LocalAsrRuntime::Foundry => {
                            foundry.is_loaded_for(target.model_id())
                        }
                        openless_core::LocalAsrRuntime::SherpaOnnx => {
                            let status = sherpa.status_snapshot(target.model_id()).await;
                            status.runtime_ready
                                && status.loaded_model_id.as_deref() == Some(target.model_id())
                        }
                        openless_core::LocalAsrRuntime::Generic => unreachable!(),
                    };
                    return ready.then_some(()).ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::InvalidState,
                            "prepare the selected native ASR model before preload",
                        )
                    });
                }
                #[cfg(not(target_os = "windows"))]
                {
                    return Err(BackendError::new(
                        BackendErrorCode::Unsupported,
                        "Foundry and Sherpa are only available on Windows",
                    ));
                }
            }
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                let provider = provider_type.as_str();
                let model = native_local_asr_model(&target)?;
                if crate::asr::local::is_local_qwen3(provider) {
                    let backend = crate::asr::local::qwen_backend_for_provider(provider)
                        .ok_or_else(|| {
                            BackendError::new(
                                BackendErrorCode::Unsupported,
                                format!("Qwen backend is unavailable: {provider}"),
                            )
                        })?;
                    if !model.is_qwen() {
                        return Err(BackendError::new(
                            BackendErrorCode::InvalidArgument,
                            "Qwen provider requires a Qwen model",
                        ));
                    }
                    let model_id = model.as_str().to_string();
                    tauri::async_runtime::spawn_blocking(move || {
                        qwen_cache.get_or_load_for_lease(
                            backend,
                            &model_id,
                            &model_dir,
                            _activation_generation,
                        )
                    })
                    .await
                    .map_err(map_native_asr_error)?
                    .map_err(map_native_asr_error)?;
                    return Ok(());
                }

                #[cfg(target_os = "macos")]
                if crate::asr::local::is_local_whisper(provider) {
                    let model_id = target.model_id().to_string();
                    let model_path =
                        crate::asr::local::whisper_model_path_for_model(&model_id, &model_dir)
                            .map_err(map_native_asr_error)?;
                    tauri::async_runtime::spawn_blocking(move || {
                        whisper_cache.get_or_load_for_lease(
                            &model_id,
                            &model_path,
                            _activation_generation,
                        )
                    })
                    .await
                    .map_err(map_native_asr_error)?
                    .map_err(map_native_asr_error)?;
                    return Ok(());
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            let _ = model_dir;
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                format!("generic local ASR provider is unavailable: {provider_type}"),
            ))
        })
    }
}

#[derive(Clone)]
struct TauriSelectionTarget {
    target: crate::selection::SelectionInsertionTarget,
    preview: bool,
}

#[cfg(not(mobile))]
trait SelectionPlatformBridge: Send + Sync {
    fn capture(
        &self,
    ) -> Result<
        (
            openless_core::SelectionCapture,
            crate::selection::SelectionInsertionTarget,
        ),
        BackendError,
    >;
    fn apply(
        &self,
        target: &crate::selection::SelectionInsertionTarget,
        source_text: &str,
        replacement_text: &str,
        reactivate: bool,
    ) -> Result<InsertOutcome, BackendError>;
    fn revert(
        &self,
        _target: &crate::selection::SelectionInsertionTarget,
    ) -> Result<InsertOutcome, BackendError> {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            "selection replacement cannot be reverted by this platform adapter",
        ))
    }
}

#[cfg(not(mobile))]
struct NativeSelectionPlatformBridge {
    app: AppHandleSlot,
}

#[cfg(not(mobile))]
impl NativeSelectionPlatformBridge {
    fn preferences(&self) -> Result<openless_core::UserPreferences, BackendError> {
        let app = self.app.lock().clone().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "Tauri AppHandle is not bound yet",
            )
        })?;
        app.try_state::<Arc<openless_core::OpenLessBackend>>()
            .map(|backend| backend.get_preferences())
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "core backend state is unavailable",
                )
            })
    }
}

#[cfg(not(mobile))]
impl SelectionPlatformBridge for NativeSelectionPlatformBridge {
    fn capture(
        &self,
    ) -> Result<
        (
            openless_core::SelectionCapture,
            crate::selection::SelectionInsertionTarget,
        ),
        BackendError,
    > {
        let (selection, target) = crate::selection::resolve_selection_workspace_capture();
        let selection = selection.ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                "selectionPolishNoSelection",
            )
        })?;
        if !crate::selection::selection_insertion_target_is_captured(&target) {
            return Err(BackendError::new(
                BackendErrorCode::Platform,
                "selectionPolishTargetUnavailable",
            ));
        }
        Ok((
            openless_core::SelectionCapture {
                text: selection.text,
                source_app: selection.source_app,
            },
            target,
        ))
    }

    fn apply(
        &self,
        target: &crate::selection::SelectionInsertionTarget,
        source_text: &str,
        replacement_text: &str,
        reactivate: bool,
    ) -> Result<InsertOutcome, BackendError> {
        if reactivate && !crate::selection::reactivate_selection_insertion_target(target) {
            return Err(BackendError::new(
                BackendErrorCode::Platform,
                "selectionPolishTargetUnavailable",
            ));
        }
        let validation = crate::selection::validate_selection_insertion_target(target, source_text);
        if let Some(code) = validation.error_code() {
            let error_code = match validation {
                crate::selection::SelectionInsertionTargetValidation::TargetUnavailable => {
                    BackendErrorCode::Platform
                }
                crate::selection::SelectionInsertionTargetValidation::TargetChanged
                | crate::selection::SelectionInsertionTargetValidation::SelectionChanged => {
                    BackendErrorCode::Cancelled
                }
                crate::selection::SelectionInsertionTargetValidation::Valid => unreachable!(),
            };
            return Err(BackendError::new(error_code, code));
        }
        let preferences = self.preferences()?;
        map_insert_status(crate::insertion::TextInserter::new().insert(
            replacement_text,
            preferences.restore_clipboard_after_paste,
            preferences.paste_shortcut,
        ))
    }
}

#[cfg(not(mobile))]
struct TauriSelectionRuntime {
    bridge: Arc<dyn SelectionPlatformBridge>,
    targets: Arc<Mutex<HashMap<SessionId, TauriSelectionTarget>>>,
}

#[cfg(not(mobile))]
impl TauriSelectionRuntime {
    fn new(app: AppHandleSlot) -> Self {
        Self::with_bridge(Arc::new(NativeSelectionPlatformBridge { app }))
    }

    fn with_bridge(bridge: Arc<dyn SelectionPlatformBridge>) -> Self {
        Self {
            bridge,
            targets: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(not(mobile))]
impl openless_core::SelectionRuntimeAdapter for TauriSelectionRuntime {
    fn capture(
        &self,
        session_id: SessionId,
        supplied_text: Option<String>,
    ) -> BoxFuture<'static, Result<openless_core::SelectionCapture, BackendError>> {
        if supplied_text.is_some() {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "Tauri selection capture does not accept injected text",
                ))
            });
        }
        let bridge = Arc::clone(&self.bridge);
        let targets = Arc::clone(&self.targets);
        Box::pin(async move {
            let (capture, target) = bridge.capture()?;
            let mut targets = targets.lock();
            if targets.contains_key(&session_id) {
                return Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "selection target is already registered for this session",
                ));
            }
            targets.clear();
            targets.insert(
                session_id,
                TauriSelectionTarget {
                    target,
                    preview: false,
                },
            );
            Ok(capture)
        })
    }

    fn apply(
        &self,
        session_id: SessionId,
        source_text: String,
        replacement_text: String,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let bridge = Arc::clone(&self.bridge);
        let targets = Arc::clone(&self.targets);
        Box::pin(async move {
            let target = targets.lock().get(&session_id).cloned().ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "selection target is no longer active",
                )
            })?;
            bridge.apply(
                &target.target,
                &source_text,
                &replacement_text,
                target.preview,
            )
        })
    }

    fn prepare_preview(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let targets = Arc::clone(&self.targets);
        Box::pin(async move {
            let mut targets = targets.lock();
            let target = targets.get_mut(&session_id).ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "selection target is no longer active",
                )
            })?;
            target.preview = true;
            Ok(())
        })
    }

    fn revert(
        &self,
        session_id: SessionId,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let bridge = Arc::clone(&self.bridge);
        let targets = Arc::clone(&self.targets);
        Box::pin(async move {
            let target = targets.lock().get(&session_id).cloned().ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "selection target is no longer active",
                )
            })?;
            bridge.revert(&target.target)
        })
    }

    fn cancel(&self, session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        let targets = Arc::clone(&self.targets);
        Box::pin(async move {
            targets.lock().remove(&session_id);
            Ok(())
        })
    }
}

#[cfg(not(mobile))]
struct TauriRemoteInputRuntimeAdapter {
    app: AppHandleSlot,
    server: Arc<tokio::sync::Mutex<Option<crate::remote_server::RemoteServerHandle>>>,
}

#[cfg(not(mobile))]
impl TauriRemoteInputRuntimeAdapter {
    fn new(app: AppHandleSlot) -> Self {
        Self {
            app,
            server: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    fn app_handle(&self) -> Result<AppHandle, BackendError> {
        self.app.lock().clone().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "Tauri app handle is not available",
            )
        })
    }

    fn backend(&self) -> Result<Arc<openless_core::OpenLessBackend>, BackendError> {
        let app = self.app_handle()?;
        app.try_state::<Arc<openless_core::OpenLessBackend>>()
            .map(|backend| Arc::clone(&*backend))
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "core backend state is unavailable",
                )
            })
    }
}

#[cfg(not(mobile))]
impl openless_core::RemoteInputRuntimeAdapter for TauriRemoteInputRuntimeAdapter {
    fn load_pairing_pin(
        &self,
    ) -> BoxFuture<'static, Result<Option<openless_core::SecretValue>, BackendError>> {
        let app = self.app_handle();
        Box::pin(async move {
            let app = app?;
            crate::remote_server::load_or_create_pin(&app)
                .map(openless_core::SecretValue::new)
                .map(Some)
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Persistence,
                        format!("persist pairing PIN failed: {error}"),
                    )
                })
        })
    }

    fn persist_pairing_pin(
        &self,
        pin: openless_core::SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let app = self.app_handle();
        Box::pin(async move {
            crate::remote_server::save_pin(&app?, pin.expose_secret()).map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("persist pairing PIN failed: {error}"),
                )
            })
        })
    }

    fn start_server(
        &self,
        config: openless_core::RemoteInputServerConfig,
    ) -> BoxFuture<'static, Result<openless_core::RemoteInputServerBinding, BackendError>> {
        let app = self.app_handle();
        let server = Arc::clone(&self.server);
        Box::pin(async move {
            let app = app?;
            let backend = app
                .try_state::<Arc<openless_core::OpenLessBackend>>()
                .map(|backend| Arc::clone(&*backend))
                .ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "core backend state is unavailable",
                    )
                })?;
            let handle = crate::remote_server::start(crate::remote_server::RemoteServerConfig {
                port: config.port,
                backend,
                app,
            })
            .await
            .map_err(|message| BackendError::new(BackendErrorCode::Platform, message))?;
            let binding = openless_core::RemoteInputServerBinding {
                port: handle.bound_port,
                urls: crate::remote_server::access_urls(handle.bound_port),
                urls_stale: false,
            };
            *server.lock().await = Some(handle);
            Ok(binding)
        })
    }

    fn stop_server(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let server = Arc::clone(&self.server);
        Box::pin(async move {
            if let Some(handle) = server.lock().await.take() {
                handle.shutdown().await;
            }
            Ok(())
        })
    }

    fn list_local_ips(&self) -> BoxFuture<'static, Result<Vec<String>, BackendError>> {
        Box::pin(async {
            Ok(crate::remote_server::local_lan_ipv4s()
                .iter()
                .map(ToString::to_string)
                .collect())
        })
    }

    fn start_audio_session(
        &self,
        insert_text: bool,
    ) -> BoxFuture<'static, Result<openless_core::SessionId, BackendError>> {
        let backend = self.backend();
        Box::pin(async move {
            let backend = backend?;
            if !backend.snapshot().running {
                backend.start().await?;
            }
            backend
                .start_external_dictation_with_options(openless_core::DictationStartOptions {
                    insert_text,
                    ..openless_core::DictationStartOptions::default()
                })
                .await
        })
    }

    fn feed_audio(
        &self,
        session_id: openless_core::SessionId,
        pcm_s16le: Vec<u8>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let backend = self.backend();
        Box::pin(async move { backend?.feed_external_pcm(session_id, &pcm_s16le) })
    }

    fn stop_audio_session(
        &self,
        session_id: openless_core::SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let backend = self.backend();
        Box::pin(async move {
            backend?
                .stop_dictation_session(session_id)
                .await
                .map(|_| ())
        })
    }

    fn cancel_audio_session(
        &self,
        session_id: openless_core::SessionId,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let backend = self.backend();
        Box::pin(async move { backend?.cancel_dictation(Some(session_id)).await })
    }
}

struct TauriPlatformApi {
    app: AppHandleSlot,
    hotkey_status: Arc<Mutex<openless_core::HotkeyStatus>>,
}

impl TauriPlatformApi {
    fn new(app: AppHandleSlot, hotkey_status: Arc<Mutex<openless_core::HotkeyStatus>>) -> Self {
        Self { app, hotkey_status }
    }

    fn app_handle(&self) -> Result<AppHandle, BackendError> {
        self.app.lock().clone().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "Tauri app handle is not available",
            )
        })
    }
}

fn map_permission_state(
    status: crate::permissions::PermissionStatus,
) -> openless_core::PermissionState {
    match status {
        crate::permissions::PermissionStatus::Granted => openless_core::PermissionState::Granted,
        crate::permissions::PermissionStatus::Denied => openless_core::PermissionState::Denied,
        crate::permissions::PermissionStatus::NotDetermined => {
            openless_core::PermissionState::Unknown
        }
        crate::permissions::PermissionStatus::Restricted => {
            openless_core::PermissionState::Restricted
        }
        crate::permissions::PermissionStatus::NotApplicable => {
            openless_core::PermissionState::Unsupported
        }
        crate::permissions::PermissionStatus::NoDevice => openless_core::PermissionState::NoDevice,
    }
}

fn current_permission_snapshot() -> openless_core::PermissionSnapshot {
    openless_core::PermissionSnapshot {
        microphone: map_permission_state(crate::permissions::check_microphone()),
        accessibility: map_permission_state(crate::permissions::check_accessibility()),
    }
}

impl openless_core::PlatformApi for TauriPlatformApi {
    fn capabilities(
        &self,
    ) -> BoxFuture<'static, Result<openless_core::PlatformCapabilities, BackendError>> {
        Box::pin(async { Ok(openless_core::PlatformCapabilities::current()) })
    }

    fn microphone_devices(
        &self,
    ) -> BoxFuture<'static, Result<Vec<openless_core::MicrophoneDevice>, BackendError>> {
        Box::pin(async move {
            #[cfg(mobile)]
            {
                Ok(Vec::new())
            }
            #[cfg(not(mobile))]
            {
                let devices =
                    tauri::async_runtime::spawn_blocking(crate::recorder::list_input_devices)
                        .await
                        .map_err(map_tauri_error)?
                        .map_err(map_recorder_error)?;
                Ok(devices
                    .into_iter()
                    .map(|device| openless_core::MicrophoneDevice {
                        id: device.name.clone(),
                        name: device.name,
                        is_default: device.is_default,
                    })
                    .collect())
            }
        })
    }

    fn microphone_permission(
        &self,
    ) -> BoxFuture<'static, Result<openless_core::PermissionSnapshot, BackendError>> {
        Box::pin(async { Ok(current_permission_snapshot()) })
    }

    fn accessibility_permission(
        &self,
    ) -> BoxFuture<'static, Result<openless_core::PermissionSnapshot, BackendError>> {
        Box::pin(async { Ok(current_permission_snapshot()) })
    }

    fn request_microphone_permission(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let app = self.app_handle();
        Box::pin(async move {
            let app = app?;
            let _ = crate::request_microphone_from_foreground(&app);
            Ok(())
        })
    }

    fn request_accessibility_permission(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            let _ = crate::permissions::request_accessibility();
            Ok(())
        })
    }

    fn hotkey_status(
        &self,
    ) -> BoxFuture<'static, Result<openless_core::HotkeyStatus, BackendError>> {
        #[cfg(mobile)]
        {
            Box::pin(async {
                Ok(openless_core::HotkeyStatus {
                    adapter: crate::types::HotkeyAdapterKind::Unavailable,
                    state: crate::types::HotkeyStatusState::Failed,
                    message: Some("移动端不支持全局热键".into()),
                    last_error: Some(crate::types::HotkeyInstallError {
                        code: "unavailable".into(),
                        message: "Global hotkeys are not available on mobile".into(),
                    }),
                })
            })
        }
        #[cfg(not(mobile))]
        {
            let hotkey_status = Arc::clone(&self.hotkey_status);
            Box::pin(async move { Ok(hotkey_status.lock().clone()) })
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
struct TauriNativeTranscriptionEngine {
    dependencies: TauriNativeAsrDependencies,
    model_store: Option<Arc<openless_core::ModelStore>>,
    #[cfg(target_os = "windows")]
    backend: BackendSlot,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl TauriNativeTranscriptionEngine {
    fn new(
        dependencies: TauriNativeAsrDependencies,
        model_store: Option<Arc<openless_core::ModelStore>>,
        _backend: BackendSlot,
    ) -> Self {
        Self {
            dependencies,
            model_store,
            #[cfg(target_os = "windows")]
            backend: _backend,
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
#[derive(Clone)]
enum TauriNativeTranscriptionSessionKind {
    #[cfg(target_os = "windows")]
    Foundry {
        provider: Arc<crate::asr::local::FoundryLocalWhisperAsr>,
        runtime: Arc<crate::asr::local::FoundryLocalRuntime>,
    },
    #[cfg(target_os = "windows")]
    Sherpa {
        provider: Arc<crate::asr::local::SherpaOnnxAsr>,
        runtime: Arc<crate::asr::local::SherpaOnnxRuntime>,
    },
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    Qwen {
        engine: Arc<crate::asr::local::LocalQwenEngine>,
        cache: Arc<crate::asr::local::LocalAsrCache>,
        pcm: Arc<Mutex<Vec<u8>>>,
        cancelled: Arc<AtomicBool>,
        operation_id: u64,
    },
    #[cfg(target_os = "macos")]
    Whisper {
        engine: Arc<crate::asr::local::WhisperEngine>,
        cache: Arc<crate::asr::local::LocalWhisperCache>,
        language: String,
        pcm: Arc<Mutex<Vec<u8>>>,
        cancelled: Arc<AtomicBool>,
    },
    #[cfg(target_os = "macos")]
    AppleSpeech(Arc<crate::asr::local::AppleSpeechAsr>),
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
#[derive(Clone)]
struct TauriNativeTranscriptionSession {
    kind: TauriNativeTranscriptionSessionKind,
    asr_call_label: openless_core::AsrCallLabel,
    #[cfg(target_os = "windows")]
    backend: BackendSlot,
    #[cfg(target_os = "windows")]
    session_id: SessionId,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    partials: Arc<dyn TextStreamSink>,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    next_offset: Arc<AtomicU64>,
    #[cfg(target_os = "windows")]
    generation: u64,
    #[cfg(target_os = "windows")]
    current_generation: Arc<AtomicU64>,
    keep_loaded_secs: u32,
    released: Arc<AtomicBool>,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl TranscriptionEngine for TauriNativeTranscriptionEngine {
    fn start(
        &self,
        _session_id: SessionId,
        context: Arc<DictationContext>,
        partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<Arc<dyn TranscriptionSession>, BackendError>> {
        // 两个 Windows runtime 独立计代；切到 Sherpa 不能让 Foundry 的释放永久失效。
        // 设置页 prepare/release 也共享这些代次，旧会话不能卸载新启用的模型。
        #[cfg(target_os = "windows")]
        let current_generation = Arc::clone(
            if context.asr.provider_type == openless_core::LocalAsrRuntime::Foundry.provider_id() {
                &self.dependencies.foundry_generation
            } else {
                &self.dependencies.sherpa_generation
            },
        );
        #[cfg(target_os = "windows")]
        let generation = current_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let keep_loaded_secs = context.asr.keep_loaded_secs.unwrap_or(0);

        #[cfg(target_os = "windows")]
        let foundry = Arc::clone(&self.dependencies.foundry);
        #[cfg(target_os = "windows")]
        let backend = Arc::clone(&self.backend);
        #[cfg(target_os = "windows")]
        let sherpa = Arc::clone(&self.dependencies.sherpa);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let qwen_cache = Arc::clone(&self.dependencies.qwen_cache);
        #[cfg(target_os = "macos")]
        let whisper_cache = Arc::clone(&self.dependencies.whisper_cache);
        let model_store = self.model_store.clone();

        Box::pin(async move {
            let provider_type = context.asr.provider_type.as_str();
            #[cfg(target_os = "windows")]
            let (kind, label_model) = if provider_type
                == openless_core::LocalAsrRuntime::Foundry.provider_id()
            {
                let model = context
                    .asr
                    .model
                    .clone()
                    .filter(|model| {
                        openless_core::LocalAsrTarget::parse(
                            openless_core::LocalAsrRuntime::Foundry,
                            model,
                        )
                        .is_ok()
                    })
                    .unwrap_or_else(|| {
                        openless_core::LocalAsrRuntime::Foundry
                            .default_model()
                            .to_string()
                    });
                (
                    TauriNativeTranscriptionSessionKind::Foundry {
                        provider: Arc::new(crate::asr::local::FoundryLocalWhisperAsr::new(
                            Arc::clone(&foundry),
                            model.clone(),
                            context
                                .asr
                                .runtime
                                .clone()
                                .unwrap_or_else(|| "auto".to_string()),
                            context.asr.language.clone(),
                        )),
                        runtime: foundry,
                    },
                    Some(model),
                )
            } else if provider_type == openless_core::LocalAsrRuntime::SherpaOnnx.provider_id() {
                let model = context
                    .asr
                    .model
                    .clone()
                    .filter(|model| {
                        openless_core::LocalAsrTarget::parse(
                            openless_core::LocalAsrRuntime::SherpaOnnx,
                            model,
                        )
                        .is_ok()
                    })
                    .unwrap_or_else(|| {
                        openless_core::LocalAsrRuntime::SherpaOnnx
                            .default_model()
                            .to_string()
                    });
                let token_sink = Arc::clone(&partials);
                let token_offset = Arc::new(AtomicU64::new(0));
                let handler_offset = Arc::clone(&token_offset);
                let token_handler = Arc::new(move |piece: String| {
                    let offset =
                        handler_offset.fetch_add(piece.chars().count() as u64, Ordering::AcqRel);
                    if let Err(error) = token_sink.publish(TextStreamChunk {
                        text: piece,
                        offset,
                    }) {
                        log::warn!("[core-adapter] publish sherpa partial failed: {error}");
                    }
                });
                let target = openless_core::LocalAsrTarget::parse(
                    openless_core::LocalAsrRuntime::SherpaOnnx,
                    model.clone(),
                )?;
                let store = model_store.as_ref().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::Unsupported,
                        "Core model store is unavailable",
                    )
                })?;
                if !store.is_installed(&target)? {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "Sherpa model is not downloaded",
                    ));
                }
                let model_dir = store.runtime_model_dir(&target)?;
                let provider = crate::asr::local::SherpaOnnxAsr::new_for_model(
                    Arc::clone(&sherpa),
                    model.clone(),
                    model_dir,
                    context.asr.language.clone(),
                    Some(token_handler),
                )
                .await
                .map_err(map_native_asr_error)?;
                (
                    TauriNativeTranscriptionSessionKind::Sherpa {
                        provider: Arc::new(provider),
                        runtime: sherpa,
                    },
                    Some(model),
                )
            } else {
                return Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    format!("native ASR provider is unavailable: {provider_type}"),
                ));
            };

            #[cfg(any(target_os = "macos", target_os = "linux"))]
            let (kind, label_model) = if crate::asr::local::is_local_qwen3(provider_type) {
                let backend = crate::asr::local::qwen_backend_for_provider(provider_type)
                    .ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Unsupported,
                            format!("Qwen backend is unavailable: {provider_type}"),
                        )
                    })?;
                let model = context
                    .asr
                    .model
                    .as_deref()
                    .and_then(crate::asr::local::ModelId::from_wire_id)
                    .filter(|model| model.is_qwen())
                    .ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Provider,
                            "local Qwen model is not configured",
                        )
                    })?;
                let model_id = model.as_str().to_string();
                let target = openless_core::LocalAsrTarget::parse(
                    openless_core::LocalAsrRuntime::Generic,
                    model.as_str(),
                )?;
                let store = model_store.as_ref().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::Unsupported,
                        "Core model store is unavailable",
                    )
                })?;
                if !store.is_installed(&target)? {
                    return Err(BackendError::new(
                        BackendErrorCode::InvalidState,
                        "local Qwen model is not downloaded",
                    ));
                }
                let model_dir = store.runtime_model_dir(&target)?;
                let cache = Arc::clone(&qwen_cache);
                let load_cache = Arc::clone(&cache);
                let load_model_id = model_id.clone();
                let engine = tauri::async_runtime::spawn_blocking(move || {
                    load_cache.get_or_load(backend, &load_model_id, &model_dir)
                })
                .await
                .map_err(map_native_asr_error)?
                .map_err(map_native_asr_error)?;
                (
                    TauriNativeTranscriptionSessionKind::Qwen {
                        operation_id: engine.next_operation_id(),
                        engine,
                        cache,
                        pcm: Arc::new(Mutex::new(Vec::new())),
                        cancelled: Arc::new(AtomicBool::new(false)),
                    },
                    Some(model_id),
                )
            } else {
                #[cfg(target_os = "macos")]
                {
                    if crate::asr::local::is_local_whisper(provider_type) {
                        let model_id = context
                            .asr
                            .model
                            .clone()
                            .filter(|model| {
                                crate::asr::local::ModelId::from_wire_id(model)
                                    .is_some_and(|model| model.is_whisper())
                            })
                            .unwrap_or_else(|| crate::asr::local::WHISPER_MODEL_ID.to_string());
                        let target = openless_core::LocalAsrTarget::parse(
                            openless_core::LocalAsrRuntime::Generic,
                            model_id.clone(),
                        )?;
                        let store = model_store.as_ref().ok_or_else(|| {
                            BackendError::new(
                                BackendErrorCode::Unsupported,
                                "Core model store is unavailable",
                            )
                        })?;
                        if !store.is_installed(&target)? {
                            return Err(BackendError::new(
                                BackendErrorCode::InvalidState,
                                "local Whisper model is not downloaded",
                            ));
                        }
                        let model_dir = store.runtime_model_dir(&target)?;
                        let model_path =
                            crate::asr::local::whisper_model_path_for_model(&model_id, &model_dir)
                                .map_err(map_native_asr_error)?;
                        let cache = Arc::clone(&whisper_cache);
                        let load_cache = Arc::clone(&cache);
                        let load_model_id = model_id.clone();
                        let engine = tauri::async_runtime::spawn_blocking(move || {
                            load_cache.get_or_load(&load_model_id, &model_path)
                        })
                        .await
                        .map_err(map_native_asr_error)?
                        .map_err(map_native_asr_error)?;
                        (
                            TauriNativeTranscriptionSessionKind::Whisper {
                                engine,
                                cache,
                                language: context
                                    .asr
                                    .language
                                    .clone()
                                    .unwrap_or_else(|| "auto".to_string()),
                                pcm: Arc::new(Mutex::new(Vec::new())),
                                cancelled: Arc::new(AtomicBool::new(false)),
                            },
                            Some(model_id),
                        )
                    } else if crate::asr::local::is_apple_speech(provider_type) {
                        let locale =
                            context.polish.working_languages.first().and_then(|name| {
                                crate::asr::local::native_name_to_apple_locale(name)
                            });
                        (
                            TauriNativeTranscriptionSessionKind::AppleSpeech(Arc::new(
                                crate::asr::local::AppleSpeechAsr::new(locale),
                            )),
                            None,
                        )
                    } else {
                        return Err(BackendError::new(
                            BackendErrorCode::Unsupported,
                            format!("native ASR provider is unavailable: {provider_type}"),
                        ));
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    return Err(BackendError::new(
                        BackendErrorCode::Unsupported,
                        format!("native ASR provider is unavailable: {provider_type}"),
                    ));
                }
            };

            #[cfg(target_os = "windows")]
            let _ = partials;

            let asr_call_label =
                openless_core::AsrCallLabel::new(context.asr.provider_type.clone(), label_model);

            Ok(Arc::new(TauriNativeTranscriptionSession {
                kind,
                asr_call_label,
                #[cfg(target_os = "windows")]
                backend,
                #[cfg(target_os = "windows")]
                session_id: _session_id,
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                partials,
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                next_offset: Arc::new(AtomicU64::new(0)),
                #[cfg(target_os = "windows")]
                generation,
                #[cfg(target_os = "windows")]
                current_generation,
                keep_loaded_secs,
                released: Arc::new(AtomicBool::new(false)),
            }) as Arc<dyn TranscriptionSession>)
        })
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl CoreAudioConsumer for TauriNativeTranscriptionSession {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        match &self.kind {
            #[cfg(target_os = "windows")]
            TauriNativeTranscriptionSessionKind::Foundry { provider, .. } => {
                LegacyAudioConsumer::consume_pcm_chunk(provider.as_ref(), pcm);
            }
            #[cfg(target_os = "windows")]
            TauriNativeTranscriptionSessionKind::Sherpa { provider, .. } => {
                LegacyAudioConsumer::consume_pcm_chunk(provider.as_ref(), pcm);
            }
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            TauriNativeTranscriptionSessionKind::Qwen { pcm: buffer, .. } => {
                buffer.lock().extend_from_slice(pcm);
            }
            #[cfg(target_os = "macos")]
            TauriNativeTranscriptionSessionKind::Whisper { pcm: buffer, .. } => {
                buffer.lock().extend_from_slice(pcm);
            }
            #[cfg(target_os = "macos")]
            TauriNativeTranscriptionSessionKind::AppleSpeech(provider) => {
                LegacyAudioConsumer::consume_pcm_chunk(provider.as_ref(), pcm);
            }
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl TranscriptionSession for TauriNativeTranscriptionSession {
    fn asr_call_label(&self) -> Option<openless_core::AsrCallLabel> {
        Some(self.asr_call_label.clone())
    }

    fn finish(&self) -> BoxFuture<'static, Result<TranscriptOutput, BackendError>> {
        #[cfg(target_os = "windows")]
        use crate::asr::local::foundry_runtime as foundry;

        let session = self.clone();
        let kind = self.kind.clone();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let partials = Arc::clone(&self.partials);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let next_offset = Arc::clone(&self.next_offset);
        Box::pin(async move {
            #[cfg(target_os = "windows")]
            let mut recovery = None;
            // 不在各分支的 ? 前安排释放：所有结果统一从这里经过。cancel 与 finish
            // 可能并发，released 保证资源收尾只安排一次；旧 generation 不清掉新会话。
            let result = async {
            let output = match kind {
                #[cfg(target_os = "windows")]
                TauriNativeTranscriptionSessionKind::Foundry { provider, .. } => {
                    let timeout = openless_core::provider_rules::native_transcribe_timeout(
                        "foundry-local-whisper", provider.buffer_duration_ms());
                    let result = match provider
                        .transcribe_with_fallback_notice(
                            timeout,
                            foundry_transcription_notices(
                                Arc::clone(&session.backend),
                                session.session_id,
                                Arc::clone(&session.released),
                            ),
                        )
                        .await
                    {
                        Ok(result) => result,
                        Err(error)
                            if foundry::is_terminal_foundry_fallback_error(
                                &error,
                            ) =>
                        {
                            log::error!(
                                "[core-adapter] Foundry retranscription reached terminal fallback error: {error:#}"
                            );
                            let mut backend_error = BackendError::new(
                                BackendErrorCode::Provider,
                                foundry::FOUNDRY_FALLBACK_TERMINAL_USER_MESSAGE,
                            );
                            backend_error.details = Some(serde_json::json!({
                                "terminal": "foundry_fallback"
                            }));
                            return Err(backend_error);
                        }
                        Err(error) => return Err(map_native_asr_error(error).retryable(true)),
                    };
                    recovery = result.primary_recovery;
                    result.raw
                }
                #[cfg(target_os = "windows")]
                TauriNativeTranscriptionSessionKind::Sherpa { provider, .. } => {
                    let timeout = openless_core::provider_rules::native_transcribe_timeout(
                        "sherpa-onnx-local", provider.buffer_duration_ms());
                    let output = provider
                        .transcribe(timeout)
                        .await
                        .map_err(map_native_asr_error)?;
                    output
                }
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                TauriNativeTranscriptionSessionKind::Qwen {
                    engine,
                    cache: _,
                    pcm,
                    cancelled,
                    operation_id,
                } => {
                    let bytes = std::mem::take(&mut *pcm.lock());
                    let duration_ms = pcm_duration_ms(&bytes);
                    let samples = pcm_i16_to_f32(&bytes);
                    let sink = Arc::clone(&partials);
                    let offset = Arc::clone(&next_offset);
                    let worker_cancelled = Arc::clone(&cancelled);
                    let cancelled_for_tokens = Arc::clone(&cancelled);
                    let timeout = openless_core::provider_rules::native_transcribe_timeout(
                        "local-qwen3", duration_ms);
                    let text = await_native_transcription(timeout, async move {
                        tauri::async_runtime::spawn_blocking(move || {
                        engine.transcribe_dictation_with_handler(
                            operation_id,
                            worker_cancelled.as_ref(),
                            samples,
                            move |piece: &str| {
                                if cancelled_for_tokens.load(Ordering::Acquire) {
                                    return;
                                }
                                let offset = offset
                                    .fetch_add(piece.chars().count() as u64, Ordering::AcqRel);
                                let _ = sink.publish(TextStreamChunk {
                                    text: piece.to_string(),
                                    offset,
                                });
                            },
                        )
                        })
                    .await
                    .map_err(map_native_asr_error)?
                    .map_err(map_native_asr_error)
                    }).await?;
                    if cancelled.load(Ordering::Acquire) {
                        return Err(cancelled_native_asr_error());
                    }
                    crate::asr::RawTranscript { text, duration_ms }
                }
                #[cfg(target_os = "macos")]
                TauriNativeTranscriptionSessionKind::Whisper {
                    engine,
                    cache: _,
                    language,
                    pcm,
                    cancelled,
                } => {
                    let bytes = std::mem::take(&mut *pcm.lock());
                    let duration_ms = pcm_duration_ms(&bytes);
                    let samples = pcm_i16_to_f32(&bytes);
                    let timeout = openless_core::provider_rules::native_transcribe_timeout(
                        "local-whisper", duration_ms);
                    let text = await_native_transcription(timeout, async move {
                        tauri::async_runtime::spawn_blocking(move || {
                            engine.transcribe(&samples, &language)
                        })
                        .await
                        .map_err(map_native_asr_error)?
                        .map_err(map_native_asr_error)
                    }).await?;
                    if cancelled.load(Ordering::Acquire) {
                        return Err(cancelled_native_asr_error());
                    }
                    crate::asr::RawTranscript { text, duration_ms }
                }
                #[cfg(target_os = "macos")]
                TauriNativeTranscriptionSessionKind::AppleSpeech(provider) => {
                    let timeout = openless_core::provider_rules::native_transcribe_timeout(
                        "apple-speech", provider.buffer_duration_ms());
                    await_native_transcription(timeout, async {
                        provider.transcribe().await.map_err(map_native_asr_error)
                    }).await?
                }
            };
            Ok(TranscriptOutput {
                text: output.text,
                duration_ms: output.duration_ms,
            })
            }.await;
            if result.is_err() {
                session.cancel_native();
            }
            #[cfg(target_os = "windows")]
            session.release_native(result.is_err(), recovery);
            #[cfg(not(target_os = "windows"))]
            session.release_native(result.is_err());
            result
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self.clone();
        Box::pin(async move {
            session.cancel_native();
            #[cfg(target_os = "windows")]
            session.release_native(true, None);
            #[cfg(not(target_os = "windows"))]
            session.release_native(true);
            Ok(())
        })
    }
}

#[cfg(target_os = "windows")]
fn foundry_transcription_notices(
    backend: BackendSlot,
    session_id: SessionId,
    released: Arc<AtomicBool>,
) -> crate::asr::local::foundry_runtime::FoundryFallbackNoticeCallback {
    Arc::new(move |notice| {
        // 直接进入同一Core事件流：CPU首次下载可能很久，不能等finish返回后
        // 才发提示。回调只持Weak Backend，finish/失败/cancel共用released收尾。
        if released.load(Ordering::Acquire) {
            return;
        }
        let Some(backend) = backend.lock().as_ref().and_then(std::sync::Weak::upgrade) else {
            return;
        };
        backend.event_publisher().publish(
            Some(session_id),
            openless_core::BackendEventKind::Notification(openless_core::NotificationPayload {
                level: openless_core::NotificationLevel::Warning,
                message: notice.message().into(),
            }),
        );
    })
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl TauriNativeTranscriptionSession {
    fn cancel_native(&self) {
        match &self.kind {
            #[cfg(target_os = "windows")]
            TauriNativeTranscriptionSessionKind::Foundry { provider, .. } => provider.cancel(),
            #[cfg(target_os = "windows")]
            TauriNativeTranscriptionSessionKind::Sherpa { provider, .. } => provider.cancel(),
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            TauriNativeTranscriptionSessionKind::Qwen {
                engine,
                pcm,
                cancelled,
                operation_id,
                ..
            } => {
                cancelled.store(true, Ordering::Release);
                pcm.lock().clear();
                engine.cancel_operation(*operation_id);
            }
            #[cfg(target_os = "macos")]
            TauriNativeTranscriptionSessionKind::Whisper { pcm, cancelled, .. } => {
                cancelled.store(true, Ordering::Release);
                pcm.lock().clear();
            }
            #[cfg(target_os = "macos")]
            TauriNativeTranscriptionSessionKind::AppleSpeech(provider) => provider.cancel(),
        }
    }

    fn release_native(
        &self,
        discard: bool,
        #[cfg(target_os = "windows")] recovery: Option<
            crate::asr::local::foundry_runtime::FoundryPrimaryRecoveryToken,
        >,
    ) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        match &self.kind {
            #[cfg(target_os = "windows")]
            TauriNativeTranscriptionSessionKind::Foundry { runtime, .. } => {
                schedule_foundry_release(
                    Arc::clone(runtime),
                    recovery,
                    self.keep_loaded_secs,
                    self.generation,
                    Arc::clone(&self.current_generation),
                );
            }
            #[cfg(target_os = "windows")]
            TauriNativeTranscriptionSessionKind::Sherpa { runtime, .. } => {
                schedule_sherpa_release(
                    Arc::clone(runtime),
                    self.keep_loaded_secs,
                    self.generation,
                    Arc::clone(&self.current_generation),
                );
            }
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            TauriNativeTranscriptionSessionKind::Qwen { engine, cache, .. } => {
                cache.finish_use(engine, discard);
                if !discard {
                    schedule_qwen_release(
                        Arc::clone(cache),
                        Arc::downgrade(engine),
                        self.keep_loaded_secs,
                    );
                }
            }
            #[cfg(target_os = "macos")]
            TauriNativeTranscriptionSessionKind::Whisper { engine, cache, .. } => {
                cache.finish_use(engine, discard);
                if !discard {
                    schedule_whisper_release(
                        Arc::clone(cache),
                        Arc::downgrade(engine),
                        self.keep_loaded_secs,
                    );
                }
            }
            #[cfg(target_os = "macos")]
            TauriNativeTranscriptionSessionKind::AppleSpeech(_) => {}
        }
        let _ = discard;
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
async fn await_native_transcription<T>(
    timeout: std::time::Duration,
    operation: impl std::future::Future<Output = Result<T, BackendError>>,
) -> Result<T, BackendError> {
    // timeout 只停止等待。调用者必须 cancel_native 并驱逐自己的 cache；
    // spawn_blocking / Whisper C API 不会因 future 被 drop 而自动停止执行。
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| {
            BackendError::new(
                BackendErrorCode::Provider,
                "native ASR transcription timed out",
            )
            .retryable(true)
        })?
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pcm_i16_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32_768.0)
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pcm_duration_ms(bytes: &[u8]) -> u64 {
    (bytes.len() as u64 / 2).saturating_mul(1_000) / 16_000
}

#[cfg(target_os = "windows")]
fn schedule_foundry_release(
    runtime: Arc<crate::asr::local::FoundryLocalRuntime>,
    recovery: Option<crate::asr::local::foundry_runtime::FoundryPrimaryRecoveryToken>,
    keep_loaded_secs: u32,
    generation: u64,
    current_generation: Arc<AtomicU64>,
) {
    tauri::async_runtime::spawn(async move {
        if current_generation.load(Ordering::Acquire) != generation {
            return;
        }
        if let Some(recovery) = recovery.as_ref() {
            if keep_loaded_secs > 0 {
                if !runtime
                    .restore_primary_for_keep_alive(recovery)
                    .await
                    .unwrap_or(false)
                {
                    return;
                }
            }
        }
        if keep_loaded_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(keep_loaded_secs as u64)).await;
        }
        if current_generation.load(Ordering::Acquire) != generation {
            return;
        }
        let result = match recovery.as_ref() {
            Some(recovery) => runtime
                .release_primary_if_current(recovery)
                .await
                .map(|_| ()),
            None => runtime
                .release_if_generation(&current_generation, generation)
                .await
                .map(|_| ()),
        };
        if let Err(error) = result {
            log::warn!("[core-adapter] release Foundry runtime failed: {error:#}");
        }
    });
}

#[cfg(target_os = "windows")]
fn schedule_sherpa_release(
    runtime: Arc<crate::asr::local::SherpaOnnxRuntime>,
    keep_loaded_secs: u32,
    generation: u64,
    current_generation: Arc<AtomicU64>,
) {
    tauri::async_runtime::spawn(async move {
        if keep_loaded_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(keep_loaded_secs as u64)).await;
        }
        if current_generation.load(Ordering::Acquire) == generation {
            if let Err(error) = runtime
                .release_if_generation(&current_generation, generation)
                .await
            {
                log::warn!("[core-adapter] release sherpa runtime failed: {error:#}");
            }
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn schedule_qwen_release(
    cache: Arc<crate::asr::local::LocalAsrCache>,
    engine: std::sync::Weak<crate::asr::local::LocalQwenEngine>,
    keep_loaded_secs: u32,
) {
    tauri::async_runtime::spawn(async move {
        let threshold = std::time::Duration::from_secs(keep_loaded_secs as u64);
        if !threshold.is_zero() {
            tokio::time::sleep(threshold).await;
        }
        cache.release_current_if_idle(&engine, threshold);
    });
}

#[cfg(target_os = "macos")]
fn schedule_whisper_release(
    cache: Arc<crate::asr::local::LocalWhisperCache>,
    engine: std::sync::Weak<crate::asr::local::WhisperEngine>,
    keep_loaded_secs: u32,
) {
    tauri::async_runtime::spawn(async move {
        let threshold = std::time::Duration::from_secs(keep_loaded_secs as u64);
        if !threshold.is_zero() {
            tokio::time::sleep(threshold).await;
        }
        cache.release_current_if_idle(&engine, threshold);
    });
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn cancelled_native_asr_error() -> BackendError {
    BackendError::new(BackendErrorCode::Cancelled, "native ASR request cancelled")
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn map_native_asr_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(
        BackendErrorCode::Provider,
        format!("native ASR provider failed: {error}"),
    )
}

pub(crate) struct TauriAudioRecorder {
    app: AppHandleSlot,
    backend: BackendSlot,
}

struct AudioConsumerBridge {
    inner: Arc<dyn CoreAudioConsumer>,
}

impl LegacyAudioConsumer for AudioConsumerBridge {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.inner.consume_pcm_chunk(pcm);
    }
}

struct TauriActiveRecording {
    recorder: Option<Recorder>,
    archive: Option<Arc<TauriRecordingArchive>>,
    _mute: Option<crate::audio_mute::AudioMuteGuard>,
}

struct TauriRecordingArchive {
    path: PathBuf,
    available: Arc<AtomicBool>,
}

impl TauriRecordingArchive {
    fn new(path: PathBuf, available: bool) -> Self {
        Self {
            path,
            available: Arc::new(AtomicBool::new(available)),
        }
    }
}

impl RecordingArchive for TauriRecordingArchive {
    fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let wav = tokio::fs::read(&path).await.map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("read dictation recording archive: {error}"),
                )
            })?;
            if wav.len() <= 44
                || &wav[..4] != b"RIFF"
                || &wav[8..12] != b"WAVE"
                || !(wav.len() - 44).is_multiple_of(2)
            {
                return Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "dictation recording archive is not canonical PCM WAV",
                ));
            }
            Ok(wav[44..].to_vec())
        })
    }

    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let path = self.path.clone();
        let available = Arc::clone(&self.available);
        Box::pin(async move {
            if !available.load(Ordering::Acquire) {
                return Ok(());
            }
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    available.store(false, Ordering::Release);
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    available.store(false, Ordering::Release);
                    Ok(())
                }
                Err(error) => {
                    log::warn!(
                        "[core-adapter] 清理成功口述的归档录音失败 {}: {error}",
                        path.display()
                    );
                    Err(BackendError::new(
                        BackendErrorCode::Persistence,
                        format!("discard dictation recording archive: {error}"),
                    ))
                }
            }
        })
    }
}

impl ActiveRecording for TauriActiveRecording {
    fn archive(&self) -> Option<Arc<dyn RecordingArchive>> {
        self.archive
            .as_ref()
            .map(|archive| archive.clone() as Arc<dyn RecordingArchive>)
    }

    fn stop(mut self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            let recorder = self.recorder.take().ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Tauri recorder was already stopped",
                )
            })?;
            tauri::async_runtime::spawn_blocking(move || {
                recorder.stop();
                Ok(())
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Internal,
                    format!("join Tauri recorder stop task: {error}"),
                )
            })?
        })
    }
}

/// 归档和系统输出静音沿用1.x的best-effort语义：目录不可写、没有默认扬声器或
/// macOS音量脚本失败，都不意味着输入麦克风不可用。分别告警并关闭失败的辅助项，
/// 让后面的Recorder::start独立报告真实采集错误；成功的静音guard仍由录音资源持有。
fn prepare_recording_options<M>(
    archive_path: Result<Option<PathBuf>, impl std::fmt::Display>,
    mute_enabled: bool,
    activate_mute: impl FnOnce() -> Result<M, String>,
) -> (Option<PathBuf>, Option<M>) {
    let archive_path = archive_path
        .map_err(|error| log::warn!("[recordings] archive unavailable; capture continues: {error}"))
        .ok()
        .flatten();
    let mute = mute_enabled.then(activate_mute).and_then(|result| {
        result
            .map_err(|error| {
                log::warn!("[audio-mute] failed to mute output; capture continues: {error}")
            })
            .ok()
    });
    (archive_path, mute)
}

impl AudioRecorder for TauriAudioRecorder {
    fn start(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
        consumer: Arc<dyn CoreAudioConsumer>,
        progress: Arc<dyn RecordingProgressSink>,
    ) -> BoxFuture<'static, Result<Box<dyn ActiveRecording>, BackendError>> {
        let app = Arc::clone(&self.app);
        let backend = Arc::clone(&self.backend);
        Box::pin(async move {
            #[cfg(not(mobile))]
            if let Some(app) = app.lock().clone() {
                let state = app.state::<crate::commands::MicrophoneMonitorState>();
                let preview = state.lock().take();
                if let Some(preview) = preview {
                    preview.stop();
                }
            }
            // QA/划词语音沿用1.x不落盘语义；不要先创建WAV，再依赖停止时删除。
            let archive_path = context
                .recording
                .archive_enabled
                .then(|| crate::persistence::recording_path_for_session(&session_id.to_string()))
                .transpose();
            let microphone = context.recording.microphone_device_name.clone();
            let recording_plan = context.recording.clone();
            let fault_progress = Arc::clone(&progress);
            let (recording, runtime_errors) = tauri::async_runtime::spawn_blocking(move || {
                if recording_plan.archive_enabled {
                    if let Err(error) = crate::persistence::prune_recordings(
                        recording_plan.retention_days,
                        recording_plan.max_entries,
                    ) {
                        log::warn!("[recordings] prune before capture failed: {error:#}");
                    }
                }
                let (archive_path, mute) = prepare_recording_options(
                    archive_path,
                    recording_plan.mute_during_recording,
                    crate::audio_mute::AudioMuteGuard::activate,
                );
                let started_at = Instant::now();
                let level_progress = Arc::clone(&progress);
                let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
                    let _ = level_progress
                        .publish_level(started_at.elapsed().as_millis() as u64, level);
                });
                let consumer: Arc<dyn LegacyAudioConsumer> =
                    Arc::new(AudioConsumerBridge { inner: consumer });
                let recorder_archive_path = archive_path.clone();
                let start_result =
                    Recorder::start(microphone, consumer, level_handler, recorder_archive_path);
                let (recorder, runtime_errors, archive_active) = match start_result {
                    Ok(started) => started,
                    Err(error) => {
                        // Recorder::start may create the WAV before the native
                        // stream fails. No archive handle is returned on this
                        // path, so remove that partial file here.
                        if let Some(path) = &archive_path {
                            let _ = std::fs::remove_file(path);
                        }
                        return Err(map_recorder_error(error));
                    }
                };
                let recording = Box::new(TauriActiveRecording {
                    recorder: Some(recorder),
                    archive: archive_path
                        .map(|path| Arc::new(TauriRecordingArchive::new(path, archive_active))),
                    _mute: mute,
                }) as Box<dyn ActiveRecording>;
                Ok((recording, runtime_errors))
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Internal,
                    format!("join Tauri recorder start task: {error}"),
                )
            })??;
            tauri::async_runtime::spawn(async move {
                let runtime_error =
                    tauri::async_runtime::spawn_blocking(move || runtime_errors.recv()).await;
                let Ok(Ok(runtime_error)) = runtime_error else {
                    return;
                };
                let error = map_recorder_error(runtime_error);
                let _ = fault_progress.publish(openless_core::RecordingEvent::Fatal(error.clone()));
                let backend = backend.lock().as_ref().and_then(std::sync::Weak::upgrade);
                if let Some(backend) = backend {
                    if let Err(report_error) =
                        backend.report_recording_fault(session_id, error).await
                    {
                        if report_error.code != BackendErrorCode::InvalidState {
                            log::warn!("[recorder] report runtime fault failed: {report_error}");
                        }
                    }
                }
            });
            Ok(recording)
        })
    }
}

fn map_recorder_error(error: RecorderError) -> BackendError {
    let code = match error {
        RecorderError::PermissionDenied => BackendErrorCode::PermissionDenied,
        RecorderError::NoInputDevice | RecorderError::EngineFailed(_) => BackendErrorCode::Platform,
    };
    BackendError::new(code, error.user_message())
}

#[derive(Clone)]
pub(crate) struct TauriTextInserter {
    app: AppHandleSlot,
    insertion_target: Option<crate::selection::SelectionInsertionTarget>,
    #[cfg(target_os = "windows")]
    windows_ime: Arc<crate::windows_ime_session::WindowsImeSessionController>,
}

#[derive(Default)]
struct TauriEditObservationAdapter {
    watcher: Mutex<Option<crate::host_document::EditWatcher>>,
}

impl EditObservationAdapter for TauriEditObservationAdapter {
    fn arm(
        &self,
        typed_text: String,
        sink: Arc<dyn EditObservationSink>,
    ) -> Result<(), BackendError> {
        *self.watcher.lock() =
            crate::host_document::watch_for_edits(typed_text, move |edit| sink.publish(edit));
        Ok(())
    }

    fn disarm(&self) {
        *self.watcher.lock() = None;
    }
}

impl TauriTextInserter {
    fn new(app: AppHandleSlot) -> Self {
        Self {
            app,
            insertion_target: None,
            #[cfg(target_os = "windows")]
            windows_ime: Arc::new(crate::windows_ime_session::WindowsImeSessionController::new()),
        }
    }
}

/// 只有实际流式输入才切 ABC。TIS 失败是平台能力降级，不能阻断无需 TIS 的
/// 一次性粘贴；Core 读取 supports_streaming 回执，仍独占 reconciliation 策略。
#[cfg(any(target_os = "macos", test))]
async fn prepare_streaming_input_source<T: Default, E: std::fmt::Display, F>(
    streaming: bool,
    switch: impl FnOnce() -> F,
) -> (T, bool)
where
    F: std::future::Future<Output = Result<T, E>>,
{
    if !streaming {
        return (T::default(), false);
    }
    match switch().await {
        Ok(previous) => (previous, true),
        Err(error) => {
            log::warn!(
                "[core-adapter] ASCII input source unavailable; use one-shot insertion: {error}"
            );
            (T::default(), false)
        }
    }
}

impl CoreTextInserter for TauriTextInserter {
    fn capture_target(&self) -> Option<Arc<dyn CoreTextInserter>> {
        Some(Arc::new(Self {
            insertion_target: Some(crate::selection::capture_selection_insertion_target()),
            ..self.clone()
        }))
    }

    fn begin(
        &self,
        session_id: SessionId,
        context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>> {
        let app = Arc::clone(&self.app);
        let insertion_target = self.insertion_target.clone();
        #[cfg(target_os = "windows")]
        let windows_ime = Arc::clone(&self.windows_ime);
        Box::pin(async move {
            // Retain the target captured before context/credential awaits in
            // the insertion resource. Every later write must reactivate it; a
            // failed restore becomes an explicit error instead of typing into
            // whichever application happens to be focused at the end.
            let insertion_target = insertion_target
                .unwrap_or_else(crate::selection::capture_selection_insertion_target);
            #[cfg(target_os = "windows")]
            let prepared = if context.insertion.windows_insertion_mode
                == openless_core::shared_types::WindowsInsertionMode::Tsf
            {
                let controller = Arc::clone(&windows_ime);
                Some(
                    tauri::async_runtime::spawn_blocking(move || controller.prepare_session())
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                BackendErrorCode::Internal,
                                format!("join Windows IME prepare task: {error}"),
                            )
                        })?,
                )
            } else {
                None
            };
            #[cfg(target_os = "macos")]
            let (app_handle, previous_input_source, streaming_ready) = {
                let app_handle = app.lock().clone().ok_or_else(|| {
                    BackendError::new(
                        BackendErrorCode::InvalidState,
                        "Tauri AppHandle is not bound yet",
                    )
                })?;
                let (previous, streaming_ready) = prepare_streaming_input_source(
                    context.uses_llm_polisher()
                        && openless_core::streaming_insert::streaming_insert_eligible(
                        context.insertion.streaming,
                        context.polish.translation_active,
                        context.polish.chinese_script_preference
                            == openless_core::shared_types::ChineseScriptPreference::Traditional,
                        false,
                    ),
                    || crate::unicode_keystroke::switch_to_ascii(&app_handle),
                )
                .await;
                (app_handle, previous, streaming_ready)
            };
            #[cfg(not(target_os = "macos"))]
            let _ = app;
            Ok(Arc::new(TauriTextInsertionSession {
                session_id,
                context,
                insertion_target,
                finished: Arc::new(AtomicBool::new(false)),
                #[cfg(target_os = "windows")]
                windows_ime,
                #[cfg(target_os = "windows")]
                prepared: Arc::new(Mutex::new(prepared)),
                #[cfg(target_os = "macos")]
                app: app_handle,
                #[cfg(target_os = "macos")]
                previous_input_source: Arc::new(Mutex::new(previous_input_source)),
                #[cfg(target_os = "macos")]
                streaming_ready,
            }) as Arc<dyn TextInsertionSession>)
        })
    }
}

#[derive(Clone)]
struct TauriTextInsertionSession {
    session_id: SessionId,
    context: Arc<DictationContext>,
    insertion_target: crate::selection::SelectionInsertionTarget,
    finished: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    windows_ime: Arc<crate::windows_ime_session::WindowsImeSessionController>,
    #[cfg(target_os = "windows")]
    prepared: Arc<Mutex<Option<crate::windows_ime_session::PreparedWindowsImeSession>>>,
    #[cfg(target_os = "macos")]
    app: AppHandle,
    #[cfg(target_os = "macos")]
    previous_input_source: Arc<Mutex<Option<crate::unicode_keystroke::PreviousInputSource>>>,
    #[cfg(target_os = "macos")]
    streaming_ready: bool,
}

impl TauriTextInsertionSession {
    fn restore_insertion_target(&self) -> Result<(), BackendError> {
        crate::selection::reactivate_selection_insertion_target(&self.insertion_target)
            .then_some(())
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    "original text insertion target is unavailable",
                )
            })
    }

    async fn write_chunk(&self, text: String) -> Result<InsertWriteResult, BackendError> {
        self.restore_insertion_target()?;
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        {
            let chunk = text.clone();
            #[cfg(target_os = "windows")]
            let newline_mode = self.context.insertion.windows_sendinput_newline_mode;
            #[cfg(target_os = "macos")]
            let newline_mode = self.context.insertion.macos_newline_mode;
            let finished = Arc::clone(&self.finished);
            let written = tauri::async_runtime::spawn_blocking(move || {
                if finished.load(Ordering::Acquire) {
                    return 0;
                }
                #[cfg(target_os = "windows")]
                let result = crate::unicode_keystroke::type_unicode_chunk_with_options(
                    &chunk,
                    crate::unicode_keystroke::WindowsSendInputOptions { newline_mode },
                );
                #[cfg(target_os = "macos")]
                let result =
                    crate::unicode_keystroke::type_unicode_chunk_with_options(&chunk, newline_mode);
                #[cfg(target_os = "linux")]
                let result = crate::unicode_keystroke::type_unicode_chunk(&chunk);
                match result {
                    Ok(written) => written,
                    Err(error) => error.typed_chars(),
                }
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Internal,
                    format!("join Tauri streaming insertion task: {error}"),
                )
            })?;
            Ok(InsertWriteResult {
                written_chars: written,
            })
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            let _ = text;
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "streaming insertion is unavailable on this platform",
            ))
        }
    }

    async fn insert_final(&self, text: String) -> Result<InsertOutcome, BackendError> {
        if let Err(error) = self.restore_insertion_target() {
            // 原目标不可用时只复制，不能向当前焦点粘贴或发送按键。
            #[cfg(target_os = "windows")]
            if self.context.insertion.allow_non_tsf_fallback {
                return self.copy_fallback(text).await;
            }
            return Err(error);
        }
        #[cfg(target_os = "windows")]
        {
            let status = match self.context.insertion.windows_insertion_mode {
                openless_core::shared_types::WindowsInsertionMode::Tsf => {
                    let prepared = self.prepared.lock().take().ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::InvalidState,
                            "prepared Windows IME session is unavailable",
                        )
                    })?;
                    let request = crate::windows_ime_ipc::ImeSubmitRequest {
                        session_id: self.session_id.to_string(),
                        text: text.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        target: crate::windows_ime_target::capture_ime_submit_target(),
                    };
                    let status = match self.windows_ime.submit_prepared(&prepared, request).await {
                        Ok(status) => status,
                        Err(error) if error.is_outcome_unknown() => {
                            log::warn!("[core-adapter] TSF outcome is unknown: {error}");
                            self.windows_ime.restore_session(prepared);
                            return Err(BackendError::new(
                                BackendErrorCode::OutcomeUnknown,
                                error.to_string(),
                            ));
                        }
                        Err(error) => {
                            log::warn!("[core-adapter] TSF submit failed: {error}");
                            crate::types::InsertStatus::Failed
                        }
                    };
                    self.windows_ime.restore_session(prepared);
                    if status == crate::types::InsertStatus::Failed
                        && self.context.insertion.allow_non_tsf_fallback
                    {
                        windows_unicode_fallback(&self.context, &text)
                    } else {
                        status
                    }
                }
                openless_core::shared_types::WindowsInsertionMode::SendInput => {
                    windows_unicode_fallback(&self.context, &text)
                }
                openless_core::shared_types::WindowsInsertionMode::Paste => {
                    crate::insertion::TextInserter::new().insert(
                        &text,
                        self.context.insertion.restore_clipboard_after_paste,
                        self.context.insertion.paste_shortcut,
                    )
                }
            };
            return map_insert_status(status);
        }
        #[cfg(target_os = "android")]
        {
            return map_insert_status(crate::android::android_insert_with_strategy(
                &crate::insertion::TextInserter::new(),
                &text,
                self.context.insertion.android_insert_strategy,
            ));
        }
        #[cfg(not(any(target_os = "windows", target_os = "android")))]
        {
            let restore = self.context.insertion.restore_clipboard_after_paste;
            let shortcut = self.context.insertion.paste_shortcut;
            tauri::async_runtime::spawn_blocking(move || {
                map_insert_status(
                    crate::insertion::TextInserter::new().insert(&text, restore, shortcut),
                )
            })
            .await
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Internal,
                    format!("join Tauri insertion task: {error}"),
                )
            })?
        }
    }

    async fn copy_fallback(&self, text: String) -> Result<InsertOutcome, BackendError> {
        tauri::async_runtime::spawn_blocking(move || {
            map_insert_status(crate::insertion::TextInserter::new().copy_fallback(&text))
        })
        .await
        .map_err(|error| {
            BackendError::new(
                BackendErrorCode::Internal,
                format!("join Tauri clipboard fallback task: {error}"),
            )
        })?
    }

    async fn restore_platform_state(&self) -> Result<(), BackendError> {
        #[cfg(target_os = "windows")]
        if let Some(prepared) = self.prepared.lock().take() {
            self.windows_ime.restore_session(prepared);
        }
        #[cfg(target_os = "macos")]
        {
            let previous_input_source = self.previous_input_source.lock().take();
            crate::unicode_keystroke::restore_input_source(&self.app, previous_input_source)
                .await
                .map_err(|error| {
                    BackendError::new(BackendErrorCode::Platform, error.to_string())
                })?;
        }
        Ok(())
    }
}

impl TextInsertionSession for TauriTextInsertionSession {
    fn supports_streaming(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.streaming_ready
        }
        #[cfg(not(target_os = "macos"))]
        {
            cfg!(any(target_os = "windows", target_os = "linux"))
        }
    }

    fn write(&self, text: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
        if self.finished.load(Ordering::Acquire) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "text insertion session is closed",
                ))
            });
        }
        let session = self.clone();
        Box::pin(async move { session.write_chunk(text).await })
    }

    fn copy(&self, text: String) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self.clone();
        Box::pin(async move { session.copy_fallback(text).await.map(|_| ()) })
    }

    fn finish(
        &self,
        final_text: String,
    ) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let session = self.clone();
        Box::pin(async move {
            if session.finished.swap(true, Ordering::AcqRel) {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "text insertion session is already closed",
                ));
            }
            let result = if final_text.is_empty() {
                Ok(InsertOutcome::Inserted)
            } else {
                session.insert_final(final_text).await
            };
            if let Err(error) = session.restore_platform_state().await {
                // 恢复输入源失败并不能撤销已经落下的文字。保留真实交付结果，
                // 避免历史误报失败后诱导用户重试造成重复；无论插入成败都记录恢复错误。
                log::warn!("[core-adapter] restore input state after insertion failed: {error}");
            }
            result
        })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let session = self.clone();
        Box::pin(async move {
            if session.finished.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            session.restore_platform_state().await
        })
    }
}

#[cfg(target_os = "windows")]
fn windows_unicode_fallback(context: &DictationContext, text: &str) -> crate::types::InsertStatus {
    let inserter = crate::insertion::TextInserter::new();
    let status = inserter.insert_via_unicode_keystrokes(
        text,
        crate::unicode_keystroke::WindowsSendInputOptions {
            newline_mode: context.insertion.windows_sendinput_newline_mode,
        },
    );
    if status == crate::types::InsertStatus::Inserted || !context.insertion.allow_non_tsf_fallback {
        status
    } else {
        inserter.copy_fallback(text)
    }
}

fn map_insert_status(status: crate::types::InsertStatus) -> Result<InsertOutcome, BackendError> {
    match status {
        crate::types::InsertStatus::Inserted => Ok(InsertOutcome::Inserted),
        crate::types::InsertStatus::PasteSent => Ok(InsertOutcome::PasteSent),
        crate::types::InsertStatus::CopiedFallback => Ok(InsertOutcome::CopiedFallback),
        crate::types::InsertStatus::Failed | crate::types::InsertStatus::NotRequested => Err(
            BackendError::new(BackendErrorCode::Platform, "Tauri text insertion failed"),
        ),
    }
}

struct TauriHostContextAdapter;

impl openless_core::HostContextAdapter for TauriHostContextAdapter {
    fn capture(
        &self,
        include_cursor: bool,
    ) -> BoxFuture<'static, Result<openless_core::HostContextCapture, BackendError>> {
        Box::pin(async move {
            let front_app = crate::coordinator::capture_frontmost_app();
            let cursor_context = if include_cursor {
                crate::host_document::read_around_cursor(crate::host_document::DEFAULT_BUDGET_CHARS)
                    .await
                    .map(|window| {
                        let before = window.text.chars().take(window.cursor).collect::<String>();
                        let after = window.text.chars().skip(window.cursor).collect::<String>();
                        openless_core::prompts::cursor_context_input(&before, &after)
                    })
            } else {
                None
            };
            Ok(openless_core::HostContextCapture {
                front_app,
                cursor_context,
            })
        })
    }
}

pub(crate) struct TauriHostActions {
    app: AppHandleSlot,
    qa_context: Arc<crate::qa_adapter::TauriQaHostContext>,
}

impl TauriHostActions {
    pub(crate) fn new(
        app: AppHandleSlot,
        qa_context: Arc<crate::qa_adapter::TauriQaHostContext>,
    ) -> Self {
        Self { app, qa_context }
    }

    fn app(&self) -> Result<AppHandle, BackendError> {
        self.app.lock().clone().ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "Tauri AppHandle is not bound yet",
            )
        })
    }
}

impl HostActions for TauriHostActions {
    fn request(&self, action: HostAction) -> Result<(), BackendError> {
        let app = self.app()?;
        match action {
            HostAction::ShowMain | HostAction::FocusMain => crate::show_main_window(&app),
            HostAction::ShowDictationFeedback | HostAction::HideDictationFeedback => {}
            HostAction::ShowSelectionPreview => crate::show_selection_polish_preview(&app),
            HostAction::HideSelectionPreview => crate::hide_selection_polish_preview(&app),
            HostAction::ShowQa => {
                self.qa_context.prepare_show();
                crate::show_qa_window(&app, "idle");
            }
            HostAction::HideQa => {
                self.qa_context.clear();
                crate::hide_qa_window(&app);
            }
            HostAction::ShowLessComputer => crate::show_less_computer_window(&app),
            HostAction::OpenExternalUrl(url) => {
                use tauri_plugin_shell::ShellExt;
                app.shell().open(url, None).map_err(map_tauri_error)?;
            }
            HostAction::OpenSystemSettings(page) => {
                crate::commands::open_system_settings(page)
                    .map_err(|message| BackendError::new(BackendErrorCode::Platform, message))?;
            }
            HostAction::RequestRestart => {
                crate::prepare_for_restart();
                app.restart();
            }
            HostAction::Notify(message) => {
                app.emit("core:notification", message)
                    .map_err(map_tauri_error)?;
            }
        }
        Ok(())
    }
}

fn map_tauri_error(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn foundry_notices_publish_before_transcription_finishes_and_stop_after_release() {
        use crate::asr::local::foundry_runtime::FoundryFallbackNotice;
        let directory =
            std::env::temp_dir().join(format!("openless-foundry-notice-{}", uuid::Uuid::new_v4()));
        let backend = Arc::new(
            openless_core::OpenLessBackend::new(
                openless_core::BackendConfig {
                    data_dir: directory.clone(),
                    ..Default::default()
                },
                openless_core::BackendDependencies::unsupported(),
            )
            .unwrap(),
        );
        let slot = backend_slot();
        *slot.lock() = Some(Arc::downgrade(&backend));
        let session_id = SessionId::new();
        let released = Arc::new(AtomicBool::new(false));
        let callback = foundry_transcription_notices(slot, session_id, Arc::clone(&released));
        let mut events = backend.subscribe();
        for notice in [
            FoundryFallbackNotice::SwitchingToCpu,
            FoundryFallbackNotice::DownloadingCpu,
        ] {
            callback(notice);
            let event = events
                .try_recv()
                .expect("native notice must reach the UI while transcription is still pending");
            assert_eq!(event.session_id, Some(session_id));
            assert!(matches!(event.kind,
                openless_core::BackendEventKind::Notification(ref payload)
                    if payload.message == notice.message()));
        }
        released.store(true, Ordering::Release);
        callback(FoundryFallbackNotice::DownloadingCpu);
        assert_eq!(
            events.try_recv().unwrap_err(),
            openless_core::EventRecvError::Empty
        );
        drop(backend);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn optional_recording_preparation_failure_keeps_microphone_options_usable() {
        // 这里只替代文件系统/输出静音边界，不启动真实麦克风或修改系统音量。
        // 1.x 的这两项准备都允许失败；准备结果仍必须能交给 Recorder::start。
        let mut options = Vec::new();
        for (archive_fails, mute_fails) in [(true, false), (false, true), (true, true)] {
            let archive_result = if archive_fails {
                Err("recordings directory cannot be created")
            } else {
                Ok(Some(PathBuf::from("fixture.wav")))
            };
            let (archive, mute) = prepare_recording_options(archive_result, true, || {
                if mute_fails {
                    Err("default render endpoint is unavailable".to_string())
                } else {
                    Ok(())
                }
            });
            options.push((archive.is_some(), mute.is_some()));
        }
        assert_eq!(
            options,
            vec![(false, true), (true, false), (false, false)],
            "optional effects must not prevent capture"
        );
    }

    #[test]
    fn optional_recording_preparation_preserves_guard_ownership_and_disabled_behavior() {
        struct MuteGuard(Arc<AtomicU64>);
        impl Drop for MuteGuard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let restored = Arc::new(AtomicU64::new(0));
        let path = PathBuf::from("fixture.wav");
        let (archive, mute) =
            prepare_recording_options(Ok::<_, String>(Some(path.clone())), true, || {
                Ok(MuteGuard(Arc::clone(&restored)))
            });
        assert_eq!(archive, Some(path.clone()));
        assert_eq!(restored.load(Ordering::SeqCst), 0);
        // 成功的guard必须交给录音资源，而不是在准备结束时提前恢复音量。
        drop(mute);
        assert_eq!(restored.load(Ordering::SeqCst), 1);
        let (archive, mute) =
            prepare_recording_options(Ok::<_, String>(None), false, || -> Result<(), String> {
                panic!("disabled muting must not call the platform")
            });
        assert!(archive.is_none());
        assert!(mute.is_none());
        let (archive, mute) = prepare_recording_options(
            Ok::<_, String>(Some(path.clone())),
            false,
            || -> Result<(), String> { panic!("disabled muting must not call the platform") },
        );
        assert_eq!(archive, Some(path));
        assert!(mute.is_none());
    }

    #[test]
    fn native_callback_tasks_run_on_the_tauri_host_runtime() {
        use openless_core::TaskSpawner;

        // The application executor is alive, but cpal callbacks and native
        // destructors run on ordinary OS threads without Tokio's thread-local
        // context. Test that boundary instead of calling spawn inside tokio::test.
        let _host_runtime = tauri::async_runtime::handle();
        let (completed, completion) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            TauriTaskSpawner.spawn(Box::pin(async move {
                // A timer also proves the task has the host's runtime services,
                // not merely a manually polled future on the callback thread.
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                completed.send(()).unwrap();
            }));
        })
        .join()
        .unwrap();
        assert_eq!(
            completion.recv_timeout(std::time::Duration::from_secs(2)),
            Ok(()),
            "native audio/cancellation work must not be dropped outside Tokio threads"
        );
    }

    #[cfg(not(mobile))]
    #[derive(Default)]
    struct TestSelectionPlatformBridge {
        capture_calls: std::sync::atomic::AtomicUsize,
        apply_calls: Mutex<Vec<(String, String, bool)>>,
        revert_calls: Mutex<usize>,
    }

    #[cfg(not(mobile))]
    impl SelectionPlatformBridge for TestSelectionPlatformBridge {
        fn capture(
            &self,
        ) -> Result<
            (
                openless_core::SelectionCapture,
                crate::selection::SelectionInsertionTarget,
            ),
            BackendError,
        > {
            self.capture_calls
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            Ok((
                openless_core::SelectionCapture {
                    text: "source".to_string(),
                    source_app: Some("Editor".to_string()),
                },
                crate::selection::SelectionInsertionTarget::default(),
            ))
        }

        fn apply(
            &self,
            _target: &crate::selection::SelectionInsertionTarget,
            source_text: &str,
            replacement_text: &str,
            reactivate: bool,
        ) -> Result<InsertOutcome, BackendError> {
            self.apply_calls.lock().push((
                source_text.to_string(),
                replacement_text.to_string(),
                reactivate,
            ));
            Ok(InsertOutcome::Inserted)
        }

        fn revert(
            &self,
            _target: &crate::selection::SelectionInsertionTarget,
        ) -> Result<InsertOutcome, BackendError> {
            *self.revert_calls.lock() += 1;
            Ok(InsertOutcome::Inserted)
        }
    }

    struct IgnoreTextStreamSink;

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn windows_preload_requires_the_requested_model_to_be_prepared() {
        use openless_core::{LocalAsrRuntime, LocalAsrTarget, ModelRuntimeAdapter};

        let adapter = TauriLocalAsrRuntimeAdapter::new(
            TauriNativeAsrDependencies::new(
                Arc::new(crate::asr::local::FoundryLocalRuntime::new()),
                Arc::new(crate::asr::local::SherpaOnnxRuntime::new()),
            ),
            Arc::new(openless_core::PreferencesStore::in_memory()),
        );
        // 这是生产 Adapter 的真实未准备状态；preload 独立调用不能虚报成功，
        // 也不能声称整个 Windows runtime Unsupported。测试不下载/加载设备模型。
        for runtime in [LocalAsrRuntime::Foundry, LocalAsrRuntime::SherpaOnnx] {
            let target = LocalAsrTarget::parse(runtime, runtime.default_model()).unwrap();
            let error = adapter
                .preload(target, PathBuf::new(), runtime.provider_id().to_string())
                .await
                .unwrap_err();
            assert_eq!(error.code, BackendErrorCode::InvalidState);
        }
        adapter.invalidate_release(LocalAsrRuntime::Foundry);
        assert_eq!(adapter.native.foundry_generation.load(Ordering::Acquire), 1);
        assert_eq!(adapter.native.sherpa_generation.load(Ordering::Acquire), 0);
        adapter.invalidate_release(LocalAsrRuntime::SherpaOnnx);
        assert_eq!(adapter.native.foundry_generation.load(Ordering::Acquire), 1);
        assert_eq!(adapter.native.sherpa_generation.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn input_source_preparation_skips_one_shot_and_degrades_on_tis_failure() {
        let (previous, streaming) = prepare_streaming_input_source(false, || async {
            panic!("one-shot insertion must not access TIS");
            #[allow(unreachable_code)]
            Ok::<Option<u8>, &str>(Some(1))
        })
        .await;
        assert_eq!(previous, None);
        assert!(!streaming);
        let (previous, streaming) = prepare_streaming_input_source(true, || async {
            Err::<Option<u8>, _>("TIS select failed")
        })
        .await;
        assert_eq!(previous, None);
        assert!(!streaming);
        assert_eq!(
            prepare_streaming_input_source(true, || async { Ok::<_, &str>(Some(7_u8)) }).await,
            (Some(7), true)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn generic_activation_uses_the_requested_target_before_preferences_commit() {
        use openless_core::{LocalAsrRuntime, LocalAsrTarget, ModelRuntimeAdapter};
        let adapter = TauriLocalAsrRuntimeAdapter::new(
            TauriNativeAsrDependencies::new(),
            Arc::new(openless_core::PreferencesStore::in_memory()),
        );
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
        assert_eq!(
            adapter
                .prepare(
                    target.clone(),
                    openless_core::FoundryRuntimeSource::Auto,
                    PathBuf::new(),
                    Arc::new(|_| {})
                )
                .await
                .unwrap(),
            target.model_id()
        );
        // Preferences 仍是旧的云端渠道。若错误读取它会返回虚假的 Ok；本次指定
        // Qwen provider + Whisper target 必须在进入任何 native loader 前明确拒绝。
        let wrong_target =
            LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-large-v3-turbo").unwrap();
        let error = adapter
            .preload(wrong_target, PathBuf::new(), "local-qwen3-c".into())
            .await
            .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::InvalidArgument);
    }

    #[tokio::test]
    async fn native_transcription_timeout_is_retryable_and_drops_pending_wait() {
        let error = await_native_transcription(
            std::time::Duration::from_millis(1),
            std::future::pending::<Result<(), BackendError>>(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert!(error.message.contains("timed out"));
        assert!(error.retryable);
        assert_eq!(
            await_native_transcription(std::time::Duration::from_secs(1), async {
                Ok::<_, BackendError>("transcript")
            })
            .await
            .unwrap(),
            "transcript"
        );
    }

    impl TextStreamSink for IgnoreTextStreamSink {
        fn publish(&self, _chunk: TextStreamChunk) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[test]
    fn insertion_status_mapping_preserves_fallback_and_failure_semantics() {
        assert_eq!(
            map_insert_status(crate::types::InsertStatus::Inserted).unwrap(),
            InsertOutcome::Inserted
        );
        assert_eq!(
            map_insert_status(crate::types::InsertStatus::CopiedFallback).unwrap(),
            InsertOutcome::CopiedFallback
        );
        assert_eq!(
            map_insert_status(crate::types::InsertStatus::PasteSent).unwrap(),
            InsertOutcome::PasteSent
        );
        assert_eq!(
            map_insert_status(crate::types::InsertStatus::Failed)
                .unwrap_err()
                .code,
            BackendErrorCode::Platform
        );
    }

    #[test]
    fn platform_permission_mapping_preserves_every_legacy_state() {
        let cases = [
            (
                crate::permissions::PermissionStatus::Granted,
                openless_core::PermissionState::Granted,
            ),
            (
                crate::permissions::PermissionStatus::Denied,
                openless_core::PermissionState::Denied,
            ),
            (
                crate::permissions::PermissionStatus::NotDetermined,
                openless_core::PermissionState::Unknown,
            ),
            (
                crate::permissions::PermissionStatus::Restricted,
                openless_core::PermissionState::Restricted,
            ),
            (
                crate::permissions::PermissionStatus::NotApplicable,
                openless_core::PermissionState::Unsupported,
            ),
            (
                crate::permissions::PermissionStatus::NoDevice,
                openless_core::PermissionState::NoDevice,
            ),
        ];

        for (legacy, core) in cases {
            assert_eq!(map_permission_state(legacy), core);
        }
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn cancelled_selection_target_rejects_an_unpolled_apply() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap();

        let pending_apply = openless_core::SelectionRuntimeAdapter::apply(
            &runtime,
            session_id,
            "source".to_string(),
            "replacement".to_string(),
        );
        openless_core::SelectionRuntimeAdapter::cancel(&runtime, session_id)
            .await
            .unwrap();

        let error = pending_apply.await.unwrap_err();
        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert!(bridge.apply_calls.lock().is_empty());
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn direct_selection_apply_does_not_reactivate_the_capture_target() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap();

        openless_core::SelectionRuntimeAdapter::apply(
            &runtime,
            session_id,
            "source".to_string(),
            "replacement".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(
            bridge.apply_calls.lock().as_slice(),
            &[("source".to_string(), "replacement".to_string(), false)]
        );
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn a_new_selection_capture_invalidates_the_previous_target() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let previous_session = SessionId::new();
        let current_session = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, previous_session, None)
            .await
            .unwrap();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, current_session, None)
            .await
            .unwrap();

        let error = openless_core::SelectionRuntimeAdapter::apply(
            &runtime,
            previous_session,
            "source".to_string(),
            "stale replacement".to_string(),
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, BackendErrorCode::Cancelled);
        assert!(bridge.apply_calls.lock().is_empty());
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn preview_selection_apply_reactivates_the_capture_target() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap();
        openless_core::SelectionRuntimeAdapter::prepare_preview(&runtime, session_id)
            .await
            .unwrap();

        openless_core::SelectionRuntimeAdapter::apply(
            &runtime,
            session_id,
            "source".to_string(),
            "replacement".to_string(),
        )
        .await
        .unwrap();

        assert_eq!(
            bridge.apply_calls.lock().as_slice(),
            &[("source".to_string(), "replacement".to_string(), true)]
        );
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn cancelled_selection_target_rejects_apply_and_revert() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap();
        openless_core::SelectionRuntimeAdapter::cancel(&runtime, session_id)
            .await
            .unwrap();

        let apply_error = openless_core::SelectionRuntimeAdapter::apply(
            &runtime,
            session_id,
            "source".to_string(),
            "replacement".to_string(),
        )
        .await
        .unwrap_err();
        let revert_error = openless_core::SelectionRuntimeAdapter::revert(&runtime, session_id)
            .await
            .unwrap_err();

        assert_eq!(apply_error.code, BackendErrorCode::Cancelled);
        assert_eq!(revert_error.code, BackendErrorCode::Cancelled);
        assert!(bridge.apply_calls.lock().is_empty());
        assert_eq!(*bridge.revert_calls.lock(), 0);
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn duplicate_selection_capture_does_not_replace_the_original_target() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap();

        let error = openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap_err();
        openless_core::SelectionRuntimeAdapter::apply(
            &runtime,
            session_id,
            "source".to_string(),
            "replacement".to_string(),
        )
        .await
        .expect("the original target should remain active");

        assert_eq!(error.code, BackendErrorCode::Busy);
        assert_eq!(
            bridge
                .capture_calls
                .load(std::sync::atomic::Ordering::Acquire),
            2
        );
        assert_eq!(bridge.apply_calls.lock().len(), 1);
    }

    #[cfg(not(mobile))]
    #[tokio::test]
    async fn selection_revert_is_delegated_once_to_the_platform_bridge() {
        let bridge = Arc::new(TestSelectionPlatformBridge::default());
        let runtime = TauriSelectionRuntime::with_bridge(bridge.clone());
        let session_id = SessionId::new();
        openless_core::SelectionRuntimeAdapter::capture(&runtime, session_id, None)
            .await
            .unwrap();

        let outcome = openless_core::SelectionRuntimeAdapter::revert(&runtime, session_id)
            .await
            .unwrap();

        assert_eq!(outcome, InsertOutcome::Inserted);
        assert_eq!(*bridge.revert_calls.lock(), 1);
    }
}
