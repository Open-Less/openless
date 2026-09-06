//! Native recording start/stop audio cues for Linux.
//!
//! The Windows/macOS Tauri shell synthesizes a "recording started" chime with
//! the Web Audio API in a webview (`app/src/lib/audioCue.ts`) and silences it
//! when the recording ends.  This crate is the native egui host, so there is no
//! webview; the equivalent cue is rendered as PCM and played to the default
//! output sink with cpal (already a dependency for the microphone).
//!
//! Honesty rules:
//! - The frame is never blocked: `play_cue_start`/`play_cue_stop` enqueue a
//!   detached worker thread and return immediately.  Any real failure to open
//!   the default output sink is logged and otherwise silent — a cue is
//!   feedback, never a hard error, matching the reference's "silently degrade,
//!   never throw" rule.
//! - Cues are gated by the caller on `audio_cue_on_record`, and the start cue
//!   is additionally suppressed when `mute_during_recording` is active (an
//!   audible start cue through a deliberately muted sink is both pointless and
//!   a needless PipeWire/KDE sink-input blip).  The stop cue may still play
//!   after output is restored.
//! - Synthesis is pure (`render_cue_mono`) so it is unit-testable without any
//!   audio device; the cpal playback path still needs real-device evidence on
//!   X11/Wayland before it may be reported as verified.

use std::sync::Arc;

/// A single synthesized sine note relative to the cue start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueTone {
    /// Frequency in Hz.
    pub freq_hz: f32,
    /// Start offset from the cue start in milliseconds.
    pub start_ms: f32,
    /// Duration in milliseconds.
    pub duration_ms: f32,
    /// Exponential-envelope peak gain (0..1).
    pub peak_gain: f32,
}

/// "Recording started" chime: rising minor third (A5 -> C#6), mirroring the
/// reference Web Audio cue so Linux and Windows/macOS share one sound.
pub fn start_cue_tones() -> Vec<CueTone> {
    vec![
        CueTone {
            freq_hz: 880.0,
            start_ms: 0.0,
            duration_ms: 130.0,
            peak_gain: 0.16,
        },
        CueTone {
            freq_hz: 1108.73,
            start_ms: 95.0,
            duration_ms: 170.0,
            peak_gain: 0.18,
        },
    ]
}

/// "Recording ended" cue: descending minor third (E5 -> C5).  Soft and short so
/// it reads as a clear "done" without masking the terminal feedback.
pub fn stop_cue_tones() -> Vec<CueTone> {
    vec![
        CueTone {
            freq_hz: 659.25,
            start_ms: 0.0,
            duration_ms: 120.0,
            peak_gain: 0.13,
        },
        CueTone {
            freq_hz: 523.25,
            start_ms: 90.0,
            duration_ms: 150.0,
            peak_gain: 0.15,
        },
    ]
}

/// Total cue duration in milliseconds (end of the last tone).
pub fn cue_total_duration_ms(tones: &[CueTone]) -> u32 {
    tones.iter().fold(0u32, |acc, tone| {
        acc.max((tone.start_ms + tone.duration_ms).round() as u32)
    })
}

/// Render a cue to mono interleaved `f32` samples in `[-1, 1]`.  Pure — no
/// device access — so it is fully unit-testable on any target.
pub fn render_cue_mono(tones: &[CueTone], sample_rate: u32) -> Vec<f32> {
    if tones.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let sr = sample_rate as f32;
    let total_samples =
        (((cue_total_duration_ms(tones) as f32) / 1000.0 * sr).ceil() as usize).max(1);
    let mut out = vec![0.0f32; total_samples];
    for tone in tones {
        let start = (tone.start_ms / 1000.0 * sr).round() as usize;
        let dur = ((tone.duration_ms / 1000.0) * sr).round() as usize;
        let attack = ((0.004 * sr).round() as usize).clamp(1, dur.max(1));
        let release_span = (dur.saturating_sub(attack)).max(1) as f32;
        for i in 0..dur {
            let idx = start + i;
            if idx >= out.len() {
                break;
            }
            let attack_env = if i < attack {
                i as f32 / attack as f32
            } else {
                1.0
            };
            let release_env = if i >= attack {
                let frac = (i - attack) as f32 / release_span;
                (-5.0 * frac).exp()
            } else {
                1.0
            };
            let env = attack_env * release_env;
            let phase = std::f32::consts::TAU * tone.freq_hz * (idx as f32 / sr);
            out[idx] += phase.sin() * tone.peak_gain * env;
        }
    }
    for sample in &mut out {
        *sample = sample.clamp(-1.0, 1.0);
    }
    out
}

/// Play a start cue asynchronously (never blocks the caller/frame).
pub fn play_cue_start() {
    play_cue(start_cue_tones());
}

/// Play a stop cue asynchronously (never blocks the caller/frame).
pub fn play_cue_stop() {
    play_cue(stop_cue_tones());
}

