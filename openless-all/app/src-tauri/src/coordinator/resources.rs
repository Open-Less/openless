use std::sync::Arc;

use crate::coordinator_state::{SessionId, SessionPhase};
use crate::recorder::Recorder;
use crate::types::CapsuleState;
use tauri::Manager;

use super::{emit_capsule, ActiveAsr, Inner};

pub(super) struct SessionResource<T> {
    pub(super) session_id: SessionId,
    resource: T,
}

impl<T> SessionResource<T> {
    pub(super) fn new(session_id: SessionId, resource: T) -> Self {
        Self {
            session_id,
            resource,
        }
    }

    fn into_inner(self) -> T {
        self.resource
    }
}

pub(super) struct SharedRecordingMuteState {
    guard: Option<crate::audio_mute::AudioMuteGuard>,
    holders: u32,
}

impl SharedRecordingMuteState {
    pub(super) fn new() -> Self {
        Self {
            guard: None,
            holders: 0,
        }
    }
}

pub(super) fn take_session_resource<T>(
    slot: &mut Option<SessionResource<T>>,
    session_id: SessionId,
) -> Option<T> {
    if slot
        .as_ref()
        .map(|resource| resource.session_id == session_id)
        .unwrap_or(false)
    {
        slot.take().map(SessionResource::into_inner)
    } else {
        None
    }
}

pub(super) fn store_asr_for_session(inner: &Arc<Inner>, session_id: SessionId, asr: ActiveAsr) {
    *inner.asr.lock() = Some(SessionResource::new(session_id, asr));
}

pub(super) fn take_asr_for_session(inner: &Arc<Inner>, session_id: SessionId) -> Option<ActiveAsr> {
    let mut slot = inner.asr.lock();
    take_session_resource(&mut slot, session_id)
}

pub(super) fn cancel_active_asr(asr: ActiveAsr) {
    match asr {
        ActiveAsr::Volcengine(v) => v.cancel(),
        ActiveAsr::Whisper(w) => w.cancel(),
        ActiveAsr::Mimo(m) => m.cancel(),
        ActiveAsr::Bailian(b) => b.cancel(),
        #[cfg(target_os = "windows")]
        ActiveAsr::FoundryLocalWhisper(local) => local.cancel(),
        #[cfg(target_os = "windows")]
        ActiveAsr::SherpaOnnxLocal(local) => local.cancel(),
        #[cfg(target_os = "macos")]
        ActiveAsr::Local(local) => local.cancel(),
        #[cfg(target_os = "macos")]
        ActiveAsr::AppleSpeech(local) => local.cancel(),
    }
}

pub(super) fn cancel_asr_for_session(inner: &Arc<Inner>, session_id: SessionId) {
    if let Some(asr) = take_asr_for_session(inner, session_id) {
        cancel_active_asr(asr);
    }
}

pub(super) fn store_recorder_for_session(
    inner: &Arc<Inner>,
    session_id: SessionId,
    recorder: Recorder,
) {
    *inner.recorder.lock() = Some(SessionResource::new(session_id, recorder));
}

