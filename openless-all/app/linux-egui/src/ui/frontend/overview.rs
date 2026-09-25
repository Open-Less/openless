//! Overview page — the 2.0 dashboard.
//!
//! This is a 1:1 port of the Tauri `pages/Overview.tsx` layout:
//!
//! ```text
//! ┌ title + refresh ───────────────────────────────────────────┐
//! │ 当前语音服务 (only while a provider is unconfigured)        │
//! │ 使用记录   [chars] [duration] [avg] [total]                │
//! │ ┌ period chart (7/30d × count/chars/duration) ┐ ┌ recent ┐ │
//! │ └───────────────────────────────────────────────┘ └────────┘ │
//! │ 年度活动 heatmap                                            │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! The page is a single-screen dashboard: it fills the height the shell gives
//! it, wraps its columns when the window is narrow, and never scrolls as a
//! whole (only the recent list scrolls internally). Every visible string comes
//! from the localization catalog through `vm.lang`; nothing is hardcoded.

use eframe::egui;

use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{
    FrontendAction, FrontendViewModel, OverviewActivityDay, OverviewMode, OverviewRecentEntry,
    OverviewSummary, Page,
};

const GAP: f32 = 12.0;
const SECTION_GAP: f32 = 14.0;
/// 窄屏断点。Tauri 用的是**整个窗口**宽度（`useMobileLayout(720)`：
/// `window.innerWidth < 720`），不是内容区宽度；egui 这里换算回窗口宽度，
/// 否则主窗口缩到最小（960）时内容区 ≈748 < 原来的 760 阀值 → 两块卡片会错误地
/// 上下堆叠（用户报的「窗口缩到最小后星期几张图被遮住」）。
const MOBILE_BREAKPOINT: f32 = 720.0;
const METRIC_CARD_HEIGHT: f32 = 92.0;
const PROVIDER_CARD_HEIGHT: f32 = 104.0;
const CARD_PADDING: f32 = 14.0;
const BOTTOM_MIN_HEIGHT: f32 = 170.0;
const HEATMAP_MIN_HEIGHT: f32 = 90.0;
const LINE_SOFT: egui::Color32 = theme::LINE_SOFT;
const MONO: f32 = 12.0;

// ── Entry point ─────────────────────────────────────────────────────────────

/// Minimum height the single-screen dashboard needs before it starts
/// scrolling: header + stats row + chart row + heatmap.
const OVERVIEW_MIN_HEIGHT: f32 = 600.0;

pub fn page(ui: &mut egui::Ui, vm: &FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    // `width` 是内容区宽度（还减了 24px 滚动条/内边距），换算回窗口宽度再比断点。
    let mobile = width + 24.0 + layout::SIDEBAR_WIDTH < MOBILE_BREAKPOINT;

    // 单屏仪表盘优先；窗口高度不够时整页滚动，而不是把底部的年度活动热力图裁掉
    // （裁掉就完全摸不到了）。
    if ui.available_height() < overview_min_height(mobile) {
        egui::ScrollArea::vertical()
            .id_salt("openless-overview-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(width);
                ui.set_max_width(width);
                body(ui, width, vm, actions, overview_min_height(mobile), mobile);
            });
        return;
    }
    body(ui, width, vm, actions, 0.0, mobile);
}

/// 单屏仪表盘需要的最小高度：低于它就把整页改成滚动布局。
/// 底部两张卡左右排列时需要的空间比堆叠少，所以窄屏的阀值反而更高。
fn overview_min_height(mobile: bool) -> f32 {
    if mobile {
        OVERVIEW_MIN_HEIGHT + BOTTOM_MIN_HEIGHT
    } else {
        OVERVIEW_MIN_HEIGHT
    }
}

