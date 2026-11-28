//! Native delivery first; macOS may recover through one guarded paste only when
//! the complete original still occupies the frozen selection.

use openless_core::{BackendError, BackendErrorCode, InsertOutcome};

pub(crate) fn native_result(result: Result<(), String>) -> Result<InsertOutcome, BackendError> {
    result.map(|()| InsertOutcome::Inserted).map_err(|reason| {
        BackendError::new(
            if reason == "selectionPolishWriteUnconfirmed" {
                BackendErrorCode::OutcomeUnknown
            } else {
                BackendErrorCode::Platform
            },
            reason,
        )
    })
}

pub(crate) fn fallback_result(
    mut error: BackendError,
    fallback: Result<InsertOutcome, BackendError>,
) -> Result<InsertOutcome, BackendError> {
    if error.code == BackendErrorCode::OutcomeUnknown {
        error.details = Some(serde_json::json!({
            "original_saved": true,
            "result_copied": matches!(fallback, Ok(InsertOutcome::CopiedFallback)),
        }));
        Err(error)
    } else {
        fallback
    }
}

pub(crate) fn deliver<T, E>(
    direct: impl FnOnce() -> Result<T, E>,
    fallback: impl FnOnce(E) -> Result<T, E>,
) -> Result<T, E> {
    match direct() {
        Ok(outcome) => Ok(outcome),
        Err(error) => fallback(error),
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn recover_with_paste(
    error: BackendError,
    confirmed: impl FnOnce() -> bool,
    paste: impl FnOnce() -> crate::types::InsertStatus,
    fallback: impl FnOnce() -> Result<InsertOutcome, BackendError>,
) -> Result<InsertOutcome, BackendError> {
    use crate::types::InsertStatus;
    if error.code == BackendErrorCode::OutcomeUnknown && confirmed() {
        return Ok(InsertOutcome::Inserted);
    }
    match paste() {
        InsertStatus::Inserted => Ok(InsertOutcome::Inserted),
        InsertStatus::CopiedFallback => Ok(InsertOutcome::CopiedFallback),
        InsertStatus::PasteSent => {
            // Dispatch is not completion. Never retry an unconfirmed paste;
            // retain its result on the clipboard and settle the session.
            fallback_result(
                BackendError::new(BackendErrorCode::OutcomeUnknown, "selectionPolishWriteUnconfirmed"),
                Ok(InsertOutcome::CopiedFallback),
            )
        }
        _ => fallback_result(error, fallback()),
    }
}
