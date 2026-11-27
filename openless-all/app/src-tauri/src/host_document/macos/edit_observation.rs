//! One existing AX observer, bound to the successful insertion's P4 owner.
//! No focused-element supplier, full-field text read or insertion search.

use super::*;
use openless_core::host_document::RangeObservation;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};

const ANCHOR_UTF16: usize = 32;
const QUIET: Duration = Duration::from_secs(5);

pub(crate) struct LearningStart {
    start: usize,
    replaced: usize,
    total: usize,
    before: String,
    after: String,
    selected_digest: [u8; 32],
}

pub(crate) struct DeliveredInsertion {
    pub(crate) target: crate::selection::SelectionInsertionTarget,
    proof: RangeObservation,
}

impl DeliveredInsertion {
    pub(crate) fn matches(&self, inserted: &str) -> bool {
        self.proof.original() == inserted
    }
}

pub(crate) fn prepare(
    target: &crate::selection::SelectionInsertionTarget,
) -> Option<LearningStart> {
    with_frozen_target_read(target, |lease| unsafe {
        let (control, _) = lease.ax_objects();
        let control = control.cast();
        let (start, replaced) = lease.range().ok_or("range unavailable")?;
        let (start, replaced) = (
            usize::try_from(start).map_err(|_| "range unavailable")?,
            usize::try_from(replaced).map_err(|_| "range unavailable")?,
        );
        let expected = CFRange {
            location: isize::try_from(start).map_err(|_| "range unavailable")?,
            length: isize::try_from(replaced).map_err(|_| "range unavailable")?,
        };
        if copy_selected_range(control) != Some(expected) {
            return Err("range changed".into());
        }
        let total =
            copy_index_attr(control, b"AXNumberOfCharacters\0").ok_or("length unavailable")?;
        let end = start
            .checked_add(replaced)
            .filter(|end| *end <= total)
            .ok_or("range unavailable")?;
        if replaced > RangeObservation::MAX_INSERTION_UTF16 {
            return Err("selection too large for local observation".into());
        }
        let selected = context::read_exact(control, start, replaced)?;
        if protected(&selected) {
            return Err("protected selection".into());
        }
        let selected_digest = Sha256::digest(selected.as_bytes()).into();
        let before = context::read_exact(
            control,
            start.saturating_sub(ANCHOR_UTF16),
            start.min(ANCHOR_UTF16),
        )?;
        let after = context::read_exact(control, end, (total - end).min(ANCHOR_UTF16))?;
        if protected(&before)
            || protected(&after)
            || copy_selected_range(control) != Some(expected)
            || copy_index_attr(control, b"AXNumberOfCharacters\0") != Some(total)
        {
            return Err("unavailable".into());
        }
        Ok(LearningStart {
            start,
            replaced,
            total,
            before,
            after,
            selected_digest,
        })
    })
    .ok()
}

/// Called only after successful insertion. A posted-input fallback is not
/// proof: require exact text at the original start, expected total and caret.
pub(crate) fn complete(
    target: crate::selection::SelectionInsertionTarget,
    start: LearningStart,
    inserted: String,
) -> Option<DeliveredInsertion> {
    if protected(&inserted)
        || inserted.encode_utf16().count() > RangeObservation::MAX_INSERTION_UTF16
        || (inserted.encode_utf16().count() == start.replaced
            && <[u8; 32]>::from(Sha256::digest(inserted.as_bytes())) == start.selected_digest)
    {
        return None;
    }
    let proof = with_frozen_target_read(&target, |lease| unsafe {
        let (control, _) = lease.ax_objects();
        let control = control.cast();
        let length = inserted.encode_utf16().count();
        let end = start.start.checked_add(length).ok_or("range unavailable")?;
        let expected_total = start
            .total
            .checked_sub(start.replaced)
            .and_then(|total| total.checked_add(length))
            .ok_or("length unavailable")?;
        let caret = CFRange {
            location: isize::try_from(end).map_err(|_| "range unavailable")?,
            length: 0,
        };
        if copy_index_attr(control, b"AXNumberOfCharacters\0") != Some(expected_total)
            || copy_selected_range(control) != Some(caret)
            || context::read_exact(control, start.start, length)? != inserted
        {
            return Err("delivery not proven".into());
        }
        let before_len = start.before.encode_utf16().count();
        if context::read_exact(control, start.start - before_len, before_len)? != start.before
            || context::read_exact(control, end, start.after.encode_utf16().count())? != start.after
            || copy_selected_range(control) != Some(caret)
            || copy_index_attr(control, b"AXNumberOfCharacters\0") != Some(expected_total)
        {
            return Err("delivery changed".into());
        }
        RangeObservation::delivered(
            start.start,
            inserted,
            expected_total,
            start.before,
            start.after,
        )
        .ok_or_else(|| "range unavailable".into())
    })
    .ok()?;
    Some(DeliveredInsertion { target, proof })
}

