//! Encrypted local baselines and unresolved operations. The wrapping key lives
//! only in the host's system vault, never beside these files.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use zeroize::Zeroizing;

use crate::cloud_sync_e2ee_protocol::crypto::{open_local, seal_local, DerivedKey};
use crate::credentials::SyncSecretAccount;
use crate::{CredentialStore, SecretValue};

use super::{error, SyncResult};

#[derive(Clone)]
pub(crate) struct LocalStorage {
    root: PathBuf,
    origin: String,
    device_id: String,
    credentials: Arc<dyn CredentialStore>,
    cipher: Arc<OnceCell<Arc<DerivedKey>>>,
    io: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClientSettings {
    pub version: u32,
    pub enabled: bool,
    pub consent_version: Option<String>,
    pub owner_id: Option<String>,
    pub remember_key: bool,
    #[serde(default)]
    pub key_preference_epoch: u64,
    pub last_success: Option<String>,
    pub last_generation: Option<String>,
    pub vault_id: Option<String>,
    pub key_id: Option<String>,
    pub prompted: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiEnvelope {
    pub schema_version: u32,
    pub revision: String,
    pub value: crate::cloud_sync_e2ee_documents::SecretJson,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            consent_version: None,
            owner_id: None,
            remember_key: false,
            key_preference_epoch: 0,
            last_success: None,
            last_generation: None,
            vault_id: None,
            key_id: None,
            prompted: false,
        }
    }
}

