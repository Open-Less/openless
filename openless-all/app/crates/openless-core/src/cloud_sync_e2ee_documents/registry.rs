//! Explicit sync classification for every persisted UserPreferences field.
use super::types::{DocumentError, DocumentResult, SyncNamespace};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceClass {
    Portable,
    DeviceProfile,
    Excluded,
}
#[derive(Debug, Clone, Copy)]
pub enum PreferenceShape {
    Boolean,
    Unsigned8,
    Unsigned16,
    Unsigned32,
    Number,
    Text,
    TextList,
    OptionalText,
    OptionalUnsigned32,
    Object,
    OptionalObject,
    ObjectList,
}
#[derive(Debug, Clone, Copy)]
pub struct PreferenceField {
    pub rust_name: &'static str,
    pub key: &'static str,
    pub class: PreferenceClass,
    pub shape: PreferenceShape,
    pub reason: &'static str,
}
pub const PREFERENCE_FIELDS: &[PreferenceField] = &[
    PreferenceField {
        rust_name: "hotkey",
        key: "hotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Object,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "dictation_hotkey",
        key: "dictationHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Object,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "default_mode",
        key: "defaultMode",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "enabled_modes",
        key: "enabledModes",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::TextList,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "active_style_pack_id",
        key: "activeStylePackId",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "style_system_prompts",
        key: "styleSystemPrompts",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Object,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "custom_style_prompts",
        key: "customStylePrompts",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Object,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "launch_at_login",
        key: "launchAtLogin",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Boolean,
        reason: "local_os_autostart",
    },
    PreferenceField {
        rust_name: "show_capsule",
        key: "showCapsule",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "capsule_style",
        key: "capsuleStyle",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "capsule_transcript_enabled",
        key: "capsuleTranscriptEnabled",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "capsule_transcript_font_size",
        key: "capsuleTranscriptFontSize",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Unsigned8,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "mute_during_recording",
        key: "muteDuringRecording",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "stable_transcription_enabled",
        key: "stableTranscriptionEnabled",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "audio_cue_on_record",
        key: "audioCueOnRecord",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "silence_auto_stop_enabled",
        key: "silenceAutoStopEnabled",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "silence_auto_stop_seconds",
        key: "silenceAutoStopSeconds",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Number,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "microphone_device_name",
        key: "microphoneDeviceName",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "active_asr_provider",
        key: "activeAsrProvider",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Text,
        reason: "selection_in_channel_unit",
    },
    PreferenceField {
        rust_name: "active_llm_provider",
        key: "activeLlmProvider",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Text,
        reason: "selection_in_channel_unit",
    },
    PreferenceField {
        rust_name: "pipeline_mode",
        key: "pipelineMode",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "multimodal_pipeline_enabled",
        key: "multimodalPipelineEnabled",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "active_omni_provider",
        key: "activeOmniProvider",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Text,
        reason: "selection_in_channel_unit",
    },
    PreferenceField {
        rust_name: "llm_thinking_enabled",
        key: "llmThinkingEnabled",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "use_system_proxy",
        key: "useSystemProxy",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Boolean,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "restore_clipboard_after_paste",
        key: "restoreClipboardAfterPaste",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Boolean,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "paste_shortcut",
        key: "pasteShortcut",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "allow_non_tsf_insertion_fallback",
        key: "allowNonTsfInsertionFallback",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Boolean,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "windows_insertion_mode",
        key: "windowsInsertionMode",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "windows_sendinput_newline_mode",
        key: "windowsSendInputNewlineMode",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "macos_newline_mode",
        key: "macosNewlineMode",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "windows_sendinput_insertion_only",
        key: "windowsSendInputInsertionOnly",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Boolean,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "windows_show_openless_in_keyboard_list",
        key: "windowsShowOpenlessInKeyboardList",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Boolean,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "working_languages",
        key: "workingLanguages",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::TextList,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "translation_target_language",
        key: "translationTargetLanguage",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "chinese_script_preference",
        key: "chineseScriptPreference",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "output_language_preference",
        key: "outputLanguagePreference",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "qa_hotkey",
        key: "qaHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "quick_note_hotkey",
        key: "quickNoteHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "selection_polish_hotkey",
        key: "selectionPolishHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "selection_polish_style_pack_id",
        key: "selectionPolishStylePackId",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_polish_output_mode",
        key: "selectionPolishOutputMode",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_voice_enabled",
        key: "selectionVoiceEnabled",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_voice_intent_mode",
        key: "selectionVoiceIntentMode",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_voice_manual_intent",
        key: "selectionVoiceManualIntent",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_voice_edit_keywords",
        key: "selectionVoiceEditKeywords",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::TextList,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_voice_edit_plan_format",
        key: "selectionVoiceEditPlanFormat",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "selection_voice_edit_system_prompt",
        key: "selectionVoiceEditSystemPrompt",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "qa_save_history",
        key: "qaSaveHistory",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "custom_combo_hotkey",
        key: "customComboHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "translation_hotkey",
        key: "translationHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Object,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "switch_style_hotkey",
        key: "switchStyleHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "open_app_hotkey",
        key: "openAppHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "style_pack_hotkeys",
        key: "stylePackHotkeys",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::ObjectList,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_enabled",
        key: "codingAgentEnabled",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Boolean,
        reason: "local_consent_or_secret",
    },
    PreferenceField {
        rust_name: "coding_agent_provider",
        key: "codingAgentProvider",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_model",
        key: "codingAgentModel",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalText,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_permission_mode",
        key: "codingAgentPermissionMode",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_workdir",
        key: "codingAgentWorkdir",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalText,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_exe",
        key: "codingAgentExe",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalText,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_voice_hotkey",
        key: "codingAgentVoiceHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_panel_hotkey",
        key: "codingAgentPanelHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "coding_agent_quick_hotkey",
        key: "codingAgentQuickHotkey",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::OptionalObject,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "remote_input_enabled",
        key: "remoteInputEnabled",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Boolean,
        reason: "local_network_consent",
    },
    PreferenceField {
        rust_name: "remote_input_port",
        key: "remoteInputPort",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Unsigned16,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "remote_input_pin",
        key: "remoteInputPin",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Text,
        reason: "local_consent_or_secret",
    },
    PreferenceField {
        rust_name: "remote_input_default_mode",
        key: "remoteInputDefaultMode",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "local_asr_active_model",
        key: "localAsrActiveModel",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "local_whisper_active_model",
        key: "localWhisperActiveModel",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "local_asr_mirror",
        key: "localAsrMirror",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "local_asr_keep_loaded_secs",
        key: "localAsrKeepLoadedSecs",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Unsigned32,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "local_asr_models_base_dir",
        key: "localAsrModelsBaseDir",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "foundry_local_asr_model",
        key: "foundryLocalAsrModel",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "foundry_local_runtime_source",
        key: "foundryLocalRuntimeSource",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "foundry_local_asr_language_hint",
        key: "foundryLocalAsrLanguageHint",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "foundry_local_asr_keep_loaded_secs",
        key: "foundryLocalAsrKeepLoadedSecs",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Unsigned32,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "sherpa_onnx_model",
        key: "sherpaOnnxModel",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "sherpa_onnx_language_hint",
        key: "sherpaOnnxLanguageHint",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "sherpa_onnx_keep_loaded_secs",
        key: "sherpaOnnxKeepLoadedSecs",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Unsigned32,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "update_channel",
        key: "updateChannel",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "update_channel_explicit",
        key: "updateChannelExplicit",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "history_retention_days",
        key: "historyRetentionDays",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Unsigned32,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "polish_context_window_minutes",
        key: "polishContextWindowMinutes",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Unsigned32,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "start_minimized",
        key: "startMinimized",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "theme_mode",
        key: "themeMode",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Text,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "streaming_insert",
        key: "streamingInsert",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "streaming_insert_default_migrated",
        key: "streamingInsertDefaultMigrated",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Boolean,
        reason: "local_consent_or_secret",
    },
    PreferenceField {
        rust_name: "streaming_insert_save_clipboard",
        key: "streamingInsertSaveClipboard",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "cursor_context_enabled",
        key: "cursorContextEnabled",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Boolean,
        reason: "local_consent_or_secret",
    },
    PreferenceField {
        rust_name: "show_overview_activity_heatmap",
        key: "showOverviewActivityHeatmap",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "stacked_row_layout",
        key: "stackedRowLayout",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "conservative_layout",
        key: "conservativeLayout",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "auto_update_check",
        key: "autoUpdateCheck",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "history_max_entries",
        key: "historyMaxEntries",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::OptionalUnsigned32,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "record_audio_for_debug",
        key: "recordAudioForDebug",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::Boolean,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "audio_recording_max_entries",
        key: "audioRecordingMaxEntries",
        class: PreferenceClass::Portable,
        shape: PreferenceShape::OptionalUnsigned32,
        reason: "portable_setting",
    },
    PreferenceField {
        rust_name: "quick_note_export_directory",
        key: "quickNoteExportDirectory",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "marketplace_base_url",
        key: "marketplaceBaseUrl",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "marketplace_dev_login",
        key: "marketplaceDevLogin",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Text,
        reason: "local_consent_or_secret",
    },
    PreferenceField {
        rust_name: "android_insert_strategy",
        key: "androidInsertStrategy",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "android_overlay_trigger",
        key: "androidOverlayTrigger",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "android_overlay_activation_mode",
        key: "androidOverlayActivationMode",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "android_overlay_left_swipe_action",
        key: "androidOverlayLeftSwipeAction",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "android_overlay_cancel_swipe_direction",
        key: "androidOverlayCancelSwipeDirection",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Text,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "android_overlay_gesture_actions",
        key: "androidOverlayGestureActions",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Object,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "android_overlay_size_dp",
        key: "androidOverlaySizeDp",
        class: PreferenceClass::DeviceProfile,
        shape: PreferenceShape::Unsigned32,
        reason: "device_bound",
    },
    PreferenceField {
        rust_name: "splash_seen_version",
        key: "splashSeenVersion",
        class: PreferenceClass::Excluded,
        shape: PreferenceShape::Text,
        reason: "local_consent_or_secret",
    },
];

