//! Native selection writes have one irreversible submission.
//! A provider error after that boundary does not prove that nothing was written.
//! selection_current includes the original range and is only checked before
//! submission. identity_current must fence cancellation and identity, not range.

pub(crate) fn replace<E>(
    preflight: impl FnOnce() -> Result<(), String>,
    selection_current: impl FnOnce() -> bool,
    identity_current: impl Fn() -> bool,
    submit: impl FnOnce() -> Result<(), E>,
    confirm: impl FnOnce() -> Result<bool, E>,
) -> Result<(), String> {
    preflight()?;
    if !selection_current() {
        return Err("selectionPolishTargetUnavailable".into());
    }
    submit_and_confirm(identity_current, submit, confirm)
}

/// Both Win32 and ValuePattern arrive here only after their original preflight.
pub(crate) fn submit_and_confirm<E>(
    identity_current: impl Fn() -> bool,
    submit: impl FnOnce() -> Result<(), E>,
    confirm: impl FnOnce() -> Result<bool, E>,
) -> Result<(), String> {
    // Never retry: a native setter may have taken effect even when it returns an error.
    submit().map_err(|_| "selectionPolishWriteUnconfirmed".to_string())?;
    // Only the identity/cancellation fence surrounds readback. The write itself
    // can legitimately collapse the old selection or select the new result.
    if !identity_current() {
        return Err("selectionPolishWriteUnconfirmed".into());
    }
    match confirm() {
        Ok(true) if identity_current() => Ok(()),
        Ok(true) => Err("selectionPolishWriteUnconfirmed".into()),
        Ok(false) | Err(_) => Err("selectionPolishWriteUnconfirmed".into()),
    }
}
