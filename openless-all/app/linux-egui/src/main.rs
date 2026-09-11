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
        drain_events, ensure_fcitx5_plugin_installed, fcitx5_copy_to_clipboard, notify,
        open_external, reload_running_fcitx5, write_jsonl, EventDrainOutcome, Fcitx5HotkeyListener,
        FcitxPluginInstallPlan, FcitxPluginStatus, HostToPopup, LinuxBackendBuilder,
        LinuxCapabilitySnapshot, LinuxLaunchIntent, LinuxNativeRuntime, LinuxPackageKind,
        LinuxResourceLayout, LinuxUpdateSupport, Notification, PopupActionGuard, PopupChatMessage,
        PopupKind, PopupState, PopupSupervisor, PopupSupervisorEvent, PopupToHost,
        SingleInstanceBroker, SingleInstanceRole, UpdateManifest, UpdateSchedule,
        POPUP_PROTOCOL_VERSION,
    };
    use openless_linux_egui::{
        fmt_l10n, load_locale_pref, save_locale_pref, tr_l10n, Lang, LocalePref, LANGS,
    };

    enum UiResult {
        HistoryTransform {
            generation: u64,
            id: String,
            repolish: bool,
            result: Result<String, String>,
        },
        Omni(Result<OmniEditor, String>),
        OmniModels(String, Result<Vec<String>, String>),
        CloudUi(openless_core::CloudSyncUiPreferences),
        CloudSync(Result<openless_core::CloudSyncStatus, String>),
        LocalModels(Result<Vec<openless_core::LocalAsrModel>, String>),
        LocalModelAction(Result<String, String>),
        Message(String),
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
        StyleSaved(Result<openless_core::StylePack, String>),
        SettingsSaved(Box<Result<openless_core::SettingsUpdateOutcome, String>>),
        Marketplace {
            generation: u64,
            result: Result<(Vec<openless_core::MarketplaceListItem>, Vec<String>), String>,
        },
        MarketplaceMutation(Result<String, String>),
        MarketplaceFlow(Result<openless_core::OAuthDeviceFlow, String>),
        MarketplaceAuthPoll(Result<openless_core::OAuthPollResult, String>),
        MarketplaceDetail(Result<openless_core::MarketplaceDetail, String>),
        MarketplaceMine(Result<(Vec<openless_core::MarketplaceMyPackItem>, Vec<String>), String>),
        Microphones(Result<Vec<openless_core::MicrophoneDevice>, String>),
        Overview(u64, Result<OverviewData, String>),
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
                merged.coding_agent_panel_hotkey = draft.coding_agent_panel_hotkey.clone();
                merged.coding_agent_quick_hotkey = draft.coding_agent_quick_hotkey.clone();
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
        duration_ms: Option<u64>,
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
        /// GitHub-style weekly columns (Sunday-first). Chronological oldest
        /// first; a partial leading week keeps left-edge calendar alignment.
        heatmap_weeks: Vec<[u32; 7]>,
        heatmap_days: u32,
        activity_days_total: usize,
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

    /// Trailing annual window rendered by the Overview heatmap.
    const OVERVIEW_HEATMAP_DAYS: i64 = 364;

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

    /// Build a Sunday-first weekly heatmap grid over the trailing `days` window
    /// (inclusive) ending at `today`. Columns are chronological weeks; the first
    /// column may be partial so weekday edges align like a GitHub contribution
    /// graph. Dates absent from the store render as an inactive (0) cell.
    fn build_heatmap_weeks(
        by_date: &std::collections::BTreeMap<chrono::NaiveDate, &openless_core::ActivityDay>,
        today: chrono::NaiveDate,
        days: i64,
    ) -> Vec<[u32; 7]> {
        let mut weeks: Vec<[u32; 7]> = Vec::new();
        let mut date = today - chrono::Duration::days(days - 1);
        while date <= today {
            let weekday = date.weekday().num_days_from_sunday() as usize;
            if weekday == 0 || weeks.is_empty() {
                // A fresh column. Sunday starts a new week; a partial leading
                // column is created on the first non-Sunday date instead.
                weeks.push([0u32; 7]);
            }
            weeks
                .last_mut()
                .expect("a heatmap week always exists before writing a cell")[weekday] =
                by_date.get(&date).map(|day| day.count).unwrap_or(0);
            date += chrono::Duration::days(1);
        }
        weeks
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
            heatmap_weeks: build_heatmap_weeks(&by_date, today, OVERVIEW_HEATMAP_DAYS),
            heatmap_days: OVERVIEW_HEATMAP_DAYS as u32,
            activity_days_total: by_date.len(),
        }
    }

    fn format_duration(ms: u64, lang: Lang) -> String {
        if ms < 1000 {
            fmt_l10n(lang, "dur.ms", &[&ms])
        } else if ms < 60_000 {
            fmt_l10n(lang, "dur.sec", &[&format!("{:.1}", ms as f64 / 1000.0)])
        } else {
            let minutes = ms / 60_000;
            let seconds = (ms % 60_000) / 1000;
            fmt_l10n(lang, "dur.min_sec", &[&minutes, &seconds])
        }
    }

    fn overview_provider_cards(ui: &mut egui::Ui, summary: &OverviewSummary, lang: Lang) {
        ui.columns(2, |columns| {
            overview_provider_card(
                &mut columns[0],
                tr_l10n(lang, "overview.provider_cards"),
                &summary.asr_provider,
                summary.asr_configured,
                lang,
            );
            overview_provider_card(
                &mut columns[1],
                tr_l10n(lang, "overview.provider_cards_llm"),
                &summary.llm_provider,
                summary.llm_configured,
                lang,
            );
        });
    }

    fn overview_provider_card(
        ui: &mut egui::Ui,
        kind: &str,
        provider: &str,
        configured: bool,
        lang: Lang,
    ) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(140.0);
            ui.label(egui::RichText::new(kind).weak());
            let name = if provider.is_empty() {
                tr_l10n(lang, "overview.not_set").to_string()
            } else {
                provider.to_string()
            };
            ui.label(egui::RichText::new(name).strong());
            if configured {
                ui.colored_label(
                    egui::Color32::from_rgb(60, 160, 90),
                    tr_l10n(lang, "overview.configured_dot"),
                );
            } else {
                ui.label(tr_l10n(lang, "overview.unconfigured"));
            }
        });
    }

    fn overview_metric(ui: &mut egui::Ui, label: &str, value: String, trend: &str) {
        ui.vertical(|ui| {
            ui.set_min_width(120.0);
            ui.label(egui::RichText::new(label).weak());
            ui.label(egui::RichText::new(value).strong().size(18.0));
            if !trend.is_empty() {
                ui.label(egui::RichText::new(trend).small().weak());
            }
        });
    }

    fn overview_metric_row(ui: &mut egui::Ui, summary: &OverviewSummary, lang: Lang) {
        let latency_trend = if summary.segments_today > 0 {
            "".to_string()
        } else {
            tr_l10n(lang, "metric.no_data_today").to_string()
        };
        egui::Grid::new("overview_metric_row")
            .num_columns(4)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                overview_metric(
                    ui,
                    tr_l10n(lang, "metric.chars_today"),
                    summary.chars_today.to_string(),
                    &fmt_l10n(lang, "metric.total_segments", &[&summary.segments_today]),
                );
                overview_metric(
                    ui,
                    tr_l10n(lang, "metric.duration_today"),
                    format_duration(summary.duration_ms_today, lang),
                    "",
                );
                overview_metric(
                    ui,
                    tr_l10n(lang, "metric.avg_latency"),
                    format_duration(summary.avg_latency_ms, lang),
                    &latency_trend,
                );
                overview_metric(
                    ui,
                    tr_l10n(lang, "metric.total"),
                    summary.history_total.to_string(),
                    &fmt_l10n(
                        lang,
                        "metric.near7",
                        &[&summary.last_7.segments, &summary.last_30.segments],
                    ),
                );
                ui.end_row();
            });
    }

    fn overview_recent(ui: &mut egui::Ui, summary: &OverviewSummary, lang: Lang) {
        ui.label(egui::RichText::new(tr_l10n(lang, "heading.recent")).strong());
        if summary.recent.is_empty() {
            ui.label(tr_l10n(lang, "overview.recent_empty"));
            return;
        }
        for entry in &summary.recent {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(format!(
                    "{} · {}",
                    entry.created_at,
                    format_duration(entry.duration_ms.unwrap_or(0), lang)
                ));
                let text = if entry.final_text.trim().is_empty() {
                    tr_l10n(lang, "overview.no_text").to_string()
                } else {
                    entry.final_text.clone()
                };
                ui.label(text);
            });
            ui.add_space(4.0);
        }
    }

    fn heat_color(count: u32) -> egui::Color32 {
        match count {
            0 => egui::Color32::from_gray(60),
            1..=2 => egui::Color32::from_rgb(80, 140, 220),
            3..=5 => egui::Color32::from_rgb(90, 120, 235),
            6..=10 => egui::Color32::from_rgb(110, 100, 235),
            _ => egui::Color32::from_rgb(150, 90, 235),
        }
    }

    fn overview_heatmap(ui: &mut egui::Ui, summary: &OverviewSummary, lang: Lang) {
        ui.label(egui::RichText::new(tr_l10n(lang, "overview.heatmap_title")).strong());
        let weeks = &summary.heatmap_weeks;
        if weeks.is_empty() {
            ui.label(tr_l10n(lang, "overview.heatmap_empty"));
            return;
        }
        let cell = 10.0f32;
        let gap = 2.0f32;
        let width = gap + weeks.len() as f32 * (cell + gap);
        let height = gap + 7.0f32 * (cell + gap);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter();
        for (column, week) in weeks.iter().enumerate() {
            for (row, count) in week.iter().enumerate() {
                let min = egui::pos2(
                    rect.left() + gap + column as f32 * (cell + gap),
                    rect.top() + gap + row as f32 * (cell + gap),
                );
                painter.rect_filled(
                    egui::Rect::from_min_size(min, egui::vec2(cell, cell)),
                    2.0,
                    heat_color(*count),
                );
            }
        }
        ui.horizontal(|ui| {
            ui.label(tr_l10n(lang, "overview.heatmap_less"));
            for count in [0u32, 1, 4, 8, 15] {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, heat_color(count));
            }
            ui.label(tr_l10n(lang, "overview.heatmap_more"));
            ui.label(fmt_l10n(
                lang,
                "overview.heatmap_footnote",
                &[&summary.heatmap_days, &summary.activity_days_total],
            ));
        });
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
        overview: OverviewState,
        overview_generation: std::sync::atomic::AtomicU64,
        microphones: Vec<openless_core::MicrophoneDevice>,
        playback: Arc<openless_linux_egui::RecordingPlayback>,
        history_confirmation: Option<Option<String>>,
        history_task: Option<tokio::task::JoinHandle<()>>,
        history_generation: u64,
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
        update_cancellation: Option<openless_linux_egui::UpdateCancellation>,
        restored_font_scale: Option<f32>,
        marketplace_items: Vec<openless_core::MarketplaceListItem>,
        marketplace_query: String,
        marketplace_ui: MarketplaceUi,
        marketplace_flow: Option<openless_core::OAuthDeviceFlow>,
        marketplace_detail: Option<openless_core::MarketplaceDetail>,
        marketplace_my_packs: Vec<openless_core::MarketplaceMyPackItem>,
        marketplace_my_likes: Vec<String>,
        style_editor: Option<openless_core::StylePack>,
        style_delete_pending: Option<String>,
        style_hotkey_pack_id: String,
        style_hotkey_primary: String,
        style_hotkey_modifiers: String,
        status: String,
        startup_error: Option<String>,
        locale_pref: LocalePref,
        lang: Lang,
        active_page: shell::Page,
        frontend_vm: FrontendViewModel,
        selection_voice_state: Option<openless_core::SelectionVoiceSnapshot>,
        less_computer_visible: bool,
        cloud_sync_status: Option<openless_core::CloudSyncStatus>,
        omni: Option<OmniEditor>,
        omni_loading: bool,
        omni_error: Option<String>,
        model_downloads: std::collections::HashMap<String, openless_core::LocalAsrDownloadProgress>,
        model_prepare: Option<openless_core::LocalAsrPrepareProgress>,
        local_models: Option<Vec<openless_core::LocalAsrModel>>,
        local_models_loading: bool,
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
                        overview: OverviewState::Loading,
                        overview_generation: std::sync::atomic::AtomicU64::new(0),
                        microphones: Vec::new(),
                        playback: Arc::default(),
                        history_confirmation: None,
                        history_task: None,
                        history_generation: 0,
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
                        update_cancellation: None,
                        restored_font_scale: None,
                        marketplace_items: Vec::new(),
                        marketplace_query: String::new(),
                        marketplace_ui: MarketplaceUi::default(),
                        marketplace_flow: None,
                        marketplace_detail: None,
                        marketplace_my_packs: Vec::new(),
                        marketplace_my_likes: Vec::new(),
                        style_editor: None,
                        style_delete_pending: None,
                        style_hotkey_pack_id: String::new(),
                        style_hotkey_primary: String::new(),
                        style_hotkey_modifiers: String::new(),
                        status: tr_l10n(lang, "status.core_started").to_string(),
                        startup_error: None,
                        locale_pref,
                        lang,
                        active_page: shell::Page::Overview,
                        frontend_vm: FrontendViewModel::default(),
                        selection_voice_state: None,
                        less_computer_visible: false,
                        cloud_sync_status: None,
                        omni: None,
                        omni_loading: false,
                        omni_error: None,
                        model_downloads: Default::default(),
                        model_prepare: None,
                        local_models: None,
                        local_models_loading: false,
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
                    preferences: None,
                    settings_dirty: SettingsDirty::default(),
                    overview: OverviewState::Loading,
                    overview_generation: std::sync::atomic::AtomicU64::new(0),
                    microphones: Vec::new(),
                    playback: Arc::default(),
                    history_confirmation: None,
                    history_task: None,
                    history_generation: 0,
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
                    update_cancellation: None,
                    restored_font_scale: None,
                    marketplace_items: Vec::new(),
                    marketplace_query: String::new(),
                    marketplace_ui: MarketplaceUi::default(),
                    marketplace_flow: None,
                    marketplace_detail: None,
                    marketplace_my_packs: Vec::new(),
                    marketplace_my_likes: Vec::new(),
                    style_editor: None,
                    style_delete_pending: None,
                    style_hotkey_pack_id: String::new(),
                    style_hotkey_primary: String::new(),
                    style_hotkey_modifiers: String::new(),
                    status: tr_l10n(lang, "status.startup_failed").to_string(),
                    startup_error: Some(error),
                    locale_pref,
                    lang,
                    active_page: shell::Page::Overview,
                    frontend_vm: FrontendViewModel::default(),
                    selection_voice_state: None,
                    less_computer_visible: false,
                    cloud_sync_status: None,
                    omni: None,
                    omni_loading: false,
                    omni_error: None,
                    model_downloads: Default::default(),
                    model_prepare: None,
                    local_models: None,
                    local_models_loading: false,
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
            openless_linux_egui::desktop_bridge::place_popup("OpenLess QA", 520, 520, false);
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
            openless_linux_egui::desktop_bridge::place_popup("OpenLess Preview", 480, 300, false);
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
            openless_linux_egui::desktop_bridge::place_popup("OpenLess Capsule", 340, 112, true);
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
                    PopupSupervisorEvent::Message(
                        PopupToHost::SubmitQa { .. }
                        | PopupToHost::ToggleQaRecording { .. }
                        | PopupToHost::DismissQa { .. },
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
            let generation = self
                .overview_generation
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                + 1;
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
                let _ = tx.send(UiResult::Overview(generation, result));
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
            let cancellation = openless_linux_egui::UpdateCancellation::default();
            self.update_cancellation = Some(cancellation.clone());
            let tx = self.tx.clone();
            self.tokio.spawn(async move {
                let progress_tx = tx.clone();
                let result = updater
                    .download_and_install_cancellable(
                        manifest,
                        move |progress| {
                            let _ = progress_tx.send(UiResult::UpdateProgress(progress));
                        },
                        cancellation,
                    )
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(UiResult::UpdateInstalled(result));
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
                BackendEventKind::LocalAsrDownloadProgress(progress) => {
                    use openless_core::LocalAsrDownloadPhase as Phase;
                    if matches!(
                        progress.phase,
                        Phase::Finished | Phase::Cancelled | Phase::Failed
                    ) {
                        self.local_models = None;
                    }
                    self.model_downloads
                        .insert(progress.model_id.clone(), progress);
                }
                BackendEventKind::LocalAsrPrepareProgress(progress) => {
                    self.model_prepare = Some(progress)
                }
                BackendEventKind::SelectionVoiceStateChanged(snapshot) => {
                    self.selection_voice_state = Some(snapshot)
                }
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
                                    backend.stop_dictation_session(request.session_id).await?;
                                }
                                openless_core::RecordingControlAction::Cancel => {
                                    backend.cancel_dictation(Some(request.session_id)).await?;
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
                BackendEventKind::HistoryChanged(_) => self.load_overview(),
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
                        HostAction::ShowLessComputer => self.less_computer_visible = true,
                        HostAction::ShowMain => {
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
                        fmt_l10n(lang, "status.backlog_reset", &[&dropped])
                    } else {
                        fmt_l10n(lang, "status.backlog_replay", &[&dropped])
                    };
                }
            }
            while let Ok(result) = self.rx.try_recv() {
                match result {
                    UiResult::HistoryTransform {
                        generation,
                        id,
                        repolish,
                        result,
                    } => {
                        if generation != self.history_generation {
                            continue;
                        }
                        self.history_task = None;
                        self.frontend_vm.history_busy = false;
                        match result {
                            Ok(text) => {
                                if repolish {
                                    self.frontend_vm
                                        .history_results
                                        .entry(id)
                                        .or_default()
                                        .insert(0, text);
                                }
                                self.status = "历史操作完成".into();
                            }
                            Err(error) => self.status = error,
                        }
                    }
                    UiResult::Omni(result) => {
                        self.omni_loading = false;
                        match result {
                            Ok(editor) => self.omni = Some(editor),
                            Err(error) => {
                                self.omni_error = Some(error.clone());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::OmniModels(provider, result) => match result {
                        Ok(models) => {
                            if let Some(editor) =
                                self.omni.as_mut().filter(|e| e.provider == provider)
                            {
                                editor.models = models;
                            }
                        }
                        Err(error) => self.status = error,
                    },
                    UiResult::CloudUi(preferences) => {
                        if let Some(locale) = preferences.locale {
                            if let Ok(serde_json::Value::String(tag)) = serde_json::to_value(locale)
                            {
                                self.locale_pref = LocalePref::from_tag(&tag);
                                self.lang = self.locale_pref.resolve();
                                if let Err(error) =
                                    openless_linux_egui::save_locale_pref(self.locale_pref)
                                {
                                    self.status = error.to_string();
                                }
                            }
                        }
                        if let Some(scale) = preferences.font_scale {
                            let size = match scale {
                                openless_core::SyncFontScale::Small => 0.9,
                                openless_core::SyncFontScale::Medium => 1.0,
                                openless_core::SyncFontScale::Large => 1.15,
                            };
                            self.restored_font_scale = Some(size);
                            if let Err(error) = openless_linux_egui::save_ui_value(
                                "fontScale",
                                serde_json::json!(size),
                            ) {
                                self.status = error.to_string();
                            }
                        }
                    }
                    UiResult::CloudSync(Ok(status)) => {
                        self.cloud_sync_status = Some(status);
                        self.load_library();
                        self.status = "云同步操作完成".into();
                    }
                    UiResult::CloudSync(Err(error)) => self.status = error,
                    UiResult::LocalModels(result) => {
                        self.local_models_loading = false;
                        match result {
                            Ok(models) => self.local_models = Some(models),
                            Err(error) => {
                                self.local_models = Some(Vec::new());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::LocalModelAction(result) => {
                        self.status = result.unwrap_or_else(|e| e);
                        self.local_models = None;
                        self.local_models_loading = false;
                    }
                    UiResult::Message(message) => self.status = message,
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
                    UiResult::StyleSaved(result) => {
                        self.frontend_vm.style_saving = false;
                        match result {
                            Ok(pack) => {
                                self.status = format!("{} 已保存", pack.name);
                                self.frontend_vm.style_editor_open = false;
                                self.style_editor = None;
                                self.load_library();
                            }
                            Err(error) => {
                                self.frontend_vm.style_notice = Some(error.clone());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::SettingsSaved(result) => match *result {
                        Ok(outcome) => {
                            self.preferences = Some(outcome.preferences.clone());
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
                        Err(error) => {
                            self.status = error;
                            if let Some(backend) = self.backend() {
                                self.preferences = Some(backend.get_preferences());
                            }
                            self.settings_dirty = SettingsDirty::default();
                        }
                    },
                    UiResult::Marketplace { generation, result } => {
                        if generation != self.marketplace_ui.generation {
                            continue;
                        }
                        self.marketplace_ui.loading = false;
                        match result {
                            Ok((items, likes)) => {
                                self.marketplace_items = items;
                                self.marketplace_my_likes = likes;
                                self.frontend_vm.marketplace_notice = None;
                            }
                            Err(error) => {
                                self.frontend_vm.marketplace_notice = Some(error.clone());
                                self.status = error;
                            }
                        }
                    }
                    UiResult::MarketplaceMutation(result) => {
                        self.marketplace_ui.busy = false;
                        self.status = result.unwrap_or_else(|e| e);
                        self.load_marketplace();
                        if self.marketplace_ui.mine_open {
                            self.load_marketplace_mine();
                        }
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
                        if self
                            .frontend_vm
                            .marketplace_selected
                            .and_then(|i| self.marketplace_items.get(i))
                            .is_some_and(|p| p.id == detail.summary.id)
                        {
                            self.frontend_vm.marketplace_prompt = Some(detail.prompt.clone());
                            self.marketplace_detail = Some(detail);
                        }
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
                    UiResult::Overview(generation, Ok(data)) => {
                        if generation
                            == self
                                .overview_generation
                                .load(std::sync::atomic::Ordering::Acquire)
                        {
                            self.overview = OverviewState::Loaded(data);
                        }
                    }
                    UiResult::Overview(generation, Err(error)) => {
                        if generation
                            != self
                                .overview_generation
                                .load(std::sync::atomic::Ordering::Acquire)
                        {
                            continue;
                        }
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
                        self.update_cancellation = None;
                        self.update_busy = false;
                        self.update_manifest = None;
                        self.status =
                            fmt_l10n(lang, "update.installed_restart", &[&installed.version]);
                    }
                    UiResult::UpdateInstalled(Err(error)) => {
                        self.update_cancellation = None;
                        self.update_busy = false;
                        self.status = fmt_l10n(lang, "update.install_failed", &[&error]);
                    }
                }
            }
            if let Some(backend) = self.backend() {
                self.snapshot = Some(backend.snapshot());
            }
        }

        fn less_computer_ui(&mut self, ui: &mut egui::Ui) {
            let lang = self.lang;
            ui.heading("Less Computer");
            ui.text_edit_multiline(&mut self.less_computer_input);
            ui.horizontal(|ui| {
                if ui.button(tr_l10n(lang, "btn.run")).clicked()
                    && !self.less_computer_input.trim().is_empty()
                {
                    if let Some(backend) = self.backend() {
                        let prompt = self.less_computer_input.clone();
                        self.less_computer_output.clear();
                        self.spawn(async move {
                            backend.submit_less_computer(prompt).await?;
                            Ok(tr_l10n(lang, "less_computer.done").to_string())
                        });
                    }
                }
                if ui.button(tr_l10n(lang, "btn.cancel")).clicked() {
                    if let Some(backend) = self.backend() {
                        self.spawn(async move {
                            backend.cancel_less_computer(None).await?;
                            Ok(tr_l10n(lang, "less_computer.cancelled").to_string())
                        });
                    }
                }
            });
            if let Some((token, command)) = self.pending_approval.clone() {
                ui.label(fmt_l10n(lang, "approval.request_run", &[&command]));
                ui.horizontal(|ui| {
                    for (label, approved) in [
                        (tr_l10n(lang, "btn.allow"), true),
                        (tr_l10n(lang, "btn.deny"), false),
                    ] {
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
                                    Ok(tr_l10n(lang, "approval.submitted").to_string())
                                });
                            }
                        }
                    }
                });
            }
            ui.label(if self.less_computer_output.is_empty() {
                tr_l10n(lang, "less_computer.no_output")
            } else {
                &self.less_computer_output
            });
        }

        fn provider_management_ui(&mut self, ui: &mut egui::Ui) {
            let lang = self.lang;
            ui.horizontal(|ui| {
                ui.strong(tr_l10n(lang, "providers.credentials"));
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
                if ui.button(tr_l10n(lang, "btn.refresh_channel")).clicked() {
                    self.providers = ProvidersState::Loading;
                    self.load_providers(self.provider_kind);
                }
            });

            let panel = match self.providers.clone() {
                ProvidersState::Loading => {
                    ui.label(tr_l10n(lang, "providers.loading_dir"));
                    return;
                }
                ProvidersState::Failed(error) => {
                    ui.colored_label(egui::Color32::RED, error);
                    return;
                }
                ProvidersState::Loaded(panel) => panel,
            };

            ui.group(|ui| {
                ui.label(tr_l10n(lang, "btn.new_channel"));
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("new-provider-type")
                        .selected_text(
                            panel
                                .descriptors
                                .iter()
                                .find(|item| item.provider_type.as_str() == self.new_provider_type)
                                .map(provider_descriptor_label)
                                .unwrap_or_else(|| {
                                    tr_l10n(lang, "lbl.choose_provider").to_string()
                                }),
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
                            egui::Button::new(tr_l10n(lang, "btn.create")),
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
                                Ok(tr_l10n(lang, "status.channel_created").to_string())
                            });
                        }
                    }
                });
                ui.small(tr_l10n(lang, "providers.core_note"));
            });

            if panel.channels.is_empty() {
                ui.label(tr_l10n(lang, "providers.empty"));
                return;
            }

            for (index, channel) in panel.channels.iter().enumerate() {
                let active = channel.id == panel.active_provider;
                ui.horizontal(|ui| {
                    let selected = self.selected_channel_id.as_deref() == Some(channel.id.as_str());
                    let active_suffix = if active {
                        tr_l10n(lang, "btn.status_active")
                    } else {
                        ""
                    };
                    let disabled_suffix = if channel.enabled {
                        ""
                    } else {
                        tr_l10n(lang, "btn.status_disabled")
                    };
                    if ui
                        .selectable_label(
                            selected,
                            format!(
                                "{} · {}{active_suffix}{disabled_suffix}",
                                channel.name, channel.provider_type,
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
                    if !active
                        && channel.enabled
                        && ui.button(tr_l10n(lang, "btn.set_active")).clicked()
                    {
                        if let Some(backend) = self.backend() {
                            let slot = provider_slot(panel.kind);
                            let channel_id = channel.id.clone();
                            self.spawn_provider_mutation(async move {
                                backend.set_active_provider(slot, channel_id).await?;
                                Ok(tr_l10n(lang, "status.channel_active").to_string())
                            });
                        }
                    }
                    if ui
                        .button(if channel.enabled {
                            tr_l10n(lang, "btn.disable")
                        } else {
                            tr_l10n(lang, "btn.enable")
                        })
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
                                Ok(tr_l10n(lang, "status.channel_enabled").to_string())
                            });
                        }
                    }
                    if index > 0 && ui.button(tr_l10n(lang, "btn.move_up")).clicked() {
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
                                Ok(tr_l10n(lang, "status.channel_reordered").to_string())
                            });
                        }
                    }
                    if index + 1 < panel.channels.len()
                        && ui.button(tr_l10n(lang, "btn.move_down")).clicked()
                    {
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
                                Ok(tr_l10n(lang, "status.channel_reordered").to_string())
                            });
                        }
                    }
                    if self.pending_channel_delete.as_deref() == Some(channel.id.as_str()) {
                        if ui.button(tr_l10n(lang, "btn.confirm_delete")).clicked() {
                            self.pending_channel_delete = None;
                            if let Some(backend) = self.backend() {
                                let kind = panel.kind;
                                let channel_id = channel.id.clone();
                                self.spawn_provider_mutation(async move {
                                    backend.delete_channel(kind, channel_id).await?;
                                    Ok(tr_l10n(lang, "status.channel_deleted").to_string())
                                });
                            }
                        }
                        if ui.button(tr_l10n(lang, "btn.cancel_delete")).clicked() {
                            self.pending_channel_delete = None;
                        }
                    } else if ui.button(tr_l10n(lang, "btn.delete")).clicked() {
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
                    ui.label(fmt_l10n(
                        lang,
                        "providers.reading_channel",
                        &[&format!("{kind:?}"), &channel_id],
                    ));
                }
                ProviderEditorState::Failed(error) => {
                    ui.colored_label(egui::Color32::RED, error);
                }
                ProviderEditorState::Loaded(editor) => {
                    let mut editor = *editor;
                    ui.separator();
                    ui.strong(fmt_l10n(lang, "providers.editing", &[&editor.channel.id]));
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
                                Ok(tr_l10n(lang, "status.provider_type_updated").to_string())
                            });
                        }
                        return;
                    }

                    ui.label(fmt_l10n(
                        lang,
                        "providers.auth_probe",
                        &[
                            &auth_requirement_label(lang, editor.descriptor.auth_requirement),
                            &format!("{:?}", editor.descriptor.validation_probe),
                        ],
                    ));
                    ui.horizontal(|ui| {
                        ui.label(tr_l10n(lang, "providers.name"));
                        ui.text_edit_singleline(&mut editor.name);
                    });
                    provider_fields_ui(ui, lang, &mut editor);

                    ui.horizontal(|ui| {
                        if ui.button(tr_l10n(lang, "btn.save_fields")).clicked() {
                            if let Some(backend) = self.backend() {
                                let saved = editor.clone();
                                self.spawn_provider_mutation(async move {
                                    save_provider_editor(backend, saved).await?;
                                    Ok(tr_l10n(lang, "status.channel_saved").to_string())
                                });
                            }
                        }
                        if ui.button(tr_l10n(lang, "btn.clear_secret")).clicked() {
                            if let Some(backend) = self.backend() {
                                let cleared = editor.clone();
                                self.spawn_provider_mutation(async move {
                                    clear_provider_secrets(backend, &cleared).await?;
                                    Ok(tr_l10n(lang, "status.secret_cleared").to_string())
                                });
                            }
                        }
                        if ui.button(tr_l10n(lang, "btn.validate")).clicked() {
                            if let Some(backend) = self.backend() {
                                let kind = editor.kind;
                                let channel_id = editor.channel.id.clone();
                                self.spawn_provider_mutation(async move {
                                    validate_provider_channel(lang, backend, kind, channel_id).await
                                });
                            }
                        }
                        if ui.button(tr_l10n(lang, "btn.list_models")).clicked() {
                            self.provider_models.clear();
                            self.request_provider_models(editor.kind, editor.channel.id.clone());
                        }
                    });
                    if !self.provider_models.is_empty() {
                        ui.label(tr_l10n(lang, "providers.model_list"));
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
        fn language_selector_ui(&mut self, ui: &mut egui::Ui) {
            let mut chosen: Option<LocalePref> = None;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tr_l10n(self.lang, "settings.language")).strong());
                let pref = self.locale_pref;
                let lang = self.lang;
                let selected = match pref {
                    LocalePref::System => {
                        tr_l10n(lang, "settings.language_follow_system").to_string()
                    }
                    LocalePref::Lang(explicit) => {
                        tr_l10n(explicit, locale_key(explicit)).to_string()
                    }
                };
                egui::ComboBox::from_id_salt("openless-ui-language")
                    .width(240.0)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                pref == LocalePref::System,
                                tr_l10n(lang, "settings.language_follow_system"),
                            )
                            .clicked()
                        {
                            chosen = Some(LocalePref::System);
                        }
                        for option in LANGS {
                            let native_label = tr_l10n(option, locale_key(option));
                            if ui
                                .selectable_label(pref == LocalePref::Lang(option), native_label)
                                .clicked()
                            {
                                chosen = Some(LocalePref::Lang(option));
                            }
                        }
                    });
            });
            if let Some(pref) = chosen {
                self.apply_locale_pref(pref);
                self.status = tr_l10n(self.lang, "settings.locale_saved").to_string();
                ui.ctx().request_repaint();
            }
        }

        fn sync_view_model(&mut self) {
            // Capture overview error before taking a mutable borrow on frontend_vm.
            let overview_err = self.overview_error();
            let backend = self.backend();
            let lang = self.lang;

            let vm = &mut self.frontend_vm;
            vm.pending_corrections = backend
                .as_ref()
                .map(|b| b.pending_corrections())
                .unwrap_or_default();

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
            vm.history_audio_playing = self.playback.is_playing();
            vm.version = env!("CARGO_PKG_VERSION").to_string();

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
                            duration_ms: entry.duration_ms,
                        })
                        .collect(),
                    last_7_segments: summary.last_7.segments,
                    last_30_segments: summary.last_30.segments,
                    heatmap_weeks: summary.heatmap_weeks,
                    heatmap_days: summary.heatmap_days,
                    activity_days_total: summary.activity_days_total,
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
                s.remote_port = prefs.remote_input_port.to_string();
                s.activity_heatmap = prefs.show_overview_activity_heatmap;
                s.theme = match prefs.theme_mode {
                    openless_core::shared_types::ThemeMode::System => 0,
                    openless_core::shared_types::ThemeMode::Light => 1,
                    openless_core::shared_types::ThemeMode::Dark => 2,
                };
                s.recording_enabled = true;
                s.realtime_mode = matches!(
                    prefs.hotkey.mode,
                    openless_core::shared_types::HotkeyMode::Hold
                );
                s.restore_clipboard = true;
                s.remember_history = true;
                s.local_model = true;
                s.selection_voice = prefs.selection_voice_enabled;
                s.restore_clipboard = prefs.restore_clipboard_after_paste;
                vm.qa_save_history = prefs.qa_save_history;
                vm.translation_target_language = prefs.translation_target_language.clone();
                vm.translation_working_languages = prefs.working_languages.clone();
                vm.selection_unsupported = false;
                vm.translation_unsupported = false;
            }

            // History: wire from Core when backend is available.
            if let OverviewState::Loaded(data) = &self.overview {
                {
                    let history = data.history.clone();
                    vm.history_entries = history
                        .into_iter()
                        .map(|item| frontend::view_model::HistoryEntry {
                            id: item.id.clone(),
                            time: item.created_at.clone(),
                            raw: item.raw_transcript.clone(),
                            mode: format!("{:?}", item.mode),
                            has_audio: item.has_audio_recording.unwrap_or(false),
                            asr: item.asr_provider.clone().unwrap_or_default(),
                            llm: item.llm_provider.clone().unwrap_or_default(),
                            asr_ms: item.asr_ms,
                            polish_ms: item.polish_ms,
                            text: item.final_text,
                            duration: item
                                .duration_ms
                                .map(|d| format_duration(d, lang))
                                .unwrap_or_default(),
                            tag: match item.insert_status {
                                HistoryInsertStatus::Inserted => "已插入",
                                HistoryInsertStatus::CopiedFallback => "已复制",
                                HistoryInsertStatus::PasteSent => "已发送",
                                HistoryInsertStatus::Failed => "失败",
                                HistoryInsertStatus::NotRequested => "未请求",
                            }
                            .to_string(),
                        })
                        .collect();
                }
            }

            // Vocabulary: wire from existing data.
            {
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
            }

            // Correction rules: wire from existing data.
            {
                vm.vocab_unsupported = false;
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
            }
            vm.vocab_saved_presets = self
                .vocab_presets
                .iter()
                .map(|preset| frontend::view_model::SavedVocabPreset {
                    id: preset.id.clone(),
                    name: preset.name.clone(),
                    phrases: preset.phrases.join("、"),
                })
                .collect();

            // Style packs: wire from existing data.
            {
                vm.style_unsupported = false;
                vm.style_packs = self
                    .style_packs
                    .iter()
                    .map(|pack| frontend::view_model::StylePack {
                        id: pack.id.clone(),
                        name: pack.name.clone(),
                        description: pack.description.clone(),
                        tags: vec![pack.base_mode.display_name().to_string()],
                        accent: theme::blue(),
                        is_builtin: pack.kind == openless_core::StylePackKind::Builtin,
                        is_active: if vm.style_selection_workflow {
                            self.preferences
                                .as_ref()
                                .is_some_and(|p| p.selection_polish_style_pack_id == pack.id)
                        } else {
                            pack.active
                        },
                    })
                    .collect();
            }

            vm.style_selected = vm
                .style_packs
                .iter()
                .position(|p| p.is_active)
                .unwrap_or(usize::MAX);
            // Marketplace: wire from Core data when available.
            {
                vm.marketplace_loading = self.marketplace_ui.loading;
                vm.marketplace_query = self.marketplace_query.clone();
                vm.marketplace_liked = self
                    .marketplace_items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| self.marketplace_my_likes.contains(&p.id).then_some(i))
                    .collect();
                vm.marketplace_unsupported = false;
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
                        is_new: false,
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
                frontend::view_model::SettingsField::RealtimeMode => {
                    preferences.hotkey.mode = match preferences.hotkey.mode {
                        openless_core::shared_types::HotkeyMode::Hold => {
                            openless_core::shared_types::HotkeyMode::Toggle
                        }
                        _ => openless_core::shared_types::HotkeyMode::Hold,
                    };
                    self.settings_dirty.recording = true;
                }
                frontend::view_model::SettingsField::RecordingEnabled => {
                    self.settings_dirty.recording = true;
                    // Toggle recording enabled state — no-op on preferences directly,
                    // but marks dirty so save will apply.
                }
                frontend::view_model::SettingsField::RestoreClipboard
                | frontend::view_model::SettingsField::StackedLayout
                | frontend::view_model::SettingsField::ConservativeLayout
                | frontend::view_model::SettingsField::SystemProxy
                | frontend::view_model::SettingsField::RememberHistory
                | frontend::view_model::SettingsField::RecordAudio
                | frontend::view_model::SettingsField::LessComputer
                | frontend::view_model::SettingsField::Multimodal
                | frontend::view_model::SettingsField::BetaChannel => {
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
                }
                frontend::view_model::SettingsField::SelectionAssistant => {
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
                }
                frontend::view_model::SettingsField::SelectionVoice => {
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
                }
                frontend::view_model::SettingsField::LocalModel => {
                    // Linux does not support local model inference.
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
                }
                frontend::view_model::SettingsField::MarketplaceEnabled => {
                    self.frontend_vm.marketplace_unsupported =
                        !self.frontend_vm.marketplace_unsupported;
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
                frontend::view_model::SettingsComboField::Provider
                | frontend::view_model::SettingsComboField::Retention
                | frontend::view_model::SettingsComboField::Microphone
                | frontend::view_model::SettingsComboField::RecordingMode => {
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
                }
            }
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
                frontend::view_model::SettingsTextField::ApiKey => {
                    self.frontend_vm.settings.api_key = text;
                }
                frontend::view_model::SettingsTextField::Endpoint => {
                    self.frontend_vm.settings.endpoint = text;
                }
                frontend::view_model::SettingsTextField::Model => {
                    self.frontend_vm.settings.model = text;
                }
                frontend::view_model::SettingsTextField::ClaudePrompt => {
                    self.frontend_vm.settings.claude_prompt = text;
                }
            }
            self.save_settings_if_dirty();
        }

        /// Apply a settings action button from the frontend.
        fn apply_settings_action(&mut self, field: frontend::view_model::SettingsActionField) {
            match field {
                frontend::view_model::SettingsActionField::ConnectionTest => {
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
                }
                frontend::view_model::SettingsActionField::ClearHistory => {
                    if let Some(backend) = self.backend() {
                        let lang = self.lang;
                        self.spawn(async move {
                            backend.clear_history()?;
                            Ok(tr_l10n(lang, "status.history_cleared").to_string())
                        });
                    }
                }
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
                frontend::view_model::SettingsActionField::CheckUpdate => {
                    let channel = self
                        .preferences
                        .as_ref()
                        .map(|prefs| prefs.update_channel)
                        .unwrap_or_default();
                    self.request_update_check(channel);
                }
                frontend::view_model::SettingsActionField::OpenGitHub => {
                    let _ = open_external("https://github.com/Open-Less/openless");
                }
                frontend::view_model::SettingsActionField::OpenHelp => {
                    let _ = open_external("https://github.com/Open-Less/openless");
                }
                frontend::view_model::SettingsActionField::OpenReleaseNotes => {
                    let _ = open_external("https://github.com/Open-Less/openless/releases");
                }
                frontend::view_model::SettingsActionField::OpenFeedback => {
                    let _ = open_external("https://github.com/Open-Less/openless/issues");
                }
                frontend::view_model::SettingsActionField::CopyQQ => {
                    match fcitx5_copy_to_clipboard("1078960553") {
                        Ok(()) => {
                            self.frontend_vm.settings_notice =
                                Some(tr_l10n(self.lang, "status.copied").to_string());
                        }
                        Err(error) => {
                            self.frontend_vm.settings_notice = Some(format!("复制失败: {error}"));
                        }
                    }
                }
                frontend::view_model::SettingsActionField::ModelManagement
                | frontend::view_model::SettingsActionField::ExtensionManagement
                | frontend::view_model::SettingsActionField::Permissions
                | frontend::view_model::SettingsActionField::ClaudeDetect
                | frontend::view_model::SettingsActionField::ClaudeConsole
                | frontend::view_model::SettingsActionField::ClaudeRunTest => {
                    self.frontend_vm.settings_notice =
                        Some(tr_l10n(self.lang, "settings.unsupported_linux").to_string());
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
                    frontend::view_model::FrontendAction::AcceptCorrection(id) => {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                backend.accept_pending_correction(&id)?;
                                Ok("已接受纠错建议".into())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::RejectCorrection(id) => {
                        if let Some(backend) = self.backend() {
                            backend.reject_pending_correction(&id);
                        }
                    }
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
                    }
                    frontend::view_model::FrontendAction::ToggleSettings => {
                        self.frontend_vm.settings_open = !self.frontend_vm.settings_open;
                    }
                    frontend::view_model::FrontendAction::CloseSettings => {
                        self.frontend_vm.settings_open = false;
                    }
                    frontend::view_model::FrontendAction::SidebarToggleStyle => {
                        self.frontend_vm.style_open = !self.frontend_vm.style_open;
                    }
                    frontend::view_model::FrontendAction::SidebarToggleTools => {
                        self.frontend_vm.tools_open = !self.frontend_vm.tools_open;
                    }
                    frontend::view_model::FrontendAction::WindowClose => {
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
                        self.marketplace_ui.mine_open = true;
                        self.load_marketplace_mine();
                    }
                    frontend::view_model::FrontendAction::MarketplaceSearch(query) => {
                        self.marketplace_query = query;
                        self.marketplace_ui.search_due =
                            Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
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
                            self.mutate_marketplace(MarketplaceMutation::Like(item.id.clone()));
                        }
                    }
                    frontend::view_model::FrontendAction::MarketplaceSort(sort) => {
                        self.frontend_vm.marketplace_sort = sort;
                        self.load_marketplace();
                    }
                    frontend::view_model::FrontendAction::HistoryClear => {
                        self.history_confirmation = Some(None)
                    }
                    frontend::view_model::FrontendAction::HistoryRefresh => {
                        self.frontend_vm.history_cleared = false;
                        self.load_overview();
                    }
                    frontend::view_model::FrontendAction::HistorySearch(query) => {
                        self.frontend_vm.history_query = query;
                    }
                    frontend::view_model::FrontendAction::HistoryFilter(index) => {
                        self.frontend_vm.history_filter = index;
                    }
                    frontend::view_model::FrontendAction::HistorySelect(index) => {
                        self.frontend_vm.history_selected = index;
                    }
                    frontend::view_model::FrontendAction::HistoryDelete(index) => {
                        self.history_confirmation = self
                            .frontend_vm
                            .history_entries
                            .get(index)
                            .map(|e| Some(e.id.clone()));
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
                    frontend::view_model::FrontendAction::HistoryRepolish => {
                        self.history_transform(false)
                    }
                    frontend::view_model::FrontendAction::HistoryRetranscribe => {
                        self.history_transform(true)
                    }
                    frontend::view_model::FrontendAction::HistoryCancel => {
                        self.history_generation += 1;
                        if let Some(task) = self.history_task.take() {
                            task.abort();
                        }
                        self.frontend_vm.history_busy = false;
                        self.status = "已取消".into();
                    }
                    frontend::view_model::FrontendAction::HistoryTogglePlay => {
                        let playback = self.playback.clone();
                        if let (Some(backend), Some(entry)) = (
                            self.backend(),
                            self.frontend_vm
                                .history_entries
                                .get(self.frontend_vm.history_selected),
                        ) {
                            let directory = backend.config().data_dir.clone();
                            let id = entry.id.clone();
                            self.spawn(async move {
                                tokio::task::spawn_blocking(move || {
                                    if playback.is_playing() {
                                        playback.stop();
                                        Ok("播放已停止".into())
                                    } else {
                                        playback
                                            .play(&directory, &id)
                                            .map(|_| "正在播放录音".into())
                                            .map_err(|e| {
                                                BackendError::new(
                                                    openless_core::BackendErrorCode::Platform,
                                                    e,
                                                )
                                            })
                                    }
                                })
                                .await
                                .map_err(|e| {
                                    BackendError::new(
                                        openless_core::BackendErrorCode::Platform,
                                        e.to_string(),
                                    )
                                })?
                            });
                        }
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
                    frontend::view_model::FrontendAction::VocabRefresh => self.load_library(),
                    frontend::view_model::FrontendAction::VocabApplyPreset(index) => {
                        if index != usize::MAX {
                            let selected = &mut self.frontend_vm.vocab_selected_presets;
                            if selected.contains(&index) {
                                selected.retain(|i| *i != index);
                            } else {
                                selected.push(index);
                            }
                        } else if let Some(backend) = self.backend() {
                            let presets: Vec<_> = self
                                .frontend_vm
                                .vocab_selected_presets
                                .iter()
                                .filter_map(|i| self.vocab_presets.get(*i))
                                .cloned()
                                .collect();
                            self.spawn(async move {
                                for preset in presets {
                                    for phrase in preset.phrases {
                                        backend
                                            .add_vocabulary(phrase, Some(preset.name.clone()))?;
                                    }
                                }
                                Ok("词汇预设已应用".into())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::VocabCreatePreset {
                        id,
                        name,
                        phrases,
                    } => {
                        if let Some(backend) = self.backend() {
                            self.spawn(async move {
                                let mut phrases: Vec<String> = phrases
                                    .split([',', '，', '、', '\n'])
                                    .map(str::trim)
                                    .filter(|p| !p.is_empty())
                                    .map(ToOwned::to_owned)
                                    .collect();
                                phrases.sort();
                                phrases.dedup();
                                let mut store = backend.list_vocabulary_presets()?;
                                let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                                let preset = openless_core::VocabPreset {
                                    id: id.clone(),
                                    name: name.trim().to_owned(),
                                    phrases,
                                };
                                let list = if openless_core::builtin_vocab_presets()
                                    .iter()
                                    .any(|p| p.id == id)
                                {
                                    &mut store.overrides
                                } else {
                                    &mut store.custom
                                };
                                list.retain(|p| p.id != id);
                                list.push(preset);
                                backend.save_vocabulary_presets(&store)?;
                                Ok("预设已保存".into())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::VocabDeletePreset(index) => {
                        if let (Some(backend), Some(preset)) =
                            (self.backend(), self.vocab_presets.get(index))
                        {
                            let id = preset.id.clone();
                            self.spawn(async move {
                                let mut store = backend.list_vocabulary_presets()?;
                                store.custom.retain(|p| p.id != id);
                                store.overrides.retain(|p| p.id != id);
                                if openless_core::builtin_vocab_presets()
                                    .iter()
                                    .any(|p| p.id == id)
                                    && !store.disabled_builtin_preset_ids.contains(&id)
                                {
                                    store.disabled_builtin_preset_ids.push(id);
                                }
                                backend.save_vocabulary_presets(&store)?;
                                Ok("预设已删除".into())
                            });
                        }
                    }
                    frontend::view_model::FrontendAction::StyleActivate(index) => {
                        self.activate_style_v2(index)
                    }
                    frontend::view_model::FrontendAction::StyleRefresh => self.load_library(),
                    frontend::view_model::FrontendAction::StyleReset => self.reset_style_v2(),
                    frontend::view_model::FrontendAction::StyleDelete => {
                        if let Some(pack) = &self.style_editor {
                            self.style_delete_pending = Some(pack.id.clone());
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
                            self.open_style_v2(pack);
                        }
                    }
                    frontend::view_model::FrontendAction::StyleSaveEditor(prompt) => {
                        self.save_style_v2(prompt)
                    }
                    frontend::view_model::FrontendAction::StyleCloseEditor => {
                        self.style_editor = None;
                        self.frontend_vm.style_editor_open = false;
                    }
                    frontend::view_model::FrontendAction::StyleNewPack => {
                        self.open_style_v2(openless_core::StylePack {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: tr_l10n(self.lang, "lbl.new_style_default").to_string(),
                            ..Default::default()
                        })
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
                        self.save_field_edits(std::collections::BTreeMap::from([(
                            "/qaSaveHistory".into(),
                            serde_json::json!(!self.frontend_vm.qa_save_history),
                        )]));
                    }
                    frontend::view_model::FrontendAction::TranslationToggleLanguage(language) => {
                        let mut languages = self.frontend_vm.translation_working_languages.clone();
                        if languages.contains(&language) {
                            languages.retain(|s| s != &language);
                        } else {
                            languages.push(language);
                        }
                        self.save_field_edits(std::collections::BTreeMap::from([(
                            "/workingLanguages".into(),
                            serde_json::json!(languages),
                        )]));
                    }
                    frontend::view_model::FrontendAction::TranslationSetTarget(language) => {
                        self.save_field_edits(std::collections::BTreeMap::from([(
                            "/translationTargetLanguage".into(),
                            serde_json::json!(language),
                        )]));
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
                    frontend::view_model::FrontendAction::SettingsNotice(msg) => {
                        self.frontend_vm.settings_notice = Some(msg);
                    }
                    frontend::view_model::FrontendAction::MarketplaceDetail(index) => {
                        self.frontend_vm.marketplace_selected = Some(index);
                        self.frontend_vm.marketplace_prompt = None;
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
            openless_linux_egui::ui_catalog::set_language(self.lang);
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
            if !self.frontend_vm.settings_open
                && ctx.input(|input| input.key_pressed(egui::Key::Escape))
            {
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
            self.native_windows(ctx);
            if let Some(scale) = self.restored_font_scale.take() {
                ctx.set_zoom_factor(scale);
                ctx.data_mut(|d| d.remove::<f32>(egui::Id::new("openless-font-scale")));
            }
            self.marketplace_windows_v2(ctx);
            self.history_confirmation_ui(ctx);
            self.style_delete_confirmation_ui(ctx);
            self.onboarding_v2(ctx);
            if self.frontend_vm.settings_open {
                self.settings_v2(ctx);
            }

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

    include!("host_settings.rs");
    include!("host_models.rs");
    include!("host_omni.rs");
    include!("host_windows.rs");
    include!("host_onboarding.rs");
    include!("host_history.rs");
    include!("host_styles.rs");
    include!("host_marketplace.rs");

    /// Map a concrete UI language to its display-name catalog key, shown in
    /// that language's own native script regardless of the current UI language.
    fn locale_key(lang: Lang) -> &'static str {
        match lang {
            Lang::ZhCn => "lang.zh-CN",
            Lang::ZhTw => "lang.zh-TW",
            Lang::En => "lang.en",
            Lang::Ja => "lang.ja",
            Lang::Ko => "lang.ko",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
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

    fn auth_requirement_label(
        lang: Lang,
        requirement: openless_core::AuthRequirement,
    ) -> &'static str {
        let key = match requirement {
            openless_core::AuthRequirement::None => "auth.none",
            openless_core::AuthRequirement::ApiKey => "auth.api_key",
            openless_core::AuthRequirement::EndpointModelOptionalApiKey => {
                "auth.endpoint_model_optional"
            }
            openless_core::AuthRequirement::ApiKeyUnlessCustomEndpoint => {
                "auth.api_key_unless_custom"
            }
            openless_core::AuthRequirement::Volcengine => "auth.volcengine",
            openless_core::AuthRequirement::Xfyun => "auth.xfyun",
            openless_core::AuthRequirement::OAuth => "auth.oauth",
            openless_core::AuthRequirement::TencentCloud => "auth.api_key",
        };
        tr_l10n(lang, key)
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

    fn provider_fields_ui(ui: &mut egui::Ui, lang: Lang, editor: &mut ProviderEditor) {
        // This match chooses which input controls to render; it does not decide
        // whether credentials are sufficient. ProviderService validates the
        // descriptor's AuthRequirement again before any protocol request.
        match editor.descriptor.auth_requirement {
            openless_core::AuthRequirement::None => {
                ui.label(tr_l10n(lang, "providers.no_cloud_note"));
            }
            openless_core::AuthRequirement::OAuth => {
                ui.label(tr_l10n(lang, "providers.oauth_note"));
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
                secret_edit(
                    ui,
                    tr_l10n(lang, "providers.api_key_hint"),
                    &mut editor.primary_secret,
                );
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
        let mut capabilities = LinuxCapabilitySnapshot::detect(tray_available, kind).capabilities;
        capabilities.supports_auto_update &= updater_available;
        capabilities.supports_overlay |= openless_linux_egui::desktop_bridge::adapter().is_some();
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
            FcitxPluginStatus::Updated => {
                reload_running_fcitx5();
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
        lang: Lang,
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
            .fill(theme::surface_2())
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
                    theme::surface_2()
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
            let preference_tick = egui::Id::new("popup-preference-refresh");
            let now = std::time::Instant::now();
            if ctx
                .data(|d| d.get_temp::<std::time::Instant>(preference_tick))
                .is_none_or(|last| now.duration_since(last) >= std::time::Duration::from_secs(1))
            {
                self.lang = openless_linux_egui::load_locale_pref().resolve();
                if let Some(scale) =
                    openless_linux_egui::load_ui_value("fontScale").and_then(|v| v.as_f64())
                {
                    ctx.set_zoom_factor(scale.clamp(0.85, 1.35) as f32);
                }
                ctx.data_mut(|d| d.insert_temp(preference_tick, now));
            }
            openless_linux_egui::ui_catalog::set_language(self.lang);
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
            let lang = self.lang;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme::surface())
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::same(18)),
                )
                .show(ctx, |ui| match self.kind {
                    PopupKind::Preview => {
                        popup_heading(ui, tr_l10n(lang, "heading.insert_preview"));
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
                            if ui.button(tr_l10n(lang, "btn.cancel")).clicked() {
                                self.dismiss(ctx);
                            }
                            if ui.button(tr_l10n(lang, "btn.insert")).clicked() {
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
                        popup_heading(ui, tr_l10n(lang, "heading.qa_preview"));
                        if let Some(selection) = &self.state.qa.selection_preview {
                            ui.label(
                                egui::RichText::new(selection)
                                    .italics()
                                    .color(theme::ink_3()),
                            );
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
                            if ui.button(tr_l10n(lang, "btn.close")).clicked() {
                                self.dismiss(ctx);
                            }
                            if ui
                                .button(if self.state.qa.phase == "Recording" {
                                    tr_l10n(lang, "btn.stop_recording")
                                } else {
                                    tr_l10n(lang, "btn.voice_ask")
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
                            let submit = ui.button(tr_l10n(lang, "btn.send")).clicked()
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
                .with_title(match kind {
                    PopupKind::Qa => "OpenLess QA",
                    PopupKind::Preview => "OpenLess Preview",
                    PopupKind::Capsule => "OpenLess Capsule",
                })
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
                    // The popup is a separate process, so it re-reads the
                    // persisted UI-locale preference rather than sharing state.
                    lang: load_locale_pref().resolve(),
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
            assert_eq!(summary.activity_days_total, 4);

            // The trailing 364-day heatmap sums every in-window day.
            let heat_total: u32 = summary
                .heatmap_weeks
                .iter()
                .flat_map(|week| week.iter())
                .sum();
            assert_eq!(heat_total, 19);
            assert_eq!(summary.heatmap_days, 364);
        }

        #[test]
        fn overview_heatmap_excludes_days_outside_trailing_window() {
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

            let heat_total: u32 = summary
                .heatmap_weeks
                .iter()
                .flat_map(|week| week.iter())
                .sum();
            assert_eq!(
                heat_total, 3,
                "days older than the trailing window must not appear in the heatmap"
            );
            assert_eq!(summary.activity_days_total, 2);
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
