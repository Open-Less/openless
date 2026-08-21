//! 无头 Claude Code（`claude -p`）调用参数构造。
//!
//! 纯逻辑：把一个 [`CodingAgentRequest`] 翻译成 `claude` 的命令行参数列表。
//! prompt 本身**不**进 argv（避免出现在进程列表里泄露），由运行器写进 stdin。

use std::path::PathBuf;

/// 后端 coding agent 提供商，对应 `UserPreferences.coding_agent_provider` 的取值。
/// 未知/缺省一律回落 Claude（既有默认），不破坏现有用户。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingAgentProvider {
    /// Claude Code CLI（`claude`）。默认。
    ClaudeCodeCli,
    /// OpenCode CLI（`opencode`），issue #579。
    OpenCodeCli,
    /// Codex CLI（`codex exec`）。护栏走它自带的 seatbelt 沙箱，没有逐命令 deny 清单。
    CodexCli,
    /// dsh / DeepSeek Harness（`dsh --profile headless`）。护栏走 `DSH_PERMISSION_MODE`
    /// 三档沙箱；流式与工具事件靠我们自带的 tap 插件（见 [`super::dsh`]）。
    DshCli,
}

impl CodingAgentProvider {
    /// 从 prefs 字符串解析。未知/缺省一律回落 Claude（既有默认），不破坏现有用户。
    pub fn from_pref(s: &str) -> Self {
        match s.trim() {
            "opencode-cli" => Self::OpenCodeCli,
            "codex-cli" => Self::CodexCli,
            "dsh-cli" => Self::DshCli,
            _ => Self::ClaudeCodeCli,
        }
    }

    /// 该后端是否支持「撞了 deny → 弹审批卡 → 放行该命令重跑」这条链路。
    ///
    /// Claude / OpenCode 的护栏是**逐命令 deny 清单**，能精确地把某一条放行再跑一次，
    /// 所以审批卡有意义。Codex / dsh 只有粗粒度沙箱档位，放行的唯一办法是整体降档
    /// （等于把护栏关掉），不是「放行这一条」。对它们弹审批卡会给用户一个假承诺：
    /// 点了批准，重跑还是同样被拦。所以这两家直接如实报错，不弹卡。
    pub fn supports_command_approval(self) -> bool {
        match self {
            Self::ClaudeCodeCli | Self::OpenCodeCli => true,
            Self::CodexCli | Self::DshCli => false,
        }
    }

    /// 该 provider 默认的可执行文件名。
    pub fn default_exe(self) -> &'static str {
        match self {
            Self::ClaudeCodeCli => "claude",
            Self::OpenCodeCli => "opencode",
            Self::CodexCli => "codex",
            Self::DshCli => "dsh",
        }
    }

    /// 该后端可声明的单次美元预算上限；`None` 表示 CLI 没有可用的美元硬上限。
    pub fn max_budget_usd(self) -> Option<f64> {
        match self {
            Self::ClaudeCodeCli => Some(2.0),
            Self::OpenCodeCli | Self::CodexCli | Self::DshCli => None,
        }
    }
}

/// 按后端解析用户选择的模型。
///
/// - Claude：保持既有的 sonnet 默认。
/// - OpenCode：只接受 `provider/model`，未选择或遗留的 Claude 别名均交给 OpenCode 自己的默认配置。
/// - Codex：接受任意非空裸模型名（`gpt-5` / `o3` 等），留空交给 `~/.codex/config.toml`。
/// - dsh：**永远返回 `None`**。headless profile 没有 `--model` 这个 flag，模型由 profile 的
///   `agent-default-model` 插件决定；这里返回 Some 只会让调用方以为选得动。
pub fn resolve_coding_agent_model(
    provider: CodingAgentProvider,
    configured: Option<String>,
) -> Option<String> {
    let configured = configured
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty());
    match provider {
        CodingAgentProvider::ClaudeCodeCli => configured.or_else(|| Some("sonnet".to_string())),
        CodingAgentProvider::OpenCodeCli => configured.filter(|model| model.contains('/')),
        CodingAgentProvider::CodexCli => configured,
        CodingAgentProvider::DshCli => None,
    }
}

