//! Tauri command surface — every IPC entry the React UI invokes lives here.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::asr::local::foundry::{
    model_alias_is_known, FoundryCatalogModel, FoundryPrepareProgressPayload, FoundryRuntimeStatus,
    DEFAULT_MODEL_ALIAS, PROVIDER_ID as FOUNDRY_LOCAL_PROVIDER_ID,
};
use crate::asr::local::FoundryLocalRuntime;
use crate::coordinator::Coordinator;
use crate::permissions::{self, PermissionStatus};
use crate::persistence::{CredentialAccount, CredentialsSnapshot, CredentialsVault};
use crate::polish::{LLMError, OpenAICompatibleConfig, OpenAICompatibleLLMProvider};
use crate::recorder::{AudioConsumer, Recorder};
use crate::types::{
    ChineseScriptPreference, ComboBinding, CredentialsStatus, DictationSession, DictionaryEntry,
    HotkeyCapability, HotkeyStatus, OutputLanguagePreference, PolishMode, ShortcutBinding,
    UpdateChannel, UserPreferences, VocabPresetStore, WindowsImeStatus,
};

type CoordinatorState<'a> = State<'a, Arc<Coordinator>>;
pub type MicrophoneMonitorState = Mutex<Option<Recorder>>;
pub type TrayMicrophoneMenuState = Mutex<Vec<TrayMicrophoneMenuItem>>;

pub struct TrayMicrophoneMenuItem {
    pub id: String,
    pub device_name: String,
    pub item: tauri::menu::CheckMenuItem<tauri::Wry>,
}

pub fn sync_tray_microphone_selection(items: &[TrayMicrophoneMenuItem], device_name: &str) {
    for item in items {
        let _ = item.item.set_checked(item.device_name == device_name);
    }
}

struct LevelProbeConsumer;

impl AudioConsumer for LevelProbeConsumer {
    fn consume_pcm_chunk(&self, _pcm: &[u8]) {}
}

// ─────────────────────────── settings + credentials ───────────────────────────

#[tauri::command]
pub fn get_settings(coord: CoordinatorState<'_>) -> UserPreferences {
    coord.prefs().get()
}

trait SettingsWriter {
    fn write_settings(&self, prefs: UserPreferences) -> Result<(), String>;
    fn refresh_dictation_hotkey(&self);
    fn refresh_qa_hotkey(&self);
    fn refresh_combo_hotkey(&self);
    fn refresh_translation_hotkey(&self);
    fn refresh_switch_style_hotkey(&self);
    fn refresh_open_app_hotkey(&self);
}

impl SettingsWriter for Coordinator {
    fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
        self.prefs().set(prefs).map_err(|e| e.to_string())
    }

    fn refresh_dictation_hotkey(&self) {
        self.update_hotkey_binding();
    }

    fn refresh_qa_hotkey(&self) {
        self.update_qa_hotkey_binding();
    }

    fn refresh_combo_hotkey(&self) {
        self.update_combo_hotkey_binding();
    }

    fn refresh_translation_hotkey(&self) {
        self.update_translation_hotkey_binding();
    }

    fn refresh_switch_style_hotkey(&self) {
        self.update_switch_style_hotkey_binding();
    }

    fn refresh_open_app_hotkey(&self) {
        self.update_open_app_hotkey_binding();
    }
}

impl<T: SettingsWriter + ?Sized> SettingsWriter for Arc<T> {
    fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
        (**self).write_settings(prefs)
    }

    fn refresh_dictation_hotkey(&self) {
        (**self).refresh_dictation_hotkey();
    }

    fn refresh_qa_hotkey(&self) {
        (**self).refresh_qa_hotkey();
    }

    fn refresh_combo_hotkey(&self) {
        (**self).refresh_combo_hotkey();
    }

    fn refresh_translation_hotkey(&self) {
        (**self).refresh_translation_hotkey();
    }

    fn refresh_switch_style_hotkey(&self) {
        (**self).refresh_switch_style_hotkey();
    }

    fn refresh_open_app_hotkey(&self) {
        (**self).refresh_open_app_hotkey();
    }
}

fn persist_settings<T: SettingsWriter>(
    coord: &T,
    mut prefs: UserPreferences,
) -> Result<(), String> {
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    reject_hotkey_collisions(&prefs)?;
    coord.write_settings(prefs)?;
    coord.refresh_dictation_hotkey();
    coord.refresh_qa_hotkey();
    coord.refresh_combo_hotkey();
    coord.refresh_translation_hotkey();
    coord.refresh_switch_style_hotkey();
    coord.refresh_open_app_hotkey();
    Ok(())
}

#[tauri::command]
pub fn set_settings(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    tray_microphones: State<'_, TrayMicrophoneMenuState>,
    prefs: UserPreferences,
) -> Result<(), String> {
    // 广播给所有 webview。issue #205：QaPanel 跑在独立 webview，
    // 没有 HotkeySettingsContext，必须靠事件感知录音键变化，否则面板可见时
    // 用户改键会让浮窗里的 "{recordHotkey}" 文案一直停留在旧值。
    persist_settings(&*coord, prefs.clone())?;
    if let Err(err) = crate::refresh_tray_microphone_menu(&app) {
        log::warn!("[tray] refresh microphone menu after settings save failed: {err}");
        sync_tray_microphone_selection(&tray_microphones.lock(), &prefs.microphone_device_name);
    }
    let _ = app.emit("prefs:changed", &prefs);
    Ok(())
}

// ─────────────────────────── release channel (Beta opt-in) ───────────────────────────
//
// 渠道偏好的写入路径跟 set_settings 复用 persist_settings：保持热键兜底归一化
// 跟其他 prefs 写入一致，且写完后 emit "prefs:changed"，让前端跨 webview 同步。
//
// 注意：plugin-updater 2.10 的 Builder 不暴露 endpoints() 运行时 API，因此切到 Beta
// 渠道**不会**改变 in-app「检查更新」的行为——它仍然只看正式版 manifest。Beta 用户
// 通过 `fetch_latest_beta_release` 获取最新 prerelease，由前端跳浏览器手动下载，
// 物理隔离 Beta 包不会通过 auto-update 推到正式版用户。详见 PR-B-2 description 与
// CLAUDE.md `Branch & release-channel workflow` 段落。

#[tauri::command]
pub fn get_update_channel(coord: CoordinatorState<'_>) -> UpdateChannel {
    coord.prefs().get().update_channel
}

