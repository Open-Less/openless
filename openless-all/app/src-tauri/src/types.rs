//! Shared value types crossing the IPC boundary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PolishMode {
    Raw,
    #[default]
    Light,
    Structured,
    Formal,
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

/// Auto-update 渠道。决定 Settings → 关于 里展示哪一类版本信息。
/// `Stable` 沿用 `tauri-plugin-updater` 的默认 endpoints（即 `tauri.conf.json`
/// 里的 `latest-{{target}}-{{arch}}.json`），与发版 pipeline 对齐。
/// `Beta` 不动 plugin endpoints —— 只解锁 Settings 里"手动下载最新 Beta"的入口
/// （fetch GitHub `prerelease` + 跳浏览器），物理隔离 Beta 包不会通过 auto-update
/// 推到正式版用户。详见 README 的"Contributing workflow"和 CLAUDE.md 的
/// `Branch & release-channel workflow` 段落。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InsertStatus {
    Inserted,
    PasteSent,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabPreset {
    pub id: String,
    pub name: String,
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct VocabPresetStore {
    pub custom: Vec<VocabPreset>,
    pub overrides: Vec<VocabPreset>,
    pub disabled_builtin_preset_ids: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserPreferences {
    pub hotkey: HotkeyBinding,
    pub dictation_hotkey: ShortcutBinding,
    pub default_mode: PolishMode,
    pub enabled_modes: Vec<PolishMode>,
    pub launch_at_login: bool,
    pub show_capsule: bool,
    /// 录音期间临时静音系统输出，停止/取消/出错后恢复原静音状态。
    #[serde(default)]
    pub mute_during_recording: bool,
    /// 录音输入设备名称。空字符串 = 使用系统默认麦克风。
    #[serde(default)]
    pub microphone_device_name: String,
    pub active_asr_provider: String, // "volcengine" | "apple-speech" | ...
    pub active_llm_provider: String, // "ark" | "openai" | ...
    /// Windows/Linux 粘贴成功后是否恢复用户原剪贴板。默认 true 跟历史行为一致；
    /// 关掉就把听写文本留在剪贴板，让 simulate_paste 实际没生效时用户能 Ctrl+V 找回。
    /// macOS 走 AX 直写，不受这个开关影响。详见 issue #111。
    pub restore_clipboard_after_paste: bool,
    /// Windows: 是否允许 TSF 失败后继续使用 SendInput / 粘贴类非 TSF 兜底。
    /// 默认开启以保持可用性；关闭后可验证文本是否真正由 TSF 上屏。
    #[serde(default = "default_true")]
    pub allow_non_tsf_insertion_fallback: bool,
    /// 用户的工作语言（多选，原生名）。会作为前提注入 LLM polish/translate 的 system prompt 头部，
    /// 让模型知道该用户在哪些语言间工作。详见 issue #4。
    #[serde(default = "default_working_languages")]
    pub working_languages: Vec<String>,
    /// 翻译输出的目标语言（单选，原生名）。空串 = 不启用翻译模式（Shift 组合键无效）。
    /// 由前端从内置语言列表中选择，后端只接收最终的原生名字符串拼进 prompt。详见 issue #4。
    #[serde(default)]
    pub translation_target_language: String,
    /// 中文输出字形偏好（不额外暴露为 UI 开关）：
    /// - Simplified: 中文输出优先简体
    /// - Traditional: 中文输出优先繁体
    /// - Auto: 不额外约束
    ///
    /// 由前端「界面语言」选择同步驱动（简体/繁体），详见 issue #259。
    #[serde(default)]
    pub chinese_script_preference: ChineseScriptPreference,
    /// 最终输出语言偏好（不额外暴露为 UI 开关）：
    /// 由前端「界面语言」选择同步驱动：zh-CN/zh-TW/en/ja/ko，其他为 Auto。
    #[serde(default)]
    pub output_language_preference: OutputLanguagePreference,
    /// 划词语音问答（QA）的全局快捷键。`None` = 关闭功能；`Some(...)` 时
    /// coordinator 用 global-hotkey crate 注册组合键（modifier + 主键）。
    /// 默认 Cmd+Shift+; (macOS) / Ctrl+Shift+; (Windows)。详见 issue #118。
    #[serde(default = "default_qa_hotkey")]
    pub qa_hotkey: Option<ShortcutBinding>,
    /// 是否把每次 QA 会话写进 history.json。默认 false：QA 默认临时不留痕。
    /// 详见 issue #118。
    #[serde(default)]
    pub qa_save_history: bool,
    /// 自定义录音组合键。当 `hotkey.trigger == Custom` 时，coordinator 用
    /// `global-hotkey` crate 注册此组合键（支持 Toggle + Hold 模式）。
    /// `None` 且 trigger == Custom 表示用户选了自定义但还没录制。
    #[serde(default)]
    pub custom_combo_hotkey: Option<ComboBinding>,
    #[serde(default = "default_translation_hotkey")]
    pub translation_hotkey: ShortcutBinding,
    #[serde(default = "default_switch_style_hotkey")]
    pub switch_style_hotkey: ShortcutBinding,
    #[serde(default = "default_open_app_hotkey")]
    pub open_app_hotkey: ShortcutBinding,
    /// 本地 Qwen3-ASR 当前激活的模型 id（"qwen3-asr-0.6b" / "qwen3-asr-1.7b"）。
    /// 仅在 active_asr_provider == "local-qwen3" 时有意义。
    #[serde(default = "default_local_asr_model")]
    pub local_asr_active_model: String,
    /// 本地模型下载源镜像（"huggingface" / "hf-mirror"）。
    #[serde(default = "default_local_asr_mirror")]
    pub local_asr_mirror: String,
    /// 本地 ASR 引擎在内存中的保留时长（秒）。0 = 说完话即释放；
    /// 较大值 = 上次使用后驻留 N 秒再释放；86400 = 一天 ≈ 永不释放。
    /// 默认 300（5 分钟）：兼顾连续听写不重加载、长时间不用释放 1.2GB+ RAM。
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    pub local_asr_keep_loaded_secs: u32,
    /// Windows Foundry Local Whisper 当前激活的模型 alias。
    #[serde(default = "default_foundry_local_asr_model")]
    pub foundry_local_asr_model: String,
    /// Windows Foundry Local Whisper 语言 hint。空字符串 = 自动检测。
    #[serde(default)]
    pub foundry_local_asr_language_hint: String,
    /// Windows Foundry Local Whisper 模型在 runtime 中保持加载多久。
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    pub foundry_local_asr_keep_loaded_secs: u32,
    /// Auto-update 渠道偏好。stable = 跟正式版（默认）；beta = Settings 里多
    /// 一个手动下载 Beta 的入口。不影响 plugin-updater 的自动检查路径。
    #[serde(default)]
    pub update_channel: UpdateChannel,
}

fn default_local_asr_model() -> String {
    "qwen3-asr-0.6b".into()
}

fn default_local_asr_mirror() -> String {
    "huggingface".into()
}

fn default_local_asr_keep_loaded_secs() -> u32 {
    300
}

fn default_foundry_local_asr_model() -> String {
    crate::asr::local::foundry::DEFAULT_MODEL_ALIAS.into()
}

fn default_active_asr_provider() -> String {
    #[cfg(target_os = "windows")]
    {
        return crate::asr::local::foundry::PROVIDER_ID.into();
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
    launch_at_login: bool,
    show_capsule: bool,
    #[serde(default)]
    mute_during_recording: bool,
    #[serde(default)]
    microphone_device_name: String,
    active_asr_provider: String,
    active_llm_provider: String,
    restore_clipboard_after_paste: bool,
    allow_non_tsf_insertion_fallback: bool,
    working_languages: Vec<String>,
    translation_target_language: String,
    chinese_script_preference: ChineseScriptPreference,
    #[serde(default)]
    output_language_preference: OutputLanguagePreference,
    qa_hotkey: Option<ShortcutBinding>,
    qa_save_history: bool,
    custom_combo_hotkey: Option<ComboBinding>,
    translation_hotkey: Option<ShortcutBinding>,
    switch_style_hotkey: Option<ShortcutBinding>,
    open_app_hotkey: Option<ShortcutBinding>,
    #[serde(default = "default_local_asr_model")]
    local_asr_active_model: String,
    #[serde(default = "default_local_asr_mirror")]
    local_asr_mirror: String,
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    local_asr_keep_loaded_secs: u32,
    #[serde(default = "default_foundry_local_asr_model")]
    foundry_local_asr_model: String,
    #[serde(default)]
    foundry_local_asr_language_hint: String,
    #[serde(default = "default_local_asr_keep_loaded_secs")]
    foundry_local_asr_keep_loaded_secs: u32,
    #[serde(default)]
    update_channel: UpdateChannel,
}

impl Default for UserPreferencesWire {
    fn default() -> Self {
        let prefs = UserPreferences::default();
        Self {
            hotkey: prefs.hotkey,
            dictation_hotkey: None,
            default_mode: prefs.default_mode,
            enabled_modes: prefs.enabled_modes,
            launch_at_login: prefs.launch_at_login,
            show_capsule: prefs.show_capsule,
            mute_during_recording: prefs.mute_during_recording,
            microphone_device_name: prefs.microphone_device_name,
            active_asr_provider: prefs.active_asr_provider,
            active_llm_provider: prefs.active_llm_provider,
            restore_clipboard_after_paste: prefs.restore_clipboard_after_paste,
            allow_non_tsf_insertion_fallback: prefs.allow_non_tsf_insertion_fallback,
            working_languages: prefs.working_languages,
            translation_target_language: prefs.translation_target_language,
            chinese_script_preference: prefs.chinese_script_preference,
            output_language_preference: prefs.output_language_preference,
            qa_hotkey: prefs.qa_hotkey,
            qa_save_history: prefs.qa_save_history,
            custom_combo_hotkey: prefs.custom_combo_hotkey,
            translation_hotkey: None,
            switch_style_hotkey: None,
            open_app_hotkey: None,
            local_asr_active_model: prefs.local_asr_active_model,
            local_asr_mirror: prefs.local_asr_mirror,
            local_asr_keep_loaded_secs: prefs.local_asr_keep_loaded_secs,
            foundry_local_asr_model: prefs.foundry_local_asr_model,
            foundry_local_asr_language_hint: prefs.foundry_local_asr_language_hint,
            foundry_local_asr_keep_loaded_secs: prefs.foundry_local_asr_keep_loaded_secs,
            update_channel: prefs.update_channel,
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
        Ok(Self {
            hotkey: wire.hotkey,
            dictation_hotkey,
            default_mode: wire.default_mode,
            enabled_modes: wire.enabled_modes,
            launch_at_login: wire.launch_at_login,
            show_capsule: wire.show_capsule,
            mute_during_recording: wire.mute_during_recording,
            microphone_device_name: wire.microphone_device_name,
            active_asr_provider: wire.active_asr_provider,
            active_llm_provider: wire.active_llm_provider,
            restore_clipboard_after_paste: wire.restore_clipboard_after_paste,
            allow_non_tsf_insertion_fallback: wire.allow_non_tsf_insertion_fallback,
            working_languages: wire.working_languages,
            translation_target_language: wire.translation_target_language,
            chinese_script_preference: wire.chinese_script_preference,
            output_language_preference: wire.output_language_preference,
            qa_hotkey: wire.qa_hotkey,
            qa_save_history: wire.qa_save_history,
            custom_combo_hotkey: wire.custom_combo_hotkey,
            translation_hotkey: wire
                .translation_hotkey
                .unwrap_or_else(default_translation_hotkey),
            switch_style_hotkey: wire
                .switch_style_hotkey
                .unwrap_or_else(default_switch_style_hotkey),
            open_app_hotkey: wire.open_app_hotkey.unwrap_or_else(default_open_app_hotkey),
            local_asr_active_model: wire.local_asr_active_model,
            local_asr_mirror: wire.local_asr_mirror,
            local_asr_keep_loaded_secs: wire.local_asr_keep_loaded_secs,
            foundry_local_asr_model: wire.foundry_local_asr_model,
            foundry_local_asr_language_hint: wire.foundry_local_asr_language_hint,
            foundry_local_asr_keep_loaded_secs: wire.foundry_local_asr_keep_loaded_secs,
            update_channel: wire.update_channel,
        })
    }
}

fn default_qa_hotkey() -> Option<ShortcutBinding> {
    Some(ShortcutBinding::default_qa())
}

fn default_translation_hotkey() -> ShortcutBinding {
    ShortcutBinding {
        primary: "Shift".into(),
        modifiers: Vec::new(),
    }
}

fn default_switch_style_hotkey() -> ShortcutBinding {
    ShortcutBinding {
        primary: "S".into(),
        modifiers: default_app_shortcut_modifiers(),
    }
}

fn default_open_app_hotkey() -> ShortcutBinding {
    ShortcutBinding {
        primary: "O".into(),
        modifiers: default_app_shortcut_modifiers(),
    }
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
    Ok(crate::shortcut_binding::binding_from_legacy_trigger(
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
            default_mode: PolishMode::Light,
            enabled_modes: vec![
                PolishMode::Raw,
                PolishMode::Light,
                PolishMode::Structured,
                PolishMode::Formal,
            ],
            launch_at_login: false,
            show_capsule: true,
            mute_during_recording: false,
            microphone_device_name: String::new(),
            active_asr_provider: default_active_asr_provider(),
            active_llm_provider: "ark".into(),
            restore_clipboard_after_paste: true,
            allow_non_tsf_insertion_fallback: true,
            working_languages: default_working_languages(),
            translation_target_language: String::new(),
            chinese_script_preference: ChineseScriptPreference::Auto,
            output_language_preference: OutputLanguagePreference::Auto,
            qa_hotkey: default_qa_hotkey(),
            qa_save_history: false,
            custom_combo_hotkey: None,
            translation_hotkey: default_translation_hotkey(),
            switch_style_hotkey: default_switch_style_hotkey(),
            open_app_hotkey: default_open_app_hotkey(),
            local_asr_active_model: default_local_asr_model(),
            local_asr_mirror: default_local_asr_mirror(),
            local_asr_keep_loaded_secs: default_local_asr_keep_loaded_secs(),
            foundry_local_asr_model: default_foundry_local_asr_model(),
            foundry_local_asr_language_hint: String::new(),
            foundry_local_asr_keep_loaded_secs: default_local_asr_keep_loaded_secs(),
            update_channel: UpdateChannel::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
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

/// 划词语音问答的全局快捷键绑定。原生名字符串：
/// - `primary`：主键（如 `";"`、`"."`、`"A"`、`"F1"`）。
/// - `modifiers`：修饰键集合，元素来自 `{"cmd","ctrl","alt","shift","super"}`。
///   小写名简单序列化即可，前端 / 后端解析时统一 lowercase。
///
/// 默认 `Cmd+Shift+;` (macOS) / `Ctrl+Shift+;` (Windows)。
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
    /// 渲染成给前端展示的可读标签。
    /// 顺序与人类阅读习惯一致：`Cmd+Shift+;`、`Ctrl+Alt+Shift+.`。
    pub fn display_label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        // 固定输出顺序：Ctrl/Cmd → Alt/Option → Shift → Super
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

/// 录音快捷键的自定义组合键绑定。结构与 `QaHotkeyBinding` 相同：
/// - `primary`：主键（如 `"D"`、`"Space"`、`"F1"`）。
/// - `modifiers`：修饰键集合，元素来自 `{"cmd","ctrl","alt","shift","super"}`。
///
/// 当 `HotkeyBinding.trigger == Custom` 时，coordinator 用 `global-hotkey` crate
/// 注册此组合键，而非 modifier-only 的 CGEventTap / WH_KEYBOARD_LL。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComboBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
}

impl ComboBinding {
    /// 渲染成给前端展示的可读标签。复用 QaHotkeyBinding 的格式化逻辑。
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
    // 单个字母键归一为大写显示（"a" → "A"）；其余原样（如 ";"、"F1"）。
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
    Fn,
    RightAlt, // Windows synonym for RightOption
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
            HotkeyTrigger::Fn => "Fn (地球键)",
            HotkeyTrigger::RightAlt => "右 Alt",
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
        #[cfg(target_os = "windows")]
        HotkeyTrigger::Fn => "ControlRight",
        #[cfg(not(target_os = "windows"))]
        HotkeyTrigger::Fn => "Fn",
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
            return Self {
                adapter: HotkeyAdapterKind::WindowsLowLevel,
                available_triggers: vec![
                    HotkeyTrigger::RightControl,
                    HotkeyTrigger::RightAlt,
                    HotkeyTrigger::LeftControl,
                    HotkeyTrigger::RightCommand,
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
                    HotkeyTrigger::Custom,
                ],
                requires_accessibility_permission: false,
                supports_modifier_only_trigger: true,
                supports_side_specific_modifiers: true,
                explicit_fallback_available: false,
                status_hint: Some(
                    "Linux 仅 best-effort：X11 可尝试 rdev 监听；Wayland 会明确提示暂不支持全局热键。".into(),
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
                mode: HotkeyMode::Toggle,
                keys: Some(vec![HotkeyKey::new("ControlRight")]),
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            Self {
                trigger: HotkeyTrigger::RightOption,
                mode: HotkeyMode::Toggle,
                keys: Some(vec![HotkeyKey::new("AltRight")]),
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
    /// 当前 session 是否处于翻译模式（用户按过 Shift）。前端用它在胶囊顶部
    /// 渲染"正在翻译"标签，让用户立刻知道这次输出会走翻译管线。详见 issue #4。
    pub translation: bool,
}

/// Snapshot of credentials read from vault — only what the UI needs to know
/// (whether keys are set; never the values themselves).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsStatus {
    pub active_asr_provider: String,
    pub active_llm_provider: String,
    pub asr_configured: bool,
    pub llm_configured: bool,
    // 兼容旧前端字段（逐步迁移中）
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

/// 划词追问浮窗里一条对话消息。多轮提问会累积成 Vec<QaChatMessage>，
/// 整段送给 LLM 维持上下文。详见 issue #118 v2。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaChatMessage {
    /// "user" | "assistant" — 直接对应 OpenAI 消息 role 字段。
    pub role: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn legacy_custom_hotkey_without_custom_binding_is_rejected() {
        let result = serde_json::from_str::<UserPreferences>(
            r#"{
                "hotkey": { "trigger": "custom", "mode": "toggle" }
            }"#,
        );

        assert!(result.is_err());
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
}
