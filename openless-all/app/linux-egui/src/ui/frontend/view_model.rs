use openless_linux_egui::Lang;

// ── Page / Tab ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    History,
    Vocab,
    Style,
    Marketplace,
    SelectionAsk,
    Translation,
    Corrections,
    Settings,
}

// ── FrontendAction ──────────────────────────────────────────────────────────

/// Every user interaction the frontend can produce. The host
/// (`OpenLessEguiApp`) drains these actions and dispatches them to existing
/// Core / backend methods without duplicating the Core state machine.
#[derive(Clone, Debug)]
pub enum FrontendAction {
    /// Navigate to a different page.
    Navigate(Page),
    /// Open / close the in-window settings overlay.
    ToggleSettings,
    /// Close the settings overlay (from the × button).
    CloseSettings,
    /// Marketplace search query changed.
    MarketplaceSearch(String),
    /// Marketplace sort mode changed.
    MarketplaceSort(MarketplaceSort),
    /// Marketplace refresh requested.
    MarketplaceRefresh,
    /// Marketplace "my packs" requested.
    MarketplaceMyPacks,
    /// Open marketplace pack detail.
    MarketplaceDetail(usize),
    /// Close marketplace detail modal.
    MarketplaceCloseDetail,
    /// Download marketplace pack ZIP.
    MarketplaceDownload(usize),
    /// Install marketplace pack.
    MarketplaceInstall(usize),
    /// Toggle marketplace pack like.
    MarketplaceToggleLike(usize),
    /// History search query changed.
    HistorySearch(String),
    /// Select a history entry (index into `history_entries`).
    HistorySelect(usize),
    /// Re-read the history list from Core.
    HistoryRefresh,
    /// Ask for confirmation before clearing all history.
    HistoryRequestClear,
    /// Ask for confirmation before deleting one entry.
    HistoryRequestDelete(usize),
    /// Confirm the pending destructive history action.
    HistoryConfirmAction,
    /// Dismiss the pending confirmation dialog.
    HistoryCancelConfirm,
    /// Export a history entry's recording to a file.
    HistoryExport(usize),
    /// Re-run ASR on a history entry's recording.
    HistoryRetranscribe(usize),
    /// Open a history entry's recording in the system player.
    HistoryPlay(usize),
    /// Vocab entry added.
    VocabAddPhrase(String),
    /// Vocab list filter changed (0 = all, 1 = auto-collected, 2 = manual).
    VocabFilter(usize),
    /// Vocab search query changed.
    VocabSearch(String),
    /// Vocab entry removed.
    VocabRemovePhrase(usize),
    /// Vocab entry toggled enabled/disabled.
    VocabTogglePhrase(usize),
    /// Correction rule added.
    VocabAddRule {
        pattern: String,
        replacement: String,
    },
    /// Correction rule removed.
    VocabRemoveRule(usize),
    /// Correction rule toggled.
    VocabToggleRule(usize),
    /// Vocab preset applied.
    VocabApplyPreset(usize),
    /// Vocab preset created.
    VocabCreatePreset {
        name: String,
        phrases: String,
    },
    /// Style pack activated.
    StyleActivate(usize),
    /// Style pack exported.
    StyleExport(usize),
    /// Style pack editor opened.
    StyleEdit(usize),
    /// Style editor prompt saved.
    StyleSaveEditor(String),
    /// Style editor closed.
    StyleCloseEditor,
    /// New style pack creation requested.
    StyleNewPack,
    /// Import style ZIP.
    StyleImport,
    /// Selection ask history toggle.
    SelectionAskToggleHistory,
    /// Translation working language toggled.
    TranslationToggleLanguage(String),
    /// Translation target language changed.
    TranslationSetTarget(String),
    /// Settings toggle changed.
    SettingsToggle(SettingsField),
    /// Settings combo index changed.
    SettingsCombo(SettingsComboField, usize),
    /// Settings text field changed.
    SettingsText(SettingsTextField, String),
    /// Settings action button clicked.
    SettingsAction(SettingsActionField),
    /// Settings section changed.
    SettingsSection(SettingsSection),
    /// Settings notice message.
    SettingsNotice(String),
    /// Overview: re-read credentials / history / activity from Core.
    OverviewRefresh,
    /// Overview: period toggle (0 = last 7 days, 1 = last 30 days).
    OverviewPeriod(usize),
    /// Overview: metric toggle (0 = count, 1 = chars, 2 = duration).
    OverviewMetric(usize),
    /// Window close requested.
    WindowClose,
    /// Window maximize/minimize toggle.
    WindowMaximize,
    /// Window minimize.
    WindowMinimize,
    /// Sidebar group toggle.
    SidebarToggleStyle,
    SidebarToggleTools,
}

