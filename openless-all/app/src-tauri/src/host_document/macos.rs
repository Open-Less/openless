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

/// How long a caret notification must trail the last text change to count as
/// "the user actually moved the caret away".
///
/// The two notifications arrive as a **pair**: one keystroke yields
/// `AXValueChanged` then `AXSelectedTextChanged` a few milliseconds apart.
/// Without this threshold the second one is misread as "caret moved away",
/// so every keystroke triggers a verdict that rejects all intermediate
/// states — on real hardware this meant nothing was ever learned.
///
/// 300ms: far above the paired-notification gap (milliseconds), far below
/// the gap to "stopped typing and clicked elsewhere".
const CARET_MOVE_QUIET: Duration = Duration::from_millis(300);

/// **Fallback** criterion for "this edit is finished": how long without
/// activity before forcing a verdict. The primary criterion is semantic —
/// the caret leaving this spot (see `value_changed_shim`).
///
/// 5s, not ~1s: intermediate states of a multi-keystroke correction are all
/// legal-looking cross-script edits; judging them early would feed several
/// garbage entries per corrected word into the lexicon. A long window also
/// avoids cutting off a user who pauses mid-edit. Known cost: an edit
/// followed by more typing only settles once the user stops, and
/// [`minimal_edit`](super::minimal_edit) can express a single contiguous
/// diff, so the correction and the following text merge into one diff that
/// may be rejected or polluted.
const EDIT_SETTLE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to wait for our own inserted text to land before anchoring the
/// baseline to the current document state.
///
/// When the target app transforms inserted text (smart quotes, autocomplete,
/// glyph conversion), the exact text never appears; waiting forever would
/// silently disable the feature — a slightly off baseline is preferable.
const BASELINE_ANCHOR_TIMEOUT: Duration = Duration::from_millis(1500);

#[repr(C)]
struct OpaqueAxRef(c_void);
type AxUiElementRef = *mut OpaqueAxRef;
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
struct OpaqueAxObserver(c_void);
type AxObserverRef = *mut OpaqueAxObserver;

