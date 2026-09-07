use eframe::egui;

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel};

// ── Overview page ───────────────────────────────────────────────────────────

pub fn overview_page(
    ui: &mut egui::Ui,
    vm: &FrontendViewModel,
    _actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);

    if vm.overview_loading {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new("概览")
                .size(28.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new("正在加载概览数据…").color(theme::INK_3));
        });
        return;
    }

    if let Some(error) = &vm.overview_error {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new("概览")
                .size(28.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(22.0);
        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
        return;
    }

    let Some(summary) = &vm.overview else {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new("概览")
                .size(28.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(22.0);
        ui.label(egui::RichText::new("暂无数据").color(theme::INK_3));
        return;
    };

    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new("概览")
            .size(28.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(22.0);

    let gap = 12.0;
    let provider_width = (width - gap) / 2.0;
    let provider_height = 98.0;
    let provider_row = ui
        .allocate_exact_size(egui::vec2(width, provider_height), egui::Sense::hover())
        .0;
    provider_card(
        ui,
        egui::Rect::from_min_size(
            provider_row.min,
            egui::vec2(provider_width, provider_height),
        ),
        "ASR 语音",
        &summary.asr_provider,
        summary.asr_configured,
        IconName::Mic,
    );
    provider_card(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(
                provider_row.left() + provider_width + gap,
                provider_row.top(),
            ),
            egui::vec2(provider_width, provider_height),
        ),
        "LLM 模型",
        &summary.llm_provider,
        summary.llm_configured,
        IconName::Sparkle,
    );
    ui.add_space(32.0);

    // Metric row
    let metric_width = (width - gap * 3.0) / 4.0;
    let metric_row = ui
        .allocate_exact_size(egui::vec2(width, 108.0), egui::Sense::hover())
        .0;
    for (index, (icon, label, value, detail, accent)) in [
        (
            IconName::Hash,
            "今日字数",
            summary.chars_today.to_string(),
            format!("{} 段", summary.segments_today),
            false,
        ),
        (
            IconName::Mic,
            "今日总时长",
            if summary.duration_ms_today > 0 {
                format_duration(summary.duration_ms_today)
            } else {
                "—".into()
            },
            String::new(),
            false,
        ),
        (
            IconName::Clock,
            "平均段落",
            if summary.avg_latency_ms > 0 {
                format_duration(summary.avg_latency_ms)
            } else {
                "—".into()
            },
            "暂无数据".to_string(),
            false,
        ),
        (
            IconName::Bolt,
            "累计记录",
            summary.history_total.to_string(),
            "本机存档".to_string(),
            true,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        metric_card(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(
                    metric_row.left() + index as f32 * (metric_width + gap),
                    metric_row.top(),
                ),
                egui::vec2(metric_width, 108.0),
            ),
            icon,
            label,
            &value,
            &detail,
            accent,
        );
    }
    ui.add_space(18.0);

    // Activity heatmap
    activity_heatmap(ui, width, summary);
    ui.add_space(18.0);

    // Bottom row: period + recent
    let row_width = width - gap;
    let left_width = row_width / 2.4;
    let right_width = row_width - left_width;
    let bottom_row = ui
        .allocate_exact_size(egui::vec2(width, 304.0), egui::Sense::hover())
        .0;
    period_card(
        ui,
        egui::Rect::from_min_size(bottom_row.min, egui::vec2(left_width, 304.0)),
        summary,
    );
    recent_card(
        ui,
        egui::Rect::from_min_size(
            egui::pos2(bottom_row.left() + left_width + gap, bottom_row.top()),
            egui::vec2(right_width, 304.0),
        ),
        summary,
    );
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let minutes = ms / 60_000;
        let seconds = (ms % 60_000) / 1000;
        format!("{}m{}s", minutes, seconds)
    }
}

