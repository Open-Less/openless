//! Admission for an independently tracked insertion, never text/length matching.

use super::{minimal_edit, EditPair};

/// Host-local identity of one frozen owner and one successful delivery in one
/// recording. Not IPC/model/history data. A future trusted editor adapter must
/// obtain these identities from its owner, not from the document's contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnedRangeIdentity {
    pub session_id: crate::SessionId,
    pub owner_id: uuid::Uuid,
    pub insertion_id: uuid::Uuid,
}

/// Exact edit-span callback from that owner. Every edit revision must arrive,
/// including edits outside the insertion (which invalidate this observation).
/// AXValueChanged, field length, caret and matching text cannot supply this.
pub struct OwnedRangeChange {
    pub identity: OwnedRangeIdentity,
    pub revision: u64,
    pub start: usize,
    pub removed_utf16: usize,
    pub inserted_utf16: usize,
    pub total_before: usize,
    pub total_after: usize,
}

/// The same trusted owner must attest the revision AND selection under which
/// the bounded read runs. The read closure must revalidate that revision before
/// returning. Ordinary AX/UIA snapshots cannot manufacture this evidence.
pub struct OwnedRangeRevision {
    pub identity: OwnedRangeIdentity,
    pub revision: u64,
    pub total: usize,
    pub selection: (usize, usize),
}

pub struct RangeObservation {
    identity: OwnedRangeIdentity,
    start: usize,
    original: String,
    total: usize,
    valid: bool,
    length: usize,
    revision: u64,
    sampled_revision: u64,
}

impl RangeObservation {
    pub const MAX_INSERTION_UTF16: usize = 16_384;

    /// Only a successful delivery receipt from the trusted owner can establish
    /// revision zero. This alone does not authorize any subsequent text read.
    pub fn delivered(
        identity: OwnedRangeIdentity,
        start: usize,
        original: String,
        total: usize,
    ) -> Option<Self> {
        let length = original.encode_utf16().count();
        if original.trim().is_empty()
            || length > Self::MAX_INSERTION_UTF16
            || start.checked_add(length)? > total
        {
            return None;
        }
        Some(Self {
            identity,
            start,
            original,
            total,
            valid: true,
            length,
            revision: 0,
            sampled_revision: 0,
        })
    }

    pub fn original(&self) -> &str {
        &self.original
    }

    pub fn record_local_change(&mut self, change: OwnedRangeChange) -> bool {
        let valid = self.valid
            && change.identity == self.identity
            && Some(change.revision) == self.revision.checked_add(1)
            && change.total_before == self.total
            && change.start >= self.start
            && change
                .start
                .checked_add(change.removed_utf16)
                .is_some_and(|end| end <= self.start + self.length)
            && self
                .total
                .checked_sub(change.removed_utf16)
                .and_then(|total| total.checked_add(change.inserted_utf16))
                == Some(change.total_after);
        let length = self
            .length
            .checked_sub(change.removed_utf16)
            .and_then(|length| length.checked_add(change.inserted_utf16));
        if !valid || length.is_none_or(|length| length > Self::MAX_INSERTION_UTF16) {
            self.invalidate();
            return false;
        }
        self.length = length.unwrap();
        self.total = change.total_after;
        self.revision = change.revision;
        true
    }

