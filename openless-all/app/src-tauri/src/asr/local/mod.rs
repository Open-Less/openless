//! 本地 ASR 引擎入口。
//!
//! 当前本地引擎：
//! - **macOS**：Qwen3-ASR 可选 MLX/Metal 或 C/CPU；
//! - **Linux**：Qwen3-ASR C/CPU；
//! - **Windows**：Foundry Local Whisper（`foundry_*`），以及 sherpa-onnx-local
//!   实验 provider（`sherpa*`，offline batch + online streaming）

pub mod cache;
pub mod download;
pub mod foundry;
pub mod foundry_native;
pub mod foundry_provider;
pub mod foundry_runtime;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod local_provider;
pub mod models;
pub mod sherpa;
pub mod sherpa_download;
pub mod sherpa_provider;
pub mod sherpa_runtime;
pub mod test_run;

#[cfg(target_os = "macos")]
mod whisper_provider;

pub use cache::LocalAsrCache;
#[allow(unused_imports)]
pub use foundry_provider::FoundryLocalWhisperAsr;
#[allow(unused_imports)]
pub use foundry_runtime::FoundryLocalRuntime;
#[allow(unused_imports)]
pub use sherpa_provider::SherpaOnnxAsr;
#[allow(unused_imports)]
pub use sherpa_runtime::SherpaOnnxRuntime;

#[cfg(target_os = "macos")]
mod apple_speech_provider;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod mlx_qwen_engine;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod mlx_worker;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod qwen_engine;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod qwen_ffi;

#[cfg(target_os = "macos")]
#[allow(unused_imports)]
pub use apple_speech_provider::{native_name_to_apple_locale, AppleSpeechAsr};
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use local_provider::LocalQwenAsr;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use mlx_qwen_engine::MlxQwenAsrEngine;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use mlx_worker::run_if_requested as run_mlx_worker_if_requested;
#[cfg(target_os = "macos")]
pub(crate) use whisper_provider::WhisperEngine;
#[cfg(target_os = "macos")]
pub use whisper_provider::MODEL_ID as WHISPER_MODEL_ID;
#[cfg(target_os = "macos")]
pub use whisper_provider::{
    model_path_for_model as whisper_model_path_for_model,
    model_ready_for_model as whisper_model_ready_for_model,
};
#[cfg(target_os = "macos")]
pub use whisper_provider::{LocalWhisperAsr, LocalWhisperCache};

pub use download::{DownloadManager, Mirror};
pub use models::{ModelId, ModelStatus};

/// 本地 Qwen3-ASR 在 active_asr 字段里的标识；与前端 ASR_PRESETS 的 id 对齐。
/// 旧版本的本地 Qwen3-ASR provider id。macOS 映射到 MLX，Linux 映射到 C，
/// 仅用于兼容已经保存的渠道配置；新渠道请使用下方两个明确后端 id。
pub const PROVIDER_ID: &str = "local-qwen3";
pub const LOCAL_QWEN3_MLX_PROVIDER_ID: &str = "local-qwen3-mlx";
pub const LOCAL_QWEN3_C_PROVIDER_ID: &str = "local-qwen3-c";

pub const LOCAL_WHISPER_PROVIDER_ID: &str = "local-whisper";

pub fn is_local_whisper(id: &str) -> bool {
    id == LOCAL_WHISPER_PROVIDER_ID
}

pub fn is_local_qwen3(id: &str) -> bool {
    matches!(
        id,
        PROVIDER_ID | LOCAL_QWEN3_MLX_PROVIDER_ID | LOCAL_QWEN3_C_PROVIDER_ID
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QwenBackend {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    Mlx,
    C,
}

impl QwenBackend {
    pub fn cache_key(self) -> &'static str {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx => "mlx",
            Self::C => "c",
        }
    }
}

