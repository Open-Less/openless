//! Shared PCM duration calculation.
//!
//! Recordings are uniformly 16 kHz / mono / 16-bit little-endian PCM; the conversion
//! `(bytes / 2) * 1000 / 16000` was previously duplicated across ASR providers
//! (foundry / sherpa / whisper / mimo). Consolidated here into the single
//! implementation, with each site now a thin wrapper (following the
//! `wav::encode_wav_16k_mono` sharing precedent).

/// Bytes per sample (16-bit -> 2 bytes).
const PCM_BYTES_PER_SAMPLE: u64 = 2;
/// Sample rate (16 kHz).
const PCM_SAMPLE_RATE_HZ: u64 = 16_000;

/// Duration in ms of 16 kHz / mono / 16-bit PCM from the raw byte count.
pub fn pcm_duration_ms_from_bytes(bytes: u64) -> u64 {
    (bytes / PCM_BYTES_PER_SAMPLE) * 1000 / PCM_SAMPLE_RATE_HZ
}

/// Duration in ms of 16 kHz / mono / 16-bit PCM from a byte slice.
pub fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    pcm_duration_ms_from_bytes(pcm.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_of_16k_i16_pcm_is_1000ms() {
        // 16000 samples × 2 bytes = 32000 bytes = 1 second
        assert_eq!(pcm_duration_ms(&vec![0u8; 32_000]), 1000);
        assert_eq!(pcm_duration_ms_from_bytes(32_000), 1000);
    }

    #[test]
    fn odd_trailing_byte_is_floored() {
        // A trailing half-sample floors down, matching historical behavior
        assert_eq!(pcm_duration_ms(&[0u8; 33]), pcm_duration_ms(&[0u8; 32]));
    }
}
