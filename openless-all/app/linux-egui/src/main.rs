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

    use crate::ui::bridge::{
        self, HostToWindow, UiBridgeClient, UiBridgeHost, WindowToHost, UI_BRIDGE_VERSION,
    };
    use crate::ui::frontend::{self, view_model::FrontendViewModel};
    use crate::ui::{shell, theme};
    use chrono::Datelike;
    use eframe::egui;
    use openless_core::{
        BackendConfig, BackendError, BackendEvent, BackendEventKind, BackendSnapshot,
        DictationPhase, HistoryInsertStatus, HostAction, LessComputerEventKind, QaStateEvent,
        QaStateKind, SelectionPhase, SelectionSnapshot, TranscriptAccumulator, UserPreferences,
    };
    use openless_linux_egui::{
        capsule_hide_delay, capsule_hide_is_still_current, capsule_needs_fallback_dismissal,
        capsule_outcome, fmt_l10n, load_locale_pref, normalize_stop_result, phase_shows_capsule,
        save_locale_pref, tr_l10n, CapsuleOutcome, Lang, LocalePref,
    };
    use openless_linux_egui::{
        drain_events, ensure_fcitx5_plugin_installed, fcitx5_copy_to_clipboard, notify,
        open_external, write_jsonl, EventDrainOutcome, Fcitx5HotkeyListener,
        FcitxPluginInstallPlan, FcitxPluginStatus, HostToPopup, LinuxBackendBuilder,
        LinuxCapabilitySnapshot, LinuxHotkeyEvent, LinuxLaunchIntent, LinuxNativeRuntime,
        LinuxPackageKind, LinuxResourceLayout, LinuxUpdateSupport, Notification, PopupActionGuard,
        PopupChatMessage, PopupKind, PopupState, PopupSupervisor, PopupSupervisorEvent,
        PopupToHost, SingleInstanceBroker, SingleInstanceRole, UpdateManifest, UpdateSchedule,
        POPUP_PROTOCOL_VERSION,
    };

    enum UiResult {
        Message(String),
        /// 终态在屏上停留结束：胶囊可以收起了（handler 会再核对会话与相位）。
        CapsuleDismissDue {
            session_id: String,
        },
        Remote(Result<(openless_core::RemoteInputStatus, String), String>),
        Providers(Result<ProviderPanel, String>),
        /// Credential channels for the settings modal's AI-services tab.
        SettingsChannels(Result<Vec<SettingsChannelRow>, String>),
        ServiceConfigured([bool; 2]),
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
        Marketplace(u64, Result<Vec<openless_core::MarketplaceListItem>, String>),
        MarketplaceLikes(Result<Vec<String>, String>),
        MarketplaceFlow(Result<openless_core::OAuthDeviceFlow, String>),
        MarketplaceAuthPoll(Result<openless_core::OAuthPollResult, String>),
        MarketplaceDetail(Result<openless_core::MarketplaceDetail, String>),
        MarketplaceMine(Result<(Vec<openless_core::MarketplaceMyPackItem>, Vec<String>), String>),
        Microphones(Result<Vec<openless_core::MicrophoneDevice>, String>),
        Overview(Result<OverviewData, String>),
        UpdateCheck(Result<Option<UpdateManifest>, String>),
        UpdateProgress(openless_linux_egui::DownloadProgress),
        UpdateInstalled(Result<openless_linux_egui::InstalledUpdate, String>),
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

    /// One credential channel cached for the settings modal.
    #[derive(Clone, Debug)]
    struct SettingsChannelRow {
        id: String,
        name: String,
        provider_type: String,
        model: String,
        enabled: bool,
        last_ok: Option<bool>,
        last_latency_ms: Option<u32>,
        last_error: Option<String>,
    }

    /// 追问编辑态：Core 只在变化时下发 `Some(..)`，所以逐字段合并。
    #[derive(Clone, Copy, Debug, Default)]
    struct QaEditFlags {
        instruction_mode: bool,
        apply_available: bool,
        revert_available: bool,
    }

    /// 固定（图钉）后不再响应宿主的自动收起；✕/Esc 仍照常关闭。
    fn qa_hides_on_host_action(pinned: bool) -> bool {
        !pinned
    }

    impl QaEditFlags {
        fn merge(&mut self, state: &openless_core::QaStateEvent) {
            if let Some(value) = state.edit_instruction_mode {
                self.instruction_mode = value;
            }
            if let Some(value) = state.edit_apply_available {
                self.apply_available = value;
            }
            if let Some(value) = state.edit_revert_available {
                self.revert_available = value;
            }
        }
    }

    #[derive(Default, Clone, Copy)]
    struct SettingsDirty {
        streaming_insert: bool,
        stable_transcription: bool,
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
                || self.stable_transcription
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
            if self.stable_transcription {
                merged.stable_transcription_enabled = draft.stable_transcription_enabled;
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
                merged.mute_during_recording = draft.mute_during_recording;
                merged.audio_cue_on_record = draft.audio_cue_on_record;
                merged.record_audio_for_debug = draft.record_audio_for_debug;
                merged.restore_clipboard_after_paste = draft.restore_clipboard_after_paste;
                merged.paste_shortcut = draft.paste_shortcut;
                merged.history_retention_days = draft.history_retention_days;
                merged.history_max_entries = draft.history_max_entries;
                merged.remote_input_default_mode = draft.remote_input_default_mode.clone();
            }
            if self.microphone {
                merged.microphone_device_name = draft.microphone_device_name.clone();
            }
            if self.appearance {
                merged.theme_mode = draft.theme_mode;
                merged.show_overview_activity_heatmap = draft.show_overview_activity_heatmap;
                merged.stacked_row_layout = draft.stacked_row_layout;
                merged.conservative_layout = draft.conservative_layout;
                merged.use_system_proxy = draft.use_system_proxy;
                merged.multimodal_pipeline_enabled = draft.multimodal_pipeline_enabled;
                merged.selection_voice_enabled = draft.selection_voice_enabled;
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

    // ---- Native Overview summary (Tauri parity) -----------------------------
    //
    // The Tauri Overview derives its dashboard from three real Core sources:
    //   * `CredentialsStatus`  -> active ASR/LLM provider and its configured state
    //   * `HistoryStore`       -> today's metrics, total count and recent entries
    //   * `ActivityStore`      -> trailing-window aggregates + daily heatmap
    // Fetching happens off the egui frame in a tokio task (`load_overview`); the
    // pure helpers below only shape already-loaded data and are unit tested
    // without a runtime, a backend or any UI.

    /// Raw snapshot fetched asynchronously from Core for the Overview tab.
    #[derive(Clone, Debug)]
    struct OverviewData {
        credentials: openless_core::CredentialsStatus,
        history: Vec<openless_core::DictationSession>,
        activity: Vec<openless_core::ActivityDay>,
    }

    #[derive(Clone, Debug, Default)]
    struct RecentEntry {
        created_at: String,
        final_text: String,
        raw_transcript: String,
        mode: openless_core::PolishMode,
        duration_ms: Option<u64>,
    }

    /// One calendar day of activity. Used both by the trailing daily series
    /// behind the period chart and by the calendar-year heatmap.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct DailyActivity {
        /// `YYYY-MM-DD` local date.
        date: String,
        count: u32,
        chars: u64,
        duration_ms: u64,
    }

    /// Activity aggregate over a trailing calendar window. Zero days that never
    /// recorded activity are absent from the store, so a window may cover more
    /// calendar days than `active_days`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct ActivityAggregate {
        active_days: usize,
        segments: u64,
        chars: u64,
        duration_ms: u64,
    }

    /// Fully derived, display-ready Overview summary (computed purely, tested).
    #[derive(Clone, Debug, Default)]
    struct OverviewSummary {
        asr_provider: String,
        llm_provider: String,
        asr_configured: bool,
        llm_configured: bool,
        chars_today: u64,
        segments_today: usize,
        duration_ms_today: u64,
        avg_latency_ms: u64,
        history_total: usize,
        recent: Vec<RecentEntry>,
        last_7: ActivityAggregate,
        last_30: ActivityAggregate,
        /// Last 30 calendar days ending today, chronological (oldest first).
        /// The 7-day view slices the tail.
        activity_daily: Vec<DailyActivity>,
        /// Calendar year rendered by the annual heatmap card.
        heatmap_year: i32,
        /// Every day of `heatmap_year`, chronological. Days without activity
        /// are present with `count == 0` so the page can lay out the grid.
        heatmap: Vec<DailyActivity>,
    }

    #[derive(Clone, Debug)]
    enum OverviewState {
        Loading,
        Loaded(OverviewData),
        Failed(String),
    }

    #[derive(Clone)]
    enum ProvidersState {
        Loading,
        Loaded(ProviderPanel),
        Failed(String),
    }

    /// Trailing window (days) covered by the period chart's daily series.
    const OVERVIEW_DAILY_DAYS: i64 = 30;

    impl OverviewState {
        fn summary(&self, today: chrono::NaiveDate) -> Option<OverviewSummary> {
            match self {
                OverviewState::Loaded(data) => Some(overview_summary(data, today)),
                OverviewState::Loading | OverviewState::Failed(_) => None,
            }
        }
    }

    /// RFC3339 history timestamp -> local calendar date. A value that cannot
    /// be parsed simply yields `None` and contributes nothing to the summary.
    fn history_local_date(created_at: &str) -> Option<chrono::NaiveDate> {
        chrono::DateTime::parse_from_rfc3339(created_at)
            .ok()
            .map(|instant| instant.with_timezone(&chrono::Local).date_naive())
    }

    /// Sum one activity window's segments/chars/duration over `[today-days+1, today]`.
    fn aggregate_window(
        by_date: &std::collections::BTreeMap<chrono::NaiveDate, &openless_core::ActivityDay>,
        today: chrono::NaiveDate,
        days: i64,
    ) -> ActivityAggregate {
        let start = today - chrono::Duration::days(days - 1);
        let mut aggregate = ActivityAggregate::default();
        for (_, day) in by_date.range(start..=today) {
            aggregate.active_days += 1;
            aggregate.segments += u64::from(day.count);
            aggregate.chars += day.chars;
            aggregate.duration_ms += day.duration_ms;
        }
        aggregate
    }

    /// Build the trailing daily series ending at `today` (inclusive), oldest
    /// first. Days absent from the store render as zero.
    fn build_daily_series(
        by_date: &std::collections::BTreeMap<chrono::NaiveDate, &openless_core::ActivityDay>,
        today: chrono::NaiveDate,
        days: i64,
    ) -> Vec<DailyActivity> {
        let mut series = Vec::with_capacity(days as usize);
        for offset in (0..days).rev() {
            let date = today - chrono::Duration::days(offset);
            series.push(daily_activity(by_date, date));
        }
        series
    }

    /// Build the full calendar-year heatmap for `year`: January 1st through
    /// December 31st, chronological, inactive days included.
    fn build_calendar_year_heatmap(
        by_date: &std::collections::BTreeMap<chrono::NaiveDate, &openless_core::ActivityDay>,
        year: i32,
    ) -> Vec<DailyActivity> {
        let mut days = Vec::with_capacity(366);
        let Some(mut date) = chrono::NaiveDate::from_ymd_opt(year, 1, 1) else {
            return days;
        };
        while date.year() == year {
            days.push(daily_activity(by_date, date));
            date += chrono::Duration::days(1);
        }
        days
    }

    fn daily_activity(
        by_date: &std::collections::BTreeMap<chrono::NaiveDate, &openless_core::ActivityDay>,
        date: chrono::NaiveDate,
    ) -> DailyActivity {
        let day = by_date.get(&date);
        DailyActivity {
            date: date.format("%Y-%m-%d").to_string(),
            count: day.map(|day| day.count).unwrap_or(0),
            chars: day.map(|day| day.chars).unwrap_or(0),
            duration_ms: day.map(|day| day.duration_ms).unwrap_or(0),
        }
    }

    /// Shape the fetched Core snapshot into the display summary. Pure and free of
    /// any runtime/IO so it can be exercised by focused unit tests.
    fn overview_summary(data: &OverviewData, today: chrono::NaiveDate) -> OverviewSummary {
        let mut segments_today = 0usize;
        let mut chars_today = 0u64;
        let mut duration_ms_today = 0u64;
        for session in &data.history {
            if history_local_date(&session.created_at) == Some(today) {
                segments_today += 1;
                chars_today += session.final_text.chars().count() as u64;
                duration_ms_today += session.duration_ms.unwrap_or(0);
            }
        }
        let avg_latency_ms = if segments_today > 0 {
            duration_ms_today / segments_today as u64
        } else {
            0
        };

        // Newest five entries. `created_at` is RFC3339 in a constant UTC offset,
        // so lexicographic ordering is a valid chronological ordering.
        let mut recent: Vec<RecentEntry> = data
            .history
            .iter()
            .map(|session| RecentEntry {
                created_at: session.created_at.clone(),
                final_text: session.final_text.clone(),
                raw_transcript: session.raw_transcript.clone(),
                mode: session.mode,
                duration_ms: session.duration_ms,
            })
            .collect();
        recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        recent.truncate(5);

        let mut by_date: std::collections::BTreeMap<
            chrono::NaiveDate,
            &openless_core::ActivityDay,
        > = std::collections::BTreeMap::new();
        for day in &data.activity {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
                by_date.insert(date, day);
            }
        }

        OverviewSummary {
            asr_provider: data.credentials.active_asr_provider.clone(),
            llm_provider: data.credentials.active_llm_provider.clone(),
            asr_configured: data.credentials.asr_configured,
            llm_configured: data.credentials.llm_configured,
            chars_today,
            segments_today,
            duration_ms_today,
            avg_latency_ms,
            history_total: data.history.len(),
            recent,
            last_7: aggregate_window(&by_date, today, 7),
            last_30: aggregate_window(&by_date, today, 30),
            activity_daily: build_daily_series(&by_date, today, OVERVIEW_DAILY_DAYS),
            heatmap_year: today.year(),
            heatmap: build_calendar_year_heatmap(&by_date, today.year()),
        }
    }

    #[derive(Clone)]
    struct ProviderEditor {
        kind: openless_core::ChannelKind,
        channel: openless_core::ChannelSummary,
        descriptor: openless_core::ProviderDescriptor,
        name: String,
        endpoint: String,
        model: String,
        volcengine_service: String,
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

    /// Draft of the open provider editor. It is the single source of truth for
    /// the editor fields: pages push their input back here as actions and the
    /// mirror writes it out each frame, so re-reading the channel list never
    /// clobbers what the user is typing.
    struct ProviderEditorForm {
        channel_id: String,
        provider_type: String,
        label: String,
        auth: frontend::view_model::SettingsProviderAuth,
        name: String,
        endpoint: String,
        model: String,
        resource_id: String,
        auth_mode: String,
        // Write-only secret drafts: they start empty on every load and are
        // cleared as soon as they have been handed to Core.
        primary_secret: String,
        secondary_secret: String,
        models: Vec<String>,
        models_loading: bool,
    }

    impl ProviderEditorForm {
        fn from_editor(editor: &ProviderEditor, lang: Lang) -> Self {
            Self {
                channel_id: editor.channel.id.clone(),
                provider_type: editor.descriptor.provider_type.as_str().to_string(),
                label: localized_provider_label(
                    lang,
                    editor.kind,
                    editor.descriptor.provider_type.as_str(),
                ),
                auth: settings_provider_auth(editor.descriptor.auth_requirement),
                name: editor.name.clone(),
                endpoint: editor.endpoint.clone(),
                model: editor.model.clone(),
                resource_id: editor.resource_id.clone(),
                auth_mode: if editor.auth_mode.is_empty() {
                    "app_id_token".to_string()
                } else {
                    editor.auth_mode.clone()
                },
                primary_secret: String::new(),
                secondary_secret: String::new(),
                models: Vec::new(),
                models_loading: false,
            }
        }
    }

    /// 「崩溃即重开」的预算：同一个面板在 [`POPUP_RESTART_WINDOW`] 内最多重开
    /// 这么多次。
    ///
    /// 面板崩溃后旧代码会立即 `show_*_popup()`，一个必崩的面板（例如渲染时
    /// panic）就会变成“弹窗一直反复弹出”：用户看到的是面板高频重现，日志里是
    /// 一串退出码。给重开加上预算，超了就停手并留一条可读日志。
    const POPUP_RESTART_LIMIT: usize = 2;
    const POPUP_RESTART_WINDOW: Duration = Duration::from_secs(60);
    /// 面板种类数（`PopupKind` 没有 index 方法，这里只用于数组下标）。
    const POPUP_KIND_COUNT: usize = 3;

    fn popup_kind_index(kind: PopupKind) -> usize {
        match kind {
            PopupKind::Qa => 0,
            PopupKind::Capsule => 1,
            PopupKind::LessComputer => 2,
        }
    }

    #[derive(Default, Clone, Copy)]
    struct PopupRestartBudget {
        attempts: usize,
        window_start: Option<std::time::Instant>,
    }

    impl PopupRestartBudget {
        /// 这次崩溃允许重开吗？窗口过期就重新计数。
        fn allow(&mut self, at: std::time::Instant) -> bool {
            match self.window_start {
                Some(start) if at.saturating_duration_since(start) < POPUP_RESTART_WINDOW => {
                    if self.attempts >= POPUP_RESTART_LIMIT {
                        return false;
                    }
                    self.attempts += 1;
                    true
                }
                _ => {
                    self.window_start = Some(at);
                    self.attempts = 1;
                    true
                }
            }
        }
    }

    pub struct OpenLessEguiApp {
        tokio: Arc<tokio::runtime::Runtime>,
        native: Option<LinuxNativeRuntime>,
        subscription: Option<openless_core::EventSubscription>,
        snapshot: Option<BackendSnapshot>,
        preferences: Option<UserPreferences>,
        settings_dirty: SettingsDirty,
        settings_channel_kind: openless_core::ChannelKind,
        settings_channels: Vec<SettingsChannelRow>,
        settings_channels_loading: bool,
        /// 语言模型 / 语音识别是否各自有启用的渠道（AI 服务页的状态点）。
        service_configured: [bool; 2],
        /// 文本型设置行（端口/条数/路径…）只在偏好刚载入或外部变更时回灌，
        /// 否则每帧覆盖会把用户正在输入的内容弹回去（表现为「输入框用不了」）。
        hydrate_text_fields: bool,
        overview: OverviewState,
        microphones: Vec<openless_core::MicrophoneDevice>,
        /// 麦克风枚举失败的原因（设置页据此显示 `microphoneLoadError`）。
        microphone_error: Option<String>,
        transcript: String,
        transcript_state: TranscriptAccumulator,
        transcript_session: Option<openless_core::SessionId>,
        recording_phase_active: bool,
        last_event_sequence: u64,
        less_computer_input: String,
        less_computer_output: String,
        less_computer_turn_start: usize,
        less_computer_session: Option<openless_core::SessionId>,
        /// Less Computer 面板要呈现的事件序列。宿主是唯一所有者，弹窗进程只负责画；
        /// 每次重连都收到完整序列，窗口进程重启不丢历史。
        less_computer_entries: Vec<openless_linux_egui::LessComputerEntry>,
        /// 本轮尚未终结（面板显示「执行中…」）。
        less_computer_working: bool,
        /// 已展示过的面板是否还在（托起面板时只推状态，不重复拉起进程）。
        less_computer_popup: Option<PopupSupervisor>,
        pending_approval: Option<(String, String)>,
        qa_visible: bool,
        /// 划词追问的图钉：固定后 `HostAction::HideQa` 不再收起窗口。
        qa_pinned: bool,
        /// 追问「编辑指令」三态（Core `QaStateEvent` 的部分更新）。
        qa_edit: QaEditFlags,
        qa_input: String,
        qa_state: Option<QaStateEvent>,
        /// 选区助手面板是否处于「润色结果」模式（独立预览窗口已下线，
        /// 润色结果由这个面板承载：提问对话 / 润色结果是同一弹窗的两套 UI）。
        polish_result_visible: bool,
        selection_draft: String,
        selection: Option<SelectionSnapshot>,
        remote_access: Option<(openless_core::RemoteInputStatus, String)>,
        provider_kind: openless_core::ChannelKind,
        providers: ProvidersState,
        selected_channel_id: Option<String>,
        provider_editor: ProviderEditorState,
        /// Draft mirrored into the view model while the editor is open.
        provider_editor_form: Option<ProviderEditorForm>,
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
        capsule_popup: Option<PopupSupervisor>,
        popup_action_guard: PopupActionGuard,
        /// 当前胶囊展示的会话 id（用于「会话消失但没收到终态」的兜底收起）。
        capsule_session: Option<String>,
        /// 已经为哪个会话排过收起计时，避免重复计时。
        capsule_dismissal_scheduled: Option<String>,
        tray: Option<openless_linux_egui::LinuxTray>,
        exit_requested: bool,
        update_support: LinuxUpdateSupport,
        update_schedule: UpdateSchedule,
        update_started: std::time::Instant,
        /// 上次打「泵心跳」日志的时间。
        last_pump_heartbeat: std::time::Instant,
        /// 用户是否希望主窗口开着。窗口本体在独立的 UI 进程里，宿主只负责
        /// 拉起来、看着它退出、再按需重拉。
        window_should_be_open: bool,
        /// 当前 UI 窗口进程；它退出后置空（窗口与任务栏条目随之消失）。
        ui_window: Option<std::process::Child>,
        /// 上次拉起 UI 进程的时刻：防抖，连续点托盘菜单不会拉出两个窗口。
        ui_window_spawned_at: Option<std::time::Instant>,
        /// 上一次发给 UI 的快照指纹；内容没变就不重复发。
        last_snapshot_fingerprint: Option<u64>,
        /// 上一次发快照的时间（长连接保活）。
        last_snapshot_at: std::time::Instant,
        /// 本帧从 UI 收到、待宿主执行的动作（按到达顺序）。
        pending_ui_actions: Vec<frontend::view_model::FrontendAction>,
        /// 本帧从 UI 窗口/面板收到、待处理的本地热键边沿（按到达时刻）。
        ///
        /// 我们自己的窗口有焦点时插件收不到按键，热键只能由窗口自己认出来
        /// （见 `local_hotkeys` 模块文档），这里按顺序攒着与插件信号一起处理。
        pending_local_hotkeys: Vec<(std::time::Instant, openless_linux_egui::LocalHotkeyEdge)>,
        /// 本地边沿与插件信号之间的去重（同一个物理按键可能两个来源都报）。
        hotkey_dedupe: openless_linux_egui::HotkeyDeduplicator,
        /// 已在 60s 内重开过的面板次数（索引见 `popup_kind_index`）。
        popup_restarts: [PopupRestartBudget; POPUP_KIND_COUNT],
        /// 最近一次下发给窗口/面板的热键配置；变了才重发。
        hotkeys_sent: Option<openless_core::HotkeyRuntimeTarget>,
        /// 本帧从 UI 收到、待回包的延迟探针序号。
        pending_ui_pongs: Vec<u64>,
        update_manifest: Option<UpdateManifest>,
        update_busy: bool,
        update_progress: Option<openless_linux_egui::DownloadProgress>,
        /// Currently playing history recording (session id + player handle).
        history_clip: Option<(String, openless_linux_egui::ClipPlayer)>,
        marketplace_items: Vec<openless_core::MarketplaceListItem>,
        /// True once a marketplace list request has completed (ok or error), so
        /// the page can leave its loading state even when the result is empty.
        marketplace_attempted: bool,
        marketplace_query: String,
        marketplace_flow: Option<openless_core::OAuthDeviceFlow>,
        marketplace_detail: Option<openless_core::MarketplaceDetail>,
        marketplace_my_packs: Vec<openless_core::MarketplaceMyPackItem>,
        marketplace_my_likes: Vec<String>,
        /// Monotonic id for marketplace list requests: a response from a
        /// superseded search is dropped instead of overwriting fresher data
        /// (same intent as the Tauri page's `reqSeqRef` guard).
        marketplace_seq: u64,
        /// Debounce deadline for the search box. The Tauri page waits 300ms
        /// after the last keystroke before hitting the API; without it every
        /// character (and every IME composition update) was its own request.
        marketplace_search_deadline: Option<std::time::Instant>,
        /// Likes are fetched once per session (Tauri refreshes them when the
        /// sign-in state changes), not on every search.
        marketplace_likes_loaded: bool,
        style_editor: Option<openless_core::StylePack>,
        style_hotkey_pack_id: String,
        style_hotkey_primary: String,
        style_hotkey_modifiers: String,
        status: String,
        startup_error: Option<String>,
        locale_pref: LocalePref,
        lang: Lang,
        active_page: shell::Page,
        frontend_vm: FrontendViewModel,
        tx: mpsc::Sender<UiResult>,
        rx: mpsc::Receiver<UiResult>,
    }

    impl OpenLessEguiApp {
        fn new(
            tokio: Arc<tokio::runtime::Runtime>,
            native: Result<LinuxNativeRuntime, String>,
            tray: Option<openless_linux_egui::LinuxTray>,
            update_support: LinuxUpdateSupport,
            window_should_be_open: bool,
        ) -> Self {
            let (tx, rx) = mpsc::channel();
            let locale_pref = load_locale_pref();
            let lang = locale_pref.resolve();
            if let Some(tray) = tray.as_ref() {
                // The tray renders labels in the resolved UI language. It runs
                // in its own worker, so push the resolved language through the
                // same control channel that updates microphone checkmarks.
                let _ = tray.set_lang(lang);
            }
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
                        settings_channel_kind: openless_core::ChannelKind::Llm,
                        settings_channels: Vec::new(),
                        settings_channels_loading: false,
                        service_configured: [false; 2],
                        hydrate_text_fields: true,
                        overview: OverviewState::Loading,
                        microphones: Vec::new(),
                        microphone_error: None,
                        transcript: String::new(),
                        transcript_state: TranscriptAccumulator::default(),
                        transcript_session: None,
                        recording_phase_active: false,
                        last_event_sequence: 0,
                        less_computer_input: String::new(),
                        less_computer_output: String::new(),
                        less_computer_turn_start: 0,
                        less_computer_session: None,
                        less_computer_entries: Vec::new(),
                        less_computer_working: false,
                        less_computer_popup: None,
                        pending_approval: None,
                        qa_visible: false,
                        qa_pinned: false,
                        qa_edit: QaEditFlags::default(),
                        qa_input: String::new(),
                        qa_state: None,
                        polish_result_visible: false,
                        selection_draft: String::new(),
                        selection: None,
                        remote_access: None,
                        provider_kind: openless_core::ChannelKind::Asr,
                        providers: ProvidersState::Loading,
                        selected_channel_id: None,
                        provider_editor: ProviderEditorState::Idle,
                        provider_editor_form: None,
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
                        capsule_popup: None,
                        popup_action_guard: PopupActionGuard::default(),
                        capsule_session: None,
                        capsule_dismissal_scheduled: None,
                        tray,
                        exit_requested: false,
                        update_support,
                        update_schedule: UpdateSchedule::new(Duration::ZERO),
                        update_started: std::time::Instant::now(),
                        last_pump_heartbeat: std::time::Instant::now(),
                        window_should_be_open,
                        ui_window: None,
                        ui_window_spawned_at: None,
                        last_snapshot_fingerprint: None,
                        last_snapshot_at: std::time::Instant::now(),
                        pending_ui_actions: Vec::new(),
                        pending_local_hotkeys: Vec::new(),
                        hotkey_dedupe: openless_linux_egui::HotkeyDeduplicator::default(),
                        popup_restarts: [PopupRestartBudget::default(); POPUP_KIND_COUNT],
                        hotkeys_sent: None,
                        pending_ui_pongs: Vec::new(),
                        update_manifest: None,
                        update_busy: false,
                        update_progress: None,
                        history_clip: None,
                        marketplace_items: Vec::new(),
                        marketplace_attempted: false,
                        marketplace_query: String::new(),
                        marketplace_flow: None,
                        marketplace_detail: None,
                        marketplace_my_packs: Vec::new(),
                        marketplace_my_likes: Vec::new(),
                        marketplace_seq: 0,
                        marketplace_search_deadline: None,
                        marketplace_likes_loaded: false,
                        style_editor: None,
                        style_hotkey_pack_id: String::new(),
                        style_hotkey_primary: String::new(),
                        style_hotkey_modifiers: String::new(),
                        status: tr_l10n(lang, "status.core_started").to_string(),
                        startup_error: None,
                        locale_pref,
                        lang,
                        active_page: shell::Page::Overview,
                        frontend_vm: FrontendViewModel::default(),
                        tx,
                        rx,
                    };
                    app.load_remote_status();
                    app.load_providers(openless_core::ChannelKind::Asr);
                    app.load_library();
                    app.load_microphones();
                    app.load_overview();
                    app
                }
                Err(error) => Self {
                    tokio,
                    native: None,
                    subscription: None,
                    snapshot: None,
                    hydrate_text_fields: true,
                    preferences: None,
                    settings_dirty: SettingsDirty::default(),
                    settings_channel_kind: openless_core::ChannelKind::Llm,
                    settings_channels: Vec::new(),
                    settings_channels_loading: false,
                    service_configured: [false; 2],
                    overview: OverviewState::Loading,
                    microphones: Vec::new(),
                    microphone_error: None,
                    transcript: String::new(),
                    transcript_state: TranscriptAccumulator::default(),
                    transcript_session: None,
                    recording_phase_active: false,
                    last_event_sequence: 0,
                    less_computer_input: String::new(),
                    less_computer_output: String::new(),
                    less_computer_turn_start: 0,
                    less_computer_session: None,
                    less_computer_entries: Vec::new(),
                    less_computer_working: false,
                    less_computer_popup: None,
                    pending_approval: None,
                    qa_visible: false,
                    qa_pinned: false,
                    qa_edit: QaEditFlags::default(),
                    qa_input: String::new(),
                    qa_state: None,
                    polish_result_visible: false,
                    selection_draft: String::new(),
                    selection: None,
                    remote_access: None,
                    provider_kind: openless_core::ChannelKind::Asr,
                    providers: ProvidersState::Loading,
                    selected_channel_id: None,
                    provider_editor: ProviderEditorState::Idle,
                    provider_editor_form: None,
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
                    capsule_popup: None,
                    popup_action_guard: PopupActionGuard::default(),
                    capsule_session: None,
                    capsule_dismissal_scheduled: None,
                    tray,
                    exit_requested: false,
                    update_support,
                    update_schedule: UpdateSchedule::new(Duration::ZERO),
                    update_started: std::time::Instant::now(),
                    last_pump_heartbeat: std::time::Instant::now(),
                    window_should_be_open,
                    ui_window: None,
                    ui_window_spawned_at: None,
                    last_snapshot_fingerprint: None,
                    last_snapshot_at: std::time::Instant::now(),
                    pending_ui_actions: Vec::new(),
                    pending_local_hotkeys: Vec::new(),
                    hotkey_dedupe: openless_linux_egui::HotkeyDeduplicator::default(),
                    popup_restarts: [PopupRestartBudget::default(); POPUP_KIND_COUNT],
                    hotkeys_sent: None,
                    pending_ui_pongs: Vec::new(),
                    update_manifest: None,
                    update_busy: false,
                    update_progress: None,
                    history_clip: None,
                    marketplace_items: Vec::new(),
                    marketplace_attempted: false,
                    marketplace_query: String::new(),
                    marketplace_flow: None,
                    marketplace_detail: None,
                    marketplace_my_packs: Vec::new(),
                    marketplace_my_likes: Vec::new(),
                    marketplace_seq: 0,
                    marketplace_search_deadline: None,
                    marketplace_likes_loaded: false,
                    style_editor: None,
                    style_hotkey_pack_id: String::new(),
                    style_hotkey_primary: String::new(),
                    style_hotkey_modifiers: String::new(),
                    status: tr_l10n(lang, "status.startup_failed").to_string(),
                    startup_error: Some(error),
                    locale_pref,
                    lang,
                    active_page: shell::Page::Overview,
                    frontend_vm: FrontendViewModel::default(),
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
                PopupKind::Capsule => &mut self.capsule_popup,
                PopupKind::LessComputer => &mut self.less_computer_popup,
            }
        }

        fn ensure_popup(&mut self, kind: PopupKind) {
            let lang = self.lang;
            if self.popup_slot(kind).is_some() {
                return;
            }
            match std::env::current_exe() {
                Ok(executable) => {
                    self.popup_action_guard.reset(kind);
                    let supervisor = PopupSupervisor::spawn(self.tokio.handle(), executable, kind);
                    *self.popup_slot(kind) = Some(supervisor);
                }
                Err(error) => self.status = fmt_l10n(lang, "popup.start_failed", &[&error]),
            }
        }

        fn send_popup(&mut self, kind: PopupKind, message: HostToPopup) {
            let lang = self.lang;
            // 记录胶囊当前承载的会话：兜底收起要靠它判断「会话是否还在快照里」。
            if let HostToPopup::Capsule { session_id, .. } = &message {
                self.capsule_session = Some(session_id.clone());
            }
            let retry = message.clone();
            if let Some(supervisor) = self.popup_slot(kind) {
                if let Err(error) = supervisor.try_send(message) {
                    self.status = fmt_l10n(lang, "popup.channel_rebuild", &[&format!("{error:?}")]);
                    *self.popup_slot(kind) = None;
                    self.ensure_popup(kind);
                    if let Some(supervisor) = self.popup_slot(kind) {
                        if let Err(retry_error) = supervisor.try_send(retry) {
                            self.status = fmt_l10n(
                                lang,
                                "popup.recover_failed",
                                &[&format!("{retry_error:?}")],
                            );
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

        /// Less Computer 面板的当前快照：宿主是事件序列的唯一所有者，弹窗进程
        /// 每次重连都收到完整序列（重开窗口不丢历史）。
        fn less_computer_snapshot(&self, lang: Lang) -> HostToPopup {
            let approval = self.pending_approval.as_ref().map(|(token, command)| {
                openless_linux_egui::LessComputerApproval {
                    token: token.clone(),
                    command: command.clone(),
                    reason: tr_l10n(lang, "less_computer.approval_rerun_warning").to_string(),
                }
            });
            HostToPopup::LessComputer {
                version: POPUP_PROTOCOL_VERSION,
                session_id: self
                    .less_computer_session
                    .map(|session| session.to_string())
                    .unwrap_or_else(|| "less-computer".to_string()),
                sequence: self.last_event_sequence.saturating_mul(2),
                entries: self.less_computer_entries.clone(),
                working: self.less_computer_working,
                approval,
                error: None,
            }
        }

        fn show_less_computer_popup(&mut self) {
            self.ensure_popup(PopupKind::LessComputer);
            let message = self.less_computer_snapshot(self.lang);
            self.send_popup(PopupKind::LessComputer, message);
        }

        /// ✕ 只收起面板：不动已完成的对话，也不结束进程。
        fn hide_less_computer_popup(&mut self) {
            let session_id = self
                .less_computer_session
                .map(|session| session.to_string())
                .unwrap_or_else(|| "less-computer".to_string());
            let sequence = self.last_event_sequence.saturating_mul(2).saturating_add(1);
            self.hide_popup(PopupKind::LessComputer, session_id, sequence);
        }

        fn expected_popup_session(&self, kind: PopupKind) -> Option<String> {
            match kind {
                PopupKind::Qa => self
                    .qa_state
                    .as_ref()
                    .map(|state| state.session_id.clone().unwrap_or_else(|| "qa".to_string())),
                PopupKind::Capsule => self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.dictation.session_id)
                    .map(|session_id| session_id.to_string()),
                PopupKind::LessComputer => self
                    .less_computer_session
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
                    edit_instruction_mode: self.qa_edit.instruction_mode,
                    edit_apply_available: self.qa_edit.apply_available,
                    edit_revert_available: self.qa_edit.revert_available,
                    pinned: self.qa_pinned,
                    viewer_login: self.marketplace_login(),
                },
            );
        }

        /// 当前 GitHub 登录名（设置里登录后写入偏好），用于追问头像。
        fn marketplace_login(&self) -> String {
            self.preferences
                .as_ref()
                .map(|prefs| prefs.marketplace_dev_login.trim().to_string())
                .unwrap_or_default()
        }

        /// 「预览并确认插入」：沿用 Tauri `confirm_selection_voice_preview` 的
        /// 四步（取 owner → 取预览文本 → 开 apply ticket → 原生落字 → finish），
        /// Linux 的原生落字走 fcitx5 选区替换。
        fn spawn_qa_edit_apply(
            &self,
            backend: std::sync::Arc<openless_core::OpenLessBackend>,
            qa_session: openless_core::SessionId,
        ) {
            let lang = self.lang;
            self.spawn(async move {
                let unavailable = || {
                    BackendError::new(
                        openless_core::BackendErrorCode::InvalidState,
                        "qa edit unavailable",
                    )
                };
                let services = backend.services();
                let snapshot = services.qa.snapshot().await?;
                let owner = snapshot.conversation_id.ok_or_else(unavailable)?;
                let preview = services
                    .selection_voice
                    .preview(Some(owner))
                    .await?
                    .ok_or_else(unavailable)?;
                let text = preview.text.trim().to_string();
                if text.is_empty() {
                    return Err(unavailable());
                }
                let ticket = services
                    .qa
                    .begin_edit_preview_apply(qa_session, text)
                    .await?;
                let outcome = match openless_linux_egui::apply_selection_voice_target(
                    &ticket.session_id.to_string(),
                    &ticket.source_text,
                    &ticket.replacement_text,
                ) {
                    Ok(()) => openless_core::SelectionVoiceApplyOutcome::Inserted,
                    Err(_) => openless_core::SelectionVoiceApplyOutcome::Failed,
                };
                let _ = services
                    .selection_voice
                    .finish_preview_apply(ticket.ticket_id, outcome)
                    .await;
                if outcome.may_have_applied() {
                    // 只剩「这一轮已经落字」的收尾：结束后再允许新一轮。
                    let _ = services.qa.dismiss_session(qa_session).await;
                }
                Ok(tr_l10n(lang, "selection.replaced").to_string())
            });
        }

        /// 润色结果：**不再有独立预览窗口**，一律送进选区助手面板的「润色结果」模式
        /// （用户确认的设计：选区只保留一个弹窗）。
        fn show_selection_popup(&mut self) {
            self.ensure_popup(PopupKind::Qa);
            let Some(selection) = self.selection.clone() else {
                return;
            };
            let Some(session_id) = selection.session_id else {
                return;
            };
            self.send_popup(
                PopupKind::Qa,
                HostToPopup::PolishPreview {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.to_string(),
                    sequence: self.last_event_sequence.saturating_mul(2),
                    text: selection.preview_text.unwrap_or_default(),
                    source: selection.source_text.unwrap_or_default(),
                },
            );
        }

        /// 胶囊是否允许显示（Tauri `showCapsule`；隐藏时提示音仍会响）。
        fn capsule_enabled(&self) -> bool {
            self.preferences
                .as_ref()
                .map(|prefs| prefs.show_capsule)
                .unwrap_or(true)
        }

        /// 胶囊样式标签：协议里传字符串，弹窗进程不需要 Core 的类型。
        fn capsule_style_tag(&self) -> String {
            match self.preferences.as_ref().map(|prefs| prefs.capsule_style) {
                Some(openless_core::shared_types::CapsuleStyle::Classic) => "classic",
                Some(openless_core::shared_types::CapsuleStyle::Typeless) => "typeless",
                _ => "siri",
            }
            .to_string()
        }

        fn show_capsule_popup(&mut self) {
            if !self.capsule_enabled() {
                return;
            }
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
            // 终态文案：Core 在失败时只给错误码名（`InvalidArgument`），成功时可能给
            // 内部状态词（`inserted`），都不能直接显示；分类规则在 dictation_feedback。
            let lang = self.lang;
            let text = match capsule_outcome(snapshot.phase, snapshot.message.as_deref()) {
                CapsuleOutcome::Inserted => {
                    frontend::popups::inserted_message(lang, self.transcript.chars().count())
                }
                CapsuleOutcome::Cancelled => tr_l10n(lang, "capsule.cancelled").to_string(),
                CapsuleOutcome::Failed => tr_l10n(lang, "capsule.error").to_string(),
                CapsuleOutcome::Progress(text) => text,
            };
            let style = self.capsule_style_tag();
            self.send_popup(
                PopupKind::Capsule,
                HostToPopup::Capsule {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.to_string(),
                    sequence: self.last_event_sequence.saturating_mul(2),
                    phase: format!("{:?}", snapshot.phase),
                    text,
                    audio_level: Some(snapshot.level),
                    translation_active: snapshot.translation_active,
                    style,
                },
            );
            self.schedule_capsule_dismissal(&session_id.to_string(), snapshot.phase);
        }

        /// 终态后按 Tauri Host 的时序自动收起胶囊：成功/失败停留 2 秒、
        /// 取消立刻；进行中的相位不收。
        fn schedule_capsule_dismissal(&mut self, session_id: &str, phase: DictationPhase) {
            // 诊断链路用（低噪声：一次听写一条）：这条日志缺失 = 终态事件没到宿主。
            let Some(delay) = capsule_hide_delay(phase) else {
                log::debug!("capsule: no dismissal for session {session_id} in {phase:?}");
                // 会话又回到进行中相位：旧计时作废。
                self.capsule_dismissal_scheduled = None;
                return;
            };
            self.capsule_dismissal_scheduled = Some(session_id.to_string());
            log::info!(
                "capsule: dismissal scheduled in {}ms for session {session_id} ({phase:?})",
                delay.as_millis()
            );
            let session_id = session_id.to_string();
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(UiResult::CapsuleDismissDue { session_id });
            });
        }

        /// 收起胶囊。三条窗口路径里只有 eframe 的两条支持「隐藏但保留进程」，
        /// layer surface 没有隐藏语义（只能销毁表面），所以统一结束弹窗进程：
        /// 下一次录音会在按热键那一刻按需重新拉起，用户看不到延迟。
        fn dismiss_capsule(&mut self) {
            let had_process = self.popup_slot(PopupKind::Capsule).is_some();
            if let Some(supervisor) = self.popup_slot(PopupKind::Capsule).as_ref() {
                let _ = supervisor.request_shutdown();
            }
            *self.popup_slot(PopupKind::Capsule) = None;
            self.capsule_session = None;
            self.capsule_dismissal_scheduled = None;
            // 这条日志缺失 = 收起决定没走到「结束弹窗进程」这一环。
            log::info!("capsule: dismissal applied (popup process was running: {had_process})");
        }

        fn poll_popup_supervisors(&mut self) {
            let lang = self.lang;
            let mut events = Vec::new();
            for kind in [PopupKind::Qa, PopupKind::Capsule] {
                if let Some(supervisor) = self.popup_slot(kind) {
                    while let Ok(event) = supervisor.try_recv() {
                        events.push((kind, event));
                    }
                }
            }
            for (kind, event) in events {
                if let PopupSupervisorEvent::Message(message) = &event {
                    let Some(expected_session) = self.expected_popup_session(kind) else {
                        self.status = tr_l10n(lang, "popup.ignore_no_session").to_string();
                        continue;
                    };
                    if !self
                        .popup_action_guard
                        .accept(kind, message, &expected_session)
                    {
                        self.status = tr_l10n(lang, "popup.ignore_stale").to_string();
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
                                Ok(tr_l10n(lang, "qa.submitted").to_string())
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
                                Ok(tr_l10n(lang, "qa.recording_updated").to_string())
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
                                Ok(tr_l10n(lang, "qa.closed").to_string())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::SetPinned {
                        session_id,
                        pinned,
                        ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        self.qa_pinned = pinned;
                        self.show_qa_popup();
                    }
                    PopupSupervisorEvent::Message(PopupToHost::SetEditInstructionMode {
                        session_id,
                        enabled,
                        ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend
                                    .services()
                                    .qa
                                    .set_edit_instruction_mode(enabled)
                                    .await?;
                                Ok(String::new())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::RevertEdit {
                        session_id, ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        if let Ok(qa_session) = session_id.parse::<uuid::Uuid>() {
                            let qa_session = openless_core::SessionId::from_uuid(qa_session);
                            if let Some(backend) = self.backend() {
                                let lang = self.lang;
                                self.spawn(async move {
                                    backend
                                        .services()
                                        .qa
                                        .revert_edit_preview(qa_session)
                                        .await?;
                                    Ok(tr_l10n(lang, "selection.reverted").to_string())
                                });
                            }
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::ApplyEdit {
                        session_id, ..
                    }) if self
                        .qa_state
                        .as_ref()
                        .and_then(|state| state.session_id.as_deref())
                        == Some(session_id.as_str()) =>
                    {
                        if let Ok(qa_session) = session_id.parse::<uuid::Uuid>() {
                            let qa_session = openless_core::SessionId::from_uuid(qa_session);
                            if let Some(backend) = self.backend() {
                                self.spawn_qa_edit_apply(backend, qa_session);
                            }
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::ConfirmPolish {
                        session_id,
                        text,
                        ..
                    }) => match session_id.parse::<uuid::Uuid>() {
                        Ok(session_id) => {
                            // 润色结束：选区助手面板回到提问模式（同一个弹窗）。
                            self.polish_result_visible = false;
                            let session_id = openless_core::SessionId::from_uuid(session_id);
                            if let Some(backend) = self.backend() {
                                self.spawn(async move {
                                    backend
                                        .services()
                                        .selection
                                        .confirm(session_id, Some(text))
                                        .await?;
                                    Ok(tr_l10n(lang, "selection.replaced").to_string())
                                });
                            }
                        }
                        Err(error) => {
                            self.status = fmt_l10n(lang, "popup.session_invalid", &[&error])
                        }
                    },
                    PopupSupervisorEvent::Message(PopupToHost::CancelPolish {
                        session_id, ..
                    }) => match session_id.parse::<uuid::Uuid>() {
                        Ok(session_id) => {
                            // 取消润色：同样退出润色模式。
                            self.polish_result_visible = false;
                            let session_id = openless_core::SessionId::from_uuid(session_id);
                            if let Some(backend) = self.backend() {
                                self.spawn(async move {
                                    backend
                                        .services()
                                        .selection
                                        .cancel(Some(session_id))
                                        .await?;
                                    Ok(tr_l10n(lang, "selection.cancelled").to_string())
                                });
                            }
                        }
                        Err(error) => {
                            self.status = fmt_l10n(lang, "popup.session_invalid", &[&error])
                        }
                    },
                    PopupSupervisorEvent::Message(PopupToHost::Ready { .. }) => match kind {
                        PopupKind::Qa => {
                            // 面板自己也要匹配本地热键（面板有焦点时插件收不到按键），
                            // 所以先下发绑定，再送内容。
                            self.send_popup_hotkeys(kind);
                            // 选区助手面板既可能是提问模式，也可能是润色结果模式。
                            if self.polish_result_visible {
                                self.show_selection_popup();
                            } else {
                                self.show_qa_popup();
                            }
                        }
                        PopupKind::Capsule => self.show_capsule_popup(),
                        PopupKind::LessComputer => {
                            self.send_popup_hotkeys(kind);
                            self.show_less_computer_popup();
                        }
                    },
                    PopupSupervisorEvent::Message(PopupToHost::Hotkey { edge, .. }) => {
                        log::info!("[hotkey] local edge from the {kind:?} panel: {edge:?}");
                        self.pending_local_hotkeys
                            .push((std::time::Instant::now(), edge));
                    }
                    PopupSupervisorEvent::Message(PopupToHost::DismissCapsule { .. }) => {
                        if let Some(snapshot) = self.snapshot.as_mut() {
                            snapshot.dictation.message = None;
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::CancelDictation { .. }) => {
                        // 胶囊 ✕：放弃这次听写。
                        let session = self
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.dictation.session_id);
                        if let (Some(backend), Some(session)) = (self.backend(), session) {
                            self.spawn(async move {
                                // 连点两次 ✕、会话已收尾之类的错误是预期内的，
                                // 归一掉，不要再弹成失败。
                                normalize_stop_result(
                                    backend.cancel_dictation(Some(session)).await,
                                )?;
                                Ok(String::new())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::StopDictation { .. }) => {
                        // 胶囊 ✓：结束录音并落字。
                        let session = self
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.dictation.session_id);
                        if let (Some(backend), Some(session)) = (self.backend(), session) {
                            self.spawn(async move {
                                // 没说话（空音频 → InvalidArgument）也是预期内的终态：
                                // 胶囊会显示本地化文案并自动收起，这里不再报错误。
                                normalize_stop_result(
                                    backend.stop_dictation_session(session).await,
                                )?;
                                Ok(String::new())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::SubmitLessComputer {
                        session_id,
                        text,
                        ..
                    }) if self
                        .less_computer_session
                        .map(|session| session.to_string())
                        .as_deref()
                        == Some(session_id.as_str()) =>
                    {
                        if let Some(backend) = self.backend() {
                            // Core 自己解析 provider / 模型 / 权限 / workdir；
                            // 宿主只负责把用户文本交给它（Tauri `lessComputerSubmitText`）。
                            self.spawn(async move {
                                backend.submit_less_computer(text).await?;
                                Ok(String::new())
                            });
                        }
                    }
                    PopupSupervisorEvent::Message(PopupToHost::ApproveLessComputer {
                        token,
                        approved,
                        ..
                    }) => {
                        let backend = self.backend();
                        self.spawn(async move {
                            if let Some(backend) = backend {
                                backend
                                    .services()
                                    .less_computer
                                    .approve(token, approved)
                                    .await?;
                            }
                            Ok(String::new())
                        });
                    }
                    PopupSupervisorEvent::Message(PopupToHost::CancelLessComputer { .. }) => {
                        let session = self.less_computer_session;
                        let backend = self.backend();
                        self.spawn(async move {
                            if let Some(backend) = backend {
                                backend.cancel_less_computer(session).await?;
                            }
                            Ok(String::new())
                        });
                    }
                    PopupSupervisorEvent::Message(PopupToHost::DismissLessComputer { .. }) => {
                        // 只收起面板：已完成的一轮保留在宿主状态里，下次打开仍在。
                        self.hide_less_computer_popup();
                    }
                    PopupSupervisorEvent::Message(
                        PopupToHost::SubmitQa { .. }
                        | PopupToHost::ToggleQaRecording { .. }
                        | PopupToHost::DismissQa { .. }
                        | PopupToHost::SetPinned { .. }
                        | PopupToHost::SetEditInstructionMode { .. }
                        | PopupToHost::ApplyEdit { .. }
                        | PopupToHost::RevertEdit { .. }
                        | PopupToHost::SubmitLessComputer { .. },
                    ) => {
                        self.status = tr_l10n(lang, "popup.ignore_late_qa").to_string();
                    }
                    PopupSupervisorEvent::ProtocolError(error) => {
                        self.status = fmt_l10n(lang, "popup.protocol_error", &[&error]);
                    }
                    PopupSupervisorEvent::SpawnFailed(error) => {
                        self.status = fmt_l10n(lang, "popup.spawn_failed", &[&error]);
                        *self.popup_slot(kind) = None;
                    }
                    PopupSupervisorEvent::Exited { code, crashed } => {
                        if crashed {
                            self.status = fmt_l10n(lang, "popup.exited", &[&format!("{code:?}")]);
                        }
                        *self.popup_slot(kind) = None;
                        if crashed {
                            // 必崩的面板不做无限重开：预算内重开，超了就停手（否则
                            // 用户看到的是“弹窗一直反复弹出”）。
                            let restarts = &mut self.popup_restarts[popup_kind_index(kind)];
                            if !restarts.allow(std::time::Instant::now()) {
                                log::warn!(
                                    "[popup] {kind:?} crashed {POPUP_RESTART_LIMIT} times within {}s; not restarting",
                                    POPUP_RESTART_WINDOW.as_secs()
                                );
                                return;
                            }
                            match kind {
                                PopupKind::Qa if self.qa_visible => self.show_qa_popup(),
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

        /// Refresh the required-service dots on the AI-services tabs.
        fn load_service_configured(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let mut configured = [false; 2];
                for (index, kind) in [
                    (0usize, openless_core::ChannelKind::Llm),
                    (1usize, openless_core::ChannelKind::Asr),
                ] {
                    if let Ok(channels) = backend.list_channels(kind).await {
                        configured[index] = channels.iter().any(|channel| channel.enabled);
                    }
                }
                let _ = tx.send(UiResult::ServiceConfigured(configured));
            });
        }

        /// Load the credential channels for the settings modal's AI-services tab.
        fn load_settings_channels(&mut self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let kind = self.settings_channel_kind;
            self.settings_channels_loading = true;
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = async {
                    let channels = backend.list_channels(kind).await?;
                    let account = model_account(kind).to_string();
                    let mut rows = Vec::with_capacity(channels.len());
                    for channel in channels {
                        let model = read_provider_value(&backend, kind, &channel.id, &account)
                            .await?
                            .unwrap_or_default();
                        rows.push(SettingsChannelRow {
                            id: channel.id.clone(),
                            name: channel.name.clone(),
                            provider_type: channel.provider_type.clone(),
                            model,
                            enabled: channel.enabled,
                            last_ok: channel.last_test.as_ref().map(|test| test.ok),
                            last_latency_ms: channel
                                .last_test
                                .as_ref()
                                .and_then(|test| test.latency_ms),
                            last_error: channel
                                .last_test
                                .as_ref()
                                .and_then(|test| test.error.clone()),
                        });
                    }
                    Ok::<_, BackendError>(rows)
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::SettingsChannels(result));
            });
        }

        fn load_marketplace(&mut self) {
            let Some(backend) = self.backend() else {
                return;
            };
            self.marketplace_attempted = false;
            // An explicit fetch supersedes a queued debounced search.
            self.marketplace_search_deadline = None;
            self.marketplace_seq = self.marketplace_seq.wrapping_add(1);
            let seq = self.marketplace_seq;
            // Likes only power the 「我赞过的」 filter; mirror the Tauri page by
            // fetching them once, then reusing the cached set for every search.
            if !self.marketplace_likes_loaded {
                self.marketplace_likes_loaded = true;
                self.load_marketplace_likes();
            }
            let query = self.marketplace_query.trim().to_string();
            // The backend only ranks by popular/new; 「我赞过的」 is a filter over
            // the signed-in user's like list, exactly like the Tauri page.
            let sort = match self.frontend_vm.marketplace_sort {
                frontend::view_model::MarketplaceSort::Popular
                | frontend::view_model::MarketplaceSort::Liked => "popular",
                frontend::view_model::MarketplaceSort::New => "new",
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .marketplace
                    .list(openless_core::MarketplaceQuery {
                        query: (!query.is_empty()).then_some(query),
                        sort: Some(sort.to_string()),
                        limit: Some(50),
                    })
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Marketplace(seq, result));
            });
        }

        /// Fetch the signed-in user's like ids. Kept separate from
        /// [`Self::load_marketplace`] so searching never re-requests them.
        fn load_marketplace_likes(&mut self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let likes = backend
                    .services()
                    .marketplace
                    .my_likes()
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::MarketplaceLikes(likes));
            });
        }

        /// Same 300ms pause the Tauri marketplace page applies to its search box.
        const MARKETPLACE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

        fn schedule_marketplace_search(&mut self) {
            self.marketplace_search_deadline =
                Some(std::time::Instant::now() + Self::MARKETPLACE_SEARCH_DEBOUNCE);
        }

        /// Runs from the host tick: fires the debounced search exactly once.
        fn poll_marketplace_search(&mut self) {
            let Some(deadline) = self.marketplace_search_deadline else {
                return;
            };
            if std::time::Instant::now() < deadline {
                return;
            }
            self.load_marketplace();
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

        /// Load the real Core-backed Overview data off the egui frame. The only
        /// blocking reads (`list_history`, `list_activity`) are pushed to a
        /// blocking task so an egui frame never waits on disk/repository IO.
        fn load_overview(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = async {
                    let credentials = backend.get_credentials_status().await?;
                    let history_backend = Arc::clone(&backend);
                    let activity_backend = Arc::clone(&backend);
                    let history =
                        tokio::task::spawn_blocking(move || history_backend.list_history())
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })??;
                    let activity =
                        tokio::task::spawn_blocking(move || activity_backend.list_activity())
                            .await
                            .map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::Internal,
                                    error.to_string(),
                                )
                            })??;
                    Ok::<_, BackendError>(OverviewData {
                        credentials,
                        history,
                        activity,
                    })
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Overview(result));
            });
        }

        fn request_update_check(&mut self, channel: openless_core::shared_types::UpdateChannel) {
            let lang = self.lang;
            let LinuxUpdateSupport::AppImage(updater) = self.update_support.clone() else {
                self.status = tr_l10n(lang, "update.system_managed").to_string();
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

        fn drain_tray(&mut self, _ctx: &egui::Context) {
            let lang = self.lang;
            let mut commands = Vec::new();
            if let Some(tray) = &self.tray {
                tray.drain(|command| commands.push(command));
                if let Some(error) = tray.take_error() {
                    self.status = fmt_l10n(lang, "status.tray_stopped", &[&error]);
                    self.tray = None;
                }
            }
            for command in commands {
                match command {
                    openless_linux_egui::TrayCommand::ShowMain => {
                        // 托盘是用户的显式动作：拉起窗口进程（已有窗口时由它自己抬起）。
                        self.request_main_window();
                    }
                    openless_linux_egui::TrayCommand::ActivatePreviousStyle => {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                let pack = backend.activate_previous_style_pack()?;
                                Ok(match pack {
                                    Some(pack) => {
                                        fmt_l10n(lang, "status.style_switched", &[&pack.name])
                                    }
                                    None => tr_l10n(lang, "status.no_previous_style").to_string(),
                                })
                            });
                        }
                    }
                    openless_linux_egui::TrayCommand::SelectMicrophone(name) => {
                        if let Some(backend) = self.backend() {
                            let selected = if name.is_empty() {
                                tr_l10n(lang, "settings.system_default").to_string()
                            } else {
                                name.clone()
                            };
                            self.spawn(async move {
                                backend.select_microphone_device(name)?;
                                Ok(fmt_l10n(lang, "status.mic_selected", &[&selected]))
                            });
                        }
                    }
                    openless_linux_egui::TrayCommand::Quit => {
                        // 宿主退出前会给 UI 发 Shutdown（见 `run_host` 收尾）。
                        self.exit_requested = true;
                    }
                }
            }
        }

        /// Payload Core needs for the current draft. Returns `None` while no
        /// channel editor is loaded, so a stale frame cannot rename or re-\n        /// credential the wrong channel.
        fn editor_from_form(&self) -> Option<ProviderEditor> {
            let form = self.provider_editor_form.as_ref()?;
            let ProviderEditorState::Loaded(loaded) = &self.provider_editor else {
                return None;
            };
            if loaded.channel.id != form.channel_id {
                return None;
            }
            let mut editor = (**loaded).clone();
            editor.name = form.name.clone();
            editor.endpoint = form.endpoint.clone();
            editor.model = form.model.clone();
            editor.resource_id = form.resource_id.clone();
            editor.auth_mode = form.auth_mode.clone();
            editor.primary_secret = form.primary_secret.clone();
            editor.secondary_secret = form.secondary_secret.clone();
            Some(editor)
        }

        /// Open a channel's provider editor. The descriptor comes from Core's
        /// provider rules, so the UI never invents a field shape; without one
        /// the editor stays closed instead of guessing.
        fn open_provider_editor(&mut self, index: usize) {
            let Some(channel_id) = self
                .settings_channels
                .get(index)
                .map(|channel| channel.id.clone())
            else {
                return;
            };
            let panel = match &self.providers {
                ProvidersState::Loaded(panel) => panel.clone(),
                _ => return,
            };
            let Some((channel, descriptor)) = provider_channel_descriptor(&panel, &channel_id)
            else {
                return;
            };
            let kind = panel.kind;
            self.selected_channel_id = Some(channel_id.clone());
            self.provider_editor = ProviderEditorState::Loading { kind, channel_id };
            self.provider_editor_form = None;
            self.load_provider_editor(kind, channel, descriptor);
        }

        fn close_provider_editor(&mut self) {
            self.provider_editor = ProviderEditorState::Idle;
            self.provider_editor_form = None;
            self.frontend_vm.provider_editor = None;
        }

        /// Core owns the model catalog; the host only forwards the request.
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
                        thinking_enabled: false,
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

        /// Play the native recording start/stop cue on a worker thread, gated by
        /// the `audio_cue_on_record` preference. The start cue is additionally
        /// suppressed while `mute_during_recording` is active: playing into a
        /// deliberately muted sink is both inaudible and a needless PipeWire/
        /// KDE sink-input blip. The stop cue plays after output has been
        /// restored. Absent preferences default to Core's defaults (cue on,
        /// mute off).
        fn play_record_cue(&self, at_start: bool) {
            let enabled = self
                .preferences
                .as_ref()
                .map(|prefs| prefs.audio_cue_on_record)
                .unwrap_or(true);
            if !enabled {
                return;
            }
            if at_start
                && self
                    .preferences
                    .as_ref()
                    .map(|prefs| prefs.mute_during_recording)
                    .unwrap_or(false)
            {
                return;
            }
            if at_start {
                openless_linux_egui::play_cue_start();
            } else {
                openless_linux_egui::play_cue_stop();
            }
        }

        fn apply_event(&mut self, event: BackendEvent) {
            let lang = self.lang;
            if event.sequence <= self.last_event_sequence {
                return;
            }
            let event_sequence = event.sequence;
            self.last_event_sequence = event.sequence;
            let session_id = event.session_id;
            match event.kind {
                BackendEventKind::DictationStateChanged(state) => {
                    // Native start/stop audio cues are a Linux host effect (no
                    // webview to synthesize them), gated by `audio_cue_on_record`
                    // and muted-aware. They must never block this frame, so the
                    // cue module plays on its own worker thread.
                    let was_recording = self.recording_phase_active;
                    self.recording_phase_active = state.phase == DictationPhase::Recording;
                    if state.phase == DictationPhase::Recording && !was_recording {
                        self.play_record_cue(true);
                    } else if !self.recording_phase_active && was_recording {
                        self.play_record_cue(false);
                    }
                    if state.phase == DictationPhase::Starting {
                        self.transcript_state = TranscriptAccumulator::default();
                        self.transcript.clear();
                        self.transcript_session = state.session_id;
                    }
                    // 终态不写状态栏：`Failed` / `Completed` / `Cancelled` 是 Core 的
                    // 内部词，用户已经能从胶囊看到本地化文案（Tauri 也只在那里显示）。
                    if capsule_hide_delay(state.phase).is_some() {
                        log::debug!(
                            "dictation terminal phase {:?} (session {:?})",
                            state.phase,
                            state.session_id
                        );
                    } else {
                        self.status = fmt_l10n(
                            lang,
                            "status.dictation_phase",
                            &[&format!("{:?}", state.phase)],
                        );
                    }
                    if let Some(session_id) = state.session_id {
                        // 上一轮胶囊被自动收起后进程已经不在了：进行中的相位必须按需
                        // 重新拉起，否则 send_popup 会因为没有 supervisor 而静默丢弃；
                        // 终态则不拉，免得把刚收起的药丸又喊回来。
                        if phase_shows_capsule(state.phase) && self.capsule_enabled() {
                            self.ensure_popup(PopupKind::Capsule);
                        }
                        // 进行中的 message 也要过一遍分类：Core 偶尔把内部错误码
                        // 写在这里，不能当成文案直接显示。
                        let text = match capsule_outcome(state.phase, state.message.as_deref()) {
                            CapsuleOutcome::Progress(text) => text,
                            _ => String::new(),
                        };
                        let style = self.capsule_style_tag();
                        self.send_popup(
                            PopupKind::Capsule,
                            HostToPopup::Capsule {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: session_id.to_string(),
                                sequence: event_sequence.saturating_mul(2),
                                phase: format!("{:?}", state.phase),
                                text,
                                audio_level: Some(state.level),
                                translation_active: state.translation_active,
                                style,
                            },
                        );
                        // 终态：按 Tauri 时序安排自动收起，否则药丸会一直贴在屏幕上。
                        self.schedule_capsule_dismissal(&session_id.to_string(), state.phase);
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
                    self.status = fmt_l10n(
                        lang,
                        "status.dictation_done",
                        &[&format!("{:?}", result.inserted)],
                    );
                }
                BackendEventKind::RecordingControlRequested(request) => {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            match request.action {
                                openless_core::RecordingControlAction::Stop => {
                                    normalize_stop_result(
                                        backend.stop_dictation_session(request.session_id).await,
                                    )?;
                                }
                                openless_core::RecordingControlAction::Cancel => {
                                    normalize_stop_result(
                                        backend.cancel_dictation(Some(request.session_id)).await,
                                    )?;
                                }
                            }
                            Ok(tr_l10n(lang, "status.auto_stopped").to_string())
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
                        if *fresh {
                            self.less_computer_entries.clear();
                        }
                        self.less_computer_entries
                            .push(openless_linux_egui::LessComputerEntry {
                                kind: "user".to_string(),
                                text: text.clone(),
                            });
                    } else if session_id != self.less_computer_session {
                        return;
                    }
                    match event.kind {
                        // Linux已有独立录音显示；新typed反馈供接手Host/UI团队继续接入。
                        LessComputerEventKind::VoiceState { .. } => {}
                        LessComputerEventKind::User { .. } => {}
                        LessComputerEventKind::Started => {
                            self.status = tr_l10n(lang, "status.less_running").to_string();
                            self.less_computer_working = true;
                        }
                        LessComputerEventKind::Delta { text } => {
                            self.less_computer_output.push_str(&text);
                            append_assistant_entry(&mut self.less_computer_entries, &text);
                        }
                        LessComputerEventKind::Tool { name } => {
                            self.status = fmt_l10n(lang, "status.less_tool", &[&name]);
                            self.less_computer_entries.push(
                                openless_linux_egui::LessComputerEntry {
                                    kind: "tool".to_string(),
                                    // 行内标记的文案在宿主侧本地化：面板只画文本。
                                    text: fmt_l10n(lang, "less_computer.tool", &[&name]),
                                },
                            );
                        }
                        LessComputerEventKind::Compaction => {
                            self.status = tr_l10n(lang, "status.less_compacted").to_string();
                            self.less_computer_entries.push(
                                openless_linux_egui::LessComputerEntry {
                                    kind: "compaction".to_string(),
                                    text: tr_l10n(lang, "less_computer.compaction").to_string(),
                                },
                            );
                        }
                        LessComputerEventKind::Completed { text, cost_usd } => {
                            // A terminal is authoritative even for final-only
                            // providers or after a missed partial event.
                            self.less_computer_output
                                .truncate(self.less_computer_turn_start);
                            self.less_computer_output.push_str(&text);
                            self.pending_approval = None;
                            self.less_computer_working = false;
                            // 终局正文替换掉流式累积的那条助手条目。
                            match self
                                .less_computer_entries
                                .iter_mut()
                                .rev()
                                .find(|entry| entry.kind == "assistant")
                            {
                                Some(entry) => entry.text = text.clone(),
                                None => self.less_computer_entries.push(
                                    openless_linux_egui::LessComputerEntry {
                                        kind: "assistant".to_string(),
                                        text: text.clone(),
                                    },
                                ),
                            }
                            if let Some(cost) = cost_usd {
                                let cost_text =
                                    fmt_l10n(lang, "less_computer.cost", &[&format!("{cost:.3}")]);
                                self.less_computer_entries.push(
                                    openless_linux_egui::LessComputerEntry {
                                        kind: "note".to_string(),
                                        text: cost_text,
                                    },
                                );
                            }
                            self.status = tr_l10n(lang, "less_computer.done").to_string();
                        }
                        LessComputerEventKind::Approval { token, command, .. } => {
                            self.pending_approval = Some((token, command));
                            self.status = tr_l10n(lang, "status.less_waiting").to_string();
                        }
                        LessComputerEventKind::Error { message } => {
                            self.pending_approval = None;
                            self.less_computer_working = false;
                            self.less_computer_entries.push(
                                openless_linux_egui::LessComputerEntry {
                                    kind: "error".to_string(),
                                    text: message.clone(),
                                },
                            );
                            self.status = message;
                        }
                        LessComputerEventKind::Cancelled => {
                            self.pending_approval = None;
                            self.less_computer_working = false;
                            self.status = tr_l10n(lang, "less_computer.cancelled").to_string();
                        }
                    }
                }
                BackendEventKind::PreferencesChanged(_) => {
                    // 外部改动（Core 事件 / 托盘 / 另一窗口）要重新灌一次文本行。
                    self.hydrate_text_fields = true;
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
                    self.qa_edit.merge(&state);
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
                                edit_instruction_mode: self.qa_edit.instruction_mode,
                                edit_apply_available: self.qa_edit.apply_available,
                                edit_revert_available: self.qa_edit.revert_available,
                                pinned: self.qa_pinned,
                                viewer_login: self.marketplace_login(),
                            },
                        );
                    }
                }
                BackendEventKind::SelectionStateChanged(snapshot) => {
                    if snapshot.phase == SelectionPhase::Preview {
                        self.selection_draft = snapshot.preview_text.clone().unwrap_or_default();
                        self.polish_result_visible = true;
                    }
                    if let Some(session_id) = snapshot.session_id {
                        // 选区助手面板：润色结果以「润色结果」帧送进去。
                        self.ensure_popup(PopupKind::Qa);
                        self.send_popup(
                            PopupKind::Qa,
                            HostToPopup::PolishPreview {
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

        /// `update()` 是唯一 drain 原生事件（热键、单实例拉起意图）的地方，而
        /// 最小化/隐藏的窗口会让 eframe 的定时重绘停摆 —— 那样按热键什么都不会
        /// 发生（胶囊、QA 面板都不弹）。这里必须用**真线程**：`self.tokio` 是
        /// current-thread 运行时，`spawn` 的任务只在别处 `block_on` 时才被推进，
        /// 当作后台泵用就是「写完看着对、最小化后照样死」（实测心跳会在窗口
        /// 收走的那一刻停）。线程只做一件事：`request_repaint()` 把事件循环戳醒。
        /// 泵心跳：宿主循环每 10s 一条。窗口关掉之后这条心跳**不能**停 ——
        /// 停了就说明热键消费与弹窗拉起也没在跑（这正是当初「关窗后热键失效」
        /// 的判据），用户/支持可以直接看日志确认。
        fn log_pump_heartbeat(&mut self, ctx: &egui::Context) {
            if self.last_pump_heartbeat.elapsed() < std::time::Duration::from_secs(10) {
                return;
            }
            self.last_pump_heartbeat = std::time::Instant::now();
            // 宿主没有窗口，所以心跳只看「窗口进程还在不在」——
            // 这正是关窗后必须继续为 true 的那一项能力。
            let _ = ctx;
            log::info!(
                "[pump] heartbeat window_process={} window_wanted={} tray={} recording={}",
                self.ui_window.is_some(),
                self.window_should_be_open,
                self.tray.is_some(),
                self.recording_phase_active,
            );
        }

        /// 当前生效的热键配置（本地匹配与去重都用它）。
        fn hotkey_target(&self) -> Option<openless_core::HotkeyRuntimeTarget> {
            self.preferences
                .as_ref()
                .map(openless_core::HotkeyRuntimeTarget::from)
        }

        /// QA 热键 = **面板显隐**。
        ///
        /// Tauri `coordinator/qa.rs::handle_qa_hotkey_pressed`：面板可见 →
        /// `qa.dismiss()`，否则 `qa.show()`。egui 侧原来把 QA 热键接到 Core 的
        /// `CliIntent::ToggleQa`，而那条是**切换录音**（`qa.toggle_recording()`）：
        /// 于是「想打开面板」变成了「开一次录音」，用户看到的正是「选区助手弹出并
        /// 开始录音、还停不下来」。面板显隐属宿主状态（`qa_visible`），Core 只按
        /// ShowQa/HideQa 指令把窗口开合。
        fn toggle_qa_panel(&mut self) {
            let lang = self.lang;
            if self.qa_visible {
                // 显式收起不受图钉门禁限制（Tauri 的 HostAction::HideQa 同样无条件收窗），
                // 图钉只管“失焦自动收起”那条路径。
                log::info!("[hotkey] QA panel toggle: dismissing");
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
                if let Some(backend) = self.backend() {
                    self.spawn(async move {
                        backend.services().qa.dismiss().await?;
                        Ok(tr_l10n(lang, "qa.closed").to_string())
                    });
                }
            } else {
                log::info!("[hotkey] QA panel toggle: showing");
                if let Some(backend) = self.backend() {
                    // `show()` 只发 HostAction::ShowQa：Core 仍停在 Idle，不录音。
                    self.spawn(async move {
                        backend.services().qa.show().await?;
                        Ok(String::new())
                    });
                }
            }
        }

        /// 面板录音开关（Tauri `coordinator/qa.rs::handle_qa_option_edge`）。
        fn toggle_qa_recording(&mut self) {
            let lang = self.lang;
            if let Some(backend) = self.backend() {
                self.spawn(async move {
                    backend.services().qa.toggle_recording().await?;
                    Ok(tr_l10n(lang, "qa.recording_updated").to_string())
                });
            }
        }

        /// 听写是否空闲（Tauri 的 QA 门禁要求 `DictationPhase::Idle`）。
        fn dictation_is_idle(&self) -> bool {
            match self.snapshot.as_ref() {
                Some(snapshot) => snapshot.dictation.phase == DictationPhase::Idle,
                None => true,
            }
        }

        /// 采纳一条插件热键信号？（与窗口报上来的本地边沿去重。）
        fn accept_plugin_hotkey(&mut self, event: &LinuxHotkeyEvent) -> bool {
            let Some(target) = self.hotkey_target() else {
                return true;
            };
            match openless_linux_egui::plugin_event_hotkey(event, &target) {
                Some(hotkey) => self
                    .hotkey_dedupe
                    .accept_signal(&hotkey, std::time::Instant::now()),
                None => true,
            }
        }

        /// 宿主自己处理掉的热键（不发往 Core）。返回 true 表示已处理。
        fn intercept_hotkey(&mut self, event: &LinuxHotkeyEvent) -> bool {
            match event {
                LinuxHotkeyEvent::QaPressed => {
                    log::info!("[hotkey] selection-ask hotkey: toggling the panel");
                    self.toggle_qa_panel();
                    true
                }
                // Tauri `coordinator/dictation_core.rs::handle_pressed_edge`：面板可见
                // 且听写空闲时，听写热键**按下**先切面板录音，而不是开始一次听写。
                // 这条缺失正是「选区助手里开始录音后，按语音热键完全没反应」的成因：
                // 原来一律送 Core 听写，Core 因 QA 正忙而拒绝，界面自然没反应。
                LinuxHotkeyEvent::DictationPressed { .. }
                    if self.qa_visible && self.dictation_is_idle() =>
                {
                    log::info!(
                        "[hotkey] dictation hotkey while the QA panel is visible: toggling QA recording"
                    );
                    self.toggle_qa_recording();
                    true
                }
                _ => false,
            }
        }

        /// 处理窗口/面板报上来的本地热键边沿，返回需要发往 Core 的事件。
        ///
        /// 与插件信号共用同一套门禁（QA 显隐、听写热键的面板录音切换），因此两条
        /// 通路的行为逐字一致；去重保证同一个物理按键只生效一次。
        fn apply_local_hotkey_edges(
            &mut self,
            edges: Vec<(std::time::Instant, openless_linux_egui::LocalHotkeyEdge)>,
        ) -> Vec<LinuxHotkeyEvent> {
            use openless_linux_egui::{LocalHotkey, LocalHotkeyEdgeKind};
            let Some(target) = self.hotkey_target() else {
                return Vec::new();
            };
            let mut events = Vec::new();
            for (at, edge) in edges {
                if !self.hotkey_dedupe.accept_local(&edge.hotkey, at) {
                    log::debug!(
                        "[hotkey] local {:?} ignored: a plugin signal just handled it",
                        edge.hotkey
                    );
                    continue;
                }
                // 单发事件（翻译/切换风格/划词润色/打开应用/风格包）只在按下或
                // 一次完整单击时触发；松开不再重复发一次。
                let single_shot = edge.kind != LocalHotkeyEdgeKind::Released;
                match &edge.hotkey {
                    LocalHotkey::Qa => {
                        log::info!("[hotkey] local selection-ask hotkey: toggling the panel");
                        self.toggle_qa_panel();
                    }
                    LocalHotkey::Dictation
                        if edge.kind == LocalHotkeyEdgeKind::Pressed
                            && self.qa_visible
                            && self.dictation_is_idle() =>
                    {
                        log::info!(
                            "[hotkey] local dictation hotkey while the QA panel is visible: toggling QA recording"
                        );
                        self.toggle_qa_recording();
                    }
                    LocalHotkey::Dictation => events.push(match edge.kind {
                        LocalHotkeyEdgeKind::Pressed => LinuxHotkeyEvent::DictationPressed {
                            symbol: 0,
                            states: 0,
                            press_id: edge.press_id,
                            at,
                        },
                        LocalHotkeyEdgeKind::Released => LinuxHotkeyEvent::DictationReleased {
                            symbol: 0,
                            states: 0,
                            press_id: edge.press_id,
                            at,
                        },
                        LocalHotkeyEdgeKind::Combined => LinuxHotkeyEvent::DictationCombined {
                            symbol: 0,
                            states: 0,
                            press_id: edge.press_id,
                            at,
                        },
                    }),
                    LocalHotkey::LessComputer => {
                        if !single_shot {
                            continue;
                        }
                        events.push(match edge.kind {
                            LocalHotkeyEdgeKind::Pressed => LinuxHotkeyEvent::LessComputerPressed {
                                symbol: 0,
                                states: 0,
                                press_id: edge.press_id,
                                at,
                            },
                            LocalHotkeyEdgeKind::Released => continue,
                            LocalHotkeyEdgeKind::Combined => {
                                LinuxHotkeyEvent::LessComputerCombined {
                                    symbol: 0,
                                    states: 0,
                                    press_id: edge.press_id,
                                    at,
                                }
                            }
                        });
                    }
                    LocalHotkey::Translation => {
                        if single_shot {
                            events.push(LinuxHotkeyEvent::TranslationPressed);
                        }
                    }
                    LocalHotkey::SwitchStyle => {
                        if single_shot {
                            events.push(LinuxHotkeyEvent::SwitchStylePressed);
                        }
                    }
                    LocalHotkey::SelectionPolish => {
                        if single_shot {
                            events.push(LinuxHotkeyEvent::SelectionPolishPressed);
                        }
                    }
                    LocalHotkey::OpenApp => {
                        if single_shot {
                            events.push(LinuxHotkeyEvent::OpenAppPressed);
                        }
                    }
                    LocalHotkey::StylePack(pack_id) => {
                        if !single_shot {
                            continue;
                        }
                        // Core 按 (keysym, states) 认包，所以带上与注册插件同源的换算。
                        if let Some((symbol, states)) =
                            openless_linux_egui::style_pack_raw(&target, pack_id)
                        {
                            events.push(LinuxHotkeyEvent::StylePackPressed { symbol, states });
                        }
                    }
                }
            }
            events
        }

        /// 把热键配置发给一个面板（面板有焦点时也要自己匹配本地热键）。
        fn send_popup_hotkeys(&mut self, kind: PopupKind) {
            let Some(bindings) = self.hotkey_target() else {
                return;
            };
            let session_id = match kind {
                PopupKind::Qa => self
                    .qa_state
                    .as_ref()
                    .and_then(|state| state.session_id.clone())
                    .unwrap_or_else(|| "qa".to_string()),
                PopupKind::LessComputer => self
                    .less_computer_session
                    .map(|session| session.to_string())
                    .unwrap_or_else(|| "less-computer".to_string()),
                // 胶囊不接受键盘焦点，本地匹配对它没有意义。
                PopupKind::Capsule => return,
            };
            self.send_popup(
                kind,
                HostToPopup::Hotkeys {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id,
                    sequence: self.last_event_sequence.saturating_mul(2).saturating_add(1),
                    bindings: Box::new(bindings),
                },
            );
        }

        /// 配置变化后把热键下发给窗口与面板（窗口进程重启由 Hello 强制重发）。
        fn sync_hotkey_bindings(&mut self, bridge: &mut UiBridgeHost) {
            let Some(bindings) = self.hotkey_target() else {
                return;
            };
            if self.hotkeys_sent.as_ref() == Some(&bindings) {
                return;
            }
            if bridge.is_connected() {
                bridge.send(HostToWindow::Hotkeys {
                    version: UI_BRIDGE_VERSION,
                    bindings: Box::new(bindings.clone()),
                });
            }
            self.send_popup_hotkeys(PopupKind::Qa);
            self.send_popup_hotkeys(PopupKind::LessComputer);
            self.hotkeys_sent = Some(bindings);
        }

        // 宿主没有窗口：ctx 只为保持调用形状（事件泵不再依赖任何视口状态）。
        fn poll(&mut self, _ctx: &egui::Context) {
            let lang = self.lang;
            // 用户再次启动应用（桌面图标 / 命令行 / 单实例转发）是**显式**的
            // 打开窗口意图；Core 的 HostAction::ShowMain 不能拿来当这个用
            // （弹窗流程里也会发，会让弹一次面板冒出一个主窗口）。
            let mut launch_intent_window_requested = false;
            if let Some(native) = &self.native {
                let (launch_intents, hotkey_events, errors) = native.drain_native_events();
                let host = native.host_arc();
                // 原生动作先收下来：`native` 的借用到此为止，后面的 `&mut self`
                // 调用（本地热键处理）才不会和它冲突。
                let mut actions = Vec::new();
                native.host_actions().drain(|action| actions.push(action));
                for intent in launch_intents {
                    // CLI 的 ToggleQa 与「按一次 QA 热键」等价（Tauri
                    // `dispatch_cli_intent` 把 ToggleQa 直接转给 `handle_qa_hotkey_pressed`），
                    // 所以它既不拉起主窗口，也不走 Core 的 ToggleQa（那条是“切换录音”）。
                    if matches!(
                        intent,
                        LinuxLaunchIntent::Cli(openless_core::CliIntent::ToggleQa)
                    ) {
                        log::info!("[ui-host] CLI intent toggles the QA panel");
                        self.toggle_qa_panel();
                        continue;
                    }
                    log::info!("[ui-host] launch intent from the user: {intent:?}");
                    launch_intent_window_requested = true;
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_launch_intent(intent).await?;
                        Ok(tr_l10n(lang, "status.launch_handled").to_string())
                    });
                }
                // 本地边沿先于插件信号处理：它们来自我们自己的窗口（有焦点时
                // 插件一个信号都不会发），两者共用同一张去重表。
                let local_edges = std::mem::take(&mut self.pending_local_hotkeys);
                for event in self.apply_local_hotkey_edges(local_edges) {
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_hotkey_event(event).await?;
                        Ok(tr_l10n(lang, "status.hotkey_handled").to_string())
                    });
                }
                for event in hotkey_events {
                    if !self.accept_plugin_hotkey(&event) {
                        log::debug!("[hotkey] plugin event ignored: a local edge just handled it");
                        continue;
                    }
                    if self.intercept_hotkey(&event) {
                        continue;
                    }
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_hotkey_event(event).await?;
                        Ok(tr_l10n(lang, "status.hotkey_handled").to_string())
                    });
                }
                if let Some(error) = errors.last() {
                    self.status = error.to_string();
                }

                // HostAction controls only native visibility/focus/effects.
                // QA and Selection contents and terminal ownership always come
                // back through sequenced Core events handled above.
                for action in actions {
                    match action {
                        HostAction::ShowMain => {
                            // Core 的 ShowMain 是「把主窗口推到前面」的提示（弹窗流程里也会发），
                            // 不是用户动作：宿主不因此拉起窗口进程，否则弹一次面板就可能
                            // 冒出一个主窗口。真正的用户动作是托盘「显示主窗口」。
                        }
                        HostAction::ShowLessComputer => {
                            // Core 在每次 Less Computer 轮次开始前发这个动作（Tauri 里
                            // 它显示 `less-computer` 窗口）。宿主是序列所有者，这里
                            // 拉起/刷新面板即可。
                            self.show_less_computer_popup();
                        }
                        HostAction::FocusMain => {
                            // 宿主没有窗口可聚焦；已有窗口的聚焦由 UI 进程自己处理。
                            // 这里刻意什么都不做：把它当成「用户想打开主窗口」会让
                            // 弹窗一出现就冒出主窗口。
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
                            self.status = tr_l10n(lang, "status.request_restart").to_string();
                        }
                        HostAction::ShowSelectionPreview => {
                            // 核心照旧发这个动作；现在它只负责把**选区助手面板**
                            // 拉到「润色结果」模式（独立预览窗口已下线）。
                            self.polish_result_visible = true;
                            self.show_selection_popup();
                        }
                        HostAction::HideSelectionPreview => {
                            // 核心在润色流程收尾（直接覆盖 / 会话结束）时照旧发这个
                            // 动作。面板此刻若在提问对话中，不能被它关掉——只撤掉
                            // 润色结果模式。
                            if self.polish_result_visible {
                                self.polish_result_visible = false;
                                let session_id = self
                                    .selection
                                    .as_ref()
                                    .and_then(|selection| selection.session_id)
                                    .map(|id| id.to_string())
                                    .unwrap_or_else(|| "selection".to_string());
                                self.hide_popup(
                                    PopupKind::Qa,
                                    session_id,
                                    self.last_event_sequence.saturating_mul(2).saturating_add(1),
                                );
                            }
                        }
                        HostAction::ShowQa => {
                            log::info!("[hotkey] QA panel show requested by the host action");
                            self.qa_visible = true;
                            self.show_qa_popup();
                        }
                        HostAction::HideQa => {
                            if !qa_hides_on_host_action(self.qa_pinned) {
                                continue;
                            }
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
            if launch_intent_window_requested {
                self.request_main_window();
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
                        self.polish_result_visible = false;
                    }
                    self.snapshot = Some(snapshot);
                    for event in replay.events {
                        self.apply_event(event);
                    }
                    self.status = if replay.truncated {
                        fmt_l10n(lang, "status.backlog_reset", &[&dropped])
                    } else {
                        fmt_l10n(lang, "status.backlog_replay", &[&dropped])
                    };
                }
            }
            while let Ok(result) = self.rx.try_recv() {
                match result {
                    UiResult::Message(message) => self.status = message,
                    UiResult::CapsuleDismissDue { session_id } => {
                        let current = self
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.dictation.session_id)
                            .map(|id| id.to_string());
                        let phase = self
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.dictation.phase)
                            .unwrap_or(DictationPhase::Idle);
                        if capsule_hide_is_still_current(current.as_deref(), &session_id, phase) {
                            self.dismiss_capsule();
                        } else {
                            log::info!(
                                "capsule: dismissal skipped for session {session_id} — \
                                 a newer session is active (snapshot {current:?}, {phase:?})"
                            );
                        }
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
                            // A refresh (enable toggle, save, validation) must not
                            // throw away the editor draft the user is editing: only
                            // a different channel re-reads the descriptor.
                            let already_loaded = matches!(
                                &self.provider_editor,
                                ProviderEditorState::Loaded(editor)
                                    if editor.channel.id == channel_id
                            );
                            if !already_loaded {
                                if let Some((channel, descriptor)) =
                                    provider_channel_descriptor(&panel, &channel_id)
                                {
                                    self.provider_editor = ProviderEditorState::Loading {
                                        kind: panel.kind,
                                        channel_id,
                                    };
                                    self.provider_editor_form = None;
                                    self.load_provider_editor(panel.kind, channel, descriptor);
                                }
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
                                self.provider_editor_form =
                                    Some(ProviderEditorForm::from_editor(&editor, self.lang));
                                self.provider_editor =
                                    ProviderEditorState::Loaded(Box::new(editor));
                            }
                            Err(error) => {
                                self.provider_editor = ProviderEditorState::Failed(error.clone());
                                self.provider_editor_form = None;
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
                                    self.status = fmt_l10n(
                                        lang,
                                        "status.provider_models_loaded",
                                        &[&models.len()],
                                    );
                                    if let Some(form) = self.provider_editor_form.as_mut() {
                                        form.models = models.clone();
                                        form.models_loading = false;
                                    }
                                    self.provider_models = models;
                                }
                                Err(error) => {
                                    if let Some(form) = self.provider_editor_form.as_mut() {
                                        form.models.clear();
                                        form.models_loading = false;
                                    }
                                    self.status = error;
                                }
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
                            // Core 可能夹取过值（例如条数下限 5），保存后重新灌一次文本行。
                            self.hydrate_text_fields = true;
                            if let Some(native) = &self.native {
                                self.snapshot = Some(native.host().snapshot());
                            }
                            self.settings_dirty = SettingsDirty::default();
                            self.status = tr_l10n(lang, "status.settings_saved").to_string();
                            // Appearance (e.g. the Overview heatmap toggle) and any
                            // provider/credential edits may change Overview state.
                            self.load_overview();
                            if let Some(backend) = self.backend() {
                                let config = openless_core::RemoteInputConfig {
                                    enabled: outcome.preferences.remote_input_enabled,
                                    port: outcome.preferences.remote_input_port,
                                };
                                self.spawn(async move {
                                    backend.services().remote_input.configure(config).await?;
                                    Ok(tr_l10n(lang, "status.remote_updated").to_string())
                                });
                            }
                        }
                        Err(error) => self.status = error,
                    },
                    UiResult::Marketplace(seq, result) => {
                        // Latest request wins: a slow earlier response must not
                        // replace the results of the query the user sees now.
                        if seq == self.marketplace_seq {
                            match result {
                                Ok(items) => {
                                    self.status = fmt_l10n(
                                        lang,
                                        "status.marketplace_loaded",
                                        &[&items.len()],
                                    );
                                    self.marketplace_items = items;
                                }
                                Err(error) => self.status = error,
                            }
                            self.marketplace_attempted = true;
                        } else {
                            log::debug!("dropping stale marketplace response (seq {seq})");
                        }
                    }
                    UiResult::MarketplaceLikes(Ok(likes)) => self.marketplace_my_likes = likes,
                    UiResult::MarketplaceLikes(Err(error)) => {
                        // Not signed in / offline: keep the previous like set.
                        log::debug!("marketplace likes unavailable: {error}");
                    }
                    UiResult::SettingsChannels(Ok(rows)) => {
                        self.settings_channels = rows;
                        self.settings_channels_loading = false;
                    }
                    UiResult::ServiceConfigured(configured) => {
                        self.service_configured = configured;
                    }
                    UiResult::SettingsChannels(Err(error)) => {
                        self.settings_channels_loading = false;
                        self.frontend_vm.settings_notice = Some(error);
                    }
                    UiResult::MarketplaceFlow(Ok(flow)) => {
                        self.status = fmt_l10n(lang, "status.device_code", &[&flow.user_code]);
                        self.marketplace_flow = Some(flow);
                    }
                    UiResult::MarketplaceFlow(Err(error)) => self.status = error,
                    UiResult::MarketplaceAuthPoll(Ok(result)) => match result {
                        openless_core::OAuthPollResult::Authorized { login } => {
                            self.marketplace_flow = None;
                            self.status = fmt_l10n(lang, "status.logged_in", &[&login]);
                        }
                        openless_core::OAuthPollResult::Pending => {
                            self.status = tr_l10n(lang, "status.oauth_pending").to_string();
                        }
                        openless_core::OAuthPollResult::SlowDown => {
                            self.status = tr_l10n(lang, "status.oauth_slowdown").to_string();
                        }
                        openless_core::OAuthPollResult::Error { message } => {
                            self.status = message;
                        }
                    },
                    UiResult::MarketplaceAuthPoll(Err(error)) => self.status = error,
                    UiResult::MarketplaceDetail(Ok(detail)) => {
                        self.status =
                            fmt_l10n(lang, "status.detail_loaded", &[&detail.summary.name]);
                        self.marketplace_detail = Some(detail);
                    }
                    UiResult::MarketplaceDetail(Err(error)) => self.status = error,
                    UiResult::MarketplaceMine(Ok((packs, likes))) => {
                        self.status = fmt_l10n(
                            lang,
                            "status.my_publish_likes",
                            &[&packs.len(), &likes.len()],
                        );
                        self.marketplace_my_packs = packs;
                        self.marketplace_my_likes = likes;
                    }
                    UiResult::MarketplaceMine(Err(error)) => self.status = error,
                    UiResult::Microphones(Ok(devices)) => {
                        self.microphone_error = None;
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
                    UiResult::Microphones(Err(error)) => {
                        self.microphone_error = Some(error.clone());
                        self.status = error;
                    }
                    UiResult::Overview(Ok(data)) => self.overview = OverviewState::Loaded(data),
                    UiResult::Overview(Err(error)) => {
                        self.status = error.clone();
                        self.overview = OverviewState::Failed(error);
                    }
                    UiResult::UpdateCheck(Ok(Some(manifest))) => {
                        self.update_busy = false;
                        self.status = fmt_l10n(lang, "update.discovered", &[&manifest.version]);
                        self.update_manifest = Some(manifest);
                    }
                    UiResult::UpdateCheck(Ok(None)) => {
                        self.update_busy = false;
                        self.status = tr_l10n(lang, "update.up_to_date").to_string();
                    }
                    UiResult::UpdateCheck(Err(error)) => {
                        self.update_busy = false;
                        self.status = fmt_l10n(lang, "update.check_failed", &[&error]);
                    }
                    UiResult::UpdateProgress(progress) => self.update_progress = Some(progress),
                    UiResult::UpdateInstalled(Ok(installed)) => {
                        self.update_busy = false;
                        self.update_manifest = None;
                        self.status =
                            fmt_l10n(lang, "update.installed_restart", &[&installed.version]);
                    }
                    UiResult::UpdateInstalled(Err(error)) => {
                        self.update_busy = false;
                        self.status = fmt_l10n(lang, "update.install_failed", &[&error]);
                    }
                }
            }
            if let Some(backend) = self.backend() {
                self.snapshot = Some(backend.snapshot());
            }
            self.reconcile_capsule_liveness();
        }

        /// 兜底：Core 有错误路径只 reset 会话、不发布终态事件，宿主就永远等不到
        /// 「终态 → 收起」，药丸会一直贴在屏上。这里每帧按快照判断会话是否已经
        /// 消失，消失且没排过收起就补一次（时长按失败终态，文案仍由胶囊自己决定）。
        fn reconcile_capsule_liveness(&mut self) {
            let live = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.dictation.session_id)
                .map(|session_id| session_id.to_string());
            let Some(session) = capsule_needs_fallback_dismissal(
                self.capsule_session.as_deref(),
                live.as_deref(),
                self.capsule_dismissal_scheduled.as_deref(),
            ) else {
                return;
            };
            log::info!(
                "capsule: session {session} vanished without a terminal event — \
                 scheduling the fallback dismissal"
            );
            self.schedule_capsule_dismissal(&session, DictationPhase::Failed);
        }

        /// Apply a newly chosen UI locale immediately: persist it as Linux-UI
        /// state (never Core business truth), resolve it to a concrete language
        /// and let the next frame re-render every localized surface. Persistence
        /// is offloaded off the egui frame so the write can never stall a repaint.
        fn apply_locale_pref(&mut self, pref: LocalePref) {
            if pref == self.locale_pref {
                return;
            }
            self.locale_pref = pref;
            self.lang = pref.resolve();
            if let Some(tray) = &self.tray {
                let _ = tray.set_lang(self.lang);
            }
            let runtime = self.tokio.clone();
            runtime.spawn_blocking(move || {
                let _ = save_locale_pref(pref);
            });
        }

        /// The language selector row shown in Settings. Changing it re-renders
        /// the whole window immediately (shell, headings, labels, popups later
        /// pick it up from the persisted UI state on their next launch).
        // ── Frontend bridge ─────────────────────────────────────────────────

        /// Sync backend state into the frontend view model each frame before
        /// rendering. Only fields that have real data sources are populated;
        /// unwired fields remain in their default empty / loading state.
        fn sync_view_model(&mut self) {
            // Capture overview error before taking a mutable borrow on frontend_vm.
            let overview_err = self.overview_error();
            let backend = self.backend();
            let lang = self.lang;
            let permissions = self.permission_snapshot();
            // 文本型设置行只在偏好载入 / 外部变更时回灌一次，避免把输入中的
            // 内容每帧弹回旧值。
            let hydrate_text = std::mem::replace(&mut self.hydrate_text_fields, false);

            let vm = &mut self.frontend_vm;

            // Map shell::Page to frontend::Page.
            vm.active_page = match self.active_page {
                shell::Page::Overview => frontend::view_model::Page::Overview,
                shell::Page::History => frontend::view_model::Page::History,
                shell::Page::Vocabulary => frontend::view_model::Page::Vocab,
                shell::Page::Styles => frontend::view_model::Page::Style,
                shell::Page::Marketplace => frontend::view_model::Page::Marketplace,
                shell::Page::Providers => frontend::view_model::Page::Settings,
                shell::Page::Assistant => frontend::view_model::Page::SelectionAsk,
                shell::Page::Translation => frontend::view_model::Page::Translation,
                shell::Page::Corrections => frontend::view_model::Page::Corrections,
            };

            vm.status = self.status.clone();
            vm.version = env!("OPENLESS_APP_VERSION").to_string();
            vm.lang = lang;
            // UI 进程没有 Core 偏好，明暗主题必须随视图模型一起过去。
            vm.theme_mode = self
                .preferences
                .as_ref()
                .map(|preferences| preferences.theme_mode)
                .unwrap_or_default();
            if let Some(prefs) = &self.preferences {
                vm.dictation_hotkey = prefs.dictation_hotkey.display_label();
                vm.qa_hotkey = prefs
                    .qa_hotkey
                    .as_ref()
                    .map(|binding| binding.display_label())
                    .unwrap_or_default();
                vm.translation_hotkey = prefs.translation_hotkey.display_label();
            }

            // Overview: wire real data when available.
            if let Some(summary) = self.overview.summary(chrono::Local::now().date_naive()) {
                vm.overview_loading = false;
                vm.overview_error = None;
                vm.overview = Some(frontend::view_model::OverviewSummary {
                    asr_provider: summary.asr_provider,
                    llm_provider: summary.llm_provider,
                    asr_configured: summary.asr_configured,
                    llm_configured: summary.llm_configured,
                    chars_today: summary.chars_today,
                    segments_today: summary.segments_today,
                    duration_ms_today: summary.duration_ms_today,
                    avg_latency_ms: summary.avg_latency_ms,
                    history_total: summary.history_total,
                    recent: summary
                        .recent
                        .into_iter()
                        .map(|entry| frontend::view_model::OverviewRecentEntry {
                            created_at: entry.created_at,
                            final_text: entry.final_text,
                            raw_transcript: entry.raw_transcript,
                            mode: overview_mode(entry.mode),
                            duration_ms: entry.duration_ms,
                        })
                        .collect(),
                    activity_daily: summary
                        .activity_daily
                        .into_iter()
                        .map(overview_activity_day)
                        .collect(),
                    heatmap_year: summary.heatmap_year,
                    heatmap: summary
                        .heatmap
                        .into_iter()
                        .map(overview_heatmap_day)
                        .collect(),
                });
            } else if let Some(error) = overview_err {
                vm.overview_loading = false;
                vm.overview_error = Some(error);
                vm.overview = None;
            } else {
                vm.overview_loading = true;
                vm.overview_error = None;
                vm.overview = None;
            }

            // Settings: populate from preferences.
            if let Some(prefs) = &self.preferences {
                let s = &mut vm.settings;
                s.streaming_insert = prefs.streaming_insert;
                s.stable_transcription = prefs.stable_transcription_enabled;
                s.start_minimized = prefs.start_minimized;
                s.auto_update = prefs.auto_update_check;
                s.remote_input = prefs.remote_input_enabled;
                if hydrate_text {
                    s.remote_port = prefs.remote_input_port.to_string();
                }
                s.activity_heatmap = prefs.show_overview_activity_heatmap;
                s.theme = match prefs.theme_mode {
                    openless_core::shared_types::ThemeMode::System => 0,
                    openless_core::shared_types::ThemeMode::Light => 1,
                    openless_core::shared_types::ThemeMode::Dark => 2,
                };
                vm.translation_working_languages = prefs.working_languages.clone();
                vm.translation_target_language = prefs.translation_target_language.clone();
                // Tauri offers 切换式 / 按住说话 / 自动识别 — the legacy DoubleClick
                // value stays untouched in the store, it just has no chip here.
                s.recording_mode = match prefs.hotkey.mode {
                    openless_core::shared_types::HotkeyMode::Hold => 1,
                    openless_core::shared_types::HotkeyMode::Auto => 2,
                    _ => 0,
                };
                s.restore_clipboard = prefs.restore_clipboard_after_paste;
                // Tauri 只提供 Ctrl+V / Ctrl+Shift+V 两项。
                s.paste_shortcut = match prefs.paste_shortcut {
                    openless_core::shared_types::PasteShortcut::CtrlShiftV => 1,
                    _ => 0,
                };
                s.silence_auto_stop = prefs.silence_auto_stop_enabled;
                s.silence_seconds =
                    prefs.silence_auto_stop_seconds.round().clamp(1.0, 5.0) as usize;
                s.microphone_name = prefs.microphone_device_name.clone();
                s.microphone_options = self
                    .microphones
                    .iter()
                    .map(|device| device.name.clone())
                    .collect();
                s.microphone_error = self.microphone_error.clone();
                // 胶囊开关与样式（Tauri `showCapsule` / `capsuleStyle`）。
                s.show_capsule = prefs.show_capsule;
                s.capsule_style = match prefs.capsule_style {
                    openless_core::shared_types::CapsuleStyle::Classic => 1,
                    openless_core::shared_types::CapsuleStyle::Typeless => 2,
                    openless_core::shared_types::CapsuleStyle::Siri => 0,
                };
                s.mute_while_recording = prefs.mute_during_recording;
                s.audio_cue = prefs.audio_cue_on_record;
                s.launch_at_login = prefs.launch_at_login;
                s.streaming_save_clipboard = prefs.streaming_insert_save_clipboard;
                s.record_audio_for_debug = prefs.record_audio_for_debug;
                if hydrate_text {
                    s.history_max_entries = prefs
                        .history_max_entries
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    s.retention_days = prefs.history_retention_days.to_string();
                    s.polish_context_window = prefs.polish_context_window_minutes.to_string();
                    s.audio_recording_max_entries = prefs
                        .audio_recording_max_entries
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                }
                s.remote_default_mode = usize::from(prefs.remote_input_default_mode == "hold");
                s.system_proxy = prefs.use_system_proxy;
                s.multimodal = prefs.multimodal_pipeline_enabled;
                s.less_computer = prefs.coding_agent_enabled;
                s.coding_agent_provider = match prefs.coding_agent_provider.as_str() {
                    "opencode-cli" => 1,
                    "codex-cli" => 2,
                    "dsh-cli" => 3,
                    _ => 0,
                };
                s.coding_agent_permission = match prefs.coding_agent_permission_mode.as_str() {
                    "plan" => 1,
                    "default" => 2,
                    "bypassPermissions" => 3,
                    _ => 0,
                };
                if hydrate_text {
                    s.coding_agent_model = prefs.coding_agent_model.clone().unwrap_or_default();
                    s.coding_agent_workdir = prefs.coding_agent_workdir.clone().unwrap_or_default();
                    s.coding_agent_exe = prefs.coding_agent_exe.clone().unwrap_or_default();
                }
                s.selection_polish_delivery = match prefs.selection_polish_output_mode {
                    openless_core::shared_types::SelectionPolishOutputMode::DirectReplace => 0,
                    openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm => 1,
                };
                s.beta_channel = matches!(
                    prefs.update_channel,
                    openless_core::shared_types::UpdateChannel::Beta
                );
                // 多模态 / 平台能力：决定 AI 服务页的视图与更新控件。
                // 远程输入的实时状态：配对码 / 访问网址 / 证书指纹。
                if let Some((status, pin)) = &self.remote_access {
                    vm.remote_running = status.running;
                    vm.remote_urls_stale = status.urls_stale;
                    vm.remote_pin = pin.clone();
                    vm.remote_urls = status.urls.clone();
                    vm.remote_cert_fingerprint = status.ca_fingerprint_sha256.clone();
                } else {
                    vm.remote_running = false;
                    vm.remote_urls_stale = false;
                    vm.remote_pin = String::new();
                    vm.remote_urls = Vec::new();
                    vm.remote_cert_fingerprint = None;
                }
                // 必配服务的状态点由 `load_service_configured` 异步刷新。
                vm.service_configured = self.service_configured;
                vm.multimodal_view = prefs.multimodal_pipeline_enabled;
                vm.pipeline_multimodal =
                    prefs.pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal;
                // Linux 宿主没有本地推理引擎。
                vm.supports_local_asr = false;
                // 热键后端是否真的起来了：没有 fcitx5 监听器时隐藏「快捷键」分区，
                // 与 Tauri 的 `visibleSettingsSections(supportsDesktopHotkey)` 一致。
                vm.hotkeys_supported = self
                    .native
                    .as_ref()
                    .is_some_and(LinuxNativeRuntime::hotkeys_available);
                vm.auto_update_capable = self.update_support.supports_auto_update();
                vm.permissions = permissions;
                vm.selection_polish_hotkey = prefs
                    .selection_polish_hotkey
                    .as_ref()
                    .map(|binding| binding.display_label())
                    .unwrap_or_default();
                // 风格包直选：只展示已经录过的快捷键（录制器尚未实现）。
                s.style_pack_hotkeys = prefs
                    .style_pack_hotkeys
                    .iter()
                    .map(|entry| {
                        let pack = self
                            .style_packs
                            .iter()
                            .find(|pack| pack.id == entry.pack_id);
                        frontend::view_model::StylePackHotkeyRow {
                            pack_id: entry.pack_id.clone(),
                            name: pack
                                .map(|pack| pack.name.clone())
                                .unwrap_or_else(|| entry.pack_id.clone()),
                            hotkey: entry.binding.display_label(),
                        }
                    })
                    .collect();
                // 草稿行的默认风格包：第一个还没绑定快捷键的（Tauri 的「＋添加」下拉）。
                if vm.style_hotkey_draft_pack >= vm.style_packs.len() {
                    vm.style_hotkey_draft_pack = vm.style_packs.len().saturating_sub(1);
                }
            }

            // AI services tab: provider picker plus the cached channel list.
            vm.channel_providers = openless_core::provider_rules::provider_descriptors(
                provider_kind(self.settings_channel_kind),
            )
            .into_iter()
            .map(|descriptor| frontend::view_model::SettingsChannelProvider {
                provider_type: descriptor.provider_type.as_str().to_string(),
                label: localized_provider_label(
                    lang,
                    self.settings_channel_kind,
                    descriptor.provider_type.as_str(),
                ),
            })
            .collect();
            vm.channels_loading = self.settings_channels_loading;
            let active_channel = self
                .settings_channels
                .iter()
                .position(|channel| channel.enabled)
                .unwrap_or(usize::MAX);
            vm.channels = self
                .settings_channels
                .iter()
                .enumerate()
                .map(|(index, channel)| frontend::view_model::SettingsChannel {
                    name: channel.name.clone(),
                    provider: localized_provider_label(
                        lang,
                        self.settings_channel_kind,
                        &channel.provider_type,
                    ),
                    provider_type: channel.provider_type.clone(),
                    model: channel.model.clone(),
                    is_active: index == active_channel,
                    enabled: channel.enabled,
                    last_check: match (
                        channel.last_ok,
                        channel.last_latency_ms,
                        channel.last_error.as_deref(),
                    ) {
                        (Some(true), Some(ms), _) => Some(format!(
                            "{} · {}",
                            tr_l10n(lang, "settings.channels.passed"),
                            fmt_l10n(lang, "settings.channels.elapsed", &[&ms]),
                        )),
                        (Some(true), None, _) => {
                            Some(tr_l10n(lang, "settings.channels.passed").to_string())
                        }
                        (Some(false), _, error) => Some(fmt_l10n(
                            lang,
                            "settings.channels.failed",
                            &[&error.unwrap_or_default()],
                        )),
                        _ => None,
                    },
                })
                .collect();

            // Provider editor: mirrored from the host draft each frame. The page
            // stays a pure renderer; Core still owns the credential schema.
            vm.provider_editor = self.provider_editor_form.as_ref().map(|form| {
                frontend::view_model::SettingsProviderEditor {
                    channel_id: form.channel_id.clone(),
                    provider: form.label.clone(),
                    provider_type: form.provider_type.clone(),
                    name: form.name.clone(),
                    endpoint: form.endpoint.clone(),
                    model: form.model.clone(),
                    resource_id: form.resource_id.clone(),
                    auth_mode: form.auth_mode.clone(),
                    auth: form.auth,
                    primary_secret: form.primary_secret.clone(),
                    secondary_secret: form.secondary_secret.clone(),
                    models: form.models.clone(),
                    models_loading: form.models_loading,
                    busy: matches!(self.provider_editor, ProviderEditorState::Loading { .. }),
                }
            });

            // History: wire from Core when backend is available.
            if let Some(backend) = backend {
                match backend.list_history() {
                    Ok(history) => {
                        // The wav on disk is the real source of truth: older records
                        // carry no `has_audio_recording` flag, so the detail panel's
                        // play/export/retranscribe actions would disappear.
                        let recordings_dir = backend.config().data_dir.clone();
                        // Core stores history newest-first; keep that order.
                        vm.history_entries = history
                            .into_iter()
                            .map(|item| {
                                let has_audio = item.has_audio_recording.unwrap_or(false)
                                    || openless_linux_egui::recording_path(
                                        &recordings_dir,
                                        &item.id,
                                    )
                                    .map(|path| path.exists())
                                    .unwrap_or(false);
                                frontend::view_model::HistoryEntry {
                                    id: item.id,
                                created_at: item.created_at,
                                mode: overview_mode(item.mode),
                                // A record's style pack name is not resolvable here without the
                                // pack catalog, so the pill falls back to the polish mode label
                                // (which is what records without a style pack show anyway).
                                style_label: polish_mode_label(lang, item.mode).to_string(),
                                raw_transcript: item.raw_transcript,
                                final_text: item.final_text,
                                duration_ms: item.duration_ms,
                                insert_status: match item.insert_status {
                                    HistoryInsertStatus::Inserted => {
                                        frontend::view_model::HistoryInsertStatus::Inserted
                                    }
                                    HistoryInsertStatus::CopiedFallback => {
                                        frontend::view_model::HistoryInsertStatus::CopiedFallback
                                    }
                                    HistoryInsertStatus::PasteSent => {
                                        frontend::view_model::HistoryInsertStatus::PasteSent
                                    }
                                    HistoryInsertStatus::Failed => {
                                        frontend::view_model::HistoryInsertStatus::Failed
                                    }
                                    HistoryInsertStatus::NotRequested => {
                                        frontend::view_model::HistoryInsertStatus::NotRequested
                                    }
                                },
                                has_audio,
                                    asr_provider: item.asr_provider,
                                asr_model: item.asr_model,
                                asr_ms: item.asr_ms,
                                llm_provider: item.llm_provider,
                                llm_model: item.llm_model,
                                polish_ms: item.polish_ms,
                                app_name: item.app_name,
                                dictionary_count: item.dictionary_entry_count,
                                }
                            })
                            .collect();
                        vm.history_loading = false;
                        vm.history_error = None;
                    }
                    Err(error) => {
                        vm.history_loading = false;
                        vm.history_error = Some(error.to_string());
                    }
                }
            }

            // In-app playback progress (dropped once the clip finishes).
            if self
                .history_clip
                .as_ref()
                .is_some_and(|(_, player)| player.is_finished())
            {
                self.history_clip = None;
            }
            vm.history_playback = self.history_clip.as_ref().map(|(id, player)| {
                frontend::view_model::HistoryPlayback {
                    id: id.clone(),
                    position_ms: player.position_ms(),
                    total_ms: player.total_ms(),
                }
            });

            // Vocabulary + correction rules: the library path is always wired, so
            // an empty store is an empty list — never an "unsupported" page.
            vm.vocab_unsupported = false;
            vm.vocab_entries = self
                .vocabulary
                .iter()
                .map(|entry| frontend::view_model::VocabEntry {
                    phrase: entry.phrase.clone(),
                    hits: entry.hits as usize,
                    enabled: entry.enabled,
                    learned: false,
                })
                .collect();
            vm.vocab_rules = self
                .correction_rules
                .iter()
                .map(|rule| frontend::view_model::CorrectionRule {
                    pattern: rule.pattern.clone(),
                    replacement: rule.replacement.clone(),
                    enabled: rule.enabled,
                    learned: false,
                })
                .collect();
            vm.vocab_saved_presets = self
                .vocab_presets
                .iter()
                .map(|preset| frontend::view_model::SavedVocabPreset {
                    name: preset.name.clone(),
                    phrases: preset.phrases.join("、"),
                })
                .collect();

            // Style packs: wired too; an empty list is a valid state.
            vm.style_unsupported = false;
            vm.style_packs = self
                .style_packs
                .iter()
                .map(|pack| frontend::view_model::StylePack {
                    id: pack.id.clone(),
                    name: pack.name.clone(),
                    description: pack.description.clone(),
                    // Localized mode label (Core's display_name is zh-only).
                    tags: vec![polish_mode_label(lang, pack.base_mode).to_string()],
                    is_builtin: pack.kind == openless_core::StylePackKind::Builtin,
                    enabled: pack.enabled,
                    is_active: pack.active,
                    selection_active: self
                        .preferences
                        .as_ref()
                        .is_some_and(|prefs| prefs.selection_polish_style_pack_id == pack.id),
                })
                .collect();

            // Translation and selection-ask are always wired through Core; the
            // pages only render state that is already loaded.
            vm.translation_unsupported = false;
            vm.selection_unsupported = false;

            // Marketplace: wired through Core; the list loads lazily on first visit.
            vm.marketplace_unsupported = false;
            vm.marketplace_loading = !self.marketplace_attempted;
            if !self.marketplace_items.is_empty() {
                vm.marketplace_loading = false;
                vm.marketplace_packs = self
                    .marketplace_items
                    .iter()
                    .map(|item| frontend::view_model::MarketplacePack {
                        name: item.name.clone(),
                        version: item.version.clone(),
                        description: item.description.clone(),
                        mode: item.base_mode.clone(),
                        author: item.author_login.clone(),
                        tags: item.tags.clone(),
                        likes: item.like_count as u32,
                        downloads: item.download_count as u32,
                        liked: self.marketplace_my_likes.contains(&item.id),
                    })
                    .collect();
            }

            // Startup error.
            if let Some(error) = &self.startup_error {
                vm.status = format!("启动失败: {error}");
            }
        }

        /// Returns the overview error string if the overview is in a failed state.
        fn overview_error(&self) -> Option<String> {
            match &self.overview {
                crate::linux_app::OverviewState::Failed(error) => Some(error.clone()),
                _ => None,
            }
        }

        /// Apply a settings toggle from the frontend to the live preferences.
        /// 隐私分区的真实状态：Linux 没有系统级授权弹窗，能列出的设备 / 已启动的
        /// 热键适配器就是「已授权」，macOS 才有的辅助功能 / 本地网络一律「不适用」。
        fn permission_snapshot(&self) -> frontend::view_model::SettingsPermissions {
            use frontend::view_model::PermissionState;
            frontend::view_model::SettingsPermissions {
                microphone: if self.microphones.is_empty() {
                    PermissionState::Unknown
                } else {
                    PermissionState::Granted
                },
                accessibility: PermissionState::Unsupported,
                network: PermissionState::Unsupported,
                hotkey: if self.native.is_some() {
                    PermissionState::Granted
                } else {
                    PermissionState::Unknown
                },
            }
        }

        fn apply_settings_toggle(&mut self, field: frontend::view_model::SettingsField) {
            let Some(preferences) = self.preferences.as_mut() else {
                return;
            };
            match field {
                frontend::view_model::SettingsField::StreamingInsert => {
                    preferences.streaming_insert = !preferences.streaming_insert;
                    self.settings_dirty.streaming_insert = true;
                }
                frontend::view_model::SettingsField::StableTranscription => {
                    preferences.stable_transcription_enabled =
                        !preferences.stable_transcription_enabled;
                    self.settings_dirty.stable_transcription = true;
                }
                frontend::view_model::SettingsField::StartMinimized => {
                    preferences.start_minimized = !preferences.start_minimized;
                    self.settings_dirty.start_minimized = true;
                }
                frontend::view_model::SettingsField::AutoUpdate => {
                    preferences.auto_update_check = !preferences.auto_update_check;
                    self.settings_dirty.auto_update_check = true;
                }
                frontend::view_model::SettingsField::RemoteInput => {
                    preferences.remote_input_enabled = !preferences.remote_input_enabled;
                    self.settings_dirty.remote_input_enabled = true;
                }
                frontend::view_model::SettingsField::ActivityHeatmap => {
                    preferences.show_overview_activity_heatmap =
                        !preferences.show_overview_activity_heatmap;
                    self.settings_dirty.appearance = true;
                }
                frontend::view_model::SettingsField::RestoreClipboard => {
                    preferences.restore_clipboard_after_paste =
                        !preferences.restore_clipboard_after_paste;
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsField::SystemProxy => {
                    preferences.use_system_proxy = !preferences.use_system_proxy;
                    self.settings_dirty.appearance = true;
                }
                frontend::view_model::SettingsField::Multimodal => {
                    preferences.multimodal_pipeline_enabled =
                        !preferences.multimodal_pipeline_enabled;
                    self.settings_dirty.appearance = true;
                }
                frontend::view_model::SettingsField::LessComputer => {
                    preferences.coding_agent_enabled = !preferences.coding_agent_enabled;
                    self.settings_dirty.appearance = true;
                }
                frontend::view_model::SettingsField::SilenceAutoStop => {
                    preferences.silence_auto_stop_enabled = !preferences.silence_auto_stop_enabled;
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsField::AudioCue => {
                    preferences.audio_cue_on_record = !preferences.audio_cue_on_record;
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsField::ShowCapsule => {
                    preferences.show_capsule = !preferences.show_capsule;
                    self.settings_dirty.recording = true;
                    // 关掉就要立刻收起，否则药丸会留在屏幕上直到下一次录音。
                    if !preferences.show_capsule {
                        self.dismiss_capsule();
                    }
                }
                frontend::view_model::SettingsField::MuteWhileRecording => {
                    preferences.mute_during_recording = !preferences.mute_during_recording;
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsField::RecordAudioForDebug => {
                    preferences.record_audio_for_debug = !preferences.record_audio_for_debug;
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsField::StreamingSaveClipboard => {
                    preferences.streaming_insert_save_clipboard =
                        !preferences.streaming_insert_save_clipboard;
                    self.settings_dirty.streaming_insert = true;
                }
                frontend::view_model::SettingsField::LaunchAtLogin => {
                    preferences.launch_at_login = !preferences.launch_at_login;
                    self.settings_dirty.launch_at_login = true;
                }
                frontend::view_model::SettingsField::BetaChannel => {
                    // The Beta toggle is the same knob as the update channel.
                    if let Some(preferences) = self.preferences.as_mut() {
                        preferences.update_channel = if preferences.update_channel
                            == openless_core::shared_types::UpdateChannel::Beta
                        {
                            openless_core::shared_types::UpdateChannel::Stable
                        } else {
                            openless_core::shared_types::UpdateChannel::Beta
                        };
                        self.settings_dirty.update_channel = true;
                    }
                    self.frontend_vm.settings.beta_channel = self
                        .preferences
                        .as_ref()
                        .map(|prefs| {
                            prefs.update_channel == openless_core::shared_types::UpdateChannel::Beta
                        })
                        .unwrap_or(false);
                }
            }
            self.save_settings_if_dirty();
        }

        /// Apply a settings combo change from the frontend.
        fn apply_settings_combo(
            &mut self,
            field: frontend::view_model::SettingsComboField,
            index: usize,
        ) {
            let Some(preferences) = self.preferences.as_mut() else {
                return;
            };
            match field {
                frontend::view_model::SettingsComboField::Theme => {
                    preferences.theme_mode = match index {
                        0 => openless_core::shared_types::ThemeMode::System,
                        1 => openless_core::shared_types::ThemeMode::Light,
                        2 => openless_core::shared_types::ThemeMode::Dark,
                        _ => return,
                    };
                    self.settings_dirty.appearance = true;
                    self.frontend_vm.settings.theme = index;
                }
                frontend::view_model::SettingsComboField::Language => {
                    let pref = match index {
                        0 => LocalePref::System,
                        1 => LocalePref::Lang(Lang::ZhCn),
                        2 => LocalePref::Lang(Lang::ZhTw),
                        3 => LocalePref::Lang(Lang::En),
                        4 => LocalePref::Lang(Lang::Ja),
                        5 => LocalePref::Lang(Lang::Ko),
                        _ => return,
                    };
                    self.apply_locale_pref(pref);
                    self.frontend_vm.settings.language = index;
                }
                frontend::view_model::SettingsComboField::RecordingMode => {
                    // Tauri 的三档：切换式 / 按住说话 / 自动识别。
                    preferences.hotkey.mode = match index {
                        1 => openless_core::shared_types::HotkeyMode::Hold,
                        2 => openless_core::shared_types::HotkeyMode::Auto,
                        _ => openless_core::shared_types::HotkeyMode::Toggle,
                    };
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsComboField::CodingAgentProvider => {
                    preferences.coding_agent_provider = match index {
                        1 => "opencode-cli",
                        2 => "codex-cli",
                        3 => "dsh-cli",
                        _ => "claude-code-cli",
                    }
                    .to_string();
                    self.settings_dirty.coding_agent_enabled = true;
                }
                frontend::view_model::SettingsComboField::CodingAgentPermission => {
                    preferences.coding_agent_permission_mode = match index {
                        1 => "plan",
                        2 => "default",
                        3 => "bypassPermissions",
                        _ => "acceptEdits",
                    }
                    .to_string();
                    self.settings_dirty.coding_agent_enabled = true;
                }
                frontend::view_model::SettingsComboField::SelectionPolishDelivery => {
                    preferences.selection_polish_output_mode = match index {
                        1 => openless_core::shared_types::SelectionPolishOutputMode::PreviewConfirm,
                        _ => openless_core::shared_types::SelectionPolishOutputMode::DirectReplace,
                    };
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsComboField::SilenceSeconds => {
                    preferences.silence_auto_stop_seconds = index as f32 + 1.0;
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsComboField::CapsuleStyle => {
                    preferences.capsule_style = match index {
                        1 => openless_core::shared_types::CapsuleStyle::Classic,
                        2 => openless_core::shared_types::CapsuleStyle::Typeless,
                        _ => openless_core::shared_types::CapsuleStyle::Siri,
                    };
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsComboField::Microphone => {
                    preferences.microphone_device_name = if index == 0 {
                        String::new()
                    } else {
                        self.frontend_vm
                            .settings
                            .microphone_options
                            .get(index - 1)
                            .cloned()
                            .unwrap_or_default()
                    };
                    self.settings_dirty.microphone = true;
                }
                frontend::view_model::SettingsComboField::PasteShortcut => {
                    preferences.paste_shortcut = match index {
                        1 => openless_core::shared_types::PasteShortcut::CtrlShiftV,
                        2 => openless_core::shared_types::PasteShortcut::ShiftInsert,
                        _ => openless_core::shared_types::PasteShortcut::CtrlV,
                    };
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsComboField::RemoteDefaultMode => {
                    preferences.remote_input_default_mode = if index == 1 {
                        "hold".to_string()
                    } else {
                        "toggle".to_string()
                    };
                    self.settings_dirty.remote_input_enabled = true;
                }
            }
            self.save_settings_if_dirty();
        }

        /// 快捷键录入完成：写入对应偏好，并以 strict 模式保存以便立即应用热键副作用。
        fn apply_shortcut_captured(
            &mut self,
            field: frontend::view_model::ShortcutField,
            primary: String,
            modifiers: Vec<String>,
        ) {
            let binding = openless_core::shared_types::ShortcutBinding { primary, modifiers };
            if let Err(error) = openless_core::validate_shortcut_binding(&binding) {
                self.frontend_vm.settings_notice = Some(fmt_l10n(
                    self.lang,
                    "settings.recording.combo_conflict",
                    &[&error.to_string()],
                ));
                self.frontend_vm.shortcut_recording = None;
                return;
            }
            // 修饰键触发（按住说话）照常保存：插件只观察不吞修饰键，
            // 按住期间若又按了别的键就判定为组合键、放弃触发。
            let draft_pack_id = self
                .frontend_vm
                .style_packs
                .get(self.frontend_vm.style_hotkey_draft_pack)
                .map(|pack| pack.id.clone());
            let Some(preferences) = self.preferences.as_mut() else {
                return;
            };
            use frontend::view_model::ShortcutField;
            match field {
                ShortcutField::Dictation => preferences.dictation_hotkey = binding,
                ShortcutField::Translation => preferences.translation_hotkey = binding,
                ShortcutField::Qa => preferences.qa_hotkey = Some(binding),
                ShortcutField::SwitchStyle => preferences.switch_style_hotkey = Some(binding),
                ShortcutField::OpenApp => preferences.open_app_hotkey = Some(binding),
                ShortcutField::CodingAgentVoice => {
                    preferences.coding_agent_voice_hotkey = Some(binding);
                    // 「按住说话」有了触发键，Agent 也就该启用（Tauri 同样顺带打开）。
                    preferences.coding_agent_enabled = true;
                }
                ShortcutField::SelectionPolish => {
                    preferences.selection_polish_hotkey = Some(binding)
                }
                ShortcutField::StylePack(index) => {
                    if let Some(row) = preferences.style_pack_hotkeys.get_mut(index) {
                        row.binding = binding;
                    }
                }
                ShortcutField::StyleDraft => {
                    if let Some(pack_id) = draft_pack_id {
                        preferences
                            .style_pack_hotkeys
                            .retain(|entry| entry.pack_id != pack_id);
                        preferences.style_pack_hotkeys.push(
                            openless_core::shared_types::StylePackHotkey { pack_id, binding },
                        );
                        self.frontend_vm.style_hotkey_draft_open = false;
                    }
                }
            }
            self.settings_dirty.hotkeys = true;
            self.frontend_vm.shortcut_recording = None;
            self.frontend_vm.shortcut_menu = None;
            self.save_settings_if_dirty();
        }

        /// 停用某个快捷键绑定（核心录音快捷键没有停用，UI 里也不给按钮）。
        fn apply_shortcut_disable(&mut self, field: frontend::view_model::ShortcutField) {
            let Some(preferences) = self.preferences.as_mut() else {
                return;
            };
            use frontend::view_model::ShortcutField;
            match field {
                ShortcutField::Qa => preferences.qa_hotkey = None,
                ShortcutField::SwitchStyle => preferences.switch_style_hotkey = None,
                ShortcutField::OpenApp => preferences.open_app_hotkey = None,
                ShortcutField::CodingAgentVoice => preferences.coding_agent_voice_hotkey = None,
                ShortcutField::SelectionPolish => preferences.selection_polish_hotkey = None,
                ShortcutField::StylePack(index) => {
                    if let Some(row) = preferences.style_pack_hotkeys.get(index) {
                        let pack_id = row.pack_id.clone();
                        preferences
                            .style_pack_hotkeys
                            .retain(|entry| entry.pack_id != pack_id);
                    }
                }
                // 录音/翻译必须保留一个绑定；草稿行还没有内容。
                ShortcutField::Dictation
                | ShortcutField::Translation
                | ShortcutField::StyleDraft => {
                    return;
                }
            }
            self.settings_dirty.hotkeys = true;
            self.frontend_vm.shortcut_menu = None;
            self.save_settings_if_dirty();
        }

        fn apply_style_hotkey_remove(&mut self, index: usize) {
            let pack_id = self
                .frontend_vm
                .settings
                .style_pack_hotkeys
                .get(index)
                .map(|row| row.pack_id.clone());
            let Some(pack_id) = pack_id else {
                return;
            };
            if let Some(preferences) = self.preferences.as_mut() {
                preferences
                    .style_pack_hotkeys
                    .retain(|entry| entry.pack_id != pack_id);
            }
            self.settings_dirty.hotkeys = true;
            self.frontend_vm.shortcut_menu = None;
            self.save_settings_if_dirty();
        }

        /// 换绑到另一个风格包（目标包已有绑定时忽略，与 Tauri 的下拉置灰同义）。
        fn apply_style_hotkey_repack(&mut self, index: usize, pack_index: usize) {
            let pack_id = self
                .frontend_vm
                .settings
                .style_pack_hotkeys
                .get(index)
                .map(|row| row.pack_id.clone());
            let target = self
                .frontend_vm
                .style_packs
                .get(pack_index)
                .map(|pack| pack.id.clone());
            let (Some(current), Some(target)) = (pack_id, target) else {
                return;
            };
            if current == target {
                return;
            }
            if let Some(preferences) = self.preferences.as_mut() {
                if preferences
                    .style_pack_hotkeys
                    .iter()
                    .any(|entry| entry.pack_id == target)
                {
                    return;
                }
                if let Some(entry) = preferences
                    .style_pack_hotkeys
                    .iter_mut()
                    .find(|entry| entry.pack_id == current)
                {
                    entry.pack_id = target;
                }
            }
            self.settings_dirty.hotkeys = true;
            self.save_settings_if_dirty();
        }

        /// Apply a settings text field change from the frontend.
        fn apply_settings_text(
            &mut self,
            field: frontend::view_model::SettingsTextField,
            text: String,
        ) {
            let Some(preferences) = self.preferences.as_mut() else {
                return;
            };
            match field {
                frontend::view_model::SettingsTextField::RemotePort => {
                    if let Ok(port) = text.parse::<u16>() {
                        preferences.remote_input_port = port;
                        self.settings_dirty.remote_input_port = true;
                        self.frontend_vm.settings.remote_port = text;
                    }
                }
                frontend::view_model::SettingsTextField::RetentionDays => {
                    let parsed = text.trim().parse::<u32>().unwrap_or(0).min(365);
                    preferences.history_retention_days = parsed;
                    self.settings_dirty.recording = true;
                    self.frontend_vm.settings.retention_days = parsed.to_string();
                }
                frontend::view_model::SettingsTextField::PolishContextWindow => {
                    let parsed = text.trim().parse::<u32>().unwrap_or(0).min(60);
                    preferences.polish_context_window_minutes = parsed;
                    self.settings_dirty.recording = true;
                    self.frontend_vm.settings.polish_context_window = parsed.to_string();
                }
                frontend::view_model::SettingsTextField::AudioRecordingMaxEntries => {
                    preferences.audio_recording_max_entries = text
                        .trim()
                        .parse::<u32>()
                        .ok()
                        .map(|value| value.clamp(1, 200));
                    self.settings_dirty.recording = true;
                    self.frontend_vm.settings.audio_recording_max_entries = text;
                }
                frontend::view_model::SettingsTextField::CodingAgentModel => {
                    preferences.coding_agent_model = if text.trim().is_empty() {
                        None
                    } else {
                        Some(text.trim().to_string())
                    };
                    self.settings_dirty.coding_agent_enabled = true;
                    self.frontend_vm.settings.coding_agent_model = text;
                }
                frontend::view_model::SettingsTextField::CodingAgentWorkdir => {
                    preferences.coding_agent_workdir = if text.trim().is_empty() {
                        None
                    } else {
                        Some(text.trim().to_string())
                    };
                    self.settings_dirty.coding_agent_enabled = true;
                    self.frontend_vm.settings.coding_agent_workdir = text;
                }
                frontend::view_model::SettingsTextField::CodingAgentExe => {
                    preferences.coding_agent_exe = if text.trim().is_empty() {
                        None
                    } else {
                        Some(text.trim().to_string())
                    };
                    self.settings_dirty.coding_agent_enabled = true;
                    self.frontend_vm.settings.coding_agent_exe = text;
                }
                frontend::view_model::SettingsTextField::HistoryMaxEntries => {
                    preferences.history_max_entries = text
                        .trim()
                        .parse::<u32>()
                        .ok()
                        .map(|value| value.clamp(5, 200));
                    self.settings_dirty.recording = true;
                    self.frontend_vm.settings.history_max_entries = text;
                }
            }
            self.save_settings_if_dirty();
        }

        /// Apply a settings action button from the frontend.
        fn apply_settings_action(&mut self, field: frontend::view_model::SettingsActionField) {
            match field {
                frontend::view_model::SettingsActionField::ExportDiagnostics => {
                    if let Some(backend) = self.backend() {
                        let source = openless_linux_egui::log_path(&backend.config().data_dir);
                        let lang = self.lang;
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
                                    tr_l10n(lang, "dialog.export_log_cancelled"),
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
                            Ok(tr_l10n(lang, "status.export_log_done").to_string())
                        });
                    }
                }
                frontend::view_model::SettingsActionField::CheckBetaUpdate => {
                    self.request_update_check(openless_core::shared_types::UpdateChannel::Beta);
                }
                frontend::view_model::SettingsActionField::CopyCertFingerprint => {
                    let fingerprint = self
                        .remote_access
                        .as_ref()
                        .and_then(|(status, _)| status.ca_fingerprint_sha256.clone());
                    match fingerprint {
                        Some(fingerprint) => match fcitx5_copy_to_clipboard(&fingerprint) {
                            Ok(()) => {
                                self.frontend_vm.settings_notice =
                                    Some(tr_l10n(self.lang, "status.copied").to_string());
                            }
                            Err(error) => {
                                self.frontend_vm.settings_notice =
                                    Some(fmt_l10n(self.lang, "status.copy_failed", &[&error]));
                            }
                        },
                        None => {
                            self.frontend_vm.settings_notice = Some(
                                tr_l10n(
                                    self.lang,
                                    "settings.remote_input.cert_fingerprint_unavailable",
                                )
                                .to_string(),
                            );
                        }
                    }
                }
                frontend::view_model::SettingsActionField::CheckUpdate => {
                    let channel = self
                        .preferences
                        .as_ref()
                        .map(|prefs| prefs.update_channel)
                        .unwrap_or_default();
                    self.request_update_check(channel);
                }
                frontend::view_model::SettingsActionField::PreviewAudioCue => {
                    // 与真实录音开始时同一段合成提示音（Tauri `playRecordStartCue`）。
                    openless_linux_egui::play_cue_start();
                }
                frontend::view_model::SettingsActionField::OpenGitHub => {
                    let _ = open_external("https://github.com/earendil-works/openless");
                }
                frontend::view_model::SettingsActionField::OpenHelp => {
                    let _ = open_external("https://github.com/earendil-works/openless");
                }
                frontend::view_model::SettingsActionField::OpenReleaseNotes => {
                    let _ = open_external("https://github.com/earendil-works/openless/releases");
                }
                frontend::view_model::SettingsActionField::OpenFeedback => {
                    let _ = open_external("https://github.com/earendil-works/openless/issues");
                }
                frontend::view_model::SettingsActionField::CopyQQ => {
                    match fcitx5_copy_to_clipboard("1078960553") {
                        Ok(()) => {
                            self.frontend_vm.settings_notice =
                                Some(tr_l10n(self.lang, "status.copied").to_string());
                        }
                        Err(error) => {
                            self.frontend_vm.settings_notice = Some(fmt_l10n(
                                self.lang,
                                "status.copy_failed",
                                &[&error.to_string()],
                            ));
                        }
                    }
                }
            }
        }

        /// Persist dirty settings if any fields have been changed.
        fn save_settings_if_dirty(&mut self) {
            if !self.settings_dirty.any() {
                return;
            }
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
                            Err(error) if error.code == openless_core::BackendErrorCode::Busy => {
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

        /// Dispatch frontend actions to existing Core / backend methods.
        fn apply_frontend_actions(
            &mut self,
            actions: Vec<frontend::view_model::FrontendAction>,
            // 宿主没有窗口：窗口类动作由 UI 进程就地处理，这里保留参数是为了
            // 让调用点保持「渲染层 → 动作 → 宿主」的形状。
            _ctx: &egui::Context,
        ) {
            for action in actions {
                match action {
                    frontend::view_model::FrontendAction::Navigate(page) => {
                        self.active_page = match page {
                            frontend::view_model::Page::Overview => shell::Page::Overview,
                            frontend::view_model::Page::History => shell::Page::History,
                            frontend::view_model::Page::Vocab => shell::Page::Vocabulary,
                            frontend::view_model::Page::Style => shell::Page::Styles,
                            frontend::view_model::Page::Marketplace => shell::Page::Marketplace,
                            frontend::view_model::Page::SelectionAsk => shell::Page::Assistant,
                            frontend::view_model::Page::Translation => shell::Page::Translation,
                            frontend::view_model::Page::Corrections => shell::Page::Corrections,
                            frontend::view_model::Page::Settings => shell::Page::Providers,
                        };
                        if page == frontend::view_model::Page::Marketplace {
                            // The list is fetched lazily; entering the page is what
                            // triggers the first load.
                            self.load_marketplace();
                        }
                    }
                    frontend::view_model::FrontendAction::ToggleSettings => {
                        self.frontend_vm.settings_open = !self.frontend_vm.settings_open;
                        if self.frontend_vm.settings_open {
                            self.frontend_vm.active_page = frontend::view_model::Page::Settings;
                            // 每次打开设置都刷新「必配服务」状态点。
                            self.load_service_configured();
                            // 也重新枚举麦克风：设备可能在启动后才插上（Tauri 在
                            // 下拉打开时同样重查）。
                            self.load_microphones();
                            if self.settings_channels.is_empty() {
                                self.load_settings_channels();
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::CloseSettings => {
                        self.frontend_vm.settings_open = false;
                        self.frontend_vm.active_page = frontend::view_model::Page::Overview;
                    }
                    frontend::view_model::FrontendAction::SidebarToggleStyle => {
                        self.frontend_vm.style_open = !self.frontend_vm.style_open;
                    }
                    frontend::view_model::FrontendAction::OverviewRefresh => {
                        self.overview = OverviewState::Loading;
                        self.load_overview();
                    }
                    frontend::view_model::FrontendAction::OverviewPeriod(period) => {
                        self.frontend_vm.overview_period = period.min(1);
                    }
                    frontend::view_model::FrontendAction::OverviewMetric(metric) => {
                        self.frontend_vm.overview_metric = metric.min(2);
                    }
                    frontend::view_model::FrontendAction::SidebarToggleTools => {
                        self.frontend_vm.tools_open = !self.frontend_vm.tools_open;
                    }
                    frontend::view_model::FrontendAction::WindowClose
                    | frontend::view_model::FrontendAction::WindowMinimize
                    | frontend::view_model::FrontendAction::WindowMaximize => {
                        // 窗口控制由 UI 进程就地处理（只有它有窗口）；宿主收到说明
                        // 某个 UI 分支忘了拦，记一条即可，不影响任何状态。
                        log::debug!("[ui-host] window action reached the host: {action:?}");
                    }
                    frontend::view_model::FrontendAction::MarketplaceRefresh => {
                        self.load_marketplace();
                    }
                    frontend::view_model::FrontendAction::MarketplaceMyPacks => {
                        self.load_marketplace_mine();
                    }
                    frontend::view_model::FrontendAction::MarketplaceSearch(query) => {
                        self.marketplace_query = query.clone();
                        // Echo it back so the field never reverts while typing.
                        self.frontend_vm.marketplace_query = query;
                        // Debounced: `poll_marketplace_search` issues the request
                        // once typing pauses, exactly like the Tauri page.
                        self.schedule_marketplace_search();
                    }
                    frontend::view_model::FrontendAction::MarketplaceCloseDetail => {
                        self.frontend_vm.marketplace_selected = None;
                    }
                    frontend::view_model::FrontendAction::MarketplaceInstall(index) => {
                        if let Some(item) = self.marketplace_items.get(index) {
                            if let Some(backend) = self.backend() {
                                let id = item.id.clone();
                                let lang = self.lang;
                                self.spawn(async move {
                                    let pack = backend.services().marketplace.install(id).await?;
                                    Ok(fmt_l10n(
                                        lang,
                                        "status.marketplace_installed",
                                        &[&pack.name],
                                    ))
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceDownload(index) => {
                        if let Some(item) = self.marketplace_items.get(index) {
                            if let Some(backend) = self.backend() {
                                let id = item.id.clone();
                                let lang = self.lang;
                                let tx = self.tx.clone();
                                self.tokio.spawn(async move {
                                    let result = async {
                                        let bytes = backend
                                            .services()
                                            .marketplace
                                            .download_archive(id.clone())
                                            .await?;
                                        let destination = tokio::task::spawn_blocking(move || {
                                            rfd::FileDialog::new()
                                                .add_filter("OpenLess style pack", &["zip"])
                                                .set_file_name(format!(
                                                    "openless-marketplace-{id}.zip"
                                                ))
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
                                                tr_l10n(lang, "dialog.marketplace_zip_cancelled"),
                                            )
                                        })?;
                                        tokio::task::spawn_blocking(move || {
                                            openless_linux_egui::atomic_save(&destination, &bytes)
                                                .map_err(|error| {
                                                    BackendError::new(
                                                        openless_core::BackendErrorCode::Internal,
                                                        error.to_string(),
                                                    )
                                                })
                                        })
                                        .await
                                        .map_err(
                                            |error| {
                                                BackendError::new(
                                                    openless_core::BackendErrorCode::Internal,
                                                    error.to_string(),
                                                )
                                            },
                                        )??;
                                        Ok::<_, BackendError>(
                                            tr_l10n(lang, "status.marketplace_zip_saved")
                                                .to_string(),
                                        )
                                    }
                                    .await
                                    .unwrap_or_else(|error| error.to_string());
                                    let _ = tx.send(UiResult::Message(result));
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceToggleLike(index) => {
                        if let Some(item) = self.marketplace_items.get(index) {
                            if let Some(backend) = self.backend() {
                                let id = item.id.clone();
                                // Optimistic flip so the star reacts immediately.
                                let was_liked = self.marketplace_my_likes.contains(&id);
                                if was_liked {
                                    self.marketplace_my_likes.retain(|liked| liked != &id);
                                } else {
                                    self.marketplace_my_likes.push(id.clone());
                                }
                                if let Some(pack) =
                                    self.frontend_vm.marketplace_packs.get_mut(index)
                                {
                                    pack.liked = !was_liked;
                                    pack.likes = if was_liked {
                                        pack.likes.saturating_sub(1)
                                    } else {
                                        pack.likes.saturating_add(1)
                                    };
                                }
                                let lang = self.lang;
                                let restore_id = id.clone();
                                self.spawn(async move {
                                    let result =
                                        backend.services().marketplace.toggle_like(id).await?;
                                    Ok(fmt_l10n(
                                        lang,
                                        "status.marketplace_like",
                                        &[&result.like_count],
                                    ))
                                });
                                let _ = restore_id;
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceSort(sort) => {
                        self.frontend_vm.marketplace_sort = sort;
                        self.load_marketplace();
                    }
                    frontend::view_model::FrontendAction::HistoryRefresh => {
                        self.frontend_vm.history_loading = true;
                        self.frontend_vm.history_error = None;
                        self.frontend_vm.history_confirm = None;
                    }
                    frontend::view_model::FrontendAction::HistorySelect(index) => {
                        self.frontend_vm.history_selected = index;
                    }
                    frontend::view_model::FrontendAction::HistoryRequestClear => {
                        self.frontend_vm.history_confirm =
                            Some(frontend::view_model::HistoryConfirm::Clear);
                    }
                    frontend::view_model::FrontendAction::HistoryRequestDelete(index) => {
                        self.frontend_vm.history_confirm =
                            Some(frontend::view_model::HistoryConfirm::Delete(index));
                    }
                    frontend::view_model::FrontendAction::HistoryCancelConfirm => {
                        self.frontend_vm.history_confirm = None;
                    }
                    frontend::view_model::FrontendAction::HistoryConfirmAction => {
                        match self.frontend_vm.history_confirm.take() {
                            Some(frontend::view_model::HistoryConfirm::Clear) => {
                                if let Some(backend) = self.backend() {
                                    let lang = self.lang;
                                    self.spawn(async move {
                                        backend.clear_history()?;
                                        Ok(tr_l10n(lang, "status.history_cleared").to_string())
                                    });
                                }
                            }
                            Some(frontend::view_model::HistoryConfirm::Delete(index)) => {
                                if let Some(backend) = self.backend() {
                                    if let Some(entry) = self.frontend_vm.history_entries.get(index)
                                    {
                                        let id = entry.id.clone();
                                        let lang = self.lang;
                                        self.spawn(async move {
                                            backend.delete_history(&id)?;
                                            Ok(tr_l10n(lang, "status.history_deleted").to_string())
                                        });
                                    }
                                }
                            }
                            None => {}
                        }
                    }
                    frontend::view_model::FrontendAction::HistoryPlay(index) => {
                        let Some(entry) = self.frontend_vm.history_entries.get(index) else {
                            return;
                        };
                        let id = entry.id.clone();
                        // Same clip again -> stop; otherwise start the new one.
                        let same = self
                            .history_clip
                            .as_ref()
                            .is_some_and(|(playing, _)| playing == &id);
                        self.history_clip = None;
                        if same {
                            return;
                        }
                        if let Some(backend) = self.backend() {
                            let data_dir = backend.config().data_dir.clone();
                            match openless_linux_egui::read_recording_wav(&data_dir, &id)
                                .and_then(|wav| {
                                    openless_linux_egui::recording_pcm(&wav).map(|pcm| pcm.to_vec())
                                })
                                .map_err(|error| error.to_string())
                                .and_then(|pcm| openless_linux_egui::ClipPlayer::play(&pcm))
                            {
                                Ok(player) => self.history_clip = Some((id, player)),
                                Err(error) => self.status = error,
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::HistoryRetranscribe(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(entry) = self.frontend_vm.history_entries.get(index) {
                                let id = entry.id.clone();
                                let data_dir = backend.config().data_dir.clone();
                                let lang = self.lang;
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
                                    Ok(fmt_l10n(lang, "status.retranscribed", &[&entry.final_text]))
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::HistoryExport(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(entry) = self.frontend_vm.history_entries.get(index) {
                                let id = entry.id.clone();
                                let data_dir = backend.config().data_dir.clone();
                                let lang = self.lang;
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
                                            tr_l10n(lang, "dialog.recording_export_cancelled"),
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
                                    Ok(fmt_l10n(
                                        lang,
                                        "status.recording_exported",
                                        &[&saved.display()],
                                    ))
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::VocabFilter(index) => {
                        self.frontend_vm.vocab_filter = index.min(2);
                    }
                    frontend::view_model::FrontendAction::VocabSearch(query) => {
                        self.frontend_vm.vocab_query = query;
                    }
                    frontend::view_model::FrontendAction::VocabAddPhrase(phrase) => {
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.add_vocabulary(phrase, None)?;
                                Ok(tr_l10n(lang, "status.vocab_saved").to_string())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::VocabRemovePhrase(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(entry) = self.vocabulary.get(index) {
                                let id = entry.id.clone();
                                let lang = self.lang;
                                self.spawn(async move {
                                    backend.remove_vocabulary(&id)?;
                                    Ok(tr_l10n(lang, "status.vocab_updated").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::VocabTogglePhrase(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(entry) = self.vocabulary.get(index) {
                                let id = entry.id.clone();
                                let enabled = !entry.enabled;
                                let lang = self.lang;
                                self.spawn(async move {
                                    backend.set_vocabulary_enabled(&id, enabled)?;
                                    Ok(tr_l10n(lang, "status.vocab_updated").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::VocabAddRule {
                        pattern,
                        replacement,
                    } => {
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.add_correction_rule(pattern, replacement)?;
                                Ok(tr_l10n(lang, "status.correction_saved").to_string())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::VocabRemoveRule(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(rule) = self.correction_rules.get(index) {
                                let id = rule.id.clone();
                                let lang = self.lang;
                                self.spawn(async move {
                                    backend.remove_correction_rule(&id)?;
                                    Ok(tr_l10n(lang, "status.correction_updated").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::VocabToggleRule(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(rule) = self.correction_rules.get(index) {
                                let id = rule.id.clone();
                                let enabled = !rule.enabled;
                                let lang = self.lang;
                                self.spawn(async move {
                                    backend.set_correction_rule_enabled(&id, enabled)?;
                                    Ok(tr_l10n(lang, "status.correction_updated").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::VocabApplyPreset(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(preset) = self.vocab_presets.get(index) {
                                let phrases = preset.phrases.clone();
                                let name = preset.name.clone();
                                let lang = self.lang;
                                self.spawn(async move {
                                    for phrase in phrases {
                                        backend.add_vocabulary(
                                            phrase,
                                            Some(fmt_l10n(lang, "status.from_preset", &[&name])),
                                        )?;
                                    }
                                    Ok(tr_l10n(lang, "status.preset_updated").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::VocabCreatePreset { name, phrases } => {
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            self.spawn(async move {
                                let mut phrase_list: Vec<String> = phrases
                                    .split([',', '，', '\n'])
                                    .map(str::trim)
                                    .filter(|p| !p.is_empty())
                                    .map(ToOwned::to_owned)
                                    .collect();
                                phrase_list.sort();
                                phrase_list.dedup();
                                let mut store = backend.list_vocabulary_presets()?;
                                store.custom.push(openless_core::VocabPreset {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    name: name.trim().to_string(),
                                    phrases: phrase_list,
                                });
                                backend.save_vocabulary_presets(&store)?;
                                Ok(tr_l10n(lang, "status.preset_updated").to_string())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::StyleActivate(index) => {
                        let Some(pack) = self.style_packs.get(index) else {
                            return;
                        };
                        let id = pack.id.clone();
                        if self.frontend_vm.style_selection_workflow {
                            // Selection polish keeps its own active pack
                            // (`prefs.selection_polish_style_pack_id`).
                            if let Some(preferences) = self.preferences.as_mut() {
                                preferences.selection_polish_style_pack_id = id;
                                self.settings_dirty.appearance = true;
                            }
                            self.save_settings_if_dirty();
                        } else if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.activate_style_pack(&id)?;
                                Ok(tr_l10n(lang, "status.style_updated").to_string())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::StyleExport(index) => {
                        if let Some(backend) = self.backend() {
                            if let Some(pack) = self.style_packs.get(index) {
                                let id = pack.id.clone();
                                let lang = self.lang;
                                self.spawn(async move {
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
                                            tr_l10n(lang, "dialog.style_export_cancelled"),
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
                                    Ok(tr_l10n(lang, "status.style_updated").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::StyleEdit(index) => {
                        if let Some(pack) = self.style_packs.get(index).cloned() {
                            self.style_editor = Some(pack);
                            self.frontend_vm.style_editor_open = true;
                            self.frontend_vm.style_prompt = self
                                .style_editor
                                .as_ref()
                                .map(|e| e.prompt.clone())
                                .unwrap_or_default();
                        }
                    }
                    frontend::view_model::FrontendAction::StyleSaveEditor(prompt) => {
                        if let Some(mut pack) = self.style_editor.take() {
                            pack.prompt = prompt;
                            if let Some(backend) = self.backend() {
                                let exists = self.style_packs.iter().any(|p| p.id == pack.id);
                                let lang = self.lang;
                                self.spawn(async move {
                                    let saved = if exists {
                                        backend.update_style_pack(pack)?
                                    } else {
                                        backend.create_style_pack(pack)?
                                    };
                                    Ok(fmt_l10n(lang, "status.style_saved", &[&saved.name]))
                                });
                            }
                        }
                        self.frontend_vm.style_editor_open = false;
                    }
                    frontend::view_model::FrontendAction::StyleCloseEditor => {
                        self.style_editor = None;
                        self.frontend_vm.style_editor_open = false;
                    }
                    frontend::view_model::FrontendAction::StyleNewPack => {
                        self.style_editor = Some(openless_core::StylePack {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: tr_l10n(self.lang, "lbl.new_style_default").to_string(),
                            ..Default::default()
                        });
                        self.frontend_vm.style_editor_open = true;
                    }
                    frontend::view_model::FrontendAction::StyleImport => {
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
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
                                        tr_l10n(lang, "dialog.style_import_cancelled"),
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
                                Ok(fmt_l10n(lang, "status.style_imported", &[&pack.name]))
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::SelectionAskToggleHistory => {
                        self.frontend_vm.qa_save_history = !self.frontend_vm.qa_save_history;
                    }
                    frontend::view_model::FrontendAction::TranslationToggleLanguage(language) => {
                        if let Some(preferences) = self.preferences.as_mut() {
                            match preferences
                                .working_languages
                                .iter()
                                .position(|value| value == &language)
                            {
                                Some(index) => {
                                    preferences.working_languages.remove(index);
                                }
                                None => preferences.working_languages.push(language),
                            }
                            self.settings_dirty.appearance = true;
                        }
                        self.save_settings_if_dirty();
                    }
                    frontend::view_model::FrontendAction::TranslationSetTarget(language) => {
                        if let Some(preferences) = self.preferences.as_mut() {
                            preferences.translation_target_language = language;
                            self.settings_dirty.appearance = true;
                        }
                        self.save_settings_if_dirty();
                    }
                    frontend::view_model::FrontendAction::SettingsToggle(field) => {
                        self.apply_settings_toggle(field);
                    }
                    frontend::view_model::FrontendAction::SettingsCombo(field, index) => {
                        self.apply_settings_combo(field, index);
                    }
                    frontend::view_model::FrontendAction::SettingsText(field, text) => {
                        self.apply_settings_text(field, text);
                    }
                    frontend::view_model::FrontendAction::SettingsAction(field) => {
                        self.apply_settings_action(field);
                    }
                    frontend::view_model::FrontendAction::SettingsSection(section) => {
                        self.frontend_vm.settings_section = section;
                    }
                    frontend::view_model::FrontendAction::SettingsServicesView(view) => {
                        self.frontend_vm.services_view = view.min(3);
                        let kind = if view == 1 {
                            openless_core::ChannelKind::Asr
                        } else {
                            openless_core::ChannelKind::Llm
                        };
                        if self.settings_channel_kind != kind {
                            self.settings_channel_kind = kind;
                            // The editor belongs to one channel kind: switching the
                            // AI-services tab must not carry it across.
                            self.close_provider_editor();
                            self.selected_channel_id = None;
                            self.load_settings_channels();
                            self.load_service_configured();
                        } else if self.settings_channels.is_empty() {
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelFormOpen(open) => {
                        self.frontend_vm.channel_form_open = open;
                        if open {
                            self.frontend_vm.channel_form_name.clear();
                            self.frontend_vm.channel_provider_index = 0;
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelProvider(index) => {
                        self.frontend_vm.channel_provider_index = index;
                    }
                    frontend::view_model::FrontendAction::SettingsChannelName(name) => {
                        self.frontend_vm.channel_form_name = name;
                    }
                    frontend::view_model::FrontendAction::SettingsChannelCreate => {
                        let kind = self.settings_channel_kind;
                        let provider_type = self
                            .frontend_vm
                            .channel_providers
                            .get(self.frontend_vm.channel_provider_index)
                            .map(|provider| provider.provider_type.clone());
                        let name = self.frontend_vm.channel_form_name.trim().to_string();
                        if let (Some(backend), Some(provider_type)) =
                            (self.backend(), provider_type)
                        {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.create_channel(kind, provider_type, name).await?;
                                Ok(tr_l10n(lang, "status.channel_created").to_string())
                            });
                            self.frontend_vm.channel_form_open = false;
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                    }
                    frontend::view_model::FrontendAction::ShortcutMenu(field) => {
                        self.frontend_vm.shortcut_menu = field;
                        if field.is_some() {
                            // 打开菜单即退出录制（Tauri 点「录制快捷键」时同时收起菜单）。
                            self.frontend_vm.shortcut_recording = None;
                        }
                    }
                    frontend::view_model::FrontendAction::ShortcutRecording(field) => {
                        self.frontend_vm.shortcut_pending_modifier = None;
                        self.frontend_vm.shortcut_recording = field;
                        if field.is_some() {
                            self.frontend_vm.shortcut_menu = None;
                        }
                        self.frontend_vm.settings_notice = None;
                    }
                    frontend::view_model::FrontendAction::ShortcutCaptured(
                        field,
                        primary,
                        modifiers,
                    ) => {
                        self.apply_shortcut_captured(field, primary, modifiers);
                    }
                    frontend::view_model::FrontendAction::ShortcutDisable(field) => {
                        self.apply_shortcut_disable(field);
                    }
                    frontend::view_model::FrontendAction::StyleHotkeyDraft(open) => {
                        self.frontend_vm.style_hotkey_draft_open = open;
                        if open {
                            let used: Vec<String> = self
                                .frontend_vm
                                .settings
                                .style_pack_hotkeys
                                .iter()
                                .map(|row| row.pack_id.clone())
                                .collect();
                            self.frontend_vm.style_hotkey_draft_pack = self
                                .frontend_vm
                                .style_packs
                                .iter()
                                .position(|pack| !used.contains(&pack.id))
                                .unwrap_or(0);
                        } else {
                            self.frontend_vm.shortcut_recording = None;
                        }
                    }
                    frontend::view_model::FrontendAction::StyleHotkeyDraftPack(index) => {
                        self.frontend_vm.style_hotkey_draft_pack = index;
                    }
                    frontend::view_model::FrontendAction::StyleHotkeyRemove(index) => {
                        self.apply_style_hotkey_remove(index);
                    }
                    frontend::view_model::FrontendAction::StyleHotkeyRepack(index, pack_index) => {
                        self.apply_style_hotkey_repack(index, pack_index);
                    }
                    frontend::view_model::FrontendAction::SettingsChannelSelect(index) => {
                        self.open_provider_editor(index);
                    }
                    frontend::view_model::FrontendAction::SettingsChannelMove { index, delta } => {
                        let kind = self.settings_channel_kind;
                        let mut ids: Vec<String> = self
                            .settings_channels
                            .iter()
                            .map(|channel| channel.id.clone())
                            .collect();
                        let target = index as isize + delta;
                        if target >= 0 && (target as usize) < ids.len() {
                            ids.swap(index, target as usize);
                            if let Some(backend) = self.backend() {
                                let lang = self.lang;
                                self.spawn(async move {
                                    backend.reorder_channels(kind, ids).await?;
                                    Ok(tr_l10n(lang, "status.channel_reordered").to_string())
                                });
                                self.load_settings_channels();
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelProviderType {
                        index,
                        provider_type,
                    } => {
                        let kind = self.settings_channel_kind;
                        let id = self
                            .settings_channels
                            .get(index)
                            .map(|channel| channel.id.clone());
                        if let (Some(backend), Some(id)) = (self.backend(), id) {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend
                                    .set_channel_provider_type(kind, id, provider_type)
                                    .await?;
                                Ok(tr_l10n(lang, "status.provider_type_updated").to_string())
                            });
                            // The descriptor changed with the provider type: the
                            // editor must re-read it instead of keeping old fields.
                            self.close_provider_editor();
                            self.selected_channel_id = None;
                            self.load_settings_channels();
                            self.load_providers(kind);
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelActivate(index) => {
                        let kind = self.settings_channel_kind;
                        let id = self
                            .settings_channels
                            .get(index)
                            .map(|channel| channel.id.clone());
                        if let (Some(backend), Some(id)) = (self.backend(), id) {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.set_active_provider(provider_slot(kind), id).await?;
                                Ok(tr_l10n(lang, "status.channel_active").to_string())
                            });
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderField(field, value) => {
                        if let Some(form) = self.provider_editor_form.as_mut() {
                            match field {
                                frontend::view_model::SettingsProviderField::Name => {
                                    form.name = value
                                }
                                frontend::view_model::SettingsProviderField::Endpoint => {
                                    form.endpoint = value
                                }
                                frontend::view_model::SettingsProviderField::Model => {
                                    form.model = value
                                }
                                frontend::view_model::SettingsProviderField::ResourceId => {
                                    form.resource_id = value
                                }
                                frontend::view_model::SettingsProviderField::AuthMode => {
                                    form.auth_mode = value
                                }
                                frontend::view_model::SettingsProviderField::PrimarySecret => {
                                    form.primary_secret = value
                                }
                                frontend::view_model::SettingsProviderField::SecondarySecret => {
                                    form.secondary_secret = value
                                }
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderSave => {
                        if let Some(editor) = self.editor_from_form() {
                            if let Some(backend) = self.backend() {
                                let lang = self.lang;
                                self.spawn(async move {
                                    save_provider_editor(backend, editor).await?;
                                    Ok(tr_l10n(lang, "status.channel_saved").to_string())
                                });
                                // Secrets are write-only: drop the drafts once Core
                                // has them so they are not kept in egui state.
                                if let Some(form) = self.provider_editor_form.as_mut() {
                                    form.primary_secret.clear();
                                    form.secondary_secret.clear();
                                }
                                self.load_settings_channels();
                                self.load_service_configured();
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderClearSecrets => {
                        if let Some(editor) = self.editor_from_form() {
                            if let Some(backend) = self.backend() {
                                let lang = self.lang;
                                self.spawn(async move {
                                    clear_provider_secrets(Arc::clone(&backend), &editor).await?;
                                    Ok(tr_l10n(lang, "status.secret_cleared").to_string())
                                });
                                if let Some(form) = self.provider_editor_form.as_mut() {
                                    form.primary_secret.clear();
                                    form.secondary_secret.clear();
                                }
                                self.load_service_configured();
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderModels => {
                        let kind = self.settings_channel_kind;
                        let channel_id = self
                            .provider_editor_form
                            .as_ref()
                            .map(|form| form.channel_id.clone());
                        if let (Some(form), Some(channel_id)) =
                            (self.provider_editor_form.as_mut(), channel_id)
                        {
                            form.models_loading = true;
                            self.request_provider_models(kind, channel_id);
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderClose => {
                        self.close_provider_editor();
                    }
                    frontend::view_model::FrontendAction::SettingsChannelToggle(index) => {
                        let kind = self.settings_channel_kind;
                        let target = self
                            .settings_channels
                            .get(index)
                            .map(|channel| (channel.id.clone(), channel.enabled));
                        if let (Some(backend), Some((id, enabled))) = (self.backend(), target) {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.set_channel_enabled(kind, id, !enabled).await?;
                                Ok(tr_l10n(lang, "status.channel_enabled").to_string())
                            });
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelValidate(index) => {
                        let kind = self.settings_channel_kind;
                        let id = self
                            .settings_channels
                            .get(index)
                            .map(|channel| channel.id.clone());
                        if let (Some(backend), Some(id)) = (self.backend(), id) {
                            let lang = self.lang;
                            self.spawn(async move {
                                validate_provider_channel(lang, backend, kind, id).await
                            });
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelDelete(index) => {
                        let kind = self.settings_channel_kind;
                        let id = self
                            .settings_channels
                            .get(index)
                            .map(|channel| channel.id.clone());
                        if let (Some(backend), Some(id)) = (self.backend(), id) {
                            let lang = self.lang;
                            self.spawn(async move {
                                backend.delete_channel(kind, id).await?;
                                Ok(tr_l10n(lang, "status.channel_deleted").to_string())
                            });
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceDetail(index) => {
                        self.frontend_vm.marketplace_selected = Some(index);
                        // Load real detail from backend, not just index.
                        if let Some(item) = self.marketplace_items.get(index) {
                            if let Some(backend) = self.backend() {
                                let id = item.id.clone();
                                let tx = self.tx.clone();
                                self.tokio.spawn(async move {
                                    let result = backend
                                        .services()
                                        .marketplace
                                        .detail(id)
                                        .await
                                        .map_err(|error| error.to_string());
                                    let _ = tx.send(UiResult::MarketplaceDetail(result));
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    /// UI 窗口进程的地址开关。
    const UI_CLIENT_FLAG: &str = "--ui-client";
    const UI_SOCKET_FLAG: &str = "--ui-socket";

    /// 解析 `--ui-client --ui-socket <path>`。普通启动（宿主）返回 `None`。
    fn ui_client_socket(args: &[String]) -> Option<std::path::PathBuf> {
        if !args.iter().any(|arg| arg == UI_CLIENT_FLAG) {
            return None;
        }
        let index = args.iter().position(|arg| arg == UI_SOCKET_FLAG)?;
        args.get(index + 1).map(std::path::PathBuf::from)
    }

    /// 单实例锁的获取结果。
    enum BrokerAcquisition {
        Primary(SingleInstanceBroker),
        /// 已有实例接管了本次启动意图，本进程应当直接退出。
        Forwarded,
    }

    /// 常规启动抢锁（抢不到就把意图转发给已有实例 —— 由它把主窗口推出来）。
    fn acquire_broker(
        runtime_dir: &std::path::Path,
        args: &[String],
    ) -> Result<BrokerAcquisition, String> {
        let lock = runtime_dir.join("openless.lock");
        let socket = runtime_dir.join("openless.sock");
        match SingleInstanceBroker::acquire_or_forward(
            &lock,
            &socket,
            LinuxLaunchIntent::from_args(args),
        )
        .map_err(|error| error.to_string())?
        {
            SingleInstanceRole::Primary(broker) => Ok(BrokerAcquisition::Primary(broker)),
            SingleInstanceRole::Forwarded => Ok(BrokerAcquisition::Forwarded),
        }
    }

    /// 主窗口的最小内尺寸（UI 进程创建窗口时用它，和 `with_min_inner_size` 同源）。
    const MAIN_WINDOW_MIN_INNER_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);

    /// 主窗口的初始尺寸。基准 = macOS（Tauri 的 main 窗口 1300×835）；
    /// 旧的 Linux 专用窗口配置不作为依据。
    const MAIN_WINDOW_INNER_SIZE: [f32; 2] = [1300.0, 835.0];

    /// 视图模型载荷指纹（FNV-1a 64）。够快，用来判断「要不要重发快照」：
    /// 内容没变就不发，UI 慢的时候也不会被无意义的帧糊住。
    fn snapshot_fingerprint(payload: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in payload {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }

    impl OpenLessEguiApp {
        /// 与渲染无关的宿主心跳：原生事件、托盘命令、自动更新检查、泵心跳日志。
        ///
        /// 宿主循环（`run_host`）每 50ms 调它一次。窗口是独立进程，所以热键消费
        /// 与弹窗拉起完全不依赖「窗口是否在绘制」——窗口关掉、最小化、压根没开，
        /// 后台照样收键、照样把弹窗进程拉起来。
        fn tick(&mut self, ctx: &egui::Context) {
            self.poll(ctx);
            self.drain_tray(ctx);
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
            self.log_pump_heartbeat(ctx);
        }

        /// 「显示主窗口」：宿主只记意图，由 `run_host` 拉起/抬起 UI 窗口进程。
        fn request_main_window(&mut self) {
            self.window_should_be_open = true;
        }

        /// UI 窗口进程是否还活着（顺带回收已经退出的子进程）。
        fn ui_window_alive(&mut self) -> bool {
            let Some(child) = self.ui_window.as_mut() else {
                return false;
            };
            match child.try_wait() {
                Ok(None) => true,
                Ok(Some(status)) => {
                    log::info!("[ui-host] UI window process exited ({status}); host keeps running");
                    self.ui_window = None;
                    false
                }
                Err(error) => {
                    log::warn!("[ui-host] UI window process wait failed: {error}");
                    self.ui_window = None;
                    false
                }
            }
        }

        /// 拉起 UI 窗口进程。
        ///
        /// 时序：调用方保证宿主已经 bind 好桥 socket（UI 进程连不上就直接报错退出，
        /// 不会自己抢单实例锁或打开数据目录）。
        fn spawn_ui_window(&mut self, socket: &std::path::Path) -> Result<(), String> {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            let mut command = std::process::Command::new(executable);
            command
                .arg(UI_CLIENT_FLAG)
                .arg(UI_SOCKET_FLAG)
                .arg(socket)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                // 让 panic / winit 警告走宿主自己的 stderr（终端或 journal），
                // 否则「窗口没起来」会变成一条无声的失败。
                .stderr(std::process::Stdio::inherit());
            let child = command.spawn().map_err(|error| error.to_string())?;
            log::info!(
                "[ui-host] spawned UI window process pid={} socket={}",
                child.id(),
                socket.display()
            );
            self.ui_window = Some(child);
            self.ui_window_spawned_at = Some(std::time::Instant::now());
            Ok(())
        }

        /// 窗口当前是否需要一个 UI 进程：用户想开着，而且现在没有活着的窗口。
        /// 刚拉起的 800ms 内不重复拉起，避免连点托盘菜单拉出两个窗口。
        fn should_spawn_ui_window(&mut self) -> bool {
            if !self.window_should_be_open {
                return false;
            }
            if self.ui_window_alive() {
                return false;
            }
            if let Some(spawned_at) = self.ui_window_spawned_at {
                if spawned_at.elapsed() < Duration::from_millis(800) {
                    return false;
                }
            }
            true
        }

        /// 处理 UI 进程发来的消息（含断连语义）。
        ///
        /// 时序：`Bye`/断开只把窗口标记为关闭，**不动**后端、会话与弹窗；
        /// 没有托盘时则连宿主一起退出 —— 否则用户再也找不到这个进程。
        fn apply_window_messages(&mut self, messages: Vec<WindowToHost>, tray_available: bool) {
            for message in messages {
                match message {
                    WindowToHost::Hello { version } => {
                        if version != UI_BRIDGE_VERSION {
                            log::warn!(
                                "[ui-host] UI window speaks protocol {version}, host speaks {UI_BRIDGE_VERSION}"
                            );
                        } else {
                            log::info!("[ui-host] UI window handshake ok (protocol {version})");
                        }
                        // 窗口进程刚起来（可能是重启）：热键配置必须无条件重发一份，
                        // 否则新窗口拿不到绑定，它自己就没法匹配本地热键。
                        self.hotkeys_sent = None;
                    }
                    WindowToHost::Action { sequence, action } => {
                        log::debug!("[ui-host] UI action #{sequence}: {action:?}");
                        self.pending_ui_actions.push(action);
                    }
                    WindowToHost::Hotkey { sequence, edge } => {
                        log::info!("[hotkey] local edge from the UI window #{sequence}: {edge:?}");
                        self.pending_local_hotkeys
                            .push((std::time::Instant::now(), edge));
                    }
                    WindowToHost::Ping { sequence } => {
                        self.pending_ui_pongs.push(sequence);
                    }
                    WindowToHost::Bye => {
                        log::info!("[ui-host] UI window said goodbye; host keeps running");
                        self.window_should_be_open = false;
                    }
                }
            }
            if !self.window_should_be_open && !tray_available {
                // 没有托盘就没有重新打开的入口，窗口退出等于应用退出。
                log::info!("[ui-host] no tray to reopen the window; exiting with it");
                self.exit_requested = true;
            }
        }

        /// 把当前视图模型发给 UI 进程：内容变过、或距上次超过 2s（保活）才发。
        fn publish_view_model(&mut self, bridge: &mut UiBridgeHost) {
            if !bridge.is_connected() {
                // UI 不在：清掉指纹，等它回来时无条件发一份完整快照。
                self.last_snapshot_fingerprint = None;
                return;
            }
            let payload = match serde_json::to_vec(&self.frontend_vm) {
                Ok(payload) => payload,
                Err(error) => {
                    log::warn!("[ui-host] view model serialization failed: {error}");
                    return;
                }
            };
            let fingerprint = snapshot_fingerprint(&payload);
            let keepalive = self.last_snapshot_at.elapsed() >= Duration::from_secs(2);
            if Some(fingerprint) == self.last_snapshot_fingerprint && !keepalive {
                return;
            }
            self.last_snapshot_fingerprint = Some(fingerprint);
            self.last_snapshot_at = std::time::Instant::now();
            bridge.send_snapshot_encoded(&payload);
        }
    }

    impl Drop for OpenLessEguiApp {
        fn drop(&mut self) {
            if let Some(native) = self.native.take() {
                let _ = self.tokio.block_on(native.shutdown());
            }
        }
    }

    /// Map a concrete UI language to its display-name catalog key, shown in
    /// that language's own native script regardless of the current UI language.
    fn overview_activity_day(day: DailyActivity) -> frontend::view_model::OverviewActivityDay {
        frontend::view_model::OverviewActivityDay {
            date: day.date,
            count: day.count,
            chars: day.chars,
            duration_ms: day.duration_ms,
        }
    }

    fn overview_heatmap_day(day: DailyActivity) -> frontend::view_model::OverviewHeatmapDay {
        frontend::view_model::OverviewHeatmapDay {
            date: day.date,
            count: day.count,
        }
    }

    /// Core polish mode -> frontend display enum.
    fn overview_mode(mode: openless_core::PolishMode) -> frontend::view_model::OverviewMode {
        match mode {
            openless_core::PolishMode::Raw => frontend::view_model::OverviewMode::Raw,
            openless_core::PolishMode::Light => frontend::view_model::OverviewMode::Light,
            openless_core::PolishMode::Structured => frontend::view_model::OverviewMode::Structured,
            openless_core::PolishMode::Formal => frontend::view_model::OverviewMode::Formal,
        }
    }

    /// Localized label for a polish mode (used as the history pill fallback).
    fn polish_mode_label(lang: Lang, mode: openless_core::PolishMode) -> &'static str {
        match mode {
            openless_core::PolishMode::Raw => tr_l10n(lang, "overview.mode_raw"),
            openless_core::PolishMode::Light => tr_l10n(lang, "overview.mode_light"),
            openless_core::PolishMode::Structured => tr_l10n(lang, "overview.mode_structured"),
            openless_core::PolishMode::Formal => tr_l10n(lang, "overview.mode_formal"),
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

    /// Localized provider name from `settings.providers.presets.<label_key>`.
    /// Falls back to the raw label id when the catalog has no entry, so a
    /// missing translation never leaks an i18n key into the UI.
    fn localized_provider_label(
        lang: Lang,
        kind: openless_core::ChannelKind,
        provider_type: &str,
    ) -> String {
        let label_key = provider_label_key(kind, provider_type);
        let key = format!("settings.providers.presets.{label_key}");
        let text = fmt_l10n(lang, &key, &[]);
        if text == key {
            label_key
        } else {
            text
        }
    }

    /// i18n lookup id for a provider type (falls back to the raw type id).
    fn provider_label_key(kind: openless_core::ChannelKind, provider_type: &str) -> String {
        openless_core::provider_rules::provider_descriptor(provider_kind(kind), provider_type)
            .map(|descriptor| descriptor.label_key)
            .unwrap_or_else(|| provider_type.to_string())
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

    /// Core's `AuthRequirement` decides which inputs the editor renders. The
    /// host maps it to a render hint and keeps validating through Core.
    fn settings_provider_auth(
        requirement: openless_core::AuthRequirement,
    ) -> frontend::view_model::SettingsProviderAuth {
        use frontend::view_model::SettingsProviderAuth as Ui;
        use openless_core::AuthRequirement as Core;
        match requirement {
            Core::None => Ui::None,
            Core::Volcengine => Ui::Volcengine,
            Core::Xfyun => Ui::Xfyun,
            Core::OAuth => Ui::OAuth,
            Core::TencentCloud => Ui::Other,
            Core::ApiKey | Core::EndpointModelOptionalApiKey | Core::ApiKeyUnlessCustomEndpoint => {
                Ui::ApiKey
            }
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

    /// Write a non-secret value (endpoint/model/resource id/auth mode), or drop
    /// it when the field was cleared: an empty string must not be stored as a
    /// credential that then reads back as "configured".
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

    /// Secrets are write-only: an empty input means "keep the stored key", not
    /// "erase it" — erasing has its own explicit action.
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

    /// Persist the editor through Core: rename, then the credential schema that
    /// matches the selected `ProviderDescriptor`. Account names are Core's wire
    /// schema; which of them is required stays in Core, never in this form.
    async fn save_provider_editor(
        backend: Arc<openless_core::OpenLessBackend>,
        editor: ProviderEditor,
    ) -> Result<(), BackendError> {
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
                    openless_core::credentials::VOLCENGINE_SERVICE_ACCOUNT,
                    &editor.volcengine_service,
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

    /// Drop every credential of the selected descriptor shape for one channel.
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
        let volcengine_service =
            if descriptor.auth_requirement == openless_core::AuthRequirement::Volcengine {
                read_provider_value(
                    &backend,
                    kind,
                    &channel.id,
                    openless_core::credentials::VOLCENGINE_SERVICE_ACCOUNT,
                )
                .await?
                .unwrap_or_else(|| "standard".to_string())
            } else {
                String::new()
            };
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
            volcengine_service,
            auth_mode,
            resource_id,
            primary_secret: String::new(),
            secondary_secret: String::new(),
        })
    }

    async fn validate_provider_channel(
        lang: Lang,
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
                thinking_enabled: false,
                channel_id: Some(channel_id.clone()),
            })
            .await;
        let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        match result {
            Ok(_) => {
                backend
                    .record_channel_test(kind, channel_id, true, Some(latency_ms), None)
                    .await?;
                Ok(fmt_l10n(lang, "status.provider_validated", &[&latency_ms]))
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

    /// OpenLess 数据目录。宿主写数据，UI 进程只用它定位日志文件。
    fn openless_data_dir() -> Result<std::path::PathBuf, String> {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".local/share"))
            })
            .map(|base| base.join("OpenLess"))
            .ok_or_else(|| "HOME/XDG_DATA_HOME is unavailable".to_string())
    }

    fn backend_config(
        tray_available: bool,
        updater_available: bool,
    ) -> Result<BackendConfig, String> {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let data_dir = openless_data_dir()?;
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
        // 安装包升级会替换 libopenless.so，但运行中的 fcitx5 仍持有旧映像 ——
        // 不重启它，新的热键匹配规则就不会生效。只在插件确实更新过时重启，
        // 并且**绝不放在启动关键路径上**：`fcitx5 -r` 会变成常驻的守护进程，
        // 早先在这里等它直接导致主窗口出不来。现在丢到后台线程，启动只做纯计算。
        let reload_plan = plan.clone();
        let reload_data_dir = config.data_dir.clone();
        std::thread::spawn(move || {
            openless_linux_egui::reload_fcitx5_if_plugin_updated(&reload_plan, &reload_data_dir);
        });
        reconcile_fcitx5_install(status)
    }

    /// Map an fcitx5 addon install result onto startup.
    ///
    /// A ready addon lets startup continue down the normal fcitx5 DBus path —
    /// never a global-hotkey fallback — and only a genuinely missing plugin
    /// aborts startup.
    /// 插件缺失/未就绪 **绝不是** 启动失败：主窗口必须照常出现，只是全局热键
    /// 暂时不可用。早先这里 `Err(...)?` 会把整个启动打断，表现就是「主窗口不显示」。
    fn reconcile_fcitx5_install(status: FcitxPluginStatus) -> Result<(), String> {
        match status {
            FcitxPluginStatus::Ready => Ok(()),
            FcitxPluginStatus::Missing => {
                log::warn!(
                    "[fcitx] no OpenLess fcitx5 addon found in the package paths; \
                     global hotkeys stay unavailable until the package is reinstalled"
                );
                Ok(())
            }
        }
    }

    /// 划词追问头像：登录名变化时后台取 `github.com/{login}.png`，解码后上传成
    /// egui 贴图（Tauri `UserAvatar`）。取图失败保持 GitHub 图标兜底。
    #[derive(Default)]
    struct QaAvatar {
        login: String,
        texture: Option<egui::TextureHandle>,
        pending: Option<mpsc::Receiver<Result<egui::ColorImage, String>>>,
    }

    impl QaAvatar {
        fn sync(&mut self, ctx: &egui::Context, login: &str) {
            if login != self.login {
                self.login = login.to_string();
                self.texture = None;
                self.pending = None;
                if !login.trim().is_empty() {
                    self.pending = Some(spawn_github_avatar_fetch(login.trim().to_string()));
                }
            }
            let Some(receiver) = self.pending.as_ref() else {
                return;
            };
            match receiver.try_recv() {
                Ok(Ok(image)) => {
                    self.texture = Some(ctx.load_texture(
                        "openless-qa-user-avatar",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                    self.pending = None;
                }
                Ok(Err(error)) => {
                    log::debug!("avatar unavailable: {error}");
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // 取图在别的线程：保持重绘直到结果回来。
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
            }
        }
    }

    fn spawn_github_avatar_fetch(
        login: String,
    ) -> mpsc::Receiver<Result<egui::ColorImage, String>> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("openless-avatar".into())
            .spawn(move || {
                let _ = tx.send(fetch_github_avatar(&login));
            })
            .ok();
        rx
    }

    /// GitHub 公开头像接口（无需登录；Tauri 用的是同一个 URL 形状）。
    fn fetch_github_avatar(login: &str) -> Result<egui::ColorImage, String> {
        let encoded: String = login
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                    character.to_string()
                } else {
                    let mut buffer = [0u8; 4];
                    character
                        .encode_utf8(&mut buffer)
                        .bytes()
                        .map(|byte| format!("%{byte:02X}"))
                        .collect()
                }
            })
            .collect();
        let url = format!("https://github.com/{encoded}.png?size=64");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let bytes = runtime.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .map_err(|error| error.to_string())?;
            let response = client
                .get(&url)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            if !response.status().is_success() {
                return Err(format!("avatar http {}", response.status()));
            }
            response.bytes().await.map_err(|error| error.to_string())
        })?;
        let decoded = image::load_from_memory(&bytes).map_err(|error| error.to_string())?;
        let rgba = decoded.to_rgba8();
        Ok(egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        ))
    }

    /// X11 overlay placement for the capsule popup.
    ///
    /// The capsule must land at the bottom centre of the work area and must
    /// never take the keyboard: on macOS Tauri gets the same guarantee from
    /// `orderFrontRegardless` ("visible but not the key window"). Under XWayland
    /// the equivalent is `WM_HINTS.input = False`, which is why the capsule is
    /// launched with the Wayland backend removed (see
    /// [`openless_linux_egui::popup_command`]).
    #[cfg(all(target_os = "linux", feature = "x11-overlay"))]
    mod popup_overlay {
        use super::*;
        use openless_linux_egui::{
            place_overlay, popup_position, OverlayEnvironment, OverlayPlacement, X11Overlay,
        };

        pub struct PopupOverlay {
            kind: PopupKind,
            connection: X11Overlay,
            environment: OverlayEnvironment,
            /// The pre-map pass (hints + geometry) ran.
            placed: bool,
            /// The post-map pass (EWMH states, once the window is managed).
            reasserted: bool,
            attempts: u8,
        }

        impl PopupOverlay {
            pub fn probe(kind: PopupKind) -> Option<Self> {
                // 纯 Wayland（没有 XWayland）时不做任何 X11 处理，按原行为跑。
                if !openless_linux_egui::x11_available(std::env::var("DISPLAY").ok().as_deref()) {
                    log::debug!("popup x11: no DISPLAY, keeping the compositor placement");
                    return None;
                }
                let connection = match X11Overlay::connect() {
                    Ok(connection) => connection,
                    Err(error) => {
                        log::warn!(
                            "capsule x11: connect failed, staying with the compositor: {error}"
                        );
                        return None;
                    }
                };
                let environment = match connection.probe() {
                    Ok(environment) => environment,
                    Err(error) => {
                        log::warn!("capsule x11: geometry probe failed: {error}");
                        OverlayEnvironment::default()
                    }
                };
                log::info!(
                    "capsule x11: work_area={:?} monitors={} cursor={:?} active_window={:?}",
                    environment.work_area,
                    environment.monitors.len(),
                    environment.cursor,
                    environment.active_window
                );
                Some(Self {
                    kind,
                    connection,
                    environment,
                    placed: false,
                    reasserted: false,
                    attempts: 0,
                })
            }

            /// Position handed to `ViewportBuilder::with_position`, so the pill
            /// is already in place the first time it is shown.
            pub fn initial_position(&self) -> Option<(i32, i32)> {
                popup_position(&self.environment, self.kind)
            }

            fn apply(&mut self, reason: &str) -> OverlayPlacement {
                let placement = place_overlay(
                    &mut self.connection,
                    std::process::id(),
                    &self.environment,
                    self.kind,
                );
                if placement.applied() {
                    log::info!(
                        "capsule x11 ({reason}): window={:?} matched={} moved_to={:?} focus_was_stolen={} focus_restored={} warnings={:?}",
                        placement.window,
                        placement
                            .matched
                            .map(openless_linux_egui::WindowMatch::as_str)
                            .unwrap_or("none"),
                        placement.moved_to,
                        placement.focus_was_stolen,
                        placement.focus_restored,
                        placement.warnings
                    );
                }
                // Milestone line for real-machine verification: the popup
                // process installs no logger, but it inherits stderr from the
                // host, so this is the one place the fallback is observable
                // (`journalctl --user -f | grep 'OpenLess capsule'`).
                eprintln!(
                    "OpenLess capsule: x11 {reason} window={:?} matched={} moved_to={:?} \
focus_was_stolen={} focus_restored={} warnings={:?}",
                    placement.window,
                    placement
                        .matched
                        .map(openless_linux_egui::WindowMatch::as_str)
                        .unwrap_or("none"),
                    placement.moved_to,
                    placement.focus_was_stolen,
                    placement.focus_restored,
                    placement.warnings
                );
                placement
            }

            pub fn place(&mut self, ctx: &egui::Context, visible: bool) {
                // 只有胶囊需要「永不聚焦 + 置顶 + 不进任务栏」；两个面板要键盘输入，
                // 位置已经由 `with_position` 在创建时给过，X11 变更一概不做。
                if self.kind != PopupKind::Capsule {
                    self.placed = true;
                    return;
                }
                if !self.placed {
                    self.attempts = self.attempts.saturating_add(1);
                    if self.apply("pre-map").applied() {
                        self.placed = true;
                    } else if self.attempts >= 100 {
                        // The window never showed up in the tree: stop asking but
                        // keep the pill working with the compositor's placement.
                        log::warn!(
                            "capsule x11: own window not found, keeping the compositor placement"
                        );
                        self.placed = true;
                    } else {
                        // The window is created a frame or two after the app
                        // starts; try again on the next tick.
                        ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    }
                    return;
                }
                if visible && !self.reasserted {
                    // Now that the window is managed, (re)assert above +
                    // skip-taskbar and the geometry the manager may have moved.
                    self.reasserted = true;
                    self.apply("post-map");
                }
            }
        }
    }

    /// Pure Wayland build: the capsule keeps the compositor's placement.
    #[cfg(not(all(target_os = "linux", feature = "x11-overlay")))]
    mod popup_overlay {
        use super::*;

        pub struct PopupOverlay;

        impl PopupOverlay {
            pub fn probe(_kind: PopupKind) -> Option<Self> {
                None
            }

            pub fn initial_position(&self) -> Option<(i32, i32)> {
                None
            }

            pub fn place(&mut self, _ctx: &egui::Context, _visible: bool) {}
        }
    }

    use popup_overlay::PopupOverlay;

    struct NativePopupApp {
        kind: PopupKind,
        state: PopupState,
        incoming: mpsc::Receiver<HostToPopup>,
        outgoing: mpsc::Sender<PopupToHost>,
        qa_input: String,
        /// Less Computer 面板的输入框（与 QA 的 composer 各自独立）。
        less_computer_input: String,
        outgoing_sequence: u64,
        ready_sent: bool,
        avatar: QaAvatar,
        lang: Lang,
        /// X11 overlay placement for the capsule (bottom-centre, never focus).
        overlay: Option<PopupOverlay>,
        /// 面板有焦点时插件收不到按键，所以面板自己也要匹配本地热键。
        hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher,
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
                PopupKind::Capsule => PopupToHost::DismissCapsule {
                    version,
                    session_id,
                    sequence,
                },
                PopupKind::LessComputer => PopupToHost::DismissLessComputer {
                    version,
                    session_id,
                    sequence,
                },
            };
            self.send(message);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    impl NativePopupApp {
        /// Drain every host message queued since the last frame. `ctx` is
        /// `None` on the layer-shell path, which has no viewport to command:
        /// visibility and shutdown are handled by the runner instead.
        ///
        /// Returns true when the process should exit.
        fn pump(&mut self, ctx: Option<&egui::Context>) -> bool {
            loop {
                let message = match self.incoming.try_recv() {
                    Ok(message) => message,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // 宿主进程没了（stdin 到 EOF → 读线程结束 → 发送端析构）。
                        // 之前这里把 Empty 和 Disconnected 一起当成「没有消息」，
                        // 于是胶囊会在宿主崩溃/被杀后永久贴在屏幕上（layer surface
                        // 不能隐藏，只能随进程销毁）。宿主不在了就该自己退场。
                        log::warn!(
                            "openless popup ({:?}): host pipe closed — closing the popup",
                            self.kind
                        );
                        if let Some(ctx) = ctx {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        return true;
                    }
                };
                if message
                    .content_kind()
                    .is_some_and(|message_kind| message_kind != self.kind)
                {
                    continue;
                }
                let shutdown = matches!(message, HostToPopup::Shutdown { .. });
                let outcome = self.state.apply(message);
                if outcome == openless_linux_egui::PopupApplyOutcome::Applied {
                    if let Some(ctx) = ctx {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.state.visible));
                    }
                }
                if shutdown || self.state.shutdown_requested {
                    if let Some(ctx) = ctx {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    return true;
                }
            }
        }

        /// 面板内的本地热键：命中就报给宿主，由宿主按与插件信号同一套规则处理。
        ///
        /// 胶囊不接受键盘焦点（layer surface 也拿不到），所以只对需要打字的面板生效。
        fn poll_local_hotkeys(&mut self, ctx: &egui::Context) {
            if self.kind == PopupKind::Capsule {
                return;
            }
            let Some(bindings) = self.state.hotkeys.as_ref() else {
                return;
            };
            let Some(edge) = self.hotkey_matcher.poll(ctx, bindings) else {
                return;
            };
            if let Some(session_id) = self.session_id() {
                let sequence = self.next_sequence();
                log::info!(
                    "openless popup ({:?}): local hotkey edge {edge:?}",
                    self.kind
                );
                self.send(PopupToHost::Hotkey {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id,
                    sequence,
                    edge,
                });
            }
        }

        /// Tell the host which session this window is serving. The host drops
        /// every message that carries another session id, so this must happen
        /// before the first content arrives.
        fn send_ready_if_needed(&mut self) {
            if self.ready_sent {
                return;
            }
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

        /// One capsule frame on a layer surface: same view, same protocol and
        /// same send / exit rules as the eframe window, minus viewport
        /// commands (a layer surface is sized by the compositor).
        fn layer_frame(
            &mut self,
            ctx: &egui::Context,
            raw: egui::RawInput,
            first: bool,
        ) -> openless_linux_egui::LayerFrame {
            if first {
                theme::install(ctx);
            }
            let mut exit = self.pump(None);
            self.send_ready_if_needed();
            let animated = matches!(
                self.state.capsule.phase.to_ascii_lowercase().as_str(),
                "starting" | "recording" | "transcribing" | "polishing" | "inserting"
            );
            let capsule = self.state.capsule.clone();
            let lang = self.lang;
            let mut action = frontend::popups::CapsuleAction::None;
            let output = ctx.run(raw, |ctx| {
                action = frontend::popups::dictation_capsule(ctx, &capsule, lang);
            });
            match action {
                frontend::popups::CapsuleAction::None => {}
                frontend::popups::CapsuleAction::Cancel
                | frontend::popups::CapsuleAction::Confirm => {
                    if let Some(session_id) = self.session_id() {
                        let sequence = self.next_sequence();
                        let message = if matches!(action, frontend::popups::CapsuleAction::Cancel) {
                            PopupToHost::CancelDictation {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id,
                                sequence,
                            }
                        } else {
                            PopupToHost::StopDictation {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id,
                                sequence,
                            }
                        };
                        self.send(message);
                    }
                    exit = true;
                }
            }
            openless_linux_egui::LayerFrame {
                output,
                exit,
                // Same cadence as the windowed popup: animate fast, idle slowly.
                repaint_after: Duration::from_millis(if self.state.visible && animated {
                    33
                } else {
                    100
                }),
            }
        }
    }

    impl eframe::App for NativePopupApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // Overlay placement first: it must run before the window is shown so
            // the compositor never gives the capsule the keyboard.
            if let Some(overlay) = self.overlay.as_mut() {
                overlay.place(ctx, self.state.visible);
            }
            if self.pump(Some(ctx)) {
                return;
            }
            self.send_ready_if_needed();
            self.poll_local_hotkeys(ctx);
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.dismiss(ctx);
                return;
            }
            let lang = self.lang;
            // 只有「有动画」的状态需要 30fps 连续重绘：录音音量条、思考光环/光点、
            // 头像取图中。静止或隐藏时降到 10fps（stdin 轮询延迟 ≤100ms，肉眼无感），
            // 避免透明置顶窗口长期白跑帧。
            let mut animated = false;
            match self.kind {
                PopupKind::Qa => {
                    self.avatar.sync(ctx, &self.state.qa.viewer_login);
                    let qa_phase = self.state.qa.phase.to_ascii_lowercase();
                    animated = self.avatar.pending.is_some()
                        || matches!(
                            qa_phase.as_str(),
                            "loading" | "thinking" | "recording" | "answerdelta"
                        );
                    let action = frontend::popups::selection_ask(
                        ctx,
                        &self.state.qa,
                        &mut self.qa_input,
                        lang,
                        self.avatar.texture.as_ref(),
                    );
                    match action {
                        frontend::popups::QaAction::Dismiss => self.dismiss(ctx),
                        frontend::popups::QaAction::ConfirmPolish(text) => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::ConfirmPolish {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    text,
                                });
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        frontend::popups::QaAction::CancelPolish => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::CancelPolish {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                });
                            }
                            self.dismiss(ctx);
                        }
                        frontend::popups::QaAction::ToggleRecording => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::ToggleQaRecording {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                });
                            }
                        }
                        frontend::popups::QaAction::SetPinned(pinned) => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::SetPinned {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    pinned,
                                });
                            }
                        }
                        frontend::popups::QaAction::SetEditInstructionMode(enabled) => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::SetEditInstructionMode {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    enabled,
                                });
                            }
                        }
                        frontend::popups::QaAction::ApplyEdit => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::ApplyEdit {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                });
                            }
                        }
                        frontend::popups::QaAction::RevertEdit => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::RevertEdit {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                });
                            }
                        }
                        frontend::popups::QaAction::Submit(text) => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::SubmitQa {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    text,
                                });
                            }
                        }
                        frontend::popups::QaAction::None => {}
                    }
                }
                PopupKind::LessComputer => {
                    // 运行中的一轮需要连续重绘（「执行中…」标记 + 滚动到底）。
                    animated = self.state.less_computer.working;
                    let action = frontend::popups::less_computer(
                        ctx,
                        &self.state.less_computer,
                        &mut self.less_computer_input,
                        lang,
                    );
                    match action {
                        frontend::popups::LessComputerAction::None => {}
                        frontend::popups::LessComputerAction::Dismiss => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::DismissLessComputer {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                });
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        frontend::popups::LessComputerAction::Cancel => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::CancelLessComputer {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                });
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        frontend::popups::LessComputerAction::Submit(text) => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::SubmitLessComputer {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    text,
                                });
                            }
                        }
                        frontend::popups::LessComputerAction::Approve { token, approved } => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::ApproveLessComputer {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    token,
                                    approved,
                                });
                            }
                        }
                    }
                }
                PopupKind::Capsule => {
                    let capsule_phase = self.state.capsule.phase.to_ascii_lowercase();
                    animated = matches!(
                        capsule_phase.as_str(),
                        "starting" | "recording" | "transcribing" | "polishing" | "inserting"
                    );
                    let action =
                        frontend::popups::dictation_capsule(ctx, &self.state.capsule, lang);
                    let message = match action {
                        frontend::popups::CapsuleAction::Cancel => {
                            Some(PopupToHost::CancelDictation {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: String::new(),
                                sequence: 0,
                            })
                        }
                        frontend::popups::CapsuleAction::Confirm => {
                            Some(PopupToHost::StopDictation {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: String::new(),
                                sequence: 0,
                            })
                        }
                        frontend::popups::CapsuleAction::None => None,
                    };
                    if let Some(mut message) = message {
                        if let Some(session_id) = self.session_id() {
                            let sequence = self.next_sequence();
                            match &mut message {
                                PopupToHost::CancelDictation {
                                    session_id: id,
                                    sequence: seq,
                                    ..
                                }
                                | PopupToHost::StopDictation {
                                    session_id: id,
                                    sequence: seq,
                                    ..
                                } => {
                                    *id = session_id;
                                    *seq = sequence;
                                }
                                _ => {}
                            }
                            self.send(message);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(if self.state.visible && animated {
                33
            } else {
                100
            }));
        }
    }

    fn popup_kind(args: &[String]) -> Option<PopupKind> {
        if !args.iter().any(|arg| arg == "--openless-egui-popup") {
            return None;
        }
        if args.iter().any(|arg| arg == "--qa") {
            Some(PopupKind::Qa)
        } else if args.iter().any(|arg| arg == "--capsule") {
            Some(PopupKind::Capsule)
        } else if args.iter().any(|arg| arg == "--less-computer") {
            Some(PopupKind::LessComputer)
        } else {
            None
        }
    }

    /// Append a streaming delta to the trailing assistant entry, creating it on
    /// the first delta of a turn. Keeps one assistant bubble per turn instead of
    /// one per delta, matching the Tauri panel's message list.
    fn append_assistant_entry(
        entries: &mut Vec<openless_linux_egui::LessComputerEntry>,
        delta: &str,
    ) {
        match entries.last_mut() {
            Some(entry) if entry.kind == "assistant" => entry.text.push_str(delta),
            _ => entries.push(openless_linux_egui::LessComputerEntry {
                kind: "assistant".to_string(),
                text: delta.to_string(),
            }),
        }
    }

    /// stdin/stdout JSONL plumbing shared by the eframe popup window and the
    /// layer-shell capsule: both talk to the host through the same protocol.
    fn popup_stdio() -> Result<(mpsc::Receiver<HostToPopup>, mpsc::Sender<PopupToHost>), String> {
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
        Ok((rx, outgoing_tx))
    }

    /// 胶囊在实现了 `zwlr_layer_shell_v1` 的合成器上跑原生 layer surface
    /// （贴底居中、键盘焦点不可能、不占工作区）；协议缺失、EGL 起不来或
    /// configure 超时都会返回 Err，由调用方回退到 XWayland 叠加层。
    fn run_capsule_layer_process() -> Result<(), LayerCapsuleFailure> {
        let geometry = openless_linux_egui::capsule_geometry(
            openless_linux_egui::CAPSULE_WINDOW_SIZE.0,
            openless_linux_egui::CAPSULE_WINDOW_SIZE.1,
            openless_linux_egui::CAPSULE_BOTTOM_GAP,
        );
        // The host pipe is opened lazily, on the first frame the runner asks
        // for: the runner only calls back once the layer surface is configured
        // and EGL is live, so a preflight failure leaves stdin untouched for the
        // XWayland fallback. `started` records that the pipe is in use, which
        // makes a late failure fatal instead of a (broken) second attempt.
        let mut app: Option<NativePopupApp> = None;
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_in_frame = Arc::clone(&started);
        let result = openless_linux_egui::run_layer_capsule(geometry, move |ctx, raw, first| {
            if app.is_none() {
                match popup_stdio() {
                    Ok((incoming, outgoing)) => {
                        started_in_frame.store(true, std::sync::atomic::Ordering::SeqCst);
                        app = Some(NativePopupApp {
                            kind: PopupKind::Capsule,
                            state: PopupState::default(),
                            incoming,
                            outgoing,
                            qa_input: String::new(),
                            less_computer_input: String::new(),
                            outgoing_sequence: 0,
                            ready_sent: false,
                            avatar: QaAvatar::default(),
                            lang: load_locale_pref().resolve(),
                            overlay: None,
                            hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
                        });
                    }
                    Err(error) => {
                        log::error!("popup pipe unavailable: {error}");
                        let output = ctx.run(raw, |_| {});
                        return openless_linux_egui::LayerFrame {
                            output,
                            exit: true,
                            repaint_after: std::time::Duration::from_millis(0),
                        };
                    }
                }
            }
            let app = app.as_mut().expect("popup app is created above");
            app.layer_frame(ctx, raw, first)
        });
        match result {
            Ok(()) => Ok(()),
            Err(error) if started.load(std::sync::atomic::Ordering::SeqCst) => {
                // The capsule was already on screen: the host pipe is in use, so
                // there is nothing to fall back to (a second window would fight
                // this process for the same stdin). Report and let it die.
                Err(LayerCapsuleFailure {
                    message: format!("layer-shell capsule stopped after startup: {error}"),
                    started: true,
                })
            }
            Err(error) => Err(LayerCapsuleFailure {
                message: error,
                started: false,
            }),
        }
    }

    /// Why the layer-shell capsule gave up, plus whether it had already taken
    /// over the host pipe — a live capsule cannot fall back to a second window.
    struct LayerCapsuleFailure {
        message: String,
        started: bool,
    }

    fn run_popup_process(kind: PopupKind) -> Result<(), String> {
        // 胶囊优先走原生 layer surface；不可用时（无该协议 / EGL 失败 /
        // configure 超时）安静回退到下面那条 XWayland 叠加层路径。
        if kind == PopupKind::Capsule
            && openless_linux_egui::detect_capsule_path()
                == openless_linux_egui::CapsulePath::LayerShell
        {
            match run_capsule_layer_process() {
                Ok(()) => return Ok(()),
                Err(failure) if failure.started => return Err(failure.message),
                Err(failure) => {
                    eprintln!(
                        "OpenLess capsule: layer-shell unavailable ({}); using the XWayland overlay",
                        failure.message
                    );
                    log::warn!(
                        "layer-shell capsule unavailable ({}); falling back to the XWayland overlay",
                        failure.message
                    );
                }
            }
        }
        let (rx, outgoing_tx) = popup_stdio()?;
        // 胶囊窗口贴着药丸尺寸（Tauri 经典药丸 176×42），并用透明背景让圆角
        // 真正透出桌面；QA / 预览是实心卡片窗口。
        let size = match kind {
            // 面板尺寸一律取自共享常量（Tauri：qa/less-computer 420×540）。
            PopupKind::Qa => [
                openless_linux_egui::QA_WINDOW_SIZE.0 as f32,
                openless_linux_egui::QA_WINDOW_SIZE.1 as f32,
            ],
            PopupKind::LessComputer => [
                openless_linux_egui::LESS_COMPUTER_WINDOW_SIZE.0 as f32,
                openless_linux_egui::LESS_COMPUTER_WINDOW_SIZE.1 as f32,
            ],
            // 经典药丸 176×42 + 16px 下边距 + 8px 间距 + 「正在翻译」徽章
            // （Tauri `getCapsuleHostMetrics(.., 'classic')` 的 100 高度）。
            PopupKind::Capsule => [
                openless_linux_egui::CAPSULE_WINDOW_SIZE.0 as f32,
                openless_linux_egui::CAPSULE_WINDOW_SIZE.1 as f32,
            ],
        };
        let transparent = matches!(kind, PopupKind::Capsule);
        // 三条路径的优先级（`popup_layer::choose_capsule_path`）：
        //   1. 合成器有 zwlr_layer_shell_v1 → 走原生 layer surface（已在上面 return）
        //   2. 否且有 X 服务器 → XWayland + 下面的 X11 叠加层（本分支）
        //   3. 两者都没有 → 普通无边框窗口，位置/焦点交给合成器
        // 胶囊只在第 2 条路径里做 X11 处理：先读一次几何，让窗口在**创建时**就落
        // 在工作区底部居中，并在映射前把 WM_HINTS.input 关掉（kwin 不会再给它焦点）。
        // 面板窗（QA/预览）需要键盘输入，只借 `with_position` 定位。
        let capsule_on_x11 = kind == PopupKind::Capsule
            && openless_linux_egui::detect_capsule_path()
                == openless_linux_egui::CapsulePath::X11Overlay;
        let overlay = if kind == PopupKind::Capsule && !capsule_on_x11 {
            None
        } else {
            PopupOverlay::probe(kind)
        };
        let initial_position = overlay
            .as_ref()
            .and_then(|overlay| overlay.initial_position());
        let mut viewport = egui::ViewportBuilder::default()
            .with_title("OpenLess")
            .with_inner_size(size)
            .with_decorations(false)
            .with_always_on_top()
            .with_transparent(transparent)
            .with_visible(false);
        if let Some(position) = initial_position {
            viewport = viewport.with_position([position.0 as f32, position.1 as f32]);
        }
        if kind == PopupKind::Capsule {
            // 不主动要激活：Wayland 下由合成器决定，X11 下就是「可见但不是 key
            // window」，与 Tauri 的 `orderFrontRegardless` 同语义。
            viewport = viewport.with_active(false);
        }
        let options = eframe::NativeOptions {
            viewport,
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
                    less_computer_input: String::new(),
                    outgoing_sequence: 0,
                    ready_sent: false,
                    avatar: QaAvatar::default(),
                    // The popup is a separate process, so it re-reads the
                    // persisted UI-locale preference rather than sharing state.
                    lang: load_locale_pref().resolve(),
                    overlay,
                    hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
                }))
            }),
        )
        .map_err(|error| error.to_string())
    }

    /// 无窗口宿主：进程里没有 eframe 窗口，事件泵、托盘与弹窗由这个循环驱动。
    ///
    /// 关掉主窗口后进程会切到这个形态：窗口（以及它在任务栏/窗口列表里的条目）
    /// 因此真的消失（Wayland 下 winit 无法隐藏窗口，只有真退出窗口进程才算数），
    /// 而热键、弹窗、托盘继续工作。托盘「显示主窗口」时再拉起带窗口的进程，
    /// 本进程退出，把单实例锁让出去。
    /// 宿主主循环的节拍。50ms：UI 动作最坏等一个节拍再进行，加上 UI 侧 30ms
    /// 的重绘间隔，端到端仍在 100ms 预算内。
    const HOST_TICK_INTERVAL: Duration = Duration::from_millis(50);

    /// 常驻宿主的运行循环：**本进程没有窗口**。
    ///
    /// 它持有后端 / 数据目录 / 单实例锁 / 热键监听 / 托盘 / 弹窗监督器，并通过
    /// `UiBridgeHost` 与独立的 UI 窗口进程通信。时序：
    /// 1. 单实例锁（`run()` 里已拿到）→ 托盘与热键（native）→ **bind 桥 socket**；
    /// 2. socket 就绪后才拉 UI 进程，UI 连不上宿主就直接报错退出；
    /// 3. 每轮先收 UI 消息、再跑宿主心跳、再按需拉窗口、最后推视图模型；
    /// 4. 退出前先给 UI 发 `Shutdown`，再由 `run()` 的 drop 释放单实例锁。
    fn run_host(
        runtime_dir: &std::path::Path,
        tokio: Arc<tokio::runtime::Runtime>,
        native: Result<LinuxNativeRuntime, String>,
        tray: Option<openless_linux_egui::LinuxTray>,
        update_support: LinuxUpdateSupport,
        start_minimized: bool,
    ) -> Result<(), String> {
        let socket = bridge::ui_socket_path(runtime_dir);
        let mut ui_bridge = UiBridgeHost::bind(socket.clone())
            .map_err(|error| format!("UI bridge bind failed: {error}"))?;
        let tray_available = tray.is_some();
        // 没有托盘时必须开窗，否则关掉就再也找不回来。
        let window_should_be_open = !start_minimized || !tray_available;
        let ctx = egui::Context::default();
        let mut app =
            OpenLessEguiApp::new(tokio, native, tray, update_support, window_should_be_open);
        log::info!(
            "[ui-host] host started (no window in this process); bridge={} window_should_be_open={window_should_be_open} tray={tray_available}",
            ui_bridge.path().display()
        );
        loop {
            ui_bridge.accept_pending();
            let messages = ui_bridge.drain();
            if !messages.is_empty() {
                app.apply_window_messages(messages, tray_available);
            }
            // Debounced marketplace search fires from the host tick.
            app.poll_marketplace_search();
            app.tick(&ctx);
            if app.should_spawn_ui_window() {
                if let Err(error) = app.spawn_ui_window(&socket) {
                    log::warn!("[ui-host] cannot start the UI window: {error}");
                }
            }
            let actions = std::mem::take(&mut app.pending_ui_actions);
            if !actions.is_empty() {
                app.apply_frontend_actions(actions, &ctx);
            }
            for sequence in std::mem::take(&mut app.pending_ui_pongs) {
                ui_bridge.send(HostToWindow::Pong { sequence });
            }
            app.sync_view_model();
            app.sync_hotkey_bindings(&mut ui_bridge);
            app.publish_view_model(&mut ui_bridge);
            if app.exit_requested {
                break;
            }
            std::thread::sleep(HOST_TICK_INTERVAL);
        }
        log::info!("[ui-host] host exiting; asking the UI window to close");
        ui_bridge.shutdown();
        Ok(())
    }

    /// UI 窗口进程入口：只渲染。
    ///
    /// 它不构造 Core 后端、不打开数据目录、不抢单实例锁、不注册托盘与热键 ——
    /// 关掉它等于「关掉一个窗口」，宿主与所有后台能力原地不动。
    fn run_ui_client(socket: std::path::PathBuf) -> Result<(), String> {
        // UI 进程不复用宿主的日志器对象，但写到同一个文件里，
        // 排查「窗口进程怎么没了」时两端日志在同一处。
        if let Ok(data_dir) = openless_data_dir() {
            if let Err(error) = openless_linux_egui::init_file_logger(&data_dir) {
                eprintln!("OpenLess UI window logger unavailable: {error}");
            }
        }
        let client = UiBridgeClient::connect(&socket)?;
        log::info!(
            "[ui-client] connected to the host bridge at {}",
            socket.display()
        );
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("OpenLess")
                .with_inner_size(MAIN_WINDOW_INNER_SIZE)
                .with_min_inner_size(MAIN_WINDOW_MIN_INNER_SIZE)
                .with_decorations(false)
                .with_transparent(true)
                .with_resizable(true)
                .with_visible(true),
            ..Default::default()
        };
        eframe::run_native(
            "OpenLess",
            options,
            Box::new(move |cc| {
                theme::install(&cc.egui_ctx);
                Ok(Box::new(UiClientApp::new(client)))
            }),
        )
        .map_err(|error| error.to_string())
    }

    /// 诊断开关：`OPENLESS_UI_DEBUG=1` 时 UI 进程把指针点击与动作记进日志。
    /// 用来分辨「按钮没反应」是没收到指针事件，还是动作没能送到宿主。
    fn ui_debug_enabled() -> bool {
        std::env::var("OPENLESS_UI_DEBUG").is_ok_and(|value| value == "1")
    }

    /// 是否采纳这一份快照：序号必须**严格递增**（重复、乱序、回退统统丢弃），
    /// 否则 UI 会把新状态画成旧状态。
    fn snapshot_supersedes(last_sequence: u64, sequence: u64) -> bool {
        sequence > last_sequence
    }

    /// UI 进程侧的 eframe 应用：收快照 → 渲染 → 把动作发回宿主。
    struct UiClientApp {
        client: UiBridgeClient,
        view_model: FrontendViewModel,
        /// 已采纳的最大快照序号。
        last_sequence: u64,
        /// 已发出的动作序号（宿主可据此看出重复或丢失）。
        action_sequence: u64,
        ping_sequence: u64,
        ping_sent_at: Option<std::time::Instant>,
        last_ping_at: std::time::Instant,
        latency_samples: Vec<u128>,
        exited: bool,
        /// 宿主下发的本地热键配置（窗口有焦点时插件收不到按键）。
        hotkeys: Option<openless_core::HotkeyRuntimeTarget>,
        hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher,
        /// 本地热键边沿的发送序号（与动作序号分开，便于日志区分）。
        hotkey_sequence: u64,
    }

    impl UiClientApp {
        fn new(client: UiBridgeClient) -> Self {
            Self {
                client,
                view_model: FrontendViewModel::default(),
                last_sequence: 0,
                action_sequence: 0,
                ping_sequence: 0,
                ping_sent_at: None,
                last_ping_at: std::time::Instant::now(),
                latency_samples: Vec::new(),
                exited: false,
                hotkeys: None,
                hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
                hotkey_sequence: 0,
            }
        }

        /// 收宿主的帧。快照按序号采纳；`Shutdown` 与断连都表示「宿主走了」，
        /// 此时 UI 必须自己退出（没有宿主就没有数据可渲染）。
        fn drain_host(&mut self, ctx: &egui::Context) {
            loop {
                match self.client.try_recv() {
                    Ok(HostToWindow::Ready { version }) => {
                        log::info!("[ui-client] host ready (protocol {version})");
                    }
                    Ok(HostToWindow::Hotkeys { bindings, .. }) => {
                        // 窗口有焦点时 fcitx5 收不到按键，本地匹配全靠这份配置。
                        log::info!("[ui-client] local hotkey bindings received");
                        self.hotkeys = Some(*bindings);
                    }
                    Ok(HostToWindow::Snapshot {
                        sequence,
                        view_model,
                    }) => {
                        if snapshot_supersedes(self.last_sequence, sequence) {
                            self.last_sequence = sequence;
                            self.view_model = *view_model;
                        } else {
                            log::debug!("[ui-client] dropped stale snapshot #{sequence}");
                        }
                    }
                    Ok(HostToWindow::Pong { sequence }) => {
                        if let Some(sent_at) = self.ping_sent_at.take() {
                            let rtt = sent_at.elapsed().as_millis();
                            self.latency_samples.push(rtt);
                            if self.latency_samples.len() >= 20 {
                                let count = self.latency_samples.len();
                                let max = self.latency_samples.iter().copied().max().unwrap_or(0);
                                let sum: u128 = self.latency_samples.iter().sum();
                                log::info!(
                                    "[ui-client] ipc round-trip avg={}ms max={max}ms over {count} probes (last #{sequence})",
                                    sum / count as u128
                                );
                                self.latency_samples.clear();
                            }
                        }
                    }
                    Ok(HostToWindow::Shutdown) => {
                        log::info!("[ui-client] host asked to shut down; closing the window");
                        self.exited = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        log::warn!("[ui-client] host connection lost; closing the window");
                        self.exited = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        break;
                    }
                }
            }
        }

        /// 本窗口内命中的热键作为边沿报给宿主。
        ///
        /// 只读 `InputState`（不消费事件），命中才发一帧；正在录制快捷键时跳过，
        /// 否则用户在设置里录「Alt+A」会顺手触发一次听写。
        fn poll_local_hotkeys(&mut self, ctx: &egui::Context) {
            let Some(bindings) = self.hotkeys.as_ref() else {
                return;
            };
            if self.view_model.shortcut_recording.is_some() {
                return;
            }
            let Some(edge) = self.hotkey_matcher.poll(ctx, bindings) else {
                return;
            };
            self.hotkey_sequence += 1;
            if let Err(error) = self.client.send(WindowToHost::Hotkey {
                sequence: self.hotkey_sequence,
                edge,
            }) {
                log::warn!("[ui-client] cannot forward a local hotkey to the host: {error}");
            }
        }

        fn ping_if_due(&mut self) {
            if self.last_ping_at.elapsed() < Duration::from_secs(2) {
                return;
            }
            self.last_ping_at = std::time::Instant::now();
            self.ping_sequence += 1;
            self.ping_sent_at = Some(std::time::Instant::now());
            let _ = self.client.send(WindowToHost::Ping {
                sequence: self.ping_sequence,
            });
        }

        /// 关窗 = 本进程退出。宿主仍在，会话、录音、弹窗都不受影响。
        fn request_exit(&mut self, ctx: &egui::Context, reason: &str) {
            if self.exited {
                return;
            }
            log::info!("[ui-client] {reason}: exiting the window process (host keeps running)");
            self.exited = true;
            let _ = self.client.send(WindowToHost::Bye);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        /// 把渲染层产生的动作分发出去：窗口控制就地处理，其余发给宿主。
        fn dispatch(
            &mut self,
            actions: Vec<frontend::view_model::FrontendAction>,
            ctx: &egui::Context,
        ) {
            for action in actions {
                match action {
                    frontend::view_model::FrontendAction::WindowClose => {
                        self.request_exit(ctx, "close button");
                    }
                    frontend::view_model::FrontendAction::WindowMinimize => {
                        // 最小化在 Wayland 上是单向门（winit 明确拒绝取消最小化），
                        // 而宿主已经接管热键与弹窗，所以按「关窗回托盘」处理：
                        // 窗口进程退出，任务栏条目消失，托盘随时能再开一个。
                        self.request_exit(ctx, "minimize button");
                    }
                    frontend::view_model::FrontendAction::WindowMaximize => {
                        let maximized =
                            ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    other => {
                        self.action_sequence += 1;
                        if let Err(error) = self.client.send(WindowToHost::Action {
                            sequence: self.action_sequence,
                            action: other,
                        }) {
                            log::warn!("[ui-client] cannot forward action to the host: {error}");
                        }
                    }
                }
            }
        }
    }

    impl Drop for UiClientApp {
        fn drop(&mut self) {
            // 关窗即退出：告别帧让宿主立刻作废窗口句柄，收尾读写线程
            // 以免宿主一直等到 EOF。
            let _ = self.client.send(WindowToHost::Bye);
            self.client.shutdown();
        }
    }

    impl eframe::App for UiClientApp {
        fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
            egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
        }

        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.drain_host(ctx);
            self.poll_local_hotkeys(ctx);
            theme::apply_visuals(ctx, self.view_model.theme_mode);
            if ctx.input(|input| input.viewport().close_requested()) {
                self.request_exit(ctx, "window manager close request");
            }
            if self.exited {
                return;
            }
            if ui_debug_enabled() {
                let (pointer, clicked) =
                    ctx.input(|input| (input.pointer.interact_pos(), input.pointer.any_click()));
                if clicked {
                    log::info!("[ui-client] pointer click at {pointer:?}");
                }
            }
            let mut actions = Vec::new();
            frontend::render(ctx, &mut self.view_model, &mut actions);
            if ui_debug_enabled() && !actions.is_empty() {
                log::info!("[ui-client] actions from the renderer: {actions:?}");
            }
            self.dispatch(actions, ctx);
            self.ping_if_due();
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    pub fn run() -> Result<(), String> {
        let args = std::env::args().collect::<Vec<_>>();
        if let Some(kind) = popup_kind(&args) {
            return run_popup_process(kind);
        }
        if let Some(socket) = ui_client_socket(&args) {
            // UI 窗口进程：只渲染，不碰后端、数据目录、单实例锁与热键。
            return run_ui_client(socket);
        }
        let start_minimized = args.iter().any(|arg| arg == "--minimized");
        let tokio = Arc::new(tokio::runtime::Runtime::new().map_err(|error| error.to_string())?);
        let kind = package_kind();
        let update_support = LinuxUpdateSupport::initialize(kind);
        let updater_available = update_support.supports_auto_update();
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CACHE_HOME")
                    .map(std::path::PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|home| std::path::PathBuf::from(home).join(".cache"))
                    })
                    .map(|cache| cache.join("OpenLess/runtime"))
            })
            .ok_or_else(|| "HOME/XDG_RUNTIME_DIR is unavailable".to_string())?;
        // 常态启动时托盘先于能力快照存在；形态切换（--takeover）时旧进程可能还
        // 占着托盘名，所以接管到单实例锁之后再重试一次。
        let mut tray = openless_linux_egui::LinuxTray::start().ok();
        let broker = match acquire_broker(&runtime_dir, &args)? {
            BrokerAcquisition::Primary(broker) => Some(broker),
            BrokerAcquisition::Forwarded => return Ok(()),
        };
        if tray.is_none() {
            tray = openless_linux_egui::LinuxTray::start().ok();
        }
        let tray_available = tray.is_some();
        let config = backend_config(tray_available, updater_available)?;
        if let Err(error) = openless_linux_egui::init_file_logger(&config.data_dir) {
            eprintln!("OpenLess file logger unavailable: {error}");
        }
        let native = (|| {
            // AppImage may need to materialize its bundled plugin into the
            // per-user fcitx5 search path. Do that before opening the DBus
            // listener: otherwise the first run can wait forever for signals
            // from a plugin fcitx5 has never loaded.
            // fcitx5 只是「全局热键」这一条能力：它缺席、插件过旧、DBus 不通
            // 都**不能**拖死后端与主窗口（否则用户看到的就是「跟后端完全没连上」）。
            if let Err(error) = ensure_fcitx5_ready(&config) {
                log::warn!("[fcitx] addon readiness check failed, continuing without it: {error}");
            }
            let hotkeys = match Fcitx5HotkeyListener::start() {
                Ok(listener) => Some(listener),
                Err(error) => {
                    // fcitx5 没在跑 / DBus 不通：热键暂时不可用，其余功能照常。
                    log::warn!("[fcitx] hotkey listener unavailable, continuing: {error}");
                    None
                }
            };
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
                .block_on(LinuxNativeRuntime::start(backend, broker, hotkeys))
                .map_err(|error| error.to_string())
        })();
        // 常驻宿主：本进程不再创建窗口，窗口交给独立的 UI 进程。
        run_host(
            &runtime_dir,
            tokio,
            native,
            tray,
            update_support,
            start_minimized,
        )?;
        Ok(())
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
        lang: Lang,
        label: &str,
        binding: &mut Option<openless_core::shared_types::ShortcutBinding>,
        default_primary: &str,
    ) -> bool {
        let mut enabled = binding.is_some();
        let mut changed = ui
            .checkbox(&mut enabled, fmt_l10n(lang, "hotkey.enable", &[&label]))
            .changed();
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_ui_client_flag_carries_the_bridge_socket() {
            let args = vec![
                "openless".to_string(),
                UI_CLIENT_FLAG.to_string(),
                UI_SOCKET_FLAG.to_string(),
                "/run/user/1000/openless-ui.sock".to_string(),
            ];
            assert_eq!(
                ui_client_socket(&args),
                Some(std::path::PathBuf::from("/run/user/1000/openless-ui.sock"))
            );
            // 普通启动（宿主）不是 UI 进程。
            assert_eq!(ui_client_socket(&["openless".to_string()]), None);
            // 带了开关却没给路径：当作普通启动，不要装作是 UI 进程。
            assert_eq!(ui_client_socket(&[UI_CLIENT_FLAG.to_string()]), None);
        }

        #[test]
        fn stale_snapshots_never_replace_newer_state() {
            // 严格递增才采纳：重复、乱序、回退的快照都必须丢掉，
            // 否则 UI 会把新状态画成旧状态。
            assert!(snapshot_supersedes(0, 1));
            assert!(snapshot_supersedes(7, 8));
            assert!(!snapshot_supersedes(7, 7), "duplicate must be dropped");
            assert!(!snapshot_supersedes(7, 3), "out-of-order must be dropped");
        }

        #[test]
        fn the_snapshot_fingerprint_notices_any_change() {
            let a = br#"{"active_page":"Overview"}"#.to_vec();
            let b = br#"{"active_page":"History"}"#.to_vec();
            assert_eq!(snapshot_fingerprint(&a), snapshot_fingerprint(&a));
            assert_ne!(snapshot_fingerprint(&a), snapshot_fingerprint(&b));
        }

        #[test]
        fn the_host_keeps_running_when_the_window_says_goodbye() {
            let mut app = fixture_app(true);
            app.apply_window_messages(vec![WindowToHost::Bye], true);
            // 关窗只关窗口：宿主不退出、后端与会话不动。
            assert!(!app.window_should_be_open);
            assert!(!app.exit_requested);
        }

        #[test]
        fn a_window_is_reopened_only_when_the_user_asks_for_it() {
            let mut app = fixture_app(false);
            // 用户已经关窗：宿主不会自己把窗口拉回来。
            app.pending_ui_actions
                .push(frontend::view_model::FrontendAction::Navigate(
                    frontend::view_model::Page::History,
                ));
            assert!(!app.should_spawn_ui_window());
            // 托盘「显示主窗口」是显式意图，必须重新拉起一个窗口进程。
            app.request_main_window();
            assert!(app.window_should_be_open);
            assert!(app.should_spawn_ui_window());
        }

        #[test]
        fn a_freshly_spawned_window_is_not_spawned_twice() {
            let mut app = fixture_app(false);
            app.request_main_window();
            // 模拟「刚拉起过」：防抖窗口内不得再拉第二个窗口进程。
            app.ui_window_spawned_at = Some(std::time::Instant::now());
            assert!(!app.should_spawn_ui_window());
            // 防抖过期且没有活着的子进程时，允许重拉。
            app.ui_window_spawned_at = Some(std::time::Instant::now() - Duration::from_secs(5));
            assert!(app.should_spawn_ui_window());
        }

        #[test]
        fn a_host_without_a_tray_exits_with_its_only_window() {
            // 没有托盘就没有重新打开的入口：窗口退出后宿主必须跟着退出，
            // 否则用户留下一个看得见进程、点不开窗口的僵尸。
            let mut app = fixture_app(false);
            app.apply_window_messages(vec![WindowToHost::Bye], false);
            assert!(app.exit_requested);
        }

        #[test]
        fn the_host_actions_that_used_to_raise_a_window_no_longer_do() {
            // Core 的 ShowMain/FocusMain 在弹窗流程里也会发，宿主若照做就会
            // 「弹一次面板冒出一个主窗口」，所以它们必须不改变窗口意图。
            let mut app = fixture_app(true);
            app.apply_window_messages(
                vec![WindowToHost::Action {
                    sequence: 1,
                    action: frontend::view_model::FrontendAction::WindowClose,
                }],
                true,
            );
            // 窗口控制由 UI 进程处理；即便漏到宿主，也只是入队后由
            // apply_frontend_actions 记一条日志，不改变窗口意图。
            assert!(app.window_should_be_open);
        }

        #[test]
        fn the_host_heartbeat_runs_without_any_window() {
            // tick() 不依赖 eframe 的帧循环：用一个没有窗口的 egui Context
            // 连续跑两次也不会 panic（热键消费/弹窗拉起就在这条路径上）。
            let ctx = egui::Context::default();
            let mut app = fixture_app(true);
            app.tick(&ctx);
            app.tick(&ctx);
        }

        fn fixture_app(window_should_be_open: bool) -> OpenLessEguiApp {
            OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture".into()),
                None,
                LinuxUpdateSupport::ManualOnly {
                    releases_url: openless_linux_egui::RELEASES_URL,
                },
                window_should_be_open,
            )
        }

        #[test]
        fn assistant_deltas_accumulate_into_one_entry_per_turn() {
            // 一个轮次里流式增量只应形成一条助手条目；工具标记之后的新增量属于
            // 新一轮正文，要另起一条（否则工具行会被并进正文里）。
            let mut entries = Vec::new();
            append_assistant_entry(&mut entries, "he");
            append_assistant_entry(&mut entries, "llo");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].kind, "assistant");
            assert_eq!(entries[0].text, "hello");
            entries.push(openless_linux_egui::LessComputerEntry {
                kind: "tool".to_string(),
                text: "Used bash".to_string(),
            });
            append_assistant_entry(&mut entries, "done");
            assert_eq!(entries.len(), 3);
            assert_eq!(entries[2].text, "done");
        }

        #[test]
        fn a_crashing_popup_stops_restarting_after_the_budget() {
            let start = std::time::Instant::now();
            let mut budget = PopupRestartBudget::default();
            for _ in 0..POPUP_RESTART_LIMIT {
                assert!(budget.allow(start), "预算内的重开必须放行");
            }
            assert!(
                !budget.allow(start + Duration::from_secs(1)),
                "超出预算后不再重开"
            );
            // 窗口过期后重新计数：偶尔崩一次的面板不该被永久关掉。
            assert!(budget.allow(start + POPUP_RESTART_WINDOW + Duration::from_secs(1)));
        }

        /// 最小可用的弹窗实例：只为了驱动 `pump` 这条退出链路。
        fn popup_app(kind: PopupKind, incoming: mpsc::Receiver<HostToPopup>) -> NativePopupApp {
            let (outgoing, _outgoing_rx) = mpsc::channel();
            NativePopupApp {
                kind,
                state: PopupState::default(),
                incoming,
                outgoing,
                qa_input: String::new(),
                less_computer_input: String::new(),
                outgoing_sequence: 0,
                ready_sent: false,
                avatar: QaAvatar::default(),
                lang: Lang::ZhCn,
                overlay: None,
                hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
            }
        }

        #[test]
        fn a_closed_host_pipe_exits_the_popup() {
            // 宿主进程崩溃/被杀时 stdin 到 EOF、发送端析构。以前 Empty 与
            // Disconnected 被一起当成「没有消息」，胶囊就会永久贴在屏幕上
            // （真机验证过：layer surface 不会自己消失，只能随进程销毁）。
            let (tx, rx) = mpsc::channel();
            let mut app = popup_app(PopupKind::Capsule, rx);
            drop(tx);
            assert!(app.pump(None), "a closed host pipe must end the popup");
        }

        #[test]
        fn a_live_host_pipe_keeps_the_popup_running() {
            let (tx, rx) = mpsc::channel();
            let mut app = popup_app(PopupKind::Capsule, rx);
            tx.send(HostToPopup::Capsule {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "s1".into(),
                sequence: 1,
                phase: "Recording".into(),
                text: String::new(),
                audio_level: Some(0.2),
                translation_active: false,
                style: "siri".into(),
            })
            .expect("channel is open");
            assert!(!app.pump(None), "a progress frame must not exit");
            // 宿主仍然活着（发送端还在）→ 不能因为消息读空就退出。
            assert!(!app.pump(None));
            drop(tx);
            assert!(app.pump(None), "losing the host must exit");
        }

        #[test]
        fn a_shutdown_frame_exits_the_popup() {
            let (tx, rx) = mpsc::channel();
            let mut app = popup_app(PopupKind::Capsule, rx);
            tx.send(HostToPopup::Shutdown {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "s1".into(),
                sequence: 2,
            })
            .expect("channel is open");
            assert!(app.pump(None), "the host shutdown must end the popup");
        }

        #[test]
        fn pinned_qa_ignores_the_automatic_hide_action() {
            assert!(qa_hides_on_host_action(false));
            assert!(!qa_hides_on_host_action(true));
        }

        #[test]
        fn qa_edit_flags_merge_partial_core_updates() {
            let mut flags = QaEditFlags::default();
            // Core 只在变化时下发 Some(..)：未下发的字段必须保持原值。
            let mut event = openless_core::QaStateEvent::simple(openless_core::QaStateKind::Idle);
            event.edit_apply_available = Some(true);
            flags.merge(&event);
            assert!(flags.apply_available);
            assert!(!flags.instruction_mode);
            assert!(!flags.revert_available);

            event.edit_instruction_mode = Some(true);
            event.edit_revert_available = Some(true);
            flags.merge(&event);
            assert!(flags.instruction_mode);
            assert!(flags.revert_available);
            assert!(flags.apply_available);

            event.edit_apply_available = Some(false);
            flags.merge(&event);
            assert!(!flags.apply_available);
            assert!(flags.revert_available);
        }

        #[test]
        fn continuation_turn_keeps_receiving_output_and_approval() {
            let mut app = OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture".into()),
                None,
                LinuxUpdateSupport::ManualOnly {
                    releases_url: openless_linux_egui::RELEASES_URL,
                },
                false,
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
                false,
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
            let latest = UserPreferences {
                remote_input_port: 9443,
                ..Default::default()
            };
            let mut draft = latest.clone();
            draft.hotkey.mode = openless_core::shared_types::HotkeyMode::Auto;
            draft.silence_auto_stop_enabled = true;
            draft.silence_auto_stop_seconds = 1.5;
            draft.mute_during_recording = true;
            draft.audio_cue_on_record = false;
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
            assert!(merged.mute_during_recording);
            assert!(!merged.audio_cue_on_record);
            assert_eq!(merged.microphone_device_name, "USB microphone");
            assert_eq!(
                merged.theme_mode,
                openless_core::shared_types::ThemeMode::Dark
            );
            assert!(!merged.show_overview_activity_heatmap);
            assert_eq!(merged.remote_input_port, 9443);
        }

        #[test]
        fn ready_install_needs_no_reload_but_continues() {
            assert!(reconcile_fcitx5_install(FcitxPluginStatus::Ready).is_ok());
        }

        /// 插件缺失只影响全局热键，**绝不能** 让启动失败 —— 早先这里返回 Err 并
        /// 用 `?` 中断启动，用户看到的就是「主窗口不显示」。
        #[test]
        fn missing_install_still_starts_the_window() {
            assert!(
                reconcile_fcitx5_install(FcitxPluginStatus::Missing).is_ok(),
                "a missing fcitx5 addon must never abort startup"
            );
        }

        // ---- Overview summary (Tauri parity) -----------------------------

        fn session_entry(
            created_at: &str,
            final_text: &str,
            duration_ms: Option<u64>,
        ) -> openless_core::DictationSession {
            openless_core::DictationSession {
                id: String::new(),
                created_at: created_at.to_string(),
                source: openless_core::HistorySource::Voice,
                raw_transcript: String::new(),
                asr_transcript: None,
                final_text: final_text.to_string(),
                mode: openless_core::PolishMode::Raw,
                style_pack_id: None,
                translation_active: false,
                polish_source: None,
                app_bundle_id: None,
                app_name: None,
                insert_status: openless_core::HistoryInsertStatus::Inserted,
                error_code: None,
                duration_ms,
                dictionary_entry_count: None,
                has_audio_recording: None,
                asr_provider: None,
                asr_model: None,
                llm_provider: None,
                llm_model: None,
                pipeline_mode: None,
                asr_ms: None,
                polish_ms: None,
            }
        }

        fn activity_day(date: &str, count: u32) -> openless_core::ActivityDay {
            openless_core::ActivityDay {
                date: date.to_string(),
                count,
                chars: 0,
                duration_ms: 0,
            }
        }

        #[test]
        fn overview_metrics_aggregate_only_today_from_history() {
            let now = chrono::Local::now();
            let today = now.date_naive();
            let history = vec![
                session_entry(&now.to_rfc3339(), "今天第一句", Some(2000)),
                session_entry(
                    &(now - chrono::Duration::days(1)).to_rfc3339(),
                    "昨天",
                    Some(999),
                ),
                session_entry(
                    &(now - chrono::Duration::days(2)).to_rfc3339(),
                    "前天",
                    None,
                ),
            ];
            let credentials = openless_core::CredentialsStatus {
                active_asr_provider: "volcengine".to_string(),
                active_llm_provider: "ark".to_string(),
                asr_configured: true,
                ..Default::default()
            };

            let summary = overview_summary(
                &OverviewData {
                    credentials,
                    history,
                    activity: Vec::new(),
                },
                today,
            );

            assert_eq!(summary.segments_today, 1, "only today's entry counts");
            assert_eq!(summary.chars_today, 5, "今日第一句 has 5 chars");
            assert_eq!(summary.duration_ms_today, 2000);
            assert_eq!(summary.avg_latency_ms, 2000);
            assert_eq!(summary.history_total, 3);
            assert_eq!(summary.asr_provider, "volcengine");
            assert!(summary.asr_configured);
            assert!(!summary.llm_configured);
            assert_eq!(summary.recent.len(), 3, "newest three retained");
            assert_eq!(
                summary.recent[0].final_text, "今天第一句",
                "recent list is newest-first"
            );
            assert_eq!(summary.recent[0].duration_ms, Some(2000));
        }

        #[test]
        fn overview_activity_windows_and_heatmap_are_windowed_by_date() {
            let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
            let credentials = openless_core::CredentialsStatus::default();
            let activity = vec![
                activity_day("2026-01-15", 5),
                activity_day("2026-01-08", 2),
                activity_day("2026-01-01", 3),
                activity_day("2025-06-01", 9),
            ];

            let summary = overview_summary(
                &OverviewData {
                    credentials,
                    history: Vec::new(),
                    activity,
                },
                today,
            );

            // Last-7 window covers only Jan 15.
            assert_eq!(summary.last_7.active_days, 1);
            assert_eq!(summary.last_7.segments, 5);
            // Last-30 window covers Jan 15, Jan 8 and Jan 1.
            assert_eq!(summary.last_30.active_days, 3);
            assert_eq!(summary.last_30.segments, 10);

            // The daily series is the trailing 30 days ending today.
            assert_eq!(summary.activity_daily.len(), 30);
            assert_eq!(summary.activity_daily.last().unwrap().date, "2026-01-15");
            assert_eq!(summary.activity_daily.last().unwrap().count, 5);

            // The heatmap now covers the whole calendar year (Jan 1 – Dec 31),
            // so every 2026 day counts and the 2025 day drops out.
            assert_eq!(summary.heatmap_year, 2026);
            assert_eq!(summary.heatmap.len(), 365);
            let heat_total: u32 = summary.heatmap.iter().map(|day| day.count).sum();
            assert_eq!(heat_total, 10);
        }

        #[test]
        fn overview_heatmap_excludes_days_outside_the_calendar_year() {
            let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
            let far = (today - chrono::Duration::days(400))
                .format("%Y-%m-%d")
                .to_string();
            let summary = overview_summary(
                &OverviewData {
                    credentials: openless_core::CredentialsStatus::default(),
                    history: Vec::new(),
                    activity: vec![activity_day("2026-01-15", 3), activity_day(&far, 7)],
                },
                today,
            );

            let heat_total: u32 = summary.heatmap.iter().map(|day| day.count).sum();
            assert_eq!(
                heat_total, 3,
                "days outside the calendar year must not appear in the heatmap"
            );
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
