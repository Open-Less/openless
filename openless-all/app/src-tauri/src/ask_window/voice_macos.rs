use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::Duration,
};

use openless_core::{BackendErrorCode, BackendEventKind, DictationPhase, SessionId};
use parking_lot::Mutex;
use tokio::sync::mpsc;

pub(crate) use super::voice_types::{Delivery, Editor, Target};
use super::{hotkey::FnCommit, Desktop};

pub(super) struct Controller {
    tx: mpsc::UnboundedSender<Message>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Message>>>,
    owner: Mutex<Option<SessionId>>,
}

enum Message {
    Start { target: Option<Target>, press: Option<u64>, request: Option<u64>, toggle: bool, fn_commit: Option<FnCommit> },
    StopPress(u64),
    CancelPress(u64),
    Cancel,
    Revalidate,
    StartFailed(SessionId, BackendErrorCode),
}

struct Active {
    id: SessionId,
    target: Target,
    opening: u64,
    press: Option<u64>,
    cancelled: Arc<AtomicBool>,
    stopping: bool,
    stop_dispatched: bool,
}

enum Completion {
    Transcribed(String),
    Failed(&'static str),
    Cancelled,
}

impl Controller {
    pub(super) fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx: Mutex::new(Some(rx)), owner: Mutex::new(None) }
    }

    pub(super) fn listen(&self, desktop: Weak<Desktop>) {
        if let Some(rx) = self.rx.lock().take() {
            tauri::async_runtime::spawn(listen(desktop, rx));
        }
    }

    pub(super) fn owner(&self) -> Option<SessionId> {
        *self.owner.lock()
    }

    pub(super) fn manual(&self, target: Option<Target>, press: u64, toggle: bool, fn_commit: Option<FnCommit>) {
        let _ = self.tx.send(Message::Start {
            target, press: Some(press), request: None, toggle, fn_commit,
        });
    }

    pub(super) fn autostart(&self, request: u64, target: Target) {
        let _ = self.tx.send(Message::Start {
            target: Some(target), press: None, request: Some(request), toggle: false, fn_commit: None,
        });
    }

    pub(super) fn release(&self, press: u64, held: Duration, hold: bool, auto: bool) {
        if hold || (auto && held >= Duration::from_millis(350)) {
            let _ = self.tx.send(Message::StopPress(press));
        }
    }

    pub(super) fn cancel_press(&self, press: u64) {
        let _ = self.tx.send(Message::CancelPress(press));
    }

    pub(super) fn revalidate(&self) {
        let _ = self.tx.send(Message::Revalidate);
    }

    pub(super) fn cancel(&self) {
        let _ = self.tx.send(Message::Cancel);
    }

    pub(super) fn capsule_action(&self, _recording_id: &str, _cancel: bool) -> bool {
        false
    }
}

