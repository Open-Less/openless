use std::sync::Arc;

use futures_util::future::BoxFuture;

use openless_core::{
    AudioRecorder, BackendConfig, BackendDependencies, BackendError, BackendErrorCode,
    BackendRepositories, BackendServices, CredentialStore, DictationEngine, DictationEngineRouter,
    MarketplaceConfig, OpenLessBackend, PipelineDictationEngine, PolishFailurePolicy,
    ProviderService, SettingsRuntime, SharedAuxiliaryTextPolisher, SharedCloudTextPolisher,
    SharedCloudTranscriptionEngine, SharedOmniDictationEngine, TextInserter, TextPolisher,
    TextPolisherRouter, TranscriptionEngine, TranscriptionRouter, SHARED_CLOUD_ASR_PROVIDER_TYPES,
    SHARED_CLOUD_LLM_PROVIDER_TYPES, SHARED_OMNI_PROVIDER_TYPES,
};

use crate::qa::LinuxQaRuntime;
use crate::{
    Fcitx5TextInserter, LinuxCpalRecorder, LinuxCredentialStore, LinuxHostActions,
    LinuxPlatformApi, LinuxSelectionRuntime, LinuxSettingsRuntime,
};

pub struct LinuxBackendRuntime {
    pub backend: Arc<OpenLessBackend>,
    pub host_actions: Arc<LinuxHostActions>,
    pub settings_runtime: Arc<dyn SettingsRuntime>,
}

/// Own the executor handle, not the executor. cpal and native teardown may call
/// Core from plain OS threads; they must enqueue on the already-running host
/// runtime even though those threads have no Tokio thread-local context.
struct LinuxTaskSpawner(tokio::runtime::Handle);

impl LinuxTaskSpawner {
    fn capture_current() -> Result<Self, BackendError> {
        tokio::runtime::Handle::try_current().map(Self).map_err(|_| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "Linux backend construction requires an entered host Tokio runtime or an explicit TaskSpawner",
            )
        })
    }
}

impl openless_core::TaskSpawner for LinuxTaskSpawner {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        self.0.spawn(task);
    }
}

/// Assemble the non-UI Linux runtime from shared provider Interfaces.
///
/// The egui team only supplies a repaint callback and consumes the returned
/// backend/actions. Recorder, insertion, credentials and core pipeline
/// ownership remain outside the UI.
pub struct LinuxBackendBuilder {
    config: BackendConfig,
    transcription: Arc<dyn TranscriptionEngine>,
    polisher: Arc<dyn TextPolisher>,
    auxiliary_polisher: Option<Arc<dyn TextPolisher>>,
    recorder: Option<Arc<dyn AudioRecorder>>,
    text_inserter: Option<Arc<dyn TextInserter>>,
    credential_store: Option<Arc<dyn CredentialStore>>,
    marketplace_config: Option<MarketplaceConfig>,
    services: Option<BackendServices>,
    host_actions: Option<Arc<LinuxHostActions>>,
    settings_runtime: Option<Arc<dyn SettingsRuntime>>,
    polish_failure_policy: PolishFailurePolicy,
    task_spawner: Option<Arc<dyn openless_core::TaskSpawner>>,
}

