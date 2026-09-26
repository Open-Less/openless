use openless_linux_egui::Lang;

// ── Page / Tab ──────────────────────────────────────────────────────────────

#[derive(
    Clone, serde::Serialize, serde::Deserialize, Copy, Debug, Default, PartialEq, Eq, Hash,
)]
pub enum Page {
    #[default]
    Overview,
    History,
    QuickNote,
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
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
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
    MarketplaceCloseMine,
    /// Select a history entry (index into `history_entries`).
    HistorySelect(usize),
    /// Re-read the history list from Core.
    HistoryRefresh,
    /// Start or finish a permanent quick-note recording through Core.
    QuickNoteToggle,
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
    HistoryRepolishOpen(usize),
    HistoryRepolishClose,
    HistoryRepolish(usize, Option<usize>),
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
    /// Reload dictionary and correction rules from Core.
    VocabRefresh,
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
    /// Dismiss / restore the shortcut card on the Quick Note page.
    QuickNoteShortcutHidden(bool),
    StyleChooseIcon(usize),
    StyleResetIcon(usize),
    /// Style pack exported.
    StyleExport(usize),
    /// Style pack editor opened.
    StyleEdit(usize),
    /// Reset a built-in pack to Core's shipped prompt (`reset_builtin_style_pack`).
    StyleResetBuiltin,
    /// Delete the imported pack the editor is showing (`remove_style_pack`).
    StyleDeleteImported,
    /// Throw away the editor's local draft and re-read the stored pack.
    StyleRevertDraft,
    /// Style editor prompt saved.
    StyleSaveEditor {
        name: String,
        description: String,
        prompt: String,
        selection_prompt: String,
        voice_edit_prompt: String,
        tags: String,
        author: String,
        version: String,
        model: String,
        compatible_version: String,
    },
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
    /// AI-services sub-tab (0 = LLM, 1 = ASR, 2 = local models, 3 = connections).
    SettingsServicesView(usize),
    /// Enable/disable a channel (index into `settings.channels`).
    SettingsChannelToggle(usize),
    /// Validate a channel (index into `settings.channels`).
    SettingsChannelValidate(usize),
    /// Delete a channel (index into `settings.channels`).
    SettingsChannelDelete(usize),
    /// Open/close the "add channel" form.
    SettingsChannelFormOpen(bool),
    /// Provider picked in the add-channel form.
    SettingsChannelProvider(usize),
    /// Channel name typed in the add-channel form.
    SettingsChannelName(String),
    /// Create the channel described by the form.
    SettingsChannelCreate,
    /// Select a channel and open its provider editor (index into `channels`).
    SettingsChannelSelect(usize),
    /// Move a channel up/down; Core's `reorder_channels` owns the order.
    SettingsChannelMove {
        index: usize,
        delta: isize,
    },
    /// Switch a channel to another provider type (Core's `set_channel_provider_type`).
    SettingsChannelProviderType {
        index: usize,
        provider_type: String,
    },
    /// Make a channel the active provider (Core's `set_active_provider`).
    SettingsChannelActivate(usize),
    /// Provider editor field edited.
    SettingsProviderField(SettingsProviderField, String),
    /// Save the editor: rename + endpoint/model + credentials through Core.
    SettingsProviderSave,
    /// Drop the channel's stored secrets (Core's `remove_credential`).
    SettingsProviderClearSecrets,
    /// Ask Core for the provider's model list (Core's `provider.list_models`).
    SettingsProviderModels,
    /// 从「可用模型」里选了一个：写进模型字段**并立刻持久化**（Tauri
    /// `applyModel`：`setCredential(modelAccount, model)` +「已保存模型 X」）。
    SettingsProviderModelSelected(String),
    /// 模型字段在「预设下拉」与「自定义输入」之间切换（Tauri `customModelMode`）。
    SettingsProviderModelCustom(bool),
    /// 打开该服务商的模型文档页（Tauri `viewModels`，来自 `endpointPresets[].modelsUrl`）。
    SettingsProviderModelsUrl,
    /// Close the provider editor.
    SettingsProviderClose,
    /// 快捷键行的交互：展开/收起编辑菜单（`None` 收起全部）。
    ShortcutMenu(Option<ShortcutField>),
    /// 进入录制态（`None` = 取消录制）。
    ShortcutRecording(Option<ShortcutField>),
    /// 录入完成：主键 + 修饰键写回该绑定。
    ShortcutCaptured(ShortcutField, String, Vec<String>),
    /// 停用该绑定（核心录音快捷键不可停用，见 `ShortcutField::Dictation`）。
    ShortcutDisable(ShortcutField),
    /// 风格直达：开关草稿行 / 选择风格包 / 移除整行。
    StyleHotkeyDraft(bool),
    StyleHotkeyDraftPack(usize),
    StyleHotkeyRemove(usize),
    StyleHotkeyRepack(usize, usize),
    /// Re-read channels for the current AI-services view.
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

#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarketplaceSort {
    #[default]
    Popular,
    New,
    Liked,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct MarketplacePack {
    /// Marketplace id; the install action is index-based, but the UI needs the
    /// id to show which pack is currently installing.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub mode: String,
    pub author: String,
    pub origin_author_login: Option<String>,
    pub tags: Vec<String>,
    pub likes: u32,
    pub downloads: u32,
    /// Whether the signed-in user has liked this pack (`me/likes`).
    pub liked: bool,
}

// ── Settings types ──────────────────────────────────────────────────────────

/// Host permission state, mirroring the Tauri permission rows. Linux has no
/// OS permission prompts, so most of these stay `Unsupported` — but the value
/// now comes from the host snapshot instead of a hardcoded label.
#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionState {
    #[default]
    Unknown,
    Granted,
    Unsupported,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct SettingsPermissions {
    pub microphone: PermissionState,
    pub accessibility: PermissionState,
    pub hotkey: PermissionState,
    pub network: PermissionState,
}

/// What Core's prompt composer actually puts into the dictation prompt right
/// now (`preview_style_pack_runtime`). The editor shows one row per directive
/// instead of letting the UI guess the rules.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct StyleRuntimePreview {
    pub context_active: bool,
    pub hotword_active: bool,
    pub history_active: bool,
    pub omits_front_app: bool,
}

