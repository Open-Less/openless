//! JNI bridge between Kotlin overlay code and the shared Rust backend.
//!
//! Dictation lifecycle calls use [`openless_core::OpenLessBackend`] directly.
//! [`Coordinator`] adapts Android overlay/window effects and forwards QA and
//! style-pack actions to their Core-owned services; it is not another owner of
//! their business state. Kotlin only receives the existing JNI wire envelope.

use std::sync::{Arc, OnceLock};

use openless_core::{
    BackendError, BackendErrorCode, DictationStartOptions, DictationStopOptions, OpenLessBackend,
};

use crate::coordinator::Coordinator;
use crate::persistence::{CredentialAccount, CredentialsSnapshot, CredentialsVault, PreferencesStore};
use crate::types::{CapsulePayload, CapsuleState};

static COORDINATOR: OnceLock<Arc<Coordinator>> = OnceLock::new();
static CORE_BACKEND: OnceLock<Arc<OpenLessBackend>> = OnceLock::new();
static OVERLAY_VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "android")]
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AndroidBackendSnapshotResponse {
    contract_version: &'static str,
    ok: bool,
    payload: Option<openless_core::BackendSnapshot>,
    error: Option<&'static str>,
}

fn android_backend_snapshot_response(backend: Option<&OpenLessBackend>) -> String {
    let response = match backend {
        Some(backend) if backend.snapshot().running => AndroidBackendSnapshotResponse {
            contract_version: openless_core::BACKEND_CONTRACT_VERSION,
            ok: true,
            payload: Some(backend.snapshot()),
            error: None,
        },
        Some(backend) => AndroidBackendSnapshotResponse {
            contract_version: openless_core::BACKEND_CONTRACT_VERSION,
            ok: false,
            payload: Some(backend.snapshot()),
            error: Some("backend is not running"),
        },
        None => AndroidBackendSnapshotResponse {
            contract_version: openless_core::BACKEND_CONTRACT_VERSION,
            ok: false,
            payload: None,
            error: Some("backend unavailable"),
        },
    };
    serde_json::to_string(&response).expect("Android backend snapshot is serializable")
}

pub fn register_android_coordinator(coordinator: Arc<Coordinator>) {
    let _ = COORDINATOR.set(coordinator);
}

/// Settings-export subset of [`openless_core::UserPreferences`] — just the
/// fields the Android keyboard settings page's export/import feature covers
/// (ASR/LLM provider selection, style pack selection). Every field is
/// `Option` so a partial import (the settings page lets the user deselect
/// categories before importing) can omit a field entirely rather than
/// forcing an empty-string overwrite.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ExportedPreferencesSubset {
    active_asr_provider: Option<String>,
    active_llm_provider: Option<String>,
    active_style_pack_id: Option<String>,
    selection_polish_style_pack_id: Option<String>,
}

fn export_preferences_subset_json() -> String {
    let prefs = match PreferencesStore::new() {
        Ok(store) => store.get(),
        Err(error) => {
            log::warn!("[android-native] open preferences store for settings export failed: {error:#}");
            return "{}".to_string();
        }
    };
    let subset = ExportedPreferencesSubset {
        active_asr_provider: Some(prefs.active_asr_provider),
        active_llm_provider: Some(prefs.active_llm_provider),
        active_style_pack_id: Some(prefs.active_style_pack_id),
        selection_polish_style_pack_id: Some(prefs.selection_polish_style_pack_id),
    };
    serde_json::to_string(&subset).unwrap_or_else(|_| "{}".to_string())
}

/// Applies only the fields present in `json` (see
/// [`ExportedPreferencesSubset`]'s own doc comment on why every field is
/// optional). Syncs the credential vault's own active-ASR pointer first —
/// see [`crate::commands::sync_active_asr_provider_to_vault`] — so the
/// plain (non-provider-scoped) `CredentialsVault::set()` calls a caller
/// separately makes via [`import_credentials_snapshot_json`] land in the
/// provider slot this import just selected, not whatever the target device
/// happened to have active before.
fn import_preferences_subset_json(json: &str) {
    let parsed: ExportedPreferencesSubset = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("[android-native] parse imported preferences subset failed: {error}");
            return;
        }
    };
    let store = match PreferencesStore::new() {
        Ok(store) => store,
        Err(error) => {
            log::warn!("[android-native] open preferences store for settings import failed: {error:#}");
            return;
        }
    };
    let mut prefs = store.get();
    if let Some(value) = parsed.active_asr_provider {
        if let Err(error) = crate::commands::sync_active_asr_provider_to_vault(&value) {
            log::warn!("[android-native] sync imported ASR provider to vault failed: {error}");
        }
        prefs.active_asr_provider = value;
    }
    if let Some(value) = parsed.active_llm_provider {
        prefs.active_llm_provider = value;
    }
    if let Some(value) = parsed.active_style_pack_id {
        prefs.active_style_pack_id = value;
    }
    if let Some(value) = parsed.selection_polish_style_pack_id {
        prefs.selection_polish_style_pack_id = value;
    }
    if let Err(error) = store.set(prefs) {
        log::warn!("[android-native] save imported preferences subset failed: {error:#}");
    }
}

