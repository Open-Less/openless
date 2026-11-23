//! 托盘麦克风设备变更监听（宿主薄封装）。
//!
//! 系统级通知（macOS CoreAudio `AudioObjectAddPropertyListener`）已迁到
//! `openless-platform::audio::device_watch`。宿主只负责把去抖闭包传进去，
//! 并在不支持的平台上退回慢轮询。

use tauri::AppHandle;

/// 注册系统级设备变更监听；返回是否注册成功。
///
/// `on_change` 在系统回调线程上被调用（可能并发/重复），内部负责去抖与线程派发。
#[cfg(target_os = "macos")]
pub(crate) fn spawn_native_watcher<F>(_app: AppHandle, on_change: F) -> bool
where
    F: Fn() + Send + Sync + 'static,
{
    openless_platform::audio::device_watch::spawn_native_watcher(on_change)
}

/// 其它平台：暂无本地验证过的原生通知路径，返回 `false`，由宿主的慢轮询兜底。
#[cfg(not(target_os = "macos"))]
pub(crate) fn spawn_native_watcher<F>(_app: AppHandle, _on_change: F) -> bool
where
    F: Fn() + Send + Sync + 'static,
{
    false
}

/// 进程退出时通知监听线程退出。
#[cfg(target_os = "macos")]
pub(crate) fn mark_watcher_stopping() {
    openless_platform::audio::device_watch::WATCHER_STOPPING
        .store(true, std::sync::atomic::Ordering::Relaxed);
}
