//! Linux host seam for the egui frontend.
//!
//! The egui team owns the `eframe::App` and all visual/UI code.  This crate is
//! intentionally a small host adapter: it re-exports the core contract and
//! provides a place for Linux window, tray, input and resource adapters to be
//! added without making the core depend on egui or Tauri.

mod audio;
mod backend;
mod capabilities;
mod coding_agent;
mod credentials;
mod fcitx5;
mod host_actions;
mod hotkeys;
mod marketplace;
mod qa;
mod remote_input;
mod resources;
mod runtime;
mod selection;
mod settings;
mod single_instance;

pub use audio::LinuxCpalRecorder;
pub use backend::{LinuxBackendBuilder, LinuxBackendRuntime};
pub use capabilities::{LinuxCapabilitySnapshot, LinuxDesktopSession, LinuxPlatformApi};
pub use credentials::LinuxCredentialStore;
pub use fcitx5::{
    available as fcitx5_available, commit_text as fcitx5_commit_text,
    ensure_plugin_installed as ensure_fcitx5_plugin_installed,
    selection_text as fcitx5_selection_text, set_hotkeys as set_fcitx5_hotkeys,
    set_less_computer_hotkey_raw as set_fcitx5_less_computer_hotkey_raw, Fcitx5TextInserter,
    FcitxPluginInstallPlan, FcitxPluginStatus,
};
pub use host_actions::LinuxHostActions;
pub use hotkeys::{Fcitx5HotkeyListener, LinuxHotkeyEvent};
pub use resources::{
    LinuxPackageKind, LinuxResourceLayout, LinuxResourceResolver, FCITX_PLUGIN_CONFIG,
    FCITX_PLUGIN_LIBRARY,
};
pub use runtime::{LinuxNativeRuntime, LinuxRuntimePumpResult};
pub use selection::LinuxSelectionRuntime;
pub use settings::{LinuxSettingsEffects, LinuxSettingsRuntime};
pub use single_instance::{
    LinuxLaunchIntent, SingleInstanceBroker, SingleInstanceGuard, SingleInstanceRole,
};

pub use openless_core::contract::*;

/// Construction seam reserved for the Linux host implementation.
///
/// Keeping this as a named type gives the egui package a stable home for
/// platform adapters while the UI is developed independently.  No window or
/// egui object is stored here.
pub struct LinuxHost {
    backend: std::sync::Arc<OpenLessBackend>,
    settings_runtime: std::sync::Arc<dyn SettingsRuntime>,
    translation_pending: std::sync::atomic::AtomicBool,
    less_computer_voice: std::sync::Arc<std::sync::Mutex<LinuxLessComputerCaptureState>>,
}

#[derive(Default)]
struct LinuxLessComputerCaptureState {
    /// Set before entering Core so a recorder callback that fires during
    /// construction can queue its Host effect against the right generation.
    expected_session_id: Option<SessionId>,
    session: Option<LessComputerVoiceSession>,
    pending: Option<RecordingControlAction>,
}

struct LinuxLessComputerRecordingControl {
    state: std::sync::Arc<std::sync::Mutex<LinuxLessComputerCaptureState>>,
    runtime: tokio::runtime::Handle,
}

