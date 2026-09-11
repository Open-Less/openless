use crate::context::platform;
use crate::desktop_bridge::{DesktopAdapter, DesktopBinding, DesktopSnapshot};
use openless_core::BackendError;
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt, EventMask, GrabMode, ModMask};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

fn modifier_mask(symbol: u32) -> u16 {
    match symbol {
        0xffe1 | 0xffe2 => 1,
        0xffe3 | 0xffe4 => 4,
        0xffe9 | 0xffea => 8,
        0xffeb | 0xffec => 64,
        _ => 0,
    }
}
fn passive_modifier(binding: &DesktopBinding) -> bool {
    binding.states == 0 && modifier_mask(binding.symbol) != 0
}

type Reply = mpsc::SyncSender<Result<(), BackendError>>;
pub struct X11Desktop {
    commands: mpsc::Sender<(Vec<DesktopBinding>, Reply)>,
    events: Arc<Mutex<Vec<crate::LinuxHotkeyEvent>>>,
}
impl X11Desktop {
    pub fn start() -> Result<Self, BackendError> {
        let (connection, screen) = x11rb::connect(None).map_err(platform)?;
        let root = connection.setup().roots[screen].root;
        connection
            .xinput_xi_query_version(2, 0)
            .map_err(platform)?
            .reply()
            .map_err(platform)?;
        connection
            .xinput_xi_select_events(
                root,
                &[xinput::EventMask {
                    deviceid: 1, // XIAllMasterDevices; no text is decoded or retained.
                    mask: vec![
                        xinput::XIEventMask::RAW_KEY_PRESS | xinput::XIEventMask::RAW_KEY_RELEASE,
                    ],
                }],
            )
            .map_err(platform)?
            .check()
            .map_err(platform)?;
        let (commands, receiver) = mpsc::channel::<(Vec<DesktopBinding>, Reply)>();
        let events = Arc::new(Mutex::new(Vec::new()));
        let output = events.clone();
        std::thread::Builder::new()
            .name("openless-x11-hotkeys".into())
            .spawn(move || {
                let mut installed: HashMap<(u8, u16), DesktopBinding> = HashMap::new();
                let press_ids = crate::hotkeys::HotkeyPressIds::default();
                let mut held: HashMap<u8, DesktopBinding> = HashMap::new();
                loop {
                    match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok((bindings, reply)) => {
                            let result = install(&connection, root, &installed, bindings);
                            let _ = reply.send(match result {
                                Ok(next) => {
                                    for binding in held.values() {
                                        if let Some(event) = crate::hotkeys::event_from_signal(
                                            &binding.action,
                                            binding.symbol,
                                            binding.states,
                                            false,
                                            std::time::Instant::now(),
                                            &press_ids,
                                        ) {
                                            output.lock().unwrap().push(event);
                                        }
                                    }
                                    held.clear();
                                    installed = next;
                                    Ok(())
                                }
                                Err(error) => Err(error),
                            });
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                    }
                    loop {
                        let event = match connection.poll_for_event() {
                            Ok(Some(event)) => event,
                            Ok(None) => break,
                            Err(_) => return,
                        };
                        let (key, state, pressed, raw) = match event {
                            Event::KeyPress(e) => (e.detail, u16::from(e.state), true, false),
                            Event::KeyRelease(e) => (e.detail, u16::from(e.state), false, false),
                            Event::XinputRawKeyPress(e) => (e.detail as u8, 0, true, true),
                            Event::XinputRawKeyRelease(e) => (e.detail as u8, 0, false, true),
                            _ => continue,
                        };
                        if raw && pressed {
                            for (held_key, binding) in &held {
                                if *held_key != key && passive_modifier(binding) {
                                    let action = match binding.action.as_str() {
                                        "DictationKeyEvent" => "DictationKeyCombined",
                                        "LessComputerKeyEvent" => "LessComputerKeyCombined",
                                        _ => continue,
                                    };
                                    if let Some(event) = crate::hotkeys::event_from_signal(
                                        action,
                                        binding.symbol,
                                        binding.states,
                                        true,
                                        std::time::Instant::now(),
                                        &press_ids,
                                    ) {
                                        output.lock().unwrap().push(event);
                                    }
                                }
                            }
                        }
                        let binding = if pressed {
                            installed.get(&(key, state & !18)).cloned()
                        } else {
                            held.get(&key)
                                .filter(|b| passive_modifier(b) == raw)
                                .cloned()
                        };
                        let binding = binding.filter(|b| passive_modifier(b) == raw);
                        if let Some(binding) = binding {
                            if !pressed {
                                held.remove(&key);
                            }
                            if raw && pressed && !held.contains_key(&key) {
                                if let Ok(cookie) = connection.query_pointer(root) {
                                    if let Ok(pointer) = cookie.reply() {
                                        if u16::from(pointer.mask)
                                            & !(18 | modifier_mask(binding.symbol))
                                            != 0
                                        {
                                            continue;
                                        }
                                    }
                                }
                            }
                            if pressed {
                                held.insert(key, binding.clone());
                            }
                            if let Some(event) = crate::hotkeys::event_from_signal(
                                &binding.action,
                                binding.symbol,
                                binding.states,
                                pressed,
                                std::time::Instant::now(),
                                &press_ids,
                            ) {
                                output.lock().unwrap().push(event);
                            }
                        }
                    }
                }
            })
            .map_err(platform)?;
        Ok(Self { commands, events })
    }
}

