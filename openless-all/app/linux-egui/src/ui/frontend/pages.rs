use eframe::egui;

use super::icons::{self, IconName};
use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel};

const SUPPORTED_LANGUAGES: [&str; 15] = [
    "简体中文",
    "繁体中文",
    "English",
    "日本語",
    "한국어",
    "Français",
    "Deutsch",
    "Español",
    "Italiano",
    "Português",
    "Русский",
    "العربية",
    "Tiếng Việt",
    "ไทย",
    "हिन्दी",
];

pub fn corrections_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    ui.add_space(28.0);
    ui.label(egui::RichText::new(theme::text("纠错")).size(28.0).strong());
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(theme::text("修正常见识别错误，让每次输入更准确"))
            .color(theme::ink_3()),
    );
    ui.add_space(24.0);
    if !vm.pending_corrections.is_empty() {
        crate::ui::settings::card(ui, "从手动修改中发现", |ui| {
            for suggestion in &vm.pending_corrections {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&suggestion.pattern);
                    ui.label("→");
                    ui.strong(&suggestion.replacement);
                    if ui.button(theme::text("好")).clicked() {
                        actions.push(FrontendAction::AcceptCorrection(suggestion.id.clone()));
                    }
                    if ui.button(theme::text("忽略")).clicked() {
                        actions.push(FrontendAction::RejectCorrection(suggestion.id.clone()));
                    }
                });
            }
        });
    }
    crate::ui::settings::card(ui, "添加替换规则", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut vm.vocab_pattern)
                    .hint_text(theme::text("识别结果"))
                    .desired_width(200.0),
            );
            ui.label("→");
            ui.add(
                egui::TextEdit::singleline(&mut vm.vocab_replacement)
                    .hint_text(theme::text("替换为"))
                    .desired_width(200.0),
            );
            if ui
                .add_enabled(
                    !vm.vocab_pattern.trim().is_empty() && !vm.vocab_replacement.trim().is_empty(),
                    egui::Button::new("添加"),
                )
                .clicked()
            {
                actions.push(FrontendAction::VocabAddRule {
                    pattern: std::mem::take(&mut vm.vocab_pattern),
                    replacement: std::mem::take(&mut vm.vocab_replacement),
                });
            }
        });
    });
    crate::ui::settings::card(ui, "替换规则", |ui| {
        if vm.vocab_rules.is_empty() {
            ui.label(theme::text("还没有替换规则"));
        }
        for (index, rule) in vm.vocab_rules.iter().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let mut enabled = rule.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        actions.push(FrontendAction::VocabToggleRule(index));
                    }
                    ui.label(&rule.pattern);
                    ui.label("→");
                    ui.strong(&rule.replacement);
                    if ui.button(theme::text("删除")).clicked() {
                        actions.push(FrontendAction::VocabRemoveRule(index));
                    }
                })
            });
            ui.separator();
        }
    });
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut value: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        value.push('…');
    }
    value
}

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
            egui::RichText::new(theme::text("今日概览"))
                .size(28.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(theme::text("正在加载概览数据…")).color(theme::ink_3()));
        });
        return;
    }

    if let Some(error) = &vm.overview_error {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new(theme::text("今日概览"))
                .size(28.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(22.0);
        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
        return;
    }

    let Some(summary) = &vm.overview else {
        ui.add_space(28.0);
        ui.label(
            egui::RichText::new(theme::text("今日概览"))
                .size(28.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(22.0);
        ui.label(egui::RichText::new(theme::text("暂无数据")).color(theme::ink_3()));
        return;
    };

    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.add_space(28.0);
    ui.label(
        egui::RichText::new(theme::text("今日概览"))
            .size(28.0)
            .strong()
            .color(theme::ink()),
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
                .rect_filled(icon_rect, egui::CornerRadius::same(10), theme::blue_soft());
            icons::draw_icon(ui, icon_rect.center(), icon, theme::blue());
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(kind).size(10.5).color(theme::ink_4()));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(name).size(14.0).strong());
                    if configured {
                        ui.label(
                            egui::RichText::new(theme::text("● 已配置"))
                                .size(10.5)
                                .color(theme::ok()),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(theme::text("未配置"))
                                .size(10.5)
                                .color(theme::ink_4()),
                        );
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
                theme::ink_3(),
            );
            ui.add_space(16.0);
            ui.label(egui::RichText::new(label).size(11.5).color(theme::ink_3()));
        });
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(value)
                .size(26.0)
                .strong()
                .color(if accent { theme::blue() } else { theme::ink() }),
        );
        if !detail.is_empty() {
            ui.label(egui::RichText::new(detail).size(10.5).color(theme::ink_4()));
        }
    });
}

fn heat_color(count: u32) -> egui::Color32 {
    match count {
        0 => theme::surface_2(),
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
            egui::RichText::new(theme::text("年度活动"))
                .size(12.0)
                .strong()
                .color(theme::ink_2()),
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
                theme::ink_4(),
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
                theme::ink_4(),
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
            ui.label(theme::text("少"));
            for count in [0u32, 1, 4, 8, 15] {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, heat_color(count));
            }
            ui.label(theme::text("多"));
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
            egui::RichText::new(theme::text("近期活动"))
                .size(13.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(12.0);
        for (label, segments, _chars, _duration) in [
            ("近 7 天", summary.last_7_segments, "—", "—"),
            ("近 30 天", summary.last_30_segments, "—", "—"),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).size(12.0).color(theme::ink_2()));
                ui.label(
                    egui::RichText::new(format!("{} 段", segments))
                        .size(12.0)
                        .color(theme::ink_3()),
                );
            });
            ui.add_space(4.0);
        }
    });
}

