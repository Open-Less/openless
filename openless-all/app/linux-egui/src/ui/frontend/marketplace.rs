use eframe::egui;
use openless_linux_egui::{tr_l10n, Lang};

use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, MarketplaceSort};

/// Every marketplace tile has the same height so rows line up.
const MARKETPLACE_TILE_HEIGHT: f32 = 176.0;

/// Render the marketplace page. All data comes from the view model; this
/// function is pure rendering — it reads from `vm` and pushes actions.
pub fn marketplace_page(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
    body_rect: egui::Rect,
) {
    let lang = vm.lang;
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);

    let header = layout::page_header(
        ui,
        width,
        tr_l10n(lang, "marketplace.kicker"),
        tr_l10n(lang, "marketplace.title"),
        Some(tr_l10n(lang, "marketplace.desc")),
    );
    let mine = tr_l10n(lang, "marketplace.my_packs_button_label");
    let mine_width = layout::text_width(ui, mine, 12.5) + 34.0;
    let mine_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - mine_width, header.top() + 22.0),
        egui::vec2(mine_width, 30.0),
    );
    if layout::action_button(ui, mine_rect, mine, None, layout::ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::MarketplaceMyPacks);
    }
    let refresh = tr_l10n(lang, "marketplace.refresh_btn");
    let refresh_width = layout::text_width(ui, refresh, 12.5) + 34.0;
    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(mine_rect.left() - 8.0 - refresh_width, header.top() + 22.0),
        egui::vec2(refresh_width, 30.0),
    );
    if layout::action_button(
        ui,
        refresh_rect,
        refresh,
        Some(super::icons::IconName::Refresh),
        layout::ButtonKind::Ghost,
    )
    .clicked()
    {
        actions.push(FrontendAction::MarketplaceRefresh);
    }
    ui.add_space(14.0);

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
                    // Bind the view-model field itself: a local clone loses every
                    // keystroke on the next frame (the host never echoed it back),
                    // so the search box looked like it ignored typing.
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut vm.marketplace_query)
                            .id(egui::Id::new("openless-marketplace-search"))
                            .hint_text(tr_l10n(lang, "marketplace.search_placeholder"))
                            .text_color(theme::INK)
                            .frame(false)
                            .desired_width(search_width - 34.0),
                    );
                    if resp.changed() {
                        actions.push(FrontendAction::MarketplaceSearch(
                            vm.marketplace_query.clone(),
                        ));
                    }
                });
            });
        ui.add_space(10.0);
        for (mode, label) in [
            (
                MarketplaceSort::Popular,
                tr_l10n(lang, "marketplace.sort_popular"),
            ),
            (MarketplaceSort::New, tr_l10n(lang, "marketplace.sort_new")),
            (
                MarketplaceSort::Liked,
                tr_l10n(lang, "marketplace.sort_liked"),
            ),
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
            ui.label(tr_l10n(lang, "common.loading"));
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
                        egui::RichText::new(tr_l10n(lang, "marketplace.kicker"))
                            .size(13.0)
                            .color(theme::INK_3),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "marketplace.desc"))
                            .size(11.0)
                            .color(theme::INK_4),
                    );
                });
            });
        return;
    }

    // 「我赞过的」 is a client-side filter over the signed-in user's like list.
    let liked_only = vm.marketplace_sort == MarketplaceSort::Liked;
    let visible: Vec<usize> = vm
        .marketplace_packs
        .iter()
        .enumerate()
        .filter(|(_, pack)| !liked_only || pack.liked)
        .map(|(index, _)| index)
        .collect();

    if visible.is_empty() {
        let (title, hint) = if liked_only {
            (
                tr_l10n(lang, "marketplace.liked_empty"),
                tr_l10n(lang, "marketplace.liked_empty_hint"),
            )
        } else {
            (
                tr_l10n(lang, "marketplace.empty"),
                tr_l10n(lang, "marketplace.empty_hint"),
            )
        };
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(title).size(13.0).color(theme::INK_3));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(hint).size(11.0).color(theme::INK_4));
                });
            });
    } else {
        // Fixed-size tiles, three per row on a wide window: every card is the
        // same height so the grid stays aligned regardless of description length.
        let columns = if ui.available_width() >= 900.0 {
            3
        } else if ui.available_width() >= 600.0 {
            2
        } else {
            1
        };
        let gap = 12.0;
        let width = ui.available_width();
        let card_width = (width - gap * (columns - 1) as f32) / columns as f32;
        for chunk in visible.chunks(columns) {
            let (row, _) = ui.allocate_exact_size(
                egui::vec2(width, MARKETPLACE_TILE_HEIGHT),
                egui::Sense::hover(),
            );
            for (slot, index) in chunk.iter().enumerate() {
                let Some(pack) = vm.marketplace_packs.get(*index) else {
                    continue;
                };
                let rect = egui::Rect::from_min_size(
                    egui::pos2(row.left() + slot as f32 * (card_width + gap), row.top()),
                    egui::vec2(card_width, MARKETPLACE_TILE_HEIGHT),
                );
                marketplace_card(ui, rect, pack, *index, vm, actions);
            }
            ui.add_space(gap);
        }
    }

    // Detail modal
    if let Some(index) = vm.marketplace_selected {
        if let Some(pack) = vm.marketplace_packs.get(index) {
            marketplace_detail(ui.ctx(), lang, pack, index, pack.liked, body_rect, actions);
        }
    }
}

