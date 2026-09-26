//! Strict v1 DTOs. Constructors/deserializers preserve canonical wire encodings.

use super::{Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{
    de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fmt};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const PROTOCOL_VERSION: u32 = 1;
pub(crate) const CRYPTO_PROFILE: &str = "argon2id-xchacha20poly1305-v1";
pub(crate) const AEAD_NAME: &str = "xchacha20poly1305-ietf";
pub(crate) const CODEC_NAME: &str = "json-pad64k-v1";
pub(crate) const HTTP_BODY_LIMIT: usize = 24 * 1024 * 1024;
pub(crate) const CONTROL_BODY_LIMIT: usize = 8192;
pub(crate) const MAX_PLAINTEXT_JSON_BYTES: usize = 15 * 1024 * 1024;
pub(crate) const MAX_PADDED_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_CIPHERTEXT_BYTES: usize = MAX_PADDED_BYTES + 16;
pub(crate) const PAD_BLOCK_BYTES: usize = 65_536;
pub(crate) const MAX_DOCUMENTS: usize = 100_000;
pub(crate) const MAX_JSON_DEPTH: usize = 64;

pub(crate) trait Validate {
    fn validate(&self) -> Result<()>;
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Revision {
    value: u64,
    wire: [u8; 20],
    length: u8,
}
impl Revision {
    pub fn new(value: u64) -> Self {
        let text = value.to_string();
        let mut wire = [b'0'; 20];
        wire[..text.len()].copy_from_slice(text.as_bytes());
        Self {
            value,
            wire,
            length: text.len() as u8,
        }
    }
    pub(crate) fn parse(wire: &str) -> Result<Self> {
        if wire.is_empty()
            || wire.len() > 20
            || !wire.bytes().all(|c| c.is_ascii_digit())
            || (wire.len() > 1 && wire.starts_with('0'))
        {
            return Err(Error::InvalidWire("revision"));
        }
        let value = wire.parse().map_err(|_| Error::InvalidWire("revision"))?;
        Ok(Self::new(value))
    }
    pub fn get(&self) -> u64 {
        self.value
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.wire[..usize::from(self.length)])
            .expect("revision constructor emits ASCII digits")
    }
    pub(crate) fn checked_next(&self) -> Result<Self> {
        self.value
            .checked_add(1)
            .map(Self::new)
            .ok_or(Error::RevisionExhausted)
    }
}
impl fmt::Debug for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl Serialize for Revision {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(|_| de::Error::custom("invalid revision"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct GithubId(Revision);
impl GithubId {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        let n = Revision::parse(s)?;
        if n.get() == 0 {
            return Err(Error::InvalidWire("github id"));
        }
        Ok(Self(n))
    }
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl fmt::Display for GithubId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Serialize for GithubId {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for GithubId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(|_| de::Error::custom("invalid github id"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct UuidV4(String);
impl UuidV4 {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        if s.len() != 36 {
            return Err(Error::InvalidWire("uuid v4"));
        }
        let id = uuid::Uuid::parse_str(s).map_err(|_| Error::InvalidWire("uuid v4"))?;
        if id.get_version() != Some(uuid::Version::Random)
            || id.get_variant() != uuid::Variant::RFC4122
            || id.to_string() != s
        {
            return Err(Error::InvalidWire("uuid v4"));
        }
        Ok(Self(s.to_owned()))
    }
    pub(crate) fn random() -> Result<Self> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| Error::RandomUnavailable)?;
        Ok(Self(
            uuid::Builder::from_random_bytes(bytes)
                .into_uuid()
                .to_string(),
        ))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for UuidV4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Serialize for UuidV4 {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for UuidV4 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(|_| de::Error::custom("invalid uuid v4"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StrongEtag(String);
impl StrongEtag {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        let b = s.as_bytes();
        if !(3..=130).contains(&b.len())
            || b[0] != b'"'
            || b[b.len() - 1] != b'"'
            || !b[1..b.len() - 1]
                .iter()
                .all(|b| *b == 0x21 || (0x23..=0x7e).contains(b))
        {
            return Err(Error::InvalidWire("strong etag"));
        }
        Ok(Self(s.to_owned()))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for StrongEtag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct EncodedBytes<const N: usize>([u8; N]);
pub(crate) type Salt = EncodedBytes<16>;
pub(crate) type Nonce = EncodedBytes<24>;
impl<const N: usize> EncodedBytes<N> {
    #[cfg(test)]
    pub(crate) fn new(bytes: [u8; N]) -> Self {
        Self(bytes)
    }
    pub(crate) fn random() -> Result<Self> {
        let mut b = [0; N];
        getrandom::fill(&mut b).map_err(|_| Error::RandomUnavailable)?;
        Ok(Self(b))
    }
    pub(crate) fn bytes(&self) -> &[u8; N] {
        &self.0
    }
    pub(crate) fn encoded(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }
    pub(crate) fn parse(s: &str) -> Result<Self> {
        if s.len() != N.div_ceil(3) * 4 - (3 - N % 3) % 3 {
            return Err(Error::InvalidWire("base64 length"));
        }
        let b = decode_base64(s, N)?;
        Ok(Self(
            b.try_into()
                .map_err(|_| Error::InvalidWire("base64 length"))?,
        ))
    }
}
impl<const N: usize> fmt::Debug for EncodedBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PublicEncodedBytes")
            .field(&self.encoded())
            .finish()
    }
}
impl<const N: usize> Serialize for EncodedBytes<N> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.encoded())
    }
}
impl<'de, const N: usize> Deserialize<'de> for EncodedBytes<N> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?)
            .map_err(|_| de::Error::custom("invalid canonical base64"))
    }
}

pub(crate) fn decode_base64(s: &str, max: usize) -> Result<Vec<u8>> {
    if s.is_empty()
        || s.len() > max.div_ceil(3) * 4
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::InvalidWire("canonical base64"));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| Error::InvalidWire("canonical base64"))?;
    if decoded.len() > max {
        return Err(Error::PayloadTooLarge);
    }
    if URL_SAFE_NO_PAD.encode(&decoded) != s {
        return Err(Error::InvalidWire("canonical base64"));
    }
    Ok(decoded)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Sha256Digest([u8; 32]);
impl Sha256Digest {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    pub(crate) fn as_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
    pub(crate) fn parse(s: &str) -> Result<Self> {
        if s.len() != 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidWire("sha256"));
        }
        let mut out = [0; 32];
        for (n, pair) in s.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            out[n] = ((hex(pair[0])) << 4) | hex(pair[1]);
        }
        Ok(Self(out))
    }
}
fn hex(b: u8) -> u8 {
    if b <= b'9' {
        b - b'0'
    } else {
        b - b'a' + 10
    }
}
impl Serialize for Sha256Digest {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.as_hex())
    }
}
impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(|_| de::Error::custom("invalid sha256"))
    }
}

