use super::*;
use std::time::{Duration, Instant};
use x11rb::{connection::Connection, protocol::{Event, xproto::{self, ConnectionExt, AtomEnum, EventMask, PropMode, SelectionNotifyEvent, WindowClass}}, wrapper::ConnectionExt as _};
pub(super) struct Clipboard {
    connection: x11rb::rust_connection::RustConnection, window: u32,
    selection: u32, property: u32, targets: u32, timestamp: u32, utf8: u32,
    own: u32, mime: u32, bytes: Vec<u8>, revision: u64, last: Option<(u32, u32)>,
}
impl Clipboard {
    pub fn connect() -> Option<Self> {
        let (connection, screen) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id().ok()?;
        connection.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 1, 1, 0,
            WindowClass::INPUT_OUTPUT, 0, &xproto::CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE)).ok()?.check().ok()?;
        let atom = |name: &[u8]| connection.intern_atom(false, name).ok()?.reply().ok().map(|r| r.atom);
        let selection = atom(b"CLIPBOARD")?; let property = atom(b"OPENLESS_CAPTURE")?;
        let targets = atom(b"TARGETS")?; let timestamp = atom(b"TIMESTAMP")?;
        let utf8 = atom(b"UTF8_STRING")?; let own = atom(b"application/x-openless-clipboard")?;
        Some(Self { connection, window, selection, property, targets, timestamp, utf8, own,
            mime: utf8, bytes: Vec::new(), revision: 0, last: None })
    }
    fn event(&mut self, event: Event) {
        if let Event::SelectionRequest(request) = event {
            let property = if request.property == 0 { request.target } else { request.property };
            let valid = if request.target == self.targets {
                self.connection.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM,
                    &[self.targets, self.timestamp, self.mime, self.own]).is_ok()
            } else if request.target == self.timestamp {
                self.connection.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::INTEGER, &[0]).is_ok()
            } else if request.target == self.mime || request.target == self.own {
                let bytes = if request.target == self.own { b"OpenLess".as_slice() } else { self.bytes.as_slice() };
                self.connection.change_property8(PropMode::REPLACE, request.requestor, property, request.target, bytes).is_ok()
            } else { false };
            let notify = SelectionNotifyEvent { response_type: xproto::SELECTION_NOTIFY_EVENT, sequence: 0,
                time: request.time, requestor: request.requestor, selection: request.selection,
                target: request.target, property: if valid { property } else { 0 } };
            let _ = self.connection.send_event(false, request.requestor, EventMask::NO_EVENT, notify);
            let _ = self.connection.flush();
        }
    }
    pub fn pump(&mut self) { while let Ok(Some(event)) = self.connection.poll_for_event() { self.event(event); } }
    fn receive(&mut self, target: u32) -> Option<Vec<u8>> {
        self.connection.delete_property(self.window, self.property).ok()?;
        self.connection.convert_selection(self.window, self.selection, target, self.property, x11rb::CURRENT_TIME).ok()?;
        self.connection.flush().ok()?;
        let deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < deadline {
            if let Some(event) = self.connection.poll_for_event().ok()? {
                if let Event::SelectionNotify(notify) = &event {
                    if notify.requestor == self.window && notify.target == target {
                        if notify.property == 0 { return None; }
                        let reply = self.connection.get_property(true, self.window, self.property, AtomEnum::ANY, 0,
                            (openless_core::dictation_references::MAX_BYTES / 4 + 1) as u32).ok()?.reply().ok()?;
                        if reply.bytes_after > 0 { return Some(vec![0; openless_core::dictation_references::MAX_BYTES as usize + 1]); }
                        return Some(reply.value);
                    }
                }
                self.event(event);
            } else { std::thread::sleep(Duration::from_millis(2)); }
        }
        None
    }
    pub fn read(&mut self) -> Option<Snapshot> {
        let owner = self.connection.get_selection_owner(self.selection).ok()?.reply().ok()?.owner;
        let timestamp = if owner == self.window || owner == 0 { 0 } else {
            let bytes = self.receive(self.timestamp)?;
            u32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?)
        };
        let key = (owner, timestamp);
        if self.last != Some(key) { self.revision = self.revision.wrapping_add(1); self.last = Some(key); }
        let revision = self.revision;
        if owner == self.window || owner == 0 { return Some(Snapshot { revision, own: owner == self.window, content: None }); }
        let available = self.receive(self.targets)?;
        let atoms = available.chunks_exact(4).map(|a| u32::from_ne_bytes(a.try_into().unwrap())).collect::<Vec<_>>();
        if atoms.contains(&self.own) { return Some(Snapshot { revision, own: true, content: None }); }
        let mut selected = None;
        for mime in ["text/uri-list", "image/png", "UTF8_STRING", "text/plain;charset=utf-8", "text/plain"] {
            let atom = self.connection.intern_atom(false, mime.as_bytes()).ok()?.reply().ok()?.atom;
            if atoms.contains(&atom) { selected = Some((mime, atom)); break; }
        }
        let (mime, atom) = selected?;
        let bytes = self.receive(atom)?;
        let current = self.connection.get_selection_owner(self.selection).ok()?.reply().ok()?.owner;
        if current != owner { return None; }
        // TIMESTAMP must still match after delayed selection rendering.
        let after = self.receive(self.timestamp)?;
        if after.get(..4)? != timestamp.to_ne_bytes() { return None; }
        Some(Snapshot { revision, own: false, content: super::content(mime, bytes) })
    }
    pub fn write(&mut self, text: String) -> bool {
        self.write_bytes("UTF8_STRING", text.into_bytes())
    }
    pub fn write_bytes(&mut self, mime: &str, bytes: Vec<u8>) -> bool {
        let Ok(cookie) = self.connection.intern_atom(false, mime.as_bytes()) else { return false; };
        let Ok(reply) = cookie.reply() else { return false; };
        self.mime = reply.atom;
        self.bytes = bytes;
        self.connection.set_selection_owner(self.window, self.selection, x11rb::CURRENT_TIME).is_ok() && self.connection.flush().is_ok()
    }
}
