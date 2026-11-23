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

// ── Shizuku（仅 Android 有实现） ──────────────────────────────────

use openless_core::android_types::{
    AndroidAccessibilityDiagnosis, AndroidShizukuState, AndroidShizukuStatus,
};



/// Shizuku 授权请求结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidShizukuPermissionResult {
    pub launched: bool,
    pub message: String,
    pub message_key: String,
}

/// 打开 Shizuku 应用的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidShizukuOpenResult {
    pub launched: bool,
    pub message: String,
    pub message_key: String,
}

/// Shizuku 状态：非 Android 上明确标记 NotAndroid。
pub fn get_android_shizuku_status() -> AndroidShizukuStatus {
    AndroidShizukuStatus {
        state: AndroidShizukuState::NotAndroid,
        message: String::new(),
        message_key: "not_android".to_string(),
        accessibility: AndroidAccessibilityDiagnosis {
            registered: false,
            operational: false,
            message: String::new(),
            message_key: "not_android".to_string(),
        },
        last_permission_message_key: None,
    }
}

pub fn request_android_shizuku_permission() -> AndroidShizukuPermissionResult {
    AndroidShizukuPermissionResult {
        launched: false,
        message: "Shizuku is only available on Android".to_string(),
        message_key: "not_android".to_string(),
    }
}

pub fn open_shizuku_app() -> AndroidShizukuOpenResult {
    AndroidShizukuOpenResult {
        launched: false,
        message: "Shizuku is only available on Android".to_string(),
        message_key: "not_android".to_string(),
    }
}

/// 用 Shizuku 粘贴：非 Android 上报告不可用。
pub fn paste_via_shizuku_with_result(_text: &str) -> String {
    "SHIZUKU_UNAVAILABLE".to_string()
}

/// 通过 Shizuku 恢复无障碍：非 Android 上视为不可用。
pub fn recover_android_accessibility(
    _confirmed: bool,
) -> openless_core::android_types::AndroidAccessibilityRecoveryResult {
    openless_core::android_types::AndroidAccessibilityRecoveryResult {
        outcome: openless_core::android_types::AndroidAccessibilityRecoveryOutcome::ShizukuUnavailable,
        message: "Android only".to_string(),
        message_key: "not_android".to_string(),
    }
}
