//! Session-owned reference collection and deterministic, platform-independent placement.
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, HashSet}, fs, io::{self, Read, Write}, path::{Path, PathBuf}, sync::Arc};

pub const RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_SESSION_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_REFERENCES: usize = 64;
pub const INLINE_CHARS: usize = 80;
static INPUT_ACTIVITY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static INPUT_MONITORS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static INPUT_MONITOR_PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub struct InputMonitorLease { _private: () }
impl InputMonitorLease {
    pub fn new() -> Self {
        INPUT_MONITORS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        note_input_activity(); Self { _private: () }
    }
}
impl Default for InputMonitorLease { fn default() -> Self { Self::new() } }
impl Drop for InputMonitorLease { fn drop(&mut self) { INPUT_MONITORS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel); note_input_activity(); } }
pub fn note_input_activity() { INPUT_ACTIVITY.fetch_add(1, std::sync::atomic::Ordering::AcqRel); INPUT_MONITOR_PAUSED.store(false, std::sync::atomic::Ordering::Release); }
pub fn invalidate_input_monitor() { INPUT_ACTIVITY.fetch_add(1, std::sync::atomic::Ordering::AcqRel); INPUT_MONITOR_PAUSED.store(true, std::sync::atomic::Ordering::Release); }
pub fn observed_input_epoch() -> Option<u64> {
    (INPUT_MONITORS.load(std::sync::atomic::Ordering::Acquire) > 0 && !INPUT_MONITOR_PAUSED.load(std::sync::atomic::Ordering::Acquire)).then(|| INPUT_ACTIVITY.load(std::sync::atomic::Ordering::Acquire))
}
#[derive(Default)]
pub struct NoticeTimer { notice: Option<String>, since: Option<std::time::Instant> }
impl NoticeTimer {
    pub fn project(&mut self, notice: Option<&str>) -> Option<String> { self.at(notice, std::time::Instant::now()) }
    fn at(&mut self, notice: Option<&str>, now: std::time::Instant) -> Option<String> {
        if self.notice.as_deref() != notice { self.notice = notice.map(str::to_owned); self.since = notice.map(|_| now); }
        self.since.filter(|since| now.saturating_duration_since(*since).as_millis() < 3000).and(self.notice.clone())
    }
}
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
pub enum FilterResult { Accept, Baseline, SelfWrite, Duplicate, TooLarge }

/// Revisions identify ownership; hashes identify consecutive duplicate content.
/// A later identical *user* copy is not mistaken for an OpenLess write.
pub struct ClipboardFilter { baseline: u64, last_hash: Option<u64>, last_revision: u64 }
impl ClipboardFilter {
    pub fn revision_seen(&self, revision: u64) -> bool { revision == self.baseline || revision == self.last_revision }
    pub fn new(baseline: u64) -> Self { Self { baseline, last_hash: None, last_revision: baseline } }
    pub fn observe(&mut self, revision: u64, hash: u64, size: u64, self_write: bool) -> FilterResult {
        if revision == self.baseline || revision == self.last_revision { return FilterResult::Baseline; }
        self.last_revision = revision;
        if self_write { self.last_hash = None; return FilterResult::SelfWrite; }
        if self.last_hash == Some(hash) { return FilterResult::Duplicate; }
        self.last_hash = Some(hash);
        if size > MAX_BYTES { FilterResult::TooLarge } else { FilterResult::Accept }
    }
}

