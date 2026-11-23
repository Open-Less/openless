//! sherpa-onnx 本地 ASR 的运行时描述（宿主薄封装）。
//!
//! 阶段枚举与 provider id 已迁到 `openless-platform::asr::sherpa`（纯逻辑，不链接
//! 原生库）；原生运行时代码仍在本目录，等 WP3 后续步骤迁入。

pub use openless_platform::asr::sherpa::*;
