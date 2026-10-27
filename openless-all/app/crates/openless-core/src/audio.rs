//! Framework-independent audio normalization used by native recorder adapters.

#[path = "audio/echo.rs"]
mod echo;
pub use echo::ReferenceAudioBuffer;

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
    pub echo_cancellation_enabled: bool,
    pub dsp: crate::audio_dsp::AudioDspConfig,
}

/// 扬声器回环（far-end）参考音频的交接队列。
///
/// AEC 需要「扬声器此刻在放什么」。宿主在一条独立的采集流里拿到输出音频后，
/// 按自己的采样率塞进这里；DSP 线程在处理每一帧之前把它取走、重采样到 48 kHz
/// 单声道后交给 MicYou。队列有上限：AEC 只关心最近的参考，堆积只会拖慢。
#[derive(Clone, Default)]
pub struct FarEndQueue {
    inner: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<FarEndFrame>>>,
}

#[derive(Debug)]
struct FarEndFrame {
    rate: u32,
    channels: usize,
    samples: Vec<f32>,
}

/// 参考队列最多保留的样本数（约 2 秒 @48 kHz）。超出时丢弃最旧的。
const FAR_END_QUEUE_LIMIT: usize = 96_000;

impl FarEndQueue {
    /// 宿主采集线程调用。非有限值与空帧直接丢弃。
    pub fn push(&self, samples: &[f32], channels: usize, rate: u32) {
        if samples.is_empty() || channels == 0 || rate == 0 {
            return;
        }
        let clean: Vec<f32> = samples
            .iter()
            .map(|sample| if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 })
            .collect();
        let mut queue = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.push_back(FarEndFrame {
            rate,
            channels,
            samples: clean,
        });
        let mut total: usize = queue.iter().map(|frame| frame.samples.len()).sum();
        while total > FAR_END_QUEUE_LIMIT {
            let Some(dropped) = queue.pop_front() else { break };
            total -= dropped.samples.len();
        }
    }

    /// DSP 线程调用：取走全部待处理参考帧（保持到达顺序）。
    fn drain(&self) -> Vec<FarEndFrame> {
        let mut queue = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.drain(..).collect()
    }

    fn is_empty(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    }
}

const ECHO_FILTER_TAPS: usize = 1_024;
const ECHO_ADAPTATION_RATE: f32 = 0.12;
const ECHO_DOUBLE_TALK_RATIO: f32 = 2.5;
const ECHO_MIN_REFERENCE_POWER: f32 = 0.000_001;

/// 轻量回声消除：有界归一化 LMS（沿用 #1228 的实现）。
///
/// 学的是「扬声器 → 麦克风」这条声学路径；近端能量明显强于估计回声（双讲）时
/// 暂停自适应，避免把说话人自己的声音学掉。单核开销约 3%，弱机可用。
#[derive(Debug, Clone)]
struct LightEchoCanceller {
    coefficients: Vec<f32>,
    history: Vec<f32>,
    history_cursor: usize,
}

impl Default for LightEchoCanceller {
    fn default() -> Self {
        Self {
            coefficients: vec![0.0; ECHO_FILTER_TAPS],
            history: vec![0.0; ECHO_FILTER_TAPS],
            history_cursor: 0,
        }
    }
}

