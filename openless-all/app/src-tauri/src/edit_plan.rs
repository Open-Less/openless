//! Structured edit plans produced by the selection-voice LLM and applied
//! deterministically to a draft (issue #987 desktop MVP; EditPlan shape refs #900).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::correction::apply_rule;
use crate::polish::clean_json_llm_output;

const MAX_OPERATIONS: usize = 32;
const MAX_OP_STRING_LEN: usize = 8_192;
const MAX_PATTERN_LEN: usize = 512;
const REGEX_TIMEOUT_MS: u64 = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditPlan {
    pub operations: Vec<EditOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditOperation {
    LiteralReplace {
        find: String,
        replace: String,
    },
    RegexReplace {
        pattern: String,
        replace: String,
        #[serde(default)]
        flags: RegexFlags,
    },
    RangeReplace {
        start: u32,
        end: u32,
        replace: String,
    },
    FullRewrite {
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct RegexFlags {
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default)]
    pub multiline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditApplyError {
    TooManyOperations,
    OperationTooLarge,
    PatternTooLarge,
    EmptyDraft,
    InvalidRange,
    RegexRejected(String),
    RegexTimedOut,
    NoOperations,
}

impl std::fmt::Display for EditApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyOperations => write!(f, "edit plan has too many operations"),
            Self::OperationTooLarge => write!(f, "edit operation exceeds size limit"),
            Self::PatternTooLarge => write!(f, "regex pattern exceeds size limit"),
            Self::EmptyDraft => write!(f, "draft is empty"),
            Self::InvalidRange => write!(f, "range replace indices are invalid"),
            Self::RegexRejected(reason) => write!(f, "regex rejected: {reason}"),
            Self::RegexTimedOut => write!(f, "regex execution timed out"),
            Self::NoOperations => write!(f, "edit plan has no operations"),
        }
    }
}

impl std::error::Error for EditApplyError {}

pub fn parse_edit_plan_json(raw: &str) -> Result<EditPlan, String> {
    let trimmed = raw.trim();
    match parse_edit_plan_json_candidate(trimmed) {
        Ok(plan) => Ok(plan),
        Err(primary) => {
            let cleaned = clean_json_llm_output(raw);
            if cleaned == trimmed {
                Err(primary)
            } else {
                parse_edit_plan_json_candidate(&cleaned).map_err(|secondary| {
                    format!("invalid EditPlan JSON: {primary}; cleaned retry: {secondary}")
                })
            }
        }
    }
}

fn parse_edit_plan_json_candidate(raw: &str) -> Result<EditPlan, String> {
    let json = extract_json_object(raw).unwrap_or(raw);
    let mut value: Value = serde_json::from_str(json)
        .map_err(|error| format!("invalid EditPlan JSON: {error}"))?;
    normalize_edit_plan_value(&mut value);
    serde_json::from_value(value).map_err(|error| format!("invalid EditPlan JSON: {error}"))
}

fn normalize_edit_plan_value(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if !obj.contains_key("operations") {
        if let Some(ops) = obj.remove("operation") {
            obj.insert("operations".to_string(), ops);
        }
    }
    if let Some(ops) = obj.get_mut("operations").and_then(|v| v.as_array_mut()) {
        for op in ops {
            normalize_edit_operation_value(op);
        }
    }
}

fn normalize_edit_operation_value(op: &mut Value) {
    let Some(obj) = op.as_object_mut() else {
        return;
    };
    if let Some(type_value) = obj.get("type").and_then(|v| v.as_str()) {
        let normalized = normalize_operation_type(type_value);
        obj.insert("type".to_string(), Value::String(normalized));
    }
    let op_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if op_type == "full_rewrite" {
        promote_alias_field(obj, "text", ["content", "body", "value", "replacement"]);
    }
    if op_type == "literal_replace" {
        promote_alias_field(obj, "replace", ["replacement", "with", "value"]);
        promote_alias_field(obj, "find", ["search", "match", "pattern"]);
    }
    if op_type == "regex_replace" {
        promote_alias_field(obj, "pattern", ["regex", "find", "search"]);
        promote_alias_field(obj, "replace", ["replacement", "with", "value"]);
    }
    if op_type == "range_replace" {
        promote_alias_field(obj, "replace", ["replacement", "with", "value", "text"]);
    }
}

