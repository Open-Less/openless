//! 阿里云百炼（DashScope）多模态生成同步接口的批量 ASR 客户端。
//!
//! `fun-asr-flash` 与 `qwen-audio-3.0-asr-flash` 系列是**非实时录音文件识别**
//! 模型，走 DashScope 私有的
//! `multimodal-generation/generation` HTTP 接口，既不是实时 WebSocket 双工
//! （见 `bailian.rs`），也不是 OpenAI 兼容的 `/audio/transcriptions`
//! （见 `whisper.rs`）。因此单独成一路批量客户端：录音结束后把整段 PCM 编成
//! WAV、base64 进 JSON body、POST 一次拿整段文本。
//!
//! 结构与 `mimo.rs`（同为「攒 PCM → POST 一段音频 → 解析私有 JSON」）一致，
//! 复用其 `split_pcm_by_duration` / `join_transcript_chunks` 分片与拼接逻辑，
//! 只有请求信封与响应解析不同。

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::asr::mimo::{join_transcript_chunks, split_pcm_by_duration};
use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

// fun-asr-flash 单条音频上限 5 分钟；但真正的硬约束是 base64 进 JSON 的请求体
// 体积。沿用 mimo 验证过的 180s 预算（16k/16-bit/mono WAV base64 后约 7.7MB），
// 稳稳落在时长和常见网关体积上限之内。超长录音按此切分后逐段识别再拼接。
const DASHSCOPE_MAX_CHUNK_DURATION_MS: u64 = 180_000;
const ASYNC_TASK_POLL_TIMEOUT_SECS: u64 = 600;
const ASYNC_WORKFLOW_OVERHEAD_SECS: u64 = 60;
const ASYNC_UPLOAD_BYTES_PER_SEC: u64 = 64 * 1024;

pub const PROVIDER_ID: &str = "bailian-fun-asr-flash";
pub const DEFAULT_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation";
pub const ASYNC_DEFAULT_ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/transcription";
pub const DEFAULT_MODEL: &str = "fun-asr-flash-2026-06-15";
pub const QWEN_AUDIO_MODEL: &str = "qwen-audio-3.0-asr-flash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashScopeBatchProtocol {
    Multimodal,
    AsyncTranscription,
}

fn is_realtime_model(model: &str) -> bool {
    model.contains("realtime")
}

fn is_qwen_filetrans_model(model: &str) -> bool {
    model.starts_with("qwen3-asr-flash-filetrans")
}

fn is_qwen_sync_model(model: &str) -> bool {
    model.starts_with("qwen3-asr-flash")
        && !is_qwen_filetrans_model(model)
        && !is_realtime_model(model)
}

fn is_qwen_audio_model(model: &str) -> bool {
    // 同步录音文件模型；`-streaming` 流式变体不在批量协议支持范围内。
    model.starts_with("qwen-audio") && !model.contains("streaming")
}

pub fn protocol_for_model(model: &str) -> Option<DashScopeBatchProtocol> {
    let model = model.trim();
    if model.is_empty() || is_realtime_model(model) {
        return None;
    }
    // qwen3-asr-flash-filetrans 官方仅接受公网音频 URL，与本地录音的临时 OSS
    // 上传 + oss:// 链路不兼容，暂不纳入支持：显式拒绝，避免被误路由到异步协议
    // 造成「验证通过但真实录音必然失败」。
    if is_qwen_filetrans_model(model) {
        return None;
    }
    if model.starts_with("fun-asr-flash")
        || is_qwen_sync_model(model)
        || is_qwen_audio_model(model)
    {
        return Some(DashScopeBatchProtocol::Multimodal);
    }
    if model == "fun-asr" || model.starts_with("fun-asr-") || model.starts_with("paraformer") {
        return Some(DashScopeBatchProtocol::AsyncTranscription);
    }
    None
}

pub struct DashScopeMultimodalASR {
    api_key: String,
    base_url: String,
    model: String,
    buffer: Mutex<Vec<u8>>,
}

impl DashScopeMultimodalASR {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn buffer_duration_ms(&self) -> u64 {
        crate::asr::pcm::pcm_duration_ms(&self.buffer.lock())
    }