/// One 风格包直选 row (style pack name + its hotkey chip).
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct StylePackHotkeyRow {
    pub pack_id: String,
    /// 风格包显示名；风格包列表里找不到该 id 时用作下拉的回退文案。
    pub name: String,
    pub hotkey: String,
}

/// One editable shortcut row in 快捷键与选区. `StylePack(index)` addresses an
/// existing entry of [`SettingsFields::style_pack_hotkeys`]; `StyleDraft` is the
/// 「＋ 添加风格快捷键」row before it is committed.
#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutField {
    Dictation,
    Translation,
    Qa,
    QuickNote,
    SwitchStyle,
    OpenApp,
    CodingAgentVoice,
    SelectionPolish,
    StylePack(usize),
    StyleDraft,
}

/// One credential channel shown in the AI-services settings tab.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct SettingsChannel {
    pub name: String,
    /// Model / endpoint summary shown under the channel name.
    pub model: String,
    /// Provider descriptor label (already localized by the host).
    pub provider: String,
    /// Provider type id, used to offer the provider switch without a round trip.
    pub provider_type: String,
    /// True for the channel currently serving requests.
    pub is_active: bool,
    pub enabled: bool,
    /// Human-readable result of the last validation, if any.
    pub last_check: Option<String>,
}

/// A field of the provider editor. Secret fields are write-only: opening an
/// editor never reads an existing key back into egui state.
#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, PartialEq, Eq)]
pub enum SettingsProviderField {
    Name,
    Endpoint,
    Model,
    ResourceId,
    AuthMode,
    PrimarySecret,
    SecondarySecret,
}

/// Which inputs the editor renders. Core's `AuthRequirement` decides this;
/// the UI never judges whether the credentials are sufficient — ProviderService
/// re-checks the descriptor before any protocol request.
#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, PartialEq, Eq)]
pub enum SettingsProviderAuth {
    /// No credentials (local models).
    None,
    /// OAuth is driven by Core; the editor only explains it.
    OAuth,
    /// Volcengine: APP ID + Access Token, or API Key + Resource ID.
    Volcengine,
    /// Xfyun: AppID + API Key.
    Xfyun,
    /// Another Core-defined shape (Tencent Cloud and friends).
    Other,
    /// Endpoint + Model + API Key.
    ApiKey,
}

