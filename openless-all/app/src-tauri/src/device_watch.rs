//! 托盘麦克风设备变更的 OS 原生监听（issue #470）。
//!
//! 目的：替代「每 10s 轮询 `list_input_devices()`」这条空闲唤醒。改用各平台原生的设备
//! 变更通知，空闲时零唤醒，设备插拔/默认设备切换时由 OS 回调实时触发刷新。
//!
//! 严格按平台分流：
//! - macOS：CoreAudio `AudioObjectAddPropertyListener` 监听
//!   `kAudioHardwarePropertyDevices`，专用线程跑 `CFRunLoop` 常驻。
//! - Windows：`IMMNotificationClient` 经 `IMMDeviceEnumerator` 注册端点通知回调。
//! - Linux：无原生路径，返回 `false`，由 `lib.rs` 的慢速兜底轮询负责。
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

/// Windows：经 MMDevice 端点通知注册。见模块内说明。
#[cfg(target_os = "windows")]
pub(crate) fn spawn_native_watcher<F>(_app: AppHandle, on_change: F) -> bool
where
    F: Fn() + Send + Sync + 'static,
{
    windows_impl::spawn(on_change)
}

/// Linux：无原生路径，纯靠 `lib.rs` 的慢速兜底轮询。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

// ===================================================================================
// Windows — IMMNotificationClient via IMMDeviceEnumerator
// ===================================================================================
//
// 注意：开发机是 macOS，本分支无法本地编译验证，正确性靠 CI + 真机。严格 cfg-gate，
// 写法保守稳妥。关键约束：
//   1. enumerator 必须在监听期间一直存活，否则回调停止——故由常驻线程持有，退出时才
//      `UnregisterEndpointNotificationCallback` + drop。
//   2. COM 在本线程 `CoInitializeEx(MULTITHREADED)`，退出时 `CoUninitialize`。
//   3. 只关心 capture（输入）设备的增删/状态/默认变化；render 设备变化忽略。回调里只调
//      `on_change`（其内部去抖会过滤掉无关变化），故即使偶尔多触发也只是多一次零副作用
//      的签名比较。
#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use windows::core::Result as WinResult;
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{
        eCapture, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

    use super::super::TRAY_MICROPHONE_WATCHER_STOPPING;

    /// IMMNotificationClient 实现：相关回调都转交 `on_change`（其内部去抖）。
    /// 拥有 `on_change` 的所有权（'static Box），故 client 自包含、不借用外部生命周期。
    #[windows::core::implement(IMMNotificationClient)]
    struct DeviceNotificationClient {
        on_change: Box<dyn Fn() + Send + Sync>,
    }

    impl DeviceNotificationClient {
        fn notify(&self) {
            (self.on_change)();
        }
    }

    // IMMNotificationClient 的全部方法都必须实现（COM 接口契约）。
    #[allow(non_snake_case)]
    impl IMMNotificationClient_Impl for DeviceNotificationClient_Impl {
        fn OnDeviceStateChanged(
            &self,
            _pwstrDeviceId: &PCWSTR,
            _dwNewState: DEVICE_STATE,
        ) -> WinResult<()> {
            // 设备启用/禁用/拔出。无法在此廉价判断 data-flow，统一交给签名去抖过滤。
            self.notify();
            Ok(())
        }

        fn OnDeviceAdded(&self, _pwstrDeviceId: &PCWSTR) -> WinResult<()> {
            self.notify();
            Ok(())
        }

        fn OnDeviceRemoved(&self, _pwstrDeviceId: &PCWSTR) -> WinResult<()> {
            self.notify();
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            _role: ERole,
            _pwstrDefaultDeviceId: &PCWSTR,
        ) -> WinResult<()> {
            // 默认设备变化：只关心 capture（输入）。render 变化忽略，减少无谓刷新。
            if flow == eCapture {
                self.notify();
            }
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _pwstrDeviceId: &PCWSTR,
            _key: &PROPERTYKEY,
        ) -> WinResult<()> {
            // 属性变化噪声大（音量等），不触发刷新。
            Ok(())
        }
    }

    /// 在专用线程注册 MMDevice 端点通知并常驻直到退出 flag 置位。
    /// 成功返回 `true`；COM/注册失败返回 `false`。
    pub(super) fn spawn<F>(on_change: F) -> bool
    where
        F: Fn() + Send + Sync + 'static,
    {
        // 闭包提前装箱：client 直接拥有它，全程不借用外部生命周期，无裸指针 hack。
        let on_change: Box<dyn Fn() + Send + Sync> = Box::new(on_change);
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let spawn_result = std::thread::Builder::new()
            .name("openless-mic-mmdevice".into())
            .spawn(move || {
                // SAFETY: 在本专用线程初始化 COM（MTA）。失败则不注册，直接报失败退出。
                let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                if hr.is_err() {
                    log::warn!("[device_watch] CoInitializeEx failed: {hr:?}");
                    let _ = tx.send(false);
                    return;
                }

                let registered = register_and_run(on_change);
                let _ = tx.send(registered);

                // SAFETY: 与上面的 CoInitializeEx 配对。register_and_run 返回时所有 COM 对象
                // 已 drop（enumerator/client 在其作用域内），此处可安全反初始化 COM。
                unsafe { CoUninitialize() };
            });

        if let Err(err) = spawn_result {
            log::warn!("[device_watch] spawn MMDevice watcher thread failed: {err}");
            return false;
        }
        rx.recv().unwrap_or(false)
    }

    /// 创建 enumerator、注册回调、常驻轮转检查退出 flag、退出前注销。注册成功返回 `true`。
    ///
    /// 关键：本函数返回前 `enumerator` 与 `client` 一直在作用域内存活 → 满足「监听期间
    /// enumerator 必须存活，否则回调停」。`client` 拥有 `on_change` 的所有权，自包含。
    fn register_and_run(on_change: Box<dyn Fn() + Send + Sync>) -> bool {
        // SAFETY: 标准 MMDevice enumerator 实例化。CLSCTX_ALL 允许进程内/外服务器。
        let enumerator: IMMDeviceEnumerator =
            match unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) } {
                Ok(e) => e,
                Err(err) => {
                    log::warn!(
                        "[device_watch] CoCreateInstance(MMDeviceEnumerator) failed: {err:?}"
                    );
                    return false;
                }
            };

        let client: IMMNotificationClient = DeviceNotificationClient { on_change }.into();

        // SAFETY: enumerator 有效；client 是合法 IMMNotificationClient。注册后系统持有 client
        // 的引用计数，回调将从 MMDevice 线程进入。client 拥有 on_change，生命周期自洽。
        if let Err(err) = unsafe { enumerator.RegisterEndpointNotificationCallback(&client) } {
            log::warn!("[device_watch] RegisterEndpointNotificationCallback failed: {err:?}");
            return false;
        }

        // 常驻：MMDevice 回调由系统线程驱动，本线程只需保持 enumerator/client 存活并周期
        // 检查退出 flag。1s 一片，退出最多 1s 内生效。
        while !TRAY_MICROPHONE_WATCHER_STOPPING.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
        }

        // SAFETY: 注销同一 client。enumerator 仍有效。注销后系统不再回调。
        if let Err(err) = unsafe { enumerator.UnregisterEndpointNotificationCallback(&client) } {
            log::warn!("[device_watch] UnregisterEndpointNotificationCallback failed: {err:?}");
        }
        // enumerator 与 client 在此 drop（COM 引用计数归还，on_change 随 client 释放）。
        true
    }
}