// ── Marketplace types ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarketplaceSort {
    #[default]
    Popular,
    New,
    Liked,
}

#[derive(Clone, Debug)]
pub struct MarketplacePack {
    pub name: String,
    pub version: String,
    pub description: String,
    pub mode: String,
    pub author: String,
    pub tags: Vec<String>,
    pub likes: u32,
    pub downloads: u32,
    pub is_new: bool,
}

// ── Settings types ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Shortcuts,
    Appearance,
    Services,
    Privacy,
    Advanced,
    About,
}

#[derive(Clone, Copy, Debug)]
pub enum SettingsField {
    RecordingEnabled,
    RealtimeMode,
    StreamingInsert,
    RestoreClipboard,
    StartMinimized,
    AutoUpdate,
    RemoteInput,
    SelectionAssistant,
    SelectionVoice,
    StackedLayout,
    ConservativeLayout,
    ActivityHeatmap,
    SystemProxy,
    LocalModel,
    MarketplaceEnabled,
    RememberHistory,
    RecordAudio,
    LessComputer,
    Multimodal,
    BetaChannel,
}

#[derive(Clone, Copy, Debug)]
pub enum SettingsComboField {
    Provider,
    Language,
    Theme,
    Retention,
    Microphone,
    RecordingMode,
}

#[derive(Clone, Debug)]
pub enum SettingsTextField {
    ApiKey,
    Endpoint,
    Model,
    RemotePort,
    ClaudePrompt,
}

#[derive(Clone, Copy, Debug)]
pub enum SettingsActionField {
    ConnectionTest,
    ModelManagement,
    ExtensionManagement,
    Permissions,
    ClearHistory,
    ClaudeDetect,
    ClaudeConsole,
    ClaudeRunTest,
    ExportDiagnostics,
    CheckUpdate,
    OpenGitHub,
    OpenHelp,
    OpenReleaseNotes,
    OpenFeedback,
    CopyQQ,
}

// ── Vocab types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VocabEntry {
    pub phrase: String,
    pub hits: usize,
    pub enabled: bool,
    pub learned: bool,
}

#[derive(Clone, Debug)]
pub struct CorrectionRule {
    pub pattern: String,
    pub replacement: String,
    pub enabled: bool,
    pub learned: bool,
}

#[derive(Clone, Debug)]
pub struct SavedVocabPreset {
    pub name: String,
    pub phrases: String,
}

// ── History types ───────────────────────────────────────────────────────────

/// Insert outcome, mirrored from Core's `HistoryInsertStatus` into a plain
/// frontend enum so the page never has to depend on Core types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryInsertStatus {
    #[default]
    NotRequested,
    Inserted,
    PasteSent,
    CopiedFallback,
    Failed,
}

/// A pending destructive action that needs an in-window confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryConfirm {
    Clear,
    Delete(usize),
}

/// One history row plus everything the detail panel shows.
#[derive(Clone, Debug, Default)]
pub struct HistoryEntry {
    pub id: String,
    pub created_at: String,
    /// Base polish mode; drives the list pill tone (raw renders as outline).
    pub mode: OverviewMode,
    /// Pill text: the style-pack name, or the mode name for records without one.
    pub style_label: String,
    pub raw_transcript: String,
    pub final_text: String,
    pub duration_ms: Option<u64>,
    pub insert_status: HistoryInsertStatus,
    pub has_audio: bool,
    pub asr_provider: Option<String>,
    pub asr_model: Option<String>,
    pub asr_ms: Option<u64>,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub polish_ms: Option<u64>,
    pub app_name: Option<String>,
    pub dictionary_count: Option<u32>,
}

// ── Style types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct StylePack {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub accent: egui::Color32,
    pub is_builtin: bool,
    pub is_active: bool,
}

// ── Overview types ──────────────────────────────────────────────────────────

/// Polish mode shown as the mode pill on a "recent" row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverviewMode {
    #[default]
    Raw,
    Light,
    Structured,
    Formal,
}

/// One calendar day of activity (chronological inside
/// [`OverviewSummary::activity_daily`]).
#[derive(Clone, Debug, Default)]
pub struct OverviewActivityDay {
    /// `YYYY-MM-DD` in the host's local timezone.
    pub date: String,
    pub count: u32,
    pub chars: u64,
    pub duration_ms: u64,
}