pub(super) fn selected_microphone_device_name(inner: &Arc<Inner>) -> Option<String> {
    let name = inner.prefs.get().microphone_device_name.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub(super) fn stop_microphone_preview_monitor(inner: &Arc<Inner>, owner: &str) {
    #[cfg(mobile)]
    {
        let _ = (inner, owner);
    }
    #[cfg(not(mobile))]
    {
        let Some(app) = inner.app.lock().as_ref().cloned() else {
            return;
        };
        let state = app.state::<crate::commands::MicrophoneMonitorState>();
        let recorder = state.lock().take();
        if let Some(recorder) = recorder {
            log::info!("[recorder] stopping microphone preview monitor before {owner}");
            recorder.stop();
        }
    }
}

/// Acquire system-output mute for the duration of a recording session.
///
/// `AudioMuteGuard::activate()` on macOS shells out to `osascript` (~100–300 ms)
/// and on Linux to `wpctl`/`pactl` (similar). When called from the async
/// `begin_session` path that blocks the tokio worker thread for the entire
/// duration, delaying the recorder start by exactly that much. Wrap the
/// activate + bookkeeping in `spawn_blocking` so the tokio worker is freed
/// while the shell-out runs. Parking-lot `Mutex` guards never cross an await
/// (they live entirely inside the blocking task). Audit 3.2.4.
pub(super) async fn acquire_recording_mute(inner: &Arc<Inner>, owner: &'static str) {
    if !inner.prefs.get().mute_during_recording {
        return;
    }
    let inner = Arc::clone(inner);
    let join_result = tokio::task::spawn_blocking(move || {
        let mut mute = inner.recording_mute.lock();
        if mute.holders == 0 {
            match crate::audio_mute::AudioMuteGuard::activate() {
                Ok(guard) => {
                    mute.guard = Some(guard);
                    log::info!("[audio-mute] system output muted for recording");
                }
                Err(err) => {
                    log::warn!("[audio-mute] failed to mute output for {owner}: {err}");
                    return;
                }
            }
        }
        mute.holders = mute.holders.saturating_add(1);
        log::info!("[audio-mute] acquired by {owner}; holders={}", mute.holders);
    })
    .await;
    // 显式记录 spawn_blocking 任务的 panic（之前是 `let _ = .await` 静默吞掉）。
    // holders/guard 状态本身在 panic 路径下仍然一致 —— 因为 panic 只能发生在
    // activate() 抛 / lock 抛，前者会让 holders 不增 + guard 仍 None，后者根本
    // 进不到 mutate 阶段；但用户碰到 system audio 在录音时漏出系统声却找不到
    // 任何 [audio-mute] 日志，没法 debug。pr_agent feedback on PR #391。
    if let Err(join_err) = join_result {
        log::error!(
            "[audio-mute] acquire task panicked for {owner}: {join_err}; mute did not activate"
        );
    }
}

/// Release the recording-mute guard. The Drop impl on `AudioMuteGuard` shells
/// out to `osascript` / `wpctl` again, so when holders reaches 0 we hand the
/// drop off to a blocking task to keep the tokio worker free. Audit 3.2.4.
///
/// Fire-and-forget (no await): callers — `cancel_session`, `end_session`,
/// recorder error monitor — don't need the mute restoration to complete
/// before they continue. The user has already stopped recording; system audio
/// recovery happening 100 ms later is fine.
///
/// `release_recording_mute` is also called from non-tokio threads (the recorder
/// error monitor uses `std::thread::spawn`), so fall back to a synchronous
/// run when there's no current tokio handle — running synchronously on a std
/// thread blocks nothing.
pub(super) fn release_recording_mute(inner: &Arc<Inner>, owner: &'static str) {
    let inner = Arc::clone(inner);
    let work = move || {
        let mut mute = inner.recording_mute.lock();
        if mute.holders == 0 {
            return;
        }
        mute.holders -= 1;
        log::info!("[audio-mute] released by {owner}; holders={}", mute.holders);
        if mute.holders == 0 {
            mute.guard.take();
            log::info!("[audio-mute] system output mute restored after recording");
        }
    };
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn_blocking(work);
    } else {
        work();
    }
}

pub(super) fn stop_qa_recorder(inner: &Arc<Inner>) {
    if let Some(rec) = inner.qa_recorder.lock().take() {
        rec.stop();
        release_recording_mute(inner, "qa");
    }
}

pub(super) fn take_recorder_for_session(
    inner: &Arc<Inner>,
    session_id: SessionId,
) -> Option<Recorder> {
    let mut slot = inner.recorder.lock();
    take_session_resource(&mut slot, session_id)
}

pub(super) fn stop_recorder_for_session(inner: &Arc<Inner>, session_id: SessionId) {
    if let Some(recorder) = take_recorder_for_session(inner, session_id) {
        recorder.stop();
        release_recording_mute(inner, "dictation");
    }
}

pub(super) fn discard_startup_resources_for_session(inner: &Arc<Inner>, session_id: SessionId) {
    stop_recorder_for_session(inner, session_id);
    cancel_asr_for_session(inner, session_id);
}

pub(super) fn stop_recorder_if_pending_start_stop(inner: &Arc<Inner>) {
    let (should_stop, session_id) = {
        let state = inner.state.lock();
        (
            state.phase == SessionPhase::Starting && state.pending_stop,
            state.session_id,
        )
    };
    if !should_stop {
        return;
    }
    if let Some(rec) = take_recorder_for_session(inner, session_id) {
        rec.stop();
        release_recording_mute(inner, "dictation");
        let elapsed = inner.state.lock().started_at.elapsed().as_millis() as u64;
        emit_capsule(inner, CapsuleState::Transcribing, 0.0, elapsed, None, None);
        log::info!("[coord] stopped recorder while ASR is still connecting");
    }
}

#[cfg(test)]
mod tests {
    // issue #609 F-05：给零覆盖的纯函数补单测。take_session_resource 是 session_id
    // 守卫的核心——只在 id 匹配时取走资源，避免 stale session 的资源被错误复用。
    use super::{take_session_resource, SessionResource};
    use uuid::Uuid;

    fn sid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn take_session_resource_returns_resource_on_id_match() {
        let id = sid(1);
        let mut slot = Some(SessionResource::new(id, "payload"));
        let taken = take_session_resource(&mut slot, id);
        assert_eq!(taken, Some("payload"));
        // 取走后槽位应为空。
        assert!(slot.is_none());
    }

    #[test]
    fn take_session_resource_keeps_resource_on_id_mismatch() {
        let mut slot = Some(SessionResource::new(sid(1), "payload"));
        let taken = take_session_resource(&mut slot, sid(2));
        assert_eq!(taken, None, "id 不匹配不应取走（stale session 守卫）");
        // 资源仍在槽里，留给真正的 owner。
        assert!(slot.is_some());
    }

    #[test]
    fn take_session_resource_empty_slot_returns_none() {
        let mut slot: Option<SessionResource<&str>> = None;
        assert_eq!(take_session_resource(&mut slot, sid(1)), None);
    }
}
