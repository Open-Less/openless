//! Canonical Linux recording archive access, independent of native audio devices.
use futures_util::future::BoxFuture;
use openless_core::{BackendError, BackendErrorCode};
use std::sync::Arc;

pub(super) struct LinuxRecordingArchive {
    pub(super) path: std::path::PathBuf,
    pub(super) available: Arc<std::sync::atomic::AtomicBool>,
}

impl openless_core::RecordingArchive for LinuxRecordingArchive {
    fn is_available(&self) -> bool {
        self.available.load(std::sync::atomic::Ordering::Acquire)
    }

    fn pcm_byte_len(&self) -> BoxFuture<'static, Result<u64, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut file = std::fs::File::open(path).map_err(|error| {
                    BackendError::new(BackendErrorCode::Persistence, error.to_string())
                })?;
                openless_core::audio::canonical_archive_pcm_len(&mut file)
            })
            .await
            .map_err(|error| BackendError::new(BackendErrorCode::Persistence, error.to_string()))?
        })
    }

    fn read_pcm_range(
        &self,
        offset: u64,
        len: usize,
    ) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut file = std::fs::File::open(path).map_err(|error| {
                    BackendError::new(BackendErrorCode::Persistence, error.to_string())
                })?;
                openless_core::audio::read_canonical_archive_pcm_range(&mut file, offset, len)
            })
            .await
            .map_err(|error| BackendError::new(BackendErrorCode::Persistence, error.to_string()))?
        })
    }

    fn read_pcm(&self) -> BoxFuture<'static, Result<Vec<u8>, BackendError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let wav = tokio::fs::read(path).await.map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Persistence,
                    format!("read Linux recording archive: {error}"),
                )
            })?;
            openless_core::audio::decode_dictation_wav(&wav).map(ToOwned::to_owned)
        })
    }

    fn discard(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        let path = self.path.clone();
        let available = Arc::clone(&self.available);
        Box::pin(async move {
            match tokio::fs::remove_file(path).await {
                Ok(()) => available.store(false, std::sync::atomic::Ordering::Release),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    available.store(false, std::sync::atomic::Ordering::Release);
                }
                Err(error) => {
                    return Err(BackendError::new(
                        BackendErrorCode::Persistence,
                        format!("discard Linux recording archive: {error}"),
                    ));
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn archive_range_reads_preserve_bounds_and_validate_final_lengths() {
        use openless_core::RecordingArchive;
        let path =
            std::env::temp_dir().join(format!("openless-linux-range-{}.wav", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            openless_core::audio::encode_dictation_wav(&[1, 2, 3, 4, 5, 6]).unwrap(),
        )
        .unwrap();
        let archive = LinuxRecordingArchive {
            path: path.clone(),
            available: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        assert_eq!(archive.pcm_byte_len().await.unwrap(), 6);
        assert_eq!(archive.read_pcm_range(2, 4).await.unwrap(), [3, 4, 5, 6]);
        assert!(archive.read_pcm_range(4, 4).await.is_err());
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(46)
            .unwrap();
        assert!(archive.pcm_byte_len().await.is_err());
        archive.discard().await.unwrap();
    }
}
