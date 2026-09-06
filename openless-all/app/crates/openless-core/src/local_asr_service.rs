//! Shared Local ASR orchestration.
//!
//! Model/runtime policy and preference transactions live here. `ModelStore`
//! owns downloads and model files; the host Adapter only supplies native engines.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::credentials::{ChannelKind, ChannelMutation, ChannelMutationResult, CredentialStore};
use crate::domains::{
    LocalAsrActivationRequest, LocalAsrActivationResult, LocalAsrApi, LocalAsrModel,
    LocalAsrModelCard, LocalAsrRemoteInfo, LocalAsrRuntimeStatus, LocalAsrSettings,
    LocalAsrStorageSettings, LocalAsrTestResult,
};
use crate::errors::{BackendError, BackendErrorCode};
use crate::events::{BackendEventKind, BackendEventPublisher};
use crate::local_asr_catalog::{
    normalize_foundry_language_hint, normalize_sherpa_language_hint, FoundryRuntimeSource,
    LocalAsrMirror, LocalAsrRuntime, LocalAsrTarget,
};
use crate::types::PreferencesChange;
use crate::{PreferencesStore, UserPreferences};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeModelState {
    pub target: LocalAsrTarget,
    pub installed: bool,
    pub size_bytes: Option<u64>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageRebind {
    Applied,
    RestartRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAsrRuntimeLease {
    pub target: LocalAsrTarget,
    pub generation: u64,
}

pub type ModelPrepareProgressSink =
    Arc<dyn Fn(crate::events::LocalAsrPrepareProgress) + Send + Sync + 'static>;

fn unsupported<T>(operation: &'static str) -> BoxFuture<'static, Result<T, BackendError>> {
    Box::pin(async move {
        Err(BackendError::new(
            BackendErrorCode::Unsupported,
            format!("local ASR runtime does not support {operation}"),
        ))
    })
}

/// Host seam for native Local ASR engines.
///
/// Defaults fail explicitly so a platform can implement only capabilities it
/// genuinely supports without reporting fake success.
pub trait ModelRuntimeAdapter: Send + Sync {
    fn engine_available(&self, _runtime: LocalAsrRuntime) -> bool {
        false
    }

    fn supports_model(&self, target: &LocalAsrTarget) -> bool {
        self.engine_available(target.runtime)
    }

    fn inspect_native_models(
        &self,
        _targets: Vec<LocalAsrTarget>,
    ) -> BoxFuture<'static, Result<Vec<NativeModelState>, BackendError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn native_model_dir(
        &self,
        _target: LocalAsrTarget,
        fallback: PathBuf,
    ) -> BoxFuture<'static, Result<PathBuf, BackendError>> {
        Box::pin(async move { Ok(fallback) })
    }

    fn delete_native_model(
        &self,
        _target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("native model deletion")
    }

    fn rebind_storage(
        &self,
        _models_root: PathBuf,
    ) -> BoxFuture<'static, Result<StorageRebind, BackendError>> {
        Box::pin(async { Ok(StorageRebind::Applied) })
    }

    fn runtime_status(
        &self,
        _settings: LocalAsrSettings,
        _model_dir: PathBuf,
    ) -> BoxFuture<'static, Result<LocalAsrRuntimeStatus, BackendError>> {
        unsupported("runtime status")
    }

    fn prepare(
        &self,
        _target: LocalAsrTarget,
        _runtime_source: FoundryRuntimeSource,
        _model_dir: PathBuf,
        _progress: ModelPrepareProgressSink,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        unsupported("runtime preparation")
    }

    fn cancel_prepare(
        &self,
        _runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("prepare cancellation")
    }

    fn release(&self, _runtime: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("runtime release")
    }

    fn release_lease(
        &self,
        lease: LocalAsrRuntimeLease,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.release(lease.target.runtime)
    }

    /// Adopt an already loaded model before starting an activation transaction.
    /// A later ordinary preload/use must revoke this lease's cleanup authority.
    fn claim_lease(&self, _lease: LocalAsrRuntimeLease) {}

    fn preload(
        &self,
        _target: LocalAsrTarget,
        _model_dir: PathBuf,
        _provider_type: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        unsupported("runtime preload")
    }

    /// Activation-owned preload. Hosts with independent caches retain this
    /// identity so releasing an old lease cannot evict a newer use of the model.
    fn preload_lease(
        &self,
        lease: LocalAsrRuntimeLease,
        model_dir: PathBuf,
        provider_type: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.preload(lease.target, model_dir, provider_type)
    }

    fn test_model(
        &self,
        _target: LocalAsrTarget,
        _model_dir: PathBuf,
    ) -> BoxFuture<'static, Result<LocalAsrTestResult, BackendError>> {
        unsupported("model test")
    }

    fn invalidate_route(&self, _runtime: LocalAsrRuntime) {}
}

