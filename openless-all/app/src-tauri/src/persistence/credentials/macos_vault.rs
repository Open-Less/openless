//! One macOS Keychain envelope for provider credentials and device-local sync keys.
//! Local keys are an opaque extension that never enters the provider/Core snapshot.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use openless_core::{credentials::SyncSecretAccount, SecretValue};
use serde_json::Value;
use zeroize::Zeroizing;

use super::{decode_single_credentials, validate_sync_key, KEYRING_SINGLE_CREDENTIALS_ACCOUNT};

const LOCAL_KEYS: &str = "_macosLocalSyncKeys";

#[derive(Default)]
pub(super) struct SingleItemVault {
    // Outer None means unread; inner None means the OS positively reported NoEntry.
    primary: Option<Option<Zeroizing<String>>>,
    legacy_keys: BTreeMap<String, Option<SecretValue>>,
}

type Reader<'a> = dyn FnMut(&str) -> Result<Option<String>> + 'a;
type Writer<'a> = dyn FnMut(&str, &str) -> Result<()> + 'a;

fn parse_payload(raw: &str) -> Result<Value> {
    decode_single_credentials(raw)
        .map_err(|_| anyhow!("could not decode macOS credential envelope"))?;
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| anyhow!("could not decode macOS credential envelope"))?;
    // Provider access keeps this extension opaque. A damaged sync key must not
    // turn otherwise readable provider credentials into an empty configuration.
    Ok(value)
}

fn key_from(payload: &Value, account: &SyncSecretAccount) -> Result<Option<Option<SecretValue>>> {
    let Some(keys) = payload.get(LOCAL_KEYS) else {
        return Ok(None);
    };
    let keys = keys
        .as_object()
        .ok_or_else(|| anyhow!("invalid macOS local key envelope"))?;
    let Some(value) = keys.get(account.as_str()) else {
        return Ok(None);
    };
    // A null tombstone prevents an older independent key from resurrecting.
    if value.is_null() {
        return Ok(Some(None));
    }
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("invalid macOS local key value"))?;
    let secret = SecretValue::new(value);
    validate_sync_key(&secret)?;
    Ok(Some(Some(secret)))
}

fn put_key(payload: &mut Value, account: &str, secret: Option<&SecretValue>) -> Result<()> {
    let object = payload
        .as_object_mut()
        .expect("validated credential object");
    let keys = object
        .entry(LOCAL_KEYS)
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("invalid macOS local key envelope"))?;
    keys.insert(
        account.to_owned(),
        secret.map_or(Value::Null, |secret| {
            Value::String(secret.expose_secret().into())
        }),
    );
    Ok(())
}

impl SingleItemVault {
    pub(super) fn read_credentials(&mut self, read: &mut Reader<'_>) -> Result<Option<String>> {
        if self.primary.is_none() {
            let raw = read(KEYRING_SINGLE_CREDENTIALS_ACCOUNT)?;
            if let Some(raw) = raw.as_deref() {
                parse_payload(raw)?;
            }
            self.primary = Some(raw.map(Zeroizing::new));
        }
        Ok(self
            .primary
            .as_ref()
            .and_then(|value| value.as_ref())
            .map(|raw| raw.to_string()))
    }

    fn merged_credentials(&self, raw: &str) -> Result<Zeroizing<String>> {
        let mut next = parse_payload(raw)?;
        // Provider writes serialize only CredsRoot. Preserve the device-only extension.
        if let Some(Some(previous)) = self.primary.as_ref() {
            if let Some(keys) = parse_payload(previous)?.get(LOCAL_KEYS) {
                next.as_object_mut()
                    .expect("validated object")
                    .insert(LOCAL_KEYS.into(), keys.clone());
            }
        }
        for (account, secret) in &self.legacy_keys {
            if secret.is_some()
                || next
                    .get(LOCAL_KEYS)
                    .and_then(|keys| keys.get(account))
                    .is_some()
            {
                put_key(&mut next, account, secret.as_ref())?;
            }
        }
        Ok(Zeroizing::new(serde_json::to_string(&next)?))
    }

    pub(super) fn write_credentials(
        &mut self,
        raw: &str,
        read: &mut Reader<'_>,
        write: &mut Writer<'_>,
    ) -> Result<()> {
        self.read_credentials(read)?;
        let next = self.merged_credentials(raw)?;
        if let Err(error) = write(KEYRING_SINGLE_CREDENTIALS_ACCOUNT, &next) {
            self.primary = None;
            return Err(error);
        }
        self.primary = Some(Some(next));
        Ok(())
    }