fn provider_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    kind: &str,
    name: &str,
    configured: bool,
    icon: IconName,
) {
    layout::card_at(ui, rect, |ui| {
        ui.horizontal(|ui| {
            let (icon_rect, _) =
                ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(icon_rect, egui::CornerRadius::same(10), theme::BLUE_SOFT);
            icons::draw_icon(ui, icon_rect.center(), icon, theme::BLUE);
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(kind).size(10.5).color(theme::INK_4));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(name).size(14.0).strong());
                    if configured {
                        ui.label(egui::RichText::new("● 已配置").size(10.5).color(theme::OK));
                    } else {
                        ui.label(egui::RichText::new("未配置").size(10.5).color(theme::INK_4));
                    }
                });
            });
        });
    });
}

fn metric_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    icon: IconName,
    label: &str,
    value: &str,
    detail: &str,
    accent: bool,
) {
    layout::card_at(ui, rect, |ui| {
        ui.horizontal(|ui| {
            icons::draw_icon(
                ui,
                ui.cursor().min + egui::vec2(7.0, 8.0),
                icon,
                theme::INK_3,
            );
            ui.add_space(16.0);
            ui.label(egui::RichText::new(label).size(11.5).color(theme::INK_3));
        });
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(value)
                .size(26.0)
                .strong()
                .color(if accent { theme::BLUE } else { theme::INK }),
        );
        if !detail.is_empty() {
            ui.label(egui::RichText::new(detail).size(10.5).color(theme::INK_4));
        }
    });
}

fn heat_color(count: u32) -> egui::Color32 {
    match count {
        0 => egui::Color32::from_gray(60),
        1..=2 => egui::Color32::from_rgb(80, 140, 220),
        3..=5 => egui::Color32::from_rgb(90, 120, 235),
        6..=10 => egui::Color32::from_rgb(110, 100, 235),
        _ => egui::Color32::from_rgb(150, 90, 235),
    }
}

fn activity_heatmap(ui: &mut egui::Ui, width: f32, summary: &super::view_model::OverviewSummary) {
    let rect = ui
        .allocate_exact_size(egui::vec2(width, 184.0), egui::Sense::hover())
        .0;
    layout::card_at(ui, rect, |ui| {
        ui.label(
            egui::RichText::new("年度活动")
                .size(12.0)
                .strong()
                .color(theme::INK_2),
        );
        ui.add_space(10.0);
        let grid_rect = ui
            .allocate_exact_size(
                egui::vec2(ui.available_width(), 126.0),
                egui::Sense::hover(),
            )
            .0;
        let painter = ui.painter().with_clip_rect(grid_rect);
        let label_width = 30.0;
        let columns = 53;
        let cell_gap = 3.0;
        let column_step = ((grid_rect.width() - label_width) / columns as f32).max(6.0);
        let cell_width = (column_step - cell_gap).max(5.0);
        let cell_height = ((grid_rect.height() - 22.0 - 6.0 * cell_gap) / 7.0).max(5.0);
        let months = [
            "9月", "10月", "11月", "12月", "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月",
        ];
        let month_columns = [0, 4, 9, 13, 18, 22, 27, 31, 36, 40, 45, 49];
        for (index, month) in months.iter().enumerate() {
            let x = grid_rect.left() + label_width + month_columns[index] as f32 * column_step;
            painter.text(
                egui::pos2(x, grid_rect.top()),
                egui::Align2::LEFT_TOP,
                *month,
                egui::FontId::proportional(9.0),
                theme::INK_4,
            );
        }
        for row in 0..7 {
            let y = grid_rect.top() + 22.0 + row as f32 * (cell_height + cell_gap);
            painter.text(
                egui::pos2(grid_rect.left(), y + cell_height / 2.0),
                egui::Align2::LEFT_CENTER,
                match row {
                    0 => "周日",
                    1 => "周一",
                    2 => "周二",
                    3 => "周三",
                    4 => "周四",
                    5 => "周五",
                    _ => "周六",
                },
                egui::FontId::proportional(10.0),
                theme::INK_4,
            );
            for col in 0..columns {
                let intensity = if col < summary.heatmap_weeks.len() as i32 {
                    summary.heatmap_weeks[col as usize][row]
                } else {
                    0
                };
                let color = heat_color(intensity);
                let x = grid_rect.left() + label_width + col as f32 * column_step;
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(cell_width, cell_height),
                    ),
                    egui::CornerRadius::same(2),
                    color,
                );
            }
        }
        ui.horizontal(|ui| {
            ui.label("少");
            for count in [0u32, 1, 4, 8, 15] {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, heat_color(count));
            }
            ui.label("多");
            ui.label(format!(
                "{} 天 · {} 天活跃",
                summary.heatmap_days, summary.activity_days_total
            ));
        });
    });
}

