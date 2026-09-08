//! Core semantic-event to legacy React event bridge.
//!
//! This module is the only place where core change notifications are translated
//! into WebView event names. Window-specific state remains owned by the Tauri
//! host; the core never sees labels such as `main` or `capsule`.

use std::sync::Arc;

use openless_core::{
    BackendEventKind, CapsulePayload, CapsuleState, CapsuleStyle, DictationPhase,
    DictationStateSnapshot, EventRecvError, LocalAsrRuntimeKind, OpenLessBackend, QaSnapshot,
    RemoteInputStatus, SessionId,
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
struct CapsuleOwners {
    transcription_notice: Option<SessionId>,
    // Presentation ownership only: Recording/level proves this turn used audio.
    // Keep a known owner across resync; a Thinking snapshot alone cannot tell a
    // voice turn from a text turn if its initial Recording event was dropped.
    qa_voice: Option<SessionId>,
    // Keep the last successful native epoch after rejection: a late CPU notice
    // or resync for that same turn must not reclaim a replaced display.
    qa_capsule: Option<(SessionId, u64)>,
}

pub fn start(app: AppHandle, backend: Arc<OpenLessBackend>) {
    let mut events = backend.subscribe();
    let backend_for_events = Arc::clone(&backend);
    tauri::async_runtime::spawn(async move {
        let mut capsule_owners = CapsuleOwners::default();
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app.emit("backend:event", &event);
                    forward_legacy_event(
                        &app,
                        &backend_for_events,
                        event.session_id,
                        event.kind,
                        &mut capsule_owners,
                    )
                    .await;
                }
                Err(EventRecvError::Lagged(dropped)) => {
                    log::warn!(
                        "[core-events] Tauri bridge lagged by {dropped} event(s); resyncing snapshots"
                    );
                    capsule_owners.transcription_notice = None;
                    emit_resync(&app, &backend_for_events, &mut capsule_owners).await;
                }
                Err(EventRecvError::Closed) => break,
                Err(EventRecvError::Empty) => unreachable!("async receive never returns Empty"),
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        if let Err(error) = backend.start().await {
            log::error!("[core-events] backend start failed: {error}");
            return;
        }
        let preferences = backend.get_preferences();
        if let Err(error) = backend
            .services()
            .remote_input
            .configure(openless_core::RemoteInputConfig {
                enabled: preferences.remote_input_enabled,
                port: preferences.remote_input_port,
            })
            .await
        {
            if error.code != openless_core::BackendErrorCode::Unsupported {
                log::error!("[core-events] remote input startup failed: {error}");
            }
        }
    });
}

/// Publish a typed semantic event through the backend instance managed by the
/// Tauri host. Platform adapters use this instead of creating a second,
/// host-only event stream.
pub(crate) fn publish<R: tauri::Runtime>(
    app: &AppHandle<R>,
    session_id: Option<SessionId>,
    kind: BackendEventKind,
) {
    let Some(backend) = app.try_state::<Arc<OpenLessBackend>>() else {
        log::warn!("[core-events] backend state unavailable while publishing adapter event");
        return;
    };
    backend.event_publisher().publish(session_id, kind);
}

