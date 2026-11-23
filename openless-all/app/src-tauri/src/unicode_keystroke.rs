//! macOS 流式逐字输入（宿主薄封装）。
//!
//! CGEvent 逐字投递、TIS 输入源切换、Secure Input 检测都在平台层
//! （`openless-platform::macos::keystroke`）。这里只保留**必须在宿主做**的部分：
//! 把同步原语派发到 AppKit 主线程，并提供 async 包装给内核调用。

#[cfg(target_os = "macos")]
pub use openless_platform::macos::keystroke::{
    is_secure_input_enabled, type_unicode_chunk, type_unicode_chunk_with_options, PreviousInputSource, TisError,
    TypeError,
};

#[cfg(target_os = "macos")]
use std::time::Duration;
#[cfg(target_os = "macos")]
use tauri::{AppHandle, Runtime};

/// 安装/移除"每批输入前重新确认 ABC 输入源"的守卫。
///
/// 守卫的检查必须在主线程做（TIS 要求），所以闭包在宿主构造：
/// `run_on_main_thread` 是 tauri 的能力，平台层不链接它。
#[cfg(target_os = "macos")]
pub fn set_streaming_ascii_guard<R: Runtime>(app: Option<AppHandle<R>>) {
    use std::sync::Arc;

    let guard: Option<Arc<dyn Fn() + Send + Sync>> = app.map(|app| {
        Arc::new(move || ensure_ascii_blocking(&app)) as Arc<dyn Fn() + Send + Sync>
    });
    openless_platform::macos::keystroke::install_ascii_guard(guard);
}

/// 在主线程确认输入源是 ABC，最多等 250ms。
///
/// 从不失败写入：超时只是让这一批跳过守卫。
#[cfg(target_os = "macos")]
fn ensure_ascii_blocking<R: Runtime>(app: &AppHandle<R>) {
    let (tx, rx) = std::sync::mpsc::channel();
    if app
        .run_on_main_thread(move || {
            let _ = tx.send(openless_platform::macos::keystroke::ensure_ascii_on_main_blocking());
        })
        .is_err()
    {
        return;
    }
    if let Ok(true) = rx.recv_timeout(Duration::from_millis(250)) {
        // 给目标应用的文本输入上下文一点时间采纳新输入源。
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// 切到 ABC 输入源（流式输入开始时调用）。
#[cfg(target_os = "macos")]
pub async fn switch_to_ascii<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<PreviousInputSource>, TisError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = openless_platform::macos::keystroke::switch_to_ascii_blocking();
        let _ = tx.send(result);
    })
    .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?;
    rx.await
        .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?
}

/// 恢复之前的输入源（流式输入结束时调用）。
#[cfg(target_os = "macos")]
pub async fn restore_input_source<R: Runtime>(
    app: &AppHandle<R>,
    prev: Option<PreviousInputSource>,
) -> Result<(), TisError> {
    if prev.is_none() {
        return Ok(());
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = openless_platform::macos::keystroke::restore_input_source_blocking(prev);
        let _ = tx.send(result);
    })
    .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?;
    rx.await
        .map_err(|e| TisError::MainThreadDispatch(e.to_string()))?
}

/// 非 macOS 目标：显式"不支持"，不静默成功。
#[cfg(not(target_os = "macos"))]
pub fn type_unicode_chunk(_text: &str) -> Result<usize, TypeError> {
    Err(TypeError::Unsupported)
}

#[cfg(not(target_os = "macos"))]
pub fn is_secure_input_enabled() -> bool {
    false
}
