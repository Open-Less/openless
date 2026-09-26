//! Newest-first dictation history with retention and count caps.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::errors::{BackendError, BackendErrorCode};
use crate::persistence::{atomic_write, persistence_error};
use crate::types::{DictationSession, HistorySource};

pub const HISTORY_CAP: usize = 200;

pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn at_data_dir(data_dir: impl AsRef<Path>) -> Self {
        Self::at_path(data_dir.as_ref().join("history.json"))
    }

    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn list(&self) -> Result<Vec<DictationSession>, BackendError> {
        let _guard = self.lock_store()?;
        self.read_locked()
    }

    pub(crate) fn sync_snapshot(
        &self,
        permit: &crate::cloud_sync_e2ee_store::gate::ExclusivePermit,
    ) -> Result<Vec<DictationSession>, BackendError> {
        crate::cloud_sync_e2ee_store::gate::require_exclusive(&self.path, permit)?;
        let _guard = self.lock_store()?;
        crate::persistence::read_lossless_rows(&self.path, &[])
    }

    /// Whole logical replacement deliberately bypasses both age and entry-count retention.
    pub(crate) fn sync_replace_all(
        &self,
        records: &[DictationSession],
        permit: &crate::cloud_sync_e2ee_store::gate::ExclusivePermit,
    ) -> Result<(), BackendError> {
        crate::cloud_sync_e2ee_store::gate::require_exclusive(&self.path, permit)?;
        let _guard = self.lock_store()?;
        let bytes = serde_json::to_vec_pretty(records)
            .map_err(|_| persistence_error("encode restored history"))?;
        crate::persistence::atomic_write_for_sync(&self.path, &bytes, permit)
    }

    pub fn append_with_retention(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<(), BackendError> {
        crate::cloud_sync_e2ee_store::gate::with_registered_mutation(
            &self.path,
            crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User,
            || {
                let _guard = self.lock_store()?;
                let mut sessions = self.read_locked()?;
                sessions.insert(0, session);
                retain_with_policy(&mut sessions, retention_days, max_entries);
                self.write_locked(&sessions)
            },
        )
    }

    /// Replace an in-progress record when its terminal result arrives, or
    /// append it when no provisional record exists (legacy/recovery path).
    pub fn upsert_with_retention(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<(), BackendError> {
        crate::cloud_sync_e2ee_store::gate::with_registered_mutation(
            &self.path,
            crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User,
            || {
                let _guard = self.lock_store()?;
                let mut sessions = self.read_locked()?;
                if let Some(existing) = sessions.iter_mut().find(|item| item.id == session.id) {
                    let mut replacement = session;
                    if replacement.has_audio_recording.is_none() {
                        replacement.has_audio_recording = existing.has_audio_recording;
                    }
                    *existing = replacement;
                } else {
                    sessions.insert(0, session);
                }
                retain_with_policy(&mut sessions, retention_days, max_entries);
                self.write_locked(&sessions)
            },
        )
    }

    pub fn contains(&self, id: &str) -> Result<bool, BackendError> {
        let _guard = self.lock_store()?;
        Ok(self.read_locked()?.iter().any(|session| session.id == id))
    }

    pub fn read_entry(&self, id: &str) -> Result<Option<DictationSession>, BackendError> {
        let _guard = self.lock_store()?;
        Ok(self
            .read_locked()?
            .into_iter()
            .find(|session| session.id == id))
    }

    pub fn recent_within_minutes(
        &self,
        minutes: u32,
    ) -> Result<Vec<DictationSession>, BackendError> {
        if minutes == 0 {
            return Ok(Vec::new());
        }
        let _guard = self.lock_store()?;
        let sessions = self.read_locked()?;
        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(i64::from(minutes));
        Ok(sessions
            .into_iter()
            .take_while(|session| {
                chrono::DateTime::parse_from_rfc3339(&session.created_at)
                    .map(|time| time.with_timezone(&chrono::Utc) >= cutoff)
                    .unwrap_or(true)
            })
            .collect())
    }

    pub fn delete(&self, id: &str) -> Result<(), BackendError> {
        crate::cloud_sync_e2ee_store::gate::with_registered_mutation(
            &self.path,
            crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User,
            || {
                let _guard = self.lock_store()?;
                let mut sessions = self.read_locked()?;
                let before = sessions.len();
                sessions.retain(|session| session.id != id);
                if sessions.len() != before {
                    self.write_locked(&sessions)?;
                }
                Ok(())
            },
        )
    }

    pub fn update_entry(&self, updated: DictationSession) -> Result<bool, BackendError> {
        crate::cloud_sync_e2ee_store::gate::with_registered_mutation(
            &self.path,
            crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User,
            || {
                let _guard = self.lock_store()?;
                let mut sessions = self.read_locked()?;
                let Some(slot) = sessions.iter_mut().find(|session| session.id == updated.id)
                else {
                    return Ok(false);
                };
                *slot = updated;
                self.write_locked(&sessions)?;
                Ok(true)
            },
        )
    }

    pub fn clear(&self) -> Result<(), BackendError> {
        crate::cloud_sync_e2ee_store::gate::with_registered_mutation(
            &self.path,
            crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User,
            || {
                let _guard = self.lock_store()?;
                let quick_notes = self
                    .read_locked()?
                    .into_iter()
                    .filter(|session| session.source == HistorySource::QuickNote)
                    .collect::<Vec<_>>();
                self.write_locked(&quick_notes)
            },
        )
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ()>, BackendError> {
        self.lock.lock().map_err(|_| {
            BackendError::new(BackendErrorCode::Internal, "history store lock poisoned")
        })
    }

    fn read_locked(&self) -> Result<Vec<DictationSession>, BackendError> {
        crate::persistence::read_lossless_rows(&self.path, &[])
    }

    fn write_locked(&self, sessions: &[DictationSession]) -> Result<(), BackendError> {
        let json = serde_json::to_vec_pretty(sessions)
            .map_err(|_| persistence_error("encode history entries"))?;
        atomic_write(&self.path, &json)
    }
}

fn retain_with_policy(
    sessions: &mut Vec<DictationSession>,
    retention_days: u32,
    max_entries: Option<u32>,
) {
    // Quick notes are intentionally outside the ordinary history retention
    // policy. A recording-start draft is also protected until it reaches a
    // terminal state, because its audio may be the only recoverable artifact
    // after a process crash.
    if retention_days > 0 {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days));
        sessions.retain(|session| {
            session.source == HistorySource::QuickNote
                || session.error_code.as_deref() == Some("recording")
                || chrono::DateTime::parse_from_rfc3339(&session.created_at)
                    .map(|time| time.with_timezone(&chrono::Utc) >= cutoff)
                    .unwrap_or(true)
        });
    }
    let cap = max_entries
        .map(|count| (count as usize).clamp(5, HISTORY_CAP))
        .unwrap_or(HISTORY_CAP);
    let mut ordinary_seen = 0usize;
    sessions.retain(|session| {
        if session.source == HistorySource::QuickNote
            || session.error_code.as_deref() == Some("recording")
        {
            true
        } else {
            ordinary_seen += 1;
            ordinary_seen <= cap
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HistoryInsertStatus, HistorySource, PolishMode};

    fn session(id: &str, created_at: String) -> DictationSession {
        DictationSession {
            id: id.into(),
            created_at,
            source: HistorySource::Voice,
            raw_transcript: "raw".into(),
            asr_transcript: None,
            final_text: "final".into(),
            mode: PolishMode::Light,
            style_pack_id: None,
            translation_active: false,
            polish_source: None,
            app_bundle_id: None,
            app_name: None,
            insert_status: HistoryInsertStatus::Inserted,
            error_code: None,
            duration_ms: Some(1000),
            dictionary_entry_count: None,
            has_audio_recording: None,
            asr_provider: None,
            asr_model: None,
            llm_provider: None,
            llm_model: None,
            pipeline_mode: None,
            asr_ms: None,
            polish_ms: None,
        }
    }

    #[test]
    fn legacy_quick_note_history_survives_read_and_append() {
        let path = std::env::temp_dir().join(format!(
            "openless-legacy-history-{}.json",
            uuid::Uuid::new_v4()
        ));
        let mut legacy =
            serde_json::to_value(session("legacy", chrono::Utc::now().to_rfc3339())).unwrap();
        legacy["source"] = "quick_note".into();
        let bytes = serde_json::to_vec(&vec![legacy]).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let store = HistoryStore::at_path(path.clone());
        assert_eq!(store.list().unwrap()[0].source, HistorySource::QuickNote);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "reading must not rewrite history"
        );
        store
            .append_with_retention(session("new", chrono::Utc::now().to_rfc3339()), 0, None)
            .unwrap();
        let entries = store.list().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].source, HistorySource::QuickNote);
        assert_eq!(entries[1].raw_transcript, "raw");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted[1]["source"], "quick_note");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn append_orders_caps_and_filters_retention() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-history-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = HistoryStore::at_path(path.clone());
        let old = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        store
            .append_with_retention(session("old", old), 0, None)
            .unwrap();
        for index in 0..7 {
            store
                .append_with_retention(
                    session(&format!("new-{index}"), chrono::Utc::now().to_rfc3339()),
                    7,
                    Some(5),
                )
                .unwrap();
        }
        let sessions = store.list().unwrap();
        assert_eq!(sessions.len(), 5);
        assert_eq!(sessions[0].id, "new-6");
        assert!(sessions.iter().all(|session| session.id != "old"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn update_delete_clear_and_recent_queries_are_stable() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-history-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = HistoryStore::at_path(path.clone());
        let mut entry = session("one", chrono::Utc::now().to_rfc3339());
        store.append_with_retention(entry.clone(), 0, None).unwrap();
        assert_eq!(store.recent_within_minutes(5).unwrap(), vec![entry.clone()]);
        entry.final_text = "updated".into();
        assert!(store.update_entry(entry.clone()).unwrap());
        assert_eq!(store.list().unwrap(), vec![entry]);
        store.delete("one").unwrap();
        store.delete("missing").unwrap();
        assert!(store.list().unwrap().is_empty());
        store.clear().unwrap();
        assert!(store.recent_within_minutes(0).unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn quick_notes_survive_ordinary_history_cap() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-quick-note-retention-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = HistoryStore::at_path(path.clone());
        let mut note = session("quick", chrono::Utc::now().to_rfc3339());
        note.source = HistorySource::QuickNote;
        store.append_with_retention(note, 1, Some(5)).unwrap();
        for index in 0..8 {
            store
                .append_with_retention(
                    session(
                        &format!("ordinary-{index}"),
                        chrono::Utc::now().to_rfc3339(),
                    ),
                    1,
                    Some(5),
                )
                .unwrap();
        }
        let sessions = store.list().unwrap();
        assert!(sessions.iter().any(|entry| entry.id == "quick"));
        assert_eq!(
            sessions
                .iter()
                .filter(|entry| entry.source == HistorySource::Voice)
                .count(),
            5
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn clearing_ordinary_history_preserves_quick_notes() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-quick-note-clear-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = HistoryStore::at_path(path.clone());
        store
            .append_with_retention(
                session("ordinary", chrono::Utc::now().to_rfc3339()),
                0,
                None,
            )
            .unwrap();
        let mut note = session("quick", chrono::Utc::now().to_rfc3339());
        note.source = HistorySource::QuickNote;
        store.append_with_retention(note, 0, None).unwrap();
        store.clear().unwrap();
        let entries = store.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "quick");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn upsert_preserves_an_existing_archived_audio_flag_when_update_omits_it() {
        let path = std::env::temp_dir().join(format!(
            "openless-core-quick-note-audio-flag-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store = HistoryStore::at_path(path.clone());
        let mut original = session("audio", chrono::Utc::now().to_rfc3339());
        original.has_audio_recording = Some(true);
        store.append_with_retention(original, 0, None).unwrap();
        let mut replacement = session("audio", chrono::Utc::now().to_rfc3339());
        replacement.has_audio_recording = None;
        store.upsert_with_retention(replacement, 0, None).unwrap();
        assert_eq!(store.list().unwrap()[0].has_audio_recording, Some(true));
        let _ = std::fs::remove_file(path);
    }
}
