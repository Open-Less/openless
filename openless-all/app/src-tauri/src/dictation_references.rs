//! Desktop reference runtime. Native resources are tied to the recording ID, not capsule focus.
use openless_core::{dictation_references::*, SessionId};
use parking_lot::Mutex;
use std::{collections::{HashMap, HashSet, VecDeque}, path::PathBuf, sync::{Arc, OnceLock}, time::{Instant, SystemTime, UNIX_EPOCH}};

#[derive(Default)]
struct OwnWrites { depth: usize, revisions: VecDeque<u64> }
static WRITES: OnceLock<Mutex<OwnWrites>> = OnceLock::new();
fn writes() -> &'static Mutex<OwnWrites> { WRITES.get_or_init(Default::default) }
pub(crate) struct SelfWrite(bool);
pub(crate) fn self_write() -> SelfWrite {
    if !sessions().lock().values().any(|s| s.collecting) { return SelfWrite(false); }
    writes().lock().depth += 1; SelfWrite(true)
}
impl Drop for SelfWrite {
    fn drop(&mut self) {
        if !self.0 { return; }
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
    dir: PathBuf,
    started: Instant,
    collecting: bool,
    collector: Collector,
    filter: ClipboardFilter,
    notice: Option<String>,
    copy_intent: Option<Instant>,
    capture_busy: bool,
}
static SESSIONS: OnceLock<Mutex<HashMap<SessionId, Session>>> = OnceLock::new();
fn sessions() -> &'static Mutex<HashMap<SessionId, Session>> { SESSIONS.get_or_init(Default::default) }
fn store() -> Option<ReferenceStore> { ReferenceStore::new(crate::persistence::data_dir().ok()?.join("dictation-references")).ok() }
pub(crate) fn cleanup() {
    if let Some(store) = store() {
        let active = sessions().lock().keys().map(ToString::to_string).collect::<HashSet<_>>();
        if let Err(e) = store.cleanup(now_ms(), RETENTION_MS, &active) { log::warn!("[references] cleanup: {e}"); }
    }
}
pub(crate) struct RecordingLease { id: SessionId }
impl Drop for RecordingLease {
    fn drop(&mut self) {
        if let Some(session) = sessions().lock().get_mut(&self.id) { session.collecting = false; }
        cleanup();
    }
}
pub(crate) fn start(id: SessionId, backend: &Arc<openless_core::OpenLessBackend>) -> Option<RecordingLease> {
    if !cfg!(target_os = "macos") || !backend.get_preferences().dictation_references_enabled { return None; }
    let dir = store()?.begin(&id.to_string(), now_ms()).ok()?;
    let prefs = backend.get_preferences();
    crate::recording_companion::configure(prefs.dictation_hotkey, prefs.dictation_reference_key);
    crate::recording_companion::install(Arc::new(capture));
    sessions().lock().insert(id, Session { dir, started: Instant::now(), collecting: true, collector: Collector::default(),
        filter: ClipboardFilter::new(crate::insertion::clipboard_sequence().unwrap_or(0)), notice: None, copy_intent: None, capture_busy: false });
    let weak = Arc::downgrade(backend);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let Some(backend) = weak.upgrade() else { break; };
            let live = backend.snapshot().dictation;
            if live.session_id != Some(id) || !matches!(live.phase, openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording) { break; }
            if !sessions().lock().get(&id).is_some_and(|s| s.collecting) { break; }
            #[cfg(target_os = "macos")]
            poll(id);
        }
        if let Some(s) = sessions().lock().get_mut(&id) { s.collecting = false; }
    });
    Some(RecordingLease { id })
}
pub(crate) fn active(id: SessionId) -> bool { sessions().lock().get(&id).is_some_and(|s| !s.collector.references.is_empty()) }
pub(crate) fn retire(id: SessionId) { sessions().lock().remove(&id); cleanup(); }
pub(crate) fn enrich(id: SessionId, text: String, duration_ms: u64) -> String {
    let all = sessions().lock();
    let Some(session) = all.get(&id) else { return text; };
    let mut references = session.collector.references.clone();
    references.sort_by_key(|r| (r.elapsed_ms, r.sequence));
    for (i, reference) in references.iter_mut().enumerate() { reference.sequence = i as u32 + 1; }
    place(&text, &references, duration_ms, CUES)
}
pub(crate) fn project(payload: &mut crate::types::CapsulePayload) {
    let all = sessions().lock();
    if let Some(session) = all.iter().find_map(|(id, s)| (payload.session_id.as_deref() == Some(id.to_string().as_str())).then_some(s)) {
        payload.reference_count = session.collector.references.len() as u32;
        payload.reference_notice = session.notice.clone();
    }
}
pub(crate) fn capture_in_progress() -> bool { sessions().lock().values().any(|s| s.collecting && s.capture_busy) }
pub(crate) fn collecting() -> bool { sessions().lock().values().any(|s| s.collecting) }

