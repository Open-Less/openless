//! 无头 Codex（`codex exec --json`）适配器。
//!
//! 与 Claude / OpenCode 适配器同形、复用同一套 [`CodingAgentRequest`] / [`CodingAgentEvent`] /
//! [`CodingAgentError`]，但对接的是 OpenAI Codex CLI：
//!
//! - 检测：`codex --version`（输出形如 `codex-cli 0.146.0`）。
//! - 运行：`codex exec --json --color never --skip-git-repo-check -s <sandbox> [-C <cwd>]
//!   [-m <model>] -`，**prompt 走 stdin**（末尾的 `-` 就是「从 stdin 读」的官方写法），
//!   逐行解析 JSONL 事件。
//! - 续接：`… -s <sandbox> resume --last -`。注意**参数顺序**：`-s` / `--json` 这些
//!   `codex exec` 的选项必须排在 `resume` 子命令**之前**，写在后面 CLI 会直接报
//!   `unexpected argument`。
//! - 护栏：Codex 没有 Claude `--settings` / OpenCode `permission` 那种逐命令 deny 清单
//!   （`.rules` execpolicy 只从 `$CODEX_HOME/rules/` 或项目目录读，实测 `-c rules=[…]`
//!   会被静默忽略）。它的护栏是自带的 seatbelt 沙箱：`-s workspace-write` 把写入限制在
//!   工作目录内、限制网络，且非交互下越权请求自动拒。见 [`codex_sandbox_mode`]。
//! - 输出：`item.completed` 里 `agent_message` 是**完整文本块**（非逐字 delta），
//!   `command_execution` 是命令执行。终局看 `turn.completed` / `turn.failed`，
//!   token 用量在 `turn.completed.usage`（**不给美元成本**，故 `cost_usd = None`）。

use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::stream::CodingAgentEvent;
use super::{augmented_command, wait_cancel, CodingAgentEventSink, CodingAgentRequest};
use super::{CodingAgentError, CodingAgentPermissionMode};

/// 权限模式 → Codex `-s/--sandbox` 取值。
///
/// Codex 给不了逐命令 deny 清单，护栏落在它自己的 seatbelt 沙箱上（用户已就此拍板）：
/// - `Plan` → `read-only`：只读，连工作目录都不让写。
/// - `AcceptEdits` → `workspace-write`：写入限制在工作目录内，网络受限，越权请求在无头下自动拒。
/// - `Plan` / 遗留的 `Default` / `BypassPermissions` → `read-only`：对 Codex 统一 fail-closed。
pub fn codex_sandbox_mode(mode: CodingAgentPermissionMode) -> &'static str {
    match mode {
        CodingAgentPermissionMode::AcceptEdits => "workspace-write",
        CodingAgentPermissionMode::Plan
        | CodingAgentPermissionMode::Default
        | CodingAgentPermissionMode::BypassPermissions => "read-only",
    }
}

/// 构造 `codex` 的命令行参数（不含可执行文件本身，也不含 prompt——prompt 走 stdin）。
///
/// 安全（参数注入防护）：prompt **完全不进 argv**，末尾固定是 `-`（Codex 官方的
/// 「从 stdin 读指令」写法），运行器把 prompt 写进 stdin 后关闭。这样以 `-` / `--` 开头的
/// prompt（语音转写或被注入诱导而成）根本没有机会被 CLI 解析成 flag，也不会出现在进程
/// 列表里泄露内容。
pub fn build_codex_args(req: &CodingAgentRequest) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        // 关掉 ANSI：JSONL 里混进转义序列会让逐行解析变脆。
        "--color".into(),
        "never".into(),
        // Less Computer 的工作目录默认是家目录，通常不是 git 仓库；不加这个 codex 会直接拒跑。
        "--skip-git-repo-check".into(),
        "--sandbox".into(),
        codex_sandbox_mode(req.permission_mode).into(),
        // 收紧 workspace-write：Codex 默认把 `/tmp` 和 `$TMPDIR` 也算成可写根，
        // 也就是说不加这两条，"写入限制在工作目录内"就是句假话——实测语音任务真的能在
        // $TMPDIR 里落文件。设置页对用户是这么承诺的，这里就必须让它成立。
        "-c".into(),
        "sandbox_workspace_write.exclude_tmpdir_env_var=true".into(),
        "-c".into(),
        "sandbox_workspace_write.exclude_slash_tmp=true".into(),
    ];
    if let Some(model) = &req.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(cwd) = &req.cwd {
        args.push("--cd".into());
        args.push(cwd.to_string_lossy().into_owned());
    }
    // 续接必须排在所有 `codex exec` 选项之后：resume 是子命令，它不认 -s/--json 这些，
    // 写反了 CLI 会报 `unexpected argument '-s' found`。
    if req.continue_session {
        args.push("resume".into());
        args.push("--last".into());
    }
    // `-` = 从 stdin 读指令。必须是最后一个位置参数。
    args.push("-".into());
    args
}

