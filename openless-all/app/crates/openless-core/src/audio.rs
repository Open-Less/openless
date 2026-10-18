//! Framework-independent audio normalization used by native recorder adapters.

pub const DICTATION_SAMPLE_RATE: u32 = 16_000;
const LEVEL_RMS_GAIN: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedPcmChunk {
    pub pcm_i16_le: Vec<u8>,
    pub level: f32,
}

/// Session-frozen processing choices for native microphone input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioProcessingConfig {
    pub enhancement_enabled: bool,
}

/// Small, deterministic microphone enhancement that does not require a model
/// or a platform-specific audio dependency.
#[derive(Debug, Clone, Copy)]
pub struct AudioEnhancer {
    enabled: bool,
    noise_suppressor: NoiseSuppressor,
}

#[derive(Debug, Clone, Copy)]
struct NoiseSuppressor {
    enabled: bool,
    noise_floor: f32,
    gain: f32,
    gate_open: bool,
    hold_remaining_secs: f32,
}

impl NoiseSuppressor {
    const INITIAL_NOISE_FLOOR: f32 = 0.008;
    const MIN_THRESHOLD: f32 = 0.012;
    const CLOSE_THRESHOLD_MULTIPLIER: f32 = 2.0;
    const OPEN_THRESHOLD_MULTIPLIER: f32 = 2.5;
    const ATTENUATED_GAIN: f32 = 0.25;
    const NOISE_FLOOR_DECAY_SECS: f32 = 0.25;
    const NOISE_FLOOR_RISE_SECS: f32 = 8.0;
    const HOLD_SECS: f32 = 0.050;
    const GAIN_ATTACK_SECS: f32 = 0.010;
    const GAIN_RELEASE_SECS: f32 = 0.080;

    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            noise_floor: Self::INITIAL_NOISE_FLOOR,
            gain: 1.0,
            gate_open: false,
            hold_remaining_secs: 0.0,
        }
    }

    fn reset(&mut self) {
        self.noise_floor = Self::INITIAL_NOISE_FLOOR;
        self.gain = 1.0;
        self.gate_open = false;
        self.hold_remaining_secs = 0.0;
    }

    fn process_in_place(&mut self, samples: &mut [f32], channels: usize, sample_rate: u32) {
        if !self.enabled || samples.is_empty() || channels == 0 || sample_rate == 0 {
            return;
        }
        let frame_count = samples.len() / channels;
        if frame_count == 0 {
            return;
        }
        let processed_samples = frame_count * channels;
        let rms = downmixed_rms(&samples[..processed_samples], channels);
        let duration_secs = frame_count as f32 / sample_rate as f32;
        let close_threshold =
            (self.noise_floor * Self::CLOSE_THRESHOLD_MULTIPLIER).max(Self::MIN_THRESHOLD);
        let open_threshold = (self.noise_floor * Self::OPEN_THRESHOLD_MULTIPLIER)
            .max(close_threshold * 1.5)
            .max(Self::MIN_THRESHOLD);

        if rms <= close_threshold {
            let adaptation_secs = if rms < self.noise_floor {
                Self::NOISE_FLOOR_DECAY_SECS
            } else {
                Self::NOISE_FLOOR_RISE_SECS
            };
            let coefficient = 1.0 - (-duration_secs / adaptation_secs).exp();
            self.noise_floor += (rms - self.noise_floor) * coefficient.clamp(0.0, 1.0);
            self.noise_floor = self.noise_floor.max(0.0);
        }

        if self.gate_open {
            if rms <= close_threshold {
                self.hold_remaining_secs -= duration_secs;
                if self.hold_remaining_secs <= 0.0 {
                    self.gate_open = false;
                    self.hold_remaining_secs = 0.0;
                }
            } else {
                self.hold_remaining_secs = Self::HOLD_SECS;
            }
        } else if rms >= open_threshold {
            self.gate_open = true;
            self.hold_remaining_secs = Self::HOLD_SECS;
        }

        let target_gain = if self.gate_open {
            1.0
        } else if rms <= close_threshold {
            Self::ATTENUATED_GAIN
        } else {
            let knee =
                (rms - close_threshold) / (open_threshold - close_threshold).max(f32::EPSILON);
            Self::ATTENUATED_GAIN + (1.0 - Self::ATTENUATED_GAIN) * knee.clamp(0.0, 1.0)
        };
        let smoothing_secs = if target_gain < self.gain {
            Self::GAIN_RELEASE_SECS
        } else {
            Self::GAIN_ATTACK_SECS
        };
        let coefficient = 1.0 - (-duration_secs / smoothing_secs).exp();
        self.gain += (target_gain - self.gain) * coefficient.clamp(0.0, 1.0);

        for sample in &mut samples[..processed_samples] {
            if sample.is_finite() {
                *sample *= self.gain;
            } else {
                *sample = 0.0;
            }
        }
    }
}

