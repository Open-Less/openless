//! Hotkey supervisor / bridge loops and shortcut wiring extracted from
//! `coordinator.rs` (behavior-preserving move; see git history).
//!
//! Functions operate on the parent `Inner`/`Coordinator` and reference
//! parent-module items via `use super::*;`. Visibility is `pub(super)` so the
//! parent `coordinator` module can call them through `use hotkey_loops::*;`.

use super::*;

// ─────────────────────────── hotkey bridging ───────────────────────────

/// Esc 取消专用消费线程。为什么不并入 `hotkey_bridge_loop`：bridge 为修 #468/#475
/// 的 latch 竞态把 Pressed/Released 改成了串行 block_on —— Hold 松手后 `end_session`
/// 会在 bridge 线程上同步跑完整段转写 + 润色，期间 bridge 无法 recv。若 Esc 与其同
/// 队列，取消事件只能排队等流程跑完（此时 phase 已回 Idle，cancel 变 no-op），#798
/// 在 `end_session` 里的 select! 取消赛跑永远等不到 `cancelled` 旗标 ——「转写 / 润色
/// 中按 Esc 停不下来」。独立通道 + 本线程保证 `cancel_session` 随到随执行（它是纯同步
/// 快路径：置旗标 + 清资源，不 await）。
pub(super) fn esc_cancel_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<()>) {
    esc_cancel_bridge_loop_with(inner, rx, |inner| {
        inner
            .host
            .block_on(super::dictation::cancel_active_session(inner));
    });
}

fn esc_cancel_bridge_loop_with(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<()>,
    cancel: impl Fn(&Arc<Inner>),
) {
    while rx.recv().is_ok() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        cancel(&inner);
    }
}

/// 组合键撤销专用消费线程。撤销事件携带触发键按下代次，避免独立通道的迟到事件
/// 误取消下一次按下开启的会话。
pub(super) fn combo_abort_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<crate::hotkey::HotkeyCombinedEdge>,
    handler: fn(&Arc<Inner>, crate::hotkey::HotkeyCombinedEdge),
) {
    while let Ok(edge) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        handler(&inner, edge);
    }
}

pub(super) fn spawn_esc_cancel_bridge(inner: &Arc<Inner>) -> mpsc::Sender<()> {
    let (cancel_tx, cancel_rx) = mpsc::channel::<()>();
    let bridge_inner = Arc::clone(inner);
    if let Err(e) = std::thread::Builder::new()
        .name("openless-esc-cancel-bridge".into())
        .spawn(move || esc_cancel_bridge_loop(bridge_inner, cancel_rx))
    {
        // 线程建不起来 = 取消通道没有消费者，Esc 取消会静默失效——这正是本 PR 想修的
        // bug 以另一种方式回归，必须留 error 日志以便排查。
        log::error!("[hotkey] esc-cancel-bridge 线程启动失败，Esc 取消将不可用: {e}");
    }
    cancel_tx
}

pub(super) fn spawn_combo_abort_bridge(
    inner: &Arc<Inner>,
    handler: fn(&Arc<Inner>, crate::hotkey::HotkeyCombinedEdge),
) -> mpsc::Sender<crate::hotkey::HotkeyCombinedEdge> {
    let (combo_tx, combo_rx) = mpsc::channel();
    let bridge_inner = Arc::clone(inner);
    std::thread::Builder::new()
        .name("openless-combo-abort-bridge".into())
        .spawn(move || combo_abort_bridge_loop(bridge_inner, combo_rx, handler))
        .ok();
    combo_tx
}

pub(super) fn hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    let capability = HotkeyMonitor::capability();
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let target = hotkey_runtime_target(&inner);

        if inner.hotkey.lock().is_some() {
            return;
        }
        // Linux: 启动前检查 fcitx5 插件是否可用
        #[cfg(target_os = "linux")]
        if !crate::linux_fcitx::available() {
            *inner.hotkey_status.lock() = HotkeyStatus {
                adapter: capability.adapter,
                state: HotkeyStatusState::Failed,
                message: Some("fcitx5 插件不可用 — 请确保 fcitx5 已安装且在运行".into()),
                last_error: Some(crate::types::HotkeyInstallError {
                    code: "fcitx5_unavailable".into(),
                    message: "fcitx5 插件 DBus 接口无响应".into(),
                }),
            };
            log::warn!("[hotkey-supervisor] fcitx5 plugin unavailable, retrying...");
            attempts += 1;
            std::thread::sleep(std::time::Duration::from_secs(3));
            continue;
        }
        *inner.hotkey_status.lock() = HotkeyStatus {
            adapter: capability.adapter,
            state: HotkeyStatusState::Starting,
            message: Some(format!("正在安装全局快捷键监听（第 {} 次）", attempts + 1)),
            last_error: None,
        };
        let trigger = crate::shortcut_binding::legacy_modifier_trigger(&target.dictation)
            .unwrap_or(crate::types::HotkeyTrigger::Custom);
        let binding = crate::types::HotkeyBinding {
            trigger,
            mode: target.dictation_mode,
            keys: None,
        };
        let (tx, rx) = mpsc::channel::<HotkeyEvent>();
        #[cfg(target_os = "linux")]
        let (fcitx_tx, fcitx_binding) = (tx.clone(), binding.clone());
        let cancel_tx = spawn_esc_cancel_bridge(&inner);
        let combo_tx = spawn_combo_abort_bridge(&inner, handle_trigger_combined);
        #[cfg(target_os = "linux")]
        let combo_tx_for_fcitx = combo_tx.clone();
        match HotkeyMonitor::start(binding, tx, cancel_tx, combo_tx) {
            Ok(monitor) => {
                let adapter = monitor.kind();
                *inner.hotkey.lock() = Some(monitor);
                if let Some(monitor) = inner.hotkey.lock().as_ref() {
                    let (qa_trigger, selection_polish_trigger, translation_trigger) =
                        modifier_shortcut_triggers(&inner);
                    monitor.update_modifier_shortcuts(
                        qa_trigger,
                        selection_polish_trigger,
                        translation_trigger,
                    );
                }
                *inner.hotkey_status.lock() = HotkeyStatus {
                    adapter,
                    state: HotkeyStatusState::Installed,
                    message: Some(format!("{} 已安装", adapter.display_name())),
                    last_error: None,
                };
                log::info!(
                    "[coord] hotkey listener installed (after {} attempt(s))",
                    attempts + 1
                );
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name("openless-hotkey-bridge".into())
                    .spawn(move || hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                // Linux: 启动 fcitx5 插件信号监听作为热键源。
                #[cfg(target_os = "linux")]
                {
                    let (qa_trigger, selection_polish_trigger, translation_trigger) =
                        modifier_shortcut_triggers(&inner);
                    let custom_key = custom_dictation_key_string(&inner);
                    crate::linux_fcitx::start_dictation_signal_listener(
                        fcitx_tx,
                        combo_tx_for_fcitx,
                        fcitx_binding.clone(),
                        qa_trigger,
                        selection_polish_trigger,
                        translation_trigger,
                        custom_key,
                    );
                    if fcitx_binding.trigger == crate::types::HotkeyTrigger::Custom {
                        sync_custom_dictation_to_plugin(&inner);
                    } else {
                        crate::linux_fcitx::sync_binding_to_plugin(&fcitx_binding);
                    }
                }
                return;
            }
            Err(e) => {
                attempts += 1;
                let error_message = e.message.clone();
                *inner.hotkey_status.lock() = HotkeyStatus {
                    adapter: capability.adapter,
                    state: HotkeyStatusState::Failed,
                    message: Some(error_message.clone()),
                    last_error: Some(e),
                };
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] hotkey listener attempt #{attempts} failed: {}; retrying in 3s",
                        error_message
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

// ─────────────────────────── QA hotkey supervisor ───────────────────────────

pub(super) fn qa_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        // 用户已经把 QA 关掉就睡着等 runtime target 改动；改动通过显式 settings effect 唤醒。
        let binding = match hotkey_runtime_target(&inner).qa {
            Some(b) => b,
            None => {
                inner.qa_hotkey.lock().take();
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        };
        if crate::shortcut_binding::legacy_modifier_trigger(&binding).is_some() {
            inner.qa_hotkey.lock().take();
            if let Some(monitor) = inner.hotkey.lock().as_ref() {
                let (qa_trigger, selection_polish_trigger, translation_trigger) =
                    modifier_shortcut_triggers(&inner);
                monitor.update_modifier_shortcuts(
                    qa_trigger,
                    selection_polish_trigger,
                    translation_trigger,
                );
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        }

        if inner.qa_hotkey.lock().is_some() {
            // 已注册成功 → 不重复装；睡 5s 复查（ binding 变化由 update 路径手动触发 ）。
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        }

        // global-hotkey crate 在 macOS 走 Carbon RegisterEventHotKey，要求 manager
        // 在主线程构造，否则 register() 看起来 Ok 但事件根本不会派发——这是 issue #118
        // PR #119 第一版漏掉的关键步骤，导致用户按了 hotkey 完全无反应。这里通过
        // run_on_main_thread 把 QaHotkeyMonitor::start 跳到主线程跑，结果再回 channel。
        let (tx, rx) = mpsc::channel::<QaHotkeyEvent>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<QaHotkeyMonitor, QaHotkeyError>>(1);
        let binding_for_main = binding.clone();
        if inner
            .host
            .run_on_main_thread(move || {
                let result = QaHotkeyMonitor::start(binding_for_main, tx);
                let _ = init_tx.send(result);
            })
            .is_err()
        {
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        // run_on_main_thread 是 fire-and-forget；等主线程跑完结果回来。给 5s 上限避免
        // 主线程繁忙时 supervisor 永久阻塞。
        let init_result = match init_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] QA hotkey 第 {attempts} 次注册超时（主线程未回执）；3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
        };

        match init_result {
            Ok(monitor) => {
                *inner.qa_hotkey.lock() = Some(monitor);
                log::info!(
                    "[coord] QA hotkey listener installed on main thread (after {} attempt(s))",
                    attempts + 1
                );
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name("openless-qa-hotkey-bridge".into())
                    .spawn(move || qa_hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                attempts = 0;
            }
            Err(e) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!("[coord] QA hotkey 第 {attempts} 次注册失败: {e}; 3s 后重试");
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

pub(super) fn qa_hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<QaHotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            QaHotkeyEvent::Pressed => {
                inner
                    .host
                    .spawn(async move { handle_qa_hotkey_pressed(&inner_cloned).await });
            }
        }
    }
}

// ─────────────────────── Selection Polish hotkey ───────────────────────
// 选区润色为桌面（Windows-first）工作流，mobile 不注册全局热键。

#[cfg(not(mobile))]
pub(super) fn selection_polish_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts = 0_u32;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        match try_update_selection_polish_hotkey_binding(&inner) {
            Ok(()) => return,
            Err(error) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[selection-polish] hotkey registration attempt #{attempts} failed: {error}; retrying in 3s"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

#[cfg(not(mobile))]
pub(super) fn try_update_selection_polish_hotkey_binding(inner: &Arc<Inner>) -> Result<(), String> {
    let binding = hotkey_runtime_target(inner).selection_polish;
    let Some(binding) = binding else {
        take_selection_polish_hotkey_on_main_thread(inner);
        update_selection_polish_modifier_shortcut(inner);
        return Ok(());
    };

    if crate::shortcut_binding::legacy_modifier_trigger(&binding).is_some() {
        take_selection_polish_hotkey_on_main_thread(inner);
        update_selection_polish_modifier_shortcut(inner);
        return Ok(());
    }

    // A generic combo is registered by global-hotkey on the UI thread. It is
    // deliberately not routed through the side-aware singleton: side-specific
    // combos remain dictation-only until that monitor supports multiple owners.
    update_selection_polish_modifier_shortcut(inner);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let inner_for_main = Arc::clone(inner);
    inner.host.run_on_main_thread(move || {
        let result = update_selection_polish_hotkey_on_main_thread(inner_for_main, binding)
            .map_err(|error| error.to_string());
        let _ = result_tx.send(result);
    })?;
    result_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "Selection Polish hotkey registration timed out".to_string())?
}

#[cfg(not(mobile))]
fn update_selection_polish_modifier_shortcut(inner: &Arc<Inner>) {
    if let Some(monitor) = inner.hotkey.lock().as_ref() {
        let (qa_trigger, selection_polish_trigger, translation_trigger) =
            modifier_shortcut_triggers(inner);
        monitor.update_modifier_shortcuts(
            qa_trigger,
            selection_polish_trigger,
            translation_trigger,
        );
    }
}

#[cfg(not(mobile))]
fn update_selection_polish_hotkey_on_main_thread(
    inner: Arc<Inner>,
    binding: crate::types::ShortcutBinding,
) -> Result<(), ComboHotkeyError> {
    if let Some(monitor) = inner.selection_polish_hotkey.lock().as_ref() {
        return monitor.update_binding(binding);
    }
    let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
    let monitor = ComboHotkeyMonitor::start(binding, tx)?;
    *inner.selection_polish_hotkey.lock() = Some(monitor);
    let bridge_inner = Arc::clone(&inner);
    std::thread::Builder::new()
        .name("openless-selection-polish-hotkey-bridge".into())
        .spawn(move || selection_polish_hotkey_bridge_loop(bridge_inner, rx))
        .map_err(|error| {
            ComboHotkeyError::RegisterFailed(format!("spawn bridge thread: {error}"))
        })?;
    Ok(())
}

#[cfg(not(mobile))]
fn selection_polish_hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<ComboHotkeyEvent>) {
    while let Ok(event) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        match event {
            ComboHotkeyEvent::Pressed { .. } => {
                crate::selection::prefetch_selection_workspace_capture();
                handle_selection_workspace_hotkey_pressed(&inner);
            }
            ComboHotkeyEvent::Released { .. } => {
                handle_selection_workspace_hotkey_released(&inner);
            }
        }
    }
}

