//! Windows UIA 文档读写（宿主薄封装）。
//!
//! UIA 通道（选区读取、音声编辑目标、光标上下文、编辑监听）已迁到
//! `openless-platform::ime::uia`。宿主只保留调度、安全门与异步超时包装。

pub(crate) use openless_platform::ime::uia::*;
