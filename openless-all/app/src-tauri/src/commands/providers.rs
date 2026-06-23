use super::*;
use std::collections::HashMap;

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
    if kind == "asr" && CredentialsVault::get_active_asr() == crate::asr::bailian::PROVIDER_ID {
        return Ok(ProviderModelsResult {
            models: vec![crate::asr::bailian::DEFAULT_MODEL.to_string()],
        });
    }
    if kind == "asr" && CredentialsVault::get_active_asr() == crate::asr::mimo::PROVIDER_ID {
        return Ok(ProviderModelsResult {
            models: vec![crate::asr::mimo::DEFAULT_MODEL.to_string()],
        });
    }
    if kind == "llm" && CredentialsVault::get_active_llm() == CODEX_OAUTH_PROVIDER_ID {
        return Ok(ProviderModelsResult {
            models: vec![
                CODEX_DEFAULT_MODEL.to_string(),
                "gpt-5.3-codex".to_string(),
                "gpt-5.4".to_string(),
                "gpt-5.5".to_string(),
            ],
        });
    }
    let config = read_openai_provider_config(&kind)?;
    fetch_provider_models(&config)
        .await
        .map(|models| ProviderModelsResult { models })
}

pub(crate) struct ProviderConfig {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) extra_headers: HashMap<String, String>,
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
    let extra_headers = if kind == "llm" {
        CredentialsVault::get_active_llm_extra_headers()
    } else {
        HashMap::new()
    };
    if api_key_required && api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("Endpoint 为空".to_string());
    }
    // issue #609 F-01 孪生 gap（@claude 复审 #617 指出）：ASR / provider 自定义 endpoint
    // 同样是 attacker-controlled，且 ASR 请求也带 API Key。复用 LLM 路径已有的 SSRF 配置
    // 校验，拒绝指向内网/回环/link-local/CGNAT/IPv6 ULA/元数据服务的地址；localhost/
    // 127.0.0.1/::1 仍放行 http（本地 Whisper 服务）。覆盖 validate_provider_credentials
    // (asr/llm) 连通性测试与 list_provider_models 模型列表两条 HTTP 路径。
    crate::coordinator::validate_llm_endpoint(&base_url)
        .map_err(|_| "endpointInvalid".to_string())?;
    Ok(ProviderConfig {
        base_url,
        api_key,
        extra_headers,
    })
}

async fn validate_llm_provider() -> Result<(), String> {
    let llm_thinking_enabled = PreferencesStore::new()
        .map_err(|e| e.to_string())?
        .get()
        .llm_thinking_enabled;
    if CredentialsVault::get_active_llm() == CODEX_OAUTH_PROVIDER_ID {
        let model = CredentialsVault::get(CredentialAccount::ArkModelId)
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
                &[],
            )
            .await
            .map(|_| ())
            .map_err(|e| match e {
                LLMError::InvalidResponse { status, .. } => {
                    format!("providerHttpStatus:{status}")
                }
                other => other.to_string(),
            });
    }

    let config = read_openai_provider_config("llm")?;
    let active_llm = CredentialsVault::get_active_llm();
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)
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
            &[],
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

    if active_asr == crate::asr::bailian::PROVIDER_ID {
        return validate_bailian_asr_provider().await;
    }
    if active_asr == crate::asr::mimo::PROVIDER_ID {
        return validate_mimo_asr_provider().await;
    }

    let config = read_openai_provider_config("asr")?;
    let model = CredentialsVault::get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "asrModelMissing".to_string())?;
    validate_asr_transcription(&config, model.trim()).await
}

