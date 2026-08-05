//! Focus-target capture and capsule-window presentation extracted from
//! `coordinator.rs` (behavior-preserving move).
//!
//! External focus/frontmost-app capture, capsule window show/hide/position,
//! and `emit_capsule`. References parent items via `use super::*;`; `pub(super)`
//! so the parent and sibling submodules reach them through `use capsule_focus::*;`.

use super::*;

/// 与 capture_focus_target 类似，但前台窗口属于本进程（即用户停在 QA / capsule / main
/// 等自家窗口）时返回 None，让 caller 区分"用户没切到别处" vs "用户切到了另一个真正的
/// 外部 app"。issue #466 多轮场景下用来刷新 qa_focus_target。
#[cfg(target_os = "windows")]
pub(super) fn capture_external_focus_target() -> Option<usize> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return None;
        }
        Some(hwnd.0 as usize)
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn capture_external_focus_target() -> Option<usize> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn capture_focus_target() -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        None
    } else {
        Some(foreground.0 as usize)
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn capture_focus_target() -> Option<usize> {
    None
}

/// 捕获用户开始 dictation 时的前台 app 标签（"localizedName (bundle.id)"），用作 LLM
/// polish/translate 的上下文前提，让模型按 app 调风格。详见 issue #116。
///
/// macOS 走 NSWorkspace.frontmostApplication（公开 API，无需额外权限）；
/// Windows 复用前台 HWND 拿窗口标题；Linux/其他平台返回 None。
pub(super) fn capture_frontmost_app() -> Option<String> {
    // 曾经这里有一份和 `selection.rs` 逐字重复的 NSWorkspace/Win32 实现（三个 cfg
    // 分支、连 nsstring 转换 helper 都是复制的）。收口到 selection：那边现在把取值
    // 拆成了结构化的 `current_front_app_parts`，`host_document` 的 bundle 黑名单要用。
    // 一处实现，三个消费方。
    match crate::selection::current_front_app_parts() {
        (Some(name), Some(bundle)) => Some(format!("{name} ({bundle})")),
        (Some(name), None) => Some(name),
        (None, Some(bundle)) => Some(bundle),
        (None, None) => None,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn restore_focus_target_if_possible(target: Option<usize>) -> bool {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let Some(raw_target) = target else {
        log::warn!("[coord] no original Windows insertion target captured");
        return false;
    };
    let hwnd = HWND(raw_target as *mut c_void);
    if hwnd.0.is_null() {
        return false;
    }
    if !unsafe { IsWindow(hwnd).as_bool() } {
        log::warn!("[coord] original Windows insertion target is no longer a valid window");
        return false;
    }

    let foreground = unsafe { GetForegroundWindow() };
    if foreground == hwnd {
        return true;
    }

    if unsafe { IsIconic(hwnd).as_bool() } {
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    let _ = unsafe { SetForegroundWindow(hwnd) };
    std::thread::sleep(std::time::Duration::from_millis(60));

    let foreground = unsafe { GetForegroundWindow() };
    if foreground != hwnd {
        log::warn!("[coord] failed to restore original Windows insertion target before paste");
        return false;
    }
    true
}

#[cfg(not(target_os = "windows"))]
pub(super) fn restore_focus_target_if_possible(_target: Option<usize>) -> bool {
    true
}

#[cfg(target_os = "windows")]
pub(super) fn windows_hwnd_is_present(hwnd: windows::Win32::Foundation::HWND) -> bool {
    hwnd != windows::Win32::Foundation::HWND::default()
}

#[cfg(target_os = "windows")]
pub(super) fn capture_ime_submit_target() -> Option<ImeSubmitTarget> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    let foreground = unsafe { GetForegroundWindow() };
    if !windows_hwnd_is_present(foreground) {
        return None;
    }

    let mut foreground_process_id = 0;
    let foreground_thread_id =
        unsafe { GetWindowThreadProcessId(foreground, Some(&mut foreground_process_id)) };
    if foreground_thread_id == 0 {
        return None;
    }

    let mut gui_info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    let target_window = if unsafe { GetGUIThreadInfo(foreground_thread_id, &mut gui_info).is_ok() }
        && windows_hwnd_is_present(gui_info.hwndFocus)
    {
        gui_info.hwndFocus
    } else {
        foreground
    };

    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(target_window, Some(&mut process_id)) };
    if process_id == 0 || thread_id == 0 {
        return None;
    }

    Some(ImeSubmitTarget {
        process_id,
        thread_id,
    })
}

// Windows topmost overlay 的已知 OS 级限制（issue #457）：
// `SetWindowPos(HWND_TOPMOST)` 让 capsule 在普通桌面合成、最大化窗口、borderless
// windowed fullscreen 上正常叠加；但**对独占全屏（exclusive fullscreen）DirectX /
// OpenGL 应用无效** —— 那条路径绕过桌面合成器，标准 topmost 窗口不参与合成 →
// 用户看不见 capsule。这是 OS 层面的限制，用户空间无法绕过（除非接入 DirectX
// overlay，工程量与风险都不在 surgical 修复范围内）。
//
// 用户侧 workaround：把游戏切到 borderless windowed fullscreen（Minecraft Java 默认
// 即是；F11 在不同版本表现不一致，按设置里的「全屏」选项决定）。
//
// 相关 UIPI 限制：若游戏以管理员身份运行而 OpenLess 不是，`WH_KEYBOARD_LL` 收不到
// 游戏的按键 → hotkey 完全不触发。这里跟 SetWindowPos 路径无关，但同源不可绕过。
#[cfg(target_os = "windows")]
pub(super) fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };

    let Ok(handle) = window.window_handle() else {
        // #470 诊断 v2：Win32 show 路径最可能的暗点之一。此前静默 return，
        // 无法观测「胶囊完全不显示」是否卡在这里。
        log::warn!(
            "[capsule] no_activate failed: window_handle() unavailable — Win32 show skipped"
        );
        return false;
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        log::warn!("[capsule] no_activate failed: non-Win32 RawWindowHandle — Win32 show skipped");
        return false;
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);

    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
    };
    true
}

