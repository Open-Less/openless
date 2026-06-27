//! 护栏：高风险命令分类 + 生成传给 `claude --settings` 的权限 JSON。
//!
//! 「放行 + 护栏」策略：
//! - `permissions.defaultMode = acceptEdits`（放行可恢复/轻动作）。
//! - `permissions.deny` 声明式拦截高风险工具调用（跨平台、稳）。
//! - 运行级 git 快照由运行器在启动前做（见 `mod.rs::create_git_snapshot`）。
//!
//! [`is_high_risk_command`] 供「Claude 控制台」等场景对**单条命令**做本地预检/展示用，
//! 与 CLI 侧的 deny 规则互为补充。

/// 高风险子串（已小写）→ 原因。命中任一即判为高风险。
pub const HIGH_RISK_PATTERNS: &[(&str, &str)] = &[
    ("rm -rf", "递归强制删除"),
    ("rm -fr", "递归强制删除"),
    ("sudo ", "提权执行"),
    ("git push --force", "强制推送会覆盖远端历史"),
    ("git push -f", "强制推送会覆盖远端历史"),
    ("git reset --hard", "硬重置会丢弃未提交改动"),
    ("git clean -fd", "强制清理未跟踪文件"),
    ("git clean -f -d", "强制清理未跟踪文件"),
    ("mkfs", "格式化文件系统"),
    ("dd if=", "裸盘写入"),
    (":(){", "fork 炸弹"),
    ("shutdown", "关机"),
    ("reboot", "重启"),
    ("> /dev/sd", "直接写入块设备"),
    ("| sh", "管道执行远程脚本"),
    ("|sh", "管道执行远程脚本"),
    ("| bash", "管道执行远程脚本"),
    ("|bash", "管道执行远程脚本"),
    ("chmod -r 777 /", "危险的全局权限修改"),
    ("chown -r", "递归改所有权"),
];

/// 若命令命中高风险模式，返回原因；否则 `None`。
pub fn is_high_risk_command(command: &str) -> Option<&'static str> {
    let lowered = command.to_lowercase();
    HIGH_RISK_PATTERNS
        .iter()
        .find(|(pat, _)| lowered.contains(pat))
        .map(|(_, reason)| *reason)
}

/// 同一风险的等价命令子串分组：approve 其中之一即整组放行（从 deny 移除 + 加入 allow）。
///
/// 审批 UI 只回传命中的单个 `HIGH_RISK_PATTERNS` 子串（如 "git push --force"），但同一风险
/// 往往有多个等价写法（"git push -f"）。若只放行被点的那一个，等价写法仍在 deny（deny 优先
/// 级高于 allow）→ 命令仍被拦，用户误以为已批准。返回整组让调用方按组放行。
/// 命中返回整组；未命中返回空（调用方回落到 pattern 自身）。
pub fn risk_equivalent_patterns(pattern: &str) -> Vec<&'static str> {
    const GROUPS: &[&[&str]] = &[
        &["git push --force", "git push -f"],
        &["rm -rf", "rm -fr"],
        &["git clean -fd", "git clean -f -d"],
        // 其余按需补充；默认返回自身
    ];
    for g in GROUPS {
        if g.contains(&pattern) {
            return g.to_vec();
        }
    }
    Vec::new()
}