/// 解析一行 `codex exec --json` 的 JSONL。
///
/// 关注的几类（其余忽略）：
/// - `item.completed` / `item.type = "agent_message"` → `text`（完整文本块，作为 Delta 抛出；
///   运行器累计成最终结果）。
/// - `item.started` / `item.type = "command_execution"` → 命令执行，作为 ToolUse 抛出。
///   只认 `item.started`，避免同一条命令在 started/completed 各报一次。
/// - `turn.failed` → Error（真·终局失败）。
///
/// **刻意不处理**的：`item.type = "error"`。Codex 会把「模型元数据没找到」这类**警告**也
/// 塞成 error item，把它当终局会让一次本来成功的运行被误判为失败。真正的失败只看
/// `turn.failed`；运行器另外把这些 error item 的文案留作失败时的补充说明。
#[derive(Debug, PartialEq)]
enum CodexProtocolEvent {
    Output(CodingAgentEvent),
    TurnCompleted,
    TurnFailed(String),
    Error(String),
}

fn protocol_error_message(value: &serde_json::Value, fallback: &str) -> String {
    value
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("message").and_then(serde_json::Value::as_str))
        .or_else(|| value.get("error").and_then(serde_json::Value::as_str))
        .unwrap_or(fallback)
        .to_string()
}

fn parse_codex_protocol_line(session_id: &str, line: &str) -> Option<CodexProtocolEvent> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event_type = value.get("type")?.as_str()?;
    match event_type {
        "turn.completed" => Some(CodexProtocolEvent::TurnCompleted),
        "turn.failed" => Some(CodexProtocolEvent::TurnFailed(protocol_error_message(
            &value,
            "Codex 本轮执行失败",
        ))),
        "error" => Some(CodexProtocolEvent::Error(protocol_error_message(
            &value,
            "Codex 协议错误",
        ))),
        _ => parse_codex_json_line(session_id, line).map(CodexProtocolEvent::Output),
    }
}

pub fn parse_codex_json_line(session_id: &str, line: &str) -> Option<CodingAgentEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let event_type = v.get("type")?.as_str()?;
    if event_type == "error" {
        return Some(CodingAgentEvent::Error {
            session_id: session_id.to_string(),
            message: protocol_error_message(&v, "Codex 协议错误"),
        });
    }
    match event_type {
        "item.completed" => {
            let item = v.get("item")?;
            match item.get("type")?.as_str()? {
                "agent_message" => {
                    let text = item.get("text")?.as_str()?.to_string();
                    if text.is_empty() {
                        return None;
                    }
                    Some(CodingAgentEvent::Delta {
                        session_id: session_id.to_string(),
                        text,
                    })
                }
                _ => None,
            }
        }
        "item.started" => {
            let item = v.get("item")?;
            match item.get("type")?.as_str()? {
                // 展示用途：取命令首个 token 当「工具名」，整条命令太长塞不进胶囊。
                "command_execution" => {
                    let command = item.get("command")?.as_str()?;
                    Some(CodingAgentEvent::ToolUse {
                        session_id: session_id.to_string(),
                        name: command_display_name(command),
                    })
                }
                "file_change" | "patch_apply" => Some(CodingAgentEvent::ToolUse {
                    session_id: session_id.to_string(),
                    name: "edit".to_string(),
                }),
                "mcp_tool_call" => {
                    let name = item
                        .get("tool")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("mcp")
                        .to_string();
                    Some(CodingAgentEvent::ToolUse {
                        session_id: session_id.to_string(),
                        name,
                    })
                }
                "web_search" => Some(CodingAgentEvent::ToolUse {
                    session_id: session_id.to_string(),
                    name: "web_search".to_string(),
                }),
                _ => None,
            }
        }
        "turn.failed" => {
            let message = v
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .or_else(|| v.get("error").and_then(serde_json::Value::as_str))
                .unwrap_or("Codex 本轮执行失败")
                .to_string();
            Some(CodingAgentEvent::Error {
                session_id: session_id.to_string(),
                message,
            })
        }
        _ => None,
    }
}