fn recent_card(ui: &mut egui::Ui, rect: egui::Rect, summary: &super::view_model::OverviewSummary) {
    layout::card_at(ui, rect, |ui| {
        ui.label(
            egui::RichText::new(theme::text("最近记录"))
                .size(13.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(12.0);
        if summary.recent.is_empty() {
            ui.label(
                egui::RichText::new(theme::text("暂无记录"))
                    .size(12.0)
                    .color(theme::ink_4()),
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
                .color(theme::ink_3()),
            );
            let text = if entry.final_text.trim().is_empty() {
                "（无文字）"
            } else {
                &entry.final_text
            };
            ui.label(egui::RichText::new(text).size(12.0).color(theme::ink_2()));
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
                            .color(theme::ink_4()),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(theme::text("历史记录"))
                            .size(28.0)
                            .strong()
                            .color(theme::ink()),
                    );
                    ui.add_space(5.0);
                    ui.label(
                        egui::RichText::new(theme::text("本机保存的识别记录。"))
                            .size(13.0)
                            .color(theme::ink_3()),
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
            .fill(theme::surface())
            .stroke(egui::Stroke::new(1.0, theme::line()))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(theme::text("暂无历史记录"))
                            .size(13.0)
                            .color(theme::ink_3()),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(theme::text("完成一次听写后，记录将显示在这里。"))
                            .size(11.0)
                            .color(theme::ink_4()),
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
            .fill(theme::surface_2())
            .stroke(egui::Stroke::new(0.8, theme::line()))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_width((search_width - 20.0).max(1.0));
                ui.horizontal(|ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 24.0), egui::Sense::hover());
                    icons::draw_icon(ui, icon_rect.center(), IconName::Search, theme::ink_3());
                    ui.add_space(6.0);
                    let mut query = vm.history_query.clone();
                    let resp = ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::TextEdit::singleline(&mut query)
                            .hint_text(theme::text("搜索转写内容…"))
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
                .color(theme::ink_4()),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let filters = ["全部", "原文", "轻度润色", "清晰结构", "正式表达"];
            for (index, label) in filters.iter().enumerate() {
                let selected = vm.history_filter == index;
                let filter_width = (label.chars().count() as f32 * 9.0 + 18.0).max(42.0);
                let response = ui.add(
                    egui::Button::new(egui::RichText::new(*label).size(11.5).color(if selected {
                        theme::surface()
                    } else {
                        theme::ink_3()
                    }))
                    .fill(if selected {
                        theme::ink()
                    } else {
                        theme::surface()
                    })
                    .stroke(egui::Stroke::new(
                        if selected { 0.0 } else { 0.8 },
                        if selected {
                            egui::Color32::TRANSPARENT
                        } else {
                            theme::line()
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
                    if vm.history_filter > 0
                        && entry.mode
                            != ["", "Raw", "Light", "Structured", "Formal"]
                                [vm.history_filter.min(4)]
                    {
                        continue;
                    }
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
                            theme::blue_soft(),
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
                        theme::ink_3(),
                    );
                    ui.painter().text(
                        egui::pos2(rect.right() - 12.0, rect.top() + 14.0),
                        egui::Align2::RIGHT_CENTER,
                        &entry.duration,
                        egui::FontId::monospace(10.0),
                        theme::ink_4(),
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
                        theme::ink_2(),
                    );
                    if !second_line.is_empty() {
                        ui.painter().text(
                            rect.min + egui::vec2(12.0, 44.0),
                            egui::Align2::LEFT_TOP,
                            second_line,
                            egui::FontId::proportional(11.5),
                            theme::ink_2(),
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
                                .color(theme::ink_3()),
                        );
                        ui.add_space(8.0);
                        let _ = layout::small_pill(
                            ui,
                            &entry.tag,
                            theme::surface_2(),
                            theme::line(),
                            theme::ink_3(),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!("录音 {}", entry.duration))
                                .size(11.0)
                                .color(theme::ink_4()),
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
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                !vm.history_busy,
                                egui::Button::new(theme::text("重新润色")),
                            )
                            .clicked()
                        {
                            actions.push(FrontendAction::HistoryRepolish);
                        }
                        if ui
                            .add_enabled(
                                entry.has_audio && !vm.history_busy,
                                egui::Button::new(theme::text("重新转写")),
                            )
                            .clicked()
                        {
                            actions.push(FrontendAction::HistoryRetranscribe);
                        }
                        if vm.history_busy && ui.button(theme::text("取消")).clicked() {
                            actions.push(FrontendAction::HistoryCancel);
                        }
                        if ui.button(theme::text("复制结果")).clicked() {
                            ui.ctx().copy_text(entry.text.clone());
                        }
                    });
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
                            egui::RichText::new(theme::text("正在播放录音…"))
                                .size(11.0)
                                .color(theme::blue()),
                        );
                    }
                    ui.add_space(10.0);
                    for (step, _provider, _status) in [
                        (
                            "识别",
                            entry.asr.as_str(),
                            entry.asr_ms.map(|n| format!("{n} ms")).unwrap_or_default(),
                        ),
                        (
                            "润色",
                            entry.llm.as_str(),
                            entry
                                .polish_ms
                                .map(|n| format!("{n} ms"))
                                .unwrap_or_default(),
                        ),
                        ("插入", entry.tag.as_str(), String::new()),
                    ] {
                        ui.horizontal(|ui| {
                            let _ = layout::small_pill(
                                ui,
                                step,
                                theme::surface_2(),
                                theme::line(),
                                theme::ink_3(),
                            );
                            ui.label(
                                egui::RichText::new(_provider)
                                    .size(10.5)
                                    .color(theme::ink_2()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(_status)
                                            .size(10.5)
                                            .color(theme::ink_4()),
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
                    detail_text_card(ui, raw_rect, "原文", &entry.raw, false);
                    detail_text_card(ui, polished_rect, "润色结果", &entry.text, true);
                    ui.add_space(16.0);
                    egui::ComboBox::from_id_salt("history-polish-style")
                        .selected_text(
                            vm.style_packs
                                .iter()
                                .find(|p| p.id == vm.history_repolish_style)
                                .map(|p| p.name.as_str())
                                .unwrap_or("当前风格"),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut vm.history_repolish_style,
                                String::new(),
                                theme::text("当前风格"),
                            );
                            for pack in &vm.style_packs {
                                ui.selectable_value(
                                    &mut vm.history_repolish_style,
                                    pack.id.clone(),
                                    &pack.name,
                                );
                            }
                        });
                    if let Some(results) = vm.history_results.get(&entry.id) {
                        for result in results {
                            crate::ui::settings::card(ui, "重新润色结果", |ui| {
                                ui.label(result);
                                if ui.button(theme::text("复制")).clicked() {
                                    ui.ctx().copy_text(result.clone());
                                }
                            });
                        }
                    }
                } else {
                    ui.label(theme::text("请选择一条记录"));
                }
            });
    });
}

fn detail_text_card(ui: &egui::Ui, rect: egui::Rect, title: &str, text: &str, blue: bool) {
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(10),
        if blue {
            theme::blue_soft()
        } else {
            theme::surface_2()
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(0.5, if blue { theme::blue() } else { theme::line() }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.min + egui::vec2(14.0, 18.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(10.5),
        if blue { theme::blue() } else { theme::ink_3() },
    );
    let text_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(14.0, 42.0),
        rect.max - egui::vec2(14.0, 40.0),
    );
    let text_painter = ui.painter().with_clip_rect(text_rect);
    let galley = text_painter.layout(
        text.to_owned(),
        egui::FontId::proportional(12.5),
        theme::ink_2(),
        text_rect.width(),
    );
    text_painter.galley(text_rect.left_top(), galley, theme::ink_2());
    let copy = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 58.0, rect.top() + 8.0),
        egui::vec2(48.0, 22.0),
    );
    ui.painter().rect_stroke(
        copy,
        egui::CornerRadius::same(6),
        egui::Stroke::new(0.5, theme::line()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        copy.center(),
        egui::Align2::CENTER_CENTER,
        "复制",
        egui::FontId::proportional(10.5),
        theme::ink_2(),
    );
    if ui
        .interact(
            copy,
            ui.id().with(("history-copy", title)),
            egui::Sense::click(),
        )
        .clicked()
    {
        ui.ctx().copy_text(text.to_owned());
    }
}

// ── Vocab page ──────────────────────────────────────────────────────────────

pub fn vocab_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.label(
        egui::RichText::new(theme::text("词汇表"))
            .size(11.0)
            .color(theme::ink_4()),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(theme::text("词汇表"))
                .size(28.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(theme::text("自定义热词，提升专有名词识别率"))
                .size(13.0)
                .color(theme::ink_3()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(theme::text("↻  刷新"))
                            .size(11.5)
                            .color(theme::ink_2()),
                    )
                    .fill(theme::surface())
                    .stroke(egui::Stroke::new(0.8, theme::line()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(70.0, 30.0)),
                )
                .clicked()
            {
                vm.vocab_error = None;
                actions.push(FrontendAction::VocabRefresh);
            }
        });
    });
    ui.add_space(24.0);

    if vm.vocab_unsupported {
        layout::unsupported_page(ui, "");
        return;
    }

    // Presets card
    vocab_card(
        ui,
        width,
        "预设",
        "选择一组常用词汇快速添加。",
        &mut vm.vocab_presets_open,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                let preset_names: Vec<&str> = vm
                    .vocab_saved_presets
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();
                for (index, name) in preset_names.iter().enumerate() {
                    let selected = vm.vocab_selected_presets.contains(&index);
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new(*name).size(12.5))
                            .fill(if selected {
                                theme::blue_soft()
                            } else {
                                theme::surface_2()
                            })
                            .stroke(egui::Stroke::new(0.5, theme::line()))
                            .corner_radius(egui::CornerRadius::same(12))
                            .min_size(egui::vec2(88.0, 32.0)),
                    );
                    if response.clicked() {
                        actions.push(FrontendAction::VocabApplyPreset(index));
                    }
                }
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(theme::text("创建预设")).size(12.5))
                            .fill(theme::surface())
                            .stroke(egui::Stroke::new(0.5, theme::line()))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(96.0, 34.0)),
                    )
                    .clicked()
                {
                    vm.vocab_editing_preset = Some(usize::MAX);
                    vm.vocab_preset_name = "新预设".into();
                    vm.vocab_preset_phrases.clear();
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(theme::text("应用"))
                                .color(theme::surface())
                                .size(12.0),
                        )
                        .fill(theme::ink())
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(64.0, 32.0)),
                    )
                    .clicked()
                {
                    actions.push(FrontendAction::VocabApplyPreset(usize::MAX));
                }
            });
            if vm.vocab_editing_preset.is_some() {
                ui.add_space(10.0);
                let input_width = ui.available_width();
                let input_content_width = (input_width - 20.0).max(1.0);
                egui::Frame::new()
                    .fill(theme::surface())
                    .stroke(egui::Stroke::new(0.8, theme::line()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(input_content_width);
                        ui.add_sized(
                            [input_content_width, 20.0],
                            egui::TextEdit::singleline(&mut vm.vocab_preset_name)
                                .hint_text(theme::text("预设名称"))
                                .desired_width(input_content_width)
                                .frame(false),
                        );
                    });
                egui::Frame::new()
                    .fill(theme::surface())
                    .stroke(egui::Stroke::new(0.8, theme::line()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(input_content_width);
                        ui.add_sized(
                            [input_content_width, 64.0],
                            egui::TextEdit::multiline(&mut vm.vocab_preset_phrases)
                                .desired_rows(3)
                                .desired_width(input_content_width)
                                .hint_text(theme::text("词汇，用逗号或换行分隔"))
                                .frame(false),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(theme::text("保存"))
                                    .color(theme::surface())
                                    .size(12.0),
                            )
                            .fill(theme::ink())
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(72.0, 30.0)),
                        )
                        .clicked()
                    {
                        let name = vm.vocab_preset_name.trim().to_owned();
                        let phrases = vm.vocab_preset_phrases.clone();
                        if !name.is_empty() {
                            let id = vm
                                .vocab_editing_preset
                                .and_then(|i| vm.vocab_saved_presets.get(i))
                                .map(|p| p.id.clone());
                            actions.push(FrontendAction::VocabCreatePreset { id, name, phrases });
                        }
                        vm.vocab_editing_preset = None;
                    }
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(theme::text("取消")).size(12.0))
                                .fill(theme::surface())
                                .stroke(egui::Stroke::new(0.8, theme::line()))
                                .corner_radius(egui::CornerRadius::same(8))
                                .min_size(egui::vec2(72.0, 30.0)),
                        )
                        .clicked()
                    {
                        vm.vocab_editing_preset = None;
                    }
                });
            }
            if vm.vocab_editing_preset.is_none() && !vm.vocab_saved_presets.is_empty() {
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    let saved_presets = vm.vocab_saved_presets.clone();
                    for (index, preset) in saved_presets.iter().enumerate() {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(format!("编辑  {}", preset.name))
                                        .size(12.5),
                                )
                                .fill(theme::surface_2())
                                .stroke(egui::Stroke::new(0.6, theme::line()))
                                .corner_radius(egui::CornerRadius::same(14))
                                .min_size(egui::vec2(92.0, 30.0)),
                            )
                            .clicked()
                        {
                            vm.vocab_preset_name = preset.name.clone();
                            vm.vocab_preset_phrases = preset.phrases.clone();
                            vm.vocab_editing_preset = Some(index);
                        }
                        if ui.small_button(theme::text("删除")).clicked() {
                            actions.push(FrontendAction::VocabDeletePreset(index));
                        }
                    }
                });
            }
        },
    );

    // Correction rules card
    vocab_card(
        ui,
        width,
        "纠错规则",
        "将识别结果中的常见错误自动替换为正确写法。",
        &mut vm.vocab_corrections_open,
        |ui| {
            ui.horizontal(|ui| {
                let spacing = ui.spacing().item_spacing.x;
                let add_width = 72.0;
                let arrow_width = 24.0;
                let input_width =
                    ((ui.available_width() - add_width - arrow_width - spacing * 3.0) / 2.0)
                        .max(60.0);
                let input_content_width = (input_width - 20.0).max(1.0);
                egui::Frame::new()
                    .fill(theme::surface_2())
                    .stroke(egui::Stroke::new(0.8, theme::line()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(input_content_width);
                        ui.add_sized(
                            [input_content_width, 20.0],
                            egui::TextEdit::singleline(&mut vm.vocab_pattern)
                                .desired_width(input_content_width)
                                .hint_text(theme::text("原文，例如：{num}粒"))
                                .frame(false),
                        );
                    });
                ui.add_sized(
                    [arrow_width, 32.0],
                    egui::Label::new(egui::RichText::new("→").color(theme::ink_4()))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
                egui::Frame::new()
                    .fill(theme::surface_2())
                    .stroke(egui::Stroke::new(0.8, theme::line()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(input_content_width);
                        ui.add_sized(
                            [input_content_width, 20.0],
                            egui::TextEdit::singleline(&mut vm.vocab_replacement)
                                .desired_width(input_content_width)
                                .hint_text(theme::text("替换为"))
                                .frame(false),
                        );
                    });
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(theme::text("添加"))
                                .color(theme::surface())
                                .size(12.0),
                        )
                        .fill(theme::ink())
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(add_width, 32.0)),
                    )
                    .clicked()
                    && !vm.vocab_pattern.trim().is_empty()
                {
                    actions.push(FrontendAction::VocabAddRule {
                        pattern: vm.vocab_pattern.trim().into(),
                        replacement: vm.vocab_replacement.trim().into(),
                    });
                    vm.vocab_pattern.clear();
                    vm.vocab_replacement.clear();
                }
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                let mut remove_index = None;
                for index in 0..vm.vocab_rules.len() {
                    let rule = &vm.vocab_rules[index];
                    let label = format!(
                        "{} → {}{}",
                        rule.pattern,
                        rule.replacement,
                        if rule.learned { "  自动" } else { "" }
                    );
                    let (toggle, remove) = correction_chip(ui, &label, rule.enabled);
                    if remove {
                        remove_index = Some(index);
                        break;
                    }
                    if toggle {
                        actions.push(FrontendAction::VocabToggleRule(index));
                    }
                }
                if let Some(index) = remove_index {
                    actions.push(FrontendAction::VocabRemoveRule(index));
                }
                if vm.vocab_rules.is_empty() {
                    ui.label(
                        egui::RichText::new(theme::text("暂无纠错规则"))
                            .size(12.0)
                            .color(theme::ink_4()),
                    );
                }
            });
        },
    );

    // Vocabulary entries card
    vocab_card(
        ui,
        width,
        "词汇",
        "添加需要优先识别的自定义词汇。",
        &mut vm.vocab_entries_open,
        |ui| {
            ui.horizontal(|ui| {
                let input_width = (ui.available_width() - 90.0).max(80.0);
                let input_content_width = (input_width - 20.0).max(1.0);
                egui::Frame::new()
                    .fill(theme::surface())
                    .stroke(egui::Stroke::new(0.8, theme::line()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(input_content_width);
                        let resp = ui.add_sized(
                            [input_content_width, 20.0],
                            egui::TextEdit::singleline(&mut vm.vocab_input)
                                .desired_width(input_content_width)
                                .hint_text(theme::text("输入词汇，按回车添加"))
                                .frame(false),
                        );
                        if resp.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            && !vm.vocab_input.trim().is_empty()
                        {
                            let phrase = vm.vocab_input.trim().to_string();
                            if !phrase.is_empty() {
                                actions.push(FrontendAction::VocabAddPhrase(phrase));
                            }
                            vm.vocab_input.clear();
                        }
                    });
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(theme::text("＋ 添加"))
                                .color(theme::surface())
                                .size(12.0),
                        )
                        .fill(theme::ink())
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(78.0, 32.0)),
                    )
                    .clicked()
                    || (ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && !vm.vocab_input.trim().is_empty())
                {
                    let phrase = vm.vocab_input.trim().to_string();
                    if !phrase.is_empty() {
                        actions.push(FrontendAction::VocabAddPhrase(phrase));
                    }
                    vm.vocab_input.clear();
                }
            });
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                let mut remove_index = None;
                for index in 0..vm.vocab_entries.len() {
                    let entry = &vm.vocab_entries[index];
                    let (toggle, remove) = vocab_chip(ui, entry);
                    if remove {
                        remove_index = Some(index);
                        break;
                    }
                    if toggle {
                        actions.push(FrontendAction::VocabTogglePhrase(index));
                    }
                }
                if let Some(index) = remove_index {
                    actions.push(FrontendAction::VocabRemovePhrase(index));
                }
            });
            let learned = vm
                .vocab_entries
                .iter()
                .filter(|entry| entry.learned)
                .count();
            if learned > 0 {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("自动收集 ({learned})"));
                    if ui.button(theme::text("全部删除")).clicked() {
                        // Remove all learned entries
                        let indices: Vec<usize> = vm
                            .vocab_entries
                            .iter()
                            .enumerate()
                            .filter(|(_, e)| e.learned)
                            .map(|(i, _)| i)
                            .rev()
                            .collect();
                        for i in indices {
                            actions.push(FrontendAction::VocabRemovePhrase(i));
                        }
                    }
                });
            }
            if let Some(error) = &vm.vocab_error {
                ui.label(
                    egui::RichText::new(error)
                        .size(12.0)
                        .color(egui::Color32::from_rgb(185, 28, 28)),
                );
            }
        },
    );
}

