//! Session-frozen speech enhancement, adapted from MicYou's 48 kHz DSP chain.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoiseReduction {
    Off,
    #[default]
    Rnnoise,
    Purevox,
    Spectral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioDspStage {
    NoiseReduction,
    Dereverb,
    Equalizer,
    Amplifier,
    Agc,
    Vad,
}

impl AudioDspStage {
    fn name(self) -> &'static str {
        match self {
            Self::NoiseReduction => "NoiseReduction",
            Self::Dereverb => "Dereverb",
            Self::Equalizer => "Equalizer",
            Self::Amplifier => "Amplifier",
            Self::Agc => "AGC",
            Self::Vad => "VAD",
        }
    }
}

/// Integer controls make persisted/network settings finite and bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioDspConfig {
    pub noise_reduction: NoiseReduction,
    pub noise_strength: u8,
    pub gain_db: i8,
    pub agc_enabled: bool,
    /// Target peak envelope as a percentage of full scale.
    pub agc_target: u8,
    pub agc_attack: u8,
    pub agc_decay: u8,
    pub vad_enabled: bool,
    pub vad_threshold_db: i8,
    /// Optional comb filter, not a learned room dereverberation model.
    pub dereverb_level: u8,
    pub equalizer_enabled: bool,
    pub equalizer_preamp_db: i8,
    pub equalizer_bands_db: [i8; 10],
    pub processing_order: [AudioDspStage; 6],
    /// Android AudioRecord effects. Other hosts ignore these local choices.
    pub android_aec_enabled: bool,
    pub android_noise_suppression_enabled: bool,
    pub android_agc_enabled: bool,
}

impl Default for AudioDspConfig {
    fn default() -> Self {
        use AudioDspStage::*;
        Self {
            noise_reduction: crate::audio_dsp::NoiseReduction::Rnnoise,
            noise_strength: 80,
            gain_db: 0,
            agc_enabled: true,
            agc_target: 35,
            agc_attack: 50,
            agc_decay: 50,
            vad_enabled: false,
            vad_threshold_db: -50,
            dereverb_level: 0,
            equalizer_enabled: false,
            equalizer_preamp_db: 0,
            equalizer_bands_db: [0; 10],
            processing_order: [NoiseReduction, Dereverb, Equalizer, Amplifier, Agc, Vad],
            android_aec_enabled: false,
            android_noise_suppression_enabled: false,
            android_agc_enabled: false,
        }
    }
}

impl AudioDspConfig {
    pub fn normalized(mut self) -> Self {
        self.noise_strength = self.noise_strength.min(100);
        self.gain_db = self.gain_db.clamp(-50, 50);
        self.agc_target = self.agc_target.min(100);
        self.agc_attack = self.agc_attack.clamp(1, 100);
        self.agc_decay = self.agc_decay.clamp(1, 100);
        self.vad_threshold_db = self.vad_threshold_db.clamp(-100, 0);
        self.dereverb_level = self.dereverb_level.min(100);
        self.equalizer_preamp_db = self.equalizer_preamp_db.clamp(-20, 20);
        self.equalizer_bands_db = self.equalizer_bands_db.map(|gain| gain.clamp(-15, 15));
        if self
            .processing_order
            .iter()
            .enumerate()
            .any(|(i, stage)| self.processing_order[..i].contains(stage))
        {
            self.processing_order = Self::default().processing_order;
        }
        self
    }

    pub(crate) fn model_delay(self) -> usize {
        if self.noise_strength > 0
            && matches!(
                self.noise_reduction,
                NoiseReduction::Rnnoise | NoiseReduction::Purevox
            )
        {
            480
        } else {
            0
        }
    }

    pub(crate) fn settings(self) -> micyou_dsp::AudioDspSettings {
        let c = self.normalized();
        micyou_dsp::AudioDspSettings {
            gain: c.gain_db as f32,
            ns_enabled: c.noise_reduction != NoiseReduction::Off && c.noise_strength > 0,
            ns_type: match c.noise_reduction {
                NoiseReduction::Purevox => "PureVox",
                NoiseReduction::Spectral => "Speexdsp",
                _ => "RNNoise",
            }
            .into(),
            ns_intensity: c.noise_strength as f32,
            dereverb_enabled: c.dereverb_level > 0,
            dereverb_level: c.dereverb_level as f32,
            agc_enabled: c.agc_enabled,
            agc_target: c.agc_target as f32 / 100.0 * 32767.0,
            agc_attack: c.agc_attack as f32,
            agc_decay: c.agc_decay as f32,
            vad_enabled: c.vad_enabled,
            vad_threshold: c.vad_threshold_db as f32,
            equalizer: micyou_dsp::EqualizerConfig {
                enabled: c.equalizer_enabled,
                pre_amp: c.equalizer_preamp_db as f32,
                gains: c.equalizer_bands_db.map(f32::from).to_vec(),
            },
            processing_chain: c.processing_order.map(|stage| stage.name().into()).to_vec(),
            ..Default::default()
        }
    }
}

/// A streaming windowed-sinc resampler. Integer positions avoid callback-size
/// dependent phase drift; lookahead is flushed once at the end of a session.
pub(crate) struct SpeechResampler {
    source_rate: u32,
    target_rate: u32,
    samples: std::collections::VecDeque<f32>,
    base: i64,
    total: u64,
    emitted: u64,
    kernels: Vec<ResampleKernel>,
}

