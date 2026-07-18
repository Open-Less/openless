use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const ENVELOPE_FORMAT: &str = "openless-android-credentials";
const ENVELOPE_VERSION: u32 = 2;
const ENVELOPE_ACCOUNT: &str = "credentials.v1";
const NONCE_BYTES: usize = 12;
const GCM_TAG_BYTES: usize = 16;
const ENVELOPE_AAD: &[u8] =
    b"openless-android-credentials\x002\x00com.openless.app\x00credentials.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct EnvelopeV2 {
    format: String,
    version: u32,
    account: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SealedPayload {
    pub(super) nonce: Vec<u8>,
    pub(super) ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum CryptoErrorKind {
    #[error("authentication failed")]
    AuthenticationFailed,
    #[error("key missing or invalidated")]
    KeyMissingOrInvalidated,
    #[error("temporarily unavailable")]
    TemporarilyUnavailable,
}

pub(super) trait AndroidCredentialsCrypto {
    fn seal(
        &mut self,
        plaintext: &[u8],
        aad: &[u8],
    ) -> std::result::Result<SealedPayload, CryptoErrorKind>;

    fn open(
        &mut self,
        sealed: &SealedPayload,
        aad: &[u8],
    ) -> std::result::Result<Vec<u8>, CryptoErrorKind>;

    fn delete_key(&mut self) -> std::result::Result<(), CryptoErrorKind>;

    fn migration_complete(&mut self) -> std::result::Result<bool, CryptoErrorKind>;

    fn mark_migration_complete(&mut self) -> std::result::Result<(), CryptoErrorKind>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReadOutcome {
    Missing,
    Legacy(Vec<u8>),
    Plaintext(Vec<u8>),
}

#[derive(Debug, thiserror::Error)]
pub(super) enum StoreError {
    #[error("invalid Android credential envelope")]
    InvalidEnvelope,
    #[error("Android credential authentication or key operation failed: {0}")]
    Crypto(#[from] CryptoErrorKind),
    #[error("Android credential persistence failed during {operation}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("Android credential persistence verification failed")]
    VerificationFailed,
    #[error("Android credential envelope serialization failed")]
    Serialization,
}

fn io_error(operation: &'static str, source: io::Error) -> StoreError {
    StoreError::Io { operation, source }
}

fn validated_envelope(bytes: &[u8]) -> Result<(EnvelopeV2, SealedPayload), StoreError> {
    let envelope =
        serde_json::from_slice::<EnvelopeV2>(bytes).map_err(|_| StoreError::InvalidEnvelope)?;
    if envelope.format != ENVELOPE_FORMAT
        || envelope.version != ENVELOPE_VERSION
        || envelope.account != ENVELOPE_ACCOUNT
    {
        return Err(StoreError::InvalidEnvelope);
    }

    let nonce = base64::engine::general_purpose::STANDARD
        .decode(&envelope.nonce)
        .map_err(|_| StoreError::InvalidEnvelope)?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(&envelope.ciphertext)
        .map_err(|_| StoreError::InvalidEnvelope)?;
    if nonce.len() != NONCE_BYTES || ciphertext.len() < GCM_TAG_BYTES {
        return Err(StoreError::InvalidEnvelope);
    }

    Ok((
        envelope,
        SealedPayload {
            nonce,
            ciphertext,
        },
    ))
}

fn open_envelope(
    bytes: &[u8],
    crypto: &mut impl AndroidCredentialsCrypto,
) -> Result<Vec<u8>, StoreError> {
    let (_, sealed) = validated_envelope(bytes)?;
    crypto
        .open(&sealed, ENVELOPE_AAD)
        .map_err(StoreError::Crypto)
}

pub(super) fn read(
    path: &Path,
    crypto: &mut impl AndroidCredentialsCrypto,
) -> Result<ReadOutcome, StoreError> {
    recover_verified_sanitized_legacy(path)?;
    recover_verified_v2_temporary(path, crypto)?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(ReadOutcome::Missing),
        Err(error) => return Err(io_error("read", error)),
    };
    let first = bytes.iter().copied().find(|byte| !byte.is_ascii_whitespace());
    let Some(first) = first else {
        return Err(StoreError::InvalidEnvelope);
    };

    if first != b'{' {
        // The non-exportable marker is created after a v2 temporary envelope
        // has been verified and before it can replace a legacy envelope. Once
        // present, a Base64 file is a rollback or injection attempt rather
        // than a legitimate upgrade source.
        if crypto
            .migration_complete()
            .map_err(StoreError::Crypto)?
        {
            return Err(StoreError::InvalidEnvelope);
        }
        let plaintext = base64::engine::general_purpose::STANDARD
            .decode(&bytes)
            .map_err(|_| StoreError::InvalidEnvelope)?;
        if plaintext.is_empty() {
            return Err(StoreError::InvalidEnvelope);
        }
        return Ok(ReadOutcome::Legacy(plaintext));
    }

    match open_envelope(&bytes, crypto) {
        Ok(plaintext) => {
            crypto
                .mark_migration_complete()
                .map_err(StoreError::Crypto)?;
            Ok(ReadOutcome::Plaintext(plaintext))
        }
        Err(StoreError::Crypto(CryptoErrorKind::KeyMissingOrInvalidated)) => {
            // The ciphertext can no longer be recovered. Reset the alias first;
            // if that is temporarily unavailable, preserve the file for retry.
            crypto.delete_key().map_err(StoreError::Crypto)?;
            secure_remove(path)?;
            Ok(ReadOutcome::Missing)
        }
        Err(error) => Err(error),
    }
}

fn v2_temporary_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn verified_v2_temporary_path(path: &Path) -> PathBuf {
    path.with_extension("json.pending")
}

fn recover_verified_v2_temporary(
    path: &Path,
    crypto: &mut impl AndroidCredentialsCrypto,
) -> Result<(), StoreError> {
    let temporary = verified_v2_temporary_path(path);
    let persisted = match fs::read(&temporary) {
        Ok(persisted) => persisted,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error("read verified v2 recovery", error)),
    };
    match open_envelope(&persisted, crypto) {
        Ok(_) => {}
        Err(StoreError::Crypto(CryptoErrorKind::KeyMissingOrInvalidated)) => {
            // Match the main-envelope recovery behavior: a permanently
            // invalidated key cannot decrypt this candidate, so clear both
            // files and let the caller reconfigure credentials.
            crypto.delete_key().map_err(StoreError::Crypto)?;
            secure_remove(&temporary)?;
            secure_remove(path)?;
            return Ok(());
        }
        Err(error) => return Err(error),
    }

    // Re-establish the non-exportable rollback barrier before promoting the
    // verified recovery candidate. A crash after this point leaves the
    // candidate available for another recovery attempt instead of admitting
    // the old Base64 envelope again.
    crypto
        .mark_migration_complete()
        .map_err(StoreError::Crypto)?;
    set_private_file_mode(&temporary)?;
    fs::rename(&temporary, path).map_err(|error| io_error("recover verified v2", error))?;
    set_private_file_mode(path)?;
    sync_parent(path)
}

fn recover_verified_sanitized_legacy(path: &Path) -> Result<(), StoreError> {
    let temporary = path.with_extension("legacy.tmp");
    let destination_needs_recovery = match fs::metadata(path) {
        Ok(metadata) => metadata.len() == 0,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => return Err(io_error("inspect legacy recovery destination", error)),
    };
    if !destination_needs_recovery {
        return Ok(());
    }

    let persisted = match fs::read(&temporary) {
        Ok(persisted) => persisted,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error("read sanitized legacy recovery", error)),
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&persisted)
        .map_err(|_| StoreError::VerificationFailed)?;
    if decoded.is_empty() {
        return Err(StoreError::VerificationFailed);
    }

    set_private_file_mode(&temporary)?;
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("remove empty legacy recovery target", error)),
    }
    fs::rename(&temporary, path)
        .map_err(|error| io_error("recover sanitized legacy", error))?;
    set_private_file_mode(path)?;
    sync_parent(path)
}

