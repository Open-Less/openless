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

    /// 学来的规则走这里，`source` 记 [`RuleSource::Learned`]。
    ///
    /// 与手动添加的唯一区别是**同 pattern 查重**：手动添加时用户明知自己在做什么，
    /// 重复录入是他的选择；学习路径是自动跑的，不查重的话同一个词每被改一次就会多出
    /// 一条规则，几天下来词库里全是重复。
    ///
    /// 已存在同 pattern 时返回 `Ok(None)`，调用方按「没新增」处理。
    pub fn add_learned(
        &self,
        pattern: String,
        replacement: String,
    ) -> Result<Option<CorrectionRule>> {
        let pattern = pattern.trim().to_string();
        let replacement = replacement.trim().to_string();
        validate_correction_rule_syntax(&pattern, &replacement)?;
        // 查重和写入必须在同一个 guard 里 —— 分成两段会留下一个 TOCTOU 窗口，
        // 同一个词被连着改两次就能穿过去，写出两条一样的规则。
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        if rules.iter().any(|r| r.pattern == pattern) {
            return Ok(None);
        }
        let rule = new_rule(pattern, replacement, RuleSource::Learned);
        rules.insert(0, rule.clone());
        self.write_locked(&rules)?;
        Ok(Some(rule))
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
    fn correction_rule_syntax_rejects_silent_noops() {
        assert!(validate_correction_rule_syntax("{num}粒", "{num}例").is_ok());
        assert!(validate_correction_rule_syntax("几粒", "几例").is_ok());
        assert!(validate_correction_rule_syntax("", "几例").is_err());
        assert!(validate_correction_rule_syntax("{num}", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("{num}到{num}粒", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("几粒", "{num}例").is_err());
    }

    /// 学来的规则是纯 literal（没有 `{num}` 占位符），必须能通过既有的语法校验 ——
    /// 否则整条学习链路会在最后一步静默失败。
    #[test]
    fn a_learned_literal_rule_passes_the_existing_syntax_check() {
        assert!(validate_correction_rule_syntax("扣德克斯", "Codex").is_ok());
        assert!(validate_correction_rule_syntax("大禹", "大鱼").is_ok());
        // 纯删除：replacement 为空是合法的（「把多余的『的』删掉」）。
        assert!(validate_correction_rule_syntax("的", "").is_ok());
    }

    /// 老的 correction-rules.json 没有 `source` 字段，反序列化必须落到 Manual ——
    /// 落到 Learned 会让用户手动录入的规则被「批量删除自动收集的」一键清空。
    #[test]
    fn a_rule_without_a_source_field_deserializes_as_manual() {
        let json = r#"{"id":"1","pattern":"甲","replacement":"乙","enabled":true,"createdAt":""}"#;
        let rule: CorrectionRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.source, RuleSource::Manual);
    }

    fn temp_store(name: &str) -> (super::CorrectionRuleStore, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("openless-correction-test-{name}.json"));
        let _ = std::fs::remove_file(&path);
        (super::CorrectionRuleStore::new_at(path.clone()), path)
    }

    /// 学习路径是自动跑的：不查重的话，同一个词每被改一次就多一条规则，几天下来
    /// 词库里全是重复。
    #[test]
    fn a_learned_rule_with_an_existing_pattern_is_not_added_twice() {
        let (store, path) = temp_store("dedupe");
        let first = store
            .add_learned("扣德克斯".into(), "Codex".into())
            .unwrap();
        assert!(first.is_some());
        let second = store
            .add_learned("扣德克斯".into(), "Codex".into())
            .unwrap();
        assert!(second.is_none(), "同 pattern 不该重复入库");
        assert_eq!(store.list().unwrap().len(), 1);
        let _ = std::fs::remove_file(path);
    }

    /// 查重只看 pattern：同一个错法这次被改成别的写法，也不该再加一条 —— 让用户去
    /// 改那条已有的规则，而不是留两条互相打架的。
    #[test]
    fn dedupe_matches_on_pattern_regardless_of_replacement() {
        let (store, path) = temp_store("dedupe-pattern");
        store.add_learned("大禹".into(), "大鱼".into()).unwrap();
        let again = store.add_learned("大禹".into(), "大宇".into()).unwrap();
        assert!(again.is_none());
        let _ = std::fs::remove_file(path);
    }

    /// 手动添加的规则也参与查重 —— 用户已经手写过的规则，不该被自动收集覆盖或复制。
    #[test]
    fn a_learned_rule_does_not_duplicate_a_manual_one() {
        let (store, path) = temp_store("dedupe-manual");
        store.add("接口".into(), "借口".into()).unwrap();
        let learned = store.add_learned("接口".into(), "借口".into()).unwrap();
        assert!(learned.is_none());
        let rules = store.list().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, RuleSource::Manual, "不该把手动规则改成 learned");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_learned_rule_is_tagged_as_learned() {
        let (store, path) = temp_store("tag");
        let rule = store
            .add_learned("扣德克斯".into(), "Codex".into())
            .unwrap()
            .unwrap();
        assert_eq!(rule.source, RuleSource::Learned);
        assert_eq!(store.add("手写".into(), "手寫".into()).unwrap().source, RuleSource::Manual);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rule_source_round_trips_as_camel_case() {
        let json = serde_json::to_string(&RuleSource::Learned).unwrap();
        assert_eq!(json, "\"learned\"");
        let back: RuleSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RuleSource::Learned);
    }
}