impl LinuxLessComputerRecordingControl {
    fn begin(&self, session_id: SessionId) -> Result<(), BackendError> {
        let mut state = self
            .state
            .lock()
            .expect("Linux Less Computer voice lock poisoned");
        if state.expected_session_id.is_some() || state.session.is_some() {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "Linux Less Computer capture is already active",
            ));
        }
        state.expected_session_id = Some(session_id);
        state.pending = None;
        Ok(())
    }

    fn abort_start(&self, session_id: SessionId) {
        let mut state = self
            .state
            .lock()
            .expect("Linux Less Computer voice lock poisoned");
        if state.expected_session_id == Some(session_id) && state.session.is_none() {
            state.expected_session_id = None;
            state.pending = None;
        }
    }

    fn install(&self, session: LessComputerVoiceSession) -> Result<(), BackendError> {
        let session_id = session.session_id();
        let pending = {
            let mut state = self
                .state
                .lock()
                .expect("Linux Less Computer voice lock poisoned");
            if state.expected_session_id != Some(session_id) || state.session.is_some() {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Linux Less Computer capture generation changed during startup",
                ));
            }
            state.session = Some(session);
            state.pending.take()
        };
        if let Some(action) = pending {
            self.execute(session_id, action)?;
        }
        Ok(())
    }

    fn execute(
        &self,
        session_id: SessionId,
        action: RecordingControlAction,
    ) -> Result<(), BackendError> {
        let session = {
            let mut state = self
                .state
                .lock()
                .expect("Linux Less Computer voice lock poisoned");
            if state.expected_session_id != Some(session_id) {
                return Err(BackendError::new(
                    BackendErrorCode::Cancelled,
                    "Linux Less Computer recording control is stale",
                ));
            }
            let Some(session) = state.session.take() else {
                // Recorder level/fault callbacks can run before Core returns
                // the capture object. Queue exactly one effect; Cancel wins
                // because it is the stronger terminal request.
                state.pending = Some(match (state.pending, action) {
                    (Some(RecordingControlAction::Cancel), _)
                    | (_, RecordingControlAction::Cancel) => RecordingControlAction::Cancel,
                    _ => RecordingControlAction::Stop,
                });
                return Ok(());
            };
            state.expected_session_id = None;
            state.pending = None;
            session
        };
        self.runtime.spawn(async move {
            let result = match action {
                RecordingControlAction::Stop => session.finish().await.map(|_| ()),
                RecordingControlAction::Cancel => session.cancel().await,
            };
            if let Err(error) = result {
                log::warn!("Linux Less Computer recording control failed: {error}");
            }
        });
        Ok(())
    }
}