pub(crate) struct LocalAsrService {
    preferences: Arc<PreferencesStore>,
    runtime: Arc<dyn ModelRuntimeAdapter>,
    model_store: Arc<crate::model_store::ModelStore>,
    default_models_root: PathBuf,
    events: BackendEventPublisher,
    preferences_revision: Arc<AtomicU64>,
    credentials: Arc<dyn CredentialStore>,
    activation_generation: Arc<AtomicU64>,
    activation_lock: Arc<tokio::sync::Mutex<()>>,
    active_lease: Arc<std::sync::Mutex<Option<LocalAsrRuntimeLease>>>,
}

impl LocalAsrService {
    pub(crate) fn new(
        preferences: Arc<PreferencesStore>,
        runtime: Arc<dyn ModelRuntimeAdapter>,
        model_store: Arc<crate::model_store::ModelStore>,
        default_models_root: PathBuf,
        events: BackendEventPublisher,
        preferences_revision: Arc<AtomicU64>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            preferences,
            runtime,
            model_store,
            default_models_root,
            events,
            preferences_revision,
            credentials,
            activation_generation: Arc::new(AtomicU64::new(0)),
            activation_lock: Arc::new(tokio::sync::Mutex::new(())),
            active_lease: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn publish_preferences(
        &self,
        update: impl FnOnce(&mut UserPreferences),
    ) -> Result<(), BackendError> {
        self.preferences.update(update)?;
        let revision = self.preferences_revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.events.publish(
            None,
            BackendEventKind::PreferencesChanged(PreferencesChange { revision }),
        );
        Ok(())
    }

    fn active_model(preferences: &UserPreferences, runtime: LocalAsrRuntime) -> String {
        match runtime {
            LocalAsrRuntime::Generic => {
                if matches!(
                    preferences.active_asr_provider.as_str(),
                    "local-whisper" | "apple-whisper"
                ) {
                    preferences.local_whisper_active_model.clone()
                } else {
                    preferences.local_asr_active_model.clone()
                }
            }
            LocalAsrRuntime::Foundry => {
                LocalAsrTarget::parse(runtime, preferences.foundry_local_asr_model.clone())
                    .map(|target| target.model_id().to_string())
                    .unwrap_or_else(|_| runtime.default_model().to_string())
            }
            LocalAsrRuntime::SherpaOnnx => {
                LocalAsrTarget::parse(runtime, preferences.sherpa_onnx_model.clone())
                    .map(|target| target.model_id().to_string())
                    .unwrap_or_else(|_| runtime.default_model().to_string())
            }
        }
    }

    fn keep_loaded_secs(preferences: &UserPreferences, runtime: LocalAsrRuntime) -> u32 {
        match runtime {
            LocalAsrRuntime::Generic => preferences.local_asr_keep_loaded_secs,
            LocalAsrRuntime::Foundry => preferences.foundry_local_asr_keep_loaded_secs,
            LocalAsrRuntime::SherpaOnnx => preferences.sherpa_onnx_keep_loaded_secs,
        }
    }

    async fn runtime_status_snapshot(
        preferences: Arc<PreferencesStore>,
        adapter: Arc<dyn ModelRuntimeAdapter>,
        model_store: Arc<crate::model_store::ModelStore>,
        runtime: LocalAsrRuntime,
    ) -> Result<LocalAsrRuntimeStatus, BackendError> {
        let preferences = preferences.get();
        let active_model = Self::active_model(&preferences, runtime);
        let target = LocalAsrTarget::parse(runtime, active_model.clone())?;
        let model_dir = model_store.runtime_model_dir(&target)?;
        adapter
            .runtime_status(
                LocalAsrSettings {
                    runtime,
                    provider_id: runtime.provider_id().to_string(),
                    active_model,
                    mirror: LocalAsrMirror::from_legacy(&preferences.local_asr_mirror),
                    models_base_dir: Self::normalized_base_dir(Some(PathBuf::from(
                        preferences.local_asr_models_base_dir.clone(),
                    )))?,
                    models_root_dir: model_store.models_root_dir(),
                    engine_available: adapter.engine_available(runtime),
                    language_hint: match runtime {
                        LocalAsrRuntime::Generic => None,
                        LocalAsrRuntime::Foundry => {
                            Some(preferences.foundry_local_asr_language_hint.clone())
                        }
                        LocalAsrRuntime::SherpaOnnx => {
                            Some(preferences.sherpa_onnx_language_hint.clone())
                        }
                    },
                    runtime_source: (runtime == LocalAsrRuntime::Foundry).then(|| {
                        FoundryRuntimeSource::from_legacy(&preferences.foundry_local_runtime_source)
                    }),
                    keep_loaded_secs: Self::keep_loaded_secs(&preferences, runtime),
                },
                model_dir,
            )
            .await
    }

    async fn publish_runtime_status(
        preferences: Arc<PreferencesStore>,
        adapter: Arc<dyn ModelRuntimeAdapter>,
        model_store: Arc<crate::model_store::ModelStore>,
        events: BackendEventPublisher,
        runtime: LocalAsrRuntime,
    ) {
        if let Ok(status) =
            Self::runtime_status_snapshot(preferences, adapter, model_store, runtime).await
        {
            events.publish(None, BackendEventKind::LocalAsrEngineChanged(status));
        }
    }

    fn normalized_base_dir(path: Option<PathBuf>) -> Result<Option<PathBuf>, BackendError> {
        match path {
            Some(path) if path.as_os_str().is_empty() => Ok(None),
            Some(path) if !path.is_absolute() => Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "local ASR model base directory must be absolute",
            )),
            path => Ok(path),
        }
    }

    fn storage_settings(&self) -> Result<LocalAsrStorageSettings, BackendError> {
        let base_dir = Self::normalized_base_dir(Some(PathBuf::from(
            self.preferences.get().local_asr_models_base_dir,
        )))?;
        Ok(LocalAsrStorageSettings {
            is_default: base_dir.is_none(),
            models_base_dir: base_dir,
            models_root_dir: self.model_store.models_root_dir(),
            restart_required: false,
        })
    }

    fn prepared_model_dir(&self, target: &LocalAsrTarget) -> Result<PathBuf, BackendError> {
        if !self.model_store.is_native(target)? && !self.model_store.is_installed(target)? {
            return Err(BackendError::new(
                BackendErrorCode::InvalidState,
                "local ASR model is not downloaded",
            ));
        }
        self.model_store.runtime_model_dir(target)
    }

    fn target_for_active_preferences(preferences: &UserPreferences) -> Option<LocalAsrTarget> {
        let provider = preferences.active_asr_provider.as_str();
        let runtime = match provider {
            "local-qwen3" | "local-qwen3-c" | "local-qwen3-mlx" | "local-whisper"
            | "apple-whisper" => LocalAsrRuntime::Generic,
            "foundry-local" | "foundry-whisper" | "foundry-local-whisper" => {
                LocalAsrRuntime::Foundry
            }
            "sherpa-onnx" | "sherpa-onnx-local" => LocalAsrRuntime::SherpaOnnx,
            _ => return None,
        };
        let model = if matches!(provider, "local-whisper" | "apple-whisper") {
            preferences.local_whisper_active_model.clone()
        } else {
            Self::active_model(preferences, runtime)
        };
        LocalAsrTarget::parse(runtime, model).ok()
    }

    fn apply_activation(
        preferences: &mut UserPreferences,
        target: &LocalAsrTarget,
        provider_id: &str,
    ) {
        preferences.active_asr_provider = provider_id.to_string();
        match target.runtime {
            LocalAsrRuntime::Generic => {
                let model = crate::LocalAsrModelId::from_wire_id(target.model_id())
                    .expect("validated local ASR target");
                if model.is_whisper() {
                    preferences.local_whisper_active_model = target.model_id().to_string();
                } else {
                    preferences.local_asr_active_model = target.model_id().to_string();
                }
            }
            LocalAsrRuntime::Foundry => {
                preferences.foundry_local_asr_model = target.model_id().to_string();
            }
            LocalAsrRuntime::SherpaOnnx => {
                preferences.sherpa_onnx_model = target.model_id().to_string();
            }
        }
    }

    fn validate_activation_provider(
        target: &LocalAsrTarget,
        provider_type: &str,
    ) -> Result<(), BackendError> {
        let provider_matches = match target.runtime {
            LocalAsrRuntime::Generic => {
                let model = crate::LocalAsrModelId::from_wire_id(target.model_id())
                    .expect("validated local ASR target");
                if model.is_qwen() {
                    matches!(
                        provider_type,
                        "local-qwen3" | "local-qwen3-c" | "local-qwen3-mlx"
                    )
                } else {
                    matches!(provider_type, "local-whisper" | "apple-whisper")
                }
            }
            LocalAsrRuntime::Foundry => matches!(
                provider_type,
                "foundry-local" | "foundry-whisper" | "foundry-local-whisper"
            ),
            LocalAsrRuntime::SherpaOnnx => {
                matches!(provider_type, "sherpa-onnx" | "sherpa-onnx-local")
            }
        };
        if provider_matches {
            Ok(())
        } else {
            Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "local ASR provider does not match the selected runtime and model",
            ))
        }
    }
}

