use futures_util::future::BoxFuture;
use openless_core::{
    normalize_foundry_language_hint, normalize_sherpa_language_hint, BackendConfig,
    BackendDependencies, BackendError, BackendErrorCode, BackendEventKind, ChannelKind,
    ChannelMutation, ChannelMutationResult, ChannelSummary, CredentialKey, CredentialStore,
    CredentialsStatus, FoundryRuntimeSource, InMemoryCredentialStore, LocalAsrActivationRequest,
    LocalAsrMirror, LocalAsrRuntime, LocalAsrRuntimeLease, LocalAsrRuntimeStatus, LocalAsrSettings,
    LocalAsrTarget, ModelRuntimeAdapter, ModelStore, ModelStoreConfig, NativeModelState,
    OpenLessBackend, PreferencesStore, ProviderSlot, SecretValue,
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
    fail_release: std::sync::atomic::AtomicBool,
    fail_prepare: std::sync::atomic::AtomicBool,
    fail_preload: std::sync::atomic::AtomicBool,
    status: Mutex<Option<LocalAsrRuntimeStatus>>,
    deleted_native: Mutex<Vec<LocalAsrTarget>>,
    restart_on_rebind: std::sync::atomic::AtomicBool,
    emit_prepare_progress: std::sync::atomic::AtomicBool,
    operations: Mutex<Vec<String>>,
    during_prepare: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    loaded_models: Arc<Mutex<std::collections::HashMap<LocalAsrTarget, u64>>>,
}

impl ModelRuntimeAdapter for RecordingLocalAsrRuntime {
    fn engine_available(&self, _: LocalAsrRuntime) -> bool {
        true
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
        Box::pin(async move {
            Ok(targets
                .into_iter()
                .map(|target| NativeModelState {
                    target,
                    installed: true,
                    size_bytes: Some(64 * 1024 * 1024),
                    display_name: Some("Whisper Small Native".into()),
                })
                .collect())
        })
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
        if let Some(update) = self.during_prepare.lock().unwrap().take() {
            update();
        }
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
}

#[derive(Default)]
struct FailingActiveCredentialStore {
    inner: InMemoryCredentialStore,
    fail_set: std::sync::atomic::AtomicBool,
}

impl CredentialStore for FailingActiveCredentialStore {
    fn status(
        &self,
        preferences: openless_core::UserPreferences,
    ) -> BoxFuture<'static, Result<CredentialsStatus, BackendError>> {
        self.inner.status(preferences)
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

            runtime
                .fail_prepare
                .store(true, std::sync::atomic::Ordering::SeqCst);
            assert!(backend.activate_local_asr(request.clone()).await.is_err());
            assert_eq!(
                backend.list_channels(ChannelKind::Asr).await.unwrap(),
                previous
            );
            assert_eq!(
                backend.active_provider(ProviderSlot::Asr).await.unwrap(),
                cloud
            );

