//! Conservative safeguards for locally observed vocabulary suggestions.

use crate::host_document::EditPair;
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
    if crate::expression_observation::contains_sensitive_text(inserted) {
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
}
