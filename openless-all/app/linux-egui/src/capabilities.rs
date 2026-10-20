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
                // Linux uses the separate egui/fcitx host; the Tauri Voice
                // Edit coordinator and panel are not available in that host.
                supports_voice_edit: false,
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
    let pulse = pulse_sources();
    let devices = host.input_devices().map_err(|error| {
        BackendError::new(
            BackendErrorCode::Platform,
            format!("failed to enumerate Linux microphones: {error}"),
        )
    })?;
    let mut out: Vec<MicrophoneDevice> = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device.to_string();
        let device_id = device
            .id()
            .ok()
            .filter(|id| matches!(id.host(), cpal::HostId::PipeWire | cpal::HostId::PulseAudio));
        let source_id = device_id.as_ref().map(|id| id.id());
        let channels = device
            .default_input_config()
            .map(|config| config.channels())
            .unwrap_or(0);
        if !keep_microphone(&name, channels, source_id, &pulse) {
            continue;
        }
        // 同一个设备会以多条节点出现（例如「内置音频 Pro」出现两次）。
        if out.iter().any(|existing| existing.name == name) {
            continue;
        }
        out.push(MicrophoneDevice {
            id: format!("cpal:{index}:{name}"),
            // cpal 的 `default_input_device` 拿到的是 PipeWire 的 `default_input`
            // 合成节点，永远不等于真实设备名；真正要标「默认」的是 pulse 的默认源。
            is_default: pulse.is_default(source_id)
                || default_name.as_deref() == Some(name.as_str()),
            name,
        });
    }
    Ok(out)
}

