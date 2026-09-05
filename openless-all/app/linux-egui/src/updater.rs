//! Verified AppImage replacement primitives.
//!
//! Network transport and minisign verification are intentionally injected.
//! The current Linux crate has neither an HTTP client nor a minisign verifier;
//! callers cannot accidentally turn either missing capability into success.

use serde::Deserialize;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const MANIFEST_HOST: &str = "linux-egui";
pub const DEFAULT_MAX_APPIMAGE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub schema_version: u32,
    pub host: String,
    pub arch: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub minisign: Option<String>,
}

impl UpdateManifest {
    pub fn parse(json: &[u8], expected_arch: &str) -> Result<Self, UpdateError> {
        let manifest: Self = serde_json::from_slice(json).map_err(UpdateError::ManifestJson)?;
        manifest.validate(expected_arch)?;
        Ok(manifest)
    }

    pub fn validate(&self, expected_arch: &str) -> Result<(), UpdateError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(UpdateError::InvalidManifest(format!(
                "unsupported updater schema version {}",
                self.schema_version
            )));
        }
        if self.host != MANIFEST_HOST {
            return Err(UpdateError::InvalidManifest(format!(
                "manifest is for host {:?}, not {:?}",
                self.host, MANIFEST_HOST
            )));
        }
        if self.arch != expected_arch {
            return Err(UpdateError::InvalidManifest(format!(
                "manifest architecture {:?} does not match {:?}",
                self.arch, expected_arch
            )));
        }
        if self.version.trim().is_empty()
            || self.version.contains(['\0', '\n', '\r'])
            || self.url.chars().any(char::is_control)
        {
            return Err(UpdateError::InvalidManifest(
                "manifest version or URL is empty/unsafe".into(),
            ));
        }
        if !is_github_release_url(&self.url) {
            return Err(UpdateError::InvalidManifest(
                "artifact URL must be a GitHub HTTPS release asset".into(),
            ));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(UpdateError::InvalidManifest(
                "manifest SHA-256 must contain exactly 64 hexadecimal digits".into(),
            ));
        }
        if self
            .minisign
            .as_deref()
            .is_some_and(|signature| signature.trim().is_empty() || signature.contains('\0'))
        {
            return Err(UpdateError::InvalidManifest(
                "manifest minisign value is empty or unsafe".into(),
            ));
        }
        Ok(())
    }

    pub fn has_new_version(&self, current: &str) -> bool {
        normalize_version(&self.version) != normalize_version(current)
    }
}

fn normalize_version(version: &str) -> &str {
    version.trim().strip_prefix('v').unwrap_or(version.trim())
}

fn is_github_release_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let mut segments = rest.split('/');
    let owner = segments.next().unwrap_or_default();
    let repository = segments.next().unwrap_or_default();
    let releases = segments.next().unwrap_or_default();
    let download = segments.next().unwrap_or_default();
    let tag = segments.next().unwrap_or_default();
    let asset = segments.next().unwrap_or_default();
    !owner.is_empty()
        && !repository.is_empty()
        && releases == "releases"
        && download == "download"
        && !tag.is_empty()
        && !asset.is_empty()
        && segments.next().is_none()
}

#[derive(Debug)]
pub enum UpdateError {
    ManifestJson(serde_json::Error),
    InvalidManifest(String),
    NotAppImage(String),
    MissingSignature,
    SignatureUnavailable(String),
    SignatureRejected(String),
    TooLarge {
        limit: u64,
    },
    ChecksumMismatch {
        expected: String,
        actual: String,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestJson(error) => write!(f, "invalid updater manifest JSON: {error}"),
            Self::InvalidManifest(message) => write!(f, "invalid updater manifest: {message}"),
            Self::NotAppImage(message) => write!(f, "AppImage update unavailable: {message}"),
            Self::MissingSignature => f.write_str("update manifest has no minisign signature"),
            Self::SignatureUnavailable(message) => {
                write!(f, "minisign verification is unavailable: {message}")
            }
            Self::SignatureRejected(message) => {
                write!(f, "minisign verification rejected the update: {message}")
            }
            Self::TooLarge { limit } => write!(f, "AppImage exceeds the {limit}-byte limit"),
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "AppImage SHA-256 mismatch: expected {expected}, got {actual}"
                )
            }
            Self::Io { operation, source } => write!(f, "{operation}: {source}"),
        }
    }
}

