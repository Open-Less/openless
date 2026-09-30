//! OS-native watching of tray microphone device changes (issue #470).
//!
//! Purpose: replace the "poll `list_input_devices()` every 10s" idle wakeup with
//! platform-native device-change notifications — zero wakeups while idle, with the OS
//! callback triggering a refresh in real time on device plug/unplug or default-device switch.
//!
//! Platform split:
//! - macOS: CoreAudio `AudioObjectAddPropertyListener` on `kAudioHardwarePropertyDevices`,
//!   with a dedicated thread running a resident `CFRunLoop`.
//! - Windows / Linux: returns `false` for now; `lib.rs`'s 60s slow polling fallback covers
//!   them. (Native Windows `IMMNotificationClient` notifications are left for later; they
//!   need a Windows dev machine to verify.)
//!
//! Contract shared by all three platforms: `spawn_native_watcher(app, on_change)`.
//! `on_change` is the debounce closure provided by `lib.rs` (it internally reuses
//! `microphone_device_signature()` so it only refreshes+emits on real changes). The callback
//! does nothing but call `on_change`. Registration failure always returns `false` (warn
//! only, no panic) and the fallback polling takes over, guaranteeing devices are always
//! detected on all three platforms.

use tauri::AppHandle;

/// Registers the OS-native device-change watcher. Returns `true` on success, `false` when
/// the platform is unsupported or registration fails.
///
/// `on_change` is invoked on the OS callback thread (possibly concurrent/duplicated); its
/// internals handle debouncing and thread dispatch.
#[cfg(target_os = "macos")]
pub(crate) fn spawn_native_watcher<F>(_app: AppHandle, on_change: F) -> bool
where
    F: Fn() + Send + Sync + 'static,
{
    macos::spawn(on_change)
}

/// Non-macOS (Windows / Linux): no locally verified native path yet; returns `false` and
/// relies entirely on `lib.rs`'s 60s slow polling fallback. Native Windows
/// `IMMNotificationClient` is left for later (needs a Windows dev machine to verify).
#[cfg(not(target_os = "macos"))]
pub(crate) fn spawn_native_watcher<F>(_app: AppHandle, _on_change: F) -> bool
where
    F: Fn() + Send + Sync + 'static,
{
    false
}

