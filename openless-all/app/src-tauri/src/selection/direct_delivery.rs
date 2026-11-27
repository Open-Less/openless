//! Selection delivery never uses paste. A native failure is the only path that
//! may replace the protected original on the clipboard.

pub(crate) fn deliver<T, E>(
    direct: impl FnOnce() -> Result<T, E>,
    fallback: impl FnOnce(E) -> Result<T, E>,
) -> Result<T, E> {
    match direct() {
        Ok(outcome) => Ok(outcome),
        Err(error) => fallback(error),
    }
}
