//! Windows 与其他桌面平台：没有辅助功能概念，只探测麦克风拒绝状态。

use super::PermissionStatus;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::time::Duration;
#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

/// Windows 不存在 macOS 那种 Accessibility 概念。
pub fn check_accessibility() -> PermissionStatus {
    PermissionStatus::NotApplicable
}

pub fn request_accessibility() -> PermissionStatus {
    PermissionStatus::NotApplicable
}

/// Windows 的麦克风权限走系统设置 → 隐私 → 麦克风；
/// 这里用 cpal 建立一次短生命周期输入流，避免只查设备格式时误报已授权。
pub fn check_microphone() -> PermissionStatus {
    {
        if windows_microphone_registry_denied() {
            log::warn!("[mic] Windows microphone privacy registry is denied");
            return PermissionStatus::Denied;
        }

        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else {
            log::warn!("[mic] no default input device");
            return PermissionStatus::NoDevice;
        };
        let supported = match device.default_input_config() {
            Ok(config) => config,
            Err(err) => return classify_audio_probe_error(err.to_string()),
        };
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.config();
        match probe_input_stream(&device, &config, sample_format) {
            Ok(()) => PermissionStatus::Granted,
            Err(message) => classify_audio_probe_error(message),
        }
    }
}

pub fn request_microphone() -> PermissionStatus {
    check_microphone()
}

/// Lightweight check for an input device: enumerate devices only, no input stream,
/// letting dictation startup quickly tell "no mic plugged in" from "device present but denied".
pub fn has_microphone_input_device() -> bool {
    cpal::default_host().default_input_device().is_some()
}

pub fn windows_microphone_access_explicitly_denied() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_microphone_registry_denied()
    }

    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

fn classify_audio_probe_error(message: String) -> PermissionStatus {
    let lower = message.to_lowercase();
    log::warn!("[mic] input probe failed: {message}");
    if super::is_no_device_error(&lower) {
        PermissionStatus::NoDevice
    } else if lower.contains("denied")
        || lower.contains("permission")
        || lower.contains("authoriz")
        || lower.contains("access")
    {
        PermissionStatus::Denied
    } else {
        PermissionStatus::NotDetermined
    }
}

fn probe_input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Result<(), String> {
    let err_cb = |err| log::warn!("[mic] probe stream error: {err}");

    macro_rules! build_probe {
        ($t:ty) => {
            device
                .build_input_stream::<$t, _, _>(
                    config,
                    move |_data: &[$t], _info| {},
                    err_cb,
                    None,
                )
                .map_err(|e| e.to_string())
        };
    }

    let stream = match sample_format {
        SampleFormat::F32 => build_probe!(f32),
        SampleFormat::I16 => build_probe!(i16),
        SampleFormat::U16 => build_probe!(u16),
        SampleFormat::I32 => build_probe!(i32),
        SampleFormat::I8 => build_probe!(i8),
        SampleFormat::U8 => build_probe!(u8),
        other => Err(format!("unsupported sample format: {other:?}")),
    }?;

    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(120));
    drop(stream);
    Ok(())
}

fn windows_microphone_registry_denied() -> bool {
    candidate_microphone_registry_paths()
        .into_iter()
        .any(|path| registry_value_is_deny(&path))
}

fn candidate_microphone_registry_paths() -> Vec<String> {
    let mut paths = vec![
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone".to_string(),
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged".to_string(),
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone".to_string(),
    ];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(encoded) = exe.to_str().map(|path| path.replace('\\', "#")) {
            paths.push(format!(
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\{encoded}"
            ));
        }
    }

    paths
}

fn registry_value_is_deny(path: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some((root, subkey)) = path.split_once('\\') else {
            return false;
        };

        let hive = match root {
            "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
            "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
            _ => return false,
        };

        match hive.open_subkey(subkey) {
            Ok(key) => match key.get_value::<String, _>("Value") {
                Ok(value) => value.eq_ignore_ascii_case("Deny"),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}
