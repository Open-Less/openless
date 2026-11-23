//! Android 系统音频预处理采集（`AudioRecord` + 平台音效）。
//!
//! 这是"录音链路的前端"：打开 `AudioRecord`、按需启用 AEC/降噪/AGC、把 48 kHz 单声道
//! f32 帧推给调用方。缓冲区、重采样、诊断与 WAV 归档仍在宿主的录音状态机里。
//!
//! 不依赖宿主类型：帧用闭包投递，错误用另一个闭包上报，频谱通过注入的 sink 发出。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use std::sync::mpsc::Receiver;

use crate::android::jni::android::with_android_env;
use jni::{
    objects::{GlobalRef, JValue},
    JNIEnv, JavaVM,
};

/// 宿主请求的系统音效开关。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AndroidDspConfig {
    pub aec: bool,
    pub noise_suppression: bool,
    pub agc: bool,
}

impl AndroidDspConfig {
    /// 三个音效都没开时，宿主应走原生采集而不是这条链路。
    pub fn any_enabled(&self) -> bool {
        self.aec || self.noise_suppression || self.agc
    }
}

/// 帧回调：48 kHz 单声道 f32 样本。
pub type AndroidFrameSink = Arc<dyn Fn(&[f32]) + Send + Sync>;

/// 运行期错误回调。
pub type AndroidErrorSink = Arc<dyn Fn(String) + Send + Sync>;

type SpectrumSink = Box<dyn Fn(&[f32], &[f32]) + Send + Sync>;
static SPECTRUM: std::sync::OnceLock<SpectrumSink> = std::sync::OnceLock::new();

/// 注册频谱回调（宿主把它接到 Kotlin 侧的动画通道）。
pub fn install_spectrum_sink(sink: SpectrumSink) {
    let _ = SPECTRUM.set(sink);
}

static OWNER: AtomicBool = AtomicBool::new(false);
static STATUS: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);
/// 采集采样率（Android 音频预处理约定 48 kHz）。
pub const RATE: u32 = 48_000;
const SAMPLES: usize = 960; // 20 ms; bounds JNI work and processing latency.
static MONITOR_UNTIL: AtomicU64 = AtomicU64::new(0);
static MONITOR_NEXT: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn set_monitoring(enabled: bool) {
    MONITOR_UNTIL.store(if enabled { now_ms() + 2500 } else { 0 }, Ordering::Release);
}

pub fn monitoring_enabled() -> bool {
    now_ms() < MONITOR_UNTIL.load(Ordering::Acquire)
}

pub fn publish_monitor(processor: &openless_core::AudioProcessor) {
    let now = now_ms();
    if now < MONITOR_NEXT.load(Ordering::Relaxed) || !monitoring_enabled() {
        return;
    }
    MONITOR_NEXT.store(now + 100, Ordering::Relaxed);
    if let Some((raw, processed)) = processor.spectrums() {
        if let Some(sink) = SPECTRUM.get() {
            sink(&raw, &processed);
        }
    }
}
const EFFECTS: [&str; 3] = [
    "android/media/audiofx/AcousticEchoCanceler",
    "android/media/audiofx/NoiseSuppressor",
    "android/media/audiofx/AutomaticGainControl",
];

pub fn record_failure(error: String) {
    *STATUS.lock().unwrap() = Some(serde_json::json!({"active": false, "error": error}));
}

fn available(env: &mut JNIEnv, class: &str) -> bool {
    let result = env
        .call_static_method(class, "isAvailable", "()Z", &[])
        .and_then(|v| v.z());
    if result.is_err() {
        let _ = env.exception_clear();
    }
    result.unwrap_or(false)
}

pub fn status() -> serde_json::Value {
    let capabilities = with_android_env(|env, _| Ok(EFFECTS.map(|class| available(env, class))))
        .unwrap_or([false; 3]);
    serde_json::json!({
        "aecAvailable": capabilities[0], "noiseSuppressionAvailable": capabilities[1],
        "agcAvailable": capabilities[2], "runtime": STATUS.lock().unwrap().clone(),
    })
}

pub fn prioritize_current_thread() {
    let _ = with_android_env(|env, _| {
        let result = env.call_static_method(
            "android/os/Process",
            "setThreadPriority",
            "(I)V",
            &[JValue::Int(-16)],
        );
        if result.is_err() {
            let _ = env.exception_clear();
        }
        result.map(|_| ()).map_err(|e| e.to_string())
    });
}

struct Owner;
impl Drop for Owner {
    fn drop(&mut self) {
        OWNER.store(false, Ordering::Release);
    }
}

struct Record {
    vm: JavaVM,
    recorder: GlobalRef,
    effects: Vec<GlobalRef>,
    _owner: Owner,
}

impl Drop for Record {
    fn drop(&mut self) {
        if let Ok(mut env) = self.vm.attach_current_thread() {
            let _ = env.call_method(self.recorder.as_obj(), "stop", "()V", &[]);
            let _ = env.exception_clear();
            for effect in &self.effects {
                let _ = env.call_method(effect.as_obj(), "release", "()V", &[]);
                let _ = env.exception_clear();
            }
            let _ = env.call_method(self.recorder.as_obj(), "release", "()V", &[]);
            let _ = env.exception_clear();
        }
        *STATUS.lock().unwrap() = Some(serde_json::json!({"active": false, "error": null}));
    }
}

pub struct AndroidCapture {
    stop: Arc<AtomicBool>,
    done: Receiver<()>,
}