#[tauri::command]
pub fn set_update_channel(
    coord: CoordinatorState<'_>,
    app: AppHandle,
    channel: UpdateChannel,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    if prefs.update_channel == channel {
        return Ok(());
    }
    prefs.update_channel = channel;
    persist_settings(&*coord, prefs.clone())?;
    let _ = app.emit("prefs:changed", &prefs);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestBetaRelease {
    pub tag_name: String,
    pub html_url: String,
    pub published_at: String,
}

/// 调 GitHub Releases API 拿最近 20 条 release，找出第一条 `prerelease=true` 且
/// tag 以 `-beta-tauri` 结尾的。返回 `Ok(None)` 表示当前没有发布过 Beta 版。
/// 网络/解析错误以 `Err(String)` 上报，让前端展示具体原因。
#[tauri::command]
pub async fn fetch_latest_beta_release() -> Result<Option<LatestBetaRelease>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("OpenLess/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;
    let resp = client
        .get("https://api.github.com/repos/appergb/openless/releases?per_page=20")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("fetch releases: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API status {}", resp.status()));
    }
    let releases: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("parse releases json: {e}"))?;
    let latest = releases.into_iter().find(|r| {
        let is_pre = r
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let tag_ok = r
            .get("tag_name")
            .and_then(|v| v.as_str())
            .map(|s| s.ends_with("-beta-tauri"))
            .unwrap_or(false);
        is_pre && tag_ok
    });
    Ok(latest.map(|r| LatestBetaRelease {
        tag_name: r
            .get("tag_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        html_url: r
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        published_at: r
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    }))
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
pub fn set_shortcut_recording_active(coord: CoordinatorState<'_>, active: bool) {
    coord.set_shortcut_recording_active(active);
}

#[tauri::command]
pub fn get_windows_ime_status() -> WindowsImeStatus {
    crate::windows_ime_profile::get_windows_ime_status()
}

#[tauri::command]
pub fn list_microphone_devices() -> Result<Vec<crate::recorder::MicrophoneDevice>, String> {
    crate::recorder::list_input_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_microphone_level_monitor(
    app: AppHandle,
    device_name: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MicrophoneMonitorState>();
        if let Some(existing) = state.lock().take() {
            existing.stop();
        }

        let selected = device_name.trim().to_string();
        let microphone_device_name = if selected.is_empty() {
            None
        } else {
            Some(selected)
        };
        let consumer: Arc<dyn AudioConsumer> = Arc::new(LevelProbeConsumer);
        let level_app = app.clone();
        let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
            let _ = level_app.emit("microphone:level", serde_json::json!({ "level": level }));
        });
        let (recorder, _runtime_errors) =
            Recorder::start(microphone_device_name, consumer, level_handler)
                .map_err(|e| e.to_string())?;
        *state.lock() = Some(recorder);
        Ok(())
    })
    .await
    .map_err(|e| format!("start microphone monitor task failed: {e}"))?
}

#[tauri::command]
pub async fn stop_microphone_level_monitor(app: AppHandle) {
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MicrophoneMonitorState>();
        let recorder = state.lock().take();
        if let Some(recorder) = recorder {
            recorder.stop();
        }
    })
    .await;
}

#[tauri::command]
pub fn get_credentials() -> CredentialsStatus {
    let snap = CredentialsVault::snapshot();
    let active_asr_provider = CredentialsVault::get_active_asr();
    let active_llm_provider = CredentialsVault::get_active_llm();
    let volcengine_configured = volcengine_configured(&snap);
    let asr_configured = asr_configured_for_provider(&active_asr_provider, &snap);
    let llm_configured = llm_configured_for_provider(&active_llm_provider, &snap);
    CredentialsStatus {
        active_asr_provider,
        active_llm_provider,
        asr_configured,
        llm_configured,
        volcengine_configured,
        ark_configured: llm_configured,
    }
}

fn volcengine_configured(snap: &CredentialsSnapshot) -> bool {
    configured(&snap.volcengine_app_key)
        && configured(&snap.volcengine_access_key)
        && configured(&snap.volcengine_resource_id)
}

fn asr_configured_for_provider(provider: &str, snap: &CredentialsSnapshot) -> bool {
    if provider == "volcengine" {
        return volcengine_configured(snap);
    }
    if provider == crate::asr::local::PROVIDER_ID || active_foundry_asr_is_supported(provider) {
        // 本地 ASR 不依赖云端凭据。
        return true;
    }
    configured(&snap.asr_endpoint) && configured(&snap.asr_model)
}

fn llm_configured_for_provider(provider: &str, snap: &CredentialsSnapshot) -> bool {
    let endpoint = snap.ark_endpoint.as_deref().unwrap_or_default();
    let endpoint_and_model = configured(&snap.ark_endpoint) && configured(&snap.ark_model_id);
    if endpoint_and_model
        && llm_provider_default_endpoint(provider)
            .map(|default| same_llm_endpoint(endpoint, default))
            .unwrap_or(false)
    {
        return configured(&snap.ark_api_key);
    }
    endpoint_and_model
}

fn llm_provider_default_endpoint(provider: &str) -> Option<&'static str> {
    match provider {
        "ark" => Some("https://ark.cn-beijing.volces.com/api/v3"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "siliconflow" => Some("https://api.siliconflow.cn/v1"),
        "openai" => Some("https://api.openai.com/v1"),
        "mimo" => Some("https://api.xiaomimimo.com/v1"),
        "cometapi" => Some("https://api.cometapi.com/v1"),
        "openrouterFree" => Some("https://openrouter.ai/api/v1"),
        "alibabaCoding" => Some("https://coding-intl.dashscope.aliyuncs.com/v1"),
        "codingPlanX" => Some("https://api.codingplanx.ai/v1"),
        _ => None,
    }
}

fn same_llm_endpoint(a: &str, b: &str) -> bool {
    fn normalize(value: &str) -> &str {
        value
            .trim()
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions")
            .trim_end_matches('/')
    }
    normalize(a).eq_ignore_ascii_case(normalize(b))
}

