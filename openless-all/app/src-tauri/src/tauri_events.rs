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

pub fn start(app: AppHandle, backend: Arc<OpenLessBackend>) {
    let mut events = backend.subscribe();
    let backend_for_events = Arc::clone(&backend);
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app.emit("backend:event", &event);
                    forward_legacy_event(&app, &backend_for_events, event.kind);
                }
                Err(EventRecvError::Lagged(dropped)) => {
                    log::warn!(
                        "[core-events] Tauri bridge lagged by {dropped} event(s); resyncing snapshots"
                    );
                    emit_resync(&app, &backend_for_events).await;
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

fn forward_legacy_event(app: &AppHandle, backend: &OpenLessBackend, kind: BackendEventKind) {
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
            emit_dictation_state(app, backend, snapshot)
        }
        BackendEventKind::TranscriptDelta(_) => {}
        BackendEventKind::DictationCompleted(result) => {
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
                        use openless_core::LessComputerVoicePhase;
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
        | BackendEventKind::BackendStopping
        | BackendEventKind::SelectionStateChanged(_)
        | BackendEventKind::SelectionVoiceStateChanged(_)
        | BackendEventKind::PolishDelta(_)
        | BackendEventKind::HistoryChanged(_)
        | BackendEventKind::StylePacksChanged(_)
        | BackendEventKind::DownloadProgress(_)
        | BackendEventKind::PermissionChanged(_)
        | BackendEventKind::HotkeyStatusChanged(_)
        | BackendEventKind::Notification(_) => {}
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
        warming: snapshot.phase == DictationPhase::Starting,
        capsule_style,
        selection_polish: false,
    }
}

fn emit_preferences(app: &AppHandle, backend: &OpenLessBackend) {
    let preferences = backend.get_preferences();
    let _ = app.emit("prefs:changed", &preferences);
}

async fn emit_resync(app: &AppHandle, backend: &OpenLessBackend) {
    emit_preferences(app, backend);
    let snapshot = backend.snapshot();
    emit_dictation_state(app, backend, snapshot.dictation.clone());
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
    for kind in resync_domain_events(qa, remote_input) {
        forward_legacy_event(app, backend, kind);
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
