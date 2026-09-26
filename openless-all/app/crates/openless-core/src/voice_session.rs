use std::sync::{Arc, Mutex, OnceLock};

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

#[derive(Default)]
pub(crate) struct VoiceSessionGate {
    active: Mutex<Option<ActiveVoiceSession>>,
    restore_guard: OnceLock<crate::domains::RuntimeRestoreGuard>,
}

impl VoiceSessionGate {
    pub(crate) fn bind_restore_guard(
        &self,
        guard: crate::domains::RuntimeRestoreGuard,
    ) -> Result<(), BackendError> {
        self.restore_guard.set(guard).map_err(|_| {
            BackendError::new(
                BackendErrorCode::InvalidState,
                "voice restore guard is already bound",
            )
        })
    }

    pub(crate) fn runtime_restore_idle(&self) -> bool {
        // A released owner with native cleanup holds is deliberately not idle.
        self.active.lock().is_ok_and(|active| active.is_none())
    }

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
                // Same mutex as the restore idle probe: the restoring flag is
                // checked before claiming the owner, without a check/claim gap.
                if let Some(guard) = self.restore_guard.get() {
                    guard()?;
                }
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

/// Counts non-audio runtime work without taking the exclusive voice slot.
/// Domain callers acquire a work lease while holding their own state lock;
/// probes read that same state lock before this short counter mutex.
#[derive(Default)]
pub(crate) struct RuntimeActivityGate {
    state: Mutex<RuntimeActivityState>,
    binding: OnceLock<RuntimeActivityBinding>,
}

#[derive(Default)]
struct RuntimeActivityState {
    active: usize,
    abandoned_cleanup: bool,
}
struct RuntimeActivityBinding {
    guard: crate::domains::RuntimeRestoreGuard,
    spawner: Arc<dyn crate::config::TaskSpawner>,
}

impl RuntimeActivityGate {
    pub(crate) fn bind(
        &self,
        guard: crate::domains::RuntimeRestoreGuard,
        spawner: Arc<dyn crate::config::TaskSpawner>,
    ) -> Result<(), BackendError> {
        self.binding
            .set(RuntimeActivityBinding { guard, spawner })
            .map_err(|_| {
                BackendError::new(
                    BackendErrorCode::InvalidState,
                    "runtime restore guard is already bound",
                )
            })
    }

    pub(crate) fn acquire(self: &Arc<Self>) -> Result<RuntimeActivityHold, BackendError> {
        let mut state = self.state.lock().expect("runtime activity lock poisoned");
        if let Some(binding) = self.binding.get() {
            (binding.guard)()?;
        }
        state.active = state
            .active
            .checked_add(1)
            .expect("runtime activity count overflow");
        Ok(RuntimeActivityHold {
            gate: Arc::clone(self),
            clean: true,
        })
    }

    /// Continue an owner already validated under the domain state lock.
    /// Its active phase makes the restore probe reject admission, even if a
    /// restore attempt has raised its flag immediately before that probe.
    pub(crate) fn existing_work(self: &Arc<Self>) -> RuntimeActivityHold {
        let mut state = self.state.lock().expect("runtime activity lock poisoned");
        state.active = state
            .active
            .checked_add(1)
            .expect("runtime activity count overflow");
        RuntimeActivityHold {
            gate: Arc::clone(self),
            clean: true,
        }
    }

    fn cleanup_hold(self: &Arc<Self>) -> RuntimeActivityHold {
        let mut state = self.state.lock().expect("runtime activity lock poisoned");
        state.active = state
            .active
            .checked_add(1)
            .expect("runtime activity count overflow");
        RuntimeActivityHold {
            gate: Arc::clone(self),
            clean: false,
        }
    }

    pub(crate) fn runtime_restore_idle(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.active == 0 && !state.abandoned_cleanup)
    }