    pub fn transcribe_timeout(&self, audio_secs: f64) -> Duration {
        if protocol_for_model(&self.model) == Some(DashScopeBatchProtocol::AsyncTranscription) {
            let pcm_bytes = (audio_secs.max(0.0) * 32_000.0).ceil() as u64;
            return async_upload_timeout(pcm_bytes.saturating_add(44))
                + Duration::from_secs(
                    ASYNC_TASK_POLL_TIMEOUT_SECS + ASYNC_WORKFLOW_OVERHEAD_SECS,
                );
        }
        let secs = ((audio_secs * 0.5).ceil() as u64)
            .saturating_add(20)
            .max(30);
        Duration::from_secs(secs)
    }

    pub async fn transcribe(&self) -> Result<RawTranscript> {
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm).await;
        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(&self, pcm: &[u8]) -> Result<RawTranscript> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("DashScope API key missing");
        }

        let duration_ms = crate::asr::pcm::pcm_duration_ms(pcm);
        if protocol_for_model(&self.model) == Some(DashScopeBatchProtocol::AsyncTranscription) {
            let samples: Vec<i16> = pcm
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            let wav = encode_wav_16k_mono(&samples);
            let text = self.transcribe_async(&wav).await?;
            return Ok(RawTranscript { text, duration_ms });
        }
        let chunks = split_pcm_by_duration(pcm, DASHSCOPE_MAX_CHUNK_DURATION_MS);
        let mut texts = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            texts.push(self.transcribe_chunk(chunk).await?);
        }

        Ok(RawTranscript {
            text: join_transcript_chunks(&texts),
            duration_ms,
        })
    }

    async fn transcribe_chunk(&self, pcm: &[u8]) -> Result<String> {
        let samples: Vec<i16> = pcm
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let body = dashscope_multimodal_body(&self.model, &wav);
        let url = generation_url(&self.base_url)?;
        let request_timeout = self.transcribe_timeout(crate::asr::pcm::pcm_duration_ms(pcm) as f64 / 1000.0);
        let resp = crate::net::credential_http()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .header("Content-Type", "application/json")
            // multimodal-generation 默认可 SSE 流式；显式关掉走一次性 JSON 响应。
            .header("X-DashScope-SSE", "disable")
            .json(&body)
            .timeout(request_timeout)
            .send()
            .await
            .context("DashScope ASR HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DashScope ASR API error {}: {}", status, body);
        }

        let json: Value = resp.json().await.context("parse DashScope ASR response")?;
        Ok(extract_dashscope_text(&json).trim().to_string())
    }

    async fn transcribe_async(&self, wav: &[u8]) -> Result<String> {
        let file_url = self.upload_temporary_wav(wav).await?;
        self.transcribe_async_url(&file_url).await
    }

    async fn upload_temporary_wav(&self, wav: &[u8]) -> Result<String> {
        let mut policy_url = api_url(&self.base_url, "/api/v1/uploads")?;
        policy_url
            .query_pairs_mut()
            .append_pair("action", "getPolicy")
            .append_pair("model", self.model.trim());
        let response = crate::net::credential_http()
            .get(policy_url)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("request DashScope temporary upload policy")?;
        let policy_json = response_json(response, "DashScope upload policy").await?;
        let data = policy_json
            .get("data")
            .context("DashScope upload policy missing data")?;
        let field = |name: &str| -> Result<String> {
            data.get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .with_context(|| format!("DashScope upload policy missing {name}"))
        };
        let upload_dir = field("upload_dir")?;
        let object_key = format!("{}/audio.wav", upload_dir.trim_end_matches('/'));
        let form = reqwest::multipart::Form::new()
            .text("OSSAccessKeyId", field("oss_access_key_id")?)
            .text("policy", field("policy")?)
            .text("Signature", field("signature")?)
            .text("key", object_key.clone())
            .text("x-oss-object-acl", field("x_oss_object_acl")?)
            .text("x-oss-forbid-overwrite", field("x_oss_forbid_overwrite")?)
            .text("success_action_status", "200")
            .part(
                "file",
                reqwest::multipart::Part::bytes(wav.to_vec())
                    .file_name("audio.wav")
                    .mime_str("audio/wav")?,
            );
        let upload_url = dashscope_transfer_url(&field("upload_host")?)?;
        let upload = crate::net::anonymous_no_redirect_http()
            .post(upload_url)
            .multipart(form)
            .timeout(async_upload_timeout(wav.len() as u64))
            .send()
            .await
            .context("upload audio to DashScope temporary storage")?;
        ensure_success(upload, "DashScope temporary upload").await?;
        Ok(format!("oss://{object_key}"))
    }

    pub async fn transcribe_async_url(&self, file_url: &str) -> Result<String> {
        self.transcribe_async_url_with_timeout(
            file_url,
            Duration::from_secs(ASYNC_TASK_POLL_TIMEOUT_SECS),
        )
        .await
    }

    /// 提交异步任务并轮询至完成。`poll_timeout` 是任务轮询阶段的硬截止时间：
    /// 真实转写用长轮询（默认 600s），连通性验证用短轮询以便快速返回，避免
    /// 「验证」按钮在最坏情况下阻塞近 11 分钟。
    pub async fn transcribe_async_url_with_timeout(
        &self,
        file_url: &str,
        poll_timeout: Duration,
    ) -> Result<String> {
        let submit_url = async_transcription_url(&self.base_url)?;
        let response = crate::net::credential_http()
            .post(submit_url)
            .header("Authorization", format!("Bearer {}", self.api_key.trim()))
            .header("Content-Type", "application/json")
            .header("X-DashScope-Async", "enable")
            .header("X-DashScope-OssResourceResolve", "enable")
            .json(&async_transcription_body(&self.model, file_url))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("submit DashScope async ASR task")?;
        let submitted = response_json(response, "DashScope async ASR submission").await?;
        let task_id = submitted
            .pointer("/output/task_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .context("DashScope async ASR response missing task_id")?;
        let task_url = api_url(&self.base_url, &format!("/api/v1/tasks/{task_id}"))?;
        let deadline = Instant::now() + poll_timeout;
        let completed = loop {
            // 轮询窗口最长可达 600s、每秒一次：对瞬态网络失败做有界重试，
            // 避免 10 分钟内单次连接抖动/5xx 直接废弃整段转写。
            let task = get_json_with_retry(
                crate::net::credential_http(),
                task_url.clone(),
                Some(self.api_key.trim()),
                deadline,
                "poll DashScope async ASR task",
            )
            .await?;
            match task
                .pointer("/output/task_status")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "SUCCEEDED" => break task,
                "FAILED" | "CANCELED" | "UNKNOWN" => {
                    let message = task
                        .get("message")
                        .or_else(|| task.pointer("/output/message"))
                        .and_then(Value::as_str)
                        .unwrap_or("task failed");
                    anyhow::bail!("DashScope async ASR task failed: {message}");
                }
                _ if Instant::now() >= deadline => {
                    anyhow::bail!("DashScope async ASR task timed out");
                }
                _ => tokio::time::sleep(Duration::from_secs(1)).await,
            }
        };
        let result = download_async_result(&extract_async_result_url(&completed)?).await?;
        extract_async_transcript_text(&result)
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

fn api_url(base_url: &str, path: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(base_url.trim()).context("parse DashScope base URL")?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn dashscope_transfer_url(raw: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(raw.trim()).context("parse DashScope transfer URL")?;
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("DashScope transfer URL must not contain credentials");
    }
    let host = url
        .host_str()
        .context("DashScope transfer URL missing host")?
        .to_ascii_lowercase();

    #[cfg(test)]
    if host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    {
        return Ok(url);
    }

    if host != "aliyuncs.com" && !host.ends_with(".aliyuncs.com") {
        anyhow::bail!("DashScope transfer URL must use an Alibaba Cloud OSS host");
    }
    match url.scheme() {
        "https" => {}
        "http" => url
            .set_scheme("https")
            .map_err(|_| anyhow::anyhow!("upgrade DashScope transfer URL to HTTPS"))?,
        _ => anyhow::bail!("DashScope transfer URL must use HTTPS"),
    }
    Ok(url)
}

fn async_upload_timeout(bytes: u64) -> Duration {
    let transfer_secs = bytes
        .saturating_add(ASYNC_UPLOAD_BYTES_PER_SEC - 1)
        / ASYNC_UPLOAD_BYTES_PER_SEC;
    Duration::from_secs(transfer_secs.saturating_add(30).max(60))
}

async fn download_async_result(raw_url: &str) -> Result<Value> {
    let result_url = dashscope_transfer_url(raw_url)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    get_json_with_retry(
        crate::net::anonymous_no_redirect_http(),
        result_url,
        None,
        deadline,
        "download DashScope async ASR result",
    )
    .await
}

/// GET JSON 请求的瞬态失败重试上限（指数退避 500ms / 1s / 2s / 4s）。
const ASYNC_HTTP_RETRY_ATTEMPTS: u32 = 3;

fn retry_backoff(attempts: u32) -> Duration {
    Duration::from_millis((500u64 * 2u64.pow(attempts.min(3))).min(4000))
}

/// 带瞬态重试的 GET JSON。
///
/// 连接失败 / 超时 / 请求阶段错误 / 5xx / 429 视为瞬态：指数退避重试，最多
/// `ASYNC_HTTP_RETRY_ATTEMPTS` 次且不晚于 `deadline`（GET 幂等，重试安全）。
/// 4xx 与确定性错误立即返回；`api_key` 为 Some 时附带 Bearer 头。
async fn get_json_with_retry(
    client: &'static reqwest::Client,
    url: reqwest::Url,
    api_key: Option<&str>,
    deadline: Instant,
    operation: &'static str,
) -> Result<Value> {
    let mut attempts: u32 = 0;
    loop {
        let mut request = client.get(url.clone()).timeout(Duration::from_secs(30));
        if let Some(key) = api_key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                return response
                    .json()
                    .await
                    .with_context(|| format!("parse {operation} response"));
            }
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let transient = status.is_server_error()
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
                if !transient || attempts >= ASYNC_HTTP_RETRY_ATTEMPTS || Instant::now() >= deadline
                {
                    anyhow::bail!("{operation} error {status}: {body}");
                }
                attempts += 1;
                tokio::time::sleep(retry_backoff(attempts)).await;
            }
            Err(err) => {
                let transient = err.is_timeout() || err.is_connect() || err.is_request();
                if !transient || attempts >= ASYNC_HTTP_RETRY_ATTEMPTS || Instant::now() >= deadline
                {
                    return Err(err).with_context(|| format!("{operation} request failed"));
                }
                attempts += 1;
                tokio::time::sleep(retry_backoff(attempts)).await;
            }
        }
    }
}