// ── Style page ──────────────────────────────────────────────────────────────

pub fn style_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);

    if vm.style_unsupported {
        layout::unsupported_page(ui, "");
        return;
    }

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(theme::text("选择润色风格，让每次输出都保持一致"))
                .size(12.0)
                .color(theme::ink_3()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let import = ui.add(
                egui::Button::new(egui::RichText::new(theme::text("▣  导入 ZIP")).size(11.5))
                    .fill(theme::blue())
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(92.0, 29.0)),
            );
            if import.clicked() {
                actions.push(FrontendAction::StyleImport);
            }
            ui.add_space(8.0);
            let refresh = ui.add(
                egui::Button::new(
                    egui::RichText::new(theme::text("↻  刷新"))
                        .size(11.5)
                        .color(theme::ink_2()),
                )
                .fill(theme::surface())
                .stroke(egui::Stroke::new(0.7, theme::line()))
                .corner_radius(egui::CornerRadius::same(8))
                .min_size(egui::vec2(70.0, 29.0)),
            );
            if refresh.clicked() {
                actions.push(FrontendAction::StyleRefresh);
            }
        });
    });
    ui.add_space(14.0);

    let mut selected_action: Option<usize> = None;

    let style_card_height = ui.available_height().max(320.0);
    layout::card_at(
        ui,
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, style_card_height)),
        |ui| {
            let raw_active = !vm.style_selection_workflow
                && vm
                    .style_packs
                    .iter()
                    .any(|p| p.id == "builtin.raw" && p.is_active);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(theme::text("本地风格包"))
                                .size(15.0)
                                .strong(),
                        );
                        let raw = ui.add(
                            egui::Button::new(
                                egui::RichText::new(theme::text("原文")).size(11.5).color(
                                    if raw_active {
                                        theme::surface()
                                    } else {
                                        theme::ink_3()
                                    },
                                ),
                            )
                            .fill(if raw_active {
                                theme::blue()
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(egui::vec2(52.0, 24.0)),
                        );
                        if raw.clicked() {
                            selected_action = Some(usize::MAX);
                            vm.style_selection_workflow = false;
                        }
                    });
                    ui.add_space(3.0);
                    ui.label(
                        egui::RichText::new(theme::text("浏览和切换风格包。"))
                            .size(11.5)
                            .color(theme::ink_3()),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::new()
                        .fill(theme::surface())
                        .stroke(egui::Stroke::new(0.8, theme::line()))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(7, 3))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} 个风格包", vm.style_packs.len()))
                                    .size(10.5)
                                    .color(theme::ink_3()),
                            );
                        });
                    let selection = ui.add(
                        egui::Button::new(
                            egui::RichText::new(theme::text("选区润色"))
                                .size(11.5)
                                .color(if vm.style_selection_workflow {
                                    theme::surface()
                                } else {
                                    theme::ink_3()
                                }),
                        )
                        .fill(if vm.style_selection_workflow {
                            theme::blue()
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(70.0, 24.0)),
                    );
                    if selection.clicked() {
                        vm.style_selection_workflow = true;
                    }
                    let dictation = ui.add(
                        egui::Button::new(
                            egui::RichText::new(theme::text("语音润色"))
                                .size(11.5)
                                .color(if !vm.style_selection_workflow && !raw_active {
                                    theme::surface()
                                } else {
                                    theme::ink_3()
                                }),
                        )
                        .fill(if !vm.style_selection_workflow && !raw_active {
                            theme::blue()
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(6))
                        .min_size(egui::vec2(70.0, 24.0)),
                    );
                    if dictation.clicked() {
                        vm.style_selection_workflow = false;
                    }
                });
            });
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(16.0);

            egui::ScrollArea::vertical()
                .id_salt("style-packs-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let grid_width = ui.available_width();
                    let gap = 12.0;
                    let columns = if grid_width >= 820.0 {
                        3
                    } else if grid_width >= 560.0 {
                        2
                    } else {
                        1
                    };
                    let card_width =
                        ((grid_width - gap * (columns - 1) as f32) / columns as f32).max(1.0);
                    let indices: Vec<usize> = vm
                        .style_packs
                        .iter()
                        .enumerate()
                        .filter_map(|(i, p)| (p.id != "builtin.raw").then_some(i))
                        .collect();
                    let total_tiles = indices.len() + 1;
                    for (row_index, start) in (0..total_tiles).step_by(columns).enumerate() {
                        if row_index > 0 {
                            ui.add_space(12.0);
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for slot in start..(start + columns).min(total_tiles) {
                                if slot == indices.len() {
                                    if new_style_pack_card(ui, egui::vec2(card_width, 232.0)) {
                                        actions.push(FrontendAction::StyleNewPack);
                                    }
                                    continue;
                                }
                                let index = indices[slot];
                                let pack = &vm.style_packs[index];
                                match style_pack_card(
                                    ui,
                                    egui::vec2(card_width, 232.0),
                                    index,
                                    vm.style_selected,
                                    &pack.name,
                                    &pack.description,
                                    &pack.tags,
                                    pack.accent,
                                    pack.is_active,
                                    pack.is_builtin,
                                ) {
                                    StyleCardAction::Activate => selected_action = Some(index),
                                    StyleCardAction::Export => {
                                        actions.push(FrontendAction::StyleExport(index));
                                    }
                                    StyleCardAction::Edit => {
                                        actions.push(FrontendAction::StyleEdit(index));
                                    }
                                    StyleCardAction::None => {}
                                }
                                if slot + 1 < (start + columns).min(total_tiles) {
                                    ui.add_space(gap);
                                }
                            }
                        });
                    }
                });
        },
    );

    if let Some(index) = selected_action {
        actions.push(FrontendAction::StyleActivate(index));
    }

    if let Some(notice) = vm.style_notice.clone() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("✓").color(theme::ok()).strong());
            ui.label(egui::RichText::new(notice).size(11.5).color(theme::ink_2()));
            if ui.small_button("×").clicked() {
                vm.style_notice = None;
            }
        });
    }
    style_editor_overlay(ui.ctx(), vm, actions);
}

