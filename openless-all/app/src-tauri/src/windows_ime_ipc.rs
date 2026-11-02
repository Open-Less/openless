#![allow(dead_code, unused_imports, unused_variables)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::windows_ime_protocol::{
    is_failed_hresult, ImeSubmitOp, ImeSubmitStatus, IME_STATUS_ACCEPTED, IME_STATUS_BAD_REQUEST,
    IME_STATUS_COMMITTED, IME_STATUS_COMPOSITION_CONFLICT,
    IME_STATUS_COMPOSITION_RECOVERY_REQUIRED, IME_STATUS_NOT_DISPATCHED, IME_STATUS_PENDING,
    IME_STATUS_PROTOCOL_VERSION,
};

pub const IME_CLIENT_WAIT_TIMEOUT: Duration = Duration::from_millis(700);

// The DLL commits from a message it posts to the host's TSF thread, and some
// hosts only grant an async edit session later. Once the submit message has
// been delivered, a timeout proves nothing: the edit can still commit
// afterwards. Every post-dispatch timeout therefore remains OutcomeUnknown,
// never a definite failure that authorizes another insertion attempt.
pub const IME_SUBMIT_TIMEOUT: Duration = Duration::from_millis(5000);
const IME_SEND_TIMEOUT_MS: u32 = 2000;
const IME_QUERY_SEND_TIMEOUT_MS: u32 = 500;
const IME_QUERY_INTERVAL: Duration = Duration::from_millis(10);
const IME_WINDOW_RETRY_INTERVAL: Duration = Duration::from_millis(25);

const HRESULT_TIMEOUT: u32 = 0x8007_05B4;
const HRESULT_CANCELLED: u32 = 0x8007_04C7;

#[cfg(target_os = "windows")]
fn diagnostic_build_id() -> &'static str {
    let value = option_env!("OPENLESS_BUILD_ID").unwrap_or("unbound");
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        value
    } else {
        "unbound"
    }
}

#[cfg(target_os = "windows")]
fn emit_tsf_diagnostic_probe(
    probe_id: &str,
    hypothesis_ids: &str,
    session_id: &str,
    event: &str,
    outcome: &str,
    safe_fields: String,
) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    log::info!(
        target: "diagnosing-bugs",
        "{{\"schema\":\"diagnosing-bugs.v1\",\"session_id\":\"{}\",\"run_id\":\"tsf-runtime-{}\",\"build_id\":\"{}\",\"protocol_revision\":\"0x{:08X}\",\"probe_id\":\"{}\",\"hypothesis_ids\":{},\"event\":\"{}\",\"outcome\":\"{}\",\"safe\":{{{}}},\"ts\":{}}}",
        session_id,
        std::process::id(),
        diagnostic_build_id(),
        IME_STATUS_PROTOCOL_VERSION,
        probe_id,
        hypothesis_ids,
        event,
        outcome,
        safe_fields,
        ts,
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsImeIpcError {
    Unavailable(String),
    NoReadyClient,
    Timeout,
    OutcomeUnknown(String),
    Protocol(String),
    Io(String),
}

impl std::fmt::Display for WindowsImeIpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message)
            | Self::OutcomeUnknown(message)
            | Self::Protocol(message)
            | Self::Io(message) => {
                write!(f, "{message}")
            }
            Self::NoReadyClient => write!(f, "no OpenLess IME client is ready"),
            Self::Timeout => write!(f, "OpenLess IME IPC timed out"),
        }
    }
}

impl std::error::Error for WindowsImeIpcError {}

impl WindowsImeIpcError {
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, Self::OutcomeUnknown(_))
    }
}

pub type WindowsImeIpcResult<T> = Result<T, WindowsImeIpcError>;

/// Reply to the submit message itself. Any error here is definite: the DLL
/// did not queue the text, so the caller may fall back to another insertion.
fn classify_submit_reply(reply: u32) -> WindowsImeIpcResult<()> {
    match reply {
        IME_STATUS_ACCEPTED => Ok(()),
        0 => Err(WindowsImeIpcError::Protocol(
            "IME window ignored the submit message; the installed OpenLessIme.dll predates the message protocol"
                .to_string(),
        )),
        IME_STATUS_BAD_REQUEST => Err(WindowsImeIpcError::Protocol(
            "IME rejected the submit payload; the target process may still have an older OpenLessIme.dll loaded; restart the target app after updating OpenLess"
                .to_string(),
        )),
        code if is_failed_hresult(code) => Err(WindowsImeIpcError::Io(format!(
            "IME could not queue the submit: hresult:0x{code:08X}"
        ))),
        other => Err(WindowsImeIpcError::Protocol(format!(
            "unexpected IME submit reply 0x{other:08X}"
        ))),
    }
}

