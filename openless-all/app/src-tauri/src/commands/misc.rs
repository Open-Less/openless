use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCheckResult {
    pub online: bool,
    pub latency_ms: Option<u64>,
}

#[tauri::command]
pub async fn check_network() -> NetworkCheckResult {
    // Probe a real endpoint. The old logic probed `/health` — it actually returns 404, so the
    // status stayed "offline" even with a working link; it also used HEAD while the backend only
    // mounts GET. Now GET `/packs` with any HTTP response counts as online.
    //
    // Single shot, no send_with_retry: this status probe runs every 30s and needs to be fast. Ten
    // backoff retries would stretch the probe to nearly a minute on filtered / black-holed
    // networks, making the status light look stuck. Occasional transient misjudgments are
    // corrected automatically on the next 30s cycle. Still uses net::http()'s shared pool.
    let url = format!("{}/packs?limit=1", openless_core::MARKETPLACE_BASE_URL);
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

/// Splash-video first-launch decision: read the `splashSeenVersion` marker in preferences.json
/// and compare it with the current app major version (first segment of `CARGO_PKG_VERSION`,
/// 2.0.0-Beta.1 → "2"). On mismatch (including never written) core writes the marker back and
/// returns true, so the frontend plays the bundled splash animation once; afterwards it always
/// returns false within the same generation.
#[tauri::command]
pub fn take_splash_playback(core: CoreState<'_>) -> bool {
    let major = env!("CARGO_PKG_VERSION")
        .split('.')
        .next()
        .unwrap_or("0")
        .to_string();
    core.take_splash_playback(&major)
}

#[tauri::command]
pub async fn get_hotkey_status(core: CoreState<'_>) -> Result<HotkeyStatus, String> {
    Ok(core
        .services()
        .platform
        .hotkey_status()
        .await
        .unwrap_or_else(|error| HotkeyStatus {
            adapter: crate::types::HotkeyAdapterKind::Unavailable,
            state: crate::types::HotkeyStatusState::Failed,
            message: Some(error.message.clone()),
            last_error: Some(crate::types::HotkeyInstallError {
                code: format!("{:?}", error.code).to_ascii_lowercase(),
                message: error.message,
            }),
        }))
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
pub async fn list_microphone_devices(
    core: CoreState<'_>,
) -> Result<Vec<crate::recorder::MicrophoneDevice>, String> {
    core.services()
        .platform
        .microphone_devices()
        .await
        .map(|devices| {
            devices
                .into_iter()
                .map(|device| crate::recorder::MicrophoneDevice {
                    name: device.name,
                    is_default: device.is_default,
                })
                .collect()
        })
        .map_err(|error| error.message)
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

/// Copy the current session's openless.log to a user-chosen location (the frontend obtains
/// target_path via plugin-dialog). The path comes from lib::log_dir_path() —
/// mac: ~/Library/Logs/OpenLess/openless.log,
/// windows: %LOCALAPPDATA%\OpenLess\Logs\openless.log.
///
/// On Android the dialog returns a `content://` URI which `std::fs::copy` cannot handle; write
/// through the JNI ContentResolver instead, avoiding the 0-byte files caused by
/// tauri-plugin-fs's detachFd.
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

/// Android：直接把当前会话日志写入公共 Downloads，绕过部分 ROM 上
/// 无法正常弹出的 CREATE_DOCUMENT / SAF 保存对话框。
#[cfg(target_os = "android")]
#[tauri::command]
pub fn export_error_log_to_downloads(file_name: String) -> Result<String, String> {
    let src = resolve_openless_log_path()?;
    let bytes = std::fs::read(&src).map_err(|e| format!("读取日志失败：{e}"))?;
    crate::android::jni::android::write_public_download(&file_name, &bytes)
        .map_err(|e| format!("导出日志失败：{e}"))
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

/// Probe "the body text around the host app's cursor" once and hand the result to the caller.
///
/// **Debug only, wired into no product path** (milestone 1 delivered "module works but nobody
/// calls it"). It exists so that, after install, one can click through real apps one by one and
/// visually confirm: the content reads correctly, terminals and password fields are blocked, and
/// a hung app does not freeze the UI.
///
/// `delayMs` is what makes this command usable: when invoked from devtools the foreground app is
/// OpenLess itself, so it would always read our own window. Pass 3000 and there are three seconds
/// to switch to Notes / VS Code / WeChat and click into an input field before the probe actually
/// reads.
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
        // Cap at 30s: this is a manual debug entry point and must not be dragged into a
        // never-returning command by arguments.
        tokio::time::sleep(std::time::Duration::from_millis(delay.min(30_000))).await;
    }
    let budget = budget_chars
        .filter(|chars| *chars > 0)
        .unwrap_or(crate::host_document::DEFAULT_BUDGET_CHARS);

    let result = crate::host_document::probe_around_cursor(budget).await;
    // Also log it: install verification usually involves switching to another app and clicking
    // manually, and digging through the log is easier than digging through devtools.
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
