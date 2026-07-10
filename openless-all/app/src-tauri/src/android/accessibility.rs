//! Android accessibility service integration for keyboard detection and paste insertion.

use serde::Serialize;

use crate::android::types::{AndroidAccessibilityState, AndroidAccessibilityStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAccessibilityPermissionResult {
    pub launched: bool,
    pub message: String,
}

pub fn get_android_accessibility_status() -> AndroidAccessibilityStatus {
    #[cfg(target_os = "android")]
    {
        android_impl::get_android_accessibility_status()
    }

    #[cfg(not(target_os = "android"))]
    {
        AndroidAccessibilityStatus {
            state: AndroidAccessibilityState::NotAndroid,
            enabled: false,
            message: "Android accessibility backend is only available on Android".to_string(),
        }
    }
}

pub fn request_android_accessibility_permission() -> AndroidAccessibilityPermissionResult {
    #[cfg(target_os = "android")]
    {
        android_impl::request_android_accessibility_permission()
    }

    #[cfg(not(target_os = "android"))]
    {
        AndroidAccessibilityPermissionResult {
            launched: false,
            message: "Android accessibility settings are only available on Android".to_string(),
        }
    }
}

pub fn paste_via_accessibility() -> bool {
    #[cfg(target_os = "android")]
    {
        return android_impl::paste_via_accessibility();
    }

    #[cfg(not(target_os = "android"))]
    false
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{AndroidAccessibilityPermissionResult, AndroidAccessibilityStatus};
    use crate::android::types::{AndroidAccessibilityState, AndroidAccessibilityStatus as Status};

    pub fn get_android_accessibility_status() -> AndroidAccessibilityStatus {
        let enabled = match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_enabled(env, context)
        }) {
            Ok(enabled) => enabled,
            Err(error) => {
                return Status {
                    state: AndroidAccessibilityState::NotEnabled,
                    enabled: false,
                    message: error,
                };
            }
        };
        if !enabled {
            return Status {
                state: AndroidAccessibilityState::NotEnabled,
                enabled: false,
                message: "请在系统设置中启用 OpenLess 无障碍服务".to_string(),
            };
        }

        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_operational(env, context)
        }) {
            Ok(true) => Status {
                state: AndroidAccessibilityState::Enabled,
                enabled: true,
                message: "无障碍服务已启用".to_string(),
            },
            Ok(false) => Status {
                state: AndroidAccessibilityState::NotEnabled,
                enabled: false,
                message: "无障碍服务已开启，但当前未运行或已被系统标记为故障，请重新开启 OpenLess 无障碍服务".to_string(),
            },
            Err(error) => Status {
                state: AndroidAccessibilityState::NotEnabled,
                enabled: false,
                message: error,
            },
        }
    }

    pub fn request_android_accessibility_permission() -> AndroidAccessibilityPermissionResult {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::launch_accessibility_settings(env, context)
        }) {
            Ok(()) => AndroidAccessibilityPermissionResult {
                launched: true,
                message: "已打开无障碍设置".to_string(),
            },
            Err(error) => AndroidAccessibilityPermissionResult {
                launched: false,
                message: error,
            },
        }
    }

    pub fn paste_via_accessibility() -> bool {
        crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_paste(env, context)
        })
        .unwrap_or(false)
    }
}
