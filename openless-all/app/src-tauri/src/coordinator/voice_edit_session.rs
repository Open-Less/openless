//! Host adapter for the core Voice Edit Session state machine (Issue #900).
//! Core owns drafts and phases; Host binds every asynchronous operation to its
//! originating session and keeps native writes behind the commit reservation.

use super::Inner;
use crate::types::InsertStatus;
use openless_core::{
    DictationOutputTarget, DictationSession, DictationStartOptions, HistoryInsertStatus,
    HistorySource, PolishMode, SessionId, TextSelection, VoiceEditPhase, VoiceEditSession,
    VoiceEditSnapshot,
};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct VoiceEditHostState {
    pub(crate) session: Option<VoiceEditSession>,
    pub(crate) pending_start_id: Option<SessionId>,
    pub(crate) dictation_session_id: Option<SessionId>,
    pub(crate) finishing_initial_dictation: bool,
    pub(crate) target: Option<VoiceEditNativeTarget>,
    pub(crate) initial_raw_text: String,
    pub(crate) duration_ms: u64,
}

#[derive(Clone)]
pub(crate) enum VoiceEditNativeTarget {
    #[cfg(target_os = "android")]
    Android { generation: i64 },
    #[cfg(not(target_os = "android"))]
    Desktop {
        target: crate::host_document::NativeVoiceEditTarget,
    },
}

fn current_session(
    host: &mut VoiceEditHostState,
    id: SessionId,
) -> Result<&mut VoiceEditSession, String> {
    let session = host
        .session
        .as_mut()
        .ok_or_else(|| "voiceEditSessionUnavailable".to_string())?;
    if session.session_id() != id {
        return Err("voiceEditSessionChanged".into());
    }
    Ok(session)
}

pub(super) async fn start(inner: &Arc<Inner>) -> Result<VoiceEditSnapshot, String> {
    start_with_recording(inner, None).await
}

pub(super) async fn from_overlay(inner: &Arc<Inner>) -> Result<VoiceEditSnapshot, String> {
    let recording = inner.backend.snapshot().dictation;
    if !matches!(
        recording.phase,
        openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
    ) || inner.backend.dictation_output_target() != Some(DictationOutputTarget::Undecided)
    {
        return Err("voiceEditInitialDictationUnavailable".into());
    }
    let recording_id = recording
        .session_id
        .ok_or_else(|| "voiceEditDictationUnavailable".to_string())?;
    let snapshot = start_with_recording(inner, Some(recording_id)).await?;
    finish_dictation(inner, snapshot.session_id).await
}

async fn start_with_recording(
    inner: &Arc<Inner>,
    existing_recording: Option<SessionId>,
) -> Result<VoiceEditSnapshot, String> {
    if !inner.backend.get_preferences().voice_edit_enabled {
        return Err("voiceEditDisabled".into());
    }
    let start_id = SessionId::new();
    {
        let mut host = inner.voice_edit_host.lock();
        if host.pending_start_id.is_some()
            || host.dictation_session_id.is_some()
            || host.session.as_ref().is_some_and(|s| {
                !matches!(
                    s.snapshot().phase,
                    VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
                )
            })
        {
            return Err("voiceEditSessionBusy".into());
        }
        host.pending_start_id = Some(start_id);
    }
    let capture = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        tokio::task::spawn_blocking(capture_target),
    )
    .await
    .map_err(|_| "voiceEditTargetUnavailable".to_string())
    .and_then(|result| result.map_err(|error| format!("voiceEditTargetCaptureFailed:{error}")))
    .and_then(|result| result);
    let session_id = {
        let mut host = inner.voice_edit_host.lock();
        if host.pending_start_id != Some(start_id) {
            return Err("voiceEditSessionChanged".into());
        }
        host.pending_start_id = None;
        let (text, selection, target) = capture?;
        let session = VoiceEditSession::start(text, selection).map_err(|e| e.to_string())?;
        let id = session.session_id();
        host.session = Some(session);
        host.target = Some(target);
        host.dictation_session_id = None;
        host.finishing_initial_dictation = false;
        host.initial_raw_text.clear();
        host.duration_ms = 0;
        id
    };
    if let Err(error) = inner.backend.start().await {
        let _ = cancel(inner, Some(session_id)).await;
        return Err(error.to_string());
    }
    {
        let mut host = inner.voice_edit_host.lock();
        if current_session(&mut host, session_id)?.snapshot().phase != VoiceEditPhase::Dictating {
            return Err("voiceEditSessionChanged".into());
        }
    }
    let recording = match existing_recording {
        Some(id) => Ok(id),
        None => {
            inner
                .backend
                .start_dictation_with_options(DictationStartOptions {
                    insert_text: false,
                    output_target: DictationOutputTarget::VoiceEdit,
                    ..DictationStartOptions::default()
                })
                .await
        }
    };
    let recording_id = match recording {
        Ok(id) => id,
        Err(error) => {
            let _ = cancel(inner, Some(session_id)).await;
            return Err(error.to_string());
        }
    };
    let snapshot = {
        let mut host = inner.voice_edit_host.lock();
        match current_session(&mut host, session_id) {
            Ok(session) if session.snapshot().phase == VoiceEditPhase::Dictating => {
                let snapshot = session.snapshot();
                host.dictation_session_id = Some(recording_id);
                inner.host.set_voice_edit_interactive(true);
                Some(snapshot)
            }
            _ => None,
        }
    };
    let Some(snapshot) = snapshot else {
        let _ = inner.backend.cancel_dictation(Some(recording_id)).await;
        return Err("voiceEditSessionChanged".into());
    };
    Ok(snapshot)
}

