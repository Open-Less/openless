//! Test-only observations of the real FIFO worker and its writer critical section.
//! Controlled writes substitute only the transport await, never the worker or clock.
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{watch, Semaphore};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::super::stream_send::{PacedSendClock, WsSendError};

tokio::task_local! {
    pub(super) static ACTIVE: Arc<Probe>;
}

#[derive(Clone)]
pub(super) struct Record {
    pub phase: &'static str,
    pub id: usize,
    pub end: bool,
    pub bytes: Vec<u8>,
    pub at: Instant,
    pub wall: std::time::Instant,
    pub deadline: Option<Instant>,
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} id={} end={} len={} fnv1a={:016x} at={:?} wall={:?} deadline={:?}",
            self.phase,
            self.id,
            self.end,
            self.bytes.len(),
            payload_tag(&self.bytes),
            self.at,
            self.wall,
            self.deadline,
        )
    }
}

pub(super) fn payload_tag(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Default)]
pub(super) struct Probe {
    records: Mutex<Vec<Record>>,
    current: Mutex<(usize, bool, Vec<u8>)>,
    controlled: AtomicBool,
    stall: Mutex<Option<(usize, Arc<Semaphore>)>>,
    fail: Mutex<Option<usize>>,
}

impl Probe {
    pub(super) fn record(&self, phase: &'static str, deadline: Option<Instant>) {
        let current = self.current.lock();
        self.records.lock().push(Record {
            phase,
            id: current.0,
            end: current.1,
            bytes: current.2.clone(),
            at: Instant::now(),
            wall: std::time::Instant::now(),
            deadline,
        });
    }

    pub(super) fn snapshot(&self) -> Vec<Record> {
        self.records.lock().clone()
    }
}

pub(super) fn frame(bytes: &[u8], end: bool) {
    ACTIVE.with(|probe| {
        {
            let mut current = probe.current.lock();
            current.0 += 1; // Stable 1-based FIFO id, including End.
            current.1 = end;
            current.2 = bytes.to_vec();
        }
        probe.record("Dequeued", None);
    });
}

pub(super) fn event(phase: &'static str) {
    ACTIVE.with(|probe| probe.record(phase, None));
}

pub(super) struct WorkerStopped;

impl Drop for WorkerStopped {
    fn drop(&mut self) {
        event("Stopped");
    }
}

pub(super) async fn wait_slot(
    clock: &PacedSendClock,
    cancel_rx: &mut watch::Receiver<bool>,
) -> Option<Instant> {
    let deadline = clock.diagnostic_deadline();
    ACTIVE.with(|probe| probe.record("Wait", deadline));
    let wait = clock.wait_slot(cancel_rx);
    tokio::pin!(wait);
    let mut observed_pending = false;
    let grant = std::future::poll_fn(|cx| {
        let polled = wait.as_mut().poll(cx);
        if polled.is_pending() && !observed_pending {
            ACTIVE.with(|probe| probe.record("WaitPending", deadline));
            observed_pending = true;
        }
        polled
    })
    .await;
    ACTIVE.with(|probe| probe.record("Grant", grant));
    grant
}

// Called only after the real SharedWriter mutex has been acquired and checked.
// The task-local is absent for other providers; no global observer is installed.
pub(in crate::asr) fn write_begin(message: &Message) {
    let _ = ACTIVE.try_with(|probe| {
        {
            let current = probe.current.lock();
            assert!(
                matches!(message, Message::Binary(bytes) if bytes.as_ref() == current.2.as_slice())
            );
        }
        probe.record("Begin", None);
    });
}

pub(in crate::asr) fn write_complete() {
    let _ = ACTIVE.try_with(|probe| probe.record("Complete", None));
}

pub(in crate::asr) async fn controlled_write(message: &Message) -> Option<Result<(), WsSendError>> {
    let probe = ACTIVE.try_with(Arc::clone).ok()?;
    if !probe.controlled.load(Ordering::Relaxed) {
        return None;
    }
    write_begin(message);
    let id = probe.current.lock().0;
    let gate = probe
        .stall
        .lock()
        .as_ref()
        .filter(|(frame, _)| *frame == id)
        .map(|(_, gate)| Arc::clone(gate));
    if let Some(gate) = gate {
        gate.acquire()
            .await
            .expect("controlled writer gate")
            .forget();
    }
    if *probe.fail.lock() == Some(id) {
        return Some(Err(WsSendError::Transport(
            "synthetic write failure".into(),
        )));
    }
    write_complete();
    Some(Ok(()))
}

