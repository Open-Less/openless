//! 宿主 app 文档读取 —— 唯一接触「用户正在写的那篇东西」的地方。
//!
//! 目标：让 LLM 润色知道用户在写什么。中文同音词（接口/借口、大鱼/大禹）声学模型
//! 分不出来，但上下文能分；今天这条信息在 OpenLess 里完全缺失。
//!
//! ## 边界
//!
//! 所有平台差异关在本模块内。macOS 走 AX；Linux 走 OpenLess fcitx5 插件提供的
//! SurroundingText。宿主不支持或插件未安装时按诊断原因降级，不影响听写主链路。
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
//! ## 产品链路
//!
//! 用户显式开启隐私总开关及「发给 LLM」子开关后，本模块会把截窗结果接进润色
//! prompt；关闭时不会触发平台读取。debug 命令保留完整诊断，便于区分安全拦截、宿主
//! 不支持与适配器未安装。

mod diff;
mod window;

#[cfg(target_os = "macos")]
mod macos;

// `minimal_edit` 目前只有 macOS 的观察回调在用，非 macOS 构建下没有消费方。
#[allow(unused_imports)]
pub use diff::{
    edit_is_within_typed_text, is_vocab_worthy, learned_rule, minimal_edit, multiple_edits,
    EditPair, LearnedRule, PositionedEdit,
};

/// 观察器上报的一处编辑及围栏置信度。high 必须来自位置范围命中；内容 contains 回落
/// 永远只能是 low，并会在确认卡片与收件箱中明确展示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditObservation {
    pub edit: EditPair,
    pub confidence: crate::types::CorrectionConfidence,
}

/// 用户在本轮 OpenLess 落字范围内选中的一段文本。
///
/// 这不是一个通用的「系统选区」事件：macOS 观察器只有在能用位置锚证明选区完整落在
/// 最近一次插入文本内时才会上报。`document_text` 只在内存中短暂保留，供 Review 生成
/// 修改建议及确认前做文档快照校验；不会写入历史或词典。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionObservation {
    pub selected_text: String,
    pub document_text: String,
    pub selection_start_utf16: usize,
    pub selection_length_utf16: usize,
    pub whole_field_selected: bool,
}

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

#[cfg(target_os = "linux")]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1200);

#[cfg(target_os = "windows")]
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
    /// 安全闸门拦下；macOS 上会在可行时保证一次 AX 都不发。
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

/// 一律不读的敏感 app（bundle id 前缀，小写比较）。
///
/// 不做 UI —— 黑名单 UI 会给用户「配一下就安全了」的错觉，而真正的防线是默认关闭
/// 加这里的硬编码。这份清单只覆盖「内容几乎必然敏感」的两类：
///
/// - **密码管理器 / 钥匙串**：正文就是凭据本身。
/// 前缀匹配，所以 `com.1password` 能同时盖住 `com.1password.1password` 和其
/// helper 进程。
const SENSITIVE_BUNDLE_PREFIXES: &[&str] = &[
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
];

