#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Shared value types used by every OpenLess host.

use serde::{Deserialize, Serialize};

use crate::android_types::{
    default_android_insert_strategy, default_android_overlay_activation_mode,
    default_android_overlay_cancel_swipe_direction, default_android_overlay_gesture_actions,
    default_android_overlay_left_swipe_action, default_android_overlay_size_dp,
    default_android_overlay_trigger, normalize_android_insert_strategy,
    normalize_android_overlay_size_dp,
};
pub use crate::android_types::{
    AndroidAccessibilityDiagnosis, AndroidAccessibilityRecoveryOutcome,
    AndroidAccessibilityRecoveryResult, AndroidAccessibilityState, AndroidAccessibilityStatus,
    AndroidInsertStrategy, AndroidOverlayActivationMode, AndroidOverlayCancelSwipeDirection,
    AndroidOverlayGestureAction, AndroidOverlayGestureActions, AndroidOverlayLeftSwipeAction,
    AndroidOverlayPermissionState, AndroidOverlayStatus, AndroidOverlayTrigger,
    AndroidShizukuState, AndroidShizukuStatus,
};

pub use crate::types::{HistorySource, PolishMode};

/// Compatibility value for "keep local ASR loaded": never auto-unload; unload only on explicit action or process exit.
pub const LOCAL_ASR_KEEP_LOADED_FOREVER_SECS: u32 = 86_400;

/// Recognition pipeline mode (issue #902): `traditional` = two-stage ASR +
/// LLM polish; `multimodal` = one multimodal model turns audio + prompt
/// into final text in a single step. The two configs live in fully isolated
/// credential namespaces; the runtime reads only the active mode and
/// switching never deletes the other config.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PipelineMode {
    #[default]
    Traditional,
    Multimodal,
}

pub fn effective_pipeline_mode(enabled: bool, configured: PipelineMode) -> PipelineMode {
    if enabled {
        configured
    } else {
        PipelineMode::Traditional
    }
}

fn default_pipeline_mode() -> PipelineMode {
    PipelineMode::Traditional
}

fn default_multimodal_pipeline_enabled() -> bool {
    false
}

fn default_active_omni_provider() -> String {
    "custom".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ChineseScriptPreference {
    #[default]
    Auto,
    Simplified,
    Traditional,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum OutputLanguagePreference {
    #[default]
    Auto,
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
}

/// Shortcut actually pressed by simulated paste. macOS uses AX direct
/// write / Cmd+V, so this enum only applies to the Windows / Linux
/// simulate_paste path. See issue #360: kitty and similar Linux terminals
/// only accept Ctrl+Shift+V — a hardcoded Ctrl+V is swallowed and the
/// dictated text survives only in the clipboard. Default `CtrlV` keeps
/// historical behavior; when the user picks `CtrlShiftV`
/// (kitty/alacritty/wezterm/gnome-terminal/foot/...) or `ShiftInsert`
/// (xterm/urxvt) in Settings, simulate_paste sends that combo.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PasteShortcut {
    #[default]
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

/// Windows dictation text insertion strategy. Default is the TSF IME; SendInput simulates keystrokes; Paste uses the clipboard plus a simulated paste key.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WindowsInsertionMode {
    #[default]
    Tsf,
    SendInput,
    Paste,
}

/// Newline simulation for the Windows SendInput path. Only applies to `WindowsInsertionMode::SendInput`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WindowsSendInputNewlineMode {
    #[default]
    Enter,
    ShiftEnter,
    CrLf,
}

/// How newlines are sent during macOS character-by-character insertion.
/// Only applies to the streaming insertion path.
///
/// Default `Auto`: chosen by the front app frozen at session start.
/// Terminal/TUI apps get U+000A; everything else and unknown apps fall
/// back to Shift+Return.
///
/// `Return` stays available because style packs in the marketplace send
/// multiple chat messages via newline, which requires a real Return.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum MacosNewlineMode {
    /// Auto-select from the front app captured at session start.
    #[default]
    Auto,
    /// Shift+Return: soft newline in chat boxes, doesn't send.
    ShiftReturn,
    /// U+000A: equivalent to Ctrl+J soft newline in Terminal/TUI.
    LineFeed,
    /// Return: sends in chat boxes — for style packs that split one passage
    /// into multiple messages.
    Return,
}

/// Auto-update channel. Picks which manifest the background AutoUpdateGate
/// fetches. `Stable` = `latest-android-{arch}.json` (or the desktop
/// plugin-updater stable endpoints). `Beta` = `latest-android-{arch}-beta.json`
/// (or the desktop beta endpoints). The Settings manual "check stable / check
/// beta" buttons pass the channel explicitly and ignore this pref.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

pub use crate::types::HistoryInsertStatus as InsertStatus;

/// How selection polish results are delivered: direct replace, or confirm in an editable preview first.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SelectionPolishOutputMode {
    #[default]
    DirectReplace,
    PreviewConfirm,
}

pub use crate::types::{SelectionVoiceIntentMode, SelectionVoiceManualIntent};

/// Split result of a front-app label: human-readable app name plus the (macOS) bundle id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
}

/// Splits the `capture_frontmost_app()` display string into
/// `FrontApp { name, bundle_id }`.
///
/// macOS composes `"Claude (com.anthropic.claudefordesktop)"`; Windows has
/// only window titles, no bundle id. History entries have separate
/// `app_name` / `app_bundle_id` fields, so splitting lets the detail page
/// show a readable app name instead of gluing the whole bundle id into the
/// body text.
///
/// Only macOS labels are `"name (bundle.id)"`; on Windows parentheses are
/// part of the title. Callers must pass `is_macos` per platform (production
/// goes through `split_front_app_opt`). Non-macOS labels and labels whose
/// bracket structure doesn't parse stay whole as the app name — better a
/// verbose display than misreading ordinary parentheses in a window title
/// as a bundle id.
pub fn split_front_app_label(label: &str, is_macos: bool) -> FrontApp {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return FrontApp {
            name: None,
            bundle_id: None,
        };
    }
    if is_macos {
        if let Some(open) = trimmed.rfind(" (") {
            if trimmed.ends_with(')') {
                let name = trimmed[..open].trim();
                let bundle = trimmed[open + 2..trimmed.len() - 1].trim();
                // A bundle id is always a dotted reverse domain. Bracketed
                // content without a dot (window titles like "Notepad
                // (unsaved)") is not a bundle id and must not be split.
                if !name.is_empty() && bundle.contains('.') && !bundle.contains(' ') {
                    return FrontApp {
                        name: Some(name.to_string()),
                        bundle_id: Some(bundle.to_string()),
                    };
                }
            }
        }
    }
    FrontApp {
        name: Some(trimmed.to_string()),
        bundle_id: None,
    }
}

/// `Option` convenience wrapper for `split_front_app_label`; keeps the
/// platform switch in one place: only macOS display strings are
/// `"name (bundle.id)"`; other platforms (Windows window titles, Linux)
/// keep the whole string as the name and leave the bundle id empty.
pub fn split_front_app_opt(label: Option<&str>) -> FrontApp {
    label
        .map(|l| split_front_app_label(l, cfg!(target_os = "macos")))
        .unwrap_or(FrontApp {
            name: None,
            bundle_id: None,
        })
}

/// Per-day summary for the Overview activity stats (date = local date YYYY-MM-DD).
///
/// The yearly heatmap uses only `count`; `chars` / `duration_ms` feed the
/// "last 7 / 30 days" character and duration metrics. Those used to be
/// computed on the fly from `list_history()`, which the 200-entry history
/// cap truncates (heavy users push last week out within days).
pub use crate::activity::ActivityDay;

pub use crate::types::DictationSession;

pub use crate::types::DictionaryEntry;

pub use crate::types::{CorrectionRule, RuleSource};

/// A vocabulary suggestion waiting for user confirmation.
///
/// Lives only in memory, never persisted: suggestions are ephemeral — when
/// the card disappears it's as if nothing happened, and correcting the same
/// word again raises a new suggestion. This is also why there is no reject
/// list: an invisible list would only make users wonder later why the app
/// stopped learning that word.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingCorrection {
    pub id: String,
    /// Absolute deadline shared by native and web confirmation surfaces.
    pub expires_at_ms: i64,
    /// The (wrong) pre-correction spelling. Only shown on the card so the
    /// user sees what changed; never persisted.
    pub pattern: String,
    /// The word the user finally wants — the one accepted into the
    /// vocabulary on "OK".
    pub replacement: String,
}

/// Max entries listed on one card. Multiple corrections in one dictation
/// merge onto a single card; beyond this the oldest is dropped — a card
/// taller than the screen is pointless.
pub const MAX_PENDING_CORRECTIONS: usize = 5;

/// Marker used to distinguish vocabulary entries accepted from the manual-edit
/// suggestion flow from entries explicitly created in Settings.
pub const LEARNED_VOCAB_NOTE: &str = "从手改中自动收集";

/// Content of the insert-failure fallback card.
///
/// When text fails to land in the target app (focus lost mid-insert,
/// Secure Input, insertion failure), surface the **full** passage with a
/// copy entry point. Previously the only fallback was silently writing the
/// clipboard, which depended on a default-off toggle and was invisible to
/// the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InsertFallbackCardPayload {
    /// Full text. Focus lost mid-insert leaves only part on screen; this
    /// carries the whole passage.
    pub text: String,
    /// Why the insert failed. **Log-only, never rendered** — the card has no
    /// title line. See `INSERT_FALLBACK_REASON_*`.
    pub reason: String,
    /// Generation of this card display. The size-measurement IPC must echo
    /// it back so a stale card's late report can't rescale the new card.
    pub presentation_id: u64,
}

/// Character-by-character insertion broke mid-stream (Secure Input opened,
/// synthetic keystrokes rejected).
pub const INSERT_FALLBACK_REASON_PARTIAL_STREAM: &str = "partialStream";
/// Insertion could not complete (Secure Input, accessibility permission
/// revoked, paste rejected, etc.).
pub const INSERT_FALLBACK_REASON_INSERT_FAILED: &str = "insertFailed";

/// Card auto-dismissal time.
///
/// On expiry it's as if nothing happened — nothing is recorded. Asking
/// again the next time the user corrects the same word is the trade-off for
/// having no reject list.
pub const VOCAB_SUGGESTION_TTL_MS: u64 = 10_000;

/// Local-only tuning. Consent and sensitive-field protections remain separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VocabularyLearningSettings {
    pub observation_seconds: u32,
    pub suggestion_seconds: u32,
    pub max_phrase_chars: u32,
}

impl Default for VocabularyLearningSettings {
    fn default() -> Self {
        Self {
            observation_seconds: 60,
            suggestion_seconds: 10,
            max_phrase_chars: 12,
        }
    }
}

impl VocabularyLearningSettings {
    pub fn normalized(self) -> Self {
        Self {
            observation_seconds: self.observation_seconds.clamp(10, 60),
            suggestion_seconds: self.suggestion_seconds.clamp(5, 60),
            max_phrase_chars: self.max_phrase_chars.clamp(2, 32),
        }
    }
}

pub use crate::types::{VocabPreset, VocabPresetStore};

pub use crate::style_packs::*;

fn default_true() -> bool {
    true
}

fn default_silence_auto_stop_seconds() -> f32 {
    3.0
}

fn resolve_windows_insertion_mode(
    mode: WindowsInsertionMode,
    legacy_sendinput_only: bool,
) -> WindowsInsertionMode {
    if mode != WindowsInsertionMode::Tsf {
        mode
    } else if legacy_sendinput_only {
        WindowsInsertionMode::SendInput
    } else {
        WindowsInsertionMode::Tsf
    }
}

fn resolve_windows_sendinput_insertion_only_legacy(
    mode: WindowsInsertionMode,
    legacy_sendinput_only: bool,
) -> bool {
    resolve_windows_insertion_mode(mode, legacy_sendinput_only) == WindowsInsertionMode::SendInput
}

