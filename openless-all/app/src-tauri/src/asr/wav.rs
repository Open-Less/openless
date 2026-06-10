//! WAV helpers for ASR providers that accept complete audio files.

/// Decode a RIFF WAV file to 16-bit PCM samples. Returns `Err` if the WAV header is
/// invalid, the format is not 16-bit mono PCM, or the sample rate is not supported.
/// Used by retranscribe to extract raw PCM from archived recording files.
pub fn decode_wav_to_pcm_i16(wav_bytes: &[u8]) -> Result<Vec<i16>, String> {
    if wav_bytes.len() < 44 {
        return Err("wav too short for valid header".into());
    }
    if &wav_bytes[0..4] != b"RIFF" || &wav_bytes[8..12] != b"WAVE" {
        return Err("not a valid RIFF WAV file".into());
    }
    if &wav_bytes[12..16] != b"fmt " {
        return Err("missing fmt chunk".into());
    }
    let audio_format = u16::from_le_bytes([wav_bytes[20], wav_bytes[21]]);
    if audio_format != 1 {
        return Err(format!("unsupported audio format {audio_format} (expected PCM=1)"));
    }
    let num_channels = u16::from_le_bytes([wav_bytes[22], wav_bytes[23]]);
    let sample_rate = u32::from_le_bytes([wav_bytes[24], wav_bytes[25], wav_bytes[26], wav_bytes[27]]);
    let bits_per_sample = u16::from_le_bytes([wav_bytes[34], wav_bytes[35]]);
    if num_channels != 1 || bits_per_sample != 16 {
        return Err(format!(
            "expected mono 16-bit PCM, got {num_channels}ch {bits_per_sample}-bit"
        ));
    }
    // Accept 8k/16k/48k; resampling not needed for most ASR APIs (they handle it server-side).
    if sample_rate != 8000 && sample_rate != 16_000 && sample_rate != 44_100 && sample_rate != 48_000 {
        log::warn!("[wav] unusual sample rate {sample_rate} Hz — ASR may reject");
    }
    // Find the data chunk (skip past fmt chunk).
    let mut offset = 36;
    while offset + 8 <= wav_bytes.len() {
        let chunk_id = &wav_bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            wav_bytes[offset + 4],
            wav_bytes[offset + 5],
            wav_bytes[offset + 6],
            wav_bytes[offset + 7],
        ]) as usize;
        if chunk_id == b"data" {
            let data_start = offset + 8;
            let data_end = (data_start + chunk_size).min(wav_bytes.len());
            let pcm_bytes = &wav_bytes[data_start..data_end];
            let samples: Vec<i16> = pcm_bytes
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            return Ok(samples);
        }
        offset += 8 + chunk_size;
        // Align to 2-byte boundary as per WAV spec.
        if chunk_size % 2 != 0 {
            offset += 1;
        }
    }
    Err("no data chunk found in WAV".into())
}

/// Encode 16 kHz / mono / 16-bit little-endian PCM samples as a RIFF WAV file.
pub fn encode_wav_16k_mono(samples: &[i16]) -> Vec<u8> {
    let sample_rate: u32 = 16_000;
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let bytes_per_sample = bits_per_sample as u32 / 8;
    let byte_rate = sample_rate * num_channels as u32 * bytes_per_sample;
    let block_align = num_channels * (bits_per_sample / 8);
    let data_size = samples.len() as u32 * bytes_per_sample;
    let chunk_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + data_size as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&num_channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests {
    use super::encode_wav_16k_mono;

    #[test]
    fn wav_header_matches_16k_mono_pcm() {
        let samples = [1i16, i16::MAX, i16::MIN, -2i16];
        let wav = encode_wav_16k_mono(&samples);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 44);
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(wav[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 32_000);
        assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(
            &wav[44..],
            &[0x01, 0x00, 0xff, 0x7f, 0x00, 0x80, 0xfe, 0xff]
        );
    }
}
