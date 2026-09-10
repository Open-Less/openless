use super::*;

#[tauri::command]
pub fn get_qa_hotkey_label(core: CoreState<'_>) -> String {
    core.get_preferences()
        .qa_hotkey
        .as_ref()
        .map(|binding| binding.display_label())
        .unwrap_or_default()
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
        reject_bare_shift_dictation_shortcut(binding)?;
    }
    let mut prefs = coord.backend().get_preferences();
    prefs.qa_hotkey = binding;
    reject_hotkey_collisions(&prefs)?;
    super::settings::persist_strict_settings(&coord, prefs)
}

/// 用户点 ✕ / 按 Esc 关 QA 浮窗。
#[tauri::command]
pub async fn qa_window_dismiss(core: CoreState<'_>) -> Result<(), String> {
    core.services()
        .qa
        .dismiss()
        .await
        .map_err(|error| error.message)
}

/// 移动端 QA 面板录音按钮：Idle -> begin_qa_session，Recording -> end_qa_session。
#[tauri::command]
pub async fn qa_toggle_recording(core: CoreState<'_>) -> Result<(), String> {
    core.services()
        .qa
        .toggle_recording()
        .await
        .map_err(|error| error.message)
}

/// QA 面板键盘输入：复用语音 QA 的 LLM 管线，只替换问题来源。
#[tauri::command]
pub async fn qa_submit_text(core: CoreState<'_>, text: String) -> Result<(), String> {
    core.services()
        .qa
        .submit_text(text)
        .await
        .map_err(|error| error.message)
}

/// 划词提问面板「编辑指令」复选框。
#[tauri::command]
pub async fn qa_set_edit_instruction_mode(
    core: CoreState<'_>,
    enabled: bool,
) -> Result<(), String> {
    core.services()
        .qa
        .set_edit_instruction_mode(enabled)
        .await
        .map_err(|error| error.message)
}

/// 用户点 ✕ / 按 Esc 关 Less Computer 浮窗。
#[tauri::command]
pub async fn less_computer_window_dismiss(coord: CoordinatorState<'_>) -> Result<(), String> {
    coord.dismiss_less_computer().await
}

/// 聊天面板（qa / less-computer）请求键盘焦点。
///
/// 两个浮窗都以「不抢前台」方式显示（macOS orderFrontRegardless，从不 makeKey），
/// 窗口不是 key window 时按键根本进不了 webview —— 「点了输入框却打不出字」的根因。
///
/// macOS：窗口已转「非激活 NSPanel」（make_chat_window_panel_macos），makeKeyAndOrderFront
/// 只给面板键盘焦点、**不激活 app**（Spotlight 同款）—— 之前用 window.set_focus() 会激活
/// 整个 app，把主窗口（设置页）一起带到前台，且 frontmost 变成 OpenLess、AX 读不到原 app
/// 选区。其它平台仍走 set_focus。仅允许两个聊天面板窗口调用（与 less_computer_approve
/// 同款收紧）。
#[tauri::command]
pub fn chat_panel_focus_keyboard(window: Window) -> Result<(), String> {
    let label = window.label();
    if label != "qa" && label != "less-computer" {
        return Err("chat_panel_focus_keyboard is only available to chat panels".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        let label = label.to_string();
        let app = window.app_handle().clone();
        // NSWindow 操作必须在主线程（macOS 26 硬断言）；异常兜底防 AppKit raise 穿透。
        let _ = window.app_handle().run_on_main_thread(move || {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            let Some(w) = app.get_webview_window(&label) else {
                return;
            };
            let Ok(handle) = w.ns_window() else {
                log::warn!("[chat-panel] ns_window unavailable; focus skipped");
                return;
            };
            let ns = handle as *mut AnyObject;
            if ns.is_null() {
                return;
            }
            // SAFETY: 闭包内只有一次无返回值的 ObjC 消息发送，无需运行 Rust 析构，
            // 异常展开跳过闭包帧不破坏内存安全。
            let result = unsafe {
                objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![ns, makeKeyAndOrderFront: nil];
                }))
            };
            if let Err(e) = result {
                log::warn!("[chat-panel] makeKeyAndOrderFront raised (caught): {e:?}");
            }
        });
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.set_focus().map_err(|e| e.to_string())
    }
}

/// 浮窗打字输入：文字指令直接进入 Less Computer 执行链（跳过录音与 ASR）。
#[tauri::command]
pub fn less_computer_submit_text(core: CoreState<'_>, coord: CoordinatorState<'_>, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let host = coord.tauri_host();
    let backend = Arc::clone(&*core);
    host.spawn(async move {
        if let Err(error) = backend.submit_less_computer(text).await {
            log::warn!("[less-computer] text submit run failed: {error}");
        }
    });
}

/// 主设置页的文字测试入口。浮窗自身无需也不允许反向调用这个命令。
#[tauri::command]
pub fn less_computer_window_open(
    window: Window,
    coord: CoordinatorState<'_>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Less Computer can only be opened from the main window".to_string());
    }
    coord.tauri_host().show_less_computer();
    Ok(())
}

/// 浮窗 mount 时拉取当前会话的事件缓冲（seq 升序）。
///
/// 浮窗首次创建时 webview 冷加载，后端事件（尤其第一条 `user` —— 用户说的话）
/// 先于前端 listener 注册被丢，表现为「AI 在干活但面板上没有我说的话」。前端
/// mount 后先注册 listener 再调本命令重放积压，按 seq 去重衔接实时流。
/// 会话内容敏感，仅允许 less-computer 窗口调用（与 less_computer_approve 同款收紧）。
#[tauri::command]
pub fn less_computer_sync(
    window: Window,
    core: CoreState<'_>,
    after_sequence: Option<u64>,
) -> Result<crate::coordinator::LessComputerEventReplay, String> {
    if window.label() != "less-computer" {
        return Err("sync can only be requested from the Less Computer window".to_string());
    }
    Ok(crate::coordinator::less_computer_event_replay_after(
        &core,
        after_sequence.unwrap_or(0),
    ))
}

/// 内联审批卡的 Approve / Deny 回执。token 关联到等待中的拦截动作。
///
/// 安全：审批 UI 渲染在 less-computer 窗口（LessComputerPanel），故仅允许该窗口提交，
/// 拦截 main / capsule / qa / glow 等其它窗口伪造审批 —— 把可调用窗口从 5 个收紧到 1 个。
#[tauri::command]
pub async fn less_computer_approve(
    window: Window,
    core: CoreState<'_>,
    token: String,
    approved: bool,
) -> Result<(), String> {
    if window.label() != "less-computer" {
        return Err("approval can only be submitted from the Less Computer window".to_string());
    }
    core.services()
        .less_computer
        .approve(token, approved)
        .await
        .map_err(|error| error.to_string())
}
