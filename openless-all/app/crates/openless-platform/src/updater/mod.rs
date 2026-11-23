//! 自更新：清单解析、源选择、签名校验、Android APK 传输。
//!
//! 平台层负责**发现与验签**（清单 URL、镜像/源竞速、minisign 验签、版本比较、
//! APK 传输与校验），宿主负责**安装**（桌面 tauri-plugin-updater、Android 系统安装器 Intent）。
//!
//! 这样"更新包从哪来、是不是官方的"只有一份实现，而"怎么装到系统里"仍是宿主职责。

pub mod artifact_fallback;
pub mod manifest;
pub mod signature_index;

pub mod android;
pub mod android_logic;
pub mod android_pipeline;
