//! 宿主 app 文档读取 —— 唯一接触「用户正在写的那篇东西」的地方。
//!
//! 目标：让 LLM 润色知道用户在写什么。中文同音词（接口/借口、大鱼/大禹）声学模型
//! 分不出来，但上下文能分；今天这条信息在 OpenLess 里完全缺失。
//!
//! ## 边界
//!
//! 所有平台差异关在本模块内。非 macOS 一律返回 [`HostDocumentStatus::Unsupported`]：
//! Windows 没有任何 UIAutomation 代码且 TSF 只在提交瞬间激活；Linux 的 fcitx5
//! SurroundingText 多数客户端不支持。留着接口形状一致，将来补实现不用改调用方。
//!
//! ## 三条硬约束（新代码不得违反，哪怕仓库里的旧 AX 代码就是这么写的）
//!
//! 1. **AX 调用必须有超时**。`AXUIElementSetMessagingTimeout` 不设就继承默认的
//!    ~6 秒 —— 对着一个卡死的 app 就是 6 秒冻结。`selection.rs` / `lib.rs` 的既有
//!    AX 代码都没设，那是缺陷，不要复制。
//! 2. **不在 tokio worker 上同步调 AX**。走 `spawn_blocking` + `tokio::time::timeout`
//!    双保险（形状照 `windows_ime_ipc.rs` 的原生调用边界）。内层超时保护线程本身，
//!    外层保证 async 调用方无论如何都能按时返回。
//! 3. **读之前先过安全闸门**。我们读的是别的应用里的任意文本，最终会进 LLM 请求体。
//!    密码框、Secure Input、密码管理器、终端一律不读，一次 AX 都不发。
//!
//! ## 本里程碑的范围
//!
//! 模块可用但**不接产品链路** —— 只有一个 debug 命令 `debug_read_cursor_context`
//! 在调它。接进润色 prompt 是下一步的事，那里才引入用户可见的开关（默认关）。

mod diff;
mod window;

#[cfg(target_os = "macos")]
mod macos;

// `minimal_edit` 目前只有 macOS 的观察回调在用，非 macOS 构建下没有消费方。
#[allow(unused_imports)]
pub use diff::{
    edit_is_within_typed_text, is_vocab_worthy, learned_rule, minimal_edit, EditPair, LearnedRule,
};

// `WindowSpan` 目前只有 `plan_window` 的返回类型用到，本 crate 内没有别的引用点；
// 跟着一起导出是为了让调用方能给它命名（对齐 `unicode_keystroke` 的既有写法）。
#[allow(unused_imports)]
pub use window::{plan_window, utf16_offset_to_char_offset, window_around_cursor, WindowSpan};

use serde::Serialize;

/// 送进 LLM 的默认上下文预算（char）。够覆盖一两段中文，又不至于让 prompt 显著变贵。
/// 真实的成本/延迟影响要等接进润色后实测，届时再调。
pub const DEFAULT_BUDGET_CHARS: usize = 600;

/// 单次 AX 消息的超时。200ms 已经远超正常 AX 往返（个位数毫秒），只用来兜住卡死的 app。
#[cfg(target_os = "macos")]
const AX_MESSAGING_TIMEOUT_SECS: f32 = 0.2;

/// 整次读取（若干次 AX 往返）在 async 侧的硬上限。
///
/// 比 `AX_MESSAGING_TIMEOUT_SECS` 大是故意的：一次读取要发 5~6 条 AX 消息，逐条
/// 200ms 封顶。超时只是让调用方别再等；阻塞线程会自己按 AX 超时收尾。
#[cfg(target_os = "macos")]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1200);

/// 手改监听最长存活多久。
///
/// 过了一分钟用户还在动这段文字，多半是在继续写新东西而不是纠我们插错的词，再学下去
/// 只会收进噪声。同时这也是「观察器绝不泄漏」的最后一道保险。
#[cfg(target_os = "macos")]
const EDIT_WATCH_MAX_LIFETIME: std::time::Duration = std::time::Duration::from_secs(60);

