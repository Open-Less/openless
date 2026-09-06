//! Freedesktop StatusNotifierItem tray integration without GTK or Tauri.
//!
//! The D-Bus worker owns no Core state. It emits typed commands that the egui
//! thread drains, and accepts menu snapshots for microphone checkmarks. A tray
//! is considered available only after the session bus name is owned and the
//! desktop watcher has acknowledged registration.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{tr_l10n, Lang};

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const DBUS_PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const DBUS_INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";
const PROCESS_INTERVAL: Duration = Duration::from_millis(100);
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(3);

const SHOW_ID: i32 = 1;
const PREVIOUS_STYLE_ID: i32 = 2;
const MICROPHONES_ID: i32 = 3;
const SEPARATOR_ID: i32 = 4;
const QUIT_ID: i32 = 5;
const FIRST_MICROPHONE_ID: i32 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    ShowMain,
    ActivatePreviousStyle,
    SelectMicrophone(String),
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMicrophone {
    pub name: String,
    pub is_default: bool,
    pub selected: bool,
}

#[derive(Debug)]
pub enum TrayError {
    Dbus(String),
    Worker(String),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dbus(message) => write!(f, "tray D-Bus initialization failed: {message}"),
            Self::Worker(message) => write!(f, "tray worker failed: {message}"),
        }
    }
}

impl std::error::Error for TrayError {}

enum TrayControl {
    SetMicrophones(Vec<TrayMicrophone>),
    SetLang(Lang),
    Shutdown,
}

#[derive(Default)]
struct TrayMenuState {
    revision: u32,
    microphones: Vec<TrayMicrophone>,
    lang: Option<Lang>,
}

impl TrayMenuState {
    fn command_for_id(&self, id: i32) -> Option<TrayCommand> {
        match id {
            SHOW_ID => Some(TrayCommand::ShowMain),
            PREVIOUS_STYLE_ID => Some(TrayCommand::ActivatePreviousStyle),
            QUIT_ID => Some(TrayCommand::Quit),
            FIRST_MICROPHONE_ID => Some(TrayCommand::SelectMicrophone(String::new())),
            id if id > FIRST_MICROPHONE_ID => self
                .microphones
                .get((id - FIRST_MICROPHONE_ID - 1) as usize)
                .map(|device| TrayCommand::SelectMicrophone(device.name.clone())),
            _ => None,
        }
    }
}

