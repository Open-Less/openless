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
    while rx.recv().is_ok() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        cancel_session(&inner);
    }
}

/// 组合键撤销专用消费线程。撤销事件携带触发键按下代次，避免独立通道的迟到事件
/// 误取消下一次按下开启的会话。
pub(super) fn combo_abort_bridge_loop(
    inner: Arc<Inner>,
    rx: mpsc::Receiver<u64>,
    handler: fn(&Arc<Inner>, u64),
) {
    while let Ok(press_id) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        handler(&inner, press_id);
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
    handler: fn(&Arc<Inner>, u64),
) -> mpsc::Sender<u64> {
    let (combo_tx, combo_rx) = mpsc::channel::<u64>();
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
        let prefs = inner.prefs.get();

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
        let trigger = crate::shortcut_binding::legacy_modifier_trigger(&prefs.dictation_hotkey)
            .unwrap_or(crate::types::HotkeyTrigger::Custom);
        let binding = crate::types::HotkeyBinding {
            trigger,
            mode: prefs.hotkey.mode,
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
                    let (qa_trigger, _selection_polish_trigger, translation_trigger) =
                        modifier_shortcut_triggers(&inner);
                    let custom_key = custom_dictation_key_string(&inner);
                    crate::linux_fcitx::start_dictation_signal_listener(
                        fcitx_tx,
                        combo_tx_for_fcitx,
                        fcitx_binding.clone(),
                        qa_trigger,
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
        // 用户已经把 QA 关掉就睡着等 prefs 改动；改动通过 update_qa_hotkey_binding 唤醒。
        let binding = match inner.prefs.get().qa_hotkey.clone() {
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
        let app = inner.app.lock().clone();
        let app = match app {
            Some(a) => a,
            None => {
                // 启动期 AppHandle 还没 bind，再等。
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        let (tx, rx) = mpsc::channel::<QaHotkeyEvent>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<QaHotkeyMonitor, QaHotkeyError>>(1);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            let result = QaHotkeyMonitor::start(binding_for_main, tx);
            let _ = init_tx.send(result);
        });

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
                async_runtime::spawn(async move { handle_qa_hotkey_pressed(&inner_cloned).await });
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
    let binding = inner.prefs.get().selection_polish_hotkey.clone();
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
    let app = inner.app.lock().clone().ok_or_else(|| {
        "AppHandle unavailable while registering Selection Polish hotkey".to_string()
    })?;
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let inner_for_main = Arc::clone(inner);
    app.run_on_main_thread(move || {
        let result = update_selection_polish_hotkey_on_main_thread(inner_for_main, binding)
            .map_err(|error| error.to_string());
        let _ = result_tx.send(result);
    })
    .map_err(|error| error.to_string())?;
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
        if inner.shortcut_recording_active.load(Ordering::SeqCst)
            || !matches!(event, ComboHotkeyEvent::Pressed { .. })
        {
            continue;
        }
        let coordinator = Coordinator {
            inner: Arc::clone(&inner),
        };
        async_runtime::spawn(async move {
            if let Err(error) = coordinator.trigger_selection_polish().await {
                log::warn!("[selection-polish] combo hotkey workflow failed: {error}");
            }
        });
    }
}

#[cfg(not(mobile))]
pub(super) fn take_selection_polish_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let app = inner.app.lock().clone();
    if let Some(app) = app {
        let inner = Arc::clone(inner);
        let _ = app.run_on_main_thread(move || {
            inner.selection_polish_hotkey.lock().take();
        });
    } else {
        inner.selection_polish_hotkey.lock().take();
    }
}

// ─────────────────────────── combo hotkey supervisor ───────────────────────────

// ─────────────────────── coding agent hotkey supervisor ───────────────────────

pub(super) fn coding_agent_hotkey_supervisor_loop(inner: Arc<Inner>) {
    // Less Computer (coding agent) is macOS-only. On Windows/Linux the binding can
    // never be installed, so do the one-shot take and let the thread exit instead
    // of waking every 5s for the entire life of the process.
    #[cfg(not(target_os = "macos"))]
    {
        update_coding_agent_hotkey_binding_now(&inner);
    }
    #[cfg(target_os = "macos")]
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        update_coding_agent_hotkey_binding_now(&inner);
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// 「Less Computer 语音键已禁用」是否已经打过日志。
///
/// supervisor 每 5s 轮询一次，没配语音键的用户每轮都会落到同一条禁用分支。无条件
/// 打印会稳定产出 720 行/小时的同一句话 —— 实测一份跑了六天的 openless.log 里它占
/// 了 95% 以上，真正有用的会话日志被冲得很难找，日志轮转也被它提前触发。
/// 只在状态翻转成禁用时打一次；重新装上语音键时清掉，下次禁用还会再打。
#[cfg(target_os = "macos")]
static LESS_COMPUTER_HOTKEY_DISABLED_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) fn update_coding_agent_hotkey_binding_now(inner: &Arc<Inner>) {
    #[cfg(not(target_os = "macos"))]
    {
        // Less Computer is intentionally macOS-only for now; keep Windows/Linux hidden and inert.
        take_coding_agent_hotkeys_on_main_thread(inner);
        return;
    }

    #[cfg(target_os = "macos")]
    {
        let prefs = inner.prefs.get();
        let Some(binding) = prefs.coding_agent_voice_hotkey.clone() else {
            take_coding_agent_hotkeys_on_main_thread(inner);
            if !LESS_COMPUTER_HOTKEY_DISABLED_LOGGED.swap(true, Ordering::SeqCst) {
                log::info!("[less-computer] hotkey disabled");
            }
            return;
        };
        if !prefs.coding_agent_enabled || is_unconfigured_shortcut(&binding) {
            take_coding_agent_hotkeys_on_main_thread(inner);
            return;
        }
        LESS_COMPUTER_HOTKEY_DISABLED_LOGGED.store(false, Ordering::SeqCst);

        if let Some(modifier_binding) = less_computer_modifier_binding(&binding) {
            take_coding_agent_combo_hotkey_on_main_thread(inner);
            if let Some(monitor) = inner.coding_agent_modifier_hotkey.lock().as_ref() {
                monitor.update_binding(modifier_binding);
                return;
            }
            let (tx, rx) = mpsc::channel::<HotkeyEvent>();
            // Less Computer 的独立 tap 也转发 Esc 取消与组合键撤销（与主 monitor 双保险；
            // cancel_session 幂等，重复触发无害）。组合键撤销撤销的是 voice_agent 会话。
            let cancel_tx = spawn_esc_cancel_bridge(inner);
            let combo_tx = spawn_combo_abort_bridge(inner, cancel_less_computer_press);
            match HotkeyMonitor::start(modifier_binding, tx, cancel_tx, combo_tx) {
                Ok(monitor) => {
                    *inner.coding_agent_modifier_hotkey.lock() = Some(monitor);
                    log::info!(
                        "[less-computer] modifier hotkey installed ({})",
                        binding.display_label()
                    );
                    let bridge_inner = Arc::clone(inner);
                    std::thread::Builder::new()
                        .name("openless-less-computer-modifier-bridge".into())
                        .spawn(move || less_computer_modifier_bridge_loop(bridge_inner, rx))
                        .ok();
                }
                Err(e) => log::warn!("[less-computer] modifier hotkey install failed: {e}"),
            }
            return;
        }

        inner.coding_agent_modifier_hotkey.lock().take();
        let app = match inner.app.lock().clone() {
            Some(app) => app,
            None => {
                log::warn!("[less-computer] AppHandle 未 bind，跳过组合键注册");
                return;
            }
        };
        let inner_clone = Arc::clone(inner);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(monitor) = inner_clone.coding_agent_combo_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(binding_for_main.clone()) {
                    log::warn!("[less-computer] combo hotkey update failed: {e}");
                }
                return;
            }
            let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
            match ComboHotkeyMonitor::start(binding_for_main.clone(), tx) {
                Ok(monitor) => {
                    *inner_clone.coding_agent_combo_hotkey.lock() = Some(monitor);
                    log::info!(
                        "[less-computer] combo hotkey installed ({})",
                        binding_for_main.display_label()
                    );
                    let bridge_inner = Arc::clone(&inner_clone);
                    std::thread::Builder::new()
                        .name("openless-less-computer-combo-bridge".into())
                        .spawn(move || less_computer_combo_bridge_loop(bridge_inner, rx))
                        .ok();
                }
                Err(e) => log::warn!("[less-computer] combo hotkey install failed: {e}"),
            }
        });
    }
}