#[cfg(not(mobile))]
fn handle_selection_workspace_hotkey_pressed(inner: &Arc<Inner>) {
    #[cfg(target_os = "windows")]
    if inner.backend.get_preferences().selection_voice_enabled {
        let inner_cloned = Arc::clone(inner);
        inner.host.spawn(async move {
            super::selection_voice_session::handle_selection_voice_pressed(&inner_cloned).await;
        });
        return;
    }
    let inner = Arc::clone(inner);
    let host = inner.host.clone();
    host.spawn(async move {
        let result = inner
            .backend
            .services()
            .selection
            .begin_polish(openless_core::SelectionPolishRequest {
                selected_text: None,
                mode: openless_core::PolishMode::Raw,
                instruction: None,
            })
            .await;
        match result {
            Ok(_) => match inner.backend.services().selection.snapshot().await {
                Ok(snapshot) => {
                    let message = match snapshot.phase {
                        openless_core::SelectionPhase::Preview => "已打开预览，等待确认",
                        openless_core::SelectionPhase::Completed => match snapshot.insert_outcome {
                            Some(openless_core::InsertOutcome::CopiedFallback) => {
                                "已复制结果，请手动粘贴"
                            }
                            _ => "已替换",
                        },
                        _ => return,
                    };
                    let epoch = emit_selection_polish_capsule(&inner, CapsuleState::Done, message);
                    schedule_selection_polish_capsule_idle(
                        &inner,
                        epoch,
                        CAPSULE_AUTO_HIDE_DELAY_MS,
                    );
                }
                Err(error) => {
                    log::warn!("[selection-polish] read completed snapshot failed: {error}");
                }
            },
            Err(error) => {
                log::warn!("[selection-polish] hotkey workflow failed: {error}");
                let message = match error.message.as_str() {
                    "selectionPolishNoSelection" | "selected text must not be empty" => {
                        "未选中内容"
                    }
                    "selectionPolishTargetUnavailable" => "目标输入框不可用，请重新选择",
                    "selectionPolishTargetChanged" | "selectionPolishSelectionChanged" => {
                        "选区已变化，未替换"
                    }
                    _ if error.code == openless_core::BackendErrorCode::Busy => {
                        "选区润色正在进行中"
                    }
                    _ => "润色失败，请重试",
                };
                let state = if matches!(
                    error.code,
                    openless_core::BackendErrorCode::Cancelled
                        | openless_core::BackendErrorCode::InvalidArgument
                ) {
                    CapsuleState::Cancelled
                } else {
                    CapsuleState::Error
                };
                let epoch = emit_selection_polish_capsule(&inner, state, message);
                schedule_selection_polish_capsule_idle(&inner, epoch, CAPSULE_AUTO_HIDE_DELAY_MS);
            }
        }
    });
}

#[cfg(not(mobile))]
fn handle_selection_workspace_hotkey_released(inner: &Arc<Inner>) {
    #[cfg(target_os = "windows")]
    {
        if !inner.backend.get_preferences().selection_voice_enabled {
            return;
        }
        let inner_cloned = Arc::clone(inner);
        inner.host.spawn(async move {
            super::selection_voice_session::handle_selection_voice_released(&inner_cloned).await;
        });
    }
    #[cfg(not(target_os = "windows"))]
    let _ = inner;
}

#[cfg(not(mobile))]
pub(super) fn take_selection_polish_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let main_inner = Arc::clone(inner);
    if inner
        .host
        .run_on_main_thread(move || {
            main_inner.selection_polish_hotkey.lock().take();
        })
        .is_err()
    {
        inner.selection_polish_hotkey.lock().take();
    }
}

// ─────────────────────────── combo hotkey supervisor ───────────────────────────

// ─────────────────────── coding agent hotkey supervisor ───────────────────────

pub(super) fn coding_agent_hotkey_supervisor_loop(inner: Arc<Inner>) {
    // The global-hotkey monitor is a Tauri desktop adapter; Linux egui uses its
    // fcitx5 listener and the same Core edge interpreter.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = update_coding_agent_hotkey_binding_now(&inner);
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        if let Err(error) = update_coding_agent_hotkey_binding_now(&inner) {
            log::warn!("[less-computer] hotkey registration failed: {error}");
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// 「Less Computer 语音键已禁用」是否已经打过日志。
///
/// supervisor 每 5s 轮询一次，没配语音键的用户每轮都会落到同一条禁用分支。无条件
/// 打印会稳定产出 720 行/小时的同一句话 —— 实测一份跑了六天的 openless.log 里它占
/// 了 95% 以上，真正有用的会话日志被冲得很难找，日志轮转也被它提前触发。
/// 只在状态翻转成禁用时打一次；重新装上语音键时清掉，下次禁用还会再打。
#[cfg(any(target_os = "macos", target_os = "windows"))]
static LESS_COMPUTER_HOTKEY_DISABLED_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) fn update_coding_agent_hotkey_binding_now(inner: &Arc<Inner>) -> Result<(), String> {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // This Tauri monitor is unavailable on the current target; the Linux egui
        // host owns its fcitx5 listener instead of duplicating this adapter.
        take_coding_agent_hotkeys_on_main_thread(inner);
        return Ok(());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        // 单修饰键的安装没有主线程等待，持目标锁使 supervisor 与重绑线性化。
        // 组合键必须在排队/等待主线程前释放它，回调再复核目标，不能带锁等 UI。
        let target_guard = inner.hotkey_runtime_target.lock();
        let target = target_guard.clone();
        let Some(binding) = target.coding_agent_voice.clone() else {
            take_coding_agent_hotkeys_on_main_thread(inner);
            if !LESS_COMPUTER_HOTKEY_DISABLED_LOGGED.swap(true, Ordering::SeqCst) {
                log::info!("[less-computer] hotkey disabled");
            }
            return Ok(());
        };
        if !target.coding_agent_enabled || is_unconfigured_shortcut(&binding) {
            take_coding_agent_hotkeys_on_main_thread(inner);
            return Ok(());
        }
        validate_less_computer_hotkey(&binding)?;
        LESS_COMPUTER_HOTKEY_DISABLED_LOGGED.store(false, Ordering::SeqCst);

        if let Some(modifier_binding) = less_computer_modifier_binding(&binding) {
            take_coding_agent_combo_hotkey_on_main_thread(inner);
            if let Some(monitor) = inner.coding_agent_modifier_hotkey.lock().as_ref() {
                monitor.update_binding(modifier_binding);
                return Ok(());
            }
            let (tx, rx) = mpsc::channel::<HotkeyEvent>();
            // Less Computer 的独立 tap 也转发 Esc 取消与组合键撤销（与主 monitor 双保险；
            // cancel_session 幂等，重复触发无害）。组合键撤销撤销的是 voice_agent 会话。
            let cancel_tx = spawn_esc_cancel_bridge(inner);
            let combo_tx = spawn_combo_abort_bridge(inner, cancel_less_computer_press);
            let monitor = HotkeyMonitor::start(modifier_binding, tx, cancel_tx, combo_tx)
                .map_err(|error| error.to_string())?;
            monitor.set_recording_active(inner.shortcut_recording_active.load(Ordering::SeqCst));
            let bridge_inner = Arc::clone(inner);
            std::thread::Builder::new()
                .name("openless-less-computer-modifier-bridge".into())
                .spawn(move || less_computer_modifier_bridge_loop(bridge_inner, rx))
                .map_err(|error| error.to_string())?;
            *inner.coding_agent_modifier_hotkey.lock() = Some(monitor);
            log::info!(
                "[less-computer] modifier hotkey installed ({})",
                binding.display_label()
            );
            return Ok(());
        }

        inner.coding_agent_modifier_hotkey.lock().take();
        drop(target_guard);
        let inner_clone = Arc::clone(inner);
        let binding_for_main = binding.clone();
        // 注册回执必须进入设置事务：系统已占用的组合键不能只记日志后保存成功。
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        inner.host.run_on_main_thread(move || {
            let result = (|| {
                let current_target = inner_clone.hotkey_runtime_target.lock();
                if *current_target != target {
                    // 包括接收超时后已回滚的情况：排队任务不能重新安装被撤销的键。
                    return Err("Less Computer hotkey target changed before registration".into());
                }
                if let Some(monitor) = inner_clone.coding_agent_combo_hotkey.lock().as_ref() {
                    return monitor
                        .update_binding(binding_for_main)
                        .map_err(|error| error.to_string());
                }
                let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
                let monitor = ComboHotkeyMonitor::start(binding_for_main, tx)
                    .map_err(|error| error.to_string())?;
                let bridge_inner = Arc::clone(&inner_clone);
                std::thread::Builder::new()
                    .name("openless-less-computer-combo-bridge".into())
                    .spawn(move || less_computer_combo_bridge_loop(bridge_inner, rx))
                    .map_err(|error| error.to_string())?;
                *inner_clone.coding_agent_combo_hotkey.lock() = Some(monitor);
                Ok(())
            })();
            let _ = result_tx.send(result);
        })?;
        result_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "Less Computer hotkey registration timed out".to_string())?
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn validate_less_computer_hotkey(binding: &crate::types::ShortcutBinding) -> Result<(), String> {
    crate::shortcut_binding::reject_side_specific_non_dictation(binding)?;
    crate::shortcut_binding::validate_binding(binding).map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    if crate::shortcut_binding::legacy_modifier_trigger(binding)
        == Some(crate::types::HotkeyTrigger::Fn)
    {
        // PC 键盘的 Fn 通常由固件消费，不能沿用主听写历史别名映射成右 Ctrl。
        return Err(
            "Windows 不支持 Fn 作为 Less Computer 快捷键，请使用 Ctrl、Alt、Shift、Win 或组合键"
                .into(),
        );
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn less_computer_modifier_binding(
    binding: &crate::types::ShortcutBinding,
) -> Option<crate::types::HotkeyBinding> {
    let trigger = crate::shortcut_binding::legacy_modifier_trigger(binding)?;
    Some(crate::types::HotkeyBinding {
        trigger,
        mode: crate::types::HotkeyMode::Hold,
        keys: None,
    })
}

pub(super) fn less_computer_modifier_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<HotkeyEvent>,
) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            HotkeyEvent::Pressed { press_id, at } => {
                inner.host.block_on(async {
                    handle_less_computer_modifier_pressed(&inner_cloned, press_id, at).await
                });
            }
            HotkeyEvent::Released { press_id, at } => {
                if inner.less_computer_press_generation.load(Ordering::SeqCst) != press_id {
                    continue;
                }
                let session_id = inner
                    .less_computer_voice
                    .lock()
                    .as_ref()
                    .map(LessComputerHostCapture::session_id);
                inner.host.block_on(async {
                    handle_less_computer_released(&inner_cloned, session_id, press_id, at).await
                });
            }
            // Esc 取消与组合键撤销都不在此枚举里：分别走 esc_cancel_bridge_loop /
            // combo_abort_bridge_loop（见各自函数注释）。
            HotkeyEvent::TranslationModifierPressed | HotkeyEvent::QaShortcutPressed => {}
            #[cfg(not(mobile))]
            HotkeyEvent::SelectionPolishShortcutPressed
            | HotkeyEvent::SelectionPolishShortcutReleased => {}
            #[cfg(not(mobile))]
            HotkeyEvent::FnRecordingPressed => {}
        }
    }
}

