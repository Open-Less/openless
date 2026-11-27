//! Native recording cue for Linux.
//!
//! The Windows/macOS Tauri shell synthesizes the "recording started" chime with
//! the Web Audio API in a webview (`app/src/lib/audioCue.ts`,
//! `recordStartCueTones()` + `scheduleCueVoices()`).  This crate is the native
//! egui host, so there is no webview; the same cue is rendered as PCM here and
//! played to the default output sink with cpal (already a dependency for the
//! microphone).  Parameters **and** envelope semantics are kept identical to
//! `audioCue.ts` so Linux and Windows/macOS share one sound:
//!
//! | `audioCue.ts`                                        | here                                |
//! |------------------------------------------------------|-------------------------------------|
//! | `recordStartCueTones()` 880 Hz / 0 ms / 130 ms / .16 | `start_cue_tones()[0]`              |
//! | 1108.73 Hz / 95 ms / 170 ms / .18                    | `start_cue_tones()[1]`              |
//! | `osc.type = 'sine'`                                  | `sin(2 * pi * freq_hz * t)`         |
//! | `setValueAtTime(0.0001, t0)`                         | `CUE_FLOOR`                         |
//! | `exponentialRamp(peakGain, t0 + 0.005)`              | 5 ms 指数 attack                    |
//! | `exponentialRamp(0.0001, tEnd)`                      | 指数 decay 到 `CUE_FLOOR`           |
//! | `stopAudioCue()` = `stopVoices()`，**不发声**        | `play_cue_stop()` = 作废当前代次    |
//! | `scheduleCueVoices()` 先 `stopVoices()` 防叠音       | 新一代次使在播的旧音静音            |
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
//!   a needless PipeWire/KDE sink-input blip).
//! - Synthesis is pure (`render_cue_mono`) so it is unit-testable without any
//!   audio device; the cpal playback path still needs real-device evidence on
//!   X11/Wayland before it may be reported as verified.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Web Audio 的指数斜坡下限（`audioCue.ts` 里用 0.0001 代替 0：0 无法走指数）。
const CUE_FLOOR: f32 = 0.0001;
/// `audioCue.ts` 的 attack 时间：`exponentialRampToValueAtTime(peakGain, t0 + 0.005)`。
const CUE_ATTACK_SECS: f32 = 0.005;

/// 当前提示音代次。与 Tauri 的 `playSeq` / `stopVoices()` 对应：
/// - 每次 `play_cue_start` 递增：新音开始时在播的旧音立即静音（Tauri 排期前会
///   `stopVoices()`，避免连按热键叠音越来越响）。
/// - `play_cue_stop` 也递增：等价于 Tauri 的 `stopAudioCue()` —— 只停不发声，
///   Tauri 在录音结束时并没有“结束提示音”。
static CUE_GENERATION: AtomicU64 = AtomicU64::new(0);

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

/// "Recording started" chime: rising minor third (A5 -> C#6) — 与 Tauri
/// `recordStartCueTones()` 逐字段一致（880/0ms/130ms/0.16 + 1108.73/95ms/170ms/0.18）。
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
        let dur_secs = dur as f32 / sr;
        // `math.exp` 的指数斜坡在**增益空间是直线**：
        //   gain.setValueAtTime(0.0001, t0)
        //   gain.exponentialRampToValueAtTime(peakGain, t0 + 0.005)
        //   gain.exponentialRampToValueAtTime(0.0001, tEnd)
        // 即 g(t) = a * (b/a)^(t/T)。这里逐步复现同一个曲线（不是线性 attack +
        // 任意 tau 的衰减）。
        let attack_secs = CUE_ATTACK_SECS.min(dur_secs.max(f32::EPSILON));
        let decay_secs = (dur_secs - attack_secs).max(f32::EPSILON);
        for i in 0..dur {
            let idx = start + i;
            if idx >= out.len() {
                break;
            }
            let t = i as f32 / sr;
            let env = if t < attack_secs {
                CUE_FLOOR * (tone.peak_gain / CUE_FLOOR).powf(t / attack_secs)
            } else {
                let frac = ((t - attack_secs) / decay_secs).min(1.0);
                tone.peak_gain * (CUE_FLOOR / tone.peak_gain).powf(frac)
            };
            let phase = std::f32::consts::TAU * tone.freq_hz * (idx as f32 / sr);
            out[idx] += phase.sin() * env;
        }
    }
    for sample in &mut out {
        *sample = sample.clamp(-1.0, 1.0);
    }
    out
}

/// Play a start cue asynchronously (never blocks the caller/frame).
/// 与 Tauri `playRecordStartCue()` 同语义：新的一生效后，上一声立即静音。
pub fn play_cue_start() {
    let generation = CUE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    play_cue(start_cue_tones(), generation);
}