impl LinuxBackendBuilder {
    /// Assemble the production Linux host with the cloud provider
    /// implementations and credential routing owned by shared core.
    ///
    /// The egui layer supplies only [`BackendConfig`]. It does not select
    /// protocol implementations or read credential accounts. Call inside the
    /// host's Tokio runtime (or a scoped `Handle::enter`) so native callbacks can
    /// retain that executor; this factory never creates a second runtime.
    pub fn from_shared_providers(config: BackendConfig) -> Result<Self, BackendError> {
        // Resolve before touching stores: missing executor is a construction
        // error, not a reason to create credentials/directories then lose work.
        let task_spawner: Arc<dyn openless_core::TaskSpawner> =
            Arc::new(LinuxTaskSpawner::capture_current()?);
        let store = LinuxCredentialStore::open(&config.data_dir)?;
        // Only a host that supplies a home directory opts into reading legacy
        // credentials. Data-only builders and integration tests leave it unset;
        // their isolation must also hold when linked against a production lib.
        if let Some(home_dir) = config.home_dir.as_deref() {
            if let Err(error) = store.migrate_legacy(home_dir) {
                // A locked/unavailable Secret Service is not a reason to block
                // construction: the migration marker remains unset, so unlocking
                // and restarting retries the original sources. Log only the
                // classification, never a provider/keyring message that might
                // contain secret values.
                log::warn!(
                    "Legacy credential migration is incomplete ({:?}); unlock the credential vault and restart to retry. Original credentials were retained.",
                    error.code
                );
            }
        }
        let credential_store: Arc<dyn CredentialStore> = Arc::new(store.clone());

        let transcription = Arc::new(TranscriptionRouter::default());
        let cloud_transcription: Arc<dyn TranscriptionEngine> =
            Arc::new(SharedCloudTranscriptionEngine::with_task_spawner(
                Arc::clone(&credential_store),
                Arc::clone(&task_spawner),
            ));
        for provider_type in SHARED_CLOUD_ASR_PROVIDER_TYPES {
            transcription.register(*provider_type, Arc::clone(&cloud_transcription))?;
        }
        // Linux has no local inference runtime: only shared cloud ASR providers
        // are registered above, and local-ASR requests stay unsupported in Core.
        let polisher = Arc::new(TextPolisherRouter::default());
        let cloud_polisher: Arc<dyn TextPolisher> =
            Arc::new(SharedCloudTextPolisher::new(Arc::clone(&credential_store)));
        for provider_type in SHARED_CLOUD_LLM_PROVIDER_TYPES {
            polisher.register(*provider_type, Arc::clone(&cloud_polisher))?;
        }
        let polisher: Arc<dyn TextPolisher> = polisher;
        let auxiliary_polisher: Arc<dyn TextPolisher> = Arc::new(SharedAuxiliaryTextPolisher::new(
            Arc::clone(&credential_store),
            Arc::clone(&polisher),
        ));

        let mut services = BackendServices::unsupported();
        services.provider = Arc::new(ProviderService::new(
            Arc::clone(&credential_store),
            Arc::clone(&task_spawner),
        ));
        services.configure_coding_agent_process(Arc::new(
            crate::coding_agent::LinuxCodingAgentProcessAdapter,
        ));

        Ok(Self::new(config, transcription, polisher)
            .with_task_spawner(task_spawner)
            .with_auxiliary_polisher(auxiliary_polisher)
            .with_credential_store(credential_store)
            .with_services(services)
            .with_marketplace_config(MarketplaceConfig::production())
            .with_settings_runtime(Arc::new(LinuxSettingsRuntime::new(store))))
    }

    /// Assemble a custom/test host with explicitly supplied provider engines.
    /// Production egui code should use [`Self::from_shared_providers`].
    pub fn new(
        config: BackendConfig,
        transcription: Arc<dyn TranscriptionEngine>,
        polisher: Arc<dyn TextPolisher>,
    ) -> Self {
        Self {
            config,
            transcription,
            polisher,
            auxiliary_polisher: None,
            recorder: None,
            text_inserter: None,
            credential_store: None,
            marketplace_config: None,
            services: None,
            host_actions: None,
            settings_runtime: None,
            polish_failure_policy: PolishFailurePolicy::UseRawText,
            task_spawner: None,
        }
    }

    /// Custom hosts built outside Tokio must supply an executor that accepts
    /// calls from arbitrary native threads and remains alive through shutdown.
    pub fn with_task_spawner(mut self, task_spawner: Arc<dyn openless_core::TaskSpawner>) -> Self {
        self.task_spawner = Some(task_spawner);
        self
    }

    pub fn with_recorder(mut self, recorder: Arc<dyn AudioRecorder>) -> Self {
        self.recorder = Some(recorder);
        self
    }

    pub fn with_auxiliary_polisher(mut self, polisher: Arc<dyn TextPolisher>) -> Self {
        self.auxiliary_polisher = Some(polisher);
        self
    }

    pub fn with_text_inserter(mut self, inserter: Arc<dyn TextInserter>) -> Self {
        self.text_inserter = Some(inserter);
        self
    }

    pub fn with_credential_store(mut self, store: Arc<dyn CredentialStore>) -> Self {
        self.credential_store = Some(store);
        self
    }

    fn with_marketplace_config(mut self, config: MarketplaceConfig) -> Self {
        self.marketplace_config = Some(config);
        self
    }

    pub fn with_services(mut self, services: BackendServices) -> Self {
        self.services = Some(services);
        self
    }

