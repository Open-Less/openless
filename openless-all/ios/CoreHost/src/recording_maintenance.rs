//! Reconcile only at startup or under the Host's admission/idle gate. The tiny
//! per-asset journal records ownership, not transcripts or a second history DB.
//! Unknown orphans remain in place, indexed as `quarantined`; only a previously
//! linked asset can be deleted merely because its Core history row disappeared.
use super::recordings::{
    canonical_id, open_regular, open_wav, prepare_directory, regular_metadata, storage_error,
};
use super::*;
use openless_core::types::{DictationSession, HistorySource};
use std::collections::{BTreeSet, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const MAX_DIRECTORY_ENTRIES: usize = 8192;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Journal {
    Capturing,
    Linked,
    Quarantined,
    Discarding,
}
impl Journal {
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Capturing => b"v1 capturing\n",
            Self::Linked => b"v1 linked\n",
            Self::Quarantined => b"v1 quarantined\n",
            Self::Discarding => b"v1 discarding\n",
        }
    }
    fn parse(bytes: &[u8]) -> Result<Self, BackendError> {
        [
            Self::Capturing,
            Self::Linked,
            Self::Quarantined,
            Self::Discarding,
        ]
        .into_iter()
        .find(|state| state.bytes() == bytes)
        .ok_or_else(storage_error)
    }
}

#[derive(Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MaintenanceReport {
    pub(super) recovered: Vec<String>,
    /// Durable quarantine index: `<id>.lifecycle` + the original `<id>.wav`.
    /// No synthetic successful history row is created for these files.
    pub(super) quarantined: Vec<String>,
    pub(super) removed: Vec<String>,
    pub(super) rejected: Vec<String>,
}

fn read_journal_file(path: &Path) -> Result<Journal, BackendError> {
    if regular_metadata(path)?.len() > 32 {
        return Err(storage_error());
    }
    let mut bytes = Vec::new();
    open_regular(path, false)?
        .take(33)
        .read_to_end(&mut bytes)
        .map_err(|_| storage_error())?;
    Journal::parse(&bytes)
}
fn read_journal(directory: &Path, id: &str) -> Result<Option<Journal>, BackendError> {
    let path = directory.join(format!("{id}.lifecycle"));
    match std::fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(storage_error()),
        Ok(_) => read_journal_file(&path).map(Some),
    }
}
fn sync_directory(directory: &Path) -> Result<(), BackendError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| storage_error())
}
fn write_journal(directory: &Path, id: &str, state: Journal) -> Result<(), BackendError> {
    canonical_id(id)?;
    let path = directory.join(format!("{id}.lifecycle"));
    if read_journal(directory, id)? == Some(state) {
        return Ok(());
    }
    let temp = directory.join(format!("{id}.lifecycle.tmp"));
    // A process crash may leave an unpublished journal. Never follow or replace
    // an unrecognized file, alias or oversized input, even at our temp name.
    match std::fs::symlink_metadata(&temp) {
        Ok(_) => {
            read_journal_file(&temp)?;
            std::fs::remove_file(&temp).map_err(|_| storage_error())?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(storage_error()),
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temp).map_err(|_| storage_error())?;
    file.write_all(state.bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| storage_error())?;
    std::fs::rename(temp, path).map_err(|_| storage_error())?;
    sync_directory(directory)
}
fn remove_journal(directory: &Path, id: &str) -> Result<(), BackendError> {
    if read_journal(directory, id)?.is_some() {
        std::fs::remove_file(directory.join(format!("{id}.lifecycle")))
            .map_err(|_| storage_error())?;
        sync_directory(directory)?;
    }
    Ok(())
}

pub(super) fn begin_capture(directory: &Path, id: &str) -> Result<(), BackendError> {
    canonical_id(id)?;
    if std::fs::symlink_metadata(directory.join(format!("{id}.wav"))).is_ok()
        || read_journal(directory, id)?.is_some()
    {
        return Err(storage_error());
    }
    write_journal(directory, id, Journal::Capturing)
}

/// Called while management owns the admission/control exclusion, before Core
/// removes rows. A durable link distinguishes intentional deletion from orphan
/// capture recovery, even if unlinking later fails or the process exits.
pub(super) fn before_history_removal(
    directory: &Path,
    entries: &[DictationSession],
) -> Result<(), BackendError> {
    prepare_directory(directory)?;
    for entry in entries {
        if canonical_id(&entry.id).is_err() {
            continue;
        }
        let path = directory.join(format!("{}.wav", entry.id));
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                open_wav(&path, true)?;
                write_journal(directory, &entry.id, Journal::Linked)?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(storage_error()),
        }
    }
    Ok(())
}

