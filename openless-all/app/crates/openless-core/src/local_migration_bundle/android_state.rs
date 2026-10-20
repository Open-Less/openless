use crate::cloud_sync_e2ee_documents::{DocumentResult, SecretJson};

/// Live Android state is owned by the Host, not by the Activity or a cached sync extension.
pub struct AndroidMigrationSnapshot {
    pub revision: String,
    pub value: SecretJson,
}

pub trait AndroidMigrationState: Send + Sync {
    fn capture(&self) -> DocumentResult<AndroidMigrationSnapshot>;
    fn acquire(
        &self,
        expected_revision: Option<&str>,
    ) -> DocumentResult<Box<dyn AndroidMigrationLease>>;
}

pub trait AndroidMigrationLease: Send {
    fn capture(&self) -> DocumentResult<SecretJson>;
    fn replace(&mut self, value: &SecretJson) -> DocumentResult<()>;
    /// A failed recovery keeps ordinary Android writers fenced until startup recovery succeeds.
    fn finish(&mut self, recovery_required: bool) -> DocumentResult<()>;
}

pub(crate) const ANDROID_SETTINGS_ID: &str = "androidPortableSettings";