fn marketplace_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    pack: &super::view_model::MarketplacePack,
    index: usize,
    vm: &FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let padding = 14.0;
    let inner = rect.shrink(padding);
    let (response_rect, response) = (
        rect,
        ui.interact(
            rect,
            ui.id().with(("marketplace-card", index)),
            egui::Sense::click(),
        ),
    );
    let _ = response_rect;
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
    let painter = ui.painter().with_clip_rect(rect);

    // Title row.
    painter.text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        &pack.name,
        egui::FontId::proportional(14.0),
        theme::INK,
    );
    painter.text(
        egui::pos2(inner.right(), inner.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("v{}", pack.version),
        egui::FontId::monospace(10.0),
        theme::INK_4,
    );

    // Description, clamped so every tile keeps the same height.
    let description =
        layout::text_galley(ui, &pack.description, theme::INK_3, 12.0, inner.width(), 3);
    let description_top = inner.top() + 26.0;
    painter.galley(
        egui::pos2(inner.left(), description_top),
        description.clone(),
        theme::INK_3,
    );

    // Tags directly under the clamped description.
    let mut x = inner.left();
    let tags_top = description_top + description.size().y + 8.0;
    for (text, tone) in std::iter::once((pack.mode.as_str(), layout::PillTone::Outline)).chain(
        pack.tags
            .iter()
            .take(2)
            .map(|tag| (tag.as_str(), layout::PillTone::Gray)),
    ) {
        let size = layout::pill_size(ui, text);
        if x + size.x > inner.right() {
            break;
        }
        layout::paint_pill(
            &painter,
            egui::Rect::from_min_size(egui::pos2(x, tags_top), size),
            text,
            tone,
        );
        x += size.x + 6.0;
    }

    // Footer pinned to the bottom of the fixed tile.
    let footer_center_y = rect.bottom() - padding - 12.0;
    painter.text(
        egui::pos2(inner.left(), footer_center_y),
        egui::Align2::LEFT_CENTER,
        format!("@{}", pack.author),
        egui::FontId::proportional(11.0),
        theme::INK_3,
    );
    let download = tr_l10n(lang, "marketplace.download_zip_btn");
    let download_width = layout::text_width(ui, download, 11.5) + 26.0;
    let download_rect = egui::Rect::from_min_size(
        egui::pos2(inner.right() - download_width, footer_center_y - 12.0),
        egui::vec2(download_width, 24.0),
    );
    let install = tr_l10n(lang, "marketplace.install_btn");
    let install_width = layout::text_width(ui, install, 11.5) + 26.0;
    let install_rect = egui::Rect::from_min_size(
        egui::pos2(
            download_rect.left() - 6.0 - install_width,
            footer_center_y - 12.0,
        ),
        egui::vec2(install_width, 24.0),
    );
    painter.text(
        egui::pos2(install_rect.left() - 10.0, footer_center_y),
        egui::Align2::RIGHT_CENTER,
        format!("☆ {}  ·  ↓ {}", pack.likes, pack.downloads),
        egui::FontId::proportional(10.5),
        theme::INK_4,
    );
    if layout::action_button(ui, install_rect, install, None, layout::ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::MarketplaceInstall(index));
    }
    if layout::action_button(ui, download_rect, download, None, layout::ButtonKind::Ghost).clicked()
    {
        actions.push(FrontendAction::MarketplaceDownload(index));
    }
    if response.clicked() {
        actions.push(FrontendAction::MarketplaceDetail(index));
    }
}

fn marketplace_detail(
    ctx: &egui::Context,
    lang: Lang,
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
                        ui.label(
                            egui::RichText::new(&pack.mode)
                                .size(11.0)
                                .color(theme::INK_3),
                        );
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
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(if liked {
                                    "★"
                                } else {
                                    "☆"
                                }))
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
                                    egui::Button::new(tr_l10n(lang, "marketplace.install_btn"))
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
                                    egui::Button::new(tr_l10n(lang, "common.cancel"))
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
