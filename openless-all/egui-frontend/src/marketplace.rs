use std::collections::HashSet;

use eframe::egui;

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Popular,
    New,
    Liked,
}

impl Default for SortMode {
    fn default() -> Self {
        Self::Popular
    }
}

#[derive(Clone)]
struct Pack {
    name: &'static str,
    version: &'static str,
    description: &'static str,
    mode: &'static str,
    author: &'static str,
    tags: &'static [&'static str],
    likes: u32,
    downloads: u32,
    is_new: bool,
}

const PACKS: &[Pack] = &[
    Pack {
        name: "清晰结构",
        version: "1.2.0",
        description: "把复杂内容整理成清楚、易读的结构。",
        mode: "结构化",
        author: "openless",
        tags: &["清晰", "结构"],
        likes: 128,
        downloads: 842,
        is_new: false,
    },
    Pack {
        name: "轻度润色",
        version: "1.1.3",
        description: "保留原意，只让表达更自然顺畅。",
        mode: "轻度",
        author: "openless",
        tags: &["自然", "日常"],
        likes: 96,
        downloads: 631,
        is_new: false,
    },
    Pack {
        name: "正式商务",
        version: "1.0.2",
        description: "适合邮件、汇报和正式沟通场景。",
        mode: "正式",
        author: "muran",
        tags: &["商务", "邮件"],
        likes: 74,
        downloads: 418,
        is_new: false,
    },
    Pack {
        name: "极简直接",
        version: "0.9.0",
        description: "去掉冗余，让每句话都更直接有力。",
        mode: "简洁",
        author: "tripmc",
        tags: &["简洁", "直接"],
        likes: 61,
        downloads: 309,
        is_new: true,
    },
    Pack {
        name: "温柔表达",
        version: "1.0.0",
        description: "在不改变边界的前提下，让语气更体贴。",
        mode: "轻度",
        author: "cooper",
        tags: &["温和", "沟通"],
        likes: 52,
        downloads: 276,
        is_new: true,
    },
    Pack {
        name: "技术文档",
        version: "0.8.4",
        description: "将技术说明写得准确、可执行、少歧义。",
        mode: "结构化",
        author: "chris233",
        tags: &["技术", "文档"],
        likes: 43,
        downloads: 198,
        is_new: false,
    },
    Pack {
        name: "会议纪要",
        version: "0.7.1",
        description: "提炼决定、行动项和待确认问题。",
        mode: "结构化",
        author: "jiangmuran",
        tags: &["会议", "效率"],
        likes: 38,
        downloads: 164,
        is_new: true,
    },
    Pack {
        name: "中文校对",
        version: "1.0.1",
        description: "修正常见病句、标点和用词问题。",
        mode: "校对",
        author: "openless",
        tags: &["中文", "校对"],
        likes: 31,
        downloads: 142,
        is_new: false,
    },
];

#[derive(Default)]
pub(crate) struct MarketplaceState {
    query: String,
    sort: SortMode,
    selected: Option<usize>,
    liked: HashSet<usize>,
    notice: Option<String>,
}