pub(super) async fn finish_dictation(
    inner: &Arc<Inner>,
    id: SessionId,
) -> Result<VoiceEditSnapshot, String> {
    let recording_id = {
        let mut host = inner.voice_edit_host.lock();
        if current_session(&mut host, id)?.snapshot().phase != VoiceEditPhase::Dictating {
            return Err("voiceEditInitialDictationUnavailable".into());
        }
        if host.finishing_initial_dictation {
            return Err("voiceEditSessionBusy".into());
        }
        let recording_id = host
            .dictation_session_id
            .ok_or_else(|| "voiceEditDictationUnavailable".to_string())?;
        host.finishing_initial_dictation = true;
        recording_id
    };
    let result = inner
        .backend
        .stop_dictation_session_with_options(
            Some(recording_id),
            openless_core::DictationStopOptions::default(),
            Some(DictationOutputTarget::VoiceEdit),
        )
        .await;
    release_recording(inner, id, recording_id);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let _ = cancel(inner, Some(id)).await;
            return Err(error.to_string());
        }
    };
    let draft = {
        let mut host = inner.voice_edit_host.lock();
        let session = current_session(&mut host, id)?;
        match session.finish_dictation(non_empty_or_fallback(
            &result.polished_text,
            &result.raw_text,
        )) {
            Ok(()) => {
                let snapshot = session.snapshot();
                host.initial_raw_text = result.raw_text;
                host.duration_ms = result.duration_ms;
                Ok(snapshot)
            }
            Err(error) => Err(error.to_string()),
        }
    };
    if draft.is_err() {
        let _ = cancel(inner, Some(id)).await;
    }
    draft
}

pub(super) async fn start_instruction(
    inner: &Arc<Inner>,
    id: SessionId,
) -> Result<VoiceEditSnapshot, String> {
    {
        let mut host = inner.voice_edit_host.lock();
        current_session(&mut host, id)?
            .enter_editing()
            .map_err(|e| e.to_string())?;
    }
    let recording_id = match inner
        .backend
        .start_dictation_with_options(DictationStartOptions {
            insert_text: false,
            output_target: DictationOutputTarget::VoiceEdit,
            ..DictationStartOptions::default()
        })
        .await
    {
        Ok(id) => id,
        Err(error) => {
            recover_after_instruction_error(inner, id);
            return Err(error.to_string());
        }
    };
    let snapshot = {
        let mut host = inner.voice_edit_host.lock();
        match current_session(&mut host, id) {
            Ok(session) if session.snapshot().phase == VoiceEditPhase::Editing => {
                let snapshot = session.snapshot();
                host.dictation_session_id = Some(recording_id);
                Some(snapshot)
            }
            _ => None,
        }
    };
    match snapshot {
        Some(snapshot) => Ok(snapshot),
        None => {
            let _ = inner.backend.cancel_dictation(Some(recording_id)).await;
            Err("voiceEditSessionChanged".into())
        }
    }
}

