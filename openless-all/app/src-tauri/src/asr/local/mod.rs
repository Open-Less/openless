//! Native local speech runtimes. macOS uses opt-in experimental modules only;
//! old IDs remain metadata-only and cannot link their former inference libraries.
#[cfg(any(local_asr_sherpa, test))]
mod blocking_decode;
pub mod experimental;
pub mod foundry;
#[cfg(target_os = "windows")]
pub mod foundry_native;
#[cfg(target_os = "windows")]
pub mod foundry_provider;
#[cfg(target_os = "windows")]
pub mod foundry_runtime;
#[cfg(target_os = "windows")]
mod native_model_receipt;
pub mod sherpa;
pub mod sherpa_provider;
pub mod sherpa_runtime;
pub mod test_run;

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub use foundry_provider::FoundryLocalWhisperAsr;
#[cfg(target_os = "windows")]
#[allow(unused_imports)]
pub use foundry_runtime::FoundryLocalRuntime;
#[allow(unused_imports)]
pub use sherpa_provider::SherpaOnnxAsr;
#[allow(unused_imports)]
pub use sherpa_runtime::SherpaOnnxRuntime;

// provider id 是对外契约，已迁到平台层统一维护（宿主不再各写一份字符串）。
pub use openless_platform::asr::ids::{
    is_local_qwen3, is_local_whisper, APPLE_SPEECH_PROVIDER_ID, LOCAL_QWEN3_C_PROVIDER_ID,
    LOCAL_QWEN3_MLX_PROVIDER_ID, LOCAL_WHISPER_PROVIDER_ID, PROVIDER_ID,
};