    /// A restore-enabled Host owns cleanup through its existing TaskSpawner.
    /// Dropping the IPC waiter does not release the lease or cancel cleanup.
    /// Hosts without restore binding retain their original executor behavior.
    pub(crate) fn cleanup<T: Send + 'static>(
        self: &Arc<Self>,
        work: futures_util::future::BoxFuture<'static, Result<T, BackendError>>,
    ) -> futures_util::future::BoxFuture<'static, Result<T, BackendError>> {
        let gate = Arc::clone(self);
        let spawner = self
            .binding
            .get()
            .map(|binding| Arc::clone(&binding.spawner));
        Box::pin(async move {
            // Claim before handing the task to the scheduler: the caller may
            // drop its main lease as soon as this future first yields.
            let hold = gate.cleanup_hold();
            let task = async move {
                let result = work.await;
                hold.complete();
                result
            };
            if let Some(spawner) = spawner {
                let (send, receive) = tokio::sync::oneshot::channel();
                spawner.spawn(Box::pin(async move {
                    let _ = send.send(task.await);
                }));
                receive.await.map_err(|_| {
                    BackendError::new(
                        BackendErrorCode::Cancelled,
                        "runtime cleanup task was interrupted",
                    )
                })?
            } else {
                task.await
            }
        })
    }
}

