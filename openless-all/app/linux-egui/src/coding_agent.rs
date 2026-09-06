//! Linux Coding Agent Adapter：只负责临时文件与子进程 I/O。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use openless_core::{
    AgentCommand, AgentMaterializationPlan, CancellationToken, CodingAgentProcessAdapter,
    ProcessExit, ProcessOutputLine, ProcessOutputSink, ProcessStream, PromptPayload,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Default)]
pub(crate) struct LinuxCodingAgentProcessAdapter;

struct TemporaryWorkspace(PathBuf);

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn materialize(
    command: &mut AgentCommand,
) -> Result<Option<TemporaryWorkspace>, openless_core::BackendError> {
    if command.temporary_files.is_empty() {
        return Ok(None);
    }
    let directory =
        std::env::temp_dir().join(format!("openless-agent-{}", uuid::Uuid::new_v4().simple()));
    let plan = AgentMaterializationPlan::new(command, &directory)?;
    std::fs::create_dir(&directory).map_err(platform_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(platform_error)?;
    }
    let workspace = TemporaryWorkspace(directory.clone());
    for file in plan.files {
        std::fs::write(file.path, file.contents).map_err(platform_error)?;
    }
    command.argv = plan.argv;
    Ok(Some(workspace))
}

static LOGIN_SHELL_PATH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();

async fn login_shell_path() -> Option<&'static str> {
    LOGIN_SHELL_PATH
        .get_or_init(|| async {
            let plan = openless_core::AgentLoginShellPathPlan::new(std::env::var("SHELL").ok())?;
            let deadline = tokio::time::Instant::now() + plan.timeout;
            for arguments in &plan.attempts {
                let mut command = tokio::process::Command::new(&plan.shell);
                command
                    .args(arguments)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                let Ok(Ok(output)) = tokio::time::timeout_at(deadline, command.output()).await
                else {
                    continue;
                };
                if output.status.success() {
                    if let Some(path) = openless_core::parse_agent_login_shell_path(
                        &String::from_utf8_lossy(&output.stdout),
                    ) {
                        return Some(path);
                    }
                }
            }
            None
        })
        .await
        .as_deref()
}

async fn augment_path(command: &mut tokio::process::Command, cancel: &CancellationToken) -> bool {
    if cancel.is_cancelled() {
        return false;
    }
    let current = std::env::var_os("PATH").unwrap_or_default();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = &home {
        command.env("HOME", home);
    }
    // Finder/desktop-launched shells may need up to five seconds to discover
    // login PATH. Cancellation must still win during that lookup; otherwise a
    // request cancelled before spawn appears to hang and no child exists for
    // the normal process-group kill path to terminate.
    let login_path = tokio::select! {
        path = login_shell_path() => path,
        _ = cancel.cancelled() => return false,
    };
    command.env(
        "PATH",
        openless_core::merge_agent_path(&current, home.as_deref(), login_path),
    );
    true
}

pub(crate) fn isolate_process_group(command: &mut tokio::process::Command) {
    #[cfg(target_os = "linux")]
    command.process_group(0);
    #[cfg(not(target_os = "linux"))]
    let _ = command;
}

pub(crate) fn kill_process_group(
    child: &mut tokio::process::Child,
) -> Result<(), openless_core::BackendError> {
    kill_process_group_with_id(child, child.id())
}

fn kill_process_group_with_id(
    child: &mut tokio::process::Child,
    _process_id: Option<u32>,
) -> Result<(), openless_core::BackendError> {
    #[cfg(target_os = "linux")]
    if _process_id.is_some_and(|pid| {
        // SAFETY: `isolate_process_group` starts the child as process-group leader.
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) == 0 }
    }) {
        return Ok(());
    }
    child.start_kill().map_err(platform_error)
}

