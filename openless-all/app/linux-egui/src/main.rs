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
        capsule_hide_delay, capsule_hide_is_still_current, capsule_outcome, fmt_l10n,
        load_locale_pref, normalize_stop_result, phase_shows_capsule, save_locale_pref, tr_l10n,
        CapsuleOutcome, Lang, LocalePref,
    };
    use openless_linux_egui::{
        drain_events, ensure_fcitx5_plugin_installed, fcitx5_copy_to_clipboard, notify,
        open_external, write_jsonl, EventDrainOutcome, Fcitx5HotkeyListener,
        FcitxPluginInstallPlan, FcitxPluginStatus, HostToPopup, LinuxBackendBuilder,
        LinuxCapabilitySnapshot, LinuxLaunchIntent, LinuxNativeRuntime, LinuxPackageKind,
        LinuxResourceLayout, LinuxUpdateSupport, Notification, PopupActionGuard, PopupChatMessage,
        PopupKind, PopupState, PopupSupervisor, PopupSupervisorEvent, PopupToHost,
        SingleInstanceBroker, SingleInstanceRole, UpdateManifest, UpdateSchedule,
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
        Marketplace(Result<Vec<openless_core::MarketplaceListItem>, String>),
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
        transcript: String,
        transcript_state: TranscriptAccumulator,
        transcript_session: Option<openless_core::SessionId>,
        recording_phase_active: bool,
        last_event_sequence: u64,
        less_computer_input: String,
        less_computer_output: String,
        less_computer_turn_start: usize,
        less_computer_session: Option<openless_core::SessionId>,
        pending_approval: Option<(String, String)>,
        qa_visible: bool,
        /// 划词追问的图钉：固定后 `HostAction::HideQa` 不再收起窗口。
        qa_pinned: bool,
        /// 追问「编辑指令」三态（Core `QaStateEvent` 的部分更新）。
        qa_edit: QaEditFlags,
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
                        transcript: String::new(),
                        transcript_state: TranscriptAccumulator::default(),
                        transcript_session: None,
                        recording_phase_active: false,
                        last_event_sequence: 0,
                        less_computer_input: String::new(),
                        less_computer_output: String::new(),
                        less_computer_turn_start: 0,
                        less_computer_session: None,
                        pending_approval: None,
                        qa_visible: false,
                        qa_pinned: false,
                        qa_edit: QaEditFlags::default(),
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
                        history_clip: None,
                        marketplace_items: Vec::new(),
                        marketplace_attempted: false,
                        marketplace_query: String::new(),
                        marketplace_flow: None,
                        marketplace_detail: None,
                        marketplace_my_packs: Vec::new(),
                        marketplace_my_likes: Vec::new(),
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
                    transcript: String::new(),
                    transcript_state: TranscriptAccumulator::default(),
                    transcript_session: None,
                    recording_phase_active: false,
                    last_event_sequence: 0,
                    less_computer_input: String::new(),
                    less_computer_output: String::new(),
                    less_computer_turn_start: 0,
                    less_computer_session: None,
                    pending_approval: None,
                    qa_visible: false,
                    qa_pinned: false,
                    qa_edit: QaEditFlags::default(),
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
                    history_clip: None,
                    marketplace_items: Vec::new(),
                    marketplace_attempted: false,
                    marketplace_query: String::new(),
                    marketplace_flow: None,
                    marketplace_detail: None,
                    marketplace_my_packs: Vec::new(),
                    marketplace_my_likes: Vec::new(),
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
                PopupKind::Preview => &mut self.preview_popup,
                PopupKind::Capsule => &mut self.capsule_popup,
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
                },
            );
            self.schedule_capsule_dismissal(&session_id.to_string(), snapshot.phase);
        }

        /// 终态后按 Tauri Host 的时序自动收起胶囊：成功/失败停留 2 秒、
        /// 取消立刻；进行中的相位不收。
        fn schedule_capsule_dismissal(&self, session_id: &str, phase: DictationPhase) {
            let Some(delay) = capsule_hide_delay(phase) else {
                return;
            };
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
            if let Some(supervisor) = self.popup_slot(PopupKind::Capsule).as_ref() {
                let _ = supervisor.request_shutdown();
            }
            *self.popup_slot(PopupKind::Capsule) = None;
        }

        fn poll_popup_supervisors(&mut self) {
            let lang = self.lang;
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
                                    Ok(tr_l10n(lang, "selection.replaced").to_string())
                                });
                            }
                        }
                        Err(error) => {
                            self.status = fmt_l10n(lang, "popup.session_invalid", &[&error])
                        }
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
                                    Ok(tr_l10n(lang, "selection.cancelled").to_string())
                                });
                            }
                        }
                        Err(error) => {
                            self.status = fmt_l10n(lang, "popup.session_invalid", &[&error])
                        }
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
                    PopupSupervisorEvent::Message(
                        PopupToHost::SubmitQa { .. }
                        | PopupToHost::ToggleQaRecording { .. }
                        | PopupToHost::DismissQa { .. }
                        | PopupToHost::SetPinned { .. }
                        | PopupToHost::SetEditInstructionMode { .. }
                        | PopupToHost::ApplyEdit { .. }
                        | PopupToHost::RevertEdit { .. },
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
                let likes = backend
                    .services()
                    .marketplace
                    .my_likes()
                    .await
                    .map_err(|error| error.to_string());
                let result = backend
                    .services()
                    .marketplace
                    .list(openless_core::MarketplaceQuery {
                        query: (!query.is_empty()).then_some(query),
                        sort: Some(sort.to_string()),
                        limit: Some(100),
                    })
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::MarketplaceLikes(likes));
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

        fn drain_tray(&mut self, ctx: &egui::Context) {
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
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
                    self.status = fmt_l10n(
                        lang,
                        "status.dictation_phase",
                        &[&format!("{:?}", state.phase)],
                    );
                    if let Some(session_id) = state.session_id {
                        // 上一轮胶囊被自动收起后进程已经不在了：进行中的相位必须按需
                        // 重新拉起，否则 send_popup 会因为没有 supervisor 而静默丢弃；
                        // 终态则不拉，免得把刚收起的药丸又喊回来。
                        if phase_shows_capsule(state.phase) {
                            self.ensure_popup(PopupKind::Capsule);
                        }
                        // 进行中的 message 也要过一遍分类：Core 偶尔把内部错误码
                        // 写在这里，不能当成文案直接显示。
                        let text = match capsule_outcome(state.phase, state.message.as_deref()) {
                            CapsuleOutcome::Progress(text) => text,
                            _ => String::new(),
                        };
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
                    } else if session_id != self.less_computer_session {
                        return;
                    }
                    match event.kind {
                        // Linux已有独立录音显示；新typed反馈供接手Host/UI团队继续接入。
                        LessComputerEventKind::VoiceState { .. } => {}
                        LessComputerEventKind::User { .. } => {}
                        LessComputerEventKind::Started => {
                            self.status = tr_l10n(lang, "status.less_running").to_string();
                        }
                        LessComputerEventKind::Delta { text } => {
                            self.less_computer_output.push_str(&text);
                        }
                        LessComputerEventKind::Tool { name } => {
                            self.status = fmt_l10n(lang, "status.less_tool", &[&name]);
                        }
                        LessComputerEventKind::Compaction => {
                            self.status = tr_l10n(lang, "status.less_compacted").to_string();
                        }
                        LessComputerEventKind::Completed { text, .. } => {
                            // A terminal is authoritative even for final-only
                            // providers or after a missed partial event.
                            self.less_computer_output
                                .truncate(self.less_computer_turn_start);
                            self.less_computer_output.push_str(&text);
                            self.pending_approval = None;
                            self.status = tr_l10n(lang, "less_computer.done").to_string();
                        }
                        LessComputerEventKind::Approval { token, command, .. } => {
                            self.pending_approval = Some((token, command));
                            self.status = tr_l10n(lang, "status.less_waiting").to_string();
                        }
                        LessComputerEventKind::Error { message } => {
                            self.pending_approval = None;
                            self.status = message;
                        }
                        LessComputerEventKind::Cancelled => {
                            self.pending_approval = None;
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
            let lang = self.lang;
            if let Some(native) = &self.native {
                let (launch_intents, hotkey_events, errors) = native.drain_native_events();
                let host = native.host_arc();
                for intent in launch_intents {
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_launch_intent(intent).await?;
                        Ok(tr_l10n(lang, "status.launch_handled").to_string())
                    });
                }
                for event in hotkey_events {
                    let host = Arc::clone(&host);
                    self.spawn(async move {
                        host.dispatch_hotkey_event(event).await?;
                        Ok(tr_l10n(lang, "status.hotkey_handled").to_string())
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
                            self.status = tr_l10n(lang, "status.request_restart").to_string();
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
                                    self.status = fmt_l10n(
                                        lang,
                                        "status.provider_models_loaded",
                                        &[&models.len()],
                                    );
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
                    UiResult::Marketplace(Ok(items)) => {
                        self.status = fmt_l10n(lang, "status.marketplace_loaded", &[&items.len()]);
                        self.marketplace_items = items;
                        self.marketplace_attempted = true;
                    }
                    UiResult::Marketplace(Err(error)) => {
                        self.status = error;
                        self.marketplace_attempted = true;
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
                    vm.remote_pin = pin.clone();
                    vm.remote_urls = status.urls.clone();
                    vm.remote_cert_fingerprint = status.ca_fingerprint_sha256.clone();
                } else {
                    vm.remote_running = false;
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
            ctx: &egui::Context,
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
                    frontend::view_model::FrontendAction::WindowClose => {
                        // Closing the window is an explicit quit: without this the
                        // tray handler below would only hide it, which reads as a
                        // dead close button.
                        self.exit_requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    frontend::view_model::FrontendAction::WindowMaximize => {
                        let maximized =
                            ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    frontend::view_model::FrontendAction::WindowMinimize => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
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
                        self.load_marketplace();
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

    impl eframe::App for OpenLessEguiApp {
        fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
            egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
        }

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
                let lang = self.lang;
                if let Some(backend) = self.backend() {
                    self.spawn(async move {
                        backend.cancel_active_voice_session(None).await?;
                        Ok(tr_l10n(lang, "voice.cancelled").to_string())
                    });
                }
            }

            // Build the view model from current backend state, then render the
            // production frontend. Actions are collected and dispatched to
            // existing Core / backend methods.
            self.sync_view_model();
            let mut actions = Vec::new();
            frontend::render(ctx, &mut self.frontend_vm, &mut actions);
            self.apply_frontend_actions(actions, ctx);

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
        // 安装包升级会替换 libopenless.so，但运行中的 fcitx5 仍持有旧映像 ——
        // 不重启它，新的热键匹配规则就不会生效。只在插件确实更新过时重启。
        openless_linux_egui::reload_fcitx5_if_plugin_updated(&plan);
        reconcile_fcitx5_install(status)
    }

    /// Map an fcitx5 addon install result onto startup.
    ///
    /// A ready addon lets startup continue down the normal fcitx5 DBus path —
    /// never a global-hotkey fallback — and only a genuinely missing plugin
    /// aborts startup.
    fn reconcile_fcitx5_install(status: FcitxPluginStatus) -> Result<(), String> {
        match status {
            FcitxPluginStatus::Ready => Ok(()),
            FcitxPluginStatus::Missing => {
                Err("未找到 OpenLess fcitx5 插件；请重新安装当前软件包".to_string())
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
        outgoing_sequence: u64,
        ready_sent: bool,
        preview_focus_requested: bool,
        avatar: QaAvatar,
        lang: Lang,
        /// X11 overlay placement for the capsule (bottom-centre, never focus).
        overlay: Option<PopupOverlay>,
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

    impl NativePopupApp {
        /// Drain every host message queued since the last frame. `ctx` is
        /// `None` on the layer-shell path, which has no viewport to command:
        /// visibility and shutdown are handled by the runner instead.
        ///
        /// Returns true when the process should exit.
        fn pump(&mut self, ctx: Option<&egui::Context>) -> bool {
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
            false
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
                PopupKind::Preview => {
                    let first_frame = !self.preview_focus_requested;
                    let action = frontend::popups::selection_preview(
                        ctx,
                        &mut self.state.preview,
                        first_frame,
                        lang,
                    );
                    if first_frame {
                        self.preview_focus_requested = true;
                    }
                    match action {
                        frontend::popups::PreviewAction::Cancel => self.dismiss(ctx),
                        frontend::popups::PreviewAction::Confirm(text) => {
                            if let Some(session_id) = self.session_id() {
                                let sequence = self.next_sequence();
                                self.send(PopupToHost::ConfirmPreview {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id,
                                    sequence,
                                    text,
                                });
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        frontend::popups::PreviewAction::None => {}
                    }
                }
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
        } else if args.iter().any(|arg| arg == "--preview") {
            Some(PopupKind::Preview)
        } else if args.iter().any(|arg| arg == "--capsule") {
            Some(PopupKind::Capsule)
        } else {
            None
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
                            outgoing_sequence: 0,
                            ready_sent: false,
                            preview_focus_requested: false,
                            avatar: QaAvatar::default(),
                            lang: load_locale_pref().resolve(),
                            overlay: None,
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
            PopupKind::Qa => [520.0, 520.0],
            PopupKind::Preview => [
                openless_linux_egui::PREVIEW_WINDOW_SIZE.0 as f32,
                openless_linux_egui::PREVIEW_WINDOW_SIZE.1 as f32,
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
                    outgoing_sequence: 0,
                    ready_sent: false,
                    preview_focus_requested: false,
                    avatar: QaAvatar::default(),
                    // The popup is a separate process, so it re-reads the
                    // persisted UI-locale preference rather than sharing state.
                    lang: load_locale_pref().resolve(),
                    overlay,
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
                .with_transparent(true)
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

        #[test]
        fn missing_install_aborts_without_reloading() {
            let error = reconcile_fcitx5_install(FcitxPluginStatus::Missing)
                .expect_err("a missing plugin must abort startup");
            assert!(
                error.contains("OpenLess fcitx5 插件"),
                "unexpected Missing message: {error}"
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