#[derive(Debug, Default)]
pub struct Collector { pub references: Vec<Reference>, bytes: u64 }
#[derive(Debug, Clone)]
pub enum ClipboardContent { Text(String), Image(Vec<u8>, &'static str), Files(Vec<PathBuf>), TooLarge }
pub fn clipboard_uri_paths(text: &str) -> Vec<PathBuf> {
    text.lines().filter(|line| !line.starts_with('#')).filter_map(|line|
        url::Url::parse(line.trim()).ok()?.to_file_path().ok()).collect()
}
impl ClipboardContent {
    pub fn fingerprint(&self) -> (u64, u64) {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let size = match self {
            Self::Text(text) => { 1u8.hash(&mut hash); text.hash(&mut hash); text.len() as u64 }
            Self::Image(bytes, ext) => { 2u8.hash(&mut hash); bytes.hash(&mut hash); ext.hash(&mut hash); bytes.len() as u64 }
            Self::Files(paths) => { 3u8.hash(&mut hash); paths.hash(&mut hash); 0 }
            Self::TooLarge => MAX_BYTES + 1,
        };
        (hash.finish(), size)
    }
}
impl Collector {
    /// Host-only IO after revision/ownership checks. Raw materials stay local;
    /// hosts enrich text only after model processing has completed.
    pub fn capture(&mut self, dir: &Path, elapsed_ms: u64, content: ClipboardContent) -> Option<String> {
        if self.references.len() >= MAX_REFERENCES || matches!(content, ClipboardContent::TooLarge) { return Some("tooLarge".into()); }
        let mut notice = None;
        let materials = match content {
            ClipboardContent::Files(paths) => {
                let mut reserved = self.bytes;
                paths.into_iter().enumerate().map(|(offset, path)| {
                    let meta = fs::symlink_metadata(&path);
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    let directory = meta.as_ref().is_ok_and(|m| m.is_dir());
                    let n = self.references.len() as u32 + offset as u32 + 1;
                    let limit = n as usize > MAX_REFERENCES || size > MAX_BYTES || reserved.saturating_add(size) > MAX_SESSION_BYTES;
                    let copied = if limit || directory { None } else { ReferenceStore::copy_file(dir, n, &path).ok() };
                    if copied.is_some() { reserved = reserved.saturating_add(size); }
                    if limit { notice = Some("tooLarge".into()); }
                    else if copied.is_none() && !directory && notice.is_none() { notice = Some("captureFailed".into()); }
                    Reference { kind: if directory { ReferenceKind::Directory } else { ReferenceKind::File }, elapsed_ms, sequence: 0,
                        name: format!("File {}", path.file_name().unwrap_or_default().to_string_lossy()), original_only: copied.is_none(),
                        path: copied.unwrap_or(path), source: "clipboard".into(), size, original_url: None, inline: None, transcript_chars: None }
                }).collect::<Vec<_>>()
            }
            ClipboardContent::Text(text) => {
                if text.is_empty() { return None; }
                if text.len() as u64 > MAX_BYTES || self.bytes.saturating_add(text.len() as u64) > MAX_SESSION_BYTES { return Some("tooLarge".into()); }
                let n = self.references.len() as u32 + 1;
                let Ok(path) = ReferenceStore::write_bytes(dir, n, "txt", text.as_bytes()) else { return Some("captureFailed".into()); };
                let link = url::Url::parse(text.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https")).map(|u| u.to_string());
                vec![Reference { kind: if link.is_some() { ReferenceKind::Link } else { ReferenceKind::ClipboardText }, elapsed_ms, sequence: 0, path,
                    source: "clipboard".into(), size: text.len() as u64, name: format!("Copied text {n}"), original_url: link,
                    inline: Some(text), transcript_chars: None, original_only: false }]
            }
            ClipboardContent::Image(bytes, ext) => {
                if bytes.len() as u64 > MAX_BYTES || self.bytes.saturating_add(bytes.len() as u64) > MAX_SESSION_BYTES { return Some("tooLarge".into()); }
                let n = self.references.len() as u32 + 1;
                let Ok(path) = ReferenceStore::write_bytes(dir, n, ext, &bytes) else { return Some("captureFailed".into()); };
                vec![Reference { kind: ReferenceKind::ClipboardImage, elapsed_ms, sequence: 0, path, source: "clipboard".into(),
                    size: bytes.len() as u64, name: format!("Copied image {n}"), original_url: None, inline: None, transcript_chars: None, original_only: false }]
            }
            ClipboardContent::TooLarge => return Some("tooLarge".into()),
        };
        for reference in materials {
            let owned = (!reference.original_only).then(|| reference.path.clone());
            if self.push(reference).is_err() {
                if let Some(path) = owned { let _ = fs::remove_file(path); }
                notice = Some("tooLarge".into());
            }
        }
        if ReferenceStore::save_references(dir, &self.references).is_err() { notice = Some("captureFailed".into()); }
        notice
    }
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
    if matches!(reference.kind, ReferenceKind::ClipboardText | ReferenceKind::Link) {
        if let Some(text) = &reference.inline { return text.clone(); }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryPart { Text(String), Image(PathBuf) }

/// Split the final, already-polished text at owned image references. The path
/// remains in the following text even when native paste is dispatched: a native
/// paste receipt cannot establish that the recipient supports image content.
pub fn delivery_plan(text: &str, references: &[Reference]) -> Vec<DeliveryPart> {
    let mut images = references.iter().filter(|r| !expired(r) && (r.kind == ReferenceKind::ClipboardImage
        || r.kind == ReferenceKind::File && !r.original_only && r.path.extension().and_then(|e| e.to_str())
            .is_some_and(|ext| ["png", "jpg", "jpeg", "tif", "tiff"].contains(&ext.to_ascii_lowercase().as_str()))))
        .filter_map(|r| text.find(&render(r)).map(|at| (at, &r.path))).collect::<Vec<_>>();
    images.sort_by_key(|(at, _)| *at);
    let mut out = Vec::new();
    let mut cursor = 0;
    for (at, path) in images {
        if at < cursor { continue; }
        if at > cursor { out.push(DeliveryPart::Text(text[cursor..at].into())); }
        out.push(DeliveryPart::Image(path.clone()));
        cursor = at;
    }
    if cursor < text.len() { out.push(DeliveryPart::Text(text[cursor..].into())); }
    out
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
    #[test]
    fn references_notices_expire_without_cross_session_or_repaint_resets() {
        let now = std::time::Instant::now();
        let mut old = NoticeTimer::default();
        assert_eq!(old.at(Some("tooLarge"), now).as_deref(), Some("tooLarge"));
        assert_eq!(old.at(Some("tooLarge"), now + std::time::Duration::from_millis(2999)).as_deref(), Some("tooLarge"));
        assert_eq!(old.at(Some("tooLarge"), now + std::time::Duration::from_secs(3)), None);
        assert_eq!(NoticeTimer::default().at(Some("tooLarge"), now + std::time::Duration::from_secs(3)).as_deref(), Some("tooLarge"));
        assert_eq!(old.at(None, now), None);
        assert_eq!(old.at(Some("captureFailed"), now).as_deref(), Some("captureFailed"));
    }
    fn reference(sequence: u32, ms: u64) -> Reference {
        Reference { kind: ReferenceKind::File, elapsed_ms: ms, sequence, path: PathBuf::from("/original.log"), source: "user".into(), size: 2, name: format!("file-{sequence}"), original_url: None, inline: None, transcript_chars: None, original_only: true }
    }
    #[test]
    fn references_full_copied_text_and_ordered_image_paths_survive_delivery() {
        let root = std::env::temp_dir().join(format!("references-delivery-{}", uuid::Uuid::new_v4()));
        let store = ReferenceStore::new(root.clone()).unwrap();
        let dir = store.begin("recording", 0).unwrap();
        let copied = format!("{}\r\nsecond line 🙂\n", "长".repeat(81));
        let mut collector = Collector::default();
        assert_eq!(collector.capture(&dir, 200, ClipboardContent::Text(copied.clone())), None);
        assert_eq!(collector.capture(&dir, 500, ClipboardContent::Image(b"local image bytes".to_vec(), "png")), None);
        let text = place("Start. This error is here. End.", &collector.references, 1000, CUES);
        assert!(text.contains(&copied));
        assert!(!text.contains("Copied text"));
        let parts = delivery_plan(&text, &collector.references);
        let image = parts.iter().position(|p| matches!(p, DeliveryPart::Image(_))).unwrap();
        assert!(matches!(&parts[image + 1], DeliveryPart::Text(body) if body.contains(&collector.references[1].path.to_string_lossy().to_string())));
        assert_eq!(parts.iter().filter_map(|part| match part { DeliveryPart::Text(body) => Some(body.as_str()), _ => None }).collect::<String>(), text);
        assert_eq!(parts.iter().filter(|p| matches!(p, DeliveryPart::Image(_))).count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn references_filter_baseline_self_writes_menu_context_screenshot_duplicates_and_limits() {
        let mut f = ClipboardFilter::new(7);
        assert_eq!(f.observe(7, 1, 1, false), FilterResult::Baseline);
        assert_eq!(f.observe(8, 2, 1, true), FilterResult::SelfWrite);
        // There is deliberately no keyboard-intent input: menu, context menu,
        // screenshot-to-clipboard and programmatic writes have identical eligibility.
        for (revision, hash) in [(9, 3), (10, 4), (11, 5), (12, 6)] {
            assert_eq!(f.observe(revision, hash, 1, false), FilterResult::Accept);
        }
        assert_eq!(f.observe(13, 6, 1, false), FilterResult::Duplicate);
        assert_eq!(f.observe(14, 7, MAX_BYTES + 1, false), FilterResult::TooLarge);
        assert_eq!(f.observe(15, 2, 1, false), FilterResult::Accept);
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
    fn references_legacy_preferences_ignore_removed_key_and_preserve_defaults() {
        let mut old = serde_json::to_value(crate::shared_types::UserPreferences::default()).unwrap();
        old.as_object_mut().unwrap().remove("dictationReferencesEnabled");
        for legacy in [serde_json::json!("S"), serde_json::json!(null), serde_json::json!({"obsolete": true})] {
            old["dictationReferenceKey"] = legacy;
            let mut p: crate::shared_types::UserPreferences = serde_json::from_value(old.clone()).unwrap();
            assert!(!p.dictation_references_enabled);
            p.dictation_references_enabled = true;
            assert!(crate::reject_hotkey_collisions(&p).is_ok());
            assert!(serde_json::to_value(p).unwrap().get("dictationReferenceKey").is_none());
        }
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
