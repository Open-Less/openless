//! 「Claude 控制台」用到的 Tauri 命令：检测安装 / MCP 列表、护栏化流式测试运行、取消。
//!
//! 这些命令不碰录音 / coordinator，是「快速 Agent」引擎最小可用的垂直切片。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Window};

use super::detect::{has_computer_use_mcp, McpServerStatus};
use super::guard::build_guard_settings_json;
use super::opencode::{detect_opencode, list_opencode_models};
use super::{
    claude_mcp_list, create_git_snapshot, detect_claude, run_claude_agent,
    CodingAgentPermissionMode, CodingAgentRequest,
};

/// 当前测试运行的取消标志（一次只跑一个）。
static TEST_CANCEL: Lazy<Mutex<Option<Arc<AtomicBool>>>> = Lazy::new(|| Mutex::new(None));

/// 测试运行计数器，给每次运行一个唯一 session id（避免依赖时间戳）。
static TEST_COUNTER: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));

fn next_session_id() -> String {
    let mut c = TEST_COUNTER.lock();
    *c = c.wrapping_add(1);
    format!("console-{}", *c)
}

/// 仅允许裸名 "claude" 或规范化到已知安装目录下的绝对路径。
/// 拒绝包含路径分隔符的相对路径（如 "../../evil"）。
fn validate_exe(exe: &str) -> Result<(), String> {
    // 纯可执行文件名，不含任何路径分隔符 — 交给 PATH 解析即可
    if !exe.contains('/') && !exe.contains('\\') {
        if exe == "claude" {
            return Ok(());
        }
        return Err(format!(
            "不允许的可执行文件名: {exe}（只接受 'claude' 或已知安装目录下的绝对路径）"
        ));
    }
    // 绝对路径：必须规范化到已知 claude 安装目录之一
    let path = std::path::Path::new(exe);
    if !path.is_absolute() {
        return Err(format!("不允许的相对路径: {exe}"));
    }
    // 已知 claude 安装目录前缀
    let known_prefixes: &[&str] = &["/usr/local/bin/", "/usr/bin/", "/opt/homebrew/bin/"];
    // 也允许 ~/.local/bin/claude（用户目录绝对路径，动态计算）
    let home_prefix = std::env::var("HOME")
        .ok()
        .map(|h| format!("{h}/.local/bin/"));

    let exe_norm = exe.replace('\\', "/");
    let allowed = known_prefixes.iter().any(|p| exe_norm.starts_with(p))
        || home_prefix
            .as_deref()
            .map_or(false, |p| exe_norm.starts_with(p));
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "不允许的 claude 路径: {exe}（必须位于已知安装目录）"
        ))
    }
}

fn normalize_exe(exe: Option<String>) -> Result<String, String> {
    let exe = exe
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "claude".to_string());
    validate_exe(&exe)?;
    Ok(exe)
}

fn ensure_main_window(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("coding agent commands are only allowed from the main window".to_string())
    }
}

/// Claude Code 检测结果（回前端，camelCase）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDetectionWire {
    /// 是否检测到可运行的 claude。
    pub installed: bool,
    /// 版本号（如 "2.1.161"）。
    pub version: Option<String>,
    /// 实际使用的可执行文件名/路径。
    pub exe: String,
    /// 已配置的 MCP server 列表（含健康状态）。
    pub mcp_servers: Vec<McpServerStatus>,
    /// 是否检测到桌面控制类（computer use）MCP。
    pub has_computer_use: bool,
}

/// 检测 claude 是否安装、版本、已配置的 MCP server（即「computer use 技能」检测口径）。
#[tauri::command]
pub async fn coding_agent_detect(
    window: Window,
    exe: Option<String>,
) -> Result<ClaudeDetectionWire, String> {
    ensure_main_window(&window)?;
    let exe = normalize_exe(exe)?;
    let version = detect_claude(&exe).await;
    let mcp_servers = if version.is_some() {
        claude_mcp_list(&exe).await
    } else {
        Vec::new()
    };
    let has_computer_use = has_computer_use_mcp(&mcp_servers);
    Ok(ClaudeDetectionWire {
        installed: version.is_some(),
        version,
        exe,
        mcp_servers,
        has_computer_use,
    })
}

/// OpenCode 检测结果（回前端，camelCase）。issue #579。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeDetectionWire {
    /// 是否检测到可运行的 opencode。
    pub installed: bool,
    /// 版本号（如 "0.x.y"）。
    pub version: Option<String>,
    /// 实际使用的可执行文件名/路径。
    pub exe: String,
}

fn normalize_opencode_exe(exe: Option<String>) -> Result<String, String> {
    let exe = exe
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "opencode".to_string());
    if exe.contains("..") {
        return Err("不允许的可执行文件路径: 包含 '..'".into());
    }
    if (exe.contains('/') || exe.contains('\\')) && !std::path::Path::new(&exe).is_absolute() {
        return Err("不允许的相对路径，仅接受裸可执行文件名或绝对路径".into());
    }
    Ok(exe)
}

