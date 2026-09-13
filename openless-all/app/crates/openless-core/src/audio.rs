//! Framework-independent audio normalization used by native recorder adapters.

pub const DICTATION_SAMPLE_RATE: u32 = 16_000;
const LEVEL_RMS_GAIN: f32 = 4.0;

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedPcmChunk {
    pub pcm_i16_le: Vec<u8>,
    pub level: f32,
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