/// Less Computer 触发键被当修饰键用（Option+任意字母/数字键之类）：撤销这次按下开出的语音会话。
/// handle_less_computer_pressed 只在 Idle 时开会话，所以此刻还在跑的 voice_agent
/// 会话必然就是这次按下开出来的；其他情况（按下被忽略）什么都不动。
async fn handle_less_computer_modifier_pressed(
    inner: &Arc<Inner>,
    press_id: u64,
    at: std::time::Instant,
) {
    inner
        .less_computer_press_generation
        .store(press_id, Ordering::SeqCst);
    if take_pending_less_combined(inner, press_id).is_some() {
        return;
    }
    let session_id = handle_less_computer_pressed(inner, press_id, at).await;
    if let Some(edge) = take_pending_less_combined(inner, press_id) {
        cancel_less_computer_voice_session(inner, press_id, edge.at, session_id);
    }
}

fn take_pending_less_combined(
    inner: &Arc<Inner>,
    press_id: u64,
) -> Option<crate::hotkey::HotkeyCombinedEdge> {
    let mut pending = inner.less_computer_combo_pending_press.lock();
    if pending
        .as_ref()
        .is_some_and(|edge| edge.press_id == press_id)
    {
        pending.take()
    } else {
        None
    }
}

fn cancel_less_computer_press(inner: &Arc<Inner>, edge: crate::hotkey::HotkeyCombinedEdge) {
    let press_id = edge.press_id;
    if press_id == 0 {
        return;
    }
    *inner.less_computer_combo_pending_press.lock() = Some(edge);
    if inner.less_computer_press_generation.load(Ordering::SeqCst) != press_id {
        return;
    }
    let session_id = inner
        .less_computer_voice
        .lock()
        .as_ref()
        .map(LessComputerHostCapture::session_id);
    cancel_less_computer_voice_session(inner, press_id, edge.at, session_id);
}

/// Tauri half of the Core-owned Less Computer recording controller.
///
/// Core decides whether silence/fault means Stop or Cancel. This adapter only
/// reaches the opaque capture handle kept by the coordinator and performs that
/// exact effect. Requests can arrive while `start_less_computer_voice` is still
/// returning; they are queued by session id and flushed immediately after the
/// handle enters the slot, so a cold ASR startup cannot lose an early directive.
pub(super) enum LessComputerHostCapture {
    Starting(
        openless_core::SessionId,
        Arc<dyn openless_core::RecordingControlSink>,
    ),
    Recording(openless_core::LessComputerVoiceSession),
}

impl LessComputerHostCapture {
    fn session_id(&self) -> openless_core::SessionId {
        match self {
            Self::Starting(session_id, _) => *session_id,
            Self::Recording(session) => session.session_id(),
        }
    }

    async fn cancel(
        self,
        backend: &openless_core::OpenLessBackend,
    ) -> Result<(), openless_core::BackendError> {
        match self {
            // 冷启动还没有原生handle，Core session id依然是精确的取消归属。
            Self::Starting(session_id, _) => backend.cancel_less_computer(Some(session_id)).await,
            Self::Recording(session) => session.cancel().await,
        }
    }
}

fn take_less_computer_host_capture(
    slot: &Mutex<Option<LessComputerHostCapture>>,
    expected_session_id: Option<openless_core::SessionId>,
) -> Option<LessComputerHostCapture> {
    let mut slot = slot.lock();
    if expected_session_id.is_some_and(|expected| {
        slot.as_ref().map(LessComputerHostCapture::session_id) != Some(expected)
    }) {
        return None;
    }
    slot.take()
}

fn attach_less_computer_recording(
    slot: &Mutex<Option<LessComputerHostCapture>>,
    session: openless_core::LessComputerVoiceSession,
) -> Result<(), openless_core::LessComputerVoiceSession> {
    let mut slot = slot.lock();
    if !matches!(slot.as_ref(), Some(LessComputerHostCapture::Starting(id, _)) if *id == session.session_id())
    {
        return Err(session);
    }
    *slot = Some(LessComputerHostCapture::Recording(session));
    Ok(())
}

struct LessComputerRecordingControl {
    inner: std::sync::Weak<Inner>,
    pending: Mutex<
        Vec<(
            openless_core::SessionId,
            openless_core::RecordingControlAction,
        )>,
    >,
}

impl LessComputerRecordingControl {
    fn new(inner: &Arc<Inner>) -> Self {
        Self {
            inner: Arc::downgrade(inner),
            pending: Mutex::new(Vec::new()),
        }
    }

    fn apply(
        inner: &Arc<Inner>,
        session_id: openless_core::SessionId,
        action: openless_core::RecordingControlAction,
    ) {
        match action {
            openless_core::RecordingControlAction::Stop => {
                let task_inner = Arc::clone(inner);
                inner.host.spawn(async move {
                    let _ = finish_less_computer_voice_session(&task_inner, Some(session_id)).await;
                });
            }
            openless_core::RecordingControlAction::Cancel => {
                cancel_less_computer_capture(inner, Some(session_id));
            }
        }
    }

    fn flush(&self, session_id: openless_core::SessionId) {
        let requests = {
            let mut pending = self.pending.lock();
            let mut requests = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                if pending[index].0 == session_id {
                    requests.push(pending.remove(index));
                } else {
                    index += 1;
                }
            }
            requests
        };
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        for (_, action) in requests {
            Self::apply(&inner, session_id, action);
        }
    }
}

impl openless_core::RecordingControlSink for LessComputerRecordingControl {
    fn request(
        &self,
        session_id: openless_core::SessionId,
        action: openless_core::RecordingControlAction,
    ) -> Result<(), openless_core::BackendError> {
        let inner = self.inner.upgrade().ok_or_else(|| {
            openless_core::BackendError::new(
                openless_core::BackendErrorCode::Cancelled,
                "Less Computer host session is no longer available",
            )
        })?;
        // 与flush共用pending锁：ready=false与入队之间不允许attach后的flush穿过。
        // 唯一嵌套锁序为pending→slot；attach先释放slot再flush，取消不持pending等待。
        let mut pending = self.pending.lock();
        let ready = inner
            .less_computer_voice
            .lock()
            .as_ref()
            .is_some_and(|capture| matches!(capture,
                LessComputerHostCapture::Recording(session) if session.session_id() == session_id
            ));
        if ready {
            drop(pending);
            Self::apply(&inner, session_id, action);
        } else {
            pending.push((session_id, action));
        }
        Ok(())
    }
}

/// Take only the expected capture generation. A late Core directive must never
/// finish or cancel a newer Less Computer session that reused the same Host slot.
fn take_less_computer_capture(
    inner: &Arc<Inner>,
    expected_session_id: Option<openless_core::SessionId>,
) -> Option<openless_core::LessComputerVoiceSession> {
    let mut slot = inner.less_computer_voice.lock();
    if let Some(expected_session_id) = expected_session_id {
        if slot.as_ref().map(|session| session.session_id()) != Some(expected_session_id) {
            return None;
        }
    }
    if !matches!(slot.as_ref(), Some(LessComputerHostCapture::Recording(_))) {
        return None;
    }
    match slot.take() {
        Some(LessComputerHostCapture::Recording(session)) => Some(session),
        _ => unreachable!("recording capture checked under the same lock"),
    }
}

/// Cancel only the native capture/session handle. The product decision has
/// already been made either by Core or by the hotkey interpreter above.
pub(super) fn cancel_less_computer_capture(
    inner: &Arc<Inner>,
    expected_session_id: Option<openless_core::SessionId>,
) {
    let session = take_less_computer_host_capture(&inner.less_computer_voice, expected_session_id);
    if session.is_none() && expected_session_id.is_some() {
        // Stale Core directive: the current glow belongs to another session.
        return;
    }
    inner
        .less_computer_press_generation
        .store(0, Ordering::SeqCst);
    let spawner = inner.host.clone();
    let host = inner.host.clone();
    let backend = Arc::clone(&inner.backend);
    spawner.spawn(async move {
        if let Some(session) = session {
            if let Err(error) = session.cancel(&backend).await {
                log::warn!("[less-computer] cancel failed: {error}");
            }
        }
        if backend.less_computer_active_session().is_none() {
            host.hide_less_computer_glow();
        }
    });
}

/// Esc和关闭窗口都必须先同步转移Host资源所有权，再等待Core取消。
/// 没有capture时只取消此刻的Less Agent id，不能误取消QA/主听写或后来的会话。
pub(super) async fn cancel_active_less_computer(
    inner: &Arc<Inner>,
) -> Result<bool, openless_core::BackendError> {
    let capture = take_less_computer_host_capture(&inner.less_computer_voice, None);
    let session_id = capture
        .as_ref()
        .map(LessComputerHostCapture::session_id)
        .or_else(|| inner.backend.less_computer_active_session());
    let Some(session_id) = session_id else {
        return Ok(false);
    };
    inner
        .less_computer_press_generation
        .store(0, Ordering::SeqCst);
    inner.less_computer_combo_pending_press.lock().take();
    let result = match capture {
        Some(capture) => capture.cancel(&inner.backend).await,
        None => inner.backend.cancel_less_computer(Some(session_id)).await,
    };
    if inner.backend.less_computer_active_session().is_none() {
        inner.host.hide_less_computer_glow();
    }
    result.map(|()| true)
}

fn cancel_less_computer_voice_session(
    inner: &Arc<Inner>,
    press_id: u64,
    at: std::time::Instant,
    session_id: Option<openless_core::SessionId>,
) {
    // 保留待处理边沿，直到启动桥越过资源交接后再取走；启动前取消不能丢失。
    let Some(session_id) = session_id else { return };
    if inner.less_computer_press_generation.load(Ordering::SeqCst) != press_id {
        return;
    }
    let _ = inner.backend.dispatch_less_computer_hotkey_edge(
        openless_core::DictationHotkeyEdge::Combined { press_id, at },
    );
    log::info!("[less-computer] 触发键与其他键组合按下 —— 取消本次按下开出的会话");
    cancel_less_computer_capture(inner, Some(session_id));
}

pub(super) async fn finish_less_computer_voice_session(
    inner: &Arc<Inner>,
    expected_session_id: Option<openless_core::SessionId>,
) -> Result<bool, openless_core::BackendError> {
    let session = take_less_computer_capture(inner, expected_session_id);
    let Some(session) = session else {
        return Ok(false);
    };
    let result = session.finish().await;
    if let Err(error) = &result {
        log::warn!("[less-computer] finish failed: {error}");
    }
    if inner.backend.less_computer_active_session().is_none() {
        inner.host.hide_less_computer_glow();
    }
    result.map(|_| true)
}

pub(super) fn less_computer_combo_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<ComboHotkeyEvent>,
) {
    let mut owned_session = None;
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            ComboHotkeyEvent::Pressed { at } => {
                let press_id = crate::hotkey::next_press_id();
                owned_session = inner
                    .host
                    .block_on(async {
                        handle_less_computer_pressed(&inner_cloned, press_id, at).await
                    })
                    .map(|session_id| (session_id, press_id));
            }
            ComboHotkeyEvent::Released { at } => {
                let Some((session_id, press_id)) = owned_session.take() else {
                    continue;
                };
                inner.host.block_on(async {
                    handle_less_computer_released(&inner_cloned, Some(session_id), press_id, at)
                        .await
                });
            }
        }
    }
}

