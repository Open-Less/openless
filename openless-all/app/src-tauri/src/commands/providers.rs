use super::*;
use base64::Engine;
use std::collections::HashMap;

/// 一次连通测试 / 模型列表请求所针对的渠道。
///
/// 渠道化之前这两条路径都隐式读"当前生效"的凭据；卡片化之后用户会对列表里**任意**
/// 一张卡片点「测试连通」，包括还没轮到它生效的那些。`channel = None` 保留旧语义
/// （当前生效的渠道），供未指定渠道的老调用点使用。
///
/// 注意这只覆盖测试与模型列表两条路径 —— 真正的听写 / 润色链路仍走隐式 active，
/// 那部分的显式化是 P1 的工作（见 docs/provider-channels-plan.md）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderKind {
    Asr,
    Llm,
    Omni,
}

impl ProviderKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "asr" => Ok(Self::Asr),
            "llm" => Ok(Self::Llm),
            "omni" => Ok(Self::Omni),
            other => Err(format!("unknown provider kind: {other}")),
        }
    }
}

pub(crate) struct ProviderScope {
    kind: ProviderKind,
    channel: Option<String>,
}

impl ProviderScope {
    fn new(kind: &str, channel: Option<String>) -> Result<Self, String> {
        let kind = ProviderKind::parse(kind)?;
        if kind == ProviderKind::Omni && channel.is_some() {
            return Err("omni provider does not support channel id".to_string());
        }
        Ok(Self { kind, channel })
    }

    /// 读该渠道的凭据；未指定渠道时回落到当前生效的那张。
    fn get(&self, account: CredentialAccount) -> Result<Option<String>, String> {
        match (&self.channel, self.kind) {
            (Some(id), ProviderKind::Asr) => CredentialsVault::get_for_asr_provider(id, account),
            (Some(id), ProviderKind::Llm) => CredentialsVault::get_for_llm_provider(id, account),
            (Some(_), ProviderKind::Omni) => {
                return Err("omni provider does not support channel id".to_string())
            }
            (None, _) => CredentialsVault::get(account),
        }
        .map_err(|e| e.to_string())
    }

    /// 该渠道的厂商 id —— 决定走哪套协议。
    fn provider_type(&self) -> String {
        match (&self.channel, self.kind) {
            (Some(id), ProviderKind::Asr) => {
                CredentialsVault::get_channel_provider_type(ChannelKind::Asr, id)
                    .unwrap_or_else(|| id.clone())
            }
            (Some(id), ProviderKind::Llm) => {
                CredentialsVault::get_channel_provider_type(ChannelKind::Llm, id)
                    .unwrap_or_else(|| id.clone())
            }
            (Some(_), ProviderKind::Omni) => CredentialsVault::get_active_omni(),
            (None, ProviderKind::Asr) => CredentialsVault::get_active_asr(),
            (None, ProviderKind::Llm) => CredentialsVault::get_active_llm(),
            (None, ProviderKind::Omni) => CredentialsVault::get_active_omni(),
        }
    }

    fn llm_extra_headers(&self) -> HashMap<String, String> {
        match &self.channel {
            Some(id) => CredentialsVault::get_llm_extra_headers_for_channel(id),
            None => CredentialsVault::get_active_llm_extra_headers(),
        }
    }

