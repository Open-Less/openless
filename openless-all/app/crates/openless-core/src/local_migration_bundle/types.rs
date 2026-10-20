use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::cloud_sync_e2ee_documents::{DocumentError, ValidatedSyncDocuments};

pub const BUNDLE_FORMAT: &str = "openless.android.migration-bundle";
pub const BUNDLE_FORMAT_VERSION: u32 = 1;
pub const BUNDLE_DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_BUNDLE_COMPRESSED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_BUNDLE_UNCOMPRESSED_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_BUNDLE_ENTRY_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_BUNDLE_ENTRIES: usize = 128;
pub const MAX_PASSWORD_BYTES: usize = 4096;
pub const MAX_CREDENTIAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleMode {
    ShareRedacted,
    PersonalMigration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPolicy {
    Omit,
    PasswordEncrypted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreMode {
    Merge,
    Replace,
}

#[derive(Clone)]
pub struct BundlePassword(Zeroizing<Vec<u8>>);

impl BundlePassword {
    pub fn new(password: impl AsRef<[u8]>) -> BundleResult<Self> {
        let bytes = password.as_ref();
        if bytes.is_empty() || bytes.len() > MAX_PASSWORD_BYTES {
            return Err(BundleError::InvalidPassword);
        }
        Ok(Self(Zeroizing::new(bytes.to_vec())))
    }

    pub(crate) fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for BundlePassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BundlePassword([REDACTED])")
    }
}

impl Drop for BundlePassword {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Clone)]
pub struct BundleExportOptions {
    pub mode: BundleMode,
    pub credential_policy: CredentialPolicy,
    pub password: Option<BundlePassword>,
    pub app_version: String,
    /// Android-local, non-secret preferences that are outside Core repositories.
    /// The value is validated against the migration allowlist before it enters the bundle.
    pub portable_android_preferences: Option<serde_json::Value>,
}

impl Default for BundleExportOptions {
    fn default() -> Self {
        Self {
            mode: BundleMode::ShareRedacted,
            credential_policy: CredentialPolicy::Omit,
            password: None,
            app_version: "unknown".into(),
            portable_android_preferences: None,
        }
    }
}

impl BundleExportOptions {
    pub(crate) fn validate(&self) -> BundleResult<()> {
        if self.app_version.is_empty() || self.app_version.len() > 128 {
            return Err(BundleError::InvalidOptions);
        }
        if matches!(self.mode, BundleMode::ShareRedacted)
            && !matches!(self.credential_policy, CredentialPolicy::Omit)
        {
            return Err(BundleError::InvalidOptions);
        }
        if matches!(self.credential_policy, CredentialPolicy::PasswordEncrypted)
            && self.password.is_none()
        {
            return Err(BundleError::PasswordRequired);
        }
        if matches!(self.mode, BundleMode::ShareRedacted) && self.password.is_some() {
            return Err(BundleError::InvalidOptions);
        }
        if let Some(value) = &self.portable_android_preferences {
            validate_portable_android_preferences(value)?;
        }
        Ok(())
    }
}

const PORTABLE_ANDROID_PREFERENCE_KEYS: &[&str] = &[
    "schemaVersion",
    "learningPrivacyVersion",
    "preferences",
    "strokeFrequency",
    "pinyinLearnedPhrases",
];
const PORTABLE_ANDROID_SETTINGS_KEYS: &[&str] = &[
    "locale",
    "themeMode",
    "hapticEnabled",
    "hapticAmplitude",
    "hapticDurationMs",
    "englishSuggestionsEnabled",
    "debugFeaturesEnabled",
];

pub(crate) fn validate_portable_android_preferences(value: &serde_json::Value) -> BundleResult<()> {
    let object = value.as_object().ok_or(BundleError::InvalidSchema)?;
    if object
        .get("learningPrivacyVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err(BundleError::InvalidSchema);
    }
    if object
        .keys()
        .any(|key| !PORTABLE_ANDROID_PREFERENCE_KEYS.contains(&key.as_str()))
        || object
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
    {
        return Err(BundleError::InvalidSchema);
    }
    for key in ["preferences", "strokeFrequency", "pinyinLearnedPhrases"] {
        if !object.contains_key(key) {
            return Err(BundleError::InvalidSchema);
        }
    }
    let preferences = object
        .get("preferences")
        .ok_or(BundleError::InvalidSchema)?;
    let preferences = preferences.as_object().ok_or(BundleError::InvalidSchema)?;
    if let Some(locale) = preferences
        .get("locale")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(
            locale,
            "system" | "zh-CN" | "zh-TW" | "en" | "ja" | "ko" | "es" | "fr" | "de"
        ) {
            return Err(BundleError::InvalidSchema);
        }
    }
    if let Some(theme) = preferences
        .get("themeMode")
        .and_then(serde_json::Value::as_str)
    {
        if !matches!(theme, "system" | "light" | "dark") {
            return Err(BundleError::InvalidSchema);
        }
    }
    if preferences
        .keys()
        .any(|key| !PORTABLE_ANDROID_SETTINGS_KEYS.contains(&key.as_str()))
    {
        return Err(BundleError::InvalidSchema);
    }
    if preferences
        .get("locale")
        .is_some_and(|value| !value.is_string())
        || preferences
            .get("themeMode")
            .is_some_and(|value| !value.is_string())
        || preferences
            .get("hapticEnabled")
            .is_some_and(|value| !value.is_boolean())
        || preferences
            .get("englishSuggestionsEnabled")
            .is_some_and(|value| !value.is_boolean())
        || preferences
            .get("debugFeaturesEnabled")
            .is_some_and(|value| !value.is_boolean())
        || preferences.get("hapticAmplitude").is_some_and(|value| {
            value
                .as_u64()
                .is_none_or(|value| !(1..=255).contains(&value))
        })
        || preferences.get("hapticDurationMs").is_some_and(|value| {
            value
                .as_u64()
                .is_none_or(|value| !(1..=1000).contains(&value))
        })
    {
        return Err(BundleError::InvalidSchema);
    }
    for key in ["strokeFrequency", "pinyinLearnedPhrases"] {
        if let Some(entries) = object.get(key) {
            let entries = entries.as_object().ok_or(BundleError::InvalidSchema)?;
            if entries.len() > 20_000
                || entries.iter().any(|(entry, value)| {
                    entry.is_empty() || entry.len() > 256 || !value.is_string()
                })
            {
                return Err(BundleError::InvalidSchema);
            }
        }
    }
    if serde_json::to_vec(value)
        .map_err(|_| BundleError::InvalidSchema)?
        .len()
        > 4 * 1024 * 1024
    {
        return Err(BundleError::TooLarge);
    }
    Ok(())
}

