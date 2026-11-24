//! 端侧实验引擎（宿主薄封装）。
//!
//! SenseVoice / EmbeddingGemma / TextStream 运行时、算力策略、逐字文本流、Swift Core ML
//! Gemma 适配与验收入口都已迁到 `openless-platform::asr::experimental`。

pub use openless_platform::asr::experimental::*;

#[cfg(all(debug_assertions, target_os = "macos", target_arch = "aarch64"))]
pub use openless_platform::asr::experimental::acceptance;