#[cfg(target_os = "macos")]
pub(super) fn show_capsule_window_no_activate<R: tauri::Runtime>(
    app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
    reassert_spaces: bool,
) -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(handle) = window.ns_window() else {
        return false;
    };
    let ns_window = handle as *mut AnyObject;
    if ns_window.is_null() {
        return false;
    }

    // emit_capsule 已经把窗口操作 marshal 到 Tauri 主线程；这里不能调用
    // window.show()/set_focus()/NSApp.activate，否则 AeroSpace 会把 workspace 切回
    // OpenLess 主窗口所在空间。直接用 orderFrontRegardless 做无激活展示。
    //
    // collectionBehavior 一次性写绝对值（与 show_less_computer_glow 的 273 同款），
    // 不再走 Tauri 的 set_visible_on_all_workspaces：那个调用会把 collectionBehavior
    // 经事件循环延后再写一遍，盖掉这里手动加的 FULL_SCREEN_AUXILIARY（→ 全屏 app 上不
    // 叠加）；而把新 bit OR 到旧的 Managed 上又是 Apple 文档明确互斥的非法组合
    // （CanJoinAllSpaces / Managed / Transient 三选一，→ 切桌面跟随不稳）。glow 窗口从不
    // 调它、直接写绝对值，跨 Space + 全屏都正常 —— 胶囊对齐它。
    //   - CAN_JOIN_ALL_SPACES：出现在所有桌面/Space，切桌面/全屏时跟随。
    //   - FULL_SCREEN_AUXILIARY：被允许进入全屏 app 的 Space。
    //   - STATIONARY：Mission Control / Exposé 时不跟着乱飞。
    // 外加 setLevel(25)：光有 FULL_SCREEN_AUXILIARY 只是「被允许」进全屏 Space，但窗口层级
    // 若停在 alwaysOnTop 的浮动层(~3) 仍会被全屏 app 的窗口盖住而看不见；抬到菜单栏(24)之上
    // 的 25（与 show_less_computer_glow 同款）才能真正叠在全屏之上。
    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    const BEHAVIOR: usize = CAN_JOIN_ALL_SPACES | STATIONARY | FULL_SCREEN_AUXILIARY;
    unsafe {
        let _: () = msg_send![ns_window, setLevel: 25i64];
        if reassert_spaces {
            // 值若被外部改动过，留一条证据 —— 用于分辨「值被改」与「值没变但
            // WindowServer 侧注册失效」（2026-07-31 事故属于后者：值一直是 273，
            // 注册却缺了桌面，窗口被钉死在单个 Space）。
            let current: usize = msg_send![ns_window, collectionBehavior];
            if current != BEHAVIOR {
                log::warn!(
                    "[capsule] collectionBehavior drifted to {current} (expected {BEHAVIOR}); re-registering"
                );
            }
            // 入场帧先以「无 CanJoinAllSpaces 位」的低值上屏（保留 Stationary/
            // FullScreenAuxiliary，全屏叠加不受影响）。体外实验（macOS 26）证明：
            // 只有「窗口可见时 CanJoinAllSpaces 位发生 0→1 转变」才触发 WindowServer
            // 重新注册贴附；隐藏时改值、或同一个 runloop tick 里连写两个值（被合并）
            // 都是 no-op。所以 273 必须等 orderFront 之后的下一个 tick 再写（见下方）。
            let low = STATIONARY | FULL_SCREEN_AUXILIARY;
            let _: () = msg_send![ns_window, setCollectionBehavior: low];
        } else {
            let _: () = msg_send![ns_window, setCollectionBehavior: BEHAVIOR];
        }
        let _: () = msg_send![ns_window, orderFrontRegardless];
    }
    if reassert_spaces {
        // 换线程再回主线程，保证落在 orderFront 之后的另一个 runloop tick——
        // run_on_main_thread 在主线程上会内联执行，起不到隔 tick 的作用。
        // 30ms 间隙里窗口以低值可见于当前桌面（用户正看着的那个），无可感知差异。
        let app = app.clone();
        let window = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            let _ = app.run_on_main_thread(move || {
                let Ok(handle) = window.ns_window() else { return };
                let ns_window = handle as *mut AnyObject;
                if ns_window.is_null() {
                    return;
                }
                unsafe {
                    let _: () = msg_send![ns_window, setCollectionBehavior: BEHAVIOR];
                }
            });
        });
    }
    true
}

