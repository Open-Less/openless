//! 跨平台共用的纯逻辑（不依赖任何系统 API，可在任意平台单测）。

pub mod codec;
pub mod error;

/// issue-1266 诊断观测（TSF/录音链路共用）。
#[cfg(feature = "ime")]
pub mod issue1266;

/// UIA 文本单元解码（纯逻辑，所有目标可测）。
pub mod uia_text;