fn export_credentials_snapshot_json() -> String {
    serde_json::to_string(&CredentialsVault::snapshot()).unwrap_or_else(|_| "{}".to_string())
}

/// Restores whichever fields are present (`Some`) in the snapshot — the
/// settings page's export always includes every field it read, but an
/// older or hand-edited import file might only have some of them. Plain
/// (non-provider-scoped) `CredentialsVault::set()` calls, matching
/// `CredentialsVault::snapshot()`'s own plain reads (see that function's
/// `credentials_snapshot()` implementation in persistence/credentials.rs) —
/// call [`import_preferences_subset_json`] first if the import also
/// includes a provider-selection change, so these land in the right slot.
fn import_credentials_snapshot_json(json: &str) {
    let snapshot: CredentialsSnapshot = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("[android-native] parse imported credentials snapshot failed: {error}");
            return;
        }
    };
    let fields: [(CredentialAccount, Option<String>); 17] = [
        (CredentialAccount::VolcengineAppKey, snapshot.volcengine_app_key),
        (
            CredentialAccount::VolcengineAccessKey,
            snapshot.volcengine_access_key,
        ),
        (
            CredentialAccount::VolcengineResourceId,
            snapshot.volcengine_resource_id,
        ),
        (CredentialAccount::VolcengineService, snapshot.volcengine_service),
        (
            CredentialAccount::VolcengineAuthMode,
            snapshot.volcengine_auth_mode,
        ),
        (CredentialAccount::VolcengineApiKey, snapshot.volcengine_api_key),
        (CredentialAccount::AsrApiKey, snapshot.asr_api_key),
        (CredentialAccount::AsrEndpoint, snapshot.asr_endpoint),
        (CredentialAccount::AsrModel, snapshot.asr_model),
        (CredentialAccount::XfyunAppId, snapshot.xfyun_app_id),
        (CredentialAccount::XfyunApiKey, snapshot.xfyun_api_key),
        (
            CredentialAccount::TencentCloudAppId,
            snapshot.tencent_cloud_app_id,
        ),
        (
            CredentialAccount::TencentCloudSecretId,
            snapshot.tencent_cloud_secret_id,
        ),
        (
            CredentialAccount::TencentCloudSecretKey,
            snapshot.tencent_cloud_secret_key,
        ),
        (CredentialAccount::ArkApiKey, snapshot.ark_api_key),
        (CredentialAccount::ArkModelId, snapshot.ark_model_id),
        (CredentialAccount::ArkEndpoint, snapshot.ark_endpoint),
    ];
    for (account, value) in fields {
        if let Some(value) = value {
            if let Err(error) = CredentialsVault::set(account, &value) {
                log::warn!("[android-native] import credential field failed: {error:#}");
            }
        }
    }
}

pub fn register_android_backend(backend: Arc<OpenLessBackend>) {
    let _ = CORE_BACKEND.set(backend);
}

#[cfg(target_os = "android")]
pub fn register_android_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

