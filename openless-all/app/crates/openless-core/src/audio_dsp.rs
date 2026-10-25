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

/// 回声消除算法。
///
/// - `Off`：不做回声消除。
/// - `Light`：Core 自带的归一化 LMS（单核约 3%），对稳态回声有效，双讲时暂停自适应。
/// - `Model`：MicYou 的 aec7 神经网络（单核约 50%），非线性/强混响下更强，弱机慎用。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AecMode {
    #[default]
    Off,
    Light,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioDspStage {
    /// 声学回声消除：需要远端参考信号，只有拿到 far-end 时才真正生效（MicYou 的
    /// 链路里它是第一级）。
    AcousticEchoCancellation,
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
            Self::AcousticEchoCancellation => "AEC",
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
    /// 声学回声消除算法。老版本 preferences 里存的是布尔 `aecEnabled`，
    /// 反序列化时 `true` 映射为 `Light`（轻量、弱机可用），`false` 映射为 `Off`。
    #[serde(default, deserialize_with = "deserialize_aec_mode")]
    pub aec_mode: AecMode,
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
    /// 输出环形缓冲的余量（毫秒）。设备回调抖动大时调大，端到端延迟会跟着变大。
    /// MicYou 的范围是 100..=1200，默认 300。
    pub output_buffer_ms: u16,
    /// 处理链路顺序，7 个阶段各一次。老版本 preferences 里存的是 6 个阶段（没有
    /// AEC），反序列化时按默认顺序补齐，不会因为数组长度变化而丢配置。
    #[serde(
        default = "default_processing_order",
        deserialize_with = "deserialize_processing_order"
    )]
    pub processing_order: [AudioDspStage; 7],
}

fn default_processing_order() -> [AudioDspStage; 7] {
    AudioDspConfig::default().processing_order
}

fn deserialize_processing_order<'de, D>(
    deserializer: D,
) -> Result<[AudioDspStage; 7], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let stored = Vec::<AudioDspStage>::deserialize(deserializer)?;
    let defaults = default_processing_order();
    // 老配置（6 级，没有 AEC）：把 AEC 补到最前面，其余顺序原样保留 —— 这既是
    // MicYou 的默认位置（AEC 必须拿到尚未被降噪改过的信号），也不会制造重复项
    // 而触发 `normalized()` 的「重复即重置」。
    if stored.len() == defaults.len() - 1 && !stored.contains(&AudioDspStage::AcousticEchoCancellation)
    {
        let mut order = [AudioDspStage::AcousticEchoCancellation; 7];
        for (slot, stage) in stored.into_iter().enumerate() {
            order[slot + 1] = stage;
        }
        return Ok(order);
    }
    let mut order = defaults;
    for (slot, stage) in order.iter_mut().enumerate() {
        // 其它长度（损坏或未来版本）：按位置取，缺的用默认，多的丢弃。
        *stage = stored.get(slot).copied().unwrap_or(defaults[slot]);
    }
    Ok(order)
}

/// 接受三种写法：新枚举名（`off`/`light`/`model`）、老布尔值（`true`→`light`）。
fn deserialize_aec_mode<'de, D>(deserializer: D) -> Result<AecMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Mode(AecMode),
        Enabled(bool),
    }
    Ok(match Wire::deserialize(deserializer)? {
        Wire::Mode(mode) => mode,
        Wire::Enabled(true) => AecMode::Light,
        Wire::Enabled(false) => AecMode::Off,
    })
}

impl Default for AudioDspConfig {
    fn default() -> Self {
        use AudioDspStage::*;
        Self {
            aec_mode: AecMode::Off,
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
            output_buffer_ms: 300,
            // 与 MicYou 的默认链路一致：AEC 在最前，其后是降噪、去混响、均衡、
            // 增益、AGC、VAD。
            processing_order: [
                AcousticEchoCancellation,
                NoiseReduction,
                Dereverb,
                Equalizer,
                Amplifier,
                Agc,
                Vad,
            ],
        }
    }
}

impl AudioDspConfig {
    pub fn normalized(mut self) -> Self {
        self.noise_strength = self.noise_strength.min(100);
        self.gain_db = self.gain_db.clamp(-30, 30);
        self.agc_target = self.agc_target.clamp(5, 90);
        self.agc_attack = self.agc_attack.clamp(1, 100);
        self.agc_decay = self.agc_decay.clamp(1, 100);
        self.vad_threshold_db = self.vad_threshold_db.clamp(-80, -10);
        self.dereverb_level = self.dereverb_level.min(100);
        self.equalizer_preamp_db = self.equalizer_preamp_db.clamp(-20, 20);
        self.equalizer_bands_db = self.equalizer_bands_db.map(|gain| gain.clamp(-12, 12));
        self.output_buffer_ms = self.output_buffer_ms.clamp(100, 1200);
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
            aec_enabled: matches!(c.aec_mode, AecMode::Model),
            output_buffer_ms: u32::from(c.output_buffer_ms),
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
}

impl SpeechResampler {
    const HALF: i64 = 24;

    pub fn new(source_rate: u32, target_rate: u32) -> Self {
        Self {
            source_rate,
            target_rate,
            samples: Default::default(),
            base: 0,
            total: 0,
            emitted: 0,
        }
    }

    pub fn push(&mut self, samples: &[f32], finish: bool) -> Vec<f32> {
        self.samples
            .extend(samples.iter().map(|s| if s.is_finite() { *s } else { 0.0 }));
        self.total += samples.len() as u64;
        let limit = (self.total * self.target_rate as u64).div_ceil(self.source_rate as u64);
        let mut output = Vec::new();
        while self.emitted < limit {
            let numerator = self.emitted * self.source_rate as u64;
            let center = (numerator / self.target_rate as u64) as i64;
            if !finish && center + Self::HALF >= self.total as i64 {
                break;
            }
            let fraction = (numerator % self.target_rate as u64) as f64 / self.target_rate as f64;
            let cutoff = (self.target_rate as f64 / self.source_rate as f64).min(1.0) * 0.94;
            let mut value = 0.0;
            let mut weight_sum = 0.0;
            for index in center - Self::HALF + 1..=center + Self::HALF {
                let distance = (index - center) as f64 - fraction;
                let x = std::f64::consts::PI * distance * cutoff;
                let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
                let window =
                    0.5 + 0.5 * (std::f64::consts::PI * distance / Self::HALF as f64).cos();
                let weight = sinc * window * cutoff;
                let sample = if index >= self.base && index < self.total as i64 {
                    self.samples[(index - self.base) as usize] as f64
                } else {
                    0.0
                };
                value += sample * weight;
                weight_sum += weight;
            }
            output.push((value / weight_sum) as f32);
            self.emitted += 1;
        }
        let next_center = (self.emitted * self.source_rate as u64 / self.target_rate as u64) as i64;
        let retain_from = (next_center - Self::HALF).max(0);
        let discard = (retain_from - self.base).max(0) as usize;
        let discard = discard.min(self.samples.len());
        self.samples.drain(..discard);
        self.base += discard as i64;
        output
    }
}
