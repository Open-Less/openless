//! Windows 平台后端。
//!
//! 每个能力一个文件，与顶层能力 feature 一一对应；关闭某个能力时对应文件不编译。

#[cfg(feature = "capsule")]
pub mod capsule_window;
#[cfg(feature = "selection")]
pub mod front_app;
#[cfg(feature = "clipboard")]
pub mod clipboard;
#[cfg(feature = "clipboard")]
pub mod history;
#[cfg(feature = "ime-profile")]
pub mod ime_profile;
#[cfg(feature = "input")]
pub mod input;
#[cfg(feature = "selection")]
pub mod selection;
#[cfg(feature = "text")]
pub mod text;