/// Rebuilds the "main" WebviewWindow for a fresh `OpenLessBackendWarmupActivity`
/// instance created after the OS truly destroyed the previous one (not a
/// config change) — the settings-reopen black screen this fixes.
///
/// Wry's own `android_setup()` (src/android/mod.rs) only sends a
/// `CreateWebView` message for an Activity instance whose `activity_id` is
/// already a key in its `WEBVIEW_ATTRIBUTES` map — populated once, for
/// whichever Activity happened to be first. A real Activity destroy erases
/// that map entry (`main_pipe.rs`'s `OnDestroy` handler calls
/// `destroy_webview()`), and a brand new Activity instance gets its own,
/// never-seen-before `activity_id` (falls back to `hashCode()` — see
/// `WryActivity.kt`), so it never matches and `onWebViewCreate()` never
/// fires: a permanently black window, confirmed via on-device logcat.
///
/// Calling this after the new Activity's `onCreate()` mirrors exactly how
/// the very first "main" window gets built at cold app start: Tao's
/// `Window::new()` on Android (`tao::platform_impl::android`) resolves the
/// target Activity via `next_available_activity()` — the first registered
/// Activity that doesn't have a window yet — which is precisely this fresh
/// instance (its own `onActivityCreate()` already registered it with
/// `window_created: false` by the time this runs). No explicit Activity
/// reference needs to cross the JNI boundary for this call.
#[cfg(target_os = "android")]
pub fn ensure_main_webview_window() -> Result<(), String> {
    use tauri::Manager;

    let app = APP_HANDLE
        .get()
        .ok_or_else(|| "AppHandle not yet registered".to_string())?;
    // This only ever runs from a fresh OpenLessBackendWarmupActivity.onCreate()
    // (see its call site) — which Kotlin only reaches after openSettings()
    // already decided no live instance exists (openSettingsIfRunning()
    // failed). So any "main" record found here is necessarily stale: on-device
    // logs showed Tauri's own WindowManager can keep a "main" entry around
    // even after Wry's activity_id-keyed bookkeeping (WEBVIEW_ATTRIBUTES/
    // CONTEXTS/ACTIVITY_PROXY) has already been cleared for the dead Activity
    // — trusting "already exists" as "nothing to do" here left the window
    // permanently un-rebuilt for the new activity_id (silent black screen,
    // no error, nothing to catch). Always destroy-then-rebuild instead
    // (see below for why "destroy" alone isn't the whole fix).
    if let Some(stale) = app.get_webview_window("main") {
        // Both close() and destroy() are asynchronous despite their doc
        // comments' wording — confirmed by reading tauri-runtime-wry's
        // source: both just send a message through the tao event loop's
        // proxy and return immediately (`self.context.proxy.send_event(...)`),
        // they don't block on the event loop actually processing it. The
        // label is only dropped from Tauri's own AppManager registry when a
        // genuine platform WindowEvent::Destroyed later bubbles all the way
        // up (AppManager::on_window_close, private to the tauri crate — this
        // file has no other way to reach it). Firing destroy() and
        // immediately calling build() right after, as before, raced that:
        // on-device logs showed the label still present a line later. Give
        // the event loop a short bounded window to actually catch up before
        // giving up — this runs on the calling JNI thread (Android's UI
        // thread, from onCreate()), so it must stay short even though it
        // means blocking that thread briefly.
        let destroyed = stale.destroy();
        log::info!(
            "[android-native] ensure_main_webview_window: destroy() requested for stale main window ok={}",
            destroyed.is_ok()
        );
        const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
        const POLL_ATTEMPTS: u32 = 10; // ~200ms total worst case
        let mut still_present = app.get_webview_window("main").is_some();
        let mut attempts = 0;
        while still_present && attempts < POLL_ATTEMPTS {
            std::thread::sleep(POLL_INTERVAL);
            still_present = app.get_webview_window("main").is_some();
            attempts += 1;
        }
        if still_present {
            // Most likely the same still-unexplained case where the stale
            // window's webview was itself never actually created in the
            // first place — there may be no real platform teardown for
            // destroy() to ever complete, so no amount of waiting here
            // would help. Surfaced as an error rather than silently
            // continuing into a doomed build() call; the caller's own
            // stuck-instance watchdog is what actually recovers from this.
            log::warn!(
                "[android-native] ensure_main_webview_window: main window record still present after {}ms; giving up",
                POLL_ATTEMPTS * POLL_INTERVAL.as_millis() as u32
            );
            return Err("stale main window did not clear in time".to_string());
        } else if attempts > 0 {
            log::info!(
                "[android-native] ensure_main_webview_window: stale main window cleared after {}ms",
                attempts * POLL_INTERVAL.as_millis() as u32
            );
        }
    }
    let result =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .build();
    log::info!("[android-native] ensure_main_webview_window: build ok={}", result.is_ok());
    result
        .map(|_| ())
        .map_err(|error| format!("rebuild main webview window: {error}"))
}

pub fn notify_capsule_state(payload: &CapsulePayload) {
    #[cfg(target_os = "android")]
    {
        let state = capsule_state_name(payload.state);
        let message = payload.message.as_deref();
        // #region agent log
        let has_ctx = crate::android::jni::android::has_active_activity();
        log::warn!(
            "[OpenLessDbg58c22b] {{\"sessionId\":\"58c22b\",\"hypothesisId\":\"A\",\"location\":\"native_bridge::notify_capsule_state\",\"message\":\"notify attempt\",\"data\":{{\"state\":\"{state}\",\"hasCtx\":{has_ctx}}},\"timestamp\":{}}}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        // #endregion
        if let Err(error) = crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::notify_overlay_bridge(
                env,
                context,
                state,
                message,
                payload.level,
            )
        }) {
            log::warn!("[android-native] notify overlay bridge failed: {error}");
            // #region agent log
            log::warn!(
                "[OpenLessDbg58c22b] {{\"sessionId\":\"58c22b\",\"hypothesisId\":\"A\",\"location\":\"native_bridge::notify_capsule_state\",\"message\":\"notify failed\",\"data\":{{\"error\":\"{error}\",\"state\":\"{state}\"}},\"timestamp\":{}}}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            // #endregion
        }
    }
    let _ = payload;
}

pub fn show_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            show_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn hide_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            hide_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn replace_overlay() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            replace_overlay_with_context(env, context)
        })?;
    }
    Ok(())
}

pub fn refresh_overlay_layout() -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::start_service_action(
                env,
                context,
                "com.openless.app.OpenLessOverlayService",
                "com.openless.app.overlay.REFRESH_LAYOUT",
            )
        })?;
    }
    Ok(())
}

pub fn refresh_overlay_if_visible() -> Result<(), String> {
    if is_overlay_visible() {
        refresh_overlay_layout()
    } else {
        Ok(())
    }
}

#[cfg(target_os = "android")]
fn show_overlay_with_context(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Result<(), String> {
    crate::android::jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.SHOW",
    )?;
    OVERLAY_VISIBLE.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(target_os = "android")]
