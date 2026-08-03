use super::*;

const LLM_EXTRA_HEADERS_ACCOUNT: &str = "ark.extra_headers";
const LLM_TEMPERATURE_ACCOUNT: &str = "ark.temperature";

#[tauri::command]
pub async fn get_credentials() -> Result<CredentialsStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
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
    })
    .await
    .map_err(|e| format!("credential status worker failed: {e}"))
}

fn volcengine_configured(snap: &CredentialsSnapshot) -> bool {
    use crate::asr::volcengine::VolcengineAuthMode;
    let mode = snap
        .volcengine_auth_mode
        .as_deref()
        .map(VolcengineAuthMode::from_str)
        .unwrap_or(VolcengineAuthMode::AppIdToken);
    // 两种模式的密钥来源不同：AppIdToken 读 Access Token 槽，ApiKey 读独立的 API Key 槽。
    let (app_id, secret) = match mode {
        VolcengineAuthMode::AppIdToken => (
            snap.volcengine_app_key.as_deref().unwrap_or(""),
            snap.volcengine_access_key.as_deref().unwrap_or(""),
        ),
        VolcengineAuthMode::ApiKey => ("", snap.volcengine_api_key.as_deref().unwrap_or("")),
    };
    mode.auth_ok(app_id, secret) && configured(&snap.volcengine_resource_id)
}

pub(crate) fn asr_configured_for_provider(provider: &str, snap: &CredentialsSnapshot) -> bool {
    // 本地 / 无凭据引擎不属于云端分类枚举（ActiveAsrProviderKind），由平台 cfg 门
    // 在此单独判定；移动端上这些引擎不可用直接判未配置。
    if cfg!(mobile)
        && (provider == crate::asr::local::PROVIDER_ID
            || provider == crate::asr::local::sherpa::PROVIDER_ID
            || provider == crate::asr::local::foundry::PROVIDER_ID
            || provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID)
    {
        return false;
    }
    if provider == crate::asr::local::PROVIDER_ID
        || active_apple_speech_asr_is_supported(provider)
        || active_foundry_asr_is_supported(provider)
        || active_sherpa_asr_is_supported(provider)
    {
        // 本地 ASR 不依赖云端凭据。
        return true;
    }
    // 云端 provider：所需字段由 ActiveAsrProviderKind 统一判定（穷尽 match，新增
    // kind 编译器强制补齐）。volcengine 亦经此路（VolcAppKey）。
    use crate::coordinator::{active_asr_provider_kind, AsrConfiguredFields};
    match active_asr_provider_kind(provider).configured_fields() {
        AsrConfiguredFields::ApiKeyOnly => configured(&snap.asr_api_key),
        AsrConfiguredFields::ApiKeyEndpointModel => {
            configured(&snap.asr_api_key)
                && configured(&snap.asr_endpoint)
                && configured(&snap.asr_model)
        }
        AsrConfiguredFields::EndpointModelOnly => {
            configured(&snap.asr_endpoint) && configured(&snap.asr_model)
        }
        AsrConfiguredFields::VolcAppKey => volcengine_configured(snap),
    }
}