async fn validate_mimo_asr_provider() -> Result<(), String> {
    let config = read_openai_provider_config("asr")?;
    let model = CredentialsVault::get(CredentialAccount::AsrModel)
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

async fn validate_bailian_asr_provider() -> Result<(), String> {
    let api_key = CredentialsVault::get(CredentialAccount::AsrApiKey)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err("API Key 为空".to_string());
    }
    // 已知残留（issue #609 F-01 孪生 gap）：Bailian endpoint 走 `wss://`，与 http/https-only 的
    // validate_llm_endpoint 不兼容，无法直接复用，需单独的 ws/wss 感知 SSRF 校验器（超本次范围）。
    let endpoint = CredentialsVault::get(CredentialAccount::AsrEndpoint)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::bailian::DEFAULT_ENDPOINT.to_string());
    let model = CredentialsVault::get(CredentialAccount::AsrModel)
        .map_err(|e| e.to_string())?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| crate::asr::bailian::DEFAULT_MODEL.to_string());
    let vocabulary_id = CredentialsVault::get(CredentialAccount::AsrVocabularyId)
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
    crate::asr::AudioConsumer::consume_pcm_chunk(
        &*asr,
        &vec![0u8; crate::asr::bailian::TARGET_AUDIO_CHUNK_BYTES],
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

async fn validate_asr_transcription(config: &ProviderConfig, model: &str) -> Result<(), String> {
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
        let wav_part = reqwest::multipart::Part::bytes(wav.clone())
            .file_name("openless-asr-check.wav")
            .mime_str("audio/wav")
            .map_err(|e| format!("请求体构建失败: {e}"))?;
        let form = reqwest::multipart::Form::new()
            .part("file", wav_part)
            .text("model", model.to_string());
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .multipart(form)
            .send()
            .await
        {
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

pub(crate) async fn fetch_provider_models(config: &ProviderConfig) -> Result<Vec<String>, String> {
    let url = models_url(&config.base_url);
    let is_gemini = is_gemini_base_url(&config.base_url);
    log::info!("[provider-check] GET {url} (gemini={is_gemini})");
    let client = http_client_builder(&config.base_url, 15)
        .build()
        .map_err(|e| format!("HTTP client 初始化失败: {e}"))?;
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
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/models") {
        return trimmed.to_string();
    }
    if let Some(prefix) = trimmed.strip_suffix("/chat/completions") {
        return format!("{prefix}/models");
    }
    format!("{trimmed}/models")
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
    use crate::coordinator::validate_llm_endpoint;

    #[test]
    fn asr_endpoint_rejects_metadata_cgnat_and_non_https_public() {
        // 元数据 / CGNAT / 非 https 外网：拒绝，避免带 API Key 的 ASR 请求被指向高价值目标 / 明文外泄。
        assert!(validate_llm_endpoint("http://169.254.169.254/v1/audio/transcriptions").is_err());
        assert!(validate_llm_endpoint("http://100.64.0.1/v1/audio/transcriptions").is_err());
        assert!(validate_llm_endpoint("http://api.example.com/v1/audio/transcriptions").is_err());
    }

    #[test]
    fn asr_endpoint_accepts_public_https_localhost_and_lan() {
        // 公网 https（如自建 Whisper 网关）放行。
        validate_llm_endpoint("https://api.example.com/v1/audio/transcriptions")
            .expect("公网 https ASR endpoint 必须通过");
        // 本地 Whisper 服务：localhost / 127.0.0.1 http 放行。
        validate_llm_endpoint("http://localhost:9000/v1").expect("本地 Whisper http 必须通过");
        validate_llm_endpoint("http://127.0.0.1:9000/v1").expect("本地 Whisper http 必须通过");
        // F-01 放宽：局域网（RFC1918）http ASR 网关放行（用户局域网自托管 Whisper）。
        validate_llm_endpoint("http://192.168.1.50:9000/v1/audio/transcriptions")
            .expect("局域网 http ASR endpoint 必须通过");
        // Mimo 官方默认 endpoint（https）放行。
        validate_llm_endpoint(crate::asr::mimo::DEFAULT_ENDPOINT)
            .expect("Mimo 官方默认 endpoint 必须通过");
    }
}