/// Reply to a query sent after the submit was accepted. `Ok(None)` means the
/// commit is still pending. Anything that is not a clear result stays
/// OutcomeUnknown, because the text may already be in the document.
fn classify_query_reply(reply: u32) -> WindowsImeIpcResult<Option<ImeSubmitStatus>> {
    match reply {
        IME_STATUS_PENDING => Ok(None),
        IME_STATUS_COMMITTED => Ok(Some(ImeSubmitStatus::Committed)),
        IME_STATUS_COMPOSITION_CONFLICT => Ok(Some(ImeSubmitStatus::CompositionConflict)),
        IME_STATUS_COMPOSITION_RECOVERY_REQUIRED => {
            Ok(Some(ImeSubmitStatus::CompositionRecoveryRequired))
        }
        IME_STATUS_NOT_DISPATCHED => Ok(Some(ImeSubmitStatus::NotDispatched)),
        HRESULT_TIMEOUT | HRESULT_CANCELLED => {
            // Keep the 1.x classification: a native timeout/cancel reply
            // does not prove InsertTextAtSelection never committed. R01's
            // definitive rejection fallback must not replay this text.
            Err(WindowsImeIpcError::OutcomeUnknown(format!(
                "native IME submission may still complete after hresult:0x{reply:08X}"
            )))
        }
        code if is_failed_hresult(code) => {
            log::warn!(
                "[windows-ime] submit result status=Rejected error_code=hresult:0x{code:08X}"
            );
            Ok(Some(ImeSubmitStatus::Rejected))
        }
        other => Err(WindowsImeIpcError::OutcomeUnknown(format!(
            "ambiguous post-dispatch reply 0x{other:08X}"
        ))),
    }
}

/// Tokens only have to differ between consecutive submits to one IME window,
/// including across an OpenLess restart; zero is reserved by the DLL.
fn next_submit_token() -> u32 {
    static SEQUENCE: AtomicU32 = AtomicU32::new(1);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed) & 0xFFFF;
    (((std::process::id() & 0xFFFF) << 16) | sequence).max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImeWindow {
    hwnd: isize,
    process_id: u32,
    thread_id: u32,
}

/// Order eligible IME windows by safety: all windows on the captured target thread first,
/// followed by same-process windows only for explicit native recovery. A request cannot use
/// another UI thread's service during normal insertion because the native service resolves its
/// focused document manager from its own thread manager.
fn ordered_ime_windows(
    target: ImeSubmitTarget,
    windows: &[ImeWindow],
    allow_same_process_fallback: bool,
) -> Vec<ImeWindow> {
    let mut ordered = windows
        .iter()
        .copied()
        .filter(|window| {
            window.process_id == target.process_id
                && (window.thread_id == target.thread_id || allow_same_process_fallback)
        })
        .collect::<Vec<_>>();
    ordered.sort_by_key(|window| window.thread_id != target.thread_id);
    ordered
}

fn select_ime_window(
    target: ImeSubmitTarget,
    windows: &[ImeWindow],
    allow_same_process_fallback: bool,
) -> Option<ImeWindow> {
    ordered_ime_windows(target, windows, allow_same_process_fallback)
        .into_iter()
        .next()
}

/// Probe candidates in priority order and return the first window running the current protocol.
/// The caller supplies the probe so the selection policy can be tested without Win32 windows.
fn select_ime_window_with_probe<F>(
    target: ImeSubmitTarget,
    windows: &[ImeWindow],
    allow_same_process_fallback: bool,
    mut probe: F,
) -> WindowsImeIpcResult<Option<ImeWindow>>
where
    F: FnMut(ImeWindow) -> WindowsImeIpcResult<()>,
{
    let candidates = ordered_ime_windows(target, windows, allow_same_process_fallback);
    if candidates.is_empty() {
        return Ok(None);
    }

    let mut last_error = None;
    for window in candidates {
        match probe(window) {
            Ok(()) => return Ok(Some(window)),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("non-empty candidate list must produce a probe result"))
}

fn classify_protocol_probe_reply(reply: u32) -> WindowsImeIpcResult<()> {
    if reply == IME_STATUS_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(WindowsImeIpcError::Protocol(format!(
            "the target process has no current OpenLessIme.dll protocol; protocol probe returned 0x{reply:08X}"
        )))
    }
}