    pub(super) fn read_sync_secret(
        &mut self,
        account: &SyncSecretAccount,
        read: &mut Reader<'_>,
        write: &mut Writer<'_>,
    ) -> Result<Option<SecretValue>> {
        let primary = self.read_credentials(read)?.map(Zeroizing::new);
        if let Some(raw) = primary.as_deref() {
            if let Some(value) = key_from(&parse_payload(raw)?, account)? {
                return Ok(value);
            }
        }
        if let Some(value) = self.legacy_keys.get(account.as_str()) {
            return Ok(value.clone());
        }
        // Each legacy item retains its own OS authorization during the one-time migration.
        let value = read(account.as_str())?.map(SecretValue::new);
        if let Some(value) = &value {
            validate_sync_key(value)?;
        }
        self.legacy_keys
            .insert(account.as_str().into(), value.clone());
        if value.is_some() {
            if let Some(raw) = primary.as_deref() {
                let next = self.merged_credentials(raw)?;
                if write(KEYRING_SINGLE_CREDENTIALS_ACCOUNT, &next).is_ok() {
                    self.primary = Some(Some(next));
                } else {
                    // Reading the authorized old key remains valid if consolidation is denied.
                    log::warn!("[vault] macOS local key consolidation deferred");
                }
            }
        }
        Ok(value)
    }

    pub(super) fn write_sync_secret(
        &mut self,
        account: &SyncSecretAccount,
        secret: &SecretValue,
        bootstrap: impl FnOnce() -> Result<String>,
        read: &mut Reader<'_>,
        write: &mut Writer<'_>,
    ) -> Result<()> {
        validate_sync_key(secret)?;
        let raw = self.read_credentials(read)?.map_or_else(bootstrap, Ok)?;
        let raw = Zeroizing::new(raw);
        let mut next = parse_payload(&self.merged_credentials(&raw)?)?;
        put_key(&mut next, account.as_str(), Some(secret))?;
        self.persist_verified(next, read, write)?;
        self.legacy_keys
            .insert(account.as_str().into(), Some(secret.clone()));
        Ok(())
    }

    pub(super) fn remove_sync_secret(
        &mut self,
        account: &SyncSecretAccount,
        read: &mut Reader<'_>,
        write: &mut Writer<'_>,
        delete: impl FnOnce(&str) -> Result<()>,
    ) -> Result<()> {
        if let Some(raw) = self.read_credentials(read)? {
            let mut next = parse_payload(&raw)?;
            put_key(&mut next, account.as_str(), None)?;
            self.persist_verified(next, read, write)?;
        }
        self.legacy_keys.insert(account.as_str().into(), None);
        // Forgetting must revoke the independent legacy item too, including old app versions.
        delete(account.as_str())
    }

