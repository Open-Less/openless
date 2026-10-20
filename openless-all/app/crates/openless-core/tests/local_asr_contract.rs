use futures_util::future::BoxFuture;
use openless_core::{
    normalize_foundry_language_hint, normalize_sherpa_language_hint, AsrRoute, BackendConfig,
    BackendDependencies, BackendError, BackendErrorCode, BackendEventKind, ChannelKind,
    ChannelMutation, ChannelMutationResult, ChannelSummary, CredentialKey, CredentialStore,
    CredentialsStatus, FoundryRuntimeSource, InMemoryCredentialStore, LocalAsrActivationRequest,
    LocalAsrMirror, LocalAsrRuntime, LocalAsrRuntimeLease, LocalAsrRuntimeStatus, LocalAsrSettings,
    LocalAsrTarget, LocalAsrTestResult, ModelRuntimeAdapter, ModelStore, ModelStoreConfig,
    NativeModelState, OpenLessBackend, PreferencesStore, ProviderSlot, SecretValue,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[test]
fn public_local_asr_catalog_rejects_unknown_models_per_runtime() {
    let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    assert_eq!(qwen.model_id(), "qwen3-asr-0.6b");

    let foundry = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap();
    assert_eq!(foundry.model_id(), "whisper-small");

    let sherpa =
        LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh").unwrap();
    assert_eq!(sherpa.model_id(), "sense-voice-small-zh");
    assert_eq!(sherpa.sherpa_family().unwrap().as_str(), "sense_voice");
    let streaming = LocalAsrTarget::parse(
        LocalAsrRuntime::SherpaOnnx,
        "zipformer-bilingual-zh-en-streaming",
    )
    .unwrap();
    assert_eq!(
        streaming.sherpa_execution_mode().unwrap().as_str(),
        "online"
    );

    let error = LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "whisper-small")
        .expect_err("a Foundry alias must not leak into the Sherpa catalog");
    assert_eq!(error.code, BackendErrorCode::InvalidArgument);
}

#[test]
fn public_local_asr_preferences_keep_legacy_normalization_semantics() {
    assert_eq!(
        LocalAsrMirror::from_legacy("hf-mirror"),
        LocalAsrMirror::HfMirror
    );
    assert_eq!(
        LocalAsrMirror::from_legacy("modelscope"),
        LocalAsrMirror::Modelscope
    );
    assert_eq!(
        LocalAsrMirror::from_legacy("unexpected"),
        LocalAsrMirror::Huggingface
    );
    assert_eq!(
        FoundryRuntimeSource::from_legacy("ort-nightly"),
        FoundryRuntimeSource::OrtNightly
    );
    assert_eq!(
        FoundryRuntimeSource::from_legacy("unexpected"),
        FoundryRuntimeSource::Auto
    );

    assert_eq!(normalize_foundry_language_hint(" zh ").unwrap(), "zh");
    assert!(normalize_foundry_language_hint("ZH").is_err());
    assert_eq!(
        normalize_sherpa_language_hint(" ZH-hans ").unwrap(),
        "zh-hans"
    );
    assert!(normalize_sherpa_language_hint("zh_CN").is_err());
}

#[derive(Default)]
struct RecordingLocalAsrRuntime {
    invalidated: Mutex<Vec<LocalAsrRuntime>>,
    invalidated_release_schedules: Mutex<Vec<LocalAsrRuntime>>,
    fail_release: std::sync::atomic::AtomicBool,
    fail_prepare: std::sync::atomic::AtomicBool,
    fail_preload: std::sync::atomic::AtomicBool,
    status: Mutex<Option<LocalAsrRuntimeStatus>>,
    deleted_native: Mutex<Vec<LocalAsrTarget>>,
    restart_on_rebind: std::sync::atomic::AtomicBool,
    emit_prepare_progress: std::sync::atomic::AtomicBool,
    operations: Mutex<Vec<String>>,
    during_validation: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    missing_native: std::sync::atomic::AtomicBool,
    restart_needed: std::sync::atomic::AtomicBool,
    loaded_models: Arc<Mutex<std::collections::HashMap<LocalAsrTarget, u64>>>,
    test_transcript: Mutex<String>,
    tested_providers: Mutex<Vec<String>>,
}

impl ModelRuntimeAdapter for RecordingLocalAsrRuntime {
    fn engine_available(&self, _: LocalAsrRuntime) -> bool {
        true
    }