/// 把一个被审批的 `HIGH_RISK_PATTERNS` 子串映射到它在 [`default_deny_rules`] 里对应的
/// **精确** deny 规则字符串。
///
/// - `Some(rule)`：该命令**可被用户批准放行**——批准时按此字符串从 deny 列表精确移除并加入
///   等值 allow（保证「移除的 deny」与「加入的 allow」严格一致）。
/// - `None`：**不可批准**——这些命令要么无法用 `Bash(<prefix>:*)` 安全表达（`| sh` / `:(){`
///   / `> /dev/sd` 出现在命令中段或依赖 shell 语法），要么是提权 / 毁盘 / 系统级动作
///   （sudo / dd / mkfs / chmod / chown / shutdown / reboot）；即使用户点「批准」也保持拦截
///   （fail-closed），且不向 allow 注入任何规则。
///
/// 这同时修掉了旧实现的不一致：旧代码对所有 pattern 一律 `format!("Bash({p}:*)")`，对
/// 带空格/参数的子串（`"sudo "` → `Bash(sudo :*)`）生成的串既不等于 deny `Bash(sudo:*)`
/// （故 deny 没被移除、批准静默失效），又把畸形规则注入了 allowed_tools。
pub fn deny_rule_for_pattern(pattern: &str) -> Option<&'static str> {
    Some(match pattern {
        "rm -rf" => "Bash(rm -rf:*)",
        "rm -fr" => "Bash(rm -fr:*)",
        "git push --force" => "Bash(git push --force:*)",
        "git push -f" => "Bash(git push -f:*)",
        "git reset --hard" => "Bash(git reset --hard:*)",
        "git clean -fd" => "Bash(git clean -fd:*)",
        "git clean -f -d" => "Bash(git clean -f -d:*)",
        // 其余 HIGH_RISK_PATTERNS（sudo / dd if= / mkfs / chmod / chown / shutdown / reboot /
        // 管道执行远程脚本 / fork 炸弹 / 写块设备）= 不可批准，保持拦截。
        _ => return None,
    })
}

/// CLI `--settings` 默认的 `permissions.deny` 规则（Claude Code 工具说明符语法）。
///
/// 注意：管道执行远程脚本（`| sh`）、fork 炸弹（`:(){`）、`> /dev/sd` 等无法用命令前缀
/// 说明符（`Bash(<prefix>:*)`）表达——它们出现在命令中段或依赖 shell 语法——仍由
/// `defaultMode = acceptEdits` + 运行级 [`is_high_risk_command`] 探测兜底。
pub fn default_deny_rules() -> Vec<String> {
    vec![
        "Bash(rm -rf:*)".into(),
        "Bash(rm -fr:*)".into(),
        "Bash(sudo:*)".into(),
        "Bash(git push --force:*)".into(),
        "Bash(git push -f:*)".into(),
        "Bash(git reset --hard:*)".into(),
        "Bash(git clean -fd:*)".into(),
        "Bash(git clean -f -d:*)".into(),
        "Bash(mkfs:*)".into(),
        "Bash(dd:*)".into(),
        "Bash(shutdown:*)".into(),
        "Bash(reboot:*)".into(),
        // 权限/所有权/持久化/系统级命令（补齐 HIGH_RISK_PATTERNS 覆盖面 + macOS 持久化面）。
        "Bash(chmod:*)".into(),
        "Bash(chown:*)".into(),
        "Bash(crontab:*)".into(),
        "Bash(osascript:*)".into(),
        "Bash(launchctl:*)".into(),
        "Bash(kextload:*)".into(),
        "Bash(nvram:*)".into(),
        "Edit(.env)".into(),
        "Edit(.git/**)".into(),
        // macOS 持久化面：开机自启 plist + 登录 shell 配置（写入即可持久驻留/提权）。
        // 用 `~/` 家目录前缀（Claude Code settings 官方写法，如 `Read(~/.zshrc)`）：
        // 文件路径规则里 bare `**/.zshrc` 是相对 agent **工作目录**匹配，命中不到工作目录
        // 之外的真正 `~/.zshrc` → 护栏失效。LaunchDaemons 是系统路径（写入需 root，已被
        // `Bash(sudo:*)` 拦），这里只覆盖用户态 LaunchAgents + 登录 shell 配置。
        "Edit(~/Library/LaunchAgents/**)".into(),
        "Write(~/Library/LaunchAgents/**)".into(),
        "Edit(~/.zshrc)".into(),
        "Write(~/.zshrc)".into(),
        "Edit(~/.zprofile)".into(),
        "Write(~/.zprofile)".into(),
        "Edit(~/.bash_profile)".into(),
        "Write(~/.bash_profile)".into(),
        "Edit(~/.bashrc)".into(),
        "Write(~/.bashrc)".into(),
    ]
}

