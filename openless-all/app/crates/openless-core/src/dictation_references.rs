//! Session-owned reference collection and deterministic, platform-independent placement.
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, HashSet}, fs, io::{self, Read, Write}, path::{Path, PathBuf}, sync::Arc};

pub const RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_SESSION_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_REFERENCES: usize = 64;
pub const INLINE_CHARS: usize = 80;
const MARKER: &str = ".openless-references.json";
const OWNER: &str = "openless.dictation-references.v1";
const PROCESS_MARKER: &str = ".openless-process.json";
const PROCESS_OWNER: &str = "openless.dictation-reference-process.v1";
pub const CUES: &[&str] = &["这张图", "看这张", "报错是这个", "这个报错", "this error", "this screenshot", "this image"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferenceKind { Screenshot, ClipboardText, ClipboardImage, File, Directory, Link }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub kind: ReferenceKind,
    pub elapsed_ms: u64,
    pub sequence: u32,
    pub path: PathBuf,
    pub source: String,
    pub size: u64,
    pub name: String,
    #[serde(default)]
    pub original_url: Option<String>,
    #[serde(default)]
    pub inline: Option<String>,
    #[serde(default)]
    pub transcript_chars: Option<usize>,
    #[serde(default)]
    pub original_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterResult { Accept, Baseline, SelfWrite, Duplicate, NotUserCopy, TooLarge }

/// Revisions identify ownership; hashes identify consecutive duplicate content.
/// A later identical *user* copy is not mistaken for an OpenLess write.
pub struct ClipboardFilter { baseline: u64, last_hash: Option<u64>, last_revision: u64 }
impl ClipboardFilter {
    pub fn revision_seen(&self, revision: u64) -> bool { revision == self.baseline || revision == self.last_revision }
    pub fn new(baseline: u64) -> Self { Self { baseline, last_hash: None, last_revision: baseline } }
    pub fn observe(&mut self, revision: u64, hash: u64, size: u64, self_write: bool, user_copy: bool) -> FilterResult {
        if revision == self.baseline || revision == self.last_revision { return FilterResult::Baseline; }
        self.last_revision = revision;
        if self_write { self.last_hash = None; return FilterResult::SelfWrite; }
        if !user_copy { return FilterResult::NotUserCopy; }
        if self.last_hash == Some(hash) { return FilterResult::Duplicate; }
        self.last_hash = Some(hash);
        if size > MAX_BYTES { FilterResult::TooLarge } else { FilterResult::Accept }
    }
}

#[derive(Debug, Default)]
pub struct Collector { pub references: Vec<Reference>, bytes: u64 }
impl Collector {
    pub fn push(&mut self, mut reference: Reference) -> Result<u32, FilterResult> {
        let owned_bytes = if reference.original_only { 0 } else { reference.size };
        if self.references.len() >= MAX_REFERENCES || self.bytes.saturating_add(owned_bytes) > MAX_SESSION_BYTES {
            return Err(FilterResult::TooLarge);
        }
        reference.sequence = self.references.len() as u32 + 1;
        self.bytes = self.bytes.saturating_add(owned_bytes);
        self.references.push(reference);
        Ok(self.references.len() as u32)
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest { owner: String, created_ms: u64 }

/// Root is supplied by the host, never by clipboard data. User paths are read-only.
#[derive(Debug, Clone)]
pub struct ReferenceStore { root: PathBuf, _lease: Option<Arc<fs::File>> }
impl ReferenceStore {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root: fs::canonicalize(root)?, _lease: None })
    }
    /// Call once after the host's single-instance gate. An OS lease protects every
    /// copy for the process lifetime, including cancelled/retired recordings.
    /// Crashes release the lease; another live process is never swept.
    pub fn for_process(root: PathBuf) -> io::Result<Self> {
        let parent = Self::new(root)?;
        let startup = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(parent.root.join(".startup.lock"))?;
        startup.lock()?;
        for entry in fs::read_dir(&parent.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() { continue; }
            let dir = entry.path();
            let marker = dir.join(PROCESS_MARKER);
            let lease = dir.join(".lease");
            let regular = |path: &Path| fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink() && m.len() < 4096);
            if !regular(&marker) || !regular(&lease) { continue; }
            let owned = fs::read(&marker).ok().and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
                .is_some_and(|m| m.owner == PROCESS_OWNER);
            if !owned { continue; }
            let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(&lease) else { continue; };
            if file.try_lock().is_err() { continue; }
            // The startup lock prevents another process from claiming a run
            // between releasing its stale lease and removing its owned tree.
            drop(file);
            fs::remove_dir_all(dir)?;
        }
        let dir = parent.root.join(format!("run-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        let lease = fs::OpenOptions::new().read(true).write(true).create_new(true).open(dir.join(".lease"))?;
        lease.lock()?;
        fs::write(dir.join(PROCESS_MARKER), serde_json::to_vec(&Manifest { owner: PROCESS_OWNER.into(), created_ms: 0 })?)?;
        Ok(Self { root: dir, _lease: Some(Arc::new(lease)) })
    }
    pub fn begin(&self, id: &str, now_ms: u64) -> io::Result<PathBuf> {
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid reference session"));
        }
        let dir = self.root.join(id);
        fs::create_dir(&dir)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        let data = serde_json::to_vec(&Manifest { owner: OWNER.into(), created_ms: now_ms })?;
        fs::write(dir.join(MARKER), data)?;
        Ok(dir)
    }
    pub fn write_bytes(dir: &Path, sequence: u32, extension: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        if bytes.len() as u64 > MAX_BYTES || !extension.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "reference too large or invalid extension"));
        }
        let path = dir.join(format!("reference-{sequence}.{extension}"));
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        file.write_all(bytes)?;
        Ok(path)
    }
    pub fn copy_file(dir: &Path, sequence: u32, source: &Path) -> io::Result<PathBuf> {
        // Reject symlinks/special files rather than following arbitrary directory trees.
        let meta = fs::symlink_metadata(source)?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "reference too large or not a regular file"));
        }
        let mut bytes = Vec::new();
        fs::File::open(source)?.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        let ext = source.extension().and_then(|v| v.to_str()).filter(|v| v.bytes().all(|c| c.is_ascii_alphanumeric())).unwrap_or("bin");
        Self::write_bytes(dir, sequence, ext, &bytes)
    }
    pub fn save_references(dir: &Path, references: &[Reference]) -> io::Result<()> {
        let marker: Manifest = serde_json::from_slice(&fs::read(dir.join(MARKER))?)?;
        if marker.owner != OWNER { return Err(io::Error::new(io::ErrorKind::PermissionDenied, "not an OpenLess reference directory")); }
        let next = dir.join("references.json.next");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&next)?;
        file.write_all(&serde_json::to_vec(references)?)?;
        fs::rename(next, dir.join("references.json"))
    }
    pub fn cleanup(&self, now_ms: u64, retention_ms: u64, active: &HashSet<String>) -> io::Result<usize> {
        let mut removed = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() || active.contains(&entry.file_name().to_string_lossy().into_owned()) { continue; }
            let marker = entry.path().join(MARKER);
            if fs::symlink_metadata(&marker).map(|m| !m.is_file() || m.file_type().is_symlink()).unwrap_or(true) { continue; }
            let manifest = fs::read(&marker).ok().filter(|b| b.len() < 4096).and_then(|b| serde_json::from_slice::<Manifest>(&b).ok());
            if manifest.is_some_and(|m| m.owner == OWNER && now_ms.saturating_sub(m.created_ms) >= retention_ms) {
                // remove_dir_all does not follow directory symlinks. Only owned copies live here.
                fs::remove_dir_all(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

pub fn expired(reference: &Reference) -> bool { reference.original_url.is_none() && !reference.original_only && !reference.path.exists() }
fn escaped(value: &str) -> String {
    value.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]").replace(['\n', '\r'], " ")
}
fn image_url(url: &str) -> bool {
    let lower = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("https://") && [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|ext| lower.ends_with(ext))
}
pub fn render(reference: &Reference) -> String {
    let label = escaped(&reference.name);
    if expired(reference) { return format!("[{label}: expired — {}]", escaped(&reference.path.to_string_lossy())); }
    if let Some(url) = &reference.original_url {
        let safe = url.replace('>', "%3E").replace(['\n', '\r'], "");
        return if image_url(url) { format!("![{label}](<{safe}>) [{label}](<{safe}>)") } else { format!("[{label}](<{safe}>)") };
    }
    if let Some(text) = &reference.inline { return format!("[{label}: {}]", escaped(text)); }
    let url = url::Url::from_file_path(&reference.path).map(|u| u.to_string()).unwrap_or_else(|_| reference.path.to_string_lossy().into_owned());
    let note = if reference.original_only { "; original path only (not copied)" } else { "" };
    format!("[{label}](<{url}>) (local: {}{note}; target may not have local-file access)", escaped(&reference.path.to_string_lossy()))
}

/// Every reference is emitted exactly once. No references means byte-for-byte identity.
/// Positions are monotonic: cue matching can never swap the spoken collection order.
pub fn place(text: &str, references: &[Reference], duration_ms: u64, cues: &[&str]) -> String {
    if references.is_empty() { return text.into(); }
    let mut boundaries: Vec<usize> = text.char_indices().filter_map(|(i, c)| matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '\n').then_some(i + c.len_utf8())).collect();
    if boundaries.last().copied() != Some(text.len()) { boundaries.push(text.len()); }
    let chars = text.chars().count();
    let mut ordered: Vec<_> = references.iter().collect();
    ordered.sort_by_key(|r| r.sequence);
    let mut inserts: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut previous = 0;
    for reference in ordered {
        let estimated_chars = reference.transcript_chars.filter(|v| *v <= chars).or_else(|| {
            (duration_ms > 0 && reference.elapsed_ms <= duration_ms).then(|| ((reference.elapsed_ms as u128 * chars as u128) / duration_ms as u128) as usize)
        });
        let position = estimated_chars.map(|n| text.char_indices().nth(n).map(|(i, _)| i).unwrap_or(text.len()));
        let mut anchor = position.map(|p| *boundaries.iter().min_by_key(|b| b.abs_diff(p)).unwrap_or(&text.len())).unwrap_or(text.len());
        if let Some(p) = position {
            let mut start = 0;
            let mut candidates = Vec::new();
            for &end in &boundaries {
                let sentence = &text[start..end];
                let near = end.abs_diff(p) <= text.len().div_ceil(3).max(32);
                if near && cues.iter().any(|cue| sentence.to_lowercase().contains(&cue.to_lowercase())) { candidates.push(end); }
                start = end;
            }
            if let Some(&end) = candidates.iter().min_by_key(|end| end.abs_diff(p)) { anchor = end; }
        }
        anchor = anchor.max(previous);
        previous = anchor;
        inserts.entry(anchor).or_default().push(render(reference));
    }
    let mut out = String::with_capacity(text.len() + references.len() * 160);
    let mut cursor = 0;
    for (position, items) in inserts {
        out.push_str(&text[cursor..position]);
        out.push(' ');
        out.push_str(&items.join(" "));
        cursor = position;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(sequence: u32, ms: u64) -> Reference {
        Reference { kind: ReferenceKind::File, elapsed_ms: ms, sequence, path: PathBuf::from("/original.log"), source: "user".into(), size: 2, name: format!("file-{sequence}"), original_url: None, inline: None, transcript_chars: None, original_only: true }
    }
    #[test]
    fn references_filter_baseline_self_writes_background_duplicates_and_limits() {
        let mut f = ClipboardFilter::new(7);
        assert_eq!(f.observe(7, 1, 1, false, true), FilterResult::Baseline);
        assert_eq!(f.observe(8, 2, 1, true, true), FilterResult::SelfWrite);
        assert_eq!(f.observe(9, 3, 1, false, false), FilterResult::NotUserCopy);
        assert_eq!(f.observe(10, 4, 1, false, true), FilterResult::Accept);
        assert_eq!(f.observe(11, 4, 1, false, true), FilterResult::Duplicate);
        assert_eq!(f.observe(12, 5, MAX_BYTES + 1, false, true), FilterResult::TooLarge);
        assert_eq!(f.observe(13, 2, 1, false, true), FilterResult::Accept);
    }
    #[test]
    fn references_no_material_is_exact_identity_and_unknown_timing_appends() {
        let text = "  原文\r\n🙂 this error.\t";
        assert_eq!(place(text, &[], 0, CUES).as_bytes(), text.as_bytes());
        use crate::ports::TextInserter;
        assert_eq!(crate::ports::UnsupportedTextInserter.enrich_dictation_text(crate::SessionId::new(), text.into(), 10).as_bytes(), text.as_bytes());
        assert!(place(text, &[reference(1, 0)], 0, CUES).starts_with(text));
    }
    #[test]
    fn references_snap_to_cues_preserve_order_and_never_lose_material() {
        let text = "Start. This error is here. Last sentence.";
        let out = place(text, &[reference(1, 400), reference(2, 500)], 1000, CUES);
        assert!(out.contains("This error is here. [file-1]"));
        assert!(out.find("[file-1]").unwrap() < out.find("[file-2]").unwrap());
        assert_eq!(out.matches("target may not").count(), 2);
        assert!(out.ends_with(" Last sentence."));
    }
    #[test]
    fn references_unicode_partial_positions_and_overlapping_collectors_are_isolated() {
        let mut a = Collector::default();
        let mut b = Collector::default();
        a.push(reference(1, 2)).unwrap();
        b.push(reference(1, 9)).unwrap();
        a.references[0].name = "A".into();
        a.references[0].transcript_chars = Some(4);
        let output = place("你好。看这张图。结束。", &a.references, 10, CUES);
        assert!(output.contains("看这张图。 [A]"));
        assert!(!output.contains("file-1"));
        assert_eq!(b.references[0].elapsed_ms, 9);
    }
    #[test]
    fn references_process_lifetime_sweeps_only_unlocked_owned_previous_runs() {
        let root = std::env::temp_dir().join(format!("openless-process-refs-{}", uuid::Uuid::new_v4()));
        let first = ReferenceStore::for_process(root.clone()).unwrap();
        let old = first.begin("cancelled-recording", 0).unwrap();
        let copy = ReferenceStore::write_bytes(&old, 1, "pdf", b"local copy").unwrap();
        let foreign = root.join("foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("original.pdf"), b"original").unwrap();
        let legacy = ReferenceStore::new(root.clone()).unwrap().begin("unleased-legacy", 0).unwrap();
        let concurrent = ReferenceStore::for_process(root.clone()).unwrap();
        assert!(copy.exists(), "a live other process owns this copy");
        let current = concurrent.begin("current", u64::MAX).unwrap();
        let current_copy = ReferenceStore::write_bytes(&current, 1, "txt", b"current").unwrap();
        let clone = first.clone();
        drop(first);
        let third = ReferenceStore::for_process(root.clone()).unwrap();
        assert!(copy.exists(), "all process store clones retain the lease");
        drop(clone);
        let next_launch = ReferenceStore::for_process(root.clone()).unwrap();
        assert!(!copy.exists(), "the next startup removes the prior run regardless of age");
        assert!(current_copy.exists());
        assert!(foreign.join("original.pdf").exists());
        assert!(legacy.exists(), "unknown legacy process ownership cannot authorize deletion");
        drop((concurrent, third, next_launch));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn references_storage_cleanup_expiry_and_original_ownership() {
        let root = std::env::temp_dir().join(format!("openless-refs-{}", uuid::Uuid::new_v4()));
        let store = ReferenceStore::new(root.clone()).unwrap();
        let old = store.begin("old", 0).unwrap();
        let busy = store.begin("busy", 0).unwrap();
        let foreign = root.join("foreign");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("user.txt"), b"user original").unwrap();
        let path = ReferenceStore::write_bytes(&old, 1, "txt", b"copy").unwrap();
        let mut r = reference(1, 0); r.path = path; r.original_only = false;
        ReferenceStore::save_references(&old, &[r.clone()]).unwrap();
        let restored: Vec<Reference> = serde_json::from_slice(&fs::read(old.join("references.json")).unwrap()).unwrap();
        assert_eq!(restored[0].path, r.path);
        assert!(!expired(&r));
        assert_eq!(store.cleanup(RETENTION_MS - 1, RETENTION_MS, &HashSet::new()).unwrap(), 0);
        assert_eq!(store.cleanup(RETENTION_MS, RETENTION_MS, &HashSet::from(["busy".into()])).unwrap(), 1);
        assert!(expired(&r)); assert!(render(&r).contains("expired"));
        assert!(busy.exists()); assert!(foreign.join("user.txt").exists());
        assert!(store.begin("../bad", 1).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn references_image_preview_retains_original_url_and_text_is_not_dumped() {
        let mut r = reference(1, 0);
        r.original_url = Some("https://example.com/a.png?q=1".into());
        assert_eq!(render(&r).matches("https://example.com/a.png?q=1").count(), 2);
        r.original_url = None;
        assert!(!render(&r).contains("raw clipboard"));
        assert_eq!(INLINE_CHARS, 80);
    }
    #[test]
    fn references_legacy_preferences_defaults_and_shortcut_conflicts() {
        let mut old = serde_json::to_value(crate::shared_types::UserPreferences::default()).unwrap();
        old.as_object_mut().unwrap().remove("dictationReferencesEnabled");
        old.as_object_mut().unwrap().remove("dictationReferenceKey");
        let mut p: crate::shared_types::UserPreferences = serde_json::from_value(old).unwrap();
        assert!(!p.dictation_references_enabled);
        assert_eq!(p.dictation_reference_key, "S");
        p.dictation_references_enabled = true;
        assert!(crate::reject_hotkey_collisions(&p).is_ok());
        p.dictation_reference_key = p.qa_companion_key.clone();
        assert!(crate::reject_hotkey_collisions(&p).is_err());
        p.dictation_references_enabled = false;
        assert!(crate::reject_hotkey_collisions(&p).is_ok());
    }
    #[test]
    fn references_limits_and_short_text_rendering_are_explicit() {
        let mut collector = Collector::default();
        for n in 0..MAX_REFERENCES { collector.push(reference(n as u32, n as u64)).unwrap(); }
        assert_eq!(collector.push(reference(99, 99)), Err(FilterResult::TooLarge));
        let mut r = reference(1, 0);
        r.inline = Some("x".repeat(INLINE_CHARS));
        assert!(render(&r).contains(&"x".repeat(INLINE_CHARS)));
        r.inline = None;
        assert!(!render(&r).contains(&"x".repeat(INLINE_CHARS)));
    }
}
