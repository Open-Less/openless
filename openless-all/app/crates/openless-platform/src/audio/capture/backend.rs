//! 桌面平台（Windows / macOS / Linux）的 cpal 采集实现。

use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use super::{
    classify_stream_error, CaptureError, DeviceInfo, ErrorSink, SampleSink, StreamFormat,
};

/// 已打开的采集流。数据源不一定是 cpal：macOS 的系统音频 tap 是另一套句柄。
///
/// `cpal::Stream` 不是 `Send`，所以本类型也不可跨线程移动：宿主必须在创建它的
/// 线程上持有它（这与录音线程模型一致）。
pub struct CaptureStream(Source);

enum Source {
    Cpal(cpal::Stream),
    #[cfg(target_os = "macos")]
    Tap(super::macos_tap::TapStream),
}

impl CaptureStream {
    fn cpal(stream: cpal::Stream) -> Self {
        Self(Source::Cpal(stream))
    }

    #[cfg(target_os = "macos")]
    fn tap(stream: super::macos_tap::TapStream) -> Self {
        Self(Source::Tap(stream))
    }

    /// 开始投递样本。
    pub fn play(&self) -> Result<(), CaptureError> {
        match &self.0 {
            Source::Cpal(stream) => stream
                .play()
                .map_err(|error| classify_stream_error(&error.to_string())),
            #[cfg(target_os = "macos")]
            Source::Tap(stream) => stream.play(),
        }
    }

    /// 暂停投递。drop 之前显式调用，确保 macOS CoreAudio 的
    /// `AudioOutputUnitStop` 同步执行（否则麦克风占用指示不会立即消失）。
    pub fn pause(&self) -> Result<(), CaptureError> {
        match &self.0 {
            Source::Cpal(stream) => stream
                .pause()
                .map_err(|error| classify_stream_error(&error.to_string())),
            #[cfg(target_os = "macos")]
            Source::Tap(stream) => stream.pause(),
        }
    }
}

/// 所有输入设备（默认设备排在最前）。
pub fn input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let mut devices = Vec::new();
    if let Ok(iter) = host.input_devices() {
        for device in iter {
            let Ok(name) = device.name() else {
                continue;
            };
            devices.push(DeviceInfo {
                is_default: Some(&name) == default_name.as_ref(),
                name,
            });
        }
    }
    devices.sort_by_key(|device| !device.is_default);
    devices
}

/// 默认输入设备名。
pub fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|device| device.name().ok())
}