pub fn preference_field(key: &str) -> Option<&'static PreferenceField> {
    PREFERENCE_FIELDS.iter().find(|field| field.key == key)
}

pub fn excluded_extension_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "oauth",
        "githubtoken",
        "accesstoken",
        "refreshtoken",
        "sessioncookie",
        "privatekey",
        "remotepin",
        "remoteinputpin",
        "syncpassword",
        "synckey",
        "synctoken",
        "syncenabled",
        "syncconsent",
        "syncqueue",
        "syncbaseline",
        "synclock",
        "permissiongrant",
    ]
    .iter()
    .any(|item| normalized.contains(item))
}

pub fn validate_preference_value(field: &PreferenceField, value: &Value) -> DocumentResult<()> {
    use PreferenceShape::*;
    let valid = match field.shape {
        Boolean => value.is_boolean(),
        Unsigned8 => value.as_u64().is_some_and(|n| n <= u8::MAX.into()),
        Unsigned16 => value.as_u64().is_some_and(|n| n <= u16::MAX.into()),
        Unsigned32 => value.as_u64().is_some_and(|n| n <= u32::MAX.into()),
        Number => value.as_f64().is_some_and(f64::is_finite),
        Text => value.is_string(),
        TextList => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
        OptionalText => value.is_null() || value.is_string(),
        OptionalUnsigned32 => {
            value.is_null() || value.as_u64().is_some_and(|n| n <= u32::MAX.into())
        }
        Object => value.is_object(),
        OptionalObject => value.is_null() || value.is_object(),
        ObjectList => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_object)),
    };
    if !valid {
        return Err(DocumentError::InvalidDocument);
    }
    let choices: &[&str] = match field.key {
        "defaultMode" => &["raw", "light", "structured", "formal"],
        "capsuleStyle" => &["siri", "classic", "typeless"],
        "pipelineMode" => &["traditional", "multimodal"],
        "chineseScriptPreference" => &["auto", "simplified", "traditional"],
        "outputLanguagePreference" => &["auto", "zhCn", "zhTw", "en", "ja", "ko"],
        "selectionPolishOutputMode" => &["directReplace", "previewConfirm"],
        "selectionVoiceIntentMode" => &["prompt", "auto", "manual", "heuristic"],
        "selectionVoiceManualIntent" => &["question", "edit"],
        "selectionVoiceEditPlanFormat" => &["xml", "json"],
        "updateChannel" => &["stable", "beta"],
        "themeMode" => &["system", "light", "dark"],
        _ => &[],
    };
    if !choices.is_empty() && !value.as_str().is_some_and(|s| choices.contains(&s)) {
        return Err(DocumentError::InvalidDocument);
    }
    if field.key == "enabledModes"
        && !value.as_array().is_some_and(|items| {
            items
                .iter()
                .all(|v| matches!(v.as_str(), Some("raw" | "light" | "structured" | "formal")))
        })
    {
        return Err(DocumentError::InvalidDocument);
    }
    Ok(())
}