    fn llm_temperature(&self) -> Option<f32> {
        match &self.channel {
            Some(id) => CredentialsVault::get_llm_temperature_for_channel(id),
            None => CredentialsVault::get_active_llm_temperature(),
        }
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

/// `channel_id = None` 时测当前生效的渠道（老行为）；卡片上的「测试连通」会带上
/// 那张卡片的 id，这样还没轮到生效的渠道也能验证。
#[tauri::command]
pub async fn validate_provider_credentials(
    kind: String,
    channel_id: Option<String>,
) -> Result<ProviderCheckResult, String> {
    let scope = ProviderScope::new(&kind, channel_id)?;
    let scope = &scope;
    match scope.kind {
        ProviderKind::Llm => validate_llm_provider(scope)
            .await
            .map(|()| ProviderCheckResult { ok: true }),
        ProviderKind::Asr => validate_asr_provider(scope)
            .await
            .map(|()| ProviderCheckResult { ok: true }),
        ProviderKind::Omni => validate_omni_provider()
            .await
            .map(|()| ProviderCheckResult { ok: true }),
    }
}

#[tauri::command]
pub async fn list_provider_models(
    kind: String,
    channel_id: Option<String>,
) -> Result<ProviderModelsResult, String> {
    let scope = ProviderScope::new(&kind, channel_id)?;
    let scope = &scope;
    if scope.kind == ProviderKind::Asr && scope.provider_type() == crate::asr::bailian::PROVIDER_ID
    {
        // 统一「阿里云百炼」入口:三条协议(实时 fun-asr-realtime / 实时 qwen3 /
        // 录音文件 fun-asr-flash)收成一个 provider。百炼各网关都没有模型列表 HTTP
        // 接口,列表是静态的;但先跑一次与「验证」相同的、按当前所选模型对应协议的
        // 连通性检查(validate_asr_provider 已按模型路由),避免 Key/endpoint 全错时
        // 也显示成功。随后返回三个可选模型供下拉。
        validate_asr_provider(scope).await?;
        // 静态清单只是常用快捷项；协议按模型名自动路由，用户也可在模型框直接手填
        // 已支持的 DashScope ASR 模型；不支持的模型会在验证/开始录音前明确拒绝。
        return Ok(ProviderModelsResult {
            models: vec![
                crate::asr::bailian::DEFAULT_MODEL.to_string(),
                "fun-asr-flash-8k-realtime".to_string(),
                crate::asr::qwen_realtime::DEFAULT_MODEL.to_string(),
                "qwen3-asr-flash-realtime-2026-02-10".to_string(),
                "qwen3-asr-flash-realtime-2025-10-27".to_string(),
                crate::asr::dashscope_multimodal::QWEN_AUDIO_MODEL.to_string(),
                crate::asr::dashscope_multimodal::DEFAULT_MODEL.to_string(),
                "qwen3-asr-flash".to_string(),
                "fun-asr".to_string(),
                "fun-asr-2025-11-07".to_string(),
                "fun-asr-2025-08-25".to_string(),
                "fun-asr-mtl".to_string(),
                "fun-asr-mtl-2025-08-25".to_string(),
                "paraformer-v2".to_string(),
            ],
        });
    }
    if scope.kind == ProviderKind::Asr
        && scope.provider_type() == crate::asr::qwen_realtime::PROVIDER_ID
    {
        // 与 bailian 同理：Realtime 网关无模型列表接口，先做真实连通性检查，
        // 列表为官方文档在案的稳定别名 + 快照版本。
        validate_qwen3_realtime_asr_provider(scope).await?;
        return Ok(ProviderModelsResult {
            models: vec![
                crate::asr::qwen_realtime::DEFAULT_MODEL.to_string(),
                "qwen3-asr-flash-realtime-2026-02-10".to_string(),
                "qwen3-asr-flash-realtime-2025-10-27".to_string(),
            ],
        });
    }
    if scope.kind == ProviderKind::Asr && scope.provider_type() == crate::asr::mimo::PROVIDER_ID {
        return Ok(ProviderModelsResult {
            models: vec![crate::asr::mimo::DEFAULT_MODEL.to_string()],
        });
    }
    if scope.kind == ProviderKind::Asr
        && scope.provider_type() == crate::asr::dashscope_multimodal::PROVIDER_ID
    {
        // multimodal-generation 无模型列表 HTTP 接口；与 mimo 同，返回静态别名。
        return Ok(ProviderModelsResult {
            models: vec![
                crate::asr::dashscope_multimodal::QWEN_AUDIO_MODEL.to_string(),
                crate::asr::dashscope_multimodal::DEFAULT_MODEL.to_string(),
            ],
        });
    }
    if scope.kind == ProviderKind::Asr
        && scope.provider_type() == crate::asr::elevenlabs::PROVIDER_ID
    {
        validate_elevenlabs_asr_provider(scope).await?;
        return Ok(ProviderModelsResult {
            models: vec![crate::asr::elevenlabs::DEFAULT_MODEL.to_string()],
        });
    }
    if scope.kind == ProviderKind::Llm && scope.provider_type() == CODEX_OAUTH_PROVIDER_ID {
        return Ok(ProviderModelsResult {
            models: vec![
                CODEX_DEFAULT_MODEL.to_string(),
                "gpt-5.3-codex".to_string(),
                "gpt-5.4".to_string(),
                "gpt-5.5".to_string(),
            ],
        });
    }
    let config = read_openai_provider_config(scope)?;
    fetch_provider_models(&config)
        .await
        .map(|models| ProviderModelsResult { models })
}

pub(crate) struct ProviderConfig {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) extra_headers: HashMap<String, String>,
    pub(crate) temperature: Option<f32>,
}

fn read_openai_provider_config(scope: &ProviderScope) -> Result<ProviderConfig, String> {
    // `openai-compatible` 允许 API Key 留空（LAN 无鉴权端点）；其余 ASR 提供商
    // 仍必填，与运行时门禁 ensure_asr_credentials 保持一致。
    let (api_key_account, endpoint_account, api_key_required) = match scope.kind {
        ProviderKind::Llm => (
            CredentialAccount::ArkApiKey,
            CredentialAccount::ArkEndpoint,
            false,
        ),
        ProviderKind::Asr => (
            CredentialAccount::AsrApiKey,
            CredentialAccount::AsrEndpoint,
            scope.provider_type() != crate::coordinator::OPENAI_COMPATIBLE_ASR_PROVIDER_ID,
        ),
        // 多模态（Omni）模型：独立命名空间，OpenAI 兼容通道要求 API Key + Base URL。
        ProviderKind::Omni => (
            CredentialAccount::OmniApiKey,
            CredentialAccount::OmniEndpoint,
            true,
        ),
    };
    let api_key = scope
        .get(api_key_account)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let base_url = scope
        .get(endpoint_account)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let (extra_headers, temperature) = if scope.kind == ProviderKind::Llm {
        let active_llm = scope.provider_type();
        (
            scope.llm_extra_headers(),
            openai_compatible_temperature_for_provider(&active_llm, scope.llm_temperature()),
        )
    } else if scope.kind == ProviderKind::Omni {
        let active_omni = CredentialsVault::get_active_omni();
        (
            CredentialsVault::get_active_omni_extra_headers(),
            openai_compatible_temperature_for_provider(
                &active_omni,
                CredentialsVault::get_active_omni_temperature(),
            ),
        )
    } else {
        (HashMap::new(), None)
    };
    if api_key_required && api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("Endpoint 为空".to_string());
    }
    // endpoint 校验：仅保证是合法 http(s) URL，地址不设任何限制（公网/局域网/内网
    // DNS/hosts 别名/本地均可）——端点由用户显式配置，选择权在用户；前端对 http://
    // 输入展示明文风险提示。覆盖 validate_provider_credentials 连通性测试与
    // list_provider_models 模型列表两条 HTTP 路径。
    crate::endpoint_security::validate_http_endpoint(&base_url)
        .map_err(|_| "endpointInvalid".to_string())?;
    Ok(ProviderConfig {
        base_url,
        api_key,
        extra_headers,
        temperature,
    })
}

async fn validate_llm_provider(scope: &ProviderScope) -> Result<(), String> {
    let llm_thinking_enabled = PreferencesStore::new()
        .map_err(|e| e.to_string())?
        .get()
        .llm_thinking_enabled;
    if scope.provider_type() == CODEX_OAUTH_PROVIDER_ID {
        let model = scope
            .get(CredentialAccount::ArkModelId)
            .map_err(|e| e.to_string())?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| CODEX_DEFAULT_MODEL.to_string());
        let provider = CodexOAuthLLMProvider::new(
            CodexOAuthConfig::new(model).with_thinking_enabled(llm_thinking_enabled),
        );
        return provider
            .polish(
                "验证连接",
                PolishMode::Raw,
                &[],
                "",
                &[],
                ChineseScriptPreference::Auto,
                OutputLanguagePreference::Auto,
                None,
                None,
                &[],
            )
            .await
            .map(|_| ())
            .map_err(provider_llm_error_message);
    }

    let config = read_openai_provider_config(scope)?;
    let active_llm = scope.provider_type();
    let model = scope
        .get(CredentialAccount::ArkModelId)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "llmModelMissing".to_string())?;
    let provider = OpenAICompatibleLLMProvider::new(
        OpenAICompatibleConfig::new(
            active_llm.clone(),
            active_llm,
            config.base_url,
            config.api_key,
            model,
        )
        .with_thinking_enabled(llm_thinking_enabled)
        .with_temperature(config.temperature)
        .with_extra_headers(config.extra_headers),
    );
    provider
        .polish(
            "验证连接",
            PolishMode::Raw,
            &[],
            "",
            &[],
            ChineseScriptPreference::Auto,
            OutputLanguagePreference::Auto,
            None,
            None,
            &[],
        )
        .await
        .map(|_| ())
        .map_err(provider_llm_error_message)
}

fn provider_llm_error_message(error: LLMError) -> String {
    match error {
        LLMError::InvalidResponse { status, .. } => format!("providerHttpStatus:{status}"),
        LLMError::Timeout => "请求超时".to_string(),
        LLMError::Network(_) => "网络请求失败".to_string(),
        LLMError::MissingCredentials => "providerCredentialsMissing".to_string(),
        LLMError::ParseError(_) => "providerInvalidResponse".to_string(),
        LLMError::CodexAuth(_) => "codexOAuthUnavailable".to_string(),
    }
}

