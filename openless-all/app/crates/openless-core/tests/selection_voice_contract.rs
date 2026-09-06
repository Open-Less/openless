use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use openless_core::testing::FixtureSelectionRuntime;
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, BackendEventKind,
    BackendRepositories, DictationContext, InsertOutcome, OpenLessBackend, PolishOutput,
    SelectionCapture, SelectionPolishOutputMode, SelectionVoiceApplyOutcome,
    SelectionVoiceEditAction, SelectionVoiceEditRequest, SelectionVoiceHotkeyAction,
    SelectionVoiceHotkeyEdge, SelectionVoiceInstructionRequest, SelectionVoiceIntent,
    SelectionVoiceIntentMode, SelectionVoiceManualIntent, SelectionVoicePhase,
    SelectionVoicePreviewUpdate, SelectionVoiceRoute, SelectionVoiceSnapshot, SessionId,
    TextPolisher, TextStreamSink, UserPreferences,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelCall {
    input: String,
    system_prompt: String,
    translation_active: bool,
    translation_target: String,
}

#[derive(Clone)]
struct ScriptedPolisher {
    responses: Arc<Mutex<VecDeque<Result<PolishOutput, BackendError>>>>,
    calls: Arc<Mutex<Vec<ModelCall>>>,
}

impl ScriptedPolisher {
    fn successful(responses: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(
                responses
                    .into_iter()
                    .map(|text| Ok(PolishOutput::text(text)))
                    .collect(),
            )),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<ModelCall> {
        self.calls.lock().expect("model call lock poisoned").clone()
    }
}

impl TextPolisher for ScriptedPolisher {
    fn polish(
        &self,
        _session_id: SessionId,
        context: Arc<DictationContext>,
        raw_text: String,
        _partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
        self.calls
            .lock()
            .expect("model call lock poisoned")
            .push(ModelCall {
                input: raw_text,
                system_prompt: context.polish.style_system_prompt.clone(),
                translation_active: context.polish.translation_active,
                translation_target: context.polish.translation_target_language.clone(),
            });
        let response = self
            .responses
            .lock()
            .expect("model response lock poisoned")
            .pop_front()
            .expect("scripted model response exhausted");
        Box::pin(async move { response })
    }

    fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

fn backend() -> (OpenLessBackend, std::path::PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-selection-voice-contract-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        BackendDependencies::unsupported(),
    )
    .unwrap();
    (backend, data_dir)
}

fn backend_with_model(
    preferences: UserPreferences,
    polisher: Arc<ScriptedPolisher>,
) -> (OpenLessBackend, std::path::PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-selection-voice-workflow-contract-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let repositories = BackendRepositories::open(&data_dir).unwrap();
    repositories.preferences.set(preferences).unwrap();
    let capture = SelectionCapture {
        text: "fixture selection".to_string(),
        source_app: Some("Fixture Editor".to_string()),
    };
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.selection_runtime = Some(Arc::new(FixtureSelectionRuntime::successful(
        capture,
        InsertOutcome::Inserted,
    )));
    dependencies.selection_polisher = Some(polisher);
    let backend = OpenLessBackend::new_with_repositories(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
        repositories,
    )
    .unwrap();
    (backend, data_dir)
}