async fn release_activation_lease(
    adapter: &Arc<dyn ModelRuntimeAdapter>,
    generation_clock: &Arc<AtomicU64>,
    operation_generation: u64,
    lease: LocalAsrRuntimeLease,
) -> Result<(), BackendError> {
    if generation_clock.load(Ordering::Acquire) != operation_generation {
        return Ok(());
    }
    adapter.release_lease(lease).await
}

async fn restore_activation_runtime(
    adapter: &Arc<dyn ModelRuntimeAdapter>,
    model_store: &Arc<crate::model_store::ModelStore>,
    operation_generation: u64,
    requested: &LocalAsrTarget,
    previous: Option<&LocalAsrRuntimeLease>,
    previous_preferences: &UserPreferences,
    progress: ModelPrepareProgressSink,
) -> Vec<BackendError> {
    // Native Windows runtimes already replace their own model during prepare;
    // preserve their existing same-target rollback behavior. Generic cache
    // ownership must be restored even when only the activation owner changed.
    if requested.runtime != LocalAsrRuntime::Generic
        && previous.map(|lease| &lease.target) == Some(requested)
    {
        return Vec::new();
    }
    let mut errors = Vec::new();
    if let Err(error) = adapter
        .release_lease(LocalAsrRuntimeLease {
            target: requested.clone(),
            generation: operation_generation,
        })
        .await
    {
        errors.push(error);
    }
    if let Some(previous) = previous {
        match model_store.runtime_model_dir(&previous.target) {
            Ok(model_dir) => {
                let source = FoundryRuntimeSource::from_legacy(
                    &previous_preferences.foundry_local_runtime_source,
                );
                match adapter
                    .prepare(previous.target.clone(), source, model_dir.clone(), progress)
                    .await
                {
                    Ok(_) => {
                        if let Err(error) = adapter
                            .preload_lease(
                                previous.clone(),
                                model_dir,
                                previous_preferences.active_asr_provider.clone(),
                            )
                            .await
                        {
                            errors.push(error);
                        }
                    }
                    Err(error) => errors.push(error),
                }
            }
            Err(error) => errors.push(error),
        }
    }
    errors
}

