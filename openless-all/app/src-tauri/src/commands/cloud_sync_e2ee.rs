//! Main-window-only encrypted sync IPC. Secrets never leave native Core.
use super::CoreState;
use openless_core::cloud_sync_e2ee::*;
use openless_core::{BackendError, BackendErrorCode};

fn require_settings_window(window: &tauri::WebviewWindow) -> Result<(), BackendError> {
    let allowed = window.label() == "main"
        && window.url().is_ok_and(|url| {
            let bundled = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                || (matches!(url.scheme(), "https" | "http")
                    && url.host_str() == Some("tauri.localhost")
                    && url.port().is_none());
            let development = cfg!(debug_assertions)
                && url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
            bundled || development
        });
    if allowed {
        Ok(())
    } else {
        Err(BackendError::new(
            BackendErrorCode::PermissionDenied,
            "encrypted_sync_settings_window_required",
        ))
    }
}

#[tauri::command]
pub async fn cloud_sync_e2ee_claim_setup_prompt(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<bool, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_claim_setup_prompt().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_status(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_status()
}

#[tauri::command]
pub async fn cloud_sync_e2ee_prepare_enable(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    consent_version: String,
) -> Result<EnablePreparation, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_prepare_enable(consent_version).await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_create(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    password: String,
    password_confirmation: String,
    remember_key: bool,
    consent_version: String,
    observed_revision: String,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_create(
        password,
        password_confirmation,
        remember_key,
        consent_version,
        observed_revision,
    )
    .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_unlock(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    password: String,
    remember_key: bool,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_unlock(password, remember_key).await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_lock(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_lock().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_set_enabled(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    enabled: bool,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_set_enabled(enabled).await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_sync_now(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_sync_now().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_cancel(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    task_id: String,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_cancel(task_id)
}

#[tauri::command]
pub async fn cloud_sync_e2ee_preview_restore(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    observed_revision: String,
) -> Result<RestorePreview, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_preview_restore(observed_revision)
        .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_apply_restore(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    preview_id: String,
    mode: RestoreMode,
    conflict_choices: Vec<SyncConflictChoice>,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_apply_restore(preview_id, mode, conflict_choices)
        .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_change_password(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    current_password: String,
    new_password: String,
    confirmation: String,
    remember_key: bool,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_change_password(current_password, new_password, confirmation, remember_key)
        .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_delete_remote(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    expected_vault_id: String,
    observed_revision: String,
    confirmed: bool,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_delete_remote(expected_vault_id, observed_revision, confirmed)
        .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_sign_out(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<EncryptedSyncStatus, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_sign_out().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_begin_sign_in(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<EncryptedSyncSignIn, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_begin_sign_in().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_poll_sign_in(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    authorization_session_id: String,
) -> Result<EncryptedSyncSignInResult, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_poll_sign_in(authorization_session_id)
        .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_cancel_sign_in(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    authorization_session_id: String,
) -> Result<(), BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_cancel_sign_in(authorization_session_id)
        .await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_get_ui_preferences(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<Option<openless_core::CloudSyncUiPreferences>, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_get_ui_preferences().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_get_ui_preferences_snapshot(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
) -> Result<EncryptedUiPreferencesSnapshot, BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_get_ui_preferences_snapshot().await
}

#[tauri::command]
pub async fn cloud_sync_e2ee_set_ui_preferences_checked(
    window: tauri::WebviewWindow,
    core: CoreState<'_>,
    locale: String,
    font_scale: String,
    expected_revision: Option<String>,
) -> Result<(), BackendError> {
    require_settings_window(&window)?;
    core.cloud_sync_e2ee_set_ui_preferences_checked(locale, font_scale, expected_revision)
        .await
}