#[derive(Debug, Clone, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserPreferences {
    pub hotkey: HotkeyBinding,
    pub dictation_hotkey: ShortcutBinding,
    pub default_mode: PolishMode,
    pub enabled_modes: Vec<PolishMode>,
    #[serde(default = "default_active_style_pack_id")]
    pub active_style_pack_id: String,
    #[serde(default)]
    pub style_system_prompts: StyleSystemPrompts,
    #[serde(default)]
    pub custom_style_prompts: CustomStylePrompts,
    pub launch_at_login: bool,
    pub show_capsule: bool,
    /// Recording capsule appearance. Preference events sync it to all windows; recording state carries the current style.
    #[serde(default)]
    pub capsule_style: CapsuleStyle,
    #[serde(default = "default_true")]
    pub capsule_transcript_enabled: bool,
    #[serde(default = "default_capsule_transcript_font_size")]
    pub capsule_transcript_font_size: u8,
    /// Temporarily mute system output during recording; restore the original mute state on stop/cancel/error.
    #[serde(default)]
    pub mute_during_recording: bool,
    /// Reconnect the current ASR after recording ends and submit the whole PCM. Default off.
    #[serde(default)]
    pub stable_transcription_enabled: bool,
    /// Play an instantly synthesized cue when the recording hotkey enters the
    /// recording state ("recording started"). Default on; can be disabled in
    /// the "Recording & Input" settings. The cue is synthesized by the capsule
    /// window via the Web Audio API and does not depend on show_capsule — it
    /// still plays while the capsule is hidden.
    #[serde(default = "default_true")]
    pub audio_cue_on_record: bool,
    /// Toggle-mode "auto-stop after speech" (issue #860): once speech is
    /// detected, continuous silence for `silence_auto_stop_seconds` stops and
    /// commits; if no speech is ever detected, auto-cancel after 10 seconds.
    /// Default off to keep the existing "press twice" behavior; push-to-talk
    /// is unaffected.
    #[serde(default)]
    pub silence_auto_stop_enabled: bool,
    /// Continuous-silence threshold after speech, in seconds. Options 1 / 1.5 / 2 / 3 / 4 / 5, default 3.
    #[serde(default = "default_silence_auto_stop_seconds")]
    pub silence_auto_stop_seconds: f32,
    /// Recording input device name. Empty string = system default microphone.
    #[serde(default)]
    pub microphone_device_name: String,
    pub active_asr_provider: String, // "volcengine" | "apple-speech" | ...
    pub active_llm_provider: String, // "ark" | "openai" | ...
    /// Recognition pipeline mode (experimental, issue #902). With
    /// `multimodal`, voice pipelines switch to the separately isolated
    /// multimodal model config (`omni.*` credential namespace) instead of
    /// the ASR/LLM pair.
    #[serde(default = "default_pipeline_mode")]
    pub pipeline_mode: PipelineMode,
    /// Master switch for the experimental "multimodal pipeline" (Advanced settings). When off, behavior matches the old version.
    #[serde(default = "default_multimodal_pipeline_enabled")]
    pub multimodal_pipeline_enabled: bool,
    /// Currently active provider id of the multimodal (Omni) model. Mirrors
    /// the credential vault's `omni.active` to initialize the settings
    /// dropdown; CredentialsVault stays authoritative at runtime.
    #[serde(default = "default_active_omni_provider")]
    pub active_omni_provider: String,
    /// LLM thinking-mode switch. Defaults to false to keep the existing
    /// "thinking off" behavior; Gemini uses native thinkingConfig, the
    /// OpenAI-compatible path sends provider/channel-level official fields
    /// only, and the official OpenAI channel skips fields unsupported by
    /// plain chat models. See issue #402.
    #[serde(default)]
    pub llm_thinking_enabled: bool,
    /// Whether to use the system proxy (issue #869). Default true follows the
    /// system proxy, matching historical behavior; when off, all reqwest
    /// requests connect directly (lower latency for China-hosted services),
    /// while GitHub login, updates, and other overseas services may become
    /// unreachable. Realtime voice streams (WebSocket) and Less Computer
    /// subprocesses ignore this switch.
    #[serde(default = "default_true")]
    pub use_system_proxy: bool,
    /// Whether to restore the user's original clipboard after a successful
    /// Windows/Linux paste. Default true matches historical behavior; when
    /// off, the dictated text stays on the clipboard so users can recover it
    /// with Ctrl+V when simulate_paste silently failed. macOS uses AX direct
    /// write and ignores this switch. See issue #111.
    pub restore_clipboard_after_paste: bool,
    /// Simulated paste key for Windows / Linux. macOS uses AX direct write
    /// and is unaffected. See issue #360: kitty and similar Linux terminals
    /// reject Ctrl+V and need Ctrl+Shift+V. Default CtrlV preserves history
    /// for existing users.
    #[serde(default)]
    pub paste_shortcut: PasteShortcut,
    /// Windows: whether TSF failure may fall back to batched Unicode
    /// SendInput / clipboard. The clipboard copy happens only after Unicode
    /// SendInput fails, avoiding text loss. Default on for usability; turn
    /// off to verify text is truly inserted by TSF.
    #[serde(default = "default_true")]
    pub allow_non_tsf_insertion_fallback: bool,
    /// Windows dictation insertion strategy: TSF / SendInput / clipboard paste.
    #[serde(default)]
    pub windows_insertion_mode: WindowsInsertionMode,
    /// Newline simulation for the Windows SendInput path.
    #[serde(default, rename = "windowsSendInputNewlineMode")]
    pub windows_sendinput_newline_mode: WindowsSendInputNewlineMode,
    /// Newline simulation for macOS character-by-character insertion.
    #[serde(default)]
    pub macos_newline_mode: MacosNewlineMode,
    /// Legacy wire compatibility: `true` is equivalent to `windows_insertion_mode = SendInput`.
    #[serde(
        default,
        rename = "windowsSendInputInsertionOnly",
        alias = "windowsSendinputInsertionOnly"
    )]
    pub windows_sendinput_insertion_only: bool,
    /// Windows: whether the OpenLess TSF IME shows in the system keyboard
    /// list (Win+Space) under non-TSF insertion (SendInput / clipboard
    /// paste). Default true keeps current behavior; when off, the language
    /// profile is disabled per-user without admin rights. TSF mode still
    /// force-enables the profile but never rewrites this pref.
    #[serde(default = "default_true", rename = "windowsShowOpenlessInKeyboardList")]
    pub windows_show_openless_in_keyboard_list: bool,
    /// User's working languages (multi-select, native names). Injected as
    /// context at the head of the LLM polish/translate system prompt so the
    /// model knows which languages the user works in. See issue #4.
    #[serde(default = "default_working_languages")]
    pub working_languages: Vec<String>,
    /// Translation output target language (single-select, native name).
    /// Empty = translation mode off (Shift combos are inert). The frontend
    /// picks from the built-in language list; the backend just splices the
    /// native name into the prompt. See issue #4.
    #[serde(default)]
    pub translation_target_language: String,
    /// Chinese output script preference (not exposed as a UI switch):
    /// - Simplified: prefer simplified for Chinese output
    /// - Traditional: prefer traditional for Chinese output
    /// - Auto: no extra constraint
    ///
    /// Driven by the frontend "UI language" choice (simplified/traditional).
    /// See issue #259.
    #[serde(default)]
    pub chinese_script_preference: ChineseScriptPreference,
    /// Final output language preference (not exposed as a UI switch). Driven
    /// by the frontend "UI language" choice: zh-CN/zh-TW/en/ja/ko, anything
    /// else is Auto.
    #[serde(default)]
    pub output_language_preference: OutputLanguagePreference,
    /// Global hotkey for selection voice QA. `None` = feature off; with
    /// `Some(...)` the coordinator registers the combo (modifier + primary
    /// key) via the global-hotkey crate. Defaults to Cmd+Shift+; (macOS) /
    /// Ctrl+Shift+; (Windows). See issue #118.
    #[serde(default = "default_qa_hotkey")]
    pub qa_hotkey: Option<ShortcutBinding>,
    /// Standalone quick-note hotkey. None = not configured; when enabled, press once to start and again to stop.
    #[serde(default)]
    pub quick_note_hotkey: Option<ShortcutBinding>,
    /// Global hotkey for selection polish. Defaults to right Alt on Windows; off on other platforms.
    #[serde(default = "default_selection_polish_hotkey")]
    pub selection_polish_hotkey: Option<ShortcutBinding>,
    /// Style pack used exclusively for written selection polish; migrates to the default built-in light-polish pack when unset.
    #[serde(default = "default_active_style_pack_id")]
    pub selection_polish_style_pack_id: String,
    /// Selection polish replaces directly, or confirms in an editable preview first.
    #[serde(default)]
    pub selection_polish_output_mode: SelectionPolishOutputMode,
    /// Selection voice editing (issue #987 desktop MVP). Default off.
    #[serde(default)]
    pub selection_voice_enabled: bool,
    #[serde(default)]
    pub selection_voice_intent_mode: SelectionVoiceIntentMode,
    #[serde(default)]
    pub selection_voice_manual_intent: SelectionVoiceManualIntent,
    #[serde(default = "default_selection_voice_edit_keywords")]
    pub selection_voice_edit_keywords: Vec<String>,
    /// Preferred output format for selection voice EditPlan (issue #1076). Default XML.
    #[serde(default)]
    pub selection_voice_edit_plan_format: crate::edit_plan::EditPlanFormat,
    /// Custom system prompt for selection voice EditPlan; empty = style pack / built-in default.
    #[serde(default)]
    pub selection_voice_edit_system_prompt: String,
    /// Whether to write each QA session into history.json. Default false:
    /// QA stays ephemeral by default. See issue #118.
    #[serde(default)]
    pub qa_save_history: bool,
    /// Custom recording combo. When `hotkey.trigger == Custom`, the
    /// coordinator registers this combo via the `global-hotkey` crate
    /// (Toggle + Hold modes supported). `None` with trigger == Custom means
    /// the user picked custom but hasn't recorded a key yet.
    #[serde(default)]
    pub custom_combo_hotkey: Option<ComboBinding>,
    #[serde(default = "default_translation_hotkey")]
    pub translation_hotkey: ShortcutBinding,
    /// "Switch style" global hotkey. `None` = disabled (no global key
    /// registered); `Some(...)` = registered. Defaults to `Some(default key)`
    /// — zero behavior change for existing users, only newly clearable
    /// (issue #576).
    #[serde(default = "default_switch_style_hotkey")]
    pub switch_style_hotkey: Option<ShortcutBinding>,
    /// "Open App" global hotkey. `None` = disabled; `Some(...)` = registered. Defaults to `Some(default key)`.
    #[serde(default = "default_open_app_hotkey")]
    pub open_app_hotkey: Option<ShortcutBinding>,
    /// Direct style-pack hotkeys: each entry binds a global combo to a
    /// specific style-pack id (issue #759). Binding by id rather than "Nth
    /// in the enabled list" means enabling/disabling other packs never
    /// shifts an existing binding. Defaults to an empty list (no Alt+1~9
    /// preset: on macOS Option+digits type special characters and global
    /// registration would swallow normal typing). Triggering a binding for
    /// a disabled pack auto-enables and activates it.
    #[serde(default)]
    pub style_pack_hotkeys: Vec<StylePackHotkey>,
    /// Less Computer: whether enabled. Default off; users must enable it in Advanced settings.
    #[serde(default)]
    pub coding_agent_enabled: bool,
    /// Agent backend: `claude-code-cli` (default) or `opencode-cli`.
    #[serde(default = "default_coding_agent_provider")]
    pub coding_agent_provider: String,
    /// Agent model (`None` = runtime picks the cheap default sonnet).
    #[serde(default)]
    pub coding_agent_model: Option<String>,
    /// Permission mode: plan/default/acceptEdits/bypassPermissions. Default acceptEdits (auto-approve with guardrails).
    #[serde(default = "default_coding_agent_permission_mode")]
    pub coding_agent_permission_mode: String,
    /// Agent working directory (`None` = temp directory).
    #[serde(default)]
    pub coding_agent_workdir: Option<String>,
    /// Agent executable path/command (`None` or blank = backend default
    /// `claude` / `opencode`). Lets users set a custom path under
    /// "Advanced → Less Computer" (e.g. an opencode binary not on PATH).
    #[serde(default)]
    pub coding_agent_exe: Option<String>,
    /// Less Computer voice trigger key. macOS only; supports single
    /// modifiers (left/right Control, left/right Option, Fn) and ordinary
    /// combos. `None` = disabled.
    #[serde(default = "default_coding_agent_voice_hotkey")]
    pub coding_agent_voice_hotkey: Option<ShortcutBinding>,
    /// Hotkey 1: voice Agent panel key. Default Cmd/Ctrl+Shift+Enter. `None` = disabled.
    #[serde(default = "default_coding_agent_panel_hotkey")]
    pub coding_agent_panel_hotkey: Option<ShortcutBinding>,
    /// Hotkey 2: quick-use key (select -> Claude -> insert back). Default `None` (user-configured).
    #[serde(default)]
    pub coding_agent_quick_hotkey: Option<ShortcutBinding>,
    /// LAN remote-input service switch. The desktop starts an HTTPS+WS server; a phone browser pushes PCM to the computer.
    #[serde(default)]
    pub remote_input_enabled: bool,
    /// LAN remote-input service port.
    #[serde(default = "default_remote_input_port")]
    pub remote_input_port: u16,
    /// Current remote-input PIN. The real runtime PIN is maintained via separate in-process/disk paths; this field stays for wire compatibility.
    #[serde(default)]
    pub remote_input_pin: String,
    /// Remote-input default button mode.
    #[serde(default = "default_remote_input_mode")]
    pub remote_input_default_mode: String,
    /// Currently active local Qwen3-ASR model id ("qwen3-asr-0.6b" /
    /// "qwen3-asr-1.7b"). Only meaningful when active_asr_provider is
    /// local-qwen3 / local-qwen3-mlx / local-qwen3-c.
    #[serde(default = "default_local_asr_model")]
    pub local_asr_active_model: String,
    /// Currently active macOS local Whisper model id. Stored separately from
    /// the Qwen preference so testing Whisper in Settings doesn't clobber
    /// the Qwen model choice.
    #[serde(default = "default_local_whisper_model")]
    pub local_whisper_active_model: String,
    /// Local model download source ("huggingface" / "hf-mirror" / "modelscope").
    #[serde(default = "default_local_asr_mirror")]
    pub local_asr_mirror: String,
    /// How long the local ASR engine stays in memory (seconds). 0 = release
    /// right after the session; larger values = stay N seconds after last
    /// use; 86400 = never auto-release. Default 300 (5 min): balances
    /// no-reload across consecutive dictations against freeing 1.2GB+ RAM
    /// when idle.
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    pub local_asr_keep_loaded_secs: u32,
    /// Custom parent directory for local models. Empty = the default
    /// `models/` under app data. When set, the actual model root is
    /// `<local_asr_models_base_dir>/OpenLess/models/`, letting users isolate
    /// OpenLess model files in any ordinary disk directory.
    #[serde(default)]
    pub local_asr_models_base_dir: String,
    /// Currently active Windows Foundry Local Whisper model alias.
    #[serde(default = "default_foundry_local_asr_model")]
    pub foundry_local_asr_model: String,
    /// Windows Foundry Local native runtime download source: "auto" / "nuget" / "ort-nightly".
    #[serde(default = "default_foundry_local_runtime_source")]
    pub foundry_local_runtime_source: String,
    /// Windows Foundry Local Whisper language hint. Empty string = auto-detect.
    #[serde(default)]
    pub foundry_local_asr_language_hint: String,
    /// How long the Windows Foundry Local Whisper model stays loaded in the runtime.
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    pub foundry_local_asr_keep_loaded_secs: u32,
    /// Currently active Windows sherpa-onnx local ASR model alias.
    #[serde(default = "default_sherpa_onnx_model")]
    pub sherpa_onnx_model: String,
    /// Windows sherpa-onnx language hint (lowercase BCP-47 / ISO 639-1). Empty = auto.
    #[serde(default)]
    pub sherpa_onnx_language_hint: String,
    /// How long the Windows sherpa-onnx model stays loaded in the runtime
    /// (seconds); same semantics as foundry/qwen3.
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    pub sherpa_onnx_keep_loaded_secs: u32,
    /// Auto-update channel. stable = background auto-update checks the
    /// stable manifest; beta = the beta manifest. Manual check buttons pass
    /// the channel explicitly, decoupled from this pref.
    #[serde(default)]
    pub update_channel: UpdateChannel,
    /// Whether the user explicitly chose the update channel. Older versions
    /// wrote Stable into the config by default, so `update_channel` alone
    /// can't distinguish default from a deliberate switch; a historical Beta
    /// always came from user opt-in.
    #[serde(default)]
    pub update_channel_explicit: bool,
    /// History retention in days. 0 = no time-based cleanup (only the
    /// 200-entry cap). Default 7 days. Cleanup runs when a new entry is
    /// written, avoiding background polling.
    #[serde(default = "default_history_retention_days")]
    pub history_retention_days: u32,
    /// Context window (minutes) for conversation-aware polish: the last N
    /// minutes of transcripts + polished text feed the LLM as multi-turn
    /// context so pronouns / incomplete sentences resolve correctly.
    /// 0 = off (each polish is a standalone single turn, as historically).
    /// Default 5 minutes.
    #[serde(default = "default_polish_context_window_minutes")]
    pub polish_context_window_minutes: u32,
    /// Start silently (no main window). Common for login-launch users who
    /// want the tray rather than a popped-up window. When on, every launch
    /// path skips the window (manual clicks included); users reach the main
    /// window via the tray menu. Default false, matching history.
    #[serde(default)]
    pub start_minimized: bool,
    /// UI theme: follow OS, force light, or force dark. Frontend applies via data-ol-theme.
    #[serde(default)]
    pub theme_mode: ThemeMode,
    /// Streaming insert: polish SSE output is typed character-by-character
    /// into the current focus as it arrives, sharply lowering perceived
    /// latency (typing starts at the polish LLM's first token).
    ///
    /// Platform primitives:
    /// - macOS: CGEvent Unicode FFI; CJK / Japanese IMEs intercept, so the
    ///   session temporarily switches to ABC
    /// - Windows: SendInput Unicode (bypasses TSF); no IME switch needed
    /// - Linux: direct write via the fcitx5 plugin commitString, or
    ///   clipboard fallback.
    ///
    /// Limits:
    /// - No clipboard path; silently refuses secure input fields
    ///   (password boxes / 1Password)
    /// - Only OpenAI-compatible providers implemented (v1); Gemini / Codex
    ///   providers use the original one-shot insertion path
    ///
    /// Default true (since 1.3.2-3) — low perceived latency, all fallback
    /// cases wired, so it works out of the box. CJK IME / Codex / Gemini
    /// providers fall back to the one-shot path transparently. See the
    /// "Limits" section above.
    #[serde(default = "default_true")]
    pub streaming_insert: bool,
    /// One-shot migration marker for issue #440. Old versions wrote the
    /// default `streamingInsert:false` into preferences.json, so after
    /// upgrade the bool alone can't distinguish "old default" from "user
    /// turned it off". Old files without this marker migrate to true; after
    /// migration a user's off choice is saved with the marker and stays false.
    #[serde(default)]
    pub streaming_insert_default_migrated: bool,
    /// Whether to write the final polished text back to the clipboard after a
    /// successful streaming insert. The one-shot path naturally uses the
    /// clipboard, so Cmd+V can re-paste; the streaming path synthesizes
    /// keystrokes without touching the clipboard, removing that safety net.
    /// When on, a successful streaming finish writes the final text to the
    /// system clipboard, matching one-shot behavior. Default true (closer to
    /// user habits).
    #[serde(default = "default_true")]
    pub streaming_insert_save_clipboard: bool,
    /// Whether to send original text near the cursor from the document the
    /// user is writing as LLM polish context.
    ///
    /// **Default false, and must stay false.** When on, every dictation
    /// reads the front app's body and sends part of it to the LLM vendor —
    /// data the user never handed over voluntarily, so only they may opt in.
    /// When off, `host_document` issues zero AX calls and prompts are
    /// byte-identical to before this feature existed.
    ///
    /// macOS-only today; Windows / Linux degrade gracefully to no context.
    /// Password boxes / Secure Input / password managers / terminals are
    /// hard-blocked regardless of this switch.
    #[serde(default)]
    pub cursor_context_enabled: bool,
    /// Observe corrections locally after insertion; independent of LLM context.
    #[serde(default)]
    pub vocabulary_learning_enabled: bool,
    #[serde(default)]
    pub vocabulary_learning_settings: VocabularyLearningSettings,
    /// Whether the Overview shows the "yearly activity" heatmap card.
    /// Default true; turning it off only hides the card — activity keeps
    /// being recorded (persistence/activity.rs), so the full year's data is
    /// still there when re-enabled.
    #[serde(default = "default_true")]
    pub show_overview_activity_heatmap: bool,
    /// Readable layout: force same-row controls to wrap on small screens or large fonts, avoiding horizontal overflow and squashed text. Default false.
    #[serde(default)]
    pub stacked_row_layout: bool,
    /// Conservative layout: force the content area to a single full-width column except for the home page, top/bottom bars, and capsule window. Default false.
    #[serde(default)]
    pub conservative_layout: bool,
    /// Check for updates automatically on main-window launch plus every 60
    /// minutes in the background. Default true. On Android this also
    /// downloads and, after verification, opens the system installer; on
    /// desktop it only checks + asks the user to confirm install. When off,
    /// only the Settings manual "check for updates" button works.
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
    /// History entry cap. `None` = the built-in 200-entry hard cap;
    /// `Some(n)` = a user-defined cap from Settings (5..=200).
    #[serde(default)]
    pub history_max_entries: Option<u32>,
    /// Whether to keep each session's raw microphone audio (wav) under
    /// `recordings/` for diagnosing ASR misrecognition / mic sensitivity.
    /// Default false. When on it consumes disk and follows the same cleanup
    /// policy as `history_retention_days`.
    #[serde(default)]
    pub record_audio_for_debug: bool,
    /// How many recent wav files `recordings/` keeps (newest first by mtime).
    /// `None` = follow `HISTORY_CAP` (200); `Some(n)` clamps to 1..=200.
    /// Trimmed before each new session. Lets users tune combinations like
    /// "200 text entries but only the 5 newest wavs" — rich text history
    /// without disk-hogging audio.
    #[serde(default)]
    pub audio_recording_max_entries: Option<u32>,
    /// Directory for quick-note exported recordings. Empty string = show a save dialog on every export.
    #[serde(default)]
    pub quick_note_export_directory: String,
    /// Style Pack Marketplace HTTP base URL. Empty = the local-dev default
    /// http://127.0.0.1:8090; users enter a production URL in Settings
    /// (e.g. https://api.openless-marketplace.com).
    #[serde(default)]
    pub marketplace_base_url: String,
    /// Display cache of the GitHub login. Not for authentication; the OAuth token lives only in CredentialsVault.
    #[serde(default)]
    pub marketplace_dev_login: String,
    /// Android: text insertion strategy for cross-app dictation results.
    #[serde(default = "default_android_insert_strategy")]
    pub android_insert_strategy: AndroidInsertStrategy,
    /// Android: when to show the floating overlay control.
    #[serde(default = "default_android_overlay_trigger")]
    pub android_overlay_trigger: AndroidOverlayTrigger,
    /// Android: how the floating overlay enters the armed interaction state.
    #[serde(default = "default_android_overlay_activation_mode")]
    pub android_overlay_activation_mode: AndroidOverlayActivationMode,
    /// Android: action performed by left swiping while the overlay is armed.
    #[serde(default = "default_android_overlay_left_swipe_action")]
    pub android_overlay_left_swipe_action: AndroidOverlayLeftSwipeAction,
    /// Android: vertical swipe direction that cancels recording.
    #[serde(default = "default_android_overlay_cancel_swipe_direction")]
    pub android_overlay_cancel_swipe_direction: AndroidOverlayCancelSwipeDirection,
    /// Android: action assigned to each overlay swipe direction.
    #[serde(default = "default_android_overlay_gesture_actions")]
    pub android_overlay_gesture_actions: AndroidOverlayGestureActions,
    /// Android: floating overlay control diameter in dp.
    #[serde(default = "default_android_overlay_size_dp")]
    pub android_overlay_size_dp: u32,
    /// Major-version generation marker of the splash PV (e.g. "2"). Empty =
    /// never played. At startup Rust compares this with the current app
    /// major version: on mismatch it writes back and plays the splash once,
    /// then stays silent within the same generation (2.x patch/minor
    /// upgrades, restarts). Consumed by `take_splash_playback`;
    /// `update_settings` always keeps the current value so whole-file client
    /// saves can't wipe it.
    #[serde(default)]
    pub splash_seen_version: String,
}