/// One dashboard layout. `forced_total` > 0 lays the page out for a scroll
/// container of that height instead of the current viewport.
fn body(
    ui: &mut egui::Ui,
    width: f32,
    vm: &FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    forced_total: f32,
    mobile: bool,
) {
    let lang = vm.lang;
    let start_y = ui.cursor().min.y;

    header(ui, width, lang, actions);

    if vm.overview_loading {
        ui.add_space(SECTION_GAP);
        placeholder_card(
            ui,
            width,
            tr_l10n(lang, "loading.overview"),
            false,
            lang,
            actions,
        );
        return;
    }
    if let Some(error) = vm.overview_error.as_deref() {
        ui.add_space(SECTION_GAP);
        placeholder_card(ui, width, error, true, lang, actions);
        return;
    }
    let Some(summary) = vm.overview.as_ref() else {
        ui.add_space(SECTION_GAP);
        placeholder_card(
            ui,
            width,
            tr_l10n(lang, "overview.metric_no_data"),
            true,
            lang,
            actions,
        );
        return;
    };

    ui.add_space(SECTION_GAP);

    if !(summary.asr_configured && summary.llm_configured) {
        providers_section(ui, width, summary, lang, actions, mobile);
        ui.add_space(SECTION_GAP);
    }

    stats_section(ui, width, summary, lang, mobile);
    ui.add_space(SECTION_GAP);

    // The bottom row absorbs the leftover height; the heatmap keeps its
    // natural size unless the window is too short, in which case it shrinks
    // (and below `HEATMAP_MIN_HEIGHT` it is dropped) so nothing overflows.
    // It is only shown when the preference is on and there is activity to draw
    // (matching the Tauri `showOverviewActivityHeatmap` gate).
    let available = if forced_total > 0.0 {
        (forced_total - (ui.cursor().min.y - start_y)).max(BOTTOM_MIN_HEIGHT)
    } else {
        ui.available_height().max(0.0)
    };
    let heatmap_cols = heatmap_columns(summary.heatmap_year, summary.heatmap.len());
    let ideal_heatmap = heatmap_card_height(width, heatmap_cols);
    let mut heatmap_height = ideal_heatmap;
    if available - heatmap_height - SECTION_GAP < BOTTOM_MIN_HEIGHT {
        heatmap_height = (available - BOTTOM_MIN_HEIGHT - SECTION_GAP).max(0.0);
    }
    let has_activity = summary.heatmap.iter().any(|day| day.count > 0);
    let show_heatmap =
        heatmap_height >= HEATMAP_MIN_HEIGHT && vm.settings.activity_heatmap && has_activity;
    let bottom_height = if show_heatmap {
        (available - heatmap_height - SECTION_GAP).max(BOTTOM_MIN_HEIGHT)
    } else {
        available.max(BOTTOM_MIN_HEIGHT)
    };

    bottom_row(ui, width, bottom_height, vm, summary, lang, actions, mobile);

    if show_heatmap {
        ui.add_space(SECTION_GAP);
        heatmap_card(ui, width, heatmap_height, heatmap_cols, summary, lang);
    }
}

// ── Header ──────────────────────────────────────────────────────────────────

fn header(ui: &mut egui::Ui, width: f32, lang: Lang, actions: &mut Vec<FrontendAction>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "overview.title"),
        theme::medium_font(26.0),
        theme::INK,
    );
    let label = tr_l10n(lang, "overview.refresh");
    let button_width = layout::text_width(ui, label, 12.5) + 42.0;
    let button_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - button_width, rect.center().y - 15.0),
        egui::vec2(button_width, 30.0),
    );
    if layout::action_button(
        ui,
        button_rect,
        label,
        Some(IconName::Refresh),
        layout::ButtonKind::Ghost,
    )
    .clicked()
    {
        actions.push(FrontendAction::OverviewRefresh);
    }
}

fn placeholder_card(
    ui: &mut egui::Ui,
    width: f32,
    message: &str,
    retry: bool,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let height = 132.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    paint_card(ui.painter(), rect);
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(
            rect.center().x,
            rect.center().y - if retry { 14.0 } else { 0.0 },
        ),
        egui::Align2::CENTER_CENTER,
        message,
        egui::FontId::proportional(12.5),
        theme::INK_3,
    );
    if retry {
        let label = tr_l10n(lang, "overview.retry");
        let button_width = layout::text_width(ui, label, 12.5) + 26.0;
        let button_rect = egui::Rect::from_min_size(
            egui::pos2(rect.center().x - button_width / 2.0, rect.center().y + 4.0),
            egui::vec2(button_width, 28.0),
        );
        if layout::action_button(ui, button_rect, label, None, layout::ButtonKind::Ghost).clicked()
        {
            actions.push(FrontendAction::OverviewRefresh);
        }
    }
}

// ── Providers ───────────────────────────────────────────────────────────────

fn providers_section(
    ui: &mut egui::Ui,
    width: f32,
    summary: &OverviewSummary,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
    mobile: bool,
) {
    let (heading, _) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
    ui.painter().with_clip_rect(heading).text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "overview.services_title"),
        theme::medium_font(13.0),
        theme::INK_2,
    );
    ui.add_space(8.0);

    // Only providers still waiting for configuration are worth a card.
    let mut pending: Vec<(&'static str, String, &'static str, IconName)> = Vec::new();
    if !summary.asr_configured {
        pending.push((
            tr_l10n(lang, "overview.asr_kind"),
            provider_name(&summary.asr_provider, lang),
            tr_l10n(lang, "overview.provider_help_asr"),
            IconName::Mic,
        ));
    }
    if !summary.llm_configured {
        pending.push((
            tr_l10n(lang, "overview.llm_kind"),
            provider_name(&summary.llm_provider, lang),
            tr_l10n(lang, "overview.provider_help_llm"),
            IconName::Sparkle,
        ));
    }
    if pending.is_empty() {
        return;
    }

    // Tauri：`mobile || pendingProviders.length < 2` → 单列，否则两列。
    let columns = if pending.len() < 2 || mobile { 1 } else { 2 };
    let mut index = 0;
    while index < pending.len() {
        let count = columns.min(pending.len() - index);
        cards_row(
            ui,
            width,
            PROVIDER_CARD_HEIGHT,
            count,
            GAP,
            |ui, slot, rect| {
                let (kind, name, help, icon) = &pending[index + slot];
                provider_card(ui, rect, kind, name, help, *icon, lang, actions);
            },
        );
        index += count;
        if index < pending.len() {
            ui.add_space(GAP);
        }
    }
}

