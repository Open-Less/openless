//! 大集合的分页：快照只带每个集合的第一页，其余由窗口按需拉取。
//!
//! 背景：视图模型里有一批「随使用无限增长」的集合（历史、词库、纠错规则、
//! 市场列表、风格包含 base64 图标）。整份发过去的做法在数据变大后会把 IPC 帧
//! 顶到上限，窗口要么拿不到数据、要么（旧实现）被宿主断开重开。
//!
//! 约定：
//! * 宿主侧的集合顺序与窗口看到的顺序**始终一致**（都是宿主存储的前缀），
//!   所以两边的下标语义天然相同 —— 窗口发出的 `HistoryPlay(index)` 之类在这里
//!   不需要改成 id。
//! * 快照里的 `*_entries` 只是第一页，`*_total` 才是真实条数；窗口据此显示
//!   「已载入 X / 共 Y」并发出 [`FrontendAction::LoadMore`]。
//! * 过滤/搜索仍由窗口在**已载入**的前缀上做（历史搜索除外，见 `linux_app`），
//!   所以筛选不会与分页错位：窗口发出的下标永远是它自己那份列表的下标。
//!
//! 条目数先到上限就停：按字节预算需要逐条序列化，而它每帧都会跑一遍，代价太大。

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};
use serde::{Deserialize, Serialize};

use super::layout::{self, ButtonKind};
use super::theme;
use super::view_model::{
    CorrectionRule, FrontendAction, FrontendViewModel, HistoryEntry, MarketplacePack, StylePack,
    VocabEntry,
};

/// 一页最多多少条。
pub const PAGE_MAX_ITEMS: usize = 200;

/// 一次发多少条。`OPENLESS_UI_PAGE_ITEMS=<n>` 可以压低它，用来在设备上
/// 演练「已载入 X / 共 Y + 加载更多」（否则要攒够 200 条才有分页可看）。
pub fn page_limit() -> usize {
    std::env::var("OPENLESS_UI_PAGE_ITEMS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(PAGE_MAX_ITEMS)
}

/// 从 `items[offset..]` 取一页。宿主发第一页和后续页都走这里，保证两边
/// 看到的是同一个顺序前缀 —— 窗口按下标拼接、宿主按下标解析动作，靠的就是它。
pub fn slice_page<T: Clone>(items: &[T], offset: usize, limit: usize) -> Vec<T> {
    items.iter().skip(offset).take(limit).cloned().collect()
}

/// 自动加载的上限：搜索/筛选把可见条目压得很小时窗口会替用户往下拉，
/// 但拉到这个条数就停在按钮上，避免一次查询把整个库都灌进来。
pub const AUTO_LOAD_MAX_ITEMS: usize = 5_000;

/// 五个可增长的集合。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Collection {
    History,
    Vocabulary,
    CorrectionRules,
    Marketplace,
    StylePacks,
}

impl Collection {
    /// 一页数据回包时用的名字（日志与测试里可读）。
    pub fn tag(self) -> &'static str {
        match self {
            Collection::History => "history",
            Collection::Vocabulary => "vocabulary",
            Collection::CorrectionRules => "correction_rules",
            Collection::Marketplace => "marketplace",
            Collection::StylePacks => "style_packs",
        }
    }

    /// 窗口当前已载入这个集合的多少条。
    pub fn loaded(self, view_model: &FrontendViewModel) -> usize {
        match self {
            Collection::History => view_model.history_entries.len(),
            Collection::Vocabulary => view_model.vocab_entries.len(),
            Collection::CorrectionRules => view_model.vocab_rules.len(),
            Collection::Marketplace => view_model.marketplace_packs.len(),
            Collection::StylePacks => view_model.style_packs.len(),
        }
    }

    /// 宿主侧一共有多少条（快照带来的真实条数）。
    pub fn total(self, view_model: &FrontendViewModel) -> usize {
        match self {
            Collection::History => view_model.history_list_total,
            Collection::Vocabulary => view_model.vocab_total,
            Collection::CorrectionRules => view_model.correction_rule_total,
            Collection::Marketplace => view_model.marketplace_total,
            Collection::StylePacks => view_model.style_pack_total,
        }
    }
}

