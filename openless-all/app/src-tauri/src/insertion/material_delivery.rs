//! Ordered final clipboard transaction; dispatch never proves image consumption.
use super::clipboard_delivery::{self, ClipboardDelivery, DispatchError};
use crate::types::InsertStatus;
use openless_core::dictation_references::DeliveryPart;

pub(crate) trait MaterialClipboard: ClipboardDelivery {
    fn set_image(&mut self, path: &std::path::Path) -> Result<(), String>;
}

pub(crate) fn deliver(
    clipboard: &mut impl MaterialClipboard,
    parts: &[DeliveryPart],
    restore_after_paste: bool,
    mut still_current: impl FnMut(bool) -> bool,
) -> InsertStatus {
    let _delivery = clipboard_delivery::lock();
    if !still_current(false) { return InsertStatus::NotRequested; }
    let previous = clipboard.text().ok().flatten();
    let mut dispatched = false;
    let mut last_sequence = None;
    for part in parts {
        if !still_current(dispatched) { return if dispatched { InsertStatus::PasteSent } else { InsertStatus::NotRequested }; }
        let written = match part {
            DeliveryPart::Text(text) => clipboard.set_text(text),
            DeliveryPart::Image(path) => clipboard.set_image(path),
        };
        if let Err(error) = written {
            log::warn!("[references] clipboard material write: {error}");
            // Image paths remain in the following text; a failed image write
            // has not dispatched anything and needs no native retry.
            if matches!(part, DeliveryPart::Image(_)) { continue; }
            return if dispatched { InsertStatus::PasteSent } else { InsertStatus::Failed };
        }
        let revision = clipboard.sequence();
        if !still_current(dispatched) || !clipboard_delivery::revision_matches(revision, clipboard.sequence()) {
            return if dispatched { InsertStatus::PasteSent } else { InsertStatus::CopiedFallback };
        }
        match clipboard.paste() {
            Ok(()) => dispatched = true,
            Err(DispatchError::OutcomeUnknown) => return InsertStatus::PasteSent,
            Err(DispatchError::NotSent) => return if dispatched { InsertStatus::PasteSent } else { InsertStatus::CopiedFallback },
        }
        clipboard.wait_for_consumption();
        if clipboard.sequence() != revision { return InsertStatus::PasteSent; }
        last_sequence = revision;
    }
    if restore_after_paste && clipboard_delivery::revision_matches(last_sequence, clipboard.sequence()) {
        if let Some(previous) = previous { let _ = clipboard.set_text(&previous); }
    }
    if dispatched { InsertStatus::PasteSent } else { InsertStatus::Failed }
}
