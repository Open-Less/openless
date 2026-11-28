//! Native CLIPBOARD access. Wayland requires the compositor's data-control
//! protocol; X11 uses selection-owner timestamps. PRIMARY is never collected.
use openless_core::dictation_references::ClipboardContent;
use std::sync::{mpsc, OnceLock};

#[derive(Clone, Debug)]
pub(crate) struct Snapshot { pub revision: u64, pub own: bool, pub content: Option<ClipboardContent> }
enum Request {
    Read(mpsc::Sender<Option<Snapshot>>),
    Write(String, mpsc::Sender<bool>),
    Image(std::path::PathBuf, mpsc::Sender<bool>),
}
fn service() -> Option<&'static mpsc::Sender<Request>> {
    static SERVICE: OnceLock<Option<mpsc::Sender<Request>>> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let (ready, result) = mpsc::channel();
        std::thread::spawn(move || {
            #[cfg(target_os = "linux")]
            {
                let Some(mut native) = native::Clipboard::connect() else { let _ = ready.send(false); return; };
                let _ = ready.send(true);
                loop {
                    match rx.recv_timeout(std::time::Duration::from_millis(30)) {
                        Ok(Request::Read(reply)) => { let _ = reply.send(native.read()); }
                        Ok(Request::Write(text, reply)) => { let _ = reply.send(native.write(text)); }
                        Ok(Request::Image(path, reply)) => { let _ = reply.send(native.image(&path)); }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    native.pump();
                }
            }
            #[cfg(not(target_os = "linux"))]
            { let _ = (rx, ready.send(false)); }
        });
        result.recv_timeout(std::time::Duration::from_secs(2)).ok().filter(|ok| *ok).map(|_| tx)
    }).as_ref()
}
pub(crate) fn supported() -> bool { service().is_some() }
pub(crate) fn snapshot() -> Option<Snapshot> {
    let (tx, rx) = mpsc::channel();
    service()?.send(Request::Read(tx)).ok()?;
    rx.recv_timeout(std::time::Duration::from_millis(500)).ok().flatten()
}
pub(crate) fn write_text(text: &str) -> bool {
    let (tx, rx) = mpsc::channel();
    service().is_some_and(|service| service.send(Request::Write(text.into(), tx)).is_ok()
        && rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap_or(false))
}
pub(crate) fn write_image(path: &std::path::Path) -> bool {
    let (tx, rx) = mpsc::channel();
    service().is_some_and(|service| service.send(Request::Image(path.into(), tx)).is_ok()
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
        pub fn pump(&mut self) { match self { Self::Wayland(c) => c.pump(), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.pump() } }
        pub fn read(&mut self) -> Option<Snapshot> { match self { Self::Wayland(c) => c.read(), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.read() } }
        pub fn write(&mut self, text: String) -> bool { match self { Self::Wayland(c) => c.write(text), #[cfg(feature = "x11-overlay")] Self::X11(c) => c.write(text) } }
        pub fn image(&mut self, path: &std::path::Path) -> bool {
            use std::io::Read;
            let Ok(file) = std::fs::File::open(path) else { return false; };
            let mut bytes = Vec::new();
            if file.take(openless_core::dictation_references::MAX_BYTES + 1).read_to_end(&mut bytes).is_err()
                || bytes.len() as u64 > openless_core::dictation_references::MAX_BYTES { return false; }
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
