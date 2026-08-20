//! 无头 coding agent 调用子系统（「快速 Agent」/ Less Computer 后端）。
//!
//! 四个后端各有一个同形的适配器，共用 [`args::CodingAgentRequest`] /
//! [`stream::CodingAgentEvent`] / [`CodingAgentError`]：
//!
//! | 后端 | 适配器 | prompt 入口 | 护栏 |
//! |---|---|---|---|
//! | Claude Code | 本模块顶层 + [`args`] / [`stream`] | stdin | `--settings` 逐命令 deny 清单 |
//! | OpenCode | [`opencode`] | argv（`--` 之后） | `OPENCODE_CONFIG_CONTENT` deny 清单 |
//! | Codex | [`codex`] | stdin（`-`） | 自带 seatbelt 沙箱 `-s` |
//! | dsh | [`dsh`] | patch 文件（不进 argv） | `DSH_PERMISSION_MODE` 沙箱 |
//!
//! 后两家**给不了逐命令 deny 清单**，护栏落在各自的沙箱档位上；对应地，
//! 「撞了 deny → 弹审批卡 → 放行重跑」这条链路对它们不生效，撞墙时如实报错。
//!
//! - [`args`]：`claude -p` 参数构造。
//! - [`stream`]：stream-json 输出解析为 [`stream::CodingAgentEvent`]。
//! - [`guard`]：高风险命令分类 + `--settings` 护栏 JSON。
//! - [`detect`]：解析 `claude --version` / `claude mcp list`。
//!
//! 本模块只负责「跑无头 Claude 并把事件抛出来」，不碰录音 / ASR / 前端——
//! 那些由 coordinator 串联（镜像现有 QA 链路）。

pub mod args;
pub mod codex;
pub mod commands;
pub mod detect;
pub mod dsh;
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
pub use codex::run_codex_agent;
pub use detect::McpServerStatus;
pub use dsh::run_dsh_agent;
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
    #[error("agent 协议错误: {0}")]
    Protocol(String),
    #[error("agent 运行超时 ({0}s)")]
    Timeout(u64),
    #[error("已取消")]
    Cancelled,
    #[error("IO 错误: {0}")]
    Io(String),
}

/// 登录 shell 的 PATH，整个进程只解析一次（解析失败缓存 `None`，不反复重试）。
#[cfg(unix)]
static LOGIN_SHELL_PATH: tokio::sync::OnceCell<Option<String>> = tokio::sync::OnceCell::const_new();

/// 从 shell 里回读 PATH 用的哨兵串。
///
/// 交互式 shell 的 rc 文件可能自己往 stdout 打东西（提示、版本横幅、插件问候），
/// 直接取整个 stdout 会把这些一起当成 PATH。加个哨兵，只取它之后的内容。
#[cfg(unix)]
const SHELL_PATH_SENTINEL: &str = "__OPENLESS_PATH__";