fn provider_name(active: &str, lang: Lang) -> String {
    if active.trim().is_empty() {
        tr_l10n(lang, "overview.not_set").to_string()
    } else {
        active.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn provider_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    kind: &str,
    name: &str,
    help: &str,
    icon: IconName,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    card_scope(ui, rect, 16.0, |ui, inner| {
        let icon_rect = egui::Rect::from_min_size(inner.min, egui::vec2(38.0, 38.0));
        let painter = ui.painter().with_clip_rect(inner);
        painter.rect_filled(icon_rect, egui::CornerRadius::same(10), theme::BLUE_SOFT);
        icons::draw_icon(ui, icon_rect.center(), icon, theme::BLUE);

        let text_left = inner.left() + 50.0;
        painter.text(
            egui::pos2(text_left, inner.top() + 3.0),
            egui::Align2::LEFT_TOP,
            kind,
            egui::FontId::proportional(12.5),
            theme::INK_4,
        );
        let pill = tr_l10n(lang, "overview.unconfigured");
        let pill_width = layout::text_width(ui, pill, 10.5) + 18.0;
        let pill_rect = egui::Rect::from_min_size(
            egui::pos2(
                text_left + layout::text_width(ui, kind, 12.5) + 8.0,
                inner.top() + 2.0,
            ),
            egui::vec2(pill_width, 18.0),
        );
        painter.rect_stroke(
            pill_rect,
            egui::CornerRadius::same(9),
            egui::Stroke::new(0.7, theme::LINE),
            egui::StrokeKind::Inside,
        );
        painter.text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            pill,
            egui::FontId::proportional(10.5),
            theme::INK_3,
        );
        painter.text(
            egui::pos2(text_left, inner.top() + 24.0),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::proportional(15.0),
            theme::INK,
        );

        // Bottom row: help text on the left, configure action on the right.
        let label = tr_l10n(lang, "overview.configure_provider");
        let button_width = layout::text_width(ui, label, 12.5) + 34.0;
        let button_rect = egui::Rect::from_min_size(
            egui::pos2(inner.right() - button_width, inner.bottom() - 28.0),
            egui::vec2(button_width, 28.0),
        );
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = (button_rect.left() - inner.left() - 12.0).max(10.0);
        job.wrap.max_rows = 2;
        job.append(
            help,
            0.0,
            egui::text::TextFormat {
                font_id: egui::FontId::proportional(12.5),
                color: theme::INK_3,
                ..Default::default()
            },
        );
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        painter.galley(
            egui::pos2(inner.left(), button_rect.center().y - galley.size().y / 2.0),
            galley,
            theme::INK_3,
        );
        if layout::action_button(ui, button_rect, label, None, layout::ButtonKind::Ghost).clicked()
        {
            actions.push(FrontendAction::ToggleSettings);
        }
    });
}

// ── Usage metrics ───────────────────────────────────────────────────────────

fn stats_section(
    ui: &mut egui::Ui,
    width: f32,
    summary: &OverviewSummary,
    lang: Lang,
    mobile: bool,
) {
    let (heading, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
    ui.painter().with_clip_rect(heading).text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "overview.stats_title"),
        theme::medium_font(13.0),
        theme::INK_2,
    );
    ui.add_space(8.0);

    if mobile {
        cards_row(ui, width, METRIC_CARD_HEIGHT, 2, GAP, |ui, slot, rect| {
            metric_card(ui, rect, slot, summary, lang);
        });
        ui.add_space(GAP);
        cards_row(ui, width, METRIC_CARD_HEIGHT, 2, GAP, |ui, slot, rect| {
            metric_card(ui, rect, slot + 2, summary, lang);
        });
    } else {
        cards_row(ui, width, METRIC_CARD_HEIGHT, 4, GAP, |ui, slot, rect| {
            metric_card(ui, rect, slot, summary, lang);
        });
    }
}

fn metric_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    index: usize,
    summary: &OverviewSummary,
    lang: Lang,
) {
    let (icon, label, value, trend) = match index {
        0 => (
            IconName::Hash,
            tr_l10n(lang, "overview.metric_chars"),
            format_number(summary.chars_today),
            fmt_l10n(lang, "overview.metric_segments", &[&summary.segments_today]),
        ),
        1 => (
            IconName::Mic,
            tr_l10n(lang, "overview.metric_duration"),
            short_duration(summary.duration_ms_today, lang),
            String::new(),
        ),
        2 => (
            IconName::Clock,
            tr_l10n(lang, "overview.metric_avg"),
            short_duration(summary.avg_latency_ms, lang),
            if summary.segments_today > 0 {
                tr_l10n(lang, "overview.metric_avg_trend").to_string()
            } else {
                tr_l10n(lang, "overview.metric_no_data").to_string()
            },
        ),
        _ => (
            IconName::Bolt,
            tr_l10n(lang, "overview.metric_total"),
            format_number(summary.history_total as u64),
            fmt_l10n(
                lang,
                "overview.metric_total_trend",
                &[&openless_core::HISTORY_CAP],
            ),
        ),
    };

    card_scope(ui, rect, CARD_PADDING, |ui, inner| {
        let painter = ui.painter().with_clip_rect(inner);
        icons::draw_icon(
            ui,
            egui::pos2(inner.left() + 6.5, inner.top() + 7.0),
            icon,
            theme::INK_3,
        );
        painter.text(
            egui::pos2(inner.left() + 18.0, inner.top() + 7.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.5),
            theme::INK_3,
        );
        painter.text(
            egui::pos2(inner.left(), inner.top() + 20.0),
            egui::Align2::LEFT_TOP,
            value,
            egui::FontId::proportional(22.0),
            theme::INK,
        );
        painter.text(
            egui::pos2(inner.left(), inner.bottom() - 16.0),
            egui::Align2::LEFT_TOP,
            trend,
            egui::FontId::proportional(12.0),
            theme::INK_4,
        );
    });
}

