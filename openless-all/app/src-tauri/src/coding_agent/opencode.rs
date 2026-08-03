//! 无头 OpenCode（`opencode run`）适配器（issue #579）。
//!
//! 与 Claude Code 适配器（[`super`] 顶层 + [`super::args`] / [`super::stream`]）同形、复用
//! 同一套 [`CodingAgentRequest`] / [`CodingAgentEvent`] / [`CodingAgentError`]，但对接的是
//! OpenCode CLI（[opencode.ai](https://opencode.ai)）：
//!
//! - 检测：`opencode --version` → 复用 [`super::detect::parse_claude_version`] 取 `x.y.z`。
//! - 模型：`opencode models --refresh` → 拉取当前账号可用的 `provider/model` 列表。
//! - 运行：`opencode run --format json --model <provider/model> --dir <cwd>
//!   [--auto]`，**prompt 作为命令行参数**（OpenCode 从 argv 读，
//!   不像 claude 从 stdin），逐行解析 JSON 事件。
//! - 护栏：OpenCode 无 `--settings`，但支持 `permission` 配置；用
//!   `OPENCODE_CONFIG_CONTENT` 环境变量注入 inline 护栏 JSON（precedence 高于项目
//!   `opencode.json`），见 [`super::guard::build_opencode_guard_config`]。
//! - 输出：OpenCode 的 `text` 事件是「完成的文本块」（带 `part.time.end`）而非逐字 delta，
//!   且**没有**带 cost 的终局 `result` 事件——本模块在 EOF 处合成一条
//!   [`CodingAgentEvent::Completed`]，`cost_usd = None`（OpenCode CLI 不在 JSON 里给单次成本）。

use std::collections::HashSet;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use super::stream::CodingAgentEvent;
use super::{augmented_command, wait_cancel, CodingAgentEventSink, CodingAgentRequest};
use super::{CodingAgentError, CodingAgentPermissionMode};

/// 探测 `opencode` 版本（`None` 表示未安装或无法运行）。复用 claude 的 `x.y.z` 解析。
pub async fn detect_opencode(exe: &str) -> Option<String> {
    let out = augmented_command(exe)
        .arg("--version")
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    super::detect::parse_claude_version(&String::from_utf8_lossy(&out.stdout))
}

/// 去掉 OpenCode CLI 状态行里的 ANSI CSI 转义序列，保留普通文本。
fn strip_ansi_csi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' || chars.peek() != Some(&'[') {
            out.push(ch);
            continue;
        }
        let _ = chars.next();
        for code in chars.by_ref() {
            if ('@'..='~').contains(&code) {
                break;
            }
        }
    }
    out
}

/// 解析 `opencode models` 输出，只保留合法的 `provider/model`，保持 CLI 顺序并去重。
pub fn parse_opencode_models_output(stdout: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    strip_ansi_csi(stdout)
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && line.contains('/')
                && !line.chars().any(char::is_whitespace)
                && line.len() <= 256
        })
        .filter(|line| seen.insert((*line).to_string()))
        .map(str::to_string)
        .collect()
}

/// 从 OpenCode 拉取当前账号可用模型。`refresh=true` 时同步刷新 models.dev 缓存。
pub async fn list_opencode_models(exe: &str, refresh: bool) -> Result<Vec<String>, String> {
    let mut cmd = augmented_command(exe);
    cmd.arg("models").kill_on_drop(true);
    if refresh {
        cmd.arg("--refresh");
    }
    let output = tokio::time::timeout(Duration::from_secs(45), cmd.output())
        .await
        .map_err(|_| "OpenCode 模型拉取超时（45 秒）".to_string())?
        .map_err(|e| format!("无法运行 OpenCode 模型命令: {e}"))?;
    if !output.status.success() {
        let stderr = strip_ansi_csi(&String::from_utf8_lossy(&output.stderr));
        let summary = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("OpenCode 模型命令执行失败")
            .trim();
        return Err(summary.chars().take(512).collect());
    }
    let models = parse_opencode_models_output(&String::from_utf8_lossy(&output.stdout));
    if models.is_empty() {
        Err("OpenCode 没有返回可用模型，请先完成登录或配置模型提供商".to_string())
    } else {
        Ok(models)
    }
}

