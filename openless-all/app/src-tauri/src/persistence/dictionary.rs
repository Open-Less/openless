#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Vocabulary dictionary store (phrase hit-counting) plus the vocab-preset
//! JSON file accessors.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use uuid::Uuid;

use super::{atomic_write, data_dir, ensure_dir, read_or_default};
use crate::types::{
    DictionaryEntry, DictionaryScope, DictionarySource, PendingCorrection, VocabPresetStore,
};

/// 与 Swift `Sources/OpenLessPersistence/DictionaryStore.swift` 同名，
/// 让旧版词汇表在升级后无缝继承。**不要**改成 `vocab.json`，会丢用户数据。
const VOCAB_FILE: &str = "dictionary.json";
const VOCAB_SUGGESTIONS_FILE: &str = "vocab-suggestions.json";
const VOCAB_PRESETS_FILE: &str = "vocab-presets.json";
const LEARNED_VOCAB_NOTE: &str = "从手改中自动收集";

pub struct DictionaryStore {
    path: PathBuf,
    suggestions_path: PathBuf,
    lock: Mutex<()>,
}

impl DictionaryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(VOCAB_FILE),
            suggestions_path: dir.join(VOCAB_SUGGESTIONS_FILE),
            lock: Mutex::new(()),
        })
    }

    /// 测试专用：指定落盘路径，让每个用例有自己独立的文件（也就不会碰到用户真实的
    /// dictionary.json）。与 `CorrectionRuleStore::new_at` 同形。
    #[cfg(test)]
    fn new_at(path: PathBuf) -> Self {
        let suggestions_path = path.with_file_name(format!(
            "{}-suggestions.json",
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("vocab")
        ));
        Self {
            path,
            suggestions_path,
            lock: Mutex::new(()),
        }
    }

    /// 降级实例：data_dir 不可用时使用临时路径（桌面）或空 path（Android 内存态）。
    pub(crate) fn new_fallback() -> Self {
        Self {
            path: super::fallback_store_path("openless_vocab_fallback.json"),
            suggestions_path: super::fallback_store_path(
                "openless_vocab_suggestions_fallback.json",
            ),
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    /// 运行时真正可以投递的词条：关闭、过期、以及不属于当前 project key 的临时词条
    /// 在这里统一过滤，避免 ASR 与 LLM 各自实现一套容易漂移的规则。
    pub fn active(&self, project_key: Option<&str>) -> Result<Vec<DictionaryEntry>> {
        let now = Utc::now();
        Ok(self
            .list()?
            .into_iter()
            .filter(|entry| entry_is_active(entry, project_key, &now))
            .collect())
    }

    pub fn add(&self, phrase: String, note: Option<String>) -> Result<DictionaryEntry> {
        self.add_with_metadata(
            phrase,
            note,
            DictionaryScope::Persistent,
            None,
            None,
        )
    }

    pub fn add_with_metadata(
        &self,
        phrase: String,
        note: Option<String>,
        scope: DictionaryScope,
        project_key: Option<String>,
        expires_at: Option<String>,
    ) -> Result<DictionaryEntry> {
        let phrase = phrase.trim().to_string();
        if phrase.is_empty() {
            return Err(anyhow!("dictionary phrase cannot be empty"));
        }
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let project_key = match scope {
            DictionaryScope::Persistent => None,
            DictionaryScope::Temporary => Some(
                project_key
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| anyhow!("project vocabulary requires an active app"))?,
            ),
        };
        let entry = DictionaryEntry {
            id: Uuid::new_v4().to_string(),
            phrase,
            note,
            enabled: true,
            hits: 0,
            created_at: Utc::now().to_rfc3339(),
            source: DictionarySource::Manual,
            scope,
            pinned: false,
            last_hit_at: None,
            expires_at: normalize_optional_timestamp(expires_at)?,
            project_key,
        };
        entries.insert(0, entry.clone());
        self.write_locked(&entries)?;
        Ok(entry)
    }

    /// 学习路径专用：已存在同 phrase 就不重复加，返回 `Ok(None)`。
    ///
    /// 手动添加不查重（用户重复录入是他的选择），自动路径必须查 —— 同一个词每被改一次
    /// 就多一条，几天下来词汇表全是重复。
    ///
    /// **追加到末尾，不像 [`Self::add`] 那样插到最前。** ASR 词表预算按词典顺序取
    /// 「最近添加的前 [`FRESH_VOCAB_SEATS`](crate::coordinator) 条」做保底席位，那个保底
    /// 的理由是「用户刚手动加它，多半是刚被它坑过」—— 对着卡片点一下勾不满足这个理由，
    /// 而卡片本来就可能建议半截词。插到最前会让连点几个勾就把保底席位全占掉，把用户
    /// 攒了几十次命中的常用词挤出 ASR 预算。
    ///
    /// 排在队尾不等于永远进不了 ASR 预算：词条进 LLM 热词块没有名额限制，那一侧立刻
    /// 生效；命中计数扫的是最终文本、与有没有进过 ASR 词表无关，所以这个词一旦真的开始
    /// 被用上就会自己按命中爬进预算。
    pub fn add_if_absent(&self, phrase: String, note: Option<String>) -> Result<Option<DictionaryEntry>> {
        let phrase = phrase.trim().to_string();
        if phrase.is_empty() {
            return Ok(None);
        }
        // 查重和写入同一个 guard 内完成，不留 TOCTOU 窗口。
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        if entries.iter().any(|e| e.phrase == phrase) {
            return Ok(None);
        }
        let entry = DictionaryEntry {
            id: Uuid::new_v4().to_string(),
            phrase,
            note,
            enabled: true,
            hits: 0,
            created_at: Utc::now().to_rfc3339(),
            source: DictionarySource::Learned,
            scope: DictionaryScope::Persistent,
            pinned: false,
            last_hit_at: None,
            expires_at: None,
            project_key: None,
        };
        entries.push(entry.clone());
        self.write_locked(&entries)?;
        Ok(Some(entry))
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() == before {
            return Ok(());
        }
        self.write_locked(&entries)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let mut found = false;
        for entry in entries.iter_mut() {
            if entry.id == id {
                entry.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Err(anyhow!("dictionary entry {} not found", id));
        }
        self.write_locked(&entries)
    }

    pub fn set_metadata(
        &self,
        id: &str,
        pinned: bool,
        scope: DictionaryScope,
        project_key: Option<String>,
        expires_at: Option<String>,
    ) -> Result<DictionaryEntry> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| anyhow!("dictionary entry {} not found", id))?;
        entry.pinned = pinned;
        entry.scope = scope;
        entry.project_key = match scope {
            DictionaryScope::Persistent => None,
            DictionaryScope::Temporary => Some(
                project_key
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| anyhow!("project vocabulary requires an active app"))?,
            ),
        };
        entry.expires_at = normalize_optional_timestamp(expires_at)?;
        let updated = entry.clone();
        self.write_locked(&entries)?;
        Ok(updated)
    }

    /// 扫描一段最终文本，对每个 enabled 词条按出现次数累加 `hits`。
    ///
    /// 匹配是大小写不敏感的子串扫描：「Hello hello HELLO」算 3 次。
    /// 返回本次累加的总命中数，方便调用方记录到 history.dictionary_entry_count。
    pub fn record_hits(
        &self,
        text: &str,
        project_key: Option<&str>,
        temporary_ttl_days: u32,
    ) -> Result<u64> {
        if text.is_empty() {
            return Ok(0);
        }
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        if entries.is_empty() {
            return Ok(0);
        }
        let haystack = text.to_lowercase();
        let mut total: u64 = 0;
        let mut changed = false;
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        for entry in entries.iter_mut() {
            if !entry_is_active(entry, project_key, &now) {
                continue;
            }
            let needle = entry.phrase.trim().to_lowercase();
            if needle.is_empty() {
                continue;
            }
            let count = count_occurrences(&haystack, &needle);
            if count > 0 {
                entry.hits = entry.hits.saturating_add(count);
                entry.last_hit_at = Some(now_text.clone());
                if entry.scope == DictionaryScope::Temporary {
                    entry.expires_at = Some(
                        (now + chrono::Duration::days(temporary_ttl_days.clamp(1, 365) as i64))
                            .to_rfc3339(),
                    );
                }
                total = total.saturating_add(count);
                changed = true;
            }
        }
        if changed {
            self.write_locked(&entries)?;
        }
        Ok(total)
    }

    /// 把临时层收敛到配置容量。永久层完全不参与；固定临时词条也不淘汰，因此当固定
    /// 数量本身超过容量时允许暂时超限。返回被淘汰的词条 id，便于诊断与测试。
    pub fn enforce_temporary_capacity(&self, capacity: u32) -> Result<Vec<String>> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let capacity = capacity.clamp(1, 10_000) as usize;
        let mut removed = Vec::new();
        while entries
            .iter()
            .filter(|entry| entry.scope == DictionaryScope::Temporary)
            .count()
            > capacity
        {
            let Some((index, _)) = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.scope == DictionaryScope::Temporary && !entry.pinned
                })
                .min_by_key(|(_, entry)| temporary_lru_key(entry))
            else {
                break;
            };
            removed.push(entries.remove(index).id);
        }
        if !removed.is_empty() {
            self.write_locked(&entries)?;
        }
        Ok(removed)
    }

    fn read_locked(&self) -> Result<Vec<DictionaryEntry>> {
        let mut entries = read_or_default::<Vec<DictionaryEntry>>(&self.path)?;
        let mut migrated = false;
        for entry in &mut entries {
            // v1.3.x 没有 source，自动学习只靠这条 note 文案辨认。第一次读取时把它迁成
            // 显式枚举，同时保留 note，旧客户端仍能按原逻辑显示。
            if entry.source == DictionarySource::Manual
                && entry.note.as_deref() == Some(LEARNED_VOCAB_NOTE)
            {
                entry.source = DictionarySource::Learned;
                migrated = true;
            }
            if entry.scope == DictionaryScope::Temporary && entry.project_key.is_none() {
                entry.scope = DictionaryScope::Persistent;
                migrated = true;
            }
        }
        if migrated {
            self.write_locked(&entries)?;
        }
        Ok(entries)
    }

    fn write_locked(&self, entries: &[DictionaryEntry]) -> Result<()> {
        let json = serde_json::to_vec_pretty(entries).context("encode vocab failed")?;
        atomic_write(&self.path, &json)
    }

    pub fn list_suggestions(&self) -> Result<Vec<PendingCorrection>> {
        let _guard = self.lock.lock();
        let mut suggestions = read_or_default::<Vec<PendingCorrection>>(&self.suggestions_path)?;
        let before = suggestions.len();
        suggestions.retain(suggestion_is_active);
        if suggestions.len() != before {
            self.write_suggestions_locked(&suggestions)?;
        }
        Ok(suggestions)
    }

    pub fn save_suggestion(&self, suggestion: PendingCorrection) -> Result<()> {
        let _guard = self.lock.lock();
        let mut suggestions = read_or_default::<Vec<PendingCorrection>>(&self.suggestions_path)?;
        suggestions.retain(suggestion_is_active);
        if let Some(existing) = suggestions.iter_mut().find(|item| {
            item.pattern == suggestion.pattern && item.replacement == suggestion.replacement
        }) {
            *existing = suggestion;
        } else {
            suggestions.insert(0, suggestion);
        }
        suggestions.truncate(100);
        self.write_suggestions_locked(&suggestions)
    }

    pub fn take_suggestion(&self, id: &str) -> Result<Option<PendingCorrection>> {
        let _guard = self.lock.lock();
        let mut suggestions = read_or_default::<Vec<PendingCorrection>>(&self.suggestions_path)?;
        let taken = suggestions
            .iter()
            .position(|item| item.id == id)
            .map(|index| suggestions.remove(index));
        if taken.is_some() {
            self.write_suggestions_locked(&suggestions)?;
        }
        Ok(taken)
    }

    pub fn remove_suggestion(&self, id: &str) -> Result<()> {
        let _ = self.take_suggestion(id)?;
        Ok(())
    }

    pub fn clear_suggestions(&self) -> Result<()> {
        let _guard = self.lock.lock();
        self.write_suggestions_locked(&[])
    }

    fn write_suggestions_locked(&self, suggestions: &[PendingCorrection]) -> Result<()> {
        let json = serde_json::to_vec_pretty(suggestions).context("encode vocab suggestions failed")?;
        atomic_write(&self.suggestions_path, &json)
    }
}

