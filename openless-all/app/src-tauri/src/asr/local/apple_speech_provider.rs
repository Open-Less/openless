//! Apple Speech local ASR adapter (macOS, issue #574).
//!
//! Wraps Apple's `SFSpeechRecognizer` as a 4th local provider with the same
//! shape as `LocalQwenAsr`: implements `crate::recorder::AudioConsumer` to
//! accumulate PCM into a buffer, and `transcribe()` returns
//! `RawTranscript{text, duration_ms}`.
//!
//! Batch processing: buffered 16k/mono/16-bit PCM is written to a temp wav via
//! `encode_wav_16k_mono` and fed to `SFSpeechURLRecognitionRequest`, avoiding
//! the objc2 bridging of `AVAudioPCMBuffer` / `AVAudioFormat`. Real-time
//! partial streaming is future work.
//!
//! Authorization uses `SFSpeechRecognizer.requestAuthorization:` (completion
//! handler block), mirroring `requestAccessForMediaType:` in `permissions.rs`.
//! `transcribe()` returns a clear error when unauthorized.
//!
//! This module is not compiled on non-macOS platforms (see the cfg gate in
//! `mod.rs`).

#![cfg(target_os = "macos")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use block2::RcBlock;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, sel};
use parking_lot::Mutex;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

/// `SFSpeechRecognizerAuthorizationStatus` (NS_ENUM(NSInteger)).
const SF_AUTH_NOT_DETERMINED: i64 = 0;
const SF_AUTH_DENIED: i64 = 1;
const SF_AUTH_RESTRICTED: i64 = 2;
const SF_AUTH_AUTHORIZED: i64 = 3;

/// Lower bound for the recognition callback wait. Recognition also has a
/// coordinator-side dynamic timeout; this only guards against the block never
/// firing and the thread blocking forever. Scaled with audio length; see
/// `recognition_wait_budget`.
const RECOGNITION_WAIT: Duration = Duration::from_secs(60);
/// Polling interval for the recognition wait. Each round checks `cancel_flag`
/// and task state, so after cancel/timeout the blocked thread exits within one
/// interval (~100ms) instead of waiting out the full budget.
const RECOGNITION_POLL: Duration = Duration::from_millis(100);
/// `completed` value of `SFSpeechRecognitionTaskState` (NS_ENUM(NSInteger)).
/// Entered once the task finishes (success, failure, or cancel) — the
/// authoritative "no more callbacks" signal.
const SF_TASK_STATE_COMPLETED: i64 = 4;
/// Grace period after observing task completion, so an in-flight final
/// resultHandler callback still lands in the accumulator instead of the
/// "state flips first, callback arrives later" race dropping the last segment.
const COMPLETION_GRACE: Duration = Duration::from_millis(250);
/// Fallback finish condition: recognition ends after isFinal plus this much
/// callback silence. Only guards against `state` polling never observing
/// completed; the normal path settles via completed + COMPLETION_GRACE. With
/// multiple segments isFinal can appear per utterance, and inter-utterance
/// callback gaps (long pauses in the audio) must stay far below this
/// threshold, or late text would be cut off.
const FINAL_QUIESCENCE: Duration = Duration::from_secs(5);
const AUTHORIZATION_WAIT: Duration = Duration::from_secs(30);
/// Poll wait for engine availability (isAvailable): a freshly initialized
/// recognizer is often briefly unavailable while language resources load
/// asynchronously, then becomes ready. Error only after the full wait elapses.
const AVAILABILITY_WAIT: Duration = Duration::from_secs(3);
const AVAILABILITY_POLL: Duration = Duration::from_millis(100);

/// Raw-pointer wrapper for `SFSpeechRecognitionTask`, used only to store the
/// task handle from the spawn_blocking thread into
/// `AppleSpeechAsr::active_task` so any thread (including one being cancelled
/// on the tokio runtime) can call `-[SFSpeechRecognitionTask cancel]` to stop
/// recognition.
///
/// SAFETY: `SFSpeechRecognitionTask` is a standard objc/ARC object whose
/// `cancel` is documented by Speech.framework as safe to call from any thread
/// (it dispatches to its own queue internally). The pointer is only stored in
/// `active_task` or passed to `cancel` — never dereferenced or mutated. It is
/// held only while the recognition request is alive: `recognize_file` resets
/// `active_task` to `None` before returning, while recognizer / request still
/// strongly reference the task from the same stack frame, so it is not freed
/// early. Passing the raw pointer across threads and calling `cancel` is
/// therefore memory- and thread-safe. `Sync` is not implemented — the wrapper
/// is only used after being taken out under a `Mutex`, with no concurrent
/// shared references.
struct SendableTask(*mut AnyObject);

// SAFETY: see the `SendableTask` doc comment — the underlying
// SFSpeechRecognitionTask is thread-safe, `cancel` can be called
// cross-thread, and the wrapper only carries the pointer for storing and
// cancelling.
unsafe impl Send for SendableTask {}

pub struct AppleSpeechAsr {
    /// Buffer of 16-bit LE PCM bytes (stores whatever the recorder pushes).
    /// Mirrors LocalQwenAsr.
    buffer: Mutex<Vec<u8>>,
    /// Recognition locale (Apple identifier, e.g. "zh-CN"); None = system
    /// default. Mapped from the user's working language — one
    /// SFSpeechRecognizer instance handles a single language, and without an
    /// explicit locale it falls back to the system preferred language (often
    /// English), which mis-recognizes other languages (root cause of a
    /// user-reported bug).
    locale: Option<String>,
    /// Cancel flag. Set by `cancel()`; the wait loop in `recognize_file`
    /// checks it each round and, when set, gives up waiting and cancels the
    /// underlying recognition task — so spawn_blocking threads abandoned by
    /// the outer dynamic timeout or by `cancel()` exit within ~100ms instead
    /// of waiting out `RECOGNITION_WAIT`.
    cancel_flag: Arc<AtomicBool>,
    /// Handle of the in-flight recognition task. `recognize_file` stores it as
    /// soon as the task exists and clears it before returning; `cancel()`
    /// takes it and calls `-[SFSpeechRecognitionTask cancel]` to stop
    /// recognition.
    active_task: Arc<Mutex<Option<SendableTask>>>,
}

