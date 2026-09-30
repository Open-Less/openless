//! 系统权限请求 / 检查（macOS / Windows）。
//!
//! Mirrors Swift `Sources/OpenLessHotkey/AccessibilityPermission.swift` +
//! `Sources/OpenLessRecorder/MicrophonePermission.swift`.
//!
//! - macOS Accessibility: `AXIsProcessTrusted` check;
//!   `AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: true})` shows the system
//!   authorization dialog.
//! - macOS Microphone: `AVAudioApplication.shared.recordPermission` + requestRecordPermission.
//! - Windows: cpal needs no Accessibility-equivalent permission; the mic shows the one-time
//!   system prompt on Win10+ at first use. With no available input device, return `NoDevice`
//!   (distinct from permission `Denied`) so "no microphone plugged in" is not misreported as
//!   "microphone not authorized". See issue #779.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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
pub(crate) fn is_no_device_error(lower: &str) -> bool {
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

// ─────────────────────────── macOS ───────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::PermissionStatus;
    use std::ffi::c_void;
    use std::sync::mpsc;
    use std::time::Duration;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        static kCFBooleanTrue: *const c_void;
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        // Take the NSString static symbol exported by AVFoundation directly; don't build an
        // NSString from a Rust string.
        static AVMediaTypeAudio: *const c_void;
    }

    // AVAudioApplication lives in the AVFAudio framework (macOS 14+). The Swift original
    // MicrophonePermission.swift takes this path; it is the single source of truth for
    // pre-recording permission checks.
    #[link(name = "AVFAudio", kind = "framework")]
    extern "C" {}

    pub fn check_accessibility() -> PermissionStatus {
        unsafe {
            if AXIsProcessTrusted() {
                PermissionStatus::Granted
            } else {
                PermissionStatus::Denied
            }
        }
    }

    /// Show the Accessibility system authorization dialog (only when not yet granted). Returns the current status.
    pub fn request_accessibility() -> PermissionStatus {
        unsafe {
            let key = kAXTrustedCheckOptionPrompt;
            let value = kCFBooleanTrue;
            let keys: [*const c_void; 1] = [key];
            let values: [*const c_void; 1] = [value];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
                &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
            );
            let trusted = AXIsProcessTrustedWithOptions(dict);
            CFRelease(dict);
            if trusted {
                PermissionStatus::Granted
            } else {
                PermissionStatus::Denied
            }
        }
    }

    pub fn check_microphone() -> PermissionStatus {
        // Mirrors Swift `MicrophonePermission.isGranted()`.
        if let Some(status) = check_microphone_via_avaudio_application() {
            return status;
        }
        check_microphone_via_avcapture_device()
    }

    pub fn request_microphone() -> PermissionStatus {
        // Mirrors Swift `MicrophonePermission.request()`, with an 8s fallback.
        if let Some(status) = request_microphone_via_avaudio_application() {
            return status;
        }
        request_microphone_via_avcapture_device()
    }

    fn check_microphone_via_avaudio_application() -> Option<PermissionStatus> {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};

        // Class missing = running on older macOS (< 14); fall back to the capture device path
        let cls = AnyClass::get("AVAudioApplication")?;
        let shared: *mut AnyObject = unsafe { msg_send![cls, sharedInstance] };
        if shared.is_null() {
            log::warn!("[mic] AVAudioApplication sharedInstance returned null");
            return None;
        }
        // AVAudioApplicationRecordPermission is an NS_ENUM(NSInteger, ...) FourCC:
        //   'grnt' = 0x67726e74 = 1735552628
        //   'deny' = 0x64656e79 = 1684368761
        //   'undt' = 0x756e6474 = 1970168948
        let perm: i64 = unsafe { msg_send![shared, recordPermission] };
        let mapped = match perm {
            0x6772_6e74 => PermissionStatus::Granted,
            0x6465_6e79 => PermissionStatus::Denied,
            0x756e_6474 => PermissionStatus::NotDetermined,
            _ => PermissionStatus::NotDetermined,
        };
        log::info!(
            "[mic] AVAudioApplication.recordPermission raw=0x{:x} ({}) → {:?}",
            perm,
            perm,
            mapped
        );
        Some(mapped)
    }

    fn request_microphone_via_avaudio_application() -> Option<PermissionStatus> {
        use block2::RcBlock;
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, Bool};

        let cls = AnyClass::get("AVAudioApplication")?;
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });

        log::info!("[mic] requesting via AVAudioApplication.requestRecordPermission");
        unsafe {
            let _: () = msg_send![
                cls,
                requestRecordPermissionWithCompletionHandler: &*block
            ];
        }

        let mapped = match rx.recv_timeout(Duration::from_secs(8)) {
            Ok(true) => PermissionStatus::Granted,
            Ok(false) => PermissionStatus::Denied,
            Err(err) => {
                log::warn!("[mic] AVAudioApplication request timeout/error: {err}");
                check_microphone_via_avaudio_application()
                    .unwrap_or(PermissionStatus::NotDetermined)
            }
        };
        log::info!(
            "[mic] AVAudioApplication.requestRecordPermission → {:?}",
            mapped
        );
        Some(mapped)
    }

    fn check_microphone_via_avcapture_device() -> PermissionStatus {
        // [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio]
        use objc2::msg_send;
        use objc2::runtime::AnyClass;

        let cls = match AnyClass::get("AVCaptureDevice") {
            Some(c) => c,
            None => return PermissionStatus::NotDetermined,
        };
        let status: i64 =
            unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] };
        let mapped = match status {
            3 => PermissionStatus::Granted,
            2 => PermissionStatus::Denied,
            1 => PermissionStatus::Restricted,
            0 => PermissionStatus::NotDetermined,
            _ => PermissionStatus::NotDetermined,
        };
        log::info!(
            "[mic] AVCaptureDevice.authStatus raw={} → {:?}",
            status,
            mapped
        );
        mapped
    }

    fn request_microphone_via_avcapture_device() -> PermissionStatus {
        use block2::RcBlock;
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, Bool};

        let cls = match AnyClass::get("AVCaptureDevice") {
            Some(c) => c,
            None => return PermissionStatus::NotDetermined,
        };
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });

        log::info!("[mic] requesting via AVCaptureDevice.requestAccessForMediaType");
        unsafe {
            let _: () = msg_send![
                cls,
                requestAccessForMediaType: AVMediaTypeAudio
                completionHandler: &*block
            ];
        }

        let mapped = match rx.recv_timeout(Duration::from_secs(8)) {
            Ok(true) => PermissionStatus::Granted,
            Ok(false) => PermissionStatus::Denied,
            Err(err) => {
                log::warn!("[mic] AVCaptureDevice request timeout/error: {err}");
                check_microphone_via_avcapture_device()
            }
        };
        log::info!("[mic] AVCaptureDevice.requestAccess → {:?}", mapped);
        mapped
    }
}

