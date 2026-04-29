//! Tauri command surface — every IPC entry the React UI invokes lives here.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::coordinator::Coordinator;
use crate::permissions::{self, PermissionStatus};
use crate::persistence::{CredentialAccount, CredentialsSnapshot, CredentialsVault};
use crate::types::{
    CredentialsStatus, DictationSession, DictionaryEntry, HotkeyCapability, HotkeyStatus,
    PolishMode, UserPreferences,
};

type CoordinatorState<'a> = State<'a, Arc<Coordinator>>;

// ─────────────────────────── settings + credentials ───────────────────────────

#[tauri::command]
pub fn get_settings(coord: CoordinatorState<'_>) -> UserPreferences {
    coord.prefs().get()
}

#[tauri::command]
pub fn set_settings(coord: CoordinatorState<'_>, prefs: UserPreferences) -> Result<(), String> {
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_hotkey_binding();
    Ok(())
}

#[tauri::command]
pub fn get_hotkey_status(coord: CoordinatorState<'_>) -> HotkeyStatus {
    coord.hotkey_status()
}

#[tauri::command]
pub fn get_hotkey_capability(coord: CoordinatorState<'_>) -> HotkeyCapability {
    coord.hotkey_capability()
}

#[tauri::command]
pub fn get_credentials() -> CredentialsStatus {
    let snap = CredentialsVault::snapshot();
    CredentialsStatus {
        volcengine_configured: configured(&snap.volcengine_app_key)
            && configured(&snap.volcengine_access_key)
            && configured(&snap.volcengine_resource_id),
        ark_configured: configured(&snap.ark_api_key),
    }
}

fn configured(field: &Option<String>) -> bool {
    field.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
}

#[tauri::command]
pub fn set_credential(account: String, value: String) -> Result<(), String> {
    let acc = parse_account(&account)?;
    if value.is_empty() {
        CredentialsVault::remove(acc).map_err(|e| e.to_string())
    } else {
        CredentialsVault::set(acc, &value).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn set_active_llm_provider(provider: String) -> Result<(), String> {
    CredentialsVault::set_active_llm_provider(&provider).map_err(|e| e.to_string())
}

/// 读出某个账号的实际值（用于设置页预填表单）。
/// 与 Swift `CredentialsVault.get` 同语义，先 Keychain，缺则回落 ~/.openless/credentials.json。
#[tauri::command]
pub fn read_credential(account: String) -> Result<Option<String>, String> {
    let acc = parse_account(&account)?;
    CredentialsVault::get(acc).map_err(|e| e.to_string())
}

fn parse_account(s: &str) -> Result<CredentialAccount, String> {
    match s {
        "volcengine.app_key" => Ok(CredentialAccount::VolcengineAppKey),
        "volcengine.access_key" => Ok(CredentialAccount::VolcengineAccessKey),
        "volcengine.resource_id" => Ok(CredentialAccount::VolcengineResourceId),
        "ark.api_key" => Ok(CredentialAccount::ArkApiKey),
        "ark.model_id" => Ok(CredentialAccount::ArkModelId),
        "ark.endpoint" => Ok(CredentialAccount::ArkEndpoint),
        _ => Err(format!("unknown account: {s}")),
    }
}

// ─────────────────────────── history ───────────────────────────

#[tauri::command]
pub fn list_history(coord: CoordinatorState<'_>) -> Result<Vec<DictationSession>, String> {
    coord.history().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_history_entry(coord: CoordinatorState<'_>, id: String) -> Result<(), String> {
    coord.history().delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.history().clear().map_err(|e| e.to_string())
}

// ─────────────────────────── vocab ───────────────────────────

#[tauri::command]
pub fn list_vocab(coord: CoordinatorState<'_>) -> Result<Vec<DictionaryEntry>, String> {
    coord.vocab().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_vocab(
    coord: CoordinatorState<'_>,
    phrase: String,
    note: Option<String>,
) -> Result<DictionaryEntry, String> {
    coord.vocab().add(phrase, note).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_vocab(coord: CoordinatorState<'_>, id: String) -> Result<(), String> {
    coord.vocab().remove(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_vocab_enabled(
    coord: CoordinatorState<'_>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    coord
        .vocab()
        .set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

// ─────────────────────────── dictation lifecycle ───────────────────────────

#[tauri::command]
pub async fn start_dictation(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.start_dictation().await
}

#[tauri::command]
pub async fn stop_dictation(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.stop_dictation().await
}

#[tauri::command]
pub fn cancel_dictation(coord: CoordinatorState<'_>) {
    coord.cancel_dictation();
}

#[tauri::command]
pub async fn repolish(
    coord: CoordinatorState<'_>,
    raw_text: String,
    mode: PolishMode,
) -> Result<String, String> {
    coord.repolish(raw_text, mode).await
}

// ─────────────────────────── style toggles (lightweight) ───────────────────────────

#[tauri::command]
pub fn set_default_polish_mode(
    coord: CoordinatorState<'_>,
    mode: PolishMode,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    prefs.default_mode = mode;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_style_enabled(
    coord: CoordinatorState<'_>,
    mode: PolishMode,
    enabled: bool,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    if enabled {
        if !prefs.enabled_modes.contains(&mode) {
            prefs.enabled_modes.push(mode);
        }
    } else {
        prefs.enabled_modes.retain(|m| *m != mode);
    }
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

// ─────────────────────────── 系统权限 ───────────────────────────

#[tauri::command]
pub fn check_accessibility_permission() -> PermissionStatus {
    permissions::check_accessibility()
}

#[tauri::command]
pub fn request_accessibility_permission() -> PermissionStatus {
    permissions::request_accessibility()
}

#[tauri::command]
pub fn check_microphone_permission() -> PermissionStatus {
    permissions::check_microphone()
}

#[tauri::command]
pub fn request_microphone_permission(app: AppHandle) -> PermissionStatus {
    crate::request_microphone_from_foreground(&app)
}

/// 跳到 macOS 系统设置的指定隐私面板。pane: "accessibility" | "microphone".
#[tauri::command]
pub fn open_system_settings(pane: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match pane.as_str() {
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            "microphone" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy",
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let uri = match pane.as_str() {
            "microphone" => "ms-settings:privacy-microphone",
            "sound" => "ms-settings:sound",
            "accessibility" => "ms-settings:easeofaccess",
            _ => "ms-settings:",
        };
        std::process::Command::new("cmd")
            .args(["/C", "start", "", uri])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// 触发 macOS 系统弹"是否允许 OpenLess 访问麦克风"对话框。
/// 与 Swift `MicrophonePermission.request()` 同语义：只信系统权限回调，
/// 不用 cpal stream 成功与否伪造授权状态。
#[tauri::command]
pub fn trigger_microphone_prompt(app: AppHandle) -> Result<(), String> {
    let status = crate::request_microphone_from_foreground(&app);
    if matches!(
        status,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        Ok(())
    } else {
        Err(format!("microphone permission is {status:?}"))
    }
}

// ─────────────────────────── unused but exported (silences dead_code) ───────────────────────────

#[allow(dead_code)]
fn _ensure_snapshot_used(_: CredentialsSnapshot) {}