async fn ensure_success(response: reqwest::Response, operation: &str) -> Result<()> {
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    anyhow::bail!("{operation} error {status}: {body}")
}

async fn response_json(response: reqwest::Response, operation: &str) -> Result<Value> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("{operation} error {status}: {body}");
    }
    response
        .json()
        .await
        .with_context(|| format!("parse {operation} response"))
}

impl crate::recorder::AudioConsumer for DashScopeMultimodalASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// 归一化到 multimodal-generation 的完整 endpoint。
///
/// preset 默认下发的就是完整地址，命中首个分支直接用；用户若只填了业务空间
/// 专属域名根（`https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com`）则补上标准
/// 路径。其余情况保守地把标准后缀拼到用户给的路径后面。
pub fn generation_url(base_url: &str) -> Result<String> {
    const CANONICAL_PATH: &str = "/api/v1/services/aigc/multimodal-generation/generation";
    let trimmed = base_url.trim();
    let parsed = reqwest::Url::parse(trimmed).context("parse DashScope base URL")?;
    let path = parsed.path().trim_end_matches('/');
    if path.ends_with("/multimodal-generation/generation") {
        let mut url = parsed.clone();
        url.set_path(path);
        return Ok(url.to_string());
    }
    let mut url = parsed.clone();
    if path.is_empty() {
        url.set_path(CANONICAL_PATH);
    } else {
        url.set_path(&format!("{path}{CANONICAL_PATH}"));
    }
    Ok(url.to_string())
}