pub(super) fn after_history_removal(
    backend: &OpenLessBackend,
    directory: &Path,
) -> Result<(), BackendError> {
    let report = reconcile(backend, directory)?;
    if report.rejected.is_empty() {
        Ok(())
    } else {
        Err(storage_error())
    }
}

fn successful_ephemeral(entry: &DictationSession) -> bool {
    // Do not infer success from timestamps, a missing error, or future states.
    // Quick Notes have an explicit different Core retention contract.
    entry.source == HistorySource::Voice
        && !entry.raw_transcript.trim().is_empty()
        && matches!(
            entry.error_code.as_deref(),
            None | Some("polishFailed" | "insertFailed")
        )
}
fn unfinished(entry: &DictationSession) -> bool {
    matches!(
        entry.error_code.as_deref(),
        Some(
            "recording"
                | "recordingFailed"
                | "transcribeFailed"
                | "emptyTranscript"
                | "cancelled"
                | "interrupted"
        )
    )
}

/// Caller must exclude *starting*, capture, processing and history mutations for
/// the entire pass. No timer or age heuristic is safe for an active recording.
fn reconcile(
    backend: &OpenLessBackend,
    directory: &Path,
) -> Result<MaintenanceReport, BackendError> {
    prepare_directory(directory)?;
    let mut entries: HashMap<_, _> = backend
        .list_history()?
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect();
    let mut ids = BTreeSet::new();
    // Bound enumeration before any mutation. Ignore all unrelated resources.
    for (count, entry) in std::fs::read_dir(directory)
        .map_err(|_| storage_error())?
        .enumerate()
    {
        if count >= MAX_DIRECTORY_ENTRIES {
            return Err(storage_error());
        }
        let entry = entry.map_err(|_| storage_error())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let id = name
            .strip_suffix(".wav")
            .or_else(|| name.strip_suffix(".lifecycle"))
            .or_else(|| name.strip_suffix(".lifecycle.tmp"));
        if let Some(id) = id.filter(|id| canonical_id(id).is_ok()) {
            ids.insert(id.to_owned());
        }
    }
    // Also settle a recording draft when the crash preceded creation/first PCM.
    for entry in entries.values().filter(|entry| {
        entry.error_code.as_deref() == Some("recording")
            || entry.has_audio_recording == Some(true)
                && (unfinished(entry) || entry.source == HistorySource::QuickNote)
    }) {
        if canonical_id(&entry.id).is_ok() {
            ids.insert(entry.id.clone());
        }
    }
    let mut report = MaintenanceReport::default();
    for id in ids {
        if reconcile_asset(backend, directory, &id, entries.remove(&id), &mut report).is_err() {
            report.rejected.push(id);
        }
    }
    Ok(report)
}

