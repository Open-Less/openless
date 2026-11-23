//! 输入能力：按键注入 + 全局热键。
//!
//! 两个子能力彼此独立：
//! - [`inject`]：把合成按键投递出去（复制键、粘贴键、逐字输入共用底层）；
//! - [`hotkey`]：监听全局热键（macOS CGEventTap / Windows WH_KEYBOARD_LL）。
//!
//! 热键监听需要宿主提供两个行为钩子（[`hotkey::HostHooks`]）：按下热键时预取选区、
//! 松开时清理录音伴生状态。平台层不反向依赖宿主，只通过这些钩子向下调用。

pub mod combo;
pub mod global_runtime;
pub mod qa_hotkey;
pub mod hotkey;
pub mod inject;
#[cfg(target_os = "macos")]
pub mod macos_dictation_key;
pub mod mouse;
pub mod side_combo;

pub use inject::send_copy_shortcut;
pub use inject::PlatformError;