/// The open channel editor. Hydrated once per load from Core, then driven by
/// the host-side draft so typing is never clobbered by a re-read.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct SettingsProviderEditor {
    pub channel_id: String,
    /// Localized provider label (read-only).
    pub provider: String,
    pub provider_type: String,
    pub name: String,
    pub endpoint: String,
    pub model: String,
    pub resource_id: String,
    pub auth_mode: String,
    pub auth: SettingsProviderAuth,
    // Write-only secret drafts: they are empty on load and cleared once Core has
    // them, so a stored key never reaches egui state.
    pub primary_secret: String,
    pub secondary_secret: String,
    /// Result of `provider.list_models`.
    pub models: Vec<String>,
    pub models_loading: bool,
    /// Core descriptor 的固定模型清单（Tauri `descriptor.staticModels`）：非空时模型
    /// 字段是一个预设下拉，而不是空白输入框。
    pub static_models: Vec<String>,
    /// Core descriptor 的默认模型（Tauri `descriptor.defaultModel`）：模型为空时
    /// 提供「填入默认」。
    pub default_model: String,
    /// 当前 endpoint 对应的服务商文档页（Tauri `modelsUrl`）：有值时「可用模型」不是
    /// 拉取，而是打开文档。
    pub has_models_url: bool,
    /// 模型字段是否处于「自定义模型…」手输模式（Tauri `customModelMode`）。
    pub custom_model: bool,
    pub busy: bool,
}

/// Provider kinds available when creating a channel.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct SettingsChannelProvider {
    /// Provider type id sent back to Core.
    pub provider_type: String,
    /// Localized label shown in the picker.
    pub label: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Shortcuts,
    Appearance,
    Services,
    Privacy,
    Advanced,
    About,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug)]
pub enum SettingsField {
    StreamingInsert,
    /// 稳定模式（先录音后识别，上游 #1082）。
    StableTranscription,
    StreamingSaveClipboard,
    RestoreClipboard,
    StartMinimized,
    LaunchAtLogin,
    AutoUpdate,
    RemoteInput,
    SilenceAutoStop,
    AudioCue,
    /// 录音胶囊是否显示（Tauri `showCapsule`；Linux 上同样生效）。
    ShowCapsule,
    MuteWhileRecording,
    RecordAudioForDebug,
    ActivityHeatmap,
    SystemProxy,
    LessComputer,
    Multimodal,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug)]
pub enum SettingsComboField {
    Language,
    Theme,
    Microphone,
    /// 胶囊样式：0 = siri（流光），1 = classic（经典药丸），2 = typeless（深色胶囊）。
    CapsuleStyle,
    RecordingMode,
    SilenceSeconds,
    RemoteDefaultMode,
    /// 选区润色交付方式：0 = 直接替换，1 = 预览确认。
    SelectionPolishDelivery,
    /// Less Computer 的 Agent 后端（0 = Claude Code, 1 = OpenCode, 2 = Codex, 3 = dsh）。
    CodingAgentProvider,
    /// Less Computer 权限模式（0 = 放行, 1 = 只读/计划, 2 = 默认, 3 = 完全放行）。
    CodingAgentPermission,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub enum SettingsTextField {
    RemotePort,
    HistoryMaxEntries,
    /// 历史保留天数（0 = 永久）。
    RetentionDays,
    /// 润色上下文窗口（分钟，0 = 关闭）。
    PolishContextWindow,
    /// 调试录音最多保留条数。
    AudioRecordingMaxEntries,
    CodingAgentModel,
    CodingAgentWorkdir,
    CodingAgentExe,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug)]
pub enum SettingsActionField {
    /// 试听录音提示音（Tauri `audioCuePreview` 按钮）。
    PreviewAudioCue,
    ExportDiagnostics,
    CopyCertFingerprint,
    OpenGitHub,
    OpenHelp,
    OpenReleaseNotes,
    OpenFeedback,
    CopyQQ,
}

