use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCheckResult {
    pub online: bool,
    pub latency_ms: Option<u64>,
}

#[tauri::command]
pub async fn check_network() -> NetworkCheckResult {
    // 探一个真实存在的接口。旧逻辑探 `/health` —— 实测返回 404，链路正常也永远判
    // 离线；且用 HEAD（后端只挂 GET）。改成 GET `/packs`，拿到任意 HTTP 响应即算通。
    //
    // 单发、不走 send_with_retry：这是每 30s 跑一次的状态探针，要的是「快」。10 次
    // 退避重试会让被过滤 / 黑洞的网络下探测拖到近一分钟、状态灯像卡死。偶发的瞬时
    // 误判由下一个 30s 周期自动纠正。仍用 net::http() 共享连接池。
    let url = format!("{MARKETPLACE_BASE_URL}/packs?limit=1");
    let start = std::time::Instant::now();
    match net::http()
        .get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
    {
        Ok(_) => NetworkCheckResult {
            online: true,
            latency_ms: Some(start.elapsed().as_millis() as u64),
        },
        Err(_) => NetworkCheckResult {
            online: false,
            latency_ms: None,
        },
    }
}

#[tauri::command]
pub fn get_hotkey_status(coord: CoordinatorState<'_>) -> HotkeyStatus {
    #[cfg(mobile)]
    {
        let _ = coord;
        return HotkeyStatus {
            adapter: crate::types::HotkeyAdapterKind::Unavailable,
            state: crate::types::HotkeyStatusState::Failed,
            message: Some("移动端不支持全局热键".into()),
            last_error: Some(crate::types::HotkeyInstallError {
                code: "unavailable".into(),
                message: "Global hotkeys are not available on mobile".into(),
            }),
        };
    }
    #[cfg(not(mobile))]
    coord.hotkey_status()
}

#[tauri::command]
pub fn get_hotkey_capability(coord: CoordinatorState<'_>) -> HotkeyCapability {
    #[cfg(mobile)]
    {
        let _ = coord;
        return HotkeyCapability::current();
    }
    #[cfg(not(mobile))]
    coord.hotkey_capability()
}

#[tauri::command]
pub fn set_shortcut_recording_active(coord: CoordinatorState<'_>, active: bool) {
    #[cfg(mobile)]
    {
        let _ = (coord, active);
        return;
    }
    #[cfg(not(mobile))]
    coord.set_shortcut_recording_active(active);
}

#[tauri::command]
#[cfg(not(mobile))]
pub fn get_windows_ime_status() -> WindowsImeStatus {
    crate::windows_ime_profile::get_windows_ime_status()
}

#[tauri::command]
#[cfg(mobile)]
pub async fn list_microphone_devices() -> Result<Vec<crate::recorder::MicrophoneDevice>, String> {
    Ok(Vec::new())
}

#[tauri::command]
#[cfg(not(mobile))]
pub async fn list_microphone_devices() -> Result<Vec<crate::recorder::MicrophoneDevice>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::recorder::list_input_devices().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("microphone device worker failed: {e}"))?
}

#[tauri::command]
#[cfg(mobile)]
pub async fn start_microphone_level_monitor(
    _app: AppHandle,
    _device_name: String,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
#[cfg(not(mobile))]
pub async fn start_microphone_level_monitor(
    app: AppHandle,
    device_name: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MicrophoneMonitorState>();
        if let Some(existing) = state.lock().take() {
            existing.stop();
        }

        let selected = device_name.trim().to_string();
        let microphone_device_name = if selected.is_empty() {
            None
        } else {
            Some(selected)
        };
        let consumer: Arc<dyn AudioConsumer> = Arc::new(LevelProbeConsumer);
        let level_app = app.clone();
        let level_handler: Arc<dyn Fn(f32) + Send + Sync> = Arc::new(move |level| {
            let _ = level_app.emit("microphone:level", serde_json::json!({ "level": level }));
        });
        let (recorder, _runtime_errors, _archive_active) =
            Recorder::start(microphone_device_name, consumer, level_handler, None)
                .map_err(|e| e.to_string())?;
        *state.lock() = Some(recorder);
        Ok(())
    })
    .await
    .map_err(|e| format!("start microphone monitor task failed: {e}"))?
}

