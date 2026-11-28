use openless_core::{dictation_references::*, SessionId};
use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::Instant};
static STORE: OnceLock<Option<ReferenceStore>> = OnceLock::new();
pub fn initialize(data: &Path) {
    STORE.get_or_init(|| ReferenceStore::for_process(data.join("dictation-references"))
        .map_err(|e| log::warn!("[references] startup: {e}")).ok());
}
struct Session { collecting: AtomicBool, data: Mutex<Data> }
struct Data { dir: PathBuf, started: Instant, filter: ClipboardFilter, collector: Collector, notice: Option<String> }
fn sessions() -> &'static Mutex<HashMap<SessionId, Arc<Session>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<SessionId, Arc<Session>>>> = OnceLock::new();
    SESSIONS.get_or_init(Default::default)
}
fn collect(session: &Session, snapshot: crate::clipboard::Snapshot, stopped: bool) {
    let mut data = session.data.lock().unwrap();
    if !stopped && !session.collecting.load(Ordering::Acquire) { return; }
    let (hash, size) = snapshot.content.as_ref().map(ClipboardContent::fingerprint).unwrap_or((0, 0));
    match data.filter.observe(snapshot.revision, hash, size, snapshot.own) {
        FilterResult::Accept => {
            if let Some(content) = snapshot.content {
                let elapsed = data.started.elapsed().as_millis() as u64;
                let dir = data.dir.clone();
                data.notice = data.collector.capture(&dir, elapsed, content);
            }
        }
        FilterResult::TooLarge => data.notice = Some("tooLarge".into()),
        _ => {}
    }
}
pub(crate) struct Lease { session: Arc<Session> }
impl Lease {
    pub(crate) fn start(id: SessionId, enabled: bool) -> Option<Self> {
        if !enabled { return None; }
        let baseline = crate::clipboard::snapshot()?;
        let dir = STORE.get()?.as_ref()?.begin(&id.to_string(), 0).ok()?;
        let session = Arc::new(Session { collecting: AtomicBool::new(true), data: Mutex::new(Data {
            dir, started: Instant::now(), filter: ClipboardFilter::new(baseline.revision), collector: Collector::default(), notice: None,
        }) });
        sessions().lock().unwrap().insert(id, session.clone());
        let worker = session.clone();
        std::thread::spawn(move || {
            while worker.collecting.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let snapshot = crate::clipboard::snapshot();
                if !worker.collecting.load(Ordering::Acquire) { break; }
                if let Some(snapshot) = snapshot { collect(&worker, snapshot, false); }
            }
        });
        Some(Self { session })
    }
    pub(crate) fn stop(self) {
        // Read/freeze the final native revision before recording shutdown.
        self.session.collecting.store(false, Ordering::Release);
        let final_snapshot = crate::clipboard::snapshot();
        if let Some(snapshot) = final_snapshot { collect(&self.session, snapshot, true); }
    }
}
impl Drop for Lease { fn drop(&mut self) { self.session.collecting.store(false, Ordering::Release); } }
pub(crate) fn enrich(id: SessionId, text: String, duration_ms: u64) -> String {
    let session = sessions().lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return text; };
    let data = session.data.lock().unwrap();
    place(&text, &data.collector.references, duration_ms, CUES)
}
pub(crate) fn retire(id: SessionId) {
    if let Some(s) = sessions().lock().unwrap().remove(&id) { s.collecting.store(false, Ordering::Release); }
}
pub(crate) fn delivery_plan(id: SessionId, text: &str) -> Vec<DeliveryPart> {
    let session = sessions().lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return vec![DeliveryPart::Text(text.into())]; };
    let data = session.data.lock().unwrap();
    openless_core::dictation_references::delivery_plan(text, &data.collector.references)
}
pub fn supported() -> bool { crate::clipboard::supported() }
pub fn feedback(id: SessionId) -> (u32, Option<String>) {
    let session = sessions().lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return (0, None); };
    let data = session.data.lock().unwrap();
    (data.collector.references.len() as u32, data.notice.clone())
}