    fn test_model_for_provider(
        &self,
        target: LocalAsrTarget,
        _model_dir: PathBuf,
        provider_type: String,
    ) -> BoxFuture<'static, Result<LocalAsrTestResult, BackendError>> {
        let transcribed_text = self.test_transcript.lock().unwrap().clone();
        self.tested_providers
            .lock()
            .unwrap()
            .push(provider_type.clone());
        Box::pin(async move {
            Ok(LocalAsrTestResult {
                target,
                backend: provider_type,
                expected_text: "Hello. This is a test of the Voxtrail speech-to-text system."
                    .into(),
                transcribed_text,
                audio_ms: 3_000,
                load_ms: 10,
                transcribe_ms: 20,
            })
        })
    }

    fn runtime_status(
        &self,
        settings: LocalAsrSettings,
        _: PathBuf,
    ) -> BoxFuture<'static, Result<LocalAsrRuntimeStatus, BackendError>> {
        let mut status = self
            .status
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(LocalAsrRuntimeStatus {
                restart_required: false,
                runtime: settings.runtime,
                provider_id: settings.provider_id,
                available: true,
                loaded: false,
                active_model: settings.active_model.clone(),
                model_id: None,
                keep_loaded_secs: settings.keep_loaded_secs,
                runtime_source: settings.runtime_source,
                endpoint: None,
                operation: None,
                error: None,
                last_error: None,
                last_prepare_ms: None,
                last_transcribe_ms: None,
                last_audio_ms: None,
            });
        status.active_model = settings.active_model;
        status.keep_loaded_secs = settings.keep_loaded_secs;
        status.runtime_source = settings.runtime_source;
        Box::pin(async move { Ok(status) })
    }

    fn inspect_native_models(
        &self,
        targets: Vec<LocalAsrTarget>,
    ) -> BoxFuture<'static, Result<Vec<NativeModelState>, BackendError>> {
        if let Some(update) = self.during_validation.lock().unwrap().take() {
            update();
        }
        let installed = !self
            .missing_native
            .load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            Ok(targets
                .into_iter()
                .map(|target| NativeModelState {
                    target,
                    installed,
                    size_bytes: Some(64 * 1024 * 1024),
                    display_name: Some("Whisper Small Native".into()),
                })
                .collect())
        })
    }

    fn restart_required(&self, _: LocalAsrRuntime) -> bool {
        self.restart_needed
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    fn download_native_model(
        &self,
        target: LocalAsrTarget,
        _: FoundryRuntimeSource,
        _: openless_core::ModelPrepareProgressSink,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("download:{}", target.model_id()));
        self.restart_needed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.release(target.runtime)
    }

    fn delete_native_model(
        &self,
        target: LocalAsrTarget,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.deleted_native.lock().unwrap().push(target);
        Box::pin(async { Ok(()) })
    }

    fn rebind_storage(
        &self,
        _: PathBuf,
    ) -> BoxFuture<'static, Result<openless_core::StorageRebind, BackendError>> {
        let restart = self
            .restart_on_rebind
            .load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            Ok(if restart {
                openless_core::StorageRebind::RestartRequired
            } else {
                openless_core::StorageRebind::Applied
            })
        })
    }

    fn prepare(
        &self,
        target: LocalAsrTarget,
        _: FoundryRuntimeSource,
        _: PathBuf,
        progress: openless_core::ModelPrepareProgressSink,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("prepare:{}", target.model_id()));
        if self
            .fail_prepare
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture prepare failed",
                ))
            });
        }
        if self
            .emit_prepare_progress
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            progress(openless_core::LocalAsrPrepareProgress {
                runtime: openless_core::LocalAsrRuntimeKind::Foundry,
                phase: openless_core::LocalAsrPreparePhase::Model,
                model_alias: target.model_id().to_string(),
                label: "native model".into(),
                percent: Some(50.0),
                error: None,
            });
        }
        let model_id = target.model_id().to_string();
        *self.status.lock().unwrap() = Some(LocalAsrRuntimeStatus {
            restart_required: false,
            runtime: target.runtime,
            provider_id: target.runtime.provider_id().to_string(),
            available: true,
            loaded: true,
            active_model: model_id.clone(),
            model_id: Some(model_id.clone()),
            keep_loaded_secs: 0,
            runtime_source: None,
            endpoint: None,
            operation: None,
            error: None,
            last_error: None,
            last_prepare_ms: Some(17),
            last_transcribe_ms: None,
            last_audio_ms: None,
        });
        Box::pin(async move { Ok(model_id) })
    }

    fn release(&self, runtime: LocalAsrRuntime) -> BoxFuture<'static, Result<(), BackendError>> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("release:{runtime:?}"));
        if self.fail_release.load(std::sync::atomic::Ordering::SeqCst) {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "native runtime refused to release",
                ))
            });
        }
        if let Some(status) = self.status.lock().unwrap().as_mut() {
            status.runtime = runtime;
            status.loaded = false;
            status.model_id = None;
        }
        self.loaded_models
            .lock()
            .unwrap()
            .retain(|target, _| target.runtime != runtime);
        Box::pin(async { Ok(()) })
    }

    fn release_lease(
        &self,
        lease: LocalAsrRuntimeLease,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.operations.lock().unwrap().push(format!(
            "release-lease:{}:{}",
            lease.target.model_id(),
            lease.generation
        ));
        if self
            .fail_release
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture runtime refused to release the lease",
                ))
            });
        }
        let mut loaded = self.loaded_models.lock().unwrap();
        let current = loaded.get(&lease.target);
        if current.is_none() || current == Some(&lease.generation) {
            loaded.remove(&lease.target);
            if let Some(status) = self.status.lock().unwrap().as_mut() {
                if status.model_id.as_deref() == Some(lease.target.model_id()) {
                    status.loaded = false;
                    status.model_id = None;
                }
            }
        }
        Box::pin(async { Ok(()) })
    }

    fn claim_lease(&self, lease: LocalAsrRuntimeLease) {
        if let Some(generation) = self.loaded_models.lock().unwrap().get_mut(&lease.target) {
            *generation = lease.generation;
        }
    }

    fn preload(
        &self,
        target: LocalAsrTarget,
        _: PathBuf,
        _: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.operations
            .lock()
            .unwrap()
            .push(format!("preload:{}", target.model_id()));
        let failed = self
            .fail_preload
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        let loaded = Arc::clone(&self.loaded_models);
        Box::pin(async move {
            if failed {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture preload failed",
                ))
            } else {
                loaded.lock().unwrap().insert(target, 0);
                Ok(())
            }
        })
    }

    fn preload_lease(
        &self,
        lease: LocalAsrRuntimeLease,
        model_dir: PathBuf,
        provider_type: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let preloaded = self.preload(lease.target.clone(), model_dir, provider_type);
        let loaded = Arc::clone(&self.loaded_models);
        Box::pin(async move {
            preloaded.await?;
            loaded
                .lock()
                .unwrap()
                .insert(lease.target, lease.generation);
            Ok(())
        })
    }

    fn invalidate_route(&self, runtime: LocalAsrRuntime) {
        self.invalidated.lock().unwrap().push(runtime);
    }

    fn invalidate_scheduled_release(&self, runtime: LocalAsrRuntime) {
        self.invalidated_release_schedules
            .lock()
            .unwrap()
            .push(runtime);
    }
}

#[derive(Default)]
struct CredentialStatusPause {
    resume: tokio::sync::Notify,
}

#[derive(Default)]
struct FailingActiveCredentialStore {
    inner: InMemoryCredentialStore,
    fail_set: std::sync::atomic::AtomicBool,
    pause_next_status: Mutex<Option<Arc<CredentialStatusPause>>>,
}

impl CredentialStore for FailingActiveCredentialStore {
    fn status(
        &self,
        preferences: openless_core::UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        let pause = self.pause_next_status.lock().unwrap().take();
        let status = self.inner.status(preferences);
        Box::pin(async move {
            if let Some(pause) = pause {
                pause.resume.notified().await;
            }
            status.await
        })
    }

    fn read(
        &self,
        key: CredentialKey,
    ) -> BoxFuture<'static, Result<Option<SecretValue>, BackendError>> {
        self.inner.read(key)
    }

    fn write(
        &self,
        key: CredentialKey,
        value: SecretValue,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        self.inner.write(key, value)
    }

    fn remove(&self, key: CredentialKey) -> BoxFuture<'static, Result<(), BackendError>> {
        self.inner.remove(key)
    }

    fn list_channels(
        &self,
        kind: ChannelKind,
    ) -> BoxFuture<'static, Result<Vec<ChannelSummary>, BackendError>> {
        self.inner.list_channels(kind)
    }

    fn mutate_channel(
        &self,
        mutation: ChannelMutation,
    ) -> BoxFuture<'static, Result<ChannelMutationResult, BackendError>> {
        if self
            .fail_set
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture active provider save failed",
                ))
            });
        }
        self.inner.mutate_channel(mutation)
    }

    fn active_provider(
        &self,
        slot: ProviderSlot,
    ) -> BoxFuture<'static, Result<String, BackendError>> {
        self.inner.active_provider(slot)
    }

    fn set_active_provider(
        &self,
        slot: ProviderSlot,
        provider_id: String,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        if self
            .fail_set
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Box::pin(async {
                Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "fixture active provider save failed",
                ))
            });
        }
        self.inner.set_active_provider(slot, provider_id)
    }
}

fn local_asr_backend() -> (PathBuf, Arc<RecordingLocalAsrRuntime>, OpenLessBackend) {
    local_asr_backend_with_credentials(Arc::new(InMemoryCredentialStore::default()), None)
}

fn local_asr_backend_with_credentials(
    credentials: Arc<dyn CredentialStore>,
    active_provider: Option<&str>,
) -> (PathBuf, Arc<RecordingLocalAsrRuntime>, OpenLessBackend) {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-local-asr-contract-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    if let Some(active_provider) = active_provider {
        let preferences = PreferencesStore::open(data_dir.join("preferences.json")).unwrap();
        let mut value = preferences.get();
        value.active_asr_provider = active_provider.to_string();
        preferences.set(value).unwrap();
    }
    let runtime = Arc::new(RecordingLocalAsrRuntime::default());
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.local_asr_runtime = Some(runtime.clone());
    dependencies.credential_store = credentials;
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    (data_dir, runtime, backend)
}

