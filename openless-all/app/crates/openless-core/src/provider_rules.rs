//! Cross-host provider routing and request-shape rules.
//!
//! Platform adapters own sockets, native runtimes, credential access, and UI
//! authorization. This module owns the deterministic decisions that every host
//! must make identically once configuration values have been supplied.

use std::collections::HashMap;
use std::time::Duration;

use crate::credentials::ProviderType;
use crate::domains::ProviderKind;
use crate::errors::{BackendError, BackendErrorCode};

pub const OPENAI_COMPATIBLE_ASR_PROVIDER_ID: &str = "openai-compatible";
pub const ZENMUX_ASR_PROVIDER_ID: &str = "zenmux";

const BAILIAN_PROVIDER_ID: &str = "bailian";
const QWEN3_REALTIME_PROVIDER_ID: &str = "bailian-qwen3-realtime";
const STEPFUN_REALTIME_PROVIDER_ID: &str = "stepfun-realtime";
const MIMO_PROVIDER_ID: &str = "xiaomi-mimo-asr";
const DASHSCOPE_MULTIMODAL_PROVIDER_ID: &str = "bailian-fun-asr-flash";
const ELEVENLABS_PROVIDER_ID: &str = "elevenlabs";
const XFYUN_PROVIDER_ID: &str = "iflytek";

const ASR_PROVIDER_TYPES: &[(&str, &str)] = &[
    ("volcengine", "asrVolcengine"),
    ("elevenlabs", "asrElevenLabs"),
    ("bailian", "asrBailian"),
    ("bailian-qwen3-realtime", "asrBailianQwen3"),
    ("bailian-fun-asr-flash", "asrBailianFunAsrFlash"),
    ("siliconflow", "asrSiliconflow"),
    ("stepfun", "asrStepfun"),
    ("zhipu", "asrZhipu"),
    ("groq", "asrGroq"),
    ("whisper", "asrWhisper"),
    ("openrouter", "asrOpenrouter"),
    ("zenmux", "asrZenmux"),
    (OPENAI_COMPATIBLE_ASR_PROVIDER_ID, "asrOpenAiCompatible"),
    ("xiaomi-mimo-asr", "asrXiaomiMimo"),
    (XFYUN_PROVIDER_ID, "asrIflytek"),
    ("foundry-local-whisper", "asrFoundryLocalWhisper"),
    ("local-whisper", "asrLocalWhisper"),
    ("sherpa-onnx-local", "asrSherpaOnnxLocal"),
    ("local-qwen3-mlx", "asrLocalQwen3Mlx"),
    ("local-qwen3-c", "asrLocalQwen3C"),
    ("local-qwen3", "asrLocalQwen3"),
    ("apple-speech", "asrAppleSpeech"),
];

const LLM_PROVIDER_TYPES: &[(&str, &str)] = &[
    ("ark", "ark"),
    ("deepseek", "deepseek"),
    ("siliconflow", "siliconflow"),
    ("atlascloud", "atlascloud"),
    ("openai", "openai"),
    ("gemini", "gemini"),
    (crate::polish::CODEX_OAUTH_PROVIDER_ID, "codexOAuth"),
    ("mimo", "mimo"),
    ("cometapi", "cometapi"),
    ("openrouterFree", "openrouterFree"),
    ("alibabaCoding", "alibabaCoding"),
    ("codingPlanX", "codingPlanX"),
    ("minimax", "minimax"),
    ("stepfun", "stepfun"),
    ("custom", "custom"),
];

const OMNI_PROVIDER_TYPES: &[(&str, &str)] = &[
    ("openai", "omniOpenai"),
    ("gemini", "omniGemini"),
    ("dashscope-omni", "omniDashscope"),
    ("custom", "custom"),
];