impl RecordingControlSink for LinuxLessComputerRecordingControl {
    fn request(
        &self,
        session_id: SessionId,
        action: RecordingControlAction,
    ) -> Result<(), BackendError> {
        self.execute(session_id, action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDrainOutcome {
    Idle { processed: usize },
    Lagged { processed: usize, dropped: u64 },
    Closed { processed: usize },
}

/// Drain every event currently available without blocking an egui frame.
///
/// `Lagged` tells the caller to replace its local view model from
/// `LinuxHost::snapshot`; `Closed` means the backend subscription ended.
pub fn drain_events(
    subscription: &mut EventSubscription,
    mut apply: impl FnMut(BackendEvent),
) -> EventDrainOutcome {
    let mut processed = 0;
    loop {
        match subscription.try_recv() {
            Ok(event) => {
                processed += 1;
                apply(event);
            }
            Err(EventRecvError::Empty) => return EventDrainOutcome::Idle { processed },
            Err(EventRecvError::Lagged(dropped)) => {
                return EventDrainOutcome::Lagged { processed, dropped };
            }
            Err(EventRecvError::Closed) => return EventDrainOutcome::Closed { processed },
        }
    }
}

impl LinuxHost {
    pub fn new(backend: std::sync::Arc<OpenLessBackend>) -> Self {
        Self::with_settings_runtime(
            backend,
            std::sync::Arc::new(LinuxSettingsRuntime::hotkeys_only()),
        )
    }

    pub fn with_settings_runtime(
        backend: std::sync::Arc<OpenLessBackend>,
        settings_runtime: std::sync::Arc<dyn SettingsRuntime>,
    ) -> Self {
        Self {
            backend,
            settings_runtime,
            translation_pending: std::sync::atomic::AtomicBool::new(false),
            less_computer_voice: std::sync::Arc::new(std::sync::Mutex::new(
                LinuxLessComputerCaptureState::default(),
            )),
        }
    }

    pub fn backend(&self) -> &std::sync::Arc<OpenLessBackend> {
        &self.backend
    }

    /// Create an independent subscription for the egui view model.
    ///
    /// The subscription is intentionally owned by the caller.  A view model
    /// can keep it beside its local state and call `try_recv` from each frame
    /// without coupling the Linux host to egui types.
    pub fn subscribe(&self) -> EventSubscription {
        self.backend.subscribe()
    }

    /// Return an owned snapshot suitable for constructing or resynchronising
    /// a view model after a lagged event subscription.
    pub fn snapshot(&self) -> BackendSnapshot {
        self.backend.snapshot()
    }

    /// Persist a complete settings document using Core validation/reconciliation
    /// and the Linux platform-effect transaction.
    pub fn save_settings(
        &self,
        preferences: UserPreferences,
        expected_preferences_revision: u64,
    ) -> Result<SettingsUpdateOutcome, BackendError> {
        self.backend.update_settings(
            preferences,
            SettingsUpdateOptions::SETTINGS_DOCUMENT.at_revision(expected_preferences_revision),
            self.settings_runtime.as_ref(),
        )
    }

    /// Apply a focused settings mutation with strict shortcut-collision rules.
    pub fn update_settings_strict(
        &self,
        preferences: UserPreferences,
        expected_preferences_revision: u64,
    ) -> Result<SettingsUpdateOutcome, BackendError> {
        self.backend.update_settings(
            preferences,
            SettingsUpdateOptions::STRICT.at_revision(expected_preferences_revision),
            self.settings_runtime.as_ref(),
        )
    }

    /// Route a primary- or secondary-process launcher action without exposing
    /// Linux socket details to the egui view model.
    pub async fn dispatch_launch_intent(
        &self,
        intent: LinuxLaunchIntent,
    ) -> Result<Option<CliDispatchOutcome>, BackendError> {
        match intent {
            LinuxLaunchIntent::ShowMain => {
                self.backend.request_host_action(HostAction::ShowMain)?;
                self.backend.request_host_action(HostAction::FocusMain)?;
                Ok(None)
            }
            LinuxLaunchIntent::Cli(intent) => {
                self.backend.dispatch_cli_intent(intent).await.map(Some)
            }
        }
    }

    /// Route fcitx5 dictation and QA signals through core use-cases. Selection
    /// and translation signals remain observable because their host capture and
    /// arming adapters are injected independently from the UI.
    pub async fn dispatch_hotkey_event(
        &self,
        event: LinuxHotkeyEvent,
    ) -> Result<Option<CliDispatchOutcome>, BackendError> {
        match event {
            LinuxHotkeyEvent::LessComputerPressed { press_id, at, .. } => {
                self.dispatch_less_computer_edge(DictationHotkeyEdge::Pressed { press_id, at })
                    .await
            }
            LinuxHotkeyEvent::LessComputerReleased { press_id, at, .. } => {
                self.dispatch_less_computer_edge(DictationHotkeyEdge::Released { press_id, at })
                    .await
            }
            LinuxHotkeyEvent::LessComputerCombined { press_id, at, .. } => {
                self.dispatch_less_computer_edge(DictationHotkeyEdge::Combined { press_id, at })
                    .await
            }
            LinuxHotkeyEvent::DictationPressed { press_id, at, .. } => {
                let translation_requested = self
                    .translation_pending
                    .swap(false, std::sync::atomic::Ordering::AcqRel);
                self.backend
                    .dispatch_dictation_hotkey_edge_with_options(
                        DictationHotkeyEdge::Pressed { press_id, at },
                        DictationStartOptions {
                            translation_requested,
                            style_pack_id: None,
                            ..DictationStartOptions::default()
                        },
                    )
                    .await
                    .map(Some)
            }
            LinuxHotkeyEvent::DictationReleased { press_id, at, .. } => self
                .backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Released { press_id, at })
                .await
                .map(Some),
            LinuxHotkeyEvent::DictationCombined { press_id, at, .. } => self
                .backend
                .dispatch_dictation_hotkey_edge(DictationHotkeyEdge::Combined { press_id, at })
                .await
                .map(Some),
            LinuxHotkeyEvent::QaPressed => self
                .backend
                .dispatch_cli_intent(CliIntent::ToggleQa)
                .await
                .map(Some),
            LinuxHotkeyEvent::SelectionPolishPressed => {
                let preferences = self.backend.get_preferences();
                let style_pack = self
                    .backend
                    .get_style_pack(&preferences.selection_polish_style_pack_id)?;
                self.backend
                    .services()
                    .selection
                    .begin_polish(SelectionPolishRequest {
                        selected_text: None,
                        mode: style_pack.base_mode,
                        instruction: None,
                    })
                    .await?;
                Ok(None)
            }
            LinuxHotkeyEvent::TranslationPressed => {
                if self.backend.snapshot().dictation.phase == DictationPhase::Idle {
                    self.translation_pending
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(None)
            }
        }
    }

    async fn dispatch_less_computer_edge(
        &self,
        edge: DictationHotkeyEdge,
    ) -> Result<Option<CliDispatchOutcome>, BackendError> {
        match self.backend.dispatch_less_computer_hotkey_edge(edge) {
            LessComputerHotkeyAction::Start => {
                let session_id = SessionId::new();
                let control = std::sync::Arc::new(LinuxLessComputerRecordingControl {
                    state: std::sync::Arc::clone(&self.less_computer_voice),
                    runtime: tokio::runtime::Handle::current(),
                });
                control.begin(session_id)?;
                let session = self
                    .backend
                    .start_less_computer_voice(session_id, control.clone())
                    .await;
                match session {
                    Ok(session) => control.install(session)?,
                    Err(error) => {
                        control.abort_start(session_id);
                        return Err(error);
                    }
                }
            }
            LessComputerHotkeyAction::Finish => {
                let session_id = self
                    .less_computer_voice
                    .lock()
                    .expect("Linux Less Computer voice lock poisoned")
                    .expected_session_id;
                if let Some(session_id) = session_id {
                    LinuxLessComputerRecordingControl {
                        state: std::sync::Arc::clone(&self.less_computer_voice),
                        runtime: tokio::runtime::Handle::current(),
                    }
                    .execute(session_id, RecordingControlAction::Stop)?;
                }
            }
            LessComputerHotkeyAction::Cancel => {
                self.backend.cancel_active_voice_session(None).await?;
                let mut state = self
                    .less_computer_voice
                    .lock()
                    .expect("Linux Less Computer voice lock poisoned");
                state.expected_session_id = None;
                state.session = None;
                state.pending = None;
            }
            LessComputerHotkeyAction::Noop => {}
        }
        Ok(None)
    }

    /// Feed one canonical PCM frame from the Linux cpal callback into the
    /// active Less Computer voice session.
    pub fn feed_less_computer_pcm(&self, pcm: &[u8]) -> Result<(), BackendError> {
        self.less_computer_voice
            .lock()
            .expect("Linux Less Computer voice lock poisoned")
            .session
            .as_ref()
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "Less Computer voice session is not active",
                )
            })?
            .feed_pcm(pcm)
    }

