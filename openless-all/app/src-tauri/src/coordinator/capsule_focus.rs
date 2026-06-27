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
#[cfg(target_os = "macos")]
pub(super) fn capture_frontmost_app() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let cls = AnyClass::get("NSWorkspace")?;
        let workspace: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let name_obj: *mut AnyObject = msg_send![app, localizedName];
        let bundle_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        let name = nsstring_to_string(name_obj);
        let bundle = nsstring_to_string(bundle_obj);
        match (name, bundle) {
            (Some(n), Some(b)) => Some(format!("{n} ({b})")),
            (Some(n), None) => Some(n),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn nsstring_to_string(ns_string: *mut objc2::runtime::AnyObject) -> Option<String> {
    use objc2::msg_send;
    if ns_string.is_null() {
        return None;
    }
    let utf8: *const std::os::raw::c_char = unsafe { msg_send![ns_string, UTF8String] };
    if utf8.is_null() {
        return None;
    }
    let cstr = unsafe { std::ffi::CStr::from_ptr(utf8) };
    let s = cstr.to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "windows")]
pub(super) fn capture_frontmost_app() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buf);
        if copied <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn capture_frontmost_app() -> Option<String> {
    None
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
    _app: &AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
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
    unsafe {
        const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
        const STATIONARY: usize = 1 << 4;
        const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
        let behavior = CAN_JOIN_ALL_SPACES | STATIONARY | FULL_SCREEN_AUXILIARY;
        let _: () = msg_send![ns_window, setLevel: 25i64];
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        let _: () = msg_send![ns_window, orderFrontRegardless];
    }
    true
}

#[cfg(target_os = "linux")]
pub(super) fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
) -> bool {
    true
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(super) fn show_capsule_window_no_activate<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    _window: &tauri::WebviewWindow<R>,
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

pub(super) fn emit_capsule(
    inner: &Arc<Inner>,
    state: CapsuleState,
    level: f32,
    elapsed_ms: u64,
    message: Option<String>,
    inserted_chars: Option<u32>,
) {
    // 在 app 句柄校验之前记录，便于无 GUI 的测试断言「按下热键 → 弹了哪种胶囊」。
    *inner.last_capsule_state.lock() = Some(state);
    let app_opt = inner.app.lock().clone();
    let Some(app) = app_opt else { return };
    let translation = inner.translation_modifier_seen.load(Ordering::SeqCst);
    let operating = inner.state.lock().voice_agent;
    let payload = CapsulePayload {
        state,
        level,
        elapsed_ms,
        message,
        inserted_chars,
        translation,
        operating,
    };

    #[cfg(target_os = "android")]
    crate::android::notify_capsule_state(&payload);

    // visible / translation 是「这一帧 capsule:state event 的 payload」内容 ——
    // 必须在 call-site（即音频线程触发 emit_capsule 时）就算定，否则 main thread
    // 闭包里读到的将是「下一帧」的 state，跟实际下发给 JS 的 payload 不一致。
    let visible = !matches!(state, CapsuleState::Idle);

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
        let show_capsule = inner_for_main.prefs.get().show_capsule;
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
            show_capsule_window_for_recording(&app_for_main, &window);
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
        }
    });

    let _ = app.emit_to("capsule", "capsule:state", &payload);
    // 主窗口也需要 capsule:state 事件：AudioCueListener 用它触发录音提示音。
    // Linux 上胶囊隐藏时提示音仍应工作，所以同时发给 main 窗口。
    let _ = app.emit_to("main", "capsule:state", &payload);
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