pub(super) async fn handle_less_computer_pressed(
    inner: &Arc<Inner>,
    press_id: u64,
    at: std::time::Instant,
) -> Option<openless_core::SessionId> {
    if !hotkey_runtime_target(inner).coding_agent_enabled {
        return None;
    }
    inner
        .less_computer_press_generation
        .store(press_id, Ordering::SeqCst);
    let action = inner.backend.dispatch_less_computer_hotkey_edge(
        openless_core::DictationHotkeyEdge::Pressed { press_id, at },
    );
    if matches!(action, openless_core::LessComputerHotkeyAction::Noop) {
        return None;
    }
    if !matches!(action, openless_core::LessComputerHotkeyAction::Start) {
        if matches!(action, openless_core::LessComputerHotkeyAction::Finish) {
            let _ = finish_less_computer_voice_session(inner, None).await;
        } else if matches!(action, openless_core::LessComputerHotkeyAction::Cancel) {
            let session_id = inner
                .less_computer_voice
                .lock()
                .as_ref()
                .map(LessComputerHostCapture::session_id);
            cancel_less_computer_voice_session(inner, press_id, at, session_id);
        }
        return None;
    }
    let session_id = openless_core::SessionId::new();
    let recording_control = Arc::new(LessComputerRecordingControl::new(inner));
    {
        let mut slot = inner.less_computer_voice.lock();
        if slot.is_some() {
            return None;
        }
        *slot = Some(LessComputerHostCapture::Starting(
            session_id,
            recording_control.clone(),
        ));
    }
    match inner
        .backend
        .start_less_computer_voice(
            session_id,
            Arc::clone(&recording_control) as Arc<dyn openless_core::RecordingControlSink>,
        )
        .await
    {
        Ok(session) => {
            if let Err(session) =
                attach_less_computer_recording(&inner.less_computer_voice, session)
            {
                // 重绑/禁用/Esc已取走Starting。迟到的原生handle只允许释放，
                // 不得重新挂回slot，更不能覆盖新的录音或继续提交给Agent。
                let _ = session.cancel().await;
                return None;
            }
            // Make the visible capture state observable before flushing an
            // early Stop/Cancel. Otherwise a queued effect could hide the glow
            // first and this function would immediately show it again.
            inner.host.show_less_computer_glow();
            recording_control.flush(session_id);
            log::info!("[less-computer] voice session started (session={session_id})");
            Some(session_id)
        }
        Err(error) => {
            let mut slot = inner.less_computer_voice.lock();
            if matches!(slot.as_ref(), Some(LessComputerHostCapture::Starting(id, _)) if *id == session_id)
            {
                slot.take();
            }
            log::warn!("[less-computer] voice session startup failed: {error}");
            None
        }
    }
}

pub(super) async fn handle_less_computer_released(
    inner: &Arc<Inner>,
    expected_session_id: Option<openless_core::SessionId>,
    press_id: u64,
    at: std::time::Instant,
) {
    let Some(session_id) = expected_session_id else {
        return;
    };
    if inner.less_computer_press_generation.load(Ordering::SeqCst) != press_id
        || inner
            .less_computer_voice
            .lock()
            .as_ref()
            .map(LessComputerHostCapture::session_id)
            != Some(session_id)
    {
        return;
    }
    let action = inner.backend.dispatch_less_computer_hotkey_edge(
        openless_core::DictationHotkeyEdge::Released { press_id, at },
    );
    if !matches!(action, openless_core::LessComputerHotkeyAction::Finish) {
        return;
    }
    let _ = finish_less_computer_voice_session(inner, Some(session_id)).await;
}

pub(super) fn take_coding_agent_hotkeys_on_main_thread(inner: &Arc<Inner>) {
    inner.coding_agent_modifier_hotkey.lock().take();
    take_coding_agent_combo_hotkey_on_main_thread(inner);
}

pub(super) fn take_coding_agent_combo_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let main_inner = Arc::clone(inner);
    if inner
        .host
        .run_on_main_thread(move || {
            main_inner.coding_agent_combo_hotkey.lock().take();
        })
        .is_err()
    {
        inner.coding_agent_combo_hotkey.lock().take();
    }
}

pub(super) fn combo_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let target = hotkey_runtime_target(&inner);
        if crate::shortcut_binding::legacy_modifier_trigger(&target.dictation).is_some() {
            take_combo_hotkey_on_main_thread(&inner);
            inner.side_aware_combo.lock().take();
            return;
        }

        let binding = target.dictation;
        if is_unconfigured_shortcut(&binding) {
            take_combo_hotkey_on_main_thread(&inner);
            inner.side_aware_combo.lock().take();
            return;
        }

        if crate::shortcut_binding::binding_requires_side_aware_hook(&binding) {
            take_combo_hotkey_on_main_thread(&inner);
            if inner.side_aware_combo.lock().is_some() {
                return;
            }
            let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
            match crate::side_aware_combo::SideAwareComboMonitor::start(binding, tx) {
                Ok(monitor) => {
                    *inner.side_aware_combo.lock() = Some(monitor);
                    let inner_clone = Arc::clone(&inner);
                    std::thread::Builder::new()
                        .name("openless-side-combo-bridge".into())
                        .spawn(move || combo_hotkey_bridge_loop(inner_clone, rx))
                        .ok();
                    return;
                }
                Err(e) => {
                    attempts += 1;
                    if attempts <= 3 || attempts % 10 == 0 {
                        log::warn!(
                            "[coord] side-aware combo 第 {attempts} 次注册失败: {e}; 3s 后重试"
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    continue;
                }
            }
        }

        inner.side_aware_combo.lock().take();

        if inner.combo_hotkey.lock().is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let (init_tx, init_rx) =
            mpsc::sync_channel::<Result<ComboHotkeyMonitor, ComboHotkeyError>>(1);
        let binding_for_main = binding.clone();
        if inner
            .host
            .run_on_main_thread(move || {
                let result = ComboHotkeyMonitor::start(binding_for_main, tx);
                let _ = init_tx.send(result);
            })
            .is_err()
        {
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        let init_result = match init_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] combo hotkey 第 {attempts} 次注册超时（主线程未回执）；3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
        };

        match init_result {
            Ok(monitor) => {
                *inner.combo_hotkey.lock() = Some(monitor);
                log::info!(
                    "[coord] combo hotkey listener installed on main thread (after {} attempt(s))",
                    attempts + 1
                );
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name("openless-combo-hotkey-bridge".into())
                    .spawn(move || combo_hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                #[cfg(target_os = "linux")]
                sync_custom_dictation_to_plugin(&inner);
                return;
            }
            Err(e) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!("[coord] combo hotkey 第 {attempts} 次注册失败: {e}; 3s 后重试");
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

pub(super) fn combo_hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<ComboHotkeyEvent>) {
    let mut current_press_id = 0;
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            // P0 #468/#475: 同 hotkey_bridge_loop —— Pressed/Released 必须串行 await，
            // 否则 latch 竞态导致 combo 快捷键二次按键失效。
            ComboHotkeyEvent::Pressed { at } => {
                current_press_id = crate::hotkey::next_press_id();
                inner.host.block_on(async {
                    handle_pressed_edge(&inner_cloned, at, current_press_id).await;
                });
            }
            ComboHotkeyEvent::Released { at } => {
                let press_id = std::mem::take(&mut current_press_id);
                inner.host.block_on(async {
                    handle_released_edge(&inner_cloned, at, press_id).await;
                });
            }
        }
    }
}

pub(super) fn translation_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        let binding = hotkey_runtime_target(&inner).translation;
        if is_builtin_translation_shift(&binding)
            || crate::shortcut_binding::legacy_modifier_trigger(&binding).is_some()
        {
            take_translation_hotkey_on_main_thread(&inner);
            if let Some(monitor) = inner.hotkey.lock().as_ref() {
                let (qa_trigger, selection_polish_trigger, translation_trigger) =
                    modifier_shortcut_triggers(&inner);
                monitor.update_modifier_shortcuts(
                    qa_trigger,
                    selection_polish_trigger,
                    translation_trigger,
                );
            }
            // 对齐主 supervisor 的 exit-on-success：装/卸交给 try_update_translation_hotkey_binding 主动路径，issue #470
            return;
        }

        if inner.translation_hotkey.lock().is_some() {
            // 对齐主 supervisor 的 exit-on-success：装/卸交给 try_update_translation_hotkey_binding 主动路径，issue #470
            return;
        }

        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let (init_tx, init_rx) =
            mpsc::sync_channel::<Result<ComboHotkeyMonitor, ComboHotkeyError>>(1);
        let binding_for_main = binding.clone();
        if inner
            .host
            .run_on_main_thread(move || {
                let result = ComboHotkeyMonitor::start(binding_for_main, tx);
                let _ = init_tx.send(result);
            })
            .is_err()
        {
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        let init_result = match init_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
        };

        match init_result {
            Ok(monitor) => {
                *inner.translation_hotkey.lock() = Some(monitor);
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name("openless-translation-hotkey-bridge".into())
                    .spawn(move || translation_hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                attempts = 0;
            }
            Err(e) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] translation hotkey 第 {attempts} 次注册失败: {e}; 3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

pub(super) fn update_translation_hotkey_on_main_thread(
    inner: Arc<Inner>,
    binding: crate::types::ShortcutBinding,
) -> Result<(), ComboHotkeyError> {
    if let Some(monitor) = inner.translation_hotkey.lock().as_ref() {
        return monitor.update_binding(binding);
    }
    let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
    let monitor = ComboHotkeyMonitor::start(binding, tx)?;
    *inner.translation_hotkey.lock() = Some(monitor);
    let bridge_inner = Arc::clone(&inner);
    std::thread::Builder::new()
        .name("openless-translation-hotkey-bridge".into())
        .spawn(move || translation_hotkey_bridge_loop(bridge_inner, rx))
        .map_err(|e| ComboHotkeyError::RegisterFailed(format!("spawn bridge thread: {e}")))?;
    Ok(())
}

pub(super) fn translation_hotkey_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<ComboHotkeyEvent>,
) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        if matches!(evt, ComboHotkeyEvent::Pressed { .. }) {
            inner.host.block_on(async {
                arm_translation_if_effective(&inner).await;
            });
        }
    }
}

pub(super) fn action_hotkey_supervisor_loop(inner: Arc<Inner>, kind: ActionHotkeyKind) {
    let mut attempts: u32 = 0;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        // None = 用户主动停用：反注册后退出守护（由 update_action_hotkey_binding 主动路径重装）。
        let Some(binding) = action_hotkey_binding(&inner, kind) else {
            take_action_hotkey_on_main_thread(&inner, kind);
            // 对齐主 supervisor 的 exit-on-success：装/卸交给 update_action_hotkey_binding 主动路径，issue #470
            return;
        };
        if is_modifier_only_shortcut(&binding) {
            take_action_hotkey_on_main_thread(&inner, kind);
            // 对齐主 supervisor 的 exit-on-success：装/卸交给 update_action_hotkey_binding 主动路径，issue #470
            return;
        }

        if action_hotkey_slot(&inner, kind).lock().is_some() {
            // 对齐主 supervisor 的 exit-on-success：装/卸交给 update_action_hotkey_binding 主动路径，issue #470
            return;
        }

        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let (init_tx, init_rx) =
            mpsc::sync_channel::<Result<ComboHotkeyMonitor, ComboHotkeyError>>(1);
        let binding_for_main = binding.clone();
        if inner
            .host
            .run_on_main_thread(move || {
                let result = ComboHotkeyMonitor::start(binding_for_main, tx);
                let _ = init_tx.send(result);
            })
            .is_err()
        {
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        let init_result = match init_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(r) => r,
            Err(_) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] action hotkey {kind:?} 第 {attempts} 次注册超时；3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
        };

        match init_result {
            Ok(monitor) => {
                *action_hotkey_slot(&inner, kind).lock() = Some(monitor);
                log::info!(
                    "[coord] action hotkey {kind:?} listener installed after {} attempt(s)",
                    attempts + 1
                );
                let inner_clone = Arc::clone(&inner);
                std::thread::Builder::new()
                    .name(action_hotkey_bridge_thread_name(kind).into())
                    .spawn(move || action_hotkey_bridge_loop(inner_clone, rx, kind))
                    .ok();
                attempts = 0;
            }
            Err(e) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] action hotkey {kind:?} 第 {attempts} 次注册失败: {e}; 3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

pub(super) fn action_hotkey_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<ComboHotkeyEvent>,
    kind: ActionHotkeyKind,
) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        if matches!(evt, ComboHotkeyEvent::Pressed { .. }) {
            handle_action_hotkey_pressed(&inner, kind);
        }
    }
}