impl AppleSpeechAsr {
    pub fn new(locale: Option<String>) -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
            locale,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            active_task: Arc::new(Mutex::new(None)),
        }
    }

    /// Duration (ms) of buffered audio. Matches
    /// LocalQwenAsr::buffer_duration_ms; the coordinator uses it to compute
    /// dynamic timeouts for local providers. Does not consume the buffer.
    pub fn buffer_duration_ms(&self) -> u64 {
        (self.buffer.lock().len() as u64 / 2) * 1000 / 16_000
    }

    /// Called on stop: encode the buffer into a temp wav, feed it to
    /// `SFSpeechURLRecognitionRequest`, and return the synchronized result.
    ///
    /// On failure the buffer is kept (consistent with WhisperBatchASR /
    /// LocalQwenAsr): denied permission or recognition failure must not
    /// discard the user's recording. Only the success path clears the buffer.
    pub async fn transcribe(&self) -> Result<RawTranscript> {
        // clone instead of take: called once per session, a few MB is
        // acceptable, and the buffer survives failures.
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }
        let duration_ms = (pcm.len() as u64 / 2) * 1000 / 16_000;
        let locale = self.locale.clone();

        // Reset the cancel flag before this recognition: a previous session
        // that ended cancelled may have left it true.
        self.cancel_flag.store(false, Ordering::SeqCst);
        let cancel_flag = Arc::clone(&self.cancel_flag);
        let active_task = Arc::clone(&self.active_task);

        // SFSpeechRecognizer bridges synchronously over the objc runloop and
        // blocks; run it in spawn_blocking to keep the tokio runtime free,
        // via the same Tauri-owned runtime handle as LocalQwenAsr.
        let result = tauri::async_runtime::spawn_blocking(move || {
            transcribe_pcm_blocking(
                &pcm,
                duration_ms,
                locale.as_deref(),
                &cancel_flag,
                &active_task,
            )
        })
        .await
        .context("apple-speech transcribe spawn_blocking join 失败")?;

        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    pub fn cancel(&self) {
        // Set the cancel flag first: the wait loop sees it on its next round
        // (~100ms) and gives up waiting, releasing the blocked thread.
        self.cancel_flag.store(true, Ordering::SeqCst);
        // Then actually terminate the in-flight recognition task, if any,
        // calling cancel immediately after taking the handle.
        if let Some(task) = self.active_task.lock().take() {
            // SAFETY: `task.0` is the SFSpeechRecognitionTask pointer returned
            // by `recognitionTaskWithRequest:`. `-[SFSpeechRecognitionTask
            // cancel]` takes no arguments, returns nothing, and is documented
            // by Speech.framework as callable from any thread. Only cancel is
            // invoked, the pointer is not dereferenced, and the handle is not
            // used again (already taken out of the Option).
            let _: () = unsafe { msg_send![task.0, cancel] };
            log::info!("[apple-speech] recognition task cancelled");
        }
        self.buffer.lock().clear();
    }
}

impl Default for AppleSpeechAsr {
    fn default() -> Self {
        Self::new(None)
    }
}

impl crate::recorder::AudioConsumer for AppleSpeechAsr {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// Write the PCM to a temp wav, ensure authorization, run batch recognition,
/// delete the temp file, and return the result. Runs synchronously on a
/// spawn_blocking thread.
fn transcribe_pcm_blocking(
    pcm: &[u8],
    duration_ms: u64,
    locale: Option<&str>,
    cancel_flag: &AtomicBool,
    active_task: &Mutex<Option<SendableTask>>,
) -> Result<RawTranscript> {
    ensure_authorized()?;

    let samples: Vec<i16> = pcm
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    let wav = encode_wav_16k_mono(&samples);

    // Temp wav: unique filename avoids concurrent-session collisions; deleted
    // when done (RAII guard).
    let path = std::env::temp_dir().join(format!(
        "openless-apple-speech-{}-{}.wav",
        std::process::id(),
        unique_suffix()
    ));
    std::fs::write(&path, &wav).with_context(|| format!("写临时 wav 失败: {}", path.display()))?;
    let _cleanup = TempFileGuard(&path);

    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("临时 wav 路径含非 UTF-8 字符: {}", path.display()))?;
    let text = recognize_file(path_str, locale, duration_ms, cancel_flag, active_task)?;

    Ok(RawTranscript { text, duration_ms })
}

/// If authorization is not yet determined, show the system prompt and wait;
/// any final non-authorized status returns a clear error.
fn ensure_authorized() -> Result<()> {
    let cls = speech_recognizer_class()?;

    // SFSpeechRecognizer.authorizationStatus (class method).
    // SAFETY: `cls` is the resolved `SFSpeechRecognizer` class object;
    // `authorizationStatus` is a no-argument class method returning NSInteger
    // (i64).
    let status: i64 = unsafe { msg_send![cls, authorizationStatus] };
    if status == SF_AUTH_AUTHORIZED {
        return Ok(());
    }
    if status == SF_AUTH_DENIED {
        bail!("语音识别权限被拒绝，请在 系统设置 → 隐私与安全性 → 语音识别 中允许 OpenLess");
    }
    if status == SF_AUTH_RESTRICTED {
        bail!("此设备的语音识别功能受限（可能由家长控制或 MDM 策略禁用）");
    }
    if status != SF_AUTH_NOT_DETERMINED {
        bail!("语音识别授权状态未知: {status}");
    }

    // NotDetermined: show the system authorization prompt and wait
    // synchronously for the callback, using the same block pattern as
    // permissions.rs.
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |granted_status: i64| {
        let _ = tx.send(granted_status);
    });
    log::info!("[apple-speech] requesting SFSpeechRecognizer authorization");
    // SAFETY: `requestAuthorization:` takes a
    // `void(^)(SFSpeechRecognizerAuthorizationStatus)` block whose callback
    // argument is NSInteger (i64). `&*block` is a stable block2 pointer and
    // the block is owned by `block` until end of scope — the callback fires
    // after the user answers the system prompt, before `rx.recv_timeout`
    // returns, so the block outlives the callback.
    let _: () = unsafe { msg_send![cls, requestAuthorization: &*block] };

    let granted = match rx.recv_timeout(AUTHORIZATION_WAIT) {
        Ok(s) => s,
        Err(err) => bail!("等待语音识别授权超时或失败: {err}"),
    };
    match granted {
        SF_AUTH_AUTHORIZED => Ok(()),
        SF_AUTH_DENIED => {
            bail!("语音识别权限被拒绝，请在 系统设置 → 隐私与安全性 → 语音识别 中允许 OpenLess")
        }
        SF_AUTH_RESTRICTED => bail!("此设备的语音识别功能受限"),
        other => bail!("语音识别未获授权（状态 {other}）"),
    }
}

