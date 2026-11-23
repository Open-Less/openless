//! Foundry Local（Windows WinML）本地 ASR。
//!
//! Windows：通过 Foundry Local SDK 做 Whisper 推理；其它目标保留类型兼容的桩，
//! 保证宿主在非 Windows 上仍能编译并给出"引擎不可用"的明确结果。
//!
//! 路径（模型缓存/运行时/日志）由宿主经 [`crate::asr::paths`] 注入。

// provider id 与阶段枚举是纯元数据，任何平台都需要（宿主的能力查询会引用）。
pub mod descriptors;

// 真正的运行时需要 WinML / Foundry SDK，只在启用了 `asr-foundry` 的目标编译。
#[cfg(feature = "asr-foundry")]
pub mod native;
#[cfg(feature = "asr-foundry")]
pub mod provider;
#[cfg(feature = "asr-foundry")]
pub mod runtime;

pub use descriptors::PROVIDER_ID;
#[cfg(feature = "asr-foundry")]
pub use provider::FoundryLocalWhisperAsr;
#[cfg(feature = "asr-foundry")]
pub use runtime::FoundryLocalRuntime;
