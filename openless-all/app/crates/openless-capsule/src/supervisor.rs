//! Event-driven, bounded helper supervision. No child or pipe operations occur
//! on the microphone callback or the native UI thread.
use crate::{Action, Frame, HostMessage, UiMessage, MAX_LINE_BYTES, PROTOCOL_VERSION};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::watch,
};

pub struct CapsuleSupervisor {
    frames: watch::Sender<Option<Frame>>,
    error: watch::Receiver<Option<String>>,
}
impl CapsuleSupervisor {
    pub fn spawn(
        handle: &tokio::runtime::Handle,
        executable: std::path::PathBuf,
        on_action: Arc<dyn Fn(String, Action) + Send + Sync>,
        on_health_change: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (frames, receiver) = watch::channel(None);
        let (status, error) = watch::channel(None);
        let on_health: HealthCallback = Arc::new(move |error| {
            if status.send_if_modified(|current| {
                if *current == error {
                    return false;
                }
                *current = error;
                true
            }) {
                on_health_change();
            }
        });
        handle.spawn(supervise(executable, receiver, on_action, on_health));
        Self { frames, error }
    }
    pub fn last_error(&self) -> Option<String> {
        self.error.borrow().clone()
    }
    /// Coalesce volume/transcript frames; never allocate an unbounded queue.
    pub fn present(&self, frame: Frame) {
        self.frames.send_replace(frame.visible().then_some(frame));
    }
    pub fn stop(&self) {
        self.frames.send_replace(None);
    }
}

type HealthCallback = Arc<dyn Fn(Option<String>) + Send + Sync>;

// Keep partial input across select cancellation by newer volume frames.
async fn read_ui(
    reader: &mut (impl AsyncBufRead + Unpin),
    bytes: &mut Vec<u8>,
) -> io::Result<Option<UiMessage>> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete capsule message",
                ))
            };
        }
        let end = chunk.iter().position(|byte| *byte == b'\n').map(|i| i + 1);
        let count = end.unwrap_or(chunk.len());
        if bytes.len() + count > MAX_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized capsule message",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if end.is_some() {
            break;
        }
    }
    let result = serde_json::from_slice(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    bytes.clear();
    result
}

async fn send(writer: &mut tokio::process::ChildStdin, message: HostMessage) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&message)?;
    if bytes.len() >= MAX_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized capsule frame",
        ));
    }
    bytes.push(b'\n');
    // A crashed/hung renderer cannot keep a pipe write alive indefinitely.
    tokio::time::timeout(Duration::from_secs(1), writer.write_all(&bytes)).await??;
    Ok(())
}

async fn run_instance(
    command: &mut Command,
    frames: &mut watch::Receiver<Option<Frame>>,
    on_action: &Arc<dyn Fn(String, Action) + Send + Sync>,
    on_health: &HealthCallback,
) -> io::Result<()> {
    run_instance_with_handshake_timeout(
        command,
        frames,
        on_action,
        on_health,
        Duration::from_secs(5),
    )
    .await
}

