//! Export deterministic PCM for comparison across revisions and architectures.
//! Usage: audio_equivalence OUTPUT_DIRECTORY
use openless_core::{AudioDspConfig, AudioProcessingConfig, AudioProcessor, NoiseReduction};
use std::{fs, path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("output directory required")?);
    fs::create_dir(&root)?;
    let mut rows = Vec::new();
    for rate in [16_000, 44_100, 48_000] {
        for channels in [1, 2] {
            for (name, algorithm) in [
                ("off", NoiseReduction::Off),
                ("rnnoise", NoiseReduction::Rnnoise),
                ("purevox", NoiseReduction::Purevox),
                ("spectral", NoiseReduction::Spectral),
            ] {
                for effects in [false, true] {
                    let config = AudioDspConfig {
                        noise_reduction: algorithm,
                        gain_db: if effects { 3 } else { 0 },
                        equalizer_enabled: effects,
                        equalizer_bands_db: if effects {
                            [-2, -1, 0, 1, 2, 3, 4, 4, 3, 2]
                        } else {
                            [0; 10]
                        },
                        dereverb_level: if effects { 12 } else { 0 },
                        vad_enabled: effects,
                        vad_threshold_db: -55,
                        ..Default::default()
                    };
                    let mut processor = AudioProcessor::new(AudioProcessingConfig {
                        enhancement_enabled: true,
                        echo_cancellation_enabled: false,
                        dsp: config,
                    });
                    let mut random = 42u32;
                    let mut input = Vec::new();
                    // Silence, speech-band harmonics with deterministic noise,
                    // near-Nyquist interference, impulses, and an unaligned tail.
                    for i in 0..rate + 37 {
                        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                        let t = i as f32 / rate as f32;
                        let noise = ((random >> 8) as f32 / 16777216.0 - 0.5) * 0.025;
                        let signal = if i < rate / 5 {
                            0.0
                        } else {
                            0.13 * (std::f32::consts::TAU * 230.0 * t).sin()
                                + 0.04 * (std::f32::consts::TAU * 1733.0 * t).sin()
                                + 0.02 * (std::f32::consts::TAU * rate as f32 * 0.43 * t).sin()
                                + noise
                        } + if i == rate / 2 { 0.4 } else { 0.0 };
                        for channel in 0..channels {
                            input.push(signal * if channel == 0 { 1.0 } else { 0.7 });
                        }
                    }
                    let start = Instant::now();
                    let mut pcm = Vec::new();
                    let mut offset = 0;
                    for size in [37, 960, 127, 4096].into_iter().cycle() {
                        if offset == input.len() {
                            break;
                        }
                        let end = (offset + size).min(input.len());
                        if let Some(chunk) = processor.process(&input[offset..end], channels, rate)
                        {
                            pcm.extend(chunk.pcm_i16_le);
                        }
                        offset = end;
                    }
                    if let Some(chunk) = processor.finish() {
                        pcm.extend(chunk.pcm_i16_le);
                    }
                    assert_eq!(
                        pcm.len(),
                        ((rate as u64 + 37) * 16000).div_ceil(rate as u64) as usize * 2
                    );
                    let file = format!("{name}-{rate}-{channels}-{effects}.pcm");
                    rows.push(serde_json::json!({"file":file,"seconds":start.elapsed().as_secs_f64(),"samples":pcm.len()/2}));
                    fs::write(root.join(file), pcm)?;
                }
            }
        }
    }
    fs::write(root.join("results.json"), serde_json::to_vec_pretty(&rows)?)?;
    Ok(())
}
