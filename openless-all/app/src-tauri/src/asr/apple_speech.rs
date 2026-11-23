//! Apple Speech 本地识别（宿主薄封装）。
//!
//! 实现已迁到 `openless-platform::asr::apple`（macOS Speech.framework）；
//! 宿主只保留引擎注册与 provider 常量引用。

pub use openless_platform::asr::apple::*;
