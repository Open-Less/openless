//! Whole-field ValuePattern selection writes have one irreversible submission.
//! A provider error after that boundary does not prove that nothing was written.

pub(crate) fn replace<E>(
    preflight: impl FnOnce() -> Result<(), String>,
    submit: impl FnOnce() -> Result<(), E>,
    confirm: impl FnOnce() -> Result<bool, E>,
) -> Result<(), String> {
    preflight()?;
    // Never retry: SetValue may have taken effect even when it returns an error.
    submit().map_err(|_| "selectionPolishWriteUnconfirmed".to_string())?;
    match confirm() {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err("selectionPolishWriteUnconfirmed".into()),
    }
}
