//! 原生模型的文件收据（宿主薄封装）。
//!
//! 实现已迁到 `openless-platform::asr::receipt`：只读写一个描述已安装文件集合的
//! 小 JSON，不初始化厂商 SDK，因此与平台无关。

pub(crate) use openless_platform::asr::receipt::*;