async fn forward_legacy_event(
    app: &AppHandle,
    backend: &OpenLessBackend,
    session_id: Option<SessionId>,
    kind: BackendEventKind,
    capsule_owners: &mut CapsuleOwners,
) {
    if matches!(
        kind,
        BackendEventKind::QaLevel(_) | BackendEventKind::QaState(_)
    ) {
        let selection = backend.services().selection.snapshot().await.ok();
        let selection_voice = backend.services().selection_voice.snapshot().await.ok();
        // Read QA after the other async snapshots. Queued levels/terminals from
        // an old turn must not be presented over its successor or another voice.
        let qa = backend.services().qa.snapshot().await.ok();
        let other_voice_active = qa_capsule_blocked(
            backend.snapshot().dictation.phase,
            backend
                .less_computer_active_session()
                .is_some_and(|session_id| !backend.less_computer_capture_cancelled(session_id)),
            selection.map(|snapshot| snapshot.phase),
            selection_voice.map(|snapshot| snapshot.phase),
        );
        let previous_voice = capsule_owners.qa_voice;
        if let Some(payload) = qa_capsule_payload(
            capsule_owners,
            session_id,
            &kind,
            qa.as_ref(),
            other_voice_active,
            backend.get_preferences().capsule_style,
        ) {
            if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
                if let Some(owner) = capsule_owners.qa_voice.or(previous_voice) {
                    present_qa_capsule(capsule_owners, owner, payload, |payload, expected| {
                        coordinator.present_core_capsule_if_current(payload, expected)
                    });
                }
            }
        }
    }
    match kind {
        BackendEventKind::PreferencesChanged(_) => emit_preferences(app, backend),
        BackendEventKind::CredentialsChanged(status) => {
            let _ = app.emit("credentials:changed", status);
        }
        BackendEventKind::VocabularyChanged(_) => {
            // Legacy listeners use this only as an invalidation signal.  Do not
            // send the core revision as the old hit-count payload.
            let _ = app.emit("vocab:updated", ());
        }
        BackendEventKind::DictationStateChanged(snapshot) => {
            capsule_owners.transcription_notice = None;
            if snapshot.phase != DictationPhase::Idle {
                capsule_owners.qa_voice = None;
                capsule_owners.qa_capsule = None;
            }
            emit_dictation_state(app, backend, snapshot)
        }
        BackendEventKind::TranscriptDelta(_) => {}
        BackendEventKind::DictationCompleted(result) => {
            capsule_owners.transcription_notice = None;
            capsule_owners.qa_voice = None;
            capsule_owners.qa_capsule = None;
            if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
                let message = match &result.inserted {
                    openless_core::DictationInsertStatus::Inserted => "已输入",
                    openless_core::DictationInsertStatus::PasteSent => "已发送粘贴，请确认",
                    openless_core::DictationInsertStatus::CopiedFallback => "已复制，请手动粘贴",
                    openless_core::DictationInsertStatus::NotRequested => "处理完成",
                };
                coordinator.present_core_capsule(CapsulePayload {
                    state: CapsuleState::Done,
                    level: 0.0,
                    elapsed_ms: result.duration_ms,
                    message: Some(message.to_string()),
                    inserted_chars: Some(
                        u32::try_from(result.polished_text.chars().count()).unwrap_or(u32::MAX),
                    ),
                    translation: false,
                    operating: false,
                    warming: false,
                    capsule_style: backend.get_preferences().capsule_style,
                    selection_polish: false,
                });
            }
        }
        BackendEventKind::RecordingControlRequested(request) => {
            let Some(backend) = app
                .try_state::<Arc<OpenLessBackend>>()
                .map(|backend| Arc::clone(&*backend))
            else {
                return;
            };
            tauri::async_runtime::spawn(async move {
                let result = match request.action {
                    openless_core::RecordingControlAction::Stop => backend
                        .stop_dictation_session(request.session_id)
                        .await
                        .map(|_| ()),
                    openless_core::RecordingControlAction::Cancel => {
                        backend.cancel_dictation(Some(request.session_id)).await
                    }
                };
                if let Err(error) = result {
                    if error.code != openless_core::BackendErrorCode::InvalidState {
                        log::warn!("[recording] automatic terminal action failed: {error}");
                    }
                }
            });
        }
        BackendEventKind::InsertFallback(fallback) => {
            if let Some(text) = fallback.copied_text {
                if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
                    coordinator.show_core_insert_fallback(text, &fallback.reason);
                }
            }
        }
        BackendEventKind::CodingAgentTest(event) => {
            let _ = app.emit("coding-agent:test", event);
        }
        BackendEventKind::LessComputerEvent(event) => {
            if let openless_core::LessComputerEventKind::VoiceState {
                session_id,
                phase,
                level,
                elapsed_ms,
            } = &event.kind
            {
                // 胶囊只展示Core语音快照。已开始的其它会话拥有共享窗口，旧Less终态不得盖掉它。
                let current = backend.less_computer_active_session();
                if !current.is_some_and(|current| current != *session_id)
                    && backend.snapshot().dictation.phase == DictationPhase::Idle
                {
                    if let Some(coordinator) =
                        app.try_state::<Arc<crate::coordinator::Coordinator>>()
                    {
                        capsule_owners.transcription_notice = None;
                        use openless_core::LessComputerVoicePhase;
                        if *phase != LessComputerVoicePhase::Idle {
                            capsule_owners.qa_voice = None;
                            capsule_owners.qa_capsule = None;
                        }
                        let state = match phase {
                            LessComputerVoicePhase::Starting
                            | LessComputerVoicePhase::Recording => CapsuleState::Recording,
                            LessComputerVoicePhase::Transcribing => CapsuleState::Transcribing,
                            LessComputerVoicePhase::Idle => CapsuleState::Idle,
                        };
                        coordinator.present_core_capsule(CapsulePayload {
                            state,
                            level: *level,
                            elapsed_ms: *elapsed_ms,
                            message: (*phase == LessComputerVoicePhase::Starting)
                                .then(|| "正在准备语音…".to_string()),
                            inserted_chars: None,
                            translation: false,
                            operating: true,
                            warming: *phase == LessComputerVoicePhase::Starting,
                            capsule_style: backend.get_preferences().capsule_style,
                            selection_polish: false,
                        });
                    }
                }
            }
            let _ = app.emit_to("less-computer", "less-computer:event", event);
        }
        BackendEventKind::LocalAsrPrepareProgress(progress) => {
            let event_name = match progress.runtime {
                LocalAsrRuntimeKind::Foundry => "foundry-local-asr-prepare-progress",
                LocalAsrRuntimeKind::SherpaOnnx => "sherpa-onnx-asr-prepare-progress",
                LocalAsrRuntimeKind::Generic => "local-asr-prepare-progress",
            };
            let payload = serde_json::json!({
                "phase": progress.phase,
                "modelAlias": progress.model_alias,
                "label": progress.label,
                "percent": progress.percent,
                "error": progress.error,
            });
            let _ = app.emit(event_name, payload);
        }
        BackendEventKind::LocalAsrDownloadProgress(progress) => {
            let event_name = match progress.runtime {
                LocalAsrRuntimeKind::SherpaOnnx => "sherpa-onnx-asr-download-progress",
                LocalAsrRuntimeKind::Foundry | LocalAsrRuntimeKind::Generic => {
                    "local-asr-download-progress"
                }
            };
            let payload = serde_json::json!({
                "modelId": progress.model_id,
                "file": progress.file,
                "fileIndex": progress.file_index,
                "fileCount": progress.file_count,
                "bytesDownloaded": progress.bytes_downloaded,
                "bytesTotal": progress.bytes_total,
                "phase": progress.phase,
                "error": progress.error,
            });
            let _ = app.emit(event_name, payload);
        }
        BackendEventKind::LocalAsrEngineChanged(status) => {
            let _ = app.emit("local-asr:engine-changed", status);
        }
        BackendEventKind::MicrophoneDevicesChanged => {
            let _ = app.emit("microphone:devices-changed", serde_json::json!({}));
        }
        BackendEventKind::QaLevel(level) => {
            let _ = app.emit_to(
                crate::coordinator::qa_event_target(),
                "qa:level",
                serde_json::json!({ "level": level.level }),
            );
        }
        BackendEventKind::QaState(state) => {
            let _ = app.emit_to(crate::coordinator::qa_event_target(), "qa:state", state);
        }
        BackendEventKind::Notification(notice) => {
            let qa = backend.services().qa.snapshot().await.ok();
            let less_voice = backend
                .event_publisher()
                .latest_less_computer_voice_state()
                .filter(|event| {
                    matches!(&event.kind,
                    openless_core::LessComputerEventKind::VoiceState { session_id, .. }
                        if backend.less_computer_active_session() == Some(*session_id))
                });
            // Recheck the current owner after awaiting QA. A queued callback
            // from a cancelled native operation must not replace a new capsule.
            if let Some(payload) = transcription_notice_payload(
                session_id,
                &notice.message,
                &backend.snapshot().dictation,
                qa.as_ref(),
                less_voice.as_ref(),
                backend.get_preferences().capsule_style,
            ) {
                if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
                    if let Some(owner) = session_id.filter(|id| {
                        qa.as_ref()
                            .is_some_and(|snapshot| snapshot.session_id == Some(*id))
                    }) {
                        // A live native-ASR notice also proves audio ownership
                        // when the initial Recording event was lost to lag.
                        if present_qa_capsule(
                            capsule_owners,
                            owner,
                            payload,
                            |payload, expected| {
                                coordinator.present_core_capsule_if_current(payload, expected)
                            },
                        ) {
                            capsule_owners.qa_voice = Some(owner);
                            capsule_owners.transcription_notice = Some(owner);
                        }
                    } else {
                        coordinator.present_core_capsule(payload);
                        capsule_owners.transcription_notice = session_id;
                    }
                }
            }
        }
        BackendEventKind::BackendStopping => {
            let notice_visible = capsule_owners.transcription_notice.take().is_some();
            let qa_visible = capsule_owners.qa_voice.take().is_some();
            if notice_visible || qa_visible {
                emit_dictation_state(app, backend, DictationStateSnapshot::default());
            }
        }
        BackendEventKind::RemoteInputStatusChanged(status) => {
            let _ = app.emit("remote-input:running", status);
        }
        BackendEventKind::RemoteInputFailed(error) => {
            let _ = app.emit("remote-input:error", error);
        }
        BackendEventKind::VocabularySuggestionsChanged(suggestions) => {
            if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
                coordinator.refresh_vocab_suggestion_presentation(!suggestions.is_empty());
            }
            let _ = app.emit_to("capsule", "vocab:suggested", suggestions);
        }
        // These domains either have no legacy push event or require a
        // window-specific payload that remains owned by the compatibility host.
        BackendEventKind::BackendStarted
        | BackendEventKind::SelectionStateChanged(_)
        | BackendEventKind::SelectionVoiceStateChanged(_)
        | BackendEventKind::PolishDelta(_)
        | BackendEventKind::HistoryChanged(_)
        | BackendEventKind::StylePacksChanged(_)
        | BackendEventKind::DownloadProgress(_)
        | BackendEventKind::PermissionChanged(_)
        | BackendEventKind::HotkeyStatusChanged(_) => {}
    }
}

