//! 权限探测能力。
//!
//! 三套平台后端，语义差异是**真的**，不做假统一：
//!
//! | 平台 | 辅助功能 | 麦克风 |
//! |---|---|---|
//! | macOS | TCC / `AXIsProcessTrusted`，可请求授权 | `AVAudioApplication.recordPermission` |
//! | Windows | 不存在该概念 → `NotApplicable` | 只探测"是否被显式拒绝"（注册表） |
//! | Android | 不存在该概念 → `NotApplicable` | 运行时权限（JNI） |
//!
//! [`PermissionStatus::NotApplicable`] 是**合法值**，调用方必须显式处理，
//! 不要把它当成"拒绝"。

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos as platform;

// Android 后端放在平台层的 android/ 下，与其它平台平级。
#[cfg(target_os = "android")]
pub use crate::android::permissions as platform;

#[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
#[path = "other.rs"]
pub mod other;
#[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
pub use other as platform;

// 系统权限请求 / 检查（macOS / Windows）。
//
// Mirrors Swift `Sources/OpenLessHotkey/AccessibilityPermission.swift` +
// `Sources/OpenLessRecorder/MicrophonePermission.swift`.
//
// - macOS Accessibility: `AXIsProcessTrusted` check;
//   `AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: true})` shows the system
//   authorization dialog.
// - macOS Microphone: `AVAudioApplication.shared.recordPermission`, with
//   `AVCaptureDevice.authorizationStatus` when that reading is not granted.
// - Windows: cpal needs no Accessibility-equivalent permission; the mic shows the one-time
//   system prompt on Win10+ at first use. With no available input device, return `NoDevice`
//   (distinct from permission `Denied`) so "no microphone plugged in" is not misreported as
//   "microphone not authorized". See issue #779.


#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatus {
    Granted,
    Denied,
    NotDetermined,
    Restricted,
    /// The current platform has no such permission (e.g. Accessibility on Windows).
    NotApplicable,
    /// No usable microphone input device right now (not a permission problem).
    NoDevice,
}

/// Whether an error string implies "no usable input device right now" (distinct from permission denied).
/// cpal's messages vary by platform, so this is a coarse keyword match.
pub fn is_no_device_error(lower: &str) -> bool {
    [
        "no default input device",
        "no default input",
        "no input device",
        "no device",
        "device not found",
        "device not available",
        "not connected",
        "unplugged",
        "disconnected",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

// AVAudioApplicationRecordPermission FourCC raw values (NSInteger).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const RECORD_PERMISSION_GRANTED: i64 = 0x6772_6e74; // 'grnt'
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const RECORD_PERMISSION_DENIED: i64 = 0x6465_6e79; // 'deny'
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const RECORD_PERMISSION_UNDETERMINED: i64 = 0x756e_6474; // 'undt'

/// Map `AVAudioApplication.recordPermission`. Unknown values stay undetermined;
/// they are not a granted TCC status.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn map_record_permission(raw: i64) -> PermissionStatus {
    match raw {
        RECORD_PERMISSION_GRANTED => PermissionStatus::Granted,
        RECORD_PERMISSION_DENIED => PermissionStatus::Denied,
        RECORD_PERMISSION_UNDETERMINED => PermissionStatus::NotDetermined,
        _ => PermissionStatus::NotDetermined,
    }
}

/// Map `AVCaptureDevice.authorizationStatus` (`AVAuthorizationStatus`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn map_capture_authorization(raw: i64) -> PermissionStatus {
    match raw {
        3 => PermissionStatus::Granted,
        2 => PermissionStatus::Denied,
        1 => PermissionStatus::Restricted,
        0 => PermissionStatus::NotDetermined,
        _ => PermissionStatus::NotDetermined,
    }
}

/// Keep a grant observed earlier in this process when a later read is only
/// undetermined. Denied and restricted clear it. Closing the main window does
/// not restart the process, so the second open must not forget a real grant.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn prefer_known_microphone_grant(
    previous_grant: bool,
    status: PermissionStatus,
) -> (bool, PermissionStatus) {
    match status {
        PermissionStatus::Granted => (true, PermissionStatus::Granted),
        PermissionStatus::Denied | PermissionStatus::Restricted => (false, status),
        PermissionStatus::NotDetermined if previous_grant => (true, PermissionStatus::Granted),
        other => (previous_grant, other),
    }
}

/// A granted reading from either source is authoritative. `recordPermission`
/// can stay undetermined after TCC has already authorized the microphone.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn resolve_microphone_permission(
    audio_app: Option<PermissionStatus>,
    capture: PermissionStatus,
) -> PermissionStatus {
    if audio_app == Some(PermissionStatus::Granted) || capture == PermissionStatus::Granted {
        return PermissionStatus::Granted;
    }
    match (audio_app, capture) {
        (Some(PermissionStatus::Denied), _) | (_, PermissionStatus::Denied) => {
            PermissionStatus::Denied
        }
        (Some(PermissionStatus::Restricted), _) | (_, PermissionStatus::Restricted) => {
            PermissionStatus::Restricted
        }
        (Some(status), _) => status,
        (None, status) => status,
    }
}

