//! Owned process completion and cancellation; no inference or model dependencies.
use openless_core::{BackendError, BackendErrorCode};
use std::process::Child;
use std::sync::Mutex;

fn error(message: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, message.to_string())
}

pub(super) struct Process {
    pub(super) child: Mutex<Option<Child>>,
}
impl Process {
    pub(super) fn running(&self) -> bool {
        self.child
            .lock()
            .unwrap()
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }
    pub(super) fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            if matches!(child.try_wait(), Ok(None)) {
                // PID cannot be reused while our unreaped Child owns it.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
    pub(super) fn completed(&self) -> Result<(), BackendError> {
        loop {
            // Keep the owned Child visible to stop() while waiting. Taking it
            // out and calling wait() made timeout/channel-disable unable to kill
            // a worker that closed stdout before exiting.
            let status = {
                let mut slot = self.child.lock().unwrap();
                let child = slot
                    .as_mut()
                    .ok_or_else(|| error("TextStream was cancelled"))?;
                let status = child.try_wait().map_err(error)?;
                if status.is_some() {
                    slot.take();
                }
                status
            };
            if let Some(status) = status {
                return if status.success() {
                    Ok(())
                } else {
                    Err(error(format!("TextStream worker exited: {status}")))
                };
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::Arc;

    #[test]
    fn cancellation_can_reap_a_worker_that_closed_stdout_before_exiting() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec 1>&-; exec /bin/sleep 30"])
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut stdout = child.stdout.take().unwrap();
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        let process = Arc::new(Process {
            child: Mutex::new(Some(child)),
        });
        let waiting = process.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let _ = tx.send(waiting.completed());
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        process.stop();
        let completed = rx.recv_timeout(std::time::Duration::from_secs(2));
        if completed.is_err() {
            // SAFETY: this test still owns the unreaped, 30-second child in its
            // waiter; kill only its newly-created process group on regression.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        reader.join().unwrap();
        assert!(
            completed.is_ok(),
            "stdout EOF must not hide the child from cancellation"
        );
        assert!(!process.running());
    }
}
