//! WAV helpers for ASR providers that accept complete audio files.

fn write_wav_16k_mono_header(wav: &mut Vec<u8>, data_size: u32) {
    let sample_rate: u32 = 16_000;
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let bytes_per_sample = bits_per_sample as u32 / 8;
    let byte_rate = sample_rate * num_channels as u32 * bytes_per_sample;
    let block_align = num_channels * (bits_per_sample / 8);
    let chunk_size = 36u32.saturating_add(data_size);

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
}

/// Encode 16 kHz / mono / 16-bit little-endian PCM samples as a RIFF WAV file.
pub fn encode_wav_16k_mono(samples: &[i16]) -> Vec<u8> {
    let data_size = samples.len() as u32 * 2;
    let mut wav = Vec::with_capacity(44 + data_size as usize);
    write_wav_16k_mono_header(&mut wav, data_size);
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Encode canonical 16 kHz / mono / 16-bit little-endian PCM bytes as a RIFF WAV
/// without allocating an intermediate `Vec<i16>`. Any trailing odd byte is ignored.
pub fn encode_wav_16k_mono_pcm_bytes(pcm: &[u8]) -> Vec<u8> {
    let aligned_len = (pcm.len() & !1).min((u32::MAX - 36) as usize);
    let data_size = aligned_len as u32;
    let mut wav = Vec::with_capacity(44 + aligned_len);
    write_wav_16k_mono_header(&mut wav, data_size);
    wav.extend_from_slice(&pcm[..aligned_len]);
    wav
}

#[cfg(test)]
mod tests {
    use super::{encode_wav_16k_mono, encode_wav_16k_mono_pcm_bytes};

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

    #[test]
    fn pcm_bytes_encoding_matches_i16_samples_and_ignores_odd_tail() {
        let samples = [1i16, i16::MAX, i16::MIN, -2i16];
        let mut bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let expected = encode_wav_16k_mono(&samples);
        assert_eq!(encode_wav_16k_mono_pcm_bytes(&bytes), expected);

        bytes.push(0xAB);
        assert_eq!(encode_wav_16k_mono_pcm_bytes(&bytes), expected);
    }
}