pub(super) async fn finish_instruction(
    inner: &Arc<Inner>,
    id: SessionId,
) -> Result<VoiceEditSnapshot, String> {
    let recording_id = {
        let mut host = inner.voice_edit_host.lock();
        let recording_id = host
            .dictation_session_id
            .ok_or_else(|| "voiceEditInstructionUnavailable".to_string())?;
        current_session(&mut host, id)?
            .begin_applying()
            .map_err(|e| e.to_string())?;
        recording_id
    };
    let result = inner.backend.stop_dictation_session(recording_id).await;
    release_recording(inner, id, recording_id);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            recover_after_instruction_error(inner, id);
            return Err(error.to_string());
        }
    };
    let (field_context, draft) = {
        let mut host = inner.voice_edit_host.lock();
        let session = current_session(&mut host, id)?;
        if session.snapshot().phase != VoiceEditPhase::Applying {
            return Err("voiceEditSessionChanged".into());
        }
        let context = session
            .snapshot()
            .context
            .ok_or_else(|| "voiceEditDraftUnavailable".to_string())?;
        (context.field_text, context.preview)
    };
    let raw = non_empty_or_fallback(&result.raw_text, &result.polished_text);
    let polished = non_empty_or_fallback(&result.polished_text, &result.raw_text);
    let generated = match inner
        .backend
        .services()
        .selection_voice
        .voice_edit_plan(openless_core::domains::VoiceEditPlanRequest {
            session_id: id,
            field_context,
            draft,
            instruction_raw: raw.clone(),
            instruction_polished: polished,
        })
        .await
    {
        Ok(generated) => generated,
        Err(error) => {
            recover_after_instruction_error(inner, id);
            return Err(error.to_string());
        }
    };
    let mut host = inner.voice_edit_host.lock();
    let session = current_session(&mut host, id)?;
    if let Err(error) =
        session.apply_instruction(raw, generated.instruction_polished, generated.plan)
    {
        let _ = session.recover_instruction();
        return Err(error.to_string());
    }
    Ok(session.snapshot())
}

fn recover_after_instruction_error(inner: &Arc<Inner>, id: SessionId) {
    let mut host = inner.voice_edit_host.lock();
    if let Ok(session) = current_session(&mut host, id) {
        let _ = session.recover_instruction();
    }
}

fn release_recording(inner: &Arc<Inner>, id: SessionId, recording_id: SessionId) {
    let mut host = inner.voice_edit_host.lock();
    if host.session.as_ref().map(VoiceEditSession::session_id) == Some(id)
        && host.dictation_session_id == Some(recording_id)
    {
        host.dictation_session_id = None;
        host.finishing_initial_dictation = false;
    }
}

pub(super) async fn commit(inner: &Arc<Inner>, id: SessionId) -> Result<VoiceEditSnapshot, String> {
    let (ticket, target, raw, duration) = {
        let mut host = inner.voice_edit_host.lock();
        let target = host
            .target
            .clone()
            .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?;
        let ticket = current_session(&mut host, id)?
            .begin_commit()
            .map_err(|e| e.to_string())?;
        (
            ticket,
            target,
            host.initial_raw_text.clone(),
            host.duration_ms,
        )
    };
    let writer = Arc::clone(inner);
    let text = ticket.text.clone();
    let outcome = tokio::task::spawn_blocking(move || apply_native_target(&writer, &target, &text))
        .await
        .map_err(|error| format!("voiceEditReplaceFailed:{error}"))
        .and_then(|result| result);
    let mut host = inner.voice_edit_host.lock();
    let session = current_session(&mut host, id)?;
    if let Err(error) = outcome {
        let _ = session.recover_commit();
        #[cfg(target_os = "android")]
        inner.host.show_voice_edit();
        return Err(error);
    }
    session.complete_commit().map_err(|e| e.to_string())?;
    let snapshot = session.snapshot();
    inner.host.set_voice_edit_interactive(false);
    drop(host);
    persist_history(&inner.backend, &raw, duration, &ticket);
    Ok(snapshot)
}

