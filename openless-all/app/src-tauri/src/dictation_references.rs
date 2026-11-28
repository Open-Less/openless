//! Recording-owned clipboard collection. Native reads and file IO never hold the session map.
use openless_core::{dictation_references::*, SessionId};
use parking_lot::Mutex;
use std::{collections::{HashMap, VecDeque}, path::PathBuf, sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}}, time::{Instant, SystemTime, UNIX_EPOCH}};

#[cfg(any(target_os = "windows", test))]
mod windows_formats;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Default)]
struct OwnWrites { depth: usize, revisions: VecDeque<u64> }
static WRITES: OnceLock<Mutex<OwnWrites>> = OnceLock::new();
fn writes() -> &'static Mutex<OwnWrites> { WRITES.get_or_init(Default::default) }
pub(crate) struct SelfWrite;
impl SelfWrite {
    /// Call only after a successful actual write, or a stable observed synthetic
    /// copy. Merely holding/dropping the suppression guard claims no revision.
    pub(crate) fn commit_revision(&self, revision: Option<u64>) {
        if let Some(revision) = revision {
            let mut writes = writes().lock();
            writes.revisions.push_back(revision);
            while writes.revisions.len() > 256 { writes.revisions.pop_front(); }
        }
    }
    pub(crate) fn commit(&self) { self.commit_revision(crate::insertion::clipboard_sequence()); }
}
pub(crate) fn self_write() -> SelfWrite {
    // Also track writes that overlap recording startup (before a session exists).
    writes().lock().depth += 1;
    SelfWrite
}
impl Drop for SelfWrite {
    fn drop(&mut self) {
        let mut writes = writes().lock();
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
fn store() -> Option<ReferenceStore> {
    static STORE: OnceLock<Option<ReferenceStore>> = OnceLock::new();
    STORE.get_or_init(|| {
        let root = crate::persistence::data_dir().ok()?.join("dictation-references");
        ReferenceStore::for_process(root).map_err(|e| log::warn!("[references] startup: {e}")).ok()
    }).clone()
}
pub(crate) fn cleanup() {
    // Initialized once by setup, after the single-instance plugin. Retirement
    // only releases session memory; all copies retain the process store lease.
    let _ = store();
}
pub(crate) struct RecordingLease { id: SessionId }
pub(crate) struct StoppedCollection { session: Arc<Session>, snapshot: Option<FrozenClipboard> }
struct FrozenClipboard { revision: u64, elapsed_ms: u64, content: Result<Option<Content>, &'static str> }
fn freeze_clipboard(session: &Session, mut sequence: impl FnMut() -> Option<u64>, read: impl FnOnce() -> Result<Option<Content>, &'static str>) -> Option<FrozenClipboard> {
    session.collecting.store(false, Ordering::Release);
    let revision = sequence()?;
    let elapsed_ms = session.data.lock().started.elapsed().as_millis() as u64;
    {
        let own = writes().lock();
        if own.depth > 0 || own.revisions.contains(&revision) { return None; }
    }
    let content = read();
    let own = writes().lock();
    if own.depth > 0 || own.revisions.contains(&revision) || sequence() != Some(revision) { return None; }
    Some(FrozenClipboard { revision, elapsed_ms, content })
}
impl RecordingLease {
    pub(crate) fn stop(self) -> Option<StoppedCollection> {
        let session = session(self.id)?;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let snapshot = freeze_clipboard(&session, crate::insertion::clipboard_sequence, clipboard_content);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let snapshot = { session.collecting.store(false, Ordering::Release); None };
        Some(StoppedCollection { session, snapshot })
    }
}
impl StoppedCollection {
    pub(crate) fn finish(self) {
        // Only persist the frozen value here. Recorder teardown and a new
        // recording may already have changed the global clipboard.
        if let Some(snapshot) = self.snapshot {
            let mut data = self.session.data.lock();
            apply_content(&mut data, snapshot.revision, snapshot.elapsed_ms, snapshot.content);
            *self.session.feedback.lock() = (data.collector.references.len() as u32, data.notice.clone());
        }
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
            if !backend.get_preferences().dictation_references_enabled { break; }
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
pub(crate) fn delivery_plan(id: SessionId, text: &str) -> Vec<DeliveryPart> {
    let Some(session) = session(id) else { return vec![DeliveryPart::Text(text.into())]; };
    let data = session.data.lock();
    openless_core::dictation_references::delivery_plan(text, &data.collector.references)
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

use openless_core::dictation_references::ClipboardContent as Content;

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
    let elapsed_ms = s.started.elapsed().as_millis() as u64;
    apply_content(&mut s, revision, elapsed_ms, content);
    *session.feedback.lock() = (s.collector.references.len() as u32, s.notice.clone());
}
fn apply_content(s: &mut Collection, revision: u64, elapsed_ms: u64, content: Result<Option<Content>, &'static str>) {
    match content {
        Ok(Some(content)) => collect_at(s, revision, elapsed_ms, content),
        Ok(None) => { s.filter.observe(revision, 0, 0, false); }
        Err("tooLarge") => collect_at(s, revision, elapsed_ms, Content::TooLarge),
        Err(notice) => {
            // Native contention already received short bounded retries. Do not
            // repeatedly decode the same malformed payload every 100ms.
            s.filter.observe(revision, 0, 0, false);
            s.notice = Some(notice.into());
        }
    }
}

#[cfg(test)]
fn collect(s: &mut Collection, revision: u64, content: Content) {
    collect_at(s, revision, s.started.elapsed().as_millis() as u64, content);
}
fn collect_at(s: &mut Collection, revision: u64, elapsed_ms: u64, content: Content) {
    let (hash, size) = content.fingerprint();
    match s.filter.observe(revision, hash, size, false) {
        FilterResult::TooLarge => { s.notice = Some("tooLarge".into()); return; }
        FilterResult::Accept => {}
        _ => return,
    }
    s.notice = s.collector.capture(&s.dir, elapsed_ms, content);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collection(dir: PathBuf) -> Collection { Collection { dir, started: Instant::now(), collector: Collector::default(), filter: ClipboardFilter::new(10), notice: None } }
    #[test]
    fn references_stop_freezes_last_copy_before_recorder_teardown_and_next_session() {
        let root = std::env::temp_dir().join(format!("references-stop-{}", uuid::Uuid::new_v4()));
        let store = ReferenceStore::new(root.clone()).unwrap();
        let old = Arc::new(Session { collecting: AtomicBool::new(true), feedback: Mutex::new((0, None)), data: Mutex::new(collection(store.begin("old", 0).unwrap())) });
        let revision = std::sync::atomic::AtomicU64::new(12345671);
        let snapshot = freeze_clipboard(&old, || Some(revision.load(Ordering::Acquire)), || Ok(Some(Content::Text("last pre-stop copy\nkept in full".into()))));
        assert!(!old.collecting.load(Ordering::Acquire));
        // The native recorder is still shutting down while another recording
        // starts and the user overwrites CLIPBOARD. Finish must not read it.
        revision.store(12345672, Ordering::Release);
        let mut new = collection(store.begin("new", 0).unwrap());
        collect(&mut new, 12345672, Content::Text("new recording only".into()));
        StoppedCollection { session: old.clone(), snapshot }.finish();
        let old_data = old.data.lock();
        assert_eq!(old_data.collector.references.len(), 1);
        assert_eq!(old_data.collector.references[0].inline.as_deref(), Some("last pre-stop copy\nkept in full"));
        assert_eq!(new.collector.references.len(), 1);
        assert_eq!(new.collector.references[0].inline.as_deref(), Some("new recording only"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn references_failed_or_noop_write_guard_never_claims_a_user_revision() {
        let user_revision = 98765432101;
        {
            let _failed_lock_or_backup = self_write();
            // No successful write: no explicit revision commit.
        }
        assert!(!writes().lock().revisions.contains(&user_revision));
        let mut filter = ClipboardFilter::new(user_revision - 1);
        assert_eq!(filter.observe(user_revision, 42, 2, writes().lock().revisions.contains(&user_revision)), FilterResult::Accept);
        let app_revision = user_revision + 1;
        let guard = self_write();
        guard.commit_revision(Some(app_revision));
        drop(guard);
        assert_eq!(filter.observe(app_revision, 43, 2, writes().lock().revisions.contains(&app_revision)), FilterResult::SelfWrite);
        assert_eq!(filter.observe(app_revision + 1, 43, 2, false), FilterResult::Accept);
    }
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
        let mut full = collection(s.dir.clone());
        for _ in 0..5 {
            let mut reference = s.collector.references[1].clone(); reference.size = MAX_BYTES;
            full.collector.push(reference).unwrap();
        }
        collect(&mut full, 11, Content::Text("session byte overflow".into()));
        assert_eq!(full.collector.references.len(), 5);
        assert_eq!(full.notice.as_deref(), Some("tooLarge"));
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
