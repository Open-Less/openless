use std::sync::Arc;

use futures_util::future::BoxFuture;
use openless_core::testing::{
    FixtureAudioRecorder, FixtureSelectionRuntime, FixtureTextInserter, FixtureTextPolisher,
    FixtureTranscriptionEngine,
};
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, InsertOutcome,
    OpenLessBackend, PipelineDictationEngine, QaInput, QaPhase, QaProgress, QaProgressSink,
    QaRuntimeAdapter, QaTurnRequest, QaTurnResult, RecordingControlAction, RecordingControlSink,
    SelectionCapture, SelectionVoiceInstructionRequest, SelectionVoiceIntentMode,
    SelectionVoiceManualIntent, SelectionVoicePhase, SelectionVoicePreviewUpdate, SessionId,
};

// Only the platform selection/recording boundary is replaced. The real QA and
// Selection Voice services share the backend's real voice gate in every test.
struct QaPlatform;

fn input(text: String) -> QaInput {
    QaInput {
        text,
        selection_text: Some("original selection".into()),
        selection_source_app: Some("Fixture Editor".into()),
    }
}

impl QaRuntimeAdapter for QaPlatform {
    fn prepare_text(
        &self,
        _session_id: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        Box::pin(async move { Ok(input(text)) })
    }

    fn start_recording(
        &self,
        session_id: SessionId,
        progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            progress.publish(
                session_id,
                QaProgress::SelectionCaptured(input(String::new()).selection_text),
            )
        })
    }

    fn finish_recording(
        &self,
        _session_id: SessionId,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        Box::pin(async { Ok(input("shorten it".into())) })
    }

    fn answer(
        &self,
        _request: QaTurnRequest,
        _progress: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>> {
        panic!("edit instructions must use the shared Selection Voice workflow")
    }

    fn bind_selection_voice_target(
        &self,
        _qa_session_id: SessionId,
        _selection_voice_session_id: SessionId,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

fn backend() -> (OpenLessBackend, std::path::PathBuf) {
    let data_dir =
        std::env::temp_dir().join(format!("openless-qa-edit-lease-{}", SessionId::new()));
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.qa_runtime = Some(Arc::new(QaPlatform));
    dependencies.selection_polisher = Some(Arc::new(FixtureTextPolisher::successful(
        "<edit_plan><full_rewrite><text>short</text></full_rewrite></edit_plan>",
    )));
    dependencies.selection_runtime = Some(Arc::new(FixtureSelectionRuntime::successful(
        SelectionCapture {
            text: "original selection".into(),
            source_app: None,
        },
        InsertOutcome::Inserted,
    )));
    dependencies.dictation_engine = Arc::new(PipelineDictationEngine::new(
        Arc::new(FixtureAudioRecorder::default()),
        Arc::new(FixtureTranscriptionEngine::successful("shorten it", 100)),
        Arc::new(FixtureTextPolisher::successful("dictation text")),
    ));
    dependencies.text_inserter =
        Arc::new(FixtureTextInserter::with_outcome(InsertOutcome::Inserted));
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
    )
    .unwrap();
    (backend, data_dir)
}

#[tokio::test]
async fn first_qa_voice_edit_generates_a_preview_without_claiming_another_microphone() {
    let (backend, data_dir) = backend();
    let qa = &backend.services().qa;
    qa.set_edit_instruction_mode(true).await.unwrap();
    qa.toggle_recording().await.unwrap();
    let turn = qa.snapshot().await.unwrap().session_id.unwrap();
    qa.stop_recording(turn).await.unwrap();

    let snapshot = qa.snapshot().await.unwrap();
    assert_eq!(snapshot.phase, QaPhase::Completed);
    assert!(snapshot.edit_apply_available);
    assert_eq!(snapshot.messages.last().unwrap().content, "short");
    assert_eq!(
        backend
            .services()
            .selection_voice
            .snapshot()
            .await
            .unwrap()
            .phase,
        SelectionVoicePhase::Preview
    );
    qa.dismiss().await.unwrap();
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn text_edit_preview_accepts_a_voice_follow_up() {
    let (backend, data_dir) = backend();
    backend.start().await.unwrap();
    let qa = &backend.services().qa;
    qa.set_edit_instruction_mode(true).await.unwrap();
    qa.submit_text("shorten it".into()).await.unwrap();
    let previous = qa.snapshot().await.unwrap();

    qa.toggle_recording().await.unwrap();
    let recording = qa.snapshot().await.unwrap();
    assert_ne!(recording.session_id, previous.session_id);
    assert_eq!(recording.conversation_id, previous.conversation_id);
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    qa.stop_recording(recording.session_id.unwrap())
        .await
        .unwrap();
    let completed = qa.snapshot().await.unwrap();
    assert_eq!(completed.phase, QaPhase::Completed);
    assert_eq!(completed.messages.len(), 4);
    assert!(completed.edit_revert_available);
    qa.dismiss().await.unwrap();
    backend.shutdown().await.unwrap();
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn text_edit_preview_does_not_reserve_or_release_the_dictation_microphone() {
    let (backend, data_dir) = backend();
    backend.start().await.unwrap();
    let qa = &backend.services().qa;
    qa.set_edit_instruction_mode(true).await.unwrap();
    qa.submit_text("shorten it".into()).await.unwrap();

    let dictation = backend.start_dictation().await.unwrap();
    qa.dismiss().await.unwrap();
    assert_eq!(backend.snapshot().dictation.session_id, Some(dictation));
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    backend.cancel_dictation(Some(dictation)).await.unwrap();
    backend.shutdown().await.unwrap();
    std::fs::remove_dir_all(data_dir).unwrap();
}

struct RecordingControl;

#[tokio::test]
async fn live_selection_voice_phases_still_reserve_the_microphone() {
    let (backend, data_dir) = backend();
    backend.start().await.unwrap();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "original selection".into(),
            source_app: None,
        })
        .await
        .unwrap();
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    voice.mark_processing(session_id).await.unwrap();
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    voice
        .resolve_instruction(SelectionVoiceInstructionRequest {
            session_id,
            raw: "shorten it".into(),
            polished: "shorten it".into(),
            intent_mode: SelectionVoiceIntentMode::Prompt,
            manual_intent: SelectionVoiceManualIntent::Edit,
            question_keywords: Vec::new(),
            auto_classification: None,
        })
        .await
        .unwrap();
    assert_eq!(
        voice.snapshot().await.unwrap().phase,
        SelectionVoicePhase::AwaitingIntent
    );
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    voice.cancel(Some(session_id)).await.unwrap();
    let dictation = backend.start_dictation().await.unwrap();
    backend.cancel_dictation(Some(dictation)).await.unwrap();
    backend.shutdown().await.unwrap();
    std::fs::remove_dir_all(data_dir).unwrap();
}

impl RecordingControlSink for RecordingControl {
    fn request(
        &self,
        _session_id: SessionId,
        _action: RecordingControlAction,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

#[tokio::test]
async fn selection_voice_preview_waits_for_its_native_capture_hold_before_new_recording() {
    let (backend, data_dir) = backend();
    backend.start().await.unwrap();
    let voice = &backend.services().selection_voice;
    let session_id = voice
        .begin(SelectionCapture {
            text: "original selection".into(),
            source_app: None,
        })
        .await
        .unwrap();
    let capture = backend
        .start_selection_voice_capture(session_id, Arc::new(RecordingControl))
        .await
        .unwrap();
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    voice.mark_processing(session_id).await.unwrap();
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    voice
        .set_preview(SelectionVoicePreviewUpdate {
            session_id,
            owner_session_id: None,
            text: "short".into(),
            summary: None,
        })
        .await
        .unwrap();
    // The logical preview no longer needs a microphone, but the native capture
    // still owns its resource hold. Do not admit another recorder until stop.
    assert_eq!(
        backend.start_dictation().await.unwrap_err().code,
        BackendErrorCode::Busy
    );
    capture.cancel().await.unwrap();
    let dictation = backend.start_dictation().await.unwrap();
    voice.cancel(Some(session_id)).await.unwrap();
    assert_eq!(backend.snapshot().dictation.session_id, Some(dictation));
    backend.cancel_dictation(Some(dictation)).await.unwrap();
    backend.shutdown().await.unwrap();
    std::fs::remove_dir_all(data_dir).unwrap();
}
