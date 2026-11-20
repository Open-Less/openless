//! A focused Ask composer is a regular dictation destination, not a QA turn.
use std::collections::VecDeque;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use openless_core::ports::{
    CapturedTextTarget, InsertOutcome, InsertWriteResult, NoopHostContextAdapter, TextInserter,
    TextInsertionSession,
};
use openless_core::{
    BackendError, BackendErrorCode, DictationContext, DictationPhase, DictationStateSnapshot,
    SessionId,
};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
struct DraftFocus {
    token: Option<String>,
    focused: bool,
}

static FOCUS: Mutex<DraftFocus> = Mutex::new(DraftFocus {
    token: None,
    focused: false,
});

pub(crate) fn set_focus(token: String, focused: bool) {
    let mut state = FOCUS.lock();
    if focused {
        *state = DraftFocus {
            token: Some(token),
            focused: true,
        };
    } else if state.token.as_ref() == Some(&token) {
        state.focused = false;
    }
}

pub(crate) fn clear() {
    *FOCUS.lock() = DraftFocus::default();
}

// Owners survive terminal events and replays. Closing the editor revokes insertion,
// not ownership: a still-finishing old session must not open a global capsule.
static OWNERS: Mutex<VecDeque<(SessionId, String)>> = Mutex::new(VecDeque::new());

fn remember_owner(owners: &mut VecDeque<(SessionId, String)>, id: SessionId, token: String) {
    owners.push_back((id, token));
    if owners.len() > 8 {
        owners.pop_front();
    }
}

fn owner(id: SessionId) -> Option<String> {
    OWNERS
        .lock()
        .iter()
        .find(|(session, _)| *session == id)
        .map(|(_, token)| token.clone())
}

pub(crate) fn owns(id: SessionId) -> bool {
    OWNERS.lock().iter().any(|(session, _)| *session == id)
}

pub(crate) fn present(app: &AppHandle, snapshot: &DictationStateSnapshot) -> bool {
    let Some(id) = snapshot.session_id else {
        return false;
    };
    let Some(token) = owner(id) else {
        return false;
    };
    let phase = match snapshot.phase {
        DictationPhase::Starting | DictationPhase::Recording => "recording",
        DictationPhase::Transcribing | DictationPhase::Polishing | DictationPhase::Inserting => {
            "processing"
        }
        DictationPhase::Failed => "failed",
        _ => "idle",
    };
    let _ = app.emit_to(
        "qa",
        "qa:draft-recording",
        serde_json::json!({
            "sessionId": id,
            "token": token,
            "phase": phase,
            "level": snapshot.level,
            "elapsedMs": snapshot.elapsed_ms,
            "error": if snapshot.phase == DictationPhase::Failed { snapshot.message.as_deref() } else { None },
        }),
    );
    true
}

pub(crate) fn capture(app: &AppHandle, session_id: SessionId) -> Option<CapturedTextTarget> {
    let token = {
        let state = FOCUS.lock();
        state.focused.then(|| state.token.clone()).flatten()?
    };
    let window = app.get_webview_window("qa")?;
    if !window.is_focused().unwrap_or(false) {
        return None;
    }
    remember_owner(&mut OWNERS.lock(), session_id, token.clone());
    Some(CapturedTextTarget {
        inserter: Arc::new(DraftTarget {
            app: app.clone(),
            token,
        }),
        host_context: Arc::new(NoopHostContextAdapter),
    })
}

#[derive(Clone)]
struct DraftTarget {
    app: AppHandle,
    token: String,
}

impl TextInserter for DraftTarget {
    fn begin(
        &self,
        _session_id: SessionId,
        _context: Arc<DictationContext>,
    ) -> BoxFuture<'static, Result<Arc<dyn TextInsertionSession>, BackendError>> {
        let target = self.clone();
        Box::pin(async move { Ok(Arc::new(target) as Arc<dyn TextInsertionSession>) })
    }
}

impl DraftTarget {
    fn deliver(&self, text: String) -> Result<(), BackendError> {
        let focus = FOCUS.lock();
        if focus.token.as_ref() != Some(&self.token) {
            return Err(BackendError::new(
                BackendErrorCode::Cancelled,
                "Ask draft is no longer open",
            ));
        }
        self.app
            .emit_to(
                "qa",
                "qa:dictation-draft",
                serde_json::json!({
                    "token": self.token,
                    "text": text,
                }),
            )
            .map_err(|error| BackendError::new(BackendErrorCode::Platform, error.to_string()))
    }
}

impl TextInsertionSession for DraftTarget {
    fn supports_streaming(&self) -> bool {
        false
    }

    fn write(&self, _text: String) -> BoxFuture<'static, Result<InsertWriteResult, BackendError>> {
        Box::pin(async { Ok(InsertWriteResult { written_chars: 0 }) })
    }

    fn copy(&self, text: String) -> BoxFuture<'static, Result<(), BackendError>> {
        let target = self.clone();
        Box::pin(async move { target.deliver(text) })
    }

    fn finish(&self, text: String) -> BoxFuture<'static, Result<InsertOutcome, BackendError>> {
        let target = self.clone();
        Box::pin(async move { target.deliver(text).map(|()| InsertOutcome::Inserted) })
    }

    fn cancel(&self) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}
