//! Conservative safeguards for locally observed vocabulary suggestions.

use crate::host_document::{EditPair, LearnedRule};
use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

fn normalized(value: &str) -> String {
    value.nfkc().flat_map(char::to_lowercase).collect()
}

fn number_signature(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_numeric()
                || "零〇一二两兩三四五六七八九十百千万萬亿億壹贰貳叁參肆伍陆陸柒捌玖拾佰仟"
                    .contains(*character)
        })
        .collect()
}

fn negation_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?-u:\b)(?:no|not|never|neither|nor)(?-u:\b)|n['’]t|[不没沒無无非别別]")
            .expect("fixed negation expression is valid")
    })
}

fn negation_signature(value: &str) -> Vec<&str> {
    negation_pattern()
        .find_iter(value)
        .map(|found| found.as_str())
        .collect()
}

/// Refuse suggestions when the observed edit could alter a number, negation,
/// or sensitive content. The caller still requires explicit user confirmation.
pub(crate) fn permits_vocabulary_suggestion(inserted: &str, edit: &EditPair) -> bool {
    let Some(edited) = reconstruct_insertion(inserted, edit) else {
        return false;
    };
    if crate::expression_observation::contains_sensitive_text(inserted) {
        return false;
    }
    // A formatting/closed-form expression preference must never become an
    // ASR hotword, including short inputs below the profile sample threshold.
    if crate::expression_observation::is_expression_only_edit(inserted, &edited) {
        return false;
    }
    let before = normalized(&format!("{}{}{}", edit.before, edit.source, edit.after));
    let after = normalized(&format!("{}{}{}", edit.before, edit.target, edit.after));
    if crate::expression_observation::contains_sensitive_text(&before)
        || crate::expression_observation::contains_sensitive_text(&after)
    {
        return false;
    }
    let source = normalized(&edit.source);
    let target = normalized(&edit.target);
    if [&source, &target].iter().any(|changed| {
        !number_signature(changed).is_empty() || !negation_signature(changed).is_empty()
    }) {
        return false;
    }
    number_signature(&before) == number_signature(&after)
        && negation_signature(&before) == negation_signature(&after)
}

/// Reconstruct only one unambiguous, insertion-local change. The full anchors
/// supplied by the host must belong to this insertion, not just a matching word
/// elsewhere in the document. No surrounding document text is retained.
pub(crate) fn reconstruct_insertion(inserted: &str, edit: &EditPair) -> Option<String> {
    if inserted.chars().count() > 16_384
        || edit.source.chars().count().max(edit.target.chars().count()) > 16_384
        || edit.source == edit.target
    {
        return None;
    }
    let mut location = None;
    for offset in inserted
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(inserted.len()))
    {
        let Some(rest) = inserted[offset..].strip_prefix(&edit.source) else {
            continue;
        };
        if !inserted[..offset].ends_with(&edit.before) || !rest.starts_with(&edit.after) {
            continue;
        }
        if location.is_some() {
            return None;
        }
        location = Some(offset);
    }
    let start = location?;
    Some(format!(
        "{}{}{}",
        &inserted[..start],
        edit.target,
        &inserted[start + edit.source.len()..]
    ))
}

/// Candidates are transient data for the confirmation API. They are neither
/// correction rules nor dictionary entries until the user accepts one.
pub(crate) fn vocabulary_candidates(
    inserted: &str,
    edit: &EditPair,
    max_chars: usize,
) -> Vec<LearnedRule> {
    if !permits_vocabulary_suggestion(inserted, edit) {
        return Vec::new();
    }
    let rules = crate::host_document::learned_rules_with_max_chars(edit, max_chars.min(32));
    if rules.is_empty() || rules.len() > 2 {
        return Vec::new();
    }
    let unchanged_letters = edit
        .before
        .chars()
        .chain(edit.after.chars())
        .filter(|c| c.is_alphabetic())
        .count();
    let changed_letters: usize = rules
        .iter()
        .map(|rule| lexical_distance(&rule.pattern, &rule.replacement))
        .sum();
    let total_letters = inserted.chars().filter(|c| c.is_alphabetic()).count();
    // Compact Han changes need surrounding evidence; otherwise a complete
    // short sentence rewrite is indistinguishable from a name correction.
    let han_change = rules.iter().any(|rule| {
        rule.pattern
            .chars()
            .chain(rule.replacement.chars())
            .any(is_han)
    });
    if (han_change
        && (unchanged_letters < 4
            || rules.iter().any(|rule| {
                rule.pattern.chars().filter(|c| is_han(*c)).count() > 4
                    || rule.replacement.chars().filter(|c| is_han(*c)).count() > 4
            })))
        || (rules.len() > 1 && changed_letters * 2 > total_letters)
        || (changed_letters > 8 && changed_letters * 2 > total_letters)
    {
        return Vec::new();
    }
    rules
        .into_iter()
        .filter(|rule| {
            inserted.contains(&rule.pattern)
                && !crate::expression_observation::contains_sensitive_text(&rule.replacement)
                && lexical_distance_is_small(&rule.pattern, &rule.replacement)
        })
        .collect()
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0x20000..=0x3134f)
}