/// 一页的数据。每个变体对应 [`Collection`] 的一个集合。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollectionItems {
    History(Vec<HistoryEntry>),
    Vocabulary(Vec<VocabEntry>),
    CorrectionRules(Vec<CorrectionRule>),
    Marketplace(Vec<MarketplacePack>),
    StylePacks(Vec<StylePack>),
}

impl CollectionItems {
    pub fn collection(&self) -> Collection {
        match self {
            CollectionItems::History(_) => Collection::History,
            CollectionItems::Vocabulary(_) => Collection::Vocabulary,
            CollectionItems::CorrectionRules(_) => Collection::CorrectionRules,
            CollectionItems::Marketplace(_) => Collection::Marketplace,
            CollectionItems::StylePacks(_) => Collection::StylePacks,
        }
    }
}

impl FrontendViewModel {
    /// 收下一页数据。返回是否真的并进去了。
    ///
    /// * `offset == loaded`：正常追加（用户点了「加载更多」或自动加载）。
    /// * `offset < loaded`：回包比手上更旧 —— 截断到 `offset` 再补上新的一页，
    ///   这样「先发出去的请求晚回来」也不会把列表拼歪。
    /// * `offset > loaded`：中间那页还没到（帧被跳过或请求乱序），先丢掉；
    ///   下一帧的快照会给回正确的前缀。
    pub fn apply_page(
        &mut self,
        collection: Collection,
        offset: usize,
        total: usize,
        items: CollectionItems,
    ) -> bool {
        if items.collection() != collection {
            log::warn!(
                "[ui-client] page for {} carried {} items; ignored",
                collection.tag(),
                items.collection().tag()
            );
            return false;
        }
        let loaded = collection.loaded(self);
        if offset > loaded {
            log::debug!(
                "[ui-client] {} page at {offset} arrived with only {loaded} loaded; ignored",
                collection.tag()
            );
            return false;
        }
        let changed = match (collection, items) {
            (Collection::History, CollectionItems::History(items)) => {
                rewrite(&mut self.history_entries, offset, items)
            }
            (Collection::Vocabulary, CollectionItems::Vocabulary(items)) => {
                rewrite(&mut self.vocab_entries, offset, items)
            }
            (Collection::CorrectionRules, CollectionItems::CorrectionRules(items)) => {
                rewrite(&mut self.vocab_rules, offset, items)
            }
            (Collection::Marketplace, CollectionItems::Marketplace(items)) => {
                rewrite(&mut self.marketplace_packs, offset, items)
            }
            (Collection::StylePacks, CollectionItems::StylePacks(items)) => {
                rewrite(&mut self.style_packs, offset, items)
            }
            _ => false,
        };
        match collection {
            Collection::History => self.history_list_total = total,
            Collection::Vocabulary => self.vocab_total = total,
            Collection::CorrectionRules => self.correction_rule_total = total,
            Collection::Marketplace => self.marketplace_total = total,
            Collection::StylePacks => self.style_pack_total = total,
        }
        changed
    }
}

/// 把 `items` 放到 `list[offset..]`：先截断到 `offset`（等于追加时是空操作）。
fn rewrite<T>(list: &mut Vec<T>, offset: usize, items: Vec<T>) -> bool {
    if items.is_empty() && offset == list.len() {
        return false;
    }
    list.truncate(offset);
    list.extend(items);
    true
}

