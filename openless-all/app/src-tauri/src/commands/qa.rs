use super::*;

#[tauri::command]
pub fn get_qa_hotkey_label(coord: CoordinatorState<'_>) -> String {
    coord.qa_hotkey_label()
}

/// 设置 QA 快捷键并热更新 monitor。
/// 传入 `None` 形式的字段不在这里支持——前端用 `binding == null` 时调下面的
/// "disable" 写法（写 prefs.qa_hotkey = None）即可。
#[tauri::command]
pub fn set_qa_hotkey(
    coord: CoordinatorState<'_>,
    binding: Option<ShortcutBinding>,
) -> Result<(), String> {
    if let Some(binding) = binding.as_ref() {
        crate::shortcut_binding::validate_binding(binding).map_err(|e| e.to_string())?;
        crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
        if binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift") {
            return Err("Shift 单键目前只能用于翻译快捷键".into());
        }
    }
    let mut prefs = coord.prefs().get();
    if let Some(binding) = binding.as_ref() {
        reject_dictation_qa_hotkey_overlap(&prefs.dictation_hotkey, binding)?;
        reject_qa_translation_hotkey_overlap(binding, &prefs.translation_hotkey)?;
        if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
            reject_qa_switch_style_hotkey_overlap(binding, switch_style)?;
        }
        if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
            reject_qa_open_app_hotkey_overlap(binding, open_app)?;
        }
        if let Some(less_computer) = prefs.coding_agent_voice_hotkey.as_ref() {
            reject_qa_less_computer_hotkey_overlap(binding, less_computer)?;
        }
    }
    prefs.qa_hotkey = binding;
    coord.prefs().set(prefs).map_err(|e| e.to_string())?;
    coord.update_qa_hotkey_binding();
    Ok(())
}

/// 用户点 ✕ / 按 Esc 关 QA 浮窗。
#[tauri::command]
pub fn qa_window_dismiss(coord: CoordinatorState<'_>) {
    coord.qa_window_dismiss();
}

/// 用户点 📌 / 取消 📌。pinned=true 时浮窗不会自动隐藏。
#[tauri::command]
pub fn qa_window_pin(coord: CoordinatorState<'_>, pinned: bool) {
    coord.qa_window_pin(pinned);
}

/// 移动端 QA 面板录音按钮：Idle -> begin_qa_session，Recording -> end_qa_session。
#[tauri::command]
pub async fn qa_toggle_recording(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.qa_toggle_recording().await;
    Ok(())
}

/// QA 面板键盘输入：复用语音 QA 的 LLM 管线，只替换问题来源。
#[tauri::command]
pub async fn qa_submit_text(coord: CoordinatorState<'_>, text: String) -> Result<(), String> {
    coord.qa_submit_text(text).await
}

/// 用户点 ✕ / 按 Esc 关 Less Computer 浮窗。
#[tauri::command]
pub fn less_computer_window_dismiss(coord: CoordinatorState<'_>) {
    coord.less_computer_window_dismiss();
}

/// 前端按内容测高后回传高度，后端 clamp + bottom-anchored 重新摆放浮窗。
#[tauri::command]
pub fn less_computer_window_resize(coord: CoordinatorState<'_>, height: f64) {
    coord.less_computer_window_resize(height);
}

/// 内联审批卡的 Approve / Deny 回执。token 关联到等待中的拦截动作。
///
/// 安全：审批 UI 渲染在 less-computer 窗口（LessComputerPanel），故仅允许该窗口提交，
/// 拦截 main / capsule / qa / glow 等其它窗口伪造审批 —— 把可调用窗口从 5 个收紧到 1 个。
#[tauri::command]
pub fn less_computer_approve(
    window: Window,
    coord: CoordinatorState<'_>,
    token: String,
    approved: bool,
) -> Result<(), String> {
    if window.label() != "less-computer" {
        return Err("approval can only be submitted from the Less Computer window".to_string());
    }
    coord.less_computer_approve(&token, approved);
    Ok(())
}