#[tokio::test]
async fn channel_test_requires_a_non_blank_transcript() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let model_dir = data_dir.join("models").join(target.model_id());
    std::fs::create_dir_all(&model_dir).unwrap();
    std::fs::write(
        model_dir.join(openless_core::MODEL_READY_SENTINEL),
        b"ready",
    )
    .unwrap();
    backend
        .services()
        .local_asr
        .set_active_model(target)
        .await
        .unwrap();
    let channel_id = backend
        .create_channel(
            ChannelKind::Asr,
            "local-qwen3-c".into(),
            "Local Qwen".into(),
        )
        .await
        .unwrap();

    for transcript in ["", " \n\t"] {
        *runtime.test_transcript.lock().unwrap() = transcript.into();
        let error = backend
            .services()
            .local_asr
            .test_channel(channel_id.clone())
            .await
            .expect_err("blank channel-test transcripts must fail");
        assert_eq!(error.code, BackendErrorCode::Provider);
        assert_eq!(
            error.message,
            "transcription provider returned an empty transcript"
        );
    }

    *runtime.test_transcript.lock().unwrap() = "Hello from the local model".into();
    let result = backend
        .services()
        .local_asr
        .test_channel(channel_id)
        .await
        .unwrap();
    assert_eq!(result.transcribed_text, "Hello from the local model");
    assert_eq!(
        runtime.tested_providers.lock().unwrap().as_slice(),
        ["local-qwen3-c", "local-qwen3-c", "local-qwen3-c"]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn foundry_channel_test_uses_the_foundry_runtime() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap();
    backend
        .services()
        .local_asr
        .set_active_model(target.clone())
        .await
        .unwrap();
    let channel_id = backend
        .create_channel(
            ChannelKind::Asr,
            "foundry-local-whisper".into(),
            "Foundry Local".into(),
        )
        .await
        .unwrap();

    *runtime.test_transcript.lock().unwrap() = "Hello from Foundry".into();
    let result = backend
        .services()
        .local_asr
        .test_channel(channel_id)
        .await
        .unwrap();

    assert_eq!(result.target, target);
    assert_eq!(result.backend, "foundry-local-whisper");
    assert_eq!(result.transcribed_text, "Hello from Foundry");
    assert_eq!(
        runtime.tested_providers.lock().unwrap().as_slice(),
        ["foundry-local-whisper"]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn sherpa_channel_test_uses_the_sherpa_runtime() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let target =
        LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh").unwrap();
    let model_store =
        ModelStore::new(ModelStoreConfig::new(data_dir.join("models")).unwrap()).unwrap();
    let model_dir = model_store.model_dir(&target).unwrap();
    std::fs::create_dir_all(&model_dir).unwrap();
    std::fs::write(
        model_dir.join(openless_core::MODEL_READY_SENTINEL),
        b"ready",
    )
    .unwrap();
    std::fs::write(model_dir.join("model.int8.onnx"), b"model").unwrap();
    std::fs::write(model_dir.join("tokens.txt"), b"tokens").unwrap();

    backend
        .services()
        .local_asr
        .set_active_model(target.clone())
        .await
        .unwrap();
    let channel_id = backend
        .create_channel(
            ChannelKind::Asr,
            "sherpa-onnx-local".into(),
            "Local Sherpa".into(),
        )
        .await
        .unwrap();

    *runtime.test_transcript.lock().unwrap() = "Hello from Sherpa".into();
    let result = backend
        .services()
        .local_asr
        .test_channel(channel_id)
        .await
        .unwrap();

    assert_eq!(result.target, target);
    assert_eq!(result.backend, "sherpa-onnx-local");
    assert_eq!(result.transcribed_text, "Hello from Sherpa");
    assert_eq!(
        runtime.tested_providers.lock().unwrap().as_slice(),
        ["sherpa-onnx-local"]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[cfg(target_os = "windows")]
#[tokio::test]
async fn cloud_route_switches_to_local_when_unconfigured_or_disabled() {
    let (data_dir, _, backend) = local_asr_backend();
    let target =
        LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh").unwrap();
    install_fixture(&data_dir, &target);
    backend
        .services()
        .local_asr
        .set_active_model(target)
        .await
        .unwrap();
    let channel = backend
        .create_channel(ChannelKind::Asr, "whisper".into(), "Cloud ASR".into())
        .await
        .unwrap();
    backend
        .set_active_provider(ProviderSlot::Asr, channel.clone())
        .await
        .unwrap();
    assert_eq!(backend.asr_route().await, AsrRoute::LocalRaw);
    backend
        .set_channel_enabled(ChannelKind::Asr, channel, false)
        .await
        .unwrap();
    assert_eq!(backend.asr_route().await, AsrRoute::LocalRaw);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn disabled_local_channel_does_not_switch_to_a_different_local_model() {
    let (data_dir, _runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("sherpa-onnx-local"),
    );
    let target =
        LocalAsrTarget::parse(LocalAsrRuntime::SherpaOnnx, "sense-voice-small-zh").unwrap();
    install_fixture(&data_dir, &target);
    backend
        .services()
        .local_asr
        .set_active_model(target)
        .await
        .unwrap();
    let local = backend
        .create_channel(
            ChannelKind::Asr,
            "sherpa-onnx-local".into(),
            "Local Sherpa".into(),
        )
        .await
        .unwrap();
    backend
        .set_active_provider(ProviderSlot::Asr, local.clone())
        .await
        .unwrap();
    backend
        .set_channel_enabled(ChannelKind::Asr, local, false)
        .await
        .unwrap();

    assert_eq!(backend.asr_route().await, AsrRoute::Unavailable);
    backend.start().await.unwrap();
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::InvalidState
    );
    backend.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn foundry_route_is_unavailable_when_restart_is_required() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("foundry-local-whisper"),
    );
    backend
        .services()
        .local_asr
        .set_active_model(
            LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap(),
        )
        .await
        .unwrap();
    let foundry = backend
        .create_channel(
            ChannelKind::Asr,
            "foundry-local-whisper".into(),
            "Foundry Local".into(),
        )
        .await
        .unwrap();
    backend
        .set_active_provider(ProviderSlot::Asr, foundry)
        .await
        .unwrap();
    runtime
        .restart_needed
        .store(true, std::sync::atomic::Ordering::SeqCst);

    assert_eq!(backend.asr_route().await, AsrRoute::Unavailable);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_owns_channel_creation_and_enabling() {
    for (runtime_kind, model_id, provider_id) in [
        (LocalAsrRuntime::Generic, "qwen3-asr-0.6b", "local-qwen3-c"),
        (
            LocalAsrRuntime::Foundry,
            "whisper-medium",
            "foundry-local-whisper",
        ),
        (
            LocalAsrRuntime::SherpaOnnx,
            "sense-voice-small-zh",
            "sherpa-onnx-local",
        ),
    ] {
        for (cloud_exists, existing) in [(false, false), (true, false), (true, true)] {
            let (data_dir, runtime, backend) = local_asr_backend();
            let cloud = if cloud_exists {
                backend
                    .create_channel(ChannelKind::Asr, "openai-compatible".into(), "Cloud".into())
                    .await
                    .unwrap()
            } else {
                String::new()
            };
            if existing {
                let id = backend
                    .create_channel(ChannelKind::Asr, provider_id.into(), "Local".into())
                    .await
                    .unwrap();
                backend
                    .set_channel_enabled(ChannelKind::Asr, id, false)
                    .await
                    .unwrap();
            }
            let previous = backend.list_channels(ChannelKind::Asr).await.unwrap();
            let request = LocalAsrActivationRequest {
                target: LocalAsrTarget::parse(runtime_kind, model_id).unwrap(),
                provider_id: provider_id.into(),
            };

            if runtime_kind != LocalAsrRuntime::Foundry {
                assert!(backend.activate_local_asr(request.clone()).await.is_err());
                assert_eq!(
                    backend.list_channels(ChannelKind::Asr).await.unwrap(),
                    previous
                );
                assert_eq!(
                    backend.active_provider(ProviderSlot::Asr).await.unwrap(),
                    cloud
                );
                let store =
                    ModelStore::new(ModelStoreConfig::new(data_dir.join("models")).unwrap())
                        .unwrap();
                let model_dir = store.model_dir(&request.target).unwrap();
                std::fs::create_dir_all(&model_dir).unwrap();
                std::fs::write(
                    model_dir.join(openless_core::MODEL_READY_SENTINEL),
                    b"ready",
                )
                .unwrap();
                if runtime_kind == LocalAsrRuntime::SherpaOnnx {
                    std::fs::write(model_dir.join("model.int8.onnx"), b"model").unwrap();
                    std::fs::write(model_dir.join("tokens.txt"), b"tokens").unwrap();
                }
            }

            // Even an unavailable/broken native loader must not be called just
            // because a downloaded model is enabled.
            runtime
                .fail_prepare
                .store(true, std::sync::atomic::Ordering::SeqCst);
            runtime
                .fail_preload
                .store(true, std::sync::atomic::Ordering::SeqCst);
            runtime.operations.lock().unwrap().clear();
            let result = backend.activate_local_asr(request).await.unwrap();
            assert!(runtime.operations.lock().unwrap().is_empty());
            let channels = backend.list_channels(ChannelKind::Asr).await.unwrap();
            assert_eq!(channels.len(), if cloud_exists { 2 } else { 1 });
            assert_eq!(channels[0].id, result.provider_id);
            assert_eq!(channels[0].provider_type, provider_id);
            assert!(channels[0].enabled);
            assert_eq!(channels[0].name, if existing { "Local" } else { "" });
            if cloud_exists {
                assert_eq!(channels[1].id, cloud);
            }
            assert_eq!(
                backend.active_provider(ProviderSlot::Asr).await.unwrap(),
                result.provider_id
            );
            let _ = std::fs::remove_dir_all(data_dir);
        }
    }
}

#[tokio::test]
async fn local_asr_activation_commits_without_preparing_or_loading() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("openai-compatible"),
    );
    backend
        .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
        .await
        .unwrap();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let result = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: target.clone(),
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .unwrap();

    assert_eq!(result.target, target);
    assert_eq!(result.provider_id, "foundry-local-whisper");
    assert_eq!(result.generation, 1);
    assert_eq!(result.prepared_model, "whisper-medium");
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        result.provider_id
    );
    assert_eq!(
        backend.get_preferences().foundry_local_asr_model,
        "whisper-medium"
    );
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        "foundry-local-whisper"
    );
    assert!(runtime.operations.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_preserves_settings_edited_during_validation_and_rollback() {
    for fail_commit in [false, true] {
        let credentials = Arc::new(FailingActiveCredentialStore::default());
        let (data_dir, runtime, backend) =
            local_asr_backend_with_credentials(credentials.clone(), Some("openai-compatible"));
        let backend = Arc::new(backend);
        backend
            .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
            .await
            .unwrap();
        credentials
            .fail_set
            .store(fail_commit, std::sync::atomic::Ordering::SeqCst);
        let weak = Arc::downgrade(&backend);
        *runtime.during_validation.lock().unwrap() = Some(Box::new(move || {
            // A settings IPC can finish while native preparation is pending.
            // Activation and rollback own the model choice, not this setting.
            let backend = weak.upgrade().unwrap();
            let mut next = backend.get_preferences();
            next.microphone_device_name = "new microphone".into();
            backend
                .update_settings(
                    next,
                    openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                    &openless_core::NoopSettingsRuntime,
                )
                .unwrap();
        }));
        let result = backend
            .activate_local_asr(LocalAsrActivationRequest {
                target: LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap(),
                provider_id: "foundry-local-whisper".into(),
            })
            .await;
        assert_eq!(result.is_err(), fail_commit);
        assert_eq!(
            backend.get_preferences().microphone_device_name,
            "new microphone"
        );
        assert_eq!(
            backend.get_preferences().active_asr_provider,
            if fail_commit {
                "openai-compatible"
            } else {
                "foundry-local-whisper"
            }
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn queued_storage_changes_read_the_current_root_after_acquiring_the_lock() {
    let (data_dir, _, backend) = local_asr_backend();
    let api = &backend.services().local_asr;
    let first = api.set_models_base_dir(Some(data_dir.join("external")));
    let second = api.set_models_base_dir(None);
    first.await.unwrap();
    let result = second.await.unwrap();
    assert!(result.is_default);
    assert_eq!(result.models_root_dir, data_dir.join("models"));
    assert!(backend
        .get_preferences()
        .local_asr_models_base_dir
        .is_empty());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_rejects_a_provider_from_another_runtime() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let error = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "local-qwen3".into(),
        })
        .await
        .expect_err("provider and runtime must be one atomic selection");

    assert_eq!(error.code, BackendErrorCode::InvalidArgument);
    assert!(runtime.operations.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_keeps_channel_id_and_provider_type_distinct() {
    let (data_dir, _, backend) = local_asr_backend();
    let first = backend
        .create_channel(
            ChannelKind::Asr,
            "foundry-local-whisper".into(),
            "Foundry A".into(),
        )
        .await
        .unwrap();
    let second = backend
        .create_channel(
            ChannelKind::Asr,
            "foundry-local-whisper".into(),
            "Foundry B".into(),
        )
        .await
        .unwrap();
    assert_ne!(first, second);
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let result = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: second.clone(),
        })
        .await
        .unwrap();

    assert_eq!(result.provider_id, second);
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        result.provider_id
    );
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "foundry-local-whisper"
    );

    // The model page names the provider type. A reused canonical ID must not
    // route it into another protocol or make it create a duplicate local card.
    backend
        .set_channel_provider_type(ChannelKind::Asr, first, "openai-compatible".into())
        .await
        .unwrap();
    let request = LocalAsrActivationRequest {
        target: LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap(),
        provider_id: "foundry-local-whisper".into(),
    };
    let result = backend.activate_local_asr(request.clone()).await.unwrap();
    assert_eq!(result.provider_id, second);
    backend
        .delete_channel(ChannelKind::Asr, second)
        .await
        .unwrap();
    let result = backend.activate_local_asr(request).await.unwrap();
    let channels = backend.list_channels(ChannelKind::Asr).await.unwrap();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].id, result.provider_id);
    assert_eq!(channels[0].provider_type, "foundry-local-whisper");
    assert_eq!(channels[1].provider_type, "openai-compatible");
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_preserves_a_channel_edited_during_validation() {
    use futures_util::FutureExt;
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let (data_dir, runtime, backend) =
        local_asr_backend_with_credentials(credentials.clone(), Some("openai-compatible"));
    let cloud = backend
        .create_channel(ChannelKind::Asr, "openai-compatible".into(), "Cloud".into())
        .await
        .unwrap();
    let local = backend
        .create_channel(
            ChannelKind::Asr,
            "foundry-local-whisper".into(),
            "Local".into(),
        )
        .await
        .unwrap();
    let edited = local.clone();
    *runtime.during_validation.lock().unwrap() = Some(Box::new(move || {
        credentials
            .mutate_channel(ChannelMutation::SetProviderType {
                kind: ChannelKind::Asr,
                id: edited,
                provider_type: "openai-compatible".into(),
            })
            .now_or_never()
            .unwrap()
            .unwrap();
    }));
    let error = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap(),
            provider_id: local.clone(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, BackendErrorCode::InvalidState);
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        cloud
    );
    let channels = backend.list_channels(ChannelKind::Asr).await.unwrap();
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[1].id, local);
    assert_eq!(channels[1].provider_type, "openai-compatible");
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "openai-compatible"
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

