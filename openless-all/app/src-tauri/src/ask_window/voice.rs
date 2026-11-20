use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Weak,
    },
    time::{Duration, Instant},
};

use openless_core::{
    api::ask_voice_capture::{Capture, Progress, Reservation},
    BackendError, BackendErrorCode, CapsulePayload, CapsuleState, SessionId,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::Desktop;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Mode {
    Insert,
    Submit,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Options {
    pub(crate) deep_thinking: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Editor {
    pub(crate) conversation_id: String,
    pub(crate) mount_id: String,
    pub(crate) editor_id: String,
    pub(crate) revision: u64,
    pub(crate) editable: bool,
    pub(crate) selection_start: u32,
    pub(crate) selection_end: u32,
    pub(crate) attachment_ids: Vec<String>,
    pub(crate) options: Options,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Target {
    pub(crate) focus_epoch: u64,
    pub(crate) editor: Editor,
}

impl Target {
    pub(super) fn matches(&self, current: &Self, mode: Mode) -> bool {
        self.focus_epoch == current.focus_epoch
            && self.editor.conversation_id == current.editor.conversation_id
            && self.editor.mount_id == current.editor.mount_id
            && self.editor.editor_id == current.editor.editor_id
            && (mode == Mode::Submit
                || (self.editor.editable
                    && current.editor.editable
                    && self.editor.revision == current.editor.revision
                    && self.editor.selection_start == current.editor.selection_start
                    && self.editor.selection_end == current.editor.selection_end))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivery {
    pub(crate) recording_id: SessionId,
    pub(crate) target: Target,
    pub(crate) mode: Mode,
    pub(crate) text: String,
}

pub(super) struct Controller {
    tx: mpsc::UnboundedSender<Message>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Message>>>,
    owner: Arc<Mutex<Option<Arc<Reservation>>>>,
}

enum Message {
    Intent {
        mode: Mode,
        target: Option<Target>,
        press: u64,
        toggle: bool,
    },
    Release {
        press: u64,
        held: Duration,
        hold: bool,
        auto: bool,
    },
    Stop(SessionId),
    Cancel(SessionId),
    CancelPress(u64),
    Revalidate,
    Progress(SessionId, Progress),
    Started(SessionId, Result<Capture, BackendError>),
    Finished(SessionId, Result<String, BackendError>),
}

struct Active {
    reservation: Arc<Reservation>,
    target: Target,
    mode: Mode,
    press: u64,
    capture: Option<Arc<Capture>>,
    finishing: bool,
}

impl Controller {
    pub(super) fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
            owner: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn listen(&self, desktop: Weak<Desktop>) {
        let Some(rx) = self.rx.lock().take() else {
            return;
        };
        tauri::async_runtime::spawn(listen(desktop, rx));
    }

    pub(super) fn owner(&self) -> Option<SessionId> {
        self.owner.lock().as_ref().map(|owner| owner.id())
    }

    pub(super) fn intent(&self, mode: Mode, target: Option<Target>, press: u64, toggle: bool) {
        let _ = self.tx.send(Message::Intent {
            mode,
            target,
            press,
            toggle,
        });
    }

    pub(super) fn cancel_press(&self, press: u64) {
        let _ = self.tx.send(Message::CancelPress(press));
    }

    pub(super) fn revalidate(&self) {
        let _ = self.tx.send(Message::Revalidate);
    }

    pub(super) fn release(&self, press: u64, held: Duration, hold: bool, auto: bool) {
        let _ = self.tx.send(Message::Release {
            press,
            held,
            hold,
            auto,
        });
    }

    pub(super) fn cancel(&self) {
        if let Some(owner) = self.owner.lock().as_ref() {
            owner.cancel();
            let _ = self.tx.send(Message::Cancel(owner.id()));
        }
    }

    pub(super) fn capsule_action(&self, recording_id: &str, cancel: bool) -> bool {
        let owner = self.owner.lock();
        let Some(owner) = owner
            .as_ref()
            .filter(|owner| owner.id().to_string() == recording_id)
        else {
            return false;
        };
        let command = if cancel {
            owner.cancel();
            Message::Cancel(owner.id())
        } else {
            Message::Stop(owner.id())
        };
        let _ = self.tx.send(command);
        true
    }
}

async fn listen(desktop: Weak<Desktop>, mut inbox: mpsc::UnboundedReceiver<Message>) {
    let mut active: Option<Active> = None;
    let mut last_start = None;
    while let Some(message) = inbox.recv().await {
        let Some(desktop) = desktop.upgrade() else {
            break;
        };
        match message {
            Message::Intent {
                mode,
                target,
                press,
                toggle,
            } => {
                let Some(target) = target.filter(|target| desktop.target_is_current(target, mode))
                else {
                    continue;
                };
                if let Some(recording) = active.as_mut() {
                    if mode == Mode::Submit
                        && recording.mode == Mode::Insert
                        && recording.press == press
                        && !recording.finishing
                        && desktop.target_is_current(&recording.target, Mode::Submit)
                    {
                        recording.mode = Mode::Submit;
                        desktop.notify_voice(
                            "started",
                            recording.reservation.id(),
                            &recording.target,
                            recording.mode,
                            None,
                        );
                        continue;
                    }
                    if toggle
                        && recording.mode == mode
                        && recording.press != press
                        && desktop.target_is_current(&recording.target, mode)
                    {
                        recording.stop(&desktop);
                    }
                    continue;
                }
                if desktop.current_voice_delivery().is_some()
                    || last_start
                        .is_some_and(|at: Instant| at.elapsed() < Duration::from_millis(250))
                {
                    continue;
                }
                desktop.voice_result.lock().take();
                let id = SessionId::new();
                let reservation = match desktop.backend.reserve_ask_voice(id) {
                    Ok(reservation) => Arc::new(reservation),
                    Err(error) => {
                        if error.code != BackendErrorCode::Busy {
                            desktop.notify_frontend("ask-window:voice-unavailable");
                        }
                        continue;
                    }
                };
                *desktop.voice.owner.lock() = Some(reservation.clone());
                if !desktop.target_is_current(&target, mode) {
                    reservation.cancel();
                    desktop.voice.owner.lock().take();
                    continue;
                }
                last_start = Some(Instant::now());
                desktop.remember_recording(id);
                desktop.notify_voice("started", id, &target, mode, None);
                frame(&desktop, id, CapsuleState::Recording, 0.0, 0, true);
                let backend = desktop.backend.clone();
                let tx = desktop.voice.tx.clone();
                let progress_tx = tx.clone();
                let last_meter = AtomicU64::new(0);
                let notify = Arc::new(move |progress| {
                    if let Progress::Level { elapsed_ms, .. } = &progress {
                        let previous = last_meter.load(Ordering::Relaxed);
                        if *elapsed_ms < previous.saturating_add(45) {
                            return;
                        }
                        last_meter.store(*elapsed_ms, Ordering::Relaxed);
                    }
                    let _ = progress_tx.send(Message::Progress(id, progress));
                });
                let startup = reservation.clone();
                tauri::async_runtime::spawn(async move {
                    let result = backend.start_ask_voice(&startup, notify).await;
                    if let Err(mpsc::error::SendError(Message::Started(_, Ok(capture)))) =
                        tx.send(Message::Started(id, result))
                    {
                        let _ = capture.cancel().await;
                    }
                });
                active = Some(Active {
                    reservation,
                    target,
                    mode,
                    press,
                    capture: None,
                    finishing: false,
                });
            }
            Message::Started(id, result) => {
                let current = active
                    .as_mut()
                    .filter(|recording| recording.reservation.id() == id);
                match (current, result) {
                    (Some(recording), Ok(capture)) => {
                        recording.capture = Some(Arc::new(capture));
                        if recording.reservation.is_cancelled()
                            || !desktop.target_is_current(&recording.target, recording.mode)
                        {
                            cancel(&desktop, &mut active, id, CapsuleState::Cancelled);
                        } else if recording.finishing {
                            recording.finish_capture(&desktop);
                        } else {
                            frame(&desktop, id, CapsuleState::Recording, 0.0, 0, false);
                        }
                    }
                    (Some(_), Err(error)) => {
                        complete(&desktop, &mut active, id, Err(error));
                    }
                    (None, Ok(capture)) => {
                        tauri::async_runtime::spawn(capture.cancel());
                    }
                    (None, Err(_)) => {}
                }
            }
            Message::Release {
                press,
                held,
                hold,
                auto,
            } => {
                if let Some(recording) = active.as_mut().filter(|recording| {
                    recording.mode == Mode::Insert
                        && recording.press == press
                        && (hold || (auto && held >= Duration::from_millis(350)))
                }) {
                    recording.stop(&desktop);
                }
            }
            Message::Stop(id) | Message::Progress(id, Progress::Stop) => {
                if let Some(recording) = active
                    .as_mut()
                    .filter(|recording| recording.reservation.id() == id)
                {
                    recording.stop(&desktop);
                }
            }
            Message::Cancel(id) | Message::Progress(id, Progress::NoSpeech) => {
                cancel(&desktop, &mut active, id, CapsuleState::Cancelled);
            }
            Message::CancelPress(press) => {
                if let Some(id) = active
                    .as_ref()
                    .filter(|recording| recording.press == press)
                    .map(|recording| recording.reservation.id())
                {
                    cancel(&desktop, &mut active, id, CapsuleState::Cancelled);
                }
            }
            Message::Revalidate => {
                if let Some(id) = active
                    .as_ref()
                    .filter(|recording| {
                        !desktop.target_is_current(&recording.target, recording.mode)
                    })
                    .map(|recording| recording.reservation.id())
                {
                    cancel(&desktop, &mut active, id, CapsuleState::Cancelled);
                }
            }
            Message::Progress(id, Progress::Fatal(_)) => {
                cancel(&desktop, &mut active, id, CapsuleState::Error);
            }
            Message::Progress(id, Progress::Level { elapsed_ms, level }) => {
                if active.as_ref().is_some_and(|recording| {
                    recording.reservation.id() == id
                        && !recording.finishing
                        && recording.capture.is_some()
                        && desktop.target_is_current(&recording.target, recording.mode)
                }) {
                    frame(
                        &desktop,
                        id,
                        CapsuleState::Recording,
                        level,
                        elapsed_ms,
                        false,
                    );
                }
            }
            Message::Finished(id, result) => complete(&desktop, &mut active, id, result),
        }
    }
    if let Some(recording) = active {
        recording.reservation.cancel();
        if let Some(capture) = recording.capture {
            let _ = capture.cancel().await;
        }
    }
}

impl Active {
    fn stop(&mut self, desktop: &Desktop) {
        if self.finishing {
            return;
        }
        self.finishing = true;
        frame(
            desktop,
            self.reservation.id(),
            CapsuleState::Transcribing,
            0.0,
            0,
            false,
        );
        self.finish_capture(desktop);
    }

    fn finish_capture(&self, desktop: &Desktop) {
        let Some(capture) = self.capture.clone() else {
            return;
        };
        let tx = desktop.voice.tx.clone();
        let id = self.reservation.id();
        tauri::async_runtime::spawn(async move {
            let result = capture.finish().await;
            let _ = tx.send(Message::Finished(id, result));
        });
    }
}

fn take_owner(desktop: &Desktop, active: &mut Option<Active>, id: SessionId) -> Option<Active> {
    if active.as_ref()?.reservation.id() != id {
        return None;
    }
    active
        .take()
        .inspect(|recording| recording.reservation.cancel())
        .map(|recording| {
            desktop.voice.owner.lock().take();
            recording
        })
}

fn cancel(desktop: &Desktop, active: &mut Option<Active>, id: SessionId, state: CapsuleState) {
    if active
        .as_ref()
        .is_none_or(|recording| recording.reservation.id() != id)
    {
        return;
    }
    if let Some(recording) = active.as_ref() {
        desktop.notify_voice("cancelled", id, &recording.target, recording.mode, None);
    }
    frame(desktop, id, state, 0.0, 0, false);
    if let Some(capture) = take_owner(desktop, active, id).and_then(|recording| recording.capture) {
        tauri::async_runtime::spawn(capture.cancel());
    }
}

fn complete(
    desktop: &Desktop,
    active: &mut Option<Active>,
    id: SessionId,
    result: Result<String, BackendError>,
) {
    let Some(recording) = active
        .as_ref()
        .filter(|recording| recording.reservation.id() == id)
    else {
        return;
    };
    let current = desktop.target_is_current(&recording.target, recording.mode)
        && !recording.reservation.is_cancelled();
    let state = match result {
        Ok(text) if current && !text.trim().is_empty() => {
            let mut delivery = desktop.voice_result.lock();
            *delivery = Some(Delivery {
                recording_id: id,
                target: recording.target.clone(),
                mode: recording.mode,
                text,
            });
            drop(delivery);
            if let Some(delivery) = desktop.current_voice_delivery() {
                desktop.notify_voice(
                    "transcribed",
                    id,
                    &delivery.target,
                    delivery.mode,
                    Some(&delivery.text),
                );
            } else {
                desktop.voice_result.lock().take();
            }
            CapsuleState::Done
        }
        Ok(_) => CapsuleState::Cancelled,
        Err(error) if error.code == BackendErrorCode::Cancelled || !current => {
            CapsuleState::Cancelled
        }
        Err(_) => CapsuleState::Error,
    };
    if state != CapsuleState::Done {
        desktop.notify_voice("cancelled", id, &recording.target, recording.mode, None);
    }
    frame(desktop, id, state, 0.0, 0, false);
    take_owner(desktop, active, id);
}

fn frame(
    desktop: &Desktop,
    id: SessionId,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    warming: bool,
) {
    desktop.ports.capsule(CapsulePayload {
        session_id: Some(id.to_string()),
        state,
        level,
        elapsed_ms,
        warming,
        message: None,
        inserted_chars: None,
        translation: false,
        operating: false,
        capsule_style: desktop.backend.get_preferences().capsule_style,
        selection_polish: false,
        qa: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composer() -> Target {
        Target {
            focus_epoch: 1,
            editor: Editor {
                conversation_id: "conversation".into(),
                mount_id: "mount".into(),
                editor_id: "editor".into(),
                revision: 1,
                editable: true,
                selection_start: 2,
                selection_end: 3,
                attachment_ids: vec![],
                options: Options {
                    deep_thinking: false,
                },
            },
        }
    }

    #[test]
    fn both_delivery_modes_reject_a_different_focus_generation_or_conversation() {
        let original = composer();
        let mut refocused = composer();
        refocused.focus_epoch += 1;
        let mut other_conversation = composer();
        other_conversation.editor.conversation_id = "another".into();
        let mut replaced_view = composer();
        replaced_view.editor.mount_id = "new-mount".into();
        for mode in [Mode::Insert, Mode::Submit] {
            assert!(original.matches(&original, mode));
            for new_target in [&refocused, &other_conversation, &replaced_view] {
                assert!(!original.matches(new_target, mode));
            }
        }
    }

    #[test]
    fn changed_draft_or_caret_blocks_insert_but_not_the_frozen_submit() {
        let original = composer();
        let mut changed = composer();
        changed.editor.selection_end += 1;
        assert!(!original.matches(&changed, Mode::Insert));
        changed.editor.revision += 1;
        assert!(original.matches(&changed, Mode::Submit));
        assert!(!original.matches(&changed, Mode::Insert));
    }
}
