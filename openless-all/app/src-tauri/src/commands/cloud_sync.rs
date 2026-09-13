//! Cloud operations delegate to Core so browser inputs never contain provider or GitHub credentials.

use super::CoreState;
use openless_core::{
    BackendError, CloudSyncRestoreResult, CloudSyncStatus, CloudSyncUiPreferences,
};

#[tauri::command]
pub async fn cloud_sync_status(core: CoreState<'_>) -> Result<CloudSyncStatus, BackendError> {
    core.cloud_sync_status().await
}

#[tauri::command]
pub async fn cloud_sync_upload(
    core: CoreState<'_>,
    base_revision: u64,
    ui_preferences: CloudSyncUiPreferences,
) -> Result<CloudSyncStatus, BackendError> {
    core.cloud_sync_upload(base_revision, ui_preferences).await
}

#[tauri::command]
pub async fn cloud_sync_restore(
    core: CoreState<'_>,
) -> Result<CloudSyncRestoreResult, BackendError> {
    core.cloud_sync_restore().await
}

#[tauri::command]
pub async fn cloud_sync_delete(
    core: CoreState<'_>,
    base_revision: u64,
) -> Result<CloudSyncStatus, BackendError> {
    core.cloud_sync_delete(base_revision).await
}
