//! iOS platform integration (clipboard, keyboard extension status).
//!
//! 结构对齐 `android/`：Rust 运行时模块挂在这里，iOS 专属 Swift 模板放在
//! 仓库 `ios/`（由 scripts/copy-ios-scaffolding.mjs 复制到 src-tauri/gen/apple）。
//! 麦克风权限不走这里——它经 permissions.rs 的 iOS platform 模块进入现有
//! check/request 命令链，与桌面端共用 IPC 面。

#[cfg(target_os = "ios")]
pub mod clipboard;
#[cfg(target_os = "ios")]
pub mod keyboard_status;

#[cfg(target_os = "ios")]
pub use keyboard_status::KeyboardExtensionStatus;
