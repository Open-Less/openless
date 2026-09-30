//! Custom combo-key listener for the recording hotkey.
//!
//! Parallel to `hotkey.rs` (modifier-only dictation hotkey) — when the user picks a custom
//! combo key (e.g. `Cmd+Shift+D`), it registers via the `global-hotkey` crate.
//!
//! Key difference from `qa_hotkey.rs`: it emits BOTH Pressed and Released edge events
//! to support Hold (push-to-talk) mode. The `global-hotkey` crate's `HotKeyState::Released`
//! works for detecting release on both macOS (Carbon) and Windows.
//!
//! Shares the process-level manager / event receiver with the QA hotkey via `global_hotkey_runtime`.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use parking_lot::Mutex;

use crate::global_hotkey_runtime::{GlobalHotkeyRuntime, RegisteredHotkey};
use crate::shortcut_binding::{parse_global_hotkey, ShortcutBindingError};
use crate::types::ShortcutBinding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboHotkeyEvent {
    /// The user pressed the configured combo key.
    Pressed { at: Instant },
    /// The user released the configured combo key (ends recording in Hold mode).
    Released { at: Instant },
}

#[derive(Debug, thiserror::Error)]
pub enum ComboHotkeyError {
    #[error("不支持的修饰键: {0}")]
    UnsupportedModifier(String),
    #[error("不支持的主键: {0}")]
    UnsupportedKey(String),
    #[error("注册全局快捷键失败: {0}")]
    RegisterFailed(String),
    #[error("初始化全局快捷键管理器失败: {0}")]
    ManagerInitFailed(String),
}

/// Global hotkey listener for the custom combo key. Unregisters on `Drop`.
///
/// Uses the `global-hotkey` crate internally; the event forwarding thread holds a shared `Sender`.
/// Unlike `QaHotkeyMonitor`, it forwards BOTH Pressed and Released events.
pub struct ComboHotkeyMonitor {
    inner: Arc<Inner>,
}

struct Inner {
    registered: Mutex<Option<RegisteredHotkey>>,
    #[cfg(target_os = "macos")]
    native_dictation: Mutex<Option<crate::macos_dictation_key::Monitor>>,
    tx: Sender<ComboHotkeyEvent>,
}