#[cfg(target_os = "macos")]
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

pub(super) fn less_computer_modifier_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<HotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            HotkeyEvent::Pressed { press_id, .. } => {
                async_runtime::block_on(async {
                    handle_less_computer_modifier_pressed(&inner_cloned, press_id).await
                });
            }
            HotkeyEvent::Released { .. } => {
                async_runtime::block_on(async {
                    handle_less_computer_released(&inner_cloned).await
                });
            }
            // Esc 取消与组合键撤销都不在此枚举里：分别走 esc_cancel_bridge_loop /
            // combo_abort_bridge_loop（见各自函数注释）。
            HotkeyEvent::TranslationModifierPressed | HotkeyEvent::QaShortcutPressed => {}
            #[cfg(not(mobile))]
            HotkeyEvent::SelectionPolishShortcutPressed => {}
        }
    }
}

/// Less Computer 触发键被当修饰键用（Option+任意字母/数字键之类）：撤销这次按下开出的语音会话。
/// handle_less_computer_pressed 只在 Idle 时开会话，所以此刻还在跑的 voice_agent
/// 会话必然就是这次按下开出来的；其他情况（按下被忽略）什么都不动。
async fn handle_less_computer_modifier_pressed(inner: &Arc<Inner>, press_id: u64) {
    if press_id == 0 {
        handle_less_computer_pressed(inner).await;
        return;
    }
    inner
        .less_computer_press_generation
        .store(press_id, Ordering::SeqCst);
    if inner
        .less_computer_combo_pending_press
        .compare_exchange(press_id, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        return;
    }
    handle_less_computer_pressed(inner).await;
    if inner
        .less_computer_combo_pending_press
        .compare_exchange(press_id, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        cancel_less_computer_voice_session(inner);
    }
}

