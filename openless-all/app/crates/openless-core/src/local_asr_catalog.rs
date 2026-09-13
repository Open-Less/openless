//! Stable local-ASR identifiers and preference normalization shared by every host.

use serde::{Deserialize, Serialize};

use crate::errors::{BackendError, BackendErrorCode};

pub(crate) const WHISPER_MODEL_ID: &str = "whisper-large-v3-turbo";
pub(crate) const FOUNDRY_PROVIDER_ID: &str = "foundry-local-whisper";
pub(crate) const FOUNDRY_DEFAULT_MODEL_ALIAS: &str = "whisper-small";
pub(crate) const SHERPA_DEFAULT_MODEL_ALIAS: &str = "sense-voice-small-zh";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocalAsrModelId {
    #[serde(rename = "qwen3-asr-0.6b")]
    Small06b,
    #[serde(rename = "qwen3-asr-1.7b")]
    Large17b,
    #[serde(rename = "whisper-base")]
    WhisperBase,
    #[serde(rename = "whisper-small")]
    WhisperSmall,
    #[serde(rename = "whisper-medium")]
    WhisperMedium,
    #[serde(rename = "whisper-large-v3")]
    WhisperLargeV3,
    #[serde(rename = "whisper-large-v3-turbo")]
    WhisperLargeV3Turbo,
    #[serde(rename = "whisper-large-v3-turbo-q5")]
    WhisperLargeV3TurboQ5,
}

impl LocalAsrModelId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small06b => "qwen3-asr-0.6b",
            Self::Large17b => "qwen3-asr-1.7b",
            Self::WhisperBase => "whisper-base",
            Self::WhisperSmall => "whisper-small",
            Self::WhisperMedium => "whisper-medium",
            Self::WhisperLargeV3 => "whisper-large-v3",
            Self::WhisperLargeV3Turbo => "whisper-large-v3-turbo",
            Self::WhisperLargeV3TurboQ5 => "whisper-large-v3-turbo-q5",
        }
    }

    pub fn from_wire_id(value: &str) -> Option<Self> {
        match value {
            "qwen3-asr-0.6b" => Some(Self::Small06b),
            "qwen3-asr-1.7b" => Some(Self::Large17b),
            "whisper-base" => Some(Self::WhisperBase),
            "whisper-small" => Some(Self::WhisperSmall),
            "whisper-medium" => Some(Self::WhisperMedium),
            "whisper-large-v3" => Some(Self::WhisperLargeV3),
            "whisper-large-v3-turbo" => Some(Self::WhisperLargeV3Turbo),
            "whisper-large-v3-turbo-q5" => Some(Self::WhisperLargeV3TurboQ5),
            _ => None,
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::Small06b,
            Self::Large17b,
            Self::WhisperBase,
            Self::WhisperSmall,
            Self::WhisperMedium,
            Self::WhisperLargeV3,
            Self::WhisperLargeV3Turbo,
            Self::WhisperLargeV3TurboQ5,
        ]
    }

    pub fn hf_repo(self) -> &'static str {
        match self {
            Self::Small06b => "Qwen/Qwen3-ASR-0.6B",
            Self::Large17b => "Qwen/Qwen3-ASR-1.7B",
            Self::WhisperBase
            | Self::WhisperSmall
            | Self::WhisperMedium
            | Self::WhisperLargeV3
            | Self::WhisperLargeV3Turbo
            | Self::WhisperLargeV3TurboQ5 => "ggerganov/whisper.cpp",
        }
    }

    pub fn file_name(self) -> Option<&'static str> {
        match self {
            Self::WhisperBase => Some("ggml-base.bin"),
            Self::WhisperSmall => Some("ggml-small.bin"),
            Self::WhisperMedium => Some("ggml-medium.bin"),
            Self::WhisperLargeV3 => Some("ggml-large-v3.bin"),
            Self::WhisperLargeV3Turbo => Some("ggml-large-v3-turbo.bin"),
            Self::WhisperLargeV3TurboQ5 => Some("ggml-large-v3-turbo-q5_0.bin"),
            Self::Small06b | Self::Large17b => None,
        }
    }

    pub fn is_whisper(self) -> bool {
        matches!(
            self,
            Self::WhisperBase
                | Self::WhisperSmall
                | Self::WhisperMedium
                | Self::WhisperLargeV3
                | Self::WhisperLargeV3Turbo
                | Self::WhisperLargeV3TurboQ5
        )
    }

    pub fn is_qwen(self) -> bool {
        matches!(self, Self::Small06b | Self::Large17b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalAsrRuntime {
    Generic,
    Foundry,
    SherpaOnnx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SherpaModelFamily {
    SenseVoice,
    Paraformer,
    Whisper,
    Qwen3Asr,
    Zipformer,
}

impl SherpaModelFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SenseVoice => "sense_voice",
            Self::Paraformer => "paraformer",
            Self::Whisper => "whisper",
            Self::Qwen3Asr => "qwen3_asr",
            Self::Zipformer => "zipformer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalAsrExecutionMode {
    Offline,
    Online,
}

impl LocalAsrExecutionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Online => "online",
        }
    }
}