/// One day of the annual activity heatmap (`YYYY-MM-DD` + dictation count).
#[derive(Clone, Debug, Default)]
pub struct OverviewHeatmapDay {
    pub date: String,
    pub count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct OverviewSummary {
    pub asr_provider: String,
    pub llm_provider: String,
    pub asr_configured: bool,
    pub llm_configured: bool,
    pub chars_today: u64,
    pub segments_today: usize,
    pub duration_ms_today: u64,
    pub avg_latency_ms: u64,
    pub history_total: usize,
    pub recent: Vec<OverviewRecentEntry>,
    /// Last 30 days ending today, chronological (oldest first). The period
    /// chart slices the tail for the 7-day view.
    pub activity_daily: Vec<OverviewActivityDay>,
    /// Calendar year rendered by the annual heatmap card.
    pub heatmap_year: i32,
    /// Every day of `heatmap_year`, chronological. Days without activity are
    /// present with `count == 0` so the page can lay out the grid.
    pub heatmap: Vec<OverviewHeatmapDay>,
}

#[derive(Clone, Debug, Default)]
pub struct OverviewRecentEntry {
    pub created_at: String,
    pub final_text: String,
    pub raw_transcript: String,
    pub mode: OverviewMode,
    pub duration_ms: Option<u64>,
}

// ── FrontendViewModel ───────────────────────────────────────────────────────

/// Pure display state for the egui frontend. Contains no mock data — every
/// field is populated by the host (`OpenLessEguiApp`) from Core / backend
/// sources. Unwired fields show empty / Loading / Unsupported states.
#[derive(Clone, Debug)]
pub struct FrontendViewModel {
    pub active_page: Page,
    pub style_open: bool,
    pub tools_open: bool,
    pub settings_open: bool,

    /// Resolved UI language, injected by the host each frame so the pure
    /// renderer can look up localized strings without touching global state.
    pub lang: Lang,

    // Overview
    pub overview_loading: bool,
    pub overview_error: Option<String>,
    pub overview: Option<OverviewSummary>,
    pub overview_period: usize,
    pub overview_metric: usize,

    // History
    pub history_query: String,
    pub history_selected: usize,
    pub history_entries: Vec<HistoryEntry>,
    pub history_loading: bool,
    pub history_error: Option<String>,
    /// Set while a destructive action awaits confirmation (clear-all / delete).
    pub history_confirm: Option<HistoryConfirm>,

    // Vocab
    pub vocab_entries: Vec<VocabEntry>,
    pub vocab_rules: Vec<CorrectionRule>,
    /// 0 = all, 1 = auto-collected, 2 = manual.
    pub vocab_filter: usize,
    pub vocab_query: String,
    pub vocab_input: String,
    pub vocab_pattern: String,
    pub vocab_replacement: String,
    pub vocab_preset_name: String,
    pub vocab_preset_phrases: String,
    pub vocab_selected_presets: Vec<usize>,
    pub vocab_editing_preset: Option<usize>,
    pub vocab_saved_presets: Vec<SavedVocabPreset>,
    pub vocab_presets_open: bool,
    pub vocab_corrections_open: bool,
    pub vocab_entries_open: bool,
    pub vocab_error: Option<String>,
    pub vocab_unsupported: bool,

    // Style
    pub style_packs: Vec<StylePack>,
    pub style_selected: usize,
    pub style_selection_workflow: bool,
    pub style_editor_open: bool,
    pub style_prompt: String,
    pub style_notice: Option<String>,
    pub style_unsupported: bool,

    // Marketplace
    pub marketplace_query: String,
    pub marketplace_sort: MarketplaceSort,
    pub marketplace_packs: Vec<MarketplacePack>,
    pub marketplace_selected: Option<usize>,
    pub marketplace_liked: Vec<usize>,
    pub marketplace_notice: Option<String>,
    pub marketplace_loading: bool,
    pub marketplace_unsupported: bool,

    // Settings
    pub settings_section: SettingsSection,
    /// Rail search query in the settings modal.
    pub settings_query: String,
    pub settings_notice: Option<String>,
    pub settings: SettingsFields,

    // Selection ask
    pub qa_save_history: bool,
    pub selection_unsupported: bool,

    // Translation
    pub translation_working_languages: Vec<String>,
    /// Language search query on the translation page.
    pub translation_query: String,
    pub translation_target_language: String,
    pub translation_unsupported: bool,