// ─────────────────────────── Android ───────────────────────────

#[cfg(target_os = "android")]
mod platform {
    use super::PermissionStatus;

    pub fn check_accessibility() -> PermissionStatus {
        PermissionStatus::NotApplicable
    }

    pub fn request_accessibility() -> PermissionStatus {
        PermissionStatus::NotApplicable
    }

    pub fn check_microphone() -> PermissionStatus {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::check_self_permission(
                env,
                context,
                "android.permission.RECORD_AUDIO",
            )
        }) {
            Ok(true) => PermissionStatus::Granted,
            Ok(false) => PermissionStatus::Denied,
            Err(error) => {
                log::warn!("[mic] Android RECORD_AUDIO permission check failed: {error}");
                PermissionStatus::NotDetermined
            }
        }
    }

    pub fn request_microphone() -> PermissionStatus {
        match crate::android::jni::android::with_android_env(|env, context| {
            crate::android::jni::android::request_record_audio_permission(env, context)
        }) {
            Ok(true) => PermissionStatus::Granted,
            Ok(false) => PermissionStatus::NotDetermined,
            Err(error) => {
                log::warn!("[mic] Android RECORD_AUDIO permission request failed: {error}");
                check_microphone()
            }
        }
    }
}

// ─────────────────────────────── Windows / 其他 ───────────────────────────────