#[cfg(target_os = "linux")]
pub(super) fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    true
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(super) fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
    _reassert_spaces: bool,
) -> bool {
    false
}

#[cfg(target_os = "windows")]
pub(super) fn hide_capsule_window_if_present() {
    use std::iter::once;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, ShowWindow, HWND_NOTOPMOST, SWP_HIDEWINDOW, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    };

    let title: Vec<u16> = "OpenLess Capsule".encode_utf16().chain(once(0)).collect();
    let hwnd = match unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        Ok(hwnd) => hwnd,
        Err(_) => return,
    };
    if hwnd == HWND::default() || hwnd.0.is_null() {
        return;
    }

    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        )
    };
}

#[cfg(not(target_os = "windows"))]
pub(super) fn hide_capsule_window_if_present() {}

/// Esc 独占判定：胶囊显示「进行中」（录音/转写/润色）且确为 dictation 会话（phase 非
/// Idle）时为 true——tap/hook 吞掉 Esc 不透传宿主应用。phase 条件专门排除 QA：QA 也走
/// 胶囊，但它的 Esc 由聚焦浮窗处理（#161），全局吞键反而会把它挡掉。纯函数便于表格测试。
fn esc_exclusive_for_capsule(state: CapsuleState, phase: SessionPhase) -> bool {
    matches!(
        state,
        CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
    ) && !matches!(phase, SessionPhase::Idle)
}

pub(super) fn emit_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
) -> u64 {
    emit_capsule_with_context(
        inner,
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        false,
    )
}

/// 选区润色复用原有无焦点 capsule 窗口，但用独立标记让前端显示一行轻量状态提示，
/// 不污染语音/QA 的光效和终态文案。
pub(super) fn emit_selection_polish_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    message: impl Into<String>,
) -> u64 {
    emit_capsule_with_context(inner, state, 0.0, 0, Some(message.into()), None, true)
}

fn emit_capsule_with_context(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
    selection_polish: bool,
) -> u64 {
    let _event_guard = inner.capsule_event_lock.lock();
    emit_capsule_with_context_locked(
        inner,
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        selection_polish,
    )
}

