//! Native local speech runtimes. macOS uses opt-in experimental modules only;
//! old IDs remain metadata-only and cannot link their former inference libraries.
#[cfg(any(local_asr_sherpa, test))]
mod blocking_decode;
pub mod experimental;
pub mod foundry;
pub mod foundry_native;
pub mod foundry_provider;
pub mod foundry_runtime;
#[cfg(target_os = "windows")]
mod native_model_receipt;
pub mod sherpa;
pub mod sherpa_provider;
pub mod sherpa_runtime;
pub mod test_run;

#[allow(unused_imports)]
pub use foundry_provider::FoundryLocalWhisperAsr;
#[allow(unused_imports)]
pub use foundry_runtime::FoundryLocalRuntime;
#[allow(unused_imports)]
pub use sherpa_provider::SherpaOnnxAsr;
#[allow(unused_imports)]
pub use sherpa_runtime::SherpaOnnxRuntime;

/// Retired channel IDs are used only for UI retirement and scoped cleanup.
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

pub const APPLE_SPEECH_PROVIDER_ID: &str = "apple-speech";