fn emit_dictation_state(
    app: &AppHandle,
    backend: &OpenLessBackend,
    snapshot: DictationStateSnapshot,
) {
    if snapshot.phase == DictationPhase::Completed {
        return;
    }
    let payload = map_dictation_state(snapshot, backend.get_preferences().capsule_style);
    if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
        coordinator.present_core_capsule(payload);
        return;
    }
    if let Some(capsule) = app.get_webview_window("capsule") {
        let _ = capsule.emit("capsule:state", &payload);
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("capsule:state", &payload);
    }
    #[cfg(target_os = "android")]
    crate::android::notify_capsule_state(&payload);
}

fn map_dictation_state(
    snapshot: DictationStateSnapshot,
    capsule_style: CapsuleStyle,
) -> CapsulePayload {
    let state = match snapshot.phase {
        DictationPhase::Idle => CapsuleState::Idle,
        DictationPhase::Starting | DictationPhase::Recording => CapsuleState::Recording,
        DictationPhase::Transcribing => CapsuleState::Transcribing,
        DictationPhase::Polishing | DictationPhase::Inserting => CapsuleState::Polishing,
        DictationPhase::Completed => CapsuleState::Done,
        DictationPhase::Cancelled => CapsuleState::Cancelled,
        DictationPhase::Failed => CapsuleState::Error,
    };
    let message = match snapshot.phase {
        DictationPhase::Failed => Some(match snapshot.message.as_deref() {
            Some("PermissionDenied") => "请允许麦克风权限后重试".to_string(),
            Some("Busy") => "另一个语音任务正在进行".to_string(),
            Some("Cancelled") => "已取消".to_string(),
            Some("Provider") | Some("Network") | Some("Timeout") => {
                "识别或润色失败，请重试".to_string()
            }
            Some("Platform") => "录音或输入失败，请重试".to_string(),
            Some("InvalidArgument") => "输入或设置无效，请检查后重试".to_string(),
            Some("InvalidState") => "当前状态无法完成此操作，请重试".to_string(),
            Some("Unsupported") => "当前配置或平台不支持此操作".to_string(),
            Some("Persistence") => "保存结果失败，请检查磁盘后重试".to_string(),
            Some("OutcomeUnknown") => "无法确认是否已输入，请检查目标应用".to_string(),
            Some("Internal") => "处理遇到内部错误，请重试".to_string(),
            Some(message) if !message.trim().is_empty() => message.to_string(),
            _ => "处理失败，请重试".to_string(),
        }),
        DictationPhase::Cancelled => Some("已取消".to_string()),
        _ => snapshot.message,
    };
    CapsulePayload {
        state,
        level: snapshot.level,
        elapsed_ms: snapshot.elapsed_ms,
        message,
        inserted_chars: None,
        translation: snapshot.translation_active,
        operating: false,
        warming: matches!(
            snapshot.phase,
            DictationPhase::Starting | DictationPhase::Recording
        ) && !snapshot.recording_ready,
        capsule_style,
        selection_polish: false,
    }
}

fn emit_preferences(app: &AppHandle, backend: &OpenLessBackend) {
    let preferences = backend.get_preferences();
    let _ = app.emit("prefs:changed", &preferences);
}

