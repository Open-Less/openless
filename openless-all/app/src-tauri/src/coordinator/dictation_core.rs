use std::sync::Arc;

use super::{qa::handle_qa_option_edge, Inner};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LessComputerEventReplay {
    pub(crate) events: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) oldest_sequence: Option<u64>,
    pub(crate) latest_sequence: u64,
    pub(crate) truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) voice_state: Option<openless_core::LessComputerEvent>,
}

pub(crate) fn less_computer_event_replay_after(
    backend: &openless_core::OpenLessBackend,
    sequence: u64,
) -> LessComputerEventReplay {
    let replay = backend.replay_events_after(sequence);
    let mut events: Vec<serde_json::Value> = replay
        .events
        .into_iter()
        .filter_map(|event| match event.kind {
            openless_core::BackendEventKind::LessComputerEvent(event) => {
                serde_json::to_value(event).ok()
            }
            _ => None,
        })
        .collect();
    if let Some(index) = events.iter().rposition(|event| {
        event.get("kind").and_then(serde_json::Value::as_str) == Some("user")
            && event.get("fresh").and_then(serde_json::Value::as_bool) == Some(true)
    }) {
        events.drain(0..index);
    }
    LessComputerEventReplay {
        events,
        oldest_sequence: replay.oldest_sequence,
        latest_sequence: replay.latest_sequence,
        truncated: replay.truncated,
        voice_state: backend.event_publisher().latest_less_computer_voice_state(),
    }
}

async fn dispatch(
    inner: &Arc<Inner>,
    edge: openless_core::DictationHotkeyEdge,
) -> Result<openless_core::CliDispatchOutcome, openless_core::BackendError> {
    if !inner.backend.snapshot().running {
        inner.backend.start().await?;
    }
    inner.backend.dispatch_dictation_hotkey_edge(edge).await
}

pub(super) async fn handle_pressed_edge(
    inner: &Arc<Inner>,
    pressed_at: std::time::Instant,
    press_id: u64,
) {
    if inner.qa_context.is_panel_visible()
        && inner.backend.snapshot().dictation.phase == openless_core::DictationPhase::Idle
    {
        handle_qa_option_edge(inner).await;
        return;
    }
    match dispatch(
        inner,
        openless_core::DictationHotkeyEdge::Pressed {
            press_id,
            at: pressed_at,
        },
    )
    .await
    {
        Ok(_) => {}
        Err(error) => log::warn!("[coord] core dictation press failed: {error}"),
    }
}

pub(super) async fn handle_released_edge(
    inner: &Arc<Inner>,
    released_at: std::time::Instant,
    press_id: u64,
) {
    if inner.qa_context.is_panel_visible()
        && inner.backend.snapshot().dictation.phase == openless_core::DictationPhase::Idle
    {
        return;
    }
    match dispatch(
        inner,
        openless_core::DictationHotkeyEdge::Released {
            press_id,
            at: released_at,
        },
    )
    .await
    {
        Ok(_) => {}
        Err(error) => log::warn!("[coord] core dictation release failed: {error}"),
    }
}

pub(super) fn handle_trigger_combined(inner: &Arc<Inner>, edge: crate::hotkey::HotkeyCombinedEdge) {
    let result = inner.host.block_on(dispatch(
        inner,
        openless_core::DictationHotkeyEdge::Combined {
            press_id: edge.press_id,
            at: edge.at,
        },
    ));
    match result {
        Ok(_) => {}
        Err(error) => log::warn!("[coord] core dictation combo cancel failed: {error}"),
    }
}

#[cfg(any(debug_assertions, test))]
pub(super) async fn handle_pressed(
    inner: &Arc<Inner>,
    pressed_at: std::time::Instant,
    press_id: u64,
) {
    handle_pressed_edge(inner, pressed_at, press_id).await;
}

#[cfg(any(debug_assertions, test))]
pub(super) async fn handle_released(
    inner: &Arc<Inner>,
    released_at: std::time::Instant,
    press_id: u64,
) {
    handle_released_edge(inner, released_at, press_id).await;
}

pub(super) async fn cancel_active_session(inner: &Arc<Inner>) -> bool {
    match super::hotkey_loops::cancel_active_less_computer(inner).await {
        Ok(true) => return true,
        Ok(false) => {}
        Err(error) => {
            log::warn!("[coord] Less Computer cancel failed: {error}");
            return false;
        }
    }
    match inner.backend.cancel_active_voice_session(None).await {
        Ok(()) => {
            inner.host.hide_less_computer_glow();
            true
        }
        Err(error) if error.code == openless_core::BackendErrorCode::InvalidState => false,
        Err(error) => {
            log::warn!("[coord] core dictation cancel failed: {error}");
            false
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) fn windows_sendinput_options_from_prefs(
    preferences: &crate::types::UserPreferences,
) -> crate::unicode_keystroke::WindowsSendInputOptions {
    crate::unicode_keystroke::WindowsSendInputOptions {
        newline_mode: preferences.windows_sendinput_newline_mode,
    }
}
