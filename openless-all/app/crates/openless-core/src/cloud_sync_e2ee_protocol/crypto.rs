//! v1 cryptography uses RustCrypto primitives, never a custom cipher/KDF.
//! Argon2 is CPU/memory intensive: callers must run derivation off UI/async workers.

use super::{types::*, Error, Result};
use argon2::{Algorithm, Argon2, Block, Params, Version};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use std::fmt;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

pub(crate) struct NormalizedPassword(Zeroizing<String>);
impl fmt::Debug for NormalizedPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NormalizedPassword([REDACTED])")
    }
}
impl NormalizedPassword {
    /// NFC only: neither trim nor case-fold changes the password sent to the KDF.
    pub(crate) fn new(input: SecretInput) -> Result<Self> {
        // Bound work before Unicode normalization, then check the normative NFC limit.
        if input.expose().len() > 4096 {
            return Err(Error::WeakPassword);
        }
        let mut text = Zeroizing::new(String::with_capacity(512));
        let mut count = 0;
        for character in input.expose().nfc() {
            count += 1;
            if count > 128 || text.len() + character.len_utf8() > 512 {
                return Err(Error::WeakPassword);
            }
            text.push(character);
        }
        if !(12..=128).contains(&count)
            || !text.bytes().any(|c| c.is_ascii_lowercase())
            || !text.bytes().any(|c| c.is_ascii_uppercase())
            || !text.bytes().any(|c| c.is_ascii_digit())
        {
            return Err(Error::WeakPassword);
        }
        Ok(Self(text))
    }
    /// Apply local password policy v1 only when creating/changing a password.
    /// Unlock remains compatible if a later release expands the weak-password list.
    pub(crate) fn validate_new(&self) -> Result<()> {
        let lower = Zeroizing::new(self.0.to_ascii_lowercase());
        let signature = Zeroizing::new(
            lower
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>(),
        );
        let weak = [
            "password",
            "qwerty",
            "12345678",
            "abcdefgh",
            "abcdefg",
            "letmein",
            "welcome",
            "administrator",
        ];
        if weak.iter().any(|word| signature.contains(word)) {
            return Err(Error::WeakPassword);
        }
        let mapped = Zeroizing::new(
            lower
                .chars()
                .map(|c| match c {
                    '@' | '4' => 'a',
                    '0' => 'o',
                    '$' | '5' => 's',
                    '3' => 'e',
                    _ => c,
                })
                .collect::<String>(),
        );
        if mapped.contains("password") || mapped.contains("qwerty") {
            return Err(Error::WeakPassword);
        }
        let chars = Zeroizing::new(lower.chars().collect::<Vec<char>>());
        let unique = chars
            .iter()
            .enumerate()
            .filter(|(i, c)| !chars[..*i].contains(c))
            .count();
        if unique < 4
            || (1..=8.min(chars.len() / 3)).any(|period| {
                chars
                    .iter()
                    .enumerate()
                    .all(|(i, c)| *c == chars[i % period])
            })
        {
            return Err(Error::WeakPassword);
        }
        Ok(())
    }
    pub(crate) fn confirmed_new(input: SecretInput, confirmation: SecretInput) -> Result<Self> {
        let password = Self::new(input)?;
        let confirmation = Self::new(confirmation)?;
        if password.0.as_bytes() != confirmation.0.as_bytes() {
            return Err(Error::PasswordConfirmationMismatch);
        }
        password.validate_new()?;
        Ok(password)
    }
}

/// Not serializable or implicitly cloneable. Secure-store/journal callers must bind
/// exported bytes to service origin + GitHub ID + vault ID + key ID themselves.
pub(crate) struct DerivedKey(Zeroizing<[u8; 32]>);
impl DerivedKey {
    pub(crate) fn from_secret_bytes(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }
    pub(crate) fn expose_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub(crate) fn copy_secret_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(*self.0)
    }
}
impl fmt::Debug for DerivedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DerivedKey([REDACTED])")
    }
}

pub(crate) fn derive_key(password: &NormalizedPassword, kdf: &Kdf) -> Result<DerivedKey> {
    kdf.validate()?;
    let params = Params::new(65_536, 3, 4, Some(32)).map_err(|_| Error::UnsupportedProtocol)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0; 32]);
    // The caller-owned Argon2 working memory is also wiped on all exits.
    let mut memory = Zeroizing::new(vec![Block::default(); 65_536]);
    argon
        .hash_password_into_with_memory(
            password.0.as_bytes(),
            kdf.salt.bytes(),
            key.as_mut(),
            &mut memory[..],
        )
        .map_err(|_| Error::InvalidPasswordOrCiphertext)?;
    Ok(DerivedKey(key))
}

