//! Reuse the item reference returned by an authorized read when updating its contents.
//! The system still owns access checks; no ACL or trusted application list is changed.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use security_framework::os::macos::{
    keychain::{SecKeychain, SecPreferencesDomain},
    keychain_item::SecKeychainItem,
    passwords::find_generic_password,
};

use super::{CredentialsVault, KEYRING_SINGLE_CREDENTIALS_ACCOUNT};

#[derive(Default)]
enum PrimaryReference {
    #[default]
    Unloaded,
    Absent,
    Known(SecKeychainItem),
}

static PRIMARY_REFERENCE: OnceLock<Mutex<PrimaryReference>> = OnceLock::new();

fn reference() -> &'static Mutex<PrimaryReference> {
    PRIMARY_REFERENCE.get_or_init(|| Mutex::new(PrimaryReference::default()))
}

fn platform_error(error: security_framework::base::Error) -> anyhow::Error {
    let error = match error.code() {
        -25291 | -25292 | -25294 | -25293 | -25308 | -128 => {
            keyring::Error::NoStorageAccess(Box::new(error))
        }
        _ => keyring::Error::PlatformFailure(Box::new(error)),
    };
    anyhow::anyhow!(error)
}

fn user_keychain() -> Result<SecKeychain> {
    SecKeychain::default_for_domain(SecPreferencesDomain::User)
        .map_err(platform_error)
        .context("open macOS user keychain")
}

pub(super) fn read_primary() -> Result<Option<String>> {
    let keychain = user_keychain()?;
    match find_generic_password(
        Some(&[keychain]),
        CredentialsVault::SERVICE_NAME,
        KEYRING_SINGLE_CREDENTIALS_ACCOUNT,
    ) {
        Ok((bytes, item)) => {
            let bytes = zeroize::Zeroizing::new(bytes.to_owned());
            let raw = std::str::from_utf8(&bytes)
                .map_err(|_| anyhow::anyhow!("invalid macOS credential encoding"))?
                .to_owned();
            *reference().lock() = PrimaryReference::Known(item);
            Ok(Some(raw))
        }
        Err(error) if error.code() == -25300 => {
            *reference().lock() = PrimaryReference::Absent;
            Ok(None)
        }
        Err(error) => {
            *reference().lock() = PrimaryReference::Unloaded;
            Err(platform_error(error)).context("read macOS credential vault")
        }
    }
}

pub(super) fn write_primary(raw: &str) -> Result<()> {
    let unloaded = matches!(*reference().lock(), PrimaryReference::Unloaded);
    if unloaded {
        // Only a newly created item lacks a reference after its first successful save.
        // Do not create over a denied or unreadable existing item.
        read_primary()?;
    }
    let mut item = match &*reference().lock() {
        PrimaryReference::Known(item) => Some(item.clone()),
        PrimaryReference::Absent => None,
        PrimaryReference::Unloaded => anyhow::bail!("macOS credential reference is unavailable"),
    };
    let result = if let Some(item) = item.as_mut() {
        item.set_password(raw.as_bytes()).map_err(platform_error)
    } else {
        user_keychain()?
            .add_generic_password(
                CredentialsVault::SERVICE_NAME,
                KEYRING_SINGLE_CREDENTIALS_ACCOUNT,
                raw.as_bytes(),
            )
            .map_err(platform_error)
    };
    if result.is_err() || item.is_none() {
        *reference().lock() = PrimaryReference::Unloaded;
    }
    result.context("write macOS credential vault")
}