// ── Bottom row: period chart + recent list ──────────────────────────────────

fn bottom_row(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    vm: &FrontendViewModel,
    summary: &OverviewSummary,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
    mobile: bool,
) {
    if !mobile {
        let (row, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let left_width = ((width - GAP) / 2.4).max(220.0);
        let right_width = (width - GAP - left_width).max(220.0);
        period_card(
            ui,
            egui::Rect::from_min_size(row.min, egui::vec2(left_width, height)),
            vm,
            summary,
            lang,
            actions,
        );
        recent_card(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(row.left() + left_width + GAP, row.top()),
                egui::vec2(right_width, height),
            ),
            summary,
            lang,
            actions,
        );
    } else {
        let chart_height = 190.0;
        let (chart, _) =
            ui.allocate_exact_size(egui::vec2(width, chart_height), egui::Sense::hover());
        period_card(ui, chart, vm, summary, lang, actions);
        ui.add_space(GAP);
        let recent_height = (height - chart_height - GAP).max(150.0);
        let (recent, _) =
            ui.allocate_exact_size(egui::vec2(width, recent_height), egui::Sense::hover());
        recent_card(ui, recent, summary, lang, actions);
    }
}

fn period_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vm: &FrontendViewModel,
    summary: &OverviewSummary,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let period = vm.overview_period.min(1);
    let metric = vm.overview_metric.min(2);
    card_scope(ui, rect, 18.0, |ui, inner| {
        let period_labels = [
            tr_l10n(lang, "overview.period_last7").to_string(),
            tr_l10n(lang, "overview.period_last30").to_string(),
        ];
        let metric_labels = [
            tr_l10n(lang, "overview.metric_count").to_string(),
            tr_l10n(lang, "overview.metric_chars_name").to_string(),
            tr_l10n(lang, "overview.metric_duration_name").to_string(),
        ];
        let period_width = segmented_width(ui, &period_labels);
        let metric_width = segmented_width(ui, &metric_labels);
        let toggle_height = 26.0;
        let mut y = inner.top();
        if period_width + metric_width + 8.0 <= inner.width() {
            let period_rect = egui::Rect::from_min_size(
                egui::pos2(inner.left(), y),
                egui::vec2(period_width, toggle_height),
            );
            if let Some(selected) = segmented(ui, period_rect, &period_labels, period) {
                actions.push(FrontendAction::OverviewPeriod(selected));
            }
            let metric_rect = egui::Rect::from_min_size(
                egui::pos2(inner.right() - metric_width, y),
                egui::vec2(metric_width, toggle_height),
            );
            if let Some(selected) = segmented(ui, metric_rect, &metric_labels, metric) {
                actions.push(FrontendAction::OverviewMetric(selected));
            }
            y += toggle_height + 14.0;
        } else {
            let period_rect = egui::Rect::from_min_size(
                egui::pos2(inner.left(), y),
                egui::vec2(period_width, toggle_height),
            );
            if let Some(selected) = segmented(ui, period_rect, &period_labels, period) {
                actions.push(FrontendAction::OverviewPeriod(selected));
            }
            y += toggle_height + 8.0;
            let metric_rect = egui::Rect::from_min_size(
                egui::pos2(inner.left(), y),
                egui::vec2(metric_width, toggle_height),
            );
            if let Some(selected) = segmented(ui, metric_rect, &metric_labels, metric) {
                actions.push(FrontendAction::OverviewMetric(selected));
            }
            y += toggle_height + 14.0;
        }

        let buckets = period_buckets(summary, period);
        let total: f64 = buckets
            .iter()
            .map(|(_, day)| metric_value(day, metric))
            .sum();
        let daily = if buckets.is_empty() {
            0.0
        } else {
            total / buckets.len() as f64
        };

        let painter = ui.painter().with_clip_rect(inner);
        painter.text(
            egui::pos2(inner.left(), y),
            egui::Align2::LEFT_TOP,
            format_metric_value(total, metric, lang),
            egui::FontId::proportional(26.0),
            theme::INK,
        );
        painter.text(
            egui::pos2(inner.left(), y + 34.0),
            egui::Align2::LEFT_TOP,
            fmt_l10n(
                lang,
                "overview.daily_avg",
                &[&format_metric_value(daily, metric, lang)],
            ),
            egui::FontId::proportional(12.0),
            theme::INK_4,
        );
        let chart_rect =
            egui::Rect::from_min_max(egui::pos2(inner.left(), y + 54.0), inner.right_bottom());
        if chart_rect.height() > 24.0 && chart_rect.width() > 24.0 {
            period_chart(ui, chart_rect, &buckets, metric, lang);
        }
    });
}

