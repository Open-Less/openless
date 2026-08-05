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
    paste_via_accessibility_with_result("") == PASTE_RESULT_SUCCESS
}

pub fn paste_via_accessibility_with_result(text: &str) -> String {
    #[cfg(target_os = "android")]
    {
        return android_impl::paste_via_accessibility_with_result(text);
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

fn is_valid_android_package_name(package_name: &str) -> bool {
    if package_name.is_empty() {
        return false;
    }
    let mut segments = package_name.split('.');
    let first = segments.next().unwrap_or("");
    if first.is_empty() || !first.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !first
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return false;
    }
    for segment in segments {
        if segment.is_empty()
            || !segment.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            || !segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return false;
        }
    }
    true
}

/// Normalizes `pkg/.Class` to `pkg/pkg.Class` for Settings.Secure comparison.
pub(crate) fn normalize_component_key(component: &str) -> Option<String> {
    let trimmed = component.trim();
    let slash = trimmed.find('/')?;
    if slash == 0 || slash == trimmed.len() - 1 {
        return None;
    }
    let package_name = trimmed[..slash].trim();
    let class_name = trimmed[slash + 1..].trim();
    if class_name.is_empty()
        || class_name
            .chars()
            .any(|c| c.is_whitespace() || c == '\n' || c == '\r')
    {
        return None;
    }
    if !is_valid_android_package_name(package_name) {
        return None;
    }
    let full_class_name = if class_name.starts_with('.') {
        format!("{package_name}{class_name}")
    } else {
        class_name.to_string()
    };
    if full_class_name
        .chars()
        .any(|c| c.is_whitespace() || c == '\n' || c == '\r' || c == '/')
    {
        return None;
    }
    Some(format!("{package_name}/{full_class_name}"))
}

pub(crate) fn parse_service_entries(raw: &str) -> Vec<String> {
    raw.split(':')
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && *entry != "null")
        .map(str::to_string)
        .collect()
}

pub(crate) fn components_equal(left: &str, right: &str) -> bool {
    let left_key = normalize_component_key(left);
    let right_key = normalize_component_key(right);
    match (left_key, right_key) {
        (Some(left_key), Some(right_key)) => left_key == right_key,
        _ => left.trim() == right.trim(),
    }
}

pub(crate) fn enabled_services_contain(services: &str, component: &str) -> bool {
    parse_service_entries(services)
        .iter()
        .any(|entry| components_equal(entry, component))
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

    fn invoke_paste_once(text: &str) -> String {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::accessibility_paste_result(env, context, text)
        }) {
            Ok(result) => result,
            Err(error) => {
                log::warn!("[android-a11y] paste IPC protocol error: {error}");
                PASTE_RESULT_IPC_PROTOCOL_ERROR.to_string()
            }
        }
    }

    pub fn paste_via_accessibility_with_result(text: &str) -> String {
        let first = invoke_paste_once(text);
        if first == PASTE_RESULT_SUCCESS {
            return first;
        }
        if super::should_retry_paste_after_failure(&first) {
            log::info!("[android-a11y] paste retry after {first}");
            thread::sleep(Duration::from_millis(200));
            let second = invoke_paste_once(text);
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
        components_equal, enabled_services_contain, normalize_component_key,
        paste_via_accessibility_with_result, parse_service_entries,
        should_retry_paste_after_failure, PASTE_RESULT_IPC_PROTOCOL_ERROR,
        PASTE_RESULT_SERVICE_NOT_CONNECTED,
    };

    const FULL: &str = "com.openless.app/com.openless.app.OpenLessAccessibilityService";
    const SHORT_FORM: &str = "com.openless.app/.OpenLessAccessibilityService";
    const THIRD_PARTY: &str = "com.example/.OtherService";
    const SIMILAR_CLASS: &str =
        "com.openless.app/com.openless.app.OpenLessAccessibilityServiceFake";

    #[cfg(not(target_os = "android"))]
    #[test]
    fn paste_result_constant_off_android() {
        assert_eq!(
            paste_via_accessibility_with_result(""),
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

    #[test]
    fn normalize_component_key_treats_short_and_full_forms_as_equal() {
        assert_eq!(
            normalize_component_key(SHORT_FORM),
            Some(FULL.to_string())
        );
        assert_eq!(normalize_component_key(FULL), Some(FULL.to_string()));
        assert!(components_equal(SHORT_FORM, FULL));
    }

    #[test]
    fn enabled_services_contain_matches_multi_service_colon_list() {
        let services = format!("{THIRD_PARTY}:{SHORT_FORM}");
        assert!(enabled_services_contain(&services, FULL));
        assert_eq!(parse_service_entries(&services).len(), 2);
    }

    #[test]
    fn enabled_services_contain_rejects_similar_class_name_substring() {
        assert!(!enabled_services_contain(SIMILAR_CLASS, FULL));
        assert!(!components_equal(SIMILAR_CLASS, FULL));
    }

    #[test]
    fn enabled_services_contain_returns_false_for_empty_list() {
        assert!(!enabled_services_contain("", FULL));
    }
}