// ── Selection ask page ──────────────────────────────────────────────────────

pub fn selection_ask_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);

    if vm.selection_unsupported {
        layout::unsupported_page(ui, "");
        return;
    }

    // History toggle
    let history_width = 142.0;
    let history_rect = ui
        .allocate_exact_size(egui::vec2(history_width, 36.0), egui::Sense::hover())
        .0;
    ui.painter().rect_filled(
        history_rect,
        egui::CornerRadius::same(10),
        egui::Color32::from_rgb(241, 241, 242),
    );
    ui.painter().rect_stroke(
        history_rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(0.5, theme::line()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        history_rect.min + egui::vec2(14.0, 18.0),
        egui::Align2::LEFT_CENTER,
        "保存历史",
        egui::FontId::proportional(12.5),
        theme::ink_2(),
    );

    let toggle_rect = egui::Rect::from_min_size(
        egui::pos2(history_rect.right() - 50.0, history_rect.center().y - 10.0),
        egui::vec2(36.0, 20.0),
    );
    let toggle = ui.interact(
        toggle_rect,
        ui.id().with("selection-ask-history"),
        egui::Sense::click(),
    );
    let toggle_color = if vm.qa_save_history {
        theme::blue()
    } else {
        egui::Color32::from_rgb(184, 184, 187)
    };
    ui.painter()
        .rect_filled(toggle_rect, egui::CornerRadius::same(10), toggle_color);
    let knob_x = if vm.qa_save_history {
        toggle_rect.right() - 10.0
    } else {
        toggle_rect.left() + 10.0
    };
    ui.painter().circle_filled(
        egui::pos2(knob_x, toggle_rect.center().y),
        8.0,
        egui::Color32::WHITE,
    );
    if toggle.clicked() {
        actions.push(FrontendAction::SelectionAskToggleHistory);
    }
    ui.add_space(12.0);

    // Usage card
    layout::card_at(
        ui,
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, 196.0)),
        |ui| {
            ui.label(
                egui::RichText::new(theme::text("使用方法"))
                    .size(13.0)
                    .strong()
                    .color(theme::ink()),
            );
            ui.add_space(10.0);
            let steps = [
                "按设置中的划词追问快捷键打开浮窗。",
                "在任意 app 选中文字。",
                "按听写快捷键录音，松开或再次按下提交。",
                "可继续使用听写快捷键进行多轮追问。",
                "按 Esc 关闭浮窗并清空历史。",
            ];
            for (index, step) in steps.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [18.0, 20.0],
                        egui::Label::new(
                            egui::RichText::new(format!("{:}.", index + 1))
                                .size(12.5)
                                .color(theme::ink_3()),
                        ),
                    );
                    ui.label(egui::RichText::new(step).size(12.5).color(theme::ink_2()));
                });
                if index < 4 {
                    ui.add_space(5.0);
                }
            }
        },
    );
}

