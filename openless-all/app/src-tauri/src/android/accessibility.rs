//! Android accessibility service integration for keyboard detection and paste insertion.

use serde::Serialize;

use crate::android::types::{AndroidAccessibilityState, AndroidAccessibilityStatus};

pub const PASTE_RESULT_SUCCESS: &str = "SUCCESS";
pub const PASTE_RESULT_SERVICE_NOT_CONNECTED: &str = "SERVICE_NOT_CONNECTED";
pub const PASTE_RESULT_IPC_PROTOCOL_ERROR: &str = "IPC_PROTOCOL_ERROR";

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
            operational: false,
            message: String::new(),
            message_key: "not_android".to_string(),
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
    paste_via_accessibility_with_result() == PASTE_RESULT_SUCCESS
}

pub fn paste_via_accessibility_with_result() -> String {
    #[cfg(target_os = "android")]
    {
        return android_impl::paste_via_accessibility_with_result();
    }

    #[cfg(not(target_os = "android"))]
    PASTE_RESULT_SERVICE_NOT_CONNECTED.to_string()
}

pub fn is_accessibility_enabled() -> bool {
    #[cfg(target_os = "android")]
    {
        return android_impl::is_accessibility_enabled();
    }

    #[cfg(not(target_os = "android"))]
    false
}

/// Only retry paste when Kotlin explicitly reports the accessibility process is unreachable.
/// TIMEOUT and JNI/protocol errors must not retry: the first paste may already have succeeded.
pub(crate) fn should_retry_paste_after_failure(reason: &str) -> bool {
    reason == PASTE_RESULT_SERVICE_NOT_CONNECTED
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{
        AndroidAccessibilityPermissionResult, PASTE_RESULT_IPC_PROTOCOL_ERROR,
        PASTE_RESULT_SERVICE_NOT_CONNECTED, PASTE_RESULT_SUCCESS,
    };
    use crate::android::types::{AndroidAccessibilityState, AndroidAccessibilityStatus as Status};
    use std::thread;
    use std::time::Duration;

    pub fn is_accessibility_enabled() -> bool {
        crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_enabled(env, context)
        })
        .unwrap_or(false)
    }

    pub fn get_android_accessibility_status() -> Status {
        let enabled = match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_enabled(env, context)
        }) {
            Ok(enabled) => enabled,
            Err(error) => {
                return Status {
                    state: AndroidAccessibilityState::NotEnabled,
                    enabled: false,
                    operational: false,
                    message: error,
                    message_key: "status_read_failed".to_string(),
                };
            }
        };
        if !enabled {
            return Status {
                state: AndroidAccessibilityState::NotEnabled,
                enabled: false,
                operational: false,
                message: String::new(),
                message_key: "not_enabled".to_string(),
            };
        }

        let operational = crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_operational(env, context)
        })
        .unwrap_or(false);

        Status {
            state: AndroidAccessibilityState::Enabled,
            enabled: true,
            operational,
            message: String::new(),
            message_key: if operational {
                "operational".to_string()
            } else {
                "authorized_not_connected".to_string()
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

    fn invoke_paste_once() -> String {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_paste_result(env, context)
        }) {
            Ok(result) => result,
            Err(error) => {
                log::warn!("[android-a11y] paste IPC protocol error: {error}");
                PASTE_RESULT_IPC_PROTOCOL_ERROR.to_string()
            }
        }
    }

    pub fn paste_via_accessibility_with_result() -> String {
        let first = invoke_paste_once();
        if first == PASTE_RESULT_SUCCESS {
            return first;
        }
        if super::should_retry_paste_after_failure(&first) {
            log::info!("[android-a11y] paste retry after {first}");
            thread::sleep(Duration::from_millis(200));
            let second = invoke_paste_once();
            log::info!("[android-a11y] paste retry result={second}");
            return second;
        }
        if first == "TIMEOUT" {
            log::warn!("[android-a11y] paste timed out without retry; text remains on clipboard");
        } else {
            log::warn!("[android-a11y] paste failed reason={first}");
        }
        first
    }
}

#[cfg(test)]
mod tests {
    use super::{
        paste_via_accessibility_with_result, should_retry_paste_after_failure,
        PASTE_RESULT_IPC_PROTOCOL_ERROR, PASTE_RESULT_SERVICE_NOT_CONNECTED,
    };

    #[cfg(not(target_os = "android"))]
    #[test]
    fn paste_result_constant_off_android() {
        assert_eq!(
            paste_via_accessibility_with_result(),
            PASTE_RESULT_SERVICE_NOT_CONNECTED
        );
    }

    #[test]
    fn should_retry_only_service_not_connected() {
        assert!(should_retry_paste_after_failure(
            PASTE_RESULT_SERVICE_NOT_CONNECTED
        ));
        assert!(!should_retry_paste_after_failure("TIMEOUT"));
        assert!(!should_retry_paste_after_failure(
            PASTE_RESULT_IPC_PROTOCOL_ERROR
        ));
        assert!(!should_retry_paste_after_failure("NO_FOCUSED_EDITOR"));
        assert!(!should_retry_paste_after_failure("PASTE_REJECTED"));
        assert!(!should_retry_paste_after_failure("SUCCESS"));
    }
}
