//! QA 问答面板 —— 浮窗宿主第二个 `HostWindow` 实现（多 viewport 版）。
//!
//! UI 对齐原 WebView 版（QaPanel）：标题 + 欢迎语 + 关闭、消息滚动列表（用户右气泡 /
//! 助手左气泡 + 选区引用 + 流式答案 + thinking/error 行）、底部录音选区 chip +
//! 输入组（打字 Enter 提交 + 麦克风开/停录音）。
//!
//! 状态由后端 `qa:state` 事件驱动：lib.rs 里 `app.listen("qa:state", ...)` 解析后经
//! [`QaHostSink::push_qa_state`] 推入宿主线程（`Arc<Mutex<QaViewState>>`）。用户操作
//! 直接同步回调 `Coordinator` 既有方法（`qa_submit_text` / `qa_toggle_recording` /
//! `qa_window_dismiss`）。事件去重/会话 token 判定复用前端 `acceptQaSessionEvent` 逻辑。

use crate::egui_host::qa_event::QaStateEvent;

/// QA 会话状态（由后端 `qa:state` 事件驱动更新）。
pub struct QaViewState {
    /// 会话 token（丢弃关闭/重开后迟到的旧轮事件）。
    pub session_id: Option<String>,
    /// 最近一个 kind。
    pub current_kind: String,
    /// 后端权威的多轮历史（user → assistant 交替）。
    pub messages: Vec<crate::types::QaChatMessage>,
    /// recording 状态时的选区预览（前 60 字）。
    pub selection_preview: Option<String>,
    /// error 状态提示。
    pub error: Option<String>,
    /// 流式答案 buffer（answer_delta 累积，answer 事件来时清空）。
    pub streaming_answer: String,
    /// 输入框文本。
    pub composer_text: String,
    /// 是否已进入「应显示」状态（独立于窗口映射；对 IME/状态机无意义，仅作标记）。
    pub shown: bool,
}

impl QaViewState {
    pub fn new() -> Self {
        Self {
            session_id: None,
            current_kind: "idle".to_string(),
            messages: Vec::new(),
            selection_preview: None,
            error: None,
            streaming_answer: String::new(),
            composer_text: String::new(),
            shown: false,
        }
    }

    pub(crate) fn mark_shown(&mut self) {
        self.shown = true;
    }

    /// 判定一个 `qa:state` 载荷是否应接收（等价前端 `acceptQaSessionEvent`）。
    fn accept(&mut self, kind: &str, session_id: Option<&str>) -> bool {
        let Some(sid) = session_id else {
            return true;
        };
        // idle 一律视为新会话 token：open_qa_panel 的 idle 总是携带新生成的 session_id。
        let starts_turn = matches!(kind, "recording" | "loading" | "thinking" | "idle");
        if let Some(current) = self.session_id.as_deref() {
            if !starts_turn && current != sid {
                return false;
            }
        }
        if !self.session_id.is_some() || starts_turn {
            self.session_id = Some(sid.to_string());
        }
        true
    }
}

/// 把后端一个 `qa:state` 事件应用到 QA 视图状态。由 lib.rs 的监听器经
/// [`QaHostSink`] 推送。
pub fn apply_qa_state(s: &mut QaViewState, payload: &QaStateEvent) {
    let kind = payload.kind.as_deref().unwrap_or("idle");
    if !s.accept(kind, payload.session_id.as_deref()) {
        return;
    }
    if let Some(messages) = &payload.messages {
        s.messages = messages.clone();
    }
    s.current_kind = kind.to_string();
    match kind {
        "idle" => {
            s.selection_preview = None;
            s.error = None;
            s.streaming_answer.clear();
        }
        "recording" => {
            s.selection_preview = payload.selection_preview.clone();
            s.error = None;
            s.streaming_answer.clear();
        }
        "loading" | "thinking" => {
            if payload.selection_preview.is_some() {
                s.selection_preview = payload.selection_preview.clone();
            }
            s.error = None;
            s.streaming_answer.clear();
        }
        "answer_delta" => {
            if let Some(chunk) = &payload.chunk {
                s.streaming_answer.push_str(chunk);
            }
        }
        "answer" => {
            s.error = None;
            s.streaming_answer.clear();
        }
        "error" => {
            s.error = Some(
                payload
                    .error
                    .clone()
                    .unwrap_or_else(|| "问答出错，请重试".to_string()),
            );
            s.streaming_answer.clear();
        }
        _ => {}
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}