fn envelope_for(sealed: &SealedPayload) -> Result<Vec<u8>, StoreError> {
    if sealed.nonce.len() != NONCE_BYTES || sealed.ciphertext.len() < GCM_TAG_BYTES {
        return Err(StoreError::VerificationFailed);
    }
    serde_json::to_vec(&EnvelopeV2 {
        format: ENVELOPE_FORMAT.to_string(),
        version: ENVELOPE_VERSION,
        account: ENVELOPE_ACCOUNT.to_string(),
        nonce: base64::engine::general_purpose::STANDARD.encode(&sealed.nonce),
        ciphertext: base64::engine::general_purpose::STANDARD.encode(&sealed.ciphertext),
    })
    .map_err(|_| StoreError::Serialization)
}

fn ensure_private_dir(path: &Path) -> Result<(), StoreError> {
    fs::create_dir_all(path).map_err(|error| io_error("create directory", error))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| io_error("set directory permissions", error))?;
    Ok(())
}

fn set_private_file_mode(path: &Path) -> Result<(), StoreError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| io_error("set file permissions", error))?;
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> Result<(), StoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync parent directory", error))
}

// This module is production-only on Android. Windows only compiles it for
// host-side unit tests, where opening a directory for `sync_all` is rejected.
#[cfg(windows)]
fn sync_parent(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove temporary file", error)),
    }
}

fn open_private_temp(path: &Path) -> Result<File, StoreError> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options
        .open(path)
        .map_err(|error| io_error("open temporary file", error))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteStage {
    TempOpen,
    AfterTempOpen,
    AfterTempWrite,
    AfterTempSync,
    AfterVerification,
    Rename,
    AfterRename,
    ParentSync,
}

pub(super) fn write_verified(
    path: &Path,
    plaintext: &[u8],
    crypto: &mut impl AndroidCredentialsCrypto,
) -> Result<(), StoreError> {
    write_verified_with_fault(path, plaintext, crypto, &mut |_| Ok(()))
}