fn hide_overlay_with_context(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Result<(), String> {
    crate::android::jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.HIDE",
    )?;
    OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(target_os = "android")]
fn replace_overlay_with_context(
    env: &mut jni::JNIEnv,
    context: &jni::objects::JObject,
) -> Result<(), String> {
    crate::android::jni::android::start_service_action(
        env,
        context,
        "com.openless.app.OpenLessOverlayService",
        "com.openless.app.overlay.REPLACE_OVERLAY",
    )?;
    OVERLAY_VISIBLE.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

pub fn is_overlay_visible() -> bool {
    OVERLAY_VISIBLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Kotlin overlay service 的 onDestroy() 调用此函数，以便在 OS 杀死服务时
/// 同步清除 OVERLAY_VISIBLE 标志，避免 refresh_overlay_if_visible() 向死亡
/// 服务发送无效命令。
pub fn notify_overlay_destroyed() {
    OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
    log::info!("[android-native] overlay service destroyed — OVERLAY_VISIBLE reset");
}

fn spawn_start_dictation(translation: bool) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        let result = start_core_dictation(&backend, translation).await;
        if let Err(error) = result {
            log::warn!(
                "[android-native] {} failed: {error}",
                if translation {
                    "start_dictation_with_translation"
                } else {
                    "start_dictation"
                }
            );
        }
    });
}

fn spawn_start_dictation_for_ime() {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = start_core_dictation_for_ime(&backend).await {
            log::warn!("[android-native] start_dictation_for_ime failed: {error}");
        }
    });
}

fn spawn_stop_dictation() {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = stop_core_dictation(&backend, None, None).await {
            log::warn!("[android-native] stop_dictation failed: {error}");
        }
    });
}

#[cfg(target_os = "android")]
fn spawn_stop_dictation_for_ime() {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        match stop_core_dictation(&backend, None, None).await {
            Ok(result) => {
                let text = result.polished_text;
                let _ = crate::android::jni::android::with_android_env(|env, context| {
                    crate::android::jni::android::notify_ime_text(env, context, &text)
                });
            }
            Err(error) => log::warn!("[android-native] stop_dictation_for_ime failed: {error}"),
        }
    });
}

fn spawn_stop_dictation_with_translation(translation: bool) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = stop_core_dictation(&backend, Some(translation), None).await {
            log::warn!("[android-native] stop_dictation_with_translation failed: {error}");
        }
    });
}

/// IME keyboard's mic-button swipe-up gesture: same "insert-as-you-go" path
/// as spawn_stop_dictation_for_ime(), but with the ASR-only override armed
/// (see DictationContext::with_raw_requested()'s doc comment) — the LLM
/// polish step is skipped entirely for this utterance and the raw
/// transcript is inserted as-is.
#[cfg(target_os = "android")]
fn spawn_stop_dictation_for_ime_with_raw(raw: bool) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        match stop_core_dictation(&backend, None, Some(raw)).await {
            Ok(result) => {
                let text = result.polished_text;
                let _ = crate::android::jni::android::with_android_env(|env, context| {
                    crate::android::jni::android::notify_ime_text(env, context, &text)
                });
            }
            Err(error) => log::warn!("[android-native] stop_dictation_for_ime_with_raw failed: {error}"),
        }
    });
}

fn spawn_stop_dictation_as_quick_note() {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = stop_core_dictation_as_quick_note(&backend).await {
            log::warn!("[android-native] stop_quick_note failed: {error}");
        }
    });
}

fn spawn_cancel_dictation() {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = cancel_core_dictation(&backend).await {
            log::warn!("[android-native] cancel_dictation failed: {error}");
        }
    });
}

/// Records a spoken correction as a global Dictionary entry instead of a
/// CorrectionRule — the IME's "edit result"/clipboard-correction flows used
/// to auto-generate a pattern->replacement rule from every edit; that's now
/// hand-maintained only (desktop's Corrections settings page), and these
/// flows just remember the corrected word/phrase itself, the same
/// mechanism `add_vocab` (commands/dictionary.rs) exposes to the desktop
/// Dictionary UI. `add_vocabulary_if_absent` skips silently if the phrase
/// is already known, so repeatedly correcting the same word never piles up
/// duplicate entries.
fn spawn_add_vocabulary_word(phrase: String) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    if phrase.trim().is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = backend.add_vocabulary_if_absent(phrase, None) {
            log::warn!("[android-native] add_vocabulary_if_absent failed: {error}");
        }
    });
}