/// `capsule_event_lock` 已由调用方持有的内部实现。自动隐藏路径必须能在验证 epoch
/// 后、发出 Idle 前一直持锁，才能保证旧 timer 不会盖掉刚到的新 payload。
fn emit_capsule_with_context_locked(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
    selection_polish: bool,
) -> u64 {
    // 每次 payload 都推进代数。这样一个选区润色终态的旧 timer 在之后出现任何
    // selection / voice / QA 状态时都失效，不会把新的可见状态强行收回 Idle。
    let event_epoch = inner
        .capsule_event_epoch
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    inner
        .selection_polish_capsule_active
        .store(selection_polish, Ordering::SeqCst);
    // 在 app 句柄校验之前记录，便于无 GUI 的测试断言「按下热键 → 弹了哪种胶囊」。
    // replace 顺带取回上一帧 state，用于判断本次是不是「入场帧」（见下方 defer_capsule_emit）。
    let prev_state = inner.last_capsule_state.lock().replace(state);
    // Esc 独占窗口：胶囊显示进行中（录音/转写/润色）且确为 dictation 会话（phase 非
    // Idle）时，tap/hook 吞掉 Esc 不透传宿主应用——此刻 Esc 的语义是「取消这个会话」，
    // 双重派发会顺带触发宿主应用的 Esc（如取消 Claude 正在生成的回复）。phase 条件排除
    // QA：QA 会话也走胶囊，但它的 Esc 由聚焦的浮窗窗口处理，吞键反而会把它挡掉。
    // 终止帧（Done/Cancelled/Error/Idle）自然清除。emit_capsule 是所有会话状态变化的
    // 单一出口（含 #77 审计保证的全部终止路径），在此维护不会漏路径。
    let esc_exclusive = esc_exclusive_for_capsule(state, inner.state.lock().phase);
    crate::hotkey::set_esc_exclusive(esc_exclusive);
    let app_opt = inner.app.lock().clone();
    let Some(app) = app_opt else {
        return event_epoch;
    };
    // 选区润色不属于语音翻译 / Less Computer，会话之间残留的标志不能带进其提示。
    let translation = !selection_polish && inner.translation_active.load(Ordering::SeqCst);
    let operating = !selection_polish && inner.state.lock().voice_agent;
    // 预备态只对 Recording 有意义：麦克风还没吐第一帧 PCM 时（capsule_warming=true）把
    // warming 打成 true，前端渲染「待命」光效；level_handler 首触发后翻 false → 光条点亮。
    let warming = !selection_polish
        && matches!(state, CapsuleState::Recording)
        && inner.capsule_warming.load(Ordering::SeqCst);
    let payload = CapsulePayload {
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        translation,
        operating,
        warming,
        selection_polish,
        // 用户选择的胶囊样式：读 Inner 上的原子缓存（主线程闭包每帧从 prefs 同步），
        // 不在音频回调线程碰偏好锁。设置里切换后下一次录音即生效。
        capsule_style: match inner.capsule_style.load(Ordering::Relaxed) {
            1 => CapsuleStyle::Classic,
            _ => CapsuleStyle::Siri,
        },
    };

    #[cfg(target_os = "android")]
    crate::android::notify_capsule_state(&payload);

    // visible / translation 是「这一帧 capsule:state event 的 payload」内容 ——
    // 必须在 call-site（即音频线程触发 emit_capsule 时）就算定，否则 main thread
    // 闭包里读到的将是「下一帧」的 state，跟实际下发给 JS 的 payload 不一致。
    let visible = !matches!(state, CapsuleState::Idle);
    // 入场帧：胶囊从不可见第一次变可见。按平时的「同步 emit + 异步 show」，前端会在窗口
    // 还隐藏时就起播 capsule-in，等窗口真 show 出来动画早已播完 → 用户看到胶囊「凭空出
    // 现」而非「滑入」。修法：入场帧把发给 capsule 窗口的事件推迟到主线程闭包里、
    // window.show 之后再 emit，保证前端起播入场动画时窗口已可见、动画完整可见。Linux 不
    // 走胶囊窗口（文字经 fcitx5 直接 commit），保持原同步 emit 不变。
    let was_visible = matches!(prev_state, Some(s) if !matches!(s, CapsuleState::Idle));
    let defer_capsule_emit = visible && !was_visible && cfg!(not(target_os = "linux"));

    // Linux: 通过 fcitx5 插件在候选词列表下方显示听写状态，不干扰输入法预编辑。
    // 只在文本变化时调用 DBus，避免录音中 ~30Hz 的音频电平回调重复调用。
    #[cfg(target_os = "linux")]
    {
        use std::sync::Mutex;
        static LAST_AUX: Mutex<Option<String>> = Mutex::new(None);

        let aux = match state {
            CapsuleState::Idle => None,
            CapsuleState::Recording => Some("🎤 收音中..."),
            CapsuleState::Transcribing => Some("🔄 识别中..."),
            CapsuleState::Polishing => Some("✨ 润色中..."),
            CapsuleState::Done => Some("✅ 已插入"),
            CapsuleState::Cancelled => Some("— 已取消"),
            CapsuleState::Error => Some("❌ 出错"),
        };

        let mut last = LAST_AUX.lock().unwrap();
        if aux != last.as_deref() {
            *last = aux.map(String::from);
            // 代数计数器：每次状态变化 +1，retry 线程只在自己代数仍为最新时生效。
            // 避免 Recording→Idle→Recording 快速切换时多个 retry 重复触发。
            static RETRY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            // fetch_add 返回旧值，所以 latest_gen > gen+1 才表示"在我之后又发生了变更"。
            let gen = RETRY_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match aux {
                Some(t) => {
                    log::info!("[capsule] set_aux_down: {t} gen={gen}");
                    let text = t.to_string();
                    std::thread::spawn(move || {
                        let current = LAST_AUX.lock().unwrap().clone();
                        if current.as_deref() != Some(&text) {
                            log::info!(
                                "[capsule] set_aux_down skipped: state changed to {current:?}"
                            );
                            return;
                        }
                        if let Err(e) = crate::linux_fcitx::set_aux_down(&text) {
                            log::warn!("[capsule] set_aux_down failed: {e}");
                        }
                    });
                    // 终态（Done/Cancelled/Error）3 秒后自动清除，避免一直跟随焦点。
                    if matches!(
                        state,
                        CapsuleState::Done | CapsuleState::Cancelled | CapsuleState::Error
                    ) {
                        let text = t.to_string();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            let latest_gen = RETRY_GEN.load(std::sync::atomic::Ordering::SeqCst);
                            if latest_gen > gen + 1 {
                                return;
                            }
                            let current = LAST_AUX.lock().unwrap().clone();
                            if current.as_deref() != Some(&text) {
                                return;
                            }
                            log::info!("[capsule] auto-clear terminal state: {text}");
                            let _ = crate::linux_fcitx::set_aux_down("");
                            *LAST_AUX.lock().unwrap() = None;
                        });
                    }
                }
                None => {
                    log::info!("[capsule] clear_aux_down gen={gen}");
                    std::thread::spawn(move || {
                        let latest_gen = RETRY_GEN.load(std::sync::atomic::Ordering::SeqCst);
                        if latest_gen > gen + 1 {
                            log::info!(
                                "[capsule] clear_aux_down skipped: gen {gen}, latest {latest_gen}"
                            );
                            return;
                        }
                        let current = LAST_AUX.lock().unwrap().clone();
                        if current.is_some() {
                            log::info!(
                                "[capsule] clear_aux_down skipped: state changed to {current:?}"
                            );
                            return;
                        }
                        if let Err(e) = crate::linux_fcitx::clear_aux_down() {
                            log::warn!("[capsule] clear_aux_down failed: {e}");
                        }
                    });
                }
            }
        }
    }

    // emit_capsule 会被 cpal process_callback（音频回调线程）调用 ~30 Hz —— 在该
    // 线程上调用 NSWindow / HWND API 会撞 macOS dispatch_assert_queue_fail SIGTRAP
    // 或者 Win32 SendMessage 死锁。把 window.show/hide + 位置调整 marshal 到主线程；
    // app.emit_to 走 Tauri 内部事件总线，本身线程安全，保留同步调用。详见 audit 3.2.2。
    //
    // show_capsule（用户偏好）在主线程执行时再读 —— 用户可以在录音过程中改设置，
    // 闭包入队到真正跑之间窗口上限是一两帧（~16-33ms），用最新值消除 stale-pref
    // 闪烁。pr_agent 关注点 — 见 audit follow-up。
    let inner_for_main = Arc::clone(inner);
    let app_for_main = app.clone();
    // 入场帧要在 window.show 之后、闭包内部把 state 回发给前端，需要 payload 的独立副本
    // move 进闭包；非入场帧走闭包外的即时同步 emit（下方），这里就是 None。
    // 注意：入场帧的 payload 在闭包同步 capsule_style 原子之前克隆，最多带一帧旧样式
    //（设置里刚切换后的首次录音，第 2 帧 ~33ms 即纠正）。这是刻意取舍——不要在音频
    // 线程改回直接读 prefs。前端第 1 帧处于 capsule-in 动画期间（380ms），无感知。
    let payload_for_deferred_emit = if defer_capsule_emit {
        Some(payload.clone())
    } else {
        None
    };
    let _ = app.run_on_main_thread(move || {
        let Some(window) = app_for_main.get_webview_window("capsule") else {
            // #470 诊断 v2：比 A/B/C 更靠前的暗点 A0 —— capsule webview 句柄取不到
            // （窗口未创建/已销毁）。此前静默 return，无法观测。一次性 warn。
            if !CAPSULE_WINDOW_MISSING_LOGGED.swap(true, Ordering::SeqCst) {
                log::warn!(
                    "[capsule] capsule webview window not found — emit_capsule show path skipped (state={})",
                    capsule_state_log_name(state)
                );
            }
            return;
        };
        // `show_capsule` 是原有“录音胶囊”偏好；Selection Polish 没有独立开关，且它的
        // 无选区/失败提示是这条无界面工作流的唯一反馈，所以始终展示轻量提示。
        let prefs_snapshot = inner_for_main.prefs.get();
        let show_capsule = selection_polish || prefs_snapshot.show_capsule;
        // 把胶囊样式同步进 Inner 原子缓存：emit_capsule（音频回调线程）从这里读
        // payload.capsuleStyle，避免在音频线程碰偏好锁。主线程每帧克隆 prefs 本就有
        //（show_capsule 同源），多读一个字段零额外代价。
        let classic_style = matches!(prefs_snapshot.capsule_style, CapsuleStyle::Classic);
        inner_for_main.capsule_style.store(
            if classic_style { 1 } else { 0 },
            Ordering::Relaxed,
        );
        // Linux: 不操作胶囊窗口（不 show/hide，不 reposition）。
        // 文字通过 fcitx5 插件直接 commit，用户始终在目标 app 中。
        #[cfg(target_os = "linux")]
        {
            return;
        }
        #[cfg(not(target_os = "linux"))]
        {

        // 三平台统一：Done / Cancelled / Error 状态保留 ~1.5s toast
        // （schedule_capsule_idle 之后会回 Idle 隐藏）。
        // Windows 上 linger 的真实问题（截图选中 / 死区 / 拖拽卡顿）由 #140 加的
        // `hide_capsule_window_if_present()` Win32 hard-hide 在 visible=false 分支
        // 处理，不依赖把 Done/Cancelled/Error 打成 invisible。详见 PR #140 评论。
        maybe_position_capsule_bottom_center(&inner_for_main, &window, translation);
        // 经典药丸（Openless 默认风格）的 ✕/✓ 按钮需要接收点击：录音/转写/润色期间
        // 关掉鼠标穿透（按钮可点，代价是窗口底部 460×180 区域在这几秒内拦截点击——
        // 与 1.3.x 经典胶囊同款取舍）；终态 toast、隐藏、Siri 光效、选区润色提示一律
        // 保持穿透，不遮挡底层 app。set_ignore_cursor_events 只在值变化时调用一次。
        // Android 没有胶囊窗口，tauri 的 set_ignore_cursor_events 在其上不可用。
        #[cfg(not(mobile))]
        {
            let interactive = classic_style
                && visible
                && !selection_polish
                && matches!(
                    state,
                    CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing
                );
            let want_passthrough = !interactive;
            if inner_for_main
                .capsule_cursor_passthrough
                .swap(want_passthrough, Ordering::SeqCst)
                != want_passthrough
            {
                if let Err(e) = window.set_ignore_cursor_events(want_passthrough) {
                    log::warn!("[capsule] set_ignore_cursor_events failed: {e}");
                }
            }
        }
        if show_capsule && visible {
            // 用户报"看不到胶囊"时第一时间能在 log 里确认：胶囊路径有跑、show_capsule
            // 开关是 true、当前进入 visible 帧 —— 排除 prefs 没存住 / emit_capsule 没触
            // 发 / state 一直 Idle 这几类常见 root cause。issue #470。
            if !CAPSULE_FIRST_SHOW_LOGGED.swap(true, Ordering::SeqCst) {
                log::info!(
                    "[capsule] first show this session: show_capsule=true visible=true state={}",
                    capsule_state_log_name(state)
                );
            }
            // 入场帧（隐藏→可见）强制重注册 Space 贴附：macOS 26 观测到系统会在运行中
            // 把窗口从「全 Space 贴附」剥离（2026-07-31 实测：胶囊被钉死单个 Space，
            // 其它桌面上听写全程不可见），而 setCollectionBehavior 写入相同值是 no-op，
            // 之后每帧重写 273 都救不回来。只在入场帧做一次 0→273 的翻转即可恢复注册，
            // 30Hz 的 level 帧不做（每帧翻转会让 WindowServer 反复重排窗口）。
            show_capsule_window_for_recording(
                &app_for_main,
                &window,
                payload_for_deferred_emit.is_some(),
            );
            // macOS/Windows 优先走 no-activate show，避免录音胶囊抢走当前工作 app 焦点。
            // 若 fallback 到 show()，OpenLess 已是前台 app 时再把 key window 还给 main。
            #[cfg(target_os = "macos")]
            crate::restore_main_window_key_if_active(&app_for_main);
        } else {
            // show_capsule 开关被用户关掉但本次确实想显示（visible=true）的情况：
            // 一次性 info log，让用户报"胶囊没显示"时能在日志里一眼看到根因 —— 维护者
            // 不必再让用户"去打开设置确认"。issue #470。
            if !show_capsule
                && visible
                && !CAPSULE_SUPPRESSED_BY_TOGGLE_LOGGED.swap(true, Ordering::SeqCst)
            {
                log::info!(
                    "[capsule] suppressed by user toggle: show_capsule=false visible=true state={}",
                    capsule_state_log_name(state)
                );
            }
            hide_capsule_window_if_present();
            let _ = window.hide();
        }
        // 入场帧：窗口刚 show（或本次用户关了胶囊显示走了 hide 分支），此刻再把 state 发给
        // capsule 前端 —— 前端起播 capsule-in 时窗口已可见，入场动画从头完整播放。
        if let Some(payload) = payload_for_deferred_emit.as_ref() {
            let _ = app_for_main.emit_to("capsule", "capsule:state", payload);
        }
        }
    });

    // 非入场帧（含 Linux、录音中的 level 更新、离场/终态）保持即时同步 emit，最低延迟；
    // 入场帧已在上面的主线程闭包里、window.show 之后 emit 过，这里跳过避免重复下发。
    if !defer_capsule_emit {
        let _ = app.emit_to("capsule", "capsule:state", &payload);
    }
    // 主窗口也需要 capsule:state 事件：AudioCueListener 用它触发录音提示音。
    // Linux 上胶囊隐藏时提示音仍应工作，所以同时发给 main 窗口。始终即时，与胶囊窗口
    // 显示时机解耦。
    let _ = app.emit_to("main", "capsule:state", &payload);
    event_epoch
}

