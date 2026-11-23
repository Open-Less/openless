//! Android 系统音频预处理采集（宿主适配层）。
//!
//! 打开 `AudioRecord`、音效开关、读取循环都在平台层
//! （`openless-platform::audio::capture::android_capture`）。宿主这里只做三件事：
//! 1. 把平台帧转成录音管线的 `RawAudioFrame`；
//! 2. 维护自己的"是否接收音频"门控与诊断；
//! 3. 把频谱转发给 Kotlin 动画通道。

use super::*;
use openless_platform::audio::capture::android_capture as platform;

pub use platform::{monitoring_enabled, prioritize_current_thread, record_failure, set_monitoring};

/// 把处理器频谱转给 Kotlin 动画通道（有速率限制，见平台层实现）。
pub fn publish_monitor(processor: &openless_core::AudioProcessor) {
    platform::publish_monitor(processor);
}

/// 把平台层的频谱回调接到 Kotlin 通道。
pub fn install_spectrum_sink() {
    platform::install_spectrum_sink(Box::new(|raw, processed| {
        crate::android::native_bridge::emit_audio_spectrum(raw.to_vec(), processed.to_vec());
    }));
}

pub fn status() -> serde_json::Value {
    platform::status()
}

/// 宿主侧采集句柄：持有平台句柄与自己的停止标志。
#[allow(dead_code)]
pub struct AndroidCapture {
    inner: platform::AndroidCapture,
}

impl AndroidCapture {
    pub fn start(
        config: openless_core::AudioDspConfig,
        sender: AudioProcessingSender,
        state: Arc<StreamState>,
        errors: Sender<RecorderError>,
    ) -> Result<Self, String> {
        let dsp = platform::AndroidDspConfig {
            aec: config.android_aec_enabled,
            noise_suppression: config.android_noise_suppression_enabled,
            agc: config.android_agc_enabled,
        };
        state.accepting_audio.store(true, Ordering::Release);

        let frame_state = Arc::clone(&state);
        let frame_sender = sender.clone();
        let on_frame: platform::AndroidFrameSink = Arc::new(move |samples: &[f32]| {
            if !frame_state.accepting_audio.load(Ordering::Acquire) {
                return;
            }
            frame_state.note_first_pcm(false);
            *frame_state.last_callback_time.lock() = Some(std::time::Instant::now());
            let frame = RawAudioFrame {
                captured_at: std::time::Instant::now(),
                samples: samples.to_vec(),
                channels: 1,
                input_sample_rate: platform::RATE,
            };
            let _ = frame_sender.try_send(AudioProcessingMessage::Frame(frame));
        });

        let error_sender = errors;
        let on_error: platform::AndroidErrorSink = Arc::new(move |message: String| {
            let _ = error_sender.send(RecorderError::EngineFailed(message));
        });

        let inner = platform::AndroidCapture::start(dsp, on_frame, on_error)?;
        Ok(Self { inner })
    }
}