/// OpenCode 权限模式到 CLI 的映射。OpenCode 1.17 使用 `--auto` 自动批准未被配置显式
/// deny 的权限，没有 claude 的四档模式。
/// 我们靠注入的 `permission` 护栏配置（deny 高风险）做真正的拦截，因此除 `Plan` 外都需要
/// 加 skip 标志，否则无头下「ask」会卡死或被默认拒。`Plan` 不传 skip（OpenCode 自身只读）。
fn skip_permissions_flag(mode: CodingAgentPermissionMode) -> bool {
    !matches!(mode, CodingAgentPermissionMode::Plan)
}

/// 构造 `opencode run` 的命令行参数（不含可执行文件本身，也不含 prompt——prompt 在运行器
/// 里作为**最后一个位置参数**追加在末尾的 `--` 之后）。
///
/// 安全（参数注入防护）：参数列表以 `--`（end-of-options 标记）结尾，运行器把 prompt 接在
/// 其后。这样以 `-` / `--` 开头的 prompt（语音转写或被 prompt 注入诱导而成）都只会被当作
/// 位置参数，绝不会被 OpenCode 解析成 flag —— 杜绝经 prompt 混入 `--dangerously-skip-
/// permissions` 绕过护栏、`--dir` 改写工作目录等参数注入。model / cwd 作为各自 flag 的取值
/// 紧跟在 flag 之后，天然不会被当成独立 flag。
pub fn build_opencode_args(req: &CodingAgentRequest) -> Vec<String> {
    let mut args: Vec<String> = vec!["run".into(), "--format".into(), "json".into()];
    if let Some(model) = &req.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(cwd) = &req.cwd {
        args.push("--dir".into());
        args.push(cwd.to_string_lossy().into_owned());
    }
    if skip_permissions_flag(req.permission_mode) {
        args.push("--auto".into());
    }
    if req.continue_session {
        args.push("--continue".into());
    }
    // end-of-options：其后由运行器追加的 prompt 一律按位置参数处理，不再解析成 flag。
    args.push("--".into());
    args
}

/// 解析一行 `opencode run --format json` 的 NDJSON。
///
/// OpenCode 事件形如 `{"type":"text|tool_use|step_start|step_finish|reasoning|error",
/// "sessionID":"…","part":{…}|"error":…}`。我们只关心：
/// - `text` → `part.text`（完整文本块，作为 Delta 抛出；运行器累计成最终结果）。
/// - `tool_use` → `part.tool`（工具名）。
/// - `error` → `error`（字符串或对象）。
///
/// 其余（step_start/step_finish/reasoning/未知）忽略。解析失败返回 `None`，不 panic。
pub fn parse_opencode_json_line(session_id: &str, line: &str) -> Option<CodingAgentEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    match v.get("type")?.as_str()? {
        "text" => {
            let text = v.get("part")?.get("text")?.as_str()?.to_string();
            if text.is_empty() {
                return None;
            }
            Some(CodingAgentEvent::Delta {
                session_id: session_id.to_string(),
                text,
            })
        }
        "tool_use" => {
            let name = v.get("part")?.get("tool")?.as_str()?.to_string();
            Some(CodingAgentEvent::ToolUse {
                session_id: session_id.to_string(),
                name,
            })
        }
        "error" => {
            let message = v
                .pointer("/error/data/message")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    v.pointer("/error/message")
                        .and_then(serde_json::Value::as_str)
                })
                .or_else(|| v.get("error").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| "OpenCode 返回了未知错误".to_string());
            Some(CodingAgentEvent::Error {
                session_id: session_id.to_string(),
                message,
            })
        }
        _ => None,
    }
}