fn period_buckets(summary: &OverviewSummary, period: usize) -> Vec<(String, OverviewActivityDay)> {
    let days = if period == 0 { 7 } else { 30 };
    let start = summary.activity_daily.len().saturating_sub(days);
    summary.activity_daily[start..]
        .iter()
        .map(|day| (day.date.clone(), day.clone()))
        .collect()
}

fn period_chart(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    buckets: &[(String, OverviewActivityDay)],
    metric: usize,
    lang: Lang,
) {
    if buckets.is_empty() {
        return;
    }
    let dense = buckets.len() > 7;
    let gap = if dense { 2.0 } else { 8.0 };
    let max = buckets
        .iter()
        .map(|(_, day)| metric_value(day, metric))
        .fold(1.0_f64, f64::max);
    let label_height = 14.0;
    let bars_top = rect.top() + if dense { 0.0 } else { 14.0 };
    let bars_bottom = rect.bottom() - label_height;
    let bars_height = (bars_bottom - bars_top).max(8.0);
    let bar_width =
        ((rect.width() - gap * (buckets.len() as f32 - 1.0)) / buckets.len() as f32).max(1.0);
    let weekday_labels = split_labels(tr_l10n(lang, "overview.week_days"));
    let painter = ui.painter().with_clip_rect(rect);
    let last = buckets.len() - 1;

    for (index, (date, day)) in buckets.iter().enumerate() {
        let value = metric_value(day, metric);
        let left = rect.left() + index as f32 * (bar_width + gap);
        let is_today = index == last;
        let height = ((value / max) * bars_height as f64) as f32;
        let bar = egui::Rect::from_min_max(
            egui::pos2(left, bars_bottom - height.max(2.0)),
            egui::pos2(left + bar_width, bars_bottom),
        );
        let color = if is_today {
            theme::BLUE
        } else {
            with_alpha(theme::INK_4, if value <= 0.0 { 0.15 } else { 0.85 })
        };
        painter.rect_filled(
            bar,
            egui::CornerRadius::same(if dense { 2 } else { 4 }),
            color,
        );
        if !dense {
            painter.text(
                egui::pos2(left + bar_width / 2.0, bars_top - 1.0),
                egui::Align2::CENTER_BOTTOM,
                format_metric_value(value, metric, lang),
                egui::FontId::proportional(9.5),
                if is_today { theme::BLUE } else { theme::INK_4 },
            );
            let weekday = weekday_label(date, &weekday_labels);
            if !weekday.is_empty() {
                painter.text(
                    egui::pos2(left + bar_width / 2.0, rect.bottom() - label_height + 2.0),
                    egui::Align2::CENTER_TOP,
                    weekday,
                    egui::FontId::proportional(9.5),
                    theme::INK_4,
                );
            }
        }
    }

    if dense {
        for (index, align) in [
            (0usize, egui::Align2::LEFT_TOP),
            (last / 2, egui::Align2::CENTER_TOP),
            (last, egui::Align2::RIGHT_TOP),
        ] {
            painter.text(
                egui::pos2(
                    match align {
                        egui::Align2::LEFT_TOP => rect.left(),
                        egui::Align2::RIGHT_TOP => rect.right(),
                        _ => rect.center().x,
                    },
                    rect.bottom() - label_height + 2.0,
                ),
                align,
                short_date(&buckets[index].0),
                egui::FontId::proportional(10.0),
                theme::INK_4,
            );
        }
    }
}

// ── Recent list ─────────────────────────────────────────────────────────────

fn recent_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    summary: &OverviewSummary,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    paint_card(ui.painter(), rect);
    let header_height = 42.0;
    let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_height));
    let painter = ui.painter().with_clip_rect(rect);
    painter.line_segment(
        [header.left_bottom(), header.right_bottom()],
        egui::Stroke::new(0.5, theme::LINE),
    );
    painter.text(
        egui::pos2(rect.left() + 18.0, header.center().y),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "overview.recent_title"),
        egui::FontId::proportional(13.0),
        theme::INK_2,
    );
    let label = tr_l10n(lang, "overview.recent_all");
    let button_width = layout::text_width(ui, label, 12.0) + 20.0;
    let button_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 18.0 - button_width, header.center().y - 14.0),
        egui::vec2(button_width, 28.0),
    );
    if layout::action_button(ui, button_rect, label, None, layout::ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::Navigate(Page::History));
    }

    let list = egui::Rect::from_min_max(egui::pos2(rect.left(), header.bottom()), rect.max);
    layout::fixed_ui(ui, list, layout::card_salt(rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("overview-recent-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = ui.available_width();
                if summary.recent.is_empty() {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 64.0), egui::Sense::hover());
                    ui.painter().with_clip_rect(rect).text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        tr_l10n(lang, "overview.recent_empty_hint"),
                        egui::FontId::proportional(12.0),
                        theme::INK_4,
                    );
                    return;
                }
                for (index, entry) in summary.recent.iter().enumerate() {
                    recent_row(ui, width, entry, index, lang);
                }
            });
    });
}