#[cfg(test)]
mod tests {
    use std::task::Poll;
    use std::time::Duration;

    use futures_util::future::BoxFuture;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    use super::*;
    use crate::asr::xfyun::tests::{loopback_asr, spawn_loopback_ws, xfyun_result_json};
    use crate::asr::xfyun::{XfyunCredentials, XfyunStreamingASR, TARGET_AUDIO_CHUNK_BYTES};
    use crate::asr::AudioConsumer;
    use crate::config::{TaskSpawner, TokioTaskSpawner};

    // Poll just past each deadline, avoiding a dependency on timer readiness
    // exactly at its boundary. Planned slots still advance by exactly 40 ms.
    const SLOT_POLL_STEP: Duration = Duration::from_millis(41);

    // Capture the first spawn (the actual production FIFO future); receive and
    // close tasks still use Tokio. Tests decide exactly when the worker is polled.
    #[derive(Default)]
    struct ManualWorker {
        captured: AtomicBool,
        worker: Mutex<Option<BoxFuture<'static, ()>>>,
    }

    impl TaskSpawner for ManualWorker {
        fn spawn(&self, task: BoxFuture<'static, ()>) {
            if !self.captured.swap(true, Ordering::Relaxed) {
                *self.worker.lock() = Some(task);
            } else {
                TokioTaskSpawner.spawn(task);
            }
        }
    }

    async fn controlled() -> (
        Arc<XfyunStreamingASR>,
        BoxFuture<'static, ()>,
        tokio::task::JoinHandle<()>,
    ) {
        let (endpoint, server) = spawn_loopback_ws(|mut ws| async move {
            ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                .await
                .unwrap();
            while ws.next().await.is_some() {}
        })
        .await;
        let spawner = Arc::new(ManualWorker::default());
        let asr = Arc::new(
            XfyunStreamingASR::with_task_spawner(
                XfyunCredentials {
                    app_id: "test-app".into(),
                    api_key: "test-key".into(),
                },
                spawner.clone(),
            )
            .with_endpoint(endpoint),
        );
        asr.evidence.controlled.store(true, Ordering::Relaxed);
        asr.open_session().await.unwrap();
        let worker = spawner.worker.lock().take().unwrap();
        // Handshake uses real time; from here only the substituted transport
        // await and production clock are timed. No socket timing is inferred.
        tokio::time::pause();
        (asr, worker, server)
    }

    fn payload() -> Vec<u8> {
        (0..TARGET_AUDIO_CHUNK_BYTES * 3 + 400)
            .map(|i| (i % 251) as u8)
            .collect()
    }

    fn phase(probe: &Probe, name: &str) -> Vec<Record> {
        probe
            .snapshot()
            .into_iter()
            .filter(|record| record.phase == name)
            .collect()
    }