// ── Vocab types ─────────────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct VocabEntry {
    pub phrase: String,
    pub hits: usize,
    pub enabled: bool,
    pub learned: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct CorrectionRule {
    pub pattern: String,
    pub replacement: String,
    pub enabled: bool,
    pub learned: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct SavedVocabPreset {
    pub name: String,
    pub phrases: String,
}

// ── History types ───────────────────────────────────────────────────────────

/// In-app playback state for the entry currently being played.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct HistoryPlayback {
    pub id: String,
    pub position_ms: u64,
    pub total_ms: u64,
}

/// A pending destructive action that needs an in-window confirmation.
#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, PartialEq, Eq)]
pub enum HistoryConfirm {
    Clear,
    Delete(usize),
}

/// One history row plus everything the detail panel shows.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct HistoryEntry {
    pub id: String,
    pub quick_note: bool,
    pub error_code: Option<String>,
    pub created_at: String,
    /// Base polish mode; drives the list pill tone (raw renders as outline).
    pub mode: OverviewMode,
    /// Pill text: the style-pack name, or the mode name for records without one.
    pub style_label: String,
    pub raw_transcript: String,
    pub final_text: String,
    pub duration_ms: Option<u64>,
    pub has_audio: bool,
    pub asr_provider: Option<String>,
    pub asr_model: Option<String>,
    pub asr_ms: Option<u64>,
    pub llm_provider: Option<String>,
    pub app_name: Option<String>,
    pub dictionary_count: Option<u32>,
}

// ── Style types ─────────────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct StylePack {
    pub id: String,
    pub icon_path: Option<String>,
    pub icon_data_url: Option<String>,
    /// `raw` / `light` / `structured` / `formal`; picks the default icon.
    pub base_mode: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub is_builtin: bool,
    pub enabled: bool,
    /// Active pack for the dictation / ASR workflow.
    pub is_active: bool,
    /// Active pack for the selection-polish workflow (`prefs.selection_polish_style_pack_id`).
    pub selection_active: bool,
}

// ── Overview types ──────────────────────────────────────────────────────────

/// Polish mode shown as the mode pill on a "recent" row.
#[derive(Clone, serde::Serialize, serde::Deserialize, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverviewMode {
    #[default]
    Raw,
    Light,
    Structured,
    Formal,
}

/// One calendar day of activity (chronological inside
/// [`OverviewSummary::activity_daily`]).
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct OverviewActivityDay {
    /// `YYYY-MM-DD` in the host's local timezone.
    pub date: String,
    pub count: u32,
    pub chars: u64,
    pub duration_ms: u64,
}