impl MarketplaceState {
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, body_rect: egui::Rect) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("探索社区风格包")
                    .size(13.0)
                    .color(theme::INK_3),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let refresh = ui.add(
                    egui::Button::new(egui::RichText::new("↻  刷新").size(11.5))
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(70.0, 29.0)),
                );
                if refresh.clicked() {
                    self.notice = Some("风格市场列表已刷新".into());
                }
                ui.add_space(8.0);
                let mine = ui.add(
                    egui::Button::new(egui::RichText::new("我的发布").size(11.5))
                        .fill(theme::SURFACE)
                        .stroke(egui::Stroke::new(0.8, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(78.0, 29.0)),
                );
                if mine.clicked() {
                    self.notice = Some("我的发布页面将在连接后端后显示".into());
                }
            });
        });
        ui.add_space(18.0);

        ui.horizontal(|ui| {
            let search_width = (ui.available_width() - 250.0).max(180.0);
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(search_width);
                    ui.horizontal(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        let icon_center = icon_rect.center() - egui::vec2(1.5, 1.5);
                        let icon_stroke = egui::Stroke::new(1.4, theme::INK_3);
                        ui.painter().circle_stroke(icon_center, 5.5, icon_stroke);
                        ui.painter().line_segment(
                            [
                                icon_center + egui::vec2(4.0, 4.0),
                                icon_center + egui::vec2(8.0, 8.0),
                            ],
                            icon_stroke,
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text("搜索风格包")
                                .frame(false)
                                .desired_width(search_width - 34.0),
                        );
                    });
                });
            ui.add_space(10.0);
            for (mode, label) in [
                (SortMode::Popular, "热门"),
                (SortMode::New, "最新"),
                (SortMode::Liked, "我赞过的"),
            ] {
                let selected = self.sort == mode;
                let response = ui.add(
                    egui::Button::new(egui::RichText::new(label).size(12.0).color(if selected {
                        theme::BLUE
                    } else {
                        theme::INK_2
                    }))
                    .fill(if selected {
                        theme::BLUE_SOFT
                    } else {
                        theme::SURFACE
                    })
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(8))
                    .min_size(egui::vec2(64.0, 30.0)),
                );
                if response.clicked() {
                    self.sort = mode;
                }
            }
        });
        ui.add_space(16.0);

        if let Some(notice) = self.notice.take() {
            egui::Frame::new()
                .fill(theme::BLUE_SOFT)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(notice).size(11.5).color(theme::BLUE));
                });
            ui.add_space(10.0);
        }

        let query = self.query.trim().to_lowercase();
        let mut visible: Vec<usize> = PACKS
            .iter()
            .enumerate()
            .filter(|(index, pack)| {
                let matches_query = query.is_empty()
                    || pack.name.to_lowercase().contains(&query)
                    || pack.description.to_lowercase().contains(&query)
                    || pack
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query));
                let matches_sort = self.sort != SortMode::Liked || self.liked.contains(index);
                matches_query && matches_sort
            })
            .map(|(index, _)| index)
            .collect();
        if self.sort == SortMode::New {
            visible.retain(|index| PACKS[*index].is_new);
        }

        if visible.is_empty() {
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::same(28))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("暂时没有找到风格包")
                                .size(13.0)
                                .color(theme::INK_3),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("试试其他关键词或筛选条件")
                                .size(11.0)
                                .color(theme::INK_4),
                        );
                    });
                });
        } else {
            let columns = if ui.available_width() >= 900.0 {
                3
            } else if ui.available_width() >= 600.0 {
                2
            } else {
                1
            };
            let gap = 12.0;
            let card_width = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
            ui.columns(columns, |uis| {
                for (column, column_ui) in uis.iter_mut().enumerate() {
                    for index in visible.iter().skip(column).step_by(columns) {
                        self.card(column_ui, card_width, *index);
                        column_ui.add_space(gap);
                    }
                }
            });
        }

        if let Some(index) = self.selected {
            self.detail(ui.ctx(), index, body_rect);
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, width: f32, index: usize) {
        let pack = &PACKS[index];
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 156.0), egui::Sense::click());
        let fill = if response.hovered() {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(12), fill);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(12),
            egui::Stroke::new(1.0, theme::LINE),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(14.0);
        let mut card_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut card_ui;
        // Marketplace cards are click targets. Disable egui label selection so
        // dragging or clicking text does not take over the card interaction.
        ui.style_mut().interaction.selectable_labels = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(pack.name)
                    .size(14.0)
                    .strong()
                    .color(theme::INK),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("v{}", pack.version))
                        .size(10.0)
                        .color(theme::INK_4)
                        .family(egui::FontFamily::Monospace),
                );
            });
        });
        ui.add_space(6.0);
        let description_width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(description_width, 36.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(pack.description)
                            .size(12.0)
                            .color(theme::INK_3),
                    )
                    .wrap(),
                );
            },
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            self.pill(ui, pack.mode, true);
            for tag in pack.tags.iter().take(2) {
                self.pill(ui, tag, false);
            }
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("@{}", pack.author))
                        .size(11.0)
                        .color(theme::INK_3),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let download = ui.add(
                        egui::Button::new(egui::RichText::new("下载 ZIP").size(10.0))
                            .fill(theme::SURFACE_2)
                            .stroke(egui::Stroke::new(0.7, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(7))
                            .min_size(egui::vec2(62.0, 23.0)),
                    );
                    if download.clicked() {
                        self.notice = Some(format!("已开始下载「{}」ZIP（演示）", pack.name));
                    }
                    ui.add_space(7.0);
                    ui.label(
                        egui::RichText::new(format!("☆ {}  ·  ↓ {}", pack.likes, pack.downloads))
                            .size(10.5)
                            .color(theme::INK_4),
                    );
                });
            });
        });
        if response.clicked() {
            self.selected = Some(index);
        }
    }

    fn pill(&self, ui: &mut egui::Ui, text: &str, outline: bool) {
        egui::Frame::new()
            .fill(if outline {
                egui::Color32::TRANSPARENT
            } else {
                theme::SURFACE_2
            })
            .stroke(egui::Stroke::new(
                0.5,
                if outline {
                    theme::LINE
                } else {
                    egui::Color32::TRANSPARENT
                },
            ))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(7, 2))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).size(10.0).color(theme::INK_3));
            });
    }

    fn detail(&mut self, ctx: &egui::Context, index: usize, body_rect: egui::Rect) {
        let pack = &PACKS[index];
        // The modal belongs to the body below the custom title bar. Use the
        // exact body rect calculated by MainWindow::update rather than
        // deriving a second rectangle from the global egui viewport.
        let modal_width = (body_rect.width() - 48.0).clamp(320.0, 480.0);
        let viewport_center = ctx.content_rect().center();
        let body_center_offset = body_rect.center() - viewport_center;
        let mut close = false;
        // Paint the backdrop directly from the current body rect. This avoids
        // using a cached Area size for rendering, which can leave stale white
        // strips after the viewport is resized.
        let backdrop_layer = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("marketplace-detail-backdrop"),
        );
        ctx.layer_painter(backdrop_layer).rect_filled(
            body_rect,
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 14,
                se: 14,
            },
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 56),
        );
        // Separate transparent Area for input capture. The shadow itself is
        // not tied to this Area's cached layout state.
        egui::Area::new(egui::Id::new("marketplace-detail-backdrop-input"))
            .order(egui::Order::Foreground)
            .fixed_pos(body_rect.min)
            .default_size(body_rect.size())
            .constrain(false)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(body_rect.size());
                ui.set_max_size(body_rect.size());
                let _ = ui.allocate_exact_size(body_rect.size(), egui::Sense::click());
            });
        egui::Area::new(egui::Id::new("marketplace-detail-overlay"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, body_center_offset)
            .constrain_to(body_rect)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_width(modal_width - 40.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(pack.name).size(18.0).strong());
                    ui.label(egui::RichText::new(pack.mode).size(11.0).color(theme::INK_3));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("v{}", pack.version)).size(10.0).color(theme::INK_4));
                    });
                });
                ui.label(egui::RichText::new(format!("@{}  ·  ☆ {}  ·  ↓ {}", pack.author, pack.likes, pack.downloads)).size(11.0).color(theme::INK_4));
                ui.add_space(10.0);
                ui.label(egui::RichText::new(pack.description).size(13.0).color(theme::INK_2));
                ui.add_space(12.0);
                egui::Frame::new()
                    .fill(theme::SURFACE_2)
                    .stroke(egui::Stroke::new(0.5, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("本地占位预览\n将原始表达保留在上下文中，优化语气、结构和可读性。\n这段内容会由真实风格包提示词替换。")
                            .size(12.0).color(theme::INK_2).family(egui::FontFamily::Monospace));
                    });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("☆").fill(theme::SURFACE).stroke(egui::Stroke::new(1.0, theme::LINE)).corner_radius(egui::CornerRadius::same(8))).clicked() {
                        if !self.liked.insert(index) { self.liked.remove(&index); }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("安装到本地").fill(theme::BLUE).stroke(egui::Stroke::NONE).corner_radius(egui::CornerRadius::same(8))).clicked() {
                            self.notice = Some(format!("已将「{}」加入本地风格包（占位操作）", pack.name));
                            close = true;
                        }
                        if ui.add(egui::Button::new("取消").fill(theme::SURFACE).stroke(egui::Stroke::new(1.0, theme::LINE)).corner_radius(egui::CornerRadius::same(8))).clicked() {
                            close = true;
                        }
                    });
                });
                    });
            });
        if close {
            self.selected = None;
        }
    }
}
