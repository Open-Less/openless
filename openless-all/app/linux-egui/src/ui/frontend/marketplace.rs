use eframe::egui;
use openless_linux_egui::{tr_l10n, Lang};

use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, MarketplaceSort};

/// Every marketplace tile has the same height so rows line up.
const MARKETPLACE_TILE_HEIGHT: f32 = 176.0;

fn my_packs_button(ui: &mut egui::Ui, rect: egui::Rect, label: &str) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with("marketplace-my-packs"),
        egui::Sense::click(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(9), theme::SURFACE);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(9),
        egui::Stroke::new(0.5, theme::LINE_STRONG),
        egui::StrokeKind::Inside,
    );
    let badge = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 21.0, rect.center().y),
        egui::vec2(18.0, 18.0),
    );
    painter.rect_filled(badge, egui::CornerRadius::same(9), theme::SURFACE_2);
    // No verified account identity is exposed by the Linux view model yet;
    // Tauri shows '?' in this exact badge when not signed in.
    painter.text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        egui::FontId::proportional(10.0),
        theme::INK_2,
    );
    let label_pos = egui::pos2(badge.right() + 8.0, rect.center().y);
    painter.text(
        label_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        theme::INK_2,
    );
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            egui::Id::new("openless-mine-button-test-rects"),
            (rect, badge, label_pos),
        )
    });
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn my_packs_avatar_is_inside_the_button_before_the_label() {
        let ctx = egui::Context::default();
        let button = egui::Rect::from_min_size(egui::pos2(50.0, 60.0), egui::vec2(150.0, 30.0));
        let _ = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                my_packs_button(ui, button, "Mine");
            },
        );
        let (outer, avatar, text): (egui::Rect, egui::Rect, egui::Pos2) = ctx.data(|data| {
            data.get_temp(egui::Id::new("openless-mine-button-test-rects"))
                .unwrap()
        });
        assert!(outer.contains_rect(avatar));
        assert_eq!(avatar.size(), egui::vec2(18.0, 18.0));
        assert_eq!(text.x - avatar.right(), 8.0);
        assert!(outer.right() > text.x);
    }
}

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
    // Tauri keeps the '?' avatar *inside* the My Publications button.
    let mine_width = layout::text_width(ui, mine, 12.0) + 12.0 * 2.0 + 18.0 + 8.0;
    let refresh = tr_l10n(lang, "marketplace.refresh_btn");
    let refresh_width = layout::text_width(ui, refresh, 12.5) + 34.0;
    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(header.right() - refresh_width, header.top() + 22.0),
        egui::vec2(refresh_width, 30.0),
    );
    let mine_rect = egui::Rect::from_min_size(
        egui::pos2(refresh_rect.left() - 8.0 - mine_width, header.top() + 22.0),
        egui::vec2(mine_width, 30.0),
    );
    if my_packs_button(ui, mine_rect, mine)
        .on_hover_text(tr_l10n(lang, "marketplace.myPacks.buttonTitleEmpty"))
        .clicked()
    {
        actions.push(FrontendAction::MarketplaceMyPacks);
    }
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
                            .frame(egui::Frame::NONE)
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
            .corner_radius(egui::CornerRadius::same(14))
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
            .corner_radius(egui::CornerRadius::same(14))
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

    if vm.marketplace_mine_open {
        marketplace_mine(ui.ctx(), vm, body_rect, actions);
    }

    // Detail modal
    if let Some(index) = vm.marketplace_selected {
        if let Some(pack) = vm.marketplace_packs.get(index) {
            marketplace_detail(
                ui.ctx(),
                lang,
                pack,
                index,
                pack.liked,
                vm.marketplace_detail_prompt.as_deref(),
                vm.marketplace_installing
                    .as_deref()
                    .is_some_and(|installing| installing == pack.id),
                body_rect,
                actions,
            );
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
        .rect_filled(rect, egui::CornerRadius::same(14), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(14),
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

    if let Some(author) = pack
        .origin_author_login
        .as_ref()
        .filter(|author| *author != &pack.author)
    {
        let badge = openless_linux_egui::fmt_l10n(lang, "marketplace.derivativeBadge", &[author]);
        let size = layout::pill_size(ui, &badge);
        layout::paint_pill(
            &painter,
            egui::Rect::from_min_size(egui::pos2(inner.left(), tags_top + 23.0), size),
            &badge,
            layout::PillTone::Green,
        );
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
    painter.text(
        egui::pos2(download_rect.left() - 10.0, footer_center_y),
        egui::Align2::RIGHT_CENTER,
        format!("☆ {}  ·  ↓ {}", pack.likes, pack.downloads),
        egui::FontId::proportional(10.5),
        theme::INK_4,
    );
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
    prompt: Option<&str>,
    installing: bool,
    body_rect: egui::Rect,
    actions: &mut Vec<FrontendAction>,
) {
    // The Tauri Modal uses a 560px card. Keep the same centred dimensions
    // while content changes; only shrink to fit genuinely small windows.
    let modal_width = (body_rect.width() - 40.0).min(560.0);
    let card_height = (body_rect.height() - 40.0).min(470.0);
    let card_rect =
        egui::Rect::from_center_size(body_rect.center(), egui::vec2(modal_width, card_height));

    let modal = egui::Area::new(egui::Id::new("openless-marketplace-detail-modal"))
        // 与设置弹窗同一套层级策略：遮罩、点击拦截与卡片必须同属**一个** `Area`（同一个
        // LayerId）。各自独立 Area 时，egui 会在按下后把被点到的 Area 抬到同层最上
        // （`move_to_top`），遮罩一旦被抬起就会盖住卡片；同一图层里先画遮罩、再画卡片在
        // 结构上就不可能出现。这里用 `Foreground` 而不是 `Tooltip`：弹窗若占 Tooltip，会
        // 盖住同层弹出的下拉/菜单（跨 Order 是 Tooltip > Foreground）。
        .order(egui::Order::Foreground)
        .fixed_pos(body_rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body_rect.size());
            ui.painter().rect_filled(
                body_rect,
                egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 14,
                    se: 14,
                },
                theme::OVERLAY,
            );
            // 点击拦截：吃掉 body 上的点击，下方页面既看不到也点不到。
            let _ = ui.allocate_rect(body_rect, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card_rect), |ui| {
                ui.set_clip_rect(body_rect.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(22))
                    .show(ui, |ui| {
                        // egui Frame adds its own 3.8px on each side; keep
                        // the measured outer card centred at the target width.
                        ui.set_width(modal_width - 52.0);
                        ui.set_min_height(card_height - 44.0);
                        ui.set_max_height(card_height - 44.0);
                        // Tauri `Modal`: name + outline mode pill + ok-toned
                        // derivative pill + mono version, baseline-aligned in one
                        // wrapping row.
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&pack.name).size(18.0).strong(),
                                )
                                .truncate(),
                            );
                            let (mode_rect, _) = ui.allocate_exact_size(
                                layout::pill_size(ui, &pack.mode),
                                egui::Sense::hover(),
                            );
                            layout::paint_pill(
                                ui.painter(),
                                mode_rect,
                                &pack.mode,
                                layout::PillTone::Outline,
                            );
                            if let Some(author) = pack
                                .origin_author_login
                                .as_ref()
                                .filter(|author| *author != &pack.author)
                            {
                                let badge = openless_linux_egui::fmt_l10n(
                                    lang,
                                    "marketplace.derivativeBadge",
                                    &[author],
                                );
                                let (rect, response) = ui.allocate_exact_size(
                                    layout::pill_size(ui, &badge),
                                    egui::Sense::hover(),
                                );
                                layout::paint_pill(
                                    ui.painter(),
                                    rect,
                                    &badge,
                                    layout::PillTone::Green,
                                );
                                response.on_hover_text(&badge);
                            }
                            ui.label(
                                egui::RichText::new(format!("v{}", pack.version))
                                    .size(11.0)
                                    .monospace()
                                    .color(theme::INK_4),
                            );
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
                            .stroke(egui::Stroke::new(0.8, theme::LINE))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width((modal_width - 68.0).max(1.0));
                                egui::ScrollArea::vertical()
                                    .max_height((card_height - 225.0).max(60.0))
                                    .min_scrolled_height((card_height - 225.0).max(60.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(prompt.unwrap_or_default())
                                                .size(12.0)
                                                .monospace()
                                                .color(theme::INK_2),
                                        );
                                    });
                            });
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Two literal call sites: the i18n sync only
                                    // registers keys written next to `tr_l10n(…,`.
                                    let install_label = if installing {
                                        tr_l10n(lang, "marketplace.installingBtn")
                                    } else {
                                        tr_l10n(lang, "marketplace.install_btn")
                                    };
                                    let install_button = ui.add_enabled(
                                        !installing,
                                        egui::Button::new(
                                            egui::RichText::new(install_label)
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(theme::BLUE)
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(egui::CornerRadius::same(8)),
                                    );
                                    if install_button.clicked() {
                                        actions.push(FrontendAction::MarketplaceInstall(index));
                                        actions.push(FrontendAction::MarketplaceCloseDetail);
                                    }
                                    if ui.button(tr_l10n(lang, "common.cancel")).clicked() {
                                        actions.push(FrontendAction::MarketplaceCloseDetail);
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(format!(
                                                    "{} {}",
                                                    if liked { "★" } else { "☆" },
                                                    pack.likes
                                                ))
                                                .size(12.0)
                                                .color(if liked {
                                                    egui::Color32::from_rgb(239, 68, 68)
                                                } else {
                                                    theme::INK_2
                                                }),
                                            )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .stroke(egui::Stroke::NONE),
                                        )
                                        .clicked()
                                    {
                                        actions.push(FrontendAction::MarketplaceToggleLike(index));
                                    }
                                },
                            );
                        });
                    })
                    .response
            })
            .inner
        });
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("openless-marketplace-detail-card-rect"),
            modal.inner.rect,
        );
    });
}

