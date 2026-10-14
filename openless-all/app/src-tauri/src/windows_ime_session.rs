#![allow(dead_code, unused_imports, unused_variables)]
use crate::types::InsertStatus;
use crate::windows_ime_ipc::{ImeSubmitRequest, WindowsImeIpcServer};
use crate::windows_ime_protocol::ImeSubmitStatus;

#[derive(Debug)]
pub enum WindowsImeSessionError {
    Ipc(String),
    OutcomeUnknown(String),
}

impl std::fmt::Display for WindowsImeSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ipc(message) | Self::OutcomeUnknown(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for WindowsImeSessionError {}

impl WindowsImeSessionError {
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, Self::OutcomeUnknown(_))
    }
}

pub fn map_ime_status_to_insert_status(status: ImeSubmitStatus) -> InsertStatus {
    match status {
        ImeSubmitStatus::Committed => InsertStatus::Inserted,
        // The DLL's rejection/failure only proves "not committed"; it never wrote the clipboard.
        // Return Failed so the caller performs the real fallback per user settings; only paths
        // with a successful clipboard write qualify to report CopiedFallback. OutcomeUnknown is
        // still passed separately via Err.
        ImeSubmitStatus::Rejected | ImeSubmitStatus::Failed => InsertStatus::Failed,
    }
}

pub fn should_fallback_after_ime_result(status: ImeSubmitStatus) -> bool {
    !matches!(status, ImeSubmitStatus::Committed)
}

/// Submits dictated text to the OpenLess text service inside the target app.
///
/// The text service is registered under the TSF speech category, which TSF keeps
/// active next to the user's keyboard IME. There is no per-dictation session to
/// prepare or restore: switching the keyboard IME away and back made third-party
/// IMEs re-activate on every TSF thread, and that re-activation could block the
/// host forever (explorer hangs with Weasel, #665 / #954).
pub struct WindowsImeSessionController {
    ipc: WindowsImeIpcServer,
}

impl WindowsImeSessionController {
    pub fn new() -> Self {
        Self {
            ipc: WindowsImeIpcServer::new(),
        }
    }

    pub async fn submit(
        &self,
        request: ImeSubmitRequest,
    ) -> Result<InsertStatus, WindowsImeSessionError> {
        let status = self.ipc.submit_text(request).await.map_err(|error| {
            if error.is_outcome_unknown() {
                WindowsImeSessionError::OutcomeUnknown(error.to_string())
            } else {
                WindowsImeSessionError::Ipc(error.to_string())
            }
        })?;
        if should_fallback_after_ime_result(status) {
            log::warn!(
                "[windows-ime] TSF submit returned {status:?}; falling back to non-TSF insertion"
            );
        }
        Ok(map_ime_status_to_insert_status(status))
    }
}

impl Default for WindowsImeSessionController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_ime_result_maps_to_inserted() {
        assert_eq!(
            map_ime_status_to_insert_status(ImeSubmitStatus::Committed),
            InsertStatus::Inserted
        );
    }

    #[test]
    fn rejected_ime_result_never_claims_that_the_clipboard_was_written() {
        for status in [ImeSubmitStatus::Rejected, ImeSubmitStatus::Failed] {
            assert_eq!(
                map_ime_status_to_insert_status(status),
                InsertStatus::Failed
            );
        }
    }

    #[test]
    fn rejected_ime_result_requests_fallback() {
        assert!(should_fallback_after_ime_result(ImeSubmitStatus::Rejected));
        assert!(should_fallback_after_ime_result(ImeSubmitStatus::Failed));
        assert!(!should_fallback_after_ime_result(
            ImeSubmitStatus::Committed
        ));
    }

    #[test]
    fn outcome_unknown_is_distinct_from_a_definite_ipc_failure() {
        assert!(WindowsImeSessionError::OutcomeUnknown("fixture".to_string()).is_outcome_unknown());
        assert!(!WindowsImeSessionError::Ipc("fixture".to_string()).is_outcome_unknown());
    }

    #[tokio::test]
    async fn submit_without_a_target_is_a_definite_failure() {
        let controller = WindowsImeSessionController::new();
        let result = controller
            .submit(ImeSubmitRequest {
                session_id: "session-1".to_string(),
                text: "hello".to_string(),
                created_at: "2026-05-01T12:00:00Z".to_string(),
                target: None,
            })
            .await;

        assert!(matches!(result, Err(WindowsImeSessionError::Ipc(_))));
    }
}
