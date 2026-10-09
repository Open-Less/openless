//! Host app document reading — the only place that touches "what the user is writing".
//!
//! Goal: let LLM polishing know what the user is writing. Chinese homophones are
//! indistinguishable to the acoustic model but clear from context; this information is
//! entirely missing from OpenLess today.
//!
//! ## Boundary
//!
//! Cursor context is available on macOS. Edit learning has separate local consent:
//! macOS AX, Windows UIA and Android accessibility provide bounded observation.
//! Linux retains explicit vocabulary entry.
//!
//! ## Three hard constraints (new code must not violate these, even though the old AX
//! code in this repo does)
//!
//! 1. **AX calls must have a timeout**. Without `AXUIElementSetMessagingTimeout` they
//!    inherit the ~6s default — a hung app means a 6-second freeze. The existing AX code
//!    in `selection.rs` / `lib.rs` doesn't set it; that's a defect, don't copy it.
//! 2. **Never call AX synchronously on a tokio worker**. Use `spawn_blocking` +
//!    `tokio::time::timeout` as double protection (shaped after the native call boundary
//!    in `windows_ime_ipc.rs`). The inner timeout protects the thread itself; the outer
//!    one guarantees the async caller returns on time regardless.
//! 3. **Pass the safety gate before reading text**. Reject password fields, Secure Input,
//!    known password managers and terminals. Cursor context may enter authorized LLM
//!    requests; edit observation remains local.
//!
//! ## Scope of this milestone
//!
//! Document context reading is invoked from explicit context entry points.
//! `KeyboardDelivery` additionally serves macOS streaming input completion timing: it
//! reuses the same focus/permission gates and reads only the selection range, never the
//! host document body.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(crate) use windows::insert_with_delivery_check;

#[cfg(target_os = "macos")]
pub(crate) use macos::{KeyboardDelivery, KeyboardDeliveryOutcome};

// `minimal_edit` is currently used only by the macOS observation callback; no consumer on non-macOS builds.
#[allow(unused_imports)]
pub use openless_core::host_document::{
    edit_is_within_typed_text, is_vocab_worthy, learned_rule, minimal_edit, plan_window,
    utf16_offset_to_char_offset, window_around_cursor, DocumentWindow, EditPair, LearnedRule,
    WindowSpan,
};

use serde::Serialize;

/// Default context budget (chars) sent to the LLM. Covers a paragraph or two without
/// significantly inflating the prompt.
pub const DEFAULT_BUDGET_CHARS: usize = 600;

/// The original control and its complete text, captured before the edit panel takes focus.
#[derive(Debug, Clone)]
pub(crate) struct NativeVoiceEditTarget {
    insertion_target: crate::selection::SelectionInsertionTarget,
    #[cfg(target_os = "windows")]
    native: windows::VoiceEditTarget,
    #[cfg(target_os = "macos")]
    native: macos::VoiceEditTarget,
}

pub(crate) fn capture_voice_edit_target() -> Result<
    (
        String,
        Option<openless_core::TextSelection>,
        NativeVoiceEditTarget,
    ),
    String,
> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let insertion_target = crate::selection::capture_selection_insertion_target();
        if !crate::selection::selection_insertion_target_is_captured(&insertion_target) {
            return Err("voiceEditTargetUnavailable".into());
        }
        #[cfg(target_os = "windows")]
        let (text, selection, native) = windows::capture_voice_edit_target()?;
        #[cfg(target_os = "macos")]
        let (text, selection, native) = macos::capture_voice_edit_target()?;
        Ok((
            text,
            selection,
            NativeVoiceEditTarget {
                insertion_target,
                native,
            },
        ))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("voiceEditTargetUnavailable".into())
    }
}

/// Restore the original app, compare the exact control, full text and selection, then write.
/// Call only on the blocking native insertion thread.
pub(crate) fn apply_voice_edit_target(
    target: &NativeVoiceEditTarget,
    text: &str,
    insert: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        if !crate::selection::reactivate_selection_insertion_target(&target.insertion_target) {
            return Err("voiceEditTargetChanged".into());
        }
        #[cfg(target_os = "windows")]
        return windows::apply_voice_edit_target(&target.native, text, insert);
        #[cfg(target_os = "macos")]
        return macos::apply_voice_edit_target(&target.native, text, insert);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (target, text, insert);
        Err("voiceEditTargetUnavailable".into())
    }
}