    fn persist_verified(
        &mut self,
        next: Value,
        read: &mut Reader<'_>,
        write: &mut Writer<'_>,
    ) -> Result<()> {
        let raw = Zeroizing::new(serde_json::to_string(&next)?);
        // A failed write/readback must never leave a process cache claiming durable success.
        self.primary = None;
        write(KEYRING_SINGLE_CREDENTIALS_ACCOUNT, &raw)?;
        let stored = Zeroizing::new(
            read(KEYRING_SINGLE_CREDENTIALS_ACCOUNT)?
                .ok_or_else(|| anyhow!("macOS local key write could not be verified"))?,
        );
        anyhow::ensure!(
            parse_payload(&stored)? == next,
            "macOS local key write verification failed"
        );
        self.primary = Some(Some(stored));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    fn account(kind: &str) -> SyncSecretAccount {
        SyncSecretAccount::new(format!("cloud-sync.e2ee.{kind}.{}", "a".repeat(64))).unwrap()
    }

    fn secret() -> SecretValue {
        SecretValue::new("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
    }

    fn payload() -> Value {
        serde_json::json!({"version":2,"active":{"asr":"existing"},"providers":{},"metadataRevision":7})
    }

    #[test]
    fn credentials_and_both_sync_key_kinds_share_one_native_read() {
        let mut value = payload();
        for kind in ["local", "key"] {
            put_key(&mut value, account(kind).as_str(), Some(&secret())).unwrap();
        }
        let reads = Cell::new(0);
        let mut read = |name: &str| {
            assert_eq!(name, KEYRING_SINGLE_CREDENTIALS_ACCOUNT);
            reads.set(reads.get() + 1);
            Ok(Some(value.to_string()))
        };
        let mut vault = SingleItemVault::default();
        let mut write = |_: &str, _: &str| panic!("startup must not rewrite a consolidated vault");
        for _ in 0..3 {
            vault.read_credentials(&mut read).unwrap();
            for kind in ["local", "key"] {
                assert_eq!(
                    vault
                        .read_sync_secret(&account(kind), &mut read, &mut write)
                        .unwrap(),
                    Some(secret())
                );
            }
        }
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn legacy_keys_migrate_without_changing_provider_metadata() {
        let old = payload();
        let entries: RefCell<BTreeMap<String, String>> = RefCell::new(BTreeMap::from([
            (KEYRING_SINGLE_CREDENTIALS_ACCOUNT.into(), old.to_string()),
            (account("local").as_str().into(), secret().into_exposed()),
        ]));
        let mut read = |name: &str| Ok(entries.borrow().get(name).cloned());
        let mut write = |name: &str, raw: &str| {
            entries.borrow_mut().insert(name.into(), raw.into());
            Ok(())
        };
        let mut vault = SingleItemVault::default();
        assert_eq!(
            vault
                .read_sync_secret(&account("local"), &mut read, &mut write)
                .unwrap(),
            Some(secret())
        );
        let migrated: Value =
            serde_json::from_str(&entries.borrow()[KEYRING_SINGLE_CREDENTIALS_ACCOUNT]).unwrap();
        let mut credentials = migrated.clone();
        credentials.as_object_mut().unwrap().remove(LOCAL_KEYS);
        assert_eq!(credentials, old);
        assert!(entries.borrow().contains_key(account("local").as_str()));
        let mut next_process = SingleItemVault::default();
        let mut read_single = |name: &str| {
            assert_eq!(name, KEYRING_SINGLE_CREDENTIALS_ACCOUNT);
            read(name)
        };
        assert_eq!(
            next_process
                .read_sync_secret(&account("local"), &mut read_single, &mut write)
                .unwrap(),
            Some(secret())
        );
    }

    #[test]
    fn provider_writes_and_restore_preserve_device_only_keys() {
        let mut value = payload();
        put_key(&mut value, account("key").as_str(), Some(&secret())).unwrap();
        let stored = RefCell::new(value.to_string());
        let mut read = |_: &str| Ok(Some(stored.borrow().clone()));
        let mut write = |_: &str, raw: &str| {
            *stored.borrow_mut() = raw.into();
            Ok(())
        };
        let mut vault = SingleItemVault::default();
        let mut replacement = payload();
        replacement["active"]["asr"] = "restored".into();
        vault
            .write_credentials(&replacement.to_string(), &mut read, &mut write)
            .unwrap();
        assert_eq!(
            vault
                .read_sync_secret(&account("key"), &mut read, &mut write)
                .unwrap(),
            Some(secret())
        );
        let root = decode_single_credentials(&stored.borrow()).unwrap();
        assert!(!serde_json::to_string(&root).unwrap().contains(LOCAL_KEYS));
        assert_eq!(
            parse_payload(&stored.borrow()).unwrap()["active"]["asr"],
            "restored"
        );
    }

    #[test]
    fn corrupt_sync_key_remains_an_error_without_disabling_provider_access_or_using_legacy() {
        let mut value = payload();
        value[LOCAL_KEYS] = serde_json::json!({(account("key").as_str()): "invalid-key-canary"});
        let stored = RefCell::new(value.to_string());
        let mut read = |name: &str| {
            assert_eq!(
                name, KEYRING_SINGLE_CREDENTIALS_ACCOUNT,
                "corruption must not fall back"
            );
            Ok(Some(stored.borrow().clone()))
        };
        let mut write = |_: &str, raw: &str| {
            *stored.borrow_mut() = raw.into();
            Ok(())
        };
        let mut vault = SingleItemVault::default();
        assert!(vault.read_credentials(&mut read).unwrap().is_some());
        let error = vault
            .read_sync_secret(&account("key"), &mut read, &mut write)
            .unwrap_err();
        assert!(!format!("{error:#}").contains("invalid-key-canary"));
        vault
            .write_credentials(&payload().to_string(), &mut read, &mut write)
            .unwrap();
        assert_eq!(
            parse_payload(&stored.borrow()).unwrap()[LOCAL_KEYS],
            value[LOCAL_KEYS]
        );
    }

    #[test]
    fn first_sync_key_creation_preserves_the_complete_legacy_bootstrap_and_verifies_storage() {
        let stored = RefCell::new(None::<String>);
        let mut read = |name: &str| {
            assert_eq!(name, KEYRING_SINGLE_CREDENTIALS_ACCOUNT);
            Ok(stored.borrow().clone())
        };
        let mut write = |_: &str, raw: &str| {
            *stored.borrow_mut() = Some(raw.into());
            Ok(())
        };
        let mut vault = SingleItemVault::default();
        vault
            .write_sync_secret(
                &account("local"),
                &secret(),
                || Ok(payload().to_string()),
                &mut read,
                &mut write,
            )
            .unwrap();
        let mut value = parse_payload(stored.borrow().as_deref().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove(LOCAL_KEYS);
        assert_eq!(value, payload());
        assert_eq!(
            vault
                .read_sync_secret(&account("local"), &mut read, &mut write)
                .unwrap(),
            Some(secret())
        );
    }

    #[test]
    fn denied_legacy_consolidation_keeps_the_authorized_key_and_does_not_read_it_again() {
        let reads = Cell::new(0);
        let mut read = |name: &str| {
            reads.set(reads.get() + 1);
            Ok(Some(if name == KEYRING_SINGLE_CREDENTIALS_ACCOUNT {
                payload().to_string()
            } else {
                secret().into_exposed()
            }))
        };
        let mut vault = SingleItemVault::default();
        let mut write = |_: &str, _: &str| Err(anyhow!("write denied"));
        for _ in 0..3 {
            assert_eq!(
                vault
                    .read_sync_secret(&account("local"), &mut read, &mut write)
                    .unwrap(),
                Some(secret())
            );
        }
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn authorization_denial_and_invalid_payload_do_not_fall_back_to_legacy_items() {
        for raw in [None, Some("{}".to_owned())] {
            let reads = Cell::new(0);
            let mut read = |_: &str| {
                reads.set(reads.get() + 1);
                raw.clone()
                    .map_or_else(|| Err(anyhow!("authorization denied")), |raw| Ok(Some(raw)))
            };
            let mut vault = SingleItemVault::default();
            assert!(vault
                .read_sync_secret(&account("key"), &mut read, &mut |_, _| Ok(()))
                .is_err());
            assert_eq!(reads.get(), 1);
        }
    }

    #[test]
    fn failed_new_key_write_readback_is_an_error_and_does_not_cache_the_key() {
        let mut read = |_: &str| Ok(Some(payload().to_string()));
        let mut vault = SingleItemVault::default();
        assert!(vault
            .write_sync_secret(
                &account("local"),
                &secret(),
                || panic!(),
                &mut read,
                &mut |_, _| Ok(())
            )
            .is_err());
        assert!(vault.primary.is_none());
        assert!(!vault.legacy_keys.contains_key(account("local").as_str()));
    }

    #[test]
    fn forgetting_a_key_cannot_resurrect_a_legacy_item() {
        let stored = RefCell::new(payload().to_string());
        let mut read = |name: &str| {
            if name == KEYRING_SINGLE_CREDENTIALS_ACCOUNT {
                Ok(Some(stored.borrow().clone()))
            } else {
                Ok(Some(secret().into_exposed()))
            }
        };
        let mut write = |_: &str, raw: &str| {
            *stored.borrow_mut() = raw.into();
            Ok(())
        };
        let mut vault = SingleItemVault::default();
        // Even if OS removal fails, the verified tombstone owns current-app revocation.
        assert!(vault
            .remove_sync_secret(&account("key"), &mut read, &mut write, |_| Err(anyhow!(
                "denied"
            )))
            .is_err());
        assert_eq!(
            vault
                .read_sync_secret(&account("key"), &mut read, &mut write)
                .unwrap(),
            None
        );
        let mut next_process = SingleItemVault::default();
        assert_eq!(
            next_process
                .read_sync_secret(&account("key"), &mut read, &mut write)
                .unwrap(),
            None
        );
    }
}