impl LocalStorage {
    pub(crate) fn new(
        root: PathBuf,
        origin: String,
        device_id: String,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            root,
            origin,
            device_id,
            credentials,
            cipher: Arc::new(OnceCell::new()),
            io: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(crate) fn initialized(&self) -> SyncResult<bool> {
        self.path("device", "client")
            .try_exists()
            .map_err(|_| error("local_storage_unavailable"))
    }

    pub(crate) async fn read_ui(&self) -> SyncResult<Option<UiEnvelope>> {
        let value: Option<UiEnvelope> =
            self.read("device", "sync-ui-preferences")
                .await
                .inspect_err(|error| {
                    log_local_failure("ui_mirror_read", error);
                })?;
        if let Some(value) = &value {
            if value.schema_version != 1
                || crate::cloud_sync_e2ee_protocol::types::UuidV4::parse(&value.revision).is_err()
            {
                return Err(error("recovery_required"));
            }
        }
        Ok(value)
    }

    pub(crate) fn binding(
        &self,
        owner: &str,
        vault: &str,
        key: &str,
    ) -> SyncResult<SyncSecretAccount> {
        let binding = serde_json::to_vec(&[&self.origin, owner, vault, key, &self.device_id])
            .map_err(|_| error("local_storage_unavailable"))?;
        SyncSecretAccount::new(format!("cloud-sync.e2ee.key.{:x}", Sha256::digest(binding)))
            .map_err(|_| error("local_storage_unavailable"))
    }

    pub(crate) async fn key(&self) -> SyncResult<Arc<DerivedKey>> {
        self.cipher
            .get_or_try_init(|| async {
                let aad = self.aad("device", "storage-key")?;
                let account = SyncSecretAccount::new(format!(
                    "cloud-sync.e2ee.local.{:x}",
                    Sha256::digest(aad)
                ))
                .map_err(|_| error("local_storage_unavailable"))?;
                let value = self
                    .credentials
                    .read_sync_secret(account.clone())
                    .await
                    .map_err(|_| {
                        log::warn!("[e2ee-local] stage=wrapping_key_read code=secure_storage_denied");
                        error("secure_storage_denied")
                    })?;
                let bytes = match value {
                    Some(value) => decode_key(value.expose_secret())?,
                    None => {
                        // Losing the system-vault key never silently resets encrypted recovery data.
                        let root = self.root.clone();
                        let has_state = tokio::task::spawn_blocking(move || {
                            if !root.exists() {
                                return Ok(false);
                            }
                            let files = fs::read_dir(root)
                                .map_err(|_| error("local_storage_unavailable"))?;
                            for file in files {
                                let file = file.map_err(|_| error("local_storage_unavailable"))?;
                                if file
                                    .path()
                                    .extension()
                                    .is_some_and(|extension| extension == "enc")
                                {
                                    return Ok(true);
                                }
                            }
                            Ok(false)
                        })
                        .await
                        .map_err(|_| error("local_storage_unavailable"))??;
                        if has_state {
                            return Err(error("recovery_required"));
                        }
                        let mut bytes = Zeroizing::new([0_u8; 32]);
                        getrandom::fill(&mut *bytes)
                            .map_err(|_| error("secure_random_unavailable"))?;
                        self.credentials
                            .write_sync_secret(
                                account.clone(),
                                SecretValue::new(URL_SAFE_NO_PAD.encode(&bytes[..])),
                            )
                            .await
                            .map_err(|_| {
                                log::warn!("[e2ee-local] stage=wrapping_key_write code=secure_storage_denied");
                                error("secure_storage_denied")
                            })?;
                        let verified = self
                            .credentials
                            .read_sync_secret(account)
                            .await
                            .map_err(|_| error("secure_storage_denied"))?
                            .ok_or_else(|| error("secure_storage_denied"))?;
                        if decode_key(verified.expose_secret())?.as_ref() != bytes.as_ref() {
                            return Err(error("secure_storage_denied"));
                        }
                        bytes
                    }
                };
                Ok(Arc::new(DerivedKey::from_secret_bytes(bytes)))
            })
            .await
            .cloned()
    }

    fn aad(&self, owner: &str, name: &str) -> SyncResult<Vec<u8>> {
        serde_json::to_vec(&[
            "openless.local-sync.v1",
            &self.origin,
            &self.device_id,
            owner,
            name,
        ])
        .map_err(|_| error("local_storage_unavailable"))
    }

    fn path(&self, owner: &str, name: &str) -> PathBuf {
        let id = format!("{owner}\0{name}");
        self.root
            .join(format!("{:x}.enc", Sha256::digest(id.as_bytes())))
    }

    pub(crate) async fn read<T: DeserializeOwned + Send + 'static>(
        &self,
        owner: &str,
        name: &str,
    ) -> SyncResult<Option<T>> {
        let io = self.io.clone().lock_owned().await;
        let path = self.path(owner, name);
        if !path
            .try_exists()
            .map_err(|_| error("local_storage_unavailable"))?
        {
            return Ok(None);
        }
        let key = self.key().await?;
        let aad = self.aad(owner, name)?;
        tokio::task::spawn_blocking(move || {
            let _io = io;
            let file = fs::File::open(path).map_err(|_| error("local_storage_unavailable"))?;
            if file
                .metadata()
                .map_err(|_| error("local_storage_unavailable"))?
                .len()
                > 41 * 1024 * 1024
            {
                return Err(error("recovery_required"));
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(41 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| error("local_storage_unavailable"))?;
            let plain = open_local(&key, &aad, &bytes).map_err(|_| error("recovery_required"))?;
            serde_json::from_slice(&plain)
                .map(Some)
                .map_err(|_| error("recovery_required"))
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn write<T: Serialize + Send + 'static>(
        &self,
        owner: &str,
        name: &str,
        value: T,
    ) -> SyncResult<()> {
        let key = self.key().await?;
        let io = self.io.clone().lock_owned().await;
        let aad = self.aad(owner, name)?;
        let path = self.path(owner, name);
        tokio::task::spawn_blocking(move || {
            let _io = io;
            let plain = Zeroizing::new(
                serde_json::to_vec(&value).map_err(|_| error("local_storage_unavailable"))?,
            );
            let sealed =
                seal_local(&key, &aad, &plain).map_err(|_| error("local_storage_unavailable"))?;
            durable_replace(&path, &sealed)
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn remove(&self, owner: &str, name: &str) -> SyncResult<()> {
        let io = self.io.clone().lock_owned().await;
        let path = self.path(owner, name);
        tokio::task::spawn_blocking(move || {
            let _io = io;
            match fs::remove_file(&path) {
                Ok(()) => sync_directory(
                    path.parent()
                        .ok_or_else(|| error("local_storage_unavailable"))?,
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(error("local_storage_unavailable")),
            }
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn remember(
        &self,
        owner: &str,
        vault: &str,
        key_id: &str,
        key: &DerivedKey,
        remember: bool,
    ) -> SyncResult<()> {
        let account = self.binding(owner, vault, key_id)?;
        if remember {
            let bytes = key.copy_secret_bytes();
            self.credentials
                .write_sync_secret(
                    account,
                    SecretValue::new(URL_SAFE_NO_PAD.encode(&bytes[..])),
                )
                .await
                .map_err(|_| error("secure_storage_denied"))
        } else {
            self.credentials
                .remove_sync_secret(account)
                .await
                .map_err(|_| error("secure_storage_denied"))
        }
    }

    pub(crate) async fn remembered(
        &self,
        owner: &str,
        vault: &str,
        key_id: &str,
    ) -> SyncResult<Option<DerivedKey>> {
        self.credentials
            .read_sync_secret(self.binding(owner, vault, key_id)?)
            .await
            .map_err(|_| error("secure_storage_denied"))?
            .map(|value| decode_key(value.expose_secret()).map(DerivedKey::from_secret_bytes))
            .transpose()
    }

    pub(crate) async fn forget(&self, owner: &str, vault: &str, key_id: &str) -> SyncResult<()> {
        self.credentials
            .remove_sync_secret(self.binding(owner, vault, key_id)?)
            .await
            .map_err(|_| error("secure_storage_denied"))
    }

    /// A non-secret, durable refusal to auto-unlock survives even when encrypted
    /// recovery metadata or the operating-system vault cannot currently be read.
    pub(crate) async fn set_lockout(&self, locked: bool) -> SyncResult<()> {
        let io = self.io.clone().lock_owned().await;
        let path = self.root.join("unlock-disabled");
        tokio::task::spawn_blocking(move || {
            let _io = io;
            if locked {
                durable_replace(&path, b"locked-v1\n")
            } else {
                match fs::remove_file(&path) {
                    Ok(()) => sync_directory(
                        path.parent()
                            .ok_or_else(|| error("local_storage_unavailable"))?,
                    ),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(_) => Err(error("local_storage_unavailable")),
                }
            }
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn locked_out(&self) -> SyncResult<bool> {
        let io = self.io.clone().lock_owned().await;
        let path = self.root.join("unlock-disabled");
        tokio::task::spawn_blocking(move || {
            let _io = io;
            path.try_exists()
                .map_err(|_| error("local_storage_unavailable"))
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }
}

fn decode_key(value: &str) -> SyncResult<Zeroizing<[u8; 32]>> {
    let bytes = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| error("secure_storage_denied"))?,
    );
    if URL_SAFE_NO_PAD.encode(&*bytes) != value {
        return Err(error("secure_storage_denied"));
    }
    let bytes =
        <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| error("secure_storage_denied"))?;
    Ok(Zeroizing::new(bytes))
}

pub(crate) fn durable_replace(path: &Path, bytes: &[u8]) -> SyncResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error("local_storage_unavailable"))?;
    fs::create_dir_all(parent).map_err(|_| error("local_storage_unavailable"))?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| error("local_storage_unavailable"))?;
        file.write_all(bytes)
            .map_err(|_| error("local_storage_unavailable"))?;
        file.sync_all()
            .map_err(|_| error("local_storage_unavailable"))?;
        fs::rename(&temporary, path).map_err(|_| error("local_storage_unavailable"))?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn sync_directory(path: &Path) -> SyncResult<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|_| error("local_storage_unavailable"))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Publish a fully written identity without replacing an existing install's ID.
pub(crate) fn durable_create(path: &Path, bytes: &[u8]) -> SyncResult<bool> {
    let parent = path
        .parent()
        .ok_or_else(|| error("local_storage_unavailable"))?;
    fs::create_dir_all(parent).map_err(|_| error("local_storage_unavailable"))?;
    let temporary = parent.join(format!(".identity-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| error("local_storage_unavailable"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| error("local_storage_unavailable"))?;
        match fs::hard_link(&temporary, path) {
            Ok(()) => {
                sync_directory(parent)?;
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(_) => Err(error("local_storage_unavailable")),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn log_local_failure(stage: &'static str, failure: &crate::BackendError) {
    let code = match failure
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(serde_json::Value::as_str)
    {
        Some("secure_storage_denied") => "secure_storage_denied",
        Some("recovery_required") => "recovery_required",
        Some("secure_random_unavailable") => "secure_random_unavailable",
        _ => "local_storage_unavailable",
    };
    log::warn!("[e2ee-local] stage={stage} code={code}");
}