/// 终端 app 的 bundle id 前缀。除了禁止读取 scrollback，也供 macOS 自动换行模式判断：
/// 已知终端发送 U+000A，其它应用保守发送 Shift+Return。
const TERMINAL_BUNDLE_PREFIXES: &[&str] = &[
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

fn bundle_id_starts_with_any(bundle_id: &str, prefixes: &[&str]) -> bool {
    let lowered = bundle_id.to_ascii_lowercase();
    prefixes.iter().any(|prefix| lowered.starts_with(prefix))
}

pub(crate) fn is_terminal_bundle_id(bundle_id: &str) -> bool {
    bundle_id_starts_with_any(bundle_id, TERMINAL_BUNDLE_PREFIXES)
}

/// 闸门判定。返回 `Some(reason)` 表示拦下，`None` 表示放行。
///
/// 判定顺序按「代价从低到高」：Secure Input 和 bundle 前缀不需要 AX，先判；
/// role/subrole 需要一次 AX 读，放在最后。
pub fn evaluate_gate(inputs: &GateInputs) -> Option<BlockReason> {
    if inputs.secure_input {
        return Some(BlockReason::SecureInput);
    }
    if let Some(bundle) = inputs.bundle_id.as_deref() {
        if bundle_id_starts_with_any(bundle, SENSITIVE_BUNDLE_PREFIXES)
            || is_terminal_bundle_id(bundle)
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

/// 读取当前焦点文本控件的完整内容，用于 Review 预览确认前的快照一致性校验。
///
/// 这条入口沿用光标上下文的安全闸门、AX 单消息超时与 async 总超时；超过 20k UTF-16
/// 单元的控件直接返回 `None`，不会做无界全文读取。当前仅 macOS 的自动选区气泡使用。
pub async fn read_focused_document_text() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let (_, bundle_id) = crate::selection::current_front_app_parts();
        let gate = GateInputs {
            secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
            bundle_id,
            role: None,
            subrole: None,
        };
        if evaluate_gate(&gate).is_some() {
            return None;
        }
        let handle = tokio::task::spawn_blocking(move || macos::read_full_document_blocking(gate));
        return match tokio::time::timeout(READ_TIMEOUT, handle).await {
            Ok(Ok(text)) => text,
            _ => None,
        };
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// 带诊断信息的读取。debug 命令用它，装机验证时靠 `status` / `reason` 判断各 app
/// 的真实覆盖情况。
pub async fn probe_around_cursor(budget_chars: usize) -> HostDocumentReadResult {
    #[cfg(target_os = "macos")]
    {
        macos_probe(budget_chars).await
    }
    #[cfg(target_os = "linux")]
    {
        linux_probe(budget_chars).await
    }
    #[cfg(target_os = "windows")]
    {
        windows_probe(budget_chars).await
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = budget_chars;
        HostDocumentReadResult::new(
            HostDocumentStatus::Unsupported,
            Some("cursor context is unavailable on this platform".to_string()),
        )
    }
}

#[cfg(target_os = "windows")]
async fn windows_probe(budget_chars: usize) -> HostDocumentReadResult {
    use crate::windows_ime_ipc::{
        capture_focused_ime_target, focused_target_block_reason, ImeContextRequest,
        WindowsImeIpcServer,
    };
    use crate::windows_ime_protocol::ImeContextStatus;

    let started = std::time::Instant::now();
    let finish = |mut result: HostDocumentReadResult| {
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        result
    };
    let Some(target) = capture_focused_ime_target() else {
        return finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some("no_focused_gui_thread".to_string()),
        ));
    };
    match focused_target_block_reason(target) {
        Ok(Some(reason)) => {
            return finish(HostDocumentReadResult::new(
                HostDocumentStatus::Blocked,
                Some(reason.to_string()),
            ));
        }
        Err(reason) => {
            return finish(HostDocumentReadResult::new(
                HostDocumentStatus::Blocked,
                Some(reason),
            ));
        }
        Ok(None) => {}
    }
    let per_side = budget_chars.clamp(1, 1200).min(u32::MAX as usize) as u32;
    let request = ImeContextRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        before_chars: per_side,
        after_chars: per_side,
        target: Some(target),
    };
    let query = WindowsImeIpcServer::new().query_context(request);
    match tokio::time::timeout(READ_TIMEOUT, query).await {
        Ok(Ok(response)) if response.status == ImeContextStatus::Ok => {
            let cursor =
                utf16_offset_to_char_offset(&response.text, response.cursor_utf16 as usize);
            let window = window_around_cursor(&response.text, cursor, budget_chars);
            finish(HostDocumentReadResult {
                window: Some(window),
                ..HostDocumentReadResult::new(HostDocumentStatus::Ok, None)
            })
        }
        Ok(Ok(response)) if response.status == ImeContextStatus::Blocked => {
            finish(HostDocumentReadResult::new(
                HostDocumentStatus::Blocked,
                response.error_code.or(Some("sensitive_input".to_string())),
            ))
        }
        Ok(Ok(response)) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            response
                .error_code
                .or(Some("tsf_context_unavailable".to_string())),
        )),
        Ok(Err(error)) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(error.to_string()),
        )),
        Err(_) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Timeout,
            Some(format!("no response within {}ms", READ_TIMEOUT.as_millis())),
        )),
    }
}

#[cfg(target_os = "linux")]
async fn linux_probe(budget_chars: usize) -> HostDocumentReadResult {
    let started = std::time::Instant::now();
    let finish = |mut result: HostDocumentReadResult| {
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        result
    };
    let handle = tokio::task::spawn_blocking(crate::linux_fcitx::get_surrounding_text);
    match tokio::time::timeout(READ_TIMEOUT, handle).await {
        Ok(Ok(Ok((text, cursor_byte, reason)))) if reason.is_empty() => {
            let cursor_byte = cursor_byte as usize;
            if cursor_byte > text.len() || !text.is_char_boundary(cursor_byte) {
                return finish(HostDocumentReadResult::new(
                    HostDocumentStatus::Unavailable,
                    Some("invalid_utf8_cursor".to_string()),
                ));
            }
            let cursor_chars = text[..cursor_byte].chars().count();
            let window = window_around_cursor(&text, cursor_chars, budget_chars);
            finish(HostDocumentReadResult {
                window: Some(window),
                ..HostDocumentReadResult::new(HostDocumentStatus::Ok, None)
            })
        }
        Ok(Ok(Ok((_text, _cursor, reason)))) if reason == "blocked_sensitive" => finish(
            HostDocumentReadResult::new(HostDocumentStatus::Blocked, Some(reason)),
        ),
        Ok(Ok(Ok((_text, _cursor, reason)))) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(reason),
        )),
        Ok(Ok(Err(error))) => finish(HostDocumentReadResult::new(
            HostDocumentStatus::Unavailable,
            Some(error),
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
    HostDocumentReadResult::new(
        HostDocumentStatus::Blocked,
        Some(reason.as_str().to_string()),
    )
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
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for EditWatcher {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// 在原有手改监听之外，同时观察「本轮落字范围内的显式选区」。
///
/// `on_selection(None)` 表示选区折叠、越过本轮落字边界，或观察器自然结束；调用方应
/// 收起对应操作气泡。非 macOS 平台保持原有降级：不创建观察器。
pub fn watch_for_edits_and_selection<F, S>(
    typed_text: String,
    on_edit: F,
    on_selection: S,
) -> Option<EditWatcher>
where
    F: Fn(EditObservation) + Send + Sync + 'static,
    S: Fn(Option<SelectionObservation>) + Send + Sync + 'static,
{
    #[cfg(target_os = "macos")]
    {
        if typed_text.trim().is_empty() {
            return None;
        }
        let stop =
            macos::spawn_edit_watcher(typed_text, Box::new(on_edit), Box::new(on_selection))?;
        Some(EditWatcher { stop })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (typed_text, on_edit, on_selection);
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
            evaluate_gate(&gate(
                Some("com.apple.Safari"),
                Some("AXSecureTextField"),
                None
            )),
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
