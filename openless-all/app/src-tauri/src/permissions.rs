//! 系统权限请求 / 检查（macOS / Windows）。
//!
//! Mirrors Swift `Sources/OpenLessHotkey/AccessibilityPermission.swift` +
//! `Sources/OpenLessRecorder/MicrophonePermission.swift`.
//!
//! - macOS Accessibility: `AXIsProcessTrusted` check;
//!   `AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: true})` shows the system
//!   authorization dialog.
//! - macOS Microphone: `AVAudioApplication.shared.recordPermission`, with
//!   `AVCaptureDevice.authorizationStatus` when that reading is not granted.
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

// AVAudioApplicationRecordPermission FourCC raw values (NSInteger).
const RECORD_PERMISSION_GRANTED: i64 = 0x6772_6e74; // 'grnt'
const RECORD_PERMISSION_DENIED: i64 = 0x6465_6e79; // 'deny'
const RECORD_PERMISSION_UNDETERMINED: i64 = 0x756e_6474; // 'undt'

/// Map `AVAudioApplication.recordPermission`. Unknown values stay undetermined;
/// they are not a granted TCC status.
pub(crate) fn map_record_permission(raw: i64) -> PermissionStatus {
    match raw {
        RECORD_PERMISSION_GRANTED => PermissionStatus::Granted,
        RECORD_PERMISSION_DENIED => PermissionStatus::Denied,
        RECORD_PERMISSION_UNDETERMINED => PermissionStatus::NotDetermined,
        _ => PermissionStatus::NotDetermined,
    }
}

/// Map `AVCaptureDevice.authorizationStatus` (`AVAuthorizationStatus`).
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
pub(crate) fn should_reset_microphone_tcc(status: PermissionStatus) -> bool {
    status != PermissionStatus::Granted
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

    // AVAudioApplication lives in the AVFAudio framework (macOS 14+). A non-granted
    // recordPermission is not authoritative: TCC may already allow capture.
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
        // AVFoundation can report NotDetermined when the status is read off the
        // main thread. The UI command runs on the async worker, so hop first.
        let status = if on_main_thread() {
            check_microphone_on_main()
        } else {
            let mut status = super::PermissionStatus::NotDetermined;
            dispatch2::DispatchQueue::main().exec_sync(|| {
                status = check_microphone_on_main();
            });
            status
        };
        remember_microphone_grant(status)
    }

    fn on_main_thread() -> bool {
        unsafe { libc::pthread_main_np() != 0 }
    }

    fn check_microphone_on_main() -> super::PermissionStatus {
        // Mirrors Swift `MicrophonePermission.isGranted()`, but a granted
        // AVCaptureDevice status wins over a stale non-granted recordPermission.
        let audio_app = check_microphone_via_avaudio_application();
        if audio_app == Some(super::PermissionStatus::Granted) {
            return super::PermissionStatus::Granted;
        }
        let capture = check_microphone_via_avcapture_device();
        super::resolve_microphone_permission(audio_app, capture)
    }

    fn remember_microphone_grant(status: super::PermissionStatus) -> super::PermissionStatus {
        use std::sync::atomic::{AtomicBool, Ordering};
        static GRANTED: AtomicBool = AtomicBool::new(false);
        let (granted, status) =
            super::prefer_known_microphone_grant(GRANTED.load(Ordering::Relaxed), status);
        GRANTED.store(granted, Ordering::Relaxed);
        status
    }

    pub fn request_microphone() -> PermissionStatus {
        // Mirrors Swift `MicrophonePermission.request()`, with an 8s fallback.
        // Do not prompt, and do not keep a negative callback, when a source
        // already reports the microphone as granted.
        let before = check_microphone();
        if before == super::PermissionStatus::Granted {
            return super::PermissionStatus::Granted;
        }
        let requested = request_microphone_via_avaudio_application()
            .unwrap_or_else(request_microphone_via_avcapture_device);
        let reread = check_microphone();
        super::microphone_status_after_request(before, requested, reread)
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
        let mapped = super::map_record_permission(perm);
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
        let mapped = super::map_capture_authorization(status);
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

pub use platform::{request_accessibility, request_microphone};
#[cfg(not(all(target_os = "windows", feature = "issue-1338-diagnostics")))]
pub use platform::{check_accessibility, check_microphone};

#[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics"))]
fn observe_native_permission(kind: crate::issue1338_debug::Kind, check: impl FnOnce() -> PermissionStatus) -> PermissionStatus {
    use crate::issue1338_debug::{Boundary, Event, Outcome, Probe, SyncContext};
    let boundary = Boundary::enter(Probe::P5, kind);
    let _context = SyncContext::enter(boundary.operation);
    let status = check();
    let outcome = match status {
        PermissionStatus::Granted => Outcome::Granted,
        PermissionStatus::Denied => Outcome::Denied,
        PermissionStatus::NotDetermined => Outcome::NotDetermined,
        PermissionStatus::Restricted => Outcome::Restricted,
        PermissionStatus::NotApplicable => Outcome::NotApplicable,
        PermissionStatus::NoDevice => Outcome::NoDevice,
    };
    boundary.record(Event::Returned, outcome);
    status
}
#[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics"))]
pub fn check_accessibility() -> PermissionStatus {
    observe_native_permission(crate::issue1338_debug::Kind::Accessibility, platform::check_accessibility)
}
#[cfg(all(target_os = "windows", feature = "issue-1338-diagnostics"))]
pub fn check_microphone() -> PermissionStatus {
    observe_native_permission(crate::issue1338_debug::Kind::Microphone, platform::check_microphone)
}

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
