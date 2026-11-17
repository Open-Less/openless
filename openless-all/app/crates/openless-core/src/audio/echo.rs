//! Timestamped playback reference and optional software cancellation.
use super::{PcmNormalizer, DICTATION_SAMPLE_RATE};
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

struct ReferenceChunk {
    captured_at: Instant,
    samples: Vec<f32>,
}
struct ReferenceState {
    chunks: VecDeque<ReferenceChunk>,
    wait_for_callback: bool,
}

/// Two seconds of transient playback. Hosts map hardware capture timestamps to
/// `Instant`; callback arrival order must never determine audio alignment.
pub struct ReferenceAudioBuffer {
    sample_rate: u32,
    state: Mutex<ReferenceState>,
    ready: Condvar,
}
impl ReferenceAudioBuffer {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            state: Mutex::new(ReferenceState {
                chunks: VecDeque::new(),
                wait_for_callback: true,
            }),
            ready: Condvar::new(),
        }
    }
    pub fn push_interleaved_at(&self, samples: &[f32], channels: usize, captured_at: Instant) {
        if channels == 0 || self.sample_rate == 0 || samples.len() < channels {
            return;
        }
        let mut mono = samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().map(|&s| finite_sample(s)).sum::<f32>() / channels as f32)
            .collect::<Vec<_>>();
        let discard = mono.len().saturating_sub(self.sample_rate as usize * 2);
        mono.drain(..discard);
        let captured_at = captured_at + sample_duration(discard, self.sample_rate);
        let end = captured_at + sample_duration(mono.len(), self.sample_rate);
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        if state
            .chunks
            .back()
            .is_some_and(|last| captured_at < last.captured_at)
        {
            state.chunks.clear();
        }
        state.chunks.push_back(ReferenceChunk {
            captured_at,
            samples: mono,
        });
        while state.chunks.len() > 1
            && state.chunks.front().is_some_and(|first| {
                end.saturating_duration_since(first.captured_at) > Duration::from_secs(2)
            })
        {
            state.chunks.pop_front();
        }
        state.wait_for_callback = true;
        self.ready.notify_one();
    }
    /// A discontinuity invalidates the learned path; fresh callbacks can resume it.
    pub fn invalidate(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.chunks.clear();
        state.wait_for_callback = false;
        self.ready.notify_one();
    }
    /// Sample playback at microphone frame times, retaining fractional positions
    /// rather than rounding up and consuming an extra source sample per callback.
    pub fn take_for_input_at(
        &self,
        frames: usize,
        rate: u32,
        captured_at: Instant,
    ) -> Option<Vec<f32>> {
        if frames == 0 || rate == 0 || self.sample_rate == 0 {
            return None;
        }
        let last_time = captured_at + sample_duration(frames - 1, rate);
        let deadline = Instant::now() + Duration::from_millis(100);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if state
                .chunks
                .front()
                .is_some_and(|first| captured_at < first.captured_at)
            {
                return None;
            }
            if state.chunks.back().is_some_and(|last| {
                last_time < last.captured_at + sample_duration(last.samples.len(), self.sample_rate)
            }) {
                break;
            }
            if !state.wait_for_callback {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.wait_for_callback = false;
                return None;
            }
            let (next, timeout) = self
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            if timeout.timed_out() {
                state.wait_for_callback = false;
                return None;
            }
        }
        let mut chunk_index = 0;
        let mut output = Vec::with_capacity(frames);
        for frame in 0..frames {
            let time = captured_at + sample_duration(frame, rate);
            loop {
                let chunk = state.chunks.get(chunk_index)?;
                let offset = time
                    .checked_duration_since(chunk.captured_at)?
                    .as_secs_f64()
                    * self.sample_rate as f64;
                let index = offset.floor() as usize;
                if index >= chunk.samples.len() {
                    chunk_index += 1;
                    continue;
                }
                let left = chunk.samples[index];
                let right = chunk.samples.get(index + 1).copied().unwrap_or(left);
                output.push(left + (right - left) * offset.fract() as f32);
                break;
            }
        }
        Some(output)
    }
}
fn sample_duration(frames: usize, rate: u32) -> Duration {
    Duration::from_nanos(frames as u64 * 1_000_000_000 / u64::from(rate))
}
fn finite_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[derive(Default)]
pub(super) struct EchoProcessor {
    microphone: PcmNormalizer,
    reference: PcmNormalizer,
    canceller: EchoCanceller,
    rate: u32,
}
impl EchoProcessor {
    pub fn process(
        &mut self,
        input: &[f32],
        channels: usize,
        rate: u32,
        reference: Option<&[f32]>,
    ) -> Vec<f32> {
        if channels == 0 || rate == 0 || input.len() < channels {
            return Vec::new();
        }
        if rate != self.rate {
            *self = Self::default();
            self.rate = rate;
        }
        let microphone = input
            .chunks_exact(channels)
            .map(|frame| frame.iter().map(|&s| finite_sample(s)).sum::<f32>() / channels as f32)
            .collect::<Vec<_>>();
        let available = reference.filter(|samples| samples.len() == microphone.len());
        let render = match available {
            Some(samples) => samples
                .iter()
                .map(|&s| finite_sample(s))
                .collect::<Vec<_>>(),
            None => vec![0.0; microphone.len()],
        };
        let microphone = self
            .microphone
            .resample(&microphone, rate, DICTATION_SAMPLE_RATE);
        // Advance both resamplers through a gap, but never train on synthetic silence.
        let render = self
            .reference
            .resample(&render, rate, DICTATION_SAMPLE_RATE);
        if available.is_none() {
            self.canceller.reset();
            return microphone;
        }
        self.canceller.process(&microphone, &render)
    }
}
const ECHO_FILTER_TAPS: usize = 1_024;
const ECHO_ADAPTATION_RATE: f32 = 0.12;
const ECHO_DOUBLE_TALK_RATIO: f32 = 2.5;
const ECHO_MIN_REFERENCE_POWER: f32 = 0.000_001;

