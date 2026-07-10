//! 解析 `claude --version` 与 `claude mcp list` 的输出（纯逻辑，便于单测）。

/// 从 `claude --version` 输出里提取版本号。
///
/// 兼容 `"2.1.161 (Claude Code)"` 与 `"Claude Code version 2.1.161"` 两种排版：
/// 取第一个形如 `x.y.z` 的 token。
pub fn parse_claude_version(stdout: &str) -> Option<String> {
    for raw in stdout.split_whitespace() {
        let token = raw.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Some(token.to_string());
        }
    }
    None
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