pub(super) async fn cancel(
    inner: &Arc<Inner>,
    id: Option<SessionId>,
) -> Result<Option<VoiceEditSnapshot>, String> {
    let (recording_id, snapshot) = {
        let mut host = inner.voice_edit_host.lock();
        if let Some(id) = id {
            let _ = current_session(&mut host, id)?;
        }
        if let Some(session) = host.session.as_mut() {
            if !matches!(
                session.snapshot().phase,
                VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
            ) {
                session.cancel().map_err(|e| e.to_string())?;
            }
        }
        host.pending_start_id = None;
        let snapshot = host.session.as_ref().map(VoiceEditSession::snapshot);
        host.target = None;
        inner.host.set_voice_edit_interactive(false);
        (host.dictation_session_id, snapshot)
    };
    if let Some(recording_id) = recording_id {
        let result = inner.backend.cancel_dictation(Some(recording_id)).await;
        // Core can release ownership before adapter cleanup reports an error.
        // Keep a retry handle only while this exact backend session is still live.
        let still_owned = inner.backend.snapshot().dictation.session_id == Some(recording_id);
        let mut host = inner.voice_edit_host.lock();
        if !still_owned && host.dictation_session_id == Some(recording_id) {
            host.dictation_session_id = None;
            host.finishing_initial_dictation = false;
        }
        result.map_err(|e| e.to_string())?;
    }
    Ok(snapshot)
}

pub(super) fn can_close(inner: &Arc<Inner>, id: Option<SessionId>) -> Result<(), String> {
    let mut host = inner.voice_edit_host.lock();
    if let Some(id) = id {
        let _ = current_session(&mut host, id)?;
    }
    if host.pending_start_id.is_some()
        || host.dictation_session_id.is_some()
        || host.session.as_ref().is_some_and(|s| {
            !matches!(
                s.snapshot().phase,
                VoiceEditPhase::Completed | VoiceEditPhase::Cancelled
            )
        })
    {
        return Err("voiceEditSessionBusy".into());
    }
    Ok(())
}

pub(super) fn snapshot(inner: &Arc<Inner>) -> Option<VoiceEditSnapshot> {
    inner
        .voice_edit_host
        .lock()
        .session
        .as_ref()
        .map(VoiceEditSession::snapshot)
}

fn capture_target() -> Result<(String, Option<TextSelection>, VoiceEditNativeTarget), String> {
    #[cfg(target_os = "android")]
    {
        let raw = crate::android::capture_voice_edit_target()
            .map_err(|e| format!("voiceEditTargetCaptureFailed:{e}"))?
            .ok_or_else(|| "voiceEditTargetUnavailable".to_string())?;
        let (generation, text, start, end) = parse_android_target(&raw)?;
        return Ok((
            text,
            (start != end).then_some(TextSelection { start, end }),
            VoiceEditNativeTarget::Android { generation },
        ));
    }
    #[cfg(not(target_os = "android"))]
    {
        let (text, selection, target) = crate::host_document::capture_voice_edit_target()?;
        Ok((text, selection, VoiceEditNativeTarget::Desktop { target }))
    }
}

#[cfg(target_os = "android")]
fn parse_android_target(raw: &str) -> Result<(i64, String, u32, u32), String> {
    let mut parts = raw.splitn(6, '|');
    let generation = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let _package = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let _window = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let start = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let end = parts
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let text = parts
        .next()
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?
        .to_string();
    let start = android_utf16_offset_to_char_offset(&text, start)
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    let end = android_utf16_offset_to_char_offset(&text, end)
        .ok_or_else(|| "voiceEditTargetProtocolError".to_string())?;
    Ok((generation, text, start, end))
}

/// Android accessibility reports selection offsets in UTF-16 code units while
/// the core session contract uses Unicode scalar-value offsets. Convert only at
/// code-point boundaries and fail closed for malformed/surrogate-split input;
/// never index a UTF-8 `str` with an Android `usize` offset.
fn android_utf16_offset_to_char_offset(text: &str, utf16_offset: usize) -> Option<u32> {
    let mut units = 0usize;
    for (char_offset, character) in text.chars().enumerate() {
        if units == utf16_offset {
            return u32::try_from(char_offset).ok();
        }
        units = units.checked_add(character.len_utf16())?;
        if units > utf16_offset {
            return None;
        }
    }
    (units == utf16_offset)
        .then(|| u32::try_from(text.chars().count()).ok())
        .flatten()
}

