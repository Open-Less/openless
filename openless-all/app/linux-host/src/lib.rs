//! Linux 平台适配层：Core 的 Linux 音频/凭据/输入/设置适配器 + 托盘 + 热键。
//!
//! 这里**不包含任何 UI 渲染**：窗口层由宿主进程（当前的 Qt/WebKit 宿主
//! `openless-linux-web`）负责，本 crate 只提供后端装配与平台能力。

pub use openless_core::{
    BackendError, BackendErrorCode, BackendEvent, BackendSnapshot, CliDispatchOutcome, CliIntent,
    DictationHotkeyEdge, DictationPhase, DictationStartOptions, EventRecvError, EventSubscription,
    HostAction, OpenLessBackend, SelectionPolishRequest, SettingsRuntime, SettingsUpdateOptions,
    SettingsUpdateOutcome, UserPreferences,
};

mod audio;
mod audio_cue;
mod audio_mute;
mod audio_player;
mod backend;
mod capabilities;
mod credentials;
mod desktop;
mod fcitx5;
mod host_actions;
mod host_context;
mod hotkeys;
mod i18n;
pub mod linux_access;
pub mod local_hotkeys;
mod logging;
mod omni_settings;
mod preference_patch;
mod provider_protocol;
mod qa;
mod recordings;
mod remote_input;
mod resources;
mod runtime;
mod selection;
mod settings;
mod single_instance;
mod style_icon;
mod siwc;
mod tray;
pub mod update_check;

pub use audio::LinuxCpalRecorder;
#[cfg(target_os = "linux")]
pub use audio::{list_loopback_devices, start_reference_capture, ReferenceCapture};
pub use audio_cue::{play_cue_start, play_cue_stop, CueTone};
pub use audio_mute::AudioMuteGuard;
pub use audio_player::ClipPlayer;
pub use backend::{LinuxBackendBuilder, LinuxBackendRuntime};
pub use capabilities::{LinuxCapabilitySnapshot, LinuxDesktopSession, LinuxPlatformApi};
pub use credentials::LinuxCredentialStore;
pub use desktop::{
    atomic_save, notify, open_external, open_local_file, validate_save_path, AutostartManager,
    DesktopError, Notification,
};
#[cfg(target_os = "linux")]
pub use fcitx5::prepare_fcitx5;
pub use fcitx5::{
    available as fcitx5_available, commit_text as fcitx5_commit_text,
    copy_to_clipboard as fcitx5_copy_to_clipboard,
    ensure_plugin_installed as ensure_fcitx5_plugin_installed, reload_fcitx5_if_plugin_updated,
    reload_running_fcitx5, selection_text as fcitx5_selection_text,
    set_hotkeys as set_fcitx5_hotkeys, surrounding_text as fcitx5_surrounding_text,
    Fcitx5TextInserter, FcitxPluginInstallPlan, FcitxPluginStatus, SurroundingText,
};
pub use host_actions::LinuxHostActions;

/// Apply one QA/selection-voice edit through the same fcitx5 selection target
/// the `SelectionApi` uses. `session_id` is the selection-voice session the host
/// registered via `rekey_selection_target`, i.e. the Core's apply ticket
/// `session_id`. Returns `Cancelled` when the selection changed meanwhile.
pub fn apply_selection_voice_target(
    session_id: &str,
    source: &str,
    replacement: &str,
) -> Result<(), openless_core::BackendError> {
    fcitx5::apply_selection_target(session_id, source, replacement)
}
pub use hotkeys::{Fcitx5HotkeyListener, LinuxHotkeyEvent};
pub use i18n::{
    fmt_catalog as fmt_l10n, provider_error_text, tr_catalog as tr_l10n, Lang, LocalePref, LANGS,
};
pub use local_hotkeys::{
    next_local_press_id, plugin_event_hotkey, style_pack_raw, translation_hotkey_event,
    HotkeyDeduplicator, LocalHotkey, LocalHotkeyEdge, LocalHotkeyEdgeKind, HOTKEY_DEDUPE_WINDOW,
};
pub use logging::{
    export_error_log, export_error_logs, init_file_logger, level_name, log_path, read_log_level,
    write_log_level,
};
#[cfg(all(target_os = "linux", feature = "x11-overlay"))]

