//! Tauri Coding Agent Adapter：只负责临时文件与子进程 I/O。

pub mod commands;

#[cfg(windows)]
mod windows_job;

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
pub struct TauriCodingAgentProcessAdapter;

struct TemporaryWorkspace(PathBuf);

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn materialize_temporary_files(
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

#[cfg(unix)]
static LOGIN_SHELL_PATH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();

#[cfg(unix)]
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

async fn augment_path(_command: &mut tokio::process::Command, cancel: &CancellationToken) -> bool {
    if cancel.is_cancelled() {
        return false;
    }
    #[cfg(unix)]
    {
        let command = _command;
        let current = std::env::var_os("PATH").unwrap_or_default();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(home) = &home {
            command.env("HOME", home);
        }
        // A GUI launch may need a slow login shell to discover PATH. Esc must
        // interrupt that lookup too, before any agent process has been spawned.
        let login_path = tokio::select! {
            path = login_shell_path() => path,
            _ = cancel.cancelled() => return false,
        };
        command.env(
            "PATH",
            openless_core::merge_agent_path(&current, home.as_deref(), login_path),
        );
    }
    true
}

#[cfg(windows)]
fn windows_executable(request: &AgentCommand) -> PathBuf {
    let executable = std::path::Path::new(&request.executable);
    // Preserve explicit paths and extensions. Bare npm commands need their
    // .cmd shim selected before stdlib builds the safely quoted command line;
    // CreateProcess's default .exe lookup does not use PATHEXT.
    if executable.components().count() != 1 || executable.extension().is_some() {
        return executable.to_path_buf();
    }
    let path = request
        .env
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| std::ffi::OsString::from(value))
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        for extension in ["exe", "com", "cmd", "bat"] {
            let candidate = directory.join(executable).with_extension(extension);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    executable.to_path_buf()
}

impl CodingAgentProcessAdapter for TauriCodingAgentProcessAdapter {
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
            let _workspace = materialize_temporary_files(&mut request)?;
            #[cfg(windows)]
            let mut command = tokio::process::Command::new(windows_executable(&request));
            #[cfg(not(windows))]
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
            #[cfg(unix)]
            command.process_group(0);
            // Start suspended so no CLI code or descendants run before joining
            // this operation's Job. Resume only after assigning ownership; CLI
            // helpers also stay hidden behind the desktop UI.
            #[cfg(windows)]
            command.creation_flags(0x08000000 | 0x00000004);
            #[cfg(windows)]
            let job = windows_job::AgentProcessJob::new()?;
            if let Some(cwd) = &request.cwd {
                command.current_dir(cwd);
            }
            if let PromptPayload::Argv(prompt) = &request.prompt {
                command.arg(prompt);
            }
            // Esc can race PATH discovery or temporary-file materialization.
            // Never turn their late completion into a newly launched agent.
            if cancel.is_cancelled() {
                return Ok(ProcessExit {
                    code: None,
                    success: false,
                });
            }
            let mut child = command.spawn().map_err(|error| {
                let code = if error.kind() == std::io::ErrorKind::NotFound {
                    openless_core::BackendErrorCode::Unsupported
                } else {
                    openless_core::BackendErrorCode::Platform
                };
                openless_core::BackendError::new(code, error.to_string())
            })?;
            #[cfg(windows)]
            job.assign_and_resume(&child, &cancel)?;
            // Keep the Unix process-group ID even if wait() reaps its leader
            // while a descendant still owns an inherited output pipe.
            #[cfg(unix)]
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
            // A large stdin prompt can fill its pipe before the CLI starts
            // reading; the CLI can simultaneously be blocked on stdout. Poll
            // all I/O and child.wait together, so neither backpressure nor an
            // inherited pipe can prevent Esc/timeout from reaching tree cleanup.
            let result = {
                let write_stdin = async move {
                    if let (Some(mut stdin), PromptPayload::Stdin(prompt)) = (stdin, request.prompt)
                    {
                        stdin.write_all(prompt.as_bytes()).await?;
                        // Windows may acknowledge a write into Tokio's blocking
                        // buffer; flush waits for actual delivery and reports
                        // its errors. This wait remains cancellable with the I/O.
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
            // Scoped I/O futures now release their pipe/sink ownership. On
            // Windows, killing the Job also releases pending blocking pipe I/O.
            // Terminate descendants before reaping the immediate child.
            let status = match result {
                Some(Ok(status)) => status,
                result => {
                    #[cfg(unix)]
                    let killed_group = process_id.is_some_and(|pid| {
                        // SAFETY: the child was started as process-group leader above.
                        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) == 0 }
                    });
                    #[cfg(windows)]
                    let killed_group = {
                        job.terminate()?;
                        true
                    };
                    #[cfg(not(any(unix, windows)))]
                    let killed_group = false;
                    if !killed_group {
                        child.start_kill().map_err(platform_error)?;
                    }
                    let status = child.wait().await.map_err(platform_error)?;
                    if let Some(Err(error)) = result {
                        return Err(platform_error(error));
                    }
                    status
                }
            };
            let success = status.success() && !cancel.is_cancelled();
            #[cfg(windows)]
            if success {
                job.release_completed()?;
            }
            Ok(ProcessExit {
                code: status.code(),
                success,
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

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    struct IgnoreOutput;

    impl ProcessOutputSink for IgnoreOutput {
        fn write(&self, _line: ProcessOutputLine) {}
    }

    #[derive(Default)]
    struct CaptureOutput(std::sync::Mutex<Vec<String>>);

    impl ProcessOutputSink for CaptureOutput {
        fn write(&self, output: ProcessOutputLine) {
            self.0.lock().unwrap().push(output.line);
        }
    }

    #[tokio::test]
    async fn windows_cli_commands_resolve_npm_shims_and_preserve_arguments() {
        let directory = std::env::temp_dir().join(format!(
            "openless agent shim {}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&directory).unwrap();
        let _workspace = TemporaryWorkspace(directory.clone());
        let executable = std::env::current_exe().unwrap();
        let shim_name = format!("openless-fixture-{}", uuid::Uuid::new_v4().simple());
        let shim = directory.join(format!("{shim_name}.cmd"));
        // Same forwarding shape as an npm shim: quote the program path and
        // forward the caller's arguments through %*. No real Agent is invoked.
        std::fs::write(
            &shim,
            "@echo off\r\n\"%OPENLESS_AGENT_TEST_EXE%\" --exact coding_agent::tests::argument_echo_child --nocapture %*\r\n",
        )
        .unwrap();
        let mut search = vec![
            directory.clone(),
            executable.parent().unwrap().to_path_buf(),
        ];
        search.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(search)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let expected = vec![
            "with spaces".to_string(),
            "with \"quotes\"".to_string(),
            "literal&pipe|redirect>less<caret^percent%bang!".to_string(),
        ];
        let arguments = expected
            .iter()
            .flat_map(|value| ["--skip".to_string(), value.clone()])
            .collect::<Vec<_>>();
        for (program, is_shim) in [
            (executable.to_string_lossy().into_owned(), false),
            (
                executable
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                false,
            ),
            (shim.to_string_lossy().into_owned(), true),
            (shim_name, true),
        ] {
            let mut argv = if is_shim {
                Vec::new()
            } else {
                vec![
                    "--exact".into(),
                    "coding_agent::tests::argument_echo_child".into(),
                    "--nocapture".into(),
                ]
            };
            argv.extend(arguments.clone());
            let output = Arc::new(CaptureOutput::default());
            let result = TauriCodingAgentProcessAdapter
                .execute(
                    AgentCommand {
                        executable: program.clone(),
                        argv,
                        env: BTreeMap::from([
                            // Windows keys are case-insensitive: envs() applies
                            // the last entry, so resolution must use it too.
                            (
                                "PATH".into(),
                                directory
                                    .join("not-the-search-path")
                                    .to_string_lossy()
                                    .into_owned(),
                            ),
                            ("Path".into(), path.clone()),
                            (
                                "OPENLESS_AGENT_TEST_EXE".into(),
                                executable.to_string_lossy().into_owned(),
                            ),
                            ("OPENLESS_AGENT_TEST_ARGUMENTS".into(), "1".into()),
                        ]),
                        cwd: None,
                        prompt: PromptPayload::Stdin(String::new()),
                        temporary_files: Vec::new(),
                    },
                    output.clone(),
                    CancellationToken::new(),
                )
                .await
                .unwrap_or_else(|error| panic!("{program}: {error}"));
            let lines = output.0.lock().unwrap();
            assert!(result.success, "{program}: {lines:?}");
            let actual = lines
                .iter()
                .find_map(|line| line.strip_prefix("OPENLESS_AGENT_ARGUMENTS="))
                .unwrap_or_else(|| panic!("argument fixture did not run for {program}: {lines:?}"));
            assert_eq!(
                serde_json::from_str::<Vec<String>>(actual).unwrap(),
                expected,
                "{program}"
            );
        }
    }

    #[test]
    fn argument_echo_child() {
        if std::env::var("OPENLESS_AGENT_TEST_ARGUMENTS").as_deref() != Ok("1") {
            return;
        }
        let argv = std::env::args().collect::<Vec<_>>();
        let forwarded = argv
            .windows(2)
            .filter(|pair| pair[0] == "--skip")
            .map(|pair| pair[1].clone())
            .collect::<Vec<_>>();
        println!(
            "OPENLESS_AGENT_ARGUMENTS={}",
            serde_json::to_string(&forwarded).unwrap()
        );
    }

    #[tokio::test]
    async fn windows_opencode_npm_shim_accepts_the_core_multiline_prompt() {
        let directory = std::env::temp_dir().join(format!(
            "openless opencode stdin {}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&directory).unwrap();
        let _workspace = TemporaryWorkspace(directory.clone());
        let shim = directory.join("opencode.cmd");
        std::fs::write(
            &shim,
            "@echo off\r\nif \"%~1\"==\"--version\" (echo 1.18.29& exit /b 0)\r\n\"%OPENLESS_AGENT_TEST_EXE%\" --exact coding_agent::tests::stdin_echo_child --nocapture\r\n",
        )
        .unwrap();
        let prompt = openless_core::autonomous_prompt("第一行：\"hello\" & %PATH%\r\n第二行：🙂");
        let mut request = openless_core::CodingAgentRequest::new("stdin", prompt.clone());
        request.provider = openless_core::CodingAgentProvider::OpenCodeCli;
        let run = openless_core::build_agent_command(&request).unwrap();
        let mut detect = run.clone();
        detect.argv = vec!["--version".into()];
        detect.prompt = PromptPayload::Stdin(String::new());
        for (mut command, marker, expected) in [
            (detect, "1.18.29", None),
            (run, "OPENLESS_AGENT_STDIN=", Some(prompt)),
        ] {
            command
                .env
                .insert("PATH".into(), directory.to_string_lossy().into_owned());
            command.env.insert(
                "OPENLESS_AGENT_TEST_EXE".into(),
                std::env::current_exe()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            command
                .env
                .insert("OPENLESS_AGENT_TEST_STDIN".into(), "1".into());
            let output = Arc::new(CaptureOutput::default());
            let result = TauriCodingAgentProcessAdapter
                .execute(command, output.clone(), CancellationToken::new())
                .await
                .expect("the detected npm shim must also accept a real Less Computer request");
            let lines = output.0.lock().unwrap();
            assert!(result.success, "{lines:?}");
            let value = lines
                .iter()
                .find_map(|line| line.strip_prefix(marker))
                .unwrap_or_else(|| panic!("missing {marker}: {lines:?}"));
            if let Some(expected) = expected {
                assert_eq!(serde_json::from_str::<String>(value).unwrap(), expected);
            }
        }
    }

    #[test]
    fn stdin_echo_child() {
        if std::env::var("OPENLESS_AGENT_TEST_STDIN").as_deref() != Ok("1") {
            return;
        }
        use std::io::Read;
        let mut prompt = String::new();
        std::io::stdin().read_to_string(&mut prompt).unwrap();
        println!(
            "OPENLESS_AGENT_STDIN={}",
            serde_json::to_string(&prompt).unwrap()
        );
    }

    #[tokio::test]
    async fn cancellation_kills_a_windows_process_tree_with_blocked_stdin() {
        // Windows' async pipe adapter can buffer a 1 MiB write internally. Keep
        // the requested prompt-size regression and exceed that buffering too,
        // so the second run proves cancellation while write_all is pending.
        for prompt_bytes in [1024 * 1024, 4 * 1024 * 1024] {
            assert_cancelled_process_tree(prompt_bytes, false).await;
        }
    }

    #[tokio::test]
    async fn cancellation_kills_descendants_after_the_parent_exits() {
        // The initial process can exit before Esc while a descendant still
        // holds both output pipes. A dead parent PID is insufficient for /T.
        assert_cancelled_process_tree(0, true).await;
    }

    #[tokio::test]
    async fn natural_success_preserves_a_redirected_background_child() {
        let ready =
            std::env::temp_dir().join(format!("openless-agent-ready-{}", uuid::Uuid::new_v4()));
        let mut request = fixture_request(&ready, 0, true);
        request
            .env
            .insert("OPENLESS_AGENT_TEST_BACKGROUND".into(), "true".into());
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            TauriCodingAgentProcessAdapter.execute(
                request,
                Arc::new(IgnoreOutput),
                CancellationToken::new(),
            ),
        )
        .await;
        let pids = std::fs::read_to_string(&ready).expect("child must actually start");
        let _ = std::fs::remove_file(&ready);
        let grandchild = pids.split_whitespace().nth(1).unwrap();
        let survived = process_is_running(grandchild).await;
        cleanup_fixture_process(grandchild).await;
        assert!(
            result
                .expect("completed CLI must return promptly")
                .unwrap()
                .success
        );
        assert!(
            survived,
            "natural success must preserve an intentionally detached worker"
        );
    }

    #[tokio::test]
    async fn dropping_execution_kills_its_owned_process_tree() {
        let ready =
            std::env::temp_dir().join(format!("openless-agent-ready-{}", uuid::Uuid::new_v4()));
        let operation = tokio::spawn(TauriCodingAgentProcessAdapter.execute(
            fixture_request(&ready, 4 * 1024 * 1024, false),
            Arc::new(IgnoreOutput),
            CancellationToken::new(),
        ));
        for _ in 0..400 {
            if std::fs::read_to_string(&ready)
                .is_ok_and(|pids| pids.split_whitespace().count() == 2)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // An outer timeout may drop the future before cooperative cancellation
        // runs. Job ownership must survive even this direct abort path.
        operation.abort();
        let _ = operation.await;
        let pids = std::fs::read_to_string(&ready).expect("child must actually start");
        let _ = std::fs::remove_file(&ready);
        for pid in pids.split_whitespace() {
            let running = process_is_running(pid).await;
            if running {
                cleanup_fixture_process(pid).await;
            }
            assert!(!running, "dropped execution left fixture PID {pid} running");
        }
    }

    fn fixture_request(
        ready: &std::path::Path,
        prompt_bytes: usize,
        parent_exits: bool,
    ) -> AgentCommand {
        AgentCommand {
            executable: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            argv: vec![
                "--exact".into(),
                "coding_agent::tests::blocked_stdin_child".into(),
                "--nocapture".into(),
            ],
            env: BTreeMap::from([
                (
                    "OPENLESS_AGENT_TEST_READY".into(),
                    ready.to_string_lossy().into_owned(),
                ),
                (
                    "OPENLESS_AGENT_TEST_PARENT_EXIT".into(),
                    parent_exits.to_string(),
                ),
            ]),
            cwd: None,
            prompt: PromptPayload::Stdin("x".repeat(prompt_bytes)),
            temporary_files: Vec::new(),
        }
    }

    async fn process_is_running(pid: &str) -> bool {
        let mut query = tokio::process::Command::new("tasklist");
        query.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
        query.creation_flags(0x08000000);
        String::from_utf8_lossy(&query.output().await.unwrap().stdout)
            .contains(&format!("\"{pid}\""))
    }

    async fn cleanup_fixture_process(pid: &str) {
        // These PIDs belong exclusively to the fixture's private ready file.
        let mut cleanup = tokio::process::Command::new("taskkill");
        cleanup
            .args(["/PID", pid, "/T", "/F"])
            .creation_flags(0x08000000);
        let _ = cleanup.output().await;
    }

    async fn assert_cancelled_process_tree(prompt_bytes: usize, parent_exits: bool) {
        let ready =
            std::env::temp_dir().join(format!("openless-agent-ready-{}", uuid::Uuid::new_v4()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let ready_for_cancel = ready.clone();
        let cancel_task = tokio::spawn(async move {
            // Do not let cancellation win before spawn: this test must reach
            // an actually full stdin pipe, not merely exercise the early guard.
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
            TauriCodingAgentProcessAdapter.execute(
                fixture_request(&ready, prompt_bytes, parent_exits),
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
            if process_is_running(pid).await {
                running.push(pid.to_owned());
                // Cleanup also runs on a red test, so no fixture process leaks
                // into subsequent tests when the assertion below fails.
                cleanup_fixture_process(pid).await;
            }
        }
        let result = result
            .expect("cancelled Windows process tree must exit promptly")
            .unwrap();
        assert!(!result.success);
        assert!(
            running.is_empty(),
            "cancelled process tree is still running: {running:?}"
        );
    }

    #[test]
    fn blocked_stdin_child() {
        use std::os::windows::process::CommandExt;
        let Ok(ready) = std::env::var("OPENLESS_AGENT_TEST_READY") else {
            return;
        };
        let mut command = std::process::Command::new("ping.exe");
        command
            .args(["-n", "31", "127.0.0.1"])
            .stdin(Stdio::null())
            .creation_flags(0x08000000);
        if std::env::var("OPENLESS_AGENT_TEST_BACKGROUND").as_deref() == Ok("true") {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Foundation::{
                SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT,
            };
            // Windows can inherit ambient handles even with redirected stdio.
            // Explicitly detach these pipe handles too, so this fixture models
            // a worker that genuinely no longer owns the execution's output.
            for handle in [
                std::io::stdout().as_raw_handle(),
                std::io::stderr().as_raw_handle(),
            ] {
                unsafe {
                    SetHandleInformation(HANDLE(handle), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
                }
                .unwrap();
            }
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let mut grandchild = command.spawn().unwrap();
        std::fs::write(ready, format!("{} {}", std::process::id(), grandchild.id())).unwrap();
        if std::env::var("OPENLESS_AGENT_TEST_PARENT_EXIT").as_deref() == Ok("true") {
            return;
        }
        // Neither this process nor the grandchild reads the supplied 1 MiB.
        // Inherited stdout/stderr keep the pipes open until the whole tree dies.
        std::thread::sleep(std::time::Duration::from_secs(30));
        let _ = grandchild.kill();
        let _ = grandchild.wait();
    }
}
