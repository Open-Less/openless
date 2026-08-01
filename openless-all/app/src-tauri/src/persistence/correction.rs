#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Correction-rule store: literal/`{num}`-token find-and-replace rules.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use uuid::Uuid;

use super::{atomic_write, data_dir, ensure_dir, read_or_default};
use crate::types::CorrectionRule;

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
        let pattern = pattern.trim().to_string();
        let replacement = replacement.trim().to_string();
        validate_correction_rule_syntax(&pattern, &replacement)?;
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let rule = CorrectionRule {
            id: Uuid::new_v4().to_string(),
            pattern,
            replacement,
            enabled: true,
            created_at: Utc::now().to_rfc3339(),
        };
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

    #[test]
    fn correction_rule_syntax_rejects_silent_noops() {
        assert!(validate_correction_rule_syntax("{num}粒", "{num}例").is_ok());
        assert!(validate_correction_rule_syntax("几粒", "几例").is_ok());
        assert!(validate_correction_rule_syntax("", "几例").is_err());
        assert!(validate_correction_rule_syntax("{num}", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("{num}到{num}粒", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("几粒", "{num}例").is_err());
    }
}