/// Restore only the selection we changed when a native write fails. Platform
/// callbacks must revalidate the original control and unchanged text first.
#[cfg(any(test, target_os = "windows", target_os = "macos"))]
fn write_with_selection_recovery(
    write: impl FnOnce() -> Result<(), String>,
    recover: impl FnOnce(),
) -> Result<(), String> {
    let result = write();
    if result.is_err() {
        recover();
    }
    result
}

/// Timeout for a single AX message. 200ms is far above a normal AX round-trip (single-digit
/// ms); it only catches hung apps.
#[cfg(target_os = "macos")]
const AX_MESSAGING_TIMEOUT_SECS: f32 = 0.2;

/// Hard upper bound on the async side for a whole read (several AX round-trips).
///
/// Deliberately larger than `AX_MESSAGING_TIMEOUT_SECS`: one read sends 5-6 AX messages,
/// each capped at 200ms. The timeout only stops the caller from waiting; the blocking thread
/// winds down on its own AX timeout.
#[cfg(target_os = "macos")]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1200);

/// Outcome of one read. Every variant beyond `Ok` must explain why nothing was read —
/// during installation verification this distinguishes "blocked" from "AX unsupported".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostDocumentStatus {
    /// Read successfully.
    Ok,
    /// Blocked by the safety gate; not a single AX call was sent.
    Blocked,
    /// Not implemented on this platform. (Unconstructible on macOS builds, hence the explicit allow.)
    #[allow(dead_code)]
    Unsupported,
    /// AX reachable but no document available (no focus / control lacks text attributes / missing permission).
    Unavailable,
    /// No response within [`READ_TIMEOUT`] — the target app is probably hung.
    Timeout,
}

/// Reason for a hard block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// macOS Secure Event Input is on (password fields, sudo prompts, etc.).
    SecureInput,
    /// Focused control's AXRole/AXSubrole is `AXSecureTextField`.
    SecureTextField,
    /// Foreground app is on the hardcoded blocklist (password managers / keychain / terminals).
    BlockedApp,
}

impl BlockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockReason::SecureInput => "secure_input",
            BlockReason::SecureTextField => "secure_text_field",
            BlockReason::BlockedApp => "blocked_app",
        }
    }
}

/// Full result of one read; the debug command serializes it straight to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDocumentReadResult {
    pub status: HostDocumentStatus,
    /// Machine-readable detail: `BlockReason::as_str()` or the unavailability reason.
    pub reason: Option<String>,
    pub window: Option<DocumentWindow>,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub elapsed_ms: u64,
}

impl HostDocumentReadResult {
    fn new(status: HostDocumentStatus, reason: Option<String>) -> Self {
        Self {
            status,
            reason,
            window: None,
            app_name: None,
            bundle_id: None,
            elapsed_ms: 0,
        }
    }
}

/// Inputs to the safety gate. Modeled as a plain data structure so the decision logic is
/// unit-testable without AX — a wrong gate decision means sending passwords to the LLM, so
/// this path must have test coverage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateInputs {
    /// Result of `unicode_keystroke::is_secure_input_enabled()`.
    pub secure_input: bool,
    /// Foreground app bundle id (macOS).
    pub bundle_id: Option<String>,
    /// Focused element's `AXRole`.
    pub role: Option<String>,
    /// Focused element's `AXSubrole`.
    pub subrole: Option<String>,
}

/// AX role/subrole value denoting a secure text field.
const AX_SECURE_TEXT_FIELD: &str = "axsecuretextfield";

/// Sensitive apps that are never read (bundle id prefixes, lowercase comparison).
///
/// No UI for this — a blocklist UI creates the illusion "configure it once and you're safe",
/// while the real defense is read-off-by-default plus this hardcoded list. The list covers only
/// the two categories whose content is almost certainly sensitive:
///
/// - **Password managers / keychain**: the document body is the credential itself.
/// Prefix matching, so `com.1password` covers both `com.1password.1password` and its helper
/// processes.
const SENSITIVE_BUNDLE_PREFIXES: &[&str] = &[
    "com.1password",
    "com.agilebits.onepassword",
    "com.apple.keychainaccess",
    "com.bitwarden",
    "com.lastpass",
    "com.dashlane",
    "org.keepassxc",
    "com.kueh.keepassium",
    "in.sinew.enpass",
    "com.sinew.enpass",
    "com.apple.passwords",
];

