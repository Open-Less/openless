use crate::{required, Host};
use base64::Engine;
use openless_core::{DictationSession, HistorySource, SessionId};
use serde_json::{json, Value};

fn recording(host: &Host, args: &Value) -> Result<(DictationSession, Vec<u8>), String> {
    // Canonicalize the UUID before constructing a sandbox path.
    let id: SessionId = serde_json::from_value(json!(required(args, "sessionId")?))
        .map_err(|_| "invalid session id")?;
    let entry = host
        .backend
        .list_history()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|entry| entry.id == id.to_string())
        .ok_or("history entry not found")?;
    if host.backend.snapshot().dictation.session_id == Some(id) {
        return Err("recording is still active; stop it first".into());
    }
    if entry.has_audio_recording != Some(true) {
        return Err("history entry has no archived recording".into());
    }
    let path = host
        .backend
        .config()
        .data_dir
        .join("recordings")
        .join(format!("{id}.wav"));
    let size = std::fs::metadata(&path)
        .map_err(|_| "recording not found")?
        .len();
    if size <= 44 || size > 64 * 1024 * 1024 {
        return Err("recording is empty or too large".into());
    }
    let wav = std::fs::read(path).map_err(|e| e.to_string())?;
    validate_wav(&wav)?;
    Ok((entry, wav))
}

fn validate_wav(wav: &[u8]) -> Result<(), String> {
    // ExternalAudioRecorder writes this canonical PCM header. Reject altered
    // formats rather than sending arbitrary file bytes to the ASR provider.
    if wav.len() <= 44
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || &wav[12..16] != b"fmt "
        || wav[16..20] != 16u32.to_le_bytes()
        || wav[20..22] != 1u16.to_le_bytes()
        || wav[22..24] != 1u16.to_le_bytes()
        || wav[24..28] != 16000u32.to_le_bytes()
        || wav[28..32] != 32000u32.to_le_bytes()
        || wav[32..34] != 2u16.to_le_bytes()
        || wav[34..36] != 16u16.to_le_bytes()
        || &wav[36..40] != b"data"
        || (wav.len() - 44) % 2 != 0
        || u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize != wav.len() - 8
        || u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize != wav.len() - 44
    {
        return Err("recording is corrupt or not 16 kHz mono PCM".into());
    }
    Ok(())
}

pub fn remove_audio(host: &Host, id: &str) -> Result<(), String> {
    let session: SessionId = serde_json::from_value(json!(id)).map_err(|_| "invalid session id")?;
    if host.backend.snapshot().dictation.session_id == Some(session) {
        return Err("stop the active recording before deleting it".into());
    }
    let path = host
        .backend
        .config()
        .data_dir
        .join("recordings")
        .join(format!("{session}.wav"));
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn read(host: &Host, args: &Value) -> Result<Value, String> {
    let (_, wav) = recording(host, args)?;
    Ok(json!(format!(
        "data:audio/wav;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(wav)
    )))
}

pub fn retranscribe(host: &Host, args: &Value) -> Result<Value, String> {
    let (entry, wav) = recording(host, args)?;
    if entry.pipeline_mode.as_deref() == Some("multimodal")
        && entry.source != HistorySource::QuickNote
    {
        return Err("multimodal history does not support retranscription".into());
    }
    let started = std::time::Instant::now();
    let result = host
        .runtime
        .block_on(
            host.backend
                .services()
                .auxiliary
                .retranscribe_pcm(wav[44..].to_vec()),
        )
        .map_err(openless_core::RetranscriptionFailure::into_message)?;
    if result.text.trim().is_empty() {
        return Err("重新转录仍未识别到语音".into());
    }
    let updated = if matches!(
        entry.error_code.as_deref(),
        Some("transcribeFailed" | "emptyTranscript" | "recording")
    ) || entry.source == HistorySource::QuickNote
    {
        Some(
            host.backend
                .apply_history_retranscription(
                    &entry.id,
                    result.text.clone(),
                    &result.asr,
                    started.elapsed().as_millis() as u64,
                )
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    Ok(json!({"text":result.text,"updatedEntry":updated}))
}