/// Run one batch recognition of the given wav file via
/// `SFSpeechURLRecognitionRequest`, synchronizing the async
/// `recognitionTaskWithRequest:resultHandler:` callbacks.
///
/// Multi-utterance accumulation: on-device recognition splits audio at pauses
/// into utterances, each reported separately with its text reset (see the
/// `SegmentAccumulator` docs). So the loop cannot stop at the first isFinal —
/// the resultHandler only feeds each callback into `SegmentAccumulator`, and
/// the wait loop decides real completion via `task.state == completed` (plus
/// an isFinal-then-silence fallback) before joining all segments.
///
/// Waiting polls every `RECOGNITION_POLL`: each round checks `cancel_flag`,
/// and on set cancels the underlying task and returns a "cancelled" error so
/// threads abandoned by the outer dynamic timeout or by `cancel()` exit
/// within ~100ms. `active_task` is cleared on every exit path (an RAII guard
/// covers `?` early returns).
fn recognize_file(
    wav_path: &str,
    locale: Option<&str>,
    duration_ms: u64,
    cancel_flag: &AtomicBool,
    active_task: &Mutex<Option<SendableTask>>,
) -> Result<String> {
    let recognizer = create_recognizer(locale)?;

    // Engine availability wait (isAvailable race): a just-initialized
    // SFSpeechRecognizer often reports isAvailable == false briefly while
    // language resources load asynchronously. Previously any false bailed out
    // — the main cause of intermittent failures. Now poll for a few seconds
    // before deciding.
    wait_until_available(recognizer)?;

    let url = file_url(wav_path)?;
    let request = create_url_request(url)?;

    // Prefer on-device: force on-device recognition when the device supports
    // it for the current language (audio stays local for privacy, works
    // offline, immune to network issues). Unsupported languages fall back to
    // the system default (possibly networked) so recognition still works.
    configure_on_device(recognizer, request);

    // Enable partial results explicitly: utterance boundary signals (results
    // with non-null speechRecognitionMetadata) arrive in non-final callbacks;
    // without partials there are no boundaries to accumulate on.
    // SAFETY: `request` is an SFSpeechURLRecognitionRequest (the BOOL setter
    // comes from the parent class).
    let _: () = unsafe { msg_send![request, setShouldReportPartialResults: Bool::new(true)] };

    let shared = Arc::new(Mutex::new(RecognitionShared::default()));
    let shared_cb = Arc::clone(&shared);
    // resultHandler: void(^)(SFSpeechRecognitionResult *result, NSError *error).
    // The callback only unwraps and feeds the accumulator; completion is
    // decided entirely by the wait loop below.
    let block = RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
        let (recognized, callback_error) = extract_callback(result, error);
        let mut s = shared_cb.lock();
        s.record_callback(recognized, callback_error, Instant::now());
    });

    log::info!("[apple-speech] starting recognitionTaskWithRequest");
    // SAFETY: `recognizer` is valid; `request` is a valid
    // `SFSpeechURLRecognitionRequest`; `&*block` is a stable block pointer and
    // the block is owned by `block` until end of scope. The returned
    // `SFSpeechRecognitionTask` is strongly referenced by the recognizer until
    // completion; we additionally store the handle in `active_task` so
    // `cancel()` can terminate it from another thread (see the SendableTask
    // docs).
    let task: *mut AnyObject = unsafe {
        msg_send![
            recognizer,
            recognitionTaskWithRequest: request,
            resultHandler: &*block
        ]
    };

    // Store the handle for cancel(); the guard clears it back to None on every
    // exit path to avoid a dangling handle.
    *active_task.lock() = Some(SendableTask(task));
    let _task_guard = ActiveTaskGuard(active_task);

    // Polling wait: each round checks cancel_flag first, then error /
    // termination conditions; times out past the budget scaled to audio
    // length (the outer coordinator's dynamic timeout usually fires first —
    // this only guards against lost callbacks).
    let deadline = Instant::now() + recognition_wait_budget(duration_ms);
    loop {
        let now = Instant::now();
        let mut s = shared.lock();
        let decision = s.lifecycle.decide(
            now,
            cancel_flag.load(Ordering::SeqCst),
            s.error.is_some(),
            now >= deadline,
        );
        match decision {
            RecognitionDecision::Cancel => {
                drop(s);
                // If cancel() has not taken the handle yet (e.g. a timeout
                // path only set the flag), send cancel here so the underlying
                // task is actually terminated instead of running to
                // completion.
                if let Some(t) = active_task.lock().take() {
                    // SAFETY: see the SendableTask docs — `cancel` takes no
                    // arguments, is callable cross-thread, and is only invoked,
                    // never dereferenced.
                    let _: () = unsafe { msg_send![t.0, cancel] };
                }
                bail!("语音识别已取消");
            }
            RecognitionDecision::Error => {
                let Some(err) = s.error.take() else {
                    bail!("语音识别失败：终止状态缺少错误详情");
                };
                // When result and error arrive together, record_callback
                // already folded the result; salvaging here settles it exactly
                // once and never replays a committed final.
                let salvaged = s.acc.salvage();
                if salvaged.is_empty() {
                    bail!("语音识别失败: {err}");
                }
                log::warn!(
                    "[apple-speech] recognition error after {} segment(s); returning salvaged text: {err}",
                    s.acc.segment_count()
                );
                return Ok(salvaged);
            }
            RecognitionDecision::Finish => {
                let text = s.acc.salvage();
                log::info!(
                    "[apple-speech] recognition finished: {} segment(s), {} chars",
                    s.acc.segment_count(),
                    text.chars().count()
                );
                return Ok(text);
            }
            RecognitionDecision::Timeout => bail!("等待语音识别结果超时"),
            RecognitionDecision::Wait => drop(s),
        }

        std::thread::sleep(RECOGNITION_POLL);
        // SAFETY: `task` stays alive, strongly referenced by the recognizer
        // within this stack frame (see above); `state` is a no-argument
        // read-only property returning NSInteger (i64). Reading an integer
        // property across threads at worst yields a momentarily stale value,
        // caught up on the next round (~100ms), without affecting correctness.
        let state: i64 = unsafe { msg_send![task, state] };
        if state == SF_TASK_STATE_COMPLETED {
            shared.lock().lifecycle.record_completed(Instant::now());
        }
    }
}

/// Recognition wait budget: audio duration + 30s, at least `RECOGNITION_WAIT`.
/// Batch recognition is usually far faster than real time, but long recordings
/// (multiple segments, results per segment) must not be cut off by the fixed
/// 60s cap. The outer coordinator's dynamic timeout still fires first.
fn recognition_wait_budget(duration_ms: u64) -> Duration {
    RECOGNITION_WAIT.max(Duration::from_millis(duration_ms).saturating_add(Duration::from_secs(30)))
}

/// Ensures `active_task` is reset to `None` on every exit path of
/// `recognize_file` (including `?` early returns, normal return, cancel and
/// timeout) so a dangling task handle cannot be misused by a later `cancel()`.
struct ActiveTaskGuard<'a>(&'a Mutex<Option<SendableTask>>);

impl Drop for ActiveTaskGuard<'_> {
    fn drop(&mut self) {
        *self.0.lock() = None;
    }
}

/// Poll until the recognition engine is available. isAvailable can be briefly
/// false right after init (resources loading asynchronously); error with
/// guidance only after AVAILABILITY_WAIT elapses with no availability.
fn wait_until_available(recognizer: *mut AnyObject) -> Result<()> {
    let deadline = std::time::Instant::now() + AVAILABILITY_WAIT;
    loop {
        // SAFETY: `recognizer` is valid; `isAvailable` is a no-argument call
        // returning BOOL.
        let available: Bool = unsafe { msg_send![recognizer, isAvailable] };
        if available.as_bool() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "当前语言的语音识别暂不可用：系统可能仍在准备识别资源，或需在 系统设置 → 键盘 → 听写 中下载对应语言。可稍后重试，或改用其它 ASR。"
            );
        }
        std::thread::sleep(AVAILABILITY_POLL);
    }
}

