//! Windows sherpa-onnx 本地 ASR 的常量、catalog 与事件载荷。
//!
//! M1 阶段：纯描述层；不依赖 `sherpa-onnx` crate，不做实际推理。
//! 与 `foundry.rs` 形状对齐，便于前端命令链路与 Foundry 同形复用。
//!
//! 推理接入见 `sherpa_runtime.rs`（M2）。

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

pub const PROVIDER_ID: &str = "sherpa-onnx-local";
pub const DEFAULT_MODEL_ALIAS: &str = "sense-voice-small-zh";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum SherpaFamily {
    SenseVoice,
    Paraformer,
    Whisper,
    Qwen3Asr,
    Zipformer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum SherpaMode {
    /// 录音停止后整段 PCM 一次性识别。
    Offline,
    /// 边录边识别 partial / final segment。M5 才接。
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SherpaModel {
    pub alias: &'static str,
    pub display_name: &'static str,
    pub family: SherpaFamily,
    pub mode: SherpaMode,
    /// 表征长度，使用 ISO 639-1 / BCP-47 风格小写串。
    pub languages: &'static [&'static str],
    pub quality_tier: &'static str,
}

/// M1 catalog 三档：默认 SenseVoice，中文专用 Paraformer，多语 Whisper 兜底。
/// 文件清单 + 校验和会在 M3 模型管理阶段补全；M1 只暴露元数据驱动 UI。
#[allow(dead_code)]
pub const MODELS: &[SherpaModel] = &[
    SherpaModel {
        alias: "sense-voice-small-zh",
        display_name: "SenseVoice Small (zh/en/ja/ko/yue)",
        family: SherpaFamily::SenseVoice,
        mode: SherpaMode::Offline,
        languages: &["zh", "en", "ja", "ko", "yue"],
        quality_tier: "balanced",
    },
    SherpaModel {
        alias: "paraformer-zh",
        display_name: "Paraformer (zh)",
        family: SherpaFamily::Paraformer,
        mode: SherpaMode::Offline,
        languages: &["zh"],
        quality_tier: "chinese-strong",
    },
    SherpaModel {
        alias: "whisper-small-multi",
        display_name: "Whisper Small (multilingual)",
        family: SherpaFamily::Whisper,
        mode: SherpaMode::Offline,
        languages: &["multi"],
        quality_tier: "english-fallback",
    },
    SherpaModel {
        alias: "qwen3-asr-0.6b-int8",
        display_name: "Qwen3-ASR 0.6B INT8",
        family: SherpaFamily::Qwen3Asr,
        mode: SherpaMode::Offline,
        languages: &["multi"],
        quality_tier: "qwen3-balanced",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SherpaReleaseArchive {
    pub url: &'static str,
    pub file_name: &'static str,
    pub root_dir: &'static str,
}

#[allow(dead_code)]
pub fn is_sherpa_onnx_local(id: &str) -> bool {
    id == PROVIDER_ID
}

#[allow(dead_code)]
pub fn model_alias_is_known(alias: &str) -> bool {
    MODELS.iter().any(|model| model.alias == alias)
}

pub fn hf_repo_for_alias(alias: &str) -> Result<&'static str> {
    match alias {
        "sense-voice-small-zh" => {
            Ok("csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17")
        }
        "paraformer-zh" => Ok("csukuangfj/sherpa-onnx-paraformer-zh-2024-03-09"),
        "whisper-small-multi" => Ok("csukuangfj/sherpa-onnx-whisper-small"),
        _ => anyhow::bail!("unknown sherpa-onnx model alias: {alias}"),
    }
}

pub fn required_files_for_alias(alias: &str) -> Result<&'static [&'static str]> {
    match alias {
        "sense-voice-small-zh" => Ok(&["model.int8.onnx", "tokens.txt"]),
        "paraformer-zh" => Ok(&["model.int8.onnx", "tokens.txt"]),
        "whisper-small-multi" => Ok(&["encoder.int8.onnx", "decoder.int8.onnx", "tokens.txt"]),
        "qwen3-asr-0.6b-int8" => Ok(&[
            "conv_frontend.onnx",
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "tokenizer",
        ]),
        _ => anyhow::bail!("unknown sherpa-onnx model alias: {alias}"),
    }
}

pub fn required_path_is_valid(alias: &str, required: &str, path: &Path) -> bool {
    if required_path_is_dir(alias, required) {
        required_dir_is_valid(alias, required, path)
    } else {
        path.is_file()
    }
}

fn required_path_is_dir(alias: &str, required: &str) -> bool {
    matches!((alias, required), ("qwen3-asr-0.6b-int8", "tokenizer"))
}

fn required_dir_is_valid(alias: &str, required: &str, path: &Path) -> bool {
    match (alias, required) {
        ("qwen3-asr-0.6b-int8", "tokenizer") => {
            path.join("tokenizer.json").is_file()
                || (path.join("vocab.json").is_file() && path.join("merges.txt").is_file())
        }
        _ => false,
    }
}

pub fn download_files_for_alias(alias: &str) -> Result<&'static [(&'static str, &'static str)]> {
    match alias {
        "sense-voice-small-zh" => Ok(&[
            ("model.int8.onnx", "model.int8.onnx"),
            ("tokens.txt", "tokens.txt"),
        ]),
        "paraformer-zh" => Ok(&[
            ("model.int8.onnx", "model.int8.onnx"),
            ("tokens.txt", "tokens.txt"),
        ]),
        "whisper-small-multi" => Ok(&[
            ("small-encoder.int8.onnx", "encoder.int8.onnx"),
            ("small-decoder.int8.onnx", "decoder.int8.onnx"),
            ("small-tokens.txt", "tokens.txt"),
        ]),
        _ => anyhow::bail!("unknown sherpa-onnx model alias: {alias}"),
    }
}