fn transcription_notice_payload(
    session_id: Option<SessionId>,
    message: &str,
    dictation: &DictationStateSnapshot,
    qa: Option<&QaSnapshot>,
    less_voice: Option<&openless_core::LessComputerEvent>,
    style: CapsuleStyle,
) -> Option<CapsulePayload> {
    let session_id = session_id?;
    if message.trim().is_empty() {
        return None;
    }
    let (elapsed_ms, operating) = if dictation.phase != DictationPhase::Idle {
        if dictation.session_id != Some(session_id)
            || dictation.phase != DictationPhase::Transcribing
        {
            return None;
        }
        (dictation.elapsed_ms, false)
    } else if let Some(openless_core::LessComputerEvent {
        kind:
            openless_core::LessComputerEventKind::VoiceState {
                session_id: owner,
                phase,
                elapsed_ms,
                ..
            },
        ..
    }) = less_voice
    {
        if *owner != session_id || *phase != openless_core::LessComputerVoicePhase::Transcribing {
            return None;
        }
        (*elapsed_ms, true)
    } else {
        let qa = qa?;
        if qa.session_id != Some(session_id) || qa.phase != openless_core::QaPhase::Thinking {
            return None;
        }
        (0, false)
    };
    Some(CapsulePayload {
        state: CapsuleState::Transcribing,
        level: 0.0,
        elapsed_ms,
        message: Some(message.to_string()),
        inserted_chars: None,
        translation: dictation.translation_active,
        operating,
        warming: false,
        capsule_style: style,
        selection_polish: false,
    })
}

fn finish_qa_transcription_notice(
    owner: &mut Option<SessionId>,
    session_id: Option<SessionId>,
    kind: openless_core::QaStateKind,
) -> bool {
    if owner.is_some() && *owner == session_id && kind != openless_core::QaStateKind::Loading {
        *owner = None;
        true
    } else {
        false
    }
}

fn qa_capsule_payload(
    owners: &mut CapsuleOwners,
    session_id: Option<SessionId>,
    event: &BackendEventKind,
    qa: Option<&QaSnapshot>,
    other_voice_active: bool,
    style: CapsuleStyle,
) -> Option<CapsulePayload> {
    use openless_core::{QaPhase, QaStateKind};

    if other_voice_active {
        // Once another domain owns the capsule, a later QA terminal must not
        // regain its old display merely because that domain has just finished.
        let previous = owners.qa_voice.take();
        if owners.transcription_notice == previous {
            owners.transcription_notice = None;
        }
        return None;
    }
    let qa = qa?;
    let (kind, level, error) = match event {
        BackendEventKind::QaState(state) => (state.kind, 0.0, state.error.as_ref()),
        BackendEventKind::QaLevel(level) => (QaStateKind::Recording, level.level, None),
        _ => return None,
    };
    let terminal = matches!(
        kind,
        QaStateKind::Idle | QaStateKind::Answer | QaStateKind::Cancelled | QaStateKind::Error
    );
    if session_id != qa.session_id && !(terminal && qa.session_id.is_none()) {
        return None;
    }
    if kind == QaStateKind::Recording
        && qa.phase == QaPhase::Recording
        && session_id.is_some()
        && session_id == qa.session_id
    {
        owners.qa_voice = session_id;
    }
    let owns_voice = owners.qa_voice.is_some()
        && (owners.qa_voice == session_id
            || (kind == QaStateKind::Idle && session_id.is_none() && qa.session_id.is_none()));
    let state = if owns_voice {
        match kind {
            QaStateKind::Recording if qa.phase == QaPhase::Recording => CapsuleState::Recording,
            QaStateKind::Loading if qa.phase == QaPhase::Thinking => CapsuleState::Transcribing,
            QaStateKind::Thinking | QaStateKind::AwaitingApproval
                if matches!(qa.phase, QaPhase::Thinking | QaPhase::AwaitingApproval) =>
            {
                CapsuleState::Polishing
            }
            QaStateKind::Error if qa.phase == QaPhase::Failed => CapsuleState::Error,
            QaStateKind::Answer if qa.phase == QaPhase::Completed || qa.session_id.is_none() => {
                CapsuleState::Idle
            }
            QaStateKind::Cancelled if qa.phase == QaPhase::Cancelled || qa.session_id.is_none() => {
                CapsuleState::Idle
            }
            QaStateKind::Idle if matches!(qa.phase, QaPhase::Idle | QaPhase::Completed) => {
                CapsuleState::Idle
            }
            _ => return None,
        }
    } else if owners.qa_voice.is_some() && session_id.is_some() && session_id == qa.session_id {
        // A following text turn owns no voice feedback. Retire only the old QA
        // display; do not show its Loading/Thinking events as a new recording.
        CapsuleState::Idle
    } else {
        return None;
    };
    if matches!(state, CapsuleState::Idle | CapsuleState::Error) {
        let previous = owners.qa_voice.take();
        if owners.transcription_notice == previous {
            owners.transcription_notice = None;
        }
    } else {
        finish_qa_transcription_notice(&mut owners.transcription_notice, session_id, kind);
    }
    Some(CapsulePayload {
        state,
        level: if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        },
        elapsed_ms: 0,
        message: (state == CapsuleState::Error).then(|| {
            error
                .or(qa.last_error.as_ref())
                .filter(|message| !message.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| "QA 处理失败，请重试".to_string())
        }),
        inserted_chars: None,
        translation: false,
        operating: false,
        // Core announces Recording before native startup completes; only a
        // real PCM level proves the microphone is ready, including level zero.
        warming: state == CapsuleState::Recording && !matches!(event, BackendEventKind::QaLevel(_)),
        capsule_style: style,
        selection_polish: false,
    })
}

fn qa_capsule_blocked(
    dictation: DictationPhase,
    less_voice_active: bool,
    selection: Option<openless_core::SelectionPhase>,
    selection_voice: Option<openless_core::SelectionVoicePhase>,
) -> bool {
    dictation != DictationPhase::Idle
        || less_voice_active
        || matches!(
            selection,
            Some(
                openless_core::SelectionPhase::Capturing | openless_core::SelectionPhase::Applying
            )
        )
        || matches!(
            selection_voice,
            Some(
                openless_core::SelectionVoicePhase::Recording
                    | openless_core::SelectionVoicePhase::Processing
                    | openless_core::SelectionVoicePhase::AwaitingIntent
                    | openless_core::SelectionVoicePhase::Applying
            )
        )
}

fn present_qa_capsule(
    owners: &mut CapsuleOwners,
    session_id: SessionId,
    payload: CapsulePayload,
    present: impl FnOnce(CapsulePayload, Option<u64>) -> Option<u64>,
) -> bool {
    let expected = owners
        .qa_capsule
        .filter(|(owner, _)| *owner == session_id)
        .map(|(_, epoch)| epoch);
    if let Some(epoch) = present(payload, expected) {
        owners.qa_capsule = Some((session_id, epoch));
        true
    } else {
        if owners.qa_voice == Some(session_id) {
            owners.qa_voice = None;
        }
        if owners.transcription_notice == Some(session_id) {
            owners.transcription_notice = None;
        }
        false
    }
}