/// 列表底部的「已载入 X / 共 Y」+「加载更多」。
///
/// 只有还有没载入的条目时才画；`auto` 为真表示窗口正在替用户自动加载
/// （搜索/筛选把可见条目压得很小时），这时只显示进度、不画按钮。
pub fn load_more_footer(
    ui: &mut egui::Ui,
    lang: Lang,
    collection: Collection,
    view_model: &FrontendViewModel,
    auto: bool,
    actions: &mut Vec<FrontendAction>,
) {
    let loaded = collection.loaded(view_model);
    let total = collection.total(view_model);
    if total <= loaded {
        return;
    }
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
    let progress = fmt_l10n(lang, "common.loaded_of_total", &[&loaded, &total]);
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left() + 2.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &progress,
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );
    if auto {
        painter.text(
            egui::pos2(rect.right() - 2.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            tr_l10n(lang, "common.loading"),
            egui::FontId::proportional(11.0),
            theme::INK_4,
        );
        // 搜索/筛选下的自动加载：宿主一个 tick 内就回包，下一帧 loaded 变大，
        // 条件自然不再成立；到上限后停在按钮上，交回给用户。
        if loaded < AUTO_LOAD_MAX_ITEMS {
            actions.push(FrontendAction::LoadMore {
                collection,
                offset: loaded,
            });
        }
        return;
    }
    let more = tr_l10n(lang, "common.load_more");
    let button_width = layout::text_width(ui, more, 12.5) + 30.0;
    let button_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - button_width, rect.top() + 3.0),
        egui::vec2(button_width, 28.0),
    );
    if layout::action_button(ui, button_rect, more, None, ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::LoadMore {
            collection,
            offset: loaded,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_entries(count: usize) -> Vec<HistoryEntry> {
        (0..count)
            .map(|index| HistoryEntry {
                id: format!("entry-{index}"),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn a_page_appends_at_the_end() {
        let mut view_model = FrontendViewModel {
            history_entries: history_entries(2),
            ..Default::default()
        };
        let applied = view_model.apply_page(
            Collection::History,
            2,
            5,
            CollectionItems::History(history_entries(3)),
        );

        assert!(applied);
        assert_eq!(view_model.history_entries.len(), 5);
        assert_eq!(view_model.history_list_total, 5);
        assert_eq!(view_model.history_entries[2].id, "entry-0");
        assert_eq!(view_model.history_entries[4].id, "entry-2");
    }

    #[test]
    fn a_late_page_replaces_the_tail_instead_of_duplicating() {
        // 先发出的请求晚回来：offset 比手上更旧，必须截断重写，不能拼歪。
        let mut view_model = FrontendViewModel {
            history_entries: history_entries(5),
            history_list_total: 9,
            ..Default::default()
        };
        let applied = view_model.apply_page(
            Collection::History,
            3,
            9,
            CollectionItems::History(history_entries(2)),
        );

        assert!(applied);
        assert_eq!(view_model.history_entries.len(), 5, "3 + 2");
        assert_eq!(view_model.history_entries[3].id, "entry-0");
    }

    #[test]
    fn a_page_with_a_gap_is_ignored() {
        let mut view_model = FrontendViewModel {
            history_entries: history_entries(2),
            ..Default::default()
        };
        let applied = view_model.apply_page(
            Collection::History,
            4,
            9,
            CollectionItems::History(history_entries(2)),
        );

        assert!(
            !applied,
            "a page with a gap must be dropped and wait for the next snapshot"
        );
        assert_eq!(view_model.history_entries.len(), 2);
    }

    #[test]
    fn a_page_for_another_collection_is_refused() {
        let mut view_model = FrontendViewModel::default();
        let applied = view_model.apply_page(
            Collection::History,
            0,
            1,
            CollectionItems::Vocabulary(Vec::new()),
        );

        assert!(!applied);
        assert!(view_model.history_entries.is_empty());
    }

    #[test]
    fn pages_are_prefixes_of_the_same_list() {
        let items: Vec<String> = (0..5).map(|index| format!("i{index}")).collect();

        assert_eq!(slice_page(&items, 0, 2), vec!["i0", "i1"]);
        assert_eq!(slice_page(&items, 2, 2), vec!["i2", "i3"]);
        // 越界只给剩下的那几条，不会报错也不会绕回开头。
        assert_eq!(slice_page(&items, 4, 9), vec!["i4"]);
        assert!(slice_page(&items, 9, 2).is_empty());
    }

    #[test]
    fn totals_track_each_collection() {
        let view_model = FrontendViewModel {
            history_entries: history_entries(3),
            history_list_total: 300,
            vocab_entries: Vec::new(),
            vocab_total: 12,
            ..Default::default()
        };

        assert_eq!(Collection::History.loaded(&view_model), 3);
        assert_eq!(Collection::History.total(&view_model), 300);
        assert_eq!(Collection::Vocabulary.loaded(&view_model), 0);
        assert_eq!(Collection::Vocabulary.total(&view_model), 12);
    }
}