const BAILIAN_MODELS: &[&str] = &[
    crate::asr::bailian::DEFAULT_MODEL,
    "fun-asr-flash-8k-realtime",
    crate::asr::qwen_realtime::DEFAULT_MODEL,
    "qwen3-asr-flash-realtime-2026-02-10",
    "qwen3-asr-flash-realtime-2025-10-27",
    crate::asr::dashscope_multimodal::QWEN_AUDIO_MODEL,
    crate::asr::dashscope_multimodal::DEFAULT_MODEL,
    "qwen3-asr-flash",
    "fun-asr",
    "fun-asr-2025-11-07",
    "fun-asr-2025-08-25",
    "fun-asr-mtl",
    "fun-asr-mtl-2025-08-25",
    "paraformer-v2",
];
const QWEN_REALTIME_MODELS: &[&str] = &[
    crate::asr::qwen_realtime::DEFAULT_MODEL,
    "qwen3-asr-flash-realtime-2026-02-10",
    "qwen3-asr-flash-realtime-2025-10-27",
];
const DASHSCOPE_MODELS: &[&str] = &[
    crate::asr::dashscope_multimodal::QWEN_AUDIO_MODEL,
    crate::asr::dashscope_multimodal::DEFAULT_MODEL,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRequirement {
    None,
    ApiKey,
    EndpointModelOptionalApiKey,
    ApiKeyUnlessCustomEndpoint,
    Volcengine,
    Xfyun,
    OAuth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationProbe {
    Unsupported,
    AsrSilence,
    AsrSilenceAllowsNoFinal,
    AsrNonSilent,
    StepfunNoSpeech,
    LlmText,
    OmniText,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub kind: ProviderKind,
    pub provider_type: ProviderType,
    pub label_key: String,
    pub default_endpoint: Option<String>,
    pub default_model: Option<String>,
    pub auth_requirement: AuthRequirement,
    pub validation_probe: ValidationProbe,
    pub static_models: Vec<String>,
}

pub fn provider_descriptors(kind: ProviderKind) -> Vec<ProviderDescriptor> {
    let providers = match kind {
        ProviderKind::Asr => ASR_PROVIDER_TYPES,
        ProviderKind::Llm => LLM_PROVIDER_TYPES,
        ProviderKind::Omni => OMNI_PROVIDER_TYPES,
    };
    providers
        .iter()
        .filter_map(|(provider_type, label_key)| {
            provider_descriptor_with_label(kind, provider_type, label_key)
        })
        .collect()
}

pub fn provider_descriptor(kind: ProviderKind, provider_type: &str) -> Option<ProviderDescriptor> {
    let label_key = match kind {
        ProviderKind::Asr => ASR_PROVIDER_TYPES,
        ProviderKind::Llm => LLM_PROVIDER_TYPES,
        ProviderKind::Omni => OMNI_PROVIDER_TYPES,
    }
    .iter()
    .find(|(candidate, _)| *candidate == provider_type)
    .map(|(_, label)| *label)
    .or((kind == ProviderKind::Llm).then_some("custom"))?;
    provider_descriptor_with_label(kind, provider_type, label_key)
}

fn provider_descriptor_with_label(
    kind: ProviderKind,
    provider_type: &str,
    label_key: &str,
) -> Option<ProviderDescriptor> {
    let provider_type = ProviderType::new(provider_type).ok()?;
    let id = provider_type.as_str().to_string();
    let (default_endpoint, default_model, auth_requirement, validation_probe) = match kind {
        ProviderKind::Asr if is_local_asr_provider(&id) => (
            None,
            None,
            AuthRequirement::None,
            ValidationProbe::Unsupported,
        ),
        ProviderKind::Asr => (
            default_asr_endpoint(&id),
            default_asr_model(&id),
            match id.as_str() {
                "volcengine" => AuthRequirement::Volcengine,
                XFYUN_PROVIDER_ID => AuthRequirement::Xfyun,
                OPENAI_COMPATIBLE_ASR_PROVIDER_ID => AuthRequirement::EndpointModelOptionalApiKey,
                _ => AuthRequirement::ApiKey,
            },
            match id.as_str() {
                "volcengine" | XFYUN_PROVIDER_ID => ValidationProbe::AsrSilenceAllowsNoFinal,
                "stepfun" => ValidationProbe::StepfunNoSpeech,
                DASHSCOPE_MULTIMODAL_PROVIDER_ID => ValidationProbe::AsrNonSilent,
                _ => ValidationProbe::AsrSilence,
            },
        ),
        ProviderKind::Llm => (
            default_llm_endpoint(&id),
            default_llm_model(&id),
            match id.as_str() {
                crate::polish::CODEX_OAUTH_PROVIDER_ID => AuthRequirement::OAuth,
                "gemini" => AuthRequirement::ApiKey,
                _ => AuthRequirement::ApiKeyUnlessCustomEndpoint,
            },
            ValidationProbe::LlmText,
        ),
        ProviderKind::Omni => (
            default_omni_endpoint(&id),
            default_omni_model(&id),
            AuthRequirement::ApiKey,
            ValidationProbe::OmniText,
        ),
    };
    Some(ProviderDescriptor {
        kind,
        provider_type,
        label_key: label_key.to_string(),
        default_endpoint: default_endpoint.map(str::to_string),
        default_model: default_model.map(str::to_string),
        auth_requirement,
        validation_probe,
        static_models: static_models(kind, &id)
            .iter()
            .map(|model| (*model).to_string())
            .collect(),
    })
}

pub fn validation_probe_for(
    kind: ProviderKind,
    provider_type: &str,
    model: Option<&str>,
) -> ValidationProbe {
    if kind == ProviderKind::Asr
        && provider_type == BAILIAN_PROVIDER_ID
        && model.and_then(dashscope_batch_protocol_for_model).is_some()
    {
        return ValidationProbe::AsrNonSilent;
    }
    provider_descriptor(kind, provider_type)
        .map(|descriptor| descriptor.validation_probe)
        .unwrap_or(ValidationProbe::Unsupported)
}

fn is_local_asr_provider(provider_type: &str) -> bool {
    matches!(
        provider_type,
        "foundry-local-whisper"
            | "local-whisper"
            | "sherpa-onnx-local"
            | "local-qwen3-mlx"
            | "local-qwen3-c"
            | "local-qwen3"
            | "apple-speech"
    )
}

fn static_models(kind: ProviderKind, provider_type: &str) -> &'static [&'static str] {
    match (kind, provider_type) {
        (ProviderKind::Asr, "bailian") => BAILIAN_MODELS,
        (ProviderKind::Asr, "bailian-qwen3-realtime") => QWEN_REALTIME_MODELS,
        (ProviderKind::Asr, "xiaomi-mimo-asr") => &[crate::asr::mimo::DEFAULT_MODEL],
        (ProviderKind::Asr, "bailian-fun-asr-flash") => DASHSCOPE_MODELS,
        (ProviderKind::Asr, "elevenlabs") => &[crate::asr::elevenlabs::DEFAULT_MODEL],
        (ProviderKind::Llm, crate::polish::CODEX_OAUTH_PROVIDER_ID) => &[
            crate::polish::CODEX_DEFAULT_MODEL,
            "gpt-5.3-codex",
            "gpt-5.4",
            "gpt-5.5",
        ],
        _ => &[],
    }
}

const BAILIAN_DEFAULT_ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference/";
const QWEN3_REALTIME_DEFAULT_ENDPOINT: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime";
const DASHSCOPE_MULTIMODAL_DEFAULT_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";
const DASHSCOPE_ASYNC_DEFAULT_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/transcription";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveAsrProviderKind {
    Bailian,
    Qwen3Realtime,
    StepfunRealtime,
    Mimo,
    DashScopeMultimodal,
    ElevenLabs,
    WhisperCompatible,
    Volcengine,
    Xfyun,
}