fn recent_row(
    ui: &mut egui::Ui,
    width: f32,
    entry: &OverviewRecentEntry,
    index: usize,
    lang: Lang,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 54.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(0.5, LINE_SOFT),
    );

    let padding = 18.0;
    let time_label = history_time_label(&entry.created_at);
    let mode_label = mode_label(lang, entry.mode);
    painter.text(
        egui::pos2(rect.left() + padding, rect.top() + 15.0),
        egui::Align2::LEFT_CENTER,
        &time_label,
        egui::FontId::monospace(MONO),
        theme::INK_3,
    );
    let pill_width = layout::text_width(ui, mode_label, 10.5) + 16.0;
    let pill = egui::Rect::from_min_size(
        egui::pos2(rect.left() + padding, rect.top() + 25.0),
        egui::vec2(pill_width, 17.0),
    );
    painter.rect_filled(pill, egui::CornerRadius::same(9), theme::SURFACE_2);
    painter.text(
        pill.center(),
        egui::Align2::CENTER_CENTER,
        mode_label,
        egui::FontId::proportional(10.5),
        theme::INK_3,
    );
    let column_width = layout::text_width(ui, &time_label, MONO)
        .max(pill_width)
        .max(60.0);

    let duration_label = entry
        .duration_ms
        .map(|ms| short_duration(ms, lang))
        .unwrap_or_else(|| "—".to_string());
    let duration_width = layout::text_width(ui, &duration_label, MONO);
    let copy_label = tr_l10n(lang, "overview.copy");
    let copy_width = layout::text_width(ui, copy_label, 11.5) + 36.0;
    let copy_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - padding - copy_width, rect.top() + 14.0),
        egui::vec2(copy_width, 26.0),
    );
    painter.text(
        egui::pos2(copy_rect.left() - 7.0, rect.top() + 16.0),
        egui::Align2::RIGHT_CENTER,
        duration_label,
        egui::FontId::monospace(11.5),
        theme::INK_4,
    );

    let text = if entry.final_text.trim().is_empty() {
        entry.raw_transcript.as_str()
    } else {
        entry.final_text.as_str()
    };
    let first_line = text.split('\n').next().unwrap_or("");
    let text_left = rect.left() + padding + column_width + 12.0;
    let text_right = copy_rect.left() - 7.0 - duration_width - 10.0;
    if text_right > text_left && !first_line.is_empty() {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = text_right - text_left;
        job.wrap.max_rows = 2;
        job.append(
            first_line,
            0.0,
            egui::text::TextFormat {
                font_id: egui::FontId::proportional(13.5),
                color: theme::INK_2,
                ..Default::default()
            },
        );
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        painter.galley(
            egui::pos2(text_left, rect.top() + 14.0),
            galley,
            theme::INK_2,
        );
    }

    let copied_id = egui::Id::new(("overview-recent-copied", index));
    let now = ui.input(|input| input.time);
    let copied = ui
        .ctx()
        .data(|data| data.get_temp::<f64>(copied_id))
        .is_some_and(|at| now - at < 1.5);
    let label = if copied {
        tr_l10n(lang, "overview.copied")
    } else {
        tr_l10n(lang, "overview.copy")
    };
    if layout::action_button(
        ui,
        copy_rect,
        label,
        Some(IconName::Copy),
        layout::ButtonKind::Ghost,
    )
    .clicked()
    {
        ui.ctx().copy_text(text.to_string());
        ui.ctx().data_mut(|data| data.insert_temp(copied_id, now));
    }
}

fn mode_label(lang: Lang, mode: OverviewMode) -> &'static str {
    match mode {
        OverviewMode::Raw => tr_l10n(lang, "overview.mode_raw"),
        OverviewMode::Light => tr_l10n(lang, "overview.mode_light"),
        OverviewMode::Structured => tr_l10n(lang, "overview.mode_structured"),
        OverviewMode::Formal => tr_l10n(lang, "overview.mode_formal"),
    }
}

// ── Annual activity heatmap ─────────────────────────────────────────────────

fn heatmap_card_height(width: f32, columns: f32) -> f32 {
    let inner_width = (width - CARD_PADDING * 2.0).max(1.0);
    let (_, cell, gap) = heatmap_cell(inner_width, columns);
    let grid_height = 7.0 * cell + 6.0 * gap + 16.0;
    // The card must be tall enough that the height-constrained cell equals the
    // width-constrained cell, otherwise the grid stops short of the right edge.
    CARD_PADDING * 2.0 + 24.0 + grid_height
}

