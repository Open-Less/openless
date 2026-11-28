//! A receipt is issued only for a stable native offer carrying our unique marker
//! AND the exact expected payload. A later global revision alone is not ownership.
use std::{hash::{Hash, Hasher}, path::Path};
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(target_os = "windows")]
use windows as native;
pub(crate) const MARKER: &str = "application/x-openless-owned-clipboard";
#[derive(Clone)]
pub(crate) struct Receipt { pub revision: u64, token: String, format: String, digest: u64, size: usize }
fn digest(bytes: &[u8]) -> u64 { let mut h = std::collections::hash_map::DefaultHasher::new(); bytes.hash(&mut h); h.finish() }
fn marker(format: &str, bytes: &[u8]) -> String { serde_json::to_string(&(uuid::Uuid::new_v4().to_string(), format, digest(bytes), bytes.len())).expect("clipboard marker") }
fn from_marker(token: String, revision: u64) -> Option<Receipt> {
    let (nonce, format, digest, size): (String, String, u64, usize) = serde_json::from_str(&token).ok()?;
    uuid::Uuid::parse_str(&nonce).ok()?;
    if size as u64 > openless_core::dictation_references::MAX_SESSION_BYTES + 4096 { return None; }
    Some(Receipt { revision, token, format, digest, size })
}
pub(crate) fn write_text(text: &str, expected: Option<u64>) -> Result<Receipt, String> {
    native::write_text(text, expected)
}
pub(crate) fn write_image(path: &Path) -> Result<Receipt, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path).and_then(|f| f.take(openless_core::dictation_references::MAX_BYTES + 1).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > openless_core::dictation_references::MAX_BYTES { return Err("image too large".into()); }
    native::write_image(path, bytes)
}
pub(crate) fn owns(receipt: &Receipt) -> bool { native::owns(receipt) }
pub(crate) fn is_own_offer() -> bool { native::is_own_offer() }