/// Non-secret facts read by a platform credential adapter. Core evaluates
/// configured state so every host applies the same provider requirements.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialConfiguration {
    pub asr_api_key: bool,
    pub asr_endpoint: bool,
    pub asr_model: bool,
    pub volcengine_auth_mode: Option<String>,
    pub volcengine_app_key: bool,
    pub volcengine_access_key: bool,
    pub volcengine_api_key: bool,
    pub volcengine_resource_id: bool,
    pub xfyun_app_id: bool,
    pub xfyun_api_key: bool,
    pub llm_api_key: bool,
    pub llm_endpoint: bool,
    pub llm_endpoint_matches_default: bool,
    pub llm_model: bool,
    pub codex_oauth: bool,
    pub omni_api_key: bool,
    pub omni_endpoint: bool,
    pub omni_model: bool,
}

pub fn volcengine_configured(configuration: &CredentialConfiguration) -> bool {
    use crate::asr::volcengine::VolcengineAuthMode;

    let credentials_ready = match configuration
        .volcengine_auth_mode
        .as_deref()
        .map(VolcengineAuthMode::parse)
        .unwrap_or(VolcengineAuthMode::AppIdToken)
    {
        VolcengineAuthMode::AppIdToken => {
            configuration.volcengine_app_key && configuration.volcengine_access_key
        }
        VolcengineAuthMode::ApiKey => configuration.volcengine_api_key,
    };
    credentials_ready && configuration.volcengine_resource_id
}

pub fn asr_configured(
    provider_id: &str,
    configuration: &CredentialConfiguration,
    local_runtime_configured: Option<bool>,
) -> bool {
    if let Some(configured) = local_runtime_configured {
        return configured;
    }
    provider_descriptor(ProviderKind::Asr, provider_id)
        .is_some_and(|descriptor| auth_requirement_satisfied(&descriptor, configuration))
}

pub fn llm_configured(provider_id: &str, configuration: &CredentialConfiguration) -> bool {
    provider_descriptor(ProviderKind::Llm, provider_id)
        .is_some_and(|descriptor| auth_requirement_satisfied(&descriptor, configuration))
}

pub fn omni_configured(provider_id: &str, configuration: &CredentialConfiguration) -> bool {
    provider_descriptor(ProviderKind::Omni, provider_id)
        .is_some_and(|descriptor| auth_requirement_satisfied(&descriptor, configuration))
}

pub fn auth_requirement_satisfied(
    descriptor: &ProviderDescriptor,
    configuration: &CredentialConfiguration,
) -> bool {
    let (api_key, endpoint, model) = match descriptor.kind {
        ProviderKind::Asr => (
            configuration.asr_api_key,
            configuration.asr_endpoint || descriptor.default_endpoint.is_some(),
            configuration.asr_model || descriptor.default_model.is_some(),
        ),
        ProviderKind::Llm => (
            configuration.llm_api_key,
            configuration.llm_endpoint || descriptor.default_endpoint.is_some(),
            configuration.llm_model || descriptor.default_model.is_some(),
        ),
        ProviderKind::Omni => (
            configuration.omni_api_key,
            configuration.omni_endpoint || descriptor.default_endpoint.is_some(),
            configuration.omni_model || descriptor.default_model.is_some(),
        ),
    };
    match descriptor.auth_requirement {
        AuthRequirement::None => true,
        AuthRequirement::ApiKey => api_key && endpoint && model,
        AuthRequirement::EndpointModelOptionalApiKey => endpoint && model,
        AuthRequirement::ApiKeyUnlessCustomEndpoint => {
            endpoint
                && model
                && (api_key
                    || (configuration.llm_endpoint && !configuration.llm_endpoint_matches_default))
        }
        AuthRequirement::Volcengine => volcengine_configured(configuration),
        AuthRequirement::Xfyun => configuration.xfyun_app_id && configuration.xfyun_api_key,
        AuthRequirement::OAuth => configuration.codex_oauth && model,
    }
}

