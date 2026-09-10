use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use futures_util::future::BoxFuture;

use crate::{BackendError, BackendErrorCode, RecordingArchive, RecordingPlan, SessionId};

pub(super) struct ExternalRecordingArchive {
    path: PathBuf,
    writer: Mutex<(Option<std::fs::File>, u32)>,
    available: AtomicBool,
}

impl ExternalRecordingArchive {
    pub(super) fn create(
        directory: &Path,
        session_id: SessionId,
        plan: &RecordingPlan,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        // 只清理本应用生成的 UUID.wav，且为本次录音预留一个名额。
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(directory)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav")
                || path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| uuid::Uuid::parse_str(stem).ok())
                    .is_none()
            {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
            if plan.retention_days > 0
                && modified
                    .elapsed()
                    .is_ok_and(|age| age.as_secs() > u64::from(plan.retention_days) * 86_400)
            {
                let _ = std::fs::remove_file(path);
            } else {
                entries.push((path, modified));
            }
        }
        entries.sort_by(|left, right| right.1.cmp(&left.1));
        let cap = plan
            .max_entries
            .map(|count| (count as usize).clamp(1, crate::history::HISTORY_CAP))
            .unwrap_or(crate::history::HISTORY_CAP);
        for (path, _) in entries.into_iter().skip(cap - 1) {
            let _ = std::fs::remove_file(path);
        }
        let path = directory.join(format!("{session_id}.wav"));
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
            available: AtomicBool::new(false),
        })
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
            // 每帧修正标准头；无需等手机发送 stop，即可读取磁盘上已收到的音频。
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
        }
        Box::pin(async move { result })
    }
}
