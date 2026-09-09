//! Bounded, position-anchored edit tracking. No OS calls and no persistence.
use super::{learned_rule, minimal_edit, EditPair};

pub(super) const MAX_FIELD_CHARS: usize = 8192;

pub(super) struct EditSession {
    prefix: String,
    suffix: String,
    baseline: String,
}

impl EditSession {
    pub fn anchor(field: &str, inserted: &str) -> Option<Self> {
        let inserted = inserted.trim_end();
        if inserted.is_empty() || field.chars().count() > MAX_FIELD_CHARS {
            return None;
        }
        let mut matches = field.match_indices(inserted);
        let (offset, _) = matches.next()?;
        // Ambiguous location: never learn edits to another occurrence.
        if matches.next().is_some() {
            return None;
        }
        Some(Self {
            prefix: field[..offset].into(),
            suffix: field[offset + inserted.len()..].into(),
            baseline: inserted.into(),
        })
    }

    /// None means the original region can no longer be tracked safely.
    pub fn region<'a>(&self, field: &'a str) -> Option<&'a str> {
        if field.chars().count() > MAX_FIELD_CHARS {
            return None;
        }
        field.strip_prefix(&self.prefix)?.strip_suffix(&self.suffix)
    }

    /// Called only after the input has been stable (IME typing debounce).
    pub fn settled_edit(&mut self, field: &str) -> Option<EditPair> {
        let current = self.region(field)?.to_string();
        let edit = minimal_edit(&self.baseline, &current)?;
        // Keep the baseline through delete-then-retype and reject sentence rewrites.
        learned_rule(&edit)?;
        self.baseline = current;
        Some(edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_only_the_inserted_region() {
        let mut s =
            EditSession::anchor("前文。今天讨论大禹养殖。后文", "今天讨论大禹养殖。").unwrap();
        assert!(s.region("改前文。今天讨论大禹养殖。后文").is_none());
        let edit = s.settled_edit("前文。今天讨论大鱼养殖。后文").unwrap();
        let rule = learned_rule(&edit).unwrap();
        assert_eq!(
            (rule.pattern.as_str(), rule.replacement.as_str()),
            ("大禹", "大鱼")
        );
        assert!(s.settled_edit("前文。今天讨论大鱼养殖。后文").is_none());
    }

    #[test]
    fn rejects_ambiguous_and_oversized_fields() {
        assert!(EditSession::anchor("重复重复", "重复").is_none());
        assert!(EditSession::anchor(&"字".repeat(MAX_FIELD_CHARS + 1), "字").is_none());
        assert!(EditSession::anchor("空白", "").is_none());
    }

    #[test]
    fn delete_then_retype_does_not_lose_the_original_word() {
        let mut s = EditSession::anchor("请用扣德克斯。", "请用扣德克斯。").unwrap();
        assert!(s.settled_edit("请用。").is_none());
        let edit = s.settled_edit("请用Codex。").unwrap();
        assert_eq!(edit.source, "扣德克斯");
        assert_eq!(edit.target, "Codex");
    }

    #[test]
    fn rejects_append_send_and_sentence_rewrite() {
        let mut s = EditSession::anchor("今天讨论大禹养殖。", "今天讨论大禹养殖。").unwrap();
        assert!(s.settled_edit("今天讨论大禹养殖。继续输入").is_none());
        assert!(s.settled_edit("").is_none());
        assert!(s.settled_edit("明天不用开会了，取消安排。").is_none());
    }
}
