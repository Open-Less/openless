use super::*;

#[tauri::command]
pub async fn get_platform_capabilities(
    core: CoreState<'_>,
) -> Result<crate::types::PlatformCapabilities, String> {
    Ok(core
        .services()
        .platform
        .capabilities()
        .await
        .unwrap_or_default())
}

#[tauri::command]
pub fn get_android_overlay_status() -> AndroidOverlayStatus {
    crate::android::get_android_overlay_status()
}

#[tauri::command]
pub fn request_android_overlay_permission() -> crate::android::AndroidOverlayPermissionResult {
    crate::android::request_android_overlay_permission()
}

#[tauri::command]
pub fn show_android_overlay() -> Result<(), String> {
    crate::android::show_android_overlay()
}

#[tauri::command]
pub fn hide_android_overlay() -> Result<(), String> {
    crate::android::hide_android_overlay()
}

#[tauri::command]
pub fn get_android_accessibility_status() -> AndroidAccessibilityStatus {
    crate::android::get_android_accessibility_status()
}

#[tauri::command]
pub fn request_android_accessibility_permission(
) -> crate::android::AndroidAccessibilityPermissionResult {
    crate::android::request_android_accessibility_permission()
}

#[tauri::command]
pub fn get_android_shizuku_status() -> AndroidShizukuStatus {
    crate::android::get_android_shizuku_status()
}

#[tauri::command]
pub fn request_android_shizuku_permission() -> crate::android::AndroidShizukuPermissionResult {
    crate::android::request_android_shizuku_permission()
}

#[tauri::command]
pub fn open_shizuku_app() -> crate::android::AndroidShizukuOpenResult {
    crate::android::open_shizuku_app()
}

#[tauri::command]
pub async fn recover_android_accessibility(confirmed: bool) -> AndroidAccessibilityRecoveryResult {
    tokio::task::spawn_blocking(move || crate::android::recover_android_accessibility(confirmed))
        .await
        .unwrap_or(AndroidAccessibilityRecoveryResult {
            outcome: AndroidAccessibilityRecoveryOutcome::ShellFailed,
            message: String::new(),
            message_key: "internal_error".to_string(),
        })
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    crate::external_url::open_external_url(&url)
}

#[tauri::command]
pub async fn check_accessibility_permission(
    core: CoreState<'_>,
) -> Result<PermissionStatus, String> {
    Ok(core
        .services()
        .platform
        .accessibility_permission()
        .await
        .map(|snapshot| map_core_permission(snapshot.accessibility))
        .unwrap_or(PermissionStatus::NotApplicable))
}

#[tauri::command]
pub async fn request_accessibility_permission(
    core: CoreState<'_>,
) -> Result<PermissionStatus, String> {
    if let Err(error) = core
        .services()
        .platform
        .request_accessibility_permission()
        .await
    {
        log::warn!("request accessibility permission through core failed: {error}");
        return Ok(PermissionStatus::NotApplicable);
    }
    check_accessibility_permission(core).await
}

#[tauri::command]
pub async fn check_microphone_permission(core: CoreState<'_>) -> Result<PermissionStatus, String> {
    Ok(core
        .services()
        .platform
        .microphone_permission()
        .await
        .map(|snapshot| map_core_permission(snapshot.microphone))
        .unwrap_or(PermissionStatus::NotApplicable))
}

#[tauri::command]
pub async fn request_microphone_permission(
    core: CoreState<'_>,
) -> Result<PermissionStatus, String> {
    if let Err(error) = core
        .services()
        .platform
        .request_microphone_permission()
        .await
    {
        log::warn!("request microphone permission through core failed: {error}");
        return Ok(PermissionStatus::NotApplicable);
    }
    check_microphone_permission(core).await
}

fn map_core_permission(state: openless_core::PermissionState) -> PermissionStatus {
    match state {
        openless_core::PermissionState::Unknown => PermissionStatus::NotDetermined,
        openless_core::PermissionState::Granted => PermissionStatus::Granted,
        openless_core::PermissionState::Denied => PermissionStatus::Denied,
        openless_core::PermissionState::Restricted => PermissionStatus::Restricted,
        openless_core::PermissionState::NoDevice => PermissionStatus::NoDevice,
        openless_core::PermissionState::Unsupported => PermissionStatus::NotApplicable,
    }
}

/// 跳到 macOS 系统设置的指定隐私面板。pane: "accessibility" | "microphone".
#[tauri::command]
pub fn open_system_settings(pane: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let url = match pane.as_str() {
            "accessibility" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            "microphone" => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy",
        };
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        fn wide_null(value: &str) -> Vec<u16> {
            value.encode_utf16().chain(std::iter::once(0)).collect()
        }

        let uri = match pane.as_str() {
            "microphone" => "ms-settings:privacy-microphone",
            "sound" => "ms-settings:sound",
            "accessibility" => "ms-settings:easeofaccess",
            _ => "ms-settings:",
        };

        let operation = wide_null("open");
        let target = wide_null(uri);
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };

        if result.0 as isize <= 32 {
            Err(format!("ShellExecuteW failed: {}", result.0 as isize))
        } else {
            Ok(())
        }
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let _ = pane;
        Err("open_system_settings is only supported on macOS and Windows".to_string())
    }
}

/// 触发 macOS 系统弹"是否允许 OpenLess 访问麦克风"对话框。
/// 与 Swift `MicrophonePermission.request()` 同语义：只信系统权限回调，
/// 不用 cpal stream 成功与否伪造授权状态。
#[tauri::command]
pub async fn trigger_microphone_prompt(core: CoreState<'_>) -> Result<(), String> {
    let status = request_microphone_permission(core).await?;
    if matches!(
        status,
        PermissionStatus::Granted | PermissionStatus::NotApplicable
    ) {
        Ok(())
    } else {
        Err(format!("microphone permission is {status:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_permission_states_preserve_the_legacy_ipc_contract() {
        let cases = [
            (
                openless_core::PermissionState::Unknown,
                PermissionStatus::NotDetermined,
            ),
            (
                openless_core::PermissionState::Granted,
                PermissionStatus::Granted,
            ),
            (
                openless_core::PermissionState::Denied,
                PermissionStatus::Denied,
            ),
            (
                openless_core::PermissionState::Restricted,
                PermissionStatus::Restricted,
            ),
            (
                openless_core::PermissionState::NoDevice,
                PermissionStatus::NoDevice,
            ),
            (
                openless_core::PermissionState::Unsupported,
                PermissionStatus::NotApplicable,
            ),
        ];

        for (core, legacy) in cases {
            assert_eq!(map_core_permission(core), legacy);
        }
    }
}