/// Claude Code 权限模式，对应 CLI `--permission-mode` 的取值（已对本机 v2.1.161 核实）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodingAgentPermissionMode {
    /// 只读/计划模式：不改文件。
    Plan,
    /// 默认：每个动作都要确认（无头下等于大多被拒，少用）。
    Default,
    /// 放行可恢复的编辑/动作（本项目「放行 + 护栏」的默认）。
    AcceptEdits,
    /// 跳过所有权限检查——仅高级区，绝不做默认。
    BypassPermissions,
}

impl CodingAgentPermissionMode {
    /// 传给 `--permission-mode` 的字符串。
    pub fn as_cli_arg(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::BypassPermissions => "bypassPermissions",
        }
    }
}

impl Default for CodingAgentPermissionMode {
    fn default() -> Self {
        Self::AcceptEdits
    }
}

/// 一次无头 agent 运行的完整请求。
#[derive(Debug, Clone)]
pub struct CodingAgentRequest {
    /// 会话标识，用于丢弃迟到事件。
    pub session_id: String,
    /// 最终发给 Claude 的指令（写入 stdin，不进 argv）。
    pub prompt: String,
    /// 工作目录；同时作为 `--add-dir` 限定文件作用域。
    pub cwd: Option<PathBuf>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub permission_mode: CodingAgentPermissionMode,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    /// 单次运行成本硬上限（`--max-budget-usd`）。
    pub max_budget_usd: Option<f64>,
    /// 运行超时（秒）。
    pub timeout_secs: u64,
    /// 额外系统提示词（`--append-system-prompt`）。
    pub extra_system_prompt: Option<String>,
    /// 护栏 settings JSON 文件路径（`--settings`）。
    pub settings_json_path: Option<PathBuf>,
    /// 是否保留会话（false 时加 `--no-session-persistence`，快取用走 false 更快）。
    pub session_persistence: bool,
    /// 续接当前 Less Computer 会话。支持原生恢复的后端翻译成各自的 continue/resume；
    /// 不支持的后端可消费 [`Self::continuation_context`] 做有界文本回放。
    pub continue_session: bool,
    /// 给不支持原生会话恢复的后端使用的有界文本历史。原生 resume 后端忽略。
    pub continuation_context: Option<String>,
}

impl CodingAgentRequest {
    /// 最小化构造：只给会话 id 和 prompt，其余取保守默认。
    pub fn new(session_id: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            prompt: prompt.into(),
            cwd: None,
            model: None,
            fallback_model: None,
            permission_mode: CodingAgentPermissionMode::default(),
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            max_budget_usd: None,
            timeout_secs: 300,
            extra_system_prompt: None,
            settings_json_path: None,
            session_persistence: true,
            continue_session: false,
            continuation_context: None,
        }
    }
}