/// Intentionally neither serializable nor cloneable. Moving an IPC String here
/// transfers ownership; all copies created by normalization are also zeroizing.
pub(crate) struct SecretInput(Zeroizing<String>);
impl SecretInput {
    pub(crate) fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretInput([REDACTED])")
    }
}

pub(crate) struct SecretToken(Zeroizing<String>);
impl SecretToken {
    pub(crate) fn new(value: String) -> Result<Self> {
        let value = Zeroizing::new(value);
        if value.is_empty() || value.len() > 2048 || !value.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::InvalidWire("bearer token"));
        }
        Ok(Self(value))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
    pub(crate) fn validate_sync(&self) -> Result<()> {
        if self.0.len() < 43 {
            Err(Error::InvalidWire("sync token"))
        } else {
            Ok(())
        }
    }
}
impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken([REDACTED])")
    }
}
impl<'de> Deserialize<'de> for SecretToken {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(|_| de::Error::custom("invalid token"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Kdf {
    pub(crate) name: String,
    pub(crate) version: u32,
    #[serde(rename = "memoryKiB")]
    pub(crate) memory_kib: u32,
    pub(crate) iterations: u32,
    pub(crate) parallelism: u32,
    pub(crate) salt: Salt,
}
impl Kdf {
    pub(crate) fn fixed(salt: Salt) -> Self {
        Self {
            name: "argon2id".into(),
            version: 19,
            memory_kib: 65536,
            iterations: 3,
            parallelism: 4,
            salt,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.name != "argon2id"
            || self.version != 19
            || self.memory_kib != 65536
            || self.iterations != 3
            || self.parallelism != 4
        {
            Err(Error::UnsupportedProtocol)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UploadKind {
    Create,
    Snapshot,
    PasswordChange,
}
impl UploadKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Snapshot => "snapshot",
            Self::PasswordChange => "password_change",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationKind {
    Create,
    Snapshot,
    PasswordChange,
    Delete,
}
impl From<UploadKind> for OperationKind {
    fn from(v: UploadKind) -> Self {
        match v {
            UploadKind::Create => Self::Create,
            UploadKind::Snapshot => Self::Snapshot,
            UploadKind::PasswordChange => Self::PasswordChange,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SnapshotUpload {
    pub(crate) protocol_version: u32,
    pub(crate) payload_schema_version: u32,
    pub(crate) owner_github_id: GithubId,
    pub(crate) vault_id: UuidV4,
    pub(crate) key_id: UuidV4,
    pub(crate) base_revision: Revision,
    pub(crate) revision: Revision,
    pub(crate) operation_id: UuidV4,
    pub(crate) kind: UploadKind,
    pub(crate) crypto_profile: String,
    pub(crate) kdf: Kdf,
    pub(crate) aead: String,
    pub(crate) codec: String,
    pub(crate) nonce: Nonce,
    pub(crate) ciphertext: String,
    pub(crate) ciphertext_sha256: Sha256Digest,
}
impl fmt::Debug for SnapshotUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotUpload")
            .field("revision", &self.revision)
            .field("kind", &self.kind)
            .field("ciphertext", &"[REDACTED]")
            .finish()
    }
}
impl SnapshotUpload {
    pub(crate) fn validate_header(&self) -> Result<()> {
        if self.protocol_version != PROTOCOL_VERSION
            || self.payload_schema_version != 1
            || self.crypto_profile != CRYPTO_PROFILE
            || self.aead != AEAD_NAME
            || self.codec != CODEC_NAME
        {
            return Err(Error::UnsupportedProtocol);
        }
        self.kdf.validate()?;
        if self.revision != self.base_revision.checked_next()? {
            return Err(Error::InvalidWire("revision increment"));
        }
        if !(87_403..=22_369_643).contains(&self.ciphertext.len()) {
            return Err(Error::InvalidWire("ciphertext length"));
        }
        Ok(())
    }
    pub(crate) fn decoded_ciphertext(&self) -> Result<Vec<u8>> {
        self.validate_header()?;
        let bytes = decode_base64(&self.ciphertext, MAX_CIPHERTEXT_BYTES)?;
        if bytes.len() < PAD_BLOCK_BYTES + 16 || (bytes.len() - 16) % PAD_BLOCK_BYTES != 0 {
            return Err(Error::InvalidWire("ciphertext framing"));
        }
        if Sha256Digest::of(&bytes) != self.ciphertext_sha256 {
            return Err(Error::InvalidWire("ciphertext hash"));
        }
        Ok(bytes)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.decoded_ciphertext().map(|_| ())
    }
    pub(crate) fn validate_against_metadata(
        &self,
        meta: &VaultMetadata,
        owner: &GithubId,
    ) -> Result<()> {
        meta.validate()?;
        if &meta.owner_github_id != owner || &self.owner_github_id != owner {
            return Err(Error::AccountMismatch);
        }
        if meta.state != VaultState::Active
            || self.revision != meta.revision
            || Some(&self.vault_id) != meta.vault_id.as_ref()
            || Some(&self.key_id) != meta.key_id.as_ref()
            || Some(&self.operation_id) != meta.last_operation_id.as_ref()
            || Some(self.payload_schema_version) != meta.payload_schema_version
            || Some(&self.ciphertext_sha256) != meta.ciphertext_sha256.as_ref()
        {
            return Err(Error::ContextMismatch);
        }
        let bytes = self.decoded_ciphertext()?;
        if bytes.len() as u64 != meta.ciphertext_bytes {
            return Err(Error::ContextMismatch);
        }
        Ok(())
    }
}
impl Validate for SnapshotUpload {
    fn validate(&self) -> Result<()> {
        self.validate()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Capabilities {
    pub(crate) protocol_version: u32,
    pub(crate) crypto_profile: String,
    pub(crate) github_client_id: String,
    pub(crate) max_http_body_bytes: u64,
    pub(crate) max_ciphertext_bytes: u64,
    pub(crate) max_plaintext_json_bytes: u64,
    pub(crate) idempotency_retention_seconds: u64,
    pub(crate) max_backup_retention_days: u32,
}
impl Validate for Capabilities {
    fn validate(&self) -> Result<()> {
        if self.protocol_version != PROTOCOL_VERSION
            || self.crypto_profile != CRYPTO_PROFILE
            || self.max_http_body_bytes != HTTP_BODY_LIMIT as u64
            || self.max_ciphertext_bytes != MAX_CIPHERTEXT_BYTES as u64
            || self.max_plaintext_json_bytes != MAX_PLAINTEXT_JSON_BYTES as u64
            || self.idempotency_retention_seconds < 604800
            || self.max_backup_retention_days > 30
        {
            return Err(Error::UnsupportedProtocol);
        }
        bounded_text(&self.github_client_id, 1, 128)
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Account {
    pub(crate) github_id: GithubId,
    pub(crate) login: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AuthSession {
    pub(crate) protocol_version: u32,
    pub(crate) access_token: SecretToken,
    pub(crate) token_type: String,
    pub(crate) expires_in: u32,
    pub(crate) account: Account,
}
impl Validate for AuthSession {
    fn validate(&self) -> Result<()> {
        if self.protocol_version != PROTOCOL_VERSION
            || self.token_type != "Bearer"
            || !(1..=900).contains(&self.expires_in)
        {
            return Err(Error::InvalidWire("auth session"));
        }
        self.access_token.validate_sync()?;
        bounded_text(&self.account.login, 1, 100)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VaultState {
    Empty,
    Active,
    Deleted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VaultMetadata {
    pub(crate) protocol_version: u32,
    pub(crate) state: VaultState,
    pub(crate) owner_github_id: GithubId,
    pub(crate) revision: Revision,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) vault_id: Option<UuidV4>,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) key_id: Option<UuidV4>,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) updated_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) payload_schema_version: Option<u32>,
    pub(crate) ciphertext_bytes: u64,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) ciphertext_sha256: Option<Sha256Digest>,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) last_operation_id: Option<UuidV4>,
}
fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
impl VaultMetadata {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(Error::UnsupportedProtocol);
        }
        if let Some(t) = &self.updated_at {
            validate_timestamp(t)?;
        }
        let valid = match self.state {
            VaultState::Empty => {
                self.revision.get() == 0
                    && self.vault_id.is_none()
                    && self.key_id.is_none()
                    && self.updated_at.is_none()
                    && self.payload_schema_version.is_none()
                    && self.ciphertext_bytes == 0
                    && self.ciphertext_sha256.is_none()
                    && self.last_operation_id.is_none()
            }
            VaultState::Active => {
                self.revision.get() > 0
                    && self.vault_id.is_some()
                    && self.key_id.is_some()
                    && self.updated_at.is_some()
                    && self.payload_schema_version == Some(1)
                    && self.ciphertext_bytes >= 65_552
                    && self.ciphertext_bytes <= MAX_CIPHERTEXT_BYTES as u64
                    && (self.ciphertext_bytes - 16).is_multiple_of(PAD_BLOCK_BYTES as u64)
                    && self.ciphertext_sha256.is_some()
                    && self.last_operation_id.is_some()
            }
            VaultState::Deleted => {
                self.revision.get() > 0
                    && self.vault_id.is_some()
                    && self.key_id.is_none()
                    && self.updated_at.is_some()
                    && self.payload_schema_version.is_none()
                    && self.ciphertext_bytes == 0
                    && self.ciphertext_sha256.is_none()
                    && self.last_operation_id.is_some()
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::InvalidWire("vault state"))
        }
    }
}
impl Validate for VaultMetadata {
    fn validate(&self) -> Result<()> {
        self.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeleteVaultRequest {
    pub(crate) protocol_version: u32,
    pub(crate) base_revision: Revision,
    pub(crate) operation_id: UuidV4,
    pub(crate) expected_vault_id: UuidV4,
}
impl Validate for DeleteVaultRequest {
    fn validate(&self) -> Result<()> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(Error::UnsupportedProtocol);
        }
        if self.base_revision.get() == 0 {
            return Err(Error::InvalidWire("delete revision"));
        }
        self.base_revision.checked_next().map(|_| ())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OperationReceipt {
    pub(crate) operation_id: UuidV4,
    pub(crate) status: String,
    pub(crate) kind: OperationKind,
    pub(crate) committed_revision: Revision,
    pub(crate) committed_at: String,
    pub(crate) vault_id: UuidV4,
    #[serde(deserialize_with = "required_nullable")]
    pub(crate) ciphertext_sha256: Option<Sha256Digest>,
}
impl Validate for OperationReceipt {
    fn validate(&self) -> Result<()> {
        if self.status != "committed"
            || self.committed_revision.get() == 0
            || (self.kind == OperationKind::Delete) != self.ciphertext_sha256.is_none()
        {
            return Err(Error::InvalidWire("receipt"));
        }
        validate_timestamp(&self.committed_at)
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OperationPending {
    pub(crate) operation_id: UuidV4,
    pub(crate) status: String,
    pub(crate) retry_after_seconds: u32,
}
impl Validate for OperationPending {
    fn validate(&self) -> Result<()> {
        if self.status != "pending" || !(1..=3600).contains(&self.retry_after_seconds) {
            Err(Error::InvalidWire("pending receipt"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RemoteErrorCode {
    InvalidRequest,
    UnsupportedProtocol,
    InvalidCryptoHeader,
    Unauthenticated,
    SessionExpired,
    WrongOauthApp,
    OwnerMismatch,
    VaultEmpty,
    VaultDeleted,
    OperationNotFound,
    IdempotencyKeyReused,
    KeyEpochChanged,
    VaultExists,
    RevisionExhausted,
    RevisionConflict,
    PayloadTooLarge,
    UnsupportedMediaType,
    PreconditionRequired,
    RateLimited,
    InternalError,
    ServiceUnavailable,
}
impl fmt::Display for RemoteErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_string(self).map_err(|_| fmt::Error)?;
        f.write_str(s.trim_matches('"'))
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ErrorEnvelope {
    pub(crate) error: ErrorBody,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ErrorBody {
    pub(crate) code: RemoteErrorCode,
    pub(crate) message: String,
    // Parse the required UUID, but never include a remote body in diagnostics.
    #[serde(rename = "requestId")]
    _request_id: UuidV4,
    #[serde(default, deserialize_with = "optional_present_nonnull")]
    pub(crate) current_revision: Option<Revision>,
}
fn optional_present_nonnull<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
impl Drop for ErrorBody {
    fn drop(&mut self) {
        self.message.zeroize();
    }
}
impl Validate for ErrorEnvelope {
    fn validate(&self) -> Result<()> {
        bounded_text(&self.error.message, 1, 200)
    }
}

#[derive(Clone, Copy, Debug, Hash, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Preferences,
    UiPreferences,
    Channels,
    ProviderCredentials,
    Dictionary,
    VocabularyPresets,
    Corrections,
    StylePacks,
    History,
    Activity,
    DeviceProfile,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceDevice {
    pub id: String,
    pub os: String,
    pub arch: String,
    pub app_version: String,
}
impl Drop for SourceDevice {
    fn drop(&mut self) {
        self.id.zeroize();
        self.os.zeroize();
        self.arch.zeroize();
        self.app_version.zeroize();
    }
}
impl fmt::Debug for SourceDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SourceDevice([REDACTED])")
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogicalDocument {
    pub id: String,
    pub kind: DocumentKind,
    pub schema_version: u32,
    pub value: Value,
}
impl Drop for LogicalDocument {
    fn drop(&mut self) {
        self.id.zeroize();
        erase_value(&mut self.value);
    }
}
impl fmt::Debug for LogicalDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LogicalDocument([REDACTED])")
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tombstone {
    pub id: String,
    pub kind: DocumentKind,
    pub deleted_at: String,
    pub base_revision: Revision,
}
impl Drop for Tombstone {
    fn drop(&mut self) {
        self.id.zeroize();
        self.deleted_at.zeroize();
    }
}
impl fmt::Debug for Tombstone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Tombstone([REDACTED])")
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentSet {
    pub schema_version: u32,
    pub exported_at: String,
    pub source_device: SourceDevice,
    pub documents: Vec<LogicalDocument>,
    pub tombstones: Vec<Tombstone>,
}
impl fmt::Debug for DocumentSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DocumentSet([REDACTED])")
    }
}
impl Drop for DocumentSet {
    fn drop(&mut self) {
        self.exported_at.zeroize();
    }
}
impl DocumentSet {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.documents.iter().any(|d| d.schema_version != 1) {
            return Err(Error::UnsupportedDocumentVersion);
        }
        if self.documents.len().saturating_add(self.tombstones.len()) > MAX_DOCUMENTS {
            return Err(Error::PayloadTooLarge);
        }
        validate_timestamp(&self.exported_at)?;
        if [
            &self.source_device.id,
            &self.source_device.os,
            &self.source_device.arch,
            &self.source_device.app_version,
        ]
        .iter()
        .any(|v| v.is_empty())
        {
            return Err(Error::InvalidWire("source device"));
        }
        let mut ids = HashSet::new();
        for d in &self.documents {
            validate_value_depth(&d.value, 3)?;
            if d.id.is_empty() || !ids.insert((d.kind, d.id.as_str())) {
                return Err(Error::InvalidWire("duplicate document identity"));
            }
        }
        for d in &self.tombstones {
            validate_timestamp(&d.deleted_at)?;
            if d.id.is_empty() || !ids.insert((d.kind, d.id.as_str())) {
                return Err(Error::InvalidWire("duplicate document identity"));
            }
        }
        Ok(())
    }
    pub(crate) fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        let mut value = parse_strict_json(bytes, MAX_PLAINTEXT_JSON_BYTES)?;
        // Future document kinds/versions require an upgrade; never erase them as
        // an empty successful restore. This preflight does not inspect value keys.
        if value
            .0
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .is_some_and(|v| v != 1)
        {
            return Err(Error::UnsupportedDocumentVersion);
        }
        for collection in ["documents", "tombstones"] {
            if let Some(items) = value.0.get(collection).and_then(Value::as_array) {
                if items.len() > MAX_DOCUMENTS {
                    return Err(Error::PayloadTooLarge);
                }
                for item in items {
                    if let Some(kind) = item.get("kind").and_then(Value::as_str) {
                        if serde_json::from_value::<DocumentKind>(Value::String(kind.into()))
                            .is_err()
                        {
                            return Err(Error::UnsupportedDocumentVersion);
                        }
                    }
                    if item
                        .get("schemaVersion")
                        .and_then(Value::as_u64)
                        .is_some_and(|v| v != 1)
                    {
                        return Err(Error::UnsupportedDocumentVersion);
                    }
                }
            }
        }
        let documents: Self = serde_json::from_value(std::mem::take(&mut value.0))
            .map_err(|_| Error::InvalidWire("document schema"))?;
        documents.validate()?;
        Ok(documents)
    }
    pub(crate) fn to_secret_json(&self) -> Result<Zeroizing<Vec<u8>>> {
        self.validate()?;
        bounded_secret_json(self, MAX_PLAINTEXT_JSON_BYTES)
    }
}
impl Validate for DocumentSet {
    fn validate(&self) -> Result<()> {
        self.validate()
    }
}
pub(crate) fn validate_timestamp(value: &str) -> Result<()> {
    if value.len() > 64 || !(value.ends_with('Z') || value.ends_with("+00:00")) {
        return Err(Error::InvalidWire("utc timestamp"));
    }
    let t = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| Error::InvalidWire("utc timestamp"))?;
    if t.offset().local_minus_utc() != 0 {
        return Err(Error::InvalidWire("utc timestamp"));
    }
    Ok(())
}
fn bounded_text(s: &str, min: usize, max: usize) -> Result<()> {
    let n = s.chars().count();
    if (min..=max).contains(&n) {
        Ok(())
    } else {
        Err(Error::InvalidWire("text length"))
    }
}

fn validate_value_depth(value: &Value, depth: usize) -> Result<()> {
    match value {
        Value::Array(items) => {
            if depth >= MAX_JSON_DEPTH {
                return Err(Error::InvalidWire("json depth"));
            }
            for item in items {
                validate_value_depth(item, depth + 1)?;
            }
        }
        Value::Object(items) => {
            if depth >= MAX_JSON_DEPTH {
                return Err(Error::InvalidWire("json depth"));
            }
            for item in items.values() {
                validate_value_depth(item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn bounded_secret_json<T: Serialize>(value: &T, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    // Count with a hard ceiling first, so serialization never grows a secret
    // buffer beyond the limit or leaves old plaintext allocations after realloc.
    struct Counter {
        size: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.size = self
                .size
                .checked_add(bytes.len())
                .filter(|v| *v <= self.limit)
                .ok_or_else(|| std::io::Error::other("JSON limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { size: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(|_| Error::PayloadTooLarge)?;
    struct Writer {
        bytes: Zeroizing<Vec<u8>>,
        limit: usize,
    }
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) > self.limit {
                return Err(std::io::Error::other("JSON limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Zeroizing::new(Vec::with_capacity(counter.size)),
        limit: counter.size,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| Error::InvalidWire("document serialization"))?;
    Ok(writer.bytes)
}

/// Best-effort cleanup of decrypted strings, including unknown extension keys.
fn erase_value(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => {
            for v in a {
                erase_value(v)
            }
        }
        Value::Object(m) => {
            for (mut k, mut v) in std::mem::take(m) {
                k.zeroize();
                erase_value(&mut v)
            }
        }
        _ => {}
    }
    *value = Value::Null;
}

struct StrictValue(Value);
impl Drop for StrictValue {
    fn drop(&mut self) {
        erase_value(&mut self.0);
    }
}
struct ValueSeed(usize);
impl<'de> DeserializeSeed<'de> for ValueSeed {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for ValueSeed {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("nonfinite JSON"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        if self.0 >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON depth"));
        }
        let mut out = StrictValue(Value::Array(Vec::new()));
        while let Some(mut v) = a.next_element_seed(ValueSeed(self.0 + 1))? {
            if let Value::Array(items) = &mut out.0 {
                if self.0 == 1 && items.len() >= MAX_DOCUMENTS {
                    erase_value(&mut v);
                    return Err(de::Error::custom("document count"));
                }
                items.push(v);
            }
        }
        Ok(std::mem::take(&mut out.0))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        if self.0 >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON depth"));
        }
        let mut out = StrictValue(Value::Object(Map::new()));
        while let Some(mut key) = a.next_key::<String>()? {
            if let Value::Object(items) = &mut out.0 {
                if items.contains_key(&key) {
                    key.zeroize();
                    return Err(de::Error::custom("duplicate JSON key"));
                }
            }
            let value = match a.next_value_seed(ValueSeed(self.0 + 1)) {
                Ok(v) => v,
                Err(e) => {
                    key.zeroize();
                    return Err(e);
                }
            };
            if let Value::Object(items) = &mut out.0 {
                items.insert(key, value);
            }
        }
        Ok(std::mem::take(&mut out.0))
    }
}
fn parse_strict_json(bytes: &[u8], limit: usize) -> Result<StrictValue> {
    if bytes.len() > limit {
        return Err(Error::PayloadTooLarge);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue(
        ValueSeed(0)
            .deserialize(&mut d)
            .map_err(|_| Error::InvalidWire("json encoding"))?,
    );
    d.end()
        .map_err(|_| Error::InvalidWire("json trailing data"))?;
    Ok(value)
}
pub(crate) fn parse_wire<T: DeserializeOwned + Validate>(bytes: &[u8], limit: usize) -> Result<T> {
    if bytes.len() > limit {
        return Err(Error::PayloadTooLarge);
    }
    // Validate syntax without constructing an attacker-chosen Value tree (for
    // example millions of `null`s where ciphertext must actually be a string).
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    SyntaxSeed(0)
        .deserialize(&mut parser)
        .map_err(|_| Error::InvalidWire("json encoding"))?;
    parser
        .end()
        .map_err(|_| Error::InvalidWire("json trailing data"))?;
    let value =
        serde_json::from_slice::<T>(bytes).map_err(|_| Error::InvalidWire("json schema"))?;
    value.validate()?;
    Ok(value)
}

struct SyntaxSeed(usize);
impl<'de> DeserializeSeed<'de> for SyntaxSeed {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}
struct SecretKeys(HashSet<String>);
impl Drop for SecretKeys {
    fn drop(&mut self) {
        for mut key in self.0.drain() {
            key.zeroize();
        }
    }
}
impl<'de> Visitor<'de> for SyntaxSeed {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<(), E> {
        if v.is_finite() {
            Ok(())
        } else {
            Err(E::custom("nonfinite JSON"))
        }
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_string<E: de::Error>(self, mut v: String) -> std::result::Result<(), E> {
        v.zeroize();
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<(), A::Error> {
        if self.0 >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON depth"));
        }
        while a.next_element_seed(SyntaxSeed(self.0 + 1))?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<(), A::Error> {
        if self.0 >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON depth"));
        }
        let mut keys = SecretKeys(HashSet::new());
        while let Some(mut key) = a.next_key::<String>()? {
            if keys.0.contains(&key) {
                key.zeroize();
                return Err(de::Error::custom("duplicate JSON key"));
            }
            keys.0.insert(key);
            a.next_value_seed(SyntaxSeed(self.0 + 1))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
