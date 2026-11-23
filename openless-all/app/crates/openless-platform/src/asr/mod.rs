//! 本地 ASR。
//!
//! 分两层：
//! - **元数据/纯逻辑**（`asr` feature）：provider id 契约、模型文件收据、
//!   sherpa 运行时阶段描述、诊断事件钩子；
//! - **原生运行时**（`asr-sherpa` / `asr-foundry` / `asr-apple` 子 feature）：
//!   真正链接厂商 SDK 的引擎实现。
//!
//! 宿主仍然拥有"选哪个引擎、什么时候启动"的策略；平台层提供引擎本身。

pub mod descriptors;
pub mod diagnostics;
pub mod ids;
pub mod paths;
pub mod receipt;

// Foundry Local：Windows 用 WinML；其它目标保留类型兼容桩。
#[cfg(feature = "asr-foundry")]
pub mod foundry;

// Apple Speech 只在 macOS 编译（Speech.framework）。
#[cfg(all(feature = "asr-apple", target_os = "macos"))]
pub mod apple;

// 只有 Windows 与 Android arm64 提供 sherpa-onnx 静态库；其它目标即使开了
// `asr-sherpa` 也不编译这个模块（引擎在宿主侧返回"不可用"）。
#[cfg(all(
    feature = "asr-sherpa",
    any(target_os = "windows", target_os = "android")
))]
pub mod sherpa;

/// 音频消费接口：引擎从录音管线接收 16 kHz 单声道 Int16 PCM。
///
/// 契约定义在内核 `openless_core::AudioConsumer`（宿主、平台层、引擎共用一份），
/// 这里只做别名，避免三处各写一个 trait。
pub use openless_core::AudioConsumer as PcmConsumer;
