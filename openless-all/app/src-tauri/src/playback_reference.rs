//! Playback-only capture, separate from the primary microphone backend.
use super::{CaptureDiagnostics, RecorderError};
use openless_core::ReferenceAudioBuffer;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

pub(super) fn capture_time(info: &cpal::InputCallbackInfo) -> Instant {
    let now = Instant::now();
    let timestamp = info.timestamp();
    match timestamp.callback.duration_since(&timestamp.capture) {
        Some(delay) => now.checked_sub(delay).unwrap_or(now),
        None => timestamp
            .capture
            .duration_since(&timestamp.callback)
            .and_then(|ahead| now.checked_add(ahead))
            .unwrap_or(now),
    }
}

#[cfg(target_os = "windows")]
pub(super) struct PlaybackReference(cpal::Stream);
#[cfg(target_os = "windows")]
impl Drop for PlaybackReference {
    fn drop(&mut self) {
        use cpal::traits::StreamTrait;
        if let Err(error) = self.0.pause() {
            log::warn!("[recorder] echo reference pause failed: {error}");
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) fn start(
    stop: &Arc<AtomicBool>,
    diagnostics: &Arc<CaptureDiagnostics>,
) -> Result<(PlaybackReference, Arc<ReferenceAudioBuffer>), RecorderError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| RecorderError::EngineFailed("no playback output device".into()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| RecorderError::EngineFailed(format!("echo reference output format: {e}")))?;
    let config = supported.config();
    let channels = config.channels as usize;
    let reference = Arc::new(ReferenceAudioBuffer::new(config.sample_rate.0));
    macro_rules! stream {
        ($t:ty,$convert:expr) => {{
            let reference = Arc::clone(&reference);
            let error_reference = Arc::clone(&reference);
            let stop = Arc::clone(stop);
            let callback_diagnostics = Arc::clone(diagnostics);
            let error_diagnostics = Arc::clone(diagnostics);
            device.build_input_stream(
                &config,
                move |data: &[$t], info| {
                    let captured_at = capture_time(info);
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    callback_diagnostics.data_callback(true, true, data.len() / channels.max(1));
                    let samples = data.iter().copied().map($convert).collect::<Vec<f32>>();
                    reference.push_interleaved_at(&samples, channels, captured_at);
                },
                move |error| {
                    error_reference.invalidate();
                    let count = error_diagnostics.reference.record_error();
                    error_diagnostics.event(
                        "P3",
                        "[\"H1\",\"H2\"]",
                        "reference_stream_error",
                        "reference_fault",
                        &format!("{{\"error_count\":{count}}}"),
                    );
                    log::warn!("[recorder] echo reference stream error: {error}");
                },
                None,
            )
        }};
    }
    // WASAPI render endpoints are distinct from input endpoints; CPAL opens
    // these with AUDCLNT_STREAMFLAGS_LOOPBACK, never as a microphone.
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => stream!(f32, |s: f32| s),
        cpal::SampleFormat::F64 => stream!(f64, |s: f64| s as f32),
        cpal::SampleFormat::I16 => stream!(i16, |s: i16| s as f32 / i16::MAX as f32),
        cpal::SampleFormat::I32 => stream!(i32, |s: i32| s as f32 / i32::MAX as f32),
        cpal::SampleFormat::I64 => stream!(i64, |s: i64| (s as f64 / i64::MAX as f64) as f32),
        cpal::SampleFormat::U8 => stream!(u8, |s: u8| (s as f32 - 128.0) / 128.0),
        cpal::SampleFormat::U16 => stream!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        cpal::SampleFormat::U32 => stream!(u32, |s: u32| (s as f64 / u32::MAX as f64 * 2.0 - 1.0)
            as f32),
        cpal::SampleFormat::U64 => stream!(u64, |s: u64| (s as f64 / u64::MAX as f64 * 2.0 - 1.0)
            as f32),
        cpal::SampleFormat::I8 => stream!(i8, |s: i8| s as f32 / i8::MAX as f32),
        _ => {
            return Err(RecorderError::EngineFailed(
                "unsupported playback reference sample format".into(),
            ))
        }
    }
    .map_err(super::classify_build_stream_err)?;
    stream
        .play()
        .map_err(|e| RecorderError::EngineFailed(format!("start playback reference: {e}")))?;
    Ok((PlaybackReference(stream), reference))
}

#[cfg(target_os = "macos")]
#[path = "playback_reference_macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
pub(super) use macos::{start, PlaybackReference};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) struct PlaybackReference;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn start(
    _stop: &Arc<AtomicBool>,
    _diagnostics: &Arc<CaptureDiagnostics>,
) -> Result<(PlaybackReference, Arc<ReferenceAudioBuffer>), RecorderError> {
    Err(RecorderError::EngineFailed(
        "playback reference is unavailable in this host; Linux uses the egui recorder".into(),
    ))
}
