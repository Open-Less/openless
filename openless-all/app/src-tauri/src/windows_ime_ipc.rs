#![allow(dead_code, unused_imports, unused_variables)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::windows_ime_protocol::{
    is_failed_hresult, ImeSubmitStatus, IME_STATUS_ACCEPTED, IME_STATUS_BAD_REQUEST,
    IME_STATUS_COMMITTED, IME_STATUS_PENDING,
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
            "IME rejected the submit payload".to_string(),
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

/// Prefer the IME window on the target thread; otherwise use another one in
/// the same process (the focused control can live on a different thread than
/// the one TSF activated the IME on).
fn select_ime_window(target: ImeSubmitTarget, windows: &[ImeWindow]) -> Option<ImeWindow> {
    windows
        .iter()
        .find(|window| {
            window.process_id == target.process_id && window.thread_id == target.thread_id
        })
        .or_else(|| {
            windows
                .iter()
                .find(|window| window.process_id == target.process_id)
        })
        .copied()
}

#[derive(Debug, Clone)]
pub struct ImeSubmitRequest {
    pub session_id: String,
    pub text: String,
    pub created_at: String,
    pub target: Option<ImeSubmitTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeSubmitTarget {
    pub process_id: u32,
    pub thread_id: u32,
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
    tokio::task::spawn_blocking(move || windows_message::submit_text_to_window(request))
        .await
        .map_err(|error| {
            WindowsImeIpcError::OutcomeUnknown(format!("IME submit task failed: {error}"))
        })?
}

#[cfg(target_os = "windows")]
mod windows_message {
    use std::ffi::c_void;
    use std::time::Instant;

    use super::{
        classify_query_reply, classify_submit_reply, next_submit_token, select_ime_window,
        ImeSubmitRequest, ImeSubmitTarget, ImeWindow, WindowsImeIpcError, WindowsImeIpcResult,
        IME_CLIENT_WAIT_TIMEOUT, IME_QUERY_INTERVAL, IME_QUERY_SEND_TIMEOUT_MS,
        IME_SEND_TIMEOUT_MS, IME_SUBMIT_TIMEOUT, IME_WINDOW_RETRY_INTERVAL,
    };
    use crate::windows_ime_protocol::{
        encode_query_payload, encode_submit_payload, ImeSubmitStatus, IME_COPYDATA_QUERY,
        IME_COPYDATA_SUBMIT, IME_MAX_SUBMIT_BYTES, OPENLESS_IME_MESSAGE_WINDOW_CLASS,
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

    pub fn submit_text_to_window(
        request: ImeSubmitRequest,
    ) -> WindowsImeIpcResult<ImeSubmitStatus> {
        let target = request.target.ok_or(WindowsImeIpcError::NoReadyClient)?;
        let token = next_submit_token();
        let payload = encode_submit_payload(token, &request.text);
        if payload.len() > IME_MAX_SUBMIT_BYTES {
            return Err(WindowsImeIpcError::Protocol(format!(
                "text is too large for one IME submit ({} bytes)",
                payload.len()
            )));
        }

        let window = find_ime_window_with_retry(target)?;
        log::debug!(
            "[windows-ime] submitting text to IME window pid={} tid={}",
            window.process_id,
            window.thread_id
        );

        let reply = send_copy_data(
            window.hwnd,
            IME_COPYDATA_SUBMIT,
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
        classify_submit_reply(reply)?;

        let deadline = Instant::now() + IME_SUBMIT_TIMEOUT;
        let query = encode_query_payload(token);
        loop {
            std::thread::sleep(IME_QUERY_INTERVAL);
            match send_copy_data(
                window.hwnd,
                IME_COPYDATA_QUERY,
                &query,
                IME_QUERY_SEND_TIMEOUT_MS,
            ) {
                Ok(reply) => {
                    if let Some(status) = classify_query_reply(reply)? {
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

    fn find_ime_window_with_retry(target: ImeSubmitTarget) -> WindowsImeIpcResult<ImeWindow> {
        let deadline = Instant::now() + IME_CLIENT_WAIT_TIMEOUT;
        loop {
            if let Some(window) = select_ime_window(target, &enumerate_ime_windows()) {
                if window.thread_id != target.thread_id {
                    log::info!(
                        "[windows-ime] no IME window on target thread {}; using same-process thread {}",
                        target.thread_id,
                        window.thread_id
                    );
                }
                return Ok(window);
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
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
        };
        let windows = [
            window(1, 4321, 1111),
            window(2, 1234, 9999),
            window(3, 1234, 5678),
        ];
        assert_eq!(select_ime_window(target, &windows), Some(windows[2]));
    }

    #[test]
    fn same_process_window_is_used_when_target_thread_has_none() {
        let target = ImeSubmitTarget {
            process_id: 1234,
            thread_id: 5678,
        };
        let windows = [window(1, 4321, 5678), window(2, 1234, 9999)];
        assert_eq!(select_ime_window(target, &windows), Some(windows[1]));
        assert_eq!(select_ime_window(target, &windows[..1]), None);
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