async fn emit_resync(app: &AppHandle, backend: &OpenLessBackend, owners: &mut CapsuleOwners) {
    emit_preferences(app, backend);
    let snapshot = backend.snapshot();
    if let Some((owner, _)) = owners
        .qa_capsule
        .filter(|_| snapshot.dictation.phase == DictationPhase::Idle)
    {
        if let Some(coordinator) = app.try_state::<Arc<crate::coordinator::Coordinator>>() {
            let payload = map_dictation_state(
                snapshot.dictation.clone(),
                backend.get_preferences().capsule_style,
            );
            present_qa_capsule(owners, owner, payload, |payload, expected| {
                coordinator.present_core_capsule_if_current(payload, expected)
            });
        }
    } else {
        emit_dictation_state(app, backend, snapshot.dictation.clone());
    }
    let _ = app.emit("credentials:changed", snapshot.credentials);
    let _ = app.emit("vocab:updated", ());

    let qa = match backend.services().qa.snapshot().await {
        Ok(snapshot) => Some(snapshot),
        Err(error) if error.code == openless_core::BackendErrorCode::Unsupported => None,
        Err(error) => {
            log::warn!("[core-events] QA resync failed: {error}");
            None
        }
    };
    let remote_input = match backend.services().remote_input.status() {
        Ok(status) => Some(status),
        Err(error) if error.code == openless_core::BackendErrorCode::Unsupported => None,
        Err(error) => {
            log::warn!("[core-events] remote input resync failed: {error}");
            None
        }
    };
    let qa_session_id = qa.as_ref().and_then(|snapshot| snapshot.session_id);
    for kind in resync_domain_events(qa, remote_input) {
        let session_id = matches!(kind, BackendEventKind::QaState(_))
            .then_some(qa_session_id)
            .flatten();
        forward_legacy_event(app, backend, session_id, kind, owners).await;
    }
}

