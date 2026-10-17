use unicode_normalization::char::is_combining_mark;

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
    if source.chars().count().max(target.chars().count()) > MAX_EDIT_CHARS
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
    let mut rules = learned_rules_with_max_chars(edit, max_chars);
    // The singular API must not silently discard a second correction.
    (rules.len() == 1).then(|| rules.remove(0))
}

/// Split bounded character changes before recovering vocabulary boundaries.
/// Formatting changes and continued typing must not swallow a word correction.
/// This is deliberately local: CJK context remains a conservative two-character
/// heuristic, not a claim to semantic word segmentation.
pub fn learned_rules_with_max_chars(edit: &EditPair, max_chars: usize) -> Vec<LearnedRule> {
    let source: Vec<char> = edit.source.chars().collect();
    let target: Vec<char> = edit.target.chars().collect();
    if source.len().max(target.len()) > MAX_EDIT_CHARS {
        return Vec::new();
    }
    let before: Vec<char> = edit.before.chars().collect();
    let after: Vec<char> = edit.after.chars().collect();
    let old: Vec<char> = before
        .iter()
        .chain(&source)
        .chain(&after)
        .copied()
        .collect();
    let new: Vec<char> = before
        .iter()
        .chain(&target)
        .chain(&after)
        .copied()
        .collect();
    let mut rules = Vec::new();
    for (old_start, old_end, new_start, new_end) in changed_ranges(&source, &target) {
        let (old_start, old_end) =
            trim_formatting(&old, old_start + before.len(), old_end + before.len());
        let (new_start, new_end) =
            trim_formatting(&new, new_start + before.len(), new_end + before.len());
        let old_word = identifier_span(&old, old_start, old_end);
        let new_word = identifier_span(&new, new_start, new_end);
        let (pattern, replacement) = match (old_word, new_word) {
            (Some((a, b)), Some((c, d))) => (
                old[a..b].iter().collect::<String>(),
                new[c..d].iter().collect::<String>(),
            ),
            (Some((a, b)), None)
                if new_start < new_end && new[new_start..new_end].iter().all(|&c| is_cjk(c)) =>
            {
                (
                    old[a..b].iter().collect(),
                    new[new_start..new_end].iter().collect(),
                )
            }
            (None, Some((c, d)))
                if old_start < old_end && old[old_start..old_end].iter().all(|&c| is_cjk(c)) =>
            {
                (
                    old[old_start..old_end].iter().collect(),
                    new[c..d].iter().collect(),
                )
            }
            // Do not join a partial identifier to neighboring punctuation.
            (Some(_), None) | (None, Some(_)) => continue,
            (None, None) => {
                if old_start == old_end
                    || new_start == new_end
                    || !old[old_start..old_end].iter().all(|&c| is_cjk(c))
                    || !new[new_start..new_end].iter().all(|&c| is_cjk(c))
                {
                    continue;
                }
                let mut a = old_start;
                let mut b = old_end;
                let mut c = new_start;
                let mut d = new_end;
                while b - a < MIN_PATTERN_CHARS {
                    if a > 0 && c > 0 && old[a - 1] == new[c - 1] && is_cjk(old[a - 1]) {
                        a -= 1;
                        c -= 1;
                    } else if b < old.len() && d < new.len() && old[b] == new[d] && is_cjk(old[b]) {
                        b += 1;
                        d += 1;
                    } else {
                        break;
                    }
                }
                (old[a..b].iter().collect(), new[c..d].iter().collect())
            }
        };
        let candidate = EditPair {
            source: pattern.clone(),
            target: replacement.clone(),
            before: String::new(),
            after: String::new(),
        };
        if pattern != replacement
            && pattern.chars().count() >= MIN_PATTERN_CHARS
            && is_vocab_worthy_with_max_chars(&candidate, max_chars)
            && pattern.chars().any(char::is_alphabetic)
            && replacement.chars().any(char::is_alphabetic)
        {
            let rule = LearnedRule {
                pattern,
                replacement,
            };
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    }
    rules
}

/// LCS is quadratic only in the changed region, capped at 64 scalars per side.
/// Equal runs separate independent edits; adjacent delete/insert steps form
/// one replacement so a spelling correction is never learned as two fragments.
fn changed_ranges(old: &[char], new: &[char]) -> Vec<(usize, usize, usize, usize)> {
    let mut lcs = vec![vec![0; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            lcs[i][j] = if old[i] == new[j] {
                1 + lcs[i + 1][j + 1]
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut ranges = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            i += 1;
            j += 1;
            continue;
        }
        let (a, b) = (i, j);
        while i < old.len() || j < new.len() {
            if i < old.len() && j < new.len() && old[i] == new[j] {
                break;
            }
            if j == new.len() || (i < old.len() && lcs[i + 1][j] >= lcs[i][j + 1]) {
                i += 1;
            } else {
                j += 1;
            }
        }
        ranges.push((a, i, b, j));
    }
    ranges
}

fn identifier_char(text: &[char], index: usize) -> bool {
    let c = text[index];
    c.is_ascii_alphanumeric()
        || (matches!(c, '_' | '-' | '.' | '\'')
            && index > 0
            && index + 1 < text.len()
            && text[index - 1].is_ascii_alphanumeric()
            && text[index + 1].is_ascii_alphanumeric())
}

fn identifier_span(text: &[char], start: usize, end: usize) -> Option<(usize, usize)> {
    // An empty diff inside a word is a spelling insertion/deletion. At a word
    // edge it could just be new typing, so require characters on both sides.
    if start == end {
        if start == 0
            || start == text.len()
            || !identifier_char(text, start - 1)
            || !identifier_char(text, start)
        {
            return None;
        }
    } else if !(start..end).all(|i| identifier_char(text, i)) {
        return None;
    }
    let (mut a, mut b) = (start, end);
    while a > 0 && identifier_char(text, a - 1) {
        a -= 1;
    }
    while b < text.len() && identifier_char(text, b) {
        b += 1;
    }
    // ASCII recovery must not truncate an accented/combining-script word.
    let non_ascii_word =
        |c: char| (!c.is_ascii() && c.is_alphanumeric() && !is_cjk(c)) || is_combining_mark(c);
    if (a > 0 && non_ascii_word(text[a - 1])) || (b < text.len() && non_ascii_word(text[b])) {
        return None;
    }
    // A component of a URL, filesystem path or email is not a vocabulary word.
    if (a > 0 && matches!(text[a - 1], '/' | '\\' | '@' | ':'))
        || (b < text.len() && matches!(text[b], '/' | '\\' | '@' | ':'))
    {
        return None;
    }
    Some((a, b))
}

fn trim_formatting(text: &[char], mut start: usize, mut end: usize) -> (usize, usize) {
    // Strip only outer formatting. Internal dots/hyphens remain part of an
    // identifier; internal sentence boundaries still reject a phrase rewrite.
    let formatting = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '。' | '，'
                    | '、'
                    | '？'
                    | '！'
                    | '；'
                    | '：'
                    | '.'
                    | ','
                    | '?'
                    | '!'
                    | ';'
                    | ':'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '"'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
            )
    };
    while start < end && formatting(text[start]) && !identifier_char(text, start) {
        start += 1;
    }
    while start < end && formatting(text[end - 1]) && !identifier_char(text, end - 1) {
        end -= 1;
    }
    (start, end)
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{20000}'..='\u{323AF}')
}

