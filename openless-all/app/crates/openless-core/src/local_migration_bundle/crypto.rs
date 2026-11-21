use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use getrandom::fill;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::types::{BundleError, BundlePassword, BundleResult};

const ENVELOPE_VERSION: u8 = 1;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const KEY_BYTES: usize = 32;
const ARGON_MEMORY_KIB: u32 = 65_536;
const ARGON_TIME: u32 = 3;
const ARGON_LANES: u32 = 4;
const HEADER_BYTES: usize = 1 + SALT_BYTES + NONCE_BYTES;

#[derive(Clone)]
struct DerivedKey(Zeroizing<[u8; KEY_BYTES]>);

impl fmt::Debug for DerivedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DerivedKey([REDACTED])")
    }
}

fn derive_key(password: &BundlePassword, salt: &[u8; SALT_BYTES]) -> BundleResult<DerivedKey> {
    let params = Params::new(ARGON_MEMORY_KIB, ARGON_TIME, ARGON_LANES, Some(KEY_BYTES))
        .map_err(|_| BundleError::InvalidPassword)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_BYTES]);
    argon
        .hash_password_into(password.expose(), salt, key.as_mut())
        .map_err(|_| BundleError::InvalidPassword)?;
    Ok(DerivedKey(key))
}

pub(crate) fn encrypt(
    password: &BundlePassword,
    plaintext: &[u8],
    aad: &[u8],
) -> BundleResult<Vec<u8>> {
    if plaintext.len() > super::types::MAX_CREDENTIAL_BYTES {
        return Err(BundleError::TooLarge);
    }
    let mut salt = [0u8; SALT_BYTES];
    let mut nonce = [0u8; NONCE_BYTES];
    fill(&mut salt).map_err(|_| BundleError::InvalidPassword)?;
    fill(&mut nonce).map_err(|_| BundleError::InvalidPassword)?;
    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new((&*key.0).into());
    // `cipher` 0.5 把 `from_slice` 标成 deprecated：`&[u8; N]` 直接转 `&Array`。
    let nonce_ref: &XNonce = (&nonce).into();
    let ciphertext = cipher
        .encrypt(
            nonce_ref,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| BundleError::InvalidPassword)?;
    let mut result = Vec::with_capacity(HEADER_BYTES + ciphertext.len());
    result.push(ENVELOPE_VERSION);
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

pub(crate) fn decrypt(
    password: &BundlePassword,
    envelope: &[u8],
    aad: &[u8],
) -> BundleResult<Zeroizing<Vec<u8>>> {
    if envelope.len() < HEADER_BYTES + 16
        || envelope.len() > super::types::MAX_CREDENTIAL_BYTES + HEADER_BYTES + 16
    {
        return Err(BundleError::InvalidPassword);
    }
    if envelope[0] != ENVELOPE_VERSION {
        return Err(BundleError::UnsupportedVersion);
    }
    let salt: &[u8; SALT_BYTES] = envelope[1..1 + SALT_BYTES]
        .try_into()
        .map_err(|_| BundleError::InvalidPassword)?;
    let nonce: &[u8; NONCE_BYTES] = envelope[1 + SALT_BYTES..HEADER_BYTES]
        .try_into()
        .map_err(|_| BundleError::InvalidPassword)?;
    let key = derive_key(password, salt)?;
    let cipher = XChaCha20Poly1305::new((&*key.0).into());
    let nonce_ref: &XNonce = nonce.into();
    let plaintext = cipher
        .decrypt(
            nonce_ref,
            Payload {
                msg: &envelope[HEADER_BYTES..],
                aad,
            },
        )
        .map_err(|_| BundleError::InvalidPassword)?;
    if plaintext.len() > super::types::MAX_CREDENTIAL_BYTES {
        return Err(BundleError::TooLarge);
    }
    Ok(Zeroizing::new(plaintext))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub(crate) fn aad(bundle_id: &str, documents_digest: &str, entry: &str) -> Vec<u8> {
    format!(
        "openless-local-migration-bundle\0{}\0{}\0{}\0{}\0{}",
        super::types::BUNDLE_FORMAT,
        super::types::BUNDLE_FORMAT_VERSION,
        bundle_id,
        documents_digest,
        entry,
    )
    .into_bytes()
}