fn install(
    connection: &RustConnection,
    root: u32,
    previous: &HashMap<(u8, u16), DesktopBinding>,
    bindings: Vec<DesktopBinding>,
) -> Result<HashMap<(u8, u16), DesktopBinding>, BackendError> {
    let setup = connection.setup();
    let mapping = connection
        .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
        .map_err(platform)?
        .reply()
        .map_err(platform)?;
    let mut next = HashMap::new();
    let mut grabbed = Vec::new();
    for binding in bindings {
        let index = mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode as usize)
            .position(|symbols| symbols.contains(&binding.symbol))
            .ok_or_else(|| platform(format!("键盘布局不包含 {}", binding.accelerator)))?;
        let key = setup.min_keycode + index as u8;
        let states = binding.states as u16;
        next.insert((key, states), binding);
    }
    // Resolve every keysym before grabbing anything. A missing symbol must not
    // leave a partially installed binding set after a failed transaction.
    for (&(key, states), binding) in &next {
        if !passive_modifier(binding) && !previous.contains_key(&(key, states)) {
            for locks in [0, 2, 16, 18] {
                let result = connection
                    .grab_key(
                        true,
                        root,
                        ModMask::from(states | locks),
                        key,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                    )
                    .map_err(platform)
                    .and_then(|cookie| cookie.check().map_err(platform));
                if let Err(error) = result {
                    for (code, mask) in grabbed {
                        let _ = connection.ungrab_key(code, root, ModMask::from(mask));
                    }
                    let _ = connection.flush();
                    return Err(platform(format!(
                        "快捷键被占用：{} ({error})",
                        binding.accelerator
                    )));
                }
                grabbed.push((key, states | locks));
            }
        }
    }
    for (key, states) in previous.keys().filter(|key| !next.contains_key(key)) {
        if passive_modifier(&previous[&(*key, *states)]) {
            continue;
        }
        for locks in [0, 2, 16, 18] {
            connection
                .ungrab_key(*key, root, ModMask::from(states | locks))
                .map_err(platform)?
                .check()
                .map_err(platform)?;
        }
    }
    connection.flush().map_err(platform)?;
    Ok(next)
}

fn atom(connection: &RustConnection, name: &str) -> Result<u32, BackendError> {
    Ok(connection
        .intern_atom(false, name.as_bytes())
        .map_err(platform)?
        .reply()
        .map_err(platform)?
        .atom)
}
fn property(
    connection: &RustConnection,
    window: u32,
    name: &str,
) -> Result<xproto::GetPropertyReply, BackendError> {
    connection
        .get_property(
            false,
            window,
            atom(connection, name)?,
            AtomEnum::ANY,
            0,
            4096,
        )
        .map_err(platform)?
        .reply()
        .map_err(platform)
}

