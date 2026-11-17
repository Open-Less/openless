//! Offline A/B tool. Input must be canonical 16 kHz mono PCM WAV.
//! cargo run --release -p openless-core --example enhance_audio -- input.wav output.wav rnnoise
use openless_core::{AudioDspConfig, AudioProcessingConfig, AudioProcessor, NoiseReduction};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(3..=4).contains(&args.len()) {
        return Err(
            "usage: enhance_audio INPUT.wav OUTPUT.wav [rnnoise|purevox|spectral|off]".into(),
        );
    }
    if std::path::Path::new(&args[1]) == std::path::Path::new(&args[2]) {
        return Err("input and output must be different files".into());
    }
    let algorithm = match args.get(3).map(String::as_str).unwrap_or("rnnoise") {
        "rnnoise" => NoiseReduction::Rnnoise,
        "purevox" => NoiseReduction::Purevox,
        "spectral" => NoiseReduction::Spectral,
        "off" => NoiseReduction::Off,
        _ => return Err("unknown noise reduction algorithm".into()),
    };
    let wav = std::fs::read(&args[1])?;
    let pcm = openless_core::audio::decode_dictation_wav(&wav)?;
    let samples: Vec<f32> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| i16::from_le_bytes([p[0], p[1]]) as f32 / 32768.0)
        .collect();
    let mut processor = AudioProcessor::new(AudioProcessingConfig {
        enhancement_enabled: true,
        echo_cancellation_enabled: false,
        dsp: AudioDspConfig {
            noise_reduction: algorithm,
            ..Default::default()
        },
    });
    let started = std::time::Instant::now();
    let mut output = Vec::new();
    for input in samples.chunks(320) {
        if let Some(chunk) = processor.process(input, 1, 16_000) {
            output.extend(chunk.pcm_i16_le);
        }
    }
    if let Some(chunk) = processor.finish() {
        output.extend(chunk.pcm_i16_le);
    }
    let elapsed = started.elapsed().as_secs_f64();
    assert_eq!(
        output.len(),
        pcm.len(),
        "enhancement changed recording duration"
    );
    let result = openless_core::encode_dictation_wav(&output)?;
    // Create-new also rejects an existing output path and an alias/symlink to
    // the input, preserving recordings used for comparison.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?
        .write_all(&result)?;
    fn rms(pcm: &[u8]) -> f64 {
        let power = pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| (i16::from_le_bytes([p[0], p[1]]) as f64 / 32768.0).powi(2))
            .sum::<f64>();
        (power / (pcm.len() / 2).max(1) as f64).sqrt()
    }
    let duration = samples.len() as f64 / 16_000.0;
    println!("algorithm={algorithm:?} duration={duration:.3}s processing={elapsed:.3}s real_time_factor={:.3} input_rms={:.6} output_rms={:.6}", elapsed / duration.max(1e-9), rms(pcm), rms(&output));
    Ok(())
}
