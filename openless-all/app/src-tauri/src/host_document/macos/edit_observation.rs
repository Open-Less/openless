//! Ordinary AX does not provide insertion-range continuity across edits.
//!
//! AXObserverCallback reports an element and notification, not an edit span or
//! revision. AX text markers have provider-defined payloads (including numeric
//! indices); their mere availability does not prove tracking across edits.
//! No verified trusted editor owner is integrated here. Do not arm an observer,
//! read subsequent body text, or infer identity from anchors/total/caret.
//! Startup context, ordinary delivery and explicit vocabulary APIs are separate.

use super::*;
use openless_core::host_document::RangeObservation;

// Keep the insertion adapter's optional receipt seam without changing ordinary
// input. No production constructor exists until a trusted owner is integrated.
pub(crate) struct LearningStart;

pub(crate) struct DeliveredInsertion {
    pub(crate) target: crate::selection::SelectionInsertionTarget,
    proof: RangeObservation,
}

impl DeliveredInsertion {
    pub(crate) fn matches(&self, inserted: &str) -> bool {
        self.proof.original() == inserted
    }
}

pub(crate) fn prepare(
    _target: &crate::selection::SelectionInsertionTarget,
) -> Option<LearningStart> {
    // Frozen control identity proves the owner, not the identity of its text
    // after arbitrary edits. Do not collect extra text for an ineligible watch.
    None
}

pub(crate) fn complete(
    _target: crate::selection::SelectionInsertionTarget,
    _start: LearningStart,
    _inserted: String,
) -> Option<DeliveredInsertion> {
    None
}

pub(crate) fn spawn(
    mut delivery: DeliveredInsertion,
    _lifetime: Duration,
    _on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
    on_end: Box<dyn Fn() + Send + Sync>,
) -> Option<Arc<AtomicBool>> {
    // Defense at the actual native entry, using the Core sampling admission.
    // AX supplies no OwnedRangeRevision. Even an old/future receipt cannot
    // authorize a read through this entry. No observer/thread is started.
    let admitted = delivery.proof.admit_revision(None);
    on_end();
    admitted?;
    None
}