/// Records a correction rule from the IME's "edit result" flow, so a
/// misrecognition the user just fixed by hand also gets fixed automatically
/// for future dictations. Uses the same CorrectionRuleStore desktop's
/// Corrections settings page writes to. Kept alongside
/// spawn_add_vocabulary_word() above rather than replaced by it — this
/// branch's own IME flows write to the Dictionary now, but upstream/beta's
/// other call sites still rely on the CorrectionRule store.
fn spawn_add_correction_rule(pattern: String, replacement: String) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    if pattern.is_empty() || replacement.is_empty() || pattern == replacement {
        return;
    }
    tauri::async_runtime::spawn(async move {
        // Idempotent by pattern: a rule for this exact wrong text should
        // never exist twice. The clipboard swipe UI already gates "add" to
        // only fire when no rule exists yet for that text, but that is only
        // a UI-level hint — this is also reachable from the dictation "edit
        // result" flow, so the actual duplicate-prevention guarantee belongs
        // here, not in either caller.
        match backend.list_correction_rules() {
            Ok(existing) => {
                for rule in existing.into_iter().filter(|rule| rule.pattern == pattern) {
                    if let Err(error) = backend.remove_correction_rule(&rule.id) {
                        log::warn!(
                            "[android-native] remove stale correction rule before re-add failed: {error}"
                        );
                    }
                }
            }
            Err(error) => {
                log::warn!("[android-native] list_correction_rules before add failed: {error}");
            }
        }
        if let Err(error) = backend.add_correction_rule(pattern, replacement) {
            log::warn!("[android-native] add_correction_rule failed: {error}");
        }
    });
}

/// Every existing correction rule's pattern, so the clipboard swipe-left
/// gesture can show "add" vs. "remove" before the user finishes the drag.
/// Synchronous rather than spawned: list_correction_rules() is just a
/// mutex + small-file read, and the caller needs the answer to render the
/// zone label, not on some later callback.
fn correction_rule_patterns_json() -> String {
    let Some(backend) = CORE_BACKEND.get() else {
        return "[]".to_string();
    };
    match backend.list_correction_rules() {
        Ok(rules) => {
            let patterns: Vec<&str> = rules.iter().map(|rule| rule.pattern.as_str()).collect();
            serde_json::to_string(&patterns).unwrap_or_else(|_| "[]".to_string())
        }
        Err(error) => {
            log::warn!("[android-native] list_correction_rules failed: {error}");
            "[]".to_string()
        }
    }
}

/// Every existing Dictionary entry's phrase — same purpose as
/// correction_rule_patterns_json() above, for the clipboard swipe-left
/// zone's "add"/"remove" label and the row's "✎" marker, now that adding
/// from the IME writes to the Dictionary instead of a CorrectionRule.
fn vocabulary_phrases_json() -> String {
    let Some(backend) = CORE_BACKEND.get() else {
        return "[]".to_string();
    };
    match backend.list_vocabulary() {
        Ok(entries) => {
            let phrases: Vec<&str> = entries.iter().map(|entry| entry.phrase.as_str()).collect();
            serde_json::to_string(&phrases).unwrap_or_else(|_| "[]".to_string())
        }
        Err(error) => {
            log::warn!("[android-native] list_vocabulary failed: {error}");
            "[]".to_string()
        }
    }
}

/// Removes every Dictionary entry whose phrase exactly matches — the
/// clipboard swipe-left "remove" action's counterpart to
/// spawn_add_vocabulary_word() above. Idempotent: no match is a silent
/// no-op, same as remove_vocabulary(id) itself.
fn spawn_remove_vocabulary_word(phrase: String) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    if phrase.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let ids: Vec<String> = match backend.list_vocabulary() {
            Ok(entries) => entries
                .into_iter()
                .filter(|entry| entry.phrase == phrase)
                .map(|entry| entry.id)
                .collect(),
            Err(error) => {
                log::warn!("[android-native] list_vocabulary for remove failed: {error}");
                return;
            }
        };
        for id in ids {
            if let Err(error) = backend.remove_vocabulary(&id) {
                log::warn!("[android-native] remove_vocabulary failed: {error}");
            }
        }
    });
}

/// Removes every correction rule whose pattern exactly matches — the
/// clipboard swipe-left "remove" action. Idempotent like the underlying
/// store's remove(id): no match is a silent no-op. Kept alongside
/// spawn_remove_vocabulary_word() above for the same reason
/// spawn_add_correction_rule() is kept alongside spawn_add_vocabulary_word().
fn spawn_remove_correction_rule(pattern: String) {
    let Some(backend) = CORE_BACKEND.get().cloned() else {
        log::warn!("[android-native] core backend unavailable");
        return;
    };
    if pattern.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let ids: Vec<String> = match backend.list_correction_rules() {
            Ok(rules) => rules
                .into_iter()
                .filter(|rule| rule.pattern == pattern)
                .map(|rule| rule.id)
                .collect(),
            Err(error) => {
                log::warn!("[android-native] list_correction_rules for remove failed: {error}");
                return;
            }
        };
        for id in ids {
            if let Err(error) = backend.remove_correction_rule(&id) {
                log::warn!("[android-native] remove_correction_rule failed: {error}");
            }
        }
    });
}

async fn ensure_core_started(backend: &OpenLessBackend) -> Result<(), BackendError> {
    if !backend.snapshot().running {
        backend.start().await?;
    }
    Ok(())
}

async fn start_core_dictation(
    backend: &OpenLessBackend,
    translation: bool,
) -> Result<(), BackendError> {
    ensure_core_started(backend).await?;
    backend
        .start_dictation_with_options(DictationStartOptions {
            translation_requested: translation,
            output_target: openless_core::DictationOutputTarget::Undecided,
            ..DictationStartOptions::default()
        })
        .await
        .map(|_| ())
}

