#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Tauri command surface — every IPC entry the React UI invokes lives here.
//!
//! issue: 历史上整个 IPC 表（127 个 `#[tauri::command]` + 跨域 helper）挤在单个
//! 4800 行的 `commands.rs` 里。按单一职责拆成 `commands/` 下的域模块（settings /
//! credentials / providers / history / …），每个文件聚焦一个领域。对外路径保持不变：
//! 本模块用 `pub use <domain>::*` glob 重导出每个子模块，`commands::<name>` 仍然解析，
//! `lib.rs` 的 `generate_handler!` 清单与类型引用零改动。`#[tauri::command]` 生成的
//! `__cmd__<name>` 伴生项也随 glob 一并重导出——这是 Tauri 拆分命令文件的标准做法。

use std::sync::Arc;

#[cfg(not(mobile))]
use parking_lot::Mutex;
#[cfg(not(mobile))]
use tauri::Manager;
use tauri::State;

// 跨域共享的 crate 级导入：以 `pub(crate) use` 重导出，子模块用 `use super::*;`
// 即可拿到，避免在 16 个文件里重复同一组 import。
pub(crate) use serde::Serialize;
pub(crate) use serde_json::Value;
pub(crate) use tauri::{AppHandle, Emitter, Window};

#[cfg(not(mobile))]
pub(crate) use crate::asr::local::foundry::{
    model_alias_is_known, FoundryCatalogModel, FoundryPrepareProgressPayload, FoundryRuntimeStatus,
    DEFAULT_MODEL_ALIAS, PROVIDER_ID as FOUNDRY_LOCAL_PROVIDER_ID,
};
#[cfg(not(mobile))]
pub(crate) use crate::asr::local::sherpa::{
    model_alias_is_known as sherpa_model_alias_is_known, SherpaCatalogModel,
    SherpaPrepareProgressPayload, SherpaRuntimeStatus,
    DEFAULT_MODEL_ALIAS as SHERPA_DEFAULT_MODEL_ALIAS,
};
#[cfg(not(mobile))]
pub(crate) use crate::asr::local::sherpa_download::{
    fetch_remote_info as fetch_sherpa_remote_info, SherpaDownloadManager, SherpaRemoteInfo,
};
#[cfg(not(mobile))]
pub(crate) use crate::asr::local::{FoundryLocalRuntime, Mirror, SherpaOnnxRuntime};
pub(crate) use crate::coordinator::Coordinator;
pub(crate) use crate::net;
pub(crate) use crate::permissions::{self, PermissionStatus};
pub(crate) use crate::persistence::{
    sync_style_pack_preferences, ChannelKind, CredentialAccount, CredentialsSnapshot,
    CredentialsVault, PreferencesStore,
};
pub(crate) use crate::polish::{
    http_client_builder, openai_compatible_temperature_for_provider, CodexOAuthConfig,
    CodexOAuthCredentials, CodexOAuthLLMProvider, LLMError, OpenAICompatibleConfig,
    OpenAICompatibleLLMProvider, CODEX_DEFAULT_MODEL, CODEX_OAUTH_PROVIDER_ID,
};
#[cfg(not(mobile))]
pub(crate) use crate::recorder::{AudioConsumer, Recorder};
#[cfg(not(mobile))]
pub(crate) use crate::types::WindowsImeStatus;
pub(crate) use crate::types::{
    builtin_style_pack_id, default_active_style_pack_id, ActivityDay,
    AndroidAccessibilityRecoveryOutcome, AndroidAccessibilityRecoveryResult,
    AndroidAccessibilityStatus, AndroidOverlayStatus, AndroidShizukuStatus,
    ChineseScriptPreference, ComboBinding, CorrectionRule, CredentialsStatus, DictationSession,
    DictionaryEntry, HotkeyCapability, HotkeyStatus, OutputLanguagePreference, PolishMode,
    ShortcutBinding, StylePack, StylePackHotkey, StylePackKind, StylePackRuntimeDiagnostics,
    StyleSystemPrompts, UpdateChannel, UserPreferences, VocabPresetStore,
};

mod channels;
mod credentials;
mod dictation;
mod dictionary;
#[cfg(not(mobile))]
mod foundry_asr;
mod github_oauth;
mod history;
mod hotkeys;
#[cfg(not(mobile))]
mod local_asr;
mod marketplace;
mod misc;
mod permissions_cmds;
mod providers;
mod qa;
#[cfg(not(mobile))]
mod remote_input;
#[cfg(all(not(mobile), debug_assertions))]
mod selection_polish;
#[cfg(not(mobile))]
mod selection_polish_preview;
#[cfg(all(not(mobile), target_os = "windows"))]
mod selection_voice;
mod settings;
#[cfg(not(mobile))]
mod sherpa_asr;
mod style_packs;

pub use channels::*;
pub use credentials::*;
pub use dictation::*;
pub use dictionary::*;
#[cfg(not(mobile))]
pub use foundry_asr::*;
pub use github_oauth::*;
pub use history::*;
pub use hotkeys::*;
#[cfg(not(mobile))]
pub use local_asr::*;
pub use marketplace::*;
pub use misc::*;
pub use permissions_cmds::*;
pub use providers::*;
pub use qa::*;
#[cfg(not(mobile))]
pub use remote_input::*;
pub use settings::*;
// sherpa_onnx_asr_* 命令整组 `#[cfg(target_os = "windows")]`（见 lib.rs 的
// generate_handler! 清单）。非 Windows 平台这组 glob 重导出无人引用，会触发
// unused_imports；这是平台 cfg 的正常结果，不是真正的死代码。
#[cfg(all(not(mobile), debug_assertions))]
pub use selection_polish::*;
#[cfg(not(mobile))]
pub use selection_polish_preview::*;
#[cfg(all(not(mobile), target_os = "windows"))]
pub use selection_voice::*;
#[cfg(not(mobile))]
#[allow(unused_imports)]
pub use sherpa_asr::*;
pub use style_packs::*;

pub(crate) type CoordinatorState<'a> = State<'a, Arc<Coordinator>>;
#[cfg(not(mobile))]
pub type MicrophoneMonitorState = Mutex<Option<Recorder>>;
#[cfg(not(mobile))]
pub type TrayMicrophoneMenuState = Mutex<Vec<TrayMicrophoneMenuItem>>;

#[cfg(not(mobile))]
pub struct TrayMicrophoneMenuItem {
    pub id: String,
    pub device_name: String,
    pub item: tauri::menu::CheckMenuItem<tauri::Wry>,
}

#[cfg(not(mobile))]
pub fn sync_tray_microphone_selection(items: &[TrayMicrophoneMenuItem], device_name: &str) {
    for item in items {
        let _ = item.item.set_checked(item.device_name == device_name);
    }
}

#[cfg(not(mobile))]
pub(crate) struct LevelProbeConsumer;