impl DesktopAdapter for X11Desktop {
    fn snapshot(&self) -> Result<DesktopSnapshot, BackendError> {
        let (connection, screen) = x11rb::connect(None).map_err(platform)?;
        let screen = &connection.setup().roots[screen];
        let window = property(&connection, screen.root, "_NET_ACTIVE_WINDOW")?
            .value32()
            .and_then(|mut v| v.next())
            .ok_or_else(|| platform("no foreground window"))?;
        let application =
            String::from_utf8_lossy(&property(&connection, window, "WM_CLASS")?.value)
                .replace('\0', " ")
                .trim()
                .to_string();
        let geometry = connection
            .get_geometry(window)
            .map_err(platform)?
            .reply()
            .map_err(platform)?;
        let coords = connection
            .translate_coordinates(window, screen.root, 0, 0)
            .map_err(platform)?
            .reply()
            .map_err(platform)?;
        // _NET_WORKAREA is already in X11 pixels; RandR logical scaling is 1.
        let work = property(&connection, screen.root, "_NET_WORKAREA")
            .ok()
            .and_then(|p| p.value32().map(|v| v.collect::<Vec<_>>()));
        let (mut x, mut y, mut width, mut height) = work
            .filter(|v| v.len() >= 4)
            .map(|v| (v[0] as i32, v[1] as i32, v[2], v[3]))
            .unwrap_or((
                coords.dst_x as i32,
                coords.dst_y as i32,
                geometry.width as u32,
                geometry.height as u32,
            ));
        // Use the monitor containing the target window, constrained by panels
        // and docks in the EWMH work area. This avoids centering between screens.
        if let Ok(cookie) = connection.randr_get_monitors(screen.root, true) {
            if let Ok(monitors) = cookie.reply() {
                let cx = i32::from(coords.dst_x) + i32::from(geometry.width) / 2;
                let cy = i32::from(coords.dst_y) + i32::from(geometry.height) / 2;
                if let Some(m) = monitors.monitors.iter().find(|m| {
                    cx >= i32::from(m.x)
                        && cy >= i32::from(m.y)
                        && cx < i32::from(m.x) + i32::from(m.width)
                        && cy < i32::from(m.y) + i32::from(m.height)
                }) {
                    let right = (x + width as i32).min(i32::from(m.x) + i32::from(m.width));
                    let bottom = (y + height as i32).min(i32::from(m.y) + i32::from(m.height));
                    x = x.max(i32::from(m.x));
                    y = y.max(i32::from(m.y));
                    width = (right - x).max(1) as u32;
                    height = (bottom - y).max(1) as u32;
                }
            }
        }
        Ok(DesktopSnapshot {
            version: 1,
            target: format!("x11:{window}"),
            application,
            x,
            y,
            width,
            height,
            scale: 1.0,
        })
    }
    fn bind(&self, bindings: &[DesktopBinding]) -> Result<(), BackendError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.commands
            .send((bindings.to_vec(), tx))
            .map_err(platform)?;
        rx.recv_timeout(Duration::from_secs(4)).map_err(platform)?
    }
    fn restore_focus(&self, target: &str) -> Result<(), BackendError> {
        let window = target
            .strip_prefix("x11:")
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| platform("invalid X11 target"))?;
        let (connection, screen) = x11rb::connect(None).map_err(platform)?;
        connection
            .get_window_attributes(window)
            .map_err(platform)?
            .reply()
            .map_err(platform)?;
        let event = xproto::ClientMessageEvent::new(
            32,
            window,
            atom(&connection, "_NET_ACTIVE_WINDOW")?,
            [2, 0, 0, 0, 0],
        );
        connection
            .send_event(
                false,
                connection.setup().roots[screen].root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(platform)?
            .check()
            .map_err(platform)?;
        connection.flush().map_err(platform)
    }
    fn place(&self, title: &str, x: i32, y: i32) -> Result<(), BackendError> {
        let (connection, screen) = x11rb::connect(None).map_err(platform)?;
        let root = connection.setup().roots[screen].root;
        let list = property(&connection, root, "_NET_CLIENT_LIST")?;
        for window in list.value32().into_iter().flatten() {
            let name = property(&connection, window, "_NET_WM_NAME")?;
            if name.value == title.as_bytes() {
                connection
                    .configure_window(window, &xproto::ConfigureWindowAux::new().x(x).y(y))
                    .map_err(platform)?
                    .check()
                    .map_err(platform)?;
                return connection.flush().map_err(platform);
            }
        }
        Err(platform("popup window not found"))
    }
    fn drain(&self) -> Vec<crate::LinuxHotkeyEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}