/// Terminal app bundle id prefixes. Besides forbidding scrollback reads, used for the macOS
/// auto-wrap-mode decision: known terminals send U+000A, other apps conservatively send
/// Shift+Return.
const TERMINAL_BUNDLE_PREFIXES: &[&str] = &[
    "com.apple.terminal",
    "com.googlecode.iterm2",
    "dev.warp.warp",
    "com.github.wez.wezterm",
    "io.alacritty",
    "org.alacritty",
    "net.kovidgoyal.kitty",
    "co.zeit.hyper",
    "org.tabby",
    "com.tabby",
    "com.mitchellh.ghostty",
];

fn bundle_id_starts_with_any(bundle_id: &str, prefixes: &[&str]) -> bool {
    let lowered = bundle_id.to_ascii_lowercase();
    prefixes.iter().any(|prefix| lowered.starts_with(prefix))
}

pub(crate) fn is_terminal_bundle_id(bundle_id: &str) -> bool {
    bundle_id_starts_with_any(bundle_id, TERMINAL_BUNDLE_PREFIXES)
}

/// Gate decision. Returns `Some(reason)` to block, `None` to allow.
///
/// Checks are ordered by cost: Secure Input and bundle prefixes need no AX and are decided
/// first; role/subrole needs one AX read and is checked last.
pub fn evaluate_gate(inputs: &GateInputs) -> Option<BlockReason> {
    if inputs.secure_input {
        return Some(BlockReason::SecureInput);
    }
    if let Some(bundle) = inputs.bundle_id.as_deref() {
        if bundle_id_starts_with_any(bundle, SENSITIVE_BUNDLE_PREFIXES)
            || is_terminal_bundle_id(bundle)
        {
            return Some(BlockReason::BlockedApp);
        }
    }
    let is_secure_field = |value: &Option<String>| {
        value
            .as_deref()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case(AX_SECURE_TEXT_FIELD))
    };
    if is_secure_field(&inputs.role) || is_secure_field(&inputs.subrole) {
        return Some(BlockReason::SecureTextField);
    }
    None
}

/// Intermediate result returned by platform implementations to [`probe_around_cursor`].
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum ReadOutcome {
    Window(DocumentWindow),
    Blocked(BlockReason),
    /// Carries a static reason so logs and the debug command can tell "no focus" from "unsupported".
    Unavailable(&'static str),
}

/// Read the context around the cursor; any failure degrades to `None`, never an error.
///
/// This is the production entry point (from milestone 2 on). To learn why nothing was read,
/// use [`probe_around_cursor`].
pub async fn read_around_cursor(budget_chars: usize) -> Option<DocumentWindow> {
    probe_around_cursor(budget_chars).await.window
}

/// Read with diagnostics. Used by the debug command; during installation verification
/// `status` / `reason` show the real per-app coverage.
pub async fn probe_around_cursor(budget_chars: usize) -> HostDocumentReadResult {
    #[cfg(target_os = "macos")]
    {
        macos_probe(budget_chars).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = budget_chars;
        HostDocumentReadResult::new(
            HostDocumentStatus::Unsupported,
            Some("cursor context is macOS-only for now".to_string()),
        )
    }
}

#[cfg(target_os = "macos")]
async fn macos_probe(budget_chars: usize) -> HostDocumentReadResult {
    let started = std::time::Instant::now();
    let (app_name, bundle_id) = crate::selection::current_front_app_parts();

    let finish = |mut result: HostDocumentReadResult| {
        result.app_name = app_name.clone();
        result.bundle_id = bundle_id.clone();
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        result
    };

    // First gate: decide the AX-free checks first; on a hit, not a single AX message is sent.
    let gate = GateInputs {
        secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
        bundle_id: bundle_id.clone(),
        role: None,
        subrole: None,
    };
    if let Some(reason) = evaluate_gate(&gate) {
        return finish(blocked_result(reason));
    }

    // AX is a synchronous blocking API: it must leave the tokio worker, or one hung app stalls the whole runtime.
    let handle =
        tokio::task::spawn_blocking(move || macos::read_around_cursor_blocking(budget_chars, gate));

    match tokio::time::timeout(READ_TIMEOUT, handle).await {
        Ok(Ok(ReadOutcome::Window(window))) => finish(HostDocumentReadResult {
            window: Some(window),
            ..HostDocumentReadResult::new(HostDocumentStatus::Ok, None)
        }),
        Ok(Ok(ReadOutcome::Blocked(reason))) => finish(blocked_result(reason)),
        Ok(Ok(ReadOutcome::Unavailable(reason))) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(reason.to_string()),
        )),
        Ok(Err(join_error)) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(format!("blocking task failed: {join_error}")),
        )),
        Err(_) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Timeout,
            Some(format!("no response within {}ms", READ_TIMEOUT.as_millis())),
        )),
    }
}

