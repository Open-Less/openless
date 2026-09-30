use super::*;

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::new(
        BackendErrorCode::Persistence,
        format!("cloud note cleanup: {error}"),
    )
}
fn remove_if_present(path: &std::path::Path) -> Result<(), BackendError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}
impl OpenLessBackend {
    fn cloud_note_marker(&self, id: SessionId) -> std::path::PathBuf {
        self.config
            .data_dir
            .join("cloud-note-cleanup")
            .join(id.to_string())
    }
    pub(super) fn is_cloud_note(&self, id: SessionId) -> bool {
        self.cloud_note_marker(id).is_file()
    }

    /// Select the destination before finalization; stale UI gestures cannot retarget a new session.
    pub fn set_dictation_cloud_note(
        &self,
        id: SessionId,
        enabled: bool,
    ) -> Result<(), BackendError> {
        let mut state = self.state.write().expect("backend state lock poisoned");
        ensure_active_session(&state, id)?;
        if !matches!(
            state.dictation.phase,
            DictationPhase::Starting | DictationPhase::Recording
        ) {
            return Err(BackendError::new(
                BackendErrorCode::Busy,
                "dictation destination is already frozen",
            ));
        }
        let marker = self.cloud_note_marker(id);
        if enabled {
            std::fs::create_dir_all(marker.parent().unwrap()).map_err(io_error)?;
            std::fs::File::create(&marker)
                .and_then(|file| file.sync_all())
                .map_err(io_error)?;
        } else {
            remove_if_present(&marker)?;
        }
        state.dictation_destination_changed = true;
        let target = if enabled {
            DictationOutputTarget::CloudNote
        } else {
            DictationOutputTarget::ForegroundApp
        };
        if let Some(context) = &state.dictation_context {
            state.dictation_context = Some(Arc::new(context.with_output_target(target)));
        } else {
            state.dictation_start_output_target = Some(target);
        }
        Ok(())
    }

    pub(super) fn cleanup_cloud_note(
        &self,
        id: SessionId,
        finished: bool,
    ) -> Result<(), BackendError> {
        if !self.is_cloud_note(id) {
            return Ok(());
        }
        for folder in ["recordings", "quick-notes/recordings"] {
            remove_if_present(&self.config.data_dir.join(folder).join(format!("{id}.wav")))?;
        }
        self.delete_history(&id.to_string())?;
        if finished {
            remove_if_present(&self.cloud_note_marker(id))?;
        }
        Ok(())
    }

    pub(super) fn recover_cloud_notes(&self) -> Result<(), BackendError> {
        let directory = self.config.data_dir.join("cloud-note-cleanup");
        if !directory.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            // A file name must be a canonical UUID before it can address any recording.
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(uuid) = uuid::Uuid::parse_str(&name) else {
                continue;
            };
            if uuid.to_string() != name {
                continue;
            }
            let id: SessionId =
                serde_json::from_value(serde_json::Value::String(name)).map_err(|_| {
                    BackendError::new(BackendErrorCode::Persistence, "invalid cleanup session")
                })?;
            self.cleanup_cloud_note(id, true)?;
        }
        Ok(())
    }
}
