//! macOS Accessibility read implementation.
//!
//! Hand-written FFI, same lineage as `lib.rs::macos_capsule_ax` /
//! `selection.rs::macos_ax` (the repo has no precedent for an accessibility
//! crate). Adds only: full `AXValue` text, `CFRange` unpacking for
//! `kAXValueCFRangeType`, `AXStringForRange` + `AXNumberOfCharacters` for large
//! documents, and the **messaging timeout** both older copies lack.
//!
//! ## Coordinate systems
//!
//! All AX text indices are **UTF-16 code units**, while the window algorithm
//! works in chars. Chinese is 1 unit in UTF-16, emoji 2; the two coordinate
//! systems must be converted explicitly — see
//! [`utf16_offset_to_char_offset`](super::utf16_offset_to_char_offset).
//!
//! ## This file only runs inside `spawn_blocking`
//!
//! Every AX call can block up to `AX_MESSAGING_TIMEOUT_SECS`; none of it may
//! run on a tokio worker. Scheduling is handled by [`super::probe_around_cursor`].

use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::runloop::{
    kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult, CFRunLoopSource, CFRunLoopSourceRef,
};

use super::{
    evaluate_gate, minimal_edit, plan_window, utf16_offset_to_char_offset, window_around_cursor,
    EditPair, GateInputs, ReadOutcome, AX_MESSAGING_TIMEOUT_SECS,
};

mod context;
pub(crate) use context::capture as capture_document_context;
mod edit_observation;
pub(crate) use edit_observation::{
    complete as complete_edit_delivery, prepare as prepare_edit_delivery,
    spawn as spawn_edit_watcher, DeliveredInsertion, LearningStart,
};

/// Above this UTF-16 length, skip a full `AXValue` read and use
/// `AXStringForRange` to fetch only the span near the cursor.
///
/// On a 100k-character document `AXValue` copies the whole text across the
/// process boundary; marshalling alone can hit the timeout, and only a few
/// hundred characters are needed. The threshold sits far above any reasonable
/// budget, so normal documents keep the simple path.
const FULL_TEXT_MAX_UTF16: usize = 20_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentLength {
    Unknown,
    WithinLimit(usize),
    OverLimit(usize),
}

fn classify_document_length(total: Option<usize>, limit: usize) -> DocumentLength {
    match total {
        None => DocumentLength::Unknown,
        Some(total) if total <= limit => DocumentLength::WithinLimit(total),
        Some(total) => DocumentLength::OverLimit(total),
    }
}

#[repr(C)]
pub(crate) struct OpaqueAxRef(c_void);
pub(crate) type AxUiElementRef = *mut OpaqueAxRef;
type CFStringRef = *const c_void;
type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFTypeId = usize;
type AxError = i32;
type AxValueRef = *const c_void;

/// CoreFoundation's `CFRange` (`CFIndex` = `isize`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CFRange {
    location: isize,
    length: isize,
}

const AX_ERROR_SUCCESS: AxError = 0;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_AX_VALUE_CF_RANGE_TYPE: i32 = 4;
/// `kCFNumberCFIndexType` — read as `CFIndex` (isize), matching AX's index width.
const K_CF_NUMBER_CF_INDEX_TYPE: i32 = 14;

/// Opaque handle for AXObserver.
#[repr(C)]
pub(crate) struct OpaqueAxObserver(c_void);
pub(crate) type AxObserverRef = *mut OpaqueAxObserver;