#[derive(Debug, Clone)]
pub struct ImeSubmitRequest {
    pub session_id: String,
    pub text: String,
    pub created_at: String,
    pub target: Option<ImeSubmitTarget>,
    pub op: ImeSubmitOp,
    /// Stable owner for the live composition across submit/query tokens.
    pub composition_owner: u32,
    /// OpenLess process instance that owns the composition. A changed value permits native
    /// recovery after the process that created an orphaned composition has exited.
    pub composition_process_id: u32,
    /// Allow a recovery Cancel to use another same-process IME window when the original target
    /// thread has no registered OpenLess service. This is only safe for native recovery because
    /// owner checks still reject a same-process composition owned by another insertion.
    pub allow_same_process_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeSubmitTarget {
    pub process_id: u32,
    pub thread_id: u32,
    /// Focused control captured with the target, used by the native prime guard.
    pub window_handle: usize,
}

#[derive(Clone)]
pub struct WindowsImeIpcServer {
    inner: std::sync::Arc<parking_lot::Mutex<WindowsImeIpcState>>,
}

#[derive(Debug, Default)]
struct WindowsImeIpcState {
    ready_client_id: Option<String>,
}

impl WindowsImeIpcServer {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::Mutex::new(WindowsImeIpcState::default())),
        }
    }

    pub fn mark_client_ready_for_test(&self, client_id: String) {
        self.inner.lock().ready_client_id = Some(client_id);
    }

    pub fn has_ready_client(&self) -> bool {
        self.inner.lock().ready_client_id.is_some()
    }

    pub async fn submit_text(
        &self,
        request: ImeSubmitRequest,
    ) -> WindowsImeIpcResult<ImeSubmitStatus> {
        #[cfg(target_os = "windows")]
        {
            let _ = self;
            submit_text_to_platform(request).await
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = self;
            let _ = request;
            Err(WindowsImeIpcError::Unavailable(
                "OpenLess IME IPC is only available on Windows".to_string(),
            ))
        }
    }
}

impl Default for WindowsImeIpcServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
async fn submit_text_to_platform(
    request: ImeSubmitRequest,
) -> WindowsImeIpcResult<ImeSubmitStatus> {
    // Sending and polling block on the host's message loop; keep that off the
    // async runtime workers.
    use crate::issue1266_debug::{Event, Facts, Operation, Outcome, Probe};
    let operation = Operation::new(Probe::P4);
    let _scope = operation.scope();
    operation.mark(Event::Requested);
    let result = tokio::task::spawn_blocking(move || {
        let _worker_scope = operation.scope();
        operation.mark(Event::Entered);
        let result = windows_message::submit_text_to_window(request, operation);
        operation.record(Event::Returned, match &result {
            Ok(_) => Outcome::Returned,
            Err(error) if error.is_outcome_unknown() => Outcome::Unknown,
            Err(_) => Outcome::Error,
        }, Facts { value: match &result {
            Ok(ImeSubmitStatus::Committed) => 1,
            Ok(ImeSubmitStatus::NotDispatched) => 2,
            Ok(ImeSubmitStatus::CompositionConflict) => 3,
            Ok(ImeSubmitStatus::CompositionRecoveryRequired) => 4,
            Ok(ImeSubmitStatus::Rejected) => 5,
            Ok(ImeSubmitStatus::Failed) => 6,
            Err(_) => 0,
        }, ..Facts::default() });
        result
    })
        .await
        .map_err(|error| {
            WindowsImeIpcError::OutcomeUnknown(format!("IME submit task failed: {error}"))
        });
    if result.is_err() {
        operation.record(Event::Returned, Outcome::Unknown, Facts::default());
    }
    result?
}

#[cfg(target_os = "windows")]
mod windows_message {
    use std::ffi::c_void;
    use std::time::Instant;