pub(super) fn handle_action_hotkey_pressed(inner: &Arc<Inner>, kind: ActionHotkeyKind) {
    match kind {
        ActionHotkeyKind::SwitchStyle => switch_to_previous_style(inner),
        ActionHotkeyKind::OpenApp => inner.host.show_main_window(),
    }
}

/// 全局快捷键切风格后的轻量提示：用户多半在别的前台 app 里按键，不弹提示
/// 无法知道切没切成功、切到了哪个风格。复用选区润色的无焦点一行提示胶囊
/// （✓ + 文案，2s 自动隐藏，不抢焦点不挡点击）；录音中按键最多闪一帧，
/// 下一个 ~30Hz 电平帧会立即夺回胶囊显示，auto-hide timer 也会因代数失效。
#[cfg(not(mobile))]
pub(super) fn show_style_switch_capsule(inner: &Arc<Inner>, name: &str) {
    let event_epoch =
        emit_selection_polish_capsule(inner, CapsuleState::Done, format!("已切换：{name}"));
    schedule_selection_polish_capsule_idle(inner, event_epoch, CAPSULE_AUTO_HIDE_DELAY_MS);
}

pub(super) fn switch_to_previous_style(inner: &Arc<Inner>) {
    let selected = match inner.backend.activate_previous_style_pack() {
        Ok(selected) => selected,
        Err(error) => {
            log::warn!("[coord] switch style hotkey failed: {error}");
            return;
        }
    };
    let Some(selected) = selected else {
        log::info!("[coord] switch style hotkey ignored: enabled style count <= 1");
        return;
    };
    log::info!(
        "[coord] switch style hotkey changed active style pack to {}",
        selected.id
    );
    #[cfg(not(mobile))]
    show_style_switch_capsule(inner, &selected.name);
    inner.host.refresh_tray_microphone_menu();
}

pub(super) fn take_combo_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let main_inner = Arc::clone(inner);
    if inner
        .host
        .run_on_main_thread(move || {
            main_inner.combo_hotkey.lock().take();
        })
        .is_err()
    {
        inner.combo_hotkey.lock().take();
    }
}

pub(super) fn take_translation_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let main_inner = Arc::clone(inner);
    if inner
        .host
        .run_on_main_thread(move || {
            main_inner.translation_hotkey.lock().take();
        })
        .is_err()
    {
        inner.translation_hotkey.lock().take();
    }
}

pub(super) fn take_action_hotkey_on_main_thread(inner: &Arc<Inner>, kind: ActionHotkeyKind) {
    let main_inner = Arc::clone(inner);
    if inner
        .host
        .run_on_main_thread(move || {
            action_hotkey_slot(&main_inner, kind).lock().take();
        })
        .is_err()
    {
        action_hotkey_slot(inner, kind).lock().take();
    }
}

pub(super) fn action_hotkey_slot(
    inner: &Arc<Inner>,
    kind: ActionHotkeyKind,
) -> &Mutex<Option<ComboHotkeyMonitor>> {
    match kind {
        ActionHotkeyKind::SwitchStyle => &inner.switch_style_hotkey,
        ActionHotkeyKind::OpenApp => &inner.open_app_hotkey,
    }
}

pub(super) fn action_hotkey_binding(
    inner: &Arc<Inner>,
    kind: ActionHotkeyKind,
) -> Option<crate::types::ShortcutBinding> {
    let target = hotkey_runtime_target(inner);
    match kind {
        ActionHotkeyKind::SwitchStyle => target.switch_style,
        ActionHotkeyKind::OpenApp => target.open_app,
    }
}

pub(super) fn hotkey_runtime_target(inner: &Arc<Inner>) -> openless_core::HotkeyRuntimeTarget {
    inner.hotkey_runtime_target.lock().clone()
}

pub(super) fn is_modifier_only_shortcut(binding: &crate::types::ShortcutBinding) -> bool {
    binding.modifiers.is_empty()
        && (binding.primary.eq_ignore_ascii_case("shift")
            || crate::shortcut_binding::legacy_modifier_trigger(binding).is_some())
}

pub(super) fn is_unconfigured_shortcut(binding: &crate::types::ShortcutBinding) -> bool {
    binding.primary.trim().is_empty()
}

pub(super) fn action_hotkey_bridge_thread_name(kind: ActionHotkeyKind) -> &'static str {
    match kind {
        ActionHotkeyKind::SwitchStyle => "openless-switch-style-hotkey-bridge",
        ActionHotkeyKind::OpenApp => "openless-open-app-hotkey-bridge",
    }
}

// ─────────────────── style pack hotkeys (issue #759) ───────────────────

fn replace_style_pack_hotkey_registrations<R>(
    entries: &[crate::types::StylePackHotkey],
    registrations: &mut std::collections::HashMap<String, R>,
    mut register: impl FnMut(&crate::types::StylePackHotkey) -> Result<R, String>,
) -> Result<(), String> {
    registrations.clear();
    for entry in entries {
        match register(entry) {
            Ok(registration) => {
                registrations.insert(entry.pack_id.clone(), registration);
            }
            Err(error) => {
                registrations.clear();
                return Err(error);
            }
        }
    }
    Ok(())
}

fn style_pack_hotkey_registrations_match<R>(
    desired: &[crate::types::StylePackHotkey],
    registrations: &std::collections::HashMap<String, R>,
    binding_of: impl for<'a> Fn(&'a R) -> &'a crate::types::ShortcutBinding,
) -> bool {
    desired.len() == registrations.len()
        && desired.iter().all(|entry| {
            registrations
                .get(&entry.pack_id)
                .is_some_and(|registration| binding_of(registration) == &entry.binding)
        })
}

fn configured_style_pack_hotkeys(inner: &Arc<Inner>) -> Vec<crate::types::StylePackHotkey> {
    hotkey_runtime_target(inner)
        .style_packs
        .into_iter()
        .filter(|entry| {
            !is_unconfigured_shortcut(&entry.binding) && !is_modifier_only_shortcut(&entry.binding)
        })
        .collect()
}

/// 常驻 supervisor：持续比较 prefs 与实际注册表。状态一致时低频复查；配置变化、
/// 主动同步失败或只注册了部分条目时按 3s 节奏重试，直到收敛或 shutdown。
pub(super) fn style_pack_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }

        let desired = configured_style_pack_hotkeys(&inner);
        let registrations_match = {
            let registrations = inner.style_pack_hotkeys.lock();
            style_pack_hotkey_registrations_match(&desired, &registrations, |registration| {
                &registration.binding
            })
        };
        if registrations_match {
            attempts = 0;
            std::thread::sleep(std::time::Duration::from_secs(5));
            continue;
        }

        match try_sync_style_pack_hotkeys_on_main_thread(&inner) {
            Ok(()) => {
                log::info!(
                    "[coord] style pack hotkey listeners synchronized after {} attempt(s)",
                    attempts + 1
                );
                attempts = 0;
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err(error) => {
                attempts += 1;
                if attempts <= 3 || attempts % 10 == 0 {
                    log::warn!(
                        "[coord] style pack hotkeys 第 {attempts} 次同步失败: {error}; 3s 后重试"
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
}

/// 按 runtime target 全量对齐风格包快捷键注册状态。**必须在主线程执行**（macOS Carbon
/// 要求 manager 在主线程构造）。策略为整表重建：先 drop 全部旧注册再逐条注册，
/// 避免「两个包互换按键」时新键仍被旧注册占用。任意条目失败会清空本轮全部注册。
pub(super) fn sync_style_pack_hotkeys(inner: &Arc<Inner>) -> Result<(), String> {
    let entries = configured_style_pack_hotkeys(inner);
    let mut registrations = inner.style_pack_hotkeys.lock();
    replace_style_pack_hotkey_registrations(&entries, &mut registrations, |entry| {
        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let monitor = ComboHotkeyMonitor::start(entry.binding.clone(), tx).map_err(|error| {
            format!(
                "style pack hotkey {} registration failed: {error}",
                entry.pack_id
            )
        })?;
        let bridge_inner = Arc::clone(inner);
        let pack_id = entry.pack_id.clone();
        std::thread::Builder::new()
            .name("openless-style-pack-hotkey-bridge".into())
            .spawn(move || style_pack_hotkey_bridge_loop(bridge_inner, rx, pack_id))
            .map_err(|error| {
                format!(
                    "style pack hotkey {} bridge thread failed: {error}",
                    entry.pack_id
                )
            })?;
        Ok(StylePackHotkeyRegistration {
            binding: entry.binding.clone(),
            _monitor: monitor,
        })
    })
}

/// 事务式设置路径：派发到主线程并等待最多 5s，确保调用方能回滚偏好并展示错误。
pub(super) fn try_sync_style_pack_hotkeys_on_main_thread(inner: &Arc<Inner>) -> Result<(), String> {
    let (result_tx, result_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    let sync_inner = Arc::clone(inner);
    inner.host.run_on_main_thread(move || {
        let _ = result_tx.send(sync_style_pack_hotkeys(&sync_inner));
    })?;
    result_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "注册风格包快捷键超时".to_string())?
}

/// 设置导入、删除风格包等不可整体回滚路径使用的主动同步。失败只记录日志，
/// 常驻 supervisor 会根据 prefs 与实际注册表差异继续重试。
pub(super) fn sync_style_pack_hotkeys_on_main_thread(inner: &Arc<Inner>) {
    let sync_inner = Arc::clone(inner);
    if let Err(error) = inner.host.run_on_main_thread(move || {
        if let Err(error) = sync_style_pack_hotkeys(&sync_inner) {
            log::warn!("[coord] style pack hotkeys 主动同步失败: {error}");
        }
    }) {
        log::warn!("[coord] dispatch style pack hotkeys sync failed: {error}");
    }
}

pub(super) fn clear_style_pack_hotkeys_on_main_thread(inner: &Arc<Inner>) {
    let main_inner = Arc::clone(inner);
    if inner
        .host
        .run_on_main_thread(move || {
            main_inner.style_pack_hotkeys.lock().clear();
        })
        .is_err()
    {
        inner.style_pack_hotkeys.lock().clear();
    }
}

pub(super) fn style_pack_hotkey_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<ComboHotkeyEvent>,
    pack_id: String,
) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        if matches!(evt, ComboHotkeyEvent::Pressed { .. }) {
            handle_style_pack_hotkey_pressed(&inner, &pack_id);
        }
    }
}

/// 复用 `activate_style_pack_by_id`（禁用包自动启用、写 prefs、sync、广播、刷托盘），
/// 与前端「点选风格包」走完全相同的激活路径；包已被删除时仅 warn 不做事。
pub(super) fn handle_style_pack_hotkey_pressed(inner: &Arc<Inner>, pack_id: &str) {
    let coord = Coordinator {
        inner: Arc::clone(inner),
    };
    match inner.host.activate_style_pack_by_id(&coord, pack_id) {
        Ok(pack) => {
            log::info!(
                "[coord] style pack hotkey activated {} ({})",
                pack.id,
                pack.name
            );
            #[cfg(not(mobile))]
            show_style_switch_capsule(inner, &pack.name);
        }
        Err(error) => {
            log::warn!("[coord] style pack hotkey {pack_id} activation failed: {error}")
        }
    }
}

pub(super) fn is_builtin_translation_shift(binding: &crate::types::ShortcutBinding) -> bool {
    binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift")
}

/// Linux: 从 runtime target 读取自定义组合键，同步到 fcitx5 插件。
#[cfg(target_os = "linux")]
pub(super) fn custom_dictation_key_string(inner: &Arc<Inner>) -> Option<String> {
    let target = hotkey_runtime_target(inner);
    let key_string = crate::linux_fcitx::binding_to_fcitx_key_string(&target.dictation);
    if key_string.is_empty() {
        None
    } else {
        Some(key_string)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn sync_custom_dictation_to_plugin(inner: &Arc<Inner>) {
    let target = hotkey_runtime_target(inner);
    let dictation = &target.dictation;
    let key_string = crate::linux_fcitx::binding_to_fcitx_key_string(dictation);
    if key_string.is_empty() {
        return;
    }
    match crate::linux_fcitx::set_custom_dictation_trigger(&key_string) {
        Ok(()) => log::info!(
            "[fcitx] Synced custom dictation trigger '{}' to plugin",
            key_string
        ),
        Err(e) => log::warn!("[fcitx] Failed to sync custom dictation trigger: {e}"),
    }
}

pub(super) fn modifier_shortcut_triggers(
    inner: &Arc<Inner>,
) -> (
    Option<crate::types::HotkeyTrigger>,
    Option<crate::types::HotkeyTrigger>,
    Option<crate::types::HotkeyTrigger>,
) {
    let target = hotkey_runtime_target(inner);
    let qa_trigger = target
        .qa
        .as_ref()
        .and_then(crate::shortcut_binding::legacy_modifier_trigger);
    let translation_trigger = if is_builtin_translation_shift(&target.translation) {
        None
    } else {
        crate::shortcut_binding::legacy_modifier_trigger(&target.translation)
    };
    let selection_polish_trigger = target
        .selection_polish
        .as_ref()
        .and_then(crate::shortcut_binding::legacy_modifier_trigger);
    (qa_trigger, selection_polish_trigger, translation_trigger)
}

/// 在这里、而不是在读取侧判定「翻译是否真的会发生」：本函数在桥接线程（翻译热键事件 /
/// 主热键循环）和安卓 overlay 命令路径上调用，均非音频回调线程，读一次 prefs 无妨；
/// 而 `translation_active` 的读取侧之一是 emit_capsule —— 它在音频回调线程按帧执行，
/// 不能碰偏好锁（见 capsule_focus.rs 注释）。
///
/// 收紧后这个 flag 的语义从「按过 Shift」变成「本次会话真的要翻译」，胶囊提示与 polish
/// 分派读同一个值，不会再出现「胶囊说正在翻译、后端其实没翻」的漂移（用户未设目标语言
/// 时按 Shift 就会撞上）。返回 true 表示本次会话翻译已置位。
pub(super) async fn arm_translation_if_effective(inner: &Arc<Inner>) -> bool {
    let phase = inner.backend.snapshot().dictation.phase;
    if !matches!(
        phase,
        openless_core::DictationPhase::Starting | openless_core::DictationPhase::Recording
    ) {
        return false;
    }
    // 目标语言和有效性由 Core 本轮冻结的上下文解释。Host 只转交真实按键，
    // 不保存跨会话标志，也不读取可能已在录音期间被修改的设置重做业务判定。
    if let Err(error) = inner
        .backend
        .update_dictation_translation_requested(true)
        .await
    {
        log::warn!("[coord] failed to update active core translation state: {error}");
        return false;
    }
    let effective = inner.backend.snapshot().dictation.translation_active;
    log::info!("[coord] translation requested during {phase:?}, effective={effective}");
    effective
}

pub(super) fn hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<HotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            // 录制态：仅上报「录制 Fn」事件给前端（recorder 在录入态检测到 Fn 按下，
            // 浏览器不向网页层下发 Fn keydown，由 CGEventTap 上报），其余热键事件
            // 一律跳过，避免录制期间误触发听写。
            #[cfg(not(mobile))]
            if matches!(evt, HotkeyEvent::FnRecordingPressed) {
                emit_fn_recording_pressed(&inner);
            }
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            // P0 #468/#475: Pressed/Released 必须串行处理，否则在 Windows 上 WH_KEYBOARD_LL
            // 边沿间隔微秒级 → 两个独立 spawn 的 task 被 work-stealing 调度器并行执行 →
            // 同一物理按键的边沿顺序错乱 → 下次按键被静默吞掉
            // (UI 关不掉 / 录音停不下来)。改为 bridge 线程内 block_on 顺序 await，
            // recv 的 FIFO 顺序就是 handler 执行顺序。
            // 注意：handle_pressed_edge / handle_released_edge 内部走 .await（含网络
            // 握手），会暂时阻塞本 bridge 线程；Hold 模式短按时 Released 会排队在 channel
            // 里直到 begin_session 完成，但 SessionPhase::Starting 已经有
            // request_stop_during_starting 兜底，begin_session 完成进 Listening 后
            // bridge 立刻 recv Released → end_session，行为正确，仅有短暂 stop 延迟。
            HotkeyEvent::Pressed { at, press_id } => {
                inner.host.block_on(async {
                    handle_pressed_edge(&inner_cloned, at, press_id).await;
                });
            }
            HotkeyEvent::Released { at, press_id } => {
                inner.host.block_on(async {
                    handle_released_edge(&inner_cloned, at, press_id).await;
                });
            }
            // Esc 取消与组合键撤销都不在此枚举里：分别走 esc_cancel_bridge_loop /
            // combo_abort_bridge_loop，避免被上面 Released → end_session /
            // Pressed → begin_session 的同步流程堵在队列里（见各自函数注释）。
            HotkeyEvent::TranslationModifierPressed => {
                let translation_hotkey = hotkey_runtime_target(&inner_cloned).translation;
                if is_builtin_translation_shift(&translation_hotkey)
                    || crate::shortcut_binding::legacy_modifier_trigger(&translation_hotkey)
                        .is_some()
                {
                    inner.host.block_on(async {
                        arm_translation_if_effective(&inner_cloned).await;
                    });
                }
            }
            HotkeyEvent::QaShortcutPressed => {
                inner.host.block_on(async {
                    handle_qa_hotkey_pressed(&inner_cloned).await;
                });
            }
            #[cfg(not(mobile))]
            HotkeyEvent::SelectionPolishShortcutPressed => {
                handle_selection_workspace_hotkey_pressed(&inner_cloned);
            }
            #[cfg(not(mobile))]
            HotkeyEvent::SelectionPolishShortcutReleased => {
                handle_selection_workspace_hotkey_released(&inner_cloned);
            }
            // 非录制态不会出现（CGEventTap 仅在 recording_active 时上报）；防御性忽略。
            #[cfg(not(mobile))]
            HotkeyEvent::FnRecordingPressed => {}
        }
    }
}

