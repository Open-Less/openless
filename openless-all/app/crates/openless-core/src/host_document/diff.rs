const MAX_EDIT_CHARS: usize = 64;
const CONTEXT_CHARS: usize = 256;
const MIN_PATTERN_CHARS: usize = 2;
const MAX_PHRASE_CHARS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPair {
    pub source: String,
    pub target: String,
    pub before: String,
    pub after: String,
}

pub fn minimal_edit(before_text: &str, after_text: &str) -> Option<EditPair> {
    let before_text = before_text.trim_end();
    let after_text = after_text.trim_end();
    if before_text == after_text {
        return None;
    }
    let old: Vec<char> = before_text.chars().collect();
    let new: Vec<char> = after_text.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let max_suffix = (old.len() - prefix).min(new.len() - prefix);
    let suffix = (0..max_suffix)
        .take_while(|index| old[old.len() - 1 - index] == new[new.len() - 1 - index])
        .count();
    let source: String = old[prefix..old.len() - suffix].iter().collect();
    let target: String = new[prefix..new.len() - suffix].iter().collect();
    if source.is_empty()
        || source.chars().count().max(target.chars().count()) > MAX_EDIT_CHARS
        || source.trim().is_empty()
        || strip_whitespace(&source) == strip_whitespace(&target)
    {
        return None;
    }
    let before_start = prefix.saturating_sub(CONTEXT_CHARS);
    let after_start = old.len() - suffix;
    Some(EditPair {
        source,
        target,
        before: old[before_start..prefix].iter().collect(),
        after: old[after_start..(after_start + CONTEXT_CHARS).min(old.len())]
            .iter()
            .collect(),
    })
}

fn strip_whitespace(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedRule {
    pub pattern: String,
    pub replacement: String,
}

pub fn is_vocab_worthy(edit: &EditPair) -> bool {
    is_vocab_worthy_with_max_chars(edit, MAX_PHRASE_CHARS)
}

fn is_vocab_worthy_with_max_chars(edit: &EditPair, max_chars: usize) -> bool {
    let source = edit.source.trim();
    let target = edit.target.trim();
    !source.is_empty()
        && !target.is_empty()
        && !crosses_boundary(source)
        && !crosses_boundary(target)
        && source.chars().count() <= max_chars
        && target.chars().count() <= max_chars
}

pub fn learned_rule(edit: &EditPair) -> Option<LearnedRule> {
    learned_rule_with_max_chars(edit, MAX_PHRASE_CHARS)
}

pub fn learned_rule_with_max_chars(edit: &EditPair, max_chars: usize) -> Option<LearnedRule> {
    if !is_vocab_worthy_with_max_chars(edit, max_chars) {
        return None;
    }
    let before: Vec<char> = edit.before.chars().collect();
    let after: Vec<char> = edit.after.chars().collect();
    let mut left = 0;
    let mut right = 0;
    while edit.source.trim().chars().count() + left + right < MIN_PATTERN_CHARS {
        if before.len() > left && !before[before.len() - left - 1].is_whitespace() {
            left += 1;
        } else if after.len() > right && !after[right].is_whitespace() {
            right += 1;
        } else {
            return None;
        }
    }
    let prefix: String = before[before.len() - left..].iter().collect();
    let suffix: String = after[..right].iter().collect();
    let pattern = format!("{prefix}{}{suffix}", edit.source)
        .trim()
        .to_string();
    let replacement = format!("{prefix}{}{suffix}", edit.target)
        .trim()
        .to_string();
    (!pattern.is_empty()
        && !replacement.is_empty()
        && pattern.chars().count() <= max_chars
        && replacement.chars().count() <= max_chars)
        .then_some(LearnedRule {
            pattern,
            replacement,
        })
}

fn crosses_boundary(value: &str) -> bool {
    value.chars().any(|c| {
        matches!(
            c,
            '\n' | '\r' | '。' | '？' | '！' | '；' | '，' | '、' | '：' | '?' | '!' | ';'
        )
    })
}

pub fn edit_is_within_typed_text(edit: &EditPair, typed_text: &str) -> bool {
    !edit.source.is_empty() && typed_text.contains(&edit.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configurable_phrase_limit_counts_characters_and_expanded_context() {
        let mut edit = EditPair {
            source: "甲".repeat(13),
            target: "乙".repeat(13),
            before: String::new(),
            after: String::new(),
        };
        assert!(learned_rule(&edit).is_none());
        assert!(learned_rule_with_max_chars(&edit, 13).is_some());
        edit.source = "甲".into();
        edit.target = "乙丙".into();
        edit.before = "丁".into();
        assert!(learned_rule_with_max_chars(&edit, 2).is_none());
        assert_eq!(
            learned_rule_with_max_chars(&edit, 3).unwrap().replacement,
            "丁乙丙"
        );
        edit.target = "乙。丙".into();
        assert!(learned_rule_with_max_chars(&edit, 32).is_none());
    }

    #[test]
    fn diff_and_rule_are_char_safe() {
        let edit = minimal_edit("今天讲大禹", "今天讲大鱼").unwrap();
        assert_eq!((edit.source.as_str(), edit.target.as_str()), ("禹", "鱼"));
        let rule = learned_rule(&edit).unwrap();
        assert_eq!(
            (rule.pattern.as_str(), rule.replacement.as_str()),
            ("大禹", "大鱼")
        );
    }
}
