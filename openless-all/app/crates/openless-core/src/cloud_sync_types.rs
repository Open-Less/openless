//! Versioned, portable user data for the official cloud service.
//!
//! These types deliberately do not embed the app's complete preferences or style-pack structs:
//! credentials, provider connections, device permissions and local paths stay on the device.

use crate::shared_types::{
    ChineseScriptPreference as SyncChineseScriptPreference,
    OutputLanguagePreference as SyncOutputLanguagePreference, ThemeMode as SyncThemeMode,
};
use crate::PolishMode;
use crate::{
    CapsuleStyle as SyncCapsuleStyle, RuleSource as SyncRuleSource,
    StylePackKind as SyncStylePackKind,
};
use serde::{Deserialize, Serialize};

pub const SYNC_SCHEMA_VERSION: u32 = 1;
pub const SYNC_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Revisions stay exact in JavaScript clients as well as in SQLite.
pub const SYNC_MAX_REVISION: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudSyncSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub updated_at: Option<String>,
    /// `None` means no snapshot or a deleted snapshot. A deletion still has a nonzero revision.
    pub payload: Option<CloudSyncPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudSyncPutRequest {
    pub schema_version: u32,
    /// The last revision read by this device; zero creates its first cloud snapshot.
    pub base_revision: u64,
    pub payload: CloudSyncPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudSyncDeleteRequest {
    pub base_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudSyncPayload {
    pub dictionary: Vec<SyncDictionaryEntry>,
    pub corrections: Vec<SyncCorrectionRule>,
    pub style_packs: Vec<SyncStylePack>,
    pub preferences: SyncPreferences,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncDictionaryEntry {
    pub id: String,
    pub phrase: String,
    pub note: Option<String>,
    pub enabled: bool,
    pub hits: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncCorrectionRule {
    pub id: String,
    pub pattern: String,
    pub replacement: String,
    pub enabled: bool,
    pub created_at: String,
    pub source: SyncRuleSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncStylePack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: Option<String>,
    pub version: String,
    pub kind: SyncStylePackKind,
    pub base_mode: PolishMode,
    pub selection_prompt: String,
    pub prompt: String,
    pub examples: Vec<SyncStylePackExample>,
    pub tags: Vec<String>,
    /// Raw standard-base64 PNG, at most 64 KiB decoded. This is never a filesystem path or URL.
    pub icon_png_base64: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub enabled: bool,
    pub recommended_model: Option<String>,
    pub compatible_app_version: Option<String>,
    pub origin_pack_id: Option<String>,
    pub origin_author_login: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncStylePackExample {
    pub title: Option<String>,
    pub input: String,
    pub output: String,
}

/// An allowlist of user-facing preferences. Missing keys leave the receiving device unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyncPreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_mode: Option<SyncThemeMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capsule_style: Option<SyncCapsuleStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_mode: Option<PolishMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_modes: Option<Vec<PolishMode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_style_pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_polish_style_pack_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_languages: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translation_target_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chinese_script_preference: Option<SyncChineseScriptPreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_language_preference: Option<SyncOutputLanguagePreference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qa_save_history: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_capsule: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_cue_on_record: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute_during_recording: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_auto_stop_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silence_auto_stop_seconds: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_overview_activity_heatmap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_thinking_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<SyncLocale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_scale: Option<SyncFontScale>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncLocale {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "en")]
    En,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "de")]
    De,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncFontScale {
    Small,
    Medium,
    Large,
}
