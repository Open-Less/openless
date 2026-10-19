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

pub(super) fn install_enter_stop_provider(inner: &Arc<Inner>) {
    let weak = Arc::downgrade(inner);
    crate::hotkey::set_enter_stop_provider(Arc::new(move || {
        weak.upgrade().and_then(|inner| enter_stop_session(&inner))
    }));
}

pub(super) fn enter_stop_session(inner: &Arc<Inner>) -> Option<openless_core::SessionId> {
    let preferences = inner.backend.get_preferences();
    if inner.shutdown.load(std::sync::atomic::Ordering::SeqCst)
        || inner
            .shortcut_recording_active
            .load(std::sync::atomic::Ordering::SeqCst)
        || !preferences.enter_stop_enabled
        || preferences.hotkey.mode != crate::types::HotkeyMode::Toggle
        || matches!(
            inner.backend.dictation_output_target(),
            Some(
                openless_core::DictationOutputTarget::VoiceEdit
                    | openless_core::DictationOutputTarget::Qa
            )
        )
    {
        return None;
    }
    let dictation = inner.backend.snapshot().dictation;
    matches!(
        dictation.phase,
        openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
    )
    .then_some(dictation.session_id)
    .flatten()
}

pub(super) async fn handle_enter_stop(inner: &Arc<Inner>, session_id: openless_core::SessionId) {
    if enter_stop_session(inner) != Some(session_id) {
        return;
    }
    if let Err(error) = inner.backend.stop_dictation_session(session_id).await {
        if !matches!(
            error.code,
            openless_core::BackendErrorCode::Busy
                | openless_core::BackendErrorCode::InvalidState
                | openless_core::BackendErrorCode::Cancelled
        ) {
            log::warn!("[coord] Enter stop failed: {error}");
        }
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

#[cfg(test)]
mod enter_stop_tests {
    use super::*;
    use crate::coordinator::hotkey_loops::less_computer_test_support::fixture_coordinator;
    use crate::types::HotkeyMode;
    use openless_core::{DictationOutputTarget, DictationPhase, DictationStartOptions, SessionId};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    fn enable_enter_stop(inner: &Arc<Inner>) {
        let mut preferences = inner.backend.get_preferences();
        preferences.enter_stop_enabled = true;
        crate::set_backend_preferences_for_test(&inner.backend, preferences);
    }

    #[tokio::test]
    async fn enter_is_opt_in_and_disabling_it_applies_to_an_active_recording() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(HotkeyMode::Toggle, Duration::ZERO);
        let inner = &coordinator.inner;
        inner.backend.start().await.unwrap();
        assert!(!inner.backend.get_preferences().enter_stop_enabled);
        let session_id = inner.backend.start_dictation().await.unwrap();
        assert_eq!(enter_stop_session(inner), None);
        handle_enter_stop(inner, session_id).await;
        assert_eq!(recorder.stop_count(), 0);
        enable_enter_stop(inner);
        assert_eq!(enter_stop_session(inner), Some(session_id));
        let mut preferences = inner.backend.get_preferences();
        preferences.enter_stop_enabled = false;
        crate::set_backend_preferences_for_test(&inner.backend, preferences);
        assert_eq!(enter_stop_session(inner), None);
        handle_enter_stop(inner, session_id).await;
        assert_eq!(recorder.stop_count(), 0);
        inner.backend.cancel_dictation(None).await.unwrap();
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn enter_finishes_toggle_once_and_never_starts_a_recording() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(HotkeyMode::Toggle, Duration::ZERO);
        let inner = &coordinator.inner;
        enable_enter_stop(inner);
        inner.backend.start().await.unwrap();
        assert_eq!(enter_stop_session(inner), None);
        handle_enter_stop(inner, SessionId::new()).await;
        assert_eq!(recorder.stop_count(), 0);

        let session_id = inner.backend.start_dictation().await.unwrap();
        assert_eq!(enter_stop_session(inner), Some(session_id));
        handle_enter_stop(inner, session_id).await;
        handle_enter_stop(inner, session_id).await;
        assert_eq!(recorder.stop_count(), 1);
        assert_eq!(inner.backend.list_history().unwrap().len(), 1);
        assert_eq!(enter_stop_session(inner), None);
        assert!(!matches!(
            inner.backend.snapshot().dictation.phase,
            DictationPhase::Starting | DictationPhase::Recording
        ));
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn stale_enter_cannot_stop_a_new_toggle_session() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(HotkeyMode::Toggle, Duration::ZERO);
        let inner = &coordinator.inner;
        enable_enter_stop(inner);
        inner.backend.start().await.unwrap();
        let first = inner.backend.start_dictation().await.unwrap();
        inner.backend.cancel_dictation(None).await.unwrap();
        let second = inner.backend.start_dictation().await.unwrap();
        let stopped = recorder.stop_count();
        handle_enter_stop(inner, first).await;
        assert_eq!(recorder.stop_count(), stopped);
        assert_eq!(
            inner.backend.snapshot().dictation.phase,
            DictationPhase::Recording
        );
        assert_eq!(enter_stop_session(inner), Some(second));
        handle_enter_stop(inner, second).await;
        assert_eq!(recorder.stop_count(), stopped + 1);
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn enter_does_not_stop_other_recording_modes() {
        for mode in [HotkeyMode::Hold, HotkeyMode::Auto, HotkeyMode::DoubleClick] {
            let (coordinator, recorder, data_dir) = fixture_coordinator(mode, Duration::ZERO);
            let inner = &coordinator.inner;
            enable_enter_stop(inner);
            inner.backend.start().await.unwrap();
            let session_id = inner.backend.start_dictation().await.unwrap();
            assert_eq!(enter_stop_session(inner), None);
            handle_enter_stop(inner, session_id).await;
            assert_eq!(recorder.stop_count(), 0);
            assert_eq!(
                inner.backend.snapshot().dictation.phase,
                DictationPhase::Recording
            );
            assert!(cancel_active_session(inner).await);
            drop(coordinator);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }

    #[tokio::test]
    async fn shortcut_capture_and_voice_edit_do_not_offer_enter_stop() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(HotkeyMode::Toggle, Duration::ZERO);
        let inner = &coordinator.inner;
        enable_enter_stop(inner);
        inner.backend.start().await.unwrap();
        let session_id = inner.backend.start_dictation().await.unwrap();
        inner.shortcut_recording_active.store(true, Ordering::SeqCst);
        assert_eq!(enter_stop_session(inner), None);
        handle_enter_stop(inner, session_id).await;
        assert_eq!(recorder.stop_count(), 0);
        inner.shortcut_recording_active.store(false, Ordering::SeqCst);
        assert_eq!(enter_stop_session(inner), Some(session_id));
        inner.backend.cancel_dictation(None).await.unwrap();

        for output_target in [DictationOutputTarget::VoiceEdit, DictationOutputTarget::Qa] {
            let session_id = inner
                .backend
                .start_dictation_with_options(DictationStartOptions {
                    insert_text: false,
                    output_target,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert_eq!(enter_stop_session(inner), None);
            handle_enter_stop(inner, session_id).await;
            assert_eq!(
                inner.backend.snapshot().dictation.phase,
                DictationPhase::Recording
            );
            inner.backend.cancel_dictation(None).await.unwrap();
        }
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn enter_can_finish_the_session_it_captured_during_starting() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(HotkeyMode::Toggle, Duration::from_millis(100));
        let inner = &coordinator.inner;
        enable_enter_stop(inner);
        inner.backend.start().await.unwrap();
        let backend = Arc::clone(&inner.backend);
        let start = tokio::spawn(async move { backend.start_dictation().await });
        let session_id = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(id) = enter_stop_session(inner) {
                    break id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            inner.backend.snapshot().dictation.phase,
            DictationPhase::Starting
        );
        handle_enter_stop(inner, session_id).await;
        assert_eq!(start.await.unwrap().unwrap(), session_id);
        assert_eq!(recorder.stop_count(), 1);
        assert_eq!(inner.backend.list_history().unwrap().len(), 1);
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
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
