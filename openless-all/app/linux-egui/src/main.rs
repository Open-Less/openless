#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("openless-linux-egui is only available on Linux");
}

#[cfg(target_os = "linux")]
mod linux_app {
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
    use openless_linux_egui::{
        drain_events, ensure_fcitx5_plugin_installed, EventDrainOutcome, Fcitx5HotkeyListener,
        FcitxPluginInstallPlan, FcitxPluginStatus, LinuxBackendBuilder, LinuxCapabilitySnapshot,
        LinuxLaunchIntent, LinuxNativeRuntime, LinuxPackageKind, LinuxResourceLayout,
        SingleInstanceBroker, SingleInstanceRole,
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
                    };
                    app.load_models();
                    app.load_remote_status();
                    app.load_providers(openless_core::ChannelKind::Asr);
                    app
                }
                Err(error) => Self {
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
                }
                BackendEventKind::SelectionStateChanged(snapshot) => {
                    if snapshot.phase == SelectionPhase::Preview {
                        self.selection_draft = snapshot.preview_text.clone().unwrap_or_default();
                        self.selection_preview_visible = true;
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
                            self.selection_preview_visible = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        HostAction::HideSelectionPreview => {
                            self.selection_preview_visible = false;
                        }
                        HostAction::ShowQa => {
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
            let mut remote_update = None;
            if let Some(preferences) = self.preferences.as_mut() {
                ui.checkbox(&mut preferences.streaming_insert, "流式插入");
                ui.checkbox(&mut preferences.coding_agent_enabled, "启用 Less Computer");
                ui.checkbox(&mut preferences.remote_input_enabled, "启用远程输入");
                ui.add(
                    egui::DragValue::new(&mut preferences.remote_input_port)
                        .range(1..=u16::MAX)
                        .prefix("端口 "),
                );
                if ui.button("保存设置").clicked() {
                    if let (Some(native), Some(snapshot)) = (&self.native, &self.snapshot) {
                        match native
                            .host()
                            .save_settings(preferences.clone(), snapshot.preferences_revision)
                        {
                            Ok(_) => {
                                self.status = "设置已保存".to_string();
                                remote_update = Some(openless_core::RemoteInputConfig {
                                    enabled: preferences.remote_input_enabled,
                                    port: preferences.remote_input_port,
                                });
                            }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                }
            }
            if let (Some(config), Some(backend)) = (remote_update, self.backend()) {
                self.spawn(async move {
                    backend.services().remote_input.configure(config).await?;
                    Ok("远程输入状态已更新".to_string())
                });
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
        }

        fn history_ui(&mut self, ui: &mut egui::Ui) {
            ui.heading("历史");
            let Some(backend) = self.backend() else {
                return;
            };
            match backend.list_history() {
                Ok(history) if history.is_empty() => {
                    ui.label("暂无历史记录");
                }
                Ok(history) => {
                    for item in history.into_iter().rev().take(20) {
                        let delivery = match item.insert_status {
                            HistoryInsertStatus::Inserted => "已插入",
                            HistoryInsertStatus::CopiedFallback => "已复制",
                            HistoryInsertStatus::PasteSent => "已发送粘贴",
                            HistoryInsertStatus::Failed => "失败",
                            HistoryInsertStatus::NotRequested => "未请求插入",
                        };
                        ui.label(format!(
                            "{} · {} · {}",
                            item.created_at, delivery, item.final_text
                        ));
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
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                if let Some(backend) = self.backend() {
                    self.spawn(async move {
                        backend.cancel_active_voice_session(None).await?;
                        Ok("语音会话已取消".to_string())
                    });
                }
            }
            egui::TopBottomPanel::top("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("OpenLess 2.0");
                    ui.separator();
                    ui.label(&self.status);
                });
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(error) = &self.startup_error {
                    ui.heading("启动失败");
                    ui.colored_label(egui::Color32::RED, error);
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.dictation_ui(ui);
                    ui.separator();
                    self.qa_ui(ui);
                    if self.qa_visible {
                        ui.separator();
                    }
                    self.selection_ui(ui);
                    ui.separator();
                    self.less_computer_ui(ui);
                    ui.separator();
                    self.models_ui(ui);
                    ui.separator();
                    self.settings_ui(ui);
                    ui.separator();
                    self.history_ui(ui);
                });
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

    fn ensure_fcitx5_ready(config: &BackendConfig) -> Result<(), String> {
        let home = config
            .home_dir
            .as_deref()
            .ok_or_else(|| "HOME is unavailable for the fcitx5 plugin".to_string())?;
        let layout = LinuxResourceLayout::detect(None).map_err(|error| error.to_string())?;
        let plan =
            FcitxPluginInstallPlan::for_layout(&layout, home).map_err(|error| error.to_string())?;
        match ensure_fcitx5_plugin_installed(&plan).map_err(|error| error.to_string())? {
            FcitxPluginStatus::Ready => Ok(()),
            FcitxPluginStatus::Updated => Err(
                "fcitx5 插件已安装或更新；请先重载 fcitx5（fcitx5-remote -r）再重启 OpenLess"
                    .to_string(),
            ),
            FcitxPluginStatus::Missing => {
                Err("未找到 OpenLess fcitx5 插件；请重新安装当前软件包".to_string())
            }
        }
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
            viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 720.0]),
            ..Default::default()
        };
        eframe::run_native(
            "OpenLess",
            options,
            Box::new(move |_| Ok(Box::new(OpenLessEguiApp::new(tokio, native)))),
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
