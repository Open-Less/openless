use openless_core::{BackendError, BackendErrorCode, CliDispatchOutcome};

use crate::{
    Fcitx5HotkeyListener, LinuxBackendRuntime, LinuxHost, LinuxHostActions, SingleInstanceBroker,
};

#[derive(Debug, Default)]
pub struct LinuxRuntimePumpResult {
    pub launch_intents: usize,
    pub hotkey_events: usize,
    pub outcomes: Vec<CliDispatchOutcome>,
    pub errors: Vec<BackendError>,
}

/// Owns every non-UI Linux background resource that must stop before the
/// shared backend shuts down.
///
/// The egui team may keep this beside its app state and schedule `pump()` on
/// the host Tokio runtime. No egui/eframe type crosses this interface.
pub struct LinuxNativeRuntime {
    host: std::sync::Arc<LinuxHost>,
    startup: openless_core::StartupSnapshot,
    host_actions: std::sync::Arc<LinuxHostActions>,
    broker: Option<std::sync::Arc<SingleInstanceBroker>>,
    hotkeys: Option<Fcitx5HotkeyListener>,
}

impl LinuxNativeRuntime {
    pub async fn start(
        backend: LinuxBackendRuntime,
        broker: Option<std::sync::Arc<SingleInstanceBroker>>,
        hotkeys: Option<Fcitx5HotkeyListener>,
    ) -> Result<Self, BackendError> {
        let initialized = async {
            // fcitx5 starts with no OpenLess shortcuts on a clean installation.
            // Hydrate its native registrations from the same Core target used by
            // settings transactions before accepting any input. Equal previous/next
            // values intentionally force the effect without rewriting preferences.
            let target =
                openless_core::HotkeyRuntimeTarget::from(&backend.backend.get_preferences());
            backend
                .settings_runtime
                .commit(
                    &openless_core::SettingsEffectPlan {
                        hotkeys: Some(openless_core::SettingsValueChange {
                            previous: target.clone(),
                            next: target,
                        }),
                        ..Default::default()
                    },
                    &mut openless_core::SettingsEffectReceipt::default(),
                )
                .map_err(|failure| failure.error)?;
            let startup = backend.backend.start().await?;
            openless_core::require_backend_contract_version(&startup.contract_version)?;
            let preferences = backend.backend.get_preferences();
            backend
                .backend
                .services()
                .remote_input
                .set_locale(
                    crate::remote_input::remote_input_locale(&backend.backend.config().locale)
                        .to_string(),
                )
                .await?;
            backend
                .backend
                .services()
                .remote_input
                .configure(openless_core::RemoteInputConfig {
                    enabled: preferences.remote_input_enabled,
                    port: preferences.remote_input_port,
                })
                .await?;
            Ok::<_, BackendError>(startup)
        }
        .await;
        let startup = match initialized {
            Ok(startup) => startup,
            Err(mut error) => {
                drop(hotkeys);
                if let Err(cleanup) = backend.backend.shutdown().await {
                    error
                        .message
                        .push_str(&format!("; startup cleanup failed: {cleanup}"));
                }
                return Err(error);
            }
        };
        Ok(Self {
            host: std::sync::Arc::new(LinuxHost::with_settings_runtime(
                backend.backend,
                backend.settings_runtime,
            )),
            startup,
            host_actions: backend.host_actions,
            broker,
            hotkeys,
        })
    }

    pub fn host(&self) -> &LinuxHost {
        &self.host
    }

    pub fn host_arc(&self) -> std::sync::Arc<LinuxHost> {
        std::sync::Arc::clone(&self.host)
    }

    pub fn startup_snapshot(&self) -> &openless_core::StartupSnapshot {
        &self.startup
    }

    pub fn host_actions(&self) -> &std::sync::Arc<LinuxHostActions> {
        &self.host_actions
    }

    /// Whether the fcitx5 global-hotkey listener came up.
    ///
    /// Tauri hides the 「快捷键」 settings section when the platform has no
    /// desktop hotkey (`visibleSettingsSections(supportsDesktopHotkey)`); the
    /// egui rail needs the same honest answer instead of always showing it.
    pub fn hotkeys_available(&self) -> bool {
        self.hotkeys.is_some()
    }

