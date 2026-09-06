use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendErrorCode, DictationContext,
    EventRecvError, OpenLessBackend, PolishOutput, QaInput, QaProgressSink, QaRuntimeAdapter,
    QaTurnRequest, QaTurnResult, SessionId, TextPolisher, TextStreamSink,
};

struct QaPlatform;

impl QaRuntimeAdapter for QaPlatform {
    fn prepare_text(
        &self,
        _: SessionId,
        text: String,
    ) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        Box::pin(async move {
            Ok(QaInput {
                text,
                selection_text: Some("source".into()),
                selection_source_app: None,
            })
        })
    }

    fn start_recording(
        &self,
        _: SessionId,
        _: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        panic!("text edit must not start a microphone")
    }

    fn finish_recording(&self, _: SessionId) -> BoxFuture<'static, Result<QaInput, BackendError>> {
        panic!("text edit must not finish a microphone")
    }

    fn answer(
        &self,
        _: QaTurnRequest,
        _: Arc<dyn QaProgressSink>,
    ) -> BoxFuture<'static, Result<QaTurnResult, BackendError>> {
        panic!("edit answers belong to Core")
    }

    fn bind_selection_voice_target(&self, _: SessionId, _: SessionId) -> Result<(), BackendError> {
        Ok(())
    }

    fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct NextPreview(AtomicUsize);

impl TextPolisher for NextPreview {
    fn polish(
        &self,
        _: SessionId,
        _: Arc<DictationContext>,
        _: String,
        _: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
        let revision = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move {
            Ok(PolishOutput::text(format!(
                "<edit_plan><full_rewrite><text>preview {revision}</text></full_rewrite></edit_plan>"
            )))
        })
    }

    fn cancel(&self, _: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

fn backend() -> (
    OpenLessBackend,
    std::path::PathBuf,
    Arc<openless_core::testing::RecordingHostActions>,
) {
    let data_dir = std::env::temp_dir().join(format!("openless-qa-answer-{}", SessionId::new()));
    let host = Arc::new(openless_core::testing::RecordingHostActions::default());
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.host_actions = host.clone();
    dependencies.qa_runtime = Some(Arc::new(QaPlatform));
    dependencies.selection_polisher = Some(Arc::new(NextPreview::default()));
    dependencies.selection_runtime = Some(Arc::new(
        openless_core::testing::FixtureSelectionRuntime::successful(
            openless_core::SelectionCapture {
                text: "source".into(),
                source_app: None,
            },
            openless_core::InsertOutcome::Inserted,
        ),
    ));
    let backend = OpenLessBackend::new(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..Default::default()
        },
        dependencies,
    )
    .unwrap();
    (backend, data_dir, host)
}

