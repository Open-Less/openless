//! Serial macOS keyboard posting with one terminal AX delivery barrier.
//!
//! AX references are created, used and released on the worker thread. The
//! cloneable handle contains only channels and completion state, never raw AX
//! pointers. Once a write is queued, dropping its future does not revoke it;
//! callers must await `finish` before restoring TIS, including on cancellation.

#![cfg(target_os = "macos")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};

use openless_core::ports::InsertWriteResult;
use openless_core::{BackendError, BackendErrorCode};
use parking_lot::Mutex;
use tokio::sync::{oneshot, Notify};

use crate::host_document::{KeyboardDelivery, KeyboardDeliveryOutcome};
use crate::selection::SelectionInsertionTarget;
use crate::types::MacosNewlineMode;

#[derive(Clone)]
pub(crate) struct MacStreamingInput {
    inner: Arc<WorkerHandle>,
}

struct WorkerHandle {
    // Taking the sender seals the queue atomically with respect to new writes.
    commands: Mutex<Option<mpsc::Sender<Command>>>,
    completion: Arc<Completion>,
    closed: Arc<AtomicBool>,
}

enum Command {
    Write {
        text: String,
        reply: oneshot::Sender<Result<InsertWriteResult, BackendError>>,
    },
    Finish,
}

#[derive(Default)]
struct Completion {
    result: OnceLock<Result<(), BackendError>>,
    ready: Notify,
}

impl Completion {
    fn settle(&self, result: Result<(), BackendError>) {
        if self.result.set(result).is_ok() {
            self.ready.notify_waiters();
        }
    }

    async fn wait(&self) -> Result<(), BackendError> {
        loop {
            let notified = self.ready.notified();
            if let Some(result) = self.result.get() {
                return result.clone();
            }
            notified.await;
        }
    }
}

// A panic or an unexpectedly abandoned channel must wake all terminal waiters.
struct WorkerExit(Arc<Completion>);

impl Drop for WorkerExit {
    fn drop(&mut self) {
        self.0.settle(Err(worker_stopped()));
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        // Best effort cleanup when the last handle goes away. This cannot
        // replace awaiting finish before restoring the caller's input source.
        if let Some(sender) = self.commands.get_mut().take() {
            if sender.send(Command::Finish).is_err() {
                self.completion.settle(Err(worker_stopped()));
            }
        }
    }
}

impl MacStreamingInput {
    pub(crate) fn spawn(
        target: SelectionInsertionTarget,
        newline_mode: MacosNewlineMode,
        closed: Arc<AtomicBool>,
    ) -> Result<Self, BackendError> {
        Self::spawn_with_backend(
            move || NativeInput {
                target,
                newline_mode,
                submitted_return: false,
            },
            closed,
        )
    }

    // B need not be Send: it is constructed and destroyed inside this thread.
    fn spawn_with_backend<B: InputBackend + 'static>(
        create: impl FnOnce() -> B + Send + 'static,
        closed: Arc<AtomicBool>,
    ) -> Result<Self, BackendError> {
        let (sender, receiver) = mpsc::channel();
        let completion = Arc::new(Completion::default());
        let worker_completion = Arc::clone(&completion);
        let worker_closed = Arc::clone(&closed);
        std::thread::Builder::new()
            .name("openless-macos-stream-input".into())
            .spawn(move || {
                let _exit = WorkerExit(Arc::clone(&worker_completion));
                let mut input = InputState::new(create());
                let mut failed: Option<BackendError> = None;
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Write { text, reply } => {
                            let result = if let Some(error) = &failed {
                                Err(error.clone())
                            } else {
                                input.write(&text, &worker_closed)
                            };
                            match &result {
                                Err(error) => failed = Some(error.clone()),
                                Ok(result) if result.written_chars < text.chars().count() => {
                                    failed = Some(BackendError::new(
                                        BackendErrorCode::Platform,
                                        "native keyboard insertion stopped after a partial write",
                                    ));
                                }
                                _ => {}
                            }
                            // A dropped receiver does not abandon a committed
                            // native write or the terminal delivery barrier.
                            let _ = reply.send(result);
                        }
                        Command::Finish => break,
                    }
                }
                // Previous write failures were reported to Core already. Its
                // clipboard reconciliation must still be able to finish cleanup.
                let result = input.finish();
                worker_completion.settle(result);
            })
            .map_err(|error| {
                BackendError::new(
                    BackendErrorCode::Platform,
                    format!("start macOS streaming input worker: {error}"),
                )
            })?;
        Ok(Self {
            inner: Arc::new(WorkerHandle {
                commands: Mutex::new(Some(sender)),
                completion,
                closed,
            }),
        })
    }

    pub(crate) async fn write(&self, text: String) -> Result<InsertWriteResult, BackendError> {
        let (reply, receiver) = oneshot::channel();
        {
            let commands = self.inner.commands.lock();
            if self.inner.closed.load(Ordering::Acquire) {
                return Err(input_closed());
            }
            let sender = commands.as_ref().ok_or_else(input_closed)?;
            sender
                .send(Command::Write { text, reply })
                .map_err(|_| worker_stopped())?;
        }
        receiver.await.map_err(|_| worker_stopped())?
    }

    /// Seal writes, drain everything already posted, then confirm the cumulative
    /// caret. Concurrent or cancelled finish callers share one terminal result.
    /// The caller may set `closed` to cancel not-yet-started writes, but finish
    /// itself does not cancel writes that were accepted before this barrier.
    pub(crate) async fn finish(&self) -> Result<(), BackendError> {
        {
            if let Some(sender) = self.inner.commands.lock().take() {
                if sender.send(Command::Finish).is_err() {
                    self.inner.completion.settle(Err(worker_stopped()));
                }
            }
        }
        self.inner.completion.wait().await
    }
}