fn crosses_boundary(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars
        .iter()
        .enumerate()
        .any(|(i, &c)| !c.is_alphabetic() && !identifier_char(&chars, i))
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
    fn assert_rule(before: &str, after: &str, pattern: &str, replacement: &str) {
        let edit = minimal_edit(before, after).unwrap();
        assert_eq!(
            learned_rule(&edit),
            Some(LearnedRule {
                pattern: pattern.into(),
                replacement: replacement.into(),
            }),
            "{before:?} -> {after:?}"
        );
    }

    #[test]
    fn restores_complete_identifiers_at_every_spelling_position() {
        for (before, after, pattern, replacement) in [
            ("你好，ZIP", "你好，VIP", "ZIP", "VIP"),
            ("你好，ZIP。", "你好，VIP。", "ZIP", "VIP"),
            ("你好， ZIP", "你好， VIP", "ZIP", "VIP"),
            ("我用codez", "我用codex", "codez", "codex"),
            ("我用codx", "我用codex", "codx", "codex"),
            ("我用codeex", "我用codex", "codeex", "codex"),
            ("使用Qwen2", "使用Qwen3", "Qwen2", "Qwen3"),
            ("使用GPT-6.0。", "使用GPT-6.1。", "GPT-6.0", "GPT-6.1"),
            ("用gpt_oss", "用gpt_osx", "gpt_oss", "gpt_osx"),
            ("(ZIP)", "(VIP)", "ZIP", "VIP"),
            ("今天讲大禹", "今天讲Codex", "大禹", "Codex"),
        ] {
            assert_rule(before, after, pattern, replacement);
        }
    }

    #[test]
    fn separates_word_corrections_from_formatting_and_continued_typing() {
        for (before, after, pattern, replacement) in [
            ("你好，ZIP", "你好，VIP。", "ZIP", "VIP"),
            ("你好,ZIP.", "你好，VIP!", "ZIP", "VIP"),
            ("你好，ZIP", "你好，Codex。", "ZIP", "Codex"),
            ("你好，ZIP", "你好，VIP。继续写下一句", "ZIP", "VIP"),
        ] {
            assert_rule(before, after, pattern, replacement);
        }
    }

    #[test]
    fn handles_multiple_word_corrections_without_fragment_duplicates() {
        let edit = minimal_edit("ZIP和codx", "VIP和codex。").unwrap();
        assert_eq!(
            learned_rules_with_max_chars(&edit, 12),
            vec![
                LearnedRule {
                    pattern: "ZIP".into(),
                    replacement: "VIP".into()
                },
                LearnedRule {
                    pattern: "codx".into(),
                    replacement: "codex".into()
                },
            ]
        );
        assert!(learned_rule(&edit).is_none());
        assert_rule("gpt_oss", "GPT_OSS", "gpt_oss", "GPT_OSS");
    }

    #[test]
    fn rejects_formatting_deletion_new_words_and_non_vocabulary_values() {
        for (before, after) in [
            ("你好，ZIP", "你好，ZIP。"),
            ("你好，ZIP。", "你好,ZIP!"),
            ("你好 ZIP", "你好  ZIP"),
            ("ZIP", ""),
            ("你好", "你好 VIP"),
            ("123", "124"),
            ("https://codx.com", "https://codex.com"),
            ("/tmp/codx", "/tmp/codex"),
            ("codx@example.com", "codex@example.com"),
            ("https://foo.com", "https://bar.com"),
            ("/tmp/foo", "/tmp/bar"),
            ("foo@example.com", "bar@example.com"),
            ("，甲", "，乙"),
            ("😀甲", "😀乙"),
            ("café", "cazé"),
            ("cafe\u{301}", "cake\u{301}"),
        ] {
            if let Some(edit) = minimal_edit(before, after) {
                assert!(
                    learned_rules_with_max_chars(&edit, 32).is_empty(),
                    "{before:?} -> {after:?}"
                );
            }
        }
    }

    #[test]
    fn validates_expanded_word_length_without_truncating_it() {
        let edit = minimal_edit("abcdefZ", "abcdefV").unwrap();
        assert!(learned_rules_with_max_chars(&edit, 6).is_empty());
        assert_eq!(
            learned_rule_with_max_chars(&edit, 7).unwrap().replacement,
            "abcdefV"
        );
        let edit = EditPair {
            source: "a".repeat(65),
            target: "b".into(),
            before: String::new(),
            after: String::new(),
        };
        assert!(learned_rules_with_max_chars(&edit, 100).is_empty());
    }
}