pub(crate) struct RuntimeActivityHold {
    gate: Arc<RuntimeActivityGate>,
    clean: bool,
}
impl RuntimeActivityHold {
    fn complete(mut self) {
        // Consume the whole guard so async field capture cannot leave its
        // Drop on the caller while moving only the Copy completion flag.
        self.clean = true;
    }
}
impl Drop for RuntimeActivityHold {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .expect("runtime activity lock poisoned");
        state.active -= 1;
        if !self.clean {
            state.abandoned_cleanup = true;
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
    fn restore_guard(
        flag: Arc<std::sync::atomic::AtomicBool>,
    ) -> crate::domains::RuntimeRestoreGuard {
        Arc::new(move || {
            if flag.load(std::sync::atomic::Ordering::Acquire) {
                Err(BackendError::new(
                    BackendErrorCode::Busy,
                    "restore in progress",
                ))
            } else {
                Ok(())
            }
        })
    }

    #[test]
    fn runtime_restore_blocks_new_voice_but_preserves_an_existing_owner() {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let gate = Arc::new(VoiceSessionGate::default());
        gate.bind_restore_guard(restore_guard(flag.clone()))
            .unwrap();
        let owner = SessionId::new();
        gate.acquire(owner, VoiceSessionKind::Dictation).unwrap();
        let resources = gate.hold_resources(owner).unwrap();
        flag.store(true, std::sync::atomic::Ordering::Release);
        gate.acquire(owner, VoiceSessionKind::Dictation).unwrap();
        gate.release(owner);
        assert!(
            !gate.runtime_restore_idle(),
            "native cleanup still owns resources"
        );
        drop(resources);
        assert!(gate.runtime_restore_idle());
        for kind in [
            VoiceSessionKind::Dictation,
            VoiceSessionKind::LessComputer,
            VoiceSessionKind::Qa,
            VoiceSessionKind::SelectionVoice,
        ] {
            assert_eq!(
                gate.acquire(SessionId::new(), kind).unwrap_err().code,
                BackendErrorCode::Busy
            );
        }
        flag.store(false, std::sync::atomic::Ordering::Release);
        gate.acquire(SessionId::new(), VoiceSessionKind::Qa)
            .unwrap();
    }

    #[test]
    fn runtime_restore_probe_cannot_cross_a_voice_check_claim_race() {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let gate = Arc::new(VoiceSessionGate::default());
        let (checked, checked_rx) = std::sync::mpsc::channel();
        let (resume, resume_rx) = std::sync::mpsc::channel();
        let resume_rx = Mutex::new(resume_rx);
        let check_flag = flag.clone();
        gate.bind_restore_guard(Arc::new(move || {
            assert!(!check_flag.load(std::sync::atomic::Ordering::Acquire));
            checked.send(()).unwrap();
            resume_rx.lock().unwrap().recv().unwrap();
            Ok(())
        }))
        .unwrap();
        let owner = SessionId::new();
        let starting = std::thread::spawn({
            let gate = gate.clone();
            move || gate.acquire(owner, VoiceSessionKind::Dictation)
        });
        checked_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        flag.store(true, std::sync::atomic::Ordering::Release);
        let probing = std::thread::spawn({
            let gate = gate.clone();
            move || gate.runtime_restore_idle()
        });
        resume.send(()).unwrap();
        starting.join().unwrap().unwrap();
        assert!(
            !probing.join().unwrap(),
            "restore must reject the newly admitted voice owner"
        );
        gate.release(owner);
    }

    #[derive(Default)]
    struct CapturingSpawner {
        tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    }
    impl crate::config::TaskSpawner for CapturingSpawner {
        fn spawn(&self, task: futures_util::future::BoxFuture<'static, ()>) {
            self.tasks.lock().unwrap().push(tokio::spawn(task));
        }
    }

    #[tokio::test]
    async fn runtime_restore_waits_for_owned_cleanup_after_waiter_is_dropped() {
        let gate = Arc::new(RuntimeActivityGate::default());
        let spawner = Arc::new(CapturingSpawner::default());
        gate.bind(Arc::new(|| Ok(())), spawner.clone()).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let waiter = tokio::spawn(gate.cleanup(Box::pin({
            let entered = entered.clone();
            let release = release.clone();
            async move {
                entered.notify_one();
                release.notified().await;
                Ok(())
            }
        })));
        entered.notified().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert!(!gate.runtime_restore_idle());
        let owned = spawner.tasks.lock().unwrap().pop().unwrap();
        release.notify_one();
        owned.await.unwrap();
        assert!(gate.runtime_restore_idle());
    }

    #[tokio::test]
    async fn runtime_restore_fails_closed_if_the_cleanup_owner_itself_is_aborted() {
        let gate = Arc::new(RuntimeActivityGate::default());
        let spawner = Arc::new(CapturingSpawner::default());
        gate.bind(Arc::new(|| Ok(())), spawner.clone()).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let waiter = tokio::spawn(gate.cleanup(Box::pin({
            let entered = entered.clone();
            async move {
                entered.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            }
        })));
        entered.notified().await;
        let owned = spawner.tasks.lock().unwrap().pop().unwrap();
        owned.abort();
        assert!(owned.await.unwrap_err().is_cancelled());
        assert_eq!(
            waiter.await.unwrap().unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        assert!(
            !gate.runtime_restore_idle(),
            "uncertain cleanup cannot make restore safe"
        );
    }
    #[derive(Default)]
    struct QueuedSpawner {
        tasks: Mutex<Vec<futures_util::future::BoxFuture<'static, ()>>>,
    }
    impl crate::config::TaskSpawner for QueuedSpawner {
        fn spawn(&self, task: futures_util::future::BoxFuture<'static, ()>) {
            self.tasks.lock().unwrap().push(task);
        }
    }

    #[tokio::test]
    async fn runtime_restore_error_cleanup_claims_before_the_owned_task_is_scheduled() {
        let gate = Arc::new(RuntimeActivityGate::default());
        let spawner = Arc::new(QueuedSpawner::default());
        gate.bind(Arc::new(|| Ok(())), spawner.clone()).unwrap();
        let mut cleanup = gate.cleanup(Box::pin(async { Ok(()) }));
        assert!(futures_util::poll!(cleanup.as_mut()).is_pending());
        assert!(
            !gate.runtime_restore_idle(),
            "queued cleanup must already own its restore lease"
        );
        let task = spawner.tasks.lock().unwrap().pop().unwrap();
        task.await;
        cleanup.await.unwrap();
        assert!(gate.runtime_restore_idle());
    }
}