/// 无头跑一次 OpenCode：prompt 作为 argv，逐行解析 NDJSON，把事件投到 `sink`。
/// 护栏 JSON 经 `OPENCODE_CONFIG_CONTENT` 注入。支持取消与超时（都会 kill 子进程）。
///
/// `guard_config_json`：[`super::guard::build_opencode_guard_config`] 的产物。`None` 表示
/// 不注入护栏（仅 Console 显式无护栏场景用；语音路径必须传 Some，调用方 fail-closed）。
pub async fn run_opencode_agent(
    exe: &str,
    req: CodingAgentRequest,
    guard_config_json: Option<String>,
    sink: CodingAgentEventSink,
    cancel: Arc<AtomicBool>,
) -> Result<(), CodingAgentError> {
    let args = build_opencode_args(&req);
    let mut cmd = augmented_command(exe);
    cmd.args(&args)
        // prompt 作为最后的位置参数（OpenCode 从 argv 读取 message）。
        .arg(&req.prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = &req.cwd {
        cmd.current_dir(cwd);
    }
    if let Some(cfg) = &guard_config_json {
        cmd.env("OPENCODE_CONFIG_CONTENT", cfg);
    }

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CodingAgentError::ExecutableNotFound(exe.to_string())
        } else {
            CodingAgentError::Spawn(e.to_string())
        }
    })?;

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
    // OpenCode 没有带最终文本/成本的 result 事件：累计所有 text 块，EOF 时合成 Completed。
    let mut accumulated = String::new();
    let mut got_error = false;
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
                got_error = true;
                outcome = Err(CodingAgentError::Timeout(req.timeout_secs));
                break;
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        if let Some(ev) = parse_opencode_json_line(&req.session_id, &l) {
                            match &ev {
                                CodingAgentEvent::Delta { text, .. } => accumulated.push_str(text),
                                CodingAgentEvent::Error { .. } => got_error = true,
                                _ => {}
                            }
                            let _ = sink.send(ev);
                        }
                    }
                    Ok(None) => break, // EOF：正常结束
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

    // 正常 EOF 收尾（没取消/超时/IO 错）：进程成功且没报错 → 合成 Completed。
    if outcome.is_ok() {
        if status.success() && !got_error {
            let _ = sink.send(CodingAgentEvent::Completed {
                session_id: req.session_id.clone(),
                text: accumulated.trim().to_string(),
                cost_usd: None,
                duration_ms: None,
            });
            return Ok(());
        }
        if !status.success() && !got_error {
            // 非 0 退出且没解析到 error 事件：补一条 Error（取 stderr 末行作摘要）。
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
            return Err(CodingAgentError::ProcessExit(status.code()));
        }
    }

    outcome
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
    fn run_args_are_json_format_and_prompt_not_in_argv() {
        let req = CodingAgentRequest::new("s1", "hello world");
        let args = build_opencode_args(&req);
        assert_eq!(args.first().map(|s| s.as_str()), Some("run"));
        assert_eq!(arg_value(&args, "--format"), Some("json"));
        // prompt 不进 build_opencode_args（运行器单独追加）。
        assert!(!args.iter().any(|a| a.contains("hello world")));
        // 参数注入防护：末尾必须是 `--`，运行器把 prompt 接在其后。
        assert_eq!(args.last().map(|s| s.as_str()), Some("--"));
    }

    #[test]
    fn end_of_options_marker_is_last_even_with_flags() {
        // 即便带 --model / --dir / --auto，`--` 仍在最末，
        // 保证以 `-` 开头的 prompt 不会被解析成 flag。
        let mut req = CodingAgentRequest::new("s", "p");
        req.model = Some("anthropic/claude-sonnet-4".into());
        req.cwd = Some(PathBuf::from("/tmp/work"));
        req.permission_mode = CodingAgentPermissionMode::AcceptEdits;
        let args = build_opencode_args(&req);
        assert_eq!(args.last().map(|s| s.as_str()), Some("--"));
        // `--` 之前才是各 flag，`--auto` 不在 `--` 之后。
        let dd = args.iter().rposition(|a| a == "--").unwrap();
        assert!(args[..dd].iter().any(|a| a == "--auto"));
    }

    #[test]
    fn plan_mode_omits_skip_permissions_other_modes_add_it() {
        let mut req = CodingAgentRequest::new("s", "p");
        req.permission_mode = CodingAgentPermissionMode::Plan;
        assert!(!build_opencode_args(&req).contains(&"--auto".to_string()));
        req.permission_mode = CodingAgentPermissionMode::AcceptEdits;
        assert!(build_opencode_args(&req).contains(&"--auto".to_string()));
        req.permission_mode = CodingAgentPermissionMode::BypassPermissions;
        assert!(build_opencode_args(&req).contains(&"--auto".to_string()));
    }

    #[test]
    fn continue_session_uses_opencode_continue_flag() {
        let mut req = CodingAgentRequest::new("s", "p");
        req.continue_session = true;
        let args = build_opencode_args(&req);
        assert!(args.contains(&"--continue".to_string()));
        assert_eq!(args.last().map(|s| s.as_str()), Some("--"));
    }

    #[test]
    fn parses_refreshed_models_and_removes_ansi_status() {
        let stdout = "\u{1b}[92m\u{1b}[1mModels cache refreshed\u{1b}[0m\n\
opencode/deepseek-v4-flash-free\n\
minimax/MiniMax-M2.7\n\
opencode/deepseek-v4-flash-free\n";
        assert_eq!(
            parse_opencode_models_output(stdout),
            vec![
                "opencode/deepseek-v4-flash-free".to_string(),
                "minimax/MiniMax-M2.7".to_string(),
            ]
        );
    }

    #[test]
    fn parses_nested_opencode_error_message() {
        let line = r#"{"type":"error","error":{"name":"UnknownError","data":{"message":"Rate limit exceeded"}}}"#;
        assert_eq!(
            parse_opencode_json_line("s1", line),
            Some(CodingAgentEvent::Error {
                session_id: "s1".into(),
                message: "Rate limit exceeded".into(),
            })
        );
    }

    #[test]
    fn model_and_dir_flags_emitted_when_set() {
        let mut req = CodingAgentRequest::new("s", "p");
        req.model = Some("anthropic/claude-sonnet-4".into());
        req.cwd = Some(PathBuf::from("/tmp/work"));
        let args = build_opencode_args(&req);
        assert_eq!(
            arg_value(&args, "--model"),
            Some("anthropic/claude-sonnet-4")
        );
        assert_eq!(arg_value(&args, "--dir"), Some("/tmp/work"));
    }

    #[test]
    fn parses_text_part_as_delta() {
        let line = r#"{"type":"text","sessionID":"x","part":{"type":"text","text":"你好"}}"#;
        assert_eq!(
            parse_opencode_json_line("s1", line),
            Some(CodingAgentEvent::Delta {
                session_id: "s1".into(),
                text: "你好".into()
            })
        );
    }

    #[test]
    fn parses_tool_use_part() {
        let line = r#"{"type":"tool_use","sessionID":"x","part":{"type":"tool","tool":"bash"}}"#;
        assert_eq!(
            parse_opencode_json_line("s1", line),
            Some(CodingAgentEvent::ToolUse {
                session_id: "s1".into(),
                name: "bash".into()
            })
        );
    }

    #[test]
    fn parses_error_event() {
        let line = r#"{"type":"error","sessionID":"x","error":"boom"}"#;
        assert_eq!(
            parse_opencode_json_line("s1", line),
            Some(CodingAgentEvent::Error {
                session_id: "s1".into(),
                message: "boom".into()
            })
        );
    }

    #[test]
    fn ignores_step_and_reasoning_and_garbage() {
        assert_eq!(
            parse_opencode_json_line("s1", r#"{"type":"step_start","part":{}}"#),
            None
        );
        assert_eq!(
            parse_opencode_json_line("s1", r#"{"type":"reasoning","part":{"text":"…"}}"#),
            None
        );
        assert_eq!(parse_opencode_json_line("s1", "not json"), None);
        assert_eq!(parse_opencode_json_line("s1", ""), None);
    }
}
