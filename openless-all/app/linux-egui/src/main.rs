#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("openless-linux-egui is only available on Linux");
}

#[cfg(target_os = "linux")]
mod ui;

#[cfg(target_os = "linux")]
mod linux_app {
    use std::future::Future;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::ui::{shell, theme};
    use eframe::egui;
    use openless_core::{
        BackendConfig, BackendError, BackendEvent, BackendEventKind, BackendSnapshot,
        DictationPhase, HistoryInsertStatus, HostAction, LessComputerEventKind, LocalAsrModel,
        LocalAsrRuntime, QaStateEvent, QaStateKind, SelectionPhase, SelectionSnapshot,
        TranscriptAccumulator, UserPreferences,
    };
    use openless_linux_egui::{
        drain_events, ensure_fcitx5_plugin_installed, notify, open_external, reload_running_fcitx5,
        write_jsonl, EventDrainOutcome, Fcitx5HotkeyListener, FcitxPluginInstallPlan,
        FcitxPluginStatus, HostToPopup, LinuxBackendBuilder, LinuxCapabilitySnapshot,
        LinuxLaunchIntent, LinuxNativeRuntime, LinuxPackageKind, LinuxResourceLayout,
        LinuxUpdateSupport, Notification, PopupActionGuard, PopupChatMessage, PopupKind,
        PopupState, PopupSupervisor, PopupSupervisorEvent, PopupToHost, SingleInstanceBroker,
        SingleInstanceRole, UpdateManifest, UpdateSchedule, POPUP_PROTOCOL_VERSION,
    };

    enum UiResult {
        Message(String),
        Models(Result<Vec<LocalAsrModel>, String>),
        Remote(Result<(openless_core::RemoteInputStatus, String), String>),
        Providers(Result<ProviderPanel, String>),
        ProviderEditor {
            kind: openless_core::ChannelKind,
            channel_id: String,
            result: Box<Result<ProviderEditor, String>>,
        },
        ProviderModels {
            kind: openless_core::ChannelKind,
            channel_id: String,
            result: Result<Vec<String>, String>,
        },
        ProviderMutation(Result<String, String>),
        Library(Result<LibraryPanel, String>),
        SettingsSaved(Box<Result<openless_core::SettingsUpdateOutcome, String>>),
        Marketplace(Result<Vec<openless_core::MarketplaceListItem>, String>),
        MarketplaceFlow(Result<openless_core::OAuthDeviceFlow, String>),
        MarketplaceAuthPoll(Result<openless_core::OAuthPollResult, String>),
        MarketplaceDetail(Result<openless_core::MarketplaceDetail, String>),
        MarketplaceMine(Result<(Vec<openless_core::MarketplaceMyPackItem>, Vec<String>), String>),
        Microphones(Result<Vec<openless_core::MicrophoneDevice>, String>),
        UpdateCheck(Result<Option<UpdateManifest>, String>),
        UpdateProgress(openless_linux_egui::DownloadProgress),
        UpdateInstalled(Result<openless_linux_egui::InstalledUpdate, String>),
        ModelMutation(Result<String, String>),
    }

    #[derive(Clone)]
    enum ModelsState {
        Loading,
        Loaded(Vec<LocalAsrModel>),
        Failed(String),
    }

    #[derive(Clone)]
    struct ProviderPanel {
        kind: openless_core::ChannelKind,
        descriptors: Vec<openless_core::ProviderDescriptor>,
        channels: Vec<openless_core::ChannelSummary>,
        active_provider: String,
    }

    struct LibraryPanel {
        vocabulary: Vec<openless_core::DictionaryEntry>,
        correction_rules: Vec<openless_core::CorrectionRule>,
        style_packs: Vec<openless_core::StylePack>,
        vocab_preset_store: openless_core::VocabPresetStore,
        vocab_presets: Vec<openless_core::VocabPreset>,
    }

    #[derive(Default, Clone, Copy)]
    struct SettingsDirty {
        streaming_insert: bool,
        coding_agent_enabled: bool,
        start_minimized: bool,
        launch_at_login: bool,
        auto_update_check: bool,
        update_channel: bool,
        remote_input_enabled: bool,
        remote_input_port: bool,
        recording: bool,
        microphone: bool,
        appearance: bool,
        hotkeys: bool,
    }

    impl SettingsDirty {
        fn any(&self) -> bool {
            self.streaming_insert
                || self.coding_agent_enabled
                || self.start_minimized
                || self.launch_at_login
                || self.auto_update_check
                || self.update_channel
                || self.remote_input_enabled
                || self.remote_input_port
                || self.recording
                || self.microphone
                || self.appearance
                || self.hotkeys
        }

        fn merge(&self, latest: &UserPreferences, draft: &UserPreferences) -> UserPreferences {
            let mut merged = latest.clone();
            if self.streaming_insert {
                merged.streaming_insert = draft.streaming_insert;
            }
            if self.coding_agent_enabled {
                merged.coding_agent_enabled = draft.coding_agent_enabled;
            }
            if self.start_minimized {
                merged.start_minimized = draft.start_minimized;
            }
            if self.launch_at_login {
                merged.launch_at_login = draft.launch_at_login;
            }
            if self.auto_update_check {
                merged.auto_update_check = draft.auto_update_check;
            }
            if self.update_channel {
                merged.update_channel = draft.update_channel;
            }
            if self.remote_input_enabled {
                merged.remote_input_enabled = draft.remote_input_enabled;
            }
            if self.remote_input_port {
                merged.remote_input_port = draft.remote_input_port;
            }
            if self.recording {
                merged.hotkey.mode = draft.hotkey.mode;
                merged.silence_auto_stop_enabled = draft.silence_auto_stop_enabled;
                merged.silence_auto_stop_seconds = draft.silence_auto_stop_seconds;
            }
            if self.microphone {
                merged.microphone_device_name = draft.microphone_device_name.clone();
            }
            if self.appearance {
                merged.theme_mode = draft.theme_mode;
                merged.show_overview_activity_heatmap = draft.show_overview_activity_heatmap;
            }
            if self.hotkeys {
                merged.dictation_hotkey = draft.dictation_hotkey.clone();
                merged.hotkey = draft.hotkey.clone();
                merged.qa_hotkey = draft.qa_hotkey.clone();
                merged.translation_hotkey = draft.translation_hotkey.clone();
                merged.switch_style_hotkey = draft.switch_style_hotkey.clone();
                merged.open_app_hotkey = draft.open_app_hotkey.clone();
                merged.selection_polish_hotkey = draft.selection_polish_hotkey.clone();
                merged.coding_agent_voice_hotkey = draft.coding_agent_voice_hotkey.clone();
            }
            merged
        }
    }

    #[derive(Clone)]
    enum ProvidersState {
        Loading,
        Loaded(ProviderPanel),
        Failed(String),
    }

    #[derive(Clone)]
    struct ProviderEditor {
        kind: openless_core::ChannelKind,
        channel: openless_core::ChannelSummary,
        descriptor: openless_core::ProviderDescriptor,
        name: String,
        endpoint: String,
        model: String,
        auth_mode: String,
        resource_id: String,
        // Secret inputs are intentionally write-only. Loading an editor never
        // exposes an existing key into egui state, logs or screenshots.
        primary_secret: String,
        secondary_secret: String,
    }

    #[derive(Clone)]
    enum ProviderEditorState {
        Idle,
        Loading {
            kind: openless_core::ChannelKind,
            channel_id: String,
        },
        Loaded(Box<ProviderEditor>),
        Failed(String),
    }

    pub struct OpenLessEguiApp {
        tokio: Arc<tokio::runtime::Runtime>,
        native: Option<LinuxNativeRuntime>,
        subscription: Option<openless_core::EventSubscription>,
        snapshot: Option<BackendSnapshot>,
        preferences: Option<UserPreferences>,
        settings_dirty: SettingsDirty,
        microphones: Vec<openless_core::MicrophoneDevice>,
        models: ModelsState,
        transcript: String,
        transcript_state: TranscriptAccumulator,
        transcript_session: Option<openless_core::SessionId>,
        last_event_sequence: u64,
        less_computer_input: String,
        less_computer_output: String,
        less_computer_turn_start: usize,
        less_computer_session: Option<openless_core::SessionId>,
        pending_approval: Option<(String, String)>,
        qa_visible: bool,
        qa_input: String,
        qa_state: Option<QaStateEvent>,
        selection_preview_visible: bool,
        selection_draft: String,
        selection: Option<SelectionSnapshot>,
        remote_access: Option<(openless_core::RemoteInputStatus, String)>,
        provider_kind: openless_core::ChannelKind,
        providers: ProvidersState,
        selected_channel_id: Option<String>,
        provider_editor: ProviderEditorState,
        provider_models: Vec<String>,
        new_provider_type: String,
        new_channel_name: String,
        pending_channel_delete: Option<String>,
        vocabulary: Vec<openless_core::DictionaryEntry>,
        correction_rules: Vec<openless_core::CorrectionRule>,
        style_packs: Vec<openless_core::StylePack>,
        vocabulary_phrase: String,
        vocabulary_note: String,
        correction_pattern: String,
        correction_replacement: String,
        vocab_preset_store: openless_core::VocabPresetStore,
        vocab_presets: Vec<openless_core::VocabPreset>,
        vocab_preset_name: String,
        vocab_preset_phrases: String,
        history_search: String,
        qa_popup: Option<PopupSupervisor>,
        preview_popup: Option<PopupSupervisor>,
        capsule_popup: Option<PopupSupervisor>,
        popup_action_guard: PopupActionGuard,
        tray: Option<openless_linux_egui::LinuxTray>,
        exit_requested: bool,
        update_support: LinuxUpdateSupport,
        update_schedule: UpdateSchedule,
        update_started: std::time::Instant,
        update_manifest: Option<UpdateManifest>,
        update_busy: bool,
        update_progress: Option<openless_linux_egui::DownloadProgress>,
        marketplace_items: Vec<openless_core::MarketplaceListItem>,
        marketplace_query: String,
        marketplace_flow: Option<openless_core::OAuthDeviceFlow>,
        marketplace_detail: Option<openless_core::MarketplaceDetail>,
        marketplace_my_packs: Vec<openless_core::MarketplaceMyPackItem>,
        marketplace_my_likes: Vec<String>,
        style_editor: Option<openless_core::StylePack>,
        style_hotkey_pack_id: String,
        style_hotkey_primary: String,
        style_hotkey_modifiers: String,
        status: String,
        startup_error: Option<String>,
        active_page: shell::Page,
        tx: mpsc::Sender<UiResult>,
        rx: mpsc::Receiver<UiResult>,
    }

    impl OpenLessEguiApp {
        fn new(
            tokio: Arc<tokio::runtime::Runtime>,
            native: Result<LinuxNativeRuntime, String>,
            tray: Option<openless_linux_egui::LinuxTray>,
            update_support: LinuxUpdateSupport,
        ) -> Self {
            let (tx, rx) = mpsc::channel();
            match native {
                Ok(native) => {
                    let backend = native.host().backend();
                    let snapshot = backend.snapshot();
                    let preferences = backend.get_preferences();
                    let subscription = backend.subscribe();
                    let app = Self {
                        tokio,
                        native: Some(native),
                        subscription: Some(subscription),
                        snapshot: Some(snapshot),
                        preferences: Some(preferences),
                        settings_dirty: SettingsDirty::default(),
                        microphones: Vec::new(),
                        models: ModelsState::Loading,
                        transcript: String::new(),
                        transcript_state: TranscriptAccumulator::default(),
                        transcript_session: None,
                        last_event_sequence: 0,
                        less_computer_input: String::new(),
                        less_computer_output: String::new(),
                        less_computer_turn_start: 0,
                        less_computer_session: None,
                        pending_approval: None,
                        qa_visible: false,
                        qa_input: String::new(),
                        qa_state: None,
                        selection_preview_visible: false,
                        selection_draft: String::new(),
                        selection: None,
                        remote_access: None,
                        provider_kind: openless_core::ChannelKind::Asr,
                        providers: ProvidersState::Loading,
                        selected_channel_id: None,
                        provider_editor: ProviderEditorState::Idle,
                        provider_models: Vec::new(),
                        new_provider_type: String::new(),
                        new_channel_name: String::new(),
                        pending_channel_delete: None,
                        vocabulary: Vec::new(),
                        correction_rules: Vec::new(),
                        style_packs: Vec::new(),
                        vocabulary_phrase: String::new(),
                        vocabulary_note: String::new(),
                        correction_pattern: String::new(),
                        correction_replacement: String::new(),
                        vocab_preset_store: openless_core::VocabPresetStore::default(),
                        vocab_presets: Vec::new(),
                        vocab_preset_name: String::new(),
                        vocab_preset_phrases: String::new(),
                        history_search: String::new(),
                        qa_popup: None,
                        preview_popup: None,
                        capsule_popup: None,
                        popup_action_guard: PopupActionGuard::default(),
                        tray,
                        exit_requested: false,
                        update_support,
                        update_schedule: UpdateSchedule::new(Duration::ZERO),
                        update_started: std::time::Instant::now(),
                        update_manifest: None,
                        update_busy: false,
                        update_progress: None,
                        marketplace_items: Vec::new(),
                        marketplace_query: String::new(),
                        marketplace_flow: None,
                        marketplace_detail: None,
                        marketplace_my_packs: Vec::new(),
                        marketplace_my_likes: Vec::new(),
                        style_editor: None,
                        style_hotkey_pack_id: String::new(),
                        style_hotkey_primary: String::new(),
                        style_hotkey_modifiers: String::new(),
                        status: "Core 2.0 已启动".to_string(),
                        startup_error: None,
                        active_page: shell::Page::Overview,
                        tx,
                        rx,
                    };
                    app.load_models();
                    app.load_remote_status();
                    app.load_providers(openless_core::ChannelKind::Asr);
                    app.load_library();
                    app.load_microphones();
                    app
                }
                Err(error) => Self {
                    tokio,
                    native: None,
                    subscription: None,
                    snapshot: None,
                    preferences: None,
                    settings_dirty: SettingsDirty::default(),
                    microphones: Vec::new(),
                    models: ModelsState::Loading,
                    transcript: String::new(),
                    transcript_state: TranscriptAccumulator::default(),
                    transcript_session: None,
                    last_event_sequence: 0,
                    less_computer_input: String::new(),
                    less_computer_output: String::new(),
                    less_computer_turn_start: 0,
                    less_computer_session: None,
                    pending_approval: None,
                    qa_visible: false,
                    qa_input: String::new(),
                    qa_state: None,
                    selection_preview_visible: false,
                    selection_draft: String::new(),
                    selection: None,
                    remote_access: None,
                    provider_kind: openless_core::ChannelKind::Asr,
                    providers: ProvidersState::Loading,
                    selected_channel_id: None,
                    provider_editor: ProviderEditorState::Idle,
                    provider_models: Vec::new(),
                    new_provider_type: String::new(),
                    new_channel_name: String::new(),
                    pending_channel_delete: None,
                    vocabulary: Vec::new(),
                    correction_rules: Vec::new(),
                    style_packs: Vec::new(),
                    vocabulary_phrase: String::new(),
                    vocabulary_note: String::new(),
                    correction_pattern: String::new(),
                    correction_replacement: String::new(),
                    vocab_preset_store: openless_core::VocabPresetStore::default(),
                    vocab_presets: Vec::new(),
                    vocab_preset_name: String::new(),
                    vocab_preset_phrases: String::new(),
                    history_search: String::new(),
                    qa_popup: None,
                    preview_popup: None,
                    capsule_popup: None,
                    popup_action_guard: PopupActionGuard::default(),
                    tray,
                    exit_requested: false,
                    update_support,
                    update_schedule: UpdateSchedule::new(Duration::ZERO),
                    update_started: std::time::Instant::now(),
                    update_manifest: None,
                    update_busy: false,
                    update_progress: None,
                    marketplace_items: Vec::new(),
                    marketplace_query: String::new(),
                    marketplace_flow: None,
                    marketplace_detail: None,
                    marketplace_my_packs: Vec::new(),
                    marketplace_my_likes: Vec::new(),
                    style_editor: None,
                    style_hotkey_pack_id: String::new(),
                    style_hotkey_primary: String::new(),
                    style_hotkey_modifiers: String::new(),
                    status: "启动失败".to_string(),
                    startup_error: Some(error),
                    active_page: shell::Page::Overview,
                    tx,
                    rx,
                },
            }
        }

        fn backend(&self) -> Option<Arc<openless_core::OpenLessBackend>> {
            self.native
                .as_ref()
                .map(|native| Arc::clone(native.host().backend()))
        }

        fn popup_slot(&mut self, kind: PopupKind) -> &mut Option<PopupSupervisor> {
            match kind {
                PopupKind::Qa => &mut self.qa_popup,
                PopupKind::Preview => &mut self.preview_popup,
                PopupKind::Capsule => &mut self.capsule_popup,
            }
        }

        fn ensure_popup(&mut self, kind: PopupKind) {
            if self.popup_slot(kind).is_some() {
                return;
            }
            match std::env::current_exe() {
                Ok(executable) => {
                    self.popup_action_guard.reset(kind);
                    let supervisor = PopupSupervisor::spawn(self.tokio.handle(), executable, kind);
                    *self.popup_slot(kind) = Some(supervisor);
                }
                Err(error) => self.status = format!("无法启动原生弹窗：{error}"),
            }
        }

        fn send_popup(&mut self, kind: PopupKind, message: HostToPopup) {
            let retry = message.clone();
            if let Some(supervisor) = self.popup_slot(kind) {
                if let Err(error) = supervisor.try_send(message) {
                    self.status = format!("原生弹窗通道重建：{error:?}");
                    *self.popup_slot(kind) = None;
                    self.ensure_popup(kind);
                    if let Some(supervisor) = self.popup_slot(kind) {
                        if let Err(retry_error) = supervisor.try_send(retry) {
                            self.status = format!("原生弹窗恢复失败：{retry_error:?}");
                        }
                    }
                }
            }
        }

        fn hide_popup(&mut self, kind: PopupKind, session_id: String, sequence: u64) {
            self.send_popup(
                kind,
                HostToPopup::Hide {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id,
                    sequence,
                },
            );
        }