/// 多模态（Omni）模型连通性验证：真发一次纯文本请求（无音频），走与运行期
/// 完全相同的 provider 构建与请求路径，避免「验证通过但真实调用失败」。
async fn validate_omni_provider() -> Result<(), String> {
    let provider =
        crate::coordinator::build_active_omni_provider(false).map_err(|e| e.to_string())?;
    provider
        .complete("验证连接", "ping", None)
        .await
        .map(|_| ())
        .map_err(provider_llm_error_message)
}

async fn validate_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let active_asr = scope.provider_type();
    if active_asr_is_keyless_for_validation(&active_asr) {
        return Ok(());
    }

    if active_asr == crate::asr::bailian::PROVIDER_ID {
        // 统一百炼:按所选模型验证对应协议（endpoint 由前端按模型同步，各 validator
        // 读到的都是该协议的正确地址）。
        let model = scope
            .get(CredentialAccount::AsrModel)
            .ok()
            .flatten()
            .unwrap_or_default();
        let effective = crate::coordinator::resolve_effective_asr_provider(&active_asr, &model)?;
        if effective == crate::asr::qwen_realtime::PROVIDER_ID {
            return validate_qwen3_realtime_asr_provider(scope).await;
        }
        if effective == crate::asr::dashscope_multimodal::PROVIDER_ID {
            return validate_dashscope_multimodal_asr_provider(scope).await;
        }
        return validate_bailian_asr_provider(scope).await;
    }
    if active_asr == crate::asr::qwen_realtime::PROVIDER_ID {
        return validate_qwen3_realtime_asr_provider(scope).await;
    }
    if active_asr == crate::asr::mimo::PROVIDER_ID {
        return validate_mimo_asr_provider(scope).await;
    }
    if active_asr == crate::asr::dashscope_multimodal::PROVIDER_ID {
        let model = scope
            .get(CredentialAccount::AsrModel)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        crate::coordinator::validate_dashscope_multimodal_model(&model)?;
        return validate_dashscope_multimodal_asr_provider(scope).await;
    }
    if active_asr == crate::asr::elevenlabs::PROVIDER_ID {
        return validate_elevenlabs_asr_provider(scope).await;
    }
    if active_asr == crate::asr::xfyun::PROVIDER_ID {
        return validate_xfyun_asr_provider(scope).await;
    }
    // 火山走专属 WS 协议与 volcengine.* 凭据槽位，不能落进下面的 OpenAI 兼容
    // HTTP 兜底（那条路只认 asr.api_key —— 火山从不写入的槽位，填对也必报
    // 「API Key 为空」）。
    if active_asr == "volcengine" {
        return validate_volcengine_asr_provider(scope).await;
    }
    // StepFun 一入口双协议：`*-stream` 模型走实时 WS 验证，其余走批式
    // /audio/transcriptions（与 build 侧 resolve_effective_asr_provider 同判据）。
    if active_asr == "stepfun" || active_asr == crate::asr::stepfun_realtime::PROVIDER_ID {
        let model = scope
            .get(CredentialAccount::AsrModel)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        if active_asr == crate::asr::stepfun_realtime::PROVIDER_ID
            || crate::coordinator::stepfun_model_is_stream(&model)
        {
            return validate_stepfun_realtime_asr_provider(scope).await;
        }
    }

    let config = read_openai_provider_config(scope)?;
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "asrModelMissing".to_string())?;
    // 验证请求体与真实转写保持一致：OpenRouter / ZenMux 走 JSON+base64，
    // 其余 whisper 兼容厂商走 multipart——避免「测试连接」假阴性（issue #837）。
    let request_format = crate::coordinator::whisper_request_format(&active_asr);
    validate_asr_transcription(&config, model.trim(), request_format).await
}

/// 讯飞 RTASR 验证：真连 + 500ms 静音 + 收尾。鉴权错误（10105 / 10110）在握手阶段
/// 即返回；纯静音会话服务端可能直接关闭且不返回任何 result（等价于「没说话」），
/// 这类 `NoFinalResult` 不算验证失败 —— 握手成功已经证明 AppID/APIKey 有效。
async fn validate_xfyun_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let app_id = scope
        .get(CredentialAccount::XfyunAppId)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if app_id.trim().is_empty() {
        return Err("讯飞 AppID 为空".to_string());
    }
    let api_key = scope
        .get(CredentialAccount::XfyunApiKey)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err("讯飞 API Key 为空".to_string());
    }
    let asr = std::sync::Arc::new(crate::asr::XfyunStreamingASR::new(
        crate::asr::XfyunCredentials { app_id, api_key },
    ));
    asr.open_session().await.map_err(|e| e.to_string())?;
    crate::asr::AudioConsumer::consume_pcm_chunk(
        &*asr,
        &vec![0u8; crate::asr::xfyun::TARGET_AUDIO_CHUNK_BYTES * 5],
    );
    asr.send_last_frame().await.map_err(|e| e.to_string())?;
    match asr.await_final_result().await {
        Ok(_) => Ok(()),
        Err(crate::asr::xfyun::XfyunASRError::NoFinalResult) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// 按鉴权模式检查火山凭据完整性，返回给前端映射多语言文案的哨兵串
/// （providerErrorMessage 识别）。与 [`VolcengineAuthMode::auth_ok`] 同一
/// trim 语义，但区分缺哪一项，让用户直接知道该补哪个输入框。
///
/// [`VolcengineAuthMode::auth_ok`]: crate::asr::volcengine::VolcengineAuthMode::auth_ok
fn volcengine_missing_credential_error(
    auth_mode: &crate::asr::volcengine::VolcengineAuthMode,
    app_id: &str,
    secret: &str,
) -> Option<&'static str> {
    use crate::asr::volcengine::VolcengineAuthMode;
    match auth_mode {
        VolcengineAuthMode::AppIdToken => {
            if app_id.trim().is_empty() {
                return Some("volcengineAppIdMissing");
            }
            if secret.trim().is_empty() {
                return Some("volcengineAccessTokenMissing");
            }
        }
        VolcengineAuthMode::ApiKey => {
            if secret.trim().is_empty() {
                return Some("volcengineApiKeyMissing");
            }
        }
    }
    None
}