#[cfg(target_os = "macos")]
fn blocked_result(reason: BlockReason) -> HostDocumentReadResult {
    HostDocumentReadResult::new(
        HostDocumentStatus::Blocked,
        Some(reason.as_str().to_string()),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Edit watcher
// ═══════════════════════════════════════════════════════════════════════════

/// An armed edit watch. **Drop disarms** — makes "forgot to disarm" unrepresentable at the type level.
///
/// An observer leak isn't just a resource issue: it means we keep holding another app's AX
/// references and keep being woken by each of its keystrokes. Besides the RAII here, the
/// observer thread itself has two more safeguards: a 60s hard timeout and self-termination
/// when the foreground app changes.
pub struct EditWatcher {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "android")]
    generation: u64,
}

impl EditWatcher {
    /// Disarm explicitly. Idempotent; also called automatically on drop.
    pub fn disarm(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        #[cfg(target_os = "android")]
        crate::android::edit_observation::disarm(self.generation);
    }
}

impl Drop for EditWatcher {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// Arm a watch for the user modifying the text we just inserted.
///
/// `typed_text` must be **the text the user actually saw land on screen**: on the streaming
/// path that is what was really typed out, possibly shorter than the full LLM output (mid-stream
/// failure, cancellation). Using the full output as the baseline would classify every unfinished
/// session as "the user deleted a big chunk".
///
/// `on_edit` is called on the observer thread, possibly multiple times. Returns `None` on any
/// failure — failing to learn is acceptable; breaking typing is not.
pub fn watch_for_edits<F>(
    typed_text: String,
    lifetime: std::time::Duration,
    on_edit: F,
) -> Option<EditWatcher>
where
    F: Fn(EditPair) -> bool + Send + Sync + 'static,
{
    #[cfg(target_os = "macos")]
    {
        if typed_text.trim().is_empty() {
            return None;
        }
        let stop = macos::spawn_edit_watcher(typed_text, lifetime, Box::new(on_edit))?;
        Some(EditWatcher { stop })
    }
    #[cfg(target_os = "windows")]
    {
        windows::spawn_edit_watcher(typed_text, lifetime, Box::new(on_edit))
            .map(|stop| EditWatcher { stop })
    }
    #[cfg(target_os = "android")]
    {
        crate::android::edit_observation::arm(typed_text, lifetime, Box::new(on_edit))
            .map(|generation| EditWatcher { generation })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        let _ = (typed_text, lifetime, on_edit);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dropping `EditWatcher` must actually stop the observer thread.
    ///
    /// The stop chain spans two files and has been misread before: `spawn_edit_watcher` only
    /// hands out the flag and nobody sets it — the `Drop` here does. The disarm site isn't an
    /// explicit `disarm()` call either but `*slot = None` (`arm_edit_watch` / `begin_session_as`).
    ///
    /// If this chain breaks the symptom is silent: the observer lives until the 60s hard
    /// timeout, keeps reading the user's document and reporting, and runs in parallel with the
    /// newly armed one. Hence this pinning test.
    #[cfg(target_os = "macos")]
    #[test]
    fn dropping_the_watcher_stops_the_observer_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let stop = Arc::new(AtomicBool::new(false));
        let watcher = EditWatcher {
            stop: Arc::clone(&stop),
        };
        assert!(!stop.load(Ordering::Relaxed), "刚建好不该是停止态");

        drop(watcher);
        assert!(
            stop.load(Ordering::Relaxed),
            "Drop 必须置位停止 flag —— 观察线程只认这一个信号（macos.rs 的 run_edit_watch_loop）"
        );
    }

    fn gate(bundle: Option<&str>, role: Option<&str>, subrole: Option<&str>) -> GateInputs {
        GateInputs {
            secure_input: false,
            bundle_id: bundle.map(str::to_string),
            role: role.map(str::to_string),
            subrole: subrole.map(str::to_string),
        }
    }

    #[test]
    fn failed_native_write_restores_selection_before_retry() {
        let caret = std::cell::Cell::new((3, 3));
        for result in [Err("clipboard unavailable".to_string()), Ok(())] {
            let original = caret.get();
            let outcome = write_with_selection_recovery(
                || {
                    caret.set((0, 6));
                    result.clone()
                },
                || caret.set(original),
            );
            assert_eq!(outcome, result);
            assert_eq!(
                caret.get(),
                if outcome.is_err() { original } else { (0, 6) }
            );
        }
    }

    #[test]
    fn ordinary_editor_passes_the_gate() {
        assert_eq!(
            evaluate_gate(&gate(
                Some("com.apple.Notes"),
                Some("AXTextArea"),
                Some("AXStandardWindow")
            )),
            None
        );
    }

    #[test]
    fn secure_input_blocks_before_anything_else() {
        let inputs = GateInputs {
            secure_input: true,
            ..gate(Some("com.apple.Notes"), Some("AXTextArea"), None)
        };
        assert_eq!(evaluate_gate(&inputs), Some(BlockReason::SecureInput));
    }

    #[test]
    fn secure_text_field_role_blocks() {
        assert_eq!(
            evaluate_gate(&gate(
                Some("com.apple.Safari"),
                Some("AXSecureTextField"),
                None
            )),
            Some(BlockReason::SecureTextField)
        );
    }

    #[test]
    fn secure_text_field_subrole_blocks() {
        // Safari / Chrome password fields often have role=AXTextField, subrole=AXSecureTextField;
        // checking role alone misses them.
        assert_eq!(
            evaluate_gate(&gate(
                Some("com.google.Chrome"),
                Some("AXTextField"),
                Some("AXSecureTextField")
            )),
            Some(BlockReason::SecureTextField)
        );
    }

    #[test]
    fn secure_text_field_match_is_case_insensitive() {
        assert_eq!(
            evaluate_gate(&gate(None, Some("axSECUREtextfield"), None)),
            Some(BlockReason::SecureTextField)
        );
    }

    #[test]
    fn password_managers_are_blocked() {
        for bundle in [
            "com.1password.1password",
            "com.agilebits.onepassword7",
            "com.apple.keychainaccess",
            "com.bitwarden.desktop",
        ] {
            assert_eq!(
                evaluate_gate(&gate(Some(bundle), Some("AXTextArea"), None)),
                Some(BlockReason::BlockedApp),
                "{bundle} should be blocked"
            );
        }
    }

    #[test]
    fn terminals_are_blocked() {
        for bundle in [
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "dev.warp.Warp-Stable",
            "com.mitchellh.ghostty",
        ] {
            assert_eq!(
                evaluate_gate(&gate(Some(bundle), Some("AXTextArea"), None)),
                Some(BlockReason::BlockedApp),
                "{bundle} should be blocked"
            );
        }
    }

    #[test]
    fn bundle_match_is_case_insensitive_and_prefix_based() {
        // NSWorkspace's casing isn't guaranteed to match the list; helper processes append suffixes.
        assert_eq!(
            evaluate_gate(&gate(Some("COM.APPLE.TERMINAL"), None, None)),
            Some(BlockReason::BlockedApp)
        );
        assert_eq!(
            evaluate_gate(&gate(Some("com.1password.1password-helper"), None, None)),
            Some(BlockReason::BlockedApp)
        );
    }

    #[test]
    fn a_bundle_that_merely_contains_a_blocked_name_is_not_blocked() {
        // Prefix match, not substring match: another app whose name contains "terminal" must not be hit.
        assert_eq!(
            evaluate_gate(&gate(Some("com.example.terminalnotes"), None, None)),
            None
        );
    }

    #[test]
    fn missing_metadata_does_not_block_by_itself() {
        // Missing bundle / role (AX permission not granted, non-macOS) must count neither as safe
        // nor as dangerous — the gate only handles known danger signals; unreadable documents
        // naturally end up as Unavailable.
        assert_eq!(evaluate_gate(&GateInputs::default()), None);
    }

    #[test]
    fn document_window_splits_at_the_cursor() {
        let win = DocumentWindow {
            text: "上下文测试".to_string(),
            cursor: 2,
        };
        assert_eq!(win.before(), "上下");
        assert_eq!(win.after(), "文测试");
    }

    #[test]
    fn document_window_cursor_at_the_end_yields_empty_after() {
        let win = DocumentWindow {
            text: "abc".to_string(),
            cursor: 3,
        };
        assert_eq!(win.before(), "abc");
        assert_eq!(win.after(), "");
    }

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn non_macos_reports_unsupported_without_touching_anything() {
        let result = probe_around_cursor(DEFAULT_BUDGET_CHARS).await;
        assert_eq!(result.status, HostDocumentStatus::Unsupported);
        assert!(result.window.is_none());
    }
}