fn resync_domain_events(
    qa: Option<QaSnapshot>,
    remote_input: Option<RemoteInputStatus>,
) -> Vec<BackendEventKind> {
    let mut events = Vec::with_capacity(2);
    if let Some(snapshot) = qa {
        events.push(BackendEventKind::QaState(
            openless_core::QaStateEvent::from_snapshot(&snapshot),
        ));
    }
    if let Some(status) = remote_input {
        events.push(BackendEventKind::RemoteInputStatusChanged(
            openless_core::RemoteInputRuntimeEvent::from(&status),
        ));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_native_epoch_is_not_reacquired_by_notice_or_resync_after_rejection() {
        let session_id = SessionId::new();
        let mut owners = CapsuleOwners {
            qa_voice: Some(session_id),
            ..Default::default()
        };
        let payload =
            || map_dictation_state(DictationStateSnapshot::default(), CapsuleStyle::Classic);
        assert!(present_qa_capsule(
            &mut owners,
            session_id,
            payload(),
            |_, expected| {
                assert_eq!(expected, None);
                Some(11)
            }
        ));
        assert_eq!(owners.qa_capsule, Some((session_id, 11)));
        for _ in 0..3 {
            // The first call is the delayed QA state; later calls use the same
            // production exit for native notices and snapshot replay.
            assert!(!present_qa_capsule(
                &mut owners,
                session_id,
                payload(),
                |_, expected| {
                    assert_eq!(expected, Some(11));
                    None
                }
            ));
            assert_eq!(owners.qa_voice, None);
            assert_eq!(owners.qa_capsule, Some((session_id, 11)));
        }
        let next = SessionId::new();
        assert!(present_qa_capsule(
            &mut owners,
            next,
            payload(),
            |_, expected| {
                assert_eq!(expected, None);
                Some(13)
            }
        ));
        assert_eq!(owners.qa_capsule, Some((next, 13)));
    }

    #[test]
    fn qa_voice_events_restore_the_capsule_lifecycle() {
        use openless_core::{QaPhase, QaStateKind};
        let session_id = SessionId::new();
        let mut qa = QaSnapshot {
            session_id: Some(session_id),
            phase: QaPhase::Recording,
            ..Default::default()
        };
        let mut owners = CapsuleOwners::default();
        let payload = qa_capsule_payload(
            &mut owners,
            Some(session_id),
            &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(&qa)),
            Some(&qa),
            false,
            CapsuleStyle::Classic,
        )
        .expect("ordinary QA recording must show the existing capsule");
        assert_eq!(payload.state, CapsuleState::Recording);
        assert!(payload.warming);
        assert_eq!(owners.qa_voice, Some(session_id));
        for expected_level in [0.0, 0.35] {
            let level = qa_capsule_payload(
                &mut owners,
                Some(session_id),
                &BackendEventKind::QaLevel(openless_core::QaRecordingLevel {
                    session_id: session_id.to_string(),
                    level: expected_level,
                }),
                Some(&qa),
                false,
                CapsuleStyle::Classic,
            )
            .unwrap();
            assert_eq!(level.state, CapsuleState::Recording);
            assert_eq!(level.level, expected_level);
            assert!(!level.warming && !level.translation && !level.operating);
        }

        for (phase, kind, expected) in [
            (
                QaPhase::Thinking,
                QaStateKind::Loading,
                CapsuleState::Transcribing,
            ),
            (
                QaPhase::Thinking,
                QaStateKind::Thinking,
                CapsuleState::Polishing,
            ),
            (QaPhase::Completed, QaStateKind::Answer, CapsuleState::Idle),
            (
                QaPhase::Recording,
                QaStateKind::Recording,
                CapsuleState::Recording,
            ),
            (QaPhase::Failed, QaStateKind::Error, CapsuleState::Error),
            (
                QaPhase::Recording,
                QaStateKind::Recording,
                CapsuleState::Recording,
            ),
            (
                QaPhase::Cancelled,
                QaStateKind::Cancelled,
                CapsuleState::Idle,
            ),
        ] {
            qa.phase = phase;
            let mut event = openless_core::QaStateEvent::from_snapshot(&qa);
            event.kind = kind;
            event.error = (kind == QaStateKind::Error).then(|| "QA permission denied".into());
            if kind == QaStateKind::Thinking {
                owners.transcription_notice = Some(session_id);
            }
            let payload = qa_capsule_payload(
                &mut owners,
                Some(session_id),
                &BackendEventKind::QaState(event),
                Some(&qa),
                false,
                CapsuleStyle::Classic,
            )
            .unwrap_or_else(|| panic!("QA {kind:?} must retain its capsule feedback"));
            assert_eq!(payload.state, expected, "{kind:?}");
            if kind == QaStateKind::Thinking {
                assert_eq!(
                    owners.transcription_notice, None,
                    "replace the CPU notice with Polishing"
                );
            }
            if kind == QaStateKind::Error {
                assert_eq!(payload.message.as_deref(), Some("QA permission denied"));
            }
        }
        assert_eq!(owners.qa_voice, None);
    }

    #[test]
    fn qa_capsule_rejects_queued_events_after_another_owner_takes_over() {
        use openless_core::{QaPhase, QaStateKind};
        let old = SessionId::new();
        let current = SessionId::new();
        let qa = QaSnapshot {
            session_id: Some(current),
            phase: QaPhase::Recording,
            ..Default::default()
        };
        let mut owners = CapsuleOwners {
            qa_voice: Some(current),
            transcription_notice: Some(current),
            ..Default::default()
        };
        for kind in [
            QaStateKind::Recording,
            QaStateKind::Loading,
            QaStateKind::Thinking,
            QaStateKind::Answer,
            QaStateKind::Cancelled,
            QaStateKind::Error,
            QaStateKind::Idle,
        ] {
            assert!(
                qa_capsule_payload(
                    &mut owners,
                    Some(old),
                    &BackendEventKind::QaState(openless_core::QaStateEvent::simple(kind)),
                    Some(&qa),
                    false,
                    CapsuleStyle::Classic,
                )
                .is_none(),
                "old {kind:?} must not overwrite the new QA capsule"
            );
            assert_eq!(owners.qa_voice, Some(current));
            assert_eq!(owners.transcription_notice, Some(current));
        }
        assert!(qa_capsule_payload(
            &mut owners,
            Some(old),
            &BackendEventKind::QaLevel(openless_core::QaRecordingLevel {
                session_id: old.to_string(),
                level: 0.8,
            }),
            Some(&qa),
            false,
            CapsuleStyle::Classic,
        )
        .is_none());
        let mut completed = qa.clone();
        completed.phase = QaPhase::Completed;
        for blocked in [
            qa_capsule_blocked(DictationPhase::Starting, false, None, None),
            qa_capsule_blocked(DictationPhase::Idle, true, None, None),
            qa_capsule_blocked(
                DictationPhase::Idle,
                false,
                Some(openless_core::SelectionPhase::Capturing),
                None,
            ),
            qa_capsule_blocked(
                DictationPhase::Idle,
                false,
                None,
                Some(openless_core::SelectionVoicePhase::Recording),
            ),
        ] {
            assert!(blocked);
            owners.qa_voice = Some(current);
            assert!(
                qa_capsule_payload(
                    &mut owners,
                    Some(current),
                    &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(
                        &completed
                    )),
                    Some(&completed),
                    blocked,
                    CapsuleStyle::Classic,
                )
                .is_none(),
                "an old QA terminal must not hide the other voice capsule"
            );
            assert_eq!(owners.qa_voice, None);
            assert!(
                qa_capsule_payload(
                    &mut owners,
                    Some(current),
                    &BackendEventKind::QaLevel(openless_core::QaRecordingLevel {
                        session_id: current.to_string(),
                        level: 0.8,
                    }),
                    Some(&completed),
                    false,
                    CapsuleStyle::Classic,
                )
                .is_none(),
                "a late level from completed QA cannot reclaim retired feedback"
            );
            assert_eq!(owners.qa_voice, None);
            assert!(
                qa_capsule_payload(
                    &mut owners,
                    Some(current),
                    &BackendEventKind::QaLevel(openless_core::QaRecordingLevel {
                        session_id: current.to_string(),
                        level: 0.8,
                    }),
                    Some(&qa),
                    blocked,
                    CapsuleStyle::Classic,
                )
                .is_none(),
                "a queued QA level must not replace another voice capsule"
            );
            assert_eq!(owners.qa_voice, None);
            assert!(
                qa_capsule_payload(
                    &mut owners,
                    Some(current),
                    &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(
                        &completed
                    )),
                    Some(&completed),
                    false,
                    CapsuleStyle::Classic,
                )
                .is_none(),
                "a late terminal cannot regain ownership after the other voice finishes"
            );
        }
    }

    #[test]
    fn qa_capsule_keeps_text_turns_quiet_and_recovers_only_known_voice_from_resync() {
        use openless_core::{QaPhase, QaStateKind};
        let session_id = SessionId::new();
        let mut qa = QaSnapshot {
            session_id: Some(session_id),
            phase: QaPhase::Thinking,
            ..Default::default()
        };
        let mut owners = CapsuleOwners::default();
        for phase in [
            QaPhase::Thinking,
            QaPhase::Completed,
            QaPhase::Failed,
            QaPhase::Cancelled,
        ] {
            qa.phase = phase;
            assert!(
                qa_capsule_payload(
                    &mut owners,
                    Some(session_id),
                    &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(&qa)),
                    Some(&qa),
                    false,
                    CapsuleStyle::Classic,
                )
                .is_none(),
                "text-only or unknown {phase:?} is not a voice recording"
            );
        }
        owners.qa_voice = Some(SessionId::new());
        qa.phase = QaPhase::Thinking;
        let idle = qa_capsule_payload(
            &mut owners,
            Some(session_id),
            &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(&qa)),
            Some(&qa),
            false,
            CapsuleStyle::Classic,
        )
        .expect("a following text turn retires the previous voice feedback");
        assert_eq!(idle.state, CapsuleState::Idle);
        assert_eq!(owners.qa_voice, None);

        qa.phase = QaPhase::Recording;
        assert_eq!(
            qa_capsule_payload(
                &mut owners,
                Some(session_id),
                &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(&qa)),
                Some(&qa),
                false,
                CapsuleStyle::Classic,
            )
            .unwrap()
            .state,
            CapsuleState::Recording
        );
        qa.phase = QaPhase::Thinking;
        assert_eq!(
            qa_capsule_payload(
                &mut owners,
                Some(session_id),
                &BackendEventKind::QaState(openless_core::QaStateEvent::from_snapshot(&qa)),
                Some(&qa),
                false,
                CapsuleStyle::Classic,
            )
            .unwrap()
            .state,
            CapsuleState::Polishing
        );

        let closed = QaSnapshot::default();
        assert_eq!(
            qa_capsule_payload(
                &mut owners,
                None,
                &BackendEventKind::QaState(openless_core::QaStateEvent::simple(QaStateKind::Idle)),
                Some(&closed),
                false,
                CapsuleStyle::Classic,
            )
            .unwrap()
            .state,
            CapsuleState::Idle
        );
        assert_eq!(owners.qa_voice, None);
        assert!(!qa_capsule_blocked(
            DictationPhase::Idle,
            false,
            None,
            Some(openless_core::SelectionVoicePhase::Preview)
        ));
    }

    #[test]
    fn transcription_notice_projects_only_its_live_owner() {
        let id = SessionId::new();
        let stale = SessionId::new();
        let mut dictation = DictationStateSnapshot {
            session_id: Some(id),
            phase: DictationPhase::Transcribing,
            elapsed_ms: 123,
            ..Default::default()
        };
        let payload = transcription_notice_payload(
            Some(id),
            "正在下载 CPU 模型，首次使用可能较慢…",
            &dictation,
            None,
            None,
            CapsuleStyle::Classic,
        )
        .expect("active Foundry notice must be displayed in the capsule");
        assert_eq!(payload.state, CapsuleState::Transcribing);
        assert_eq!(
            payload.message.as_deref(),
            Some("正在下载 CPU 模型，首次使用可能较慢…")
        );
        assert_eq!(payload.elapsed_ms, 123);
        assert!(!payload.operating);
        for session_id in [None, Some(stale)] {
            assert!(transcription_notice_payload(
                session_id,
                "old",
                &dictation,
                None,
                None,
                CapsuleStyle::Classic
            )
            .is_none());
        }
        for phase in [
            DictationPhase::Starting,
            DictationPhase::Recording,
            DictationPhase::Polishing,
            DictationPhase::Completed,
            DictationPhase::Cancelled,
            DictationPhase::Failed,
            DictationPhase::Idle,
        ] {
            dictation.phase = phase;
            assert!(
                transcription_notice_payload(
                    Some(id),
                    "old",
                    &dictation,
                    None,
                    None,
                    CapsuleStyle::Classic
                )
                .is_none(),
                "{phase:?}"
            );
        }
        let mut qa = QaSnapshot {
            phase: openless_core::QaPhase::Thinking,
            session_id: Some(id),
            ..Default::default()
        };
        assert!(transcription_notice_payload(
            Some(id),
            "QA notice",
            &dictation,
            Some(&qa),
            None,
            CapsuleStyle::Classic
        )
        .is_some());
        for phase in [
            openless_core::QaPhase::Cancelled,
            openless_core::QaPhase::Completed,
            openless_core::QaPhase::Failed,
            openless_core::QaPhase::Idle,
        ] {
            qa.phase = phase;
            assert!(transcription_notice_payload(
                Some(id),
                "old",
                &dictation,
                Some(&qa),
                None,
                CapsuleStyle::Classic
            )
            .is_none());
        }
        let voice = openless_core::LessComputerEvent {
            seq: Some(1),
            kind: openless_core::LessComputerEventKind::VoiceState {
                session_id: id,
                phase: openless_core::LessComputerVoicePhase::Transcribing,
                level: 0.0,
                elapsed_ms: 456,
            },
        };
        let payload = transcription_notice_payload(
            Some(id),
            "Less notice",
            &dictation,
            None,
            Some(&voice),
            CapsuleStyle::Classic,
        )
        .unwrap();
        assert!(payload.operating);
        assert_eq!(payload.elapsed_ms, 456);
        dictation.phase = DictationPhase::Starting;
        dictation.session_id = Some(stale);
        assert!(transcription_notice_payload(
            Some(id),
            "old Less notice",
            &dictation,
            None,
            Some(&voice),
            CapsuleStyle::Classic
        )
        .is_none());
    }

    #[test]
    fn transcription_notice_is_cleared_when_qa_leaves_transcription() {
        use openless_core::QaStateKind;
        let id = SessionId::new();
        for kind in [
            QaStateKind::Thinking,
            QaStateKind::Answer,
            QaStateKind::Cancelled,
            QaStateKind::Error,
            QaStateKind::Idle,
        ] {
            let mut owner = Some(id);
            assert!(!finish_qa_transcription_notice(
                &mut owner,
                Some(SessionId::new()),
                kind
            ));
            assert!(!finish_qa_transcription_notice(
                &mut owner,
                Some(id),
                QaStateKind::Loading
            ));
            assert_eq!(owner, Some(id));
            assert!(
                finish_qa_transcription_notice(&mut owner, Some(id), kind),
                "{kind:?}"
            );
            assert_eq!(owner, None);
            assert!(!finish_qa_transcription_notice(&mut owner, Some(id), kind));
        }
    }

    #[test]
    fn core_dictation_state_maps_to_the_legacy_capsule_contract() {
        let cases = [
            (DictationPhase::Idle, CapsuleState::Idle, false),
            (DictationPhase::Starting, CapsuleState::Recording, true),
            (DictationPhase::Recording, CapsuleState::Recording, false),
            (
                DictationPhase::Transcribing,
                CapsuleState::Transcribing,
                false,
            ),
            (DictationPhase::Polishing, CapsuleState::Polishing, false),
            (DictationPhase::Inserting, CapsuleState::Polishing, false),
            (DictationPhase::Completed, CapsuleState::Done, false),
            (DictationPhase::Cancelled, CapsuleState::Cancelled, false),
            (DictationPhase::Failed, CapsuleState::Error, false),
        ];

        for (phase, expected_state, expected_warming) in cases {
            let payload = map_dictation_state(
                DictationStateSnapshot {
                    phase,
                    session_id: None,
                    elapsed_ms: 321,
                    level: 0.25,
                    message: Some("fixture".to_string()),
                    translation_active: true,
                    recording_ready: phase != DictationPhase::Starting,
                },
                CapsuleStyle::Classic,
            );
            assert_eq!(payload.state, expected_state);
            assert_eq!(payload.warming, expected_warming);
            assert!(payload.translation);
            assert_eq!(payload.capsule_style, CapsuleStyle::Classic);
            assert_eq!(payload.elapsed_ms, 321);
            assert_eq!(payload.level, 0.25);
            assert_eq!(
                payload.message.as_deref(),
                Some(if phase == DictationPhase::Cancelled {
                    "已取消"
                } else {
                    "fixture"
                })
            );
        }
    }

    #[test]
    fn internal_failure_tokens_are_not_shown_to_users() {
        for token in [
            "InvalidArgument",
            "InvalidState",
            "Busy",
            "Cancelled",
            "PermissionDenied",
            "Unsupported",
            "Provider",
            "Persistence",
            "Platform",
            "OutcomeUnknown",
            "Internal",
        ] {
            let payload = map_dictation_state(
                DictationStateSnapshot {
                    phase: DictationPhase::Failed,
                    message: Some(token.into()),
                    ..DictationStateSnapshot::default()
                },
                CapsuleStyle::Classic,
            );
            assert_ne!(payload.message.as_deref(), Some(token), "{token}");
        }
    }

    #[test]
    fn migration_event_names_are_owned_by_the_tauri_bridge() {
        use openless_core::{
            CodingAgentStreamEvent, LessComputerEvent, LessComputerEventKind, LocalAsrPreparePhase,
            LocalAsrPrepareProgress, QaStateEvent, QaStateKind, RemoteInputRuntimeEvent,
        };

        let cases = [
            (
                BackendEventKind::CodingAgentTest(CodingAgentStreamEvent::Started {
                    session_id: "coding".into(),
                }),
                "coding-agent:test",
            ),
            (
                BackendEventKind::LessComputerEvent(LessComputerEvent {
                    seq: Some(1),
                    kind: LessComputerEventKind::Started,
                }),
                "less-computer:event",
            ),
            (
                BackendEventKind::LocalAsrPrepareProgress(LocalAsrPrepareProgress {
                    runtime: LocalAsrRuntimeKind::Foundry,
                    phase: LocalAsrPreparePhase::Runtime,
                    model_alias: "fixture".into(),
                    label: "runtime".into(),
                    percent: None,
                    error: None,
                }),
                "foundry-local-asr-prepare-progress",
            ),
            (
                BackendEventKind::QaState(QaStateEvent::simple(QaStateKind::Idle)),
                "qa:state",
            ),
            (
                BackendEventKind::RemoteInputStatusChanged(RemoteInputRuntimeEvent {
                    running: false,
                    port: None,
                    urls: Vec::new(),
                }),
                "remote-input:running",
            ),
        ];

        for (kind, expected) in cases {
            assert_eq!(migration_legacy_event_name(&kind), Some(expected));
        }
    }

    #[test]
    fn lagged_resync_rebuilds_qa_and_remote_input_semantic_events() {
        use openless_core::{QaPhase, QaSnapshot, RemoteInputStatus};

        let events = resync_domain_events(
            Some(QaSnapshot {
                phase: QaPhase::Thinking,
                ..QaSnapshot::default()
            }),
            Some(RemoteInputStatus {
                enabled: true,
                running: true,
                starting: false,
                port: 9443,
                urls: vec!["https://192.168.1.2:9443".into()],
                urls_stale: false,
                locale: "zh-CN".into(),
                connection_count: 1,
                active_session_id: None,
            }),
        );

        assert!(matches!(
            &events[0],
            BackendEventKind::QaState(state)
                if state.kind == openless_core::QaStateKind::Thinking
        ));
        assert!(matches!(
            &events[1],
            BackendEventKind::RemoteInputStatusChanged(status)
                if status.running && status.port == Some(9443)
        ));
    }
}

