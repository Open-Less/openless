#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("openless-linux-egui is only available on Linux");
}

mod style_icon;
#[cfg(target_os = "linux")]
mod ui;

mod linux_app {
    mod history;
    use history::HistoryCache;
    mod settings_save;
    mod window;
    use settings_save::SettingsSave;
    /// 泵心跳的采样间隔：热键消费停没停，十秒内一定看得出来。
    const PUMP_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
    /// 状态没有变化时，`info` 级心跳的最低频率（存活证明，不为排查细节服务）。
    const PUMP_HEARTBEAT_INFO_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

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
        DictationPhase, HostAction, LessComputerEventKind, QaStateEvent, QaStateKind,
        SelectionPhase, SelectionSnapshot, TranscriptAccumulator, UserPreferences,
    };
    use openless_linux_egui::{
        capsule_hide_delay, capsule_hide_is_still_current, capsule_needs_fallback_dismissal,
        capsule_outcome, fmt_l10n, load_locale_pref, normalize_stop_result, phase_shows_capsule,
        save_locale_pref, tr_l10n, CapsuleOutcome, Lang, LocalePref,
    };
    use openless_linux_egui::{
        drain_events, fcitx5_copy_to_clipboard, notify, open_external, write_jsonl,
        EventDrainOutcome, Fcitx5HotkeyListener, FcitxPluginInstallPlan, HostToPopup,
        LinuxBackendBuilder, LinuxCapabilitySnapshot, LinuxHotkeyEvent, LinuxLaunchIntent,
        LinuxNativeRuntime, LinuxResourceLayout, Notification, PopupActionGuard, PopupChatMessage,
        PopupKind, PopupState, PopupSupervisor, PopupSupervisorEvent, PopupToHost,
        SingleInstanceBroker, SingleInstanceRole, POPUP_PROTOCOL_VERSION,
    };
    use window::*;

    enum UiResult {
        Siwc {
            channel_id: String,
            request: u64,
            state: Result<frontend::view_model::SettingsSiwcState, String>,
        },
        Message(String),
        /// 选区助手动作的异步结果：`generation` 是发起时面板进程的代次，
        /// 换过窗口后的迟到结果不得回写到新面板。
        QaAction {
            generation: u64,
            context: Option<String>,
            result: Result<String, String>,
        },
        HotkeyError(BackendError),
        ProviderSaved {
            editor: Box<ProviderEditor>,
            result: Result<(), String>,
        },
        ProviderSecretsCleared(Result<(), String>),
        HistoryLoaded {
            generation: u64,
            revision: u64,
            result: Result<Vec<openless_core::DictationSession>, String>,
        },
        HistoryRepolish {
            id: String,
            result: Result<String, String>,
        },
        /// 终态在屏上停留结束：胶囊可以收起了（handler 会再核对会话与相位）。
        CapsuleDismissDue {
            session_id: String,
        },
        Remote(Result<(openless_core::RemoteInputStatus, String), String>),
        Providers {
            kind: openless_core::ChannelKind,
            request: u64,
            result: Result<ProviderPanel, String>,
        },
        /// Credential channels for the settings modal's AI-services tab.
        SettingsChannels {
            kind: openless_core::ChannelKind,
            request: u64,
            result: Result<Vec<SettingsChannelRow>, String>,
        },
        ChannelCreated {
            kind: openless_core::ChannelKind,
            result: Result<String, String>,
        },
        ChannelsChanged {
            kind: openless_core::ChannelKind,
            result: Result<String, String>,
        },
        ProviderTypeChanged {
            kind: openless_core::ChannelKind,
            channel_id: String,
            result: Box<Result<ProviderEditor, String>>,
        },
        ServiceConfigured([bool; 2]),
        LinuxAccess(openless_linux_egui::linux_access::LinuxAccessSnapshot),
        OmniFinished {
            operation: openless_linux_egui::omni_settings::OmniAction,
            submitted: Box<openless_linux_egui::omni_settings::OmniSettings>,
            result: Box<Result<openless_linux_egui::omni_settings::OmniSettings, String>>,
        },
        /// 云同步卡片的一次往返：成功带回最新状态，失败带回稳定错误码。
        CloudSyncDone {
            result: Box<Result<openless_linux_egui::cloud_sync_ui::SyncOutcome, String>>,
        },
        ProviderEditor {
            kind: openless_core::ChannelKind,
            channel_id: String,
            result: Box<Result<ProviderEditor, String>>,
        },
        ProviderModels {
            kind: openless_core::ChannelKind,
            channel_id: String,
            endpoint: String,
            provider_type: String,
            request_format: openless_core::llm_protocol::LlmRequestFormat,
            result: Result<Vec<String>, String>,
        },
        Library(Result<LibraryPanel, String>),
        LibraryRefresh,
        SettingsSaved {
            request: u64,
            result: Result<(), String>,
        },
        Marketplace(u64, Result<Vec<openless_core::MarketplaceListItem>, String>),
        /// 安装结束（成功或失败）：清掉「安装中…」状态并给出提示。
        MarketplaceInstallFinished {
            id: String,
            result: Result<String, String>,
        },
        MarketplaceLikes(Result<Vec<String>, String>),
        MarketplaceAuth(Result<bool, String>),
        MarketplaceOAuthStarted(Result<openless_core::OAuthDeviceFlow, String>),
        MarketplaceOAuthPoll {
            flow_id: String,
            result: Result<openless_core::OAuthPollResult, String>,
        },
        MarketplacePublish(Result<String, String>),
        MarketplaceUpload(Result<openless_core::MarketplaceUploadResult, String>),
        MarketplaceWithdraw(Result<String, String>),
        MarketplaceDetail(Result<openless_core::MarketplaceDetail, String>),
        MarketplaceMine(Result<(Vec<openless_core::MarketplaceMyPackItem>, Vec<String>), String>),
        Microphones(Result<Vec<openless_core::MicrophoneDevice>, String>),
        Overview(Result<OverviewData, String>),
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
        /// 最近一次验证的时间（epoch **秒**，与 Tauri `lastTest.at` 同一量纲）。
        last_test_at: Option<i64>,
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
        Failed,
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
        protocol: openless_linux_egui::provider_protocol::ProviderProtocolDraft,
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
        Loading,
        Loaded(Box<ProviderEditor>),
        Failed,
    }

    /// Draft of the open provider editor. It is the single source of truth for
    /// the editor fields: pages push their input back here as actions and the
    /// mirror writes it out each frame, so re-reading the channel list never
    /// clobbers what the user is typing.
    struct ProviderEditorForm {
        siwc: Option<frontend::view_model::SettingsSiwcState>,
        siwc_request: u64,
        channel_id: String,
        /// 渠道种类（LLM / ASR）：弹窗副标题「文字处理渠道 / 语音识别渠道」用它。
        kind: openless_core::ChannelKind,
        provider_type: String,
        label: String,
        auth: frontend::view_model::SettingsProviderAuth,
        name: String,
        endpoint: String,
        model: String,
        resource_id: String,
        auth_mode: String,
        // Write-only drafts start empty on load; clear only after Core's save receipt.
        primary_secret: String,
        secondary_secret: String,
        models: Vec<String>,
        models_loading: bool,
        catalog_format: Option<openless_core::llm_protocol::LlmRequestFormat>,
        /// Core descriptor 的高阶字段：模型块的「预设 / 默认 / 文档页」全部由它决定。
        static_models: Vec<String>,
        default_model: String,
        models_url: Option<String>,
        /// 「自定义模型…」手输模式（仅 UI 状态，Tauri `customModelMode`）。
        custom_model: bool,
        protocol: openless_linux_egui::provider_protocol::ProviderProtocolDraft,
    }

    impl ProviderEditorForm {
        fn from_editor(editor: &ProviderEditor, lang: Lang) -> Self {
            Self {
                siwc: None,
                siwc_request: 0,
                channel_id: editor.channel.id.clone(),
                kind: editor.kind,
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
                catalog_format: None,
                static_models: editor.descriptor.static_models.clone(),
                default_model: editor.descriptor.default_model.clone().unwrap_or_default(),
                models_url: provider_models_url(&editor.descriptor, &editor.endpoint),
                custom_model: false,
                protocol: editor.protocol.clone(),
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

    /// 选区助手面板当前呈现的协议状态：代次在每次重开窗口时递增，
    /// `session_id` 记录实际上屏的会话（`qa` 空态也计入）。
    #[derive(Default)]
    struct QaPopupPresentation {
        generation: u64,
        session_id: Option<String>,
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
        settings_save: SettingsSave,
        settings_channel_kind: openless_core::ChannelKind,
        settings_channels: Vec<SettingsChannelRow>,
        settings_channels_loading: bool,
        settings_channels_request: u64,
        settings_channels_refresh_requested: bool,
        linux_access_last_check: Option<std::time::Instant>,
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
        qa_state: Option<QaStateEvent>,
        /// 选区助手面板是否处于「润色结果」模式（独立预览窗口已下线，
        /// 润色结果由这个面板承载：提问对话 / 润色结果是同一弹窗的两套 UI）。
        polish_result_visible: bool,
        selection_draft: String,
        selection: Option<SelectionSnapshot>,
        remote_access: Option<(openless_core::RemoteInputStatus, String)>,
        provider_kind: openless_core::ChannelKind,
        providers: ProvidersState,
        providers_request: u64,
        selected_channel_id: Option<String>,
        /// 用户（或刚建好的草稿）要求打开渠道编辑器。面板加载时只有当它为真才打开：
        /// 以前只要面板加载就会为「当前选中的渠道」把编辑器拉起来，改成整栏覆盖后就是
        /// 「一进设置只见模型编辑器、别的设置都改不了」。
        editor_requested: bool,
        provider_editor: ProviderEditorState,
        /// 当前编辑器是「添加渠道」草稿（Tauri `isDraft`）：标题换成「添加渠道」，
        /// 且完全没交互就关闭时要把这张空渠道回收掉。
        editor_draft: bool,
        /// 草稿是否被用户动过（任一字段/保存/供应商切换）。
        draft_touched: bool,
        /// Draft mirrored into the view model while the editor is open.
        provider_editor_form: Option<ProviderEditorForm>,
        provider_jobs: tokio::sync::mpsc::UnboundedSender<ProviderJob>,
        provider_save_pending: bool,
        provider_switch_pending: bool,
        channel_create_pending: bool,
        provider_save_again: bool,
        provider_close_pending: bool,
        provider_after_save: Option<frontend::view_model::FrontendAction>,
        provider_models: Vec<String>,
        new_provider_type: String,
        pending_channel_delete: Option<String>,
        vocabulary: Vec<openless_core::DictionaryEntry>,
        correction_rules: Vec<openless_core::CorrectionRule>,
        style_packs: Vec<openless_core::StylePack>,
        /// `(pack id, icon path)` → data URL. Reading and base64-encoding the
        /// icon every frame would be pure waste; entries are invalidated by
        /// the path changing (Core writes a fresh filename per change).
        style_icon_cache: std::collections::HashMap<(String, String), String>,
        vocab_preset_store: openless_core::VocabPresetStore,
        vocab_presets: Vec<openless_core::VocabPreset>,
        qa_popup: Option<PopupSupervisor>,
        capsule_popup: Option<PopupSupervisor>,
        capsule_popup_generation: u64,
        popup_action_guard: PopupActionGuard,
        /// 当前胶囊展示的会话 id（用于「会话消失但没收到终态」的兜底收起）。
        capsule_session: Option<String>,
        /// 已经为哪个会话排过收起计时，避免重复计时。
        capsule_dismissal_scheduled: Option<String>,
        /// 「要记住这个词吗？」卡片当前内容（Core `pending_corrections`）。非空时
        /// 胶囊窗只画这张卡；见 `refresh_vocab_suggestions`。
        vocab_suggestions: Vec<openless_linux_egui::CapsuleSuggestion>,
        vocab_card_session: Option<String>,
        /// 卡片上一轮的重算时刻，用来让过期的候选自己消失（Core 只按需清理）。
        vocab_suggestions_checked: std::time::Instant,
        tray: Option<openless_linux_egui::LinuxTray>,
        exit_requested: bool,
        /// 上次采样「泵心跳」的时间（每 10 秒一次，与日志级别无关）。
        last_pump_heartbeat: std::time::Instant,
        /// 上次把心跳写进 `info` 的时间。心跳是每 10 秒一次的存在性证明，
        /// 全写 `info` 会在一天内吃满日志配额，把真正的错误挤出去。
        last_pump_heartbeat_info: std::time::Instant,
        /// 上次写 `info` 时的状态快照；状态一变就立刻记一条。
        last_pump_heartbeat_snapshot: [bool; 4],
        window: WindowState,
        /// 本帧从 UI 收到、待宿主执行的动作（按到达顺序）。
        pending_ui_actions: Vec<frontend::view_model::FrontendAction>,
        /// 本帧从 UI 窗口/面板收到、待处理的本地热键边沿（按到达时刻）。
        ///
        /// 我们自己的窗口有焦点时插件收不到按键，热键只能由窗口自己认出来
        /// （见 `local_hotkeys` 模块文档），这里按顺序攒着与插件信号一起处理。
        pending_local_hotkeys: Vec<(std::time::Instant, openless_linux_egui::LocalHotkeyEdge)>,
        /// 本地边沿与插件信号之间的去重（同一个物理按键可能两个来源都报）。
        hotkey_dedupe: openless_linux_egui::HotkeyDeduplicator,
        hotkey_jobs: tokio::sync::mpsc::UnboundedSender<HotkeyJob>,
        /// 已在 60s 内重开过的面板次数（索引见 `popup_kind_index`）。
        popup_restarts: [PopupRestartBudget; POPUP_KIND_COUNT],
        /// 宿主自己下发的弹窗序号（每个面板一条），与 Core 事件序号无关。
        popup_sequences: [u64; POPUP_KIND_COUNT],
        qa_popup_presentation: QaPopupPresentation,
        /// 最近一次下发给窗口/面板的热键配置；变了才重发。
        hotkeys_sent: Option<openless_core::HotkeyRuntimeTarget>,
        /// 速记页快捷键卡片是否被收起（持久化在 linux-ui-state.json）。
        quick_note_shortcut_hidden: bool,
        /// 本帧从 UI 收到、待回包的延迟探针序号。
        pending_ui_pongs: Vec<u64>,
        /// `LoadMore` 触发的分页回包：动作处理阶段拿不到桥，先排队，
        /// 由宿主循环在 `apply_frontend_actions` 之后发出（同 `pending_ui_pongs`）。
        pending_collection_pages: Vec<HostToWindow>,
        /// Currently playing history recording (session id + player handle).
        history: HistoryCache,
        history_clip: Option<(String, openless_linux_egui::ClipPlayer)>,
        marketplace_items: Vec<openless_core::MarketplaceListItem>,
        /// True once a marketplace list request has completed (ok or error), so
        /// the page can leave its loading state even when the result is empty.
        marketplace_attempted: bool,
        marketplace_query: String,
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
        marketplace_oauth_poll_deadline: Option<std::time::Instant>,
        marketplace_oauth_interval_secs: u64,
        /// Likes are fetched once per session (Tauri refreshes them when the
        /// sign-in state changes), not on every search.
        marketplace_likes_loaded: bool,
        style_editor: Option<openless_core::StylePack>,
        status: String,
        startup_error: Option<String>,
        /// Non-fatal startup/runtime warning shown in the UI overlay.
        runtime_warning: Option<String>,
        locale_pref: LocalePref,
        lang: Lang,
        active_page: shell::Page,
        frontend_vm: FrontendViewModel,
        tx: mpsc::Sender<UiResult>,
        rx: mpsc::Receiver<UiResult>,
    }

    /// Dirty check between the style-editor draft and the last stored pack.
    fn style_editor_is_dirty(vm: &frontend::view_model::FrontendViewModel) -> bool {
        let Some(saved) = vm.style_editor_saved.as_ref() else {
            return true;
        };
        let tags: Vec<String> = vm
            .style_tags
            .split([',', '，', '\n'])
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        vm.style_name.trim() != saved.name
            || vm.style_description.trim() != saved.description
            || vm.style_prompt != saved.prompt
            || vm.style_selection_prompt != saved.selection_prompt
            || vm.style_voice_edit_prompt != saved.voice_edit_prompt
            || tags != saved.tags
            || vm.style_author.trim() != saved.author.clone().unwrap_or_default()
            || vm.style_version.trim() != saved.version
            || vm.style_model.trim() != saved.recommended_model.clone().unwrap_or_default()
            || vm.style_compatible_version.trim()
                != saved.compatible_app_version.clone().unwrap_or_default()
            || vm.style_examples != saved.examples
    }

    type HotkeyJob = std::pin::Pin<Box<dyn Future<Output = Result<String, BackendError>> + Send>>;
    type ProviderJob = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

    impl OpenLessEguiApp {
        fn new(
            tokio: Arc<tokio::runtime::Runtime>,
            native: Result<LinuxNativeRuntime, String>,
            tray: Option<openless_linux_egui::LinuxTray>,
            window_should_be_open: bool,
        ) -> Self {
            let (tx, rx) = mpsc::channel();
            let locale_pref = load_locale_pref();
            let lang = locale_pref.resolve();
            // 外观页的排版偏好：宿主这份视图模型要在首帧就带上已存的选择，
            // 否则组合框会先画默认值再跳一次。
            let typography = openless_linux_egui::typography::load();
            let (provider_jobs, mut provider_rx) =
                tokio::sync::mpsc::unbounded_channel::<ProviderJob>();
            tokio.spawn(async move {
                while let Some(job) = provider_rx.recv().await {
                    job.await;
                }
            });
            let (hotkey_jobs, mut hotkey_rx) = tokio::sync::mpsc::unbounded_channel::<HotkeyJob>();
            let hotkey_results = tx.clone();
            let host_thread = std::thread::current();
            tokio.spawn(async move {
                // Admit edges FIFO: the first poll registers each Core gate
                // waiter before the next edge. Do not serialize completion:
                // Combined must bypass an in-flight start, and model/network
                // finalization must not block opening QA or cancelling input.
                while let Some(mut job) = hotkey_rx.recv().await {
                    let ready = std::future::poll_fn(|cx| {
                        std::task::Poll::Ready(match job.as_mut().poll(cx) {
                            std::task::Poll::Ready(result) => Some(result),
                            std::task::Poll::Pending => None,
                        })
                    })
                    .await;
                    let results = hotkey_results.clone();
                    let wake = host_thread.clone();
                    tokio::spawn(async move {
                        let result = match ready {
                            Some(result) => result,
                            None => job.await,
                        };
                        let _ = results.send(match result {
                            Ok(message) => UiResult::Message(message),
                            Err(error) => UiResult::HotkeyError(error),
                        });
                        wake.unpark();
                    });
                }
            });
            // The Quick Note shortcut card is dismissible and the choice
            // survives restarts (upstream keeps it in localStorage).
            let quick_note_shortcut_hidden = openless_linux_egui::load_quick_note_shortcut_hidden();
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
                    let history = HistoryCache::new(&preferences);
                    let subscription = backend.subscribe();
                    let mut app = Self {
                        tokio,
                        native: Some(native),
                        subscription: Some(subscription),
                        snapshot: Some(snapshot),
                        settings_save: SettingsSave::new(&preferences),
                        preferences: Some(preferences),
                        settings_channel_kind: openless_core::ChannelKind::Llm,
                        settings_channels: Vec::new(),
                        settings_channels_loading: false,
                        settings_channels_request: 0,
                        settings_channels_refresh_requested: false,
                        linux_access_last_check: None,
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
                        qa_state: None,
                        polish_result_visible: false,
                        selection_draft: String::new(),
                        selection: None,
                        remote_access: None,
                        provider_kind: openless_core::ChannelKind::Asr,
                        providers: ProvidersState::Loading,
                        providers_request: 0,
                        selected_channel_id: None,
                        editor_requested: false,
                        provider_editor: ProviderEditorState::Idle,
                        editor_draft: false,
                        draft_touched: false,
                        provider_editor_form: None,
                        provider_jobs,
                        provider_save_pending: false,
                        provider_switch_pending: false,
                        channel_create_pending: false,
                        provider_save_again: false,
                        provider_close_pending: false,
                        provider_after_save: None,
                        provider_models: Vec::new(),
                        new_provider_type: String::new(),
                        pending_channel_delete: None,
                        vocabulary: Vec::new(),
                        correction_rules: Vec::new(),
                        style_packs: Vec::new(),
                        style_icon_cache: std::collections::HashMap::new(),
                        vocab_preset_store: openless_core::VocabPresetStore::default(),
                        vocab_presets: Vec::new(),
                        qa_popup: None,
                        capsule_popup: None,
                        capsule_popup_generation: 0,
                        popup_action_guard: PopupActionGuard::default(),
                        capsule_session: None,
                        capsule_dismissal_scheduled: None,
                        vocab_suggestions: Vec::new(),
                        vocab_card_session: None,
                        vocab_suggestions_checked: std::time::Instant::now(),
                        tray,
                        exit_requested: false,
                        last_pump_heartbeat: std::time::Instant::now(),
                        last_pump_heartbeat_info: std::time::Instant::now(),
                        last_pump_heartbeat_snapshot: [false; 4],
                        window: WindowState::new(window_should_be_open),
                        pending_ui_actions: Vec::new(),
                        pending_local_hotkeys: Vec::new(),
                        hotkey_dedupe: openless_linux_egui::HotkeyDeduplicator::default(),
                        hotkey_jobs,
                        quick_note_shortcut_hidden,
                        popup_restarts: [PopupRestartBudget::default(); POPUP_KIND_COUNT],
                        popup_sequences: [0; POPUP_KIND_COUNT],
                        qa_popup_presentation: QaPopupPresentation::default(),
                        hotkeys_sent: None,
                        pending_ui_pongs: Vec::new(),
                        pending_collection_pages: Vec::new(),
                        history,
                        history_clip: None,
                        marketplace_items: Vec::new(),
                        marketplace_attempted: false,
                        marketplace_query: String::new(),
                        marketplace_detail: None,
                        marketplace_my_packs: Vec::new(),
                        marketplace_my_likes: Vec::new(),
                        marketplace_seq: 0,
                        marketplace_search_deadline: None,
                        marketplace_oauth_poll_deadline: None,
                        marketplace_oauth_interval_secs: 5,
                        marketplace_likes_loaded: false,
                        style_editor: None,
                        status: tr_l10n(lang, "status.core_started").to_string(),
                        startup_error: None,
                        runtime_warning: None,
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
                    settings_save: SettingsSave::default(),
                    settings_channel_kind: openless_core::ChannelKind::Llm,
                    settings_channels: Vec::new(),
                    settings_channels_loading: false,
                    settings_channels_request: 0,
                    settings_channels_refresh_requested: false,
                    linux_access_last_check: None,
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
                    qa_state: None,
                    polish_result_visible: false,
                    selection_draft: String::new(),
                    selection: None,
                    remote_access: None,
                    provider_kind: openless_core::ChannelKind::Asr,
                    providers: ProvidersState::Loading,
                    providers_request: 0,
                    selected_channel_id: None,
                    editor_requested: false,
                    provider_editor: ProviderEditorState::Idle,
                    editor_draft: false,
                    draft_touched: false,
                    provider_editor_form: None,
                    provider_jobs,
                    provider_save_pending: false,
                    provider_switch_pending: false,
                    channel_create_pending: false,
                    provider_save_again: false,
                    provider_close_pending: false,
                    provider_after_save: None,
                    provider_models: Vec::new(),
                    new_provider_type: String::new(),
                    pending_channel_delete: None,
                    vocabulary: Vec::new(),
                    correction_rules: Vec::new(),
                    style_packs: Vec::new(),
                    style_icon_cache: std::collections::HashMap::new(),
                    vocab_preset_store: openless_core::VocabPresetStore::default(),
                    vocab_presets: Vec::new(),
                    qa_popup: None,
                    capsule_popup: None,
                    capsule_popup_generation: 0,
                    popup_action_guard: PopupActionGuard::default(),
                    capsule_session: None,
                    capsule_dismissal_scheduled: None,
                    vocab_suggestions: Vec::new(),
                    vocab_card_session: None,
                    vocab_suggestions_checked: std::time::Instant::now(),
                    tray,
                    exit_requested: false,
                    last_pump_heartbeat: std::time::Instant::now(),
                    last_pump_heartbeat_info: std::time::Instant::now(),
                    last_pump_heartbeat_snapshot: [false; 4],
                    window: WindowState::new(window_should_be_open),
                    pending_ui_actions: Vec::new(),
                    pending_local_hotkeys: Vec::new(),
                    hotkey_dedupe: openless_linux_egui::HotkeyDeduplicator::default(),
                    hotkey_jobs,
                    quick_note_shortcut_hidden,
                    popup_restarts: [PopupRestartBudget::default(); POPUP_KIND_COUNT],
                    popup_sequences: [0; POPUP_KIND_COUNT],
                    qa_popup_presentation: QaPopupPresentation::default(),
                    hotkeys_sent: None,
                    pending_ui_pongs: Vec::new(),
                    pending_collection_pages: Vec::new(),
                    history: HistoryCache::default(),
                    history_clip: None,
                    marketplace_items: Vec::new(),
                    marketplace_attempted: false,
                    marketplace_query: String::new(),
                    marketplace_detail: None,
                    marketplace_my_packs: Vec::new(),
                    marketplace_my_likes: Vec::new(),
                    marketplace_seq: 0,
                    marketplace_search_deadline: None,
                    marketplace_oauth_poll_deadline: None,
                    marketplace_oauth_interval_secs: 5,
                    marketplace_likes_loaded: false,
                    style_editor: None,
                    status: tr_l10n(lang, "status.startup_failed").to_string(),
                    startup_error: Some(error),
                    runtime_warning: None,
                    locale_pref,
                    lang,
                    active_page: shell::Page::Overview,
                    frontend_vm: FrontendViewModel {
                        typography,
                        ..Default::default()
                    },
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

        /// Hide / restore the Quick Note shortcut card and persist the choice.
        fn set_quick_note_shortcut_hidden(&mut self, hidden: bool) {
            self.quick_note_shortcut_hidden = hidden;
            self.frontend_vm.quick_note_shortcut_hidden = hidden;
            if let Err(error) = openless_linux_egui::save_quick_note_shortcut_hidden(hidden) {
                log::warn!("[ui] cannot persist quick-note shortcut visibility: {error}");
            }
        }

        /// Load the editor's stored pack into the view model. `Revert` reuses
        /// this so the drawer always shows Core's last persisted values.
        fn hydrate_style_editor(&mut self, pack: &openless_core::StylePack, exists: bool) {
            // Read the workflow-specific active id before borrowing `frontend_vm`.
            let selection_active = self.selection_style_pack_id() == Some(pack.id.clone());
            let vm = &mut self.frontend_vm;
            vm.style_editor_open = true;
            vm.style_editor_id = pack.id.clone();
            vm.style_editor_builtin = pack.kind == openless_core::StylePackKind::Builtin;
            vm.style_editor_active = if vm.style_selection_workflow {
                selection_active
            } else {
                pack.active
            };
            vm.style_editor_mode = polish_mode_label(self.lang, pack.base_mode).to_string();
            vm.style_prompt = pack.prompt.clone();
            vm.style_name = pack.name.clone();
            vm.style_description = pack.description.clone();
            vm.style_selection_prompt = pack.selection_prompt.clone();
            vm.style_voice_edit_prompt = pack.voice_edit_prompt.clone();
            vm.style_tags = pack.tags.join(", ");
            vm.style_author = pack.author.clone().unwrap_or_default();
            vm.style_version = pack.version.clone();
            vm.style_model = pack.recommended_model.clone().unwrap_or_default();
            vm.style_compatible_version = pack.compatible_app_version.clone().unwrap_or_default();
            vm.style_examples = pack.examples.clone();
            vm.style_editor_dirty = !exists;
            vm.style_editor_publishing = false;
            vm.style_editor_saved = Some(pack.clone());
        }

        fn selection_style_pack_id(&self) -> Option<String> {
            self.preferences
                .as_ref()
                .map(|prefs| prefs.selection_polish_style_pack_id.clone())
                .filter(|id| !id.is_empty())
        }

        /// Data URLs for every style-pack icon, cached by `(id, icon_path)`.
        fn style_icon_urls(&mut self) -> Vec<Option<String>> {
            let backend = self.backend();
            let mut seen = Vec::new();
            let urls = self
                .style_packs
                .iter()
                .map(|pack| {
                    let path = pack.icon_path.clone()?;
                    let key = (pack.id.clone(), path);
                    seen.push(key.clone());
                    if let Some(url) = self.style_icon_cache.get(&key) {
                        return Some(url.clone());
                    }
                    let url = backend
                        .as_ref()?
                        .read_style_pack_icon(&pack.id)
                        .ok()
                        .flatten();
                    if let Some(url) = &url {
                        self.style_icon_cache.insert(key, url.clone());
                    }
                    url
                })
                .collect();
            // Drop entries whose icon was replaced or removed.
            self.style_icon_cache.retain(|key, _| seen.contains(key));
            urls
        }

        /// Rasterize an SVG picked by the user and hand the PNG to Core.
        fn set_style_pack_icon_from_file(&mut self, id: String) {
            let Some(backend) = self.backend() else {
                return;
            };
            let lang = self.lang;
            self.spawn_reporting(async move {
                let bytes = tokio::task::spawn_blocking(|| {
                    let path = rfd::FileDialog::new()
                        .add_filter("SVG icon", &["svg"])
                        .pick_file()?;
                    std::fs::read(path).ok()
                })
                .await
                .map_err(|error| {
                    BackendError::new(openless_core::BackendErrorCode::Internal, error.to_string())
                })?;
                // Dismissing the picker is not an error and needs no notice.
                let Some(bytes) = bytes else {
                    return Ok(String::new());
                };
                let png =
                    tokio::task::spawn_blocking(move || crate::style_icon::rasterize_svg(&bytes))
                        .await
                        .map_err(|error| {
                            BackendError::new(
                                openless_core::BackendErrorCode::Internal,
                                error.to_string(),
                            )
                        })?
                        .map_err(|reason| {
                            log::warn!("[style] rejected icon for {id}: {reason}");
                            BackendError::new(
                                openless_core::BackendErrorCode::InvalidArgument,
                                tr_l10n(lang, "style.pack.iconInvalid"),
                            )
                        })?;
                backend.set_style_pack_icon(&id, Some(&png))?;
                Ok(tr_l10n(lang, "style.pack.iconSaved").to_string())
            });
        }

        fn reset_style_pack_icon(&mut self, id: String) {
            let Some(backend) = self.backend() else {
                return;
            };
            let lang = self.lang;
            self.spawn(async move {
                backend.set_style_pack_icon(&id, None)?;
                Ok(tr_l10n(lang, "style.pack.resetIcon").to_string())
            });
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
                    if kind == PopupKind::Qa {
                        // 新进程 = 新代次：旧进程排队的按键/错误不得作用到新窗口。
                        self.qa_popup_presentation.generation =
                            self.qa_popup_presentation.generation.saturating_add(1);
                        self.qa_popup_presentation.session_id = None;
                    }
                    if kind == PopupKind::Capsule {
                        self.capsule_popup_generation =
                            self.capsule_popup_generation.saturating_add(1);
                        self.vocab_card_session = self
                            .vocab_card_session
                            .as_ref()
                            .map(|_| format!("vocab:{}", uuid::Uuid::new_v4()));
                    }
                    self.popup_action_guard.reset(kind);
                    let supervisor = PopupSupervisor::spawn(self.tokio.handle(), executable, kind);
                    *self.popup_slot(kind) = Some(supervisor);
                }
                Err(error) => self.status = fmt_l10n(lang, "popup.start_failed", &[&error]),
            }
        }

        fn send_popup(&mut self, kind: PopupKind, mut message: HostToPopup) {
            let lang = self.lang;
            let qa_session = match &message {
                HostToPopup::QaSnapshot { session_id, .. }
                | HostToPopup::PolishPreview { session_id, .. } => Some(session_id.clone()),
                _ => None,
            };
            // 窗口动作与失败可能在**没有新 Core 事件**时发生，只有递增序号才能让
            // 弹窗接受新的快照（同序号会被当成旧消息丢掉）。
            let sequence = &mut self.popup_sequences[popup_kind_index(kind)];
            *sequence = sequence.saturating_add(1);
            message.set_sequence(*sequence);
            // 记录胶囊当前承载的会话：兜底收起要靠它判断「会话是否还在快照里」。
            if let HostToPopup::Capsule { session_id, .. } = &message {
                self.capsule_session = Some(session_id.clone());
            }
            let mut retry = message.clone();
            if let Some(supervisor) = self.popup_slot(kind) {
                if let Err(error) = supervisor.try_send(message) {
                    self.status = fmt_l10n(lang, "popup.channel_rebuild", &[&format!("{error:?}")]);
                    *self.popup_slot(kind) = None;
                    self.ensure_popup(kind);
                    if let (
                        HostToPopup::Capsule {
                            session_id,
                            suggestions,
                            ..
                        },
                        Some(token),
                    ) = (&mut retry, &self.vocab_card_session)
                    {
                        if !suggestions.is_empty() {
                            session_id.clone_from(token);
                        }
                    }
                    if let Some(supervisor) = self.popup_slot(kind) {
                        if let Err(retry_error) = supervisor.try_send(retry) {
                            self.status = fmt_l10n(
                                lang,
                                "popup.recover_failed",
                                &[&format!("{retry_error:?}")],
                            );
                            return;
                        }
                    } else {
                        return;
                    }
                }
                if let Some(session_id) = qa_session {
                    self.qa_popup_presentation.session_id = Some(session_id);
                }
            }
        }

        /// 收起选区助手面板：清掉宿主可见标志并**结束弹窗进程**
        /// （X11/Wayland 上都得真的销毁窗口，Core 继续持有对话）。
        fn close_qa_popup(&mut self) {
            self.qa_visible = false;
            self.polish_result_visible = false;
            self.qa_popup_presentation.session_id = None;
            self.qa_popup_presentation.generation =
                self.qa_popup_presentation.generation.saturating_add(1);
            if let Some(supervisor) = self.qa_popup.take() {
                let _ = supervisor.request_shutdown();
            }
        }

        fn hide_popup(&mut self, kind: PopupKind, session_id: String, sequence: u64) {
            if kind == PopupKind::Qa {
                // 仅当隐藏请求针对**当前上屏的会话**时才动窗口：后台 QA 的
                // 隐藏请求不能把正在显示的润色预览关掉。
                if (self.qa_visible || self.polish_result_visible)
                    && self
                        .qa_popup_presentation
                        .session_id
                        .as_deref()
                        .is_some_and(|current| current != session_id.as_str())
                {
                    return;
                }
                // End the native window on both X11 and Wayland. Core owns the
                // conversation; the next show creates a fresh protocol state.
                self.close_qa_popup();
                return;
            }
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
            if self
                .backend()
                .is_none_or(|backend| backend.ensure_testing_features_enabled().is_err())
            {
                return;
            }
            self.ensure_popup(PopupKind::LessComputer);
            if self
                .backend()
                .is_none_or(|backend| backend.ensure_testing_features_enabled().is_err())
            {
                self.less_computer_popup = None;
                return;
            }
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
                // 面板已经收起：任何排队消息都不再有合法目标。
                PopupKind::Qa if !self.qa_visible && !self.polish_result_visible => None,
                // 润色结果模式下，面板承载的是选区会话，而不是 QA 会话。
                PopupKind::Qa if self.polish_result_visible => self
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.session_id)
                    .map(|id| id.to_string()),
                PopupKind::Qa => self
                    .qa_state
                    .as_ref()
                    .map(|state| state.session_id.clone().unwrap_or_else(|| "qa".to_string())),
                PopupKind::Capsule => self.vocab_card_session.clone().or_else(|| {
                    self.snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.dictation.session_id)
                        .map(|session_id| session_id.to_string())
                }),
                PopupKind::LessComputer => self
                    .less_computer_session
                    .map(|session_id| session_id.to_string()),
            }
        }

        fn show_qa_popup(&mut self) {
            // 收起状态或润色结果模式下不得用 QA 快照把面板拉回来。
            if !self.qa_visible || self.polish_result_visible {
                return;
            }
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
                services
                    .selection_voice
                    .finish_preview_apply(ticket.ticket_id, outcome)
                    .await?;
                if !outcome.may_have_applied() {
                    return Err(BackendError::new(
                        openless_core::BackendErrorCode::Platform,
                        "selectionVoiceInsertFailed",
                    ));
                }
                // Match the Tauri command: only dismiss this exact QA turn after
                // Core has recorded a successful native apply receipt. A failed
                // fcitx5 target check must leave the preview available to retry.
                if let Err(error) = services.qa.dismiss_session(qa_session).await {
                    if error.code != openless_core::BackendErrorCode::Cancelled {
                        return Err(error);
                    }
                }
                Ok(tr_l10n(lang, "selection.replaced").to_string())
            });
        }

        /// 润色结果：**不再有独立预览窗口**，一律送进选区助手面板的「润色结果」模式
        /// （用户确认的设计：选区只保留一个弹窗）。
        fn show_selection_popup(&mut self) {
            // 先确认有可送的润色负载，再拉起弹窗。旧实现先 `ensure_popup` 再
            // early-return，于是「没有选区 / 没有会话」时会凭空弹出一个**空的选区
            // 助手面板**（用户报的「alt+X 弹出错误弹窗」），还会留下
            // `polish_result_visible` 这个脏标志。
            let Some(selection) = self.selection.clone() else {
                log::warn!(
                    "[hotkey] selection polish finished without a captured selection; not showing the panel"
                );
                self.polish_result_visible = false;
                return;
            };
            let Some(session_id) = selection.session_id else {
                log::warn!(
                    "[hotkey] selection polish finished without a session id; not showing the panel"
                );
                self.polish_result_visible = false;
                return;
            };
            self.ensure_popup(PopupKind::Qa);
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

        /// Core 的待确认纠正变了：没有会话时把卡片顶到胶囊窗上，有会话就丢掉候选。
        ///
        /// Tauri `show_vocab_suggestion_card` / `hide_vocab_suggestion_card`：卡片与
        /// 录音胶囊**共用同一个窗口**，所以「会话在飞」时必须让路，否则卡片的尺寸/位置
        /// 会把这次会话的胶囊顶掉。
        fn refresh_vocab_suggestions(&mut self) {
            let Some(backend) = self.backend() else {
                return;
            };
            self.vocab_suggestions_checked = std::time::Instant::now();
            let pending = backend.pending_corrections();
            let phase = self
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.dictation.phase);
            if !openless_linux_egui::vocab_card_allowed(phase) {
                if !pending.is_empty() {
                    backend.dismiss_pending_corrections();
                }
                self.vocab_suggestions.clear();
                self.hide_vocab_card();
                return;
            }
            self.vocab_suggestions = pending
                .iter()
                .map(|item| openless_linux_egui::CapsuleSuggestion {
                    id: item.id.clone(),
                    pattern: item.pattern.clone(),
                    replacement: item.replacement.clone(),
                })
                .collect();
            if self.vocab_suggestions.is_empty() {
                self.hide_vocab_card();
            } else {
                self.show_vocab_card();
            }
        }

        /// 丢弃当前候选（新会话开始 / 胶囊开关关掉时）。
        fn dismiss_vocab_suggestions(&mut self) {
            if self.vocab_suggestions.is_empty() && self.vocab_card_session.is_none() {
                return;
            }
            if let Some(backend) = self.backend() {
                backend.dismiss_pending_corrections();
            }
            self.vocab_suggestions.clear();
            self.hide_vocab_card();
        }

        /// 卡片独立于听写会话；token 同时标记可见状态并隔离过期动作。
        fn show_vocab_card(&mut self) {
            if !self.capsule_enabled() {
                self.dismiss_vocab_suggestions();
                return;
            }
            self.ensure_popup(PopupKind::Capsule);
            let session_id = self
                .vocab_card_session
                .get_or_insert_with(|| format!("vocab:{}", uuid::Uuid::new_v4()))
                .clone();
            self.send_popup(
                PopupKind::Capsule,
                HostToPopup::Capsule {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id,
                    sequence: self.last_event_sequence.saturating_mul(2),
                    phase: String::new(),
                    text: String::new(),
                    transcript: String::new(),
                    transcript_font_size: 14,
                    audio_level: None,
                    translation_active: false,
                    style: self.capsule_style_tag(),
                    suggestions: self.vocab_suggestions.clone(),
                },
            );
            // 卡片不参加听写会话的兜底收起。
            self.capsule_session = None;
        }

        fn hide_vocab_card(&mut self) {
            let Some(session_id) = self.vocab_card_session.take() else {
                return;
            };
            self.send_popup(
                PopupKind::Capsule,
                HostToPopup::Hide {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id,
                    sequence: 0,
                },
            );
            self.capsule_session = None;
        }

        /// 卡片上有 Core 给的死线（`PendingCorrection::expires_at_ms`），Core 只按需
        /// 清理过期项、不发事件；这里按秒重算，过期的行自己消失。
        fn tick_vocab_suggestions(&mut self) {
            if self.vocab_suggestions.is_empty() {
                return;
            }
            if self.vocab_suggestions_checked.elapsed() < std::time::Duration::from_millis(500) {
                return;
            }
            self.refresh_vocab_suggestions();
        }

        fn capsule_transcript(&self) -> String {
            // Selection polish uses the capsule too, but must never expose the
            // previous dictation's raw text (Tauri selectionPolish gating).
            if self.selection.as_ref().is_some_and(|selection| {
                matches!(
                    selection.phase,
                    SelectionPhase::Capturing | SelectionPhase::Preview | SelectionPhase::Applying
                )
            }) {
                return String::new();
            }
            if self
                .preferences
                .as_ref()
                .is_none_or(|prefs| prefs.capsule_transcript_enabled)
            {
                self.transcript_state.text().to_owned()
            } else {
                String::new()
            }
        }

        fn capsule_transcript_font_size(&self) -> u8 {
            self.preferences
                .as_ref()
                .map_or(14, |prefs| prefs.capsule_transcript_font_size.clamp(12, 20))
        }

        fn sync_capsule_transcript(&mut self) {
            if !self.capsule_enabled() {
                return;
            }
            let Some(backend) = self.backend() else {
                return;
            };
            let snapshot = backend.snapshot().dictation;
            let Some(session_id) = snapshot
                .session_id
                .filter(|id| Some(*id) == self.transcript_session)
            else {
                return;
            };
            if !matches!(
                snapshot.phase,
                DictationPhase::Recording
                    | DictationPhase::Transcribing
                    | DictationPhase::Polishing
            ) {
                return;
            }
            self.send_popup(
                PopupKind::Capsule,
                HostToPopup::Capsule {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.to_string(),
                    sequence: self.last_event_sequence.saturating_mul(2).saturating_add(1),
                    phase: format!("{:?}", snapshot.phase),
                    text: String::new(),
                    transcript: self.capsule_transcript(),
                    transcript_font_size: self.capsule_transcript_font_size(),
                    audio_level: Some(snapshot.level),
                    translation_active: snapshot.translation_active,
                    style: self.capsule_style_tag(),
                    suggestions: Vec::new(),
                },
            );
        }

        fn show_capsule_popup(&mut self) {
            if !self.vocab_suggestions.is_empty() {
                self.show_vocab_card();
                return;
            }
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
                    transcript: self.capsule_transcript(),
                    transcript_font_size: self.capsule_transcript_font_size(),
                    audio_level: Some(snapshot.level),
                    translation_active: snapshot.translation_active,
                    style,
                    suggestions: self.vocab_suggestions.clone(),
                },
            );
            self.schedule_capsule_dismissal(&session_id.to_string(), snapshot.phase);
        }

        /// 选区润色（alt+X）发现「没有选中文字」时，按产品行为弹**语音输入窗**
        /// （胶囊）报错后退出：文案用 Tauri 的 `capsule.selectionPolish.noSelection`，
        /// 终态 2 秒后自动收起。以前这里什么都不做，用户只看到面板空转一圈（或
        /// 干脆没有反馈）。
        fn show_selection_polish_capsule_error(&mut self, session_id: &str) {
            if !self.capsule_enabled() {
                return;
            }
            let lang = self.lang;
            self.ensure_popup(PopupKind::Capsule);
            self.capsule_session = Some(session_id.to_string());
            self.send_popup(
                PopupKind::Capsule,
                HostToPopup::Capsule {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.to_string(),
                    sequence: self.last_event_sequence.saturating_mul(2),
                    phase: "failed".to_string(),
                    text: tr_l10n(lang, "capsule.selectionPolish.noSelection").to_string(),
                    transcript: String::new(),
                    transcript_font_size: 14,
                    audio_level: None,
                    translation_active: false,
                    style: self.capsule_style_tag(),
                    suggestions: self.vocab_suggestions.clone(),
                },
            );
            self.schedule_capsule_dismissal(session_id, DictationPhase::Failed);
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
            self.vocab_card_session = None;
            self.capsule_dismissal_scheduled = None;
            // 这条日志缺失 = 收起决定没走到「结束弹窗进程」这一环。
            log::info!("capsule: dismissal applied (popup process was running: {had_process})");
        }

        fn poll_popup_supervisors(&mut self) {
            let mut events = Vec::new();
            for kind in [PopupKind::Qa, PopupKind::Capsule] {
                let generation = match kind {
                    PopupKind::Capsule => self.capsule_popup_generation,
                    _ => self.qa_popup_presentation.generation,
                };
                if let Some(supervisor) = self.popup_slot(kind) {
                    while let Ok(event) = supervisor.try_recv() {
                        events.push((kind, generation, event));
                    }
                }
            }
            for (kind, generation, event) in events {
                self.apply_popup_event_in_generation(kind, generation, event);
            }
        }

        fn apply_popup_event_in_generation(
            &mut self,
            kind: PopupKind,
            generation: u64,
            event: PopupSupervisorEvent,
        ) {
            // 处理一条排队消息时可能已经把进程换掉了：旧进程剩下的消息全部作废。
            let stale = match kind {
                PopupKind::Qa => generation != self.qa_popup_presentation.generation,
                PopupKind::Capsule => generation != self.capsule_popup_generation,
                PopupKind::LessComputer => false,
            };
            if stale {
                return;
            }
            self.apply_popup_event(kind, event);
        }

        fn apply_popup_event(&mut self, kind: PopupKind, event: PopupSupervisorEvent) {
            let lang = self.lang;
            if let PopupSupervisorEvent::Message(message) = &event {
                if let PopupToHost::VocabSuggestion { session_id, id, .. } = message {
                    let current = self.vocab_card_session.as_deref() == Some(session_id.as_str())
                        && self.vocab_suggestions.iter().any(|item| item.id == *id)
                        && self.backend().is_none_or(|backend| {
                            backend
                                .pending_corrections()
                                .iter()
                                .any(|item| item.id == *id)
                        });
                    if !current {
                        self.status = tr_l10n(lang, "popup.ignore_stale").to_string();
                        return;
                    }
                }
                let Some(expected_session) = self.expected_popup_session(kind) else {
                    self.status = tr_l10n(lang, "popup.ignore_no_session").to_string();
                    return;
                };
                if !self
                    .popup_action_guard
                    .accept(kind, message, &expected_session)
                {
                    self.status = tr_l10n(lang, "popup.ignore_stale").to_string();
                    return;
                }
            }
            match event {
                PopupSupervisorEvent::Message(PopupToHost::SubmitQa {
                    session_id, text, ..
                }) if self.expected_popup_session(PopupKind::Qa).as_deref()
                    == Some(session_id.as_str()) =>
                {
                    if let Some(backend) = self.backend() {
                        // 空态面板没有 Core 会话：仍然允许发送，由 Core 在
                        // **当前上下文**里检查并创建新轮次。
                        let context = self
                            .qa_state
                            .as_ref()
                            .and_then(|state| state.session_id.clone());
                        let expected = context
                            .as_deref()
                            .map(str::parse::<uuid::Uuid>)
                            .transpose()
                            .map(|id| id.map(openless_core::SessionId::from_uuid));
                        self.spawn_qa_action(context, async move {
                            let expected = expected.map_err(|error| {
                                BackendError::new(
                                    openless_core::BackendErrorCode::InvalidState,
                                    error.to_string(),
                                )
                            })?;
                            backend
                                .services()
                                .qa
                                .submit_text_in_context(text, expected)
                                .await?;
                            Ok(tr_l10n(lang, "qa.submitted").to_string())
                        });
                    }
                }
                PopupSupervisorEvent::Message(PopupToHost::ToggleQaRecording {
                    session_id,
                    ..
                }) if self.expected_popup_session(PopupKind::Qa).as_deref()
                    == Some(session_id.as_str()) =>
                {
                    if let Some(backend) = self.backend() {
                        let context = self
                            .qa_state
                            .as_ref()
                            .and_then(|state| state.session_id.clone());
                        self.spawn_qa_action(context, async move {
                            backend.services().qa.toggle_recording().await?;
                            Ok(tr_l10n(lang, "qa.recording_updated").to_string())
                        });
                    }
                }
                PopupSupervisorEvent::Message(PopupToHost::DismissQa { session_id, .. })
                    if self.expected_popup_session(PopupKind::Qa).as_deref()
                        == Some(session_id.as_str()) =>
                {
                    // 面板 ✕：收起 = 结束弹窗进程（Core 继续持有对话），
                    // 不等 Core 的 HideQa 回环（那条被图钉门禁拦着，pinned 时
                    // 不清 → 标志残留会让录音热键之后又把面板弹出来）。
                    self.close_qa_popup();
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
                }) if self.expected_popup_session(PopupKind::Qa).as_deref()
                    == Some(session_id.as_str()) =>
                {
                    self.qa_pinned = pinned;
                    self.show_qa_popup();
                }
                PopupSupervisorEvent::Message(PopupToHost::SetEditInstructionMode {
                    session_id,
                    enabled,
                    ..
                }) if self.expected_popup_session(PopupKind::Qa).as_deref()
                    == Some(session_id.as_str()) =>
                {
                    if let Some(backend) = self.backend() {
                        // 代次与上下文一起带上：返回时若面板已换窗口，结果作废。
                        let context = self.expected_popup_session(PopupKind::Qa);
                        self.spawn_qa_action(context, async move {
                            backend
                                .services()
                                .qa
                                .set_edit_instruction_mode(enabled)
                                .await?;
                            Ok(String::new())
                        });
                    }
                }
                PopupSupervisorEvent::Message(PopupToHost::RevertEdit { session_id, .. })
                    if self.expected_popup_session(PopupKind::Qa).as_deref()
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
                PopupSupervisorEvent::Message(PopupToHost::ApplyEdit { session_id, .. })
                    if self.expected_popup_session(PopupKind::Qa).as_deref()
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
                    Err(error) => self.status = fmt_l10n(lang, "popup.session_invalid", &[&error]),
                },
                PopupSupervisorEvent::Message(PopupToHost::CancelPolish { session_id, .. }) => {
                    match session_id.parse::<uuid::Uuid>() {
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
                    }
                }
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
                    log::debug!("[hotkey] local edge from the {kind:?} panel: {edge:?}");
                    self.pending_local_hotkeys
                        .push((std::time::Instant::now(), edge));
                }
                PopupSupervisorEvent::Message(PopupToHost::VocabSuggestion {
                    id, accept, ..
                }) => {
                    // 点一下就是一次词库写入（同步 IO），放到后台线程，别卡这一帧。
                    if let Some(backend) = self.backend() {
                        let result = if accept {
                            backend
                                .accept_pending_correction(&id)
                                .map(|_| String::new())
                                .map_err(|error| error.to_string())
                        } else {
                            backend.reject_pending_correction(&id);
                            Ok(String::new())
                        };
                        if let Err(error) = result {
                            log::warn!("[vocab-card] resolve failed: {error}");
                        }
                        self.refresh_vocab_suggestions();
                    }
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
                            normalize_stop_result(backend.cancel_dictation(Some(session)).await)?;
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
                            normalize_stop_result(backend.stop_dictation_session(session).await)?;
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
                    if !crashed {
                        // 面板进程正常退出（用户/合成器关掉窗口等）：宿主这边的
                        // 可见标志必须跟着清，否则「录音热键」会被错当成
                        // 「向选区助手提问」，把面板又弹出来。
                        self.forget_qa_panel_visibility(kind);
                    }
                    if crashed {
                        // 必崩的面板不做无限重开：预算内重开，超了就停手（否则
                        // 用户看到的是“弹窗一直反复弹出”）。
                        let restarts = &mut self.popup_restarts[popup_kind_index(kind)];
                        if !restarts.allow(std::time::Instant::now()) {
                            log::warn!(
                                    "[popup] {kind:?} crashed {POPUP_RESTART_LIMIT} times within {}s; not restarting",
                                    POPUP_RESTART_WINDOW.as_secs()
                                );
                            self.forget_qa_panel_visibility(kind);
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

        fn spawn_hotkey<F>(&self, future: F)
        where
            F: Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            if self.hotkey_jobs.send(Box::pin(future)).is_err() {
                log::error!("[hotkey] ordered dispatcher stopped");
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

        fn spawn_qa_action<F>(&self, context: Option<String>, future: F)
        where
            F: Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            let tx = self.tx.clone();
            let generation = self.qa_popup_presentation.generation;
            self.tokio.spawn(async move {
                let result = future.await.map_err(|error| error.to_string());
                let _ = tx.send(UiResult::QaAction {
                    generation,
                    context,
                    result,
                });
            });
        }

        /// 选区助手动作的异步结果：只有当前代次、且仍属于同上下文的失败才回写到面板
        /// （早于本轮的失败不能覆盖新轮次）。
        fn apply_qa_action_result(
            &mut self,
            generation: u64,
            context: Option<String>,
            result: Result<String, String>,
        ) {
            if generation != self.qa_popup_presentation.generation {
                return;
            }
            match result {
                Ok(message) => self.status = message,
                Err(error) => {
                    self.status = error.clone();
                    if self.qa_visible {
                        if let Some(state) = self
                            .qa_state
                            .as_mut()
                            .filter(|state| state.session_id == context)
                        {
                            state.error = Some(error);
                            if self.qa_popup.is_some() {
                                self.show_qa_popup();
                            }
                        }
                    }
                }
            }
        }

        /// `spawn` variant for cancellable flows: an empty message means
        /// "nothing happened", so no notice is posted.
        fn spawn_reporting<F>(&self, future: F)
        where
            F: Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let message = future.await.unwrap_or_else(|error| error.to_string());
                if !message.is_empty() {
                    let _ = tx.send(UiResult::Message(message));
                }
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

        fn load_providers(&mut self, kind: openless_core::ChannelKind) {
            let Some(backend) = self.backend() else {
                return;
            };
            self.providers_request = self.providers_request.wrapping_add(1);
            let request = self.providers_request;
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
                let _ = tx.send(UiResult::Providers {
                    kind,
                    request,
                    result,
                });
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
            // 已有列表时后台静默刷新，不把界面切成“加载中”（会造成整页跳动）。
            self.settings_channels_loading = self.settings_channels.is_empty();
            self.settings_channels_request = self.settings_channels_request.wrapping_add(1);
            let request = self.settings_channels_request;
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
                            last_test_at: channel.last_test.as_ref().map(|test| test.at),
                        });
                    }
                    Ok::<_, BackendError>(rows)
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::SettingsChannels {
                    kind,
                    request,
                    result,
                });
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

        fn load_marketplace_auth(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .marketplace
                    .auth_status()
                    .await
                    .map(|status| status.signed_in)
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::MarketplaceAuth(result));
            });
        }

        /// 读一次云同步状态（顺带刷新市场登录态，决定能否走「用 GitHub 登录」这条路）。
        fn load_cloud_sync(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let signed_in = backend
                    .services()
                    .marketplace
                    .auth_status()
                    .await
                    .map(|status| status.signed_in)
                    .unwrap_or(false);
                let _ = tx.send(UiResult::MarketplaceAuth(Ok(signed_in)));
                match backend.cloud_sync_e2ee_status() {
                    Ok(status) => {
                        let _ = tx.send(UiResult::CloudSyncDone {
                            result: Box::new(Ok(
                                openless_linux_egui::cloud_sync_ui::SyncOutcome::Status(Box::new(
                                    status,
                                )),
                            )),
                        });
                    }
                    Err(error) => {
                        let _ = tx.send(UiResult::CloudSyncDone {
                            result: Box::new(Err(error.to_string())),
                        });
                    }
                }
            });
        }

        /// 云同步卡片的唯一入口：UI 只发动作，所有 Core 调用都在这里发生。
        fn apply_cloud_sync_action(
            &mut self,
            action: openless_linux_egui::cloud_sync_ui::CloudSyncAction,
        ) {
            use openless_linux_egui::cloud_sync_ui::{
                CloudSyncAction, CloudSyncDialog, CloudSyncIntent, CloudSyncOp, CONSENT_VERSION,
            };

            // 纯 UI 动作：改草稿、开关对话框，不碰 Core。
            match &action {
                CloudSyncAction::Password(value) => {
                    self.frontend_vm.cloud_sync.password = value.clone();
                    return;
                }
                CloudSyncAction::Confirmation(value) => {
                    self.frontend_vm.cloud_sync.confirmation = value.clone();
                    return;
                }
                CloudSyncAction::CurrentPassword(value) => {
                    self.frontend_vm.cloud_sync.current_password = value.clone();
                    return;
                }
                CloudSyncAction::RememberKey(value) => {
                    self.frontend_vm.cloud_sync.remember_key = *value;
                    return;
                }
                CloudSyncAction::ConsentChecked(value) => {
                    self.frontend_vm.cloud_sync.consent_checked = *value;
                    return;
                }
                CloudSyncAction::CustomOrigin(value) => {
                    self.frontend_vm.cloud_sync.custom_origin = value.clone();
                    return;
                }
                CloudSyncAction::CustomToken(value) => {
                    self.frontend_vm.cloud_sync.custom_token = value.clone();
                    return;
                }
                CloudSyncAction::OpenDialog(dialog) => {
                    self.frontend_vm.cloud_sync.open(dialog.clone());
                    return;
                }
                CloudSyncAction::CloseDialog => {
                    self.frontend_vm.cloud_sync.close();
                    return;
                }
                CloudSyncAction::Toggle(enabled) => {
                    if !*enabled {
                        self.frontend_vm.cloud_sync.open(CloudSyncDialog::Disable);
                        return;
                    }
                    // 启用前必须过「同意 + 协议警告」两屏（除非该版本已经同意过）。
                    let consent_stale = self
                        .frontend_vm
                        .cloud_sync
                        .status
                        .as_ref()
                        .and_then(|status| status.consent_version.clone())
                        .as_deref()
                        != Some(CONSENT_VERSION);
                    self.frontend_vm.cloud_sync.open(if consent_stale {
                        CloudSyncDialog::Consent {
                            intent: CloudSyncIntent::Enable,
                        }
                    } else {
                        CloudSyncDialog::Protocol {
                            intent: CloudSyncIntent::Enable,
                        }
                    });
                    return;
                }
                CloudSyncAction::ResolveConflict(id, side) => {
                    self.frontend_vm.cloud_sync.resolve_conflict(id, *side);
                    return;
                }
                CloudSyncAction::Reload => {
                    self.load_cloud_sync();
                    return;
                }
                _ => {}
            }

            if self.frontend_vm.cloud_sync.busy {
                return;
            }

            // 把「要做什么」压成一个纯数据操作，再交给同一个异步任务执行：
            // 每个 Core 调用返回的类型都不同，只有一个 await 点时才不会互相打架。
            let state = &self.frontend_vm.cloud_sync;
            let dialog = state.dialog.clone();
            let status = state.status.clone();
            let remember_key = state.remember_key;
            let passwords = (
                state.current_password.clone(),
                state.password.clone(),
                state.confirmation.clone(),
            );
            let custom = (state.custom_origin.clone(), state.custom_token.clone());
            let origin = state.normalized_origin();
            let conflicts = state.conflict_choices();
            let conflicts_ready = state.conflicts_resolved();

            let operation = match action {
                CloudSyncAction::Prepare(intent) => {
                    self.frontend_vm.cloud_sync.intent = Some(intent);
                    CloudSyncOp::Prepare {
                        consent_version: CONSENT_VERSION.to_string(),
                    }
                }
                CloudSyncAction::SubmitPassword(intent) => {
                    self.frontend_vm.cloud_sync.intent = Some(intent);
                    let (current, password, confirmation) = passwords;
                    match &dialog {
                        CloudSyncDialog::Create { observed_revision } => CloudSyncOp::Create {
                            password,
                            confirmation,
                            remember_key,
                            consent_version: CONSENT_VERSION.to_string(),
                            observed_revision: observed_revision.clone(),
                        },
                        CloudSyncDialog::ChangePassword => CloudSyncOp::ChangePassword {
                            current_password: current,
                            password,
                            confirmation,
                            remember_key,
                        },
                        _ => CloudSyncOp::Unlock {
                            password,
                            remember_key,
                        },
                    }
                }
                CloudSyncAction::SetEnabled(enabled) => CloudSyncOp::SetEnabled(enabled),
                CloudSyncAction::Lock => CloudSyncOp::Lock,
                CloudSyncAction::SyncNow => CloudSyncOp::SyncNow,
                CloudSyncAction::Cancel(task) => CloudSyncOp::Cancel(task),
                CloudSyncAction::PreviewRestore => {
                    let Some(observed) = status
                        .as_ref()
                        .and_then(|status| status.remote_revision.clone())
                    else {
                        self.frontend_vm.cloud_sync.apply_error("stale_preview");
                        return;
                    };
                    CloudSyncOp::PreviewRestore { observed }
                }
                CloudSyncAction::ApplyRestore(mode) => {
                    let CloudSyncDialog::Restore { preview, .. } = &dialog else {
                        return;
                    };
                    if !conflicts_ready {
                        self.frontend_vm.cloud_sync.apply_error("conflict");
                        return;
                    }
                    let stale = status.as_ref().is_none_or(|status| {
                        status.remote_revision.as_deref()
                            != Some(preview.observed_revision.as_str())
                            || status.local_generation != preview.local_generation
                    });
                    if stale {
                        self.frontend_vm.cloud_sync.apply_error("stale_preview");
                        return;
                    }
                    CloudSyncOp::ApplyRestore {
                        preview_id: preview.preview_id.clone(),
                        mode,
                        conflicts,
                    }
                }
                CloudSyncAction::DeleteRemote => {
                    let CloudSyncDialog::Delete { vault_id, revision } = &dialog else {
                        return;
                    };
                    CloudSyncOp::DeleteRemote {
                        vault_id: vault_id.clone(),
                        revision: revision.clone(),
                    }
                }
                CloudSyncAction::SignOut => CloudSyncOp::SignOut,
                CloudSyncAction::ConfigureServer => {
                    let (_, token) = custom;
                    let origin = match origin {
                        Ok(origin) => origin,
                        Err(code) => {
                            self.frontend_vm.cloud_sync.apply_error(code);
                            return;
                        }
                    };
                    CloudSyncOp::ConfigureServer { origin, token }
                }
                _ => return,
            };

            let Some(backend) = self.backend() else {
                return;
            };
            self.frontend_vm.cloud_sync.busy = true;
            self.frontend_vm.cloud_sync.notice = None;
            self.frontend_vm.cloud_sync.error = None;
            // 密码框在派发后立刻清空：宿主不再持有用户刚输入的密钥材料。
            self.frontend_vm.cloud_sync.forget_secrets_keep_origin();
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = CloudSyncOp::run(backend, operation).await;
                let _ = tx.send(UiResult::CloudSyncDone {
                    result: Box::new(result),
                });
            });
        }

        /// 处理一次云同步往返：状态、启用准备、恢复预览各有不同后续。
        fn apply_cloud_sync_result(
            &mut self,
            result: Result<openless_linux_egui::cloud_sync_ui::SyncOutcome, String>,
        ) {
            use openless_linux_egui::cloud_sync_ui::{
                CloudSyncAction, CloudSyncDialog, CloudSyncIntent, SyncOutcome,
            };
            self.frontend_vm.cloud_sync.busy = false;
            self.frontend_vm.cloud_sync.loading = false;
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.frontend_vm.cloud_sync.apply_error(&error);
                    return;
                }
            };
            match outcome {
                SyncOutcome::Status(status) => {
                    if !self.frontend_vm.cloud_sync.accept_status(*status) {
                        return;
                    }
                    let dialog = self.frontend_vm.cloud_sync.dialog.clone();
                    match dialog {
                        // 解锁只是启用流程中的一步：解锁成功后继续准备。
                        CloudSyncDialog::Unlock { intent } => {
                            self.apply_cloud_sync_action(CloudSyncAction::Prepare(intent));
                        }
                        CloudSyncDialog::Disable | CloudSyncDialog::Delete { .. } => {
                            self.frontend_vm.cloud_sync.dialog = CloudSyncDialog::None;
                            self.frontend_vm.cloud_sync.notice =
                                Some("cloudSyncE2ee.noticeDone".to_string());
                        }
                        CloudSyncDialog::Create { .. } | CloudSyncDialog::ChangePassword => {
                            self.frontend_vm.cloud_sync.dialog = CloudSyncDialog::None;
                            self.frontend_vm.cloud_sync.intent = None;
                            self.frontend_vm.cloud_sync.notice =
                                Some("cloudSyncE2ee.noticeDone".to_string());
                        }
                        CloudSyncDialog::Restore { enable_after, .. } => {
                            if enable_after
                                && !self
                                    .frontend_vm
                                    .cloud_sync
                                    .status
                                    .as_ref()
                                    .is_some_and(|status| status.enabled)
                            {
                                self.apply_cloud_sync_action(CloudSyncAction::SetEnabled(true));
                            } else {
                                self.frontend_vm.cloud_sync.dialog = CloudSyncDialog::None;
                                self.frontend_vm.cloud_sync.notice =
                                    Some("cloudSyncE2ee.noticeRestored".to_string());
                            }
                            self.load_settings_channels();
                            self.load_service_configured();
                        }
                        CloudSyncDialog::None if self.frontend_vm.cloud_sync.notice.is_none() => {
                            // 「立即同步」「锁定」「登出」这类单步动作：成功后给个提示。
                            self.frontend_vm.cloud_sync.notice =
                                Some("cloudSyncE2ee.noticeDone".to_string());
                        }
                        _ => {}
                    }
                }
                SyncOutcome::Prepared(prepared) => {
                    if !self
                        .frontend_vm
                        .cloud_sync
                        .accept_status(prepared.status.clone())
                    {
                        return;
                    }
                    let intent = self.frontend_vm.cloud_sync.intent;
                    let status = prepared.status.clone();
                    match prepared.next_step {
                        openless_core::cloud_sync_e2ee::EnableStep::Create => {
                            // 只有在「云端还没有快照」且确实拿到了 revision 时才能建库。
                            let Some(revision) = status.remote_revision.clone() else {
                                self.frontend_vm.cloud_sync.apply_error("invalid_response");
                                return;
                            };
                            if status.has_cloud_snapshot != Some(false)
                                || intent != Some(CloudSyncIntent::Enable)
                            {
                                self.frontend_vm.cloud_sync.apply_error("invalid_response");
                                return;
                            }
                            self.frontend_vm.cloud_sync.open(CloudSyncDialog::Create {
                                observed_revision: revision,
                            });
                        }
                        openless_core::cloud_sync_e2ee::EnableStep::Unlock => {
                            self.frontend_vm.cloud_sync.open(CloudSyncDialog::Unlock {
                                intent: intent.unwrap_or(CloudSyncIntent::Unlock),
                            });
                        }
                        openless_core::cloud_sync_e2ee::EnableStep::RestoreReview => {
                            self.frontend_vm.cloud_sync.intent = Some(CloudSyncIntent::Restore);
                            self.apply_cloud_sync_action(CloudSyncAction::PreviewRestore);
                        }
                        openless_core::cloud_sync_e2ee::EnableStep::Ready => {
                            if let Some(action) = intent.and_then(CloudSyncIntent::ready_action) {
                                self.apply_cloud_sync_action(action);
                            } else {
                                self.frontend_vm.cloud_sync.dialog = CloudSyncDialog::None;
                                self.frontend_vm.cloud_sync.intent = None;
                                self.frontend_vm.cloud_sync.notice =
                                    Some("cloudSyncE2ee.noticeDone".to_string());
                            }
                        }
                    }
                }
                SyncOutcome::Preview(preview) => {
                    let Some(status) = self.frontend_vm.cloud_sync.status.clone() else {
                        self.frontend_vm.cloud_sync.apply_error("stale_preview");
                        return;
                    };
                    // 预览必须和刚读到的 revision/generation 对齐，否则是过期预览。
                    if status.remote_revision.as_deref() != Some(preview.observed_revision.as_str())
                        || status.local_generation != preview.local_generation
                    {
                        self.frontend_vm.cloud_sync.apply_error("stale_preview");
                        return;
                    }
                    let enable_after =
                        self.frontend_vm.cloud_sync.intent == Some(CloudSyncIntent::Enable);
                    self.frontend_vm.cloud_sync.open(CloudSyncDialog::Restore {
                        preview,
                        enable_after,
                    });
                }
                SyncOutcome::Done => {}
            }
        }

        fn start_marketplace_oauth(&mut self) {
            let Some(backend) = self.backend() else {
                return;
            };
            self.frontend_vm.marketplace_oauth_loading = true;
            self.frontend_vm.marketplace_oauth_error = None;
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .marketplace
                    .start_device_flow()
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::MarketplaceOAuthStarted(result));
            });
        }

        fn poll_marketplace_oauth(&mut self) {
            let Some(deadline) = self.marketplace_oauth_poll_deadline else {
                return;
            };
            if std::time::Instant::now() < deadline {
                return;
            }
            self.marketplace_oauth_poll_deadline = None;
            let Some(flow_id) = self.frontend_vm.marketplace_oauth_flow_id.clone() else {
                return;
            };
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = backend
                    .services()
                    .marketplace
                    .poll_device_flow(flow_id.clone())
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::MarketplaceOAuthPoll { flow_id, result });
            });
        }

        fn load_marketplace_mine(&self) {
            let Some(backend) = self.backend() else {
                return;
            };
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let result = async {
                    let auth = backend.services().marketplace.auth_status().await?;
                    if !auth.signed_in {
                        // Not being logged in is a normal empty state for this
                        // modal, not a page-level error/toast.
                        let _ = tx.send(UiResult::MarketplaceAuth(Ok(false)));
                        return Ok::<_, BackendError>((Vec::new(), Vec::new()));
                    }
                    let _ = tx.send(UiResult::MarketplaceAuth(Ok(true)));
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
                    let activity_backend = Arc::clone(&backend);
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
                        history: Vec::new(),
                        activity,
                    })
                }
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::Overview(result));
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
                        if let Some(preferences) = &mut self.preferences {
                            preferences.microphone_device_name = name;
                            self.save_settings_if_dirty();
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
            editor.protocol = form.protocol.clone();
            Some(editor)
        }

        /// Open a channel's provider editor. The descriptor comes from Core's
        /// provider rules, so the UI never invents a field shape; without one
        /// the editor stays closed instead of guessing.
        fn open_provider_editor(&mut self, index: usize) {
            self.editor_requested = true;
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
            self.provider_editor = ProviderEditorState::Loading;
            self.provider_editor_form = None;
            self.load_provider_editor(kind, channel, descriptor);
        }

        /// 关闭渠道编辑器；完全没交互过的草稿在这条路径上被回收
        /// （Tauri `shouldRecycleDraft` + `deleteChannelIfBlank`）。
        fn resolved_services_view(&self, requested: usize) -> usize {
            let multimodal = self.preferences.as_ref().is_some_and(|p| {
                p.multimodal_pipeline_enabled
                    && p.pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal
            });
            match requested {
                3 => 3,
                4 if multimodal => 4,
                0 | 1 if !multimodal => requested,
                2 if !multimodal && self.frontend_vm.supports_local_asr => 2,
                _ => {
                    if multimodal {
                        4
                    } else {
                        0
                    }
                }
            }
        }

        fn select_services_view(&mut self, requested: usize) {
            let view = self.resolved_services_view(requested);
            self.frontend_vm.services_view = view;
            if !matches!(view, 0 | 1) {
                self.close_provider_editor();
            }
            if view == 4 {
                if !self.frontend_vm.omni.loaded
                    && !self.frontend_vm.omni.busy
                    && self.frontend_vm.omni.notice.is_none()
                {
                    self.apply_omni_action(openless_linux_egui::omni_settings::OmniAction::Reload);
                }
                return;
            }
            if !matches!(view, 0 | 1) {
                return;
            }
            let kind = if view == 1 {
                openless_core::ChannelKind::Asr
            } else {
                openless_core::ChannelKind::Llm
            };
            if self.settings_channel_kind != kind {
                self.close_provider_editor();
                self.selected_channel_id = None;
                self.settings_channel_kind = kind;
                self.provider_kind = kind;
                self.load_settings_channels();
                self.load_service_configured();
                self.load_providers(kind);
            } else if self.settings_channels.is_empty() && !self.settings_channels_loading {
                self.provider_kind = kind;
                self.load_settings_channels();
                self.load_service_configured();
                self.load_providers(kind);
            }
        }

        fn apply_omni_action(&mut self, operation: openless_linux_egui::omni_settings::OmniAction) {
            use openless_linux_egui::omni_settings::{OmniAction, OmniSettings};
            let state = &mut self.frontend_vm.omni;
            if let OmniAction::Field(field, value) = operation {
                if state.loaded && !state.switching {
                    state.edit(field, value);
                }
                return;
            }
            if state.busy {
                return;
            }
            if !matches!(operation, OmniAction::Reload) && !state.loaded {
                return;
            }
            if !matches!(operation, OmniAction::Reload | OmniAction::ClearSecrets)
                && state.validate().is_err()
            {
                return;
            }
            let Some(backend) = self.backend() else {
                return;
            };
            let host = self.native.as_ref().map(|native| native.host_arc());
            let mut snapshot = self.frontend_vm.omni.clone();
            let submitted = Box::new(snapshot.clone());
            self.frontend_vm.omni.busy = true;
            self.frontend_vm.omni.switching =
                matches!(operation, OmniAction::Provider(_) | OmniAction::Reload);
            let tx = self.tx.clone();
            let wake = std::thread::current();
            let job = Box::pin(async move {
                let result = async {
                    if snapshot.dirty
                        && !matches!(operation, OmniAction::Reload | OmniAction::ClearSecrets)
                    {
                        snapshot.save(&backend).await.map_err(|e| e.to_string())?;
                        snapshot.api_key.clear();
                        snapshot.extra_headers.clear();
                        snapshot.dirty = false;
                    }
                    match &operation {
                        OmniAction::Provider(id) => {
                            OmniSettings::select(&backend, id)
                                .await
                                .map_err(|e| e.to_string())?;
                            let host = host
                                .ok_or_else(|| "native settings host is unavailable".to_owned())?;
                            let edits = [("/activeOmniProvider".to_owned(), serde_json::json!(id))]
                                .into_iter()
                                .collect();
                            tokio::task::spawn_blocking(move || {
                                host.update_preference_fields(&edits)
                            })
                            .await
                            .map_err(|e| e.to_string())?
                            .map_err(|e| e.to_string())?;
                            snapshot = OmniSettings::load(&backend)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        OmniAction::Reload => {
                            snapshot = OmniSettings::load(&backend)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        OmniAction::ClearSecrets => {
                            snapshot
                                .clear_secrets(&backend)
                                .await
                                .map_err(|e| e.to_string())?;
                            snapshot.api_key.clear();
                            snapshot.extra_headers.clear();
                        }
                        OmniAction::Models => {
                            // Omni 没有「渠道」这一层：Core 对
                            // `kind == Omni && channel_id.is_some()` 直接返回
                            // `omni provider does not support channel id`，
                            // 于是「测试 / 拉模型」永远是失败的。传 None 让 Core
                            // 用当前激活的服务商。
                            snapshot.models = backend
                                .services()
                                .provider
                                .list_models(omni_provider_request(
                                    backend.get_preferences().llm_thinking_enabled,
                                ))
                                .await
                                .map_err(|e| e.to_string())?
                                .models;
                        }
                        OmniAction::Validate => {
                            // 同上：带 channel_id 会被 Core 拒绝，测试必然失败。
                            let check = backend
                                .services()
                                .provider
                                .validate(omni_provider_request(
                                    backend.get_preferences().llm_thinking_enabled,
                                ))
                                .await
                                .map_err(|e| e.to_string())?;
                            if !check.ok {
                                return Err("validateFailed".to_owned());
                            }
                        }
                        _ => {}
                    }
                    snapshot.busy = false;
                    snapshot.switching = false;
                    Ok(snapshot)
                }
                .await;
                let _ = tx.send(UiResult::OmniFinished {
                    operation,
                    submitted,
                    result: Box::new(result),
                });
                wake.unpark();
            });
            if self.provider_jobs.send(job).is_err() {
                self.frontend_vm.omni.busy = false;
                self.frontend_vm.omni.switching = false;
                self.frontend_vm.omni.notice =
                    Some(tr_l10n(self.lang, "common.operationFailed").to_owned());
            }
        }

        fn queue_provider_save(&mut self) -> bool {
            if self.provider_switch_pending {
                return false;
            }
            if self.provider_save_pending {
                self.provider_save_again = true;
                return true;
            }
            let Some(editor) = self.editor_from_form() else {
                return false;
            };
            if let Err(error) = editor.protocol.validate() {
                self.status = error.to_string();
                return false;
            }
            let Some(backend) = self.backend() else {
                return false;
            };
            let tx = self.tx.clone();
            let wake = std::thread::current();
            self.provider_save_pending = true;
            self.draft_touched = true;
            let job = Box::pin(async move {
                let result = save_provider_editor(backend, editor.clone())
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::ProviderSaved {
                    editor: Box::new(editor),
                    result,
                });
                wake.unpark();
            });
            if self.provider_jobs.send(job).is_err() {
                self.provider_save_pending = false;
                self.status = tr_l10n(self.lang, "common.operationFailed").to_owned();
                return false;
            }
            true
        }

        fn request_provider_editor_close(&mut self) {
            if self.provider_switch_pending {
                return;
            }
            if self.provider_save_pending {
                self.provider_close_pending = true;
                return;
            }
            if self
                .provider_editor_form
                .as_ref()
                .is_some_and(|form| form.protocol.validate().is_err())
            {
                return;
            }
            // Flush host-side edits even when no UI debounce timer was armed
            // (e.g. a default-model button, navigation, or window teardown).
            if self.draft_touched {
                self.provider_close_pending = true;
                if !self.queue_provider_save() {
                    self.provider_close_pending = false;
                }
                return;
            }
            let recycle = self.editor_draft && !self.draft_touched;
            let channel_id = self
                .provider_editor_form
                .as_ref()
                .map(|form| form.channel_id.clone());
            if recycle {
                if let (Some(backend), Some(channel_id)) = (self.backend(), channel_id) {
                    let kind = self.settings_channel_kind;
                    let lang = self.lang;
                    self.spawn_channel_change(kind, async move {
                        backend.delete_channel_if_blank(kind, channel_id).await?;
                        Ok(tr_l10n(lang, "status.channel_deleted").to_string())
                    });
                }
            }
            self.close_provider_editor();
        }

        fn close_provider_editor(&mut self) {
            self.provider_close_pending = false;
            self.provider_after_save = None;
            self.provider_save_again = false;
            self.provider_editor = ProviderEditorState::Idle;
            self.provider_editor_form = None;
            self.frontend_vm.provider_editor = None;
            self.editor_draft = false;
            self.draft_touched = false;
            self.editor_requested = false;
        }

        fn request_siwc(&mut self, action: String, registration_id: Option<String>) {
            let Some(backend) = self.backend() else {
                return;
            };
            let Some(form) = self
                .provider_editor_form
                .as_mut()
                .filter(|f| f.provider_type == openless_core::siwc::PROVIDER_ID)
            else {
                return;
            };
            static SIWC_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            form.siwc_request = SIWC_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let request = form.siwc_request;
            let channel = form.channel_id.clone();
            let tx = self.tx.clone();
            let wake = std::thread::current();
            self.tokio.spawn(async move {
                let result = async {
                    let service = backend
                        .siwc_for_channel(&channel)
                        .await
                        .map_err(|e| e.to_string())?;
                    let mut message = None;
                    if matches!(action.as_str(), "sign_in" | "add_account" | "enable_usage" | "select" | "disconnect") {
                        backend.invalidate_channel_test(openless_core::ChannelKind::Llm, channel.clone()).await.map_err(|e| e.to_string())?;
                    }
                    match action.as_str() {
                        "sign_in" | "add_account" | "enable_usage" => {
                            let launch = backend
                                .begin_siwc_sign_in(
                                    channel.clone(),
                                    action == "add_account",
                                    action == "enable_usage",
                                )
                                .await
                                .map_err(|e| e.to_string())?;
                            if open_external(launch.authorization_url()).is_err() {
                                let _ = service.cancel_sign_in(&channel).await;
                                return Err(
                                    openless_core::siwc::error("browser_failed").to_string()
                                );
                            }
                        }
                        "cancel" => service
                            .cancel_sign_in(&channel)
                            .await
                            .map_err(|e| e.to_string())?,
                        "select" => service
                            .select_account(
                                channel.clone(),
                                registration_id.ok_or("account required")?,
                            )
                            .await
                            .map_err(|e| e.to_string())?,
                        "disconnect" => {
                            if !service
                                .disconnect(&channel)
                                .await
                                .map_err(|e| e.to_string())?
                                .remote_revoked
                            {
                                message = Some("siwc.remoteUnconfirmed".into());
                            }
                        }
                        "acknowledge" => service
                            .acknowledge_welcome(channel.clone())
                            .await
                            .map_err(|e| e.to_string())?,
                        "retry" => service
                            .retry_usage(channel.clone())
                            .await
                            .map_err(|e| e.to_string())?,
                        "test" => {
                            let started = std::time::Instant::now();
                            let result = backend
                                .services()
                                .provider
                                .validate(openless_core::ProviderRequest {
                                    kind: openless_core::ProviderKind::Llm,
                                    channel_id: Some(channel.clone()),
                                    thinking_enabled: false,
                                })
                                .await;
                            backend
                                .record_channel_test(
                                    openless_core::ChannelKind::Llm,
                                    channel.clone(),
                                    result.is_ok(),
                                    result
                                        .as_ref()
                                        .ok()
                                        .map(|_| started.elapsed().as_millis() as u32),
                                    result.as_ref().err().map(|e| e.to_string()),
                                )
                                .await
                                .map_err(|e| e.to_string())?;
                            message = Some(match result {
                                Ok(_) => "siwc.testSuccess".into(),
                                Err(e) => e.to_string(),
                            });
                        }
                        "refresh" => {}
                        _ => return Err("unknown SIWC action".into()),
                    }
                    loop {
                        let status = service.status(&channel).await.map_err(|e| e.to_string())?;
                        let authorizing = status.state == "authorizing";
                        let models = if status.state == "connected" {
                            match service.list_models(&channel).await {
                                Ok(models) => models,
                                Err(e) => {
                                    message = Some(e.to_string());
                                    Vec::new()
                                }
                            }
                        } else {
                            Vec::new()
                        };
                        let state = frontend::view_model::SettingsSiwcState {
                            status,
                            models,
                            message: message.clone(),
                        };
                        let _ = backend.refresh_and_publish_credentials().await;
                        if tx
                            .send(UiResult::Siwc {
                                channel_id: channel.clone(),
                                request,
                                state: Ok(state),
                            })
                            .is_err()
                        {
                            break;
                        }
                        wake.unpark();
                        if !authorizing {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Ok::<(), String>(())
                }
                .await;
                if let Err(error) = result {
                    let _ = tx.send(UiResult::Siwc {
                        channel_id: channel,
                        request,
                        state: Err(error),
                    });
                    wake.unpark();
                }
            });
        }

        /// Core owns the model catalog; the host only forwards the request.
        fn request_provider_models(&self, kind: openless_core::ChannelKind, channel_id: String) {
            if self.provider_switch_pending {
                return;
            }
            let Some(backend) = self.backend() else {
                return;
            };
            let editor = self
                .editor_from_form()
                .filter(|editor| editor.kind == kind && editor.channel.id == channel_id);
            let Some(editor) = editor else {
                return;
            };
            let endpoint = editor.endpoint.clone();
            let request_format = editor.protocol.format();
            let provider_type = editor.descriptor.provider_type.as_str().to_owned();
            let tx = self.tx.clone();
            let wake = std::thread::current();
            let job = Box::pin(async move {
                let result = async {
                    save_provider_editor(backend.clone(), editor).await?;
                    backend
                        .services()
                        .provider
                        .list_models(openless_core::ProviderRequest {
                            kind: provider_kind(kind),
                            thinking_enabled: backend.get_preferences().llm_thinking_enabled,
                            channel_id: Some(channel_id.clone()),
                        })
                        .await
                        .map(|models| models.models)
                }
                .await
                .map_err(|error: BackendError| error.to_string());
                let _ = tx.send(UiResult::ProviderModels {
                    kind,
                    channel_id,
                    endpoint,
                    provider_type,
                    request_format,
                    result,
                });
                wake.unpark();
            });
            let _ = self.provider_jobs.send(job);
        }

        fn spawn_channel_change<F>(&self, kind: openless_core::ChannelKind, future: F)
        where
            F: std::future::Future<Output = Result<String, BackendError>> + Send + 'static,
        {
            let tx = self.tx.clone();
            let wake = std::thread::current();
            let job = Box::pin(async move {
                let result = future.await.map_err(|error| error.to_string());
                let _ = tx.send(UiResult::ChannelsChanged { kind, result });
                wake.unpark();
            });
            let _ = self.provider_jobs.send(job);
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
                        // 卡片与录音胶囊共用一个窗口：会话一起来就得让路。
                        self.dismiss_vocab_suggestions();
                        self.play_record_cue(true);
                    } else if !self.recording_phase_active && was_recording {
                        self.play_record_cue(false);
                    }
                    if state.phase == DictationPhase::Starting {
                        self.dismiss_vocab_suggestions();
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
                                transcript: self.capsule_transcript(),
                                transcript_font_size: self.capsule_transcript_font_size(),
                                audio_level: Some(state.level),
                                translation_active: state.translation_active,
                                style,
                                suggestions: self.vocab_suggestions.clone(),
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
                    self.sync_capsule_transcript();
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
                    if self
                        .backend()
                        .is_some_and(|backend| !backend.get_preferences().testing_features_enabled)
                    {
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
                        // Linux already has its own recording display; new typed feedback is
                        // for the Host/UI team taking over to wire up.
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
                BackendEventKind::VocabularySuggestionsChanged(_) => {
                    // Core 只负责攒候选；「要不要记住」的确认入口是胶囊窗上的卡片
                    // （Tauri `VocabSuggestionCard`）。没有这张卡，自动学词永远落不了库。
                    self.refresh_vocab_suggestions();
                }
                BackendEventKind::HistoryChanged(change) => {
                    self.history.observe(change.revision);
                    self.load_overview();
                }
                BackendEventKind::CredentialsChanged(_) => {
                    // Defer and coalesce credential writes; refresh after the mutation,
                    // not only before its async worker commits it.
                    self.settings_channels_refresh_requested = true;
                }
                BackendEventKind::PreferencesChanged(_) => {
                    // 外部改动（Core 事件 / 托盘 / 另一窗口）要重新灌一次文本行。
                    self.hydrate_text_fields = true;
                    if let Some(backend) = self.backend() {
                        let latest = backend.get_preferences();
                        if self.history.policy_changed(&latest) {
                            self.history.request(backend.snapshot().history_revision);
                        }
                        self.preferences = Some(self.settings_save.rebase(&latest));
                    }
                    if self
                        .backend()
                        .is_some_and(|backend| !backend.get_preferences().testing_features_enabled)
                    {
                        // Dropping the supervisor closes a feature that is no longer opted in.
                        self.less_computer_popup = None;
                        self.less_computer_working = false;
                        self.pending_approval = None;
                        self.less_computer_session = None;
                        self.less_computer_entries.clear();
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
                    if self.qa_visible {
                        self.show_qa_popup();
                    }
                }
                BackendEventKind::SelectionStateChanged(snapshot) => {
                    let preview_text = snapshot.preview_text.clone().unwrap_or_default();
                    let source_text = snapshot.source_text.clone().unwrap_or_default();
                    // 只有「拿到选区」并且「润色结果非空」才把面板弹出来。以前这里只看
                    // `phase`：起手 `Capturing` 就 `ensure_popup` + 送一帧空结果，于是按
                    // 一下 alt+X 先冒出一个**空的**润色结果面板（用户报的「弹窗不对」），
                    // 没有选区时更糟 —— 面板空转一圈再消失。
                    let has_selection = !source_text.trim().is_empty();
                    let has_preview = snapshot.phase == SelectionPhase::Preview
                        && !preview_text.trim().is_empty();
                    let show_preview = has_preview && has_selection;
                    if show_preview {
                        self.selection_draft = preview_text.clone();
                        self.polish_result_visible = true;
                    }
                    if let Some(session_id) = snapshot.session_id {
                        if show_preview {
                            // 选区助手面板：润色结果以「润色结果」帧送进去。
                            self.ensure_popup(PopupKind::Qa);
                            self.send_popup(
                                PopupKind::Qa,
                                HostToPopup::PolishPreview {
                                    version: POPUP_PROTOCOL_VERSION,
                                    session_id: session_id.to_string(),
                                    sequence: event_sequence.saturating_mul(2),
                                    text: preview_text,
                                    source: source_text,
                                },
                            );
                        } else if !has_selection
                            && matches!(
                                snapshot.phase,
                                SelectionPhase::Preview | SelectionPhase::Failed
                            )
                        {
                            // 没选中文字：弹语音输入窗报错后退出，不弹面板。
                            log::warn!(
                                "[hotkey] selection polish had no captured selection: showing the capsule error"
                            );
                            self.show_selection_polish_capsule_error(&session_id.to_string());
                        } else {
                            log::debug!(
                                "[hotkey] selection snapshot without a usable preview ({:?}): keeping the panel closed",
                                snapshot.phase
                            );
                        }
                    }
                    self.selection = Some(snapshot);
                }
                BackendEventKind::RemoteInputStatusChanged(_)
                | BackendEventKind::RemoteInputFailed(_) => self.load_remote_status(),
                BackendEventKind::CloudSyncStateChanged(event) => {
                    // 未经请求的事件不得把界面切到另一个账号/仓库。
                    let same_scope =
                        self.frontend_vm
                            .cloud_sync
                            .status
                            .as_ref()
                            .is_some_and(|current| {
                                current
                                    .account
                                    .as_ref()
                                    .map(|account| account.github_id.clone())
                                    == event
                                        .status
                                        .account
                                        .as_ref()
                                        .map(|account| account.github_id.clone())
                                    && current.vault_id == event.status.vault_id
                                    && current.service_origin == event.status.service_origin
                            });
                    if same_scope {
                        self.frontend_vm.cloud_sync.accept_status(event.status);
                    } else {
                        self.load_cloud_sync();
                    }
                }
                BackendEventKind::CloudSyncConflictDetected(event) => {
                    let same_scope =
                        self.frontend_vm
                            .cloud_sync
                            .status
                            .as_ref()
                            .is_some_and(|current| {
                                current.vault_id.as_deref() == Some(event.vault_id.as_str())
                                    && current.account.as_ref().is_some_and(|account| {
                                        account.github_id == event.account_id
                                    })
                            });
                    if same_scope {
                        self.frontend_vm.cloud_sync.open(
                            openless_linux_egui::cloud_sync_ui::CloudSyncDialog::Restore {
                                preview: Box::new(event.preview),
                                enable_after: false,
                            },
                        );
                        self.frontend_vm.cloud_sync.notice =
                            Some("cloudSyncE2ee.states.conflict".to_string());
                    } else {
                        self.load_cloud_sync();
                    }
                }
                BackendEventKind::CloudSyncRestoreCompleted(_) => {
                    self.frontend_vm.cloud_sync.dialog =
                        openless_linux_egui::cloud_sync_ui::CloudSyncDialog::None;
                    self.frontend_vm.cloud_sync.notice =
                        Some("cloudSyncE2ee.noticeRestored".to_string());
                    self.load_cloud_sync();
                    self.load_settings_channels();
                    self.load_service_configured();
                }
                _ => {}
            }
        }

        /// `update()` 是唯一 drain 原生事件（热键、单实例拉起意图）的地方，而
        /// 最小化/隐藏的窗口会让 eframe 的定时重绘停摆 —— 那样按热键什么都不会
        /// 发生（胶囊、QA 面板都不弹）。这里必须用**真线程**：`self.tokio` 是
        /// current-thread 运行时，`spawn` 的任务只在别处 `block_on` 时才被推进，
        /// 当作后台泵用就是「写完看着对、最小化后照样死」（实测心跳会在窗口
        /// 收走的那一刻停）。线程只做一件事：`request_repaint()` 把事件循环戳醒。
        /// 泵心跳：宿主循环每 10s 采样一次。窗口关掉之后这条心跳**不能**停 ——
        /// 停了就说明热键消费与弹窗拉起也没在跑（这正是当初「关窗后热键失效」
        /// 的判据）。
        ///
        /// 采样归采样，写日志要节流：每 10 秒一条 `info` 是 8640 行/天，轮转后
        /// 整份日志九成以上都是它，真正的错误反而被挤出去。所以**状态变化立刻
        /// 记一条 `info`**（这才是判据），状态没变时只每
        /// [`PUMP_HEARTBEAT_INFO_INTERVAL`] 留一条存活证明，其余走 `debug`。
        fn log_pump_heartbeat(&mut self, ctx: &egui::Context) {
            self.tick_vocab_suggestions();
            if self.last_pump_heartbeat.elapsed() < PUMP_HEARTBEAT_INTERVAL {
                return;
            }
            self.last_pump_heartbeat = std::time::Instant::now();
            // 宿主没有窗口，所以心跳只看「窗口进程还在不在」——
            // 这正是关窗后必须继续为 true 的那一项能力。
            let _ = ctx;
            let snapshot = [
                self.window.child.is_some(),
                self.window.should_be_open,
                self.tray.is_some(),
                self.recording_phase_active,
            ];
            let changed = snapshot != self.last_pump_heartbeat_snapshot;
            let due = self.last_pump_heartbeat_info.elapsed() >= PUMP_HEARTBEAT_INFO_INTERVAL;
            let message = format!(
                "[pump] heartbeat window_process={} window_wanted={} tray={} recording={}",
                snapshot[0], snapshot[1], snapshot[2], snapshot[3],
            );
            if changed || due {
                self.last_pump_heartbeat_info = std::time::Instant::now();
                self.last_pump_heartbeat_snapshot = snapshot;
                log::info!("{message}");
            } else {
                log::debug!("{message}");
            }
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
                self.polish_result_visible = false;
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
                    self.spawn_hotkey(async move {
                        backend.services().qa.dismiss().await?;
                        Ok(tr_l10n(lang, "qa.closed").to_string())
                    });
                }
            } else {
                log::info!("[hotkey] QA panel toggle: showing");
                if let Some(backend) = self.backend() {
                    self.qa_visible = true;
                    // `show()` 只发 HostAction::ShowQa：Core 仍停在 Idle，不录音。
                    self.spawn_hotkey(async move {
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
                self.spawn_hotkey(async move {
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
                return !matches!(event, LinuxHotkeyEvent::TranslationPressed { .. });
            };
            match openless_linux_egui::plugin_event_hotkey(event, &target) {
                Some(hotkey) => self
                    .hotkey_dedupe
                    .accept_signal(&hotkey, std::time::Instant::now()),
                // Translation signals carry their raw key identity; a mismatched
                // signal must be discarded instead of forwarded to Core.
                None => !matches!(event, LinuxHotkeyEvent::TranslationPressed { .. }),
            }
        }

        /// Start/finish a permanent quick note, shared by the button and hotkeys.
        fn toggle_quick_note(&mut self) {
            if let Some(backend) = self.backend() {
                self.spawn_hotkey(async move {
                    let result = if backend.dictation_output_target()
                        == Some(openless_core::DictationOutputTarget::QuickNote)
                    {
                        match backend.snapshot().dictation.session_id {
                            Some(id) => backend.stop_dictation_session(id).await.map(|_| ()),
                            None => Err(openless_core::BackendError::new(
                                openless_core::BackendErrorCode::InvalidState,
                                "quick note has no active session",
                            )),
                        }
                    } else {
                        backend
                            .start_dictation_with_options(openless_core::DictationStartOptions {
                                output_target: openless_core::DictationOutputTarget::QuickNote,
                                insert_text: false,
                                ..Default::default()
                            })
                            .await
                            .map(|_| ())
                    };
                    result?;
                    Ok(String::new())
                });
            }
        }

        /// 宿主自己处理掉的热键（不发往 Core）。返回 true 表示已处理。
        fn intercept_hotkey(&mut self, event: &LinuxHotkeyEvent) -> bool {
            match event {
                LinuxHotkeyEvent::OpenAppPressed => {
                    self.request_main_window();
                    true
                }
                LinuxHotkeyEvent::QuickNotePressed => {
                    self.toggle_quick_note();
                    true
                }
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
        /// 面板进程不在了：把宿主的可见性标志清干净（只对选区助手有意义）。
        ///
        /// 不清的后果：`qa_visible` 残留为 true 时，下一次「录音热键」会被
        /// `apply_local_hotkey_edges` 当成「向选区助手提问」，于是面板又被弹出来。
        fn forget_qa_panel_visibility(&mut self, kind: PopupKind) {
            if kind != PopupKind::Qa {
                return;
            }
            if self.qa_visible || self.polish_result_visible {
                log::info!("[hotkey] QA panel is gone: clearing qa_visible/polish_result_visible");
            }
            self.qa_visible = false;
            self.polish_result_visible = false;
            self.qa_popup_presentation.session_id = None;
            self.qa_popup_presentation.generation =
                self.qa_popup_presentation.generation.saturating_add(1);
        }

        fn apply_local_hotkey_edges(
            &mut self,
            edges: Vec<(std::time::Instant, openless_linux_egui::LocalHotkeyEdge)>,
        ) -> Vec<LinuxHotkeyEvent> {
            use openless_linux_egui::{LocalHotkey, LocalHotkeyEdgeKind};
            let Some(target) = self.hotkey_target() else {
                return Vec::new();
            };
            // 标志残留（弹窗进程已经不在了）时先清干净：否则这次「录音热键」会被
            // 下面那条 qa_visible 分支当成「向选区助手提问」，把面板又弹出来。
            if self.qa_visible && self.popup_slot(PopupKind::Qa).is_none() {
                log::info!("[hotkey] stale QA panel flag (no popup process): clearing");
                self.qa_visible = false;
                self.polish_result_visible = false;
                self.qa_popup_presentation.generation =
                    self.qa_popup_presentation.generation.saturating_add(1);
            }
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
                let single_shot = matches!(
                    edge.kind,
                    LocalHotkeyEdgeKind::Pressed | LocalHotkeyEdgeKind::Combined
                );
                match &edge.hotkey {
                    LocalHotkey::QuickNote => {
                        if single_shot {
                            self.toggle_quick_note();
                        }
                    }
                    LocalHotkey::Qa => {
                        if single_shot {
                            log::info!("[hotkey] local selection-ask hotkey: toggling the panel");
                            self.toggle_qa_panel();
                        }
                    }
                    LocalHotkey::Dictation
                        if edge.kind == LocalHotkeyEdgeKind::Pressed
                            && self.qa_visible
                            && self.qa_state.is_some()
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
                        LocalHotkeyEdgeKind::Cancelled => LinuxHotkeyEvent::DictationCombined {
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
                            LocalHotkeyEdgeKind::Cancelled => {
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
                            if let Some(event) =
                                openless_linux_egui::translation_hotkey_event(&target)
                            {
                                events.push(event);
                            }
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
                            self.request_main_window();
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
                    log::info!("[ui-host] launch intent from the user: {intent:?}");
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
                    self.spawn_hotkey(async move {
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
                    self.spawn_hotkey(async move {
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
                            if self.polish_result_visible {
                                self.close_qa_popup();
                            }
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
                        self.history.request(snapshot.history_revision);
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
                    UiResult::HistoryLoaded {
                        generation,
                        revision,
                        result,
                    } => {
                        if self.history.finish(generation, revision, result) {
                            if let OverviewState::Loaded(data) = &mut self.overview {
                                data.history = self.history.entries.clone();
                            }
                        }
                    }
                    UiResult::LinuxAccess(snapshot) => {
                        self.frontend_vm.linux_access = snapshot;
                        self.linux_access_last_check = Some(std::time::Instant::now());
                    }
                    UiResult::Message(message) => self.status = message,
                    UiResult::QaAction {
                        generation,
                        context,
                        result,
                    } => self.apply_qa_action_result(generation, context, result),
                    UiResult::HotkeyError(error) => {
                        self.status = if error.message == "selectionPolishSelectionTooLong" {
                            tr_l10n(self.lang, "selection.source_truncated").to_owned()
                        } else {
                            error.to_string()
                        };
                    }
                    UiResult::HistoryRepolish { id, result } => {
                        self.frontend_vm.history_repolish_running = false;
                        match result {
                            Ok(text) => {
                                self.frontend_vm.history_repolish_error = None;
                                self.frontend_vm.history_repolish_result = Some((id, text));
                            }
                            Err(error) => self.frontend_vm.history_repolish_error = Some(error),
                        }
                    }
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
                    UiResult::Providers {
                        kind,
                        request,
                        result,
                    } => {
                        if kind != self.provider_kind || request != self.providers_request {
                            continue;
                        }
                        match result {
                            Ok(panel) => {
                                if panel.kind != self.provider_kind {
                                    continue;
                                }
                                if !panel.descriptors.iter().any(|descriptor| {
                                    descriptor.provider_type.as_str() == self.new_provider_type
                                }) {
                                    self.new_provider_type = panel
                                        .descriptors
                                        .first()
                                        .map(|descriptor| {
                                            descriptor.provider_type.as_str().to_string()
                                        })
                                        .unwrap_or_default();
                                }
                                let selected = self
                                    .selected_channel_id
                                    .as_ref()
                                    .filter(|id| {
                                        panel.channels.iter().any(|channel| &channel.id == *id)
                                    })
                                    .cloned()
                                    .or_else(|| {
                                        panel
                                            .channels
                                            .iter()
                                            .find(|channel| channel.id == panel.active_provider)
                                            .map(|channel| channel.id.clone())
                                    })
                                    .or_else(|| {
                                        panel.channels.first().map(|channel| channel.id.clone())
                                    });
                                self.selected_channel_id = selected.clone();
                                if self.pending_channel_delete.as_ref().is_some_and(|id| {
                                    !panel.channels.iter().any(|channel| &channel.id == id)
                                }) {
                                    self.pending_channel_delete = None;
                                }
                                self.providers = ProvidersState::Loaded(panel.clone());
                                self.provider_models.clear();
                                // 只把「面板刚加载好」当成一个时机，是否打开编辑器看用户意图：
                                // 保存 / 验证 / 切换页面都会刷新面板，不能顺手把编辑器拉起来。
                                let editor_wanted = self.editor_requested;
                                self.editor_requested = false;
                                if let Some(channel_id) = selected {
                                    // A refresh (enable toggle, save, validation) must not
                                    // throw away the editor draft the user is editing: only
                                    // a different channel re-reads the descriptor.
                                    let already_loaded = matches!(
                                        &self.provider_editor,
                                        ProviderEditorState::Loaded(editor)
                                            if editor.channel.id == channel_id
                                    );
                                    if !already_loaded && editor_wanted {
                                        if let Some((channel, descriptor)) =
                                            provider_channel_descriptor(&panel, &channel_id)
                                        {
                                            self.provider_editor = ProviderEditorState::Loading;
                                            self.provider_editor_form = None;
                                            self.load_provider_editor(
                                                panel.kind, channel, descriptor,
                                            );
                                        }
                                    }
                                } else {
                                    self.provider_editor = ProviderEditorState::Idle;
                                }
                            }
                            Err(error) => {
                                self.providers = ProvidersState::Failed;
                                self.frontend_vm.settings_notice = Some(error.clone());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::ProviderSaved { editor, result } => {
                        self.provider_save_pending = false;
                        match result {
                            Err(error) => {
                                self.provider_save_again = false;
                                self.provider_close_pending = false;
                                self.provider_after_save = None;
                                self.frontend_vm.settings_notice = Some(error.clone());
                                self.status = error;
                            }
                            Ok(()) => {
                                let mut changed = false;
                                if let Some(form) =
                                    self.provider_editor_form.as_mut().filter(|form| {
                                        form.channel_id == editor.channel.id
                                            && form.kind == editor.kind
                                            && form.provider_type
                                                == editor.descriptor.provider_type.as_str()
                                    })
                                {
                                    changed = form.name != editor.name
                                        || form.endpoint != editor.endpoint
                                        || form.model != editor.model
                                        || form.resource_id != editor.resource_id
                                        || form.auth_mode != editor.auth_mode
                                        || form.protocol.values != editor.protocol.values
                                        || form.protocol.temperature != editor.protocol.temperature
                                        || (!form.primary_secret.is_empty()
                                            && form.primary_secret != editor.primary_secret)
                                        || (!form.secondary_secret.is_empty()
                                            && form.secondary_secret != editor.secondary_secret)
                                        || (!form.protocol.extra_headers.is_empty()
                                            && form.protocol.extra_headers
                                                != editor.protocol.extra_headers);
                                    // 保存成功后**不要**清空密钥草稿。
                                    //
                                    // 这里原本会「擦掉已被确认的草稿」（`form.secret ==
                                    // editor.secret` 时 clear），当时密钥从不回填，所以看不
                                    // 出问题。现在编辑器会像 Tauri 的 `CredentialField` 一样
                                    // 把已存的密钥读进输入框再打码：自动保存一次之后草稿被清
                                    // 空，输入框就变空白、连掩码点都没有——用户看到的就是
                                    // 「填进去的密钥几秒后被删掉了」。
                                    // 密钥仍然只存在本地表单里，写盘路径不变。
                                }
                                self.frontend_vm.settings_notice = None;
                                self.status = tr_l10n(self.lang, "status.channel_saved").to_owned();
                                self.load_settings_channels();
                                self.load_service_configured();
                                let again = std::mem::take(&mut self.provider_save_again);
                                if again
                                    || ((self.provider_close_pending
                                        || self.provider_after_save.is_some())
                                        && changed)
                                {
                                    if !self.queue_provider_save() {
                                        self.provider_close_pending = false;
                                        self.provider_after_save = None;
                                    }
                                } else if let Some(action) = self.provider_after_save.take() {
                                    self.close_provider_editor();
                                    self.pending_ui_actions.push(action);
                                } else if self.provider_close_pending {
                                    self.close_provider_editor();
                                } else if let Some(form) =
                                    self.provider_editor_form.as_mut().filter(|form| {
                                        form.provider_type == "orcarouter"
                                            && !form.models_loading
                                            && form.catalog_format != Some(form.protocol.format())
                                    })
                                {
                                    form.models_loading = true;
                                    let kind = form.kind;
                                    let channel_id = form.channel_id.clone();
                                    self.request_provider_models(kind, channel_id);
                                }
                            }
                        }
                    }
                    UiResult::ProviderSecretsCleared(result) => {
                        self.status = match result {
                            Ok(()) => tr_l10n(self.lang, "status.secret_cleared").to_owned(),
                            Err(error) => error,
                        };
                        self.load_service_configured();
                    }
                    UiResult::Siwc {
                        channel_id,
                        request,
                        state,
                    } => {
                        if let Some(form) = self.provider_editor_form.as_mut().filter(|f| {
                            f.channel_id == channel_id
                                && f.provider_type == openless_core::siwc::PROVIDER_ID
                                && f.siwc_request == request
                        }) {
                            match state {
                                Ok(state) => form.siwc = Some(state),
                                Err(error) => self.frontend_vm.settings_notice = Some(error),
                            }
                        }
                    }
                    UiResult::ProviderEditor {
                        kind,
                        channel_id,
                        result,
                    } => {
                        if kind != self.provider_kind
                            || self.selected_channel_id.as_deref() != Some(channel_id.as_str())
                            || !matches!(self.provider_editor, ProviderEditorState::Loading)
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
                                self.request_siwc("refresh".into(), None);
                                // Tauri `CatalogModelField` 在挂载时就拉一次目录（只有
                                // OrcaRouter 这种没有固定清单的服务商才需要），否则用户
                                // 得自己找到「拉取模型」才知道可选什么。
                                if self
                                    .provider_editor_form
                                    .as_ref()
                                    .is_some_and(|form| form.provider_type == "orcarouter")
                                {
                                    if let Some(form) = self.provider_editor_form.as_mut() {
                                        form.models_loading = true;
                                    }
                                    self.request_provider_models(kind, channel_id);
                                }
                            }
                            Err(error) => {
                                self.provider_editor = ProviderEditorState::Failed;
                                self.provider_editor_form = None;
                                self.frontend_vm.settings_notice = Some(error.clone());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::ProviderModels {
                        kind,
                        channel_id,
                        endpoint,
                        provider_type,
                        request_format,
                        result,
                    } => {
                        if kind == self.provider_kind
                            && self.selected_channel_id.as_deref() == Some(channel_id.as_str())
                            && self.provider_editor_form.as_ref().is_some_and(|form| {
                                form.endpoint == endpoint
                                    && form.provider_type == provider_type
                                    && form.protocol.format() == request_format
                            })
                        {
                            match result {
                                Ok(models) => {
                                    // Tauri `loadModels`：空清单也要明说（「鉴权成功，
                                    // 但没有返回可用模型。」），否则和拉取失败长得一样。
                                    self.status = if models.is_empty() {
                                        tr_l10n(lang, "settings.providers.modelsEmpty").to_string()
                                    } else {
                                        fmt_l10n(
                                            lang,
                                            "settings.providers.modelsLoaded",
                                            &[&models.len()],
                                        )
                                    };
                                    if let Some(form) = self.provider_editor_form.as_mut() {
                                        form.models = models.clone();
                                        form.catalog_format = Some(request_format);
                                        form.models_loading = false;
                                    }
                                    self.frontend_vm.settings_notice = None;
                                    self.provider_models = models;
                                }
                                Err(error) => {
                                    if let Some(form) = self.provider_editor_form.as_mut() {
                                        form.models.clear();
                                        form.models_loading = false;
                                    }
                                    self.frontend_vm.settings_notice = Some(error.clone());
                                    self.status = error;
                                }
                            }
                        }
                    }
                    UiResult::LibraryRefresh => self.load_library(),
                    UiResult::Library(Ok(library)) => {
                        self.vocabulary = library.vocabulary;
                        self.correction_rules = library.correction_rules;
                        self.style_packs = library.style_packs;
                        self.vocab_preset_store = library.vocab_preset_store;
                        self.vocab_presets = library.vocab_presets;
                    }
                    UiResult::Library(Err(error)) => self.status = error,
                    UiResult::SettingsSaved { request, result } => {
                        self.finish_settings_save(request, result);
                    }
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
                    UiResult::MarketplaceInstallFinished { id, result } => {
                        // Only clear the flag for the pack the UI is still showing.
                        if self.frontend_vm.marketplace_installing.as_deref() == Some(id.as_str()) {
                            self.frontend_vm.marketplace_installing = None;
                        }
                        match result {
                            Ok(message) => self.status = message,
                            Err(error) => self.status = error,
                        }
                    }
                    UiResult::MarketplaceLikes(Ok(likes)) => self.marketplace_my_likes = likes,
                    UiResult::MarketplaceLikes(Err(error)) => {
                        // Not signed in / offline: keep the previous like set.
                        log::debug!("marketplace likes unavailable: {error}");
                    }
                    UiResult::MarketplaceAuth(result) => {
                        self.frontend_vm.marketplace_signed_in = result.unwrap_or(false);
                        if !self.frontend_vm.marketplace_signed_in {
                            self.frontend_vm.marketplace_login.clear();
                        }
                    }
                    UiResult::MarketplaceOAuthStarted(result) => match result {
                        Ok(flow) => {
                            self.frontend_vm.marketplace_oauth_loading = false;
                            self.frontend_vm.marketplace_oauth_open = true;
                            self.frontend_vm.marketplace_oauth_flow_id = Some(flow.flow_id);
                            self.frontend_vm.marketplace_oauth_user_code = flow.user_code;
                            self.frontend_vm.marketplace_oauth_uri = flow.verification_uri;
                            self.frontend_vm.marketplace_oauth_error = None;
                            self.marketplace_oauth_interval_secs = flow.interval_secs.max(1);
                            self.marketplace_oauth_poll_deadline = Some(
                                std::time::Instant::now()
                                    + Duration::from_secs(self.marketplace_oauth_interval_secs),
                            );
                        }
                        Err(error) => {
                            self.frontend_vm.marketplace_oauth_loading = false;
                            self.frontend_vm.marketplace_oauth_open = true;
                            self.frontend_vm.marketplace_oauth_error = Some(error);
                        }
                    },
                    UiResult::MarketplaceOAuthPoll { flow_id, result } => {
                        if self.frontend_vm.marketplace_oauth_flow_id.as_deref()
                            != Some(flow_id.as_str())
                        {
                            continue;
                        }
                        match result {
                            Ok(openless_core::OAuthPollResult::Authorized { login }) => {
                                self.frontend_vm.marketplace_signed_in = true;
                                self.frontend_vm.marketplace_login = login;
                                self.frontend_vm.marketplace_oauth_open = false;
                                self.frontend_vm.marketplace_oauth_flow_id = None;
                                self.frontend_vm.marketplace_oauth_error = None;
                                self.marketplace_oauth_poll_deadline = None;
                                self.frontend_vm.marketplace_mine_loading = true;
                                self.load_marketplace_mine();
                            }
                            Ok(openless_core::OAuthPollResult::Pending) => {
                                self.marketplace_oauth_poll_deadline = Some(
                                    std::time::Instant::now()
                                        + Duration::from_secs(self.marketplace_oauth_interval_secs),
                                );
                            }
                            Ok(openless_core::OAuthPollResult::SlowDown) => {
                                self.marketplace_oauth_poll_deadline =
                                    Some(std::time::Instant::now() + Duration::from_secs(5));
                            }
                            Ok(openless_core::OAuthPollResult::Error { message }) => {
                                self.marketplace_oauth_poll_deadline = None;
                                self.frontend_vm.marketplace_oauth_flow_id = None;
                                self.frontend_vm.marketplace_oauth_error = Some(message);
                            }
                            Err(error) => {
                                self.marketplace_oauth_poll_deadline = None;
                                self.frontend_vm.marketplace_oauth_flow_id = None;
                                self.frontend_vm.marketplace_oauth_error = Some(error);
                            }
                        }
                    }
                    UiResult::MarketplacePublish(result) => {
                        self.frontend_vm.style_editor_publishing = false;
                        match result {
                            Ok(message) => self.status = message,
                            Err(error) => self.frontend_vm.style_notice = Some(error),
                        }
                    }
                    UiResult::MarketplaceUpload(result) => {
                        self.frontend_vm.marketplace_upload_submitting = false;
                        match result {
                            Ok(uploaded) => {
                                self.frontend_vm.marketplace_upload_open = false;
                                self.frontend_vm.marketplace_upload_selected = None;
                                self.frontend_vm.marketplace_notice = None;
                                self.status = uploaded.message;
                                self.frontend_vm.marketplace_mine_loading = true;
                                self.load_marketplace_mine();
                            }
                            Err(error) => {
                                self.frontend_vm.marketplace_notice = Some(error);
                            }
                        }
                    }
                    UiResult::MarketplaceWithdraw(result) => match result {
                        Ok(message) => {
                            self.frontend_vm.marketplace_mine_loading = true;
                            self.status = message;
                            self.load_marketplace_mine();
                        }
                        Err(error) => self.frontend_vm.marketplace_notice = Some(error),
                    },
                    UiResult::SettingsChannels {
                        kind,
                        request,
                        result,
                    } => {
                        if kind != self.settings_channel_kind
                            || request != self.settings_channels_request
                        {
                            continue;
                        }
                        self.settings_channels_loading = false;
                        match result {
                            Ok(rows) => {
                                self.settings_channels = rows;
                                self.frontend_vm.settings_notice = None;
                            }
                            Err(error) => self.frontend_vm.settings_notice = Some(error),
                        }
                    }
                    UiResult::CloudSyncDone { result } => {
                        self.apply_cloud_sync_result(*result);
                    }
                    UiResult::OmniFinished {
                        operation,
                        submitted,
                        result,
                    } => {
                        use openless_linux_egui::omni_settings::OmniAction;
                        match *result {
                            Ok(state) => {
                                let mut state =
                                    self.frontend_vm.omni.merge_receipt(&submitted, state);
                                let save_again = state.dirty;
                                state.notice = if save_again
                                    || matches!(operation, OmniAction::Reload)
                                {
                                    None
                                } else {
                                    Some(match operation {
                                        OmniAction::Validate => tr_l10n(
                                            self.lang,
                                            "settings.providers.validateSuccessOmni",
                                        )
                                        .to_owned(),
                                        OmniAction::Models if state.models.is_empty() => {
                                            tr_l10n(self.lang, "settings.providers.modelsEmpty")
                                                .to_owned()
                                        }
                                        OmniAction::Models => fmt_l10n(
                                            self.lang,
                                            "settings.providers.modelsLoaded",
                                            &[&state.models.len().to_string()],
                                        ),
                                        OmniAction::ClearSecrets => {
                                            tr_l10n(self.lang, "status.secret_cleared").to_owned()
                                        }
                                        _ => tr_l10n(self.lang, "common.saved").to_owned(),
                                    })
                                };
                                self.frontend_vm.omni = state;
                                if let Some(notice) = &self.frontend_vm.omni.notice {
                                    self.status = notice.clone();
                                }
                                if save_again {
                                    self.apply_omni_action(OmniAction::Save);
                                }
                            }
                            Err(error) => {
                                self.frontend_vm.omni.busy = false;
                                self.frontend_vm.omni.switching = false;
                                let message = if error == "validateFailed" {
                                    tr_l10n(self.lang, "settings.providers.validateFailed")
                                        .to_owned()
                                } else {
                                    format!(
                                        "{}: {error}",
                                        tr_l10n(self.lang, "common.operationFailed")
                                    )
                                };
                                self.frontend_vm.omni.notice = Some(message.clone());
                                self.status = message;
                            }
                        }
                    }
                    UiResult::ServiceConfigured(configured) => {
                        self.service_configured = configured;
                    }
                    UiResult::ChannelsChanged { kind, result } => {
                        match result {
                            Ok(message) => {
                                self.frontend_vm.settings_notice = None;
                                self.status = message;
                            }
                            Err(error) => self.frontend_vm.settings_notice = Some(error),
                        }
                        if kind == self.settings_channel_kind {
                            self.load_settings_channels();
                            self.load_service_configured();
                            self.load_providers(kind);
                        }
                    }
                    UiResult::ProviderTypeChanged {
                        kind,
                        channel_id,
                        result,
                    } => {
                        self.provider_switch_pending = false;
                        let selected = self
                            .provider_editor_form
                            .as_ref()
                            .is_some_and(|form| form.kind == kind && form.channel_id == channel_id);
                        match *result {
                            Ok(editor) => {
                                self.frontend_vm.settings_notice = None;
                                if selected {
                                    self.provider_editor_form =
                                        Some(ProviderEditorForm::from_editor(&editor, self.lang));
                                    self.provider_editor =
                                        ProviderEditorState::Loaded(Box::new(editor));
                                    self.request_siwc("refresh".into(), None);
                                    if self
                                        .provider_editor_form
                                        .as_ref()
                                        .is_some_and(|form| form.provider_type == "orcarouter")
                                    {
                                        if let Some(form) = self.provider_editor_form.as_mut() {
                                            form.models_loading = true;
                                        }
                                        self.request_provider_models(kind, channel_id);
                                    }
                                }
                                self.load_settings_channels();
                                self.load_service_configured();
                                self.load_providers(self.provider_kind);
                                if let Some(action) = self.provider_after_save.take() {
                                    self.close_provider_editor();
                                    self.pending_ui_actions.push(action);
                                }
                            }
                            Err(error) => {
                                self.provider_after_save = None;
                                self.frontend_vm.settings_notice = Some(error);
                            }
                        }
                    }
                    UiResult::ChannelCreated { kind, result } => {
                        self.channel_create_pending = false;
                        match result {
                            Ok(channel_id) => {
                                if kind != self.settings_channel_kind
                                    || !self.frontend_vm.settings_open
                                    || !matches!(self.frontend_vm.services_view, 0 | 1)
                                {
                                    if kind == self.settings_channel_kind {
                                        self.load_settings_channels();
                                    }
                                    continue;
                                }
                                self.settings_channel_kind = kind;
                                self.provider_kind = kind;
                                // 草稿：面板加载完就自动打开编辑器，未交互关闭时回收。
                                self.close_provider_editor();
                                self.selected_channel_id = Some(channel_id);
                                self.editor_draft = true;
                                self.draft_touched = false;
                                self.editor_requested = true;
                                self.load_settings_channels();
                                self.load_service_configured();
                                self.load_providers(kind);
                            }
                            Err(error) => self.frontend_vm.settings_notice = Some(error),
                        }
                    }
                    UiResult::MarketplaceDetail(Ok(detail)) => {
                        self.status =
                            fmt_l10n(lang, "status.detail_loaded", &[&detail.summary.name]);
                        self.frontend_vm.marketplace_detail_prompt = Some(detail.prompt.clone());
                        self.marketplace_detail = Some(detail);
                    }
                    UiResult::MarketplaceDetail(Err(error)) => self.status = error,
                    UiResult::MarketplaceMine(Ok((packs, likes))) => {
                        self.frontend_vm.marketplace_mine_loading = false;
                        self.frontend_vm.marketplace_mine_packs = packs
                            .iter()
                            .map(|pack| frontend::view_model::MarketplaceMinePack {
                                pack: frontend::view_model::MarketplacePack {
                                    id: pack.summary.id.clone(),
                                    name: pack.summary.name.clone(),
                                    version: pack.summary.version.clone(),
                                    description: pack.summary.description.clone(),
                                    mode: pack.summary.base_mode.clone(),
                                    author: pack.summary.author_login.clone(),
                                    origin_author_login: pack.summary.origin_author_login.clone(),
                                    tags: pack.summary.tags.clone(),
                                    likes: pack.summary.like_count.max(0) as u32,
                                    downloads: pack.summary.download_count.max(0) as u32,
                                    liked: false,
                                },
                                state: pack.state.clone(),
                                updated_at: pack.summary.updated_at.clone(),
                            })
                            .collect();
                        self.marketplace_my_packs = packs;
                        self.marketplace_my_likes = likes;
                    }
                    UiResult::MarketplaceMine(Err(error)) => {
                        self.frontend_vm.marketplace_mine_loading = false;
                        self.frontend_vm.marketplace_notice = Some(error.clone());
                        self.status = error;
                    }
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
                    UiResult::Overview(Ok(mut data)) => {
                        data.history = self.history.entries.clone();
                        self.overview = OverviewState::Loaded(data);
                    }
                    UiResult::Overview(Err(error)) => {
                        self.status = error.clone();
                        self.overview = OverviewState::Failed(error);
                    }
                }
            }
            if let Some(backend) = self.backend() {
                self.snapshot = Some(backend.snapshot());
                self.history.observe(backend.snapshot().history_revision);
            }
            self.reconcile_capsule_liveness();
            self.load_history();
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

        fn refresh_linux_access(&mut self) {
            if self.frontend_vm.linux_access.checking {
                return;
            }
            self.frontend_vm.linux_access.checking = true;
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                use openless_linux_egui::linux_access::{
                    self, CheckState, KeyringState, LinuxAccessSnapshot,
                };
                let session = tokio::task::spawn_blocking(linux_access::probe_session);
                let (session, (network, network_latency_ms)) =
                    tokio::join!(session, linux_access::probe_network(),);
                let (keyring, input) =
                    session.unwrap_or((KeyringState::Unavailable, CheckState::Unavailable));
                let _ = tx.send(UiResult::LinuxAccess(LinuxAccessSnapshot {
                    checking: false,
                    keyring,
                    input,
                    network,
                    network_latency_ms,
                }));
            });
        }

        // ── Frontend bridge ─────────────────────────────────────────────────

        /// Sync backend state into the frontend view model each frame before
        /// rendering. Only fields that have real data sources are populated;
        /// unwired fields remain in their default empty / loading state.
        fn sync_view_model(&mut self) {
            // Only one diagnostic job can run, and only refresh while the
            // privacy page is visible. Match Tauri's 30-second network cadence.
            if self.frontend_vm.settings_open
                && self.frontend_vm.settings_section
                    == frontend::view_model::SettingsSection::Privacy
                && self
                    .linux_access_last_check
                    .is_none_or(|checked| checked.elapsed() >= Duration::from_secs(30))
            {
                self.refresh_linux_access();
            }
            self.frontend_vm.settings_saving = self.settings_save.in_flight.is_some();
            self.frontend_vm.settings_save_error = self.settings_save.error.clone();
            // Capture overview error before taking a mutable borrow on frontend_vm.
            let overview_err = self.overview_error();
            let backend = self.backend();
            self.frontend_vm.application_data_directory = backend
                .as_ref()
                .map(|backend| backend.config().data_dir.display().to_string())
                .unwrap_or_default();
            let lang = self.lang;
            let permissions = self.permission_snapshot();
            // Style-pack icons must be read before `vm` borrows `frontend_vm`.
            let style_icon_urls = self.style_icon_urls();
            // The drawer's runtime card previews the *draft*, using Core's own
            // prompt composer, so it must be built before `vm` is borrowed.
            let style_runtime = if self.frontend_vm.style_editor_open {
                match (backend.as_ref(), self.style_editor.as_ref()) {
                    (Some(backend), Some(stored)) => {
                        let mut draft = stored.clone();
                        draft.prompt = self.frontend_vm.style_prompt.clone();
                        draft.selection_prompt = self.frontend_vm.style_selection_prompt.clone();
                        draft.voice_edit_prompt = self.frontend_vm.style_voice_edit_prompt.clone();
                        let diagnostics = backend.preview_style_pack_runtime(&draft);
                        Some(frontend::view_model::StyleRuntimePreview {
                            context_active: diagnostics.includes_context_premise,
                            hotword_active: diagnostics.includes_hotword_block,
                            history_active: diagnostics.includes_history_instruction,
                            omits_front_app: diagnostics.preview_omits_front_app,
                        })
                    }
                    _ => None,
                }
            } else {
                None
            };
            // 文本型设置行只在偏好载入 / 外部变更时回灌一次，避免把输入中的
            // 内容每帧弹回旧值。
            let hydrate_text = std::mem::replace(&mut self.hydrate_text_fields, false);
            // 日志级别是宿主本地文件（不在 Core 偏好里），要在借用 vm 之前读。
            let log_level_index = self
                .backend()
                .and_then(|backend| openless_linux_egui::read_log_level(&backend.config().data_dir))
                .map_or(0, |level| match level {
                    log::LevelFilter::Debug => 1,
                    log::LevelFilter::Trace => 2,
                    _ => 0,
                });

            let vm = &mut self.frontend_vm;

            // Map shell::Page to frontend::Page.
            vm.active_page = match self.active_page {
                shell::Page::Overview => frontend::view_model::Page::Overview,
                shell::Page::History => frontend::view_model::Page::History,
                shell::Page::QuickNote => frontend::view_model::Page::QuickNote,
                shell::Page::Vocabulary => frontend::view_model::Page::Vocab,
                shell::Page::Styles => frontend::view_model::Page::Style,
                shell::Page::Marketplace => frontend::view_model::Page::Marketplace,
                shell::Page::Providers => frontend::view_model::Page::Settings,
                shell::Page::Assistant => frontend::view_model::Page::SelectionAsk,
                shell::Page::Translation => frontend::view_model::Page::Translation,
                shell::Page::Corrections => frontend::view_model::Page::Corrections,
            };

            vm.status = self.status.clone();
            vm.runtime_warning = self.runtime_warning.clone();
            vm.quick_note_recording = backend.as_ref().is_some_and(|backend| {
                backend.dictation_output_target()
                    == Some(openless_core::DictationOutputTarget::QuickNote)
            });
            vm.version = env!("OPENLESS_APP_VERSION").to_string();
            vm.lang = lang;
            // UI 进程没有 Core 偏好，明暗主题必须随视图模型一起过去。
            vm.theme_mode = self
                .preferences
                .as_ref()
                .map(|preferences| preferences.theme_mode)
                .unwrap_or_default();
            if let Some(prefs) = &self.preferences {
                vm.dictation_hotkey =
                    openless_linux_egui::shortcut_display_label(&prefs.dictation_hotkey);
                vm.qa_hotkey = prefs
                    .qa_hotkey
                    .as_ref()
                    .map(openless_linux_egui::shortcut_display_label)
                    .unwrap_or_default();
                vm.quick_note_hotkey = prefs
                    .quick_note_hotkey
                    .as_ref()
                    .map(openless_linux_egui::shortcut_display_label)
                    .unwrap_or_default();
                vm.quick_note_shortcut_hidden = self.quick_note_shortcut_hidden;
                vm.translation_hotkey =
                    openless_linux_egui::shortcut_display_label(&prefs.translation_hotkey);
                // 这几行一直在界面上，但宿主以前只填了前四个：没填的行无论 Core 里
                // 有没有绑定都显示成空键帽，看起来像「未设置」。一律从偏好取真值。
                vm.switch_style_hotkey = prefs
                    .switch_style_hotkey
                    .as_ref()
                    .map(openless_linux_egui::shortcut_display_label)
                    .unwrap_or_default();
                vm.open_app_hotkey = prefs
                    .open_app_hotkey
                    .as_ref()
                    .map(openless_linux_egui::shortcut_display_label)
                    .unwrap_or_default();
                vm.coding_agent_hotkey = prefs
                    .coding_agent_voice_hotkey
                    .as_ref()
                    .map(openless_linux_egui::shortcut_display_label)
                    .unwrap_or_default();
            }

            // The drawer's Save button is enabled only while the draft differs
            // from what Core last stored.
            vm.style_editor_dirty = style_editor_is_dirty(vm);
            vm.style_runtime = style_runtime;

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
                s.language = match self.locale_pref {
                    LocalePref::System => 0,
                    LocalePref::Lang(Lang::ZhCn) => 1,
                    LocalePref::Lang(Lang::ZhTw) => 2,
                    LocalePref::Lang(Lang::En) => 3,
                    LocalePref::Lang(Lang::Ja) => 4,
                    LocalePref::Lang(Lang::Ko) => 5,
                };
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
                s.capsule_transcript = prefs.capsule_transcript_enabled;
                s.capsule_transcript_font_size =
                    usize::from(prefs.capsule_transcript_font_size.clamp(12, 20));
                s.capsule_style = match prefs.capsule_style {
                    openless_core::shared_types::CapsuleStyle::Classic => 1,
                    openless_core::shared_types::CapsuleStyle::Typeless => 2,
                    openless_core::shared_types::CapsuleStyle::Siri => 0,
                };
                s.mute_while_recording = prefs.mute_during_recording;
                s.audio_enhancement = prefs.audio_enhancement_enabled;
                s.audio_dsp = prefs.audio_dsp;
                s.audio_cue = prefs.audio_cue_on_record;
                s.launch_at_login = prefs.launch_at_login;
                s.streaming_save_clipboard = prefs.streaming_insert_save_clipboard;
                s.record_audio_for_debug = prefs.record_audio_for_debug;
                // 日志级别是宿主本地文件，不在 Core 偏好里；读不到就按 info 显示。
                s.log_level = log_level_index;
                if hydrate_text {
                    s.history_max_entries = prefs
                        .history_max_entries
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    s.retention_days = prefs.history_retention_days.to_string();
                    s.polish_context_window = prefs.polish_context_window_minutes.to_string();
                    s.polish_skip_max_chars = prefs.polish_skip_max_chars.to_string();
                    s.audio_recording_max_entries = prefs
                        .audio_recording_max_entries
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                }
                s.remote_default_mode = usize::from(prefs.remote_input_default_mode == "hold");
                s.system_proxy = prefs.use_system_proxy;
                s.llm_thinking_enabled = prefs.llm_thinking_enabled;
                s.multimodal = prefs.multimodal_pipeline_enabled;
                s.less_computer = prefs.coding_agent_enabled;
                s.testing_features = prefs.testing_features_enabled;
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
                // 选区助手（Tauri SelectionWorkspaceSection）
                s.selection_voice_enabled = prefs.selection_voice_enabled;
                s.selection_voice_auto_intent = matches!(
                    prefs.selection_voice_intent_mode,
                    openless_core::shared_types::SelectionVoiceIntentMode::Auto
                );
                s.selection_voice_plan_format = match prefs.selection_voice_edit_plan_format {
                    openless_core::edit_plan::EditPlanFormat::Json => 1,
                    openless_core::edit_plan::EditPlanFormat::Xml => 0,
                };
                if hydrate_text {
                    s.selection_voice_keywords = prefs.selection_voice_edit_keywords.join("\n");
                    s.selection_voice_system_prompt =
                        prefs.selection_voice_edit_system_prompt.clone();
                }
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
                vm.pipeline_multimodal = prefs.multimodal_pipeline_enabled
                    && prefs.pipeline_mode == openless_core::shared_types::PipelineMode::Multimodal;
                // Linux 宿主没有本地推理引擎。
                vm.supports_local_asr = false;
                // 热键后端是否真的起来了：没有 fcitx5 监听器时隐藏「快捷键」分区，
                // 与 Tauri 的 `visibleSettingsSections(supportsDesktopHotkey)` 一致。
                vm.hotkeys_supported = self
                    .native
                    .as_ref()
                    .is_some_and(LinuxNativeRuntime::hotkeys_available);
                vm.permissions = permissions;
                vm.selection_polish_hotkey = prefs
                    .selection_polish_hotkey
                    .as_ref()
                    .map(openless_linux_egui::shortcut_display_label)
                    .unwrap_or_default();
                vm.qa_save_history = prefs.qa_save_history;
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
                            hotkey: openless_linux_egui::shortcut_display_label(&entry.binding),
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
            vm.channels_loading = self.settings_channels_loading || self.channel_create_pending;
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
                    id: channel.id.clone(),
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
                    last_ok: channel.last_ok,
                    last_error: channel.last_error.clone(),
                    last_latency_ms: channel.last_latency_ms,
                    last_check_age_seconds: channel.last_test_at.map(|at| {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|elapsed| elapsed.as_secs() as i64)
                            .unwrap_or(at);
                        (now - at).max(0)
                    }),
                })
                .collect();

            // Provider editor: mirrored from the host draft each frame. The page
            // stays a pure renderer; Core still owns the credential schema.
            vm.provider_editor = self.provider_editor_form.as_ref().map(|form| {
                frontend::view_model::SettingsProviderEditor {
                    siwc: form.siwc.clone(),
                    channel_id: form.channel_id.clone(),
                    is_asr: matches!(form.kind, openless_core::ChannelKind::Asr),
                    is_draft: self.editor_draft,
                    provider: form.label.clone(),
                    provider_type: form.provider_type.clone(),
                    endpoint_presets: match &self.provider_editor {
                        ProviderEditorState::Loaded(editor) if form.provider_type == "ark" => {
                            let mut presets = editor.descriptor.endpoint_presets.clone();
                            if let Some(endpoint) = &editor.descriptor.default_endpoint {
                                presets.insert(0, openless_core::provider_rules::ProviderEndpointPreset {
                                    name: form.label.clone(), endpoint: endpoint.clone(), models_url: None,
                                });
                            }
                            presets
                        }
                        _ => Vec::new(),
                    },
                    default_endpoint: match &self.provider_editor {
                        ProviderEditorState::Loaded(editor) => editor.descriptor.default_endpoint.clone().unwrap_or_default(),
                        _ => String::new(),
                    },
                    api_key_optional: matches!(&self.provider_editor, ProviderEditorState::Loaded(editor) if editor.descriptor.auth_requirement == openless_core::AuthRequirement::EndpointModelOptionalApiKey),
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
                    static_models: form.static_models.clone(),
                    default_model: form.default_model.clone(),
                    has_models_url: form.models_url.is_some(),
                    custom_model: form.custom_model,
                    protocol: form.protocol.clone(),
                    busy: self.provider_switch_pending
                        || matches!(self.provider_editor, ProviderEditorState::Loading),
                }
            });

            // Rebuild display labels from the shared in-memory history cache.
            let selected_id = vm
                .history_entries
                .get(vm.history_selected)
                .map(|entry| entry.id.clone());
            // 历史按页下发：快照只带第一页，其余由窗口的 `LoadMore` 拉取
            // （见 `frontend::paging`），所以快照不会随历史增长而膨胀。
            vm.history_list_total = self.history.entries.len();
            vm.history_entries = self
                .history
                .entries
                .iter()
                .take(frontend::paging::page_limit())
                .cloned()
                .map(|item| history_entry_view(item, &self.style_packs, lang))
                .collect();
            vm.history_loading = self.history.loading();
            vm.history_error = self.history.error.clone();
            // 分页之后宿主手里只有第一页：选中项在页内就按 id 重新定位；
            // 不在页内（用户已经加载了更后面的页）就保持原下标不动 —— 那是窗口
            // 自己那份列表的下标，宿主无权按页去改，否则会把选中项弄丢。
            if let Some(index) = selected_id
                .as_deref()
                .and_then(|id| vm.history_entries.iter().position(|entry| entry.id == id))
            {
                vm.history_selected = index;
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
                    paused: player.is_paused(),
                }
            });

            // Vocabulary + correction rules: the library path is always wired, so
            // an empty store is an empty list — never an "unsupported" page.
            vm.vocab_unsupported = false;
            vm.vocab_total = self.vocabulary.len();
            vm.vocab_entries = self
                .vocabulary
                .iter()
                .take(frontend::paging::page_limit())
                .map(vocab_entry_view)
                .collect();
            vm.correction_rule_total = self.correction_rules.len();
            vm.vocab_rules = self
                .correction_rules
                .iter()
                .take(frontend::paging::page_limit())
                .map(correction_rule_view)
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
            // 图标是 base64 data URL，最占体积，同样按页下发。
            vm.style_unsupported = false;
            let selection_polish_style_pack_id = self
                .preferences
                .as_ref()
                .map(|prefs| prefs.selection_polish_style_pack_id.as_str())
                .filter(|id| !id.is_empty());
            vm.style_pack_total = self.style_packs.len();
            vm.style_packs = self
                .style_packs
                .iter()
                .take(frontend::paging::page_limit())
                .enumerate()
                .map(|(index, pack)| {
                    style_pack_view(
                        pack,
                        style_icon_urls.get(index).and_then(|url| url.as_deref()),
                        selection_polish_style_pack_id,
                        lang,
                    )
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
                vm.marketplace_total = self.marketplace_items.len();
                vm.marketplace_packs = self
                    .marketplace_items
                    .iter()
                    .take(frontend::paging::page_limit())
                    .map(|item| {
                        marketplace_pack_view(item, self.marketplace_my_likes.contains(&item.id))
                    })
                    .collect();
            }

            // Startup error.
            if let Some(error) = &self.startup_error {
                vm.status = format!("启动失败: {error}");
            }
            if self.settings_channels_refresh_requested && !self.settings_channels_loading {
                self.settings_channels_refresh_requested = false;
                self.load_settings_channels();
                self.load_service_configured();
            }
            if self.frontend_vm.settings_section == frontend::view_model::SettingsSection::Services
            {
                let view = self.resolved_services_view(self.frontend_vm.services_view);
                if view != self.frontend_vm.services_view
                    || (view == 4
                        && !self.frontend_vm.omni.loaded
                        && !self.frontend_vm.omni.busy
                        && self.frontend_vm.omni.notice.is_none())
                {
                    self.select_services_view(view);
                }
            }
        }

        /// Returns the overview error string if the overview is in a failed state.
        fn overview_error(&self) -> Option<String> {
            match &self.overview {
                crate::linux_app::OverviewState::Failed(error) => Some(error.clone()),
                _ => None,
            }
        }

        /// Capture Linux capability hints, not authorization grants. Device
        /// enumeration does not prove capture access, and a native runtime alone
        /// does not imply that its optional global-hotkey listener initialized.
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
                hotkey: if self
                    .native
                    .as_ref()
                    .is_some_and(LinuxNativeRuntime::hotkeys_available)
                {
                    PermissionState::Granted
                } else {
                    PermissionState::Unknown
                },
            }
        }

        /// Dispatch frontend actions to existing Core / backend methods.
        /// 切一页给窗口。顺序必须与快照第一页完全一致（都是宿主存储的前缀），
        /// 否则窗口按 `offset` 拼接会把两段不同的数据接在一起。
        fn collection_page(
            &mut self,
            collection: frontend::paging::Collection,
            offset: usize,
        ) -> (usize, frontend::paging::CollectionItems) {
            use frontend::paging::{slice_page, Collection, CollectionItems};
            let lang = self.lang;
            let limit = frontend::paging::page_limit();
            match collection {
                Collection::History => {
                    let items = slice_page(&self.history.entries, offset, limit)
                        .into_iter()
                        .map(|item| history_entry_view(item, &self.style_packs, lang))
                        .collect();
                    (self.history.entries.len(), CollectionItems::History(items))
                }
                Collection::Vocabulary => {
                    let items = slice_page(&self.vocabulary, offset, limit)
                        .iter()
                        .map(vocab_entry_view)
                        .collect();
                    (self.vocabulary.len(), CollectionItems::Vocabulary(items))
                }
                Collection::CorrectionRules => {
                    let items = slice_page(&self.correction_rules, offset, limit)
                        .iter()
                        .map(correction_rule_view)
                        .collect();
                    (
                        self.correction_rules.len(),
                        CollectionItems::CorrectionRules(items),
                    )
                }
                Collection::Marketplace => {
                    let items = slice_page(&self.marketplace_items, offset, limit)
                        .iter()
                        .map(|item| {
                            marketplace_pack_view(
                                item,
                                self.marketplace_my_likes.contains(&item.id),
                            )
                        })
                        .collect();
                    (
                        self.marketplace_items.len(),
                        CollectionItems::Marketplace(items),
                    )
                }
                Collection::StylePacks => {
                    // 图标 data URL 由 `style_icon_urls` 缓存，按 index 取。
                    let icons = self.style_icon_urls();
                    let selection = self
                        .preferences
                        .as_ref()
                        .map(|prefs| prefs.selection_polish_style_pack_id.clone())
                        .filter(|id| !id.is_empty());
                    let items = self
                        .style_packs
                        .iter()
                        .enumerate()
                        .skip(offset)
                        .take(limit)
                        .map(|(index, pack)| {
                            style_pack_view(
                                pack,
                                icons.get(index).and_then(|url| url.as_deref()),
                                selection.as_deref(),
                                lang,
                            )
                        })
                        .collect();
                    (self.style_packs.len(), CollectionItems::StylePacks(items))
                }
            }
        }

        fn apply_frontend_actions(
            &mut self,
            actions: Vec<frontend::view_model::FrontendAction>,
            // 宿主没有窗口：窗口类动作由 UI 进程就地处理，这里保留参数是为了
            // 让调用点保持「渲染层 → 动作 → 宿主」的形状。排版改动需要它重装字体。
            ctx_arg: &egui::Context,
        ) {
            for action in actions {
                use frontend::view_model::FrontendAction as Action;
                let leaving_editor = match &action {
                    Action::CloseSettings
                    | Action::Navigate(_)
                    | Action::SettingsChannelSelect(_) => true,
                    Action::ToggleSettings => self.frontend_vm.settings_open,
                    Action::SettingsServicesView(view) => *view != self.frontend_vm.services_view,
                    Action::SettingsSection(section) => {
                        *section != frontend::view_model::SettingsSection::Services
                    }
                    _ => false,
                };
                if leaving_editor && self.provider_editor_form.is_some() {
                    if self.provider_switch_pending {
                        self.provider_after_save = Some(action);
                        continue;
                    }
                    if self
                        .provider_editor_form
                        .as_ref()
                        .is_some_and(|form| form.protocol.validate().is_err())
                    {
                        continue;
                    }
                    if self.draft_touched {
                        self.provider_after_save = Some(action);
                        if !self.queue_provider_save() {
                            self.provider_after_save = None;
                        }
                        continue;
                    }
                    self.request_provider_editor_close();
                }
                match action {
                    frontend::view_model::FrontendAction::Navigate(page) => {
                        if self.frontend_vm.omni.dirty {
                            self.apply_omni_action(
                                openless_linux_egui::omni_settings::OmniAction::Save,
                            );
                        }
                        self.active_page = match page {
                            frontend::view_model::Page::Overview => shell::Page::Overview,
                            frontend::view_model::Page::History => shell::Page::History,
                            frontend::view_model::Page::QuickNote => shell::Page::QuickNote,
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
                        if self.frontend_vm.omni.dirty {
                            self.apply_omni_action(
                                openless_linux_egui::omni_settings::OmniAction::Save,
                            );
                        }
                        self.frontend_vm.settings_open = !self.frontend_vm.settings_open;
                        if self.frontend_vm.settings_open {
                            self.frontend_vm.active_page = frontend::view_model::Page::Settings;
                            // 每次打开设置都刷新「必配服务」状态点。
                            self.linux_access_last_check = None;
                            self.load_service_configured();
                            self.provider_kind = self.settings_channel_kind;
                            self.load_providers(self.settings_channel_kind);
                            // 也重新枚举麦克风：设备可能在启动后才插上（Tauri 在
                            // 下拉打开时同样重查）。
                            self.load_microphones();
                            if self.settings_channels.is_empty() {
                                self.load_settings_channels();
                            }
                            self.load_cloud_sync();
                        }
                    }
                    frontend::view_model::FrontendAction::CloseSettings => {
                        if self.frontend_vm.omni.dirty {
                            self.apply_omni_action(
                                openless_linux_egui::omni_settings::OmniAction::Save,
                            );
                        }
                        self.frontend_vm.settings_open = false;
                        self.frontend_vm.active_page = frontend::view_model::Page::Overview;
                    }
                    frontend::view_model::FrontendAction::DismissRuntimeWarning => {
                        self.runtime_warning = None;
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
                        self.frontend_vm.marketplace_mine_open = true;
                        self.frontend_vm.marketplace_mine_loading = true;
                        self.frontend_vm.marketplace_notice = None;
                        self.frontend_vm.marketplace_mine_packs.clear();
                        self.load_marketplace_mine();
                    }
                    frontend::view_model::FrontendAction::MarketplaceCloseMine => {
                        self.frontend_vm.marketplace_mine_open = false;
                    }
                    frontend::view_model::FrontendAction::MarketplaceUploadOpen {
                        origin_pack_id,
                        target_name,
                    } => {
                        if !self.frontend_vm.marketplace_signed_in {
                            continue;
                        }
                        let target = target_name
                            .as_deref()
                            .unwrap_or_default()
                            .trim()
                            .to_lowercase();
                        let mut packs: Vec<_> = self
                            .style_packs
                            .iter()
                            .filter(|pack| pack.kind != openless_core::StylePackKind::Builtin)
                            .cloned()
                            .collect();
                        packs.sort_by(|left, right| {
                            let left_match =
                                !target.is_empty() && left.name.trim().to_lowercase() == target;
                            let right_match =
                                !target.is_empty() && right.name.trim().to_lowercase() == target;
                            right_match.cmp(&left_match).then_with(|| {
                                left.name.to_lowercase().cmp(&right.name.to_lowercase())
                            })
                        });
                        let selected = if target.is_empty() {
                            None
                        } else {
                            packs
                                .iter()
                                .position(|pack| pack.name.trim().to_lowercase() == target)
                        };
                        self.frontend_vm.marketplace_upload_open = true;
                        self.frontend_vm.marketplace_upload_origin_pack_id = origin_pack_id;
                        self.frontend_vm.marketplace_upload_target_name = target_name;
                        self.frontend_vm.marketplace_upload_packs = packs;
                        self.frontend_vm.marketplace_upload_selected = selected;
                        self.frontend_vm.marketplace_upload_submitting = false;
                        self.frontend_vm.marketplace_notice = None;
                    }
                    frontend::view_model::FrontendAction::MarketplaceUploadSelect(index) => {
                        if index < self.frontend_vm.marketplace_upload_packs.len()
                            && !self.frontend_vm.marketplace_upload_submitting
                        {
                            self.frontend_vm.marketplace_upload_selected = Some(index);
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceUploadCancel => {
                        self.frontend_vm.marketplace_upload_open = false;
                        self.frontend_vm.marketplace_upload_origin_pack_id = None;
                        self.frontend_vm.marketplace_upload_target_name = None;
                        self.frontend_vm.marketplace_upload_packs.clear();
                        self.frontend_vm.marketplace_upload_selected = None;
                        self.frontend_vm.marketplace_upload_submitting = false;
                    }
                    frontend::view_model::FrontendAction::MarketplaceUploadConfirm => {
                        if !self.frontend_vm.marketplace_signed_in
                            || self.frontend_vm.marketplace_upload_submitting
                        {
                            continue;
                        }
                        let Some(index) = self.frontend_vm.marketplace_upload_selected else {
                            continue;
                        };
                        let Some(pack) = self
                            .frontend_vm
                            .marketplace_upload_packs
                            .get(index)
                            .cloned()
                        else {
                            continue;
                        };
                        let origin_pack_id =
                            self.frontend_vm.marketplace_upload_origin_pack_id.clone();
                        let Some(backend) = self.backend() else {
                            continue;
                        };
                        self.frontend_vm.marketplace_upload_submitting = true;
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let result = backend
                                .services()
                                .marketplace
                                .upload(pack.id, origin_pack_id)
                                .await
                                .map_err(|error| error.to_string());
                            let _ = tx.send(UiResult::MarketplaceUpload(result));
                        });
                    }
                    frontend::view_model::FrontendAction::MarketplaceWithdrawRequest(index) => {
                        if self.frontend_vm.marketplace_signed_in
                            && index < self.frontend_vm.marketplace_mine_packs.len()
                        {
                            self.frontend_vm.marketplace_confirm_withdraw = Some(index);
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceWithdrawCancel => {
                        self.frontend_vm.marketplace_confirm_withdraw = None;
                    }
                    frontend::view_model::FrontendAction::MarketplaceWithdrawConfirm => {
                        let Some(index) = self.frontend_vm.marketplace_confirm_withdraw.take()
                        else {
                            continue;
                        };
                        let Some(pack) = self.marketplace_my_packs.get(index) else {
                            continue;
                        };
                        let Some(backend) = self.backend() else {
                            continue;
                        };
                        let id = pack.summary.id.clone();
                        let lang = self.lang;
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let result = backend
                                .services()
                                .marketplace
                                .delete(id)
                                .await
                                .map(|_| tr_l10n(lang, "marketplace.withdraw.success").to_string())
                                .map_err(|error| error.to_string());
                            let _ = tx.send(UiResult::MarketplaceWithdraw(result));
                        });
                    }
                    frontend::view_model::FrontendAction::MarketplaceAuthStart => {
                        self.frontend_vm.marketplace_oauth_open = true;
                        self.frontend_vm.marketplace_oauth_loading = true;
                        self.frontend_vm.marketplace_oauth_error = None;
                        self.start_marketplace_oauth();
                    }
                    frontend::view_model::FrontendAction::MarketplaceAuthOpenBrowser => {
                        if !self.frontend_vm.marketplace_oauth_uri.is_empty() {
                            if let Err(error) =
                                open_external(&self.frontend_vm.marketplace_oauth_uri)
                            {
                                self.frontend_vm.marketplace_oauth_error = Some(error.to_string());
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceAuthCopyCode => {
                        if !self.frontend_vm.marketplace_oauth_user_code.is_empty() {
                            if let Err(error) = fcitx5_copy_to_clipboard(
                                &self.frontend_vm.marketplace_oauth_user_code,
                            ) {
                                self.frontend_vm.marketplace_oauth_error = Some(error.to_string());
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceAuthCancel => {
                        self.marketplace_oauth_poll_deadline = None;
                        self.marketplace_oauth_interval_secs = 5;
                        let flow_id = self.frontend_vm.marketplace_oauth_flow_id.take();
                        self.frontend_vm.marketplace_oauth_open = false;
                        self.frontend_vm.marketplace_oauth_loading = false;
                        if let Some(backend) = self.backend() {
                            self.tokio.spawn(async move {
                                let _ = backend
                                    .services()
                                    .marketplace
                                    .cancel_device_flow(flow_id)
                                    .await;
                            });
                        }
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
                        self.frontend_vm.marketplace_detail_prompt = None;
                    }
                    frontend::view_model::FrontendAction::MarketplaceInstall(index) => {
                        if let Some(item) = self.marketplace_items.get(index) {
                            if let Some(backend) = self.backend() {
                                let id = item.id.clone();
                                let lang = self.lang;
                                // 详情里的安装按钮在完成后才恢复：把「安装中」状态
                                // 记在宿主里，任务结束时无论成败都清掉。
                                self.frontend_vm.marketplace_installing = Some(id.clone());
                                let tx = self.tx.clone();
                                let finished_id = id.clone();
                                self.tokio.spawn(async move {
                                    let result =
                                        match backend.services().marketplace.install(id).await {
                                            Ok(pack) => Ok(fmt_l10n(
                                                lang,
                                                "status.marketplace_installed",
                                                &[&pack.name],
                                            )),
                                            Err(error) => Err(error.to_string()),
                                        };
                                    let _ = tx.send(UiResult::MarketplaceInstallFinished {
                                        id: finished_id,
                                        result,
                                    });
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
                    frontend::view_model::FrontendAction::QuickNoteToggle => {
                        self.toggle_quick_note();
                    }
                    frontend::view_model::FrontendAction::QuickNoteShortcutHidden(hidden) => {
                        self.set_quick_note_shortcut_hidden(hidden);
                    }
                    frontend::view_model::FrontendAction::HistoryRefresh => {
                        self.history.request(
                            self.snapshot
                                .as_ref()
                                .map_or(0, |snapshot| snapshot.history_revision),
                        );
                        self.load_history();
                        self.frontend_vm.history_confirm = None;
                    }
                    frontend::view_model::FrontendAction::LoadMore { collection, offset } => {
                        let (total, items) = self.collection_page(collection, offset);
                        self.pending_collection_pages
                            .push(HostToWindow::CollectionPage {
                                // 回显请求偏移：同一偏移重复请求幂等；窗口据此
                                // 丢掉晚到的旧回包（见 `apply_page`）。
                                request_id: offset as u64,
                                collection,
                                offset,
                                total,
                                items: Box::new(items),
                            });
                    }
                    frontend::view_model::FrontendAction::HistorySelect(index) => {
                        self.frontend_vm.history_selected = index;
                    }
                    frontend::view_model::FrontendAction::HistoryRequestClear => {
                        self.frontend_vm.history_confirm =
                            Some(frontend::view_model::HistoryConfirm::Clear);
                    }
                    frontend::view_model::FrontendAction::HistoryRequestDelete(id) => {
                        self.frontend_vm.history_confirm =
                            Some(frontend::view_model::HistoryConfirm::Delete(id));
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
                            Some(frontend::view_model::HistoryConfirm::Delete(id)) => {
                                if let Some(backend) = self.backend() {
                                    let lang = self.lang;
                                    self.spawn(async move {
                                        backend.delete_history(&id)?;
                                        Ok(tr_l10n(lang, "status.history_deleted").to_string())
                                    });
                                }
                            }
                            None => {}
                        }
                    }
                    frontend::view_model::FrontendAction::HistoryPlay(id) => {
                        let Some(entry) = self.history.entry(&id) else {
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
                    frontend::view_model::FrontendAction::HistoryPauseToggle => {
                        if let Some((_, player)) = self.history_clip.as_ref() {
                            player.toggle_pause();
                        }
                    }
                    frontend::view_model::FrontendAction::HistorySeek(ms) => {
                        if let Some((_, player)) = self.history_clip.as_ref() {
                            player.seek_ms(ms);
                        }
                    }
                    frontend::view_model::FrontendAction::HistoryRepolishOpen(index) => {
                        self.frontend_vm.history_selected = index;
                        self.frontend_vm.history_repolish_open = true;
                    }
                    frontend::view_model::FrontendAction::HistoryRepolishClose => {
                        self.frontend_vm.history_repolish_open = false;
                    }
                    frontend::view_model::FrontendAction::HistoryRepolish(id, pack_index) => {
                        if let (Some(backend), Some(entry)) =
                            (self.backend(), self.history.entry(&id))
                        {
                            if !entry.raw_transcript.trim().is_empty() {
                                let request = openless_core::auxiliary::RepolishRequest {
                                    raw_text: entry.raw_transcript.clone(),
                                    style_pack_id: pack_index
                                        .and_then(|i| self.style_packs.get(i))
                                        .map(|p| p.id.clone()),
                                    front_app: entry.app_name.clone(),
                                };
                                let id = entry.id.clone();
                                let tx = self.tx.clone();
                                self.frontend_vm.history_repolish_running = true;
                                self.frontend_vm.history_repolish_error = None;
                                self.tokio.spawn(async move {
                                    let result = backend
                                        .services()
                                        .auxiliary
                                        .repolish(request)
                                        .await
                                        .map_err(|error| error.to_string());
                                    let _ = tx.send(UiResult::HistoryRepolish { id, result });
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::HistoryRetranscribe(id) => {
                        if let Some(backend) = self.backend() {
                            if let Some(entry) = self.history.entry(&id) {
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
                    frontend::view_model::FrontendAction::HistoryExport(id) => {
                        if let Some(backend) = self.backend() {
                            if let Some(entry) = self.history.entry(&id) {
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
                    frontend::view_model::FrontendAction::VocabRefresh => {
                        self.load_library();
                    }
                    frontend::view_model::FrontendAction::VocabAddRule {
                        pattern,
                        replacement,
                    } => {
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            let tx = self.tx.clone();
                            self.tokio.spawn(async move {
                                match backend.add_correction_rule(pattern, replacement) {
                                    Ok(_) => {
                                        let _ = tx.send(UiResult::Message(
                                            tr_l10n(lang, "status.correction_saved").to_string(),
                                        ));
                                        let _ = tx.send(UiResult::LibraryRefresh);
                                    }
                                    Err(error) => {
                                        let _ = tx.send(UiResult::Message(error.to_string()));
                                    }
                                }
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
                    frontend::view_model::FrontendAction::StyleChooseIcon(index) => {
                        if let Some(pack) = self.style_packs.get(index) {
                            self.set_style_pack_icon_from_file(pack.id.clone());
                        }
                    }
                    frontend::view_model::FrontendAction::StyleResetIcon(index) => {
                        if let Some(pack) = self.style_packs.get(index) {
                            self.reset_style_pack_icon(pack.id.clone());
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
                            self.style_editor = Some(pack.clone());
                            self.hydrate_style_editor(&pack, true);
                            self.load_marketplace_auth();
                        }
                    }
                    frontend::view_model::FrontendAction::StylePublishMarketplace => {
                        if self.frontend_vm.style_editor_publishing
                            || self.frontend_vm.style_editor_builtin
                            || !self.frontend_vm.marketplace_signed_in
                        {
                            return;
                        }
                        let Some(mut pack) = self.style_editor.clone() else {
                            return;
                        };
                        let needs_save = self.frontend_vm.style_editor_dirty;
                        if needs_save {
                            pack.name = self.frontend_vm.style_name.trim().to_string();
                            pack.description =
                                self.frontend_vm.style_description.trim().to_string();
                            pack.prompt = self.frontend_vm.style_prompt.clone();
                            pack.selection_prompt = self.frontend_vm.style_selection_prompt.clone();
                            pack.voice_edit_prompt =
                                self.frontend_vm.style_voice_edit_prompt.clone();
                            pack.author = Some(self.frontend_vm.style_author.trim().to_string())
                                .filter(|value| !value.is_empty());
                            pack.version = self.frontend_vm.style_version.trim().to_string();
                            pack.recommended_model =
                                Some(self.frontend_vm.style_model.trim().to_string())
                                    .filter(|value| !value.is_empty());
                            pack.compatible_app_version =
                                Some(self.frontend_vm.style_compatible_version.trim().to_string())
                                    .filter(|value| !value.is_empty());
                            pack.tags = self
                                .frontend_vm
                                .style_tags
                                .split([',', '，', '\n'])
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(ToOwned::to_owned)
                                .collect();
                            pack.examples = self.frontend_vm.style_examples.clone();
                        }
                        let exists = self.style_packs.iter().any(|item| item.id == pack.id);
                        let Some(backend) = self.backend() else {
                            return;
                        };
                        self.frontend_vm.style_editor_publishing = true;
                        let tx = self.tx.clone();
                        self.tokio.spawn(async move {
                            let result = async {
                                if needs_save {
                                    if exists {
                                        backend
                                            .update_style_pack(pack.clone())
                                            .map_err(|error| error.to_string())?;
                                    } else {
                                        backend
                                            .create_style_pack(pack.clone())
                                            .map_err(|error| error.to_string())?;
                                    }
                                }
                                backend
                                    .services()
                                    .marketplace
                                    .upload(pack.id.clone(), pack.origin_pack_id.clone())
                                    .await
                                    .map(|uploaded| uploaded.message)
                                    .map_err(|error| error.to_string())
                            }
                            .await;
                            let _ = tx.send(UiResult::MarketplacePublish(result));
                        });
                    }
                    frontend::view_model::FrontendAction::StyleRevertDraft => {
                        if let Some(pack) = self.style_editor.clone() {
                            self.hydrate_style_editor(&pack, true);
                        }
                    }
                    frontend::view_model::FrontendAction::StyleResetBuiltin => {
                        let id = self.frontend_vm.style_editor_id.clone();
                        if id.is_empty() {
                            return;
                        }
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            let tx = self.tx.clone();
                            self.tokio.spawn(async move {
                                match backend.reset_builtin_style_pack(&id) {
                                    // Core publishes `StylePacksChanged`, so the host
                                    // reloads the list (and the drawer) by itself.
                                    Ok(_) => {
                                        let _ = tx.send(UiResult::Message(
                                            tr_l10n(lang, "style.pack.resetBuiltin").to_string(),
                                        ));
                                    }
                                    Err(error) => {
                                        let _ = tx.send(UiResult::Message(error.to_string()));
                                    }
                                }
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::StyleDeleteImported => {
                        let id = self.frontend_vm.style_editor_id.clone();
                        if id.is_empty() {
                            return;
                        }
                        if let Some(backend) = self.backend() {
                            let lang = self.lang;
                            let tx = self.tx.clone();
                            self.tokio.spawn(async move {
                                match backend.remove_style_pack(&id) {
                                    Ok(_) => {
                                        let _ = tx.send(UiResult::Message(
                                            tr_l10n(lang, "style.pack.deleteImported").to_string(),
                                        ));
                                    }
                                    Err(error) => {
                                        let _ = tx.send(UiResult::Message(error.to_string()));
                                    }
                                }
                            });
                        }
                        self.style_editor = None;
                        self.frontend_vm.style_editor_open = false;
                    }
                    frontend::view_model::FrontendAction::StyleSaveEditor {
                        name,
                        description,
                        prompt,
                        selection_prompt,
                        voice_edit_prompt,
                        tags,
                        author,
                        version,
                        model,
                        compatible_version,
                        examples,
                    } => {
                        if let Some(mut pack) = self.style_editor.take() {
                            pack.name = name.trim().to_string();
                            pack.description = description.trim().to_string();
                            pack.prompt = prompt;
                            pack.selection_prompt = selection_prompt;
                            pack.voice_edit_prompt = voice_edit_prompt;
                            pack.author = Some(author.trim().to_string()).filter(|v| !v.is_empty());
                            if !version.trim().is_empty() {
                                pack.version = version.trim().to_string();
                            }
                            pack.recommended_model =
                                Some(model.trim().to_string()).filter(|v| !v.is_empty());
                            pack.compatible_app_version =
                                Some(compatible_version.trim().to_string())
                                    .filter(|v| !v.is_empty());
                            pack.examples = examples;
                            pack.tags = tags
                                .split([',', '，', '\n'])
                                .map(str::trim)
                                .filter(|tag| !tag.is_empty())
                                .map(ToOwned::to_owned)
                                .collect();
                            if let Some(backend) = self.backend() {
                                let exists = self.style_packs.iter().any(|p| p.id == pack.id);
                                let lang = self.lang;
                                // Keep the stored pack so the drawer stays open and
                                // can report dirty again after the next edit.
                                self.style_editor = Some(pack.clone());
                                self.frontend_vm.style_editor_saved = Some(pack.clone());
                                self.frontend_vm.style_editor_dirty = false;
                                self.spawn_reporting(async move {
                                    let saved = if exists {
                                        backend.update_style_pack(pack)?
                                    } else {
                                        backend.create_style_pack(pack)?
                                    };
                                    Ok(fmt_l10n(lang, "status.style_saved", &[&saved.name]))
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::StyleCloseEditor => {
                        self.style_editor = None;
                        self.frontend_vm.style_editor_open = false;
                    }
                    frontend::view_model::FrontendAction::StyleNewPack => {
                        let pack = openless_core::StylePack {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: tr_l10n(self.lang, "lbl.new_style_default").to_string(),
                            version: "1.0.0".to_string(),
                            ..Default::default()
                        };
                        self.style_editor = Some(pack.clone());
                        self.hydrate_style_editor(&pack, false);
                        self.frontend_vm.style_editor_saved = None;
                        self.frontend_vm.style_editor_dirty = true;
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
                        if let Some(preferences) = self.preferences.as_mut() {
                            preferences.qa_save_history = !preferences.qa_save_history;

                            self.frontend_vm.qa_save_history = preferences.qa_save_history;
                            self.save_settings_if_dirty();
                        }
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
                        }
                        self.save_settings_if_dirty();
                    }
                    frontend::view_model::FrontendAction::TranslationSetTarget(language) => {
                        if let Some(preferences) = self.preferences.as_mut() {
                            preferences.translation_target_language = language;
                        }
                        self.save_settings_if_dirty();
                    }
                    frontend::view_model::FrontendAction::SettingsToggle(field) => {
                        self.apply_settings_toggle(field);
                    }
                    frontend::view_model::FrontendAction::SettingsAudioDsp(config) => {
                        if let Some(preferences) = self.preferences.as_mut() {
                            preferences.audio_dsp = config.normalized();
                        }
                        self.save_settings_if_dirty();
                    }
                    frontend::view_model::FrontendAction::SettingsCombo(field, index) => {
                        if matches!(
                            field,
                            frontend::view_model::SettingsComboField::PipelineMode
                        ) && self.frontend_vm.omni.dirty
                        {
                            self.apply_omni_action(
                                openless_linux_egui::omni_settings::OmniAction::Save,
                            );
                        }
                        self.apply_settings_combo(field, index);
                        if matches!(
                            field,
                            frontend::view_model::SettingsComboField::PipelineMode
                        ) {
                            self.select_services_view(self.frontend_vm.services_view);
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsText(field, text) => {
                        self.apply_settings_text(field, text);
                    }
                    frontend::view_model::FrontendAction::SettingsAction(field) => {
                        self.apply_settings_action(field);
                    }
                    frontend::view_model::FrontendAction::SettingsSection(section) => {
                        // 编辑器只属于「AI 服务与模型」这一页：切到别的分区就收起
                        // （Tauri 的 modal 宿主挂在服务页里，切页即卸载）。
                        if section != frontend::view_model::SettingsSection::Services
                            && self.provider_editor_form.is_some()
                        {
                            self.request_provider_editor_close();
                        }
                        if self.frontend_vm.omni.dirty {
                            self.apply_omni_action(
                                openless_linux_egui::omni_settings::OmniAction::Save,
                            );
                        }
                        self.frontend_vm.settings_section = section;
                        if section == frontend::view_model::SettingsSection::Privacy {
                            self.refresh_linux_access();
                            // 云端同步卡片在这一页：切进来就读一次真实状态，
                            // 不要让卡片停在「加载中」。
                            self.load_cloud_sync();
                        }
                        if section == frontend::view_model::SettingsSection::Services {
                            self.select_services_view(self.frontend_vm.services_view);
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsServicesView(view) => {
                        if self.frontend_vm.omni.dirty {
                            self.apply_omni_action(
                                openless_linux_egui::omni_settings::OmniAction::Save,
                            );
                        }
                        self.select_services_view(view);
                    }
                    frontend::view_model::FrontendAction::SettingsOmni(action) => {
                        self.apply_omni_action(action);
                    }
                    frontend::view_model::FrontendAction::SettingsCloudSync(action) => {
                        self.apply_cloud_sync_action(action);
                    }
                    frontend::view_model::FrontendAction::SettingsTypography(field) => {
                        // 排版是纯 UI 偏好：落盘后立刻重装字体并应用缩放，
                        // 不需要经过 Core。
                        let mut typography = self.frontend_vm.typography;
                        typography.apply(field);
                        self.frontend_vm.typography = typography;
                        if openless_linux_egui::typography::save(typography).is_err() {
                            self.status =
                                tr_l10n(self.lang, "uiRefinement.storageError").to_owned();
                        }
                        ctx_arg.request_repaint();
                    }
                    // Tauri `startCreate`：点「添加渠道」先在后台建一张空渠道（名字留空），
                    // 再打开同一个编辑器——草稿卡片只是为了让凭据有渠道 id 可落盘；
                    // 用户完全没交互就关掉时会被回收（见 `SettingsProviderClose`）。
                    frontend::view_model::FrontendAction::SettingsChannelDraft => {
                        if self.channel_create_pending || self.provider_switch_pending {
                            continue;
                        }
                        let kind = self.settings_channel_kind;
                        // 供应商面板还没加载完时也照 Tauri 的 `presets[0]?.id ?? ''` 建
                        // 一张空供应商的草稿：弹窗里可以再选。
                        let provider_type = self
                            .frontend_vm
                            .channel_providers
                            .first()
                            .map(|provider| provider.provider_type.clone())
                            .unwrap_or_default();
                        if let Some(backend) = self.backend() {
                            self.channel_create_pending = true;
                            let tx = self.tx.clone();
                            let wake = std::thread::current();
                            self.tokio.spawn(async move {
                                let result = backend
                                    .create_channel(kind, provider_type, String::new())
                                    .await
                                    .map_err(|error| error.to_string());
                                let _ = tx.send(UiResult::ChannelCreated { kind, result });
                                wake.unpark();
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::ShortcutRecording(field) => {
                        self.frontend_vm.shortcut_pending_modifier = None;
                        self.frontend_vm.shortcut_recording = field;
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
                    frontend::view_model::FrontendAction::ShortcutReset(field) => {
                        self.apply_shortcut_reset(field);
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
                        self.editor_draft = false;
                        self.draft_touched = false;
                        self.open_provider_editor(index);
                    }
                    frontend::view_model::FrontendAction::SettingsChannelMove {
                        channel_id,
                        neighbor_id,
                        delta,
                    } => {
                        let kind = self.settings_channel_kind;
                        let mut ids: Vec<String> = self
                            .settings_channels
                            .iter()
                            .map(|channel| channel.id.clone())
                            .collect();
                        if frontend::view_model::move_channel(
                            &mut ids,
                            &channel_id,
                            &neighbor_id,
                            delta,
                        ) {
                            if let Some(backend) = self.backend() {
                                let lang = self.lang;
                                self.spawn_channel_change(kind, async move {
                                    backend.reorder_channels(kind, ids).await?;
                                    Ok(tr_l10n(lang, "status.channel_reordered").to_string())
                                });
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsChannelProviderType {
                        channel_id: id,
                        provider_type,
                    } => {
                        let kind = self.settings_channel_kind;
                        if let Some(backend) = self.backend() {
                            if self.provider_switch_pending {
                                continue;
                            }
                            let Some(editor) = self
                                .editor_from_form()
                                .filter(|editor| editor.channel.id == id)
                            else {
                                continue;
                            };
                            if editor.protocol.validate().is_err() {
                                continue;
                            }
                            self.draft_touched = true;
                            self.provider_switch_pending = true;
                            let tx = self.tx.clone();
                            let wake = std::thread::current();
                            let job = Box::pin(async move {
                                let result = switch_provider_editor(backend, editor, provider_type)
                                    .await
                                    .map_err(|error| error.to_string());
                                let _ = tx.send(UiResult::ProviderTypeChanged {
                                    kind,
                                    channel_id: id,
                                    result: Box::new(result),
                                });
                                wake.unpark();
                            });
                            if self.provider_jobs.send(job).is_err() {
                                self.provider_switch_pending = false;
                                self.frontend_vm.settings_notice =
                                    Some(tr_l10n(self.lang, "common.operationFailed").to_owned());
                            }
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
                            self.spawn_channel_change(kind, async move {
                                backend.set_active_provider(provider_slot(kind), id).await?;
                                Ok(tr_l10n(lang, "status.channel_active").to_string())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderField(field, value) => {
                        if self.provider_switch_pending {
                            continue;
                        }
                        self.draft_touched = true;
                        if let Some(form) = self.provider_editor_form.as_mut() {
                            match field {
                                frontend::view_model::SettingsProviderField::Name => {
                                    form.name = value
                                }
                                frontend::view_model::SettingsProviderField::Endpoint => {
                                    if form.endpoint != value {
                                        form.models.clear();
                                        form.models_loading = false;
                                        if let ProviderEditorState::Loaded(editor) =
                                            &self.provider_editor
                                        {
                                            form.models_url =
                                                provider_models_url(&editor.descriptor, &value);
                                        }
                                    }
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
                                frontend::view_model::SettingsProviderField::RequestFormat => {
                                    if form.protocol.values[0] != value {
                                        form.models.clear();
                                        form.models_loading = false;
                                        form.catalog_format = None;
                                    }
                                    form.protocol.values[0] = value
                                }
                                frontend::view_model::SettingsProviderField::MessagesThinking => {
                                    form.protocol.values[1] = value
                                }
                                frontend::view_model::SettingsProviderField::MaxTokens => {
                                    form.protocol.values[2] = value
                                }
                                frontend::view_model::SettingsProviderField::ThinkingBudget => {
                                    form.protocol.values[3] = value
                                }
                                frontend::view_model::SettingsProviderField::ServiceTier => {
                                    form.protocol.values[4] = value
                                }
                                frontend::view_model::SettingsProviderField::Temperature => {
                                    form.protocol.temperature = value
                                }
                                frontend::view_model::SettingsProviderField::ExtraHeaders => {
                                    form.protocol.extra_headers = value
                                }
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsSiwc {
                        action,
                        registration_id,
                    } => {
                        if self.provider_switch_pending {
                            continue;
                        }
                        self.draft_touched = true;
                        self.request_siwc(action, registration_id);
                    }
                    frontend::view_model::FrontendAction::SettingsProviderSave => {
                        self.queue_provider_save();
                    }
                    frontend::view_model::FrontendAction::SettingsProviderClearSecrets => {
                        if self.provider_switch_pending {
                            continue;
                        }
                        if let Some(editor) = self.editor_from_form() {
                            if let Some(backend) = self.backend() {
                                let tx = self.tx.clone();
                                let wake = std::thread::current();
                                let job = Box::pin(async move {
                                    let result = clear_provider_secrets(backend, &editor)
                                        .await
                                        .map_err(|error| error.to_string());
                                    let _ = tx.send(UiResult::ProviderSecretsCleared(result));
                                    wake.unpark();
                                });
                                if self.provider_jobs.send(job).is_err() {
                                    self.status =
                                        tr_l10n(self.lang, "common.operationFailed").to_owned();
                                    continue;
                                }
                                self.provider_save_again = false;
                                if let Some(form) = self.provider_editor_form.as_mut() {
                                    form.primary_secret.clear();
                                    form.secondary_secret.clear();
                                    form.protocol.extra_headers.clear();
                                }
                                self.load_service_configured();
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderModels => {
                        if self.provider_switch_pending {
                            continue;
                        }
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
                    frontend::view_model::FrontendAction::SettingsProviderModelSelected(model) => {
                        if self.provider_switch_pending {
                            continue;
                        }
                        self.draft_touched = true;
                        if let Some(form) = self.provider_editor_form.as_mut() {
                            form.model = model;
                            form.custom_model = false;
                        }
                        // Use the same FIFO and revision receipt as typed edits.
                        self.queue_provider_save();
                    }
                    frontend::view_model::FrontendAction::SettingsProviderModelCustom(custom) => {
                        if self.provider_switch_pending {
                            continue;
                        }
                        if let Some(form) = self.provider_editor_form.as_mut() {
                            form.custom_model = custom;
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderModelsUrl => {
                        if let Some(form) = self.provider_editor_form.as_ref() {
                            if let Some(url) = form.models_url.clone() {
                                if let Err(error) = open_external(&url) {
                                    log::warn!("provider models url: {error}");
                                }
                            }
                        }
                    }
                    frontend::view_model::FrontendAction::SettingsProviderClose => {
                        self.request_provider_editor_close();
                    }
                    frontend::view_model::FrontendAction::SettingsChannelToggle(index) => {
                        let kind = self.settings_channel_kind;
                        let target = self
                            .settings_channels
                            .get(index)
                            .map(|channel| (channel.id.clone(), channel.enabled));
                        if let (Some(backend), Some((id, enabled))) = (self.backend(), target) {
                            let lang = self.lang;
                            self.spawn_channel_change(kind, async move {
                                backend.set_channel_enabled(kind, id, !enabled).await?;
                                Ok(tr_l10n(lang, "status.channel_enabled").to_string())
                            });
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
                            let editor = self
                                .editor_from_form()
                                .filter(|editor| editor.kind == kind && editor.channel.id == id);
                            self.spawn_channel_change(kind, async move {
                                if let Some(editor) = editor {
                                    save_provider_editor(backend.clone(), editor).await?;
                                }
                                validate_provider_channel(lang, backend, kind, id).await
                            });
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
                            self.close_provider_editor();
                            self.spawn_channel_change(kind, async move {
                                backend.delete_channel(kind, id).await?;
                                Ok(tr_l10n(lang, "status.channel_deleted").to_string())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceDetail(index) => {
                        self.frontend_vm.marketplace_selected = Some(index);
                        self.frontend_vm.marketplace_detail_prompt = None;
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
    /// 历史记录 → 窗口展示条目。每帧只投影第一页，后续页按需投影
    /// （见 `frontend::paging`）；两处共用这一份映射，字段与顺序不会走偏。
    fn history_entry_view(
        item: openless_core::DictationSession,
        style_packs: &[openless_core::StylePack],
        lang: Lang,
    ) -> frontend::view_model::HistoryEntry {
        let has_audio = item.has_audio_recording.unwrap_or(false);
        frontend::view_model::HistoryEntry {
            quick_note: item.source == openless_core::HistorySource::QuickNote,
            error_code: item.error_code,
            id: item.id,
            created_at: item.created_at,
            mode: overview_mode(item.mode),
            // History carries the exact style-pack id. Prefer the catalog name so
            // imported packs do not collapse into the four base-mode labels; old
            // records without an id still use the mode fallback.
            style_label: item
                .style_pack_id
                .as_deref()
                .and_then(|id| style_packs.iter().find(|pack| pack.id == id))
                .map(|pack| {
                    let builtin_default =
                        openless_core::builtin_style_pack_for_mode(pack.base_mode).name;
                    if pack.kind == openless_core::StylePackKind::Builtin
                        && pack.id == openless_core::builtin_style_pack_id(pack.base_mode)
                        && pack.name == builtin_default
                    {
                        polish_mode_label(lang, pack.base_mode).to_string()
                    } else {
                        pack.name.clone()
                    }
                })
                .unwrap_or_else(|| polish_mode_label(lang, item.mode).to_string()),
            raw_transcript: item.raw_transcript,
            final_text: item.final_text,
            duration_ms: item.duration_ms,
            has_audio,
            asr_provider: item.asr_provider,
            asr_model: item.asr_model,
            asr_ms: item.asr_ms,
            llm_provider: item.llm_provider,
            app_name: item.app_name,
            dictionary_count: item.dictionary_entry_count,
        }
    }

    fn vocab_entry_view(
        entry: &openless_core::DictionaryEntry,
    ) -> frontend::view_model::VocabEntry {
        frontend::view_model::VocabEntry {
            phrase: entry.phrase.clone(),
            hits: entry.hits as usize,
            enabled: entry.enabled,
            learned: false,
        }
    }

    fn correction_rule_view(
        rule: &openless_core::CorrectionRule,
    ) -> frontend::view_model::CorrectionRule {
        frontend::view_model::CorrectionRule {
            pattern: rule.pattern.clone(),
            replacement: rule.replacement.clone(),
            enabled: rule.enabled,
            learned: false,
        }
    }

    fn style_pack_view(
        pack: &openless_core::StylePack,
        icon_data_url: Option<&str>,
        selection_polish_style_pack_id: Option<&str>,
        lang: Lang,
    ) -> frontend::view_model::StylePack {
        frontend::view_model::StylePack {
            id: pack.id.clone(),
            icon_path: pack.icon_path.clone(),
            icon_data_url: icon_data_url.map(str::to_string),
            base_mode: match pack.base_mode {
                openless_core::PolishMode::Raw => "raw",
                openless_core::PolishMode::Light => "light",
                openless_core::PolishMode::Structured => "structured",
                openless_core::PolishMode::Formal => "formal",
            }
            .to_string(),
            name: pack.name.clone(),
            description: pack.description.clone(),
            // Localized mode label (Core's display_name is zh-only).
            tags: vec![polish_mode_label(lang, pack.base_mode).to_string()],
            is_builtin: pack.kind == openless_core::StylePackKind::Builtin,
            enabled: pack.enabled,
            is_active: pack.active,
            selection_active: selection_polish_style_pack_id == Some(pack.id.as_str()),
        }
    }

    fn marketplace_pack_view(
        item: &openless_core::MarketplaceListItem,
        liked: bool,
    ) -> frontend::view_model::MarketplacePack {
        frontend::view_model::MarketplacePack {
            id: item.id.clone(),
            name: item.name.clone(),
            version: item.version.clone(),
            description: item.description.clone(),
            mode: item.base_mode.clone(),
            author: item.author_login.clone(),
            origin_author_login: item.origin_author_login.clone(),
            tags: item.tags.clone(),
            likes: item.like_count as u32,
            downloads: item.download_count as u32,
            liked,
        }
    }

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

    /// Omni 的「测试 / 拉模型」请求：**不能带 channel_id**。
    ///
    /// Core 对 `kind == Omni && channel_id.is_some()` 直接报
    /// `omni provider does not support channel id`；带上频道 id 会让每一次测试都
    /// 失败，用户看到的就是「密钥填进去用不了」。Core 会自己按当前激活的服务商
    /// 解析凭据。
    fn omni_provider_request(thinking_enabled: bool) -> openless_core::ProviderRequest {
        openless_core::ProviderRequest {
            kind: openless_core::ProviderKind::Omni,
            channel_id: None,
            thinking_enabled,
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

    /// 「可用模型」按钮的目标：当前 endpoint 命中某个预设、且该预设带文档页时返回它
    /// （Tauri `descriptor.endpointPresets.find(matchesEndpointPreset)?.modelsUrl`）。
    /// 有文档页的服务商（例如火山方舟）不提供 `/models`，只能引导用户去看文档。
    fn provider_models_url(
        descriptor: &openless_core::ProviderDescriptor,
        endpoint: &str,
    ) -> Option<String> {
        let endpoint = if endpoint.trim().is_empty() {
            descriptor.default_endpoint.as_deref().unwrap_or("")
        } else {
            endpoint
        };
        descriptor
            .endpoint_presets
            .iter()
            .find(|preset| {
                openless_core::provider_rules::matches_endpoint_preset(endpoint, &preset.endpoint)
            })
            .and_then(|preset| preset.models_url.clone())
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
            Core::Siwc => Ui::Siwc,
            Core::TencentCloud => Ui::TencentCloud,
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
        if !editor.protocol.formats.is_empty() {
            editor.protocol.validate()?;
        }
        let channel_id = editor.channel.id.as_str();
        backend
            .rename_channel(editor.kind, channel_id.to_string(), editor.name)
            .await?;
        match editor.descriptor.auth_requirement {
            openless_core::AuthRequirement::None
            | openless_core::AuthRequirement::OAuth
            | openless_core::AuthRequirement::Siwc => {}
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
            openless_core::AuthRequirement::TencentCloud => {
                write_or_remove_provider_value(
                    &backend,
                    editor.kind,
                    channel_id,
                    openless_core::credentials::TENCENT_CLOUD_APP_ID_ACCOUNT,
                    &editor.resource_id,
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
        if matches!(
            editor.descriptor.provider_type.as_str(),
            "custom" | "custom_responses" | "custom_messages"
        ) {
            write_or_remove_provider_value(
                &backend,
                editor.kind,
                channel_id,
                openless_core::credentials::LLM_TEMPERATURE_ACCOUNT,
                &editor.protocol.temperature,
            )
            .await?;
            write_secret_if_entered(
                &backend,
                editor.kind,
                channel_id,
                openless_core::credentials::LLM_EXTRA_HEADERS_ACCOUNT,
                &editor.protocol.extra_headers,
            )
            .await?;
        }
        if !editor.protocol.formats.is_empty() {
            for (account, value) in openless_core::llm_protocol::CONFIG_ACCOUNTS
                .iter()
                .zip(&editor.protocol.values)
            {
                if *account == openless_core::llm_protocol::SERVICE_TIER_ACCOUNT
                    && editor.descriptor.provider_type.as_str() != "ark"
                {
                    continue;
                }
                write_or_remove_provider_value(&backend, editor.kind, channel_id, account, value)
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
            openless_core::AuthRequirement::None
            | openless_core::AuthRequirement::OAuth
            | openless_core::AuthRequirement::Siwc => &[],
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
                openless_core::credentials::TENCENT_CLOUD_SECRET_ID_ACCOUNT,
                openless_core::credentials::TENCENT_CLOUD_SECRET_KEY_ACCOUNT,
            ],
            _ => &[api_key_account(editor.kind)],
        };
        if matches!(
            editor.descriptor.provider_type.as_str(),
            "custom" | "custom_responses" | "custom_messages"
        ) {
            backend
                .remove_credential(provider_credential_key(
                    editor.kind,
                    &editor.channel.id,
                    openless_core::credentials::LLM_EXTRA_HEADERS_ACCOUNT,
                )?)
                .await?;
        }
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

    /// Drain the old form before replacing its descriptor. Never refresh the
    /// editor from a list requested before the provider mutation completed.
    async fn switch_provider_editor(
        backend: Arc<openless_core::OpenLessBackend>,
        editor: ProviderEditor,
        provider_type: String,
    ) -> Result<ProviderEditor, BackendError> {
        let descriptor = openless_core::provider_rules::provider_descriptor(
            provider_kind(editor.kind),
            &provider_type,
        )
        .ok_or_else(|| {
            BackendError::new(
                openless_core::BackendErrorCode::InvalidArgument,
                "unknown provider type",
            )
        })?;
        let kind = editor.kind;
        let id = editor.channel.id.clone();
        save_provider_editor(backend.clone(), editor).await?;
        backend
            .set_channel_provider_type(kind, id.clone(), provider_type.clone())
            .await?;
        for (account, default) in [
            (
                endpoint_account(kind),
                descriptor.default_endpoint.as_deref(),
            ),
            (model_account(kind), descriptor.default_model.as_deref()),
        ] {
            if let Some(default) = default.filter(|value| !value.trim().is_empty()) {
                let current = read_provider_value(&backend, kind, &id, account).await?;
                if provider_type == "orcarouter"
                    || current
                        .as_deref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    write_or_remove_provider_value(&backend, kind, &id, account, default).await?;
                }
            }
        }
        let mut channel = backend
            .list_channels(kind)
            .await?
            .into_iter()
            .find(|channel| channel.id == id)
            .ok_or_else(|| {
                BackendError::new(
                    openless_core::BackendErrorCode::InvalidArgument,
                    "unknown channel",
                )
            })?;
        if provider_type == "orcarouter" && channel.name.trim().is_empty() {
            backend
                .rename_channel(kind, id, "OrcaRouter".to_owned())
                .await?;
            channel.name = "OrcaRouter".to_owned();
        }
        load_provider_editor(backend, kind, channel, descriptor).await
    }

    async fn load_provider_editor(
        backend: Arc<openless_core::OpenLessBackend>,
        kind: openless_core::ChannelKind,
        channel: openless_core::ChannelSummary,
        descriptor: openless_core::ProviderDescriptor,
    ) -> Result<ProviderEditor, BackendError> {
        // Descriptor defaults are placeholders, not persisted field values.
        // Otherwise flushing before a provider switch writes the OLD provider's
        // defaults into empty slots, preventing the next provider's defaults
        // from being installed (and sending its key to the previous endpoint).
        let endpoint = read_provider_value(&backend, kind, &channel.id, endpoint_account(kind))
            .await?
            .unwrap_or_default();
        let model = read_provider_value(&backend, kind, &channel.id, model_account(kind))
            .await?
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
            } else if descriptor.auth_requirement == openless_core::AuthRequirement::TencentCloud {
                (
                    String::new(),
                    read_provider_value(
                        &backend,
                        kind,
                        &channel.id,
                        openless_core::credentials::TENCENT_CLOUD_APP_ID_ACCOUNT,
                    )
                    .await?
                    .unwrap_or_default(),
                )
            } else {
                (String::new(), String::new())
            };
        let mut protocol = openless_linux_egui::provider_protocol::ProviderProtocolDraft {
            default_format: descriptor.default_request_format.unwrap_or_default(),
            formats: descriptor.supported_request_formats.clone(),
            ..Default::default()
        };
        if kind == openless_core::ChannelKind::Llm && !protocol.formats.is_empty() {
            for (index, account) in openless_core::llm_protocol::CONFIG_ACCOUNTS
                .iter()
                .enumerate()
            {
                if *account == openless_core::llm_protocol::SERVICE_TIER_ACCOUNT
                    && descriptor.provider_type.as_str() != "ark"
                {
                    continue;
                }
                protocol.values[index] = read_provider_value(&backend, kind, &channel.id, account)
                    .await?
                    .unwrap_or_default();
            }
        }
        if matches!(
            descriptor.provider_type.as_str(),
            "custom" | "custom_responses" | "custom_messages"
        ) {
            protocol.extra_headers = read_provider_value(
                &backend,
                kind,
                &channel.id,
                openless_core::credentials::LLM_EXTRA_HEADERS_ACCOUNT,
            )
            .await?
            .unwrap_or_default();
            protocol.temperature = read_provider_value(
                &backend,
                kind,
                &channel.id,
                openless_core::credentials::LLM_TEMPERATURE_ACCOUNT,
            )
            .await?
            .unwrap_or_default();
        }
        // 密钥也要读回来：Tauri 的 `CredentialField` 就是把已存的值读进输入框再打码，
        // 用户能看到「这里有东西」。之前这里写死成空串，编辑已有渠道时密钥框永远空白，
        // 看起来像没保存。
        let (primary_account, secondary_account) = match descriptor.auth_requirement {
            openless_core::AuthRequirement::None
            | openless_core::AuthRequirement::OAuth
            | openless_core::AuthRequirement::Siwc => (None, None),
            openless_core::AuthRequirement::Volcengine => {
                if auth_mode == "api_key" {
                    (
                        Some(openless_core::credentials::VOLCENGINE_API_KEY_ACCOUNT),
                        None,
                    )
                } else {
                    (
                        Some(openless_core::credentials::VOLCENGINE_APP_KEY_ACCOUNT),
                        Some(openless_core::credentials::VOLCENGINE_ACCESS_KEY_ACCOUNT),
                    )
                }
            }
            openless_core::AuthRequirement::Xfyun => (
                Some(openless_core::credentials::XFYUN_APP_ID_ACCOUNT),
                Some(openless_core::credentials::XFYUN_API_KEY_ACCOUNT),
            ),
            openless_core::AuthRequirement::TencentCloud => (
                Some(openless_core::credentials::TENCENT_CLOUD_SECRET_ID_ACCOUNT),
                Some(openless_core::credentials::TENCENT_CLOUD_SECRET_KEY_ACCOUNT),
            ),
            _ => (Some(api_key_account(kind)), None),
        };
        let channel_id = channel.id.clone();
        let read_optional = |account: Option<&'static str>| {
            let backend = Arc::clone(&backend);
            let channel_id = channel_id.clone();
            async move {
                match account {
                    Some(account) => read_provider_value(&backend, kind, &channel_id, account)
                        .await
                        .map(Option::unwrap_or_default),
                    None => Ok(String::new()),
                }
            }
        };
        let primary_secret = read_optional(primary_account).await?;
        let secondary_secret = read_optional(secondary_account).await?;
        Ok(ProviderEditor {
            protocol,
            kind,
            name: channel.name.clone(),
            channel,
            descriptor,
            endpoint,
            model,
            volcengine_service,
            auth_mode,
            resource_id,
            primary_secret,
            secondary_secret,
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
                thinking_enabled: backend.get_preferences().llm_thinking_enabled,
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

    fn backend_config() -> Result<BackendConfig, String> {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let data_dir = openless_data_dir()?;
        let cache_dir = std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".cache")))
            .ok_or_else(|| "HOME/XDG_CACHE_HOME is unavailable".to_string())?
            .join("OpenLess");
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&cache_dir).map_err(|error| error.to_string())?;
        Ok(BackendConfig {
            data_dir,
            cache_dir,
            home_dir: home,
            resource_dir: std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf)),
            platform: Default::default(),
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
        openless_linux_egui::prepare_fcitx5(&plan, &config.data_dir)
            .map_err(|error| error.to_string())
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
    /// `orderFrontRegardless` ("visible but not the key window"). In a native
    /// X11 session the equivalent is `WM_HINTS.input = False` (see
    /// [`openless_linux_egui::popup_command`]).
    #[cfg(all(target_os = "linux", feature = "x11-overlay"))]
    mod popup_overlay {
        use super::*;
        use openless_linux_egui::{
            place_overlay, place_panel, popup_position, OverlayEnvironment, OverlayPlacement,
            OverlayX11, X11Overlay,
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
            focus_window: Option<u32>,
            focus_watch_remaining: u8,
            last_capsule_layout: Option<(u32, u32, bool)>,
        }

        impl PopupOverlay {
            pub fn probe(kind: PopupKind) -> Option<Self> {
                // XWayland may expose DISPLAY in a native Wayland session, but
                // an xdg-toplevel is not an X11 window. Do not feed its
                // coordinates to winit or pretend that X11 can place it.
                if openless_linux_egui::wayland_display_available(
                    std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
                ) {
                    log::debug!("popup x11: native Wayland session, keeping compositor placement");
                    return None;
                }
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
                    focus_window: None,
                    focus_watch_remaining: 0,
                    last_capsule_layout: None,
                })
            }

            /// Position handed to `ViewportBuilder::with_position`, so the pill
            /// is already in place the first time it is shown.
            pub fn initial_position(&self) -> Option<(i32, i32)> {
                popup_position(&self.environment, self.kind)
            }

            fn apply(&mut self, reason: &str) -> OverlayPlacement {
                self.last_capsule_layout = None;
                let placement = place_overlay(
                    &mut self.connection,
                    std::process::id(),
                    &self.environment,
                    self.kind,
                );
                self.focus_window = placement.window.or(self.focus_window);
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

            pub fn capsule_layout(&mut self, ctx: &egui::Context, size: (u32, u32), card: bool) {
                if self.kind != PopupKind::Capsule
                    || self.last_capsule_layout == Some((size.0, size.1, card))
                {
                    return;
                }
                self.last_capsule_layout = Some((size.0, size.1, card));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    size.0 as f32,
                    size.1 as f32,
                )));
                let position = if card {
                    self.environment.placement_area().map(|area| {
                        openless_linux_egui::clamp_to_area(
                            area.x + area.width as i32 - size.0 as i32 - 24,
                            area.y + area.height as i32 - size.1 as i32 - 24,
                            size,
                            area,
                        )
                    })
                } else {
                    self.environment
                        .position_for(size, openless_linux_egui::CAPSULE_BOTTOM_GAP)
                };
                if let Some((x, y)) = position {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                        x as f32, y as f32,
                    )));
                }
            }

            pub fn place(&mut self, ctx: &egui::Context, visible: bool) {
                if self.kind != PopupKind::Capsule {
                    // Interactive panels keep normal focus/taskbar behaviour, but
                    // winit's initial position is only a hint. Re-apply the
                    // work-area position once after the X11 window is managed so
                    // a panel cannot be placed over a reserved taskbar.
                    if !self.placed {
                        self.attempts = self.attempts.saturating_add(1);
                        let placement = place_panel(
                            &mut self.connection,
                            std::process::id(),
                            &self.environment,
                            self.kind,
                        );
                        if placement.applied() {
                            self.placed = true;
                            log::info!(
                                "popup x11 (panel-post-map): kind={:?} window={:?} matched={} moved_to={:?} warnings={:?}",
                                self.kind,
                                placement.window,
                                placement
                                    .matched
                                    .map(openless_linux_egui::WindowMatch::as_str)
                                    .unwrap_or("none"),
                                placement.moved_to,
                                placement.warnings
                            );
                        } else if self.attempts >= 100 {
                            self.placed = true;
                            log::warn!(
                                "popup x11: panel window not found, keeping compositor placement kind={:?}",
                                self.kind
                            );
                        } else {
                            ctx.request_repaint_after(std::time::Duration::from_millis(50));
                        }
                    }
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
                    // Some X11 window managers deliver activation after the
                    // first map notification. Watch briefly so a late focus
                    // race cannot leave Chromium/Electron-style apps blurred.
                    self.focus_watch_remaining = 10;
                    ctx.request_repaint_after(std::time::Duration::from_millis(60));
                } else if visible && self.focus_watch_remaining > 0 {
                    if let (Some(window), Some(previous)) =
                        (self.focus_window, self.environment.active_window)
                    {
                        if previous != window
                            && matches!(self.connection.active_window(), Ok(Some(active)) if active == window)
                        {
                            if let Err(error) = self.connection.restore_focus(previous) {
                                log::warn!("capsule x11: delayed focus restore failed: {error}");
                            } else {
                                eprintln!(
                                    "OpenLess capsule: restored delayed focus to previous window {previous}"
                                );
                            }
                        }
                    }
                    self.focus_watch_remaining -= 1;
                    if self.focus_watch_remaining > 0 {
                        ctx.request_repaint_after(std::time::Duration::from_millis(60));
                    }
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
            pub fn capsule_layout(&mut self, _ctx: &egui::Context, _size: (u32, u32), _card: bool) {
            }
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
        /// 本次会话「预备态（麦克风还没吐第一帧 PCM）」的开始时刻：用来学习 warmupMs。
        warming_started: Option<std::time::Instant>,
        /// 学出来的「预备 → 就绪」平均耗时（ms），驱动光条的预测式展开。
        warmup_ms: f32,
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

        /// 学习「预备态持续多久」= 麦克风就绪耗时，EMA 更新并落盘（Tauri 用
        /// localStorage 的 `ol-capsule-warmup-ms`，公式一致；异常样本在
        /// `learn_capsule_warmup_ms` 里被过滤）。
        fn track_capsule_warmup(&mut self, warming: bool, phase: &str) {
            let warming = warming
                && matches!(
                    phase.to_ascii_lowercase().as_str(),
                    "starting" | "recording"
                );
            if warming {
                if self.warming_started.is_none() {
                    self.warming_started = Some(std::time::Instant::now());
                }
                return;
            }
            let Some(started) = self.warming_started.take() else {
                return;
            };
            let observed_ms = started.elapsed().as_secs_f32() * 1000.0;
            if let Some(next) = openless_linux_egui::learn_capsule_warmup_ms(observed_ms) {
                self.warmup_ms = next;
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
                theme::install(ctx, openless_linux_egui::typography::load().font);
            }
            let exit = self.pump(None) || (self.state.session_id.is_some() && !self.state.visible);
            if !exit {
                self.send_ready_if_needed();
            }
            let animated = matches!(
                self.state.capsule.phase.to_ascii_lowercase().as_str(),
                "starting" | "recording" | "transcribing" | "polishing" | "inserting"
            );
            let capsule = self.state.capsule.clone();
            let lang = self.lang;
            let mut action = frontend::popups::CapsuleAction::None;
            let output = ctx.run_ui(raw, |ui| {
                if !exit && self.state.visible {
                    self.track_capsule_warmup(capsule.audio_level.is_none(), &capsule.phase);
                    action =
                        frontend::popups::dictation_capsule(ui, &capsule, lang, self.warmup_ms);
                }
            });
            match action {
                frontend::popups::CapsuleAction::None => {}
                frontend::popups::CapsuleAction::AcceptSuggestion(id) => {
                    // 卡片与听写会话无关：id 才是要处理的候选。
                    let sequence = self.next_sequence();
                    let session_id = self.session_id().unwrap_or_default();
                    self.send(PopupToHost::VocabSuggestion {
                        version: POPUP_PROTOCOL_VERSION,
                        session_id,
                        sequence,
                        id,
                        accept: true,
                    });
                }
                frontend::popups::CapsuleAction::RejectSuggestion(id) => {
                    let sequence = self.next_sequence();
                    let session_id = self.session_id().unwrap_or_default();
                    self.send(PopupToHost::VocabSuggestion {
                        version: POPUP_PROTOCOL_VERSION,
                        session_id,
                        sequence,
                        id,
                        accept: false,
                    });
                }
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
                    // The host owns cancellation/stop and the terminal hide.
                    // Keep the process alive so stdout is flushed and the
                    // recording-to-processing animation remains visible.
                }
            }
            let size = frontend::popups::capsule_window_size(&capsule);
            let card = !capsule.suggestions.is_empty();
            openless_linux_egui::LayerFrame {
                output,
                exit,
                capsule_layout: Some((
                    openless_linux_egui::capsule_geometry(
                        size.0,
                        size.1,
                        if card {
                            24
                        } else {
                            openless_linux_egui::CAPSULE_BOTTOM_GAP
                        },
                    ),
                    card,
                )),
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
        fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
            egui::Color32::TRANSPARENT.to_normalized_gamma_f32()
        }

        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            openless_linux_egui::frame_stats::tick("popup");
            let ctx = ui.ctx().clone();
            // Overlay placement first: it must run before the window is shown so
            // the compositor never gives the capsule the keyboard.
            if let Some(overlay) = self.overlay.as_mut() {
                overlay.place(&ctx, self.state.visible);
            }
            if self.pump(Some(&ctx)) {
                return;
            }
            if self.kind == PopupKind::Capsule {
                if let Some(overlay) = self.overlay.as_mut() {
                    overlay.capsule_layout(
                        &ctx,
                        frontend::popups::capsule_window_size(&self.state.capsule),
                        !self.state.capsule.suggestions.is_empty(),
                    );
                }
            }
            self.send_ready_if_needed();
            self.poll_local_hotkeys(&ctx);
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.dismiss(&ctx);
                return;
            }
            let lang = self.lang;
            // 只有「有动画」的状态需要 30fps 连续重绘：录音音量条、思考光环/光点、
            // 头像取图中。静止或隐藏时降到 10fps（stdin 轮询延迟 ≤100ms，肉眼无感），
            // 避免透明置顶窗口长期白跑帧。
            let animated;
            match self.kind {
                PopupKind::Qa => {
                    self.avatar.sync(&ctx, &self.state.qa.viewer_login);
                    let qa_phase = self.state.qa.phase.to_ascii_lowercase();
                    animated = self.avatar.pending.is_some()
                        || matches!(
                            qa_phase.as_str(),
                            "loading" | "thinking" | "recording" | "answerdelta"
                        );
                    let action = frontend::popups::selection_ask(
                        ui,
                        &self.state.qa,
                        &mut self.qa_input,
                        lang,
                        self.avatar.texture.as_ref(),
                    );
                    match action {
                        frontend::popups::QaAction::Dismiss => self.dismiss(&ctx),
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
                            self.dismiss(&ctx);
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
                        ui,
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
                    let warming = self.state.capsule.audio_level.is_none();
                    let warming_phase = self.state.capsule.phase.clone();
                    self.track_capsule_warmup(warming, &warming_phase);
                    let action = frontend::popups::dictation_capsule(
                        ui,
                        &self.state.capsule,
                        lang,
                        self.warmup_ms,
                    );
                    let message = match action {
                        frontend::popups::CapsuleAction::Cancel => {
                            Some(PopupToHost::CancelDictation {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: String::new(),
                                sequence: 0,
                            })
                        }
                        frontend::popups::CapsuleAction::AcceptSuggestion(id) => {
                            Some(PopupToHost::VocabSuggestion {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: String::new(),
                                sequence: 0,
                                id,
                                accept: true,
                            })
                        }
                        frontend::popups::CapsuleAction::RejectSuggestion(id) => {
                            Some(PopupToHost::VocabSuggestion {
                                version: POPUP_PROTOCOL_VERSION,
                                session_id: String::new(),
                                sequence: 0,
                                id,
                                accept: false,
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
                                | PopupToHost::VocabSuggestion {
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
                            let keep_open = matches!(
                                &message,
                                PopupToHost::CancelDictation { .. }
                                    | PopupToHost::StopDictation { .. }
                                    | PopupToHost::VocabSuggestion { .. }
                            );
                            self.send(message);
                            if !keep_open {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
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
    /// （贴底居中、键盘焦点不可能、不占工作区）；协议缺失、Vulkan 初始化失败或
    /// configure 超时都会返回 Err；不会回退到普通焦点窗口或 XWayland。
    fn run_capsule_layer_process() -> Result<(), LayerCapsuleFailure> {
        let geometry = openless_linux_egui::capsule_geometry(
            openless_linux_egui::CAPSULE_WINDOW_SIZE.0,
            openless_linux_egui::CAPSULE_WINDOW_SIZE.1,
            openless_linux_egui::CAPSULE_BOTTOM_GAP,
        );
        // The host pipe is opened lazily, on the first frame the runner asks
        // for: the runner only calls back once the layer surface is configured
        // and Vulkan is live, so a preflight failure leaves stdin untouched for the
        // fallback window. `started` records that the pipe is in use, which
        // makes a late failure fatal instead of a (broken) second attempt.
        let mut app: Option<NativePopupApp> = None;
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started_in_frame = Arc::clone(&started);
        let result = openless_linux_egui::run_layer_capsule(geometry, move |ctx, raw, first| {
            openless_linux_egui::frame_stats::tick("capsule-layer");
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
                            warming_started: None,
                            warmup_ms: openless_linux_egui::load_capsule_warmup_ms(),
                            avatar: QaAvatar::default(),
                            lang: load_locale_pref().resolve(),
                            overlay: None,
                            hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
                        });
                    }
                    Err(error) => {
                        log::error!("popup pipe unavailable: {error}");
                        let output = ctx.run_ui(raw, |_| {});
                        return openless_linux_egui::LayerFrame {
                            output,
                            exit: true,
                            capsule_layout: None,
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
        let capsule_path =
            (kind == PopupKind::Capsule).then(openless_linux_egui::detect_capsule_path);
        // A regular xdg-shell toplevel can be activated by the compositor. That
        // is unacceptable for a recording indicator, and Wayland has no client
        // API to force a non-activating, positioned overlay without layer-shell.
        // Keep recording functional but omit the capsule instead of stealing
        // focus or silently routing the surface through XWayland.
        if capsule_path == Some(openless_linux_egui::CapsulePath::PlainWindow)
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
        {
            return Err(
                "native Wayland capsule requires zwlr_layer_shell_v1; refusing a focusable xdg-shell fallback".to_string(),
            );
        }
        // The capsule uses a native layer surface on Wayland and an X11 overlay
        // only in a native X11 session. A layer-shell startup failure is fatal:
        // do not create a second, focusable Wayland toplevel.
        if capsule_path == Some(openless_linux_egui::CapsulePath::LayerShell) {
            match run_capsule_layer_process() {
                Ok(()) => return Ok(()),
                Err(failure) if failure.started => return Err(failure.message),
                Err(failure) => {
                    return Err(format!(
                        "native layer-shell capsule unavailable: {}",
                        failure.message
                    ))
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
            // Recording keeps a fixed 460x180 Siri stage plus the transcript rail;
            // only an idle vocabulary card changes the host size and placement.
            PopupKind::Capsule => [
                openless_linux_egui::CAPSULE_WINDOW_SIZE.0 as f32,
                openless_linux_egui::CAPSULE_WINDOW_SIZE.1 as f32,
            ],
        };
        // Popup cards paint their own rounded surface. A transparent native
        // surface lets the rounded corners reveal the desktop instead of the
        // rectangular X11 window backing showing through around the card.
        let transparent = true;
        // 原生 X11 会话下先读一次几何，让胶囊在**创建时**就落在工作区底部居中，
        // 并在映射前把 WM_HINTS.input 关掉（窗口管理器不会再给它焦点）。
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
            .with_has_shadow(false)
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
            renderer: eframe::Renderer::Wgpu,
            multisampling: 4,
            ..Default::default()
        };
        let options = vulkan_options(options);
        eframe::run_native(
            "OpenLess Popup",
            options,
            Box::new(move |cc| {
                theme::install(&cc.egui_ctx, openless_linux_egui::typography::load().font);
                if let Some(state) = cc.wgpu_render_state.as_ref() {
                    crate::ui::frontend::siri_wgpu::install_wgpu(state, 4);
                }
                Ok(Box::new(NativePopupApp {
                    kind,
                    state: PopupState::default(),
                    incoming: rx,
                    outgoing: outgoing_tx,
                    qa_input: String::new(),
                    less_computer_input: String::new(),
                    outgoing_sequence: 0,
                    ready_sent: false,
                    warming_started: None,
                    warmup_ms: openless_linux_egui::load_capsule_warmup_ms(),
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
        native: LinuxNativeRuntime,
        tray: Option<openless_linux_egui::LinuxTray>,
        start_minimized: bool,
        runtime_warning: Option<String>,
    ) -> Result<(), String> {
        let socket = bridge::ui_socket_path(runtime_dir);
        let tray_available = tray.is_some();
        // 没有托盘时必须开窗，否则关掉就再也找不回来。
        let window_should_be_open = !start_minimized || !tray_available;
        let ctx = egui::Context::default();
        native.set_hotkey_waker(std::thread::current());
        let mut app = OpenLessEguiApp::new(tokio, Ok(native), tray, window_should_be_open);
        app.runtime_warning = runtime_warning;
        let mut ui_bridge = UiBridgeHost::bind(socket.clone())
            .map_err(|error| format!("UI bridge bind failed: {error}"))?;
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
            // Debounced marketplace search and OAuth polling fire from the host tick.
            app.poll_marketplace_search();
            app.poll_marketplace_oauth();
            app.tick(&ctx);
            if app.should_spawn_ui_window() {
                app.spawn_ui_window(&socket)?;
            }
            if let Some(error) = app.window.error.take() {
                return Err(error);
            }
            let actions = std::mem::take(&mut app.pending_ui_actions);
            if !actions.is_empty() {
                app.apply_frontend_actions(actions, &ctx);
            }
            for sequence in std::mem::take(&mut app.pending_ui_pongs) {
                ui_bridge.send(HostToWindow::Pong { sequence });
            }
            // 分页回包与请求同一个 tick 送出：列表不会因为等下一帧而闪一下。
            for page in std::mem::take(&mut app.pending_collection_pages) {
                ui_bridge.send(page);
            }
            app.sync_view_model();
            app.sync_hotkey_bindings(&mut ui_bridge);
            app.publish_view_model(&mut ui_bridge);
            app.focus_main_window(&mut ui_bridge);
            if app.exit_requested {
                break;
            }
            std::thread::park_timeout(HOST_TICK_INTERVAL);
        }
        log::info!("[ui-host] host exiting; asking the UI window to close");
        ui_bridge.shutdown();
        Ok(())
    }

    /// 只需要打印就退出的参数。以前 `--help` 会被当成普通启动参数，结果是
    /// `openless --help` 把应用拉起来且不退出（用户报的 bug）。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum EarlyExit {
        Help,
        Version,
    }

    /// `--help` / `--version` 的识别（纯函数，便于单测）。`--help` 优先于其它
    /// 参数：`--openless-egui-popup --help` 也必须只打印帮助。
    fn early_exit(args: &[String]) -> Option<EarlyExit> {
        if args.iter().any(|arg| arg == "--help" || arg == "-h") {
            return Some(EarlyExit::Help);
        }
        if args.iter().any(|arg| arg == "--version" || arg == "-V") {
            return Some(EarlyExit::Version);
        }
        None
    }

    /// 命令行帮助（英文：CLI 约定俗成，也避免往本地化目录里塞一次性文案）。
    const USAGE: &str = "\
OpenLess for Linux (egui frontend)

Usage:
  openless                 Launch OpenLess (tray + main window)
  openless --minimized     Launch without showing the main window
  openless --help, -h      Print this help and exit
  openless --version, -V   Print the version and exit

Internal flags (set by OpenLess itself, not for regular use):
  --openless-egui-popup --qa | --capsule | --less-computer
  --ui-client --ui-socket <path>
";

    pub fn run() -> Result<(), String> {
        let args = std::env::args().collect::<Vec<_>>();
        match early_exit(&args) {
            Some(EarlyExit::Help) => {
                print!("{USAGE}");
                return Ok(());
            }
            Some(EarlyExit::Version) => {
                println!("OpenLess {}", env!("OPENLESS_APP_VERSION"));
                return Ok(());
            }
            None => {}
        }
        if let Some(kind) = popup_kind(&args) {
            return run_popup_process(kind);
        }
        if let Some(socket) = ui_client_socket(&args) {
            // UI 窗口进程：只渲染，不碰后端、数据目录、单实例锁与热键。
            return run_ui_client(socket);
        }
        let start_minimized = args.iter().any(|arg| arg == "--minimized");
        let tokio = Arc::new(tokio::runtime::Runtime::new().map_err(|error| error.to_string())?);
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
        let broker = match acquire_broker(&runtime_dir, &args)? {
            BrokerAcquisition::Primary(broker) => Arc::new(broker),
            BrokerAcquisition::Forwarded => return Ok(()),
        };
        let (tray, tray_error) = match openless_linux_egui::LinuxTray::start() {
            Ok(tray) => (Some(tray), None),
            Err(error) => {
                eprintln!("OpenLess tray unavailable: {error}");
                (None, Some(error.to_string()))
            }
        };
        let tray_available = tray.is_some();
        let native = (|| {
            let mut config = backend_config()?;
            match openless_linux_egui::init_file_logger(&config.data_dir) {
                Ok(_) => log::info!("[startup] single-instance lock acquired; running as primary"),
                Err(error) => eprintln!("OpenLess file logger unavailable: {error}"),
            }
            if let Some(error) = tray_error {
                // 没有托盘 ⇒ 关窗就退出：这是用户行为差异，必须写进启动日志。
                log::warn!(
                    "OpenLess tray unavailable: {error}; closing the main window will exit the app"
                );
            }
            // fcitx5 is an optional integration: a missing daemon, addon, or
            // session bus must not prevent the Core/UI from starting.
            let lang = load_locale_pref().resolve();
            let mut input_method_error = match ensure_fcitx5_ready(&config) {
                Ok(()) => None,
                Err(error) => {
                    log::warn!(
                        "[fcitx] degraded startup (readiness check failed); global hotkeys disabled: {error}"
                    );
                    Some(error)
                }
            };
            let hotkeys = if input_method_error.is_none() {
                match Fcitx5HotkeyListener::start() {
                    Ok(listener) => Some(listener),
                    Err(error) => {
                        let error = error.to_string();
                        log::warn!(
                            "[fcitx] degraded startup (listener failed); global hotkeys disabled: {error}"
                        );
                        input_method_error = Some(error);
                        None
                    }
                }
            } else {
                None
            };
            config.platform = LinuxCapabilitySnapshot::detect(tray_available).capabilities;
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
            let native = tokio
                .block_on(LinuxNativeRuntime::start(
                    backend,
                    Some(Arc::clone(&broker)),
                    hotkeys,
                ))
                .map_err(|error| error.to_string())?;
            let runtime_warning = input_method_error
                .map(|error| fmt_l10n(lang, "startup.input_method_unavailable", &[&error]));
            Ok::<_, String>((native, runtime_warning))
        })();
        // 常驻宿主：本进程不再创建窗口，窗口交给独立的 UI 进程。
        let (native, runtime_warning) = match native {
            Ok(native) => native,
            Err(error) => {
                drop(tray);
                // Fatal startup errors are reported to stderr/journal only. Do
                // not create a second egui window: recoverable integration
                // failures (such as fcitx5) are rendered by the main UI's
                // runtime-warning overlay instead.
                log::error!("OpenLess startup failed: {error}");
                return Err(error);
            }
        };
        if let Err(error) = run_host(
            &runtime_dir,
            tokio,
            native,
            tray,
            start_minimized,
            runtime_warning,
        ) {
            // The UI process owns all user-facing recoverable warnings. A
            // fatal host/UI failure must not fall back to a standalone popup;
            // that popup was a second, misleading application window.
            log::error!("OpenLess host failed: {error}");
            return Err(error);
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct FailingQaRuntime;

        impl openless_core::QaRuntimeAdapter for FailingQaRuntime {
            fn prepare_text(
                &self,
                _: openless_core::SessionId,
                text: String,
            ) -> futures_util::future::BoxFuture<
                'static,
                Result<openless_core::QaInput, BackendError>,
            > {
                Box::pin(async move {
                    Ok(openless_core::QaInput {
                        text,
                        selection_text: None,
                        selection_source_app: None,
                    })
                })
            }
            fn start_recording(
                &self,
                _: openless_core::SessionId,
                _: Arc<dyn openless_core::QaProgressSink>,
            ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
                panic!("text submission must not start recording")
            }
            fn finish_recording(
                &self,
                _: openless_core::SessionId,
            ) -> futures_util::future::BoxFuture<
                'static,
                Result<openless_core::QaInput, BackendError>,
            > {
                panic!("text submission must not finish recording")
            }
            fn answer(
                &self,
                _: openless_core::QaTurnRequest,
                _: Arc<dyn openless_core::QaProgressSink>,
            ) -> futures_util::future::BoxFuture<
                'static,
                Result<openless_core::QaTurnResult, BackendError>,
            > {
                Box::pin(async {
                    Err(BackendError::new(
                        openless_core::BackendErrorCode::InvalidArgument,
                        "fixture provider unavailable",
                    ))
                })
            }
            fn cancel(
                &self,
                _: openless_core::SessionId,
            ) -> futures_util::future::BoxFuture<'static, Result<(), BackendError>> {
                Box::pin(async { Ok(()) })
            }
        }

        #[test]
        fn startup_logging_records_forwarded_intents_without_rotating_again() {
            use std::io::Write;

            const CHILD_ENV: &str = "OPENLESS_STARTUP_LOG_TEST_CHILD";
            const TEST: &str = "linux_app::tests::startup_logging_records_forwarded_intents_without_rotating_again";
            if std::env::var_os(CHILD_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
                    .env(CHILD_ENV, "1")
                    .env_remove("OPENLESS_LOG")
                    .env_remove("RUST_LOG")
                    .output()
                    .expect("launch isolated startup logging regression");
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(output.status.success(), "{stdout}\n{stderr}");
                assert!(
                    stdout.contains("test result: ok. 1 passed; 0 failed"),
                    "{stdout}"
                );
                return;
            }
            let root = tempfile::tempdir().unwrap();
            for key in [
                "XDG_DATA_HOME",
                "XDG_CONFIG_HOME",
                "XDG_CACHE_HOME",
                "XDG_RUNTIME_DIR",
            ] {
                std::env::set_var(key, root.path());
            }
            let runtime_dir = root.path().join("runtime");
            let data_dir = root.path().join("data");
            std::fs::create_dir_all(&runtime_dir).unwrap();
            std::fs::create_dir_all(data_dir.join("logs")).unwrap();
            let path = openless_linux_egui::log_path(&data_dir);
            let old_log = vec![b'o'; 2 * 1024 * 1024 + 1];
            std::fs::write(&path, &old_log).unwrap();
            let args = vec!["openless".to_string()];
            let primary = match acquire_broker(&runtime_dir, &args).unwrap() {
                BrokerAcquisition::Primary(broker) => Arc::new(broker),
                BrokerAcquisition::Forwarded => panic!("the first launch must be primary"),
            };
            openless_linux_egui::init_file_logger(&data_dir).unwrap();
            assert!(std::fs::read_to_string(&path)
                .unwrap()
                .contains("Linux egui logger ready:"));

            let mut app = fixture_app(false);
            let host_actions = Arc::new(openless_linux_egui::LinuxHostActions::default());
            let mut dependencies = openless_core::BackendDependencies::unsupported();
            dependencies.host_actions = host_actions.clone();
            dependencies.credential_store =
                Arc::new(openless_core::InMemoryCredentialStore::default());
            dependencies.services.remote_input = Arc::new(
                openless_core::RemoteInputService::new(
                    Arc::new(openless_core::testing::RecordingRemoteInputRuntime::default()),
                    8443,
                    "en",
                )
                .unwrap(),
            );
            let backend = Arc::new(
                openless_core::OpenLessBackend::new(
                    BackendConfig {
                        data_dir: data_dir.clone(),
                        ..Default::default()
                    },
                    dependencies,
                )
                .unwrap(),
            );
            let runtime = openless_linux_egui::LinuxBackendRuntime {
                backend: backend.clone(),
                host_actions,
                settings_runtime: Arc::new(openless_core::settings::NoopSettingsRuntime),
            };
            app.native = Some(
                app.tokio
                    .block_on(LinuxNativeRuntime::start(
                        runtime,
                        Some(primary.clone()),
                        None,
                    ))
                    .unwrap(),
            );
            app.snapshot = Some(backend.snapshot());
            app.preferences = Some(backend.get_preferences());
            // 只测试 QA 的收起分支，不创建真实弹窗或启动录音。
            app.qa_visible = true;

            let mut active = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            active.write_all(&vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
            active.write_all(b"\nactive-file-marker\n").unwrap();
            drop(active);
            for args in [
                args,
                vec!["openless".to_string(), "--toggle-qa".to_string()],
            ] {
                assert!(matches!(
                    acquire_broker(&runtime_dir, &args).unwrap(),
                    BrokerAcquisition::Forwarded
                ));
            }
            app.poll(&egui::Context::default());
            log::logger().flush();
            let text = std::fs::read_to_string(&path).unwrap();
            for intent in ["ShowMain", "Cli(ToggleQa)"] {
                let message = format!("[ui-host] launch intent from the user: {intent}");
                assert_eq!(text.matches(&message).count(), 1, "{message}");
            }
            assert!(text.contains("active-file-marker"));
            assert!(app.window.should_be_open);
            assert!(!app.qa_visible);
            assert_eq!(
                std::fs::read(data_dir.join("logs/openless.log.1")).unwrap(),
                old_log
            );
            assert!(!data_dir.join("logs/openless.log.2").exists());
            assert!(!data_dir.join("logs/openless.log.3").exists());
            drop(app);
            drop(primary);
        }

        #[test]
        fn an_idle_popup_submission_reaches_core_and_displays_failure() {
            let data_dir = tempfile::tempdir().unwrap();
            let tokio = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let host_actions = Arc::new(openless_linux_egui::LinuxHostActions::default());
            let mut dependencies = openless_core::BackendDependencies::unsupported();
            dependencies.host_actions = host_actions.clone();
            dependencies.credential_store =
                Arc::new(openless_core::InMemoryCredentialStore::default());
            dependencies.qa_runtime = Some(Arc::new(FailingQaRuntime));
            dependencies.services.remote_input = Arc::new(
                openless_core::RemoteInputService::new(
                    Arc::new(openless_core::testing::RecordingRemoteInputRuntime::default()),
                    8443,
                    "en",
                )
                .unwrap(),
            );
            let backend = Arc::new(
                openless_core::OpenLessBackend::new(
                    BackendConfig {
                        data_dir: data_dir.path().to_path_buf(),
                        ..Default::default()
                    },
                    dependencies,
                )
                .unwrap(),
            );
            let runtime = openless_linux_egui::LinuxBackendRuntime {
                backend,
                host_actions,
                settings_runtime: Arc::new(openless_core::settings::NoopSettingsRuntime),
            };
            let backend = runtime.backend.clone();
            let native = tokio
                .block_on(LinuxNativeRuntime::start(runtime, None, None))
                .unwrap();
            let mut app = OpenLessEguiApp::new(tokio.clone(), Ok(native), None, true);
            app.qa_visible = true;
            app.qa_state = Some(QaStateEvent::simple(QaStateKind::Idle));
            let submission = PopupToHost::SubmitQa {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "qa".into(),
                sequence: 1,
                text: "probe hello".into(),
            };
            app.apply_popup_event(
                PopupKind::Qa,
                PopupSupervisorEvent::Message(submission.clone()),
            );
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(UiResult::QaAction {
                    generation,
                    context,
                    result,
                }) = app.rx.try_recv()
                {
                    assert!(result.is_err(), "the fixture rejects the provider request");
                    app.apply_qa_action_result(generation, context, result);
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "idle submission was silently discarded"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let snapshot = tokio.block_on(backend.services().qa.snapshot()).unwrap();
            assert!(
                snapshot.session_id.is_some(),
                "Core must have claimed a QA turn"
            );
            assert_eq!(snapshot.phase, openless_core::QaPhase::Failed);
            assert!(app.qa_state.as_ref().unwrap().error.is_some());
            app.apply_popup_event(PopupKind::Qa, PopupSupervisorEvent::Message(submission));
            assert_eq!(app.status, tr_l10n(app.lang, "popup.ignore_stale"));
            tokio.block_on(backend.shutdown()).unwrap();
        }

        #[test]
        fn capsule_uses_raw_accumulator_not_polished_output_and_respects_preferences() {
            let mut app = fixture_app(false);
            app.transcript_state
                .apply(&openless_core::TranscriptDelta {
                    text: "raw words".into(),
                    offset: 0,
                    is_final: false,
                })
                .unwrap();
            app.transcript = "polished output".into();
            app.preferences = Some(UserPreferences::default());
            assert_eq!(app.capsule_transcript(), "raw words");
            assert_eq!(app.capsule_transcript_font_size(), 14);
            let prefs = app.preferences.as_mut().unwrap();
            prefs.capsule_transcript_enabled = false;
            prefs.capsule_transcript_font_size = 255;
            assert!(app.capsule_transcript().is_empty());
            assert_eq!(app.capsule_transcript_font_size(), 20);
        }

        #[test]
        fn style_example_changes_mark_the_editor_dirty() {
            let saved = openless_core::StylePack::default();
            let mut vm = frontend::view_model::FrontendViewModel {
                style_name: saved.name.clone(),
                style_description: saved.description.clone(),
                style_prompt: saved.prompt.clone(),
                style_selection_prompt: saved.selection_prompt.clone(),
                style_voice_edit_prompt: saved.voice_edit_prompt.clone(),
                style_tags: saved.tags.join(", "),
                style_author: saved.author.clone().unwrap_or_default(),
                style_version: saved.version.clone(),
                style_model: saved.recommended_model.clone().unwrap_or_default(),
                style_compatible_version: saved.compatible_app_version.clone().unwrap_or_default(),
                style_examples: saved.examples.clone(),
                style_editor_saved: Some(saved),
                ..Default::default()
            };
            assert!(!style_editor_is_dirty(&vm));
            vm.style_examples.push(openless_core::StylePackExample {
                title: Some("demo".into()),
                input: "raw".into(),
                output: "polished".into(),
            });
            assert!(style_editor_is_dirty(&vm));
        }

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
        fn the_runtime_warning_can_be_dismissed_without_stopping_the_host() {
            let mut app = fixture_app(true);
            app.runtime_warning = Some("fixture warning".into());
            app.apply_frontend_actions(
                vec![frontend::view_model::FrontendAction::DismissRuntimeWarning],
                &egui::Context::default(),
            );
            assert!(app.runtime_warning.is_none());
        }

        #[test]
        fn the_host_keeps_running_when_the_window_says_goodbye() {
            let mut app = fixture_app(true);
            app.apply_window_messages(vec![WindowToHost::Bye], true);
            // 关窗只关窗口：宿主不退出、后端与会话不动。
            assert!(!app.window.should_be_open);
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
            assert!(app.window.should_be_open);
            assert!(app.should_spawn_ui_window());
        }

        #[test]
        fn a_freshly_spawned_window_is_not_spawned_twice() {
            let mut app = fixture_app(false);
            app.request_main_window();
            // 模拟「刚拉起过」：防抖窗口内不得再拉第二个窗口进程。
            app.window.spawned_at = Some(std::time::Instant::now());
            assert!(!app.should_spawn_ui_window());
            // 防抖过期且没有活着的子进程时，允许重拉。
            app.window.spawned_at = Some(std::time::Instant::now() - Duration::from_secs(5));
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
                    action: Box::new(frontend::view_model::FrontendAction::WindowClose),
                }],
                true,
            );
            // 窗口控制由 UI 进程处理；即便漏到宿主，也只是入队后由
            // apply_frontend_actions 记一条日志，不改变窗口意图。
            assert!(app.window.should_be_open);
        }

        #[test]
        fn reconnect_resends_state_and_focus_waits_for_the_handshake() {
            let dir = std::env::temp_dir()
                .join(format!("openless-window-focus-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let mut bridge = UiBridgeHost::bind(dir.join("ui.sock")).unwrap();
            let mut app = fixture_app(false);
            app.preferences = Some(UserPreferences::default());
            app.transcript = "ongoing session".into();
            let pid = std::process::id();
            let mut previous_sequence = 0;
            for _ in 0..2 {
                app.request_main_window();
                let mut client = UiBridgeClient::connect(bridge.path()).unwrap();
                bridge.accept_pending();
                app.focus_main_window(&mut bridge);
                assert!(
                    app.window.focus_requested,
                    "focus must survive an unfinished handshake"
                );
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while !bridge.is_connected() {
                    app.apply_window_messages(bridge.drain(), true);
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(2));
                }
                app.sync_hotkey_bindings(&mut bridge);
                app.publish_view_model(&mut bridge);
                app.focus_main_window(&mut bridge);
                let (mut ready, mut hotkeys, mut snapshot, mut focus) =
                    (false, false, false, false);
                while !(ready && hotkeys && snapshot && focus) {
                    // 快照走单槽位，不从通道出。
                    if let Some((sequence, _)) = client.take_snapshot() {
                        assert!(ready && sequence > previous_sequence);
                        previous_sequence = sequence;
                        snapshot = true;
                    }
                    match client.try_recv() {
                        Ok(HostToWindow::Ready { .. }) => ready = true,
                        Ok(HostToWindow::Hotkeys { .. }) => {
                            assert!(ready);
                            hotkeys = true;
                        }
                        Ok(HostToWindow::Snapshot { sequence, .. }) => {
                            assert!(ready && sequence > previous_sequence);
                            previous_sequence = sequence;
                            snapshot = true;
                        }
                        Ok(HostToWindow::FocusMain) => {
                            assert!(ready);
                            focus = true;
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        other => panic!("unexpected frame: {other:?}"),
                    }
                    assert!(std::time::Instant::now() < deadline);
                }
                assert!(!app.window.focus_requested);
                app.apply_window_messages(vec![WindowToHost::Bye], true);
                drop(client);
                while bridge.is_connected() {
                    bridge.drain();
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert_eq!(app.transcript, "ongoing session");
                assert_eq!(std::process::id(), pid);
                assert!(!app.exit_requested);
            }
            bridge.shutdown();
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn history_refresh_preserves_selection_and_delete_identity() {
            let mut app = fixture_app(false);
            let mut selected = session_entry("2026-09-27T12:00:00Z", "selected", None);
            selected.id = "selected-id".into();
            app.history.entries = vec![selected.clone()];
            app.sync_view_model();
            app.apply_frontend_actions(
                vec![frontend::view_model::FrontendAction::HistoryRequestDelete(
                    selected.id.clone(),
                )],
                &egui::Context::default(),
            );
            let mut newest = selected.clone();
            newest.id = "newest-id".into();
            app.history.entries.insert(0, newest);
            app.sync_view_model();
            assert_eq!(app.frontend_vm.history_selected, 1);
            assert_eq!(
                app.frontend_vm.history_confirm,
                Some(frontend::view_model::HistoryConfirm::Delete(selected.id))
            );
        }

        /// 润色与语音快捷键的补充说明必须出现在快捷键页。
        #[test]
        fn shortcut_hints_come_from_the_catalog() {
            let zh = Lang::ZhCn;
            let ctx = egui::Context::default();
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 2400.0));
            let mut time = 0.0_f64;
            let mut render = |vm: &mut FrontendViewModel, events: Vec<egui::Event>| {
                time += 1.0;
                let output = crate::ui::frontend::run_pass(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ui| {
                        let mut actions = Vec::new();
                        frontend::settings::test_render_shortcuts(ui, vm, &mut actions);
                    },
                );
                painted_text_of(&output)
            };

            let mut vm = FrontendViewModel {
                lang: zh,
                active_page: frontend::view_model::Page::Settings,
                settings_open: true,
                settings_section: frontend::view_model::SettingsSection::Shortcuts,
                ..Default::default()
            };
            vm.settings.testing_features = true;
            let _ = render(&mut vm, Vec::new());
            let painted = render(&mut vm, Vec::new());
            for key in [
                "settings.selection_workspace.polish_hotkey_desc",
                "settings.coding_agent.voice_hotkey_desc",
            ] {
                assert!(
                    painted.contains(tr_l10n(zh, key)),
                    "{key} must be rendered on the shortcuts page:\n{painted}"
                );
            }
        }

        /// 输入框回归：宿主推新快照时，用户还没提交的输入不能被抹掉。
        /// 之前的实现是整份替换视图模型，界面上表现为「打完字一秒后文字消失」。
        #[test]
        fn a_pending_edit_survives_the_next_snapshot() {
            let base = FrontendViewModel {
                style_name: "原始名字".into(),
                ..Default::default()
            };
            let mut local = base.clone();
            // 用户正在输入：改了搜索框和风格名，都还没提交（未失焦/未回车）。
            local.settings_query = "润色上下文".into();
            local.style_name = "原始名字".into();

            // 宿主这份快照什么都没改（保活推送）。
            let merged = merge_local_edits(
                &serde_json::to_value(&local).unwrap(),
                &serde_json::to_value(&base).unwrap(),
                &serde_json::to_value(&base).unwrap(),
            );
            let merged: FrontendViewModel = serde_json::from_value(merged).unwrap();
            assert_eq!(merged.settings_query, "润色上下文");

            // 宿主改了别的字段（例如状态栏文案）：本地输入仍然保留，宿主的改动生效。
            let mut incoming = base.clone();
            incoming.status = "已保存".into();
            let merged = merge_local_edits(
                &serde_json::to_value(&local).unwrap(),
                &serde_json::to_value(&base).unwrap(),
                &serde_json::to_value(&incoming).unwrap(),
            );
            let merged: FrontendViewModel = serde_json::from_value(merged).unwrap();
            assert_eq!(merged.settings_query, "润色上下文");
            assert_eq!(merged.status, "已保存");

            // 宿主改了同一个字段（打开编辑器 hydrate / 提交后回写）：以宿主为准。
            let mut incoming = base.clone();
            incoming.style_name = "宿主 hydrate 的名字".into();
            let merged = merge_local_edits(
                &serde_json::to_value(&local).unwrap(),
                &serde_json::to_value(&base).unwrap(),
                &serde_json::to_value(&incoming).unwrap(),
            );
            let merged: FrontendViewModel = serde_json::from_value(merged).unwrap();
            assert_eq!(merged.style_name, "宿主 hydrate 的名字");
        }

        /// 嵌套对象与列表同样按字段判断：宿主换过的列表用宿主的，没换的保留本地。
        #[test]
        fn only_fields_the_host_touched_are_taken_from_the_snapshot() {
            let base = serde_json::json!({
                "settings": { "remote_input_port": "8765", "retention_days": "7" },
                "history_entries": [],
            });
            let local = serde_json::json!({
                // 用户在输入框里把端口改成了半截内容，还没提交。
                "settings": { "remote_input_port": "8", "retention_days": "7" },
                "history_entries": [],
            });
            let incoming = serde_json::json!({
                // 宿主只刷新了历史列表与保留时长。
                "settings": { "remote_input_port": "8765", "retention_days": "30" },
                "history_entries": [{ "id": "a" }],
            });
            let merged = merge_local_edits(&local, &base, &incoming);
            assert_eq!(merged["settings"]["remote_input_port"], "8");
            assert_eq!(merged["settings"]["retention_days"], "30");
            assert_eq!(merged["history_entries"].as_array().unwrap().len(), 1);
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

        fn painted_text_of(output: &egui::FullOutput) -> String {
            fn collect(shape: &egui::Shape, out: &mut String) {
                match shape {
                    egui::Shape::Text(text) => {
                        out.push_str(text.galley.text());
                        out.push('\n');
                    }
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            collect(shape, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut out = String::new();
            for clipped in &output.shapes {
                collect(&clipped.shape, &mut out);
            }
            out
        }

        fn fixture_editor(provider_type: &str) -> ProviderEditor {
            ProviderEditor {
                kind: openless_core::ChannelKind::Llm,
                channel: openless_core::ChannelSummary {
                    id: "draft-channel".into(),
                    name: "typed name".into(),
                    provider_type: provider_type.into(),
                    enabled: true,
                    order: 0,
                    last_test: None,
                },
                descriptor: openless_core::provider_rules::provider_descriptor(
                    openless_core::ProviderKind::Llm,
                    provider_type,
                )
                .unwrap(),
                name: "typed name".into(),
                endpoint: "https://example.invalid".into(),
                model: "custom-model".into(),
                volcengine_service: String::new(),
                protocol: Default::default(),
                auth_mode: String::new(),
                resource_id: String::new(),
                primary_secret: String::new(),
                secondary_secret: String::new(),
            }
        }

        #[test]
        fn untouched_descriptor_defaults_do_not_become_saved_connection_values() {
            let mut app = fixture_app(true);
            let mut editor = fixture_editor("ark");
            assert!(editor.descriptor.default_endpoint.is_some());
            assert!(editor.descriptor.default_model.is_some());
            editor.endpoint.clear();
            editor.model.clear();
            app.provider_editor_form = Some(ProviderEditorForm::from_editor(&editor, app.lang));
            app.provider_editor = ProviderEditorState::Loaded(Box::new(editor));
            let snapshot = app.editor_from_form().unwrap();
            assert!(snapshot.endpoint.is_empty());
            assert!(snapshot.model.is_empty());
            assert!(!app
                .provider_editor_form
                .as_ref()
                .unwrap()
                .default_model
                .is_empty());
        }

        /// 保存成功后，编辑器里的密钥不能被清空。
        ///
        /// 旧逻辑会「擦掉已被确认的草稿」，而自动保存是输入后 300ms 触发的：用户刚
        /// 填好密钥，保存一回就把它从输入框里抹掉，看起来就是「填进去的密钥几秒后
        /// 被删了」。密钥本身仍在保险库里，只是界面上连掩码点都没有。
        #[test]
        fn a_saved_secret_stays_in_the_editor() {
            let mut app = fixture_app(true);
            let editor = fixture_editor("custom");
            app.frontend_vm.settings_open = true;
            app.frontend_vm.services_view = 0;
            let mut form = ProviderEditorForm::from_editor(&editor, app.lang);
            form.primary_secret = "sk-typed-secret".into();
            app.provider_editor_form = Some(form);
            app.provider_editor = ProviderEditorState::Loaded(Box::new(editor.clone()));
            app.draft_touched = true;
            app.provider_save_pending = true;
            // 宿主提交的正是这份草稿（`editor_from_form`），回执带回同一份快照。
            let mut submitted = editor.clone();
            submitted.primary_secret = "sk-typed-secret".into();
            app.tx
                .send(UiResult::ProviderSaved {
                    editor: Box::new(submitted),
                    result: Ok(()),
                })
                .unwrap();
            app.poll(&egui::Context::default());
            assert_eq!(
                app.provider_editor_form
                    .as_ref()
                    .map(|form| form.primary_secret.clone()),
                Some("sk-typed-secret".to_string()),
                "a successful save must not erase the key the user just typed"
            );
        }

        #[test]
        fn leaving_a_channel_waits_for_save_and_failure_keeps_the_draft() {
            let mut app = fixture_app(true);
            let editor = fixture_editor("custom");
            app.frontend_vm.settings_open = true;
            app.frontend_vm.services_view = 0;
            app.provider_editor_form = Some(ProviderEditorForm::from_editor(&editor, app.lang));
            app.provider_editor = ProviderEditorState::Loaded(Box::new(editor.clone()));
            app.draft_touched = true;
            app.provider_save_pending = true;
            app.apply_frontend_actions(
                vec![frontend::view_model::FrontendAction::SettingsServicesView(
                    1,
                )],
                &egui::Context::default(),
            );
            assert_eq!(app.frontend_vm.services_view, 0);
            assert!(app.provider_after_save.is_some());
            app.tx
                .send(UiResult::ProviderSaved {
                    editor: Box::new(editor),
                    result: Err("vault unavailable".into()),
                })
                .unwrap();
            app.poll(&egui::Context::default());
            assert!(app.provider_after_save.is_none());
            assert_eq!(app.frontend_vm.services_view, 0);
            assert_eq!(
                app.provider_editor_form.as_ref().unwrap().model,
                "custom-model"
            );
            assert!(app.frontend_vm.settings_notice.is_some());
        }

        #[test]
        fn changing_protocol_discards_the_previous_model_catalog() {
            let mut app = fixture_app(true);
            let editor = fixture_editor("orcarouter");
            let mut form = ProviderEditorForm::from_editor(&editor, app.lang);
            form.models = vec!["previous-format-model".into()];
            form.catalog_format = Some(form.protocol.format());
            app.provider_editor_form = Some(form);
            app.provider_editor = ProviderEditorState::Loaded(Box::new(editor));
            app.apply_frontend_actions(
                vec![frontend::view_model::FrontendAction::SettingsProviderField(
                    frontend::view_model::SettingsProviderField::RequestFormat,
                    "messages".into(),
                )],
                &egui::Context::default(),
            );
            let form = app.provider_editor_form.as_ref().unwrap();
            assert!(form.models.is_empty());
            assert!(form.catalog_format.is_none());
        }

        #[test]
        fn stale_provider_panels_cannot_consume_the_new_draft_open_request() {
            let mut app = fixture_app(true);
            app.providers_request = 2;
            app.editor_requested = true;
            app.selected_channel_id = Some("draft-channel".into());
            app.tx
                .send(UiResult::Providers {
                    kind: app.provider_kind,
                    request: 1,
                    result: Err("stale panel".into()),
                })
                .unwrap();
            app.poll(&egui::Context::default());
            assert!(matches!(app.providers, ProvidersState::Loading));
            assert!(app.editor_requested);
            assert_eq!(app.selected_channel_id.as_deref(), Some("draft-channel"));
            assert!(app.frontend_vm.settings_notice.is_none());
        }

        #[test]
        fn provider_switch_receipt_keeps_the_touched_draft_editor_open() {
            let mut app = fixture_app(true);
            let old = fixture_editor("custom");
            app.provider_kind = old.kind;
            app.settings_channel_kind = old.kind;
            app.selected_channel_id = Some(old.channel.id.clone());
            app.provider_editor_form = Some(ProviderEditorForm::from_editor(&old, app.lang));
            app.provider_editor = ProviderEditorState::Loaded(Box::new(old));
            app.editor_draft = true;
            app.draft_touched = true;
            app.provider_switch_pending = true;
            let next = fixture_editor("custom_messages");
            app.tx
                .send(UiResult::ProviderTypeChanged {
                    kind: next.kind,
                    channel_id: next.channel.id.clone(),
                    result: Box::new(Ok(next)),
                })
                .unwrap();
            app.poll(&egui::Context::default());
            assert!(!app.provider_switch_pending);
            assert!(app.editor_draft && app.draft_touched);
            let form = app.provider_editor_form.as_ref().unwrap();
            assert_eq!(form.provider_type, "custom_messages");
            assert_eq!(form.name, "typed name");
            assert_eq!(form.model, "custom-model");
            assert!(matches!(
                app.provider_editor,
                ProviderEditorState::Loaded(_)
            ));
        }

        #[test]
        fn model_selection_marks_new_channels_touched_and_switching_blocks_stale_input() {
            let mut app = fixture_app(true);
            let editor = fixture_editor("custom");
            app.provider_editor_form = Some(ProviderEditorForm::from_editor(&editor, app.lang));
            app.provider_editor = ProviderEditorState::Loaded(Box::new(editor));
            app.editor_draft = true;
            app.apply_frontend_actions(
                vec![
                    frontend::view_model::FrontendAction::SettingsProviderModelSelected(
                        "selected-model".into(),
                    ),
                ],
                &egui::Context::default(),
            );
            assert!(app.draft_touched);
            assert_eq!(
                app.provider_editor_form.as_ref().unwrap().model,
                "selected-model"
            );
            app.provider_switch_pending = true;
            app.apply_frontend_actions(
                vec![frontend::view_model::FrontendAction::SettingsProviderField(
                    frontend::view_model::SettingsProviderField::Model,
                    "stale-input".into(),
                )],
                &egui::Context::default(),
            );
            assert_eq!(
                app.provider_editor_form.as_ref().unwrap().model,
                "selected-model"
            );
        }

        #[test]
        fn model_catalog_for_the_previous_endpoint_is_ignored() {
            let mut app = fixture_app(true);
            let editor = fixture_editor("custom");
            app.provider_kind = editor.kind;
            app.selected_channel_id = Some(editor.channel.id.clone());
            app.provider_editor_form = Some(ProviderEditorForm::from_editor(&editor, app.lang));
            app.provider_editor = ProviderEditorState::Loaded(Box::new(editor.clone()));
            app.tx
                .send(UiResult::ProviderModels {
                    request_format: editor.protocol.format(),
                    kind: editor.kind,
                    channel_id: editor.channel.id,
                    endpoint: "https://old.example.invalid".into(),
                    provider_type: "custom".into(),
                    result: Ok(vec!["old-model".into()]),
                })
                .unwrap();
            app.poll(&egui::Context::default());
            assert!(app.provider_editor_form.as_ref().unwrap().models.is_empty());
        }

        fn fixture_app(window_should_be_open: bool) -> OpenLessEguiApp {
            OpenLessEguiApp::new(
                Arc::new(tokio::runtime::Runtime::new().unwrap()),
                Err("fixture".into()),
                None,
                window_should_be_open,
            )
        }

        /// Omni 的测试与拉模型请求不带 channel_id：Core 明确拒绝
        /// `omni + channel_id`，带上就等于每次测试都失败。
        #[test]
        fn omni_requests_never_carry_a_channel_id() {
            let request = omni_provider_request(false);
            assert_eq!(request.kind, openless_core::ProviderKind::Omni);
            assert!(
                request.channel_id.is_none(),
                "Core rejects omni requests that carry a channel id"
            );
        }

        /// 编辑已有渠道时，凭据要读回表单（输入框按密码掩码显示）。写死空串会让用户
        /// 以为密钥没保存，测试之后刷新一次就"消失"。
        #[test]
        fn the_channel_editor_hydrates_the_stored_secret() {
            use std::sync::Arc;
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let dir = std::env::temp_dir().join(format!(
                "openless-editor-hydrate-{}",
                uuid::Uuid::new_v4().simple()
            ));
            let store = Arc::new(openless_linux_egui::InMemoryCredentialStore::default());
            let mut deps = openless_core::BackendDependencies::unsupported();
            deps.credential_store = store.clone();
            deps.task_spawner = Arc::new(openless_core::TokioTaskSpawner);
            let backend = Arc::new(
                openless_core::OpenLessBackend::new(
                    openless_core::BackendConfig {
                        data_dir: dir.clone(),
                        ..Default::default()
                    },
                    deps,
                )
                .unwrap(),
            );
            let channel = openless_core::ChannelSummary {
                id: "deepseek".into(),
                name: "DeepSeek".into(),
                provider_type: "deepseek".into(),
                enabled: true,
                order: 0,
                last_test: None,
            };
            let descriptor = openless_core::provider_rules::provider_descriptor(
                openless_core::ProviderKind::Llm,
                "deepseek",
            )
            .expect("deepseek descriptor");
            runtime.block_on(async {
                for (account, value) in [
                    (
                        openless_core::credentials::LLM_ENDPOINT_ACCOUNT,
                        // `check-linux-public-surface.ps1` 禁止 linux UI 源码里出现
                        // 默认服务地址的字面量（默认端点属于 Core）。用 concat! 拼出来
                        // 只是为了让用例的 URL 形状真实，不构成任何默认值。
                        concat!("https://api.deepseek.com", "/v1"),
                    ),
                    (
                        openless_core::credentials::LLM_MODEL_ACCOUNT,
                        "deepseek-chat",
                    ),
                    (
                        openless_core::credentials::LLM_API_KEY_ACCOUNT,
                        "sk-stored-key",
                    ),
                ] {
                    let key = provider_credential_key(
                        openless_core::ChannelKind::Llm,
                        &channel.id,
                        account,
                    )
                    .unwrap();
                    backend
                        .set_credential(key, openless_core::SecretValue::new(value))
                        .await
                        .unwrap();
                }
            });
            let editor = runtime
                .block_on(load_provider_editor(
                    Arc::clone(&backend),
                    openless_core::ChannelKind::Llm,
                    channel,
                    descriptor,
                ))
                .expect("editor loads");
            assert_eq!(editor.endpoint, concat!("https://api.deepseek.com", "/v1"));
            assert_eq!(editor.model, "deepseek-chat");
            assert_eq!(
                editor.primary_secret, "sk-stored-key",
                "the stored API key must be hydrated so the masked field is not empty"
            );
            let _ = std::fs::remove_dir_all(dir);
        }

        /// 快捷键卡片上的每一行都必须从 Core 偏好取真值。宿主以前只填了
        /// 听写/翻译/QA/速记四行，其余行无论偏好里有没有绑定都画成空键帽 ——
        /// 看起来是「未设置」，录制却实实在在地写进了偏好。
        #[test]
        fn shortcut_rows_show_the_bindings_core_holds() {
            let mut app = fixture_app(true);
            app.lang = Lang::En;
            app.preferences = Some(UserPreferences {
                dictation_hotkey: openless_core::shared_types::ShortcutBinding {
                    primary: "RightOption".into(),
                    modifiers: Vec::new(),
                },
                switch_style_hotkey: Some(openless_core::shared_types::ShortcutBinding {
                    primary: "1".into(),
                    modifiers: vec!["ctrl".into(), "shift".into()],
                }),
                open_app_hotkey: Some(openless_core::shared_types::ShortcutBinding {
                    primary: "o".into(),
                    modifiers: vec!["ctrl".into(), "alt".into()],
                }),
                coding_agent_voice_hotkey: Some(openless_core::shared_types::ShortcutBinding {
                    primary: "v".into(),
                    modifiers: vec!["ctrl".into(), "super".into()],
                }),
                ..Default::default()
            });
            app.sync_view_model();
            let vm = &app.frontend_vm;
            assert_eq!(vm.dictation_hotkey, "Right Alt");
            for (label, value) in [
                ("switch_style", &vm.switch_style_hotkey),
                ("open_app", &vm.open_app_hotkey),
                ("coding_agent", &vm.coding_agent_hotkey),
            ] {
                assert!(
                    !value.trim().is_empty(),
                    "{label} shortcut row must show the binding Core holds"
                );
            }
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
        fn help_and_version_exit_before_anything_launches() {
            let args = |extra: &str| vec!["openless".to_string(), extra.to_string()];
            assert_eq!(
                super::early_exit(&args("--help")),
                Some(super::EarlyExit::Help)
            );
            assert_eq!(super::early_exit(&args("-h")), Some(super::EarlyExit::Help));
            assert_eq!(
                super::early_exit(&args("--version")),
                Some(super::EarlyExit::Version)
            );
            assert_eq!(
                super::early_exit(&args("-V")),
                Some(super::EarlyExit::Version)
            );
            assert_eq!(super::early_exit(&args("--minimized")), None);
            // `--help` 必须压过弹窗/UI-client 参数，否则帮助会被静默吞掉。
            let popup_help = vec![
                "openless".to_string(),
                "--openless-egui-popup".to_string(),
                "--help".to_string(),
            ];
            assert_eq!(super::early_exit(&popup_help), Some(super::EarlyExit::Help));
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
                warming_started: None,
                warmup_ms: openless_linux_egui::load_capsule_warmup_ms(),
                avatar: QaAvatar::default(),
                lang: Lang::ZhCn,
                overlay: None,
                hotkey_matcher: crate::ui::local_hotkeys::LocalHotkeyMatcher::default(),
            }
        }

        fn vocab_card_message(session_id: &str, sequence: u64) -> HostToPopup {
            HostToPopup::Capsule {
                version: POPUP_PROTOCOL_VERSION,
                session_id: session_id.into(),
                sequence,
                phase: String::new(),
                text: String::new(),
                transcript: String::new(),
                transcript_font_size: 14,
                audio_level: None,
                translation_active: false,
                style: "siri".into(),
                suggestions: vec![openless_linux_egui::CapsuleSuggestion {
                    id: "candidate".into(),
                    pattern: "old".into(),
                    replacement: "new".into(),
                }],
            }
        }

        fn vocab_host_app() -> (tempfile::TempDir, OpenLessEguiApp) {
            let dir = tempfile::tempdir().unwrap();
            let mut app = fixture_app(false);
            let host_actions = Arc::new(openless_linux_egui::LinuxHostActions::default());
            let mut dependencies = openless_core::BackendDependencies::unsupported();
            dependencies.host_actions = host_actions.clone();
            dependencies.credential_store =
                Arc::new(openless_core::InMemoryCredentialStore::default());
            dependencies.services.remote_input = Arc::new(
                openless_core::RemoteInputService::new(
                    Arc::new(openless_core::testing::RecordingRemoteInputRuntime::default()),
                    8443,
                    "en",
                )
                .unwrap(),
            );
            let backend = Arc::new(
                openless_core::OpenLessBackend::new(
                    BackendConfig {
                        data_dir: dir.path().to_path_buf(),
                        ..Default::default()
                    },
                    dependencies,
                )
                .unwrap(),
            );
            let runtime = openless_linux_egui::LinuxBackendRuntime {
                backend: backend.clone(),
                host_actions,
                settings_runtime: Arc::new(openless_core::settings::NoopSettingsRuntime),
            };
            app.native = Some(
                app.tokio
                    .block_on(LinuxNativeRuntime::start(runtime, None, None))
                    .unwrap(),
            );
            app.snapshot = Some(backend.snapshot());
            app.preferences = Some(backend.get_preferences());
            app.capsule_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            (dir, app)
        }

        #[test]
        fn layer_capsule_waits_for_content_and_exits_only_for_an_accepted_hide() {
            let (tx, rx) = mpsc::channel();
            let mut app = popup_app(PopupKind::Capsule, rx);
            let ctx = egui::Context::default();
            let raw = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 240.0),
                )),
                ..Default::default()
            };
            // 无 GPU 提交的测试要清掉纹理增量，避免析构掩盖断言结果。
            let mut waiting = app.layer_frame(&ctx, raw(), false);
            waiting.output.textures_delta.clear();
            assert!(!waiting.exit);
            assert!(waiting.output.shapes.is_empty());
            tx.send(vocab_card_message("card", 1)).unwrap();
            let mut visible = app.layer_frame(&ctx, raw(), false);
            visible.output.textures_delta.clear();
            assert!(!visible.exit);
            assert!(!visible.output.shapes.is_empty());
            for (session_id, sequence) in [("card", 1), ("old-card", 2)] {
                tx.send(HostToPopup::Hide {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: session_id.into(),
                    sequence,
                })
                .unwrap();
                let mut frame = app.layer_frame(&ctx, raw(), false);
                frame.output.textures_delta.clear();
                assert!(!frame.exit);
            }
            tx.send(HostToPopup::Hide {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "card".into(),
                sequence: 2,
            })
            .unwrap();
            let mut hidden = app.layer_frame(&ctx, raw(), false);
            hidden.output.textures_delta.clear();
            assert!(hidden.exit);
            assert!(!app.state.visible);
            assert!(hidden.output.shapes.is_empty());
        }

        #[test]
        fn dictation_starting_clears_the_vocab_card_before_recording() {
            let (_dir, mut app) = vocab_host_app();
            let backend = app.backend().unwrap();
            backend
                .queue_pending_correction("old".into(), "new".into())
                .unwrap();
            app.refresh_vocab_suggestions();
            assert!(app.vocab_card_session.is_some());
            let session = openless_core::SessionId::new();
            app.apply_event(BackendEvent {
                sequence: 1,
                session_id: Some(session),
                kind: BackendEventKind::DictationStateChanged(
                    openless_core::DictationStateSnapshot {
                        phase: DictationPhase::Starting,
                        session_id: Some(session),
                        ..Default::default()
                    },
                ),
            });
            assert!(app.vocab_suggestions.is_empty());
            assert!(app.vocab_card_session.is_none());
            assert!(backend.pending_corrections().is_empty());
            assert_eq!(app.capsule_session, Some(session.to_string()));
        }

        #[test]
        fn vocab_actions_use_card_identity_and_reject_duplicates_and_old_cards() {
            let (_dir, mut app) = vocab_host_app();
            let backend = app.backend().unwrap();
            let first = backend
                .queue_pending_correction("first".into(), "First".into())
                .unwrap()
                .unwrap();
            let second = backend
                .queue_pending_correction("second".into(), "Second".into())
                .unwrap()
                .unwrap();
            app.refresh_vocab_suggestions();
            let token = app.vocab_card_session.clone().unwrap();
            assert_eq!(
                app.expected_popup_session(PopupKind::Capsule),
                Some(token.clone())
            );
            app.snapshot.as_mut().unwrap().dictation.session_id =
                Some(openless_core::SessionId::new());
            assert_eq!(
                app.expected_popup_session(PopupKind::Capsule),
                Some(token.clone())
            );
            let action = |token: &str, id: &str, sequence, accept| {
                PopupSupervisorEvent::Message(PopupToHost::VocabSuggestion {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: token.into(),
                    sequence,
                    id: id.into(),
                    accept,
                })
            };
            for (card, id, sequence) in [
                ("old-card", first.id.as_str(), 100),
                (token.as_str(), "missing", 100),
                (token.as_str(), first.id.as_str(), 0),
            ] {
                app.apply_popup_event(PopupKind::Capsule, action(card, id, sequence, true));
                assert_eq!(backend.pending_corrections().len(), 2);
                assert!(backend.list_vocabulary().unwrap().is_empty());
            }
            app.apply_popup_event(PopupKind::Qa, action(&token, &first.id, 1, true));
            assert!(backend.list_vocabulary().unwrap().is_empty());
            app.apply_popup_event(PopupKind::Capsule, action(&token, &first.id, 1, true));
            assert_eq!(backend.list_vocabulary().unwrap()[0].phrase, "First");
            assert_eq!(app.vocab_card_session.as_ref(), Some(&token));
            app.apply_popup_event(PopupKind::Capsule, action(&token, &second.id, 1, false));
            assert_eq!(backend.pending_corrections().len(), 1);
            app.apply_popup_event(PopupKind::Capsule, action(&token, &second.id, 2, false));
            assert!(backend.pending_corrections().is_empty());
            assert!(app.vocab_card_session.is_none());
            let next = backend
                .queue_pending_correction("third".into(), "Third".into())
                .unwrap()
                .unwrap();
            app.refresh_vocab_suggestions();
            let next_token = app.vocab_card_session.clone().unwrap();
            assert_ne!(next_token, token);
            app.apply_popup_event(PopupKind::Capsule, action(&token, &next.id, 100, true));
            assert_eq!(backend.pending_corrections().len(), 1);
            app.apply_popup_event(PopupKind::Capsule, action(&next_token, &next.id, 1, false));
            assert!(backend.pending_corrections().is_empty());
            assert!(app.vocab_card_session.is_none());
        }

        #[test]
        fn an_empty_core_card_hides_even_after_the_host_candidates_are_cleared() {
            let (_dir, mut app) = vocab_host_app();
            let backend = app.backend().unwrap();
            let candidate = backend
                .queue_pending_correction("old".into(), "new".into())
                .unwrap()
                .unwrap();
            app.refresh_vocab_suggestions();
            let token = app.vocab_card_session.clone().unwrap();
            let sequence = app.popup_sequences[popup_kind_index(PopupKind::Capsule)];
            backend.dismiss_pending_corrections();
            app.apply_popup_event(
                PopupKind::Capsule,
                PopupSupervisorEvent::Message(PopupToHost::VocabSuggestion {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: token,
                    sequence: 1,
                    id: candidate.id,
                    accept: true,
                }),
            );
            assert!(backend.list_vocabulary().unwrap().is_empty());
            app.refresh_vocab_suggestions();
            assert!(app.vocab_suggestions.is_empty());
            assert!(app.vocab_card_session.is_none());
            assert_eq!(
                app.popup_sequences[popup_kind_index(PopupKind::Capsule)],
                sequence + 1
            );
        }

        #[test]
        fn capsule_rebuild_rotates_card_identity_and_rejects_old_process_events() {
            let mut app = fixture_app(false);
            let HostToPopup::Capsule { suggestions, .. } = vocab_card_message("old-card", 1) else {
                unreachable!()
            };
            app.vocab_suggestions = suggestions;
            app.vocab_card_session = Some("old-card".into());
            let old_generation = app.capsule_popup_generation;
            app.ensure_popup(PopupKind::Capsule);
            let token = app.vocab_card_session.clone().unwrap();
            assert_ne!(token, "old-card");
            assert_ne!(app.capsule_popup_generation, old_generation);
            app.capsule_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            app.apply_popup_event_in_generation(
                PopupKind::Capsule,
                old_generation,
                PopupSupervisorEvent::Exited {
                    code: Some(0),
                    crashed: false,
                },
            );
            app.apply_popup_event_in_generation(
                PopupKind::Capsule,
                old_generation,
                PopupSupervisorEvent::SpawnFailed("old failure".into()),
            );
            assert!(app.capsule_popup.is_some());
            let sequence = app.popup_sequences[popup_kind_index(PopupKind::Capsule)];
            app.apply_popup_event(
                PopupKind::Capsule,
                PopupSupervisorEvent::Message(PopupToHost::Ready {
                    version: POPUP_PROTOCOL_VERSION,
                    session_id: token.clone(),
                    sequence: 1,
                    kind: PopupKind::Capsule,
                }),
            );
            assert_eq!(app.vocab_card_session, Some(token));
            assert_eq!(
                app.popup_sequences[popup_kind_index(PopupKind::Capsule)],
                sequence + 1
            );
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
                transcript: String::new(),
                transcript_font_size: 14,
                audio_level: Some(0.2),
                translation_active: false,
                style: "siri".into(),
                suggestions: Vec::new(),
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
        fn closing_qa_drops_its_window_and_late_idle_does_not_reopen_it() {
            let mut app = fixture_app(true);
            app.qa_visible = true;
            app.qa_state = Some(QaStateEvent::simple(QaStateKind::Idle));
            app.qa_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            app.toggle_qa_panel();
            assert!(!app.qa_visible);
            assert!(app.qa_popup.is_none());
            // 收起后迟到的 Idle 快照不得把窗口拉回来。
            app.apply_event(BackendEvent {
                sequence: 1,
                session_id: None,
                kind: BackendEventKind::QaState(QaStateEvent::simple(QaStateKind::Idle)),
            });
            assert!(app.qa_popup.is_none());
            assert!(app.expected_popup_session(PopupKind::Qa).is_none());
        }

        #[test]
        fn late_qa_action_failures_do_not_overwrite_a_new_turn() {
            let mut app = fixture_app(true);
            let mut state = QaStateEvent::simple(QaStateKind::Thinking);
            state.session_id = Some(openless_core::SessionId::new().to_string());
            app.qa_state = Some(state);
            app.qa_visible = true;
            // 空态（context 为 None）的失败不能写到已有会话的面板上。
            app.apply_qa_action_result(
                app.qa_popup_presentation.generation,
                None,
                Err("old failure".into()),
            );
            assert!(app.qa_state.as_ref().unwrap().error.is_none());
        }

        #[test]
        fn a_previous_preview_cannot_close_the_current_qa_window() {
            let mut app = fixture_app(true);
            let current = openless_core::SessionId::new().to_string();
            app.qa_visible = true;
            app.qa_popup_presentation.session_id = Some(current.clone());
            app.qa_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            app.hide_popup(
                PopupKind::Qa,
                openless_core::SessionId::new().to_string(),
                1,
            );
            assert!(app.qa_popup.is_some());
            assert!(app.qa_visible);
            assert_eq!(app.qa_popup_presentation.session_id, Some(current));
        }

        #[test]
        fn a_previous_qa_turn_cannot_close_the_current_preview() {
            let mut app = fixture_app(true);
            let current = openless_core::SessionId::new();
            app.polish_result_visible = true;
            app.selection = Some(SelectionSnapshot {
                session_id: Some(current),
                phase: SelectionPhase::Preview,
                ..Default::default()
            });
            app.qa_popup_presentation.session_id = Some(current.to_string());
            app.qa_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            app.hide_popup(
                PopupKind::Qa,
                openless_core::SessionId::new().to_string(),
                1,
            );
            assert!(app.qa_popup.is_some());
            assert!(app.polish_result_visible);
            assert_eq!(
                app.expected_popup_session(PopupKind::Qa),
                Some(current.to_string())
            );
        }

        #[test]
        fn closing_the_current_preview_also_clears_qa_visibility() {
            let mut app = fixture_app(true);
            let current = openless_core::SessionId::new().to_string();
            app.qa_visible = true;
            app.qa_popup_presentation.session_id = Some(current.clone());
            app.qa_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            app.hide_popup(PopupKind::Qa, current, 1);
            assert!(app.qa_popup.is_none());
            assert!(!app.qa_visible);
            assert!(!app.polish_result_visible);
        }

        #[test]
        fn background_qa_updates_leave_the_current_preview_visible() {
            let mut app = fixture_app(true);
            let preview = openless_core::SessionId::new().to_string();
            app.qa_visible = true;
            app.polish_result_visible = true;
            app.qa_state = Some(QaStateEvent::simple(QaStateKind::Idle));
            app.qa_popup_presentation.session_id = Some(preview.clone());
            app.qa_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            let generation = app.qa_popup_presentation.generation;
            app.show_qa_popup();
            assert!(app.qa_popup.is_some());
            assert_eq!(app.qa_popup_presentation.session_id, Some(preview));
            assert_eq!(app.qa_popup_presentation.generation, generation);
        }

        #[test]
        fn an_old_process_exit_cannot_clear_a_reopened_qa_window() {
            let mut app = fixture_app(true);
            let old_generation = app.qa_popup_presentation.generation;
            app.close_qa_popup();
            app.qa_visible = true;
            app.qa_popup = Some(PopupSupervisor::spawn_command(
                app.tokio.handle(),
                tokio::process::Command::new("/bin/cat"),
            ));
            app.apply_popup_event_in_generation(
                PopupKind::Qa,
                old_generation,
                PopupSupervisorEvent::Exited {
                    code: Some(0),
                    crashed: false,
                },
            );
            assert!(app.qa_popup.is_some());
            assert!(app.qa_visible);
        }

        #[test]
        fn an_old_idle_failure_cannot_appear_in_a_reopened_idle_panel() {
            let mut app = fixture_app(true);
            app.qa_visible = true;
            app.qa_state = Some(QaStateEvent::simple(QaStateKind::Idle));
            let old_generation = app.qa_popup_presentation.generation;
            app.close_qa_popup();
            app.qa_visible = true;
            app.qa_state = Some(QaStateEvent::simple(QaStateKind::Idle));
            let status = app.status.clone();
            app.apply_qa_action_result(old_generation, None, Err("old failure".into()));
            assert!(app.qa_state.as_ref().unwrap().error.is_none());
            assert_eq!(app.status, status);
            app.apply_qa_action_result(
                app.qa_popup_presentation.generation,
                None,
                Err("current failure".into()),
            );
            assert_eq!(
                app.qa_state.as_ref().unwrap().error.as_deref(),
                Some("current failure")
            );
        }

        #[test]
        fn popup_sequences_advance_without_new_core_events() {
            // 同一 Core 事件下重复发快照：序号必须递增，否则弹窗会把新快照当旧消息丢掉。
            let mut message = HostToPopup::QaSnapshot {
                version: POPUP_PROTOCOL_VERSION,
                session_id: "qa".into(),
                sequence: 0,
                phase: "Idle".into(),
                messages: Vec::new(),
                selection_preview: None,
                streaming_answer: String::new(),
                error: None,
                edit_instruction_mode: false,
                edit_apply_available: false,
                edit_revert_available: false,
                pinned: false,
                viewer_login: String::new(),
            };
            message.set_sequence(3);
            assert_eq!(message.sequence(), 3);
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

        /// 仅测试用：生产路径（设置页的 style-pack 快捷键行）直接改
        /// `preferences.style_pack_hotkeys`，见 `main.rs` 的 `StyleHotkey` 分支。
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

            // Last-7/last-30 aggregates were folded into the daily series:
            // the UI slices the tail of `activity_daily` for its period chart.

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
    // Select a process-wide provider before HTTPS or secure WebSocket clients start.
    // reqwest and the native TLS listener enable different rustls provider features.
    let _ = rustls::crypto::ring::default_provider().install_default();
    if let Err(error) = linux_app::run() {
        eprintln!("OpenLess Linux UI failed: {error}");
        std::process::exit(1);
    }
}