impl AndroidCapture {
    pub fn start(
        config: AndroidDspConfig,
        on_frame: AndroidFrameSink,
        on_error: AndroidErrorSink,
    ) -> Result<Self, String> {
        OWNER
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                "busy: previous Android AudioRecord teardown has not completed".to_string()
            })?;
        let owner = Owner;
        let requested = [config.aec, config.noise_suppression, config.agc];
        let record = with_android_env(|env, _| {
            let result = (|| -> Result<Record, jni::errors::Error> {
                let min = env
                    .call_static_method(
                        "android/media/AudioRecord",
                        "getMinBufferSize",
                        "(III)I",
                        &[JValue::Int(RATE as i32), JValue::Int(16), JValue::Int(2)],
                    )?
                    .i()?;
                if min <= 0 {
                    return Err(jni::errors::Error::NullPtr("AudioRecord minimum buffer"));
                }
                let recorder = env.new_object(
                    "android/media/AudioRecord",
                    "(IIIII)V",
                    &[
                        JValue::Int(if requested[0] { 7 } else { 6 }), // communication / recognition
                        JValue::Int(RATE as i32),
                        JValue::Int(16),
                        JValue::Int(2),
                        JValue::Int(min.max((SAMPLES * 4) as i32)),
                    ],
                )?;
                let mut record = Record {
                    vm: env.get_java_vm()?,
                    recorder: env.new_global_ref(&recorder)?,
                    effects: Vec::new(),
                    _owner: owner,
                };
                if env.call_method(&recorder, "getState", "()I", &[])?.i()? != 1 {
                    return Err(jni::errors::Error::NullPtr("AudioRecord initialization"));
                }
                let session = env
                    .call_method(&recorder, "getAudioSessionId", "()I", &[])?
                    .i()?;
                for (class, enabled) in EFFECTS.into_iter().zip(requested) {
                    if !available(env, class) {
                        if enabled {
                            return Err(jni::errors::Error::NullPtr(
                                "requested Android audio effect unavailable",
                            ));
                        }
                        continue;
                    }
                    let signature = format!("(I)L{class};");
                    let effect = env
                        .call_static_method(class, "create", &signature, &[JValue::Int(session)])?
                        .l()?;
                    if effect.is_null() {
                        if enabled {
                            return Err(jni::errors::Error::NullPtr(
                                "Android audio effect creation",
                            ));
                        }
                        continue;
                    }
                    // Own it before enabling so failed setup also releases it.
                    record.effects.push(env.new_global_ref(&effect)?);
                    let code = env
                        .call_method(
                            &effect,
                            "setEnabled",
                            "(Z)I",
                            &[JValue::Bool(u8::from(enabled))],
                        )?
                        .i()?;
                    let actual = env.call_method(&effect, "getEnabled", "()Z", &[])?.z()?;
                    if code != 0 || actual != enabled {
                        return Err(jni::errors::Error::NullPtr(
                            "Android audio effect rejected setting",
                        ));
                    }
                }
                env.call_method(&recorder, "startRecording", "()V", &[])?;
                if env
                    .call_method(&recorder, "getRecordingState", "()I", &[])?
                    .i()?
                    != 3
                {
                    return Err(jni::errors::Error::NullPtr(
                        "Android recording did not start",
                    ));
                }
                Ok(record)
            })();
            if result.is_err() {
                let _ = env.exception_clear();
            }
            result.map_err(|error| error.to_string())
        })?;
        *STATUS.lock().unwrap() = Some(
            serde_json::json!({"active": true, "aec": requested[0], "noiseSuppression": requested[1], "agc": requested[2], "error": null}),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (done_tx, done) = std::sync::mpsc::channel();
        thread::Builder::new()
            .name("openless-android-audio".into())
            .spawn(move || {
                let result = read_audio(&record, &on_frame, &worker_stop);
                if let Err(error) = result {
                    if !worker_stop.load(Ordering::Acquire) {
                        on_error(format!("Android AudioRecord: {error}"));
                    }
                }
                drop(record);
                let _ = done_tx.send(());
            })
            .map_err(|error| error.to_string())?;
        Ok(Self { stop, done })
    }
}

fn read_audio(
    record: &Record,
    on_frame: &AndroidFrameSink,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut env = record
        .vm
        .attach_current_thread()
        .map_err(|e| e.to_string())?;
    let result = env.call_static_method(
        "android/os/Process",
        "setThreadPriority",
        "(I)V",
        &[JValue::Int(-16)],
    );
    if result.is_err() {
        let _ = env.exception_clear();
    }
    let array = env
        .new_short_array(SAMPLES as i32)
        .map_err(|e| e.to_string())?;
    let mut samples = vec![0i16; SAMPLES];
    let result = (|| -> Result<(), jni::errors::Error> {
        while !stop.load(Ordering::Acquire) {
            // Nonblocking reads keep stop independent of an OEM's blocking read.
            let count = env
                .call_method(
                    record.recorder.as_obj(),
                    "read",
                    "([SIII)I",
                    &[
                        JValue::Object(array.as_ref()),
                        JValue::Int(0),
                        JValue::Int(SAMPLES as i32),
                        JValue::Int(1),
                    ],
                )?
                .i()?;
            if count < 0 {
                return Err(jni::errors::Error::NullPtr("AudioRecord read failed"));
            }
            if count == 0 {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            env.get_short_array_region(&array, 0, &mut samples[..count as usize])?;
            if stop.load(Ordering::Acquire) {
                break;
            }
            let frame: Vec<f32> = samples[..count as usize]
                .iter()
                .map(|s| *s as f32 / 32768.0)
                .collect();
            on_frame(&frame);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = env.exception_clear();
    }
    result.map_err(|error| error.to_string())
}

impl Drop for AndroidCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Some OEMs block stop/release. Keep their owner gate held in that
        // thread, but never make UI cancellation wait for native teardown.
        if self.done.recv_timeout(Duration::from_millis(250)).is_err() {
            log::warn!("[android-audio] AudioRecord teardown is still pending");
        }
    }
}