fn entry_is_active(
    entry: &DictionaryEntry,
    project_key: Option<&str>,
    now: &chrono::DateTime<Utc>,
) -> bool {
    if !entry.enabled {
        return false;
    }
    if entry
        .expires_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|expires| expires.with_timezone(&Utc) <= *now)
    {
        return false;
    }
    match entry.scope {
        DictionaryScope::Persistent => true,
        DictionaryScope::Temporary => entry.project_key.as_deref() == project_key,
    }
}

fn suggestion_is_active(suggestion: &PendingCorrection) -> bool {
    suggestion
        .expires_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_none_or(|expires| expires.with_timezone(&Utc) > Utc::now())
}

fn normalize_optional_timestamp(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value.map(|value| value.trim().to_string()) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(&value)
        .with_context(|| format!("invalid expiresAt: {value}"))?;
    Ok(Some(parsed.with_timezone(&Utc).to_rfc3339()))
}

fn temporary_lru_key(entry: &DictionaryEntry) -> i64 {
    entry
        .last_hit_at
        .as_deref()
        .or((!entry.created_at.is_empty()).then_some(entry.created_at.as_str()))
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_nanos_opt().unwrap_or(i64::MIN))
        .unwrap_or(i64::MIN)
}

/// 统计 `needle` 在 `haystack` 中的非重叠出现次数。两侧调用前都应已转小写。
fn count_occurrences(haystack: &str, needle: &str) -> u64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count: u64 = 0;
    let mut start = 0usize;
    while let Some(pos) = haystack[start..].find(needle) {
        count = count.saturating_add(1);
        start = start + pos + needle.len();
        if start >= haystack.len() {
            break;
        }
    }
    count
}

