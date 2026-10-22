use super::*;

#[tauri::command]
pub async fn get_startup_snapshot(
    core: CoreState<'_>,
) -> Result<openless_core::StartupSnapshot, String> {
    core.start().await.map_err(|error| error.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn get_capsule_snapshot(
    coord: CoordinatorState<'_>,
) -> Result<Option<crate::tauri_coordinator_host::CapsuleSnapshot>, String> {
    Ok(coord.tauri_host().capsule_snapshot())
}

async fn ensure_core_started(core: &openless_core::OpenLessBackend) -> Result<(), String> {
    if !core.snapshot().running {
        core.start().await.map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn start_dictation(core: CoreState<'_>) -> Result<(), String> {
    ensure_core_started(&core).await?;
    core.start_dictation()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_dictation(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    ensure_core_started(&core).await?;
    if coord.stop_less_computer_recording().await? {
        return Ok(());
    }
    core.stop_dictation()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_dictation(
    core: CoreState<'_>,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    ensure_core_started(&core).await?;
    coord.cancel_active_voice().await;
    Ok(())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn set_capsule_transcript_visible(
    coord: CoordinatorState<'_>,
    visible: bool,
) -> Result<(), String> {
    let host = coord.tauri_host();
    let Some(capsule) = host.capsule_window() else {
        return Ok(());
    };
    host.run_on_main_thread(move || {
        if let Err(error) = capsule.set_transcript_visible(visible) {
            log::warn!("[capsule] transcript visibility layout failed: {error}");
        }
    })
}

#[tauri::command]
pub async fn handle_window_hotkey_event(
    coord: CoordinatorState<'_>,
    event_type: String,
    key: String,
    code: String,
    repeat: bool,
) -> Result<(), String> {
    coord
        .handle_window_hotkey_event(event_type, key, code, repeat)
        .await
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn inject_hotkey_click_for_dev(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.inject_hotkey_click_for_dev().await
}

/// Omitting `style_pack_id` = use the currently active style pack (history
/// page "retry"); passing an id = trial-polish once with that style pack
/// (history page "re-polish with another style") without changing the active
/// one.
#[tauri::command]
pub async fn repolish(
    core: CoreState<'_>,
    raw_text: String,
    mode: PolishMode,
    style_pack_id: Option<String>,
) -> Result<String, String> {
    log::info!(
        "[style-pack] command repolish requested legacy_mode={:?} raw_chars={} style_pack_id={:?}",
        mode,
        raw_text.chars().count(),
        style_pack_id
    );
    core.services()
        .auxiliary
        .repolish(openless_core::RepolishRequest {
            raw_text,
            style_pack_id,
            front_app: crate::coordinator::capture_frontmost_app(),
        })
        .await
        .map_err(|error| error.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn get_dictation_translation_target(
    core: CoreState<'_>,
) -> Result<Option<openless_core::DictationTranslationTarget>, String> {
    Ok(core.dictation_translation_target())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn select_dictation_translation_target(
    core: CoreState<'_>,
    session_id: openless_core::SessionId,
    target_language: String,
) -> Result<openless_core::DictationTranslationTarget, String> {
    core.select_dictation_translation_target(session_id, target_language)
        .map_err(|error| error.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
pub async fn set_capsule_language_menu(
    window: Window,
    coord: CoordinatorState<'_>,
    session_id: openless_core::SessionId,
    open: bool,
) -> Result<(), String> {
    let label = window.label().to_string();
    if label != "capsule" && !label.starts_with("capsule-mirror-") {
        return Err("capsule_window_required".into());
    }
    let core = coord.backend();
    let capsule = coord
        .tauri_host()
        .capsule_window()
        .ok_or("capsule_window_unavailable")?;
    let (send, receive) = tokio::sync::oneshot::channel();
    capsule.run_on_main_thread(move |capsule| {
        if open
            && !core
                .dictation_translation_target()
                .is_some_and(|active| active.mutable && active.session_id == session_id)
        {
            let _ = send.send(Err("recording_translation_required".into()));
            return;
        }
        let _ = send.send(capsule.set_translation_menu_open(label, session_id, open));
    })?;
    receive
        .await
        .map_err(|_| "capsule_menu_cancelled".to_string())?
}
