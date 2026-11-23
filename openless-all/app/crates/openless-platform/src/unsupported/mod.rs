//! 其余平台（Android / iOS / Linux）的占位后端。
//!
//! 在对应能力迁入之前返回显式错误，避免"看起来成功"的静默失败。

#[cfg(feature = "clipboard")]
pub mod clipboard;
#[cfg(feature = "input")]
pub mod input;
#[cfg(feature = "selection")]
pub mod selection;
#[cfg(feature = "text")]
pub mod text;
