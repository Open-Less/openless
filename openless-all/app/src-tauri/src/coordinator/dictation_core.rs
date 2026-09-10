use std::sync::Arc;

use super::{qa::handle_qa_option_edge, Inner};

const RECORDING_RECOVERY_PROMPT_DELAY_MS: u64 = 120;
const RECORDING_RECOVERY_TIMEOUT_MS: u64 = 3_000;

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
    dismiss_cancelled_recording_recovery(inner, None);
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

pub(super) async fn cancel_active_session_after_escape(inner: &Arc<Inner>) -> bool {
    match super::hotkey_loops::cancel_active_less_computer(inner).await {
        Ok(true) => return true,
        Ok(false) => {}
        Err(error) => {
            log::warn!("[coord] Less Computer cancel failed: {error}");
            return false;
        }
    }
    let snapshot = inner.backend.snapshot();
    let Some(active_session_id) = snapshot.dictation.session_id else {
        return match inner.backend.cancel_active_voice_session(None).await {
            Ok(()) => true,
            Err(error) if error.code == openless_core::BackendErrorCode::InvalidState => false,
            Err(error) => {
                log::warn!("[coord] voice cancel after Esc failed: {error}");
                false
            }
        };
    };
    let pending_session_id = active_session_id.to_string();
    let recovery_armed = inner
        .backend
        .get_preferences()
        .esc_recording_recovery_enabled
        && matches!(
            snapshot.dictation.phase,
            openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
        );
    if recovery_armed {
        *inner.cancelled_recording_recovery.lock() = Some(pending_session_id.clone());
        crate::hotkey::set_esc_exclusive(true);
    }
    match inner.backend.cancel_dictation_after_escape(None).await {
        Ok(Some(recovery)) => {
            inner.host.hide_less_computer_glow();
            let session_id = recovery.session_id.to_string();
            let should_schedule = should_schedule_cancelled_recording_recovery(inner, &session_id);
            if should_schedule {
                schedule_cancelled_recording_recovery_prompt(
                    inner,
                    session_id,
                    recovery.duration_ms,
                );
            } else {
                dismiss_cancelled_recording_recovery(inner, Some(&session_id));
            }
            true
        }
        Ok(None) => {
            inner.host.hide_less_computer_glow();
            if recovery_armed {
                dismiss_cancelled_recording_recovery(inner, Some(&pending_session_id));
            }
            true
        }
        Err(error) => {
            if recovery_armed {
                dismiss_cancelled_recording_recovery(inner, Some(&pending_session_id));
            }
            log::warn!("[coord] dictation cancel after Esc failed: {error}");
            false
        }
    }
}

fn should_schedule_cancelled_recording_recovery(inner: &Arc<Inner>, session_id: &str) -> bool {
    inner
        .backend
        .get_preferences()
        .esc_recording_recovery_enabled
        && inner.cancelled_recording_recovery.lock().as_deref() == Some(session_id)
}

pub(super) fn dismiss_cancelled_recording_recovery(
    inner: &Arc<Inner>,
    session_id: Option<&str>,
) -> bool {
    let dismissed = {
        let mut pending = inner.cancelled_recording_recovery.lock();
        match (pending.as_deref(), session_id) {
            (Some(active), Some(requested)) if active != requested => false,
            (Some(_), _) => {
                pending.take();
                true
            }
            (None, _) => false,
        }
    };
    if dismissed {
        super::capsule_focus::hide_capsule_if_all_sessions_idle(inner);
    }
    dismissed
}