/// Force on-device recognition for languages that support it (audio stays
/// local, works offline); leave others unset so they fall back to the system
/// default (possibly networked) and still work.
fn configure_on_device(recognizer: *mut AnyObject, request: *mut AnyObject) {
    // SFSpeechRecognizer.supportsOnDeviceRecognition (macOS 10.15+, BOOL
    // property).
    // SAFETY: `recognizer` is valid; no-argument call returning BOOL.
    let supports: Bool = unsafe { msg_send![recognizer, supportsOnDeviceRecognition] };
    if supports.as_bool() {
        // SFSpeechRecognitionRequest.requiresOnDeviceRecognition = YES.
        // SAFETY: `request` is an SFSpeechURLRecognitionRequest (the setter is
        // provided by the parent class SFSpeechRecognitionRequest); BOOL
        // argument.
        let _: () = unsafe { msg_send![request, setRequiresOnDeviceRecognition: Bool::new(true)] };
        log::info!("[apple-speech] on-device recognition enabled");
    } else {
        log::info!(
            "[apple-speech] on-device unsupported for current locale; using default (may use network)"
        );
    }
}

/// State shared between the resultHandler callback and the wait loop (written
/// by the block side, read by the polling side).
#[derive(Default)]
struct RecognitionShared {
    acc: SegmentAccumulator,
    lifecycle: RecognitionLifecycle,
    /// First recognition error (the first is kept, later ones ignored).
    error: Option<String>,
}

struct RecognizedCallback {
    text: String,
    /// This result carries non-null `speechRecognitionMetadata` — an
    /// utterance ends here and `text` is that utterance's full text.
    utterance_ended: bool,
    is_final: bool,
}

impl RecognitionShared {
    fn record_callback(
        &mut self,
        recognized: Option<RecognizedCallback>,
        error: Option<String>,
        at: Instant,
    ) {
        if let Some(result) = recognized {
            if result.utterance_ended {
                log::info!(
                    "[apple-speech] utterance boundary: segment captured ({} chars)",
                    result.text.chars().count()
                );
            }
            self.acc
                .fold(&result.text, result.utterance_ended, result.is_final);
            self.lifecycle.record_callback(at, result.is_final);
        }
        if self.error.is_none() {
            self.error = error;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecognitionDecision {
    Wait,
    Finish,
    Cancel,
    Error,
    Timeout,
}

#[derive(Default)]
struct RecognitionLifecycle {
    completed_at: Option<Instant>,
    last_callback_at: Option<Instant>,
    saw_final: bool,
}

impl RecognitionLifecycle {
    fn record_callback(&mut self, at: Instant, is_final: bool) {
        self.last_callback_at = Some(at);
        self.saw_final |= is_final;
    }

    fn record_completed(&mut self, at: Instant) {
        self.completed_at.get_or_insert(at);
    }

    fn decide(
        &self,
        now: Instant,
        cancelled: bool,
        has_error: bool,
        deadline_reached: bool,
    ) -> RecognitionDecision {
        if cancelled {
            return RecognitionDecision::Cancel;
        }
        if has_error {
            return RecognitionDecision::Error;
        }

        let completion_settled = self
            .completed_at
            .map(|at| now.saturating_duration_since(at) >= COMPLETION_GRACE)
            .unwrap_or(false)
            && self
                .last_callback_at
                .map(|at| now.saturating_duration_since(at) >= COMPLETION_GRACE)
                .unwrap_or(true);
        let final_quiesced = self.saw_final
            && self
                .last_callback_at
                .map(|at| now.saturating_duration_since(at) >= FINAL_QUIESCENCE)
                .unwrap_or(false);
        if completion_settled || final_quiesced {
            return RecognitionDecision::Finish;
        }
        if deadline_reached {
            return RecognitionDecision::Timeout;
        }
        RecognitionDecision::Wait
    }
}

/// Unwrap both the recognition result and the error from `(result, error)`.
/// Apple allows both in one callback; callers must fold the result before
/// recording the error so error salvage includes this final text exactly once.
fn extract_callback(
    result: *mut AnyObject,
    error: *mut AnyObject,
) -> (Option<RecognizedCallback>, Option<String>) {
    let callback_error = if !error.is_null() {
        Some(ns_error_description(error))
    } else if result.is_null() {
        Some("识别返回空结果".to_string())
    } else {
        None
    };
    if result.is_null() {
        return (None, callback_error);
    }
    // SAFETY: `result` is non-null and an `SFSpeechRecognitionResult`;
    // `isFinal` is a no-argument call returning BOOL.
    let is_final: Bool = unsafe { msg_send![result, isFinal] };
    // Non-null speechRecognitionMetadata = an utterance ends (macOS 11.3+).
    // Older systems lack the selector; probe with respondsToSelector first to
    // avoid crashing on an unknown selector.
    // SAFETY: `respondsToSelector:` is an NSObject protocol method taking a Sel
    // and returning BOOL.
    let has_metadata_sel: Bool =
        unsafe { msg_send![result, respondsToSelector: sel!(speechRecognitionMetadata)] };
    let utterance_ended = if has_metadata_sel.as_bool() {
        // SAFETY: the selector's existence was confirmed above; no-argument
        // call returning an object pointer (possibly nil).
        let metadata: *mut AnyObject = unsafe { msg_send![result, speechRecognitionMetadata] };
        !metadata.is_null()
    } else {
        false
    };
    // result.bestTranscription.formattedString → NSString → Rust String.
    // SAFETY: `result` is non-null; `bestTranscription` returns SFTranscription
    // (possibly nil), `formattedString` returns NSString.
    let transcription: *mut AnyObject = unsafe { msg_send![result, bestTranscription] };
    let text = if transcription.is_null() {
        String::new()
    } else {
        let formatted: *mut AnyObject = unsafe { msg_send![transcription, formattedString] };
        ns_string_to_rust(formatted)
    };
    let recognized = RecognizedCallback {
        text,
        utterance_ended,
        is_final: is_final.as_bool(),
    };
    (Some(recognized), callback_error)
}

/// Accumulates recognized text across utterances (fixes loss of pre-pause
/// text; issue: Apple Speech truncation at pauses).
///
/// Apple on-device recognition (`requiresOnDeviceRecognition`) splits audio at
/// pauses into utterances: each utterance end reports one result carrying
/// `speechRecognitionMetadata` whose text covers only that utterance, then
/// partial text restarts from empty; `isFinal` usually appears only on the
/// last utterance (some system versions emit isFinal per utterance). Keeping
/// only the first isFinal's text discards every earlier utterance — the root
/// cause of "pausing mid-speech loses everything said before". Each utterance
/// is stored here and the segments are joined with CJK rules when recognition
/// ends.
///
/// Server-side recognition has no utterance resets: partials accumulate
/// throughout and the final is the full text, so `segments` receives a single
/// final full-text entry (or one merged via prefix replacement) — the same
/// behavior as before.
#[derive(Default)]
struct SegmentAccumulator {
    /// Texts of finished utterances, in order.
    segments: Vec<String>,
    /// Latest partial text of the current utterance.
    current: String,
    /// Whether a new-generation partial was seen since the last explicit
    /// boundary commit. Distinguishes "next utterance" from the same task
    /// replaying final full text at the end, without guessing identity from
    /// cross-utterance text prefixes.
    current_generation_active: bool,
    /// Cumulative full text the task may replay at completion; only an
    /// explicit boundary may create this candidate. Identical-text isFinal-only
    /// sequences must still count as distinct utterances.
    cumulative_replay_candidate: Option<String>,
}

impl SegmentAccumulator {
    /// Feed one recognition callback. Text with `utterance_ended` / `is_final`
    /// is treated as the utterance's complete text and committed; a plain
    /// partial only updates `current` unless a silent reset is detected.
    fn fold(&mut self, text: &str, utterance_ended: bool, is_final: bool) {
        if utterance_ended {
            // metadata is Apple's explicit utterance evidence; adjacent texts
            // that are equal or prefixes of each other must still be committed
            // separately, so normal repetition/self-correction is not swallowed
            // as a cumulative replay.
            let segment = if text.trim().is_empty() {
                std::mem::take(&mut self.current)
            } else {
                text.to_string()
            };
            self.push_segment(&segment);
            self.current.clear();
            self.current_generation_active = false;
            self.cumulative_replay_candidate = Some(normalized(&self.joined()));
        } else if is_final {
            let segment = if text.trim().is_empty() {
                std::mem::take(&mut self.current)
            } else {
                text.to_string()
            };
            // Only a snapshot created by a metadata boundary proves this is
            // the same task replaying cumulative full text; deduplicating on
            // "final text == joined" alone would drop consecutive identical
            // final-only utterances.
            let normalized_segment = normalized(&segment);
            let is_cumulative_replay = !self.current_generation_active
                && self.cumulative_replay_candidate.as_deref() == Some(normalized_segment.as_str());
            if !is_cumulative_replay {
                self.push_segment(&segment);
                self.cumulative_replay_candidate = None;
            }
            self.current.clear();
            self.current_generation_active = false;
        } else if self.reset_detected(text) {
            // Defensive path: no metadata boundary callback but the partial
            // shrank sharply — on-device recognition silently started a new
            // utterance. Commit the longest partial seen for the previous
            // utterance, then accumulate from the new text.
            let previous = std::mem::take(&mut self.current);
            self.push_segment(&previous);
            self.current = text.to_string();
            self.current_generation_active = true;
            self.cumulative_replay_candidate = None;
        } else {
            self.current = text.to_string();
            self.current_generation_active = true;
            self.cumulative_replay_candidate = None;
        }
    }

    /// Treats a sharp partial shrink as an utterance reset. Conservative
    /// threshold (previous text >= 12 chars and new text below 1/3 of it):
    /// normal hypothesis revisions only make small edits.
    fn reset_detected(&self, text: &str) -> bool {
        let current_chars = self.current.chars().count();
        let new_chars = text.chars().count();
        current_chars >= 12 && new_chars.saturating_mul(3) < current_chars
    }

    /// Commits an explicit utterance. The caller first determines commit
    /// identity from metadata / generation / final state; no cross-utterance
    /// text heuristics here, so normal repetition and prefix-style
    /// self-correction are not swallowed.
    fn push_segment(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        self.segments.push(trimmed.to_string());
    }

    /// Final settlement: commit any residual partial, then return the joined
    /// text of all utterances.
    fn salvage(&mut self) -> String {
        if self.current_generation_active {
            let current = std::mem::take(&mut self.current);
            self.push_segment(&current);
            self.current_generation_active = false;
        }
        self.joined()
    }

    fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Utterance joining: Han, hiragana, katakana and CJK punctuation connect
    /// without spaces; other scripts (Korean, Cyrillic, Arabic, ...) get word
    /// spaces. The LLM still normalizes in polish mode.
    fn joined(&self) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            if out.is_empty() {
                out.push_str(segment);
                continue;
            }
            let join_bare = matches!(
                (out.chars().last(), segment.chars().next()),
                (Some(prev), Some(next)) if should_join_without_space(prev, next)
            );
            if !join_bare {
                out.push(' ');
            }
            out.push_str(segment);
        }
        out
    }
}

fn should_join_without_space(prev: char, next: char) -> bool {
    (is_han_or_japanese(prev) && is_han_or_japanese(next))
        || (is_cjk_punctuation(prev) && is_han_or_japanese(next))
        || (is_han_or_japanese(prev) && is_cjk_punctuation(next))
        || is_opening_punctuation(prev)
        || is_closing_punctuation(next)
}

fn is_han_or_japanese(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
            | 0x3040..=0x30FF
            | 0x31F0..=0x31FF
            | 0xFF66..=0xFF9D
    )
}