/// Best-effort asynchronous playback on a detached worker thread.
fn play_cue(tones: Vec<CueTone>) {
    if tones.is_empty() {
        return;
    }
    std::thread::Builder::new()
        .name("openless-audio-cue".to_string())
        .spawn(move || {
            if let Err(error) = play_cue_blocking(&tones) {
                log::debug!("[audio-cue] cue playback unavailable: {error}");
            }
        })
        .map_err(|error| log::debug!("[audio-cue] failed to spawn cue thread: {error}"))
        .ok();
}

#[cfg(target_os = "linux")]
fn play_cue_blocking(tones: &[CueTone]) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no Linux default output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|error| format!("default output config failed: {error}"))?;
    let sample_format = supported.sample_format();
    let sample_rate = supported.sample_rate().0;
    let channels = usize::from(supported.channels()).max(1);
    let config: cpal::StreamConfig = supported.into();
    let mono = Arc::new(render_cue_mono(tones, sample_rate));
    if mono.is_empty() {
        return Ok(());
    }
    let idx = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let build = |format: cpal::SampleFormat| -> Result<cpal::Stream, cpal::BuildStreamError> {
        macro_rules! make {
            ($ty:ty, $convert:expr) => {{
                let mono = Arc::clone(&mono);
                let idx = Arc::clone(&idx);
                let done = Arc::clone(&done);
                let device = &device;
                let config = &config;
                device.build_output_stream::<$ty, _, _>(
                    config,
                    move |data: &mut [$ty], _: &cpal::OutputCallbackInfo| {
                        let frames = data.len() / channels;
                        let mut pos = idx.load(std::sync::atomic::Ordering::Acquire);
                        for frame in 0..frames {
                            let sample = if pos < mono.len() { mono[pos] } else { 0.0 };
                            pos += 1;
                            let converted = $convert(sample);
                            for channel in 0..channels {
                                data[frame * channels + channel] = converted;
                            }
                        }
                        if pos >= mono.len() {
                            done.store(true, std::sync::atomic::Ordering::Release);
                        }
                        idx.store(pos, std::sync::atomic::Ordering::Release);
                    },
                    move |_error| {},
                    None,
                )
            }};
        }
        match format {
            cpal::SampleFormat::F32 => make!(f32, |s: f32| s.clamp(-1.0, 1.0)),
            cpal::SampleFormat::I16 => {
                make!(i16, |s: f32| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            }
            cpal::SampleFormat::U16 => make!(u16, |s: f32| {
                (((s.clamp(-1.0, 1.0) + 1.0) / 2.0) * u16::MAX as f32) as u16
            }),
            cpal::SampleFormat::I32 => {
                make!(i32, |s: f32| (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32)
            }
            other => {
                // Unusual sink format: fall back to f32 which most Linux sinks
                // accept even when it is not the default config.
                let _ = other;
                make!(f32, |s: f32| s.clamp(-1.0, 1.0))
            }
        }
    };

    let stream = build(sample_format)
        .or_else(|_| build(cpal::SampleFormat::F32))
        .map_err(|error| format!("build output stream failed: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("start output stream failed: {error}"))?;

    // Keep the stream alive on this thread until the cue buffer is consumed or
    // a short watchdog elapses, then drop it to release the sink.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !done.load(std::sync::atomic::Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    drop(stream);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn play_cue_blocking(_tones: &[CueTone]) -> Result<(), String> {
    Err("audio cue playback is only available on Linux".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_cue_is_a_rising_two_tone_and_stop_is_descending() {
        let start = start_cue_tones();
        assert_eq!(start.len(), 2);
        assert!(start[0].freq_hz < start[1].freq_hz, "start cue rises");
        let stop = stop_cue_tones();
        assert_eq!(stop.len(), 2);
        assert!(stop[0].freq_hz > stop[1].freq_hz, "stop cue descends");
    }

    #[test]
    fn total_duration_is_last_tone_end() {
        let start = start_cue_tones();
        assert_eq!(cue_total_duration_ms(&start), 265);
        assert!(cue_total_duration_ms(&stop_cue_tones()) > 0);
    }

    #[test]
    fn rendering_is_bounded_nonempty_and_expected_length() {
        let sr = 48_000;
        let mono = render_cue_mono(&start_cue_tones(), sr);
        let expected = ((cue_total_duration_ms(&start_cue_tones()) as f32 / 1000.0) * sr as f32)
            .ceil() as usize;
        assert_eq!(mono.len(), expected);
        assert!(mono.iter().any(|s| s.abs() > 1e-3), "cue is not silent");
        assert!(
            mono.iter().all(|s| (-1.0..=1.0).contains(s)),
            "cue stays within [-1, 1]"
        );
        // Envelope is peak-limited well below full scale so it never clips.
        let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak <= 0.34,
            "start cue peak {peak} stays under envelope sum"
        );
    }

    #[test]
    fn empty_tones_render_to_empty_and_play_is_a_noop() {
        assert!(render_cue_mono(&[], 44_100).is_empty());
        play_cue(Vec::new());
    }

    #[test]
    fn mono_cue_respects_sample_rate_scaling() {
        let at_44k = render_cue_mono(&stop_cue_tones(), 44_100);
        let at_48k = render_cue_mono(&stop_cue_tones(), 48_000);
        // Higher sample rate yields proportionally more samples for the same cue.
        assert!(at_48k.len() > at_44k.len());
    }
}