pub fn api_key_required(
    kind: ProviderKind,
    provider_type: &str,
    configured_endpoint: Option<&str>,
) -> bool {
    let Some(descriptor) = provider_descriptor(kind, provider_type) else {
        return true;
    };
    match descriptor.auth_requirement {
        AuthRequirement::None
        | AuthRequirement::EndpointModelOptionalApiKey
        | AuthRequirement::OAuth => false,
        AuthRequirement::ApiKeyUnlessCustomEndpoint => {
            let Some(endpoint) = configured_endpoint.filter(|value| !value.trim().is_empty())
            else {
                return true;
            };
            descriptor
                .default_endpoint
                .as_deref()
                .is_some_and(|default| equivalent_endpoint(endpoint, default))
        }
        _ => true,
    }
}

pub fn equivalent_endpoint(left: &str, right: &str) -> bool {
    fn normalize(value: &str) -> &str {
        value
            .trim()
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions")
            .trim_end_matches('/')
    }
    normalize(left).eq_ignore_ascii_case(normalize(right))
}

pub fn default_asr_endpoint(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "elevenlabs" => Some("https://api.elevenlabs.io/v1"),
        "bailian" => Some(BAILIAN_DEFAULT_ENDPOINT),
        "bailian-qwen3-realtime" => Some(QWEN3_REALTIME_DEFAULT_ENDPOINT),
        "bailian-fun-asr-flash" => Some(DASHSCOPE_MULTIMODAL_DEFAULT_ENDPOINT),
        "siliconflow" => Some("https://api.siliconflow.cn/v1"),
        "stepfun" => Some("https://api.stepfun.com/v1"),
        "zhipu" => Some("https://open.bigmodel.cn/api/paas/v4"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "whisper" => Some("https://api.openai.com/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "zenmux" => Some("https://zenmux.ai/api/v1"),
        "xiaomi-mimo-asr" => Some("https://api.xiaomimimo.com/v1"),
        _ => None,
    }
}

pub fn default_asr_model(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "elevenlabs" => Some(crate::asr::elevenlabs::DEFAULT_MODEL),
        "bailian" => Some(crate::asr::bailian::DEFAULT_MODEL),
        "bailian-qwen3-realtime" => Some(crate::asr::qwen_realtime::DEFAULT_MODEL),
        "bailian-fun-asr-flash" => Some(crate::asr::dashscope_multimodal::DEFAULT_MODEL),
        "siliconflow" => Some("FunAudioLLM/SenseVoiceSmall"),
        "stepfun" => Some("stepaudio-2.5-asr"),
        "zhipu" => Some("glm-asr-2512"),
        "groq" => Some("whisper-large-v3-turbo"),
        "whisper" => Some("whisper-1"),
        "openrouter" => Some("openai/whisper-large-v3-turbo"),
        "zenmux" => Some(crate::asr::whisper::ZENMUX_DEFAULT_MODEL),
        "xiaomi-mimo-asr" => Some(crate::asr::mimo::DEFAULT_MODEL),
        _ => None,
    }
}

pub fn default_llm_endpoint(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "ark" => Some("https://ark.cn-beijing.volces.com/api/v3"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "siliconflow" => Some("https://api.siliconflow.cn/v1"),
        "atlascloud" => Some("https://api.atlascloud.ai/v1"),
        "openai" => Some("https://api.openai.com/v1"),
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta"),
        "mimo" => Some("https://api.xiaomimimo.com/v1"),
        "cometapi" => Some("https://api.cometapi.com/v1"),
        "openrouterFree" => Some("https://openrouter.ai/api/v1"),
        "alibabaCoding" => Some("https://coding-intl.dashscope.aliyuncs.com/v1"),
        "codingPlanX" => Some("https://api.codingplanx.ai/v1"),
        "minimax" => Some("https://api.minimaxi.com/v1"),
        "stepfun" => Some("https://api.stepfun.com/v1"),
        _ => None,
    }
}

pub fn default_llm_model(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "ark" => Some("deepseek-v3-2"),
        "deepseek" => Some("deepseek-v4-flash"),
        "siliconflow" => Some("Qwen/Qwen2.5-7B-Instruct"),
        "atlascloud" => Some("qwen/qwen3.5-flash"),
        "openai" | "cometapi" => Some("gpt-4o"),
        "gemini" => Some("gemini-2.5-flash"),
        crate::polish::CODEX_OAUTH_PROVIDER_ID => Some(crate::polish::CODEX_DEFAULT_MODEL),
        "mimo" => Some("xiaomi/mimo-v2-flash"),
        "openrouterFree" => Some("qwen/qwen3-coder:free"),
        "alibabaCoding" => Some("qwen3-coder-plus"),
        "codingPlanX" => Some("gpt-5-mini"),
        "minimax" => Some("MiniMax-M3"),
        "stepfun" => Some("step-1o-turbo-vision"),
        _ => None,
    }
}

pub fn default_omni_endpoint(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "openai" => Some("https://api.openai.com/v1"),
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta"),
        "dashscope-omni" => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        _ => None,
    }
}

