//! 音频能力。
//!
//! 目前迁入两块自包含的能力：
//! - [`mute`]：录音期间临时静音系统输出，结束后恢复原状态；
//! - [`cue`]：不依赖 WebView 的合成提示音（"开始录音"的两音提示）。
//!
//! 采集接线与回声参考（AEC）仍留在宿主：它们与宿主的录音状态机、诊断回调绑定，
//! 需要先定义宿主侧的对象模型才能搬。

// 采集与回环参考需要 cpal；只在桌面平台编译。
pub mod capture;

// 提示音需要 cpal；只在桌面平台编译，其余平台没有这个模块。
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub mod cue;
pub mod mute;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use cue::{play_start, stop as stop_cue};
pub use mute::AudioMuteGuard;