// global-hotkey 0.6's GlobalHotKeyManager internally holds HHOOK / window handles and other
// `*mut c_void` on Windows; the crate doesn't mark Send/Sync. Same reasoning as qa_hotkey.rs.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl ComboHotkeyMonitor {
    /// Start listening and register one combo key. `tx` receives an event on every press/release edge.
    ///
    /// Note: the `global-hotkey` crate requires the manager to be constructed on the main thread
    /// on macOS. Callers must trigger this from the main thread.
    pub fn start(
        binding: ShortcutBinding,
        tx: Sender<ComboHotkeyEvent>,
    ) -> Result<Self, ComboHotkeyError> {
        #[cfg(target_os = "macos")]
        if is_native_dictation(&binding) {
            let native = crate::macos_dictation_key::Monitor::start(tx.clone())
                .map_err(ComboHotkeyError::RegisterFailed)?;
            return Ok(Self {
                inner: Arc::new(Inner {
                    registered: Mutex::new(None),
                    native_dictation: Mutex::new(Some(native)),
                    tx,
                }),
            });
        }
        let runtime = GlobalHotkeyRuntime::shared()
            .map_err(|e| ComboHotkeyError::ManagerInitFailed(e.to_string()))?;

        let hotkey = parse_binding(&binding)?;
        let (registered, rx) = runtime
            .register(hotkey)
            .map_err(|e| ComboHotkeyError::RegisterFailed(e.to_string()))?;

        // The runtime already dispatches by hotkey id; the id check stays as a defense line in case a
        // future change wires this back to the process-level event stream and bleeds into other hotkeys.
        let hotkey_id = registered.hotkey().id();
        let tx_for_thread = tx.clone();
        std::thread::Builder::new()
            .name("openless-combo-hotkey-forward".into())
            .spawn(move || forward_loop(hotkey_id, rx, tx_for_thread))
            .map_err(|e| ComboHotkeyError::RegisterFailed(format!("spawn forward thread: {e}")))?;

        Ok(Self {
            inner: Arc::new(Inner {
                registered: Mutex::new(Some(registered)),
                #[cfg(target_os = "macos")]
                native_dictation: Mutex::new(None),
                tx,
            }),
        })
    }

    /// Replace the currently registered combo key (user changed it in settings).
    pub fn update_binding(&self, binding: ShortcutBinding) -> Result<(), ComboHotkeyError> {
        #[cfg(target_os = "macos")]
        if is_native_dictation(&binding) {
            if self.native_dictation_active() {
                return Ok(());
            }
            let native = crate::macos_dictation_key::Monitor::start(self.inner.tx.clone())
                .map_err(ComboHotkeyError::RegisterFailed)?;
            *self.inner.native_dictation.lock() = Some(native);
            self.inner.registered.lock().take();
            return Ok(());
        }
        let next = parse_binding(&binding)?;
        let mut current = self.inner.registered.lock();
        if let Some(prev) = current.as_ref() {
            if prev.hotkey() == next {
                return Ok(());
            }
        }
        let runtime = GlobalHotkeyRuntime::shared()
            .map_err(|e| ComboHotkeyError::ManagerInitFailed(e.to_string()))?;
        let (registered, rx) = runtime
            .register(next)
            .map_err(|e| ComboHotkeyError::RegisterFailed(e.to_string()))?;
        let hotkey_id = registered.hotkey().id();
        std::thread::Builder::new()
            .name("openless-combo-hotkey-forward".into())
            .spawn({
                let tx = self.inner.tx.clone();
                move || forward_loop(hotkey_id, rx, tx)
            })
            .map_err(|e| ComboHotkeyError::RegisterFailed(format!("spawn forward thread: {e}")))?;
        *current = Some(registered);
        #[cfg(target_os = "macos")]
        self.inner.native_dictation.lock().take();
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn is_native_dictation(binding: &ShortcutBinding) -> bool {
    binding.primary == crate::macos_dictation_key::PRIMARY && binding.modifiers.is_empty()
}
#[cfg(target_os = "macos")]
impl ComboHotkeyMonitor {
    pub fn native_dictation_active(&self) -> bool {
        self.inner
            .native_dictation
            .lock()
            .as_ref()
            .is_some_and(|m| m.active())
    }
}
impl Drop for ComboHotkeyMonitor {
    fn drop(&mut self) {
        self.inner.registered.lock().take();
        #[cfg(target_os = "macos")]
        self.inner.native_dictation.lock().take();
    }
}

fn forward_loop(hotkey_id: u32, rx: Receiver<GlobalHotKeyEvent>, tx: Sender<ComboHotkeyEvent>) {
    while let Ok(event) = rx.recv() {
        if event.id() != hotkey_id {
            continue;
        }
        let at = Instant::now();
        let combo_event = match event.state() {
            HotKeyState::Pressed => ComboHotkeyEvent::Pressed { at },
            HotKeyState::Released => ComboHotkeyEvent::Released { at },
        };
        if let Err(e) = tx.send(combo_event) {
            log::warn!("[combo-hotkey] 事件投递失败: {e}");
            break;
        }
    }
    log::info!("[combo-hotkey] 转发线程退出");
}

/// Test whether a combo key can be registered (validates format only, no actual registration).
pub fn validate_binding(binding: &ShortcutBinding) -> Result<(), ComboHotkeyError> {
    #[cfg(target_os = "macos")]
    if is_native_dictation(binding) {
        return Ok(());
    }
    parse_binding(binding)?;
    Ok(())
}

fn parse_binding(
    binding: &ShortcutBinding,
) -> Result<global_hotkey::hotkey::HotKey, ComboHotkeyError> {
    parse_global_hotkey(binding).map_err(|e| match e {
        ShortcutBindingError::UnsupportedModifier(m) => ComboHotkeyError::UnsupportedModifier(m),
        ShortcutBindingError::UnsupportedKey(k) => ComboHotkeyError::UnsupportedKey(k),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::{Code, Modifiers};

    #[test]
    fn parse_cmd_shift_d() {
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let parsed = parse_binding(&binding).expect("binding parses");
        #[cfg(target_os = "windows")]
        assert!(parsed.mods.contains(Modifiers::CONTROL));
        #[cfg(not(target_os = "windows"))]
        assert!(parsed.mods.contains(Modifiers::SUPER));
        assert!(parsed.mods.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::KeyD);
    }

    #[test]
    fn parse_ctrl_shift_space() {
        let binding = ShortcutBinding {
            primary: "Space".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };
        let parsed = parse_binding(&binding).expect("binding parses");
        assert!(parsed.mods.contains(Modifiers::CONTROL));
        assert!(parsed.mods.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::Space);
    }

    #[test]
    fn unsupported_modifier_rejected() {
        let binding = ShortcutBinding {
            primary: "D".into(),
            modifiers: vec!["hyper".into()],
        };
        assert!(matches!(
            parse_binding(&binding),
            Err(ComboHotkeyError::UnsupportedModifier(_))
        ));
    }

    #[test]
    fn empty_primary_rejected() {
        let binding = ShortcutBinding {
            primary: "".into(),
            modifiers: vec!["cmd".into()],
        };
        assert!(matches!(
            parse_binding(&binding),
            Err(ComboHotkeyError::UnsupportedKey(_))
        ));
    }

    #[test]
    fn bare_shift_is_rejected_for_combo_hotkey() {
        let binding = ShortcutBinding {
            primary: "Shift".into(),
            modifiers: vec![],
        };
        assert!(matches!(
            validate_binding(&binding),
            Err(ComboHotkeyError::UnsupportedKey(_))
        ));
    }

    #[test]
    fn legacy_modifier_only_is_rejected_for_combo_hotkey() {
        let binding = ShortcutBinding {
            primary: "RightOption".into(),
            modifiers: vec![],
        };
        assert!(matches!(
            validate_binding(&binding),
            Err(ComboHotkeyError::UnsupportedKey(_))
        ));
    }

    #[test]
    fn forward_loop_ignores_unrelated_hotkey_ids() {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (out_tx, out_rx) = std::sync::mpsc::channel();

        event_tx
            .send(GlobalHotKeyEvent {
                id: 7,
                state: HotKeyState::Pressed,
            })
            .unwrap();
        event_tx
            .send(GlobalHotKeyEvent {
                id: 8,
                state: HotKeyState::Released,
            })
            .unwrap();
        event_tx
            .send(GlobalHotKeyEvent {
                id: 8,
                state: HotKeyState::Pressed,
            })
            .unwrap();
        drop(event_tx);

        forward_loop(8, event_rx, out_tx);

        assert!(matches!(
            out_rx.recv().unwrap(),
            ComboHotkeyEvent::Released { .. }
        ));
        assert!(matches!(
            out_rx.recv().unwrap(),
            ComboHotkeyEvent::Pressed { .. }
        ));
        assert!(out_rx.try_recv().is_err());
    }
}
