//! 窗口进程里的本地热键匹配（读 egui 输入的那一半）。
//!
//! 判定与类型在库侧 [`openless_linux_egui::local_hotkeys`]；这里只做「读本帧输入
//! → 捕获候选 → 命中判定 → 产出边沿」。之所以放在可执行文件里：`ui/` 属于
//! `main.rs`，而协议类型必须在库侧（`ui::bridge` / `popup` 要用）。
//!
//! 捕获复用设置页录制器的 `captured_binding`，命中判定复用注册给插件的
//! `(keysym, states)` 换算，因此不会出现第二套键名表。

use openless_core::HotkeyRuntimeTarget;
use openless_core::shared_types::ShortcutBinding;
use openless_linux_egui::local_hotkeys::{
    LocalHotkey, LocalHotkeyEdge, LocalHotkeyEdgeKind, match_hotkey, next_local_press_id,
};

use crate::ui::frontend::settings::{bare_modifier_name, captured_binding};

/// 一次按键的本地匹配器（每个窗口进程一份）。
#[derive(Default)]
pub struct LocalHotkeyMatcher {
    /// 裸修饰键绑定的跨帧挂起态（由 `captured_binding` 维护，见它的文档）。
    pending_modifier: Option<String>,
    /// 已按下、还没松开的组合键。
    held: Option<HeldHotkey>,
}

struct HeldHotkey {
    hotkey: LocalHotkey,
    primary: String,
    key: Option<egui::Key>,
    press_id: u64,
}

impl LocalHotkeyMatcher {
    /// 读本帧输入，返回至多一个热键边沿。
    ///
    /// 每帧调用一次；没有按下/松开配置里的热键时返回 `None`，其余按键一律放行
    /// （本函数只读 `InputState`，不消费事件，文本框照常收到按键）。
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        target: &HotkeyRuntimeTarget,
    ) -> Option<LocalHotkeyEdge> {
        if let Some(held) = self.held.take() {
            if held.key.is_none() && first_press_key(ctx).is_some() {
                return Some(LocalHotkeyEdge {
                    hotkey: held.hotkey,
                    kind: LocalHotkeyEdgeKind::Cancelled,
                    press_id: held.press_id,
                });
            }
            if !released(ctx, held.key, &held.primary) {
                self.held = Some(held);
                return None;
            } else {
                return Some(LocalHotkeyEdge {
                    hotkey: held.hotkey,
                    kind: LocalHotkeyEdgeKind::Released,
                    press_id: held.press_id,
                });
            }
        }

        // A modifier-only binding needs a real press edge so Hold mode can start
        // recording and stop on release. Core's modifier grace period handles
        // ordinary typing; a following non-modifier key sends Cancelled above.
        let bare_modifier = ctx
            .input(|input| bare_modifier_name(input.modifiers))
            .map(str::to_string);
        if let Some(primary) = bare_modifier {
            let binding = ShortcutBinding {
                primary: primary.clone(),
                modifiers: Vec::new(),
            };
            if let Some(hotkey) = match_hotkey(target, &binding) {
                let press_id = next_local_press_id();
                self.held = Some(HeldHotkey {
                    hotkey: hotkey.clone(),
                    primary,
                    key: None,
                    press_id,
                });
                return Some(LocalHotkeyEdge {
                    hotkey,
                    kind: LocalHotkeyEdgeKind::Pressed,
                    press_id,
                });
            }
        }

        let (primary, modifiers) = captured_binding(ctx, &mut self.pending_modifier)?;
        if primary.is_empty() {
            return None;
        }
        let pressed = ShortcutBinding {
            primary: primary.clone(),
            modifiers,
        };
        let hotkey = match_hotkey(target, &pressed)?;
        // 组合键在按下时就捕获到（`key` 有值）→ 发 Pressed，松开发 Released；
        // 裸修饰键只能在松手时判定（egui 不给修饰键单独的 Key 事件）→ 发 Combined。
        let key = first_press_key(ctx);
        let press_id = next_local_press_id();
        match key {
            Some(key) => {
                self.held = Some(HeldHotkey {
                    hotkey: hotkey.clone(),
                    primary,
                    key: Some(key),
                    press_id,
                });
                Some(LocalHotkeyEdge {
                    hotkey,
                    kind: LocalHotkeyEdgeKind::Pressed,
                    press_id,
                })
            }
            None => Some(LocalHotkeyEdge {
                hotkey,
                kind: LocalHotkeyEdgeKind::Combined,
                press_id,
            }),
        }
    }
}

/// 挂起的组合键是否已经松开。
fn released(ctx: &egui::Context, key: Option<egui::Key>, primary: &str) -> bool {
    match key {
        Some(key) => ctx.input(|input| input.key_released(key)),
        // 裸修饰键：`bare_modifier_name` 只在「恰好按住一个修饰键类别」时给名字，
        // 所以它不再是那个名字就说明松开了。
        None => bare_modifier_name(ctx.input(|input| input.modifiers)) != Some(primary),
    }
}

/// 本帧第一个「真键」按下的 `Key`（裸修饰键绑定返回 `None`）。
fn first_press_key(ctx: &egui::Context) -> Option<egui::Key> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                ..
            } if !matches!(
                key,
                egui::Key::Escape | egui::Key::Copy | egui::Key::Cut | egui::Key::Paste
            ) =>
            {
                Some(*key)
            }
            _ => None,
        })
    })
}