/// 火山 bigmodel 验证：真连 + 1s 静音 + 收尾。密钥槽位随鉴权模式（与
/// `read_volc_credentials` 同规则）：旧版读 volcengine.access_key，新版控制台
/// 读 volcengine.api_key，互不污染。鉴权错误（401/403 → AuthRejected）在
/// WebSocket 握手阶段即返回；纯静音会话服务端可能不回 final（等价「没说话」），
/// 这类 `NoFinalResult` 不算验证失败 —— 握手成功已经证明凭据有效。
async fn validate_volcengine_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    use crate::asr::volcengine::{VolcengineAuthMode, VolcengineCredentials};
    let auth_mode = scope
        .get(CredentialAccount::VolcengineAuthMode)?
        .map(|s| VolcengineAuthMode::from_str(&s))
        .unwrap_or(VolcengineAuthMode::AppIdToken);
    let app_id = scope
        .get(CredentialAccount::VolcengineAppKey)?
        .unwrap_or_default();
    let secret = match auth_mode {
        VolcengineAuthMode::AppIdToken => scope.get(CredentialAccount::VolcengineAccessKey)?,
        VolcengineAuthMode::ApiKey => scope.get(CredentialAccount::VolcengineApiKey)?,
    }
    .unwrap_or_default();
    if let Some(message) = volcengine_missing_credential_error(&auth_mode, &app_id, &secret) {
        return Err(message.to_string());
    }
    let resource_id = VolcengineCredentials::resolve_resource_id(
        scope.get(CredentialAccount::VolcengineResourceId)?,
    );
    let asr = std::sync::Arc::new(crate::asr::VolcengineStreamingASR::new(
        VolcengineCredentials {
            auth_mode,
            app_id,
            access_token: secret,
            resource_id,
        },
        Vec::new(),
    ));
    asr.open_session().await.map_err(|e| e.to_string())?;
    crate::asr::AudioConsumer::consume_pcm_chunk(
        &*asr,
        &vec![0u8; crate::asr::volcengine::TARGET_AUDIO_CHUNK_BYTES * 5],
    );
    asr.send_last_frame().await.map_err(|e| e.to_string())?;
    match asr.await_final_result().await {
        Ok(_) => Ok(()),
        Err(crate::asr::volcengine::VolcengineASRError::NoFinalResult) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// StepFun 实时 WS 验证：真连 + session.update + 500ms 静音 + 收尾。
/// 协议无 finish 事件，收尾走静音帧 + 宽限期（纯静音会话以空文本成功返回，
/// 见 stepfun_realtime 模块注释），全程 ~2s。
async fn validate_stepfun_realtime_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let api_key = scope
        .get(CredentialAccount::AsrApiKey)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    let endpoint = scope
        .get(CredentialAccount::AsrEndpoint)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::stepfun_realtime::DEFAULT_MODEL.to_string());
    let asr = std::sync::Arc::new(crate::asr::StepfunRealtimeASR::new(
        crate::asr::StepfunRealtimeCredentials {
            api_key,
            endpoint,
            model,
            prompt: None,
        },
    ));
    asr.open_session().await.map_err(|e| e.to_string())?;
    crate::asr::AudioConsumer::consume_pcm_chunk(
        &*asr,
        &vec![0u8; crate::asr::stepfun_realtime::TARGET_AUDIO_CHUNK_BYTES * 5],
    );
    asr.send_last_frame().await.map_err(|e| e.to_string())?;
    asr.await_final_result()
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn validate_mimo_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let config = read_openai_provider_config(scope)?;
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::mimo::DEFAULT_MODEL.to_string());
    let asr = crate::asr::MimoBatchASR::new(config.api_key, config.base_url, model);
    crate::recorder::AudioConsumer::consume_pcm_chunk(
        &asr,
        &encode_wav_16k_mono_silence(250)[44..],
    );
    asr.transcribe()
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn validate_elevenlabs_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let api_key = scope
        .get(CredentialAccount::AsrApiKey)
        .map_err(|e| e.to_string())?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "API Key 为空".to_string())?;
    let base_url = scope
        .get(CredentialAccount::AsrEndpoint)
        .map_err(|e| e.to_string())?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| crate::asr::elevenlabs::DEFAULT_ENDPOINT.to_string());
    crate::endpoint_security::validate_http_endpoint(&base_url)
        .map_err(|_| "endpointInvalid".to_string())?;
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| crate::asr::elevenlabs::DEFAULT_MODEL.to_string());
    let asr = crate::asr::ElevenLabsBatchASR::new(api_key, base_url, model);
    crate::recorder::AudioConsumer::consume_pcm_chunk(
        &asr,
        &encode_wav_16k_mono_silence(250)[44..],
    );
    asr.transcribe().await.map(|_| ()).map_err(|error| {
        if error.chain().any(|cause| {
            cause
                .downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_timeout)
        }) {
            "providerRequestTimeout".to_string()
        } else {
            error.to_string()
        }
    })
}

/// DashScope 录音文件 ASR 官方公开示例音频，用于两个已支持模型的连通性校验。
/// 这类模型对纯静音会返回
/// 400（"no speech" 类错误），无法像 Whisper/Mimo 那样发静音探活；改用这段
/// 阿里官方文档在案的示例 wav（由 DashScope 侧拉取），key/endpoint/model 有效
/// 即返回 200。
const DASHSCOPE_ASR_VALIDATE_SAMPLE_URL: &str =
    "https://dashscope.oss-cn-beijing.aliyuncs.com/samples/audio/paraformer/hello_world_female2.wav";
// 异步验证只需确认「提交 → 轮询 → 下载」链路可用：示例音频很短，任务通常在
// 数十秒内完成。外层 120s（30s 提交 + 60s 轮询 + 30s 下载）封顶，避免验证按钮
// 在最坏情况下阻塞近 11 分钟（真实转写仍用长轮询，不受影响）。
const DASHSCOPE_ASR_VALIDATE_TIMEOUT_SECS: u64 = 120;
const DASHSCOPE_ASR_VALIDATE_POLL_SECS: u64 = 60;