/// 已按预算截过窗的上下文。`cursor` 是窗口内的 char 下标。
///
/// 没有与之对应的「完整文档」类型：手改监听的基线是**落字那一段文本**而不是整篇文档
/// （见 [`watch_for_edits`]），整篇文档在本模块里除了被截窗之外没有第二个用途。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentWindow {
    pub text: String,
    pub cursor: usize,
}

impl DocumentWindow {
    /// 光标之前的部分（用户已经写完的语境）。
    pub fn before(&self) -> &str {
        let byte_idx = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len());
        &self.text[..byte_idx]
    }

    /// 光标之后的部分。
    pub fn after(&self) -> &str {
        let byte_idx = self
            .text
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len());
        &self.text[byte_idx..]
    }
}

/// 一次读取的结局。`Ok` 之外的每一种都要能说清「为什么没读到」—— 装机验证时全靠它
/// 判断某个 app 是「被拦了」还是「AX 根本不支持」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostDocumentStatus {
    /// 读到了。
    Ok,
    /// 安全闸门拦下，一次 AX 都没发。
    Blocked,
    /// 本平台没有实现。（macOS 编译时构造不到它，故显式 allow。）
    #[allow(dead_code)]
    Unsupported,
    /// AX 可达但拿不到文档（没焦点 / 该控件不支持文本属性 / 权限缺失）。
    Unavailable,
    /// 超过 [`READ_TIMEOUT`] 还没返回 —— 目标 app 大概率卡死。
    Timeout,
}

/// 硬拦原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// macOS Secure Event Input 已开启（密码框、sudo 提示等）。
    SecureInput,
    /// 焦点控件的 AXRole/AXSubrole 是 `AXSecureTextField`。
    SecureTextField,
    /// 前台 app 在硬编码黑名单里（密码管理器 / 钥匙串 / 终端）。
    BlockedApp,
}

impl BlockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockReason::SecureInput => "secure_input",
            BlockReason::SecureTextField => "secure_text_field",
            BlockReason::BlockedApp => "blocked_app",
        }
    }
}

/// 一次读取的完整结果，debug 命令直接把它序列化给前端看。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDocumentReadResult {
    pub status: HostDocumentStatus,
    /// 机器可读的细节：`BlockReason::as_str()` 或不可用原因。
    pub reason: Option<String>,
    pub window: Option<DocumentWindow>,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    pub elapsed_ms: u64,
}

impl HostDocumentReadResult {
    fn new(status: HostDocumentStatus, reason: Option<String>) -> Self {
        Self {
            status,
            reason,
            window: None,
            app_name: None,
            bundle_id: None,
            elapsed_ms: 0,
        }
    }
}

/// 安全闸门的输入。抽成一个纯数据结构，是为了让判定逻辑能脱离 AX 单测 —— 闸门判错
/// 的代价是把密码送进 LLM，这条路径必须有测试覆盖。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateInputs {
    /// `unicode_keystroke::is_secure_input_enabled()` 的结果。
    pub secure_input: bool,
    /// 前台 app 的 bundle id（macOS）。
    pub bundle_id: Option<String>,
    /// 焦点元素的 `AXRole`。
    pub role: Option<String>,
    /// 焦点元素的 `AXSubrole`。
    pub subrole: Option<String>,
}

/// AX 里表示「密码输入框」的 role/subrole 值。
const AX_SECURE_TEXT_FIELD: &str = "axsecuretextfield";

/// 一律不读的 app（bundle id 前缀，小写比较）。
///
/// 不做 UI —— 黑名单 UI 会给用户「配一下就安全了」的错觉，而真正的防线是默认关闭
/// 加这里的硬编码。这份清单只覆盖「内容几乎必然敏感」的两类：
///
/// - **密码管理器 / 钥匙串**：正文就是凭据本身。
/// - **终端**：命令行里混着 token、私钥路径、内网地址，而且很多终端的 AX 会把整个
///   scrollback 当作一个文本元素返回 —— 一读就是几千行历史命令。
///
/// 前缀匹配，所以 `com.1password` 能同时盖住 `com.1password.1password` 和其
/// helper 进程。
const BLOCKED_BUNDLE_PREFIXES: &[&str] = &[
    // 密码管理器 / 钥匙串
    "com.1password",
    "com.agilebits.onepassword",
    "com.apple.keychainaccess",
    "com.bitwarden",
    "com.lastpass",
    "com.dashlane",
    "org.keepassxc",
    "com.kueh.keepassium",
    "in.sinew.enpass",
    "com.sinew.enpass",
    "com.apple.passwords",
    // 终端
    "com.apple.terminal",
    "com.googlecode.iterm2",
    "dev.warp.warp",
    "com.github.wez.wezterm",
    "io.alacritty",
    "org.alacritty",
    "net.kovidgoyal.kitty",
    "co.zeit.hyper",
    "org.tabby",
    "com.tabby",
    "com.mitchellh.ghostty",
];