pub fn list_vocab_presets() -> Result<VocabPresetStore> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    read_or_default::<VocabPresetStore>(&dir.join(VOCAB_PRESETS_FILE))
}

pub fn save_vocab_presets(store: &VocabPresetStore) -> Result<()> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    let path = dir.join(VOCAB_PRESETS_FILE);
    let json = serde_json::to_vec_pretty(store).context("encode vocab presets failed")?;
    atomic_write(&path, &json)
}

#[cfg(test)]
mod tests {
    use super::{list_vocab_presets, save_vocab_presets, DictionaryStore};
    use crate::types::{
        CorrectionAttribution, CorrectionConfidence, DictionaryScope, DictionarySource,
        PendingCorrection, VocabPreset, VocabPresetStore,
    };
    use chrono::Utc;
    use std::fs;
    use std::path::PathBuf;

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LegacyDictionaryEntry {
        id: String,
        phrase: String,
        note: Option<String>,
        enabled: bool,
        hits: u64,
        created_at: String,
    }

    fn temp_store() -> DictionaryStore {
        let path = std::env::temp_dir().join(format!("openless-vocab-{}.json", uuid::Uuid::new_v4()));
        DictionaryStore::new_at(path)
    }

    /// 手动添加插在最前，学来的追加到最后。
    ///
    /// 这不是排版偏好，是**跟 ASR 词表预算的接口约定**：预算把「词典最前面的若干条」
    /// 当保底席位，理由是「用户刚手动加它，多半刚被它坑过」。对着建议卡片点一下勾不
    /// 满足这个理由，而卡片本来就可能建议出半截词（真机上见过 `ap → ype`）。学来的词
    /// 要是也插到最前，连点几个勾就能把保底席位全占掉，把用户攒了几十次命中的常用词
    /// 挤出预算 —— 那正是这个功能要解决的问题本身。
    #[test]
    fn a_learned_entry_lands_behind_the_manual_ones() {
        let store = temp_store();
        store.add("手动一".into(), None).expect("add");
        store
            .add_if_absent("学来的".into(), Some("从手改中自动收集".into()))
            .expect("add_if_absent");
        store.add("手动二".into(), None).expect("add");

        let phrases: Vec<String> = store
            .list()
            .expect("list")
            .into_iter()
            .map(|e| e.phrase)
            .collect();
        assert_eq!(phrases, vec!["手动二", "手动一", "学来的"]);
    }