impl CodingAgentProcessAdapter for LinuxCodingAgentProcessAdapter {
    fn execute(
        &self,
        mut request: AgentCommand,
        output: Arc<dyn ProcessOutputSink>,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<ProcessExit, openless_core::BackendError>> {
        Box::pin(async move {
            if cancel.is_cancelled() {
                return Ok(ProcessExit {
                    code: None,
                    success: false,
                });
            }
            let _workspace = materialize(&mut request)?;
            let mut command = tokio::process::Command::new(&request.executable);
            if !augment_path(&mut command, &cancel).await {
                return Ok(ProcessExit {
                    code: None,
                    success: false,
                });
            }
            command
                .args(&request.argv)
                .envs(&request.env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            isolate_process_group(&mut command);
            if let Some(cwd) = &request.cwd {
                command.current_dir(cwd);
            }
            if let PromptPayload::Argv(prompt) = &request.prompt {
                command.arg(prompt);
            }
            // PATH discovery/materialization may have completed at the same
            // time as Esc. Check again at the last boundary before spawn.
            if cancel.is_cancelled() {
                return Ok(ProcessExit {
                    code: None,
                    success: false,
                });
            }
            let mut child = command.spawn().map_err(|error| {
                openless_core::BackendError::new(
                    if error.kind() == std::io::ErrorKind::NotFound {
                        openless_core::BackendErrorCode::Unsupported
                    } else {
                        openless_core::BackendErrorCode::Platform
                    },
                    error.to_string(),
                )
            })?;
            // Remember the group before wait() reaps the leader: a descendant
            // can keep stdout/stderr open even after the leader has exited.
            let process_id = child.id();
            let stdin = child.stdin.take();
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| invalid("missing process stdout"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| invalid("missing process stderr"))?;
            let stdout_sink = Arc::clone(&output);
            let read_stdout = async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    stdout_sink.write(ProcessOutputLine {
                        stream: ProcessStream::Stdout,
                        line,
                    });
                }
            };
            let read_stderr = async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    output.write(ProcessOutputLine {
                        stream: ProcessStream::Stderr,
                        line,
                    });
                }
            };
            // Pipe backpressure is bidirectional: a CLI can fill stdout before
            // consuming a large prompt. Write, drain, and wait concurrently,
            // under the same cancellation lifetime. Scoped futures (not spawned
            // tasks) close every pipe before process-group cleanup begins.
            let result = {
                let write_stdin = async move {
                    if let (Some(mut stdin), PromptPayload::Stdin(prompt)) = (stdin, request.prompt)
                    {
                        stdin.write_all(prompt.as_bytes()).await?;
                        stdin.flush().await?;
                        stdin.shutdown().await?;
                    }
                    Ok::<_, std::io::Error>(())
                };
                let drain = async move {
                    tokio::join!(read_stdout, read_stderr);
                    Ok::<_, std::io::Error>(())
                };
                let operation = async {
                    tokio::try_join!(write_stdin, drain, child.wait()).map(|(_, _, status)| status)
                };
                tokio::pin!(operation);
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => None,
                    result = &mut operation => Some(result),
                }
            };
            let status = match result {
                Some(Ok(status)) => status,
                result => {
                    kill_process_group_with_id(&mut child, process_id)?;
                    let status = child.wait().await.map_err(platform_error)?;
                    if let Some(Err(error)) = result {
                        return Err(platform_error(error));
                    }
                    status
                }
            };
            Ok(ProcessExit {
                code: status.code(),
                success: status.success() && !cancel.is_cancelled(),
            })
        })
    }
}

fn invalid(message: impl Into<String>) -> openless_core::BackendError {
    openless_core::BackendError::new(openless_core::BackendErrorCode::InvalidArgument, message)
}

fn platform_error(error: impl std::fmt::Display) -> openless_core::BackendError {
    openless_core::BackendError::new(openless_core::BackendErrorCode::Platform, error.to_string())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use openless_core::{
        AgentCommand, CancellationToken, CodingAgentProcessAdapter, ProcessOutputLine,
        ProcessOutputSink, PromptPayload,
    };

    use super::LinuxCodingAgentProcessAdapter;

    struct IgnoreOutput;

    impl ProcessOutputSink for IgnoreOutput {
        fn write(&self, _line: ProcessOutputLine) {}
    }

    #[tokio::test]
    async fn cancellation_kills_a_running_child_with_blocked_stdin() {
        super::login_shell_path().await;
        let ready =
            std::env::temp_dir().join(format!("openless-agent-ready-{}", uuid::Uuid::new_v4()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let ready_for_cancel = ready.clone();
        let cancel_task = tokio::spawn(async move {
            for _ in 0..400 {
                if ready_for_cancel.exists() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            flag.store(true, Ordering::Release);
        });
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            LinuxCodingAgentProcessAdapter.execute(
                AgentCommand {
                    executable: "sh".into(),
                    argv: vec![
                        "-c".into(),
                        "sleep 30 & printf '%s %s' $$ $! > \"$OPENLESS_AGENT_TEST_READY\"; wait"
                            .into(),
                    ],
                    env: BTreeMap::from([(
                        "OPENLESS_AGENT_TEST_READY".into(),
                        ready.to_string_lossy().into_owned(),
                    )]),
                    cwd: None,
                    // Exceeds the pipe capacity; `sleep` never reads stdin.
                    prompt: PromptPayload::Stdin("x".repeat(1024 * 1024)),
                    temporary_files: Vec::new(),
                },
                Arc::new(IgnoreOutput),
                CancellationToken::from_flag(cancelled),
            ),
        )
        .await;
        cancel_task.await.unwrap();
        let pids = std::fs::read_to_string(&ready).expect("child must actually start");
        let _ = std::fs::remove_file(&ready);
        let mut running = Vec::new();
        for pid in pids.split_whitespace() {
            if std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(") ")
                        .map(|(_, rest)| !rest.starts_with('Z'))
                })
                .unwrap_or(false)
            {
                running.push(pid.to_owned());
                // Only fixture PIDs read from our private ready file are killed.
                unsafe {
                    libc::kill(pid.parse().unwrap(), libc::SIGKILL);
                }
            }
        }
        let result = result.expect("cancelled child must exit promptly").unwrap();
        assert!(!result.success);
        assert!(
            running.is_empty(),
            "cancelled process group is still running: {running:?}"
        );
    }
}