pub(super) fn rewrite_legacy_without_bearer(
    path: &Path,
    sanitized_plaintext: &[u8],
) -> Result<(), StoreError> {
    rewrite_legacy_without_bearer_with_fault(path, sanitized_plaintext, &mut |_| Ok(()))
}

fn rewrite_legacy_without_bearer_with_fault(
    path: &Path,
    sanitized_plaintext: &[u8],
    fault: &mut impl FnMut(WriteStage) -> io::Result<()>,
) -> Result<(), StoreError> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(sanitized_plaintext);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_private_dir(parent)?;
    let temporary = path.with_extension("legacy.tmp");
    remove_if_present(&temporary)?;
    let mut replaced = false;
    let mut replacement_verified = false;

    let result = (|| {
        fault(WriteStage::TempOpen).map_err(|error| io_error("open sanitized legacy", error))?;
        let mut output = open_private_temp(&temporary)?;
        fault(WriteStage::AfterTempOpen)
            .map_err(|error| io_error("after opening sanitized legacy", error))?;
        output
            .write_all(encoded.as_bytes())
            .map_err(|error| io_error("write sanitized legacy", error))?;
        fault(WriteStage::AfterTempWrite)
            .map_err(|error| io_error("after writing sanitized legacy", error))?;
        output
            .flush()
            .map_err(|error| io_error("flush sanitized legacy", error))?;
        output
            .sync_all()
            .map_err(|error| io_error("sync sanitized legacy", error))?;
        fault(WriteStage::AfterTempSync)
            .map_err(|error| io_error("after syncing sanitized legacy", error))?;
        drop(output);
        set_private_file_mode(&temporary)?;

        let persisted = fs::read(&temporary)
            .map_err(|error| io_error("reread sanitized legacy", error))?;
        let verified = base64::engine::general_purpose::STANDARD
            .decode(&persisted)
            .map_err(|_| StoreError::VerificationFailed)?;
        if verified != sanitized_plaintext {
            return Err(StoreError::VerificationFailed);
        }
        replacement_verified = true;
        fault(WriteStage::AfterVerification)
            .map_err(|error| io_error("after verifying sanitized legacy", error))?;

        // The replacement is ready and durable. Erase the bearer-bearing
        // source before installing it, preserving the previous fail-closed
        // contract while keeping all non-Marketplace credentials recoverable.
        secure_remove(path)?;
        fault(WriteStage::Rename).map_err(|error| io_error("replace sanitized legacy", error))?;
        fs::rename(&temporary, path)
            .map_err(|error| io_error("replace sanitized legacy", error))?;
        replaced = true;
        fault(WriteStage::AfterRename)
            .map_err(|error| io_error("after replacing sanitized legacy", error))?;
        set_private_file_mode(path)?;
        fault(WriteStage::ParentSync)
            .map_err(|error| io_error("sync sanitized legacy directory", error))?;
        sync_parent(path)
    })();

    if result.is_err() {
        if !replaced && replacement_verified {
            // Keep the durable bearer-free copy recoverable if the cutover
            // fails after verification. read() will install it when the
            // destination is missing or was already truncated.
            let _ = secure_remove(path);
        } else {
            let _ = fs::remove_file(&temporary);
        }
        if !replaced && !replacement_verified {
            // A returned migration error must never leave the old bearer
            // recoverable. This deliberately favors token invalidation over
            // availability, matching the pre-v2 Android policy.
            let _ = secure_remove(path);
        }
    }
    result
}

