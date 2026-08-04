#![cfg_attr(target_os = "linux", allow(dead_code, unused_variables))]
//! Vocabulary dictionary store (phrase hit-counting) plus the vocab-preset
//! JSON file accessors.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use uuid::Uuid;

use super::{atomic_write, data_dir, ensure_dir, read_or_default};
use crate::types::{DictionaryEntry, VocabPresetStore};

/// 与 Swift `Sources/OpenLessPersistence/DictionaryStore.swift` 同名，
/// 让旧版词汇表在升级后无缝继承。**不要**改成 `vocab.json`，会丢用户数据。
const VOCAB_FILE: &str = "dictionary.json";
const VOCAB_PRESETS_FILE: &str = "vocab-presets.json";

pub struct DictionaryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl DictionaryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(VOCAB_FILE),
            lock: Mutex::new(()),
        })
    }

    /// 测试专用：指定落盘路径，让每个用例有自己独立的文件（也就不会碰到用户真实的
    /// dictionary.json）。与 `CorrectionRuleStore::new_at` 同形。
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
            path: super::fallback_store_path("openless_vocab_fallback.json"),
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn add(&self, phrase: String, note: Option<String>) -> Result<DictionaryEntry> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let entry = DictionaryEntry {
            id: Uuid::new_v4().to_string(),
            phrase,
            note,
            enabled: true,
            hits: 0,
            created_at: Utc::now().to_rfc3339(),
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

    /// 扫描一段最终文本，对每个 enabled 词条按出现次数累加 `hits`。
    ///
    /// 匹配是大小写不敏感的子串扫描：「Hello hello HELLO」算 3 次。
    /// 返回本次累加的总命中数，方便调用方记录到 history.dictionary_entry_count。
    pub fn record_hits(&self, text: &str) -> Result<u64> {
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
        for entry in entries.iter_mut() {
            if !entry.enabled {
                continue;
            }
            let needle = entry.phrase.trim().to_lowercase();
            if needle.is_empty() {
                continue;
            }
            let count = count_occurrences(&haystack, &needle);
            if count > 0 {
                entry.hits = entry.hits.saturating_add(count);
                total = total.saturating_add(count);
                changed = true;
            }
        }
        if changed {
            self.write_locked(&entries)?;
        }
        Ok(total)
    }

    fn read_locked(&self) -> Result<Vec<DictionaryEntry>> {
        read_or_default::<Vec<DictionaryEntry>>(&self.path)
    }

    fn write_locked(&self, entries: &[DictionaryEntry]) -> Result<()> {
        let json = serde_json::to_vec_pretty(entries).context("encode vocab failed")?;
        atomic_write(&self.path, &json)
    }
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
    use crate::types::{VocabPreset, VocabPresetStore};
    use std::fs;
    use std::path::PathBuf;

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