#[tauri::command]
pub async fn stop_microphone_level_monitor(app: AppHandle) {
    #[cfg(mobile)]
    {
        let _ = app;
        return;
    }
    #[cfg(not(mobile))]
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MicrophoneMonitorState>();
        let recorder = state.lock().take();
        if let Some(recorder) = recorder {
            recorder.stop();
        }
    })
    .await;
}

/// 把当前会话的 openless.log 复制到用户选择的位置（前端用 plugin-dialog 拿 target_path）。
/// 路径来自 lib::log_dir_path() —— mac: ~/Library/Logs/OpenLess/openless.log，
/// windows: %LOCALAPPDATA%\OpenLess\Logs\openless.log。
///
/// Android 上 dialog 返回 `content://` URI，不能用 `std::fs::copy`；走 JNI
/// ContentResolver 写入，避免 tauri-plugin-fs detachFd 导致 0 字节文件。
#[tauri::command]
pub fn export_error_log(target_path: String) -> Result<(), String> {
    let src = resolve_openless_log_path()?;

    #[cfg(target_os = "android")]
    {
        if target_path.starts_with("content://") {
            let bytes = std::fs::read(&src).map_err(|e| format!("读取日志失败：{e}"))?;
            return crate::android::jni::android::write_content_uri(&target_path, &bytes)
                .map_err(|e| format!("复制日志失败：{e}"));
        }
        let path = target_path
            .strip_prefix("file://")
            .unwrap_or(target_path.as_str());
        return std::fs::copy(&src, std::path::Path::new(path))
            .map(|_| ())
            .map_err(|e| format!("复制日志失败：{e}"));
    }

    #[cfg(not(target_os = "android"))]
    {
        std::fs::copy(&src, std::path::Path::new(&target_path))
            .map(|_| ())
            .map_err(|e| format!("复制日志失败：{e}"))
    }
}

fn resolve_openless_log_path() -> Result<std::path::PathBuf, String> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "android")]
    {
        candidates.extend(crate::persistence::android_openless_log_candidates());
    }
    candidates.push(crate::log_dir_path().join("openless.log"));

    if let Some(src) = candidates.iter().find(|path| path.exists()) {
        return Ok(src.clone());
    }
    let tried = candidates
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("日志文件不存在（已尝试：{tried}）"))
}

// ─────────────────────────── cursor context (debug only) ───────────────────────────

/// 探一次「宿主 app 光标周围的正文」，把结果原样交给调用方。
///
/// **调试用，不接任何产品链路**（里程碑 1 的产物就是「模块可用但没人调它」）。
/// 存在的意义是装机之后能在各个真实 app 里挨个点一遍，肉眼确认：读到的内容对不对、
/// 终端和密码框有没有被拦住、卡死的 app 会不会把界面冻住。
///
/// `delayMs` 是这个命令能用起来的关键：从 devtools 里 invoke 时前台 app 是 OpenLess
/// 自己，读到的永远是我们自己的窗口。传个 3000 就有三秒时间切到备忘录 / VS Code /
/// 微信里点进输入框，探针在那时才真正开始读。
///
/// ```js
/// await window.__TAURI_INTERNALS__.invoke('debug_read_cursor_context', { delayMs: 3000 })
/// ```
#[tauri::command]
pub async fn debug_read_cursor_context(
    budget_chars: Option<usize>,
    delay_ms: Option<u64>,
) -> crate::host_document::HostDocumentReadResult {
    if let Some(delay) = delay_ms.filter(|ms| *ms > 0) {
        // 上限 30s：这是手动调试入口，不该能被参数拖成一个永不返回的命令。
        tokio::time::sleep(std::time::Duration::from_millis(delay.min(30_000))).await;
    }
    let budget = budget_chars
        .filter(|chars| *chars > 0)
        .unwrap_or(crate::host_document::DEFAULT_BUDGET_CHARS);

    let result = crate::host_document::probe_around_cursor(budget).await;
    // 同步打进日志：装机验证时多半是切到别的 app 手动点，回头翻日志比翻 devtools 顺手。
    log::info!(
        "[cursor-context] status={:?} reason={:?} app={:?} bundle={:?} chars={} elapsed={}ms",
        result.status,
        result.reason,
        result.app_name,
        result.bundle_id,
        result
            .window
            .as_ref()
            .map(|w| w.text.chars().count())
            .unwrap_or(0),
        result.elapsed_ms,
    );
    result
}

// ─────────────────────────── unused but exported (silences dead_code) ───────────────────────────

#[allow(dead_code)]
fn _ensure_snapshot_used(_: CredentialsSnapshot) {}