fn install_fixture(data_dir: &std::path::Path, target: &LocalAsrTarget) -> PathBuf {
    let store = ModelStore::new(ModelStoreConfig::new(data_dir.join("models")).unwrap()).unwrap();
    let dir = store.model_dir(target).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
    match (target.runtime, target.model_id()) {
        (LocalAsrRuntime::Generic, "whisper-small") => {
            std::fs::write(dir.join("ggml-small.bin"), b"model").unwrap();
        }
        (LocalAsrRuntime::SherpaOnnx, _) => {
            std::fs::write(dir.join("model.int8.onnx"), b"model").unwrap();
            std::fs::write(dir.join("tokens.txt"), b"tokens").unwrap();
        }
        _ => {}
    }
    dir
}

#[tokio::test]
async fn startup_and_read_only_queries_do_not_download_prepare_or_preload() {
    for provider in [
        "volcengine",
        "local-qwen3-mlx",
        "local-whisper",
        "foundry-local-whisper",
        "sherpa-onnx-local",
    ] {
        for downloaded in [false, true] {
            let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
                Arc::new(InMemoryCredentialStore::default()),
                Some(provider),
            );
            if downloaded {
                install_fixture(
                    &data_dir,
                    &LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap(),
                );
            }
            for kind in [
                LocalAsrRuntime::Generic,
                LocalAsrRuntime::Foundry,
                LocalAsrRuntime::SherpaOnnx,
            ] {
                backend.services().local_asr.settings(kind).await.unwrap();
                backend
                    .services()
                    .local_asr
                    .list_models(kind)
                    .await
                    .unwrap();
                assert!(
                    !backend
                        .services()
                        .local_asr
                        .runtime_status(kind)
                        .await
                        .unwrap()
                        .loaded
                );
            }
            assert!(
                runtime.operations.lock().unwrap().is_empty(),
                "{provider}, downloaded={downloaded}"
            );
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }
}