async fn start_core_dictation_for_ime(
    backend: &OpenLessBackend,
) -> Result<(), BackendError> {
    ensure_core_started(backend).await?;
    backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            ..DictationStartOptions::default()
        })
        .await
        .map(|_| ())
}

async fn stop_core_dictation(
    backend: &OpenLessBackend,
    translation: Option<bool>,
    raw: Option<bool>,
) -> Result<openless_core::DictationResult, BackendError> {
    ensure_core_started(backend).await?;
    backend
        .stop_dictation_with_options(DictationStopOptions {
            translation_requested: translation,
            raw_requested: raw,
            quick_note: Some(false),
        })
        .await
}

async fn stop_core_dictation_as_quick_note(backend: &OpenLessBackend) -> Result<(), BackendError> {
    ensure_core_started(backend).await?;
    backend
        .stop_dictation_with_options(DictationStopOptions {
            translation_requested: None,
            raw_requested: None,
            quick_note: Some(true),
        })
        .await
        .map(|_| ())
}

async fn cancel_core_dictation(backend: &OpenLessBackend) -> Result<(), BackendError> {
    match backend.cancel_dictation(None).await {
        Err(error) if error.code == BackendErrorCode::InvalidState => Ok(()),
        result => result,
    }
}

fn spawn_switch_style_pack() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    coordinator.switch_to_previous_style_pack();
}

fn spawn_finalize_qa_from_overlay() {
    let Some(coordinator) = COORDINATOR.get().cloned() else {
        log::warn!("[android-native] coordinator unavailable");
        return;
    };
    log::info!("[android-native] finalize_qa_from_overlay requested");
    tauri::async_runtime::spawn(async move {
        if let Err(error) = coordinator.finalize_qa_from_overlay().await {
            log::warn!("[android-native] finalize_qa_from_overlay failed: {error}");
        }
    });
}

fn capsule_state_name(state: CapsuleState) -> &'static str {
    match state {
        CapsuleState::Idle => "idle",
        CapsuleState::Recording => "recording",
        CapsuleState::Transcribing => "transcribing",
        CapsuleState::Polishing => "polishing",
        CapsuleState::Done => "done",
        CapsuleState::Cancelled => "cancelled",
        CapsuleState::Error => "error",
    }
}

#[cfg(target_os = "android")]
mod jni_exports {
    use super::*;
    use jni::objects::{JClass, JObject, JString};
    use jni::sys::{jboolean, jstring, JNIEnv};
    use jni::JNIEnv as JniEnv;