fn write_verified_with_fault(
    path: &Path,
    plaintext: &[u8],
    crypto: &mut impl AndroidCredentialsCrypto,
    fault: &mut impl FnMut(WriteStage) -> io::Result<()>,
) -> Result<(), StoreError> {
    recover_verified_v2_temporary(path, crypto)?;
    let sealed = crypto
        .seal(plaintext, ENVELOPE_AAD)
        .map_err(StoreError::Crypto)?;
    let reopened = crypto
        .open(&sealed, ENVELOPE_AAD)
        .map_err(StoreError::Crypto)?;
    if reopened != plaintext {
        return Err(StoreError::VerificationFailed);
    }
    let encoded = envelope_for(&sealed)?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_private_dir(parent)?;
    let temporary = v2_temporary_path(path);
    let recovery = verified_v2_temporary_path(path);
    remove_if_present(&temporary)?;
    let mut recovery_prepared = false;

    let result = (|| {
        fault(WriteStage::TempOpen).map_err(|error| io_error("open temporary file", error))?;
        let mut output = open_private_temp(&temporary)?;
        fault(WriteStage::AfterTempOpen)
            .map_err(|error| io_error("after opening temporary file", error))?;
        output
            .write_all(&encoded)
            .map_err(|error| io_error("write temporary file", error))?;
        fault(WriteStage::AfterTempWrite)
            .map_err(|error| io_error("after writing temporary file", error))?;
        output
            .flush()
            .map_err(|error| io_error("flush temporary file", error))?;
        output
            .sync_all()
            .map_err(|error| io_error("sync temporary file", error))?;
        fault(WriteStage::AfterTempSync)
            .map_err(|error| io_error("after syncing temporary file", error))?;
        drop(output);
        set_private_file_mode(&temporary)?;

        let persisted = fs::read(&temporary)
            .map_err(|error| io_error("reread temporary file", error))?;
        let verified = open_envelope(&persisted, crypto)?;
        if verified != plaintext {
            return Err(StoreError::VerificationFailed);
        }

        // Keep a separately named, durable recovery candidate only after the
        // reread/decrypt check succeeds. `read` never promotes the ordinary
        // write temp, so a crash before this point cannot replace a complete
        // envelope with an unverified write.
        fs::rename(&temporary, &recovery)
            .map_err(|error| io_error("prepare verified credential recovery", error))?;
        recovery_prepared = true;
        sync_parent(&recovery)?;

        // Retire Base64 envelopes before replacing one. If the process stops
        // after this point, `recovery` can be promoted on the next launch;
        // the old file is never admitted as a downgrade path.
        crypto
            .mark_migration_complete()
            .map_err(StoreError::Crypto)?;
        fault(WriteStage::AfterVerification)
            .map_err(|error| io_error("after verifying temporary file", error))?;

        fault(WriteStage::Rename).map_err(|error| io_error("replace credential file", error))?;
        fs::rename(&recovery, path).map_err(|error| io_error("replace credential file", error))?;
        fault(WriteStage::AfterRename)
            .map_err(|error| io_error("after replacing credential file", error))?;
        set_private_file_mode(path)?;
        fault(WriteStage::ParentSync)
            .map_err(|error| io_error("sync parent directory", error))?;
        sync_parent(path)
    })();

    if result.is_err() {
        if !recovery_prepared {
            let _ = fs::remove_file(&temporary);
        }
    }
    result
}

pub(super) fn secure_remove(path: &Path) -> Result<(), StoreError> {
    match OpenOptions::new().write(true).open(path) {
        Ok(file) => {
            file.set_len(0)
                .map_err(|error| io_error("truncate unrecoverable credential file", error))?;
            file.sync_all()
                .map_err(|error| io_error("sync unrecoverable credential file", error))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error("open unrecoverable credential file", error)),
    }
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove unrecoverable credential file", error)),
    }
}

#[cfg(target_os = "android")]
pub(super) struct AndroidKeystoreCrypto;

#[cfg(target_os = "android")]
impl AndroidCredentialsCrypto for AndroidKeystoreCrypto {
    fn seal(
        &mut self,
        plaintext: &[u8],
        aad: &[u8],
    ) -> std::result::Result<SealedPayload, CryptoErrorKind> {
        let packet = crate::android::jni::android::keystore_seal(plaintext, aad)
            .map_err(map_keystore_failure)?;
        split_packet(&packet)
    }

    fn open(
        &mut self,
        sealed: &SealedPayload,
        aad: &[u8],
    ) -> std::result::Result<Vec<u8>, CryptoErrorKind> {
        crate::android::jni::android::keystore_open(&join_packet(sealed), aad)
            .map_err(map_keystore_failure)
    }

    fn delete_key(&mut self) -> std::result::Result<(), CryptoErrorKind> {
        crate::android::jni::android::keystore_delete_key().map_err(map_keystore_failure)
    }

    fn migration_complete(&mut self) -> std::result::Result<bool, CryptoErrorKind> {
        crate::android::jni::android::keystore_migration_complete()
            .map_err(map_keystore_failure)
    }

    fn mark_migration_complete(&mut self) -> std::result::Result<(), CryptoErrorKind> {
        crate::android::jni::android::keystore_mark_migration_complete()
            .map_err(map_keystore_failure)
    }
}

#[cfg(target_os = "android")]
fn map_keystore_failure(
    failure: crate::android::jni::android::AndroidKeystoreFailure,
) -> CryptoErrorKind {
    use crate::android::jni::android::AndroidKeystoreFailure;
    match failure {
        AndroidKeystoreFailure::AuthenticationFailed | AndroidKeystoreFailure::Malformed => {
            CryptoErrorKind::AuthenticationFailed
        }
        AndroidKeystoreFailure::KeyMissingOrInvalidated => {
            CryptoErrorKind::KeyMissingOrInvalidated
        }
        AndroidKeystoreFailure::TemporarilyUnavailable => {
            CryptoErrorKind::TemporarilyUnavailable
        }
    }
}

#[cfg(target_os = "android")]
fn split_packet(packet: &[u8]) -> std::result::Result<SealedPayload, CryptoErrorKind> {
    let Some((&nonce_len, body)) = packet.split_first() else {
        return Err(CryptoErrorKind::TemporarilyUnavailable);
    };
    let nonce_len = usize::from(nonce_len);
    if nonce_len != NONCE_BYTES || body.len() < nonce_len + GCM_TAG_BYTES {
        return Err(CryptoErrorKind::TemporarilyUnavailable);
    }
    Ok(SealedPayload {
        nonce: body[..nonce_len].to_vec(),
        ciphertext: body[nonce_len..].to_vec(),
    })
}

