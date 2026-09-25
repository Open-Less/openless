//! 我们自己的窗口有焦点时，fcitx5 插件收不到按键 —— 本地兜底匹配的**判定层**。
//!
//! 插件把热键匹配挂在 fcitx5 的 `EventType::InputContextKeyEvent` 上
//! （`scripts/linux-fcitx5-plugin/openless.cpp`），而 fcitx5 只有在聚焦的客户端
//! 注册了 text-input 时才会收到那个客户端的按键。我们的 winit/egui 窗口从不注册
//! （`ime_allowed` 默认 false → 不会调用 `zwp_text_input_v3.enable()`），于是
//! **主窗口、选区助手、Less Computer 面板有焦点时插件一个信号都不会发**：用户
//! 看到的就是「快捷键完全没反应」「选区助手开始录音后停不下来」。
//!
//! 这个模块只放**不依赖 egui** 的部分：边沿类型、命中判定、与插件信号的去重。
//! 读 egui 输入的那一半在窗口进程里（`main.rs` 的 `ui::local_hotkeys`），因为
//! `ui/` 属于可执行文件而不是这个库。
//!
//! 判定刻意复用既有通路，避免出现第二套键名表：命中用
//! `crate::settings::shortcut_to_raw`（注册给插件的同一个 `(keysym, states)` 换算）。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use openless_core::shared_types::ShortcutBinding;
use openless_core::HotkeyRuntimeTarget;
use serde::{Deserialize, Serialize};

use crate::hotkeys::LinuxHotkeyEvent;

