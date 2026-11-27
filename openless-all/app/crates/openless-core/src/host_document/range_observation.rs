//! Explicit UTF-16 insertion range, not a search/diff over a whole field.

use super::{minimal_edit, EditPair};

pub struct RangeObservation {
    start: usize,
    original: String,
    total: usize,
    before: String,
    after: String,
    valid: bool,
    length: usize,
    revision: u64,
}

/// Host-only evidence from a native range/edit owner, never inferred from
/// field length, caret or matching text. Plain AXValueChanged has no such
/// payload and must not manufacture this record.
pub struct OwnedRangeChange {
    pub revision: u64,
    pub start: usize,
    pub removed_utf16: usize,
    pub inserted_utf16: usize,
    pub total_before: usize,
    pub total_after: usize,
}

impl RangeObservation {
    pub const MAX_INSERTION_UTF16: usize = 16_384;

    /// The host must first verify delivery at this exact start, the expected
    /// collapsed caret, document length and both unchanged boundary anchors.
    pub fn delivered(
        start: usize,
        original: String,
        total: usize,
        before: String,
        after: String,
    ) -> Option<Self> {
        let length = original.encode_utf16().count();
        if original.trim().is_empty()
            || length > Self::MAX_INSERTION_UTF16
            || start.checked_add(length)? > total
            || before.encode_utf16().count() > start
            || after.encode_utf16().count() > total - start - length
        {
            return None;
        }
        Some(Self {
            start,
            original,
            total,
            before,
            after,
            valid: true,
            length,
            revision: 0,
        })
    }

    pub fn original(&self) -> &str {
        &self.original
    }
    pub fn before(&self) -> &str {
        &self.before
    }
    pub fn after(&self) -> &str {
        &self.after
    }

    /// Native range APIs receive these UTF-16 coordinates. They are never Rust
    /// byte/character indices. Field length is a rejection condition, never
    /// evidence that the inserted range itself grew or shrank.
    pub fn range(&self, total: usize) -> Option<(usize, usize)> {
        if !self.valid {
            return None;
        }
        (total == self.total).then_some((self.start, self.length))
    }

    pub fn known_range(&self) -> Option<(usize, usize)> {
        self.valid.then_some((self.start, self.length))
    }