#[cfg(not(mobile))]
impl AudioConsumer for LevelProbeConsumer {
    fn consume_pcm_chunk(&self, _pcm: &[u8]) {}
}

// ─────────────────────────── 跨域共享校验 helper ───────────────────────────

/// UUID-v4 字面校验：36 字符 + 5 段 `-` 分隔（8-4-4-4-12）+ 仅 ASCII 十六进制。
/// 用于 install/detail/like —— pack_id 来自远端服务器，必须是它发的 UUID。
pub(crate) fn is_valid_session_id(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        let is_dash_position = matches!(i, 8 | 13 | 18 | 23);
        if is_dash_position {
            if *b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// 本地 style pack id 白名单：`[A-Za-z0-9._-]`、长度 1..=128。
/// 上传走本地 id（`builtin.light` / 用户自取 slug / UUID 都可），不是远端 UUID。
/// 仍阻断 `..` / `/` / `\` / 控制字符，避免 path traversal 进临时 zip 文件名。
pub(crate) fn is_valid_local_pack_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
}

// ─── 在系统文件管理器中打开路径（三个 ASR 模块共用，cfg 分平台）───

#[cfg(target_os = "windows")]
pub(crate) fn open_path_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let operation = wide_null("open");
    let target = wide_null(&path.display().to_string());
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err(format!("ShellExecuteW failed: {}", result.0 as isize))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn open_path_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
pub(crate) fn open_path_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{is_valid_local_pack_id, is_valid_session_id};
    use crate::commands::credentials::{
        asr_configured_for_provider, llm_configured_for_provider,
        local_asr_release_plan_for_provider, release_foundry_runtime_if_inactive,
        release_sherpa_runtime_if_inactive,
    };
    use crate::commands::foundry_asr::{
        active_foundry_model_from_prefs, normalize_foundry_language_hint,
        validate_foundry_model_alias,
    };
    use crate::commands::providers::{
        active_asr_is_keyless_for_validation, asr_transcriptions_url, fetch_provider_models,
        is_gemini_base_url, models_url, parse_gemini_model_ids, parse_model_ids, ProviderConfig,
    };
    use crate::commands::settings::{
        parse_latest_beta_from_atom, persist_settings, SettingsWriter,
    };
    use crate::commands::sherpa_asr::{
        active_sherpa_model_from_prefs, normalize_sherpa_language_hint, validate_sherpa_model_alias,
    };
    use crate::persistence::CredentialsSnapshot;
    use crate::types::{
        ComboBinding, HotkeyBinding, HotkeyMode, HotkeyTrigger, ShortcutBinding, UserPreferences,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::thread;

    #[derive(Default)]
    struct FakeSettingsWriter {
        saved: Mutex<Option<UserPreferences>>,
        active_asr_provider_syncs: Mutex<Vec<String>>,
        write_settings_error: Mutex<Option<String>>,
        write_settings_errors: Mutex<Vec<Option<String>>>,
        active_asr_provider_sync_error: Mutex<Option<String>>,
        active_asr_provider_sync_errors: Mutex<Vec<Option<String>>>,
        dictation_refreshes: Mutex<u32>,
        qa_refreshes: Mutex<u32>,
        combo_refreshes: Mutex<u32>,
        translation_refreshes: Mutex<u32>,
        switch_style_refreshes: Mutex<u32>,
        open_app_refreshes: Mutex<u32>,
        coding_agent_refreshes: Mutex<u32>,
    }

    fn snapshot() -> CredentialsSnapshot {
        CredentialsSnapshot::default()
    }

    #[test]
    fn credentials_status_follows_active_asr_provider_requirements() {
        let volcengine = CredentialsSnapshot {
            volcengine_app_key: Some("app".into()),
            volcengine_access_key: Some("access".into()),
            volcengine_resource_id: Some("resource".into()),
            volcengine_auth_mode: None, // 默认 AppIdToken 模式
            ..snapshot()
        };
        assert!(asr_configured_for_provider("volcengine", &volcengine));

        // AppIdToken 模式缺 access_key → 未配置（即使 app_key / resource_id 已填）。
        let volcengine_no_access = CredentialsSnapshot {
            volcengine_app_key: Some("app".into()),
            volcengine_resource_id: Some("resource".into()),
            ..snapshot()
        };
        assert!(!asr_configured_for_provider(
            "volcengine",
            &volcengine_no_access
        ));

        // ApiKey 模式：只需独立 api_key 槽 + resource_id，无需 app_key。
        let volcengine_api_key = CredentialsSnapshot {
            volcengine_api_key: Some("key".into()),
            volcengine_resource_id: Some("resource".into()),
            volcengine_auth_mode: Some("api_key".into()),
            ..snapshot()
        };
        assert!(asr_configured_for_provider(
            "volcengine",
            &volcengine_api_key
        ));
        // ApiKey 模式缺 api_key（旧 access_key 槽有值也不满足）→ 未配置。
        let volcengine_api_key_missing = CredentialsSnapshot {
            volcengine_access_key: Some("old-access-token".into()),
            volcengine_resource_id: Some("resource".into()),
            volcengine_auth_mode: Some("api_key".into()),
            ..snapshot()
        };
        assert!(!asr_configured_for_provider(
            "volcengine",
            &volcengine_api_key_missing
        ));

        let whisper_key_only = CredentialsSnapshot {
            asr_api_key: Some("key".into()),
            ..snapshot()
        };
        assert!(!asr_configured_for_provider("whisper", &whisper_key_only));
        assert!(asr_configured_for_provider(
            crate::asr::bailian::PROVIDER_ID,
            &whisper_key_only
        ));

        let whisper_keyless_ready = CredentialsSnapshot {
            asr_endpoint: Some("https://api.openai.com/v1".into()),
            asr_model: Some("whisper-1".into()),
            ..snapshot()
        };
        assert!(asr_configured_for_provider(
            "whisper",
            &whisper_keyless_ready
        ));
        assert!(!asr_configured_for_provider(
            crate::asr::bailian::PROVIDER_ID,
            &whisper_keyless_ready
        ));

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            assert!(asr_configured_for_provider(
                crate::asr::local::PROVIDER_ID,
                &snapshot()
            ));
            assert!(asr_configured_for_provider(
                crate::asr::local::LOCAL_QWEN3_C_PROVIDER_ID,
                &snapshot()
            ));
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        assert!(!asr_configured_for_provider(
            crate::asr::local::PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert!(asr_configured_for_provider(
            crate::asr::local::LOCAL_QWEN3_MLX_PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(target_os = "windows")]
        assert!(asr_configured_for_provider(
            crate::asr::local::foundry::PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(target_os = "windows")]
        assert!(asr_configured_for_provider(
            crate::asr::local::sherpa::PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(not(target_os = "windows"))]
        assert!(!asr_configured_for_provider(
            crate::asr::local::foundry::PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(not(target_os = "windows"))]
        assert!(!asr_configured_for_provider(
            crate::asr::local::sherpa::PROVIDER_ID,
            &snapshot()
        ));
    }

    #[test]
    fn credentials_status_treats_foundry_local_asr_as_configured() {
        #[cfg(target_os = "windows")]
        {
            assert!(asr_configured_for_provider(
                crate::asr::local::foundry::PROVIDER_ID,
                &CredentialsSnapshot::default()
            ));
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(!asr_configured_for_provider(
                crate::asr::local::foundry::PROVIDER_ID,
                &CredentialsSnapshot::default()
            ));
        }
    }

    #[test]
    fn local_asr_providers_skip_external_validation() {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::PROVIDER_ID
        ));
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        assert!(!active_asr_is_keyless_for_validation(
            crate::asr::local::PROVIDER_ID
        ));
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::LOCAL_QWEN3_C_PROVIDER_ID
        ));
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::LOCAL_QWEN3_MLX_PROVIDER_ID
        ));
        #[cfg(target_os = "macos")]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::LOCAL_WHISPER_PROVIDER_ID
        ));
        #[cfg(not(target_os = "macos"))]
        assert!(!active_asr_is_keyless_for_validation(
            crate::asr::local::LOCAL_WHISPER_PROVIDER_ID
        ));
        #[cfg(target_os = "windows")]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::foundry::PROVIDER_ID
        ));
        #[cfg(target_os = "windows")]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::sherpa::PROVIDER_ID
        ));
        #[cfg(not(target_os = "windows"))]
        assert!(!active_asr_is_keyless_for_validation(
            crate::asr::local::foundry::PROVIDER_ID
        ));
        #[cfg(not(target_os = "windows"))]
        assert!(!active_asr_is_keyless_for_validation(
            crate::asr::local::sherpa::PROVIDER_ID
        ));
        assert!(!active_asr_is_keyless_for_validation("volcengine"));
        assert!(!active_asr_is_keyless_for_validation("whisper"));
    }

    #[test]
    fn provider_switch_release_plan_covers_inactive_local_runtimes() {
        let qwen = local_asr_release_plan_for_provider(crate::asr::local::PROVIDER_ID);
        assert!(!qwen.qwen);
        assert!(qwen.whisper);
        assert!(qwen.foundry);
        assert!(qwen.sherpa);

        let qwen_c =
            local_asr_release_plan_for_provider(crate::asr::local::LOCAL_QWEN3_C_PROVIDER_ID);
        assert!(!qwen_c.qwen);
        assert!(qwen_c.whisper);

        let whisper =
            local_asr_release_plan_for_provider(crate::asr::local::LOCAL_WHISPER_PROVIDER_ID);
        assert!(whisper.qwen);
        assert!(!whisper.whisper);
        assert!(whisper.foundry);
        assert!(whisper.sherpa);

        let foundry = local_asr_release_plan_for_provider(crate::asr::local::foundry::PROVIDER_ID);
        assert!(foundry.qwen);
        assert!(foundry.whisper);
        assert!(!foundry.foundry);
        assert!(foundry.sherpa);

        let sherpa = local_asr_release_plan_for_provider(crate::asr::local::sherpa::PROVIDER_ID);
        assert!(sherpa.qwen);
        assert!(sherpa.whisper);
        assert!(sherpa.foundry);
        assert!(!sherpa.sherpa);

        let cloud = local_asr_release_plan_for_provider("volcengine");
        assert!(cloud.qwen);
        assert!(cloud.whisper);
        assert!(cloud.foundry);
        assert!(cloud.sherpa);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn provider_switch_release_requests_foundry_prepare_cancel_first() {
        let runtime = std::sync::Arc::new(crate::asr::local::FoundryLocalRuntime::new());

        release_foundry_runtime_if_inactive(&runtime, true).await;

        assert!(runtime.cancel_prepare_requested_for_tests());
    }

    #[tokio::test]
    async fn provider_switch_release_requests_sherpa_prepare_cancel_first() {
        let runtime = std::sync::Arc::new(crate::asr::local::SherpaOnnxRuntime::new());

        release_sherpa_runtime_if_inactive(&runtime, true).await;

        assert!(runtime.cancel_prepare_requested_for_tests());
        let status = runtime.status_snapshot("sense-voice-small-zh").await;
        assert!(!status.runtime_ready);
    }

    #[test]
    fn foundry_language_hint_accepts_empty_and_lowercase_iso_639_1() {
        assert_eq!(normalize_foundry_language_hint("").unwrap(), "");
        assert_eq!(normalize_foundry_language_hint("   ").unwrap(), "");
        assert_eq!(normalize_foundry_language_hint("zh").unwrap(), "zh");
        assert_eq!(normalize_foundry_language_hint(" en ").unwrap(), "en");
    }

    #[test]
    fn foundry_language_hint_rejects_non_lowercase_iso_639_1() {
        assert!(normalize_foundry_language_hint("ZH").is_err());
        assert!(normalize_foundry_language_hint("zho").is_err());
        assert!(normalize_foundry_language_hint("z1").is_err());
    }

    #[test]
    fn foundry_model_alias_validation_rejects_unknown_alias() {
        assert!(
            validate_foundry_model_alias(crate::asr::local::foundry::DEFAULT_MODEL_ALIAS).is_ok()
        );
        assert!(validate_foundry_model_alias("whisper-medium").is_ok());
        assert!(validate_foundry_model_alias("whisper-large-v3-turbo").is_ok());
        assert!(validate_foundry_model_alias("whisper-large").is_err());
    }

    #[test]
    fn foundry_active_model_pref_falls_back_to_default_for_unknown_alias() {
        let prefs = UserPreferences {
            foundry_local_asr_model: "whisper-large".to_string(),
            ..Default::default()
        };

        assert_eq!(
            active_foundry_model_from_prefs(&prefs),
            crate::asr::local::foundry::DEFAULT_MODEL_ALIAS
        );
    }

    #[test]
    fn foundry_active_model_pref_preserves_large_model_aliases() {
        for alias in ["whisper-medium", "whisper-large-v3-turbo"] {
            let prefs = UserPreferences {
                foundry_local_asr_model: alias.to_string(),
                ..Default::default()
            };

            assert_eq!(active_foundry_model_from_prefs(&prefs), alias);
        }
    }

    #[test]
    fn sherpa_language_hint_accepts_empty_and_supported_lowercase_tags() {
        assert_eq!(normalize_sherpa_language_hint("").unwrap(), "");
        assert_eq!(normalize_sherpa_language_hint("   ").unwrap(), "");
        assert_eq!(normalize_sherpa_language_hint("zh").unwrap(), "zh");
        assert_eq!(normalize_sherpa_language_hint(" en ").unwrap(), "en");
        assert_eq!(normalize_sherpa_language_hint("zh-cn").unwrap(), "zh-cn");
        assert_eq!(normalize_sherpa_language_hint("yue").unwrap(), "yue");
    }

    #[test]
    fn sherpa_language_hint_normalizes_uppercase_and_rejects_digits() {
        assert_eq!(normalize_sherpa_language_hint("ZH").unwrap(), "zh");
        assert!(normalize_sherpa_language_hint("zh-1").is_err());
        assert!(normalize_sherpa_language_hint("zh_CN").is_err());
    }

    #[test]
    fn sherpa_model_alias_validation_matches_catalog() {
        assert!(
            validate_sherpa_model_alias(crate::asr::local::sherpa::DEFAULT_MODEL_ALIAS).is_ok()
        );
        assert!(validate_sherpa_model_alias("qwen3-asr-0.6b-int8").is_ok());
        assert!(
            validate_sherpa_model_alias(crate::asr::local::sherpa::DEFAULT_ONLINE_MODEL_ALIAS)
                .is_ok()
        );
        assert!(validate_sherpa_model_alias("zipformer-zh-streaming").is_err());
    }

    #[test]
    fn sherpa_active_model_pref_falls_back_to_default_for_unknown_alias() {
        let prefs = UserPreferences {
            sherpa_onnx_model: "zipformer-zh-streaming".to_string(),
            ..Default::default()
        };

        assert_eq!(
            active_sherpa_model_from_prefs(&prefs),
            crate::asr::local::sherpa::DEFAULT_MODEL_ALIAS
        );
    }

    #[test]
    fn credentials_status_accepts_keyless_custom_llm_only() {
        let keyless_ready = CredentialsSnapshot {
            ark_endpoint: Some("http://localhost:11434/v1".into()),
            ark_model_id: Some("qwen".into()),
            ..snapshot()
        };
        assert!(llm_configured_for_provider("custom", &keyless_ready));
        assert!(llm_configured_for_provider("self-hosted", &keyless_ready));
        assert!(llm_configured_for_provider(
            "openrouterFree",
            &keyless_ready
        ));

        let hosted_keyless = CredentialsSnapshot {
            ark_endpoint: Some("https://openrouter.ai/api/v1".into()),
            ark_model_id: Some("qwen/qwen3-coder:free".into()),
            ..snapshot()
        };
        assert!(!llm_configured_for_provider(
            "openrouterFree",
            &hosted_keyless
        ));

        let hosted_ready = CredentialsSnapshot {
            ark_api_key: Some("key".into()),
            ark_endpoint: Some("https://openrouter.ai/api/v1/chat/completions".into()),
            ark_model_id: Some("qwen/qwen3-coder:free".into()),
            ..snapshot()
        };
        assert!(llm_configured_for_provider("openrouterFree", &hosted_ready));

        let key_without_endpoint = CredentialsSnapshot {
            ark_api_key: Some("key".into()),
            ark_model_id: Some("qwen".into()),
            ..snapshot()
        };
        assert!(!llm_configured_for_provider(
            "custom",
            &key_without_endpoint
        ));

        let endpoint_without_model = CredentialsSnapshot {
            ark_endpoint: Some("http://localhost:11434/v1".into()),
            ..snapshot()
        };
        assert!(!llm_configured_for_provider(
            "custom",
            &endpoint_without_model
        ));
    }

    #[test]
    fn credentials_status_requires_api_key_for_atlascloud() {
        let keyless = CredentialsSnapshot {
            ark_endpoint: Some("https://api.atlascloud.ai/v1".into()),
            ark_model_id: Some("qwen/qwen3.5-flash".into()),
            ..snapshot()
        };
        assert!(!llm_configured_for_provider("atlascloud", &keyless));

        let ready = CredentialsSnapshot {
            ark_api_key: Some("key".into()),
            ..keyless
        };
        assert!(llm_configured_for_provider("atlascloud", &ready));
    }

    impl SettingsWriter for FakeSettingsWriter {
        fn read_settings(&self) -> UserPreferences {
            self.saved.lock().unwrap().clone().unwrap_or_default()
        }

        fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
            if let Some(error) = {
                let mut errors = self.write_settings_errors.lock().unwrap();
                if errors.is_empty() {
                    None
                } else {
                    errors.remove(0)
                }
            } {
                return Err(error);
            }
            if let Some(error) = self.write_settings_error.lock().unwrap().clone() {
                return Err(error);
            }
            *self.saved.lock().unwrap() = Some(prefs);
            Ok(())
        }

        fn sync_active_asr_provider(&self, provider: &str) -> Result<(), String> {
            self.active_asr_provider_syncs
                .lock()
                .unwrap()
                .push(provider.to_string());
            if let Some(error) = {
                let mut errors = self.active_asr_provider_sync_errors.lock().unwrap();
                if errors.is_empty() {
                    None
                } else {
                    errors.remove(0)
                }
            } {
                return Err(error);
            }
            if let Some(error) = self.active_asr_provider_sync_error.lock().unwrap().clone() {
                return Err(error);
            }
            Ok(())
        }

        fn refresh_dictation_hotkey(&self) {
            *self.dictation_refreshes.lock().unwrap() += 1;
        }

        fn refresh_qa_hotkey(&self) {
            *self.qa_refreshes.lock().unwrap() += 1;
        }

        fn refresh_combo_hotkey(&self) {
            *self.combo_refreshes.lock().unwrap() += 1;
        }

        fn refresh_translation_hotkey(&self) {
            *self.translation_refreshes.lock().unwrap() += 1;
        }

        fn refresh_switch_style_hotkey(&self) {
            *self.switch_style_refreshes.lock().unwrap() += 1;
        }

        fn refresh_open_app_hotkey(&self) {
            *self.open_app_refreshes.lock().unwrap() += 1;
        }

        fn refresh_selection_polish_hotkey(&self) {}

        fn refresh_coding_agent_hotkey(&self) {
            *self.coding_agent_refreshes.lock().unwrap() += 1;
        }
    }

    #[test]
    fn models_url_accepts_base_or_chat_endpoint() {
        assert_eq!(
            models_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            models_url("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn asr_transcriptions_url_accepts_base_or_transcriptions_endpoint() {
        assert_eq!(
            asr_transcriptions_url("https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/audio/transcriptions"
        );
        assert_eq!(
            asr_transcriptions_url("https://api.openai.com/v1/chat/completions").unwrap(),
            "https://api.openai.com/v1/audio/transcriptions"
        );
        assert_eq!(
            asr_transcriptions_url("https://api.openai.com/v1/audio").unwrap(),
            "https://api.openai.com/v1/audio/transcriptions"
        );
        assert_eq!(
            asr_transcriptions_url("https://api.openai.com/v1/audio/transcriptions").unwrap(),
            "https://api.openai.com/v1/audio/transcriptions"
        );
        assert_eq!(
            asr_transcriptions_url("https://api.openai.com/v1?api-version=2024-12-01").unwrap(),
            "https://api.openai.com/v1/audio/transcriptions?api-version=2024-12-01"
        );
        assert_eq!(
            asr_transcriptions_url("http://192.168.1.10:8000/v1").unwrap(),
            "http://192.168.1.10:8000/v1/audio/transcriptions"
        );
    }

    #[test]
    fn parse_model_ids_sorts_and_deduplicates() {
        let models =
            parse_model_ids(r#"{ "data": [{ "id": "b" }, { "id": "a" }, { "id": "b" }] }"#)
                .unwrap();
        assert_eq!(models, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parse_gemini_model_ids_strips_models_prefix_and_dedups() {
        // Google v1beta/models 真实响应的子集——name 字段带 `models/` 前缀，
        // ProviderTools 选中后写入 ark.model_id 时不能带这个前缀（generateContent
        // URL 拼接已经会加 `models/`，不去前缀就会变成 `models/models/...`）。
        // 字段缺失时保守保留（视为支持 generateContent）。
        let body = r#"{"models":[
            {"name":"models/gemini-2.5-pro"},
            {"name":"models/gemini-2.5-flash"},
            {"name":"models/gemini-2.5-flash"},
            {"name":"models/gemini-3-flash-preview"}
        ]}"#;
        let ids = parse_gemini_model_ids(body).unwrap();
        assert_eq!(
            ids,
            vec![
                "gemini-2.5-flash".to_string(),
                "gemini-2.5-pro".to_string(),
                "gemini-3-flash-preview".to_string(),
            ]
        );
    }

    #[test]
    fn parse_gemini_model_ids_filters_out_non_generate_content_families() {
        // 真实 Google v1beta/models 响应里同时有 generateContent / embedContent /
        // generateMessage 等多种家族。用户选中 embedding/TTS/image 模型写入
        // ark.model_id → polish 必败。这里是 PR #398 pr_agent advisory 的回归用例：
        // 只把 supportedGenerationMethods 里含 generateContent 的过滤出来。
        let body = r#"{"models":[
            {"name":"models/gemini-2.5-flash","supportedGenerationMethods":["generateContent","streamGenerateContent","countTokens"]},
            {"name":"models/gemini-embedding-2","supportedGenerationMethods":["embedContent"]},
            {"name":"models/text-embedding-004","supportedGenerationMethods":["embedContent","countTextTokens"]},
            {"name":"models/gemini-2.5-pro-preview-tts","supportedGenerationMethods":["generateContent"]},
            {"name":"models/gemini-2.5-flash-image","supportedGenerationMethods":["predict"]}
        ]}"#;
        let ids = parse_gemini_model_ids(body).unwrap();
        // 只剩两条声明 generateContent 的；embedding 与 image (predict-only) 必须被过滤。
        assert_eq!(
            ids,
            vec![
                "gemini-2.5-flash".to_string(),
                "gemini-2.5-pro-preview-tts".to_string(),
            ]
        );
    }

    #[test]
    fn is_gemini_base_url_matches_official_domain() {
        assert!(is_gemini_base_url(
            "https://generativelanguage.googleapis.com/v1beta"
        ));
        assert!(is_gemini_base_url(
            "https://generativelanguage.googleapis.com/v1beta/"
        ));
        assert!(!is_gemini_base_url("https://api.openai.com/v1"));
        assert!(!is_gemini_base_url(
            "https://ark.cn-beijing.volces.com/api/v3"
        ));
    }

    #[test]
    fn persist_settings_refreshes_changed_hotkey_pipelines() {
        let writer = FakeSettingsWriter::default();
        let previous = UserPreferences::default();
        *writer.saved.lock().unwrap() = Some(previous);
        let prefs = UserPreferences {
            dictation_hotkey: ShortcutBinding {
                primary: "D".to_string(),
                modifiers: vec!["ctrl".to_string()],
            },
            qa_hotkey: Some(ShortcutBinding {
                primary: "Q".to_string(),
                modifiers: vec!["ctrl".to_string(), "alt".to_string()],
            }),
            translation_hotkey: ShortcutBinding {
                primary: "T".to_string(),
                modifiers: vec!["ctrl".to_string(), "alt".to_string()],
            },
            switch_style_hotkey: Some(ShortcutBinding {
                primary: "S".to_string(),
                modifiers: vec!["ctrl".to_string(), "alt".to_string()],
            }),
            open_app_hotkey: Some(ShortcutBinding {
                primary: "O".to_string(),
                modifiers: vec!["ctrl".to_string(), "alt".to_string()],
            }),
            coding_agent_voice_hotkey: Some(ShortcutBinding {
                primary: "RightControl".to_string(),
                modifiers: vec![],
            }),
            // This fixture deliberately assigns Right Control to Less Computer.
            // Keep selection polish disabled so the test exercises the intended
            // independent refresh paths.
            selection_polish_hotkey: None,
            hotkey: HotkeyBinding {
                trigger: HotkeyTrigger::Custom,
                mode: HotkeyMode::Hold,
                ..Default::default()
            },
            ..Default::default()
        };

        persist_settings(&writer, prefs.clone()).unwrap();

        let saved = writer
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("settings saved");
        assert_eq!(saved.hotkey.trigger, HotkeyTrigger::Custom);
        assert_eq!(saved.hotkey.mode, prefs.hotkey.mode);
        assert_eq!(
            saved.dictation_hotkey.primary,
            prefs.dictation_hotkey.primary
        );
        assert_eq!(saved.qa_hotkey.unwrap().primary, "Q");
        assert_eq!(*writer.dictation_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.combo_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.qa_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.translation_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.switch_style_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.open_app_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.coding_agent_refreshes.lock().unwrap(), 1);
    }

    #[test]
    fn persist_settings_reconciles_less_computer_dictation_overlap_and_saves() {
        let writer = FakeSettingsWriter::default();
        let binding = ShortcutBinding {
            primary: "LeftControl".into(),
            modifiers: vec![],
        };
        let prefs = UserPreferences {
            dictation_hotkey: binding.clone(),
            coding_agent_voice_hotkey: Some(binding),
            ..Default::default()
        };

        // 兜底（#904）：冲突不再拒绝整份保存，较低优先级的 Less Computer 键被停用。
        persist_settings(&writer, prefs).unwrap();
        let saved = writer.saved.lock().unwrap().clone().expect("prefs saved");
        assert_eq!(saved.dictation_hotkey.primary, "LeftControl");
        assert!(saved.coding_agent_voice_hotkey.is_none());
    }

    #[test]
    fn persist_settings_syncs_active_asr_provider_without_hotkey_refresh() {
        let writer = FakeSettingsWriter::default();
        let previous = UserPreferences::default();
        *writer.saved.lock().unwrap() = Some(previous.clone());
        let prefs = UserPreferences {
            active_asr_provider: "whisper".to_string(),
            microphone_device_name: "External Mic".to_string(),
            hotkey: previous.hotkey,
            dictation_hotkey: previous.dictation_hotkey,
            qa_hotkey: previous.qa_hotkey,
            translation_hotkey: previous.translation_hotkey,
            switch_style_hotkey: previous.switch_style_hotkey,
            open_app_hotkey: previous.open_app_hotkey,
            ..Default::default()
        };

        persist_settings(&writer, prefs.clone()).unwrap();

        let saved = writer
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("settings saved");
        assert_eq!(saved.active_asr_provider, prefs.active_asr_provider);
        assert_eq!(saved.microphone_device_name, prefs.microphone_device_name);
        assert_eq!(
            writer.active_asr_provider_syncs.lock().unwrap().clone(),
            vec![prefs.active_asr_provider.clone()]
        );
        assert_eq!(*writer.dictation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.combo_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.qa_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.translation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.switch_style_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.open_app_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.coding_agent_refreshes.lock().unwrap(), 0);
    }

    #[test]
    fn persist_settings_does_not_save_when_active_asr_sync_fails() {
        let writer = FakeSettingsWriter::default();
        let previous = UserPreferences::default();
        *writer.saved.lock().unwrap() = Some(previous.clone());
        *writer.active_asr_provider_sync_error.lock().unwrap() = Some("sync failed".to_string());
        let prefs = UserPreferences {
            active_asr_provider: "whisper".to_string(),
            microphone_device_name: "External Mic".to_string(),
            hotkey: previous.hotkey,
            dictation_hotkey: previous.dictation_hotkey,
            qa_hotkey: previous.qa_hotkey,
            translation_hotkey: previous.translation_hotkey,
            switch_style_hotkey: previous.switch_style_hotkey,
            open_app_hotkey: previous.open_app_hotkey,
            ..Default::default()
        };

        let error = persist_settings(&writer, prefs).unwrap_err();

        assert_eq!(error, "sync failed");
        let saved = writer
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("previous settings remain saved");
        assert_eq!(saved.active_asr_provider, previous.active_asr_provider);
        assert_eq!(
            saved.microphone_device_name,
            previous.microphone_device_name
        );
        assert_eq!(
            writer.active_asr_provider_syncs.lock().unwrap().clone(),
            vec!["whisper".to_string()]
        );
        assert_eq!(*writer.dictation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.combo_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.qa_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.translation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.switch_style_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.open_app_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.coding_agent_refreshes.lock().unwrap(), 0);
    }

    #[test]
    fn persist_settings_restores_active_asr_provider_when_save_fails_after_sync() {
        let writer = FakeSettingsWriter::default();
        let previous = UserPreferences::default();
        *writer.saved.lock().unwrap() = Some(previous.clone());
        *writer.write_settings_error.lock().unwrap() = Some("save failed".to_string());
        let prefs = UserPreferences {
            active_asr_provider: "whisper".to_string(),
            microphone_device_name: "External Mic".to_string(),
            hotkey: previous.hotkey,
            dictation_hotkey: previous.dictation_hotkey,
            qa_hotkey: previous.qa_hotkey,
            translation_hotkey: previous.translation_hotkey,
            switch_style_hotkey: previous.switch_style_hotkey,
            open_app_hotkey: previous.open_app_hotkey,
            ..Default::default()
        };

        let error = persist_settings(&writer, prefs).unwrap_err();

        assert_eq!(error, "save failed");
        let saved = writer
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("previous settings remain saved");
        assert_eq!(saved.active_asr_provider, previous.active_asr_provider);
        assert_eq!(
            saved.microphone_device_name,
            previous.microphone_device_name
        );
        assert_eq!(
            writer.active_asr_provider_syncs.lock().unwrap().clone(),
            vec!["whisper".to_string(), previous.active_asr_provider.clone()]
        );
        assert_eq!(*writer.dictation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.combo_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.qa_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.translation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.switch_style_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.open_app_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.coding_agent_refreshes.lock().unwrap(), 0);
    }

    #[test]
    fn persist_settings_keeps_new_active_asr_provider_when_rollback_fails() {
        let writer = FakeSettingsWriter::default();
        let previous = UserPreferences::default();
        *writer.saved.lock().unwrap() = Some(previous.clone());
        *writer.write_settings_errors.lock().unwrap() = vec![Some("save failed".to_string()), None];
        *writer.active_asr_provider_sync_errors.lock().unwrap() =
            vec![None, Some("rollback failed".to_string())];
        let prefs = UserPreferences {
            active_asr_provider: "whisper".to_string(),
            microphone_device_name: "External Mic".to_string(),
            hotkey: previous.hotkey,
            dictation_hotkey: previous.dictation_hotkey,
            qa_hotkey: previous.qa_hotkey,
            translation_hotkey: previous.translation_hotkey,
            switch_style_hotkey: previous.switch_style_hotkey,
            open_app_hotkey: previous.open_app_hotkey,
            ..Default::default()
        };

        persist_settings(&writer, prefs.clone()).expect("settings remain consistent");

        let saved = writer
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("new settings saved");
        assert_eq!(saved.active_asr_provider, prefs.active_asr_provider);
        assert_eq!(saved.microphone_device_name, prefs.microphone_device_name);
        assert_eq!(
            writer.active_asr_provider_syncs.lock().unwrap().clone(),
            vec!["whisper".to_string(), previous.active_asr_provider.clone()]
        );
        assert_eq!(*writer.dictation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.combo_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.qa_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.translation_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.switch_style_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.open_app_refreshes.lock().unwrap(), 0);
        assert_eq!(*writer.coding_agent_refreshes.lock().unwrap(), 0);
    }

    #[test]
    fn sync_dictation_hotkey_sets_modifier_trigger_and_clears_combo() {
        let mut prefs = UserPreferences {
            hotkey: HotkeyBinding {
                trigger: HotkeyTrigger::Custom,
                mode: HotkeyMode::Toggle,
                keys: None,
            },
            custom_combo_hotkey: Some(ComboBinding {
                primary: "D".into(),
                modifiers: vec!["cmd".into(), "shift".into()],
            }),
            dictation_hotkey: ShortcutBinding {
                primary: "RightControl".into(),
                modifiers: vec![],
            },
            ..Default::default()
        };

        super::sync_dictation_hotkey_legacy_fields(&mut prefs);

        assert_eq!(prefs.hotkey.trigger, HotkeyTrigger::RightControl);
        assert!(prefs.custom_combo_hotkey.is_none());
    }

    #[test]
    fn sync_dictation_hotkey_sets_custom_trigger_and_combo_binding() {
        let mut prefs = UserPreferences {
            hotkey: HotkeyBinding {
                trigger: HotkeyTrigger::RightControl,
                mode: HotkeyMode::Toggle,
                keys: None,
            },
            dictation_hotkey: ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd".into(), "shift".into()],
            },
            ..Default::default()
        };

        super::sync_dictation_hotkey_legacy_fields(&mut prefs);

        assert_eq!(prefs.hotkey.trigger, HotkeyTrigger::Custom);
        let combo = prefs.custom_combo_hotkey.expect("combo binding saved");
        assert_eq!(combo.primary, "D");
        assert_eq!(
            combo.modifiers,
            vec!["cmd".to_string(), "shift".to_string()]
        );
    }

    #[test]
    fn sync_dictation_hotkey_clears_empty_custom_binding() {
        let mut prefs = UserPreferences {
            hotkey: HotkeyBinding {
                trigger: HotkeyTrigger::RightControl,
                mode: HotkeyMode::Toggle,
                keys: None,
            },
            custom_combo_hotkey: Some(ComboBinding {
                primary: "D".into(),
                modifiers: vec!["cmd".into(), "shift".into()],
            }),
            dictation_hotkey: ShortcutBinding {
                primary: " ".into(),
                modifiers: vec!["cmd".into()],
            },
            ..Default::default()
        };

        super::sync_dictation_hotkey_legacy_fields(&mut prefs);

        assert_eq!(prefs.hotkey.trigger, HotkeyTrigger::Custom);
        assert!(prefs.custom_combo_hotkey.is_none());
    }

    #[test]
    fn validate_combo_hotkey_rejects_bare_shift() {
        let result = super::validate_combo_hotkey(ComboBinding {
            primary: "Shift".into(),
            modifiers: vec![],
        });

        assert!(result.is_err());
    }

    #[test]
    fn combo_hotkey_bare_shift_rejection_matches_dictation_setter() {
        let binding = ShortcutBinding {
            primary: "Shift".into(),
            modifiers: vec![],
        };

        assert_eq!(
            super::reject_bare_shift_dictation_shortcut(&binding),
            Err("Shift 单键目前只能用于翻译快捷键".into())
        );
    }

    #[test]
    fn dictation_qa_overlap_rejects_same_modifier_only_binding() {
        let binding = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };

        assert_eq!(
            super::reject_dictation_qa_hotkey_overlap(&binding, &binding),
            Err("QA 快捷键不能和听写快捷键相同".into())
        );
    }

    #[test]
    fn dictation_qa_overlap_rejects_same_combo_binding() {
        let dictation = ShortcutBinding {
            primary: ";".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };
        let qa = ShortcutBinding {
            primary: ";".into(),
            modifiers: vec!["control".into(), "shift".into()],
        };

        assert_eq!(
            super::reject_dictation_qa_hotkey_overlap(&dictation, &qa),
            Err("QA 快捷键不能和听写快捷键相同".into())
        );
    }

    #[test]
    fn dictation_qa_overlap_allows_distinct_bindings() {
        let dictation = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };
        let qa = ShortcutBinding {
            primary: ";".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };

        assert!(super::reject_dictation_qa_hotkey_overlap(&dictation, &qa).is_ok());
    }

    #[test]
    fn dictation_translation_overlap_rejects_same_modifier_only_binding() {
        let binding = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };

        assert_eq!(
            super::reject_dictation_translation_hotkey_overlap(&binding, &binding),
            Err("翻译快捷键不能和听写快捷键相同".into())
        );
    }

    #[test]
    fn dictation_translation_overlap_rejects_same_combo_binding() {
        let dictation = ShortcutBinding {
            primary: "T".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };
        let translation = ShortcutBinding {
            primary: "T".into(),
            modifiers: vec!["control".into(), "shift".into()],
        };

        assert_eq!(
            super::reject_dictation_translation_hotkey_overlap(&dictation, &translation),
            Err("翻译快捷键不能和听写快捷键相同".into())
        );
    }

    #[test]
    fn dictation_translation_overlap_allows_distinct_bindings() {
        let dictation = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };
        let translation = ShortcutBinding {
            primary: "Shift".into(),
            modifiers: vec![],
        };

        assert!(
            super::reject_dictation_translation_hotkey_overlap(&dictation, &translation).is_ok()
        );
    }

    #[test]
    fn persist_settings_reconciles_dictation_translation_overlap_and_saves() {
        let writer = FakeSettingsWriter::default();
        let binding = ShortcutBinding {
            primary: "RightControl".into(),
            modifiers: vec![],
        };
        let prefs = UserPreferences {
            dictation_hotkey: binding.clone(),
            translation_hotkey: binding,
            ..Default::default()
        };

        // 兜底（#904）：冲突不再拒绝整份保存，翻译键恢复为旧默认 Shift。
        persist_settings(&writer, prefs).unwrap();
        let saved = writer.saved.lock().unwrap().clone().expect("prefs saved");
        assert_eq!(saved.dictation_hotkey.primary, "RightControl");
        assert_eq!(saved.translation_hotkey.primary, "Shift");
    }

    #[test]
    fn persist_settings_reconciles_translation_switch_style_overlap_and_saves() {
        let writer = FakeSettingsWriter::default();
        let binding = ShortcutBinding {
            primary: "T".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let prefs = UserPreferences {
            translation_hotkey: binding.clone(),
            switch_style_hotkey: Some(binding),
            ..Default::default()
        };

        // 兜底（#904）：冲突不再拒绝整份保存，切换风格键恢复为旧默认。
        persist_settings(&writer, prefs).unwrap();
        let saved = writer.saved.lock().unwrap().clone().expect("prefs saved");
        let defaults = UserPreferences::default();
        assert_eq!(saved.translation_hotkey.primary, "T");
        assert_eq!(saved.switch_style_hotkey, defaults.switch_style_hotkey);
    }

    #[test]
    fn persist_settings_reconciles_switch_style_open_app_overlap_and_saves() {
        let writer = FakeSettingsWriter::default();
        let binding = ShortcutBinding {
            primary: "K".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let prefs = UserPreferences {
            switch_style_hotkey: Some(binding.clone()),
            open_app_hotkey: Some(binding.clone()),
            ..Default::default()
        };

        // 兜底（#904）：冲突不再拒绝整份保存，打开应用键恢复为旧默认。
        persist_settings(&writer, prefs).unwrap();
        let saved = writer.saved.lock().unwrap().clone().expect("prefs saved");
        let defaults = UserPreferences::default();
        assert_eq!(saved.switch_style_hotkey, Some(binding));
        assert_eq!(saved.open_app_hotkey, defaults.open_app_hotkey);
    }

    #[test]
    fn parse_latest_beta_from_atom_picks_first_beta_tagged_entry() {
        // Fixture trimmed from real `releases.atom`：包含一条 stable + 一条 Beta。
        // 解析必须跳过 stable（tag 不以 -beta-tauri 结尾），返回 Beta。
        let body = r#"<?xml version="1.0"?>
<feed>
  <entry>
    <id>tag:github.com,2008:Repository/X/v1.2.23-tauri</id>
    <updated>2026-05-07T09:05:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v1.2.23-tauri"/>
    <title>OpenLess v1.2.23-tauri</title>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/X/v1.2.24-2-beta-tauri</id>
    <updated>2026-05-08T01:27:23Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v1.2.24-2-beta-tauri"/>
    <title>OpenLess v1.2.24-2-beta-tauri</title>
  </entry>
</feed>"#;
        let got = parse_latest_beta_from_atom(body).expect("must find a Beta entry");
        assert_eq!(got.tag_name, "v1.2.24-2-beta-tauri");
        assert_eq!(
            got.html_url,
            "https://github.com/Open-Less/openless/releases/tag/v1.2.24-2-beta-tauri"
        );
        assert_eq!(got.published_at, "2026-05-08T01:27:23Z");
    }

    #[test]
    fn parse_latest_beta_from_atom_prefers_modern_beta_tag_over_legacy_beta() {
        let body = r#"<?xml version="1.0"?>
<feed>
  <entry>
    <updated>2026-07-15T08:00:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-tauri"/>
  </entry>
  <entry>
    <updated>2026-07-15T07:00:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.1-tauri"/>
  </entry>
  <entry>
    <updated>2026-06-17T15:41:46Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v1.3.10-4-beta-tauri"/>
  </entry>
</feed>"#;

        let got = parse_latest_beta_from_atom(body).expect("must find the newest Beta entry");

        assert_eq!(got.tag_name, "v1.3.15-Beta.1-tauri");
        assert_eq!(got.published_at, "2026-07-15T07:00:00Z");
    }

    #[test]
    fn parse_latest_beta_from_atom_skips_malformed_modern_tags() {
        let body = r#"<feed>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/garbage-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/1.3.15-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15.0-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1..15-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.x-Beta.1-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.x-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.1-extra-tauri"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.1-tauri-extra"/></entry>
  <entry><link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.2-Beta.1-tauri"/></entry>
  <entry>
    <updated>2026-07-15T07:00:00Z</updated>
    <link href="https://github.com/Open-Less/openless/releases/tag/v1.3.15-Beta.1-tauri"/>
  </entry>
</feed>"#;

        let got = parse_latest_beta_from_atom(body).expect("must skip malformed Beta tags");

        assert_eq!(got.tag_name, "v1.3.15-Beta.1-tauri");
        assert_eq!(got.published_at, "2026-07-15T07:00:00Z");
    }

    #[test]
    fn parse_latest_beta_from_atom_returns_none_when_only_stable_releases() {
        let body = r#"<feed>
  <entry>
    <link rel="alternate" type="text/html" href="https://github.com/Open-Less/openless/releases/tag/v1.2.23-tauri"/>
    <updated>2026-05-07T09:05:00Z</updated>
  </entry>
</feed>"#;
        assert!(parse_latest_beta_from_atom(body).is_none());
    }

    #[tokio::test]
    async fn fetch_provider_models_omits_authorization_when_api_key_is_empty() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let mut request = Vec::new();
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(!request_text.contains("Authorization: Bearer"));

            let body = r#"{"data":[{"id":"m1"},{"id":"m2"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let models = fetch_provider_models(&ProviderConfig {
            base_url: format!("http://{}", addr),
            api_key: String::new(),
            extra_headers: Default::default(),
            temperature: None,
        })
        .await
        .unwrap();

        assert_eq!(models, vec!["m1".to_string(), "m2".to_string()]);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn fetch_provider_models_sends_extra_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let mut request = Vec::new();
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text
                .to_ascii_lowercase()
                .contains("x-openless-test-token: secret"));
            assert!(!request_text.contains("Authorization: Bearer"));

            let body = r#"{"data":[{"id":"m1"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let models = fetch_provider_models(&ProviderConfig {
            base_url: format!("http://{}", addr),
            api_key: String::new(),
            extra_headers: [("x-openless-test-token".to_string(), "secret".to_string())]
                .into_iter()
                .collect(),
            temperature: None,
        })
        .await
        .unwrap();

        assert_eq!(models, vec!["m1".to_string()]);
        server.join().unwrap();
    }

    #[test]
    fn is_valid_session_id_accepts_canonical_uuid_v4() {
        // canonical UUID-v4 字面：8-4-4-4-12，全小写、全大写、混合都接受。
        assert!(is_valid_session_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_session_id("550E8400-E29B-41D4-A716-446655440000"));
        assert!(is_valid_session_id("Abc12345-6789-abcd-EF01-234567890abc"));
    }

    #[test]
    fn is_valid_session_id_rejects_path_traversal_and_garbage() {
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("../../etc/passwd"));
        assert!(!is_valid_session_id("..\\..\\windows\\system32"));
        // 长度对但含 `/`：dash 位置错或非 hex 字符都不通过
        assert!(!is_valid_session_id("550e8400-e29b-41d4-a716-44665544/000"));
        assert!(!is_valid_session_id("550e8400_e29b_41d4_a716_446655440000")); // 用 _ 代 -
                                                                               // 非 hex 字符
        assert!(!is_valid_session_id("550e8400-e29b-41d4-a716-44665544000g"));
        // 长度不对（35 / 37）
        assert!(!is_valid_session_id("550e8400-e29b-41d4-a716-44665544000"));
        assert!(!is_valid_session_id(
            "550e8400-e29b-41d4-a716-4466554400000"
        ));
        // NUL 字节
        assert!(!is_valid_session_id(
            "550e8400-e29b-41d4-a716-44665544\x00000"
        ));
        // 百分号编码与绝对路径
        assert!(!is_valid_session_id("%2e%2e/recordings/x"));
        assert!(!is_valid_session_id("/Users/attacker/secret.wav"));
    }

    #[test]
    fn is_valid_local_pack_id_accepts_realistic_ids() {
        assert!(is_valid_local_pack_id("builtin.light"));
        assert!(is_valid_local_pack_id("builtin.structured"));
        assert!(is_valid_local_pack_id("custom.meeting"));
        assert!(is_valid_local_pack_id(
            "550e8400-e29b-41d4-a716-446655440000"
        ));
        assert!(is_valid_local_pack_id("my_pack_v2"));
        assert!(is_valid_local_pack_id("Pack-2026.05"));
    }

    #[test]
    fn is_valid_local_pack_id_rejects_path_traversal() {
        assert!(!is_valid_local_pack_id(""));
        assert!(!is_valid_local_pack_id("../etc/passwd"));
        assert!(!is_valid_local_pack_id("..\\windows\\system32"));
        assert!(!is_valid_local_pack_id("pack/../../etc"));
        assert!(!is_valid_local_pack_id("/abs/path"));
        assert!(!is_valid_local_pack_id("with space"));
        assert!(!is_valid_local_pack_id("with\x00null"));
        assert!(!is_valid_local_pack_id(&"a".repeat(129)));
    }
}