/// Conservative normalized-LMS echo canceller for a mono render reference.
///
/// The filter is intentionally bounded and stateful. It learns the acoustic path while
/// the render signal is present, but pauses adaptation when near-end energy is clearly
/// stronger than the estimated echo (double talk). It is not a replacement for a
/// platform voice-processing stack; hosts therefore treat it as an optional best-effort
/// processor and bypass it when the render reference is unavailable.
#[derive(Debug, Clone)]
struct EchoCanceller {
    coefficients: Vec<f32>,
    history: Vec<f32>,
    history_cursor: usize,
}

impl Default for EchoCanceller {
    fn default() -> Self {
        Self {
            coefficients: vec![0.0; ECHO_FILTER_TAPS],
            history: vec![0.0; ECHO_FILTER_TAPS],
            history_cursor: 0,
        }
    }
}

impl EchoCanceller {
    fn reset(&mut self) {
        self.coefficients.fill(0.0);
        self.history.fill(0.0);
        self.history_cursor = 0;
    }

    fn process(&mut self, microphone: &[f32], reference: &[f32]) -> Vec<f32> {
        if microphone.is_empty() || reference.is_empty() {
            return microphone.to_vec();
        }
        let mut output = Vec::with_capacity(microphone.len());
        for (index, &sample) in microphone.iter().enumerate() {
            let Some(&reference_sample) = reference.get(index) else {
                // A resampler may produce one fewer sample at a chunk boundary. Do not
                // repeat the last render value across the missing tail; preserve the
                // microphone sample and let the next reference chunk re-establish alignment.
                output.push(sample);
                continue;
            };
            self.history[self.history_cursor] = reference_sample;
            let mut estimate = 0.0;
            let mut reference_power = 0.0;
            for tap in 0..ECHO_FILTER_TAPS {
                let history_index =
                    (self.history_cursor + ECHO_FILTER_TAPS - tap) % ECHO_FILTER_TAPS;
                let value = self.history[history_index];
                estimate += self.coefficients[tap] * value;
                reference_power += value * value;
            }
            if !estimate.is_finite() {
                self.reset();
                output.push(sample);
                continue;
            }
            let error = sample - estimate;
            let near_power = sample * sample;
            let should_adapt = reference_power > ECHO_MIN_REFERENCE_POWER
                && (estimate.abs() < 0.01
                    || estimate * estimate * ECHO_DOUBLE_TALK_RATIO >= near_power
                    || near_power < ECHO_MIN_REFERENCE_POWER);
            if should_adapt {
                let normalization = reference_power + 0.000_01;
                for tap in 0..ECHO_FILTER_TAPS {
                    let history_index =
                        (self.history_cursor + ECHO_FILTER_TAPS - tap) % ECHO_FILTER_TAPS;
                    self.coefficients[tap] +=
                        ECHO_ADAPTATION_RATE * error * self.history[history_index] / normalization;
                    self.coefficients[tap] = self.coefficients[tap].clamp(-2.0, 2.0);
                }
            }
            output.push(if error.is_finite() { error } else { 0.0 });
            self.history_cursor = (self.history_cursor + 1) % ECHO_FILTER_TAPS;
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioProcessingConfig, AudioProcessor};
    #[test]
    fn non_finite_microphone_samples_do_not_poison_future_audio() {
        let mut p = AudioProcessor::new(AudioProcessingConfig {
            echo_cancellation_enabled: true,
            ..Default::default()
        });
        let far = vec![0.1; 160];
        let mut mic = vec![0.2; 160];
        mic[0] = f32::NAN;
        mic[1] = f32::INFINITY;
        p.process_with_reference(&mic, 1, 16_000, Some(&far))
            .unwrap();
        let out = p
            .process_with_reference(&vec![0.2; 160], 1, 16_000, Some(&far))
            .unwrap();
        assert!(out
            .pcm_i16_le
            .as_chunks::<2>()
            .0
            .iter()
            .any(|b| i16::from_le_bytes([b[0], b[1]]) != 0));
        assert!(out.level.is_finite());
    }
    #[test]
    fn reference_conversion_has_no_rounding_drift_or_startup_backlog() {
        let epoch = Instant::now();
        let b = ReferenceAudioBuffer::new(48_000);
        b.push_interleaved_at(&vec![-0.75; 4800], 1, epoch);
        let start = epoch + Duration::from_millis(100);
        let mut published = 0_u64;
        for callback in 0..4000_u64 {
            let total = ((callback + 1) * 512 * 48_000).div_ceil(44_100);
            b.push_interleaved_at(
                &vec![0.25; (total - published) as usize],
                1,
                start + sample_duration(published as usize, 48_000),
            );
            published = total;
            let chunk = b
                .take_for_input_at(
                    512,
                    44_100,
                    start + sample_duration(callback as usize * 512, 44_100),
                )
                .expect("equal-duration streams must stay aligned");
            assert_eq!(chunk.len(), 512);
            assert!(chunk.iter().all(|&s| (s - 0.25).abs() < 1e-5));
        }
        assert!(
            b.state
                .lock()
                .unwrap()
                .chunks
                .iter()
                .map(|c| c.samples.len())
                .sum::<usize>()
                <= 96_558
        );
    }
    #[test]
    fn reference_gap_does_not_reuse_stale_samples() {
        let epoch = Instant::now();
        let b = ReferenceAudioBuffer::new(16_000);
        b.push_interleaved_at(&[0.25; 80], 1, epoch);
        b.push_interleaved_at(&[0.75; 160], 1, epoch + Duration::from_millis(10));
        assert!(b.take_for_input_at(160, 16_000, epoch).is_none());
        assert_eq!(
            b.take_for_input_at(160, 16_000, epoch + Duration::from_millis(10))
                .unwrap(),
            vec![0.75; 160]
        );
        b.invalidate();
        assert!(b
            .take_for_input_at(160, 16_000, epoch + Duration::from_millis(10))
            .is_none());
    }
    #[test]
    fn reference_gaps_preserve_dsp_format_and_tail_duration() {
        for rate in [16_000, 44_100, 48_000] {
            let mut p = AudioProcessor::new(AudioProcessingConfig {
                enhancement_enabled: true,
                echo_cancellation_enabled: true,
                ..Default::default()
            });
            let mut samples = 0;
            for available in [true, false, true, false] {
                let far = vec![0.0; 317];
                if let Some(out) = p.process_with_reference(
                    &vec![0.25; 317],
                    1,
                    rate,
                    available.then_some(far.as_slice()),
                ) {
                    samples += out.pcm_i16_le.len() / 2;
                }
            }
            if let Some(tail) = p.finish() {
                samples += tail.pcm_i16_le.len() / 2;
            }
            assert_eq!(
                samples,
                (4_u64 * 317 * 16_000).div_ceil(u64::from(rate)) as usize
            );
            assert!(p.finish().is_none());
        }
    }
    #[test]
    fn startup_offset_is_aligned_before_the_causal_filter() {
        let epoch = Instant::now();
        let b = ReferenceAudioBuffer::new(16_000);
        let mut seed = 0x91234567_u32;
        let far = (0..40000)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 8) as f32 / 16777216.0 - 0.5) * 0.4
            })
            .collect::<Vec<_>>();
        let mut c = EchoCanceller::default();
        let mut input_power = 0.0;
        let mut output_power = 0.0;
        for callback in 0..200 {
            let time = 2400 + callback * 160;
            let start = if callback == 0 { 0 } else { time };
            b.push_interleaved_at(
                &far[start..time + 160],
                1,
                epoch + sample_duration(start, 16_000),
            );
            let r = b
                .take_for_input_at(160, 16_000, epoch + sample_duration(time, 16_000))
                .unwrap();
            let mic = far[time - 160..time]
                .iter()
                .map(|s| s * 0.7)
                .collect::<Vec<_>>();
            let out = c.process(&mic, &r);
            if callback >= 100 {
                input_power += mic.iter().map(|s| s * s).sum::<f32>();
                output_power += out.iter().map(|s| s * s).sum::<f32>();
            }
        }
        assert!(
            output_power < input_power * 0.5,
            "{output_power} >= {input_power}"
        );
    }
    #[test]
    fn echo_canceller_reduces_delayed_render_signal() {
        let mut canceller = EchoCanceller::default();
        let mut reference = vec![0.0_f32; 16_000];
        for (index, sample) in reference.iter_mut().enumerate() {
            *sample = (index as f32 * 0.071).sin() * 0.25;
        }
        let delay = 96;
        let microphone = reference
            .iter()
            .enumerate()
            .map(|(index, _)| {
                reference
                    .get(index.saturating_sub(delay))
                    .copied()
                    .unwrap_or_default()
                    * 0.7
            })
            .collect::<Vec<_>>();
        let mut cancelled = Vec::new();
        for (mic, far) in microphone.chunks(160).zip(reference.chunks(160)) {
            cancelled.extend(canceller.process(mic, far));
        }
        let input_rms = microphone[2_000..]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>();
        let output_rms = cancelled[2_000..]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>();
        assert!(output_rms < input_rms * 0.35, "{output_rms} >= {input_rms}");
    }

    #[test]
    fn echo_canceller_preserves_near_end_during_double_talk() {
        let mut canceller = EchoCanceller::default();
        let mut reference = vec![0.0_f32; 12_000];
        for (index, sample) in reference.iter_mut().enumerate() {
            // Deterministic broadband-ish reference gives the adaptive filter enough
            // information to learn the delayed acoustic path.
            *sample = (((index as u32)
                .wrapping_mul(1_103_515_245)
                .wrapping_add(12_345)
                >> 16) as f32
                / 32_768.0
                - 1.0)
                * 0.2;
        }
        let delay = 96;
        let echo = |index: usize| {
            reference
                .get(index.saturating_sub(delay))
                .copied()
                .unwrap_or_default()
                * 0.7
        };
        for (index, far) in reference.chunks(160).enumerate() {
            let start = index * 160;
            let microphone = (start..start + far.len()).map(echo).collect::<Vec<_>>();
            let _ = canceller.process(&microphone, far);
        }

        let mut output = Vec::new();
        let mut near_end = Vec::new();
        for (index, far) in reference[6_000..].chunks(160).enumerate() {
            let start = 6_000 + index * 160;
            let near = (start..start + far.len())
                .map(|sample| (sample as f32 * 0.017).sin() * 0.22)
                .collect::<Vec<_>>();
            let microphone = far
                .iter()
                .enumerate()
                .map(|(offset, _sample)| echo(start + offset) + near[offset])
                .collect::<Vec<_>>();
            output.extend(canceller.process(&microphone, far));
            near_end.extend(near);
        }
        let dot = output
            .iter()
            .zip(&near_end)
            .map(|(processed, near)| processed * near)
            .sum::<f32>();
        let near_power = near_end.iter().map(|sample| sample * sample).sum::<f32>();
        assert!(
            dot > near_power * 0.35,
            "near-end correlation was suppressed"
        );
    }
}
