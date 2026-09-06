#![cfg_attr(
    target_os = "linux",
    allow(dead_code, unused_imports, unused_variables)
)]
//! Dictation coordinator.
//!
//! Mirrors the Swift `DictationCoordinator` state machine. Single owner of
//! session state. Receives hotkey edges, drives recorder + ASR + polish +
//! insertion, persists history, emits `capsule:state` events to the capsule
//! window.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

#[cfg(target_os = "windows")]
use crate::asr::local::{FoundryLocalRuntime, SherpaOnnxRuntime};
use crate::combo_hotkey::{ComboHotkeyError, ComboHotkeyEvent, ComboHotkeyMonitor};
use crate::hotkey::{HotkeyEvent, HotkeyMonitor};
use crate::insertion::TextInserter;
use crate::persistence::{
    ActivityStore, CorrectionRuleStore, DictionaryStore, HistoryStore, PreferencesStore,
    StylePackStore,
};
use crate::qa_adapter::TauriQaHostContext;

use crate::qa_hotkey::{QaHotkeyError, QaHotkeyEvent, QaHotkeyMonitor};
use crate::types::{
    CapsulePayload, CapsuleState, CapsuleStyle, HotkeyCapability, HotkeyStatus, HotkeyStatusState,
};

mod capsule_focus;
#[path = "coordinator/dictation_core.rs"]
mod dictation;
mod hotkey_loops;
mod qa;
#[cfg(all(not(mobile), target_os = "windows"))]
pub(crate) mod selection_voice_session;
use capsule_focus::*;
pub(crate) use capsule_focus::{
    capture_external_focus_target, capture_focus_target, capture_frontmost_app,
    restore_focus_target_if_possible,
};
use hotkey_loops::*;

// Instance-local Less Computer replay source used by the compatibility command.
pub(crate) use dictation::{less_computer_event_replay_after, LessComputerEventReplay};

pub(super) fn qa_event_target() -> &'static str {
    #[cfg(target_os = "android")]
    {
        "main"
    }
    #[cfg(not(target_os = "android"))]
    {
        "qa"
    }
}

#[cfg(any(debug_assertions, test))]
use dictation::{handle_pressed, handle_released};
use dictation::{handle_pressed_edge, handle_released_edge, handle_trigger_combined};
use qa::handle_qa_hotkey_pressed;

/// 词条建议卡片的窗口尺寸（逻辑点）。
///
/// 显示卡片时必须把胶囊窗口缩到这个大小 —— 见 [`show_vocab_suggestion_card`] 里关于
/// 鼠标穿透的说明。
const VOCAB_CARD_WIDTH: f64 = 320.0;
/// 一行建议的高度：勾叉按钮 28pt + 行间距 8pt，与 `VocabSuggestionCard.tsx` 对齐。
const VOCAB_CARD_ROW_HEIGHT: f64 = 36.0;
/// 标题行 + 卡片内边距 + 留给投影的外边距。
const VOCAB_CARD_CHROME_HEIGHT: f64 = 72.0;
/// 卡片离屏幕右边缘留多少。
const VOCAB_CARD_EDGE_MARGIN: f64 = 24.0;

