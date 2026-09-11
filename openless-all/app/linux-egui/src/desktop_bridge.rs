//! Desktop bridge v1: screen coordinates are logical pixels, targets are
//! opaque, and hotkeys are installed as a transaction before preferences commit.
use crate::context::platform;
use openless_core::{BackendError, HotkeyRuntimeTarget};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};

pub const DESKTOP_PROTOCOL_VERSION: u32 = 1;
pub const BUS_NAME: &str = "org.openless.Desktop1";
pub const BUS_PATH: &str = "/org/openless/Desktop1";

pub fn manage_component(mode: &str) -> Result<String, BackendError> {
    if !["install", "enable", "uninstall"].contains(&mode) {
        return Err(platform("invalid desktop component action"));
    }
    let layout = crate::LinuxResourceLayout::detect(
        std::env::var_os("OPENLESS_LINUX_RESOURCES").map(std::path::PathBuf::from),
    )?;
    let packaged = layout.resource_root.join("linux-desktop/install.sh");
    let development = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/linux-desktop/install.sh");
    let script = if packaged.is_file() {
        packaged
    } else {
        development
    };
    if !script.is_file() {
        return Err(platform("桌面组件缺失，请重新安装 Linux 安装包"));
    }
    let output = std::process::Command::new("timeout")
        .args(["30s", "bash"])
        .arg(script)
        .arg(mode)
        .output()
        .map_err(platform)?;
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(message.trim().into())
    } else {
        Err(platform(message.trim()))
    }
}

static TARGETS: OnceLock<Mutex<std::collections::HashMap<String, DesktopSnapshot>>> =
    OnceLock::new();
static LAST_SCREEN: OnceLock<Mutex<Option<DesktopSnapshot>>> = OnceLock::new();
fn targets() -> &'static Mutex<std::collections::HashMap<String, DesktopSnapshot>> {
    TARGETS.get_or_init(Mutex::default)
}
pub(crate) fn remember_focus(ticket: &str) {
    if let Some(snapshot) = adapter().and_then(|a| a.snapshot().ok()) {
        *LAST_SCREEN.get_or_init(Mutex::default).lock().unwrap() = Some(snapshot.clone());
        let mut targets = targets().lock().unwrap();
        if targets.len() >= 128 {
            targets.clear();
        }
        targets.insert(ticket.into(), snapshot);
    }
}
pub(crate) fn rekey_focus(from: &str, to: &str) {
    let mut targets = targets().lock().unwrap();
    if let Some(snapshot) = targets.remove(from) {
        targets.insert(to.into(), snapshot);
    }
}
pub(crate) fn forget_focus(ticket: &str) {
    targets().lock().unwrap().remove(ticket);
}
pub(crate) fn restore_bound_focus(ticket: &str) -> Result<(), BackendError> {
    let target = targets().lock().unwrap().get(ticket).cloned();
    if let Some(target) = target {
        let adapter = adapter().ok_or_else(|| platform("桌面组件暂时不可用"))?;
        adapter.restore_focus(&target.target)?;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(700) {
            if adapter.snapshot().is_ok_and(|s| s.target == target.target) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        return Err(platform("无法恢复原目标窗口"));
    }
    Ok(()) // fcitx5 still verifies the original focused input context.
}
pub fn place_popup(title: &'static str, width: i32, height: i32, capsule: bool) {
    let screen = LAST_SCREEN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .clone();
    let Some(screen) = screen else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("openless-popup-placement".into())
        .spawn(move || {
            let x = screen.x + ((screen.width as i32 - width) / 2).max(0);
            let y = screen.y
                + if capsule {
                    (screen.height as i32 - height - 28).max(0)
                } else {
                    ((screen.height as i32 - height) / 2).max(0)
                };
            for _ in 0..12 {
                std::thread::sleep(std::time::Duration::from_millis(80));
                if adapter().is_some_and(|a| a.place(title, x, y).is_ok()) {
                    break;
                }
            }
        });
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    pub version: u32,
    pub target: String,
    pub application: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default = "one")]
    pub scale: f64,
}
fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBinding {
    pub action: String,
    pub symbol: u32,
    pub states: u32,
    pub accelerator: String,
}

pub trait DesktopAdapter: Send + Sync {
    fn snapshot(&self) -> Result<DesktopSnapshot, BackendError>;
    fn bind(&self, bindings: &[DesktopBinding]) -> Result<(), BackendError>;
    fn restore_focus(&self, target: &str) -> Result<(), BackendError>;
    fn place(&self, title: &str, x: i32, y: i32) -> Result<(), BackendError>;
    fn drain(&self) -> Vec<crate::LinuxHotkeyEvent>;
}