fn protected(text: &str) -> bool {
    openless_core::expression_observation::contains_sensitive_content(text)
}

#[derive(PartialEq, Eq)]
struct Sample {
    total: usize,
    text: String,
    before: String,
    after: String,
    caret: (usize, usize),
}

fn sample(delivery: &DeliveredInsertion, stop: &AtomicBool) -> Option<Sample> {
    if stop.load(Ordering::Acquire) {
        return None;
    }
    with_frozen_target_read(&delivery.target, |lease| unsafe {
        if stop.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        let (control, _) = lease.ax_objects();
        let control = control.cast();
        // Metadata/position first: do not read text for an out-of-range caret.
        let total =
            copy_index_attr(control, b"AXNumberOfCharacters\0").ok_or("length unavailable")?;
        let range = copy_selected_range(control).ok_or("range unavailable")?;
        let caret = (
            usize::try_from(range.location).map_err(|_| "range unavailable")?,
            usize::try_from(range.length).map_err(|_| "range unavailable")?,
        );
        let (start, length) = delivery.proof.range(total).ok_or("range unavailable")?;
        if caret.1 != 0
            || caret.0 < start
            || caret
                .0
                .checked_add(caret.1)
                .is_none_or(|end| end > start + length)
        {
            return Err("caret outside insertion".into());
        }
        let before_len = delivery.proof.before().encode_utf16().count();
        let before = context::read_exact(control, start - before_len, before_len)?;
        let after = context::read_exact(
            control,
            start + length,
            delivery.proof.after().encode_utf16().count(),
        )?;
        if before != delivery.proof.before() || after != delivery.proof.after() {
            return Err("range boundary changed".into());
        }
        let text = context::read_exact(control, start, length)?;
        if protected(&text)
            || protected(&before)
            || protected(&after)
            || copy_selected_range(control) != Some(range)
            || copy_index_attr(control, b"AXNumberOfCharacters\0") != Some(total)
        {
            return Err("document changed or protected".into());
        }
        Ok(Sample {
            total,
            text,
            before,
            after,
            caret,
        })
    })
    .ok()
}

struct Watch {
    delivery: RefCell<DeliveredInsertion>,
    stop: Arc<AtomicBool>,
    changed: Cell<Option<Instant>>,
    on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
}

unsafe extern "C" fn changed(
    _: AxObserverRef,
    _: AxUiElementRef,
    notification: CFStringRef,
    data: *mut c_void,
) {
    if data.is_null() {
        return;
    }
    let watch = &*(data as *const Watch);
    if watch.stop.load(Ordering::Acquire) {
        return;
    }
    match cfstring_to_rust(notification).as_deref() {
        Some("AXValueChanged" | "AXSelectedTextChanged") => {
            // A caret/selection leaving the known range ends collection at the
            // event boundary, even if it returns before the quiet timer.
            let delivery = watch.delivery.borrow();
            let valid = with_frozen_target_read(&delivery.target, |lease| {
                let (control, _) = lease.ax_objects();
                let control = control.cast();
                let total =
                    copy_index_attr(control, b"AXNumberOfCharacters\0").ok_or("unavailable")?;
                let selected = copy_selected_range(control).ok_or("unavailable")?;
                let (start, length) = delivery.proof.known_range().ok_or("unavailable")?;
                let caret = usize::try_from(selected.location).map_err(|_| "unavailable")?;
                let size = usize::try_from(selected.length).map_err(|_| "unavailable")?;
                if caret < start
                    || caret
                        .checked_add(size)
                        .is_none_or(|end| end > start + length)
                {
                    return Err("range changed".into());
                }
                // AX notifications supply no edit span. Field-length changes
                // cannot advance the known range or authorize any text read.
                Ok(delivery.proof.range(total).is_some())
            });
            match valid {
                Ok(true) => watch.changed.set(Some(Instant::now())),
                Ok(false) => watch.changed.set(None),
                Err(_) => watch.stop.store(true, Ordering::Release),
            }
        }
        // Focus/window loss or destruction permanently invalidates this watch,
        // including A -> B -> A. A matching later snapshot cannot revive it.
        _ => watch.stop.store(true, Ordering::Release),
    }
}

struct End(Box<dyn Fn() + Send + Sync>);
impl Drop for End {
    fn drop(&mut self) {
        (self.0)();
    }
}

pub(crate) fn spawn(
    delivery: DeliveredInsertion,
    lifetime: Duration,
    on_edit: Box<dyn Fn(EditPair) -> bool + Send + Sync>,
    on_end: Box<dyn Fn() + Send + Sync>,
) -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    std::thread::Builder::new()
        .name("openless-cursor-edit-watch".into())
        .spawn(move || {
            let _end = End(on_end);
            let watch = Watch {
                delivery: RefCell::new(delivery),
                stop: thread_stop,
                changed: Cell::new(None),
                on_edit,
            };
            run(watch, lifetime.min(Duration::from_secs(60)));
        })
        .ok()?;
    Some(stop)
}

