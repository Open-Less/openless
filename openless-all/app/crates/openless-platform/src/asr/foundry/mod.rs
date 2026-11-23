//! Foundry Local（Windows WinML）本地 ASR。
//!
//! Windows：通过 Foundry Local SDK 做 Whisper 推理；其它目标保留类型兼容的桩，
//! 保证宿主在非 Windows 上仍能编译并给出"引擎不可用"的明确结果。
//!
//! 路径（模型缓存/运行时/日志）由宿主经 [`crate::asr::paths`] 注入。

pub mod descriptors;
pub mod native;
pub mod provider;
pub mod runtime;

pub use descriptors::PROVIDER_ID;
pub use provider::FoundryLocalWhisperAsr;
pub use runtime::FoundryLocalRuntime;
