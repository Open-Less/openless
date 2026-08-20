//! 解析 `claude --version` 与 `claude mcp list` 的输出（纯逻辑，便于单测）。

/// 从 `<cli> --version` 输出里提取版本号，四个后端共用。
///
/// 兼容这些真实排版（都对着本机实际输出核过）：
/// - `"2.1.161 (Claude Code)"` / `"Claude Code version 2.1.161"`（claude）
/// - `"codex-cli 0.146.0"`（codex）
/// - `"0.1.0-rc.6"`（dsh）
///
/// **预发布号必须认。** 早先的实现要求严格三段全数字，`0.1.0-rc.6` 会被切成
/// `["0","1","0-rc","6"]` 四段而判定为「没装」——dsh 装机后设置页一直报「未检测到
/// dsh 命令」就是这个原因，跟 PATH 无关。所以 patch 段之后跟着的 `-…` / `+…` 后缀
/// 要原样保留（用户看到的版本号才是真的）。
pub fn parse_cli_version(stdout: &str) -> Option<String> {
    for raw in stdout.split_whitespace() {
        // 跳过纯文字 token（"Claude" / "codex-cli"），从第一个数字开始看。
        let Some(start) = raw.find(|c: char| c.is_ascii_digit()) else {
            continue;
        };
        let candidate = &raw[start..];
        let mut parts = candidate.splitn(3, '.');
        let (Some(major), Some(minor), Some(rest)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !all_digits(major) || !all_digits(minor) {
            continue;
        }
        // rest 形如 `"0"` / `"0-rc.6"` / `"3)"`：先吃掉开头的连续数字当 patch。
        let patch_len = rest.bytes().take_while(u8::is_ascii_digit).count();
        if patch_len == 0 {
            continue;
        }
        // 只有 `-`（预发布）/ `+`（构建元数据）开头的尾巴才保留；
        // `)` `,` 这类是排版噪声，丢掉。
        let tail = &rest[patch_len..];
        let keep = usize::from(tail.starts_with('-') || tail.starts_with('+')) * tail.len();
        return Some(format!("{major}.{minor}.{}", &rest[..patch_len + keep]));
    }
    None
}

/// 旧名，保留给既有调用方。见 [`parse_cli_version`]。
pub fn parse_claude_version(stdout: &str) -> Option<String> {
    parse_cli_version(stdout)
}

/// MCP server 健康状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpHealth {
    Connected,
    Failed,
    NeedsAuth,
    Unknown,
}

/// `claude mcp list` 里的一项。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpServerStatus {
    pub name: String,
    pub detail: String,
    pub health: McpHealth,
}

/// 解析 `claude mcp list` 输出。忽略 "Checking..." 等噪声行。
///
/// 行格式约为：`<name>: <detail> - <✓|✗|!> <status text>`。
pub fn parse_mcp_list(stdout: &str) -> Vec<McpServerStatus> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Checking") {
            continue;
        }
        let Some((name, rest)) = line.split_once(": ") else {
            continue;
        };
        // 用最后一个 " - " 分隔 detail 与状态，避免 URL 里的连字符误伤。
        let (detail, status) = match rest.rfind(" - ") {
            Some(idx) => (rest[..idx].trim(), rest[idx + 3..].trim()),
            None => (rest.trim(), ""),
        };
        let health = if status.contains("Connected") {
            McpHealth::Connected
        } else if status.contains("Failed") {
            McpHealth::Failed
        } else if status.contains("authentication") || status.contains("Needs") {
            McpHealth::NeedsAuth
        } else {
            McpHealth::Unknown
        };
        out.push(McpServerStatus {
            name: name.trim().to_string(),
            detail: detail.to_string(),
            health,
        });
    }
    out
}

