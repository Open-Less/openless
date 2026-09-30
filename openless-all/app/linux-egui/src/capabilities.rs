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
        let channels = device
            .default_input_config()
            .map(|config| config.channels())
            .unwrap_or(0);
        if !keep_microphone(&name, channels, &pulse) {
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
            is_default: pulse.default.as_deref() == Some(name.as_str())
                || default_name.as_deref() == Some(name.as_str()),
            name,
        });
    }
    Ok(out)
}

/// 一台设备要不要进「首选麦克风」：先按节点特征筛（[`selectable_microphone`]），再剔除
/// 输出监听（[`PulseSources::monitors`]）。抽成纯函数是为了能直接测这两段合起来的结果。
#[cfg(target_os = "linux")]
fn keep_microphone(name: &str, channels: u16, pulse: &PulseSources) -> bool {
    selectable_microphone(name, channels) && !pulse.monitors.iter().any(|monitor| monitor == name)
}

/// PipeWire/Pulse 的源列表里，我们唯一拿不到、但足以区分「音响监听」的信息。
///
/// 现场：本机 `cpal::input_devices()` 给出 8 个节点，其中
/// `TU116 High Definition Audio Controller Pro`（显卡 HDMI 输出的监听）、
/// `内置音频 Pro`（内置扬声器的监听）都被当成麦克风列了出来，真实采集源
/// `内置音频 Pro 2` 反而排在后面 —— 用户反馈「麦克风选择变成了音响选择」。
///
/// 成因：PipeWire 给每个 sink 都造一个 monitor source，Pulse 给它的描述是
/// `Monitor of <sink 描述>`，而 cpal 的 Pulse 后端把 `Monitor of ` 前缀去掉后原样
/// 当成设备名。`pactl list sources` 的 monitor 项带 `Monitor of Sink:` 字段，据此把
/// 描述精确还原成 cpal 会显示的那个名字即可剔除，不需要猜命名规律：真实采集源
/// （如本机 `内置音频 Pro 2`）的描述不带前缀，永远不会被误删。
#[derive(Debug, Default, PartialEq, Eq)]
struct PulseSources {
    /// 要剔除的 cpal 设备名（monitor 描述去掉 `Monitor of ` 前缀）。
    monitors: Vec<String>,
    /// 默认采集源的描述；cpal 对真实源用的就是 Pulse 的 `Description`。
    default: Option<String>,
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
    let mut out = PulseSources::default();
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut is_monitor = false;
    let flush = |name: Option<String>,
                 description: Option<String>,
                 is_monitor: bool,
                 out: &mut PulseSources| {
        let Some(description) = description else {
            return;
        };
        if is_monitor {
            let stripped = description
                .strip_prefix("Monitor of ")
                .unwrap_or(&description)
                .to_string();
            if !out.monitors.contains(&stripped) {
                out.monitors.push(stripped);
            }
        } else if name.as_deref() == default_name {
            out.default = Some(description);
        }
    };
    for line in text.lines() {
        let Some(field) = line.strip_prefix('\t') else {
            continue;
        };
        // `Properties:` 块里的字段缩进是两层，`node.name`/`device.description` 都在其中，
        // 不能当成顶层字段读。
        if field.starts_with('\t') {
            continue;
        }
        if let Some(value) = field.strip_prefix("Name: ") {
            if name.is_some() {
                flush(name.take(), description.take(), is_monitor, &mut out);
                is_monitor = false;
            }
            name = Some(value.trim().to_string());
        } else if let Some(value) = field.strip_prefix("Description: ") {
            description = Some(value.trim().to_string());
        } else if field.starts_with("Monitor of Sink:") {
            is_monitor = field
                .trim_start_matches("Monitor of Sink:")
                .trim()
                .ne("n/a");
        }
    }
    flush(name, description, is_monitor, &mut out);
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
        // 监听源的描述在 cpal 侧就是去掉前缀后的 sink 描述。
        assert_eq!(
            pulse.monitors,
            vec![
                "TU116 High Definition Audio Controller Pro".to_string(),
                "\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro".to_string(),
            ]
        );
        // 默认采集源是那台真麦克风，不是任何监听。
        assert_eq!(
            pulse.default.as_deref(),
            Some("\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro 2")
        );
    }

    #[test]
    fn real_capture_sources_survive_the_monitor_filter() {
        let pulse = parse_pulse_sources(
            PACTL_LIST_SOURCES,
            Some("alsa_input.pci-0000_00_1f.3.pro-input-2"),
        );
        let keep = |name: &str, channels: u16| keep_microphone(name, channels, &pulse);
        // 现场 cpal 给出的 8 个节点，逐个核对（raw 顺序即此）。
        assert!(!keep("default_sink", 2));
        assert!(!keep("default_input", 2));
        assert!(!keep("TU116 High Definition Audio Controller Pro", 2));
        assert!(!keep("TU116 High Definition Audio Controller Pro 7", 8));
        assert!(!keep("TU116 High Definition Audio Controller Pro 9", 8));
        assert!(!keep("\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro", 2));
        // 只有真麦克风留下来。
        assert!(keep("\u{5185}\u{7f6e}\u{97f3}\u{9891} Pro 2", 2));
    }

    #[test]
    fn without_pactl_no_device_is_dropped() {
        // 拿不到源列表时退回旧行为：宁可多列，也不能误删用户的麦克风。
        let pulse = parse_pulse_sources("", None);
        assert_eq!(pulse, super::PulseSources::default());
        assert!(keep_microphone("Some USB microphone", 1, &pulse));
        assert!(keep_microphone("USB Headset", 2, &pulse));
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
