//! Global hotkey listener dedicated to the selection-based voice QA flow.
//!
//! 与 `hotkey.rs`（modifier-only 听写热键）平行——QA 用的是组合键
//! `Cmd+Shift+;` / `Ctrl+Shift+;`，所以走 `global-hotkey` crate（macOS 内部
//! 用 Carbon `RegisterEventHotKey`，Windows 用 `RegisterHotKey`）。
//!
//! Produces only `QaHotkeyEvent::Pressed` edge events; the toggle / recording lifecycle is
//! interpreted by the coordinator (first press -> start QA; second press -> end).
//!
//! Shares the process-wide manager / event receiver via `global_hotkey_runtime`.

use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use parking_lot::Mutex;

use crate::global_hotkey_runtime::{GlobalHotkeyRuntime, RegisteredHotkey};
use crate::shortcut_binding::{parse_global_hotkey, ShortcutBindingError};
use crate::types::ShortcutBinding;

#[derive(Debug, Clone, Copy)]
pub enum QaHotkeyEvent {
    /// The user pressed the configured QA chord (toggle mode: first press starts, second ends).
    Pressed,
}

#[derive(Debug, thiserror::Error)]
pub enum QaHotkeyError {
    #[error("不支持的修饰键: {0}")]
    UnsupportedModifier(String),
    #[error("不支持的主键: {0}")]
    UnsupportedKey(String),
    #[error("注册全局快捷键失败: {0}")]
    RegisterFailed(String),
    #[error("初始化全局快捷键管理器失败: {0}")]
    ManagerInitFailed(String),
}

/// QA global hotkey listener. Unregisters on `Drop`.
///
/// Uses the `global-hotkey` crate internally; the forwarding thread holds a shared `Sender`.
pub struct QaHotkeyMonitor {
    inner: Arc<Inner>,
}

struct Inner {
    /// Handle of the currently registered hotkey; used to unregister.
    registered: Mutex<Option<RegisteredHotkey>>,
    tx: Sender<QaHotkeyEvent>,
}

// global-hotkey 0.6's GlobalHotKeyManager internally holds HHOOK / window handles and other
// `*mut c_void` on Windows, and the crate does not mark it Send/Sync. These handles are OS
// process-level resources whose cross-thread access the OS itself synchronizes; coordinator.rs
// needs to put `Arc<Inner>` (indirectly holding QaHotkeyMonitor) into async_runtime::spawn,
// which requires Send. Mark manually. The same applies to GlobalHotKeyManager on macOS, which
// uses Carbon EventHotKey internally.
// Same approach as the existing unsafe impl Send/Sync for hotkey.rs::CallbackContext.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl QaHotkeyMonitor {
    /// Start listening and register one hotkey. `tx` receives `QaHotkeyEvent::Pressed` on each
    /// press edge.
    ///
    /// **Note**: the `global-hotkey` crate requires the manager to be constructed on the main
    /// thread on macOS. The caller must ensure the call is initiated from the main thread (the
    /// coordinator's supervisor thread hops to the main thread via
    /// `AppHandle::run_on_main_thread` before spawning this monitor).
    /// This function does not assert the main thread — unit / integration tests never reach the
    /// manager creation line either.
    pub fn start(
        binding: ShortcutBinding,
        tx: Sender<QaHotkeyEvent>,
    ) -> Result<Self, QaHotkeyError> {
        let runtime = GlobalHotkeyRuntime::shared()
            .map_err(|e| QaHotkeyError::ManagerInitFailed(e.to_string()))?;

        let hotkey = parse_binding(&binding)?;
        let (registered, rx) = runtime
            .register(hotkey)
            .map_err(|e| QaHotkeyError::RegisterFailed(e.to_string()))?;

        // Start the forwarding thread: the runtime already dispatches by hotkey id; the id
        // check stays as a defensive line so a future accidental re-connection to the
        // process-wide event stream cannot leak into other shortcuts.
        let hotkey_id = registered.hotkey().id();
        let tx_for_thread = tx.clone();
        std::thread::Builder::new()
            .name("openless-qa-hotkey-forward".into())
            .spawn(move || forward_loop(hotkey_id, rx, tx_for_thread))
            .map_err(|e| QaHotkeyError::RegisterFailed(format!("spawn forward thread: {e}")))?;

        Ok(Self {
            inner: Arc::new(Inner {
                registered: Mutex::new(Some(registered)),
                tx,
            }),
        })
    }

    /// Replace the currently registered hotkey (when the user changes the chord in settings).
    pub fn update_binding(&self, binding: ShortcutBinding) -> Result<(), QaHotkeyError> {
        let next = parse_binding(&binding)?;
        let mut current = self.inner.registered.lock();
        if let Some(prev) = current.as_ref() {
            if prev.hotkey() == next {
                return Ok(());
            }
        }
        let runtime = GlobalHotkeyRuntime::shared()
            .map_err(|e| QaHotkeyError::ManagerInitFailed(e.to_string()))?;
        let (registered, rx) = runtime
            .register(next)
            .map_err(|e| QaHotkeyError::RegisterFailed(e.to_string()))?;
        let hotkey_id = registered.hotkey().id();
        // Keep event forwarding alive for the replacement registration.
        std::thread::Builder::new()
            .name("openless-qa-hotkey-forward".into())
            .spawn({
                let tx = self.inner.tx.clone();
                move || forward_loop(hotkey_id, rx, tx)
            })
            .map_err(|e| QaHotkeyError::RegisterFailed(format!("spawn forward thread: {e}")))?;
        *current = Some(registered);
        Ok(())
    }
}

