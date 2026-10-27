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

/// Shared processor for native and external microphone hosts.
/// Enhancement starts with the session's settings and may be disabled under
/// load. Hosts must deliver every returned tail before feeding more input or
/// closing their ASR consumer or recording archive.
pub struct AudioProcessor {
    config: AudioProcessingConfig,
    normalizer: PcmNormalizer,
    enhanced: Option<EnhancedStream>,
    echo: Option<echo::EchoProcessor>,
    finished: bool,
}

impl std::fmt::Debug for AudioProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioProcessor")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl AudioProcessor {
    pub fn new(config: AudioProcessingConfig) -> Self {
        Self {
            config,
            normalizer: Default::default(),
            enhanced: None,
            echo: config
                .echo_cancellation_enabled
                .then(echo::EchoProcessor::default),
            finished: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.config);
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
        if !self.config.enhancement_enabled {
            return self.normalizer.process(input, channels, rate);
        }
        let stream = self
            .enhanced
            .get_or_insert_with(|| EnhancedStream::new(self.config.dsp, channels, rate));
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
        let mono = downmix_to_mono(&stream.interleaved[..complete], channels);
        stream.interleaved.drain(..complete);
        stream.input_frames += mono.len() as u64;
        let working = stream.input_resampler.push(&mono, false);
        let processed = stream.frames(&working, false);
        pcm_chunk(&processed)
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
        let working = stream.input_resampler.push(&[], true);
        let processed = stream.frames(&working, true);
        pcm_chunk(&processed)
    }
}

struct EnhancedStream {
    channels: usize,
    rate: u32,
    input_resampler: crate::audio_dsp::SpeechResampler,
    output_resampler: crate::audio_dsp::SpeechResampler,
    dsp: micyou_dsp::DspProcessor,
    interleaved: Vec<f32>,
    pending: std::collections::VecDeque<f32>,
    skip: usize,
    model_delay: usize,
    input_frames: u64,
    emitted_frames: u64,
}

impl EnhancedStream {
    fn new(config: crate::audio_dsp::AudioDspConfig, channels: usize, rate: u32) -> Self {
        let config = config.normalized();
        Self {
            channels,
            rate,
            input_resampler: crate::audio_dsp::SpeechResampler::new(rate, 48_000),
            output_resampler: crate::audio_dsp::SpeechResampler::new(48_000, DICTATION_SAMPLE_RATE),
            dsp: micyou_dsp::DspProcessor::new(
                std::sync::Arc::new(std::sync::RwLock::new(config.settings())),
                None,
            ),
            interleaved: Vec::new(),
            pending: Default::default(),
            skip: config.model_delay(),
            model_delay: config.model_delay(),
            input_frames: 0,
            emitted_frames: 0,
        }
    }

    fn frames(&mut self, input: &[f32], finish: bool) -> Vec<f32> {
        self.pending.extend(input);
        if finish {
            let tail = self.pending.len().div_ceil(480) * 480 + self.model_delay;
            self.pending.resize(tail, 0.0);
        }
        let mut output = Vec::new();
        while self.pending.len() >= 480 {
            let mut frame: Vec<f32> = self.pending.drain(..480).collect();
            self.dsp.process(&mut frame, 1, 0.0);
            let skip = self.skip.min(frame.len());
            self.skip -= skip;
            output.extend(self.output_resampler.push(&frame[skip..], false));
        }
        if finish {
            output.extend(self.output_resampler.push(&[], true));
        }
        // The last padded model frame may produce additional samples. Bound
        // output by real capture duration, independently of callback sizes.
        let valid = (self.input_frames * DICTATION_SAMPLE_RATE as u64).div_ceil(self.rate as u64);
        output.truncate(valid.saturating_sub(self.emitted_frames) as usize);
        self.emitted_frames += output.len() as u64;
        output
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