/// 录制态检测到 Fn 按下 → 发事件给前端，让 ShortcutRecorder 提交 Fn 绑定。
#[cfg(not(mobile))]
fn emit_fn_recording_pressed(inner: &Arc<Inner>) {
    log::info!(
        "[hotkey] 录制 Fn 按下 → emit fn-shortcut-pressed (app_ready={})",
        inner.host.is_bound()
    );
    if inner.host.is_bound() {
        inner.host.emit_fn_shortcut_pressed();
    }
}

pub(super) fn reset_shortcut_held_state(inner: &Arc<Inner>) {
    if let Some(monitor) = inner.hotkey.lock().as_ref() {
        monitor.reset_held_state();
    }
    let target = hotkey_runtime_target(inner);
    if let Some(binding) = target.qa.as_ref() {
        if crate::shortcut_binding::legacy_modifier_trigger(binding).is_none() {
            if let Some(monitor) = inner.qa_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(binding.clone()) {
                    log::warn!("[coord] reset QA hotkey latch failed: {e}");
                }
            }
        }
    }
    if !is_builtin_translation_shift(&target.translation)
        && crate::shortcut_binding::legacy_modifier_trigger(&target.translation).is_none()
    {
        if let Some(monitor) = inner.translation_hotkey.lock().as_ref() {
            if let Err(e) = monitor.update_binding(target.translation.clone()) {
                log::warn!("[coord] reset translation hotkey latch failed: {e}");
            }
        }
    }
    if let Some(switch_style) = target.switch_style.as_ref() {
        if !is_modifier_only_shortcut(switch_style) {
            if let Some(monitor) = inner.switch_style_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(switch_style.clone()) {
                    log::warn!("[coord] reset switch-style hotkey latch failed: {e}");
                }
            }
        }
    }
    if let Some(open_app) = target.open_app.as_ref() {
        if !is_modifier_only_shortcut(open_app) {
            if let Some(monitor) = inner.open_app_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(open_app.clone()) {
                    log::warn!("[coord] reset open-app hotkey latch failed: {e}");
                }
            }
        }
    }
}