impl Drop for QaHotkeyMonitor {
    fn drop(&mut self) {
        self.inner.registered.lock().take();
    }
}

fn forward_loop(hotkey_id: u32, rx: Receiver<GlobalHotKeyEvent>, tx: Sender<QaHotkeyEvent>) {
    while let Ok(event) = rx.recv() {
        if event.id() != hotkey_id {
            continue;
        }
        if !matches!(event.state(), HotKeyState::Pressed) {
            continue;
        }
        if let Err(e) = tx.send(QaHotkeyEvent::Pressed) {
            log::warn!("[qa-hotkey] 事件投递失败: {e}");
            break;
        }
    }
    log::info!("[qa-hotkey] 转发线程退出");
}

fn parse_binding(
    binding: &ShortcutBinding,
) -> Result<global_hotkey::hotkey::HotKey, QaHotkeyError> {
    parse_global_hotkey(binding).map_err(|e| match e {
        ShortcutBindingError::UnsupportedModifier(m) => QaHotkeyError::UnsupportedModifier(m),
        ShortcutBindingError::UnsupportedKey(k) => QaHotkeyError::UnsupportedKey(k),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::{Code, Modifiers};

    #[test]
    fn parse_default_binding() {
        let binding = ShortcutBinding::default_qa();
        let parsed = parse_binding(&binding).expect("default binding parses");
        assert!(parsed.mods.contains(Modifiers::SHIFT));
        assert_eq!(parsed.key, Code::Semicolon);
    }

    #[test]
    fn parse_letter_binding() {
        let binding = ShortcutBinding {
            primary: "k".into(),
            modifiers: vec!["cmd".into(), "alt".into()],
        };
        let parsed = parse_binding(&binding).expect("letter binding parses");
        assert_eq!(parsed.key, Code::KeyK);
        #[cfg(target_os = "windows")]
        assert!(parsed.mods.contains(Modifiers::CONTROL));
        #[cfg(not(target_os = "windows"))]
        assert!(parsed.mods.contains(Modifiers::SUPER));
        assert!(parsed.mods.contains(Modifiers::ALT));
    }

    #[test]
    fn unsupported_modifier_rejected() {
        let binding = ShortcutBinding {
            primary: ";".into(),
            modifiers: vec!["hyper".into()],
        };
        assert!(matches!(
            parse_binding(&binding),
            Err(QaHotkeyError::UnsupportedModifier(_))
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
            Err(QaHotkeyError::UnsupportedKey(_))
        ));
    }

    #[test]
    fn cmd_modifier_normalizes_per_platform() {
        let binding = ShortcutBinding {
            primary: ";".into(),
            modifiers: vec!["cmd".into(), "shift".into()],
        };
        let parsed = parse_binding(&binding).expect("binding parses");

        #[cfg(target_os = "windows")]
        {
            assert!(parsed.mods.contains(Modifiers::CONTROL));
            assert!(!parsed.mods.contains(Modifiers::SUPER));
        }

        #[cfg(not(target_os = "windows"))]
        {
            assert!(parsed.mods.contains(Modifiers::SUPER));
        }
    }

    #[test]
    fn forward_loop_ignores_unrelated_hotkey_ids() {
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (out_tx, out_rx) = std::sync::mpsc::channel();

        event_tx
            .send(GlobalHotKeyEvent {
                id: 41,
                state: HotKeyState::Pressed,
            })
            .unwrap();
        event_tx
            .send(GlobalHotKeyEvent {
                id: 42,
                state: HotKeyState::Released,
            })
            .unwrap();
        event_tx
            .send(GlobalHotKeyEvent {
                id: 42,
                state: HotKeyState::Pressed,
            })
            .unwrap();
        drop(event_tx);

        forward_loop(42, event_rx, out_tx);

        assert!(matches!(out_rx.recv().unwrap(), QaHotkeyEvent::Pressed));
        assert!(out_rx.try_recv().is_err());
    }
}