async fn validate_dashscope_multimodal_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    // 统一百炼复用配置中的区域/工作空间主机，并推导 multimodal 的 https 路径。
    // 隐藏别名仍按原有完整 endpoint 读取。
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::dashscope_multimodal::DEFAULT_MODEL.to_string());
    crate::coordinator::validate_dashscope_multimodal_model(&model)?;
    let protocol = crate::asr::dashscope_multimodal::protocol_for_model(&model)
        .unwrap_or(crate::asr::dashscope_multimodal::DashScopeBatchProtocol::Multimodal);
    let (api_key, base_url) = if crate::coordinator::unified_bailian_is_active() {
        let api_key = scope
            .get(CredentialAccount::AsrApiKey)
            .map_err(|e| e.to_string())?
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "API Key 为空".to_string())?;
        let endpoint = scope
            .get(CredentialAccount::AsrEndpoint)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let endpoint_protocol = match protocol {
            crate::asr::dashscope_multimodal::DashScopeBatchProtocol::Multimodal => {
                crate::coordinator::BailianEndpointProtocol::Multimodal
            }
            crate::asr::dashscope_multimodal::DashScopeBatchProtocol::AsyncTranscription => {
                crate::coordinator::BailianEndpointProtocol::AsyncTranscription
            }
        };
        let endpoint = crate::coordinator::derive_bailian_endpoint(&endpoint, endpoint_protocol)?;
        (api_key, endpoint)
    } else {
        let config = read_openai_provider_config(scope)?;
        (config.api_key, config.base_url)
    };
    if protocol == crate::asr::dashscope_multimodal::DashScopeBatchProtocol::AsyncTranscription {
        let asr = crate::asr::DashScopeMultimodalASR::new(api_key, base_url, model);
        return match tokio::time::timeout(
            std::time::Duration::from_secs(DASHSCOPE_ASR_VALIDATE_TIMEOUT_SECS),
            asr.transcribe_async_url_with_timeout(
                DASHSCOPE_ASR_VALIDATE_SAMPLE_URL,
                std::time::Duration::from_secs(DASHSCOPE_ASR_VALIDATE_POLL_SECS),
            ),
        )
        .await
        {
            Ok(result) => result.map(|_| ()).map_err(|error| error.to_string()),
            Err(_) => Err("providerRequestTimeout".to_string()),
        };
    }
    let url = crate::asr::dashscope_multimodal::generation_url(&base_url)
        .map_err(|_| "endpointInvalid".to_string())?;
    let body = crate::asr::dashscope_multimodal::dashscope_multimodal_body_from_uri(
        &model,
        DASHSCOPE_ASR_VALIDATE_SAMPLE_URL,
    );
    send_dashscope_multimodal_validation(&api_key, url.as_str(), &body).await
}

async fn send_dashscope_multimodal_validation(
    api_key: &str,
    url: &str,
    body: &Value,
) -> Result<(), String> {
    let response = crate::net::credential_http()
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("X-DashScope-SSE", "disable")
        .json(&body)
        .timeout(std::time::Duration::from_secs(20))
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
    Ok(())
}

async fn validate_bailian_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let api_key = scope
        .get(CredentialAccount::AsrApiKey)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    // 已知残留（issue #609 F-01 孪生 gap）：Bailian endpoint 走 `wss://`，与 http/https-only 的
    // validate_http_endpoint 不兼容，无法直接复用，需单独的 ws/wss 感知 SSRF 校验器（超本次范围）。
    let stored_endpoint = scope
        .get(CredentialAccount::AsrEndpoint)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::bailian::DEFAULT_ENDPOINT.to_string());
    let endpoint = if crate::coordinator::unified_bailian_is_active() {
        crate::coordinator::derive_bailian_endpoint(
            &stored_endpoint,
            crate::coordinator::BailianEndpointProtocol::ClassicRealtime,
        )?
    } else {
        stored_endpoint
    };
    // 协议头先行校验：填成 https://（百炼兼容模式 / 专属域名地址）时，WebSocket
    // 握手报的 "URL scheme not supported" 会被前端兜底成笼统的「操作失败」，
    // 用户无从定位。这里拦下并返回专用错误码，前端映射成可操作的提示。
    if !crate::asr::bailian::endpoint_scheme_is_websocket(&endpoint) {
        return Err("bailianEndpointSchemeInvalid".to_string());
    }
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::bailian::DEFAULT_MODEL.to_string());
    let vocabulary_id = scope
        .get(CredentialAccount::AsrVocabularyId)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty());
    let asr = std::sync::Arc::new(crate::asr::BailianRealtimeASR::new(
        crate::asr::BailianCredentials {
            api_key,
            endpoint,
            model,
            vocabulary_id,
        },
    ));
    asr.open_session().await.map_err(|e| e.to_string())?;
    // 验证音频必须 ≥200ms：只发 1 个 100ms 静音块时 DashScope 必然返回
    // task-failed: EmptyAudio，导致有效凭据也永远验证失败（2026-07 实测边界：
    // 100ms 拒、200ms 起收）。取 500ms 留余量，与 Mimo 验证的 250ms 同量级。
    crate::asr::AudioConsumer::consume_pcm_chunk(
        &*asr,
        &vec![0u8; crate::asr::bailian::TARGET_AUDIO_CHUNK_BYTES * 5],
    );
    asr.send_last_frame().await.map_err(|e| e.to_string())?;
    asr.await_final_result()
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn validate_qwen3_realtime_asr_provider(scope: &ProviderScope) -> Result<(), String> {
    let api_key = scope
        .get(CredentialAccount::AsrApiKey)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    // 统一百炼保留配置中的区域/工作空间主机，并切换到 Qwen Realtime 路径。
    let endpoint = if crate::coordinator::unified_bailian_is_active() {
        let endpoint = scope
            .get(CredentialAccount::AsrEndpoint)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        crate::coordinator::derive_bailian_endpoint(
            &endpoint,
            crate::coordinator::BailianEndpointProtocol::QwenRealtime,
        )?
    } else {
        scope
            .get(CredentialAccount::AsrEndpoint)
            .map_err(|e| e.to_string())?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| crate::asr::qwen_realtime::DEFAULT_ENDPOINT.to_string())
    };
    if !crate::asr::qwen_realtime::endpoint_scheme_is_secure_websocket(&endpoint) {
        return Err("qwen3EndpointSchemeInvalid".to_string());
    }
    let model = scope
        .get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::qwen_realtime::DEFAULT_MODEL.to_string());
    let asr = std::sync::Arc::new(crate::asr::Qwen3RealtimeASR::new(
        crate::asr::Qwen3RealtimeCredentials {
            api_key,
            endpoint,
            model,
        },
    ));
    asr.open_session().await.map_err(|e| e.to_string())?;
    // Realtime 协议对纯静音 + finish 干净返回 session.finished（2026-07 实测），
    // 无经典协议 <200ms 必报 EmptyAudio 的问题；发 500ms 与 bailian 验证对齐。
    crate::asr::AudioConsumer::consume_pcm_chunk(
        &*asr,
        &vec![0u8; crate::asr::qwen_realtime::TARGET_AUDIO_CHUNK_BYTES * 5],
    );
    asr.send_last_frame().await.map_err(|e| e.to_string())?;
    asr.await_final_result()
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(crate) fn active_asr_is_keyless_for_validation(provider: &str) -> bool {
    if cfg!(mobile) {
        return false;
    }
    provider == crate::asr::local::PROVIDER_ID
        || active_apple_speech_asr_is_supported(provider)
        || active_foundry_asr_is_supported(provider)
        || active_sherpa_asr_is_supported(provider)
}

pub(crate) fn active_apple_speech_asr_is_supported(provider: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = provider;
        false
    }
}

pub(crate) fn active_foundry_asr_is_supported(provider: &str) -> bool {
    #[cfg(all(not(mobile), target_os = "windows"))]
    {
        provider == FOUNDRY_LOCAL_PROVIDER_ID
    }
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    {
        let _ = provider;
        false
    }
}