fn cancel_less_computer_press(inner: &Arc<Inner>, press_id: u64) {
    if press_id == 0 {
        return;
    }
    inner
        .less_computer_combo_pending_press
        .store(press_id, Ordering::SeqCst);
    if inner.less_computer_press_generation.load(Ordering::SeqCst) != press_id {
        return;
    }
    cancel_less_computer_voice_session(inner);
}

fn cancel_less_computer_voice_session(inner: &Arc<Inner>) {
    let (phase, voice_agent) = {
        let state = inner.state.lock();
        (state.phase, state.voice_agent)
    };
    if !voice_agent || !matches!(phase, SessionPhase::Starting | SessionPhase::Listening) {
        return;
    }
    let _ = inner.less_computer_combo_pending_press.swap(0, Ordering::SeqCst);
    log::info!("[less-computer] 触发键与其他键组合按下 —— 取消本次按下开出的会话");
    cancel_session(inner);
    if let Some(app) = inner.app.lock().clone() {
        crate::hide_less_computer_glow(&app);
    }
}

pub(super) fn less_computer_combo_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<ComboHotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            ComboHotkeyEvent::Pressed { .. } => {
                async_runtime::block_on(async {
                    handle_less_computer_pressed(&inner_cloned).await
                });
            }
            ComboHotkeyEvent::Released { .. } => {
                async_runtime::block_on(async {
                    handle_less_computer_released(&inner_cloned).await
                });
            }
        }
    }
}

