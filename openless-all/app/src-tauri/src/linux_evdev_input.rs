//! Linux evdev input for side-specific combos and mouse dictation triggers.

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use evdev::{Device, InputEvent, InputEventKind, Key};

    use crate::hold_source_tracker::TriggerSource;
    use crate::hotkey::HotkeyEvent;
    use crate::mouse_dictation::MouseDictationConfig;
    use crate::shortcut_binding::binding_requires_side_aware_hook;
    use crate::side_aware_combo::{handle_primary_key, handle_side_modifier, SideModifier};
    use crate::types::ShortcutBinding;

    static USER_STATUS: OnceLock<Mutex<Option<String>>> = OnceLock::new();

    fn user_status() -> &'static Mutex<Option<String>> {
        USER_STATUS.get_or_init(|| Mutex::new(None))
    }

    /// User-visible evdev status for settings / capability hints.
    pub fn status_message() -> Option<String> {
        user_status().lock().ok()?.clone()
    }

    pub fn set_status_message(message: Option<String>) {
        if let Ok(mut slot) = user_status().lock() {
            *slot = message;
        }
    }

    const UDEV_HINT: &str = "鼠标/侧别组合键需读取 /dev/input/event*。若无权限，请将用户加入 input 组：`sudo usermod -aG input $USER`，然后重新登录。";

    pub struct LinuxEvdevConfig {
        pub side_combo: Option<ShortcutBinding>,
        pub mouse: MouseDictationConfig,
    }

    pub fn config_from_prefs(prefs: &crate::types::UserPreferences) -> LinuxEvdevConfig {
        LinuxEvdevConfig {
            side_combo: if binding_requires_side_aware_hook(&prefs.dictation_hotkey) {
                Some(prefs.dictation_hotkey.clone())
            } else {
                None
            },
            mouse: MouseDictationConfig {
                middle_enabled: prefs.mouse_middle_button_dictation,
                side_enabled: prefs.mouse_side_button_dictation,
            },
        }
    }

    pub fn monitor_needed(config: &LinuxEvdevConfig) -> bool {
        config.mouse.middle_enabled
            || config.mouse.side_enabled
            || config
                .side_combo
                .as_ref()
                .is_some_and(binding_requires_side_aware_hook)
    }

    pub struct LinuxEvdevMonitor {
        shutdown: Arc<AtomicBool>,
    }

    impl LinuxEvdevMonitor {
        pub fn start(
            config: LinuxEvdevConfig,
            dictation_tx: Sender<(HotkeyEvent, TriggerSource)>,
        ) -> Result<Self, String> {
            let needs_combo = config
                .side_combo
                .as_ref()
                .is_some_and(binding_requires_side_aware_hook);
            if !config.mouse.middle_enabled
                && !config.mouse.side_enabled
                && !needs_combo
            {
                return Err("linux evdev monitor has nothing to watch".into());
            }

            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_for_thread = Arc::clone(&shutdown);
            thread::Builder::new()
                .name("openless-linux-evdev".into())
                .spawn(move || {
                    run_loop(config, dictation_tx, shutdown_for_thread);
                })
                .map_err(|e| format!("spawn linux evdev thread failed: {e}"))?;

            Ok(Self { shutdown })
        }
    }

    impl Drop for LinuxEvdevMonitor {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
        }
    }

    fn run_loop(
        config: LinuxEvdevConfig,
        dictation_tx: Sender<(HotkeyEvent, TriggerSource)>,
        shutdown: Arc<AtomicBool>,
    ) {
        let mut devices: Vec<Device> = Vec::new();
        let mut rescan_at = std::time::Instant::now();
        let middle_held = AtomicBool::new(false);
        let side_held = AtomicBool::new(false);
        while !shutdown.load(Ordering::SeqCst) {
            if devices.is_empty() || rescan_at.elapsed() >= Duration::from_secs(5) {
                match open_input_devices() {
                    Ok(opened) if !opened.is_empty() => {
                        devices = opened;
                        set_status_message(None);
                    }
                    Ok(_) => {
                        set_status_message(Some(format!(
                            "未找到可读的输入设备。{UDEV_HINT}"
                        )));
                    }
                    Err(err) => {
                        set_status_message(Some(format!("{err} {UDEV_HINT}")));
                    }
                }
                rescan_at = std::time::Instant::now();
            }

            if devices.is_empty() {
                thread::sleep(Duration::from_secs(3));
                continue;
            }

            poll_devices(
                &mut devices,
                &config,
                &dictation_tx,
                &shutdown,
                &middle_held,
                &side_held,
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn open_input_devices() -> Result<Vec<Device>, String> {
        let mut devices = Vec::new();
        for entry in
            std::fs::read_dir("/dev/input").map_err(|e| format!("read /dev/input: {e}"))?
        {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path.to_string_lossy().contains("/dev/input/event") {
                continue;
            }
            if let Ok(device) = Device::open(&path) {
                devices.push(device);
            }
        }
        Ok(devices)
    }

    fn poll_devices(
        devices: &mut Vec<Device>,
        config: &LinuxEvdevConfig,
        dictation_tx: &Sender<(HotkeyEvent, TriggerSource)>,
        shutdown: &Arc<AtomicBool>,
        middle_held: &AtomicBool,
        side_held: &AtomicBool,
    ) {
        devices.retain_mut(|device| {
            if shutdown.load(Ordering::SeqCst) {
                return false;
            }
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        dispatch_event(
                            &event,
                            config,
                            dictation_tx,
                            middle_held,
                            side_held,
                        );
                    }
                    true
                }
                Err(err) => {
                    log::warn!("[linux-evdev] device read failed: {err}");
                    false
                }
            }
        });
    }

    fn key_edge_value(value: i32) -> Option<bool> {
        match value {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn dispatch_event(
        event: &InputEvent,
        config: &LinuxEvdevConfig,
        dictation_tx: &Sender<(HotkeyEvent, TriggerSource)>,
        middle_held: &AtomicBool,
        side_held: &AtomicBool,
    ) {
        let InputEventKind::Key(key) = event.kind() else {
            return;
        };
        let Some(pressed) = key_edge_value(event.value()) else {
            return;
        };

        match key {
            Key::BTN_MIDDLE if config.mouse.middle_enabled => {
                dispatch_mouse_edge(dictation_tx, middle_held, TriggerSource::MouseMiddle, pressed);
            }
            Key::BTN_SIDE | Key::BTN_EXTRA if config.mouse.side_enabled => {
                dispatch_mouse_edge(dictation_tx, side_held, TriggerSource::MouseSide, pressed);
            }
            _ if config.side_combo.is_some() => {
                if let Some(side) = side_from_key(key) {
                    handle_side_modifier(side, pressed);
                } else if let Some(primary) = primary_from_key(key) {
                    handle_primary_key(&primary, pressed);
                }
            }
            _ => {}
        }
    }

    fn dispatch_mouse_edge(
        tx: &Sender<(HotkeyEvent, TriggerSource)>,
        held: &AtomicBool,
        source: TriggerSource,
        pressed: bool,
    ) {
        if pressed {
            if !held.swap(true, Ordering::SeqCst) {
                let _ = tx.send((HotkeyEvent::Pressed, source));
            }
        } else if held.swap(false, Ordering::SeqCst) {
            let _ = tx.send((HotkeyEvent::Released, source));
        }
    }

    fn side_from_key(key: Key) -> Option<SideModifier> {
        match key {
            Key::KEY_LEFTMETA => Some(SideModifier::CmdLeft),
            Key::KEY_RIGHTMETA => Some(SideModifier::CmdRight),
            Key::KEY_LEFTCTRL => Some(SideModifier::CtrlLeft),
            Key::KEY_RIGHTCTRL => Some(SideModifier::CtrlRight),
            Key::KEY_LEFTALT => Some(SideModifier::AltLeft),
            Key::KEY_RIGHTALT => Some(SideModifier::AltRight),
            Key::KEY_LEFTSHIFT => Some(SideModifier::ShiftLeft),
            Key::KEY_RIGHTSHIFT => Some(SideModifier::ShiftRight),
            _ => None,
        }
    }

    fn primary_from_key(key: Key) -> Option<String> {
        match key {
            Key::KEY_A => Some("A".into()),
            Key::KEY_B => Some("B".into()),
            Key::KEY_C => Some("C".into()),
            Key::KEY_D => Some("D".into()),
            Key::KEY_E => Some("E".into()),
            Key::KEY_F => Some("F".into()),
            Key::KEY_G => Some("G".into()),
            Key::KEY_H => Some("H".into()),
            Key::KEY_I => Some("I".into()),
            Key::KEY_J => Some("J".into()),
            Key::KEY_K => Some("K".into()),
            Key::KEY_L => Some("L".into()),
            Key::KEY_M => Some("M".into()),
            Key::KEY_N => Some("N".into()),
            Key::KEY_O => Some("O".into()),
            Key::KEY_P => Some("P".into()),
            Key::KEY_Q => Some("Q".into()),
            Key::KEY_R => Some("R".into()),
            Key::KEY_S => Some("S".into()),
            Key::KEY_T => Some("T".into()),
            Key::KEY_U => Some("U".into()),
            Key::KEY_V => Some("V".into()),
            Key::KEY_W => Some("W".into()),
            Key::KEY_X => Some("X".into()),
            Key::KEY_Y => Some("Y".into()),
            Key::KEY_Z => Some("Z".into()),
            Key::KEY_0 => Some("0".into()),
            Key::KEY_1 => Some("1".into()),
            Key::KEY_2 => Some("2".into()),
            Key::KEY_3 => Some("3".into()),
            Key::KEY_4 => Some("4".into()),
            Key::KEY_5 => Some("5".into()),
            Key::KEY_6 => Some("6".into()),
            Key::KEY_7 => Some("7".into()),
            Key::KEY_8 => Some("8".into()),
            Key::KEY_9 => Some("9".into()),
            Key::KEY_SPACE => Some("Space".into()),
            Key::KEY_ENTER => Some("Enter".into()),
            Key::KEY_TAB => Some("Tab".into()),
            Key::KEY_BACKSPACE => Some("Backspace".into()),
            Key::KEY_DELETE => Some("Delete".into()),
            Key::KEY_ESC => Some("Escape".into()),
            Key::KEY_F1 => Some("F1".into()),
            Key::KEY_F2 => Some("F2".into()),
            Key::KEY_F3 => Some("F3".into()),
            Key::KEY_F4 => Some("F4".into()),
            Key::KEY_F5 => Some("F5".into()),
            Key::KEY_F6 => Some("F6".into()),
            Key::KEY_F7 => Some("F7".into()),
            Key::KEY_F8 => Some("F8".into()),
            Key::KEY_F9 => Some("F9".into()),
            Key::KEY_F10 => Some("F10".into()),
            Key::KEY_F11 => Some("F11".into()),
            Key::KEY_F12 => Some("F12".into()),
            _ => None,
        }
    }

    pub fn is_available() -> bool {
        std::path::Path::new("/dev/input").is_dir()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn mouse_middle_edge_dispatches_press_release() {
            let (tx, rx) = std::sync::mpsc::channel();
            let held = AtomicBool::new(false);
            dispatch_mouse_edge(&tx, &held, TriggerSource::MouseMiddle, true);
            assert_eq!(rx.recv().unwrap().0, HotkeyEvent::Pressed);
            dispatch_mouse_edge(&tx, &held, TriggerSource::MouseMiddle, true);
            assert!(rx.try_recv().is_err());
            dispatch_mouse_edge(&tx, &held, TriggerSource::MouseMiddle, false);
            assert_eq!(rx.recv().unwrap().0, HotkeyEvent::Released);
        }

        #[test]
        fn mouse_side_edge_dispatches() {
            let (tx, rx) = std::sync::mpsc::channel();
            let held = AtomicBool::new(false);
            dispatch_mouse_edge(&tx, &held, TriggerSource::MouseSide, true);
            assert_eq!(rx.recv().unwrap().1, TriggerSource::MouseSide);
        }

        #[test]
        fn mouse_held_state_persists_across_poll_cycles() {
            let (tx, rx) = std::sync::mpsc::channel();
            let middle_held = AtomicBool::new(false);
            dispatch_mouse_edge(&tx, &middle_held, TriggerSource::MouseMiddle, true);
            assert_eq!(rx.recv().unwrap().0, HotkeyEvent::Pressed);
            dispatch_mouse_edge(&tx, &middle_held, TriggerSource::MouseMiddle, false);
            assert_eq!(rx.recv().unwrap().0, HotkeyEvent::Released);
        }

        #[test]
        fn key_edge_value_ignores_repeat() {
            assert_eq!(key_edge_value(1), Some(true));
            assert_eq!(key_edge_value(0), Some(false));
            assert_eq!(key_edge_value(2), None);
        }

        #[test]
        fn side_combo_follows_side_aware_dictation_hotkey() {
            let mut prefs = crate::types::UserPreferences::default();
            prefs.dictation_hotkey = crate::types::ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-left".into()],
            };
            let config = config_from_prefs(&prefs);
            assert!(config.side_combo.is_some());
            assert!(monitor_needed(&config));
        }

        #[test]
        fn generic_combo_without_mouse_does_not_need_monitor() {
            let mut prefs = crate::types::UserPreferences::default();
            prefs.dictation_hotkey = crate::types::ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            };
            prefs.mouse_middle_button_dictation = false;
            prefs.mouse_side_button_dictation = false;
            let config = config_from_prefs(&prefs);
            assert!(config.side_combo.is_none());
            assert!(!monitor_needed(&config));
        }

        #[test]
        fn mouse_only_config_has_no_side_combo() {
            let mut prefs = crate::types::UserPreferences::default();
            prefs.mouse_middle_button_dictation = true;
            let config = config_from_prefs(&prefs);
            assert!(config.side_combo.is_none());
            assert!(monitor_needed(&config));
        }

        #[test]
        fn side_combo_to_generic_without_mouse_stops_monitor() {
            let mut prefs = crate::types::UserPreferences::default();
            prefs.dictation_hotkey = crate::types::ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-left".into()],
            };
            assert!(monitor_needed(&config_from_prefs(&prefs)));

            prefs.dictation_hotkey = crate::types::ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["ctrl".into(), "shift".into()],
            };
            prefs.mouse_middle_button_dictation = false;
            prefs.mouse_side_button_dictation = false;
            assert!(!monitor_needed(&config_from_prefs(&prefs)));
        }

        #[test]
        fn mouse_only_to_side_combo_adds_side_combo() {
            let mut prefs = crate::types::UserPreferences::default();
            prefs.mouse_middle_button_dictation = true;
            assert!(config_from_prefs(&prefs).side_combo.is_none());

            prefs.dictation_hotkey = crate::types::ShortcutBinding {
                primary: "D".into(),
                modifiers: vec!["cmd-left".into()],
            };
            let config = config_from_prefs(&prefs);
            assert!(config.side_combo.is_some());
            assert!(monitor_needed(&config));
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(not(target_os = "linux"))]
pub struct LinuxEvdevConfig {
    pub side_combo: Option<crate::types::ShortcutBinding>,
    pub mouse: crate::mouse_dictation::MouseDictationConfig,
}

#[cfg(not(target_os = "linux"))]
pub struct LinuxEvdevMonitor;

#[cfg(not(target_os = "linux"))]
impl LinuxEvdevMonitor {
    pub fn start(
        _config: LinuxEvdevConfig,
        _dictation_tx: std::sync::mpsc::Sender<(
            crate::hotkey::HotkeyEvent,
            crate::hold_source_tracker::TriggerSource,
        )>,
    ) -> Result<Self, String> {
        Err("linux evdev only available on linux".into())
    }
}

#[cfg(not(target_os = "linux"))]
pub fn status_message() -> Option<String> {
    None
}

#[cfg(not(target_os = "linux"))]
pub fn set_status_message(_message: Option<String>) {}

#[cfg(not(target_os = "linux"))]
pub fn is_available() -> bool {
    false
}