async fn run_instance_with_handshake_timeout(
    command: &mut Command,
    frames: &mut watch::Receiver<Option<Frame>>,
    on_action: &Arc<dyn Fn(String, Action) + Send + Sync>,
    on_health: &HealthCallback,
    handshake_timeout: Duration,
) -> io::Result<()> {
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW: no console flash.
    let mut child = command.spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("missing capsule stdin"))?;
    let mut output = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing capsule stdout"))?,
    );
    let mut ready = false;
    let mut message_bytes = Vec::new();
    let deadline = tokio::time::sleep(handshake_timeout);
    tokio::pin!(deadline);
    let result = async {
        loop {
            tokio::select! {
                biased;
                changed = frames.changed() => {
                    if changed.is_err() || frames.borrow().is_none() { return Ok(()); }
                    if ready {
                        let frame = frames.borrow_and_update().clone();
                        if let Some(frame) = frame { send(&mut input, HostMessage::Frame { version: PROTOCOL_VERSION, frame: Box::new(frame) }).await?; }
                    }
                }
                message = read_ui(&mut output, &mut message_bytes) => {
                    match message? {
                        Some(UiMessage::Ready { version }) if version == PROTOCOL_VERSION && !ready => {
                            ready = true;
                            on_health(None);
                            let frame = frames.borrow_and_update().clone();
                            if let Some(frame) = frame { send(&mut input, HostMessage::Frame { version: PROTOCOL_VERSION, frame: Box::new(frame) }).await?; }
                            else { return Ok(()); }
                        }
                        Some(UiMessage::Action { version, session_id, sequence, action }) if version == PROTOCOL_VERSION && ready => {
                            let accepted = frames.borrow().as_ref().is_some_and(|frame| frame.accepts(&session_id, sequence, action));
                            if accepted { on_action(session_id, action); }
                        }
                        None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "capsule helper exited")),
                        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "capsule protocol mismatch")),
                    }
                }
                _ = &mut deadline, if !ready => return Err(io::Error::new(io::ErrorKind::TimedOut, "capsule handshake timed out")),
            }
        }
    }.await;
    // Always reap before starting the next instance; a late old helper cannot
    // overlap a new helper or retain its graphics resources.
    let _ = send(
        &mut input,
        HostMessage::Shutdown {
            version: PROTOCOL_VERSION,
        },
    )
    .await;
    drop(input);
    if tokio::time::timeout(Duration::from_millis(300), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

async fn supervise(
    executable: std::path::PathBuf,
    mut frames: watch::Receiver<Option<Frame>>,
    on_action: Arc<dyn Fn(String, Action) + Send + Sync>,
    on_health: HealthCallback,
) {
    loop {
        while frames.borrow().is_none() {
            if frames.changed().await.is_err() {
                return;
            }
        }
        let session = frames
            .borrow_and_update()
            .as_ref()
            .map(|frame| frame.session_id.clone());
        let mut failed = true;
        // A continuous stream of volume updates must not cause a crash loop.
        for attempt in 0..2 {
            match run_instance(
                &mut Command::new(&executable),
                &mut frames,
                &on_action,
                &on_health,
            )
            .await
            {
                Ok(()) => {
                    failed = false;
                    break;
                }
                Err(error) => {
                    log::warn!("[native-capsule] helper failed: {error}");
                    on_health(Some(error.to_string()));
                }
            }
            if attempt == 0 {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if frames.borrow().as_ref().map(|frame| &frame.session_id) != session.as_ref() {
                    break;
                }
            }
        }
        if frames.has_changed().is_err() {
            return;
        }
        if failed {
            while frames.borrow().as_ref().map(|frame| &frame.session_id) == session.as_ref() {
                if frames.changed().await.is_err() {
                    return;
                }
            }
        }
        // Inspect the current value directly. A new session may have arrived
        // while we were reaping the previous child; waiting for another change
        // here would lose that session's first (and sometimes only) frame.
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn recording(session: &str, sequence: u64) -> Frame {
        Frame {
            session_id: session.into(),
            sequence,
            state: crate::CapsulePopupState {
                phase: "recording".into(),
                ..Default::default()
            },
            transcript: String::new(),
            transcript_font_size: 15.0,
            labels: Default::default(),
            show_transcript: false,
            theme: Default::default(),
            leaving: false,
            bounds: None,
            interactive: true,
        }
    }

    #[tokio::test]
    async fn helper_replays_the_latest_frame_and_rejects_another_sessions_action() {
        use std::os::unix::fs::PermissionsExt;
        let script = std::env::temp_dir().join(format!(
            "openless-capsule-test-{}-{}.sh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&script, r#"#!/bin/sh
printf '%s\n' '{"type":"ready","version":2}'
while IFS= read -r frame; do
  case "$frame" in
    *'"shutdown"'*) exit 0;;
    *'"frame"'*)
      printf '%s\n' '{"type":"action","version":2,"session_id":"old","sequence":7,"action":"cancel"}'
      printf '%s\n' '{"type":"action","version":2,"session_id":"current","sequence":7,"action":"confirm"}'
      ;;
  esac
done
"#).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (tx, mut rx) = watch::channel(Some(recording("old", 1)));
        tx.send_replace(Some(recording("current", 7)));
        let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel();
        let callback: Arc<dyn Fn(String, Action) + Send + Sync> =
            Arc::new(move |session, action| {
                let _ = action_tx.send((session, action));
            });
        let path = script.clone();
        let task = tokio::spawn(async move {
            run_instance(
                &mut Command::new(&path),
                &mut rx,
                &callback,
                &(Arc::new(|_| {}) as HealthCallback),
            )
            .await
        });
        let received = tokio::time::timeout(Duration::from_secs(2), action_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, ("current".into(), Action::Confirm));
        assert!(
            action_rx.try_recv().is_err(),
            "old-session cancel must be rejected"
        );
        tx.send_replace(None);
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
        std::fs::remove_file(script).unwrap();
    }

    #[tokio::test]
    async fn idle_cancels_a_helper_before_the_ready_handshake() {
        let (tx, mut rx) = watch::channel(Some(Frame {
            session_id: "one".into(),
            sequence: 1,
            state: crate::CapsulePopupState {
                phase: "recording".into(),
                ..Default::default()
            },
            transcript: String::new(),
            transcript_font_size: 15.0,
            labels: Default::default(),
            show_transcript: false,
            theme: Default::default(),
            leaving: false,
            bounds: None,
            interactive: false,
        }));
        let action: Arc<dyn Fn(String, Action) + Send + Sync> =
            Arc::new(|_, _| panic!("no UI action expected"));
        let task = tokio::spawn(async move {
            run_instance(
                &mut Command::new("/bin/cat"),
                &mut rx,
                &action,
                &(Arc::new(|_| {}) as HealthCallback),
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        tx.send_replace(None);
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;

    fn recording() -> Frame {
        Frame {
            session_id: "health-test".into(),
            sequence: 1,
            state: crate::CapsulePopupState {
                phase: "recording".into(),
                ..Default::default()
            },
            transcript: String::new(),
            transcript_font_size: 14.0,
            labels: Default::default(),
            show_transcript: false,
            theme: Default::default(),
            leaving: false,
            bounds: None,
            interactive: true,
        }
    }

    #[tokio::test]
    async fn failed_spawn_is_reported_and_is_not_cleared_by_idle() {
        let (changed, mut changes) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = CapsuleSupervisor::spawn(
            &tokio::runtime::Handle::current(),
            std::env::temp_dir().join(format!("missing-capsule-{}", std::process::id())),
            Arc::new(|_, _| panic!("failed helper must not send actions")),
            Arc::new(move || {
                let _ = changed.send(());
            }),
        );
        assert!(supervisor.last_error().is_none());
        supervisor.present(recording());
        tokio::time::timeout(Duration::from_secs(2), changes.recv())
            .await
            .unwrap()
            .unwrap();
        let failure = supervisor
            .last_error()
            .expect("spawn failure must reach the UI status");
        supervisor.stop();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(supervisor.last_error().as_deref(), Some(failure.as_str()));
    }

    fn helper(ready: bool) -> Command {
        #[cfg(windows)]
        {
            let mut command = Command::new("powershell.exe");
            let script = if ready {
                r#"[Console]::WriteLine('{"type":"ready","version":2}'); while ($null -ne ($line = [Console]::ReadLine())) { if ($line.Contains('"type":"shutdown"')) { break } }"#
            } else {
                "while ($null -ne [Console]::ReadLine()) {}"
            };
            command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("/bin/sh");
            let script = if ready {
                r#"printf '%s\n' '{"type":"ready","version":2}'; while IFS= read -r line; do case "$line" in *'"shutdown"'*) exit 0;; esac; done"#
            } else {
                "cat >/dev/null"
            };
            command.args(["-c", script]);
            command
        }
    }

    #[tokio::test]
    async fn successful_handshake_clears_failure_and_idle_reaps_the_child() {
        let (frames, mut receiver) = watch::channel(Some(recording()));
        let (status, mut health) = watch::channel(Some("previous startup failed".to_string()));
        let notify: HealthCallback = Arc::new(move |error| {
            status.send_replace(error);
        });
        let action: Arc<dyn Fn(String, Action) + Send + Sync> =
            Arc::new(|_, _| panic!("unexpected action"));
        let mut task = tokio::spawn(async move {
            // The Windows test fixture cold-starts PowerShell, unlike the
            // production native helper. Do not make its startup speed part
            // of the protocol/lifecycle contract. Production retains 5 s.
            run_instance_with_handshake_timeout(
                &mut helper(true),
                &mut receiver,
                &action,
                &notify,
                Duration::from_secs(15),
            )
            .await
        });
        tokio::select! {
            changed = health.changed() => changed.expect("helper health channel closed before Ready"),
            result = &mut task => panic!("helper exited before Ready: {result:?}"),
            _ = tokio::time::sleep(Duration::from_secs(20)) => panic!("test helper did not report Ready within its startup budget"),
        }
        assert!(health.borrow().is_none());
        frames.send_replace(None);
        assert!(tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
    }

    #[tokio::test]
    async fn unresponsive_helper_times_out_without_reporting_healthy() {
        let (_frames, mut receiver) = watch::channel(Some(recording()));
        let notify: HealthCallback = Arc::new(|_| panic!("no Ready means no healthy status"));
        let action: Arc<dyn Fn(String, Action) + Send + Sync> =
            Arc::new(|_, _| panic!("unexpected action"));
        let result = run_instance(&mut helper(false), &mut receiver, &action, &notify).await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn partial_protocol_line_survives_a_cancelled_read() {
        let (mut writer, reader) = tokio::io::duplex(128);
        let mut reader = BufReader::new(reader);
        let mut pending = Vec::new();
        writer.write_all(br#"{"type":"ready","#).await.unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            read_ui(&mut reader, &mut pending)
        )
        .await
        .is_err());
        writer.write_all(b"\"version\":2}\n").await.unwrap();
        assert!(matches!(
            read_ui(&mut reader, &mut pending).await.unwrap(),
            Some(UiMessage::Ready {
                version: PROTOCOL_VERSION
            })
        ));
        assert!(pending.is_empty());
    }
}