// ── Translation page ─────────────────────────────────────────────────────────

pub fn translation_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);

    if vm.translation_unsupported {
        layout::unsupported_page(ui, "");
        return;
    }

    let gap = 12.0;
    let card_width = width.min(760.0);

    // Working languages card
    translation_card(ui, card_width, |ui| {
        ui.label(
            egui::RichText::new(theme::text("工作语言"))
                .size(13.0)
                .strong(),
        );
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for language in SUPPORTED_LANGUAGES {
                let selected = vm
                    .translation_working_languages
                    .iter()
                    .any(|value| value == language);
                let response = ui.add(
                    egui::Button::new(egui::RichText::new(language).size(12.5).color(
                        if selected {
                            egui::Color32::WHITE
                        } else {
                            theme::ink_2()
                        },
                    ))
                    .fill(if selected {
                        theme::blue()
                    } else {
                        theme::surface_2()
                    })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(255))
                    .min_size(egui::vec2(0.0, 28.0)),
                );
                if response.clicked() {
                    actions.push(FrontendAction::TranslationToggleLanguage(
                        language.to_string(),
                    ));
                }
            }
        });
    });
    ui.add_space(gap);

    // Target language card
    let target = vm.translation_target_language.clone();
    let redundant = !target.is_empty()
        && vm.translation_working_languages.len() == 1
        && vm.translation_working_languages[0] == target;
    let enabled = !target.is_empty() && !redundant;
    translation_card(ui, card_width, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(theme::text("翻译目标语言"))
                    .size(13.0)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(if enabled { "已启用" } else { "未启用" })
                        .size(10.5)
                        .strong()
                        .color(if enabled {
                            theme::blue()
                        } else {
                            theme::ink_4()
                        }),
                );
            });
        });
        ui.add_space(12.0);
        ui.scope(|ui| {
            let style = ui.style_mut();
            style.visuals.menu_corner_radius = egui::CornerRadius::same(10);
            style.spacing.button_padding = egui::vec2(10.0, 0.0);
            style.spacing.icon_spacing = 8.0;
            style.spacing.icon_width = 11.0;
            for widget in [
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                widget.corner_radius = egui::CornerRadius::same(8);
                widget.weak_bg_fill = theme::surface();
                widget.bg_fill = theme::surface();
                widget.bg_stroke = egui::Stroke::new(0.8, theme::line());
                widget.fg_stroke = egui::Stroke::new(1.0, theme::ink_2());
            }
            let mut selected_target = target.clone();
            egui::ComboBox::from_id_salt("translation-target-language")
                .width(360.0)
                .height(32.0)
                .truncate()
                .icon(|ui, rect, visuals, _| {
                    let center = rect.center();
                    let stroke = egui::Stroke::new(1.1, visuals.fg_stroke.color);
                    ui.painter()
                        .line_segment([center + egui::vec2(-3.5, -1.5), center], stroke);
                    ui.painter()
                        .line_segment([center, center + egui::vec2(3.5, -1.5)], stroke);
                })
                .selected_text(if target.is_empty() {
                    egui::RichText::new(theme::text("不启用（Shift 按下不触发翻译）"))
                        .color(theme::ink_4())
                } else {
                    egui::RichText::new(target.as_str()).color(theme::ink())
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            selected_target.is_empty(),
                            "不启用（Shift 按下不触发翻译）",
                        )
                        .clicked()
                    {
                        selected_target = String::new();
                        ui.close();
                    }
                    for language in SUPPORTED_LANGUAGES {
                        if ui
                            .selectable_label(selected_target == language, language)
                            .clicked()
                        {
                            selected_target = language.to_string();
                            ui.close();
                        }
                    }
                });
            if selected_target != target {
                actions.push(FrontendAction::TranslationSetTarget(selected_target));
            }
        });
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(theme::text("翻译风格"))
                        .size(12.0)
                        .strong(),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(theme::text("自动继承“风格”页当前激活的风格包。"))
                        .size(11.5)
                        .color(theme::ink_4()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let style_name = if let Some(pack) = vm.style_packs.get(vm.style_selected) {
                    pack.name.as_str()
                } else if vm.style_selected == usize::MAX {
                    "原样保留"
                } else {
                    "轻度润色"
                };
                egui::Frame::new()
                    .fill(theme::blue_soft())
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(9, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(style_name)
                                .size(11.0)
                                .strong()
                                .color(theme::blue()),
                        );
                    });
            });
        });
        if redundant {
            ui.add_space(10.0);
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(217, 119, 6, 20))
                .stroke(egui::Stroke::new(
                    0.5,
                    egui::Color32::from_rgba_unmultiplied(217, 119, 6, 62),
                ))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "目标语言与唯一工作语言相同，按翻译快捷键不会触发翻译。",
                        )
                        .size(11.5)
                        .color(egui::Color32::from_rgb(180, 103, 10)),
                    );
                });
        }
    });
    ui.add_space(gap);

    // Usage card
    translation_card(ui, card_width, |ui| {
        ui.label(
            egui::RichText::new(theme::text("使用方法"))
                .size(13.0)
                .strong(),
        );
        ui.add_space(10.0);
        for (number, text) in [
            ("1", "按右 Option 开始录音。"),
            ("2", "再次按右 Option 停止录音。"),
            ("3", "录音过程中按翻译快捷键切换到翻译模式。"),
            ("4", "松开按键后，译文会自动插入当前应用。"),
            ("5", "翻译模式会在胶囊顶部显示状态。"),
        ] {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{number}."))
                        .size(12.5)
                        .color(theme::ink_3()),
                );
                ui.label(egui::RichText::new(text).size(12.5).color(theme::ink_2()));
            });
            ui.add_space(4.0);
        }
    });
}