impl Default for NoiseSuppressor {
    fn default() -> Self {
        Self::new(false)
    }
}

fn downmixed_rms(samples: &[f32], channels: usize) -> f32 {
    let frame_count = samples.len() / channels;
    if frame_count == 0 {
        return 0.0;
    }
    let square_sum = samples
        .chunks_exact(channels)
        .map(|frame| {
            let mono = frame
                .iter()
                .map(|sample| if sample.is_finite() { *sample } else { 0.0 })
                .sum::<f32>()
                / channels as f32;
            f64::from(mono) * f64::from(mono)
        })
        .sum::<f64>();
    (square_sum / frame_count as f64).sqrt() as f32
}

impl Default for AudioEnhancer {
    fn default() -> Self {
        Self::new(AudioProcessingConfig::default())
    }
}

impl AudioEnhancer {
    const INPUT_GAIN: f32 = 1.5;
    const SOFT_CLIP_KNEE: f32 = 0.75;

    pub fn new(config: AudioProcessingConfig) -> Self {
        Self {
            enabled: config.enhancement_enabled,
            noise_suppressor: NoiseSuppressor::new(config.enhancement_enabled),
        }
    }

    pub fn reset(&mut self) {
        self.noise_suppressor.reset();
    }

    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        self.process_in_place_with_format(samples, 1, DICTATION_SAMPLE_RATE);
    }

    fn process_in_place_with_format(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        sample_rate: u32,
    ) {
        if !self.enabled {
            return;
        }
        self.noise_suppressor
            .process_in_place(samples, channels, sample_rate);
        for sample in samples {
            let boosted = *sample * Self::INPUT_GAIN;
            *sample = Self::soft_clip(boosted);
        }
    }

    fn soft_clip(sample: f32) -> f32 {
        let sample = sample.clamp(-1.0, 1.0);
        let magnitude = sample.abs();
        if magnitude <= Self::SOFT_CLIP_KNEE {
            return sample;
        }
        let normalized = (magnitude - Self::SOFT_CLIP_KNEE) / (1.0 - Self::SOFT_CLIP_KNEE);
        let eased = normalized * normalized * (3.0 - 2.0 * normalized);
        sample.signum() * (Self::SOFT_CLIP_KNEE + (1.0 - Self::SOFT_CLIP_KNEE) * eased)
    }
}

/// Shared stateful processor used by native microphone hosts.
#[derive(Debug)]
pub struct AudioProcessor {
    normalizer: PcmNormalizer,
    enhancer: AudioEnhancer,
}

impl AudioProcessor {
    pub fn new(config: AudioProcessingConfig) -> Self {
        Self {
            normalizer: PcmNormalizer::default(),
            enhancer: AudioEnhancer::new(config),
        }
    }

    pub fn reset(&mut self) {
        self.normalizer = PcmNormalizer::default();
        self.enhancer.reset();
    }

    pub fn process(
        &mut self,
        interleaved: &[f32],
        channels: usize,
        input_sample_rate: u32,
    ) -> Option<NormalizedPcmChunk> {
        if !self.enhancer.enabled {
            return self
                .normalizer
                .process(interleaved, channels, input_sample_rate);
        }
        let mut enhanced = interleaved.to_vec();
        self.enhancer
            .process_in_place_with_format(&mut enhanced, channels, input_sample_rate);
        self.normalizer
            .process(&enhanced, channels, input_sample_rate)
    }
}

/// Stateful interleaved-audio normalizer.
///
/// Native adapters convert their device sample format to `f32` and pass each
/// callback here. The output contract is always 16 kHz, mono, signed 16-bit
/// little-endian PCM plus the UI level used by both hosts.
#[derive(Debug, Default)]
pub struct PcmNormalizer {
    resample_phase: f64,
    last_sample: f32,
}

pub fn encode_dictation_wav(pcm_i16_le: &[u8]) -> Result<Vec<u8>, crate::BackendError> {
    if !pcm_i16_le.len().is_multiple_of(2) {
        return Err(crate::BackendError::new(
            crate::BackendErrorCode::InvalidArgument,
            "dictation PCM length must be a multiple of two bytes",
        ));
    }
    let data_size = u32::try_from(pcm_i16_le.len()).map_err(|_| {
        crate::BackendError::new(
            crate::BackendErrorCode::InvalidArgument,
            "dictation PCM is too large for a WAV container",
        )
    })?;
    let riff_size = 36_u32.checked_add(data_size).ok_or_else(|| {
        crate::BackendError::new(
            crate::BackendErrorCode::InvalidArgument,
            "dictation WAV size overflow",
        )
    })?;
    let byte_rate = DICTATION_SAMPLE_RATE * 2;
    let mut wav = Vec::with_capacity(44 + pcm_i16_le.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&DICTATION_SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm_i16_le);
    Ok(wav)
}