fn configured(field: &Option<String>) -> bool {
    field
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalAsrReleasePlan {
    qwen: bool,
    foundry: bool,
}

fn local_asr_release_plan_for_provider(provider: &str) -> LocalAsrReleasePlan {
    LocalAsrReleasePlan {
        qwen: provider != crate::asr::local::PROVIDER_ID,
        foundry: provider != FOUNDRY_LOCAL_PROVIDER_ID,
    }
}

async fn release_foundry_runtime_if_inactive(
    runtime: &Arc<FoundryLocalRuntime>,
    release_foundry: bool,
) {
    if release_foundry {
        runtime.request_cancel_prepare();
        if let Err(error) = runtime.release_now().await {
            log::warn!("[foundry-asr] release inactive runtime failed: {error:#}");
        }
    }
}

#[tauri::command]
pub fn set_credential(window: Window, account: String, value: String) -> Result<(), String> {
    ensure_main_window(&window)?;
    let acc = parse_account(&account)?;
    if value.is_empty() {
        CredentialsVault::remove(acc).map_err(|e| e.to_string())
    } else {
        CredentialsVault::set(acc, &value).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn set_active_asr_provider(
    coord: CoordinatorState<'_>,
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
    provider: String,
) -> Result<(), String> {
    if provider == FOUNDRY_LOCAL_PROVIDER_ID && !active_foundry_asr_is_supported(&provider) {
        return Err("Foundry Local Whisper is only available on Windows".to_string());
    }
    CredentialsVault::set_active_asr_provider(&provider).map_err(|e| e.to_string())?;
    let release_plan = local_asr_release_plan_for_provider(&provider);
    if provider == crate::asr::local::PROVIDER_ID {
        // 切到本地 ASR → 后台预加载模型，下次按 hotkey 时不必等数秒。
        coord.preload_local_asr_in_background();
    }
    if release_plan.qwen {
        // 切回云端 → 用户已不需要本地引擎，立刻释放 1.2GB+ RAM；不释放的话只会等到
        // schedule_local_asr_release 的下一次 dictation 才触发，而切回云端后根本不会
        // 再走 local 路径，引擎会驻留到进程退出。
        coord.release_local_asr_engine();
    }
    release_foundry_runtime_if_inactive(runtime.inner(), release_plan.foundry).await;
    Ok(())
}

#[tauri::command]
pub fn set_active_llm_provider(provider: String) -> Result<(), String> {
    CredentialsVault::set_active_llm_provider(&provider).map_err(|e| e.to_string())
}

/// 读出某个账号的实际值（用于设置页预填表单）。
/// 凭据来自系统凭据库；只允许主设置窗口读取 raw secret，避免胶囊 / QA 等辅助窗口默认暴露。
#[tauri::command]
pub fn read_credential(window: Window, account: String) -> Result<Option<String>, String> {
    ensure_main_window(&window)?;
    let acc = parse_account(&account)?;
    CredentialsVault::get(acc).map_err(|e| e.to_string())
}

fn ensure_main_window(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("credential access is only allowed from the main window".to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCheckResult {
    ok: bool,
}

#[derive(Serialize)]
pub struct ProviderModelsResult {
    models: Vec<String>,
}

#[tauri::command]
pub async fn validate_provider_credentials(kind: String) -> Result<ProviderCheckResult, String> {
    match kind.as_str() {
        "llm" => validate_llm_provider()
            .await
            .map(|()| ProviderCheckResult { ok: true }),
        "asr" => validate_asr_provider()
            .await
            .map(|()| ProviderCheckResult { ok: true }),
        _ => Err(format!("unknown provider kind: {kind}")),
    }
}

#[tauri::command]
pub async fn list_provider_models(kind: String) -> Result<ProviderModelsResult, String> {
    let config = read_openai_provider_config(&kind)?;
    fetch_provider_models(&config)
        .await
        .map(|models| ProviderModelsResult { models })
}

struct ProviderConfig {
    base_url: String,
    api_key: String,
}

fn read_openai_provider_config(kind: &str) -> Result<ProviderConfig, String> {
    let (api_key_account, endpoint_account, api_key_required) = match kind {
        "llm" => (
            CredentialAccount::ArkApiKey,
            CredentialAccount::ArkEndpoint,
            false,
        ),
        "asr" => (
            CredentialAccount::AsrApiKey,
            CredentialAccount::AsrEndpoint,
            true,
        ),
        _ => return Err(format!("unknown provider kind: {kind}")),
    };
    let api_key = CredentialsVault::get(api_key_account)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let base_url = CredentialsVault::get(endpoint_account)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if api_key_required && api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("Endpoint 为空".to_string());
    }
    Ok(ProviderConfig { base_url, api_key })
}

async fn validate_llm_provider() -> Result<(), String> {
    let config = read_openai_provider_config("llm")?;
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "llmModelMissing".to_string())?;
    let provider = OpenAICompatibleLLMProvider::new(OpenAICompatibleConfig::new(
        "ark",
        "Doubao Ark",
        config.base_url,
        config.api_key,
        model,
    ));
    provider
        .polish(
            "验证连接",
            PolishMode::Raw,
            &[],
            &[],
            ChineseScriptPreference::Auto,
            OutputLanguagePreference::Auto,
            None,
        )
        .await
        .map(|_| ())
        .map_err(|e| match e {
            LLMError::InvalidResponse { status, .. } => {
                format!("providerHttpStatus:{status}")
            }
            other => other.to_string(),
        })
}

async fn validate_asr_provider() -> Result<(), String> {
    let active_asr = CredentialsVault::get_active_asr();
    if active_asr_is_keyless_for_validation(&active_asr) {
        return Ok(());
    }

    let config = read_openai_provider_config("asr")?;
    let model = CredentialsVault::get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "asrModelMissing".to_string())?;
    validate_asr_transcription(&config, model.trim()).await
}

fn active_asr_is_keyless_for_validation(provider: &str) -> bool {
    provider == crate::asr::local::PROVIDER_ID || active_foundry_asr_is_supported(provider)
}

fn active_foundry_asr_is_supported(provider: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        provider == FOUNDRY_LOCAL_PROVIDER_ID
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = provider;
        false
    }
}

async fn validate_asr_transcription(config: &ProviderConfig, model: &str) -> Result<(), String> {
    const MAX_ASR_VALIDATE_BODY_BYTES: usize = 1024 * 1024;
    let url = asr_transcriptions_url(&config.base_url)?;
    let wav = encode_wav_16k_mono_silence(250);
    let wav_part = reqwest::multipart::Part::bytes(wav)
        .file_name("openless-asr-check.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("请求体构建失败: {e}"))?;
    let form = reqwest::multipart::Form::new()
        .part("file", wav_part)
        .text("model", model.to_string());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "providerClientInitFailed".to_string())?;
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "providerRequestTimeout".to_string()
            } else {
                "providerNetworkError".to_string()
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("providerHttpStatus:{}", status.as_u16()));
    }
    if let Some(len) = response.content_length() {
        if len as usize > MAX_ASR_VALIDATE_BODY_BYTES {
            return Err("providerResponseTooLarge".to_string());
        }
    }
    use futures_util::StreamExt;
    let mut body = Vec::<u8>::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "providerReadResponseFailed".to_string())?;
        if body.len().saturating_add(chunk.len()) > MAX_ASR_VALIDATE_BODY_BYTES {
            return Err("providerResponseTooLarge".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    let json: Value = serde_json::from_slice(&body).map_err(|_| "asrInvalidJson".to_string())?;
    if !json.is_object() || json.get("text").is_none() {
        return Err("asrMissingTextField".to_string());
    }
    Ok(())
}

fn asr_transcriptions_url(base_url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(base_url.trim()).map_err(|_| "endpointInvalid".to_string())?;
    let host = parsed.host_str().unwrap_or_default();
    let localhost = host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1";
    if parsed.scheme() != "https" && !localhost {
        return Err("endpointMustUseHttps".to_string());
    }

    // Work on the URL path only so we don't corrupt query parameters.
    let mut url = parsed.clone();
    let path = parsed.path().trim_end_matches('/');
    let next_path = if path.ends_with("/audio/transcriptions") {
        path.to_string()
    } else if path.ends_with("/audio") {
        format!("{path}/transcriptions")
    } else if let Some(prefix) = path.strip_suffix("/chat/completions") {
        format!("{prefix}/audio/transcriptions")
    } else {
        format!("{path}/audio/transcriptions")
    };
    url.set_path(&next_path);
    Ok(url.to_string())
}

fn encode_wav_16k_mono_silence(duration_ms: u32) -> Vec<u8> {
    let sample_rate: u32 = 16_000;
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    let samples = (sample_rate as usize * duration_ms as usize) / 1000;
    let pcm_len = samples * bytes_per_sample;
    let data_size = pcm_len as u32;
    let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let chunk_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + pcm_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&num_channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.resize(44 + pcm_len, 0);
    wav
}

async fn fetch_provider_models(config: &ProviderConfig) -> Result<Vec<String>, String> {
    let url = models_url(&config.base_url);
    log::info!("[provider-check] GET {url}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client 初始化失败: {e}"))?;
    let mut request = client.get(&url);
    if !config.api_key.trim().is_empty() {
        request = request.header("Authorization", format!("Bearer {}", config.api_key));
    }
    let response = request.send().await.map_err(|e| {
        if e.is_timeout() {
            "请求超时".to_string()
        } else {
            format!("网络错误: {e}")
        }
    })?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("读取响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("providerHttpStatus:{}", status.as_u16()));
    }
    parse_model_ids(&body)
}

