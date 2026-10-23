//! App-private, bounded WAV archives implementing Core's recording lifecycle.
//! Native PCM arrives on the capture delivery worker, never on the audio tap.
//! Core retains failed ASR and discards successful/cancelled ordinary recordings.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

pub(super) const MAX_BYTES: u32 = 16_000 * 2 * 30 * 60;
pub(super) const HEADER_LEN: u64 = 44;
struct State {
    file: Option<File>,
    bytes: u32,
}
pub(super) struct WavArchive {
    path: PathBuf,
    state: Mutex<State>,
}

pub(super) fn storage_error() -> BackendError {
    error(
        BackendErrorCode::Persistence,
        "recording file is unavailable",
    )
}
pub(super) fn header(bytes: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(44);
    data.extend(b"RIFF");
    data.extend((36 + bytes).to_le_bytes());
    data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(16_000u32.to_le_bytes());
    data.extend(32_000u32.to_le_bytes());
    data.extend(2u16.to_le_bytes());
    data.extend(16u16.to_le_bytes());
    data.extend(b"data");
    data.extend(bytes.to_le_bytes());
    data
}

/// Reject aliases in every ancestor, not just the final filename. All callers
/// supply the main App's private recordings directory, never an App Group root.
pub(super) fn prepare_directory(directory: &Path) -> Result<(), BackendError> {
    if !directory.is_absolute() {
        return Err(storage_error());
    }
    let mut path = PathBuf::new();
    for component in directory.components() {
        match component {
            Component::RootDir | Component::Normal(_) => path.push(component.as_os_str()),
            _ => return Err(storage_error()),
        }
        if !path.exists() {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            builder.create(&path).map_err(|_| storage_error())?;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| storage_error())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(storage_error());
        }
    }
    Ok(())
}

pub(super) fn canonical_id(id: &str) -> Result<Uuid, BackendError> {
    let uuid = Uuid::parse_str(id)
        .map_err(|_| error(BackendErrorCode::InvalidArgument, "invalid recording id"))?;
    if uuid.to_string() != id {
        return Err(error(
            BackendErrorCode::InvalidArgument,
            "invalid recording id",
        ));
    }
    Ok(uuid)
}

pub(super) fn regular_metadata(path: &Path) -> Result<std::fs::Metadata, BackendError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| storage_error())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(storage_error());
    }
    #[cfg(unix)]
    if metadata.nlink() != 1 {
        return Err(storage_error());
    }
    Ok(metadata)
}

/// Check the opened inode too, so replacing a checked leaf with an alias cannot
/// cause us to read/write the alias target. Directory mutation is serialized by
/// the Host idle gate; the directory itself is private to this App process.
pub(super) fn open_regular(path: &Path, write: bool) -> Result<File, BackendError> {
    let before = regular_metadata(path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(write)
        .open(path)
        .map_err(|_| storage_error())?;
    let opened = file.metadata().map_err(|_| storage_error())?;
    #[cfg(unix)]
    if before.dev() != opened.dev() || before.ino() != opened.ino() || opened.nlink() != 1 {
        return Err(storage_error());
    }
    Ok(file)
}

/// Only our exact PCM format is accepted. Recovery may repair stale length
/// words after a chunk write, but never interprets arbitrary RIFF chunks/codecs.
pub(super) fn open_wav(path: &Path, repair: bool) -> Result<(File, u32), BackendError> {
    let mut file = open_regular(path, repair)?;
    let len = file.metadata().map_err(|_| storage_error())?.len();
    if len < HEADER_LEN || len > u64::from(MAX_BYTES) + HEADER_LEN || (len - HEADER_LEN) % 2 != 0 {
        return Err(storage_error());
    }
    let bytes = (len - HEADER_LEN) as u32;
    let mut found = [0; 44];
    file.read_exact(&mut found).map_err(|_| storage_error())?;
    let expected = header(bytes);
    if found[..4] != expected[..4] || found[8..40] != expected[8..40] {
        return Err(storage_error());
    }
    let riff_len = u32::from_le_bytes(found[4..8].try_into().unwrap());
    let data_len = u32::from_le_bytes(found[40..44].try_into().unwrap());
    if data_len > bytes
        || data_len % 2 != 0
        || riff_len < 36
        || riff_len > bytes + 36
        || riff_len % 2 != 0
    {
        return Err(storage_error());
    }
    if found.as_slice() != expected {
        if !repair {
            return Err(storage_error());
        }
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(&expected))
            .and_then(|_| file.sync_all())
            .map_err(|_| storage_error())?;
    }
    Ok((file, bytes))
}

