use openless_core::DictationSession;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetranscriptionResult {
    pub text: String,
    pub updated_entry: Option<DictationSession>,
}

pub(super) async fn retranscribe_recording_with_path(
    core: &openless_core::OpenLessBackend,
    session_id: String,
    recording_path: impl FnOnce(&str) -> Result<std::path::PathBuf, String>,
) -> Result<HistoryRetranscriptionResult, String> {
    let entry = core
        .list_history()
        .map_err(|_| "historyRetranscribeStorage".to_string())?
        .into_iter()
        .find(|entry| entry.id == session_id)
        .ok_or_else(|| "historyRetranscribeMissing".to_string())?;
    if core
        .snapshot()
        .dictation
        .session_id
        .is_some_and(|active| active.to_string() == session_id)
    {
        return Err("historyRetranscribeActive".into());
    }
    if entry.has_audio_recording != Some(true) {
        return Err("historyRetranscribeMissing".into());
    }
    if entry.pipeline_mode.as_deref() == Some("multimodal")
        && entry.source != openless_core::HistorySource::QuickNote
    {
        return Err("historyRetranscribeUnsupported".into());
    }
    let path = recording_path(&session_id).map_err(|_| "historyRetranscribeMissing".to_string())?;
    let archive = tokio::task::spawn_blocking(move || HistoryWavArchive::open(&path))
        .await
        .map_err(|_| "historyRetranscribeStorage".to_string())??;
    let pcm_bytes = archive.len;

    let retranscribe_started = std::time::Instant::now();
    let retranscription = core
        .services()
        .auxiliary
        .retranscribe_archive(std::sync::Arc::new(archive))
        .await
        .map_err(|failure| {
            // Only bounded categories leave this command; provider bodies may contain secrets.
            let reason = retranscription_error_reason(&failure.error);
            log::warn!(
                "[history] retranscribe id={} bytes={} failed reason={} code={:?}",
                session_id,
                pcm_bytes,
                reason,
                failure.error.code
            );
            reason.to_string()
        })?;
    let text = retranscription.text;
    let asr_call_label = retranscription.asr;
    if text.trim().is_empty() {
        log::info!(
            "[history] retranscribe id={} bytes={} result=empty_transcript",
            session_id,
            pcm_bytes
        );
        return Err("historyRetranscribeNoSpeech".into());
    }
    let retranscribe_ms = retranscribe_started.elapsed().as_millis() as u64;

    let updated_entry = if entry.can_apply_retranscription() {
        Some(
            core.apply_history_retranscription(
                &session_id,
                text.clone(),
                &asr_call_label,
                retranscribe_ms,
            )
            .map_err(|_| "historyRetranscribeStorage".to_string())?,
        )
    } else {
        None
    };
    Ok(HistoryRetranscriptionResult {
        text,
        updated_entry,
    })
}

fn retranscription_error_reason(error: &openless_core::BackendError) -> &'static str {
    use openless_core::BackendErrorCode;
    match error.code {
        BackendErrorCode::Cancelled => "user cancelled",
        BackendErrorCode::PermissionDenied => "historyRetranscribePermission",
        BackendErrorCode::Busy => "historyRetranscribeBusy",
        BackendErrorCode::Unsupported => "historyRetranscribeUnsupported",
        BackendErrorCode::InvalidArgument => "historyRetranscribeConfiguration",
        BackendErrorCode::Persistence => "historyRetranscribeStorage",
        _ => "historyRetranscribeProvider",
    }
}

// Hold the file open for the entire request; reads stay bounded to one ASR segment.
struct HistoryWavArchive {
    file: std::sync::Arc<std::sync::Mutex<std::fs::File>>,
    offset: u64,
    len: u64,
}

impl HistoryWavArchive {
    fn open(path: &std::path::Path) -> Result<Self, String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).map_err(|error| {
            match error.kind() {
                std::io::ErrorKind::NotFound => "historyRetranscribeMissing",
                std::io::ErrorKind::PermissionDenied => "historyRetranscribePermission",
                _ => "historyRetranscribeStorage",
            }
            .to_string()
        })?;
        let invalid = || "historyRetranscribeInvalidAudio".to_string();
        let size = file.metadata().map_err(|_| invalid())?.len();
        let mut riff = [0; 12];
        file.read_exact(&mut riff).map_err(|_| invalid())?;
        if &riff[..4] != b"RIFF"
            || &riff[8..] != b"WAVE"
            || u64::from(u32::from_le_bytes(riff[4..8].try_into().unwrap())) + 8 != size
        {
            return Err(invalid());
        }
        let mut position = 12u64;
        let mut format_ok = false;
        let mut data = None;
        while position < size {
            file.seek(SeekFrom::Start(position))
                .map_err(|_| invalid())?;
            let mut header = [0; 8];
            file.read_exact(&mut header).map_err(|_| invalid())?;
            let len = u64::from(u32::from_le_bytes(header[4..].try_into().unwrap()));
            let end = position + 8 + len;
            if end > size {
                return Err(invalid());
            }
            match &header[..4] {
                b"fmt " => {
                    if len < 16 {
                        return Err(invalid());
                    }
                    let mut fmt = [0; 16];
                    file.read_exact(&mut fmt).map_err(|_| invalid())?;
                    format_ok = fmt[..2] == 1u16.to_le_bytes()
                        && fmt[2..4] == 1u16.to_le_bytes()
                        && fmt[4..8] == 16_000u32.to_le_bytes()
                        && fmt[8..12] == 32_000u32.to_le_bytes()
                        && fmt[12..14] == 2u16.to_le_bytes()
                        && fmt[14..16] == 16u16.to_le_bytes();
                    if !format_ok {
                        return Err(invalid());
                    }
                }
                b"data" => {
                    if data.is_some() || len == 0 || !len.is_multiple_of(2) {
                        return Err(invalid());
                    }
                    data = Some((position + 8, len));
                }
                _ => {}
            }
            position = end + len % 2;
        }
        if !format_ok || position != size {
            return Err(invalid());
        }
        let (offset, len) = data.ok_or_else(invalid)?;
        Ok(Self {
            file: std::sync::Arc::new(std::sync::Mutex::new(file)),
            offset,
            len,
        })
    }
}