/// 从一条 shell 命令里取出用于展示的「工具名」。
///
/// Codex 的 `command_execution` 给的是整条命令（常见形如 `/bin/zsh -lc 'cat a.txt'`），
/// 直接塞进胶囊又长又不可读。这里剥掉 shell 包装、取真正执行的那个命令名。
fn command_display_name(command: &str) -> String {
    let trimmed = command.trim();
    // `/bin/zsh -lc '<真实命令>'` / `bash -c "<真实命令>"`：取引号里的第一个 token。
    if let Some(idx) = trimmed.find(|c| c == '\'' || c == '"') {
        let quote = trimmed.as_bytes()[idx] as char;
        let inner = &trimmed[idx + 1..];
        if let Some(end) = inner.find(quote) {
            if let Some(first) = inner[..end].split_whitespace().next() {
                if !first.is_empty() {
                    return first.to_string();
                }
            }
        }
    }
    trimmed
        .split_whitespace()
        .next()
        .unwrap_or("bash")
        .rsplit('/')
        .next()
        .unwrap_or("bash")
        .to_string()
}

#[derive(Default)]
struct CodexProtocolState {
    accumulated: String,
    saw_turn_completed: bool,
    protocol_error: Option<String>,
}

impl CodexProtocolState {
    fn observe(&mut self, event: CodexProtocolEvent) -> Option<CodingAgentEvent> {
        match event {
            CodexProtocolEvent::Output(event) => {
                if let CodingAgentEvent::Delta { text, .. } = &event {
                    self.accumulated.push_str(text);
                }
                Some(event)
            }
            CodexProtocolEvent::TurnCompleted => {
                self.saw_turn_completed = true;
                None
            }
            CodexProtocolEvent::TurnFailed(message) | CodexProtocolEvent::Error(message) => {
                if self.protocol_error.is_none() {
                    self.protocol_error = Some(message);
                }
                None
            }
        }
    }
}

struct CodexRunFailure {
    error: CodingAgentError,
    message: String,
}

fn finalize_codex_run(
    session_id: &str,
    state: &CodexProtocolState,
    process_succeeded: bool,
    exit_code: Option<i32>,
    stderr: &str,
) -> Result<CodingAgentEvent, CodexRunFailure> {
    if let Some(message) = &state.protocol_error {
        return Err(CodexRunFailure {
            error: CodingAgentError::Protocol(message.clone()),
            message: message.clone(),
        });
    }

    if !process_succeeded {
        let message = stderr.lines().last().unwrap_or("").trim().to_string();
        return Err(CodexRunFailure {
            error: CodingAgentError::ProcessExit(exit_code),
            message: if message.is_empty() {
                format!("agent 异常退出 (code={exit_code:?})")
            } else {
                message
            },
        });
    }

    if !state.saw_turn_completed {
        let message = "Codex 进程结束但未收到 turn.completed".to_string();
        return Err(CodexRunFailure {
            error: CodingAgentError::Protocol(message.clone()),
            message,
        });
    }

    Ok(CodingAgentEvent::Completed {
        session_id: session_id.to_string(),
        text: state.accumulated.trim().to_string(),
        cost_usd: None,
        duration_ms: None,
    })
}