pub(super) async fn handle_less_computer_pressed(inner: &Arc<Inner>) {
    let prefs = inner.prefs.get();
    if !prefs.coding_agent_enabled {
        return;
    }
    if !matches!(inner.state.lock().phase, SessionPhase::Idle) {
        log::info!("[less-computer] press ignored: dictation session already active");
        return;
    }
    if !matches!(inner.qa_state.lock().phase, QaPhase::Idle) {
        log::info!("[less-computer] press ignored: QA session active");
        return;
    }

    // voice_agent=true 在 Starting 阶段就写入 state，防止 finish_starting_session
    // 处理 pending_stop 时（快速松手 race）丢失标志，导致意外走普通听写路径。
    if begin_session_as(inner, true).await.is_err() {
        return;
    }
    let started = {
        let state = inner.state.lock();
        // voice_agent 已在 begin_session_as 内设置；这里只检查阶段是否推进成功。
        if matches!(
            state.phase,
            SessionPhase::Starting | SessionPhase::Listening | SessionPhase::Processing
        ) {
            log::info!(
                "[less-computer] voice session started (session={:?})",
                state.session_id
            );
            true
        } else {
            false
        }
    };
    // 一按下键（开始录音）就点亮整屏彩虹描边，贯穿 录音 → 处理 → 出结果，完成/关闭才熄灭。
    if started {
        if let Some(app) = inner.app.lock().clone() {
            crate::show_less_computer_glow(&app);
        }
    }
}

pub(super) async fn handle_less_computer_released(inner: &Arc<Inner>) {
    let (phase, voice_agent) = {
        let state = inner.state.lock();
        (state.phase, state.voice_agent)
    };
    if !voice_agent {
        return;
    }
    match phase {
        SessionPhase::Listening => {
            let _ = end_session(inner).await;
            // 收尾后熄灭整屏描边。正常路径 run_voice_agent_transcript 已熄过、这里兜底；
            // 空转写/出错路径不进 run_voice_agent_transcript，全靠这里熄，否则描边卡住不灭。
            if let Some(app) = inner.app.lock().clone() {
                crate::hide_less_computer_glow(&app);
            }
        }
        SessionPhase::Starting => {
            // 握手中松手：排队；正常路径真正收尾在 begin 续流的 end_session → run_voice_agent_transcript 熄灭。
            request_stop_during_starting(inner, "less-computer release edge");
            // 但若初始化失败永远到不了 Listening（不会进 run_voice_agent_transcript），
            // 描边会永久卡屏 → 这里兜底熄灭。Listening 分支已有熄灭逻辑，故只在 Starting 加。
            if let Some(app) = inner.app.lock().clone() {
                crate::hide_less_computer_glow(&app);
            }
        }
        _ => {}
    }
}

pub(super) fn take_coding_agent_hotkeys_on_main_thread(inner: &Arc<Inner>) {
    inner.coding_agent_modifier_hotkey.lock().take();
    take_coding_agent_combo_hotkey_on_main_thread(inner);
}

pub(super) fn take_coding_agent_combo_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let app = inner.app.lock().clone();
    if let Some(app) = app {
        let inner = Arc::clone(inner);
        let _ = app.run_on_main_thread(move || {
            inner.coding_agent_combo_hotkey.lock().take();
        });
    } else {
        inner.coding_agent_combo_hotkey.lock().take();
    }
}