    use super::{
        classify_protocol_probe_reply, classify_query_reply, classify_submit_reply,
        emit_tsf_diagnostic_probe, next_submit_token, select_ime_window_with_probe,
        ImeSubmitRequest, ImeSubmitTarget, ImeWindow, WindowsImeIpcError, WindowsImeIpcResult,
        IME_CLIENT_WAIT_TIMEOUT, IME_QUERY_INTERVAL, IME_QUERY_SEND_TIMEOUT_MS,
        IME_SEND_TIMEOUT_MS, IME_SUBMIT_TIMEOUT, IME_WINDOW_RETRY_INTERVAL,
    };
    use crate::windows_ime_protocol::{
        encode_composition_submit_payload, encode_query_payload, ImeSubmitStatus,
        IME_COPYDATA_QUERY, IME_COPYDATA_VERSION, IME_MAX_SUBMIT_BYTES, IME_STATUS_ACCEPTED,
        IME_STATUS_PROTOCOL_VERSION, OPENLESS_IME_MESSAGE_WINDOW_CLASS,
    };

    const HWND_MESSAGE: isize = -3;
    const WM_COPYDATA: u32 = 0x004A;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_TIMEOUT: u32 = 1460;
    const MAX_ENUMERATED_WINDOWS: usize = 4096;