impl PcmNormalizer {
    pub fn process(
        &mut self,
        interleaved: &[f32],
        channels: usize,
        input_sample_rate: u32,
    ) -> Option<NormalizedPcmChunk> {
        if interleaved.is_empty() || channels == 0 || input_sample_rate == 0 {
            return None;
        }
        let mono = downmix_to_mono(interleaved, channels);
        let resampled = self.resample(&mono, input_sample_rate, DICTATION_SAMPLE_RATE);
        if resampled.is_empty() {
            return None;
        }
        let (pcm_i16_le, rms) = quantize_to_i16_le(&resampled);
        Some(NormalizedPcmChunk {
            pcm_i16_le,
            level: (rms * LEVEL_RMS_GAIN).clamp(0.0, 1.0),
        })
    }

    fn resample(&mut self, samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
        if samples.is_empty() {
            return Vec::new();
        }
        if source_rate == target_rate {
            self.last_sample = *samples.last().unwrap_or(&0.0);
            return samples.to_vec();
        }

        let step = source_rate as f64 / target_rate as f64;
        let mut phase = self.resample_phase;
        let mut output = Vec::with_capacity(
            ((samples.len() as f64) / step).ceil() as usize + usize::from(step < 1.0),
        );
        while phase < samples.len() as f64 {
            let floor = phase.floor() as isize;
            let fraction = (phase - phase.floor()) as f32;
            let left = if floor < 0 {
                self.last_sample
            } else {
                samples[floor as usize]
            };
            let right_index = (floor + 1) as usize;
            if right_index >= samples.len() {
                output.push(left);
                phase += step;
                break;
            }
            let right = samples[right_index];
            output.push(left + (right - left) * fraction);
            phase += step;
        }
        self.resample_phase = (phase - samples.len() as f64).max(0.0);
        self.last_sample = *samples.last().unwrap_or(&0.0);
        output
    }
}

fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