fn period_card(ui: &mut egui::Ui, rect: egui::Rect, summary: &super::view_model::OverviewSummary) {
    layout::card_at(ui, rect, |ui| {
        let row_width = ui.available_width();
        ui.allocate_exact_size(egui::vec2(row_width, 28.0), egui::Sense::hover());
        ui.label(
            egui::RichText::new("近期活动")
                .size(13.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(12.0);
        for (label, segments, _chars, _duration) in [
            ("近 7 天", summary.last_7_segments, "—", "—"),
            ("近 30 天", summary.last_30_segments, "—", "—"),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).size(12.0).color(theme::INK_2));
                ui.label(
                    egui::RichText::new(format!("{} 段", segments))
                        .size(12.0)
                        .color(theme::INK_3),
                );
            });
            ui.add_space(4.0);
        }
    });
}

fn recent_card(ui: &mut egui::Ui, rect: egui::Rect, summary: &super::view_model::OverviewSummary) {
    layout::card_at(ui, rect, |ui| {
        ui.label(
            egui::RichText::new("最近记录")
                .size(13.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(12.0);
        if summary.recent.is_empty() {
            ui.label(
                egui::RichText::new("暂无记录")
                    .size(12.0)
                    .color(theme::INK_4),
            );
            return;
        }
        for entry in &summary.recent {
            ui.label(
                egui::RichText::new(format!(
                    "{} · {}",
                    entry.created_at,
                    entry
                        .duration_ms
                        .map(|d| format_duration(d))
                        .unwrap_or_else(|| "—".into())
                ))
                .size(11.0)
                .color(theme::INK_3),
            );
            let text = if entry.final_text.trim().is_empty() {
                "（无文字）"
            } else {
                &entry.final_text
            };
            ui.label(egui::RichText::new(text).size(12.0).color(theme::INK_2));
            ui.add_space(8.0);
        }
    });
}

// ── History page ────────────────────────────────────────────────────────────

pub fn history_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);

    let header_rect = ui
        .allocate_exact_size(egui::vec2(width, 84.0), egui::Sense::hover())
        .0;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(header_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("HISTORY")
                            .size(11.0)
                            .strong()
                            .color(theme::INK_4),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("历史记录")
                            .size(28.0)
                            .strong()
                            .color(theme::INK),
                    );
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new("本机保存的识别记录。")
                            .size(13.0)
                            .color(theme::INK_3),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let clear = layout::icon_text_button(ui, "清空", IconName::Trash, 70.0);
                    if clear.clicked() {
                        actions.push(FrontendAction::HistoryClear);
                    }
                    ui.add_space(8.0);
                    let refresh = layout::icon_text_button(ui, "刷新", IconName::Refresh, 70.0);
                    if refresh.clicked() {
                        actions.push(FrontendAction::HistoryRefresh);
                    }
                });
            });
        },
    );

    if vm.history_entries.is_empty() {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(if vm.history_cleared {
                            "暂无历史记录"
                        } else {
                            "历史记录暂未接线"
                        })
                        .size(13.0)
                        .color(theme::INK_3),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("数据桥接将在后续阶段完成")
                            .size(11.0)
                            .color(theme::INK_4),
                    );
                });
            });
        return;
    }

    let gap = 14.0;
    let list_width = 300.0;
    let detail_width = (width - list_width - gap).max(300.0);
    let body_height = ui.available_height().max(300.0);
    let body = ui
        .allocate_exact_size(egui::vec2(width, body_height), egui::Sense::hover())
        .0;
    let list_rect = egui::Rect::from_min_size(body.min, egui::vec2(list_width, body.height()));
    let detail_rect = egui::Rect::from_min_size(
        egui::pos2(body.left() + list_width + gap, body.top()),
        egui::vec2(detail_width, body.height()),
    );

    // List panel
    layout::card_at(ui, list_rect, |ui| {
        ui.add_space(1.0);
        let search_width = ui.available_width();
        egui::Frame::new()
            .fill(theme::SURFACE_2)
            .stroke(egui::Stroke::new(0.8, theme::LINE))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_width((search_width - 20.0).max(1.0));
                ui.horizontal(|ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 24.0), egui::Sense::hover());
                    icons::draw_icon(ui, icon_rect.center(), IconName::Search, theme::INK_3);
                    ui.add_space(6.0);
                    let mut query = vm.history_query.clone();
                    let resp = ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("搜索转写内容…")
                            .font(egui::FontId::proportional(12.5))
                            .vertical_align(egui::Align::Center)
                            .frame(false),
                    );
                    if resp.changed() {
                        actions.push(FrontendAction::HistorySearch(query));
                    }
                });
            });
        ui.label(
            egui::RichText::new(format!("共 {} 条记录", vm.history_entries.len()))
                .size(10.5)
                .color(theme::INK_4),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let filters = ["全部", "原文", "轻度润色", "清晰结构", "正式表达"];
            for (index, label) in filters.iter().enumerate() {
                let selected = vm.history_filter == index;
                let filter_width = (label.chars().count() as f32 * 9.0 + 18.0).max(42.0);
                let response = ui.add(
                    egui::Button::new(egui::RichText::new(*label).size(11.5).color(if selected {
                        theme::SURFACE
                    } else {
                        theme::INK_3
                    }))
                    .fill(if selected { theme::INK } else { theme::SURFACE })
                    .stroke(egui::Stroke::new(
                        if selected { 0.0 } else { 0.8 },
                        if selected {
                            egui::Color32::TRANSPARENT
                        } else {
                            theme::LINE
                        },
                    ))
                    .corner_radius(egui::CornerRadius::same(10))
                    .min_size(egui::vec2(filter_width, 24.0)),
                );
                if response.clicked() {
                    actions.push(FrontendAction::HistoryFilter(index));
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("openless-history-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, entry) in vm.history_entries.iter().enumerate() {
                    if !vm.history_query.is_empty()
                        && !entry
                            .text
                            .to_lowercase()
                            .contains(&vm.history_query.to_lowercase())
                    {
                        continue;
                    }
                    let selected = vm.history_selected == index;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 84.0),
                        egui::Sense::click(),
                    );
                    if selected {
                        ui.painter().rect_filled(
                            rect,
                            egui::CornerRadius::same(8),
                            theme::BLUE_SOFT,
                        );
                        let indicator_color = egui::Color32::from_rgb(29, 78, 216);
                        let left = rect.left() + 1.0;
                        let right = rect.left() + 4.0;
                        let top = rect.top() + 2.0;
                        let bottom = rect.bottom() - 2.0;
                        let radius = 3.0;
                        let mut indicator = Vec::with_capacity(18);
                        indicator.push(egui::pos2(right, top));
                        indicator.push(egui::pos2(left + radius, top));
                        for step in 0..=6 {
                            let angle = -std::f32::consts::FRAC_PI_2
                                - std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
                            indicator.push(egui::pos2(
                                left + radius + angle.cos() * radius,
                                top + radius + angle.sin() * radius,
                            ));
                        }
                        indicator.push(egui::pos2(left, bottom - radius));
                        for step in 0..=6 {
                            let angle = std::f32::consts::PI
                                - std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
                            indicator.push(egui::pos2(
                                left + radius + angle.cos() * radius,
                                bottom - radius + angle.sin() * radius,
                            ));
                        }
                        indicator.push(egui::pos2(right, bottom));
                        ui.painter().add(egui::Shape::convex_polygon(
                            indicator,
                            indicator_color,
                            egui::Stroke::NONE,
                        ));
                    }
                    ui.painter().text(
                        rect.min + egui::vec2(12.0, 14.0),
                        egui::Align2::LEFT_CENTER,
                        &entry.time,
                        egui::FontId::monospace(10.5),
                        theme::INK_3,
                    );
                    ui.painter().text(
                        egui::pos2(rect.right() - 12.0, rect.top() + 14.0),
                        egui::Align2::RIGHT_CENTER,
                        &entry.duration,
                        egui::FontId::monospace(10.0),
                        theme::INK_4,
                    );
                    let chars_per_line = (((rect.width() - 24.0) / 11.5).floor() as usize).max(1);
                    let chars: Vec<char> = entry.text.chars().collect();
                    let first_line: String = chars.iter().take(chars_per_line).collect();
                    let mut second_line: String = chars
                        .iter()
                        .skip(chars_per_line)
                        .take(chars_per_line)
                        .collect();
                    if chars.len() > chars_per_line * 2 {
                        second_line.pop();
                        second_line.push('…');
                    }
                    ui.painter().text(
                        rect.min + egui::vec2(12.0, 29.0),
                        egui::Align2::LEFT_TOP,
                        first_line,
                        egui::FontId::proportional(11.5),
                        theme::INK_2,
                    );
                    if !second_line.is_empty() {
                        ui.painter().text(
                            rect.min + egui::vec2(12.0, 44.0),
                            egui::Align2::LEFT_TOP,
                            second_line,
                            egui::FontId::proportional(11.5),
                            theme::INK_2,
                        );
                    }
                    layout::tag(
                        ui,
                        egui::pos2(rect.min.x + 12.0, rect.bottom() - 23.0),
                        &entry.tag,
                        false,
                    );
                    if response.clicked() {
                        actions.push(FrontendAction::HistorySelect(index));
                    }
                    ui.add_space(1.0);
                }
            });
    });

    // Detail panel
    layout::card_at(ui, detail_rect, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("openless-history-detail-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(entry) = vm.history_entries.get(vm.history_selected) {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&entry.time)
                                .size(12.0)
                                .color(theme::INK_3),
                        );
                        ui.add_space(8.0);
                        let _ = layout::small_pill(
                            ui,
                            &entry.tag,
                            theme::SURFACE_2,
                            theme::LINE,
                            theme::INK_3,
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!("录音 {}", entry.duration))
                                .size(11.0)
                                .color(theme::INK_4),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let del = layout::icon_text_button(ui, "删除", IconName::Trash, 70.0);
                            if del.clicked() {
                                actions.push(FrontendAction::HistoryDelete(vm.history_selected));
                            }
                            ui.add_space(8.0);
                            let export =
                                layout::icon_text_button(ui, "导出录音", IconName::Download, 92.0);
                            if export.clicked() {
                                actions.push(FrontendAction::HistoryExport(vm.history_selected));
                            }
                        });
                    });
                    ui.add_space(12.0);
                    layout::soft_separator(ui);
                    ui.add_space(10.0);
                    let play = layout::icon_text_button(
                        ui,
                        if vm.history_audio_playing {
                            "停止播放"
                        } else {
                            "播放录音"
                        },
                        IconName::Play,
                        92.0,
                    );
                    if play.clicked() {
                        actions.push(FrontendAction::HistoryTogglePlay);
                    }
                    if vm.history_audio_playing {
                        ui.label(
                            egui::RichText::new("正在播放录音…")
                                .size(11.0)
                                .color(theme::BLUE),
                        );
                    }
                    ui.add_space(10.0);
                    for (step, _provider, _status) in [
                        ("识别", "ASR 服务", "—"),
                        ("润色", "LLM 服务", "—"),
                        ("插入", "—", "—"),
                    ] {
                        ui.horizontal(|ui| {
                            let _ = layout::small_pill(
                                ui,
                                step,
                                theme::SURFACE_2,
                                theme::LINE,
                                theme::INK_3,
                            );
                            ui.label(
                                egui::RichText::new(_provider)
                                    .size(10.5)
                                    .color(theme::INK_2),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(_status).size(10.5).color(theme::INK_4),
                                    );
                                },
                            );
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(8.0);
                    let inner_width = ui.available_width();
                    let column_gap = 12.0;
                    let column_width = ((inner_width - column_gap) / 2.0).max(120.0);
                    let cards_row = ui
                        .allocate_exact_size(egui::vec2(inner_width, 165.0), egui::Sense::hover())
                        .0;
                    let raw_rect = egui::Rect::from_min_size(
                        cards_row.min,
                        egui::vec2(column_width, cards_row.height()),
                    );
                    let polished_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            cards_row.left() + column_width + column_gap,
                            cards_row.top(),
                        ),
                        egui::vec2(column_width, cards_row.height()),
                    );
                    detail_text_card(ui, raw_rect, "原文", &entry.text, false);
                    detail_text_card(ui, polished_rect, "润色结果", &entry.text, true);
                } else {
                    ui.label("请选择一条记录");
                }
            });
    });
}

