use super::tests::decode;
use super::*;
use crate::{AudioDspConfig, NoiseReduction};

fn config(algorithm: NoiseReduction) -> AudioProcessingConfig {
    AudioProcessingConfig {
        enhancement_enabled: true,
        dsp: AudioDspConfig {
            noise_reduction: algorithm,
            agc_enabled: false,
            ..Default::default()
        },
    }
}

fn render(
    input: &[f32],
    rate: u32,
    channels: usize,
    block: usize,
    config: AudioProcessingConfig,
) -> Vec<u8> {
    let mut processor = AudioProcessor::new(config);
    let mut bytes = Vec::new();
    for chunk in input.chunks(block) {
        if let Some(output) = processor.process(chunk, channels, rate) {
            assert!(output.level.is_finite() && (0.0..=1.0).contains(&output.level));
            bytes.extend(output.pcm_i16_le);
        }
    }
    if let Some(output) = processor.finish() {
        bytes.extend(output.pcm_i16_le);
    }
    assert!(processor.finish().is_none());
    assert!(processor.process(&[0.5; 480], 1, rate).is_none());
    bytes
}

fn signal(size: usize) -> Vec<f32> {
    (0..size).map(|i| (i as f32 * 0.073).sin() * 0.2).collect()
}

#[test]
fn enhanced_output_is_independent_of_callback_boundaries() {
    for algorithm in [
        NoiseReduction::Off,
        NoiseReduction::Rnnoise,
        NoiseReduction::Spectral,
        NoiseReduction::Purevox,
    ] {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let input = signal(rate as usize / 12 + 37);
            let whole = render(&input, rate, 1, input.len(), config(algorithm));
            let partitioned = render(&input, rate, 1, 137, config(algorithm));
            assert_eq!(
                whole.len(),
                (input.len() as u64 * 16_000).div_ceil(rate as u64) as usize * 2
            );
            assert_eq!(whole, partitioned, "algorithm={algorithm:?}, rate={rate}");
        }
    }
}

#[test]
fn short_recordings_flush_without_padding_or_losing_samples() {
    for algorithm in [
        NoiseReduction::Off,
        NoiseReduction::Rnnoise,
        NoiseReduction::Purevox,
    ] {
        for len in [1, 7, 159, 160, 161, 479, 480, 481] {
            let pcm = render(&signal(len), 16_000, 1, 13, config(algorithm));
            assert_eq!(pcm.len(), len * 2, "algorithm={algorithm:?}, len={len}");
        }
    }
}

#[test]
fn dry_mix_is_aligned_and_preserves_the_last_impulse() {
    // At 1% wet, the dry component should keep its original time position.
    let mut input = vec![0.0; 1600];
    input[160] = 0.5;
    input[1590] = -0.5;
    let mut c = config(NoiseReduction::Rnnoise);
    c.dsp.noise_strength = 1;
    let pcm = decode(&render(&input, 16_000, 1, 73, c));
    assert!(pcm[160] > 10_000, "first impulse moved or lost");
    assert!(pcm[1590] < -10_000, "last impulse moved or lost");
}

#[test]
fn stereo_downmix_and_partial_channel_frames_are_consistent() {
    let mono = signal(1733);
    let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
    let c = config(NoiseReduction::Rnnoise);
    assert_eq!(
        render(&mono, 44_100, 1, 51, c),
        render(&stereo, 44_100, 2, 79, c)
    );
}

#[test]
fn invalid_samples_are_sanitized_and_reset_restarts_the_session() {
    let mut p = AudioProcessor::new(config(NoiseReduction::Rnnoise));
    assert!(p.process(&[], 1, 16_000).is_none());
    assert!(p.process(&[1.0], 0, 16_000).is_none());
    assert!(p.process(&[1.0], 1, 0).is_none());
    let _ = p.process(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY], 1, 16_000);
    let output = p.finish().unwrap();
    assert!(output.level.is_finite());
    assert_eq!(output.pcm_i16_le.len(), 6);
    p.reset();
    let input = signal(1733);
    let mut output = p.process(&input, 1, 16_000).unwrap().pcm_i16_le;
    output.extend(p.finish().unwrap().pcm_i16_le);
    assert_eq!(
        output,
        render(&input, 16_000, 1, 99, config(NoiseReduction::Rnnoise))
    );
}

#[test]
fn resampling_suppresses_out_of_band_tones() {
    fn tone(frequency: f32) -> Vec<f32> {
        (0..4800)
            .map(|i| (std::f32::consts::TAU * frequency * i as f32 / 48_000.0).sin() * 0.3)
            .collect()
    }
    fn rms(pcm: Vec<u8>) -> f64 {
        let samples = decode(&pcm);
        (samples[160..samples.len() - 160]
            .iter()
            .map(|s| f64::from(*s).powi(2))
            .sum::<f64>()
            / (samples.len() - 320) as f64)
            .sqrt()
    }
    let c = config(NoiseReduction::Off);
    let speech_band = rms(render(&tone(1000.0), 48_000, 1, 197, c));
    let alias_band = rms(render(&tone(12_000.0), 48_000, 1, 197, c));
    assert!(
        alias_band < speech_band * 0.02,
        "alias RMS {alias_band}, passband RMS {speech_band}"
    );
}

#[test]
fn partial_preferences_migrate_and_invalid_controls_are_bounded() {
    let config: AudioDspConfig = serde_json::from_str(r#"{"noiseReduction":"purevox"}"#).unwrap();
    assert_eq!(config.noise_reduction, NoiseReduction::Purevox);
    assert!(config.agc_enabled);
    let mut config = config;
    config.gain_db = 127;
    config.noise_strength = 255;
    config.processing_order.fill(crate::AudioDspStage::Agc);
    let normalized = config.normalized();
    assert_eq!(normalized.gain_db, 30);
    assert_eq!(normalized.noise_strength, 100);
    assert_eq!(
        normalized.processing_order,
        AudioDspConfig::default().processing_order
    );
}

#[test]
fn default_agc_does_not_amplify_denoised_room_noise_back_to_original_level() {
    let mut seed = 0x12345678u32;
    let noise: Vec<f32> = (0..48_000)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            ((seed >> 8) as f32 / 16_777_216.0 - 0.5) * 0.1
        })
        .collect();
    for algorithm in [NoiseReduction::Rnnoise, NoiseReduction::Purevox] {
        let config = AudioProcessingConfig {
            enhancement_enabled: true,
            dsp: AudioDspConfig {
                noise_reduction: algorithm,
                ..Default::default()
            },
        };
        let output = decode(&render(&noise, 16_000, 1, 320, config));
        let input_power = noise[32_000..]
            .iter()
            .map(|s| f64::from(*s).powi(2))
            .sum::<f64>();
        let output_power = output[32_000..]
            .iter()
            .map(|s| (f64::from(*s) / 32768.0).powi(2))
            .sum::<f64>();
        assert!(
            output_power < input_power * 0.25,
            "{algorithm:?}: output/input power {}",
            output_power / input_power
        );
    }
}
