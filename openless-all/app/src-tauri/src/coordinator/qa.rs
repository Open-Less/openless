use super::Inner;
use std::sync::Arc;

pub(super) fn install_recording_companion(inner: &Arc<Inner>) {
    let target = inner.hotkey_runtime_target.lock().clone();
    crate::recording_companion::configure(target.dictation, target.qa_companion_key);
    let weak = Arc::downgrade(inner);
    crate::recording_companion::install(Arc::new(move || {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        if inner
            .shortcut_recording_active
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let task = Arc::clone(&inner);
        let _ = inner.host.spawn(async move {
            handle_qa_hotkey_pressed(&task).await;
        });
    }));
}

/// One chord owns the complete question: start → record → finish. Processing
/// never queues another question and a completed result is replaced by a fresh turn.
pub(super) async fn handle_qa_hotkey_pressed(inner: &Arc<Inner>) {
    let Ok(_handoff) = inner.overlay_qa_handoff.try_lock() else {
        return;
    };
    let qa = &inner.backend.services().qa;
    let Ok(snapshot) = qa.snapshot().await else {
        return;
    };
    let result = match (snapshot.phase, snapshot.session_id) {
        (openless_core::QaPhase::Recording, Some(session_id)) => {
            qa.stop_recording(session_id).await
        }
        (openless_core::QaPhase::Thinking | openless_core::QaPhase::AwaitingApproval, _) => return,
        _ => {
            inner
                .backend
                .start_voice_question_from_recording_key()
                .await
        }
    };
    if let Err(error) = result {
        log::warn!("[ask] question hotkey failed: {error}");
    }
}