fn run(watch: Watch, lifetime: Duration) {
    if watch.stop.load(Ordering::Acquire) {
        return;
    }
    let target = watch.delivery.borrow().target.clone();
    // Borrow original AX objects only inside the synchronous P4 owner callback.
    // The one existing AX observer is registered/unregistered within that lease.
    let _ = with_frozen_target_read(&target, |lease| unsafe {
        let (control, window) = lease.ax_objects();
        let control = control.cast();
        let window = window.cast();
        let app = AXUIElementCreateApplication(lease.process_id());
        if app.is_null() {
            return Err("observer unavailable".into());
        }
        let mut observer = std::ptr::null_mut();
        if AXObserverCreate(lease.process_id(), changed, &mut observer) != AX_ERROR_SUCCESS
            || observer.is_null()
        {
            CFRelease(app.cast());
            return Err("observer unavailable".into());
        }
        let mut registered = Vec::new();
        let mut ready = true;
        for (object, name) in [
            (control, &b"AXValueChanged\0"[..]),
            (control, &b"AXSelectedTextChanged\0"[..]),
            (control, &b"AXUIElementDestroyed\0"[..]),
            (window, &b"AXUIElementDestroyed\0"[..]),
            (app, &b"AXFocusedUIElementChanged\0"[..]),
            (app, &b"AXFocusedWindowChanged\0"[..]),
            (app, &b"AXApplicationDeactivated\0"[..]),
        ] {
            let Some(notification) = cfstring_from_static(name) else {
                ready = false;
                break;
            };
            if AXObserverAddNotification(
                observer,
                object,
                notification,
                &watch as *const _ as *mut c_void,
            ) != AX_ERROR_SUCCESS
            {
                CFRelease(notification);
                ready = false;
                break;
            }
            registered.push((object, notification));
        }
        let raw_source = AXObserverGetRunLoopSource(observer);
        if raw_source.is_null() {
            for (object, notification) in registered {
                AXObserverRemoveNotification(observer, object, notification);
                CFRelease(notification);
            }
            CFRelease(observer.cast());
            CFRelease(app.cast());
            return Err("observer source unavailable".into());
        }
        let source = CFRunLoopSource::wrap_under_get_rule(raw_source);
        let runloop = CFRunLoop::get_current();
        let mode = kCFRunLoopDefaultMode;
        if ready {
            runloop.add_source(&source, mode);
            let started = Instant::now();
            // Recheck delivery after registration. No timeout/contains fallback.
            let initial = sample(&watch.delivery.borrow(), &watch.stop);
            if initial
                .as_ref()
                .is_none_or(|sample| sample.text != watch.delivery.borrow().proof.original())
            {
                watch.stop.store(true, Ordering::Release);
            }
            while !watch.stop.load(Ordering::Acquire) && started.elapsed() < lifetime {
                if matches!(
                    CFRunLoop::run_in_mode(mode, Duration::from_millis(100), false),
                    CFRunLoopRunResult::Finished
                ) {
                    watch.stop.store(true, Ordering::Release);
                    break;
                }
                if watch.stop.load(Ordering::Acquire) {
                    break;
                }
                // Even without value events, focus/permission/range loss must
                // end the watch. This probes metadata/gates before any text.
                if !lease.is_current()
                    || crate::permissions::check_accessibility()
                        != crate::permissions::PermissionStatus::Granted
                    || crate::unicode_keystroke::is_secure_input_enabled()
                {
                    watch.stop.store(true, Ordering::Release);
                    break;
                }
                let Some(changed_at) = watch.changed.get().filter(|at| at.elapsed() >= QUIET)
                else {
                    continue;
                };
                let first = sample(&watch.delivery.borrow(), &watch.stop);
                CFRunLoop::run_in_mode(mode, Duration::ZERO, true);
                if watch.stop.load(Ordering::Acquire) || watch.changed.get() != Some(changed_at) {
                    continue;
                }
                let second = sample(&watch.delivery.borrow(), &watch.stop);
                let (Some(first), Some(second)) = (first, second) else {
                    watch.stop.store(true, Ordering::Release);
                    break;
                };
                if first != second {
                    watch.changed.set(Some(Instant::now()));
                    continue;
                }
                watch.changed.set(None);
                if !watch.stop.load(Ordering::Acquire) && lease.is_current() {
                    let valid = watch.delivery.borrow_mut().proof.observe(
                        second.total,
                        &second.text,
                        &second.before,
                        &second.after,
                        second.caret,
                        |edit| (watch.on_edit)(edit),
                    );
                    if !valid {
                        watch.stop.store(true, Ordering::Release);
                    }
                }
            }
            // Never flush on focus loss, cancellation or timeout.
            runloop.remove_source(&source, mode);
        }
        for (object, notification) in registered {
            AXObserverRemoveNotification(observer, object, notification);
            CFRelease(notification);
        }
        CFRelease(observer.cast());
        CFRelease(app.cast());
        Ok(())
    });
}
