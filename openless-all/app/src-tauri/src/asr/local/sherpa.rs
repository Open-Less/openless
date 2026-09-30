//! Native runtime modes and event payloads for Windows sherpa-onnx local ASR.
//!
//! The current catalog covers Windows offline batch models and experimental online streaming
//! models; `sherpa_runtime.rs` holds the `OfflineRecognizer` / `OnlineRecognizer` respectively.

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
    /// Whether the current platform has sherpa-onnx inference capability. True on Windows;
    /// other platforms keep the provider metadata but provide no local sherpa inference.
    pub available: bool,
    /// Whether the current model is loaded in memory.
    pub runtime_ready: bool,
    pub active_model: String,
    pub loaded_model_id: Option<String>,
    pub error: Option<String>,
    /// Duration of the most recent prepare/load. A cache hit also records a very small value.
    pub last_prepare_ms: Option<u64>,
    /// Duration of the most recent batch decode, excluding recording time.
    pub last_transcribe_ms: Option<u64>,
    /// Duration of the audio most recently fed to the recognizer.
    pub last_audio_ms: Option<u64>,
    /// Most recent prepare/transcribe error, helping UI and logs locate recoverable failures.
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