/// 仅剔除能按稳定身份唯一关联的输出监听；显示名相同不足以证明设备是监听。
#[cfg(target_os = "linux")]
fn keep_microphone(
    name: &str,
    channels: u16,
    device_id: Option<&str>,
    pulse: &PulseSources,
) -> bool {
    selectable_microphone(name, channels)
        && !pulse
            .source(device_id)
            .is_some_and(|source| source.monitor_of_sink.is_some())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PulseSource {
    name: String,
    node_name: Option<String>,
    monitor_of_sink: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PulseSources {
    sources: Vec<PulseSource>,
    /// 默认采集源的稳定名称，不使用可能重名的 Description。
    default: Option<String>,
}

#[cfg(target_os = "linux")]
impl PulseSources {
    fn source(&self, device_id: Option<&str>) -> Option<&PulseSource> {
        let device_id = device_id.filter(|id| !id.is_empty())?;
        let mut matches = self.sources.iter().filter(|source| {
            source.name == device_id
                || source.node_name.as_deref() == Some(device_id)
                || source.monitor_of_sink.as_deref() == Some(device_id)
        });
        let source = matches.next()?;
        matches.next().is_none().then_some(source)
    }

    fn is_default(&self, device_id: Option<&str>) -> bool {
        self.source(device_id).is_some_and(|source| {
            source.monitor_of_sink.is_none()
                && self.default.as_deref() == Some(source.name.as_str())
        })
    }
}

/// 读 `pactl`。工具缺失或失败时返回空值：宁可多列（退回旧行为），也不误删用户的麦克风。
#[cfg(target_os = "linux")]
fn pulse_sources() -> PulseSources {
    let list = std::process::Command::new("pactl")
        .args(["list", "sources"])
        .output();
    let Ok(list) = list else {
        return PulseSources::default();
    };
    if !list.status.success() {
        return PulseSources::default();
    }
    let default = std::process::Command::new("pactl")
        .arg("get-default-source")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|name| !name.is_empty());
    parse_pulse_sources(&String::from_utf8_lossy(&list.stdout), default.as_deref())
}

/// 解析 `pactl list sources`。
///
/// 每个 source 形如（顶层字段缩进一个制表符，`Properties:` 里两个；下面用空格示意）：
///
/// ```text
/// Source #57
///   Name: alsa_output.pci-0000_00_1f.3.pro-output-0.monitor
///   Description: Monitor of Built-in Audio Pro
///   Monitor of Sink: alsa_output.pci-0000_00_1f.3.pro-output-0
///   Properties:
///     node.name = "alsa_output.pci-0000_00_1f.3.pro-output-0.monitor"
/// ```
#[cfg(target_os = "linux")]
fn parse_pulse_sources(text: &str, default_name: Option<&str>) -> PulseSources {
    let mut out = PulseSources {
        default: default_name.map(str::to_string),
        ..Default::default()
    };
    let mut source = PulseSource::default();
    for line in text.lines() {
        let Some(field) = line.strip_prefix('\t') else {
            continue;
        };
        // PipeWire 的 CPAL ID 是 node.name；PulseAudio 的 ID 是顶层 Name。
        if let Some(value) = field.strip_prefix("\tnode.name = ") {
            source.node_name = Some(value.trim().trim_matches('"').to_string());
        }
        if field.starts_with('\t') {
            continue;
        }
        if let Some(value) = field.strip_prefix("Name: ") {
            if !source.name.is_empty() {
                out.sources.push(std::mem::take(&mut source));
            }
            source.name = value.trim().to_string();
        } else if let Some(value) = field.strip_prefix("Monitor of Sink:") {
            let value = value.trim();
            source.monitor_of_sink =
                (!value.is_empty() && value != "n/a").then(|| value.to_string());
        }
    }
    if !source.name.is_empty() {
        out.sources.push(source);
    }
    out
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
    use super::{keep_microphone, parse_pulse_sources, selectable_microphone};

    /// 本机 `pactl list sources` 的真实形态（两个 sink 监听 + 一个真采集源），
    /// 设备名沿用现场的转义写法。
    const PACTL_LIST_SOURCES: &str = "Source #57\n\
\tName: alsa_output.pci-0000_01_00.1.pro-output-3.monitor\n\
\tDescription: Monitor of TU116 High Definition Audio Controller Pro\n\
\tMonitor of Sink: alsa_output.pci-0000_01_00.1.pro-output-3\n\
\tProperties:\n\
\t\tnode.name = \"alsa_output.pci-0000_01_00.1.pro-output-3.monitor\"\n\
\t\tdevice.description = \"TU116 High Definition Audio Controller\"\n\
Source #5207\n\
\tName: alsa_output.pci-0000_00_1f.3.pro-output-0.monitor\n\
\tDescription: Monitor of \u{5185}\u{7f6e}\u{97f3}\u{9891} Pro\n\
\tMonitor of Sink: alsa_output.pci-0000_00_1f.3.pro-output-0\n\
Source #5208\n\
\tName: alsa_input.pci-0000_00_1f.3.pro-input-2\n\
\tDescription: \u{5185}\u{7f6e}\u{97f3}\u{9891} Pro 2\n\
\tMonitor of Sink: n/a\n\
\tProperties:\n\
\t\tdevice.description = \"\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro 2\"\n";

    #[test]
    fn monitor_sources_are_not_offered_as_microphones() {
        let pulse = parse_pulse_sources(
            PACTL_LIST_SOURCES,
            Some("alsa_input.pci-0000_00_1f.3.pro-input-2"),
        );
        assert_eq!(pulse.sources.len(), 3);
        assert_eq!(
            pulse.sources[0].node_name.as_deref(),
            Some("alsa_output.pci-0000_01_00.1.pro-output-3.monitor")
        );
        assert!(pulse.sources[0].monitor_of_sink.is_some());
        assert!(pulse.sources[1].monitor_of_sink.is_some());
        assert!(pulse.sources[2].monitor_of_sink.is_none());
        // 默认采集源是那台真麦克风，不是任何监听。
        assert_eq!(
            pulse.default.as_deref(),
            Some("alsa_input.pci-0000_00_1f.3.pro-input-2")
        );
    }

    #[test]
    fn real_capture_sources_survive_the_monitor_filter() {
        let pulse = parse_pulse_sources(
            PACTL_LIST_SOURCES,
            Some("alsa_input.pci-0000_00_1f.3.pro-input-2"),
        );
        let keep =
            |name: &str, channels: u16, id: &str| keep_microphone(name, channels, Some(id), &pulse);
        // 现场 cpal 给出的 8 个节点，逐个核对（raw 顺序即此）。
        assert!(!keep("default_sink", 2, "sink_default"));
        assert!(!keep("default_input", 2, "input_default"));
        assert!(!keep(
            "TU116 High Definition Audio Controller Pro",
            2,
            "alsa_output.pci-0000_01_00.1.pro-output-3",
        ));
        assert!(!keep(
            "TU116 High Definition Audio Controller Pro 7",
            8,
            "hdmi7"
        ));
        assert!(!keep(
            "TU116 High Definition Audio Controller Pro 9",
            8,
            "hdmi9"
        ));
        assert!(!keep(
            "\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro",
            2,
            "alsa_output.pci-0000_00_1f.3.pro-output-0.monitor",
        ));
        // 只有真麦克风留下来。
        assert!(keep(
            "\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro 2",
            2,
            "alsa_input.pci-0000_00_1f.3.pro-input-2",
        ));
    }

    const SAME_DESCRIPTION_SOURCES: &str = "Source #1\n\
\tName: output.monitor\n\
\tDescription: Monitor of USB Headset\n\
\tMonitor of Sink: output\n\
\tProperties:\n\
\t\tnode.name = \"output\"\n\
Source #2\n\
\tName: input\n\
\tDescription: USB Headset\n\
\tMonitor of Sink: n/a\n\
\tProperties:\n\
\t\tnode.name = \"capture-node\"\n";

    #[test]
    fn same_description_does_not_hide_the_real_default_source() {
        let pulse = parse_pulse_sources(SAME_DESCRIPTION_SOURCES, Some("input"));
        for id in ["input", "capture-node"] {
            assert!(keep_microphone("USB Headset", 2, Some(id), &pulse));
            assert!(pulse.is_default(Some(id)));
        }
        for id in ["output.monitor", "output"] {
            assert!(!keep_microphone("USB Headset", 2, Some(id), &pulse));
            assert!(!pulse.is_default(Some(id)));
        }
    }

    #[test]
    fn unavailable_or_unknown_identity_does_not_filter_by_description() {
        let pulse = parse_pulse_sources(SAME_DESCRIPTION_SOURCES, Some("input"));
        for id in [None, Some(""), Some("unknown-id")] {
            assert!(keep_microphone("USB Headset", 2, id, &pulse));
            assert!(!pulse.is_default(id));
        }
    }

    #[test]
    fn ambiguous_source_identity_is_preserved() {
        let text = SAME_DESCRIPTION_SOURCES.replace("capture-node", "output");
        let pulse = parse_pulse_sources(&text, Some("input"));
        assert!(keep_microphone("USB Headset", 2, Some("output"), &pulse));
        assert!(!pulse.is_default(Some("output")));
        assert!(keep_microphone("USB Headset", 2, Some("input"), &pulse));
    }

    #[test]
    fn monitor_role_does_not_depend_on_english_description_prefix() {
        let text = SAME_DESCRIPTION_SOURCES.replace("Monitor of USB Headset", "USB Headset");
        let pulse = parse_pulse_sources(&text, Some("output.monitor"));
        assert!(!keep_microphone(
            "USB Headset",
            2,
            Some("output.monitor"),
            &pulse
        ));
        assert!(!pulse.is_default(Some("output.monitor")));
        assert!(keep_microphone("USB Headset", 2, Some("input"), &pulse));
    }

    #[test]
    fn without_pactl_no_device_is_dropped() {
        // 拿不到源列表时退回旧行为：宁可多列，也不能误删用户的麦克风。
        let pulse = parse_pulse_sources("", None);
        assert_eq!(pulse, super::PulseSources::default());
        assert!(keep_microphone("Some USB microphone", 1, None, &pulse));
        assert!(keep_microphone("USB Headset", 2, Some("input"), &pulse));
        assert!(!pulse.is_default(Some("input")));
    }

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