pub(crate) fn active_sherpa_asr_is_supported(provider: &str) -> bool {
    #[cfg(all(not(mobile), target_os = "windows"))]
    {
        provider == crate::asr::local::sherpa::PROVIDER_ID
    }
    #[cfg(not(all(not(mobile), target_os = "windows")))]
    {
        let _ = provider;
        false
    }
}

async fn validate_asr_transcription(
    config: &ProviderConfig,
    model: &str,
    request_format: crate::asr::whisper::AsrRequestFormat,
) -> Result<(), String> {
    const MAX_ASR_VALIDATE_BODY_BYTES: usize = 1024 * 1024;
    const MAX_ATTEMPTS: u32 = 6;
    let url = asr_transcriptions_url(&config.base_url)?;
    let wav = encode_wav_16k_mono_silence(250);
    let client = http_client_builder(&url, 20)
        .build()
        .map_err(|_| "providerClientInitFailed".to_string())?;
    // 连接 / 请求未送出类失败做指数退避重试 —— 这类失败请求尚未送达服务端，重试
    // 安全。超时不重试（服务端可能已在处理）。multipart 是流式 body，每次重建。
    let mut attempt: u32 = 0;
    let response = loop {
        attempt += 1;
        let request = match request_format {
            crate::asr::whisper::AsrRequestFormat::Multipart => {
                let wav_part = reqwest::multipart::Part::bytes(wav.clone())
                    .file_name("openless-asr-check.wav")
                    .mime_str("audio/wav")
                    .map_err(|e| format!("请求体构建失败: {e}"))?;
                let form = reqwest::multipart::Form::new()
                    .part("file", wav_part)
                    .text("model", model.to_string());
                let mut request = client.post(&url);
                if !config.api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", config.api_key));
                }
                request.multipart(form)
            }
            crate::asr::whisper::AsrRequestFormat::OpenRouterJson => {
                // OpenRouter：application/json + base64（issue #582），与真实
                // 转写请求同形；不带 multipart 专属字段。
                let body = serde_json::json!({
                    "model": model,
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                        "format": "wav",
                    },
                });
                let mut request = client.post(&url);
                if !config.api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", config.api_key));
                }
                request.json(&body)
            }
            crate::asr::whisper::AsrRequestFormat::ZenMuxJson => {
                // ZenMux：application/json + base64，enable_itn 与真实请求默认
                // 一致（true），language 留空走服务端自动检测（issue #837）。
                let body = serde_json::json!({
                    "model": model,
                    "input_audio": {
                        "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                        "format": "wav",
                    },
                    "enable_itn": true,
                });
                let mut request = client.post(&url);
                if !config.api_key.trim().is_empty() {
                    request = request.header("Authorization", format!("Bearer {}", config.api_key));
                }
                request.json(&body)
            }
        };
        match request.send().await {
            Ok(resp) => break resp,
            Err(e) if e.is_timeout() => return Err("providerRequestTimeout".to_string()),
            Err(e) if (e.is_connect() || e.is_request()) && attempt < MAX_ATTEMPTS => {
                let backoff = (200u64 * 2u64.pow((attempt - 1).min(3))).min(900);
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                continue;
            }
            Err(_) => return Err("providerNetworkError".to_string()),
        }
    };
    let status = response.status();
    if !status.is_success() {
        // 探针音频是纯静音，有的厂商（StepFun）对无语音内容直接 400
        // "no speech found"。走到这一步说明鉴权（错 key 是 401）和模型名
        // （错模型是 404 model_invalid）都已通过、转写管线是通的——这类
        // 内容拒收判为验证成功，避免对静音敏感的厂商恒报假阴性。
        if status.as_u16() == 400 {
            let body = response.text().await.unwrap_or_default();
            if asr_error_is_no_speech_rejection(&body) {
                return Ok(());
            }
        }
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

/// 400 应答体是否是「音频里没有语音」类内容拒收（而非参数错误）。
/// 只匹配语义明确的措辞，宁可漏判（用户看到 400 后实测仍可用）也不误判
/// 真正的参数错误为成功。
fn asr_error_is_no_speech_rejection(body: &str) -> bool {
    body.to_ascii_lowercase().contains("no speech")
}

pub(crate) fn asr_transcriptions_url(base_url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(base_url.trim()).map_err(|_| "endpointInvalid".to_string())?;

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

fn sanitized_provider_destination(raw_url: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw_url.trim()) else {
        return "<invalid-provider-url>".to_string();
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.set_username("").is_err()
        || url.set_password(None).is_err()
    {
        return "<invalid-provider-url>".to_string();
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn provider_log_context(raw_url: &str, is_gemini: bool) -> String {
    format!(
        "GET {} (gemini={is_gemini})",
        sanitized_provider_destination(raw_url)
    )
}

fn provider_request_error_message(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "请求超时"
    } else if error.is_connect() {
        "网络连接失败"
    } else {
        "网络请求失败"
    }
}

pub(crate) async fn fetch_provider_models(config: &ProviderConfig) -> Result<Vec<String>, String> {
    let url = models_url(&config.base_url);
    let is_gemini = is_gemini_base_url(&config.base_url);
    let log_context = provider_log_context(&url, is_gemini);
    log::info!("[provider-check] {log_context}");
    let client = http_client_builder(&config.base_url, 15)
        .build()
        .map_err(|_| {
            log::warn!("[provider-check] {log_context} failed: client-init");
            "HTTP client 初始化失败".to_string()
        })?;
    // Observability uses only the sanitized copy above; requests retain the original URL.
    let mut request = client.get(&url);
    if !config.api_key.trim().is_empty() {
        // 谷歌原生 generativelanguage.googleapis.com 不识别 Bearer Authorization,
        // 必须用 x-goog-api-key 头。其它 OpenAI 兼容 provider 仍走 Bearer。
        if is_gemini {
            request = request.header("x-goog-api-key", config.api_key.as_str());
        } else {
            request = request.header("Authorization", format!("Bearer {}", config.api_key));
        }
    }
    for (k, v) in &config.extra_headers {
        request = request.header(k.as_str(), v.as_str());
    }
    let response = request.send().await.map_err(|error| {
        let message = provider_request_error_message(&error);
        log::warn!("[provider-check] {log_context} failed: {message}");
        message.to_string()
    })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        let reason = if error.is_timeout() {
            "response-timeout"
        } else {
            "response-read"
        };
        log::warn!("[provider-check] {log_context} failed: {reason}");
        "读取响应失败".to_string()
    })?;
    if !status.is_success() {
        return Err(format!("providerHttpStatus:{}", status.as_u16()));
    }
    if is_gemini {
        parse_gemini_model_ids(&body)
    } else {
        parse_model_ids(&body)
    }
}

pub(crate) fn is_gemini_base_url(base_url: &str) -> bool {
    base_url.contains("generativelanguage.googleapis.com")
}

pub(crate) fn models_url(base_url: &str) -> String {
    let trimmed = base_url.trim();
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        let fallback = trimmed.trim_end_matches('/');
        return format!("{fallback}/models");
    };
    let path = url.path().trim_end_matches('/');
    let next_path = if path.ends_with("/models") {
        path.to_string()
    } else if let Some(prefix) = path.strip_suffix("/chat/completions") {
        format!("{prefix}/models")
    } else {
        format!("{path}/models")
    };
    url.set_path(&next_path);
    url.to_string()
}