fn schedule_cancelled_recording_recovery_prompt(
    inner: &Arc<Inner>,
    session_id: String,
    elapsed_ms: u64,
) {
    let inner = Arc::clone(inner);
    let spawner = inner.host.clone();
    spawner.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(
            RECORDING_RECOVERY_PROMPT_DELAY_MS,
        ))
        .await;
        let still_pending =
            inner.cancelled_recording_recovery.lock().as_deref() == Some(session_id.as_str());
        if !still_pending {
            return;
        }
        if !inner
            .backend
            .get_preferences()
            .esc_recording_recovery_enabled
        {
            dismiss_cancelled_recording_recovery(&inner, Some(&session_id));
            return;
        }
        if inner.backend.snapshot().dictation.phase != openless_core::DictationPhase::Idle {
            return;
        }
        let epoch = super::capsule_focus::emit_core_capsule(
            &inner,
            crate::types::CapsulePayload {
                state: crate::types::CapsuleState::Cancelled,
                level: 0.0,
                elapsed_ms,
                message: None,
                inserted_chars: None,
                translation: false,
                operating: false,
                warming: false,
                capsule_style: inner.backend.get_preferences().capsule_style,
                selection_polish: false,
                recovery_session_id: Some(session_id.clone()),
            },
            None,
        );
        tokio::time::sleep(std::time::Duration::from_millis(
            RECORDING_RECOVERY_TIMEOUT_MS,
        ))
        .await;
        let still_pending =
            inner.cancelled_recording_recovery.lock().as_deref() == Some(session_id.as_str());
        if still_pending {
            *inner.cancelled_recording_recovery.lock() = None;
            if let Some(epoch) = epoch {
                super::capsule_focus::hide_core_capsule_if_current(&inner, epoch);
            }
        }
    });
}

#[cfg(target_os = "windows")]
pub(super) fn windows_sendinput_options_from_prefs(
    preferences: &crate::types::UserPreferences,
) -> crate::unicode_keystroke::WindowsSendInputOptions {
    crate::unicode_keystroke::WindowsSendInputOptions {
        newline_mode: preferences.windows_sendinput_newline_mode,
    }
}

#[cfg(all(test, target_os = "windows"))]
mod recording_recovery_tests {
    use super::*;

    #[tokio::test]
    async fn escape_arms_recovery_before_async_cancellation_finishes() {
        let (coordinator, _, data_dir) =
            super::super::hotkey_loops::windows_less_computer_tests::fixture_coordinator(
                crate::types::HotkeyMode::Toggle,
                std::time::Duration::from_millis(100),
            );
        let inner = &coordinator.inner;
        let mut preferences = inner.backend.get_preferences();
        preferences.esc_recording_recovery_enabled = true;
        crate::set_backend_preferences_for_test(&inner.backend, preferences);
        inner.backend.start().await.unwrap();
        let backend = Arc::clone(&inner.backend);
        let starting = tokio::spawn(async move { backend.start_dictation().await });
        while inner.backend.snapshot().dictation.session_id.is_none() {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let session_id = inner
            .backend
            .snapshot()
            .dictation
            .session_id
            .unwrap()
            .to_string();

        let mut cancelling = Box::pin(cancel_active_session_after_escape(inner));
        assert!(futures_util::poll!(cancelling.as_mut()).is_pending());
        assert_eq!(
            inner.cancelled_recording_recovery.lock().as_deref(),
            Some(session_id.as_str())
        );

        assert!(cancelling.await);
        assert_eq!(
            starting.await.unwrap().unwrap_err().code,
            openless_core::BackendErrorCode::Cancelled
        );
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn second_escape_and_disabled_setting_prevent_a_late_recovery_prompt() {
        for disable_setting in [false, true] {
            let (coordinator, _, data_dir) =
                super::super::hotkey_loops::windows_less_computer_tests::fixture_coordinator(
                    crate::types::HotkeyMode::Toggle,
                    std::time::Duration::ZERO,
                );
            let inner = &coordinator.inner;
            let session_id = openless_core::SessionId::new().to_string();
            let mut preferences = inner.backend.get_preferences();
            preferences.esc_recording_recovery_enabled = true;
            crate::set_backend_preferences_for_test(&inner.backend, preferences);
            *inner.cancelled_recording_recovery.lock() = Some(session_id.clone());
            assert!(should_schedule_cancelled_recording_recovery(
                inner,
                &session_id
            ));

            if disable_setting {
                let mut preferences = inner.backend.get_preferences();
                preferences.esc_recording_recovery_enabled = false;
                crate::set_backend_preferences_for_test(&inner.backend, preferences);
            } else {
                assert!(dismiss_cancelled_recording_recovery(inner, None));
            }
            assert!(!should_schedule_cancelled_recording_recovery(
                inner,
                &session_id
            ));
            drop(coordinator);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }
}
