use eframe::egui;

use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, MarketplaceSort};

/// Render the marketplace page. All data comes from the view model; this
/// function is pure rendering — it reads from `vm` and pushes actions.
pub fn marketplace_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    body_rect: egui::Rect,
) {
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
                actions.push(FrontendAction::MarketplaceRefresh);
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
                actions.push(FrontendAction::MarketplaceMyPacks);
            }
        });
    });
    ui.add_space(18.0);

    // Search + sort
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
                    let mut query = vm.marketplace_query.clone();
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("搜索风格包")
                            .frame(false)
                            .desired_width(search_width - 34.0),
                    );
                    if resp.changed() {
                        actions.push(FrontendAction::MarketplaceSearch(query));
                    }
                });
            });
        ui.add_space(10.0);
        for (mode, label) in [
            (MarketplaceSort::Popular, "热门"),
            (MarketplaceSort::New, "最新"),
            (MarketplaceSort::Liked, "我赞过的"),
        ] {
            let selected = vm.marketplace_sort == mode;
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
                actions.push(FrontendAction::MarketplaceSort(mode));
            }
        }
    });
    ui.add_space(16.0);

    // Notice
    if let Some(notice) = &vm.marketplace_notice {
        egui::Frame::new()
            .fill(theme::BLUE_SOFT)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(notice).size(11.5).color(theme::BLUE));
            });
        ui.add_space(10.0);
    }

    if vm.marketplace_loading {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("正在加载风格市场…");
        });
        return;
    }

    if vm.marketplace_unsupported {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("风格市场暂未接线")
                            .size(13.0)
                            .color(theme::INK_3),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("市场后端桥接将在后续阶段完成")
                            .size(11.0)
                            .color(theme::INK_4),
                    );
                });
            });
        return;
    }

    if vm.marketplace_packs.is_empty() {
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
                for (pack_index, pack) in vm.marketplace_packs.iter().enumerate() {
                    if pack_index % columns != column {
                        continue;
                    }
                    marketplace_card(column_ui, card_width, pack, pack_index, vm, actions);
                    column_ui.add_space(gap);
                }
            }
        });
    }

    // Detail modal
    if let Some(index) = vm.marketplace_selected {
        if let Some(pack) = vm.marketplace_packs.get(index) {
            marketplace_detail(
                ui.ctx(),
                pack,
                index,
                vm.marketplace_liked.contains(&index),
                body_rect,
                actions,
            );
        }
    }
}

fn marketplace_card(
    ui: &mut egui::Ui,
    width: f32,
    pack: &super::view_model::MarketplacePack,
    index: usize,
    _vm: &FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 156.0), egui::Sense::click());
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
    ui.style_mut().interaction.selectable_labels = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&pack.name)
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
                    egui::RichText::new(&pack.description)
                        .size(12.0)
                        .color(theme::INK_3),
                )
                .wrap(),
            );
        },
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        pill(ui, &pack.mode, true);
        for tag in pack.tags.iter().take(2) {
            pill(ui, tag, false);
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
                    actions.push(FrontendAction::MarketplaceDownload(index));
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
        actions.push(FrontendAction::MarketplaceDetail(index));
    }
}

fn pill(ui: &mut egui::Ui, text: &str, outline: bool) {
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

fn marketplace_detail(
    ctx: &egui::Context,
    pack: &super::view_model::MarketplacePack,
    index: usize,
    liked: bool,
    body_rect: egui::Rect,
    actions: &mut Vec<FrontendAction>,
) {
    let modal_width = (body_rect.width() - 48.0).clamp(320.0, 480.0);
    let viewport_center = ctx.content_rect().center();
    let body_center_offset = body_rect.center() - viewport_center;

    // Backdrop
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
    // Input capture
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
                        ui.label(egui::RichText::new(&pack.name).size(18.0).strong());
                        ui.label(egui::RichText::new(&pack.mode).size(11.0).color(theme::INK_3));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(format!("v{}", pack.version))
                                    .size(10.0)
                                    .color(theme::INK_4),
                            );
                        });
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "@{}  ·  ☆ {}  ·  ↓ {}",
                            pack.author, pack.likes, pack.downloads
                        ))
                        .size(11.0)
                        .color(theme::INK_4),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(&pack.description)
                            .size(13.0)
                            .color(theme::INK_2),
                    );
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(theme::SURFACE_2)
                        .stroke(egui::Stroke::new(0.5, theme::LINE))
                        .corner_radius(egui::CornerRadius::same(10))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(
                                    "本地占位预览\n将原始表达保留在上下文中，优化语气、结构和可读性。\n这段内容会由真实风格包提示词替换。",
                                )
                                .size(12.0)
                                .color(theme::INK_2)
                                .family(egui::FontFamily::Monospace),
                            );
                        });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(if liked { "★" } else { "☆" }),
                                )
                                .fill(theme::SURFACE)
                                .stroke(egui::Stroke::new(1.0, theme::LINE))
                                .corner_radius(egui::CornerRadius::same(8)),
                            )
                            .clicked()
                        {
                            actions.push(FrontendAction::MarketplaceToggleLike(index));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new("安装到本地")
                                        .fill(theme::BLUE)
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(egui::CornerRadius::same(8)),
                                )
                                .clicked()
                            {
                                actions.push(FrontendAction::MarketplaceInstall(index));
                                actions.push(FrontendAction::MarketplaceCloseDetail);
                            }
                            if ui
                                .add(
                                    egui::Button::new("取消")
                                        .fill(theme::SURFACE)
                                        .stroke(egui::Stroke::new(1.0, theme::LINE))
                                        .corner_radius(egui::CornerRadius::same(8)),
                                )
                                .clicked()
                            {
                                actions.push(FrontendAction::MarketplaceCloseDetail);
                            }
                        });
                    });
                });
        });
}