pub(super) fn combo_hotkey_supervisor_loop(inner: Arc<Inner>) {
    let mut attempts: u32 = 0;
    loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            return;
        }
        // 读当前 prefs
        let prefs = inner.prefs.get();
        if crate::shortcut_binding::legacy_modifier_trigger(&prefs.dictation_hotkey).is_some() {
            take_combo_hotkey_on_main_thread(&inner);
            inner.side_aware_combo.lock().take();
            return;
        }

        let binding = prefs.dictation_hotkey.clone();
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
                        log::warn!("[coord] side-aware combo 第 {attempts} 次注册失败: {e}; 3s 后重试");
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

        let app = inner.app.lock().clone();
        let app = match app {
            Some(a) => a,
            None => {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let (init_tx, init_rx) =
            mpsc::sync_channel::<Result<ComboHotkeyMonitor, ComboHotkeyError>>(1);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            let result = ComboHotkeyMonitor::start(binding_for_main, tx);
            let _ = init_tx.send(result);
        });

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
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            // P0 #468/#475: 同 hotkey_bridge_loop —— Pressed/Released 必须串行 await，
            // 否则 latch 竞态导致 combo 快捷键二次按键失效。
            ComboHotkeyEvent::Pressed { at } => {
                async_runtime::block_on(async {
                    handle_pressed_edge(&inner_cloned, at, 0).await;
                });
            }
            ComboHotkeyEvent::Released { at } => {
                async_runtime::block_on(async {
                    handle_released_edge(&inner_cloned, at).await;
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
        let binding = inner.prefs.get().translation_hotkey;
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

        let app = match inner.app.lock().clone() {
            Some(a) => a,
            None => {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let (init_tx, init_rx) =
            mpsc::sync_channel::<Result<ComboHotkeyMonitor, ComboHotkeyError>>(1);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            let result = ComboHotkeyMonitor::start(binding_for_main, tx);
            let _ = init_tx.send(result);
        });

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

pub(super) fn translation_hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<ComboHotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        if matches!(evt, ComboHotkeyEvent::Pressed { .. }) {
            mark_translation_modifier_seen(&inner);
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

        let app = match inner.app.lock().clone() {
            Some(a) => a,
            None => {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };

        let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
        let (init_tx, init_rx) =
            mpsc::sync_channel::<Result<ComboHotkeyMonitor, ComboHotkeyError>>(1);
        let binding_for_main = binding.clone();
        let _ = app.run_on_main_thread(move || {
            let result = ComboHotkeyMonitor::start(binding_for_main, tx);
            let _ = init_tx.send(result);
        });

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
        ActionHotkeyKind::OpenApp => {
            if let Some(app) = inner.app.lock().clone() {
                let app_for_main = app.clone();
                let _ = app.run_on_main_thread(move || {
                    crate::show_main_window(&app_for_main);
                });
            }
        }
    }
}

pub(super) fn switch_to_previous_style(inner: &Arc<Inner>) {
    let mut prefs = inner.prefs.get();
    let packs = match inner.style_packs.list() {
        Ok(packs) => packs,
        Err(error) => {
            log::warn!("[coord] switch style hotkey failed to load style packs: {error}");
            return;
        }
    };
    let enabled: Vec<crate::types::StylePack> =
        packs.into_iter().filter(|pack| pack.enabled).collect();
    if enabled.len() <= 1 {
        log::info!("[coord] switch style hotkey ignored: enabled style count <= 1");
        return;
    }
    let current_index = enabled
        .iter()
        .position(|pack| pack.id == prefs.active_style_pack_id)
        .unwrap_or(0);
    let next_index = if current_index == 0 {
        enabled.len() - 1
    } else {
        current_index - 1
    };
    prefs.active_style_pack_id = enabled[next_index].id.clone();
    sync_style_pack_preferences(&mut prefs, &enabled);
    if let Err(e) = inner.prefs.set(prefs.clone()) {
        log::warn!("[coord] switch style hotkey 保存失败: {e}");
    } else {
        log::info!(
            "[coord] switch style hotkey changed active style pack to {}",
            prefs.active_style_pack_id
        );
        if let Some(app) = inner.app.lock().clone() {
            let _ = app.emit("prefs:changed", &prefs);
            let _ = app.emit_to("main", "prefs:changed", &prefs);
            let app_for_main = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Err(err) = crate::refresh_tray_microphone_menu(&app_for_main) {
                    log::warn!("[tray] refresh style menu after switch style hotkey failed: {err}");
                }
            });
        }
    }
}

pub(super) fn take_combo_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let app = inner.app.lock().clone();
    if let Some(app) = app {
        let inner = Arc::clone(inner);
        let _ = app.run_on_main_thread(move || {
            inner.combo_hotkey.lock().take();
        });
    } else {
        inner.combo_hotkey.lock().take();
    }
}

pub(super) fn take_translation_hotkey_on_main_thread(inner: &Arc<Inner>) {
    let app = inner.app.lock().clone();
    if let Some(app) = app {
        let inner = Arc::clone(inner);
        let _ = app.run_on_main_thread(move || {
            inner.translation_hotkey.lock().take();
        });
    } else {
        inner.translation_hotkey.lock().take();
    }
}

