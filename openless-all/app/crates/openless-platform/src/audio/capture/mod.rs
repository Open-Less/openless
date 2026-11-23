//! 音频采集：设备枚举、输入流、扬声器回环参考。
//!
//! 平台层负责"把设备打开并投递 f32 样本"，宿主负责"这些样本怎么用"
//! （缓冲、重采样、诊断、WAV 归档、录音状态机）。
//!
//! 平台差异：
//! - **Windows / Linux 桌面**：cpal；扬声器回环用默认输出设备开 capture 流
//!   （WASAPI loopback / PulseAudio monitor）。
//! - **macOS**：cpal（CoreAudio）；回环走同一设备的 input 流，由 CoreAudio 的
//!   聚合行为决定是否有可用回环源。
//! - **Android / iOS**：系统采集由宿主 JNI / AVAudioSession 承担，这里返回
//!   [`CaptureError::Unsupported`]。
//!
//! 所有错误都是显式的：调用方可以区分"没设备"、"没权限"、"被占用"，
//! 而不是拿到一个空样本流。

use std::time::Instant;

/// 采集失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// 系统拒绝了麦克风权限。
    PermissionDenied,
    /// 当前没有可用的输入设备。
    NoDevice,
    /// 该平台没有这条通道（Android / iOS）。
    Unsupported,
    /// 设备打不开或流构建失败。
    Unavailable(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied => write!(f, "microphone permission denied"),
            Self::NoDevice => write!(f, "no input device"),
            Self::Unsupported => write!(f, "audio capture is not available on this platform"),
            Self::Unavailable(reason) => write!(f, "{reason}"),
        }
    }
}

/// 已打开的采集流格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// 一个输入设备。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// 样本回调：交错 f32 样本 + 采集时刻（系统能给时）。
///
/// 用 `Arc<Mutex<..>>` 而不是 `Box<dyn FnMut>`：设备/格式可能有多个候选，
/// 每次重试都要新建流，共享同一个回调比装箱克隆更清晰也更安全。
pub type SampleSink = std::sync::Arc<parking_lot::Mutex<dyn FnMut(&[f32], Option<Instant>) + Send>>;

/// 流运行期错误回调（设备掉线、后端故障）。
///
/// 宿主用它把错误送进自己的运行时错误通道；不提供时平台层只记日志。
pub type ErrorSink = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// 把闭包包装成 [`ErrorSink`]。
pub fn error_sink(callback: impl Fn(&str) + Send + Sync + 'static) -> ErrorSink {
    std::sync::Arc::new(callback)
}

/// 把闭包包装成 [`SampleSink`]。
pub fn sample_sink(
    callback: impl FnMut(&[f32], Option<Instant>) + Send + 'static,
) -> SampleSink {
    std::sync::Arc::new(parking_lot::Mutex::new(callback))
}

/// 把 cpal 的错误消息归类。宿主据此给出"权限/设备/其它"的用户提示。
///
/// 纯函数，便于在没有真实声卡的环境里测试。
pub fn classify_stream_error(message: &str) -> CaptureError {
    // 有些后端用紧凑写法（DeviceNotAvailable），先归一化再匹配。
    let lower = message.to_ascii_lowercase().replace(['_', '-'], "");
    if lower.contains("permission")
        || lower.contains("not authorized")
        || lower.contains("denied")
        || lower.contains("0x80070005")
    {
        return CaptureError::PermissionDenied;
    }
    if lower.contains("no device")
        || lower.contains("no default input")
        || lower.contains("device not found")
        || lower.contains("device not available")
        || lower.contains("devicenotavailable")
    {
        return CaptureError::NoDevice;
    }
    CaptureError::Unavailable(message.to_string())
}

#[cfg(target_os = "android")]
pub mod android_capture;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod backend;

#[cfg(target_os = "macos")]
mod macos_tap;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use backend::{
    default_input_name, input_devices, start_input_stream, start_reference_stream,
};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use backend::CaptureStream as DesktopCaptureStream;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use backend::CaptureStream;

#[cfg(any(target_os = "android", target_os = "ios"))]
pub struct CaptureStream;

#[cfg(any(target_os = "android", target_os = "ios"))]
impl CaptureStream {
    /// 移动端由宿主的系统采集通道负责，桌面采集流不可用。
    pub fn play(&self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }

    /// 同上：没有需要暂停的桌面流。
    pub fn pause(&self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn input_devices() -> Vec<DeviceInfo> {
    Vec::new()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn default_input_name() -> Option<String> {
    None
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn start_input_stream(
    _device_name: Option<&str>,
    _sink: SampleSink,
    _on_error: Option<ErrorSink>,
) -> Result<(CaptureStream, StreamFormat), CaptureError> {
    Err(CaptureError::Unsupported)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn start_reference_stream(
    _sink: SampleSink,
    _on_error: Option<ErrorSink>,
) -> Result<(CaptureStream, StreamFormat), CaptureError> {
    Err(CaptureError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_errors_are_recognized() {
        for message in [
            "permission denied",
            "Not Authorized to access microphone",
            "access denied",
            "HRESULT 0x80070005",
        ] {
            assert_eq!(classify_stream_error(message), CaptureError::PermissionDenied);
        }
    }

    #[test]
    fn missing_device_is_distinguished_from_permission() {
        for message in [
            "no default input device",
            "device not found",
            "DeviceNotAvailable",
        ] {
            assert_eq!(classify_stream_error(message), CaptureError::NoDevice);
        }
    }

    #[test]
    fn other_failures_keep_their_message() {
        match classify_stream_error("callback panic") {
            CaptureError::Unavailable(reason) => assert_eq!(reason, "callback panic"),
            other => panic!("unexpected classification: {other:?}"),
        }
    }

    #[test]
    fn errors_are_readable() {
        assert_eq!(
            CaptureError::PermissionDenied.to_string(),
            "microphone permission denied"
        );
        assert_eq!(CaptureError::Unsupported.to_string(), "audio capture is not available on this platform");
    }
}
