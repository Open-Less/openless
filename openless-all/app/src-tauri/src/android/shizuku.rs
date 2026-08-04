//! Android Shizuku integration for optional accessibility recovery and paste injection.

use serde::Serialize;

use crate::android::types::{
    AndroidAccessibilityDiagnosis, AndroidAccessibilityRecoveryOutcome,
    AndroidAccessibilityRecoveryResult, AndroidShizukuState, AndroidShizukuStatus,
};

pub const PASTE_RESULT_SUCCESS: &str = "SUCCESS";
pub const PASTE_RESULT_SHIZUKU_UNAVAILABLE: &str = "SHIZUKU_UNAVAILABLE";
pub const PASTE_RESULT_INJECT_FAILED: &str = "INJECT_FAILED";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidShizukuPermissionResult {
    pub launched: bool,
    #[serde(default)]
    pub message: String,
    pub message_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidShizukuOpenResult {
    pub launched: bool,
    #[serde(default)]
    pub message: String,
    pub message_key: String,
}

pub fn get_android_shizuku_status() -> AndroidShizukuStatus {
    #[cfg(target_os = "android")]
    {
        android_impl::get_android_shizuku_status()
    }

    #[cfg(not(target_os = "android"))]
    {
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
}

pub fn request_android_shizuku_permission() -> AndroidShizukuPermissionResult {
    #[cfg(target_os = "android")]
    {
        android_impl::request_android_shizuku_permission()
    }

    #[cfg(not(target_os = "android"))]
    {
        AndroidShizukuPermissionResult {
            launched: false,
            message: String::new(),
            message_key: "not_android".to_string(),
        }
    }
}

pub fn open_shizuku_app() -> AndroidShizukuOpenResult {
    #[cfg(target_os = "android")]
    {
        android_impl::open_shizuku_app()
    }

    #[cfg(not(target_os = "android"))]
    {
        AndroidShizukuOpenResult {
            launched: false,
            message: String::new(),
            message_key: "not_android".to_string(),
        }
    }
}

pub fn recover_android_accessibility(confirmed: bool) -> AndroidAccessibilityRecoveryResult {
    #[cfg(target_os = "android")]
    {
        android_impl::recover_android_accessibility(confirmed)
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = confirmed;
        AndroidAccessibilityRecoveryResult {
            outcome: AndroidAccessibilityRecoveryOutcome::ShizukuUnavailable,
            message: String::new(),
            message_key: "not_android".to_string(),
        }
    }
}

pub fn paste_via_shizuku_with_result() -> String {
    #[cfg(target_os = "android")]
    {
        return android_impl::paste_via_shizuku_with_result();
    }

    #[cfg(not(target_os = "android"))]
    PASTE_RESULT_SHIZUKU_UNAVAILABLE.to_string()
}

mod json {
    use super::{
        AndroidAccessibilityDiagnosis, AndroidAccessibilityRecoveryOutcome,
        AndroidAccessibilityRecoveryResult, AndroidShizukuState, AndroidShizukuStatus,
    };

    fn read_message_key(value: &serde_json::Value) -> String {
        value
            .get("messageKey")
            .or_else(|| value.get("message_key"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    }

    pub fn parse_shizuku_status(json: &str) -> Result<AndroidShizukuStatus, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|error| format!("parse Shizuku status: {error}"))?;
        let state = parse_shizuku_state(value.get("state").and_then(|v| v.as_str()))?;
        let message_key = read_message_key(&value);
        let accessibility = value
            .get("accessibility")
            .ok_or_else(|| "missing accessibility diagnosis".to_string())?;
        let last_permission_message_key = value
            .get("lastPermissionMessageKey")
            .or_else(|| value.get("last_permission_message_key"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(AndroidShizukuStatus {
            state,
            message: String::new(),
            message_key,
            accessibility: AndroidAccessibilityDiagnosis {
                registered: accessibility
                    .get("registered")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                operational: accessibility
                    .get("operational")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                message: String::new(),
                message_key: read_message_key(accessibility),
            },
            last_permission_message_key,
        })
    }

    pub fn parse_recovery_result(json: &str) -> Result<AndroidAccessibilityRecoveryResult, String> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|error| format!("parse recovery result: {error}"))?;
        let outcome = parse_recovery_outcome(value.get("outcome").and_then(|v| v.as_str()))?;
        Ok(AndroidAccessibilityRecoveryResult {
            outcome,
            message: String::new(),
            message_key: read_message_key(&value),
        })
    }

    fn parse_shizuku_state(raw: Option<&str>) -> Result<AndroidShizukuState, String> {
        match raw {
            Some("NotInstalled") => Ok(AndroidShizukuState::NotInstalled),
            Some("NotRunning") => Ok(AndroidShizukuState::NotRunning),
            Some("NotAuthorized") => Ok(AndroidShizukuState::NotAuthorized),
            Some("Authorized") => Ok(AndroidShizukuState::Authorized),
            Some("BinderDead") => Ok(AndroidShizukuState::BinderDead),
            Some(other) => Err(format!("unknown Shizuku state: {other}")),
            None => Err("missing Shizuku state".to_string()),
        }
    }

    fn parse_recovery_outcome(
        raw: Option<&str>,
    ) -> Result<AndroidAccessibilityRecoveryOutcome, String> {
        match raw {
            Some("Success") => Ok(AndroidAccessibilityRecoveryOutcome::Success),
            Some("WriteRejected") => Ok(AndroidAccessibilityRecoveryOutcome::WriteRejected),
            Some("ServiceNotBound") => Ok(AndroidAccessibilityRecoveryOutcome::ServiceNotBound),
            Some("ShizukuUnavailable") => {
                Ok(AndroidAccessibilityRecoveryOutcome::ShizukuUnavailable)
            }
            Some("UserNotConfirmed") => Ok(AndroidAccessibilityRecoveryOutcome::UserNotConfirmed),
            Some("ShellFailed") => Ok(AndroidAccessibilityRecoveryOutcome::ShellFailed),
            Some(other) => Err(format!("unknown recovery outcome: {other}")),
            None => Err("missing recovery outcome".to_string()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            parse_recovery_outcome, parse_recovery_result, parse_shizuku_state,
            parse_shizuku_status,
        };
        use crate::android::types::{AndroidAccessibilityRecoveryOutcome, AndroidShizukuState};

        #[test]
        fn parses_shizuku_status_json() {
            let status = parse_shizuku_status(
                r#"{"state":"Authorized","messageKey":"authorized_can_recover","accessibility":{"registered":true,"operational":false,"messageKey":"registered_stale"}}"#,
            )
            .expect("status");
            assert_eq!(status.state, AndroidShizukuState::Authorized);
            assert_eq!(status.message_key, "authorized_can_recover");
            assert!(status.accessibility.registered);
            assert!(!status.accessibility.operational);
            assert_eq!(status.accessibility.message_key, "registered_stale");
        }

        #[test]
        fn parses_recovery_result_json() {
            let result = parse_recovery_result(
                r#"{"outcome":"WriteRejected","messageKey":"concurrent_change"}"#,
            )
            .expect("recovery");
            assert_eq!(
                result.outcome,
                AndroidAccessibilityRecoveryOutcome::WriteRejected
            );
            assert_eq!(result.message_key, "concurrent_change");
        }

        #[test]
        fn rejects_unknown_shizuku_state() {
            assert!(parse_shizuku_state(Some("Broken")).is_err());
            assert!(parse_recovery_outcome(Some("Broken")).is_err());
        }
    }
}

#[cfg(target_os = "android")]
mod android_impl {
    use super::{
        AndroidAccessibilityRecoveryOutcome, AndroidAccessibilityRecoveryResult,
        AndroidShizukuOpenResult, AndroidShizukuPermissionResult, AndroidShizukuStatus,
    };
    use crate::android::types::{AndroidShizukuState, AndroidShizukuStatus as Status};

    pub fn get_android_shizuku_status() -> AndroidShizukuStatus {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::shizuku_get_status_json(env, context)
        }) {
            Ok(json) => super::json::parse_shizuku_status(&json).unwrap_or_else(|error| Status {
                state: AndroidShizukuState::NotRunning,
                message: String::new(),
                message_key: "status_parse_failed".to_string(),
                accessibility: crate::android::types::AndroidAccessibilityDiagnosis {
                    registered: false,
                    operational: false,
                    message: String::new(),
                    message_key: "status_parse_failed".to_string(),
                },
                last_permission_message_key: None,
            }),
            Err(_error) => Status {
                state: AndroidShizukuState::NotRunning,
                message: String::new(),
                message_key: "jni_error".to_string(),
                accessibility: crate::android::types::AndroidAccessibilityDiagnosis {
                    registered: false,
                    operational: false,
                    message: String::new(),
                    message_key: "jni_error".to_string(),
                },
                last_permission_message_key: None,
            },
        }
    }

    pub fn request_android_shizuku_permission() -> AndroidShizukuPermissionResult {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::shizuku_request_permission(env, context)
        }) {
            Ok(launched) => AndroidShizukuPermissionResult {
                launched,
                message: String::new(),
                message_key: if launched {
                    "launched".to_string()
                } else {
                    "launch_failed".to_string()
                },
            },
            Err(_error) => AndroidShizukuPermissionResult {
                launched: false,
                message: String::new(),
                message_key: "jni_error".to_string(),
            },
        }
    }

    pub fn open_shizuku_app() -> AndroidShizukuOpenResult {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::shizuku_open_app(env, context)
        }) {
            Ok(launched) => AndroidShizukuOpenResult {
                launched,
                message: String::new(),
                message_key: if launched {
                    "launched".to_string()
                } else {
                    "launch_failed".to_string()
                },
            },
            Err(_error) => AndroidShizukuOpenResult {
                launched: false,
                message: String::new(),
                message_key: "jni_error".to_string(),
            },
        }
    }

    pub fn recover_android_accessibility(confirmed: bool) -> AndroidAccessibilityRecoveryResult {
        if !confirmed {
            return AndroidAccessibilityRecoveryResult {
                outcome: AndroidAccessibilityRecoveryOutcome::UserNotConfirmed,
                message: String::new(),
                message_key: "user_not_confirmed".to_string(),
            };
        }

        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::shizuku_recover_accessibility_json(env, context, true)
        }) {
            Ok(json) => super::json::parse_recovery_result(&json).unwrap_or_else(|_error| {
                AndroidAccessibilityRecoveryResult {
                    outcome: AndroidAccessibilityRecoveryOutcome::ShellFailed,
                    message: String::new(),
                    message_key: "parse_failed".to_string(),
                }
            }),
            Err(_error) => AndroidAccessibilityRecoveryResult {
                outcome: AndroidAccessibilityRecoveryOutcome::ShizukuUnavailable,
                message: String::new(),
                message_key: "jni_error".to_string(),
            },
        }
    }

    pub fn paste_via_shizuku_with_result() -> String {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::shizuku_inject_paste_key(env, context)
        }) {
            Ok(true) => super::PASTE_RESULT_SUCCESS.to_string(),
            Ok(false) => super::PASTE_RESULT_INJECT_FAILED.to_string(),
            Err(error) => {
                log::info!("[android-shizuku] paste inject unavailable: {error}");
                super::PASTE_RESULT_SHIZUKU_UNAVAILABLE.to_string()
            }
        }
    }
}