    /// Shared native-entry/sampling gate. Missing evidence permanently rejects
    /// even a single equal-length batch; a restored length cannot revive it.
    /// Coordinates and text equality are never a substitute for continuity.
    pub fn admit_revision(
        &mut self,
        evidence: Option<OwnedRangeRevision>,
    ) -> Option<(usize, usize)> {
        let accepted = evidence.is_some_and(|evidence| {
            self.valid
                && evidence.identity == self.identity
                && evidence.revision == self.revision
                && self.revision > self.sampled_revision
                && evidence.total == self.total
                && evidence.selection.0 >= self.start
                && evidence
                    .selection
                    .0
                    .checked_add(evidence.selection.1)
                    .is_some_and(|end| end <= self.start + self.length)
        });
        if !accepted {
            self.invalidate();
            return None;
        }
        self.sampled_revision = self.revision;
        Some((self.start, self.length))
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Admission precedes ALL subsequent body reads. The only readable range
    /// is the independently tracked insertion, in native UTF-16 coordinates.
    /// This is a contract for a future trusted adapter, not AX/UIA support.
    pub fn sample_revision(
        &mut self,
        evidence: Option<OwnedRangeRevision>,
        read: impl FnOnce(usize, usize) -> Option<String>,
        publish: impl FnOnce(EditPair) -> bool,
    ) -> bool {
        let Some((start, length)) = self.admit_revision(evidence) else {
            return false;
        };
        let Some(current) = read(start, length) else {
            self.invalidate();
            return false;
        };
        if current.encode_utf16().count() != length {
            self.invalidate();
            return false;
        }
        if let Some(edit) = minimal_edit(&self.original, &current) {
            publish(edit);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> OwnedRangeIdentity {
        OwnedRangeIdentity {
            session_id: crate::SessionId::from_uuid(uuid::Uuid::from_u128(1)),
            owner_id: uuid::Uuid::from_u128(2),
            insertion_id: uuid::Uuid::from_u128(3),
        }
    }

    #[test]
    fn trusted_owner_delete_then_type_and_multiple_corrections_contract() {
        // Synthetic trusted editor callbacks, NOT evidence of native AX support.
        let identity = identity();
        let original = "请使用 codx 完成任务";
        let mut range = RangeObservation::delivered(identity, 10, original.into(), 40).unwrap();
        for (revision, removed, inserted, before, after, text) in [
            (1, 4, 0, 40, 36, "请使用  完成任务"),
            (2, 0, 5, 36, 41, "请使用 codex 完成任务"),
            (3, 1, 1, 41, 41, "请使用 Codex 完成任务"),
        ] {
            assert!(range.record_local_change(OwnedRangeChange {
                identity,
                revision,
                start: 14,
                removed_utf16: removed,
                inserted_utf16: inserted,
                total_before: before,
                total_after: after,
            }));
            assert!(range.sample_revision(
                Some(OwnedRangeRevision {
                    identity,
                    revision,
                    total: after,
                    selection: (14, 0),
                }),
                |start, length| {
                    assert_eq!(start, 10);
                    assert_eq!(length, text.encode_utf16().count());
                    Some(text.into())
                },
                |edit| {
                    assert!(original.starts_with(&edit.before));
                    true
                },
            ));
        }
    }

    #[test]
    fn missing_or_wrong_owner_revision_selection_never_reads_and_cannot_revive() {
        for failure in 0..7 {
            let identity = identity();
            let mut range = RangeObservation::delivered(identity, 10, "codx".into(), 20).unwrap();
            assert!(range.record_local_change(OwnedRangeChange {
                identity,
                revision: 1,
                start: 10,
                removed_utf16: 4,
                inserted_utf16: 5,
                total_before: 20,
                total_after: 21,
            }));
            let mut evidence = OwnedRangeRevision {
                identity,
                revision: 1,
                total: 21,
                selection: (15, 0),
            };
            match failure {
                1 => {
                    evidence.identity.session_id =
                        crate::SessionId::from_uuid(uuid::Uuid::from_u128(4))
                }
                2 => evidence.identity.owner_id = uuid::Uuid::from_u128(4),
                3 => evidence.identity.insertion_id = uuid::Uuid::from_u128(4),
                4 => evidence.selection = (0, 0),
                5 => evidence.revision = 2,
                6 => evidence.total = 20,
                _ => {}
            }
            assert!(!range.sample_revision(
                (failure != 0).then_some(evidence),
                |_, _| panic!("unproven ownership must precede all body reads"),
                |_| panic!("unproven ownership must precede all sinks"),
            ));
            assert!(range
                .admit_revision(Some(OwnedRangeRevision {
                    identity,
                    revision: 1,
                    total: 21,
                    selection: (15, 0),
                }))
                .is_none());
        }
    }

    #[test]
    fn skipped_or_remote_edit_span_invalidates_before_sampling() {
        for (revision, start) in [(2, 10), (1, 0), (1, 19)] {
            let identity = identity();
            let mut range = RangeObservation::delivered(identity, 10, "codx".into(), 20).unwrap();
            assert!(!range.record_local_change(OwnedRangeChange {
                identity,
                revision,
                start,
                removed_utf16: 1,
                inserted_utf16: 1,
                total_before: 20,
                total_after: 20,
            }));
            assert!(!range.sample_revision(
                Some(OwnedRangeRevision {
                    identity,
                    revision,
                    total: 20,
                    selection: (14, 0),
                }),
                |_, _| panic!("skipped/remote edit is not an insertion sample"),
                |_| panic!("skipped/remote edit is not learning evidence"),
            ));
        }
    }

    #[test]
    fn unproven_remote_append_with_repeated_newlines_never_reads_a_body() {
        let original = "今天先完成这个工作然后再讨论";
        let total = original.encode_utf16().count() + 66;
        let mut range = RangeObservation::delivered(identity(), 0, original.into(), total).unwrap();
        let field_after_remote_append = format!("{original}{}TAIL\n", "\n".repeat(62));
        assert_eq!(field_after_remote_append.encode_utf16().count(), total + 1);
        assert!(!range.sample_revision(
            None,
            |_, _| panic!("field length and repeated newlines never authorize text reads"),
            |_| panic!("remote append must not become word or expression evidence"),
        ));
    }
}