fn marketplace_mine(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    body: egui::Rect,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let size = egui::vec2(
        (body.width() - 48.0).min(540.0),
        (body.height() - 48.0).min(560.0),
    );
    let card = egui::Rect::from_center_size(body.center(), size);
    egui::Area::new(egui::Id::new("openless-marketplace-mine-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            ui.painter().rect_filled(body, 0, theme::OVERLAY);
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(20))
                    .show(ui, |ui| {
                        ui.set_width(size.x - 40.0);
                        ui.set_min_height(size.y - 40.0);
                        ui.horizontal(|ui| {
                            ui.heading(tr_l10n(lang, "marketplace.myPacks.buttonLabel"));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button(tr_l10n(lang, "common.close")).clicked() {
                                        actions.push(FrontendAction::MarketplaceCloseMine);
                                    }
                                },
                            );
                        });
                        ui.add_space(12.0);
                        ui.add_sized(
                            [ui.available_width(), 32.0],
                            egui::TextEdit::singleline(&mut vm.marketplace_mine_query)
                                .hint_text(tr_l10n(lang, "marketplace.myPacks.searchPlaceholder")),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(openless_linux_egui::fmt_l10n(
                                lang,
                                "marketplace.myPacks.summary",
                                &[&vm.marketplace_mine_packs.len()],
                            ));
                            if ui.button(tr_l10n(lang, "common.refresh")).clicked() {
                                actions.push(FrontendAction::MarketplaceMyPacks);
                            }
                        });
                        if let Some(notice) = &vm.marketplace_notice {
                            ui.colored_label(theme::ERR, notice);
                        }
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical()
                            .max_height(size.y - 156.0)
                            .show(ui, |ui| {
                                let query = vm.marketplace_mine_query.trim().to_lowercase();
                                let visible: Vec<_> = vm
                                    .marketplace_mine_packs
                                    .iter()
                                    .filter(|(name, _, tags)| {
                                        query.is_empty()
                                            || name.to_lowercase().contains(&query)
                                            || tags
                                                .iter()
                                                .any(|tag| tag.to_lowercase().contains(&query))
                                    })
                                    .collect();
                                if visible.is_empty() {
                                    ui.label(tr_l10n(
                                        lang,
                                        if query.is_empty() {
                                            "marketplace.myPacks.emptyTitle"
                                        } else {
                                            "marketplace.myPacks.noMatch"
                                        },
                                    ));
                                }
                                for (name, description, tags) in visible {
                                    egui::Frame::new()
                                        .fill(theme::SURFACE_2)
                                        .corner_radius(egui::CornerRadius::same(9))
                                        .inner_margin(egui::Margin::same(10))
                                        .show(ui, |ui| {
                                            ui.set_width(size.x - 62.0);
                                            ui.label(egui::RichText::new(name).strong());
                                            ui.label(
                                                egui::RichText::new(description)
                                                    .size(11.5)
                                                    .color(theme::INK_3),
                                            );
                                            ui.label(
                                                egui::RichText::new(tags.join(" · "))
                                                    .size(11.0)
                                                    .color(theme::INK_4),
                                            );
                                        });
                                    ui.add_space(8.0);
                                }
                            });
                    });
            });
        });
}
