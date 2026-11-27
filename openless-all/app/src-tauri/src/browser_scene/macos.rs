//! Read-only enrichment of an already-bound native target.
//!
//! This module never captures an application, window or focused input. The
//! existing target owner supplies its retained AX elements and verifier.
//! No tree search, AXValue read, activation, AppleScript or input event is used.

use core_foundation::{
    base::{CFType, TCFType},
    string::CFString,
    url::CFURL,
};
use openless_core::{
    browser_scene::{
        classify_input, page_location, BrowserInputKind, ControlFacts, SceneAuthorization,
    },
    input_scene::BrowserFamily,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};

#[link(name = "ApplicationServices", kind = "framework")]
#[allow(clashing_extern_declarations)]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementGetPid(element: *mut c_void, pid: *mut i32) -> i32;
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

fn text(element: &CFType, name: &str) -> Option<String> {
    attr(element, name)?
        .downcast::<CFString>()
        .map(|value| value.to_string())
}

fn owned_element(element: &CFType, expected_pid: i32) -> bool {
    if element.type_of() != unsafe { AXUIElementGetTypeID() } {
        return false;
    }
    let mut pid = 0;
    unsafe {
        AXUIElementSetMessagingTimeout(element.as_CFTypeRef().cast_mut(), 0.065) == 0
            && AXUIElementGetPid(element.as_CFTypeRef().cast_mut(), &mut pid) == 0
            && pid == expected_pid
    }
}

fn raw_location(page: &CFType) -> Option<String> {
    let value = attr(page, "AXURL")?;
    value
        .downcast::<CFURL>()
        .map(|url| url.get_string().to_string())
        .or_else(|| value.downcast::<CFString>().map(|value| value.to_string()))
}

/// No Debug or serialization: native handles and raw URL fingerprints stay local.
pub(crate) struct Enrichment {
    pub(crate) input_kind: BrowserInputKind,
    pub(crate) location: Option<String>,
    page: Option<CFType>,
    raw_location_digest: Option<[u8; 32]>,
}

impl Enrichment {
    /// This is additional page validation, not a substitute for target/focus
    /// validation. Same URL in a different tab is rejected by the host binding
    /// and AXWebArea identity; same tab navigating to a new URL is rejected here.
    pub(crate) fn page_is_current(&self, other: &Self) -> bool {
        self.input_kind == other.input_kind
            && self.page == other.page
            && self.raw_location_digest == other.raw_location_digest
    }
}

/// Call on the native target's existing blocking worker. `verify` must include
/// its focus-generation check, permission epoch and secure-input gate.
pub(crate) fn enrich(
    expected_pid: i32,
    browser: BrowserFamily,
    window: &CFType,
    input: &CFType,
    grant: SceneAuthorization,
    verify: impl Fn() -> bool,
) -> Option<Enrichment> {
    // A ChatGPT conversation can itself be backed by a web area. That does
    // not prove its embedded browser is the target. Until a public structure
    // uniquely identifies that browser, leave its browser context unavailable.
    if browser == BrowserFamily::ChatGptEmbedded
        || expected_pid <= 0
        || !grant.app_metadata
        || !grant.browser_metadata
        || !verify()
        || !unsafe { AXIsProcessTrusted() }
        || !owned_element(window, expected_pid)
        || !owned_element(input, expected_pid)
    {
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let role = text(input, "AXRole")?;
    let subrole = text(input, "AXSubrole").unwrap_or_default();
    if role == "AXSecureTextField" || subrole == "AXSecureTextField" {
        return None;
    }
    // A focusable non-text control is not an input scenario. Reading only the
    // role/subrole before deciding avoids exposing protected values.
    let editable = matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox");
    let mut current = input.clone();
    let mut page = None;
    let mut in_toolbar = false;
    let mut reached_window = false;
    for _ in 0..32 {
        if Instant::now() > deadline || !verify() || !owned_element(&current, expected_pid) {
            return None;
        }
        let current_role = text(&current, "AXRole")?;
        let current_subrole = text(&current, "AXSubrole").unwrap_or_default();
        if current_role == "AXSecureTextField" || current_subrole == "AXSecureTextField" {
            return None;
        }
        if current == *window {
            reached_window = true;
            break;
        }
        if current_role == "AXWebArea" {
            // Do not silently select one of nested or ambiguous web areas.
            if page.is_some() {
                return None;
            }
            page = Some(current.clone());
        }
        in_toolbar |= current_role == "AXToolbar";
        let parent = attr(&current, "AXParent")?;
        if parent == current {
            return None;
        }
        current = parent;
    }
    if !reached_window || !verify() {
        return None;
    }
    // Labels are data read only after the entire original ancestry is verified.
    // Never read a window title, full webpage text, AXValue or sibling controls.
    let labels: Vec<_> = ["AXTitle", "AXDescription", "AXPlaceholderValue"]
        .into_iter()
        .filter_map(|name| text(input, name))
        .filter(|label| !label.is_empty())
        .collect();
    let label = labels
        .iter()
        .find(|label| {
            if page.is_some() {
                label.trim().eq_ignore_ascii_case("Ask anything")
            } else {
                matches!(label.as_str(), "Address and search bar" | "地址和搜索栏")
            }
        })
        .or_else(|| labels.first())
        .map(String::as_str)
        .unwrap_or_default();
    let input_kind = classify_input(
        browser,
        &ControlFacts {
            role: &role,
            subrole: &subrole,
            ancestry_verified: true,
            in_web_area: page.is_some(),
            in_toolbar,
            editable,
            protected: false,
            label,
        },
    );
    let raw_url = match &page {
        Some(page) => Some(raw_location(page)?),
        None => None,
    };
    let location = match &raw_url {
        Some(raw) => Some(page_location(raw)?),
        None => None,
    };
    if !verify() {
        return None;
    }
    Some(Enrichment {
        input_kind,
        location,
        page,
        raw_location_digest: raw_url.map(|raw| Sha256::digest(raw.as_bytes()).into()),
    })
}