fn models_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/models") {
        return trimmed.to_string();
    }
    if let Some(prefix) = trimmed.strip_suffix("/chat/completions") {
        return format!("{prefix}/models");
    }
    format!("{trimmed}/models")
}

fn parse_model_ids(body: &str) -> Result<Vec<String>, String> {
    let json: Value =
        serde_json::from_str(body).map_err(|e| format!("模型列表不是有效 JSON: {e}"))?;
    let data = json
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "模型列表缺少 data 数组".to_string())?;
    let mut models = data
        .iter()
        .filter_map(|item| item.get("id").and_then(|id| id.as_str()))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    Ok(models)
}

fn parse_account(s: &str) -> Result<CredentialAccount, String> {
    match s {
        "volcengine.app_key" => Ok(CredentialAccount::VolcengineAppKey),
        "volcengine.access_key" => Ok(CredentialAccount::VolcengineAccessKey),
        "volcengine.resource_id" => Ok(CredentialAccount::VolcengineResourceId),
        "ark.api_key" => Ok(CredentialAccount::ArkApiKey),
        "ark.model_id" => Ok(CredentialAccount::ArkModelId),
        "ark.endpoint" => Ok(CredentialAccount::ArkEndpoint),
        "asr.api_key" => Ok(CredentialAccount::AsrApiKey),
        "asr.endpoint" => Ok(CredentialAccount::AsrEndpoint),
        "asr.model" => Ok(CredentialAccount::AsrModel),
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

#[tauri::command]
pub fn list_vocab_presets() -> Result<VocabPresetStore, String> {
    crate::persistence::list_vocab_presets().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_vocab_presets(store: VocabPresetStore) -> Result<(), String> {
    crate::persistence::save_vocab_presets(&store).map_err(|e| e.to_string())
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
pub async fn handle_window_hotkey_event(
    coord: CoordinatorState<'_>,
    event_type: String,
    key: String,
    code: String,
    repeat: bool,
) -> Result<(), String> {
    coord
        .handle_window_hotkey_event(event_type, key, code, repeat)
        .await
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn inject_hotkey_click_for_dev(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.inject_hotkey_click_for_dev().await
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
    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        fn wide_null(value: &str) -> Vec<u16> {
            value.encode_utf16().chain(std::iter::once(0)).collect()
        }

        let uri = match pane.as_str() {
            "microphone" => "ms-settings:privacy-microphone",
            "sound" => "ms-settings:sound",
            "accessibility" => "ms-settings:easeofaccess",
            _ => "ms-settings:",
        };

        let operation = wide_null("open");
        let target = wide_null(uri);
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
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let _ = pane;
        Err("open_system_settings is only supported on macOS and Windows".to_string())
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

// ─────────────────────────── QA (划词语音问答, issue #118) ───────────────────────────

/// 给前端 Settings 页渲染当前 QA 快捷键 label（如 `"Cmd+Shift+;"`）。
/// 未启用时返回空串。
#[tauri::command]
pub fn get_qa_hotkey_label(coord: CoordinatorState<'_>) -> String {
    coord.qa_hotkey_label()
}

/// 设置 QA 快捷键并热更新 monitor。
/// 传入 `None` 形式的字段不在这里支持——前端用 `binding == null` 时调下面的
/// "disable" 写法（写 prefs.qa_hotkey = None）即可。
#[tauri::command]
pub fn set_qa_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
            return Err("Shift 单键目前只能用于翻译快捷键".into());
        }
    }
    let mut prefs = coord.prefs().get();
    if let Some(binding) = binding.as_ref() {
        reject_dictation_qa_hotkey_overlap(&prefs.dictation_hotkey, binding)?;
        reject_qa_translation_hotkey_overlap(binding, &prefs.translation_hotkey)?;
        reject_qa_switch_style_hotkey_overlap(binding, &prefs.switch_style_hotkey)?;
        reject_qa_open_app_hotkey_overlap(binding, &prefs.open_app_hotkey)?;
    }
    prefs.qa_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_qa_hotkey_binding();
    Ok(())
}

/// 用户点 ✕ / 按 Esc 关 QA 浮窗。
#[tauri::command]
pub fn qa_window_dismiss(coord: CoordinatorState<'_>) {
    coord.qa_window_dismiss();
}

/// 用户点 📌 / 取消 📌。pinned=true 时浮窗不会自动隐藏。
#[tauri::command]
pub fn qa_window_pin(coord: CoordinatorState<'_>, pinned: bool) {
    coord.qa_window_pin(pinned);
}

// ─────────────────────────── 自定义组合键 ───────────────────────────

/// 测试一个组合键是否可以注册（验证格式，不实际注册）。
#[tauri::command]
pub fn validate_shortcut_binding(binding: ShortcutBinding) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_dictation_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    reject_bare_shift_dictation_shortcut(&binding)?;
    let mut prefs = coord.prefs().get();
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&binding, qa_hotkey)?;
    }
    reject_dictation_translation_hotkey_overlap(&binding, &prefs.translation_hotkey)?;
    reject_dictation_switch_style_hotkey_overlap(&binding, &prefs.switch_style_hotkey)?;
    reject_dictation_open_app_hotkey_overlap(&binding, &prefs.open_app_hotkey)?;
    prefs.dictation_hotkey = binding;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_hotkey_binding();
    coord.update_combo_hotkey_binding();
    Ok(())
}