#[cfg(target_os = "android")]
fn join_packet(sealed: &SealedPayload) -> Vec<u8> {
    let mut packet = Vec::with_capacity(1 + sealed.nonce.len() + sealed.ciphertext.len());
    packet.push(sealed.nonce.len() as u8);
    packet.extend_from_slice(&sealed.nonce);
    packet.extend_from_slice(&sealed.ciphertext);
    packet
}

#[cfg(test)]
pub(super) struct TestCrypto {
    key: [u8; 32],
    counter: u64,
    key_present: bool,
    migration_complete: bool,
    pub(super) fail_next_seal: Option<CryptoErrorKind>,
    pub(super) fail_next_open: Option<CryptoErrorKind>,
    pub(super) fail_next_mark_migration: Option<CryptoErrorKind>,
    pub(super) delete_key_calls: usize,
}

#[cfg(test)]
impl Default for TestCrypto {
    fn default() -> Self {
        Self {
            key: [0xA7; 32],
            counter: 0,
            key_present: false,
            migration_complete: false,
            fail_next_seal: None,
            fail_next_open: None,
            fail_next_mark_migration: None,
            delete_key_calls: 0,
        }
    }
}

#[cfg(test)]
impl TestCrypto {
    fn authentication_tag(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> [u8; 16] {
        use sha2::{Digest, Sha256};

        let mut digest = Sha256::new();
        digest.update(self.key);
        digest.update(aad);
        digest.update(nonce);
        digest.update(ciphertext);
        let digest = digest.finalize();
        let mut tag = [0_u8; 16];
        tag.copy_from_slice(&digest[..16]);
        tag
    }
}

#[cfg(test)]
impl AndroidCredentialsCrypto for TestCrypto {
    fn seal(
        &mut self,
        plaintext: &[u8],
        aad: &[u8],
    ) -> std::result::Result<SealedPayload, CryptoErrorKind> {
        if let Some(error) = self.fail_next_seal.take() {
            return Err(error);
        }
        self.key_present = true;
        self.counter += 1;
        let mut nonce = vec![0x51, 0x7A, 0xC3, 0x19];
        nonce.extend_from_slice(&self.counter.to_be_bytes());
        let mut ciphertext = plaintext
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ self.key[index % self.key.len()])
            .collect::<Vec<_>>();
        let tag = self.authentication_tag(&nonce, &ciphertext, aad);
        ciphertext.extend_from_slice(&tag);
        Ok(SealedPayload {
            nonce,
            ciphertext,
        })
    }