/// 是否存在桌面控制类（computer use）MCP server。
///
/// 这是 OpenLess 对「computer use 技能是否安装」的检测口径：Claude Code 本身无原生
/// computer use，桌面 GUI 控制只能通过挂载相应 MCP server 获得。
pub fn has_computer_use_mcp(servers: &[McpServerStatus]) -> bool {
    servers.iter().any(|s| {
        let n = s.name.to_lowercase();
        n.contains("computer") || n.contains("desktop") || n.contains("screen")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_from_both_layouts() {
        assert_eq!(
            parse_claude_version("2.1.161 (Claude Code)").as_deref(),
            Some("2.1.161")
        );
        assert_eq!(
            parse_claude_version("Claude Code version 2.1.161").as_deref(),
            Some("2.1.161")
        );
        assert_eq!(parse_claude_version("no version here"), None);
    }

    #[test]
    fn parses_prerelease_versions() {
        // 回归防线：dsh 的真实输出是 `0.1.0-rc.6`。旧实现把它切成四段直接判「没装」，
        // 装机后设置页一直报「未检测到 dsh 命令」。预发布后缀要原样保留。
        assert_eq!(
            parse_cli_version("0.1.0-rc.6").as_deref(),
            Some("0.1.0-rc.6")
        );
        assert_eq!(
            parse_cli_version("2.0.0+build.7").as_deref(),
            Some("2.0.0+build.7")
        );
    }

    #[test]
    fn parses_versions_from_every_backend_layout() {
        // 四个后端的真实 --version 排版，都对着本机核过。
        assert_eq!(
            parse_cli_version("codex-cli 0.146.0").as_deref(),
            Some("0.146.0")
        );
        assert_eq!(parse_cli_version("v1.2.3").as_deref(), Some("1.2.3"));
        // 排版噪声不能混进版本号里。
        assert_eq!(parse_cli_version("(1.2.3)").as_deref(), Some("1.2.3"));
        assert_eq!(parse_cli_version("1.2.3, ok").as_deref(), Some("1.2.3"));
        // 两段不算版本号。
        assert_eq!(parse_cli_version("1.2"), None);
        assert_eq!(parse_cli_version("abc"), None);
    }

    #[test]
    fn parses_mcp_list_health() {
        let stdout = "Checking MCP server health…\n\
memory: npx -y @modelcontextprotocol/server-memory - ✓ Connected\n\
railway: npx -y @railway/mcp-server - ✗ Failed to connect\n\
cloudflare-observability: https://observability.mcp.cloudflare.com/mcp (HTTP) - ! Needs authentication\n";
        let servers = parse_mcp_list(stdout);
        assert_eq!(servers.len(), 3);
        assert_eq!(servers[0].name, "memory");
        assert_eq!(servers[0].health, McpHealth::Connected);
        assert_eq!(servers[1].health, McpHealth::Failed);
        assert_eq!(servers[2].name, "cloudflare-observability");
        assert_eq!(servers[2].health, McpHealth::NeedsAuth);
        // URL 里的 "-" 不应把状态切错
        assert!(servers[2]
            .detail
            .contains("observability.mcp.cloudflare.com"));
    }

    #[test]
    fn detects_computer_use_mcp_by_name() {
        let with = vec![McpServerStatus {
            name: "computer-use".into(),
            detail: String::new(),
            health: McpHealth::Connected,
        }];
        let without = vec![McpServerStatus {
            name: "playwright".into(),
            detail: String::new(),
            health: McpHealth::Connected,
        }];
        assert!(has_computer_use_mcp(&with));
        assert!(!has_computer_use_mcp(&without));
    }
}

/// 拿本机四个 CLI 的**真实** `--version` 输出跑解析器。默认 `#[ignore]`：要本机装了这些 CLI。
/// 手动跑：`cargo test --lib coding_agent::detect::live -- --ignored --nocapture`
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "要本机装了对应 CLI"]
    fn every_installed_cli_version_parses() {
        // 单测里的样例串是我抄进去的，抄错了测试照样绿。这条直接问真实 CLI 要输出，
        // 是「设置页会不会误报没装」的唯一可信证据。
        for exe in ["claude", "opencode", "codex", "dsh"] {
            let out = std::process::Command::new(exe).arg("--version").output();
            let Ok(out) = out else {
                println!("[skip] {exe} 未安装");
                continue;
            };
            let stdout = String::from_utf8_lossy(&out.stdout);
            let parsed = parse_cli_version(&stdout);
            println!("{exe:>9}: {:?} → {parsed:?}", stdout.trim());
            assert!(
                parsed.is_some(),
                "{exe} 的版本号解析不出来 → 设置页会误报「未检测到」。原始输出: {stdout:?}"
            );
        }
    }
}