impl std::error::Error for UpdateError {}

fn io_error(operation: &'static str, source: io::Error) -> UpdateError {
    UpdateError::Io { operation, source }
}

/// Signature verification seam. The verifier must validate the complete
/// minisign file stored as base64 in the release manifest against a pinned
/// public key.
pub trait SignatureVerifier {
    fn verify_base64_minisign(
        &self,
        artifact: &Path,
        encoded_signature: &str,
    ) -> Result<(), UpdateError>;
}

/// Honest placeholder used until `minisign-verify` and a pinned public key are
/// added to this crate. It always rejects signed updates.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableSignatureVerifier;

impl SignatureVerifier for UnavailableSignatureVerifier {
    fn verify_base64_minisign(
        &self,
        _artifact: &Path,
        _encoded_signature: &str,
    ) -> Result<(), UpdateError> {
        Err(UpdateError::SignatureUnavailable(
            "the Linux host was built without a minisign verifier".into(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppImageTarget {
    path: PathBuf,
}

impl AppImageTarget {
    pub fn detect() -> Result<Self, UpdateError> {
        let path = std::env::var_os("APPIMAGE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| UpdateError::NotAppImage("APPIMAGE is not set".into()))?;
        Self::new(path)
    }

    pub fn new(path: PathBuf) -> Result<Self, UpdateError> {
        if !path.is_absolute() {
            return Err(UpdateError::NotAppImage(
                "APPIMAGE must be an absolute path".into(),
            ));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error("inspect current AppImage", error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(UpdateError::NotAppImage(
                "current AppImage is not a regular, non-symlink file".into(),
            ));
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledUpdate {
    pub path: PathBuf,
    pub version: String,
    pub bytes_written: u64,
    pub sha256: String,
}

/// Streams, hashes, signature-checks, and atomically replaces an AppImage.
/// Verification happens before rename, so every pre-commit failure leaves the
/// currently installed file untouched.
pub fn install_verified_appimage(
    manifest: &UpdateManifest,
    expected_arch: &str,
    source: impl Read,
    target: &AppImageTarget,
    verifier: &dyn SignatureVerifier,
) -> Result<InstalledUpdate, UpdateError> {
    install_verified_appimage_with_limit(
        manifest,
        expected_arch,
        source,
        target,
        verifier,
        DEFAULT_MAX_APPIMAGE_BYTES,
    )
}

pub fn install_verified_appimage_with_limit(
    manifest: &UpdateManifest,
    expected_arch: &str,
    mut source: impl Read,
    target: &AppImageTarget,
    verifier: &dyn SignatureVerifier,
    max_bytes: u64,
) -> Result<InstalledUpdate, UpdateError> {
    manifest.validate(expected_arch)?;
    let signature = manifest
        .minisign
        .as_deref()
        .ok_or(UpdateError::MissingSignature)?;
    let parent = target.path.parent().ok_or_else(|| {
        UpdateError::NotAppImage("current AppImage has no parent directory".into())
    })?;
    let name = target
        .path
        .file_name()
        .ok_or_else(|| UpdateError::NotAppImage("current AppImage has no filename".into()))?;
    let temp = parent.join(format!(
        ".{}.update-{}",
        name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| io_error("create AppImage update file", error))?;
        let mut digest = Sha256::new();
        let mut written = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| io_error("read AppImage download", error))?;
            if read == 0 {
                break;
            }
            written = written
                .checked_add(read as u64)
                .ok_or(UpdateError::TooLarge { limit: max_bytes })?;
            if written > max_bytes {
                return Err(UpdateError::TooLarge { limit: max_bytes });
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| io_error("write AppImage update file", error))?;
            digest.update(&buffer[..read]);
        }
        output
            .sync_all()
            .map_err(|error| io_error("sync AppImage update file", error))?;
        let actual = digest.finish_hex();
        if !actual.eq_ignore_ascii_case(&manifest.sha256) {
            return Err(UpdateError::ChecksumMismatch {
                expected: manifest.sha256.to_ascii_lowercase(),
                actual,
            });
        }
        verifier.verify_base64_minisign(&temp, signature)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let current_mode = fs::metadata(&target.path)
                .map_err(|error| io_error("read current AppImage permissions", error))?
                .permissions()
                .mode();
            fs::set_permissions(&temp, fs::Permissions::from_mode(current_mode))
                .map_err(|error| io_error("set AppImage update permissions", error))?;
        }
        commit_with_rollback(&temp, &target.path)?;
        Ok(InstalledUpdate {
            path: target.path.clone(),
            version: manifest.version.clone(),
            bytes_written: written,
            sha256: actual,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Keep a hard-linked copy of the old inode until the replacement and its
/// directory entry are durable. This permits rollback if the commit itself
/// fails without ever exposing a partially written AppImage.
fn commit_with_rollback(temp: &Path, target: &Path) -> Result<(), UpdateError> {
    let parent = target.parent().ok_or_else(|| {
        UpdateError::NotAppImage("current AppImage has no parent directory".into())
    })?;
    let metadata = fs::symlink_metadata(target)
        .map_err(|error| io_error("inspect current AppImage before commit", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(UpdateError::NotAppImage(
            "current AppImage changed before update commit".into(),
        ));
    }
    let name = target
        .file_name()
        .ok_or_else(|| UpdateError::NotAppImage("current AppImage has no filename".into()))?;
    let backup = parent.join(format!(
        ".{}.rollback-{}",
        name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    fs::hard_link(target, &backup)
        .map_err(|error| io_error("prepare AppImage rollback link", error))?;

    if let Err(error) = fs::rename(temp, target) {
        let _ = fs::remove_file(&backup);
        return Err(io_error("atomically replace AppImage", error));
    }
    if let Err(error) = File::open(parent).and_then(|directory| directory.sync_all()) {
        let _ = fs::rename(&backup, target);
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
        return Err(io_error("sync AppImage directory", error));
    }
    if let Err(error) = fs::remove_file(&backup) {
        // The rollback inode still exists, so restore it before reporting the
        // cleanup failure. A failed restoration is intentionally not hidden.
        return match fs::rename(&backup, target) {
            Ok(()) => Err(io_error("remove AppImage rollback link", error)),
            Err(rollback_error) => Err(io_error("restore AppImage rollback link", rollback_error)),
        };
    }
    Ok(())
}

// Small self-contained SHA-256 implementation. This avoids pretending the
// updater is functional while waiting for a direct `sha2` dependency.
struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    block_len: usize,
    total_len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            block_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut bytes: &[u8]) {
        self.total_len = self.total_len.wrapping_add(bytes.len() as u64);
        if self.block_len != 0 {
            let take = (64 - self.block_len).min(bytes.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&bytes[..take]);
            self.block_len += take;
            bytes = &bytes[take..];
            if self.block_len == 64 {
                let block = self.block;
                self.compress(&block);
                self.block_len = 0;
            } else {
                return;
            }
        }
        while bytes.len() >= 64 {
            let block: &[u8; 64] = bytes[..64].try_into().expect("slice has exact block size");
            self.compress(block);
            bytes = &bytes[64..];
        }
        self.block[..bytes.len()].copy_from_slice(bytes);
        self.block_len = bytes.len();
    }

    fn finish_hex(mut self) -> String {
        let bit_len = self.total_len.wrapping_mul(8);
        self.block[self.block_len] = 0x80;
        self.block_len += 1;
        if self.block_len > 56 {
            self.block[self.block_len..].fill(0);
            let block = self.block;
            self.compress(&block);
            self.block_len = 0;
        }
        self.block[self.block_len..56].fill(0);
        self.block[56..].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.block;
        self.compress(&block);
        let mut result = String::with_capacity(64);
        for word in self.state {
            use fmt::Write as _;
            write!(&mut result, "{word:08x}").expect("formatting into String cannot fail");
        }
        result
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (index, chunk) in block.chunks_exact(4).take(16).enumerate() {
            w[index] = u32::from_be_bytes(chunk.try_into().expect("four-byte SHA word"));
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "openless-updater-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn manifest(body: &[u8]) -> UpdateManifest {
        let mut digest = Sha256::new();
        digest.update(body);
        UpdateManifest {
            schema_version: 1,
            host: MANIFEST_HOST.into(),
            arch: "x86_64".into(),
            version: "2.0.0".into(),
            url: "https://github.com/Open-Less/openless/releases/download/v2.0.0/OpenLess.AppImage"
                .into(),
            sha256: digest.finish_hex(),
            minisign: Some("dGVzdC1taW5pc2lnbg==".into()),
        }
    }

    struct AcceptTestSignature;

    impl SignatureVerifier for AcceptTestSignature {
        fn verify_base64_minisign(
            &self,
            artifact: &Path,
            encoded_signature: &str,
        ) -> Result<(), UpdateError> {
            if encoded_signature != "dGVzdC1taW5pc2lnbg==" || fs::metadata(artifact).is_err() {
                return Err(UpdateError::SignatureRejected(
                    "test signature mismatch".into(),
                ));
            }
            Ok(())
        }
    }

    #[test]
    fn sha256_matches_standard_vectors_and_streaming() {
        let mut empty = Sha256::new();
        empty.update(b"");
        assert_eq!(
            empty.finish_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let mut abc = Sha256::new();
        abc.update(b"a");
        abc.update(b"bc");
        assert_eq!(
            abc.finish_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn manifest_parser_enforces_host_arch_hash_and_release_url() {
        let valid = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "host": "linux-egui",
            "arch": "x86_64",
            "version": "2.0.0",
            "url": "https://github.com/Open-Less/openless/releases/download/v2.0.0/OpenLess.AppImage",
            "sha256": "0".repeat(64),
            "minisign": "c2ln"
        })).unwrap();
        assert!(UpdateManifest::parse(&valid, "x86_64").is_ok());
        assert!(UpdateManifest::parse(&valid, "aarch64").is_err());
        let mut bad_url: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        bad_url["url"] = "http://example.test/update".into();
        assert!(UpdateManifest::parse(&serde_json::to_vec(&bad_url).unwrap(), "x86_64").is_err());
    }

    #[test]
    fn verified_update_atomically_replaces_appimage() {
        let root = temp_dir("success");
        let path = root.join("OpenLess.AppImage");
        fs::write(&path, b"old image").unwrap();
        let target = AppImageTarget::new(path.clone()).unwrap();
        let body = b"new verified appimage";
        let installed = install_verified_appimage(
            &manifest(body),
            "x86_64",
            body.as_slice(),
            &target,
            &AcceptTestSignature,
        )
        .unwrap();
        assert_eq!(installed.bytes_written, body.len() as u64);
        assert_eq!(fs::read(&path).unwrap(), body);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checksum_signature_and_size_failures_preserve_current_appimage() {
        for failure in ["checksum", "signature", "size"] {
            let root = temp_dir(failure);
            let path = root.join("OpenLess.AppImage");
            fs::write(&path, b"old image").unwrap();
            let target = AppImageTarget::new(path.clone()).unwrap();
            let body = b"new image";
            let mut candidate = manifest(body);
            let result = match failure {
                "checksum" => {
                    candidate.sha256 = "0".repeat(64);
                    install_verified_appimage(
                        &candidate,
                        "x86_64",
                        body.as_slice(),
                        &target,
                        &AcceptTestSignature,
                    )
                }
                "signature" => install_verified_appimage(
                    &candidate,
                    "x86_64",
                    body.as_slice(),
                    &target,
                    &UnavailableSignatureVerifier,
                ),
                "size" => install_verified_appimage_with_limit(
                    &candidate,
                    "x86_64",
                    body.as_slice(),
                    &target,
                    &AcceptTestSignature,
                    3,
                ),
                _ => unreachable!(),
            };
            assert!(result.is_err());
            assert_eq!(fs::read(&path).unwrap(), b"old image");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn unsigned_update_is_rejected() {
        let root = temp_dir("unsigned");
        let path = root.join("OpenLess.AppImage");
        fs::write(&path, b"old").unwrap();
        let target = AppImageTarget::new(path.clone()).unwrap();
        let mut candidate = manifest(b"new");
        candidate.minisign = None;
        assert!(matches!(
            install_verified_appimage(
                &candidate,
                "x86_64",
                b"new".as_slice(),
                &target,
                &AcceptTestSignature
            ),
            Err(UpdateError::MissingSignature)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"old");
        fs::remove_dir_all(root).unwrap();
    }
}
