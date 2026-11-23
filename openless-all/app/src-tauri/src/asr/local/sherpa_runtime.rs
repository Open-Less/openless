//! sherpa-onnx 运行时（宿主薄封装）。
//!
//! 实现已迁到 `openless-platform::asr::sherpa::runtime`；原生库仍由宿主的构建
//! 脚本提供（`local_asr_sherpa` cfg 决定是否编译这个模块）。

pub use openless_platform::asr::sherpa::runtime::*;