async fn listen(desktop: Weak<Desktop>, mut inbox: mpsc::UnboundedReceiver<Message>) {
    let mut events = match desktop.upgrade() {
        Some(desktop) => desktop.backend.subscribe(),
        None => return,
    };
    let mut active: Option<Active> = None;
    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(desktop) = desktop.upgrade() else { break };
                if matches!(event, Err(openless_core::EventRecvError::Closed)) {
                    break;
                }
                let Some(recording) = active.as_mut() else { continue };
                let Ok(event) = event else {
                    recording.cancel(&desktop);
                    complete(&desktop, &mut active, Completion::Cancelled);
                    continue;
                };
                if event.session_id != Some(recording.id) {
                    continue;
                }
                match event.kind {
                    BackendEventKind::DictationCompleted(result) => {
                        complete(&desktop, &mut active, Completion::Transcribed(result.polished_text));
                    }
                    BackendEventKind::DictationStateChanged(state) => match state.phase {
                        DictationPhase::Starting if recording.stopping => {
                            recording.request_stop(&desktop);
                        }
                        DictationPhase::Failed => {
                            complete(&desktop, &mut active, Completion::Failed("processing"));
                        }
                        DictationPhase::Cancelled => {
                            complete(&desktop, &mut active, Completion::Cancelled);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            message = inbox.recv() => {
                let (Some(desktop), Some(message)) = (desktop.upgrade(), message) else { break };
                match message {
                    Message::Start { target, press, request, toggle, fn_commit } => {
                        if request.is_some_and(|request| !desktop.autostart_is_pending(request)) {
                            continue;
                        }
                        let press_is_current = move || fn_commit.is_none_or(|commit| {
                            press.is_some_and(|press| super::hotkey::fn_stroke_is_current(press, commit))
                        });
                        if !press_is_current() {
                            continue;
                        }
                        if let Some(recording) = active.as_mut() {
                            if toggle && desktop.target_is_current(&recording.target) {
                                recording.stop(&desktop);
                            }
                        } else if let Some(target) = target.filter(|target| desktop.target_is_current(target)) {
                            if desktop.current_voice_delivery().is_none() {
                                let id = SessionId::new();
                                let cancelled = Arc::new(AtomicBool::new(false));
                                *desktop.voice.owner.lock() = Some(id);
                                desktop.notify_voice("started", id, &target, None);
                                active = Some(Active {
                                    id, target: target.clone(), opening: desktop.invocation_epoch.load(Ordering::Acquire),
                                    press, cancelled: cancelled.clone(),
                                    stopping: false, stop_dispatched: false,
                                });
                                let desktop = desktop.clone();
                                tauri::async_runtime::spawn(async move {
                                    let current = AtomicBool::new(false);
                                    let result = desktop.backend.start_dictation_for_ask_composer(id, || {
                                        let owns_press = current.load(Ordering::Relaxed) || press_is_current();
                                        current.store(owns_press, Ordering::Relaxed);
                                        owns_press && !cancelled.load(Ordering::Acquire)
                                            && desktop.target_is_current(&target)
                                    }).await;
                                    if let Err(error) = result {
                                        let _ = desktop.voice.tx.send(Message::StartFailed(id, error.code));
                                    }
                                });
                            }
                        }
                        if let Some(request) = request {
                            desktop.complete_autostart(request);
                        }
                    }
                    Message::StopPress(press) => {
                        if let Some(recording) = active.as_mut().filter(|recording| recording.press == Some(press)) {
                            recording.stop(&desktop);
                        }
                    }
                    Message::CancelPress(press) => {
                        if let Some(recording) = active.as_ref().filter(|recording| recording.press == Some(press)) {
                            recording.cancel(&desktop);
                        }
                    }
                    Message::Cancel => {
                        if let Some(recording) = active.as_ref() {
                            recording.cancel(&desktop);
                        }
                    }
                    Message::Revalidate => {
                        if let Some(recording) = active.as_ref().filter(|recording| !desktop.target_is_current(&recording.target)) {
                            recording.cancel(&desktop);
                        }
                    }
                    Message::StartFailed(id, code) => {
                        if active.as_ref().is_some_and(|recording| recording.id == id) {
                            let completion = if matches!(code, BackendErrorCode::Cancelled | BackendErrorCode::Busy) {
                                Completion::Cancelled
                            } else {
                                Completion::Failed("start")
                            };
                            complete(&desktop, &mut active, completion);
                        }
                    }
                }
            }
        }
    }
}

impl Active {
    fn stop(&mut self, desktop: &Desktop) {
        if self.stopping || self.cancelled.load(Ordering::Acquire) {
            return;
        }
        self.stopping = true;
        if desktop.backend.snapshot().dictation.session_id == Some(self.id) {
            self.request_stop(desktop);
        }
    }

    fn request_stop(&mut self, desktop: &Desktop) {
        if self.stop_dispatched {
            return;
        }
        self.stop_dispatched = true;
        let backend = desktop.backend.clone();
        let id = self.id;
        tauri::async_runtime::spawn(async move {
            if backend.snapshot().dictation.session_id == Some(id) {
                let _ = backend.stop_dictation_session(id).await;
            }
        });
    }

    fn cancel(&self, desktop: &Desktop) {
        if self.cancelled.swap(true, Ordering::AcqRel) {
            return;
        }
        let backend = desktop.backend.clone();
        let id = self.id;
        tauri::async_runtime::spawn(async move {
            let _ = backend.cancel_dictation(Some(id)).await;
        });
    }
}

fn complete(desktop: &Desktop, active: &mut Option<Active>, completion: Completion) {
    let Some(recording) = active.take() else { return };
    let current = !recording.cancelled.load(Ordering::Acquire)
        && recording.opening == desktop.invocation_epoch.load(Ordering::Acquire)
        && desktop.target_is_current(&recording.target);
    match completion {
        Completion::Transcribed(text) if current && !text.trim().is_empty() => {
            *desktop.voice_result.lock() = Some(Delivery {
                recording_id: recording.id,
                target: recording.target.clone(),
                text,
            });
            if let Some(delivery) = desktop.current_voice_delivery() {
                desktop.notify_voice("transcribed", recording.id, &recording.target, Some(&delivery.text));
            } else {
                desktop.voice_result.lock().take();
            }
        }
        Completion::Failed(stage) if current => {
            desktop.notify_voice("failed", recording.id, &recording.target, Some(stage));
        }
        _ => desktop.notify_voice("cancelled", recording.id, &recording.target, None),
    }
    desktop.voice.owner.lock().take();
}