/// 生成护栏 settings JSON。`mode` 为 `--permission-mode` 同名取值；
/// `extra_deny` 追加在默认 deny 之后。
pub fn build_guard_settings_json(mode: &str, extra_deny: &[String]) -> serde_json::Value {
    let mut deny = default_deny_rules();
    deny.extend(extra_deny.iter().cloned());
    serde_json::json!({
        "permissions": {
            "defaultMode": mode,
            "deny": deny,
        }
    })
}

/// OpenCode `permission` 护栏的高风险 bash 子命令前缀（OpenCode glob 语法，如 `"rm *"`）。
///
/// OpenCode 的 `permission.bash` 是「glob → allow/ask/deny」映射（见官方 permissions 文档），
/// 与 Claude 的 `Bash(<prefix>:*)` 说明符是同类东西，但写法不同。这里把
/// [`default_deny_rules`] 里的 Bash 前缀翻译成 OpenCode glob。管道执行远程脚本
/// （`| sh`）、fork 炸弹等中段/shell 语法仍无法用前缀 glob 表达，由运行级
/// [`is_high_risk_command`] 兜底。
pub fn opencode_bash_deny_prefixes() -> Vec<&'static str> {
    vec![
        "rm -rf", "rm -fr", "sudo", "git push --force", "git push -f", "git reset --hard",
        "git clean -fd", "git clean -f -d", "mkfs", "dd", "shutdown", "reboot", "chmod", "chown",
        "crontab", "osascript", "launchctl", "kextload", "nvram",
    ]
}

