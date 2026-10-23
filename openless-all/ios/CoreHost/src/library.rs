//! Product operations reuse Core repositories and auxiliary use-cases. No secrets
//! or audio bytes enter the keyboard mailbox. Lists contain bounded previews;
//! original text is fetched only for an explicitly opened record.
use super::*;
use openless_core::auxiliary::RepolishRequest;
use openless_core::types::DictationSession;

pub(super) fn history_summary(entry: &DictationSession) -> Value {
    json!({
        "id":entry.id, "createdAt":entry.created_at,
        "finalText":entry.final_text.chars().take(160).collect::<String>(),
        "rawTranscript":entry.raw_transcript.chars().take(160).collect::<String>(),
        "errorCode":entry.error_code, "durationMs":entry.duration_ms,
        "mode":entry.mode, "stylePackId":entry.style_pack_id,
        "hasAudioRecording":entry.has_audio_recording,
        "characterCount":entry.final_text.chars().count(), "isPreview":true
    })
}

impl Host {
    /// Control dispatch owns control_busy; hold admission throughout the durable
    /// journal -> Core mutation -> asset reconciliation transaction. Startup can
    /// finish an interrupted unlink without reintroducing deleted history rows.
    pub(super) fn delete_history_with_recordings(&self, id: &str) -> Result<(), BackendError> {
        let _admission = self.history_maintenance_admission()?;
        let entries: Vec<_> = self.backend.list_history()?.into_iter().filter(|entry| entry.id == id).collect();
        recording_maintenance::before_history_removal(&self.recordings_directory, &entries)?;
        self.backend.delete_history(id)?;
        recording_maintenance::after_history_removal(&self.backend, &self.recordings_directory)
    }

    pub(super) fn clear_history_with_recordings(&self) -> Result<(), BackendError> {
        let _admission = self.history_maintenance_admission()?;
        recording_maintenance::before_history_removal(&self.recordings_directory, &self.backend.list_history()?)?;
        self.backend.clear_history()?;
        // Core preserves Quick Notes. Re-read Core history instead of deleting
        // every asset from the pre-clear snapshot.
        recording_maintenance::after_history_removal(&self.backend, &self.recordings_directory)
    }

    fn history_maintenance_admission(&self) -> Result<std::sync::MutexGuard<'_, Option<Admission>>, BackendError> {
        let admission = self.admission.lock().map_err(|_| recordings::storage_error())?;
        if !self.control_busy.load(Ordering::Acquire) || admission.is_some()
            || !self.broker.captures.lock().map_err(|_| recordings::storage_error())?.is_empty()
            || self.backend.snapshot().dictation.session_id.is_some() {
            return Err(error(BackendErrorCode::Busy, "recording or processing is active"));
        }
        Ok(admission)
    }

    pub(super) fn library_snapshot(&self) -> Result<Value, BackendError> {
        let history = self.backend.list_history()?;
        let preferences = self.backend.get_preferences();
        let styles: Vec<Value> = self.backend.list_style_packs(&preferences.active_style_pack_id)?
            .into_iter().filter(|pack| pack.enabled)
            .map(|pack| json!({"id":pack.id,"name":pack.name})).collect();
        Ok(json!({"history":history.iter().map(history_summary).collect::<Vec<_>>(),
            "activity":self.backend.list_activity()?, "styles":styles}))
    }

    pub(super) fn search_history(&self, query: &str) -> Result<Value, BackendError> {
        let query = query.trim().to_lowercase();
        let entries = self.backend.list_history()?;
        let matches: Vec<_> = entries.iter().filter(|entry| history_matches(entry, &query))
            .take(200).map(history_summary).collect();
        Ok(json!({"entries": matches}))
    }

    pub(super) fn history_entry(&self, id: &str) -> Result<DictationSession, BackendError> {
        self.backend.list_history()?.into_iter().find(|entry| entry.id == id)
            .ok_or_else(|| error(BackendErrorCode::InvalidArgument, "history entry not found"))
    }

    pub(super) async fn repolish_history(&self, id: &str, style_id: String) -> Result<Value, BackendError> {
        let entry = self.history_entry(id)?;
        let pack = self.backend.get_style_pack(&style_id)?;
        if !pack.enabled || entry.raw_transcript.trim().is_empty() {
            return Err(error(BackendErrorCode::InvalidArgument, "nonempty transcript and enabled style required"));
        }
        if style_id != "builtin.raw" && !self.backend.get_credentials_status().await?.llm_configured {
            return Err(error(BackendErrorCode::InvalidState, "configure an LLM before polishing history"));
        }
        let text = self.backend.services().auxiliary.repolish(RepolishRequest {
            raw_text:entry.raw_transcript, style_pack_id:Some(style_id), front_app:None,
        }).await?;
        // A preview never overwrites the original record or inserts into an app.
        Ok(json!({"text":text}))
    }
}

fn history_matches(entry: &DictationSession, query: &str) -> bool {
    query.is_empty() || [&entry.final_text, &entry.raw_transcript]
        .into_iter().chain(entry.asr_transcript.as_ref())
        .any(|text| text.to_lowercase().contains(query))
}