#[tokio::test]
async fn transcript_correction_polish_and_auto_classification_are_core_owned() {
    let preferences = UserPreferences {
        selection_voice_intent_mode: SelectionVoiceIntentMode::Auto,
        ..UserPreferences::default()
    };
    let polisher = Arc::new(ScriptedPolisher::successful([
        "polished question?",
        "<intent>question</intent>",
    ]));
    let (backend, data_dir) = backend_with_model(preferences, Arc::clone(&polisher));
    backend
        .add_correction_rule("牵引".to_string(), "迁移".to_string())
        .unwrap();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();

    let disposition = voice
        .process_transcript(session_id, "把牵引改成什么？".to_string())
        .await
        .unwrap();

    assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Question));
    let snapshot = voice.snapshot().await.unwrap();
    assert_eq!(
        snapshot.instruction_raw.as_deref(),
        Some("把迁移改成什么？")
    );
    assert_eq!(
        snapshot.instruction_polished.as_deref(),
        Some("polished question?")
    );
    let calls = polisher.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].input, "把迁移改成什么？");
    assert!(calls[0].system_prompt.contains("指令润色"));
    assert_eq!(calls[1].input, "polished question?");
    assert!(calls[1].system_prompt.contains("意图分类"));

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn direct_edit_plan_generation_and_application_are_core_owned() {
    let preferences = UserPreferences {
        selection_voice_intent_mode: SelectionVoiceIntentMode::Manual,
        selection_voice_manual_intent: SelectionVoiceManualIntent::Edit,
        selection_polish_output_mode: SelectionPolishOutputMode::DirectReplace,
        ..UserPreferences::default()
    };
    let polisher = Arc::new(ScriptedPolisher::successful([
        "replace beta with gamma",
        "<edit_plan><summary>rename</summary><literal_replace><find>beta</find><replace>gamma</replace></literal_replace></edit_plan>",
    ]));
    let (backend, data_dir) = backend_with_model(preferences, Arc::clone(&polisher));
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "alpha beta".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();
    let disposition = voice
        .process_transcript(session_id, "replace beta".to_string())
        .await
        .unwrap();
    assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Edit));

    let route = voice.route_disposition(disposition).await.unwrap();
    let SelectionVoiceRoute::ReadyToApply { preview } = route else {
        panic!("direct-replace mode must return a ready preview");
    };
    assert_eq!(preview.text, "alpha gamma");
    assert_eq!(preview.summary.as_deref(), Some("rename"));
    let calls = polisher.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].system_prompt.contains("语音编辑"));
    assert!(calls[1].input.contains("<draft>\nalpha beta\n</draft>"));

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn hold_toggle_auto_and_busy_hotkey_decisions_are_core_owned() {
    let (backend, data_dir) = backend();
    let voice = &backend.services().selection_voice;
    let mut preferences = backend.get_preferences();
    preferences.selection_voice_enabled = true;

    preferences.hotkey.mode = openless_core::HotkeyMode::Hold;
    backend
        .repositories()
        .preferences
        .set(preferences.clone())
        .unwrap();
    let pressed = std::time::Instant::now();
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Pressed { at: pressed })
            .unwrap(),
        SelectionVoiceHotkeyAction::Start
    );
    let session_id = voice
        .begin(SelectionCapture {
            text: "selection".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Released {
                at: pressed + std::time::Duration::from_millis(20),
            })
            .unwrap(),
        SelectionVoiceHotkeyAction::Finish
    );
    voice.cancel(Some(session_id)).await.unwrap();

    preferences.hotkey.mode = openless_core::HotkeyMode::Auto;
    backend
        .repositories()
        .preferences
        .set(preferences.clone())
        .unwrap();
    let pressed = std::time::Instant::now();
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Pressed { at: pressed })
            .unwrap(),
        SelectionVoiceHotkeyAction::Start
    );
    let session_id = voice
        .begin(SelectionCapture {
            text: "selection".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Released {
                at: pressed + std::time::Duration::from_millis(349),
            })
            .unwrap(),
        SelectionVoiceHotkeyAction::Noop
    );
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Pressed {
                at: pressed + std::time::Duration::from_millis(500),
            })
            .unwrap(),
        SelectionVoiceHotkeyAction::Finish
    );
    voice.mark_processing(session_id).await.unwrap();
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Pressed {
                at: pressed + std::time::Duration::from_secs(1),
            })
            .unwrap(),
        SelectionVoiceHotkeyAction::Noop
    );
    voice.cancel(Some(session_id)).await.unwrap();

    preferences.hotkey.mode = openless_core::HotkeyMode::Toggle;
    backend.repositories().preferences.set(preferences).unwrap();
    assert_eq!(
        voice
            .dispatch_hotkey_edge(SelectionVoiceHotkeyEdge::Pressed {
                at: std::time::Instant::now(),
            })
            .unwrap(),
        SelectionVoiceHotkeyAction::Start
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn recording_fault_fails_only_the_current_selection_voice_session_and_releases_busy() {
    let (backend, data_dir) = backend();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "selection".to_string(),
            source_app: None,
        })
        .await
        .unwrap();

    voice
        .recording_fault(
            session_id,
            BackendError::new(BackendErrorCode::Platform, "microphone disconnected"),
        )
        .await
        .unwrap();
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::Failed
    );
    assert_eq!(
        voice
            .recording_fault(
                session_id,
                BackendError::new(BackendErrorCode::Platform, "late fault"),
            )
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::InvalidState
    );
    voice
        .begin(SelectionCapture {
            text: "next selection".to_string(),
            source_app: None,
        })
        .await
        .unwrap();

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn translation_edit_uses_the_core_translation_path_and_target() {
    let preferences = UserPreferences {
        selection_voice_intent_mode: SelectionVoiceIntentMode::Manual,
        selection_voice_manual_intent: SelectionVoiceManualIntent::Edit,
        ..UserPreferences::default()
    };
    let polisher = Arc::new(ScriptedPolisher::successful([
        "翻译成英文",
        "## Translation\nHello world",
    ]));
    let (backend, data_dir) = backend_with_model(preferences, Arc::clone(&polisher));
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "你好世界".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();
    voice
        .process_transcript(session_id, "翻译成英文".to_string())
        .await
        .unwrap();

    let action = voice.prepare_edit(session_id, None).await.unwrap();
    let SelectionVoiceEditAction::ReadyToApply { preview } = action else {
        panic!("translation in direct mode must return a ready preview");
    };
    assert_eq!(preview.text, "Hello world");
    assert_eq!(preview.summary.as_deref(), Some("翻译为English"));
    let calls = polisher.calls();
    assert!(calls[1].translation_active);
    assert_eq!(calls[1].translation_target, "English");
    assert_eq!(calls[1].input, "你好世界");

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn preview_mode_and_qa_preview_revision_are_core_owned() {
    let preferences = UserPreferences {
        selection_voice_intent_mode: SelectionVoiceIntentMode::Manual,
        selection_voice_manual_intent: SelectionVoiceManualIntent::Edit,
        selection_polish_output_mode: SelectionPolishOutputMode::PreviewConfirm,
        ..UserPreferences::default()
    };
    let polisher = Arc::new(ScriptedPolisher::successful([
        "rewrite this",
        "<edit_plan><full_rewrite><text>first preview</text></full_rewrite></edit_plan>",
        "<edit_plan><summary>second pass</summary><full_rewrite><text>second preview</text></full_rewrite></edit_plan>",
    ]));
    let (backend, data_dir) = backend_with_model(preferences, Arc::clone(&polisher));
    let voice = &backend.services().selection_voice;
    let capture = SelectionCapture {
        text: "source".to_string(),
        source_app: None,
    };
    let session_id = voice.begin(capture.clone()).await.unwrap();
    voice.mark_processing(session_id).await.unwrap();
    voice
        .process_transcript(session_id, "rewrite".to_string())
        .await
        .unwrap();

    let action = voice.prepare_edit(session_id, None).await.unwrap();
    assert!(matches!(
        action,
        SelectionVoiceEditAction::OpenConversation { .. }
    ));
    assert_eq!(polisher.calls().len(), 1);
    let owner_session_id = SessionId::new();
    let first = voice
        .edit_preview(SelectionVoiceEditRequest {
            owner_session_id,
            capture: capture.clone(),
            instruction: "first".to_string(),
        })
        .await
        .unwrap();
    assert!(!first.replaced_existing);
    assert_eq!(first.preview.text, "first preview");
    let second = voice
        .edit_preview(SelectionVoiceEditRequest {
            owner_session_id,
            capture,
            instruction: "second".to_string(),
        })
        .await
        .unwrap();
    assert!(second.replaced_existing);
    assert_eq!(second.preview.text, "second preview");
    assert!(second.preview.can_revert);
    assert_eq!(second.preview.summary.as_deref(), Some("second pass"));

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn intent_prompt_is_core_owned_and_invalid_confirmation_is_non_destructive() {
    let (backend, data_dir) = backend();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: Some("Fixture Editor".to_string()),
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();

    let disposition = voice
        .resolve_instruction(SelectionVoiceInstructionRequest {
            session_id,
            raw: "raw instruction".to_string(),
            polished: "polished instruction".to_string(),
            intent_mode: SelectionVoiceIntentMode::Prompt,
            manual_intent: SelectionVoiceManualIntent::Question,
            question_keywords: Vec::new(),
            auto_classification: None,
        })
        .await
        .unwrap();
    assert!(disposition.is_awaiting_intent());

    let error = voice
        .confirm_intent(session_id, "unknown".to_string())
        .await
        .expect_err("invalid intent must be rejected");
    assert_eq!(error.code, BackendErrorCode::InvalidArgument);
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::AwaitingIntent
    );

    let disposition = voice
        .confirm_intent(session_id, "edit".to_string())
        .await
        .unwrap();
    assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Edit));
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::Processing
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn preview_apply_consumes_state_only_after_the_host_confirms_success() {
    let (backend, data_dir) = backend();
    backend
        .add_correction_rule("{num}粒".to_string(), "{num}例".to_string())
        .unwrap();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id,
            owner_session_id: Some(session_id),
            text: "preview".to_string(),
            summary: Some("summary".to_string()),
        })
        .await
        .unwrap();

    let ticket = voice
        .begin_preview_apply(Some(session_id), "edited 1粒".to_string())
        .unwrap();
    assert_eq!(ticket.replacement_text, "edited 1例");
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::Failed)
        .await
        .unwrap();
    assert!(voice.preview(Some(session_id)).await.unwrap().is_some());
    assert!(backend.list_history().unwrap().is_empty());

    let ticket = voice
        .begin_preview_apply(Some(session_id), "edited 1粒".to_string())
        .unwrap();
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::Inserted)
        .await
        .unwrap();
    assert!(voice.preview(Some(session_id)).await.unwrap().is_none());
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::Completed
    );
    let history = backend.list_history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].final_text, "edited 1例");
    assert_eq!(
        history[0].source,
        openless_core::HistorySource::SelectionVoiceEdit
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn paste_sent_completes_direct_and_qa_previews_once_without_claiming_insertion() {
    let paste_sent: SelectionVoiceApplyOutcome = serde_json::from_str("\"paste_sent\"")
        .expect("Windows paste dispatch must be represented in the shared result contract");
    for qa_preview in [false, true] {
        let (backend, data_dir) = backend();
        let voice = &backend.services().selection_voice;
        let session_id = voice
            .begin(SelectionCapture {
                text: "source".into(),
                source_app: None,
            })
            .await
            .unwrap();
        let owner = qa_preview.then(SessionId::new);
        voice.mark_processing(session_id).await.unwrap();
        voice
            .set_preview(SelectionVoicePreviewUpdate {
                session_id,
                owner_session_id: owner,
                text: "preview".into(),
                summary: None,
            })
            .await
            .unwrap();

        let failed = voice
            .begin_preview_apply(owner, "replacement".into())
            .unwrap();
        voice
            .finish_preview_apply(failed.ticket_id, SelectionVoiceApplyOutcome::Failed)
            .await
            .unwrap();
        assert!(voice.preview(owner).await.unwrap().is_some());
        assert!(backend.list_history().unwrap().is_empty());

        let ticket = voice
            .begin_preview_apply(owner, "replacement".into())
            .unwrap();
        assert!(voice
            .begin_preview_apply(owner, "duplicate".into())
            .is_err());
        voice
            .finish_preview_apply(ticket.ticket_id, paste_sent)
            .await
            .unwrap();
        let snapshot = voice.snapshot().await.unwrap();
        assert_eq!(snapshot.phase, SelectionVoicePhase::Completed);
        assert_eq!(
            serde_json::to_value(snapshot).unwrap()["applyOutcome"],
            "paste_sent"
        );
        assert!(voice.preview(owner).await.unwrap().is_none());
        assert!(voice
            .finish_preview_apply(ticket.ticket_id, paste_sent)
            .await
            .is_err());
        assert!(voice
            .begin_preview_apply(owner, "duplicate".into())
            .is_err());
        let history = backend.list_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].final_text, "replacement");
        assert_eq!(
            history[0].insert_status,
            openless_core::HistoryInsertStatus::PasteSent
        );
        assert_eq!(
            history[0].source,
            openless_core::HistorySource::SelectionVoiceEdit
        );
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn stale_preview_requests_preserve_the_current_owner_and_revert_is_single_step() {
    let (backend, data_dir) = backend();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id,
            owner_session_id: Some(session_id),
            text: "first".to_string(),
            summary: None,
        })
        .await
        .unwrap();
    voice
        .replace_preview(Some(session_id), "second".to_string(), None)
        .await
        .unwrap();

    let stale = openless_core::SessionId::new();
    let error = voice
        .revert_preview(Some(stale))
        .expect_err("stale owner must not mutate the current preview");
    assert_eq!(error.code, BackendErrorCode::Cancelled);
    assert_eq!(
        voice.preview(Some(session_id)).await.unwrap().unwrap().text,
        "second"
    );

    let preview = voice.revert_preview(Some(session_id)).unwrap();
    assert_eq!(preview.text, "first");
    assert!(!preview.can_revert);
    assert_eq!(
        voice
            .revert_preview(Some(session_id))
            .expect_err("only the immediately previous preview can be restored")
            .code,
        BackendErrorCode::InvalidState
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn shutdown_cancels_an_active_selection_voice_session() {
    let (backend, data_dir) = backend();
    backend.start().await.unwrap();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();

    backend.shutdown().await.unwrap();

    let snapshot = voice.snapshot().await.unwrap();
    assert_eq!(snapshot.session_id, Some(session_id));
    assert_eq!(snapshot.phase, SelectionVoicePhase::Cancelled);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn selection_voice_publishes_typed_lifecycle_events() {
    let (backend, data_dir) = backend();
    let mut events = backend.subscribe();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();
    voice
        .resolve_instruction(SelectionVoiceInstructionRequest {
            session_id,
            raw: "edit".to_string(),
            polished: "edit".to_string(),
            intent_mode: SelectionVoiceIntentMode::Prompt,
            manual_intent: SelectionVoiceManualIntent::Edit,
            question_keywords: Vec::new(),
            auto_classification: None,
        })
        .await
        .unwrap();
    voice
        .confirm_intent(session_id, "edit".to_string())
        .await
        .unwrap();

    let mut phases = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let BackendEventKind::SelectionVoiceStateChanged(snapshot) = event.kind {
            phases.push(snapshot.phase);
        }
    }
    assert_eq!(
        phases,
        vec![
            SelectionVoicePhase::Recording,
            SelectionVoicePhase::Processing,
            SelectionVoicePhase::AwaitingIntent,
            SelectionVoicePhase::Processing,
        ]
    );
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn auto_intent_uses_the_host_provider_result_but_falls_back_inside_core() {
    let (backend, data_dir) = backend();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();

    let disposition = voice
        .resolve_instruction(SelectionVoiceInstructionRequest {
            session_id,
            raw: "summarize".to_string(),
            polished: "summarize".to_string(),
            intent_mode: SelectionVoiceIntentMode::Auto,
            manual_intent: SelectionVoiceManualIntent::Question,
            question_keywords: Vec::new(),
            auto_classification: Some(r#"{"intent":"question"}"#.to_string()),
        })
        .await
        .unwrap();
    assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Question));
    voice.complete(session_id).await.unwrap();

    let fallback_session = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(fallback_session).await.unwrap();
    let disposition = voice
        .resolve_instruction(SelectionVoiceInstructionRequest {
            session_id: fallback_session,
            raw: "summarize".to_string(),
            polished: "summarize".to_string(),
            intent_mode: SelectionVoiceIntentMode::Auto,
            manual_intent: SelectionVoiceManualIntent::Question,
            question_keywords: Vec::new(),
            auto_classification: Some("not a classification".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Edit));

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn completing_a_question_is_terminal_and_stale_completion_is_rejected() {
    let (backend, data_dir) = backend();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "source".to_string(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(session_id).await.unwrap();
    voice.complete(session_id).await.unwrap();
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::Completed
    );
    assert_eq!(
        voice
            .complete(openless_core::SessionId::new())
            .await
            .expect_err("stale completion must not finish the current session")
            .code,
        BackendErrorCode::Cancelled
    );

    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn selection_voice_snapshot_apply_outcome_wire_fixture_is_stable() {
    let session_id = openless_core::SessionId::from_uuid(uuid::Uuid::nil());
    let snapshot = SelectionVoiceSnapshot {
        phase: SelectionVoicePhase::Completed,
        session_id: Some(session_id),
        source_text: Some("source".to_string()),
        instruction_raw: None,
        instruction_polished: None,
        intent_prompt: None,
        preview: None,
        apply_outcome: Some(SelectionVoiceApplyOutcome::CopiedFallback),
    };

    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        serde_json::json!({
            "phase": "completed",
            "sessionId": "00000000-0000-0000-0000-000000000000",
            "sourceText": "source",
            "applyOutcome": "copied_fallback"
        })
    );
}

#[test]
fn selection_voice_instruction_auto_classification_wire_fixture_is_stable() {
    let request = SelectionVoiceInstructionRequest {
        session_id: openless_core::SessionId::from_uuid(uuid::Uuid::nil()),
        raw: "raw".to_string(),
        polished: "polished".to_string(),
        intent_mode: SelectionVoiceIntentMode::Auto,
        manual_intent: SelectionVoiceManualIntent::Question,
        question_keywords: vec!["why".to_string()],
        auto_classification: Some(r#"{"intent":"edit"}"#.to_string()),
    };

    assert_eq!(
        serde_json::to_value(request).unwrap(),
        serde_json::json!({
            "sessionId": "00000000-0000-0000-0000-000000000000",
            "raw": "raw",
            "polished": "polished",
            "intentMode": "auto",
            "manualIntent": "question",
            "questionKeywords": ["why"],
            "autoClassification": "{\"intent\":\"edit\"}"
        })
    );
}