// ===================================================================================
// macOS — CoreAudio AudioObjectAddPropertyListener
// ===================================================================================
#[cfg(target_os = "macos")]
mod macos {
    use coreaudio_sys::{
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioObjectAddPropertyListener,
        AudioObjectID, AudioObjectPropertyAddress, AudioObjectRemovePropertyListener, OSStatus,
        UInt32,
    };
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult};

    use super::super::TRAY_MICROPHONE_WATCHER_STOPPING;

    /// Double-indirection wrapper that passes the user closure (a fat pointer) into the C
    /// callback through a single `*mut c_void`.
    /// Mirrors cpal's `PropertyListenerCallbackWrapper` pattern
    /// (cpal-0.15.3/src/host/coreaudio/macos/property_listener.rs).
    struct ListenerWrapper(Box<dyn Fn() + Send + Sync>);

    /// CoreAudio property-listener callback shim: reconstructs the user closure from the
    /// `*mut c_void` and invokes it.
    /// Mirrors cpal's `property_listener_handler_shim`.
    ///
    /// # Safety
    /// `user_data` must be the `*const ListenerWrapper` passed at registration time in
    /// `spawn`'s `AudioObjectAddPropertyListener` call, and it must stay valid for the
    /// lifetime of the listener (held by the resident thread, never freed early).
    unsafe extern "C" fn listener_shim(
        _object: AudioObjectID,
        _num_addresses: UInt32,
        _addresses: *const AudioObjectPropertyAddress,
        user_data: *mut c_void,
    ) -> OSStatus {
        // SAFETY: user_data is the &ListenerWrapper passed at registration time (see the
        // SAFETY comment below); the resident thread holds it for the listener's lifetime,
        // so this dereference is valid.
        let wrapper = &*(user_data as *const ListenerWrapper);
        (wrapper.0)();
        0
    }

    /// Registers the CoreAudio device-change listener on a dedicated thread and runs a
    /// resident CFRunLoop.
    /// Returns `true` on success (thread started and listener registered); `false` on
    /// registration failure.
    pub(super) fn spawn<F>(on_change: F) -> bool
    where
        F: Fn() + Send + Sync + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let spawn_result = std::thread::Builder::new()
            .name("openless-mic-coreaudio".into())
            .spawn(move || {
                // The wrapper must outlive the whole listener period, so it leaks/resides
                // on this thread's stack until the runloop exits.
                let wrapper = ListenerWrapper(Box::new(on_change));
                let address = AudioObjectPropertyAddress {
                    mSelector: kAudioHardwarePropertyDevices,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain,
                };

                // SAFETY: kAudioObjectSystemObject is a valid system-level AudioObjectID;
                // address points to a valid struct on this stack; listener_shim is a
                // 'static extern "C" callback; &wrapper outlives the whole runloop (until
                // this thread exits), satisfying CoreAudio's user_data lifetime
                // requirement. The return value is an OSStatus where 0 means success.
                let status: OSStatus = unsafe {
                    AudioObjectAddPropertyListener(
                        kAudioObjectSystemObject as AudioObjectID,
                        &address as *const _,
                        Some(listener_shim),
                        &wrapper as *const _ as *mut c_void,
                    )
                };

                if status != 0 {
                    log::warn!(
                        "[device_watch] AudioObjectAddPropertyListener failed: OSStatus={status}"
                    );
                    let _ = tx.send(false);
                    return;
                }
                let _ = tx.send(true);

                // Resident CFRunLoop. Instead of `CFRunLoopRun()`, rotate `run_in_mode`
                // with a short timeout, waking every 1s to check the exit flag — avoiding
                // the race of a cross-thread CFRunLoopStop and thread leaks. CoreAudio
                // callbacks are still dispatched inside run_in_mode (they belong to the
                // default mode).
                while !TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                    // SAFETY: kCFRunLoopDefaultMode is a 'static constant string provided
                    // by CoreFoundation.
                    let mode = unsafe { kCFRunLoopDefaultMode };
                    let result = CFRunLoop::run_in_mode(mode, Duration::from_secs(1), false);
                    // Finished means the runloop returned immediately (no input source).
                    // The CoreAudio listener itself installs a source on the default mode,
                    // so this normally never happens; but in extreme cases sleep briefly to
                    // avoid a busy loop, then loop back to the exit-flag check.
                    if matches!(result, CFRunLoopRunResult::Finished) {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }

                // SAFETY: same (object, address, shim, user_data) tuple as registration,
                // and the wrapper is still alive. Remove the listener before exiting so
                // CoreAudio never holds a dangling pointer.
                let remove_status: OSStatus = unsafe {
                    AudioObjectRemovePropertyListener(
                        kAudioObjectSystemObject as AudioObjectID,
                        &address as *const _,
                        Some(listener_shim),
                        &wrapper as *const _ as *mut c_void,
                    )
                };
                if remove_status != 0 {
                    log::warn!(
                        "[device_watch] AudioObjectRemovePropertyListener failed: OSStatus={remove_status}"
                    );
                }
                // The wrapper drops here — the listener is already removed, so the C side
                // will not call back anymore; safe.
                let _ = &wrapper;
            });

        if let Err(err) = spawn_result {
            log::warn!("[device_watch] spawn CoreAudio watcher thread failed: {err}");
            return false;
        }

        // Wait for the thread to report the registration result (registration is
        // synchronous and instantaneous). A crashed thread / broken channel counts as failure.
        rx.recv().unwrap_or(false)
    }
}