fn input_closed() -> BackendError {
    BackendError::new(
        BackendErrorCode::Cancelled,
        "macOS streaming insertion is closed",
    )
}

fn worker_stopped() -> BackendError {
    BackendError::new(
        BackendErrorCode::Internal,
        "macOS streaming input worker stopped",
    )
}

/// Only native side effects are replaceable in tests; queueing, target guards,
/// prefix accounting and terminal ordering exercise the production code.
trait InputBackend {
    type Delivery;
    fn restore_target(&mut self) -> bool;
    fn capture_delivery(&mut self) -> Option<Self::Delivery>;
    fn is_focused(&mut self, delivery: &Self::Delivery) -> bool;
    fn post(&mut self, text: &str) -> usize;
    fn record_posted(&mut self, delivery: &mut Self::Delivery, posted: &str);
    fn finish_delivery(&mut self, delivery: Self::Delivery) -> Result<(), BackendError>;
}

struct InputState<B: InputBackend> {
    backend: B,
    captured: bool,
    delivery: Option<B::Delivery>,
}

impl<B: InputBackend> InputState<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            captured: false,
            delivery: None,
        }
    }

    fn write(
        &mut self,
        text: &str,
        closed: &AtomicBool,
    ) -> Result<InsertWriteResult, BackendError> {
        if closed.load(Ordering::Acquire) {
            return Err(input_closed());
        }
        if text.is_empty() {
            return Ok(InsertWriteResult { written_chars: 0 });
        }
        if !self.backend.restore_target() {
            return Err(BackendError::new(
                BackendErrorCode::Platform,
                "original text insertion target is unavailable",
            ));
        }
        if !self.captured {
            self.delivery = self.backend.capture_delivery();
            self.captured = true;
        }
        if self
            .delivery
            .as_ref()
            .is_some_and(|delivery| !self.backend.is_focused(delivery))
        {
            return Err(BackendError::new(
                BackendErrorCode::Platform,
                "original text insertion control lost focus",
            ));
        }
        // Restoring focus / AX capture can take time. A cancellation that won
        // during those operations must not start posting a fresh batch.
        if closed.load(Ordering::Acquire) {
            return Err(input_closed());
        }
        let written_chars = self.backend.post(text);
        if let Some(delivery) = self.delivery.as_mut() {
            let posted: String = text.chars().take(written_chars).collect();
            self.backend.record_posted(delivery, &posted);
        }
        Ok(InsertWriteResult { written_chars })
    }

    fn finish(mut self) -> Result<(), BackendError> {
        match self.delivery.take() {
            Some(delivery) => self.backend.finish_delivery(delivery),
            None => Ok(()),
        }
    }
}

struct NativeInput {
    target: SelectionInsertionTarget,
    newline_mode: MacosNewlineMode,
    submitted_return: bool,
}

impl InputBackend for NativeInput {
    type Delivery = KeyboardDelivery;

    fn restore_target(&mut self) -> bool {
        crate::selection::reactivate_selection_insertion_target(&self.target)
    }

    fn capture_delivery(&mut self) -> Option<Self::Delivery> {
        KeyboardDelivery::capture()
    }

    fn is_focused(&mut self, delivery: &Self::Delivery) -> bool {
        // Explicit Return mode can submit/replace the focused control. Preserve
        // its existing posted-input fallback after an actual submitting key;
        // the original application check still runs before every later batch.
        self.submitted_return || delivery.is_focused()
    }

    fn post(&mut self, text: &str) -> usize {
        match crate::unicode_keystroke::type_unicode_chunk_with_options(text, self.newline_mode) {
            Ok(written) => written,
            Err(error) => {
                let written = error.typed_chars();
                log::warn!("[insertion] keyboard posting failed after {written} chars: {error}");
                written
            }
        }
    }

