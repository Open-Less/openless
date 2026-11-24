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
    let wav = tokio::fs::read(&path).await.map_err(|error| {
        match error.kind() {
            std::io::ErrorKind::NotFound => "historyRetranscribeMissing",
            std::io::ErrorKind::PermissionDenied => "historyRetranscribePermission",
            _ => "historyRetranscribeStorage",
        }
        .to_string()
    })?;
    let pcm = openless_core::audio::decode_dictation_wav(&wav)
        .map_err(|_| "historyRetranscribeInvalidAudio".to_string())?;
    if pcm.is_empty() {
        return Err("historyRetranscribeInvalidAudio".into());
    }

    let retranscribe_started = std::time::Instant::now();
    let retranscription = core
        .services()
        .auxiliary
        .retranscribe_pcm(pcm.to_vec())
        .await
        .map_err(|failure| {
            // Only bounded categories leave this command; provider bodies may contain secrets.
            let reason = retranscription_error_reason(&failure.error);
            log::warn!(
                "[history] retranscribe id={} bytes={} failed reason={} code={:?}",
                session_id,
                pcm.len(),
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
            pcm.len()
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