impl openless_core::RecordingArchive for HistoryWavArchive {
    fn is_available(&self) -> bool {
        true
    }
    fn pcm_byte_len(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Result<u64, openless_core::BackendError>> {
        let len = self.len;
        Box::pin(async move { Ok(len) })
    }
    fn read_pcm(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Result<Vec<u8>, openless_core::BackendError>>
    {
        let Ok(len) = usize::try_from(self.len) else {
            return Box::pin(async {
                Err(openless_core::BackendError::new(
                    openless_core::BackendErrorCode::Persistence,
                    "recording archive exceeds addressable size",
                ))
            });
        };
        self.read_pcm_range(0, len)
    }
    fn read_pcm_range(
        &self,
        offset: u64,
        len: usize,
    ) -> futures_util::future::BoxFuture<'static, Result<Vec<u8>, openless_core::BackendError>>
    {
        let file = self.file.clone();
        let start = (offset.is_multiple_of(2)
            && len.is_multiple_of(2)
            && offset <= self.len
            && len as u64 <= self.len - offset)
            .then(|| self.offset.checked_add(offset))
            .flatten();
        Box::pin(async move {
            let read = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
                use std::io::{Read, Seek, SeekFrom};
                let start =
                    start.ok_or_else(|| std::io::Error::other("PCM range out of bounds"))?;
                let mut file = file
                    .lock()
                    .map_err(|_| std::io::Error::other("archive lock poisoned"))?;
                file.seek(SeekFrom::Start(start))?;
                let mut pcm = vec![0; len];
                file.read_exact(&mut pcm)?;
                Ok(pcm)
            })
            .await;
            read.map_err(|error| {
                openless_core::BackendError::new(
                    openless_core::BackendErrorCode::Persistence,
                    error.to_string(),
                )
            })?
            .map_err(|error| {
                openless_core::BackendError::new(
                    openless_core::BackendErrorCode::Persistence,
                    error.to_string(),
                )
            })
        })
    }
    fn discard(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Result<(), openless_core::BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use openless_core::RecordingArchive;
    use std::io::{Seek, SeekFrom, Write};

    #[tokio::test]
    async fn sparse_long_archive_reads_only_requested_tail_and_rejects_short_reads() {
        let path = std::env::temp_dir().join(format!(
            "openless-history-range-{}.wav",
            uuid::Uuid::new_v4()
        ));
        let pcm_len = 31 * 60 * 32_000u32;
        let mut header = openless_core::audio::encode_dictation_wav(&[0, 0]).unwrap();
        header.truncate(44);
        header[4..8].copy_from_slice(&(36 + pcm_len).to_le_bytes());
        header[40..44].copy_from_slice(&pcm_len.to_le_bytes());
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&header).unwrap();
        file.set_len(44 + u64::from(pcm_len)).unwrap();
        file.seek(SeekFrom::End(-4)).unwrap();
        file.write_all(&[1, 2, 3, 4]).unwrap();
        let archive = HistoryWavArchive::open(&path).unwrap();
        assert_eq!(archive.pcm_byte_len().await.unwrap(), u64::from(pcm_len));
        assert_eq!(
            archive
                .read_pcm_range(u64::from(pcm_len) - 4, 4)
                .await
                .unwrap(),
            [1, 2, 3, 4]
        );
        assert!(archive
            .read_pcm_range(u64::from(pcm_len) - 2, 4)
            .await
            .is_err());
        assert!(archive.read_pcm_range(1, 2).await.is_err());
        assert!(archive.read_pcm_range(0, 3).await.is_err());
        assert!(archive.read_pcm_range(u64::MAX - 1, 2).await.is_err());
        file.set_len(44).unwrap();
        assert!(archive
            .read_pcm_range(u64::from(pcm_len) - 4, 4)
            .await
            .is_err());
        drop(archive);
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