fn activation_error(primary: BackendError, rollback: Vec<BackendError>) -> BackendError {
    if rollback.is_empty() {
        return primary;
    }
    BackendError::new(
        BackendErrorCode::Internal,
        format!(
            "{}; local ASR activation rollback failed: {}",
            primary.message,
            rollback
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ")
        ),
    )
}

fn restore_activation_preferences(
    preferences: &Arc<PreferencesStore>,
    previous_preferences: &UserPreferences,
    request: &LocalAsrActivationRequest,
    provider_type: &str,
) -> Vec<BackendError> {
    let mut errors = Vec::new();
    let mut applied = previous_preferences.clone();
    LocalAsrService::apply_activation(&mut applied, &request.target, provider_type);
    if let Err(error) = preferences.update(|current| {
        // Compensation owns only fields changed by this activation. A later
        // user edit (including to the same model field) must survive rollback.
        for (value, before, after) in [
            (
                &mut current.active_asr_provider,
                &previous_preferences.active_asr_provider,
                &applied.active_asr_provider,
            ),
            (
                &mut current.local_asr_active_model,
                &previous_preferences.local_asr_active_model,
                &applied.local_asr_active_model,
            ),
            (
                &mut current.local_whisper_active_model,
                &previous_preferences.local_whisper_active_model,
                &applied.local_whisper_active_model,
            ),
            (
                &mut current.foundry_local_asr_model,
                &previous_preferences.foundry_local_asr_model,
                &applied.foundry_local_asr_model,
            ),
            (
                &mut current.sherpa_onnx_model,
                &previous_preferences.sherpa_onnx_model,
                &applied.sherpa_onnx_model,
            ),
        ] {
            if before != after && value == after {
                *value = before.clone();
            }
        }
    }) {
        errors.push(error);
    }
    errors
}