pub use openless_core::contract::*;
pub use preference_patch::patch_preferences;
pub use recordings::{read_recording_wav, recording_path, recording_pcm, RecordingError};
pub use resources::{
    LinuxPackageKind, LinuxResourceLayout, LinuxResourceResolver, FCITX_PLUGIN_CONFIG,
    FCITX_PLUGIN_LIBRARY,
};
pub use runtime::{LinuxNativeRuntime, LinuxRuntimePumpResult};
pub use selection::LinuxSelectionRuntime;
pub use settings::{
    is_bare_modifier_binding, shortcut_display_label, LinuxSettingsEffects, LinuxSettingsRuntime,
};
pub use single_instance::{
    LinuxLaunchIntent, SingleInstanceBroker, SingleInstanceGuard, SingleInstanceRole,
};
pub use tray::{LinuxTray, TrayCommand, TrayError, TrayMicrophone};

/// Coordinates Core operations with Linux settings and recording lifecycles.
///
/// Capture state binds recorder callbacks to their owning Core session. Window
/// objects and UI widgets stay in the frontend.
pub struct LinuxHost {
    backend: std::sync::Arc<OpenLessBackend>,
    settings_runtime: std::sync::Arc<dyn SettingsRuntime>,
    translation_pending: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDrainOutcome {
    Idle { processed: usize },
    Lagged { processed: usize, dropped: u64 },
    Closed { processed: usize },
}

/// Drain every event currently available without blocking an UI frame.
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
        }
    }

    pub fn backend(&self) -> &std::sync::Arc<OpenLessBackend> {
        &self.backend
    }

    /// Create an independent subscription for the UI view model.
    ///
    /// The subscription is intentionally owned by the caller.  A view model
    /// can keep it beside its local state and call `try_recv` from each frame
    /// without coupling the Linux host to UI types.
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
        mut preferences: UserPreferences,
        expected_preferences_revision: u64,
    ) -> Result<SettingsUpdateOutcome, BackendError> {
        // The retired Linux binding must not reserve keys in Core's shared validation.
        preferences.coding_agent_voice_hotkey = None;
        self.backend.update_settings(
            preferences,
            SettingsUpdateOptions::SETTINGS_DOCUMENT.at_revision(expected_preferences_revision),
            self.settings_runtime.as_ref(),
        )
    }

    /// Apply a focused settings mutation with strict shortcut-collision rules.
    pub fn update_settings_strict(
        &self,
        mut preferences: UserPreferences,
        expected_preferences_revision: u64,
    ) -> Result<SettingsUpdateOutcome, BackendError> {
        // The retired Linux binding must not reserve keys in Core's shared validation.
        preferences.coding_agent_voice_hotkey = None;
        self.backend.update_settings(
            preferences,
            SettingsUpdateOptions::STRICT.at_revision(expected_preferences_revision),
            self.settings_runtime.as_ref(),
        )
    }

    /// Route a primary- or secondary-process launcher action without exposing
    /// Linux socket details to the UI view model.
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
            LinuxHotkeyEvent::QuickNotePressed => Ok(None), // Host toggles the permanent recording.
            LinuxHotkeyEvent::QaPressed => {
                // 用户报「选区助手快捷键打不开」时，这一行 + 宿主的 ShowQa/HideQa
                // 日志能直接区分「键没到」和「到了但被隐藏」。
                log::info!("[hotkey] selection-ask hotkey pressed; toggling the QA panel");
                self.backend
                    .dispatch_cli_intent(CliIntent::ToggleQa)
                    .await
                    .map(Some)
            }
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
            LinuxHotkeyEvent::TranslationPressed { .. } => {
                if self.backend.snapshot().dictation.phase == DictationPhase::Idle {
                    self.translation_pending
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                Ok(None)
            }
            LinuxHotkeyEvent::SwitchStylePressed => {
                self.backend.activate_previous_style_pack()?;
                Ok(None)
            }
            LinuxHotkeyEvent::OpenAppPressed => {
                self.backend.request_host_action(HostAction::ShowMain)?;
                self.backend.request_host_action(HostAction::FocusMain)?;
                Ok(None)
            }
            LinuxHotkeyEvent::StylePackPressed { symbol, states } => {
                let preferences = self.backend.get_preferences();
                let pack_id = preferences
                    .style_pack_hotkeys
                    .iter()
                    .find_map(|hotkey| {
                        crate::settings::shortcut_to_raw(&hotkey.binding)
                            .ok()
                            .filter(|raw| *raw == (symbol, states))
                            .map(|_| hotkey.pack_id.clone())
                    })
                    .ok_or_else(|| {
                        BackendError::new(
                            BackendErrorCode::Cancelled,
                            "style-pack hotkey no longer matches current settings",
                        )
                    })?;
                self.backend.activate_style_pack(&pack_id)?;
                Ok(None)
            }
        }
    }
}
