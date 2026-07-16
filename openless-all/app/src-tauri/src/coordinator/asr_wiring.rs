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
        return Ok(());
    }

    let status = permissions::check_microphone();
    if matches!(
        status,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        return Ok(());
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

    // 本地 Qwen3-ASR 没有"凭据"概念，但需要：(a) macOS 平台 (b) 模型已下载。
    if crate::asr::local::is_local_qwen3(&active_asr) {
        #[cfg(not(target_os = "macos"))]
        {
            return Err("本地 ASR 当前仅支持 macOS（Windows 见 issue #256）".to_string());
        }
        #[cfg(target_os = "macos")]
        {
            return ensure_local_qwen3_model_ready();
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
            let creds = read_volc_credentials();
            if creds.app_id.trim().is_empty() || creds.access_token.trim().is_empty() {
                Err("请先在设置中填写火山引擎 ASR App Key 和 Access Key".to_string())
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
pub(super) fn is_keyless_local_asr_provider(id: &str) -> bool {
    if crate::asr::local::is_local_qwen3(id) {
        return true;
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

#[cfg(target_os = "macos")]
pub(super) fn ensure_local_qwen3_model_ready() -> Result<(), String> {
    let prefs = || -> Result<crate::types::UserPreferences, String> {
        // 这里没法拿到 inner，直接读 preferences.json 即可（Coordinator 写盘后总是同步的）。
        crate::persistence::PreferencesStore::new()
            .map_err(|e| e.to_string())
            .map(|s| s.get())
    }()?;
    let model_id = crate::asr::local::ModelId::from_str(&prefs.local_asr_active_model)
        .ok_or_else(|| format!("未知的本地模型 id: {}", prefs.local_asr_active_model))?;
    if !crate::asr::local::models::is_downloaded(model_id) {
        return Err(format!(
            "本地模型 {} 未下载完整，请到 设置 → 模型设置 中下载",
            model_id.as_str()
        ));
    }
    Ok(())
}

/// 引擎加载/释放/keepLoadedSecs 变化时主动推给前端，前端 listen
/// `local-asr:engine-changed` 即可零轮询同步 UI（issue #470 / #6）。
/// 只反映 Qwen3 这一路（loaded_model_id / prefs），不碰 Foundry / Sherpa。
/// 仅用桌面端跨平台符号；Android 无本地 ASR 引擎（LocalAsrEngineStatus 不在该 target
/// 编译），单独给 no-op stub（见下），让各调用点在所有平台统一编译。
#[cfg(not(target_os = "android"))]
pub(super) fn emit_local_asr_engine_status(inner: &Arc<Inner>) {
    let model_id = inner.local_asr_cache.loaded_model_id();
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

/// 一次 dictation 结束后，按 prefs.local_asr_keep_loaded_secs 决定何时释放
/// 内存里的 Qwen3-ASR 引擎。0 = 立即释放；其它值 = sleep N 秒后看 last_used。
/// 多次会话叠加多个 sleep 任务，每个独立 check：只要中间又被使用过就跳过释放。
pub(super) fn schedule_local_asr_release(inner: &Arc<Inner>) {
    let keep_secs = inner.prefs.get().local_asr_keep_loaded_secs;
    let cache = Arc::clone(&inner.local_asr_cache);
    if keep_secs == 0 {
        cache.release_now();
        emit_local_asr_engine_status(inner);
        return;
    }
    let dur = std::time::Duration::from_secs(keep_secs as u64);
    let inner = Arc::clone(inner);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(dur).await;
        if cache.release_if_idle(dur) {
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
pub(super) fn asr_release_session_is_current(inner: &Arc<Inner>, session: AsrReleaseSession) -> bool {
    match session {
        AsrReleaseSession::Dictation(session_id) => inner.state.lock().session_id == session_id,
        AsrReleaseSession::Qa(session_id) => inner.qa_state.lock().session_id == session_id,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn schedule_foundry_local_asr_release(inner: &Arc<Inner>, session: AsrReleaseSession) {
    let keep_secs = foundry_local_asr_release_keep_secs(inner);
    let runtime = Arc::clone(&inner.foundry_local_runtime);
    let inner = Arc::clone(inner);
    tauri::async_runtime::spawn(async move {
        if keep_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(keep_secs as u64)).await;
        }
        if !asr_release_session_is_current(&inner, session) {
            return;
        }
        if let Err(error) = runtime.release_now().await {
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

#[cfg(target_os = "macos")]
/// 返回 (provider, 实际加载的模型 id)。模型 id 是 ModelId 校验归一后的值，调用方
/// 直接用它做历史归因（构建时快照，PR #826 review）。
pub(super) async fn build_local_qwen3(
    inner: &Arc<Inner>,
) -> anyhow::Result<(Arc<crate::asr::local::LocalQwenAsr>, String)> {
    let prefs = inner.prefs.get();
    let model_id = crate::asr::local::ModelId::from_str(&prefs.local_asr_active_model)
        .ok_or_else(|| anyhow::anyhow!("未知本地模型 id: {}", prefs.local_asr_active_model))?;
    let dir = crate::asr::local::models::model_dir(model_id)?;
    let app = inner
        .app
        .lock()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("AppHandle 未绑定"))?;
    // 走缓存：如果已有同 id 的引擎在内存里就直接复用，避免每次会话都重加载
    // 1.2GB+ 模型。第一次加载阻塞数秒，spawn_blocking 不卡 tokio runtime。
    let cache = Arc::clone(&inner.local_asr_cache);
    let mid = model_id.as_str().to_string();
    let engine = tauri::async_runtime::spawn_blocking(move || cache.get_or_load(&mid, &dir))
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
/// 新增 OpenAI 兼容 ASR 时只需在这里加一项。
///
/// 注：DashScope 的 Qwen3-ASR-Flash 不在此列——它用 MultiModalConversation
/// (messages=[{content:[{audio:...}]}]) 协议，不是 Whisper multipart，需要
/// 单独 ASR 客户端，留给 V2。
pub(super) fn is_whisper_compatible_provider(id: &str) -> bool {
    matches!(
        id,
        "whisper" | "siliconflow" | "zhipu" | "groq" | "openrouter" | "stepfun"
    )
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
pub(super) fn whisper_request_format(provider_id: &str) -> crate::asr::whisper::AsrRequestFormat {
    match provider_id {
        "openrouter" => crate::asr::whisper::AsrRequestFormat::OpenRouterJson,
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
pub(super) fn whisper_supports_verbose_json(provider_id: &str) -> bool {
    matches!(provider_id, "whisper" | "groq")
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
            QaAsrStart::Ready { active, .. } => active.clone(),
        }
    }

    pub(super) fn recorder_consumer(&self) -> Arc<dyn crate::recorder::AudioConsumer> {
        match self {
            QaAsrStart::Volcengine { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::Bailian { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::Qwen3Realtime { bridge, .. } => Arc::clone(bridge) as _,
            QaAsrStart::StepfunRealtime { bridge, .. } => Arc::clone(bridge) as _,
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
    if crate::asr::local::is_local_qwen3(active_asr) {
        let (local, model) = build_local_qwen3(inner)
            .await
            .map_err(|e| format!("local ASR init failed: {e}"))?;
        let active = ActiveAsr::Local(Arc::clone(&local));
        let consumer: Arc<dyn crate::recorder::AudioConsumer> = local;
        let label = AsrCallLabel::new(crate::asr::local::PROVIDER_ID, Some(model));
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
            let prompt = crate::asr::whisper::build_prompt_from_phrases(&enabled_phrases(inner));
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
                whisper_vocab_for_provider(active_asr, enabled_phrases(inner));
            let whisper = Arc::new(
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
            );
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
    }
}
