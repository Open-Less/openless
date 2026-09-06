//! Deterministic user correction rules shared by every UI host.
//!
//! Correction rules are separate from vocabulary hints. They are applied after
//! transcription/polishing and intentionally support only the conservative
//! `{num}` wildcard instead of arbitrary regular expressions.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error, read_or_default};
use crate::types::{CorrectionRule, RuleSource};

const NUM_TOKEN: &str = "{num}";

/// Persistent correction-rule repository with framework-independent paths.
pub struct CorrectionRuleStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl CorrectionRuleStore {
    pub fn at_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::at_path(data_dir.as_ref().join("correction-rules.json"))
    }

    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<CorrectionRule>, BackendError> {
        let _guard = self.lock_store()?;
        self.read_locked()
    }

    pub fn add(
        &self,
        pattern: String,
        replacement: String,
    ) -> Result<CorrectionRule, BackendError> {
        self.add_with_source(pattern, replacement, RuleSource::Manual)
    }

    pub fn add_with_source(
        &self,
        pattern: String,
        replacement: String,
        source: RuleSource,
    ) -> Result<CorrectionRule, BackendError> {
        let pattern = pattern.trim().to_string();
        let replacement = replacement.trim().to_string();
        validate_correction_rule_syntax(&pattern, &replacement)?;
        let _guard = self.lock_store()?;
        let mut rules = self.read_locked()?;
        let rule = CorrectionRule {
            id: uuid::Uuid::new_v4().to_string(),
            pattern,
            replacement,
            enabled: true,
            created_at: Utc::now().to_rfc3339(),
            source,
        };
        rules.insert(0, rule.clone());
        self.write_locked(&rules)?;
        Ok(rule)
    }

    /// Removing an unknown id is deliberately idempotent.
    pub fn remove(&self, id: &str) -> Result<(), BackendError> {
        let _guard = self.lock_store()?;
        let mut rules = self.read_locked()?;
        let before = rules.len();
        rules.retain(|rule| rule.id != id);
        if rules.len() != before {
            self.write_locked(&rules)?;
        }
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), BackendError> {
        let _guard = self.lock_store()?;
        let mut rules = self.read_locked()?;
        let rule = rules.iter_mut().find(|rule| rule.id == id).ok_or_else(|| {
            BackendError::new(
                BackendErrorCode::InvalidArgument,
                "correction rule not found",
            )
        })?;
        if rule.enabled != enabled {
            rule.enabled = enabled;
            self.write_locked(&rules)?;
        }
        Ok(())
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ()>, BackendError> {
        self.lock.lock().map_err(|_| {
            BackendError::new(
                BackendErrorCode::Internal,
                "correction rule store lock poisoned",
            )
        })
    }

    fn read_locked(&self) -> Result<Vec<CorrectionRule>, BackendError> {
        read_or_default(&self.path)
    }

    fn write_locked(&self, rules: &[CorrectionRule]) -> Result<(), BackendError> {
        let json = serde_json::to_vec_pretty(rules)
            .map_err(|_| persistence_error("encode correction rules"))?;
        atomic_write(&self.path, &json)
    }
}

pub fn validate_correction_rule_syntax(
    pattern: &str,
    replacement: &str,
) -> Result<(), BackendError> {
    if pattern.is_empty() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "correction rule pattern is empty",
        ));
    }
    let pattern_token_count = pattern.matches(NUM_TOKEN).count();
    let invalid = pattern_token_count > 1
        || (replacement.contains(NUM_TOKEN) && pattern_token_count == 0)
        || (pattern_token_count == 1
            && pattern
                .split_once(NUM_TOKEN)
                .is_none_or(|(prefix, suffix)| prefix.is_empty() && suffix.is_empty()));
    if invalid {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "unsupported correction rule syntax",
        ));
    }
    Ok(())
}

/// Apply enabled correction rules sequentially.
pub fn apply_correction_rules(text: &str, rules: &[CorrectionRule]) -> String {
    let mut current = text.to_string();
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let pattern = rule.pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        current = apply_rule(&current, pattern, &rule.replacement);
    }
    current
}

/// Apply one correction pattern.
///
/// This lower-level operation remains public for the selection edit-plan
/// compatibility adapter. UI hosts should normally use
/// [`apply_correction_rules`].
pub fn apply_rule(text: &str, pattern: &str, replacement: &str) -> String {
    let token_count = pattern.matches(NUM_TOKEN).count();
    if token_count == 0 {
        if replacement.contains(NUM_TOKEN) {
            return text.to_string();
        }
        return text.replace(pattern, replacement);
    }
    if token_count != 1 {
        return text.to_string();
    }
    apply_num_rule(text, pattern, replacement)
}

fn apply_num_rule(text: &str, pattern: &str, replacement: &str) -> String {
    let Some((prefix, suffix)) = pattern.split_once(NUM_TOKEN) else {
        return text.to_string();
    };
    if prefix.is_empty() && suffix.is_empty() {
        return text.to_string();
    }

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some((match_start, token_start)) = next_prefix_match(text, cursor, prefix) else {
            break;
        };
        let Some(token_end) = consume_number_token(text, token_start) else {
            output.push_str(&text[cursor..next_char_boundary(text, match_start)]);
            cursor = next_char_boundary(text, match_start);
            continue;
        };
        let after_number = &text[token_end..];
        if !after_number.starts_with(suffix) {
            output.push_str(&text[cursor..next_char_boundary(text, match_start)]);
            cursor = next_char_boundary(text, match_start);
            continue;
        }

        let match_end = token_end + suffix.len();
        output.push_str(&text[cursor..match_start]);
        output.push_str(&replacement.replace(NUM_TOKEN, &text[token_start..token_end]));
        cursor = match_end;
    }
    output.push_str(&text[cursor..]);
    output
}