/// 无头跑一次 Codex：prompt 写进 stdin，逐行解析 JSONL，把事件投到 `sink`。
/// 支持取消与超时（都会 kill 子进程）。
pub async fn run_codex_agent(
    exe: &str,
    req: CodingAgentRequest,
    sink: CodingAgentEventSink,
    cancel: Arc<AtomicBool>,
) -> Result<(), CodingAgentError> {
    let args = build_codex_args(&req);
    let mut cmd = augmented_command(exe).await;
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    }

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CodingAgentError::ExecutableNotFound(exe.to_string())
        } else {
            CodingAgentError::Spawn(e.to_string())
        }
    })?;

    // 写入 prompt 后立即关闭 stdin，`-` 才会读到 EOF 开始处理。
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(req.prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let stderr_task = child.stderr.take().map(|s| {
        tokio::spawn(async move {
            let mut buf = String::new();
            let _ = BufReader::new(s).read_to_string(&mut buf).await;
            buf
        })
    });

    let _ = sink.send(CodingAgentEvent::Started {
        session_id: req.session_id.clone(),
    });

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CodingAgentError::Io("子进程无 stdout".into()))?;
    let mut lines = BufReader::new(stdout).lines();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(req.timeout_secs.max(1));
    // Codex 的 turn.completed 不带最终文本：累计所有 agent_message 块，收到终局后合成 Completed。
    let mut protocol_state = CodexProtocolState::default();
    let mut outcome: Result<(), CodingAgentError> = Ok(());

    loop {
        tokio::select! {
            biased;
            _ = wait_cancel(&cancel) => {
                let _ = child.start_kill();
                let _ = sink.send(CodingAgentEvent::Cancelled { session_id: req.session_id.clone() });
                outcome = Err(CodingAgentError::Cancelled);
                break;
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = child.start_kill();
                let _ = sink.send(CodingAgentEvent::Error {
                    session_id: req.session_id.clone(),
                    message: format!("运行超时（{}s）", req.timeout_secs),
                });
                outcome = Err(CodingAgentError::Timeout(req.timeout_secs));
                break;
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        if let Some(protocol_event) =
                            parse_codex_protocol_line(&req.session_id, &l)
                        {
                            if let Some(ev) =
                                protocol_state.observe(protocol_event)
                            {
                                let _ = sink.send(ev);
                            }
                        }
                    }
                    Ok(None) => break, // EOF：交给终局校验
                    Err(e) => {
                        outcome = Err(CodingAgentError::Io(e.to_string()));
                        break;
                    }
                }
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| CodingAgentError::Io(e.to_string()))?;

    if let Err(error) = outcome {
        return Err(error);
    }

    let stderr = match stderr_task {
        Some(t) => t.await.unwrap_or_default(),
        None => String::new(),
    };
    match finalize_codex_run(
        &req.session_id,
        &protocol_state,
        status.success(),
        status.code(),
        &stderr,
    ) {
        Ok(event) => {
            let _ = sink.send(event);
            Ok(())
        }
        Err(failure) => {
            let _ = sink.send(CodingAgentEvent::Error {
                session_id: req.session_id.clone(),
                message: failure.message,
            });
            Err(failure.error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
    }

    #[test]
    fn run_args_are_json_and_prompt_never_enters_argv() {
        let req = CodingAgentRequest::new("s1", "hello world");
        let args = build_codex_args(&req);
        assert_eq!(args.first().map(|s| s.as_str()), Some("exec"));
        assert!(args.contains(&"--json".to_string()));
        assert_eq!(arg_value(&args, "--color"), Some("never"));
        // prompt 走 stdin：argv 里不能有它的任何痕迹。
        assert!(!args.iter().any(|a| a.contains("hello world")));
        // 末尾固定是 `-`（从 stdin 读指令）。
        assert_eq!(args.last().map(|s| s.as_str()), Some("-"));
    }

    #[test]
    fn stdin_marker_stays_last_even_with_every_flag_set() {
        let mut req = CodingAgentRequest::new("s", "p");
        req.model = Some("gpt-5".into());
        req.cwd = Some(PathBuf::from("/tmp/work"));
        req.permission_mode = CodingAgentPermissionMode::AcceptEdits;
        let args = build_codex_args(&req);
        assert_eq!(args.last().map(|s| s.as_str()), Some("-"));
        assert_eq!(arg_value(&args, "--model"), Some("gpt-5"));
        assert_eq!(arg_value(&args, "--cd"), Some("/tmp/work"));
    }

    #[test]
    fn permission_mode_maps_to_sandbox_flag() {
        assert_eq!(
            codex_sandbox_mode(CodingAgentPermissionMode::Plan),
            "read-only"
        );
        assert_eq!(
            codex_sandbox_mode(CodingAgentPermissionMode::AcceptEdits),
            "workspace-write"
        );
        assert_eq!(
            codex_sandbox_mode(CodingAgentPermissionMode::Default),
            "read-only"
        );
        assert_eq!(
            codex_sandbox_mode(CodingAgentPermissionMode::BypassPermissions),
            "read-only"
        );
        let mut req = CodingAgentRequest::new("s", "p");
        req.permission_mode = CodingAgentPermissionMode::Plan;
        assert_eq!(
            arg_value(&build_codex_args(&req), "--sandbox"),
            Some("read-only")
        );
    }

    #[test]
    fn continue_session_puts_resume_after_exec_options_but_before_stdin_marker() {
        // 关键不变量：`resume` 必须排在 --json / --sandbox 这些 exec 选项之后，
        // 否则真实 CLI 会报 `unexpected argument '-s' found`（已对本机 0.146.0 核实）。
        let mut req = CodingAgentRequest::new("s", "p");
        req.continue_session = true;
        let args = build_codex_args(&req);
        let resume_idx = args.iter().position(|a| a == "resume").unwrap();
        let sandbox_idx = args.iter().position(|a| a == "--sandbox").unwrap();
        let json_idx = args.iter().position(|a| a == "--json").unwrap();
        assert!(sandbox_idx < resume_idx, "--sandbox 必须在 resume 之前");
        assert!(json_idx < resume_idx, "--json 必须在 resume 之前");
        assert!(args.contains(&"--last".to_string()));
        assert_eq!(args.last().map(|s| s.as_str()), Some("-"));
    }

    #[test]
    fn workspace_write_excludes_tmp_dirs() {
        // 回归防线：不加这两条，Codex 的 workspace-write 会把 /tmp 和 $TMPDIR 也放开
        // （实测能在 $TMPDIR 里落文件），设置页承诺的「限制在工作目录内」就不成立。
        let req = CodingAgentRequest::new("s", "p");
        let args = build_codex_args(&req);
        assert!(args
            .iter()
            .any(|a| a == "sandbox_workspace_write.exclude_tmpdir_env_var=true"));
        assert!(args
            .iter()
            .any(|a| a == "sandbox_workspace_write.exclude_slash_tmp=true"));
    }

    #[test]
    fn skip_git_repo_check_is_always_on() {
        // Less Computer 的工作目录默认是家目录（通常不是 git 仓库），少了这个 codex 直接拒跑。
        let req = CodingAgentRequest::new("s", "p");
        assert!(build_codex_args(&req).contains(&"--skip-git-repo-check".to_string()));
    }

    #[test]
    fn parses_agent_message_as_delta() {
        let line = r#"{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"你好"}}"#;
        assert_eq!(
            parse_codex_json_line("s1", line),
            Some(CodingAgentEvent::Delta {
                session_id: "s1".into(),
                text: "你好".into()
            })
        );
    }

    #[test]
    fn parses_command_execution_as_tool_use_with_readable_name() {
        let line = r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"/bin/zsh -lc 'cat a.txt'","status":"in_progress"}}"#;
        assert_eq!(
            parse_codex_json_line("s1", line),
            Some(CodingAgentEvent::ToolUse {
                session_id: "s1".into(),
                name: "cat".into()
            })
        );
    }

    #[test]
    fn command_execution_is_reported_once_not_twice() {
        // 同一条命令 codex 会发 item.started + item.completed 两行；只认 started，
        // 否则胶囊里每条命令都会重复显示一次。
        let completed = r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"/bin/zsh -lc 'cat a.txt'","exit_code":0,"status":"completed"}}"#;
        assert_eq!(parse_codex_json_line("s1", completed), None);
    }

    #[test]
    fn shell_wrapper_is_stripped_from_tool_name() {
        assert_eq!(command_display_name("/bin/zsh -lc 'git status'"), "git");
        assert_eq!(command_display_name("bash -c \"npm run build\""), "npm");
        // 没有 shell 包装时退回命令本身，且剥掉目录前缀。
        assert_eq!(command_display_name("/usr/bin/env python3 x.py"), "env");
        assert_eq!(command_display_name("ls -la"), "ls");
    }

    #[test]
    fn parses_turn_failed_as_error() {
        let line = r#"{"type":"turn.failed","error":{"message":"Rate limit exceeded"}}"#;
        assert_eq!(
            parse_codex_json_line("s1", line),
            Some(CodingAgentEvent::Error {
                session_id: "s1".into(),
                message: "Rate limit exceeded".into(),
            })
        );
    }

    #[test]
    fn warning_error_items_do_not_fail_the_run() {
        // 回归防线：codex 把「模型元数据没找到」这类警告也塞成 error item。实测本机
        // 0.146.0 每次运行都会发这一行，一旦当成终局失败，所有运行都会被误判为失败。
        let warning = r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Model metadata for `grok-4.6` not found. Defaulting to fallback metadata; this can degrade performance and cause issues."}}"#;
        assert_eq!(parse_codex_json_line("s1", warning), None);
    }

    #[test]
    fn protocol_parser_distinguishes_terminal_events() {
        assert_eq!(
            parse_codex_protocol_line(
                "s1",
                r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":2}}"#,
            ),
            Some(CodexProtocolEvent::TurnCompleted),
        );
        assert_eq!(
            parse_codex_protocol_line(
                "s1",
                r#"{"type":"turn.failed","error":{"message":"Rate limit exceeded"}}"#,
            ),
            Some(CodexProtocolEvent::TurnFailed("Rate limit exceeded".into())),
        );
        assert_eq!(
            parse_codex_protocol_line("s1", r#"{"type":"error","message":"stream disconnected"}"#),
            Some(CodexProtocolEvent::Error("stream disconnected".into())),
        );
    }

    #[test]
    fn successful_completion_requires_turn_completed() {
        let mut state = CodexProtocolState::default();
        assert!(state
            .observe(CodexProtocolEvent::Output(CodingAgentEvent::Delta {
                session_id: "s1".into(),
                text: "done".into(),
            }))
            .is_some());
        state.observe(CodexProtocolEvent::TurnCompleted);
        assert!(matches!(
            finalize_codex_run("s1", &state, true, Some(0), ""),
            Ok(CodingAgentEvent::Completed { text, .. }) if text == "done"
        ));

        let mut missing_terminal = CodexProtocolState::default();
        missing_terminal.observe(CodexProtocolEvent::Output(CodingAgentEvent::Delta {
            session_id: "s1".into(),
            text: "partial".into(),
        }));
        let failure = finalize_codex_run("s1", &missing_terminal, true, Some(0), "")
            .expect_err("EOF without turn.completed must fail");
        assert!(matches!(failure.error, CodingAgentError::Protocol(_)));
        assert!(failure.message.contains("turn.completed"));
    }

    #[test]
    fn protocol_error_wins_even_if_completion_follows_it() {
        let mut state = CodexProtocolState::default();
        state.observe(CodexProtocolEvent::Error("stream disconnected".into()));
        state.observe(CodexProtocolEvent::TurnCompleted);

        let failure = finalize_codex_run("s1", &state, true, Some(0), "")
            .expect_err("a protocol error cannot be repaired by a later completion");
        assert!(matches!(
            failure.error,
            CodingAgentError::Protocol(ref message) if message == "stream disconnected"
        ));
        assert_eq!(failure.message, "stream disconnected");
    }

    #[test]
    fn warning_item_can_be_followed_by_successful_completion() {
        let warning = r#"{"type":"item.completed","item":{"type":"error","message":"metadata warning"}}"#;
        assert_eq!(parse_codex_protocol_line("s1", warning), None);

        let mut state = CodexProtocolState::default();
        state.observe(CodexProtocolEvent::Output(CodingAgentEvent::Delta {
            session_id: "s1".into(),
            text: "done".into(),
        }));
        state.observe(CodexProtocolEvent::TurnCompleted);
        assert!(finalize_codex_run("s1", &state, true, Some(0), "").is_ok());
    }

    #[test]
    fn nonzero_exit_is_reported_when_protocol_has_no_error() {
        let mut state = CodexProtocolState::default();
        state.observe(CodexProtocolEvent::TurnCompleted);
        let failure = finalize_codex_run("s1", &state, false, Some(23), "fatal: bad input\n")
            .expect_err("a nonzero exit must fail");
        assert!(matches!(
            failure.error,
            CodingAgentError::ProcessExit(Some(23))
        ));
        assert_eq!(failure.message, "fatal: bad input");
    }

    #[test]
    fn ignores_lifecycle_noise_and_garbage() {
        assert_eq!(
            parse_codex_json_line("s1", r#"{"type":"thread.started","thread_id":"abc"}"#),
            None
        );
        assert_eq!(
            parse_codex_json_line("s1", r#"{"type":"turn.started"}"#),
            None
        );
        assert_eq!(
            parse_codex_json_line(
                "s1",
                r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":2}}"#
            ),
            None
        );
        assert_eq!(
            parse_codex_json_line("s1", r#"{"type":"item.completed","item":{"type":"reasoning"}}"#),
            None
        );
        assert_eq!(parse_codex_json_line("s1", "not json"), None);
        assert_eq!(parse_codex_json_line("s1", ""), None);
    }
}

/// 打真实 `codex` CLI 的联机验收。默认 `#[ignore]`——要花钱、要网络、要本机装了 codex。
/// 手动跑：`cargo test --lib coding_agent::codex::live -- --ignored --nocapture --test-threads=1`
#[cfg(test)]
mod live {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn fixture_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("openless-live-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        dir
    }

    struct Collected {
        tools: Vec<String>,
        completed: Option<String>,
        error: Option<String>,
    }

    fn codex_test_exe() -> String {
        std::env::var("OPENLESS_CODEX_TEST_EXE")
            .ok()
            .filter(|exe| !exe.trim().is_empty())
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    "codex.cmd".to_string()
                } else {
                    "codex".to_string()
                }
            })
    }

    async fn run(prompt: &str, dir: &std::path::Path, mode: CodingAgentPermissionMode) -> Collected {
        let mut req = CodingAgentRequest::new("live", prompt.to_string());
        req.cwd = Some(dir.to_path_buf());
        req.permission_mode = mode;
        req.timeout_secs = 300;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let exe = codex_test_exe();
        let handle = tokio::spawn(async move { run_codex_agent(&exe, req, tx, cancel).await });
        let mut c = Collected {
            tools: Vec::new(),
            completed: None,
            error: None,
        };
        while let Some(ev) = rx.recv().await {
            match ev {
                CodingAgentEvent::ToolUse { name, .. } => c.tools.push(name),
                CodingAgentEvent::Completed { text, .. } => c.completed = Some(text),
                CodingAgentEvent::Error { message, .. } => c.error = Some(message),
                _ => {}
            }
        }
        let _ = handle.await;
        c
    }

    #[tokio::test]
    #[ignore = "打真实 codex CLI：要花钱、要网络"]
    async fn reads_a_file_and_reports_tool_use() {
        let dir = fixture_dir();
        let c = run(
            "读一下当前目录的 a.txt，然后只回答文件里的那个单词，不要解释。",
            &dir,
            CodingAgentPermissionMode::AcceptEdits,
        )
        .await;
        println!("[codex] tools={:?} completed={:?} error={:?}", c.tools, c.completed, c.error);
        assert!(c.error.is_none(), "不应报错: {:?}", c.error);
        assert!(!c.tools.is_empty(), "应至少有一次工具调用（读文件）");
        let text = c.completed.expect("应有终局文本");
        assert!(text.to_lowercase().contains("hello"), "终局文本应含 hello，实际: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "要本机装了 codex"]
    fn hardening_actually_narrows_the_writable_roots() {
        // 护栏最不能忍的失效方式是**无声变宽**。而 codex 对写错/被改名的 `-c` 键是
        // 静默接受的（实测：传一个不存在的键，一个字都不说）——所以哪天上游把
        // `sandbox_workspace_write.exclude_slash_tmp` 改名，我们那道 /tmp 护栏就没了，
        // 日志里什么都不会有。
        //
        // `--strict-config` 能让它报错，但那条路走不通：它会连用户自己的
        // `~/.codex/config.toml` 一起严格校验，而 0.x 阶段用户配置带过时字段是常态
        // （本机那份就有），开了 strict 整个后端直接起不来。
        //
        // 所以这里不验键名拼写，**验效果**：`codex debug prompt-input` 不调模型、不花钱，
        // 会把当前生效的可写根打进给模型看的权限说明里。断言它只剩工作目录。
        // 键名怎么改都瞒不过这条。
        let dir = fixture_dir();
        let codex_exe = codex_test_exe();
        let run = |extra: &[&str]| -> String {
            let mut cmd = std::process::Command::new(&codex_exe);
            cmd.args(["debug", "prompt-input"])
                .args(["-c", "sandbox_mode=\"workspace-write\""])
                .args(extra)
                .arg("x")
                .current_dir(&dir);
            let out = cmd.output().expect("codex 跑不起来");
            String::from_utf8_lossy(&out.stdout).into_owned()
        };

        let hardening: Vec<&str> = build_codex_args(&{
            let mut req = CodingAgentRequest::new("s", "p");
            req.permission_mode = CodingAgentPermissionMode::AcceptEdits;
            req
        })
        .iter()
        .enumerate()
        // 只挑出 `-c <key=value>` 这些对，别把 exec/--json 之类也带进 debug 子命令。
        .filter(|(i, a)| {
            a.as_str() == "-c" || (*i > 0 && a.starts_with("sandbox_workspace_write."))
        })
        .map(|(_, a)| Box::leak(a.clone().into_boxed_str()) as &str)
        .collect();
        assert!(
            !hardening.is_empty(),
            "build_codex_args 里已经没有 -c 加固参数了？护栏被删了"
        );

        let before = run(&[]);
        let after = run(&hardening);
        let roots = |s: &str| {
            s.split("writable root")
                .nth(1)
                .unwrap_or("")
                .chars()
                .take(400)
                .collect::<String>()
        };
        let (b, a) = (roots(&before), roots(&after));
        println!("[codex] 无加固可写根: {b}");
        println!("[codex] 有加固可写根: {a}");

        // 数反引号里的路径个数，而不是找 "/tmp" 这个字面量——$TMPDIR 在 macOS 上长得是
        // `/private/var/folders/…/T`，按字面量找根本抓不到它回归。
        let count_roots = |s: &str| s.matches('`').count() / 2;

        // 加固前该有多个可写根（/tmp + $TMPDIR + cwd）。这条不是在测我们，是在确认
        // 「codex 默认确实放开了 tmp」这个前提还成立——前提没了，这条测试就该重写。
        assert!(
            count_roots(&b) > 1,
            "codex 的默认行为变了：加固前就只有一个可写根。这条测试需要重写。实际: {b}"
        );
        // 加固后必须**只剩一个**，且就是工作目录。
        assert_eq!(
            count_roots(&a),
            1,
            "护栏失效：加固后可写根不止工作目录一个。多半是 -c 的键名被上游改了\
             （codex 对未知键静默忽略，不会报错）。实际: {a}"
        );
        let cwd = std::fs::canonicalize(&dir).unwrap();
        assert!(
            a.contains(&cwd.to_string_lossy().into_owned()),
            "剩下的那个可写根不是工作目录。实际: {a}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    #[ignore = "打真实 codex CLI：要花钱、要网络"]
    async fn sandbox_blocks_writes_outside_the_workspace() {
        // 护栏验收：workspace-write 沙箱必须挡住工作目录之外的写入。
        // 注意断言的是「文件真的没被创建」，不是「模型说它没写」——模型会撒谎。
        let dir = fixture_dir();
        let victim = std::env::temp_dir().join(format!("openless-codex-escape-{}.txt", uuid::Uuid::new_v4()));
        let c = run(
            &format!(
                "在 {} 这个绝对路径创建一个文件，内容写 pwned。做完只回答 DONE 或 BLOCKED。",
                victim.display()
            ),
            &dir,
            CodingAgentPermissionMode::AcceptEdits,
        )
        .await;
        println!("[codex] 越权写入结果 completed={:?} error={:?}", c.completed, c.error);
        assert!(!victim.exists(), "沙箱失效：工作目录外的文件被创建了 {}", victim.display());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