pub(crate) fn parse_model_ids(body: &str) -> Result<Vec<String>, String> {
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

/// 谷歌 v1beta/models 响应形状：`{models: [{name: "models/gemini-2.5-flash",
/// supportedGenerationMethods: ["generateContent", ...], ...}, ...]}`。
/// 与 OpenAI `{data: [{id: "..."}]}` 不兼容，所以单独解析；name 字段去掉
/// "models/" 前缀后即是 ProviderTools「拉取模型」按钮可直接写入 ark.model_id
/// 的字符串。
///
/// 过滤：只保留声明支持 `generateContent` 的模型——Google 的 model list 同时
/// 暴露 embedding (`gemini-embedding-2`)、TTS、image 等不支持
/// generateContent 的家族；用户选中那种 ID 后 polish 必失败（PR #398 pr_agent
/// 漏洞反馈）。`supportedGenerationMethods` 字段缺失时保守保留——某些 preview
/// 模型可能未暴露这个字段，宁误显示也不要把新模型挡在外面。
pub(crate) fn parse_gemini_model_ids(body: &str) -> Result<Vec<String>, String> {
    let json: Value =
        serde_json::from_str(body).map_err(|e| format!("模型列表不是有效 JSON: {e}"))?;
    let models = json
        .get("models")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Gemini 模型列表缺少 models 数组".to_string())?;
    let mut ids = models
        .iter()
        .filter(|item| {
            match item
                .get("supportedGenerationMethods")
                .and_then(|v| v.as_array())
            {
                Some(methods) => methods
                    .iter()
                    .any(|m| m.as_str() == Some("generateContent")),
                None => true, // 字段缺失：保守包含
            }
        })
        .filter_map(|item| item.get("name").and_then(|n| n.as_str()))
        .map(|name| {
            name.strip_prefix("models/")
                .unwrap_or(name)
                .trim()
                .to_string()
        })
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    // issue #609 F-01 孪生 gap（@claude 复审 #617）：ASR / provider 自定义 endpoint 也带
    // API Key 发请求，read_openai_provider_config（连通性测试 + 模型列表 chokepoint）现在复用
    // LLM 路径的 SSRF 校验。read_openai_provider_config 依赖凭据库无法纯单测，这里直接对它调用
    // 的校验器锁定 ASR 形态 endpoint 的拒绝/放行契约。
    use super::{
        asr_error_is_no_speech_rejection, fetch_provider_models, models_url,
        provider_llm_error_message, provider_log_context, provider_request_error_message,
        sanitized_provider_destination, send_dashscope_multimodal_validation,
        volcengine_missing_credential_error, ProviderConfig, ProviderScope,
    };
    use crate::endpoint_security::validate_http_endpoint;

    #[test]
    fn provider_scope_accepts_omni_without_channel() {
        assert!(ProviderScope::new("omni", None).is_ok());
    }

    #[test]
    fn provider_scope_rejects_channel_id_for_omni() {
        let error = ProviderScope::new("omni", Some("channel-1".to_string()))
            .err()
            .expect("omni must remain outside channel storage");
        assert_eq!(error, "omni provider does not support channel id");
    }

    #[test]
    fn provider_scope_rejects_unknown_kind() {
        let error = ProviderScope::new("unknown", None)
            .err()
            .expect("unknown provider kind must fail");
        assert_eq!(error, "unknown provider kind: unknown");
    }

    #[test]
    fn provider_scope_keeps_channel_ids_for_asr_and_llm() {
        for kind in ["asr", "llm"] {
            let scope = ProviderScope::new(kind, Some("channel-1".to_string()))
                .expect("channel provider kind must remain supported");
            assert_eq!(scope.channel.as_deref(), Some("channel-1"));
        }
    }

    #[test]
    fn volcengine_missing_credential_error_follows_auth_mode() {
        use crate::asr::volcengine::VolcengineAuthMode;
        // 旧版：先查 APP ID 再查 Access Token；全空格视为未填（trim 语义，
        // 与 VolcengineAuthMode::auth_ok 一致）。
        assert_eq!(
            volcengine_missing_credential_error(&VolcengineAuthMode::AppIdToken, "  ", "tok"),
            Some("volcengineAppIdMissing")
        );
        assert_eq!(
            volcengine_missing_credential_error(&VolcengineAuthMode::AppIdToken, "app", "  "),
            Some("volcengineAccessTokenMissing")
        );
        assert_eq!(
            volcengine_missing_credential_error(&VolcengineAuthMode::AppIdToken, "app", "tok"),
            None
        );
        // 新版控制台：只查 API Key，不要求 APP ID。
        assert_eq!(
            volcengine_missing_credential_error(&VolcengineAuthMode::ApiKey, "", "  "),
            Some("volcengineApiKeyMissing")
        );
        assert_eq!(
            volcengine_missing_credential_error(&VolcengineAuthMode::ApiKey, "", "key"),
            None
        );
    }

    #[test]
    fn silence_probe_content_rejection_is_not_a_credential_error() {
        // StepFun 对静音探针的实测应答（2026-07）：鉴权/模型都通过，只是探针
        // 音频没有语音内容——不能报成凭据错误。
        assert!(asr_error_is_no_speech_rejection(
            r#"{"error":{"message":"no speech found","type":"request_params_invalid"}}"#
        ));
        // 真正的参数错误不能被误判成功。
        assert!(!asr_error_is_no_speech_rejection(
            r#"{"error":{"message":"Request param: response_format is invalid","type":"input_invalid"}}"#
        ));
        assert!(!asr_error_is_no_speech_rejection(""));
    }

    #[test]
    fn provider_destination_redacts_userinfo_query_and_fragment() {
        let raw = "https://alice:password@example.com:8443/v1/models?api_key=query-secret#private-fragment";
        let destination = sanitized_provider_destination(raw);

        assert_eq!(destination, "https://example.com:8443/v1/models");
        for secret in [
            "alice",
            "password",
            "api_key",
            "query-secret",
            "private-fragment",
        ] {
            assert!(!destination.contains(secret), "destination leaked {secret}");
        }
    }

    #[test]
    fn provider_destination_preserves_normal_origin_path_and_port() {
        assert_eq!(
            sanitized_provider_destination("https://api.example.com:9443/v1/models"),
            "https://api.example.com:9443/v1/models"
        );
    }

    #[test]
    fn provider_destination_never_echoes_malformed_input() {
        let raw = "not a url?token=malformed-secret#private";
        let destination = sanitized_provider_destination(raw);

        assert_eq!(destination, "<invalid-provider-url>");
        assert!(!destination.contains("malformed-secret"));
        assert!(!provider_log_context(raw, false).contains("malformed-secret"));
    }

    #[test]
    fn provider_log_context_contains_only_the_sanitized_destination() {
        let context = provider_log_context(
            "https://user:pass@example.com/v1/models?token=query-secret#fragment-secret",
            true,
        );

        assert_eq!(context, "GET https://example.com/v1/models (gemini=true)");
        for secret in ["user", "pass", "query-secret", "fragment-secret"] {
            assert!(!context.contains(secret), "log context leaked {secret}");
        }
    }

    #[test]
    fn provider_validation_ipc_error_never_includes_network_details() {
        let secret = "https://user:pass@example.com/v1?token=query-secret#fragment";
        let message = provider_llm_error_message(crate::polish::LLMError::Network(format!(
            "request failed for {secret}"
        )));
        assert_eq!(message, "网络请求失败");
        assert!(!message.contains(secret));
    }

    #[tokio::test]
    async fn provider_request_error_does_not_echo_reqwest_url_secrets() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let raw =
            format!("http://user:password@{addr}/v1/models?token=query-secret#fragment-secret");
        let error = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&raw)
            .send()
            .await
            .expect_err("closed listener should reject the request");
        let message = provider_request_error_message(&error);

        assert_eq!(message, "网络连接失败");
        for secret in ["user", "password", "query-secret", "fragment-secret"] {
            assert!(!message.contains(secret), "IPC error leaked {secret}");
        }
    }

    #[tokio::test]
    async fn fetch_provider_models_keeps_url_secrets_out_of_ipc_errors() {
        use std::collections::HashMap;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let secrets = ["user", "password", "query-secret", "fragment-secret"];
        let error = fetch_provider_models(&ProviderConfig {
            base_url: format!(
                "http://{}:{}@{addr}/v1?token={}#{}",
                secrets[0], secrets[1], secrets[2], secrets[3]
            ),
            api_key: String::new(),
            extra_headers: HashMap::new(),
            temperature: None,
        })
        .await
        .expect_err("closed listener should reject the provider request");

        assert_eq!(error, "网络连接失败");
        for secret in secrets {
            assert!(!error.contains(secret), "IPC error leaked {secret}");
        }
    }

    #[test]
    fn models_url_appends_to_the_path_without_corrupting_query_or_fragment() {
        assert_eq!(
            models_url("https://example.com/v1?token=query-secret#client-fragment"),
            "https://example.com/v1/models?token=query-secret#client-fragment"
        );
        assert_eq!(
            models_url("https://example.com/v1/chat/completions?token=query-secret"),
            "https://example.com/v1/models?token=query-secret"
        );
    }

    #[tokio::test]
    async fn fetch_provider_models_preserves_the_original_request_query() {
        use std::collections::HashMap;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0, "client closed before sending request headers");
                request.extend_from_slice(&buffer[..count]);
            }
            let request_line = String::from_utf8(request)
                .unwrap()
                .lines()
                .next()
                .unwrap()
                .to_string();
            let body = r#"{"data":[{"id":"model-a"}]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            request_line
        });

        let models = fetch_provider_models(&ProviderConfig {
            base_url: format!("http://{addr}/v1?token=query-secret#client-fragment"),
            api_key: String::new(),
            extra_headers: HashMap::new(),
            temperature: None,
        })
        .await
        .unwrap();

        assert_eq!(models, vec!["model-a"]);
        assert_eq!(
            server.await.unwrap(),
            "GET /v1/models?token=query-secret HTTP/1.1"
        );
    }

    #[tokio::test]
    async fn dashscope_validation_does_not_follow_redirects_with_credentials() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let redirect_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect_addr = redirect_listener.local_addr().unwrap();
        let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let redirect_server = tokio::spawn(async move {
            let (mut stream, _) = redirect_listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer sk-test"));
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let target_server = tokio::spawn(async move {
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                target_listener.accept(),
            )
            .await
            .is_ok()
        });

        let error = send_dashscope_multimodal_validation(
            "sk-test",
            &format!("http://{redirect_addr}/validate"),
            &serde_json::json!({"model": "fun-asr-flash"}),
        )
        .await
        .unwrap_err();

        redirect_server.await.unwrap();
        assert_eq!(error, "providerHttpStatus:302");
        assert!(
            !target_server.await.unwrap(),
            "validation followed redirect"
        );
    }

    #[test]
    fn asr_endpoint_accepts_any_http_or_https_url() {
        // 地址选择权完全交给用户：公网 / 局域网 / 元数据地址一律放行，
        // 前端对 http:// 输入展示明文风险提示。
        validate_http_endpoint("http://169.254.169.254/v1/audio/transcriptions")
            .expect("用户显式配置的 endpoint 必须放行");
        validate_http_endpoint("http://100.64.0.1/v1/audio/transcriptions")
            .expect("用户显式配置的 endpoint 必须放行");
        validate_http_endpoint("http://api.example.com/v1/audio/transcriptions")
            .expect("公网 http ASR endpoint 必须放行");
        // 公网 https（如自建 Whisper 网关）放行。
        validate_http_endpoint("https://api.example.com/v1/audio/transcriptions")
            .expect("公网 https ASR endpoint 必须通过");
        // 本地 Whisper 服务：localhost / 127.0.0.1 http 放行。
        validate_http_endpoint("http://localhost:9000/v1").expect("本地 Whisper http 必须通过");
        validate_http_endpoint("http://127.0.0.1:9000/v1").expect("本地 Whisper http 必须通过");
        // 局域网（RFC1918）http ASR 网关放行（用户局域网自托管 Whisper）。
        validate_http_endpoint("http://192.168.1.50:9000/v1/audio/transcriptions")
            .expect("局域网 http ASR endpoint 必须通过");
        // Mimo 官方默认 endpoint（https）放行。
        validate_http_endpoint(crate::asr::mimo::DEFAULT_ENDPOINT)
            .expect("Mimo 官方默认 endpoint 必须通过");
    }

    #[test]
    fn asr_endpoint_rejects_malformed_or_non_http_urls() {
        assert!(validate_http_endpoint("not a url").is_err());
        assert!(validate_http_endpoint("ftp://example.com/").is_err());
        assert!(validate_http_endpoint("wss://example.com/").is_err());
    }
}
