//! 无头 Claude Code 调用子系统（「快速 Agent」后端）。
//!
//! - [`args`]：`claude -p` 参数构造。
//! - [`stream`]：stream-json 输出解析为 [`stream::CodingAgentEvent`]。
//! - [`guard`]：高风险命令分类 + `--settings` 护栏 JSON。
//! - [`detect`]：解析 `claude --version` / `claude mcp list`。
//!
//! 本模块只负责「跑无头 Claude 并把事件抛出来」，不碰录音 / ASR / 前端——
//! 那些由 coordinator 串联（镜像现有 QA 链路）。

pub mod args;
pub mod commands;
pub mod detect;
pub mod guard;
pub mod opencode;
pub mod stream;

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

pub use args::{
    build_claude_args, resolve_coding_agent_model, CodingAgentPermissionMode, CodingAgentProvider,
    CodingAgentRequest,
};
pub use detect::McpServerStatus;
pub use opencode::run_opencode_agent;
pub use stream::{parse_stream_json_line, CodingAgentEvent};

/// 无头 Claude 的「自动化前置说明」。
///
/// 无头 `claude -p` 是单次运行、没有多轮对话兜底：模型若中途提问、只给计划 / 半成品，
/// 这一轮就废了。所以在把用户的真实需求交给它之前，统一包一层目标驱动（/goal 式）的
/// 自动化指令，要求它一口气把任务彻底做完、只回最终结果。所有走 [`run_claude_agent`]
/// 的「让 Claude 干活」入口都应该用它来构造 prompt。
pub fn autonomous_prompt(task: &str) -> String {
    format!(
        "【自动化任务 · 一次性完成】这是一次无人值守的单次无头运行，没有多轮对话机会，\
你无法事后追问或补充。请把下面的需求当成一个必须在本次运行内彻底达成的目标（等价于先 /goal \
设定目标与完成标准，再自主执行直到达成）：\n\
- 先想清楚目标和「完成」的判定标准，再开始动手；\n\
- 自主、连续地一口气执行到完全完成，不要中途停下来提问或等待确认；遇到歧义按最合理的方式继续；\n\
- 不要只给计划、思路或半成品，也不要留「后续步骤」给别人——要交付最终可用的结果；\n\
- 任务较长也要想办法在这一次运行内拆解并跑完；\n\
- 全部完成后，只输出最终结果本身，不要解释过程、不要前后缀、不要引号。\n\n\
需求：\n{task}"
    )
}

/// 运行器把事件投递到这个 sink（coordinator / 命令层再转成 Tauri event）。
pub type CodingAgentEventSink = tokio::sync::mpsc::UnboundedSender<CodingAgentEvent>;

#[derive(Debug, thiserror::Error)]
pub enum CodingAgentError {
    #[error("找不到可执行文件: {0}")]
    ExecutableNotFound(String),
    #[error("启动 agent 进程失败: {0}")]
    Spawn(String),
    #[error("agent 进程异常退出 (code={0:?})")]
    ProcessExit(Option<i32>),
    #[error("agent 运行超时 ({0}s)")]
    Timeout(u64),
    #[error("已取消")]
    Cancelled,
    #[error("IO 错误: {0}")]
    Io(String),
}

/// 给 GUI 进程补 PATH / HOME：macOS 从 Finder 启动的进程不继承登录 shell 环境，
/// `claude` 常装在 `~/.local/bin`、Homebrew 在 `/opt/homebrew/bin`。
pub(super) fn augment_env(cmd: &mut Command) {
    let mut path = std::env::var("PATH").unwrap_or_default();
    if let Some(home_os) = std::env::var_os("HOME") {
        let home = home_os.to_string_lossy().to_string();
        let extras = [
            format!("{home}/.local/bin"),
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
        ];
        for extra in extras {
            if !path.split(':').any(|p| p == extra) {
                path = if path.is_empty() {
                    extra
                } else {
                    format!("{extra}:{path}")
                };
            }
        }
        cmd.env("HOME", home);
    }
    cmd.env("PATH", path);
}

pub(super) fn augmented_command(exe: &str) -> Command {
    let mut cmd = Command::new(exe);
    augment_env(&mut cmd);
    cmd
}