pub(super) fn take_action_hotkey_on_main_thread(inner: &Arc<Inner>, kind: ActionHotkeyKind) {
    let app = inner.app.lock().clone();
    if let Some(app) = app {
        let inner = Arc::clone(inner);
        let _ = app.run_on_main_thread(move || {
            action_hotkey_slot(&inner, kind).lock().take();
        });
    } else {
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
    let prefs = inner.prefs.get();
    match kind {
        ActionHotkeyKind::SwitchStyle => prefs.switch_style_hotkey,
        ActionHotkeyKind::OpenApp => prefs.open_app_hotkey,
    }
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

pub(super) fn is_builtin_translation_shift(binding: &crate::types::ShortcutBinding) -> bool {
    binding.modifiers.is_empty() && binding.primary.eq_ignore_ascii_case("shift")
}

/// Linux: 从 prefs 读取自定义组合键，同步到 fcitx5 插件。
#[cfg(target_os = "linux")]
pub(super) fn custom_dictation_key_string(inner: &Arc<Inner>) -> Option<String> {
    let prefs = inner.prefs.get();
    let key_string = crate::linux_fcitx::binding_to_fcitx_key_string(&prefs.dictation_hotkey);
    if key_string.is_empty() {
        None
    } else {
        Some(key_string)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn sync_custom_dictation_to_plugin(inner: &Arc<Inner>) {
    let prefs = inner.prefs.get();
    let dictation = &prefs.dictation_hotkey;
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
    let prefs = inner.prefs.get();
    let qa_trigger = prefs
        .qa_hotkey
        .as_ref()
        .and_then(crate::shortcut_binding::legacy_modifier_trigger);
    let translation_trigger = if is_builtin_translation_shift(&prefs.translation_hotkey) {
        None
    } else {
        crate::shortcut_binding::legacy_modifier_trigger(&prefs.translation_hotkey)
    };
    let selection_polish_trigger = prefs
        .selection_polish_hotkey
        .as_ref()
        .and_then(crate::shortcut_binding::legacy_modifier_trigger);
    (qa_trigger, selection_polish_trigger, translation_trigger)
}

pub(super) fn mark_translation_modifier_seen(inner: &Arc<Inner>) {
    let phase = inner.state.lock().phase;
    if matches!(phase, SessionPhase::Starting | SessionPhase::Listening) {
        inner
            .translation_modifier_seen
            .store(true, Ordering::SeqCst);
        log::info!("[coord] translation modifier seen during {phase:?}");
    }
}

pub(super) fn hotkey_bridge_loop(inner: Arc<Inner>, rx: mpsc::Receiver<HotkeyEvent>) {
    while let Ok(evt) = rx.recv() {
        if inner.shortcut_recording_active.load(Ordering::SeqCst) {
            continue;
        }
        let inner_cloned = Arc::clone(&inner);
        match evt {
            // P0 #468/#475: Pressed/Released 必须串行处理，否则在 Windows 上 WH_KEYBOARD_LL
            // 边沿间隔微秒级 → 两个独立 spawn 的 task 被 work-stealing 调度器并行执行 →
            // `hotkey_trigger_held` latch 翻转顺序错乱 → 下次按键被静默吞掉
            // (UI 关不掉 / 录音停不下来)。改为 bridge 线程内 block_on 顺序 await，
            // recv 的 FIFO 顺序就是 handler 执行顺序。
            // 注意：handle_pressed_edge / handle_released_edge 内部走 .await（含网络
            // 握手），会暂时阻塞本 bridge 线程；Hold 模式短按时 Released 会排队在 channel
            // 里直到 begin_session 完成，但 SessionPhase::Starting 已经有
            // request_stop_during_starting 兜底，begin_session 完成进 Listening 后
            // bridge 立刻 recv Released → end_session，行为正确，仅有短暂 stop 延迟。
            HotkeyEvent::Pressed { at, press_id } => {
                async_runtime::block_on(async {
                    handle_pressed_edge(&inner_cloned, at, press_id).await;
                });
            }
            HotkeyEvent::Released { at } => {
                async_runtime::block_on(async {
                    handle_released_edge(&inner_cloned, at).await;
                });
            }
            // Esc 取消与组合键撤销都不在此枚举里：分别走 esc_cancel_bridge_loop /
            // combo_abort_bridge_loop，避免被上面 Released → end_session /
            // Pressed → begin_session 的同步流程堵在队列里（见各自函数注释）。
            HotkeyEvent::TranslationModifierPressed => {
                let translation_hotkey = inner_cloned.prefs.get().translation_hotkey;
                if is_builtin_translation_shift(&translation_hotkey)
                    || crate::shortcut_binding::legacy_modifier_trigger(&translation_hotkey)
                        .is_some()
                {
                    mark_translation_modifier_seen(&inner_cloned);
                }
            }
            HotkeyEvent::QaShortcutPressed => {
                async_runtime::block_on(async {
                    handle_qa_hotkey_pressed(&inner_cloned).await;
                });
            }
            #[cfg(not(mobile))]
            HotkeyEvent::SelectionPolishShortcutPressed => {
                let coordinator = Coordinator {
                    inner: Arc::clone(&inner_cloned),
                };
                // Selection Polish has no paired release edge. Run the cloud
                // workflow independently so its network wait cannot stall the
                // shared modifier-key bridge (Esc, dictation, QA, etc.).
                async_runtime::spawn(async move {
                    if let Err(error) = coordinator.trigger_selection_polish().await {
                        log::warn!("[selection-polish] hotkey workflow failed: {error}");
                    }
                });
            }
        }
    }
}

pub(super) fn reset_shortcut_held_state(inner: &Arc<Inner>) {
    inner.hotkey_trigger_held.store(false, Ordering::SeqCst);
    if let Some(monitor) = inner.hotkey.lock().as_ref() {
        monitor.reset_held_state();
    }
    let prefs = inner.prefs.get();
    if let Some(binding) = prefs.qa_hotkey.as_ref() {
        if crate::shortcut_binding::legacy_modifier_trigger(binding).is_none() {
            if let Some(monitor) = inner.qa_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(binding.clone()) {
                    log::warn!("[coord] reset QA hotkey latch failed: {e}");
                }
            }
        }
    }
    if !is_builtin_translation_shift(&prefs.translation_hotkey)
        && crate::shortcut_binding::legacy_modifier_trigger(&prefs.translation_hotkey).is_none()
    {
        if let Some(monitor) = inner.translation_hotkey.lock().as_ref() {
            if let Err(e) = monitor.update_binding(prefs.translation_hotkey.clone()) {
                log::warn!("[coord] reset translation hotkey latch failed: {e}");
            }
        }
    }
    if let Some(switch_style) = prefs.switch_style_hotkey.as_ref() {
        if !is_modifier_only_shortcut(switch_style) {
            if let Some(monitor) = inner.switch_style_hotkey.lock().as_ref() {
                if let Err(e) = monitor.update_binding(switch_style.clone()) {
                    log::warn!("[coord] reset switch-style hotkey latch failed: {e}");
                }
            }
        }
    }
    if let Some(open_app) = prefs.open_app_hotkey.as_ref() {
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
        let qa_active = {
            let st = inner.qa_state.lock();
            st.panel_visible || st.phase != QaPhase::Idle
        };
        if qa_active {
            close_qa_panel(inner);
        } else {
            cancel_session(inner);
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

        let Some(trigger) =
            crate::shortcut_binding::legacy_modifier_trigger(&inner.prefs.get().dictation_hotkey)
        else {
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
                handle_pressed_edge(inner, std::time::Instant::now(), 0).await;
            }
            "keyup" => {
                log::info!("[window-hotkey] released trigger={trigger:?} code={code}");
                handle_released_edge(inner, std::time::Instant::now()).await;
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
pub(super) fn window_key_matches_trigger(trigger: crate::types::HotkeyTrigger, key: &str, code: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 构造一个处于 Processing 阶段、cancelled=false 的 Coordinator。
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
        let handle = std::thread::spawn(move || esc_cancel_bridge_loop(bridge_inner, rx));
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