pub fn default_omni_model(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        "openai" => Some("gpt-4o-audio-preview"),
        "gemini" => Some("gemini-2.5-flash"),
        "dashscope-omni" => Some("qwen3-omni-flash"),
        _ => None,
    }
}

pub fn parse_extra_headers(value: &str) -> Result<HashMap<String, String>, BackendError> {
    if value.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let headers: HashMap<String, String> = serde_json::from_str(value).map_err(|_| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            "provider extra headers must be a JSON object with string values",
        )
    })?;
    for name in headers.keys() {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "authorization" | "content-type" | "accept" | "host" | "content-length"
        ) {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                "provider extra headers contain a reserved header name",
            ));
        }
    }
    Ok(headers)
}

pub fn active_asr_provider_kind(id: &str) -> ActiveAsrProviderKind {
    match id {
        BAILIAN_PROVIDER_ID => ActiveAsrProviderKind::Bailian,
        QWEN3_REALTIME_PROVIDER_ID => ActiveAsrProviderKind::Qwen3Realtime,
        STEPFUN_REALTIME_PROVIDER_ID => ActiveAsrProviderKind::StepfunRealtime,
        MIMO_PROVIDER_ID => ActiveAsrProviderKind::Mimo,
        DASHSCOPE_MULTIMODAL_PROVIDER_ID => ActiveAsrProviderKind::DashScopeMultimodal,
        ELEVENLABS_PROVIDER_ID => ActiveAsrProviderKind::ElevenLabs,
        XFYUN_PROVIDER_ID => ActiveAsrProviderKind::Xfyun,
        value if is_whisper_compatible_provider(value) => ActiveAsrProviderKind::WhisperCompatible,
        _ => ActiveAsrProviderKind::Volcengine,
    }
}

pub fn is_bailian_provider(id: &str) -> bool {
    id == BAILIAN_PROVIDER_ID
}

pub fn is_qwen3_realtime_provider(id: &str) -> bool {
    id == QWEN3_REALTIME_PROVIDER_ID
}

pub fn is_stepfun_realtime_provider(id: &str) -> bool {
    id == STEPFUN_REALTIME_PROVIDER_ID
}

pub fn is_mimo_provider(id: &str) -> bool {
    id == MIMO_PROVIDER_ID
}

pub fn is_dashscope_multimodal_provider(id: &str) -> bool {
    id == DASHSCOPE_MULTIMODAL_PROVIDER_ID
}

pub fn is_elevenlabs_provider(id: &str) -> bool {
    id == ELEVENLABS_PROVIDER_ID
}

pub fn is_xfyun_provider(id: &str) -> bool {
    id == XFYUN_PROVIDER_ID
}

pub fn is_whisper_compatible_provider(id: &str) -> bool {
    matches!(
        id,
        "whisper" | "siliconflow" | "zhipu" | "groq" | "openrouter" | "stepfun" | "zenmux"
    ) || id == OPENAI_COMPATIBLE_ASR_PROVIDER_ID
}

pub fn resolve_effective_asr_provider(active_asr: &str, model: &str) -> Result<String, String> {
    if !is_bailian_provider(active_asr) {
        if is_dashscope_multimodal_provider(active_asr) {
            validate_dashscope_multimodal_model(model)?;
        }
        if active_asr == "stepfun" && stepfun_model_is_stream(model) {
            return Ok(STEPFUN_REALTIME_PROVIDER_ID.to_string());
        }
        return Ok(active_asr.to_string());
    }

    let model = model.trim();
    if model.is_empty() || is_classic_bailian_realtime_model(model) {
        Ok(BAILIAN_PROVIDER_ID.to_string())
    } else if model.starts_with("qwen3-asr-flash-realtime") {
        Ok(QWEN3_REALTIME_PROVIDER_ID.to_string())
    } else if dashscope_batch_protocol_for_model(model).is_some() {
        Ok(DASHSCOPE_MULTIMODAL_PROVIDER_ID.to_string())
    } else {
        Err(format!(
            "不支持的百炼 ASR 模型：{model}。支持 Fun-ASR、Paraformer、SenseVoice、qwen-audio-3.0-asr-flash 和 Qwen3-ASR 的实时、同步及录音文件模型"
        ))
    }
}

fn is_classic_bailian_realtime_model(model: &str) -> bool {
    model.starts_with("fun-asr-realtime")
        || model.starts_with("fun-asr-flash-8k-realtime")
        || model.starts_with("paraformer-realtime")
        || model.starts_with("paraformer-8k-realtime")
        || model.starts_with("sensevoice-realtime")
        || model.starts_with("sensevoice-8k-realtime")
}

pub fn stepfun_model_is_stream(model: &str) -> bool {
    model.trim().ends_with("-stream")
}