    // Status bar
    pub version: String,
    pub status: String,
    /// Display label for the dictation shortcut (e.g. `Ctrl+Shift+Space`).
    pub dictation_hotkey: String,
    /// Display label for the selection-ask popup shortcut.
    pub qa_hotkey: String,
    /// Display label for the translation modifier shortcut.
    pub translation_hotkey: String,
}

impl Default for FrontendViewModel {
    fn default() -> Self {
        Self {
            lang: Lang::ZhCn,
            active_page: Page::Overview,
            style_open: true,
            tools_open: true,
            settings_open: false,
            overview_loading: true,
            overview_error: None,
            overview: None,
            overview_period: 0,
            overview_metric: 0,
            history_query: String::new(),
            history_selected: 0,
            history_entries: Vec::new(),
            history_loading: true,
            history_error: None,
            history_confirm: None,
            vocab_entries: Vec::new(),
            vocab_rules: Vec::new(),
            vocab_filter: 0,
            vocab_query: String::new(),
            vocab_input: String::new(),
            vocab_pattern: String::new(),
            vocab_replacement: String::new(),
            vocab_preset_name: String::new(),
            vocab_preset_phrases: String::new(),
            vocab_selected_presets: Vec::new(),
            vocab_editing_preset: None,
            vocab_saved_presets: Vec::new(),
            vocab_presets_open: false,
            vocab_corrections_open: false,
            vocab_entries_open: true,
            vocab_error: None,
            vocab_unsupported: true,
            style_packs: Vec::new(),
            style_selected: 0,
            style_selection_workflow: false,
            style_editor_open: false,
            style_prompt: String::new(),
            style_notice: None,
            style_unsupported: true,
            marketplace_query: String::new(),
            marketplace_sort: MarketplaceSort::Popular,
            marketplace_packs: Vec::new(),
            marketplace_selected: None,
            marketplace_liked: Vec::new(),
            marketplace_notice: None,
            marketplace_loading: true,
            marketplace_unsupported: true,
            settings_section: SettingsSection::General,
            settings_query: String::new(),
            settings_notice: None,
            settings: SettingsFields::default(),
            qa_save_history: false,
            selection_unsupported: true,
            translation_working_languages: Vec::new(),
            translation_query: String::new(),
            translation_target_language: String::new(),
            translation_unsupported: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
            status: String::new(),
            dictation_hotkey: String::new(),
            qa_hotkey: String::new(),
            translation_hotkey: String::new(),
        }
    }
}

/// Mirror of the egui-frontend `SettingsState` fields, but with no default
/// mock data. All values come from the host.
#[derive(Clone, Debug)]
pub struct SettingsFields {
    pub recording_enabled: bool,
    pub realtime_mode: bool,
    pub streaming_insert: bool,
    pub restore_clipboard: bool,
    pub start_minimized: bool,
    pub auto_update: bool,
    pub remote_input: bool,
    pub selection_assistant: bool,
    pub selection_voice: bool,
    pub stacked_layout: bool,
    pub conservative_layout: bool,
    pub activity_heatmap: bool,
    pub system_proxy: bool,
    pub local_model: bool,
    pub marketplace_enabled: bool,
    pub remember_history: bool,
    pub record_audio: bool,
    pub less_computer: bool,
    pub multimodal: bool,
    pub beta_channel: bool,
    pub claude_expanded: bool,
    pub provider: usize,
    pub language: usize,
    pub theme: usize,
    pub retention: usize,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub remote_port: String,
    pub claude_prompt: String,
}

impl Default for SettingsFields {
    fn default() -> Self {
        Self {
            recording_enabled: false,
            realtime_mode: false,
            streaming_insert: false,
            restore_clipboard: false,
            start_minimized: false,
            auto_update: false,
            remote_input: false,
            selection_assistant: false,
            selection_voice: false,
            stacked_layout: false,
            conservative_layout: false,
            activity_heatmap: false,
            system_proxy: false,
            local_model: false,
            marketplace_enabled: false,
            remember_history: false,
            record_audio: false,
            less_computer: false,
            multimodal: false,
            beta_channel: false,
            claude_expanded: false,
            provider: 0,
            language: 0,
            theme: 0,
            retention: 0,
            api_key: String::new(),
            endpoint: String::new(),
            model: String::new(),
            remote_port: String::new(),
            claude_prompt: String::new(),
        }
    }
}
