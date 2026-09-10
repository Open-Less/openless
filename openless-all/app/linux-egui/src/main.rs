#[cfg(any(target_os = "linux", test))]
mod ui_state;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("openless-linux-egui is only available on Linux");
}

#[cfg(target_os = "linux")]
mod linux_app {
    use super::ui_state::{Navigation, Page};
    use std::future::Future;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use eframe::egui;
    use openless_core::{
        BackendConfig, BackendError, BackendEvent, BackendEventKind, BackendSnapshot,
        DictationPhase, HistoryInsertStatus, HostAction, LessComputerEventKind, LocalAsrModel,
        LocalAsrRuntime, QaStateEvent, QaStateKind, SelectionPhase, SelectionSnapshot,
        TranscriptAccumulator, UserPreferences,
    };
    use openless_linux_egui::ui::{self as ui20, UiPrefs};
    use openless_linux_egui::{
        drain_events, ensure_fcitx5_plugin_installed, EventDrainOutcome, Fcitx5HotkeyListener,
        FcitxPluginInstallPlan, FcitxPluginStatus, LinuxBackendBuilder, LinuxCapabilitySnapshot,
        LinuxDesktopSession, LinuxLaunchIntent, LinuxNativeRuntime, LinuxPackageKind,
        LinuxResourceLayout, SingleInstanceBroker, SingleInstanceRole,
    };

    enum UiResult {
        Environment(LinuxCapabilitySnapshot),
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
        app_id: String,
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
        navigation: Navigation,
        environment: Option<LinuxCapabilitySnapshot>,
        environment_refreshing: bool,
        plugin_check: Option<Result<FcitxPluginStatus, String>>,
        less_computer_running: bool,
        remote_error: Option<String>,
        tokio: Arc<tokio::runtime::Runtime>,
        native: Option<LinuxNativeRuntime>,
        subscription: Option<openless_core::EventSubscription>,
        snapshot: Option<BackendSnapshot>,
        preferences: Option<UserPreferences>,
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
        status: String,
        startup_error: Option<String>,
        tx: mpsc::Sender<UiResult>,
        rx: mpsc::Receiver<UiResult>,
        /// 2.0 外观偏好（深浅主题）；表现层状态，不进 Core 合同。
        ui_prefs: UiPrefs,
        /// 设置弹窗当前打开的分区；None = 关闭。Services / Models / Remote /
        /// Settings 与其他平台一致地收纳进设置弹窗，不再是主导航页面。
        settings_modal: Option<Page>,
        /// 侧栏「工具」分组是否展开（对应 React 侧栏的可展开分组）。
        tools_group_open: bool,
    }

    impl OpenLessEguiApp {
        fn new(
            tokio: Arc<tokio::runtime::Runtime>,
            native: Result<LinuxNativeRuntime, String>,
        ) -> Self {
            let (tx, rx) = mpsc::channel();
            match native {
                Ok(native) => {
                    let backend = native.host().backend();
                    let snapshot = backend.snapshot();
                    let preferences = backend.get_preferences();
                    let subscription = backend.subscribe();
                    let app = Self {
                        navigation: Navigation::default(),
                        environment: None,
                        environment_refreshing: false,
                        plugin_check: None,
                        less_computer_running: false,
                        remote_error: None,
                        tokio,
                        native: Some(native),
                        subscription: Some(subscription),
                        snapshot: Some(snapshot),
                        preferences: Some(preferences),
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
                        status: "Core 2.0 已启动".to_string(),
                        startup_error: None,
                        tx,
                        rx,
                        ui_prefs: UiPrefs::load(),
                        settings_modal: None,
                        tools_group_open: true,
                    };
                    app.load_models();
                    app.load_remote_status();
                    app.load_providers(openless_core::ChannelKind::Asr);
                    app
                }
                Err(error) => Self {
                    navigation: Navigation::default(),
                    environment: None,
                    environment_refreshing: false,
                    plugin_check: None,
                    less_computer_running: false,
                    remote_error: None,
                    tokio,
                    native: None,
                    subscription: None,
                    snapshot: None,
                    preferences: None,
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
                    status: "启动失败".to_string(),
                    startup_error: Some(error),
                    tx,
                    rx,
                    ui_prefs: UiPrefs::load(),
                    settings_modal: None,
                    tools_group_open: true,
                },
            }
        }

        fn backend(&self) -> Option<Arc<openless_core::OpenLessBackend>> {
            self.native
                .as_ref()
                .map(|native| Arc::clone(native.host().backend()))
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
                        thinking_enabled: backend.get_preferences().llm_thinking_enabled,
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
            self.last_event_sequence = event.sequence;
            let session_id = event.session_id;
            match event.kind {
                BackendEventKind::DictationStateChanged(state) => {
                    self.navigation.notify(Page::Dictation);
                    if state.phase == DictationPhase::Starting {
                        self.transcript_state = TranscriptAccumulator::default();
                        self.transcript.clear();
                        self.transcript_session = state.session_id;
                    }
                    self.status = format!("听写：{:?}", state.phase);
                }
                BackendEventKind::TranscriptDelta(delta)
                    if session_id == self.transcript_session =>
                {
                    if self.transcript_state.apply(&delta).is_ok() {
                        self.transcript = self.transcript_state.text().to_string();
                    }
                }
                BackendEventKind::PolishDelta(delta) if delta.is_final => {
                    self.transcript = delta.text;
                }
                BackendEventKind::DictationCompleted(result) => {
                    self.navigation.notify(Page::Dictation);
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
                    // Voice capture has its own session, preceding a chat User
                    // turn. Keep a navigation notice without assigning it to
                    // the current chat or inventing microphone readiness.
                    if matches!(&event.kind, LessComputerEventKind::VoiceState { .. }) {
                        self.navigation.notify(Page::Agent);
                        return;
                    }
                    // Less Computer events may complete after a newer turn has
                    // already started. Session ownership, not arrival time,
                    // decides whether a delta/terminal may mutate this view.
                    if let LessComputerEventKind::User { text, fresh } = &event.kind {
                        // Every User starts a new turn UUID, including a
                        // continuation. `fresh` describes conversation history,
                        // never whether this turn is allowed to receive output.
                        self.less_computer_session = session_id;
                        self.less_computer_running = true;
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
                    self.navigation.notify(Page::Agent);
                    match event.kind {
                        // Linux已有独立录音显示；新typed反馈供接手Host/UI团队继续接入。
                        LessComputerEventKind::VoiceState { .. } => {}
                        LessComputerEventKind::User { .. } => {}
                        LessComputerEventKind::Started => {
                            self.less_computer_running = true;
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
                            self.less_computer_running = false;
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
                            self.less_computer_running = false;
                            self.pending_approval = None;
                            self.status = message;
                        }
                        LessComputerEventKind::Cancelled => {
                            self.less_computer_running = false;
                            self.pending_approval = None;
                            self.status = "Less Computer 已取消".to_string();
                        }
                    }
                }
                BackendEventKind::LocalAsrDownloadProgress(progress) => {
                    self.navigation.notify(Page::Models);
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
                        self.preferences = Some(backend.get_preferences());
                    }
                    self.load_remote_status();
                }
                BackendEventKind::QaState(state) => {
                    if state.kind == QaStateKind::AnswerDelta {
                        if let Some(current) = self
                            .qa_state
                            .as_mut()
                            .filter(|current| current.session_id == state.session_id)
                        {
                            self.navigation.notify(Page::Qa);
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
                        self.navigation.notify(Page::Qa);
                        self.qa_state = Some(state);
                    }
                }
                BackendEventKind::SelectionStateChanged(snapshot) => {
                    self.navigation.notify(Page::Selection);
                    if snapshot.phase == SelectionPhase::Preview {
                        self.selection_draft = snapshot.preview_text.clone().unwrap_or_default();
                        self.selection_preview_visible = true;
                    }
                    self.selection = Some(snapshot);
                }
                BackendEventKind::RemoteInputStatusChanged(_)
                | BackendEventKind::RemoteInputFailed(_) => {
                    self.navigation.notify(Page::Remote);
                    self.load_remote_status();
                }
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
                        HostAction::ShowMain => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        HostAction::ShowLessComputer => {
                            self.navigation.open(Page::Agent);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        HostAction::FocusMain => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                        HostAction::Notify(message) => self.status = message,
                        HostAction::OpenExternalUrl(url) | HostAction::OpenSystemSettings(url) => {
                            std::thread::spawn(move || {
                                let _ = std::process::Command::new("xdg-open").arg(url).status();
                            });
                        }
                        HostAction::RequestRestart => {
                            self.status = "请手动重启 OpenLess".to_string();
                        }
                        HostAction::ShowSelectionPreview => {
                            self.navigation.open(Page::Selection);
                            self.selection_preview_visible = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        HostAction::HideSelectionPreview => {
                            self.selection_preview_visible = false;
                        }
                        HostAction::ShowQa => {
                            self.navigation.open(Page::Qa);
                            self.qa_visible = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        HostAction::HideQa => self.qa_visible = false,
                        HostAction::ShowDictationFeedback | HostAction::HideDictationFeedback => {}
                    }
                }
            }
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
                        self.less_computer_running = false;
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
                    UiResult::Environment(environment) => {
                        self.environment = Some(environment);
                        self.environment_refreshing = false;
                    }
                    UiResult::Message(message) => self.status = message,
                    UiResult::Models(Ok(models)) => self.models = ModelsState::Loaded(models),
                    UiResult::Models(Err(error)) => {
                        self.models = ModelsState::Failed(error.clone());
                        self.status = error;
                    }
                    UiResult::Remote(Ok(remote)) => {
                        self.remote_error = None;
                        self.remote_access = Some(remote);
                    }
                    UiResult::Remote(Err(error)) => {
                        self.remote_access = None;
                        self.remote_error = Some(error.clone());
                        self.status = error;
                    }
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
                }
            }
            if let Some(backend) = self.backend() {
                self.snapshot = Some(backend.snapshot());
            }
        }