impl WavArchive {
    pub(super) fn create(directory: &Path, id: SessionId) -> Result<Arc<Self>, BackendError> {
        prepare_directory(directory)?;
        // This is not proof that Core persisted its draft. An unlinked capture
        // is quarantined, never treated as deleted history on the next launch.
        super::recording_maintenance::begin_capture(directory, &id.to_string())?;
        let path = directory.join(format!("{id}.wav"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&path).map_err(|_| storage_error())?;
        if file
            .write_all(&header(0))
            .and_then(|_| file.sync_all())
            .is_err()
        {
            let _ = std::fs::remove_file(&path);
            return Err(storage_error());
        }
        File::open(directory)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| storage_error())?;
        Ok(Arc::new(Self {
            path,
            state: Mutex::new(State {
                file: Some(file),
                bytes: 0,
            }),
        }))
    }
    pub(super) fn append(&self, pcm: &[u8]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.file.is_none() {
            return;
        }
        let count = u32::try_from(pcm.len())
            .ok()
            .filter(|n| n % 2 == 0)
            .and_then(|n| state.bytes.checked_add(n))
            .filter(|n| *n <= MAX_BYTES);
        let success = count.is_some_and(|count| {
            let Some(file) = state.file.as_mut() else {
                return false;
            };
            let result = file
                .seek(SeekFrom::End(0))
                .and_then(|_| file.write_all(pcm))
                .and_then(|_| file.seek(SeekFrom::Start(0)))
                .and_then(|_| file.write_all(&header(count)));
            if result.is_ok() {
                state.bytes = count;
                true
            } else {
                false
            }
        });
        if !success {
            state.file = None;
            if count.is_none() {
                state.bytes = 0;
                let _ = std::fs::remove_file(&self.path);
            }
            // An I/O failure can leave previously valid PCM on disk. Stop
            // appending but preserve it for bounded startup validation/repair.
        }
    }
    pub(super) fn finish(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(file) = state.file.take() {
            if state.bytes == 0 {
                let _ = std::fs::remove_file(&self.path);
            } else {
                // A failed flush is not permission to destroy recoverable PCM.
                let _ = file.sync_all();
            }
        }
    }
}
impl RecordingArchive for WavArchive {
    fn is_available(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.bytes > 0) && self.path.is_file()
    }
    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            prepare_directory(path.parent().ok_or_else(storage_error)?)?;
            let (mut file, bytes) = open_wav(&path, false)?;
            let mut data = vec![0; bytes as usize];
            file.seek(SeekFrom::Start(HEADER_LEN))
                .and_then(|_| file.read_exact(&mut data))
                .map_err(|_| storage_error())?;
            Ok(data)
        })
    }
    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let path = self.path.clone();
        let result = self
            .state
            .lock()
            .map_err(|_| storage_error())
            .map(|mut state| {
                state.file = None;
                state.bytes = 0;
            });
        Box::pin(async move {
            result?;
            let directory = path.parent().ok_or_else(storage_error)?;
            let id = path
                .file_stem()
                .and_then(|id| id.to_str())
                .ok_or_else(storage_error)?;
            remove(directory, id)
        })
    }
}

pub(super) fn asset_path(directory: &Path, id: &str) -> Result<PathBuf, BackendError> {
    prepare_directory(directory)?;
    let uuid = canonical_id(id)?;
    let path = directory.join(format!("{uuid}.wav"));
    if open_wav(&path, false)?.1 == 0 {
        return Err(storage_error());
    }
    Ok(path)
}
pub(super) fn remove(directory: &Path, id: &str) -> Result<(), BackendError> {
    prepare_directory(directory)?;
    let Ok(uuid) = canonical_id(id) else {
        return Ok(());
    };
    let path = directory.join(format!("{uuid}.wav"));
    match std::fs::symlink_metadata(&path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(storage_error()),
        Ok(_) => {
            open_wav(&path, false)?;
        }
    }
    std::fs::remove_file(path)
        .and_then(|_| File::open(directory)?.sync_all())
        .map_err(|_| storage_error())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> tempfile::TempDir {
        tempfile::Builder::new()
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap()
    }
    #[test]
    fn wav_header_survives_each_chunk_and_finished_audio_is_canonical() {
        let dir = directory();
        let id = SessionId::new();
        let archive = WavArchive::create(dir.path(), id).unwrap();
        assert!(!archive.is_available());
        archive.append(&[1, 2, 3, 4]);
        archive.append(&[5, 6]);
        archive.finish();
        assert!(archive.is_available());
        let data = std::fs::read(asset_path(dir.path(), &id.to_string()).unwrap()).unwrap();
        assert_eq!(&data[..4], b"RIFF");
        assert_eq!(&data[40..44], &6u32.to_le_bytes());
        assert_eq!(&data[44..], &[1, 2, 3, 4, 5, 6]);
    }
    #[test]
    fn invalid_frame_discards_asset_and_never_overwrites_existing_recording() {
        let dir = directory();
        let id = SessionId::new();
        let archive = WavArchive::create(dir.path(), id).unwrap();
        assert!(WavArchive::create(dir.path(), id).is_err());
        archive.append(&[1]);
        assert!(!archive.is_available());
        assert!(asset_path(dir.path(), &id.to_string()).is_err());
        assert!(asset_path(dir.path(), "../outside").is_err());
    }
    #[test]
    fn discard_is_idempotent_and_late_frames_cannot_recreate_audio() {
        let dir = directory();
        let id = SessionId::new();
        let archive = WavArchive::create(dir.path(), id).unwrap();
        archive.append(&[0, 0]);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            archive.discard().await.unwrap();
            archive.discard().await.unwrap();
        });
        archive.append(&[1, 2]);
        assert!(!archive.is_available());
        assert!(asset_path(dir.path(), &id.to_string()).is_err());
    }
}