#[tokio::test]
async fn delayed_preview_revert_cannot_replace_a_new_qa_turn_or_conversation() {
    for reopen in [false, true] {
        let (backend, data_dir, _) = backend();
        let qa = &backend.services().qa;
        let voice = &backend.services().selection_voice;
        qa.set_edit_instruction_mode(true).await.unwrap();
        qa.submit_text("first".into()).await.unwrap();
        qa.submit_text("second".into()).await.unwrap();

        // The real Tauri command now carries the displayed turn straight to
        // one Core transaction, before either the preview or answer can change.
        let captured = qa.snapshot().await.unwrap();
        let owner = captured.conversation_id;
        let delayed = qa.revert_edit_preview(captured.session_id.unwrap());

        // Pause at the final await. The user can complete another turn either
        // in this conversation or after closing and reopening the panel.
        if reopen {
            qa.dismiss().await.unwrap();
            qa.set_edit_instruction_mode(true).await.unwrap();
        }
        qa.submit_text("third".into()).await.unwrap();
        let current = qa.snapshot().await.unwrap();
        assert_ne!(current.session_id, captured.session_id);
        assert_eq!(current.conversation_id != owner, reopen);
        assert_eq!(current.messages.last().unwrap().content, "preview 3");
        let current_preview = voice.preview(current.conversation_id).await.unwrap();
        let mut events = backend.subscribe();
        let result = delayed.await;
        assert_eq!(
            qa.snapshot().await.unwrap(),
            current,
            "a stale revert overwrote the new answer"
        );
        assert_eq!(result.unwrap_err().code, BackendErrorCode::Cancelled);
        assert_eq!(
            voice.preview(current.conversation_id).await.unwrap(),
            current_preview,
            "a stale revert changed the new turn's underlying preview"
        );
        assert!(matches!(events.try_recv(), Err(EventRecvError::Empty)));

        // The current turn still supports a normal single-step preview revert.
        qa.submit_text("fourth".into()).await.unwrap();
        let current = qa.snapshot().await.unwrap();
        qa.revert_edit_preview(current.session_id.unwrap())
            .await
            .unwrap();
        let text = voice
            .preview(current.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .text;
        assert_eq!(text, "preview 3");
        let reverted = qa.snapshot().await.unwrap();
        assert_eq!(reverted.session_id, current.session_id);
        assert_eq!(reverted.messages.last().unwrap().content, text);
        assert!(!reverted.edit_revert_available);
        qa.dismiss().await.unwrap();
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preview_revert_and_qa_answer_remain_atomic_at_the_selection_event_boundary() {
    use futures_util::task::{waker_ref, ArcWake};
    use std::future::Future;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Condvar, Mutex};

    struct PauseDelivery {
        once: AtomicBool,
        entered: tokio::sync::Notify,
        release: (Mutex<bool>, Condvar),
    }
    impl ArcWake for PauseDelivery {
        fn wake_by_ref(this: &Arc<Self>) {
            if !this.once.swap(true, Ordering::AcqRel) {
                this.entered.notify_one();
                let mut released = this.release.0.lock().unwrap();
                while !*released {
                    released = this.release.1.wait(released).unwrap();
                }
            }
        }
    }
    struct ReleaseOnDrop(Arc<PauseDelivery>);
    impl ReleaseOnDrop {
        fn release(&self) {
            *self.0.release.0.lock().unwrap() = true;
            self.0.release.1.notify_all();
        }
    }
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.release();
        }
    }

    for reopen in [false, true] {
        let (backend, data_dir, host) = backend();
        let qa = Arc::clone(&backend.services().qa);
        qa.set_edit_instruction_mode(true).await.unwrap();
        qa.submit_text("first".into()).await.unwrap();
        qa.submit_text("second".into()).await.unwrap();
        let turn = qa.snapshot().await.unwrap().session_id.unwrap();
        let shown_before = host.actions().len();
        let pause = Arc::new(PauseDelivery {
            once: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            release: (Mutex::new(false), Condvar::new()),
        });
        let release = ReleaseOnDrop(pause.clone());
        let mut events = backend.subscribe();
        let mut receive = Box::pin(events.recv());
        let waker = waker_ref(&pause);
        assert!(receive
            .as_mut()
            .poll(&mut std::task::Context::from_waker(&waker))
            .is_pending());

        // Preempt A after its Selection preview changes, before the QA answer
        // changes. This was the Host's second await boundary in the old chain.
        let executor = tokio::runtime::Handle::current();
        let revert = std::thread::spawn({
            let qa = qa.clone();
            let executor = executor.clone();
            move || executor.block_on(qa.revert_edit_preview(turn))
        });
        pause.entered.notified().await;
        let next = std::thread::spawn({
            let qa = qa.clone();
            move || {
                executor.block_on(async move {
                    if reopen {
                        qa.dismiss().await?;
                        qa.set_edit_instruction_mode(true).await?;
                    }
                    qa.submit_text("third".into()).await
                })
            }
        });
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), async {
            while host.actions().len() == shown_before {
                tokio::task::yield_now().await;
            }
        })
        .await;
        release.release();
        let (reverted, next) =
            tokio::task::spawn_blocking(move || (revert.join().unwrap(), next.join().unwrap()))
                .await
                .unwrap();
        reverted.unwrap();
        next.unwrap();
        let snapshot = qa.snapshot().await.unwrap();
        assert_ne!(snapshot.session_id, Some(turn));
        assert_eq!(snapshot.messages.last().unwrap().content, "preview 3");
        assert_eq!(
            backend
                .services()
                .selection_voice
                .preview(snapshot.conversation_id)
                .await
                .unwrap()
                .unwrap()
                .text,
            "preview 3"
        );
        drop(receive);
        qa.dismiss().await.unwrap();
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn delayed_preview_confirmation_must_not_apply_old_text_to_a_new_qa_turn() {
    let (backend, data_dir, _) = backend();
    let qa = &backend.services().qa;
    let voice = &backend.services().selection_voice;
    qa.set_edit_instruction_mode(true).await.unwrap();
    qa.submit_text("first".into()).await.unwrap();
    let captured = qa.snapshot().await.unwrap();
    let text = voice
        .preview(captured.conversation_id)
        .await
        .unwrap()
        .unwrap()
        .text;

    // The real command retains the original turn until the atomic Core begin.
    let pending = qa.begin_edit_preview_apply(captured.session_id.unwrap(), text);
    qa.submit_text("second".into()).await.unwrap();
    let current = qa.snapshot().await.unwrap();
    assert_ne!(current.session_id, captured.session_id);
    assert_eq!(current.conversation_id, captured.conversation_id);
    let preview = voice.preview(current.conversation_id).await.unwrap();
    assert_eq!(preview.as_ref().unwrap().text, "preview 2");

    let result = pending.await;
    // Replace only the native write with a receipt. All owner/ticket/history
    // decisions use the production services, as does the Tauri confirm command.
    let applied = if let Ok(ticket) = &result {
        voice
            .finish_preview_apply(
                ticket.ticket_id,
                openless_core::SelectionVoiceApplyOutcome::PasteSent,
            )
            .await
            .unwrap();
        Some(ticket.replacement_text.clone())
    } else {
        None
    };
    assert_eq!(
        applied, None,
        "an old confirmation was authorized to apply text to the new preview"
    );
    assert_eq!(result.unwrap_err().code, BackendErrorCode::Cancelled);
    assert_eq!(
        voice.preview(current.conversation_id).await.unwrap(),
        preview
    );
    assert!(backend.list_history().unwrap().is_empty());
    qa.dismiss().await.unwrap();
    drop(backend);
    std::fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn completed_preview_confirmation_must_not_dismiss_a_reopened_qa_panel() {
    for complete_new_turn in [false, true] {
        let (backend, data_dir, host) = backend();
        let qa = &backend.services().qa;
        let voice = &backend.services().selection_voice;
        qa.set_edit_instruction_mode(true).await.unwrap();
        qa.submit_text("first".into()).await.unwrap();
        let captured = qa.snapshot().await.unwrap();
        let text = voice
            .preview(captured.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .text;
        let ticket = qa
            .begin_edit_preview_apply(captured.session_id.unwrap(), text)
            .await
            .unwrap();
        let applied = openless_core::SelectionVoiceApplyOutcome::PasteSent;
        voice
            .finish_preview_apply(ticket.ticket_id, applied)
            .await
            .unwrap();

        // Native paste and Core finalization have succeeded. Pause before the
        // command's unscoped dismiss; a user then closes A and completes a new B.
        let pending_close = qa.dismiss_session(captured.session_id.unwrap());
        qa.dismiss().await.unwrap();
        if complete_new_turn {
            qa.set_edit_instruction_mode(true).await.unwrap();
            qa.submit_text("second".into()).await.unwrap();
        } else {
            qa.show().await.unwrap();
        }
        let current = qa.snapshot().await.unwrap();
        assert_ne!(current.conversation_id, captured.conversation_id);
        let actions = host.actions();
        // The actual command preserves its native receipt when ownership changed.
        assert_eq!(
            pending_close.await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert_eq!(
            qa.snapshot().await.unwrap(),
            current,
            "old apply cleanup closed the new QA panel"
        );
        assert_eq!(
            host.actions(),
            actions,
            "old apply cleanup must not request HideQa for B"
        );
        assert_eq!(
            backend.list_history().unwrap()[0].insert_status,
            openless_core::HistoryInsertStatus::PasteSent
        );
        qa.dismiss().await.unwrap();
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn qa_preview_confirmation_retries_native_failure_and_closes_only_its_owner() {
    use openless_core::SelectionVoiceApplyOutcome;

    let (backend, data_dir, host) = backend();
    let qa = &backend.services().qa;
    let voice = &backend.services().selection_voice;
    qa.set_edit_instruction_mode(true).await.unwrap();
    qa.submit_text("first".into()).await.unwrap();
    let captured = qa.snapshot().await.unwrap();
    let turn = captured.session_id.unwrap();
    let failed = qa
        .begin_edit_preview_apply(turn, "replacement".into())
        .await
        .unwrap();
    assert!(qa
        .begin_edit_preview_apply(turn, "duplicate".into())
        .await
        .is_err());
    voice
        .finish_preview_apply(failed.ticket_id, SelectionVoiceApplyOutcome::Failed)
        .await
        .unwrap();
    assert!(voice
        .preview(captured.conversation_id)
        .await
        .unwrap()
        .is_some());
    assert!(backend.list_history().unwrap().is_empty());

    let ticket = qa
        .begin_edit_preview_apply(turn, "replacement".into())
        .await
        .unwrap();
    assert_eq!(ticket.owner_session_id, captured.conversation_id);
    voice
        .finish_preview_apply(ticket.ticket_id, SelectionVoiceApplyOutcome::PasteSent)
        .await
        .unwrap();
    qa.dismiss_session(turn).await.unwrap();
    assert_eq!(
        qa.snapshot().await.unwrap(),
        openless_core::QaSnapshot::default()
    );
    assert_eq!(
        host.actions().last(),
        Some(&openless_core::HostAction::HideQa)
    );
    let history = backend.list_history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].insert_status,
        openless_core::HistoryInsertStatus::PasteSent
    );
    assert_eq!(
        qa.begin_edit_preview_apply(turn, "duplicate".into())
            .await
            .unwrap_err()
            .code,
        BackendErrorCode::Cancelled
    );
    let actions = host.actions();
    assert_eq!(
        qa.dismiss_session(turn).await.unwrap_err().code,
        BackendErrorCode::Cancelled
    );
    assert_eq!(host.actions(), actions);
    drop(backend);
    std::fs::remove_dir_all(data_dir).unwrap();
}