#[tauri::command]
pub fn set_translation_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    let previous = coord.prefs().get();
    reject_dictation_translation_hotkey_overlap(&previous.dictation_hotkey, &binding)?;
    if let Some(qa_hotkey) = previous.qa_hotkey.as_ref() {
        reject_qa_translation_hotkey_overlap(qa_hotkey, &binding)?;
    }
    reject_translation_switch_style_hotkey_overlap(&binding, &previous.switch_style_hotkey)?;
    reject_translation_open_app_hotkey_overlap(&binding, &previous.open_app_hotkey)?;
    let mut prefs = previous.clone();
    prefs.translation_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    if let Err(e) = coord.try_update_translation_hotkey_binding() {
        if let Err(rollback_err) = coord.prefs().set(previous) {
            log::warn!("[commands] 回滚翻译快捷键失败: {rollback_err}");
        }
        coord.update_translation_hotkey_binding();
        return Err(e);
    }
    Ok(())
}

#[tauri::command]
pub fn set_switch_style_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    reject_modifier_only_action_shortcut(&binding)?;
    let mut prefs = coord.prefs().get();
    reject_dictation_switch_style_hotkey_overlap(&prefs.dictation_hotkey, &binding)?;
    reject_translation_switch_style_hotkey_overlap(&prefs.translation_hotkey, &binding)?;
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_qa_switch_style_hotkey_overlap(qa_hotkey, &binding)?;
    }
    reject_switch_style_open_app_hotkey_overlap(&binding, &prefs.open_app_hotkey)?;
    prefs.switch_style_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_switch_style_hotkey_binding();
    Ok(())
}

#[tauri::command]
pub fn set_open_app_hotkey(
    coord: CoordinatorState<'_>,
    binding: ShortcutBinding,
) -> Result<(), String> {
    crate::shortcut_binding::validate_binding(&binding).map_err(|e| e.to_string())?;
    reject_modifier_only_action_shortcut(&binding)?;
    let mut prefs = coord.prefs().get();
    reject_dictation_open_app_hotkey_overlap(&prefs.dictation_hotkey, &binding)?;
    reject_translation_open_app_hotkey_overlap(&prefs.translation_hotkey, &binding)?;
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_qa_open_app_hotkey_overlap(qa_hotkey, &binding)?;
    }
    reject_switch_style_open_app_hotkey_overlap(&prefs.switch_style_hotkey, &binding)?;
    prefs.open_app_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_open_app_hotkey_binding();
    Ok(())
}

fn reject_modifier_only_action_shortcut(binding: &ShortcutBinding) -> Result<(), String> {
    if binding.modifiers.is_empty()
        && (binding.primary.eq_ignore_ascii_case("shift")
            || crate::shortcut_binding::legacy_modifier_trigger(binding).is_some())
    {
        return Err("该快捷键需要使用组合键或非修饰主键".into());
    }
    Ok(())
}

#[tauri::command]
pub fn validate_combo_hotkey(binding: ComboBinding) -> Result<(), String> {
    let shortcut = ShortcutBinding {
        primary: binding.primary,
        modifiers: binding.modifiers,
    };
    reject_bare_shift_dictation_shortcut(&shortcut)?;
    crate::combo_hotkey::validate_binding(&shortcut).map_err(|e| e.to_string())
}

/// 设置自定义录音组合键并热更新 monitor。
#[tauri::command]
pub fn set_combo_hotkey(coord: CoordinatorState<'_>, binding: ComboBinding) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    let shortcut = ShortcutBinding {
        primary: binding.primary.clone(),
        modifiers: binding.modifiers.clone(),
    };
    reject_bare_shift_dictation_shortcut(&shortcut)?;
    crate::combo_hotkey::validate_binding(&shortcut).map_err(|e| e.to_string())?;
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&shortcut, qa_hotkey)?;
    }
    reject_dictation_translation_hotkey_overlap(&shortcut, &prefs.translation_hotkey)?;
    reject_dictation_switch_style_hotkey_overlap(&shortcut, &prefs.switch_style_hotkey)?;
    reject_dictation_open_app_hotkey_overlap(&shortcut, &prefs.open_app_hotkey)?;
    prefs.custom_combo_hotkey = Some(binding);
    prefs.dictation_hotkey = shortcut;
    sync_dictation_hotkey_legacy_fields(&mut prefs);
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_hotkey_binding();
    coord.update_combo_hotkey_binding();
    Ok(())
}

fn reject_bare_shift_dictation_shortcut(binding: &ShortcutBinding) -> Result<(), String> {
    if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
        return Err("Shift 单键目前只能用于翻译快捷键".into());
    }
    Ok(())
}

fn sync_dictation_hotkey_legacy_fields(prefs: &mut UserPreferences) {
    if let Some(trigger) = crate::shortcut_binding::legacy_modifier_trigger(&prefs.dictation_hotkey)
    {
        prefs.hotkey.trigger = trigger;
        prefs.custom_combo_hotkey = None;
        return;
    }
    prefs.hotkey.trigger = crate::types::HotkeyTrigger::Custom;
    prefs.custom_combo_hotkey = if prefs.dictation_hotkey.primary.trim().is_empty() {
        None
    } else {
        Some(ComboBinding {
            primary: prefs.dictation_hotkey.primary.clone(),
            modifiers: prefs.dictation_hotkey.modifiers.clone(),
        })
    };
}

fn reject_dictation_qa_hotkey_overlap(
    dictation: &ShortcutBinding,
    qa: &ShortcutBinding,
) -> Result<(), String> {
    if shortcut_bindings_overlap(dictation, qa) {
        return Err("QA 快捷键不能和听写快捷键相同".into());
    }
    Ok(())
}

fn reject_hotkey_overlap(
    left: &ShortcutBinding,
    right: &ShortcutBinding,
    message: &'static str,
) -> Result<(), String> {
    if shortcut_bindings_overlap(left, right) {
        return Err(message.into());
    }
    Ok(())
}