fn is_cjk_punctuation(c: char) -> bool {
    matches!(
        c,
        '、' | '。'
            | '，'
            | '！'
            | '？'
            | '：'
            | '；'
            | '「'
            | '」'
            | '『'
            | '』'
            | '【'
            | '】'
            | '《'
            | '》'
            | '〈'
            | '〉'
            | '・'
            | '〜'
            | '…'
            | '—'
    )
}

fn is_opening_punctuation(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '（' | '［' | '｛' | '「' | '『' | '【' | '《' | '〈'
    )
}

fn is_closing_punctuation(c: char) -> bool {
    matches!(
        c,
        ',' | '.'
            | '!'
            | '?'
            | ':'
            | ';'
            | ')'
            | ']'
            | '}'
            | '，'
            | '。'
            | '！'
            | '？'
            | '：'
            | '；'
            | '）'
            | '］'
            | '｝'
            | '、'
            | '」'
            | '』'
            | '】'
            | '》'
            | '〉'
    )
}

/// Whitespace-insensitive comparison: strips all whitespace. Utterance joins
/// and engine full-text replays may use different separators, so compare
/// content only.
fn normalized(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn speech_recognizer_class() -> Result<&'static AnyClass> {
    AnyClass::get("SFSpeechRecognizer").ok_or_else(|| {
        anyhow!("SFSpeechRecognizer 类不可用（需要 macOS 10.15+ 并链接 Speech.framework）")
    })
}

