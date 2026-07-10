//! 共享 PCM 时长计算。
//!
//! 录音统一是 16 kHz / 单声道 / 16-bit 小端 PCM，时长换算 `(字节数 / 2) * 1000 / 16000`
//! 原本散落在各 ASR provider 里重复（foundry / sherpa / whisper / mimo），这里收口成
//! 唯一实现，各处改为薄封装调用（参考 `wav::encode_wav_16k_mono` 的共享先例）。

/// 每个采样的字节数（16-bit → 2 字节）。
const PCM_BYTES_PER_SAMPLE: u64 = 2;
/// 采样率（16 kHz）。
const PCM_SAMPLE_RATE_HZ: u64 = 16_000;

/// 由原始字节数计算 16 kHz / 单声道 / 16-bit PCM 的时长（毫秒）。
pub fn pcm_duration_ms_from_bytes(bytes: u64) -> u64 {
    (bytes / PCM_BYTES_PER_SAMPLE) * 1000 / PCM_SAMPLE_RATE_HZ
}

/// 由 PCM 字节切片计算 16 kHz / 单声道 / 16-bit PCM 的时长（毫秒）。
pub fn pcm_duration_ms(pcm: &[u8]) -> u64 {
    pcm_duration_ms_from_bytes(pcm.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_of_16k_i16_pcm_is_1000ms() {
        // 16000 采样 × 2 字节 = 32000 字节 = 1 秒
        assert_eq!(pcm_duration_ms(&vec![0u8; 32_000]), 1000);
        assert_eq!(pcm_duration_ms_from_bytes(32_000), 1000);
    }

    #[test]
    fn odd_trailing_byte_is_floored() {
        // 末尾半个采样向下取整，与历史行为一致
        assert_eq!(
            pcm_duration_ms(&vec![0u8; 33]),
            pcm_duration_ms(&vec![0u8; 32])
        );
    }
}
