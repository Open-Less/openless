//! macOS 平台后端。

#[cfg(feature = "input")]
pub mod cgevent;
#[cfg(feature = "capsule")]
pub mod capsule_window;
#[cfg(feature = "clipboard")]
pub mod clipboard;
#[cfg(feature = "selection")]
pub mod selection;
#[cfg(feature = "text")]
pub mod text;

#[cfg(feature = "ime")]
pub mod ax;
#[cfg(all(feature = "text", target_os = "macos"))]
pub mod keystroke;
#[cfg(feature = "permissions")]
pub mod guide;
#[cfg(feature = "selection")]
pub mod front_app;
#[cfg(feature = "capsule")]
pub mod window;
#[cfg(feature = "capsule")]
pub mod locale;
pub mod secure_input;
