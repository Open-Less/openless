//! Separate vault namespace: SIWC sessions are never part of provider exports or backups.
use openless_core::{
    siwc::{error, locked_head, SiwcError, SiwcService, SiwcVault},
    SecretValue,
};
use std::sync::Arc;
struct NativeSiwcVault;
pub(crate) fn siwc_service() -> Arc<SiwcService> {
    SiwcService::new(Arc::new(NativeSiwcVault))
}
fn check_key(key: &str) -> Result<(), SiwcError> {
    if key.len() <= 100
        && !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
    {
        Ok(())
    } else {
        Err(error("storage_failed"))
    }
}
#[cfg(not(target_os = "android"))]
fn entry(key: &str) -> Result<keyring::Entry, SiwcError> {
    check_key(key)?;
    keyring::Entry::new("com.openless.app.siwc", key).map_err(|_| error("storage_failed"))
}
#[cfg(target_os = "android")]
fn item_path(key: &str) -> Result<std::path::PathBuf, SiwcError> {
    check_key(key)?;
    let directory = super::data_dir()
        .map_err(|_| error("storage_failed"))?
        .join("siwc-local");
    std::fs::create_dir_all(&directory).map_err(|_| error("storage_failed"))?;
    Ok(directory.join(format!("{key}.enc.json")))
}
impl SiwcVault for NativeSiwcVault {
    fn read(&self, key: &str) -> Result<Option<SecretValue>, SiwcError> {
        #[cfg(target_os = "macos")]
        {
            Ok(macos_values()?.remove(key).map(SecretValue::new))
        }
        #[cfg(not(any(target_os = "android", target_os = "macos")))]
        {
            match entry(key)?.get_password() {
                Ok(value) => Ok(Some(SecretValue::new(value))),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(error("storage_unavailable")),
            }
        }
        #[cfg(target_os = "android")]
        {
            use super::android_credentials::{read_only, AndroidKeystoreCrypto, ReadOutcome};
            let bytes = match read_only(&item_path(key)?, &mut AndroidKeystoreCrypto)
                .map_err(|_| error("storage_unavailable"))?
            {
                ReadOutcome::Missing => return Ok(None),
                ReadOutcome::Plaintext(bytes) => zeroize::Zeroizing::new(bytes),
                ReadOutcome::Legacy(_) => return Err(error("storage_corrupt")),
            };
            let record: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| error("storage_corrupt"))?;
            if record["key"] != key {
                return Err(error("storage_corrupt"));
            }
            Ok(Some(SecretValue::new(
                record["value"]
                    .as_str()
                    .ok_or_else(|| error("storage_corrupt"))?,
            )))
        }
    }
    fn write(&self, key: &str, value: &str) -> Result<(), SiwcError> {
        #[cfg(target_os = "macos")]
        {
            macos_update(|values| {
                values.insert(key.into(), value.into());
                Ok(())
            })
        }
        #[cfg(not(any(target_os = "android", target_os = "macos")))]
        {
            entry(key)?
                .set_password(value)
                .map_err(|_| error("storage_failed"))
        }
        #[cfg(target_os = "android")]
        {
            let bytes = zeroize::Zeroizing::new(
                serde_json::to_vec(&serde_json::json!({"key":key,"value":value}))
                    .map_err(|_| error("storage_failed"))?,
            );
            super::android_credentials::write_verified(
                &item_path(key)?,
                &bytes,
                &mut super::android_credentials::AndroidKeystoreCrypto,
            )
            .map_err(|_| error("storage_failed"))
        }
    }
    fn remove(&self, key: &str) -> Result<(), SiwcError> {
        #[cfg(target_os = "macos")]
        {
            macos_update(|values| {
                values.remove(key);
                Ok(())
            })
        }
        #[cfg(not(any(target_os = "android", target_os = "macos")))]
        {
            match entry(key)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(error("storage_failed")),
            }
        }
        #[cfg(target_os = "android")]
        {
            super::android_credentials::secure_remove(&item_path(key)?)
                .map_err(|_| error("storage_failed"))
        }
    }
    fn compare_head(&self, expected: Option<&str>, next: &str) -> Result<bool, SiwcError> {
        #[cfg(target_os = "macos")]
        {
            macos_update(|values| {
                if values.get("head").map(String::as_str) != expected {
                    return Ok(false);
                }
                values.insert("head".into(), next.into());
                Ok(true)
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let lock = super::data_dir()
                .map_err(|_| error("storage_failed"))?
                .join("siwc-local")
                .join("transaction.lock");
            locked_head(&lock, || {
                if self.read("head")?.as_ref().map(SecretValue::expose_secret) != expected {
                    return Ok(false);
                }
                self.write("head", next)?;
                Ok(true)
            })
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_values() -> Result<std::collections::BTreeMap<String, String>, SiwcError> {
    let raw = match entry("vault.v1")?.get_password() {
        Ok(raw) => zeroize::Zeroizing::new(raw),
        Err(keyring::Error::NoEntry) => return Ok(Default::default()),
        Err(_) => return Err(error("storage_unavailable")),
    };
    serde_json::from_str(&raw).map_err(|_| error("storage_corrupt"))
}
#[cfg(target_os = "macos")]
fn macos_update<T>(
    update: impl FnOnce(&mut std::collections::BTreeMap<String, String>) -> Result<T, SiwcError>,
) -> Result<T, SiwcError> {
    let lock = super::data_dir()
        .map_err(|_| error("storage_failed"))?
        .join("siwc-local/transaction.lock");
    locked_head(&lock, || {
        let mut values = macos_values()?;
        let result = update(&mut values)?;
        let payload = zeroize::Zeroizing::new(
            serde_json::to_string(&values).map_err(|_| error("storage_failed"))?,
        );
        // Keep one stable Keychain item instead of prompting for every generation's chunks.
        entry("vault.v1")?
            .set_password(&payload)
            .map_err(|_| error("storage_failed"))?;
        Ok(result)
    })
}