fn reconcile_asset(
    backend: &OpenLessBackend,
    directory: &Path,
    id: &str,
    entry: Option<DictationSession>,
    report: &mut MaintenanceReport,
) -> Result<(), BackendError> {
    let state = read_journal(directory, id)?;
    let temp = directory.join(format!("{id}.lifecycle.tmp"));
    if std::fs::symlink_metadata(&temp).is_ok() {
        read_journal_file(&temp)?;
        std::fs::remove_file(&temp).map_err(|_| storage_error())?;
        sync_directory(directory)?;
    }
    let path = directory.join(format!("{id}.wav"));
    let bytes = match std::fs::symlink_metadata(&path) {
        Ok(_) => match open_wav(&path, true) {
            Ok((_, bytes)) => Some(bytes),
            Err(err) => {
                // A crash can precede even the first complete header. Preserve
                // untrusted bytes, but do not leave a draft claiming live audio.
                if let Some(mut entry) =
                    entry.filter(|entry| entry.error_code.as_deref() == Some("recording"))
                {
                    entry.error_code = Some("interrupted".into());
                    entry.has_audio_recording = Some(false);
                    entry.duration_ms = None;
                    backend.update_history_entry(entry)?;
                }
                return Err(err);
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(storage_error()),
    };
    let Some(mut entry) = entry else {
        if matches!(state, Some(Journal::Linked | Journal::Discarding)) {
            // Ownership was journalled before history deletion/retention. Keep
            // that evidence until the unlink is durable, then forget it.
            write_journal(directory, id, Journal::Discarding)?;
            recordings::remove(directory, id)?;
            remove_journal(directory, id)?;
            report.removed.push(id.into());
        } else if bytes.is_some() {
            write_journal(directory, id, Journal::Quarantined)?;
            report.quarantined.push(id.into());
        } else {
            remove_journal(directory, id)?;
        }
        return Ok(());
    };
    // Always establish durable linkage before a later delete can remove history.
    write_journal(directory, id, Journal::Linked)?;
    let before = entry.clone();
    if successful_ephemeral(&entry) {
        write_journal(directory, id, Journal::Discarding)?;
        recordings::remove(directory, id)?;
        entry.has_audio_recording = Some(false);
        if entry != before {
            backend.update_history_entry(entry)?;
        }
        remove_journal(directory, id)?;
        if bytes.is_some() {
            report.removed.push(id.into());
        }
        return Ok(());
    }
    if entry.error_code.as_deref() == Some("recording") {
        entry.error_code = Some("interrupted".into());
        // Duration is measured from actual PCM, not stale capture metadata.
        entry.duration_ms = bytes
            .filter(|bytes| *bytes > 0)
            .map(|bytes| u64::from(bytes) * 1000 / 32_000);
    }
    if unfinished(&entry) || entry.source == HistorySource::QuickNote {
        entry.has_audio_recording = Some(bytes.is_some_and(|bytes| bytes > 0));
    }
    if entry != before {
        backend.update_history_entry(entry)?;
        report.recovered.push(id.into());
    }
    // Header-only files contain no recoverable audio. Referenced empty drafts
    // keep their truthful interrupted record, but never a player/export flag.
    if bytes == Some(0) {
        recordings::remove(directory, id)?;
    }
    if bytes.is_none() || bytes == Some(0) {
        remove_journal(directory, id)?;
    }
    Ok(())
}

/// Run before the Host handle is published; no native capture can yet exist.
pub(super) fn at_startup(
    backend: &OpenLessBackend,
    directory: &Path,
) -> Result<MaintenanceReport, BackendError> {
    reconcile(backend, directory)
}

impl Host {
    /// Uses the same admission mutex as Start and management dispatch. A query
    /// during capture must neither scan the directory nor touch its journal.
    pub(super) fn maintain_recordings_if_idle(
        &self,
    ) -> Result<Option<MaintenanceReport>, BackendError> {
        let admission = self.admission.lock().map_err(|_| storage_error())?;
        if admission.is_some()
            || !self
                .broker
                .captures
                .lock()
                .map_err(|_| storage_error())?
                .is_empty()
            || self.backend.snapshot().dictation.session_id.is_some()
        {
            return Ok(None);
        }
        if self.control_busy.swap(true, Ordering::AcqRel) {
            return Ok(None);
        }
        let result = reconcile(&self.backend, &self.recordings_directory);
        self.control_busy.store(false, Ordering::Release);
        drop(admission);
        result.map(Some)
    }
}

#[cfg(test)]
#[path = "recording_maintenance_tests.rs"]
mod tests;