/// 构造 `claude` 的命令行参数（不含可执行文件本身，也不含 prompt）。
///
/// 固定使用无头流式：`-p --output-format stream-json --verbose --include-partial-messages`，
/// 这样前端能拿到逐字 delta。
pub fn build_claude_args(req: &CodingAgentRequest) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--permission-mode".into(),
        req.permission_mode.as_cli_arg().into(),
    ];

    if let Some(model) = &req.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(fm) = &req.fallback_model {
        args.push("--fallback-model".into());
        args.push(fm.clone());
    }
    if let Some(cwd) = &req.cwd {
        args.push("--add-dir".into());
        args.push(cwd.to_string_lossy().into_owned());
    }
    if !req.allowed_tools.is_empty() {
        args.push("--allowedTools".into());
        args.push(req.allowed_tools.join(","));
    }
    if !req.disallowed_tools.is_empty() {
        args.push("--disallowedTools".into());
        args.push(req.disallowed_tools.join(","));
    }
    if let Some(budget) = req.max_budget_usd {
        args.push("--max-budget-usd".into());
        args.push(format!("{budget}"));
    }
    if let Some(path) = &req.settings_json_path {
        args.push("--settings".into());
        args.push(path.to_string_lossy().into_owned());
    }
    if let Some(sp) = &req.extra_system_prompt {
        args.push("--append-system-prompt".into());
        args.push(sp.clone());
    }
    if !req.session_persistence {
        args.push("--no-session-persistence".into());
    }
    if req.continue_session {
        args.push("--continue".into());
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
    }

    #[test]
    fn default_args_are_headless_streaming() {
        let req = CodingAgentRequest::new("s1", "hello");
        let args = build_claude_args(&req);
        assert!(args.contains(&"-p".to_string()));
        assert_eq!(arg_value(&args, "--output-format"), Some("stream-json"));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--include-partial-messages".to_string()));
        // prompt 不能出现在 argv 里
        assert!(!args.iter().any(|a| a.contains("hello")));
    }

    #[test]
    fn provider_budget_capability_only_belongs_to_claude() {
        assert_eq!(
            CodingAgentProvider::ClaudeCodeCli.max_budget_usd(),
            Some(2.0)
        );
        assert_eq!(CodingAgentProvider::OpenCodeCli.max_budget_usd(), None);
        assert_eq!(CodingAgentProvider::CodexCli.max_budget_usd(), None);
        assert_eq!(CodingAgentProvider::DshCli.max_budget_usd(), None);
        assert_eq!(CodingAgentRequest::new("s", "p").timeout_secs, 300);
    }

    #[test]
    fn permission_mode_maps_to_cli_string() {
        assert_eq!(CodingAgentPermissionMode::Plan.as_cli_arg(), "plan");
        assert_eq!(
            CodingAgentPermissionMode::AcceptEdits.as_cli_arg(),
            "acceptEdits"
        );
        assert_eq!(
            CodingAgentPermissionMode::BypassPermissions.as_cli_arg(),
            "bypassPermissions"
        );
        let mut req = CodingAgentRequest::new("s", "p");
        req.permission_mode = CodingAgentPermissionMode::Plan;
        assert_eq!(
            arg_value(&build_claude_args(&req), "--permission-mode"),
            Some("plan")
        );
    }

    #[test]
    fn default_permission_mode_is_accept_edits() {
        assert_eq!(
            CodingAgentPermissionMode::default(),
            CodingAgentPermissionMode::AcceptEdits
        );
    }

    #[test]
    fn optional_flags_are_emitted_when_set() {
        let mut req = CodingAgentRequest::new("s", "p");
        req.model = Some("sonnet".into());
        req.fallback_model = Some("haiku".into());
        req.max_budget_usd = Some(0.5);
        req.cwd = Some(PathBuf::from("/tmp/work"));
        req.allowed_tools = vec!["Bash(git *)".into(), "Edit".into()];
        req.disallowed_tools = vec!["Bash(rm -rf:*)".into()];
        req.settings_json_path = Some(PathBuf::from("/tmp/guard.json"));
        req.extra_system_prompt = Some("be terse".into());
        req.session_persistence = false;

        let args = build_claude_args(&req);
        assert_eq!(arg_value(&args, "--model"), Some("sonnet"));
        assert_eq!(arg_value(&args, "--fallback-model"), Some("haiku"));
        assert_eq!(arg_value(&args, "--max-budget-usd"), Some("0.5"));
        assert_eq!(arg_value(&args, "--add-dir"), Some("/tmp/work"));
        assert_eq!(arg_value(&args, "--allowedTools"), Some("Bash(git *),Edit"));
        assert_eq!(
            arg_value(&args, "--disallowedTools"),
            Some("Bash(rm -rf:*)")
        );
        assert_eq!(arg_value(&args, "--settings"), Some("/tmp/guard.json"));
        assert_eq!(arg_value(&args, "--append-system-prompt"), Some("be terse"));
        assert!(args.contains(&"--no-session-persistence".to_string()));
    }

    #[test]
    fn optional_flags_absent_by_default() {
        let req = CodingAgentRequest::new("s", "p");
        let args = build_claude_args(&req);
        assert!(arg_value(&args, "--model").is_none());
        assert!(arg_value(&args, "--max-budget-usd").is_none());
        assert!(!args.contains(&"--no-session-persistence".to_string()));
    }

    #[test]
    fn provider_parses_from_pref_with_claude_fallback() {
        assert_eq!(
            CodingAgentProvider::from_pref("opencode-cli"),
            CodingAgentProvider::OpenCodeCli
        );
        assert_eq!(
            CodingAgentProvider::from_pref("claude-code-cli"),
            CodingAgentProvider::ClaudeCodeCli
        );
        // 未知/空 → 回落 Claude（不破坏现有用户）。
        assert_eq!(
            CodingAgentProvider::from_pref(""),
            CodingAgentProvider::ClaudeCodeCli
        );
        assert_eq!(
            CodingAgentProvider::from_pref("something-else"),
            CodingAgentProvider::ClaudeCodeCli
        );
    }

    #[test]
    fn provider_default_exe() {
        assert_eq!(CodingAgentProvider::ClaudeCodeCli.default_exe(), "claude");
        assert_eq!(CodingAgentProvider::OpenCodeCli.default_exe(), "opencode");
        assert_eq!(CodingAgentProvider::CodexCli.default_exe(), "codex");
        assert_eq!(CodingAgentProvider::DshCli.default_exe(), "dsh");
    }

    #[test]
    fn new_providers_parse_from_pref() {
        assert_eq!(
            CodingAgentProvider::from_pref("codex-cli"),
            CodingAgentProvider::CodexCli
        );
        assert_eq!(
            CodingAgentProvider::from_pref("dsh-cli"),
            CodingAgentProvider::DshCli
        );
        // 带空白也要认（prefs 来自前端，历史上出现过带空格的值）。
        assert_eq!(
            CodingAgentProvider::from_pref("  codex-cli  "),
            CodingAgentProvider::CodexCli
        );
    }

    #[test]
    fn only_deny_list_backends_offer_command_approval() {
        // 审批卡只对「能精确放行单条命令」的后端有意义，见 supports_command_approval 的文档。
        assert!(CodingAgentProvider::ClaudeCodeCli.supports_command_approval());
        assert!(CodingAgentProvider::OpenCodeCli.supports_command_approval());
        assert!(!CodingAgentProvider::CodexCli.supports_command_approval());
        assert!(!CodingAgentProvider::DshCli.supports_command_approval());
    }

    #[test]
    fn codex_takes_bare_model_names_and_dsh_takes_none() {
        // Codex 的模型名是裸名（gpt-5 / o3），不像 OpenCode 要求 provider/model。
        assert_eq!(
            resolve_coding_agent_model(CodingAgentProvider::CodexCli, Some("gpt-5".into())),
            Some("gpt-5".to_string())
        );
        // 留空交给 ~/.codex/config.toml，不替用户瞎猜一个默认。
        assert_eq!(
            resolve_coding_agent_model(CodingAgentProvider::CodexCli, None),
            None
        );
        assert_eq!(
            resolve_coding_agent_model(CodingAgentProvider::CodexCli, Some("   ".into())),
            None
        );
        // dsh 的 headless profile 压根没有 --model：无论用户选了什么都必须是 None，
        // 否则调用方会以为模型选得动。
        assert_eq!(
            resolve_coding_agent_model(CodingAgentProvider::DshCli, None),
            None
        );
        assert_eq!(
            resolve_coding_agent_model(
                CodingAgentProvider::DshCli,
                Some("deepseek-v4-flash".into())
            ),
            None
        );
    }

    #[test]
    fn provider_specific_model_defaults_do_not_leak_sonnet_into_opencode() {
        assert_eq!(
            resolve_coding_agent_model(CodingAgentProvider::ClaudeCodeCli, None),
            Some("sonnet".to_string())
        );
        assert_eq!(
            resolve_coding_agent_model(CodingAgentProvider::OpenCodeCli, None),
            None
        );
        assert_eq!(
            resolve_coding_agent_model(
                CodingAgentProvider::OpenCodeCli,
                Some("sonnet".to_string())
            ),
            None
        );
        assert_eq!(
            resolve_coding_agent_model(
                CodingAgentProvider::OpenCodeCli,
                Some("opencode/deepseek-v4-flash-free".to_string())
            ),
            Some("opencode/deepseek-v4-flash-free".to_string())
        );
    }
}