#[derive(Clone, Debug)]
pub(crate) struct EncryptContext {
    pub(crate) owner_github_id: GithubId,
    pub(crate) vault_id: UuidV4,
    pub(crate) key_id: UuidV4,
    pub(crate) base_revision: Revision,
    pub(crate) operation_id: UuidV4,
    pub(crate) kind: UploadKind,
    pub(crate) kdf: Kdf,
}
impl EncryptContext {
    pub(crate) fn validate(&self) -> Result<()> {
        self.kdf.validate()?;
        self.base_revision.checked_next().map(|_| ())
    }
    fn header(&self, nonce: Nonce) -> Result<SnapshotUpload> {
        self.validate()?;
        Ok(SnapshotUpload {
            protocol_version: PROTOCOL_VERSION,
            payload_schema_version: 1,
            owner_github_id: self.owner_github_id.clone(),
            vault_id: self.vault_id.clone(),
            key_id: self.key_id.clone(),
            base_revision: self.base_revision,
            revision: self.base_revision.checked_next()?,
            operation_id: self.operation_id.clone(),
            kind: self.kind,
            crypto_profile: CRYPTO_PROFILE.into(),
            kdf: self.kdf.clone(),
            aead: AEAD_NAME.into(),
            codec: CODEC_NAME.into(),
            nonce,
            ciphertext: String::new(),
            ciphertext_sha256: Sha256Digest::of(&[]),
        })
    }
}

/// Explicit ordered JSON array, independent of object/map serialization order.
pub(crate) fn aad(snapshot: &SnapshotUpload) -> Result<Vec<u8>> {
    snapshot.kdf.validate()?;
    if snapshot.protocol_version != PROTOCOL_VERSION
        || snapshot.payload_schema_version != 1
        || snapshot.crypto_profile != CRYPTO_PROFILE
        || snapshot.aead != AEAD_NAME
        || snapshot.codec != CODEC_NAME
    {
        return Err(Error::UnsupportedProtocol);
    }
    let fields = serde_json::json!([
        "openless-cloud-sync",
        1,
        "snapshot",
        snapshot.owner_github_id.as_str(),
        snapshot.vault_id.as_str(),
        snapshot.key_id.as_str(),
        snapshot.base_revision.as_str(),
        snapshot.revision.as_str(),
        snapshot.operation_id.as_str(),
        snapshot.kind.as_str(),
        1,
        CRYPTO_PROFILE,
        "argon2id",
        19,
        65536,
        3,
        4,
        snapshot.kdf.salt.encoded(),
        AEAD_NAME,
        CODEC_NAME
    ]);
    serde_json::to_vec(&fields).map_err(|_| Error::InvalidWire("aad"))
}

fn padded_length(json_len: usize) -> Result<usize> {
    if json_len == 0 || json_len > MAX_PLAINTEXT_JSON_BYTES {
        return Err(Error::PayloadTooLarge);
    }
    let len = (json_len + 4).div_ceil(PAD_BLOCK_BYTES) * PAD_BLOCK_BYTES;
    if len > MAX_PADDED_BYTES {
        return Err(Error::PayloadTooLarge);
    }
    Ok(len)
}

pub(crate) fn encrypt_snapshot(
    documents: &DocumentSet,
    context: &EncryptContext,
    key: &DerivedKey,
) -> Result<SnapshotUpload> {
    let json = documents.to_secret_json()?;
    context.validate()?;
    let nonce = Nonce::random()?;
    let length = padded_length(json.len())?;
    let mut plaintext = Zeroizing::new(Vec::with_capacity(length + 16));
    plaintext.resize(length, 0);
    plaintext[..4].copy_from_slice(&(json.len() as u32).to_be_bytes());
    plaintext[4..4 + json.len()].copy_from_slice(&json);
    getrandom::fill(&mut plaintext[4 + json.len()..]).map_err(|_| Error::RandomUnavailable)?;
    seal_frame(context, key, nonce, plaintext)
}

fn seal_frame(
    context: &EncryptContext,
    key: &DerivedKey,
    nonce: Nonce,
    mut frame: Zeroizing<Vec<u8>>,
) -> Result<SnapshotUpload> {
    let mut snapshot = context.header(nonce)?;
    let cipher = XChaCha20Poly1305::new(key.expose_bytes().into());
    let associated = aad(&snapshot)?;
    cipher
        .encrypt_in_place(
            XNonce::from_slice(snapshot.nonce.bytes()),
            &associated,
            &mut *frame,
        )
        .map_err(|_| Error::InvalidPasswordOrCiphertext)?;
    snapshot.ciphertext_sha256 = Sha256Digest::of(&frame);
    snapshot.ciphertext = URL_SAFE_NO_PAD.encode(&frame);
    snapshot.validate()?;
    Ok(snapshot)
}

/// The authenticated account and the exact metadata read are mandatory, not an
/// optional caller check. Authentication is verified before parsing any plaintext.
pub(crate) fn decrypt_snapshot(
    snapshot: &SnapshotUpload,
    metadata: &VaultMetadata,
    owner: &GithubId,
    key: &DerivedKey,
) -> Result<DocumentSet> {
    snapshot.validate_against_metadata(metadata, owner)?;
    let mut frame = Zeroizing::new(snapshot.decoded_ciphertext()?);
    let cipher = XChaCha20Poly1305::new(key.expose_bytes().into());
    cipher
        .decrypt_in_place(
            XNonce::from_slice(snapshot.nonce.bytes()),
            &aad(snapshot)?,
            &mut *frame,
        )
        .map_err(|_| Error::InvalidPasswordOrCiphertext)?;
    decode_frame(&frame)
}