    #[repr(C)]
    struct CopyDataStruct {
        dw_data: usize,
        cb_data: u32,
        lp_data: *const c_void,
    }

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowExW(
            hWndParent: isize,
            hWndChildAfter: isize,
            lpszClass: *const u16,
            lpszWindow: *const u16,
        ) -> isize;
        fn GetWindowThreadProcessId(hWnd: isize, lpdwProcessId: *mut u32) -> u32;
        fn SendMessageTimeoutW(
            hWnd: isize,
            Msg: u32,
            wParam: usize,
            lParam: isize,
            fuFlags: u32,
            uTimeout: u32,
            lpdwResult: *mut usize,
        ) -> isize;
    }

    enum SendFailure {
        /// The host did not answer in time; the message may still be handled.
        TimedOut,
        /// The message never reached the window (OS error code).
        NotDelivered(u32),
    }

    fn verify_protocol_version(hwnd: isize, timeout_ms: u32) -> WindowsImeIpcResult<()> {
        match send_copy_data(hwnd, IME_COPYDATA_VERSION, &[], timeout_ms) {
            Ok(reply) if reply == IME_STATUS_PROTOCOL_VERSION => {
                emit_tsf_diagnostic_probe(
                    "P1",
                    "[\"H1\",\"H2\"]",
                    "runtime",
                    "protocol_probe",
                    "current",
                    format!("\"raw_status\":\"0x{reply:08X}\""),
                );
                log::debug!(
                    "[windows-ime] protocol probe status=current raw_status=0x{:08X}",
                    reply,
                );
                Ok(())
            }
            Ok(reply) => {
                emit_tsf_diagnostic_probe(
                    "P1",
                    "[\"H1\",\"H2\"]",
                    "runtime",
                    "protocol_probe",
                    "rejected",
                    format!("\"raw_status\":\"0x{reply:08X}\""),
                );
                log::warn!(
                    "[windows-ime] protocol probe status=stale_or_unknown raw_status=0x{:08X}",
                    reply,
                );
                classify_protocol_probe_reply(reply)
            }
            Err(SendFailure::TimedOut) => {
                emit_tsf_diagnostic_probe(
                    "P1",
                    "[\"H1\",\"H2\"]",
                    "runtime",
                    "protocol_probe",
                    "timeout",
                    "\"error_class\":\"timeout\"".to_string(),
                );
                Err(WindowsImeIpcError::Protocol(
                    "the target process did not answer the OpenLessIme.dll protocol probe"
                        .to_string(),
                ))
            }
            Err(SendFailure::NotDelivered(code)) => {
                emit_tsf_diagnostic_probe(
                    "P1",
                    "[\"H1\",\"H2\"]",
                    "runtime",
                    "protocol_probe",
                    "not_delivered",
                    format!("\"error_code\":{}", code),
                );
                Err(WindowsImeIpcError::Io(format!(
                    "sending the OpenLessIme.dll protocol probe failed: OS error {code}"
                )))
            }
        }
    }

    pub fn submit_text_to_window(
        request: ImeSubmitRequest,
        operation: crate::issue1266_debug::Operation,
    ) -> WindowsImeIpcResult<ImeSubmitStatus> {
        let target = request.target.ok_or(WindowsImeIpcError::NoReadyClient)?;
        let token = next_submit_token();
        let payload = if request.op == crate::windows_ime_protocol::ImeSubmitOp::Commit {
            crate::windows_ime_protocol::encode_commit_submit_payload(
                token,
                request.composition_process_id,
                target.window_handle as u64,
                &request.text,
            )
        } else {
            encode_composition_submit_payload(
                token,
                request.composition_owner,
                request.composition_process_id,
                target.window_handle as u64,
                &request.text,
            )
        };
        if payload.len() > IME_MAX_SUBMIT_BYTES {
            return Err(WindowsImeIpcError::Protocol(format!(
                "text is too large for one IME submit ({} bytes)",
                payload.len()
            )));
        }

        let window = find_ime_window_with_retry(target, request.allow_same_process_fallback)?;
        operation.mark(crate::issue1266_debug::Event::WindowFound);
        log::info!(
            "[windows-ime] submit begin op={} target_pid={} target_tid={} target_hwnd=0x{:X} ime_pid={} ime_tid={} owner={} process={} payload_bytes={} same_process_fallback={}",
            request.op.label(),
            target.process_id,
            target.thread_id,
            target.window_handle,
            window.process_id,
            window.thread_id,
            request.composition_owner,
            request.composition_process_id,
            payload.len(),
            request.allow_same_process_fallback,
        );

        let reply = send_copy_data(
            window.hwnd,
            request.op.copydata_tag(),
            &payload,
            IME_SEND_TIMEOUT_MS,
        )
        .map_err(|failure| match failure {
            SendFailure::TimedOut => WindowsImeIpcError::OutcomeUnknown(
                "OpenLess IME did not acknowledge the submit in time".to_string(),
            ),
            SendFailure::NotDelivered(ERROR_ACCESS_DENIED) => WindowsImeIpcError::Io(
                "IME window refused the message (target runs at a higher integrity level)"
                    .to_string(),
            ),
            SendFailure::NotDelivered(code) => {
                WindowsImeIpcError::Io(format!("sending to the IME window failed: OS error {code}"))
            }
        })?;
        operation.record(crate::issue1266_debug::Event::SubmitAck,
            if reply == IME_STATUS_ACCEPTED { crate::issue1266_debug::Outcome::Accepted }
            else { crate::issue1266_debug::Outcome::Rejected }, crate::issue1266_debug::Facts::default());
        log::info!(
            "[windows-ime] submit ack op={} raw_status=0x{:08X}",
            request.op.label(),
            reply,
        );
        emit_tsf_diagnostic_probe(
            "P3",
            "[\"H4\"]",
            &request.session_id,
            "submit_ack",
            if reply == IME_STATUS_ACCEPTED {
                "accepted"
            } else {
                "rejected"
            },
            format!(
                "\"op\":\"{}\",\"raw_status\":\"0x{reply:08X}\"",
                request.op.label()
            ),
        );
        classify_submit_reply(reply)?;

        let deadline = Instant::now() + IME_SUBMIT_TIMEOUT;
        let query = encode_query_payload(token);
        struct QueryCount(crate::issue1266_debug::Operation, u64);
        impl Drop for QueryCount {
            fn drop(&mut self) {
                self.0.record(crate::issue1266_debug::Event::QueryCount,
                    crate::issue1266_debug::Outcome::Observed,
                    crate::issue1266_debug::Facts { value: self.1, ..Default::default() });
            }
        }
        let mut query_count = QueryCount(operation, 0);
        loop {
            std::thread::sleep(IME_QUERY_INTERVAL);
            query_count.1 += 1;
            match send_copy_data(
                window.hwnd,
                IME_COPYDATA_QUERY,
                &query,
                IME_QUERY_SEND_TIMEOUT_MS,
            ) {
                Ok(reply) => {
                    log::debug!(
                        "[windows-ime] submit query op={} raw_status=0x{:08X}",
                        request.op.label(),
                        reply,
                    );
                    if let Some(status) = classify_query_reply(reply)? {
                        emit_tsf_diagnostic_probe(
                            "P3",
                            "[\"H4\"]",
                            &request.session_id,
                            "submit_complete",
                            match status {
                                ImeSubmitStatus::Committed => "committed",
                                ImeSubmitStatus::NotDispatched => "not_dispatched",
                                ImeSubmitStatus::CompositionConflict => "composition_conflict",
                                ImeSubmitStatus::CompositionRecoveryRequired => {
                                    "composition_recovery_required"
                                }
                                ImeSubmitStatus::Rejected => "rejected",
                                ImeSubmitStatus::Failed => "failed",
                            },
                            format!(
                                "\"op\":\"{}\",\"raw_status\":\"0x{reply:08X}\"",
                                request.op.label()
                            ),
                        );
                        log::info!(
                            "[windows-ime] submit complete op={} status={:?}",
                            request.op.label(),
                            status,
                        );
                        return Ok(status);
                    }
                }
                // The host is busy, possibly inside the commit itself.
                Err(SendFailure::TimedOut) => {}
                Err(SendFailure::NotDelivered(code)) => {
                    return Err(WindowsImeIpcError::OutcomeUnknown(format!(
                        "IME window went away after submit dispatch: OS error {code}"
                    )));
                }
            }
            if Instant::now() >= deadline {
                return Err(WindowsImeIpcError::OutcomeUnknown(
                    "OpenLess IME IPC timed out after submit dispatch".to_string(),
                ));
            }
        }
    }

    fn find_ime_window_with_retry(
        target: ImeSubmitTarget,
        allow_same_process_fallback: bool,
    ) -> WindowsImeIpcResult<ImeWindow> {
        let deadline = Instant::now() + IME_CLIENT_WAIT_TIMEOUT;
        let mut last_probe_error = None;
        loop {
            let windows = enumerate_ime_windows();
            log::debug!(
                "[windows-ime] discover target_pid={} target_tid={} candidates={} same_process_fallback={}",
                target.process_id,
                target.thread_id,
                windows.len(),
                allow_same_process_fallback,
            );
            match select_ime_window_with_probe(
                target,
                &windows,
                allow_same_process_fallback,
                |window| {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let timeout_ms = remaining.as_millis().min(u32::MAX as u128) as u32;
                    let result = if timeout_ms == 0 {
                        Err(WindowsImeIpcError::Protocol(
                            "the IME protocol probe deadline expired".to_string(),
                        ))
                    } else {
                        verify_protocol_version(window.hwnd, timeout_ms)
                    };
                    if let Err(error) = &result {
                        log::debug!(
                            "[windows-ime] protocol candidate rejected pid={} tid={} hwnd=0x{:X} error={}",
                            window.process_id,
                            window.thread_id,
                            window.hwnd,
                            error,
                        );
                    }
                    emit_tsf_diagnostic_probe(
                        "P2",
                        "[\"H1\",\"H3\"]",
                        "runtime",
                        "ime_candidate_probe",
                        if result.is_ok() { "accepted" } else { "rejected" },
                        format!(
                            "\"candidate_pid\":{},\"candidate_tid\":{},\"target_pid\":{},\"target_tid\":{},\"same_process_fallback\":{}",
                            window.process_id,
                            window.thread_id,
                            target.process_id,
                            target.thread_id,
                            allow_same_process_fallback,
                        ),
                    );
                    result
                },
            ) {
                Ok(Some(window)) => {
                    if window.thread_id != target.thread_id {
                        log::info!(
                            "[windows-ime] no IME window on target thread {}; using current same-process thread {}",
                            target.thread_id,
                            window.thread_id
                        );
                    } else {
                        log::debug!(
                            "[windows-ime] selected current exact target thread pid={} tid={}",
                            window.process_id,
                            window.thread_id,
                        );
                    }
                    return Ok(window);
                }
                Ok(None) => {}
                Err(error) => {
                    last_probe_error = Some(error);
                }
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                if let Some(error) = last_probe_error {
                    return Err(error);
                }
                log::warn!(
                    "[windows-ime] no message window for target_pid={} target_tid={} after {:?}",
                    target.process_id,
                    target.thread_id,
                    IME_CLIENT_WAIT_TIMEOUT,
                );
                return Err(WindowsImeIpcError::NoReadyClient);
            }
            std::thread::sleep(remaining.min(IME_WINDOW_RETRY_INTERVAL));
        }
    }

    fn enumerate_ime_windows() -> Vec<ImeWindow> {
        let class_name = OPENLESS_IME_MESSAGE_WINDOW_CLASS
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();

        let mut windows = Vec::new();
        let mut previous = 0isize;
        for _ in 0..MAX_ENUMERATED_WINDOWS {
            let hwnd = unsafe {
                FindWindowExW(
                    HWND_MESSAGE,
                    previous,
                    class_name.as_ptr(),
                    std::ptr::null(),
                )
            };
            if hwnd == 0 {
                break;
            }
            let mut process_id = 0u32;
            let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
            if thread_id != 0 && process_id != 0 {
                windows.push(ImeWindow {
                    hwnd,
                    process_id,
                    thread_id,
                });
            }
            previous = hwnd;
        }
        windows
    }

    fn send_copy_data(
        hwnd: isize,
        kind: usize,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<u32, SendFailure> {
        let data = CopyDataStruct {
            dw_data: kind,
            cb_data: payload.len() as u32,
            lp_data: payload.as_ptr().cast(),
        };
        let mut reply = 0usize;
        let delivered = unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_COPYDATA,
                0,
                &data as *const CopyDataStruct as isize,
                SMTO_ABORTIFHUNG,
                timeout_ms,
                &mut reply,
            )
        };
        if delivered != 0 {
            // The DLL's replies fit in 32 bits; a 32-bit host sign-extends them.
            return Ok(reply as u32);
        }

        match std::io::Error::last_os_error()
            .raw_os_error()
            .map(|code| code as u32)
        {
            None | Some(0) | Some(ERROR_TIMEOUT) => Err(SendFailure::TimedOut),
            Some(code) => Err(SendFailure::NotDelivered(code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_ime_protocol::IME_STATUS_UNKNOWN_TOKEN;

    fn window(hwnd: isize, process_id: u32, thread_id: u32) -> ImeWindow {
        ImeWindow {
            hwnd,
            process_id,
            thread_id,
        }
    }

    #[test]
    fn submit_timeout_stays_within_followup_stall_budget() {
        assert_eq!(IME_SUBMIT_TIMEOUT, Duration::from_millis(5000));
    }

    #[test]
    fn only_post_dispatch_failures_have_unknown_outcomes() {
        assert!(WindowsImeIpcError::OutcomeUnknown("fixture".to_string()).is_outcome_unknown());
        assert!(!WindowsImeIpcError::Timeout.is_outcome_unknown());
        assert!(!WindowsImeIpcError::NoReadyClient.is_outcome_unknown());
    }

    #[test]
    fn submit_tokens_are_nonzero_and_change_between_submits() {
        let first = next_submit_token();
        let second = next_submit_token();
        assert_ne!(first, 0);
        assert_ne!(second, 0);
        assert_ne!(first, second);
    }

    #[test]
    fn exact_thread_window_wins_over_same_process_window() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [
            window(1, 4321, 1111),
            window(2, 1234, 9999),
            window(3, 1234, 5678),
        ];
        assert_eq!(select_ime_window(target, &windows, true), Some(windows[2]));
    }

    #[test]
    fn same_process_window_is_used_only_for_explicit_recovery() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 4321, 5678), window(2, 1234, 9999)];
        assert_eq!(select_ime_window(target, &windows, true), Some(windows[1]));
        assert_eq!(select_ime_window(target, &windows, false), None);
        assert_eq!(select_ime_window(target, &windows[..1], true), None);
    }

    #[test]
    fn composition_requires_a_window_on_the_captured_thread() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 4321, 5678), window(2, 1234, 9999)];
        assert_eq!(select_ime_window(target, &windows, false), None);
    }

    #[test]
    fn protocol_candidates_keep_exact_thread_first() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [
            window(1, 1234, 9999),
            window(2, 1234, 5678),
            window(3, 1234, 9999),
        ];
        assert_eq!(
            ordered_ime_windows(target, &windows, true),
            vec![windows[1], windows[0], windows[2]]
        );
    }

    #[test]
    fn stale_protocol_candidate_is_skipped_for_current_candidate() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 1234, 5678), window(2, 1234, 5678)];
        let mut probed = Vec::new();
        let selected = select_ime_window_with_probe(target, &windows, false, |candidate| {
            probed.push(candidate.hwnd);
            if candidate.hwnd == 2 {
                Ok(())
            } else {
                classify_protocol_probe_reply(0x4F4C_0005)
            }
        })
        .expect("a current candidate should be selected");
        assert_eq!(selected, Some(windows[1]));
        assert_eq!(probed, vec![1, 2]);
    }

    #[test]
    fn current_protocol_candidate_short_circuits_probe() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 1234, 5678), window(2, 1234, 5678)];
        let mut probed = Vec::new();
        let selected = select_ime_window_with_probe(target, &windows, false, |candidate| {
            probed.push(candidate.hwnd);
            Ok(())
        })
        .expect("the first current candidate should be selected");
        assert_eq!(selected, Some(windows[0]));
        assert_eq!(probed, vec![1]);
    }

    #[test]
    fn ordinary_probe_does_not_cross_same_process_threads() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 1234, 9999)];
        let mut probed = Vec::new();
        assert_eq!(
            select_ime_window_with_probe(target, &windows, false, |candidate| {
                probed.push(candidate.hwnd);
                Ok(())
            })
            .expect("no eligible candidate is not a probe error"),
            None
        );
        assert!(probed.is_empty());
    }

    #[test]
    fn recovery_probe_can_cross_to_same_process_thread() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 4321, 5678), window(2, 1234, 9999)];
        let selected = select_ime_window_with_probe(target, &windows, true, |_| Ok(()))
            .expect("same-process recovery candidate should be eligible");
        assert_eq!(selected, Some(windows[1]));
    }

    #[test]
    fn all_protocol_candidates_return_the_last_probe_error() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 1234, 5678), window(2, 1234, 5678)];
        let error = select_ime_window_with_probe(target, &windows, false, |candidate| {
            Err(WindowsImeIpcError::Protocol(format!(
                "stale hwnd={}",
                candidate.hwnd
            )))
        })
        .expect_err("all stale candidates should preserve a definite protocol error");
        assert_eq!(
            error,
            WindowsImeIpcError::Protocol("stale hwnd=2".to_string())
        );
    }

    #[test]
    fn no_eligible_protocol_candidate_is_not_a_probe_failure() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
            window_handle: 0,
        };
        let windows = [window(1, 4321, 5678)];
        assert_eq!(
            select_ime_window_with_probe(target, &windows, true, |_| {
                panic!("other-process windows must not be probed")
            })
            .expect("other-process windows are ineligible"),
            None
        );
    }

    #[test]
    fn protocol_probe_accepts_only_the_current_version() {
        assert_eq!(
            classify_protocol_probe_reply(IME_STATUS_PROTOCOL_VERSION),
            Ok(())
        );
        for reply in [0, 0x4F4C_0005, IME_STATUS_BAD_REQUEST] {
            assert!(matches!(
                classify_protocol_probe_reply(reply),
                Err(WindowsImeIpcError::Protocol(_))
            ));
        }
    }

    #[test]
    fn submit_reply_failures_are_definite_and_allow_fallback() {
        assert_eq!(classify_submit_reply(IME_STATUS_ACCEPTED), Ok(()));
        for reply in [0, IME_STATUS_BAD_REQUEST, IME_STATUS_COMMITTED, 0x8007_000E] {
            let error = classify_submit_reply(reply).unwrap_err();
            assert!(!error.is_outcome_unknown(), "reply 0x{reply:08X}");
        }
    }

    #[test]
    fn query_reply_reports_pending_committed_and_definite_rejection() {
        assert_eq!(classify_query_reply(IME_STATUS_PENDING), Ok(None));
        assert_eq!(
            classify_query_reply(IME_STATUS_COMMITTED),
            Ok(Some(ImeSubmitStatus::Committed))
        );
        assert_eq!(
            classify_query_reply(0x8000_4005),
            Ok(Some(ImeSubmitStatus::Rejected))
        );
    }

    #[test]
    fn native_timeout_and_cancel_responses_are_not_safe_to_retry() {
        for reply in [HRESULT_TIMEOUT, HRESULT_CANCELLED] {
            assert!(matches!(
                classify_query_reply(reply),
                Err(WindowsImeIpcError::OutcomeUnknown(_))
            ));
        }
    }

    #[test]
    fn ambiguous_query_replies_never_allow_definite_fallback() {
        // An unhandled message, a superseded token or a stray ack all mean the
        // dispatched text can no longer be accounted for.
        for reply in [0, IME_STATUS_UNKNOWN_TOKEN, IME_STATUS_ACCEPTED, 0x1234] {
            assert!(matches!(
                classify_query_reply(reply),
                Err(WindowsImeIpcError::OutcomeUnknown(_))
            ));
        }
    }
}