    pub fn drain_native_events(
        &self,
    ) -> (
        Vec<crate::LinuxLaunchIntent>,
        Vec<crate::LinuxHotkeyEvent>,
        Vec<BackendError>,
    ) {
        let mut launch_intents = Vec::new();
        let mut hotkey_events = Vec::new();
        let mut errors = Vec::new();
        if let Some(broker) = &self.broker {
            broker.drain(|intent| launch_intents.push(intent));
            if let Some(error) = broker.take_error() {
                errors.push(BackendError::new(BackendErrorCode::Platform, error));
            }
        }
        if let Some(hotkeys) = &self.hotkeys {
            hotkeys.drain(|event| hotkey_events.push(event));
            if let Some(error) = hotkeys.take_error() {
                errors.push(error);
            }
        }
        (launch_intents, hotkey_events, errors)
    }

    /// Drain currently queued native events without blocking on DBus or Unix
    /// sockets, then execute their shared core use-cases asynchronously.
    pub async fn pump(&self) -> LinuxRuntimePumpResult {
        let mut result = LinuxRuntimePumpResult::default();
        let (launch_intents, hotkey_events, errors) = self.drain_native_events();
        result.launch_intents = launch_intents.len();
        result.hotkey_events = hotkey_events.len();
        result.errors = errors;

        for intent in launch_intents {
            match self.host.dispatch_launch_intent(intent).await {
                Ok(Some(outcome)) => result.outcomes.push(outcome),
                Ok(None) => {}
                Err(error) => result.errors.push(error),
            }
        }
        for event in hotkey_events {
            match self.host.dispatch_hotkey_event(event).await {
                Ok(Some(outcome)) => result.outcomes.push(outcome),
                Ok(None) => {}
                Err(error) => result.errors.push(error),
            }
        }
        result
    }

