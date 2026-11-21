//! Web Audio's existing two-note start cue, synthesized without a WebView.
//! The temporary output stream is owned by its short-lived worker thread.
use std::sync::atomic::{AtomicU64, Ordering};
static GENERATION: AtomicU64 = AtomicU64::new(0);
pub(crate) fn stop() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}
pub(crate) fn play_start() {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = std::thread::Builder::new()
        .name("openless-native-cue".into())
        .spawn(move || {
            if let Err(error) = play(generation) {
                log::debug!("[native-cue] {error}");
            }
        });
}
fn sample(time: f32) -> f32 {
    [(880.0, 0.0, 0.13, 0.16), (1108.73, 0.095, 0.17, 0.18)]
        .into_iter()
        .map(|(hz, start, duration, peak): (f32, f32, f32, f32)| {
            let t = time - start;
            if t < 0.0 || t >= duration {
                return 0.0;
            }
            let gain = if t < 0.005 {
                0.0001 * (peak / 0.0001).powf(t / 0.005)
            } else {
                peak * (0.0001 / peak).powf((t - 0.005) / (duration - 0.005))
            };
            (std::f32::consts::TAU * hz * t).sin() * gain
        })
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}
fn play(generation: u64) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default output device")?;
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0 as f32;
    macro_rules! stream {
        ($type:ty, $convert:expr) => {{
            let mut position = 0usize;
            device.build_output_stream(
                &config,
                move |data: &mut [$type], _: &cpal::OutputCallbackInfo| {
                    for frame in data.chunks_mut(channels) {
                        let value = if GENERATION.load(Ordering::Relaxed) == generation {
                            sample(position as f32 / sample_rate)
                        } else {
                            0.0
                        };
                        frame.fill($convert(value));
                        position += 1;
                    }
                },
                |error| log::debug!("[native-cue] output error: {error}"),
                None,
            )
        }};
    }
    let stream = match format {
        cpal::SampleFormat::F32 => stream!(f32, |value: f32| value),
        cpal::SampleFormat::I16 => stream!(i16, |value: f32| (value * i16::MAX as f32) as i16),
        cpal::SampleFormat::U16 => {
            stream!(u16, |value: f32| ((value + 1.0) * 0.5 * u16::MAX as f32)
                as u16)
        }
        _ => return Err("unsupported native cue output format".into()),
    }
    .map_err(|error| error.to_string())?;
    stream.play().map_err(|error| error.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(320));
    drop(stream);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn cue_is_silent_after_the_existing_web_audio_envelope() {
        assert_eq!(super::sample(0.0), 0.0);
        assert_eq!(super::sample(0.266), 0.0);
        assert!(super::sample(0.100).abs() <= 0.34);
    }
}