fn detail_text_card(ui: &egui::Ui, rect: egui::Rect, title: &str, text: &str, blue: bool) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(10),
        if blue {
            theme::BLUE_SOFT
        } else {
            theme::SURFACE_2
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(0.5, if blue { theme::BLUE } else { theme::LINE }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.min + egui::vec2(14.0, 18.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(10.5),
        if blue { theme::BLUE } else { theme::INK_3 },
    );
    let text_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(14.0, 42.0),
        rect.max - egui::vec2(14.0, 40.0),
    );
    let text_painter = ui.painter().with_clip_rect(text_rect);
    let galley = text_painter.layout(
        text.to_owned(),
        egui::FontId::proportional(12.5),
        theme::INK_2,
        text_rect.width(),
    );
    text_painter.galley(text_rect.left_top(), galley, theme::INK_2);
    let copy = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 58.0, rect.top() + 8.0),
        egui::vec2(48.0, 22.0),
    );
    ui.painter().rect_stroke(
        copy,
        egui::CornerRadius::same(6),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        copy.center(),
        egui::Align2::CENTER_CENTER,
        "复制",
        egui::FontId::proportional(10.5),
        theme::INK_2,
    );
}

// ── Vocab page ──────────────────────────────────────────────────────────────

pub fn vocab_page(ui: &mut egui::Ui, _vm: &FrontendViewModel, _actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);
    ui.label(egui::RichText::new("词汇表").size(11.0).color(theme::INK_4));
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("词汇表")
            .size(28.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("自定义热词，提升专有名词识别率")
            .size(13.0)
            .color(theme::INK_3),
    );
    ui.add_space(24.0);
    layout::unsupported_page(ui, "");
}

// ── Style page ──────────────────────────────────────────────────────────────

pub fn style_page(ui: &mut egui::Ui, vm: &FrontendViewModel, _actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new("润色模式")
            .size(28.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(22.0);

    if vm.style_unsupported {
        layout::unsupported_page(ui, "");
    }
}

// ── Selection ask page ──────────────────────────────────────────────────────

pub fn selection_ask_page(
    ui: &mut egui::Ui,
    vm: &FrontendViewModel,
    _actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new("划词追问")
            .size(28.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(22.0);

    if vm.selection_unsupported {
        layout::unsupported_page(ui, "");
    }
}

// ── Translation page ─────────────────────────────────────────────────────────

pub fn translation_page(
    ui: &mut egui::Ui,
    vm: &FrontendViewModel,
    _actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new("翻译")
            .size(28.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(22.0);

    if vm.translation_unsupported {
        layout::unsupported_page(ui, "");
    }
}
