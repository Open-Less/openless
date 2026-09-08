//! Immutable configuration captured for one dictation session.
//!
//! Hosts and provider adapters receive this snapshot instead of re-reading
//! mutable preferences while recording, transcribing, polishing or inserting.

use crate::shared_types::{
    AndroidInsertStrategy, ChineseScriptPreference, MacosNewlineMode, OutputLanguagePreference,
    PasteShortcut, PipelineMode, UserPreferences, WindowsInsertionMode,
    WindowsSendInputNewlineMode,
};
use crate::style_packs::{translation_effective, StylePack};
use crate::types::{DictationSession, PolishMode};

pub const ASR_PROMPT_CHAR_BUDGET: usize = 240;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DictationAudioSource {
    #[default]
    Microphone,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictationStartOptions {
    pub translation_requested: bool,
    pub audio_source: DictationAudioSource,
    pub insert_text: bool,
    pub style_pack_id: Option<String>,
    pub front_app: Option<String>,
    pub cursor_context: Option<String>,
}

impl Default for DictationStartOptions {
    fn default() -> Self {
        Self {
            translation_requested: false,
            audio_source: DictationAudioSource::Microphone,
            insert_text: true,
            style_pack_id: None,
            front_app: None,
            cursor_context: None,
        }
    }
}

/// User intent that is only known when a recording is stopped.
///
/// Android's overlay keeps the existing gesture contract where a left swipe
/// while recording means "finish and translate". All mutable settings remain
/// frozen at start; this option may only select between the already captured
/// normal and translation polish paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DictationStopOptions {
    pub translation_requested: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInvocation {
    /// Stable channel identifier used to read channel-scoped credentials.
    /// This is deliberately distinct from `provider_type`: users may configure
    /// multiple channels backed by the same provider protocol.
    pub provider_id: String,
    /// Protocol/implementation routing key such as `volcengine`, `deepseek`,
    /// or `openai-compatible`.
    pub provider_type: String,
    pub model: Option<String>,
    pub language: Option<String>,
    pub prompt: Option<String>,
    /// Optional native runtime selector frozen with the session (for example
    /// Foundry `auto`/`gpu`/`cpu`). Cloud providers normally leave it empty.
    pub runtime: Option<String>,
    /// Native model retention policy captured at session start.
    pub keep_loaded_secs: Option<u32>,
}

pub(crate) struct DictationProviderInvocations {
    pub asr: ProviderInvocation,
    pub llm: ProviderInvocation,
    pub omni: ProviderInvocation,
}

impl DictationProviderInvocations {
    pub(crate) fn new(
        asr: ProviderInvocation,
        llm: ProviderInvocation,
        omni: ProviderInvocation,
    ) -> Self {
        Self { asr, llm, omni }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictationPolishContext {
    pub mode: PolishMode,
    pub style_pack_id: String,
    pub style_system_prompt: String,
    pub hotwords: Vec<String>,
    pub working_languages: Vec<String>,
    pub translation_target_language: String,
    pub translation_active: bool,
    pub chinese_script_preference: ChineseScriptPreference,
    pub output_language_preference: OutputLanguagePreference,
    pub llm_thinking_enabled: bool,
    pub context_window_minutes: u32,
    pub front_app: Option<String>,
    pub cursor_context: Option<String>,
    /// Newest-first turns captured when the session starts.
    pub prior_turns: Vec<PolishHistoryTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolishHistoryTurn {
    pub raw_text: String,
    pub polished_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictationInsertionContext {
    pub enabled: bool,
    pub observe_edits: bool,
    pub streaming: bool,
    pub save_streamed_text_to_clipboard: bool,
    pub restore_clipboard_after_paste: bool,
    pub paste_shortcut: PasteShortcut,
    pub windows_insertion_mode: WindowsInsertionMode,
    pub windows_sendinput_newline_mode: WindowsSendInputNewlineMode,
    pub macos_newline_mode: MacosNewlineMode,
    pub allow_non_tsf_fallback: bool,
    pub android_insert_strategy: AndroidInsertStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingPlan {
    pub microphone_device_name: Option<String>,
    pub mute_during_recording: bool,
    /// Whether the Host may create an audio archive at all. QA/Selection Voice
    /// keep PCM in memory; successful-recording retention is a separate policy.
    pub archive_enabled: bool,
    pub archive_successful_recording: bool,
    pub retention_days: u32,
    pub max_entries: Option<u32>,
    pub silence_after_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictationContext {
    pub audio_source: DictationAudioSource,
    pub recording: RecordingPlan,
    pub pipeline_mode: PipelineMode,
    pub correction_rules: Vec<crate::types::CorrectionRule>,
    pub asr: ProviderInvocation,
    pub llm: ProviderInvocation,
    /// Raw may start without an LLM. If translation is requested at stop,
    /// preserve the start-time resolution failure instead of using a disabled
    /// channel or silently selecting a newly configured provider.
    pub deferred_llm_error: Option<crate::errors::BackendError>,
    pub omni: ProviderInvocation,
    pub polish: DictationPolishContext,
    pub insertion: DictationInsertionContext,
}

impl Default for DictationContext {
    fn default() -> Self {
        let preferences = UserPreferences::default();
        let style_pack = crate::style_packs::builtin_style_pack_for_mode(preferences.default_mode);
        Self::capture(
            &preferences,
            &style_pack,
            DictationProviderInvocations::new(
                ProviderInvocation::for_provider(preferences.active_asr_provider.clone()),
                ProviderInvocation::for_provider(preferences.active_llm_provider.clone()),
                ProviderInvocation::for_provider(preferences.active_omni_provider.clone()),
            ),
            Vec::new(),
            Vec::new(),
            &DictationStartOptions::default(),
        )
    }
}

impl DictationContext {
    pub(crate) fn capture(
        preferences: &UserPreferences,
        style_pack: &StylePack,
        providers: DictationProviderInvocations,
        hotwords: Vec<String>,
        recent_history: Vec<DictationSession>,
        options: &DictationStartOptions,
    ) -> Self {
        let DictationProviderInvocations {
            mut asr,
            mut llm,
            omni,
        } = providers;
        let hotwords = normalize_hotwords(hotwords);
        let translation_target_language =
            preferences.translation_target_language.trim().to_string();
        let translation_active = translation_effective(
            options.translation_requested,
            &translation_target_language,
            &preferences.working_languages,
        );
        let (fallback_asr_model, fallback_asr_language) =
            selected_asr_details(preferences, &asr.provider_type);
        if asr.model.is_none() {
            asr.model = fallback_asr_model;
        }
        if asr.language.is_none() {
            asr.language = fallback_asr_language;
        }
        if matches!(
            asr.provider_type.as_str(),
            "foundry-local" | "foundry-whisper" | "foundry-local-whisper"
        ) {
            if asr.runtime.is_none() {
                asr.runtime = non_blank(&preferences.foundry_local_runtime_source);
            }
            if asr.keep_loaded_secs.is_none() {
                asr.keep_loaded_secs = Some(preferences.foundry_local_asr_keep_loaded_secs);
            }
        } else if matches!(
            asr.provider_type.as_str(),
            "sherpa-onnx" | "sherpa-onnx-local"
        ) {
            if asr.keep_loaded_secs.is_none() {
                asr.keep_loaded_secs = Some(preferences.sherpa_onnx_keep_loaded_secs);
            }
        } else if matches!(
            asr.provider_type.as_str(),
            "local-qwen3" | "local-qwen3-mlx" | "local-qwen3-c" | "local-whisper" | "apple-whisper"
        ) && asr.keep_loaded_secs.is_none()
        {
            asr.keep_loaded_secs = Some(preferences.local_asr_keep_loaded_secs);
        }
        asr.prompt = build_asr_prompt(&hotwords);
        if llm.model.is_none() {
            llm.model = style_pack
                .recommended_model
                .clone()
                .and_then(non_blank_owned);
        }
        let prior_turns =
            eligible_polish_context_turns(recent_history, &style_pack.id, translation_active);
        Self {
            audio_source: options.audio_source,
            recording: RecordingPlan {
                microphone_device_name: non_blank(&preferences.microphone_device_name),
                mute_during_recording: preferences.mute_during_recording,
                archive_enabled: true,
                archive_successful_recording: preferences.record_audio_for_debug,
                retention_days: preferences.history_retention_days,
                // Recordings and transcript history have independent caps in
                // the UI; only the age limit is shared with history.
                max_entries: preferences.audio_recording_max_entries,
                silence_after_ms: (preferences.silence_auto_stop_enabled
                    && preferences.hotkey.mode == crate::shared_types::HotkeyMode::Toggle)
                    .then(|| (preferences.silence_auto_stop_seconds * 1_000.0).round() as u64),
            },
            pipeline_mode: crate::shared_types::effective_pipeline_mode(
                preferences.multimodal_pipeline_enabled,
                preferences.pipeline_mode,
            ),
            correction_rules: Vec::new(),
            asr,
            llm,
            deferred_llm_error: None,
            omni,
            polish: DictationPolishContext {
                mode: style_pack.base_mode,
                style_pack_id: style_pack.id.clone(),
                style_system_prompt: style_pack.prompt.clone(),
                hotwords,
                working_languages: preferences.working_languages.clone(),
                translation_target_language,
                translation_active,
                chinese_script_preference: preferences.chinese_script_preference,
                output_language_preference: preferences.output_language_preference,
                llm_thinking_enabled: preferences.llm_thinking_enabled,
                context_window_minutes: preferences.polish_context_window_minutes,
                front_app: options.front_app.clone().and_then(non_blank_owned),
                cursor_context: options.cursor_context.clone().and_then(non_blank_owned),
                prior_turns,
            },
            insertion: DictationInsertionContext {
                enabled: options.insert_text,
                observe_edits: preferences.cursor_context_enabled,
                streaming: preferences.streaming_insert,
                save_streamed_text_to_clipboard: preferences.streaming_insert_save_clipboard,
                restore_clipboard_after_paste: preferences.restore_clipboard_after_paste,
                paste_shortcut: preferences.paste_shortcut,
                windows_insertion_mode: preferences.windows_insertion_mode,
                windows_sendinput_newline_mode: preferences.windows_sendinput_newline_mode,
                macos_newline_mode: crate::streaming_insert::resolve_macos_newline_mode(
                    preferences.macos_newline_mode,
                    options.front_app.as_deref(),
                ),
                allow_non_tsf_fallback: preferences.allow_non_tsf_insertion_fallback,
                android_insert_strategy: preferences.android_insert_strategy,
            },
        }
    }

    pub fn effective_polish_prompts(&self, raw_text: &str) -> (String, String) {
        let style_system_prompt = if self.polish.translation_active {
            crate::prompt_compose::build_polish_translate_system_prompt(
                &self.polish.style_system_prompt,
                &self.polish.translation_target_language,
            )
        } else {
            self.polish.style_system_prompt.clone()
        };
        crate::prompt_compose::compose_polish_prompts(
            raw_text,
            self.polish.mode,
            &self.polish.hotwords,
            &style_system_prompt,
            &self.polish.working_languages,
            self.polish.chinese_script_preference,
            self.polish.output_language_preference,
            self.polish.front_app.as_deref(),
            self.polish.cursor_context.as_deref(),
            !self.polish.prior_turns.is_empty(),
        )
    }

    /// Build the exact system prompt captured for this session.
    pub fn effective_polish_system_prompt(&self) -> String {
        self.effective_polish_prompts("").0
    }

    /// Match the legacy dictation rule: the untouched built-in Raw style is a
    /// true passthrough and must not require an LLM channel or open an LLM request.
    /// A custom Raw prompt and every translation request still use the
    /// polisher.
    pub fn uses_llm_polisher(&self) -> bool {
        self.polish.translation_active
            || self.polish.mode != PolishMode::Raw
            || self.polish.style_system_prompt
                != crate::style_packs::default_style_system_prompt_for_mode(PolishMode::Raw)
    }

    pub(crate) fn with_translation_requested(&self, requested: bool) -> Self {
        let mut updated = self.clone();
        updated.polish.translation_active = translation_effective(
            requested,
            &updated.polish.translation_target_language,
            &updated.polish.working_languages,
        );
        updated
    }
}

impl ProviderInvocation {
    pub fn new(provider_id: impl Into<String>, provider_type: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            provider_type: provider_type.into(),
            model: None,
            language: None,
            prompt: None,
            runtime: None,
            keep_loaded_secs: None,
        }
    }

    pub fn for_provider(provider_id: impl Into<String>) -> Self {
        let provider_id = provider_id.into();
        Self::new(provider_id.clone(), provider_id)
    }
}

pub fn build_asr_prompt(phrases: &[String]) -> Option<String> {
    let mut included = Vec::new();
    let mut used_chars = 0_usize;
    for phrase in phrases {
        let phrase = phrase.trim();
        if phrase.is_empty() {
            continue;
        }
        let added = phrase.chars().count() + usize::from(!included.is_empty()) * 2;
        if used_chars + added + 1 > ASR_PROMPT_CHAR_BUDGET {
            continue;
        }
        included.push(phrase);
        used_chars += added;
    }
    if included.is_empty() {
        return None;
    }
    Some(format!("{}.", included.join(", ")))
}

/// Keep only recent successful turns from the same style pack. The input and
/// output are newest-first so providers can reverse them when building chat
/// messages.
pub fn eligible_polish_context_turns(
    sessions: Vec<DictationSession>,
    active_style_pack_id: &str,
    current_translation_active: bool,
) -> Vec<PolishHistoryTurn> {
    const MAX_POLISH_CONTEXT_TURNS: usize = 2;

    sessions
        .into_iter()
        .filter(|session| session.error_code.is_none() && !session.final_text.trim().is_empty())
        .filter(|session| session.style_pack_id.as_deref() == Some(active_style_pack_id))
        .filter_map(|session| {
            let polished_text = if session.translation_active && !current_translation_active {
                session
                    .polish_source
                    .filter(|source| !source.trim().is_empty())?
            } else {
                session.final_text
            };
            Some(PolishHistoryTurn {
                raw_text: session.raw_transcript,
                polished_text,
            })
        })
        .take(MAX_POLISH_CONTEXT_TURNS)
        .collect()
}

fn selected_asr_details(
    preferences: &UserPreferences,
    provider: &str,
) -> (Option<String>, Option<String>) {
    match provider {
        "local-qwen3" | "local-qwen3-mlx" | "local-qwen3-c" => {
            (non_blank(&preferences.local_asr_active_model), None)
        }
        "local-whisper" | "apple-whisper" => {
            (non_blank(&preferences.local_whisper_active_model), None)
        }
        "foundry-local" | "foundry-whisper" | "foundry-local-whisper" => (
            non_blank(&preferences.foundry_local_asr_model),
            non_blank(&preferences.foundry_local_asr_language_hint),
        ),
        "sherpa-onnx" | "sherpa-onnx-local" => (
            non_blank(&preferences.sherpa_onnx_model),
            non_blank(&preferences.sherpa_onnx_language_hint),
        ),
        _ => (None, None),
    }
}

fn normalize_hotwords(hotwords: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for hotword in hotwords {
        let hotword = hotword.trim();
        if !hotword.is_empty() && !normalized.iter().any(|current| current == hotword) {
            normalized.push(hotword.to_string());
        }
    }
    normalized
}

fn non_blank(value: &str) -> Option<String> {
    non_blank_owned(value.to_string())
}

fn non_blank_owned(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style_packs::builtin_style_pack_for_mode;

    #[test]
    fn context_is_an_owned_snapshot_of_session_preferences() {
        let mut preferences = UserPreferences {
            microphone_device_name: "USB microphone".to_string(),
            active_asr_provider: "local-qwen3".to_string(),
            active_llm_provider: "openai".to_string(),
            local_asr_active_model: "qwen3-asr-1.7b".to_string(),
            history_max_entries: Some(100),
            audio_recording_max_entries: Some(7),
            working_languages: vec!["简体中文".to_string(), "English".to_string()],
            translation_target_language: "English".to_string(),
            ..UserPreferences::default()
        };
        let pack = builtin_style_pack_for_mode(PolishMode::Structured);
        let context = DictationContext::capture(
            &preferences,
            &pack,
            DictationProviderInvocations::new(
                ProviderInvocation::for_provider(preferences.active_asr_provider.clone()),
                ProviderInvocation::for_provider(preferences.active_llm_provider.clone()),
                ProviderInvocation::for_provider("omni"),
            ),
            vec!["OpenLess".to_string()],
            Vec::new(),
            &DictationStartOptions {
                translation_requested: true,
                style_pack_id: None,
                ..DictationStartOptions::default()
            },
        );

        preferences.microphone_device_name = "changed".to_string();
        preferences.active_asr_provider = "changed".to_string();
        assert_eq!(
            context.recording.microphone_device_name.as_deref(),
            Some("USB microphone")
        );
        assert_eq!(context.asr.provider_id, "local-qwen3");
        assert_eq!(context.asr.model.as_deref(), Some("qwen3-asr-1.7b"));
        // Audio archives have their own user-visible limit. History retention
        // may be much larger and must not silently override the recording cap.
        assert_eq!(context.recording.max_entries, Some(7));
        assert_eq!(
            context.insertion.windows_sendinput_newline_mode,
            preferences.windows_sendinput_newline_mode
        );
        assert_eq!(
            context.insertion.android_insert_strategy,
            preferences.android_insert_strategy
        );
        assert!(context.polish.translation_active);
        let prompt = context.effective_polish_system_prompt();
        assert!(prompt.contains("按当前风格润色并翻译"));
        assert!(prompt.contains("English"));
        assert!(prompt.contains(crate::prompt_compose::POLISH_TRANSLATE_TGT_MARKER));
    }

    #[test]
    fn asr_prompt_skips_entries_that_do_not_fit_without_dropping_later_ones() {
        let over_budget = "x".repeat(ASR_PROMPT_CHAR_BUDGET);
        let prompt = build_asr_prompt(&[over_budget, "OpenLess".to_string()]).unwrap();
        assert_eq!(prompt, "OpenLess.");
        assert!(prompt.chars().count() <= ASR_PROMPT_CHAR_BUDGET);
    }
}