/// 把「要不要记住这个词」的卡片弹到胶囊那个位置。
///
/// 复用胶囊窗口而不是新开一个：多显示器定位、Space 贴附（macOS 26 上那个把窗口钉死在
/// 单个桌面的坑）、nonactivating panel 都是踩过坑才对的，重开一个窗口等于重踩一遍。
///
/// 但有一处必须动：**胶囊平时是鼠标完全穿透的**（`set_ignore_cursor_events(true)`），
/// 因为它浮在别的 app 上面，不能挡住用户点下面的东西。卡片要能点，就得临时关掉穿透；
/// 而透明窗口一旦不穿透，**连透明的部分也会拦鼠标**。所以显示卡片时把窗口缩到卡片实际
/// 大小，挡住的范围就只有卡片本身；收起时再恢复。
fn show_vocab_suggestion_card(inner: &Arc<Inner>) {
    let pending = inner.backend.pending_corrections();
    if pending.is_empty() {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let height = VOCAB_CARD_CHROME_HEIGHT + VOCAB_CARD_ROW_HEIGHT * pending.len() as f64;
    let inner_for_main = Arc::clone(inner);
    let _ = capsule.run_on_main_thread(move |capsule| {
        let inner = inner_for_main;
        // **最后一道闸：听写不在 Idle 就绝不弹卡片。**
        //
        // 上游那些判据（观察器代次、`pending_corrections` 是否为空）全都是「读一次再去
        // 干活」，读完到这里还隔着一次跨线程调度 —— 排队期间 Core 会话可能已开始。
        // 完全可能已经跑完：解除观察器、收起卡片、开启新一轮听写。那种 check-then-act
        // 无论怎么加都堵不住这一段。
        //
        // 判据放在这里才有意义：这是碰窗口之前的最后一个时点，而且问的是**真正的不变量**
        // —— 卡片和录音胶囊共用一个窗口，显示卡片要把窗口缩到卡片大小，在听写进行中弹
        // 出来就是把那次听写的胶囊弄没了（真机踩过，表现是「热键像是坏了」）。
        //
        if inner.backend.snapshot().dictation.phase != openless_core::DictationPhase::Idle
            || inner.backend.less_computer_active_session().is_some()
        {
            log::debug!("[vocab-card] suppressed: a dictation session is in flight");
            inner.backend.dismiss_pending_corrections();
            return;
        }
        inner.vocab_card_visible.store(true, Ordering::SeqCst);
        // 卡片是要点的，穿透必须关掉。
        // Android 没有胶囊窗口，tauri 的 set_ignore_cursor_events 在其上不存在
        //（与 capsule_focus.rs 里同一处理）。
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(false) {
            log::warn!("[vocab-card] set_ignore_cursor_events(false) failed: {e}");
        }
        if let Err(e) = capsule.set_size(VOCAB_CARD_WIDTH, height) {
            log::warn!("[vocab-card] resize failed: {e}");
        }
        if let Err(e) =
            capsule.position_vocab_card(VOCAB_CARD_WIDTH, height, VOCAB_CARD_EDGE_MARGIN)
        {
            log::warn!("[vocab-card] position failed: {e}");
        }
        // 位置同理：`maybe_position_capsule_bottom_center` 的去重缓存只记「显示器 +
        // 翻译态」，卡片这一挪它一无所知。不清掉的话，下一次录音时它会拿相同的
        // 显示器快照判定「没变化」→ 跳过重新定位 → 胶囊留在卡片挪过去的右下角。
        capsule.invalidate_layout();
        capsule.show_for_recording(true);
        #[cfg(target_os = "macos")]
        capsule.restore_main_window_key_if_active();
    });
}

/// 收起卡片：把窗口完整还给胶囊。
///
/// 四条路径都会走到这里 —— 用户点了「好」/「都不用」、10 秒到时、新一轮听写开始。
///
/// **没有卡片时必须原样返回。** 新听写会话会调它，如果无条件去
/// `hide()` 那个窗口，就会和 `emit_capsule` 的 show 抢同一个窗口 —— 胶囊时隐时不显，
/// 用户会以为热键坏了。
fn hide_vocab_suggestion_card(inner: &Arc<Inner>) {
    inner.backend.dismiss_pending_corrections();
    if !inner.vocab_card_visible.swap(false, Ordering::SeqCst) {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let _ = capsule.run_on_main_thread(move |capsule| {
        // 先隐藏再改几何：复原要同时动尺寸和位置，窗口还亮着时改就有概率被合成出
        // 一帧「卡片被拉宽、还横着飞过半个屏幕」。
        let _ = capsule.hide();
        // 穿透必须还回去，否则胶囊会一直挡着屏幕底部那一块。
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(true) {
            log::warn!("[vocab-card] restoring cursor passthrough failed: {e}");
        }
        // 尺寸也必须还回去 —— 卡片把窗口缩到过自己的大小，不复原的话下一次胶囊
        // 就挤在一个 320×108 的窗口里，等于看不见。
        let bounds = crate::capsule_window_bounds(false);
        if let Err(e) = capsule.set_size(bounds.width, bounds.height) {
            log::warn!("[vocab-card] restoring capsule size failed: {e}");
        }
        // 位置一样要还 —— 卡片把窗口挪到了右下角，胶囊的位置是底部居中。
        // 只还尺寸不还位置，下一次录音胶囊就出现在右下角（真机上就是这个 bug）。
        //
        // 清缓存和这次重定位是两件事，都要做：清缓存保证「就算这次重定位失败，
        // 下一次 emit_capsule 也一定会重算」，重定位保证「就算有哪条路径绕过了
        // emit_capsule 直接 show，窗口也已经在对的地方」。
        capsule.invalidate_layout();
        if let Err(e) = capsule.position_capsule_bottom_center(false) {
            log::warn!("[vocab-card] restoring capsule position failed: {e}");
        }
    });
}

/// 兜底卡片的窗口宽度（逻辑点）。比词条卡片宽一点 —— 这张要放一整段话。
const FALLBACK_CARD_WIDTH: f64 = 360.0;
/// Webview 首次渲染前的安全高度。真实高度由卡片 DOM 测量后通过 IPC 回报。
const FALLBACK_CARD_INITIAL_HEIGHT: f64 = 260.0;
/// 尺寸 IPC 的原生安全边界，不表达任何 CSS 布局规则。
const FALLBACK_CARD_MIN_HEIGHT: f64 = 96.0;
const FALLBACK_CARD_MAX_HEIGHT: f64 = 320.0;

fn validated_fallback_card_height(
    active_presentation_id: Option<u64>,
    presentation_id: u64,
    height: f64,
) -> Result<Option<f64>, String> {
    if !height.is_finite() {
        return Err("fallback card height must be finite".into());
    }
    if active_presentation_id != Some(presentation_id) {
        return Ok(None);
    }
    Ok(Some(
        height
            .ceil()
            .clamp(FALLBACK_CARD_MIN_HEIGHT, FALLBACK_CARD_MAX_HEIGHT),
    ))
}

/// 文本没能落到目标 app 时，把它连同一个复制按钮弹出来。
///
/// 为什么需要这张卡片：这些场景下唯一的兜底是「把文本写进剪贴板」，而它既依赖一个
/// 默认可关的开关，用户也**根本不知道文本在剪贴板里** —— 没有任何提示。屏幕上要么
/// 什么都没有，要么只有半截。
///
/// 窗口机制整套照搬 [`show_vocab_suggestion_card`]（复用胶囊窗口、关穿透、缩尺寸、
/// 右下角定位），理由见那里。多的一件事是 `insert_fallback_card_visible`：这张卡片
/// 在会话收尾那一刻弹出，而收尾自己安排了一次 `schedule_capsule_idle` → `hide()`，
/// 必须让那次 hide 认得出卡片并让路。
fn show_insert_fallback_card(inner: &Arc<Inner>, text: String, reason: &'static str) {
    if text.trim().is_empty() {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let inner_for_main = Arc::clone(inner);
    let _ = capsule.run_on_main_thread(move |capsule| {
        let inner = inner_for_main;
        // 与词条卡片同一道闸、同一理由：听写不在 Idle 就绝不碰这个窗口，否则等于把
        // 正在进行的那次听写的胶囊弄没了。收尾路径是先把 phase 置回 Idle 再走到这里的。
        if inner.backend.snapshot().dictation.phase != openless_core::DictationPhase::Idle
            || inner.backend.less_computer_active_session().is_some()
        {
            log::debug!("[fallback-card] suppressed: a dictation session is in flight");
            return;
        }
        let presentation_id = inner.host.begin_insert_fallback_card();
        let payload = crate::types::InsertFallbackCardPayload {
            text,
            reason: reason.to_string(),
            presentation_id,
        };
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(false) {
            log::warn!("[fallback-card] set_ignore_cursor_events(false) failed: {e}");
        }
        if let Err(e) = capsule.set_size(FALLBACK_CARD_WIDTH, FALLBACK_CARD_INITIAL_HEIGHT) {
            log::warn!("[fallback-card] resize failed: {e}");
        }
        if let Err(e) =
            capsule.position_fallback_card(FALLBACK_CARD_WIDTH, FALLBACK_CARD_INITIAL_HEIGHT)
        {
            log::warn!("[fallback-card] position failed: {e}");
        }
        // 位置同理：`maybe_position_capsule_bottom_center` 的去重缓存只记「显示器 +
        // 翻译态」，卡片这一挪它一无所知。不清掉的话下一次录音会判定「没变化」→
        // 跳过重新定位 → 胶囊留在卡片挪过去的右下角。
        capsule.invalidate_layout();
        inner.host.emit_insert_fallback(&payload);
        capsule.show_for_recording(true);
        #[cfg(target_os = "macos")]
        capsule.restore_main_window_key_if_active();
        log::info!(
            "[fallback-card] shown: reason={reason} chars={}",
            payload.text.chars().count()
        );
    });
}

fn report_insert_fallback_card_height(
    inner: &Arc<Inner>,
    presentation_id: u64,
    height: f64,
) -> Result<(), String> {
    let active_presentation_id = inner.host.active_insert_fallback_presentation_id();
    let Some(height) =
        validated_fallback_card_height(active_presentation_id, presentation_id, height)?
    else {
        return Ok(());
    };
    let Some(capsule) = inner.host.capsule_window() else {
        return Ok(());
    };
    let inner_for_main = Arc::clone(inner);
    capsule.run_on_main_thread(move |capsule| {
        if !inner_for_main
            .host
            .insert_fallback_presentation_is_current(presentation_id)
        {
            return;
        }
        if let Err(e) = capsule.set_size(FALLBACK_CARD_WIDTH, height) {
            log::warn!("[fallback-card] measured resize failed: {e}");
        }
        if let Err(e) = capsule.position_fallback_card(FALLBACK_CARD_WIDTH, height) {
            log::warn!("[fallback-card] measured position failed: {e}");
        }
    })
}

/// 收起兜底卡片：把窗口完整还给胶囊。
///
/// 与 [`hide_vocab_suggestion_card`] 同款：**没有卡片时必须原样返回**，否则每次听写
/// 开始都会去 hide 那个窗口，和 `emit_capsule` 的 show 抢。
fn hide_insert_fallback_card(inner: &Arc<Inner>) {
    let _event_guard = inner.capsule_event_lock.lock();
    let (was_visible, deferred_capsule) = inner.host.dismiss_insert_fallback_card();
    if !was_visible {
        return;
    }
    let Some(capsule) = inner.host.capsule_window() else {
        return;
    };
    let host = inner.host.clone();
    let backend = Arc::clone(&inner.backend);
    let _ = capsule.run_on_main_thread(move |capsule| {
        host.clear_insert_fallback();
        // 先隐藏再改几何：复原要同时动尺寸和位置，窗口还亮着时改就有概率被合成出
        // 一帧「卡片被拉宽、还横着飞过半个屏幕」。
        let _ = capsule.hide();
        // 穿透必须还回去，否则胶囊会一直挡着屏幕那一块。
        #[cfg(not(mobile))]
        if let Err(e) = capsule.set_cursor_passthrough(true) {
            log::warn!("[fallback-card] restoring cursor passthrough failed: {e}");
        }
        // 尺寸也必须还回去 —— 卡片把窗口缩到过自己的大小，不复原的话下一次胶囊
        // 就挤在一个卡片大小的窗口里，等于看不见。
        let bounds = crate::capsule_window_bounds(false);
        if let Err(e) = capsule.set_size(bounds.width, bounds.height) {
            log::warn!("[fallback-card] restoring capsule size failed: {e}");
        }
        // 位置一样要还 —— 卡片把窗口挪到了右下角，胶囊的位置是底部居中。只还尺寸
        // 不还位置，下一次录音胶囊就出现在右下角（词条卡片在真机上踩过这个 bug）。
        // 清缓存和这次重定位两件都要做，理由见 `hide_vocab_suggestion_card`。
        capsule.invalidate_layout();
        if let Err(e) = capsule.position_capsule_bottom_center(false) {
            log::warn!("[fallback-card] restoring capsule position failed: {e}");
        }
        if let Some(payload) = deferred_capsule {
            // 卡片期间 QA / Selection Polish 仍会推进胶囊状态，只是不能碰共享窗口。
            // 卡片释放后把最新状态一次性应用回来；若最新是 Idle，该 helper 会正常隐藏。
            let preferences = backend.get_preferences();
            let show_capsule = payload.selection_polish || preferences.show_capsule;
            let classic_style = matches!(preferences.capsule_style, CapsuleStyle::Classic);
            capsule.apply_capsule_payload(&payload, show_capsule, classic_style, true);
        }
    });
}

pub struct Coordinator {
    inner: Arc<Inner>,
}

fn shared_backend_from_stores(
    history: &HistoryStore,
    activity: &ActivityStore,
    prefs: &PreferencesStore,
    style_packs: &StylePackStore,
    vocab: &DictionaryStore,
    correction_rules: &CorrectionRuleStore,
    app: crate::core_adapters::AppHandleSlot,
    native_asr: crate::core_adapters::TauriNativeAsrDependencies,
    hotkey_status: Arc<Mutex<HotkeyStatus>>,
    qa_context: Arc<TauriQaHostContext>,
) -> Arc<openless_core::OpenLessBackend> {
    let data_dir = crate::persistence::data_dir().unwrap_or_else(|error| {
        log::warn!("[core] data directory unavailable, using fallback config path: {error}");
        std::env::temp_dir().join("openless-core-fallback")
    });
    let locale = std::env::var("LANG")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "en-US".to_string());
    let repositories = openless_core::BackendRepositories {
        preferences: prefs.core(),
        history: history.core(),
        activity: activity.core(),
        vocabulary: vocab.core(),
        correction_rules: correction_rules.core(),
        style_packs: style_packs.core(),
    };
    let backend_slot = crate::core_adapters::backend_slot();
    let mut dependencies = crate::core_adapters::backend_dependencies(
        app,
        Arc::clone(&backend_slot),
        native_asr,
        Arc::clone(&repositories.preferences),
        hotkey_status,
        qa_context,
    );
    dependencies.marketplace_config = Some(openless_core::MarketplaceConfig::production());
    let backend = Arc::new(
        openless_core::OpenLessBackend::new_with_repositories(
            openless_core::BackendConfig {
                cache_dir: data_dir.join("cache"),
                data_dir,
                home_dir: std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(std::path::PathBuf::from),
                resource_dir: None,
                platform: crate::types::PlatformCapabilities::current(),
                locale,
            },
            dependencies,
            repositories,
        )
        .expect("shared backend config always has a non-empty data directory"),
    );
    *backend_slot.lock() = Some(Arc::downgrade(&backend));
    backend
}

/// Install the only narrow callback the QA runtime needs from the Tauri host:
/// attaching an opaque selection insertion target to a Core-owned preview.
/// The callback only captures the shared opaque-target state; the QA adapter
/// never performs a Tauri managed-state lookup back into `Coordinator`.
#[cfg(all(not(mobile), target_os = "windows"))]
fn bind_qa_selection_voice_target(
    qa_context: &Arc<TauriQaHostContext>,
    selection_voice_host: &Arc<Mutex<selection_voice_session::SelectionVoiceHostState>>,
) {
    let selection_voice_host = Arc::clone(selection_voice_host);
    qa_context.set_selection_voice_target_binder(Arc::new(move |session_id, target| {
        selection_voice_session::bind_selection_voice_target_state(
            &selection_voice_host,
            session_id,
            target,
        )
    }));
}

struct StylePackHotkeyRegistration {
    binding: crate::types::ShortcutBinding,
    _monitor: ComboHotkeyMonitor,
}

struct Inner {
    host: crate::tauri_coordinator_host::TauriCoordinatorHost,
    backend: Arc<openless_core::OpenLessBackend>,
    less_computer_voice: Mutex<Option<LessComputerHostCapture>>,
    /// 实际安装在宿主上的快捷键目标。设置事务只通过显式 target 更新这里，
    /// 监听器安装/恢复不得回读尚未提交或已回滚的 preferences。
    hotkey_runtime_target: Mutex<openless_core::HotkeyRuntimeTarget>,
    /// 串行化 Tauri 侧“Core 设置事务 + 宿主 effect”以及风格包删除 effect，
    /// 防止两个命令把显式 runtime target 乱序安装。
    settings_host_gate: Mutex<()>,
    inserter: TextInserter,
    /// 建议卡片是不是正占着胶囊窗口。
    ///
    /// 门控 `hide_vocab_suggestion_card`：没有卡片时它必须什么都不做，否则每次听写
    /// 开始都会去 hide 胶囊窗口，和 `emit_capsule` 的 show 抢同一个窗口。
    vocab_card_visible: AtomicBool,
    hotkey: Mutex<Option<HotkeyMonitor>>,
    hotkey_status: Arc<Mutex<HotkeyStatus>>,
    /// Webview fallback only: pairs one raw keydown/up edge with the same Core press id.
    window_hotkey_press_id: AtomicU64,
    shortcut_recording_active: AtomicBool,
    /// Less Computer modifier 热键的按下代次与待处理组合键事件。
    less_computer_press_generation: AtomicU64,
    less_computer_combo_pending_press: Mutex<Option<crate::hotkey::HotkeyCombinedEdge>>,
    /// 自定义组合键监听器（global-hotkey crate）。当 `prefs.hotkey.trigger == Custom` 时
    /// 代替 modifier-only 的 hotkey monitor。`None` 表示不使用自定义组合键或还没成功安装。
    combo_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    side_aware_combo: Mutex<Option<crate::side_aware_combo::SideAwareComboMonitor>>,
    translation_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    switch_style_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    open_app_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    /// 风格包直达快捷键监听器（issue #759）：pack_id → 实际绑定 + monitor。
    /// 绑定元数据让 supervisor 能区分「同一 pack_id 但按键已变化」，并在任何
    /// 非事务设置路径注册失败后继续重试到实际状态与 prefs 一致。
    style_pack_hotkeys: Mutex<std::collections::HashMap<String, StylePackHotkeyRegistration>>,
    /// 选区润色快捷键：modifier-only 复用 `HotkeyMonitor`，其它组合键复用
    /// `ComboHotkeyMonitor`。桌面（非 mobile）专属。
    #[cfg(not(mobile))]
    selection_polish_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    /// 选区语音宿主资源。业务 session/prompt/preview 由 openless-core 独占。
    #[cfg(all(not(mobile), target_os = "windows"))]
    selection_voice_host: Arc<Mutex<selection_voice_session::SelectionVoiceHostState>>,
    #[cfg(all(not(mobile), target_os = "windows"))]
    selection_voice_capture: Mutex<Option<Arc<openless_core::VoiceTranscriptionSession>>>,
    /// 划词语音问答（issue #118）：与 dictation hotkey 平行的全局快捷键
    /// 监听器（global-hotkey crate）。`None` 表示功能关闭或还没成功安装。
    qa_hotkey: Mutex<Option<QaHotkeyMonitor>>,
    coding_agent_modifier_hotkey: Mutex<Option<HotkeyMonitor>>,
    coding_agent_combo_hotkey: Mutex<Option<ComboHotkeyMonitor>>,
    /// 最近一次 emit_capsule 下发的 state，纯内省/测试用途（在 app 句柄校验之前写入，
    /// 因此无 GUI 的测试环境也能断言「按下热键 → 弹了哪种胶囊」）。写入是单次廉价
    /// 加锁，对 ~30Hz 录音回调可忽略。
    last_capsule_state: Mutex<Option<CapsuleState>>,
    /// 每次 capsule payload 递增。选区润色的终态自动隐藏会带上该代数，防止旧 timer
    /// 覆盖新的选区润色/语音/QA 可见状态。
    capsule_event_epoch: AtomicU64,
    /// 将 capsule 事件与自动隐藏线性化。这样一个旧 timer 要么在新的 payload 之前收起
    /// 旧提示，要么发现代数已改变直接放弃，绝不会在新会话之后补发 Idle。
    capsule_event_lock: Mutex<()>,
    /// 选区润色的轻量提示仍在显示或处理中。已有语音/QA 的旧 auto-hide timer 必须在
    /// 此期间让路，避免把选区润色浮窗提前收掉。
    selection_polish_capsule_active: AtomicBool,
    /// Tauri QA window visibility. All QA business state belongs to openless-core.
    qa_context: Arc<TauriQaHostContext>,
    /// 预备态标志：按下热键即"乐观显示"胶囊（带入场动画），此时麦克风还在 cpal
    /// init 窗口内、没有第一帧 PCM。为 true 时 emit_capsule 把 Recording payload 的
    /// `warming` 打成 true（前端渲染"待命"光效）；`level_handler` 首次触发（PCM 真的
    /// 流入）后置 false，光条"点亮"进入正式录音。begin_session 每次入场重置为 true。
    /// Coordinator 退出信号。各 hotkey supervisor loop 在每轮重试 sleep 之前会检查
    /// 此 flag；为 true 时 loop 立刻 return。生产场景里 process exit 一并 reap 所有
    /// supervisor 线程，但 integration test 和未来 RunEvent::Exit 钩子需要这条
    /// 显式退出路径。审计 3.1.2。
    shutdown: AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionHotkeyKind {
    SwitchStyle,
    OpenApp,
}

impl Coordinator {
    #[cfg(mobile)]
    pub(crate) fn bind_selection_voice_target(
        &self,
        _session_id: openless_core::SessionId,
        _insertion_target: crate::selection::SelectionInsertionTarget,
    ) -> Result<(), String> {
        Err("selectionVoiceTargetUnavailable".to_string())
    }

    pub fn new() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::new_with_local_runtimes(
                Arc::new(FoundryLocalRuntime::new()),
                Arc::new(SherpaOnnxRuntime::new()),
            )
        }

        #[cfg(not(target_os = "windows"))]
        {
            #[cfg(target_os = "android")]
            const PERSIST_DEGRADE_SUFFIX: &str = " (Android 禁止 /data/local/tmp)";
            #[cfg(not(target_os = "android"))]
            const PERSIST_DEGRADE_SUFFIX: &str = "";

            let history = HistoryStore::new().unwrap_or_else(|e| {
                log::error!(
                    "[coord] HistoryStore init failed: {e}; 降级为空历史记录{PERSIST_DEGRADE_SUFFIX}"
                );
                HistoryStore::new_fallback()
            });
            let prefs = PreferencesStore::new().unwrap_or_else(|e| {
                log::error!(
                    "[coord] PreferencesStore init failed: {e}; 降级为默认偏好设置{PERSIST_DEGRADE_SUFFIX}"
                );
                PreferencesStore::new_fallback()
            });
            // 启动即同步系统代理开关（issue #869），让首个请求就按用户设置建客户端。
            crate::net::set_use_system_proxy(prefs.get().use_system_proxy);
            let style_packs = StylePackStore::new(&prefs).unwrap_or_else(|e| {
                log::error!(
                    "[coord] StylePackStore init failed: {e}; 降级为空样式包列表{PERSIST_DEGRADE_SUFFIX}"
                );
                StylePackStore::new_fallback()
            });
            let vocab = DictionaryStore::new().unwrap_or_else(|e| {
                log::error!(
                    "[coord] DictionaryStore init failed: {e}; 降级为空词库{PERSIST_DEGRADE_SUFFIX}"
                );
                DictionaryStore::new_fallback()
            });
            let correction_rules = CorrectionRuleStore::new().unwrap_or_else(|e| {
                log::error!(
                    "[coord] CorrectionRuleStore init failed: {e}; 降级为空纠错规则{PERSIST_DEGRADE_SUFFIX}"
                );
                CorrectionRuleStore::new_fallback()
            });

            let activity = ActivityStore::load().unwrap_or_else(|e| {
                log::error!("[coord] ActivityStore init failed: {e}; 活动计数降级为内存态");
                ActivityStore::new_fallback()
            });

            let app = crate::core_adapters::app_handle_slot();
            let native_asr = crate::core_adapters::TauriNativeAsrDependencies::new();
            let hotkey_status = Arc::new(Mutex::new(HotkeyStatus::default()));
            let qa_context = Arc::new(TauriQaHostContext::default());
            let backend = shared_backend_from_stores(
                &history,
                &activity,
                &prefs,
                &style_packs,
                &vocab,
                &correction_rules,
                Arc::clone(&app),
                native_asr.clone(),
                Arc::clone(&hotkey_status),
                Arc::clone(&qa_context),
            );

            let host = crate::tauri_coordinator_host::TauriCoordinatorHost::new(Arc::clone(&app));
            let hotkey_runtime_target = (&backend.get_preferences()).into();
            let inner = Arc::new(Inner {
                host,
                backend,
                less_computer_voice: Mutex::new(None),
                hotkey_runtime_target: Mutex::new(hotkey_runtime_target),
                settings_host_gate: Mutex::new(()),
                inserter: TextInserter::new(),
                vocab_card_visible: AtomicBool::new(false),
                hotkey: Mutex::new(None),
                hotkey_status,
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
                #[cfg(not(mobile))]
                selection_polish_hotkey: Mutex::new(None),
                #[cfg(all(not(mobile), target_os = "windows"))]
                selection_voice_host: Arc::new(Mutex::new(
                    selection_voice_session::SelectionVoiceHostState::default(),
                )),
                #[cfg(all(not(mobile), target_os = "windows"))]
                selection_voice_capture: Mutex::new(None),
                qa_hotkey: Mutex::new(None),
                coding_agent_modifier_hotkey: Mutex::new(None),
                coding_agent_combo_hotkey: Mutex::new(None),
                last_capsule_state: Mutex::new(None),
                capsule_event_epoch: AtomicU64::new(0),
                capsule_event_lock: Mutex::new(()),
                selection_polish_capsule_active: AtomicBool::new(false),
                qa_context: Arc::clone(&qa_context),
                shutdown: AtomicBool::new(false),
            });
            #[cfg(all(not(mobile), target_os = "windows"))]
            bind_qa_selection_voice_target(&qa_context, &inner.selection_voice_host);
            Self { inner }
        }
    }

    /// 保留旧构造函数：现有调用点（含单元测试）只传 Foundry runtime。
    /// sherpa-onnx runtime 这里创建默认 offline batch 实例；入产后（lib.rs）请走
    /// `new_with_local_runtimes`，确保 Tauri State 共享同一个 Arc。
    #[cfg(target_os = "windows")]
    pub fn new_with_foundry_runtime(foundry_local_runtime: Arc<FoundryLocalRuntime>) -> Self {
        Self::new_with_local_runtimes(foundry_local_runtime, Arc::new(SherpaOnnxRuntime::new()))
    }

    #[cfg(target_os = "windows")]
    pub fn new_with_local_runtimes(
        foundry_local_runtime: Arc<FoundryLocalRuntime>,
        sherpa_onnx_runtime: Arc<SherpaOnnxRuntime>,
    ) -> Self {
        let history = HistoryStore::new().unwrap_or_else(|e| {
            log::error!("[coord] HistoryStore init failed: {e}; 降级为空历史记录");
            HistoryStore::new_fallback()
        });
        let prefs = PreferencesStore::new().unwrap_or_else(|e| {
            log::error!("[coord] PreferencesStore init failed: {e}; 降级为默认偏好设置");
            PreferencesStore::new_fallback()
        });
        // 启动即同步系统代理开关（issue #869），让首个请求就按用户设置建客户端。
        crate::net::set_use_system_proxy(prefs.get().use_system_proxy);
        let style_packs = StylePackStore::new(&prefs).unwrap_or_else(|e| {
            log::error!("[coord] StylePackStore init failed: {e}; 降级为空样式包列表");
            StylePackStore::new_fallback()
        });
        let vocab = DictionaryStore::new().unwrap_or_else(|e| {
            log::error!("[coord] DictionaryStore init failed: {e}; 降级为空词库");
            DictionaryStore::new_fallback()
        });
        let correction_rules = CorrectionRuleStore::new().unwrap_or_else(|e| {
            log::error!("[coord] CorrectionRuleStore init failed: {e}; 降级为空纠错规则");
            CorrectionRuleStore::new_fallback()
        });

        let activity = ActivityStore::load().unwrap_or_else(|e| {
            log::error!("[coord] ActivityStore init failed: {e}; 活动计数降级为内存态");
            ActivityStore::new_fallback()
        });

        let app = crate::core_adapters::app_handle_slot();
        let hotkey_status = Arc::new(Mutex::new(HotkeyStatus::default()));
        let selection_voice_host = Arc::new(Mutex::new(
            selection_voice_session::SelectionVoiceHostState::default(),
        ));
        let qa_context = Arc::new(TauriQaHostContext::default());
        let backend = shared_backend_from_stores(
            &history,
            &activity,
            &prefs,
            &style_packs,
            &vocab,
            &correction_rules,
            Arc::clone(&app),
            crate::core_adapters::TauriNativeAsrDependencies::new(
                Arc::clone(&foundry_local_runtime),
                Arc::clone(&sherpa_onnx_runtime),
            ),
            Arc::clone(&hotkey_status),
            Arc::clone(&qa_context),
        );

        let host = crate::tauri_coordinator_host::TauriCoordinatorHost::new(Arc::clone(&app));
        let hotkey_runtime_target = (&backend.get_preferences()).into();
        let inner = Arc::new(Inner {
            host,
            backend,
            less_computer_voice: Mutex::new(None),
            hotkey_runtime_target: Mutex::new(hotkey_runtime_target),
            settings_host_gate: Mutex::new(()),
            inserter: TextInserter::new(),
            vocab_card_visible: AtomicBool::new(false),
            hotkey: Mutex::new(None),
            hotkey_status,
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
            #[cfg(not(mobile))]
            selection_polish_hotkey: Mutex::new(None),
            #[cfg(all(not(mobile), target_os = "windows"))]
            selection_voice_host: Arc::clone(&selection_voice_host),
            #[cfg(all(not(mobile), target_os = "windows"))]
            selection_voice_capture: Mutex::new(None),
            qa_hotkey: Mutex::new(None),
            coding_agent_modifier_hotkey: Mutex::new(None),
            coding_agent_combo_hotkey: Mutex::new(None),
            last_capsule_state: Mutex::new(None),
            capsule_event_epoch: AtomicU64::new(0),
            capsule_event_lock: Mutex::new(()),
            selection_polish_capsule_active: AtomicBool::new(false),
            qa_context: Arc::clone(&qa_context),
            shutdown: AtomicBool::new(false),
        });
        bind_qa_selection_voice_target(&qa_context, &selection_voice_host);
        Self { inner }
    }

    pub fn backend(&self) -> Arc<openless_core::OpenLessBackend> {
        Arc::clone(&self.inner.backend)
    }

    pub fn show_core_insert_fallback(&self, text: String, reason: &str) {
        let reason = match reason {
            "partial_stream" => crate::types::INSERT_FALLBACK_REASON_PARTIAL_STREAM,
            _ => crate::types::INSERT_FALLBACK_REASON_INSERT_FAILED,
        };
        show_insert_fallback_card(&self.inner, text, reason);
    }

    pub fn present_core_capsule(&self, payload: CapsulePayload) {
        let _ = self.present_core_capsule_if_current(payload, None);
    }

    pub(crate) fn present_core_capsule_if_current(
        &self,
        payload: CapsulePayload,
        expected_epoch: Option<u64>,
    ) -> Option<u64> {
        let state = payload.state;
        // Core 已拥有本帧的翻译、准备态和会话归属；不可在窗口层按迟到的
        // 当前状态重新拼装，否则冷启动或快速切换会丢失真实反馈。
        let epoch = emit_core_capsule(&self.inner, payload, expected_epoch)?;
        if let Some(delay_ms) = core_capsule_hide_delay(state) {
            let inner = Arc::clone(&self.inner);
            self.inner.host.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                hide_core_capsule_if_current(&inner, epoch);
            });
        }
        Some(epoch)
    }

    pub(crate) fn tauri_host(&self) -> crate::tauri_coordinator_host::TauriCoordinatorHost {
        self.inner.host.clone()
    }

    pub fn android_insert_strategy(&self) -> crate::types::AndroidInsertStrategy {
        self.inner.backend.get_preferences().android_insert_strategy
    }

    pub fn android_overlay_trigger(&self) -> crate::types::AndroidOverlayTrigger {
        self.inner
            .backend
            .get_preferences()
            .android_overlay_trigger
            .normalized()
    }

    pub fn apply_android_overlay_settings_change(
        &self,
        previous: &crate::types::UserPreferences,
        next: &crate::types::UserPreferences,
    ) {
        #[cfg(target_os = "android")]
        {
            use crate::types::android_types::{
                classify_android_overlay_settings_change, AndroidOverlaySettingsAction,
            };
            match classify_android_overlay_settings_change(previous, next) {
                AndroidOverlaySettingsAction::None => {}
                AndroidOverlaySettingsAction::RefreshLayout => {
                    self.refresh_android_overlay_layout();
                }
                AndroidOverlaySettingsAction::Transition { from, to } => {
                    self.transition_android_overlay_trigger(from, to);
                }
            }
        }
        let _ = (previous, next);
    }

    pub fn transition_android_overlay_trigger(
        &self,
        from: crate::types::AndroidOverlayTrigger,
        to: crate::types::AndroidOverlayTrigger,
    ) {
        #[cfg(target_os = "android")]
        {
            use crate::types::AndroidOverlayTrigger;
            fn overlay_trigger_log_name(trigger: AndroidOverlayTrigger) -> &'static str {
                match trigger.normalized() {
                    AndroidOverlayTrigger::Background => "background",
                    AndroidOverlayTrigger::Keyboard => "keyboard",
                    AndroidOverlayTrigger::Always => "always",
                }
            }
            if from == to {
                return;
            }
            log::info!(
                "[coord] overlay transition from={} to={}",
                overlay_trigger_log_name(from),
                overlay_trigger_log_name(to),
            );
            match (from, to) {
                (
                    AndroidOverlayTrigger::Background | AndroidOverlayTrigger::Keyboard,
                    AndroidOverlayTrigger::Always,
                ) => {
                    let _ = crate::android::replace_android_overlay();
                }
                (
                    AndroidOverlayTrigger::Always,
                    AndroidOverlayTrigger::Background | AndroidOverlayTrigger::Keyboard,
                ) => {
                    let _ = crate::android::hide_android_overlay();
                }
                _ => {}
            }
        }
        let _ = (from, to);
    }

    pub fn apply_android_overlay_on_startup(&self) {
        #[cfg(target_os = "android")]
        {
            use crate::types::AndroidOverlayTrigger;
            match self.android_overlay_trigger() {
                AndroidOverlayTrigger::Always => {
                    let _ = crate::android::replace_android_overlay();
                }
                AndroidOverlayTrigger::Background | AndroidOverlayTrigger::Keyboard => {
                    let _ = crate::android::hide_android_overlay();
                }
            }
        }
    }

    pub fn refresh_android_overlay_layout(&self) {
        #[cfg(target_os = "android")]
        {
            let _ = crate::android::refresh_android_overlay_layout();
        }
    }

    /// 让所有 hotkey supervisor loop（dictation / qa / combo / translation /
    /// switch_style / open_app / style_pack / selection_polish）在下一轮 sleep / poll
    /// 后退出。生产场景下进程退出
    /// 一并 reap 所有线程，但 integration test 和未来 RunEvent::Exit 钩子需要
    /// 显式退出路径。审计 3.1.2。
    #[allow(dead_code)]
    pub fn request_shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn start_hotkey_listener(&self) {
        // 起一个守护线程，反复尝试安装 hotkey hook。Accessibility 一被授予就立即生效，
        // 用户不需要手动重启 OpenLess。
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-hotkey-supervisor".into())
            .spawn(move || hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_hotkey_listener(&self) {
        self.inner.hotkey.lock().take();
    }

    /// 启动 QA hotkey supervisor（issue #118）。和 `start_hotkey_listener` 平行：
    /// 守护线程反复尝试注册（用户可能改了组合键），失败则 3s 后重试。
    pub fn start_qa_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-qa-hotkey-supervisor".into())
            .spawn(move || qa_hotkey_supervisor_loop(inner))
            .ok();
    }

    /// 启动「快速 Agent」双热键 supervisor。与 QA hotkey 平行；功能默认关闭，
    /// 仅在 `coding_agent_enabled` 时注册。
    pub fn start_coding_agent_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-coding-agent-hotkey-supervisor".into())
            .spawn(move || coding_agent_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_coding_agent_hotkey_listener(&self) {
        take_coding_agent_hotkeys_on_main_thread(&self.inner);
    }

    pub(crate) fn update_coding_agent_hotkey_binding(&self) -> Result<(), String> {
        update_coding_agent_hotkey_binding_now(&self.inner)
    }

    pub fn stop_qa_hotkey_listener(&self) {
        // QaHotkeyMonitor::drop 在 macOS 底层是 Carbon RemoveEventHotKey，要求主线程。
        // RunEvent::Exit 回调不保证在 AppKit 主线程跑，drop 漏到 tokio worker 上会
        // 触发 macOS dispatch_assert_queue_fail SIGTRAP。包到 run_on_main_thread 让
        // drop 在主线程发生；AppHandle 已 None 时直接 drop（最坏 crash 也是退出时刻）。
        // 详见 issue #169。
        let inner = Arc::clone(&self.inner);
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                inner.qa_hotkey.lock().take();
            })
            .is_err()
        {
            self.inner.qa_hotkey.lock().take();
        }
    }

    #[cfg(not(mobile))]
    pub fn start_selection_polish_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-selection-polish-hotkey-supervisor".into())
            .spawn(move || selection_polish_hotkey_supervisor_loop(inner))
            .ok();
    }

    #[cfg(not(mobile))]
    pub fn stop_selection_polish_hotkey_listener(&self) {
        take_selection_polish_hotkey_on_main_thread(&self.inner);
    }

    #[cfg(not(mobile))]
    pub(crate) fn try_update_selection_polish_hotkey_binding(&self) -> Result<(), String> {
        try_update_selection_polish_hotkey_binding(&self.inner)
    }

    #[cfg(not(mobile))]
    pub(crate) fn update_selection_polish_hotkey_binding(&self) {
        if let Err(error) = self.try_update_selection_polish_hotkey_binding() {
            log::warn!("[coord] update selection polish hotkey binding failed: {error}");
        }
    }

    /// 启动自定义组合键监听器。当 `prefs.hotkey.trigger == Custom` 时，
    /// 代替 modifier-only 的 hotkey monitor。
    pub fn start_combo_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-combo-hotkey-supervisor".into())
            .spawn(move || combo_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_combo_hotkey_listener(&self) {
        take_combo_hotkey_on_main_thread(&self.inner);
    }

    pub fn start_translation_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-translation-hotkey-supervisor".into())
            .spawn(move || translation_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_translation_hotkey_listener(&self) {
        take_translation_hotkey_on_main_thread(&self.inner);
    }

    pub fn start_switch_style_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-switch-style-hotkey-supervisor".into())
            .spawn(move || action_hotkey_supervisor_loop(inner, ActionHotkeyKind::SwitchStyle))
            .ok();
    }

    pub fn stop_switch_style_hotkey_listener(&self) {
        take_action_hotkey_on_main_thread(&self.inner, ActionHotkeyKind::SwitchStyle);
    }

    pub fn start_open_app_hotkey_listener(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-open-app-hotkey-supervisor".into())
            .spawn(move || action_hotkey_supervisor_loop(inner, ActionHotkeyKind::OpenApp))
            .ok();
    }

    pub fn stop_open_app_hotkey_listener(&self) {
        take_action_hotkey_on_main_thread(&self.inner, ActionHotkeyKind::OpenApp);
    }

    /// 启动风格包直达快捷键监听（issue #759）。supervisor 线程等 AppHandle 就绪后
    /// 按 prefs 全量注册，个别注册失败按 action hotkey 的节奏重试。
    pub fn start_style_pack_hotkey_listeners(&self) {
        let inner = Arc::clone(&self.inner);
        std::thread::Builder::new()
            .name("openless-style-pack-hotkey-supervisor".into())
            .spawn(move || style_pack_hotkey_supervisor_loop(inner))
            .ok();
    }

    pub fn stop_style_pack_hotkey_listeners(&self) {
        clear_style_pack_hotkeys_on_main_thread(&self.inner);
    }

    /// 用户在设置里改了风格快捷键列表时调用：按最新 prefs 全量对齐注册状态。
    pub(crate) fn update_style_pack_hotkey_bindings(&self) {
        sync_style_pack_hotkeys_on_main_thread(&self.inner);
    }

    /// 事务式设置路径使用：等待主线程完成整表注册并返回精确失败原因。
    pub(crate) fn try_update_style_pack_hotkey_bindings(&self) -> Result<(), String> {
        try_sync_style_pack_hotkeys_on_main_thread(&self.inner)
    }

    /// 用户在设置里改了自定义组合键时调用。
    pub(crate) fn update_combo_hotkey_binding(&self) {
        let target = hotkey_runtime_target(&self.inner);
        if crate::shortcut_binding::legacy_modifier_trigger(&target.dictation).is_some() {
            take_combo_hotkey_on_main_thread(&self.inner);
            self.inner.side_aware_combo.lock().take();
            log::info!("[coord] combo hotkey 已关闭（modifier-only）");
            return;
        }
        let binding = target.dictation;
        if is_unconfigured_shortcut(&binding) {
            take_combo_hotkey_on_main_thread(&self.inner);
            self.inner.side_aware_combo.lock().take();
            log::info!("[coord] combo hotkey 已关闭（无绑定）");
            return;
        }

        if crate::shortcut_binding::binding_requires_side_aware_hook(&binding) {
            take_combo_hotkey_on_main_thread(&self.inner);
            self.inner.side_aware_combo.lock().take();
            let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
            match crate::side_aware_combo::SideAwareComboMonitor::start(binding, tx) {
                Ok(monitor) => {
                    *self.inner.side_aware_combo.lock() = Some(monitor);
                    let bridge_inner = Arc::clone(&self.inner);
                    std::thread::Builder::new()
                        .name("openless-side-combo-bridge".into())
                        .spawn(move || combo_hotkey_bridge_loop(bridge_inner, rx))
                        .ok();
                    log::info!("[coord] side-aware combo hotkey listener installed (via update)");
                }
                Err(e) => {
                    log::warn!("[coord] update side-aware combo binding 失败: {e}");
                }
            }
            return;
        }

        self.inner.side_aware_combo.lock().take();
        let inner_clone = Arc::clone(&self.inner);
        let binding_for_main = binding.clone();
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                if let Some(monitor) = inner_clone.combo_hotkey.lock().as_ref() {
                    if let Err(e) = monitor.update_binding(binding_for_main.clone()) {
                        log::warn!("[coord] update combo hotkey binding 失败: {e}");
                    }
                    return;
                }
                let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
                match ComboHotkeyMonitor::start(binding_for_main, tx) {
                    Ok(monitor) => {
                        *inner_clone.combo_hotkey.lock() = Some(monitor);
                        log::info!(
                            "[coord] combo hotkey listener installed on main thread (via update)"
                        );
                        let bridge_inner = Arc::clone(&inner_clone);
                        std::thread::Builder::new()
                            .name("openless-combo-hotkey-bridge".into())
                            .spawn(move || combo_hotkey_bridge_loop(bridge_inner, rx))
                            .ok();
                        #[cfg(target_os = "linux")]
                        sync_custom_dictation_to_plugin(&inner_clone);
                    }
                    Err(e) => {
                        log::warn!("[coord] update combo hotkey binding 失败: {e}");
                    }
                }
            })
            .is_err()
        {
            log::warn!("[coord] update combo hotkey binding: AppHandle 未 bind，跳过");
        }
    }

    /// 用户在设置里改了 QA 组合键时调用。先持久化（由 prefs.set 完成），
    /// 然后通知活着的 monitor 重新注册；monitor 不存在时 supervisor 会自然
    /// 在下一次循环里读到新的 prefs。
    pub(crate) fn update_qa_hotkey_binding(&self) {
        let target = hotkey_runtime_target(&self.inner);
        let Some(binding) = target.qa else {
            // 用户把功能关了 → 直接 drop monitor。drop 也得在主线程，否则 Carbon
            // unregister 会失败/UB。
            let inner_clone = Arc::clone(&self.inner);
            if self
                .inner
                .host
                .run_on_main_thread(move || {
                    inner_clone.qa_hotkey.lock().take();
                })
                .is_err()
            {
                self.inner.qa_hotkey.lock().take();
            }
            log::info!("[coord] QA hotkey 已关闭");
            self.update_modifier_shortcut_bindings();
            return;
        };
        if crate::shortcut_binding::legacy_modifier_trigger(&binding).is_some() {
            let inner_clone = Arc::clone(&self.inner);
            if self
                .inner
                .host
                .run_on_main_thread(move || {
                    inner_clone.qa_hotkey.lock().take();
                })
                .is_err()
            {
                self.inner.qa_hotkey.lock().take();
            }
            self.update_modifier_shortcut_bindings();
            log::info!("[coord] QA hotkey uses modifier-only listener");
            return;
        }
        self.update_modifier_shortcut_bindings();
        // global-hotkey crate 的 manager.register/unregister 必须主线程跑。
        // 没在主线程会让 Carbon 句柄注册看似成功但事件不派发。
        let inner_clone = Arc::clone(&self.inner);
        let binding_for_main = binding.clone();
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                // 路径 1：当前已有 monitor → 在主线程换绑定。
                if let Some(monitor) = inner_clone.qa_hotkey.lock().as_ref() {
                    if let Err(e) = monitor.update_binding(binding_for_main.clone()) {
                        log::warn!("[coord] update QA hotkey binding 失败: {e}");
                    }
                    return;
                }
                // 路径 2：之前还没装上 → 主线程上重装一次（supervisor 也会重试，
                // 但用户体感更快：set_qa_hotkey 命令一返回，hotkey 立即生效）。
                let (tx, rx) = mpsc::channel::<QaHotkeyEvent>();
                match QaHotkeyMonitor::start(binding_for_main, tx) {
                    Ok(monitor) => {
                        *inner_clone.qa_hotkey.lock() = Some(monitor);
                        log::info!(
                            "[coord] QA hotkey listener installed on main thread (via update)"
                        );
                        let bridge_inner = Arc::clone(&inner_clone);
                        std::thread::Builder::new()
                            .name("openless-qa-hotkey-bridge".into())
                            .spawn(move || qa_hotkey_bridge_loop(bridge_inner, rx))
                            .ok();
                    }
                    Err(e) => {
                        log::warn!("[coord] update QA hotkey binding 失败: {e}");
                    }
                }
            })
            .is_err()
        {
            log::warn!("[coord] update QA hotkey binding: AppHandle 未 bind，跳过");
        }
    }

    pub(crate) fn update_translation_hotkey_binding(&self) {
        if let Err(e) = self.try_update_translation_hotkey_binding() {
            log::warn!("[coord] update translation hotkey binding 失败: {e}");
        }
    }

    pub(crate) fn try_update_translation_hotkey_binding(&self) -> Result<(), String> {
        let target = hotkey_runtime_target(&self.inner);
        if is_builtin_translation_shift(&target.translation)
            || crate::shortcut_binding::legacy_modifier_trigger(&target.translation).is_some()
        {
            take_translation_hotkey_on_main_thread(&self.inner);
            self.update_modifier_shortcut_bindings();
            log::info!("[coord] translation hotkey uses modifier-only listener");
            return Ok(());
        }
        self.update_modifier_shortcut_bindings();
        let inner_clone = Arc::clone(&self.inner);
        let binding_for_main = target.translation;
        let (result_tx, result_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        self.inner.host.run_on_main_thread(move || {
            let result = update_translation_hotkey_on_main_thread(inner_clone, binding_for_main);
            let _ = result_tx.send(result.map_err(|e| e.to_string()));
        })?;
        match result_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => Err("注册翻译快捷键超时".into()),
        }
    }

    pub(crate) fn update_switch_style_hotkey_binding(&self) {
        self.update_action_hotkey_binding(ActionHotkeyKind::SwitchStyle);
    }

    pub(crate) fn update_open_app_hotkey_binding(&self) {
        self.update_action_hotkey_binding(ActionHotkeyKind::OpenApp);
    }

    fn update_action_hotkey_binding(&self, kind: ActionHotkeyKind) {
        // None = 用户主动停用：反注册全局键，立即生效。
        let Some(binding) = action_hotkey_binding(&self.inner, kind) else {
            take_action_hotkey_on_main_thread(&self.inner, kind);
            log::info!("[coord] action hotkey {kind:?} 已停用（用户清空）");
            return;
        };
        if is_modifier_only_shortcut(&binding) {
            take_action_hotkey_on_main_thread(&self.inner, kind);
            log::warn!("[coord] action hotkey {kind:?} 使用了不支持的 modifier-only 绑定，已关闭");
            return;
        }

        let inner_clone = Arc::clone(&self.inner);
        if self
            .inner
            .host
            .run_on_main_thread(move || {
                if let Some(monitor) = action_hotkey_slot(&inner_clone, kind).lock().as_ref() {
                    if let Err(e) = monitor.update_binding(binding.clone()) {
                        log::warn!("[coord] update action hotkey {kind:?} binding 失败: {e}");
                    }
                    return;
                }
                let (tx, rx) = mpsc::channel::<ComboHotkeyEvent>();
                match ComboHotkeyMonitor::start(binding, tx) {
                    Ok(monitor) => {
                        *action_hotkey_slot(&inner_clone, kind).lock() = Some(monitor);
                        let bridge_inner = Arc::clone(&inner_clone);
                        std::thread::Builder::new()
                            .name(action_hotkey_bridge_thread_name(kind).into())
                            .spawn(move || action_hotkey_bridge_loop(bridge_inner, rx, kind))
                            .ok();
                    }
                    Err(e) => log::warn!("[coord] update action hotkey {kind:?} binding 失败: {e}"),
                }
            })
            .is_err()
        {
            log::warn!("[coord] update action hotkey binding: AppHandle 未 bind，跳过");
        }
    }

    /// 给前端 Settings 渲染当前 QA 快捷键 label（如 "Cmd+Shift+;"）。
    /// `qa_hotkey == None` 时返回空串，UI 据此显示「未启用」。
    pub fn qa_hotkey_label(&self) -> String {
        self.inner
            .backend
            .get_preferences()
            .qa_hotkey
            .as_ref()
            .map(|b| b.display_label())
            .unwrap_or_default()
    }

    /// 设置保存后立即把胶囊样式同步进 Tauri Host 缓存。
    /// emit_capsule 的 ~30Hz 主线程闭包本来也会同步，但入场帧的 payload 是在闭包
    /// 同步之前克隆的（会带一帧旧样式），且 Windows 上主线程拥塞时闭包可能延迟
    /// 执行——用户反馈「切换成默认风格后仍显示流光 Siri」。在保存路径直接同步后，
    /// 任何平台的下一次录音从入场帧起就携带最新样式，不再依赖 emit 闭包的时序。
    pub fn sync_capsule_style_from_preferences(&self) {
        self.inner
            .host
            .cache_capsule_style(self.inner.backend.get_preferences().capsule_style);
    }
    /// Apply only the Tauri presentation side of the Core-owned vocabulary
    /// suggestion state. Commands mutate the Core collection first, then pass
    /// this narrow boolean to the host so Coordinator never owns or rewrites
    /// the suggestion business state.
    pub(crate) fn refresh_vocab_suggestion_presentation(&self, has_pending: bool) {
        if has_pending {
            show_vocab_suggestion_card(&self.inner);
        } else {
            hide_vocab_suggestion_card(&self.inner);
        }
    }

    /// 落字失败兜底卡片自己关掉了（用户点关闭 / TTL 到时）。
    pub fn dismiss_insert_fallback_card(&self) {
        hide_insert_fallback_card(&self.inner);
    }

    pub fn report_insert_fallback_card_height(
        &self,
        presentation_id: u64,
        height: f64,
    ) -> Result<(), String> {
        report_insert_fallback_card_height(&self.inner, presentation_id, height)
    }

    pub(crate) fn update_hotkey_binding(&self) {
        let target = hotkey_runtime_target(&self.inner);
        let dictation_trigger = crate::shortcut_binding::legacy_modifier_trigger(&target.dictation);
        let binding = crate::types::HotkeyBinding {
            trigger: dictation_trigger.unwrap_or(crate::types::HotkeyTrigger::Custom),
            mode: target.dictation_mode,
            keys: None,
        };
        if dictation_trigger.is_some() {
            take_combo_hotkey_on_main_thread(&self.inner);
        } else {
            self.update_combo_hotkey_binding();
        }
        self.ensure_modifier_hotkey_monitor(binding);
        self.update_modifier_shortcut_bindings();
    }

    fn ensure_modifier_hotkey_monitor(&self, binding: crate::types::HotkeyBinding) {
        if let Some(monitor) = self.inner.hotkey.lock().as_ref() {
            #[cfg(target_os = "linux")]
            let plugin_binding = binding.clone();
            monitor.update_binding(binding);
            #[cfg(target_os = "linux")]
            if plugin_binding.trigger == crate::types::HotkeyTrigger::Custom {
                sync_custom_dictation_to_plugin(&self.inner);
            } else {
                crate::linux_fcitx::sync_binding_to_plugin(&plugin_binding);
            }
            return;
        }
        let (tx, rx) = mpsc::channel::<HotkeyEvent>();
        #[cfg(target_os = "linux")]
        let (fcitx_tx, fcitx_binding) = (tx.clone(), binding.clone());
        let cancel_tx = spawn_esc_cancel_bridge(&self.inner);
        let combo_tx = spawn_combo_abort_bridge(&self.inner, handle_trigger_combined);
        #[cfg(target_os = "linux")]
        let combo_tx_for_fcitx = combo_tx.clone();
        match HotkeyMonitor::start(binding, tx, cancel_tx, combo_tx) {
            Ok(monitor) => {
                let adapter = monitor.kind();
                *self.inner.hotkey.lock() = Some(monitor);
                *self.inner.hotkey_status.lock() = HotkeyStatus {
                    adapter,
                    state: HotkeyStatusState::Installed,
                    message: Some(format!("{} 已安装", adapter.display_name())),
                    last_error: None,
                };
                let inner_clone = Arc::clone(&self.inner);
                std::thread::Builder::new()
                    .name("openless-hotkey-bridge".into())
                    .spawn(move || hotkey_bridge_loop(inner_clone, rx))
                    .ok();
                // Linux: 启动 fcitx5 插件信号监听作为热键源。
                #[cfg(target_os = "linux")]
                {
                    let (qa_trigger, selection_polish_trigger, translation_trigger) =
                        modifier_shortcut_triggers(&self.inner);
                    let custom_key = custom_dictation_key_string(&self.inner);
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
                        sync_custom_dictation_to_plugin(&self.inner);
                    } else {
                        crate::linux_fcitx::sync_binding_to_plugin(&fcitx_binding);
                    }
                }
            }
            Err(e) => {
                *self.inner.hotkey_status.lock() = HotkeyStatus {
                    adapter: HotkeyMonitor::capability().adapter,
                    state: HotkeyStatusState::Failed,
                    message: Some(e.message.clone()),
                    last_error: Some(e),
                };
            }
        }
    }

    pub fn update_modifier_shortcut_bindings(&self) {
        if let Some(monitor) = self.inner.hotkey.lock().as_ref() {
            let (qa_trigger, selection_polish_trigger, translation_trigger) =
                modifier_shortcut_triggers(&self.inner);
            monitor.update_modifier_shortcuts(
                qa_trigger,
                selection_polish_trigger,
                translation_trigger,
            );
        }
    }

    /// 将 Core 已校验并完成冲突协调的显式目标应用到宿主监听器。
    ///
    /// 本方法不会读取 preferences；失败时 target 保持为 `next`，由 Core 根据
    /// receipt 调用反向 change 恢复，从而让部分安装也能收敛回旧状态。
    pub(crate) fn apply_hotkey_runtime_change(
        &self,
        change: &openless_core::SettingsValueChange<openless_core::HotkeyRuntimeTarget>,
    ) -> Result<(), String> {
        let previous = &change.previous;
        let next = &change.next;
        *self.inner.hotkey_runtime_target.lock() = next.clone();

        if previous.translation != next.translation {
            self.try_update_translation_hotkey_binding()?;
        }
        #[cfg(not(mobile))]
        if previous.selection_polish != next.selection_polish {
            self.try_update_selection_polish_hotkey_binding()?;
        }
        if previous.style_packs != next.style_packs {
            self.try_update_style_pack_hotkey_bindings()?;
        }
        if previous.dictation != next.dictation || previous.dictation_mode != next.dictation_mode {
            self.update_hotkey_binding();
        }
        if previous.dictation != next.dictation {
            self.update_combo_hotkey_binding();
        }
        if previous.qa != next.qa {
            self.update_qa_hotkey_binding();
        }
        if previous.switch_style != next.switch_style {
            self.update_switch_style_hotkey_binding();
        }
        if previous.open_app != next.open_app {
            self.update_open_app_hotkey_binding();
        }
        if previous.coding_agent_enabled != next.coding_agent_enabled
            || previous.coding_agent_voice != next.coding_agent_voice
        {
            // 旧键被注销后不会再有 Released；只取消其尚在采集的 Less 会话。
            // Agent 已处理的任务已取走 capture，不在这个 slot 中。
            cancel_less_computer_capture(&self.inner, None);
            self.update_coding_agent_hotkey_binding()?;
        }
        Ok(())
    }

    pub(crate) fn lock_settings_host(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.inner.settings_host_gate.lock()
    }

    pub(crate) async fn dismiss_less_computer(&self) -> Result<(), String> {
        // 先结束当前对话身份，随后精确释放Host capture/Core run；隐藏窗口不等于停麦。
        self.inner.backend.services().less_computer.dismiss();
        // 立即隐藏。若清理期间用户重新开启窗口，其show epoch会撤销这次退出动画。
        self.inner.host.hide_less_computer();
        let result = cancel_active_less_computer(&self.inner).await;
        result.map(|_| ()).map_err(|error| error.to_string())
    }

    pub(crate) async fn stop_less_computer_recording(&self) -> Result<bool, String> {
        let starting = {
            let slot = self.inner.less_computer_voice.lock();
            match slot.as_ref() {
                Some(LessComputerHostCapture::Starting(id, control)) => {
                    Some((*id, control.clone()))
                }
                _ => None,
            }
        };
        if let Some((id, control)) = starting {
            // 胶囊停止与静音停止共用交接队列，冷启动期间点击不能被吞掉。
            return control
                .request(id, openless_core::RecordingControlAction::Stop)
                .map(|_| true)
                .map_err(|error| error.to_string());
        }
        finish_less_computer_voice_session(&self.inner, None)
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn cancel_active_voice(&self) {
        dictation::cancel_active_session(&self.inner).await;
    }

    pub(crate) async fn cancel_dictation_from_cli(
        &self,
    ) -> Result<openless_core::CliDispatchOutcome, openless_core::BackendError> {
        // CLI取消与1.x主session范围一致：先释放Less Host slot，之后只委托主听写取消。
        // 不复用全局Esc的QA/Selection分支，以免扩大既有CLI命令的作用域。
        if cancel_active_less_computer(&self.inner).await? {
            return Ok(openless_core::CliDispatchOutcome::DictationCancelled);
        }
        self.inner
            .backend
            .dispatch_cli_intent(crate::cli::CliIntent::CancelDictation)
            .await
    }

    pub fn hotkey_capability(&self) -> HotkeyCapability {
        HotkeyMonitor::capability()
    }

    pub fn switch_to_previous_style_pack(&self) {
        switch_to_previous_style(&self.inner);
    }

    pub async fn open_qa_from_overlay(&self) -> Result<(), String> {
        log::info!("[coord] overlay QA open requested");
        self.inner
            .backend
            .services()
            .qa
            .show()
            .await
            .map_err(|error| error.message)?;
        self.inner
            .backend
            .services()
            .qa
            .toggle_recording()
            .await
            .map_err(|error| error.message)
    }

    pub async fn finalize_qa_from_overlay(&self) -> Result<(), String> {
        log::info!("[coord] overlay QA finalize requested");
        self.inner
            .backend
            .services()
            .qa
            .toggle_recording()
            .await
            .map_err(|error| error.message)
    }

    /// CLI 入口的 QA toggle：直接复用 modifier-only QA 热键边沿的处理函数。
    /// 与 `handle_qa_hotkey_pressed` 同语义 — Idle → 开浮窗 / Recording → 收尾 /
    /// Processing → 忽略。桌面快捷键 → CLI 转发的备用进入点。
    pub async fn cli_toggle_qa_panel(&self) {
        handle_qa_hotkey_pressed(&self.inner).await;
    }

    pub fn set_shortcut_recording_active(&self, active: bool) {
        self.inner
            .shortcut_recording_active
            .store(active, Ordering::SeqCst);
        // 同步给热键监听器：录制态激活时 CGEventTap 上报 Fn 按下边沿，
        // 供前端 ShortcutRecorder 提交 Fn 绑定（浏览器不向网页层下发 Fn keydown）。
        #[cfg(not(mobile))]
        let sync_ok = self.inner.hotkey.lock().as_ref().map(|m| {
            m.set_recording_active(active);
            true
        });
        #[cfg(mobile)]
        let sync_ok = None;
        #[cfg(not(mobile))]
        if let Some(monitor) = self.inner.coding_agent_modifier_hotkey.lock().as_ref() {
            monitor.set_recording_active(active);
            if active {
                monitor.reset_held_state();
            }
        }
        if active {
            reset_shortcut_held_state(&self.inner);
        }
        log::info!(
            "[coord] shortcut recording active={active} (synced_to_hotkey={})",
            sync_ok.unwrap_or(false)
        );
    }

    pub async fn handle_window_hotkey_event(
        &self,
        event_type: String,
        key: String,
        code: String,
        repeat: bool,
    ) -> Result<(), String> {
        handle_window_hotkey_event(&self.inner, event_type, key, code, repeat).await
    }

    #[cfg(any(debug_assertions, test))]
    pub async fn inject_hotkey_click_for_dev(&self) -> Result<(), String> {
        log::info!("[coord] dev hotkey injection started");
        let press_id = crate::hotkey::next_press_id();
        handle_pressed(&self.inner, std::time::Instant::now(), press_id).await;
        handle_released(&self.inner, std::time::Instant::now(), press_id).await;
        let _ = self.inner.backend.cancel_dictation(None).await;
        Ok(())
    }
}