    pub fn with_host_actions(mut self, actions: Arc<LinuxHostActions>) -> Self {
        self.host_actions = Some(actions);
        self
    }

    pub fn with_settings_runtime(mut self, runtime: Arc<dyn SettingsRuntime>) -> Self {
        self.settings_runtime = Some(runtime);
        self
    }

    pub fn with_polish_failure_policy(mut self, policy: PolishFailurePolicy) -> Self {
        self.polish_failure_policy = policy;
        self
    }

    pub fn build(self) -> Result<LinuxBackendRuntime, BackendError> {
        let task_spawner = match self.task_spawner {
            Some(spawner) => spawner,
            None => Arc::new(LinuxTaskSpawner::capture_current()?),
        };
        let repositories = BackendRepositories::open(&self.config.data_dir)?;
        let recordings_dir = self.config.data_dir.join("recordings");
        let recorder = self.recorder.unwrap_or_else(|| {
            Arc::new(LinuxCpalRecorder::with_recordings_dir(None, recordings_dir))
                as Arc<dyn AudioRecorder>
        });
        let text_inserter = self
            .text_inserter
            .unwrap_or_else(|| Arc::new(Fcitx5TextInserter::new(true)) as Arc<dyn TextInserter>);
        let (credential_store, default_settings_runtime): (
            Arc<dyn CredentialStore>,
            Arc<dyn SettingsRuntime>,
        ) = match self.credential_store {
            Some(store) => (store, Arc::new(LinuxSettingsRuntime::hotkeys_only())),
            None => {
                let store = LinuxCredentialStore::open(&self.config.data_dir)?;
                (
                    Arc::new(store.clone()),
                    Arc::new(LinuxSettingsRuntime::new(store)),
                )
            }
        };
        let settings_runtime = self.settings_runtime.unwrap_or(default_settings_runtime);
        let mut services = self.services.unwrap_or_else(BackendServices::unsupported);
        services.platform = Arc::new(LinuxPlatformApi::new(self.config.platform.clone()));
        let host_actions = self
            .host_actions
            .unwrap_or_else(|| Arc::new(LinuxHostActions::default()));
        let selection_polisher = Arc::clone(&self.polisher);
        let auxiliary_polisher = self
            .auxiliary_polisher
            .unwrap_or_else(|| Arc::clone(&self.polisher));
        services.configure_auxiliary_runtime(auxiliary_polisher, Arc::clone(&self.transcription));
        let traditional: Arc<dyn DictationEngine> = Arc::new(
            PipelineDictationEngine::new(Arc::clone(&recorder), self.transcription, self.polisher)
                .with_polish_failure_policy(self.polish_failure_policy),
        );
        let dictation_engine = Arc::new(DictationEngineRouter::new(traditional));
        let omni: Arc<dyn DictationEngine> = Arc::new(SharedOmniDictationEngine::new(
            Arc::clone(&credential_store),
            recorder,
        ));
        for provider_type in SHARED_OMNI_PROVIDER_TYPES {
            dictation_engine.register_omni(*provider_type, Arc::clone(&omni))?;
        }
        let backend_slot = crate::qa::backend_slot();
        let qa_runtime = Arc::new(LinuxQaRuntime::new(
            Arc::clone(&backend_slot),
            Arc::clone(&credential_store),
        ));
        let remote_runtime = Arc::new(crate::remote_input::LinuxRemoteInputRuntime::new(
            Arc::clone(&backend_slot),
            Arc::clone(&credential_store),
            self.config.data_dir.clone(),
        ));
        services.remote_input = Arc::new(openless_core::RemoteInputService::new(
            remote_runtime,
            8443,
            crate::remote_input::remote_input_locale(&self.config.locale),
        )?);
        let backend = Arc::new(OpenLessBackend::new_with_repositories(
            self.config,
            BackendDependencies {
                host_actions: host_actions.clone(),
                text_inserter,
                dictation_engine,
                task_spawner,
                credential_store,
                services,
                // Linux ships no local inference runtime. Leaving this unset
                // makes every local-ASR call resolve to Core's unsupported
                // adapter instead of fabricating a local capability.
                local_asr_runtime: None,
                marketplace_config: self.marketplace_config,
                selection_runtime: Some(Arc::new(LinuxSelectionRuntime::new())),
                selection_polisher: Some(selection_polisher),
                qa_runtime: Some(qa_runtime),
            },
            repositories,
        )?);
        crate::qa::bind_backend(&backend_slot, &backend);
        Ok(LinuxBackendRuntime {
            backend,
            host_actions,
            settings_runtime,
        })
    }
}