/// 打开输入流并按设备原生格式投递 f32 样本。
///
/// 设备选择：优先 `device_name` 指定的设备，找不到就退回默认设备；
/// 再不行就依次尝试所有输入设备（宿主的旧实现也有这层回退）。
pub fn start_input_stream(
    device_name: Option<&str>,
    sink: SampleSink,
    on_error: Option<ErrorSink>,
) -> Result<(CaptureStream, StreamFormat), CaptureError> {
    let host = cpal::default_host();
    let preferred = device_name.map(str::trim).filter(|name| !name.is_empty());

    let mut candidates: Vec<cpal::Device> = Vec::new();
    if let (Some(preferred), Ok(devices)) = (preferred, host.input_devices()) {
        for device in devices {
            if device.name().ok().as_deref() == Some(preferred) {
                candidates.push(device);
                break;
            }
        }
    }
    if let Some(default) = host.default_input_device() {
        candidates.push(default);
    }
    if let Ok(devices) = host.input_devices() {
        candidates.extend(devices);
    }
    if candidates.is_empty() {
        return Err(CaptureError::NoDevice);
    }

    let mut last_error = None;
    for device in candidates {
        match open_stream(&device, &sink, on_error.as_ref()) {
            Ok(opened) => return Ok(opened),
            Err(CaptureError::PermissionDenied) => return Err(CaptureError::PermissionDenied),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or(CaptureError::NoDevice))
}

/// 打开扬声器回环参考采集（默认输出设备）。
pub fn start_reference_stream(
    sink: SampleSink,
    on_error: Option<ErrorSink>,
) -> Result<(CaptureStream, StreamFormat), CaptureError> {
    // macOS：cpal 的通用输入路径在 duplex 声卡上拿不到系统输出，必须用 process tap。
    #[cfg(target_os = "macos")]
    {
        let _ = &on_error;
        let (stream, format) = super::macos_tap::start(sink)?;
        return Ok((CaptureStream::tap(stream), format));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(CaptureError::NoDevice)?;
        let supported = device
            .default_output_config()
            .map_err(|error| classify_stream_error(&error.to_string()))?;
        let config = supported.config();
        let format = StreamFormat {
            sample_rate: config.sample_rate.0,
            channels: config.channels,
        };
        let stream = build_stream(
            &device,
            &config,
            supported.sample_format(),
            sink,
            on_error,
            "speaker reference",
        )?;
        // 播放由宿主控制：它要先知道格式，才能把格式写进自己的音频帧。
        Ok((stream, format))
    }
}

fn open_stream(
    device: &cpal::Device,
    sink: &SampleSink,
    on_error: Option<&ErrorSink>,
) -> Result<(CaptureStream, StreamFormat), CaptureError> {
    // 优先用设备默认格式；失败后再试它支持的范围（不强行改单声道或 ASR 采样率）。
    let mut last_error = None;
    match device.default_input_config() {
        Ok(supported) => {
            let config = supported.config();
            let format = StreamFormat {
                sample_rate: config.sample_rate.0,
                channels: config.channels,
            };
            match build_stream(
                device,
                &config,
                supported.sample_format(),
                sink.clone(),
                on_error.cloned(),
                "microphone",
            ) {
                Ok(stream) => {
                    log::info!(
                        "[capture] input device={} rate={} channels={} fmt={:?}",
                        device.name().unwrap_or_else(|_| "<unknown>".into()),
                        format.sample_rate,
                        format.channels,
                        supported.sample_format()
                    );
                    return Ok((stream, format));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(error) => last_error = Some(classify_stream_error(&error.to_string())),
    }

    if let Ok(ranges) = device.supported_input_configs() {
        for range in ranges {
            for rate in [range.min_sample_rate().0, range.max_sample_rate().0] {
                let supported = range.with_sample_rate(cpal::SampleRate(rate));
                let config = supported.config();
                let format = StreamFormat {
                    sample_rate: config.sample_rate.0,
                    channels: config.channels,
                };
                if let Ok(stream) = build_stream(
                    device,
                    &config,
                    supported.sample_format(),
                    sink.clone(),
                    on_error.cloned(),
                    "microphone",
                ) {
                    return Ok((stream, format));
                }
            }
        }
    }
    Err(last_error.unwrap_or(CaptureError::NoDevice))
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    sink: SampleSink,
    on_error: Option<ErrorSink>,
    label: &'static str,
) -> Result<CaptureStream, CaptureError> {
    macro_rules! build {
        ($t:ty, $to_f32:expr) => {{
            let sink = sink.clone();
            let on_error = on_error.clone();
            device.build_input_stream(
                config,
                move |data: &[$t], info: &cpal::InputCallbackInfo| {
                    let mut samples: Vec<f32> = data.iter().copied().map($to_f32).collect();
                    // 时间戳：cpal 的 callback/capture 差值给出采集延迟。
                    let now = Instant::now();
                    let captured_at = info
                        .timestamp()
                        .callback
                        .duration_since(&info.timestamp().capture)
                        .and_then(|delay| now.checked_sub(delay))
                        .or(Some(now));
                    (sink.lock())(&mut samples, captured_at);
                },
                move |error| {
                    let message = error.to_string();
                    log::warn!("[capture] {label} stream error: {message}");
                    if let Some(on_error) = on_error.as_ref() {
                        on_error(&message);
                    }
                },
                None,
            )
        }};
    }

    let stream = match sample_format {
        SampleFormat::F32 => build!(f32, |s: f32| s),
        SampleFormat::F64 => build!(f64, |s: f64| s as f32),
        SampleFormat::I16 => build!(i16, |s: i16| s as f32 / i16::MAX as f32),
        SampleFormat::I32 => build!(i32, |s: i32| s as f32 / i32::MAX as f32),
        SampleFormat::I64 => build!(i64, |s: i64| (s as f64 / i64::MAX as f64) as f32),
        SampleFormat::U8 => build!(u8, |s: u8| (s as f32 - 128.0) / 128.0),
        SampleFormat::U16 => build!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        SampleFormat::U32 => {
            build!(u32, |s: u32| (s as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32)
        }
        SampleFormat::U64 => {
            build!(u64, |s: u64| (s as f64 / u64::MAX as f64 * 2.0 - 1.0) as f32)
        }
        SampleFormat::I8 => build!(i8, |s: i8| s as f32 / i8::MAX as f32),
        other => {
            return Err(CaptureError::Unavailable(format!(
                "unsupported sample format: {other:?}"
            )))
        }
    }
    .map_err(|error| classify_stream_error(&error.to_string()))?;
    Ok(CaptureStream::cpal(stream))
}