#[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
mod platform {
    use super::PermissionStatus;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};
    use std::time::Duration;
    #[cfg(target_os = "windows")]
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    #[cfg(target_os = "windows")]
    use winreg::RegKey;

    /// Windows 不存在 macOS 那种 Accessibility 概念。
    pub fn check_accessibility() -> PermissionStatus {
        PermissionStatus::NotApplicable
    }

    pub fn request_accessibility() -> PermissionStatus {
        PermissionStatus::NotApplicable
    }

    /// Windows 的麦克风权限走系统设置 → 隐私 → 麦克风；
    /// 这里用 cpal 建立一次短生命周期输入流，避免只查设备格式时误报已授权。
    pub fn check_microphone() -> PermissionStatus {
        {
            if windows_microphone_registry_denied() {
                log::warn!("[mic] Windows microphone privacy registry is denied");
                return PermissionStatus::Denied;
            }

            let host = cpal::default_host();
            let Some(device) = host.default_input_device() else {
                log::warn!("[mic] no default input device");
                return PermissionStatus::NoDevice;
            };
            let supported = match device.default_input_config() {
                Ok(config) => config,
                Err(err) => return classify_audio_probe_error(err.to_string()),
            };
            let sample_format = supported.sample_format();
            let config: StreamConfig = supported.config();
            match probe_input_stream(&device, &config, sample_format) {
                Ok(()) => PermissionStatus::Granted,
                Err(message) => classify_audio_probe_error(message),
            }
        }
    }

    pub fn request_microphone() -> PermissionStatus {
        check_microphone()
    }

    /// Lightweight check for an input device: enumerate devices only, no input stream,
    /// letting dictation startup quickly tell "no mic plugged in" from "device present but denied".
    pub fn has_microphone_input_device() -> bool {
        cpal::default_host().default_input_device().is_some()
    }

    pub fn windows_microphone_access_explicitly_denied() -> bool {
        #[cfg(target_os = "windows")]
        {
            windows_microphone_registry_denied()
        }

        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    fn classify_audio_probe_error(message: String) -> PermissionStatus {
        let lower = message.to_lowercase();
        log::warn!("[mic] input probe failed: {message}");
        if super::is_no_device_error(&lower) {
            PermissionStatus::NoDevice
        } else if lower.contains("denied")
            || lower.contains("permission")
            || lower.contains("authoriz")
            || lower.contains("access")
        {
            PermissionStatus::Denied
        } else {
            PermissionStatus::NotDetermined
        }
    }

    fn probe_input_stream(
        device: &cpal::Device,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<(), String> {
        let err_cb = |err| log::warn!("[mic] probe stream error: {err}");

        macro_rules! build_probe {
            ($t:ty) => {
                device
                    .build_input_stream::<$t, _, _>(
                        config,
                        move |_data: &[$t], _info| {},
                        err_cb,
                        None,
                    )
                    .map_err(|e| e.to_string())
            };
        }

        let stream = match sample_format {
            SampleFormat::F32 => build_probe!(f32),
            SampleFormat::I16 => build_probe!(i16),
            SampleFormat::U16 => build_probe!(u16),
            SampleFormat::I32 => build_probe!(i32),
            SampleFormat::I8 => build_probe!(i8),
            SampleFormat::U8 => build_probe!(u8),
            other => Err(format!("unsupported sample format: {other:?}")),
        }?;

        stream.play().map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(120));
        drop(stream);
        Ok(())
    }

    fn windows_microphone_registry_denied() -> bool {
        candidate_microphone_registry_paths()
            .into_iter()
            .any(|path| registry_value_is_deny(&path))
    }

    fn candidate_microphone_registry_paths() -> Vec<String> {
        let mut paths = vec![
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone".to_string(),
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged".to_string(),
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone".to_string(),
        ];

        if let Ok(exe) = std::env::current_exe() {
            if let Some(encoded) = exe.to_str().map(|path| path.replace('\\', "#")) {
                paths.push(format!(
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\{encoded}"
                ));
            }
        }

        paths
    }

    fn registry_value_is_deny(path: &str) -> bool {
        #[cfg(target_os = "windows")]
        {
            let Some((root, subkey)) = path.split_once('\\') else {
                return false;
            };

            let hive = match root {
                "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
                "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
                _ => return false,
            };

            match hive.open_subkey(subkey) {
                Ok(key) => match key.get_value::<String, _>("Value") {
                    Ok(value) => value.eq_ignore_ascii_case("Deny"),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = path;
            false
        }
    }
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
    use super::is_no_device_error;

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
}
