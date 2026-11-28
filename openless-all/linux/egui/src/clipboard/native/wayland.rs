use super::*;
use std::{collections::HashMap, io::{Read, Write}, os::{fd::{AsFd, AsRawFd}, unix::net::UnixStream}, sync::Arc, time::{Duration, Instant}};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, Proxy, globals::{registry_queue_init, GlobalListContents}, protocol::{wl_registry, wl_seat}};
use wayland_protocols_wlr::data_control::v1::client::{zwlr_data_control_manager_v1 as manager, zwlr_data_control_device_v1 as device, zwlr_data_control_offer_v1 as offer, zwlr_data_control_source_v1 as source};
const OWN: &str = "application/x-openless-clipboard";
const TEXT: &str = "text/plain;charset=utf-8";
#[derive(Default)]
struct State {
    revision: u64,
    selected: Option<offer::ZwlrDataControlOfferV1>,
    mimes: HashMap<wayland_client::backend::ObjectId, Vec<String>>,
    finished: bool,
}
pub(super) struct Clipboard {
    connection: Connection, queue: EventQueue<State>, state: State,
    manager: manager::ZwlrDataControlManagerV1, device: device::ZwlrDataControlDeviceV1,
    source: Option<source::ZwlrDataControlSourceV1>,
}
impl Clipboard {
    pub fn connect() -> Option<Self> {
        let connection = Connection::connect_to_env().ok()?;
        let (globals, mut queue) = registry_queue_init::<State>(&connection).ok()?;
        let qh = queue.handle();
        let manager: manager::ZwlrDataControlManagerV1 = globals.bind(&qh, 1..=2, ()).ok()?;
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).ok()?;
        let device = manager.get_data_device(&seat, &qh, ());
        let mut state = State::default();
        queue.roundtrip(&mut state).ok()?;
        if state.finished { return None; }
        Some(Self { connection, queue, state, manager, device, source: None })
    }
    pub fn pump(&mut self) -> bool {
        if let Some(reader) = self.queue.prepare_read() {
            let mut fd = libc::pollfd { fd: reader.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut fd, 1, 0) } > 0 {
                if let Err(error) = reader.read() {
                    if !matches!(error, wayland_client::backend::WaylandError::Io(ref e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted)) { self.state.finished = true; }
                }
            }
        }
        if self.queue.dispatch_pending(&mut self.state).is_err() || self.connection.flush().is_err() { self.state.finished = true; }
        !self.state.finished
    }
    pub fn read(&mut self) -> Option<Snapshot> {
        self.queue.roundtrip(&mut self.state).ok()?;
        if self.state.finished { return None; }
        let revision = self.state.revision;
        let Some(offer) = self.state.selected.clone() else { return Some(Snapshot { revision, own: false, content: None }); };
        let mimes = self.state.mimes.get(&offer.id())?;
        if mimes.iter().any(|m| m == OWN) { return Some(Snapshot { revision, own: true, content: None }); }
        let mime = ["text/uri-list", "image/png", "image/jpeg", "image/tiff", TEXT, "text/plain"].into_iter().find(|mime| mimes.iter().any(|m| m == mime));
        let Some(mime) = mime else { return Some(Snapshot { revision, own: false, content: None }); };
        let (mut reader, writer) = UnixStream::pair().ok()?;
        reader.set_nonblocking(true).ok()?;
        offer.receive(mime.into(), writer.as_fd());
        self.connection.flush().ok()?;
        drop(writer);
        let mut bytes = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(150);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now()).as_millis();
            if remaining == 0 { return None; }
            let mut fd = libc::pollfd { fd: reader.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut fd, 1, remaining as i32) } <= 0 { return None; }
            let mut chunk = [0; 65536];
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => { bytes.extend_from_slice(&chunk[..n]); if bytes.len() as u64 > openless_core::dictation_references::MAX_BYTES { break; } }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(_) => return None,
            }
        }
        self.queue.roundtrip(&mut self.state).ok()?;
        if self.state.revision != revision { return None; }
        Some(Snapshot { revision, own: false, content: super::content(mime, bytes) })
    }
    pub fn write(&mut self, text: String) -> Option<Receipt> {
        self.write_bytes(TEXT, text.into_bytes())
    }
    pub fn write_bytes(&mut self, mime: &str, bytes: Vec<u8>) -> Option<Receipt> {
        if self.state.finished { return None; }
        let qh = self.queue.handle();
        let source = self.manager.create_data_source(&qh, Arc::new(bytes));
        source.offer(mime.into());
        if mime == TEXT { source.offer("text/plain".into()); }
        source.offer(OWN.into());
        let token = format!("{OWN};id={}", uuid::Uuid::new_v4());
        source.offer(token.clone());
        self.device.set_selection(Some(&source));
        if let Some(old) = self.source.replace(source) { old.destroy(); }
        self.queue.roundtrip(&mut self.state).ok()?;
        let receipt = Receipt { revision: self.state.revision, token };
        self.owns(&receipt).then_some(receipt)
    }
    pub fn owns(&mut self, receipt: &Receipt) -> bool {
        if self.queue.roundtrip(&mut self.state).is_err() || self.state.finished || self.state.revision != receipt.revision { return false; }
        self.state.selected.as_ref().and_then(|offer| self.state.mimes.get(&offer.id()))
            .is_some_and(|mimes| mimes.contains(&receipt.token) && mimes.iter().any(|mime| mime == OWN))
    }
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State: ignore manager::ZwlrDataControlManagerV1);
impl Dispatch<device::ZwlrDataControlDeviceV1, ()> for State {
    fn event(state: &mut Self, _: &device::ZwlrDataControlDeviceV1, event: device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            device::Event::Selection { id } => {
                state.revision = state.revision.wrapping_add(1);
                if let Some(old) = state.selected.take() { state.mimes.remove(&old.id()); old.destroy(); }
                state.selected = id;
            }
            device::Event::Finished => state.finished = true,
            device::Event::PrimarySelection { id: Some(offer) } => {
                state.mimes.remove(&offer.id());
                offer.destroy();
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, device::ZwlrDataControlDeviceV1, [0 => (offer::ZwlrDataControlOfferV1, ())]);
}
impl Dispatch<offer::ZwlrDataControlOfferV1, ()> for State {
    fn event(state: &mut Self, proxy: &offer::ZwlrDataControlOfferV1, event: offer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let offer::Event::Offer { mime_type } = event { state.mimes.entry(proxy.id()).or_default().push(mime_type); }
    }
}
impl Dispatch<source::ZwlrDataControlSourceV1, Arc<Vec<u8>>> for State {
    fn event(_: &mut Self, _: &source::ZwlrDataControlSourceV1, event: source::Event, data: &Arc<Vec<u8>>, _: &Connection, _: &QueueHandle<Self>) {
        if let source::Event::Send { mime_type, fd } = event {
            let mut bytes = if mime_type == OWN { b"OpenLess".as_slice() } else { data.as_slice() };
            let mut file = std::fs::File::from(fd);
            let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 { return; }
            let deadline = Instant::now() + Duration::from_millis(750);
            while !bytes.is_empty() && Instant::now() < deadline {
                let mut fd = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
                let remaining = deadline.saturating_duration_since(Instant::now()).as_millis() as i32;
                if unsafe { libc::poll(&mut fd, 1, remaining) } <= 0 { break; }
                match file.write(bytes) { Ok(0) => break, Ok(n) => bytes = &bytes[n..], Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}, Err(_) => break }
            }
        }
    }
}