/// 停止当前提示音。对应 Tauri `stopAudioCue()` —— **只停不发声**：
/// Tauri 在录音结束时调用的就是它，并没有“结束提示音”。
/// 正在播放的提示音会被静音（代次作废），未播放的不再开始。
pub fn play_cue_stop() {
    CUE_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// 该代次是否仍是当前代次；`false` 表示应立刻静音（等价 `stopVoices()`）。
fn cue_generation_is_current(generation: u64) -> bool {
    CUE_GENERATION.load(Ordering::SeqCst) == generation
}

/// Best-effort asynchronous playback on a detached worker thread.
fn play_cue(tones: Vec<CueTone>, generation: u64) {
    if tones.is_empty() {
        return;
    }
    std::thread::Builder::new()
        .name("openless-audio-cue".to_string())
        .spawn(move || {
            if let Err(error) = play_cue_blocking(&tones, generation) {
                log::debug!("[audio-cue] cue playback unavailable: {error}");
            }
        })
        .map_err(|error| log::debug!("[audio-cue] failed to spawn cue thread: {error}"))
        .ok();
}

#[cfg(target_os = "linux")]
fn play_cue_blocking(tones: &[CueTone], generation: u64) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no Linux default output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|error| format!("default output config failed: {error}"))?;
    let sample_format = supported.sample_format();
    let sample_rate = supported.sample_rate();
    let channels = usize::from(supported.channels()).max(1);
    let config: cpal::StreamConfig = supported.into();
    let mono = Arc::new(render_cue_mono(tones, sample_rate));
    if mono.is_empty() {
        return Ok(());
    }
    let idx = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let build = |format: cpal::SampleFormat| -> Result<cpal::Stream, cpal::Error> {
        macro_rules! make {
            ($ty:ty, $convert:expr) => {{
                let mono = Arc::clone(&mono);
                let idx = Arc::clone(&idx);
                let done = Arc::clone(&done);
                let device = &device;
                let config = &config;
                device.build_output_stream::<$ty, _, _>(
                    *config,
                    move |data: &mut [$ty], _: &cpal::OutputCallbackInfo| {
                        // 代次不再是最新（被新的提示音接管，或录音结束调了
                        // `play_cue_stop`）→ 立刻静音并结束，等价 Tauri 的
                        // `stopVoices()`。
                        if !cue_generation_is_current(generation) {
                            for sample in data.iter_mut() {
                                *sample = $convert(0.0);
                            }
                            done.store(true, std::sync::atomic::Ordering::Release);
                            return;
                        }
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
fn play_cue_blocking(_tones: &[CueTone], _generation: u64) -> Result<(), String> {
    Err("audio cue playback is only available on Linux".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 Tauri `recordStartCueTones()`（`src/lib/audioCue.ts:29-34`）逐字段一致。
    #[test]
    fn start_cue_matches_the_tauri_tone_table() {
        let tones = start_cue_tones();
        assert_eq!(tones.len(), 2, "the Tauri cue has two tones");
        assert_eq!(tones[0].freq_hz, 880.0);
        assert_eq!(tones[0].start_ms, 0.0);
        assert_eq!(tones[0].duration_ms, 130.0);
        assert_eq!(tones[0].peak_gain, 0.16);
        assert_eq!(tones[1].freq_hz, 1108.73, "rising minor third A5 -> C#6");
        assert_eq!(tones[1].start_ms, 95.0);
        assert_eq!(tones[1].duration_ms, 170.0);
        assert_eq!(tones[1].peak_gain, 0.18);
        assert_eq!(
            cue_total_duration_ms(&tones),
            265,
            "total duration is 265ms"
        );
    }

    /// 包络复现 Web Audio 的两段指数斜坡：0.0001 → 5ms 到峰值 → 到末尾回到 0.0001。
    #[test]
    fn envelope_is_two_exponential_ramps_like_web_audio() {
        let sr = 48_000u32;
        let tone = CueTone {
            freq_hz: 880.0,
            start_ms: 0.0,
            duration_ms: 130.0,
            peak_gain: 0.16,
        };
        // 用 1 Hz 的音来取包络：sin(2pi * 1 * t) 在 t<0.25s 内近似线性递增，
        // 便于直接看曲线；这里改为直接验证端点与峰值位置。
        let mono = render_cue_mono(&[tone], sr);
        let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.05 && peak <= 0.17,
            "peak should sit near peak_gain: {peak}"
        );
        // 起点的指数 attack 从 0.0001 开始 → 首样本几乎无声。
        assert!(
            mono[0].abs() < 0.01,
            "first sample starts at CUE_FLOOR: {}",
            mono[0]
        );
        // 末尾回到 floor → 最后一个样本远小于峰值。
        let last = mono[mono.len() - 1].abs();
        assert!(
            last < peak * 0.5,
            "tail decays towards the floor: {last} vs peak {peak}"
        );
        // 5ms 之后就应在衰减（第 5ms 样本的包络低于峰值）。
        let at_5ms = (0.005 * sr as f32) as usize;
        assert!(
            mono[at_5ms].abs() < peak,
            "exponential decay starts after 5ms"
        );
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

    /// `play_cue_stop` 对应 Tauri `stopAudioCue()`：只停不发声（作废代次），
    /// 因此不能合成任何“结束音”。
    #[test]
    fn stopping_a_cue_only_silences_it() {
        let before = CUE_GENERATION.load(Ordering::SeqCst);
        play_cue_stop();
        let after = CUE_GENERATION.load(Ordering::SeqCst);
        assert_eq!(after, before + 1, "stop only bumps the generation");
        assert!(
            !cue_generation_is_current(before),
            "the previous generation must be invalidated (an in-flight cue is silenced)"
        );
    }

    #[test]
    fn empty_tones_render_to_empty_and_play_is_a_noop() {
        assert!(render_cue_mono(&[], 44_100).is_empty());
        let generation = CUE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        play_cue(Vec::new(), generation);
    }

    #[test]
    fn mono_cue_respects_sample_rate_scaling() {
        let at_44k = render_cue_mono(&start_cue_tones(), 44_100);
        let at_48k = render_cue_mono(&start_cue_tones(), 48_000);
        // Higher sample rate yields proportionally more samples for the same cue.
        assert!(at_48k.len() > at_44k.len());
    }
}