pub(super) async fn handle_window_hotkey_event(
    inner: &Arc<Inner>,
    event_type: String,
    key: String,
    code: String,
    repeat: bool,
) -> Result<(), String> {
    if inner.shortcut_recording_active.load(Ordering::SeqCst) {
        return Ok(());
    }
    if event_type == "keydown" && key == "Escape" {
        // Esc 路由（issue #161）：QA 浮窗可见时优先取消 QA（不动 dictation）；
        // 否则走 dictation 取消通路。之前无条件 cancel_session 导致 QA 浮窗
        // 按 Esc 杀的是 dictation 而 QA 流还在烧 token。
        let panel_visible = inner.qa_context.is_panel_visible();
        let qa_active = inner
            .backend
            .services()
            .qa
            .snapshot()
            .await
            .map(|snapshot| snapshot.phase != openless_core::QaPhase::Idle)
            .unwrap_or(false);
        if panel_visible || qa_active {
            if let Err(error) = inner.backend.services().qa.dismiss().await {
                log::warn!("[coord] QA dismiss from Escape failed: {error}");
            }
        } else {
            super::dictation::cancel_active_session(inner).await;
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (inner, event_type, key, code, repeat);
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        if !window_hotkey_fallback_enabled() {
            if event_type == "keydown" && !repeat {
                log::info!(
                    "[window-hotkey] ignored because Windows lifecycle owner is the low-level hook"
                );
            }
            return Ok(());
        }

        let Some(trigger) = crate::shortcut_binding::legacy_modifier_trigger(
            &hotkey_runtime_target(inner).dictation,
        ) else {
            return Ok(());
        };
        if !window_key_matches_trigger(trigger, &key, &code) {
            return Ok(());
        }

        match event_type.as_str() {
            "keydown" => {
                if repeat {
                    return Ok(());
                }
                log::info!(
                    "[window-hotkey] pressed trigger={trigger:?} code={code} repeat={repeat}"
                );
                let press_id = crate::hotkey::next_press_id();
                inner
                    .window_hotkey_press_id
                    .store(press_id, Ordering::SeqCst);
                handle_pressed_edge(inner, std::time::Instant::now(), press_id).await;
            }
            "keyup" => {
                log::info!("[window-hotkey] released trigger={trigger:?} code={code}");
                let press_id = inner.window_hotkey_press_id.swap(0, Ordering::SeqCst);
                handle_released_edge(inner, std::time::Instant::now(), press_id).await;
            }
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn window_hotkey_fallback_enabled() -> bool {
    crate::types::HotkeyCapability::current().explicit_fallback_available
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn window_key_matches_trigger(
    trigger: crate::types::HotkeyTrigger,
    key: &str,
    code: &str,
) -> bool {
    use crate::types::HotkeyTrigger;

    match trigger {
        HotkeyTrigger::RightControl => key == "Control" && code == "ControlRight",
        HotkeyTrigger::LeftControl => key == "Control" && code == "ControlLeft",
        HotkeyTrigger::RightOption | HotkeyTrigger::RightAlt => {
            (key == "Alt" || key == "AltGraph") && code == "AltRight"
        }
        HotkeyTrigger::LeftOption => (key == "Alt" || key == "AltGraph") && code == "AltLeft",
        HotkeyTrigger::RightCommand => key == "Meta" && code == "MetaRight",
        HotkeyTrigger::LeftCommand => key == "Meta" && code == "MetaLeft",
        HotkeyTrigger::LeftShift => key == "Shift" && code == "ShiftLeft",
        HotkeyTrigger::RightShift => key == "Shift" && code == "ShiftRight",
        HotkeyTrigger::Fn => key == "Control" && code == "ControlRight",
        // MediaPlayPause 走 WH_KEYBOARD_LL，不走 window hotkey fallback
        HotkeyTrigger::MediaPlayPause => false,
        // Custom 走 global-hotkey crate，不走 window hotkey fallback
        HotkeyTrigger::Custom => false,
    }
}

#[cfg(all(test, target_os = "windows"))]
pub(crate) mod windows_less_computer_tests {
    use super::*;

    // 只替换设备/云边界；边沿、取消、Host slot和Core lease都使用生产实现。
    pub(crate) fn fixture_coordinator(
        mode: crate::types::HotkeyMode,
        startup_delay: std::time::Duration,
    ) -> (
        Coordinator,
        Arc<openless_core::testing::FixtureAudioRecorder>,
        std::path::PathBuf,
    ) {
        use openless_core::testing::{
            FixtureAudioRecorder, FixtureTextPolisher, FixtureTranscriptionEngine,
        };
        let data_dir =
            std::env::temp_dir().join(format!("openless-less-host-{}", uuid::Uuid::new_v4()));
        let recorder = Arc::new(FixtureAudioRecorder::default());
        let backend = Arc::new(
            openless_core::OpenLessBackend::new(
                openless_core::BackendConfig {
                    data_dir: data_dir.clone(),
                    ..Default::default()
                },
                openless_core::BackendDependencies {
                    host_actions: Arc::new(openless_core::ports::NoopHostActions),
                    text_inserter: Arc::new(
                        openless_core::testing::FixtureTextInserter::with_outcome(
                            openless_core::InsertOutcome::Inserted,
                        ),
                    ),
                    dictation_engine: Arc::new(openless_core::PipelineDictationEngine::new(
                        Arc::new(DelayedFixtureRecorder {
                            recorder: recorder.clone(),
                            startup_delay,
                        }),
                        Arc::new(FixtureTranscriptionEngine::successful("voice", 120)),
                        Arc::new(FixtureTextPolisher::successful("unused")),
                    )),
                    ..openless_core::BackendDependencies::unsupported()
                },
            )
            .unwrap(),
        );
        let mut prefs = backend.get_preferences();
        prefs.coding_agent_enabled = true;
        prefs.hotkey.mode = mode;
        crate::set_backend_preferences_for_test(&backend, prefs);
        let inner = Arc::new(Inner {
            host: crate::tauri_coordinator_host::TauriCoordinatorHost::new(
                crate::core_adapters::app_handle_slot(),
            ),
            hotkey_runtime_target: Mutex::new((&backend.get_preferences()).into()),
            backend,
            less_computer_voice: Mutex::new(None),
            settings_host_gate: Mutex::new(()),
            inserter: TextInserter::new(),
            vocab_card_visible: AtomicBool::new(false),
            hotkey: Mutex::new(None),
            hotkey_status: Arc::new(Mutex::new(HotkeyStatus::default())),
            window_hotkey_press_id: AtomicU64::new(0),
            shortcut_recording_active: AtomicBool::new(false),
            less_computer_press_generation: AtomicU64::new(0),
            less_computer_combo_pending_press: Mutex::new(None),
            combo_hotkey: Mutex::new(None),
            side_aware_combo: Mutex::new(None),
            translation_hotkey: Mutex::new(None),
            switch_style_hotkey: Mutex::new(None),
            open_app_hotkey: Mutex::new(None),
            style_pack_hotkeys: Mutex::new(std::collections::HashMap::new()),
            selection_polish_hotkey: Mutex::new(None),
            selection_voice_host: Arc::new(Mutex::new(
                selection_voice_session::SelectionVoiceHostState::default(),
            )),
            selection_voice_capture: Mutex::new(None),
            qa_hotkey: Mutex::new(None),
            coding_agent_modifier_hotkey: Mutex::new(None),
            coding_agent_combo_hotkey: Mutex::new(None),
            last_capsule_state: Mutex::new(None),
            capsule_event_epoch: AtomicU64::new(0),
            capsule_event_lock: Mutex::new(()),
            selection_polish_capsule_active: AtomicBool::new(false),
            qa_context: Arc::new(TauriQaHostContext::default()),
            shutdown: AtomicBool::new(false),
        });
        (Coordinator { inner }, recorder, data_dir)
    }

    #[tokio::test]
    async fn translation_stopped_outside_the_hotkey_does_not_leak_to_the_next_session() {
        for stop_entry in ["button", "cli", "silence"] {
            let (coordinator, _, data_dir) =
                fixture_coordinator(crate::types::HotkeyMode::Toggle, std::time::Duration::ZERO);
            let inner = &coordinator.inner;
            let mut prefs = inner.backend.get_preferences();
            prefs.translation_target_language = "English".into();
            prefs.working_languages = vec!["简体中文".into()];
            crate::set_backend_preferences_for_test(&inner.backend, prefs);
            let at = std::time::Instant::now();
            handle_pressed_edge(inner, at, 1).await;
            handle_released_edge(inner, at + std::time::Duration::from_millis(300), 1).await;
            assert!(arm_translation_if_effective(inner).await);
            match stop_entry {
                "cli" => {
                    inner
                        .backend
                        .dispatch_cli_intent(openless_core::CliIntent::ToggleDictation)
                        .await
                        .unwrap();
                }
                "silence" => {
                    inner
                        .backend
                        .stop_dictation_session(
                            inner.backend.snapshot().dictation.session_id.unwrap(),
                        )
                        .await
                        .unwrap();
                }
                _ => {
                    inner.backend.stop_dictation().await.unwrap();
                }
            }
            assert!(inner.backend.list_history().unwrap()[0].translation_active);
            handle_pressed_edge(inner, at + std::time::Duration::from_secs(2), 2).await;
            assert_eq!(
                inner.backend.snapshot().dictation.phase,
                openless_core::DictationPhase::Recording
            );
            assert!(
                !inner.backend.snapshot().dictation.translation_active,
                "{stop_entry} must not retain the previous request"
            );
            inner.backend.cancel_dictation(None).await.unwrap();
            drop(coordinator);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }

    #[test]
    fn core_capsule_preserves_complete_feedback_before_the_window_is_bound() {
        let (coordinator, _, data_dir) =
            fixture_coordinator(crate::types::HotkeyMode::Toggle, std::time::Duration::ZERO);
        coordinator.inner.host.begin_insert_fallback_card();
        let expected = CapsulePayload {
            state: CapsuleState::Recording,
            level: 0.0,
            elapsed_ms: 0,
            message: Some("准备录音".into()),
            inserted_chars: None,
            warming: true,
            translation: true,
            operating: true,
            selection_polish: false,
            capsule_style: CapsuleStyle::Classic,
        };
        coordinator.present_core_capsule(expected.clone());
        let (_, actual) = coordinator.inner.host.dismiss_insert_fallback_card();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    struct DelayedFixtureRecorder {
        recorder: Arc<openless_core::testing::FixtureAudioRecorder>,
        startup_delay: std::time::Duration,
    }

    impl openless_core::AudioRecorder for DelayedFixtureRecorder {
        fn start(
            &self,
            session_id: openless_core::SessionId,
            context: Arc<openless_core::DictationContext>,
            consumer: Arc<dyn openless_core::AudioConsumer>,
            progress: Arc<dyn openless_core::RecordingProgressSink>,
        ) -> futures_util::future::BoxFuture<
            'static,
            Result<Box<dyn openless_core::ActiveRecording>, openless_core::BackendError>,
        > {
            let recorder = self.recorder.clone();
            let delay = self.startup_delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                recorder
                    .start(session_id, context, consumer, progress)
                    .await
            })
        }
    }

    #[tokio::test]
    async fn escape_after_toggle_release_allows_the_next_less_recording() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(crate::types::HotkeyMode::Toggle, std::time::Duration::ZERO);
        let inner = &coordinator.inner;
        let first = handle_less_computer_pressed(inner, 1, std::time::Instant::now())
            .await
            .unwrap();
        handle_less_computer_released(inner, Some(first), 1, std::time::Instant::now()).await;
        assert!(super::super::dictation::cancel_active_session(inner).await);
        assert_eq!(recorder.stop_count(), 1);
        assert!(
            handle_less_computer_pressed(inner, 2, std::time::Instant::now())
                .await
                .is_some(),
            "Esc must release the Host capture slot as well as the Core lease"
        );
        super::super::dictation::cancel_active_session(inner).await;
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn cli_cancel_releases_less_capture_and_allows_recording_again() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(crate::types::HotkeyMode::Toggle, std::time::Duration::ZERO);
        handle_less_computer_pressed(&coordinator.inner, 1, std::time::Instant::now())
            .await
            .unwrap();
        assert_eq!(
            coordinator.cancel_dictation_from_cli().await.unwrap(),
            openless_core::CliDispatchOutcome::DictationCancelled
        );
        assert_eq!(recorder.stop_count(), 1);
        assert!(
            handle_less_computer_pressed(&coordinator.inner, 2, std::time::Instant::now())
                .await
                .is_some()
        );
        coordinator.dismiss_less_computer().await.unwrap();
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn dismiss_less_window_stops_capture_and_allows_reopening() {
        let (coordinator, recorder, data_dir) =
            fixture_coordinator(crate::types::HotkeyMode::Toggle, std::time::Duration::ZERO);
        handle_less_computer_pressed(&coordinator.inner, 1, std::time::Instant::now())
            .await
            .unwrap();
        coordinator.dismiss_less_computer().await.unwrap();
        assert_eq!(
            recorder.stop_count(),
            1,
            "closing the panel must stop the microphone"
        );
        assert!(
            handle_less_computer_pressed(&coordinator.inner, 2, std::time::Instant::now())
                .await
                .is_some()
        );
        coordinator.dismiss_less_computer().await.unwrap();
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[tokio::test]
    async fn auto_short_tap_keeps_recording_after_a_slow_native_start() {
        // 两种原生监听器都排队保存原始时间，不能把设备启动耗时算进物理按住时长。
        for combo in [false, true] {
            let (coordinator, recorder, data_dir) = fixture_coordinator(
                crate::types::HotkeyMode::Auto,
                std::time::Duration::from_millis(450),
            );
            let inner = coordinator.inner.clone();
            let pressed = std::time::Instant::now();
            let released = pressed + std::time::Duration::from_millis(50);
            let bridge = if combo {
                let (tx, rx) = mpsc::channel();
                tx.send(ComboHotkeyEvent::Pressed { at: pressed }).unwrap();
                tx.send(ComboHotkeyEvent::Released { at: released })
                    .unwrap();
                drop(tx);
                std::thread::spawn(move || less_computer_combo_bridge_loop(inner, rx))
            } else {
                let (tx, rx) = mpsc::channel();
                tx.send(HotkeyEvent::Pressed {
                    at: pressed,
                    press_id: 47,
                })
                .unwrap();
                tx.send(HotkeyEvent::Released {
                    at: released,
                    press_id: 47,
                })
                .unwrap();
                drop(tx);
                std::thread::spawn(move || less_computer_modifier_bridge_loop(inner, rx))
            };
            tokio::task::spawn_blocking(move || bridge.join().unwrap())
                .await
                .unwrap();
            assert_eq!(
                recorder.stop_count(),
                0,
                "50ms Auto tap must not become a Hold release after 450ms native startup"
            );
            coordinator.dismiss_less_computer().await.unwrap();
            drop(coordinator);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }

    #[tokio::test]
    async fn recording_control_stop_is_delivered_once_on_both_sides_of_handoff() {
        use openless_core::RecordingControlSink;
        for before_handoff in [true, false] {
            let (coordinator, recorder, data_dir) =
                fixture_coordinator(crate::types::HotkeyMode::Toggle, std::time::Duration::ZERO);
            let inner = &coordinator.inner;
            let id = openless_core::SessionId::new();
            let control = Arc::new(LessComputerRecordingControl::new(inner));
            *inner.less_computer_voice.lock() =
                Some(LessComputerHostCapture::Starting(id, control.clone()));
            let session = inner
                .backend
                .start_less_computer_voice(id, control.clone())
                .await
                .unwrap();
            if before_handoff {
                control
                    .request(id, openless_core::RecordingControlAction::Stop)
                    .unwrap();
            }
            assert!(attach_less_computer_recording(&inner.less_computer_voice, session).is_ok());
            if !before_handoff {
                control
                    .request(id, openless_core::RecordingControlAction::Stop)
                    .unwrap();
            }
            control.flush(id);
            control.flush(id);
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while recorder.stop_count() != 1
                    || inner.backend.less_computer_active_session().is_some()
                {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            })
            .await
            .expect("Stop must reach native capture and release the lease");
            assert_eq!(recorder.stop_count(), 1);
            drop(coordinator);
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }

    #[tokio::test]
    async fn capsule_stop_during_starting_waits_for_one_native_handoff() {
        let (coordinator, recorder, data_dir) = fixture_coordinator(
            crate::types::HotkeyMode::Toggle,
            std::time::Duration::from_millis(100),
        );
        let inner = coordinator.inner.clone();
        let starting = tokio::spawn(async move {
            handle_less_computer_pressed(&inner, 1, std::time::Instant::now()).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while coordinator
                .backend()
                .less_computer_active_session()
                .is_none()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(coordinator.stop_less_computer_recording().await.unwrap());
        starting.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while recorder.stop_count() != 1
                || coordinator
                    .backend()
                    .less_computer_active_session()
                    .is_some()
            {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("the queued capsule Stop must be applied after recorder startup");
        assert_eq!(recorder.stop_count(), 1);
        drop(coordinator);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    struct IgnoreRecordingControl;

    impl openless_core::RecordingControlSink for IgnoreRecordingControl {
        fn request(
            &self,
            _: openless_core::SessionId,
            _: openless_core::RecordingControlAction,
        ) -> Result<(), openless_core::BackendError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn rebind_cancels_only_the_owned_starting_or_recording_capture() {
        use openless_core::testing::{
            FixtureAudioRecorder, FixtureTextPolisher, FixtureTranscriptionEngine,
        };
        let data_dir =
            std::env::temp_dir().join(format!("openless-less-hotkey-{}", uuid::Uuid::new_v4()));
        let recorder = Arc::new(FixtureAudioRecorder::default());
        let transcription = Arc::new(FixtureTranscriptionEngine::successful("voice", 120));
        let backend = openless_core::OpenLessBackend::new(
            openless_core::BackendConfig {
                data_dir: data_dir.clone(),
                ..Default::default()
            },
            openless_core::BackendDependencies {
                host_actions: Arc::new(openless_core::ports::NoopHostActions),
                dictation_engine: Arc::new(openless_core::PipelineDictationEngine::new(
                    recorder.clone(),
                    transcription.clone(),
                    Arc::new(FixtureTextPolisher::successful("unused")),
                )),
                ..openless_core::BackendDependencies::unsupported()
            },
        )
        .unwrap();
        let mut prefs = backend.get_preferences();
        prefs.coding_agent_enabled = true;
        crate::set_backend_preferences_for_test(&backend, prefs);

        // 冷启动只有session id。取走slot后立即释放所属lease，不能等未来的Released。
        let pending = openless_core::SessionId::new();
        backend.begin_less_computer_capture(pending).unwrap();
        let slot = Mutex::new(Some(LessComputerHostCapture::Starting(
            pending,
            Arc::new(IgnoreRecordingControl),
        )));
        take_less_computer_host_capture(&slot, None)
            .unwrap()
            .cancel(&backend)
            .await
            .unwrap();
        assert!(slot.lock().is_none());
        assert_eq!(backend.less_computer_active_session(), None);

        // 已开麦的同一取消入口必须实际停止录音和ASR，各一次。
        let recording_id = openless_core::SessionId::new();
        let session = backend
            .start_less_computer_voice(recording_id, Arc::new(IgnoreRecordingControl))
            .await
            .unwrap();
        *slot.lock() = Some(LessComputerHostCapture::Recording(session));
        assert!(take_less_computer_host_capture(&slot, Some(pending)).is_none());
        assert_eq!(backend.less_computer_active_session(), Some(recording_id));
        take_less_computer_host_capture(&slot, None)
            .unwrap()
            .cancel(&backend)
            .await
            .unwrap();
        assert_eq!(recorder.stop_count(), 1);
        assert_eq!(transcription.cancel_count(), 1);
        assert_eq!(backend.less_computer_active_session(), None);

        // 原生启动迟到时，slot已经被取消/替换。释放真实fixture资源而不覆盖新owner。
        let late_id = openless_core::SessionId::new();
        let late = backend
            .start_less_computer_voice(late_id, Arc::new(IgnoreRecordingControl))
            .await
            .unwrap();
        let new_owner = openless_core::SessionId::new();
        *slot.lock() = Some(LessComputerHostCapture::Starting(
            new_owner,
            Arc::new(IgnoreRecordingControl),
        ));
        attach_less_computer_recording(&slot, late)
            .unwrap_err()
            .cancel()
            .await
            .unwrap();
        assert_eq!(
            slot.lock()
                .as_ref()
                .map(LessComputerHostCapture::session_id),
            Some(new_owner)
        );
        assert_eq!(recorder.stop_count(), 2);
        assert_eq!(transcription.cancel_count(), 2);
        slot.lock().take();

        // 旧id的异步取消不能影响替换会话；没有Host capture也不会发全局取消。
        let replacement = openless_core::SessionId::new();
        backend.begin_less_computer_capture(replacement).unwrap();
        LessComputerHostCapture::Starting(pending, Arc::new(IgnoreRecordingControl))
            .cancel(&backend)
            .await
            .unwrap();
        assert_eq!(backend.less_computer_active_session(), Some(replacement));
        assert!(take_less_computer_host_capture(&slot, None).is_none());
        backend.abort_less_computer_capture(replacement).unwrap();
        drop(backend);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn agent_shortcuts_validate_real_windows_capabilities() {
        for primary in [
            "LeftControl",
            "RightControl",
            "LeftOption",
            "RightOption",
            "LeftShift",
            "RightShift",
            "LeftCommand",
            "RightCommand",
            "MediaPlayPause",
        ] {
            let binding = crate::types::ShortcutBinding {
                primary: primary.into(),
                modifiers: Vec::new(),
            };
            validate_less_computer_hotkey(&binding).unwrap();
            let native = less_computer_modifier_binding(&binding).unwrap();
            assert_eq!(native.mode, crate::types::HotkeyMode::Hold);
        }
        let combo = crate::types::ShortcutBinding {
            primary: "J".into(),
            modifiers: vec!["ctrl".into(), "shift".into()],
        };
        validate_less_computer_hotkey(&combo).unwrap();
        assert!(less_computer_modifier_binding(&combo).is_none());
        assert!(
            validate_less_computer_hotkey(&crate::types::ShortcutBinding {
                primary: "Fn".into(),
                modifiers: Vec::new(),
            })
            .unwrap_err()
            .contains("Windows 不支持 Fn")
        );
        assert!(
            validate_less_computer_hotkey(&crate::types::ShortcutBinding {
                primary: "J".into(),
                modifiers: vec!["ctrl-left".into()],
            })
            .is_err()
        );
    }
}

#[cfg(any())]
mod tests {
    use super::*;

    fn style_hotkey(pack_id: &str, primary: &str) -> crate::types::StylePackHotkey {
        crate::types::StylePackHotkey {
            pack_id: pack_id.into(),
            binding: crate::types::ShortcutBinding {
                primary: primary.into(),
                modifiers: vec!["alt".into()],
            },
        }
    }

    #[test]
    fn style_pack_hotkey_registration_failure_leaves_no_partial_table() {
        let entries = [
            style_hotkey("builtin.raw", "1"),
            style_hotkey("imported.x", "2"),
        ];
        let mut registrations = std::collections::HashMap::from([("old".into(), 9_u8)]);

        let result =
            replace_style_pack_hotkey_registrations(&entries, &mut registrations, |entry| {
                if entry.pack_id == "imported.x" {
                    Err("register imported.x failed".into())
                } else {
                    Ok(1)
                }
            });

        assert_eq!(result.unwrap_err(), "register imported.x failed");
        assert!(registrations.is_empty());
    }

    #[test]
    fn style_pack_hotkey_registration_success_replaces_entire_table() {
        let entries = [
            style_hotkey("builtin.raw", "1"),
            style_hotkey("imported.x", "2"),
        ];
        let mut registrations = std::collections::HashMap::from([("old".into(), 9_u8)]);

        replace_style_pack_hotkey_registrations(&entries, &mut registrations, |entry| {
            Ok(if entry.pack_id == "builtin.raw" { 1 } else { 2 })
        })
        .unwrap();

        assert_eq!(registrations.len(), 2);
        assert_eq!(registrations.get("builtin.raw"), Some(&1));
        assert_eq!(registrations.get("imported.x"), Some(&2));
        assert!(!registrations.contains_key("old"));
    }

    #[test]
    fn style_pack_hotkey_registration_state_matches_exact_bindings() {
        let desired = [
            style_hotkey("builtin.raw", "1"),
            style_hotkey("imported.x", "2"),
        ];
        let registrations = desired
            .iter()
            .map(|entry| (entry.pack_id.clone(), entry.binding.clone()))
            .collect();

        assert!(style_pack_hotkey_registrations_match(
            &desired,
            &registrations,
            |binding| binding,
        ));
    }

    #[test]
    fn style_pack_hotkey_registration_state_detects_changed_or_missing_bindings() {
        let desired = [
            style_hotkey("builtin.raw", "1"),
            style_hotkey("imported.x", "2"),
        ];
        let changed = std::collections::HashMap::from([
            (
                "builtin.raw".into(),
                style_hotkey("builtin.raw", "9").binding,
            ),
            ("imported.x".into(), desired[1].binding.clone()),
        ]);
        let partial =
            std::collections::HashMap::from([("builtin.raw".into(), desired[0].binding.clone())]);

        assert!(!style_pack_hotkey_registrations_match(
            &desired,
            &changed,
            |binding| binding,
        ));
        assert!(!style_pack_hotkey_registrations_match(
            &desired,
            &partial,
            |binding| binding,
        ));
        assert!(style_pack_hotkey_registrations_match(
            &[],
            &std::collections::HashMap::<String, crate::types::ShortcutBinding>::new(),
            |binding| binding,
        ));
        assert!(!style_pack_hotkey_registrations_match(
            &[style_hotkey("builtin.raw", "1")],
            &std::collections::HashMap::<String, crate::types::ShortcutBinding>::new(),
            |binding| binding,
        ));
    }

    /// 轮询 `inner.state.cancelled` 直到满足条件，超时返回 false。
    fn wait_until(mut cond: impl FnMut() -> bool, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    /// 构造旧 Coordinator 状态，用注入的 handler 验证 bridge 自身的信号语义。
    fn coordinator_in_processing() -> Coordinator {
        let coordinator = Coordinator::new();
        let mut state = coordinator.inner.state.lock();
        state.phase = SessionPhase::Processing;
        state.cancelled = false;
        drop(state);
        coordinator
    }

    /// 后台运行 esc_cancel_bridge_loop，返回 sender 与 join handle。
    fn spawn_loop(inner: &Arc<Inner>) -> (mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<()>();
        let bridge_inner = Arc::clone(inner);
        let handle = std::thread::spawn(move || {
            esc_cancel_bridge_loop_with(bridge_inner, rx, |inner| {
                inner.state.lock().cancelled = true;
            })
        });
        (tx, handle)
    }

    #[test]
    fn esc_cancel_bridge_sets_cancelled_during_processing() {
        let coordinator = coordinator_in_processing();
        let (tx, handle) = spawn_loop(&coordinator.inner);

        tx.send(()).unwrap();
        assert!(
            wait_until(
                || coordinator.inner.state.lock().cancelled,
                std::time::Duration::from_secs(2)
            ),
            "取消信号应置 cancelled 旗标"
        );
        // #798 语义：Processing 阶段保持 phase=Processing，由 end_session 自行收尾。
        assert_eq!(
            coordinator.inner.state.lock().phase,
            SessionPhase::Processing
        );

        drop(tx);
        handle.join().unwrap();
    }

    #[test]
    fn esc_cancel_bridge_skips_while_shortcut_recording_active() {
        let coordinator = coordinator_in_processing();
        let (tx, handle) = spawn_loop(&coordinator.inner);

        coordinator.set_shortcut_recording_active(true);
        tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert!(
            !coordinator.inner.state.lock().cancelled,
            "录制快捷键期间按 Esc 应被忽略"
        );

        // 录制结束后 Esc 恢复生效。
        coordinator.set_shortcut_recording_active(false);
        tx.send(()).unwrap();
        assert!(
            wait_until(
                || coordinator.inner.state.lock().cancelled,
                std::time::Duration::from_secs(2)
            ),
            "录制结束后取消信号应恢复生效"
        );

        drop(tx);
        handle.join().unwrap();
    }

    #[test]
    fn esc_cancel_bridge_is_idempotent_on_repeat_signals() {
        let coordinator = coordinator_in_processing();
        let (tx, handle) = spawn_loop(&coordinator.inner);

        for _ in 0..3 {
            tx.send(()).unwrap();
        }
        assert!(
            wait_until(
                || coordinator.inner.state.lock().cancelled,
                std::time::Duration::from_secs(2)
            ),
            "首个取消信号应置 cancelled 旗标"
        );
        // 连按 Esc / 双通道重复触发时 cancel_session 幂等：不 panic、状态不回写。
        tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert_eq!(
            coordinator.inner.state.lock().phase,
            SessionPhase::Processing
        );

        drop(tx);
        handle.join().unwrap();
    }
}
