//! Read-only enrichment of an already-bound native target.
//!
//! This module never captures an application, window or focused input. The
//! existing target owner supplies its retained AX elements and verifier.
//! Native chrome structure is inspected without descending into webpage text
//! or tab contents. No AXValue read, activation or input event is used.

use core_foundation::{
    array::CFArray,
    base::{CFEqual, CFGetTypeID, CFType, TCFType},
    runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource},
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
    sync::{atomic::AtomicU64, Arc},
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
    fn AXObserverCreate(
        pid: i32,
        callback: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *mut c_void),
        observer: *mut *mut c_void,
    ) -> i32;
    fn AXObserverAddNotification(
        observer: *mut c_void,
        element: *mut c_void,
        name: *const c_void,
        context: *mut c_void,
    ) -> i32;
    fn AXObserverGetRunLoopSource(observer: *mut c_void) -> *mut c_void;
}

/// Required notifications are deliberately all-or-nothing. Providers which
/// cannot observe page replacement/navigation remain unavailable for sharing.
pub(crate) struct PageWatch {
    source: CFRunLoopSource,
    _observer: CFType,
    _epoch: Box<Arc<AtomicU64>>,
    _certificate: super::observation::Certificate,
}

unsafe extern "C" fn page_changed(
    _: *mut c_void,
    _: *mut c_void,
    _: *const c_void,
    context: *mut c_void,
) {
    if let Some(epoch) = context.cast::<Arc<AtomicU64>>().as_ref() {
        super::observation::transition(epoch);
    }
}

impl PageWatch {
    pub(crate) fn new(pid: i32, scene: &Enrichment, epoch: Arc<AtomicU64>) -> Option<Self> {
        unsafe {
            let page = scene.page.as_ref()?;
            let tabs = scene.tab_group.as_ref()?;
            let mut observer = ptr::null_mut();
            if AXObserverCreate(pid, page_changed, &mut observer) != 0 || observer.is_null() {
                return None;
            }
            let observer = CFType::wrap_under_create_rule(observer.cast());
            let mut epoch = Box::new(epoch);
            let certificate = super::observation::establish(|subject, name| {
                let element = match subject {
                    super::observation::Subject::Page => page,
                    super::observation::Subject::TabGroup => tabs,
                };
                let name = CFString::new(name);
                AXObserverAddNotification(
                    observer.as_CFTypeRef().cast_mut(),
                    element.as_CFTypeRef().cast_mut(),
                    name.as_CFTypeRef().cast(),
                    ptr::from_mut(&mut *epoch).cast(),
                ) == 0
            })?;
            let raw_source = AXObserverGetRunLoopSource(observer.as_CFTypeRef().cast_mut());
            if raw_source.is_null() {
                return None;
            }
            let source = CFRunLoopSource::wrap_under_get_rule(raw_source.cast());
            CFRunLoop::get_current().add_source(&source, kCFRunLoopDefaultMode);
            Some(Self {
                source,
                _observer: observer,
                _epoch: epoch,
                _certificate: certificate,
            })
        }
    }
}

impl Drop for PageWatch {
    fn drop(&mut self) {
        unsafe { CFRunLoop::get_current().remove_source(&self.source, kCFRunLoopDefaultMode) }
    }
}

