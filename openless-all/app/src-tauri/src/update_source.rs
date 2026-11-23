//! 更新源发现与签名校验（宿主薄封装）。
//!
//! 实现已迁到 `openless-platform::updater`：清单 URL、CDN/GitHub 竞速、minisign 验签、
//! Android APK 传输都在那里。宿主只保留安装动作（桌面 tauri-plugin-updater、
//! Android 系统安装器 Intent）。

pub use openless_platform::updater::manifest::*;
pub use openless_platform::updater::{artifact_fallback, signature_index};
