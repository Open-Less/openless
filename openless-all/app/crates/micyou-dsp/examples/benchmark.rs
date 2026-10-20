//! Synthetic throughput check; does not measure speech recognition accuracy.
use micyou_dsp::{AudioDspSettings, DspProcessor};
use std::sync::{Arc, RwLock};

fn main() {
    for algorithm in ["RNNoise", "PureVox", "Speexdsp"] {
        let settings = AudioDspSettings {
            ns_enabled: true,
            ns_type: algorithm.into(),
            ns_intensity: 80.0,
            agc_enabled: true,
            ..Default::default()
        };
        let mut dsp = DspProcessor::new(Arc::new(RwLock::new(settings)), None);
        let mut state = 0x12345678u32;
        let started = std::time::Instant::now();
        for _ in 0..500 {
            let mut frame: Vec<f32> = (0..480)
                .map(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    ((state >> 8) as f32 / 16_777_216.0 - 0.5) * 0.1
                })
                .collect();
            dsp.process(&mut frame, 1, 0.0);
            assert_eq!(frame.len(), 480);
            assert!(frame.iter().all(|s| s.is_finite()));
        }
        let time = started.elapsed().as_secs_f64();
        println!(
            "{algorithm}: 5.00s audio, {time:.3}s processing, real-time factor {:.3}",
            time / 5.0
        );
    }
}
