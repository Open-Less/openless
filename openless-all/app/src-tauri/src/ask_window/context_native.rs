//! Native read-only device facts. No shell, automation script or input events.

use openless_core::ask::AskCancellation;

pub(super) fn architecture() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        let mut arm64: i32 = 0;
        let mut size = std::mem::size_of_val(&arm64);
        if unsafe {
            libc::sysctlbyname(
                c"hw.optional.arm64".as_ptr(),
                (&mut arm64 as *mut i32).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } == 0
            && arm64 == 1
        {
            return "aarch64";
        }
    }
    std::env::consts::ARCH
}

#[cfg(target_os = "macos")]
pub(super) fn system_version() -> Option<String> {
    let mut value = [0u8; 128];
    let mut size = value.len();
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            value.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && size > 1 && size <= value.len())
        .then(|| String::from_utf8_lossy(&value[..size - 1]).into_owned())
}

#[cfg(target_os = "windows")]
pub(super) fn system_version() -> Option<String> {
    #[repr(C)]
    struct Version {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(version: *mut Version) -> i32;
    }
    let mut version = Version {
        size: std::mem::size_of::<Version>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    (unsafe { RtlGetVersion(&mut version) } == 0)
        .then(|| format!("{}.{}.{}", version.major, version.minor, version.build))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn system_version() -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
pub(super) fn browser_tab(
    _: u32,
    _: &str,
    _: &AskCancellation,
) -> Result<serde_json::Value, &'static str> {
    Err("browserTabUnsupported")
}

#[cfg(target_os = "macos")]
pub(super) fn browser_tab(
    pid: u32,
    expected_bundle: &str,
    cancellation: &AskCancellation,
) -> Result<serde_json::Value, &'static str> {
    macos::read_tab(pid, expected_bundle, cancellation)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use core_foundation::{
        array::CFArray,
        base::{CFType, TCFType},
        string::CFString,
        url::CFURL,
    };
    use objc2::{
        msg_send,
        rc::autoreleasepool,
        runtime::{AnyClass, AnyObject},
    };
    use std::{
        ffi::c_void,
        ptr,
        time::{Duration, Instant},
    };

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    #[link(name = "ApplicationServices", kind = "framework")]
    // This standalone adapter uses raw pointers for opaque AX objects. The document
    // adapter declares the same C ABI with its private opaque struct.
    #[allow(clashing_extern_declarations)]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
        fn AXUIElementSetMessagingTimeout(element: *mut c_void, timeout: f32) -> i32;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            name: *const c_void,
            out: *mut *const c_void,
        ) -> i32;
    }

    fn attr(element: &CFType, name: &str) -> Option<CFType> {
        let name = CFString::new(name);
        let mut value = ptr::null();
        let status = unsafe {
            AXUIElementCopyAttributeValue(
                element.as_CFTypeRef().cast_mut(),
                name.as_CFTypeRef().cast(),
                &mut value,
            )
        };
        (status == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
    }

    fn string(value: CFType) -> Option<String> {
        if let Some(value) = value.downcast::<CFString>() {
            return Some(value.to_string());
        }
        value
            .downcast::<CFURL>()
            .map(|url| url.get_string().to_string())
    }

    fn http_url(value: CFType) -> Option<String> {
        let value = string(value)?;
        let url = url::Url::parse(&value).ok()?;
        (matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none())
        .then(|| url.to_string())
    }

    pub(super) fn read_tab(
        pid: u32,
        expected_bundle: &str,
        cancellation: &AskCancellation,
    ) -> Result<serde_json::Value, &'static str> {
        if cancellation.is_cancelled() {
            return Err("askCancelled");
        }
        if !unsafe { AXIsProcessTrusted() } {
            return Err("accessibilityPermissionRequired");
        }
        if !matches!(
            expected_bundle,
            "com.apple.Safari"
                | "com.google.Chrome"
                | "com.google.Chrome.canary"
                | "org.chromium.Chromium"
                | "com.microsoft.edgemac"
                | "com.brave.Browser"
                | "com.vivaldi.Vivaldi"
                | "company.thebrowser.Browser"
                | "com.kagi.kagimacOS"
                | "com.citrolabs.ego.lite"
        ) {
            return Err("browserTabUnsupported");
        }
        // Bind to the original app identity; never inspect whichever app Ask currently fronts.
        let identity_matches = autoreleasepool(|_| unsafe {
            let class = AnyClass::get("NSRunningApplication")?;
            let app: *mut AnyObject =
                msg_send![class, runningApplicationWithProcessIdentifier: pid as i32];
            if app.is_null() {
                return None;
            }
            let bundle: *mut AnyObject = msg_send![app, bundleIdentifier];
            if bundle.is_null() {
                return None;
            }
            let bytes: *const std::ffi::c_char = msg_send![bundle, UTF8String];
            (!bytes.is_null())
                .then(|| std::ffi::CStr::from_ptr(bytes).to_string_lossy() == expected_bundle)
        }) == Some(true);
        if !identity_matches {
            return Err("browserSourceChanged");
        }
        let application = unsafe { AXUIElementCreateApplication(pid as i32) };
        if application.is_null() {
            return Err("browserSourceUnavailable");
        }
        let application = unsafe { CFType::wrap_under_create_rule(application.cast_const()) };
        unsafe {
            AXUIElementSetMessagingTimeout(application.as_CFTypeRef().cast_mut(), 0.08);
        }
        let window = attr(&application, "AXFocusedWindow").ok_or("browserWindowUnavailable")?;
        let title = attr(&window, "AXTitle").and_then(string);
        // Embedded browsers can expose more than one web area in one native window.
        // Prefer the focused page's ancestry and refuse ambiguous fallback candidates.
        let mut focused = attr(&application, "AXFocusedUIElement");
        for _ in 0..16 {
            let Some(element) = focused else {
                break;
            };
            if cancellation.is_cancelled() {
                return Err("askCancelled");
            }
            if attr(&element, "AXRole").and_then(string).as_deref() == Some("AXWebArea") {
                if let Some(url) = attr(&element, "AXURL").and_then(http_url) {
                    let title = attr(&element, "AXTitle")
                        .and_then(string)
                        .filter(|title| !title.is_empty())
                        .or_else(|| title.clone());
                    return Ok(serde_json::json!({"url":url,"title":title}));
                }
            }
            focused = attr(&element, "AXParent");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut pending = vec![(window, 0)];
        let mut visited = 0;
        let mut candidate = None;
        while let Some((element, depth)) = pending.pop() {
            visited += 1;
            if visited > 192 || Instant::now() > deadline || cancellation.is_cancelled() {
                return Err("browserReadInterrupted");
            }
            let role = attr(&element, "AXRole").and_then(string);
            if role.as_deref() == Some("AXWebArea") {
                if let Some(url) = attr(&element, "AXURL").and_then(http_url) {
                    let title = attr(&element, "AXTitle")
                        .and_then(string)
                        .filter(|title| !title.is_empty())
                        .or_else(|| title.clone());
                    if candidate.is_some() {
                        return Err("browserTabAmbiguous");
                    }
                    candidate = Some(serde_json::json!({"url":url,"title":title}));
                }
                // Page text is read through the public-page tool; this port only reads tab metadata.
                continue;
            }
            if depth < 12 {
                if let Some(children) =
                    attr(&element, "AXChildren").and_then(|value| value.downcast_into::<CFArray>())
                {
                    pending.extend(children.iter().take(64).filter_map(|child| {
                        (!(*child).is_null())
                            .then(|| (unsafe { CFType::wrap_under_get_rule(*child) }, depth + 1))
                    }));
                }
            }
        }
        candidate.ok_or("browserTabUnavailable")
    }
}
