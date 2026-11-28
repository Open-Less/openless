//! The host clipboard transaction, with native I/O supplied by the caller.

use crate::types::InsertStatus;

// Lock order: delivery gate, then any clipboard/target metadata. Native target validation
// must not acquire this gate again; it reads AX/UIA without simulated copy.
static CLIPBOARD_DELIVERY: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

pub(crate) fn lock() -> parking_lot::MutexGuard<'static, ()> {
    CLIPBOARD_DELIVERY.lock()
}

// Hotkey-edge selection capture cannot wait behind another app's paste consumption window.
pub(crate) fn try_lock() -> Option<parking_lot::MutexGuard<'static, ()>> {
    CLIPBOARD_DELIVERY.try_lock()
}

pub(crate) trait ClipboardDelivery {
    fn text(&mut self) -> Result<Option<String>, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn set_text_if_revision(&mut self, text: &str, expected: Option<u64>) -> Result<bool, String> {
        if expected.is_some() && !revision_matches(expected, self.sequence()) { return Ok(false); }
        self.set_text(text)?; Ok(true)
    }
    fn sequence(&self) -> Option<u64>;
    /// The native write returns this receipt under its own write authority.
    /// Controlled fixtures issue a receipt as part of their simulated write.
    fn owned_sequence(&self) -> Option<u64> { self.sequence() }
    fn owns_write(&mut self, receipt: Option<u64>) -> bool { revision_matches(receipt, self.sequence()) }
    fn paste(&mut self) -> Result<(), DispatchError>;
    fn wait_for_consumption(&mut self);
}

pub(crate) enum DispatchError {
    NotSent,
    OutcomeUnknown,
}

struct ClipboardRestorePlan {
    inserted_text: String,
    previous_text: Option<String>,
    sequence: Option<u64>,
}

pub(crate) fn revision_matches(expected: Option<u64>, current: Option<u64>) -> bool {
    expected.is_some() && expected == current
}

pub(crate) fn copy_with(
    clipboard: &mut impl ClipboardDelivery,
    text: &str,
    still_current: impl FnOnce() -> bool,
) -> Result<Option<u64>, String> {
    let _delivery = lock();
    if !still_current() {
        return Err("selection session cancelled".into());
    }
    clipboard.set_text(text)?;
    if clipboard.text()?.as_deref() != Some(text) {
        return Err("clipboard write was not retained".into());
    }
    let receipt = clipboard.owned_sequence();
    if !clipboard.owns_write(receipt) { return Err("clipboard ownership changed".into()); }
    Ok(receipt)
}

pub(crate) fn copy_if_current_with(
    clipboard: &mut impl ClipboardDelivery,
    text: &str,
    expected: Option<u64>,
    still_current: impl FnOnce() -> bool,
) -> Result<bool, String> {
    let _delivery = lock();
    if !still_current() || !revision_matches(expected, clipboard.sequence()) {
        return Ok(false);
    }
    if !clipboard.set_text_if_revision(text, expected)? { return Ok(false); }
    if clipboard.text()?.as_deref() != Some(text) {
        return Err("clipboard write was not retained".into());
    }
    let receipt = clipboard.owned_sequence();
    Ok(clipboard.owns_write(receipt))
}

fn owns(clipboard: &mut impl ClipboardDelivery, plan: &ClipboardRestorePlan) -> bool {
    let same_text = clipboard.text().ok().flatten().as_deref() == Some(&plan.inserted_text);
    same_text && clipboard.owns_write(plan.sequence)
        && plan
            .sequence
            .is_none_or(|sequence| clipboard.sequence() == Some(sequence))
}

fn restore(clipboard: &mut impl ClipboardDelivery, plan: &ClipboardRestorePlan) {
    if owns(clipboard, plan) {
        if let Some(previous) = &plan.previous_text {
            if let Err(error) = clipboard.set_text_if_revision(previous, plan.sequence) {
                log::warn!("[insertion] clipboard restore failed: {error}");
            }
        }
    }
}

pub(crate) fn insert_with(
    clipboard: &mut impl ClipboardDelivery,
    text: &str,
    restore_after_paste: bool,
    expected: Option<u64>,
    mut still_current: impl FnMut() -> bool,
) -> InsertStatus {
    insert_with_confirmation(
        clipboard,
        text,
        restore_after_paste,
        expected,
        &mut still_current,
        None::<fn() -> bool>,
    )
}

pub(crate) fn insert_with_confirmation(
    clipboard: &mut impl ClipboardDelivery,
    text: &str,
    restore_after_paste: bool,
    expected: Option<u64>,
    mut still_current: impl FnMut() -> bool,
    confirm: Option<impl FnOnce() -> bool>,
) -> InsertStatus {
    if text.is_empty() {
        return InsertStatus::CopiedFallback;
    }
    let _delivery = lock();
    // A workflow cancelled or moved while waiting must not even borrow the clipboard.
    if !still_current() {
        return InsertStatus::NotRequested;
    }
    if expected.is_some() && !revision_matches(expected, clipboard.sequence()) {
        return InsertStatus::NotRequested;
    }
    let previous_text = match clipboard.text() {
        Ok(previous) => previous,
        Err(error) => {
            log::warn!("[insertion] clipboard backup failed: {error}");
            return InsertStatus::Failed;
        }
    };
    // Reading the backup may take time. Do not borrow a newer external copy.
    if expected.is_some() && !revision_matches(expected, clipboard.sequence()) {
        return InsertStatus::NotRequested;
    }
    match clipboard.set_text_if_revision(text, expected) {
        Ok(true) => {}
        Ok(false) => return InsertStatus::NotRequested,
        Err(error) => { log::warn!("[insertion] clipboard write failed: {error}"); return InsertStatus::Failed; }
    }
    let plan = ClipboardRestorePlan {
        inserted_text: text.to_string(),
        previous_text,
        sequence: clipboard.owned_sequence(),
    };
    if !still_current() {
        if owns(clipboard, &plan) {
            return InsertStatus::CopiedFallback;
        }
        return InsertStatus::NotRequested;
    }
    // AX/UIA validation can cross a newer external copy. Never dispatch that content.
    if !owns(clipboard, &plan) {
        return InsertStatus::NotRequested;
    }
    let status = match clipboard.paste() {
        Ok(()) | Err(DispatchError::OutcomeUnknown) => InsertStatus::PasteSent,
        Err(DispatchError::NotSent) => {
            return if owns(clipboard, &plan) {
                InsertStatus::CopiedFallback
            } else {
                InsertStatus::NotRequested
            };
        }
    };
    clipboard.wait_for_consumption();
    // An unconfirmed ordinary paste must retain the result for manual pasting.
    // Run the native receipt before restore, under the same delivery lease.
    let confirmed = confirm.map(|confirm| confirm());
    if restore_after_paste && confirmed != Some(false) {
        restore(clipboard, &plan);
    }
    if confirmed == Some(true) {
        InsertStatus::Inserted
    } else {
        status
    }
}
