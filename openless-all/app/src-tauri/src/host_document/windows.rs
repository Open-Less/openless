//! Short-lived UIA observation. All registration and COM objects belong to one
//! MTA worker; callbacks only mark text dirty and never read document contents.
use super::EditPair;
use openless_core::host_document::ObservedInsertion;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, OnceLock,
};
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, Result, PWSTR, VARIANT};
use windows::Win32::{
    Foundation::{CloseHandle, BOOL, HWND},
    System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::{Accessibility::*, WindowsAndMessaging::GetForegroundWindow},
};

type Callback = Box<dyn Fn(EditPair) -> bool + Send + Sync>;

/// The paste command itself remains authoritative on failures. UIA may only
/// promote PasteSent after observing a change in the very same editor. Failure
/// to read the host never retries, suppresses, or changes the actual paste.
pub(crate) fn insert_with_delivery_check(
    text: &str,
    consent: impl Fn() -> bool,
    insert: impl FnOnce() -> crate::types::InsertStatus,
) -> crate::types::InsertStatus {
    use crate::types::InsertStatus;
    unsafe {
        if !consent() || CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return insert();
        }
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = ComGuard;
        let snapshot = (|| -> Result<_> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            let timeouts: IUIAutomation2 = uia.cast()?;
            timeouts.SetConnectionTimeout(200)?;
            timeouts.SetTransactionTimeout(200)?;
            let window = GetForegroundWindow();
            let element = uia.GetFocusedElement()?;
            if !consent() || element.CurrentIsPassword()?.as_bool() || !allowed_process(&element)? {
                return Err(windows::core::Error::from_win32());
            }
            let before = read_text(&element)?;
            Ok((uia, element, window, before))
        })()
        .ok();
        let status = insert();
        if status != InsertStatus::PasteSent {
            return status;
        }
        let Some((uia, element, window, before)) = snapshot else {
            return status;
        };
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1) && consent() {
            let verified = (|| -> Result<bool> {
                if GetForegroundWindow() != window
                    || !uia
                        .CompareElements(&element, &uia.GetFocusedElement()?)?
                        .as_bool()
                {
                    return Err(windows::core::Error::from_win32());
                }
                if !consent() {
                    return Ok(false);
                }
                let after = read_text(&element)?;
                Ok(ObservedInsertion::delivered(&before, &after, text))
            })();
            match verified {
                Ok(true) => return InsertStatus::Inserted,
                Err(_) => break,
                Ok(false) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        status
    }
}
struct Request {
    text: String,
    window: isize,
    lifetime: Duration,
    stop: Arc<AtomicBool>,
    callback: Callback,
}

#[implement(IUIAutomationEventHandler, IUIAutomationPropertyChangedEventHandler)]
struct Changed {
    dirty: Arc<AtomicBool>,
}
impl IUIAutomationEventHandler_Impl for Changed_Impl {
    fn HandleAutomationEvent(
        &self,
        _: Option<&IUIAutomationElement>,
        _: UIA_EVENT_ID,
    ) -> Result<()> {
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }
}
impl IUIAutomationPropertyChangedEventHandler_Impl for Changed_Impl {
    fn HandlePropertyChangedEvent(
        &self,
        _: Option<&IUIAutomationElement>,
        _: UIA_PROPERTY_ID,
        _: &VARIANT,
    ) -> Result<()> {
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }
}