/// 闸门判定。返回 `Some(reason)` 表示拦下，`None` 表示放行。
///
/// 判定顺序按「代价从低到高」：Secure Input 和 bundle 前缀不需要 AX，先判；
/// role/subrole 需要一次 AX 读，放在最后。
pub fn evaluate_gate(inputs: &GateInputs) -> Option<BlockReason> {
    if inputs.secure_input {
        return Some(BlockReason::SecureInput);
    }
    if let Some(bundle) = inputs.bundle_id.as_deref() {
        let lowered = bundle.to_ascii_lowercase();
        if BLOCKED_BUNDLE_PREFIXES
            .iter()
            .any(|prefix| lowered.starts_with(prefix))
        {
            return Some(BlockReason::BlockedApp);
        }
    }
    let is_secure_field = |value: &Option<String>| {
        value
            .as_deref()
            .is_some_and(|v| v.trim().eq_ignore_ascii_case(AX_SECURE_TEXT_FIELD))
    };
    if is_secure_field(&inputs.role) || is_secure_field(&inputs.subrole) {
        return Some(BlockReason::SecureTextField);
    }
    None
}

/// 平台实现返回给 [`probe_around_cursor`] 的中间结果。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum ReadOutcome {
    Window(DocumentWindow),
    Blocked(BlockReason),
    /// 带一句静态原因，供日志和 debug 命令区分「没焦点」和「不支持」。
    Unavailable(&'static str),
}

/// 读光标周围的上下文；任何失败都退化为 `None`，绝不向上抛错。
///
/// 这是产品链路要用的入口（里程碑 2 起）。想知道「为什么没读到」用
/// [`probe_around_cursor`]。
pub async fn read_around_cursor(budget_chars: usize) -> Option<DocumentWindow> {
    probe_around_cursor(budget_chars).await.window
}

/// 带诊断信息的读取。debug 命令用它，装机验证时靠 `status` / `reason` 判断各 app
/// 的真实覆盖情况。
pub async fn probe_around_cursor(budget_chars: usize) -> HostDocumentReadResult {
    #[cfg(target_os = "macos")]
    {
        macos_probe(budget_chars).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = budget_chars;
        HostDocumentReadResult::new(
            HostDocumentStatus::Unsupported,
            Some("cursor context is macOS-only for now".to_string()),
        )
    }
}

#[cfg(target_os = "macos")]
async fn macos_probe(budget_chars: usize) -> HostDocumentReadResult {
    let started = std::time::Instant::now();
    let (app_name, bundle_id) = crate::selection::current_front_app_parts();

    let finish = |mut result: HostDocumentReadResult| {
        result.app_name = app_name.clone();
        result.bundle_id = bundle_id.clone();
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        result
    };

    // 第一道闸门：不需要 AX 的部分先判掉，命中就一条 AX 消息都不发。
    let gate = GateInputs {
        secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
        bundle_id: bundle_id.clone(),
        role: None,
        subrole: None,
    };
    if let Some(reason) = evaluate_gate(&gate) {
        return finish(blocked_result(reason));
    }

    // AX 是同步阻塞 API：必须离开 tokio worker，否则一个卡死的 app 会拖住整个运行时。
    let handle =
        tokio::task::spawn_blocking(move || macos::read_around_cursor_blocking(budget_chars, gate));

    match tokio::time::timeout(READ_TIMEOUT, handle).await {
        Ok(Ok(ReadOutcome::Window(window))) => finish(HostDocumentReadResult {
            window: Some(window),
            ..HostDocumentReadResult::new(HostDocumentStatus::Ok, None)
        }),
        Ok(Ok(ReadOutcome::Blocked(reason))) => finish(blocked_result(reason)),
        Ok(Ok(ReadOutcome::Unavailable(reason))) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(reason.to_string()),
        )),
        Ok(Err(join_error)) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(format!("blocking task failed: {join_error}")),
        )),
        Err(_) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Timeout,
            Some(format!("no response within {}ms", READ_TIMEOUT.as_millis())),
        )),
    }
}