/// One day of the annual activity heatmap (`YYYY-MM-DD` + dictation count).
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct OverviewHeatmapDay {
    pub date: String,
    pub count: u32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
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

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Default)]
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
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct FrontendViewModel {
    pub active_page: Page,
    pub style_open: bool,
    pub tools_open: bool,
    pub settings_open: bool,

    /// Resolved UI language, injected by the host each frame so the pure
    /// renderer can look up localized strings without touching global state.
    #[serde(with = "lang_tag")]
    pub lang: Lang,

    /// 明暗主题（Core 偏好）。宿主注入，UI 进程按它决定配色。
    pub theme_mode: openless_core::shared_types::ThemeMode,

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
    pub quick_note_recording: bool,
    /// Whether the dismissible shortcut card on the Quick Note page is hidden.
    pub quick_note_shortcut_hidden: bool,
    pub history_loading: bool,
    pub history_error: Option<String>,
    /// Set while a destructive action awaits confirmation (clear-all / delete).
    pub history_confirm: Option<HistoryConfirm>,
    /// In-app playback progress for one history entry.
    pub history_playback: Option<HistoryPlayback>,
    pub history_repolish_open: bool,
    pub history_repolish_running: bool,
    pub history_repolish_result: Option<(String, String)>,
    pub history_repolish_error: Option<String>,

    // Vocab
    pub vocab_entries: Vec<VocabEntry>,
    pub vocab_rules: Vec<CorrectionRule>,
    /// 纠正规则页的「只看自动收集」筛选（Tauri 的 `onlyLearnedRules`）。
    pub vocab_rules_only_learned: bool,
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
    pub style_voice_edit_prompt: String,
    pub style_tags: String,
    /// Editor-only fields (`author` / `version` / model hints) from the pack.
    pub style_author: String,
    pub style_version: String,
    pub style_model: String,
    pub style_compatible_version: String,
    /// Editor chrome: which pack is open, whether it is pristine, and the pills.
    pub style_editor_id: String,
    pub style_editor_builtin: bool,
    pub style_editor_active: bool,
    pub style_editor_mode: String,
    pub style_editor_dirty: bool,
    /// The stored pack the draft is compared against (dirty check + Revert).
    pub style_editor_saved: Option<openless_core::StylePack>,
    /// Live runtime directives of the draft (dictation workflow only).
    pub style_runtime: Option<StyleRuntimePreview>,
    pub style_notice: Option<String>,
    pub style_unsupported: bool,

    // Marketplace
    pub marketplace_query: String,
    pub marketplace_sort: MarketplaceSort,
    pub marketplace_packs: Vec<MarketplacePack>,
    pub marketplace_selected: Option<usize>,
    pub marketplace_detail_prompt: Option<String>,
    pub marketplace_mine_open: bool,
    pub marketplace_mine_query: String,
    pub marketplace_mine_packs: Vec<(String, String, Vec<String>)>,
    pub marketplace_notice: Option<String>,
    pub marketplace_loading: bool,
    /// Pack id currently being installed (the detail button shows 安装中…).
    pub marketplace_installing: Option<String>,

    pub marketplace_unsupported: bool,

    // Settings
    pub settings_section: SettingsSection,
    /// AI-services sub-tab (0 = LLM, 1 = ASR, 2 = local models, 3 = connections,
    /// 4 = 多模态). `ServiceView` decides which of these are shown.
    pub services_view: usize,
    /// Whether the host has an enabled channel for LLM / ASR (drives the
    /// required-service status dots on the AI-services tabs).
    pub service_configured: [bool; 2],
    /// The host has no local inference engine (Linux) → hide the local-model
    /// tab, exactly like the Tauri app gates on `supports_local_asr`.
    pub supports_local_asr: bool,
    /// Multimodal pipeline is enabled → the 多模态 view joins the tab strip.
    pub multimodal_view: bool,
    /// The host can self-update (AppImage) → beta-channel / check-update rows.
    /// The platform has a working desktop hotkey backend (fcitx5 listener up).
    /// Tauri's `visibleSettingsSections(supportsDesktopHotkey)` hides the
    /// 「快捷键」 section when there is none; the rail follows the same rule.
    /// Defaults to true so pure-rendering callers (and the common case where the
    /// host did start the listener) keep the section visible.
    pub hotkeys_supported: bool,
    /// 展开编辑菜单的快捷键行（`None` = 都收起）。
    pub shortcut_menu: Option<ShortcutField>,
    /// 正在录入按键的快捷键行（`None` = 未在录入）。
    pub shortcut_recording: Option<ShortcutField>,
    /// 「＋ 添加风格快捷键」草稿行是否打开。
    pub style_hotkey_draft_open: bool,
    /// 草稿行选中的风格包（`FrontendViewModel::style_packs` 索引）。
    pub style_hotkey_draft_pack: usize,
    /// Real host permission snapshot for 隐私与数据（不再写死「已授权」）。
    pub permissions: SettingsPermissions,
    /// 远程输入服务正在监听 → 显示配对码 / 访问网址 / 证书指纹。
    pub remote_running: bool,
    /// 服务报出的地址已过期：继续展示旧网址会诱导用户在错误的地址上配对，
    /// 因此与 Core 的 `RemoteInputStatus::urls_stale` 一起决定是否展示连接细节。
    pub remote_urls_stale: bool,
    pub remote_pin: String,
    pub remote_urls: Vec<String>,
    pub remote_cert_fingerprint: Option<String>,
    /// Channels of the active AI-services kind.
    pub channels: Vec<SettingsChannel>,
    /// Provider kinds offered by the add-channel form.
    pub channel_providers: Vec<SettingsChannelProvider>,
    pub channels_loading: bool,
    pub channel_form_open: bool,
    pub channel_form_name: String,
    pub channel_provider_index: usize,
    /// Open provider editor, or `None` when the channel list is the whole view.
    pub provider_editor: Option<SettingsProviderEditor>,
    /// Rail search query in the settings modal.
    pub settings_query: String,
    /// 录制中的裸修饰键挂起状态（egui 没有修饰键 Key 事件，只能跨帧判断）。
    pub shortcut_pending_modifier: Option<String>,
    /// Expanded drill-in row in the 实验与扩展 section (`usize::MAX` = none).
    pub advanced_open: usize,
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
    pub quick_note_hotkey: String,
    /// Display label for the translation modifier shortcut.
    pub translation_hotkey: String,
    /// Display label for the switch-style shortcut.
    pub switch_style_hotkey: String,
    /// Display label for the open-app shortcut.
    pub open_app_hotkey: String,
    /// Display label for the Less Computer voice shortcut.
    pub coding_agent_hotkey: String,
    /// Display label for the selection-polish shortcut.
    pub selection_polish_hotkey: String,
    /// Pipeline mode is 多模态 → the task strip hides the legacy LLM/ASR views.
    pub pipeline_multimodal: bool,
}