/// Reset and re-request must not replace an already-granted status.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn microphone_status_after_request(
    before: PermissionStatus,
    requested: PermissionStatus,
    reread: PermissionStatus,
) -> PermissionStatus {
    if before == PermissionStatus::Granted
        || requested == PermissionStatus::Granted
        || reread == PermissionStatus::Granted
    {
        PermissionStatus::Granted
    } else {
        requested
    }
}

/// `tccutil reset Microphone` drops a real grant. Skip it once the status is granted.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn should_reset_microphone_tcc(status: PermissionStatus) -> bool {
    status != PermissionStatus::Granted
}

pub use platform::{
    check_accessibility, check_microphone, request_accessibility, request_microphone,
};

#[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
pub use platform::has_microphone_input_device;

#[cfg(any(target_os = "macos", target_os = "android"))]
pub fn has_microphone_input_device() -> bool {
    // macOS / Android permission checks go through system authorization callbacks and don't
    // rely on this; the recording-start failure path has its own NoInputDevice fallback message.
    false
}

#[cfg(target_os = "windows")]
pub use platform::windows_microphone_access_explicitly_denied;

#[cfg(not(target_os = "windows"))]
pub fn windows_microphone_access_explicitly_denied() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        is_no_device_error, map_capture_authorization, map_record_permission,
        microphone_status_after_request, prefer_known_microphone_grant,
        resolve_microphone_permission, should_reset_microphone_tcc, PermissionStatus,
        RECORD_PERMISSION_DENIED, RECORD_PERMISSION_GRANTED, RECORD_PERMISSION_UNDETERMINED,
    };

    #[test]
    fn no_device_errors_are_recognized() {
        for message in [
            "no default input device",
            "No input device is available",
            "device not found",
            "default device is not connected",
            "unplugged microphone",
        ] {
            assert!(
                is_no_device_error(&message.to_lowercase()),
                "should classify as no-device: {message}"
            );
        }
    }

    #[test]
    fn permission_errors_are_not_no_device() {
        for message in ["access denied", "permission denied", "not authorized"] {
            assert!(
                !is_no_device_error(&message.to_lowercase()),
                "should NOT classify as no-device: {message}"
            );
        }
    }

    #[test]
    fn record_permission_fourcc_values_keep_granted_distinct() {
        assert_eq!(
            map_record_permission(RECORD_PERMISSION_GRANTED),
            PermissionStatus::Granted
        );
        assert_eq!(
            map_record_permission(RECORD_PERMISSION_DENIED),
            PermissionStatus::Denied
        );
        assert_eq!(
            map_record_permission(RECORD_PERMISSION_UNDETERMINED),
            PermissionStatus::NotDetermined
        );
        assert_eq!(map_record_permission(3), PermissionStatus::NotDetermined);
    }

    #[test]
    fn capture_authorization_maps_authorized_to_granted() {
        assert_eq!(map_capture_authorization(3), PermissionStatus::Granted);
        assert_eq!(map_capture_authorization(2), PermissionStatus::Denied);
        assert_eq!(map_capture_authorization(1), PermissionStatus::Restricted);
        assert_eq!(
            map_capture_authorization(0),
            PermissionStatus::NotDetermined
        );
    }

    #[test]
    fn an_undetermined_reread_keeps_a_grant_from_earlier_in_the_process() {
        assert_eq!(
            prefer_known_microphone_grant(true, PermissionStatus::NotDetermined),
            (true, PermissionStatus::Granted)
        );
        assert_eq!(
            prefer_known_microphone_grant(false, PermissionStatus::NotDetermined),
            (false, PermissionStatus::NotDetermined)
        );
        assert_eq!(
            prefer_known_microphone_grant(true, PermissionStatus::Denied),
            (false, PermissionStatus::Denied)
        );
    }

    #[test]
    fn granted_capture_status_is_not_hidden_by_record_permission() {
        assert_eq!(
            resolve_microphone_permission(
                Some(PermissionStatus::NotDetermined),
                PermissionStatus::Granted
            ),
            PermissionStatus::Granted
        );
        assert_eq!(
            resolve_microphone_permission(
                Some(PermissionStatus::Denied),
                PermissionStatus::Granted
            ),
            PermissionStatus::Granted
        );
        assert_eq!(
            resolve_microphone_permission(None, PermissionStatus::Granted),
            PermissionStatus::Granted
        );
        assert_eq!(
            resolve_microphone_permission(
                Some(PermissionStatus::NotDetermined),
                PermissionStatus::Denied
            ),
            PermissionStatus::Denied
        );
    }

    #[test]
    fn re_request_and_reset_keep_an_already_granted_microphone() {
        assert_eq!(
            microphone_status_after_request(
                PermissionStatus::Granted,
                PermissionStatus::Denied,
                PermissionStatus::NotDetermined
            ),
            PermissionStatus::Granted
        );
        assert_eq!(
            microphone_status_after_request(
                PermissionStatus::NotDetermined,
                PermissionStatus::Denied,
                PermissionStatus::Granted
            ),
            PermissionStatus::Granted
        );
        assert_eq!(
            microphone_status_after_request(
                PermissionStatus::NotDetermined,
                PermissionStatus::Denied,
                PermissionStatus::Denied
            ),
            PermissionStatus::Denied
        );
        assert!(!should_reset_microphone_tcc(PermissionStatus::Granted));
        assert!(should_reset_microphone_tcc(PermissionStatus::NotDetermined));
        assert!(should_reset_microphone_tcc(PermissionStatus::Denied));
    }
}
