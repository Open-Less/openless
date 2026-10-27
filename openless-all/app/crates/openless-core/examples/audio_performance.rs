//! Full capture-processing throughput probe; no ASR/network/device timing.
use openless_core::{AudioDspConfig, AudioProcessingConfig, AudioProcessor, NoiseReduction};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args.get(1).map(String::as_str).unwrap_or("rnnoise");
    let rate: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(48_000);
    let mut processor = AudioProcessor::new(AudioProcessingConfig {
        enhancement_enabled: name != "disabled",
        echo_cancellation_enabled: false,
        dsp: AudioDspConfig {
            noise_reduction: match name {
                "purevox" => NoiseReduction::Purevox,
                "spectral" => NoiseReduction::Spectral,
                "off" | "disabled" => NoiseReduction::Off,
                _ => NoiseReduction::Rnnoise,
            },
            ..Default::default()
        },
    });
    let samples: Vec<f32> = (0..rate / 50)
        .map(|i| {
            let t = i as f32 / rate as f32;
            0.1 * (std::f32::consts::TAU * 230.0 * t).sin()
                + 0.015 * (std::f32::consts::TAU * 1733.0 * t).sin()
        })
        .collect();
    let cold = Instant::now();
    let mut output_bytes = processor
        .process(&samples, 1, rate)
        .map_or(0, |c| c.pcm_i16_le.len());
    let first_ms = cold.elapsed().as_secs_f64() * 1000.0;
    let mut times = Vec::with_capacity(1499);
    let steady = Instant::now();
    for _ in 1..1500 {
        let start = Instant::now();
        output_bytes += processor
            .process(&samples, 1, rate)
            .map_or(0, |c| c.pcm_i16_le.len());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let seconds = steady.elapsed().as_secs_f64();
    output_bytes += processor.finish().map_or(0, |c| c.pcm_i16_le.len());
    assert_eq!(output_bytes, 30 * 16_000 * 2);
    times.sort_by(f64::total_cmp);
    println!("algorithm={name} rate={rate} audio_seconds=30 first_callback_ms={first_ms:.3} steady_seconds={seconds:.3} rtf={:.4} p50_ms={:.3} p95_ms={:.3} max_ms={:.3} output_bytes={output_bytes}", seconds / 29.98, times[times.len()/2], times[times.len()*95/100], times[times.len()-1]);
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|s| s.starts_with("VmRSS:") || s.starts_with("VmHWM:"))
        {
            println!("{line}");
        }
    }
}
