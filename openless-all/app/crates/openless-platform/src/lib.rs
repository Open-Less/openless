//! OpenLess 平台能力层。
//!
//! # 分层
//!
//! - `openless-core` 只定义端口 trait 与业务规则，不含平台实现；
//! - **本 crate** 承担所有"与操作系统打交道"的实现；
//! - 宿主（Tauri、Harmony、Android）只做 UI、IPC 与进程模型。
//!
//! # 可选编译模块
//!
//! 结构像内核的 Kconfig：**能力**是 feature，**平台**由 `target_os` 自动选。
//! 默认全部编进去；裁剪时：
//!
//! ```text
//! cargo build --no-default-features --features clipboard,text
//! ```
//!
//! 关闭某个能力时，依赖它的能力会被 Cargo 的 feature 依赖一起关掉，
//! 而不是留下悬空引用。
//!
//! # 目录
//!
//! ```text
//! src/
//!   clipboard.rs       剪贴板门面
//!   text.rs            文本写入门面
//!   selection.rs       选区模拟复制门面
//!   permissions/       权限探测（macos / android / other）
//!   storage/           平台加固的原子文件存储
//!   audio/             静音 + 提示音（含 mute/ 子模块）
//!   credentials/       macOS Keychain 原语
//!   input/
//!     inject.rs        按键注入
//!     hotkey/          全局热键（macos / windows / unsupported）
//!     side_combo/      侧键组合 + keys 解析
//!     combo.rs         自定义组合键
//!     global_runtime.rs global-hotkey 共享运行时
//!     mouse.rs         Mouse4/5 侧听写
//!     macos_dictation_key.rs  macOS Fn 听写键
//!   common/            跨平台纯逻辑（codec / error）
//!   windows/ macos/ unsupported/   按平台分的后端实现
//! ```

#[cfg(feature = "asr")]
pub mod asr;
#[cfg(feature = "audio")]
pub mod audio;
#[cfg(feature = "capsule")]
pub mod capsule;
#[cfg(feature = "clipboard")]
pub mod clipboard;
pub mod common;
#[cfg(feature = "credentials")]
pub mod credentials;
#[cfg(feature = "input")]
pub mod input;
#[cfg(feature = "permissions")]
pub mod permissions;
#[cfg(feature = "selection")]
pub mod selection;
#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "text")]
pub mod text;
#[cfg(feature = "updater")]
pub mod updater;

/// 与宿主同构的类型别名：内核共享类型从这里转发，迁移过来的代码不用改 `crate::types::`。
#[cfg(feature = "input")]
pub(crate) mod types {
    pub use openless_core::shared_types::*;
}

/// 与宿主同构的别名：侧键规范化函数来自内核。
#[cfg(feature = "input")]
pub(crate) mod shortcut_binding {
    pub use openless_core::{is_side_specific_modifier_tag, normalize_side_modifier_tag};
}

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")]
pub mod macos;
/// Android。能力实现经宿主注入的 JNI 接缝（[`android::AndroidEnv`]）。
#[cfg(target_os = "android")]
pub mod android;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod unsupported;

#[cfg(feature = "clipboard")]
pub use clipboard::{Clipboard, ClipboardError, Intent, Snapshot};
#[cfg(feature = "selection")]
pub use selection::{capture_via_simulated_copy, CaptureOutcome};