fn lexical_distance_is_small(source: &str, target: &str) -> bool {
    let old: Vec<char> = normalized(source).chars().collect();
    let new: Vec<char> = normalized(target).chars().collect();
    // Script switches can represent a transliterated proper name, but only in
    // the compact, anchored Han case already checked above.
    if old.iter().all(|c| is_han(*c)) != new.iter().all(|c| is_han(*c)) {
        return old.len().min(new.len()) >= 2 && old.len().max(new.len()) <= 16;
    }
    lexical_distance(source, target) <= 2.max(old.len().max(new.len()) / 3)
}

fn lexical_distance(source: &str, target: &str) -> usize {
    let old: Vec<char> = normalized(source).chars().collect();
    let new: Vec<char> = normalized(target).chars().collect();
    let mut row: Vec<usize> = (0..=new.len()).collect();
    for (i, a) in old.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in new.iter().enumerate() {
            let prior = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(prior + 1)
                .min(diagonal + usize::from(a != b));
            diagonal = prior;
        }
    }
    row[new.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(before: &str, source: &str, target: &str, after: &str) -> EditPair {
        EditPair {
            before: before.into(),
            source: source.into(),
            target: target.into(),
            after: after.into(),
        }
    }

    #[test]
    fn permits_ordinary_technical_word_corrections() {
        assert!(permits_vocabulary_suggestion(
            "请导出 ZIP 文件",
            &edit("请导出 ", "ZIP", "VIP", " 文件")
        ));
        assert!(permits_vocabulary_suggestion(
            "使用 codx 完成任务",
            &edit("使用 ", "codx", "codex", " 完成任务")
        ));
    }

    #[test]
    fn rejects_fact_and_negation_changes() {
        assert!(!permits_vocabulary_suggestion(
            "今天买了十个苹果",
            &edit("今天买了", "十", "九", "个苹果")
        ));
        assert!(!permits_vocabulary_suggestion(
            "订单编号是 10",
            &edit("订单编号是 ", "10", "11", "")
        ));
        assert!(!permits_vocabulary_suggestion(
            "这件事情不能做",
            &edit("这件事情", "不", "", "能做")
        ));
        assert!(!permits_vocabulary_suggestion(
            "I can't send this",
            &edit("I can", "'t", "", " send this")
        ));
        assert!(!permits_vocabulary_suggestion(
            "回复no即可",
            &edit("回复", "no", "yes", "即可")
        ));
    }

    #[test]
    fn rejects_sensitive_context_without_persisting_it() {
        assert!(!permits_vocabulary_suggestion(
            "联系 user@example.com 处理 ZIP",
            &edit("处理 ", "ZIP", "VIP", "")
        ));
        assert!(!permits_vocabulary_suggestion(
            "无敏感文本",
            &edit("账户 ", "12345678", "87654321", "")
        ));
    }

    #[test]
    fn candidates_require_this_insertion_and_local_lexical_evidence() {
        assert_eq!(
            vocabulary_candidates(
                "请使用 codx 完成这个任务",
                &edit("请使用 c", "odx", "odex", " 完成这个任务"),
                16,
            ),
            vec![LearnedRule {
                pattern: "codx".into(),
                replacement: "codex".into(),
            }]
        );
        assert_eq!(
            vocabulary_candidates(
                "今天联系加好讨论项目",
                &edit("今天联系", "加好", "佳豪", "讨论项目"),
                16,
            )[0]
            .replacement,
            "佳豪"
        );
        for (inserted, pair) in [
            ("codx 在别处", edit("另一个控件 ", "codx", "codex", "")),
            ("codx codx", edit("", "codx", "codex", "")),
            ("明天再说", edit("", "明天再说", "今天先做", "")),
            (
                "Please complete this work tomorrow",
                edit(
                    "",
                    "Please complete this work tomorrow",
                    "Cancel everything now",
                    "",
                ),
            ),
            (
                "请你帮忙完成这项工作",
                edit("", "请你帮忙", "请", "完成这项工作"),
            ),
        ] {
            assert!(vocabulary_candidates(inserted, &pair, 32).is_empty());
        }
    }

    #[test]
    fn an_explicit_delivered_span_still_refuses_a_paragraph_rewrite() {
        let original = "今天先讨论接口参数，然后安排明天的项目验收";
        let edited = "改成下周讨论文档就好了";
        let mut range = crate::host_document::RangeObservation::delivered(
            10,
            original.into(),
            10 + original.encode_utf16().count() + 4,
            "原前文".into(),
            "原后文".into(),
        )
        .unwrap();
        assert!(range.record_local_change(crate::host_document::OwnedRangeChange {
            revision: 1,
            start: 10,
            removed_utf16: original.encode_utf16().count(),
            inserted_utf16: edited.encode_utf16().count(),
            total_before: 10 + original.encode_utf16().count() + 4,
            total_after: 10 + edited.encode_utf16().count() + 4,
        }));
        let mut seen = false;
        assert!(range.observe(
            10 + edited.encode_utf16().count() + 4,
            edited,
            "原前文",
            "原后文",
            (10 + edited.encode_utf16().count(), 0),
            |edit| {
                seen = true;
                assert!(vocabulary_candidates(original, &edit, 32).is_empty());
                assert!(
                    crate::expression_observation::signal_from_observed_edit(original, &edit)
                        .is_err()
                );
                false
            },
        ));
        assert!(seen);
    }
}