pub fn qwen_backend_for_provider(id: &str) -> Option<QwenBackend> {
    match id {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        PROVIDER_ID | LOCAL_QWEN3_MLX_PROVIDER_ID => Some(QwenBackend::Mlx),
        #[cfg(target_os = "linux")]
        PROVIDER_ID | LOCAL_QWEN3_C_PROVIDER_ID => Some(QwenBackend::C),
        #[cfg(target_os = "macos")]
        LOCAL_QWEN3_C_PROVIDER_ID => Some(QwenBackend::C),
        #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
        PROVIDER_ID => Some(QwenBackend::C),
        _ => None,
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub enum LocalQwenEngine {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    Mlx(MlxQwenAsrEngine),
    C(qwen_engine::QwenAsrEngine),
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl LocalQwenEngine {
    pub fn load(backend: QwenBackend, model_dir: &std::path::Path) -> anyhow::Result<Self> {
        match backend {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            QwenBackend::Mlx => Ok(Self::Mlx(MlxQwenAsrEngine::load(model_dir)?)),
            QwenBackend::C => Ok(Self::C(qwen_engine::QwenAsrEngine::load(model_dir)?)),
        }
    }

    pub fn transcribe_pcm(&self, samples: &[f32]) -> anyhow::Result<String> {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx(engine) => engine.transcribe_pcm(samples),
            Self::C(engine) => engine.transcribe_audio(samples),
        }
    }

    pub fn next_operation_id(&self) -> u64 {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx(engine) => engine.next_operation_id(),
            Self::C(_) => 0,
        }
    }

    pub fn cancel_operation(&self, operation_id: u64) {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx(engine) => engine.cancel_operation(operation_id),
            Self::C(_) => {
                let _ = operation_id;
            }
        }
    }

    pub fn cancel(&self) {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx(engine) => engine.abort(),
            Self::C(_) => {}
        }
    }

    pub fn is_healthy(&self) -> bool {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx(engine) => engine.is_healthy(),
            Self::C(_) => true,
        }
    }

    /// Dictation 转写保持各后端原有语义：MLX 整段 batch；C 追加 0.5 秒静音后
    /// 走流式解码，并将稳定 token 交给调用方实时显示。
    pub fn transcribe_dictation_with_handler<F>(
        &self,
        operation_id: u64,
        cancelled: &std::sync::atomic::AtomicBool,
        mut samples: Vec<f32>,
        handler: F,
    ) -> anyhow::Result<String>
    where
        F: FnMut(&str) + Send + 'static,
    {
        match self {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            Self::Mlx(engine) => {
                engine.transcribe_pcm_for_operation(operation_id, &samples, cancelled)
            }
            Self::C(engine) => {
                let _ = (operation_id, cancelled);
                append_c_stream_tail_padding(&mut samples);
                engine.transcribe_stream_with_handler(&samples, handler)
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
const C_STREAM_TAIL_PADDING_SAMPLES: usize = 8_000;

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn append_c_stream_tail_padding(samples: &mut Vec<f32>) {
    samples.resize(samples.len() + C_STREAM_TAIL_PADDING_SAMPLES, 0.0);
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod qwen_dictation_tests {
    use super::*;

    #[test]
    fn c_stream_padding_adds_half_a_second_without_changing_existing_audio() {
        let mut samples = vec![0.25, -0.5, 1.0];
        let original = samples.clone();

        append_c_stream_tail_padding(&mut samples);

        assert_eq!(&samples[..original.len()], original.as_slice());
        assert_eq!(samples.len(), original.len() + 8_000);
        assert!(samples[original.len()..]
            .iter()
            .all(|sample| *sample == 0.0));
    }
}

/// Apple Speech（SFSpeechRecognizer）本地 ASR 的 provider id；与前端
/// ASR_PRESETS 的 id 对齐（issue #574）。该字符串在所有平台都可被识别，
/// 但 provider 实现只在 macOS 编译；非 macOS 上由上层判为 not-configured /
/// 不可用（见 commands / coordinator 的平台门控）。
pub const APPLE_SPEECH_PROVIDER_ID: &str = "apple-speech";

#[allow(dead_code)]
pub fn is_apple_speech(id: &str) -> bool {
    id == APPLE_SPEECH_PROVIDER_ID
}