/// 检测 `opencode` 是否安装、版本。语音 Agent 选了 OpenCode 后端时，设置页据此提示
/// 用户是否需要先 `npm i -g opencode-ai` / 登录。
#[tauri::command]
pub async fn coding_agent_detect_opencode(
    window: Window,
    exe: Option<String>,
) -> Result<OpenCodeDetectionWire, String> {
    ensure_main_window(&window)?;
    let exe = normalize_opencode_exe(exe)?;
    let version = detect_opencode(&exe).await;
    Ok(OpenCodeDetectionWire {
        installed: version.is_some(),
        version,
        exe,
    })
}

/// 拉取 OpenCode 当前账号可用模型，供 Less Computer 设置页自动填充模型选择器。
#[tauri::command]
pub async fn coding_agent_list_opencode_models(
    window: Window,
    exe: Option<String>,
    refresh: Option<bool>,
) -> Result<Vec<String>, String> {
    ensure_main_window(&window)?;
    let exe = normalize_opencode_exe(exe)?;
    list_opencode_models(&exe, refresh.unwrap_or(true)).await
}

/// 护栏化地无头跑一次 claude，事件流式 emit 到前端 `coding-agent:test`。
///
/// 安全：附 `--settings`（acceptEdits + 高风险 deny）、`--max-budget-usd` 成本上限；
/// 若 workdir 是 git 仓库，运行前做一次 `git stash create` 快照（可回滚）。
#[tauri::command]
pub async fn coding_agent_run_test(
    window: Window,
    app: AppHandle,
    prompt: String,
    exe: Option<String>,
    permission_mode: Option<CodingAgentPermissionMode>,
    workdir: Option<String>,
    model: Option<String>,
    max_budget_usd: Option<f64>,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("指令为空".into());
    }
    let exe = normalize_exe(exe)?;
    let mode = permission_mode.unwrap_or_default();

    let cwd = workdir
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .map(std::path::PathBuf::from);

    // 运行前 git 快照（仅当是 git 仓库；非仓库返回 None，无副作用）。
    if let Some(dir) = &cwd {
        if let Some(sha) = create_git_snapshot(dir) {
            log::info!("[coding-agent] 运行前已生成 git 快照 {sha}（git stash apply 可回滚）");
        }
    }

    // 写护栏 settings 到临时文件。
    let settings_json = build_guard_settings_json(mode.as_cli_arg(), &[]);
    let settings_path = std::env::temp_dir().join(format!(
        "openless-claude-guard-{}.json",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&settings_json).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写护栏配置失败: {e}"))?;

    let mut req = CodingAgentRequest::new(next_session_id(), prompt);
    req.cwd = cwd;
    // 控制台测试默认走 sonnet：比用户默认的 Opus 便宜约一个数量级，足够验证连通与流式。
    req.model = model
        .filter(|m| !m.trim().is_empty())
        .or_else(|| Some("sonnet".to_string()));
    req.permission_mode = mode;
    req.max_budget_usd = max_budget_usd.or(Some(0.5));
    req.timeout_secs = 120;
    req.settings_json_path = Some(settings_path.clone());
    req.session_persistence = false;
    // 「放行 + 护栏」：允许轻动作与可恢复编辑；高风险由 deny 清单拦截。
    req.allowed_tools = vec![
        "Bash".into(),
        "Read".into(),
        "Edit".into(),
        "Write".into(),
        "Glob".into(),
        "Grep".into(),
        // 去掉 WebFetch：控制台 prompt 同样可被注入诱导 SSRF（与语音路径保持一致）。
        "WebSearch".into(),
    ];

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = Arc::new(AtomicBool::new(false));
    *TEST_CANCEL.lock() = Some(cancel.clone());

    let exe_for_task = exe.clone();
    let handle = tauri::async_runtime::spawn(async move {
        run_claude_agent(&exe_for_task, req, tx, cancel).await
    });

    // 边收边发：runner 结束会 drop sink，rx 收到 None 退出。
    while let Some(ev) = rx.recv().await {
        let _ = app.emit("coding-agent:test", &ev);
    }

    let run_result = handle.await;
    *TEST_CANCEL.lock() = None;
    let _ = std::fs::remove_file(&settings_path);

    match run_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(join_err) => Err(format!("agent 任务异常: {join_err}")),
    }
}

/// 取消当前正在跑的测试运行。
#[tauri::command]
pub fn coding_agent_cancel_test() {
    if let Some(flag) = TEST_CANCEL.lock().clone() {
        flag.store(true, Ordering::Relaxed);
    }
}

/// 本地预检一条命令是否高风险，返回原因（控制台在运行前给用户警示用）。
#[tauri::command]
pub fn coding_agent_command_risk(command: String) -> Option<String> {
    super::guard::is_high_risk_command(&command).map(|r| r.to_string())
}