#[cfg(test)]
fn migration_legacy_event_name(kind: &BackendEventKind) -> Option<&'static str> {
    match kind {
        BackendEventKind::CodingAgentTest(_) => Some("coding-agent:test"),
        BackendEventKind::LessComputerEvent(_) => Some("less-computer:event"),
        BackendEventKind::LocalAsrPrepareProgress(progress) => Some(match progress.runtime {
            LocalAsrRuntimeKind::Foundry => "foundry-local-asr-prepare-progress",
            LocalAsrRuntimeKind::SherpaOnnx => "sherpa-onnx-asr-prepare-progress",
            LocalAsrRuntimeKind::Generic => "local-asr-prepare-progress",
        }),
        BackendEventKind::LocalAsrDownloadProgress(progress) => Some(match progress.runtime {
            LocalAsrRuntimeKind::SherpaOnnx => "sherpa-onnx-asr-download-progress",
            LocalAsrRuntimeKind::Foundry | LocalAsrRuntimeKind::Generic => {
                "local-asr-download-progress"
            }
        }),
        BackendEventKind::LocalAsrEngineChanged(_) => Some("local-asr:engine-changed"),
        BackendEventKind::MicrophoneDevicesChanged => Some("microphone:devices-changed"),
        BackendEventKind::QaLevel(_) => Some("qa:level"),
        BackendEventKind::QaState(_) => Some("qa:state"),
        BackendEventKind::RemoteInputStatusChanged(_) => Some("remote-input:running"),
        BackendEventKind::RemoteInputFailed(_) => Some("remote-input:error"),
        BackendEventKind::VocabularySuggestionsChanged(_) => Some("vocab:suggested"),
        _ => None,
    }
}