// ── Vocab helpers ───────────────────────────────────────────────────────────

fn vocab_card(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    desc: &str,
    open: &mut bool,
    contents: impl FnOnce(&mut egui::Ui),
) {
    let frame = egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::line()))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(17));
    frame.show(ui, |ui| {
        ui.set_width(width - 34.0);
        let header = ui.horizontal(|ui| {
            let (arrow_rect, response) =
                ui.allocate_exact_size(egui::vec2(20.0, 24.0), egui::Sense::click());
            let arrow_stroke = egui::Stroke::new(1.4, theme::ink_4());
            let center = arrow_rect.center();
            if *open {
                ui.painter().line_segment(
                    [
                        center + egui::vec2(-4.0, -2.0),
                        center + egui::vec2(0.0, 2.0),
                    ],
                    arrow_stroke,
                );
                ui.painter().line_segment(
                    [
                        center + egui::vec2(0.0, 2.0),
                        center + egui::vec2(4.0, -2.0),
                    ],
                    arrow_stroke,
                );
            } else {
                ui.painter().line_segment(
                    [
                        center + egui::vec2(-2.0, -4.0),
                        center + egui::vec2(2.0, 0.0),
                    ],
                    arrow_stroke,
                );
                ui.painter().line_segment(
                    [
                        center + egui::vec2(2.0, 0.0),
                        center + egui::vec2(-2.0, 4.0),
                    ],
                    arrow_stroke,
                );
            }
            if response.clicked() {
                *open = !*open;
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(title).size(13.0).strong());
                ui.label(egui::RichText::new(desc).size(11.5).color(theme::ink_4()));
            });
        });
        let _ = header;
        if *open {
            ui.add_space(12.0);
            contents(ui);
        }
    });
    ui.add_space(12.0);
}