fn normalize_operation_type(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "fullrewrite" | "full_rewrite" | "rewrite" | "translate" | "translation" => {
            "full_rewrite".into()
        }
        "literalreplace" | "literal_replace" | "replace" | "text_replace" => {
            "literal_replace".into()
        }
        "regexreplace" | "regex_replace" | "regexp_replace" => "regex_replace".into(),
        "rangereplace" | "range_replace" | "substring_replace" => "range_replace".into(),
        other => other.to_string(),
    }
}

fn promote_alias_field(
    obj: &mut serde_json::Map<String, Value>,
    canonical: &str,
    aliases: [&str; 4],
) {
    if obj.contains_key(canonical) {
        return;
    }
    for alias in aliases {
        if let Some(value) = obj.remove(alias) {
            obj.insert(canonical.to_string(), value);
            return;
        }
    }
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (start <= end).then(|| &raw[start..=end])
}

pub fn apply_edit_plan(draft: &str, plan: &EditPlan) -> Result<String, EditApplyError> {
    if draft.is_empty() {
        return Err(EditApplyError::EmptyDraft);
    }
    if plan.operations.is_empty() {
        return Err(EditApplyError::NoOperations);
    }
    if plan.operations.len() > MAX_OPERATIONS {
        return Err(EditApplyError::TooManyOperations);
    }

    let mut current = draft.to_string();
    for op in &plan.operations {
        validate_operation_size(op)?;
        current = apply_operation(&current, op)?;
    }
    Ok(current)
}

fn validate_operation_size(op: &EditOperation) -> Result<(), EditApplyError> {
    let too_large = |value: &str| value.chars().count() > MAX_OP_STRING_LEN;
    match op {
        EditOperation::LiteralReplace { find, replace } => {
            if too_large(find) || too_large(replace) {
                return Err(EditApplyError::OperationTooLarge);
            }
        }
        EditOperation::RegexReplace {
            pattern,
            replace,
            ..
        } => {
            if pattern.chars().count() > MAX_PATTERN_LEN
                || too_large(replace)
            {
                return Err(EditApplyError::PatternTooLarge);
            }
        }
        EditOperation::RangeReplace { replace, .. } => {
            if too_large(replace) {
                return Err(EditApplyError::OperationTooLarge);
            }
        }
        EditOperation::FullRewrite { text } => {
            if too_large(text) {
                return Err(EditApplyError::OperationTooLarge);
            }
        }
    }
    Ok(())
}

fn apply_operation(text: &str, op: &EditOperation) -> Result<String, EditApplyError> {
    match op {
        EditOperation::LiteralReplace { find, replace } => {
            if find.is_empty() {
                return Ok(text.to_string());
            }
            Ok(apply_rule(text, find, replace))
        }
        EditOperation::RegexReplace {
            pattern,
            replace,
            flags,
        } => apply_regex_replace(text, pattern, replace, *flags),
        EditOperation::RangeReplace {
            start,
            end,
            replace,
        } => apply_range_replace(text, *start, *end, replace),
        EditOperation::FullRewrite { text } => Ok(text.clone()),
    }
}

fn apply_range_replace(
    text: &str,
    start: u32,
    end: u32,
    replacement: &str,
) -> Result<String, EditApplyError> {
    if end < start {
        return Err(EditApplyError::InvalidRange);
    }
    let char_len = text.chars().count() as u32;
    if start > char_len || end > char_len {
        return Err(EditApplyError::InvalidRange);
    }
    let start_byte = char_index_to_byte(text, start as usize)?;
    let end_byte = char_index_to_byte(text, end as usize)?;
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..start_byte]);
    out.push_str(replacement);
    out.push_str(&text[end_byte..]);
    Ok(out)
}

fn char_index_to_byte(text: &str, char_index: usize) -> Result<usize, EditApplyError> {
    if char_index == 0 {
        return Ok(0);
    }
    let mut count = 0usize;
    for (byte_index, _) in text.char_indices() {
        if count == char_index {
            return Ok(byte_index);
        }
        count += 1;
    }
    if count == char_index {
        return Ok(text.len());
    }
    Err(EditApplyError::InvalidRange)
}