/// Called on the native event tap before companion cancellation. Never reads clipboard here.
#[cfg(target_os = "macos")]
pub(crate) fn key_event(code: i64, flags: u64, down: bool, hardware: bool) -> bool {
    let collecting = sessions().lock().values().any(|s| s.collecting);
    if !collecting { return false; }
    if !hardware { return false; }
    if code == 53 && capture_in_progress() { return false; }
    if hardware && down && code == 8 && flags & (1 << 20) != 0 {
        for s in sessions().lock().values_mut().filter(|s| s.collecting) { s.copy_intent = Some(Instant::now()); }
    }
    crate::recording_companion::sync_fn(flags & (1 << 23) != 0);
    for (code, key) in [(55, "LeftCommand"), (54, "RightCommand"), (58, "LeftAlt"), (61, "RightAlt"),
        (59, "LeftControl"), (62, "RightControl"), (56, "LeftShift"), (60, "RightShift")] {
        crate::recording_companion::event(key, crate::side_aware_combo::platform::keycode_is_down(code));
    }
    let primary = match code { 63 => Some("Fn"), 55 => Some("LeftCommand"), 54 => Some("RightCommand"),
        58 => Some("LeftAlt"), 61 => Some("RightAlt"), 59 => Some("LeftControl"), 62 => Some("RightControl"),
        56 => Some("LeftShift"), 60 => Some("RightShift"), _ => crate::side_aware_combo::macos_keycode_to_primary(code) };
    primary.is_some_and(|key| crate::recording_companion::event(key, down))
}