fn correction_chip(ui: &mut egui::Ui, label: &str, enabled: bool) -> (bool, bool) {
    let fill = if enabled {
        theme::surface()
    } else {
        theme::surface_2()
    };
    let text_color = if enabled {
        theme::ink()
    } else {
        theme::ink_4()
    };
    let text_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.5),
        text_color,
    );
    let close_size = 22.0;
    let width = 12.0 + text_galley.size().x + 8.0 + close_size + 10.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(16), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(16),
        egui::Stroke::new(0.6, theme::line()),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        egui::pos2(
            rect.left() + 12.0,
            rect.center().y - text_galley.size().y / 2.0,
        ),
        text_galley,
        text_color,
    );

    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
        egui::vec2(close_size, close_size),
    );
    painter.circle_filled(close_rect.center(), close_size / 2.0, theme::surface_2());
    painter.circle_stroke(
        close_rect.center(),
        close_size / 2.0,
        egui::Stroke::new(0.5, theme::line()),
    );
    let center = close_rect.center();
    let x_stroke = egui::Stroke::new(1.1, theme::ink_4());
    painter.line_segment(
        [
            center + egui::vec2(-3.0, -3.0),
            center + egui::vec2(3.0, 3.0),
        ],
        x_stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(3.0, -3.0),
            center + egui::vec2(-3.0, 3.0),
        ],
        x_stroke,
    );

    if response.clicked() {
        if response
            .interact_pointer_pos()
            .is_some_and(|pointer| close_rect.contains(pointer))
        {
            return (false, true);
        }
        return (true, false);
    }
    (false, false)
}

fn vocab_chip(ui: &mut egui::Ui, entry: &super::view_model::VocabEntry) -> (bool, bool) {
    let fill = if entry.enabled && entry.hits > 0 {
        theme::blue_soft()
    } else if entry.enabled {
        theme::surface()
    } else {
        theme::surface_2()
    };
    let text_color = if entry.enabled {
        theme::ink()
    } else {
        theme::ink_4()
    };
    let phrase_galley = ui.painter().layout_no_wrap(
        entry.phrase.clone(),
        egui::FontId::proportional(13.0),
        text_color,
    );
    let hits_text = entry.hits.to_string();
    let hits_color = if entry.enabled && entry.hits > 0 {
        theme::surface()
    } else {
        theme::ink_4()
    };
    let hits_galley =
        ui.painter()
            .layout_no_wrap(hits_text, egui::FontId::proportional(11.0), hits_color);
    let hits_size = egui::vec2((hits_galley.size().x + 12.0).max(24.0), 22.0);
    let close_size = 22.0;
    let width = 12.0 + phrase_galley.size().x + 8.0 + hits_size.x + 6.0 + close_size + 10.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(16), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(16),
        egui::Stroke::new(0.6, theme::line()),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        egui::pos2(
            rect.left() + 12.0,
            rect.center().y - phrase_galley.size().y / 2.0,
        ),
        phrase_galley,
        text_color,
    );

    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 10.0 - close_size / 2.0, rect.center().y),
        egui::vec2(close_size, close_size),
    );
    let hits_rect = egui::Rect::from_min_size(
        egui::pos2(
            close_rect.left() - 6.0 - hits_size.x,
            rect.center().y - hits_size.y / 2.0,
        ),
        hits_size,
    );
    painter.rect_filled(
        hits_rect,
        egui::CornerRadius::same(5),
        if entry.enabled && entry.hits > 0 {
            theme::blue()
        } else {
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 15)
        },
    );
    painter.galley(
        egui::pos2(
            hits_rect.center().x - hits_galley.size().x / 2.0,
            hits_rect.center().y - hits_galley.size().y / 2.0,
        ),
        hits_galley,
        hits_color,
    );
    painter.circle_filled(close_rect.center(), close_size / 2.0, theme::surface_2());
    painter.circle_stroke(
        close_rect.center(),
        close_size / 2.0,
        egui::Stroke::new(0.5, theme::line()),
    );
    let center = close_rect.center();
    let x_stroke = egui::Stroke::new(1.1, theme::ink_4());
    painter.line_segment(
        [
            center + egui::vec2(-3.0, -3.0),
            center + egui::vec2(3.0, 3.0),
        ],
        x_stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(3.0, -3.0),
            center + egui::vec2(-3.0, 3.0),
        ],
        x_stroke,
    );

    if response.clicked() {
        if response
            .interact_pointer_pos()
            .is_some_and(|pointer| close_rect.contains(pointer))
        {
            return (false, true);
        }
        return (true, false);
    }
    (false, false)
}