pub(super) fn spawn_edit_watcher(
    text: String,
    lifetime: Duration,
    callback: Callback,
) -> Option<Arc<AtomicBool>> {
    if text.trim().is_empty() {
        return None;
    }
    static WORKER: OnceLock<Option<mpsc::Sender<Request>>> = OnceLock::new();
    let worker = WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Request>();
            std::thread::Builder::new()
                .name("vocab-uia".into())
                .spawn(move || unsafe {
                    if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                        return;
                    }
                    while let Ok(request) = rx.recv() {
                        if !request.stop.load(Ordering::Acquire) {
                            let _ = observe(&request);
                        }
                    }
                    CoUninitialize();
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()?;
    let stop = Arc::new(AtomicBool::new(false));
    worker
        .send(Request {
            text,
            lifetime,
            window: unsafe { GetForegroundWindow().0 as isize },
            stop: stop.clone(),
            callback,
        })
        .ok()?;
    Some(stop)
}

/// Sensitive process name fragments (password managers, terminals). Matched against the
/// lowercased executable filename only (not the full path), substring match.
///
/// Pure function — no COM, so it is unit-testable without a live UIA element. The live lookup
/// is [`allowed_process`], shared by the edit watcher and the cursor-context reader so both
/// paths answer identically (see `openless-core`'s equivalent `SENSITIVE_BUNDLE_PREFIXES` note
/// on macOS: one ungated path means no gate).
const BLOCKED_PROCESS_NAME_FRAGMENTS: &[&str] = &[
    "keepass",
    "1password",
    "bitwarden",
    "lastpass",
    "dashlane",
    "windowsterminal",
    "powershell",
    "pwsh",
    "cmd.exe",
    "conhost",
    "mintty",
    "wezterm",
    "alacritty",
    "putty",
];

fn is_blocked_process_name(executable_name: &str) -> bool {
    let lowered = executable_name.to_lowercase();
    BLOCKED_PROCESS_NAME_FRAGMENTS
        .iter()
        .any(|blocked| lowered.contains(blocked))
}

unsafe fn allowed_process(element: &IUIAutomationElement) -> Result<bool> {
    let pid = element.CurrentProcessId()? as u32;
    if pid == std::process::id() {
        return Ok(false);
    }
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?;
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_WIN32,
        PWSTR(buffer.as_mut_ptr()),
        &mut len,
    );
    let _ = CloseHandle(process);
    result?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    Ok(!is_blocked_process_name(name))
}

unsafe fn read_text(element: &IUIAutomationElement) -> Result<String> {
    if element.CurrentIsPassword()?.as_bool() {
        return Err(windows::core::Error::from_win32());
    }
    let text = if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
    {
        pattern
            .DocumentRange()?
            .GetText((ObservedInsertion::MAX_DOCUMENT_UTF16 + 1) as i32)?
            .to_string()
    } else {
        element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)?
            .CurrentValue()?
            .to_string()
    };
    if text.encode_utf16().count() > ObservedInsertion::MAX_DOCUMENT_UTF16 {
        return Err(windows::core::Error::from_win32());
    }
    Ok(text)
}

