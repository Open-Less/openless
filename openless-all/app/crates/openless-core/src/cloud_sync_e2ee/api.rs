use super::OpenLessBackend;
use crate::{cloud_sync_e2ee::*, BackendError};

impl OpenLessBackend {
    pub async fn sign_out_account(&self) -> Result<(), BackendError> {
        if let Some(service) = &self.encrypted_sync {
            service.sign_out().await.map(|_| ())
        } else {
            self.services().marketplace.logout().await
        }
    }
    fn encrypted_sync_service(
        &self,
    ) -> Result<&crate::cloud_sync_e2ee::EncryptedSyncService, BackendError> {
        self.encrypted_sync
            .as_ref()
            .ok_or_else(|| crate::cloud_sync_e2ee::error("service_unavailable"))
    }

    pub fn cloud_sync_e2ee_status(&self) -> Result<EncryptedSyncStatus, BackendError> {
        Ok(self.encrypted_sync_service()?.status())
    }

    pub async fn cloud_sync_e2ee_claim_setup_prompt(&self) -> Result<bool, BackendError> {
        let Some(service) = self.encrypted_sync.as_ref() else {
            return Ok(false);
        };
        let Some(store) = self.encrypted_sync_store.as_ref() else {
            return Ok(false);
        };
        let store = std::sync::Arc::clone(store);
        let settings_gate = std::sync::Arc::clone(&self.settings_write_gate);
        service
            .claim_setup_prompt(
                std::sync::Arc::clone(&self.deps.credential_store),
                move || {
                    let _settings = match settings_gate.try_lock() {
                        Ok(guard) => guard,
                        Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
                        Err(std::sync::TryLockError::Poisoned(_)) => {
                            return Err(crate::cloud_sync_e2ee::error("recovery_required"))
                        }
                    };
                    store
                        .setup_prompt_state()
                        .map_err(crate::cloud_sync_e2ee::document_error)
                },
            )
            .await
    }

    pub async fn cloud_sync_e2ee_prepare_enable(
        &self,
        consent_version: String,
    ) -> Result<EnablePreparation, BackendError> {
        self.encrypted_sync_service()?
            .prepare_enable(consent_version)
            .await
    }

    pub async fn cloud_sync_e2ee_create(
        &self,
        password: String,
        password_confirmation: String,
        remember_key: bool,
        consent_version: String,
        observed_revision: String,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?
            .create(
                password,
                password_confirmation,
                remember_key,
                consent_version,
                observed_revision,
            )
            .await
    }

    pub async fn cloud_sync_e2ee_unlock(
        &self,
        password: String,
        remember_key: bool,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?
            .unlock(password, remember_key)
            .await
    }

    pub async fn cloud_sync_e2ee_lock(&self) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?.lock().await
    }

    pub async fn cloud_sync_e2ee_set_enabled(
        &self,
        enabled: bool,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?.set_enabled(enabled).await
    }

    pub async fn cloud_sync_e2ee_sync_now(&self) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?.sync_now().await
    }

    pub fn cloud_sync_e2ee_cancel(
        &self,
        task_id: String,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?.cancel(&task_id)
    }

    pub async fn cloud_sync_e2ee_preview_restore(
        &self,
        observed_revision: String,
    ) -> Result<RestorePreview, BackendError> {
        self.encrypted_sync_service()?
            .preview_restore(observed_revision)
            .await
    }

    pub async fn cloud_sync_e2ee_apply_restore(
        &self,
        preview_id: String,
        mode: RestoreMode,
        conflict_choices: Vec<SyncConflictChoice>,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?
            .apply_restore(preview_id, mode, conflict_choices)
            .await
    }

    pub async fn cloud_sync_e2ee_change_password(
        &self,
        current_password: String,
        new_password: String,
        confirmation: String,
        remember_key: bool,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?
            .change_password(current_password, new_password, confirmation, remember_key)
            .await
    }

    pub async fn cloud_sync_e2ee_delete_remote(
        &self,
        expected_vault_id: String,
        observed_revision: String,
        confirmed: bool,
    ) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?
            .delete_remote(expected_vault_id, observed_revision, confirmed)
            .await
    }

    pub async fn cloud_sync_e2ee_sign_out(&self) -> Result<EncryptedSyncStatus, BackendError> {
        self.encrypted_sync_service()?.sign_out().await
    }

    pub async fn cloud_sync_e2ee_begin_sign_in(&self) -> Result<EncryptedSyncSignIn, BackendError> {
        self.encrypted_sync_service()?.begin_sign_in().await
    }

    pub async fn cloud_sync_e2ee_poll_sign_in(
        &self,
        authorization_session_id: String,
    ) -> Result<EncryptedSyncSignInResult, BackendError> {
        self.encrypted_sync_service()?
            .poll_sign_in(authorization_session_id)
            .await
    }

    pub async fn cloud_sync_e2ee_cancel_sign_in(
        &self,
        authorization_session_id: String,
    ) -> Result<(), BackendError> {
        self.encrypted_sync_service()?
            .cancel_sign_in(authorization_session_id)
            .await
    }

    pub async fn cloud_sync_e2ee_set_ui_preferences(
        &self,
        locale: String,
        font_scale: String,
    ) -> Result<(), BackendError> {
        let store = self
            .encrypted_sync_store
            .as_ref()
            .ok_or_else(|| crate::cloud_sync_e2ee::error("service_unavailable"))?;
        store
            .set_ui_preferences(crate::cloud_sync_e2ee_documents::ui_preferences(
                &locale,
                &font_scale,
            ))
            .await
            .map_err(crate::cloud_sync_e2ee::document_error)
    }

    pub async fn cloud_sync_e2ee_get_ui_preferences(
        &self,
    ) -> Result<Option<crate::CloudSyncUiPreferences>, BackendError> {
        self.encrypted_sync_service()?.ui_preferences().await
    }

    pub async fn cloud_sync_e2ee_get_ui_preferences_snapshot(
        &self,
    ) -> Result<EncryptedUiPreferencesSnapshot, BackendError> {
        self.encrypted_sync_service()?
            .ui_preferences_snapshot()
            .await
    }

    pub async fn cloud_sync_e2ee_set_ui_preferences_checked(
        &self,
        locale: String,
        font_scale: String,
        expected_revision: Option<String>,
    ) -> Result<(), BackendError> {
        let store = self
            .encrypted_sync_store
            .as_ref()
            .ok_or_else(|| crate::cloud_sync_e2ee::error("service_unavailable"))?;
        store
            .set_ui_preferences_checked(
                crate::cloud_sync_e2ee_documents::ui_preferences(&locale, &font_scale),
                expected_revision,
            )
            .await
            .map_err(crate::cloud_sync_e2ee::document_error)
    }
}