impl UserPreferences {
    pub fn preserve_style_preferences_from(&mut self, current: &Self) {
        self.default_mode = current.default_mode;
        self.enabled_modes = current.enabled_modes.clone();
        self.active_style_pack_id = current.active_style_pack_id.clone();
        self.style_system_prompts = current.style_system_prompts.clone();
        self.custom_style_prompts = current.custom_style_prompts.clone();
    }
}

fn default_local_asr_model() -> String {
    "qwen3-asr-0.6b".into()
}

fn default_local_whisper_model() -> String {
    crate::local_asr_catalog::WHISPER_MODEL_ID.into()
}

fn default_remote_input_port() -> u16 {
    8443
}

fn default_remote_input_mode() -> String {
    "toggle".into()
}

fn default_history_retention_days() -> u32 {
    7
}

fn default_polish_context_window_minutes() -> u32 {
    5
}

fn default_local_asr_mirror() -> String {
    "huggingface".into()
}

fn default_local_asr_keep_loaded_secs() -> u32 {
    300
}

fn default_foundry_local_asr_model() -> String {
    crate::local_asr_catalog::FOUNDRY_DEFAULT_MODEL_ALIAS.into()
}

fn default_foundry_local_runtime_source() -> String {
    "auto".into()
}

fn default_sherpa_onnx_model() -> String {
    crate::local_asr_catalog::SHERPA_DEFAULT_MODEL_ALIAS.into()
}

fn default_active_asr_provider() -> String {
    #[cfg(target_os = "windows")]
    {
        crate::local_asr_catalog::FOUNDRY_PROVIDER_ID.into()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "volcengine".into()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UserPreferencesWire {
    hotkey: HotkeyBinding,
    dictation_hotkey: Option<ShortcutBinding>,
    default_mode: PolishMode,
    enabled_modes: Vec<PolishMode>,
    #[serde(default)]
    active_style_pack_id: Option<String>,
    #[serde(default)]
    style_system_prompts: StyleSystemPrompts,
    #[serde(default)]
    custom_style_prompts: CustomStylePrompts,
    launch_at_login: bool,
    show_capsule: bool,
    #[serde(default)]
    capsule_style: CapsuleStyle,
    #[serde(default = "default_true")]
    capsule_transcript_enabled: bool,
    #[serde(default = "default_capsule_transcript_font_size")]
    capsule_transcript_font_size: u8,
    #[serde(default)]
    mute_during_recording: bool,
    #[serde(default)]
    stable_transcription_enabled: bool,
    #[serde(default = "default_true")]
    audio_cue_on_record: bool,
    #[serde(default)]
    silence_auto_stop_enabled: bool,
    #[serde(default = "default_silence_auto_stop_seconds")]
    silence_auto_stop_seconds: f32,
    #[serde(default)]
    microphone_device_name: String,
    active_asr_provider: String,
    active_llm_provider: String,
    #[serde(default = "default_pipeline_mode")]
    pipeline_mode: PipelineMode,
    #[serde(default = "default_multimodal_pipeline_enabled")]
    multimodal_pipeline_enabled: bool,
    #[serde(default = "default_active_omni_provider")]
    active_omni_provider: String,
    #[serde(default)]
    llm_thinking_enabled: bool,
    #[serde(default = "default_true")]
    use_system_proxy: bool,
    restore_clipboard_after_paste: bool,
    #[serde(default)]
    paste_shortcut: PasteShortcut,
    allow_non_tsf_insertion_fallback: bool,
    #[serde(default)]
    windows_insertion_mode: WindowsInsertionMode,
    #[serde(
        default,
        rename = "windowsSendInputNewlineMode",
        alias = "windowsSendinputNewlineMode"
    )]
    windows_sendinput_newline_mode: WindowsSendInputNewlineMode,
    #[serde(default)]
    macos_newline_mode: MacosNewlineMode,
    #[serde(
        default,
        rename = "windowsSendInputInsertionOnly",
        alias = "windowsSendinputInsertionOnly"
    )]
    windows_sendinput_insertion_only: bool,
    #[serde(default = "default_true", rename = "windowsShowOpenlessInKeyboardList")]
    windows_show_openless_in_keyboard_list: bool,
    working_languages: Vec<String>,
    translation_target_language: String,
    chinese_script_preference: ChineseScriptPreference,
    #[serde(default)]
    output_language_preference: OutputLanguagePreference,
    qa_hotkey: Option<ShortcutBinding>,
    #[serde(default)]
    quick_note_hotkey: Option<ShortcutBinding>,
    /// Outer `None` means the field was absent in a pre-Selection-Polish file;
    /// `Some(None)` means the user explicitly disabled it.
    #[serde(default, deserialize_with = "deserialize_selection_polish_hotkey")]
    selection_polish_hotkey: Option<Option<ShortcutBinding>>,
    #[serde(default = "default_active_style_pack_id")]
    selection_polish_style_pack_id: String,
    #[serde(default)]
    selection_polish_output_mode: SelectionPolishOutputMode,
    #[serde(default)]
    selection_voice_enabled: bool,
    #[serde(default)]
    selection_voice_intent_mode: SelectionVoiceIntentMode,
    #[serde(default)]
    selection_voice_manual_intent: SelectionVoiceManualIntent,
    #[serde(default = "default_selection_voice_edit_keywords")]
    selection_voice_edit_keywords: Vec<String>,
    #[serde(default)]
    selection_voice_edit_plan_format: crate::edit_plan::EditPlanFormat,
    #[serde(default)]
    selection_voice_edit_system_prompt: String,
    qa_save_history: bool,
    custom_combo_hotkey: Option<ComboBinding>,
    translation_hotkey: Option<ShortcutBinding>,
    switch_style_hotkey: Option<ShortcutBinding>,
    open_app_hotkey: Option<ShortcutBinding>,
    #[serde(default)]
    style_pack_hotkeys: Vec<StylePackHotkey>,
    #[serde(default)]
    coding_agent_enabled: bool,
    #[serde(default = "default_coding_agent_provider")]
    coding_agent_provider: String,
    #[serde(default)]
    coding_agent_model: Option<String>,
    #[serde(default = "default_coding_agent_permission_mode")]
    coding_agent_permission_mode: String,
    #[serde(default)]
    coding_agent_workdir: Option<String>,
    #[serde(default)]
    coding_agent_exe: Option<String>,
    #[serde(default = "default_coding_agent_voice_hotkey")]
    coding_agent_voice_hotkey: Option<ShortcutBinding>,
    #[serde(default = "default_coding_agent_panel_hotkey")]
    coding_agent_panel_hotkey: Option<ShortcutBinding>,
    #[serde(default)]
    coding_agent_quick_hotkey: Option<ShortcutBinding>,
    #[serde(default)]
    remote_input_enabled: bool,
    #[serde(default = "default_remote_input_port")]
    remote_input_port: u16,
    #[serde(default)]
    remote_input_pin: String,
    #[serde(default = "default_remote_input_mode")]
    remote_input_default_mode: String,
    #[serde(default = "default_local_asr_model")]
    local_asr_active_model: String,
    /// `None` preserves "the field was absent in the old config" for local ASR model preference migration.
    #[serde(default)]
    local_whisper_active_model: Option<String>,
    #[serde(default = "default_local_asr_mirror")]
    local_asr_mirror: String,
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    local_asr_keep_loaded_secs: u32,
    #[serde(default)]
    local_asr_models_base_dir: String,
    #[serde(default = "default_foundry_local_asr_model")]
    foundry_local_asr_model: String,
    #[serde(default = "default_foundry_local_runtime_source")]
    foundry_local_runtime_source: String,
    #[serde(default)]
    foundry_local_asr_language_hint: String,
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    foundry_local_asr_keep_loaded_secs: u32,
    #[serde(default = "default_sherpa_onnx_model")]
    sherpa_onnx_model: String,
    #[serde(default)]
    sherpa_onnx_language_hint: String,
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    sherpa_onnx_keep_loaded_secs: u32,
    #[serde(default)]
    update_channel: UpdateChannel,
    #[serde(default)]
    update_channel_explicit: Option<bool>,
    #[serde(default = "default_history_retention_days")]
    history_retention_days: u32,
    #[serde(default = "default_polish_context_window_minutes")]
    polish_context_window_minutes: u32,
    #[serde(default)]
    start_minimized: bool,
    #[serde(default)]
    theme_mode: ThemeMode,
    #[serde(default = "default_true")]
    streaming_insert: bool,
    #[serde(default)]
    streaming_insert_default_migrated: bool,
    #[serde(default = "default_true")]
    streaming_insert_save_clipboard: bool,
    #[serde(default)]
    cursor_context_enabled: bool,
    #[serde(default)]
    vocabulary_learning_enabled: bool,
    vocabulary_learning_settings: VocabularyLearningSettings,
    #[serde(default = "default_true")]
    show_overview_activity_heatmap: bool,
    #[serde(default)]
    stacked_row_layout: bool,
    #[serde(default)]
    conservative_layout: bool,
    #[serde(default = "default_true")]
    auto_update_check: bool,
    #[serde(default)]
    history_max_entries: Option<u32>,
    #[serde(default)]
    record_audio_for_debug: bool,
    #[serde(default)]
    audio_recording_max_entries: Option<u32>,
    #[serde(default)]
    quick_note_export_directory: String,
    #[serde(default)]
    marketplace_base_url: String,
    #[serde(default)]
    marketplace_dev_login: String,
    #[serde(default = "default_android_insert_strategy")]
    android_insert_strategy: AndroidInsertStrategy,
    #[serde(default = "default_android_overlay_trigger")]
    android_overlay_trigger: AndroidOverlayTrigger,
    #[serde(default = "default_android_overlay_activation_mode")]
    android_overlay_activation_mode: AndroidOverlayActivationMode,
    #[serde(default = "default_android_overlay_left_swipe_action")]
    android_overlay_left_swipe_action: AndroidOverlayLeftSwipeAction,
    #[serde(default = "default_android_overlay_cancel_swipe_direction")]
    android_overlay_cancel_swipe_direction: AndroidOverlayCancelSwipeDirection,
    #[serde(default)]
    android_overlay_gesture_actions: Option<AndroidOverlayGestureActions>,
    #[serde(default = "default_android_overlay_size_dp")]
    android_overlay_size_dp: u32,
    #[serde(default)]
    splash_seen_version: String,
}

fn deserialize_selection_polish_hotkey<'de, D>(
    deserializer: D,
) -> Result<Option<Option<ShortcutBinding>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // A nested Option normally collapses an explicit JSON `null` and a missing
    // field into the same value. Keep the outer Option as a presence marker so
    // users can actually disable this shortcut and legacy files can migrate.
    Option::<ShortcutBinding>::deserialize(deserializer).map(Some)
}

/// Migrates the legacy shared `localAsrActiveModel` into the separate Qwen /
/// Whisper preferences.
///
/// The old field was shared by both providers, so a straight string copy is
/// wrong: a Qwen legacy value leaves Whisper at its default; a legacy value
/// mistakenly stored as a Whisper id migrates to Whisper and Qwen resets to
/// its default. When the new field exists explicitly it wins, but only
/// Whisper model ids are accepted.
fn migrate_local_asr_models(
    legacy_model: String,
    whisper_model: Option<String>,
) -> (String, String) {
    let legacy_id = crate::local_asr_catalog::LocalAsrModelId::from_wire_id(&legacy_model);
    let qwen_model = legacy_id
        .filter(|id| id.is_qwen())
        .map(|id| id.as_str().to_string())
        .unwrap_or_else(default_local_asr_model);
    let migrated_whisper = match whisper_model {
        Some(model) => crate::local_asr_catalog::LocalAsrModelId::from_wire_id(&model)
            .filter(|id| id.is_whisper())
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(default_local_whisper_model),
        None => legacy_id
            .filter(|id| id.is_whisper())
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(default_local_whisper_model),
    };
    (qwen_model, migrated_whisper)
}

