use std::sync::Arc;

use tauri::Emitter;

use crate::coordinator_state::{initial_session_id, new_session_id, SessionId, SessionPhase};
use crate::selection::SelectionContext;
use crate::types::CapsuleState;

use super::{
    begin_qa_session, cancel_qa_session, capture_focus_target, capture_frontmost_app, emit_capsule,
    end_qa_session, qa_event_target, Inner,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QaPhase {
    Idle,
    Recording,
    Processing,
}

pub(super) struct QaSessionState {
    pub(super) phase: QaPhase,
    pub(super) cancelled: bool,
    pub(super) selection: Option<SelectionContext>,
    pub(super) front_app: Option<String>,
    /// open_qa_panel 时用户原 app 的 HWND（Windows 专用，存 usize 跨线程安全）。
    /// begin_qa_session 抓选区前临时把焦点还给它，避开 #466 修复后 QA 自己抢前台导致
    /// simulate_copy 在 QA webview 上跑空。非 Windows / macOS 平台为 None 不参与。
    pub(super) qa_focus_target: Option<usize>,
    /// 用于忽略迟到的 RMS / runtime error。
    pub(super) session_id: SessionId,
    /// 浮窗是否对用户可见。Cmd+Shift+; 边沿 toggle 此 flag；
    /// 主听写 hotkey（rightOption）边沿来时，看这个 flag 决定是走 QA 还是走 dictation。
    /// 详见 issue #118 v2。
    pub(super) panel_visible: bool,
    /// 多轮对话累积。每轮 user→assistant 加两条；关浮窗清空。
    pub(super) messages: Vec<crate::types::QaChatMessage>,
}

impl Default for QaSessionState {
    fn default() -> Self {
        Self {
            phase: QaPhase::Idle,
            cancelled: false,
            selection: None,
            front_app: None,
            qa_focus_target: None,
            session_id: initial_session_id(),
            panel_visible: false,
            messages: Vec::new(),
        }
    }
}

pub(super) async fn handle_qa_hotkey_pressed(inner: &Arc<Inner>) {
    // QA hotkey（默认 Cmd+Shift+;）现在只 toggle 浮窗可见性。
    // 浮窗内的录音 / 提问由 Option 边沿驱动（handle_pressed_edge → handle_qa_option_edge）。
    let visible = inner.qa_state.lock().panel_visible;
    log::info!("[coord] QA hotkey edge (panel_visible={visible})");
    if visible {
        close_qa_panel(inner);
    } else {
        open_qa_panel(inner);
    }
}

pub(super) async fn handle_qa_option_edge(inner: &Arc<Inner>) {
    let phase = inner.qa_state.lock().phase;
    log::info!("[coord] QA option edge (phase={phase:?})");
    match phase {
        QaPhase::Idle => {
            let _ = begin_qa_session(inner).await;
        }
        QaPhase::Recording => {
            let _ = end_qa_session(inner).await;
        }
        // Processing 阶段再次按键忽略（避免与正在跑的 LLM 冲突）。
        QaPhase::Processing => {}
    }
}

pub(super) fn open_qa_panel(inner: &Arc<Inner>) {
    let session_id = {
        let mut state = inner.qa_state.lock();
        state.panel_visible = true;
        state.phase = QaPhase::Idle;
        state.cancelled = false;
        state.messages.clear();
        state.selection = None;
        state.front_app = capture_frontmost_app();
        // 在 show_qa_window 抢前台之前抓一下：每次 begin_qa_session 抓选区时拿这个 HWND
        // 临时把焦点还回去，让 simulate_copy 跑在用户原 app 上。issue #466 focus-dance。
        state.qa_focus_target = capture_focus_target();
        state.session_id = new_session_id();
        state.session_id
    };
    // 主听写 phase 是 Idle 才需要 sweep capsule —— 这里的语义是清掉「上一次 dictation
    // Done 状态残留」的 message / insertedChars，让 QA 自己的 capsule 状态从干净起跑
    // （否则 capsule UI 会出现 "已粘贴这个 0" 之类把上一次 inserted_chars 错误复用的
    // 显示）。但如果 dictation 当前正处于 Recording / Polishing / Inserting / Done toast
    // 显示中，强行 emit Idle 会把用户没看完的反馈抹掉、或者把 Polishing 中的进度条
    // 卡死。审计 3.3.4。
    let dictation_idle = matches!(inner.state.lock().phase, SessionPhase::Idle);
    if dictation_idle {
        emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    }
    {
        let state = inner.qa_state.lock();
        if !state.panel_visible || state.session_id != session_id {
            return;
        }
        if let Some(app) = inner.app.lock().clone() {
            crate::show_qa_window(&app, "idle");
            let _ = app.emit_to(
                qa_event_target(),
                "qa:state",
                serde_json::json!({
                    "kind": "idle",
                    "session_id": session_id,
                    "selection_warning": null,
                    "messages": Vec::<crate::types::QaChatMessage>::new(),
                }),
            );
        }
    }
    log::info!("[coord] QA panel opened (awaiting Option to record)");
}

pub(super) fn close_qa_panel(inner: &Arc<Inner>) {
    cancel_qa_session(inner);
    {
        let mut state = inner.qa_state.lock();
        state.panel_visible = false;
        state.messages.clear();
        state.selection = None;
        state.front_app = None;
        state.qa_focus_target = None;
        state.phase = QaPhase::Idle;
        state.cancelled = false;
        // 让仍在阻塞选区捕获或 provider await 中的旧任务无法在关闭后写回状态。
        state.session_id = new_session_id();
    }
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_qa_window(&app);
    }
    // 胶囊一同收掉，避免浮窗关了胶囊还在显示。
    emit_capsule(inner, CapsuleState::Idle, 0.0, 0, None, None);
    log::info!("[coord] QA panel closed, history cleared");
}

#[cfg(test)]
mod tests {
    // issue #609 F-05：给零覆盖的纯逻辑补单测。QaSessionState::default() 的初始不变量
    // 是 open/close panel、begin/end session 一系列状态机的起点，任何字段默认值漂移
    // （如 panel_visible 默认 true、messages 非空）都会让 QA 流程行为错乱。
    use super::{QaPhase, QaSessionState};

    #[test]
    fn qa_session_state_default_starts_idle_and_clean() {
        let st = QaSessionState::default();
        assert_eq!(st.phase, QaPhase::Idle);
        assert!(!st.cancelled);
        assert!(st.selection.is_none());
        assert!(st.front_app.is_none());
        assert!(st.qa_focus_target.is_none());
        assert!(!st.panel_visible, "浮窗默认不可见，等用户 toggle");
        assert!(st.messages.is_empty(), "新建会话历史必须为空");
    }
}
