//! Vocabulary entries, hit accounting, and preset persistence.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error, read_or_default};
use crate::shared_types::LEARNED_VOCAB_NOTE;
use crate::types::{DictionaryEntry, VocabPresetStore};

/// Number of recently added manual entries that are guaranteed ASR hotword
/// seats before hit-count ranking is applied.
pub(crate) const FRESH_VOCAB_SEATS: usize = 5;

/// Order enabled vocabulary entries for ASR hotword biasing.
///
/// The persisted dictionary keeps the newest manual entries first. Reserve a
/// bounded number of those entries, rank the remainder by hit count, then
/// collapse case variants while keeping the highest-hit spelling at the first
/// position. This is a pure Core rule shared by every host.
pub(crate) fn prioritize_vocabulary_for_asr(entries: Vec<DictionaryEntry>) -> Vec<String> {
    let mut fresh_manual = Vec::with_capacity(FRESH_VOCAB_SEATS.min(entries.len()));
    let mut ranked = Vec::with_capacity(entries.len());
    for entry in entries {
        let learned = entry.note.as_deref() == Some(LEARNED_VOCAB_NOTE);
        if !learned && fresh_manual.len() < FRESH_VOCAB_SEATS {
            fresh_manual.push(entry);
        } else {
            ranked.push(entry);
        }
    }
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.hits));
    fresh_manual.extend(ranked);

    let mut best: std::collections::HashMap<String, (usize, DictionaryEntry)> =
        std::collections::HashMap::new();
    for (index, entry) in fresh_manual.into_iter().enumerate() {
        let key = entry.phrase.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        match best.entry(key) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert((index, entry));
            }
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                if entry.hits > slot.get().1.hits {
                    let position = slot.get().0;
                    slot.insert((position, entry));
                }
            }
        }
    }

    let mut picked: Vec<(usize, String)> = best
        .into_values()
        .map(|(index, entry)| (index, entry.phrase))
        .collect();
    picked.sort_by_key(|(index, _)| *index);
    picked.into_iter().map(|(_, phrase)| phrase).collect()
}

pub struct DictionaryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl DictionaryStore {
    pub fn at_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::at_path(data_dir.as_ref().join("dictionary.json"))
    }

    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<DictionaryEntry>, BackendError> {
        let _guard = self.lock_store()?;
        read_or_default(&self.path)
    }

    /// Manual entries are intentionally inserted at the front.
    pub fn add(
        &self,
        phrase: String,
        note: Option<String>,
    ) -> Result<DictionaryEntry, BackendError> {
        let _guard = self.lock_store()?;
        let mut entries = self.read_locked()?;
        let entry = new_entry(phrase, note);
        entries.insert(0, entry.clone());
        self.write_locked(&entries)?;
        Ok(entry)
    }

    /// Learned entries are deduplicated and appended behind manual entries.
    pub fn add_if_absent(
        &self,
        phrase: String,
        note: Option<String>,
    ) -> Result<Option<DictionaryEntry>, BackendError> {
        let phrase = phrase.trim().to_string();
        if phrase.is_empty() {
            return Ok(None);
        }
        let _guard = self.lock_store()?;
        let mut entries = self.read_locked()?;
        if entries.iter().any(|entry| entry.phrase == phrase) {
            return Ok(None);
        }
        let entry = new_entry(phrase, note);
        entries.push(entry.clone());
        self.write_locked(&entries)?;
        Ok(Some(entry))
    }

    pub fn remove(&self, id: &str) -> Result<(), BackendError> {
        let _guard = self.lock_store()?;
        let mut entries = self.read_locked()?;
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        if entries.len() != before {
            self.write_locked(&entries)?;
        }
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), BackendError> {
        let _guard = self.lock_store()?;
        let mut entries = self.read_locked()?;
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    "dictionary entry not found",
                )
            })?;
        if entry.enabled != enabled {
            entry.enabled = enabled;
            self.write_locked(&entries)?;
        }
        Ok(())
    }

    /// Count case-insensitive, non-overlapping occurrences in final output.
    pub fn record_hits(&self, text: &str) -> Result<u64, BackendError> {
        if text.is_empty() {
            return Ok(0);
        }
        let _guard = self.lock_store()?;
        let mut entries = self.read_locked()?;
        let haystack = text.to_lowercase();
        let mut total = 0_u64;
        let mut changed = false;
        for entry in entries.iter_mut().filter(|entry| entry.enabled) {
            let needle = entry.phrase.trim().to_lowercase();
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

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ()>, BackendError> {
        self.lock.lock().map_err(|_| {
            BackendError::new(BackendErrorCode::Internal, "dictionary store lock poisoned")
        })
    }

    fn read_locked(&self) -> Result<Vec<DictionaryEntry>, BackendError> {
        read_or_default(&self.path)
    }

    fn write_locked(&self, entries: &[DictionaryEntry]) -> Result<(), BackendError> {
        let json = serde_json::to_vec_pretty(entries)
            .map_err(|_| persistence_error("encode dictionary entries"))?;
        atomic_write(&self.path, &json)
    }
}

fn new_entry(phrase: String, note: Option<String>) -> DictionaryEntry {
    DictionaryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        phrase,
        note,
        enabled: true,
        hits: 0,
        created_at: Utc::now().to_rfc3339(),
    }
}

