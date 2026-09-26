//! Journal-fenced reconciliation of actual Host state. This is deliberately
//! separate from ordinary settings persistence: credentials and preferences
//! have already been restored under the exclusive source permit.
use super::*;
use futures_util::future::BoxFuture;
use openless_core::{BackendError, BackendErrorCode};
use std::sync::Weak;

struct RestoreHost {
    coordinator: Weak<Inner>,
}

fn failure(reason: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, reason)
}

impl openless_core::config::RestoreRuntimeEffects for RestoreHost {
    fn apply_target(
        &self,
        target: crate::types::UserPreferences,
    ) -> BoxFuture<'static, Result<(), BackendError>> {
        let weak = self.coordinator.clone();
        Box::pin(async move {
            let inner = weak
                .upgrade()
                .ok_or_else(|| failure("restore_host_unavailable"))?;
            openless_core::reject_hotkey_collisions(&target)
                .map_err(|_| failure("restore_hotkey_conflict"))?;
            let coord = Coordinator {
                inner: Arc::clone(&inner),
            };
            let work_target = target.clone();
            let work_inner = Arc::clone(&inner);
            // Ordinary settings transactions take this same Host lock and are
            // refused by Core's restoring guard before preparing new effects.
            inner
                .host
                .spawn_blocking(move || {
                    let coord = Coordinator { inner: work_inner };
                    let _host_guard = coord.lock_settings_host();
                    #[cfg(target_os = "windows")]
                    crate::windows_ime_profile::apply_windows_openless_keyboard_list(
                        openless_core::WindowsKeyboardRuntimeTarget::from(&work_target)
                            .openless_language_profile_enabled,
                    )
                    .map_err(|_| failure("restore_windows_keyboard_failed"))?;
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    reconcile_hotkeys(&coord, (&work_target).into())?;
                    #[cfg(not(any(
                        target_os = "macos",
                        target_os = "windows",
                        target_os = "android",
                        target_os = "ios"
                    )))]
                    return Err(failure("restore_host_platform_unsupported"));
                    crate::net::set_use_system_proxy(work_target.use_system_proxy);
                    coord
                        .inner
                        .host
                        .cache_capsule_style(work_target.capsule_style);
                    Ok::<_, BackendError>(())
                })
                .await
                .map_err(|_| failure("restore_host_task_interrupted"))??;

            #[cfg(target_os = "android")]
            match target.android_overlay_trigger.normalized() {
                crate::types::AndroidOverlayTrigger::Always => {
                    crate::android::replace_android_overlay()
                }
                crate::types::AndroidOverlayTrigger::Background
                | crate::types::AndroidOverlayTrigger::Keyboard => {
                    crate::android::hide_android_overlay()
                }
            }
            .map_err(|_| failure("restore_android_overlay_failed"))?;

            // Enabled remains receiver-local consent. Its restored port still
            // has to reach the actual listener; configure owns stop/start and
            // exposes bind failures to the journal's rollback path.
            reconcile_remote_input(
                inner.backend.services().remote_input.as_ref(),
                target.remote_input_enabled,
                target.remote_input_port,
            )
            .await?;
            // The presentation cache enqueues a native refresh. A final main
            // thread acknowledgement keeps that refresh inside the fence.
            let (send, receive) = tokio::sync::oneshot::channel();
            inner
                .host
                .run_on_main_thread(move || {
                    let _ = send.send(());
                })
                .map_err(|_| failure("restore_main_thread_unavailable"))?;
            receive
                .await
                .map_err(|_| failure("restore_main_thread_interrupted"))?;
            coord.backend().reset_restored_runtime_preferences();
            Ok(())
        })
    }
}

async fn reconcile_remote_input(
    remote: &dyn openless_core::domains::RemoteInputApi,
    enabled: bool,
    port: u16,
) -> Result<(), BackendError> {
    match remote.status() {
        Ok(status) => {
            if status.enabled != enabled || status.port != port || (enabled && !status.running) {
                remote
                    .configure(openless_core::RemoteInputConfig { enabled, port })
                    .await
                    .map_err(|_| failure("restore_remote_input_failed"))?;
            }
        }
        Err(error) if error.code == BackendErrorCode::Unsupported && !enabled => {}
        Err(_) => return Err(failure("restore_remote_input_unavailable")),
    }
    Ok(())
}