pub fn async_transcription_url(base_url: &str) -> Result<String> {
    Ok(api_url(base_url, "/api/v1/services/audio/asr/transcription")?.to_string())
}

pub fn dashscope_multimodal_body(model: &str, wav: &[u8]) -> Value {
    let audio_data = format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(wav)
    );
    dashscope_multimodal_body_from_uri(model, &audio_data)
}

pub fn dashscope_multimodal_body_from_uri(model: &str, audio_uri: &str) -> Value {
    if is_qwen_sync_model(model) {
        return serde_json::json!({
            "model": model,
            "input": {
                "messages": [{
                    "role": "user",
                    "content": [{ "audio": audio_uri }],
                }],
            },
        });
    }
    // qwen-audio-3.0-asr-flash 还支持 vocabulary 与 language_hints；当前批量客户端
    // 尚未将这两项设置映射到请求体，暂时保持自动语言检测且不传热词。
    serde_json::json!({
        "model": model,
        "input": {
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "input_audio",
                    "input_audio": { "data": audio_uri },
                }],
            }],
        },
        "parameters": {
            "format": "wav",
            "sample_rate": "16000",
        },
    })
}

pub fn async_transcription_body(model: &str, file_url: &str) -> Value {
    serde_json::json!({
        "model": model,
        "input": { "file_urls": [file_url] },
        "parameters": {},
    })
}

