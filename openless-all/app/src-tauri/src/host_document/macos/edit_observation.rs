//! Finite AX snapshot observation, distinct from trusted editor revisions.
use super::*;
use openless_core::host_document::SnapshotObservation;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy)]
struct Key { sequence: u64, at: Instant, eligible: bool }
fn key_state() -> &'static Mutex<Key> {
    static KEY: OnceLock<Mutex<Key>> = OnceLock::new();
    KEY.get_or_init(|| Mutex::new(Key { sequence: 0, at: Instant::now(), eligible: false }))
}
pub(crate) fn record_key(code: i64, flags: u64, hardware: bool) {
    if let Ok(mut key) = key_state().lock() {
        key.sequence = key.sequence.saturating_add(1);
        key.at = Instant::now();
        // No command, paste, undo, option/control shortcut or synthetic typing.
        key.eligible = hardware && flags & ((1 << 18) | (1 << 19) | (1 << 20)) == 0
            && ((0..=50).contains(&code) || code == 51 || code == 117);
    }
}
fn current_key() -> Option<Key> { key_state().lock().ok().map(|key| *key) }

pub(crate) struct LearningStart { field: String, start: usize, length: usize }
pub(crate) struct DeliveredInsertion {
    pub(crate) target: crate::selection::SelectionInsertionTarget,
    proof: SnapshotObservation,
    baseline: String,
}
impl DeliveredInsertion {
    pub(crate) fn matches(&self, inserted: &str) -> bool { self.proof.original() == inserted }
}

fn snapshot(target: &crate::selection::SelectionInsertionTarget) -> Option<(String, (usize, usize))> {
    with_frozen_target_read(target, |lease| unsafe {
        let (control, _) = lease.ax_objects();
        let control = control.cast();
        let total = copy_index_attr(control, b"AXNumberOfCharacters\0").ok_or("length unavailable")?;
        if total > FULL_TEXT_MAX_UTF16 { return Err("bounded snapshot unavailable".into()); }
        let selection = copy_selected_range(control).ok_or("selection unavailable")?;
        let text = context::read_exact(control, 0, total)?;
        if copy_selected_range(control) != Some(selection)
            || copy_index_attr(control, b"AXNumberOfCharacters\0") != Some(total)
            || openless_core::expression_observation::contains_sensitive_content(&text) {
            return Err("snapshot changed or protected".into());
        }
        Ok((text, (usize::try_from(selection.location).map_err(|_| "selection unavailable")?,
            usize::try_from(selection.length).map_err(|_| "selection unavailable")?)))
    }).ok()
}

pub(crate) fn prepare(target: &crate::selection::SelectionInsertionTarget) -> Option<LearningStart> {
    let (field, (start, length)) = snapshot(target)?;
    let frozen = crate::selection::borrow_frozen_macos_target(target, |lease| Ok(lease.range())).ok()??;
    if frozen != (start as u64, length as u64) { return None; }
    Some(LearningStart { field, start, length })
}

pub(crate) fn complete(target: crate::selection::SelectionInsertionTarget, start: LearningStart,
    inserted: String) -> Option<DeliveredInsertion> {
    let before: Vec<u16> = start.field.encode_utf16().collect();
    let inserted_units: Vec<u16> = inserted.encode_utf16().collect();
    let expected: Vec<u16> = before.get(..start.start)?.iter().copied()
        .chain(inserted_units.iter().copied())
        .chain(before.get(start.start.checked_add(start.length)?..)?.iter().copied()).collect();
    let (baseline, selection) = snapshot(&target)?;
    if baseline.encode_utf16().collect::<Vec<_>>() != expected
        || selection != (start.start + inserted_units.len(), 0) { return None; }
    let proof = SnapshotObservation::delivered(&baseline, start.start, inserted, current_key()?.sequence)?;
    Some(DeliveredInsertion { target, proof, baseline })
}

struct Watch {
    delivery: DeliveredInsertion,
    stop: Arc<AtomicBool>,
    on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
    latest: Option<EditPair>,
    last_change: Instant,
    last_sequence: u64,
    published: bool,
}
unsafe extern "C" fn changed(_: AxObserverRef, _: AxUiElementRef, _: CFStringRef, data: *mut c_void) {
    let watch = &mut *(data.cast::<Watch>());
    if watch.stop.load(Ordering::Acquire) { return; }
    if watch.published { watch.stop.store(true, Ordering::Release); return; }
    let valid = (|| {
        let key = current_key()?;
        if !key.eligible || key.at.elapsed() > Duration::from_millis(400) { return None; }
        let (text, selection) = snapshot(&watch.delivery.target)?;
        // Metadata and text must be from the same hardware sequence.
        if current_key()?.sequence != key.sequence { return None; }
        let edit = watch.delivery.proof.sample(&text, selection, key.sequence);
        if !watch.delivery.proof.is_valid() { return None; }
        watch.latest = edit;
        watch.last_change = Instant::now();
        watch.last_sequence = key.sequence;
        Some(())
    })().is_some();
    if !valid { watch.stop.store(true, Ordering::Release); }
}

pub(crate) fn spawn(delivery: DeliveredInsertion, lifetime: Duration,
    on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>, on_end: Box<dyn Fn() + Send + Sync>) -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    std::thread::Builder::new().name("bounded-edit-learning".into()).spawn(move || {
        let mut watch = Box::new(Watch { delivery, stop: thread_stop, on_edit,
            latest: None, last_change: Instant::now(), last_sequence: 0, published: false });
        let target = watch.delivery.target.clone();
        // The borrow retains the original native objects until unregistration.
        let _ = with_frozen_target_read(&target, |lease| unsafe {
            let (element, _) = lease.ax_objects();
            let element = element.cast();
            let mut observer = std::ptr::null_mut();
            if AXObserverCreate(lease.process_id(), changed, &mut observer) != AX_ERROR_SUCCESS { return Err("observer unavailable".into()); }
            let Some(notification) = cfstring_from_static(b"AXValueChanged\0") else {
                CFRelease(observer.cast()); return Err("notification unavailable".into());
            };
            let data = (&mut *watch as *mut Watch).cast();
            if AXObserverAddNotification(observer, element, notification, data) == AX_ERROR_SUCCESS {
                let source = CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(observer));
                let runloop = CFRunLoop::get_current();
                runloop.add_source(&source, kCFRunLoopDefaultMode);
                if snapshot(&target).is_none_or(|(text, _)| text != watch.delivery.baseline) {
                    watch.stop.store(true, Ordering::Release);
                }
                let began = Instant::now();
                while !watch.stop.load(Ordering::Acquire) && began.elapsed() < lifetime.min(Duration::from_secs(60)) {
                    if with_frozen_target_read(&target, |_| Ok(())).is_err() { break; }
                    CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, Duration::from_millis(50), false);
                    if !watch.stop.load(Ordering::Acquire) && !watch.published
                        && watch.last_change.elapsed() >= Duration::from_millis(650)
                        && current_key().is_some_and(|key| key.sequence == watch.last_sequence) {
                        if let Some(edit) = watch.latest.take() {
                            watch.published = true;
                            (watch.on_edit)(edit);
                        }
                    }
                }
                runloop.remove_source(&source, kCFRunLoopDefaultMode);
                AXObserverRemoveNotification(observer, element, notification);
            }
            CFRelease(notification);
            CFRelease(observer.cast());
            Ok(())
        });
        on_end();
    }).ok()?;
    Some(stop)
}