/// 跑一次 shell 把 PATH 打回来。`flags` 形如 `-lic` / `-lc`。超时返回 `None`。
#[cfg(unix)]
async fn probe_shell_path(
    shell: &str,
    flags: &str,
    deadline: tokio::time::Instant,
) -> Option<String> {
    // 注意 `%s` 是给 shell 的 printf 用的字面量，`{SHELL_PATH_SENTINEL}` 才是 Rust 插值。
    let script = format!("printf '{SHELL_PATH_SENTINEL}%s' \"$PATH\"");
    let mut cmd = Command::new(shell);
    cmd.args([flags, &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout_at(deadline, cmd.output())
        .await
        .ok()?
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // 只取哨兵之后的内容；没有哨兵说明这次输出不可信。
    let path = stdout
        .rsplit(SHELL_PATH_SENTINEL)
        .next()?
        .trim()
        .to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// 问用户的 shell 要一份真实 PATH。**仅 unix**。
///
/// macOS 从 Finder / LaunchServices 启动的 GUI 进程拿到的是一份极简 PATH，不经过用户的 shell
/// 配置，所以任何由版本管理器安装的 CLI 都不在里面。这对本模块是致命的：`dsh` 常在 nvm 的
/// `~/.nvm/versions/node/<版本>/bin`、`codex` 常在全局 npm 前缀——这些路径**无法硬编码**
/// （带版本号、随配置变）。唯一可靠的办法是问 shell 自己。
///
/// 先试 `-lic`（登录 + 交互）再退 `-lc`（仅登录），因为 **nvm 这类版本管理器通常在 `.zshrc`
/// 里初始化，而 `.zshrc` 只有交互式 shell 才读**。本机实测：`-lc` 拿到的 PATH 里 nvm 排在
/// `/usr/local/bin` 之后，于是 `dsh` 的 `#!/usr/bin/env node` 命中了 v20 的 node 直接崩；
/// `-lic` 才把 nvm v24 排到最前。这个顺序是正确性问题，别为了「更安全」改回 `-lc`。
///
/// Windows 上没有「登录 shell 的 PATH」这个概念（进程直接继承系统/用户环境变量），而且装了
/// Git Bash 的机器 `SHELL` 也可能有值——真去跑它只会莫名其妙拉起一个 bash。所以整段限定 unix。
#[cfg(unix)]
async fn login_shell_path() -> Option<&'static str> {
    LOGIN_SHELL_PATH
        .get_or_init(|| async {
            let shell = std::env::var("SHELL")
                .ok()
                .filter(|s| !s.trim().is_empty())?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            if let Some(path) = probe_shell_path(&shell, "-lic", deadline).await {
                Some(path)
            } else {
                probe_shell_path(&shell, "-lc", deadline).await
            }
        })
        .await
        .as_deref()
}

#[cfg(not(unix))]
async fn login_shell_path() -> Option<&'static str> {
    None
}

/// 静态兜底目录：登录 shell 问不出来时至少覆盖几个常见落点。
///
/// 只对 unix 有意义（Windows 的 CLI 不落在这些位置，进程也直接继承系统 PATH）。
/// 注意这份清单**盖不全** nvm / 全局 npm 前缀这类带版本号或可配置的路径——那是
/// [`login_shell_path`] 的活。用户还可以在「高级 → Less Computer」直接填可执行文件绝对路径，
/// 那条路绕过 PATH 解析，是最终兜底。
fn static_path_extras(home: &str) -> Vec<String> {
    if cfg!(not(unix)) {
        return Vec::new();
    }
    vec![
        format!("{home}/.local/bin"),
        // opencode 官方安装脚本的默认落点。
        format!("{home}/.opencode/bin"),
        // 常见的全局 npm 前缀写法（codex / dsh 都可能装在这里）。
        format!("{home}/.npm-global/bin"),
        format!("{home}/.bun/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
    ]
}

/// 按优先级拼出最终 PATH：登录 shell 的 PATH > 静态兜底目录 > 进程原有 PATH。
/// 同一目录只保留第一次出现，**各段内部的相对顺序原样保留**。
///
/// 分隔符走 [`std::env::split_paths`] / [`std::env::join_paths`]，**不能手写 `:`**：
/// Windows 用 `;`，而且盘符本身带冒号——按 `:` 切 `C:\Windows;C:\System32` 会把整条 PATH
/// 打烂，之后所有子进程都找不到任何命令。（这条是踩出来的：早先的实现就是手写 `:` 切分再
/// 用 `:` 拼回去。CI 在 Windows 上只跑 `cargo check`，编译得过，但跑起来就废。）
///
/// 顺序在这里同样是正确性问题：早先的实现把每一项依次前插，等于把登录 shell 的 PATH 整个
/// **倒过来**，于是 `dsh`（装在 nvm 某个版本的 node_modules 下）被交给了错版本的 node，
/// 一启动就崩。所以这个函数必须保序，而且必须有下面那几条单测盯着。
fn merge_path(current: &str, extras: &[String], shell_path: Option<&str>) -> String {
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    let mut push_all = |raw: &str| {
        for seg in std::env::split_paths(raw) {
            // 空段在 PATH 里等价于「当前目录」，是个安全隐患，丢掉。
            if seg.as_os_str().is_empty() {
                continue;
            }
            if seen.insert(seg.clone()) {
                out.push(seg);
            }
        }
    };
    if let Some(sp) = shell_path {
        push_all(sp);
    }
    for extra in extras {
        push_all(extra);
    }
    push_all(current);
    // join_paths 只在段里含分隔符时才失败；真失败就退回原 PATH，绝不返回半截的。
    std::env::join_paths(&out)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|_| current.to_string())
}

/// 给 GUI 进程补 PATH / HOME：macOS 从 Finder 启动的进程不继承登录 shell 环境。
pub(super) async fn augment_env(cmd: &mut Command) {
    let current = std::env::var("PATH").unwrap_or_default();
    let extras = match std::env::var_os("HOME") {
        Some(home_os) => {
            let home = home_os.to_string_lossy().to_string();
            let extras = static_path_extras(&home);
            cmd.env("HOME", home);
            extras
        }
        None => Vec::new(),
    };
    cmd.env(
        "PATH",
        merge_path(&current, &extras, login_shell_path().await),
    );
}

pub(super) async fn augmented_command(exe: &str) -> Command {
    let mut cmd = Command::new(exe);
    augment_env(&mut cmd).await;
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

/// 一次 `<cli> --version` 探测的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliProbe {
    /// **命令跑通了**（进程起得来且 exit 0）。这才是「装没装」的判据。
    pub installed: bool,
    /// 解析出来的版本号，**纯展示用**。解析不出来不影响 [`Self::installed`]。
    pub version: Option<String>,
}

/// 探测某个 agent CLI：跑 `<exe> --version`。四个后端共用。
///
/// **判据是「命令跑通」，不是「解析出版本号」**——这两件事分开，是踩过一次才分开的。
/// 早先用 `version.is_some()` 当 installed，于是 dsh 的 `0.1.0-rc.6` 解析不出来（旧解析器
/// 不认预发布后缀）就被判成「没装」，设置页一直报「未检测到 dsh 命令」，而 dsh 明明装着。
///
/// 版本字符串是这些 CLI 里**最容易变**的东西：改排版、加后缀、换前缀，对它们都不算
/// breaking change，但对「拿版本号当判据」的我们就是。所以：跑通 = 装了；版本号只用来显示，
/// 解析失败最多让界面少显示一个号码，不会把能用的后端说成没装。
pub async fn probe_cli(exe: &str) -> CliProbe {
    let missing = CliProbe {
        installed: false,
        version: None,
    };
    // 进程起不来（找不到可执行文件 / 没有执行权限）——这才是真的没装。
    let mut cmd = augmented_command(exe).await;
    let Ok(out) = cmd.arg("--version").output().await else {
        return missing;
    };
    if !out.status.success() {
        return missing;
    }
    let version = detect::parse_cli_version(&String::from_utf8_lossy(&out.stdout));
    if version.is_none() {
        // 能跑通但版本号读不出来：多半是上游改了 --version 的排版。不影响可用性，
        // 但值得留一笔——下次有人问「为什么版本号显示成问号」时这行就是答案。
        log::info!("[coding-agent] {exe} --version 跑通了但版本号解析不出来，仅影响显示");
    }
    CliProbe {
        installed: true,
        version,
    }
}

/// 列出 Claude Code 已配置的 MCP server（含健康状态）。
pub async fn claude_mcp_list(exe: &str) -> Vec<McpServerStatus> {
    let mut cmd = augmented_command(exe).await;
    match cmd.args(["mcp", "list"]).output().await {
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

    /// 按当前平台的 PATH 分隔符拼一串（Windows `;` / unix `:`），
    /// 让下面的断言在三个平台上都成立——手写 `:` 的话 Windows CI 会挂。
    fn path_str(parts: &[&str]) -> String {
        std::env::join_paths(parts)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn path_parts(joined: &str) -> Vec<String> {
        std::env::split_paths(joined)
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn merged_path_keeps_login_shell_order_intact() {
        // 回归防线：登录 shell 的 PATH 顺序**必须原样保留**。倒过来会让 `node` /
        // `dsh` 命中错误的版本管理器目录（本机同时装了三个 node），子进程直接崩。
        let shell = path_str(&["/nvm/v24/bin", "/opt/homebrew/bin", "/usr/bin"]);
        let merged = merge_path(&path_str(&["/usr/bin", "/bin"]), &[], Some(&shell));
        let order = path_parts(&merged);
        assert_eq!(order[0], "/nvm/v24/bin", "登录 shell 的首项必须还是首项");
        assert_eq!(order[1], "/opt/homebrew/bin");
        assert_eq!(order[2], "/usr/bin");
    }

    #[test]
    fn merged_path_priority_is_shell_then_extras_then_current() {
        let merged = merge_path(
            &path_str(&["/current/bin"]),
            &["/extra/bin".to_string()],
            Some(&path_str(&["/shell/bin"])),
        );
        assert_eq!(
            path_parts(&merged),
            vec!["/shell/bin", "/extra/bin", "/current/bin"]
        );
    }

    #[test]
    fn merged_path_dedupes_and_drops_empties() {
        // 重复目录只留第一次出现（优先级最高的那次）。
        let merged = merge_path(
            &path_str(&["/a", "/b", "/a"]),
            &["/b".to_string()],
            Some(&path_str(&["/b", "/c"])),
        );
        assert_eq!(path_parts(&merged), vec!["/b", "/c", "/a"]);
    }

    #[test]
    fn merged_path_uses_the_platform_separator_not_a_hardcoded_colon() {
        // 跨平台不变量：输出必须逐字等于 `join_paths` 的结果，也就是用**平台自己**的
        // 分隔符（Windows `;` / unix `:`）。早先的实现手写 `:` 切分再用 `:` 拼回去，
        // 在 Windows 上会把 `C:\Windows` 切成 `C` 和 `\Windows`，整条 PATH 报废。
        // 这条在三个平台都会跑，任何一处退回手写分隔符都会当场红。
        let merged = merge_path(&path_str(&["/a", "/b"]), &[], None);
        assert_eq!(merged, path_str(&["/a", "/b"]));
    }

    /// 盘符（`C:`）这条只有 Windows 能构造——unix 的 PATH 段里不允许出现 `:`，
    /// `join_paths` 会直接拒绝。所以它在 Windows CI 上跑，unix 上编译掉。
    #[cfg(windows)]
    #[test]
    fn merged_path_keeps_drive_letters_intact() {
        let current = path_str(&["C:\\Windows", "C:\\Windows\\System32"]);
        let parts = path_parts(&merge_path(&current, &[], None));
        assert_eq!(parts.len(), 2, "段数不能变，实际: {parts:?}");
        assert!(!parts.iter().any(|p| p == "C"), "盘符被切开了: {parts:?}");
    }

    #[tokio::test]
    async fn missing_executable_is_not_installed() {
        let probe = probe_cli("openless-definitely-not-a-real-binary-xyz").await;
        assert!(!probe.installed);
        assert_eq!(probe.version, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_but_unparseable_version_still_counts_as_installed() {
        // 这条是 dsh 那个 bug 的**形状**：命令跑得通，但 `--version` 的输出里没有可解析的
        // 版本号。旧实现（installed = version.is_some()）会把它判成「没装」，设置页就报
        // 「未检测到 xxx 命令」——而命令明明在。
        //
        // `echo --version` 会原样打印 "--version" 并 exit 0，正好是这个形状，
        // 而且不依赖装了哪些 agent CLI，任何 unix 机器都能跑。
        let probe = probe_cli("echo").await;
        assert!(
            probe.installed,
            "命令跑通了就该算装了，哪怕版本号读不出来"
        );
        assert_eq!(probe.version, None, "这里本来就不该解析出版本号");
    }

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