fn reject_hotkey_collisions(prefs: &UserPreferences) -> Result<(), String> {
    if let Some(qa_hotkey) = prefs.qa_hotkey.as_ref() {
        reject_dictation_qa_hotkey_overlap(&prefs.dictation_hotkey, qa_hotkey)?;
        reject_qa_translation_hotkey_overlap(qa_hotkey, &prefs.translation_hotkey)?;
        reject_qa_switch_style_hotkey_overlap(qa_hotkey, &prefs.switch_style_hotkey)?;
        reject_qa_open_app_hotkey_overlap(qa_hotkey, &prefs.open_app_hotkey)?;
    }
    reject_dictation_translation_hotkey_overlap(
        &prefs.dictation_hotkey,
        &prefs.translation_hotkey,
    )?;
    reject_dictation_switch_style_hotkey_overlap(
        &prefs.dictation_hotkey,
        &prefs.switch_style_hotkey,
    )?;
    reject_dictation_open_app_hotkey_overlap(&prefs.dictation_hotkey, &prefs.open_app_hotkey)?;
    reject_translation_switch_style_hotkey_overlap(
        &prefs.translation_hotkey,
        &prefs.switch_style_hotkey,
    )?;
    reject_translation_open_app_hotkey_overlap(&prefs.translation_hotkey, &prefs.open_app_hotkey)?;
    reject_switch_style_open_app_hotkey_overlap(
        &prefs.switch_style_hotkey,
        &prefs.open_app_hotkey,
    )?;
    Ok(())
}

fn reject_dictation_translation_hotkey_overlap(
    dictation: &ShortcutBinding,
    translation: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(dictation, translation, "翻译快捷键不能和听写快捷键相同")
}

fn reject_dictation_switch_style_hotkey_overlap(
    dictation: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        dictation,
        switch_style,
        "切换风格快捷键不能和听写快捷键相同",
    )
}

fn reject_dictation_open_app_hotkey_overlap(
    dictation: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(dictation, open_app, "打开应用快捷键不能和听写快捷键相同")
}

fn reject_qa_translation_hotkey_overlap(
    qa: &ShortcutBinding,
    translation: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(qa, translation, "翻译快捷键不能和 QA 快捷键相同")
}

fn reject_qa_switch_style_hotkey_overlap(
    qa: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(qa, switch_style, "切换风格快捷键不能和 QA 快捷键相同")
}

fn reject_qa_open_app_hotkey_overlap(
    qa: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(qa, open_app, "打开应用快捷键不能和 QA 快捷键相同")
}

fn reject_translation_switch_style_hotkey_overlap(
    translation: &ShortcutBinding,
    switch_style: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        translation,
        switch_style,
        "切换风格快捷键不能和翻译快捷键相同",
    )
}

fn reject_translation_open_app_hotkey_overlap(
    translation: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(translation, open_app, "打开应用快捷键不能和翻译快捷键相同")
}

fn reject_switch_style_open_app_hotkey_overlap(
    switch_style: &ShortcutBinding,
    open_app: &ShortcutBinding,
) -> Result<(), String> {
    reject_hotkey_overlap(
        switch_style,
        open_app,
        "打开应用快捷键不能和切换风格快捷键相同",
    )
}

fn shortcut_bindings_overlap(left: &ShortcutBinding, right: &ShortcutBinding) -> bool {
    let left_legacy = crate::shortcut_binding::legacy_modifier_trigger(left);
    let right_legacy = crate::shortcut_binding::legacy_modifier_trigger(right);
    match (left_legacy, right_legacy) {
        (Some(left), Some(right)) => left == right,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => {
            let Ok(left) = crate::shortcut_binding::parse_global_hotkey(left) else {
                return false;
            };
            let Ok(right) = crate::shortcut_binding::parse_global_hotkey(right) else {
                return false;
            };
            left == right
        }
    }
}

// ─────────────────────────── local ASR (Qwen3-ASR) ───────────────────────────

