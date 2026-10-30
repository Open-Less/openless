use std::collections::HashMap;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use futures_util::future::BoxFuture;

use crate::{BackendError, BackendErrorCode, RecordingArchive, RecordingPlan, SessionId};

pub(super) struct ExternalRecordingArchive {
    path: PathBuf,
    writer: Mutex<(Option<std::fs::File>, u32)>,
    cleanup_paths: Mutex<Vec<PathBuf>>,
    available: AtomicBool,
}

impl ExternalRecordingArchive {
    pub(super) fn create(
        directory: &Path,
        session_id: SessionId,
        plan: &RecordingPlan,
    ) -> std::io::Result<Self> {
        Self::create_named(directory, format!("{session_id}.wav"), plan)
    }

    pub(super) fn create_raw(
        directory: &Path,
        session_id: SessionId,
        plan: &RecordingPlan,
    ) -> std::io::Result<Self> {
        Self::create_named(directory, format!("{session_id}.raw.wav"), plan)
    }

    fn create_named(
        directory: &Path,
        file_name: String,
        plan: &RecordingPlan,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let mut groups: HashMap<uuid::Uuid, (SystemTime, Vec<PathBuf>)> = HashMap::new();
        for entry in std::fs::read_dir(directory)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let session_stem = stem.strip_suffix(".raw").unwrap_or(stem);
            let Ok(session_id) = uuid::Uuid::parse_str(session_stem) else {
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            groups
                .entry(session_id)
                .and_modify(|group| {
                    group.0 = group.0.max(modified);
                    group.1.push(path.clone());
                })
                .or_insert_with(|| (modified, vec![path]));
        }
        let now = SystemTime::now();
        let retention = plan.retention_days;
        groups.retain(|_, (modified, paths)| {
            let expired = retention > 0
                && now
                    .duration_since(*modified)
                    .is_ok_and(|age| age.as_secs() > u64::from(retention) * 86_400);
            if expired {
                for path in paths {
                    let _ = std::fs::remove_file(path);
                }
            }
            !expired
        });
        let cap = plan
            .max_entries
            .map(|count| (count as usize).clamp(1, crate::history::HISTORY_CAP))
            .unwrap_or(crate::history::HISTORY_CAP);
        let mut ordered: Vec<_> = groups.into_iter().collect();
        ordered.sort_by_key(|(_, (modified, _))| std::cmp::Reverse(*modified));
        for (_, (_, paths)) in ordered.into_iter().skip(cap.saturating_sub(1)) {
            for path in paths {
                let _ = std::fs::remove_file(path);
            }
        }

        let path = directory.join(file_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(&crate::audio::encode_dictation_wav(&[]).expect("empty PCM is valid"))?;
        Ok(Self {
            path,
            writer: Mutex::new((Some(file), 0)),
            cleanup_paths: Mutex::new(Vec::new()),
            available: AtomicBool::new(false),
        })
    }

    pub(super) fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub(super) fn add_cleanup_path(&self, path: PathBuf) {
        self.cleanup_paths
            .lock()
            .expect("external archive cleanup lock poisoned")
            .push(path);
    }

    pub(super) fn append(&self, pcm: &[u8]) {
        let mut writer = self.writer.lock().expect("external archive lock poisoned");
        if writer.0.is_none() {
            return;
        }
        let result = (|| -> std::io::Result<()> {
            let size = u32::try_from(pcm.len())
                .ok()
                .and_then(|size| writer.1.checked_add(size))
                .filter(|size| *size <= u32::MAX - 36)
                .ok_or_else(|| std::io::Error::other("remote recording exceeds WAV size limit"))?;
            let file = writer.0.as_mut().unwrap();
            file.write_all(pcm)?;
            file.seek(SeekFrom::Start(4))?;
            file.write_all(&(36 + size).to_le_bytes())?;
            file.seek(SeekFrom::Start(40))?;
            file.write_all(&size.to_le_bytes())?;
            file.seek(SeekFrom::End(0))?;
            writer.1 = size;
            Ok(())
        })();
        if let Err(error) = result {
            log::warn!("[remote-input] 写入录音归档失败：{error}");
            writer.0 = None;
            self.available.store(false, Ordering::Release);
        } else {
            self.available.store(true, Ordering::Release);
        }
    }

    pub(super) fn finish(&self) {
        let mut writer = self.writer.lock().expect("external archive lock poisoned");
        if let Some(file) = writer.0.take() {
            if let Err(error) = file.sync_data() {
                log::warn!("[remote-input] 同步录音归档失败：{error}");
                self.available.store(false, Ordering::Release);
            }
        }
        if writer.1 == 0 {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl RecordingArchive for ExternalRecordingArchive {
    fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let wav = std::fs::read(path).map_err(|error| {
                BackendError::new(BackendErrorCode::Persistence, error.to_string())
            })?;
            if wav.len() <= 44
                || &wav[..4] != b"RIFF"
                || &wav[8..12] != b"WAVE"
                || !(wav.len() - 44).is_multiple_of(2)
            {
                return Err(BackendError::new(
                    BackendErrorCode::Persistence,
                    "remote recording WAV is empty or invalid",
                ));
            }
            Ok(wav[44..].to_vec())
        })
    }

    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        self.writer
            .lock()
            .expect("external archive lock poisoned")
            .0 = None;
        let result = match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(BackendError::new(
                BackendErrorCode::Persistence,
                error.to_string(),
            )),
        };
        if result.is_ok() {
            self.available.store(false, Ordering::Release);
            for path in self
                .cleanup_paths
                .lock()
                .expect("external archive cleanup lock poisoned")
                .drain(..)
            {
                let _ = std::fs::remove_file(path);
            }
        }
        Box::pin(async move { result })
    }
}