/// `git stash create`：生成一个表示当前工作区的提交对象，**不改动工作区、也不进 stash 列表**。
/// 返回该快照的 commit SHA，供出问题时 `git stash apply <sha>` 回滚。无改动时返回 `None`。
pub fn create_git_snapshot(cwd: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["stash", "create", "openless-agent-pre-run"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

/// 探测 `claude` 版本（`None` 表示未安装或无法运行）。
pub async fn detect_claude(exe: &str) -> Option<String> {
    let out = augmented_command(exe)
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    detect::parse_claude_version(&String::from_utf8_lossy(&out.stdout))
}

/// 列出 Claude Code 已配置的 MCP server（含健康状态）。
pub async fn claude_mcp_list(exe: &str) -> Vec<McpServerStatus> {
    match augmented_command(exe).args(["mcp", "list"]).output().await {
        Ok(out) => detect::parse_mcp_list(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Vec::new(),
    }
}

pub(super) async fn wait_cancel(cancel: &Arc<AtomicBool>) {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// 无头跑一次 Claude：写 prompt 到 stdin，逐行解析 stream-json，把事件投到 `sink`。
/// 支持取消（`cancel` 置 true）与超时（`req.timeout_secs`），两者都会 kill 子进程。
pub async fn run_claude_agent(
    exe: &str,
    req: CodingAgentRequest,
    sink: CodingAgentEventSink,
    cancel: Arc<AtomicBool>,
) -> Result<(), CodingAgentError> {
    let args = build_claude_args(&req);
    let mut cmd = augmented_command(exe);
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

    // 写入 prompt 后立即关闭 stdin，触发 claude 开始处理。
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(req.prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    // 后台排空 stderr，避免管道写满导致子进程阻塞；出错时用作摘要。
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
    let mut got_terminal = false;
    let mut outcome: Result<(), CodingAgentError> = Ok(());

    loop {
        tokio::select! {
            biased;
            _ = wait_cancel(&cancel) => {
                let _ = child.start_kill();
                let _ = sink.send(CodingAgentEvent::Cancelled { session_id: req.session_id.clone() });
                got_terminal = true;
                outcome = Err(CodingAgentError::Cancelled);
                break;
            }
            _ = tokio::time::sleep_until(deadline) => {
                let _ = child.start_kill();
                let _ = sink.send(CodingAgentEvent::Error {
                    session_id: req.session_id.clone(),
                    message: format!("运行超时（{}s）", req.timeout_secs),
                });
                got_terminal = true;
                outcome = Err(CodingAgentError::Timeout(req.timeout_secs));
                break;
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        if let Some(ev) = parse_stream_json_line(&req.session_id, &l) {
                            if matches!(ev, CodingAgentEvent::Completed { .. } | CodingAgentEvent::Error { .. }) {
                                got_terminal = true;
                            }
                            let _ = sink.send(ev);
                        }
                    }
                    Ok(None) => break, // EOF
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
    if !status.success() && outcome.is_ok() {
        // 进程非 0 退出且我们还没判终局：补一条 Error。
        if !got_terminal {
            let stderr = match stderr_task {
                Some(t) => t.await.unwrap_or_default(),
                None => String::new(),
            };
            let summary = stderr.lines().last().unwrap_or("").trim().to_string();
            let _ = sink.send(CodingAgentEvent::Error {
                session_id: req.session_id.clone(),
                message: if summary.is_empty() {
                    format!("agent 异常退出 (code={:?})", status.code())
                } else {
                    summary
                },
            });
        }
        return Err(CodingAgentError::ProcessExit(status.code()));
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonomous_prompt_wraps_task_with_oneshot_directive() {
        let p = autonomous_prompt("把这段话翻译成英文：你好");
        // 原始需求必须原样带上。
        assert!(p.contains("把这段话翻译成英文：你好"));
        // 必含「一次性完成 / 单次无头运行 / 不要提问 / 只输出最终结果」这些核心约束。
        assert!(p.contains("一次性完成"));
        assert!(p.contains("无头"));
        assert!(p.contains("不要中途停下来提问"));
        assert!(p.contains("只输出最终结果"));
        // 需求要排在自动化说明之后（前置说明在前）。
        let directive_idx = p.find("自动化任务").unwrap();
        let task_idx = p.find("把这段话翻译成英文").unwrap();
        assert!(directive_idx < task_idx, "自动化前置说明必须在需求之前");
    }
}
