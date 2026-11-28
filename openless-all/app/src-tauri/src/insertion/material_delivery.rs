//! Ordered final clipboard transaction. Neither a paste receipt nor a caret
//! observation proves that the recipient consumed the image payload.
use super::clipboard_delivery::{self, ClipboardDelivery, DispatchError};
use crate::types::InsertStatus;
use openless_core::dictation_references::DeliveryPart;

pub(crate) trait MaterialClipboard: ClipboardDelivery {
    fn set_image(&mut self, path: &std::path::Path) -> Result<(), String>;
}
pub(crate) trait MaterialTarget {
    fn before(&mut self) -> bool;
    fn after(&mut self, part: &DeliveryPart) -> bool;
}
pub(crate) struct TargetContinuity<I, P, E> { expected: Option<(u64, u64)>, identity: I, position: P, activity: E, epoch: Option<u64> }
impl<I: FnMut() -> bool, P: FnMut() -> Option<(u64, u64)>, E: FnMut() -> Option<u64>> TargetContinuity<I, P, E> {
    pub(crate) fn new(expected: Option<(u64, u64)>, identity: I, position: P, mut activity: E) -> Self { let epoch = activity(); Self { expected, identity, position, activity, epoch } }
}
impl<I: FnMut() -> bool, P: FnMut() -> Option<(u64, u64)>, E: FnMut() -> Option<u64>> MaterialTarget for TargetContinuity<I, P, E> {
    fn before(&mut self) -> bool { (self.activity)() == self.epoch && (self.identity)() && self.expected.is_some() && (self.position)() == self.expected }
    fn after(&mut self, part: &DeliveryPart) -> bool {
        if (self.activity)() != self.epoch || !(self.identity)() { return false; }
        let Some(before) = self.expected else { return false; };
        let observed = (self.position)();
        let valid = match part {
            DeliveryPart::Text(text) => before.0.checked_add(text.encode_utf16().count() as u64).is_some_and(|end| observed == Some((end, 0))),
            // Rich text exposes one object-replacement unit; a recipient that
            // ignores images leaves the original range untouched. Anything else
            // is unproven continuity and stops before another segment is sent.
            DeliveryPart::Image(_) => observed == Some(before) || self.epoch.is_some() && before.0.checked_add(1).is_some_and(|end| observed == Some((end, 0))),
        };
        if valid { self.expected = observed; }
        valid
    }
}
fn recover(clipboard: &mut impl MaterialClipboard, parts: &[DeliveryPart], from: usize, receipt: Option<u64>) -> bool {
    let pending = parts[from..].iter().filter_map(|p| match p { DeliveryPart::Text(text) => Some(text.as_str()), _ => None }).collect::<String>();
    // Never replace a newer user copy. The complete result also remains in
    // history; this clipboard recovery contains only the unsent remainder.
    !pending.is_empty() && clipboard.owns_write(receipt) && clipboard.set_text_if_revision(&pending, receipt).unwrap_or(false)
}
pub(crate) fn deliver(
    clipboard: &mut impl MaterialClipboard,
    parts: &[DeliveryPart],
    restore_after_paste: bool,
    mut target: impl MaterialTarget,
) -> InsertStatus {
    let _delivery = clipboard_delivery::lock();
    if !target.before() { return InsertStatus::NotRequested; }
    let previous = clipboard.text().ok().flatten();
    let mut dispatched = false;
    let mut last_receipt = None;
    for (index, part) in parts.iter().enumerate() {
        if !target.before() || last_receipt.is_some() && !clipboard.owns_write(last_receipt) {
            let copied = recover(clipboard, parts, index, last_receipt);
            return if dispatched { InsertStatus::PasteSent } else if copied { InsertStatus::CopiedFallback } else { InsertStatus::NotRequested };
        }
        let before = clipboard.sequence();
        let written = match part { DeliveryPart::Text(text) => clipboard.set_text(text), DeliveryPart::Image(path) => clipboard.set_image(path) };
        if let Err(error) = written {
            log::warn!("[references] clipboard material write: {error}");
            if matches!(part, DeliveryPart::Image(_)) && clipboard.sequence() == before { continue; }
            let copied = recover(clipboard, parts, index, last_receipt);
            return if dispatched { InsertStatus::PasteSent } else if copied { InsertStatus::CopiedFallback } else { InsertStatus::Failed };
        }
        let receipt = clipboard.owned_sequence();
        if !target.before() || !clipboard.owns_write(receipt) {
            let copied = recover(clipboard, parts, index, receipt);
            return if dispatched { InsertStatus::PasteSent } else if copied { InsertStatus::CopiedFallback } else { InsertStatus::NotRequested };
        }
        last_receipt = receipt;
        match clipboard.paste() {
            Ok(()) => dispatched = true,
            Err(DispatchError::OutcomeUnknown) => {
                clipboard.wait_for_consumption();
                recover(clipboard, parts, index + usize::from(matches!(part, DeliveryPart::Image(_))), receipt);
                return InsertStatus::PasteSent;
            }
            Err(DispatchError::NotSent) if matches!(part, DeliveryPart::Image(_)) => {
                // Definitely no image dispatch: continue the following path,
                // provided the exact target and owned offer remain intact.
                continue;
            }
            Err(DispatchError::NotSent) => {
                let copied = recover(clipboard, parts, index, receipt);
                return if dispatched { InsertStatus::PasteSent } else if copied { InsertStatus::CopiedFallback } else { InsertStatus::Failed };
            }
        }
        clipboard.wait_for_consumption();
        if !clipboard.owns_write(receipt) || !target.after(part) {
            recover(clipboard, parts, index + 1, receipt);
            return InsertStatus::PasteSent;
        }
    }
    if restore_after_paste && clipboard.owns_write(last_receipt) {
        if let Some(previous) = previous { let _ = clipboard.set_text_if_revision(&previous, last_receipt); }
    }
    if dispatched { InsertStatus::PasteSent } else { InsertStatus::Failed }
}