    fn assert_complete(probe: &Probe, expected: &[u8]) {
        let begins = phase(probe, "Begin");
        let completed = phase(probe, "Complete");
        assert_eq!(begins.len(), 5);
        assert_eq!(completed.len(), 5);
        let mut audio = Vec::new();
        for (i, (begin, complete)) in begins.iter().zip(&completed).enumerate() {
            assert_eq!(begin.id, i + 1);
            assert_eq!(complete.id, begin.id);
            assert_eq!(begin.bytes, complete.bytes);
            assert!(begin.at <= complete.at);
            assert_eq!(complete.end, i == 4);
            if !complete.end {
                audio.extend_from_slice(&complete.bytes);
            }
        }
        for frame in &completed[..3] {
            assert_eq!(frame.bytes.len(), TARGET_AUDIO_CHUNK_BYTES);
        }
        assert_eq!(completed[3].bytes.len(), 400);
        assert_eq!(audio, expected);
        assert_eq!(completed[4].bytes, br#"{"end": true}"#);
        assert_eq!(phase(probe, "Stopped").len(), 1);
        eprintln!("controlled FIFO trace: {:#?}", probe.snapshot());
    }

    async fn cleanup(asr: &XfyunStreamingASR, server: tokio::task::JoinHandle<()>) {
        tokio::time::resume();
        asr.cancel();
        server.abort();
        if let Err(error) = server.await {
            assert!(error.is_cancelled());
        }
    }

    #[tokio::test]
    async fn normal_full_frames_and_short_tail_characterization() {
        let (asr, mut worker, server) = controlled().await;
        let data = payload();
        asr.consume_pcm_chunk(&data);
        let finish = asr.send_last_frame();
        tokio::pin!(finish);
        assert!(futures_util::poll!(&mut finish).is_pending());
        assert!(futures_util::poll!(&mut worker).is_pending());
        let start = phase(&asr.evidence, "Begin")[0].at;
        for last in [false, false, true] {
            tokio::time::advance(SLOT_POLL_STEP).await;
            let polled = futures_util::poll!(&mut worker);
            assert_eq!(polled.is_ready(), last);
        }
        assert!(matches!(
            futures_util::poll!(&mut finish),
            Poll::Ready(Ok(()))
        ));
        let begins = phase(&asr.evidence, "Begin");
        for (record, ms) in begins.iter().zip([0, 41, 82, 123, 123]) {
            assert_eq!(record.at - start, Duration::from_millis(ms));
        }
        let grants = phase(&asr.evidence, "Grant");
        assert_eq!(grants.len(), 4);
        for (grant, ms) in grants.iter().zip([0, 40, 80, 120]) {
            assert_eq!(grant.deadline.unwrap() - start, Duration::from_millis(ms));
        }
        assert_complete(&asr.evidence, &data);
        assert!(asr.is_stream_complete());
        cleanup(&asr, server).await;
    }

    #[tokio::test]
    async fn late_wake_characterizes_current_sender_catch_up() {
        let (asr, mut worker, server) = controlled().await;
        let data = payload();
        asr.consume_pcm_chunk(&data);
        let finish = asr.send_last_frame();
        tokio::pin!(finish);
        assert!(futures_util::poll!(&mut finish).is_pending());
        assert!(futures_util::poll!(&mut worker).is_pending());
        let start = phase(&asr.evidence, "Begin")[0].at;
        let pending = phase(&asr.evidence, "WaitPending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, 2);
        assert_eq!(
            pending[0].deadline.unwrap() - start,
            Duration::from_millis(40)
        );
        // The real wait_slot was already Pending. Do not poll the worker at 40.
        tokio::time::advance(Duration::from_millis(90)).await;
        assert!(futures_util::poll!(&mut worker).is_pending());
        let begins = phase(&asr.evidence, "Begin");
        assert_eq!(begins.len(), 3, "three full frames, tail still waiting");
        assert_eq!(begins[1].at - start, Duration::from_millis(90));
        assert_eq!(begins[2].at, begins[1].at);
        tokio::time::advance(SLOT_POLL_STEP).await;
        assert!(futures_util::poll!(&mut worker).is_ready());
        assert!(matches!(
            futures_util::poll!(&mut finish),
            Poll::Ready(Ok(()))
        ));
        let grants = phase(&asr.evidence, "Grant");
        assert_eq!(grants.len(), 4);
        assert_eq!(
            grants[1].deadline.unwrap() - start,
            Duration::from_millis(40)
        );
        assert_eq!(grants[1].at - start, Duration::from_millis(90));
        assert_complete(&asr.evidence, &data);
        cleanup(&asr, server).await;
    }

    #[tokio::test]
    async fn slow_write_characterizes_current_sender_catch_up() {
        let (asr, mut worker, server) = controlled().await;
        let gate = Arc::new(Semaphore::new(0));
        *asr.evidence.stall.lock() = Some((2, gate.clone()));
        let data = payload();
        asr.consume_pcm_chunk(&data);
        let finish = asr.send_last_frame();
        tokio::pin!(finish);
        assert!(futures_util::poll!(&mut finish).is_pending());
        assert!(futures_util::poll!(&mut worker).is_pending());
        let start = phase(&asr.evidence, "Begin")[0].at;
        tokio::time::advance(SLOT_POLL_STEP).await;
        assert!(futures_util::poll!(&mut worker).is_pending());
        assert_eq!(
            phase(&asr.evidence, "Begin")[1].at - start,
            Duration::from_millis(41)
        );
        assert_eq!(phase(&asr.evidence, "Complete").len(), 1);
        assert!(
            asr.writer.try_lock().is_err(),
            "slow write holds the real writer"
        );
        tokio::time::advance(Duration::from_millis(90)).await;
        gate.add_permits(1);
        assert!(futures_util::poll!(&mut worker).is_pending());
        let completed = phase(&asr.evidence, "Complete");
        let begins = phase(&asr.evidence, "Begin");
        let grants = phase(&asr.evidence, "Grant");
        assert_eq!(grants.len(), 3, "tail has not obtained a slot yet");
        assert_eq!(grants[1].at - start, Duration::from_millis(41));
        assert_eq!(
            grants[1].deadline.unwrap() - start,
            Duration::from_millis(40)
        );
        assert_eq!(completed[1].at - start, Duration::from_millis(131));
        assert_eq!(begins[2].at, completed[1].at);
        assert_eq!(begins[2].bytes.len(), TARGET_AUDIO_CHUNK_BYTES);
        tokio::time::advance(SLOT_POLL_STEP).await;
        assert!(futures_util::poll!(&mut worker).is_ready());
        assert!(matches!(
            futures_util::poll!(&mut finish),
            Poll::Ready(Ok(()))
        ));
        assert_complete(&asr.evidence, &data);
        cleanup(&asr, server).await;
    }

    async fn interrupted(cancel: bool, during_write: bool) {
        let (asr, mut worker, server) = controlled().await;
        let gate = Arc::new(Semaphore::new(0));
        if during_write {
            *asr.evidence.stall.lock() = Some((2, gate.clone()));
        }
        if !cancel {
            *asr.evidence.fail.lock() = Some(2);
        }
        let data = payload();
        asr.consume_pcm_chunk(&data);
        let finish = asr.send_last_frame();
        tokio::pin!(finish);
        assert!(futures_util::poll!(&mut finish).is_pending());
        assert!(futures_util::poll!(&mut worker).is_pending());
        if during_write || !cancel {
            tokio::time::advance(SLOT_POLL_STEP).await;
        }
        if during_write {
            assert!(futures_util::poll!(&mut worker).is_pending());
            assert!(asr.writer.try_lock().is_err());
        }
        if cancel {
            asr.cancel();
        }
        assert!(futures_util::poll!(&mut worker).is_ready());
        assert!(matches!(
            futures_util::poll!(&mut finish),
            Poll::Ready(Err(_))
        ));
        gate.add_permits(1);
        tokio::time::advance(Duration::from_millis(200)).await;
        let completed = phase(&asr.evidence, "Complete");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, 1);
        assert_eq!(completed[0].bytes, data[..TARGET_AUDIO_CHUNK_BYTES]);
        let begins = phase(&asr.evidence, "Begin");
        assert_eq!(begins.len(), if cancel && !during_write { 1 } else { 2 });
        assert!(begins.iter().all(|r| r.id <= 2 && !r.end));
        assert_eq!(phase(&asr.evidence, "Dequeued").len(), 2);
        assert_eq!(
            phase(&asr.evidence, "Failed").len(),
            usize::from(!cancel || during_write)
        );
        assert_eq!(phase(&asr.evidence, "Stopped").len(), 1);
        assert_eq!(phase(&asr.evidence, "Cancel").is_empty(), !cancel);
        assert!(!asr.is_stream_complete());
        assert_eq!(
            asr.audio_coverage(),
            (TARGET_AUDIO_CHUNK_BYTES as u64, data.len() as u64)
        );
        assert!(asr.send_last_frame().await.is_err());
        eprintln!("interrupted FIFO trace: {:#?}", asr.evidence.snapshot());
        cleanup(&asr, server).await;
    }

    #[tokio::test]
    async fn cancel_during_slot_does_not_continue_or_send_end() {
        interrupted(true, false).await;
    }

    #[tokio::test]
    async fn cancel_during_write_does_not_continue_or_send_end() {
        interrupted(true, true).await;
    }

    #[tokio::test]
    async fn failed_write_does_not_continue_or_send_end() {
        interrupted(false, false).await;
    }

    #[tokio::test]
    async fn receiver_barrier_distinguishes_reads_from_sends() {
        // Real socket + real clock only. Compare an ordinary reader with one
        // released strictly after End completion, without per-frame acks.
        for pause_reader in [false, true] {
            let gate = Arc::new(Semaphore::new(0));
            let gate_server = gate.clone();
            let reads = Arc::new(Mutex::new(Vec::new()));
            let reads_server = reads.clone();
            let (endpoint, server) = spawn_loopback_ws(move |mut ws| async move {
                ws.send(Message::Text(r#"{"action":"started","code":"0"}"#.into()))
                    .await
                    .unwrap();
                if pause_reader {
                    gate_server.acquire().await.unwrap().forget();
                }
                while let Some(Ok(message)) = ws.next().await {
                    if let Message::Binary(data) = message {
                        let end = data.as_ref() == br#"{"end": true}"#;
                        reads_server
                            .lock()
                            .push((std::time::Instant::now(), data.to_vec()));
                        if end {
                            ws.send(Message::Text(xfyun_result_json("synthetic", true).into()))
                                .await
                                .unwrap();
                            let _ = ws.close(None).await;
                            break;
                        }
                    }
                }
            })
            .await;
            let asr = loopback_asr(endpoint);
            asr.open_session().await.unwrap();
            let data: Vec<_> = (0..TARGET_AUDIO_CHUNK_BYTES * 5 + 400)
                .map(|i| (i % 251) as u8)
                .collect();
            asr.consume_pcm_chunk(&data);
            asr.send_last_frame().await.unwrap();
            let completed = phase(&asr.evidence, "Complete");
            assert_eq!(completed.len(), 7);
            if pause_reader {
                assert!(
                    reads.lock().is_empty(),
                    "receiver barrier must really be closed"
                );
                gate.add_permits(1);
            }
            assert_eq!(asr.await_final_result().await.unwrap().text, "synthetic");
            server.await.unwrap();
            assert_eq!(phase(&asr.evidence, "Stopped").len(), 1);
            assert_eq!(phase(&asr.evidence, "Complete").len(), 7);
            let reads = reads.lock();
            assert_eq!(reads.len(), 7);
            let mut audio = Vec::new();
            for (i, ((read_at, bytes), sent)) in reads.iter().zip(&completed).enumerate() {
                assert_eq!(sent.id, i + 1);
                assert_eq!(bytes, &sent.bytes);
                assert_eq!(sent.end, i == 6);
                if i < 6 {
                    assert_eq!(
                        bytes.len(),
                        if i == 5 {
                            400
                        } else {
                            TARGET_AUDIO_CHUNK_BYTES
                        }
                    );
                    audio.extend_from_slice(bytes);
                }
                if pause_reader {
                    assert!(*read_at >= completed[6].wall);
                }
            }
            assert_eq!(audio, data);
            assert_eq!(reads[6].1, br#"{"end": true}"#);
            // Causal proof, independent of receiver gap thresholds: all grants,
            // begins and completions occurred before the first paused read.
            // Moving the read barrier changes the observation window without
            // feeding any timing signal back to the sender.
            let begins = phase(&asr.evidence, "Begin");
            assert_eq!(begins.len(), 7);
            if pause_reader {
                assert!(begins.iter().all(|r| r.wall <= reads[0].0));
                assert!(phase(&asr.evidence, "Grant")
                    .iter()
                    .all(|r| r.wall <= reads[0].0));
                assert!(begins[5].at > begins[0].at);
            }
            let read_trace: Vec<_> = reads
                .iter()
                .enumerate()
                .map(|(i, (at, bytes))| (i + 1, at, bytes.len(), payload_tag(bytes)))
                .collect();
            eprintln!(
                "pause_reader={pause_reader}; sender={:#?}; receiver={read_trace:#?}",
                asr.evidence.snapshot()
            );
        }
    }
}
