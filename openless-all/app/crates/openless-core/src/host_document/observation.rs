use super::{minimal_edit, EditPair};

/// A bounded document snapshot anchored to one unambiguous insertion.
/// Native observers must additionally retain and verify the editor identity.
/// Offsets here are Unicode scalar offsets, never platform UTF-16 offsets.
pub struct ObservedInsertion {
    baseline: String,
    start: usize,
    end: usize,
    valid: bool,
}

impl ObservedInsertion {
    pub const MAX_DOCUMENT_UTF16: usize = 20_000;

    /// Prove that a new, unique insertion replaced one contiguous region. Merely
    /// finding the same text in an unchanged document is not a delivery receipt.
    pub fn delivered(before: &str, after: &str, inserted: &str) -> bool {
        if before == after || before.encode_utf16().count() > Self::MAX_DOCUMENT_UTF16 {
            return false;
        }
        if Self::new(after.to_owned(), inserted).is_none() {
            return false;
        }
        let start = after.find(inserted).unwrap();
        let prefix = &after[..start];
        let suffix = &after[start + inserted.len()..];
        before.len() >= prefix.len() + suffix.len()
            && before.starts_with(prefix)
            && before.ends_with(suffix)
    }

    pub fn new(document: String, inserted: &str) -> Option<Self> {
        if inserted.trim().is_empty() || document.encode_utf16().count() > Self::MAX_DOCUMENT_UTF16
        {
            return None;
        }
        // Include overlapping occurrences: "aaa" contains "aa" twice.
        let mut matches = document
            .char_indices()
            .filter_map(|(i, _)| document[i..].starts_with(inserted).then_some(i));
        let byte_start = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let start = document[..byte_start].chars().count();
        Some(Self {
            baseline: document,
            start,
            end: start + inserted.chars().count(),
            valid: true,
        })
    }

    /// Call after the editor settles. A rejected deletion retains the baseline
    /// so delete-then-type remains a replacement. An out-of-span change ends
    /// this observation rather than guessing how the insertion moved.
    pub fn observe(&mut self, current: &str, publish: impl FnOnce(EditPair) -> bool) -> bool {
        if !self.valid {
            return false;
        }
        if current.encode_utf16().count() > Self::MAX_DOCUMENT_UTF16 {
            self.valid = false;
            return false;
        }
        if current == self.baseline {
            return true;
        }
        let old: Vec<char> = self.baseline.chars().collect();
        let new: Vec<char> = current.chars().collect();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix_limit = (old.len() - prefix).min(new.len() - prefix);
        let suffix = (0..suffix_limit)
            .take_while(|i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
            .count();
        let old_end = old.len() - suffix;
        if prefix < self.start || old_end > self.end {
            self.valid = false;
            return false;
        }
        // Supply only insertion-local context. Single-character vocabulary
        // expansion must not pull unrelated neighboring document text in.
        let old_span: String = old[self.start..self.end].iter().collect();
        let new_end = self.end - (old_end - prefix) + (new.len() - suffix - prefix);
        let new_span: String = new[self.start..new_end].iter().collect();
        if let Some(edit) = minimal_edit(&old_span, &new_span) {
            if publish(edit) {
                self.baseline = current.to_owned();
                self.end = new_end;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_requires_a_real_insertion_and_unchanged_surroundings() {
        assert!(!ObservedInsertion::delivered(
            "existing word",
            "existing word",
            "word"
        ));
        assert!(ObservedInsertion::delivered("前😀后", "前😀新词后", "新词"));
        assert!(ObservedInsertion::delivered("前旧词后", "前新词后", "新词"));
        assert!(!ObservedInsertion::delivered(
            "前旧词后",
            "别处新词后",
            "新词"
        ));
        assert!(!ObservedInsertion::delivered("word", "word word", "word"));
    }

    #[test]
    fn rejects_ambiguous_and_missing_insertions() {
        assert!(ObservedInsertion::new("aaa".into(), "aa").is_none());
        assert!(ObservedInsertion::new("old".into(), "new").is_none());
        assert!(ObservedInsertion::new(" ".into(), " ").is_none());
    }

    #[test]
    fn same_word_outside_insertion_ends_observation() {
        let mut observed = ObservedInsertion::new("大禹。今天讲大禹".into(), "今天讲大禹").unwrap();
        assert!(!observed.observe("大鱼。今天讲大禹", |_| panic!("outside insertion")));
        assert!(!observed.observe("大禹。今天讲大鱼", |_| panic!("already ended")));
    }

    #[test]
    fn deletion_then_typing_preserves_original_and_unicode_offsets() {
        let mut observed =
            ObservedInsertion::new("😀前文：今天讲大禹。后文".into(), "今天讲大禹").unwrap();
        assert!(observed.observe("😀前文：今天讲。后文", |_| false));
        let mut pair = None;
        assert!(observed.observe("😀前文：今天讲Codex。后文", |edit| {
            pair = Some(edit);
            true
        }));
        let pair = pair.unwrap();
        assert_eq!(pair.source, "大禹");
        assert_eq!(pair.target, "Codex");
        assert_eq!(pair.before, "今天讲");
        assert_eq!(pair.after, "");
        assert!(observed.observe("😀前文：今天讲Claude。后文", |edit| {
            assert_eq!(edit.source, "odex");
            true
        }));
    }

    #[test]
    fn oversized_documents_are_never_observed() {
        assert!(ObservedInsertion::new("a".repeat(20_001), "a").is_none());
        let mut observed = ObservedInsertion::new("hello".into(), "hello").unwrap();
        assert!(!observed.observe(&"a".repeat(20_001), |_| panic!("oversized")));
    }
}