fn quantize_to_i16_le(samples: &[f32]) -> (Vec<u8>, f32) {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    let mut square_sum = 0.0_f64;
    for sample in samples {
        let normalized = sample.clamp(-1.0, 1.0);
        let quantized = (normalized * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&quantized.to_le_bytes());
        square_sum += f64::from(normalized) * f64::from(normalized);
    }
    let rms = (square_sum / samples.len() as f64).sqrt() as f32;
    (bytes, rms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> Vec<i16> {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
    }

    #[test]
    fn normalizer_downmixes_quantizes_and_scales_level() {
        let mut normalizer = PcmNormalizer::default();
        let output = normalizer
            .process(&[1.0, -1.0, 0.5, 0.5], 2, DICTATION_SAMPLE_RATE)
            .unwrap();
        assert_eq!(decode(&output.pcm_i16_le), vec![0, 16383]);
        assert_eq!(output.level, 1.0);
    }

    #[test]
    fn normalizer_resamples_to_sixteen_kilohertz_across_callbacks() {
        let mut normalizer = PcmNormalizer::default();
        let first = normalizer.process(&[0.0, 1.0], 1, 8_000).unwrap();
        let second = normalizer.process(&[1.0, 0.0], 1, 8_000).unwrap();
        assert_eq!(decode(&first.pcm_i16_le), vec![0, 16383, 32767]);
        assert_eq!(decode(&second.pcm_i16_le), vec![32767, 16383, 0]);
    }

    #[test]
    fn normalizer_rejects_empty_or_invalid_input_without_emitting_pcm() {
        let mut normalizer = PcmNormalizer::default();
        assert!(normalizer.process(&[], 1, 48_000).is_none());
        assert!(normalizer.process(&[0.0], 0, 48_000).is_none());
        assert!(normalizer.process(&[0.0], 1, 0).is_none());
    }

    #[test]
    fn enhancement_boosts_quiet_samples_and_soft_clips_loud_samples() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let mut samples = [0.5, -0.5, 1.0, -1.0];
        enhancer.process_in_place(&mut samples);
        assert_eq!(samples[0], 0.75);
        assert_eq!(samples[1], -0.75);
        assert_eq!(samples[2], 1.0);
        assert_eq!(samples[3], -1.0);
    }

    #[test]
    fn enhancement_replaces_non_finite_samples_and_stays_in_range() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let mut samples = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0, -2.0];
        enhancer.process_in_place(&mut samples);
        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        assert_eq!(samples[0], 0.0);
    }

    #[test]
    fn noise_suppression_attenuates_quiet_frames_and_recovers_for_speech() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let quiet = [0.004_f32; 160];
        let mut processed_quiet = quiet;
        for _ in 0..24 {
            processed_quiet = quiet;
            enhancer.process_in_place(&mut processed_quiet);
        }
        assert!(processed_quiet[0].abs() < quiet[0].abs() * 0.5);

        let mut speech = [0.2_f32; 160];
        enhancer.process_in_place(&mut speech);
        assert!(speech[0] > 0.1);
    }

    #[test]
    fn noise_suppression_reset_restores_initial_gain_state() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let quiet = [0.004_f32; 160];
        let mut settled = quiet;
        for _ in 0..24 {
            enhancer.process_in_place(&mut settled);
        }
        enhancer.reset();
        let mut after_reset = quiet;
        enhancer.process_in_place(&mut after_reset);
        assert!(after_reset[0] > settled[0]);
    }

    #[test]
    fn noise_suppression_keeps_stereo_frame_processing_consistent() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let mut samples = [0.004_f32; 320];
        enhancer.process_in_place_with_format(&mut samples, 2, DICTATION_SAMPLE_RATE);
        assert_eq!(samples[0], samples[1]);
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn noise_suppression_does_not_learn_sustained_speech_as_noise() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        for _ in 0..1_000 {
            let mut speech = [0.2_f32; 160];
            enhancer.process_in_place(&mut speech);
            assert!(speech[0] > 0.25);
        }
    }

    #[test]
    fn noise_suppression_uses_downmixed_rms_for_stereo_input() {
        let mut enhancer = AudioEnhancer::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let mut out_of_phase = [0.2_f32; 320];
        for frame in out_of_phase.as_chunks_mut::<2>().0 {
            frame[1] = -0.2;
        }
        for _ in 0..24 {
            enhancer.process_in_place_with_format(&mut out_of_phase, 2, DICTATION_SAMPLE_RATE);
        }
        assert!(out_of_phase[0].abs() < 0.15);
        assert_eq!(out_of_phase[0], -out_of_phase[1]);
    }

    #[test]
    fn processor_disabled_output_matches_normalizer() {
        let input = [0.0, 1.0, 0.5, -0.5];
        let mut processor = AudioProcessor::new(AudioProcessingConfig::default());
        let mut normalizer = PcmNormalizer::default();
        assert_eq!(
            processor.process(&input, 2, DICTATION_SAMPLE_RATE),
            normalizer.process(&input, 2, DICTATION_SAMPLE_RATE)
        );
    }

    #[test]
    fn processor_applies_noise_suppression_to_worker_facing_output() {
        let quiet = [0.004_f32; 160];
        let mut processor = AudioProcessor::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let mut output = None;
        for _ in 0..24 {
            output = processor.process(&quiet, 1, DICTATION_SAMPLE_RATE);
        }
        let output = output.unwrap();
        let mut normalizer = PcmNormalizer::default();
        let baseline = normalizer
            .process(&quiet, 1, DICTATION_SAMPLE_RATE)
            .unwrap();
        let output_peak = decode(&output.pcm_i16_le)
            .into_iter()
            .map(|sample| i32::from(sample).abs())
            .max()
            .unwrap_or(0);
        let baseline_peak = decode(&baseline.pcm_i16_le)
            .into_iter()
            .map(|sample| i32::from(sample).abs())
            .max()
            .unwrap_or(0);
        assert!(output_peak < baseline_peak);
    }

    #[test]
    fn processor_enhancement_preserves_canonical_output_contract() {
        let mut processor = AudioProcessor::new(AudioProcessingConfig {
            enhancement_enabled: true,
        });
        let output = processor
            .process(&[0.25, -0.25, 0.5, -0.5], 1, DICTATION_SAMPLE_RATE)
            .unwrap();
        assert!(output.pcm_i16_le.len().is_multiple_of(2));
        assert!(output.level.is_finite());
        assert!((0.0..=1.0).contains(&output.level));
        assert_eq!(
            decode(&output.pcm_i16_le),
            vec![12287, -12287, 24575, -24575]
        );
    }

    #[test]
    fn processor_reset_discards_resampling_state() {
        let mut processor = AudioProcessor::new(AudioProcessingConfig::default());
        let _ = processor.process(&[0.0, 1.0], 1, 8_000);
        processor.reset();
        let output = processor.process(&[0.0, 1.0], 1, 8_000).unwrap();
        assert_eq!(decode(&output.pcm_i16_le), vec![0, 16383, 32767]);
    }

    #[test]
    fn wav_encoder_preserves_canonical_pcm_and_rejects_partial_samples() {
        let pcm = [1, 0, 255, 127];
        let wav = encode_dictation_wav(&pcm).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(&wav[44..], &pcm);
        assert_eq!(
            encode_dictation_wav(&[1]).unwrap_err().code,
            crate::BackendErrorCode::InvalidArgument
        );
    }
}