/// Sunday-first week columns needed to lay out the calendar year.
fn heatmap_columns(year: i32, days: usize) -> f32 {
    let offset = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
        .map(weekday_index)
        .unwrap_or(0);
    ((offset as f32 + days as f32) / 7.0).ceil().max(1.0)
}

/// `(step, cell, gap)` for the heatmap grid given the inner card width.
fn heatmap_cell(inner_width: f32, columns: f32) -> (f32, f32, f32) {
    let label_width = 30.0;
    let available = (inner_width - label_width).max(60.0);
    let step = available / columns.max(1.0);
    let gap = if step >= 13.0 { 3.0 } else { 2.0 };
    (step, (step - gap).max(4.0), gap)
}

fn heatmap_card(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    columns: f32,
    summary: &OverviewSummary,
    lang: Lang,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    card_scope(ui, rect, CARD_PADDING, |ui, inner| {
        let painter = ui.painter().with_clip_rect(inner);
        painter.text(
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            tr_l10n(lang, "overview.activity_title"),
            egui::FontId::proportional(13.0),
            theme::INK_2,
        );
        let grid = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.top() + 24.0),
            inner.right_bottom(),
        );
        heatmap_grid(ui, grid, columns, summary, lang);
    });
}

fn heatmap_grid(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    columns: f32,
    summary: &OverviewSummary,
    lang: Lang,
) {
    if summary.heatmap.is_empty() || rect.height() < 20.0 {
        return;
    }
    let weekdays = split_labels(tr_l10n(lang, "overview.week_days"));
    let months = split_labels(tr_l10n(lang, "overview.months"));
    let label_width = weekdays
        .iter()
        .map(|label| layout::text_width(ui, label, 9.0))
        .fold(0.0_f32, f32::max)
        + 8.0;
    let (_, width_cell, gap) = heatmap_cell(rect.width(), columns);
    let month_row = 16.0;
    let height_cell = ((rect.height() - month_row - gap * 6.0) / 7.0).max(3.0);
    let cell = width_cell.min(height_cell);
    let step = cell + gap;
    let grid_left = rect.left() + label_width;
    let grid_top = rect.top() + month_row;

    // Calendar-year grid, Sunday-first, like the Tauri Heatmap component.
    let first = chrono::NaiveDate::from_ymd_opt(summary.heatmap_year, 1, 1);
    let Some(first) = first else {
        return;
    };
    let offset = weekday_index(first) as f32;
    let column_count = columns.ceil() as usize;
    let max_count = summary
        .heatmap
        .iter()
        .map(|day| day.count)
        .max()
        .unwrap_or(0);

    let mut month_label: Vec<Option<usize>> = vec![None; column_count];
    let painter = ui.painter().with_clip_rect(rect);
    for (index, day) in summary.heatmap.iter().enumerate() {
        let position = offset as usize + index;
        let column = position / 7;
        let row = position % 7;
        let x = grid_left + column as f32 * step;
        let y = grid_top + row as f32 * step;
        let color = if day.count == 0 || max_count == 0 {
            theme::SURFACE_2
        } else {
            heat_color(day.count, max_count)
        };
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell, cell)),
            egui::CornerRadius::same(2),
            color,
        );
        if let Ok(date) = chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d") {
            use chrono::Datelike;
            if date.day() == 1 && month_label[column].is_none() {
                month_label[column] = Some(date.month0() as usize);
            }
        }
    }

    for (column, month) in month_label.iter().enumerate() {
        if let Some(month) = month {
            if let Some(label) = months.get(*month) {
                painter.text(
                    egui::pos2(grid_left + column as f32 * step, rect.top()),
                    egui::Align2::LEFT_TOP,
                    *label,
                    egui::FontId::proportional(9.0),
                    theme::INK_4,
                );
            }
        }
    }
    for (row, label) in weekdays.iter().enumerate() {
        if row % 2 == 1 {
            painter.text(
                egui::pos2(rect.left(), grid_top + row as f32 * step + cell / 2.0),
                egui::Align2::LEFT_CENTER,
                *label,
                egui::FontId::proportional(9.0),
                theme::INK_4,
            );
        }
    }
}

fn heat_color(count: u32, max: u32) -> egui::Color32 {
    let ratio = count as f64 / max.max(1) as f64;
    let t = ratio.sqrt().min(1.0) as f32;
    let min = (191.0, 219.0, 254.0);
    let max = (29.0, 78.0, 216.0);
    egui::Color32::from_rgb(
        (min.0 + (max.0 - min.0) * t) as u8,
        (min.1 + (max.1 - min.1) * t) as u8,
        (min.2 + (max.2 - min.2) * t) as u8,
    )
}

// ── Layout primitives ───────────────────────────────────────────────────────

fn cards_row(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    count: usize,
    gap: f32,
    mut draw: impl FnMut(&mut egui::Ui, usize, egui::Rect),
) {
    if count == 0 {
        return;
    }
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let card_width = ((width - gap * (count as f32 - 1.0)) / count as f32).max(1.0);
    for slot in 0..count {
        let rect = egui::Rect::from_min_size(
            egui::pos2(row.left() + slot as f32 * (card_width + gap), row.top()),
            egui::vec2(card_width, height),
        );
        draw(ui, slot, rect);
    }
}