type AxObserverCallback = unsafe extern "C" fn(
    observer: AxObserverRef,
    element: AxUiElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementGetTypeID() -> CFTypeId;
    pub(crate) fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    pub(crate) fn AXUIElementGetPid(element: AxUiElementRef, pid: *mut i32) -> AxError;
    pub(crate) fn AXObserverCreate(
        application: i32,
        callback: AxObserverCallback,
        observer: *mut AxObserverRef,
    ) -> AxError;
    pub(crate) fn AXObserverAddNotification(
        observer: AxObserverRef,
        element: AxUiElementRef,
        notification: CFStringRef,
        refcon: *mut c_void,
    ) -> AxError;
    fn AXObserverRemoveNotification(
        observer: AxObserverRef,
        element: AxUiElementRef,
        notification: CFStringRef,
    ) -> AxError;
    pub(crate) fn AXObserverGetRunLoopSource(observer: AxObserverRef) -> CFRunLoopSourceRef;
    pub(crate) fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, timeout: f32) -> AxError;
    pub(crate) fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    pub(crate) fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AxError;
    fn AXUIElementIsAttributeSettable(
        element: AxUiElementRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> AxError;
    pub(crate) fn AXUIElementCopyParameterizedAttributeValue(
        element: AxUiElementRef,
        parameterized_attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    pub(crate) fn AXValueGetValue(value: AxValueRef, value_type: i32, out: *mut c_void) -> u8;
    fn AXValueGetTypeID() -> CFTypeId;
    fn AXValueGetType(value: AxValueRef) -> i32;
    pub(crate) fn AXValueCreate(value_type: i32, value_ptr: *const c_void) -> AxValueRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeId;
    fn CFStringGetTypeID() -> CFTypeId;
    fn CFNumberGetTypeID() -> CFTypeId;
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        cstr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetLength(s: CFStringRef) -> isize;
    fn CFStringGetCharacters(s: CFStringRef, range: CFRange, buffer: *mut u16);
    fn CFNumberGetValue(number: CFTypeRef, number_type: i32, value_ptr: *mut c_void) -> u8;
}

/// Result of acquiring the focused element. The caller must `CFRelease` the ref in `Ready`.
enum GatedElement {
    Ready(AxUiElementRef),
    Blocked(super::BlockReason),
    Unavailable(&'static str),
}

/// **The only way to obtain the focused element — anything read from the host
/// app must come through here.**
///
/// Taking the element and passing the gate are fused: when they were separate,
/// the manual-edit observer opened its own path and read a terminal's full
/// text even though the context read was correctly blocked. One ungated path
/// means no gate.
///
/// Two checks in order, not merged:
///
/// 1. A coarse check against the **front app** (Secure Input, bundle
///    blacklist) — a hit sends no AX message at all;
/// 2. After taking the element, re-check using the **element's own pid** to
///    resolve the real bundle, together with `role` / `subrole` — this one
///    counts.
///
/// The second check re-resolves the bundle because the front app was sampled
/// before the element was taken and every AX call can block up to
/// [`AX_MESSAGING_TIMEOUT_SECS`]; if the user switched apps in between, the
/// stale identity would admit an element owned by the new app.
///
/// `AXUIElementSetMessagingTimeout` is also set here; without it AX's ~6s
/// default freezes the worker against a stuck app — the most important line
/// in this module.
unsafe fn focused_element_passing_the_gate(mut gate: GateInputs) -> GatedElement {
    if let Some(reason) = evaluate_gate(&gate) {
        return GatedElement::Blocked(reason);
    }

    // Query the captured foreground application directly. System-wide focus can
    // return kAXErrorCannotComplete even while this application exposes a valid editor.
    let Some(front_pid) = crate::selection::current_front_app_pid() else {
        return GatedElement::Unavailable("no foreground application");
    };
    let application = AXUIElementCreateApplication(front_pid);
    if application.is_null() {
        return GatedElement::Unavailable("application AX element unavailable");
    }
    AXUIElementSetMessagingTimeout(application, AX_MESSAGING_TIMEOUT_SECS);
    let focused = copy_element_attr(application, b"AXFocusedUIElement\0");
    CFRelease(application as CFTypeRef);

    let Some(focused) = focused else {
        return GatedElement::Unavailable("no focused UI element (AX permission or no focus)");
    };
    // Set again explicitly: the process default only applies to refs created afterwards.
    AXUIElementSetMessagingTimeout(focused, AX_MESSAGING_TIMEOUT_SECS);

    // Re-check with the element's own identity; do not trust the front app sampled earlier.
    //
    // **Fail closed: if ownership cannot be confirmed, do not read.** Keeping
    // the front-app value sampled earlier falls back to a frontmost-app gate,
    // and clearing `bundle_id` to `None` is another fail-open (`evaluate_gate`
    // allows missing metadata, see `missing_metadata_does_not_block_by_itself`).
    //
    // The cost is that processes without a bundle id get no context. Few such
    // processes exist, and not reading is this feature's privacy baseline.
    let mut pid: i32 = 0;
    let owner = (AXUIElementGetPid(focused, &mut pid) == AX_ERROR_SUCCESS && pid > 0)
        .then(|| crate::selection::bundle_id_for_pid(pid))
        .flatten();
    let Some(owner) = owner else {
        CFRelease(focused as CFTypeRef);
        return GatedElement::Unavailable("could not confirm which app owns the focused element");
    };
    if crate::selection::current_front_app_pid() != Some(pid) {
        CFRelease(focused as CFTypeRef);
        return GatedElement::Unavailable("foreground application changed during AX capture");
    }
    gate.bundle_id = Some(owner);
    // Secure Input is global state; refresh it too — it may have been enabled during these AX calls.
    gate.secure_input = crate::unicode_keystroke::is_secure_input_enabled();
    gate.role = copy_string_attr(focused, b"AXRole\0");
    gate.subrole = copy_string_attr(focused, b"AXSubrole\0");
    if let Some(reason) = evaluate_gate(&gate) {
        CFRelease(focused as CFTypeRef);
        return GatedElement::Blocked(reason);
    }

    GatedElement::Ready(focused)
}

/// Synchronously reads the document around the cursor. **Only callable in a
/// `spawn_blocking` context.**
///
/// `gate` carries the caller-filled `secure_input` / `bundle_id`;
/// [`focused_element_passing_the_gate`] fills in `role` / `subrole` and makes
/// the final decision.
pub(super) fn read_around_cursor_blocking(
    budget_chars: usize,
    gate: GateInputs,
    target: Option<crate::selection::SelectionInsertionTarget>,
) -> ReadOutcome {
    unsafe {
        if let Some(target) = target.as_ref() {
            if !crate::selection::selection_insertion_target_is_current(target) {
                return ReadOutcome::Unavailable("insertion target changed before capture");
            }
        }
        let focused = match focused_element_passing_the_gate(gate) {
            GatedElement::Ready(el) => el,
            GatedElement::Blocked(reason) => return ReadOutcome::Blocked(reason),
            GatedElement::Unavailable(why) => return ReadOutcome::Unavailable(why),
        };
        if let Some(target) = target.as_ref() {
            if !crate::selection::selection_insertion_target_is_current(target) {
                CFRelease(focused as CFTypeRef);
                return ReadOutcome::Unavailable("insertion target changed before AX read");
            }
        }
        let outcome = read_document(focused, budget_chars);
        if let Some(target) = target.as_ref() {
            if !crate::selection::selection_insertion_target_is_current(target) {
                CFRelease(focused as CFTypeRef);
                return ReadOutcome::Unavailable("insertion target changed during capture");
            }
        }
        CFRelease(focused as CFTypeRef);
        outcome
    }
}

unsafe fn read_document(focused: AxUiElementRef, budget_chars: usize) -> ReadOutcome {
    let Some(cursor_utf16) = copy_caret_offset(focused) else {
        return ReadOutcome::Unavailable("AXSelectedTextRange unavailable (not a text element?)");
    };
    let total_utf16 = match classify_document_length(
        copy_index_attr(focused, b"AXNumberOfCharacters\0"),
        FULL_TEXT_MAX_UTF16,
    ) {
        DocumentLength::Unknown => {
            return ReadOutcome::Unavailable(
                "AXNumberOfCharacters unavailable; refusing an unbounded AXValue read",
            );
        }
        DocumentLength::WithinLimit(total) => {
            // Small document (the common case): read the whole text and slice
            // the window precisely in chars. If AXValue is unreadable, fall
            // through to the bounded AXStringForRange path using the known length.
            if let Some(text) = copy_string_attr(focused, b"AXValue\0") {
                let cursor = utf16_offset_to_char_offset(&text, cursor_utf16);
                return ReadOutcome::Window(window_around_cursor(&text, cursor, budget_chars));
            }
            total
        }
        DocumentLength::OverLimit(total) => total,
    };

    // Fallback: the document is too large, or the control exposes no AXValue
    // (common in Electron apps). Request only a span around the cursor. The
    // UTF-16 budget is doubled — better to over-fetch and trim here than to
    // lose context to char/UTF-16 conversion drift.
    let span = plan_window(total_utf16, cursor_utf16, budget_chars.saturating_mul(2));
    if span.len == 0 {
        return ReadOutcome::Window(super::DocumentWindow {
            text: String::new(),
            cursor: 0,
        });
    }
    let Some(text) = copy_string_for_range(focused, span.start, span.len) else {
        return ReadOutcome::Unavailable("AXStringForRange unavailable");
    };
    let cursor = utf16_offset_to_char_offset(&text, span.cursor_in_span);
    ReadOutcome::Window(window_around_cursor(&text, cursor, budget_chars))
}

/// Reads the start of `AXSelectedTextRange` — with no selection it is the caret position (length == 0).
unsafe fn copy_caret_offset(focused: AxUiElementRef) -> Option<usize> {
    let range = copy_selected_range(focused)?;
    caret_offset_from_location(range.location)
}

/// Converts an `AXSelectedTextRange` location into a caret offset.
/// **Negative means "no caret", not 0.**
///
/// Some apps (notably Electron ones) return `kCFNotFound` (-1) when there is
/// no insertion point. The old `.max(0)` treated "caret unknown" as "caret at
/// the start", silently sending the document's opening lines as cursor
/// context (`before=0 after=N` in logs looked like empty context). Returning
/// `None` routes `read_document` to `Unavailable`: no context beats wrong
/// context.
fn caret_offset_from_location(location: isize) -> Option<usize> {
    (location >= 0).then_some(location as usize)
}

unsafe fn copy_selected_range(focused: AxUiElementRef) -> Option<CFRange> {
    let value = copy_attr(focused, b"AXSelectedTextRange\0")?;
    let range = crate::selection::bound_native::checked_range(
        || CFGetTypeID(value) == AXValueGetTypeID(),
        || AXValueGetType(value as AxValueRef) == K_AX_VALUE_CF_RANGE_TYPE,
        || {
            let mut range = CFRange::default();
            (AXValueGetValue(value as AxValueRef, K_AX_VALUE_CF_RANGE_TYPE,
                &mut range as *mut _ as *mut c_void) != 0).then_some(range)
        },
    );
    CFRelease(value);
    range
}

/// Selection coordinates only: reuse the same gated AX reader as delivery
/// verification without reading AXValue, selected text or surrounding text.
pub(crate) fn focused_input_position() -> Option<(u64, u64)> {
    unsafe {
        let GatedElement::Ready(element) = focused_element_passing_the_gate(voice_edit_gate()) else {
            return None;
        };
        let range = copy_selected_range(element);
        CFRelease(element as CFTypeRef);
        let range = range.filter(|range| range.location >= 0 && range.length >= 0)?;
        Some((range.location as u64, range.length as u64))
    }
}

#[derive(Debug, Clone)]
pub(super) struct VoiceEditTarget {
    element: SendableElement,
    text: String,
    selection_utf16: CFRange,
}

fn voice_edit_gate() -> GateInputs {
    GateInputs {
        secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
        bundle_id: crate::selection::current_front_app_parts().1,
        ..GateInputs::default()
    }
}

unsafe fn voice_edit_text(element: AxUiElementRef) -> Option<String> {
    // A context window or the selection module's 4000-char summary cannot be an edit baseline.
    let total = copy_index_attr(element, b"AXNumberOfCharacters\0")?;
    if total > FULL_TEXT_MAX_UTF16 {
        return None;
    }
    let text = copy_string_attr(element, b"AXValue\0")
        .or_else(|| copy_string_for_range(element, 0, total))?;
    (text.encode_utf16().count() == total).then_some(text)
}

pub(super) fn capture_voice_edit_target() -> Result<
    (
        String,
        Option<openless_core::TextSelection>,
        VoiceEditTarget,
    ),
    String,
> {
    unsafe {
        let GatedElement::Ready(element) = focused_element_passing_the_gate(voice_edit_gate())
        else {
            return Err("voiceEditTargetUnavailable".into());
        };
        let retained = SendableElement::retained(element);
        CFRelease(element as CFTypeRef);
        let text = voice_edit_text(retained.as_ref())
            .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?;
        let range = copy_selected_range(retained.as_ref())
            .filter(|range| {
                range.location >= 0
                    && range.length >= 0
                    && (range.location as usize)
                        .checked_add(range.length as usize)
                        .is_some_and(|end| end <= text.encode_utf16().count())
            })
            .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?;
        let GatedElement::Ready(current) = focused_element_passing_the_gate(voice_edit_gate())
        else {
            return Err("voiceEditTargetChanged".into());
        };
        let same = CFEqual(current as CFTypeRef, retained.as_ref() as CFTypeRef) != 0;
        CFRelease(current as CFTypeRef);
        if !same {
            return Err("voiceEditTargetChanged".into());
        }
        let selection = (range.length > 0).then(|| openless_core::TextSelection {
            start: utf16_offset_to_char_offset(&text, range.location as usize) as u32,
            end: utf16_offset_to_char_offset(&text, (range.location + range.length) as usize)
                as u32,
        });
        Ok((
            text.clone(),
            selection,
            VoiceEditTarget {
                element: retained,
                text,
                selection_utf16: range,
            },
        ))
    }
}

unsafe fn set_voice_edit_selection(element: AxUiElementRef, range: CFRange) -> Result<(), String> {
    let attribute = cfstring_from_static(b"AXSelectedTextRange\0")
        .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?;
    let value = AXValueCreate(
        K_AX_VALUE_CF_RANGE_TYPE,
        &range as *const _ as *const c_void,
    );
    if value.is_null() {
        CFRelease(attribute);
        return Err("voiceEditTargetUnavailable".into());
    }
    let selected = AXUIElementSetAttributeValue(element, attribute, value) == AX_ERROR_SUCCESS;
    CFRelease(attribute);
    CFRelease(value);
    if !selected || copy_selected_range(element) != Some(range) {
        return Err("voiceEditTargetUnavailable".into());
    }
    Ok(())
}

unsafe fn validate_voice_edit_target(
    target: &VoiceEditTarget,
    selection: CFRange,
) -> Result<SendableElement, String> {
    let GatedElement::Ready(element) = focused_element_passing_the_gate(voice_edit_gate()) else {
        return Err("voiceEditTargetUnavailable".into());
    };
    let retained = SendableElement::retained(element);
    CFRelease(element as CFTypeRef);
    let element = retained.as_ref();
    if CFEqual(element as CFTypeRef, target.element.as_ref() as CFTypeRef) == 0 {
        return Err("voiceEditTargetChanged".into());
    }
    if voice_edit_text(element).as_deref() != Some(target.text.as_str())
        || copy_selected_range(element) != Some(selection)
    {
        return Err("voiceEditFieldChanged".into());
    }
    Ok(retained)
}

pub(super) fn apply_voice_edit_target(
    target: &VoiceEditTarget,
    text: &str,
    insert: impl FnOnce(&str, &mut dyn FnMut() -> bool) -> Result<(), String>,
) -> Result<(), String> {
    unsafe {
        let retained = validate_voice_edit_target(target, target.selection_utf16)?;
        let element = retained.as_ref();
        let range = if target.selection_utf16.length > 0 {
            target.selection_utf16
        } else {
            CFRange {
                location: 0,
                length: target.text.encode_utf16().count() as isize,
            }
        };
        super::write_with_selection_recovery(
            || {
                set_voice_edit_selection(element, range)?;
                validate_voice_edit_target(target, range)?;
                // The predicate only reads the prepared range; never reselect after waiting.
                insert(text, &mut || {
                    validate_voice_edit_target(target, range).is_ok()
                })
            },
            || {
                if validate_voice_edit_target(target, range).is_ok() {
                    let _ = set_voice_edit_selection(element, target.selection_utf16);
                }
            },
        )
    }
}

/// Synchronous owner-scoped read seam for P5. Read only these borrowed original
/// objects; never recapture focus as a substitute or let native refs escape.
/// Call on spawn_blocking; this does not supply focus/tab continuity evidence.
pub(crate) fn with_frozen_target_read<R>(
    target: &crate::selection::SelectionInsertionTarget,
    read: impl for<'a> FnOnce(&crate::selection::FrozenMacosTarget<'a>) -> Result<R, String>,
) -> Result<R, String> {
    crate::selection::borrow_frozen_macos_target(target, |lease| {
        let (control, window) = unsafe { lease.ax_objects() };
        let native = FrozenSelectionNative { lease };
        use crate::selection::bound_native::Native;
        native.guard(&control, &window).map_err(|error| error.message().to_string())?;
        let result = read(lease)?;
        native.guard(&control, &window).map_err(|error| error.message().to_string())?;
        Ok(result)
    })
}

struct FrozenSelectionNative<'a, 'owner> {
    lease: &'a crate::selection::FrozenMacosTarget<'owner>,
}

impl crate::selection::bound_native::Native for FrozenSelectionNative<'_, '_> {
    type Control = *mut c_void;
    type Window = *mut c_void;

    fn guard(&self, control: &Self::Control, window: &Self::Window)
        -> Result<(), crate::selection::bound_native::Failure>
    {
        use crate::selection::bound_native::Failure;
        unsafe {
            if crate::permissions::check_accessibility() != crate::permissions::PermissionStatus::Granted
                || crate::unicode_keystroke::is_secure_input_enabled()
            {
                return Err(Failure::Unavailable);
            }
            let (owned_control, owned_window) = self.lease.ax_objects();
            if CFGetTypeID((*control).cast()) != AXUIElementGetTypeID()
                || CFGetTypeID((*window).cast()) != AXUIElementGetTypeID()
            {
                return Err(Failure::Unavailable);
            }
            if CFEqual((*control).cast(), owned_control.cast()) == 0
                || CFEqual((*window).cast(), owned_window.cast()) == 0
                || !self.lease.is_current()
            {
                return Err(Failure::TargetChanged);
            }
            let element = (*control).cast::<OpaqueAxRef>();
            let window = (*window).cast::<OpaqueAxRef>();
            for object in [element, window] {
                AXUIElementSetMessagingTimeout(object, AX_MESSAGING_TIMEOUT_SECS);
                let mut pid = 0;
                if AXUIElementGetPid(object, &mut pid) != AX_ERROR_SUCCESS
                    || pid != self.lease.process_id()
                {
                    return Err(Failure::TargetChanged);
                }
            }
            let owner_window = copy_element_attr(element, b"AXWindow\0").ok_or(Failure::Unavailable)?;
            let same_window = CFEqual(owner_window.cast(), window.cast()) != 0;
            CFRelease(owner_window.cast());
            if !same_window { return Err(Failure::TargetChanged); }
            let bundle_id = crate::selection::bundle_id_for_pid(self.lease.process_id())
                .ok_or(Failure::Unavailable)?;
            let role = copy_string_attr(element, b"AXRole\0").ok_or(Failure::Unavailable)?;
            let gate = GateInputs {
                secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
                bundle_id: Some(bundle_id),
                role: Some(role),
                subrole: copy_string_attr(element, b"AXSubrole\0"),
            };
            if evaluate_gate(&gate).is_some() { return Err(Failure::Unavailable); }
            if !self.lease.is_current() { return Err(Failure::TargetChanged); }
            Ok(())
        }
    }

    fn selected_range(&self, control: &Self::Control)
        -> Result<crate::selection::bound_native::TextRange, crate::selection::bound_native::Failure>
    {
        use crate::selection::bound_native::{Failure, TextRange};
        let range = unsafe { copy_selected_range((*control).cast()) }.ok_or(Failure::Unavailable)?;
        Ok(TextRange {
            start: u64::try_from(range.location).map_err(|_| Failure::Unavailable)?,
            length: u64::try_from(range.length).map_err(|_| Failure::Unavailable)?,
        })
    }

    fn text(&self, control: &Self::Control, range: crate::selection::bound_native::TextRange)
        -> Result<String, crate::selection::bound_native::Failure>
    {
        use crate::selection::bound_native::Failure;
        let end = range.start.checked_add(range.length).ok_or(Failure::Unavailable)?;
        isize::try_from(end).map_err(|_| Failure::Unavailable)?;
        unsafe { copy_string_for_range((*control).cast(), range.start as usize, range.length as usize) }
            .ok_or(Failure::Unavailable)
    }

    fn writable(&self, control: &Self::Control) -> Result<(), crate::selection::bound_native::Failure> {
        use crate::selection::bound_native::Failure;
        unsafe {
            let attribute = cfstring_from_static(b"AXSelectedText\0").ok_or(Failure::Unavailable)?;
            let mut settable = 0u8;
            let result = AXUIElementIsAttributeSettable((*control).cast(), attribute, &mut settable);
            CFRelease(attribute);
            if result != AX_ERROR_SUCCESS || settable == 0 { return Err(Failure::Unavailable); }
            Ok(())
        }
    }

    fn submit(&self, control: &Self::Control, replacement: &str)
        -> Result<(), crate::selection::bound_native::Failure>
    {
        use crate::selection::bound_native::Failure;
        unsafe {
            let value = core_foundation::string::CFString::new(replacement);
            let attribute = cfstring_from_static(b"AXSelectedText\0").ok_or(Failure::Unavailable)?;
            let result = AXUIElementSetAttributeValue((*control).cast(), attribute,
                value.as_concrete_TypeRef().cast());
            CFRelease(attribute);
            if result != AX_ERROR_SUCCESS { return Err(Failure::WriteUnconfirmed); }
            Ok(())
        }
    }
}

/// No focused-element supplier exists in this path. All operations address the
/// immutable frozen control and AX window kept alive by the owner callback.
pub(super) fn replace_selection_direct(
    target: &crate::selection::SelectionInsertionTarget,
    original: &str,
    replacement: &str,
) -> Result<(), String> {
    use crate::selection::bound_native::{self, Binding, TextRange};
    crate::selection::borrow_frozen_macos_target(target, |lease| {
        let (control, window) = unsafe { lease.ax_objects() };
        let (start, length) = lease.range().ok_or("selectionPolishTargetUnavailable")?;
        let binding = Binding { control, window, range: TextRange { start, length } };
        bound_native::replace(&FrozenSelectionNative { lease }, &binding, original, replacement)
            .map_err(|error| error.message().to_string())
    })
}

/// Confirms all posted keyboard input against the original text control's caret.
/// Only metadata is read; the target's document text is never fetched.
/// Create, wait and drop on the same blocking insertion thread.
pub(crate) struct KeyboardDelivery {
    element: AxUiElementRef,
    progress: KeyboardDeliveryProgress,
}

impl KeyboardDelivery {
    pub(crate) fn capture() -> Option<Self> {
        let gate = GateInputs {
            secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
            bundle_id: crate::selection::current_front_app_parts().1,
            ..GateInputs::default()
        };
        // SAFETY: the shared gate returns a retained AX element with a messaging
        // timeout. This thread owns it until Drop, including failed caret reads.
        unsafe {
            let GatedElement::Ready(element) = focused_element_passing_the_gate(gate) else {
                return None;
            };
            Some(Self {
                element,
                progress: KeyboardDeliveryProgress::new(copy_caret_offset(element)),
            })
        }
    }

    pub(crate) fn is_focused(&self) -> bool {
        // AX capture can take time. Recheck the exact control before sending
        // keys so a focus change during capture does not redirect this chunk.
        unsafe {
            let GatedElement::Ready(focused) = focused_element_passing_the_gate(voice_edit_gate())
            else {
                return false;
            };
            let same = CFEqual(focused as CFTypeRef, self.element as CFTypeRef) != 0;
            CFRelease(focused as CFTypeRef);
            same
        }
    }

    /// Account only for the prefix actually posted by the native typer. This
    /// never reads AX or waits, so another streamed chunk can follow immediately.
    pub(crate) fn record_posted(
        &mut self,
        posted_text: &str,
        newline_mode: crate::types::MacosNewlineMode,
    ) {
        self.progress.record_posted(posted_text, newline_mode);
    }

    /// One terminal delivery barrier, on the same thread that captured the
    /// element. Unreadable/stale controls keep the existing posted-input fallback.
    pub(crate) fn finish(self) -> KeyboardDeliveryOutcome {
        let started = Instant::now();
        let outcome = match self.progress.expected() {
            DeliveryExpectation::NothingPosted => KeyboardDeliveryOutcome::Delivered,
            DeliveryExpectation::Unavailable => KeyboardDeliveryOutcome::Unavailable,
            DeliveryExpectation::Caret { start, expected } => wait_for_caret_delivery(
                start,
                expected,
                || {
                    if crate::unicode_keystroke::is_secure_input_enabled() {
                        return None;
                    }
                    // SAFETY: self retains the original control on this worker.
                    unsafe { copy_selected_range(self.element) }.and_then(|range| {
                        if range.length < 0 {
                            return None;
                        }
                        caret_offset_from_location(range.location)
                            .map(|offset| (offset, range.length))
                    })
                },
                || started.elapsed(),
                || std::thread::sleep(Duration::from_millis(10)),
            ),
        };
        log::info!(
            "[insertion] final keyboard delivery outcome={outcome:?} elapsed_ms={}",
            started.elapsed().as_millis()
        );
        outcome
    }
}

impl Drop for KeyboardDelivery {
    fn drop(&mut self) {
        // SAFETY: capture owns exactly one retained reference.
        unsafe { CFRelease(self.element as CFTypeRef) };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyboardDeliveryOutcome {
    Delivered,
    Unavailable,
    NoProgress,
    TimedOut,
}

const DELIVERY_NO_PROGRESS_BUDGET: Duration = Duration::from_millis(250);
const DELIVERY_TOTAL_BUDGET: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq)]
enum DeliveryExpectation {
    NothingPosted,
    Unavailable,
    Caret { start: usize, expected: usize },
}

/// Pure accounting, independent of AX ownership. A terminal capture cannot
/// substitute for this cumulative offset: previously posted keys may still queue.
#[derive(Debug)]
struct KeyboardDeliveryProgress {
    start: Option<usize>,
    expected: Option<usize>,
    posted: bool,
}

impl KeyboardDeliveryProgress {
    fn new(start: Option<usize>) -> Self {
        Self {
            start,
            expected: start,
            posted: false,
        }
    }

    fn record_posted(&mut self, text: &str, newline_mode: crate::types::MacosNewlineMode) {
        let mut units = 0usize;
        for ch in text.chars().filter(|ch| *ch != '\r') {
            self.posted = true;
            units = units.saturating_add(ch.len_utf16());
            if ch == '\n' && newline_mode == crate::types::MacosNewlineMode::Return {
                // A submitting Return can clear the field or move focus. Keep
                // its keys, but never wait for a monotonically increasing caret.
                self.expected = None;
            }
        }
        self.expected = self.expected.and_then(|offset| offset.checked_add(units));
    }

    fn expected(&self) -> DeliveryExpectation {
        if !self.posted {
            return DeliveryExpectation::NothingPosted;
        }
        match (self.start, self.expected) {
            (Some(start), Some(expected)) => DeliveryExpectation::Caret { start, expected },
            _ => DeliveryExpectation::Unavailable,
        }
    }
}

fn wait_for_caret_delivery(
    start: usize,
    expected: usize,
    mut read: impl FnMut() -> Option<(usize, isize)>,
    mut elapsed: impl FnMut() -> Duration,
    mut pause: impl FnMut(),
) -> KeyboardDeliveryOutcome {
    let mut high_water = start;
    let mut last_progress = Duration::ZERO;
    loop {
        let now = elapsed();
        if now >= DELIVERY_TOTAL_BUDGET {
            return KeyboardDeliveryOutcome::TimedOut;
        }
        if now.saturating_sub(last_progress) >= DELIVERY_NO_PROGRESS_BUDGET {
            return KeyboardDeliveryOutcome::NoProgress;
        }
        let Some((offset, selected_length)) = read() else {
            return KeyboardDeliveryOutcome::Unavailable;
        };
        let now = elapsed();
        if now >= DELIVERY_TOTAL_BUDGET {
            return KeyboardDeliveryOutcome::TimedOut;
        }
        if selected_length == 0 && offset >= expected {
            return KeyboardDeliveryOutcome::Delivered;
        }
        if offset > high_water {
            high_water = offset;
            last_progress = now;
        }
        if now.saturating_sub(last_progress) >= DELIVERY_NO_PROGRESS_BUDGET {
            return KeyboardDeliveryOutcome::NoProgress;
        }
        pause();
    }
}

#[cfg(test)]
mod keyboard_delivery_tests {
    use super::*;
    use crate::types::MacosNewlineMode;
    use std::cell::Cell;

    #[test]
    fn cumulative_receipt_counts_only_posted_unicode_and_ignores_swallowed_cr() {
        let mut progress = KeyboardDeliveryProgress::new(Some(7));
        assert_eq!(progress.expected(), DeliveryExpectation::NothingPosted);
        progress.record_posted("A🙂\r", MacosNewlineMode::ShiftReturn);
        progress.record_posted("\n界", MacosNewlineMode::ShiftReturn);
        assert_eq!(
            progress.expected(),
            DeliveryExpectation::Caret {
                start: 7,
                expected: 12
            }
        );
    }

    #[test]
    fn submitting_return_and_unknown_offsets_keep_posted_input_fallback() {
        let mut progress = KeyboardDeliveryProgress::new(Some(5));
        progress.record_posted("first", MacosNewlineMode::Return);
        progress.record_posted("\nsecond", MacosNewlineMode::Return);
        progress.record_posted("more", MacosNewlineMode::Return);
        assert_eq!(progress.expected(), DeliveryExpectation::Unavailable);
        let mut unknown = KeyboardDeliveryProgress::new(None);
        unknown.record_posted("text", MacosNewlineMode::ShiftReturn);
        assert_eq!(unknown.expected(), DeliveryExpectation::Unavailable);
        let mut cr = KeyboardDeliveryProgress::new(None);
        cr.record_posted("\r", MacosNewlineMode::Return);
        assert_eq!(cr.expected(), DeliveryExpectation::NothingPosted);
    }

    #[test]
    fn delivery_offset_overflow_does_not_wrap_into_a_false_receipt() {
        let mut progress = KeyboardDeliveryProgress::new(Some(usize::MAX));
        progress.record_posted("x", MacosNewlineMode::ShiftReturn);
        assert_eq!(progress.expected(), DeliveryExpectation::Unavailable);
    }

    #[test]
    fn final_delivery_waits_for_the_cumulative_end_not_a_selected_range() {
        let mut samples = [(120, 20), (101, 0), (119, 0), (120, 0)].into_iter();
        let clock = Cell::new(Duration::ZERO);
        assert_eq!(
            wait_for_caret_delivery(
                100,
                120,
                || samples.next(),
                || clock.get(),
                || clock.set(clock.get() + Duration::from_millis(10))
            ),
            KeyboardDeliveryOutcome::Delivered
        );
        assert_eq!(clock.get(), Duration::from_millis(30));
    }

    #[test]
    fn stale_readable_caret_stops_after_short_no_progress_budget() {
        let clock = Cell::new(Duration::ZERO);
        assert_eq!(
            wait_for_caret_delivery(
                100,
                120,
                || Some((100, 0)),
                || clock.get(),
                || clock.set(clock.get() + Duration::from_millis(10))
            ),
            KeyboardDeliveryOutcome::NoProgress
        );
        assert_eq!(clock.get(), DELIVERY_NO_PROGRESS_BUDGET);
    }

    #[test]
    fn progressing_target_can_take_longer_than_one_no_progress_budget() {
        let clock = Cell::new(Duration::ZERO);
        let offset = Cell::new(0);
        assert_eq!(
            wait_for_caret_delivery(
                0,
                5,
                || {
                    offset.set(offset.get() + 1);
                    Some((offset.get(), 0))
                },
                || clock.get(),
                || clock.set(clock.get() + Duration::from_millis(200))
            ),
            KeyboardDeliveryOutcome::Delivered
        );
        assert_eq!(clock.get(), Duration::from_millis(800));
    }

    #[test]
    fn even_progressing_targets_have_a_hard_deadline() {
        let clock = Cell::new(Duration::ZERO);
        let offset = Cell::new(0);
        assert_eq!(
            wait_for_caret_delivery(
                0,
                usize::MAX,
                || {
                    offset.set(offset.get() + 1);
                    Some((offset.get(), 0))
                },
                || clock.get(),
                || clock.set(clock.get() + Duration::from_millis(100))
            ),
            KeyboardDeliveryOutcome::TimedOut
        );
        assert_eq!(clock.get(), DELIVERY_TOTAL_BUDGET);
    }

    #[test]
    fn unavailable_range_never_waits() {
        assert_eq!(
            wait_for_caret_delivery(
                0,
                5,
                || None,
                || Duration::ZERO,
                || panic!("unavailable target must not wait")
            ),
            KeyboardDeliveryOutcome::Unavailable
        );
    }

    #[test]
    fn a_slow_ax_read_cannot_extend_the_total_deadline() {
        let clock = Cell::new(Duration::ZERO);
        assert_eq!(
            wait_for_caret_delivery(
                0,
                5,
                || {
                    clock.set(DELIVERY_TOTAL_BUDGET);
                    Some((5, 0))
                },
                || clock.get(),
                || panic!("deadline already elapsed"),
            ),
            KeyboardDeliveryOutcome::TimedOut
        );
    }
}

/// `AXStringForRange(range)` — copies only the span near the cursor across the process boundary.
unsafe fn copy_string_for_range(
    focused: AxUiElementRef,
    start: usize,
    len: usize,
) -> Option<String> {
    let attr = cfstring_from_static(b"AXStringForRange\0")?;
    let range = CFRange {
        location: start as isize,
        length: len as isize,
    };
    let range_value = AXValueCreate(
        K_AX_VALUE_CF_RANGE_TYPE,
        &range as *const _ as *const c_void,
    );
    if range_value.is_null() {
        CFRelease(attr);
        return None;
    }

    let mut out: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(focused, attr, range_value, &mut out);
    CFRelease(attr);
    CFRelease(range_value);
    if err != AX_ERROR_SUCCESS || out.is_null() {
        return None;
    }

    let text = if CFGetTypeID(out) == CFStringGetTypeID() {
        cfstring_to_rust(out)
    } else {
        None
    };
    CFRelease(out);
    text
}

/// Reads an attribute and verifies it is really a CFString.
///
/// The type check is not redundant: `AXValue` is a number on sliders, a
/// boolean on checkboxes; decoding a CFNumber as a string yields garbage or
/// an out-of-bounds read.
unsafe fn copy_string_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<String> {
    let value = copy_attr(element, attribute)?;
    let text = if CFGetTypeID(value) == CFStringGetTypeID() {
        cfstring_to_rust(value)
    } else {
        None
    };
    CFRelease(value);
    text
}

/// Reads a CFNumber attribute as a `CFIndex`.
unsafe fn copy_index_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<usize> {
    let value = copy_attr(element, attribute)?;
    if CFGetTypeID(value) != CFNumberGetTypeID() {
        CFRelease(value);
        return None;
    }
    let mut out: isize = 0;
    let ok = CFNumberGetValue(
        value,
        K_CF_NUMBER_CF_INDEX_TYPE,
        &mut out as *mut _ as *mut c_void,
    );
    CFRelease(value);
    if ok != 0 && out >= 0 {
        Some(out as usize)
    } else {
        None
    }
}

/// Reads an attribute whose value is itself an AXUIElement (e.g. `AXFocusedUIElement`).
unsafe fn copy_element_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<AxUiElementRef> {
    let value = copy_attr(element, attribute)?;
    if CFGetTypeID(value) == AXUIElementGetTypeID() {
        Some(value as AxUiElementRef)
    } else {
        CFRelease(value);
        None
    }
}

/// Reads the raw CFTypeRef of any attribute. **Caller must `CFRelease`.**
unsafe fn copy_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<CFTypeRef> {
    let attr = cfstring_from_static(attribute)?;
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
    CFRelease(attr);
    if err != AX_ERROR_SUCCESS || value.is_null() {
        None
    } else {
        Some(value)
    }
}

unsafe fn cfstring_from_static(bytes_with_nul: &[u8]) -> Option<CFStringRef> {
    let cstr = CStr::from_bytes_with_nul(bytes_with_nul).ok()?;
    let s = CFStringCreateWithCString(std::ptr::null(), cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
    if s.is_null() {
        None
    } else {
        Some(s)
    }
}

unsafe fn cfstring_to_rust(s: CFStringRef) -> Option<String> {
    let length = CFStringGetLength(s);
    if length < 0 {
        return None;
    }
    let mut units = vec![0u16; length as usize];
    CFStringGetCharacters(s, CFRange { location: 0, length }, units.as_mut_ptr());
    String::from_utf16(&units).ok()
}

// ═══════════════════════════════════════════════════════════════════════════
// Manual edit watch (AXObserver)
// ═══════════════════════════════════════════════════════════════════════════
//
// Same shape as `device_watch.rs` (CoreAudio device watch): dedicated thread →
// register callback (user_data double indirection wrapping the closure fat
// pointer) → `CFRunLoop::run_in_mode(1s)` rotation + stop flag → unregister
// before exit → warn only on failure. Cross-thread `CFRunLoopStop` is avoided
// for the same reason documented there: it races and can leak the thread.
//
// **Disarm must be guaranteed.** A leaked observer keeps AX references to
// another app and wakes on every keystroke — a resource leak and a privacy
// issue. Three safeguards: caller disarm, 60s hard timeout, self-termination
// when the front app changes.

/// Carrier for passing an AX reference across threads.
///
/// `AXUIElementRef` is a CFType, safe to use across threads (CF refcounting is
/// atomic), but the raw pointer is not `Send`. Same approach as
/// `unicode_keystroke::PreviousInputSource`: store as `usize` + manual `Send`,
/// `CFRetain` before handoff, `CFRelease` when done.
///
/// The element is captured on the calling thread instead of having the worker
/// read `AXFocusedUIElement` itself: arming happens right after insertion
/// lands, while focus is still on the target control; a new thread reading a
/// few ms later may find focus already moved elsewhere.
#[derive(Debug)]
struct SendableElement(usize);
unsafe impl Send for SendableElement {}

impl Clone for SendableElement {
    fn clone(&self) -> Self {
        unsafe { Self::retained(self.as_ref()) }
    }
}

impl SendableElement {
    /// # Safety
    /// `element` must be a valid `AXUIElementRef`. This function retains it
    /// itself; the caller's ownership is unaffected (still must be released).
    unsafe fn retained(element: AxUiElementRef) -> Self {
        CFRetain(element as CFTypeRef);
        Self(element as usize)
    }

    fn as_ref(&self) -> AxUiElementRef {
        self.0 as AxUiElementRef
    }
}

impl Drop for SendableElement {
    fn drop(&mut self) {
        // SAFETY: `retained` did one CFRetain; release it in pairs here.
        unsafe { CFRelease(self.0 as CFTypeRef) };
    }
}

#[cfg(test)]
mod tests {
    use super::{caret_offset_from_location, classify_document_length, DocumentLength};

    #[test]
    fn unknown_document_length_is_not_safe_for_a_full_value_read() {
        assert_eq!(
            classify_document_length(None, 20_000),
            DocumentLength::Unknown
        );
    }

    #[test]
    fn small_document_length_allows_a_full_value_read() {
        assert_eq!(
            classify_document_length(Some(20_000), 20_000),
            DocumentLength::WithinLimit(20_000)
        );
    }

    #[test]
    fn large_document_length_requires_a_bounded_range_read() {
        assert_eq!(
            classify_document_length(Some(20_001), 20_000),
            DocumentLength::OverLimit(20_001)
        );
    }

    /// A negative location is a "no caret" sentinel and must stay distinct
    /// from "caret at the start".
    ///
    /// Electron apps repeatedly produced `before=0 after=N` on hardware,
    /// long taken as "this app's context is unreadable"; actually
    /// `AXSelectedTextRange` returned kCFNotFound(-1), clamped to 0, so the
    /// document's opening was read and sent to the LLM as cursor context.
    /// Wrong context is worse than none — it looks right.
    #[test]
    fn a_negative_caret_location_is_not_the_start_of_the_document() {
        assert_eq!(caret_offset_from_location(0), Some(0), "光标真在开头");
        assert_eq!(caret_offset_from_location(42), Some(42));
        assert_eq!(
            caret_offset_from_location(-1),
            None,
            "kCFNotFound：没有光标"
        );
        assert_eq!(caret_offset_from_location(isize::MIN), None);
    }
}

/// Owns only the transcript range, never the rest of the user's document.
/// Every update verifies control identity, complete baseline and caret before writing.
pub(crate) struct LiveComposition {
    target: VoiceEditTarget,
    original: String,
    original_selection: CFRange,
    start: usize,
    end: usize,
    wrote: bool,
}

impl LiveComposition {
    pub(crate) fn has_written(&self) -> bool {
        self.wrote
    }

    pub(crate) fn capture() -> Result<Self, String> {
        match crate::selection::current_front_app_pid() {
            Some(pid) if pid != std::process::id() as i32 => {}
            _ => return Err("liveInputTargetUnavailable".into()),
        }
        let (_, _, target) = capture_voice_edit_target()?;
        let mut owner_pid = 0;
        if unsafe { AXUIElementGetPid(target.element.as_ref(), &mut owner_pid) } != AX_ERROR_SUCCESS
            || owner_pid <= 0
            || owner_pid == std::process::id() as i32
        {
            return Err("liveInputTargetUnavailable".into());
        }
        let start =
            utf16_offset_to_char_offset(&target.text, target.selection_utf16.location as usize);
        let end = utf16_offset_to_char_offset(
            &target.text,
            (target.selection_utf16.location + target.selection_utf16.length) as usize,
        );
        let original = target.text.chars().skip(start).take(end - start).collect();
        let original_selection = target.selection_utf16;
        Ok(Self {
            target,
            original,
            original_selection,
            start,
            end,
            wrote: false,
        })
    }

    pub(crate) fn replace(&mut self, text: &str) -> Result<(), String> {
        unsafe {
            let GatedElement::Ready(current) = focused_element_passing_the_gate(voice_edit_gate())
            else {
                return Err("liveInputTargetUnavailable".into());
            };
            let same = CFEqual(
                current as CFTypeRef,
                self.target.element.as_ref() as CFTypeRef,
            ) != 0;
            CFRelease(current as CFTypeRef);
            if !same
                || voice_edit_text(self.target.element.as_ref()).as_deref()
                    != Some(&self.target.text)
                || copy_selected_range(self.target.element.as_ref())
                    != Some(self.target.selection_utf16)
            {
                return Err("liveInputTargetChanged".into());
            }
            let start = self
                .target
                .text
                .chars()
                .take(self.start)
                .collect::<String>();
            let suffix = self.target.text.chars().skip(self.end).collect::<String>();
            let range = CFRange {
                location: start.encode_utf16().count() as isize,
                length: self
                    .target
                    .text
                    .chars()
                    .skip(self.start)
                    .take(self.end - self.start)
                    .collect::<String>()
                    .encode_utf16()
                    .count() as isize,
            };
            set_voice_edit_selection(self.target.element.as_ref(), range)?;
            let value = std::ffi::CString::new(text).map_err(|_| "liveInputInvalidText")?;
            let value = CFStringCreateWithCString(
                std::ptr::null(),
                value.as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            );
            let attr = cfstring_from_static(b"AXSelectedText\0").ok_or("liveInputUnavailable")?;
            // A failed AX call can have an unknown outcome; never retarget after submission.
            self.wrote = true;
            let result = AXUIElementSetAttributeValue(self.target.element.as_ref(), attr, value);
            CFRelease(attr);
            CFRelease(value);
            if result != AX_ERROR_SUCCESS {
                return Err("liveInputUnsupportedEditor".into());
            }
            let expected = format!("{start}{text}{suffix}");
            if voice_edit_text(self.target.element.as_ref()).as_deref() != Some(&expected) {
                return Err("liveInputEditorTransformedText".into());
            }
            self.end = self.start + text.chars().count();
            self.target.text = expected;
            self.target.selection_utf16 = copy_selected_range(self.target.element.as_ref())
                .ok_or("liveInputCaretUnavailable")?;
            Ok(())
        }
    }

    pub(crate) fn cancel(&mut self) -> Result<(), String> {
        if !self.wrote {
            return Ok(());
        }
        self.replace(&self.original.clone())?;
        unsafe { set_voice_edit_selection(self.target.element.as_ref(), self.original_selection) }
    }
}

#[cfg(test)]
mod live_composition_acceptance {
    use super::*;

    #[test]
    #[ignore = "requires the foreground disposable native-live-input-fixture app"]
    fn live_original_translation_cancel_and_user_caret_changes() {
        unsafe extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        assert!(
            unsafe { AXIsProcessTrusted() },
            "native acceptance executable needs Accessibility permission"
        );
        println!(
            "fixture_front={:?}",
            crate::selection::current_front_app_parts()
        );
        unsafe {
            match focused_element_passing_the_gate(voice_edit_gate()) {
                GatedElement::Ready(element) => {
                    println!(
                        "fixture_role={:?} chars={:?} range={:?}",
                        copy_string_attr(element, b"AXRole\0"),
                        copy_index_attr(element, b"AXNumberOfCharacters\0"),
                        copy_selected_range(element)
                    );
                    CFRelease(element as CFTypeRef);
                }
                GatedElement::Blocked(reason) => println!("fixture_gate_blocked={reason:?}"),
                GatedElement::Unavailable(reason) => println!("fixture_unavailable={reason}"),
            }
        }
        let (baseline, _, _) = capture_voice_edit_target().expect("fixture accessibility");
        assert_eq!(
            baseline, "before 原文 after",
            "refuse to modify a non-fixture editor"
        );
        let delivery = KeyboardDelivery::capture().expect("fixture keyboard delivery target");
        assert!(delivery.is_focused());
        let mut live = LiveComposition::capture().unwrap();
        live.replace("这是一段实时原文").unwrap();
        assert_eq!(
            capture_voice_edit_target().unwrap().0,
            "before 这是一段实时原文 after"
        );
        live.replace("This is the final translation.").unwrap();
        assert_eq!(
            capture_voice_edit_target().unwrap().0,
            "before This is the final translation. after"
        );
        live.cancel().unwrap();
        assert_eq!(capture_voice_edit_target().unwrap().0, baseline);
        let mut live = LiveComposition::capture().unwrap();
        unsafe {
            set_voice_edit_selection(
                live.target.element.as_ref(),
                CFRange {
                    location: 0,
                    length: 0,
                },
            )
            .unwrap();
        }
        assert!(live
            .replace("must not overwrite manual cursor changes")
            .is_err());
        assert_eq!(capture_voice_edit_target().unwrap().0, baseline);
        println!("NATIVE_LIVE_INPUT=PASS partial=replace final=translation cancel=baseline caret_change=refused");
    }
}
