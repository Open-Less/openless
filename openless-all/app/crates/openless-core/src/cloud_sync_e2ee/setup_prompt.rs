//! One local claim after a usable pipeline is configured. This path never connects
//! to OAuth/sync servers, exports private documents, or enables synchronization.
use super::*;
use crate::{CredentialStore, CredentialsStatus, UserPreferences};

type Eligibility = Option<(UserPreferences, (Revision, u64))>;

fn configured(preferences: &UserPreferences, status: &CredentialsStatus) -> bool {
    match crate::shared_types::effective_pipeline_mode(
        preferences.multimodal_pipeline_enabled,
        preferences.pipeline_mode,
    ) {
        crate::shared_types::PipelineMode::Traditional => {
            status.asr_configured && status.llm_configured
        }
        crate::shared_types::PipelineMode::Multimodal => status.omni_configured,
    }
}

impl EncryptedSyncService {
    pub(crate) async fn claim_setup_prompt(
        &self,
        credentials: Arc<dyn CredentialStore>,
        eligibility: impl Fn() -> SyncResult<Eligibility> + Send + Sync,
    ) -> SyncResult<bool> {
        let mut runtime = match self.0.runtime.try_lock() {
            Ok(runtime) => runtime,
            Err(_) => return Ok(false),
        };
        if self.0.shutdown.load(Ordering::Acquire) {
            return Ok(false);
        }
        let Some((preferences, stamp)) = eligibility()? else {
            return Ok(false);
        };
        self.initialize(&mut runtime).await?;
        if runtime.settings.enabled || runtime.settings.prompted {
            return Ok(false);
        }
        let status = credentials.status(preferences.clone()).await?;
        if !configured(&preferences, &status) {
            return Ok(false);
        }
        let Some((_, current)) = eligibility()? else {
            return Ok(false);
        };
        if current != stamp || self.0.shutdown.load(Ordering::Acquire) {
            return Ok(false);
        }
        // Publish the in-memory claim only after durable encrypted persistence succeeds.
        // Read errors and busy/changing inputs do not consume the installation's prompt.
        let mut settings = runtime.settings.clone();
        settings.prompted = true;
        self.0
            .local
            .write("device", "client", settings.clone())
            .await?;
        runtime.settings = settings;
        Ok(true)
    }
}