fn paint_card(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, egui::CornerRadius::same(14), theme::SURFACE);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
}

fn card_scope(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    padding: f32,
    contents: impl FnOnce(&mut egui::Ui, egui::Rect),
) {
    paint_card(ui.painter(), rect);
    let inner = rect.shrink(padding);
    // `fixed_ui` (not `scope_builder`) so painting card contents never rewinds
    // the page cursor and overlaps the next row.
    layout::fixed_ui(ui, inner, layout::card_salt(rect), |ui| contents(ui, inner));
}

fn segmented_width(ui: &egui::Ui, options: &[String]) -> f32 {
    let mut width = 4.0;
    for (index, option) in options.iter().enumerate() {
        if index > 0 {
            width += 2.0;
        }
        width += layout::text_width(ui, option, 12.0) + 18.0;
    }
    width
}

fn segmented(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    options: &[String],
    selected: usize,
) -> Option<usize> {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 2.0;
    let mut clicked = None;
    for (index, option) in options.iter().enumerate() {
        let width = layout::text_width(ui, option, 12.0) + 18.0;
        let option_rect = egui::Rect::from_min_size(
            egui::pos2(x, rect.top() + 2.0),
            egui::vec2(width, rect.height() - 4.0),
        );
        let id = ui.id().with((
            "overview-segment",
            index,
            rect.left().round() as i32,
            rect.top().round() as i32,
        ));
        let response = ui.interact(option_rect, id, egui::Sense::click());
        let is_selected = index == selected;
        if is_selected {
            painter.rect_filled(option_rect, egui::CornerRadius::same(6), theme::BLUE);
        } else if response.hovered() {
            painter.rect_filled(option_rect, egui::CornerRadius::same(6), theme::SURFACE);
        }
        painter.text(
            option_rect.center(),
            egui::Align2::CENTER_CENTER,
            option,
            egui::FontId::proportional(12.0),
            if is_selected {
                egui::Color32::WHITE
            } else {
                theme::INK_3
            },
        );
        if response.clicked() {
            clicked = Some(index);
        }
        x += width + 2.0;
    }
    clicked
}

// ── Text / value helpers ────────────────────────────────────────────────────

fn split_labels(value: &str) -> Vec<&str> {
    value.split('|').collect()
}

fn with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn weekday_index(date: chrono::NaiveDate) -> u32 {
    use chrono::Datelike;
    date.weekday().num_days_from_sunday()
}

fn weekday_label(date: &str, labels: &[&str]) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .map(weekday_index)
        .and_then(|index| labels.get(index as usize).copied())
        .unwrap_or_default()
        .to_string()
}

fn short_date(date: &str) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|date| {
            use chrono::Datelike;
            format!("{}/{}", date.month(), date.day())
        })
        .unwrap_or_else(|_| date.to_string())
}

fn history_time_label(created_at: &str) -> String {
    use chrono::{Datelike, Timelike};
    let Ok(instant) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return created_at.to_string();
    };
    let local = instant.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    if local.date_naive() == now.date_naive() {
        format!("{:02}:{:02}", local.hour(), local.minute())
    } else if local.year() == now.year() {
        format!(
            "{}/{} {:02}:{:02}",
            local.month(),
            local.day(),
            local.hour(),
            local.minute()
        )
    } else {
        format!(
            "{}/{}/{} {:02}:{:02}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute()
        )
    }
}

fn format_number(value: u64) -> String {
    let raw = value.to_string();
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && (bytes.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(*byte as char);
    }
    out
}

fn metric_value(day: &OverviewActivityDay, metric: usize) -> f64 {
    match metric {
        1 => day.chars as f64,
        2 => day.duration_ms as f64,
        _ => day.count as f64,
    }
}

fn format_metric_value(value: f64, metric: usize, lang: Lang) -> String {
    if metric == 2 {
        long_duration(value as u64, lang)
    } else {
        format_number(value.round() as u64)
    }
}

/// Seconds/minutes/hours for a period total (`formatLongDuration`).
fn long_duration(ms: u64, lang: Lang) -> String {
    if ms == 0 {
        return "0".to_string();
    }
    let seconds = (ms as f64 / 1000.0).round() as u64;
    if seconds < 60 {
        return fmt_l10n(lang, "dur.sec", &[&seconds]);
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return fmt_l10n(lang, "overview.minutes", &[&minutes]);
    }
    fmt_l10n(
        lang,
        "overview.hours_minutes",
        &[&(minutes / 60), &(minutes % 60)],
    )
}

/// `formatDuration`: `—` / `3.1 秒` / `3:05`.
fn short_duration(ms: u64, lang: Lang) -> String {
    if ms == 0 {
        return "—".to_string();
    }
    let seconds = ms as f64 / 1000.0;
    if seconds < 60.0 {
        return fmt_l10n(lang, "dur.sec", &[&format!("{seconds:.1}")]);
    }
    format!("{}:{:02}", (seconds / 60.0) as u64, (seconds % 60.0) as u64)
}
