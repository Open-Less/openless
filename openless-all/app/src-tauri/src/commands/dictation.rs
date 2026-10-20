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