pub fn extract_async_result_url(json: &Value) -> Result<String> {
    let url = json
        .pointer("/output/results/0/transcription_url")
        .and_then(Value::as_str);
    url.map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned)
        .context("DashScope async ASR response missing transcription_url")
}

pub fn extract_async_transcript_text(json: &Value) -> Result<String> {
    let transcripts = json
        .get("transcripts")
        .context("DashScope async ASR result missing transcripts")?
        .as_array()
        .context("DashScope async ASR transcripts must be an array")?;
    let mut texts = Vec::new();
    for transcript in transcripts {
        if let Some(value) = transcript.get("text") {
            let text = value
                .as_str()
                .context("DashScope async ASR transcript text must be a string")?
                .trim();
            if !text.is_empty() {
                texts.push(text.to_string());
            }
            continue;
        }
        let sentences = transcript
            .get("sentences")
            .context("DashScope async ASR transcript missing text or sentences")?
            .as_array()
            .context("DashScope async ASR sentences must be an array")?;
        for sentence in sentences {
            let text = sentence
                .get("text")
                .and_then(Value::as_str)
                .context("DashScope async ASR sentence missing text")?
                .trim();
            if !text.is_empty() {
                texts.push(text.to_string());
            }
        }
    }
    // 段间用空格分隔：中文识别结果几乎不含空格，连成整句无感知；而拉丁语言
    // （英文等）的词汇若直接拼接会粘在一起，空格分隔对两种场景都更安全。
    Ok(texts.join(" "))
}