/// When a working language is given, its locale must be used; return an error
/// on failure instead of silently recognizing with the system default language.
fn create_recognizer(locale: Option<&str>) -> Result<*mut AnyObject> {
    let cls = speech_recognizer_class()?;
    let requested_locale = locale
        .map(|locale| {
            ns_locale(locale).ok_or_else(|| anyhow!("Apple Speech 无法使用所选语言 {locale}"))
        })
        .transpose()?;
    let recognizer: *mut AnyObject = match requested_locale {
        Some(ns_loc) => {
            log::info!(
                "[apple-speech] recognizer locale = {}",
                locale.unwrap_or("")
            );
            // SAFETY: `cls` is the SFSpeechRecognizer class; `alloc` yields an
            // uninitialized instance, `initWithLocale:` initializes it with a
            // valid NSLocale, and the returned instance is handed to the
            // caller (ARC-managed).
            unsafe {
                let alloc: *mut AnyObject = msg_send![cls, alloc];
                msg_send![alloc, initWithLocale: ns_loc]
            }
        }
        None => {
            // SAFETY: as above; `init` uses the system default locale.
            unsafe {
                let alloc: *mut AnyObject = msg_send![cls, alloc];
                msg_send![alloc, init]
            }
        }
    };
    if recognizer.is_null() {
        bail!("无法创建 SFSpeechRecognizer（当前语言可能不支持语音识别）");
    }
    Ok(recognizer)
}

/// `[NSLocale localeWithLocaleIdentifier:<id>]`. Returns None on failure so
/// the caller reports the selected language as unavailable.
fn ns_locale(identifier: &str) -> Option<*mut AnyObject> {
    let ns_id = ns_string_from_str(identifier).ok()?;
    let cls = AnyClass::get("NSLocale")?;
    // SAFETY: `cls` is NSLocale; `localeWithLocaleIdentifier:` takes an
    // NSString (`ns_id` is valid) and returns an autoreleased NSLocale (alive
    // in the spawn_blocking thread's autorelease pool).
    let loc: *mut AnyObject = unsafe { msg_send![cls, localeWithLocaleIdentifier: ns_id] };
    if loc.is_null() {
        None
    } else {
        Some(loc)
    }
}

/// Maps the preferred working language to an Apple locale; the caller must
/// still check system availability. Unlisted languages return None so the
/// caller reports them as unsupported instead of switching to the system
/// default.
pub fn native_name_to_apple_locale(native_name: &str) -> Option<String> {
    openless_core::language_catalog::apple_speech_locale(native_name)
}

/// `[NSURL fileURLWithPath:<path>]`.
fn file_url(path: &str) -> Result<*mut AnyObject> {
    let ns_path = ns_string_from_str(path)?;
    let cls = AnyClass::get("NSURL").ok_or_else(|| anyhow!("NSURL 类不可用"))?;
    // SAFETY: `cls` is NSURL; `fileURLWithPath:` takes an NSString (`ns_path`
    // is valid) and returns an autoreleased NSURL (alive in the spawn_blocking
    // thread's implicit autorelease pool).
    let url: *mut AnyObject = unsafe { msg_send![cls, fileURLWithPath: ns_path] };
    if url.is_null() {
        bail!("构造文件 URL 失败: {path}");
    }
    Ok(url)
}

/// `[[SFSpeechURLRecognitionRequest alloc] initWithURL:<url>]`.
fn create_url_request(url: *mut AnyObject) -> Result<*mut AnyObject> {
    let cls = AnyClass::get("SFSpeechURLRecognitionRequest")
        .ok_or_else(|| anyhow!("SFSpeechURLRecognitionRequest 类不可用"))?;
    // SAFETY: `cls` is the request class; `alloc`+`initWithURL:` initializes
    // the request instance with the valid `url`.
    let request: *mut AnyObject = unsafe {
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        msg_send![alloc, initWithURL: url]
    };
    if request.is_null() {
        bail!("构造 SFSpeechURLRecognitionRequest 失败");
    }
    Ok(request)
}

/// `[NSString stringWithUTF8String:<bytes>]`. `s` must not contain an interior
/// NUL.
fn ns_string_from_str(s: &str) -> Result<*mut AnyObject> {
    let c = std::ffi::CString::new(s).context("字符串含 NUL，无法构造 NSString")?;
    let cls = AnyClass::get("NSString").ok_or_else(|| anyhow!("NSString 类不可用"))?;
    // SAFETY: `cls` is NSString; `stringWithUTF8String:` takes a
    // NUL-terminated C string (`c.as_ptr()` is valid while `c` lives, the call
    // completes synchronously, and NSString copies the contents).
    let ns: *mut AnyObject = unsafe { msg_send![cls, stringWithUTF8String: c.as_ptr()] };
    if ns.is_null() {
        bail!("stringWithUTF8String 返回 nil");
    }
    Ok(ns)
}