pub const ASR_ACCOUNTS: &[&str] = &[
    "asr.api_key",
    "asr.endpoint",
    "asr.model",
    "asr.vocabulary_id",
    "asr.advanced_config",
    "volcengine.app_key",
    "volcengine.access_key",
    "volcengine.resource_id",
    "volcengine.service",
    "volcengine.auth_mode",
    "volcengine.api_key",
    "xfyun.app_id",
    "xfyun.api_key",
    "tencent_cloud.app_id",
    "tencent_cloud.secret_id",
    "tencent_cloud.secret_key",
];
pub const LLM_ACCOUNTS: &[&str] = &[
    "ark.api_key",
    "ark.model_id",
    "ark.endpoint",
    "ark.extra_headers",
    "ark.temperature",
    "ark.request_format",
    "ark.messages_thinking",
    "ark.max_tokens",
    "ark.thinking_budget",
];
pub const OMNI_ACCOUNTS: &[&str] = &[
    "omni.api_key",
    "omni.endpoint",
    "omni.model",
    "omni.extra_headers",
    "omni.temperature",
];
pub fn credential_accounts(namespace: SyncNamespace) -> &'static [&'static str] {
    match namespace {
        SyncNamespace::Asr => ASR_ACCOUNTS,
        SyncNamespace::Llm => LLM_ACCOUNTS,
        SyncNamespace::Omni => OMNI_ACCOUNTS,
    }
}

pub fn is_device_channel(namespace: SyncNamespace, provider: &str) -> bool {
    namespace == SyncNamespace::Asr
        && matches!(
            provider,
            "apple-speech"
                | "local-whisper"
                | "local-qwen3"
                | "local-qwen3-mlx"
                | "local-qwen3-c"
                | "foundry-local-whisper"
                | "sherpa-onnx-local"
        )
}