    /// Stop/join native listeners before asking the shared backend to cancel
    /// sessions and flush its lifecycle.
    pub async fn shutdown(mut self) -> Result<(), BackendError> {
        self.hotkeys.take();
        self.broker.take();
        self.host.backend().shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use openless_core::testing::{
        FixtureDictationEngine, FixtureTextInserter, RecordingHostActions,
        RecordingRemoteInputRuntime,
    };
    use openless_core::{
        BackendConfig, BackendDependencies, BackendServices, InMemoryCredentialStore,
        InsertOutcome, OpenLessBackend, RemoteInputService, TokioTaskSpawner,
    };

    use super::*;

    #[derive(Default)]
    struct StartupHotkeys(
        std::sync::Mutex<Vec<openless_core::HotkeyRuntimeTarget>>,
        bool,
    );

    impl crate::LinuxSettingsEffects for StartupHotkeys {
        fn apply_hotkeys(
            &self,
            target: &openless_core::HotkeyRuntimeTarget,
        ) -> Result<(), BackendError> {
            self.0.lock().unwrap().push(target.clone());
            if self.1 {
                return Err(BackendError::new(
                    BackendErrorCode::Platform,
                    "required hotkey registration failed",
                ));
            }
            Ok(())
        }

        fn set_active_asr_provider(&self, _: &str) -> Result<(), BackendError> {
            Ok(())
        }
    }

    fn fixture_backend(
        data_dir: &std::path::Path,
        services: BackendServices,
    ) -> Arc<OpenLessBackend> {
        Arc::new(
            OpenLessBackend::new(
                BackendConfig {
                    data_dir: data_dir.to_path_buf(),
                    ..BackendConfig::default()
                },
                BackendDependencies {
                    host_actions: Arc::new(RecordingHostActions::default()),
                    text_inserter: Arc::new(FixtureTextInserter::with_outcome(
                        InsertOutcome::Inserted,
                    )),
                    dictation_engine: Arc::new(FixtureDictationEngine::successful(
                        "raw", "polished",
                    )),
                    task_spawner: Arc::new(TokioTaskSpawner),
                    credential_store: Arc::new(InMemoryCredentialStore::default()),
                    services,
                    local_asr_runtime: None,
                    marketplace_config: None,
                    selection_runtime: None,
                    selection_polisher: None,
                    qa_runtime: None,
                },
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn native_runtime_starts_pumps_and_shuts_down_without_ui() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-linux-native-runtime-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let host_actions = Arc::new(LinuxHostActions::default());
        // LinuxNativeRuntime is a production shell: even when Remote Input is
        // disabled it synchronizes locale/config through the real Core service.
        // Supplying that boundary here keeps the test honest and prevents an
        // Unsupported fallback from silently returning to production startup.
        let mut services = BackendServices::unsupported();
        services.remote_input = Arc::new(
            RemoteInputService::new(Arc::new(RecordingRemoteInputRuntime::default()), 8443, "en")
                .unwrap(),
        );
        let backend = fixture_backend(&data_dir, services);
        let hotkeys = Arc::new(StartupHotkeys::default());
        let runtime = LinuxNativeRuntime::start(
            LinuxBackendRuntime {
                backend: Arc::clone(&backend),
                host_actions,
                settings_runtime: Arc::new(crate::LinuxSettingsRuntime::with_effects(
                    hotkeys.clone(),
                )),
            },
            None,
            None,
        )
        .await
        .unwrap();

        assert!(backend.snapshot().running);
        assert_eq!(
            hotkeys.0.lock().unwrap().as_slice(),
            &[openless_core::HotkeyRuntimeTarget::from(
                &backend.get_preferences()
            )]
        );
        assert_eq!(
            runtime.startup_snapshot().contract_version,
            openless_core::BACKEND_CONTRACT_VERSION
        );
        let pump = runtime.pump().await;
        assert_eq!(pump.launch_intents, 0);
        assert_eq!(pump.hotkey_events, 0);
        assert!(pump.outcomes.is_empty());
        assert!(pump.errors.is_empty());

        runtime.shutdown().await.unwrap();
        assert!(!backend.snapshot().running);
        let _ = std::fs::remove_dir_all(data_dir);
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn startup_failures_stop_core_and_keep_the_outer_lock_until_the_error_window_closes() {
        for fail_hotkeys in [true, false] {
            let dir = std::env::temp_dir()
                .join(format!("openless-startup-failure-{}", uuid::Uuid::new_v4()));
            let backend = fixture_backend(&dir, BackendServices::unsupported());
            let lock = dir.join("openless.lock");
            let broker = match crate::SingleInstanceBroker::acquire_or_forward(
                &lock,
                &dir.join("openless.sock"),
                crate::LinuxLaunchIntent::ShowMain,
            )
            .unwrap()
            {
                crate::SingleInstanceRole::Primary(broker) => Arc::new(broker),
                crate::SingleInstanceRole::Forwarded => panic!("isolated test must own its lock"),
            };
            let mut events = backend.subscribe();
            let result = LinuxNativeRuntime::start(
                LinuxBackendRuntime {
                    backend: backend.clone(),
                    host_actions: Arc::new(LinuxHostActions::default()),
                    settings_runtime: Arc::new(crate::LinuxSettingsRuntime::with_effects(
                        Arc::new(StartupHotkeys(Default::default(), fail_hotkeys)),
                    )),
                },
                Some(broker.clone()),
                None,
            )
            .await;
            assert!(result.is_err());
            assert!(!backend.snapshot().running);
            let mut started = false;
            while let Ok(event) = events.try_recv() {
                started |= matches!(event.kind, openless_core::BackendEventKind::BackendStarted);
            }
            assert_eq!(
                started, !fail_hotkeys,
                "registration must precede Core startup"
            );
            assert!(crate::SingleInstanceGuard::acquire(&lock)
                .unwrap()
                .is_none());
            drop(broker);
            assert!(crate::SingleInstanceGuard::acquire(&lock)
                .unwrap()
                .is_some());
            drop(backend);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
