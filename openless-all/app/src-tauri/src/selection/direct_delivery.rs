//! Selection delivery never uses paste. A native failure is the only path that
//! may replace the protected original on the clipboard.

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