#[derive(Default)]
struct BridgeState {
    adapter: Option<Arc<dyn DesktopAdapter>>,
    bindings: Option<Vec<DesktopBinding>>,
    owner: Option<String>,
}
static ADAPTER: OnceLock<Arc<Mutex<BridgeState>>> = OnceLock::new();
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DISCONNECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn take_disconnected() -> bool {
    DISCONNECTED.swap(false, std::sync::atomic::Ordering::AcqRel)
}
pub fn active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Acquire)
}
pub fn bind_target(target: &HotkeyRuntimeTarget) -> Result<(), BackendError> {
    let bindings = bindings(target)?;
    let mut state = registry().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(adapter) = &state.adapter {
        adapter.bind(&bindings)?;
        ACTIVE.store(true, std::sync::atomic::Ordering::Release);
    }
    state.bindings = Some(bindings);
    Ok(())
}
fn registry() -> &'static Arc<Mutex<BridgeState>> {
    ADAPTER.get_or_init(|| {
        let state = Arc::new(Mutex::new(BridgeState::default()));
        refresh(&state);
        let shared = state.clone();
        std::thread::Builder::new()
            .name("openless-desktop-recovery".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                refresh(&shared);
            })
            .expect("create desktop recovery worker");
        state
    })
}
pub fn adapter() -> Option<Arc<dyn DesktopAdapter>> {
    registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .adapter
        .clone()
}
fn refresh(shared: &Mutex<BridgeState>) {
    #[cfg(target_os = "linux")]
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    #[cfg(target_os = "linux")]
    let owner = if wayland {
        desktop_owner()
    } else {
        Some("x11".into())
    };
    let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
    #[cfg(target_os = "linux")]
    {
        if state.owner != owner {
            if ACTIVE.swap(false, std::sync::atomic::Ordering::AcqRel) {
                DISCONNECTED.store(true, std::sync::atomic::Ordering::Release);
            }
            state.adapter = None;
            state.owner = owner.clone();
        }
        if state.adapter.is_some() || owner.is_none() {
            return;
        }
        let detected = {
            if wayland {
                WaylandDesktop::start()
                    .map(|a| Arc::new(a) as Arc<dyn DesktopAdapter>)
                    .ok()
            } else {
                crate::x11_desktop::X11Desktop::start()
                    .map(|a| Arc::new(a) as Arc<dyn DesktopAdapter>)
                    .ok()
            }
        };
        if let Some(adapter) = detected {
            if let Some(bindings) = &state.bindings {
                if adapter.bind(bindings).is_err() {
                    return;
                }
                ACTIVE.store(true, std::sync::atomic::Ordering::Release);
            }
            state.adapter = Some(adapter);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = &mut state;
}
#[cfg(target_os = "linux")]
fn desktop_owner() -> Option<String> {
    let connection = dbus::blocking::Connection::new_session().ok()?;
    let result: Result<(String,), _> = connection
        .with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            std::time::Duration::from_millis(500),
        )
        .method_call("org.freedesktop.DBus", "GetNameOwner", (BUS_NAME,));
    result.ok().map(|value| value.0)
}

fn accelerator(symbol: u32, states: u32) -> String {
    let mut result = String::new();
    for (bit, name) in [
        (4, "<Control>"),
        (1, "<Shift>"),
        (8, "<Alt>"),
        (64, "<Super>"),
    ] {
        if states & bit != 0 {
            result.push_str(name);
        }
    }
    // GDK accepts X11 keysym hexadecimal notation as well as named keys.
    let name = match symbol {
        0xffe1 => "Shift_L",
        0xffe2 => "Shift_R",
        0xffe3 => "Control_L",
        0xffe4 => "Control_R",
        0xffe9 => "Alt_L",
        0xffea => "Alt_R",
        0xffeb => "Super_L",
        0xffec => "Super_R",
        0xff0d => "Return",
        0xff09 => "Tab",
        0xff1b => "Escape",
        0x20 => "space",
        0xff08 => "BackSpace",
        0xffff => "Delete",
        0xff50 => "Home",
        0xff57 => "End",
        0xff55 => "Page_Up",
        0xff56 => "Page_Down",
        0xff52 => "Up",
        0xff54 => "Down",
        0xff51 => "Left",
        0xff53 => "Right",
        _ => "",
    };
    if !name.is_empty() {
        result.push_str(name);
    } else if (0xffbe..=0xffd5).contains(&symbol) {
        result.push_str(&format!("F{}", symbol - 0xffbe + 1));
    } else if (0x21..=0x7e).contains(&symbol) {
        result.push(char::from_u32(symbol).unwrap());
    } else {
        result.push_str(&format!("0x{symbol:x}"));
    }
    result
}

pub fn bindings(target: &HotkeyRuntimeTarget) -> Result<Vec<DesktopBinding>, BackendError> {
    let mut result = Vec::new();
    let items = [
        ("DictationKeyEvent", Some(&target.dictation)),
        ("QaShortcutEvent", target.qa.as_ref()),
        ("SelectionPolishEvent", target.selection_polish.as_ref()),
        ("TranslationModifierEvent", Some(&target.translation)),
        ("OpenAppEvent", target.open_app.as_ref()),
        (
            "LessComputerPanelEvent",
            target
                .coding_agent_panel
                .as_ref()
                .filter(|_| target.coding_agent_enabled),
        ),
        (
            "LessComputerQuickEvent",
            target
                .coding_agent_quick
                .as_ref()
                .filter(|_| target.coding_agent_enabled),
        ),
        ("SwitchStyleEvent", target.switch_style.as_ref()),
        (
            "LessComputerKeyEvent",
            target
                .coding_agent_voice
                .as_ref()
                .filter(|_| target.coding_agent_enabled),
        ),
    ];
    for (action, binding) in items {
        if let Some(binding) = binding {
            let (symbol, states) = crate::settings::shortcut_to_raw(binding)?;
            result.push(DesktopBinding {
                action: action.into(),
                symbol,
                states,
                accelerator: accelerator(symbol, states),
            });
        }
    }
    for hotkey in &target.style_packs {
        let (symbol, states) = crate::settings::shortcut_to_raw(&hotkey.binding)?;
        result.push(DesktopBinding {
            action: "StylePackHotkeyEvent".into(),
            symbol,
            states,
            accelerator: accelerator(symbol, states),
        });
    }
    for (index, binding) in result.iter().enumerate() {
        if result[..index]
            .iter()
            .any(|b| b.symbol == binding.symbol && b.states == binding.states)
        {
            return Err(platform(format!("快捷键冲突：{}", binding.accelerator)));
        }
    }
    Ok(result)
}

#[cfg(target_os = "linux")]
struct WaylandDesktop {
    events: Arc<Mutex<Vec<crate::LinuxHotkeyEvent>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(target_os = "linux")]
impl WaylandDesktop {
    fn call<T: dbus::arg::ReadAll>(
        &self,
        method: &str,
        args: impl dbus::arg::AppendAll,
    ) -> Result<T, BackendError> {
        let connection = dbus::blocking::Connection::new_session().map_err(platform)?;
        connection
            .with_proxy(BUS_NAME, BUS_PATH, std::time::Duration::from_secs(2))
            .method_call(BUS_NAME, method, args)
            .map_err(platform)
    }
    fn start() -> Result<Self, BackendError> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let this = Self {
            events: events.clone(),
            stop: stop.clone(),
        };
        let (version,): (u32,) = this.call("Version", ())?;
        if version != DESKTOP_PROTOCOL_VERSION {
            return Err(platform("desktop bridge version mismatch"));
        }
        std::thread::Builder::new()
            .name("openless-desktop-events".into())
            .spawn(move || {
                let Ok(connection) = dbus::blocking::Connection::new_session() else {
                    return;
                };
                let mut rule = dbus::message::MatchRule::new_signal(BUS_NAME, "Hotkey");
                rule.sender = Some(BUS_NAME.into());
                rule.path = Some(BUS_PATH.into());
                let press_ids = crate::hotkeys::HotkeyPressIds::default();
                let _subscription = connection.add_match(
                    rule,
                    move |(action, symbol, states, pressed): (String, u32, u32, bool), _, _| {
                        if let Some(event) = crate::hotkeys::event_from_signal(
                            &action,
                            symbol,
                            states,
                            pressed,
                            std::time::Instant::now(),
                            &press_ids,
                        ) {
                            events.lock().unwrap().push(event);
                        }
                        true
                    },
                );
                while !stop.load(std::sync::atomic::Ordering::Acquire)
                    && connection
                        .process(std::time::Duration::from_secs(1))
                        .is_ok()
                {}
            })
            .map_err(platform)?;
        Ok(this)
    }
}
#[cfg(target_os = "linux")]
impl Drop for WaylandDesktop {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
    }
}
#[cfg(target_os = "linux")]
impl DesktopAdapter for WaylandDesktop {
    fn snapshot(&self) -> Result<DesktopSnapshot, BackendError> {
        let (json,): (String,) = self.call("Snapshot", ())?;
        let snapshot: DesktopSnapshot = serde_json::from_str(&json).map_err(platform)?;
        if snapshot.version != 1 {
            return Err(platform("desktop version mismatch"));
        }
        Ok(snapshot)
    }
    fn bind(&self, bindings: &[DesktopBinding]) -> Result<(), BackendError> {
        let (error,): (String,) = self.call(
            "Bind",
            (serde_json::to_string(bindings).map_err(platform)?,),
        )?;
        if error.is_empty() {
            Ok(())
        } else {
            Err(platform(error))
        }
    }
    fn restore_focus(&self, target: &str) -> Result<(), BackendError> {
        let (ok,): (bool,) = self.call("Restore", (target,))?;
        if ok {
            Ok(())
        } else {
            Err(platform("original window no longer exists"))
        }
    }
    fn place(&self, title: &str, x: i32, y: i32) -> Result<(), BackendError> {
        let (ok,): (bool,) = self.call("Place", (title, x, y))?;
        if ok {
            Ok(())
        } else {
            Err(platform("popup not found"))
        }
    }
    fn drain(&self) -> Vec<crate::LinuxHotkeyEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_shortcuts_fail_before_installing_anything() {
        let mut preferences = openless_core::UserPreferences::default();
        preferences.translation_hotkey = preferences.dictation_hotkey.clone();
        assert!(bindings(&HotkeyRuntimeTarget::from(&preferences)).is_err());
    }
}
