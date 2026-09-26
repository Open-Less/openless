use futures_util::future::BoxFuture;
use openless_core::domains::{MicrophoneDevice, PlatformApi};
use openless_core::shared_types::{HotkeyAdapterKind, HotkeyStatusState};
use openless_core::{
    BackendError, BackendErrorCode, HotkeyStatus, PermissionSnapshot, PermissionState,
    PlatformCapabilities,
};

use crate::fcitx5_available;

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
                // Linux ships no local inference engine (Generic/Qwen, MLX or
                // Foundry). Report false on every desktop session so the UI and
                // downstream gate on the honest answer.
                supports_local_asr: false,
                supports_local_qwen3_mlx: false,
                supports_in_app_dictation: false,
                // Linux ships deb/rpm only (no AppImage, no updater manifest),
                // so the host can never replace its own package. Always false —
                // the UI gates every update control on it.
                supports_auto_update: false,
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

    pub fn detect(tray_available: bool) -> Self {
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let x11 = std::env::var("DISPLAY").ok();
        Self::from_environment(
            wayland.as_deref(),
            x11.as_deref(),
            fcitx5_available(),
            tray_available,
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
    let default_name = host.default_input_device().map(|device| device.to_string());
    let devices = host.input_devices().map_err(|error| {
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to enumerate Linux microphones: {error}"),
        )
    })?;
    let mut out: Vec<MicrophoneDevice> = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device.to_string();
        let channels = device
            .default_input_config()
            .map(|config| config.channels())
            .unwrap_or(0);
        if !selectable_microphone(&name, channels) {
            continue;
        }
        // 同一个设备会以多条节点出现（例如「内置音频 Pro」出现两次）。
        if out.iter().any(|existing| existing.name == name) {
            continue;
        }
        out.push(MicrophoneDevice {
            id: format!("cpal:{index}:{name}"),
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
        });
    }
    Ok(out)
}

/// cpal 的 PipeWire 后端把**所有节点**都当成输入设备：输出监听（`sink-*`、
/// `*.monitor`）、合成默认节点、以及 HDMI/环绕监听都会出现在 `input_devices()` 里，
/// 同名设备还会重复出现。原样透传会让设置页的「首选麦克风」变成一个装满噪声的
/// 下拉框（用户反馈「没有首选麦克风功能」），所以这里按可识别特征过滤：
///
/// * 合成节点（`default_input` / `default_sink` / `unknown`）——界面里已经有
///   「系统默认」这一项；
/// * PulseAudio/PipeWire 的 sink 与 monitor 命名；
/// * 声道数 > 2 的节点（HDMI 与环绕监听），真实麦克风是 1 或 2 声道。取不到配置时
///   保留（0 声道），宁可多列也不误删用户真正的麦克风。
#[cfg(target_os = "linux")]
fn selectable_microphone(name: &str, channels: u16) -> bool {
    let lowered = name.trim().to_ascii_lowercase();
    if lowered.is_empty() || lowered == "unknown" {
        return false;
    }
    if lowered == "default_input" || lowered == "default_sink" {
        return false;
    }
    if lowered.starts_with("sink-") || lowered.starts_with("sink_") || lowered.ends_with(".monitor")
    {
        return false;
    }
    channels <= 2
}

#[cfg(all(test, target_os = "linux"))]
mod microphone_filter_tests {
    use super::selectable_microphone;

    #[test]
    fn non_capture_nodes_are_not_offered_as_microphones() {
        // 实测（本机 PipeWire）：这 5 个都出现在 cpal 的 input_devices() 里。
        for junk in [
            "default_sink",
            "default_input",
            "sink-sunshine-stereo",
            "sink-sunshine-surround71",
            "unknown",
        ] {
            assert!(
                !selectable_microphone(junk, 2),
                "{junk} is not a selectable microphone"
            );
        }
        // HDMI/环绕监听是 6/8 声道。
        assert!(!selectable_microphone(
            "TU116 High Definition Audio Controller Pro 9",
            8
        ));
        assert!(!selectable_microphone(
            "TU116 High Definition Audio Controller Pro",
            6
        ));
        // 真实设备保留（含非 ASCII 描述名与取不到配置的兜底）。
        assert!(selectable_microphone(
            "\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro",
            2
        ));
        assert!(selectable_microphone("USB microphone", 1));
        assert!(selectable_microphone("Scarlett Solo", 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_and_wayland_have_explicitly_different_overlay_capabilities() {
        let x11 = LinuxCapabilitySnapshot::from_environment(None, Some(":0"), true, true);
        assert_eq!(x11.session, LinuxDesktopSession::X11);
        assert!(x11.capabilities.supports_overlay);
        assert!(!x11.capabilities.supports_local_asr);
        // deb/rpm 是本平台唯一的发布格式：宿主永远不能替换自己。
        assert!(!x11.capabilities.supports_auto_update);

        let wayland =
            LinuxCapabilitySnapshot::from_environment(Some("wayland-0"), Some(":0"), false, false);
        assert_eq!(wayland.session, LinuxDesktopSession::Wayland);
        assert!(!wayland.capabilities.supports_overlay);
        assert!(!wayland.capabilities.supports_desktop_hotkey);
        assert!(!wayland.capabilities.supports_auto_update);
    }

    #[test]
    fn headless_session_does_not_claim_desktop_or_microphone_support() {
        let snapshot = LinuxCapabilitySnapshot::from_environment(None, None, false, false);
        assert_eq!(snapshot.session, LinuxDesktopSession::Headless);
        assert!(!snapshot.capabilities.supports_local_asr);
        assert_eq!(
            snapshot.permissions.microphone,
            PermissionState::Unsupported
        );
    }
}
