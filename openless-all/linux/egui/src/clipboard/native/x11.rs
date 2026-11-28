use super::*;
use std::{time::{Duration, Instant}, collections::HashMap, sync::Arc};
use x11rb::{connection::Connection, protocol::{Event, xproto::{self, ConnectionExt, AtomEnum, EventMask, PropMode, SelectionNotifyEvent, WindowClass}}, wrapper::ConnectionExt as _};
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
pub(super) struct Clipboard {
    connection: x11rb::rust_connection::RustConnection, window: u32,
    selection: u32, property: u32, targets: u32, timestamp: u32, utf8: u32,
    own: u32, mime: u32, bytes: Arc<Vec<u8>>, revision: u64, incr: u32,
    transfers: HashMap<(u32, u32), Transfer>,
    token: String,
}
struct Transfer { bytes: Arc<Vec<u8>>, offset: usize, target: u32 }
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
        let incr = atom(b"INCR")?;
        connection.xfixes_query_version(5, 0).ok()?.reply().ok()?;
        connection.xfixes_select_selection_input(window, selection, SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY | SelectionEventMask::SELECTION_CLIENT_CLOSE).ok()?.check().ok()?;
        Some(Self { connection, window, selection, property, targets, timestamp, utf8, own,
            mime: utf8, bytes: Arc::new(Vec::new()), revision: 1, incr, transfers: HashMap::new(), token: String::new() })
    }
    fn event(&mut self, event: Event) {
        if let Event::XfixesSelectionNotify(notify) = &event {
            if notify.selection == self.selection { self.revision = self.revision.wrapping_add(1); }
        }
        if let Event::PropertyNotify(notify) = &event {
            if notify.state == xproto::Property::DELETE {
                let key = (notify.window, notify.atom);
                if let Some(transfer) = self.transfers.get_mut(&key) {
                    let end = (transfer.offset + 65536).min(transfer.bytes.len());
                    let _ = self.connection.change_property8(PropMode::REPLACE, notify.window, notify.atom,
                        transfer.target, &transfer.bytes[transfer.offset..end]);
                    let complete = transfer.offset == end;
                    transfer.offset = end;
                    if complete { self.transfers.remove(&key); }
                    let _ = self.connection.flush();
                    return;
                }
            }
        }
        if let Event::SelectionRequest(request) = event {
            let property = if request.property == 0 { request.target } else { request.property };
            let valid = if request.target == self.targets {
                self.connection.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM,
                    &[self.targets, self.timestamp, self.mime, self.own]).is_ok()
            } else if request.target == self.timestamp {
                self.connection.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::INTEGER, &[0]).is_ok()
            } else if request.target == self.mime || request.target == self.own {
                let bytes = if request.target == self.own { b"OpenLess".as_slice() } else { self.bytes.as_slice() };
                if bytes.len() > 65536 && self.transfers.len() < 8 {
                    let _ = self.connection.change_window_attributes(request.requestor,
                        &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE));
                    self.transfers.insert((request.requestor, property), Transfer { bytes: self.bytes.clone(), offset: 0, target: request.target });
                    self.connection.change_property32(PropMode::REPLACE, request.requestor, property, self.incr, &[bytes.len() as u32]).is_ok()
                } else if bytes.len() <= 65536 {
                    self.connection.change_property8(PropMode::REPLACE, request.requestor, property, request.target, bytes).is_ok()
                } else { false }
            } else { false };
            let notify = SelectionNotifyEvent { response_type: xproto::SELECTION_NOTIFY_EVENT, sequence: 0,
                time: request.time, requestor: request.requestor, selection: request.selection,
                target: request.target, property: if valid { property } else { 0 } };
            let _ = self.connection.send_event(false, request.requestor, EventMask::NO_EVENT, notify);
            let _ = self.connection.flush();
        }
    }
    pub fn pump(&mut self) -> bool {
        loop { match self.connection.poll_for_event() { Ok(Some(event)) => self.event(event), Ok(None) => return true, Err(_) => return false } }
    }
    fn receive(&mut self, target: u32) -> Option<Vec<u8>> {
        self.connection.delete_property(self.window, self.property).ok()?;
        self.connection.convert_selection(self.window, self.selection, target, self.property, x11rb::CURRENT_TIME).ok()?;
        self.connection.flush().ok()?;
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            if let Some(event) = self.connection.poll_for_event().ok()? {
                if let Event::SelectionNotify(notify) = &event {
                    if notify.requestor == self.window && notify.target == target {
                        if notify.property == 0 { return None; }
                        let reply = self.connection.get_property(true, self.window, self.property, AtomEnum::ANY, 0,
                            (openless_core::dictation_references::MAX_BYTES / 4 + 1) as u32).ok()?.reply().ok()?;
                        if reply.type_ == self.incr {
                            self.connection.flush().ok()?;
                            return self.receive_incremental(deadline);
                        }
                        if reply.bytes_after > 0 { return Some(vec![0; openless_core::dictation_references::MAX_BYTES as usize + 1]); }
                        return Some(reply.value);
                    }
                }
                self.event(event);
            } else { std::thread::sleep(Duration::from_millis(2)); }
        }
        None
    }
    fn receive_incremental(&mut self, deadline: Instant) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        while Instant::now() < deadline {
            if let Some(event) = self.connection.poll_for_event().ok()? {
                if let Event::PropertyNotify(notify) = &event {
                    if notify.window == self.window && notify.atom == self.property && notify.state == xproto::Property::NEW_VALUE {
                        let reply = self.connection.get_property(true, self.window, self.property, AtomEnum::ANY, 0,
                            (openless_core::dictation_references::MAX_BYTES / 4 + 1) as u32).ok()?.reply().ok()?;
                        if reply.type_ == 0 { continue; }
                        if reply.value.is_empty() { return Some(bytes); }
                        bytes.extend_from_slice(&reply.value);
                        self.connection.flush().ok()?;
                        if bytes.len() as u64 > openless_core::dictation_references::MAX_BYTES { return Some(bytes); }
                        continue;
                    }
                }
                self.event(event);
            } else { std::thread::sleep(Duration::from_millis(2)); }
        }
        None
    }
    pub fn read(&mut self) -> Option<Snapshot> {
        self.pump();
        let owner = self.connection.get_selection_owner(self.selection).ok()?.reply().ok()?.owner;
        self.pump();
        let revision = self.revision;
        if owner == self.window || owner == 0 { return Some(Snapshot { revision, own: owner == self.window, content: None }); }
        let available = self.receive(self.targets)?;
        let atoms = available.chunks_exact(4).map(|a| u32::from_ne_bytes(a.try_into().unwrap())).collect::<Vec<_>>();
        if atoms.contains(&self.own) { return Some(Snapshot { revision, own: true, content: None }); }
        let mut selected = None;
        for mime in ["text/uri-list", "image/png", "image/jpeg", "image/tiff", "UTF8_STRING", "text/plain;charset=utf-8", "text/plain"] {
            let atom = self.connection.intern_atom(false, mime.as_bytes()).ok()?.reply().ok()?.atom;
            if atoms.contains(&atom) { selected = Some((mime, atom)); break; }
        }
        let Some((mime, atom)) = selected else { return Some(Snapshot { revision, own: false, content: None }); };
        let bytes = self.receive(atom)?;
        let current = self.connection.get_selection_owner(self.selection).ok()?.reply().ok()?.owner;
        if current != owner { return None; }
        self.pump();
        if self.revision != revision { return None; }
        Some(Snapshot { revision, own: false, content: super::content(mime, bytes) })
    }
    pub fn write(&mut self, text: String) -> Option<Receipt> {
        self.write_bytes("UTF8_STRING", text.into_bytes())
    }
    pub fn write_bytes(&mut self, mime: &str, bytes: Vec<u8>) -> Option<Receipt> {
        let cookie = self.connection.intern_atom(false, mime.as_bytes()).ok()?;
        let reply = cookie.reply().ok()?;
        self.mime = reply.atom;
        self.bytes = Arc::new(bytes);
        self.token = uuid::Uuid::new_v4().to_string();
        self.connection.set_selection_owner(self.window, self.selection, x11rb::CURRENT_TIME).ok()?.check().ok()?;
        self.connection.flush().ok()?;
        if !self.pump() { return None; }
        let receipt = Receipt { revision: self.revision, token: self.token.clone() };
        self.owns(&receipt).then_some(receipt)
    }
    pub fn owns(&mut self, receipt: &Receipt) -> bool {
        if !self.pump() || self.token != receipt.token { return false; }
        let owner = self.connection.get_selection_owner(self.selection).ok().and_then(|cookie| cookie.reply().ok()).map(|reply| reply.owner);
        self.pump() && owner == Some(self.window) && self.revision == receipt.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, process::{Child, Command, Stdio}, sync::mpsc};
    struct UserCopy(Child);
    impl Drop for UserCopy { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    fn user_copy(mime: &str, bytes: &[u8]) -> UserCopy {
        let mut child = Command::new("xclip").args(["-selection", "clipboard", "-in", "-quiet", "-t", mime])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        UserCopy(child)
    }
    fn wait_external(board: &mut Clipboard) -> Snapshot {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(snapshot) = board.read().filter(|s| !s.own && s.content.is_some()) { return snapshot; }
            assert!(Instant::now() < deadline, "external clipboard offer did not arrive");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "requires the dedicated Xvfb and xclip cloud fixture"]
    fn references_x11_external_copies_and_app_owned_transfers_are_distinct() {
        let mut board = Clipboard::connect().expect("Xvfb provides CLIPBOARD and XFixes");
        // Exercise real INCR reception, including full copied text/newlines.
        let text = format!("{}\nsecond line 🙂", "user material ".repeat(30000));
        let user = user_copy("UTF8_STRING", text.as_bytes());
        let first = wait_external(&mut board);
        assert!(matches!(first.content, Some(ClipboardContent::Text(ref body)) if body == &text));
        let own = board.write(text.clone()).unwrap();
        assert!(board.owns(&own));
        board.pump();
        assert!(board.read().unwrap().own);
        drop(user);
        // A later identical real user copy has a new native ownership event,
        // and is never excluded merely because its bytes equal an app write.
        let user_again = user_copy("UTF8_STRING", text.as_bytes());
        let second = wait_external(&mut board);
        assert!(!board.owns(&own), "external B replaced the app offer before native dispatch");
        assert_ne!(second.revision, first.revision);
        assert!(matches!(second.content, Some(ClipboardContent::Text(ref body)) if body == &text));
        drop(user_again);
        // Exercise native image output, including INCR serving to a separate
        // X11 client, rather than only verifying a mocked paste command.
        let image = (0..1_000_000u32).map(|n| n.wrapping_mul(113).wrapping_add(n >> 8) as u8).collect::<Vec<_>>();
        assert!(board.write_bytes("image/png", image.clone()).is_some());
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let output = Command::new("xclip").args(["-selection", "clipboard", "-out", "-t", "image/png"]).output().unwrap();
            send.send(output).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let output = loop {
            board.pump();
            if let Ok(output) = receive.try_recv() { break output; }
            assert!(Instant::now() < deadline, "native INCR image transfer timed out");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert!(output.status.success());
        assert_eq!(output.stdout, image);
    }
}