    unsafe fn with_jni_context<R>(
        env_ptr: *mut JNIEnv,
        context: JObject,
        f: impl for<'local> FnOnce(&mut JniEnv<'local>, &JObject<'local>) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut env =
            JniEnv::from_raw(env_ptr).map_err(|error| format!("attach JNI env: {error}"))?;
        f(&mut env, &context)
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStartDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_start_dictation(false);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStartDictationForIme(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_start_dictation_for_ime();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStartDictationWithTranslation(
        _env: *mut JNIEnv,
        _class: JClass,
        translation: jboolean,
    ) {
        spawn_start_dictation(translation != 0);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_stop_dictation();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictationForIme(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_stop_dictation_for_ime();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictationForImeWithRaw(
        _env: *mut JNIEnv,
        _class: JClass,
        raw: jboolean,
    ) {
        spawn_stop_dictation_for_ime_with_raw(raw != 0);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictationWithTranslation(
        _env: *mut JNIEnv,
        _class: JClass,
        translation: jboolean,
    ) {
        spawn_stop_dictation_with_translation(translation != 0);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeStopDictationAsQuickNote(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_stop_dictation_as_quick_note();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeCancelDictation(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_cancel_dictation();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeAddVocabularyWord(
        env: *mut JNIEnv,
        _class: JClass,
        phrase: jstring,
    ) {
        let mut jni_env = match JniEnv::from_raw(env) {
            Ok(env) => env,
            Err(error) => {
                log::warn!("[android-native] attach JNI env for add_vocabulary_word failed: {error}");
                return;
            }
        };
        let phrase_str: String = jni_env
            .get_string(&JString::from_raw(phrase))
            .map(|value| value.into())
            .unwrap_or_default();
        spawn_add_vocabulary_word(phrase_str);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeAddCorrectionRule(
        env: *mut JNIEnv,
        _class: JClass,
        pattern: jstring,
        replacement: jstring,
    ) {
        let mut jni_env = match JniEnv::from_raw(env) {
            Ok(env) => env,
            Err(error) => {
                log::warn!(
                    "[android-native] attach JNI env for add_correction_rule failed: {error}"
                );
                return;
            }
        };
        let pattern_str: String = jni_env
            .get_string(&JString::from_raw(pattern))
            .map(|value| value.into())
            .unwrap_or_default();
        let replacement_str: String = jni_env
            .get_string(&JString::from_raw(replacement))
            .map(|value| value.into())
            .unwrap_or_default();
        spawn_add_correction_rule(pattern_str, replacement_str);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeCorrectionRulePatterns(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let response = correction_rule_patterns_json();
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android::jni::android::export_jstring(&mut env, &response),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeVocabularyPhrases(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let response = vocabulary_phrases_json();
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android::jni::android::export_jstring(&mut env, &response),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeRemoveVocabularyWord(
        env: *mut JNIEnv,
        _class: JClass,
        phrase: jstring,
    ) {
        let mut jni_env = match JniEnv::from_raw(env) {
            Ok(env) => env,
            Err(error) => {
                log::warn!("[android-native] attach JNI env for remove_vocabulary_word failed: {error}");
                return;
            }
        };
        let phrase_str: String = jni_env
            .get_string(&JString::from_raw(phrase))
            .map(|value| value.into())
            .unwrap_or_default();
        spawn_remove_vocabulary_word(phrase_str);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeRemoveCorrectionRule(
        env: *mut JNIEnv,
        _class: JClass,
        pattern: jstring,
    ) {
        let mut jni_env = match JniEnv::from_raw(env) {
            Ok(env) => env,
            Err(error) => {
                log::warn!(
                    "[android-native] attach JNI env for remove_correction_rule failed: {error}"
                );
                return;
            }
        };
        let pattern_str: String = jni_env
            .get_string(&JString::from_raw(pattern))
            .map(|value| value.into())
            .unwrap_or_default();
        spawn_remove_correction_rule(pattern_str);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeBackendSnapshot(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let response = android_backend_snapshot_response(CORE_BACKEND.get().map(Arc::as_ref));
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android::jni::android::export_jstring(&mut env, &response),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeSwitchStylePack(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_switch_style_pack();
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeFinalizeQaFromOverlay(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        spawn_finalize_qa_from_overlay();
    }

    // Registered from OpenLessRuntimeService.onCreate()/onDestroy() (a
    // foreground Service that starts/stops in lockstep with the IME being
    // active) so with_android_env() (every Rust->Kotlin JNI call, including
    // the dictation/waveform capsule notifications) has a Context valid for
    // that Service's whole lifecycle. This was previously registered from
    // OpenLessBackendWarmupActivity instead — an Activity that spends
    // nearly all its life backgrounded via moveTaskToBack() and can be
    // reclaimed by the OS at any point during that, which is exactly one of
    // the two failure modes this mechanism was already built to avoid (see
    // android::jni::android::ACTIVE_CONTEXT's doc comment) — it just hadn't
    // been pointed at a registrant stable enough to actually avoid it.
    // register_active_activity()/unregister_active_activity() only ever
    // stored a generic JObject, so this parameter never actually needed to
    // be an Activity specifically.
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeRegisterActivityContext(
        env: *mut JNIEnv,
        _class: JClass,
        context: JObject,
    ) {
        let result = with_jni_context(env, context, |env, context| {
            crate::android::jni::android::register_active_activity(env, context)
        });
        if let Err(error) = result {
            log::warn!("[android-native] register activity context failed: {error}");
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeUnregisterActivityContext(
        env: *mut JNIEnv,
        _class: JClass,
        context: JObject,
    ) {
        let _ = with_jni_context(env, context, |env, context| {
            crate::android::jni::android::unregister_active_activity(env, context);
            Ok(())
        });
    }

    /// Lets ensureBackendReady() tell "backend healthy but no Activity
    /// left to notify" apart from "backend actually cold" — see
    /// android::jni::android::has_active_activity()'s doc comment.
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeHasRegisteredActivityContext(
        _env: *mut JNIEnv,
        _class: JClass,
    ) -> jboolean {
        crate::android::jni::android::has_active_activity() as jboolean
    }

    /// 供 Kotlin overlay service 的 onDestroy() 调用，将 OVERLAY_VISIBLE 清除。
    /// 解决 OS 杀死服务时 Rust 端状态永久失同步的问题。
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeNotifyOverlayDestroyed(
        _env: *mut JNIEnv,
        _class: JClass,
    ) {
        notify_overlay_destroyed();
    }

    /// Called from OpenLessBackendWarmupActivity.onCreate() on every fresh
    /// instance — see ensure_main_webview_window()'s doc comment for why
    /// this is what actually fixes the settings-reopen black screen.
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeEnsureMainWebviewWindow(
        _env: *mut JNIEnv,
        _class: JClass,
    ) -> jboolean {
        match super::ensure_main_webview_window() {
            Ok(()) => crate::android::jni::android::export_jboolean(true),
            Err(error) => {
                log::warn!("[android-native] ensure_main_webview_window failed: {error}");
                crate::android::jni::android::export_jboolean(false)
            }
        }
    }

    /// Settings export/import (OpenLessKeyboardSettingsActivity's "导出/导入配置")
    /// — see export_preferences_subset_json()/import_preferences_subset_json()'s
    /// own doc comments. Synchronous (unlike the dictation lifecycle calls
    /// above): these are plain file reads/writes on a background settings
    /// screen, not something already running on a worker thread of its own,
    /// so there's no separate spawn_*() wrapper to hand off to.
    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeExportPreferencesSubset(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let response = export_preferences_subset_json();
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android::jni::android::export_jstring(&mut env, &response),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeImportPreferencesSubset(
        env: *mut JNIEnv,
        _class: JClass,
        json: jstring,
    ) {
        let mut jni_env = match JniEnv::from_raw(env) {
            Ok(env) => env,
            Err(error) => {
                log::warn!("[android-native] attach JNI env for import_preferences_subset failed: {error}");
                return;
            }
        };
        let json_str: String = jni_env
            .get_string(&JString::from_raw(json))
            .map(|value| value.into())
            .unwrap_or_default();
        import_preferences_subset_json(&json_str);
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeExportCredentialsSnapshot(
        env: *mut JNIEnv,
        _class: JClass,
    ) -> jstring {
        let response = export_credentials_snapshot_json();
        match JniEnv::from_raw(env) {
            Ok(mut env) => crate::android::jni::android::export_jstring(&mut env, &response),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn Java_com_openless_app_OpenLessNative_nativeImportCredentialsSnapshot(
        env: *mut JNIEnv,
        _class: JClass,
        json: jstring,
    ) {
        let mut jni_env = match JniEnv::from_raw(env) {
            Ok(env) => env,
            Err(error) => {
                log::warn!("[android-native] attach JNI env for import_credentials_snapshot failed: {error}");
                return;
            }
        };
        let json_str: String = jni_env
            .get_string(&JString::from_raw(json))
            .map(|value| value.into())
            .unwrap_or_default();
        import_credentials_snapshot_json(&json_str);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::testing::{
        FixtureDictationEngine, FixtureEngineAction, FixtureTextInserter, RecordingHostActions,
    };
    use openless_core::{
        BackendConfig, BackendDependencies, BackendServices, DictationPhase,
        InMemoryCredentialStore, InsertOutcome, TokioTaskSpawner,
    };

    #[test]
    fn android_snapshot_envelope_always_carries_contract_version() {
        let value: serde_json::Value =
            serde_json::from_str(&android_backend_snapshot_response(None)).unwrap();
        assert_eq!(
            value["contractVersion"],
            openless_core::BACKEND_CONTRACT_VERSION
        );
        assert_eq!(value["ok"], false);
        assert!(value["payload"].is_null());
        assert_eq!(value["error"], "backend unavailable");
    }

    #[tokio::test]
    async fn android_dictation_bridge_uses_core_and_preserves_stop_time_translation() {
        let data_dir = std::env::temp_dir().join(format!(
            "openless-android-core-bridge-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let engine = FixtureDictationEngine::successful("raw", "translated");
        let backend = OpenLessBackend::new(
            BackendConfig {
                data_dir: data_dir.clone(),
                ..BackendConfig::default()
            },
            BackendDependencies {
                host_actions: Arc::new(RecordingHostActions::default()),
                text_inserter: Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted)),
                dictation_engine: Arc::new(engine.clone()),
                task_spawner: Arc::new(TokioTaskSpawner),
                credential_store: Arc::new(InMemoryCredentialStore::default()),
                services: BackendServices::unsupported(),
                local_asr_runtime: None,
                selection_runtime: None,
                selection_polisher: None,
                qa_runtime: None,
                marketplace_config: None,
            },
        )
        .unwrap();
        let not_ready: serde_json::Value =
            serde_json::from_str(&android_backend_snapshot_response(Some(&backend))).unwrap();
        assert_eq!(not_ready["ok"], false);
        assert_eq!(not_ready["error"], "backend is not running");
        let mut preferences = backend.get_preferences();
        preferences.translation_target_language = "English".to_string();
        preferences.working_languages = vec!["简体中文".to_string()];
        crate::set_backend_preferences_for_test(&backend, preferences);

        start_core_dictation(&backend, false).await.unwrap();
        let ready: serde_json::Value =
            serde_json::from_str(&android_backend_snapshot_response(Some(&backend))).unwrap();
        assert_eq!(ready["ok"], true);
        let translated_session = backend.snapshot().dictation.session_id.unwrap();
        stop_core_dictation(&backend, Some(true), None).await.unwrap();
        start_core_dictation(&backend, true).await.unwrap();
        let cancelled_session = backend.snapshot().dictation.session_id.unwrap();
        cancel_core_dictation(&backend).await.unwrap();

        assert_eq!(backend.snapshot().dictation.phase, DictationPhase::Idle);
        assert_eq!(
            engine.actions(),
            vec![
                FixtureEngineAction::Start(translated_session),
                FixtureEngineAction::UpdateContext(translated_session),
                FixtureEngineAction::Finish(translated_session),
                FixtureEngineAction::Start(cancelled_session),
                FixtureEngineAction::Cancel(cancelled_session),
            ]
        );
        let contexts = engine.contexts();
        assert!(!contexts[0].polish.translation_active);
        assert!(contexts[1].polish.translation_active);
        assert!(contexts[2].polish.translation_active);

        backend.shutdown().await.unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
