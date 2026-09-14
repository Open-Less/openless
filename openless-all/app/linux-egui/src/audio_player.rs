//! In-app playback for history recordings.
//!
//! Recordings are 16 kHz mono signed-16 WAV. This plays them through the default
//! CPAL output device with linear resampling to whatever rate the device runs
//! at, so the history page can show a real player bar instead of shelling out to
//! an external player.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// A playing clip. Dropping it stops playback.
pub struct ClipPlayer {
    _stream: cpal::Stream,
    played_samples: Arc<AtomicUsize>,
    total_samples: usize,
    channels: usize,
    sample_rate: u32,
    finished: Arc<AtomicBool>,
}

impl ClipPlayer {
    /// Play `pcm` (16 kHz mono little-endian i16 bytes).
    pub fn play(pcm: &[u8]) -> Result<Self, String> {
        let mono: Vec<f32> = pcm
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32_768.0)
            .collect();
        if mono.is_empty() {
            return Err("empty recording".to_string());
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        // Linear resample 16 kHz -> device rate and fan out to every channel.
        let ratio = 16_000.0_f64 / sample_rate as f64;
        let out_frames = (mono.len() as f64 / ratio).ceil() as usize;
        let mut interleaved: Vec<f32> = Vec::with_capacity(out_frames * channels);
        for frame in 0..out_frames {
            let source = frame as f64 * ratio;
            let index = source.floor() as usize;
            let frac = (source - index as f64) as f32;
            let first = mono.get(index).copied().unwrap_or(0.0);
            let second = mono.get(index + 1).copied().unwrap_or(first);
            let value = first + (second - first) * frac;
            for _ in 0..channels {
                interleaved.push(value);
            }
        }

        let data = Arc::new(interleaved);
        let played_samples = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let total_samples = data.len();

        let stream = build_stream(
            &device,
            config,
            sample_format,
            data.clone(),
            played_samples.clone(),
            finished.clone(),
        )?;
        stream.play().map_err(|error| error.to_string())?;

        Ok(Self {
            _stream: stream,
            played_samples,
            total_samples,
            channels,
            sample_rate,
            finished,
        })
    }

    /// Playback head in milliseconds.
    pub fn position_ms(&self) -> u64 {
        let frames = self.played_samples.load(Ordering::Relaxed) / self.channels.max(1);
        (frames as u64 * 1000) / self.sample_rate.max(1) as u64
    }

    /// Clip length in milliseconds.
    pub fn total_ms(&self) -> u64 {
        let frames = self.total_samples / self.channels.max(1);
        (frames as u64 * 1000) / self.sample_rate.max(1) as u64
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

fn build_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
    data: Arc<Vec<f32>>,
    played: Arc<AtomicUsize>,
    finished: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let error_callback = |error| log::warn!("audio playback error: {error}");
    // Fill the output buffer from `data`, starting at the played head.
    macro_rules! writer {
        ($ty:ty, $convert:expr) => {{
            let data = data.clone();
            let played = played.clone();
            let finished = finished.clone();
            device
                .build_output_stream(
                    config.clone(),
                    move |output: &mut [$ty], _| {
                        let convert: fn(f32) -> $ty = $convert;
                        let start = played.load(Ordering::Relaxed);
                        let mut done = false;
                        for (offset, slot) in output.iter_mut().enumerate() {
                            let index = start + offset;
                            match data.get(index) {
                                Some(value) => *slot = convert(*value),
                                None => {
                                    done = true;
                                    *slot = convert(0.0);
                                }
                            }
                        }
                        played.store(start + output.len(), Ordering::Relaxed);
                        if done {
                            finished.store(true, Ordering::Relaxed);
                        }
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| error.to_string())
        }};
    }
    match format {
        cpal::SampleFormat::F32 => writer!(f32, |value| value),
        cpal::SampleFormat::I16 => {
            writer!(i16, |value| (value * 32_767.0).clamp(-32_768.0, 32_767.0)
                as i16)
        }
        cpal::SampleFormat::U16 => {
            writer!(
                u16,
                |value| ((value * 0.5 + 0.5) * 65_535.0).clamp(0.0, 65_535.0) as u16
            )
        }
        other => Err(format!("unsupported output sample format: {other:?}")),
    }
}