pub fn release_archive_for_alias(alias: &str) -> Option<SherpaReleaseArchive> {
    match alias {
        "qwen3-asr-0.6b-int8" => Some(SherpaReleaseArchive {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
            file_name: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
            root_dir: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25",
        }),
        _ => None,
    }
}

pub fn model_dir_for_alias(alias: &str) -> Result<PathBuf> {
    if !model_alias_is_known(alias) {
        anyhow::bail!("unknown sherpa-onnx model alias: {alias}");
    }
    #[cfg(target_os = "windows")]
    {
        Ok(crate::persistence::sherpa_onnx_models_root()?.join(alias))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(std::env::temp_dir()
            .join("openless-sherpa-onnx")
            .join(alias))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SherpaCatalogModel {
    pub alias: String,
    pub display_name: String,
    pub family: SherpaFamily,
    pub mode: SherpaMode,
    pub languages: Vec<String>,
    pub cached: bool,
    pub downloaded_bytes: u64,
    pub file_size_mb: Option<u64>,
}

impl SherpaCatalogModel {
    #[allow(dead_code)]
    pub fn from_static(model: &SherpaModel) -> Self {
        Self {
            alias: model.alias.to_string(),
            display_name: model.display_name.to_string(),
            family: model.family,
            mode: model.mode,
            languages: model.languages.iter().map(|s| s.to_string()).collect(),
            cached: false,
            downloaded_bytes: 0,
            file_size_mb: None,
        }
    }
}

#[allow(dead_code)]
pub fn static_catalog_models() -> Vec<SherpaCatalogModel> {
    MODELS.iter().map(SherpaCatalogModel::from_static).collect()
}

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
    /// M1 阶段恒为 false：sherpa-onnx crate 尚未接入。
    pub available: bool,
    /// 当前模型是否已加载到内存。
    pub runtime_ready: bool,
    pub active_model: String,
    pub loaded_model_id: Option<String>,
    pub error: Option<String>,
}

impl SherpaRuntimeStatus {
    #[allow(dead_code)]
    pub fn unavailable(active_model: String, error: impl Into<String>) -> Self {
        Self {
            provider_id: PROVIDER_ID.into(),
            available: false,
            runtime_ready: false,
            active_model,
            loaded_model_id: None,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn provider_id_is_stable() {
        assert!(is_sherpa_onnx_local("sherpa-onnx-local"));
        assert!(!is_sherpa_onnx_local("foundry-local-whisper"));
        assert!(!is_sherpa_onnx_local("local-qwen3"));
    }

    #[test]
    fn default_model_is_registered() {
        assert!(model_alias_is_known(DEFAULT_MODEL_ALIAS));
    }

    #[test]
    fn static_catalog_preserves_ui_order() {
        let catalog = static_catalog_models();
        assert_eq!(
            catalog.iter().map(|m| m.alias.as_str()).collect::<Vec<_>>(),
            vec![
                "sense-voice-small-zh",
                "paraformer-zh",
                "whisper-small-multi",
                "qwen3-asr-0.6b-int8",
            ]
        );
        assert!(catalog.iter().all(|m| !m.cached));
    }

    #[test]
    fn qwen3_tokenizer_accepts_supported_layouts() {
        let dir = std::env::temp_dir().join(format!(
            "openless-sherpa-tokenizer-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("create tokenizer dir");

        assert!(!required_path_is_valid(
            "qwen3-asr-0.6b-int8",
            "tokenizer",
            &dir
        ));

        fs::write(dir.join("tokenizer.json"), b"{}").expect("write tokenizer json");
        assert!(required_path_is_valid(
            "qwen3-asr-0.6b-int8",
            "tokenizer",
            &dir
        ));

        fs::remove_file(dir.join("tokenizer.json")).expect("remove tokenizer json");
        fs::write(dir.join("vocab.json"), b"{}").expect("write vocab json");
        assert!(!required_path_is_valid(
            "qwen3-asr-0.6b-int8",
            "tokenizer",
            &dir
        ));

        fs::write(dir.join("merges.txt"), b"#version: 0.2").expect("write merges txt");
        assert!(required_path_is_valid(
            "qwen3-asr-0.6b-int8",
            "tokenizer",
            &dir
        ));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unavailable_status_uses_provider_id() {
        let status = SherpaRuntimeStatus::unavailable("paraformer-zh".into(), "not ready");
        assert_eq!(status.provider_id, PROVIDER_ID);
        assert!(!status.available);
        assert!(!status.runtime_ready);
        assert_eq!(status.active_model, "paraformer-zh");
        assert_eq!(status.error.as_deref(), Some("not ready"));
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
