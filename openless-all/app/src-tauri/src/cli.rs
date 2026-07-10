//! 极简 CLI 参数解析 — 用于支持桌面环境快捷键调起 OpenLess 触发听写 / QA。
//!
//! 这条路径的来历：Linux 上 fcitx5 插件提供了热键 + 文字提交的完整方案，
//! `openless --toggle-dictation` → tauri-plugin-single-instance 转发的 CLI 路径。
//! macOS / Windows 上仍走原生 hotkey 监听器，CLI 是补充而非替代。
//!
//! 解析约束：
//! - **不依赖 clap**。CLI surface 极小（4 个 flag、无子命令），引入 clap 既增加二进制体积
//!   也带来「未知参数即 panic exit」的风险——GUI app 必须吃下未知参数照常起来，否则
//!   .desktop launcher 或发行版包装传 dragged-in 文件路径就直接崩。
//! - **未知参数静默忽略**。第一个能识别的 flag 即返回；其他参数（路径 / 自动注入的
//!   launcher 标志）不报错。
//! - **同一份解析复用**首次启动 + single-instance 回调两个入口，行为完全一致。

/// 桌面环境快捷键能给 OpenLess 触发的动作集合。
///
/// 与 modifier-only / combo 热键对齐 — 只覆盖「单次触发」语义，不含 push-to-talk
/// （桌面 OS 级快捷键大多只在 key-press 触发，不传 key-release，无法支持「按住说话」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliIntent {
    /// 等价于按一次主听写热键：Idle → 开始；Listening → 结束。
    ToggleDictation,
    /// 等价于按一次 QA 热键：toggle QA 浮窗显隐。
    ToggleQa,
    /// 等价于按 Esc：取消当前听写 session。
    CancelDictation,
}

/// 扫描 argv 找第一个能识别的 intent。未知参数静默忽略，绝不 panic。
///
/// `args` 通常是 `std::env::args().collect::<Vec<_>>()` 或 single-instance 回调里
/// 传入的 `Vec<String>`；两条路径走同一份解析。
pub fn parse_cli_intent<S: AsRef<str>>(args: &[S]) -> Option<CliIntent> {
    // 跳过 argv[0]（自身路径），逐项匹配。命中第一个就返回 —
    // 多个 flag 时取首个，避免出现"toggle + cancel"这种自相矛盾组合。
    for arg in args.iter().skip(1) {
        match arg.as_ref() {
            "--toggle-dictation" => return Some(CliIntent::ToggleDictation),
            "--toggle-qa" => return Some(CliIntent::ToggleQa),
            "--cancel-dictation" | "--cancel" => return Some(CliIntent::CancelDictation),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_none_for_empty_argv() {
        let args: Vec<&str> = vec![];
        assert_eq!(parse_cli_intent(&args), None);
    }

    #[test]
    fn parse_returns_none_when_only_argv0() {
        // GUI 双击启动 / Tauri 默认启动场景：只有 argv[0]，没有 intent。
        let args = vec!["openless"];
        assert_eq!(parse_cli_intent(&args), None);
    }

    #[test]
    fn parse_recognizes_toggle_dictation() {
        let args = vec!["openless", "--toggle-dictation"];
        assert_eq!(parse_cli_intent(&args), Some(CliIntent::ToggleDictation));
    }

    #[test]
    fn parse_recognizes_toggle_qa() {
        let args = vec!["openless", "--toggle-qa"];
        assert_eq!(parse_cli_intent(&args), Some(CliIntent::ToggleQa));
    }

    #[test]
    fn parse_recognizes_cancel_dictation() {
        let args = vec!["openless", "--cancel-dictation"];
        assert_eq!(parse_cli_intent(&args), Some(CliIntent::CancelDictation));
    }

    #[test]
    fn parse_accepts_cancel_alias() {
        // --cancel 也接受（research doc 5 节里写成 --cancel；为兼容两种写法都收）。
        let args = vec!["openless", "--cancel"];
        assert_eq!(parse_cli_intent(&args), Some(CliIntent::CancelDictation));
    }

    #[test]
    fn parse_ignores_unknown_args() {
        // GUI app 必须吃下未知参数照常起来。
        let args = vec!["openless", "--unknown-flag", "/some/path"];
        assert_eq!(parse_cli_intent(&args), None);
    }

    #[test]
    fn parse_returns_first_matching_intent() {
        // 多个 flag 时取首个，确定行为。
        let args = vec!["openless", "--toggle-dictation", "--toggle-qa"];
        assert_eq!(parse_cli_intent(&args), Some(CliIntent::ToggleDictation));
    }

    #[test]
    fn parse_skips_argv0_even_if_it_looks_like_a_flag() {
        // argv[0] 是进程路径，永远跳过。即便构造出诡异的"argv[0]=--toggle-dictation"
        // 也不应被当作 intent —— skip(1) 已保证。
        let args = vec!["--toggle-dictation"];
        assert_eq!(parse_cli_intent(&args), None);
    }

    #[test]
    fn parse_finds_intent_among_unknown_args() {
        let args = vec!["openless", "/path/to/file", "--toggle-dictation", "extra"];
        assert_eq!(parse_cli_intent(&args), Some(CliIntent::ToggleDictation));
    }
}
