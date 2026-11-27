//! Command-side admission before the existing native picker operation.
use openless_core::{OpenLessBackend, SessionId};

pub(super) fn dispatch<T>(
    core: &OpenLessBackend,
    session_id: SessionId,
    open: bool,
    style: bool,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    // Style admission belongs to its request-aware reservation: checking only
    // current editability here would misclassify an ended/replaced request.
    // Keep translation's existing precheck and let close retain its owner check.
    if open && !style && !core.dictation_translation_target()
        .is_some_and(|active| active.mutable && active.session_id == session_id) {
        return Err("recording_translation_required".into());
    }
    operation()
}