/// 生成传给 OpenCode 的护栏配置（写入 `OPENCODE_CONFIG_CONTENT` inline JSON）。
///
/// 与 [`build_guard_settings_json`]（Claude `--settings`）等价的 OpenCode 形态：
/// - `permission.bash`：默认 `allow`（放行可恢复轻动作），高风险前缀 `deny`；
///   `extra_allow_prefixes` 里的前缀（审批通过的）显式 `allow`，盖掉默认 deny。
/// - `permission.edit` / `permission.write`：补齐与 Claude [`default_deny_rules`] 对等的
///   文件级 deny（.env / .git/** / macOS 持久化面）。若 OpenCode 版本不支持 per-file glob deny，
///   这些键被静默忽略（无害）；若支持则获得与 Claude 路径同级的文件保护。
/// - `permission.webfetch = "deny"`：去掉直抓任意 URL 面（与 Claude 路径不放 WebFetch 一致）。
/// - 其余工具（read/glob/grep/websearch）默认 `allow`，靠 `*: allow` 兜底，
///   保持「放行 + 护栏」语义（高风险只在 bash/文件这两层拦）。
pub fn build_opencode_guard_config(extra_allow_prefixes: &[String]) -> serde_json::Value {
    let mut bash: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    // 先放默认：未命中规则一律 allow（轻动作不打断无头执行）。
    bash.insert("*".into(), serde_json::Value::String("allow".into()));
    for prefix in opencode_bash_deny_prefixes() {
        bash.insert(format!("{prefix} *"), serde_json::Value::String("deny".into()));
        // 无参形式（如 `reboot`）也要拦：glob `reboot *` 不匹配光秃秃的 `reboot`。
        bash.insert(prefix.to_string(), serde_json::Value::String("deny".into()));
    }
    // 审批放行：把通过的高风险前缀显式 allow，盖掉上面的 deny（后写覆盖）。
    for prefix in extra_allow_prefixes {
        bash.insert(format!("{prefix} *"), serde_json::Value::String("allow".into()));
        bash.insert(prefix.clone(), serde_json::Value::String("allow".into()));
    }

    // 文件级 deny：与 Claude default_deny_rules() 对等 —— .env / .git/** / macOS 持久化面。
    // 若 OpenCode 不支持 edit/write 的 per-file glob deny，该键被静默忽略（无害）。
    let mut edit_rules: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    edit_rules.insert("*".into(), "allow".into());
    for pat in [".env", ".git/**"] {
        edit_rules.insert(pat.to_string(), "deny".into());
    }
    for pat in [
        "~/Library/LaunchAgents/**",
        "~/.zshrc",
        "~/.zprofile",
        "~/.bash_profile",
        "~/.bashrc",
    ] {
        edit_rules.insert(pat.to_string(), "deny".into());
    }

    let mut write_rules: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    write_rules.insert("*".into(), "allow".into());
    for pat in [
        "~/Library/LaunchAgents/**",
        "~/.zshrc",
        "~/.zprofile",
        "~/.bash_profile",
        "~/.bashrc",
    ] {
        write_rules.insert(pat.to_string(), "deny".into());
    }

    serde_json::json!({
        "permission": {
            "*": "allow",
            "bash": bash,
            "edit": edit_rules,
            "write": write_rules,
            "webfetch": "deny",
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_rm_rf_regardless_of_case_and_spacing() {
        assert!(is_high_risk_command("rm -rf /tmp/x").is_some());
        assert!(is_high_risk_command("RM -RF /").is_some());
        assert!(is_high_risk_command("sudo apt install").is_some());
        assert!(is_high_risk_command("git push --force origin main").is_some());
    }

    #[test]
    fn flags_pipe_to_shell() {
        assert!(is_high_risk_command("curl http://x | sh").is_some());
        assert!(is_high_risk_command("wget -qO- x|bash").is_some());
    }

    #[test]
    fn allows_ordinary_reversible_commands() {
        assert!(is_high_risk_command("ls -la").is_none());
        assert!(is_high_risk_command("git status").is_none());
        assert!(is_high_risk_command("pbcopy < file.txt").is_none());
        assert!(is_high_risk_command("echo hi").is_none());
    }

    #[test]
    fn guard_settings_has_accept_edits_and_deny_list() {
        let v = build_guard_settings_json("acceptEdits", &[]);
        assert_eq!(v["permissions"]["defaultMode"], "acceptEdits");
        let deny = v["permissions"]["deny"].as_array().unwrap();
        assert!(deny.iter().any(|d| d == "Bash(rm -rf:*)"));
        assert!(deny.iter().any(|d| d == "Bash(sudo:*)"));
    }

    #[test]
    fn guard_settings_appends_extra_deny() {
        let extra = vec!["Bash(npm publish:*)".to_string()];
        let v = build_guard_settings_json("acceptEdits", &extra);
        let deny = v["permissions"]["deny"].as_array().unwrap();
        assert!(deny.iter().any(|d| d == "Bash(npm publish:*)"));
    }

    #[test]
    fn opencode_guard_denies_high_risk_bash_and_webfetch() {
        let v = build_opencode_guard_config(&[]);
        let perm = &v["permission"];
        assert_eq!(perm["*"], "allow");
        assert_eq!(perm["webfetch"], "deny");
        let bash = &perm["bash"];
        assert_eq!(bash["*"], "allow");
        // 高风险前缀两种形态都拦（带参 glob + 无参）。
        assert_eq!(bash["rm -rf *"], "deny");
        assert_eq!(bash["sudo *"], "deny");
        assert_eq!(bash["reboot"], "deny");
        assert_eq!(bash["git push --force *"], "deny");
    }

    #[test]
    fn opencode_guard_extra_allow_overrides_deny() {
        let v = build_opencode_guard_config(&["git push --force".to_string()]);
        let bash = &v["permission"]["bash"];
        // 审批放行后，被批准的前缀变 allow（覆盖默认 deny）。
        assert_eq!(bash["git push --force *"], "allow");
        assert_eq!(bash["git push --force"], "allow");
        // 未放行的仍然 deny。
        assert_eq!(bash["rm -rf *"], "deny");
    }

    #[test]
    fn default_deny_covers_perms_and_macos_persistence() {
        let deny = default_deny_rules();
        // 新增的权限/系统级命令。
        for rule in [
            "Bash(chmod:*)",
            "Bash(chown:*)",
            "Bash(crontab:*)",
            "Bash(osascript:*)",
            "Bash(launchctl:*)",
            "Bash(kextload:*)",
            "Bash(nvram:*)",
        ] {
            assert!(deny.iter().any(|d| d == rule), "缺少 deny: {rule}");
        }
        // macOS 持久化面（`~/` 家目录前缀，全 Edit/Write 变体）。
        for rule in [
            "Edit(~/Library/LaunchAgents/**)",
            "Write(~/Library/LaunchAgents/**)",
            "Edit(~/.zshrc)",
            "Write(~/.zshrc)",
            "Edit(~/.zprofile)",
            "Write(~/.zprofile)",
            "Edit(~/.bash_profile)",
            "Write(~/.bash_profile)",
            "Edit(~/.bashrc)",
            "Write(~/.bashrc)",
        ] {
            assert!(deny.iter().any(|d| d == rule), "缺少 deny: {rule}");
        }
    }

    #[test]
    fn risk_equivalent_force_push_releases_whole_group() {
        // approve "--force" 应同时放行 "-f" 等价写法。
        let group = risk_equivalent_patterns("git push --force");
        assert!(group.contains(&"git push --force"));
        assert!(group.contains(&"git push -f"));
        // 反向也成立：approve "-f" 同样放行 "--force"。
        let group2 = risk_equivalent_patterns("git push -f");
        assert!(group2.contains(&"git push --force"));
    }

    #[test]
    fn risk_equivalent_rm_group_and_unknown_returns_empty() {
        let rm = risk_equivalent_patterns("rm -rf");
        assert!(rm.contains(&"rm -rf"));
        assert!(rm.contains(&"rm -fr"));
        // approve "git clean -fd" 应同时放行 "git clean -f -d" 等价写法。
        let clean = risk_equivalent_patterns("git clean -fd");
        assert!(clean.contains(&"git clean -fd"));
        assert!(clean.contains(&"git clean -f -d"));
        // 不在任何分组里 → 返回空，调用方回落到 pattern 自身。
        assert!(risk_equivalent_patterns("sudo ").is_empty());
    }

    #[test]
    fn approvable_pattern_maps_to_existing_deny_rule() {
        // 关键不变量：每个「可批准」pattern 映射到的 deny 规则必须真实存在于 default_deny_rules
        // 中，否则批准时 deny.retain 无从移除（= 旧 bug：批准静默失效）。
        let deny = default_deny_rules();
        for (pat, _reason) in HIGH_RISK_PATTERNS {
            if let Some(rule) = deny_rule_for_pattern(pat) {
                assert!(
                    deny.iter().any(|d| d == rule),
                    "可批准 pattern {pat:?} 映射到 {rule:?}，但它不在 default_deny_rules 中"
                );
            }
        }
    }

    #[test]
    fn dangerous_system_commands_are_not_approvable() {
        // 提权 / 毁盘 / 系统级 / 管道执行 / fork 炸弹：即使被「批准」也必须保持拦截（fail-closed），
        // deny_rule_for_pattern 返回 None → 不移除 deny、不注入 allow。
        for pat in [
            "sudo ",
            "dd if=",
            "mkfs",
            "chmod -r 777 /",
            "chown -r",
            "shutdown",
            "reboot",
            "> /dev/sd",
            "| sh",
            "| bash",
            ":(){",
        ] {
            assert!(
                deny_rule_for_pattern(pat).is_none(),
                "{pat:?} 不应可批准（应保持拦截）"
            );
        }
    }

    #[test]
    fn approvable_git_and_rm_map_to_exact_rules() {
        assert_eq!(
            deny_rule_for_pattern("git push --force"),
            Some("Bash(git push --force:*)")
        );
        assert_eq!(deny_rule_for_pattern("rm -rf"), Some("Bash(rm -rf:*)"));
        assert_eq!(
            deny_rule_for_pattern("git reset --hard"),
            Some("Bash(git reset --hard:*)")
        );
    }
}