#[derive(Debug)]
pub struct BundleImportOptions {
    pub mode: RestoreMode,
    pub password: Option<BundlePassword>,
}

impl Default for BundleImportOptions {
    fn default() -> Self {
        Self {
            mode: RestoreMode::Merge,
            password: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleInspection {
    pub format: String,
    pub format_version: u32,
    pub document_schema_version: u32,
    pub bundle_id: String,
    pub created_at: String,
    pub platform: String,
    pub mode: BundleMode,
    pub document_count: usize,
    pub tombstone_count: usize,
    pub section_counts: SectionCounts,
    pub skipped_history: usize,
    pub skipped_activity: usize,
    pub skipped_tombstones: usize,
    pub credentials_present: bool,
    pub credentials_encrypted: bool,
    pub credentials_unlocked: bool,
    pub permissions_migrated: bool,
    pub device_profile_migrated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionCounts {
    pub preferences: usize,
    pub ui_preferences: usize,
    pub channels: usize,
    pub dictionary: usize,
    pub vocabulary_presets: usize,
    pub corrections: usize,
    pub style_packs: usize,
    pub history: usize,
    pub activity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleMergePreview {
    pub inspection: BundleInspection,
    pub observed_generation: crate::cloud_sync_e2ee_documents::Revision,
    pub observed_android_revision: Option<String>,
    pub removed_documents: Vec<crate::cloud_sync_e2ee_documents::DocumentKey>,
    pub conflicts: Vec<crate::cloud_sync_e2ee_documents::RedactedConflict>,
}

#[derive(Debug)]
pub struct BundleArtifact {
    pub bytes: Zeroizing<Vec<u8>>,
    pub inspection: BundleInspection,
}

#[derive(Debug)]
pub struct DecodedBundle {
    pub inspection: BundleInspection,
    pub documents: ValidatedSyncDocuments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    #[error("migration_bundle_invalid_options")]
    InvalidOptions,
    #[error("migration_bundle_unsupported_version")]
    UnsupportedVersion,
    #[error("migration_bundle_malformed_container")]
    MalformedContainer,
    #[error("migration_bundle_too_large")]
    TooLarge,
    #[error("migration_bundle_digest_mismatch")]
    DigestMismatch,
    #[error("migration_bundle_password_required")]
    PasswordRequired,
    #[error("migration_bundle_invalid_password")]
    InvalidPassword,
    #[error("migration_bundle_invalid_schema")]
    InvalidSchema,
    #[error("migration_bundle_resource_invalid")]
    InvalidResource,
    #[error("migration_bundle_backup_failed")]
    BackupFailed,
    #[error("migration_bundle_stale_preview")]
    StalePreview,
    #[error("migration_bundle_restore_rolled_back")]
    RestoreRolledBack,
    #[error("migration_bundle_recovery_required")]
    RecoveryRequired,
    #[error("migration_bundle_document_error")]
    Document(DocumentError),
}

impl From<DocumentError> for BundleError {
    fn from(value: DocumentError) -> Self {
        match value {
            DocumentError::PayloadTooLarge => Self::TooLarge,
            DocumentError::Unsupported => Self::UnsupportedVersion,
            DocumentError::RestoreRolledBack => Self::RestoreRolledBack,
            DocumentError::RecoveryRequired => Self::RecoveryRequired,
            DocumentError::StalePreview => Self::StalePreview,
            other => Self::Document(other),
        }
    }
}

pub type BundleResult<T> = Result<T, BundleError>;

pub(crate) struct BundleEntries {
    pub(crate) manifest: Vec<u8>,
    pub(crate) documents: Vec<u8>,
    pub(crate) credentials: Option<Vec<u8>>,
}

impl BundleEntries {
    pub(crate) fn new(manifest: Vec<u8>, documents: Vec<u8>, credentials: Option<Vec<u8>>) -> Self {
        Self {
            manifest,
            documents,
            credentials,
        }
    }
}