        fn dictation_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "听写",
                Some("用已配置的听写快捷键或此页控制录音；转写与润色结果实时返回。"),
            );
            let phase = self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.dictation.phase)
                .unwrap_or(DictationPhase::Idle);
            let (phase_label, phase_status) = match phase {
                DictationPhase::Idle => ("空闲", ui20::widgets::Status::Info),
                DictationPhase::Starting => ("启动中", ui20::widgets::Status::Info),
                DictationPhase::Recording => ("录音中", ui20::widgets::Status::Warn),
                DictationPhase::Transcribing => ("转写中", ui20::widgets::Status::Info),
                DictationPhase::Polishing => ("润色中", ui20::widgets::Status::Info),
                DictationPhase::Inserting => ("落字中", ui20::widgets::Status::Info),
                DictationPhase::Completed => ("已完成", ui20::widgets::Status::Ok),
                DictationPhase::Cancelled => ("已取消", ui20::widgets::Status::Warn),
                DictationPhase::Failed => ("失败", ui20::widgets::Status::Err),
            };
            ui20::widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui20::widgets::status_pill(ui, phase_label, phase_status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui20::widgets::secondary_button(
                            ui,
                            "取消",
                            phase != DictationPhase::Idle,
                        )
                        .clicked()
                        {
                            if let Some(backend) = self.backend() {
                                self.spawn(async move {
                                    backend.cancel_dictation(None).await?;
                                    Ok("听写已取消".to_string())
                                });
                            }
                        }
                        if ui20::widgets::secondary_button(
                            ui,
                            "停止",
                            phase == DictationPhase::Recording,
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
                        if ui20::widgets::primary_button(ui, "开始", phase == DictationPhase::Idle)
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
                    });
                });
            });
            ui.add_space(8.0);
            ui20::widgets::subtle_card(ui, |ui| {
                ui20::widgets::section_title(ui, "转写结果");
                if self.transcript.is_empty() {
                    ui20::widgets::hint(ui, "尚无转写结果");
                } else {
                    ui.label(egui::RichText::new(&self.transcript).size(14.0));
                }
            });
        }

        fn less_computer_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "Less Computer",
                Some("向 Agent 描述任务；工具执行沿用 Core 的审批规则。"),
            );
            if self.pending_approval.is_some() {
                let p = ui20::current(ui.ctx());
                egui::Frame::default()
                    .fill(p.warn_soft())
                    .corner_radius(egui::CornerRadius::same(
                        openless_linux_egui::design_tokens::radius::CARD,
                    ))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui20::widgets::section_title(ui, "等待审批");
                        self.agent_approval_ui(ui);
                    });
                ui.add_space(8.0);
            }
            ui20::widgets::card(ui, |ui| {
                ui20::widgets::section_title(ui, "任务");
                ui.add(
                    egui::TextEdit::multiline(&mut self.less_computer_input)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui20::widgets::primary_button(
                        ui,
                        "运行",
                        !self.less_computer_input.trim().is_empty(),
                    )
                    .clicked()
                    {
                        if let Some(backend) = self.backend() {
                            let prompt = self.less_computer_input.clone();
                            self.less_computer_output.clear();
                            self.spawn(async move {
                                backend.submit_less_computer(prompt).await?;
                                Ok("Less Computer 已完成".to_string())
                            });
                        }
                    }
                    if ui20::widgets::secondary_button(ui, "取消", true).clicked() {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.cancel_less_computer(None).await?;
                                Ok("Less Computer 已取消".to_string())
                            });
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui20::widgets::subtle_card(ui, |ui| {
                ui20::widgets::section_title(ui, "输出");
                if self.less_computer_output.is_empty() {
                    ui20::widgets::hint(ui, "尚无 Agent 输出");
                } else {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.less_computer_output)
                                .size(13.5)
                                .monospace(),
                        )
                        .wrap(),
                    );
                }
            });
        }

        fn agent_approval_ui(&mut self, ui: &mut egui::Ui) {
            if let Some((token, command)) = self.pending_approval.clone() {
                egui::ScrollArea::vertical()
                    .id_salt("approval_command")
                    .max_height(72.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("请求执行：{command}"))
                                    .size(13.5)
                                    .monospace(),
                            )
                            .wrap(),
                        );
                    });
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
        }

        fn qa_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "划词问答",
                Some("文字或语音提问；切换页面保留当前会话。"),
            );
            if !self.qa_visible {
                ui20::widgets::info_panel(ui, |ui| {
                    ui20::widgets::section_title(ui, "打开问答后可文字提问或语音提问。");
                    ui20::widgets::hint(ui, "切换页面会保留当前会话；关闭会话使用下方的关闭操作。");
                    if ui20::widgets::blue_button(ui, "打开问答", true).clicked() {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.services().qa.show().await?;
                                Ok("问答已打开".to_string())
                            });
                        }
                    }
                });
                return;
            }
            ui20::widgets::card(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("qa_messages")
                    .max_height((ui.available_height() - 96.0).max(160.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut last_was_assistant = false;
                        if let Some(state) = &self.qa_state {
                            if let Some(messages) = &state.messages {
                                for message in messages {
                                    Self::qa_bubble(ui, &message.role, &message.content);
                                    last_was_assistant = message.role == "assistant";
                                }
                            }
                        }
                        // 流式增量与错误行紧跟在最后一条消息之后。
                        if let Some(state) = &self.qa_state {
                            if let Some(chunk) = &state.chunk {
                                if !chunk.is_empty() && !last_was_assistant {
                                    Self::qa_bubble(ui, "assistant", chunk);
                                }
                            }
                            if let Some(error) = &state.error {
                                ui20::widgets::status_pill(ui, error, ui20::widgets::Status::Err);
                            }
                            match state.kind {
                                QaStateKind::Recording => {
                                    ui20::widgets::status_pill(
                                        ui,
                                        "录音中…",
                                        ui20::widgets::Status::Warn,
                                    );
                                }
                                QaStateKind::Thinking => {
                                    ui20::widgets::status_pill(
                                        ui,
                                        "思考中…",
                                        ui20::widgets::Status::Info,
                                    );
                                }
                                _ => {}
                            }
                        }
                    });
            });
            ui.add_space(8.0);
            ui20::widgets::subtle_card(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.qa_input)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("输入问题，Enter 发送"),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let recording = self
                        .qa_state
                        .as_ref()
                        .is_some_and(|state| state.kind == QaStateKind::Recording);
                    if ui20::widgets::secondary_button(
                        ui,
                        if recording {
                            "结束录音"
                        } else {
                            "语音提问"
                        },
                        true,
                    )
                    .clicked()
                    {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.services().qa.toggle_recording().await?;
                                Ok("问答录音状态已更新".to_string())
                            });
                        }
                    }
                    if ui20::widgets::primary_button(ui, "发送", !self.qa_input.trim().is_empty())
                        .clicked()
                    {
                        if let Some(backend) = self.backend() {
                            let text = std::mem::take(&mut self.qa_input);
                            self.spawn(async move {
                                backend.services().qa.submit_text(text).await?;
                                Ok("问答已提交".to_string())
                            });
                        }
                    }
                    if ui20::widgets::secondary_button(ui, "取消本轮", true).clicked() {
                        if let Some(backend) = self.backend() {
                            let session_id = self
                                .qa_state
                                .as_ref()
                                .and_then(|state| state.session_id.as_deref())
                                .and_then(|id| uuid::Uuid::parse_str(id).ok())
                                .map(openless_core::SessionId::from_uuid);
                            self.spawn(async move {
                                backend.services().qa.cancel(session_id).await?;
                                Ok("问答本轮已取消".to_string())
                            });
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui20::widgets::danger_button(ui, "关闭会话", true).clicked() {
                            if let Some(backend) = self.backend() {
                                self.spawn(async move {
                                    backend.services().qa.dismiss().await?;
                                    Ok("问答已关闭".to_string())
                                });
                            }
                        }
                    });
                });
            });
        }

        /// QA 消息气泡：用户右对齐（蓝色软底）、助手左对齐（surface-2 软底），
        /// 对齐 #997 egui 弹窗与 React QaPanel 的双气泡布局。
        fn qa_bubble(ui: &mut egui::Ui, role: &str, content: &str) {
            let p = ui20::current(ui.ctx());
            let user = role.eq_ignore_ascii_case("user");
            let align = if user {
                egui::Layout::right_to_left(egui::Align::Min)
            } else {
                egui::Layout::left_to_right(egui::Align::Min)
            };
            ui.with_layout(align, |ui| {
                let max_width = ui.available_width() * 0.78;
                egui::Frame::default()
                    .fill(if user { p.blue_soft() } else { p.surface_2() })
                    .corner_radius(egui::CornerRadius::same(
                        openless_linux_egui::design_tokens::radius::MD,
                    ))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(max_width);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(content).size(14.0).color(if user {
                                    p.ink()
                                } else {
                                    p.ink_2()
                                }),
                            )
                            .wrap(),
                        );
                    });
            });
            ui.add_space(2.0);
        }

        fn selection_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "选区润色",
                Some("在目标应用选中文字后用选区润色快捷键触发；确认前可编辑或取消。"),
            );
            let Some(selection) = self.selection.clone() else {
                ui20::widgets::info_panel(ui, |ui| {
                    ui20::widgets::section_title(
                        ui,
                        "先在目标应用中选中文字，再使用已配置的选区润色快捷键。",
                    );
                    ui20::widgets::hint(
                        ui,
                        "预览会在此显示，确认前可以编辑或取消。此入口是现有 Selection polish；Selection Voice 的完整意图路由尚未接入。",
                    );
                });
                return;
            };
            ui20::widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    let (phase_label, phase_status) = match selection.phase {
                        SelectionPhase::Preview => ("待确认", ui20::widgets::Status::Warn),
                        SelectionPhase::Completed => ("已完成", ui20::widgets::Status::Ok),
                        _ => ("进行中", ui20::widgets::Status::Info),
                    };
                    ui20::widgets::status_pill(ui, phase_label, phase_status);
                });
                ui.add_space(6.0);
                if self.selection_preview_visible && selection.phase == SelectionPhase::Preview {
                    ui20::widgets::section_title(ui, "选区预览");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.selection_draft)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui20::widgets::primary_button(ui, "确认替换", true).clicked() {
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
                        if ui20::widgets::secondary_button(ui, "取消", true).clicked() {
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
                        ui.label(egui::RichText::new("最近一次选区替换已完成").size(14.0));
                        if ui20::widgets::secondary_button(ui, "撤销", true).clicked() {
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
            });
        }

        fn models_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui20::widgets::page_header(ui, "本地模型", None);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui20::widgets::secondary_button(ui, "刷新", true).clicked() {
                        self.models = ModelsState::Loading;
                        self.load_models();
                    }
                });
            });
            let models = match self.models.clone() {
                ModelsState::Loading => {
                    ui20::widgets::hint(ui, "正在加载模型目录…");
                    return;
                }
                ModelsState::Failed(error) => {
                    ui20::widgets::status_pill(ui, &error, ui20::widgets::Status::Err);
                    return;
                }
                ModelsState::Loaded(models) if models.is_empty() => {
                    ui20::widgets::info_panel(ui, |ui| {
                        ui20::widgets::hint(ui, "模型目录未返回任何可用模型");
                    });
                    return;
                }
                ModelsState::Loaded(models) => models,
            };
            for model in models {
                ui20::widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&model.display_name).size(15.0).strong());
                        ui20::widgets::status_pill(
                            ui,
                            if model.installed {
                                "已安装"
                            } else {
                                "未安装"
                            },
                            if model.installed {
                                ui20::widgets::Status::Ok
                            } else {
                                ui20::widgets::Status::Info
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui20::widgets::secondary_button(ui, "取消下载", true).clicked() {
                                if let Some(backend) = self.backend() {
                                    let target = model.target.clone();
                                    self.spawn(async move {
                                        backend
                                            .services()
                                            .local_asr
                                            .cancel_download(target)
                                            .await?;
                                        Ok("模型下载已取消".to_string())
                                    });
                                }
                            }
                            if model.installed
                                && ui20::widgets::primary_button(ui, "激活", true).clicked()
                            {
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
                                        let provider_type =
                                            descriptor.provider_type.as_str().to_string();
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
                                            .activate_local_asr(
                                                openless_core::LocalAsrActivationRequest {
                                                    target,
                                                    provider_id,
                                                },
                                            )
                                            .await?;
                                        Ok("本地模型已激活并预加载".to_string())
                                    });
                                }
                            }
                            if !model.installed
                                && ui20::widgets::blue_button(ui, "下载", true).clicked()
                            {
                                if let Some(backend) = self.backend() {
                                    let target = model.target.clone();
                                    self.spawn(async move {
                                        backend
                                            .services()
                                            .local_asr
                                            .start_download(target, None)
                                            .await?;
                                        Ok("模型下载完成".to_string())
                                    });
                                }
                            }
                        });
                    });
                    ui20::widgets::divider(ui);
                    ui20::widgets::kv_row(ui, "模型家族", &model.family);
                    if let Some(mode) = &model.mode {
                        ui20::widgets::kv_row(ui, "模式", mode);
                    }
                    if !model.languages.is_empty() {
                        ui20::widgets::kv_row(ui, "支持语言", &model.languages.join(" / "));
                    }
                    if let Some(size) = model.size_bytes {
                        ui20::widgets::kv_row(ui, "体积", &format!("{} MB", size / (1024 * 1024)));
                    }
                });
                ui.add_space(2.0);
            }
        }

        fn provider_management_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                ui20::widgets::section_title(ui, "凭据渠道");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui20::widgets::secondary_button(ui, "刷新渠道", true).clicked() {
                        self.providers = ProvidersState::Loading;
                        self.load_providers(self.provider_kind);
                    }
                });
            });
            ui.add_space(4.0);
            let kind_index = match self.provider_kind {
                openless_core::ChannelKind::Asr => 0,
                openless_core::ChannelKind::Llm => 1,
            };
            if let Some(picked) = ui20::widgets::segmented(
                ui,
                "provider-kind",
                &["ASR 语音识别", "LLM 文本处理"],
                kind_index,
            ) {
                let kind = match picked {
                    0 => openless_core::ChannelKind::Asr,
                    _ => openless_core::ChannelKind::Llm,
                };
                if self.provider_kind != kind {
                    self.provider_kind = kind;
                    self.providers = ProvidersState::Loading;
                    self.selected_channel_id = None;
                    self.pending_channel_delete = None;
                    self.provider_editor = ProviderEditorState::Idle;
                    self.provider_models.clear();
                    self.load_providers(kind);
                }
            }
            ui.add_space(6.0);

            let panel = match self.providers.clone() {
                ProvidersState::Loading => {
                    ui20::widgets::hint(ui, "正在读取 Core 渠道目录…");
                    return;
                }
                ProvidersState::Failed(error) => {
                    ui20::widgets::status_pill(ui, &error, ui20::widgets::Status::Err);
                    return;
                }
                ProvidersState::Loaded(panel) => panel,
            };

            ui20::widgets::subtle_card(ui, |ui| {
                ui20::widgets::section_title(ui, "新增渠道");
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

        fn services_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "AI 服务",
                Some("选择 ASR 语音识别、LLM 文本处理或 Omni 服务，再编辑并校验渠道。已配置不代表网络请求已通过。"),
            );
            if let Some(snapshot) = &self.snapshot {
                let credentials = &snapshot.credentials;
                ui20::widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui20::widgets::status_pill(
                            ui,
                            &format!(
                                "ASR：{}（{}）",
                                credentials.active_asr_provider,
                                if credentials.asr_configured {
                                    "已配置"
                                } else {
                                    "未配置"
                                }
                            ),
                            if credentials.asr_configured {
                                ui20::widgets::Status::Ok
                            } else {
                                ui20::widgets::Status::Warn
                            },
                        );
                        ui20::widgets::status_pill(
                            ui,
                            &format!(
                                "LLM：{}（{}）",
                                credentials.active_llm_provider,
                                if credentials.llm_configured {
                                    "已配置"
                                } else {
                                    "未配置"
                                }
                            ),
                            if credentials.llm_configured {
                                ui20::widgets::Status::Ok
                            } else {
                                ui20::widgets::Status::Warn
                            },
                        );
                    });
                });
                ui.add_space(8.0);
            }
            self.provider_management_ui(ui);
        }

        fn save_preferences(&mut self) {
            let (Some(native), Some(snapshot), Some(preferences)) =
                (&self.native, &self.snapshot, &self.preferences)
            else {
                return;
            };
            match native
                .host()
                .save_settings(preferences.clone(), snapshot.preferences_revision)
            {
                Ok(_) => {
                    self.status = "设置已保存".to_string();
                    let config = openless_core::RemoteInputConfig {
                        enabled: preferences.remote_input_enabled,
                        port: preferences.remote_input_port,
                    };
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.services().remote_input.configure(config).await?;
                            Ok("远程输入状态已更新".to_string())
                        });
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }

        fn settings_actions_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal_wrapped(|ui| {
                if ui20::widgets::primary_button(ui, "保存设置", true).clicked() {
                    self.save_preferences();
                }
                if ui20::widgets::secondary_button(ui, "放弃修改并重新读取", true).clicked()
                {
                    if let Some(backend) = self.backend() {
                        self.preferences = Some(backend.get_preferences());
                        self.snapshot = Some(backend.snapshot());
                        self.status = "已重新读取设置".to_string();
                    }
                }
            });
            ui20::widgets::hint(
                ui,
                "环境与设置、手机输入共用设置草稿；保存会一起应用。保存冲突时可重新读取后再修改。",
            );
        }

        fn settings_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "环境与设置",
                Some("环境与设置、手机输入共用设置草稿；保存会一起应用。"),
            );
            ui20::widgets::card(ui, |ui| {
                ui20::widgets::section_title(ui, "外观");
                let mut dark = self.ui_prefs.dark;
                ui20::widgets::toggle_row(ui, &mut dark, "深色主题", Some("2.0 默认浅色"));
                if dark != self.ui_prefs.dark {
                    self.ui_prefs.dark = dark;
                    self.ui_prefs.save();
                    ui20::apply_theme(ui.ctx(), dark);
                }
            });
            ui.add_space(8.0);
            ui20::widgets::card(ui, |ui| {
                ui20::widgets::section_title(ui, "现有功能设置");
                if let Some(preferences) = self.preferences.as_mut() {
                    let mut streaming = preferences.streaming_insert;
                    ui20::widgets::toggle_row(
                        ui,
                        &mut streaming,
                        "流式插入",
                        Some("将转写逐步发送到原输入目标，实际结果以听写与历史反馈为准。"),
                    );
                    preferences.streaming_insert = streaming;
                    let mut agent = preferences.coding_agent_enabled;
                    ui20::widgets::toggle_row(
                        ui,
                        &mut agent,
                        "启用 Less Computer",
                        Some("使用已有 Agent 配置与 CLI；进程执行仍遵循 Core 的审批规则。"),
                    );
                    preferences.coding_agent_enabled = agent;
                    ui.add_space(4.0);
                    self.settings_actions_ui(ui);
                } else {
                    ui20::widgets::hint(ui, "Core 未连接，暂无可编辑设置。");
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui20::widgets::secondary_button(ui, "配置 AI 服务", true).clicked() {
                        self.settings_modal = Some(Page::Services);
                    }
                    if ui20::widgets::secondary_button(ui, "设置手机输入", true).clicked() {
                        self.settings_modal = Some(Page::Remote);
                    }
                    if ui20::widgets::secondary_button(ui, "管理本地模型", true).clicked() {
                        self.settings_modal = Some(Page::Models);
                    }
                });
                ui20::widgets::hint(
                    ui,
                    "托盘、自启、自动更新、系统静音与额外全局热键尚未完整接入，此页没有对应开关。",
                );
            });
            ui.add_space(8.0);
            self.environment_ui(ui);
        }

        fn remote_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "手机输入",
                Some(
                    "先启用并保存，再让手机连接同一局域网，打开本机提供的 HTTPS 地址并输入配对码。",
                ),
            );
            ui20::widgets::card(ui, |ui| {
                ui20::widgets::section_title(ui, "远程输入");
                ui.label(
                    egui::RichText::new(
                        "首次连接需要确认并信任本服务的证书；服务运行不代表手机已连接。",
                    )
                    .size(13.0),
                );
                ui.add_space(4.0);
                if let Some(preferences) = self.preferences.as_mut() {
                    let mut enabled = preferences.remote_input_enabled;
                    ui20::widgets::toggle_row(ui, &mut enabled, "启用远程输入", None);
                    preferences.remote_input_enabled = enabled;
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("端口").size(14.0));
                        ui.add(
                            egui::DragValue::new(&mut preferences.remote_input_port)
                                .range(1..=u16::MAX),
                        );
                    });
                    self.settings_actions_ui(ui);
                } else {
                    ui20::widgets::hint(ui, "Core 未连接，暂不可配置远程输入。");
                }
            });
            ui.add_space(8.0);
            ui20::widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui20::widgets::section_title(ui, "连接状态");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui20::widgets::secondary_button(ui, "刷新连接状态", true).clicked()
                        {
                            self.load_remote_status();
                        }
                    });
                });
                ui.add_space(4.0);
                if let Some(error) = &self.remote_error {
                    ui20::widgets::status_pill(
                        ui,
                        &format!("暂时无法读取连接状态：{error}"),
                        ui20::widgets::Status::Warn,
                    );
                    ui20::widgets::hint(
                        ui,
                        "检查桌面密钥环、网络和端口后刷新；旧地址与配对码已隐藏。",
                    );
                } else if self.remote_access.is_none() {
                    ui20::widgets::hint(ui, "尚未取得手机输入状态。");
                }
                if let Some((remote, pin)) = &self.remote_access {
                    let running_status = if remote.running {
                        ("运行中", ui20::widgets::Status::Ok)
                    } else if remote.starting {
                        ("启动中", ui20::widgets::Status::Info)
                    } else {
                        ("已停止", ui20::widgets::Status::Warn)
                    };
                    ui.horizontal(|ui| {
                        ui20::widgets::status_pill(ui, "远程输入服务", running_status.1);
                        ui.label(egui::RichText::new(running_status.0).size(13.0));
                    });
                    ui20::widgets::kv_row(ui, "当前连接数", &remote.connection_count.to_string());
                    if remote.active_session_id.is_some() {
                        ui20::widgets::hint(ui, "手机语音会话进行中，可使用语音取消。");
                    }
                    if remote.urls_stale {
                        ui20::widgets::status_pill(
                            ui,
                            "网络地址已过期，请检查网络后刷新状态",
                            ui20::widgets::Status::Warn,
                        );
                    }
                    if remote.enabled && remote.running && !remote.urls_stale {
                        // 首次信任前必须核对根证书指纹：网页与描述文件名称不能证明身份。
                        ui.label("本机根证书 SHA-256");
                        match remote.ca_fingerprint_sha256.as_ref().filter(|value| {
                            value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                        }) {
                            Some(fingerprint) => {
                                let display = fingerprint
                                    .as_bytes()
                                    .chunks(2)
                                    .map(|pair| {
                                        std::str::from_utf8(pair).unwrap().to_ascii_uppercase()
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&display).monospace())
                                        .wrap(),
                                );
                                if ui.button("复制完整指纹").clicked() {
                                    ui.ctx().copy_text(display);
                                }
                            }
                            // 两臂统一为 ()：None 臂不能返回 Response（match 臂类型
                            // 必须一致；这是 #1048 移植后未被编译验证过的潜伏错误）。
                            None => {
                                ui.label("完整指纹不可用。请勿安装或信任下载的证书。");
                            }
                        }
                        ui.label("安装或开启完全信任前，在手机系统的证书详情中核对全部 SHA-256 字符，必须与此处一致。网页、描述文件名称和标识不能证明证书身份。若不一致或无法查看，请停止并移除已下载或安装的描述文件。");
                        ui.label("描述文件应只包含一张根证书。若有其他证书、VPN 或设备管理配置，请勿安装。首次下载仍可能被局域网攻击者替换；核验后再信任。根证书可签发其他证书，不再使用时请移除。");
                        ui.monospace(format!("PIN：{pin}"));
                        for url in &remote.urls {
                            ui.monospace(url);
                        }
                        if remote.urls.is_empty() {
                            ui.label("服务已启动，但尚未提供可用地址；请检查本机局域网连接。");
                        }
                    }
                    if remote.enabled && ui.button("重置配对码").clicked() {
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
            });
        }

        fn environment_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::card(ui, |ui| {
                ui20::widgets::section_title(ui, "Linux 环境准备");
                let connected = self.native.is_some();
                ui20::widgets::status_pill(
                    ui,
                    if connected {
                        "Core 已连接"
                    } else {
                        "Core 未连接"
                    },
                    if connected {
                        ui20::widgets::Status::Ok
                    } else {
                        ui20::widgets::Status::Warn
                    },
                );
                ui20::widgets::hint(
                    ui,
                    if connected {
                        "下面的环境检查不代表录音、落字或服务调用已经实测成功。"
                    } else {
                        "可查看准备步骤；修复启动问题后，请退出并重新启动 OpenLess。"
                    },
                );
                ui20::widgets::divider(ui);
                if let Some(environment) = &self.environment {
                    ui20::widgets::kv_row(
                        ui,
                        "桌面会话",
                        match environment.session {
                            LinuxDesktopSession::X11 => "检测到 X11 环境",
                            LinuxDesktopSession::Wayland => "检测到 Wayland 环境",
                            LinuxDesktopSession::Headless => "未检测到 DISPLAY / WAYLAND_DISPLAY",
                        },
                    );
                    ui20::widgets::kv_row(
                        ui,
                        "fcitx5",
                        if environment.fcitx5_ready {
                            "D-Bus 探测有响应；插件加载、快捷键与落字仍需实际确认"
                        } else {
                            "D-Bus 探测未通过，可能是会话总线、服务或插件未就绪"
                        },
                    );
                    ui20::widgets::kv_row(
                        ui,
                        "麦克风",
                        match environment.permissions.microphone {
                            openless_core::PermissionState::Unsupported => {
                                "当前探测环境不支持；请进入图形桌面会话"
                            }
                            _ => "尚未验证录音；请在系统声音设置选择输入设备后短听写",
                        },
                    );
                } else {
                    ui20::widgets::hint(ui, "尚未取得桌面环境探测结果。");
                }
                ui20::widgets::kv_row(
                    ui,
                    "本次启动插件检查",
                    match &self.plugin_check {
                        Some(Ok(FcitxPluginStatus::Ready)) => {
                            "找到插件文件；文件存在不代表 fcitx5 已加载它"
                        }
                        Some(Ok(FcitxPluginStatus::Updated)) => {
                            "插件文件已安装或更新，需要重载配置并重新启动 fcitx5"
                        }
                        Some(Ok(FcitxPluginStatus::Missing)) => {
                            "未找到插件文件，请重新安装含 OpenLess 插件的软件包"
                        }
                        Some(Err(_)) => "检查失败，请查看下方具体原因",
                        None => "未执行",
                    },
                );
                if let Some(Err(error)) = &self.plugin_check {
                    ui20::widgets::status_pill(ui, error, ui20::widgets::Status::Warn);
                }
                ui.add_space(4.0);
                if ui20::widgets::secondary_button(
                    ui,
                    if self.environment_refreshing {
                        "正在检测…"
                    } else {
                        "重新检测会话与 D-Bus"
                    },
                    !self.environment_refreshing,
                )
                .clicked()
                {
                    self.environment_refreshing = true;
                    let tx = self.tx.clone();
                    self.tokio.spawn_blocking(move || {
                        let environment = LinuxCapabilitySnapshot::detect(false, package_kind());
                        let _ = tx.send(UiResult::Environment(environment));
                    });
                }
                ui20::widgets::hint(
                    ui,
                    "重新检测只更新上面的会话与 D-Bus 信息，不安装插件，也不重新连接 Core。本次启动检查结果保留到退出。",
                );
            });
            ui.add_space(6.0);
            egui::CollapsingHeader::new("准备步骤与官方指南")
                .default_open(self.native.is_none())
                .show(ui, |ui| {
            ui.separator();
            ui.strong("1 · 准备输入法与桌面会话");
            ui.label("在当前图形桌面安装并启用 fcitx5，再安装含 OpenLess 插件的当前软件包。先在普通编辑器中确认输入法可以输入。");
            ui.label("在终端运行以下诊断，查看输入法环境与插件加载信息：");
            command_ui(ui, "fcitx5-diagnose");
            ui.label("插件安装或更新后可先重载配置；若插件仍未加载，退出并重新登录桌面，再启动 OpenLess：");
            command_ui(ui, "fcitx5-remote -r");
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to(
                    "Fcitx 5 官方设置指南",
                    "https://fcitx-im.org/wiki/Setup_Fcitx_5",
                );
                ui.hyperlink_to(
                    "Wayland 桌面配置差异",
                    "https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland",
                );
            });
            ui.small("Wayland 的输入法配置取决于桌面和应用工具包，请按官方对应章节配置；检测到 Wayland 不代表所有目标应用都支持替换。X11 的 overlay 能力标记也不代表本应用已接入录音浮层。");
            ui.separator();
            ui.strong("2 · 准备密钥环与识别服务");
            ui.label("Secret Service：当前没有独立的服务连接或解锁状态检测；渠道显示“已配置”也不能证明密钥环现在可读写。");
            ui.label("打开桌面的密码／密钥环管理器，确认当前登录会话的密钥环已解锁。然后到 AI 服务选择渠道，填写所需凭据、保存并校验；若返回锁定或访问失败，解锁后重试。");
            ui.hyperlink_to(
                "Secret Service 官方规范",
                "https://specifications.freedesktop.org/secret-service/latest/",
            );
            ui.small("API 密钥输入只用于写入，不回显已有密钥。本地识别可在本地模型页下载并激活 Generic Qwen。");
            ui.separator();
            ui.strong("3 · 做一次短听写");
            ui.label("在系统声音设置确认输入设备有电平。配置识别服务后，在目标编辑器聚焦输入框，用已有听写快捷键录制一句话并结束，检查转写和落字结果。问答、选区润色与 Agent 分别从导航进入。");
            ui.small("请分别验证你使用的 X11／Wayland、GTK／Qt／浏览器／终端。托盘、自启和应用内自动更新仍未完整接入。");
                });
        }

        fn start_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "从一次听写开始",
                Some("先准备 Linux 输入环境，再选择识别服务。切换页面不会停止正在进行的任务。"),
            );
            if let Some(error) = &self.startup_error {
                ui20::widgets::status_pill(
                    ui,
                    &format!("启动未完成：{error}"),
                    ui20::widgets::Status::Warn,
                );
                ui.add_space(6.0);
            }
            if let Some(snapshot) = &self.snapshot {
                ui20::widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui20::widgets::status_pill(
                            ui,
                            if snapshot.running {
                                "Core 运行中"
                            } else {
                                "Core 未运行"
                            },
                            if snapshot.running {
                                ui20::widgets::Status::Ok
                            } else {
                                ui20::widgets::Status::Warn
                            },
                        );
                    });
                    ui20::widgets::divider(ui);
                    let credentials = &snapshot.credentials;
                    match credentials.pipeline_mode {
                        openless_core::shared_types::PipelineMode::Multimodal => {
                            ui20::widgets::kv_row(ui, "当前管线", "多模态（Omni）");
                            ui20::widgets::kv_row(
                                ui,
                                "Omni",
                                if credentials.omni_configured {
                                    "已配置"
                                } else {
                                    "尚未配置，请到 AI 服务配置 Omni"
                                },
                            );
                        }
                        openless_core::shared_types::PipelineMode::Traditional => {
                            ui20::widgets::kv_row(ui, "当前管线", "传统（ASR + LLM）");
                            ui20::widgets::kv_row(
                                ui,
                                "ASR 语音识别",
                                if credentials.asr_configured {
                                    "已配置"
                                } else {
                                    "尚未配置，请配置 AI 服务或激活本地模型"
                                },
                            );
                            ui20::widgets::kv_row(
                                ui,
                                "LLM 润色",
                                if credentials.llm_configured {
                                    "已配置"
                                } else {
                                    "尚未配置"
                                },
                            );
                        }
                    }
                    ui20::widgets::hint(ui, "已配置不代表校验通过；请到 AI 服务验证连接。");
                });
                ui.add_space(8.0);
            }
            ui20::widgets::subtle_card(ui, |ui| {
                ui20::widgets::section_title(ui, "快速开始");
                ui.horizontal_wrapped(|ui| {
                    for (page, label) in [
                        (Page::Settings, "1. 准备环境"),
                        (Page::Services, "2. 配置 AI 服务"),
                        (Page::Models, "使用本地模型"),
                        (Page::Dictation, "3. 打开听写"),
                    ] {
                        let enabled = self.native.is_some() || page == Page::Settings;
                        if ui20::widgets::secondary_button(ui, label, enabled).clicked() {
                            if matches!(page, Page::Settings | Page::Services | Page::Models) {
                                self.navigation.open(Page::Start);
                                self.settings_modal = Some(page);
                            } else {
                                self.navigation.open(page);
                            }
                        }
                    }
                });
            });
            ui.add_space(8.0);
            if self.native.is_none() {
                self.environment_ui(ui);
            } else {
                ui20::widgets::card(ui, |ui| {
                    ui20::widgets::section_title(ui, "继续其他任务");
                    ui.horizontal_wrapped(|ui| {
                        for page in [Page::Qa, Page::Selection, Page::Agent, Page::History] {
                            if ui20::widgets::secondary_button(ui, page.label(), true).clicked() {
                                self.navigation.open(page);
                            }
                        }
                        if ui20::widgets::secondary_button(ui, "手机输入", true).clicked() {
                            self.settings_modal = Some(Page::Remote);
                        }
                    });
                    ui20::widgets::hint(
                        ui,
                        "问答支持文字与语音；选区润色保留确认、取消与撤销；Less Computer 的工具执行继续使用原有审批。",
                    );
                    ui20::widgets::hint(
                        ui,
                        "Linux 当前提供已有 Core / Host 能力的入口，完整原生支持与发布验收仍在继续。",
                    );
                });
            }
        }

        fn page_activity(&self, page: Page) -> Option<&'static str> {
            match page {
                Page::Dictation
                    if self.snapshot.as_ref().is_some_and(|snapshot| {
                        matches!(
                            snapshot.dictation.phase,
                            DictationPhase::Starting
                                | DictationPhase::Recording
                                | DictationPhase::Transcribing
                                | DictationPhase::Polishing
                                | DictationPhase::Inserting
                        )
                    }) =>
                {
                    Some("进行中")
                }
                Page::Qa if self.qa_visible => Some("会话"),
                Page::Selection
                    if self.selection_preview_visible
                        && self.selection.as_ref().is_some_and(|selection| {
                            selection.phase == SelectionPhase::Preview
                        }) =>
                {
                    Some("待确认")
                }
                Page::Agent if self.pending_approval.is_some() => Some("待审批"),
                Page::Agent if self.less_computer_running => Some("进行中"),
                _ if self.navigation.has_update(page) => Some("有更新"),
                _ => None,
            }
        }

        /// 2.0 主侧栏：226px、`--ol-sidebar-bg` 底、版本行 + 导航 + 底部设置。
        /// 服务商 / 本地模型 / 手机输入 / 环境设置与其他平台一致，收纳进设置弹窗。
        fn sidebar_ui(&mut self, ctx: &egui::Context) {
            let p = ui20::current(ctx);
            egui::SidePanel::left("navigation")
                .resizable(false)
                .exact_width(openless_linux_egui::design_tokens::SIDEBAR_WIDTH)
                .frame(
                    egui::Frame::default()
                        .fill(p.sidebar_bg())
                        .stroke(egui::Stroke::new(0.5_f32, p.line()))
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: 14,
                            bottom: 12,
                        }),
                )
                .show(ctx, |ui| {
                    egui::TopBottomPanel::bottom("navigation_footer")
                        .frame(egui::Frame::default())
                        .show_inside(ui, |ui| {
                            ui20::widgets::divider(ui);
                            if !self.status.is_empty() {
                                let status = self.status.clone();
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(status).size(11.5).color(p.ink_4()),
                                    )
                                    .truncate(),
                                );
                            }
                            self.nav_settings_item(ui);
                        });
                    egui::ScrollArea::vertical()
                        .id_salt("navigation_scroll")
                        .show(ui, |ui| {
                            self.version_row(ui);
                            ui.add_space(6.0);
                            self.nav_item(ui, Page::Start, false);
                            self.nav_item(ui, Page::Dictation, false);
                            self.nav_item(ui, Page::History, false);
                            ui.add_space(2.0);
                            // 「工具」可展开分组（React 侧栏同款交互：点标题展开/收起）。
                            let group_active = self.settings_modal.is_none()
                                && matches!(
                                    self.navigation.page,
                                    Page::Qa | Page::Selection | Page::Agent
                                );
                            egui::CollapsingHeader::new(
                                egui::RichText::new("工具").size(12.5).color(p.ink_4()),
                            )
                            .default_open(true)
                            .show_unindented(ui, |ui| {
                                if group_active {
                                    ui.add_space(0.0);
                                }
                                self.nav_item(ui, Page::Qa, true);
                                self.nav_item(ui, Page::Selection, true);
                                self.nav_item(ui, Page::Agent, true);
                            });
                        });
                });
        }

        /// 版本行：品牌 + BETA 徽章 + 版本号（React `shell.footer.version` 同款）。
        /// 版本显示 Core 后端合同版本，与 Tauri 端读取的合同保持同源。
        fn version_row(&mut self, ui: &mut egui::Ui) {
            let p = ui20::current(ui.ctx());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("OpenLess")
                        .size(13.0)
                        .strong()
                        .color(p.ink()),
                );
                egui::Frame::default()
                    .stroke(egui::Stroke::new(0.5_f32, p.blue()))
                    .corner_radius(egui::CornerRadius::same(5))
                    .inner_margin(egui::Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("BETA")
                                .size(10.0)
                                .strong()
                                .color(p.blue()),
                        );
                    });
            });
            ui.label(
                egui::RichText::new(format!("版本 {}", openless_core::BACKEND_CONTRACT_VERSION))
                    .size(12.0)
                    .color(p.ink_4()),
            );
        }

        /// 2.0 导航项：`.ol-nav-btn` 规格（8px 10px 内边距、8px 圆角、15px 文字）；
        /// 选中 = surface-2 底 + ink 文字，未选中 = ink-3。
        fn nav_item(&mut self, ui: &mut egui::Ui, page: Page, indent: bool) {
            let p = ui20::current(ui.ctx());
            let active = self.settings_modal.is_none() && self.navigation.page == page;
            let activity = self.page_activity(page);
            let frame = egui::Frame::default()
                .fill(if active {
                    p.surface_2()
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin {
                    left: if indent { 30 } else { 10 },
                    right: 10,
                    top: 8,
                    bottom: 8,
                });
            let response = frame
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(page.label())
                                .size(15.0)
                                .color(if active { p.ink() } else { p.ink_3() }),
                        );
                        if let Some(activity) = activity {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui20::widgets::pill(ui, activity, p.blue_soft(), p.blue());
                                },
                            );
                        }
                    });
                })
                .response;
            let response = ui.interact(
                response.rect,
                ui.id().with(("nav", page)),
                egui::Sense::click(),
            );
            if response.clicked() {
                self.navigation.open(page);
                self.settings_modal = None;
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        /// 侧栏底部「设置」入口：打开 2.0 设置弹窗（含 AI 服务/本地模型/手机输入）。
        fn nav_settings_item(&mut self, ui: &mut egui::Ui) {
            let p = ui20::current(ui.ctx());
            let active = self.settings_modal.is_some();
            let frame = egui::Frame::default()
                .fill(if active {
                    p.surface_2()
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin {
                    left: 10,
                    right: 10,
                    top: 8,
                    bottom: 8,
                });
            let response = frame
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("设置").size(15.0).color(if active {
                            p.ink()
                        } else {
                            p.ink_3()
                        }));
                    });
                })
                .response;
            let response = ui.interact(
                response.rect,
                ui.id().with("nav-settings"),
                egui::Sense::click(),
            );
            if response.clicked() {
                self.settings_modal = Some(Page::Settings);
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        /// 紧凑窗口（<760px）下的横向导航：胶囊式主页面 + 设置入口。
        fn compact_navigation_ui(&mut self, ctx: &egui::Context) {
            let p = ui20::current(ctx);
            egui::TopBottomPanel::top("compact_navigation")
                .frame(
                    egui::Frame::default()
                        .fill(p.surface())
                        .stroke(egui::Stroke::new(0.5_f32, p.line()))
                        .inner_margin(egui::Margin::symmetric(10, 8)),
                )
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for page in [
                            Page::Start,
                            Page::Dictation,
                            Page::History,
                            Page::Qa,
                            Page::Selection,
                            Page::Agent,
                        ] {
                            self.compact_nav_chip(ui, page);
                        }
                        let modal_open = self.settings_modal.is_some();
                        if ui20::widgets::pill(
                            ui,
                            "设置",
                            if modal_open {
                                p.surface_2()
                            } else {
                                egui::Color32::TRANSPARENT
                            },
                            p.ink_3(),
                        )
                        .interact(egui::Sense::click())
                        .clicked()
                        {
                            self.settings_modal = Some(Page::Settings);
                        }
                    });
                });
        }

        fn compact_nav_chip(&mut self, ui: &mut egui::Ui, page: Page) {
            let p = ui20::current(ui.ctx());
            let active = self.settings_modal.is_none() && self.navigation.page == page;
            let response = ui20::widgets::pill(
                ui,
                page.label(),
                if active {
                    p.surface_2()
                } else {
                    egui::Color32::TRANSPARENT
                },
                if active { p.ink() } else { p.ink_3() },
            )
            .interact(egui::Sense::click());
            if response.clicked() {
                self.navigation.open(page);
                self.settings_modal = None;
            }
        }

        /// 2.0 设置弹窗：遮罩 + 居中 960x680 面板（14px 圆角、`--ol-settings-*`），
        /// 左侧 rail 分区导航、右侧分区内容。分区复用各页面既有渲染函数。
        fn settings_modal_ui(&mut self, ctx: &egui::Context) {
            let Some(section) = self.settings_modal else {
                return;
            };
            let p = ui20::current(ctx);
            let screen = ctx.screen_rect();
            // 遮罩：`--ol-overlay-bg`。Middle 层在面板之后、弹窗（Foreground）之下。
            let overlay_layer =
                egui::LayerId::new(egui::Order::Middle, egui::Id::new("settings-overlay"));
            ctx.layer_painter(overlay_layer).rect_filled(
                screen,
                egui::CornerRadius::same(0),
                p.overlay(),
            );

            let size = egui::vec2(
                (screen.width() * 0.94).min(openless_linux_egui::design_tokens::SETTINGS_MAX_WIDTH),
                (screen.height() * 0.92)
                    .min(openless_linux_egui::design_tokens::SETTINGS_MAX_HEIGHT),
            );
            let mut open = true;
            egui::Window::new("settings-modal")
                .id(egui::Id::new("settings-modal"))
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .movable(false)
                .fixed_size(size)
                .current_pos(egui::pos2(
                    screen.center().x - size.x / 2.0,
                    screen.center().y - size.y / 2.0,
                ))
                .frame(
                    egui::Frame::default()
                        .fill(p.settings_content_bg())
                        .corner_radius(egui::CornerRadius::same(
                            openless_linux_egui::design_tokens::radius::CARD,
                        ))
                        .stroke(egui::Stroke::new(0.5_f32, p.line()))
                        .inner_margin(egui::Margin::same(0)),
                )
                .show(ctx, |ui| {
                    egui::TopBottomPanel::top("settings-header")
                        .frame(
                            egui::Frame::default()
                                .fill(p.surface())
                                .stroke(egui::Stroke::new(0.5_f32, p.line()))
                                .inner_margin(egui::Margin {
                                    left: 18,
                                    right: 14,
                                    top: 12,
                                    bottom: 12,
                                }),
                        )
                        .show_inside(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("设置")
                                        .size(17.0)
                                        .strong()
                                        .color(p.ink()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui20::widgets::secondary_button(ui, "关闭", true)
                                            .clicked()
                                        {
                                            open = false;
                                        }
                                    },
                                );
                            });
                        });
                    egui::SidePanel::left("settings-rail")
                        .resizable(false)
                        .exact_width(196.0)
                        .frame(
                            egui::Frame::default()
                                .fill(p.settings_rail_bg())
                                .stroke(egui::Stroke::new(0.5_f32, p.line()))
                                .inner_margin(egui::Margin {
                                    left: 10,
                                    right: 10,
                                    top: 14,
                                    bottom: 10,
                                }),
                        )
                        .show_inside(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("settings-rail-scroll")
                                .show(ui, |ui| {
                                    for rail_page in
                                        [Page::Settings, Page::Services, Page::Models, Page::Remote]
                                    {
                                        self.settings_rail_item(ui, rail_page, section);
                                    }
                                });
                        });
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::default()
                                .fill(p.settings_content_bg())
                                .inner_margin(egui::Margin::same(20)),
                        )
                        .show_inside(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt(("settings-section", section))
                                .show(ui, |ui| {
                                    if self.native.is_none() && !matches!(section, Page::Settings) {
                                        ui20::widgets::page_header(ui, section.label(), None);
                                        ui.label("Core 尚未连接，此分区需要 Core 启动后才能使用。");
                                        if ui20::widgets::blue_button(ui, "前往环境准备", true)
                                            .clicked()
                                        {
                                            self.settings_modal = Some(Page::Settings);
                                        }
                                        return;
                                    }
                                    match section {
                                        Page::Settings => self.settings_ui(ui),
                                        Page::Services => self.services_ui(ui),
                                        Page::Models => self.models_ui(ui),
                                        Page::Remote => self.remote_ui(ui),
                                        _ => {}
                                    }
                                });
                        });
                });
            if !open {
                self.settings_modal = None;
            }
        }

        /// 设置弹窗左 rail 的分区项。
        fn settings_rail_item(&mut self, ui: &mut egui::Ui, page: Page, section: Page) {
            let p = ui20::current(ui.ctx());
            let active = section == page;
            let frame = egui::Frame::default()
                .fill(if active {
                    p.surface()
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin {
                    left: 10,
                    right: 10,
                    top: 8,
                    bottom: 8,
                });
            let response = frame
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(page.label())
                                .size(14.0)
                                .color(if active { p.ink() } else { p.ink_3() }),
                        );
                    });
                })
                .response;
            let response = ui.interact(
                response.rect,
                ui.id().with(("settings-rail", page)),
                egui::Sense::click(),
            );
            if response.clicked() {
                self.settings_modal = Some(page);
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        fn activity_ui(&mut self, ui: &mut egui::Ui) {
            ui.horizontal_wrapped(|ui| {
                for page in Page::ALL {
                    if let Some(activity) = self.page_activity(page) {
                        if ui
                            .link(format!("{} · {activity} →", page.label()))
                            .clicked()
                        {
                            self.navigation.open(page);
                        }
                    }
                }
                if self.native.is_some() && ui.button("取消当前语音 · Esc").clicked() {
                    self.cancel_voice();
                }
                if self.qa_visible && ui.button("取消问答").clicked() {
                    if let Some(backend) = self.backend() {
                        let session_id = self
                            .qa_state
                            .as_ref()
                            .and_then(|state| state.session_id.as_deref())
                            .and_then(|id| uuid::Uuid::parse_str(id).ok())
                            .map(openless_core::SessionId::from_uuid);
                        self.spawn(async move {
                            backend.services().qa.cancel(session_id).await?;
                            Ok("问答本轮已取消".to_string())
                        });
                    }
                }
                if let Some(session_id) = self
                    .selection
                    .as_ref()
                    .filter(|selection| selection.phase == SelectionPhase::Preview)
                    .and_then(|selection| selection.session_id)
                {
                    if ui.button("取消选区预览").clicked() {
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
                }
                if (self.less_computer_running || self.pending_approval.is_some())
                    && ui.button("取消 Agent").clicked()
                {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.cancel_less_computer(None).await?;
                            Ok("Less Computer 已取消".to_string())
                        });
                    }
                }
            });
        }

        fn cancel_voice(&self) {
            if let Some(backend) = self.backend() {
                self.spawn(async move {
                    backend.cancel_active_voice_session(None).await?;
                    Ok("语音会话已取消".to_string())
                });
            }
        }

        fn history_ui(&mut self, ui: &mut egui::Ui) {
            ui20::widgets::page_header(
                ui,
                "历史",
                Some("最近 20 条，只读。插入、复制回退与已发送粘贴分别显示实际结果。"),
            );
            let Some(backend) = self.backend() else {
                return;
            };
            match backend.list_history() {
                Ok(history) if history.is_empty() => {
                    ui20::widgets::info_panel(ui, |ui| {
                        ui20::widgets::hint(ui, "暂无历史记录");
                    });
                }
                Ok(history) => {
                    for item in history.into_iter().rev().take(20) {
                        let delivery = match item.insert_status {
                            HistoryInsertStatus::Inserted => ("已插入", ui20::widgets::Status::Ok),
                            HistoryInsertStatus::CopiedFallback => {
                                ("已复制", ui20::widgets::Status::Info)
                            }
                            HistoryInsertStatus::PasteSent => {
                                ("已发送粘贴", ui20::widgets::Status::Info)
                            }
                            HistoryInsertStatus::Failed => ("失败", ui20::widgets::Status::Err),
                            HistoryInsertStatus::NotRequested => {
                                ("未请求插入", ui20::widgets::Status::Warn)
                            }
                        };
                        ui20::widgets::card(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&item.created_at)
                                        .size(12.5)
                                        .color(ui20::current(ui.ctx()).ink_4()),
                                );
                                ui20::widgets::status_pill(ui, delivery.0, delivery.1);
                            });
                            ui.add_space(2.0);
                            ui.add(
                                egui::Label::new(egui::RichText::new(&item.final_text).size(14.0))
                                    .wrap(),
                            );
                        });
                        ui.add_space(2.0);
                    }
                }
                Err(error) => {
                    ui20::widgets::status_pill(ui, &error.to_string(), ui20::widgets::Status::Err);
                }
            }
        }
    }

    impl eframe::App for OpenLessEguiApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // Poll every frame before routing pages. Hidden pages retain their
            // drafts, session ownership, event replay and async completion paths.
            self.poll(ctx);
            // 设置弹窗打开时 Esc 只关弹窗；关闭后才恢复全局语音取消。
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                if self.settings_modal.is_some() {
                    self.settings_modal = None;
                } else {
                    self.cancel_voice();
                }
            }
            let compact = ctx.screen_rect().width() < 760.0;
            if compact {
                self.compact_navigation_ui(ctx);
            } else {
                self.sidebar_ui(ctx);
            }
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::default()
                        .fill(ui20::current(ctx).surface())
                        .inner_margin(egui::Margin {
                            left: 20,
                            right: 20,
                            top: 14,
                            bottom: 14,
                        }),
                )
                .show(ctx, |ui| {
                    let page = self.navigation.page;
                    // 跨页审批：Agent 页自身渲染审批卡，其余页面顶部保留入口。
                    if self.pending_approval.is_some() && page != Page::Agent {
                        ui20::widgets::info_panel(ui, |ui| {
                            ui20::widgets::section_title(ui, "Less Computer 等待审批");
                            self.agent_approval_ui(ui);
                        });
                        ui.add_space(8.0);
                    }
                    egui::ScrollArea::vertical()
                        .id_salt(("page", page))
                        .show(ui, |ui| {
                            if self.native.is_none()
                                && !matches!(page, Page::Start | Page::Settings)
                            {
                                ui20::widgets::page_header(ui, page.label(), None);
                                ui.label("Core 尚未连接，请先完成 Linux 环境准备并重新启动应用。");
                                if let Some(error) = &self.startup_error {
                                    ui20::widgets::status_pill(
                                        ui,
                                        &format!("启动未完成：{error}"),
                                        ui20::widgets::Status::Warn,
                                    );
                                }
                                if ui20::widgets::blue_button(ui, "查看环境准备步骤", true)
                                    .clicked()
                                {
                                    self.navigation.open(Page::Start);
                                    self.settings_modal = Some(Page::Settings);
                                }
                                return;
                            }
                            match page {
                                Page::Start => self.start_ui(ui),
                                Page::Dictation => self.dictation_ui(ui),
                                Page::Qa => self.qa_ui(ui),
                                Page::Selection => self.selection_ui(ui),
                                Page::Agent => self.less_computer_ui(ui),
                                Page::Services => self.services_ui(ui),
                                Page::Models => self.models_ui(ui),
                                Page::Remote => self.remote_ui(ui),
                                Page::History => self.history_ui(ui),
                                Page::Settings => self.settings_ui(ui),
                            }
                        });
                });
            if self.settings_modal.is_some() {
                self.settings_modal_ui(ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn command_ui(ui: &mut egui::Ui, command: &str) {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(command);
            if ui.button("复制命令").clicked() {
                ui.ctx().copy_text(command.to_string());
            }
        });
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
            openless_core::AuthRequirement::TencentCloud => "腾讯云 AppID + SecretID + SecretKey",
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
        let app_id = if descriptor.auth_requirement == openless_core::AuthRequirement::TencentCloud
        {
            read_provider_value(
                &backend,
                kind,
                &channel.id,
                openless_core::credentials::TENCENT_CLOUD_APP_ID_ACCOUNT,
            )
            .await?
            .unwrap_or_default()
        } else {
            String::new()
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
            app_id,
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
            openless_core::AuthRequirement::TencentCloud => {
                ui.horizontal(|ui| {
                    ui.label("腾讯云 AppID");
                    ui.text_edit_singleline(&mut editor.app_id);
                });
                secret_edit(ui, "SecretID", &mut editor.primary_secret);
                secret_edit(ui, "SecretKey", &mut editor.secondary_secret);
                ui.horizontal(|ui| {
                    ui.label("Model");
                    ui.text_edit_singleline(&mut editor.model);
                });
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
            openless_core::AuthRequirement::TencentCloud => {
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::TENCENT_CLOUD_APP_ID_ACCOUNT,
                    &editor.app_id,
                )
                .await?;
                write_secret_if_entered(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::TENCENT_CLOUD_SECRET_ID_ACCOUNT,
                    &editor.primary_secret,
                )
                .await?;
                write_secret_if_entered(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
                    &editor.secondary_secret,
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
            openless_core::AuthRequirement::TencentCloud => &[
                openless_core::credentials::TENCENT_CLOUD_APP_ID_ACCOUNT,
                openless_core::credentials::TENCENT_CLOUD_SECRET_ID_ACCOUNT,
                openless_core::credentials::TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
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
                thinking_enabled: backend.get_preferences().llm_thinking_enabled,
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

    fn package_kind() -> LinuxPackageKind {
        if std::env::var_os("APPDIR").is_some() {
            LinuxPackageKind::AppImage
        } else if cfg!(debug_assertions) {
            LinuxPackageKind::Development
        } else {
            LinuxPackageKind::SystemPackage
        }
    }

    fn backend_config() -> Result<BackendConfig, String> {
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
        let capabilities = LinuxCapabilitySnapshot::detect(false, kind).capabilities;
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

    fn ensure_fcitx5_ready(config: &BackendConfig) -> Result<FcitxPluginStatus, String> {
        let home = config
            .home_dir
            .as_deref()
            .ok_or_else(|| "HOME is unavailable for the fcitx5 plugin".to_string())?;
        let layout = LinuxResourceLayout::detect(None).map_err(|error| error.to_string())?;
        let plan =
            FcitxPluginInstallPlan::for_layout(&layout, home).map_err(|error| error.to_string())?;
        ensure_fcitx5_plugin_installed(&plan).map_err(|error| error.to_string())
    }

    pub fn run() -> Result<(), String> {
        let tokio = Arc::new(tokio::runtime::Runtime::new().map_err(|error| error.to_string())?);
        let config = backend_config()?;
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| config.cache_dir.join("runtime"));
        let args = std::env::args().collect::<Vec<_>>();
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
        let plugin_check = ensure_fcitx5_ready(&config);
        let environment = LinuxCapabilitySnapshot::detect(false, package_kind());
        let native = (|| {
            // AppImage may need to materialize its bundled plugin into the
            // per-user fcitx5 search path. Do that before opening the DBus
            // listener: otherwise the first run can wait forever for signals
            // from a plugin fcitx5 has never loaded.
            match &plugin_check {
                Ok(FcitxPluginStatus::Ready) => {}
                Ok(FcitxPluginStatus::Updated) => return Err(
                    "fcitx5 插件已安装或更新；请重载配置（fcitx5-remote -r），重新启动 fcitx5 或重新登录桌面，再启动 OpenLess".to_string()
                ),
                Ok(FcitxPluginStatus::Missing) => return Err(
                    "未找到 OpenLess fcitx5 插件；请重新安装当前软件包".to_string()
                ),
                Err(error) => return Err(error.clone()),
            }
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
                .with_inner_size([1040.0, 760.0])
                .with_min_inner_size([420.0, 400.0]),
            ..Default::default()
        };
        eframe::run_native(
            "OpenLess",
            options,
            Box::new(move |cc| {
                let mut app = OpenLessEguiApp::new(tokio, native);
                app.environment = Some(environment);
                app.plugin_check = Some(plugin_check);
                // 2.0 主题与 CJK 字体：默认白底体系（偏好持久化在 ui-prefs.json）。
                ui20::install_cjk_fonts(&cc.egui_ctx);
                ui20::apply_theme(&cc.egui_ctx, app.ui_prefs.dark);
                Ok(Box::new(app))
            }),
        )
        .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn disconnected_app() -> OpenLessEguiApp {
            OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture: plugin unavailable".into()),
            )
        }

        fn rendered_text(mut draw: impl FnMut(&mut egui::Ui)) -> String {
            let ctx = egui::Context::default();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(720.0, 1800.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        draw(ui);
                    });
                },
            );
            output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::epaint::Shape::Text(text) => Some(text.galley.job.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        }

        #[test]
        fn start_page_pipeline_multimodal_uses_only_omni_configuration() {
            use openless_core::shared_types::PipelineMode;

            let mut app = disconnected_app();
            // A pending settings draft must not override Core's effective mode.
            app.preferences = Some(UserPreferences {
                multimodal_pipeline_enabled: false,
                pipeline_mode: PipelineMode::Traditional,
                ..Default::default()
            });
            for omni_configured in [true, false] {
                for (asr_configured, llm_configured) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    app.snapshot = Some(BackendSnapshot {
                        credentials: openless_core::shared_types::CredentialsStatus {
                            pipeline_mode: PipelineMode::Multimodal,
                            omni_configured,
                            asr_configured,
                            llm_configured,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    let text = rendered_text(|ui| app.start_ui(ui));
                    // 2.0 卡片把状态拆成键值两行（rendered_text 按 \n 连接），
                    // 断言跟随结构：多模态只报 Omni，不出现传统管线的行。
                    let expected = if omni_configured {
                        "已配置"
                    } else {
                        "尚未配置，请到 AI 服务配置 Omni"
                    };
                    assert!(text.contains("Omni"), "{text}");
                    assert!(text.contains(expected), "missing {expected}: {text}");
                    assert!(!text.contains("ASR 语音识别"), "{text}");
                    assert!(!text.contains("LLM 润色"), "{text}");
                    assert!(text.contains("已配置不代表校验通过"), "{text}");
                }
            }
        }

        #[test]
        fn start_page_pipeline_traditional_reports_asr_and_llm_independently() {
            use openless_core::shared_types::PipelineMode;

            let mut app = disconnected_app();
            // Conversely, a multimodal draft must not hide the effective
            // traditional pipeline's missing ASR or LLM configuration.
            app.preferences = Some(UserPreferences {
                multimodal_pipeline_enabled: true,
                pipeline_mode: PipelineMode::Multimodal,
                ..Default::default()
            });
            for omni_configured in [true, false] {
                for (asr_configured, llm_configured) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    app.snapshot = Some(BackendSnapshot {
                        credentials: openless_core::shared_types::CredentialsStatus {
                            pipeline_mode: PipelineMode::Traditional,
                            omni_configured,
                            asr_configured,
                            llm_configured,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    let text = rendered_text(|ui| app.start_ui(ui));
                    // 键值两行结构：键行 + 状态值行分别断言（语义与旧断言一致）。
                    assert!(text.contains("ASR 语音识别"), "{text}");
                    assert!(
                        text.contains(if asr_configured {
                            "已配置"
                        } else {
                            "尚未配置，请配置 AI 服务或激活本地模型"
                        }),
                        "{text}"
                    );
                    assert!(text.contains("LLM 润色"), "{text}");
                    assert!(
                        text.contains(if llm_configured {
                            "已配置"
                        } else {
                            "尚未配置"
                        }),
                        "{text}"
                    );
                    assert!(!text.contains("Omni"), "{text}");
                    assert!(text.contains("已配置不代表校验通过"), "{text}");
                }
            }
        }

        #[test]
        fn background_approval_survives_navigation_and_stale_terminals() {
            let mut app = disconnected_app();
            let session = openless_core::SessionId::new();
            for (sequence, kind) in [
                (
                    1,
                    LessComputerEventKind::User {
                        text: "task".into(),
                        fresh: true,
                    },
                ),
                (
                    2,
                    LessComputerEventKind::Approval {
                        token: "approval".into(),
                        command: "echo test".into(),
                        reason: "fixture".into(),
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
            for page in Page::ALL {
                app.navigation.open(page);
                assert_eq!(app.page_activity(Page::Agent), Some("待审批"));
                let text = rendered_text(|ui| {
                    app.activity_ui(ui);
                    app.agent_approval_ui(ui);
                });
                for control in ["允许", "拒绝", "取消 Agent"] {
                    assert!(
                        text.contains(control),
                        "missing {control} on {page:?}: {text}"
                    );
                }
            }
            app.apply_event(BackendEvent {
                sequence: 3,
                session_id: Some(openless_core::SessionId::new()),
                kind: BackendEventKind::LessComputerEvent(openless_core::LessComputerEvent {
                    seq: None,
                    kind: LessComputerEventKind::Cancelled,
                }),
            });
            assert_eq!(
                app.pending_approval,
                Some(("approval".into(), "echo test".into()))
            );
            assert!(app.less_computer_running);
        }

        #[test]
        fn qa_and_selection_events_on_settings_keep_drafts_and_action_notices() {
            let mut app = disconnected_app();
            app.navigation.open(Page::Settings);
            app.qa_input = "unsent question".into();
            let qa_session = openless_core::SessionId::new();
            let mut thinking = QaStateEvent::simple(QaStateKind::Thinking);
            thinking.session_id = Some(qa_session.to_string());
            app.apply_event(BackendEvent {
                sequence: 1,
                session_id: Some(qa_session),
                kind: BackendEventKind::QaState(thinking),
            });
            let selection_session = openless_core::SessionId::new();
            app.apply_event(BackendEvent {
                sequence: 2,
                session_id: Some(selection_session),
                kind: BackendEventKind::SelectionStateChanged(SelectionSnapshot {
                    phase: SelectionPhase::Preview,
                    session_id: Some(selection_session),
                    preview_text: Some("editable preview".into()),
                    ..Default::default()
                }),
            });
            assert_eq!(app.navigation.page, Page::Settings);
            assert!(app.navigation.has_update(Page::Qa));
            assert_eq!(app.page_activity(Page::Selection), Some("待确认"));
            app.selection_draft = "user edited preview".into();
            app.navigation.open(Page::Qa);
            app.navigation.open(Page::Selection);
            app.navigation.open(Page::Models);
            assert_eq!(app.qa_input, "unsent question");
            assert_eq!(app.selection_draft, "user edited preview");
            assert_eq!(
                app.selection.as_ref().unwrap().session_id,
                Some(selection_session)
            );
            let text = rendered_text(|ui| app.activity_ui(ui));
            assert!(text.contains("取消选区预览"), "{text}");
        }

        #[test]
        fn startup_failure_keeps_preparation_steps_without_claiming_connection() {
            let mut app = disconnected_app();
            app.environment = Some(LinuxCapabilitySnapshot::from_environment(
                Some("wayland-0"),
                None,
                false,
                false,
                LinuxPackageKind::AppImage,
            ));
            app.plugin_check = Some(Ok(FcitxPluginStatus::Updated));
            let text = rendered_text(|ui| app.start_ui(ui));
            for expected in [
                "Core 未连接",
                "Wayland",
                "D-Bus 探测未通过",
                "尚未验证录音",
                "fcitx5-diagnose",
                "fcitx5-remote -r",
                "Secret Service",
            ] {
                assert!(text.contains(expected), "missing {expected}: {text}");
            }
            assert!(!text.contains("Core 已连接"));
            assert!(!text.contains("Core：运行中"));
        }

        #[test]
        fn stopped_or_stale_remote_status_never_shows_pairing_secrets_or_old_urls() {
            let mut app = disconnected_app();
            for (running, urls_stale) in [(false, false), (true, true)] {
                app.remote_access = Some((
                    openless_core::RemoteInputStatus {
                        enabled: true,
                        running,
                        starting: false,
                        port: 8443,
                        urls: vec!["https://old.example.invalid".into()],
                        urls_stale,
                        locale: "en".into(),
                        ca_fingerprint_sha256: None,
                        connection_count: 0,
                        active_session_id: None,
                    },
                    "fixture-pin".into(),
                ));
                let text = rendered_text(|ui| app.remote_ui(ui));
                assert!(!text.contains("fixture-pin"), "{text}");
                assert!(!text.contains("https://old.example.invalid"), "{text}");
                assert!(text.contains("当前连接数"), "{text}");
            }
        }

        #[test]
        fn continuation_turn_keeps_receiving_output_and_approval() {
            let mut app = OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture".into()),
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
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux_app::run() {
        eprintln!("OpenLess Linux UI failed: {error}");
        std::process::exit(1);
    }
}
