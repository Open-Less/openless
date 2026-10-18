//! File-only WAV decoding for explicit experimental acceptance.
use anyhow::Result;

/// Walk the RIFF chunk chain strictly to find "data" — both jfk.wav and test_speech.wav carry
/// LIST/INFO metadata after the fmt chunk, so a hardcoded +44 offset reads garbage.
pub(super) fn decode_wav_16k_mono(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        anyhow::bail!("不是有效的 RIFF/WAVE 文件");
    }

    let mut cursor = 12usize;
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;
    let mut bits_per_sample: u16 = 0;
    let mut data_offset: usize = 0;
    let mut data_size: usize = 0;

    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let body_start = cursor + 8;

        match id {
            b"fmt " => {
                if body_start + 16 > bytes.len() {
                    anyhow::bail!("fmt chunk 越界");
                }
                let format =
                    u16::from_le_bytes(bytes[body_start..body_start + 2].try_into().unwrap());
                if format != 1 {
                    anyhow::bail!("只支持 PCM（format=1），当前 format={format}");
                }
                channels =
                    u16::from_le_bytes(bytes[body_start + 2..body_start + 4].try_into().unwrap());
                sample_rate =
                    u32::from_le_bytes(bytes[body_start + 4..body_start + 8].try_into().unwrap());
                bits_per_sample =
                    u16::from_le_bytes(bytes[body_start + 14..body_start + 16].try_into().unwrap());
            }
            b"data" => {
                data_offset = body_start;
                data_size = size;
                break;
            }
            _ => { /* LIST / INFO / other metadata — skip */ }
        }
        // Chunk body length must be even-aligned.
        let advance = size + (size & 1);
        cursor = body_start + advance;
    }

    if data_offset == 0 || data_size == 0 {
        anyhow::bail!("未找到 data chunk");
    }
    if sample_rate != 16_000 || channels != 1 || bits_per_sample != 16 {
        anyhow::bail!(
            "测试 WAV 必须是 16kHz mono 16-bit；实际 {sample_rate}Hz / {channels}ch / {bits_per_sample}bit"
        );
    }

    let data_end = (data_offset + data_size).min(bytes.len());
    let samples_i16 = &bytes[data_offset..data_end];
    let samples: Vec<f32> = samples_i16
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect();
    Ok(samples)
}