unsafe fn observe(request: &Request) -> Result<()> {
    let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
    let timeouts: IUIAutomation2 = uia.cast()?;
    timeouts.SetConnectionTimeout(200)?;
    timeouts.SetTransactionTimeout(200)?;
    let element = uia.GetFocusedElement()?;
    if element.CurrentIsPassword()?.as_bool() || !allowed_process(&element)? {
        return Ok(());
    }
    let active = || -> Result<bool> {
        Ok(!request.stop.load(Ordering::Acquire)
            && GetForegroundWindow() == HWND(request.window as *mut _)
            && !element.CurrentIsPassword()?.as_bool()
            && uia
                .CompareElements(&element, &uia.GetFocusedElement()?)?
                .as_bool())
    };
    let started = Instant::now();
    let mut anchor = loop {
        if !active()? || started.elapsed() >= Duration::from_secs(1) {
            return Ok(());
        }
        if let Some(anchor) = ObservedInsertion::new(read_text(&element)?, &request.text) {
            break anchor;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let dirty = Arc::new(AtomicBool::new(true));
    let handler: IUIAutomationEventHandler = Changed {
        dirty: dirty.clone(),
    }
    .into();
    let property: IUIAutomationPropertyChangedEventHandler = handler.cast()?;
    let text_registered = uia
        .AddAutomationEventHandler(
            UIA_Text_TextChangedEventId,
            &element,
            TreeScope_Element,
            None,
            &handler,
        )
        .is_ok();
    let value_registered = uia
        .AddPropertyChangedEventHandlerNativeArray(
            &element,
            TreeScope_Element,
            None,
            &property,
            &[UIA_ValueValuePropertyId],
        )
        .is_ok();
    if !text_registered && !value_registered {
        return Ok(());
    }
    // Always unregister, including provider errors and opt-out. Late callbacks
    // only retain their own dirty flag; no host text or Core sink is accessible.
    let result = (|| -> Result<()> {
        let mut changed_at = Some(Instant::now());
        while started.elapsed() < request.lifetime && active()? {
            if dirty.swap(false, Ordering::AcqRel) {
                changed_at = Some(Instant::now());
            }
            if changed_at.is_some_and(|at| at.elapsed() >= Duration::from_millis(700)) {
                changed_at = None;
                let current = read_text(&element)?;
                if !active()?
                    || !anchor.observe(&current, |edit| {
                        !request.stop.load(Ordering::Acquire) && (request.callback)(edit)
                    })
                {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    if text_registered {
        let _ = uia.RemoveAutomationEventHandler(UIA_Text_TextChangedEventId, &element, &handler);
    }
    if value_registered {
        let _ = uia.RemovePropertyChangedEventHandler(&element, &property);
    }
    result
}

// ═══════════════════════════════════════════════════════════════════════════
// Cursor context reader
// ═══════════════════════════════════════════════════════════════════════════
//
// A separate, read-only path from the edit watcher above: `read_text()` / `observe()`
// serve insertion-delivery checks and local edit learning (full document, never leaves the
// device). This path serves LLM cursor context — it only ever fetches a bounded span around
// the caret, and the result may be sent to the configured LLM provider, so it goes through
// its own safety gate ([`allowed_process`] + `CurrentIsPassword`) before a single UIA text
// call is made. See `windows_cursor_context开发方案.md` §4.

/// Outcome of locating a usable caret [`IUIAutomationTextRange`].
enum CaretLookup {
    /// A zero-length (collapsed) range at the insertion point, tagged with which pattern
    /// produced it (diagnostic only — never logged with document content).
    Found(IUIAutomationTextRange, &'static str),
    /// `TextPattern::GetSelection()` fallback found a real (non-collapsed) selection. Per
    /// §8 of the design doc, the first version never guesses which end is the caret —
    /// "selection rewrite" is a separate future feature, not folded into dictation context.
    NonCollapsedSelection,
    Unavailable(&'static str),
}

/// Finds the caret as a zero-length text range. **Only callable in a `spawn_blocking`
/// context** (every call here is a synchronous COM round-trip).
///
/// Priority, matching §5/§8 of the design doc:
/// 1. `IUIAutomationTextPattern2::GetCaretRange()`, only when `isActive == TRUE` — an inactive
///    caret is unusable and short-circuits to `Unavailable` without trying the fallback (an
///    inactive caret on this element is unlikely to become valid by asking a different pattern).
/// 2. `IUIAutomationTextPattern::GetSelection()`, used only when it is exactly one collapsed
///    range — this is the compatibility path for controls without `TextPattern2`.
/// 3. Neither pattern exists (including controls that only expose `ValuePattern`, see §9):
///    `Unavailable`. **Never** derived from `ValuePattern::CurrentValue()` — that pattern
///    cannot report a caret offset, and guessing one (e.g. end-of-value) would silently inject
///    wrong context when the user is editing mid-document.
unsafe fn find_caret_range(element: &IUIAutomationElement) -> CaretLookup {
    if let Ok(pattern2) = element.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
    {
        let mut is_active = BOOL(0);
        match pattern2.GetCaretRange(&mut is_active) {
            Ok(range) if is_active.as_bool() => {
                return CaretLookup::Found(range, "textPattern2")
            }
            Ok(_) => return CaretLookup::Unavailable("caret is not active"),
            // GetCaretRange itself failed even though the pattern exists; fall through and
            // try the TextPattern/GetSelection compatibility path below.
            Err(_) => {}
        }
    }

    let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
    else {
        let has_value_only = element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .is_ok();
        return CaretLookup::Unavailable(if has_value_only {
            "focused control exposes value but no caret text range"
        } else {
            "no caret-capable text pattern on focused element"
        });
    };
    let Ok(selection) = pattern.GetSelection() else {
        return CaretLookup::Unavailable("GetSelection failed");
    };
    let Ok(count) = selection.Length() else {
        return CaretLookup::Unavailable("selection length unavailable");
    };
    if count != 1 {
        // 0 => no selection/caret reported at all; >1 => discontiguous selection. Neither is
        // "a caret"; stay conservative rather than picking one range heuristically.
        return CaretLookup::Unavailable("no single caret-equivalent selection range");
    }
    let Ok(range) = selection.GetElement(0) else {
        return CaretLookup::Unavailable("selection range unavailable");
    };
    match range.CompareEndpoints(
        TextPatternRangeEndpoint_Start,
        &range,
        TextPatternRangeEndpoint_End,
    ) {
        Ok(0) => CaretLookup::Found(range, "textPattern"),
        Ok(_) => CaretLookup::NonCollapsedSelection,
        Err(_) => CaretLookup::Unavailable("selection endpoint comparison failed"),
    }
}

/// Reads the document window around the caret. **Only callable in a `spawn_blocking`
/// context.** Never reads the full document (§7 of the design doc): the range is expanded
/// from the caret by `MoveEndpointByUnit`, not sliced out of `DocumentRange()`.
unsafe fn read_document(element: &IUIAutomationElement, budget_chars: usize) -> super::ReadOutcome {
    let (caret, source) = match find_caret_range(element) {
        CaretLookup::Found(range, source) => (range, source),
        CaretLookup::NonCollapsedSelection => {
            return super::ReadOutcome::Unavailable("non-collapsed selection")
        }
        CaretLookup::Unavailable(reason) => return super::ReadOutcome::Unavailable(reason),
    };

    // Over-fetch relative to the final budget: `TextUnit_Character` is defined by the text
    // provider, which for some controls counts UTF-16 code units rather than Unicode scalars
    // (surrogate pairs, e.g. emoji, would then move fewer "characters" than Rust chars). Core's
    // `window_around_cursor` below does the exact, char-precise 80/20 split and redistribution
    // (design doc §6); this local fetch only needs to be generous enough to feed it, not exact.
    let over_fetch = (budget_chars.saturating_mul(2)).max(1) as i32;
    let before_want = over_fetch * 4 / 5;
    let after_want = over_fetch - before_want;
    // Defensive cap on the cross-process text copy; the range itself is already bounded by the
    // Move calls above, this only guards against a provider returning more than requested.
    let text_cap = over_fetch.saturating_add(64);

    let Ok(before_range) = caret.Clone() else {
        return super::ReadOutcome::Unavailable("caret range clone failed");
    };
    let before_moved = before_range
        .MoveEndpointByUnit(
            TextPatternRangeEndpoint_Start,
            TextUnit_Character,
            -before_want,
        )
        .unwrap_or(0);
    let Ok(before_text) = before_range.GetText(text_cap) else {
        return super::ReadOutcome::Unavailable("caret range text unavailable");
    };

    let Ok(after_range) = caret.Clone() else {
        return super::ReadOutcome::Unavailable("caret range clone failed");
    };
    let after_moved = after_range
        .MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, after_want)
        .unwrap_or(0);
    let Ok(after_text) = after_range.GetText(text_cap) else {
        return super::ReadOutcome::Unavailable("caret range text unavailable");
    };

    let before_text = before_text.to_string();
    let after_text = after_text.to_string();
    // Diagnostic only: how far the provider actually let each endpoint move vs. what was
    // requested, and how many chars that text turned out to hold. No document content.
    // `moved` vs the resulting char count diverging a lot (e.g. moved=-1200 but
    // before_text has only a handful of chars) points at a provider whose "character" unit
    // or caret tracking doesn't match the visible cursor (seen on Chromium-based UIA: VS
    // Code, Chrome) rather than a bug in this redistribution math.
    log::info!(
        "[cursor-context] source={source} before_want={before_want} before_moved={before_moved} before_chars={} after_want={after_want} after_moved={after_moved} after_chars={}",
        before_text.chars().count(),
        after_text.chars().count(),
    );

    let mut text = before_text;
    let cursor = text.chars().count();
    text.push_str(&after_text);

    super::ReadOutcome::Window(super::window_around_cursor(&text, cursor, budget_chars))
}

/// Synchronously reads the document around the cursor. **Only callable in a `spawn_blocking`
/// context** — see [`super::windows_probe`] for the async/timeout wrapper.
///
/// Gate order follows §15-17 of the design doc: password field and process blocklist are
/// checked immediately after acquiring the focused element, before any text pattern is
/// touched. Focus consistency (§12) is re-checked right before returning a successful window,
/// so a focus change mid-read (Alt+Tab, OpenLess's own UI stealing focus, …) discards the
/// result instead of mixing one app's text with another's dictation.
pub(super) fn read_around_cursor_blocking(budget_chars: usize) -> super::ReadOutcome {
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return super::ReadOutcome::Unavailable("COM initialization failed");
        }
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = ComGuard;

        let outcome = (|| -> Result<super::ReadOutcome> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            let timeouts: IUIAutomation2 = uia.cast()?;
            timeouts.SetConnectionTimeout(200)?;
            timeouts.SetTransactionTimeout(200)?;

            let initial_window = GetForegroundWindow();
            let element = uia.GetFocusedElement()?;

            if element.CurrentIsPassword()?.as_bool() {
                return Ok(super::ReadOutcome::Blocked(super::BlockReason::SecureTextField));
            }
            if !allowed_process(&element)? {
                return Ok(super::ReadOutcome::Blocked(super::BlockReason::BlockedApp));
            }

            let outcome = read_document(&element, budget_chars);
            if !matches!(outcome, super::ReadOutcome::Window(_)) {
                return Ok(outcome);
            }

            // Focus must not have moved during the read above — otherwise this window's "before"
            // text could belong to a different app than the dictation that is about to use it.
            if GetForegroundWindow() != initial_window
                || !uia
                    .CompareElements(&element, &uia.GetFocusedElement()?)?
                    .as_bool()
            {
                return Ok(super::ReadOutcome::Unavailable("focus changed during capture"));
            }

            Ok(outcome)
        })();

        outcome.unwrap_or(super::ReadOutcome::Unavailable("UIA call failed"))
    }
}

#[cfg(test)]
mod cursor_context_tests {
    use super::*;

    #[test]
    fn password_managers_and_terminals_are_blocked_by_name() {
        for name in [
            "KeePassXC.exe",
            "1Password.exe",
            "Bitwarden.exe",
            "powershell.exe",
            "WindowsTerminal.exe",
            "cmd.exe",
            "conhost.exe",
        ] {
            assert!(is_blocked_process_name(name), "{name} should be blocked");
        }
    }

    #[test]
    fn ordinary_editors_are_not_blocked_by_name() {
        for name in ["notepad.exe", "WINWORD.EXE", "Code.exe", "chrome.exe"] {
            assert!(!is_blocked_process_name(name), "{name} should not be blocked");
        }
    }

    #[test]
    fn a_name_that_merely_contains_a_blocked_fragment_is_still_blocked() {
        // Substring match is intentional here (unlike the macOS bundle-id prefix match): Windows
        // executable names have no reverse-DNS namespace to anchor a prefix check against, and
        // helper/updater binaries commonly append suffixes to the vendor name.
        assert!(is_blocked_process_name("1password-updater.exe"));
    }
}