type AxObserverCallback = unsafe extern "C" fn(
    observer: AxObserverRef,
    element: AxUiElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AxUiElementRef;
    fn AXUIElementGetPid(element: AxUiElementRef, pid: *mut i32) -> AxError;
    fn AXObserverCreate(
        application: i32,
        callback: AxObserverCallback,
        observer: *mut AxObserverRef,
    ) -> AxError;
    fn AXObserverAddNotification(
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
    fn AXObserverGetRunLoopSource(observer: AxObserverRef) -> CFRunLoopSourceRef;
    fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, timeout: f32) -> AxError;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AxError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AxUiElementRef,
        parameterized_attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    fn AXValueGetValue(value: AxValueRef, value_type: i32, out: *mut c_void) -> u8;
    fn AXValueCreate(value_type: i32, value_ptr: *const c_void) -> AxValueRef;
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
    fn CFStringGetCStringPtr(s: CFStringRef, encoding: u32) -> *const c_char;
    // Returns `u8`, not `bool`: CoreFoundation's `Boolean` is `unsigned char`,
    // not C's `_Bool`. Rust's `bool` requires the bit pattern to be exactly 0
    // or 1 (anything else is UB), so it must not receive an `unsigned char`.
    // `AXValueGetValue` in this file already uses `u8`.
    fn CFStringGetCString(
        s: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFStringGetLength(s: CFStringRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
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

    let system = AXUIElementCreateSystemWide();
    if system.is_null() {
        return GatedElement::Unavailable("system-wide AX element unavailable");
    }
    // Settings on the system-wide element become this process's default.
    AXUIElementSetMessagingTimeout(system, AX_MESSAGING_TIMEOUT_SECS);

    let focused = copy_element_attr(system, b"AXFocusedUIElement\0");
    CFRelease(system as CFTypeRef);

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
pub(super) fn read_around_cursor_blocking(budget_chars: usize, gate: GateInputs) -> ReadOutcome {
    unsafe {
        let focused = match focused_element_passing_the_gate(gate) {
            GatedElement::Ready(el) => el,
            GatedElement::Blocked(reason) => return ReadOutcome::Blocked(reason),
            GatedElement::Unavailable(why) => return ReadOutcome::Unavailable(why),
        };
        let outcome = read_document(focused, budget_chars);
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
    let mut range = CFRange::default();
    let ok = AXValueGetValue(
        value as AxValueRef,
        K_AX_VALUE_CF_RANGE_TYPE,
        &mut range as *mut _ as *mut c_void,
    );
    CFRelease(value);
    (ok != 0).then_some(range)
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

pub(super) fn apply_voice_edit_target(
    target: &VoiceEditTarget,
    text: &str,
    insert: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    unsafe {
        let GatedElement::Ready(element) = focused_element_passing_the_gate(voice_edit_gate())
        else {
            return Err("voiceEditTargetUnavailable".into());
        };
        let retained = SendableElement::retained(element);
        CFRelease(element as CFTypeRef);
        let element = retained.as_ref();
        if CFEqual(element as CFTypeRef, target.element.as_ref() as CFTypeRef) == 0 {
            return Err("voiceEditTargetChanged".into());
        }
        if voice_edit_text(element).as_deref() != Some(target.text.as_str())
            || copy_selected_range(element) != Some(target.selection_utf16)
        {
            return Err("voiceEditFieldChanged".into());
        }
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
                let GatedElement::Ready(current) =
                    focused_element_passing_the_gate(voice_edit_gate())
                else {
                    return Err("voiceEditTargetChanged".into());
                };
                let same = CFEqual(current as CFTypeRef, element as CFTypeRef) != 0;
                CFRelease(current as CFTypeRef);
                if !same {
                    return Err("voiceEditTargetChanged".into());
                }
                if voice_edit_text(element).as_deref() != Some(target.text.as_str()) {
                    return Err("voiceEditFieldChanged".into());
                }
                insert(text)
            },
            || {
                let GatedElement::Ready(current) =
                    focused_element_passing_the_gate(voice_edit_gate())
                else {
                    return;
                };
                let same = CFEqual(current as CFTypeRef, element as CFTypeRef) != 0;
                CFRelease(current as CFTypeRef);
                if same
                    && voice_edit_text(element).as_deref() == Some(target.text.as_str())
                    && copy_selected_range(element) == Some(range)
                {
                    let _ = set_voice_edit_selection(element, target.selection_utf16);
                }
            },
        )
    }
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
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return false;
            }
            AXUIElementSetMessagingTimeout(system, AX_MESSAGING_TIMEOUT_SECS);
            let focused = copy_element_attr(system, b"AXFocusedUIElement\0");
            CFRelease(system as CFTypeRef);
            let Some(focused) = focused else { return false };
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
    copy_attr(element, attribute).map(|value| value as AxUiElementRef)
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
    let direct = CFStringGetCStringPtr(s, K_CF_STRING_ENCODING_UTF8);
    if !direct.is_null() {
        return CStr::from_ptr(direct).to_str().ok().map(str::to_string);
    }
    let length = CFStringGetLength(s);
    if length <= 0 {
        return Some(String::new());
    }
    let max_bytes = CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) + 1;
    let mut buf: Vec<u8> = vec![0; max_bytes as usize];
    let ok = CFStringGetCString(
        s,
        buf.as_mut_ptr() as *mut c_char,
        max_bytes,
        K_CF_STRING_ENCODING_UTF8,
    );
    if ok == 0 {
        return None;
    }
    CStr::from_ptr(buf.as_ptr() as *const c_char)
        .to_str()
        .ok()
        .map(str::to_string)
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

/// All state held by the watch thread. The callback receives it via `refcon`.
struct WatchContext {
    element: SendableElement,
    /// Stop flag, the same one [`run_edit_watch_loop`] holds.
    ///
    /// The callback must check it too, not just the loop. When the disarm
    /// signal arrives, the thread may be parked in `CFRunLoop::run_in_mode`
    /// (up to 1s); AX notifications queued in that window still dispatch to
    /// the callback, which the loop's trailing `if !stop.load(..)` cannot
    /// cover. This is the earliest, cheapest defense: a mismatch skips the
    /// cross-process AX full-text read and diff entirely.
    stop: Arc<AtomicBool>,
    /// Comparison baseline: the control's full text **after our insertion
    /// lands**.
    ///
    /// It cannot be fixed at arm time: `inserter.insert()` returning only
    /// means the events were sent; the target app applies them tens to
    /// hundreds of ms later, so a baseline read then is pre-insertion. The
    /// first diff would then be our own inserted text, discarded as a "pure
    /// insertion", and the user's real edit would never be seen. Hence the
    /// baseline anchors only after the typed text lands.
    baseline: std::cell::RefCell<String>,
    /// Whether the baseline has anchored to the post-insertion state.
    anchored: std::cell::Cell<bool>,
    /// Arm time, backing the anchor with a deadline.
    armed_at: Instant,
    /// The text actually typed this time. Only edits landing inside this span
    /// count as "the user changed what we inserted".
    typed_text: String,
    on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
    /// Number of edits reported during this armed window.
    reports: std::cell::Cell<u64>,
    /// Text seen at the previous notification.
    ///
    /// Distinguishes the two notifications — the **primary** criterion for
    /// "has this edit finished":
    ///
    /// - typing / deleting: text changed, caret moved along;
    /// - clicking elsewhere, arrow keys, selecting: text unchanged, caret
    ///   moved.
    ///
    /// "Caret moved but text unchanged" means the user left this spot — the
    /// semantic moment the edit becomes final. This is a semantic signal, not
    /// a timing guess, and it uses `AXSelectedTextChanged`, which is received
    /// anyway.
    last_text: std::cell::RefCell<String>,
    /// Time of the last **text** change. Separates caret events caused by
    /// typing from a real caret move — see [`CARET_MOVE_QUIET`].
    last_value_change: std::cell::Cell<Option<Instant>>,
    /// Start time of a pending unjudged edit; `None` means nothing pending.
    ///
    /// The callback only records; the watch thread judges. Intermediate states
    /// can still change, so nothing is analyzed until the boundary. Callback
    /// and loop share the runloop thread (notifications are dispatched by it),
    /// so `Cell` suffices, no lock needed.
    pending_since: std::cell::Cell<Option<Instant>>,
    /// Number of notifications received during this armed window, logged at
    /// disarm. This one number separates "the observer never worked" (0) from
    /// "notifications arrived but were filtered later" (>0) — without it both
    /// look identical in the logs.
    notifications: std::cell::Cell<u64>,
}

/// `AXValueChanged` callback shim: recovers `WatchContext` from `refcon` and diffs the text.
///
/// # Safety
/// `refcon` must be the `*const WatchContext` passed when `run_edit_watch_loop`
/// registered the observer, valid for the observer's lifetime (owned by the
/// watch thread's stack; unregistration happens before it returns).
unsafe extern "C" fn value_changed_shim(
    _observer: AxObserverRef,
    _element: AxUiElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
) {
    if refcon.is_null() {
        return;
    }
    let ctx = &*(refcon as *const WatchContext);
    ctx.notifications.set(ctx.notifications.get() + 1);
    // Do nothing once disarmed. **This check must precede the AXValue read.**
    //
    // While the thread is parked in `run_in_mode` (up to 1s), AX notifications
    // queued in that window still dispatch here; the loop's trailing
    // `if !stop.load(..)` does not cover the callback path. Without this, a
    // voided watch would re-read the host app's full text cross-process.
    if ctx.stop.load(Ordering::Relaxed) {
        return;
    }
    // Every early return must log. Otherwise "callback never ran" and
    // "callback ran but was filtered" look identical in the logs.
    let Some(current) = copy_string_attr(ctx.element.as_ref(), b"AXValue\0") else {
        log::debug!("[cursor-context] notified but AXValue is unreadable");
        return;
    };
    // Phase 1: wait for our own inserted text to land, then anchor the baseline.
    if !ctx.anchored.get() {
        // Normal path: the typed text appears in the document — insertion
        // landed. Fallback: the target app may transform the text (smart
        // quotes, autocomplete) so `contains` never matches; at the deadline
        // take the current state as-is.
        let inserted = current.contains(&ctx.typed_text);
        if inserted || ctx.armed_at.elapsed() >= BASELINE_ANCHOR_TIMEOUT {
            log::debug!(
                "[cursor-context] baseline anchored at {} chars ({})",
                current.chars().count(),
                if inserted {
                    "insertion landed"
                } else {
                    "timeout"
                }
            );
            // Both must advance together: `baseline` is the diff start,
            // `last_text` is "last seen". Updating only the former would make
            // the first post-anchor notification report the insertion itself
            // as a user edit.
            *ctx.last_text.borrow_mut() = current.clone();
            *ctx.baseline.borrow_mut() = current;
            ctx.anchored.set(true);
        }
        return;
    }

    // Phase 2: separate "typing" from "caret moved away" — record only, analyze at the boundary.
    if *ctx.last_text.borrow() != current {
        // Still editing. Record, do not judge: intermediate states can still change.
        *ctx.last_text.borrow_mut() = current;
        ctx.last_value_change.set(Some(Instant::now()));
        ctx.pending_since.set(Some(Instant::now()));
        return;
    }

    // Text unchanged: either the user moved the caret away (boundary) or this
    // is the paired notification from the last keystroke — the latter must be
    // blocked or every keystroke triggers a verdict.
    if !is_caret_notification(notification) || ctx.pending_since.get().is_none() {
        return;
    }
    let quiet = ctx
        .last_value_change
        .get()
        .is_none_or(|t| t.elapsed() >= CARET_MOVE_QUIET);
    if !quiet {
        return;
    }
    log::debug!("[cursor-context] caret moved away; settling the pending edit");
    settle_pending_edit(ctx, true);
}

/// Whether this notification is `AXSelectedTextChanged` (caret/selection change).
unsafe fn is_caret_notification(notification: CFStringRef) -> bool {
    cfstring_to_rust(notification).as_deref() == Some("AXSelectedTextChanged")
}

/// An edit has settled; diff once and report.
///
/// `force` marks a definite semantic boundary (caret moved away, front app
/// changed, watch ending); otherwise only after [`EDIT_SETTLE_TIMEOUT`] since
/// the last change — the fallback for apps that emit no caret events.
unsafe fn settle_pending_edit(ctx: &WatchContext, force: bool) {
    let Some(since) = ctx.pending_since.get() else {
        return;
    };
    if !force && since.elapsed() < EDIT_SETTLE_TIMEOUT {
        return;
    }
    ctx.pending_since.set(None);

    let Some(current) = copy_string_attr(ctx.element.as_ref(), b"AXValue\0") else {
        return;
    };
    let baseline = ctx.baseline.borrow().clone();
    let Some(edit) = minimal_edit(&baseline, &current) else {
        log::debug!(
            "[cursor-context] settled but no minimal edit (baseline={} chars, current={} chars)",
            baseline.chars().count(),
            current.chars().count()
        );
        return;
    };
    // Core decides whether this edit belongs to the just-inserted text and can
    // become a lexicon suggestion. On rejection the old baseline is kept on
    // purpose: delete-then-type must still merge into one replacement, not
    // split into two unlearnable half-edits. AX owns native watch timing, not
    // lexicon business rules.
    if !(ctx.on_edit)(edit) {
        return;
    }
    *ctx.baseline.borrow_mut() = current;
    ctx.reports.set(ctx.reports.get() + 1);
}

/// Largest document the observer will watch (UTF-16 code units).
///
/// Every notification triggers a full `AXValue` read plus an O(n) diff, up to
/// once per keystroke for the 60s window; past a certain size that becomes
/// "copy the whole document cross-process per keystroke" — lag, even AX
/// timeouts. Same order as [`FULL_TEXT_MAX_UTF16`]: a document too large to
/// read once is not worth watching per key. Beyond it, do not arm at all —
/// missing a word is acceptable, making typing laggy is not.
const EDIT_WATCH_MAX_UTF16: usize = 20_000;

/// Arms the manual edit watch. Returns the stop flag on success, `None` on
/// failure (warn only; never disturbs the main path).
///
/// `typed_text` is what the user actually saw land on screen — under
/// streaming it is the content really typed, not the full LLM output; the two
/// can differ.
///
/// **The focused element and baseline are read on the new thread, not the
/// caller's.** The caller `arm_edit_watch` sits on the async `end_session`
/// path, i.e. a tokio worker, and each of these AX calls can burn
/// [`AX_MESSAGING_TIMEOUT_SECS`] against an AX-unresponsive app — the very
/// case the timeout exists for. The cost is a thread spawn (tens of µs);
/// better than `spawn_blocking`, which queues on the tokio blocking pool and
/// can be later under load.
pub(super) fn spawn_edit_watcher(
    typed_text: String,
    lifetime: std::time::Duration,
    on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
) -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let spawn_result = std::thread::Builder::new()
        .name("openless-cursor-edit-watch".into())
        .spawn(move || {
            let Some((element, baseline, pid)) = grab_focused_element() else {
                return;
            };
            // Backstop. The primary check lives in `grab_focused_element` via
            // `AXNumberOfCharacters`, which rejects before the full copy; this
            // guards against the target app reporting a length inconsistent
            // with AXValue, so the watcher never runs on wrong metadata.
            let baseline_utf16 = baseline.encode_utf16().count();
            if baseline_utf16 > EDIT_WATCH_MAX_UTF16 {
                log::info!(
                    "[cursor-context] edit watch skipped: AXValue is {baseline_utf16} UTF-16 units (limit {EDIT_WATCH_MAX_UTF16})"
                );
                return;
            }
            let (_, bundle_id) = crate::selection::current_front_app_parts();
            let baseline_for_last_text = baseline.clone();
            run_edit_watch_loop(
                WatchContext {
                    element,
                    stop: Arc::clone(&thread_stop),
                    // If the inserted text is already in the document at arm
                    // time, insertion landed and the baseline is usable as-is.
                    anchored: std::cell::Cell::new(baseline.contains(&typed_text)),
                    baseline: std::cell::RefCell::new(baseline),
                    armed_at: Instant::now(),
                    last_text: std::cell::RefCell::new(baseline_for_last_text),
                    last_value_change: std::cell::Cell::new(None),
                    pending_since: std::cell::Cell::new(None),
                    typed_text,
                    on_edit,
                    reports: std::cell::Cell::new(0),
                    notifications: std::cell::Cell::new(0),
                },
                pid,
                bundle_id,
                thread_stop,
                lifetime,
            );
        });

    if let Err(err) = spawn_result {
        log::warn!("[cursor-context] spawn edit watch thread failed: {err}");
        return None;
    }
    Some(stop)
}

/// Grabs the focused element + reads the baseline full text once + gets the
/// pid. **Only called on the watch thread.**
///
/// ## The security gate must run here too
///
/// The observer reads exactly what [`read_around_cursor_blocking`] reads —
/// the focused element's full `AXValue` — just more often (once per
/// notification for the whole window), and the diff may reach logs or become
/// a suggestion card.
///
/// The two paths reach AX **separately**: the read path via
/// `probe_around_cursor`, the watch path via `arm_edit_watch`. When only the
/// former was gated, the latter was a bypass — dictating in a terminal had
/// the context read correctly blocked, yet the watcher still armed and read
/// the terminal's full text. The feature's entire premise is "never read
/// password fields / Secure Input / password managers / terminals"; both
/// paths must answer identically. Hence the same
/// [`focused_element_passing_the_gate`], no separate route.
fn grab_focused_element() -> Option<(SendableElement, String, i32)> {
    let (_, bundle_id) = crate::selection::current_front_app_parts();
    let gate = GateInputs {
        secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
        bundle_id,
        role: None,
        subrole: None,
    };

    unsafe {
        let focused = match focused_element_passing_the_gate(gate) {
            GatedElement::Ready(el) => el,
            GatedElement::Blocked(reason) => {
                log::info!("[cursor-context] edit watch blocked: {reason:?}");
                return None;
            }
            GatedElement::Unavailable(why) => {
                log::info!("[cursor-context] edit watch skipped: {why}");
                return None;
            }
        };

        // Ask the length before deciding on a full copy — same approach as
        // `read_document`. `AXValue` copies the whole document cross-process;
        // on a 100k-character file marshalling alone can hit the timeout, and
        // over-limit documents are not watched anyway (see
        // `EDIT_WATCH_MAX_UTF16`), so the copy would be pure waste.
        match classify_document_length(
            copy_index_attr(focused, b"AXNumberOfCharacters\0"),
            EDIT_WATCH_MAX_UTF16,
        ) {
            DocumentLength::Unknown => {
                log::info!(
                    "[cursor-context] edit watch skipped: AXNumberOfCharacters unavailable; refusing an unbounded AXValue read"
                );
                CFRelease(focused as CFTypeRef);
                return None;
            }
            DocumentLength::OverLimit(total) => {
                log::info!(
                    "[cursor-context] edit watch skipped: document is {total} UTF-16 units (limit {EDIT_WATCH_MAX_UTF16})"
                );
                CFRelease(focused as CFTypeRef);
                return None;
            }
            DocumentLength::WithinLimit(_) => {}
        }

        let baseline = copy_string_attr(focused, b"AXValue\0");
        let mut pid: i32 = 0;
        let pid_err = AXUIElementGetPid(focused, &mut pid);
        let element = SendableElement::retained(focused);
        CFRelease(focused as CFTypeRef);

        let Some(baseline) = baseline else {
            log::info!("[cursor-context] edit watch skipped: focused element has no AXValue");
            return None;
        };
        if pid_err != AX_ERROR_SUCCESS || pid <= 0 {
            log::info!("[cursor-context] edit watch skipped: AXUIElementGetPid failed");
            return None;
        }
        Some((element, baseline, pid))
    }
}

fn run_edit_watch_loop(
    ctx: WatchContext,
    pid: i32,
    bundle_id: Option<String>,
    stop: Arc<AtomicBool>,
    lifetime: std::time::Duration,
) {
    unsafe {
        let mut observer: AxObserverRef = std::ptr::null_mut();
        let err = AXObserverCreate(pid, value_changed_shim, &mut observer);
        if err != AX_ERROR_SUCCESS || observer.is_null() {
            log::warn!("[cursor-context] AXObserverCreate failed: AXError={err}");
            return;
        }
        // Register two notifications, not one.
        //
        // `AXValueChanged` is the standard "text content changed" signal, but
        // not every text control emits it. `AXSelectedTextChanged`
        // (caret/selection moved) is a second evidence path for the same
        // event — editing a word necessarily moves the caret. Either one
        // triggers a text diff costing one AX read; missing one means the
        // whole feature silently dies in that app.
        let mut registered: Vec<(CFStringRef, &str)> = Vec::new();
        for name in [&b"AXValueChanged\0"[..], &b"AXSelectedTextChanged\0"[..]] {
            let Some(notification) = cfstring_from_static(name) else {
                continue;
            };
            // SAFETY: &ctx lives until this function returns, and
            // unregistration happens before the return, so C never sees a
            // dangling pointer.
            let add_err = AXObserverAddNotification(
                observer,
                ctx.element.as_ref(),
                notification,
                &ctx as *const _ as *mut c_void,
            );
            let label = std::str::from_utf8(&name[..name.len() - 1]).unwrap_or("?");
            if add_err == AX_ERROR_SUCCESS {
                registered.push((notification, label));
            } else {
                log::info!(
                    "[cursor-context] {label} not registered: AXError={add_err} (app does not emit it)"
                );
                CFRelease(notification);
            }
        }
        if registered.is_empty() {
            log::info!(
                "[cursor-context] no usable AX notification on this element; edit watch off"
            );
            CFRelease(observer as CFTypeRef);
            return;
        }

        // Use core_foundation's wrapper for the runloop part instead of
        // re-declaring the externs: `hotkey.rs` already declares
        // CFRunLoopGetCurrent / CFRunLoopAddSource, and duplicates trigger
        // clashing_extern_declarations (ABI-compatible, but only by luck).
        let source = CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(observer));
        let runloop = CFRunLoop::get_current();
        // SAFETY: kCFRunLoopDefaultMode is a CoreFoundation 'static constant string.
        let mode = kCFRunLoopDefaultMode;
        runloop.add_source(&source, mode);
        log::info!(
            "[cursor-context] edit watch armed (pid={pid} bundle={bundle_id:?} notifications=[{}])",
            registered
                .iter()
                .map(|(_, l)| *l)
                .collect::<Vec<_>>()
                .join(", ")
        );

        let started = Instant::now();
        let mut end_reason = "disarmed";
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Stop when the configured observation window expires.
            if started.elapsed() >= lifetime {
                end_reason = "timeout";
                break;
            }
            // Stop as soon as the front app changes — watching someone else's
            // window is both pointless and improper.
            let (_, current_bundle) = crate::selection::current_front_app_parts();
            if current_bundle != bundle_id {
                end_reason = "front app changed";
                break;
            }
            let result = CFRunLoop::run_in_mode(mode, Duration::from_secs(1), false);
            // The disarm signal may arrive exactly within this 1s. Check
            // before judging — otherwise an edit from the previous round gets
            // reported (see the long comment at the loop's exit).
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // Ask once per rotation whether typing has been quiet long enough.
            // Judging happens here, not in the callback.
            settle_pending_edit(&ctx, false);
            // Finished means the runloop has no input source — the observer's
            // source is installed, so this normally cannot happen; if it does,
            // the focused element is gone. Stop.
            if matches!(result, CFRunLoopRunResult::Finished) {
                end_reason = "focused element gone";
                break;
            }
        }

        // Final settle before exit: if the user finished editing and
        // immediately switched apps, the loop ended before the quiet timer
        // fired; that edit should not be lost.
        //
        // **But not when actively disarmed.** `stop` is set only by a new
        // dictation session (`begin_session_as`) or the user turning the
        // switch off (`disarm_edit_watch`); in both cases the coordinator has
        // already dismissed the suggestion card, and a late report would pop
        // a card **mid new session**, shrinking the capsule window to the
        // card's size and erasing the capsule of the dictation in progress
        // (seen once on hardware as "the hotkey seems broken").
        //
        // Only natural ends (timeout / app switch / focus element gone)
        // settle — no new session is running then, and the user's edit truly
        // was never judged.
        if !stop.load(Ordering::Relaxed) {
            settle_pending_edit(&ctx, true);
        }

        // Whatever the exit path, this unregistration must run.
        runloop.remove_source(&source, mode);
        for (notification, label) in registered {
            let remove_err =
                AXObserverRemoveNotification(observer, ctx.element.as_ref(), notification);
            if remove_err != AX_ERROR_SUCCESS {
                // -25202 = notification not registered, usually because the
                // target app destroyed and recreated the element (Electron
                // does this on every input) — which also explains why the
                // notifications stopped arriving.
                log::warn!(
                    "[cursor-context] remove {label} failed: AXError={remove_err} (element gone?)"
                );
            }
            CFRelease(notification);
        }
        CFRelease(observer as CFTypeRef);
        log::info!(
            "[cursor-context] edit watch disarmed after {}ms ({end_reason}, {} notifications, {} edits)",
            started.elapsed().as_millis(),
            ctx.notifications.get(),
            ctx.reports.get()
        );
        // ctx drops here — the observer is removed and C no longer calls back; safe.
        drop(ctx);
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