    #[test]
    fn the_same_learned_phrase_is_not_collected_twice() {
        let store = temp_store();
        let note = Some("从手改中自动收集".to_string());
        assert!(store
            .add_if_absent("Codex".into(), note.clone())
            .expect("first")
            .is_some());
        assert!(store
            .add_if_absent("Codex".into(), note)
            .expect("second")
            .is_none());
        assert_eq!(store.list().expect("list").len(), 1);
    }

    #[test]
    fn legacy_learned_note_migrates_to_explicit_source() {
        let store = temp_store();
        fs::write(
            &store.path,
            r#"[{"id":"old","phrase":"Codex","notes":"从手改中自动收集","enabled":true,"hitCount":2,"createdAt":"2026-01-01T00:00:00Z"}]"#,
        )
        .expect("seed legacy dictionary");
        let entries = store.list().expect("migrate");
        assert_eq!(entries[0].source, DictionarySource::Learned);
        let persisted = fs::read_to_string(&store.path).expect("read migrated file");
        assert!(persisted.contains("\"source\": \"learned\""));
    }

    #[test]
    fn v1_4_dictionary_remains_readable_by_the_legacy_flat_array_contract() {
        let store = temp_store();
        let entry = store
            .add_with_metadata(
                "OpenLess".into(),
                Some("project vocabulary".into()),
                DictionaryScope::Temporary,
                Some("com.example.editor".into()),
                Some((Utc::now() + chrono::Duration::days(7)).to_rfc3339()),
            )
            .expect("add v1.4 entry");
        let json = fs::read_to_string(&store.path).expect("read v1.4 dictionary");
        let legacy: Vec<LegacyDictionaryEntry> =
            serde_json::from_str(&json).expect("legacy reader ignores additive fields");

        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].id, entry.id);
        assert_eq!(legacy[0].phrase, "OpenLess");
        assert_eq!(legacy[0].note.as_deref(), Some("project vocabulary"));
        assert!(legacy[0].enabled);
        assert_eq!(legacy[0].hits, 0);
        assert!(!legacy[0].created_at.is_empty());
    }

    #[test]
    fn project_and_expiry_filter_the_runtime_view_without_deleting_entries() {
        let store = temp_store();
        store
            .add_with_metadata(
                "项目词".into(),
                None,
                DictionaryScope::Temporary,
                Some("com.example.editor".into()),
                Some((Utc::now() + chrono::Duration::days(1)).to_rfc3339()),
            )
            .expect("project entry");
        store
            .add_with_metadata(
                "过期词".into(),
                None,
                DictionaryScope::Persistent,
                None,
                Some((Utc::now() - chrono::Duration::seconds(1)).to_rfc3339()),
            )
            .expect("expired entry");
        assert_eq!(
            store
                .active(Some("com.example.editor"))
                .expect("active")
                .into_iter()
                .map(|entry| entry.phrase)
                .collect::<Vec<_>>(),
            vec!["项目词"]
        );
        assert!(store.active(Some("com.other")).expect("other").is_empty());
        assert_eq!(store.list().expect("management view").len(), 2);
    }

    #[test]
    fn hit_recording_sets_last_hit_and_respects_project_scope() {
        let store = temp_store();
        let entry = store
            .add_with_metadata(
                "OpenLess".into(),
                None,
                DictionaryScope::Temporary,
                Some("editor".into()),
                None,
            )
            .expect("add");
        assert_eq!(
            store
                .record_hits("OpenLess", Some("other"), 7)
                .expect("miss"),
            0
        );
        assert_eq!(
            store
                .record_hits("OpenLess openless", Some("editor"), 7)
                .expect("hits"),
            2
        );
        let updated = store
            .list()
            .expect("list")
            .into_iter()
            .find(|item| item.id == entry.id)
            .expect("entry");
        assert_eq!(updated.hits, 2);
        assert!(updated.last_hit_at.is_some());
        assert!(updated
            .expires_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|expires| expires > Utc::now()));
    }

    #[test]
    fn temporary_lru_evicts_only_old_unpinned_entries() {
        let store = temp_store();
        let oldest = store
            .add_with_metadata(
                "最旧".into(),
                None,
                DictionaryScope::Temporary,
                Some("editor".into()),
                None,
            )
            .expect("oldest");
        let pinned = store
            .add_with_metadata(
                "固定".into(),
                None,
                DictionaryScope::Temporary,
                Some("editor".into()),
                None,
            )
            .expect("pinned");
        store
            .set_metadata(
                &pinned.id,
                true,
                DictionaryScope::Temporary,
                Some("editor".into()),
                None,
            )
            .expect("pin");
        let newest = store
            .add_with_metadata(
                "最新".into(),
                None,
                DictionaryScope::Temporary,
                Some("editor".into()),
                None,
            )
            .expect("newest");

        let removed = store.enforce_temporary_capacity(2).expect("enforce");
        assert_eq!(removed, vec![oldest.id]);
        let remaining = store.list().expect("list");
        assert!(remaining.iter().any(|entry| entry.id == pinned.id));
        assert!(remaining.iter().any(|entry| entry.id == newest.id));
        assert!(remaining
            .iter()
            .all(|entry| entry.scope == DictionaryScope::Temporary));
    }

    #[test]
    fn suggestion_inbox_roundtrips_and_take_is_destructive() {
        let store = temp_store();
        let suggestion = PendingCorrection {
            id: "suggestion-1".into(),
            pattern: "扣得死".into(),
            replacement: "Codex".into(),
            confidence: CorrectionConfidence::High,
            attribution: CorrectionAttribution::Asr,
            created_at: Utc::now().to_rfc3339(),
            expires_at: Some((Utc::now() + chrono::Duration::days(7)).to_rfc3339()),
            source_app: Some("Editor".into()),
        };
        store.save_suggestion(suggestion.clone()).expect("save");
        assert_eq!(store.list_suggestions().expect("list"), vec![suggestion.clone()]);
        assert_eq!(
            store.take_suggestion(&suggestion.id).expect("take"),
            Some(suggestion)
        );
        assert!(store.list_suggestions().expect("empty").is_empty());
    }

    #[test]
    fn vocab_presets_roundtrip_json_file() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        // Linux path helper uses XDG_DATA_HOME first.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &tmp);
        }
        let store = VocabPresetStore {
            custom: vec![VocabPreset {
                id: "test".into(),
                name: "测试".into(),
                phrases: vec!["PR".into(), "CI".into()],
            }],
            overrides: vec![],
            disabled_builtin_preset_ids: vec!["chef".into()],
        };
        save_vocab_presets(&store).expect("save presets");
        let loaded = list_vocab_presets().expect("list presets");
        assert_eq!(loaded.custom.len(), 1);
        assert_eq!(loaded.custom[0].id, "test");
        assert_eq!(
            loaded.custom[0].phrases,
            vec!["PR".to_string(), "CI".to_string()]
        );
        assert_eq!(loaded.disabled_builtin_preset_ids, vec!["chef".to_string()]);
        let _ = fs::remove_dir_all(&tmp);
    }
}
