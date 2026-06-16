//! 托盘麦克风设备变更的 OS 原生监听（issue #470）。
//!
//! 目的：替代「每 10s 轮询 `list_input_devices()`」这条空闲唤醒。改用各平台原生的设备
//! 变更通知，空闲时零唤醒，设备插拔/默认设备切换时由 OS 回调实时触发刷新。
//!
//! 平台分流：
//! - macOS：CoreAudio `AudioObjectAddPropertyListener` 监听
//!   `kAudioHardwarePropertyDevices`，专用线程跑 `CFRunLoop` 常驻。
//! - Windows / Linux：暂返回 `false`，由 `lib.rs` 的 60s 慢速兜底轮询负责。
//!   （Windows 原生 `IMMNotificationClient` 通知留作后续，需 Windows 开发机验证。）
//!
//! 三平台共用契约：`spawn_native_watcher(app, on_change)`。`on_change` 是 `lib.rs` 提供
//! 的去抖闭包（内部复用 `microphone_device_signature()`，真正变化才刷新+emit）。回调里
//! 只调用 `on_change`，不做别的。注册失败一律返回 `false`（只 warn 不 panic），交由
//! 兜底轮询兜底，保证三平台都「永远能检测到设备」。

use tauri::AppHandle;

/// 注册 OS 原生设备变更监听。成功返回 `true`，平台不支持或注册失败返回 `false`。
///
/// `on_change` 在 OS 回调线程上被调用（可能并发/重复），其内部负责去抖与线程派发。
#[cfg(target_os = "macos")]
pub(crate) fn spawn_native_watcher<F>(_app: AppHandle, on_change: F) -> bool
where
    F: Fn() + Send + Sync + 'static,
{
    macos::spawn(on_change)
}

/// 非 macOS（Windows / Linux）：暂无本地验证过的原生路径，返回 `false`，纯靠 `lib.rs`
/// 的 60s 慢速兜底轮询。Windows 原生 `IMMNotificationClient` 留作后续（需 Windows 开发机验证）。
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

    /// 把用户闭包（胖指针）经单个 `*mut c_void` 传进 C 回调的双重间接封装。
    /// 照抄 cpal 的 `PropertyListenerCallbackWrapper` 模式
    /// (cpal-0.15.3/src/host/coreaudio/macos/property_listener.rs)。
    struct ListenerWrapper(Box<dyn Fn() + Send + Sync>);

    /// CoreAudio 属性监听回调 shim：把 `*mut c_void` 还原成用户闭包并调用。
    /// 照抄 cpal 的 `property_listener_handler_shim`。
    ///
    /// # Safety
    /// `user_data` 必须是 `spawn` 里 `AudioObjectAddPropertyListener` 注册时传入、且在监听
    /// 存活期间一直有效的 `*const ListenerWrapper`（由常驻线程持有，不会提前释放）。
    unsafe extern "C" fn listener_shim(
        _object: AudioObjectID,
        _num_addresses: UInt32,
        _addresses: *const AudioObjectPropertyAddress,
        user_data: *mut c_void,
    ) -> OSStatus {
        // SAFETY: user_data 是注册时传入的 &ListenerWrapper（见下方 SAFETY 注释），
        // 监听存活期间常驻线程一直持有它，故此处解引用有效。
        let wrapper = &*(user_data as *const ListenerWrapper);
        (wrapper.0)();
        0
    }

    /// 在专用线程注册 CoreAudio 设备变更监听并跑 CFRunLoop 常驻。
    /// 成功返回 `true`（线程已起且监听已注册）；注册失败返回 `false`。
    pub(super) fn spawn<F>(on_change: F) -> bool
    where
        F: Fn() + Send + Sync + 'static,
    {
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let spawn_result = std::thread::Builder::new()
            .name("openless-mic-coreaudio".into())
            .spawn(move || {
                // wrapper 必须活过整个监听期，故 leak/常驻在本线程栈上直到 runloop 退出。
                let wrapper = ListenerWrapper(Box::new(on_change));
                let address = AudioObjectPropertyAddress {
                    mSelector: kAudioHardwarePropertyDevices,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain,
                };

                // SAFETY: kAudioObjectSystemObject 是合法的系统级 AudioObjectID；address 指向
                // 本栈上有效结构；listener_shim 是 'static extern "C" 回调；&wrapper 在整个
                // runloop 期间存活（直到本线程退出），满足 CoreAudio 对 user_data 生命周期的
                // 要求。返回值是 OSStatus，0 表示成功。
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

                // CFRunLoop 常驻。用 `run_in_mode` 短超时轮转代替 `CFRunLoopRun()`，
                // 每 1s 醒来一次检查退出 flag——避免跨线程 CFRunLoopStop 的竞态与线程泄漏。
                // CoreAudio 回调照常在 run_in_mode 内被派发（属于 default mode）。
                while !TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
                    // SAFETY: kCFRunLoopDefaultMode 是 CoreFoundation 提供的 'static 常量字符串。
                    let mode = unsafe { kCFRunLoopDefaultMode };
                    let result = CFRunLoop::run_in_mode(mode, Duration::from_secs(1), false);
                    // Finished 表示 runloop 立即返回（没有任何 input source）。CoreAudio 监听
                    // 本身会给 default mode 装上 source，正常不会走到这里；但极端情况下用一小段
                    // sleep 避免空转 busy loop，再回到顶部按退出 flag 判断。
                    if matches!(result, CFRunLoopRunResult::Finished) {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }

                // SAFETY: 与注册时同一组 (object, address, shim, user_data)，且 wrapper 仍存活。
                // 退出前移除监听，避免 CoreAudio 持有悬垂指针。
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
                // wrapper 在此 drop——此时监听已移除，C 侧不再回调，安全。
                let _ = &wrapper;
            });

        if let Err(err) = spawn_result {
            log::warn!("[device_watch] spawn CoreAudio watcher thread failed: {err}");
            return false;
        }

        // 等线程报告注册结果（注册是同步的、瞬时的）。线程崩溃/通道断开按失败处理。
        rx.recv().unwrap_or(false)
    }
}