#[cfg(test)]
mod tests {
    use openless_core::testing::{
        FixtureAudioRecorder, FixtureTextInserter, FixtureTextPolisher, FixtureTranscriptionEngine,
    };
    use openless_core::{
        BackendErrorCode, CodingAgentProvider, CodingAgentTestRequest, InMemoryCredentialStore,
        InsertOutcome, ProviderKind, ProviderRequest,
    };

    use super::*;

    #[test]
    fn builder_requires_an_executor_before_opening_stores() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-executor-required-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let config = BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        };
        let error = LinuxBackendBuilder::from_shared_providers(config.clone())
            .err()
            .expect("production construction outside the runtime must fail explicitly");
        assert_eq!(error.code, BackendErrorCode::InvalidState);
        let custom = || {
            LinuxBackendBuilder::new(
                config.clone(),
                Arc::new(FixtureTranscriptionEngine::successful("unused", 0)),
                Arc::new(FixtureTextPolisher::successful("unused")),
            )
        };
        let error = custom()
            .build()
            .err()
            .expect("custom default construction must not silently choose a thread-local spawner");
        assert_eq!(error.code, BackendErrorCode::InvalidState);
        assert!(
            !data_dir.exists(),
            "executor errors precede persistence effects"
        );

        // A host may construct synchronously if it explicitly supplies its own
        // existing runtime handle. Keeping the Runtime here models host lifetime;
        // the adapter itself never creates or owns an executor.
        let executor = tokio::runtime::Runtime::new().unwrap();
        let backend = custom()
            .with_task_spawner(Arc::new(LinuxTaskSpawner(executor.handle().clone())))
            .build()
            .unwrap();
        assert!(!backend.backend.snapshot().running);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn native_callback_tasks_use_the_production_builder_runtime() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-native-task-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let builder = LinuxBackendBuilder::from_shared_providers(BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        })
        .unwrap();
        let spawner = Arc::clone(builder.task_spawner.as_ref().unwrap());
        let (completed, completion) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            spawner.spawn(Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                let _ = completed.send(());
            }));
        })
        .join()
        .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), completion).await;
        let _ = std::fs::remove_dir_all(data_dir);
        assert_eq!(
            result,
            Ok(Ok(())),
            "cpal/native cleanup callbacks must reach the existing host executor"
        );
    }

    #[tokio::test]
    async fn shared_provider_builder_requires_no_ui_or_provider_factory() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-shared-provider-builder-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let runtime = LinuxBackendBuilder::from_shared_providers(BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        })
        .unwrap()
        .with_recorder(Arc::new(FixtureAudioRecorder::new(Vec::new(), Vec::new())))
        .with_text_inserter(Arc::new(FixtureTextInserter::with_outcome(
            InsertOutcome::Inserted,
        )))
        .build()
        .unwrap();

        assert!(!runtime.backend.snapshot().running);
        assert!(!data_dir.join("credential-metadata.json").exists());
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn shared_provider_builder_registers_the_core_marketplace_service() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-marketplace-builder-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let builder = LinuxBackendBuilder::from_shared_providers(BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        })
        .unwrap();

        assert!(builder.marketplace_config.is_some());
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn shared_provider_factory_does_not_fall_back_to_unsupported() -> Result<(), BackendError>
    {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-provider-factory-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let runtime = LinuxBackendBuilder::from_shared_providers(BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        })?
        .build()?;

        let error = runtime
            .backend
            .services()
            .provider
            .list_models(ProviderRequest {
                kind: ProviderKind::Llm,
                thinking_enabled: false,
                channel_id: None,
            })
            .await
            .expect_err("an unconfigured provider should fail explicitly");
        assert_ne!(error.code, BackendErrorCode::Unsupported);
        let _ = std::fs::remove_dir_all(data_dir);
        Ok(())
    }

    #[tokio::test]
    async fn builder_runs_the_shared_pipeline_without_egui_or_tauri() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-builder-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let recorder = FixtureAudioRecorder::new(vec![vec![1, 0, 2, 0]], vec![(20, 0.5)]);
        let transcription = FixtureTranscriptionEngine::successful("fixture raw", 20);
        let runtime = LinuxBackendBuilder::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            Arc::new(transcription.clone()),
            Arc::new(FixtureTextPolisher::successful("fixture polished")),
        )
        .with_recorder(Arc::new(recorder.clone()))
        .with_text_inserter(Arc::new(FixtureTextInserter::with_outcome(
            InsertOutcome::Inserted,
        )))
        .with_credential_store(Arc::new(InMemoryCredentialStore::default()))
        .build()
        .unwrap();

        runtime.backend.start().await.unwrap();
        runtime.backend.start_dictation().await.unwrap();
        let result = runtime.backend.stop_dictation().await.unwrap();
        assert_eq!(result.raw_text, "fixture raw");
        assert_eq!(result.polished_text, "fixture polished");
        assert_eq!(transcription.pcm(), vec![1, 0, 2, 0]);
        assert_eq!(recorder.stop_count(), 1);
        runtime.backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn production_builder_wires_qa_and_https_remote_input() -> Result<(), BackendError> {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-domain-builder-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let credentials = Arc::new(InMemoryCredentialStore::default());
        let runtime = LinuxBackendBuilder::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                locale: "en-US".into(),
                ..BackendConfig::default()
            },
            Arc::new(FixtureTranscriptionEngine::successful("question", 20)),
            Arc::new(FixtureTextPolisher::successful("answer")),
        )
        .with_recorder(Arc::new(FixtureAudioRecorder::new(
            vec![vec![1, 0, 2, 0]],
            vec![(20, 0.5)],
        )))
        .with_text_inserter(Arc::new(FixtureTextInserter::with_outcome(
            InsertOutcome::Inserted,
        )))
        .with_credential_store(credentials)
        .build()?;
        runtime.backend.start().await?;

        runtime.backend.services().qa.show().await?;
        runtime.backend.services().qa.toggle_recording().await?;
        assert_eq!(
            runtime.backend.services().qa.snapshot().await?.phase,
            openless_core::QaPhase::Recording
        );
        runtime.backend.services().qa.cancel(None).await?;

        let probe = std::net::TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| BackendError::new(BackendErrorCode::Platform, error.to_string()))?;
        let port = probe
            .local_addr()
            .map_err(|error| BackendError::new(BackendErrorCode::Platform, error.to_string()))?
            .port();
        drop(probe);
        runtime
            .backend
            .services()
            .remote_input
            .configure(openless_core::RemoteInputConfig {
                enabled: true,
                port,
            })
            .await?;
        assert!(runtime.backend.services().remote_input.status()?.running);
        runtime
            .backend
            .services()
            .remote_input
            .configure(openless_core::RemoteInputConfig {
                enabled: false,
                port,
            })
            .await?;

        runtime.backend.shutdown().await?;
        let _ = std::fs::remove_dir_all(data_dir);
        Ok(())
    }

    #[tokio::test]
    async fn shared_provider_build_reports_local_asr_unsupported() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-local-asr-unsupported-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let runtime = LinuxBackendBuilder::from_shared_providers(BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        })
        .unwrap()
        .build()
        .unwrap();

        // Linux ships no local inference runtime: Core must answer Unsupported
        // rather than fabricate a Generic/Qwen capability.
        let error = runtime
            .backend
            .services()
            .local_asr
            .runtime_status(openless_core::LocalAsrRuntime::Generic)
            .await
            .expect_err("Linux must not advertise a local ASR engine");
        assert_eq!(error.code, BackendErrorCode::Unsupported);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn coding_agent_runtime_reports_unavailable_cli_without_fake_success() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-coding-agent-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let runtime = LinuxBackendBuilder::from_shared_providers(BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        })
        .unwrap()
        .build()
        .unwrap();
        let error = runtime
            .backend
            .services()
            .coding_agent
            .run_test(CodingAgentTestRequest {
                provider: CodingAgentProvider::ClaudeCodeCli,
                executable: Some("openless-command-that-does-not-exist".into()),
                prompt: "test".into(),
                permission_mode: openless_core::CodingAgentPermissionMode::Plan,
                workdir: None,
                model: None,
                max_budget_usd: Some(0.5),
                timeout_secs: 5,
            })
            .await
            .expect_err("missing coding agent executable must be explicit");
        assert_eq!(error.code, BackendErrorCode::Unsupported);
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