#[cfg(target_os = "macos")]
fn blocked_result(reason: BlockReason) -> HostDocumentReadResult {
    HostDocumentReadResult::new(HostDocumentStatus::Blocked, Some(reason.as_str().to_string()))
}

// ═══════════════════════════════════════════════════════════════════════════
// 手改监听
// ═══════════════════════════════════════════════════════════════════════════

/// 已武装的手改监听。**drop 即解除** —— 让「忘了解除」在类型层面不成立。
///
/// 观察器泄漏不只是资源问题：它意味着我们持续持有别的 app 的 AX 引用、持续被那个 app
/// 的每次击键唤醒。所以除了这里的 RAII，观察线程自己还有 60 秒硬超时和「前台 app 一换
/// 就自杀」两道保险。
pub struct EditWatcher {
    #[cfg(target_os = "macos")]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl EditWatcher {
    /// 主动解除。幂等，drop 时会自动调用。
    pub fn disarm(&self) {
        #[cfg(target_os = "macos")]
        self.stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for EditWatcher {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// 武装「用户改了我们刚插入的文本」的监听。
///
/// `typed_text` 必须是**用户实际看到落到屏幕上的那段文字**：流式路径下它是真正打出去的
/// 内容，可能短于完整的 LLM 输出（中途失败、被取消）。拿完整输出当基线会让所有没打完的
/// 会话都被判成「用户删掉了一大段」。
///
/// `on_edit` 在观察线程上被调用，可能多次。任何失败都返回 `None` —— 学不到东西是可以
/// 接受的，影响落字不行。
pub fn watch_for_edits<F>(typed_text: String, on_edit: F) -> Option<EditWatcher>
where
    F: Fn(EditPair) + Send + Sync + 'static,
{
    #[cfg(target_os = "macos")]
    {
        if typed_text.trim().is_empty() {
            return None;
        }
        let stop = macos::spawn_edit_watcher(typed_text, Box::new(on_edit))?;
        Some(EditWatcher { stop })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (typed_text, on_edit);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 丢掉 `EditWatcher` 必须真的把观察线程停掉。
    ///
    /// 停止链路横跨两个文件，读单个文件看不全，实际被误读过：`spawn_edit_watcher`
    /// 只是把 flag 交出来，谁都没置位它 —— 置位的是这里的 `Drop`。解除的调用点也不是
    /// 显式的 `disarm()`，而是 `*slot = None`（`arm_edit_watch` / `begin_session_as`）。
    ///
    /// 这条链一旦断了，症状是**静默的**：观察器活到 60 秒硬超时才停，期间继续读用户
    /// 正在写的文档、继续上报，还会和新武装的那个并行跑。所以钉一个测试在这里。
    #[cfg(target_os = "macos")]
    #[test]
    fn dropping_the_watcher_stops_the_observer_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let stop = Arc::new(AtomicBool::new(false));
        let watcher = EditWatcher {
            stop: Arc::clone(&stop),
        };
        assert!(!stop.load(Ordering::Relaxed), "刚建好不该是停止态");

        drop(watcher);
        assert!(
            stop.load(Ordering::Relaxed),
            "Drop 必须置位停止 flag —— 观察线程只认这一个信号（macos.rs 的 run_edit_watch_loop）"
        );
    }

    fn gate(bundle: Option<&str>, role: Option<&str>, subrole: Option<&str>) -> GateInputs {
        GateInputs {
            secure_input: false,
            bundle_id: bundle.map(str::to_string),
            role: role.map(str::to_string),
            subrole: subrole.map(str::to_string),
        }
    }

    #[test]
    fn ordinary_editor_passes_the_gate() {
        assert_eq!(
            evaluate_gate(&gate(
                Some("com.apple.Notes"),
                Some("AXTextArea"),
                Some("AXStandardWindow")
            )),
            None
        );
    }

    #[test]
    fn secure_input_blocks_before_anything_else() {
        let inputs = GateInputs {
            secure_input: true,
            ..gate(Some("com.apple.Notes"), Some("AXTextArea"), None)
        };
        assert_eq!(evaluate_gate(&inputs), Some(BlockReason::SecureInput));
    }

    #[test]
    fn secure_text_field_role_blocks() {
        assert_eq!(
            evaluate_gate(&gate(Some("com.apple.Safari"), Some("AXSecureTextField"), None)),
            Some(BlockReason::SecureTextField)
        );
    }

    #[test]
    fn secure_text_field_subrole_blocks() {
        // Safari / Chrome 的密码框常常 role=AXTextField、subrole=AXSecureTextField，
        // 只看 role 会漏。
        assert_eq!(
            evaluate_gate(&gate(
                Some("com.google.Chrome"),
                Some("AXTextField"),
                Some("AXSecureTextField")
            )),
            Some(BlockReason::SecureTextField)
        );
    }

    #[test]
    fn secure_text_field_match_is_case_insensitive() {
        assert_eq!(
            evaluate_gate(&gate(None, Some("axSECUREtextfield"), None)),
            Some(BlockReason::SecureTextField)
        );
    }

    #[test]
    fn password_managers_are_blocked() {
        for bundle in [
            "com.1password.1password",
            "com.agilebits.onepassword7",
            "com.apple.keychainaccess",
            "com.bitwarden.desktop",
        ] {
            assert_eq!(
                evaluate_gate(&gate(Some(bundle), Some("AXTextArea"), None)),
                Some(BlockReason::BlockedApp),
                "{bundle} should be blocked"
            );
        }
    }

    #[test]
    fn terminals_are_blocked() {
        for bundle in [
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "dev.warp.Warp-Stable",
            "com.mitchellh.ghostty",
        ] {
            assert_eq!(
                evaluate_gate(&gate(Some(bundle), Some("AXTextArea"), None)),
                Some(BlockReason::BlockedApp),
                "{bundle} should be blocked"
            );
        }
    }

    #[test]
    fn bundle_match_is_case_insensitive_and_prefix_based() {
        // NSWorkspace 返回的大小写不保证和清单一致；helper 进程会在后面缀东西。
        assert_eq!(
            evaluate_gate(&gate(Some("COM.APPLE.TERMINAL"), None, None)),
            Some(BlockReason::BlockedApp)
        );
        assert_eq!(
            evaluate_gate(&gate(Some("com.1password.1password-helper"), None, None)),
            Some(BlockReason::BlockedApp)
        );
    }

    #[test]
    fn a_bundle_that_merely_contains_a_blocked_name_is_not_blocked() {
        // 前缀匹配而非子串匹配：别人的 app 名里带 "terminal" 不该被误伤。
        assert_eq!(
            evaluate_gate(&gate(Some("com.example.terminalnotes"), None, None)),
            None
        );
    }

    #[test]
    fn missing_metadata_does_not_block_by_itself() {
        // 读不到 bundle / role（AX 权限没给、非 macOS）时不能当成「安全」也不能当成
        // 「危险」——闸门只负责已知的危险信号，读不到文档自然会走 Unavailable。
        assert_eq!(evaluate_gate(&GateInputs::default()), None);
    }

    #[test]
    fn document_window_splits_at_the_cursor() {
        let win = DocumentWindow {
            text: "上下文测试".to_string(),
            cursor: 2,
        };
        assert_eq!(win.before(), "上下");
        assert_eq!(win.after(), "文测试");
    }

    #[test]
    fn document_window_cursor_at_the_end_yields_empty_after() {
        let win = DocumentWindow {
            text: "abc".to_string(),
            cursor: 3,
        };
        assert_eq!(win.before(), "abc");
        assert_eq!(win.after(), "");
    }

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn non_macos_reports_unsupported_without_touching_anything() {
        let result = probe_around_cursor(DEFAULT_BUDGET_CHARS).await;
        assert_eq!(result.status, HostDocumentStatus::Unsupported);
        assert!(result.window.is_none());
    }
}
