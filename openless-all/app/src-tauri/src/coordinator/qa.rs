use std::sync::Arc;

use super::Inner;

pub(super) async fn handle_qa_hotkey_pressed(inner: &Arc<Inner>) {
    let visible = inner.qa_context.is_panel_visible();
    log::info!("[coord] QA hotkey edge (panel_visible={visible})");
    let result = if visible {
        inner.backend.services().qa.dismiss().await
    } else {
        inner.backend.services().qa.show().await
    };
    if let Err(error) = result {
        log::warn!("[coord] QA panel toggle failed: {error}");
    }
}

pub(super) async fn handle_qa_option_edge(inner: &Arc<Inner>) {
    if !inner.qa_context.is_panel_visible() {
        return;
    }
    if let Err(error) = inner.backend.services().qa.toggle_recording().await {
        if error.code != openless_core::BackendErrorCode::Busy {
            log::warn!("[coord] QA recording toggle failed: {error}");
        }
    }
}