    /// Download a Core-validated Marketplace archive and save it to a user-selected
    /// Linux filesystem path without exposing HTTP, OAuth or archive validation to UI code.
    pub async fn download_marketplace_archive(
        &self,
        pack_id: String,
        target: std::path::PathBuf,
    ) -> Result<(), BackendError> {
        let bytes = self
            .backend
            .services()
            .marketplace
            .download_archive(pack_id)
            .await?;
        marketplace::write_archive(&target, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn less_computer_recording_control_queues_early_cancel_for_the_current_generation() {
        let state = std::sync::Arc::new(std::sync::Mutex::new(
            LinuxLessComputerCaptureState::default(),
        ));
        let control = LinuxLessComputerRecordingControl {
            state: std::sync::Arc::clone(&state),
            runtime: tokio::runtime::Handle::current(),
        };
        let session_id = SessionId::new();
        control.begin(session_id).unwrap();

        control
            .request(session_id, RecordingControlAction::Stop)
            .unwrap();
        control
            .request(session_id, RecordingControlAction::Cancel)
            .unwrap();

        let queued = state.lock().unwrap();
        assert_eq!(queued.expected_session_id, Some(session_id));
        assert_eq!(queued.pending, Some(RecordingControlAction::Cancel));
        drop(queued);
        assert_eq!(
            control
                .request(SessionId::new(), RecordingControlAction::Stop)
                .unwrap_err()
                .code,
            BackendErrorCode::Cancelled
        );
    }
}