fn next_prefix_match(text: &str, cursor: usize, prefix: &str) -> Option<(usize, usize)> {
    if prefix.is_empty() {
        let match_start = next_number_start(text, cursor)?;
        return Some((match_start, match_start));
    }
    let relative = text[cursor..].find(prefix)?;
    let match_start = cursor + relative;
    Some((match_start, match_start + prefix.len()))
}

fn next_number_start(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .char_indices()
        .find_map(|(offset, ch)| is_number_char(ch).then_some(cursor + offset))
}

fn consume_number_token(text: &str, start: usize) -> Option<usize> {
    let mut end = start;
    let mut consumed = false;
    for (offset, ch) in text[start..].char_indices() {
        if !is_number_char(ch) {
            break;
        }
        consumed = true;
        end = start + offset + ch.len_utf8();
    }
    consumed.then_some(end)
}

fn is_number_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || matches!(
            ch,
            '零' | '〇'
                | '一'
                | '二'
                | '两'
                | '兩'
                | '三'
                | '四'
                | '五'
                | '六'
                | '七'
                | '八'
                | '九'
                | '十'
                | '百'
                | '千'
                | '万'
                | '萬'
                | '亿'
                | '億'
                | '几'
                | '幾'
        )
}

fn next_char_boundary(text: &str, start: usize) -> usize {
    text[start..]
        .chars()
        .next()
        .map(|ch| start + ch.len_utf8())
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuleSource;

    fn rule(pattern: &str, replacement: &str) -> CorrectionRule {
        CorrectionRule {
            id: "rule".into(),
            pattern: pattern.into(),
            replacement: replacement.into(),
            enabled: true,
            created_at: String::new(),
            source: RuleSource::Manual,
        }
    }

    #[test]
    fn applies_literal_replacement() {
        let rules = vec![rule("几粒", "几例")];
        assert_eq!(
            apply_correction_rules("这里有几粒样品", &rules),
            "这里有几例样品"
        );
    }

    #[test]
    fn applies_num_wildcard_for_arabic_digits() {
        let rules = vec![rule("{num}粒", "{num}例")];
        assert_eq!(
            apply_correction_rules("2粒样品和10粒对照", &rules),
            "2例样品和10例对照"
        );
    }

    #[test]
    fn applies_num_wildcard_for_chinese_numbers() {
        let rules = vec![rule("{num}粒", "{num}例")];
        assert_eq!(
            apply_correction_rules("两粒样品和幾粒对照", &rules),
            "两例样品和幾例对照"
        );
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut disabled = rule("{num}粒", "{num}例");
        disabled.enabled = false;
        assert_eq!(apply_correction_rules("10粒样品", &[disabled]), "10粒样品");
    }

    #[test]
    fn malformed_rules_are_inert() {
        let rules = vec![
            rule("{num}到{num}粒", "{num}例"),
            rule("几粒", "{num}例"),
            rule("{num}", "{num}例"),
        ];
        assert_eq!(apply_correction_rules("几粒和10粒", &rules), "几粒和10粒");
    }

    #[test]
    fn applies_rules_sequentially() {
        let rules = vec![rule("{num}粒", "{num}例"), rule("样本", "样品")];
        assert_eq!(apply_correction_rules("10粒样本", &rules), "10例样品");
    }

    #[test]
    fn syntax_validation_rejects_silent_noops() {
        assert!(validate_correction_rule_syntax("{num}粒", "{num}例").is_ok());
        assert!(validate_correction_rule_syntax("几粒", "几例").is_ok());
        for (pattern, replacement) in [
            ("", "几例"),
            ("{num}", "{num}例"),
            ("{num}到{num}粒", "{num}例"),
            ("几粒", "{num}例"),
        ] {
            assert_eq!(
                validate_correction_rule_syntax(pattern, replacement)
                    .unwrap_err()
                    .code,
                BackendErrorCode::InvalidArgument
            );
        }
    }

    #[test]
    fn legacy_source_defaults_and_round_trips() {
        let json = r#"{"id":"1","pattern":"甲","replacement":"乙","enabled":true,"createdAt":""}"#;
        let rule: CorrectionRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source, RuleSource::Manual);
        assert_eq!(
            serde_json::to_string(&RuleSource::Learned).unwrap(),
            "\"learned\""
        );
    }

    #[test]
    fn store_round_trips_mutations_and_keeps_remove_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-correction-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = CorrectionRuleStore::at_path(path.clone());
        let rule = store.add(" {num}粒 ".into(), " {num}例 ".into()).unwrap();
        assert_eq!(rule.pattern, "{num}粒");
        assert_eq!(store.list().unwrap(), vec![rule.clone()]);

        store.set_enabled(&rule.id, false).unwrap();
        assert!(!store.list().unwrap()[0].enabled);
        store.remove(&rule.id).unwrap();
        store.remove(&rule.id).unwrap();
        assert!(store.list().unwrap().is_empty());

        let _ = std::fs::remove_file(path);
    }
}