/// NSString → Rust String (via `UTF8String`). Returns an empty string for nil.
fn ns_string_to_rust(ns: *mut AnyObject) -> String {
    if ns.is_null() {
        return String::new();
    }
    // SAFETY: `ns` is non-null and an NSString; `UTF8String` returns a pointer
    // into the NSString's internal NUL-terminated UTF-8 buffer, valid while
    // the autorelease pool lives. Copied into an owned String immediately.
    let ptr: *const std::os::raw::c_char = unsafe { msg_send![ns, UTF8String] };
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: `ptr` is a valid NUL-terminated C string (from
    // NSString.UTF8String).
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// NSError → human-readable string (`localizedDescription`).
fn ns_error_description(error: *mut AnyObject) -> String {
    if error.is_null() {
        return "未知错误".to_string();
    }
    // SAFETY: `error` is non-null and an NSError; `localizedDescription`
    // returns NSString.
    let desc: *mut AnyObject = unsafe { msg_send![error, localizedDescription] };
    let message = ns_string_to_rust(desc);
    if message.is_empty() {
        "未知错误".to_string()
    } else {
        message
    }
}

/// Process-local monotonic suffix, avoiding concurrent temp wav filename
/// collisions within the process.
fn unique_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// RAII temp-file cleanup: deletes the wav when transcribe returns (success
/// or failure).
struct TempFileGuard<'a>(&'a std::path::Path);

impl Drop for TempFileGuard<'_> {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_file(self.0) {
            log::warn!(
                "[apple-speech] 删除临时 wav 失败 {}: {err}",
                self.0.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder::AudioConsumer;

    #[test]
    fn buffer_duration_tracks_consumed_pcm() {
        let asr = AppleSpeechAsr::new(None);
        assert_eq!(asr.buffer_duration_ms(), 0);
        // 16k * 2 bytes/sample * 1s = 32000 bytes.
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        assert_eq!(asr.buffer_duration_ms(), 1_000);
        asr.consume_pcm_chunk(&vec![0u8; 16_000]);
        assert_eq!(asr.buffer_duration_ms(), 1_500);
    }

    #[test]
    fn cancel_clears_buffer() {
        let asr = AppleSpeechAsr::new(None);
        asr.consume_pcm_chunk(&vec![0u8; 32_000]);
        asr.cancel();
        assert_eq!(asr.buffer_duration_ms(), 0);
    }

    #[tokio::test]
    async fn transcribe_empty_buffer_returns_empty() {
        let asr = AppleSpeechAsr::new(None);
        let transcript = asr.transcribe().await.unwrap();
        assert_eq!(transcript.text, "");
        assert_eq!(transcript.duration_ms, 0);
    }

    #[test]
    fn temp_file_guard_removes_file_on_drop() {
        let path = std::env::temp_dir().join(format!(
            "openless-apple-speech-test-{}.wav",
            unique_suffix()
        ));
        std::fs::write(&path, b"x").unwrap();
        assert!(path.exists());
        {
            let _guard = TempFileGuard(&path);
        }
        assert!(!path.exists());
    }

    #[test]
    fn unique_suffix_is_monotonic() {
        let a = unique_suffix();
        let b = unique_suffix();
        assert!(b > a);
    }

    #[test]
    fn cancel_flag_defaults_false_and_set_by_cancel() {
        let asr = AppleSpeechAsr::new(None);
        assert!(
            !asr.cancel_flag.load(Ordering::SeqCst),
            "取消标志初值应为 false"
        );
        asr.cancel();
        assert!(
            asr.cancel_flag.load(Ordering::SeqCst),
            "cancel() 应把取消标志置位，让等待轮询下一轮退出"
        );
    }

    #[test]
    fn active_task_defaults_none() {
        let asr = AppleSpeechAsr::new(None);
        assert!(
            asr.active_task.lock().is_none(),
            "尚未发起识别时 active_task 应为 None"
        );
    }

    #[test]
    fn active_task_guard_clears_handle_on_drop() {
        // After active_task holds a handle, ActiveTaskGuard dropping out of
        // scope must reset it to None. The dangling pointer is a placeholder
        // only: the guard's Drop just takes and sets None, never touching the
        // pointer's contents.
        let slot: Mutex<Option<SendableTask>> = Mutex::new(None);
        *slot.lock() = Some(SendableTask(std::ptr::null_mut()));
        assert!(slot.lock().is_some());
        {
            let _guard = ActiveTaskGuard(&slot);
        }
        assert!(
            slot.lock().is_none(),
            "ActiveTaskGuard drop 后 active_task 必须清空，避免悬挂句柄"
        );
    }

    #[test]
    fn cancel_on_empty_active_task_is_noop_and_sets_flag() {
        // With active_task None, cancel() must issue no objc calls, only set
        // the flag and clear the buffer.
        let asr = AppleSpeechAsr::new(None);
        assert!(asr.active_task.lock().is_none());
        asr.cancel(); // must not panic
        assert!(asr.cancel_flag.load(Ordering::SeqCst));
        assert!(asr.active_task.lock().is_none());
    }

    #[tokio::test]
    async fn transcribe_empty_buffer_short_circuits_before_flag_reset() {
        // An empty buffer early-returns before the cancel flag is reset, so
        // recognition logic is not entered and the flag keeps its value. Pins
        // the short-circuit order: reset happens only when recognition
        // actually runs (non-empty buffer).
        let asr = AppleSpeechAsr::new(None);
        asr.cancel_flag.store(true, Ordering::SeqCst);
        let out = asr.transcribe().await.unwrap();
        assert_eq!(out.text, "");
        assert!(asr.cancel_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn sendable_task_is_send() {
        // Compile-time assertion: SendableTask must be Send so spawn_blocking
        // can capture it across threads.
        fn assert_send<T: Send>() {}
        assert_send::<SendableTask>();
        assert_send::<Arc<Mutex<Option<SendableTask>>>>();
    }

    // ---- SegmentAccumulator: multi-utterance accumulation across pauses ----

    #[test]
    fn server_style_growing_partials_keep_full_final() {
        // Server-side recognition: partials accumulate throughout, final is
        // full text — behavior must match the old implementation.
        let mut acc = SegmentAccumulator::default();
        acc.fold("hello", false, false);
        acc.fold("hello there", false, false);
        acc.fold("hello there how are you", false, true);
        assert_eq!(acc.salvage(), "hello there how are you");
    }

    #[test]
    fn on_device_pause_segments_are_all_kept() {
        // User bug reproduction: a pause creates an utterance boundary
        // (metadata); the old implementation kept only the last segment.
        let mut acc = SegmentAccumulator::default();
        acc.fold("今天天气", false, false);
        acc.fold("今天天气很好", true, false); // pause -> utterance 1 ends
        acc.fold("我们", false, false); // partials restart from empty
        acc.fold("我们去公园", false, true); // last utterance ends with isFinal
        assert_eq!(acc.salvage(), "今天天气很好我们去公园");
    }

    #[test]
    fn per_segment_finals_are_all_kept() {
        // Some systems emit isFinal per utterance: every final must be
        // stored, not just the first.
        let mut acc = SegmentAccumulator::default();
        acc.fold("第一段内容", false, true);
        acc.fold("第二段内容", false, true);
        assert_eq!(acc.salvage(), "第一段内容第二段内容");
    }

    #[test]
    fn repeated_per_segment_finals_are_distinct_utterances() {
        // Two adjacent utterances can have identical content; the second
        // final must not be swallowed as a task-level full-text replay.
        let mut acc = SegmentAccumulator::default();
        acc.fold("hello", false, true);
        acc.fold("hello", false, true);
        assert_eq!(acc.salvage(), "hello hello");
    }

    #[test]
    fn silent_reset_without_metadata_is_salvaged() {
        // Defensive path: no metadata boundary and a sharp partial shrink ->
        // commit the previous utterance first.
        let mut acc = SegmentAccumulator::default();
        acc.fold("这是停顿之前说的很长一段话啊", false, false); // 14 chars
        acc.fold("后", false, false); // sharp shrink -> reset detected
        acc.fold("后半段", false, true);
        assert_eq!(acc.salvage(), "这是停顿之前说的很长一段话啊后半段");
    }

    #[test]
    fn small_revision_is_not_treated_as_reset() {
        // Normal hypothesis revision (small shrink) must not trigger a reset,
        // otherwise duplicates are manufactured.
        let mut acc = SegmentAccumulator::default();
        acc.fold("hello there my friend", false, false);
        acc.fold("hello there my frien", false, false); // shrinks by only 1 char
        acc.fold("hello there my friends", false, true);
        assert_eq!(acc.salvage(), "hello there my friends");
    }

    #[test]
    fn equal_boundary_segments_are_distinct_utterances() {
        let mut acc = SegmentAccumulator::default();
        acc.fold("hello", true, false);
        acc.fold("hello", true, false);
        assert_eq!(acc.salvage(), "hello hello");
    }

    #[test]
    fn longer_prefix_boundary_segment_is_not_a_cumulative_replay() {
        let mut acc = SegmentAccumulator::default();
        acc.fold("好的", true, false);
        acc.fold("好的我们继续", true, false);
        assert_eq!(acc.salvage(), "好的好的我们继续");
    }

    #[test]
    fn shorter_prefix_boundary_segment_is_not_a_cumulative_replay() {
        let mut acc = SegmentAccumulator::default();
        acc.fold("好的我们继续", true, false);
        acc.fold("好的", true, false);
        assert_eq!(acc.salvage(), "好的我们继续好的");
    }

    #[test]
    fn full_text_replay_at_final_is_not_duplicated() {
        // Defensive: after committing utterances one by one, a final
        // replaying the cumulative full text (possibly with different
        // separators) must be ignored via whitespace-insensitive dedup, not
        // appended a second time.
        let mut acc = SegmentAccumulator::default();
        acc.fold("今天天气很好", true, false);
        acc.fold("我们去公园", true, false);
        acc.fold("今天天气很好 我们去公园", false, true);
        assert_eq!(acc.salvage(), "今天天气很好我们去公园");
    }

    #[test]
    fn empty_boundary_text_falls_back_to_partial() {
        // Boundary results occasionally arrive empty: fall back to the longest
        // partial seen for the current utterance so no content is lost.
        let mut acc = SegmentAccumulator::default();
        acc.fold("前半句", false, false);
        acc.fold("", true, false);
        acc.fold("后半句", false, true);
        assert_eq!(acc.salvage(), "前半句后半句");
    }

    #[test]
    fn salvage_includes_residual_partial() {
        // Error fallback: even without a final, salvage the partials seen so
        // far.
        let mut acc = SegmentAccumulator::default();
        acc.fold("说到一半", false, false);
        assert_eq!(acc.salvage(), "说到一半");
    }

    #[test]
    fn ascii_segments_join_with_space_cjk_join_bare() {
        let mut acc = SegmentAccumulator::default();
        acc.fold("first part", true, false);
        acc.fold("second part", true, false);
        assert_eq!(acc.salvage(), "first part second part");

        let mut mixed = SegmentAccumulator::default();
        mixed.fold("中文段落", true, false);
        mixed.fold("english tail", true, false);
        assert_eq!(mixed.salvage(), "中文段落 english tail");
    }

    #[test]
    fn non_cjk_non_ascii_segments_keep_word_spaces() {
        for (first, second, expected) in [
            ("привет", "мир", "привет мир"),
            ("مرحبا", "بالعالم", "مرحبا بالعالم"),
            ("안녕", "하세요", "안녕 하세요"),
        ] {
            let mut acc = SegmentAccumulator::default();
            acc.fold(first, true, false);
            acc.fold(second, true, false);
            assert_eq!(acc.salvage(), expected);
        }
    }

    #[test]
    fn chinese_and_japanese_scripts_join_without_spaces_around_native_punctuation() {
        let mut chinese = SegmentAccumulator::default();
        chinese.fold("你好，", true, false);
        chinese.fold("我们继续", true, false);
        assert_eq!(chinese.salvage(), "你好，我们继续");

        let mut japanese = SegmentAccumulator::default();
        japanese.fold("今日は", true, false);
        japanese.fold("晴れです。", true, false);
        assert_eq!(japanese.salvage(), "今日は晴れです。");
    }

    #[test]
    fn recognition_wait_budget_scales_with_audio_length() {
        // Short audio keeps the 60s floor; long audio scales to duration +
        // 30s, no longer cut off by a fixed cap.
        assert_eq!(recognition_wait_budget(5_000), RECOGNITION_WAIT);
        assert_eq!(recognition_wait_budget(300_000), Duration::from_secs(330));
    }

    // ---- RecognitionLifecycle: completion / late callbacks / quiescence and
    // termination priority ----

    #[test]
    fn completed_task_waits_for_the_full_grace_period() {
        let start = Instant::now();
        let mut lifecycle = RecognitionLifecycle::default();
        lifecycle.record_completed(start);

        assert_eq!(
            lifecycle.decide(
                start + COMPLETION_GRACE - Duration::from_millis(1),
                false,
                false,
                false
            ),
            RecognitionDecision::Wait
        );
        assert_eq!(
            lifecycle.decide(start + COMPLETION_GRACE, false, false, false),
            RecognitionDecision::Finish
        );
    }

    #[test]
    fn callback_after_completed_restarts_the_grace_window() {
        let start = Instant::now();
        let late = start + Duration::from_millis(200);
        let mut lifecycle = RecognitionLifecycle::default();
        lifecycle.record_completed(start);
        lifecycle.record_callback(late, false);

        assert_eq!(
            lifecycle.decide(
                late + COMPLETION_GRACE - Duration::from_millis(1),
                false,
                false,
                false
            ),
            RecognitionDecision::Wait
        );
        assert_eq!(
            lifecycle.decide(late + COMPLETION_GRACE, false, false, false),
            RecognitionDecision::Finish
        );
    }

    #[test]
    fn final_callback_without_completed_uses_silent_fallback() {
        let start = Instant::now();
        let mut lifecycle = RecognitionLifecycle::default();
        lifecycle.record_callback(start, true);

        assert_eq!(
            lifecycle.decide(
                start + FINAL_QUIESCENCE - Duration::from_millis(1),
                false,
                false,
                false
            ),
            RecognitionDecision::Wait
        );
        assert_eq!(
            lifecycle.decide(start + FINAL_QUIESCENCE, false, false, false),
            RecognitionDecision::Finish
        );
    }

    #[test]
    fn cancellation_wins_over_error_timeout_and_completion() {
        let start = Instant::now();
        let mut lifecycle = RecognitionLifecycle::default();
        lifecycle.record_completed(start);
        assert_eq!(
            lifecycle.decide(start + COMPLETION_GRACE, true, true, true),
            RecognitionDecision::Cancel
        );
    }

    #[test]
    fn error_wins_over_timeout_and_completion_when_not_cancelled() {
        let start = Instant::now();
        let mut lifecycle = RecognitionLifecycle::default();
        lifecycle.record_completed(start);
        assert_eq!(
            lifecycle.decide(start + COMPLETION_GRACE, false, true, true),
            RecognitionDecision::Error
        );
    }

    #[test]
    fn result_and_error_in_one_callback_salvages_the_result_once() {
        let start = Instant::now();
        let mut shared = RecognitionShared::default();
        shared.record_callback(
            Some(RecognizedCallback {
                text: "已经识别的内容".to_string(),
                utterance_ended: false,
                is_final: true,
            }),
            Some("尾部错误".to_string()),
            start,
        );

        assert_eq!(
            shared
                .lifecycle
                .decide(start, false, shared.error.is_some(), false),
            RecognitionDecision::Error
        );
        assert_eq!(shared.acc.salvage(), "已经识别的内容");
        assert_eq!(shared.acc.salvage(), "已经识别的内容");
    }

    #[test]
    fn settled_completion_wins_over_timeout_but_timeout_ends_plain_waiting() {
        let start = Instant::now();
        let mut completed = RecognitionLifecycle::default();
        completed.record_completed(start);
        assert_eq!(
            completed.decide(start + COMPLETION_GRACE, false, false, true),
            RecognitionDecision::Finish
        );

        assert_eq!(
            RecognitionLifecycle::default().decide(start, false, false, true),
            RecognitionDecision::Timeout
        );
    }
}