fn attr(element: *mut c_void, name: &str) -> Option<CFType> {
    let name = CFString::new(name);
    let mut value = ptr::null();
    let status =
        unsafe { AXUIElementCopyAttributeValue(element, name.as_CFTypeRef().cast(), &mut value) };
    (status == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}

fn text(element: *mut c_void, name: &str) -> Option<String> {
    attr(element, name)?
        .downcast::<CFString>()
        .map(|value| value.to_string())
}

fn owned_element(element: *mut c_void, expected_pid: i32) -> bool {
    if element.is_null() || unsafe { CFGetTypeID(element.cast()) != AXUIElementGetTypeID() } {
        return false;
    }
    let mut pid = 0;
    unsafe {
        AXUIElementSetMessagingTimeout(element, 0.065) == 0
            && AXUIElementGetPid(element, &mut pid) == 0
            && pid == expected_pid
    }
}

fn raw_location(page: &CFType) -> Option<String> {
    let value = attr(page.as_CFTypeRef().cast_mut(), "AXURL")?;
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
    tab_group: Option<CFType>,
    selected_tab: Option<CFType>,
}

impl Enrichment {
    /// Check only the original page URL and the native group's one selected-tab
    /// identity. No other tab's labels/URL, input values or window tree is read.
    pub(crate) fn original_page_is_current(&self) -> bool {
        let tab_current = self
            .tab_group
            .as_ref()
            .zip(self.selected_tab.as_ref())
            .is_some_and(|(group, tab)| selected_tab(group).as_ref() == Some(tab));
        tab_current
            && self
                .page
                .as_ref()
                .zip(self.raw_location_digest)
                .is_some_and(|(page, digest)| {
                    raw_location(page).is_some_and(|raw| {
                        <[u8; 32]>::from(Sha256::digest(raw.as_bytes())) == digest
                    })
                })
    }
    /// This is additional page validation, not a substitute for target/focus
    /// validation. Same URL in a different tab is rejected by the host binding
    /// and AXWebArea identity; same tab navigating to a new URL is rejected here.
    pub(crate) fn page_is_current(&self, other: &Self) -> bool {
        self.input_kind == other.input_kind
            && self.page == other.page
            && self.raw_location_digest == other.raw_location_digest
            && self.tab_group == other.tab_group
            && self.selected_tab == other.selected_tab
    }
}

fn selected_tab(group: &CFType) -> Option<CFType> {
    let children =
        attr(group.as_CFTypeRef().cast_mut(), "AXSelectedChildren")?.downcast_into::<CFArray>()?;
    if children.len() != 1 {
        return None;
    }
    let child = *children.get(0)?;
    if child.is_null() || unsafe { CFGetTypeID(child) != AXUIElementGetTypeID() } {
        return None;
    }
    Some(unsafe { CFType::wrap_under_get_rule(child) })
}

/// Inspect native chrome structure only. Never descend into a WebArea or a
/// tab group's children: read only its one AXSelectedChildren identity.
/// More than one native tab group or page makes the association ambiguous.
fn tab_witness(
    window: *mut c_void,
    page: &CFType,
    pid: i32,
    deadline: Instant,
    current: &impl Fn() -> bool,
) -> Option<(CFType, CFType)> {
    let roots = attr(window, "AXChildren")?.downcast_into::<CFArray>()?;
    if roots.len() > 64 {
        return None;
    }
    let mut pending = Vec::new();
    for child in roots.iter() {
        if !(*child).is_null() {
            pending.push((unsafe { CFType::wrap_under_get_rule(*child) }, 0));
        }
    }
    let mut visited = 0;
    let mut pages = 0;
    let mut tabs = None;
    while let Some((element, depth)) = pending.pop() {
        visited += 1;
        let raw = element.as_CFTypeRef().cast_mut();
        if visited > 160
            || depth > 12
            || Instant::now() > deadline
            || !current()
            || !owned_element(raw, pid)
        {
            return None;
        }
        match text(raw, "AXRole")?.as_str() {
            "AXWebArea" => {
                if element != *page {
                    return None;
                }
                pages += 1;
                continue;
            }
            "AXTabGroup" => {
                if tabs.is_some() {
                    return None;
                }
                let tab = selected_tab(&element)?;
                if !owned_element(tab.as_CFTypeRef().cast_mut(), pid) {
                    return None;
                }
                tabs = Some((element, tab));
                continue;
            }
            _ => {}
        }
        if let Some(children) = attr(raw, "AXChildren") {
            let children = children.downcast_into::<CFArray>()?;
            if children.len() > 64 {
                return None;
            }
            for child in children.iter() {
                if !(*child).is_null() {
                    pending.push((unsafe { CFType::wrap_under_get_rule(*child) }, depth + 1));
                }
            }
        }
    }
    (pages == 1).then_some(tabs).flatten()
}

/// Call on the native target's existing blocking worker. `verify` must include
/// its focus-generation check, permission epoch and secure-input gate.
/// SAFETY: input/window must be the callback's borrowed P4 objects. Neither
/// reference is retained, released or returned. Only CopyAttributeValue-owned
/// ancestor objects may be retained as page verification evidence.
pub(crate) unsafe fn enrich(
    expected_pid: i32,
    browser: BrowserFamily,
    window: *mut c_void,
    input: *mut c_void,
    grant: SceneAuthorization,
    include_location: bool,
    verify: impl Fn() -> bool,
) -> Option<Enrichment> {
    // A ChatGPT conversation can itself be backed by a web area. That does
    // not prove its embedded browser is the target. Until a public structure
    // uniquely identifies that browser, leave its browser context unavailable.
    if browser == BrowserFamily::ChatGptEmbedded
        || expected_pid <= 0
        || !grant.app_metadata
        || (include_location && !grant.browser_metadata)
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
    let mut current = input;
    let mut current_owner: Option<CFType> = None;
    let mut page = None;
    let mut in_toolbar = false;
    let mut reached_window = false;
    for _ in 0..32 {
        if Instant::now() > deadline || !verify() || !owned_element(current, expected_pid) {
            return None;
        }
        let current_role = text(current, "AXRole")?;
        let current_subrole = text(current, "AXSubrole").unwrap_or_default();
        if current_role == "AXSecureTextField" || current_subrole == "AXSecureTextField" {
            return None;
        }
        if CFEqual(current.cast(), window.cast()) != 0 {
            reached_window = true;
            break;
        }
        if current_role == "AXWebArea" {
            // Do not silently select one of nested or ambiguous web areas.
            if page.is_some() {
                return None;
            }
            page = Some(current_owner.as_ref()?.clone());
        }
        in_toolbar |= current_role == "AXToolbar";
        let parent = attr(current, "AXParent")?;
        if CFEqual(parent.as_CFTypeRef(), current.cast()) != 0 {
            return None;
        }
        current = parent.as_CFTypeRef().cast_mut();
        current_owner = Some(parent);
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
    if matches!(
        input_kind,
        BrowserInputKind::Unknown | BrowserInputKind::Protected
    ) {
        return None;
    }
    let tab = if include_location {
        Some(tab_witness(
            window,
            page.as_ref()?,
            expected_pid,
            deadline,
            &verify,
        )?)
    } else {
        None
    };
    let raw_url = match page.as_ref().filter(|_| include_location) {
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
        tab_group: tab.as_ref().map(|(group, _)| group.clone()),
        selected_tab: tab.map(|(_, tab)| tab),
    })
}
