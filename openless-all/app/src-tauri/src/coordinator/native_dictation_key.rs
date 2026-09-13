//! Synchronous native-key registration participates in the Core settings transaction.
use super::*;

impl Coordinator {
    pub(crate) fn dictation_shortcut_is_busy(&self) -> bool {
        !matches!(
            self.backend().snapshot().dictation.phase,
            openless_core::DictationPhase::Idle
                | openless_core::DictationPhase::Completed
                | openless_core::DictationPhase::Cancelled
                | openless_core::DictationPhase::Failed
        )
    }

    /// Keep the previous listener until replacement registration succeeds. The
    /// caller is a worker thread; Carbon ownership changes run on the UI thread.
    pub(crate) fn try_update_native_dictation_binding(&self) -> Result<(), String> {
        let target = hotkey_runtime_target(&self.inner);
        let inner = Arc::clone(&self.inner);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        // Cancel queued work on timeout. Once a callback has started, wait for
        // its acknowledgement before Core can roll back the runtime target.
        // This gate never nests with the target mutex on the calling thread.
        let cancelled = Arc::new(Mutex::new(false));
        let callback_cancelled = Arc::clone(&cancelled);
        self.inner.host.run_on_main_thread(move || {
            let cancelled = callback_cancelled.lock();
            let result = (|| {
                if *cancelled || hotkey_runtime_target(&inner) != target {
                    return Err("macDictationKeyChanged".into());
                }
                let binding = target.dictation.clone();
                let trigger = crate::shortcut_binding::legacy_modifier_trigger(&binding);
                if trigger.is_some() || is_unconfigured_shortcut(&binding) {
                    if trigger.is_some() && inner.hotkey.lock().is_none() {
                        return Err("macDictationKeyUnavailable".into());
                    }
                    inner.combo_hotkey.lock().take();
                    inner.side_aware_combo.lock().take();
                } else if crate::shortcut_binding::binding_requires_side_aware_hook(&binding) {
                    let mut slot = inner.side_aware_combo.lock();
                    if let Some(monitor) = slot.as_ref() {
                        // A failed native registration leaves this route alive.
                        // Reuse its sender: dropping an old side-aware handle
                        // after creating another would clear the singleton route.
                        monitor
                            .update_binding(binding)
                            .map_err(|error| error.to_string())?;
                    } else {
                        let (tx, rx) = mpsc::channel();
                        let monitor =
                            crate::side_aware_combo::SideAwareComboMonitor::start(binding, tx)
                                .map_err(|error| error.to_string())?;
                        let bridge_inner = Arc::clone(&inner);
                        std::thread::Builder::new()
                            .name("openless-side-combo-bridge".into())
                            .spawn(move || combo_hotkey_bridge_loop(bridge_inner, rx))
                            .map_err(|error| error.to_string())?;
                        *slot = Some(monitor);
                    }
                    inner.combo_hotkey.lock().take();
                } else {
                    let mut slot = inner.combo_hotkey.lock();
                    if let Some(monitor) = slot.as_ref() {
                        monitor
                            .update_binding(binding)
                            .map_err(|error| error.to_string())?;
                    } else {
                        let (tx, rx) = mpsc::channel();
                        let monitor = ComboHotkeyMonitor::start(binding, tx)
                            .map_err(|error| error.to_string())?;
                        let bridge_inner = Arc::clone(&inner);
                        std::thread::Builder::new()
                            .name("openless-combo-hotkey-bridge".into())
                            .spawn(move || combo_hotkey_bridge_loop(bridge_inner, rx))
                            .map_err(|error| error.to_string())?;
                        *slot = Some(monitor);
                    }
                    inner.side_aware_combo.lock().take();
                }
                if let Some(monitor) = inner.hotkey.lock().as_ref() {
                    monitor.update_binding(crate::types::HotkeyBinding {
                        trigger: trigger.unwrap_or(crate::types::HotkeyTrigger::Custom),
                        mode: target.dictation_mode,
                        keys: None,
                    });
                }
                Ok(())
            })();
            let _ = done_tx.send(result);
        })?;
        match done_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => {
                let mut cancelled = cancelled.lock();
                // A running callback may have finished while we acquired the
                // gate. Its real result takes precedence over the timeout.
                match done_rx.try_recv() {
                    Ok(result) => result,
                    Err(_) => {
                        *cancelled = true;
                        Err("macDictationKeyUnavailable".into())
                    }
                }
            }
        }
    }
}
