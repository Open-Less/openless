//! sherpa-onnx 本地 ASR 引擎（Windows / Android arm64）。
//!
//! 原生库由宿主侧构建脚本提供（`sherpa-onnx` 依赖按 target 门控），
//! 这里只包含 Rust 侧运行时与 provider。

pub mod blocking_decode;
pub mod provider;
pub mod runtime;

pub use provider::SherpaOnnxAsr;
pub use runtime::SherpaOnnxRuntime;
