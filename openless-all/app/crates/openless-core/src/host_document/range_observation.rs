//! Explicit UTF-16 insertion range, not a search/diff over a whole field.

use super::{minimal_edit, EditPair};

pub struct RangeObservation {
    start: usize,
    original: String,
    total: usize,
    before: String,
    after: String,
    valid: bool,
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
    /// byte/character indices. Length change must also pass anchor/caret proof.
    pub fn range(&self, total: usize) -> Option<(usize, usize)> {
        if !self.valid {
            return None;
        }
        let outside = self
            .total
            .checked_sub(self.original.encode_utf16().count())?;
        let length = total.checked_sub(outside)?;
        (length <= Self::MAX_INSERTION_UTF16 && self.start.checked_add(length)? <= total)
            .then_some((self.start, length))
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
            101,
            "请使用 codex 完成任务",
            "😀前文",
            "原后文",
            (50, 0),
            |edit| {
                pairs.push(edit);
                true
            }
        ));
        assert_eq!(pairs[0].source, "x");
        assert_eq!(pairs[0].target, "ex");
        assert!(observed.observe(
            101,
            "请使用 Codex 完成任务",
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
                101,
                "请使用 codex 完成任务",
                before,
                "原后文",
                caret,
                |_| { panic!("outside observation must not publish") }
            ));
            assert!(observed.range(100).is_none());
        }
    }
}
