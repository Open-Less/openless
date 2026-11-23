//! macOS 平台后端。

#[cfg(feature = "input")]
pub mod cgevent;
#[cfg(feature = "clipboard")]
pub mod clipboard;
#[cfg(feature = "selection")]
pub mod selection;
#[cfg(feature = "text")]
pub mod text;
