use std::sync::{Arc, Mutex};

use crate::{BackendError, BackendErrorCode, SessionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceSessionKind {
    Dictation,
    LessComputer,
    SelectionVoice,
    Qa,
}

#[derive(Debug)]
struct ActiveVoiceSession {
    session_id: SessionId,
    kind: VoiceSessionKind,
    released: bool,
    resources: usize,
    cancel: crate::CancellationToken,
}

#[derive(Debug, Default)]
pub(crate) struct VoiceSessionGate {
    active: Mutex<Option<ActiveVoiceSession>>,
}

impl VoiceSessionGate {
    pub(crate) fn acquire(
        &self,
        session_id: SessionId,
        kind: VoiceSessionKind,
    ) -> Result<(), BackendError> {
        let mut active = self.active.lock().expect("voice session lock poisoned");
        match active.as_ref() {
            Some(current)
                if current.session_id == session_id
                    && current.kind == kind
                    && !current.released =>
            {
                Ok(())
            }
            Some(current) => Err(BackendError::new(
                BackendErrorCode::Busy,
                format!("another voice session is active: {:?}", current.kind),
            )),
            None => {
                *active = Some(ActiveVoiceSession {
                    session_id,
                    kind,
                    released: false,
                    resources: 0,
                    cancel: crate::CancellationToken::new(),
                });
                Ok(())
            }
        }
    }

    pub(crate) fn release(&self, session_id: SessionId) {
        let mut active = self.active.lock().expect("voice session lock poisoned");
        if let Some(current) = active
            .as_mut()
            .filter(|current| current.session_id == session_id)
        {
            current.released = true;
            current.cancel.cancel();
            if current.resources == 0 {
                *active = None;
            }
        }
    }

    /// Logical cancellation invalidates the token immediately. Native startup,
    /// stop and provider cleanup keep this hold until their last owned task ends.
    pub(crate) fn hold_resources(
        self: &Arc<Self>,
        session_id: SessionId,
    ) -> Result<Arc<VoiceResourceHold>, BackendError> {
        let mut active = self.active.lock().expect("voice session lock poisoned");
        let current = active
            .as_mut()
            .filter(|current| current.session_id == session_id && !current.released)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::Cancelled,
                    "voice session was cancelled before capture",
                )
            })?;
        current.resources += 1;
        Ok(Arc::new(VoiceResourceHold {
            gate: Arc::clone(self),
            session_id,
            cancel: current.cancel.clone(),
        }))
    }
}

pub(crate) struct VoiceResourceHold {
    gate: Arc<VoiceSessionGate>,
    session_id: SessionId,
    pub(crate) cancel: crate::CancellationToken,
}

impl Drop for VoiceResourceHold {
    fn drop(&mut self) {
        let mut active = self
            .gate
            .active
            .lock()
            .expect("voice session lock poisoned");
        if let Some(current) = active
            .as_mut()
            .filter(|current| current.session_id == self.session_id)
        {
            current.resources -= 1;
            if current.released && current.resources == 0 {
                *active = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_session_is_idempotent_and_other_kinds_are_busy() {
        let gate = VoiceSessionGate::default();
        let session_id = SessionId::new();
        gate.acquire(session_id, VoiceSessionKind::Dictation)
            .unwrap();
        gate.acquire(session_id, VoiceSessionKind::Dictation)
            .unwrap();
        assert_eq!(
            gate.acquire(SessionId::new(), VoiceSessionKind::Qa)
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        gate.release(SessionId::new());
        assert_eq!(
            gate.acquire(SessionId::new(), VoiceSessionKind::Qa)
                .unwrap_err()
                .code,
            BackendErrorCode::Busy
        );
        gate.release(session_id);
        gate.acquire(SessionId::new(), VoiceSessionKind::Qa)
            .unwrap();
    }
}