        fn expected_popup_session(&self, kind: PopupKind) -> Option<String> {
            match kind {
                PopupKind::Qa => self
                    .qa_state
                    .as_ref()
                    .map(|state| state.session_id.clone().unwrap_or_else(|| "qa".to_string())),
                PopupKind::Preview => self
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.session_id)
                    .map(|session_id| session_id.to_string()),
                PopupKind::Capsule => self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.dictation.session_id)
                    .map(|session_id| session_id.to_string()),
            }
        }

        fn show_qa_popup(&mut self) {
            self.ensure_popup(PopupKind::Qa);
            let Some(state) = self.qa_state.clone() else {
                return;
            };
            self.send_popup(
                PopupKind::Qa,
                HostToPopup::QaSnapshot {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: state.session_id.unwrap_or_else(|| "qa".to_string()),
                    sequence: self.last_event_sequence.saturating_mul(2),
                    phase: format!("{:?}", state.kind),
                    messages: state
                        .messages
                        .unwrap_or_default()
                        .into_iter()
                        .map(|message| PopupChatMessage {
                            role: message.role,
                            content: message.content,
                            selection_text: message.selection_text,
                        })
                        .collect(),
                    selection_preview: state.selection_preview,
                    streaming_answer: state.chunk.unwrap_or_default(),
                    error: state.error,
                },
            );
        }

        fn show_selection_popup(&mut self) {
            self.ensure_popup(PopupKind::Preview);
            let Some(selection) = self.selection.clone() else {
                return;
            };
            let Some(session_id) = selection.session_id else {
                return;
            };
            self.send_popup(
                PopupKind::Preview,
                HostToPopup::Preview {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.to_string(),
                    sequence: self.last_event_sequence.saturating_mul(2),
                    text: selection.preview_text.unwrap_or_default(),
                    source: selection.source_text.unwrap_or_default(),
                },
            );
        }

        fn show_capsule_popup(&mut self) {
            self.ensure_popup(PopupKind::Capsule);
            let Some(snapshot) = self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.dictation.clone())
            else {
                return;
            };
            let Some(session_id) = snapshot.session_id else {
                return;
            };
            self.send_popup(
                PopupKind::Capsule,
                HostToPopup::Capsule {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.to_string(),
                    sequence: self.last_event_sequence.saturating_mul(2),
                    phase: format!("{:?}", snapshot.phase),
                    text: snapshot.message.unwrap_or_default(),
                    audio_level: Some(snapshot.level),
                },
            );
        }

        fn poll_popup_supervisors(&mut self) {
            let mut events = Vec::new();
            for kind in [PopupKind::Qa, PopupKind::Preview, PopupKind::Capsule] {
                if let Some(supervisor) = self.popup_slot(kind) {
                    while let Ok(event) = supervisor.try_recv() {
                        events.push((kind, event));
                    }
                }
            }
            for (kind, event) in events {
                if let PopupSupervisorEvent::Message(message) = &event {
                    let Some(expected_session) = self.expected_popup_session(kind) else {
                        self.status = "已忽略没有活动会话的弹窗操作".to_string();
                        continue;
                    };
                    if !self
                        .popup_action_guard
                        .accept(kind, message, &expected_session)
                    {
                        self.status = "已忽略迟到、重复或跨类型的弹窗操作".to_string();
                        continue;
                    }
                }
                match event {
                    PopupSupervisorEvent::Message(PopupToHost::SubmitQa {
                        session_id,
                        text,
                        ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.services().qa.submit_text(text).await?;
                                Ok("问答已提交".to_string())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::ToggleQaRecording {
                        session_id,
                        ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.services().qa.toggle_recording().await?;
                                Ok("问答录音状态已更新".to_string())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::DismissQa {
                        session_id, ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.services().qa.dismiss().await?;
                                Ok("问答已关闭".to_string())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::ConfirmPreview {
                        session_id,
                        text,
                        ..
                    }) => match session_id.parse::<uuid::Uuid>() {
                        Ok(session_id) => {
                            let session_id = openless_core::SessionId::from_uuid(session_id);
                            if let Some(backend) = self.backend() {
                                self.spawn(async move {
                                    backend
                                        .services()
                                        .selection
                                        .confirm(session_id, Some(text))
                                        .await?;
                                    Ok("选区替换已确认".to_string())
                                });
                            }
                        }
                        Err(error) => self.status = format!("弹窗 session 无效：{error}"),
                    },
                    PopupSupervisorEvent::Message(PopupToHost::CancelPreview {
                        session_id,
                        ..
                    }) => match session_id.parse::<uuid::Uuid>() {
                        Ok(session_id) => {
                            let session_id = openless_core::SessionId::from_uuid(session_id);
                            if let Some(backend) = self.backend() {
                                self.spawn(async move {
                                    backend
                                        .services()
                                        .selection
                                        .cancel(Some(session_id))
                                        .await?;
                                    Ok("选区替换已取消".to_string())
                                });
                            }
                        }
                        Err(error) => self.status = format!("弹窗 session 无效：{error}"),
                    },
                    PopupSupervisorEvent::Message(PopupToHost::Ready { .. }) => match kind {
                        PopupKind::Qa => self.show_qa_popup(),
                        PopupKind::Preview => self.show_selection_popup(),
                        PopupKind::Capsule => self.show_capsule_popup(),
                    },
                    PopupSupervisorEvent::Message(PopupToHost::DismissCapsule { .. }) => {
                        if let Some(snapshot) = self.snapshot.as_mut() {
                            snapshot.dictation.message = None;
                        }
                    }
                    PopupSupervisorEvent::Message(
                        PopupToHost::SubmitQa { .. }
                        | PopupToHost::ToggleQaRecording { .. }
                        | PopupToHost::DismissQa { .. },
                    ) => {
                        self.status = "已忽略迟到的问答弹窗操作".to_string();
                    }
                    PopupSupervisorEvent::ProtocolError(error) => {
                        self.status = format!("原生弹窗协议错误：{error}");
                    }
                    PopupSupervisorEvent::SpawnFailed(error) => {
                        self.status = format!("原生弹窗启动失败：{error}");
                        *self.popup_slot(kind) = None;
                    }
                    PopupSupervisorEvent::Exited { code, crashed } => {
                        if crashed {
                            self.status = format!("原生弹窗异常退出：{code:?}");
                        }
                        *self.popup_slot(kind) = None;
                        if crashed {
                            match kind {
                                PopupKind::Qa if self.qa_visible => self.show_qa_popup(),
                                PopupKind::Preview if self.selection_preview_visible => {
                                    self.show_selection_popup();
                                }
                                PopupKind::Capsule
                                    if self.snapshot.as_ref().is_some_and(|snapshot| {
                                        snapshot.dictation.phase != DictationPhase::Idle
                                    }) =>
                                {
                                    self.show_capsule_popup();
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        fn spawn<F>(&self, future: F)
        where
            F: Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let message = future.await.unwrap_or_else(|error| error.to_string());
                let _ = tx.send(UiResult::Message(message));
            });
        }

        fn load_models(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let models = backend
                    .services()
                    .local_asr
                    .list_models(LocalAsrRuntime::Generic)
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Models(models));
            });
        }

        fn load_remote_status(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = async {
                    let status = backend.services().remote_input.status()?;
                    let pin = if status.enabled {
                        backend
                            .services()
                            .remote_input
                            .read_pairing_pin()
                            .await?
                            .into_exposed()
                    } else {
                        String::new()
                    };
                    Ok::<_, BackendError>((status, pin))
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Remote(result));
            });
        }

        fn load_providers(&self, kind: openless_core::ChannelKind) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = async {
                    let provider_kind = provider_kind(kind);
                    let mut channels = backend.list_channels(kind).await?;
                    channels.sort_by_key(|channel| channel.order);
                    Ok::<_, BackendError>(ProviderPanel {
                        kind,
                        descriptors: openless_core::provider_rules::provider_descriptors(
                            provider_kind,
                        ),
                        channels,
                        active_provider: backend.active_provider(provider_slot(kind)).await?,
                    })
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Providers(result));
            });
        }

        fn load_library(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = (|| {
                    let preferences = backend.get_preferences();
                    let vocab_preset_store = backend.list_vocabulary_presets()?;
                    let vocab_presets = openless_core::resolve_vocab_presets(&vocab_preset_store);
                    Ok::<_, BackendError>(LibraryPanel {
                        vocabulary: backend.list_vocabulary()?,
                        correction_rules: backend.list_correction_rules()?,
                        style_packs: backend.list_style_packs(&preferences.active_style_pack_id)?,
                        vocab_preset_store,
                        vocab_presets,
                    })
                })()
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Library(result));
            });
        }

        fn load_marketplace(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let query = self.marketplace_query.trim().to_string();
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .marketplace
                    .list(openless_core::MarketplaceQuery {
                        query: (!query.is_empty()).then_some(query),
                        sort: Some("updated".to_string()),
                        limit: Some(100),
                    })
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Marketplace(result));
            });
        }

        fn load_marketplace_mine(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = async {
                    let packs = backend.services().marketplace.my_packs().await?;
                    let likes = backend.services().marketplace.my_likes().await?;
                    Ok::<_, BackendError>((packs, likes))
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::MarketplaceMine(result));
            });
        }

        fn load_microphones(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .platform
                    .microphone_devices()
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Microphones(result));
            });
        }

        fn request_update_check(&mut self, channel: openless_core::shared_types::UpdateChannel) {
            let LinuxUpdateSupport::AppImage(updater) = self.update_support.clone() else {
                self.status = "当前安装包由系统包管理器更新".to_string();
                return;
            };
            if self.update_busy {
                return;
            }
            self.update_busy = true;
            self.update_progress = None;
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = updater
                    .check(channel)
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::UpdateCheck(result));
            });
        }

        fn install_update(&mut self) {
            let (LinuxUpdateSupport::AppImage(updater), Some(manifest)) =
                (self.update_support.clone(), self.update_manifest.clone())
            else {
                return;
            };
            if self.update_busy {
                return;
            }
            self.update_busy = true;
            self.update_progress = Some(openless_linux_egui::DownloadProgress {
                downloaded: 0,
                content_length: None,
            });
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let progress_tx = tx.clone();
                let result = updater
                    .download_and_install(manifest, move |progress| {
                        let _ = progress_tx.send(UiResult::UpdateProgress(progress));
                    })
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::UpdateInstalled(result));
            });
        }

        fn drain_tray(&mut self, ctx: &egui::Context) {
            let mut commands = Vec::new();
            if let Some(tray) = &self.tray {
                tray.drain(|command| commands.push(command));
                if let Some(error) = tray.take_error() {
                    self.status = format!("系统托盘已停止：{error}");
                    self.tray = None;
                }
            }
            for command in commands {
                match command {
                    openless_linux_egui::TrayCommand::ShowMain => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    openless_linux_egui::TrayCommand::ActivatePreviousStyle => {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                let pack = backend.activate_previous_style_pack()?;
                                Ok(pack
                                    .map(|pack| format!("已切换风格：{}", pack.name))
                                    .unwrap_or_else(|| "没有可切换的上一风格".to_string()))
                            });
                        }
                    }
                    openless_linux_egui::TrayCommand::SelectMicrophone(name) => {
                        if let Some(backend) = self.backend() {
                            let selected = if name.is_empty() {
                                "系统默认".to_string()
                            } else {
                                name.clone()
                            };
                            self.spawn(async move {
                                backend.select_microphone_device(name)?;
                                Ok(format!("已选择麦克风：{selected}"))
                            });
                        }
                    }
                    openless_linux_egui::TrayCommand::Quit => {
                        self.exit_requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }

        fn load_provider_editor(
            &self,
            kind: openless_core::ChannelKind,
            channel: openless_core::ChannelSummary,
            descriptor: openless_core::ProviderDescriptor,
        ) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            let channel_id = channel.id.clone();
            self.tokio.spawn(async move {
                let result = load_provider_editor(backend, kind, channel, descriptor)
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::ProviderEditor {
                    kind,
                    channel_id,
                    result: Box::new(result),
                });
            });
        }

        fn spawn_provider_mutation<F>(&self, future: F)
        where
            F: Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let _ = tx.send(UiResult::ProviderMutation(
                    future.await.map_err(|error| error.to_string()),
                ));
            });
        }

        fn spawn_model_mutation<F>(&self, future: F)
        where
            F: Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = future.await.map_err(|error| error.to_string());
                let _ = tx.send(UiResult::ModelMutation(result));
            });
        }

        fn request_provider_models(&self, kind: openless_core::ChannelKind, channel_id: String) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .provider
                    .list_models(openless_core::ProviderRequest {
                        kind: provider_kind(kind),
                        channel_id: Some(channel_id.clone()),
                    })
                    .await
                    .map(|models| models.models)
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::ProviderModels {
                    kind,
                    channel_id,
                    result,
                });
            });
        }

        fn apply_event(&mut self, event: BackendEvent) {
            if event.sequence <= self.last_event_sequence {
                return;
            }
            let event_sequence = event.sequence;
            self.last_event_sequence = event.sequence;
            let session_id = event.session_id;
            match event.kind {
                BackendEventKind::DictationStateChanged(state) => {
                    if state.phase == DictationPhase::Starting {
                        self.transcript_state = TranscriptAccumulator::default();
                        self.transcript.clear();
                        self.transcript_session = state.session_id;
                    }
                    self.status = format!("听写：{:?}", state.phase);
                    if let Some(session_id) = state.session_id {
                        self.send_popup(
                            PopupKind::Capsule,
                            HostToPopup::Capsule {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: session_id.to_string(),
                                sequence: event_sequence.saturating_mul(2),
                                phase: format!("{:?}", state.phase),
                                text: state.message.unwrap_or_default(),
                                audio_level: Some(state.level),
                            },
                        );
                    }
                }
                BackendEventKind::TranscriptDelta(delta)
                    if session_id == self.transcript_session
                        && self.transcript_state.apply(&delta).is_ok() =>
                {
                    self.transcript = self.transcript_state.text().to_string();
                }
                BackendEventKind::PolishDelta(delta) if delta.is_final => {
                    self.transcript = delta.text;
                }
                BackendEventKind::DictationCompleted(result) => {
                    self.transcript = result.polished_text;
                    self.status = format!("听写完成：{:?}", result.inserted);
                }
                BackendEventKind::RecordingControlRequested(request) => {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            match request.action {
                                openless_core::RecordingControlAction::Stop => {
                                    backend.stop_dictation_session(request.session_id).await?;
                                }
                                openless_core::RecordingControlAction::Cancel => {
                                    backend.cancel_dictation(Some(request.session_id)).await?;
                                }
                            }
                            Ok("录音已自动结束".to_string())
                        });
                    }
                }
                BackendEventKind::LessComputerEvent(event) => {
                    // Less Computer events may complete after a newer turn has
                    // already started. Session ownership, not arrival time,
                    // decides whether a delta/terminal may mutate this view.
                    if let LessComputerEventKind::User { text, fresh } = &event.kind {
                        // Every User starts a new turn UUID, including a
                        // continuation. `fresh` describes conversation history,
                        // never whether this turn is allowed to receive output.
                        self.less_computer_session = session_id;
                        self.pending_approval = None;
                        if *fresh {
                            self.less_computer_output.clear();
                        } else if !self.less_computer_output.is_empty() {
                            self.less_computer_output.push_str("\n\n");
                        }
                        self.less_computer_turn_start = self.less_computer_output.len();
                        self.less_computer_input = text.clone();
                    } else if session_id != self.less_computer_session {
                        return;
                    }
                    match event.kind {
                        // Linux已有独立录音显示；新typed反馈供接手Host/UI团队继续接入。
                        LessComputerEventKind::VoiceState { .. } => {}
                        LessComputerEventKind::User { .. } => {}
                        LessComputerEventKind::Started => {
                            self.status = "Less Computer 正在运行".to_string();
                        }
                        LessComputerEventKind::Delta { text } => {
                            self.less_computer_output.push_str(&text);
                        }
                        LessComputerEventKind::Tool { name } => {
                            self.status = format!("Less Computer 正在使用工具：{name}");
                        }
                        LessComputerEventKind::Compaction => {
                            self.status = "Less Computer 已压缩上下文".to_string();
                        }
                        LessComputerEventKind::Completed { text, .. } => {
                            // A terminal is authoritative even for final-only
                            // providers or after a missed partial event.
                            self.less_computer_output
                                .truncate(self.less_computer_turn_start);
                            self.less_computer_output.push_str(&text);
                            self.pending_approval = None;
                            self.status = "Less Computer 已完成".to_string();
                        }
                        LessComputerEventKind::Approval { token, command, .. } => {
                            self.pending_approval = Some((token, command));
                            self.status = "Less Computer 等待审批".to_string();
                        }
                        LessComputerEventKind::Error { message } => {
                            self.pending_approval = None;
                            self.status = message;
                        }
                        LessComputerEventKind::Cancelled => {
                            self.pending_approval = None;
                            self.status = "Less Computer 已取消".to_string();
                        }
                    }
                }
                BackendEventKind::LocalAsrDownloadProgress(progress) => {
                    self.status = format!(
                        "模型 {}：{:?} {}/{}",
                        progress.model_id,
                        progress.phase,
                        progress.bytes_downloaded,
                        progress.bytes_total
                    );
                    if matches!(
                        progress.phase,
                        openless_core::LocalAsrDownloadPhase::Finished
                            | openless_core::LocalAsrDownloadPhase::Failed
                            | openless_core::LocalAsrDownloadPhase::Cancelled
                    ) {
                        self.models = ModelsState::Loading;
                        self.load_models();
                    }
                }
                BackendEventKind::PreferencesChanged(_) => {
                    if let Some(backend) = self.backend() {
                        let latest = backend.get_preferences();
                        self.preferences = Some(match self.preferences.as_ref() {
                            Some(draft) if self.settings_dirty.any() => {
                                self.settings_dirty.merge(&latest, draft)
                            }
                            _ => latest,
                        });
                    }
                    self.load_remote_status();
                    self.load_library();
                }
                BackendEventKind::VocabularyChanged(_) | BackendEventKind::StylePacksChanged(_) => {
                    self.load_library()
                }
                BackendEventKind::QaState(state) => {
                    if state.kind == QaStateKind::AnswerDelta {
                        if let Some(current) = self
                            .qa_state
                            .as_mut()
                            .filter(|current| current.session_id == state.session_id)
                        {
                            // Core deltas deliberately omit messages. Preserve
                            // the conversation and append only this turn's text;
                            // the following Answer replaces it with Core history.
                            current.kind = state.kind;
                            current
                                .chunk
                                .get_or_insert_with(String::new)
                                .push_str(state.chunk.as_deref().unwrap_or_default());
                        }
                    } else if matches!(
                        state.kind,
                        QaStateKind::Idle
                            | QaStateKind::Loading
                            | QaStateKind::Thinking
                            | QaStateKind::Recording
                    ) || self
                        .qa_state
                        .as_ref()
                        .is_none_or(|current| current.session_id == state.session_id)
                    {
                        self.qa_state = Some(state);
                    }
                    if let Some(state) = self.qa_state.clone() {
                        let session_id =
                            state.session_id.clone().unwrap_or_else(|| "qa".to_string());
                        self.send_popup(
                            PopupKind::Qa,
                            HostToPopup::QaSnapshot {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id,
                                sequence: event_sequence.saturating_mul(2),
                                phase: format!("{:?}", state.kind),
                                messages: state
                                    .messages
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|message| PopupChatMessage {
                                        role: message.role,
                                        content: message.content,
                                        selection_text: message.selection_text,
                                    })
                                    .collect(),
                                selection_preview: state.selection_preview,
                                streaming_answer: state.chunk.unwrap_or_default(),
                                error: state.error,
                            },
                        );
                    }
                }
                BackendEventKind::SelectionStateChanged(snapshot) => {
                    if snapshot.phase == SelectionPhase::Preview {
                        self.selection_draft = snapshot.preview_text.clone().unwrap_or_default();
                        self.selection_preview_visible = true;
                    }
                    if let Some(session_id) = snapshot.session_id {
                        self.send_popup(
                            PopupKind::Preview,
                            HostToPopup::Preview {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: session_id.to_string(),
                                sequence: event_sequence.saturating_mul(2),
                                text: snapshot.preview_text.clone().unwrap_or_default(),
                                source: snapshot.source_text.clone().unwrap_or_default(),
                            },
                        );
                    }
                    self.selection = Some(snapshot);
                }
                BackendEventKind::RemoteInputStatusChanged(_)
                | BackendEventKind::RemoteInputFailed(_) => self.load_remote_status(),
                _ => {}
            }
        }

        fn poll(&mut self, ctx: &egui::Context) {
            if let Some(native) = &self.native {
                let (launch_intents, hotkey_events, errors) = native.drain_native_events();
                let host = native.host_arc();
                for intent in launch_intents {
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_launch_intent(intent).await?;
                        Ok("已处理启动请求".to_string())
                    });
                }
                for event in hotkey_events {
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_hotkey_event(event).await?;
                        Ok("已处理快捷键".to_string())
                    });
                }
                if let Some(error) = errors.last() {
                    self.status = error.to_string();
                }

                let mut actions = Vec::new();
                native.host_actions().drain(|action| actions.push(action));
                // HostAction controls only native visibility/focus/effects.
                // QA and Selection contents and terminal ownership always come
                // back through sequenced Core events handled above.
                for action in actions {
                    match action {
                        HostAction::ShowMain | HostAction::ShowLessComputer => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        HostAction::FocusMain => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                        HostAction::Notify(message) => {
                            self.status = message.clone();
                            std::thread::spawn(move || {
                                if let Err(error) = notify(Notification {
                                    summary: "OpenLess",
                                    body: &message,
                                    icon: "openless",
                                    timeout_ms: 0,
                                }) {
                                    eprintln!("OpenLess desktop notification failed: {error}");
                                }
                            });
                        }
                        HostAction::OpenExternalUrl(url) | HostAction::OpenSystemSettings(url) => {
                            std::thread::spawn(move || {
                                if let Err(error) = open_external(&url) {
                                    eprintln!("OpenLess external URL failed: {error}");
                                }
                            });
                        }
                        HostAction::RequestRestart => {
                            self.status = "请手动重启 OpenLess".to_string();
                        }
                        HostAction::ShowSelectionPreview => {
                            self.selection_preview_visible = true;
                            self.show_selection_popup();
                        }
                        HostAction::HideSelectionPreview => {
                            self.selection_preview_visible = false;
                            let session_id = self
                                .selection
                                .as_ref()
                                .and_then(|selection| selection.session_id)
                                .map(|id| id.to_string())
                                .unwrap_or_else(|| "selection".to_string());
                            self.hide_popup(
                                PopupKind::Preview,
                                session_id,
                                self.last_event_sequence.saturating_mul(2).saturating_add(1),
                            );
                        }
                        HostAction::ShowQa => {
                            self.qa_visible = true;
                            self.show_qa_popup();
                        }
                        HostAction::HideQa => {
                            self.qa_visible = false;
                            let session_id = self
                                .qa_state
                                .as_ref()
                                .and_then(|state| state.session_id.clone())
                                .unwrap_or_else(|| "qa".to_string());
                            self.hide_popup(
                                PopupKind::Qa,
                                session_id,
                                self.last_event_sequence.saturating_mul(2).saturating_add(1),
                            );
                        }
                        HostAction::ShowDictationFeedback => self.show_capsule_popup(),
                        HostAction::HideDictationFeedback => {
                            let session_id = self
                                .snapshot
                                .as_ref()
                                .and_then(|snapshot| snapshot.dictation.session_id)
                                .map(|id| id.to_string())
                                .unwrap_or_else(|| "dictation".to_string());
                            self.hide_popup(
                                PopupKind::Capsule,
                                session_id,
                                self.last_event_sequence.saturating_mul(2).saturating_add(1),
                            );
                        }
                    }
                }
            }
            self.poll_popup_supervisors();
            let mut events = Vec::new();
            let drain = self
                .subscription
                .as_mut()
                .map(|subscription| drain_events(subscription, |event| events.push(event)));
            for event in events {
                self.apply_event(event);
            }
            if let Some(EventDrainOutcome::Lagged { dropped, .. }) = drain {
                if let Some(backend) = self.backend() {
                    // Broadcast lag does not imply Core lost the events. Replay
                    // from the last applied sequence first; duplicate delivery
                    // from the live receiver is rejected by apply_event above.
                    let replay = backend.replay_events_after(self.last_event_sequence);
                    let snapshot = backend.snapshot();
                    if replay.truncated {
                        // The bounded tail cannot reconstruct derived text/UI
                        // state. Reset it before applying the authoritative tail
                        // so no stale transcript, approval or preview survives.
                        self.transcript_state = TranscriptAccumulator::default();
                        self.transcript.clear();
                        self.transcript_session = snapshot.dictation.session_id;
                        self.less_computer_input.clear();
                        self.less_computer_output.clear();
                        self.less_computer_turn_start = 0;
                        self.less_computer_session = None;
                        self.pending_approval = None;
                        self.qa_state = None;
                        self.qa_visible = false;
                        self.selection = None;
                        self.selection_draft.clear();
                        self.selection_preview_visible = false;
                    }
                    self.snapshot = Some(snapshot);
                    for event in replay.events {
                        self.apply_event(event);
                    }
                    self.status = if replay.truncated {
                        format!("事件积压 {dropped} 条，已重置派生界面并重放可用事件")
                    } else {
                        format!("事件积压 {dropped} 条，已从 Core 重放补齐")
                    };
                }
            }
            while let Ok(result) = self.rx.try_recv() {
                match result {
                    UiResult::Message(message) => self.status = message,
                    UiResult::Models(Ok(models)) => self.models = ModelsState::Loaded(models),
                    UiResult::Models(Err(error)) => {
                        self.models = ModelsState::Failed(error.clone());
                        self.status = error;
                    }
                    UiResult::Remote(Ok(remote)) => self.remote_access = Some(remote),
                    UiResult::Remote(Err(error)) => self.status = error,
                    UiResult::Providers(Ok(panel)) => {
                        if panel.kind != self.provider_kind {
                            continue;
                        }
                        if !panel.descriptors.iter().any(|descriptor| {
                            descriptor.provider_type.as_str() == self.new_provider_type
                        }) {
                            self.new_provider_type = panel
                                .descriptors
                                .first()
                                .map(|descriptor| descriptor.provider_type.as_str().to_string())
                                .unwrap_or_default();
                        }
                        let selected = self
                            .selected_channel_id
                            .as_ref()
                            .filter(|id| panel.channels.iter().any(|channel| &channel.id == *id))
                            .cloned()
                            .or_else(|| {
                                panel
                                    .channels
                                    .iter()
                                    .find(|channel| channel.id == panel.active_provider)
                                    .map(|channel| channel.id.clone())
                            })
                            .or_else(|| panel.channels.first().map(|channel| channel.id.clone()));
                        self.selected_channel_id = selected.clone();
                        if self.pending_channel_delete.as_ref().is_some_and(|id| {
                            !panel.channels.iter().any(|channel| &channel.id == id)
                        }) {
                            self.pending_channel_delete = None;
                        }
                        self.providers = ProvidersState::Loaded(panel.clone());
                        self.provider_models.clear();
                        if let Some(channel_id) = selected {
                            if let Some((channel, descriptor)) =
                                provider_channel_descriptor(&panel, &channel_id)
                            {
                                self.provider_editor = ProviderEditorState::Loading {
                                    kind: panel.kind,
                                    channel_id,
                                };
                                self.load_provider_editor(panel.kind, channel, descriptor);
                            }
                        } else {
                            self.provider_editor = ProviderEditorState::Idle;
                        }
                    }
                    UiResult::Providers(Err(error)) => {
                        self.providers = ProvidersState::Failed(error.clone());
                        self.status = error;
                    }
                    UiResult::ProviderEditor {
                        kind,
                        channel_id,
                        result,
                    } => {
                        if kind != self.provider_kind
                            || self.selected_channel_id.as_deref() != Some(channel_id.as_str())
                        {
                            continue;
                        }
                        match *result {
                            Ok(editor) => {
                                // Reads race with channel switching and mutation
                                // refreshes. Only the still-selected channel may install
                                // its editor, otherwise late credential data is ignored.
                                self.provider_editor =
                                    ProviderEditorState::Loaded(Box::new(editor));
                            }
                            Err(error) => {
                                self.provider_editor = ProviderEditorState::Failed(error.clone());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::ProviderModels {
                        kind,
                        channel_id,
                        result,
                    } => {
                        if kind == self.provider_kind
                            && self.selected_channel_id.as_deref() == Some(channel_id.as_str())
                        {
                            match result {
                                Ok(models) => {
                                    self.status = format!("已读取 {} 个模型", models.len());
                                    self.provider_models = models;
                                }
                                Err(error) => self.status = error,
                            }
                        }
                    }
                    UiResult::ProviderMutation(result) => {
                        match result {
                            Ok(message) => self.status = message,
                            Err(error) => self.status = error,
                        }
                        self.providers = ProvidersState::Loading;
                        self.provider_editor = ProviderEditorState::Idle;
                        self.provider_models.clear();
                        self.load_providers(self.provider_kind);
                    }
                    UiResult::Library(Ok(library)) => {
                        self.vocabulary = library.vocabulary;
                        self.correction_rules = library.correction_rules;
                        self.style_packs = library.style_packs;
                        self.vocab_preset_store = library.vocab_preset_store;
                        self.vocab_presets = library.vocab_presets;
                    }
                    UiResult::Library(Err(error)) => self.status = error,
                    UiResult::SettingsSaved(result) => match *result {
                        Ok(outcome) => {
                            self.preferences = Some(outcome.preferences.clone());
                            if let Some(native) = &self.native {
                                self.snapshot = Some(native.host().snapshot());
                            }
                            self.settings_dirty = SettingsDirty::default();
                            self.status = "设置已保存".to_string();
                            if let Some(backend) = self.backend() {
                                let config = openless_core::RemoteInputConfig {
                                    enabled: outcome.preferences.remote_input_enabled,
                                    port: outcome.preferences.remote_input_port,
                                };
                                self.spawn(async move {
                                    backend.services().remote_input.configure(config).await?;
                                    Ok("远程输入状态已更新".to_string())
                                });
                            }
                        }
                        Err(error) => self.status = error,
                    },
                    UiResult::Marketplace(Ok(items)) => {
                        self.status = format!("Marketplace 已加载 {} 个风格包", items.len());
                        self.marketplace_items = items;
                    }
                    UiResult::Marketplace(Err(error)) => self.status = error,
                    UiResult::MarketplaceFlow(Ok(flow)) => {
                        self.status = format!("GitHub 设备码：{}", flow.user_code);
                        self.marketplace_flow = Some(flow);
                    }
                    UiResult::MarketplaceFlow(Err(error)) => self.status = error,
                    UiResult::MarketplaceAuthPoll(Ok(result)) => match result {
                        openless_core::OAuthPollResult::Authorized { login } => {
                            self.marketplace_flow = None;
                            self.status = format!("Marketplace 已登录：{login}");
                        }
                        openless_core::OAuthPollResult::Pending => {
                            self.status = "GitHub 授权仍在等待".to_string();
                        }
                        openless_core::OAuthPollResult::SlowDown => {
                            self.status = "GitHub 要求降低检查频率".to_string();
                        }
                        openless_core::OAuthPollResult::Error { message } => {
                            self.status = message;
                        }
                    },
                    UiResult::MarketplaceAuthPoll(Err(error)) => self.status = error,
                    UiResult::MarketplaceDetail(Ok(detail)) => {
                        self.status = format!("已加载风格详情：{}", detail.summary.name);
                        self.marketplace_detail = Some(detail);
                    }
                    UiResult::MarketplaceDetail(Err(error)) => self.status = error,
                    UiResult::MarketplaceMine(Ok((packs, likes))) => {
                        self.status =
                            format!("我的发布 {} 个，喜欢 {} 个", packs.len(), likes.len());
                        self.marketplace_my_packs = packs;
                        self.marketplace_my_likes = likes;
                    }
                    UiResult::MarketplaceMine(Err(error)) => self.status = error,
                    UiResult::Microphones(Ok(devices)) => {
                        self.microphones = devices.clone();
                        let selected = self
                            .preferences
                            .as_ref()
                            .map(|prefs| prefs.microphone_device_name.as_str())
                            .unwrap_or_default();
                        if let Some(tray) = &self.tray {
                            let microphones = devices
                                .into_iter()
                                .map(|device| openless_linux_egui::TrayMicrophone {
                                    selected: !selected.is_empty()
                                        && (selected == device.id || selected == device.name),
                                    name: device.name,
                                    is_default: device.is_default,
                                })
                                .collect();
                            if let Err(error) = tray.set_microphones(microphones) {
                                self.status = error.to_string();
                            }
                        }
                    }
                    UiResult::Microphones(Err(error)) => self.status = error,
                    UiResult::UpdateCheck(Ok(Some(manifest))) => {
                        self.update_busy = false;
                        self.status = format!("发现新版本 {}", manifest.version);
                        self.update_manifest = Some(manifest);
                    }
                    UiResult::UpdateCheck(Ok(None)) => {
                        self.update_busy = false;
                        self.status = "当前已是最新版本".to_string();
                    }
                    UiResult::UpdateCheck(Err(error)) => {
                        self.update_busy = false;
                        self.status = format!("检查更新失败：{error}");
                    }
                    UiResult::UpdateProgress(progress) => self.update_progress = Some(progress),
                    UiResult::UpdateInstalled(Ok(installed)) => {
                        self.update_busy = false;
                        self.update_manifest = None;
                        self.status = format!("已安装 {}，请重启 OpenLess", installed.version);
                    }
                    UiResult::UpdateInstalled(Err(error)) => {
                        self.update_busy = false;
                        self.status = format!("安装更新失败：{error}");
                    }
                    UiResult::ModelMutation(result) => {
                        self.status = result.unwrap_or_else(|error| error);
                        self.models = ModelsState::Loading;
                        self.load_models();
                    }
                }
            }
            if let Some(backend) = self.backend() {
                self.snapshot = Some(backend.snapshot());
            }
        }

        fn dictation_ui(&mut self, ui: &mut egui::Ui) {
            ui.heading("听写");
            let phase = self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.dictation.phase)
                .unwrap_or(DictationPhase::Idle);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(phase == DictationPhase::Idle, egui::Button::new("开始"))
                    .clicked()
                {
                    if let Some(backend) = self.backend() {
                        self.transcript.clear();
                        self.spawn(async move {
                            backend.start_dictation().await?;
                            Ok("正在录音".to_string())
                        });
                    }
                }
                if ui
                    .add_enabled(
                        phase == DictationPhase::Recording,
                        egui::Button::new("停止"),
                    )
                    .clicked()
                {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            let result = backend.stop_dictation().await?;
                            Ok(format!("完成：{} 字", result.polished_text.chars().count()))
                        });
                    }
                }
                if ui
                    .add_enabled(phase != DictationPhase::Idle, egui::Button::new("取消"))
                    .clicked()
                {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.cancel_dictation(None).await?;
                            Ok("听写已取消".to_string())
                        });
                    }
                }
            });
            ui.label(if self.transcript.is_empty() {
                "尚无转写结果"
            } else {
                &self.transcript
            });
        }

        fn less_computer_ui(&mut self, ui: &mut egui::Ui) {
            ui.heading("Less Computer");
            ui.text_edit_multiline(&mut self.less_computer_input);
            ui.horizontal(|ui| {
                if ui.button("运行").clicked() && !self.less_computer_input.trim().is_empty() {
                    if let Some(backend) = self.backend() {
                        let prompt = self.less_computer_input.clone();
                        self.less_computer_output.clear();
                        self.spawn(async move {
                            backend.submit_less_computer(prompt).await?;
                            Ok("Less Computer 已完成".to_string())
                        });
                    }
                }
                if ui.button("取消").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.cancel_less_computer(None).await?;
                            Ok("Less Computer 已取消".to_string())
                        });
                    }
                }
            });
            if let Some((token, command)) = self.pending_approval.clone() {
                ui.label(format!("请求执行：{command}"));
                ui.horizontal(|ui| {
                    for (label, approved) in [("允许", true), ("拒绝", false)] {
                        if ui.button(label).clicked() {
                            if let Some(backend) = self.backend() {
                                let token = token.clone();
                                self.pending_approval = None;
                                self.spawn(async move {
                                    backend
                                        .services()
                                        .less_computer
                                        .approve(token, approved)
                                        .await?;
                                    Ok("审批已提交".to_string())
                                });
                            }
                        }
                    }
                });
            }
            ui.label(if self.less_computer_output.is_empty() {
                "尚无 Agent 输出"
            } else {
                &self.less_computer_output
            });
        }

        fn qa_ui(&mut self, ui: &mut egui::Ui) {
            if !self.qa_visible {
                return;
            }
            ui.heading("问答");
            if let Some(state) = &self.qa_state {
                if let Some(messages) = &state.messages {
                    for message in messages {
                        ui.label(format!("{}：{}", message.role, message.content));
                    }
                }
                if let Some(chunk) = &state.chunk {
                    ui.label(chunk);
                }
                if let Some(error) = &state.error {
                    ui.colored_label(egui::Color32::RED, error);
                }
            }
            ui.text_edit_multiline(&mut self.qa_input);
            ui.horizontal(|ui| {
                let recording = self
                    .qa_state
                    .as_ref()
                    .is_some_and(|state| state.kind == QaStateKind::Recording);
                if ui
                    .button(if recording {
                        "结束录音"
                    } else {
                        "语音提问"
                    })
                    .clicked()
                {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.services().qa.toggle_recording().await?;
                            Ok("问答录音状态已更新".to_string())
                        });
                    }
                }
                if ui.button("发送").clicked() && !self.qa_input.trim().is_empty() {
                    if let Some(backend) = self.backend() {
                        let text = std::mem::take(&mut self.qa_input);
                        self.spawn(async move {
                            backend.services().qa.submit_text(text).await?;
                            Ok("问答已提交".to_string())
                        });
                    }
                }
                if ui.button("关闭").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.services().qa.dismiss().await?;
                            Ok("问答已关闭".to_string())
                        });
                    }
                }
            });
        }

        fn selection_ui(&mut self, ui: &mut egui::Ui) {
            let Some(selection) = self.selection.clone() else {
                return;
            };
            if self.selection_preview_visible && selection.phase == SelectionPhase::Preview {
                ui.heading("选区预览");
                ui.text_edit_multiline(&mut self.selection_draft);
                ui.horizontal(|ui| {
                    if ui.button("确认替换").clicked() {
                        if let (Some(backend), Some(session_id)) =
                            (self.backend(), selection.session_id)
                        {
                            let text = self.selection_draft.clone();
                            self.spawn(async move {
                                backend
                                    .services()
                                    .selection
                                    .confirm(session_id, Some(text))
                                    .await?;
                                Ok("选区替换已确认".to_string())
                            });
                        }
                    }
                    if ui.button("取消").clicked() {
                        if let (Some(backend), Some(session_id)) =
                            (self.backend(), selection.session_id)
                        {
                            self.spawn(async move {
                                backend
                                    .services()
                                    .selection
                                    .cancel(Some(session_id))
                                    .await?;
                                Ok("选区替换已取消".to_string())
                            });
                        }
                    }
                });
            } else if selection.phase == SelectionPhase::Completed
                && selection.revert_outcome.is_none()
            {
                ui.horizontal(|ui| {
                    ui.label("最近一次选区替换已完成");
                    if ui.button("撤销").clicked() {
                        if let (Some(backend), Some(session_id)) =
                            (self.backend(), selection.session_id)
                        {
                            self.spawn(async move {
                                backend.services().selection.revert(session_id).await?;
                                Ok("选区替换已撤销".to_string())
                            });
                        }
                    }
                });
            }
        }

        fn models_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.heading("本地模型");
                if ui.button("刷新").clicked() {
                    self.models = ModelsState::Loading;
                    self.load_models();
                }
                if ui.button("预加载当前模型").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn_model_mutation(async move {
                            backend
                                .services()
                                .local_asr
                                .preload(LocalAsrRuntime::Generic)
                                .await?;
                            Ok("当前模型已预加载".to_string())
                        });
                    }
                }
                if ui.button("释放模型").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn_model_mutation(async move {
                            backend
                                .services()
                                .local_asr
                                .release(LocalAsrRuntime::Generic)
                                .await?;
                            Ok("模型已释放".to_string())
                        });
                    }
                }
                if ui.button("取消准备").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn_model_mutation(async move {
                            backend
                                .services()
                                .local_asr
                                .cancel_prepare(LocalAsrRuntime::Generic)
                                .await?;
                            Ok("已请求取消模型准备".to_string())
                        });
                    }
                }
            });
            let models = match self.models.clone() {
                ModelsState::Loading => {
                    ui.label("正在加载模型目录…");
                    return;
                }
                ModelsState::Failed(error) => {
                    ui.colored_label(egui::Color32::RED, error);
                    return;
                }
                ModelsState::Loaded(models) if models.is_empty() => {
                    ui.label("模型目录未返回任何可用模型");
                    return;
                }
                ModelsState::Loaded(models) => models,
            };
            let mut action: Option<(openless_core::LocalAsrTarget, &'static str)> = None;
            for model in models {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} · {} · {}",
                        model.display_name,
                        model.family,
                        if model.installed {
                            "已安装"
                        } else {
                            "未安装"
                        }
                    ));
                    if !model.installed && ui.button("下载").clicked() {
                        action = Some((model.target.clone(), "download"));
                    }
                    if !model.installed && ui.button("取消下载").clicked() {
                        action = Some((model.target.clone(), "cancel_download"));
                    }
                    if model.installed && ui.button("激活").clicked() {
                        if let Some(backend) = self.backend() {
                            let target = model.target.clone();
                            self.spawn(async move {
                                let descriptor =
                                    openless_core::provider_rules::provider_descriptor(
                                        openless_core::ProviderKind::Asr,
                                        "local-qwen3-c",
                                    )
                                    .ok_or_else(|| {
                                        openless_core::BackendError::new(
                                            openless_core::BackendErrorCode::Unsupported,
                                            "local Qwen provider is unavailable",
                                        )
                                    })?;
                                let provider_type = descriptor.provider_type.as_str().to_string();
                                let existing = backend
                                    .list_channels(openless_core::ChannelKind::Asr)
                                    .await?
                                    .into_iter()
                                    .find(|channel| channel.provider_type == provider_type)
                                    .map(|channel| channel.id);
                                let provider_id = match existing {
                                    Some(provider_id) => provider_id,
                                    None => {
                                        backend
                                            .create_channel(
                                                openless_core::ChannelKind::Asr,
                                                provider_type,
                                                descriptor.label_key,
                                            )
                                            .await?
                                    }
                                };
                                backend
                                    .activate_local_asr(openless_core::LocalAsrActivationRequest {
                                        target,
                                        provider_id,
                                    })
                                    .await?;
                                Ok("本地模型已激活并预加载".to_string())
                            });
                        }
                    }
                    if ui.button("取消").clicked() {
                        if let Some(backend) = self.backend() {
                            let target = model.target.clone();
                            self.spawn(async move {
                                backend.services().local_asr.cancel_download(target).await?;
                                Ok("模型下载已取消".to_string())
                            });
                        }
                    }
                    if model.installed && ui.button("验证/准备").clicked() {
                        action = Some((model.target.clone(), "prepare"));
                    }
                    if model.installed && ui.button("测试").clicked() {
                        action = Some((model.target.clone(), "test"));
                    }
                    if model.installed && ui.button("删除").clicked() {
                        action = Some((model.target.clone(), "delete"));
                    }
                });
            }
            if let (Some(backend), Some((target, operation))) = (self.backend(), action) {
                self.spawn_model_mutation(async move {
                    match operation {
                        "download" => {
                            backend
                                .services()
                                .local_asr
                                .start_download(target, None)
                                .await?;
                            Ok("模型下载完成".to_string())
                        }
                        "cancel_download" => {
                            backend.services().local_asr.cancel_download(target).await?;
                            Ok("已请求取消模型下载".to_string())
                        }
                        "prepare" => {
                            let prepared = backend.services().local_asr.prepare(target).await?;
                            Ok(format!("模型验证完成：{prepared}"))
                        }
                        "test" => {
                            let result = backend.services().local_asr.test_model(target).await?;
                            Ok(format!(
                                "模型测试完成：{}（{} ms）",
                                result.transcribed_text, result.transcribe_ms
                            ))
                        }
                        "delete" => {
                            backend.services().local_asr.delete_model(target).await?;
                            Ok("模型已删除".to_string())
                        }
                        _ => unreachable!(),
                    }
                });
            }
        }

        fn provider_management_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.strong("凭据渠道");
                for (kind, label) in [
                    (openless_core::ChannelKind::Asr, "ASR"),
                    (openless_core::ChannelKind::Llm, "LLM"),
                ] {
                    if ui
                        .selectable_label(self.provider_kind == kind, label)
                        .clicked()
                        && self.provider_kind != kind
                    {
                        self.provider_kind = kind;
                        self.providers = ProvidersState::Loading;
                        self.selected_channel_id = None;
                        self.pending_channel_delete = None;
                        self.provider_editor = ProviderEditorState::Idle;
                        self.provider_models.clear();
                        self.load_providers(kind);
                    }
                }
                if ui.button("刷新渠道").clicked() {
                    self.providers = ProvidersState::Loading;
                    self.load_providers(self.provider_kind);
                }
            });

            let panel = match self.providers.clone() {
                ProvidersState::Loading => {
                    ui.label("正在读取 Core 渠道目录…");
                    return;
                }
                ProvidersState::Failed(error) => {
                    ui.colored_label(egui::Color32::RED, error);
                    return;
                }
                ProvidersState::Loaded(panel) => panel,
            };

            ui.group(|ui| {
                ui.label("新增渠道");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("new-provider-type")
                        .selected_text(
                            panel
                                .descriptors
                                .iter()
                                .find(|item| item.provider_type.as_str() == self.new_provider_type)
                                .map(provider_descriptor_label)
                                .unwrap_or_else(|| "选择 Provider".to_string()),
                        )
                        .show_ui(ui, |ui| {
                            for descriptor in &panel.descriptors {
                                ui.selectable_value(
                                    &mut self.new_provider_type,
                                    descriptor.provider_type.as_str().to_string(),
                                    provider_descriptor_label(descriptor),
                                );
                            }
                        });
                    ui.text_edit_singleline(&mut self.new_channel_name);
                    if ui
                        .add_enabled(
                            !self.new_provider_type.is_empty(),
                            egui::Button::new("创建"),
                        )
                        .clicked()
                    {
                        if let (Some(backend), Some(descriptor)) = (
                            self.backend(),
                            panel
                                .descriptors
                                .iter()
                                .find(|item| item.provider_type.as_str() == self.new_provider_type),
                        ) {
                            let kind = panel.kind;
                            let provider_type = descriptor.provider_type.as_str().to_string();
                            let name = if self.new_channel_name.trim().is_empty() {
                                descriptor.label_key.clone()
                            } else {
                                self.new_channel_name.trim().to_string()
                            };
                            self.new_channel_name.clear();
                            self.spawn_provider_mutation(async move {
                                backend.create_channel(kind, provider_type, name).await?;
                                Ok("渠道已创建".to_string())
                            });
                        }
                    }
                });
                ui.small("Provider 类型、默认 Endpoint/Model 与鉴权要求均来自 Core descriptor。");
            });

            if panel.channels.is_empty() {
                ui.label("尚无渠道；先从上方 Core Provider 列表创建一个。");
                return;
            }

            for (index, channel) in panel.channels.iter().enumerate() {
                let active = channel.id == panel.active_provider;
                ui.horizontal(|ui| {
                    let selected = self.selected_channel_id.as_deref() == Some(channel.id.as_str());
                    if ui
                        .selectable_label(
                            selected,
                            format!(
                                "{} · {}{}{}",
                                channel.name,
                                channel.provider_type,
                                if active { " · active" } else { "" },
                                if channel.enabled { "" } else { " · 已禁用" },
                            ),
                        )
                        .clicked()
                    {
                        self.selected_channel_id = Some(channel.id.clone());
                        self.provider_models.clear();
                        if let Some((channel, descriptor)) =
                            provider_channel_descriptor(&panel, &channel.id)
                        {
                            self.provider_editor = ProviderEditorState::Loading {
                                kind: panel.kind,
                                channel_id: channel.id.clone(),
                            };
                            self.load_provider_editor(panel.kind, channel, descriptor);
                        }
                    }
                    if !active && channel.enabled && ui.button("设为 active").clicked() {
                        if let Some(backend) = self.backend() {
                            let slot = provider_slot(panel.kind);
                            let channel_id = channel.id.clone();
                            self.spawn_provider_mutation(async move {
                                backend.set_active_provider(slot, channel_id).await?;
                                Ok("active 渠道已更新".to_string())
                            });
                        }
                    }
                    if ui
                        .button(if channel.enabled { "禁用" } else { "启用" })
                        .clicked()
                    {
                        if let Some(backend) = self.backend() {
                            let kind = panel.kind;
                            let channel_id = channel.id.clone();
                            let enabled = !channel.enabled;
                            self.spawn_provider_mutation(async move {
                                backend
                                    .set_channel_enabled(kind, channel_id, enabled)
                                    .await?;
                                Ok("渠道启用状态已更新".to_string())
                            });
                        }
                    }
                    if index > 0 && ui.button("上移").clicked() {
                        if let Some(backend) = self.backend() {
                            let kind = panel.kind;
                            let mut ids = panel
                                .channels
                                .iter()
                                .map(|item| item.id.clone())
                                .collect::<Vec<_>>();
                            ids.swap(index, index - 1);
                            self.spawn_provider_mutation(async move {
                                backend.reorder_channels(kind, ids).await?;
                                Ok("渠道顺序已更新".to_string())
                            });
                        }
                    }
                    if index + 1 < panel.channels.len() && ui.button("下移").clicked() {
                        if let Some(backend) = self.backend() {
                            let kind = panel.kind;
                            let mut ids = panel
                                .channels
                                .iter()
                                .map(|item| item.id.clone())
                                .collect::<Vec<_>>();
                            ids.swap(index, index + 1);
                            self.spawn_provider_mutation(async move {
                                backend.reorder_channels(kind, ids).await?;
                                Ok("渠道顺序已更新".to_string())
                            });
                        }
                    }
                    if self.pending_channel_delete.as_deref() == Some(channel.id.as_str()) {
                        if ui.button("确认删除").clicked() {
                            self.pending_channel_delete = None;
                            if let Some(backend) = self.backend() {
                                let kind = panel.kind;
                                let channel_id = channel.id.clone();
                                self.spawn_provider_mutation(async move {
                                    backend.delete_channel(kind, channel_id).await?;
                                    Ok("渠道已删除".to_string())
                                });
                            }
                        }
                        if ui.button("取消删除").clicked() {
                            self.pending_channel_delete = None;
                        }
                    } else if ui.button("删除").clicked() {
                        // Channel deletion may remove the last usable provider
                        // and its persisted secrets, so require a deliberate
                        // second click even in this intentionally compact UI.
                        self.pending_channel_delete = Some(channel.id.clone());
                    }
                });
            }

            match self.provider_editor.clone() {
                ProviderEditorState::Idle => {}
                ProviderEditorState::Loading { kind, channel_id } => {
                    ui.label(format!("正在读取 {:?} 渠道 {channel_id}…", kind));
                }
                ProviderEditorState::Failed(error) => {
                    ui.colored_label(egui::Color32::RED, error);
                }
                ProviderEditorState::Loaded(editor) => {
                    let mut editor = *editor;
                    ui.separator();
                    ui.strong(format!("编辑渠道 {}", editor.channel.id));
                    let mut provider_type = editor.descriptor.provider_type.as_str().to_string();
                    egui::ComboBox::from_id_salt("edit-provider-type")
                        .selected_text(provider_descriptor_label(&editor.descriptor))
                        .show_ui(ui, |ui| {
                            for descriptor in &panel.descriptors {
                                ui.selectable_value(
                                    &mut provider_type,
                                    descriptor.provider_type.as_str().to_string(),
                                    provider_descriptor_label(descriptor),
                                );
                            }
                        });
                    if provider_type != editor.descriptor.provider_type.as_str() {
                        if let Some(backend) = self.backend() {
                            let kind = editor.kind;
                            let channel_id = editor.channel.id.clone();
                            self.spawn_provider_mutation(async move {
                                backend
                                    .set_channel_provider_type(kind, channel_id, provider_type)
                                    .await?;
                                Ok("Provider 类型已更新".to_string())
                            });
                        }
                        return;
                    }

                    ui.label(format!(
                        "鉴权：{} · 探针：{:?}",
                        auth_requirement_label(editor.descriptor.auth_requirement),
                        editor.descriptor.validation_probe
                    ));
                    ui.horizontal(|ui| {
                        ui.label("名称");
                        ui.text_edit_singleline(&mut editor.name);
                    });
                    provider_fields_ui(ui, &mut editor);

                    ui.horizontal(|ui| {
                        if ui.button("保存字段/Secret").clicked() {
                            if let Some(backend) = self.backend() {
                                let saved = editor.clone();
                                self.spawn_provider_mutation(async move {
                                    save_provider_editor(backend, saved).await?;
                                    Ok("渠道配置已保存".to_string())
                                });
                            }
                        }
                        if ui.button("清除 Secret").clicked() {
                            if let Some(backend) = self.backend() {
                                let cleared = editor.clone();
                                self.spawn_provider_mutation(async move {
                                    clear_provider_secrets(backend, &cleared).await?;
                                    Ok("渠道 Secret 已清除".to_string())
                                });
                            }
                        }
                        if ui.button("验证连接").clicked() {
                            if let Some(backend) = self.backend() {
                                let kind = editor.kind;
                                let channel_id = editor.channel.id.clone();
                                self.spawn_provider_mutation(async move {
                                    validate_provider_channel(backend, kind, channel_id).await
                                });
                            }
                        }
                        if ui.button("列出模型").clicked() {
                            self.provider_models.clear();
                            self.request_provider_models(editor.kind, editor.channel.id.clone());
                        }
                    });
                    if !self.provider_models.is_empty() {
                        ui.label("模型列表（点击填入）：");
                        for model in self.provider_models.clone() {
                            if ui.button(&model).clicked() {
                                editor.model = model;
                            }
                        }
                    }
                    self.provider_editor = ProviderEditorState::Loaded(Box::new(editor));
                }
            }
        }

        fn settings_ui(&mut self, ui: &mut egui::Ui) {
            ui.heading("Provider 与设置");
            if let Some(snapshot) = &self.snapshot {
                let credentials = &snapshot.credentials;
                ui.label(format!(
                    "ASR：{}（{}）",
                    credentials.active_asr_provider,
                    if credentials.asr_configured {
                        "已配置"
                    } else {
                        "未配置"
                    }
                ));
                ui.label(format!(
                    "LLM：{}（{}）",
                    credentials.active_llm_provider,
                    if credentials.llm_configured {
                        "已配置"
                    } else {
                        "未配置"
                    }
                ));
            }
            self.provider_management_ui(ui);
            ui.separator();
            let mut save_settings = false;
            if let Some(preferences) = self.preferences.as_mut() {
                self.settings_dirty.streaming_insert |= ui
                    .checkbox(&mut preferences.streaming_insert, "流式插入")
                    .changed();
                self.settings_dirty.coding_agent_enabled |= ui
                    .checkbox(&mut preferences.coding_agent_enabled, "启用 Less Computer")
                    .changed();
                ui.collapsing("录音与输入", |ui| {
                    let previous_mode = preferences.hotkey.mode;
                    egui::ComboBox::from_label("录音方式")
                        .selected_text(match preferences.hotkey.mode {
                            openless_core::shared_types::HotkeyMode::Toggle => "切换",
                            openless_core::shared_types::HotkeyMode::Hold => "按住说话",
                            openless_core::shared_types::HotkeyMode::DoubleClick => "双击",
                            openless_core::shared_types::HotkeyMode::Auto => "自动识别",
                        })
                        .show_ui(ui, |ui| {
                            for (mode, label) in [
                                (openless_core::shared_types::HotkeyMode::Toggle, "切换"),
                                (openless_core::shared_types::HotkeyMode::Hold, "按住说话"),
                                (openless_core::shared_types::HotkeyMode::Auto, "自动识别"),
                            ] {
                                ui.selectable_value(&mut preferences.hotkey.mode, mode, label);
                            }
                        });
                    self.settings_dirty.recording |= preferences.hotkey.mode != previous_mode;
                    self.settings_dirty.recording |= ui
                        .checkbox(&mut preferences.silence_auto_stop_enabled, "说完后自动停止")
                        .changed();
                    if preferences.silence_auto_stop_enabled {
                        let previous = preferences.silence_auto_stop_seconds;
                        egui::ComboBox::from_label("连续静音时长")
                            .selected_text(format!("{} 秒", previous))
                            .show_ui(ui, |ui| {
                                for seconds in [1.0, 1.5, 2.0, 3.0, 4.0, 5.0] {
                                    ui.selectable_value(
                                        &mut preferences.silence_auto_stop_seconds,
                                        seconds,
                                        format!("{seconds} 秒"),
                                    );
                                }
                            });
                        self.settings_dirty.recording |=
                            preferences.silence_auto_stop_seconds != previous;
                    }
                    let selected_microphone = if preferences.microphone_device_name.is_empty() {
                        "系统默认".to_string()
                    } else {
                        preferences.microphone_device_name.clone()
                    };
                    let previous_microphone = preferences.microphone_device_name.clone();
                    egui::ComboBox::from_label("麦克风")
                        .selected_text(selected_microphone)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut preferences.microphone_device_name,
                                String::new(),
                                "系统默认",
                            );
                            for device in &self.microphones {
                                ui.selectable_value(
                                    &mut preferences.microphone_device_name,
                                    device.name.clone(),
                                    &device.name,
                                );
                            }
                        });
                    self.settings_dirty.microphone |=
                        preferences.microphone_device_name != previous_microphone;
                });
                ui.collapsing("外观", |ui| {
                    let previous_theme = preferences.theme_mode;
                    egui::ComboBox::from_label("主题")
                        .selected_text(match preferences.theme_mode {
                            openless_core::shared_types::ThemeMode::System => "跟随系统",
                            openless_core::shared_types::ThemeMode::Light => "浅色",
                            openless_core::shared_types::ThemeMode::Dark => "深色",
                        })
                        .show_ui(ui, |ui| {
                            for (mode, label) in [
                                (openless_core::shared_types::ThemeMode::System, "跟随系统"),
                                (openless_core::shared_types::ThemeMode::Light, "浅色"),
                                (openless_core::shared_types::ThemeMode::Dark, "深色"),
                            ] {
                                ui.selectable_value(&mut preferences.theme_mode, mode, label);
                            }
                        });
                    self.settings_dirty.appearance |= preferences.theme_mode != previous_theme;
                    self.settings_dirty.appearance |= ui
                        .checkbox(
                            &mut preferences.show_overview_activity_heatmap,
                            "显示活动热力图",
                        )
                        .changed();
                });
                ui.collapsing("fcitx5 快捷键", |ui| {
                    self.settings_dirty.hotkeys |=
                        shortcut_editor(ui, "听写", &mut preferences.dictation_hotkey);
                    self.settings_dirty.hotkeys |=
                        optional_shortcut_editor(ui, "QA", &mut preferences.qa_hotkey, ";");
                    self.settings_dirty.hotkeys |=
                        shortcut_editor(ui, "翻译修饰键", &mut preferences.translation_hotkey);
                    self.settings_dirty.hotkeys |= optional_shortcut_editor(
                        ui,
                        "选区润色",
                        &mut preferences.selection_polish_hotkey,
                        "P",
                    );
                    self.settings_dirty.hotkeys |= optional_shortcut_editor(
                        ui,
                        "切换风格",
                        &mut preferences.switch_style_hotkey,
                        "S",
                    );
                    self.settings_dirty.hotkeys |= optional_shortcut_editor(
                        ui,
                        "打开应用",
                        &mut preferences.open_app_hotkey,
                        "O",
                    );
                    self.settings_dirty.hotkeys |= optional_shortcut_editor(
                        ui,
                        "Coding Agent 语音",
                        &mut preferences.coding_agent_voice_hotkey,
                        "L",
                    );
                });
                self.settings_dirty.start_minimized |= ui
                    .checkbox(&mut preferences.start_minimized, "启动时隐藏主窗口")
                    .changed();
                self.settings_dirty.launch_at_login |= ui
                    .checkbox(&mut preferences.launch_at_login, "开机启动")
                    .changed();
                self.settings_dirty.auto_update_check |= ui
                    .checkbox(&mut preferences.auto_update_check, "自动检查更新")
                    .changed();
                let previous_channel = preferences.update_channel;
                egui::ComboBox::from_label("更新渠道")
                    .selected_text(match preferences.update_channel {
                        openless_core::shared_types::UpdateChannel::Stable => "稳定版",
                        openless_core::shared_types::UpdateChannel::Beta => "Beta",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut preferences.update_channel,
                            openless_core::shared_types::UpdateChannel::Stable,
                            "稳定版",
                        );
                        ui.selectable_value(
                            &mut preferences.update_channel,
                            openless_core::shared_types::UpdateChannel::Beta,
                            "Beta",
                        );
                    });
                self.settings_dirty.update_channel |=
                    preferences.update_channel != previous_channel;
                self.settings_dirty.remote_input_enabled |= ui
                    .checkbox(&mut preferences.remote_input_enabled, "启用远程输入")
                    .changed();
                self.settings_dirty.remote_input_port |= ui
                    .add(
                        egui::DragValue::new(&mut preferences.remote_input_port)
                            .range(1..=u16::MAX)
                            .prefix("端口 "),
                    )
                    .changed();
                if ui.button("保存设置").clicked() {
                    save_settings = true;
                }
            }
            if save_settings {
                if let (Some(native), Some(draft), Some(snapshot)) =
                    (&self.native, self.preferences.clone(), &self.snapshot)
                {
                    let host = native.host_arc();
                    let revision = snapshot.preferences_revision;
                    let dirty = self.settings_dirty;
                    let tx = self.tx.clone();
                    self.tokio.spawn(async move {
                        let outcome = tokio::task::spawn_blocking(move || {
                            let save = |preferences, revision| {
                                if dirty.hotkeys {
                                    host.update_settings_strict(preferences, revision)
                                } else {
                                    host.save_settings(preferences, revision)
                                }
                            };
                            match save(draft.clone(), revision) {
                                Err(error)
                                    if error.code == openless_core::BackendErrorCode::Busy =>
                                {
                                    let latest_snapshot = host.snapshot();
                                    let latest = host.backend().get_preferences();
                                    save(
                                        dirty.merge(&latest, &draft),
                                        latest_snapshot.preferences_revision,
                                    )
                                }
                                result => result,
                            }
                        })
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result.map_err(|error| error.to_string()));
                        let _ = tx.send(UiResult::SettingsSaved(Box::new(outcome)));
                    });
                }
            }
            if let Some((remote, pin)) = &self.remote_access {
                ui.label(if remote.running {
                    "远程输入：运行中"
                } else if remote.starting {
                    "远程输入：启动中"
                } else {
                    "远程输入：已停止"
                });
                if remote.enabled {
                    ui.label(format!(
                        "语言：{} · 连接数：{}",
                        remote.locale, remote.connection_count
                    ));
                    ui.monospace(format!("PIN：{pin}"));
                    for url in &remote.urls {
                        ui.monospace(url);
                    }
                    if ui.button("重置配对码").clicked() {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend
                                    .services()
                                    .remote_input
                                    .regenerate_pairing_pin()
                                    .await?;
                                Ok("远程输入配对码已重置".to_string())
                            });
                        }
                    }
                }
            }
            if ui.button("导出错误日志").clicked() {
                if let Some(backend) = self.backend() {
                    let source = openless_linux_egui::log_path(&backend.config().data_dir);
                    self.spawn(async move {
                        let destination = tokio::task::spawn_blocking(|| {
                            rfd::FileDialog::new()
                                .add_filter("Log", &["log"])
                                .set_file_name("openless.log")
                                .save_file()
                        })
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Internal,
                                error.to_string(),
                            )
                        })?
                        .ok_or_else(|| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Cancelled,
                                "日志导出已取消",
                            )
                        })?;
                        tokio::task::spawn_blocking(move || {
                            openless_linux_egui::export_error_log(&source, &destination)
                        })
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Internal,
                                error.to_string(),
                            )
                        })?
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Platform,
                                error.to_string(),
                            )
                        })?;
                        Ok("错误日志已导出".to_string())
                    });
                }
            }
            ui.separator();
            ui.heading("软件更新");
            let channel = self
                .preferences
                .as_ref()
                .map(|preferences| preferences.update_channel)
                .unwrap_or_default();
            match &self.update_support {
                LinuxUpdateSupport::AppImage(_) => {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!self.update_busy, egui::Button::new("立即检查"))
                            .clicked()
                        {
                            self.request_update_check(channel);
                        }
                        if self.update_manifest.is_some()
                            && ui
                                .add_enabled(!self.update_busy, egui::Button::new("下载并安装"))
                                .clicked()
                        {
                            self.install_update();
                        }
                    });
                    if let Some(manifest) = &self.update_manifest {
                        ui.label(format!("可用版本：{}", manifest.version));
                    }
                    if let Some(progress) = self.update_progress {
                        let fraction = progress
                            .content_length
                            .filter(|total| *total > 0)
                            .map(|total| progress.downloaded as f32 / total as f32);
                        if let Some(fraction) = fraction {
                            ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)));
                        }
                        ui.label(format!("已下载 {} 字节", progress.downloaded));
                    }
                }
                LinuxUpdateSupport::ManualOnly { releases_url } => {
                    ui.label("deb/rpm 与开发构建由包管理器或发布页更新。");
                    if ui.button("打开发布页").clicked() {
                        let url = (*releases_url).to_string();
                        std::thread::spawn(move || {
                            let _ = open_external(&url);
                        });
                    }
                }
            }
        }

        fn vocabulary_ui(&mut self, ui: &mut egui::Ui) {
            if let Some(backend) = self.backend() {
                let pending = backend.pending_corrections();
                if !pending.is_empty() {
                    ui.heading("待确认的手改建议");
                    let mut action: Option<(String, bool)> = None;
                    for suggestion in pending {
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} → {}",
                                suggestion.pattern, suggestion.replacement
                            ));
                            if ui.small_button("接受").clicked() {
                                action = Some((suggestion.id.clone(), true));
                            }
                            if ui.small_button("忽略").clicked() {
                                action = Some((suggestion.id.clone(), false));
                            }
                        });
                    }
                    if ui.button("全部关闭").clicked() {
                        backend.dismiss_pending_corrections();
                    }
                    if let Some((id, accept)) = action {
                        self.spawn(async move {
                            if accept {
                                backend.accept_pending_correction(&id)?;
                            } else {
                                backend.reject_pending_correction(&id);
                            }
                            Ok("词汇建议已处理".to_string())
                        });
                    }
                    ui.separator();
                }
            }
            ui.heading("词汇预设");
            ui.label("预设由 Core 合并内置版本、用户覆盖和自定义内容。");
            let mut preset_action: Option<(String, String)> = None;
            for preset in &self.vocab_presets {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&preset.name);
                    ui.label(format!("{} 个词", preset.phrases.len()));
                    if ui.small_button("应用").clicked() {
                        preset_action = Some((preset.id.clone(), "apply".into()));
                    }
                    if self
                        .vocab_preset_store
                        .custom
                        .iter()
                        .any(|custom| custom.id == preset.id)
                        && ui.small_button("删除").clicked()
                    {
                        preset_action = Some((preset.id.clone(), "delete".into()));
                    } else if openless_core::builtin_vocab_presets()
                        .iter()
                        .any(|builtin| builtin.id == preset.id)
                        && ui.small_button("隐藏内置预设").clicked()
                    {
                        preset_action = Some((preset.id.clone(), "disable".into()));
                    }
                    ui.weak(preset.phrases.join("、"));
                });
            }
            for id in self.vocab_preset_store.disabled_builtin_preset_ids.clone() {
                if ui.small_button(format!("恢复内置预设：{id}")).clicked() {
                    preset_action = Some((id, "enable".into()));
                }
            }
            ui.group(|ui| {
                ui.label("新建自定义预设");
                ui.text_edit_singleline(&mut self.vocab_preset_name);
                ui.add(
                    egui::TextEdit::multiline(&mut self.vocab_preset_phrases)
                        .hint_text("每行或逗号分隔一个词")
                        .desired_rows(3),
                );
                if ui.button("保存预设").clicked()
                    && !self.vocab_preset_name.trim().is_empty()
                    && !self.vocab_preset_phrases.trim().is_empty()
                {
                    preset_action = Some((String::new(), "create".into()));
                }
            });
            if let (Some(backend), Some((id, operation))) = (self.backend(), preset_action) {
                let name = std::mem::take(&mut self.vocab_preset_name);
                let phrases_text = std::mem::take(&mut self.vocab_preset_phrases);
                let selected = self
                    .vocab_presets
                    .iter()
                    .find(|preset| preset.id == id)
                    .cloned();
                self.spawn(async move {
                    match operation.as_str() {
                        "apply" => {
                            let preset = selected.ok_or_else(|| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Cancelled,
                                    "词汇预设已不存在",
                                )
                            })?;
                            for phrase in preset.phrases {
                                backend.add_vocabulary(
                                    phrase,
                                    Some(format!("预设：{}", preset.name)),
                                )?;
                            }
                        }
                        "create" => {
                            let mut phrases = phrases_text
                                .split([',', '，', '\n'])
                                .map(str::trim)
                                .filter(|phrase| !phrase.is_empty())
                                .map(ToOwned::to_owned)
                                .collect::<Vec<_>>();
                            phrases.sort();
                            phrases.dedup();
                            let mut store = backend.list_vocabulary_presets()?;
                            store.custom.push(openless_core::VocabPreset {
                                id: uuid::Uuid::new_v4().to_string(),
                                name: name.trim().to_string(),
                                phrases,
                            });
                            backend.save_vocabulary_presets(&store)?;
                        }
                        "delete" => {
                            let mut store = backend.list_vocabulary_presets()?;
                            store.custom.retain(|preset| preset.id != id);
                            backend.save_vocabulary_presets(&store)?;
                        }
                        "disable" => {
                            let mut store = backend.list_vocabulary_presets()?;
                            if !store.disabled_builtin_preset_ids.contains(&id) {
                                store.disabled_builtin_preset_ids.push(id);
                            }
                            backend.save_vocabulary_presets(&store)?;
                        }
                        "enable" => {
                            let mut store = backend.list_vocabulary_presets()?;
                            store
                                .disabled_builtin_preset_ids
                                .retain(|preset_id| preset_id != &id);
                            backend.save_vocabulary_presets(&store)?;
                        }
                        _ => unreachable!(),
                    }
                    Ok("词汇预设已更新".to_string())
                });
            }
            ui.separator();
            ui.heading("自定义词汇");
            ui.horizontal(|ui| {
                ui.label("词语");
                ui.text_edit_singleline(&mut self.vocabulary_phrase);
                ui.label("备注");
                ui.text_edit_singleline(&mut self.vocabulary_note);
                if ui.button("添加").clicked() && !self.vocabulary_phrase.trim().is_empty() {
                    if let Some(backend) = self.backend() {
                        let phrase = std::mem::take(&mut self.vocabulary_phrase);
                        let note = std::mem::take(&mut self.vocabulary_note);
                        self.spawn(async move {
                            backend.add_vocabulary(
                                phrase,
                                (!note.trim().is_empty()).then_some(note),
                            )?;
                            Ok("词汇已保存".to_string())
                        });
                    }
                }
            });
            let mut vocabulary_action = None;
            for entry in &self.vocabulary {
                ui.horizontal(|ui| {
                    let mut enabled = entry.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        vocabulary_action = Some((entry.id.clone(), Some(enabled)));
                    }
                    ui.label(egui::RichText::new(&entry.phrase).strong());
                    if let Some(note) = &entry.note {
                        ui.label(note);
                    }
                    ui.label(format!("命中 {}", entry.hits));
                    if ui.small_button("删除").clicked() {
                        vocabulary_action = Some((entry.id.clone(), None));
                    }
                });
            }
            if let (Some(backend), Some((id, enabled))) = (self.backend(), vocabulary_action) {
                self.spawn(async move {
                    if let Some(enabled) = enabled {
                        backend.set_vocabulary_enabled(&id, enabled)?;
                    } else {
                        backend.remove_vocabulary(&id)?;
                    }
                    Ok("词汇已更新".to_string())
                });
            }

            ui.separator();
            ui.heading("纠错规则");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.correction_pattern);
                ui.label("→");
                ui.text_edit_singleline(&mut self.correction_replacement);
                if ui.button("添加规则").clicked()
                    && !self.correction_pattern.trim().is_empty()
                    && !self.correction_replacement.trim().is_empty()
                {
                    if let Some(backend) = self.backend() {
                        let pattern = std::mem::take(&mut self.correction_pattern);
                        let replacement = std::mem::take(&mut self.correction_replacement);
                        self.spawn(async move {
                            backend.add_correction_rule(pattern, replacement)?;
                            Ok("纠错规则已保存".to_string())
                        });
                    }
                }
            });
            let mut correction_action = None;
            for rule in &self.correction_rules {
                ui.horizontal(|ui| {
                    let mut enabled = rule.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        correction_action = Some((rule.id.clone(), Some(enabled)));
                    }
                    ui.label(format!("{} → {}", rule.pattern, rule.replacement));
                    ui.label(format!("{:?}", rule.source));
                    if ui.small_button("删除").clicked() {
                        correction_action = Some((rule.id.clone(), None));
                    }
                });
            }
            if let (Some(backend), Some((id, enabled))) = (self.backend(), correction_action) {
                self.spawn(async move {
                    if let Some(enabled) = enabled {
                        backend.set_correction_rule_enabled(&id, enabled)?;
                    } else {
                        backend.remove_correction_rule(&id)?;
                    }
                    Ok("纠错规则已更新".to_string())
                });
            }
        }

        fn styles_ui(&mut self, ui: &mut egui::Ui) {
            ui.label("风格包数据直接来自 Core repository；运行时 Prompt 由 Core 组合。");
            ui.group(|ui| {
                ui.strong("风格包直达快捷键");
                let previous_id = self.style_hotkey_pack_id.clone();
                egui::ComboBox::from_id_salt("style-hotkey-pack")
                    .selected_text(
                        self.style_packs
                            .iter()
                            .find(|pack| pack.id == self.style_hotkey_pack_id)
                            .map(|pack| pack.name.as_str())
                            .unwrap_or("选择风格包"),
                    )
                    .show_ui(ui, |ui| {
                        for pack in &self.style_packs {
                            ui.selectable_value(
                                &mut self.style_hotkey_pack_id,
                                pack.id.clone(),
                                &pack.name,
                            );
                        }
                    });
                if previous_id != self.style_hotkey_pack_id {
                    let binding = self.preferences.as_ref().and_then(|preferences| {
                        preferences
                            .style_pack_hotkeys
                            .iter()
                            .find(|hotkey| hotkey.pack_id == self.style_hotkey_pack_id)
                            .map(|hotkey| hotkey.binding.clone())
                    });
                    self.style_hotkey_primary = binding
                        .as_ref()
                        .map(|binding| binding.primary.clone())
                        .unwrap_or_default();
                    self.style_hotkey_modifiers = binding
                        .map(|binding| binding.modifiers.join("+"))
                        .unwrap_or_default();
                }
                ui.horizontal(|ui| {
                    ui.label("主键");
                    ui.text_edit_singleline(&mut self.style_hotkey_primary);
                    ui.label("修饰键（+ 分隔）");
                    ui.text_edit_singleline(&mut self.style_hotkey_modifiers);
                });
                let save = ui
                    .add_enabled(
                        !self.style_hotkey_pack_id.is_empty()
                            && !self.style_hotkey_primary.trim().is_empty(),
                        egui::Button::new("保存直达快捷键"),
                    )
                    .clicked();
                let remove = ui
                    .add_enabled(
                        !self.style_hotkey_pack_id.is_empty(),
                        egui::Button::new("移除直达快捷键"),
                    )
                    .clicked();
                if save || remove {
                    if let (Some(native), Some(mut preferences), Some(snapshot)) =
                        (&self.native, self.preferences.clone(), &self.snapshot)
                    {
                        let pack_id = self.style_hotkey_pack_id.clone();
                        let desired = save.then(|| openless_core::shared_types::ShortcutBinding {
                            primary: self.style_hotkey_primary.trim().to_string(),
                            modifiers: self
                                .style_hotkey_modifiers
                                .split('+')
                                .map(str::trim)
                                .filter(|modifier| !modifier.is_empty())
                                .map(ToOwned::to_owned)
                                .collect(),
                        });
                        set_style_pack_hotkey(&mut preferences, &pack_id, desired.clone());
                        let host = native.host_arc();
                        let revision = snapshot.preferences_revision;
                        self.spawn(async move {
                            tokio::task::spawn_blocking(move || {
                                match host.update_settings_strict(preferences, revision) {
                                    Err(error)
                                        if error.code == openless_core::BackendErrorCode::Busy =>
                                    {
                                        let mut latest = host.backend().get_preferences();
                                        set_style_pack_hotkey(&mut latest, &pack_id, desired);
                                        let revision = host.snapshot().preferences_revision;
                                        host.update_settings_strict(latest, revision)
                                    }
                                    result => result,
                                }
                            })
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })??;
                            Ok("风格包快捷键已更新".to_string())
                        });
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("新建风格包").clicked() {
                    self.style_editor = Some(openless_core::StylePack {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: "新风格".to_string(),
                        ..Default::default()
                    });
                }
                if ui.button("导入 ZIP").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            let path = tokio::task::spawn_blocking(|| {
                                rfd::FileDialog::new()
                                    .add_filter("OpenLess style pack", &["zip"])
                                    .pick_file()
                            })
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })?
                            .ok_or_else(|| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Cancelled,
                                    "风格包导入已取消",
                                )
                            })?;
                            let pack = tokio::task::spawn_blocking(move || {
                                backend.import_style_pack_path(&path)
                            })
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })??;
                            Ok(format!("已导入风格包：{}", pack.name))
                        });
                    }
                }
            });
            if let Some(editor) = self.style_editor.as_mut() {
                ui.group(|ui| {
                    ui.heading("风格包编辑器");
                    ui.horizontal(|ui| {
                        ui.label("名称");
                        ui.text_edit_singleline(&mut editor.name);
                        ui.label("版本");
                        ui.text_edit_singleline(&mut editor.version);
                    });
                    ui.label("描述");
                    ui.text_edit_multiline(&mut editor.description);
                    egui::ComboBox::from_label("基础模式")
                        .selected_text(editor.base_mode.display_name())
                        .show_ui(ui, |ui| {
                            for mode in [
                                openless_core::PolishMode::Raw,
                                openless_core::PolishMode::Light,
                                openless_core::PolishMode::Structured,
                                openless_core::PolishMode::Formal,
                            ] {
                                ui.selectable_value(
                                    &mut editor.base_mode,
                                    mode,
                                    mode.display_name(),
                                );
                            }
                        });
                    ui.label("听写 Prompt");
                    ui.add(egui::TextEdit::multiline(&mut editor.prompt).desired_rows(6));
                    ui.label("选区 Prompt（留空则使用 Core 默认）");
                    ui.add(egui::TextEdit::multiline(&mut editor.selection_prompt).desired_rows(4));
                });
                let mut save = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    save = ui.button("保存风格包").clicked();
                    cancel = ui.button("取消编辑").clicked();
                });
                if cancel {
                    self.style_editor = None;
                } else if save {
                    let pack = self.style_editor.take().expect("editor exists");
                    if let Some(backend) = self.backend() {
                        let exists = self.style_packs.iter().any(|item| item.id == pack.id);
                        self.spawn(async move {
                            let saved = if exists {
                                backend.update_style_pack(pack)?
                            } else {
                                backend.create_style_pack(pack)?
                            };
                            Ok(format!("风格包已保存：{}", saved.name))
                        });
                    }
                }
                ui.separator();
            }
            let mut action: Option<(String, &'static str, bool)> = None;
            for pack in self.style_packs.clone() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(&pack.name);
                        if pack.active {
                            ui.label(egui::RichText::new("当前").color(theme::BLUE));
                        }
                        ui.label(format!("{:?} · {:?}", pack.kind, pack.base_mode));
                    });
                    ui.label(&pack.description);
                    if let Some(author) = &pack.author {
                        ui.label(format!("作者：{author} · 版本 {}", pack.version));
                    }
                    ui.horizontal(|ui| {
                        if !pack.active && ui.button("设为当前").clicked() {
                            action = Some((pack.id.clone(), "activate", true));
                        }
                        let mut enabled = pack.enabled;
                        if ui.checkbox(&mut enabled, "启用").changed() {
                            action = Some((pack.id.clone(), "enabled", enabled));
                        }
                        if ui.button("运行时 Prompt 预览").clicked() {
                            if let Some(backend) = self.backend() {
                                let diagnostics = backend.preview_style_pack_runtime(&pack);
                                self.status = format!(
                                    "{}：单轮 {} 字，多轮 {} 字，热词 {} 个",
                                    diagnostics.pack_name,
                                    diagnostics.single_turn_prompt_chars,
                                    diagnostics.multi_turn_prompt_chars,
                                    diagnostics.hotwords.len()
                                );
                            }
                        }
                        if pack.kind == openless_core::StylePackKind::Imported
                            && ui.button("编辑").clicked()
                        {
                            self.style_editor = Some(pack.clone());
                        }
                        if pack.kind == openless_core::StylePackKind::Imported
                            && ui.button("删除").clicked()
                        {
                            action = Some((pack.id.clone(), "delete", false));
                        }
                        if pack.kind == openless_core::StylePackKind::Builtin
                            && ui.button("恢复内置默认").clicked()
                        {
                            action = Some((pack.id.clone(), "reset", false));
                        }
                        if ui.button("导出 ZIP").clicked() {
                            action = Some((pack.id.clone(), "export", false));
                        }
                    });
                });
                ui.add_space(8.0);
            }
            if let (Some(backend), Some((id, operation, value))) = (self.backend(), action) {
                self.spawn(async move {
                    match operation {
                        "activate" => {
                            backend.activate_style_pack(&id)?;
                        }
                        "enabled" => {
                            backend.set_style_pack_enabled(&id, value)?;
                        }
                        "delete" => {
                            backend.remove_style_pack(&id)?;
                        }
                        "reset" => {
                            backend.reset_builtin_style_pack(&id)?;
                        }
                        "export" => {
                            let bytes = backend.export_style_pack_bytes(&id)?;
                            let destination = tokio::task::spawn_blocking(move || {
                                rfd::FileDialog::new()
                                    .add_filter("OpenLess style pack", &["zip"])
                                    .set_file_name(format!("openless-style-{id}.zip"))
                                    .save_file()
                            })
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })?
                            .ok_or_else(|| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Cancelled,
                                    "风格包导出已取消",
                                )
                            })?;
                            tokio::task::spawn_blocking(move || {
                                openless_linux_egui::atomic_save(&destination, &bytes)
                            })
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })?
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Platform,
                                    error.to_string(),
                                )
                            })?;
                        }
                        _ => unreachable!(),
                    }
                    Ok("风格包已更新".to_string())
                });
            }
        }

        fn marketplace_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.marketplace_query);
                if ui.button("搜索/刷新").clicked() {
                    self.load_marketplace();
                }
                if ui.button("GitHub 登录").clicked() {
                    if let Some(backend) = self.backend() {
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let result = backend
                                .services()
                                .marketplace
                                .start_device_flow()
                                .await
                                .map_err(|error| error.to_string());
                            let _ = tx.send(UiResult::MarketplaceFlow(result));
                        });
                    }
                }
                if ui.button("退出登录").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.services().marketplace.logout().await?;
                            Ok("Marketplace 已退出登录".to_string())
                        });
                    }
                }
                if ui.button("我的发布/喜欢").clicked() {
                    self.load_marketplace_mine();
                }
            });
            if let Some(flow) = self.marketplace_flow.clone() {
                ui.horizontal(|ui| {
                    ui.label(format!("设备码：{}", flow.user_code));
                    if ui.button("打开 GitHub").clicked() {
                        let url = flow.verification_uri.clone();
                        std::thread::spawn(move || {
                            if let Err(error) = open_external(&url) {
                                eprintln!("OpenLess GitHub login URL failed: {error}");
                            }
                        });
                    }
                    if ui.button("检查授权").clicked() {
                        if let Some(backend) = self.backend() {
                            let tx = self.tx.clone();
                            let flow_id = flow.flow_id.clone();
                            self.tokio.spawn(async move {
                                let result = backend
                                    .services()
                                    .marketplace
                                    .poll_device_flow(flow_id)
                                    .await
                                    .map_err(|error| error.to_string());
                                let _ = tx.send(UiResult::MarketplaceAuthPoll(result));
                            });
                        }
                    }
                });
            }
            if self.marketplace_items.is_empty() {
                ui.label("尚未加载 Marketplace；点击“搜索/刷新”。");
                return;
            }
            if let Some(detail) = &self.marketplace_detail {
                ui.group(|ui| {
                    ui.heading(format!("详情：{}", detail.summary.name));
                    ui.label(format!("状态：{}", detail.state));
                    ui.label(&detail.prompt);
                });
            }
            if !self.marketplace_my_packs.is_empty() || !self.marketplace_my_likes.is_empty() {
                ui.group(|ui| {
                    ui.heading("我的 Marketplace");
                    ui.label(format!("喜欢的风格：{}", self.marketplace_my_likes.len()));
                    for pack in &self.marketplace_my_packs {
                        ui.label(format!("{} · {}", pack.summary.name, pack.state));
                    }
                });
            }
            let mut action: Option<(String, &'static str)> = None;
            for pack in &self.marketplace_items {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(&pack.name);
                        ui.label(format!("@{} · {}", pack.author_login, pack.version));
                    });
                    ui.label(&pack.description);
                    ui.label(format!(
                        "喜欢 {} · 下载 {} · {}",
                        pack.like_count, pack.download_count, pack.base_mode
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("安装").clicked() {
                            action = Some((pack.id.clone(), "install"));
                        }
                        if ui.button("喜欢/取消喜欢").clicked() {
                            action = Some((pack.id.clone(), "like"));
                        }
                        if ui.button("详情").clicked() {
                            action = Some((pack.id.clone(), "detail"));
                        }
                        if ui.button("下载 ZIP").clicked() {
                            action = Some((pack.id.clone(), "download"));
                        }
                    });
                });
                ui.add_space(8.0);
            }
            if let (Some(backend), Some((id, operation))) = (self.backend(), action) {
                let tx = self.tx.clone();
                self.tokio.spawn(async move {
                    match operation {
                        "install" => {
                            let result = backend
                                .services()
                                .marketplace
                                .install(id)
                                .await
                                .map(|pack| format!("已安装风格包：{}", pack.name))
                                .map_err(|error| error.to_string());
                            let _ =
                                tx.send(UiResult::Message(result.unwrap_or_else(|error| error)));
                        }
                        "like" => {
                            let result = backend.services().marketplace.toggle_like(id).await;
                            let message = result
                                .map(|result| format!("喜欢数：{}", result.like_count))
                                .unwrap_or_else(|error| error.to_string());
                            let _ = tx.send(UiResult::Message(message));
                        }
                        "detail" => {
                            let result = backend
                                .services()
                                .marketplace
                                .detail(id)
                                .await
                                .map_err(|error| error.to_string());
                            let _ = tx.send(UiResult::MarketplaceDetail(result));
                        }
                        "download" => {
                            let result = async {
                                let bytes = backend
                                    .services()
                                    .marketplace
                                    .download_archive(id.clone())
                                    .await?;
                                let destination = tokio::task::spawn_blocking(move || {
                                    rfd::FileDialog::new()
                                        .add_filter("OpenLess style pack", &["zip"])
                                        .set_file_name(format!("openless-marketplace-{id}.zip"))
                                        .save_file()
                                })
                                .await
                                .map_err(|error| {
                                    BackendError::new(
                                        openless_core::BackendErrorCode::Internal,
                                        error.to_string(),
                                    )
                                })?
                                .ok_or_else(|| {
                                    BackendError::new(
                                        openless_core::BackendErrorCode::Cancelled,
                                        "Marketplace 下载已取消",
                                    )
                                })?;
                                tokio::task::spawn_blocking(move || {
                                    openless_linux_egui::atomic_save(&destination, &bytes)
                                })
                                .await
                                .map_err(|error| {
                                    BackendError::new(
                                        openless_core::BackendErrorCode::Internal,
                                        error.to_string(),
                                    )
                                })?
                                .map_err(|error| {
                                    BackendError::new(
                                        openless_core::BackendErrorCode::Platform,
                                        error.to_string(),
                                    )
                                })?;
                                Ok::<_, BackendError>("Marketplace ZIP 已保存".to_string())
                            }
                            .await
                            .unwrap_or_else(|error| error.to_string());
                            let _ = tx.send(UiResult::Message(result));
                        }
                        _ => unreachable!(),
                    }
                });
            }
            ui.separator();
            ui.heading("发布本地风格包");
            let mut local_action: Option<(String, Option<String>, &'static str)> = None;
            for pack in self
                .style_packs
                .iter()
                .filter(|pack| pack.kind == openless_core::StylePackKind::Imported)
            {
                ui.horizontal(|ui| {
                    ui.label(&pack.name);
                    if ui.button("上传/更新").clicked() {
                        local_action =
                            Some((pack.id.clone(), pack.origin_pack_id.clone(), "upload"));
                    }
                });
            }
            for pack in &self.marketplace_my_packs {
                ui.horizontal(|ui| {
                    ui.label(format!("已发布：{}", pack.summary.name));
                    if ui.button("删除发布").clicked() {
                        local_action = Some((pack.summary.id.clone(), None, "delete"));
                    }
                });
            }
            if let (Some(backend), Some((id, origin, operation))) = (self.backend(), local_action) {
                self.spawn(async move {
                    match operation {
                        "upload" => {
                            let result = backend.services().marketplace.upload(id, origin).await?;
                            Ok(format!("发布状态：{} · {}", result.state, result.message))
                        }
                        "delete" => {
                            backend.services().marketplace.delete(id).await?;
                            Ok("Marketplace 发布已删除".to_string())
                        }
                        _ => unreachable!(),
                    }
                });
            }
        }

        fn history_ui(&mut self, ui: &mut egui::Ui) {
            ui.heading("历史");
            ui.horizontal(|ui| {
                ui.label("搜索");
                ui.text_edit_singleline(&mut self.history_search);
                if ui.button("清空全部").clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.clear_history()?;
                            Ok("历史已清空".to_string())
                        });
                    }
                }
            });
            let Some(backend) = self.backend() else {
                return;
            };
            match backend.list_history() {
                Ok(history) if history.is_empty() => {
                    ui.label("暂无历史记录");
                }
                Ok(history) => {
                    let query = self.history_search.trim().to_lowercase();
                    let mut action: Option<(String, &'static str, String)> = None;
                    for item in history
                        .into_iter()
                        .rev()
                        .filter(|item| {
                            query.is_empty()
                                || item.final_text.to_lowercase().contains(&query)
                                || item.raw_transcript.to_lowercase().contains(&query)
                        })
                        .take(100)
                    {
                        let delivery = match item.insert_status {
                            HistoryInsertStatus::Inserted => "已插入",
                            HistoryInsertStatus::CopiedFallback => "已复制",
                            HistoryInsertStatus::PasteSent => "已发送粘贴",
                            HistoryInsertStatus::Failed => "失败",
                            HistoryInsertStatus::NotRequested => "未请求插入",
                        };
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(format!("{} · {}", item.created_at, delivery));
                            ui.label(&item.final_text);
                            ui.horizontal(|ui| {
                                if ui.small_button("复制").clicked() {
                                    action =
                                        Some((item.id.clone(), "copy", item.final_text.clone()));
                                }
                                if ui.small_button("重新润色").clicked() {
                                    action = Some((
                                        item.id.clone(),
                                        "repolish",
                                        item.raw_transcript.clone(),
                                    ));
                                }
                                if item.has_audio_recording == Some(true) {
                                    if ui.small_button("播放录音").clicked() {
                                        action = Some((item.id.clone(), "play", String::new()));
                                    }
                                    if ui.small_button("导出录音").clicked() {
                                        action = Some((item.id.clone(), "export", String::new()));
                                    }
                                    if ui.small_button("重新转写").clicked() {
                                        action =
                                            Some((item.id.clone(), "retranscribe", String::new()));
                                    }
                                }
                                if ui.small_button("删除").clicked() {
                                    action = Some((item.id.clone(), "delete", String::new()));
                                }
                            });
                        });
                        ui.add_space(6.0);
                    }
                    if let Some((id, operation, text)) = action {
                        match operation {
                            "copy" => match arboard::Clipboard::new()
                                .and_then(|mut clipboard| clipboard.set_text(text))
                            {
                                Ok(()) => self.status = "历史文本已复制".to_string(),
                                Err(error) => self.status = format!("复制失败：{error}"),
                            },
                            "repolish" => {
                                let service = Arc::clone(&backend.services().auxiliary);
                                self.spawn(async move {
                                    let polished = service
                                        .repolish(openless_core::RepolishRequest {
                                            raw_text: text,
                                            style_pack_id: None,
                                            front_app: None,
                                        })
                                        .await?;
                                    Ok(format!("重新润色完成：{polished}"))
                                });
                            }
                            "delete" => self.spawn(async move {
                                backend.delete_history(&id)?;
                                Ok("历史记录已删除".to_string())
                            }),
                            "play" => {
                                let data_dir = backend.config().data_dir.clone();
                                self.spawn(async move {
                                    let path = openless_linux_egui::recording_path(&data_dir, &id)
                                        .map_err(|error| {
                                            BackendError::new(
                                                openless_core::BackendErrorCode::Persistence,
                                                error.to_string(),
                                            )
                                        })?;
                                    tokio::task::spawn_blocking(move || {
                                        openless_linux_egui::open_local_file(&path)
                                    })
                                    .await
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Internal,
                                            error.to_string(),
                                        )
                                    })?
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Platform,
                                            error.to_string(),
                                        )
                                    })?;
                                    Ok("已交给系统播放器".to_string())
                                });
                            }
                            "export" => {
                                let data_dir = backend.config().data_dir.clone();
                                self.spawn(async move {
                                    let file_name = format!("openless-recording-{id}.wav");
                                    let destination = tokio::task::spawn_blocking(move || {
                                        rfd::FileDialog::new()
                                            .add_filter("WAV audio", &["wav"])
                                            .set_file_name(file_name)
                                            .save_file()
                                    })
                                    .await
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Internal,
                                            error.to_string(),
                                        )
                                    })?
                                    .ok_or_else(|| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Cancelled,
                                            "录音导出已取消",
                                        )
                                    })?;
                                    let wav = tokio::task::spawn_blocking(move || {
                                        openless_linux_egui::read_recording_wav(&data_dir, &id)
                                    })
                                    .await
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Internal,
                                            error.to_string(),
                                        )
                                    })?
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Persistence,
                                            error.to_string(),
                                        )
                                    })?;
                                    let saved = tokio::task::spawn_blocking(move || {
                                        openless_linux_egui::atomic_save(&destination, &wav)
                                    })
                                    .await
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Internal,
                                            error.to_string(),
                                        )
                                    })?
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Platform,
                                            error.to_string(),
                                        )
                                    })?;
                                    Ok(format!("录音已导出：{}", saved.display()))
                                });
                            }
                            "retranscribe" => {
                                let data_dir = backend.config().data_dir.clone();
                                self.spawn(async move {
                                    let recording_id = id.clone();
                                    let wav = tokio::task::spawn_blocking(move || {
                                        openless_linux_egui::read_recording_wav(
                                            &data_dir,
                                            &recording_id,
                                        )
                                    })
                                    .await
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Internal,
                                            error.to_string(),
                                        )
                                    })?
                                    .map_err(|error| {
                                        BackendError::new(
                                            openless_core::BackendErrorCode::Persistence,
                                            error.to_string(),
                                        )
                                    })?;
                                    let pcm = openless_linux_egui::recording_pcm(&wav)
                                        .map_err(|error| {
                                            BackendError::new(
                                                openless_core::BackendErrorCode::Persistence,
                                                error.to_string(),
                                            )
                                        })?
                                        .to_vec();
                                    let started = std::time::Instant::now();
                                    let result = backend
                                        .services()
                                        .auxiliary
                                        .retranscribe_pcm(pcm)
                                        .await
                                        .map_err(|failure| failure.error)?;
                                    let entry = backend.apply_history_retranscription(
                                        &id,
                                        result.text,
                                        &result.asr,
                                        started.elapsed().as_millis() as u64,
                                    )?;
                                    Ok(format!("重新转写完成：{}", entry.final_text))
                                });
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                Err(error) => {
                    ui.label(error.to_string());
                }
            }
        }
    }

    impl eframe::App for OpenLessEguiApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.poll(ctx);
            self.drain_tray(ctx);
            theme::apply_visuals(
                ctx,
                self.preferences
                    .as_ref()
                    .map(|preferences| preferences.theme_mode)
                    .unwrap_or_default(),
            );
            let auto_check = self
                .preferences
                .as_ref()
                .is_some_and(|preferences| preferences.auto_update_check);
            if auto_check
                && !self.update_busy
                && self.update_manifest.is_none()
                && self
                    .update_schedule
                    .poll(self.update_started.elapsed(), false)
                    .is_some()
            {
                let channel = self
                    .preferences
                    .as_ref()
                    .map(|preferences| preferences.update_channel)
                    .unwrap_or_default();
                self.request_update_check(channel);
            }
            if ctx.input(|input| input.viewport().close_requested())
                && !self.exit_requested
                && self.tray.is_some()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                if let Some(backend) = self.backend() {
                    self.spawn(async move {
                        backend.cancel_active_voice_session(None).await?;
                        Ok("语音会话已取消".to_string())
                    });
                }
            }
            shell::titlebar(ctx);
            shell::sidebar(ctx, &mut self.active_page, &self.status);
            let active_page = self.active_page;
            shell::content_panel(ctx, active_page, |ui| {
                if let Some(error) = &self.startup_error {
                    ui.heading("启动失败");
                    ui.colored_label(egui::Color32::RED, error);
                    return;
                }
                match active_page {
                    shell::Page::Overview => {
                        self.dictation_ui(ui);
                        ui.separator();
                        self.qa_ui(ui);
                        if self.qa_visible {
                            ui.separator();
                        }
                        self.selection_ui(ui);
                    }
                    shell::Page::History => self.history_ui(ui),
                    shell::Page::Vocabulary => self.vocabulary_ui(ui),
                    shell::Page::Styles => self.styles_ui(ui),
                    shell::Page::Marketplace => self.marketplace_ui(ui),
                    shell::Page::Providers => self.settings_ui(ui),
                    shell::Page::Models => self.models_ui(ui),
                    shell::Page::Assistant => self.less_computer_ui(ui),
                }
            });
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    impl Drop for OpenLessEguiApp {
        fn drop(&mut self) {
            if let Some(native) = self.native.take() {
                let _ = self.tokio.block_on(native.shutdown());
            }
        }
    }

    fn provider_kind(kind: openless_core::ChannelKind) -> openless_core::ProviderKind {
        match kind {
            openless_core::ChannelKind::Asr => openless_core::ProviderKind::Asr,
            openless_core::ChannelKind::Llm => openless_core::ProviderKind::Llm,
        }
    }

    fn provider_slot(kind: openless_core::ChannelKind) -> openless_core::ProviderSlot {
        match kind {
            openless_core::ChannelKind::Asr => openless_core::ProviderSlot::Asr,
            openless_core::ChannelKind::Llm => openless_core::ProviderSlot::Llm,
        }
    }

    fn provider_namespace(kind: openless_core::ChannelKind) -> openless_core::CredentialNamespace {
        match kind {
            openless_core::ChannelKind::Asr => openless_core::CredentialNamespace::Asr,
            openless_core::ChannelKind::Llm => openless_core::CredentialNamespace::Llm,
        }
    }

    fn endpoint_account(kind: openless_core::ChannelKind) -> &'static str {
        match kind {
            openless_core::ChannelKind::Asr => openless_core::credentials::ASR_ENDPOINT_ACCOUNT,
            openless_core::ChannelKind::Llm => openless_core::credentials::LLM_ENDPOINT_ACCOUNT,
        }
    }

    fn model_account(kind: openless_core::ChannelKind) -> &'static str {
        match kind {
            openless_core::ChannelKind::Asr => openless_core::credentials::ASR_MODEL_ACCOUNT,
            openless_core::ChannelKind::Llm => openless_core::credentials::LLM_MODEL_ACCOUNT,
        }
    }

    fn api_key_account(kind: openless_core::ChannelKind) -> &'static str {
        match kind {
            openless_core::ChannelKind::Asr => openless_core::credentials::ASR_API_KEY_ACCOUNT,
            openless_core::ChannelKind::Llm => openless_core::credentials::LLM_API_KEY_ACCOUNT,
        }
    }

    fn provider_credential_key(
        kind: openless_core::ChannelKind,
        channel_id: &str,
        account: &str,
    ) -> Result<openless_core::CredentialKey, BackendError> {
        openless_core::CredentialKey::new(
            provider_namespace(kind),
            Some(channel_id.to_string()),
            account,
        )
    }

    fn provider_descriptor_label(descriptor: &openless_core::ProviderDescriptor) -> String {
        format!(
            "{} ({})",
            descriptor.label_key,
            descriptor.provider_type.as_str()
        )
    }

    fn auth_requirement_label(requirement: openless_core::AuthRequirement) -> &'static str {
        match requirement {
            openless_core::AuthRequirement::None => "无需 Secret",
            openless_core::AuthRequirement::ApiKey => "API Key",
            openless_core::AuthRequirement::EndpointModelOptionalApiKey => {
                "Endpoint + Model，API Key 可选"
            }
            openless_core::AuthRequirement::ApiKeyUnlessCustomEndpoint => {
                "公共 Endpoint 需要 API Key；自建 Endpoint 可无 Key"
            }
            openless_core::AuthRequirement::Volcengine => "火山引擎凭据",
            openless_core::AuthRequirement::Xfyun => "讯飞 AppID + API Key",
            openless_core::AuthRequirement::OAuth => "OAuth",
        }
    }

    fn provider_channel_descriptor(
        panel: &ProviderPanel,
        channel_id: &str,
    ) -> Option<(
        openless_core::ChannelSummary,
        openless_core::ProviderDescriptor,
    )> {
        let channel = panel
            .channels
            .iter()
            .find(|channel| channel.id == channel_id)?
            .clone();
        let descriptor = panel
            .descriptors
            .iter()
            .find(|descriptor| descriptor.provider_type.as_str() == channel.provider_type)
            .cloned()
            .or_else(|| {
                openless_core::provider_rules::provider_descriptor(
                    provider_kind(panel.kind),
                    &channel.provider_type,
                )
            })?;
        Some((channel, descriptor))
    }

    async fn read_provider_value(
        backend: &openless_core::OpenLessBackend,
        kind: openless_core::ChannelKind,
        channel_id: &str,
        account: &str,
    ) -> Result<Option<String>, BackendError> {
        backend
            .read_credential(provider_credential_key(kind, channel_id, account)?)
            .await
            .map(|value| value.map(openless_core::SecretValue::into_exposed))
    }

    async fn load_provider_editor(
        backend: Arc<openless_core::OpenLessBackend>,
        kind: openless_core::ChannelKind,
        channel: openless_core::ChannelSummary,
        descriptor: openless_core::ProviderDescriptor,
    ) -> Result<ProviderEditor, BackendError> {
        let endpoint = read_provider_value(&backend, kind, &channel.id, endpoint_account(kind))
            .await?
            .or_else(|| descriptor.default_endpoint.clone())
            .unwrap_or_default();
        let model = read_provider_value(&backend, kind, &channel.id, model_account(kind))
            .await?
            .or_else(|| descriptor.default_model.clone())
            .unwrap_or_default();
        let (auth_mode, resource_id) =
            if descriptor.auth_requirement == openless_core::AuthRequirement::Volcengine {
                (
                    read_provider_value(
                        &backend,
                        kind,
                        &channel.id,
                        openless_core::credentials::VOLCENGINE_AUTH_MODE_ACCOUNT,
                    )
                    .await?
                    .unwrap_or_else(|| "app_id_token".to_string()),
                    read_provider_value(
                        &backend,
                        kind,
                        &channel.id,
                        openless_core::credentials::VOLCENGINE_RESOURCE_ID_ACCOUNT,
                    )
                    .await?
                    .unwrap_or_default(),
                )
            } else {
                (String::new(), String::new())
            };
        Ok(ProviderEditor {
            kind,
            name: channel.name.clone(),
            channel,
            descriptor,
            endpoint,
            model,
            auth_mode,
            resource_id,
            primary_secret: String::new(),
            secondary_secret: String::new(),
        })
    }

    fn secret_edit(ui: &mut egui::Ui, label: &str, value: &mut String) {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::TextEdit::singleline(value).password(true));
        });
    }

    fn provider_fields_ui(ui: &mut egui::Ui, editor: &mut ProviderEditor) {
        // This match chooses which input controls to render; it does not decide
        // whether credentials are sufficient. ProviderService validates the
        // descriptor's AuthRequirement again before any protocol request.
        match editor.descriptor.auth_requirement {
            openless_core::AuthRequirement::None => {
                ui.label("此 Provider 不使用云凭据；模型由本地模型面板管理。");
            }
            openless_core::AuthRequirement::OAuth => {
                ui.label("此 Provider 使用 OAuth；Linux egui 不读取或显示 OAuth token。");
            }
            openless_core::AuthRequirement::Volcengine => {
                egui::ComboBox::from_id_salt("volcengine-auth-mode")
                    .selected_text(&editor.auth_mode)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut editor.auth_mode,
                            "app_id_token".to_string(),
                            "APP ID + Access Token",
                        );
                        ui.selectable_value(
                            &mut editor.auth_mode,
                            "api_key".to_string(),
                            "API Key",
                        );
                    });
                if editor.auth_mode == "api_key" {
                    secret_edit(ui, "API Key", &mut editor.primary_secret);
                } else {
                    secret_edit(ui, "APP ID", &mut editor.primary_secret);
                    secret_edit(ui, "Access Token", &mut editor.secondary_secret);
                }
                ui.horizontal(|ui| {
                    ui.label("Resource ID");
                    ui.text_edit_singleline(&mut editor.resource_id);
                });
                ui.horizontal(|ui| {
                    ui.label("Model");
                    ui.text_edit_singleline(&mut editor.model);
                });
            }
            openless_core::AuthRequirement::Xfyun => {
                secret_edit(ui, "AppID", &mut editor.primary_secret);
                secret_edit(ui, "API Key", &mut editor.secondary_secret);
            }
            _ => {
                secret_edit(ui, "API Key（留空表示不修改）", &mut editor.primary_secret);
                ui.horizontal(|ui| {
                    ui.label("Endpoint");
                    ui.text_edit_singleline(&mut editor.endpoint);
                });
                ui.horizontal(|ui| {
                    ui.label("Model");
                    ui.text_edit_singleline(&mut editor.model);
                });
            }
        }
    }

    async fn write_or_remove_provider_value(
        backend: &openless_core::OpenLessBackend,
        kind: openless_core::ChannelKind,
        channel_id: &str,
        account: &str,
        value: &str,
    ) -> Result<(), BackendError> {
        let key = provider_credential_key(kind, channel_id, account)?;
        if value.trim().is_empty() {
            backend.remove_credential(key).await?;
        } else {
            backend
                .set_credential(key, openless_core::SecretValue::new(value.trim()))
                .await?;
        }
        Ok(())
    }

    async fn write_secret_if_entered(
        backend: &openless_core::OpenLessBackend,
        kind: openless_core::ChannelKind,
        channel_id: &str,
        account: &str,
        value: &str,
    ) -> Result<(), BackendError> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(());
        }
        backend
            .set_credential(
                provider_credential_key(kind, channel_id, account)?,
                openless_core::SecretValue::new(value),
            )
            .await?;
        Ok(())
    }

    async fn save_provider_editor(
        backend: Arc<openless_core::OpenLessBackend>,
        editor: ProviderEditor,
    ) -> Result<(), BackendError> {
        // Account names are the stable credential wire schema exported by
        // Core. Defaults and required/optional semantics stay in the selected
        // ProviderDescriptor and ProviderService, never in this Host form.
        let channel_id = editor.channel.id.as_str();
        backend
            .rename_channel(editor.kind, channel_id.to_string(), editor.name)
            .await?;
        match editor.descriptor.auth_requirement {
            openless_core::AuthRequirement::None | openless_core::AuthRequirement::OAuth => {}
            openless_core::AuthRequirement::Volcengine => {
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::VOLCENGINE_AUTH_MODE_ACCOUNT,
                    &editor.auth_mode,
                )
                .await?;
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::VOLCENGINE_RESOURCE_ID_ACCOUNT,
                    &editor.resource_id,
                )
                .await?;
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    model_account(editor.kind),
                    &editor.model,
                )
                .await?;
                if editor.auth_mode == "api_key" {
                    write_secret_if_entered(
                        &backend,
                        editor.kind,
                        channel_id,
                        openless_core::credentials::VOLCENGINE_API_KEY_ACCOUNT,
                        &editor.primary_secret,
                    )
                    .await?;
                } else {
                    write_secret_if_entered(
                        &backend,
                        editor.kind,
                        channel_id,
                        openless_core::credentials::VOLCENGINE_APP_KEY_ACCOUNT,
                        &editor.primary_secret,
                    )
                    .await?;
                    write_secret_if_entered(
                        &backend,
                        editor.kind,
                        channel_id,
                        openless_core::credentials::VOLCENGINE_ACCESS_KEY_ACCOUNT,
                        &editor.secondary_secret,
                    )
                    .await?;
                }
            }
            openless_core::AuthRequirement::Xfyun => {
                write_secret_if_entered(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::XFYUN_APP_ID_ACCOUNT,
                    &editor.primary_secret,
                )
                .await?;
                write_secret_if_entered(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::XFYUN_API_KEY_ACCOUNT,
                    &editor.secondary_secret,
                )
                .await?;
            }
            _ => {
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    endpoint_account(editor.kind),
                    &editor.endpoint,
                )
                .await?;
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    model_account(editor.kind),
                    &editor.model,
                )
                .await?;
                write_secret_if_entered(
                    &backend,
                    editor.kind,
                    channel_id,
                    api_key_account(editor.kind),
                    &editor.primary_secret,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn clear_provider_secrets(
        backend: Arc<openless_core::OpenLessBackend>,
        editor: &ProviderEditor,
    ) -> Result<(), BackendError> {
        let accounts: &[&str] = match editor.descriptor.auth_requirement {
            openless_core::AuthRequirement::None | openless_core::AuthRequirement::OAuth => &[],
            openless_core::AuthRequirement::Volcengine => &[
                openless_core::credentials::VOLCENGINE_APP_KEY_ACCOUNT,
                openless_core::credentials::VOLCENGINE_ACCESS_KEY_ACCOUNT,
                openless_core::credentials::VOLCENGINE_API_KEY_ACCOUNT,
            ],
            openless_core::AuthRequirement::Xfyun => &[
                openless_core::credentials::XFYUN_APP_ID_ACCOUNT,
                openless_core::credentials::XFYUN_API_KEY_ACCOUNT,
            ],
            _ => &[api_key_account(editor.kind)],
        };
        for account in accounts {
            backend
                .remove_credential(provider_credential_key(
                    editor.kind,
                    &editor.channel.id,
                    account,
                )?)
                .await?;
        }
        Ok(())
    }

    async fn validate_provider_channel(
        backend: Arc<openless_core::OpenLessBackend>,
        kind: openless_core::ChannelKind,
        channel_id: String,
    ) -> Result<String, BackendError> {
        let started = std::time::Instant::now();
        let result = backend
            .services()
            .provider
            .validate(openless_core::ProviderRequest {
                kind: provider_kind(kind),
                channel_id: Some(channel_id.clone()),
            })
            .await;
        let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        match result {
            Ok(_) => {
                backend
                    .record_channel_test(kind, channel_id, true, Some(latency_ms), None)
                    .await?;
                Ok(format!("Provider 验证通过（{latency_ms} ms）"))
            }
            Err(error) => {
                let _ = backend
                    .record_channel_test(
                        kind,
                        channel_id,
                        false,
                        Some(latency_ms),
                        Some(error.message.clone()),
                    )
                    .await;
                Err(error)
            }
        }
    }

    fn shortcut_editor(
        ui: &mut egui::Ui,
        label: &str,
        binding: &mut openless_core::shared_types::ShortcutBinding,
    ) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(label);
            changed |= ui.text_edit_singleline(&mut binding.primary).changed();
            for (modifier, caption) in [
                ("ctrl", "Ctrl"),
                ("alt", "Alt"),
                ("shift", "Shift"),
                ("super", "Super"),
            ] {
                let mut enabled = binding
                    .modifiers
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(modifier));
                if ui.checkbox(&mut enabled, caption).changed() {
                    changed = true;
                    binding
                        .modifiers
                        .retain(|value| !value.eq_ignore_ascii_case(modifier));
                    if enabled {
                        binding.modifiers.push(modifier.to_string());
                    }
                }
            }
        });
        changed
    }

    fn optional_shortcut_editor(
        ui: &mut egui::Ui,
        label: &str,
        binding: &mut Option<openless_core::shared_types::ShortcutBinding>,
        default_primary: &str,
    ) -> bool {
        let mut enabled = binding.is_some();
        let mut changed = ui.checkbox(&mut enabled, format!("启用{label}")).changed();
        if enabled && binding.is_none() {
            *binding = Some(openless_core::shared_types::ShortcutBinding {
                primary: default_primary.to_string(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            });
        } else if !enabled && binding.is_some() {
            *binding = None;
        }
        if let Some(binding) = binding {
            changed |= shortcut_editor(ui, label, binding);
        }
        changed
    }

    fn set_style_pack_hotkey(
        preferences: &mut UserPreferences,
        pack_id: &str,
        binding: Option<openless_core::shared_types::ShortcutBinding>,
    ) {
        preferences
            .style_pack_hotkeys
            .retain(|hotkey| hotkey.pack_id != pack_id);
        if let Some(binding) = binding {
            preferences
                .style_pack_hotkeys
                .push(openless_core::shared_types::StylePackHotkey {
                    pack_id: pack_id.to_string(),
                    binding,
                });
        }
    }

    fn package_kind() -> LinuxPackageKind {
        if std::env::var_os("APPDIR").is_some() {
            LinuxPackageKind::AppImage
        } else if cfg!(debug_assertions) {
            LinuxPackageKind::Development
        } else {
            LinuxPackageKind::SystemPackage
        }
    }

    fn backend_config(
        tray_available: bool,
        updater_available: bool,
    ) -> Result<BackendConfig, String> {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let data_dir = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".local/share")))
            .ok_or_else(|| "HOME/XDG_DATA_HOME is unavailable".to_string())?
            .join("OpenLess");
        let cache_dir = std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".cache")))
            .ok_or_else(|| "HOME/XDG_CACHE_HOME is unavailable".to_string())?
            .join("OpenLess");
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&cache_dir).map_err(|error| error.to_string())?;
        let kind = package_kind();
        let capabilities =
            LinuxCapabilitySnapshot::detect(tray_available, kind, updater_available).capabilities;
        Ok(BackendConfig {
            data_dir,
            cache_dir,
            home_dir: home,
            resource_dir: std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf)),
            platform: capabilities,
            locale: std::env::var("LANG").unwrap_or_else(|_| "en-US".to_string()),
        })
    }

    fn ensure_fcitx5_ready(config: &BackendConfig) -> Result<(), String> {
        let home = config
            .home_dir
            .as_deref()
            .ok_or_else(|| "HOME is unavailable for the fcitx5 plugin".to_string())?;
        let layout = LinuxResourceLayout::detect(None).map_err(|error| error.to_string())?;
        let plan =
            FcitxPluginInstallPlan::for_layout(&layout, home).map_err(|error| error.to_string())?;
        let status = ensure_fcitx5_plugin_installed(&plan).map_err(|error| error.to_string())?;
        reconcile_fcitx5_install(status, || {
            reload_running_fcitx5();
        })
    }

    /// Map an fcitx5 addon install result onto startup.
    ///
    /// A freshly written addon (`Updated`) is harmless: the addon is loaded
    /// either by the next fcitx5 start or, when a daemon is already running, by
    /// `reload` right now. Both `Ready` and `Updated` let startup continue down
    /// the normal fcitx5 DBus path — never a global-hotkey fallback — and only a
    /// genuinely missing plugin aborts startup.
    fn reconcile_fcitx5_install(
        status: FcitxPluginStatus,
        mut reload: impl FnMut(),
    ) -> Result<(), String> {
        match status {
            FcitxPluginStatus::Ready => Ok(()),
            FcitxPluginStatus::Updated => {
                reload();
                Ok(())
            }
            FcitxPluginStatus::Missing => {
                Err("未找到 OpenLess fcitx5 插件；请重新安装当前软件包".to_string())
            }
        }
    }

    struct NativePopupApp {
        kind: PopupKind,
        state: PopupState,
        incoming: mpsc::Receiver<HostToPopup>,
        outgoing: mpsc::Sender<PopupToHost>,
        qa_input: String,
        outgoing_sequence: u64,
        ready_sent: bool,
        preview_focus_requested: bool,
    }

    impl NativePopupApp {
        fn send(&mut self, message: PopupToHost) {
            if self.outgoing.send(message).is_err() {
                eprintln!("OpenLess popup output channel closed");
            }
        }

        fn next_sequence(&mut self) -> u64 {
            self.outgoing_sequence = self.outgoing_sequence.saturating_add(1);
            self.outgoing_sequence
        }

        fn session_id(&self) -> Option<String> {
            self.state.session_id.clone()
        }

        fn dismiss(&mut self, ctx: &egui::Context) {
            let Some(session_id) = self.session_id() else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            };
            let version = POPUP_PROTOCOL_VERSION;
            let sequence = self.next_sequence();
            let message = match self.kind {
                PopupKind::Qa => PopupToHost::DismissQa {
                    version,
                    session_id,
                    sequence,
                },
                PopupKind::Preview => PopupToHost::CancelPreview {
                    version,
                    session_id,
                    sequence,
                },
                PopupKind::Capsule => PopupToHost::DismissCapsule {
                    version,
                    session_id,
                    sequence,
                },
            };
            self.send(message);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn popup_heading(ui: &mut egui::Ui, title: &str) {
        let response = ui
            .horizontal(|ui| ui.heading(title))
            .response
            .interact(egui::Sense::drag());
        if response.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    /// Lightweight Markdown renderer ported from #997. It intentionally covers
    /// the structures emitted by QA without introducing a WebView dependency.
    fn render_popup_markdown(ui: &mut egui::Ui, markdown: &str) {
        let mut code = String::new();
        let mut in_code = false;
        for line in markdown.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code {
                    render_popup_code(ui, code.trim_end());
                    code.clear();
                }
                in_code = !in_code;
                continue;
            }
            if in_code {
                code.push_str(line);
                code.push('\n');
                continue;
            }
            if trimmed.is_empty() {
                ui.add_space(4.0);
                continue;
            }
            let (text, size, strong, italics, bullet) =
                if let Some(value) = trimmed.strip_prefix("### ") {
                    (value, 14.0, true, false, false)
                } else if let Some(value) = trimmed.strip_prefix("## ") {
                    (value, 15.0, true, false, false)
                } else if let Some(value) = trimmed.strip_prefix("# ") {
                    (value, 16.0, true, false, false)
                } else if let Some(value) = trimmed.strip_prefix("> ") {
                    (value, 13.0, false, true, false)
                } else if let Some(value) = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))
                {
                    (value, 13.0, false, false, true)
                } else {
                    (trimmed, 13.0, false, false, false)
                };
            let display = if bullet {
                format!("• {text}")
            } else {
                text.to_string()
            };
            render_popup_inline(ui, &display, size, strong, italics);
        }
        if in_code && !code.is_empty() {
            render_popup_code(ui, code.trim_end());
        }
    }

    fn render_popup_code(ui: &mut egui::Ui, code: &str) {
        egui::Frame::new()
            .fill(theme::SURFACE_2)
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(code).monospace().size(12.0)).wrap());
            });
    }

    fn render_popup_inline(
        ui: &mut egui::Ui,
        text: &str,
        size: f32,
        base_strong: bool,
        base_italics: bool,
    ) {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = ui.available_width();
        let mut rest = text;
        while !rest.is_empty() {
            let mut matched = false;
            for (open, close, strong, italics, monospace) in [
                ("**", "**", true, false, false),
                ("__", "__", true, false, false),
                ("`", "`", false, false, true),
                ("*", "*", false, true, false),
                ("_", "_", false, true, false),
            ] {
                if let Some(after_open) = rest.strip_prefix(open) {
                    if let Some(end) = after_open.find(close) {
                        append_popup_text(
                            &mut job,
                            &after_open[..end],
                            size,
                            base_strong || strong,
                            base_italics || italics,
                            monospace,
                            ui,
                        );
                        rest = &after_open[end + close.len()..];
                        matched = true;
                        break;
                    }
                }
            }
            if matched {
                continue;
            }
            let next = ["**", "__", "`", "*", "_"]
                .iter()
                .filter_map(|marker| rest.find(marker))
                .min()
                .unwrap_or(rest.len());
            let length = if next == 0 {
                rest.chars().next().map(char::len_utf8).unwrap_or(0)
            } else {
                next
            };
            append_popup_text(
                &mut job,
                &rest[..length],
                size,
                base_strong,
                base_italics,
                false,
                ui,
            );
            rest = &rest[length..];
        }
        ui.add(egui::Label::new(job).wrap());
    }

    fn append_popup_text(
        job: &mut egui::text::LayoutJob,
        text: &str,
        size: f32,
        strong: bool,
        italics: bool,
        monospace: bool,
        ui: &egui::Ui,
    ) {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::new(
                    size,
                    if monospace {
                        egui::FontFamily::Monospace
                    } else {
                        egui::FontFamily::Proportional
                    },
                ),
                color: if strong {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                },
                background: if monospace {
                    theme::SURFACE_2
                } else {
                    egui::Color32::TRANSPARENT
                },
                italics,
                ..Default::default()
            },
        );
    }

    impl eframe::App for NativePopupApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            while let Ok(message) = self.incoming.try_recv() {
                if message
                    .content_kind()
                    .is_some_and(|message_kind| message_kind != self.kind)
                {
                    continue;
                }
                if matches!(message, HostToPopup::Preview { .. }) {
                    self.preview_focus_requested = false;
                }
                let shutdown = matches!(message, HostToPopup::Shutdown { .. });
                let outcome = self.state.apply(message);
                if outcome == openless_linux_egui::PopupApplyOutcome::Applied {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.state.visible));
                }
                if shutdown || self.state.shutdown_requested {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
            if !self.ready_sent {
                if let Some(session_id) = self.session_id() {
                    let sequence = self.next_sequence();
                    self.send(PopupToHost::Ready {
                        version: POPUP_PROTOCOL_VERSION,
                        session_id,
                        sequence,
                        kind: self.kind,
                    });
                    self.ready_sent = true;
                }
            }
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.dismiss(ctx);
                return;
            }
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme::SURFACE)
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::same(18)),
                )
                .show(ctx, |ui| match self.kind {
                    PopupKind::Preview => {
                        popup_heading(ui, "插入预览");
                        ui.label(&self.state.preview.source);
                        let editor = ui.add(
                            egui::TextEdit::multiline(&mut self.state.preview.text)
                                .desired_rows(6)
                                .desired_width(f32::INFINITY),
                        );
                        if !self.preview_focus_requested {
                            editor.request_focus();
                            self.preview_focus_requested = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.button("取消").clicked() {
                                self.dismiss(ctx);
                            }
                            if ui.button("插入").clicked() {
                                if let Some(session_id) = self.session_id() {
                                    let sequence = self.next_sequence();
                                    self.send(PopupToHost::ConfirmPreview {
                                        version: POPUP_PROTOCOL_VERSION,
                                        session_id,
                                        sequence,
                                        text: self.state.preview.text.clone(),
                                    });
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }
                        });
                    }
                    PopupKind::Qa => {
                        popup_heading(ui, "划词追问");
                        if let Some(selection) = &self.state.qa.selection_preview {
                            ui.label(egui::RichText::new(selection).italics().color(theme::INK_3));
                        }
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for message in &self.state.qa.messages {
                                    ui.label(egui::RichText::new(&message.role).strong());
                                    render_popup_markdown(ui, &message.content);
                                }
                                if !self.state.qa.streaming_answer.is_empty() {
                                    render_popup_markdown(ui, &self.state.qa.streaming_answer);
                                }
                                if let Some(error) = &self.state.qa.error {
                                    ui.colored_label(egui::Color32::RED, error);
                                }
                            });
                        let input = ui.text_edit_singleline(&mut self.qa_input);
                        ui.horizontal(|ui| {
                            if ui.button("关闭").clicked() {
                                self.dismiss(ctx);
                            }
                            if ui
                                .button(if self.state.qa.phase == "Recording" {
                                    "停止录音"
                                } else {
                                    "语音提问"
                                })
                                .clicked()
                            {
                                if let Some(session_id) = self.session_id() {
                                    let sequence = self.next_sequence();
                                    self.send(PopupToHost::ToggleQaRecording {
                                        version: POPUP_PROTOCOL_VERSION,
                                        session_id,
                                        sequence,
                                    });
                                }
                            }
                            let submit = ui.button("发送").clicked()
                                || (input.lost_focus()
                                    && ui.input(|state| state.key_pressed(egui::Key::Enter)));
                            if submit && !self.qa_input.trim().is_empty() {
                                if let Some(session_id) = self.session_id() {
                                    let sequence = self.next_sequence();
                                    let text = std::mem::take(&mut self.qa_input);
                                    self.send(PopupToHost::SubmitQa {
                                        version: POPUP_PROTOCOL_VERSION,
                                        session_id,
                                        sequence,
                                        text,
                                    });
                                }
                            }
                        });
                    }
                    PopupKind::Capsule => {
                        let response = ui
                            .horizontal(|ui| {
                                ui.spinner();
                                ui.strong(&self.state.capsule.phase);
                            })
                            .response
                            .interact(egui::Sense::drag());
                        if response.drag_started() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if !self.state.capsule.text.is_empty() {
                            ui.label(&self.state.capsule.text);
                        }
                        if let Some(level) = self.state.capsule.audio_level {
                            ui.add(egui::ProgressBar::new(level.clamp(0.0, 1.0)));
                        }
                    }
                });
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    fn popup_kind(args: &[String]) -> Option<PopupKind> {
        if !args.iter().any(|arg| arg == "--openless-egui-popup") {
            return None;
        }
        if args.iter().any(|arg| arg == "--qa") {
            Some(PopupKind::Qa)
        } else if args.iter().any(|arg| arg == "--preview") {
            Some(PopupKind::Preview)
        } else if args.iter().any(|arg| arg == "--capsule") {
            Some(PopupKind::Capsule)
        } else {
            None
        }
    }

    fn run_popup_process(kind: PopupKind) -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel(256);
        std::thread::Builder::new()
            .name("openless-popup-input".into())
            .spawn(move || {
                let stdin = std::io::stdin();
                let mut reader = std::io::BufReader::new(stdin.lock());
                if let Err(error) = openless_linux_egui::run_popup(&mut reader, |message| {
                    let _ = tx.send(message);
                }) {
                    eprintln!("OpenLess popup input failed: {error}");
                }
            })
            .map_err(|error| error.to_string())?;
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<PopupToHost>();
        std::thread::Builder::new()
            .name("openless-popup-output".into())
            .spawn(move || {
                let stdout = std::io::stdout();
                let mut writer = stdout.lock();
                while let Ok(message) = outgoing_rx.recv() {
                    if let Err(error) = write_jsonl(&mut writer, &message) {
                        eprintln!("OpenLess popup output failed: {error}");
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        let size = match kind {
            PopupKind::Qa => [520.0, 520.0],
            PopupKind::Preview => [480.0, 300.0],
            PopupKind::Capsule => [340.0, 112.0],
        };
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("OpenLess")
                .with_inner_size(size)
                .with_decorations(false)
                .with_always_on_top()
                .with_visible(false),
            ..Default::default()
        };
        eframe::run_native(
            "OpenLess Popup",
            options,
            Box::new(move |cc| {
                theme::install(&cc.egui_ctx);
                Ok(Box::new(NativePopupApp {
                    kind,
                    state: PopupState::default(),
                    incoming: rx,
                    outgoing: outgoing_tx,
                    qa_input: String::new(),
                    outgoing_sequence: 0,
                    ready_sent: false,
                    preview_focus_requested: false,
                }))
            }),
        )
        .map_err(|error| error.to_string())
    }

    pub fn run() -> Result<(), String> {
        let args = std::env::args().collect::<Vec<_>>();
        if let Some(kind) = popup_kind(&args) {
            return run_popup_process(kind);
        }
        let start_minimized = args.iter().any(|arg| arg == "--minimized");
        let tokio = Arc::new(tokio::runtime::Runtime::new().map_err(|error| error.to_string())?);
        let tray = openless_linux_egui::LinuxTray::start().ok();
        let tray_available = tray.is_some();
        let kind = package_kind();
        let update_support = LinuxUpdateSupport::initialize(kind);
        let updater_available = update_support.supports_auto_update();
        let config = backend_config(tray_available, updater_available)?;
        if let Err(error) = openless_linux_egui::init_file_logger(&config.data_dir) {
            eprintln!("OpenLess file logger unavailable: {error}");
        }
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| config.cache_dir.join("runtime"));
        let broker = match SingleInstanceBroker::acquire_or_forward(
            &runtime_dir.join("openless.lock"),
            &runtime_dir.join("openless.sock"),
            LinuxLaunchIntent::from_args(&args),
        )
        .map_err(|error| error.to_string())?
        {
            SingleInstanceRole::Primary(broker) => broker,
            SingleInstanceRole::Forwarded => return Ok(()),
        };
        let native = (|| {
            // AppImage may need to materialize its bundled plugin into the
            // per-user fcitx5 search path. Do that before opening the DBus
            // listener: otherwise the first run can wait forever for signals
            // from a plugin fcitx5 has never loaded.
            ensure_fcitx5_ready(&config)?;
            let hotkeys = Fcitx5HotkeyListener::start().map_err(|error| error.to_string())?;
            let backend = {
                // Construction captures the existing executor for cpal/native
                // callbacks. The GUI thread leaves its context before block_on;
                // no extra runtime or per-callback runtime is created.
                let _runtime_context = tokio.enter();
                LinuxBackendBuilder::from_shared_providers(config)
                    .map_err(|error| error.to_string())?
                    .build()
                    .map_err(|error| error.to_string())?
            };
            tokio
                .block_on(LinuxNativeRuntime::start(
                    backend,
                    Some(broker),
                    Some(hotkeys),
                ))
                .map_err(|error| error.to_string())
        })();
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("OpenLess")
                .with_inner_size([1240.0, 800.0])
                .with_min_inner_size([960.0, 640.0])
                .with_decorations(false)
                .with_transparent(false)
                .with_resizable(true)
                .with_visible(!start_minimized || !tray_available),
            ..Default::default()
        };
        eframe::run_native(
            "OpenLess",
            options,
            Box::new(move |cc| {
                theme::install(&cc.egui_ctx);
                Ok(Box::new(OpenLessEguiApp::new(
                    tokio,
                    native,
                    tray,
                    update_support,
                )))
            }),
        )
        .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn continuation_turn_keeps_receiving_output_and_approval() {
            let mut app = OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture".into()),
                None,
                LinuxUpdateSupport::ManualOnly {
                    releases_url: openless_linux_egui::RELEASES_URL,
                },
            );
            let first = openless_core::SessionId::new();
            let second = openless_core::SessionId::new();
            for (sequence, session, kind) in [
                (
                    1,
                    first,
                    LessComputerEventKind::User {
                        text: "first".into(),
                        fresh: true,
                    },
                ),
                (
                    2,
                    first,
                    LessComputerEventKind::Completed {
                        text: "first answer".into(),
                        cost_usd: None,
                    },
                ),
                (
                    3,
                    second,
                    LessComputerEventKind::User {
                        text: "follow up".into(),
                        fresh: false,
                    },
                ),
                (
                    4,
                    second,
                    LessComputerEventKind::Delta {
                        text: "second answer".into(),
                    },
                ),
                (
                    5,
                    second,
                    LessComputerEventKind::Approval {
                        token: "approval".into(),
                        command: "echo test".into(),
                        reason: "test".into(),
                    },
                ),
                (
                    6,
                    first,
                    LessComputerEventKind::Delta {
                        text: "stale".into(),
                    },
                ),
            ] {
                app.apply_event(BackendEvent {
                    sequence,
                    session_id: Some(session),
                    kind: BackendEventKind::LessComputerEvent(openless_core::LessComputerEvent {
                        seq: None,
                        kind,
                    }),
                });
            }
            assert_eq!(app.less_computer_session, Some(second));
            assert!(app.less_computer_output.ends_with("second answer"));
            assert_eq!(
                app.pending_approval,
                Some(("approval".into(), "echo test".into()))
            );
        }

        #[test]
        fn qa_deltas_accumulate_without_hiding_conversation_history() {
            let mut app = OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture".into()),
                None,
                LinuxUpdateSupport::ManualOnly {
                    releases_url: openless_linux_egui::RELEASES_URL,
                },
            );
            let session = openless_core::SessionId::new();
            let mut thinking = QaStateEvent::simple(QaStateKind::Thinking);
            thinking.session_id = Some(session.to_string());
            thinking.messages = Some(vec![openless_core::shared_types::QaChatMessage {
                role: "user".into(),
                content: "question".into(),
                selection_text: None,
            }]);
            app.apply_event(BackendEvent {
                sequence: 1,
                session_id: Some(session),
                kind: BackendEventKind::QaState(thinking),
            });
            for (sequence, chunk) in [(2, "Hello"), (3, " world")] {
                let mut delta = QaStateEvent::simple(QaStateKind::AnswerDelta);
                delta.session_id = Some(session.to_string());
                delta.chunk = Some(chunk.into());
                app.apply_event(BackendEvent {
                    sequence,
                    session_id: Some(session),
                    kind: BackendEventKind::QaState(delta),
                });
            }
            let state = app.qa_state.as_ref().unwrap();
            assert_eq!(state.chunk.as_deref(), Some("Hello world"));
            assert_eq!(state.messages.as_ref().unwrap()[0].content, "question");
        }

        #[test]
        fn settings_conflict_merge_preserves_only_dirty_draft_fields() {
            let latest = UserPreferences {
                remote_input_port: 9443,
                streaming_insert: false,
                ..Default::default()
            };
            let draft = UserPreferences {
                remote_input_port: 7777,
                streaming_insert: true,
                ..Default::default()
            };
            let dirty = SettingsDirty {
                streaming_insert: true,
                ..Default::default()
            };

            let merged = dirty.merge(&latest, &draft);

            assert!(merged.streaming_insert);
            assert_eq!(merged.remote_input_port, 9443);
        }

        #[test]
        fn style_pack_hotkey_update_preserves_other_pack_bindings() {
            let mut preferences = UserPreferences::default();
            let first = openless_core::shared_types::ShortcutBinding {
                primary: "1".into(),
                modifiers: vec!["ctrl".into()],
            };
            let second = openless_core::shared_types::ShortcutBinding {
                primary: "2".into(),
                modifiers: vec!["alt".into()],
            };
            set_style_pack_hotkey(&mut preferences, "first", Some(first.clone()));
            set_style_pack_hotkey(&mut preferences, "second", Some(second.clone()));
            set_style_pack_hotkey(&mut preferences, "first", None);

            assert_eq!(preferences.style_pack_hotkeys.len(), 1);
            assert_eq!(preferences.style_pack_hotkeys[0].pack_id, "second");
            assert_eq!(preferences.style_pack_hotkeys[0].binding, second);
        }

        #[test]
        fn settings_conflict_merge_preserves_hotkey_drafts_as_one_domain() {
            let latest = UserPreferences::default();
            let mut draft = latest.clone();
            draft.open_app_hotkey = Some(openless_core::shared_types::ShortcutBinding {
                primary: "O".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            });
            let dirty = SettingsDirty {
                hotkeys: true,
                ..Default::default()
            };

            let merged = dirty.merge(&latest, &draft);

            assert_eq!(merged.open_app_hotkey, draft.open_app_hotkey);
        }

        #[test]
        fn settings_conflict_merge_preserves_recording_device_and_appearance_domains() {
            let mut latest = UserPreferences::default();
            latest.remote_input_port = 9443;
            let mut draft = latest.clone();
            draft.hotkey.mode = openless_core::shared_types::HotkeyMode::Auto;
            draft.silence_auto_stop_enabled = true;
            draft.silence_auto_stop_seconds = 1.5;
            draft.microphone_device_name = "USB microphone".into();
            draft.theme_mode = openless_core::shared_types::ThemeMode::Dark;
            draft.show_overview_activity_heatmap = false;
            draft.remote_input_port = 7777;
            let dirty = SettingsDirty {
                recording: true,
                microphone: true,
                appearance: true,
                ..Default::default()
            };

            let merged = dirty.merge(&latest, &draft);

            assert_eq!(merged.hotkey.mode, draft.hotkey.mode);
            assert!(merged.silence_auto_stop_enabled);
            assert_eq!(merged.silence_auto_stop_seconds, 1.5);
            assert_eq!(merged.microphone_device_name, "USB microphone");
            assert_eq!(
                merged.theme_mode,
                openless_core::shared_types::ThemeMode::Dark
            );
            assert!(!merged.show_overview_activity_heatmap);
            assert_eq!(merged.remote_input_port, 9443);
        }

        #[test]
        fn updated_install_reloads_running_fcitx5_and_continues_startup() {
            let mut reloads = 0;
            let reload = || reloads += 1;

            // A freshly written addon must reload a running fcitx5 and then let
            // startup continue (not hard-error as it used to).
            assert!(reconcile_fcitx5_install(FcitxPluginStatus::Updated, reload).is_ok());
            assert_eq!(reloads, 1, "Updated must issue one fcitx5 reload");
        }

        #[test]
        fn ready_install_needs_no_reload_but_continues() {
            let mut reloads = 0;
            assert!(reconcile_fcitx5_install(FcitxPluginStatus::Ready, || reloads += 1).is_ok());
            assert_eq!(reloads, 0, "Ready must not reload fcitx5");
        }

        #[test]
        fn missing_install_aborts_without_reloading() {
            let mut reloads = 0;
            let error = reconcile_fcitx5_install(FcitxPluginStatus::Missing, || reloads += 1)
                .expect_err("a missing plugin must abort startup");
            assert!(
                error.contains("OpenLess fcitx5 插件"),
                "unexpected Missing message: {error}"
            );
            assert_eq!(reloads, 0, "Missing must never reload fcitx5");
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux_app::run() {
        eprintln!("OpenLess Linux UI failed: {error}");
        std::process::exit(1);
    }
}
