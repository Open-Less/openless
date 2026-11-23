//! macOS AX 文档读写（宿主薄封装）。
//!
//! AX 通道已迁到 `openless-platform::macos::ax`。宿主只保留调度、异步超时包装
//! 与安全门结果到对外状态的映射。

pub(crate) use openless_platform::macos::ax::*;
