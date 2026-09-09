#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Correction-rule store: literal/`{num}`-token find-and-replace rules.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use uuid::Uuid;

use super::{atomic_write, data_dir, ensure_dir, read_or_default};
use crate::types::{CorrectionRule, RuleSource};

const CORRECTION_RULES_FILE: &str = "correction-rules.json";
const CORRECTION_NUM_TOKEN: &str = "{num}";

pub struct CorrectionRuleStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl CorrectionRuleStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(CORRECTION_RULES_FILE),
            lock: Mutex::new(()),
        })
    }

    /// 测试专用：指定落盘路径，让每个用例有自己独立的文件。
    #[cfg(test)]
    fn new_at(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    /// 降级实例：data_dir 不可用时使用临时路径（桌面）或空 path（Android 内存态）。
    pub(crate) fn new_fallback() -> Self {
        Self {
            path: super::fallback_store_path("openless_correction_rules_fallback.json"),
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<CorrectionRule>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn add(&self, pattern: String, replacement: String) -> Result<CorrectionRule> {
        self.add_with_source(pattern, replacement, RuleSource::Manual)
    }

    /// Only called after an explicit confirmation. Never create wildcard or
    /// chained rules from observed edits; duplicates and conflicts are visible.
    #[cfg(any(target_os = "windows", test))]
    pub fn add_confirmed(&self, pattern: String, replacement: String) -> Result<CorrectionRule> {
        let pattern = pattern.trim().to_string();
        let replacement = replacement.trim().to_string();
        if pattern.chars().count() < 2 || replacement.is_empty() || pattern == replacement
            || pattern.contains(CORRECTION_NUM_TOKEN) || replacement.contains(CORRECTION_NUM_TOKEN)
            || replacement.contains(&pattern) {
            return Err(anyhow!("该改法不适合作为固定纠正规则，请在词汇表手动设置"));
        }
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        if let Some(rule) = rules.iter().find(|r| r.pattern == pattern) {
            if rule.enabled && rule.replacement == replacement { return Ok(rule.clone()); }
            return Err(anyhow!("此写法已有不同或已停用的规则，请在词汇表检查"));
        }
        if rules.iter().filter(|r| r.enabled).any(|r|
            replacement.contains(&r.pattern) || r.replacement.contains(&pattern)) {
            return Err(anyhow!("此改法会与已有规则产生连锁替换，请在词汇表检查"));
        }
        let rule = new_rule(pattern, replacement, RuleSource::Learned);
        rules.insert(0, rule.clone());
        self.write_locked(&rules)?;
        Ok(rule)
    }

    fn add_with_source(
        &self,
        pattern: String,
        replacement: String,
        source: RuleSource,
    ) -> Result<CorrectionRule> {
        let pattern = pattern.trim().to_string();
        let replacement = replacement.trim().to_string();
        validate_correction_rule_syntax(&pattern, &replacement)?;
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let rule = new_rule(pattern, replacement, source);
        rules.insert(0, rule.clone());
        self.write_locked(&rules)?;
        Ok(rule)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let before = rules.len();
        rules.retain(|r| r.id != id);
        if rules.len() == before {
            return Ok(());
        }
        self.write_locked(&rules)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let mut found = false;
        for rule in rules.iter_mut() {
            if rule.id == id {
                rule.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Err(anyhow!("correction rule {} not found", id));
        }
        self.write_locked(&rules)
    }

    fn read_locked(&self) -> Result<Vec<CorrectionRule>> {
        read_or_default::<Vec<CorrectionRule>>(&self.path)
    }

    fn write_locked(&self, rules: &[CorrectionRule]) -> Result<()> {
        let json = serde_json::to_vec_pretty(rules).context("encode correction rules failed")?;
        atomic_write(&self.path, &json)
    }
}

fn new_rule(pattern: String, replacement: String, source: RuleSource) -> CorrectionRule {
    CorrectionRule {
        id: Uuid::new_v4().to_string(),
        pattern,
        replacement,
        enabled: true,
        created_at: Utc::now().to_rfc3339(),
        source,
    }
}

fn validate_correction_rule_syntax(pattern: &str, replacement: &str) -> Result<()> {
    if pattern.is_empty() {
        return Err(anyhow!("correction rule pattern is empty"));
    }
    let pattern_token_count = pattern.matches(CORRECTION_NUM_TOKEN).count();
    if pattern_token_count > 1 {
        return Err(anyhow!("unsupported correction rule syntax"));
    }
    if replacement.contains(CORRECTION_NUM_TOKEN) && pattern_token_count == 0 {
        return Err(anyhow!("unsupported correction rule syntax"));
    }
    if pattern_token_count == 1 {
        let Some((prefix, suffix)) = pattern.split_once(CORRECTION_NUM_TOKEN) else {
            return Err(anyhow!("unsupported correction rule syntax"));
        };
        if prefix.is_empty() && suffix.is_empty() {
            return Err(anyhow!("unsupported correction rule syntax"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_correction_rule_syntax;
    use crate::types::{CorrectionRule, RuleSource};

    #[test]
    fn confirmed_rules_persist_deduplicate_and_reject_chains() {
        let directory = std::env::temp_dir().join(format!("openless-confirmed-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("rules.json");
        let store = super::CorrectionRuleStore::new_at(path.clone());
        assert!(store.list().unwrap().is_empty());
        let rule = store.add_confirmed("玄策".into(), "旋测".into()).unwrap();
        assert_eq!(rule.source, RuleSource::Learned);
        let again = store.add_confirmed("玄策".into(), "旋测".into()).unwrap();
        assert_eq!(rule.id, again.id);
        let reopened = super::CorrectionRuleStore::new_at(path.clone());
        let rules = reopened.list().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(crate::correction::apply_correction_rules("江苏玄策", &rules), "江苏旋测");
        assert!(store.add_confirmed("旋测".into(), "玄策".into()).is_err());
        assert!(store.add_confirmed("另词".into(), "玄策".into()).is_err());
        assert!(store.add_confirmed("玄策".into(), "其他".into()).is_err());
        assert!(store.add_confirmed("{num}".into(), "数字".into()).is_err());
        assert!(store.add_confirmed("甲".into(), "乙".into()).is_err());
        store.set_enabled(&rule.id, false).unwrap();
        assert_eq!(crate::correction::apply_correction_rules("江苏玄策", &store.list().unwrap()), "江苏玄策");
        assert!(store.add_confirmed("玄策".into(), "旋测".into()).is_err());
        store.remove(&rule.id).unwrap();
        assert!(store.list().unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn correction_rule_syntax_rejects_silent_noops() {
        assert!(validate_correction_rule_syntax("{num}粒", "{num}例").is_ok());
        assert!(validate_correction_rule_syntax("几粒", "几例").is_ok());
        assert!(validate_correction_rule_syntax("", "几例").is_err());
        assert!(validate_correction_rule_syntax("{num}", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("{num}到{num}粒", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("几粒", "{num}例").is_err());
    }

    /// 老的 correction-rules.json 没有 `source` 字段，反序列化必须落到 Manual。
    ///
    /// Windows 用户明确确认的替换使用 Learned；手动规则仍使用 Manual。
    #[test]
    fn a_rule_without_a_source_field_deserializes_as_manual() {
        let json = r#"{"id":"1","pattern":"甲","replacement":"乙","enabled":true,"createdAt":""}"#;
        let rule: CorrectionRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source, RuleSource::Manual);
    }

    #[test]
    fn rule_source_round_trips_as_camel_case() {
        let json = serde_json::to_string(&RuleSource::Learned).unwrap();
        assert_eq!(json, "\"learned\"");
        let back: RuleSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RuleSource::Learned);
    }
}