/// 返回一个选区润色终态 timer 是否仍有资格收起 capsule。
///
/// 该判断同时覆盖两类竞态：同一功能的新一轮触发，以及随后开始的语音/QA 会话。
pub(super) fn selection_polish_capsule_epoch_is_current(
    inner: &Arc<Inner>,
    expected_epoch: u64,
) -> bool {
    inner.selection_polish_capsule_active.load(Ordering::SeqCst)
        && inner.capsule_event_epoch.load(Ordering::SeqCst) == expected_epoch
}

/// 旧 dictation/QA timer 的收起路径。它与所有 emit 共享一把短锁：如果 Selection
/// Polish 已经显示，就让路；如果新语音/QA 先一步发了状态，也会在锁序上排在 Idle 前。
pub(super) fn hide_capsule_if_all_sessions_idle(inner: &Arc<Inner>) {
    // 先读 session lock，再进 capsule lock。QA 收尾路径会持有 qa_state 并 emit；反过来
    // 在这里持 capsule lock 等 qa_state 会产生锁反转。event epoch 负责在两次读取之间
    // 有任何新 payload 时取消本次 Idle。
    let dictation_idle = inner.state.lock().phase == SessionPhase::Idle;
    let qa_idle = inner.qa_state.lock().phase == QaPhase::Idle;
    let selection_polish_active = inner.selection_polish_capsule_active.load(Ordering::SeqCst);
    let observed_epoch = inner.capsule_event_epoch.load(Ordering::SeqCst);
    if !dictation_idle || !qa_idle || selection_polish_active {
        return;
    }

    let _event_guard = inner.capsule_event_lock.lock();
    if inner.capsule_event_epoch.load(Ordering::SeqCst) == observed_epoch
        && !inner.selection_polish_capsule_active.load(Ordering::SeqCst)
    {
        emit_capsule_with_context_locked(inner, CapsuleState::Idle, 0.0, 0, None, None, false);
    }
}