pub fn validate_dashscope_multimodal_model(model: &str) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() || dashscope_batch_protocol_for_model(model).is_some() {
        return Ok(());
    }
    Err(format!("不支持的 DashScope 录音文件 ASR 模型：{model}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashScopeBatchProtocol {
    Multimodal,
    AsyncTranscription,
}

pub fn dashscope_batch_protocol_for_model(model: &str) -> Option<DashScopeBatchProtocol> {
    let model = model.trim();
    if model.is_empty() || model.contains("realtime") {
        return None;
    }
    if model.starts_with("qwen3-asr-flash-filetrans") {
        return None;
    }
    let qwen_sync = dashscope_uses_qwen_sync_envelope(model);
    let qwen_audio = model.starts_with("qwen-audio") && !model.contains("streaming");
    if model.starts_with("fun-asr-flash") || qwen_sync || qwen_audio {
        return Some(DashScopeBatchProtocol::Multimodal);
    }
    if model == "fun-asr" || model.starts_with("fun-asr-") || model.starts_with("paraformer") {
        return Some(DashScopeBatchProtocol::AsyncTranscription);
    }
    None
}

pub fn dashscope_uses_qwen_sync_envelope(model: &str) -> bool {
    let model = model.trim();
    model.starts_with("qwen3-asr-flash")
        && !model.starts_with("qwen3-asr-flash-filetrans")
        && !model.contains("realtime")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BailianEndpointProtocol {
    ClassicRealtime,
    QwenRealtime,
    Multimodal,
    AsyncTranscription,
}

pub fn derive_bailian_endpoint(
    endpoint: &str,
    protocol: BailianEndpointProtocol,
) -> Result<String, String> {
    let default_endpoint = match protocol {
        BailianEndpointProtocol::ClassicRealtime => BAILIAN_DEFAULT_ENDPOINT,
        BailianEndpointProtocol::QwenRealtime => QWEN3_REALTIME_DEFAULT_ENDPOINT,
        BailianEndpointProtocol::Multimodal => DASHSCOPE_MULTIMODAL_DEFAULT_ENDPOINT,
        BailianEndpointProtocol::AsyncTranscription => DASHSCOPE_ASYNC_DEFAULT_ENDPOINT,
    };
    let source = if endpoint.trim().is_empty() {
        default_endpoint
    } else {
        endpoint.trim()
    };
    let mut url = url::Url::parse(source).map_err(|_| "endpointInvalid".to_string())?;
    if url.host_str().is_none() {
        return Err("endpointInvalid".to_string());
    }
    let (scheme, path) = match protocol {
        BailianEndpointProtocol::ClassicRealtime => ("wss", "/api-ws/v1/inference/"),
        BailianEndpointProtocol::QwenRealtime => ("wss", "/api-ws/v1/realtime"),
        BailianEndpointProtocol::Multimodal => (
            "https",
            "/api/v1/services/aigc/multimodal-generation/generation",
        ),
        BailianEndpointProtocol::AsyncTranscription => {
            ("https", "/api/v1/services/audio/asr/transcription")
        }
    };
    url.set_scheme(scheme)
        .map_err(|_| "endpointInvalid".to_string())?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvancedAsrConfig {
    pub verbose_json: bool,
    pub chunk_duration_ms: Option<u64>,
    pub enable_itn: bool,
}

impl Default for AdvancedAsrConfig {
    fn default() -> Self {
        Self {
            verbose_json: false,
            chunk_duration_ms: None,
            enable_itn: true,
        }
    }
}

pub fn parse_advanced_asr_config(raw: Option<&str>) -> AdvancedAsrConfig {
    let Some(raw) = raw else {
        return AdvancedAsrConfig::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return AdvancedAsrConfig::default();
    };
    AdvancedAsrConfig {
        verbose_json: value
            .get("verboseJson")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        chunk_duration_ms: value.get("chunkDurationMs").and_then(|value| {
            value.as_u64().filter(|millis| *millis > 0).or_else(|| {
                value
                    .as_f64()
                    .filter(|millis| {
                        millis.is_finite() && *millis > 0.0 && *millis <= u64::MAX as f64
                    })
                    .map(|millis| millis.floor() as u64)
            })
        }),
        enable_itn: value
            .get("enableItn")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    }
}

pub fn advanced_asr_config_for(provider_id: &str, raw: Option<&str>) -> AdvancedAsrConfig {
    if provider_id != OPENAI_COMPATIBLE_ASR_PROVIDER_ID && provider_id != ZENMUX_ASR_PROVIDER_ID {
        return AdvancedAsrConfig::default();
    }
    parse_advanced_asr_config(raw)
}

pub fn batch_asr_chunk_limit_ms(provider_id: &str, advanced: AdvancedAsrConfig) -> Option<u64> {
    match provider_id {
        "zhipu" | "openrouter" | "zenmux" => Some(30_000),
        _ => advanced.chunk_duration_ms,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrRequestFormat {
    Multipart,
    OpenRouterJson,
    ZenMuxJson,
}

pub fn whisper_request_format(provider_id: &str) -> AsrRequestFormat {
    match provider_id {
        "openrouter" => AsrRequestFormat::OpenRouterJson,
        "zenmux" => AsrRequestFormat::ZenMuxJson,
        _ => AsrRequestFormat::Multipart,
    }
}

pub fn whisper_uses_hotwords(provider_id: &str) -> bool {
    provider_id == "stepfun"
}

pub fn whisper_supports_verbose_json(provider_id: &str, advanced: AdvancedAsrConfig) -> bool {
    match provider_id {
        "whisper" | "groq" => true,
        "zenmux" => false,
        _ => advanced.verbose_json,
    }
}

pub fn zenmux_language_code(native_name: &str) -> Option<String> {
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

pub fn volc_resource_history_label(resource_id: &str) -> Option<String> {
    let id = resource_id.trim();
    let allowed = id.starts_with("volc.")
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    allowed.then(|| id.to_string())
}

/// 1.x native ASR 的动态预算保留在 Core，Host 只负责执行 deadline 后的原生取消。
/// MLX/C 和 Apple Speech 给短音频 30 秒余量；Whisper Metal 保留 15 秒地板；
/// Windows batch 的 CPU/GPU 回退各自消费完整预算，不能再套一个更短的外层计时器。
pub fn native_transcribe_timeout(provider_type: &str, duration_ms: u64) -> Duration {
    let (numerator, denominator, extra, minimum) = match provider_type {
        "local-whisper" | "apple-whisper" => (1_u64, 2_000_u64, 10, 15),
        "local-qwen3" | "local-qwen3-mlx" | "local-qwen3-c" | "apple-speech" => (3, 5_000, 10, 30),
        _ => (1, 1_000, 20, 30),
    };
    let seconds = duration_ms
        .saturating_mul(numerator)
        .div_ceil(denominator)
        .saturating_add(extra)
        .max(minimum);
    Duration::from_secs(seconds)
}

pub fn whisper_transcribe_timeout(audio_secs: f64) -> Duration {
    let secs = ((audio_secs * 0.5).ceil() as u64)
        .saturating_add(20)
        .max(30);
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_bailian_and_stepfun_models() {
        assert_eq!(
            resolve_effective_asr_provider(BAILIAN_PROVIDER_ID, "fun-asr-realtime").unwrap(),
            BAILIAN_PROVIDER_ID
        );
        assert_eq!(
            resolve_effective_asr_provider(BAILIAN_PROVIDER_ID, "qwen3-asr-flash-realtime")
                .unwrap(),
            QWEN3_REALTIME_PROVIDER_ID
        );
        assert_eq!(
            resolve_effective_asr_provider(BAILIAN_PROVIDER_ID, "fun-asr-flash-2026-06-15")
                .unwrap(),
            DASHSCOPE_MULTIMODAL_PROVIDER_ID
        );
        assert_eq!(
            resolve_effective_asr_provider("stepfun", "stepaudio-2.5-asr-stream").unwrap(),
            STEPFUN_REALTIME_PROVIDER_ID
        );
        assert!(resolve_effective_asr_provider(BAILIAN_PROVIDER_ID, "unknown-asr").is_err());
    }

    #[test]
    fn derives_bailian_protocol_endpoints_without_leaking_source_paths() {
        let source = "https://workspace.ap-southeast-1.maas.aliyuncs.com/custom?x=1";
        assert_eq!(
            derive_bailian_endpoint(source, BailianEndpointProtocol::ClassicRealtime).unwrap(),
            "wss://workspace.ap-southeast-1.maas.aliyuncs.com/api-ws/v1/inference/"
        );
        assert_eq!(
            derive_bailian_endpoint(source, BailianEndpointProtocol::AsyncTranscription).unwrap(),
            "https://workspace.ap-southeast-1.maas.aliyuncs.com/api/v1/services/audio/asr/transcription"
        );
    }

    #[test]
    fn advanced_config_is_scoped_and_conservative() {
        let parsed = advanced_asr_config_for(
            OPENAI_COMPATIBLE_ASR_PROVIDER_ID,
            Some(r#"{"verboseJson":true,"chunkDurationMs":30000.9,"enableItn":false}"#),
        );
        assert!(parsed.verbose_json);
        assert_eq!(parsed.chunk_duration_ms, Some(30_000));
        assert!(!parsed.enable_itn);
        assert_eq!(
            advanced_asr_config_for("whisper", Some(r#"{"verboseJson":true}"#)),
            AdvancedAsrConfig::default()
        );
    }

    #[test]
    fn request_shape_and_timeout_rules_are_stable() {
        assert_eq!(
            whisper_request_format("openrouter"),
            AsrRequestFormat::OpenRouterJson
        );
        assert_eq!(
            whisper_request_format("zenmux"),
            AsrRequestFormat::ZenMuxJson
        );
        assert_eq!(
            batch_asr_chunk_limit_ms("openrouter", AdvancedAsrConfig::default()),
            Some(30_000)
        );
        assert_eq!(whisper_transcribe_timeout(10.0), Duration::from_secs(30));
        assert_eq!(whisper_transcribe_timeout(60.0), Duration::from_secs(50));
    }

    #[test]
    fn configured_state_uses_one_cross_host_provider_policy() {
        let mut configuration = CredentialConfiguration {
            asr_api_key: true,
            llm_endpoint: true,
            llm_endpoint_matches_default: true,
            llm_model: true,
            omni_api_key: true,
            omni_model: true,
            ..CredentialConfiguration::default()
        };
        assert!(asr_configured(BAILIAN_PROVIDER_ID, &configuration, None));
        assert!(!llm_configured("openrouterFree", &configuration));
        configuration.llm_api_key = true;
        assert!(llm_configured("openrouterFree", &configuration));
        configuration.llm_api_key = false;
        configuration.llm_endpoint_matches_default = false;
        assert!(llm_configured("openrouterFree", &configuration));
        assert!(omni_configured("gemini", &configuration));

        configuration.volcengine_auth_mode = Some("api_key".into());
        configuration.volcengine_api_key = true;
        configuration.volcengine_resource_id = true;
        assert!(volcengine_configured(&configuration));
        assert!(!asr_configured(
            "foundry-local-whisper",
            &configuration,
            Some(false)
        ));
        assert!(equivalent_endpoint(
            "https://api.openai.com/v1/chat/completions/",
            default_llm_endpoint("openai").unwrap()
        ));
        assert_eq!(default_llm_model("gemini"), Some("gemini-2.5-flash"));
        assert_eq!(
            default_omni_endpoint("dashscope-omni"),
            Some("https://dashscope.aliyuncs.com/compatible-mode/v1")
        );
        assert!(parse_extra_headers(r#"{"x-trace":"enabled"}"#).is_ok());
        assert!(parse_extra_headers(r#"{"authorization":"secret"}"#).is_err());
    }

    #[test]
    fn descriptors_are_the_single_source_for_defaults_auth_and_probes() {
        let compatible = provider_descriptor(ProviderKind::Asr, "openai-compatible").unwrap();
        assert_eq!(
            compatible.auth_requirement,
            AuthRequirement::EndpointModelOptionalApiKey
        );
        assert_eq!(compatible.default_endpoint, None);
        assert_eq!(compatible.default_model, None);

        let mut configuration = CredentialConfiguration {
            asr_endpoint: true,
            asr_model: true,
            ..CredentialConfiguration::default()
        };
        assert!(auth_requirement_satisfied(&compatible, &configuration));
        configuration.asr_endpoint = false;
        assert!(!auth_requirement_satisfied(&compatible, &configuration));

        let stepfun = provider_descriptor(ProviderKind::Asr, "stepfun").unwrap();
        assert_eq!(stepfun.validation_probe, ValidationProbe::StepfunNoSpeech);
        assert!(api_key_required(
            ProviderKind::Asr,
            "stepfun",
            Some("https://api.stepfun.com/v1")
        ));

        let dashscope =
            provider_descriptor(ProviderKind::Asr, DASHSCOPE_MULTIMODAL_PROVIDER_ID).unwrap();
        assert_eq!(dashscope.validation_probe, ValidationProbe::AsrNonSilent);
        assert!(!dashscope.static_models.is_empty());
    }

    #[test]
    fn custom_llm_auth_depends_on_the_effective_endpoint() {
        assert!(!api_key_required(
            ProviderKind::Llm,
            "custom",
            Some("http://127.0.0.1:8080/v1")
        ));
        assert!(api_key_required(
            ProviderKind::Llm,
            "openai",
            Some("https://api.openai.com/v1/chat/completions")
        ));
        assert!(!api_key_required(
            ProviderKind::Llm,
            "openai",
            Some("http://127.0.0.1:8080/v1")
        ));
    }

    #[test]
    fn provider_descriptor_catalogs_have_unique_protocol_ids() {
        for kind in [ProviderKind::Asr, ProviderKind::Llm, ProviderKind::Omni] {
            let descriptors = provider_descriptors(kind);
            let unique = descriptors
                .iter()
                .map(|descriptor| descriptor.provider_type.as_str())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(unique.len(), descriptors.len());
        }
    }

    #[test]
    fn secret_like_volc_resource_ids_are_not_attributed() {
        assert_eq!(
            volc_resource_history_label("volc.seedasr.sauc.duration").as_deref(),
            Some("volc.seedasr.sauc.duration")
        );
        assert_eq!(volc_resource_history_label("my-secret-tenant"), None);
        assert_eq!(volc_resource_history_label("volc.a b"), None);
    }
    #[test]
    fn native_asr_deadlines_preserve_short_floor_and_long_audio_budget() {
        for provider in [
            "local-qwen3",
            "local-qwen3-mlx",
            "local-qwen3-c",
            "apple-speech",
        ] {
            assert_eq!(
                native_transcribe_timeout(provider, 1_000),
                Duration::from_secs(30)
            );
            assert_eq!(
                native_transcribe_timeout(provider, 60_001),
                Duration::from_secs(47)
            );
        }
        for provider in ["local-whisper", "apple-whisper"] {
            assert_eq!(
                native_transcribe_timeout(provider, 1_000),
                Duration::from_secs(15)
            );
            assert_eq!(
                native_transcribe_timeout(provider, 60_001),
                Duration::from_secs(41)
            );
        }
        for provider in ["foundry-local-whisper", "sherpa-onnx-local"] {
            assert_eq!(
                native_transcribe_timeout(provider, 1_000),
                Duration::from_secs(30)
            );
            assert_eq!(
                native_transcribe_timeout(provider, 60_001),
                Duration::from_secs(81)
            );
        }
    }
}
