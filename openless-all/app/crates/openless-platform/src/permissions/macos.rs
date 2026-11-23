//! macOS 权限探测：TCC（辅助功能 / 麦克风）。

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