    fn open(
        &mut self,
        sealed: &SealedPayload,
        aad: &[u8],
    ) -> std::result::Result<Vec<u8>, CryptoErrorKind> {
        if let Some(error) = self.fail_next_open.take() {
            return Err(error);
        }
        if !self.key_present {
            return Err(CryptoErrorKind::KeyMissingOrInvalidated);
        }
        if sealed.nonce.len() != NONCE_BYTES || sealed.ciphertext.len() < GCM_TAG_BYTES {
            return Err(CryptoErrorKind::AuthenticationFailed);
        }
        let split = sealed.ciphertext.len() - GCM_TAG_BYTES;
        let (ciphertext, tag) = sealed.ciphertext.split_at(split);
        let expected = self.authentication_tag(&sealed.nonce, ciphertext, aad);
        if expected.as_slice() != tag {
            return Err(CryptoErrorKind::AuthenticationFailed);
        }
        Ok(ciphertext
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ self.key[index % self.key.len()])
            .collect())
    }

    fn delete_key(&mut self) -> std::result::Result<(), CryptoErrorKind> {
        self.delete_key_calls += 1;
        self.key_present = false;
        Ok(())
    }

    fn migration_complete(&mut self) -> std::result::Result<bool, CryptoErrorKind> {
        Ok(self.migration_complete)
    }

    fn mark_migration_complete(&mut self) -> std::result::Result<(), CryptoErrorKind> {
        if let Some(error) = self.fail_next_mark_migration.take() {
            return Err(error);
        }
        self.migration_complete = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::io;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn test_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("openless-{name}-{}", uuid::Uuid::new_v4()))
            .join("credentials.enc.json")
    }

    fn remove_test_parent(path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn v2_round_trip_hides_plaintext() {
        let path = test_path("android-v2-round-trip");
        let plaintext = br#"{"version":1,"providers":{"llm":{"ark":{"apiKey":"sk-secret-sentinel"}}}}"#;
        let mut crypto = TestCrypto::default();

        write_verified(&path, plaintext, &mut crypto).unwrap();

        let disk = std::fs::read(&path).unwrap();
        assert!(!disk.windows(b"sk-secret-sentinel".len()).any(|window| {
            window == b"sk-secret-sentinel"
        }));
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&disk) {
            assert!(!String::from_utf8_lossy(&decoded).contains("sk-secret-sentinel"));
        }
        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Plaintext(plaintext.to_vec())
        );
        remove_test_parent(&path);
    }

    #[test]
    fn v2_uses_a_fresh_nonce_for_each_write() {
        let path = test_path("android-v2-fresh-nonce");
        let mut crypto = TestCrypto::default();

        write_verified(&path, b"same plaintext", &mut crypto).unwrap();
        let first: EnvelopeV2 = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        write_verified(&path, b"same plaintext", &mut crypto).unwrap();
        let second: EnvelopeV2 = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();

        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ciphertext, second.ciphertext);
        remove_test_parent(&path);
    }

    #[test]
    fn v2_rejects_tampered_metadata_and_ciphertext_without_deleting_file() {
        let path = test_path("android-v2-tamper");
        let mut crypto = TestCrypto::default();
        write_verified(&path, b"authenticated secret", &mut crypto).unwrap();
        let original = std::fs::read(&path).unwrap();

        let mut envelope: EnvelopeV2 = serde_json::from_slice(&original).unwrap();
        envelope.version += 1;
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));
        assert!(path.exists());

        let mut envelope: EnvelopeV2 = serde_json::from_slice(&original).unwrap();
        envelope.format = "attacker-format".to_string();
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));

        let mut envelope: EnvelopeV2 = serde_json::from_slice(&original).unwrap();
        envelope.account = "attacker-account".to_string();
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));

        let mut envelope: EnvelopeV2 = serde_json::from_slice(&original).unwrap();
        let mut nonce = base64::engine::general_purpose::STANDARD
            .decode(&envelope.nonce)
            .unwrap();
        nonce[0] ^= 1;
        envelope.nonce = base64::engine::general_purpose::STANDARD.encode(nonce);
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::Crypto(CryptoErrorKind::AuthenticationFailed))
        ));

        let mut envelope: EnvelopeV2 = serde_json::from_slice(&original).unwrap();
        let mut ciphertext = base64::engine::general_purpose::STANDARD
            .decode(&envelope.ciphertext)
            .unwrap();
        ciphertext[0] ^= 1;
        envelope.ciphertext = base64::engine::general_purpose::STANDARD.encode(ciphertext);
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::Crypto(CryptoErrorKind::AuthenticationFailed))
        ));
        assert!(path.exists());
        remove_test_parent(&path);
    }

    #[test]
    fn empty_truncated_or_unknown_field_envelopes_fail_closed() {
        let path = test_path("android-v2-malformed");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut crypto = TestCrypto::default();

        std::fs::write(&path, b"").unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));

        std::fs::write(&path, br#"{"format":"openless-android-credentials""#).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));

        write_verified(&path, b"valid", &mut crypto).unwrap();
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        envelope["unexpected"] = serde_json::Value::Bool(true);
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));
        assert!(path.exists());
        remove_test_parent(&path);
    }

    #[test]
    fn legacy_base64_migrates_only_after_verified_round_trip() {
        let path = test_path("android-legacy-migration");
        let plaintext = br#"{"version":1,"active":{"asr":"volcengine","llm":"ark"}}"#;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = base64::engine::general_purpose::STANDARD.encode(plaintext);
        std::fs::write(&path, &legacy).unwrap();
        let mut crypto = TestCrypto::default();

        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Legacy(plaintext.to_vec())
        );
        crypto.fail_next_open = Some(CryptoErrorKind::TemporarilyUnavailable);
        assert!(write_verified(&path, plaintext, &mut crypto).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), legacy);

        let result = write_verified_with_fault(
            &path,
            plaintext,
            &mut crypto,
            &mut |stage| {
                if stage == WriteStage::AfterVerification {
                    Err(io::Error::other("injected post-verification failure"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), legacy);
        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Plaintext(plaintext.to_vec())
        );
        assert!(!verified_v2_temporary_path(&path).exists());
        remove_test_parent(&path);
    }

    #[test]
    fn verified_v2_commit_barrier_recovers_verified_temp_after_pre_rename_failure() {
        let path = test_path("android-v2-commit-barrier");
        let plaintext = br#"{"version":1,"active":{"llm":"ark"}}"#;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = base64::engine::general_purpose::STANDARD.encode(plaintext);
        std::fs::write(&path, &legacy).unwrap();
        let mut crypto = TestCrypto::default();

        let result = write_verified_with_fault(
            &path,
            plaintext,
            &mut crypto,
            &mut |stage| {
                if stage == WriteStage::AfterVerification {
                    Err(io::Error::other("injected pre-rename failure"))
                } else {
                    Ok(())
                }
            },
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), legacy);
        assert!(verified_v2_temporary_path(&path).exists());
        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Plaintext(plaintext.to_vec())
        );
        assert!(!verified_v2_temporary_path(&path).exists());
        remove_test_parent(&path);
    }

    #[test]
    fn verified_v2_recovery_candidate_survives_temporary_marker_failure() {
        let path = test_path("android-v2-marker-retry");
        let plaintext = br#"{"version":1,"active":{"llm":"ark"}}"#;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = base64::engine::general_purpose::STANDARD.encode(plaintext);
        std::fs::write(&path, &legacy).unwrap();
        let mut crypto = TestCrypto::default();
        crypto.fail_next_mark_migration = Some(CryptoErrorKind::TemporarilyUnavailable);

        assert!(write_verified(&path, plaintext, &mut crypto).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), legacy);
        assert!(verified_v2_temporary_path(&path).exists());
        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Plaintext(plaintext.to_vec())
        );
        assert!(!verified_v2_temporary_path(&path).exists());
        remove_test_parent(&path);
    }

    #[test]
    fn invalidated_key_clears_pending_v2_recovery_candidate() {
        let path = test_path("android-v2-pending-missing-key");
        let plaintext = br#"{"version":1,"active":{"llm":"ark"}}"#;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode(plaintext),
        )
        .unwrap();
        let mut crypto = TestCrypto::default();

        assert!(write_verified_with_fault(
            &path,
            plaintext,
            &mut crypto,
            &mut |stage| {
                if stage == WriteStage::AfterVerification {
                    Err(io::Error::other("inject pending recovery candidate"))
                } else {
                    Ok(())
                }
            },
        )
        .is_err());
        assert!(verified_v2_temporary_path(&path).exists());

        crypto.fail_next_open = Some(CryptoErrorKind::KeyMissingOrInvalidated);
        assert_eq!(read(&path, &mut crypto).unwrap(), ReadOutcome::Missing);
        assert!(!path.exists());
        assert!(!verified_v2_temporary_path(&path).exists());
        assert_eq!(crypto.delete_key_calls, 1);

        write_verified(&path, b"reconfigured", &mut crypto).unwrap();
        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Plaintext(b"reconfigured".to_vec())
        );
        remove_test_parent(&path);
    }

    #[test]
    fn successful_v2_rejects_legacy_base64_downgrade() {
        let path = test_path("android-v2-downgrade");
        let mut crypto = TestCrypto::default();
        write_verified(&path, b"authenticated value", &mut crypto).unwrap();
        let injected = base64::engine::general_purpose::STANDARD
            .encode(br#"{"version":1,"active":{"llm":"attacker"}}"#);
        std::fs::write(&path, &injected).unwrap();

        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::InvalidEnvelope)
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), injected);
        remove_test_parent(&path);
    }

    fn assert_token_absent_from_legacy_candidates(path: &Path, token: &str) {
        for candidate in [path.to_path_buf(), path.with_extension("legacy.tmp")] {
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            assert!(!String::from_utf8_lossy(&bytes).contains(token));
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&bytes) {
                assert!(!String::from_utf8_lossy(&decoded).contains(token));
            }
        }
    }

    #[test]
    fn legacy_bearer_sanitized_rewrite_is_fail_closed_for_every_io_failure() {
        let stages = [
            WriteStage::TempOpen,
            WriteStage::AfterTempOpen,
            WriteStage::AfterTempWrite,
            WriteStage::AfterTempSync,
            WriteStage::AfterVerification,
            WriteStage::Rename,
            WriteStage::AfterRename,
            WriteStage::ParentSync,
        ];

        for failed_stage in stages {
            let path = test_path(&format!("android-legacy-bearer-{failed_stage:?}"));
            let token = format!("gho_legacy_{failed_stage:?}");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let legacy_json =
                format!(r#"{{"marketplace":{{"githubAccessToken":"{token}"}}}}"#);
            let legacy = base64::engine::general_purpose::STANDARD.encode(legacy_json.as_bytes());
            std::fs::write(&path, legacy).unwrap();
            let sanitized = br#"{"version":1,"active":{"llm":"ark"}}"#;

            let result = rewrite_legacy_without_bearer_with_fault(
                &path,
                sanitized,
                &mut |stage| {
                    if stage == failed_stage {
                        Err(io::Error::other(format!("injected {stage:?}")))
                    } else {
                        Ok(())
                    }
                },
            );

            assert!(result.is_err(), "{failed_stage:?} should fail");
            assert_token_absent_from_legacy_candidates(&path, &token);
            if matches!(
                failed_stage,
                WriteStage::AfterVerification
                    | WriteStage::Rename
                    | WriteStage::AfterRename
                    | WriteStage::ParentSync
            ) {
                let mut crypto = TestCrypto::default();
                assert_eq!(
                    read(&path, &mut crypto).unwrap(),
                    ReadOutcome::Legacy(sanitized.to_vec()),
                    "{failed_stage:?} should preserve sanitized credentials"
                );
            }
            remove_test_parent(&path);
        }
    }

    #[test]
    fn legacy_bearer_is_unrecoverable_at_post_erase_crash_boundaries() {
        for crash_stage in [
            WriteStage::Rename,
            WriteStage::AfterRename,
            WriteStage::ParentSync,
        ] {
            let path = test_path(&format!("android-legacy-crash-{crash_stage:?}"));
            let token = format!("gho_crash_{crash_stage:?}");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let legacy_json =
                format!(r#"{{"marketplace":{{"githubAccessToken":"{token}"}}}}"#);
            let legacy = base64::engine::general_purpose::STANDARD.encode(legacy_json.as_bytes());
            std::fs::write(&path, legacy).unwrap();

            let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = rewrite_legacy_without_bearer_with_fault(
                    &path,
                    br#"{"version":1}"#,
                    &mut |stage| {
                        assert_ne!(stage, crash_stage, "injected crash at {stage:?}");
                        Ok(())
                    },
                );
            }));

            assert!(crashed.is_err(), "{crash_stage:?} should simulate a crash");
            assert_token_absent_from_legacy_candidates(&path, &token);
            let mut crypto = TestCrypto::default();
            assert_eq!(
                read(&path, &mut crypto).unwrap(),
                ReadOutcome::Legacy(br#"{"version":1}"#.to_vec()),
                "{crash_stage:?} should recover sanitized credentials"
            );
            remove_test_parent(&path);
        }
    }

    #[test]
    fn truncated_legacy_target_recovers_verified_sanitized_copy() {
        let path = test_path("android-legacy-truncated-recovery");
        let sanitized = br#"{"version":1,"active":{"llm":"ark"}}"#;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"").unwrap();
        std::fs::write(
            path.with_extension("legacy.tmp"),
            base64::engine::general_purpose::STANDARD.encode(sanitized),
        )
        .unwrap();
        let mut crypto = TestCrypto::default();

        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Legacy(sanitized.to_vec())
        );
        assert!(!path.with_extension("legacy.tmp").exists());
        remove_test_parent(&path);
    }

    #[test]
    fn atomic_update_faults_leave_one_complete_envelope() {
        let path = test_path("android-atomic-faults");
        let mut crypto = TestCrypto::default();
        let stages = [
            WriteStage::TempOpen,
            WriteStage::AfterTempOpen,
            WriteStage::AfterTempWrite,
            WriteStage::AfterTempSync,
            WriteStage::AfterVerification,
            WriteStage::Rename,
            WriteStage::AfterRename,
            WriteStage::ParentSync,
        ];

        for failed_stage in stages {
            write_verified(&path, b"old complete value", &mut crypto).unwrap();
            let result = write_verified_with_fault(
                &path,
                b"new complete value",
                &mut crypto,
                &mut |stage| {
                    if stage == failed_stage {
                        Err(io::Error::other(format!("injected {stage:?}")))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err(), "{failed_stage:?} should fail");
            let expected: &[u8] = if matches!(
                failed_stage,
                WriteStage::AfterVerification
                    | WriteStage::Rename
                    | WriteStage::AfterRename
                    | WriteStage::ParentSync
            ) {
                b"new complete value"
            } else {
                b"old complete value"
            };
            assert_eq!(
                read(&path, &mut crypto).unwrap(),
                ReadOutcome::Plaintext(expected.to_vec()),
                "{failed_stage:?} left an incomplete envelope"
            );
            assert!(!path.with_extension("json.tmp").exists());
            assert!(!verified_v2_temporary_path(&path).exists());
        }
        remove_test_parent(&path);
    }

    #[test]
    fn transient_keystore_failure_is_retryable_and_preserves_ciphertext() {
        let path = test_path("android-transient-keystore");
        let mut crypto = TestCrypto::default();
        write_verified(&path, b"retry me", &mut crypto).unwrap();
        let disk = std::fs::read(&path).unwrap();

        crypto.fail_next_open = Some(CryptoErrorKind::TemporarilyUnavailable);
        assert!(matches!(
            read(&path, &mut crypto),
            Err(StoreError::Crypto(CryptoErrorKind::TemporarilyUnavailable))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        assert_eq!(crypto.delete_key_calls, 0);
        assert_eq!(
            read(&path, &mut crypto).unwrap(),
            ReadOutcome::Plaintext(b"retry me".to_vec())
        );
        remove_test_parent(&path);
    }

    #[test]
    fn errors_never_include_plaintext_secrets() {
        let path = test_path("android-secret-free-errors");
        let secret = b"sk-never-include-this-value";
        let mut crypto = TestCrypto::default();
        crypto.fail_next_open = Some(CryptoErrorKind::TemporarilyUnavailable);

        let error = write_verified(&path, secret, &mut crypto).unwrap_err();

        assert!(!error.to_string().contains("sk-never-include-this-value"));
        remove_test_parent(&path);
    }

    #[test]
    fn missing_key_clears_unrecoverable_ciphertext() {
        let path = test_path("android-missing-key");
        let mut crypto = TestCrypto::default();
        write_verified(&path, b"unrecoverable", &mut crypto).unwrap();

        crypto.fail_next_open = Some(CryptoErrorKind::KeyMissingOrInvalidated);
        assert_eq!(read(&path, &mut crypto).unwrap(), ReadOutcome::Missing);
        assert!(!path.exists());
        assert_eq!(crypto.delete_key_calls, 1);
        remove_test_parent(&path);
    }

    #[cfg(unix)]
    #[test]
    fn credential_file_is_owner_private() {
        let path = test_path("android-owner-private");
        let mut crypto = TestCrypto::default();
        write_verified(&path, b"private", &mut crypto).unwrap();

        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
        remove_test_parent(&path);
    }
}
