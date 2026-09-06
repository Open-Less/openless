use futures_util::future::BoxFuture;
use openless_core::domains::{MicrophoneDevice, PlatformApi};
use openless_core::shared_types::{HotkeyAdapterKind, HotkeyStatusState};
use openless_core::{
    BackendError, BackendErrorCode, HotkeyStatus, PermissionSnapshot, PermissionState,
    PlatformCapabilities,
};

use crate::{fcitx5_available, LinuxPackageKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDesktopSession {
    X11,
    Wayland,
    Headless,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxCapabilitySnapshot {
    pub session: LinuxDesktopSession,
    pub fcitx5_ready: bool,
    pub capabilities: PlatformCapabilities,
    pub permissions: PermissionSnapshot,
}

impl LinuxCapabilitySnapshot {
    pub fn from_environment(
        wayland_display: Option<&str>,
        x11_display: Option<&str>,
        fcitx5_ready: bool,
        tray_available: bool,
        package_kind: LinuxPackageKind,
    ) -> Self {
        let session = if wayland_display.is_some_and(|value| !value.trim().is_empty()) {
            LinuxDesktopSession::Wayland
        } else if x11_display.is_some_and(|value| !value.trim().is_empty()) {
            LinuxDesktopSession::X11
        } else {
            LinuxDesktopSession::Headless
        };
        let desktop = session != LinuxDesktopSession::Headless;
        Self {
            session,
            fcitx5_ready,
            capabilities: PlatformCapabilities {
                platform: "linux".into(),
                supports_desktop_hotkey: desktop && fcitx5_ready,
                supports_tray: desktop && tray_available,
                supports_overlay: session == LinuxDesktopSession::X11,
                supports_ime_input: desktop && fcitx5_ready,
                supports_local_asr: desktop,
                supports_local_qwen3_mlx: false,
                supports_in_app_dictation: false,
                supports_auto_update: package_kind == LinuxPackageKind::AppImage,
            },
            permissions: PermissionSnapshot {
                microphone: if desktop {
                    PermissionState::Unknown
                } else {
                    PermissionState::Unsupported
                },
                accessibility: PermissionState::Unsupported,
            },
        }
    }

    pub fn detect(tray_available: bool, package_kind: LinuxPackageKind) -> Self {
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let x11 = std::env::var("DISPLAY").ok();
        Self::from_environment(
            wayland.as_deref(),
            x11.as_deref(),
            fcitx5_available(),
            tray_available,
            package_kind,
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct LinuxPlatformApi {
    capabilities: PlatformCapabilities,
}

impl LinuxPlatformApi {
    pub fn new(capabilities: PlatformCapabilities) -> Self {
        Self { capabilities }
    }
}

impl PlatformApi for LinuxPlatformApi {
    fn capabilities(&self) -> BoxFuture<'static, Result<PlatformCapabilities, BackendError>> {
        let capabilities = self.capabilities.clone();
        Box::pin(async move { Ok(capabilities) })
    }

    fn microphone_devices(
        &self,
    ) -> BoxFuture<'static, Result<Vec<MicrophoneDevice>, BackendError>> {
        Box::pin(async {
            #[cfg(target_os = "linux")]
            {
                tokio::task::spawn_blocking(enumerate_microphones)
                    .await
                    .map_err(|error| {
                        BackendError::new(
                            BackendErrorCode::Internal,
                            format!("microphone enumeration task failed: {error}"),
                        )
                    })?
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(BackendError::new(
                    BackendErrorCode::Unsupported,
                    "Linux microphone enumeration is unavailable on this target",
                ))
            }
        })
    }

    fn microphone_permission(
        &self,
    ) -> BoxFuture<'static, Result<PermissionSnapshot, BackendError>> {
        Box::pin(async {
            Ok(PermissionSnapshot {
                microphone: PermissionState::Unknown,
                accessibility: PermissionState::Unsupported,
            })
        })
    }

    fn accessibility_permission(
        &self,
    ) -> BoxFuture<'static, Result<PermissionSnapshot, BackendError>> {
        Box::pin(async {
            Ok(PermissionSnapshot {
                microphone: PermissionState::Unknown,
                accessibility: PermissionState::Unsupported,
            })
        })
    }

    fn request_microphone_permission(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "Linux microphone permission is managed by the desktop audio portal",
            ))
        })
    }

    fn request_accessibility_permission(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async {
            Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "Linux does not expose the macOS accessibility permission flow",
            ))
        })
    }

    fn hotkey_status(&self) -> BoxFuture<'static, Result<HotkeyStatus, BackendError>> {
        Box::pin(async {
            #[cfg(target_os = "linux")]
            let ready = tokio::task::spawn_blocking(fcitx5_available)
                .await
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Internal,
                        format!("fcitx5 probe task failed: {error}"),
                    )
                })?;
            #[cfg(not(target_os = "linux"))]
            let ready = false;
            Ok(HotkeyStatus {
                adapter: if ready {
                    HotkeyAdapterKind::Fcitx5
                } else {
                    HotkeyAdapterKind::Unavailable
                },
                state: if ready {
                    HotkeyStatusState::Installed
                } else {
                    HotkeyStatusState::Failed
                },
                message: (!ready).then(|| "fcitx5 OpenLess plugin is unavailable".into()),
                last_error: None,
            })
        })
    }
}

#[cfg(target_os = "linux")]
fn enumerate_microphones() -> Result<Vec<MicrophoneDevice>, BackendError> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let devices = host.input_devices().map_err(|error| {
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to enumerate Linux microphones: {error}"),
        )
    })?;
    devices
        .enumerate()
        .map(|(index, device)| {
            let name = device.name().map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("failed to read Linux microphone name: {error}"),
                )
            })?;
            Ok(MicrophoneDevice {
                id: format!("cpal:{index}:{name}"),
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_and_wayland_have_explicitly_different_overlay_capabilities() {
        let x11 = LinuxCapabilitySnapshot::from_environment(
            None,
            Some(":0"),
            true,
            true,
            LinuxPackageKind::AppImage,
        );
        assert_eq!(x11.session, LinuxDesktopSession::X11);
        assert!(x11.capabilities.supports_overlay);
        assert!(x11.capabilities.supports_auto_update);

        let wayland = LinuxCapabilitySnapshot::from_environment(
            Some("wayland-0"),
            Some(":0"),
            false,
            false,
            LinuxPackageKind::SystemPackage,
        );
        assert_eq!(wayland.session, LinuxDesktopSession::Wayland);
        assert!(!wayland.capabilities.supports_overlay);
        assert!(!wayland.capabilities.supports_desktop_hotkey);
        assert!(!wayland.capabilities.supports_auto_update);
    }

    #[test]
    fn headless_session_does_not_claim_desktop_or_microphone_support() {
        let snapshot = LinuxCapabilitySnapshot::from_environment(
            None,
            None,
            false,
            false,
            LinuxPackageKind::Development,
        );
        assert_eq!(snapshot.session, LinuxDesktopSession::Headless);
        assert!(!snapshot.capabilities.supports_local_asr);
        assert_eq!(
            snapshot.permissions.microphone,
            PermissionState::Unsupported
        );
    }
}