fn apply_regex_replace(
    text: &str,
    pattern: &str,
    replacement: &str,
    flags: RegexFlags,
) -> Result<String, EditApplyError> {
    if pattern.trim().is_empty() {
        return Ok(text.to_string());
    }
    if contains_nested_quantifiers(pattern) {
        return Err(EditApplyError::RegexRejected(
            "nested quantifiers are not allowed".into(),
        ));
    }

    let mut builder = regex::RegexBuilder::new(pattern);
    builder.case_insensitive(flags.case_insensitive);
    if flags.multiline {
        builder.multi_line(true);
    }
    let regex = builder
        .size_limit(1 << 20)
        .build()
        .map_err(|error| EditApplyError::RegexRejected(error.to_string()))?;

    let started = Instant::now();
    let haystack = text.to_string();
    let pattern_owned = pattern.to_string();
    let replacement_owned = replacement.to_string();
    let regex_owned = regex;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = regex_owned.replace_all(&haystack, replacement_owned.as_str());
        let _ = tx.send(result.into_owned());
    });

    match rx.recv_timeout(Duration::from_millis(REGEX_TIMEOUT_MS)) {
        Ok(replaced) => {
            if started.elapsed() > Duration::from_millis(REGEX_TIMEOUT_MS) {
                return Err(EditApplyError::RegexTimedOut);
            }
            Ok(replaced)
        }
        Err(_) => {
            log::warn!(
                "[edit-plan] regex timed out after {REGEX_TIMEOUT_MS}ms (pattern={pattern_owned:?})"
            );
            Err(EditApplyError::RegexTimedOut)
        }
    }
}

fn contains_nested_quantifiers(pattern: &str) -> bool {
    let quantifiers = ['*', '+', '?', '{'];
    let mut prev_was_quantifier = false;
    for ch in pattern.chars() {
        let is_quantifier = quantifiers.contains(&ch);
        if is_quantifier && prev_was_quantifier {
            return true;
        }
        prev_was_quantifier = is_quantifier;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_replace_masks_credentials() {
        let draft = "账号: old@mail.com\n密码: secret123";
        let plan = EditPlan {
            operations: vec![EditOperation::LiteralReplace {
                find: "old@mail.com".into(),
                replace: "user@example.com".into(),
            }],
            summary: None,
        };
        assert_eq!(
            apply_edit_plan(draft, &plan).unwrap(),
            "账号: user@example.com\n密码: secret123"
        );
    }

    #[test]
    fn regex_replace_batch_email_format() {
        let draft = "邮箱1: a@b.com\n邮箱2: c@d.com";
        let plan = EditPlan {
            operations: vec![EditOperation::RegexReplace {
                pattern: r"([a-z]+)@([a-z]+\.com)".into(),
                replace: r"$1@company.com".into(),
                flags: RegexFlags::default(),
            }],
            summary: Some("normalize email domains".into()),
        };
        let out = apply_edit_plan(draft, &plan).unwrap();
        assert!(out.contains("a@company.com"));
        assert!(out.contains("c@company.com"));
    }

    #[test]
    fn range_replace_is_char_safe() {
        let draft = "你好世界";
        let plan = EditPlan {
            operations: vec![EditOperation::RangeReplace {
                start: 2,
                end: 4,
                replace: "Rust".into(),
            }],
            summary: None,
        };
        assert_eq!(apply_edit_plan(draft, &plan).unwrap(), "你好Rust");
    }

    #[test]
    fn full_rewrite_replaces_entire_draft() {
        let draft = "旧内容";
        let plan = EditPlan {
            operations: vec![EditOperation::FullRewrite {
                text: "新内容".into(),
            }],
            summary: None,
        };
        assert_eq!(apply_edit_plan(draft, &plan).unwrap(), "新内容");
    }

    #[test]
    fn rejects_empty_operations() {
        let plan = EditPlan {
            operations: vec![],
            summary: None,
        };
        assert_eq!(
            apply_edit_plan("text", &plan),
            Err(EditApplyError::NoOperations)
        );
    }

    #[test]
    fn rejects_invalid_range() {
        let plan = EditPlan {
            operations: vec![EditOperation::RangeReplace {
                start: 5,
                end: 2,
                replace: "x".into(),
            }],
            summary: None,
        };
        assert_eq!(
            apply_edit_plan("abc", &plan),
            Err(EditApplyError::InvalidRange)
        );
    }

    #[test]
    fn parses_operation_alias_and_translate_type() {
        let raw = r#"{"operation":[{"type":"translate","content":"Hello"}]}"#;
        let plan = parse_edit_plan_json(raw).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(
            plan.operations[0],
            EditOperation::FullRewrite {
                text: "Hello".into()
            }
        );
    }

    #[test]
    fn parses_json_with_surrounding_markdown() {
        let raw = r#"Here is the plan:
```json
{"operations":[{"type":"literal_replace","find":"a","replace":"b"}],"summary":"ok"}
```"#;
        let plan = parse_edit_plan_json(raw).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert_eq!(plan.summary.as_deref(), Some("ok"));
    }
}