#[tokio::test]
async fn missing_models_are_rejected_before_any_native_operation() {
    for (kind, model, provider) in [
        (
            LocalAsrRuntime::Generic,
            "qwen3-asr-0.6b",
            "local-qwen3-mlx",
        ),
        (LocalAsrRuntime::Generic, "whisper-small", "local-whisper"),
        (
            LocalAsrRuntime::SherpaOnnx,
            "sense-voice-small-zh",
            "sherpa-onnx-local",
        ),
        (
            LocalAsrRuntime::Foundry,
            "whisper-small",
            "foundry-local-whisper",
        ),
    ] {
        let (data_dir, runtime, backend) = local_asr_backend();
        runtime
            .missing_native
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let target = LocalAsrTarget::parse(kind, model).unwrap();
        let api = &backend.services().local_asr;
        assert!(backend
            .activate_local_asr(LocalAsrActivationRequest {
                target: target.clone(),
                provider_id: provider.into(),
            })
            .await
            .is_err());
        assert!(api.prepare(target).await.is_err());
        assert!(api.preload(kind).await.is_err());
        assert!(runtime.operations.lock().unwrap().is_empty());
        assert!(backend
            .list_channels(ChannelKind::Asr)
            .await
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn activation_commit_failure_does_not_load_or_restore_a_runtime() {
    let credentials = Arc::new(FailingActiveCredentialStore::default());
    let (data_dir, runtime, backend) =
        local_asr_backend_with_credentials(credentials.clone(), Some("openai-compatible"));
    credentials
        .inner
        .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
        .await
        .unwrap();
    credentials
        .fail_set
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap(),
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .is_err());
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "openai-compatible"
    );
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        "openai-compatible"
    );
    assert!(runtime.operations.lock().unwrap().is_empty());
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn failed_activation_restores_the_channel_selected_during_native_preparation() {
    use futures_util::FutureExt;
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let (data_dir, runtime, backend) =
        local_asr_backend_with_credentials(credentials.clone(), Some("local-qwen3"));
    let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let qwen_dir = data_dir.join("models").join(qwen.model_id());
    std::fs::create_dir_all(&qwen_dir).unwrap();
    std::fs::write(qwen_dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
    backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: qwen,
            provider_id: "local-qwen3".into(),
        })
        .await
        .unwrap();
    let ChannelMutationResult::Created(cloud_id) = credentials
        .mutate_channel(ChannelMutation::Create {
            kind: ChannelKind::Asr,
            provider_type: "openai-compatible".into(),
            name: "Cloud channel selected during preparation".into(),
        })
        .await
        .unwrap()
    else {
        panic!("expected cloud channel")
    };
    let expected_cloud_id = cloud_id.clone();
    let backend = Arc::new(backend);
    let weak = Arc::downgrade(&backend);
    *runtime.during_validation.lock().unwrap() = Some(Box::new(move || {
        // The in-memory store completes synchronously. This models the user
        // choosing C while validation for B is still awaiting native work.
        credentials
            .set_active_provider(ProviderSlot::Asr, cloud_id.clone())
            .now_or_never()
            .unwrap()
            .unwrap();
        let backend = weak.upgrade().unwrap();
        let mut next = backend.get_preferences();
        next.active_asr_provider = cloud_id.clone();
        backend
            .update_settings(
                next,
                openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
                &openless_core::NoopSettingsRuntime,
            )
            .unwrap();
    }));
    runtime
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap(),
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .is_err());
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        expected_cloud_id
    );
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        expected_cloud_id
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn switching_models_releases_old_inference_but_does_not_load_the_selection() {
    let (data_dir, runtime, backend) = local_asr_backend();
    for (model, provider) in [
        ("qwen3-asr-0.6b", "local-qwen3-mlx"),
        ("whisper-small", "local-whisper"),
    ] {
        let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, model).unwrap();
        install_fixture(&data_dir, &target);
        backend
            .activate_local_asr(LocalAsrActivationRequest {
                target,
                provider_id: provider.into(),
            })
            .await
            .unwrap();
        assert!(runtime.loaded_models.lock().unwrap().is_empty());
        // Explicit loading remains possible; activation alone never calls it.
        backend
            .services()
            .local_asr
            .preload(LocalAsrRuntime::Generic)
            .await
            .unwrap();
    }
    assert_eq!(
        *runtime.operations.lock().unwrap(),
        [
            "preload:qwen3-asr-0.6b",
            "release-lease:qwen3-asr-0.6b:2",
            "preload:whisper-small",
        ]
    );
    backend
        .set_active_provider(ProviderSlot::Asr, "volcengine".into())
        .await
        .unwrap();
    assert!(runtime.loaded_models.lock().unwrap().is_empty());
    assert_eq!(
        runtime.operations.lock().unwrap().last().unwrap(),
        "release:Generic"
    );
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn channel_reordering_away_from_local_asr_releases_its_runtime() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let cloud = backend
        .create_channel(ChannelKind::Asr, "volcengine".into(), "Cloud".into())
        .await
        .unwrap();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    install_fixture(&data_dir, &target);
    let local = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "local-qwen3-mlx".into(),
        })
        .await
        .unwrap()
        .provider_id;
    backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .unwrap();
    backend
        .reorder_channels(ChannelKind::Asr, vec![cloud.clone(), local])
        .await
        .unwrap();
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        cloud
    );
    assert!(runtime.loaded_models.lock().unwrap().is_empty());
    assert_eq!(
        runtime.operations.lock().unwrap().last().unwrap(),
        "release:Generic"
    );
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn failed_old_runtime_release_preserves_selection_without_loading_any_replacement() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    install_fixture(&data_dir, &qwen);
    backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: qwen,
            provider_id: "local-qwen3-mlx".into(),
        })
        .await
        .unwrap();
    runtime
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap(),
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .is_err());
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "local-qwen3-mlx"
    );
    assert_eq!(
        *runtime.operations.lock().unwrap(),
        ["release-lease:qwen3-asr-0.6b:2"]
    );
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn disabling_releases_but_keeps_files_and_stays_cold_after_restart() {
    let credentials = Arc::new(InMemoryCredentialStore::default());
    let (data_dir, runtime, backend) =
        local_asr_backend_with_credentials(credentials.clone(), None);
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let dir = install_fixture(&data_dir, &target);
    let activation = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "local-qwen3-mlx".into(),
        })
        .await
        .unwrap();
    backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .unwrap();
    backend
        .set_channel_enabled(ChannelKind::Asr, activation.provider_id, false)
        .await
        .unwrap();
    assert!(runtime.loaded_models.lock().unwrap().is_empty());
    assert!(dir.join(openless_core::MODEL_READY_SENTINEL).is_file());
    assert!(backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .is_err());
    drop(backend);

    let fresh_runtime = Arc::new(RecordingLocalAsrRuntime::default());
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.local_asr_runtime = Some(fresh_runtime.clone());
    dependencies.credential_store = credentials;
    let restarted = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    assert!(restarted
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .is_err());
    assert!(fresh_runtime.operations.lock().unwrap().is_empty());
    assert!(!restarted.list_channels(ChannelKind::Asr).await.unwrap()[0].enabled);
    std::fs::remove_dir_all(data_dir).unwrap();
}

