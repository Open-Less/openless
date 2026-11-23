//! 非 Android 目标上的 Android 能力桩。
//!
//! 命令层与设置页在所有平台都编译，需要这些符号存在；真实实现只在 Android 上由
//! `openless-platform::android` 提供。这里返回"不适用"而不是 panic，也不是静默成功。
#![cfg(not(target_os = "android"))]

use crate::android::types::{AndroidAccessibilityState, AndroidAccessibilityStatus};

/// 无障碍状态：非 Android 上明确标记 NotAndroid。
pub fn get_android_accessibility_status() -> AndroidAccessibilityStatus {
    AndroidAccessibilityStatus {
        state: AndroidAccessibilityState::NotAndroid,
        enabled: false,
        operational: false,
        message: String::new(),
        message_key: "not_android".to_string(),
    }
}

/// 请求无障碍权限：非 Android 上不做任何事。
pub fn request_android_accessibility_permission() -> AndroidAccessibilityPermissionResult {
    AndroidAccessibilityPermissionResult {
        launched: false,
        message: "Android accessibility settings are only available on Android".to_string(),
    }
}

use serde::Serialize;

/// 权限请求结果（与 Android 实现同构）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAccessibilityPermissionResult {
    pub launched: bool,
    pub message: String,
}