const CAPSULE_AUTO_HIDE_DELAY_MS: u64 = 2000;

/// Core 终态事件只描述语义，原生胶囊保持多久由 Tauri Host 负责。把映射集中成纯函数，
/// 可以明确锁定成功、失败、取消三条时序，并让测试不依赖真实窗口与计时器。
fn core_capsule_hide_delay(state: CapsuleState) -> Option<u64> {
    match state {
        CapsuleState::Done | CapsuleState::Error => Some(CAPSULE_AUTO_HIDE_DELAY_MS),
        CapsuleState::Cancelled => Some(0),
        _ => None,
    }
}

fn schedule_capsule_idle(inner: &Arc<Inner>, delay_ms: u64) {
    let expected = inner.last_capsule_state.lock().as_ref().copied();
    let inner = Arc::clone(inner);
    let spawner = inner.host.clone();
    spawner.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if inner.last_capsule_state.lock().as_ref().copied() == expected {
            hide_capsule_if_all_sessions_idle(&inner);
        }
    });
}

#[cfg(test)]
mod core_capsule_tests {
    use super::*;

    #[test]
    fn core_terminal_capsule_timing_preserves_the_legacy_contract() {
        assert_eq!(core_capsule_hide_delay(CapsuleState::Done), Some(2000));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Error), Some(2000));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Cancelled), Some(0));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Recording), None);
    }
}

#[cfg(not(mobile))]
fn schedule_selection_polish_capsule_idle(inner: &Arc<Inner>, epoch: u64, delay_ms: u64) {
    let inner = Arc::clone(inner);
    let spawner = inner.host.clone();
    spawner.spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        hide_selection_polish_capsule_if_current(&inner, epoch);
    });
}