use crate::asr::local::{
    download::{fetch_remote_info, RemoteInfo},
    DownloadManager, Mirror, ModelId, ModelStatus, PROVIDER_ID as LOCAL_PROVIDER_ID,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrSettings {
    pub provider_id: String,
    pub active_model: String,
    pub mirror: String,
    /// macOS 才编入引擎；Windows 端 UI 需要据此把"开始下载"按钮灰掉。
    pub engine_available: bool,
}

#[tauri::command]
pub fn local_asr_get_settings(coord: CoordinatorState<'_>) -> LocalAsrSettings {
    let prefs = coord.prefs().get();
    LocalAsrSettings {
        provider_id: LOCAL_PROVIDER_ID.into(),
        active_model: prefs.local_asr_active_model,
        mirror: prefs.local_asr_mirror,
        engine_available: cfg!(target_os = "macos"),
    }
}

#[tauri::command]
pub fn local_asr_set_active_model(
    coord: CoordinatorState<'_>,
    model_id: String,
) -> Result<(), String> {
    if ModelId::from_str(&model_id).is_none() {
        return Err(format!("unknown model id: {model_id}"));
    }
    let mut prefs = coord.prefs().get();
    prefs.local_asr_active_model = model_id;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_asr_set_mirror(coord: CoordinatorState<'_>, mirror: String) -> Result<(), String> {
    let _normalized = Mirror::from_str(&mirror);
    let mut prefs = coord.prefs().get();
    prefs.local_asr_mirror = mirror;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_asr_list_models() -> Vec<ModelStatus> {
    crate::asr::local::models::list_status()
}

/// 实时去 HuggingFace API 拉某个模型的真实文件清单 + 总尺寸；
/// 前端在显示模型卡时调一次，避免硬编码尺寸过期。
#[tauri::command]
pub async fn local_asr_fetch_remote_info(
    model_id: String,
    mirror: Option<String>,
) -> Result<RemoteInfo, String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    let m = mirror.as_deref().map(Mirror::from_str).unwrap_or_default();
    fetch_remote_info(id, m).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn local_asr_download_model(
    app: AppHandle,
    manager: State<'_, Arc<DownloadManager>>,
    model_id: String,
    mirror: Option<String>,
) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    let m = mirror.as_deref().map(Mirror::from_str).unwrap_or_default();
    manager.start(app, id, m);
    Ok(())
}

#[tauri::command]
pub fn local_asr_cancel_download(
    manager: State<'_, Arc<DownloadManager>>,
    model_id: String,
) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    manager.cancel(id);
    Ok(())
}

#[tauri::command]
pub fn local_asr_delete_model(coord: CoordinatorState<'_>, model_id: String) -> Result<(), String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    // 如果内存里加载的就是要删的这个模型，先释放：否则 mmap 残留指向已 unlink 的文件，
    // 且 RAM 直到下次切模型 / 用户手动按"释放"才回收。
    if coord.local_asr_loaded_model().as_deref() == Some(id.as_str()) {
        coord.release_local_asr_engine();
    }
    crate::asr::local::models::delete_model(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn local_asr_test_model(
    model_id: String,
) -> Result<crate::asr::local::test_run::TestResult, String> {
    let id = ModelId::from_str(&model_id).ok_or_else(|| format!("unknown model id: {model_id}"))?;
    crate::asr::local::test_run::run_test(id)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrEngineStatus {
    pub loaded: bool,
    pub model_id: Option<String>,
    pub keep_loaded_secs: u32,
}

#[tauri::command]
pub fn local_asr_engine_status(coord: CoordinatorState<'_>) -> LocalAsrEngineStatus {
    let prefs = coord.prefs().get();
    LocalAsrEngineStatus {
        loaded: coord.local_asr_loaded_model().is_some(),
        model_id: coord.local_asr_loaded_model(),
        keep_loaded_secs: prefs.local_asr_keep_loaded_secs,
    }
}

#[tauri::command]
pub fn local_asr_release_engine(coord: CoordinatorState<'_>) {
    coord.release_local_asr_engine();
}

#[tauri::command]
pub fn local_asr_preload(coord: tauri::State<'_, std::sync::Arc<crate::coordinator::Coordinator>>) {
    coord.preload_local_asr_in_background();
}

#[tauri::command]
pub fn local_asr_set_keep_loaded_secs(
    coord: CoordinatorState<'_>,
    seconds: u32,
) -> Result<(), String> {
    let mut prefs = coord.prefs().get();
    prefs.local_asr_keep_loaded_secs = seconds;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

// ───────────────────── Windows local ASR (Foundry Local Whisper) ─────────────────────

fn active_foundry_model_from_prefs(prefs: &UserPreferences) -> String {
    if model_alias_is_known(&prefs.foundry_local_asr_model) {
        prefs.foundry_local_asr_model.clone()
    } else {
        DEFAULT_MODEL_ALIAS.to_string()
    }
}

fn validate_foundry_model_alias(model_alias: &str) -> Result<(), String> {
    if model_alias_is_known(model_alias) {
        Ok(())
    } else {
        Err(format!(
            "unknown Foundry Whisper model alias: {model_alias}"
        ))
    }
}

fn normalize_foundry_language_hint(language_hint: &str) -> Result<String, String> {
    let normalized = language_hint.trim().to_string();
    if normalized.is_empty()
        || (normalized.len() == 2 && normalized.bytes().all(|b| b.is_ascii_lowercase()))
    {
        Ok(normalized)
    } else {
        Err("language hint must be empty or ISO 639-1 lowercase code".to_string())
    }
}

#[tauri::command]
pub fn foundry_local_asr_status(
    coord: CoordinatorState<'_>,
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
) -> FoundryRuntimeStatus {
    let prefs = coord.prefs().get();
    let active_model = active_foundry_model_from_prefs(&prefs);
    runtime.status_snapshot(&active_model)
}

#[tauri::command]
pub async fn foundry_local_asr_catalog(
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
) -> Result<Vec<FoundryCatalogModel>, String> {
    runtime
        .catalog_snapshot()
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn foundry_local_asr_set_model(
    coord: CoordinatorState<'_>,
    model_alias: String,
) -> Result<(), String> {
    validate_foundry_model_alias(&model_alias)?;
    let mut prefs = coord.prefs().get();
    prefs.foundry_local_asr_model = model_alias;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn foundry_local_asr_set_language_hint(
    coord: CoordinatorState<'_>,
    language_hint: String,
) -> Result<(), String> {
    let normalized = normalize_foundry_language_hint(&language_hint)?;
    let mut prefs = coord.prefs().get();
    prefs.foundry_local_asr_language_hint = normalized;
    coord.prefs().set(prefs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn foundry_local_asr_prepare(
    app: AppHandle,
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
    model_alias: String,
) -> Result<String, String> {
    validate_foundry_model_alias(&model_alias)?;
    let progress_app = app.clone();
    let result = runtime
        .ensure_loaded_with_progress(&model_alias, move |payload| {
            emit_foundry_prepare_progress(&progress_app, payload);
        })
        .await;
    match result {
        Ok(model_id) => Ok(model_id),
        Err(error) => {
            let message = format!("{error:#}");
            emit_foundry_prepare_progress(
                &app,
                FoundryPrepareProgressPayload::failed(
                    model_alias,
                    "Foundry Local Whisper prepare failed",
                    message.clone(),
                ),
            );
            Err(message)
        }
    }
}

#[tauri::command]
pub fn foundry_local_asr_cancel_prepare(
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
) -> Result<(), String> {
    runtime.request_cancel_prepare();
    Ok(())
}

#[tauri::command]
pub async fn foundry_local_asr_release(
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
) -> Result<(), String> {
    runtime.release_now().await.map_err(|e| format!("{e:#}"))
}

fn emit_foundry_prepare_progress(app: &AppHandle, payload: FoundryPrepareProgressPayload) {
    if let Err(error) = app.emit("foundry-local-asr-prepare-progress", payload) {
        log::warn!("[foundry-asr] emit prepare progress failed: {error}");
    }
}

/// 把当前会话的 openless.log 复制到用户选择的位置（前端用 plugin-dialog 拿 target_path）。
/// 路径来自 lib::log_dir_path() —— mac: ~/Library/Logs/OpenLess/openless.log，
/// windows: %LOCALAPPDATA%\OpenLess\Logs\openless.log。
#[tauri::command]
pub fn export_error_log(target_path: String) -> Result<(), String> {
    let src = crate::log_dir_path().join("openless.log");
    if !src.exists() {
        return Err(format!("日志文件不存在：{}", src.display()));
    }
    std::fs::copy(&src, std::path::Path::new(&target_path))
        .map(|_| ())
        .map_err(|e| format!("复制日志失败：{}", e))
}

// ─────────────────────────── unused but exported (silences dead_code) ───────────────────────────

#[allow(dead_code)]
fn _ensure_snapshot_used(_: CredentialsSnapshot) {}

#[cfg(test)]
mod tests {
    use super::{
        active_asr_is_keyless_for_validation, active_foundry_model_from_prefs,
        asr_configured_for_provider, asr_transcriptions_url, fetch_provider_models,
        llm_configured_for_provider, local_asr_release_plan_for_provider, models_url,
        normalize_foundry_language_hint, parse_model_ids, persist_settings,
        release_foundry_runtime_if_inactive, validate_foundry_model_alias, ProviderConfig,
        SettingsWriter,
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
        dictation_refreshes: Mutex<u32>,
        qa_refreshes: Mutex<u32>,
        combo_refreshes: Mutex<u32>,
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
            ..snapshot()
        };
        assert!(asr_configured_for_provider("volcengine", &volcengine));

        let whisper_key_only = CredentialsSnapshot {
            asr_api_key: Some("key".into()),
            ..snapshot()
        };
        assert!(!asr_configured_for_provider("whisper", &whisper_key_only));

        let whisper_keyless_ready = CredentialsSnapshot {
            asr_endpoint: Some("https://api.openai.com/v1".into()),
            asr_model: Some("whisper-1".into()),
            ..snapshot()
        };
        assert!(asr_configured_for_provider(
            "whisper",
            &whisper_keyless_ready
        ));

        assert!(asr_configured_for_provider(
            crate::asr::local::PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(target_os = "windows")]
        assert!(asr_configured_for_provider(
            crate::asr::local::foundry::PROVIDER_ID,
            &snapshot()
        ));
        #[cfg(not(target_os = "windows"))]
        assert!(!asr_configured_for_provider(
            crate::asr::local::foundry::PROVIDER_ID,
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
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::PROVIDER_ID
        ));
        #[cfg(target_os = "windows")]
        assert!(active_asr_is_keyless_for_validation(
            crate::asr::local::foundry::PROVIDER_ID
        ));
        #[cfg(not(target_os = "windows"))]
        assert!(!active_asr_is_keyless_for_validation(
            crate::asr::local::foundry::PROVIDER_ID
        ));
        assert!(!active_asr_is_keyless_for_validation("volcengine"));
        assert!(!active_asr_is_keyless_for_validation("whisper"));
    }

    #[test]
    fn provider_switch_release_plan_covers_inactive_local_runtimes() {
        let qwen = local_asr_release_plan_for_provider(crate::asr::local::PROVIDER_ID);
        assert!(!qwen.qwen);
        assert!(qwen.foundry);

        let foundry = local_asr_release_plan_for_provider(crate::asr::local::foundry::PROVIDER_ID);
        assert!(foundry.qwen);
        assert!(!foundry.foundry);

        let cloud = local_asr_release_plan_for_provider("volcengine");
        assert!(cloud.qwen);
        assert!(cloud.foundry);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn provider_switch_release_requests_foundry_prepare_cancel_first() {
        let runtime = std::sync::Arc::new(crate::asr::local::FoundryLocalRuntime::new());

        release_foundry_runtime_if_inactive(&runtime, true).await;

        assert!(runtime.cancel_prepare_requested_for_tests());
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
    fn credentials_status_accepts_keyless_custom_llm_only() {
        let keyless_ready = CredentialsSnapshot {
            ark_endpoint: Some("http://localhost:11434/v1".into()),
            ark_model_id: Some("qwen".into()),
            ..snapshot()
        };
        assert!(llm_configured_for_provider("custom", &keyless_ready));
        assert!(llm_configured_for_provider("self-hosted", &keyless_ready));
        assert!(llm_configured_for_provider("openrouterFree", &keyless_ready));

        let hosted_keyless = CredentialsSnapshot {
            ark_endpoint: Some("https://openrouter.ai/api/v1".into()),
            ark_model_id: Some("qwen/qwen3-coder:free".into()),
            ..snapshot()
        };
        assert!(!llm_configured_for_provider("openrouterFree", &hosted_keyless));

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
        assert!(!llm_configured_for_provider("custom", &key_without_endpoint));

        let endpoint_without_model = CredentialsSnapshot {
            ark_endpoint: Some("http://localhost:11434/v1".into()),
            ..snapshot()
        };
        assert!(!llm_configured_for_provider("custom", &endpoint_without_model));
    }

    impl SettingsWriter for FakeSettingsWriter {
        fn write_settings(&self, prefs: UserPreferences) -> Result<(), String> {
            *self.saved.lock().unwrap() = Some(prefs);
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

        fn refresh_translation_hotkey(&self) {}
        fn refresh_switch_style_hotkey(&self) {}
        fn refresh_open_app_hotkey(&self) {}
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
    }

    #[test]
    fn parse_model_ids_sorts_and_deduplicates() {
        let models =
            parse_model_ids(r#"{ "data": [{ "id": "b" }, { "id": "a" }, { "id": "b" }] }"#)
                .unwrap();
        assert_eq!(models, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn persist_settings_refreshes_both_hotkey_pipelines() {
        let writer = FakeSettingsWriter::default();
        let prefs = UserPreferences {
            hotkey: HotkeyBinding {
                trigger: HotkeyTrigger::RightControl,
                mode: HotkeyMode::Toggle,
                ..Default::default()
            },
            qa_hotkey: Some(ShortcutBinding {
                primary: ";".to_string(),
                modifiers: vec!["ctrl".to_string(), "shift".to_string()],
            }),
            ..Default::default()
        };

        persist_settings(&writer, prefs.clone()).unwrap();

        let saved = writer
            .saved
            .lock()
            .unwrap()
            .clone()
            .expect("settings saved");
        assert_eq!(saved.hotkey.trigger, HotkeyTrigger::RightOption);
        assert_eq!(saved.hotkey.mode, prefs.hotkey.mode);
        assert_eq!(
            saved.qa_hotkey.unwrap().primary,
            prefs.qa_hotkey.unwrap().primary
        );
        assert_eq!(*writer.dictation_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.qa_refreshes.lock().unwrap(), 1);
        assert_eq!(*writer.combo_refreshes.lock().unwrap(), 1);
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
    fn persist_settings_rejects_dictation_translation_overlap() {
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

        assert_eq!(
            persist_settings(&writer, prefs),
            Err("翻译快捷键不能和听写快捷键相同".into())
        );
        assert!(writer.saved.lock().unwrap().is_none());
    }

    #[test]
    fn persist_settings_rejects_translation_switch_style_overlap() {
        let writer = FakeSettingsWriter::default();
        let binding = ShortcutBinding {
            primary: "T".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let prefs = UserPreferences {
            translation_hotkey: binding.clone(),
            switch_style_hotkey: binding,
            ..Default::default()
        };

        assert_eq!(
            persist_settings(&writer, prefs),
            Err("切换风格快捷键不能和翻译快捷键相同".into())
        );
        assert!(writer.saved.lock().unwrap().is_none());
    }

    #[test]
    fn persist_settings_rejects_switch_style_open_app_overlap() {
        let writer = FakeSettingsWriter::default();
        let binding = ShortcutBinding {
            primary: "K".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let prefs = UserPreferences {
            switch_style_hotkey: binding.clone(),
            open_app_hotkey: binding,
            ..Default::default()
        };

        assert_eq!(
            persist_settings(&writer, prefs),
            Err("打开应用快捷键不能和切换风格快捷键相同".into())
        );
        assert!(writer.saved.lock().unwrap().is_none());
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
        })
        .await
        .unwrap();

        assert_eq!(models, vec!["m1".to_string(), "m2".to_string()]);
        server.join().unwrap();
    }
}
