//! Android 运行时权限探测（经宿主注入的 JNI 环境）。

use crate::permissions::PermissionStatus;

/// Android 没有 macOS 那种 Accessibility 授权概念。
pub fn check_accessibility() -> PermissionStatus {
    PermissionStatus::NotApplicable
}

pub fn request_accessibility() -> PermissionStatus {
    PermissionStatus::NotApplicable
}

pub fn check_microphone() -> PermissionStatus {
    match super::env() {
        Some(env) => match env.check_permission("android.permission.RECORD_AUDIO") {
            Ok(true) => PermissionStatus::Granted,
            Ok(false) => PermissionStatus::Denied,
            Err(error) => {
                log::warn!("[mic] Android RECORD_AUDIO check failed: {error}");
                PermissionStatus::NotDetermined
            }
        },
        None => PermissionStatus::NotDetermined,
    }
}

pub fn request_microphone() -> PermissionStatus {
    match super::env() {
        Some(env) => match env.request_permission("android.permission.RECORD_AUDIO") {
            Ok(true) => PermissionStatus::Granted,
            Ok(false) => PermissionStatus::Denied,
            Err(error) => {
                log::warn!("[mic] Android RECORD_AUDIO request failed: {error}");
                PermissionStatus::Denied
            }
        },
        None => PermissionStatus::NotDetermined,
    }
}

/// Android 侧录音设备由系统统一管理，不需要枚举。
pub fn has_microphone_input_device() -> bool {
    false
}