impl Default for UserPreferencesWire {
    fn default() -> Self {
        let prefs = UserPreferences::default();
        Self {
            hotkey: prefs.hotkey,
            dictation_hotkey: None,
            default_mode: prefs.default_mode,
            enabled_modes: prefs.enabled_modes,
            active_style_pack_id: Some(prefs.active_style_pack_id),
            style_system_prompts: prefs.style_system_prompts,
            custom_style_prompts: prefs.custom_style_prompts,
            launch_at_login: prefs.launch_at_login,
            show_capsule: prefs.show_capsule,
            capsule_style: prefs.capsule_style,
            capsule_transcript_enabled: prefs.capsule_transcript_enabled,
            capsule_transcript_font_size: prefs.capsule_transcript_font_size,
            mute_during_recording: prefs.mute_during_recording,
            stable_transcription_enabled: prefs.stable_transcription_enabled,
            audio_cue_on_record: prefs.audio_cue_on_record,
            silence_auto_stop_enabled: prefs.silence_auto_stop_enabled,
            silence_auto_stop_seconds: prefs.silence_auto_stop_seconds,
            microphone_device_name: prefs.microphone_device_name,
            active_asr_provider: prefs.active_asr_provider,
            active_llm_provider: prefs.active_llm_provider,
            pipeline_mode: prefs.pipeline_mode,
            multimodal_pipeline_enabled: prefs.multimodal_pipeline_enabled,
            active_omni_provider: prefs.active_omni_provider,
            llm_thinking_enabled: prefs.llm_thinking_enabled,
            use_system_proxy: prefs.use_system_proxy,
            restore_clipboard_after_paste: prefs.restore_clipboard_after_paste,
            paste_shortcut: prefs.paste_shortcut,
            allow_non_tsf_insertion_fallback: prefs.allow_non_tsf_insertion_fallback,
            windows_insertion_mode: prefs.windows_insertion_mode,
            windows_sendinput_newline_mode: prefs.windows_sendinput_newline_mode,
            macos_newline_mode: prefs.macos_newline_mode,
            windows_sendinput_insertion_only: prefs.windows_sendinput_insertion_only,
            windows_show_openless_in_keyboard_list: prefs.windows_show_openless_in_keyboard_list,
            working_languages: prefs.working_languages,
            translation_target_language: prefs.translation_target_language,
            chinese_script_preference: prefs.chinese_script_preference,
            output_language_preference: prefs.output_language_preference,
            qa_hotkey: prefs.qa_hotkey,
            quick_note_hotkey: prefs.quick_note_hotkey,
            selection_polish_hotkey: None,
            selection_polish_style_pack_id: prefs.selection_polish_style_pack_id,
            selection_polish_output_mode: prefs.selection_polish_output_mode,
            selection_voice_enabled: prefs.selection_voice_enabled,
            selection_voice_intent_mode: prefs.selection_voice_intent_mode,
            selection_voice_manual_intent: prefs.selection_voice_manual_intent,
            selection_voice_edit_keywords: prefs.selection_voice_edit_keywords,
            selection_voice_edit_plan_format: prefs.selection_voice_edit_plan_format,
            selection_voice_edit_system_prompt: prefs.selection_voice_edit_system_prompt,
            qa_save_history: prefs.qa_save_history,
            custom_combo_hotkey: prefs.custom_combo_hotkey,
            translation_hotkey: None,
            // Carry the default key (Some) so a missing field still means enabled; None exclusively means "user disabled".
            switch_style_hotkey: prefs.switch_style_hotkey,
            open_app_hotkey: prefs.open_app_hotkey,
            style_pack_hotkeys: prefs.style_pack_hotkeys,
            coding_agent_enabled: prefs.coding_agent_enabled,
            coding_agent_provider: prefs.coding_agent_provider,
            coding_agent_model: prefs.coding_agent_model,
            coding_agent_permission_mode: prefs.coding_agent_permission_mode,
            coding_agent_workdir: prefs.coding_agent_workdir,
            coding_agent_exe: prefs.coding_agent_exe,
            coding_agent_voice_hotkey: prefs.coding_agent_voice_hotkey,
            coding_agent_panel_hotkey: prefs.coding_agent_panel_hotkey,
            coding_agent_quick_hotkey: prefs.coding_agent_quick_hotkey,
            remote_input_enabled: prefs.remote_input_enabled,
            remote_input_port: prefs.remote_input_port,
            remote_input_pin: prefs.remote_input_pin,
            remote_input_default_mode: prefs.remote_input_default_mode,
            local_asr_active_model: prefs.local_asr_active_model,
            // New fields must stay None: deserializing old configs needs to distinguish "field missing" from an explicit value.
            local_whisper_active_model: None,
            local_asr_mirror: prefs.local_asr_mirror,
            local_asr_keep_loaded_secs: prefs.local_asr_keep_loaded_secs,
            local_asr_models_base_dir: prefs.local_asr_models_base_dir,
            foundry_local_asr_model: prefs.foundry_local_asr_model,
            foundry_local_runtime_source: prefs.foundry_local_runtime_source,
            foundry_local_asr_language_hint: prefs.foundry_local_asr_language_hint,
            foundry_local_asr_keep_loaded_secs: prefs.foundry_local_asr_keep_loaded_secs,
            sherpa_onnx_model: prefs.sherpa_onnx_model,
            sherpa_onnx_language_hint: prefs.sherpa_onnx_language_hint,
            sherpa_onnx_keep_loaded_secs: prefs.sherpa_onnx_keep_loaded_secs,
            update_channel: prefs.update_channel,
            // None preserves the fact that the old config lacked the marker; deserialization treats only historical Beta as explicit.
            update_channel_explicit: None,
            history_retention_days: prefs.history_retention_days,
            polish_context_window_minutes: prefs.polish_context_window_minutes,
            start_minimized: prefs.start_minimized,
            theme_mode: prefs.theme_mode,
            streaming_insert: prefs.streaming_insert,
            streaming_insert_default_migrated: prefs.streaming_insert_default_migrated,
            streaming_insert_save_clipboard: prefs.streaming_insert_save_clipboard,
            cursor_context_enabled: prefs.cursor_context_enabled,
            vocabulary_learning_enabled: prefs.vocabulary_learning_enabled,
            vocabulary_learning_settings: prefs.vocabulary_learning_settings,
            show_overview_activity_heatmap: prefs.show_overview_activity_heatmap,
            stacked_row_layout: prefs.stacked_row_layout,
            conservative_layout: prefs.conservative_layout,
            auto_update_check: prefs.auto_update_check,
            history_max_entries: prefs.history_max_entries,
            record_audio_for_debug: prefs.record_audio_for_debug,
            audio_recording_max_entries: prefs.audio_recording_max_entries,
            quick_note_export_directory: prefs.quick_note_export_directory.clone(),
            marketplace_base_url: prefs.marketplace_base_url,
            marketplace_dev_login: prefs.marketplace_dev_login,
            android_insert_strategy: prefs.android_insert_strategy,
            android_overlay_trigger: prefs.android_overlay_trigger,
            android_overlay_activation_mode: prefs.android_overlay_activation_mode,
            android_overlay_left_swipe_action: prefs.android_overlay_left_swipe_action,
            android_overlay_cancel_swipe_direction: prefs.android_overlay_cancel_swipe_direction,
            android_overlay_gesture_actions: None,
            android_overlay_size_dp: prefs.android_overlay_size_dp,
            splash_seen_version: prefs.splash_seen_version,
        }
    }
}

impl<'de> Deserialize<'de> for UserPreferences {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = UserPreferencesWire::deserialize(deserializer)?;
        let dictation_hotkey = match wire.dictation_hotkey {
            Some(binding) => binding,
            None => default_dictation_hotkey_from_legacy(&wire.hotkey, &wire.custom_combo_hotkey)
                .map_err(serde::de::Error::custom)?,
        };
        let selection_polish_hotkey_was_missing = wire.selection_polish_hotkey.is_none();
        let mut selection_polish_hotkey = wire
            .selection_polish_hotkey
            .unwrap_or_else(default_selection_polish_hotkey);
        if selection_polish_hotkey_was_missing {
            // The 1.3.15 selection-polish default key (Windows = right Alt)
            // must not steal or collide with an existing user binding:
            // - Users who never customized the recording key (still the
            //   historical Right Control default): keep the new feature off
            //   so the right Alt global key doesn't disturb existing habits;
            // - Overlap with the recording key (strings may differ but the
            //   physical key matches, e.g. legacy rightAlt derives
            //   RightOption while the default is RightAlt): disable as well,
            //   otherwise every settings save is rejected by hotkey-conflict
            //   validation and all changes are lost (#904).
            let legacy_default_user = cfg!(target_os = "windows")
                && is_right_control_modifier_shortcut(&dictation_hotkey);
            let default_taken_by_dictation =
                selection_polish_hotkey.as_ref().is_some_and(|binding| {
                    crate::shortcut_types::bindings_overlap(binding, &dictation_hotkey)
                });
            if legacy_default_user || default_taken_by_dictation {
                selection_polish_hotkey = None;
            }
        }
        let streaming_insert_default_migrated = wire.streaming_insert_default_migrated;
        let streaming_insert = if streaming_insert_default_migrated {
            wire.streaming_insert
        } else {
            true
        };
        let (local_asr_active_model, local_whisper_active_model) =
            migrate_local_asr_models(wire.local_asr_active_model, wire.local_whisper_active_model);
        let update_channel_explicit = wire
            .update_channel_explicit
            .unwrap_or(matches!(wire.update_channel, UpdateChannel::Beta));
        let android_overlay_gesture_actions =
            wire.android_overlay_gesture_actions
                .unwrap_or_else(|| AndroidOverlayGestureActions {
                    up: if wire.android_overlay_cancel_swipe_direction
                        == AndroidOverlayCancelSwipeDirection::Up
                    {
                        AndroidOverlayGestureAction::Cancel
                    } else {
                        AndroidOverlayGestureAction::None
                    },
                    down: if wire.android_overlay_cancel_swipe_direction
                        == AndroidOverlayCancelSwipeDirection::Down
                    {
                        AndroidOverlayGestureAction::Cancel
                    } else {
                        AndroidOverlayGestureAction::None
                    },
                    left: match wire.android_overlay_left_swipe_action {
                        AndroidOverlayLeftSwipeAction::Translation => {
                            AndroidOverlayGestureAction::Translation
                        }
                        AndroidOverlayLeftSwipeAction::StylePack => {
                            AndroidOverlayGestureAction::StylePack
                        }
                    },
                    right: AndroidOverlayGestureAction::Qa,
                });

        Ok(Self {
            hotkey: wire.hotkey,
            dictation_hotkey,
            default_mode: wire.default_mode,
            enabled_modes: wire.enabled_modes,
            active_style_pack_id: wire
                .active_style_pack_id
                .filter(|id| !id.trim().is_empty())
                .unwrap_or_else(|| builtin_style_pack_id(wire.default_mode).to_string()),
            style_system_prompts: wire
                .style_system_prompts
                .with_legacy_custom_prompts(&wire.custom_style_prompts),
            custom_style_prompts: wire.custom_style_prompts,
            launch_at_login: wire.launch_at_login,
            show_capsule: wire.show_capsule,
            capsule_style: wire.capsule_style,
            capsule_transcript_enabled: wire.capsule_transcript_enabled,
            capsule_transcript_font_size: wire.capsule_transcript_font_size.clamp(12, 20),
            mute_during_recording: wire.mute_during_recording,
            stable_transcription_enabled: wire.stable_transcription_enabled,
            audio_cue_on_record: wire.audio_cue_on_record,
            silence_auto_stop_enabled: wire.silence_auto_stop_enabled,
            silence_auto_stop_seconds: wire.silence_auto_stop_seconds,
            microphone_device_name: wire.microphone_device_name,
            active_asr_provider: wire.active_asr_provider,
            active_llm_provider: wire.active_llm_provider,
            pipeline_mode: wire.pipeline_mode,
            multimodal_pipeline_enabled: wire.multimodal_pipeline_enabled,
            active_omni_provider: wire.active_omni_provider,
            llm_thinking_enabled: wire.llm_thinking_enabled,
            use_system_proxy: wire.use_system_proxy,
            restore_clipboard_after_paste: wire.restore_clipboard_after_paste,
            paste_shortcut: wire.paste_shortcut,
            allow_non_tsf_insertion_fallback: wire.allow_non_tsf_insertion_fallback,
            windows_insertion_mode: resolve_windows_insertion_mode(
                wire.windows_insertion_mode,
                wire.windows_sendinput_insertion_only,
            ),
            windows_sendinput_newline_mode: wire.windows_sendinput_newline_mode,
            macos_newline_mode: wire.macos_newline_mode,
            windows_sendinput_insertion_only: resolve_windows_sendinput_insertion_only_legacy(
                wire.windows_insertion_mode,
                wire.windows_sendinput_insertion_only,
            ),
            windows_show_openless_in_keyboard_list: wire.windows_show_openless_in_keyboard_list,
            working_languages: wire.working_languages,
            translation_target_language: wire.translation_target_language,
            chinese_script_preference: wire.chinese_script_preference,
            output_language_preference: wire.output_language_preference,
            qa_hotkey: wire.qa_hotkey,
            quick_note_hotkey: wire.quick_note_hotkey,
            selection_polish_hotkey,
            selection_polish_style_pack_id: wire.selection_polish_style_pack_id,
            selection_polish_output_mode: wire.selection_polish_output_mode,
            selection_voice_enabled: wire.selection_voice_enabled,
            selection_voice_intent_mode: wire.selection_voice_intent_mode,
            selection_voice_manual_intent: wire.selection_voice_manual_intent,
            selection_voice_edit_keywords: wire.selection_voice_edit_keywords,
            selection_voice_edit_plan_format: wire.selection_voice_edit_plan_format,
            selection_voice_edit_system_prompt: wire.selection_voice_edit_system_prompt,
            qa_save_history: wire.qa_save_history,
            coding_agent_enabled: wire.coding_agent_enabled,
            coding_agent_provider: wire.coding_agent_provider,
            coding_agent_model: wire.coding_agent_model,
            coding_agent_permission_mode: wire.coding_agent_permission_mode,
            coding_agent_workdir: wire.coding_agent_workdir,
            coding_agent_exe: wire.coding_agent_exe,
            coding_agent_voice_hotkey: wire.coding_agent_voice_hotkey,
            coding_agent_panel_hotkey: wire.coding_agent_panel_hotkey,
            coding_agent_quick_hotkey: wire.coding_agent_quick_hotkey,
            remote_input_enabled: wire.remote_input_enabled,
            remote_input_port: wire.remote_input_port,
            remote_input_pin: wire.remote_input_pin,
            remote_input_default_mode: wire.remote_input_default_mode,
            custom_combo_hotkey: wire.custom_combo_hotkey,
            translation_hotkey: wire
                .translation_hotkey
                .unwrap_or_else(default_translation_hotkey),
            // Pass the Option through: None = user disabled — no
            // unwrap_or_else collapse back to the default key (the root
            // cause of #576 "can't disable"). A missing field falls to the
            // wire's serde struct-default Some(default key), so old and new
            // users stay enabled.
            switch_style_hotkey: wire.switch_style_hotkey,
            open_app_hotkey: wire.open_app_hotkey,
            style_pack_hotkeys: wire.style_pack_hotkeys,
            local_asr_active_model,
            local_whisper_active_model,
            local_asr_mirror: wire.local_asr_mirror,
            local_asr_keep_loaded_secs: wire.local_asr_keep_loaded_secs,
            local_asr_models_base_dir: wire.local_asr_models_base_dir,
            foundry_local_asr_model: wire.foundry_local_asr_model,
            foundry_local_runtime_source:
                crate::local_asr_catalog::normalize_foundry_runtime_source(
                    &wire.foundry_local_runtime_source,
                ),
            foundry_local_asr_language_hint: wire.foundry_local_asr_language_hint,
            foundry_local_asr_keep_loaded_secs: wire.foundry_local_asr_keep_loaded_secs,
            sherpa_onnx_model: wire.sherpa_onnx_model,
            sherpa_onnx_language_hint: wire.sherpa_onnx_language_hint,
            sherpa_onnx_keep_loaded_secs: wire.sherpa_onnx_keep_loaded_secs,
            update_channel: wire.update_channel,
            update_channel_explicit,
            history_retention_days: wire.history_retention_days,
            polish_context_window_minutes: wire.polish_context_window_minutes,
            start_minimized: wire.start_minimized,
            theme_mode: wire.theme_mode,
            streaming_insert,
            streaming_insert_default_migrated: true,
            streaming_insert_save_clipboard: wire.streaming_insert_save_clipboard,
            cursor_context_enabled: wire.cursor_context_enabled,
            vocabulary_learning_enabled: wire.vocabulary_learning_enabled,
            vocabulary_learning_settings: wire.vocabulary_learning_settings.normalized(),
            show_overview_activity_heatmap: wire.show_overview_activity_heatmap,
            stacked_row_layout: wire.stacked_row_layout,
            conservative_layout: wire.conservative_layout,
            auto_update_check: wire.auto_update_check,
            history_max_entries: wire.history_max_entries,
            record_audio_for_debug: wire.record_audio_for_debug,
            audio_recording_max_entries: wire.audio_recording_max_entries,
            quick_note_export_directory: wire.quick_note_export_directory,
            marketplace_base_url: wire.marketplace_base_url,
            marketplace_dev_login: wire.marketplace_dev_login,
            android_insert_strategy: normalize_android_insert_strategy(
                wire.android_insert_strategy,
            ),
            android_overlay_trigger: wire.android_overlay_trigger.normalized(),
            android_overlay_activation_mode: wire.android_overlay_activation_mode,
            android_overlay_left_swipe_action: wire.android_overlay_left_swipe_action,
            android_overlay_cancel_swipe_direction: wire.android_overlay_cancel_swipe_direction,
            android_overlay_gesture_actions,
            android_overlay_size_dp: normalize_android_overlay_size_dp(
                wire.android_overlay_size_dp,
            ),
            splash_seen_version: wire.splash_seen_version,
        })
    }
}

impl UserPreferences {
    /// Field-by-field rescue of a preferences.json that fails strict
    /// deserialization.
    ///
    /// `UserPreferencesWire`'s container-level `#[serde(default)]` already
    /// tolerates missing fields (old files read by a newer version). What
    /// actually fails the whole parse — and silently falls back to defaults,
    /// wiping every user setting at once — is a field that exists with an
    /// invalid value, e.g. after a refactor renames an enum variant or
    /// changes a field type. This is the root cause of "settings unreadable
    /// after reinstall" reports.
    ///
    /// Strategy: parse the JSON as an object, normalize known aliases, then
    /// try each key in isolation. Since Wire defaults every field, a
    /// single-key object `{k: v}` fails only when `v` is invalid for field
    /// `k` — so bad fields can be dropped precisely while all valid settings
    /// (hotkeys, model choices, styles, ...) survive, followed by one normal
    /// deserialization. Only input that isn't a JSON object falls back to
    /// defaults entirely.
    pub fn salvage_from_json_bytes(bytes: &[u8]) -> Self {
        let Ok(serde_json::Value::Object(mut map)) =
            serde_json::from_slice::<serde_json::Value>(bytes)
        else {
            return Self::default();
        };

        normalize_preference_aliases(&mut map);

        let mut cleaned = serde_json::Map::new();
        for (key, value) in map {
            if preference_field_is_valid(&key, &value) {
                cleaned.insert(key, value);
            } else {
                log::warn!("[prefs] salvage dropping unparseable field: {key}");
            }
        }

        match serde_json::from_value::<Self>(serde_json::Value::Object(cleaned.clone())) {
            Ok(prefs) => prefs,
            Err(err) => {
                if let Some(prefs) = salvage_without_incomplete_legacy_hotkey(cleaned) {
                    return prefs;
                }
                log::warn!(
                    "[prefs] salvage still failed after field filtering: {err}; using defaults"
                );
                Self::default()
            }
        }
    }
}