            runtime
                .fail_prepare
                .store(false, std::sync::atomic::Ordering::SeqCst);
            let result = backend.activate_local_asr(request).await.unwrap();
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
async fn local_asr_activation_prepares_before_committing_provider_and_preferences() {
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
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        ["prepare:whisper-medium", "preload:whisper-medium"]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_preserves_settings_edited_during_prepare_and_rollback() {
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
        *runtime.during_prepare.lock().unwrap() = Some(Box::new(move || {
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
async fn local_asr_activation_preserves_a_channel_edited_during_preparation() {
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
    *runtime.during_prepare.lock().unwrap() = Some(Box::new(move || {
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

#[tokio::test]
async fn local_asr_activation_rolls_back_runtime_when_preload_fails() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("openai-compatible"),
    );
    backend
        .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
        .await
        .unwrap();
    runtime
        .fail_preload
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let error = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .expect_err("preload failure must abort the transaction");

    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "openai-compatible"
    );
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        "openai-compatible"
    );
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        [
            "prepare:whisper-medium",
            "preload:whisper-medium",
            "release-lease:whisper-medium:1"
        ]
    );
    assert!(!runtime.status.lock().unwrap().as_ref().unwrap().loaded);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_rolls_back_runtime_when_prepare_fails() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("openai-compatible"),
    );
    backend
        .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
        .await
        .unwrap();
    runtime
        .fail_prepare
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let error = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .expect_err("prepare failure must abort the transaction");

    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "openai-compatible"
    );
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        ["prepare:whisper-medium", "release-lease:whisper-medium:1"]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_rolls_back_when_active_provider_commit_fails() {
    let credentials = Arc::new(FailingActiveCredentialStore::default());
    credentials
        .inner
        .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
        .await
        .unwrap();
    let (data_dir, runtime, backend) =
        local_asr_backend_with_credentials(credentials.clone(), Some("openai-compatible"));
    credentials
        .fail_set
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let target = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let error = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target,
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .expect_err("credential commit failure must abort the transaction");

    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(
        backend.get_preferences().active_asr_provider,
        "openai-compatible"
    );
    assert_eq!(
        credentials
            .active_provider(ProviderSlot::Asr)
            .await
            .unwrap(),
        "openai-compatible"
    );
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        [
            "prepare:whisper-medium",
            "preload:whisper-medium",
            "release-lease:whisper-medium:1"
        ]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_releases_the_previous_runtime_lease_before_channel_commit() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("openai-compatible"),
    );
    let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let qwen_dir = data_dir.join("models").join(qwen.model_id());
    std::fs::create_dir_all(&qwen_dir).unwrap();
    std::fs::write(qwen_dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
    let first = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: qwen.clone(),
            provider_id: "local-qwen3".into(),
        })
        .await
        .unwrap();
    let foundry = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let second = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: foundry.clone(),
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .unwrap();

    assert_eq!((first.generation, second.generation), (1, 2));
    assert_eq!(second.target, foundry);
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        [
            "prepare:qwen3-asr-0.6b",
            "preload:qwen3-asr-0.6b",
            "prepare:whisper-medium",
            "preload:whisper-medium",
            "release-lease:qwen3-asr-0.6b:1"
        ]
    );
    assert_eq!(
        runtime
            .status
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .model_id
            .as_deref(),
        Some("whisper-medium")
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn local_asr_activation_switches_qwen_and_whisper_without_releasing_the_new_generic_lease() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("openai-compatible"),
    );
    let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let whisper = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-small").unwrap();
    let qwen_dir = data_dir.join("models").join(qwen.model_id());
    let whisper_dir = data_dir.join("models").join(whisper.model_id());
    std::fs::create_dir_all(&qwen_dir).unwrap();
    std::fs::create_dir_all(&whisper_dir).unwrap();
    std::fs::write(qwen_dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
    std::fs::write(
        whisper_dir.join(openless_core::MODEL_READY_SENTINEL),
        b"ready",
    )
    .unwrap();
    std::fs::write(whisper_dir.join("ggml-small.bin"), b"model").unwrap();

    let first = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: qwen,
            provider_id: "local-qwen3".into(),
        })
        .await
        .unwrap();
    let second = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: whisper.clone(),
            provider_id: "local-whisper".into(),
        })
        .await
        .unwrap();

    assert_eq!((first.generation, second.generation), (1, 2));
    assert_eq!(
        *runtime.loaded_models.lock().unwrap(),
        std::collections::HashMap::from([(whisper.clone(), second.generation)]),
        "Qwen and Whisper use separate caches; only the new lease may remain loaded"
    );
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        [
            "prepare:qwen3-asr-0.6b",
            "preload:qwen3-asr-0.6b",
            "prepare:whisper-small",
            "preload:whisper-small",
            "release-lease:qwen3-asr-0.6b:1"
        ]
    );
    assert_eq!(
        runtime
            .status
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .model_id
            .as_deref(),
        Some(whisper.model_id())
    );
    // Reuse the first model under a new activation generation, then deliver
    // its obsolete lease. A matching model ID alone must not unload it.
    let third = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: first.target.clone(),
            provider_id: "local-qwen3".into(),
        })
        .await
        .unwrap();
    runtime
        .release_lease(LocalAsrRuntimeLease {
            target: first.target.clone(),
            generation: first.generation,
        })
        .await
        .unwrap();
    assert_eq!(
        *runtime.loaded_models.lock().unwrap(),
        std::collections::HashMap::from([(third.target.clone(), third.generation)]),
    );
    assert!(runtime.status.lock().unwrap().as_ref().unwrap().loaded);
    // An ordinary preload also supersedes the activation's ownership.
    runtime
        .preload(third.target.clone(), PathBuf::new(), "local-qwen3".into())
        .await
        .unwrap();
    runtime
        .release_lease(LocalAsrRuntimeLease {
            target: third.target.clone(),
            generation: third.generation,
        })
        .await
        .unwrap();
    assert_eq!(
        runtime.loaded_models.lock().unwrap().get(&third.target),
        Some(&0)
    );
    // The channel settings path may have selected and normally preloaded a
    // different model since the last atomic activation. Retire that current
    // cache, not the stale lease remembered by the model page.
    backend
        .services()
        .local_asr
        .release(LocalAsrRuntime::Generic)
        .await
        .unwrap();
    let mut preferences = backend.get_preferences();
    preferences.active_asr_provider = "local-whisper".into();
    backend
        .update_settings(
            preferences,
            openless_core::SettingsUpdateOptions::SETTINGS_DOCUMENT,
            &openless_core::NoopSettingsRuntime,
        )
        .unwrap();
    backend
        .services()
        .local_asr
        .preload(LocalAsrRuntime::Generic)
        .await
        .unwrap();
    let fourth = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: third.target.clone(),
            provider_id: "local-qwen3".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        *runtime.loaded_models.lock().unwrap(),
        std::collections::HashMap::from([(fourth.target, fourth.generation)]),
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn generic_activation_failure_restores_the_previous_independent_cache_lease() {
    for failure in ["prepare", "preload", "metadata"] {
        let credentials = Arc::new(FailingActiveCredentialStore::default());
        let (data_dir, runtime, backend) =
            local_asr_backend_with_credentials(credentials.clone(), Some("openai-compatible"));
        let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
        let whisper = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "whisper-small").unwrap();
        for target in [&qwen, &whisper] {
            let dir = data_dir.join("models").join(target.model_id());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
            if target == &whisper {
                std::fs::write(dir.join("ggml-small.bin"), b"model").unwrap();
            }
        }
        let first = backend
            .activate_local_asr(LocalAsrActivationRequest {
                target: qwen.clone(),
                provider_id: "local-qwen3".into(),
            })
            .await
            .unwrap();
        match failure {
            "prepare" => &runtime.fail_prepare,
            "preload" => &runtime.fail_preload,
            _ => &credentials.fail_set,
        }
        .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            backend
                .activate_local_asr(LocalAsrActivationRequest {
                    target: whisper,
                    provider_id: "local-whisper".into(),
                })
                .await
                .is_err(),
            "{failure}"
        );
        assert_eq!(
            backend.get_preferences().active_asr_provider,
            "local-qwen3",
            "{failure}"
        );
        assert_eq!(
            backend.active_provider(ProviderSlot::Asr).await.unwrap(),
            first.provider_id
        );
        assert_eq!(
            *runtime.loaded_models.lock().unwrap(),
            std::collections::HashMap::from([(qwen, first.generation)]),
            "{failure}: rollback must restore exactly the previous cache and its owner"
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn native_activation_without_saved_lease_preserves_the_prepared_model() {
    for (kind, provider) in [
        (LocalAsrRuntime::Foundry, "foundry-local-whisper"),
        (LocalAsrRuntime::SherpaOnnx, "sherpa-onnx-local"),
    ] {
        let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
            Arc::new(InMemoryCredentialStore::default()),
            Some(provider),
        );
        let target = LocalAsrTarget::parse(kind, kind.default_model()).unwrap();
        if kind == LocalAsrRuntime::SherpaOnnx {
            let store =
                ModelStore::new(ModelStoreConfig::new(data_dir.join("models")).unwrap()).unwrap();
            let dir = store.model_dir(&target).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
            std::fs::write(dir.join("model.int8.onnx"), b"model").unwrap();
            std::fs::write(dir.join("tokens.txt"), b"tokens").unwrap();
        }
        backend
            .services()
            .local_asr
            .prepare(target.clone())
            .await
            .unwrap();
        runtime.operations.lock().unwrap().clear();
        let request = LocalAsrActivationRequest {
            target: target.clone(),
            provider_id: provider.into(),
        };
        runtime
            .fail_prepare
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(backend.activate_local_asr(request.clone()).await.is_err());
        assert!(runtime.status.lock().unwrap().as_ref().unwrap().loaded);
        backend.activate_local_asr(request).await.unwrap();
        assert!(runtime
            .operations
            .lock()
            .unwrap()
            .iter()
            .all(|operation| !operation.starts_with("release")));
        let status = backend
            .services()
            .local_asr
            .runtime_status(kind)
            .await
            .unwrap();
        assert_eq!(status.model_id.as_deref(), Some(target.model_id()));
        assert!(status.loaded);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}

#[tokio::test]
async fn local_asr_activation_restores_state_when_the_previous_lease_cannot_release() {
    let (data_dir, runtime, backend) = local_asr_backend_with_credentials(
        Arc::new(InMemoryCredentialStore::default()),
        Some("openai-compatible"),
    );
    let qwen = LocalAsrTarget::parse(LocalAsrRuntime::Generic, "qwen3-asr-0.6b").unwrap();
    let qwen_dir = data_dir.join("models").join(qwen.model_id());
    std::fs::create_dir_all(&qwen_dir).unwrap();
    std::fs::write(qwen_dir.join(openless_core::MODEL_READY_SENTINEL), b"ready").unwrap();
    backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: qwen.clone(),
            provider_id: "local-qwen3".into(),
        })
        .await
        .unwrap();
    runtime
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let foundry = LocalAsrTarget::parse(LocalAsrRuntime::Foundry, "whisper-medium").unwrap();

    let error = backend
        .activate_local_asr(LocalAsrActivationRequest {
            target: foundry,
            provider_id: "foundry-local-whisper".into(),
        })
        .await
        .expect_err("old runtime release must be part of the transaction");

    assert_eq!(error.code, BackendErrorCode::Platform);
    assert_eq!(backend.get_preferences().active_asr_provider, "local-qwen3");
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        "local-qwen3"
    );
    assert_eq!(
        runtime
            .status
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .model_id
            .as_deref(),
        Some(qwen.model_id())
    );
    assert_eq!(
        runtime.operations.lock().unwrap().as_slice(),
        [
            "prepare:qwen3-asr-0.6b",
            "preload:qwen3-asr-0.6b",
            "prepare:whisper-medium",
            "preload:whisper-medium",
            "release-lease:qwen3-asr-0.6b:1",
            "release-lease:whisper-medium:2",
            "prepare:qwen3-asr-0.6b",
            "preload:qwen3-asr-0.6b"
        ]
    );
    let _ = std::fs::remove_dir_all(data_dir);
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
    let backend = Arc::new(backend);
    let weak = Arc::downgrade(&backend);
    *runtime.during_prepare.lock().unwrap() = Some(Box::new(move || {
        // The in-memory store completes synchronously. This models the user
        // choosing C while preparation for B is still awaiting native work.
        credentials
            .set_active_provider(ProviderSlot::Asr, "openai-compatible".into())
            .now_or_never()
            .unwrap()
            .unwrap();
        let backend = weak.upgrade().unwrap();
        let mut next = backend.get_preferences();
        next.active_asr_provider = "openai-compatible".into();
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
        "openai-compatible"
    );
    assert_eq!(
        backend.active_provider(ProviderSlot::Asr).await.unwrap(),
        "openai-compatible"
    );
    let _ = std::fs::remove_dir_all(data_dir);
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