pub(crate) fn llm_configured_for_provider(provider: &str, snap: &CredentialsSnapshot) -> bool {
    if provider == CODEX_OAUTH_PROVIDER_ID {
        return CodexOAuthCredentials::load_default().is_ok();
    }
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
        "atlascloud" => Some("https://api.atlascloud.ai/v1"),
        "openai" => Some("https://api.openai.com/v1"),
        // 谷歌 Gemini 原生 API（v1beta）。后端 llm_gemini.rs 会拼成
        // `{baseUrl}/models/{model}:generateContent`，认证用 x-goog-api-key 头。
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta"),
        "mimo" => Some("https://api.xiaomimimo.com/v1"),
        "cometapi" => Some("https://api.cometapi.com/v1"),
        "openrouterFree" => Some("https://openrouter.ai/api/v1"),
        "alibabaCoding" => Some("https://coding-intl.dashscope.aliyuncs.com/v1"),
        "codingPlanX" => Some("https://api.codingplanx.ai/v1"),
        "stepfun" => Some("https://api.stepfun.com/v1"),
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
#[cfg(not(mobile))]
pub(crate) struct LocalAsrReleasePlan {
    pub(crate) qwen: bool,
    pub(crate) foundry: bool,
    pub(crate) sherpa: bool,
}

#[cfg(not(mobile))]
pub(crate) fn local_asr_release_plan_for_provider(provider: &str) -> LocalAsrReleasePlan {
    LocalAsrReleasePlan {
        qwen: provider != crate::asr::local::PROVIDER_ID,
        foundry: provider != FOUNDRY_LOCAL_PROVIDER_ID,
        sherpa: provider != crate::asr::local::sherpa::PROVIDER_ID,
    }
}

#[cfg(not(mobile))]
pub(crate) async fn release_foundry_runtime_if_inactive(
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

#[cfg(not(mobile))]
pub(crate) async fn release_sherpa_runtime_if_inactive(
    runtime: &Arc<SherpaOnnxRuntime>,
    release_sherpa: bool,
) {
    if release_sherpa {
        runtime.request_cancel_prepare();
        if let Err(error) = runtime.release_now().await {
            log::warn!("[sherpa-asr] release inactive runtime failed: {error:#}");
        }
    }
}

#[tauri::command]
pub async fn set_credential(
    window: Window,
    account: String,
    value: String,
    provider: Option<String>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let extra_headers = account == LLM_EXTRA_HEADERS_ACCOUNT;
    let temperature = account == LLM_TEMPERATURE_ACCOUNT;
    let parsed = if extra_headers || temperature {
        None
    } else {
        Some(parse_account(&account)?)
    };
    tauri::async_runtime::spawn_blocking(move || {
        if extra_headers {
            return CredentialsVault::set_active_llm_extra_headers_json(&value)
                .map_err(|e| e.to_string());
        }
        if temperature {
            return CredentialsVault::set_active_llm_temperature(&value)
                .map_err(|e| e.to_string());
        }
        let acc = parsed.expect("non-extra credential account must be parsed");
        if let Some(provider) = provider {
            if !matches!(
                acc,
                CredentialAccount::VolcengineAppKey
                    | CredentialAccount::VolcengineAccessKey
                    | CredentialAccount::VolcengineResourceId
                    | CredentialAccount::VolcengineAuthMode
                    | CredentialAccount::VolcengineApiKey
                    | CredentialAccount::AsrApiKey
                    | CredentialAccount::AsrEndpoint
                    | CredentialAccount::AsrModel
                    | CredentialAccount::AsrVocabularyId
            ) {
                return Err("provider-scoped credential must be an ASR account".to_string());
            }
            CredentialsVault::set_for_asr_provider(&provider, acc, &value)
                .map_err(|e| e.to_string())
        } else if value.is_empty() {
            CredentialsVault::remove(acc).map_err(|e| e.to_string())
        } else {
            CredentialsVault::set(acc, &value).map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("credential write worker failed: {e}"))??;
    // 通知前端凭据已变更（如 Overview 页需要刷新 asrConfigured 状态）。
    // issue #532 / #573：在 Settings 填写凭据但不切换提供商时，Overview 不会重拉状态，
    // 仍显示「未配置」。该修复曾随 #538 合入 main，但被 beta→main 合并覆盖，beta 上缺失。
    let _ = window.emit("credentials:changed", ());
    Ok(())
}

#[cfg(mobile)]
#[tauri::command]
pub async fn set_active_asr_provider(
    _coord: CoordinatorState<'_>,
    provider: String,
) -> Result<(), String> {
    if provider == crate::asr::local::PROVIDER_ID
        || provider == crate::asr::local::sherpa::PROVIDER_ID
        || provider == crate::asr::local::foundry::PROVIDER_ID
        || provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
    {
        return Err("Local ASR is not available on mobile".to_string());
    }
    if CredentialsVault::get_active_asr() == provider {
        return Ok(());
    }
    CredentialsVault::set_active_asr_provider(&provider).map_err(|e| e.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn set_active_asr_provider(
    coord: CoordinatorState<'_>,
    runtime: State<'_, Arc<FoundryLocalRuntime>>,
    sherpa_runtime: State<'_, Arc<SherpaOnnxRuntime>>,
    provider: String,
) -> Result<(), String> {
    if provider == FOUNDRY_LOCAL_PROVIDER_ID && !active_foundry_asr_is_supported(&provider) {
        return Err("Foundry Local Whisper is only available on Windows".to_string());
    }
    if provider == crate::asr::local::sherpa::PROVIDER_ID
        && !active_sherpa_asr_is_supported(&provider)
    {
        return Err("sherpa-onnx local ASR is only available on Windows".to_string());
    }
    if provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
        && !active_apple_speech_asr_is_supported(&provider)
    {
        return Err("Apple Speech recognition is only available on macOS".to_string());
    }
    if CredentialsVault::get_active_asr() == provider {
        return Ok(());
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
    release_sherpa_runtime_if_inactive(sherpa_runtime.inner(), release_plan.sherpa).await;
    Ok(())
}

#[tauri::command]
pub fn set_active_llm_provider(provider: String) -> Result<(), String> {
    CredentialsVault::set_active_llm_provider(&provider).map_err(|e| e.to_string())
}

/// 读出某个账号的实际值（用于设置页预填表单）。
/// 凭据来自系统凭据库；只允许主设置窗口读取 raw secret，避免胶囊 / QA 等辅助窗口默认暴露。
#[tauri::command]
pub async fn read_credential(
    window: Window,
    account: String,
    provider: Option<String>,
) -> Result<Option<String>, String> {
    ensure_main_window(&window)?;
    let extra_headers = account == LLM_EXTRA_HEADERS_ACCOUNT;
    let temperature = account == LLM_TEMPERATURE_ACCOUNT;
    let parsed = if extra_headers || temperature {
        None
    } else {
        Some(parse_account(&account)?)
    };
    tauri::async_runtime::spawn_blocking(move || {
        if extra_headers {
            return CredentialsVault::get_active_llm_extra_headers_json()
                .map_err(|e| e.to_string());
        }
        if temperature {
            return Ok(CredentialsVault::get_active_llm_temperature_string());
        }
        let acc = parsed.expect("non-extra credential account must be parsed");
        if let Some(provider) = provider {
            CredentialsVault::get_for_asr_provider(&provider, acc).map_err(|e| e.to_string())
        } else {
            CredentialsVault::get(acc).map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("credential read worker failed: {e}"))?
}

fn ensure_main_window(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("credential access is only allowed from the main window".to_string())
    }
}

fn parse_account(s: &str) -> Result<CredentialAccount, String> {
    match s {
        "volcengine.app_key" => Ok(CredentialAccount::VolcengineAppKey),
        "volcengine.access_key" => Ok(CredentialAccount::VolcengineAccessKey),
        "volcengine.resource_id" => Ok(CredentialAccount::VolcengineResourceId),
        "volcengine.auth_mode" => Ok(CredentialAccount::VolcengineAuthMode),
        "volcengine.api_key" => Ok(CredentialAccount::VolcengineApiKey),
        "ark.api_key" => Ok(CredentialAccount::ArkApiKey),
        "ark.model_id" => Ok(CredentialAccount::ArkModelId),
        "ark.endpoint" => Ok(CredentialAccount::ArkEndpoint),
        "asr.api_key" => Ok(CredentialAccount::AsrApiKey),
        "asr.endpoint" => Ok(CredentialAccount::AsrEndpoint),
        "asr.model" => Ok(CredentialAccount::AsrModel),
        "asr.vocabulary_id" => Ok(CredentialAccount::AsrVocabularyId),
        _ => Err(format!("unknown account: {s}")),
    }
}