fn preference_field_is_valid(key: &str, value: &serde_json::Value) -> bool {
    let probe =
        serde_json::Value::Object(std::iter::once((key.to_string(), value.clone())).collect());
    serde_json::from_value::<UserPreferencesWire>(probe).is_ok()
}

fn normalize_preference_aliases(map: &mut serde_json::Map<String, serde_json::Value>) {
    for (canonical, alias) in [
        ("windowsSendInputNewlineMode", "windowsSendinputNewlineMode"),
        (
            "windowsSendInputInsertionOnly",
            "windowsSendinputInsertionOnly",
        ),
    ] {
        let Some(alias_value) = map.remove(alias) else {
            continue;
        };
        let canonical_valid = map
            .get(canonical)
            .map(|value| preference_field_is_valid(canonical, value));
        let alias_valid = preference_field_is_valid(canonical, &alias_value);

        match canonical_valid {
            None => {
                map.insert(canonical.to_string(), alias_value);
            }
            Some(true) => log::warn!(
                "[prefs] salvage dropping duplicate legacy alias {alias}; canonical {canonical} wins"
            ),
            Some(false) if alias_valid => {
                log::warn!(
                    "[prefs] salvage replacing invalid canonical {canonical} with valid legacy alias {alias}"
                );
                map.insert(canonical.to_string(), alias_value);
            }
            Some(false) => {}
        }
    }
}

fn salvage_without_incomplete_legacy_hotkey(
    mut map: serde_json::Map<String, serde_json::Value>,
) -> Option<UserPreferences> {
    let is_custom_legacy_hotkey = map
        .get("hotkey")
        .and_then(|value| value.get("trigger"))
        .and_then(serde_json::Value::as_str)
        == Some("custom");
    if !is_custom_legacy_hotkey {
        return None;
    }

    let has_dictation_hotkey = map
        .get("dictationHotkey")
        .and_then(|value| serde_json::from_value::<Option<ShortcutBinding>>(value.clone()).ok())
        .flatten()
        .is_some();
    let has_custom_combo_hotkey = map
        .get("customComboHotkey")
        .and_then(|value| serde_json::from_value::<Option<ComboBinding>>(value.clone()).ok())
        .flatten()
        .is_some();
    if has_dictation_hotkey || has_custom_combo_hotkey {
        return None;
    }

    map.remove("hotkey");
    serde_json::from_value::<UserPreferences>(serde_json::Value::Object(map)).ok()
}

fn default_qa_hotkey() -> Option<ShortcutBinding> {
    Some(ShortcutBinding::default_qa())
}

fn default_selection_polish_hotkey() -> Option<ShortcutBinding> {
    #[cfg(target_os = "windows")]
    {
        // Right Alt on Windows; other platforms default to off to avoid clashing with the historical dictation default key.
        Some(ShortcutBinding {
            primary: "RightAlt".into(),
            modifiers: Vec::new(),
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn default_selection_voice_edit_keywords() -> Vec<String> {
    // Pre-#987 defaults were edit imperatives; interrogative routing treats these
    // as extra question cues — empty default avoids misrouting e.g. "change this".
    Vec::new()
}

fn is_right_control_modifier_shortcut(binding: &ShortcutBinding) -> bool {
    binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("RightControl")
}

fn default_coding_agent_provider() -> String {
    "claude-code-cli".to_string()
}

fn default_coding_agent_permission_mode() -> String {
    "acceptEdits".to_string()
}

pub(crate) fn default_coding_agent_voice_hotkey() -> Option<ShortcutBinding> {
    Some(ShortcutBinding {
        primary: "LeftControl".into(),
        modifiers: Vec::new(),
    })
}

pub(crate) fn default_coding_agent_panel_hotkey() -> Option<ShortcutBinding> {
    Some(ShortcutBinding {
        primary: "Enter".into(),
        modifiers: vec!["cmd".into(), "shift".into()],
    })
}

fn default_translation_hotkey() -> ShortcutBinding {
    ShortcutBinding {
        primary: "Shift".into(),
        modifiers: Vec::new(),
    }
}

fn default_switch_style_hotkey() -> Option<ShortcutBinding> {
    Some(ShortcutBinding {
        primary: "S".into(),
        modifiers: default_app_shortcut_modifiers(),
    })
}

fn default_open_app_hotkey() -> Option<ShortcutBinding> {
    Some(ShortcutBinding {
        primary: "O".into(),
        modifiers: default_app_shortcut_modifiers(),
    })
}

fn default_app_shortcut_modifiers() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        vec!["cmd".into(), "shift".into()]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec!["ctrl".into(), "shift".into()]
    }
}

fn default_dictation_hotkey_from_legacy(
    hotkey: &HotkeyBinding,
    custom_combo_hotkey: &Option<ComboBinding>,
) -> Result<ShortcutBinding, String> {
    if hotkey.trigger == HotkeyTrigger::Custom {
        if let Some(combo) = custom_combo_hotkey {
            return Ok(ShortcutBinding {
                primary: combo.primary.clone(),
                modifiers: combo.modifiers.clone(),
            });
        }
        return Err(
            "hotkey.trigger is custom but dictationHotkey/customComboHotkey is missing".into(),
        );
    }
    Ok(crate::shortcut_types::binding_from_legacy_trigger(
        hotkey.trigger,
    ))
}

fn default_working_languages() -> Vec<String> {
    vec!["简体中文".into()]
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding::default(),
            dictation_hotkey: default_dictation_hotkey_from_legacy(
                &HotkeyBinding::default(),
                &None,
            )
            .expect("default legacy hotkey is not custom"),
            default_mode: PolishMode::Structured,
            enabled_modes: vec![
                PolishMode::Raw,
                PolishMode::Light,
                PolishMode::Structured,
                PolishMode::Formal,
            ],
            active_style_pack_id: default_active_style_pack_id(),
            style_system_prompts: StyleSystemPrompts::default(),
            custom_style_prompts: CustomStylePrompts::default(),
            launch_at_login: false,
            show_capsule: true,
            capsule_style: CapsuleStyle::Siri,
            capsule_transcript_enabled: true,
            capsule_transcript_font_size: default_capsule_transcript_font_size(),
            mute_during_recording: false,
            stable_transcription_enabled: false,
            audio_cue_on_record: true,
            silence_auto_stop_enabled: false,
            silence_auto_stop_seconds: default_silence_auto_stop_seconds(),
            microphone_device_name: String::new(),
            active_asr_provider: default_active_asr_provider(),
            active_llm_provider: "ark".into(),
            pipeline_mode: PipelineMode::Traditional,
            multimodal_pipeline_enabled: false,
            active_omni_provider: "custom".into(),
            llm_thinking_enabled: false,
            use_system_proxy: true,
            restore_clipboard_after_paste: true,
            paste_shortcut: PasteShortcut::default(),
            allow_non_tsf_insertion_fallback: true,
            windows_insertion_mode: WindowsInsertionMode::default(),
            windows_sendinput_newline_mode: WindowsSendInputNewlineMode::default(),
            macos_newline_mode: MacosNewlineMode::default(),
            windows_sendinput_insertion_only: false,
            windows_show_openless_in_keyboard_list: true,
            working_languages: default_working_languages(),
            translation_target_language: String::new(),
            chinese_script_preference: ChineseScriptPreference::Auto,
            output_language_preference: OutputLanguagePreference::Auto,
            qa_hotkey: default_qa_hotkey(),
            quick_note_hotkey: None,
            selection_polish_hotkey: default_selection_polish_hotkey(),
            selection_polish_style_pack_id: default_active_style_pack_id(),
            selection_polish_output_mode: SelectionPolishOutputMode::default(),
            selection_voice_enabled: false,
            selection_voice_intent_mode: SelectionVoiceIntentMode::default(),
            selection_voice_manual_intent: SelectionVoiceManualIntent::default(),
            selection_voice_edit_keywords: default_selection_voice_edit_keywords(),
            selection_voice_edit_plan_format: crate::edit_plan::EditPlanFormat::default(),
            selection_voice_edit_system_prompt: String::new(),
            qa_save_history: false,
            custom_combo_hotkey: None,
            translation_hotkey: default_translation_hotkey(),
            switch_style_hotkey: default_switch_style_hotkey(),
            open_app_hotkey: default_open_app_hotkey(),
            style_pack_hotkeys: Vec::new(),
            coding_agent_enabled: false,
            coding_agent_provider: default_coding_agent_provider(),
            coding_agent_model: None,
            coding_agent_permission_mode: default_coding_agent_permission_mode(),
            coding_agent_workdir: None,
            coding_agent_exe: None,
            coding_agent_voice_hotkey: default_coding_agent_voice_hotkey(),
            coding_agent_panel_hotkey: default_coding_agent_panel_hotkey(),
            coding_agent_quick_hotkey: None,
            remote_input_enabled: false,
            remote_input_port: default_remote_input_port(),
            remote_input_pin: String::new(),
            remote_input_default_mode: default_remote_input_mode(),
            local_asr_active_model: default_local_asr_model(),
            local_whisper_active_model: default_local_whisper_model(),
            local_asr_mirror: default_local_asr_mirror(),
            local_asr_keep_loaded_secs: default_local_asr_keep_loaded_secs(),
            local_asr_models_base_dir: String::new(),
            foundry_local_asr_model: default_foundry_local_asr_model(),
            foundry_local_runtime_source: default_foundry_local_runtime_source(),
            foundry_local_asr_language_hint: String::new(),
            foundry_local_asr_keep_loaded_secs: default_local_asr_keep_loaded_secs(),
            sherpa_onnx_model: default_sherpa_onnx_model(),
            sherpa_onnx_language_hint: String::new(),
            sherpa_onnx_keep_loaded_secs: default_local_asr_keep_loaded_secs(),
            update_channel: UpdateChannel::default(),
            update_channel_explicit: false,
            history_retention_days: default_history_retention_days(),
            polish_context_window_minutes: default_polish_context_window_minutes(),
            start_minimized: false,
            theme_mode: ThemeMode::default(),
            streaming_insert: true,
            streaming_insert_default_migrated: true,
            streaming_insert_save_clipboard: true,
            cursor_context_enabled: false,
            vocabulary_learning_enabled: false,
            vocabulary_learning_settings: VocabularyLearningSettings::default(),
            show_overview_activity_heatmap: true,
            stacked_row_layout: false,
            conservative_layout: false,
            auto_update_check: true,
            history_max_entries: None,
            record_audio_for_debug: false,
            audio_recording_max_entries: None,
            quick_note_export_directory: String::new(),
            marketplace_base_url: String::new(),
            marketplace_dev_login: String::new(),
            android_insert_strategy: default_android_insert_strategy(),
            android_overlay_trigger: default_android_overlay_trigger(),
            android_overlay_activation_mode: default_android_overlay_activation_mode(),
            android_overlay_left_swipe_action: default_android_overlay_left_swipe_action(),
            android_overlay_cancel_swipe_direction: default_android_overlay_cancel_swipe_direction(
            ),
            android_overlay_gesture_actions: default_android_overlay_gesture_actions(),
            android_overlay_size_dp: default_android_overlay_size_dp(),
            splash_seen_version: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
}

/// Direct style-pack hotkey: pressing `binding` activates the style pack with id `pack_id` (issue #759).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StylePackHotkey {
    pub pack_id: String,
    pub binding: ShortcutBinding,
}

impl ShortcutBinding {
    pub fn default_qa() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                primary: ";".into(),
                modifiers: vec!["cmd".into(), "shift".into()],
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                primary: ";".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            }
        }
    }

    pub fn display_label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let modifier_order = ["cmd", "ctrl", "alt", "shift", "super"];
        for tag in modifier_order {
            if self.modifiers.iter().any(|m| m.eq_ignore_ascii_case(tag)) {
                parts.push(modifier_display(tag).to_string());
            }
        }
        parts.push(display_primary(&self.primary));
        parts.join("+")
    }
}

/// Global hotkey binding for selection voice QA. Native-name strings:
/// - `primary`: the primary key (e.g. `";"`, `"."`, `"A"`, `"F1"`).
/// - `modifiers`: modifier set, elements from
///   `{"cmd","ctrl","alt","shift","super"}`. Lowercase names serialize
///   plainly; frontend / backend parsing lowercases uniformly.
///
/// Default `Cmd+Shift+;` (macOS) / `Ctrl+Shift+;` (Windows).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QaHotkeyBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
}

impl Default for QaHotkeyBinding {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                primary: ";".into(),
                modifiers: vec!["cmd".into(), "shift".into()],
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                primary: ";".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            }
        }
    }
}

impl QaHotkeyBinding {
    /// Renders a readable label for the frontend.
    /// Order matches human reading habits: `Cmd+Shift+;`, `Ctrl+Alt+Shift+.`.
    pub fn display_label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        // Fixed output order: Ctrl/Cmd -> Alt/Option -> Shift -> Super
        let modifier_order = ["cmd", "ctrl", "alt", "shift", "super"];
        for tag in modifier_order {
            if self.modifiers.iter().any(|m| m.eq_ignore_ascii_case(tag)) {
                parts.push(modifier_display(tag).to_string());
            }
        }
        let key_label = display_primary(&self.primary);
        parts.push(key_label);
        parts.join("+")
    }
}

/// Custom combo binding for the recording hotkey. Same shape as
/// `QaHotkeyBinding`:
/// - `primary`: the primary key (e.g. `"D"`, `"Space"`, `"F1"`).
/// - `modifiers`: modifier set, elements from `{"cmd","ctrl","alt","shift","super"}`.
///
/// When `HotkeyBinding.trigger == Custom`, the coordinator registers this
/// combo via the `global-hotkey` crate instead of the modifier-only
/// CGEventTap / WH_KEYBOARD_LL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComboBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
}

impl ComboBinding {
    /// Renders a readable label for the frontend; reuses QaHotkeyBinding's formatting.
    pub fn display_label(&self) -> String {
        let qa = QaHotkeyBinding {
            primary: self.primary.clone(),
            modifiers: self.modifiers.clone(),
        };
        qa.display_label()
    }
}

fn modifier_display(tag: &str) -> &'static str {
    match tag {
        "cmd" => {
            #[cfg(target_os = "macos")]
            {
                "Cmd"
            }
            #[cfg(target_os = "windows")]
            {
                "Ctrl"
            }
            #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
            {
                "Super"
            }
        }
        "ctrl" => "Ctrl",
        "alt" => {
            #[cfg(target_os = "macos")]
            {
                "Option"
            }
            #[cfg(not(target_os = "macos"))]
            {
                "Alt"
            }
        }
        "shift" => "Shift",
        "super" => "Super",
        _ => "",
    }
}

fn display_primary(primary: &str) -> String {
    let trimmed = primary.trim();
    if trimmed.is_empty() {
        return "?".to_string();
    }
    // Single letter keys display uppercased ("a" -> "A"); everything else as-is (e.g. ";", "F1").
    if trimmed.chars().count() == 1 {
        let ch = trimmed.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            return ch.to_ascii_uppercase().to_string();
        }
    }
    trimmed.to_string()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyTrigger {
    RightOption,
    LeftOption,
    RightControl,
    LeftControl,
    RightCommand,
    LeftCommand,
    LeftShift,
    RightShift,
    Fn,
    RightAlt, // Windows synonym for RightOption
    MediaPlayPause,
    Custom,
}