fn apply_native_target(
    inner: &Arc<Inner>,
    target: &VoiceEditNativeTarget,
    text: &str,
) -> Result<(), String> {
    match target {
        #[cfg(target_os = "android")]
        VoiceEditNativeTarget::Android { generation } => {
            let backgrounded = crate::android::jni::android::with_android_env(|env, context| {
                crate::android::jni::android::background_voice_edit_host(env, context)
            })
            .map_err(|error| format!("voiceEditReplaceFailed:{error}"))?;
            if !backgrounded {
                return Err("voiceEditTargetUnavailable".into());
            }
            // Each accessibility request already has a 500 ms IPC timeout.
            let retry_deadline = std::time::Instant::now() + std::time::Duration::from_millis(1000);
            loop {
                let result = crate::android::replace_voice_edit_target(*generation, text)
                    .map_err(|error| format!("voiceEditReplaceFailed:{error}"))?;
                if result == crate::android::accessibility::PASTE_RESULT_SUCCESS {
                    return Ok(());
                }
                // Only temporary panel focus is retryable; rejected writes are never repeated.
                if result != "NO_FOCUSED_EDITOR" || std::time::Instant::now() >= retry_deadline {
                    return Err(format!("voiceEditReplaceFailed:{result}"));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        #[cfg(not(target_os = "android"))]
        VoiceEditNativeTarget::Desktop { target } => {
            crate::host_document::apply_voice_edit_target(target, text, |text| {
                let preferences = inner.backend.get_preferences();
                match inner.inserter.insert(
                    text,
                    preferences.restore_clipboard_after_paste,
                    preferences.paste_shortcut,
                ) {
                    InsertStatus::Inserted | InsertStatus::PasteSent => Ok(()),
                    InsertStatus::CopiedFallback => Err("voiceEditInsertFallback".into()),
                    InsertStatus::Failed | InsertStatus::NotRequested => {
                        Err("voiceEditInsertFailed".into())
                    }
                }
            })
        }
    }
}

fn persist_history(
    backend: &openless_core::OpenLessBackend,
    initial_raw_text: &str,
    duration_ms: u64,
    commit: &openless_core::VoiceEditCommit,
) {
    let preferences = backend.get_preferences();
    let turns = commit
        .turns
        .iter()
        .map(|turn| turn.instruction_polished.as_str())
        .collect::<Vec<_>>()
        .join("；");
    let raw = if turns.is_empty() {
        initial_raw_text.to_string()
    } else if initial_raw_text.is_empty() {
        turns
    } else {
        format!("{}；{}", initial_raw_text, turns)
    };
    let session = DictationSession {
        id: commit.session_id.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source: HistorySource::VoiceEdit,
        raw_transcript: raw,
        asr_transcript: None,
        final_text: commit.text.clone(),
        mode: PolishMode::Light,
        style_pack_id: None,
        translation_active: false,
        polish_source: Some("voice_edit_session".to_string()),
        app_bundle_id: None,
        app_name: None,
        insert_status: HistoryInsertStatus::Inserted,
        error_code: None,
        duration_ms: Some(duration_ms),
        dictionary_entry_count: None,
        has_audio_recording: None,
        asr_provider: None,
        asr_model: None,
        llm_provider: None,
        llm_model: None,
        pipeline_mode: None,
        asr_ms: None,
        polish_ms: None,
    };
    if let Err(error) = backend.append_history(
        session,
        preferences.history_retention_days,
        preferences.history_max_entries,
    ) {
        log::warn!("voice edit history persistence failed: {error}");
    }
}

fn non_empty_or_fallback(primary: &str, fallback: &str) -> String {
    if primary.trim().is_empty() {
        fallback.trim().to_string()
    } else {
        primary.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{android_utf16_offset_to_char_offset, non_empty_or_fallback};

    #[test]
    fn dictation_text_falls_back_to_raw_when_polish_is_empty() {
        assert_eq!(non_empty_or_fallback("  ", " raw "), "raw");
        assert_eq!(non_empty_or_fallback(" polished ", "raw"), "polished");
    }

    #[test]
    fn android_utf16_offsets_convert_without_utf8_indexing() {
        assert_eq!(android_utf16_offset_to_char_offset("a😀中", 0), Some(0));
        assert_eq!(android_utf16_offset_to_char_offset("a😀中", 1), Some(1));
        assert_eq!(android_utf16_offset_to_char_offset("a😀中", 3), Some(2));
        assert_eq!(android_utf16_offset_to_char_offset("a😀中", 4), Some(3));
        assert_eq!(android_utf16_offset_to_char_offset("a😀中", 2), None);
    }
}
