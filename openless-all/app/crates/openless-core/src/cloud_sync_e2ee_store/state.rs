use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cloud_sync_e2ee_documents::{DocumentError, DocumentResult, SecretJson, SyncScope};
use crate::cloud_sync_e2ee_protocol::types::{DocumentSet, LogicalDocument, Revision, Tombstone};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScopeState {
    pub schema_version: u32,
    pub scope_id: String,
    pub observed_revision: Revision,
    pub baseline_revision: Revision,
    pub baseline: Option<DocumentSet>,
    pub retained_documents: Vec<LogicalDocument>,
    pub tombstones: Vec<Tombstone>,
}

impl ScopeState {
    pub fn empty(scope: &SyncScope) -> DocumentResult<Self> {
        Ok(Self {
            schema_version: 1,
            scope_id: scope_id(scope)?,
            observed_revision: Revision::new(0),
            baseline_revision: Revision::new(0),
            baseline: None,
            retained_documents: Vec::new(),
            tombstones: Vec::new(),
        })
    }
    pub fn decode(scope: &SyncScope, value: Option<SecretJson>) -> DocumentResult<Self> {
        let Some(value) = value else {
            return Self::empty(scope);
        };
        let state: Self = serde_json::from_value(value.expose().clone())
            .map_err(|_| DocumentError::RecoveryRequired)?;
        if state.schema_version != 1 || state.scope_id != scope_id(scope)? {
            return Err(DocumentError::RecoveryRequired);
        }
        Ok(state)
    }
    pub fn secret_json(&self) -> DocumentResult<SecretJson> {
        SecretJson::from_serializable(self)
    }
}

/// Cloud key rotation changes encryption, not the account/vault's local logical baseline.
pub fn scope_id(scope: &SyncScope) -> DocumentResult<String> {
    let bytes = serde_json::to_vec(&(
        "openless-local-documents-v1",
        &scope.service_origin,
        &scope.owner_github_id,
        &scope.vault_id,
        &scope.device_id,
    ))
    .map_err(|_| DocumentError::InvalidDocument)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