pub(crate) fn decrypt_snapshot_with_password(
    snapshot: &SnapshotUpload,
    metadata: &VaultMetadata,
    owner: &GithubId,
    password: SecretInput,
) -> Result<(DerivedKey, DocumentSet)> {
    // Validate fixed KDF parameters, ciphertext bounds/hash and identity before
    // allocating Argon2's 64 MiB memory, even when a server supplied this header.
    snapshot.validate_against_metadata(metadata, owner)?;
    let password = NormalizedPassword::new(password)?;
    let key = derive_key(&password, &snapshot.kdf)?;
    let documents = decrypt_snapshot(snapshot, metadata, owner, &key)?;
    Ok((key, documents))
}

fn decode_frame(frame: &[u8]) -> Result<DocumentSet> {
    if frame.len() < PAD_BLOCK_BYTES
        || frame.len() > MAX_PADDED_BYTES
        || !frame.len().is_multiple_of(PAD_BLOCK_BYTES)
    {
        return Err(Error::InvalidPasswordOrCiphertext);
    }
    let count = u32::from_be_bytes(
        frame[..4]
            .try_into()
            .map_err(|_| Error::InvalidPasswordOrCiphertext)?,
    ) as usize;
    if count > MAX_PLAINTEXT_JSON_BYTES
        || count == 0
        || count > frame.len() - 4
        || padded_length(count)? != frame.len()
    {
        return Err(Error::InvalidPasswordOrCiphertext);
    }
    DocumentSet::from_json_bytes(&frame[4..4 + count])
}

// Two independently bounded 15 MiB snapshots plus transaction metadata fit here;
// this local-only ceiling never changes the remote v1 15 MiB JSON limit.
pub(crate) const MAX_LOCAL_PLAINTEXT_BYTES: usize = 40 * 1024 * 1024;
const LOCAL_ENVELOPE_MAGIC: &[u8] = b"OL-E2EE-JOURNAL\x01";

fn local_aad(scope: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if scope.is_empty() || scope.len() > CONTROL_BODY_LIMIT {
        return Err(Error::InvalidWire("local journal scope"));
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(64 + scope.len()));
    // Separate domain: a network snapshot can never authenticate as a journal.
    bytes.extend_from_slice(b"openless-cloud-sync\0local-journal\0v1\0");
    bytes.extend_from_slice(&(scope.len() as u32).to_be_bytes());
    bytes.extend_from_slice(scope);
    Ok(bytes)
}

/// Local-only journal encryption. The caller supplies canonical scope bytes
/// including service origin, owner, vault/key IDs and operation identity.
/// No journal bytes may be submitted through the snapshot HTTP API.
pub(crate) fn seal_local(key: &DerivedKey, scope_aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    if plaintext.len() > MAX_LOCAL_PLAINTEXT_BYTES {
        return Err(Error::PayloadTooLarge);
    }
    let associated = local_aad(scope_aad)?;
    let nonce = Nonce::random()?;
    let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + 16));
    buffer.extend_from_slice(plaintext);
    XChaCha20Poly1305::new(key.expose_bytes().into())
        .encrypt_in_place(XNonce::from_slice(nonce.bytes()), &associated, &mut *buffer)
        .map_err(|_| Error::InvalidPasswordOrCiphertext)?;
    let mut envelope = Vec::with_capacity(LOCAL_ENVELOPE_MAGIC.len() + 24 + buffer.len());
    envelope.extend_from_slice(LOCAL_ENVELOPE_MAGIC);
    envelope.extend_from_slice(nonce.bytes());
    envelope.extend_from_slice(&buffer);
    Ok(envelope)
}

pub(crate) fn open_local(
    key: &DerivedKey,
    scope_aad: &[u8],
    envelope: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let prefix = LOCAL_ENVELOPE_MAGIC.len() + 24;
    if envelope.len() < prefix + 16
        || envelope.len() > prefix + 16 + MAX_LOCAL_PLAINTEXT_BYTES
        || !envelope.starts_with(LOCAL_ENVELOPE_MAGIC)
    {
        return Err(Error::InvalidPasswordOrCiphertext);
    }
    let associated = local_aad(scope_aad)?;
    let nonce = &envelope[LOCAL_ENVELOPE_MAGIC.len()..prefix];
    let mut buffer = Zeroizing::new(envelope[prefix..].to_vec());
    XChaCha20Poly1305::new(key.expose_bytes().into())
        .decrypt_in_place(XNonce::from_slice(nonce), &associated, &mut *buffer)
        .map_err(|_| Error::InvalidPasswordOrCiphertext)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests;
