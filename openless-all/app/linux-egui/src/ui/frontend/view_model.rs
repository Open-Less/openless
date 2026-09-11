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
    AcceptCorrection(String),
    RejectCorrection(String),
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
    /// History filter changed.
    HistoryFilter(usize),
    /// Select a history entry.
    HistorySelect(usize),
    /// Clear all history.
    HistoryClear,
    /// Refresh history list.
    HistoryRefresh,
    /// Play/pause history audio.
    HistoryTogglePlay,
    /// Repolish a history entry.
    HistoryRepolish,
    HistoryRetranscribe,
    HistoryCancel,
    /// Delete a history entry.
    HistoryDelete(usize),
    /// Export history recording.
    HistoryExport(usize),
    /// Vocab entry added.
    VocabAddPhrase(String),
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
        id: Option<String>,
        name: String,
        phrases: String,
    },
    VocabDeletePreset(usize),
    VocabRefresh,
    /// Style pack activated.
    StyleActivate(usize),
    StyleRefresh,
    StyleReset,
    StyleDelete,
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
    pub id: String,
    pub name: String,
    pub phrases: String,
}

// ── History types ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub id: String,
    pub raw: String,
    pub mode: String,
    pub has_audio: bool,
    pub asr: String,
    pub llm: String,
    pub asr_ms: Option<u64>,
    pub polish_ms: Option<u64>,
    pub time: String,
    pub text: String,
    pub duration: String,
    pub tag: String,
}

// ── Style types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct StylePack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub accent: egui::Color32,
    pub is_builtin: bool,
    pub is_active: bool,
}

// ── Overview types ──────────────────────────────────────────────────────────

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
    pub last_7_segments: u64,
    pub last_30_segments: u64,
    pub heatmap_weeks: Vec<[u32; 7]>,
    pub heatmap_days: u32,
    pub activity_days_total: usize,
}

#[derive(Clone, Debug, Default)]
pub struct OverviewRecentEntry {
    pub created_at: String,
    pub final_text: String,
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

    // Overview
    pub overview_loading: bool,
    pub overview_error: Option<String>,
    pub overview: Option<OverviewSummary>,

    // History
    pub history_query: String,
    pub history_filter: usize,
    pub history_selected: usize,
    pub history_entries: Vec<HistoryEntry>,
    pub history_cleared: bool,
    pub history_repolished: bool,
    pub history_audio_playing: bool,
    pub history_busy: bool,
    pub history_repolish_style: String,
    pub history_results: std::collections::HashMap<String, Vec<String>>,
    pub history_style_picker_open: bool,

    // Vocab
    pub vocab_entries: Vec<VocabEntry>,
    pub vocab_rules: Vec<CorrectionRule>,
    pub pending_corrections: Vec<openless_core::shared_types::PendingCorrection>,
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
    pub style_name: String,
    pub style_description: String,
    pub style_selection_prompt: String,
    pub style_builtin: bool,
    pub style_saving: bool,
    pub style_notice: Option<String>,
    pub style_unsupported: bool,

    // Marketplace
    pub marketplace_query: String,
    pub marketplace_sort: MarketplaceSort,
    pub marketplace_packs: Vec<MarketplacePack>,
    pub marketplace_selected: Option<usize>,
    pub marketplace_liked: Vec<usize>,
    pub marketplace_notice: Option<String>,
    pub marketplace_prompt: Option<String>,
    pub marketplace_loading: bool,
    pub marketplace_unsupported: bool,

    // Settings
    pub settings_section: SettingsSection,
    pub settings_notice: Option<String>,
    pub settings: SettingsFields,

    // Selection ask
    pub qa_save_history: bool,
    pub selection_unsupported: bool,

    // Translation
    pub translation_working_languages: Vec<String>,
    pub translation_target_language: String,
    pub translation_unsupported: bool,

    // Status bar
    pub version: String,
    pub status: String,
}

impl Default for FrontendViewModel {
    fn default() -> Self {
        Self {
            active_page: Page::Overview,
            style_open: true,
            tools_open: false,
            settings_open: false,
            overview_loading: true,
            overview_error: None,
            overview: None,
            history_query: String::new(),
            history_filter: 0,
            history_selected: 0,
            history_entries: Vec::new(),
            history_cleared: false,
            history_repolished: false,
            history_audio_playing: false,
            history_busy: false,
            history_repolish_style: String::new(),
            history_results: Default::default(),
            history_style_picker_open: false,
            vocab_entries: Vec::new(),
            vocab_rules: Vec::new(),
            pending_corrections: Vec::new(),
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
            style_name: String::new(),
            style_description: String::new(),
            style_selection_prompt: String::new(),
            style_builtin: false,
            style_saving: false,
            style_notice: None,
            style_unsupported: true,
            marketplace_query: String::new(),
            marketplace_sort: MarketplaceSort::Popular,
            marketplace_packs: Vec::new(),
            marketplace_selected: None,
            marketplace_liked: Vec::new(),
            marketplace_notice: None,
            marketplace_prompt: None,
            marketplace_loading: true,
            marketplace_unsupported: true,
            settings_section: SettingsSection::General,
            settings_notice: None,
            settings: SettingsFields::default(),
            qa_save_history: false,
            selection_unsupported: true,
            translation_working_languages: Vec::new(),
            translation_target_language: String::new(),
            translation_unsupported: true,
            version: env!("CARGO_PKG_VERSION").to_string(),
            status: String::new(),
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