fn count_occurrences(haystack: &str, needle: &str) -> u64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0_u64;
    let mut start = 0_usize;
    while let Some(position) = haystack[start..].find(needle) {
        count = count.saturating_add(1);
        start += position + needle.len();
        if start >= haystack.len() {
            break;
        }
    }
    count
}

pub fn list_vocab_presets(data_dir: &Path) -> Result<VocabPresetStore, BackendError> {
    read_or_default(&data_dir.join("vocab-presets.json"))
}

pub fn save_vocab_presets(data_dir: &Path, store: &VocabPresetStore) -> Result<(), BackendError> {
    let json = serde_json::to_vec_pretty(store)
        .map_err(|_| persistence_error("encode vocabulary presets"))?;
    atomic_write(&data_dir.join("vocab-presets.json"), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VocabPreset;

    fn temp_store() -> (DictionaryStore, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "openless-core-vocab-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        (DictionaryStore::at_path(path.clone()), path)
    }

    #[test]
    fn manual_entries_lead_learned_entries_and_learning_deduplicates() {
        let (store, path) = temp_store();
        store.add("手动一".into(), None).unwrap();
        assert!(store
            .add_if_absent("学来的".into(), Some("自动收集".into()))
            .unwrap()
            .is_some());
        assert!(store
            .add_if_absent("学来的".into(), None)
            .unwrap()
            .is_none());
        store.add("手动二".into(), None).unwrap();
        let phrases = store
            .list()
            .unwrap()
            .into_iter()
            .map(|entry| entry.phrase)
            .collect::<Vec<_>>();
        assert_eq!(phrases, vec!["手动二", "手动一", "学来的"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn records_hits_only_for_enabled_entries() {
        let (store, path) = temp_store();
        let enabled = store.add("Codex".into(), None).unwrap();
        let disabled = store.add("Rust".into(), None).unwrap();
        store.set_enabled(&disabled.id, false).unwrap();
        assert_eq!(store.record_hits("codex CODEX Rust").unwrap(), 2);
        let entries = store.list().unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.id == enabled.id)
                .unwrap()
                .hits,
            2
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.id == disabled.id)
                .unwrap()
                .hits,
            0
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn vocabulary_presets_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "openless-core-vocab-presets-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = VocabPresetStore {
            custom: vec![VocabPreset {
                id: "test".into(),
                name: "测试".into(),
                phrases: vec!["PR".into(), "CI".into()],
            }],
            overrides: vec![],
            disabled_builtin_preset_ids: vec!["chef".into()],
        };
        save_vocab_presets(&dir, &store).unwrap();
        assert_eq!(list_vocab_presets(&dir).unwrap(), store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn asr_priority_preserves_fresh_manual_entries_and_dedupes_case_variants() {
        let entry = |phrase: &str, hits: u64, note: Option<&str>| DictionaryEntry {
            id: phrase.to_string(),
            phrase: phrase.to_string(),
            note: note.map(str::to_string),
            enabled: true,
            hits,
            created_at: String::new(),
        };
        let mut entries = vec![entry("fresh", 0, None), entry("claude", 0, None)];
        entries.extend([
            entry("Claude", 33, Some(LEARNED_VOCAB_NOTE)),
            entry("frequent", 12, Some(LEARNED_VOCAB_NOTE)),
        ]);

        assert_eq!(
            prioritize_vocabulary_for_asr(entries),
            vec!["fresh", "Claude", "frequent"]
        );
    }

    fn vocab_entry(phrase: &str, hits: u64) -> DictionaryEntry {
        DictionaryEntry {
            id: phrase.to_string(),
            phrase: phrase.to_string(),
            note: None,
            enabled: true,
            hits,
            created_at: String::new(),
        }
    }

    fn learned_vocab_entry(phrase: &str, hits: u64) -> DictionaryEntry {
        let mut entry = vocab_entry(phrase, hits);
        entry.note = Some(LEARNED_VOCAB_NOTE.to_string());
        entry
    }

    #[test]
    fn asr_priority_ranks_hits_after_fresh_manual_seats() {
        let mut entries: Vec<_> = (0..FRESH_VOCAB_SEATS)
            .map(|index| vocab_entry(&format!("fresh{index}"), 0))
            .collect();
        entries.extend([
            vocab_entry("scrap", 1),
            vocab_entry("hermes", 18),
            vocab_entry("win-shukong", 7),
        ]);

        let ordered = prioritize_vocabulary_for_asr(entries);
        let position = |phrase: &str| ordered.iter().position(|item| item == phrase).unwrap();
        assert!(position("hermes") < position("scrap"));
        assert!(position("win-shukong") < position("scrap"));
        assert!(position("hermes") < position("win-shukong"));
    }

    #[test]
    fn asr_priority_reserves_a_seat_for_a_new_manual_phrase() {
        let mut entries = vec![vocab_entry("Pathwyze", 0)];
        entries.extend((0..30).map(|index| vocab_entry(&format!("old{index}"), 100 + index)));

        assert_eq!(
            prioritize_vocabulary_for_asr(entries)
                .first()
                .map(String::as_str),
            Some("Pathwyze")
        );
    }

    #[test]
    fn asr_priority_keeps_the_highest_hit_case_variant_at_the_first_position() {
        let ordered = prioritize_vocabulary_for_asr(vec![
            vocab_entry("claude", 0),
            vocab_entry("mac-mini", 27),
            vocab_entry("Claude", 33),
        ]);
        assert_eq!(ordered, vec!["Claude", "mac-mini"]);
    }

    #[test]
    fn learned_entries_do_not_consume_or_backfill_manual_seats() {
        let mut entries = Vec::new();
        for index in 0..FRESH_VOCAB_SEATS {
            entries.push(learned_vocab_entry(
                &format!("learned{index}"),
                1_000 - index as u64,
            ));
            entries.push(vocab_entry(&format!("manual{index}"), 0));
        }
        let ordered = prioritize_vocabulary_for_asr(entries);
        let expected_manual: Vec<_> = (0..FRESH_VOCAB_SEATS)
            .map(|index| format!("manual{index}"))
            .collect();
        assert_eq!(&ordered[..FRESH_VOCAB_SEATS], expected_manual.as_slice());

        let ordered = prioritize_vocabulary_for_asr(vec![
            learned_vocab_entry("learned-low", 1),
            vocab_entry("only-manual", 0),
            learned_vocab_entry("learned-high", 20),
        ]);
        assert_eq!(ordered, vec!["only-manual", "learned-high", "learned-low"]);
    }

    #[test]
    fn asr_priority_ranks_all_learned_entries_by_hits() {
        assert_eq!(
            prioritize_vocabulary_for_asr(vec![
                learned_vocab_entry("cold", 0),
                learned_vocab_entry("hot", 12),
                learned_vocab_entry("warm", 5),
            ]),
            vec!["hot", "warm", "cold"]
        );
    }

    #[test]
    fn asr_priority_dedupes_manual_and_learned_case_variants() {
        assert_eq!(
            prioritize_vocabulary_for_asr(vec![
                vocab_entry("claude", 0),
                learned_vocab_entry("Claude", 33),
                learned_vocab_entry("other", 10),
            ]),
            vec!["Claude", "other"]
        );
    }
}
