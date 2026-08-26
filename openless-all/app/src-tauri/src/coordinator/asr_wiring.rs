//! ASR engine wiring, credential/permission gates, and release scheduling
//! extracted from `coordinator.rs` (behavior-preserving move).
//!
//! References parent items via `use super::*;`; `pub(super)` so the parent
//! `coordinator` module reaches them through `use asr_wiring::*;`.

use super::*;

#[cfg(any(debug_assertions, test))]
pub(super) fn hotkey_injection_dry_run_enabled() -> bool {
    std::env::var_os("OPENLESS_HOTKEY_INJECTION_DRY_RUN").is_some()
}

#[cfg(any(debug_assertions, test))]
pub(super) fn debug_transcript_override_text() -> Option<String> {
    let path = std::env::var_os("OPENLESS_DEBUG_TRANSCRIPT_FILE")?;
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub(super) fn ensure_microphone_permission(_inner: &Arc<Inner>) -> Result<(), String> {
    use crate::permissions::{self, PermissionStatus};

    #[cfg(target_os = "windows")]
    {
        if permissions::windows_microphone_access_explicitly_denied() {
            return Err("需要麦克风权限，当前状态: Denied".to_string());
        }
        // 注册表只反映隐私开关；没插麦克风时不能当成“已就绪”，
        // 否则用户会被误导去系统设置找不存在的麦克风权限。见 issue #779。
        if permissions::has_microphone_input_device() {
            return Ok(());
        }
        return Err("未检测到麦克风，请连接麦克风后重试".to_string());
    }

    let status = permissions::check_microphone();
    if matches!(
        status,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        return Ok(());
    }
    if status == PermissionStatus::NoDevice {
        return Err("未检测到麦克风，请连接麦克风后重试".to_string());
    }

    // 听写路径不抢前台焦点：缺 mic 权限时直接请求系统授权，不再先 show_main_window。
    // 用户在设置页手动点“请求权限”仍走 request_microphone_from_foreground，那是显式操作。
    // 这里若系统不弹框，后续会通过 capsule error 引导用户主动去权限页处理。详见 #166。
    let requested = permissions::request_microphone();
    if matches!(
        requested,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        Ok(())
    } else {
        Err(format!("需要麦克风权限，当前状态: {requested:?}"))
    }
}

pub(super) fn ensure_asr_credentials() -> Result<(), String> {
    let active_asr = CredentialsVault::get_active_asr();

    // 本地 Qwen3-ASR 没有"凭据"概念，但需要：(a) 当前渠道的后端可用 (b) 模型已下载。
    if crate::asr::local::is_local_qwen3(&active_asr) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            if crate::asr::local::qwen_backend_for_provider(&active_asr).is_none() {
                return Err(format!("本地 Qwen3-ASR 渠道 {active_asr} 不支持当前系统"));
            }
            return ensure_local_qwen3_model_ready();
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            return Err(
                "本地 Qwen3-ASR C 后端目前支持 macOS/Linux；MLX 后端仅支持 macOS".to_string(),
            );
        }
    }

    if crate::asr::local::is_local_whisper(&active_asr) {
        #[cfg(not(target_os = "macos"))]
        {
            return Err("本地 Whisper 当前仅支持 macOS".to_string());
        }
        #[cfg(target_os = "macos")]
        {
            return ensure_local_whisper_model_ready();
        }
    }

    if crate::asr::local::is_apple_speech(&active_asr) {
        #[cfg(not(target_os = "macos"))]
        {
            return Err("Apple Speech 当前仅支持 macOS".to_string());
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(());
        }
    }

    if crate::asr::local::foundry::is_foundry_local_whisper(&active_asr) {
        #[cfg(not(target_os = "windows"))]
        {
            return Err("Foundry Local Whisper 当前仅支持 Windows".to_string());
        }
        #[cfg(target_os = "windows")]
        {
            return Ok(());
        }
    }

    if crate::asr::local::sherpa::is_sherpa_onnx_local(&active_asr) {
        #[cfg(not(target_os = "windows"))]
        {
            return Err("sherpa-onnx local ASR 当前仅支持 Windows".to_string());
        }
        #[cfg(target_os = "windows")]
        {
            return Ok(());
        }
    }

    // `openai-compatible` 通用预设没有厂商默认值：endpoint 与 model 必须由用户
    // 填写，缺一即明确报错（不再静默回落 whisper-1）。API Key 允许留空——
    // LAN 自建端点（llama.cpp 等）常无需鉴权，故直接跳过下方 AsrApiKey 检查。
    if active_asr == OPENAI_COMPATIBLE_ASR_PROVIDER_ID {
        let endpoint = CredentialsVault::get(CredentialAccount::AsrEndpoint)
            .ok()
            .flatten()
            .unwrap_or_default();
        let model = CredentialsVault::get(CredentialAccount::AsrModel)
            .ok()
            .flatten()
            .unwrap_or_default();
        return require_openai_compatible_fields(&endpoint, &model);
    }

    // 云端 provider 的预检凭据由 ActiveAsrProviderKind 统一判定（穷尽 match，
    // 编译器保证新增 kind 不会被漏掉 —— 取代旧的「provider 白名单 + 火山兜底」，
    // 那个静默 else 曾让新通道误落到火山分支）。
    match active_asr_provider_kind(&active_asr).preflight_credential() {
        AsrPreflightCredential::AsrApiKey => {
            let api_key = CredentialsVault::get(CredentialAccount::AsrApiKey)
                .ok()
                .flatten()
                .unwrap_or_default();
            if api_key.trim().is_empty() {
                return Err("请先在设置中填写 ASR 服务商 API Key".to_string());
            }
            Ok(())
        }
        AsrPreflightCredential::VolcAppKey => {
            use crate::asr::volcengine::VolcengineAuthMode;
            let creds = read_volc_credentials();
            // 统一走 VolcengineAuthMode::auth_ok：与 open_session / volcengine_configured
            // 共用同一份按模式判定 + trim 语义，避免三处规则漂移。
            if creds.auth_ok() {
                Ok(())
            } else {
                match creds.auth_mode {
                    VolcengineAuthMode::AppIdToken => {
                        Err("请先在设置中填写火山引擎 ASR App Key 和 Access Key".to_string())
                    }
                    VolcengineAuthMode::ApiKey => {
                        Err("请先在设置中填写豆包语音新版控制台 API Key".to_string())
                    }
                }
            }
        }
        AsrPreflightCredential::XfyunAppKey => {
            let creds = read_xfyun_credentials();
            if creds.auth_ok() {
                Ok(())
            } else {
                Err("请先在设置中填写讯飞 AppID 和 API Key".to_string())
            }
        }
    }
}

