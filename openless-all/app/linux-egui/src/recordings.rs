use std::fmt;
use std::path::{Path, PathBuf};

pub const MAX_RECORDING_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug)]
pub enum RecordingError {
    InvalidSession,
    NotFound,
    TooLarge,
    InvalidWav,
    Io(std::io::Error),
}

impl fmt::Display for RecordingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSession => f.write_str("invalid recording session id"),
            Self::NotFound => f.write_str("recording not found"),
            Self::TooLarge => f.write_str("recording exceeds the size limit"),
            Self::InvalidWav => f.write_str("recording is not canonical PCM WAV"),
            Self::Io(error) => write!(f, "recording I/O failed: {error}"),
        }
    }
}

impl std::error::Error for RecordingError {}

pub fn recording_path(data_dir: &Path, session_id: &str) -> Result<PathBuf, RecordingError> {
    let parsed = uuid::Uuid::parse_str(session_id).map_err(|_| RecordingError::InvalidSession)?;
    if parsed.to_string() != session_id {
        return Err(RecordingError::InvalidSession);
    }
    Ok(data_dir
        .join("recordings")
        .join(format!("{session_id}.wav")))
}

pub fn read_recording_wav(data_dir: &Path, session_id: &str) -> Result<Vec<u8>, RecordingError> {
    let path = recording_path(data_dir, session_id)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RecordingError::NotFound
        } else {
            RecordingError::Io(error)
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RecordingError::InvalidWav);
    }
    if metadata.len() > MAX_RECORDING_BYTES {
        return Err(RecordingError::TooLarge);
    }
    let wav = std::fs::read(path).map_err(RecordingError::Io)?;
    recording_pcm(&wav)?;
    Ok(wav)
}

pub fn recording_pcm(wav: &[u8]) -> Result<&[u8], RecordingError> {
    if wav.len() <= 44
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || &wav[12..16] != b"fmt "
        || u16::from_le_bytes([wav[20], wav[21]]) != 1
        || u16::from_le_bytes([wav[22], wav[23]]) != 1
        || u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]) != 16_000
        || u16::from_le_bytes([wav[34], wav[35]]) != 16
        || &wav[36..40] != b"data"
        || !(wav.len() - 44).is_multiple_of(2)
    {
        return Err(RecordingError::InvalidWav);
    }
    let declared = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
    if declared != wav.len() - 44 {
        return Err(RecordingError::InvalidWav);
    }
    Ok(&wav[44..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_path_rejects_traversal_and_wav_contract_is_strict() {
        assert!(recording_path(Path::new("/tmp/openless"), "../escape").is_err());
        let id = uuid::Uuid::new_v4().to_string();
        assert!(recording_path(Path::new("/tmp/openless"), &id)
            .unwrap()
            .ends_with(format!("{id}.wav")));
        let mut wav = crate::audio::wav_header(2).to_vec();
        wav.extend_from_slice(&[1, 0]);
        assert_eq!(recording_pcm(&wav).unwrap(), [1, 0]);
        wav[24] = 0;
        assert!(recording_pcm(&wav).is_err());
    }
}
