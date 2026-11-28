//! Recording-owned clipboard collection. Native reads and file IO never hold the session map.
use openless_core::{dictation_references::*, SessionId};
use parking_lot::Mutex;
use std::{collections::{HashMap, HashSet, VecDeque}, path::PathBuf, sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}}, time::{Instant, SystemTime, UNIX_EPOCH}};

#[cfg(any(target_os = "windows", test))]
mod windows_formats;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Default)]
struct OwnWrites { depth: usize, revisions: VecDeque<u64> }
static WRITES: OnceLock<Mutex<OwnWrites>> = OnceLock::new();
fn writes() -> &'static Mutex<OwnWrites> { WRITES.get_or_init(Default::default) }
pub(crate) struct SelfWrite;
pub(crate) fn self_write() -> SelfWrite {
    // Also track writes that overlap recording startup (before a session exists).
    writes().lock().depth += 1;
    SelfWrite
}
impl Drop for SelfWrite {
    fn drop(&mut self) {
        let revision = crate::insertion::clipboard_sequence();
        let mut writes = writes().lock();
        if let Some(revision) = revision {
            writes.revisions.push_back(revision);
            while writes.revisions.len() > 256 { writes.revisions.pop_front(); }
        }
        writes.depth = writes.depth.saturating_sub(1);
    }
}
fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }
struct Session {
    collecting: AtomicBool,
    feedback: Mutex<(u32, Option<String>)>,
    data: Mutex<Collection>,
}
struct Collection {
    dir: PathBuf,
    started: Instant,
    collector: Collector,
    filter: ClipboardFilter,
    notice: Option<String>,
}
static SESSIONS: OnceLock<Mutex<HashMap<SessionId, Arc<Session>>>> = OnceLock::new();
fn sessions() -> &'static Mutex<HashMap<SessionId, Arc<Session>>> { SESSIONS.get_or_init(Default::default) }
fn session(id: SessionId) -> Option<Arc<Session>> { sessions().lock().get(&id).cloned() }
static TARGETS: OnceLock<Mutex<VecDeque<SessionId>>> = OnceLock::new();
fn targets() -> &'static Mutex<VecDeque<SessionId>> { TARGETS.get_or_init(Default::default) }
pub(crate) fn bind_target(id: SessionId) {
    let mut targets = targets().lock();
    targets.push_back(id);
    while targets.len() > 256 { targets.pop_front(); }
}
fn store() -> Option<ReferenceStore> { ReferenceStore::new(crate::persistence::data_dir().ok()?.join("dictation-references")).ok() }
pub(crate) fn cleanup() {
    if let Some(store) = store() {
        let active = sessions().lock().keys().map(ToString::to_string).collect::<HashSet<_>>();
        if let Err(e) = store.cleanup(now_ms(), RETENTION_MS, &active) { log::warn!("[references] cleanup: {e}"); }
    }
}
pub(crate) struct RecordingLease { id: SessionId }
pub(crate) struct StoppedCollection { session: Arc<Session>, revision: Option<u64> }
impl RecordingLease {
    pub(crate) fn stop(self) -> Option<StoppedCollection> {
        let session = session(self.id)?;
        let revision = crate::insertion::clipboard_sequence();
        session.collecting.store(false, Ordering::Release);
        Some(StoppedCollection { session, revision })
    }
}
impl StoppedCollection {
    pub(crate) fn finish(self) {
        // Flush only the stop-time revision on the recorder's blocking worker.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(revision) = self.revision { poll_revision(&self.session, revision, true); }
    }
}
impl Drop for RecordingLease {
    fn drop(&mut self) {
        if let Some(s) = session(self.id) { s.collecting.store(false, Ordering::Release); }
    }
}
pub(crate) fn start(id: SessionId, backend: &Arc<openless_core::OpenLessBackend>) -> Option<RecordingLease> {
    let bound = {
        let mut targets = targets().lock();
        let bound = targets.contains(&id);
        targets.retain(|target| *target != id);
        bound
    };
    if !cfg!(any(target_os = "macos", target_os = "windows")) || !bound || !backend.get_preferences().dictation_references_enabled { return None; }
    let baseline = crate::insertion::clipboard_sequence()?;
    let dir = store()?.begin(&id.to_string(), now_ms()).ok()?;
    let s = Arc::new(Session { collecting: AtomicBool::new(true), feedback: Mutex::new((0, None)), data: Mutex::new(Collection {
        dir, started: Instant::now(), collector: Collector::default(), filter: ClipboardFilter::new(baseline), notice: None,
    }) });
    sessions().lock().insert(id, Arc::clone(&s));
    let weak = Arc::downgrade(backend);
    std::thread::spawn(move || {
        while s.collecting.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let Some(backend) = weak.upgrade() else { break; };
            let live = backend.snapshot().dictation;
            if live.session_id != Some(id) || !matches!(live.phase, openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording) { break; }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            poll(&s);
        }
        s.collecting.store(false, Ordering::Release);
    });
    Some(RecordingLease { id })
}
pub(crate) fn active(id: SessionId) -> bool { session(id).is_some_and(|s| !s.data.lock().collector.references.is_empty()) }
pub(crate) fn retire(id: SessionId) {
    targets().lock().retain(|target| *target != id);
    if let Some(s) = sessions().lock().remove(&id) { s.collecting.store(false, Ordering::Release); }
    cleanup();
}
pub(crate) fn enrich(id: SessionId, text: String, duration_ms: u64) -> String {
    let Some(session) = session(id) else { return text; };
    let mut references = session.data.lock().collector.references.clone();
    references.sort_by_key(|r| (r.elapsed_ms, r.sequence));
    for (i, reference) in references.iter_mut().enumerate() { reference.sequence = i as u32 + 1; }
    place(&text, &references, duration_ms, CUES)
}
pub(crate) fn project(payload: &mut crate::types::CapsulePayload) {
    let s = sessions().lock().iter().find_map(|(id, s)| (payload.session_id.as_deref() == Some(id.to_string().as_str())).then(|| Arc::clone(s)));
    if let Some(s) = s {
        // Feedback must not wait on native reads / a slow filesystem.
        let feedback = s.feedback.lock();
        payload.reference_count = feedback.0;
        payload.reference_notice = feedback.1.clone();
    }
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status { supported: bool }
pub(crate) fn status() -> Status { Status { supported: cfg!(any(target_os = "macos", target_os = "windows")) } }

#[derive(Debug)]
enum Content { Text(String), Image(Vec<u8>, &'static str), Files(Vec<PathBuf>), TooLarge }

#[cfg(target_os = "macos")]
fn clipboard_content() -> Result<Option<Content>, &'static str> {
    use objc2::{msg_send, runtime::{AnyClass, AnyObject}, rc::autoreleasepool};
    use objc2_foundation::NSString;
    autoreleasepool(|_| unsafe {
        let class = AnyClass::get("NSPasteboard").ok_or("captureFailed")?;
        let board: *mut AnyObject = msg_send![class, generalPasteboard];
        if board.is_null() { return Err("captureFailed"); }
        let items: *mut AnyObject = msg_send![board, pasteboardItems];
        let mut files = Vec::new();
        if !items.is_null() {
            let count: usize = msg_send![items, count];
            for i in 0..count {
                let item: *mut AnyObject = msg_send![items, objectAtIndex: i];
                let value: *mut NSString = msg_send![item, stringForType: &*NSString::from_str("public.file-url")];
                if !value.is_null() {
                    if let Ok(url) = url::Url::parse(&(&*value).to_string()) { if let Ok(path) = url.to_file_path() { files.push(path); } }
                    if files.len() > MAX_REFERENCES { return Ok(Some(Content::TooLarge)); }
                }
            }
        }
        if !files.is_empty() { return Ok(Some(Content::Files(files))); }
        for (ty, ext) in [("public.png", "png"), ("public.tiff", "tiff")] {
            let data: *mut AnyObject = msg_send![board, dataForType: &*NSString::from_str(ty)];
            if data.is_null() { continue; }
            let length: usize = msg_send![data, length];
            if length as u64 > MAX_BYTES { return Ok(Some(Content::TooLarge)); }
            let bytes: *const u8 = msg_send![data, bytes];
            if !bytes.is_null() && length > 0 { return Ok(Some(Content::Image(std::slice::from_raw_parts(bytes, length).to_vec(), ext))); }
            return Err("captureFailed");
        }
        let ty = NSString::from_str("public.utf8-plain-text");
        let data: *mut AnyObject = msg_send![board, dataForType: &*ty];
        if !data.is_null() {
            let length: usize = msg_send![data, length];
            if length as u64 > MAX_BYTES { return Ok(Some(Content::TooLarge)); }
        }
        let text: *mut NSString = msg_send![board, stringForType: &*ty];
        Ok(if text.is_null() { None } else { Some(Content::Text((&*text).to_string())) })
    })
}
#[cfg(target_os = "windows")]
fn clipboard_content() -> Result<Option<Content>, &'static str> { windows::read() }

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn poll(session: &Session) {
    let Some(revision) = crate::insertion::clipboard_sequence() else { return; };
    poll_revision(session, revision, false);
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn poll_revision(session: &Session, revision: u64, stopped: bool) {
    let mut s = session.data.lock();
    if (!stopped && !session.collecting.load(Ordering::Acquire)) || s.filter.revision_seen(revision) { return; }
    if crate::insertion::clipboard_sequence() != Some(revision) { return; }
    let own = writes().lock();
    if own.depth > 0 { return; }
    if own.revisions.contains(&revision) {
        s.filter.observe(revision, 0, 0, true);
        return;
    }
    drop(own);
    let content = clipboard_content();
    // Ownership may have changed during a delayed clipboard render/read.
    let own = writes().lock();
    if own.depth > 0 || own.revisions.contains(&revision) || crate::insertion::clipboard_sequence() != Some(revision) { return; }
    drop(own);
    if !stopped && !session.collecting.load(Ordering::Acquire) { return; }
    match content {
        Ok(Some(content)) => collect(&mut s, revision, content),
        Ok(None) => { s.filter.observe(revision, 0, 0, false); }
        Err(notice) => { s.notice = Some(notice.into()); } // Retry on the next recording-only tick.
    }
    *session.feedback.lock() = (s.collector.references.len() as u32, s.notice.clone());
}

fn collect(s: &mut Collection, revision: u64, content: Content) {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let size = match &content {
        Content::Text(text) => { 1u8.hash(&mut hash); text.hash(&mut hash); text.len() as u64 }
        Content::Image(bytes, ext) => { 2u8.hash(&mut hash); bytes.hash(&mut hash); ext.hash(&mut hash); bytes.len() as u64 }
        Content::Files(paths) => { 3u8.hash(&mut hash); paths.hash(&mut hash); 0 }
        Content::TooLarge => MAX_BYTES + 1,
    };
    match s.filter.observe(revision, hash.finish(), size, false) {
        FilterResult::TooLarge => { s.notice = Some("tooLarge".into()); return; }
        FilterResult::Accept => {}
        _ => return,
    }
    let owned_bytes: u64 = s.collector.references.iter().filter(|r| !r.original_only).map(|r| r.size).sum();
    if s.collector.references.len() >= MAX_REFERENCES || owned_bytes.saturating_add(size) > MAX_SESSION_BYTES {
        s.notice = Some("tooLarge".into()); return;
    }
    let elapsed_ms = s.started.elapsed().as_millis() as u64;
    let mut reserved_bytes = owned_bytes;
    let mut failure = None;
    let materials = match content {
        Content::Files(paths) => paths.into_iter().enumerate().map(|(offset, path)| {
            let meta = std::fs::symlink_metadata(&path);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let directory = meta.as_ref().is_ok_and(|m| m.is_dir());
            let n = s.collector.references.len() as u32 + offset as u32 + 1;
            let limit = n as usize > MAX_REFERENCES || size > MAX_BYTES || reserved_bytes.saturating_add(size) > MAX_SESSION_BYTES;
            let copied = if limit || directory { None } else { ReferenceStore::copy_file(&s.dir, n, &path).ok() };
            if copied.is_some() { reserved_bytes = reserved_bytes.saturating_add(size); }
            let original_only = copied.is_none();
            if limit { failure = Some("tooLarge".to_string()); }
            else if original_only && !directory && failure.is_none() { failure = Some("captureFailed".to_string()); }
            Reference { kind: if directory { ReferenceKind::Directory } else { ReferenceKind::File }, elapsed_ms, sequence: 0,
                name: format!("File {}", path.file_name().unwrap_or_default().to_string_lossy()), path: copied.unwrap_or(path), source: "clipboard".into(), size, original_url: None, inline: None, transcript_chars: None, original_only }
        }).collect::<Vec<_>>(),
        Content::Text(text) => {
            if text.is_empty() { return; }
            let n = s.collector.references.len() as u32 + 1;
            let Ok(path) = ReferenceStore::write_bytes(&s.dir, n, "txt", text.as_bytes()) else { s.notice = Some("captureFailed".into()); return; };
            let link = url::Url::parse(text.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https")).map(|u| u.to_string());
            let inline = (link.is_none() && !text.contains(['\n', '\r']) && text.chars().count() <= INLINE_CHARS).then_some(text);
            vec![Reference { kind: if link.is_some() { ReferenceKind::Link } else { ReferenceKind::ClipboardText }, elapsed_ms, sequence: 0, path, source: "clipboard".into(), size,
                name: format!("Copied text {n}"), original_url: link, inline, transcript_chars: None, original_only: false }]
        }
        Content::Image(bytes, ext) => {
            let n = s.collector.references.len() as u32 + 1;
            let Ok(path) = ReferenceStore::write_bytes(&s.dir, n, ext, &bytes) else { s.notice = Some("captureFailed".into()); return; };
            vec![Reference { kind: ReferenceKind::ClipboardImage, elapsed_ms, sequence: 0, path, source: "clipboard".into(), size, name: format!("Copied image {n}"), original_url: None, inline: None, transcript_chars: None, original_only: false }]
        }
        Content::TooLarge => return,
    };
    s.notice = failure;
    for reference in materials {
        let owned_path = (!reference.original_only).then(|| reference.path.clone());
        if s.collector.push(reference).is_err() {
            if let Some(path) = owned_path { let _ = std::fs::remove_file(path); }
            s.notice = Some("tooLarge".into());
        }
    }
    if ReferenceStore::save_references(&s.dir, &s.collector.references).is_err() { s.notice = Some("captureFailed".into()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collection(dir: PathBuf) -> Collection { Collection { dir, started: Instant::now(), collector: Collector::default(), filter: ClipboardFilter::new(10), notice: None } }
    #[test]
    fn reference_self_write_ownership_is_revision_not_text() {
        let own = OwnWrites { depth: 0, revisions: VecDeque::from([12]) };
        let mut filter = ClipboardFilter::new(10);
        assert_eq!(filter.observe(12, 42, 2, own.revisions.contains(&12)), FilterResult::SelfWrite);
        assert_eq!(filter.observe(13, 42, 2, own.revisions.contains(&13)), FilterResult::Accept);
    }
    #[test]
    fn references_host_clipboard_without_intent_images_files_limits_and_feedback() {
        let root = std::env::temp_dir().join(format!("references-host-{}", uuid::Uuid::new_v4()));
        let store = ReferenceStore::new(root.clone()).unwrap();
        let mut s = collection(store.begin("test", 0).unwrap());
        collect(&mut s, 10, Content::Text("baseline".into()));
        assert!(s.collector.references.is_empty());
        collect(&mut s, 11, Content::Text("menu copy".into()));
        collect(&mut s, 12, Content::Text("menu copy".into()));
        assert_eq!(s.collector.references.len(), 1);
        let image = windows_formats::dib_to_png(&windows_formats::tests::dib24()).unwrap();
        collect(&mut s, 13, Content::Image(image.clone(), "png"));
        assert_eq!(std::fs::read(&s.collector.references[1].path).unwrap(), image);
        assert_eq!(s.collector.references[1].kind, ReferenceKind::ClipboardImage);
        let file = root.join("original.txt");
        std::fs::write(&file, b"original").unwrap();
        collect(&mut s, 14, Content::Files(vec![file.clone(), root.clone()]));
        assert_eq!(s.collector.references.len(), 4);
        assert_ne!(s.collector.references[2].path, file);
        assert_eq!(std::fs::read(&s.collector.references[2].path).unwrap(), b"original");
        assert_eq!(s.collector.references[3].kind, ReferenceKind::Directory);
        assert!(s.notice.is_none());
        collect(&mut s, 15, Content::TooLarge);
        assert_eq!(s.notice.as_deref(), Some("tooLarge"));
        assert_eq!(s.collector.references.len(), 4);
        for i in 4..MAX_REFERENCES { collect(&mut s, 16 + i as u64, Content::Text(format!("item {i}"))); }
        collect(&mut s, 100, Content::Text("overflow".into()));
        assert_eq!(s.collector.references.len(), MAX_REFERENCES);
        assert_eq!(s.notice.as_deref(), Some("tooLarge"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn references_host_overlapping_sessions_keep_owned_results_and_empty_identity() {
        let a = SessionId::new(); let b = SessionId::new();
        let mut old = collection(PathBuf::new());
        old.collector.push(Reference { kind: ReferenceKind::File, elapsed_ms: 1, sequence: 0,
            path: PathBuf::from("/original-A.log"), source: "clipboard".into(), size: 2, name: "A".into(),
            original_url: None, inline: None, transcript_chars: None, original_only: true }).unwrap();
        sessions().lock().insert(a, Arc::new(Session { collecting: AtomicBool::new(false), feedback: Mutex::new((1, None)), data: Mutex::new(old) }));
        sessions().lock().insert(b, Arc::new(Session { collecting: AtomicBool::new(true), feedback: Mutex::new((0, None)), data: Mutex::new(collection(PathBuf::new())) }));
        assert!(active(a)); assert!(!active(b));
        assert_eq!(enrich(b, "  exact\r\n🙂".into(), 20).as_bytes(), b"  exact\r\n\xf0\x9f\x99\x82");
        assert_eq!(enrich(SessionId::new(), "disabled\r\n".into(), 20), "disabled\r\n");
        assert!(enrich(a, "First.".into(), 20).contains("original-A.log"));
        drop(RecordingLease { id: a });
        assert!(session(b).unwrap().collecting.load(Ordering::Acquire));
        sessions().lock().remove(&a); sessions().lock().remove(&b);
    }
}