// ── Style helpers ───────────────────────────────────────────────────────────

enum StyleCardAction {
    None,
    Activate,
    Export,
    Edit,
}

fn style_pack_card(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    index: usize,
    selected: usize,
    name: &str,
    description: &str,
    tags: &[String],
    accent: egui::Color32,
    is_active: bool,
    is_builtin: bool,
) -> StyleCardAction {
    let active = is_active || selected == index;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(14),
        if active || response.hovered() {
            theme::blue_soft()
        } else {
            theme::surface()
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(
            if active { 1.5 } else { 1.0 },
            if active { theme::blue() } else { theme::line() },
        ),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(16.0);
    let mut action = StyleCardAction::None;
    let mut card_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut card_ui;
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(name)
                .size(14.0)
                .strong()
                .color(theme::ink()),
        );
        ui.add_space(8.0);
        if is_builtin {
            egui::Frame::new()
                .fill(theme::surface())
                .stroke(egui::Stroke::new(0.7, accent))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(theme::text("内置"))
                            .size(10.5)
                            .color(accent),
                    );
                });
        }
        if active {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(theme::ink())
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(theme::text("当前"))
                            .size(10.5)
                            .strong()
                            .color(theme::surface()),
                    );
                });
        }
    });
    ui.add_space(9.0);
    let description_width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(description_width, 48.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(truncate_text(description, 96))
                        .size(11.5)
                        .color(theme::ink_3()),
                )
                .wrap(),
            );
        },
    );
    ui.add_space(9.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (tag_index, tag) in tags.iter().enumerate() {
            egui::Frame::new()
                .fill(if tag_index == 0 {
                    theme::blue_soft()
                } else {
                    theme::surface_2()
                })
                .stroke(egui::Stroke::new(
                    0.5,
                    if tag_index == 0 {
                        accent
                    } else {
                        theme::line()
                    },
                ))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(7, 3))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(tag.as_str()).size(10.5).color(
                        if tag_index == 0 {
                            accent
                        } else {
                            theme::ink_3()
                        },
                    ));
                });
        }
    });
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let activate = ui.add_enabled(
                !active,
                egui::Button::new(egui::RichText::new(theme::text("激活")).size(10.5))
                    .fill(if active { theme::ink() } else { theme::blue() })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(64.0, 24.0)),
            );
            if activate.clicked() {
                action = StyleCardAction::Activate;
            }
            let export = ui.add(
                egui::Button::new(egui::RichText::new(theme::text("导出")).size(10.5))
                    .fill(theme::surface_2())
                    .stroke(egui::Stroke::new(0.7, theme::line()))
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(64.0, 24.0)),
            );
            if export.clicked() {
                action = StyleCardAction::Export;
            }
            let edit = ui.add_enabled(
                true,
                egui::Button::new(egui::RichText::new(theme::text("编辑")).size(10.5))
                    .fill(theme::surface_2())
                    .stroke(egui::Stroke::new(0.7, theme::line()))
                    .corner_radius(egui::CornerRadius::same(7))
                    .min_size(egui::vec2(64.0, 24.0)),
            );
            if edit.clicked() {
                action = StyleCardAction::Edit;
            }
        });
    });
    if response.double_clicked() {
        StyleCardAction::Edit
    } else {
        action
    }
}

fn new_style_pack_card(ui: &mut egui::Ui, size: egui::Vec2) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(14),
        if response.hovered() {
            theme::surface_2()
        } else {
            theme::surface()
        },
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(1.0, theme::line()),
        egui::StrokeKind::Inside,
    );
    let center = rect.center() - egui::vec2(0.0, 20.0);
    ui.painter().circle_filled(center, 22.0, theme::surface_2());
    let stroke = egui::Stroke::new(1.5, theme::blue());
    ui.painter().line_segment(
        [center - egui::vec2(8.0, 0.0), center + egui::vec2(8.0, 0.0)],
        stroke,
    );
    ui.painter().line_segment(
        [center - egui::vec2(0.0, 8.0), center + egui::vec2(0.0, 8.0)],
        stroke,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        "新建风格包",
        egui::FontId::proportional(14.0),
        theme::ink_2(),
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 45.0),
        egui::Align2::CENTER_CENTER,
        "从模板开始创建自己的风格",
        egui::FontId::proportional(11.0),
        theme::ink_4(),
    );
    response.clicked()
}

pub fn style_editor_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    if !vm.style_editor_open {
        return;
    }
    let mut open = true;
    egui::Window::new(theme::text("编辑风格包"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .default_height(520.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(580.0)
                .show(ui, |ui| {
                    ui.label(theme::text("风格名称"));
                    ui.text_edit_singleline(&mut vm.style_name);
                    ui.label(theme::text("风格描述"));
                    ui.text_edit_singleline(&mut vm.style_description);
                    ui.add_space(10.0);
                    ui.label(theme::text("润色提示词"));
                    ui.add(
                        egui::TextEdit::multiline(&mut vm.style_prompt)
                            .desired_rows(8)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(10.0);
                    ui.label(theme::text("选区润色提示词"));
                    ui.add(
                        egui::TextEdit::multiline(&mut vm.style_selection_prompt)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(12.0);
                    if let Some(error) = &vm.style_notice {
                        ui.label(error);
                    }
                    ui.add_enabled_ui(!vm.style_saving, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new(theme::text("保存")).fill(theme::blue()))
                                .clicked()
                            {
                                actions
                                    .push(FrontendAction::StyleSaveEditor(vm.style_prompt.clone()));
                                vm.style_saving = true;
                            }
                            if vm.style_builtin && ui.button(theme::text("恢复默认")).clicked()
                            {
                                actions.push(FrontendAction::StyleReset);
                            }
                            if !vm.style_builtin && ui.button(theme::text("删除")).clicked() {
                                actions.push(FrontendAction::StyleDelete);
                            }
                            if ui.button(theme::text("取消")).clicked() {
                                actions.push(FrontendAction::StyleCloseEditor);
                            }
                        });
                    });
                });
        });
    if !open {
        actions.push(FrontendAction::StyleCloseEditor);
    }
}

// ── Translation helpers ─────────────────────────────────────────────────────

fn translation_card(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::line()))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(17))
        .show(ui, |ui| {
            ui.set_width((width - 34.0).max(1.0));
            contents(ui);
        });
}