struct ResampleKernel {
    weights: [f64; 48],
    sum: f64,
}

impl SpeechResampler {
    const HALF: i64 = 24;

    pub fn new(source_rate: u32, target_rate: u32) -> Self {
        let mut a = source_rate;
        let mut b = target_rate;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let phases = target_rate / a;
        // Common audio clocks need at most 640 phases. Bound uncommon,
        // relatively-prime rates instead of allocating megabytes per stream.
        let kernels = if phases <= 1024 {
            (0..phases)
                .map(|phase| Self::kernel(source_rate, target_rate, phase as f64 / phases as f64))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            source_rate,
            target_rate,
            samples: Default::default(),
            base: 0,
            total: 0,
            emitted: 0,
            kernels,
        }
    }

    fn kernel(source_rate: u32, target_rate: u32, fraction: f64) -> ResampleKernel {
        let cutoff = (target_rate as f64 / source_rate as f64).min(1.0) * 0.94;
        let weights = std::array::from_fn(|tap| {
            let distance = tap as f64 - Self::HALF as f64 + 1.0 - fraction;
            let x = std::f64::consts::PI * distance * cutoff;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
            let window = 0.5 + 0.5 * (std::f64::consts::PI * distance / Self::HALF as f64).cos();
            sinc * window * cutoff
        });
        ResampleKernel {
            sum: weights.iter().sum(),
            weights,
        }
    }

    /// Append into caller-owned scratch storage; steady-state processing
    /// does not allocate a new vector for each 10 ms model frame.
    pub fn push_into(&mut self, samples: &[f32], finish: bool, output: &mut Vec<f32>) {
        self.samples
            .extend(samples.iter().map(|s| if s.is_finite() { *s } else { 0.0 }));
        self.total += samples.len() as u64;
        let limit = (self.total * self.target_rate as u64).div_ceil(self.source_rate as u64);
        while self.emitted < limit {
            let numerator = self.emitted * self.source_rate as u64;
            let center = (numerator / self.target_rate as u64) as i64;
            if !finish && center + Self::HALF >= self.total as i64 {
                break;
            }
            let fraction = (numerator % self.target_rate as u64) as f64 / self.target_rate as f64;
            let uncached;
            let kernel = if self.kernels.is_empty() {
                uncached = Self::kernel(self.source_rate, self.target_rate, fraction);
                &uncached
            } else {
                let phase = ((numerator % self.target_rate as u64) * self.kernels.len() as u64
                    / self.target_rate as u64) as usize;
                &self.kernels[phase]
            };
            let mut value = 0.0;
            for (tap, weight) in kernel.weights.iter().enumerate() {
                let index = center - Self::HALF + 1 + tap as i64;
                let sample = if index >= self.base && index < self.total as i64 {
                    self.samples[(index - self.base) as usize] as f64
                } else {
                    0.0
                };
                value += sample * weight;
            }
            output.push((value / kernel.sum) as f32);
            self.emitted += 1;
        }
        let next_center = (self.emitted * self.source_rate as u64 / self.target_rate as u64) as i64;
        let retain_from = (next_center - Self::HALF).max(0);
        let discard = (retain_from - self.base).max(0) as usize;
        let discard = discard.min(self.samples.len());
        self.samples.drain(..discard);
        self.base += discard as i64;
    }
}

#[cfg(test)]
mod resampler_tests {
    use super::SpeechResampler;

    #[test]
    fn cached_phases_match_the_original_sinc_and_bound_unusual_clocks() {
        let input: Vec<f32> = (0..997).map(|i| (i as f32 * 0.037).sin() * 0.3).collect();
        for (source, target) in [
            (16_000, 48_000),
            (48_000, 16_000),
            (44_100, 48_000),
            (48_000, 48_000),
            (48_001, 48_000),
        ] {
            let mut resampler = SpeechResampler::new(source, target);
            assert!(resampler.kernels.len() <= 1024);
            if source == 48_001 {
                assert!(resampler.kernels.is_empty());
            }
            let mut actual = Vec::new();
            for block in input.chunks(71) {
                resampler.push_into(block, false, &mut actual);
            }
            resampler.push_into(&[], true, &mut actual);
            assert_eq!(
                actual.len(),
                (input.len() as u64 * target as u64).div_ceil(source as u64) as usize
            );
            for (i, sample) in actual.iter().enumerate() {
                let numerator = i as u64 * source as u64;
                let center = (numerator / target as u64) as i64;
                let fraction = (numerator % target as u64) as f64 / target as f64;
                let cutoff = (target as f64 / source as f64).min(1.0) * 0.94;
                let (mut value, mut sum) = (0.0, 0.0);
                for tap in center - 23..=center + 24 {
                    let distance = (tap - center) as f64 - fraction;
                    let x = std::f64::consts::PI * distance * cutoff;
                    let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
                    let weight = sinc
                        * (0.5 + 0.5 * (std::f64::consts::PI * distance / 24.0).cos())
                        * cutoff;
                    value += input.get(tap as usize).copied().unwrap_or(0.0) as f64 * weight;
                    sum += weight;
                }
                assert!(
                    (*sample as f64 - value / sum).abs() < 1e-7,
                    "clock {source}/{target}, sample {i}"
                );
            }
        }
    }
}