/// 只在同一代 Selection Polish 终态仍是最新可见 capsule 时收起它。锁会让“检查 +
/// 发送 Idle”成为一个不可插队的顺序点，因此旧 timer 不可能在新会话之后覆盖 UI。
pub(super) fn hide_selection_polish_capsule_if_current(inner: &Arc<Inner>, expected_epoch: u64) {
    let _event_guard = inner.capsule_event_lock.lock();
    if selection_polish_capsule_epoch_is_current(inner, expected_epoch) {
        emit_capsule_with_context_locked(inner, CapsuleState::Idle, 0.0, 0, None, None, false);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CapsuleLayoutState {
    translation_active: bool,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    scale_bits: u64,
}

/// 返回胶囊「应该摆放到的显示器」的标识信息。
///
/// 它看的显示器必须和 `position_capsule_bottom_center` 实际定位用的一致：
/// Windows 看「正在输入的 App 所在显示器」，macOS 看「鼠标光标所在显示器」，
/// 其它平台看胶囊自己的显示器。这是「是否需要重新定位」去重缓存
/// （`maybe_position_capsule_bottom_center`）的 key，如果这里看错了显示器，
/// 就会出现「焦点/光标移到另一块屏、胶囊却没跟过去」的 bug。
pub(super) fn capsule_layout_snapshot<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    translation_active: bool,
) -> Option<CapsuleLayoutState> {
    // Windows：以「正在输入的 App 所在显示器」为基准。若用胶囊自己的
    // current_monitor，输入焦点切到另一块屏时胶囊仍在原屏 → 误判「没变化」
    // → 跳过重新定位。
    #[cfg(target_os = "windows")]
    {
        if let Some(mon) = crate::foreground_window_monitor() {
            return Some(CapsuleLayoutState {
                translation_active,
                monitor_x: mon.left,
                monitor_y: mon.top,
                monitor_width: (mon.right - mon.left).max(0) as u32,
                monitor_height: (mon.bottom - mon.top).max(0) as u32,
                scale_bits: mon.scale.to_bits(),
            });
        }
        // 仅当 Win32 取不到前台显示器时，落回下面的 current_monitor。
    }
    // macOS：以「鼠标光标所在显示器」为基准，必须和
    // position_capsule_bottom_center 实际定位用的同一块屏；否则光标移到另一块
    // 屏时这里仍读到胶囊旧屏 → 误判「没变化」→ 跳过重新定位 → 胶囊锁死在第一块屏。
    #[cfg(target_os = "macos")]
    {
        if let Some(mon) = crate::capsule_target_monitor(window) {
            return Some(CapsuleLayoutState {
                translation_active,
                monitor_x: mon.physical_x,
                monitor_y: mon.physical_y,
                monitor_width: mon.physical_width,
                monitor_height: mon.physical_height,
                scale_bits: mon.scale.to_bits(),
            });
        }
        // 取不到光标 / AX 位置时落回下面的 current_monitor。
    }
    let monitor = window.current_monitor().ok().flatten()?;
    Some(CapsuleLayoutState {
        translation_active,
        monitor_x: monitor.position().x,
        monitor_y: monitor.position().y,
        monitor_width: monitor.size().width,
        monitor_height: monitor.size().height,
        scale_bits: monitor.scale_factor().to_bits(),
    })
}

pub(super) fn maybe_position_capsule_bottom_center<R: tauri::Runtime>(
    inner: &Arc<Inner>,
    window: &tauri::WebviewWindow<R>,
    translation_active: bool,
) {
    let Some(next) = capsule_layout_snapshot(window, translation_active) else {
        return;
    };
    {
        let last = inner.capsule_layout.lock();
        if last.as_ref() == Some(&next) {
            return;
        }
    }
    if crate::position_capsule_bottom_center(window, translation_active).is_ok() {
        let mut last = inner.capsule_layout.lock();
        *last = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CapsuleState;

    #[test]
    fn esc_exclusive_flag_matches_capsule_and_phase() {
        // 进行中胶囊 + dictation phase 非 Idle → 独占 Esc（不透传宿主应用）。
        for (state, phase) in [
            (CapsuleState::Recording, SessionPhase::Listening),
            (CapsuleState::Transcribing, SessionPhase::Processing),
            (CapsuleState::Polishing, SessionPhase::Processing),
            (CapsuleState::Recording, SessionPhase::Inserting),
        ] {
            assert!(
                esc_exclusive_for_capsule(state, phase),
                "{state:?} @ {phase:?} 应独占 Esc"
            );
        }

        // 终止帧（Done/Cancelled/Error/Idle）→ 清除独占。
        for (state, phase) in [
            (CapsuleState::Done, SessionPhase::Idle),
            (CapsuleState::Cancelled, SessionPhase::Idle),
            (CapsuleState::Error, SessionPhase::Idle),
            (CapsuleState::Idle, SessionPhase::Idle),
        ] {
            assert!(
                !esc_exclusive_for_capsule(state, phase),
                "{state:?} @ {phase:?} 不应独占 Esc"
            );
        }

        // QA 场景：胶囊显示进行中但 dictation phase=Idle → 不独占（Esc 归浮窗，#161）。
        for (state, phase) in [
            (CapsuleState::Recording, SessionPhase::Idle),
            (CapsuleState::Transcribing, SessionPhase::Idle),
            (CapsuleState::Polishing, SessionPhase::Idle),
        ] {
            assert!(
                !esc_exclusive_for_capsule(state, phase),
                "{state:?} @ {phase:?}（QA）不应独占 Esc"
            );
        }
    }
}