    /// Only a real native owner reporting the exact edit span can advance this
    /// boundary. This is intentionally not wired to AX value/selection events.
    pub fn record_local_change(&mut self, change: OwnedRangeChange) -> bool {
        let valid = self.valid
            && change.revision == self.revision.saturating_add(1)
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

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Called only after native events settle and two owner-scoped reads agree.
    /// Keep the original insertion as comparison baseline: subsequent reports
    /// stay in Core's original coordinates, including delete-then-type.
    pub fn observe(
        &mut self,
        total: usize,
        current: &str,
        before: &str,
        after: &str,
        caret: (usize, usize),
        publish: impl FnOnce(EditPair) -> bool,
    ) -> bool {
        let Some((start, length)) = self.range(total) else {
            self.invalidate();
            return false;
        };
        if current.encode_utf16().count() != length
            || before != self.before
            || after != self.after
            || caret.0 < start
            || caret
                .0
                .checked_add(caret.1)
                .is_none_or(|end| end > start + length)
        {
            self.invalidate();
            return false;
        }
        if let Some(edit) = minimal_edit(&self.original, current) {
            publish(edit);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_document_words_are_irrelevant_to_an_explicit_inserted_range() {
        let mut observed = RangeObservation::delivered(
            40,
            "请使用 codx 完成任务".into(),
            100,
            "😀前文".into(),
            "原后文".into(),
        )
        .unwrap();
        let mut pairs = Vec::new();
        assert!(observed.observe(
            100,
            "请使用 code 完成任务",
            "😀前文",
            "原后文",
            (50, 0),
            |edit| {
                pairs.push(edit);
                true
            }
        ));
        assert_eq!(pairs[0].source, "x");
        assert_eq!(pairs[0].target, "e");
        assert!(observed.observe(
            100,
            "请使用 Code 完成任务",
            "😀前文",
            "原后文",
            (50, 0),
            |edit| {
                assert_eq!(edit.before, "请使用 ");
                true
            }
        ));
    }

    #[test]
    fn changed_anchor_or_caret_outside_the_inserted_range_permanently_ends_collection() {
        for (before, caret) in [("别处", (50, 0)), ("😀前文", (0, 0))] {
            let mut observed = RangeObservation::delivered(
                40,
                "请使用 codx 完成任务".into(),
                100,
                "😀前文".into(),
                "原后文".into(),
            )
            .unwrap();
            assert!(!observed.observe(
                100,
                "请使用 code 完成任务",
                before,
                "原后文",
                caret,
                |_| { panic!("outside observation must not publish") }
            ));
            assert!(observed.range(100).is_none());
        }
    }

    #[test]
    fn remote_append_with_repeated_newline_boundary_cannot_expand_or_read_a_sample() {
        let original = "今天先完成这个工作然后再讨论";
        let length = original.encode_utf16().count();
        let mut observed = RangeObservation::delivered(
            0,
            original.into(),
            length + 66,
            String::new(),
            "\n".repeat(32),
        )
        .unwrap();
        // Production host checks range before requesting any text. No expanded
        // read may occur, even though shifted 32-newline anchors would match.
        let mut text_reads = 0;
        if let Some((_start, _length)) = observed.range(length + 67) {
            text_reads += 1;
        }
        assert_eq!(text_reads, 0);
        assert_eq!(observed.range(length + 66), Some((0, length)));
        let mut published = 0;
        assert!(!observed.observe(
            length + 67,
            &(original.to_owned() + "\n"),
            "",
            &"\n".repeat(32),
            (length, 0),
            |_| {
                published += 1;
                true
            },
        ));
        assert_eq!(
            published, 0,
            "neither vocabulary nor expression may receive this edit"
        );
    }

    #[test]
    fn owner_proven_delete_then_type_and_multiple_edits_keep_original_coordinates() {
        let mut observed = RangeObservation::delivered(
            10,
            "请使用 codx 完成任务".into(),
            40,
            "原前文".into(),
            "原后文".into(),
        )
        .unwrap();
        let length = observed.original.encode_utf16().count();
        assert!(observed.record_local_change(OwnedRangeChange {
            revision: 1,
            start: 14,
            removed_utf16: 4,
            inserted_utf16: 0,
            total_before: 40,
            total_after: 36,
        }));
        assert_eq!(observed.range(36), Some((10, length - 4)));
        assert!(observed.observe(
            36,
            "请使用  完成任务",
            "原前文",
            "原后文",
            (14, 0),
            |_| false
        ));
        assert!(observed.record_local_change(OwnedRangeChange {
            revision: 2,
            start: 14,
            removed_utf16: 0,
            inserted_utf16: 5,
            total_before: 36,
            total_after: 41,
        }));
        assert!(observed.observe(
            41,
            "请使用 codex 完成任务",
            "原前文",
            "原后文",
            (19, 0),
            |edit| {
                assert_eq!((edit.source.as_str(), edit.target.as_str()), ("x", "ex"));
                true
            }
        ));
        assert!(observed.record_local_change(OwnedRangeChange {
            revision: 3,
            start: 14,
            removed_utf16: 1,
            inserted_utf16: 1,
            total_before: 41,
            total_after: 41,
        }));
        assert!(observed.observe(
            41,
            "请使用 Codex 完成任务",
            "原前文",
            "原后文",
            (19, 0),
            |edit| {
                assert_eq!(edit.before, "请使用 ");
                true
            }
        ));
    }

    #[test]
    fn plain_ax_can_resume_a_fixed_equal_length_correction_after_unlocated_delete_then_type() {
        let mut observed = RangeObservation::delivered(
            10,
            "请使用 codx 完成任务".into(),
            40,
            "原前文".into(),
            "原后文".into(),
        )
        .unwrap();
        assert!(
            observed.range(36).is_none(),
            "no intermediate text read without range evidence"
        );
        assert!(observed.observe(
            40,
            "请使用 code 完成任务",
            "原前文",
            "原后文",
            (18, 0),
            |edit| {
                assert_eq!((edit.source.as_str(), edit.target.as_str()), ("x", "e"));
                true
            }
        ));
        assert!(observed.observe(
            40,
            "请使用 Code 完成任务",
            "原前文",
            "原后文",
            (18, 0),
            |edit| {
                assert_eq!(edit.before, "请使用 ");
                true
            }
        ));
    }
}