impl Default for FrontendViewModel {
    fn default() -> Self {
        Self {
            lang: Lang::ZhCn,
            theme_mode: openless_core::shared_types::ThemeMode::System,
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
            quick_note_recording: false,
            quick_note_shortcut_hidden: false,
            history_loading: true,
            history_error: None,
            history_confirm: None,
            history_playback: None,
            history_repolish_open: false,
            history_repolish_running: false,
            history_repolish_result: None,
            history_repolish_error: None,
            vocab_entries: Vec::new(),
            vocab_rules: Vec::new(),
            vocab_rules_only_learned: false,
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
            style_voice_edit_prompt: String::new(),
            style_tags: String::new(),
            style_author: String::new(),
            style_version: String::new(),
            style_model: String::new(),
            style_compatible_version: String::new(),
            style_editor_id: String::new(),
            style_editor_builtin: false,
            style_editor_active: false,
            style_editor_mode: String::new(),
            style_editor_dirty: false,
            style_editor_saved: None,
            style_runtime: None,
            style_notice: None,
            style_unsupported: true,
            marketplace_query: String::new(),
            marketplace_sort: MarketplaceSort::Popular,
            marketplace_packs: Vec::new(),
            marketplace_selected: None,
            marketplace_detail_prompt: None,
            marketplace_mine_open: false,
            marketplace_mine_query: String::new(),
            marketplace_mine_packs: Vec::new(),
            marketplace_notice: None,
            marketplace_loading: true,
            marketplace_installing: None,
            marketplace_unsupported: true,
            settings_section: SettingsSection::General,
            services_view: 0,
            channels: Vec::new(),
            channel_providers: Vec::new(),
            channels_loading: false,
            channel_form_open: false,
            channel_form_name: String::new(),
            channel_provider_index: 0,
            provider_editor: None,
            settings_query: String::new(),
            shortcut_pending_modifier: None,
            advanced_open: usize::MAX,
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
            selection_polish_hotkey: String::new(),
            pipeline_multimodal: false,
            remote_running: false,
            remote_urls_stale: false,
            remote_pin: String::new(),
            remote_urls: Vec::new(),
            remote_cert_fingerprint: None,
            service_configured: [false; 2],
            supports_local_asr: false,
            multimodal_view: false,
            hotkeys_supported: true,
            shortcut_menu: None,
            shortcut_recording: None,
            style_hotkey_draft_open: false,
            style_hotkey_draft_pack: 0,
            permissions: SettingsPermissions::default(),
            dictation_hotkey: String::new(),
            qa_hotkey: String::new(),
            quick_note_hotkey: String::new(),
            translation_hotkey: String::new(),
            switch_style_hotkey: String::new(),
            open_app_hotkey: String::new(),
            coding_agent_hotkey: String::new(),
        }
    }
}