/// fun-asr-flash 的响应信封与标准多模态接口不同，且不同模型版本字段路径略有
/// 差异（`output.text` / `output.output.sentence.text` / 标准 `choices`）。
/// 这里按已知路径逐一兜底提取，取到第一个非空文本即返回，避免因单一路径假设
/// 而在某个版本上静默丢字。
pub fn extract_dashscope_text(json: &Value) -> String {
    let output = json.get("output");

    // 1) output.text —— fun-asr-flash 文档主路径
    if let Some(text) = output.and_then(|o| o.get("text")).and_then(Value::as_str) {
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }

    // 2) output.output.sentence.text —— 文档给出的另一种嵌套形态
    if let Some(text) = output
        .and_then(|o| o.get("output"))
        .and_then(|o| o.get("sentence"))
        .and_then(|s| s.get("text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }

    // 3) output.sentence.text
    if let Some(text) = output
        .and_then(|o| o.get("sentence"))
        .and_then(|s| s.get("text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.trim().to_string();
        }
    }

    // 4) 标准多模态 output.choices[0].message.content（字符串或 [{text}] 数组）
    if let Some(content) = output
        .and_then(|o| o.get("choices"))
        .and_then(|c| c.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    {
        if let Some(text) = content.as_str() {
            return text.trim().to_string();
        }
        if let Some(items) = content.as_array() {
            return items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_string();
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn generation_url_from_full_endpoint_is_unchanged() {
        assert_eq!(generation_url(DEFAULT_ENDPOINT).unwrap(), DEFAULT_ENDPOINT);
        assert_eq!(
            generation_url("https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation/").unwrap(),
            DEFAULT_ENDPOINT
        );
    }

    #[test]
    fn generation_url_from_workspace_host_gets_canonical_path() {
        assert_eq!(
            generation_url("https://ws-xxx.cn-beijing.maas.aliyuncs.com").unwrap(),
            "https://ws-xxx.cn-beijing.maas.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
        );
    }

    #[test]
    fn body_uses_multimodal_generation_shape() {
        for model in [DEFAULT_MODEL, QWEN_AUDIO_MODEL] {
            let body = dashscope_multimodal_body(model, b"wav");
            assert_eq!(body["model"], model);
            let audio = &body["input"]["messages"][0]["content"][0];
            assert_eq!(audio["type"], "input_audio");
            assert!(audio["input_audio"]["data"]
                .as_str()
                .unwrap()
                .starts_with("data:audio/wav;base64,"));
            assert_eq!(body["parameters"]["format"], "wav");
            assert_eq!(body["parameters"]["sample_rate"], "16000");
            assert!(body["parameters"].get("vocabulary_id").is_none());
        }
    }

    #[test]
    fn qwen_flash_body_uses_documented_audio_shape() {
        let body = dashscope_multimodal_body("qwen3-asr-flash", b"wav");
        assert_eq!(body["model"], "qwen3-asr-flash");
        let audio = &body["input"]["messages"][0]["content"][0];
        assert!(audio["audio"]
            .as_str()
            .unwrap()
            .starts_with("data:audio/wav;base64,"));
        assert!(audio.get("input_audio").is_none());
    }

    #[test]
    fn classifies_supported_batch_model_protocols() {
        assert_eq!(
            protocol_for_model("fun-asr-flash-2026-06-15"),
            Some(DashScopeBatchProtocol::Multimodal)
        );
        assert_eq!(
            protocol_for_model("qwen3-asr-flash-2026-02-10"),
            Some(DashScopeBatchProtocol::Multimodal)
        );
        // beta 合并：#876 引入的 qwen-audio-3.0-asr-flash 走同步 multimodal。
        assert_eq!(
            protocol_for_model("qwen-audio-3.0-asr-flash"),
            Some(DashScopeBatchProtocol::Multimodal)
        );
        for model in ["fun-asr", "fun-asr-mtl-2025-08-25", "paraformer-v2"] {
            assert_eq!(
                protocol_for_model(model),
                Some(DashScopeBatchProtocol::AsyncTranscription),
                "unexpected protocol for {model}"
            );
        }
        // qwen3-asr-flash-filetrans 仅接受公网 URL，与本地录音的临时 OSS 链路
        // 不兼容：显式拒绝，不得路由到异步协议。
        assert_eq!(
            protocol_for_model("qwen3-asr-flash-filetrans-2025-11-17"),
            None
        );
        assert_eq!(protocol_for_model("unknown-asr"), None);
    }

    #[test]
    fn async_models_get_a_task_polling_timeout() {
        let asr = DashScopeMultimodalASR::new(
            "sk-test".to_string(),
            ASYNC_DEFAULT_ENDPOINT.to_string(),
            "fun-asr".to_string(),
        );
        assert!(asr.transcribe_timeout(1.0) >= Duration::from_secs(660));
        assert!(asr.transcribe_timeout(1_800.0) >= Duration::from_secs(1_500));
        assert!(asr.transcribe_timeout(1_800.0) > asr.transcribe_timeout(1.0));
        assert!(async_upload_timeout(58_000_000) >= Duration::from_secs(900));
    }

    #[test]
    fn async_body_uses_file_urls_input_shape() {
        let funasr = async_transcription_body("fun-asr", "oss://bucket/test.wav");
        assert_eq!(funasr["input"]["file_urls"][0], "oss://bucket/test.wav");
        assert!(funasr["input"].get("file_url").is_none());
        assert_eq!(funasr["parameters"], serde_json::json!({}));
    }

    #[test]
    fn extracts_async_result_url_from_results_array() {
        let json = serde_json::json!({
            "output": {"results": [{
                "subtask_status": "SUCCEEDED",
                "transcription_url": "https://result.example/funasr.json"
            }]}
        });
        assert_eq!(
            extract_async_result_url(&json).unwrap(),
            "https://result.example/funasr.json"
        );
    }

    #[test]
    fn extracts_text_from_async_result_documents() {
        let funasr = serde_json::json!({
            "transcripts": [{"sentences": [{"text": "第一句"}, {"text": "第二句"}]}]
        });
        assert_eq!(extract_async_transcript_text(&funasr).unwrap(), "第一句 第二句");

        let qwen = serde_json::json!({
            "transcripts": [{"text": "Qwen 转写结果"}]
        });
        assert_eq!(extract_async_transcript_text(&qwen).unwrap(), "Qwen 转写结果");
    }

    #[test]
    fn rejects_malformed_async_result_documents() {
        assert!(extract_async_transcript_text(&serde_json::json!({})).is_err());
        assert!(extract_async_transcript_text(&serde_json::json!({
            "transcripts": [{"unexpected": "shape"}]
        }))
        .is_err());
    }

    #[test]
    fn validates_dashscope_transfer_urls() {
        let upgraded = dashscope_transfer_url(
            "http://dashscope-file.oss-cn-beijing.aliyuncs.com/result.json",
        )
        .unwrap();
        assert_eq!(upgraded.scheme(), "https");
        assert!(dashscope_transfer_url("http://169.254.169.254/latest/meta-data").is_err());
        assert!(dashscope_transfer_url("https://aliyuncs.com.evil.example/result.json").is_err());
    }

    #[tokio::test]
    async fn get_json_retries_transient_5xx_then_succeeds() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let server = tokio::spawn(async move {
            for expected_status in [503_u16, 200] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).await.unwrap();
                server_hits.fetch_add(1, Ordering::SeqCst);
                let (status_text, body) = if expected_status == 503 {
                    ("Service Unavailable", "retry me")
                } else {
                    ("OK", "{\"ok\":true}")
                };
                let response = format!(
                    "HTTP/1.1 {expected_status} {status_text}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let value = get_json_with_retry(
            crate::net::credential_http(),
            format!("http://{addr}/poll").parse().unwrap(),
            None,
            Instant::now() + Duration::from_secs(10),
            "test poll",
        )
        .await
        .unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        server.await.unwrap();
    }

    #[test]
    fn extract_text_prefers_output_text() {
        let json = serde_json::json!({ "output": { "text": "  你好世界  " } });
        assert_eq!(extract_dashscope_text(&json), "你好世界");
    }

    #[test]
    fn extract_text_falls_back_to_nested_sentence() {
        let json = serde_json::json!({
            "output": { "output": { "sentence": { "text": "嵌套句" } } }
        });
        assert_eq!(extract_dashscope_text(&json), "嵌套句");
    }

    #[test]
    fn extract_text_falls_back_to_choices_content_array() {
        let json = serde_json::json!({
            "output": {
                "choices": [{
                    "message": { "content": [{ "text": "第一段" }, { "text": "第二段" }] }
                }]
            }
        });
        assert_eq!(extract_dashscope_text(&json), "第一段第二段");
    }

    #[test]
    fn extract_text_empty_when_no_known_path() {
        let json = serde_json::json!({ "request_id": "abc", "output": {} });
        assert_eq!(extract_dashscope_text(&json), "");
    }

    #[tokio::test]
    async fn posts_multimodal_generation_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for DashScope ASR test request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept DashScope ASR test request failed: {err}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_http_request(&mut stream);
            let request_text = String::from_utf8_lossy(&request);
            let lower = request_text.to_ascii_lowercase();
            assert!(request_text.starts_with(
                "POST /api/v1/services/aigc/multimodal-generation/generation HTTP/1.1"
            ));
            assert!(lower.contains("authorization: bearer sk-test"));
            assert!(lower.contains("content-type: application/json"));
            assert!(request_text.contains(r#""model":"fun-asr-flash-2026-06-15""#));
            assert!(request_text.contains(r#""type":"input_audio""#));
            assert!(request_text.contains("data:audio/wav;base64,"));
            assert!(!request_text.contains("vocabulary_id"));
            write_json_response(
                &mut stream,
                r#"{"output":{"text":"你好百炼"},"request_id":"r1"}"#,
            );
        });

        let asr = DashScopeMultimodalASR::new(
            "sk-test".to_string(),
            format!(
                "http://{}/api/v1/services/aigc/multimodal-generation/generation",
                addr
            ),
            DEFAULT_MODEL.to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        assert_eq!(asr.buffer_duration_ms(), 1_000);
        let transcript = asr.transcribe().await.unwrap();

        assert_eq!(transcript.text, "你好百炼");
        assert_eq!(transcript.duration_ms, 1_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn uploads_and_polls_async_transcription() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for step in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_http_request(&mut stream);
                let request_text = String::from_utf8_lossy(&request);
                let lower = request_text.to_ascii_lowercase();
                if matches!(step, 0 | 2 | 3) {
                    assert!(lower.contains("authorization: bearer sk-test"));
                } else {
                    assert!(!lower.contains("authorization:"));
                }
                match step {
                    0 => {
                        assert!(request_text.starts_with(
                            "GET /api/v1/uploads?action=getPolicy&model=fun-asr HTTP/1.1"
                        ));
                        write_json_response(
                            &mut stream,
                            &format!(
                                r#"{{"data":{{"policy":"policy","signature":"signature","upload_dir":"dashscope-instant/test","upload_host":"http://{addr}","oss_access_key_id":"key-id","x_oss_object_acl":"private","x_oss_forbid_overwrite":"true"}}}}"#
                            ),
                        );
                    }
                    1 => {
                        assert!(request_text.starts_with("POST / HTTP/1.1"));
                        assert!(lower.contains("content-type: multipart/form-data"));
                        assert!(request_text.contains("OSSAccessKeyId"));
                        assert!(request_text.contains("success_action_status"));
                        assert!(request_text.contains("audio.wav"));
                        write_json_response(&mut stream, "{}");
                    }
                    2 => {
                        assert!(request_text
                            .starts_with("POST /api/v1/services/audio/asr/transcription HTTP/1.1"));
                        assert!(lower.contains("x-dashscope-async: enable"));
                        assert!(lower.contains("x-dashscope-ossresourceresolve: enable"));
                        assert!(request_text
                            .contains(r#""file_urls":["oss://dashscope-instant/test/audio.wav"]"#));
                        write_json_response(&mut stream, r#"{"output":{"task_id":"task-1"}}"#);
                    }
                    3 => {
                        assert!(request_text.starts_with("GET /api/v1/tasks/task-1 HTTP/1.1"));
                        write_json_response(
                            &mut stream,
                            &format!(
                                r#"{{"output":{{"task_status":"SUCCEEDED","results":[{{"subtask_status":"SUCCEEDED","transcription_url":"http://{addr}/result.json"}}]}}}}"#
                            ),
                        );
                    }
                    4 => {
                        assert!(request_text.starts_with("GET /result.json HTTP/1.1"));
                        write_json_response(
                            &mut stream,
                            r#"{"transcripts":[{"sentences":[{"text":"异步"},{"text":"转写"}]}]}"#,
                        );
                    }
                    _ => unreachable!(),
                }
            }
        });

        let asr = DashScopeMultimodalASR::new(
            "sk-test".to_string(),
            format!("http://{addr}/api/v1/services/audio/asr/transcription"),
            "fun-asr".to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "异步 转写");
        assert_eq!(transcript.duration_ms, 1_000);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn async_policy_request_does_not_follow_redirects_with_credentials() {
        let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_addr = redirect_listener.local_addr().unwrap();
        let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        target_listener.set_nonblocking(true).unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let followed = Arc::new(AtomicBool::new(false));
        let target_followed = Arc::clone(&followed);

        let redirect_server = thread::spawn(move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            assert!(String::from_utf8_lossy(&request)
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-test"));
            let response = format!(
                "HTTP/1.1 302 Found\r\nlocation: http://{target_addr}/api/v1/uploads\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });
        let target_server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match target_listener.accept() {
                    Ok((mut stream, _)) => {
                        target_followed.store(true, Ordering::SeqCst);
                        let _request = read_http_request(&mut stream);
                        let body = r#"{"message":"redirect followed"}"#;
                        let response = format!(
                            "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                        stream.flush().unwrap();
                        break;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept redirect target request failed: {err}"),
                }
            }
        });

        let asr = DashScopeMultimodalASR::new(
            "sk-test".to_string(),
            format!("http://{redirect_addr}/api/v1/services/audio/asr/transcription"),
            "fun-asr".to_string(),
        );
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        let error = asr.transcribe().await.unwrap_err().to_string();

        assert!(error.contains("302 Found"), "unexpected error: {error}");
        redirect_server.join().unwrap();
        target_server.join().unwrap();
        assert!(!followed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn async_result_download_does_not_follow_redirects() {
        let redirect_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_addr = redirect_listener.local_addr().unwrap();
        let target_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        target_listener.set_nonblocking(true).unwrap();
        let target_addr = target_listener.local_addr().unwrap();
        let followed = Arc::new(AtomicBool::new(false));
        let target_followed = Arc::clone(&followed);
        let redirect_server = thread::spawn(move || {
            let (mut stream, _) = redirect_listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            assert!(!String::from_utf8_lossy(&request)
                .to_ascii_lowercase()
                .contains("authorization:"));
            let response = format!(
                "HTTP/1.1 302 Found\r\nlocation: http://{target_addr}/result.json\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });
        let target_server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match target_listener.accept() {
                    Ok((_stream, _)) => {
                        target_followed.store(true, Ordering::SeqCst);
                        break;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("accept result redirect target failed: {err}"),
                }
            }
        });

        let error = download_async_result(&format!("http://{redirect_addr}/result.json"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("302 Found"), "unexpected error: {error}");
        redirect_server.join().unwrap();
        target_server.join().unwrap();
        assert!(!followed.load(Ordering::SeqCst));
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        let mut expected_len = None;
        loop {
            let read = stream.read(&mut buf).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if expected_len.is_none() {
                expected_len = parse_expected_request_len(&request);
            }
            if expected_len.is_some_and(|len| request.len() >= len) {
                break;
            }
        }
        request
    }

    fn parse_expected_request_len(request: &[u8]) -> Option<usize> {
        let header_end = request.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_len = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        Some(header_end + content_len)
    }

    fn write_json_response(stream: &mut std::net::TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }
}