impl LocalAsrRuntime {
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Generic => "local-qwen3",
            Self::Foundry => FOUNDRY_PROVIDER_ID,
            Self::SherpaOnnx => "sherpa-onnx-local",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Generic => LocalAsrModelId::Small06b.as_str(),
            Self::Foundry => FOUNDRY_DEFAULT_MODEL_ALIAS,
            Self::SherpaOnnx => SHERPA_DEFAULT_MODEL_ALIAS,
        }
    }
}

const FOUNDRY_MODEL_ALIASES: &[&str] = &[
    "whisper-small",
    "whisper-medium",
    "whisper-large-v3-turbo",
    "whisper-base",
    "whisper-tiny",
];

const SHERPA_MODEL_ALIASES: &[&str] = &[
    "sense-voice-small-zh",
    "paraformer-zh",
    "whisper-small-multi",
    "whisper-large-v3-multi",
    "qwen3-asr-0.6b-int8",
    "zipformer-bilingual-zh-en-streaming",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsrTarget {
    pub runtime: LocalAsrRuntime,
    model_id: String,
}

impl LocalAsrTarget {
    pub fn parse(
        runtime: LocalAsrRuntime,
        model_id: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let model_id = model_id.into();
        let known = match runtime {
            LocalAsrRuntime::Generic => LocalAsrModelId::from_wire_id(&model_id).is_some(),
            LocalAsrRuntime::Foundry => FOUNDRY_MODEL_ALIASES.contains(&model_id.as_str()),
            LocalAsrRuntime::SherpaOnnx => SHERPA_MODEL_ALIASES.contains(&model_id.as_str()),
        };
        if !known {
            return Err(BackendError::new(
                BackendErrorCode::InvalidArgument,
                format!(
                    "unknown {} local ASR model: {model_id}",
                    runtime.provider_id()
                ),
            ));
        }
        Ok(Self { runtime, model_id })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn sherpa_family(&self) -> Option<SherpaModelFamily> {
        if self.runtime != LocalAsrRuntime::SherpaOnnx {
            return None;
        }
        match self.model_id.as_str() {
            "sense-voice-small-zh" => Some(SherpaModelFamily::SenseVoice),
            "paraformer-zh" => Some(SherpaModelFamily::Paraformer),
            "whisper-small-multi" | "whisper-large-v3-multi" => Some(SherpaModelFamily::Whisper),
            "qwen3-asr-0.6b-int8" => Some(SherpaModelFamily::Qwen3Asr),
            "zipformer-bilingual-zh-en-streaming" => Some(SherpaModelFamily::Zipformer),
            _ => None,
        }
    }

    pub fn sherpa_execution_mode(&self) -> Option<LocalAsrExecutionMode> {
        if self.runtime != LocalAsrRuntime::SherpaOnnx {
            return None;
        }
        Some(if self.model_id == "zipformer-bilingual-zh-en-streaming" {
            LocalAsrExecutionMode::Online
        } else {
            LocalAsrExecutionMode::Offline
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalAsrMirror {
    #[default]
    Huggingface,
    HfMirror,
    GithubRelease,
}

impl LocalAsrMirror {
    pub fn from_legacy(value: &str) -> Self {
        match value.trim() {
            "hf-mirror" => Self::HfMirror,
            "github-release" => Self::GithubRelease,
            _ => Self::Huggingface,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Huggingface => "huggingface",
            Self::HfMirror => "hf-mirror",
            Self::GithubRelease => "github-release",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FoundryRuntimeSource {
    #[default]
    Auto,
    Nuget,
    OrtNightly,
}

impl FoundryRuntimeSource {
    pub fn from_legacy(value: &str) -> Self {
        match value.trim() {
            "nuget" => Self::Nuget,
            "ort-nightly" => Self::OrtNightly,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Nuget => "nuget",
            Self::OrtNightly => "ort-nightly",
        }
    }
}

fn invalid_language_hint(message: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::InvalidArgument, message)
}

pub fn normalize_foundry_language_hint(value: &str) -> Result<String, BackendError> {
    let normalized = value.trim().to_string();
    if normalized.is_empty()
        || (normalized.len() == 2 && normalized.bytes().all(|byte| byte.is_ascii_lowercase()))
    {
        Ok(normalized)
    } else {
        Err(invalid_language_hint(
            "language hint must be empty or ISO 639-1 lowercase code",
        ))
    }
}

pub fn normalize_sherpa_language_hint(value: &str) -> Result<String, BackendError> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty()
        || normalized
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '-')
    {
        Ok(normalized)
    } else {
        Err(invalid_language_hint(
            "language hint must be empty or BCP-47 lowercase code",
        ))
    }
}

pub(crate) fn normalize_foundry_runtime_source(value: &str) -> String {
    FoundryRuntimeSource::from_legacy(value).as_str().into()
}