pub struct LinuxTray {
    commands: mpsc::Receiver<TrayCommand>,
    control: mpsc::Sender<TrayControl>,
    last_error: Arc<Mutex<Option<String>>>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxTray {
    /// Start and register a StatusNotifierItem. `Ok` means the desktop watcher
    /// accepted it; callers may then truthfully expose `supports_tray=true`.
    pub fn start() -> Result<Self, TrayError> {
        #[cfg(target_os = "linux")]
        {
            let (command_tx, command_rx) = mpsc::channel();
            let (control_tx, control_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let last_error = Arc::new(Mutex::new(None));
            let shutdown = Arc::new(AtomicBool::new(false));
            let worker_error = Arc::clone(&last_error);
            let worker_shutdown = Arc::clone(&shutdown);
            let worker = std::thread::Builder::new()
                .name("openless-tray".into())
                .spawn(move || {
                    let result = run_dbus_worker(command_tx, control_rx, worker_shutdown, ready_tx);
                    if let Err(error) = result {
                        *worker_error.lock().expect("tray error lock poisoned") =
                            Some(error.to_string());
                    }
                })
                .map_err(|error| TrayError::Worker(error.to_string()))?;

            match ready_rx.recv_timeout(REGISTRATION_TIMEOUT) {
                Ok(Ok(())) => Ok(Self {
                    commands: command_rx,
                    control: control_tx,
                    last_error,
                    shutdown,
                    worker: Some(worker),
                }),
                Ok(Err(error)) => {
                    shutdown.store(true, Ordering::Release);
                    let _ = worker.join();
                    Err(error)
                }
                Err(error) => {
                    shutdown.store(true, Ordering::Release);
                    let _ = worker.join();
                    Err(TrayError::Worker(format!(
                        "timed out waiting for tray registration: {error}"
                    )))
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(TrayError::Worker(
                "StatusNotifierItem is available only on Linux".into(),
            ))
        }
    }

    pub fn drain(&self, mut apply: impl FnMut(TrayCommand)) -> usize {
        let mut count = 0;
        while let Ok(command) = self.commands.try_recv() {
            count += 1;
            apply(command);
        }
        count
    }

    pub fn set_microphones(&self, microphones: Vec<TrayMicrophone>) -> Result<(), TrayError> {
        self.control
            .send(TrayControl::SetMicrophones(microphones))
            .map_err(|_| TrayError::Worker("tray worker has stopped".into()))
    }

    /// Set the UI language used for the tray menu labels. Callers should keep
    /// this in sync with the persisted Linux-UI locale preference whenever it
    /// changes so a system tray re-layout reads the right language.
    pub fn set_lang(&self, lang: Lang) -> Result<(), TrayError> {
        self.control
            .send(TrayControl::SetLang(lang))
            .map_err(|_| TrayError::Worker("tray worker has stopped".into()))
    }

    pub fn take_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .expect("tray error lock poisoned")
            .take()
    }
}

impl Drop for LinuxTray {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.control.send(TrayControl::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn run_dbus_worker(
    command_tx: mpsc::Sender<TrayCommand>,
    control_rx: mpsc::Receiver<TrayControl>,
    shutdown: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), TrayError>>,
) -> Result<(), TrayError> {
    use dbus::blocking::stdintf::org_freedesktop_dbus::RequestNameReply;
    use dbus::blocking::Connection;
    use dbus::channel::{MatchingReceiver, Sender};
    use dbus::message::MatchRule;

    let connection = Connection::new_session().map_err(dbus_error)?;
    let service_name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let ownership = connection
        .request_name(&service_name, false, true, true)
        .map_err(dbus_error)?;
    if ownership != RequestNameReply::PrimaryOwner {
        let error = TrayError::Dbus(format!("D-Bus name {service_name} is already owned"));
        let _ = ready.send(Err(TrayError::Dbus(error.to_string())));
        return Err(error);
    }

    let menu = Arc::new(Mutex::new(TrayMenuState::default()));
    let callback_menu = Arc::clone(&menu);
    let callback_commands = command_tx;
    connection.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |message, connection| {
            if let Some(reply) = handle_method_call(&message, &callback_menu, &callback_commands) {
                let _ = connection.send(reply);
            }
            true
        }),
    );

    let watcher = connection.with_proxy(WATCHER_NAME, WATCHER_PATH, REGISTRATION_TIMEOUT);
    let registration: Result<(), dbus::Error> = watcher.method_call(
        WATCHER_INTERFACE,
        "RegisterStatusNotifierItem",
        (service_name.as_str(),),
    );
    if let Err(error) = registration {
        let error = TrayError::Dbus(format!(
            "StatusNotifierWatcher rejected registration: {error}"
        ));
        let _ = ready.send(Err(TrayError::Dbus(error.to_string())));
        return Err(error);
    }
    let _ = ready.send(Ok(()));

    while !shutdown.load(Ordering::Acquire) {
        while let Ok(control) = control_rx.try_recv() {
            match control {
                TrayControl::SetMicrophones(microphones) => {
                    let revision = {
                        let mut state = menu.lock().expect("tray menu lock poisoned");
                        state.microphones = microphones;
                        state.revision = state.revision.wrapping_add(1).max(1);
                        state.revision
                    };
                    let signal =
                        dbus::Message::new_signal(MENU_PATH, MENU_INTERFACE, "LayoutUpdated")
                            .map_err(TrayError::Dbus)?
                            .append2(revision, 0i32);
                    connection.send(signal).map_err(|_| {
                        TrayError::Dbus("failed to publish tray menu update".into())
                    })?;
                }
                TrayControl::SetLang(lang) => {
                    let revision = {
                        let mut state = menu.lock().expect("tray menu lock poisoned");
                        state.lang = Some(lang);
                        state.revision = state.revision.wrapping_add(1).max(1);
                        state.revision
                    };
                    let signal =
                        dbus::Message::new_signal(MENU_PATH, MENU_INTERFACE, "LayoutUpdated")
                            .map_err(TrayError::Dbus)?
                            .append2(revision, 0i32);
                    connection.send(signal).map_err(|_| {
                        TrayError::Dbus("failed to publish tray menu update".into())
                    })?;
                }
                TrayControl::Shutdown => return Ok(()),
            }
        }
        connection.process(PROCESS_INTERVAL).map_err(dbus_error)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn dbus_error(error: dbus::Error) -> TrayError {
    TrayError::Dbus(error.to_string())
}

#[cfg(target_os = "linux")]
type Properties = HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>;

#[cfg(target_os = "linux")]
type Children = Vec<dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>;

#[cfg(target_os = "linux")]
fn property<T: dbus::arg::RefArg + 'static>(
    value: T,
) -> dbus::arg::Variant<Box<dyn dbus::arg::RefArg>> {
    dbus::arg::Variant(Box::new(value))
}

#[cfg(target_os = "linux")]
fn menu_properties(label: &str) -> Properties {
    HashMap::from([
        ("label".into(), property(label.to_string())),
        ("enabled".into(), property(true)),
        ("visible".into(), property(true)),
    ])
}

#[cfg(target_os = "linux")]
fn menu_item(
    id: i32,
    properties: Properties,
    children: Children,
) -> dbus::arg::Variant<Box<dyn dbus::arg::RefArg>> {
    property((id, properties, children))
}

#[cfg(target_os = "linux")]
fn menu_layout(state: &TrayMenuState) -> (i32, Properties, Children) {
    let lang = state.lang.unwrap_or(Lang::ZhCn);
    let mut microphone_children = Vec::new();
    let default_selected = state.microphones.iter().all(|device| !device.selected);
    let mut default_props = menu_properties(tr_l10n(lang, "settings.system_default"));
    default_props.insert("toggle-type".into(), property("checkmark".to_string()));
    default_props.insert("toggle-state".into(), property(i32::from(default_selected)));
    microphone_children.push(menu_item(FIRST_MICROPHONE_ID, default_props, Vec::new()));
    for (index, device) in state.microphones.iter().enumerate() {
        let mut props = menu_properties(&device.name);
        props.insert("toggle-type".into(), property("checkmark".to_string()));
        props.insert("toggle-state".into(), property(i32::from(device.selected)));
        if device.is_default {
            props.insert("x-openless-default".into(), property(true));
        }
        microphone_children.push(menu_item(
            FIRST_MICROPHONE_ID + index as i32 + 1,
            props,
            Vec::new(),
        ));
    }
    let mut microphone_props = menu_properties(tr_l10n(lang, "settings.microphone"));
    microphone_props.insert("children-display".into(), property("submenu".to_string()));
    let separator = HashMap::from([("type".into(), property("separator".to_string()))]);
    (
        0,
        HashMap::new(),
        vec![
            menu_item(
                SHOW_ID,
                menu_properties(tr_l10n(lang, "tray.show")),
                Vec::new(),
            ),
            menu_item(
                PREVIOUS_STYLE_ID,
                menu_properties(tr_l10n(lang, "tray.previous_style")),
                Vec::new(),
            ),
            menu_item(MICROPHONES_ID, microphone_props, microphone_children),
            menu_item(SEPARATOR_ID, separator, Vec::new()),
            menu_item(
                QUIT_ID,
                menu_properties(tr_l10n(lang, "tray.quit")),
                Vec::new(),
            ),
        ],
    )
}

#[cfg(target_os = "linux")]
fn handle_method_call(
    message: &dbus::Message,
    menu: &Arc<Mutex<TrayMenuState>>,
    commands: &mpsc::Sender<TrayCommand>,
) -> Option<dbus::Message> {
    use dbus::arg::Variant;

    let path = message.path()?.to_string();
    let interface = message.interface()?.to_string();
    let member = message.member()?.to_string();

    if interface == DBUS_INTROSPECTABLE && member == "Introspect" {
        return Some(message.method_return().append1(INTROSPECTION_XML));
    }
    if path == ITEM_PATH && interface == ITEM_INTERFACE {
        if member == "Activate" || member == "SecondaryActivate" {
            let _ = commands.send(TrayCommand::ShowMain);
        }
        return Some(message.method_return());
    }
    if path == MENU_PATH && interface == MENU_INTERFACE {
        match member.as_str() {
            "GetLayout" => {
                let state = menu.lock().expect("tray menu lock poisoned");
                return Some(
                    message
                        .method_return()
                        .append2(state.revision, menu_layout(&state)),
                );
            }
            "GetGroupProperties" => {
                let entries: Vec<(i32, Properties)> = Vec::new();
                return Some(message.method_return().append1(entries));
            }
            "Event" => {
                if let Ok((id, event, _data, _timestamp)) =
                    message.read4::<i32, String, Variant<Box<dyn dbus::arg::RefArg>>, u32>()
                {
                    if event == "clicked" {
                        if let Some(command) = menu
                            .lock()
                            .expect("tray menu lock poisoned")
                            .command_for_id(id)
                        {
                            let _ = commands.send(command);
                        }
                    }
                }
                return Some(message.method_return());
            }
            "AboutToShow" => return Some(message.method_return().append1(false)),
            _ => return Some(message.method_return()),
        }
    }
    if interface == DBUS_PROPERTIES && member == "Get" {
        if let Ok((requested_interface, name)) = message.read2::<String, String>() {
            let value = item_property(&requested_interface, &name)
                .or_else(|| menu_property(&requested_interface, &name));
            if let Some(value) = value {
                return Some(message.method_return().append1(value));
            }
        }
    }
    if interface == DBUS_PROPERTIES && member == "GetAll" {
        let requested_interface = message.read1::<String>().unwrap_or_default();
        let properties = if requested_interface == ITEM_INTERFACE {
            item_properties()
        } else if requested_interface == MENU_INTERFACE {
            menu_properties_all()
        } else {
            HashMap::new()
        };
        return Some(message.method_return().append1(properties));
    }
    dbus::channel::default_reply(message)
}

#[cfg(target_os = "linux")]
fn item_property(
    interface: &str,
    name: &str,
) -> Option<dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>> {
    if interface != ITEM_INTERFACE {
        return None;
    }
    match name {
        "Category" => Some(property("ApplicationStatus".to_string())),
        "Id" => Some(property("openless".to_string())),
        "Title" => Some(property("OpenLess".to_string())),
        "Status" => Some(property("Active".to_string())),
        "IconName" => Some(property("openless".to_string())),
        "Menu" => Some(property(
            dbus::Path::new(MENU_PATH).expect("static D-Bus path"),
        )),
        "ItemIsMenu" => Some(property(false)),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn item_properties() -> Properties {
    [
        "Category",
        "Id",
        "Title",
        "Status",
        "IconName",
        "Menu",
        "ItemIsMenu",
    ]
    .into_iter()
    .filter_map(|name| item_property(ITEM_INTERFACE, name).map(|value| (name.into(), value)))
    .collect()
}

#[cfg(target_os = "linux")]
fn menu_property(
    interface: &str,
    name: &str,
) -> Option<dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>> {
    if interface != MENU_INTERFACE {
        return None;
    }
    match name {
        "Version" => Some(property(3u32)),
        "TextDirection" => Some(property("ltr".to_string())),
        "Status" => Some(property("normal".to_string())),
        "IconThemePath" => Some(property(Vec::<String>::new())),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn menu_properties_all() -> Properties {
    ["Version", "TextDirection", "Status", "IconThemePath"]
        .into_iter()
        .filter_map(|name| menu_property(MENU_INTERFACE, name).map(|value| (name.into(), value)))
        .collect()
}

#[cfg(target_os = "linux")]
const INTROSPECTION_XML: &str = r#"<node>
 <interface name="org.kde.StatusNotifierItem">
  <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
  <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
 </interface>
 <interface name="com.canonical.dbusmenu">
  <method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/><arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
  <method name="GetGroupProperties"><arg type="ai" direction="in"/><arg type="as" direction="in"/><arg type="a(ia{sv})" direction="out"/></method>
  <method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
  <method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
 </interface>
</node>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_only_to_supported_commands() {
        let state = TrayMenuState {
            revision: 1,
            microphones: vec![TrayMicrophone {
                name: "Studio Mic".into(),
                is_default: true,
                selected: true,
            }],
            lang: None,
        };
        assert_eq!(state.command_for_id(SHOW_ID), Some(TrayCommand::ShowMain));
        assert_eq!(
            state.command_for_id(PREVIOUS_STYLE_ID),
            Some(TrayCommand::ActivatePreviousStyle)
        );
        assert_eq!(state.command_for_id(QUIT_ID), Some(TrayCommand::Quit));
        assert_eq!(
            state.command_for_id(FIRST_MICROPHONE_ID + 1),
            Some(TrayCommand::SelectMicrophone("Studio Mic".into()))
        );
        assert_eq!(state.command_for_id(MICROPHONES_ID), None);
        assert_eq!(
            state.command_for_id(FIRST_MICROPHONE_ID),
            Some(TrayCommand::SelectMicrophone(String::new()))
        );
        assert_eq!(state.command_for_id(999), None);
    }

    #[test]
    fn tray_capability_is_not_inferred_from_the_desktop_environment() {
        let snapshot = crate::LinuxCapabilitySnapshot::from_environment(
            None,
            Some(":0"),
            true,
            false,
            crate::LinuxPackageKind::Development,
        );
        assert!(!snapshot.capabilities.supports_tray);
    }
}