impl LocalAsrApi for LocalAsrService {
    fn activate(
        &self,
        request: LocalAsrActivationRequest,
    ) -> BoxFuture<'static, Result<LocalAsrActivationResult, BackendError>> {
        if request.provider_id.trim().is_empty() {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "local ASR provider channel id must not be blank",
                ))
            });
        }
        if !self.runtime.supports_model(&request.target) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "local ASR runtime does not support the selected model",
                ))
            });
        }
        let preferences = Arc::clone(&self.preferences);
        let adapter = Arc::clone(&self.runtime);
        let model_store = Arc::clone(&self.model_store);
        let credentials = Arc::clone(&self.credentials);
        let events = self.events.clone();
        let preferences_revision = Arc::clone(&self.preferences_revision);
        let generation_clock = Arc::clone(&self.activation_generation);
        let activation_lock = Arc::clone(&self.activation_lock);
        let active_lease = Arc::clone(&self.active_lease);
        Box::pin(async move {
            let _guard = activation_lock.lock().await;
            let previous_preferences = preferences.get();
            let channels = credentials.list_channels(ChannelKind::Asr).await?;
            // A model page can name a provider before it has a channel. Existing
            // channel IDs remain distinct from provider types, including renamed
            // cards whose old ID now names another protocol.
            let channel = channels
                .iter()
                .find(|channel| {
                    channel.id == request.provider_id
                        && Self::validate_activation_provider(
                            &request.target,
                            &channel.provider_type,
                        )
                        .is_ok()
                })
                .or_else(|| {
                    channels
                        .iter()
                        .find(|channel| channel.provider_type == request.provider_id)
                });
            let channel_id = channel.map(|channel| channel.id.clone());
            let provider_type = channel
                .map(|channel| channel.provider_type.clone())
                .unwrap_or_else(|| request.provider_id.clone());
            Self::validate_activation_provider(&request.target, &provider_type)?;
            let generation = generation_clock.fetch_add(1, Ordering::AcqRel) + 1;
            let previous_target = Self::target_for_active_preferences(&previous_preferences);
            let previous_lease = active_lease
                .lock()
                .expect("local ASR activation lease lock poisoned")
                .clone()
                .filter(|lease| previous_target.as_ref() == Some(&lease.target))
                .or_else(|| {
                    previous_target.map(|target| LocalAsrRuntimeLease {
                        target,
                        generation: generation.saturating_sub(1),
                    })
                });
            if !model_store.is_native(&request.target)?
                && !model_store.is_installed(&request.target)?
            {
                return Err(BackendError::new(
                    BackendErrorCode::InvalidState,
                    "local ASR model is not downloaded",
                ));
            }
            let model_dir = model_store.runtime_model_dir(&request.target)?;
            if let Some(previous) = previous_lease.as_ref() {
                adapter.claim_lease(previous.clone());
            }
            let progress_events = events.clone();
            let progress: ModelPrepareProgressSink = Arc::new(move |progress| {
                progress_events.publish(None, BackendEventKind::LocalAsrPrepareProgress(progress));
            });
            let source = FoundryRuntimeSource::from_legacy(
                &previous_preferences.foundry_local_runtime_source,
            );
            let prepared_model = match adapter
                .prepare(
                    request.target.clone(),
                    source,
                    model_dir.clone(),
                    Arc::clone(&progress),
                )
                .await
            {
                Ok(model) => model,
                Err(error) => {
                    let rollback = restore_activation_runtime(
                        &adapter,
                        &model_store,
                        generation,
                        &request.target,
                        previous_lease.as_ref(),
                        &previous_preferences,
                        progress,
                    )
                    .await;
                    return Err(activation_error(error, rollback));
                }
            };
            if let Err(error) = adapter
                .preload_lease(
                    LocalAsrRuntimeLease {
                        target: request.target.clone(),
                        generation,
                    },
                    model_dir.clone(),
                    provider_type.clone(),
                )
                .await
            {
                let rollback = restore_activation_runtime(
                    &adapter,
                    &model_store,
                    generation,
                    &request.target,
                    previous_lease.as_ref(),
                    &previous_preferences,
                    progress,
                )
                .await;
                return Err(activation_error(error, rollback));
            }

            // Finish every fallible native operation before committing the
            // selected channel. A user may choose another channel while native
            // preparation awaits; rolling back an old channel snapshot would
            // erase that choice. Channel persistence is the final commit point.
            if let Some(previous_lease) = previous_lease.as_ref() {
                // Generic is a catalog group, not a single native cache: macOS
                // Qwen and Whisper have independent engines. Retire the old
                // model lease even when the replacement has the same runtime.
                if previous_lease.target.runtime != request.target.runtime
                    || (request.target.runtime == LocalAsrRuntime::Generic
                        && previous_lease.target != request.target)
                {
                    if let Err(error) = release_activation_lease(
                        &adapter,
                        &generation_clock,
                        generation,
                        previous_lease.clone(),
                    )
                    .await
                    {
                        let rollback = restore_activation_runtime(
                            &adapter,
                            &model_store,
                            generation,
                            &request.target,
                            Some(previous_lease),
                            &previous_preferences,
                            progress,
                        )
                        .await;
                        return Err(activation_error(error, rollback));
                    }
                }
            }

            let committed_previous = match preferences.update(|current| {
                let before = current.clone();
                Self::apply_activation(current, &request.target, &provider_type);
                before
            }) {
                Ok(before) => before,
                Err(error) => {
                    let rollback = restore_activation_runtime(
                        &adapter,
                        &model_store,
                        generation,
                        &request.target,
                        previous_lease.as_ref(),
                        &previous_preferences,
                        progress,
                    )
                    .await;
                    return Err(activation_error(error, rollback));
                }
            };
            let channel_commit = credentials
                .mutate_channel(ChannelMutation::ActivateLocalAsr {
                    id: channel_id,
                    provider_type: provider_type.clone(),
                })
                .await;
            let provider_id = match channel_commit {
                Ok(ChannelMutationResult::Activated(id)) => id,
                result => {
                    let error = result.err().unwrap_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Internal,
                            "credential store returned an invalid local ASR activation result",
                        )
                    });
                    let mut rollback = restore_activation_preferences(
                        &preferences,
                        &committed_previous,
                        &request,
                        &provider_type,
                    );
                    rollback.extend(
                        restore_activation_runtime(
                            &adapter,
                            &model_store,
                            generation,
                            &request.target,
                            previous_lease.as_ref(),
                            &previous_preferences,
                            progress,
                        )
                        .await,
                    );
                    return Err(activation_error(error, rollback));
                }
            };

            *active_lease
                .lock()
                .expect("local ASR activation lease lock poisoned") = Some(LocalAsrRuntimeLease {
                target: request.target.clone(),
                generation,
            });

            let revision = preferences_revision.fetch_add(1, Ordering::SeqCst) + 1;
            events.publish(
                None,
                BackendEventKind::PreferencesChanged(PreferencesChange { revision }),
            );
            adapter.invalidate_route(request.target.runtime);
            Self::publish_runtime_status(
                preferences,
                adapter,
                model_store,
                events,
                request.target.runtime,
            )
            .await;
            Ok(LocalAsrActivationResult {
                target: request.target,
                provider_id,
                generation,
                prepared_model,
            })
        })
    }

    fn settings(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<LocalAsrSettings, BackendError>> {
        let preferences = self.preferences.get();
        let root = self.model_store.models_root_dir();
        let engine_available = self.runtime.engine_available(runtime);
        Box::pin(async move {
            let models_base_dir = Self::normalized_base_dir(Some(PathBuf::from(
                preferences.local_asr_models_base_dir.clone(),
            )))?;
            Ok(LocalAsrSettings {
                runtime,
                provider_id: runtime.provider_id().to_string(),
                active_model: Self::active_model(&preferences, runtime),
                mirror: LocalAsrMirror::from_legacy(&preferences.local_asr_mirror),
                models_base_dir,
                models_root_dir: root,
                engine_available,
                language_hint: match runtime {
                    LocalAsrRuntime::Generic => None,
                    LocalAsrRuntime::Foundry => {
                        Some(preferences.foundry_local_asr_language_hint.clone())
                    }
                    LocalAsrRuntime::SherpaOnnx => {
                        Some(preferences.sherpa_onnx_language_hint.clone())
                    }
                },
                runtime_source: (runtime == LocalAsrRuntime::Foundry).then(|| {
                    FoundryRuntimeSource::from_legacy(&preferences.foundry_local_runtime_source)
                }),
                keep_loaded_secs: Self::keep_loaded_secs(&preferences, runtime),
            })
        })
    }

    fn storage_settings(
        &self,
    ) -> BoxFuture<'static, Result<LocalAsrStorageSettings, BackendError>> {
        let result = self.storage_settings();
        Box::pin(async move { result })
    }

    fn list_models(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<Vec<LocalAsrModel>, BackendError>> {
        let store = Arc::clone(&self.model_store);
        let adapter = Arc::clone(&self.runtime);
        Box::pin(async move {
            let mut models = store.list_models(runtime)?;
            models.retain(|model| adapter.supports_model(&model.target));
            let native = adapter
                .inspect_native_models(
                    models
                        .iter()
                        .filter(|model| store.is_native(&model.target).unwrap_or(false))
                        .map(|model| model.target.clone())
                        .collect(),
                )
                .await?;
            for state in native {
                if let Some(model) = models.iter_mut().find(|model| model.target == state.target) {
                    model.installed = state.installed;
                    model.downloaded_bytes =
                        state.size_bytes.filter(|_| state.installed).unwrap_or(0);
                    model.size_bytes = state.size_bytes;
                    if let Some(display_name) = state.display_name {
                        model.display_name = display_name;
                    }
                }
            }
            Ok(models)
        })
    }

    fn runtime_status(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<LocalAsrRuntimeStatus, BackendError>> {
        let preferences = Arc::clone(&self.preferences);
        let adapter = Arc::clone(&self.runtime);
        let model_store = Arc::clone(&self.model_store);
        Box::pin(Self::runtime_status_snapshot(
            preferences,
            adapter,
            model_store,
            runtime,
        ))
    }

    fn remote_info(
        &self,
        target: LocalAsrTarget,
        mirror: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<LocalAsrRemoteInfo, BackendError>> {
        let mirror = mirror.unwrap_or_else(|| {
            LocalAsrMirror::from_legacy(&self.preferences.get().local_asr_mirror)
        });
        let store = Arc::clone(&self.model_store);
        Box::pin(async move { store.remote_info(target, mirror).await })
    }

    fn model_card(
        &self,
        target: LocalAsrTarget,
        mirror: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<LocalAsrModelCard, BackendError>> {
        let mirror = mirror.unwrap_or_else(|| {
            LocalAsrMirror::from_legacy(&self.preferences.get().local_asr_mirror)
        });
        let store = Arc::clone(&self.model_store);
        Box::pin(async move { store.model_card(target, mirror).await })
    }

    fn set_models_base_dir(
        &self,
        path: Option<PathBuf>,
    ) -> BoxFuture<'static, Result<LocalAsrStorageSettings, BackendError>> {
        let next = match Self::normalized_base_dir(path) {
            Ok(path) => path,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let model_store = Arc::clone(&self.model_store);
        let preferences = Arc::clone(&self.preferences);
        let events = self.events.clone();
        let revision = Arc::clone(&self.preferences_revision);
        let adapter = Arc::clone(&self.runtime);
        let default_models_root = self.default_models_root.clone();
        let activation_lock = Arc::clone(&self.activation_lock);
        Box::pin(async move {
            let _activation_guard = activation_lock.lock().await;
            // A second directory request may be queued behind an in-flight
            // relocation. Resolve the starting root only after owning the lock.
            let current_preferences = preferences.get();
            let current = Self::normalized_base_dir(Some(PathBuf::from(
                current_preferences.local_asr_models_base_dir.clone(),
            )))?;
            if current == next {
                return Ok(LocalAsrStorageSettings {
                    is_default: next.is_none(),
                    models_base_dir: next,
                    models_root_dir: model_store.models_root_dir(),
                    restart_required: false,
                });
            }
            let next_root = next
                .as_ref()
                .map(|path| path.join("OpenLess").join("models"))
                .unwrap_or(default_models_root);
            let previous_root = model_store.models_root_dir();
            model_store.cancel_all_downloads_and_wait().await?;
            for runtime in [
                LocalAsrRuntime::Generic,
                LocalAsrRuntime::Foundry,
                LocalAsrRuntime::SherpaOnnx,
            ] {
                adapter.release(runtime).await?;
            }
            model_store.relocate_root(next_root.clone())?;
            let next_preference = next
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Err(error) = preferences.update(|current| {
                current.local_asr_models_base_dir = next_preference.clone();
            }) {
                if let Err(rollback) = model_store.rollback_relocation(previous_root) {
                    return Err(BackendError::new(
                        BackendErrorCode::Internal,
                        format!(
                            "save model storage preference failed: {}; relocation rollback also failed: {}",
                            error.message, rollback.message
                        ),
                    ));
                }
                return Err(error);
            }
            let rebind = match adapter.rebind_storage(next_root).await {
                Ok(rebind) => rebind,
                Err(error) => {
                    if let Err(rollback) = preferences.update(|current| {
                        if current.local_asr_models_base_dir == next_preference {
                            current.local_asr_models_base_dir =
                                current_preferences.local_asr_models_base_dir.clone();
                        }
                    }) {
                        return Err(BackendError::new(
                            BackendErrorCode::Internal,
                            format!(
                                "rebind model storage failed: {}; preference rollback also failed: {}",
                                error.message, rollback.message
                            ),
                        ));
                    }
                    model_store.rollback_relocation(previous_root.clone())?;
                    adapter.rebind_storage(previous_root).await?;
                    return Err(error);
                }
            };
            if rebind == StorageRebind::Applied {
                model_store.finish_pending_relocation()?;
            }
            let revision = revision.fetch_add(1, Ordering::SeqCst) + 1;
            events.publish(
                None,
                BackendEventKind::PreferencesChanged(PreferencesChange { revision }),
            );
            Ok(LocalAsrStorageSettings {
                is_default: next.is_none(),
                models_base_dir: next,
                models_root_dir: model_store.models_root_dir(),
                restart_required: rebind == StorageRebind::RestartRequired,
            })
        })
    }

    fn set_active_model(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let runtime = target.runtime;
        let result = self.publish_preferences(|preferences| match runtime {
            LocalAsrRuntime::Generic => {
                let model = crate::LocalAsrModelId::from_wire_id(target.model_id())
                    .expect("validated target");
                if model.is_whisper() {
                    preferences.local_whisper_active_model = target.model_id().to_string();
                } else {
                    preferences.local_asr_active_model = target.model_id().to_string();
                }
            }
            LocalAsrRuntime::Foundry => {
                preferences.foundry_local_asr_model = target.model_id().to_string();
            }
            LocalAsrRuntime::SherpaOnnx => {
                preferences.sherpa_onnx_model = target.model_id().to_string();
            }
        });
        if result.is_ok() {
            self.runtime.invalidate_route(runtime);
        }
        let preferences = Arc::clone(&self.preferences);
        let adapter = Arc::clone(&self.runtime);
        let model_store = Arc::clone(&self.model_store);
        let events = self.events.clone();
        Box::pin(async move {
            result?;
            Self::publish_runtime_status(preferences, adapter, model_store, events, runtime).await;
            Ok(())
        })
    }

    fn set_mirror(&self, mirror: LocalAsrMirror) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.publish_preferences(|preferences| {
            preferences.local_asr_mirror = mirror.as_str().to_string();
        });
        Box::pin(async move { result })
    }

    fn set_language_hint(
        &self,
        runtime: LocalAsrRuntime,
        language_hint: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let normalized = match runtime {
            LocalAsrRuntime::Foundry => normalize_foundry_language_hint(&language_hint),
            LocalAsrRuntime::SherpaOnnx => normalize_sherpa_language_hint(&language_hint),
            LocalAsrRuntime::Generic => Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "generic local ASR has no runtime language hint",
            )),
        };
        let normalized = match normalized {
            Ok(value) => value,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let result = self.publish_preferences(|preferences| match runtime {
            LocalAsrRuntime::Foundry => preferences.foundry_local_asr_language_hint = normalized,
            LocalAsrRuntime::SherpaOnnx => preferences.sherpa_onnx_language_hint = normalized,
            LocalAsrRuntime::Generic => unreachable!(),
        });
        Box::pin(async move { result })
    }

    fn set_foundry_runtime_source(
        &self,
        source: FoundryRuntimeSource,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.publish_preferences(|preferences| {
            preferences.foundry_local_runtime_source = source.as_str().to_string();
        });
        if result.is_ok() {
            self.runtime.invalidate_route(LocalAsrRuntime::Foundry);
        }
        let preferences = Arc::clone(&self.preferences);
        let adapter = Arc::clone(&self.runtime);
        let model_store = Arc::clone(&self.model_store);
        let events = self.events.clone();
        Box::pin(async move {
            result?;
            Self::publish_runtime_status(
                preferences,
                adapter,
                model_store,
                events,
                LocalAsrRuntime::Foundry,
            )
            .await;
            Ok(())
        })
    }

    fn set_keep_loaded_secs(
        &self,
        runtime: LocalAsrRuntime,
        seconds: u32,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.publish_preferences(|preferences| match runtime {
            LocalAsrRuntime::Generic => preferences.local_asr_keep_loaded_secs = seconds,
            LocalAsrRuntime::Foundry => preferences.foundry_local_asr_keep_loaded_secs = seconds,
            LocalAsrRuntime::SherpaOnnx => preferences.sherpa_onnx_keep_loaded_secs = seconds,
        });
        let preferences = Arc::clone(&self.preferences);
        let adapter = Arc::clone(&self.runtime);
        let model_store = Arc::clone(&self.model_store);
        let events = self.events.clone();
        Box::pin(async move {
            result?;
            Self::publish_runtime_status(preferences, adapter, model_store, events, runtime).await;
            Ok(())
        })
    }

    fn start_download(
        &self,
        target: LocalAsrTarget,
        mirror: Option<LocalAsrMirror>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let mirror = mirror.unwrap_or_else(|| {
            LocalAsrMirror::from_legacy(&self.preferences.get().local_asr_mirror)
        });
        let store = Arc::clone(&self.model_store);
        Box::pin(async move { store.download_target(target, mirror).await.map(|_| ()) })
    }

    fn cancel_download(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let result = self.model_store.cancel_download(&target).map(|_| ());
        Box::pin(async move { result })
    }

    fn prepare(&self, target: LocalAsrTarget) -> BoxFuture<'static, Result<String, BackendError>> {
        let source =
            FoundryRuntimeSource::from_legacy(&self.preferences.get().foundry_local_runtime_source);
        let runtime = target.runtime;
        let adapter = Arc::clone(&self.runtime);
        let model_dir = match self.prepared_model_dir(&target) {
            Ok(path) => path,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let progress_events = self.events.clone();
        let progress: ModelPrepareProgressSink = Arc::new(move |progress| {
            progress_events.publish(None, BackendEventKind::LocalAsrPrepareProgress(progress));
        });
        let operation = adapter.prepare(target, source, model_dir, progress);
        let preferences = Arc::clone(&self.preferences);
        let model_store = Arc::clone(&self.model_store);
        let events = self.events.clone();
        Box::pin(async move {
            let result = operation.await?;
            Self::publish_runtime_status(preferences, adapter, model_store, events, runtime).await;
            Ok(result)
        })
    }

    fn cancel_prepare(
        &self,
        runtime: LocalAsrRuntime,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.runtime.cancel_prepare(runtime)
    }

    fn release(&self, runtime: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        let adapter = Arc::clone(&self.runtime);
        let operation = adapter.release(runtime);
        let preferences = Arc::clone(&self.preferences);
        let events = self.events.clone();
        let model_store = Arc::clone(&self.model_store);
        Box::pin(async move {
            operation.await?;
            Self::publish_runtime_status(preferences, adapter, model_store, events, runtime).await;
            Ok(())
        })
    }

    fn preload(&self, runtime: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        let preferences = self.preferences.get();
        let target = match LocalAsrTarget::parse(runtime, Self::active_model(&preferences, runtime))
        {
            Ok(target) => target,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let model_dir = match self.prepared_model_dir(&target) {
            Ok(path) => path,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        self.runtime
            .preload(target, model_dir, preferences.active_asr_provider)
    }

    fn delete_model(&self, target: LocalAsrTarget) -> BoxFuture<'static, Result<(), BackendError>> {
        let runtime = target.runtime;
        let adapter = Arc::clone(&self.runtime);
        let store = Arc::clone(&self.model_store);
        let delete_target = target.clone();
        let native = match store.is_native(&target) {
            Ok(native) => native,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let operation = adapter.release(runtime);
        let preferences = Arc::clone(&self.preferences);
        let events = self.events.clone();
        let model_store = Arc::clone(&self.model_store);
        Box::pin(async move {
            operation.await?;
            if native {
                adapter.delete_native_model(delete_target).await?;
            } else {
                store.delete_model(&delete_target)?;
            }
            Self::publish_runtime_status(preferences, adapter, model_store, events, runtime).await;
            Ok(())
        })
    }

    fn model_dir(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<PathBuf, BackendError>> {
        let fallback = match self.model_store.runtime_model_dir(&target) {
            Ok(path) => path,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        self.runtime.native_model_dir(target, fallback)
    }

    fn test_model(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<LocalAsrTestResult, BackendError>> {
        let model_dir = match self.prepared_model_dir(&target) {
            Ok(path) => path,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        self.runtime.test_model(target, model_dir)
    }
}
