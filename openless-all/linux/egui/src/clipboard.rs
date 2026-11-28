//! Native CLIPBOARD access. Wayland requires the compositor's data-control
//! protocol; X11 uses selection-owner timestamps. PRIMARY is never collected.
use openless_core::dictation_references::ClipboardContent;
use std::sync::{mpsc, OnceLock, Arc, atomic::{AtomicBool, Ordering}};

#[derive(Clone, Debug)]
pub(crate) struct Snapshot { pub revision: u64, pub own: bool, pub content: Option<ClipboardContent> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Receipt { pub revision: u64, pub token: String }
enum Request {
    Read(mpsc::Sender<Option<Snapshot>>),
    Write(String, mpsc::Sender<Option<Receipt>>),
    Image(std::path::PathBuf, mpsc::Sender<Option<Receipt>>),
    Owns(Receipt, mpsc::Sender<bool>),
}
struct Service { tx: mpsc::Sender<Request>, live: Arc<AtomicBool> }
struct LiveLease(Arc<AtomicBool>);
impl Drop for LiveLease { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }
impl Service { fn supported(&self) -> bool { self.live.load(Ordering::Acquire) } }
fn service() -> Option<&'static Service> {
    static SERVICE: OnceLock<Option<Service>> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let (ready, result) = mpsc::channel();
        let live = Arc::new(AtomicBool::new(false));
        let worker_live = live.clone();
        std::thread::spawn(move || {
            let _lease = LiveLease(worker_live.clone());
            #[cfg(target_os = "linux")]
            {
                let Some(mut native) = native::Clipboard::connect() else { let _ = ready.send(false); return; };
                worker_live.store(true, Ordering::Release);
                let _ = ready.send(true);
                loop {
                    if !native.pump() { worker_live.store(false, Ordering::Release); break; }
                    match rx.recv_timeout(std::time::Duration::from_millis(30)) {
                        Ok(Request::Read(reply)) => { let _ = reply.send(native.read()); }
                        Ok(Request::Write(text, reply)) => { let _ = reply.send(native.write(text)); }
                        Ok(Request::Image(path, reply)) => { let _ = reply.send(native.image(&path)); }
                        Ok(Request::Owns(receipt, reply)) => { let _ = reply.send(native.owns(&receipt)); }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
                worker_live.store(false, Ordering::Release);
            }
            #[cfg(not(target_os = "linux"))]
            { let _ = (rx, ready.send(false)); }
        });
        result.recv_timeout(std::time::Duration::from_secs(2)).ok().filter(|ok| *ok).map(|_| Service { tx, live })
    }).as_ref()
}
pub(crate) fn supported() -> bool { service().is_some_and(Service::supported) }
pub(crate) fn snapshot() -> Option<Snapshot> {
    let (tx, rx) = mpsc::channel();
    let service = service()?;
    if !service.live.load(Ordering::Acquire) { return None; }
    service.tx.send(Request::Read(tx)).ok()?;
    rx.recv_timeout(std::time::Duration::from_millis(500)).ok().flatten()
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn references_provider_finished_or_worker_disconnect_revokes_live_capability_even_with_a_channel() {
        let (tx, _rx) = mpsc::channel();
        let live = Arc::new(AtomicBool::new(true));
        let service = Service { tx, live: live.clone() };
        assert!(service.supported());
        // The worker's lease is dropped on Finished, disconnect, early return,
        // and unwinding; the still-existing Sender is never the capability.
        drop(LiveLease(live));
        assert!(!service.supported());
    }
}
pub(crate) fn write_text(text: &str) -> bool {
    write_text_receipt(text).is_some()
}
pub(crate) fn write_text_receipt(text: &str) -> Option<Receipt> {
    let (tx, rx) = mpsc::channel();
    let service = service()?;
    if !service.live.load(Ordering::Acquire) { return None; }
    service.tx.send(Request::Write(text.into(), tx)).ok()?;
    rx.recv_timeout(std::time::Duration::from_millis(1000)).ok().flatten()
}
pub(crate) fn write_image(path: &std::path::Path) -> Option<Receipt> {
    let (tx, rx) = mpsc::channel();
    let service = service()?;
    if !service.live.load(Ordering::Acquire) { return None; }
    service.tx.send(Request::Image(path.into(), tx)).ok()?;
    rx.recv_timeout(std::time::Duration::from_millis(1000)).ok().flatten()
}
pub(crate) fn owns(receipt: &Receipt) -> bool {
    let (tx, rx) = mpsc::channel();
    service().is_some_and(|service| service.live.load(Ordering::Acquire)
        && service.tx.send(Request::Owns(receipt.clone(), tx)).is_ok()
        && rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap_or(false))
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    mod wayland;
    #[cfg(feature = "x11-overlay")]
    mod x11;
    pub(super) enum Clipboard {
        Wayland(wayland::Clipboard),
        #[cfg(feature = "x11-overlay")]
        X11(x11::Clipboard),
    }
    impl Clipboard {
        pub fn connect() -> Option<Self> {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                // Never pretend XWayland observes the compositor clipboard.
                return wayland::Clipboard::connect().map(Self::Wayland);
            }
            #[cfg(feature = "x11-overlay")]
            { x11::Clipboard::connect().map(Self::X11) }
            #[cfg(not(feature = "x11-overlay"))]
            { None }
        }
        pub fn pump(&mut self) -> bool { match self { Self::Wayland(c) => c.pump(), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.pump() } }
        pub fn read(&mut self) -> Option<Snapshot> { match self { Self::Wayland(c) => c.read(), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.read() } }
        pub fn write(&mut self, text: String) -> Option<Receipt> { match self { Self::Wayland(c) => c.write(text), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.write(text) } }
        pub fn owns(&mut self, receipt: &Receipt) -> bool { match self { Self::Wayland(c) => c.owns(receipt), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.owns(receipt) } }
        pub fn image(&mut self, path: &std::path::Path) -> Option<Receipt> {
            use std::io::Read;
            let Ok(file) = std::fs::File::open(path) else { return None; };
            let mut bytes = Vec::new();
            if file.take(openless_core::dictation_references::MAX_BYTES + 1).read_to_end(&mut bytes).is_err()
                || bytes.len() as u64 > openless_core::dictation_references::MAX_BYTES { return None; }
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
            let mime = match extension.as_str() { "jpg" | "jpeg" => "image/jpeg", "tif" | "tiff" => "image/tiff", _ => "image/png" };
            match self { Self::Wayland(c) => c.write_bytes(mime, bytes), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.write_bytes(mime, bytes) }
        }
    }
    pub(super) fn content(mime: &str, bytes: Vec<u8>) -> Option<ClipboardContent> {
        if bytes.len() as u64 > openless_core::dictation_references::MAX_BYTES { return Some(ClipboardContent::TooLarge); }
        match mime {
            "image/png" => return Some(ClipboardContent::Image(bytes, "png")),
            "image/jpeg" => return Some(ClipboardContent::Image(bytes, "jpg")),
            "image/tiff" => return Some(ClipboardContent::Image(bytes, "tiff")),
            _ => {}
        }
        let text = String::from_utf8(bytes).ok()?;
        if mime == "text/uri-list" {
            let paths = openless_core::dictation_references::clipboard_uri_paths(&text);
            return (!paths.is_empty()).then_some(ClipboardContent::Files(paths));
        }
        Some(ClipboardContent::Text(text))
    }
}