/// Mirror of the egui-frontend `SettingsState` fields, but with no default
/// mock data. All values come from the host.
#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct SettingsFields {
    /// 0 = toggle, 1 = hold, 2 = double click, 3 = auto.
    pub recording_mode: usize,
    pub streaming_insert: bool,
    /// 稳定模式（先录音后识别）：上游 #1082 新增的偏好，录音期间不连接 ASR。
    pub stable_transcription: bool,
    pub streaming_save_clipboard: bool,
    pub restore_clipboard: bool,
    pub start_minimized: bool,
    pub launch_at_login: bool,
    pub auto_update: bool,
    pub remote_input: bool,
    pub remote_default_mode: usize,
    pub silence_auto_stop: bool,
    pub silence_seconds: usize,
    pub microphone_name: String,
    pub microphone_options: Vec<String>,
    /// 枚举麦克风失败时的原因（Tauri `microphoneLoadError` 行）。
    pub microphone_error: Option<String>,
    pub mute_while_recording: bool,
    pub audio_cue: bool,
    /// 录音胶囊开关与样式（Tauri `capsuleLabel` / `capsuleStyleLabel`）。
    pub show_capsule: bool,
    pub capsule_style: usize,
    pub record_audio_for_debug: bool,
    pub history_max_entries: String,
    /// 风格直达快捷键（可录制/停用/移除）。
    pub style_pack_hotkeys: Vec<StylePackHotkeyRow>,
    /// 润色上下文窗口分钟数（0 = 只用当前这条转写）。
    pub polish_context_window: String,
    /// 调试录音最多保留条数。
    pub audio_recording_max_entries: String,
    /// 选区润色交付方式：0 = 直接替换，1 = 预览确认。
    pub selection_polish_delivery: usize,
    pub activity_heatmap: bool,
    pub system_proxy: bool,
    pub less_computer: bool,
    /// Less Computer（Coding Agent）配置，全部直连 `coding_agent_*` 偏好。
    pub coding_agent_provider: usize,
    pub coding_agent_permission: usize,
    pub coding_agent_model: String,
    pub coding_agent_workdir: String,
    pub coding_agent_exe: String,
    pub multimodal: bool,
    pub language: usize,
    pub theme: usize,
    /// 历史保留天数（0 = 永久，输入框）。
    pub retention_days: String,
    pub remote_port: String,
}

impl Default for SettingsFields {
    fn default() -> Self {
        Self {
            recording_mode: 0,
            streaming_insert: false,
            stable_transcription: false,
            streaming_save_clipboard: false,
            restore_clipboard: false,
            start_minimized: false,
            launch_at_login: false,
            auto_update: false,
            remote_input: false,
            remote_default_mode: 0,
            silence_auto_stop: false,
            silence_seconds: 2,
            microphone_name: String::new(),
            microphone_options: Vec::new(),
            microphone_error: None,
            mute_while_recording: false,
            audio_cue: false,
            show_capsule: true,
            capsule_style: 0,
            record_audio_for_debug: false,
            history_max_entries: String::new(),
            style_pack_hotkeys: Vec::new(),
            polish_context_window: String::new(),
            audio_recording_max_entries: String::new(),
            selection_polish_delivery: 0,
            activity_heatmap: true,
            system_proxy: false,
            less_computer: false,
            coding_agent_provider: 0,
            coding_agent_permission: 0,
            coding_agent_model: String::new(),
            coding_agent_workdir: String::new(),
            coding_agent_exe: String::new(),
            multimodal: false,
            language: 0,
            theme: 0,
            retention_days: "0".to_string(),
            remote_port: String::new(),
        }
    }
}

/// `Lang` 定义在 `i18n.rs`（由同步脚本生成，禁止手改），所以按语言标签序列化，
/// 而不是给它加 serde derive。跨进程传视图模型时用得上。
mod lang_tag {
    use openless_linux_egui::Lang;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(lang: &Lang, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(lang.tag())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Lang, D::Error> {
        let tag = String::deserialize(deserializer)?;
        Lang::parse(&tag).ok_or_else(|| serde::de::Error::custom(format!("unknown lang tag {tag}")))
    }
}