fn capture() {
    #[cfg(target_os = "macos")]
    {
        let request = {
            let mut all = sessions().lock();
            let Some((&id, s)) = all.iter_mut().find(|(_, s)| s.collecting && !s.capture_busy) else { return; };
            if !screen_granted() { s.notice = Some("screenPermission".into()); return; }
            s.capture_busy = true;
            (id, s.dir.join(format!("capture-{}.png", uuid::Uuid::new_v4())), s.started.elapsed().as_millis() as u64)
        };
        std::thread::spawn(move || {
            let (id, path, elapsed_ms) = request;
            let result = std::process::Command::new("/usr/sbin/screencapture").args(["-i", "-x"]).arg(&path).status();
            let mut all = sessions().lock();
            if let Some(s) = all.get_mut(&id) {
                s.capture_busy = false;
                if s.collecting {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if meta.len() > 0 && meta.len() <= MAX_BYTES {
                            let n = s.collector.references.len() + 1;
                            let r = Reference { kind: ReferenceKind::Screenshot, elapsed_ms, sequence: 0, path: path.clone(), source: "screencapture".into(), size: meta.len(), name: format!("Screenshot {n}"), original_url: None, inline: None, transcript_chars: None, original_only: false };
                            s.notice = s.collector.push(r).err().map(|_| "tooLarge".into());
                        } else { s.notice = Some("tooLarge".into()); }
                    } else if result.is_err() { s.notice = Some("captureFailed".into()); }
                    // A completed interactive capture without a file is Esc/cancel, not material.
                }
            }
            if !all.get(&id).is_some_and(|s| s.collector.references.iter().any(|r| r.path == path)) { let _ = std::fs::remove_file(path); }
        });
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" { fn CGPreflightScreenCaptureAccess() -> bool; }
#[cfg(target_os = "macos")]
fn screen_granted() -> bool { unsafe { CGPreflightScreenCaptureAccess() } }

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status { supported: bool, screen_capture: &'static str }
pub(crate) fn status() -> Status {
    Status { supported: cfg!(target_os = "macos"), screen_capture: {
        #[cfg(target_os = "macos")] { if screen_granted() { "granted" } else { "denied" } }
        #[cfg(not(target_os = "macos"))] { "unknown" }
    } }
}

#[cfg(target_os = "macos")]
enum Content { Text(String), Image(Vec<u8>, &'static str), Files(Vec<PathBuf>), TooLarge }
#[cfg(target_os = "macos")]
fn clipboard_content() -> Option<Content> {
    use objc2::{msg_send, runtime::{AnyClass, AnyObject}, rc::autoreleasepool};
    use objc2_foundation::NSString;
    autoreleasepool(|_| unsafe {
        let board: *mut AnyObject = msg_send![AnyClass::get("NSPasteboard")?, generalPasteboard];
        if board.is_null() { return None; }
        let items: *mut AnyObject = msg_send![board, pasteboardItems];
        let mut files = Vec::new();
        if !items.is_null() {
            let count: usize = msg_send![items, count];
            for i in 0..count.min(MAX_REFERENCES) {
                let item: *mut AnyObject = msg_send![items, objectAtIndex: i];
                let value: *mut NSString = msg_send![item, stringForType: &*NSString::from_str("public.file-url")];
                if !value.is_null() {
                    if let Ok(url) = url::Url::parse(&(&*value).to_string()) { if let Ok(path) = url.to_file_path() { files.push(path); } }
                }
            }
        }
        if !files.is_empty() { return Some(Content::Files(files)); }
        for (ty, ext) in [("public.png", "png"), ("public.tiff", "tiff")] {
            let data: *mut AnyObject = msg_send![board, dataForType: &*NSString::from_str(ty)];
            if data.is_null() { continue; }
            let length: usize = msg_send![data, length];
            if length as u64 > MAX_BYTES { return Some(Content::TooLarge); }
            let bytes: *const u8 = msg_send![data, bytes];
            if !bytes.is_null() && length > 0 { return Some(Content::Image(std::slice::from_raw_parts(bytes, length).to_vec(), ext)); }
        }
        // NSData length is checked before allocating a native string copy.
        let ty = NSString::from_str("public.utf8-plain-text");
        let data: *mut AnyObject = msg_send![board, dataForType: &*ty];
        if !data.is_null() {
            let length: usize = msg_send![data, length];
            if length as u64 > MAX_BYTES { return Some(Content::TooLarge); }
        }
        let text: *mut NSString = msg_send![board, stringForType: &*ty];
        if text.is_null() { None } else { Some(Content::Text((&*text).to_string())) }
    })
}

#[cfg(target_os = "macos")]
fn poll(id: SessionId) {
    use std::hash::{Hash, Hasher};
    let Some(revision) = crate::insertion::clipboard_sequence() else { return; };
    let mut all = sessions().lock();
    let Some(s) = all.get_mut(&id).filter(|s| s.collecting) else { return; };
    if s.filter.revision_seen(revision) { return; }
    let own = writes().lock();
    if own.depth > 0 { return; }
    let self_write = own.revisions.contains(&revision);
    drop(own);
    let user_copy = s.copy_intent.is_some_and(|at| at.elapsed().as_millis() < 1500);
    if !user_copy || self_write {
        s.filter.observe(revision, 0, 0, self_write, false);
        return;
    }
    let before = revision;
    let Some(content) = clipboard_content() else { return; };
    if crate::insertion::clipboard_sequence() != Some(before) { return; }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let size = match &content {
        Content::Text(text) => { text.hash(&mut hash); text.len() as u64 }
        Content::Image(bytes, ext) => { bytes.hash(&mut hash); ext.hash(&mut hash); bytes.len() as u64 }
        Content::Files(paths) => { paths.hash(&mut hash); 0 }
        Content::TooLarge => MAX_BYTES + 1,
    };
    let result = s.filter.observe(revision, hash.finish(), size, false, true);
    if result == FilterResult::TooLarge { s.notice = Some("tooLarge".into()); return; }
    if result != FilterResult::Accept { return; }
    s.copy_intent = None;
    let elapsed_ms = s.started.elapsed().as_millis() as u64;
    let materials = match content {
        Content::Files(paths) => paths.into_iter().enumerate().map(|(offset, path)| {
            let meta = std::fs::symlink_metadata(&path);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let directory = meta.as_ref().is_ok_and(|m| m.is_dir());
            let n = s.collector.references.len() as u32 + offset as u32 + 1;
            let copied = ReferenceStore::copy_file(&s.dir, n, &path).ok();
            let original_only = copied.is_none();
            if original_only { s.notice = Some(if size > MAX_BYTES { "tooLarge" } else { "captureFailed" }.into()); }
            Reference { kind: if directory { ReferenceKind::Directory } else { ReferenceKind::File }, elapsed_ms, sequence: 0,
                name: format!("File {}", path.file_name().unwrap_or_default().to_string_lossy()), path: copied.unwrap_or(path), source: "user-copy".into(), size, original_url: None, inline: None, transcript_chars: None, original_only }
        }).collect::<Vec<_>>(),
        Content::Text(text) => {
            if text.is_empty() { return; }
            let n = s.collector.references.len() as u32 + 1;
            let Ok(path) = ReferenceStore::write_bytes(&s.dir, n, "txt", text.as_bytes()) else { s.notice = Some("captureFailed".into()); return; };
            let link = url::Url::parse(text.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https")).map(|u| u.to_string());
            let inline = (link.is_none() && !text.contains(['\n', '\r']) && text.chars().count() <= INLINE_CHARS).then_some(text);
            vec![Reference { kind: if link.is_some() { ReferenceKind::Link } else { ReferenceKind::ClipboardText }, elapsed_ms, sequence: 0, path, source: "user-copy".into(), size,
                name: format!("Copied text {n}"), original_url: link, inline, transcript_chars: None, original_only: false }]
        }
        Content::Image(bytes, ext) => {
            let n = s.collector.references.len() as u32 + 1;
            let Ok(path) = ReferenceStore::write_bytes(&s.dir, n, ext, &bytes) else { s.notice = Some("captureFailed".into()); return; };
            vec![Reference { kind: ReferenceKind::ClipboardImage, elapsed_ms, sequence: 0, path, source: "user-copy".into(), size, name: format!("Copied image {n}"), original_url: None, inline: None, transcript_chars: None, original_only: false }]
        }
        Content::TooLarge => return,
    };
    let failure = if materials.iter().any(|r| r.original_only) { s.notice.take() } else { None };
    s.notice = None;
    for reference in materials {
        if s.collector.push(reference).is_err() { s.notice = Some("tooLarge".into()); }
    }
    if s.notice.is_none() { s.notice = failure; }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_self_write_ownership_is_revision_not_text() {
        let own = OwnWrites { depth: 0, revisions: VecDeque::from([12]) };
        assert!(own.revisions.contains(&12));
        assert!(!own.revisions.contains(&13));
        let mut filter = ClipboardFilter::new(10);
        assert_eq!(filter.observe(12, 42, 2, own.revisions.contains(&12), true), FilterResult::SelfWrite);
        assert_eq!(filter.observe(13, 42, 2, own.revisions.contains(&13), true), FilterResult::Accept);
    }
}