impl HotkeyTrigger {
    pub fn display_name(&self) -> &'static str {
        match self {
            HotkeyTrigger::RightOption => "右 Option",
            HotkeyTrigger::LeftOption => "左 Option",
            HotkeyTrigger::RightControl => "右 Control",
            HotkeyTrigger::LeftControl => "左 Control",
            HotkeyTrigger::RightCommand => "右 Command",
            HotkeyTrigger::LeftCommand => "左 Command",
            HotkeyTrigger::LeftShift => "左 Shift",
            HotkeyTrigger::RightShift => "右 Shift",
            HotkeyTrigger::Fn => "Fn (地球键)",
            HotkeyTrigger::RightAlt => "右 Alt",
            HotkeyTrigger::MediaPlayPause => "⏯ Media 播放/暂停",
            HotkeyTrigger::Custom => "自定义组合键",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyMode {
    Toggle,
    Hold,
    DoubleClick,
    /// Auto-detect: recording starts on press; on release the hold duration
    /// decides the semantics — a short press (< AUTO_HOLD_THRESHOLD) acts as
    /// Toggle (latched, recording continues until the next press), a long
    /// press as Hold (release stops).
    Auto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyAdapterKind {
    MacEventTap,
    WindowsLowLevel,
    Fcitx5,
    /// Mobile platforms do not expose desktop global hotkey adapters.
    Unavailable,
}

impl HotkeyAdapterKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            HotkeyAdapterKind::MacEventTap => "macOS Event Tap",
            HotkeyAdapterKind::WindowsLowLevel => "Windows 低层键盘 hook",
            HotkeyAdapterKind::Fcitx5 => "fcitx5 输入法插件",
            HotkeyAdapterKind::Unavailable => "不可用",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyKey {
    pub code: String,
}

impl HotkeyKey {
    pub fn new(code: impl Into<String>) -> Self {
        Self { code: code.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct HotkeyBinding {
    pub trigger: HotkeyTrigger,
    pub mode: HotkeyMode,
    pub keys: Option<Vec<HotkeyKey>>,
}

impl HotkeyBinding {
    pub fn effective_codes(&self) -> Vec<String> {
        let Some(keys) = &self.keys else {
            let code = legacy_trigger_code(self.trigger);
            return if code.is_empty() {
                Vec::new()
            } else {
                vec![code.to_string()]
            };
        };
        keys.iter()
            .map(|key| key.code.trim().to_string())
            .filter(|code| !code.is_empty())
            .collect()
    }

    pub fn display_label(&self) -> String {
        let codes = self.effective_codes();
        if codes.is_empty() {
            return "未设置".to_string();
        }
        codes
            .iter()
            .map(|code| display_hotkey_code(code))
            .collect::<Vec<_>>()
            .join("+")
    }
}

fn legacy_trigger_code(trigger: HotkeyTrigger) -> &'static str {
    match trigger {
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => "AltRight",
        HotkeyTrigger::LeftOption => "AltLeft",
        HotkeyTrigger::RightControl => "ControlRight",
        HotkeyTrigger::LeftControl => "ControlLeft",
        HotkeyTrigger::RightCommand => "MetaRight",
        HotkeyTrigger::LeftCommand => "MetaLeft",
        HotkeyTrigger::LeftShift => "ShiftLeft",
        HotkeyTrigger::RightShift => "ShiftRight",
        #[cfg(target_os = "windows")]
        HotkeyTrigger::Fn => "ControlRight",
        #[cfg(not(target_os = "windows"))]
        HotkeyTrigger::Fn => "Fn",
        HotkeyTrigger::MediaPlayPause => "MediaPlayPause",
        HotkeyTrigger::Custom => "",
    }
}

fn display_hotkey_code(code: &str) -> String {
    let label = match code {
        "ControlLeft" => "左Ctrl",
        "ControlRight" => "右 Control",
        "AltLeft" => "左Alt",
        "AltRight" => "右Alt",
        "ShiftLeft" => "左Shift",
        "ShiftRight" => "右Shift",
        "MetaLeft" | "OSLeft" => "左Win",
        "MetaRight" | "OSRight" => "右Win",
        "Fn" => "Fn",
        "FnLock" => "FnLock",
        "CapsLock" => "CapsLock",
        "ScrollLock" => "ScrLock",
        "Pause" => "Pause",
        "PrintScreen" => "PrtSc",
        "Backspace" => "Backspace",
        "Tab" => "Tab",
        "Enter" => "Enter",
        "Space" => "Space",
        "Insert" => "Insert",
        "Delete" => "Delete",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "PageUp",
        "PageDown" => "PageDown",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "NumpadAdd" => "Num+",
        "NumpadSubtract" => "Num-",
        "NumpadMultiply" => "Num*",
        "NumpadDivide" => "Num/",
        "NumpadDecimal" => "Num.",
        "NumpadEnter" => "NumEnter",
        "Mouse4" => "Mouse4",
        "Mouse5" => "Mouse5",
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        _ => "",
    };
    if !label.is_empty() {
        return label.to_string();
    }
    if let Some(letter) = code.strip_prefix("Key") {
        if letter.len() == 1 {
            return letter.to_string();
        }
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        if digit.len() == 1 {
            return digit.to_string();
        }
    }
    if let Some(num) = code.strip_prefix("Numpad") {
        if num.len() == 1 && num.as_bytes()[0].is_ascii_digit() {
            return format!("Num{num}");
        }
    }
    code.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyCapability {
    pub adapter: HotkeyAdapterKind,
    pub available_triggers: Vec<HotkeyTrigger>,
    pub requires_accessibility_permission: bool,
    pub supports_modifier_only_trigger: bool,
    pub supports_side_specific_modifiers: bool,
    pub explicit_fallback_available: bool,
    pub status_hint: Option<String>,
}

impl HotkeyCapability {
    pub fn current() -> Self {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            return Self {
                adapter: HotkeyAdapterKind::Unavailable,
                available_triggers: Vec::new(),
                requires_accessibility_permission: false,
                supports_modifier_only_trigger: false,
                supports_side_specific_modifiers: false,
                explicit_fallback_available: false,
                status_hint: Some(
                    "移动端不支持全局热键；请使用应用内录音按钮或悬浮窗（需授权）。".into(),
                ),
            };
        }

        #[cfg(target_os = "macos")]
        {
            Self {
                adapter: HotkeyAdapterKind::MacEventTap,
                available_triggers: vec![
                    HotkeyTrigger::RightOption,
                    HotkeyTrigger::LeftOption,
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::LeftControl,
                    HotkeyTrigger::RightCommand,
                    HotkeyTrigger::LeftCommand,
                    HotkeyTrigger::LeftShift,
                    HotkeyTrigger::RightShift,
                    HotkeyTrigger::Fn,
                    HotkeyTrigger::Custom,
                ],
                requires_accessibility_permission: true,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some("授权辅助功能后，通常需要完全退出并重新打开 OpenLess。".into()),
            }
        }

        #[cfg(target_os = "windows")]
        {
            Self {
                adapter: HotkeyAdapterKind::WindowsLowLevel,
                // Windows has no Command key: leftCommand/rightCommand map to
                // the Win key, and a lone Win press opens the Start menu, so
                // it can't serve as a recording hotkey. Hence no Command
                // option among Windows' single-key presets (issue #784).
                available_triggers: vec![
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::RightAlt,
                    HotkeyTrigger::LeftControl,
                    HotkeyTrigger::LeftShift,
                    HotkeyTrigger::RightShift,
                    HotkeyTrigger::MediaPlayPause,
                    HotkeyTrigger::Custom,
                ],
                requires_accessibility_permission: false,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some(
                    "默认建议使用“右Ctrl + 单击”；若更习惯按住说话，可在录音设置里切回“按住”。若无响应，可在权限页查看 hook 安装状态。"
                        .into(),
                ),
            }
        }

        #[cfg(all(
            not(target_os = "macos"),
            not(target_os = "windows"),
            not(any(target_os = "android", target_os = "ios"))
        ))]
        {
            Self {
                adapter: HotkeyAdapterKind::Fcitx5,
                available_triggers: vec![
                    HotkeyTrigger::RightAlt,
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::LeftControl,
                    HotkeyTrigger::LeftCommand,
                    HotkeyTrigger::LeftShift,
                    HotkeyTrigger::RightShift,
                    HotkeyTrigger::Custom,
                ],
                requires_accessibility_permission: false,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some(
                    "Linux 使用 fcitx5 插件监听热键和提交文字。鼠标/侧别组合键需 evdev 读取 /dev/input/event*；若无权限请将用户加入 input 组（sudo usermod -aG input $USER）后重新登录。"
                        .into(),
                ),
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyInstallError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for HotkeyInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub adapter: HotkeyAdapterKind,
    pub state: HotkeyStatusState,
    pub message: Option<String>,
    pub last_error: Option<HotkeyInstallError>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsImeInstallState {
    Installed,
    NotInstalled,
    RegistrationBroken,
    NotWindows,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsImeStatus {
    pub state: WindowsImeInstallState,
    pub using_tsf_backend: bool,
    pub message: String,
    pub dll_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub platform: String,
    pub supports_ime_input: bool,
    pub supports_overlay: bool,
    pub supports_desktop_hotkey: bool,
    pub supports_tray: bool,
    pub supports_local_asr: bool,
    pub supports_local_qwen3_mlx: bool,
    pub supports_in_app_dictation: bool,
    pub supports_auto_update: bool,
}

impl PlatformCapabilities {
    pub fn current() -> Self {
        #[cfg(target_os = "android")]
        {
            Self {
                platform: "android".to_string(),
                supports_ime_input: false,
                supports_overlay: true,
                supports_desktop_hotkey: false,
                supports_tray: false,
                supports_local_asr: false,
                supports_local_qwen3_mlx: false,
                supports_in_app_dictation: true,
                supports_auto_update: true,
            }
        }

        #[cfg(all(
            any(target_os = "android", target_os = "ios"),
            not(target_os = "android")
        ))]
        {
            Self {
                platform: "mobile".to_string(),
                supports_ime_input: false,
                supports_overlay: false,
                supports_desktop_hotkey: false,
                supports_tray: false,
                supports_local_asr: false,
                supports_local_qwen3_mlx: false,
                supports_in_app_dictation: false,
                supports_auto_update: false,
            }
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            Self {
                platform: "desktop".to_string(),
                supports_ime_input: cfg!(target_os = "windows"),
                supports_overlay: true,
                supports_desktop_hotkey: true,
                supports_tray: true,
                supports_local_asr: cfg!(any(
                    target_os = "macos",
                    target_os = "linux",
                    target_os = "windows"
                )),
                supports_local_qwen3_mlx: cfg!(all(target_os = "macos", target_arch = "aarch64")),
                supports_in_app_dictation: false,
                supports_auto_update: true,
            }
        }
    }
}

impl Default for PlatformCapabilities {
    fn default() -> Self {
        Self {
            platform: "unknown".to_string(),
            supports_ime_input: false,
            supports_overlay: false,
            supports_desktop_hotkey: false,
            supports_tray: false,
            supports_local_asr: false,
            supports_local_qwen3_mlx: false,
            supports_in_app_dictation: false,
            supports_auto_update: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyStatusState {
    Starting,
    Installed,
    Failed,
}

impl Default for HotkeyStatus {
    fn default() -> Self {
        Self {
            adapter: HotkeyCapability::current().adapter,
            state: HotkeyStatusState::Starting,
            message: Some("正在安装全局快捷键监听".into()),
            last_error: None,
        }
    }
}

impl Default for HotkeyBinding {
    fn default() -> Self {
        // keys must stay None; never prefill a concrete code.
        //
        // HotkeyBinding's `#[serde(default)]` is a **struct-level default** —
        // deserialization fills the whole struct from Default before JSON
        // fields override. If keys were prefilled with Some([...]), old prefs
        // like `{"trigger":"rightControl","mode":"toggle"}` (no keys field)
        // would deserialize as `{trigger=RightControl, keys=Some([defaults])}`,
        // i.e. trigger and keys disagree — and effective_codes() trusts keys
        // directly, so the effective hotkey wouldn't match the trigger the
        // user chose. With keys=None, effective_codes() takes the
        // legacy_trigger_code(trigger) path and stays in sync with trigger.
        #[cfg(target_os = "windows")]
        {
            Self {
                trigger: HotkeyTrigger::RightControl,
                mode: HotkeyMode::Toggle,
                keys: None,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            Self {
                trigger: HotkeyTrigger::RightOption,
                mode: HotkeyMode::Toggle,
                keys: None,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapsuleState {
    Idle,
    Recording,
    Transcribing,
    Polishing,
    Done,
    Cancelled,
    Error,
}

/// Recording capsule appearance; serialized values feed preference storage and each Host's window events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CapsuleStyle {
    /// SiriGL light-effect stage style (default).
    #[default]
    Siri,
    /// Openless default style: classic frosted-glass pill (volume bar +
    /// cancel/confirm buttons).
    Classic,
    /// Classic dark capsule: blue waveform that narrows into a status hint
    /// while processing.
    Typeless,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsulePayload {
    /// Session generation that owns this capsule frame. Keeping this on the
    /// payload lets a Recording -> Recording transition reset the transcript
    /// without depending on the order of backend:event and capsule:state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub state: CapsuleState,
    pub level: f32, // 0..1 RMS
    pub elapsed_ms: u64,
    pub message: Option<String>,
    pub inserted_chars: Option<u32>,
    /// Whether the session is in translation mode (user pressed Shift). The
    /// frontend renders a "translating" tag at the top of the capsule so the
    /// user immediately knows this output goes through the translation
    /// pipeline. See issue #4.
    pub translation: bool,
    /// Whether this is a Less Computer (voice agent controlling the
    /// computer) session. The frontend switches the processing label from
    /// "thinking" to "using" — the agent is operating the computer, not
    /// merely thinking.
    #[serde(default)]
    pub operating: bool,
    /// Warming state: the capsule is "optimistically" shown (it pops up and
    /// plays its entrance animation on hotkey press) but the mic hasn't
    /// captured its first PCM frame yet. While true the frontend renders a
    /// standby glow (soft breathing, not wired to real levels) hinting the
    /// user to hold off speaking; the first `level_handler` firing (real PCM
    /// flowing) flips it false and the bar "lights up" into the recording
    /// state. Only meaningful for the Recording state.
    #[serde(default)]
    pub warming: bool,
    /// User's chosen capsule style (siri / classic). Sent with every state
    /// event, so a Settings switch takes effect on the next recording
    /// without an extra request from the capsule webview.
    #[serde(default)]
    pub capsule_style: CapsuleStyle,
    /// Lightweight feedback flag for selection polish. Shares the same
    /// non-focus-stealing capsule window as voice/QA sessions, but the
    /// frontend switches to a one-line status hint so existing voice glow
    /// and copy stay untouched.
    #[serde(default)]
    pub selection_polish: bool,
}

/// Snapshot of credentials read from vault — only what the UI needs to know
/// (whether keys are set; never the values themselves).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsStatus {
    pub active_asr_provider: String,
    pub active_llm_provider: String,
    /// Current recognition pipeline mode ("traditional" | "multimodal"); the
    /// frontend uses it to pick which config cards to render and which set
    /// the Overview treats as "configured".
    pub pipeline_mode: PipelineMode,
    pub asr_configured: bool,
    pub llm_configured: bool,
    /// Whether the multimodal (omni) model is configured. Only meaningful when `pipeline_mode == multimodal`.
    pub omni_configured: bool,
    // Legacy frontend fields (being migrated incrementally)
    pub volcengine_configured: bool,
    pub ark_configured: bool,
}

/// Today's metrics shown on the Overview tab.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodayMetrics {
    pub chars_today: u64,
    pub segments_today: u64,
    pub avg_latency_ms: u64,
    pub total_duration_ms: u64,
}

/// One chat message in the selection-QA popover. Follow-up questions
/// accumulate into a Vec<QaChatMessage> sent whole to the LLM to keep
/// context. See issue #118 v2.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QaChatMessage {
    /// "user" | "assistant" — maps directly to the OpenAI message role field.
    pub role: String,
    pub content: String,
    /// For safe frontend display of the selection text only; the LLM channel reads only `role` / `content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_text: Option<String>,
}

#[cfg(test)]
mod split_front_app_label_tests {
    use super::{split_front_app_label, split_front_app_opt, FrontApp};

    #[test]
    fn macos_label_splits_into_name_and_bundle() {
        let split = split_front_app_label("Claude (com.anthropic.claudefordesktop)", true);
        assert_eq!(split.name.as_deref(), Some("Claude"));
        assert_eq!(
            split.bundle_id.as_deref(),
            Some("com.anthropic.claudefordesktop")
        );
    }

    #[test]
    fn app_names_containing_spaces_and_parens_still_split_on_the_last_group() {
        let split = split_front_app_label("Visual Studio Code (com.microsoft.VSCode)", true);
        assert_eq!(split.name.as_deref(), Some("Visual Studio Code"));
        assert_eq!(split.bundle_id.as_deref(), Some("com.microsoft.VSCode"));
    }

    /// Windows titles are window titles; their parentheses are body text, not
    /// bundle ids. With the platform switch off the whole string stays
    /// intact — even when the bracketed part looks like a reverse domain,
    /// file path, or version. Splitting would truncate the title and record
    /// a wrong bundle id.
    #[test]
    fn window_titles_are_never_split_outside_macos() {
        for title in [
            "未命名文档 (未保存)",
            "report.txt (~/Documents)",
            "Inbox (12)",
            "script.py (C:\\dir\\script.py)",
            "会议 (meet.example.com)",
            "卸载 (2.4.1)",
        ] {
            let split = split_front_app_label(title, false);
            assert_eq!(
                split.name.as_deref(),
                Some(title),
                "{title} should stay intact"
            );
            assert_eq!(split.bundle_id, None, "{title} has no bundle id");
        }
    }

    #[test]
    fn bare_names_pass_through() {
        let split = split_front_app_label("Terminal", true);
        assert_eq!(split.name.as_deref(), Some("Terminal"));
        assert_eq!(split.bundle_id, None);
    }

    #[test]
    fn blank_input_yields_nothing() {
        assert_eq!(
            split_front_app_label("", true),
            FrontApp {
                name: None,
                bundle_id: None
            }
        );
        assert_eq!(
            split_front_app_label("   ", true),
            FrontApp {
                name: None,
                bundle_id: None
            }
        );
        assert_eq!(
            split_front_app_label("", false),
            FrontApp {
                name: None,
                bundle_id: None
            }
        );
        assert_eq!(
            split_front_app_label("   ", false),
            FrontApp {
                name: None,
                bundle_id: None
            }
        );
        assert_eq!(
            split_front_app_opt(None),
            FrontApp {
                name: None,
                bundle_id: None
            }
        );
    }
}

#[cfg(test)]
mod translation_effective_tests {
    use super::translation_effective;

    fn langs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn requires_the_modifier() {
        assert!(!translation_effective(
            false,
            "English",
            &langs(&["简体中文"])
        ));
    }

    #[test]
    fn unset_target_language_is_not_translation() {
        // Pressing Shift without a chosen target language: the capsule used
        // to show "translating" while the backend ran plain polish.
        assert!(!translation_effective(true, "", &langs(&["简体中文"])));
        assert!(!translation_effective(true, "   ", &langs(&["简体中文"])));
    }

    #[test]
    fn target_equal_to_the_only_working_language_is_a_no_op() {
        // Only working language is Chinese and the target is Chinese — the source is necessarily the target, so translation is a no-op.
        assert!(!translation_effective(
            true,
            "简体中文",
            &langs(&["简体中文"])
        ));
        // Surrounding whitespace must not slip past this check.
        assert!(!translation_effective(
            true,
            " 简体中文 ",
            &langs(&["简体中文"])
        ));
    }

    #[test]
    fn simplified_to_traditional_still_translates() {
        // Simplified/traditional are separate entries in the language list; simplified -> traditional is a real conversion and must not be blocked as "the same Chinese".
        assert!(translation_effective(
            true,
            "繁体中文",
            &langs(&["简体中文"])
        ));
    }

    #[test]
    fn multiple_working_languages_are_never_blocked() {
        // Bilingual users targeting English is normal usage (speak Chinese,
        // output English); the source language can't be known up front, so
        // the target appearing among working languages must not block it.
        assert!(translation_effective(
            true,
            "English",
            &langs(&["简体中文", "English"])
        ));
    }

    #[test]
    fn empty_working_languages_still_translates() {
        assert!(translation_effective(true, "English", &[]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_newline_modes_round_trip_legacy_auto_and_line_feed() {
        assert_eq!(MacosNewlineMode::default(), MacosNewlineMode::Auto);
        for (wire, expected) in [
            ("\"auto\"", MacosNewlineMode::Auto),
            ("\"shiftReturn\"", MacosNewlineMode::ShiftReturn),
            ("\"lineFeed\"", MacosNewlineMode::LineFeed),
            ("\"return\"", MacosNewlineMode::Return),
        ] {
            let decoded: MacosNewlineMode = serde_json::from_str(wire).unwrap();
            assert_eq!(decoded, expected);
            assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        }
    }

    #[test]
    fn multimodal_mode_requires_the_experiment_switch() {
        assert_eq!(
            effective_pipeline_mode(false, PipelineMode::Traditional),
            PipelineMode::Traditional
        );
        assert_eq!(
            effective_pipeline_mode(false, PipelineMode::Multimodal),
            PipelineMode::Traditional
        );
        assert_eq!(
            effective_pipeline_mode(true, PipelineMode::Traditional),
            PipelineMode::Traditional
        );
        assert_eq!(
            effective_pipeline_mode(true, PipelineMode::Multimodal),
            PipelineMode::Multimodal
        );
    }

    #[test]
    fn obsolete_selection_voice_hotkey_is_ignored_and_not_serialized() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "selectionVoiceEnabled": true,
                "selectionVoiceHotkey": { "primary": "E", "modifiers": ["ctrl", "shift"] }
            }"#,
        )
        .unwrap();

        assert!(prefs.selection_voice_enabled);
        assert!(!serde_json::to_string(&prefs)
            .unwrap()
            .contains("selectionVoiceHotkey"));
    }

    #[test]
    fn local_asr_model_preferences_migrate_without_cross_provider_overwrite() {
        let old_qwen: UserPreferences =
            serde_json::from_str(r#"{"localAsrActiveModel":"qwen3-asr-1.7b"}"#).unwrap();
        assert_eq!(old_qwen.local_asr_active_model, "qwen3-asr-1.7b");
        assert_eq!(
            old_qwen.local_whisper_active_model,
            default_local_whisper_model()
        );

        let old_whisper: UserPreferences =
            serde_json::from_str(r#"{"localAsrActiveModel":"whisper-small"}"#).unwrap();
        assert_eq!(
            old_whisper.local_asr_active_model,
            default_local_asr_model()
        );
        assert_eq!(old_whisper.local_whisper_active_model, "whisper-small");

        let separated: UserPreferences = serde_json::from_str(
            r#"{
                "localAsrActiveModel":"qwen3-asr-1.7b",
                "localWhisperActiveModel":"whisper-medium"
            }"#,
        )
        .unwrap();
        assert_eq!(separated.local_asr_active_model, "qwen3-asr-1.7b");
        assert_eq!(separated.local_whisper_active_model, "whisper-medium");
    }

    #[test]
    fn salvage_preserves_valid_fields_when_one_value_is_invalid() {
        // Simulates an old file after "a refactor renamed an enum variant":
        // defaultMode holds a value that no longer exists, while
        // dictationHotkey / activeAsrProvider stay valid. Salvage must keep
        // the valid fields and reset only the invalid one — not discard
        // everything.
        let json = br#"{
            "defaultMode": "totally-removed-mode",
            "dictationHotkey": { "primary": "LeftOption", "modifiers": [] },
            "activeAsrProvider": "bailian-qwen3-realtime"
        }"#;

        // Strict parsing must fail (otherwise this test is meaningless).
        assert!(serde_json::from_slice::<UserPreferences>(json).is_err());

        let salvaged = UserPreferences::salvage_from_json_bytes(json);
        assert_eq!(salvaged.dictation_hotkey.primary, "LeftOption");
        assert_eq!(salvaged.active_asr_provider, "bailian-qwen3-realtime");
        // The invalid field falls back to its default instead of failing the whole parse.
        assert_eq!(
            salvaged.default_mode,
            UserPreferences::default().default_mode
        );
    }

    #[test]
    fn salvage_normalizes_duplicate_legacy_aliases_without_resetting_other_fields() {
        let json = br#"{
            "windowsSendInputInsertionOnly": false,
            "windowsSendinputInsertionOnly": true,
            "windowsSendInputNewlineMode": "removed-mode",
            "windowsSendinputNewlineMode": "shiftEnter",
            "activeAsrProvider": "preserved-provider"
        }"#;

        assert!(serde_json::from_slice::<UserPreferences>(json).is_err());

        let salvaged = UserPreferences::salvage_from_json_bytes(json);
        assert!(!salvaged.windows_sendinput_insertion_only);
        assert_eq!(
            salvaged.windows_sendinput_newline_mode,
            WindowsSendInputNewlineMode::ShiftEnter
        );
        assert_eq!(salvaged.active_asr_provider, "preserved-provider");
    }

    #[test]
    fn non_tsf_insertion_fallback_defaults_to_enabled() {
        let prefs = UserPreferences::default();

        assert!(prefs.allow_non_tsf_insertion_fallback);
    }

    #[test]
    fn missing_non_tsf_insertion_fallback_pref_defaults_to_enabled() {
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();

        assert!(prefs.allow_non_tsf_insertion_fallback);
    }

    #[test]
    fn windows_sendinput_insertion_only_defaults_to_disabled() {
        let prefs = UserPreferences::default();
        assert!(!prefs.windows_sendinput_insertion_only);
        assert_eq!(prefs.windows_insertion_mode, WindowsInsertionMode::Tsf);

        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(!prefs.windows_sendinput_insertion_only);
        assert_eq!(prefs.windows_insertion_mode, WindowsInsertionMode::Tsf);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn missing_selection_polish_hotkey_preserves_legacy_right_control_dictation() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{"dictationHotkey":{"primary":"RightControl","modifiers":[]}}"#,
        )
        .unwrap();
        assert!(prefs.selection_polish_hotkey.is_none());
        assert_eq!(prefs.dictation_hotkey.primary, "RightControl");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn legacy_right_alt_dictation_upgrade_disables_selection_polish_instead_of_colliding() {
        // #904: when an old config with the recording key customized to right
        // Alt upgrades, the injected selection-polish default (right Alt)
        // collides persistently and blocks all later settings saves. The
        // migration must disable the new feature instead.
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "hotkey": { "trigger": "rightAlt", "mode": "hold", "keys": null },
                "dictationHotkey": { "primary": "RightAlt", "modifiers": [] }
            }"#,
        )
        .unwrap();
        assert!(prefs.selection_polish_hotkey.is_none());
        assert_eq!(prefs.dictation_hotkey.primary, "RightAlt");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn legacy_right_alt_trigger_upgrade_disables_selection_polish_by_overlap() {
        // #904 variant: the old file has no dictationHotkey, only legacy
        // hotkey.trigger=rightAlt, whose derived recording key primary is
        // "RightOption" — unequal as a string to the injected "RightAlt" but
        // the same physical key (bindings_overlap=true). The migration must
        // judge by overlap, not by == string comparison.
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "hotkey": { "trigger": "rightAlt", "mode": "hold", "keys": null }
            }"#,
        )
        .unwrap();
        assert!(prefs.selection_polish_hotkey.is_none());
        assert_eq!(prefs.dictation_hotkey.primary, "RightOption");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn new_preferences_keep_the_existing_dictation_default_and_use_right_alt_for_selection_polish()
    {
        let prefs = UserPreferences::default();
        assert_eq!(prefs.dictation_hotkey.primary, "RightControl");
        assert_eq!(
            prefs.selection_polish_hotkey,
            Some(ShortcutBinding {
                primary: "RightAlt".into(),
                modifiers: Vec::new(),
            })
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn explicit_selection_polish_setting_does_not_rewrite_dictation_binding() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{"dictationHotkey":{"primary":"RightControl","modifiers":[]},"selectionPolishHotkey":null}"#,
        )
        .unwrap();
        assert!(prefs.selection_polish_hotkey.is_none());
        assert_eq!(prefs.dictation_hotkey.primary, "RightControl");
    }

    #[test]
    fn windows_sendinput_insertion_only_deserializes_frontend_wire_key() {
        let prefs: UserPreferences =
            serde_json::from_str(r#"{"windowsSendInputInsertionOnly": true}"#).unwrap();
        assert!(prefs.windows_sendinput_insertion_only);
        assert_eq!(
            prefs.windows_insertion_mode,
            WindowsInsertionMode::SendInput
        );
    }

    #[test]
    fn windows_sendinput_insertion_only_deserializes_legacy_wrong_camel_key() {
        let prefs: UserPreferences =
            serde_json::from_str(r#"{"windowsSendinputInsertionOnly": true}"#).unwrap();
        assert!(prefs.windows_sendinput_insertion_only);
        assert_eq!(
            prefs.windows_insertion_mode,
            WindowsInsertionMode::SendInput
        );
    }

    #[test]
    fn windows_insertion_mode_deserializes_explicit_paste() {
        let prefs: UserPreferences =
            serde_json::from_str(r#"{"windowsInsertionMode":"paste"}"#).unwrap();
        assert_eq!(prefs.windows_insertion_mode, WindowsInsertionMode::Paste);
        assert!(!prefs.windows_sendinput_insertion_only);
    }

    #[test]
    fn windows_sendinput_newline_mode_defaults_to_enter() {
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(
            prefs.windows_sendinput_newline_mode,
            WindowsSendInputNewlineMode::Enter
        );
    }

    #[test]
    fn windows_sendinput_newline_mode_deserializes_shift_enter() {
        let prefs: UserPreferences =
            serde_json::from_str(r#"{"windowsSendInputNewlineMode":"shiftEnter"}"#).unwrap();
        assert_eq!(
            prefs.windows_sendinput_newline_mode,
            WindowsSendInputNewlineMode::ShiftEnter
        );
    }

    #[test]
    fn windows_sendinput_newline_mode_serializes_frontend_wire_key() {
        let prefs = UserPreferences {
            windows_insertion_mode: WindowsInsertionMode::SendInput,
            windows_sendinput_newline_mode: WindowsSendInputNewlineMode::ShiftEnter,
            ..UserPreferences::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(json.contains(r#""windowsSendInputNewlineMode":"shiftEnter""#));
        assert!(!json.contains("windowsSendinputNewlineMode"));
    }

    #[test]
    fn windows_sendinput_insertion_only_serializes_frontend_wire_key() {
        let enabled = UserPreferences {
            windows_insertion_mode: WindowsInsertionMode::SendInput,
            windows_sendinput_insertion_only: true,
            ..UserPreferences::default()
        };
        let json = serde_json::to_string(&enabled).unwrap();
        assert!(json.contains(r#""windowsSendInputInsertionOnly":true"#));
        assert!(!json.contains("windowsSendinputInsertionOnly"));
    }

    #[test]
    fn windows_sendinput_insertion_only_pref_round_trips_explicit_true() {
        let enabled = UserPreferences {
            windows_insertion_mode: WindowsInsertionMode::SendInput,
            windows_sendinput_insertion_only: true,
            ..UserPreferences::default()
        };
        let json = serde_json::to_string(&enabled).unwrap();
        assert!(json.contains(r#""windowsSendInputInsertionOnly":true"#));
        assert!(json.contains(r#""windowsInsertionMode":"sendInput""#));
        let restored: UserPreferences = serde_json::from_str(&json).unwrap();
        assert!(restored.windows_sendinput_insertion_only);
        assert_eq!(
            restored.windows_insertion_mode,
            WindowsInsertionMode::SendInput
        );
    }

    #[test]
    fn windows_show_openless_in_keyboard_list_defaults_to_enabled() {
        let prefs = UserPreferences::default();
        assert!(prefs.windows_show_openless_in_keyboard_list);

        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(prefs.windows_show_openless_in_keyboard_list);
    }

    #[test]
    fn windows_show_openless_in_keyboard_list_deserializes_frontend_wire_key() {
        let prefs: UserPreferences =
            serde_json::from_str(r#"{"windowsShowOpenlessInKeyboardList": false}"#).unwrap();
        assert!(!prefs.windows_show_openless_in_keyboard_list);
    }

    #[test]
    fn windows_show_openless_in_keyboard_list_serializes_frontend_wire_key() {
        let hidden = UserPreferences {
            windows_show_openless_in_keyboard_list: false,
            ..UserPreferences::default()
        };
        let json = serde_json::to_string(&hidden).unwrap();
        assert!(json.contains(r#""windowsShowOpenlessInKeyboardList":false"#));
    }

    #[test]
    fn missing_audio_cue_on_record_pref_defaults_to_enabled() {
        // Old preferences.json lacks this field -> should default to enabled (cue on record press).
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();

        assert!(prefs.audio_cue_on_record);
    }

    #[test]
    fn capsule_style_pref_defaults_to_siri_and_round_trips_wire_key() {
        // Old preferences.json has no capsuleStyle field -> falls back to the default Siri.
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(prefs.capsule_style, CapsuleStyle::Siri);

        for (style, wire_name) in [
            (CapsuleStyle::Siri, "siri"),
            (CapsuleStyle::Classic, "classic"),
            (CapsuleStyle::Typeless, "typeless"),
        ] {
            let preferences = UserPreferences {
                capsule_style: style,
                ..Default::default()
            };
            let value = serde_json::to_value(&preferences).unwrap();
            assert_eq!(value["capsuleStyle"], wire_name);
            let restored: UserPreferences = serde_json::from_value(value).unwrap();
            assert_eq!(restored.capsule_style, style);
        }
    }

    #[test]
    fn audio_cue_on_record_pref_round_trips_explicit_false() {
        // After the user turns it off in Settings, set_settings -> save ->
        // get_settings must preserve false, or the toggle snaps back to true
        // on refresh (classic symptom of the field being dropped in the Wire
        // round trip).
        let disabled = UserPreferences {
            audio_cue_on_record: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&disabled).unwrap();
        assert!(
            json.contains("\"audioCueOnRecord\":false"),
            "序列化应输出 camelCase 字段，实际: {json}"
        );

        let restored: UserPreferences = serde_json::from_str(&json).unwrap();
        assert!(!restored.audio_cue_on_record);
    }

    #[test]
    fn stable_transcription_defaults_off_and_round_trips_when_enabled() {
        let legacy: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(!legacy.stable_transcription_enabled);

        let enabled = UserPreferences {
            stable_transcription_enabled: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&enabled).unwrap();
        assert!(json.contains("\"stableTranscriptionEnabled\":true"));
        let restored: UserPreferences = serde_json::from_str(&json).unwrap();
        assert!(restored.stable_transcription_enabled);
    }

    #[test]
    fn action_hotkeys_default_to_enabled() {
        // issue #576: still enabled by default (Some default key), zero behavior change for existing users.
        let prefs = UserPreferences::default();
        assert!(prefs.switch_style_hotkey.is_some());
        assert!(prefs.open_app_hotkey.is_some());
    }

    #[test]
    fn missing_action_hotkeys_default_to_enabled() {
        // Old users / missing field: the wire struct-default lands on Some(default key), which must not count as disabled.
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(prefs.switch_style_hotkey.is_some());
        assert!(prefs.open_app_hotkey.is_some());
    }

    #[test]
    fn disabled_action_hotkeys_round_trip_as_null() {
        // issue #576: after the user clears it (None=disabled), save -> read
        // back must stay None, not collapse to the default key via
        // unwrap_or_else like the old logic.
        let disabled = UserPreferences {
            switch_style_hotkey: None,
            open_app_hotkey: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&disabled).unwrap();
        assert!(
            json.contains("\"switchStyleHotkey\":null"),
            "停用应序列化成 null，实际: {json}"
        );
        let restored: UserPreferences = serde_json::from_str(&json).unwrap();
        assert!(restored.switch_style_hotkey.is_none());
        assert!(restored.open_app_hotkey.is_none());
    }

    #[test]
    fn style_pack_hotkeys_default_empty_and_round_trip() {
        // issue #759: old preferences.json lacks the field -> empty list, no error.
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(prefs.style_pack_hotkeys.is_empty());

        // Configured bindings survive save -> read back unchanged (camelCase field names).
        let configured = UserPreferences {
            style_pack_hotkeys: vec![StylePackHotkey {
                pack_id: "imported.demo".into(),
                binding: ShortcutBinding {
                    primary: "1".into(),
                    modifiers: vec!["alt".into()],
                },
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&configured).unwrap();
        assert!(
            json.contains("\"stylePackHotkeys\":[{\"packId\":\"imported.demo\""),
            "应序列化为 camelCase，实际: {json}"
        );
        let restored: UserPreferences = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.style_pack_hotkeys, configured.style_pack_hotkeys);
    }

    #[test]
    fn explicit_action_hotkey_binding_round_trips() {
        // Old preferences.json with a real binding -> read back keeps Some (enabled).
        let prefs: UserPreferences = serde_json::from_str(
            r#"{"switchStyleHotkey":{"primary":"S","modifiers":["cmd","shift"]}}"#,
        )
        .unwrap();
        let binding = prefs.switch_style_hotkey.expect("应保留为 Some");
        assert_eq!(binding.primary, "S");
        assert_eq!(
            binding.modifiers,
            vec!["cmd".to_string(), "shift".to_string()]
        );
    }

    #[test]
    fn missing_custom_style_prompts_defaults_to_empty() {
        let prefs: UserPreferences = serde_json::from_str("{}").unwrap();

        assert_eq!(prefs.custom_style_prompts, CustomStylePrompts::default());
        assert!(!prefs.custom_style_prompts.has_for_mode(PolishMode::Raw));
    }

    #[test]
    fn style_pack_workflow_prompts_are_selected_independently() {
        let mut pack = builtin_style_pack_for_mode(PolishMode::Light);
        pack.prompt = "ASR prompt marker".into();
        pack.selection_prompt = "selected-text prompt marker".into();

        assert_eq!(
            style_pack_prompt(&pack, StylePromptKind::DictationAsr),
            "ASR prompt marker"
        );
        assert_eq!(
            style_pack_prompt(&pack, StylePromptKind::Selection),
            "selected-text prompt marker"
        );
    }

    #[test]
    fn empty_selection_prompt_uses_non_asr_fallback_without_touching_asr_prompt() {
        let mut pack = builtin_style_pack_for_mode(PolishMode::Light);
        pack.prompt = "ASR prompt marker".into();
        pack.selection_prompt.clear();

        let selection_prompt = style_pack_prompt(&pack, StylePromptKind::Selection);
        assert!(selection_prompt.contains("不是语音识别（ASR）转写"));
        assert_eq!(
            style_pack_prompt(&pack, StylePromptKind::DictationAsr),
            "ASR prompt marker"
        );
    }

    #[test]
    fn custom_style_prompts_round_trip_explicit_values() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "customStylePrompts": {
                    "raw": "保留我的口头禅",
                    "light": "更像微信消息",
                    "structured": "按项目符号整理",
                    "formal": "像正式周报"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(prefs.custom_style_prompts.raw, "保留我的口头禅");
        assert_eq!(prefs.custom_style_prompts.light, "更像微信消息");
        assert_eq!(prefs.custom_style_prompts.structured, "按项目符号整理");
        assert_eq!(prefs.custom_style_prompts.formal, "像正式周报");
        assert!(prefs.custom_style_prompts.has_for_mode(PolishMode::Formal));
    }

    #[test]
    fn missing_active_style_pack_id_uses_legacy_default_mode() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "defaultMode": "structured"
            }"#,
        )
        .unwrap();

        assert_eq!(prefs.default_mode, PolishMode::Structured);
        assert_eq!(prefs.active_style_pack_id, BUILTIN_STYLE_PACK_STRUCTURED_ID);
    }

    #[test]
    fn explicit_active_style_pack_id_is_preserved() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "defaultMode": "formal",
                "activeStylePackId": "custom.meeting"
            }"#,
        )
        .unwrap();

        assert_eq!(prefs.default_mode, PolishMode::Formal);
        assert_eq!(prefs.active_style_pack_id, "custom.meeting");
    }

    #[test]
    fn legacy_custom_style_prompts_are_not_appended_twice() {
        let base = StyleSystemPrompts::default();
        let legacy = CustomStylePrompts {
            light: "更像微信消息".into(),
            ..CustomStylePrompts::default()
        };

        let once = base.clone().with_legacy_custom_prompts(&legacy);
        let twice = once.clone().with_legacy_custom_prompts(&legacy);

        assert_eq!(once.light, twice.light);
        assert_eq!(twice.light.matches("# 用户自定义附加要求").count(), 1);
    }

    /// issue #360: the default must be CtrlV, matching historical behavior;
    /// old config files without a pasteShortcut field must also deserialize
    /// to CtrlV, otherwise existing users' paste behavior changes silently.
    #[test]
    fn paste_shortcut_defaults_to_ctrl_v() {
        let prefs = UserPreferences::default();
        assert_eq!(prefs.paste_shortcut, PasteShortcut::CtrlV);

        let from_empty: UserPreferences = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty.paste_shortcut, PasteShortcut::CtrlV);
    }

    /// issue #440: old versions wrote the default `streamingInsert:false`
    /// into preferences.json. Old files without the migration marker move to
    /// true uniformly; once the marker is present, a user's manually chosen
    /// false must be preserved.
    #[test]
    fn streaming_insert_defaults_to_enabled_for_missing_or_legacy_unmigrated_pref() {
        let prefs = UserPreferences::default();
        assert!(prefs.streaming_insert);
        assert!(prefs.streaming_insert_default_migrated);
        assert!(prefs.streaming_insert_save_clipboard);

        let from_empty: UserPreferences = serde_json::from_str("{}").unwrap();
        assert!(from_empty.streaming_insert);
        assert!(from_empty.streaming_insert_default_migrated);
        assert!(from_empty.streaming_insert_save_clipboard);

        let from_legacy_false: UserPreferences = serde_json::from_str(
            r#"{
                "streamingInsert": false,
                "streamingInsertSaveClipboard": true
            }"#,
        )
        .unwrap();
        assert!(from_legacy_false.streaming_insert);
        assert!(from_legacy_false.streaming_insert_default_migrated);
    }

    #[test]
    fn streaming_insert_preserves_explicit_disabled_value() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "streamingInsert": false,
                "streamingInsertDefaultMigrated": true,
                "streamingInsertSaveClipboard": false
            }"#,
        )
        .unwrap();

        assert!(!prefs.streaming_insert);
        assert!(prefs.streaming_insert_default_migrated);
        assert!(!prefs.streaming_insert_save_clipboard);
    }

    #[test]
    fn update_channel_migration_preserves_only_explicit_legacy_beta_opt_in() {
        let legacy_stable: UserPreferences =
            serde_json::from_str(r#"{ "updateChannel": "stable" }"#).unwrap();
        assert_eq!(legacy_stable.update_channel, UpdateChannel::Stable);
        assert!(!legacy_stable.update_channel_explicit);

        let legacy_beta: UserPreferences =
            serde_json::from_str(r#"{ "updateChannel": "beta" }"#).unwrap();
        assert_eq!(legacy_beta.update_channel, UpdateChannel::Beta);
        assert!(legacy_beta.update_channel_explicit);

        let explicit_stable: UserPreferences = serde_json::from_str(
            r#"{
                "updateChannel": "stable",
                "updateChannelExplicit": true
            }"#,
        )
        .unwrap();
        assert!(explicit_stable.update_channel_explicit);

        let round_trip: UserPreferences =
            serde_json::from_str(&serde_json::to_string(&explicit_stable).unwrap()).unwrap();
        assert!(round_trip.update_channel_explicit);
    }

    #[test]
    fn paste_shortcut_round_trips_explicit_values() {
        for (raw, expected) in [
            ("ctrlV", PasteShortcut::CtrlV),
            ("ctrlShiftV", PasteShortcut::CtrlShiftV),
            ("shiftInsert", PasteShortcut::ShiftInsert),
        ] {
            let json = format!(r#"{{ "pasteShortcut": "{raw}" }}"#);
            let prefs: UserPreferences = serde_json::from_str(&json).unwrap();
            assert_eq!(prefs.paste_shortcut, expected, "raw={raw}");
        }
    }

    #[test]
    fn legacy_custom_hotkey_without_custom_binding_is_rejected() {
        let result = serde_json::from_str::<UserPreferences>(
            r#"{
                "hotkey": { "trigger": "custom", "mode": "toggle" }
            }"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn salvage_preserves_valid_fields_when_legacy_custom_hotkey_is_incomplete() {
        let json = br#"{
            "hotkey": { "trigger": "custom", "mode": "toggle", "keys": null },
            "activeAsrProvider": "preserved-provider"
        }"#;

        assert!(serde_json::from_slice::<UserPreferences>(json).is_err());

        let salvaged = UserPreferences::salvage_from_json_bytes(json);
        assert_eq!(salvaged.active_asr_provider, "preserved-provider");
        assert_eq!(salvaged.hotkey, UserPreferences::default().hotkey);
    }

    #[test]
    fn legacy_custom_hotkey_uses_custom_combo_binding() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "hotkey": { "trigger": "custom", "mode": "toggle" },
                "customComboHotkey": { "primary": "D", "modifiers": ["cmd", "shift"] }
            }"#,
        )
        .unwrap();

        assert_eq!(prefs.dictation_hotkey.primary, "D");
        assert_eq!(prefs.dictation_hotkey.modifiers, vec!["cmd", "shift"]);
    }

    #[test]
    fn custom_hotkey_with_dictation_hotkey_preserves_dictation_binding() {
        let prefs: UserPreferences = serde_json::from_str(
            r#"{
                "hotkey": { "trigger": "custom", "mode": "toggle" },
                "dictationHotkey": { "primary": "Space", "modifiers": ["ctrl"] }
            }"#,
        )
        .unwrap();

        assert_eq!(prefs.dictation_hotkey.primary, "Space");
        assert_eq!(prefs.dictation_hotkey.modifiers, vec!["ctrl"]);
    }

    #[test]
    fn legacy_hotkey_trigger_still_produces_effective_key_codes() {
        let binding: HotkeyBinding =
            serde_json::from_str(r#"{"trigger":"rightControl","mode":"toggle"}"#).unwrap();

        assert_eq!(binding.effective_codes(), vec!["ControlRight".to_string()]);
        assert_eq!(binding.display_label(), "右 Control");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn legacy_fn_trigger_uses_windows_control_right_alias() {
        let binding: HotkeyBinding =
            serde_json::from_str(r#"{"trigger":"fn","mode":"toggle"}"#).unwrap();

        assert_eq!(binding.effective_codes(), vec!["ControlRight".to_string()]);
    }

    #[test]
    fn hotkey_binding_supports_combo_side_keys_mouse_and_double_click_mode() {
        let binding = HotkeyBinding {
            trigger: HotkeyTrigger::RightControl,
            mode: HotkeyMode::DoubleClick,
            keys: Some(vec![
                HotkeyKey::new("ControlLeft"),
                HotkeyKey::new("AltLeft"),
                HotkeyKey::new("Mouse4"),
            ]),
        };

        assert_eq!(
            binding.effective_codes(),
            vec![
                "ControlLeft".to_string(),
                "AltLeft".to_string(),
                "Mouse4".to_string()
            ]
        );
        assert_eq!(binding.display_label(), "左Ctrl+左Alt+Mouse4");

        let json = serde_json::to_value(&binding).unwrap();
        assert_eq!(json["mode"], "doubleClick");
    }

    #[test]
    fn explicit_empty_hotkey_keys_clear_the_binding() {
        let binding: HotkeyBinding =
            serde_json::from_str(r#"{"trigger":"rightControl","mode":"toggle","keys":[]}"#)
                .unwrap();

        assert!(binding.effective_codes().is_empty());
    }

    /// PR #826: the new model/latency fields must be backward compatible — old history.json has none of these keys.
    #[test]
    fn dictation_session_deserializes_legacy_json_without_model_fields() {
        let legacy = r#"{
            "id": "abc",
            "createdAt": "2026-07-01T00:00:00Z",
            "rawTranscript": "你好",
            "finalText": "你好。",
            "mode": "light",
            "appBundleId": null,
            "appName": null,
            "insertStatus": "inserted",
            "errorCode": null,
            "durationMs": 1200,
            "dictionaryEntryCount": null
        }"#;
        let session: DictationSession = serde_json::from_str(legacy).expect("legacy json");
        assert_eq!(session.source, HistorySource::Voice);
        assert_eq!(session.asr_provider, None);
        assert_eq!(session.asr_model, None);
        assert_eq!(session.llm_provider, None);
        assert_eq!(session.llm_model, None);
        assert_eq!(session.asr_ms, None);
        assert_eq!(session.polish_ms, None);
    }

    /// New fields must serialize as camelCase (the frontend types.ts mirror reads camelCase).
    #[test]
    fn dictation_session_serializes_model_fields_as_camel_case() {
        let session = DictationSession {
            id: "abc".into(),
            created_at: "2026-07-01T00:00:00Z".into(),
            source: HistorySource::SelectionPolish,
            raw_transcript: "你好".into(),
            asr_transcript: None,
            final_text: "你好。".into(),
            mode: PolishMode::Light,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: None,
            insert_status: InsertStatus::Inserted,
            error_code: None,
            duration_ms: Some(1200),
            dictionary_entry_count: None,
            has_audio_recording: None,
            asr_provider: Some("bailian".into()),
            asr_model: Some("fun-asr-realtime".into()),
            llm_provider: Some("ark".into()),
            llm_model: Some("deepseek-v3-2".into()),
            pipeline_mode: None,
            asr_ms: Some(230),
            polish_ms: Some(1450),
        };
        let json = serde_json::to_value(&session).expect("serialize");
        assert_eq!(json["source"], "selection_polish");
        assert_eq!(json["asrProvider"], "bailian");
        assert_eq!(json["asrModel"], "fun-asr-realtime");
        assert_eq!(json["llmProvider"], "ark");
        assert_eq!(json["llmModel"], "deepseek-v3-2");
        assert_eq!(json["asrMs"], 230);
        assert_eq!(json["polishMs"], 1450);
    }
}

fn default_capsule_transcript_font_size() -> u8 {
    14
}

#[cfg(test)]
mod capsule_transcript_preferences_tests {
    use super::*;
    #[test]
    fn capsule_transcript_defaults_and_roundtrip() {
        let mut value = serde_json::to_value(UserPreferences::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("capsuleTranscriptEnabled");
        value
            .as_object_mut()
            .unwrap()
            .remove("capsuleTranscriptFontSize");
        let old: UserPreferences = serde_json::from_value(value).unwrap();
        assert!(old.capsule_transcript_enabled);
        assert_eq!(old.capsule_transcript_font_size, 14);
        let mut prefs = old;
        prefs.capsule_transcript_enabled = false;
        prefs.capsule_transcript_font_size = 20;
        let restored: UserPreferences =
            serde_json::from_slice(&serde_json::to_vec(&prefs).unwrap()).unwrap();
        assert!(!restored.capsule_transcript_enabled);
        assert_eq!(restored.capsule_transcript_font_size, 20);
        let mut value = serde_json::to_value(prefs).unwrap();
        value["capsuleTranscriptFontSize"] = 0.into();
        assert_eq!(
            serde_json::from_value::<UserPreferences>(value)
                .unwrap()
                .capsule_transcript_font_size,
            12
        );
    }
}