async fn loaded_local_channel(
    credentials: Arc<dyn CredentialStore>,
) -> (
    PathBuf,
    Arc<RecordingLocalAsrRuntime>,
    OpenLessBackend,
    String,
) {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(credentials, None);
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    install_fixture(&data_dir, &target);
    let local = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "local-qwen3-mlx".into(),
        })
        .await
        .unwrap()
        .provider_id;
    backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .unwrap();
    runtime.operations.lock().unwrap().clear();
    (data_dir, runtime, backend, local)
}

async fn retire_channel(backend: &OpenLessBackend, id: String, action: &str) {
    match action {
        "disable" => backend
            .set_channel_enabled(ChannelKind::Asr, id, false)
            .await
            .unwrap(),
        "delete" => backend.delete_channel(ChannelKind::Asr, id).await.unwrap(),
        "provider_type" => backend
            .set_channel_provider_type(ChannelKind::Asr, id, "openai-compatible".into())
            .await
            .unwrap(),
        "delete_blank" => assert!(backend
            .delete_channel_if_blank(ChannelKind::Asr, id)
            .await
            .unwrap()),
        _ => panic!("unknown channel retirement fixture"),
    }
}

#[tokio::test]
async fn non_active_same_provider_channel_edits_do_not_release_the_active_runtime() {
    for action in ["disable", "delete", "provider_type", "delete_blank"] {
        let (data_dir, runtime, backend, local) =
            loaded_local_channel(Arc::new(InMemoryCredentialStore::default())).await;
        let spare = backend
            .create_channel(ChannelKind::Asr, "local-qwen3-mlx".into(), String::new())
            .await
            .unwrap();
        retire_channel(&backend, spare, action).await;
        assert_eq!(
            backend.active_provider(ProviderSlot::Asr).await.unwrap(),
            local
        );
        assert!(
            runtime.operations.lock().unwrap().is_empty(),
            "{action} retired another channel's runtime"
        );
        assert!(!runtime.loaded_models.lock().unwrap().is_empty());
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn active_local_channel_edits_release_its_runtime_when_no_local_owner_remains() {
    for action in ["disable", "delete", "provider_type", "delete_blank"] {
        let (data_dir, runtime, backend, local) =
            loaded_local_channel(Arc::new(InMemoryCredentialStore::default())).await;
        backend
            .create_channel(ChannelKind::Asr, "volcengine".into(), "Cloud".into())
            .await
            .unwrap();
        retire_channel(&backend, local, action).await;
        assert_eq!(
            *runtime.operations.lock().unwrap(),
            ["release:Generic"],
            "{action}"
        );
        assert!(runtime.loaded_models.lock().unwrap().is_empty());
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn committed_channel_changes_are_published_even_when_runtime_release_fails() {
    let (data_dir, runtime, backend, local) =
        loaded_local_channel(Arc::new(InMemoryCredentialStore::default())).await;
    let cloud = backend
        .create_channel(ChannelKind::Asr, "volcengine".into(), "Cloud".into())
        .await
        .unwrap();
    let mut events = backend.subscribe();
    runtime
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let error = backend
        .set_channel_enabled(ChannelKind::Asr, local, false)
        .await
        .unwrap_err();
    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        cloud
    );
    assert!(matches!(
        events.try_recv().unwrap().kind,
        BackendEventKind::CredentialsChanged(_)
    ));
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn a_same_runtime_channel_takeover_preserves_the_shared_loaded_instance() {
    let (data_dir, runtime, backend, local) =
        loaded_local_channel(Arc::new(InMemoryCredentialStore::default())).await;
    let replacement = backend
        .create_channel(
            ChannelKind::Asr,
            "local-qwen3-mlx".into(),
            "Replacement".into(),
        )
        .await
        .unwrap();
    backend
        .set_channel_enabled(ChannelKind::Asr, local, false)
        .await
        .unwrap();
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        replacement
    );
    assert!(runtime.operations.lock().unwrap().is_empty());
    assert!(!runtime.loaded_models.lock().unwrap().is_empty());
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn late_credentials_refresh_never_releases_a_reselected_or_reloaded_local_owner() {
    for action in ["disable", "delete", "provider_type", "select", "reorder"] {
        let credentials = Arc::new(FailingActiveCredentialStore::default());
        let (data_dir, runtime, backend, local) = loaded_local_channel(credentials.clone()).await;
        let cloud = backend
            .create_channel(ChannelKind::Asr, "volcengine".into(), "Cloud".into())
            .await
            .unwrap();
        let pause = Arc::new(CredentialStatusPause::default());
        *credentials.pause_next_status.lock().unwrap() = Some(pause.clone());
        let retiring: BoxFuture<'_, ()> = match action {
            "select" => Box::pin(async {
                backend
                    .set_active_provider(ProviderSlot::Asr, cloud.clone())
                    .await
                    .unwrap();
            }),
            "reorder" => Box::pin(async {
                backend
                    .reorder_channels(ChannelKind::Asr, vec![cloud.clone(), local.clone()])
                    .await
                    .unwrap();
            }),
            _ => Box::pin(retire_channel(&backend, local.clone(), action)),
        };
        tokio::pin!(retiring);
        assert!(futures_util::poll!(retiring.as_mut()).is_pending());
        assert_eq!(
            *runtime.operations.lock().unwrap(),
            ["release:Generic"],
            "{action}: release must precede refresh"
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            if action == "provider_type" {
                backend
                    .set_channel_provider_type(
                        ChannelKind::Asr,
                        local.clone(),
                        "local-qwen3-mlx".into(),
                    )
                    .await
                    .unwrap();
            }
            backend
                .activate_local_asr(LocalAsrActivationRequest {
                    target: LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b")
                        .unwrap(),
                    provider_id: "local-qwen3-mlx".into(),
                })
                .await
                .unwrap();
            backend
                .services()
                .local_asr
                .preload(LocalAsrRuntime::Generic)
                .await
                .unwrap();
        })
        .await
        .expect("a delayed status read must not lock out a replacement owner");
        let reloaded = runtime.loaded_models.lock().unwrap().clone();
        assert!(!reloaded.is_empty());
        pause.resume.notify_one();
        retiring.await;
        assert_eq!(
            *runtime.loaded_models.lock().unwrap(),
            reloaded,
            "{action}: stale cleanup unloaded the new instance"
        );
        assert_eq!(
            *runtime.operations.lock().unwrap(),
            ["release:Generic", "preload:qwen3-asr-0.6b"],
            "{action}"
        );
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn preload_cannot_use_a_saved_local_model_while_cloud_is_selected() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    install_fixture(&data_dir, &target);
    backend
        .create_channel(ChannelKind::Asr, "volcengine".into(), "Cloud".into())
        .await
        .unwrap();
    backend
        .services()
        .local_asr
        .set_active_model(target)
        .await
        .unwrap();
    assert!(backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .is_err());
    assert!(runtime.operations.lock().unwrap().is_empty());
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn native_download_does_not_enable_and_requires_restart_before_activation() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let before = backend.get_preferences();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();
    backend
        .services()
        .local_asr
        .start_download(target.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        *runtime.operations.lock().unwrap(),
        ["download:whisper-medium", "release:Foundry"]
    );
    assert!(backend
        .list_channels(ChannelKind::Asr)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        before.active_asr_provider
    );
    let status = backend
        .services()
        .local_asr
        .runtime_status(LocalAsrRuntime::Foundry)
        .await
        .unwrap();
    assert!(status.restart_required);
    assert!(!status.loaded);
    assert!(backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: target.clone(),
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .is_err());
    assert!(backend.services().local_asr.prepare(target).await.is_err());
    std::fs::remove_dir_all(data_dir).unwrap();
}

struct DownloadOnlyTransport;
impl openless_core::ModelTransport for DownloadOnlyTransport {
    fn request(
        &self,
        request: openless_core::ModelTransportRequest,
    ) -> BoxFuture<'static, Result<openless_core::ModelTransportResponse, BackendError>> {
        Box::pin(async move {
            let (status, bytes, content_range) = if request.url.contains("/tree/") {
                (
                    200,
                    br#"[{"type":"file","path":"ggml-small.bin","size":8}]"#.to_vec(),
                    None,
                )
            } else {
                let (start, end) = request.range.unwrap_or((0, 7));
                (
                    206,
                    b"testdata"[start as usize..=end as usize].to_vec(),
                    Some(openless_core::ModelContentRange {
                        start,
                        end,
                        total: 8,
                    }),
                )
            };
            Ok(openless_core::ModelTransportResponse {
                status,
                metadata: openless_core::ModelHttpMetadata {
                    content_length: Some(bytes.len() as u64),
                    content_range,
                    link: None,
                },
                bytes,
            })
        })
    }
}

#[tokio::test]
async fn successful_file_download_never_calls_the_runtime_or_activates_a_channel() {
    let data_dir =
        std::env::temp_dir().join(format!("openless-download-only-{}", uuid::Uuid::new_v4()));
    let runtime = Arc::new(RecordingLocalAsrRuntime::default());
    let store = Arc::new(ModelStore::with_transport(
        ModelStoreConfig::new(data_dir.join("models")).unwrap(),
        Arc::new(DownloadOnlyTransport),
    ));
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.local_asr_runtime = Some(runtime.clone());
    dependencies.credential_store = Arc::new(InMemoryCredentialStore::default());
    dependencies.services.configure_model_store(store.clone());
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-small").unwrap();
    backend
        .services()
        .local_asr
        .start_download(target.clone(), None)
        .await
        .unwrap();
    assert!(store.is_installed(&target).unwrap());
    assert!(runtime.operations.lock().unwrap().is_empty());
    assert!(backend
        .list_channels(ChannelKind::Asr)
        .await
        .unwrap()
        .is_empty());
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[test]
fn backend_startup_migrates_default_and_current_model_roots() {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-core-local-asr-two-roots-{}",
        uuid::Uuid::new_v4()
    ));
    let default_root = data_dir.join("models");
    let custom_base = data_dir.join("custom-volume");
    let current_root = custom_base.join("OpenLess").join("models");
    let default_legacy = default_root.join("qwen3-asr/qwen3-asr-0.6b");
    let current_legacy = current_root.join("qwen3-asr/qwen3-asr-1.7b");
    for legacy in [&default_legacy, &current_legacy] {
        std::fs::create_dir_all(legacy).unwrap();
        std::fs::write(legacy.join("config.json"), b"{}").unwrap();
        std::fs::write(legacy.join(".ready"), b"ready").unwrap();
    }
    let preferences = PreferencesStore::open(data_dir.join("preferences.json")).unwrap();
    let mut value = preferences.get();
    value.local_asr_models_base_dir = custom_base.to_string_lossy().into_owned();
    preferences.set(value).unwrap();

    let model_store =
        Arc::new(ModelStore::new(ModelStoreConfig::new(current_root.clone()).unwrap()).unwrap());
    let runtime = Arc::new(RecordingLocalAsrRuntime::default());
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.local_asr_runtime = Some(runtime);
    dependencies
        .services
        .configure_model_store(Arc::clone(&model_store));

    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();

    assert!(current_root.join("qwen3-asr-0.6b/config.json").is_file());
    assert!(current_root.join("qwen3-asr-1.7b/config.json").is_file());
    assert!(!default_legacy.exists());
    assert!(!current_legacy.exists());
    drop(backend);

    let mut dependencies = BackendDependencies::unsupported();
    dependencies.local_asr_runtime = Some(Arc::new(RecordingLocalAsrRuntime::default()));
    dependencies.services.configure_model_store(Arc::new(
        ModelStore::new(ModelStoreConfig::new(current_root.clone()).unwrap()).unwrap(),
    ));
    let _restarted = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    assert!(current_root.join("qwen3-asr-0.6b/config.json").is_file());
    assert!(current_root.join("qwen3-asr-1.7b/config.json").is_file());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn backend_local_asr_service_owns_preferences_and_change_events() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let mut events = backend.subscribe();
    let foundry = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    backend
        .services()
        .local_asr
        .set_active_model(foundry)
        .await
        .unwrap();
    backend
        .services()
        .local_asr
        .set_language_hint(LocalAsrRuntime::Foundry, " zh ".into())
        .await
        .unwrap();
    backend
        .services()
        .local_asr
        .set_language_hint(LocalAsrRuntime::SherpaOnnx, " ZH-hans ".into())
        .await
        .unwrap();
    backend
        .services()
        .local_asr
        .set_foundry_runtime_source(FoundryRuntimeSource::OrtNightly)
        .await
        .unwrap();
    backend
        .services()
        .local_asr
        .set_keep_loaded_secs(LocalAsrRuntime::Foundry, 42)
        .await
        .unwrap();

    let preferences = backend.get_preferences();
    assert_eq!(preferences.foundry_local_asr_model, "whisper-medium");
    assert_eq!(preferences.foundry_local_asr_language_hint, "zh");
    assert_eq!(preferences.sherpa_onnx_language_hint, "zh-hans");
    assert_eq!(preferences.foundry_local_runtime_source, "ort-nightly");
    assert_eq!(preferences.foundry_local_asr_keep_loaded_secs, 42);
    assert_eq!(preferences.local_asr_keep_loaded_secs, 300);
    assert_eq!(preferences.sherpa_onnx_keep_loaded_secs, 300);
    assert_eq!(
        runtime.invalidated.lock().unwrap().as_slice(),
        [LocalAsrRuntime::Foundry, LocalAsrRuntime::Foundry]
    );

    let event = events.try_recv().expect("preference mutation event");
    assert!(matches!(
        event.kind,
        BackendEventKind::PreferencesChanged(_)
    ));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn foundry_never_release_invalidates_only_the_pending_finite_schedule() {
    let (data_dir, runtime, backend) = local_asr_backend();

    for seconds in [0, 60, 300, 1_800] {
        backend
            .services()
            .local_asr
            .set_keep_loaded_secs(LocalAsrRuntime::Foundry, seconds)
            .await
            .unwrap();
    }
    assert!(runtime
        .invalidated_release_schedules
        .lock()
        .unwrap()
        .is_empty());

    backend
        .services()
        .local_asr
        .set_keep_loaded_secs(
            LocalAsrRuntime::Foundry,
            openless_core::LOCAL_ASR_KEEP_LOADED_FOREVER_SECS,
        )
        .await
        .unwrap();

    let preferences = backend.get_preferences();
    assert_eq!(
        preferences.foundry_local_asr_keep_loaded_secs,
        openless_core::LOCAL_ASR_KEEP_LOADED_FOREVER_SECS
    );
    assert_eq!(preferences.local_asr_keep_loaded_secs, 300);
    assert_eq!(preferences.sherpa_onnx_keep_loaded_secs, 300);
    assert_eq!(
        runtime
            .invalidated_release_schedules
            .lock()
            .unwrap()
            .as_slice(),
        [LocalAsrRuntime::Foundry]
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn backend_local_asr_storage_change_commits_only_after_runtime_quiesces() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let requested = data_dir.join("external-model-volume");
    std::fs::create_dir_all(&requested).unwrap();
    runtime
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let error = backend
        .services()
        .local_asr
        .set_models_base_dir(Some(requested.clone()))
        .await
        .expect_err("a busy runtime must stop the preference commit");
    assert_eq!(error.code, BackendErrorCode::Platform);
    assert!(backend
        .get_preferences()
        .local_asr_models_base_dir
        .is_empty());

    runtime
        .fail_release
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let storage = backend
        .services()
        .local_asr
        .set_models_base_dir(Some(requested.clone()))
        .await
        .unwrap();
    assert_eq!(
        storage.models_base_dir.as_deref(),
        Some(requested.as_path())
    );
    assert_eq!(
        storage.models_root_dir,
        requested.join("OpenLess").join("models")
    );
    assert_eq!(
        backend.get_preferences().local_asr_models_base_dir,
        requested.to_string_lossy()
    );
    let reset = backend
        .services()
        .local_asr
        .set_models_base_dir(None)
        .await
        .unwrap();
    assert!(reset.is_default);
    assert_eq!(reset.models_root_dir, data_dir.join("models"));
    assert!(backend
        .get_preferences()
        .local_asr_models_base_dir
        .is_empty());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn native_model_catalog_and_delete_use_the_runtime_adapter() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap();

    let models = backend
        .services()
        .local_asr
        .list_models(LocalAsrRuntime::Foundry)
        .await
        .unwrap();
    let model = models
        .iter()
        .find(|model| model.target == target)
        .expect("Foundry model remains Core catalog owned");
    assert!(model.installed);
    assert_eq!(model.size_bytes, Some(64 * 1024 * 1024));
    assert_eq!(model.display_name, "Whisper Small Native");

    backend
        .services()
        .local_asr
        .delete_model(target.clone())
        .await
        .unwrap();
    assert_eq!(runtime.deleted_native.lock().unwrap().as_slice(), [target]);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn storage_change_reports_native_restart_requirement() {
    let (data_dir, runtime, backend) = local_asr_backend();
    runtime
        .restart_on_rebind
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let requested = data_dir.join("restart-volume");

    let storage = backend
        .services()
        .local_asr
        .set_models_base_dir(Some(requested))
        .await
        .unwrap();

    assert!(storage.restart_required);
    assert!(storage
        .models_root_dir
        .join(".openless-model-relocation.json")
        .is_file());
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn native_prepare_progress_is_published_by_core() {
    let (data_dir, runtime, backend) = local_asr_backend();
    runtime
        .emit_prepare_progress
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let mut events = backend.subscribe();
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap();

    backend.services().local_asr.prepare(target).await.unwrap();

    let BackendEventKind::LocalAsrPrepareProgress(progress) = events.try_recv().unwrap().kind
    else {
        panic!("native progress must cross the Core event seam");
    };
    assert_eq!(progress.model_alias, "whisper-small");
    assert_eq!(progress.percent, Some(50.0));
    assert!(matches!(
        events.try_recv().unwrap().kind,
        BackendEventKind::LocalAsrEngineChanged(_)
    ));
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn successful_runtime_mutations_publish_the_latest_engine_status() {
    let (data_dir, _, backend) = local_asr_backend();
    let mut events = backend.subscribe();

    backend
        .services()
        .local_asr
        .set_keep_loaded_secs(LocalAsrRuntime::Foundry, 42)
        .await
        .unwrap();
    assert!(matches!(
        events.try_recv().unwrap().kind,
        BackendEventKind::PreferencesChanged(_)
    ));
    let BackendEventKind::LocalAsrEngineChanged(status) = events.try_recv().unwrap().kind else {
        panic!("keep-loaded mutation must publish runtime status");
    };
    assert_eq!(status.runtime, LocalAsrRuntime::Foundry);
    assert_eq!(status.keep_loaded_secs, 42);
    assert!(!status.loaded);

    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-small").unwrap();
    backend.services().local_asr.prepare(target).await.unwrap();
    let BackendEventKind::LocalAsrEngineChanged(status) = events.try_recv().unwrap().kind else {
        panic!("completed prepare must publish runtime status");
    };
    assert!(status.loaded);
    assert_eq!(status.model_id.as_deref(), Some("whisper-small"));

    backend
        .services()
        .local_asr
        .release(LocalAsrRuntime::Foundry)
        .await
        .unwrap();
    let BackendEventKind::LocalAsrEngineChanged(status) = events.try_recv().unwrap().kind else {
        panic!("completed release must publish runtime status");
    };
    assert!(!status.loaded);
    assert_eq!(status.model_id, None);

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn failed_runtime_mutation_does_not_publish_a_success_status() {
    let (data_dir, runtime, backend) = local_asr_backend();
    let mut events = backend.subscribe();
    runtime
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let error = backend
        .services()
        .local_asr
        .release(LocalAsrRuntime::Foundry)
        .await
        .expect_err("release failure must cross the public Interface");
    assert_eq!(error.code, BackendErrorCode::Platform);
    assert!(matches!(
        events.try_recv(),
        Err(openless_core::EventRecvError::Empty)
    ));

    let _ = std::fs::remove_dir_all(data_dir);
}
