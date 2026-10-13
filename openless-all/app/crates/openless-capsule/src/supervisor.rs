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
}
impl CapsuleSupervisor {
    pub fn spawn(
        handle: &tokio::runtime::Handle,
        executable: std::path::PathBuf,
        on_action: Arc<dyn Fn(String, Action) + Send + Sync>,
    ) -> Self {
        let (frames, receiver) = watch::channel(None);
        handle.spawn(supervise(executable, receiver, on_action));
        Self { frames }
    }
    /// Coalesce volume/transcript frames; never allocate an unbounded queue.
    pub fn present(&self, frame: Frame) {
        self.frames.send_replace(frame.visible().then_some(frame));
    }
    pub fn stop(&self) {
        self.frames.send_replace(None);
    }
}

async fn read_ui(reader: &mut (impl AsyncBufRead + Unpin)) -> io::Result<Option<UiMessage>> {
    let mut bytes = Vec::new();
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
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
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
    executable: &std::path::Path,
    frames: &mut watch::Receiver<Option<Frame>>,
    on_action: &Arc<dyn Fn(String, Action) + Send + Sync>,
) -> io::Result<()> {
    let mut command = Command::new(executable);
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
    let deadline = tokio::time::sleep(Duration::from_secs(5));
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
                message = read_ui(&mut output) => {
                    match message? {
                        Some(UiMessage::Ready { version }) if version == PROTOCOL_VERSION && !ready => {
                            ready = true;
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
            match run_instance(&executable, &mut frames, &on_action).await {
                Ok(()) => {
                    failed = false;
                    break;
                }
                Err(error) => log::warn!("[native-capsule] helper failed: {error}"),
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
printf '%s\n' '{"type":"ready","version":1}'
while IFS= read -r frame; do
  case "$frame" in
    *'"shutdown"'*) exit 0;;
    *'"frame"'*)
      printf '%s\n' '{"type":"action","version":1,"session_id":"old","sequence":7,"action":"cancel"}'
      printf '%s\n' '{"type":"action","version":1,"session_id":"current","sequence":7,"action":"confirm"}'
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
        let task = tokio::spawn(async move { run_instance(&path, &mut rx, &callback).await });
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
            bounds: None,
            interactive: false,
        }));
        let action: Arc<dyn Fn(String, Action) + Send + Sync> =
            Arc::new(|_, _| panic!("no UI action expected"));
        let task = tokio::spawn(async move {
            run_instance(std::path::Path::new("/bin/cat"), &mut rx, &action).await
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