    fn record_posted(&mut self, delivery: &mut Self::Delivery, posted: &str) {
        delivery.record_posted(posted, self.newline_mode);
        if self.newline_mode == MacosNewlineMode::Return && posted.contains('\n') {
            self.submitted_return = true;
        }
    }

    fn finish_delivery(&mut self, delivery: Self::Delivery) -> Result<(), BackendError> {
        let outcome = delivery.finish();
        if outcome != KeyboardDeliveryOutcome::Delivered {
            log::warn!("[insertion] using posted-input completion; caret outcome={outcome:?}");
        }
        // Unsupported/stalled AX targets already use posting completion. Never
        // turn an unobservable caret into a retry/paste of text already sent.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::time::Duration;

    struct FakeInput {
        events: mpsc::Sender<String>,
        restore: bool,
        focused: bool,
        observable: bool,
        limit: usize,
        post_gate: Option<mpsc::Receiver<()>>,
        finish_gate: Option<mpsc::Receiver<()>>,
        cancel_during_restore: Option<Arc<AtomicBool>>,
    }

    impl FakeInput {
        fn new(events: mpsc::Sender<String>) -> Self {
            Self {
                events,
                restore: true,
                focused: true,
                observable: true,
                limit: usize::MAX,
                post_gate: None,
                finish_gate: None,
                cancel_during_restore: None,
            }
        }
    }

    // Rc deliberately makes the receipt !Send, like the real AX reference.
    impl InputBackend for FakeInput {
        type Delivery = (Rc<()>, String);
        fn restore_target(&mut self) -> bool {
            if let Some(closed) = &self.cancel_during_restore {
                closed.store(true, Ordering::Release);
            }
            self.restore
        }
        fn capture_delivery(&mut self) -> Option<Self::Delivery> {
            self.events.send("capture".into()).unwrap();
            self.observable.then(|| (Rc::new(()), String::new()))
        }
        fn is_focused(&mut self, _: &Self::Delivery) -> bool {
            self.focused
        }
        fn post(&mut self, text: &str) -> usize {
            self.events.send(format!("post:{text}")).unwrap();
            if let Some(gate) = self.post_gate.take() {
                gate.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            text.chars().count().min(self.limit)
        }
        fn record_posted(&mut self, delivery: &mut Self::Delivery, text: &str) {
            delivery.1.push_str(text);
        }
        fn finish_delivery(&mut self, delivery: Self::Delivery) -> Result<(), BackendError> {
            self.events.send(format!("ack:{}", delivery.1)).unwrap();
            if let Some(gate) = self.finish_gate.take() {
                gate.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            self.events.send("finished".into()).unwrap();
            Ok(())
        }
    }

    fn event(events: &mpsc::Receiver<String>) -> String {
        events.recv_timeout(Duration::from_secs(2)).unwrap()
    }

    #[tokio::test]
    async fn writes_do_not_wait_for_ack_and_finish_waits_for_the_combined_receipt() {
        let (tx, events) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let input = MacStreamingInput::spawn_with_backend(
            move || {
                let mut backend = FakeInput::new(tx);
                backend.finish_gate = Some(gate);
                backend
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(input.write("A🙂".into()).await.unwrap().written_chars, 2);
        assert_eq!(input.write("B".into()).await.unwrap().written_chars, 1);
        assert_eq!(event(&events), "capture");
        assert_eq!(event(&events), "post:A🙂");
        assert_eq!(event(&events), "post:B");
        assert!(events.try_recv().is_err());
        let mut finish = std::pin::pin!(input.finish());
        assert!(futures_util::poll!(finish.as_mut()).is_pending());
        assert_eq!(event(&events), "ack:A🙂B");
        assert!(futures_util::poll!(finish.as_mut()).is_pending());
        release.send(()).unwrap();
        finish.await.unwrap();
        input.finish().await.unwrap();
        assert_eq!(event(&events), "finished");
        assert_eq!(
            input.write("late".into()).await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );
    }

    #[tokio::test]
    async fn a_dropped_write_and_finish_future_do_not_abandon_queued_input() {
        let (tx, events) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let input = MacStreamingInput::spawn_with_backend(
            move || {
                let mut backend = FakeInput::new(tx);
                backend.post_gate = Some(gate);
                backend
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        {
            let mut write = std::pin::pin!(input.write("committed".into()));
            assert!(futures_util::poll!(write.as_mut()).is_pending());
            assert_eq!(event(&events), "capture");
            assert_eq!(event(&events), "post:committed");
        }
        {
            let mut finish = std::pin::pin!(input.finish());
            assert!(futures_util::poll!(finish.as_mut()).is_pending());
        }
        release.send(()).unwrap();
        input.finish().await.unwrap();
        assert_eq!(event(&events), "ack:committed");
        assert_eq!(event(&events), "finished");
    }

    #[tokio::test]
    async fn cancellation_rejects_queued_writes_but_drains_the_started_batch() {
        let (tx, events) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let input = MacStreamingInput::spawn_with_backend(
            move || {
                let mut backend = FakeInput::new(tx);
                backend.post_gate = Some(gate);
                backend
            },
            Arc::clone(&closed),
        )
        .unwrap();
        let mut first = std::pin::pin!(input.write("first".into()));
        assert!(futures_util::poll!(first.as_mut()).is_pending());
        assert_eq!(event(&events), "capture");
        assert_eq!(event(&events), "post:first");
        let mut second = std::pin::pin!(input.write("cancelled".into()));
        assert!(futures_util::poll!(second.as_mut()).is_pending());
        closed.store(true, Ordering::Release);
        let mut finish = std::pin::pin!(input.finish());
        assert!(futures_util::poll!(finish.as_mut()).is_pending());
        release.send(()).unwrap();
        assert_eq!(first.await.unwrap().written_chars, 5);
        assert_eq!(second.await.unwrap_err().code, BackendErrorCode::Cancelled);
        finish.await.unwrap();
        assert_eq!(event(&events), "ack:first");
        assert_eq!(event(&events), "finished");
    }

    #[tokio::test]
    async fn target_failure_prevents_posting_and_does_not_block_cleanup() {
        for restore_failure in [true, false] {
            let (tx, events) = mpsc::channel();
            let input = MacStreamingInput::spawn_with_backend(
                move || {
                    let mut backend = FakeInput::new(tx);
                    backend.restore = !restore_failure;
                    backend.focused = false;
                    backend
                },
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            assert_eq!(
                input.write("must not post".into()).await.unwrap_err().code,
                BackendErrorCode::Platform
            );
            input.finish().await.unwrap();
            assert!(events.try_iter().all(|event| !event.starts_with("post:")));
        }
    }

    #[tokio::test]
    async fn cancellation_during_target_restore_is_checked_before_posting() {
        let (tx, events) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&closed);
        let input = MacStreamingInput::spawn_with_backend(
            move || {
                let mut backend = FakeInput::new(tx);
                backend.cancel_during_restore = Some(cancel);
                backend
            },
            closed,
        )
        .unwrap();
        assert_eq!(
            input.write("must not post".into()).await.unwrap_err().code,
            BackendErrorCode::Cancelled
        );
        input.finish().await.unwrap();
        assert!(events.try_iter().all(|event| !event.starts_with("post:")));
    }

    #[tokio::test]
    async fn unobservable_targets_keep_posting_fallback_without_recapturing() {
        let (tx, events) = mpsc::channel();
        let input = MacStreamingInput::spawn_with_backend(
            move || {
                let mut backend = FakeInput::new(tx);
                backend.observable = false;
                backend
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        input.write("one".into()).await.unwrap();
        input.write("two".into()).await.unwrap();
        input.finish().await.unwrap();
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            ["capture", "post:one", "post:two"]
        );
    }

    #[tokio::test]
    async fn partial_writes_account_only_for_the_posted_prefix_and_stop_later_input() {
        let (tx, events) = mpsc::channel();
        let input = MacStreamingInput::spawn_with_backend(
            move || {
                let mut backend = FakeInput::new(tx);
                backend.limit = 2;
                backend
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(input.write("A🙂B".into()).await.unwrap().written_chars, 2);
        assert!(input.write("duplicate".into()).await.is_err());
        input.finish().await.unwrap();
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            ["capture", "post:A🙂B", "ack:A🙂", "finished"]
        );
    }

    #[tokio::test]
    async fn last_handle_drop_closes_the_queue_and_drains_its_receipt() {
        let (tx, events) = mpsc::channel();
        let input = MacStreamingInput::spawn_with_backend(
            move || FakeInput::new(tx),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        input.write("done".into()).await.unwrap();
        let completion = Arc::clone(&input.inner.completion);
        drop(input);
        completion.wait().await.unwrap();
        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            ["capture", "post:done", "ack:done", "finished"]
        );
    }

    #[tokio::test]
    async fn worker_start_panic_wakes_write_and_finish_waiters() {
        let input = MacStreamingInput::spawn_with_backend(
            || -> FakeInput { panic!("test worker initialization failure") },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(
            input.write("not posted".into()).await.unwrap_err().code,
            BackendErrorCode::Internal
        );
        assert_eq!(
            input.finish().await.unwrap_err().code,
            BackendErrorCode::Internal
        );
    }
}
