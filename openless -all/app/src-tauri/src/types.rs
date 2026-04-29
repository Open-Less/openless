//! Shared value types crossing the IPC boundary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolishMode {
    Raw,
    Light,
    Structured,
    Formal,
}

impl Default for PolishMode {
    fn default() -> Self {
        PolishMode::Light
    }
}

impl PolishMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            PolishMode::Raw => "原文",
            PolishMode::Light => "轻度润色",
            PolishMode::Structured => "清晰结构",
            PolishMode::Formal => "正式表达",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InsertStatus {
    Inserted,
    CopiedFallback,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationSession {
    pub id: String,
    pub created_at: String, // ISO-8601
    pub raw_transcript: String,
    pub final_text: String,
    pub mode: PolishMode,
    pub app_bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub insert_status: InsertStatus,
    pub error_code: Option<String>,
    pub duration_ms: Option<u64>,
    pub dictionary_entry_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub phrase: String,
    /// Swift `DictionaryEntry.swift` 用的是 `notes`(复数)；Rust 用 `note`(单数)。
    /// alias 接受老文件 + 自身字段名。
    #[serde(default, alias = "notes")]
    pub note: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Swift 用 `hitCount`,Rust 用 `hits`。alias + default 让老文件不缺字段。
    #[serde(default, alias = "hitCount")]
    pub hits: u64,
    /// Swift 写 ISO8601;Rust 也用 String,直接通过。
    #[serde(default)]
    pub created_at: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferences {
    pub hotkey: HotkeyBinding,
    pub default_mode: PolishMode,
    pub enabled_modes: Vec<PolishMode>,
    pub launch_at_login: bool,
    pub show_capsule: bool,
    pub active_asr_provider: String, // "volcengine" | "apple-speech" | ...
    pub active_llm_provider: String, // "ark" | "openai" | ...
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding::default(),
            default_mode: PolishMode::Light,
            enabled_modes: vec![
                PolishMode::Raw,
                PolishMode::Light,
                PolishMode::Structured,
                PolishMode::Formal,
            ],
            launch_at_login: false,
            show_capsule: true,
            active_asr_provider: "volcengine".into(),
            active_llm_provider: "ark".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyTrigger {
    RightOption,
    LeftOption,
    RightControl,
    LeftControl,
    RightCommand,
    Fn,
    RightAlt, // Windows synonym for RightOption
}

impl HotkeyTrigger {
    pub fn display_name(&self) -> &'static str {
        match self {
            HotkeyTrigger::RightOption => "右 Option",
            HotkeyTrigger::LeftOption => "左 Option",
            HotkeyTrigger::RightControl => "右 Control",
            HotkeyTrigger::LeftControl => "左 Control",
            HotkeyTrigger::RightCommand => "右 Command",
            HotkeyTrigger::Fn => "Fn (地球键)",
            HotkeyTrigger::RightAlt => "右 Alt",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyMode {
    Toggle,
    Hold,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyAdapterKind {
    MacEventTap,
    WindowsLowLevel,
    Rdev,
}

impl HotkeyAdapterKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            HotkeyAdapterKind::MacEventTap => "macOS Event Tap",
            HotkeyAdapterKind::WindowsLowLevel => "Windows 低层键盘 hook",
            HotkeyAdapterKind::Rdev => "rdev 监听器",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyBinding {
    pub trigger: HotkeyTrigger,
    pub mode: HotkeyMode,
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
        #[cfg(target_os = "macos")]
        {
            return Self {
                adapter: HotkeyAdapterKind::MacEventTap,
                available_triggers: vec![
                    HotkeyTrigger::RightOption,
                    HotkeyTrigger::LeftOption,
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::LeftControl,
                    HotkeyTrigger::RightCommand,
                    HotkeyTrigger::Fn,
                ],
                requires_accessibility_permission: true,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some("授权辅助功能后，通常需要完全退出并重新打开 OpenLess。".into()),
            };
        }

        #[cfg(target_os = "windows")]
        {
            return Self {
                adapter: HotkeyAdapterKind::WindowsLowLevel,
                available_triggers: vec![
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::RightAlt,
                    HotkeyTrigger::LeftControl,
                    HotkeyTrigger::RightCommand,
                ],
                requires_accessibility_permission: false,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some(
                    "默认建议使用“右 Control + 按住说话”；若无响应，可在权限页查看 hook 安装状态。"
                        .into(),
                ),
            };
        }

        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            Self {
                adapter: HotkeyAdapterKind::Rdev,
                available_triggers: vec![
                    HotkeyTrigger::RightAlt,
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::LeftControl,
                ],
                requires_accessibility_permission: false,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some(
                    "Linux 仅 best-effort：不同桌面环境 / Wayland 组合可能限制全局热键。".into(),
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
        #[cfg(target_os = "windows")]
        {
            Self {
                trigger: HotkeyTrigger::RightControl,
                mode: HotkeyMode::Hold,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            Self {
                trigger: HotkeyTrigger::RightOption,
                mode: HotkeyMode::Toggle,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsulePayload {
    pub state: CapsuleState,
    pub level: f32, // 0..1 RMS
    pub elapsed_ms: u64,
    pub message: Option<String>,
    pub inserted_chars: Option<u32>,
}

/// Snapshot of credentials read from vault — only what the UI needs to know
/// (whether keys are set; never the values themselves).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsStatus {
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