impl Coordinator {
    pub fn bind_restore_runtime_effects(&self) -> Result<(), BackendError> {
        if self.startup_error().is_some() {
            return Ok(());
        }
        if !self.inner.host.is_bound() {
            return Err(failure("restore_host_not_bound"));
        }
        self.inner
            .backend
            .bind_restore_runtime_effects(Arc::new(RestoreHost {
                coordinator: Arc::downgrade(&self.inner),
            }))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn reconcile_hotkeys(
    coord: &Coordinator,
    target: openless_core::HotkeyRuntimeTarget,
) -> Result<(), BackendError> {
    *coord.inner.hotkey_runtime_target.lock() = target.clone();
    let trigger = crate::shortcut_binding::legacy_modifier_trigger(&target.dictation);
    let binding = crate::types::HotkeyBinding {
        trigger: trigger.unwrap_or(crate::types::HotkeyTrigger::Custom),
        mode: target.dictation_mode,
        keys: None,
    };
    coord
        .try_ensure_modifier_hotkey_monitor(binding)
        .map_err(|_| failure("restore_dictation_monitor_failed"))?;
    let (send, receive) = mpsc::sync_channel(1);
    let inner = Arc::clone(&coord.inner);
    // No timeout success and no abandoned queued registration: keep waiting
    // for this exact callback before the journal can roll back another target.
    coord
        .inner
        .host
        .run_on_main_thread(move || {
            let result = reconcile_hotkeys_on_main(&inner, &target);
            let _ = send.send(result);
        })
        .map_err(|_| failure("restore_main_thread_unavailable"))?;
    receive
        .recv()
        .map_err(|_| failure("restore_main_thread_interrupted"))?
        .map_err(|_| failure("restore_hotkeys_failed"))?;
    coord
        .try_update_selection_polish_hotkey_binding()
        .map_err(|_| failure("restore_selection_hotkey_failed"))?;
    coord
        .update_coding_agent_hotkey_binding()
        .map_err(|_| failure("restore_agent_hotkey_failed"))?;
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn reconcile_hotkeys_on_main(
    inner: &Arc<Inner>,
    target: &openless_core::HotkeyRuntimeTarget,
) -> Result<(), String> {
    // Release old combinations together before applying an absolute target;
    // this also permits swapping two bindings during a restore.
    inner.combo_hotkey.lock().take();
    inner.qa_hotkey.lock().take();
    inner.translation_hotkey.lock().take();
    inner.switch_style_hotkey.lock().take();
    inner.open_app_hotkey.lock().take();
    inner.quick_note_hotkey.lock().take();
    inner.style_pack_hotkeys.lock().clear();
    inner.selection_polish_hotkey.lock().take();
    inner.coding_agent_combo_hotkey.lock().take();
    let trigger = crate::shortcut_binding::legacy_modifier_trigger(&target.dictation);
    if trigger.is_some() || is_unconfigured_shortcut(&target.dictation) {
        inner.side_aware_combo.lock().take();
    } else if crate::shortcut_binding::binding_requires_side_aware_hook(&target.dictation) {
        let mut side_slot = inner.side_aware_combo.lock();
        if let Some(monitor) = side_slot.as_ref() {
            monitor
                .update_binding(target.dictation.clone())
                .map_err(|error| error.to_string())?;
        } else {
            let (send, receive) = mpsc::channel();
            let combined = spawn_combo_abort_bridge(inner, handle_trigger_combined);
            let monitor = crate::side_aware_combo::SideAwareComboMonitor::start(
                target.dictation.clone(),
                send,
                combined,
            )
            .map_err(|error| error.to_string())?;
            let owned = Arc::clone(inner);
            std::thread::Builder::new()
                .name("openless-side-combo-bridge".into())
                .spawn(move || hotkey_bridge_loop(owned, receive))
                .map_err(|error| error.to_string())?;
            *side_slot = Some(monitor);
        }
    } else {
        inner.side_aware_combo.lock().take();
        let (send, receive) = mpsc::channel();
        let monitor = ComboHotkeyMonitor::start(target.dictation.clone(), send)
            .map_err(|error| error.to_string())?;
        let owned = Arc::clone(inner);
        std::thread::Builder::new()
            .name("openless-combo-hotkey-bridge".into())
            .spawn(move || combo_hotkey_bridge_loop(owned, receive))
            .map_err(|error| error.to_string())?;
        *inner.combo_hotkey.lock() = Some(monitor);
    }
    if let Some(binding) = target.qa.as_ref().filter(|binding| {
        crate::shortcut_binding::legacy_modifier_trigger(binding).is_none()
            && !is_unconfigured_shortcut(binding)
    }) {
        let (send, receive) = mpsc::channel();
        let monitor =
            QaHotkeyMonitor::start(binding.clone(), send).map_err(|error| error.to_string())?;
        let owned = Arc::clone(inner);
        std::thread::Builder::new()
            .name("openless-qa-hotkey-bridge".into())
            .spawn(move || qa_hotkey_bridge_loop(owned, receive))
            .map_err(|error| error.to_string())?;
        *inner.qa_hotkey.lock() = Some(monitor);
    }
    if !is_builtin_translation_shift(&target.translation)
        && crate::shortcut_binding::legacy_modifier_trigger(&target.translation).is_none()
        && !is_unconfigured_shortcut(&target.translation)
    {
        update_translation_hotkey_on_main_thread(Arc::clone(inner), target.translation.clone())
            .map_err(|error| error.to_string())?;
    }
    for kind in [
        ActionHotkeyKind::SwitchStyle,
        ActionHotkeyKind::OpenApp,
        ActionHotkeyKind::QuickNote,
    ] {
        let Some(binding) = action_hotkey_binding(inner, kind) else {
            continue;
        };
        if is_unconfigured_shortcut(&binding) {
            continue;
        }
        if is_modifier_only_shortcut(&binding) {
            return Err("unsupported action hotkey".into());
        }
        let (send, receive) = mpsc::channel();
        let monitor =
            ComboHotkeyMonitor::start(binding, send).map_err(|error| error.to_string())?;
        let owned = Arc::clone(inner);
        std::thread::Builder::new()
            .name(action_hotkey_bridge_thread_name(kind).into())
            .spawn(move || action_hotkey_bridge_loop(owned, receive, kind))
            .map_err(|error| error.to_string())?;
        *action_hotkey_slot(inner, kind).lock() = Some(monitor);
    }
    sync_style_pack_hotkeys(inner)?;
    if let Some(monitor) = inner.hotkey.lock().as_ref() {
        let (qa, selection, translation) = modifier_shortcut_triggers(inner);
        monitor.update_modifier_shortcuts(qa, selection, translation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::domains::RemoteInputApi;

    fn remote() -> (
        openless_core::RemoteInputService,
        Arc<openless_core::testing::RecordingRemoteInputRuntime>,
    ) {
        let runtime = Arc::new(openless_core::testing::RecordingRemoteInputRuntime::default());
        let service = openless_core::RemoteInputService::new(runtime.clone(), 8443, "en").unwrap();
        let publisher_owner = openless_core::OpenLessBackend::blocked_startup(
            openless_core::BackendConfig::default(),
            failure("fixture has no external adapters"),
        );
        service.bind_event_publisher(publisher_owner.event_publisher());
        (service, runtime)
    }

    #[tokio::test]
    async fn restore_host_running_remote_port_and_rollback_reach_the_listener() {
        let (remote, runtime) = remote();
        remote
            .configure(openless_core::RemoteInputConfig {
                enabled: true,
                port: 8443,
            })
            .await
            .unwrap();
        reconcile_remote_input(&remote, true, 9443).await.unwrap();
        assert!(remote.status().unwrap().running);
        assert_eq!(remote.status().unwrap().port, 9443);
        assert_eq!(runtime.server_start_count(), 2);
        assert_eq!(runtime.server_stop_count(), 1);
        // The journal invokes the same absolute callback with its old target.
        reconcile_remote_input(&remote, true, 8443).await.unwrap();
        assert!(remote.status().unwrap().running);
        assert_eq!(remote.status().unwrap().port, 8443);
        assert_eq!(runtime.server_start_count(), 3);
        assert_eq!(runtime.server_stop_count(), 2);
    }

    #[tokio::test]
    async fn restore_host_invalid_remote_target_is_an_error_and_disabled_capability_is_explicit() {
        let (remote, runtime) = remote();
        remote
            .configure(openless_core::RemoteInputConfig {
                enabled: true,
                port: 8443,
            })
            .await
            .unwrap();
        assert!(reconcile_remote_input(&remote, true, 0).await.is_err());
        assert_eq!(remote.status().unwrap().port, 8443);
        assert_eq!(runtime.server_stop_count(), 0);
        let unsupported = openless_core::domains::UnsupportedDomainServices;
        reconcile_remote_input(&unsupported, false, 8443)
            .await
            .unwrap();
        assert!(reconcile_remote_input(&unsupported, true, 8443)
            .await
            .is_err());
    }
}
