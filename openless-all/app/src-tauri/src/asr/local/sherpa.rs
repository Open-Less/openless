//! Windows sherpa-onnx 本地 ASR 的原生运行模式与事件载荷。
//!
//! 当前 catalog 覆盖 Windows offline batch 模型和实验 online streaming 模型；
//! `sherpa_runtime.rs` 分别持有 `OfflineRecognizer` / `OnlineRecognizer`。

use serde::Serialize;

pub const PROVIDER_ID: &str = "sherpa-onnx-local";
#[cfg(test)]
pub const DEFAULT_MODEL_ALIAS: &str = "sense-voice-small-zh";
#[cfg(test)]
pub const DEFAULT_ONLINE_MODEL_ALIAS: &str = "zipformer-bilingual-zh-en-streaming";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum SherpaPreparePhase {
    Runtime,
    Model,
    Load,
    Finished,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SherpaPrepareProgressPayload {
    pub phase: SherpaPreparePhase,
    pub model_alias: String,
    pub label: String,
    pub percent: Option<f64>,
    pub error: Option<String>,
}

impl SherpaPrepareProgressPayload {
    #[allow(dead_code)]
    pub fn new(
        phase: SherpaPreparePhase,
        model_alias: impl Into<String>,
        label: impl Into<String>,
        percent: Option<f64>,
        error: Option<String>,
    ) -> Self {
        Self {
            phase,
            model_alias: model_alias.into(),
            label: label.into(),
            percent: percent.map(|value| value.clamp(0.0, 100.0)),
            error,
        }
    }

    #[allow(dead_code)]
    pub fn failed(
        model_alias: impl Into<String>,
        label: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self::new(
            SherpaPreparePhase::Failed,
            model_alias,
            label,
            None,
            Some(error.into()),
        )
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SherpaRuntimeStatus {
    pub provider_id: String,
    /// 当前平台是否具备 sherpa-onnx 推理能力。Windows 为 true；其他平台保留
    /// provider 元数据但不提供本地 sherpa 推理。
    pub available: bool,
    /// 当前模型是否已加载到内存。
    pub runtime_ready: bool,
    pub active_model: String,
    pub loaded_model_id: Option<String>,
    pub error: Option<String>,
    /// 最近一次 prepare/load 耗时。缓存命中也会记录一次很小的耗时。
    pub last_prepare_ms: Option<u64>,
    /// 最近一次 batch decode 耗时，不含录音时间。
    pub last_transcribe_ms: Option<u64>,
    /// 最近一次送入 recognizer 的音频时长。
    pub last_audio_ms: Option<u64>,
    /// 最近一次 prepare/transcribe 错误，方便 UI 和日志定位可恢复失败。
    pub last_error: Option<String>,
}

impl SherpaRuntimeStatus {
    #[allow(dead_code)]
    pub fn unavailable(active_model: String, error: impl Into<String>) -> Self {
        let error = error.into();
        Self {
            provider_id: PROVIDER_ID.into(),
            available: false,
            runtime_ready: false,
            active_model,
            loaded_model_id: None,
            error: Some(error.clone()),
            last_prepare_ms: None,
            last_transcribe_ms: None,
            last_audio_ms: None,
            last_error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_status_uses_provider_id() {
        let status = SherpaRuntimeStatus::unavailable("paraformer-zh".into(), "not ready");
        assert_eq!(status.provider_id, PROVIDER_ID);
        assert!(!status.available);
        assert!(!status.runtime_ready);
        assert_eq!(status.active_model, "paraformer-zh");
        assert_eq!(status.error.as_deref(), Some("not ready"));
        assert_eq!(status.last_error.as_deref(), Some("not ready"));
    }

    #[test]
    fn prepare_progress_payload_uses_expected_event_shape() {
        let payload = SherpaPrepareProgressPayload::new(
            SherpaPreparePhase::Model,
            "sense-voice-small-zh",
            "download model",
            Some(42.4),
            None,
        );
        let value = serde_json::to_value(payload).unwrap();
        assert_eq!(value["phase"], "model");
        assert_eq!(value["modelAlias"], "sense-voice-small-zh");
        assert_eq!(value["label"], "download model");
        assert_eq!(value["percent"], 42.4);
        assert_eq!(value["error"], serde_json::Value::Null);
    }
}