/// `openai-compatible` 预设的必填字段校验：endpoint / model 均须非空（trim），
/// 返回明确的中文错误。API Key 是否必填由调用方决定（本预设允许留空）。
pub(super) fn require_openai_compatible_fields(endpoint: &str, model: &str) -> Result<(), String> {
    if endpoint.trim().is_empty() {
        return Err("自定义 OpenAI 兼容 ASR：请先在设置中填写服务端地址（endpoint）".to_string());
    }
    if model.trim().is_empty() {
        return Err("自定义 OpenAI 兼容 ASR：请先在设置中填写模型名（model）".to_string());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn is_keyless_local_asr_provider(id: &str) -> bool {
    if crate::asr::local::is_local_qwen3(id) {
        return crate::asr::local::qwen_backend_for_provider(id).is_some();
    }
    #[cfg(target_os = "macos")]
    if crate::asr::local::is_apple_speech(id) {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        crate::asr::local::foundry::is_foundry_local_whisper(id)
            || crate::asr::local::sherpa::is_sherpa_onnx_local(id)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        false
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn ensure_local_qwen3_model_ready() -> Result<(), String> {
    let prefs = || -> Result<crate::types::UserPreferences, String> {
        // 这里没法拿到 inner，直接读 preferences.json 即可（Coordinator 写盘后总是同步的）。
        crate::persistence::PreferencesStore::new()
            .map_err(|e| e.to_string())
            .map(|s| s.get())
    }()?;
    let model_id = crate::asr::local::ModelId::from_str(&prefs.local_asr_active_model)
        .ok_or_else(|| format!("未知的本地模型 id: {}", prefs.local_asr_active_model))?;
    if !model_id.is_qwen() {
        return Err(format!(
            "当前模型 {} 不属于本地 Qwen3-ASR",
            model_id.as_str()
        ));
    }
    if !crate::asr::local::models::is_downloaded(model_id) {
        return Err(format!(
            "本地模型 {} 未下载完整，请到 设置 → 模型设置 中下载",
            model_id.as_str()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn ensure_local_whisper_model_ready() -> Result<(), String> {
    let model_id = crate::persistence::PreferencesStore::new()
        .map(|store| store.get().local_whisper_active_model)
        .ok()
        .filter(|id| {
            crate::asr::local::ModelId::from_str(id)
                .map(|model| model.is_whisper())
                .unwrap_or(false)
        })
        .unwrap_or_else(|| crate::asr::local::WHISPER_MODEL_ID.to_string());
    if crate::asr::local::whisper_model_ready_for_model(&model_id) {
        return Ok(());
    }
    let path =
        crate::asr::local::whisper_model_path_for_model(&model_id).map_err(|e| e.to_string())?;
    Err(format!(
        "本地 Whisper 模型 {} 不存在，请到 设置 → 本地模型 下载，或将模型文件放到 {}",
        model_id,
        path.display()
    ))
}

/// 引擎加载/释放/keepLoadedSecs 变化时主动推给前端，前端 listen
/// `local-asr:engine-changed` 即可零轮询同步 UI（issue #470 / #6）。
/// 反映当前选中的 Qwen3 / Whisper cache，不碰 Foundry / Sherpa。
/// 仅用桌面端跨平台符号；Android 无本地 ASR 引擎（LocalAsrEngineStatus 不在该 target
/// 编译），单独给 no-op stub（见下），让各调用点在所有平台统一编译。
#[cfg(not(target_os = "android"))]
pub(super) fn active_local_asr_loaded_model(inner: &Arc<Inner>) -> Option<String> {
    let provider = CredentialsVault::get_active_asr();
    if crate::asr::local::is_local_qwen3(&provider) {
        return inner.local_asr_cache.loaded_model_id();
    }
    #[cfg(target_os = "macos")]
    if crate::asr::local::is_local_whisper(&provider) {
        return inner.local_whisper_cache.loaded_model_id();
    }
    None
}

#[cfg(target_os = "android")]
pub(super) fn active_local_asr_loaded_model(_inner: &Arc<Inner>) -> Option<String> {
    None
}

#[cfg(not(target_os = "android"))]
pub(super) fn emit_local_asr_engine_status(inner: &Arc<Inner>) {
    let model_id = active_local_asr_loaded_model(inner);
    let keep_loaded_secs = inner.prefs.get().local_asr_keep_loaded_secs;
    let status = crate::commands::LocalAsrEngineStatus {
        loaded: model_id.is_some(),
        model_id,
        keep_loaded_secs,
    };
    if let Some(app) = inner.app.lock().clone() {
        let _ = app.emit("local-asr:engine-changed", &status);
    }
}

/// Android no-op：该 target 不编译 LocalAsrEngineStatus / 本地 ASR 引擎。issue #470 / #6。
#[cfg(target_os = "android")]
pub(super) fn emit_local_asr_engine_status(_inner: &Arc<Inner>) {}

/// 统一通过本地 ASR 生命周期门闩驱逐 Qwen / Whisper cache。
///
/// `spawn_blocking` 被 timeout 或取消时，单纯丢弃 future 不会停止 native 解码。
/// MLX provider 的 operation cancel 会终止自己的隔离 worker；cache 自动驱逐不再
/// 终止其它共享会话。C / Whisper 保持原有行为，旧任务由自身持有的 `Arc` 安全收尾。
#[cfg(not(target_os = "android"))]
fn release_local_asr_engines_locked(
    inner: &Arc<Inner>,
    release_qwen: bool,
    release_whisper: bool,
    abort_qwen_in_use: bool,
) {
    if release_qwen {
        if abort_qwen_in_use {
            inner.local_asr_cache.release_now();
        } else {
            inner.local_asr_cache.evict_now();
        }
    }
    #[cfg(target_os = "macos")]
    if release_whisper {
        inner.local_whisper_cache.release_now();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = release_whisper;
}

#[cfg(not(target_os = "android"))]
pub(super) fn release_local_asr_engines_now(
    inner: &Arc<Inner>,
    release_qwen: bool,
    release_whisper: bool,
) {
    let _lifecycle_guard = inner.local_asr_lifecycle.lock();
    release_local_asr_engines_locked(inner, release_qwen, release_whisper, false);
}

#[cfg(target_os = "android")]
pub(super) fn release_local_asr_engines_now(
    _inner: &Arc<Inner>,
    _release_qwen: bool,
    _release_whisper: bool,
) {
}

/// 用户主动释放、切换 provider 或删除模型时保留原有全局终止语义。
#[cfg(not(target_os = "android"))]
pub(super) fn abort_local_asr_engines_now(
    inner: &Arc<Inner>,
    release_qwen: bool,
    release_whisper: bool,
) {
    let _lifecycle_guard = inner.local_asr_lifecycle.lock();
    release_local_asr_engines_locked(inner, release_qwen, release_whisper, true);
}

#[cfg(target_os = "android")]
pub(super) fn abort_local_asr_engines_now(
    _inner: &Arc<Inner>,
    _release_qwen: bool,
    _release_whisper: bool,
) {
}

/// 一次 dictation 结束后，按 prefs.local_asr_keep_loaded_secs 决定何时释放
/// 内存里的 Qwen3-ASR 引擎。0 = 立即释放；其它值 = sleep N 秒后看 last_used。
/// 多次会话叠加多个 sleep 任务，每个独立 check：只要中间又被使用过就跳过释放。
pub(super) fn schedule_local_asr_release(inner: &Arc<Inner>) {
    let keep_secs = inner.prefs.get().local_asr_keep_loaded_secs;
    let cache = Arc::clone(&inner.local_asr_cache);
    if keep_secs == 0 {
        release_local_asr_engines_now(inner, true, false);
        emit_local_asr_engine_status(inner);
        return;
    }
    let dur = std::time::Duration::from_secs(keep_secs as u64);
    let inner = Arc::clone(inner);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(dur).await;
        let released = {
            let _lifecycle_guard = inner.local_asr_lifecycle.lock();
            cache.release_if_idle(dur)
        };
        if released {
            emit_local_asr_engine_status(&inner);
        }
    });
}

#[cfg(target_os = "macos")]
pub(super) fn schedule_local_whisper_release(inner: &Arc<Inner>) {
    let keep_secs = inner.prefs.get().local_asr_keep_loaded_secs;
    let cache = Arc::clone(&inner.local_whisper_cache);
    if keep_secs == 0 {
        release_local_asr_engines_now(inner, false, true);
        emit_local_asr_engine_status(inner);
        return;
    }
    let threshold = std::time::Duration::from_secs(keep_secs as u64);
    let inner = Arc::clone(inner);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(threshold).await;
        let released = {
            let _lifecycle_guard = inner.local_asr_lifecycle.lock();
            cache.release_if_idle(threshold)
        };
        if released {
            emit_local_asr_engine_status(&inner);
        }
    });
}

#[cfg(target_os = "windows")]
pub(super) fn foundry_local_asr_release_keep_secs(inner: &Arc<Inner>) -> u32 {
    inner.prefs.get().foundry_local_asr_keep_loaded_secs
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
pub(super) enum AsrReleaseSession {
    Dictation(SessionId),
    Qa(SessionId),
}

#[cfg(target_os = "windows")]
pub(super) fn asr_release_session_is_current(
    inner: &Arc<Inner>,
    session: AsrReleaseSession,
) -> bool {
    match session {
        AsrReleaseSession::Dictation(session_id) => inner.state.lock().session_id == session_id,
        AsrReleaseSession::Qa(session_id) => inner.qa_state.lock().session_id == session_id,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn schedule_foundry_local_asr_release(
    inner: &Arc<Inner>,
    session: AsrReleaseSession,
    primary_recovery: Option<crate::asr::local::foundry_runtime::FoundryPrimaryRecoveryToken>,
) {
    let keep_secs = foundry_local_asr_release_keep_secs(inner);
    let runtime = Arc::clone(&inner.foundry_local_runtime);
    let scheduled_epoch = runtime.route_epoch_snapshot();
    let inner = Arc::clone(inner);
    tauri::async_runtime::spawn(async move {
        let deadline = tokio::time::Instant::now()
            .checked_add(std::time::Duration::from_secs(keep_secs as u64));
        if let Some(token) = primary_recovery.as_ref() {
            if keep_secs == 0 {
                if let Err(error) = runtime.release_if_route_epoch(token.route_epoch()).await {
                    log::warn!(
                        "[foundry-asr] immediate temporary fallback cleanup failed: {error:#}"
                    );
                }
                return;
            }
            match runtime.restore_primary_for_keep_alive(token).await {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    log::warn!("[foundry-asr] background primary recovery failed: {error:#}");
                    return;
                }
            }
        }
        if let Some(deadline) = deadline {
            tokio::time::sleep_until(deadline).await;
        }
        if !asr_release_session_is_current(&inner, session) {
            return;
        }
        let release = match primary_recovery.as_ref() {
            Some(token) => runtime.release_primary_if_current(token).await.map(|_| ()),
            None => runtime
                .release_if_route_epoch(scheduled_epoch)
                .await
                .map(|_| ()),
        };
        if let Err(error) = release {
            log::warn!("[foundry-asr] scheduled release failed: {error:#}");
        }
    });
}

#[cfg(target_os = "windows")]
pub(super) fn sherpa_onnx_release_keep_secs(inner: &Arc<Inner>) -> u32 {
    inner.prefs.get().sherpa_onnx_keep_loaded_secs
}

/// 与 `schedule_foundry_local_asr_release` 同形：session_id 老旧则不释放，
/// 避免下一轮 session 立即重加载同一个 offline batch 模型。
#[cfg(target_os = "windows")]
pub(super) fn schedule_sherpa_onnx_release(inner: &Arc<Inner>, session: AsrReleaseSession) {
    let keep_secs = sherpa_onnx_release_keep_secs(inner);
    let runtime = Arc::clone(&inner.sherpa_onnx_runtime);
    let inner = Arc::clone(inner);
    tauri::async_runtime::spawn(async move {
        if keep_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(keep_secs as u64)).await;
        }
        if !asr_release_session_is_current(&inner, session) {
            return;
        }
        if let Err(error) = runtime.release_now().await {
            log::warn!("[sherpa-asr] scheduled release failed: {error:#}");
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn selected_local_qwen_target(
    inner: &Arc<Inner>,
) -> anyhow::Result<(crate::asr::local::ModelId, std::path::PathBuf)> {
    let prefs = inner.prefs.get();
    let model_id = crate::asr::local::ModelId::from_str(&prefs.local_asr_active_model)
        .filter(|id| id.is_qwen())
        .ok_or_else(|| anyhow::anyhow!("未知本地模型 id: {}", prefs.local_asr_active_model))?;
    let dir = crate::asr::local::models::model_dir(model_id)?;
    Ok((model_id, dir))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn load_current_local_qwen_engine(
    inner: &Arc<Inner>,
    backend: crate::asr::local::QwenBackend,
    model_id: &str,
    model_dir: &std::path::Path,
) -> anyhow::Result<Arc<crate::asr::local::LocalQwenEngine>> {
    let _lifecycle_guard = inner.local_asr_lifecycle.lock();
    let target_is_current = || {
        let prefs = inner.prefs.get();
        let model_matches = crate::asr::local::ModelId::from_str(&prefs.local_asr_active_model)
            .filter(|id| id.is_qwen())
            .is_some_and(|id| id.as_str() == model_id);
        crate::asr::local::qwen_backend_for_provider(&CredentialsVault::get_active_asr())
            == Some(backend)
            && model_matches
    };
    if !target_is_current() {
        anyhow::bail!("本地 Qwen3-ASR 加载目标已切换，取消旧后端加载");
    }

    let engine = inner
        .local_asr_cache
        .get_or_load(backend, model_id, model_dir)?;
    if !target_is_current() {
        drop(engine);
        release_local_asr_engines_locked(inner, true, false, true);
        anyhow::bail!("本地 Qwen3-ASR 加载期间目标已切换，丢弃旧后端");
    }
    Ok(engine)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) async fn preload_local_qwen3(
    inner: &Arc<Inner>,
    provider_id: &str,
) -> anyhow::Result<()> {
    let backend = crate::asr::local::qwen_backend_for_provider(provider_id)
        .ok_or_else(|| anyhow::anyhow!("本地 Qwen3-ASR 渠道 {provider_id} 不支持当前系统"))?;
    let (model_id, dir) = selected_local_qwen_target(inner)?;
    let model_id = model_id.as_str().to_string();
    let load_inner = Arc::clone(inner);
    tauri::async_runtime::spawn_blocking(move || {
        let engine = load_current_local_qwen_engine(&load_inner, backend, &model_id, &dir)?;
        drop(engine);
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    emit_local_asr_engine_status(inner);
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
/// 返回 (provider, 实际加载的模型 id)。模型 id 是 ModelId 校验归一后的值，调用方
/// 直接用它做历史归因（构建时快照，PR #826 review）。
pub(super) async fn build_local_qwen3(
    inner: &Arc<Inner>,
    provider_id: &str,
) -> anyhow::Result<(Arc<crate::asr::local::LocalQwenAsr>, String)> {
    let backend = crate::asr::local::qwen_backend_for_provider(provider_id)
        .ok_or_else(|| anyhow::anyhow!("本地 Qwen3-ASR 渠道 {provider_id} 不支持当前系统"))?;
    let (model_id, dir) = selected_local_qwen_target(inner)?;
    let app = inner
        .app
        .lock()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("AppHandle 未绑定"))?;
    // 走缓存：如果已有同 id 的引擎在内存里就直接复用，避免每次会话都重加载
    // 1.2GB+ 模型。第一次加载阻塞数秒，spawn_blocking 不卡 tokio runtime。
    let mid = model_id.as_str().to_string();
    let load_inner = Arc::clone(inner);
    let engine = tauri::async_runtime::spawn_blocking(move || {
        load_current_local_qwen_engine(&load_inner, backend, &mid, &dir)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    // 加载完成（含缓存命中刷新 last_used）后推一次状态，前端零轮询更新「已加载」。
    emit_local_asr_engine_status(inner);
    let model_label = model_id.as_str().to_string();
    Ok((
        Arc::new(crate::asr::local::LocalQwenAsr::new(app, engine)),
        model_label,
    ))
}

#[cfg(target_os = "macos")]
fn selected_local_whisper_target(
    inner: &Arc<Inner>,
) -> anyhow::Result<(String, std::path::PathBuf)> {
    let model_id =
        crate::asr::local::ModelId::from_str(&inner.prefs.get().local_whisper_active_model)
            .filter(|id| id.is_whisper())
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(|| crate::asr::local::WHISPER_MODEL_ID.to_string());
    let path = crate::asr::local::whisper_model_path_for_model(&model_id)?;
    Ok((model_id, path))
}

#[cfg(target_os = "macos")]
fn load_current_local_whisper_engine(
    inner: &Arc<Inner>,
    model_id: &str,
    model_path: &std::path::Path,
) -> anyhow::Result<Arc<crate::asr::local::WhisperEngine>> {
    let _lifecycle_guard = inner.local_asr_lifecycle.lock();
    let target_is_current = || {
        let prefs = inner.prefs.get();
        let model_matches = crate::asr::local::ModelId::from_str(&prefs.local_whisper_active_model)
            .filter(|id| id.is_whisper())
            .map(|id| id.as_str() == model_id)
            .unwrap_or(model_id == crate::asr::local::WHISPER_MODEL_ID);
        crate::asr::local::is_local_whisper(&CredentialsVault::get_active_asr()) && model_matches
    };
    if !target_is_current() {
        anyhow::bail!("本地 Whisper 加载目标已切换，取消旧后端加载");
    }

    let engine = inner
        .local_whisper_cache
        .get_or_load(model_id, model_path)?;
    if !target_is_current() {
        drop(engine);
        release_local_asr_engines_locked(inner, false, true, true);
        anyhow::bail!("本地 Whisper 加载期间目标已切换，丢弃旧后端");
    }
    Ok(engine)
}

#[cfg(target_os = "macos")]
pub(super) async fn preload_local_whisper(inner: &Arc<Inner>) -> anyhow::Result<()> {
    let (model_id, path) = selected_local_whisper_target(inner)?;
    let load_inner = Arc::clone(inner);
    tauri::async_runtime::spawn_blocking(move || {
        let engine = load_current_local_whisper_engine(&load_inner, &model_id, &path)?;
        drop(engine);
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    emit_local_asr_engine_status(inner);
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) async fn build_local_whisper(
    inner: &Arc<Inner>,
) -> anyhow::Result<(Arc<crate::asr::local::LocalWhisperAsr>, String)> {
    let (model_id, path) = selected_local_whisper_target(inner)?;
    let cache_model_id = model_id.clone();
    let load_inner = Arc::clone(inner);
    let engine = tauri::async_runtime::spawn_blocking(move || {
        load_current_local_whisper_engine(&load_inner, &cache_model_id, &path)
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking join failed: {e:#}"))??;
    emit_local_asr_engine_status(inner);
    let language = inner
        .prefs
        .get()
        .working_languages
        .first()
        .and_then(|name| crate::asr::local::native_name_to_apple_locale(name))
        .map(|locale| locale.split('-').next().unwrap_or("auto").to_string())
        .unwrap_or_else(|| "auto".to_string());
    Ok((
        Arc::new(crate::asr::local::LocalWhisperAsr::new(engine, language)),
        model_id,
    ))
}

#[cfg(target_os = "macos")]
pub(super) fn build_apple_speech(
    prefs: &crate::types::UserPreferences,
) -> Arc<crate::asr::local::AppleSpeechAsr> {
    // Apple 识别 locale 跟随用户工作语言主语言 —— 不显式指定 SFSpeechRecognizer 就落到
    // 系统首选语言（常是英文），中文语音会被识别成英文且理解错误。未收录语言回退默认。
    let locale = prefs
        .working_languages
        .first()
        .and_then(|name| crate::asr::local::native_name_to_apple_locale(name));
    Arc::new(crate::asr::local::AppleSpeechAsr::new(locale))
}

/// `whisper` 是 OpenAI 原生；`siliconflow` / `zhipu` / `groq` / `stepfun`
/// 都暴露 OpenAI 兼容的 `/audio/transcriptions`，统一走 `WhisperBatchASR`。
/// `openai-compatible` 是通用预设：任意 OpenAI 兼容端点（自建 / LAN llama.cpp
/// 等），无默认 endpoint/model，高级选项见 `AdvancedAsrConfig`。
/// 新增 OpenAI 兼容 ASR 时只需在这里加一项。
///
/// 注：DashScope 的 Qwen3-ASR-Flash 不在此列——它用 MultiModalConversation
/// (messages=[{content:[{audio:...}]}]) 协议，不是 Whisper multipart，需要
/// 单独 ASR 客户端，留给 V2。
pub(super) fn is_whisper_compatible_provider(id: &str) -> bool {
    matches!(
        id,
        "whisper" | "siliconflow" | "zhipu" | "groq" | "openrouter" | "stepfun" | "zenmux"
    ) || id == OPENAI_COMPATIBLE_ASR_PROVIDER_ID
}

/// 用户词典该走 `prompt` 还是一等 `hotwords` 参数。
///
/// StepFun 的 `/audio/transcriptions` **静默忽略** `prompt`（实测 2026-07：带
/// prompt 返回 200 但不参与偏置），词汇偏置走专门的 `hotwords` 字段（可解析的
/// JSON 数组字符串）。其余兼容厂商维持 Whisper 惯例的 `prompt`。
pub(super) fn whisper_uses_hotwords(provider_id: &str) -> bool {
    provider_id == "stepfun"
}

/// 词典启用词条 → (prompt, hotwords) 二选一路由，QA 与听写两处构造点共用。
/// hotwords 厂商不再拼 prompt（免得白占请求体），prompt 厂商 hotwords 恒空。
pub(super) fn whisper_vocab_for_provider(
    provider_id: &str,
    phrases: Vec<String>,
) -> (Option<String>, Vec<String>) {
    if whisper_uses_hotwords(provider_id) {
        (None, phrases)
    } else {
        (
            crate::asr::whisper::build_prompt_from_phrases(&phrases),
            Vec::new(),
        )
    }
}

/// 该 provider 的请求体编码方式。OpenRouter 的 `/audio/transcriptions` 是
/// `application/json` + base64 音频（issue #582），其余兼容厂商沿用 multipart。
/// ZenMux 同形但带 `language` / `enable_itn`（issue #837），单独走 `ZenMuxJson`。
pub(crate) fn whisper_request_format(provider_id: &str) -> crate::asr::whisper::AsrRequestFormat {
    match provider_id {
        "openrouter" => crate::asr::whisper::AsrRequestFormat::OpenRouterJson,
        "zenmux" => crate::asr::whisper::AsrRequestFormat::ZenMuxJson,
        _ => crate::asr::whisper::AsrRequestFormat::Multipart,
    }
}

/// 该 provider 的 `/audio/transcriptions` 是否支持 `response_format=verbose_json`
/// 并返回带 `no_speech_prob` / `avg_logprob` / `compression_ratio` 的 segments，
/// 用于幻听过滤。
///
/// - `whisper`（OpenAI）/ `groq`：原生 Whisper，完整支持，过滤有效。
/// - `siliconflow`：模型是 SenseVoice / TeleSpeech，文档无 `response_format`，
///   发送 verbose_json 可能被拒，**保持关闭**走旧的 `json`。
/// - `zhipu`（GLM-ASR）：虽接受 verbose_json，但不产出上述指标，过滤是空转；
///   为最小化行为变更，这里也**保持关闭**，仅对确证有收益的 whisper/groq 开启。
/// - `openai-compatible`：由用户高级配置（`AdvancedAsrConfig.verbose_json`）决定，
///   默认关闭，与服务端能力对齐。
pub(super) fn whisper_supports_verbose_json(provider_id: &str) -> bool {
    match provider_id {
        "whisper" | "groq" => true,
        // ZenMux 的 JSON 请求体协议没有 response_format，恒关闭。
        "zenmux" => false,
        // openai-compatible 由用户高级配置决定；其余厂商保持关闭。
        _ => read_advanced_asr_config(provider_id).verbose_json,
    }
}

/// OpenLess 工作语言（原生名，见前端 `SUPPORTED_LANGUAGES`）→ ZenMux `language`
/// 字段值（ISO 639-1 码）。取 `working_languages` 主语言映射；未收录的语言返回
/// None —— 请求体省略 `language`，由 ZenMux 服务端自动检测（issue #837）。
pub(super) fn zenmux_language_code(native_name: &str) -> Option<String> {
    let code = match native_name.trim() {
        "简体中文" | "繁体中文" => "zh",
        "English" => "en",
        "日本語" => "ja",
        "한국어" => "ko",
        "Français" => "fr",
        "Deutsch" => "de",
        "Español" => "es",
        "Italiano" => "it",
        "Português" => "pt",
        "Русский" => "ru",
        "العربية" => "ar",
        "Tiếng Việt" => "vi",
        "ไทย" => "th",
        "हिन्दी" => "hi",
        _ => return None,
    };
    Some(code.to_string())
}

/// 当前 prefs 的主工作语言 → ZenMux `language`（None = 不发送，自动检测）。
pub(super) fn zenmux_language_for_prefs(prefs: &crate::types::UserPreferences) -> Option<String> {
    prefs
        .working_languages
        .first()
        .and_then(|name| zenmux_language_code(name))
}

/// 构造完的 `WhisperBatchASR` 上注入 zenmux 专属选项（`language` 跟随工作语言、
/// `enable_itn` 读用户高级配置）。非 zenmux 原样返回，保持现有行为；QA 与听写
/// 两处构造点共用，避免重复逻辑。
pub(super) fn apply_zenmux_asr_options(
    builder: crate::asr::whisper::WhisperBatchASR,
    active_asr: &str,
    inner: &Arc<Inner>,
) -> crate::asr::whisper::WhisperBatchASR {
    if active_asr != ZENMUX_ASR_PROVIDER_ID {
        return builder;
    }
    builder
        .with_language(zenmux_language_for_prefs(&inner.prefs.get()))
        .with_enable_itn(read_advanced_asr_config(ZENMUX_ASR_PROVIDER_ID).enable_itn)
}

pub(super) fn is_bailian_provider(id: &str) -> bool {
    id == crate::asr::bailian::PROVIDER_ID
}

pub(super) fn is_qwen3_realtime_provider(id: &str) -> bool {
    id == crate::asr::qwen_realtime::PROVIDER_ID
}

pub(super) fn is_stepfun_realtime_provider(id: &str) -> bool {
    id == crate::asr::stepfun_realtime::PROVIDER_ID
}

pub(super) fn is_mimo_provider(id: &str) -> bool {
    id == crate::asr::mimo::PROVIDER_ID
}

pub(super) fn is_dashscope_multimodal_provider(id: &str) -> bool {
    id == crate::asr::dashscope_multimodal::PROVIDER_ID
}

pub(super) fn is_elevenlabs_provider(id: &str) -> bool {
    id == crate::asr::elevenlabs::PROVIDER_ID
}

pub(super) fn is_xfyun_provider(id: &str) -> bool {
    id == crate::asr::xfyun::PROVIDER_ID
}

pub(super) fn apply_chinese_script_preference(text: &str, pref: ChineseScriptPreference) -> String {
    if text.is_empty() {
        return String::new();
    }
    let config = match pref {
        ChineseScriptPreference::Simplified => Some(BuiltinConfig::T2s),
        ChineseScriptPreference::Traditional => Some(BuiltinConfig::S2t),
        ChineseScriptPreference::Auto => None,
    };
    let Some(config) = config else {
        return text.to_string();
    };
    match OpenCC::from_config(config) {
        Ok(converter) => converter.convert(text),
        Err(err) => {
            log::warn!("[coord] OpenCC init failed, skip script conversion: {err}");
            text.to_string()
        }
    }
}

pub(super) enum QaAsrStart {
    Volcengine {
        asr: Arc<VolcengineStreamingASR>,
        bridge: Arc<DeferredAsrBridge>,
    },
    Bailian {
        asr: Arc<BailianRealtimeASR>,
        bridge: Arc<DeferredAsrBridge>,
    },
    Qwen3Realtime {
        asr: Arc<Qwen3RealtimeASR>,
        bridge: Arc<DeferredAsrBridge>,
    },
    StepfunRealtime {
        asr: Arc<crate::asr::StepfunRealtimeASR>,
        bridge: Arc<DeferredAsrBridge>,
    },
    Xfyun {
        asr: Arc<crate::asr::XfyunStreamingASR>,
        bridge: Arc<DeferredAsrBridge>,
    },
    Ready {
        active: ActiveAsr,
        consumer: Arc<dyn crate::recorder::AudioConsumer>,
    },
}

impl QaAsrStart {
    pub(super) fn active_asr(&self) -> ActiveAsr {
        match self {
            QaAsrStart::Volcengine { asr, .. } => ActiveAsr::Volcengine(Arc::clone(asr)),
            QaAsrStart::Bailian { asr, .. } => ActiveAsr::Bailian(Arc::clone(asr)),
            QaAsrStart::Qwen3Realtime { asr, .. } => ActiveAsr::Qwen3Realtime(Arc::clone(asr)),
            QaAsrStart::StepfunRealtime { asr, .. } => ActiveAsr::StepfunRealtime(Arc::clone(asr)),
            QaAsrStart::Xfyun { asr, .. } => ActiveAsr::Xfyun(Arc::clone(asr)),
            QaAsrStart::Ready { active, .. } => active.clone(),
        }
    }

    pub(super) fn recorder_consumer(&self) -> Arc<dyn crate::recorder::AudioConsumer> {
        match self {
            QaAsrStart::Volcengine { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::Bailian { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::Qwen3Realtime { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::StepfunRealtime { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::Xfyun { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::Ready { consumer, .. } => Arc::clone(consumer),
        }
    }

    pub(super) async fn open_streaming_session(&self) -> Result<(), String> {
        match self {
            QaAsrStart::Volcengine { asr, bridge } => {
                asr.open_session().await.map_err(|e| e.to_string())?;
                let target: Arc<dyn crate::asr::AudioConsumer> = Arc::clone(asr) as _;
                let flushed = bridge.attach(target);
                log::info!("[coord] QA ASR connected; flushed {flushed} deferred audio bytes");
                Ok(())
            }
            QaAsrStart::Bailian { asr, bridge } => {
                asr.open_session().await.map_err(|e| e.to_string())?;
                let target: Arc<dyn crate::asr::AudioConsumer> = Arc::clone(asr) as _;
                let flushed = bridge.attach(target);
                log::info!(
                    "[coord] QA Bailian ASR connected; flushed {flushed} deferred audio bytes"
                );
                Ok(())
            }
            QaAsrStart::Qwen3Realtime { asr, bridge } => {
                asr.open_session().await.map_err(|e| e.to_string())?;
                let target: Arc<dyn crate::asr::AudioConsumer> = Arc::clone(asr) as _;
                let flushed = bridge.attach(target);
                log::info!(
                    "[coord] QA Qwen3 realtime ASR connected; flushed {flushed} deferred audio bytes"
                );
                Ok(())
            }
            QaAsrStart::StepfunRealtime { asr, bridge } => {
                asr.open_session().await.map_err(|e| e.to_string())?;
                let target: Arc<dyn crate::asr::AudioConsumer> = Arc::clone(asr) as _;
                let flushed = bridge.attach(target);
                log::info!(
                    "[coord] QA StepFun realtime ASR connected; flushed {flushed} deferred audio bytes"
                );
                Ok(())
            }
            QaAsrStart::Xfyun { asr, bridge } => {
                asr.open_session().await.map_err(|e| e.to_string())?;
                let target: Arc<dyn crate::asr::AudioConsumer> = Arc::clone(asr) as _;
                let flushed = bridge.attach(target);
                log::info!(
                    "[coord] QA iFlytek ASR connected; flushed {flushed} deferred audio bytes"
                );
                Ok(())
            }
            QaAsrStart::Ready { .. } => Ok(()),
        }
    }
}

/// 返回 (启动器, 构建时 (provider, model) 快照)。快照供 QA / 重转录把「实际用了哪个
/// 模型」写回历史（PR #826 review：归因必须来自构建现场，不能事后重读设置）。
pub(super) async fn build_qa_asr_start(
    inner: &Arc<Inner>,
    active_asr: &str,
) -> Result<(QaAsrStart, AsrCallLabel), String> {
    #[cfg(target_os = "windows")]
    if foundry::is_foundry_local_whisper(active_asr) {
        let prefs = inner.prefs.get();
        let model_alias = if foundry::model_alias_is_known(&prefs.foundry_local_asr_model) {
            prefs.foundry_local_asr_model.clone()
        } else {
            foundry::DEFAULT_MODEL_ALIAS.to_string()
        };
        let language_hint = prefs.foundry_local_asr_language_hint.trim().to_string();
        let language_hint = if language_hint.is_empty() {
            None
        } else {
            Some(language_hint)
        };
        let local = Arc::new(FoundryLocalWhisperAsr::new(
            Arc::clone(&inner.foundry_local_runtime),
            model_alias.clone(),
            prefs.foundry_local_runtime_source.clone(),
            language_hint,
        ));
        let active = ActiveAsr::FoundryLocalWhisper(Arc::clone(&local));
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        let label = AsrCallLabel::new(foundry::PROVIDER_ID, Some(model_alias));
        return Ok((QaAsrStart::Ready { active, consumer }, label));
    }

    #[cfg(target_os = "windows")]
    if sherpa::is_sherpa_onnx_local(active_asr) {
        let prefs = inner.prefs.get();
        let model_alias = if sherpa::model_alias_is_known(&prefs.sherpa_onnx_model) {
            prefs.sherpa_onnx_model.clone()
        } else {
            sherpa::DEFAULT_MODEL_ALIAS.to_string()
        };
        let language_hint = prefs.sherpa_onnx_language_hint.trim().to_string();
        let language_hint = if language_hint.is_empty() {
            None
        } else {
            Some(language_hint)
        };
        let token_handler = inner.app.lock().clone().map(|app| {
            Arc::new(move |piece: String| {
                if let Err(error) = app.emit("local-asr-token", piece) {
                    log::warn!("[sherpa-asr] emit token failed: {error}");
                }
            }) as crate::asr::local::sherpa_provider::SherpaTokenHandler
        });
        let local = SherpaOnnxAsr::new_for_model(
            Arc::clone(&inner.sherpa_onnx_runtime),
            model_alias.clone(),
            language_hint,
            token_handler,
        )
        .await
        .map_err(|e| format!("sherpa-onnx init failed: {e}"))?;
        let local = Arc::new(local);
        let active = ActiveAsr::SherpaOnnxLocal(Arc::clone(&local));
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        let label = AsrCallLabel::new(sherpa::PROVIDER_ID, Some(model_alias));
        return Ok((QaAsrStart::Ready { active, consumer }, label));
    }

    #[cfg(target_os = "macos")]
    if crate::asr::local::is_local_whisper(active_asr) {
        let (local, model) = build_local_whisper(inner)
            .await
            .map_err(|e| format!("local Whisper init failed: {e}"))?;
        let active = ActiveAsr::LocalWhisper(Arc::clone(&local));
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        let label = AsrCallLabel::new(crate::asr::local::LOCAL_WHISPER_PROVIDER_ID, Some(model));
        return Ok((QaAsrStart::Ready { active, consumer }, label));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if crate::asr::local::is_local_qwen3(active_asr) {
        let (local, model) = build_local_qwen3(inner, active_asr)
            .await
            .map_err(|e| format!("local ASR init failed: {e}"))?;
        let active = ActiveAsr::Local(Arc::clone(&local));
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        let label = AsrCallLabel::new(active_asr, Some(model));
        return Ok((QaAsrStart::Ready { active, consumer }, label));
    }

    #[cfg(target_os = "macos")]
    if crate::asr::local::is_apple_speech(active_asr) {
        let local = build_apple_speech(&inner.prefs.get());
        let active = ActiveAsr::AppleSpeech(Arc::clone(&local));
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        let label = AsrCallLabel::new(crate::asr::local::APPLE_SPEECH_PROVIDER_ID, None);
        return Ok((QaAsrStart::Ready { active, consumer }, label));
    }

    // 统一百炼:按所选模型把 build 分发重定向到具体协议（凭据仍读真实 active
    // `bailian` 的那把 key；endpoint 由前端按模型同步好）。别名 id 原样返回。
    let asr_model = CredentialsVault::get(CredentialAccount::AsrModel)
        .ok()
        .flatten()
        .unwrap_or_default();
    let effective_asr = resolve_effective_asr_provider(active_asr, &asr_model)?;
    match active_asr_provider_kind(&effective_asr) {
        ActiveAsrProviderKind::Bailian => {
            let creds = read_bailian_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), Some(creds.model.clone()));
            Ok((
                QaAsrStart::Bailian {
                    asr: Arc::new(BailianRealtimeASR::new(creds)),
                    bridge: Arc::new(DeferredAsrBridge::new()),
                },
                label,
            ))
        }
        ActiveAsrProviderKind::Qwen3Realtime => {
            let creds = read_qwen3_realtime_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), Some(creds.model.clone()));
            Ok((
                QaAsrStart::Qwen3Realtime {
                    asr: Arc::new(Qwen3RealtimeASR::new(creds)),
                    bridge: Arc::new(DeferredAsrBridge::new()),
                },
                label,
            ))
        }
        ActiveAsrProviderKind::StepfunRealtime => {
            let prompt = crate::asr::whisper::build_prompt_from_phrases(&asr_vocab_phrases(inner));
            let creds = read_stepfun_realtime_credentials(prompt);
            let label = AsrCallLabel::new(effective_asr.clone(), Some(creds.model.clone()));
            Ok((
                QaAsrStart::StepfunRealtime {
                    asr: Arc::new(crate::asr::StepfunRealtimeASR::new(creds)),
                    bridge: Arc::new(DeferredAsrBridge::new()),
                },
                label,
            ))
        }
        ActiveAsrProviderKind::Mimo => {
            let (api_key, base_url, model) = read_mimo_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
            let mimo = Arc::new(MimoBatchASR::new(api_key, base_url, model));
            let active = ActiveAsr::Mimo(Arc::clone(&mimo));
            let consumer: Arc<dyn crate::recorder::AudioConsumer> = mimo;
            Ok((QaAsrStart::Ready { active, consumer }, label))
        }
        ActiveAsrProviderKind::DashScopeMultimodal => {
            let (api_key, base_url, model) = read_dashscope_multimodal_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
            let asr = Arc::new(DashScopeMultimodalASR::new(api_key, base_url, model));
            let active = ActiveAsr::DashScopeMultimodal(Arc::clone(&asr));
            let consumer: Arc<dyn crate::recorder::AudioConsumer> = asr;
            Ok((QaAsrStart::Ready { active, consumer }, label))
        }
        ActiveAsrProviderKind::ElevenLabs => {
            let (api_key, base_url, model) = read_elevenlabs_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
            let asr = Arc::new(ElevenLabsBatchASR::new(api_key, base_url, model));
            let active = ActiveAsr::ElevenLabs(Arc::clone(&asr));
            let consumer: Arc<dyn crate::recorder::AudioConsumer> = asr;
            Ok((QaAsrStart::Ready { active, consumer }, label))
        }
        ActiveAsrProviderKind::WhisperCompatible => {
            let (api_key, base_url, model) = read_whisper_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), Some(model.clone()));
            let (whisper_prompt, hotwords) =
                whisper_vocab_for_provider(active_asr, asr_vocab_phrases(inner));
            let whisper = Arc::new(apply_zenmux_asr_options(
                WhisperBatchASR::new(
                    api_key,
                    base_url,
                    model,
                    whisper_prompt,
                    batch_asr_chunk_limit_ms(active_asr),
                    whisper_supports_verbose_json(active_asr),
                )
                .with_request_format(whisper_request_format(active_asr))
                .with_hotwords(hotwords),
                active_asr,
                inner,
            ));
            let active = ActiveAsr::Whisper(Arc::clone(&whisper));
            let consumer: Arc<dyn crate::recorder::AudioConsumer> = whisper;
            Ok((QaAsrStart::Ready { active, consumer }, label))
        }
        ActiveAsrProviderKind::Volcengine => {
            let creds = read_volc_credentials();
            let label = AsrCallLabel::new(
                effective_asr.clone(),
                volc_resource_history_label(&creds.resource_id),
            );
            Ok((
                QaAsrStart::Volcengine {
                    asr: Arc::new(VolcengineStreamingASR::new(creds, enabled_hotwords(inner))),
                    bridge: Arc::new(DeferredAsrBridge::new()),
                },
                label,
            ))
        }
        ActiveAsrProviderKind::Xfyun => {
            let creds = read_xfyun_credentials();
            let label = AsrCallLabel::new(effective_asr.clone(), None);
            Ok((
                QaAsrStart::Xfyun {
                    asr: Arc::new(crate::asr::XfyunStreamingASR::new(creds)),
                    bridge: Arc::new(DeferredAsrBridge::new()),
                },
                label,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zenmux_is_whisper_compatible_json_provider() {
        use crate::asr::whisper::AsrRequestFormat;
        // issue #837：ZenMux 走 whisper 兼容路由，请求体 JSON+base64（ZenMuxJson），
        // 与 OpenRouter 共用 30s 切分；JSON 协议不吃 response_format / hotwords。
        assert!(is_whisper_compatible_provider("zenmux"));
        assert_eq!(
            active_asr_provider_kind("zenmux"),
            ActiveAsrProviderKind::WhisperCompatible
        );
        assert_eq!(
            whisper_request_format("zenmux"),
            AsrRequestFormat::ZenMuxJson
        );
        assert!(!whisper_supports_verbose_json("zenmux"));
        assert!(!whisper_uses_hotwords("zenmux"));
    }

    #[test]
    fn zenmux_language_code_covers_supported_languages_and_omits_unknown() {
        // 覆盖前端 SUPPORTED_LANGUAGES 的全部 15 种语言；未收录 → None（自动检测）。
        assert_eq!(zenmux_language_code("简体中文").as_deref(), Some("zh"));
        assert_eq!(zenmux_language_code("繁体中文").as_deref(), Some("zh"));
        assert_eq!(zenmux_language_code("English").as_deref(), Some("en"));
        assert_eq!(zenmux_language_code("日本語").as_deref(), Some("ja"));
        assert_eq!(zenmux_language_code("한국어").as_deref(), Some("ko"));
        assert_eq!(zenmux_language_code("Français").as_deref(), Some("fr"));
        assert_eq!(zenmux_language_code("Deutsch").as_deref(), Some("de"));
        assert_eq!(zenmux_language_code("Español").as_deref(), Some("es"));
        assert_eq!(zenmux_language_code("Italiano").as_deref(), Some("it"));
        assert_eq!(zenmux_language_code("Português").as_deref(), Some("pt"));
        assert_eq!(zenmux_language_code("Русский").as_deref(), Some("ru"));
        assert_eq!(zenmux_language_code("العربية").as_deref(), Some("ar"));
        assert_eq!(zenmux_language_code("Tiếng Việt").as_deref(), Some("vi"));
        assert_eq!(zenmux_language_code("ไทย").as_deref(), Some("th"));
        assert_eq!(zenmux_language_code("हिन्दी").as_deref(), Some("hi"));
        assert_eq!(zenmux_language_code(""), None);
        assert_eq!(zenmux_language_code("Esperanto"), None);
        assert_eq!(zenmux_language_code("   "), None);
    }

    #[test]
    fn require_openai_compatible_fields_errors_on_missing_endpoint_or_model() {
        // endpoint 缺失（含纯空白）→ 明确报错，绝不静默回落 whisper-1。
        assert!(require_openai_compatible_fields("", "qwen3-asr")
            .unwrap_err()
            .contains("endpoint"));
        assert!(require_openai_compatible_fields("   ", "qwen3-asr")
            .unwrap_err()
            .contains("endpoint"));
        // model 缺失 → 明确报错。
        assert!(
            require_openai_compatible_fields("http://192.168.9.31:8090/v1", "")
                .unwrap_err()
                .contains("模型")
        );
        // 两者都填 → 通过；API Key 必填与否由调用方决定，不在此函数内。
        assert!(
            require_openai_compatible_fields("http://192.168.9.31:8090/v1", "qwen3-asr").is_ok()
        );
    }
}