/// 本地命中的热键。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocalHotkey {
    Dictation,
    Qa,
    QuickNote,
    Translation,
    SwitchStyle,
    SelectionPolish,
    OpenApp,
    LessComputer,
    /// 风格包热键带 **pack id**（不是下标）：窗口进程与宿主各自持有的
    /// `HotkeyRuntimeTarget` 可能来自不同时刻，用下标会指错包。
    StylePack(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalHotkeyEdgeKind {
    /// 组合键按下（与插件 `*KeyEvent` 的 press 同一语义）。
    Pressed,
    /// 组合键松开；`press_id` 与对应的 `Pressed` 相同。
    Released,
    /// 一次完整的按下+松开：裸修饰键绑定只能在松手时判定。
    Combined,
    /// 裸修饰键按下后又按了其他键；取消这次修饰键热键。
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalHotkeyEdge {
    pub hotkey: LocalHotkey,
    pub kind: LocalHotkeyEdgeKind,
    pub press_id: u64,
}

/// 本地 `press_id` 的起始值。
///
/// 宿主自己也用 `hotkeys::next_press_id()` 给插件信号配号，两者都从 1 开始会撞号 ——
/// Core 用 `press_id` 配对按下/松开，撞号会把 A 的松开记到 B 的按下上。把本地号段
/// 抬到远高于一次会话可能产生的信号数，两个号段就永远不会重叠。
pub const LOCAL_PRESS_ID_BASE: u64 = 1 << 40;

static NEXT_LOCAL_PRESS_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(LOCAL_PRESS_ID_BASE);

/// 分配一个本地边沿号（窗口进程各自从 [`LOCAL_PRESS_ID_BASE`] 起递增）。
pub fn next_local_press_id() -> u64 {
    NEXT_LOCAL_PRESS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// 本地命中与插件信号之间的去重窗口。
///
/// 两个来源可能对**同一次物理按键**各报一次：窗口是 XWayland 客户端时 fcitx5
/// 同样收得到键。300ms 足够覆盖「同一次按键的两个来源」，又远小于用户有意连按
/// 两次的间隔。
pub const HOTKEY_DEDUPE_WINDOW: Duration = Duration::from_millis(300);

/// 命中哪个热键。比较用注册给插件的同一套 `(keysym, states)`，因此「左/右修饰键」
/// 「Shift 隐含的符号」「大小写」这些细节与插件判定完全一致。
///
/// 顺序即优先级：同一个组合被绑到多处时（Core 的冲突校验会拦，但配置可能来自旧
/// 版本），听写优先，其次选区助手，其余按注册顺序。
pub fn match_hotkey(
    target: &HotkeyRuntimeTarget,
    pressed: &ShortcutBinding,
) -> Option<LocalHotkey> {
    // 这次按键对应的 `(keysym, states)`；裸修饰键名（egui 只给类别，`primary_keysym`
    // 未必认得每个拼写）允许为空，走下面的类别放宽。
    let pressed_raw = crate::settings::shortcut_to_raw(pressed)
        .ok()
        .filter(|raw| raw.0 != 0);
    if pressed_raw.is_none() && bare_modifier_category(&pressed.primary).is_none() {
        // 不支持的主键（egui 词表里有、X11 keysym 表里没有）：交给插件那条路。
        return None;
    }
    let candidates: [(Option<&ShortcutBinding>, LocalHotkey); 8] = [
        (Some(&target.dictation), LocalHotkey::Dictation),
        (target.qa.as_ref(), LocalHotkey::Qa),
        (target.quick_note.as_ref(), LocalHotkey::QuickNote),
        (
            target.selection_polish.as_ref(),
            LocalHotkey::SelectionPolish,
        ),
        (Some(&target.translation), LocalHotkey::Translation),
        (target.switch_style.as_ref(), LocalHotkey::SwitchStyle),
        (target.open_app.as_ref(), LocalHotkey::OpenApp),
        (
            target
                .coding_agent_voice
                .as_ref()
                .filter(|_| target.coding_agent_enabled),
            LocalHotkey::LessComputer,
        ),
    ];
    for (binding, hotkey) in candidates {
        let Some(binding) = binding else { continue };
        if binding_matches(binding, pressed, pressed_raw) {
            return Some(hotkey);
        }
    }
    for pack in &target.style_packs {
        if binding_matches(&pack.binding, pressed, pressed_raw) {
            return Some(LocalHotkey::StylePack(pack.pack_id.clone()));
        }
    }
    None
}

/// 风格包热键的 `(keysym, states)`：Core 按这两个值认包，所以本地命中后要带上
/// 与注册给插件时同源的换算结果。
pub fn style_pack_raw(target: &HotkeyRuntimeTarget, pack_id: &str) -> Option<(u32, u32)> {
    target
        .style_packs
        .iter()
        .find(|pack| pack.pack_id == pack_id)
        .and_then(|pack| crate::settings::shortcut_to_raw(&pack.binding).ok())
        .filter(|raw| raw.0 != 0)
}

/// 这条配置与这次按键算不算同一个热键。
fn binding_matches(
    binding: &ShortcutBinding,
    pressed: &ShortcutBinding,
    pressed_raw: Option<(u32, u32)>,
) -> bool {
    if let Some(pressed_raw) = pressed_raw {
        if crate::settings::shortcut_to_raw(binding).ok() == Some(pressed_raw) {
            return true;
        }
    }
    // 裸修饰键绑定的本地放宽：egui 的 `Modifiers` 只有类别、分不出左右，所以
    // 「按住右 Option 说话」这类绑定在窗口里只能按类别命中（插件那条路仍然
    // 区分左右 —— 这正是需要本地兜底时才放宽的理由）。
    if !binding.modifiers.is_empty() || !pressed.modifiers.is_empty() {
        return false;
    }
    match (
        bare_modifier_category(&binding.primary),
        bare_modifier_category(&pressed.primary),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// 裸修饰键主键名的类别（`None` = 不是裸修饰键）。
fn bare_modifier_category(primary: &str) -> Option<&'static str> {
    Some(match primary.trim().to_ascii_lowercase().as_str() {
        "alt" | "option" | "opt" | "leftalt" | "rightalt" | "altleft" | "altright"
        | "leftoption" | "rightoption" => "alt",
        "control" | "ctrl" | "leftcontrol" | "rightcontrol" | "ctrlleft" | "ctrlright" => "ctrl",
        "shift" | "leftshift" | "rightshift" | "shiftleft" | "shiftright" => "shift",
        "super" | "meta" | "win" | "cmd" | "command" | "leftsuper" | "rightsuper" | "leftmeta"
        | "rightmeta" | "leftcmd" | "rightcmd" => "super",
        _ => return None,
    })
}

/// 插件信号对应哪个热键（去重按热键身份对齐，而不是按原始 sym/states）。
pub fn plugin_event_hotkey(
    event: &LinuxHotkeyEvent,
    target: &HotkeyRuntimeTarget,
) -> Option<LocalHotkey> {
    Some(match event {
        LinuxHotkeyEvent::DictationPressed { .. }
        | LinuxHotkeyEvent::DictationReleased { .. }
        | LinuxHotkeyEvent::DictationCombined { .. } => LocalHotkey::Dictation,
        LinuxHotkeyEvent::QaPressed => LocalHotkey::Qa,
        LinuxHotkeyEvent::QuickNotePressed => LocalHotkey::QuickNote,
        LinuxHotkeyEvent::SelectionPolishPressed => LocalHotkey::SelectionPolish,
        LinuxHotkeyEvent::TranslationPressed { symbol, states } => {
            if !shortcut_event_matches(&target.translation, *symbol, *states) {
                return None;
            }
            LocalHotkey::Translation
        }
        LinuxHotkeyEvent::SwitchStylePressed => LocalHotkey::SwitchStyle,
        LinuxHotkeyEvent::OpenAppPressed => LocalHotkey::OpenApp,
        LinuxHotkeyEvent::LessComputerPressed { .. }
        | LinuxHotkeyEvent::LessComputerReleased { .. }
        | LinuxHotkeyEvent::LessComputerCombined { .. } => LocalHotkey::LessComputer,
        LinuxHotkeyEvent::StylePackPressed { symbol, states } => {
            let raw = (*symbol, *states);
            target
                .style_packs
                .iter()
                .find(|pack| crate::settings::shortcut_to_raw(&pack.binding).ok() == Some(raw))
                .map(|pack| LocalHotkey::StylePack(pack.pack_id.clone()))?
        }
    })
}

/// Match a raw fcitx signal like `hotkey_match.h::matches`: fold letter case,
/// keep Ctrl/Alt/Super exact, and permit a shifted US symbol pair only when the
/// configured binding includes Shift.
fn shortcut_event_matches(binding: &ShortcutBinding, symbol: u32, states: u32) -> bool {
    let Ok((expected_symbol, expected_states)) = crate::settings::shortcut_to_raw(binding) else {
        return false;
    };
    const MODIFIERS: u32 = 0x01 | 0x04 | 0x08 | 0x40;
    let fold = |key: u32| {
        if (u32::from(b'a')..=u32::from(b'z')).contains(&key) {
            key - 32
        } else {
            key
        }
    };
    let actual = fold(symbol);
    let expected = fold(expected_symbol);
    if actual == expected {
        return states & MODIFIERS == expected_states & MODIFIERS;
    }
    if (0xffe1..=0xffee).contains(&actual) || (0xffe1..=0xffee).contains(&expected) {
        return false;
    }
    let shifted_pair = [
        (b';', b':'),
        (b',', b'<'),
        (b'.', b'>'),
        (b'/', b'?'),
        (b'\\', b'|'),
        (b'[', b'{'),
        (b']', b'}'),
        (b'\'', b'"'),
        (b'`', b'~'),
        (b'-', b'_'),
        (b'=', b'+'),
        (b'1', b'!'),
        (b'2', b'@'),
        (b'3', b'#'),
        (b'4', b'$'),
        (b'5', b'%'),
        (b'6', b'^'),
        (b'7', b'&'),
        (b'8', b'*'),
        (b'9', b'('),
        (b'0', b')'),
    ]
    .iter()
    .any(|(base, shifted)| {
        (actual == u32::from(*base) && expected == u32::from(*shifted))
            || (actual == u32::from(*shifted) && expected == u32::from(*base))
    });
    shifted_pair
        && expected_states & 0x01 != 0
        && states & (MODIFIERS & !0x01) == expected_states & (MODIFIERS & !0x01)
}

/// Build the same raw translation event emitted by the fcitx5 signal adapter.
/// This keeps the standalone host binary from reaching into the library's
/// private shortcut-conversion module.
pub fn translation_hotkey_event(target: &HotkeyRuntimeTarget) -> Option<LinuxHotkeyEvent> {
    let (symbol, states) = crate::settings::shortcut_to_raw(&target.translation).ok()?;
    Some(LinuxHotkeyEvent::TranslationPressed { symbol, states })
}

/// 本地边沿与插件信号之间的去重记账。
#[derive(Default)]
pub struct HotkeyDeduplicator {
    seen: HashMap<LocalHotkey, SourceStamps>,
}

#[derive(Default)]
struct SourceStamps {
    local: Option<Instant>,
    signal: Option<Instant>,
}

impl HotkeyDeduplicator {
    /// 采纳一条本地边沿？本地命中后 [`HOTKEY_DEDUPE_WINDOW`] 内到达的同键插件信号
    /// 会被丢弃，这里的判定方向相反：插件信号刚处理过，本地边沿不再重复触发。
    pub fn accept_local(&mut self, hotkey: &LocalHotkey, at: Instant) -> bool {
        let stamps = self.seen.entry(hotkey.clone()).or_default();
        stamps.local = Some(at);
        !stamps
            .signal
            .is_some_and(|signal| at.saturating_duration_since(signal) < HOTKEY_DEDUPE_WINDOW)
    }

    /// 采纳一条插件信号？本地刚命中过（同一窗口还是 XWayland 客户端）就丢弃。
    pub fn accept_signal(&mut self, hotkey: &LocalHotkey, at: Instant) -> bool {
        let stamps = self.seen.entry(hotkey.clone()).or_default();
        stamps.signal = Some(at);
        !stamps
            .local
            .is_some_and(|local| at.saturating_duration_since(local) < HOTKEY_DEDUPE_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::shared_types::{HotkeyMode, StylePackHotkey};

    fn binding(primary: &str, modifiers: &[&str]) -> ShortcutBinding {
        ShortcutBinding {
            primary: primary.to_string(),
            modifiers: modifiers.iter().map(|tag| tag.to_string()).collect(),
        }
    }

    fn target() -> HotkeyRuntimeTarget {
        HotkeyRuntimeTarget {
            dictation: binding("A", &["alt"]),
            dictation_mode: HotkeyMode::Toggle,
            qa: Some(binding(";", &["ctrl", "shift"])),
            quick_note: None,
            translation: binding("Z", &["alt"]),
            switch_style: Some(binding("S", &["ctrl", "shift"])),
            open_app: None,
            selection_polish: Some(binding("X", &["alt"])),
            coding_agent_enabled: true,
            coding_agent_voice: Some(binding("V", &["alt"])),
            style_packs: vec![StylePackHotkey {
                pack_id: "pack-1".to_string(),
                binding: binding("1", &["ctrl", "alt"]),
            }],
        }
    }

    #[test]
    fn a_configured_combo_matches_its_own_keysym_pair() {
        let target = target();
        assert_eq!(
            match_hotkey(&target, &binding("A", &["alt"])),
            Some(LocalHotkey::Dictation)
        );
        assert_eq!(
            match_hotkey(&target, &binding(";", &["ctrl", "shift"])),
            Some(LocalHotkey::Qa)
        );
        assert_eq!(
            match_hotkey(&target, &binding("V", &["alt"])),
            Some(LocalHotkey::LessComputer)
        );
        // 没绑的组合不命中。
        assert_eq!(match_hotkey(&target, &binding("B", &["alt"])), None);
        // 修饰键不同也不命中（Alt+Z 是翻译，不是听写）。
        assert_eq!(
            match_hotkey(&target, &binding("Z", &["alt"])),
            Some(LocalHotkey::Translation)
        );
        assert_eq!(match_hotkey(&target, &binding("A", &["ctrl"])), None);
    }

    #[test]
    fn implied_shift_matches_regardless_of_the_recorded_shift_tag() {
        // `primary_keysym(':')` 自己带 Shift 位，所以「: + Ctrl」与「: + Ctrl+Shift」
        // 会被算成同一个 (keysym, states) —— 与注册给插件的判定完全一致。
        let mut target = target();
        target.qa = Some(binding(":", &["ctrl"]));
        assert_eq!(
            match_hotkey(&target, &binding(":", &["ctrl"])),
            Some(LocalHotkey::Qa)
        );
        assert_eq!(
            match_hotkey(&target, &binding(":", &["ctrl", "shift"])),
            Some(LocalHotkey::Qa)
        );
    }

    #[test]
    fn a_bare_modifier_binding_matches_the_modifier_itself() {
        let mut target = target();
        target.dictation = binding("AltRight", &[]);
        assert_eq!(
            match_hotkey(&target, &binding("AltRight", &[])),
            Some(LocalHotkey::Dictation)
        );
        // egui 分不出左右：右 Option 的配置在窗口里按「alt 类」命中。
        assert_eq!(
            match_hotkey(&target, &binding("LeftAlt", &[])),
            Some(LocalHotkey::Dictation)
        );
        // 别的类别不算命中。
        assert_eq!(match_hotkey(&target, &binding("Shift", &[])), None);
        assert_eq!(match_hotkey(&target, &binding("A", &["alt"])), None);
    }

    #[test]
    fn style_pack_bindings_match_by_pack_id() {
        let target = target();
        assert_eq!(
            match_hotkey(&target, &binding("1", &["ctrl", "alt"])),
            Some(LocalHotkey::StylePack("pack-1".to_string()))
        );
    }

    #[test]
    fn a_disabled_less_computer_hotkey_is_not_matched() {
        let mut target = target();
        target.coding_agent_enabled = false;
        assert_eq!(match_hotkey(&target, &binding("V", &["alt"])), None);
    }

    #[test]
    fn plugin_events_map_to_the_same_hotkey_identity() {
        let target = target();
        let event = |event| plugin_event_hotkey(&event, &target);
        assert_eq!(
            event(LinuxHotkeyEvent::DictationPressed {
                symbol: 0x61,
                states: 8,
                press_id: 1,
                at: Instant::now(),
            }),
            Some(LocalHotkey::Dictation)
        );
        assert_eq!(event(LinuxHotkeyEvent::QaPressed), Some(LocalHotkey::Qa));
        let (symbol, states) = crate::settings::shortcut_to_raw(&binding("1", &["ctrl", "alt"]))
            .expect("style pack hotkey converts");
        assert_eq!(
            event(LinuxHotkeyEvent::StylePackPressed { symbol, states }),
            Some(LocalHotkey::StylePack("pack-1".to_string()))
        );
        // 不认识的 (keysym, states) 不冒充任何热键。
        assert_eq!(
            event(LinuxHotkeyEvent::StylePackPressed {
                symbol: 0xffff,
                states: 0
            }),
            None
        );
    }

    #[test]
    fn the_dedupe_window_swallows_the_other_source_in_both_directions() {
        let hotkey = LocalHotkey::Dictation;
        let start = Instant::now();
        let mut dedupe = HotkeyDeduplicator::default();

        // 本地先命中：紧随其后的插件信号被丢弃。
        assert!(dedupe.accept_local(&hotkey, start));
        assert!(!dedupe.accept_signal(&hotkey, start + Duration::from_millis(50)));
        // 窗口过后，插件信号重新被采纳。
        assert!(dedupe.accept_signal(
            &hotkey,
            start + HOTKEY_DEDUPE_WINDOW + Duration::from_millis(1)
        ));

        // 反方向：插件信号先到，本地边沿被丢弃。
        let later = start + Duration::from_secs(5);
        let mut dedupe = HotkeyDeduplicator::default();
        assert!(dedupe.accept_signal(&hotkey, later));
        assert!(!dedupe.accept_local(&hotkey, later + Duration::from_millis(10)));
        assert!(dedupe.accept_local(&hotkey, later + HOTKEY_DEDUPE_WINDOW));
    }

    #[test]
    fn local_press_ids_never_collide_with_plugin_signal_ids() {
        let first = next_local_press_id();
        let second = next_local_press_id();
        assert!(first >= LOCAL_PRESS_ID_BASE);
        assert!(second > first);
    }
}
