use openless_core::{
    siwc::{error, locked_head, SiwcError, SiwcService, SiwcVault},
    SecretValue,
};
use std::{path::PathBuf, sync::Arc};
pub(crate) fn service(directory: PathBuf) -> Arc<SiwcService> {
    SiwcService::new(Arc::new(Vault(directory)))
}
struct Vault(PathBuf);
#[cfg(target_os = "linux")]
fn entry(key: &str) -> Result<keyring::Entry, SiwcError> {
    keyring::Entry::new("top.openless.linux.siwc", key).map_err(|_| error("storage_failed"))
}
impl SiwcVault for Vault {
    fn read(&self, key: &str) -> Result<Option<SecretValue>, SiwcError> {
        #[cfg(target_os = "linux")]
        {
            match entry(key)?.get_password() {
                Ok(s) => Ok(Some(SecretValue::new(s))),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(error("storage_unavailable")),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = key;
            Err(error("storage_unavailable"))
        }
    }
    fn write(&self, key: &str, value: &str) -> Result<(), SiwcError> {
        #[cfg(target_os = "linux")]
        {
            entry(key)?
                .set_password(value)
                .map_err(|_| error("storage_failed"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (key, value);
            Err(error("storage_unavailable"))
        }
    }
    fn remove(&self, key: &str) -> Result<(), SiwcError> {
        #[cfg(target_os = "linux")]
        {
            match entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(error("storage_failed")),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = key;
            Err(error("storage_unavailable"))
        }
    }
    fn compare_head(&self, expected: Option<&str>, next: &str) -> Result<bool, SiwcError> {
        locked_head(&self.0.join("siwc-transaction.lock"), || {
            if self.read("head")?.as_ref().map(SecretValue::expose_secret) != expected {
                return Ok(false);
            }
            self.write("head", next)?;
            Ok(true)
        })
    }
}