impl LightEchoCanceller {
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
            // 块边界处重采样可能少一个样本：保留麦克风样本、等下一块重建对齐，
            // 不要把上一个参考值铺满，那样会教坏滤波器。
            let Some(&reference_sample) = reference.get(index) else {
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

/// Shared processor for native and external microphone hosts.
/// Enhancement starts with the session's settings and may be disabled under
/// load. Hosts must deliver every returned tail before feeding more input or
/// closing their ASR consumer or recording archive.
pub struct AudioProcessor {
    config: AudioProcessingConfig,
    normalizer: PcmNormalizer,
    enhanced: Option<EnhancedStream>,
    /// AEC 的扬声器参考输入（宿主在独立的回环采集流里喂）。
    far_end: Option<FarEndQueue>,
    /// 轻量 NLMS 抵消器（`AecMode::Light`）。
    light_aec: Option<LightEchoCanceller>,
    /// 轻量路径把参考重采样到输出采样率（16 kHz 单声道）。
    light_reference_resampler: Option<crate::audio_dsp::SpeechResampler>,
    light_reference_rate: Option<(u32, u32)>,
    light_reference_pending: Vec<f32>,
    echo: Option<echo::EchoProcessor>,
    finished: bool,
    monitoring: bool,
}

impl std::fmt::Debug for AudioProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioProcessor")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl AudioProcessor {
    pub fn new(mut config: AudioProcessingConfig) -> Self {
        // The independent playback-cancellation switch owns AEC when enabled; never stack cancellers.
        if config.echo_cancellation_enabled {
            config.dsp.aec_mode = crate::audio_dsp::AecMode::Off;
        }
        Self {
            config,
            normalizer: Default::default(),
            enhanced: None,
            far_end: None,
            light_aec: matches!(config.dsp.aec_mode, crate::audio_dsp::AecMode::Light)
                .then(LightEchoCanceller::default),
            light_reference_resampler: None,
            light_reference_rate: None,
            light_reference_pending: Vec::new(),
            echo: config
                .echo_cancellation_enabled
                .then(echo::EchoProcessor::default),
            finished: false,
            monitoring: false,
        }
    }

    /// 把参考队列里的帧转成「与麦克风同一采样率」的单声道，供轻量抵消器使用。
    ///
    /// NLMS 的抽头是按样本对齐的，采样率不一致就完全对不上 —— 早先版本把参考重
    /// 采样到 16 kHz 却拿 48 kHz 的麦克风样本做抵消，实测 ERLE ≈ 0（等于没做）。
    fn light_reference_at(
        &mut self,
        queue: &FarEndQueue,
        rate: u32,
        needed_frames: usize,
    ) -> Option<Vec<f32>> {
        for frame in queue.drain() {
            let key = (frame.rate, rate);
            if self.light_reference_rate != Some(key) {
                self.light_reference_rate = Some(key);
                self.light_reference_resampler = Some(
                    crate::audio_dsp::SpeechResampler::new(frame.rate, rate),
                );
            }
            let mono = downmix_to_mono(&frame.samples, frame.channels);
            if let Some(resampler) = self.light_reference_resampler.as_mut() {
                resampler.push_into(&mono, false, &mut self.light_reference_pending);
            }
        }
        if self.light_reference_pending.is_empty() {
            return None;
        }
        let take = needed_frames.min(self.light_reference_pending.len());
        Some(self.light_reference_pending.drain(..take).collect())
    }

    fn apply_light_aec(
        &mut self,
        input: &[f32],
        channels: usize,
        reference: &[f32],
    ) -> Vec<f32> {
        if channels <= 1 {
            return self
                .light_aec
                .as_mut()
                .expect("light AEC is enabled")
                .process(input, reference);
        }
        let frame_count = input.len() / channels;
        let mono = input
            .chunks_exact(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect::<Vec<_>>();
        let clean = self
            .light_aec
            .as_mut()
            .expect("light AEC is enabled")
            .process(&mono, reference);
        let mut output = input.to_vec();
        for frame in 0..frame_count {
            let correction = clean[frame] - mono[frame];
            for channel in 0..channels {
                output[frame * channels + channel] += correction;
            }
        }
        output
    }

    /// 会话结束时清掉抵消器状态，避免上一场的声学路径带到下一场。
    pub fn reset_echo_cancellers(&mut self) {
        if let Some(canceller) = self.light_aec.as_mut() {
            canceller.reset();
        }
        self.light_reference_resampler = None;
        self.light_reference_rate = None;
        self.light_reference_pending.clear();
    }

    /// 接上扬声器回环参考队列。只有 AEC 打开且宿主真的采到了输出音频时才有意义；
    /// 没接或队列为空时 AEC 按「无参考」处理（MicYou 内部会退化）。
    pub fn attach_far_end(&mut self, queue: FarEndQueue) {
        self.far_end = Some(queue);
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.config);
    }

    /// Visual analysis is opt-in and does not open a microphone or retain PCM.
    pub fn set_monitoring_enabled(&mut self, enabled: bool) {
        self.monitoring = enabled;
        if let Some(stream) = &mut self.enhanced {
            stream.dsp.set_spectrum_enabled(enabled);
        }
    }

    pub fn monitoring_sample_rate(&self) -> Option<u32> {
        self.enhanced.as_ref().map(|stream| stream.work_rate)
    }

    pub fn spectrums(&self) -> Option<(Vec<f32>, Vec<f32>)> {
        self.monitoring
            .then(|| {
                self.enhanced
                    .as_ref()
                    .map(|stream| stream.dsp.get_spectrums())
            })
            .flatten()
    }

    /// Flush delayed enhanced samples before switching to basic normalization.
    /// The change lasts until the processor is replaced for a new capture.
    pub fn disable_enhancement(&mut self) -> Option<NormalizedPcmChunk> {
        if !self.config.enhancement_enabled || self.finished {
            return None;
        }
        let tail = self.finish();
        self.config.enhancement_enabled = false;
        self.enhanced = None;
        self.finished = false;
        tail
    }

    pub fn process(
        &mut self,
        input: &[f32],
        channels: usize,
        rate: u32,
    ) -> Option<NormalizedPcmChunk> {
        if self.finished || input.is_empty() || channels == 0 || rate == 0 {
            return None;
        }
        // 轻量 AEC 在主链之前做：抵消后的信号再进降噪/增益，顺序与 MicYou 的
        // 链路一致（AEC 必须拿到尚未被其它处理改过的信号）。
        let mut cancelled: Option<Vec<f32>> = None;
        if self.light_aec.is_some() {
            let needed_frames = input.len() / channels;
            if let Some(queue) = self.far_end.clone() {
                if let Some(reference) = self.light_reference_at(&queue, rate, needed_frames) {
                    cancelled = Some(self.apply_light_aec(input, channels, &reference));
                }
            }
        }
        let input = cancelled.as_deref().unwrap_or(input);

        // 模型 AEC（`AecMode::Model`）走 MicYou 链路；增强关掉时也要能单独跑。
        let aec_standalone = matches!(self.config.dsp.aec_mode, crate::audio_dsp::AecMode::Model)
            && self.far_end.is_some();
        if !self.config.enhancement_enabled && !aec_standalone {
            return self.normalizer.process(input, channels, rate);
        }
        let stream = self
            .enhanced
            .get_or_insert_with(|| EnhancedStream::new(self.config.dsp, channels, rate));
        stream.dsp.set_spectrum_enabled(self.monitoring);
        if stream.channels != channels || stream.rate != rate {
            log::error!("audio format changed during an enhancement session; reset required");
            return None;
        }
        stream.interleaved.extend(input.iter().map(|s| {
            if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }));
        let complete = stream.interleaved.len() / channels * channels;
        stream.mono.clear();
        stream.mono.extend(
            stream.interleaved[..complete]
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        stream.interleaved.drain(..complete);
        stream.input_frames += stream.mono.len() as u64;
        // 参考音频必须早于本帧进入 AEC：MicYou 用最近的历史做自适应滤波。
        if let Some(far_end) = self.far_end.as_ref().filter(|queue| !queue.is_empty()) {
            let frames = far_end.drain();
            stream.push_far_end(frames);
        }
        stream.working.clear();
        if let Some(resampler) = &mut stream.input_resampler {
            resampler.push_into(&stream.mono, false, &mut stream.working);
        } else {
            stream.working.extend_from_slice(&stream.mono);
        }
        pcm_chunk(stream.frames(false))
    }

    /// A playback gap bypasses only cancellation; enhancement keeps its format and tail.
    pub fn process_with_reference(
        &mut self,
        input: &[f32],
        channels: usize,
        rate: u32,
        reference: Option<&[f32]>,
    ) -> Option<NormalizedPcmChunk> {
        let Some(echo) = self.echo.as_mut() else {
            return self.process(input, channels, rate);
        };
        let samples = echo.process(input, channels, rate, reference);
        self.process(&samples, 1, DICTATION_SAMPLE_RATE)
    }

    /// Idempotent end-of-stream barrier. Flushes fractional resampling state,
    /// a partial 10 ms frame and model lookahead without adding padding to PCM.
    pub fn finish(&mut self) -> Option<NormalizedPcmChunk> {
        if self.finished {
            return None;
        }
        self.finished = true;
        let stream = self.enhanced.as_mut()?;
        stream.working.clear();
        if let Some(resampler) = &mut stream.input_resampler {
            resampler.push_into(&[], true, &mut stream.working);
        }
        pcm_chunk(stream.frames(true))
    }
}

struct EnhancedStream {
    channels: usize,
    rate: u32,
    input_resampler: Option<crate::audio_dsp::SpeechResampler>,
    output_resampler: Option<crate::audio_dsp::SpeechResampler>,
    work_rate: u32,
    hop: usize,
    dsp: micyou_dsp::DspProcessor,
    interleaved: Vec<f32>,
    mono: Vec<f32>,
    working: Vec<f32>,
    frame: Vec<f32>,
    output: Vec<f32>,
    pending: std::collections::VecDeque<f32>,
    skip: usize,
    model_delay: usize,
    input_frames: u64,
    emitted_frames: u64,
    /// 参考音频的重采样器，按输入采样率惰性重建（回环流通常固定，重建仅一次）。
    far_end_rate: Option<u32>,
    far_end_resampler: Option<crate::audio_dsp::SpeechResampler>,
    far_end_pending: Vec<f32>,
}

impl EnhancedStream {
    fn new(config: crate::audio_dsp::AudioDspConfig, channels: usize, rate: u32) -> Self {
        let config = config.normalized();
        let work_rate = 48_000;
        Self {
            channels,
            rate,
            // Preserve the original input filter even at 48 kHz: cached
            // coefficients remove redundant computation without changing
            // the signal presented to the denoiser.
            input_resampler: Some(crate::audio_dsp::SpeechResampler::new(rate, work_rate)),
            output_resampler: (work_rate != DICTATION_SAMPLE_RATE)
                .then(|| crate::audio_dsp::SpeechResampler::new(work_rate, DICTATION_SAMPLE_RATE)),
            work_rate,
            hop: (work_rate / 100) as usize,
            dsp: micyou_dsp::DspProcessor::new(
                std::sync::Arc::new(std::sync::RwLock::new(config.settings())),
                None,
            ),
            interleaved: Vec::new(),
            mono: Vec::new(),
            working: Vec::new(),
            frame: Vec::with_capacity(480),
            output: Vec::new(),
            pending: Default::default(),
            skip: config.model_delay(),
            model_delay: config.model_delay(),
            input_frames: 0,
            emitted_frames: 0,
            far_end_rate: None,
            far_end_resampler: None,
            far_end_pending: Vec::new(),
        }
    }

    /// 把宿主采到的回环参考帧转成 48 kHz 单声道后交给 AEC。
    /// 把宿主采到的回环参考帧转成 48 kHz 单声道后交给 AEC。
    fn push_far_end(&mut self, frames: Vec<FarEndFrame>) {
        for frame in frames {
            let mono = downmix_to_mono(&frame.samples, frame.channels);
            if self.far_end_rate != Some(frame.rate) {
                self.far_end_rate = Some(frame.rate);
                self.far_end_resampler =
                    Some(crate::audio_dsp::SpeechResampler::new(frame.rate, 48_000));
                self.far_end_pending.clear();
            }
            if let Some(resampler) = self.far_end_resampler.as_mut() {
                resampler.push_into(&mono, false, &mut self.far_end_pending);
            }
        }
        if self.far_end_pending.is_empty() {
            return;
        }
        let reference = std::mem::take(&mut self.far_end_pending);
        self.dsp.set_far_end_audio(&reference);
    }


    fn frames(&mut self, finish: bool) -> &[f32] {
        self.pending.extend(&self.working);
        if finish {
            let tail = self.pending.len().div_ceil(self.hop) * self.hop + self.model_delay;
            self.pending.resize(tail, 0.0);
        }
        self.output.clear();
        while self.pending.len() >= self.hop {
            self.frame.clear();
            self.frame.extend(self.pending.drain(..self.hop));
            self.dsp.process(&mut self.frame, 1, 0.0);
            let skip = self.skip.min(self.frame.len());
            self.skip -= skip;
            if let Some(resampler) = &mut self.output_resampler {
                resampler.push_into(&self.frame[skip..], false, &mut self.output);
            } else {
                self.output.extend_from_slice(&self.frame[skip..]);
            }
        }
        if finish {
            if let Some(resampler) = &mut self.output_resampler {
                resampler.push_into(&[], true, &mut self.output);
            }
        }
        // The last padded model frame may produce additional samples. Bound
        // output by real capture duration, independently of callback sizes.
        let valid = (self.input_frames * DICTATION_SAMPLE_RATE as u64).div_ceil(self.rate as u64);
        self.output
            .truncate(valid.saturating_sub(self.emitted_frames) as usize);
        self.emitted_frames += self.output.len() as u64;
        &self.output
    }
}

fn pcm_chunk(samples: &[f32]) -> Option<NormalizedPcmChunk> {
    if samples.is_empty() {
        return None;
    }
    let (pcm_i16_le, rms) = quantize_to_i16_le(samples);
    Some(NormalizedPcmChunk {
        pcm_i16_le,
        level: (rms * LEVEL_RMS_GAIN).clamp(0.0, 1.0),
    })
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

/// Borrow canonical dictation PCM from a RIFF WAV, including files with metadata chunks.
pub fn decode_dictation_wav(wav: &[u8]) -> Result<&[u8], crate::BackendError> {
    let invalid = || {
        crate::BackendError::new(
            crate::BackendErrorCode::InvalidArgument,
            "expected a complete 16 kHz mono signed 16-bit PCM WAV",
        )
    };
    if wav.len() < 12
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
        || u32::from_le_bytes(wav[4..8].try_into().map_err(|_| invalid())?) as u64 + 8
            != wav.len() as u64
    {
        return Err(invalid());
    }
    let mut offset = 12;
    let mut pcm = None;
    let mut format_ok = false;
    while offset < wav.len() {
        let header = wav.get(offset..offset + 8).ok_or_else(invalid)?;
        let size = u32::from_le_bytes(header[4..8].try_into().map_err(|_| invalid())?) as usize;
        let end = (offset + 8).checked_add(size).ok_or_else(invalid)?;
        let chunk = wav.get(offset + 8..end).ok_or_else(invalid)?;
        match &header[..4] {
            b"fmt " => {
                format_ok = chunk.len() >= 16
                    && chunk[..2] == 1_u16.to_le_bytes()
                    && chunk[2..4] == 1_u16.to_le_bytes()
                    && chunk[4..8] == DICTATION_SAMPLE_RATE.to_le_bytes()
                    && chunk[8..12] == (DICTATION_SAMPLE_RATE * 2).to_le_bytes()
                    && chunk[12..14] == 2_u16.to_le_bytes()
                    && chunk[14..16] == 16_u16.to_le_bytes();
                if !format_ok {
                    return Err(invalid());
                }
            }
            b"data" => {
                if pcm.is_some() || !size.is_multiple_of(2) {
                    return Err(invalid());
                }
                pcm = Some(chunk);
            }
            _ => {}
        }
        offset = end.checked_add(size % 2).ok_or_else(invalid)?;
    }
    if !format_ok || offset != wav.len() {
        return Err(invalid());
    }
    pcm.ok_or_else(invalid)
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

/// f32 交织采样 → 小端 i16 字节流，供宿主把「处理前」的原始 PCM 也写成 WAV
/// （调试时与处理后那份做 A/B 对照）。量化规则与 [`AudioProcessor`] 内部一致。
pub fn pcm_f32_to_i16_le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let quantized = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&quantized.to_le_bytes());
    }
    bytes
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
#[path = "audio/dsp_tests.rs"]
mod dsp_tests;

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn decode(bytes: &[u8]) -> Vec<i16> {
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
    fn light_aec_resamples_reference_and_preserves_backlog() {
        let mut processor = AudioProcessor::new(AudioProcessingConfig {
            dsp: crate::audio_dsp::AudioDspConfig {
                aec_mode: crate::audio_dsp::AecMode::Light,
                ..Default::default()
            },
            ..Default::default()
        });
        let queue = FarEndQueue::default();
        queue.push(&vec![0.1; 480], 1, 16_000);
        let first = processor.light_reference_at(&queue, 48_000, 480).unwrap();
        assert_eq!(first.len(), 480);
        assert!(!processor.light_reference_pending.is_empty());
        let second = processor
            .light_reference_at(&queue, 48_000, 480)
            .unwrap();
        assert!(!second.is_empty());
    }

    #[test]
    fn light_aec_applies_one_mono_correction_to_all_input_channels() {
        let mut processor = AudioProcessor::new(AudioProcessingConfig {
            dsp: crate::audio_dsp::AudioDspConfig {
                aec_mode: crate::audio_dsp::AecMode::Light,
                ..Default::default()
            },
            ..Default::default()
        });
        processor.light_aec.as_mut().unwrap().coefficients[0] = 1.0;
        let input = [0.2, 0.4, 0.2, 0.4];
        let reference = [0.1, 0.1];
        let output = processor.apply_light_aec(&input, 2, &reference);
        assert_eq!(output.len(), input.len());
        assert_eq!(output[0] - output[1], input[0] - input[1]);
        assert_eq!(output[2] - output[3], input[2] - input[3]);
        assert!(output[0] < input[0]);
    }

    #[test]
    fn standalone_echo_switch_does_not_stack_enhancement_aec() {
        for enhancement_enabled in [false, true] {
            let config = AudioProcessingConfig {
                enhancement_enabled,
                echo_cancellation_enabled: true,
                ..Default::default()
            };
            for mode in [crate::AecMode::Light, crate::AecMode::Model] {
                let mut selected = AudioProcessor::new(AudioProcessingConfig {
                    dsp: crate::AudioDspConfig { aec_mode: mode, ..config.dsp },
                    ..config
                });
                let mut standalone = AudioProcessor::new(config);
                assert!(selected.light_aec.is_none());
                assert_eq!(selected.config.dsp.aec_mode, crate::AecMode::Off);
                let microphone = vec![0.2; 480];
                let reference = vec![0.1; 240];
                for input_reference in [Some(reference.as_slice()), None, Some(reference.as_slice())] {
                    assert_eq!(
                        selected.process_with_reference(&microphone, 2, 48_000, input_reference),
                        standalone.process_with_reference(&microphone, 2, 48_000, input_reference),
                    );
                }
                assert_eq!(selected.finish(), standalone.finish());
            }
        }
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
    fn disabling_enhancement_flushes_lookahead_once_and_preserves_duration() {
        for rate in [16_000, 44_100, 48_000] {
            let mut processor = AudioProcessor::new(AudioProcessingConfig {
                enhancement_enabled: true,
                ..Default::default()
            });
            let mut samples = 0;
            for chunk in [
                processor.process(&vec![0.25; 317], 1, rate),
                processor.disable_enhancement(),
            ]
            .into_iter()
            .flatten()
            {
                samples += chunk.pcm_i16_le.len() / 2;
            }
            assert_eq!(
                samples,
                (317_u64 * 16_000).div_ceil(u64::from(rate)) as usize
            );
            assert!(processor.disable_enhancement().is_none());
            let next = processor.process(&[0.5; 160], 1, 16_000).unwrap();
            assert_eq!(decode(&next.pcm_i16_le), vec![16383; 160]);
            assert!(processor.finish().is_none());
            assert!(processor.disable_enhancement().is_none());
            assert!(processor.process(&[0.5; 160], 1, 16_000).is_none());
        }
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
        assert_eq!(decode_dictation_wav(&wav).unwrap(), &pcm);
        assert_eq!(
            encode_dictation_wav(&[1]).unwrap_err().code,
            crate::BackendErrorCode::InvalidArgument
        );
    }

    #[test]
    fn wav_decoder_skips_padded_metadata_and_preserves_pcm() {
        let pcm = [1, 0, 255, 127];
        let original = encode_dictation_wav(&pcm).unwrap();
        let mut wav = original[..36].to_vec();
        wav.extend_from_slice(b"JUNK");
        wav.extend_from_slice(&3_u32.to_le_bytes());
        wav.extend_from_slice(&[8, 9, 10, 0]);
        wav.extend_from_slice(&original[36..]);
        let size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(decode_dictation_wav(&wav).unwrap(), &pcm);
    }

    #[test]
    fn wav_decoder_rejects_truncation_wrong_format_and_oversized_chunks() {
        let original = encode_dictation_wav(&[1, 0, 2, 0]).unwrap();
        for length in 0..original.len() {
            assert!(decode_dictation_wav(&original[..length]).is_err());
        }
        for (offset, replacement) in [
            (20, vec![3, 0]),
            (22, vec![2, 0]),
            (24, 48_000_u32.to_le_bytes().to_vec()),
            (34, vec![8, 0]),
            (40, u32::MAX.to_le_bytes().to_vec()),
        ] {
            let mut wav = original.clone();
            wav[offset..offset + replacement.len()].copy_from_slice(&replacement);
            assert!(decode_dictation_wav(&wav).is_err());
        }
    }
}
