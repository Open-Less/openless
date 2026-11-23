//! sherpa-onnx 的运行时描述（宿主薄封装）。
//!
//! 阶段枚举、默认模型别名与 provider id 已迁到 `openless-platform::asr::descriptors`；
//! 原生运行时在 `openless-platform::asr::sherpa`（仅 Windows / Android arm64 编译）。

pub use openless_platform::asr::descriptors::*;
