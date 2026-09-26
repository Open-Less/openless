use eframe::egui;
use openless_linux_egui::{tr_l10n, Lang};

use super::layout;
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, MarketplaceSort};

/// Every marketplace tile has the same height so rows line up.
const MARKETPLACE_TILE_HEIGHT: f32 = 176.0;

/// GitHub device-flow dialog. Tauri's modal is `min(440px, 100%)`; the egui
/// card keeps the reference's left edge and extends 30px further right so the
/// browser hint and the授权 status row are not cramped.
const OAUTH_CARD_WIDTH: f32 = 470.0;
const OAUTH_CARD_SHIFT_X: f32 = 15.0;
/// Title size matches the Tauri heading (16px) rather than the 15px body step.
const OAUTH_TITLE_SIZE: f32 = 16.0;

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

    /// The GitHub device-flow dialog must keep Tauri's left edge, extend to the
    /// right, use the 16px heading and centre the status row on the card axis.
    #[test]
    fn oauth_dialog_widens_right_and_centres_the_status_row() {
        use super::super::view_model::Page;
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: Lang::ZhCn,
            active_page: Page::Marketplace,
            marketplace_loading: false,
            marketplace_unsupported: false,
            marketplace_oauth_open: true,
            marketplace_oauth_user_code: "68A0-0EB0".into(),
            marketplace_oauth_uri: "https://github.com/login/device".into(),
            ..Default::default()
        };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1240.0, 800.0));
        // Two frames: the modal `Area` needs one frame to publish its rect.
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            });
            crate::ui::frontend::render(&ctx, &mut vm, &mut Vec::new());
            let _ = crate::ui::frontend::end_pass(&ctx);
        }
        let card: egui::Rect = ctx
            .data(|data| data.get_temp(egui::Id::new("openless-marketplace-oauth-card-rect")))
            .expect("the open dialog must publish its card rect");
        let body = layout::body_rect(&ctx);
        assert_eq!(card.width(), OAUTH_CARD_WIDTH);
        assert_eq!(card.center().x - body.center().x, OAUTH_CARD_SHIFT_X);
        assert_eq!(card.top(), body.center().y - 165.0);
        assert!(card.right() <= body.right(), "{card:?} in {body:?}");

        let row: egui::Rect = ctx
            .data(|data| data.get_temp(egui::Id::new("openless-marketplace-oauth-status-row")))
            .expect("the pending phase must paint the status row");
        assert!(
            (row.center().x - card.center().x).abs() <= 1.0,
            "the status row must sit on the card's axis: {row:?} in {card:?}"
        );
        assert!(row.left() > card.left() + 22.0, "{row:?} in {card:?}");
        assert_eq!(OAUTH_TITLE_SIZE, 16.0);
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
    if vm.marketplace_upload_open {
        marketplace_upload(ui.ctx(), vm, body_rect, actions);
    }
    if vm.marketplace_confirm_withdraw.is_some() {
        marketplace_withdraw_confirm(ui.ctx(), vm, body_rect, actions);
    }
    if vm.marketplace_oauth_open {
        let nested = vm.marketplace_selected.is_some()
            || vm.marketplace_mine_open
            || vm.marketplace_upload_open
            || vm.marketplace_confirm_withdraw.is_some();
        marketplace_oauth(ui.ctx(), vm, body_rect, actions, nested);
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
            layout::paint_blurred_overlay(ctx, ui, body_rect, layout::body_corner_radius(ctx));
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

fn marketplace_oauth(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    body: egui::Rect,
    actions: &mut Vec<FrontendAction>,
    nested: bool,
) {
    let lang = vm.lang;
    // Keep the dialog's left edge stable while giving the right side a little
    // more breathing room for the browser hint and the status row.
    let size = egui::vec2(
        (body.width() - 40.0).min(OAUTH_CARD_WIDTH).max(340.0),
        330.0,
    );
    let card =
        egui::Rect::from_center_size(body.center() + egui::vec2(OAUTH_CARD_SHIFT_X, 0.0), size);
    #[cfg(test)]
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("openless-marketplace-oauth-card-rect"), card)
    });
    egui::Area::new(egui::Id::new("openless-marketplace-oauth-modal"))
        .order(egui::Order::Tooltip)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            if nested {
                // Keep the already-open detail/mine card visible, like the
                // stacked Tauri Modal, instead of replacing it with a second
                // snapshot of the page.
                ui.painter().rect_filled(
                    body,
                    layout::body_corner_radius(ctx),
                    egui::Color32::from_black_alpha(52),
                );
            } else {
                layout::paint_blurred_overlay(ctx, ui, body, layout::body_corner_radius(ctx));
            }
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                    .corner_radius(egui::CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(22))
                    .show(ui, |ui| {
                        ui.set_width((size.x - 44.0).max(1.0));
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(tr_l10n(lang, "marketplace.oauth.title"))
                                    .size(OAUTH_TITLE_SIZE)
                                    .strong()
                                    .color(theme::INK),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let close = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("×").size(16.0).color(theme::INK_2),
                                        )
                                        .fill(theme::SURFACE)
                                        .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                        .corner_radius(egui::CornerRadius::same(8))
                                        .min_size(egui::vec2(28.0, 28.0)),
                                    );
                                    if close.clicked() {
                                        actions.push(FrontendAction::MarketplaceAuthCancel);
                                    }
                                },
                            );
                        });
                        ui.add_space(13.0);
                        if vm.marketplace_oauth_loading {
                            ui.vertical_centered(|ui| {
                                ui.spinner();
                                ui.add_space(8.0);
                                ui.label(tr_l10n(lang, "marketplace.oauth.generating"));
                            });
                        } else if let Some(error) = vm.marketplace_oauth_error.as_deref() {
                            ui.label(egui::RichText::new(error).size(12.0).color(theme::ERR));
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .button(tr_l10n(lang, "marketplace.oauth.retryBtn"))
                                    .clicked()
                                {
                                    actions.push(FrontendAction::MarketplaceAuthStart);
                                }
                                if ui
                                    .button(tr_l10n(lang, "marketplace.oauth.cancelBtn"))
                                    .clicked()
                                {
                                    actions.push(FrontendAction::MarketplaceAuthCancel);
                                }
                            });
                        } else {
                            ui.label(
                                egui::RichText::new(openless_linux_egui::fmt_l10n(
                                    lang,
                                    "marketplace.oauth.browserHint",
                                    &[&vm.marketplace_oauth_uri],
                                ))
                                .size(12.0)
                                .color(theme::INK_3),
                            );
                            ui.add_space(10.0);
                            egui::Frame::new()
                                .fill(theme::SURFACE_2)
                                .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                .corner_radius(egui::CornerRadius::same(10))
                                .inner_margin(egui::Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.with_layout(
                                                egui::Layout::left_to_right(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(
                                                            &vm.marketplace_oauth_user_code,
                                                        )
                                                        .size(22.0)
                                                        .strong()
                                                        .color(theme::BLUE),
                                                    );
                                                    ui.add_space(10.0);
                                                    let copy = ui.add(
                                                        egui::Button::new(
                                                            egui::RichText::new(tr_l10n(
                                                                lang,
                                                                "marketplace.oauth.copyBtn",
                                                            ))
                                                            .size(11.0)
                                                            .color(theme::INK_3),
                                                        )
                                                        .fill(theme::SURFACE)
                                                        .stroke(egui::Stroke::new(
                                                            0.5,
                                                            theme::LINE_STRONG,
                                                        ))
                                                        .corner_radius(egui::CornerRadius::same(7)),
                                                    );
                                                    if copy.clicked() {
                                                        actions.push(
                                                            FrontendAction::MarketplaceAuthCopyCode,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                    });
                                });
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let open = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(tr_l10n(
                                            lang,
                                            "marketplace.oauth.openBrowserBtn",
                                        ))
                                        .size(11.5)
                                        .color(theme::INK_2),
                                    )
                                    .fill(theme::SURFACE)
                                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                    .corner_radius(egui::CornerRadius::same(8)),
                                );
                                if open.clicked() {
                                    actions.push(FrontendAction::MarketplaceAuthOpenBrowser);
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let cancel = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(tr_l10n(
                                                    lang,
                                                    "marketplace.oauth.cancelBtn",
                                                ))
                                                .size(11.5)
                                                .color(theme::INK_2),
                                            )
                                            .fill(theme::SURFACE)
                                            .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                            .corner_radius(egui::CornerRadius::same(8)),
                                        );
                                        if cancel.clicked() {
                                            actions.push(FrontendAction::MarketplaceAuthCancel);
                                        }
                                    },
                                );
                            });
                            ui.add_space(11.0);
                            // Tauri centres the 「蓝色圆点 + 等待文案」 group inside the
                            // card (`justify-content: center`, 8px dot, 6px gap).
                            // `ui.horizontal` allocates the full width and would pin the
                            // group to the left edge, so the row is measured and
                            // allocated at its natural width; `vertical_centered` then
                            // centres that allocation on the card's axis.
                            ui.vertical_centered(|ui| {
                                let label = tr_l10n(lang, "marketplace.oauth.waiting");
                                const DOT: f32 = 8.0;
                                const GAP: f32 = 6.0;
                                let width = DOT + GAP + layout::text_width(ui, label, 11.5);
                                let (row, _) = ui.allocate_exact_size(
                                    egui::vec2(width, 16.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(
                                    egui::pos2(row.left() + DOT / 2.0, row.center().y),
                                    3.5,
                                    theme::BLUE,
                                );
                                ui.painter().text(
                                    egui::pos2(row.left() + DOT + GAP, row.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    egui::FontId::proportional(11.5),
                                    theme::INK_4,
                                );
                                #[cfg(test)]
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(
                                        egui::Id::new("openless-marketplace-oauth-status-row"),
                                        row,
                                    )
                                });
                            });
                        }
                    });
            });
        });
}

fn marketplace_upload(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    body: egui::Rect,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let target_name = vm.marketplace_upload_target_name.clone();
    let card_width = (body.width() - 40.0).min(560.0).max(320.0);
    let card_height = (body.height() * 0.82).min(560.0).max(300.0);
    let card = egui::Rect::from_center_size(body.center(), egui::vec2(card_width, card_height));
    egui::Area::new(egui::Id::new("openless-marketplace-upload-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            layout::paint_blurred_overlay(ctx, ui, body, layout::body_corner_radius(ctx));
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                    .corner_radius(egui::CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(22))
                    .show(ui, |ui| {
                        ui.set_width((card_width - 44.0).max(1.0));
                        ui.horizontal(|ui| {
                            let title = if let Some(name) = target_name.as_deref() {
                                openless_linux_egui::fmt_l10n(
                                    lang,
                                    "marketplace.upload.updateTitle",
                                    &[&name],
                                )
                            } else {
                                tr_l10n(lang, "marketplace.upload.title").to_string()
                            };
                            ui.label(
                                egui::RichText::new(title)
                                    .size(16.0)
                                    .strong()
                                    .color(theme::INK),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("×").clicked() {
                                        actions.push(FrontendAction::MarketplaceUploadCancel);
                                    }
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(if target_name.is_some() {
                                tr_l10n(lang, "marketplace.upload.updateHint")
                            } else {
                                tr_l10n(lang, "marketplace.upload.hint")
                            })
                            .size(11.5)
                            .color(theme::INK_3),
                        );
                        ui.add_space(12.0);
                        if vm.marketplace_upload_submitting {
                            ui.vertical_centered(|ui| {
                                ui.add_space(42.0);
                                ui.spinner();
                                ui.add_space(8.0);
                                ui.label(tr_l10n(lang, "marketplace.upload.submitting"));
                            });
                        } else if vm.marketplace_upload_packs.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(28.0);
                                ui.label(
                                    egui::RichText::new(tr_l10n(
                                        lang,
                                        "marketplace.upload.noLocal",
                                    ))
                                    .size(12.0)
                                    .color(theme::INK_4),
                                );
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height((card_height - 150.0).max(100.0))
                                .show(ui, |ui| {
                                    for (index, pack) in
                                        vm.marketplace_upload_packs.iter().enumerate()
                                    {
                                        let selected =
                                            vm.marketplace_upload_selected == Some(index);
                                        let label = format!(
                                            "{}  ·  v{}  ·  {}",
                                            pack.name,
                                            pack.version,
                                            pack.base_mode.display_name(),
                                        );
                                        let response = ui.add_sized(
                                            [ui.available_width(), 46.0],
                                            egui::Button::new(
                                                egui::RichText::new(label)
                                                    .size(12.0)
                                                    .color(theme::INK),
                                            )
                                            .fill(if selected {
                                                theme::BLUE_SOFT
                                            } else {
                                                theme::SURFACE
                                            })
                                            .stroke(egui::Stroke::new(
                                                0.5,
                                                if selected {
                                                    theme::BLUE
                                                } else {
                                                    theme::LINE_STRONG
                                                },
                                            ))
                                            .corner_radius(egui::CornerRadius::same(9)),
                                        );
                                        if response.clicked() {
                                            actions.push(FrontendAction::MarketplaceUploadSelect(
                                                index,
                                            ));
                                        }
                                        if !pack.description.is_empty() {
                                            ui.label(
                                                egui::RichText::new(&pack.description)
                                                    .size(10.5)
                                                    .color(theme::INK_4),
                                            );
                                        }
                                        ui.add_space(6.0);
                                    }
                                });
                        }
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let confirm = ui.add_enabled(
                                        vm.marketplace_upload_selected.is_some()
                                            && !vm.marketplace_upload_submitting,
                                        egui::Button::new(tr_l10n(
                                            lang,
                                            "marketplace.upload.confirm",
                                        ))
                                        .fill(theme::BLUE)
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(egui::CornerRadius::same(8)),
                                    );
                                    if confirm.clicked() {
                                        actions.push(FrontendAction::MarketplaceUploadConfirm);
                                    }
                                    if ui.button(tr_l10n(lang, "common.cancel")).clicked() {
                                        actions.push(FrontendAction::MarketplaceUploadCancel);
                                    }
                                },
                            );
                        });
                    });
            });
        });
}

fn marketplace_withdraw_confirm(
    ctx: &egui::Context,
    vm: &FrontendViewModel,
    body: egui::Rect,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let Some(index) = vm.marketplace_confirm_withdraw else {
        return;
    };
    let Some(pack) = vm.marketplace_mine_packs.get(index) else {
        return;
    };
    let message =
        openless_linux_egui::fmt_l10n(lang, "marketplace.withdraw.confirm", &[&pack.pack.name]);
    let card =
        egui::Rect::from_center_size(body.center(), egui::vec2(body.width().min(420.0), 170.0));
    egui::Area::new(egui::Id::new("openless-marketplace-withdraw-confirm"))
        .order(egui::Order::Tooltip)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            layout::paint_blurred_overlay(ctx, ui, body, layout::body_corner_radius(ctx));
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(20))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(message).size(13.0).color(theme::INK_2));
                        ui.add_space(20.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(tr_l10n(lang, "marketplace.withdraw.confirmBtn"))
                                .clicked()
                            {
                                actions.push(FrontendAction::MarketplaceWithdrawConfirm);
                            }
                            if ui.button(tr_l10n(lang, "common.cancel")).clicked() {
                                actions.push(FrontendAction::MarketplaceWithdrawCancel);
                            }
                        });
                    });
            });
        });
}

fn market_state_label(lang: Lang, state: &str) -> String {
    let key = match state {
        "pending" => "marketplace.state.pending",
        "approved" => "marketplace.state.approved",
        "rejected" => "marketplace.state.rejected",
        "withdrawn" => "marketplace.state.withdrawn",
        "superseded" => "marketplace.state.superseded",
        _ => "marketplace.state.unknown",
    };
    if state.is_empty()
        || !matches!(
            state,
            "pending" | "approved" | "rejected" | "withdrawn" | "superseded"
        )
    {
        if state.is_empty() {
            return tr_l10n(lang, key).to_string();
        }
        return state.to_string();
    }
    tr_l10n(lang, key).to_string()
}

fn marketplace_mine(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    body: egui::Rect,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let signed_in = vm.marketplace_signed_in;
    let pack_count = vm
        .marketplace_mine_packs
        .iter()
        .filter(|entry| !matches!(entry.state.as_str(), "withdrawn" | "rejected"))
        .count();
    let pending_count = vm
        .marketplace_mine_packs
        .iter()
        .filter(|entry| entry.state == "pending")
        .count();
    let modal_width = (body.width() - 40.0).min(560.0).max(320.0);
    let natural_height = if signed_in && pack_count > 0 {
        280.0 + pack_count as f32 * 144.0
    } else if signed_in {
        330.0
    } else {
        300.0
    };
    let modal_height = natural_height.min((body.height() * 0.85).max(260.0));
    let size = egui::vec2(modal_width, modal_height);
    let card = egui::Rect::from_center_size(body.center(), size);
    #[cfg(test)]
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("openless-marketplace-mine-card-rect"), card)
    });
    egui::Area::new(egui::Id::new("openless-marketplace-mine-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            layout::paint_blurred_overlay(ctx, ui, body, layout::body_corner_radius(ctx));
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                    .corner_radius(egui::CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(22))
                    .show(ui, |ui| {
                        ui.set_width((size.x - 44.0).max(1.0));
                        ui.set_min_height((size.y - 44.0).max(1.0));

                        // Tauri: search, identity chip and close are one compact
                        // header row rather than a title on its own line.
                        let row_width = ui.available_width();
                        let close_width = 30.0;
                        let login_label = if signed_in && !vm.marketplace_login.is_empty() {
                            format!("@{}", vm.marketplace_login)
                        } else if signed_in {
                            tr_l10n(lang, "marketplace.modal.loggedInLabel").to_string()
                        } else {
                            tr_l10n(lang, "marketplace.oauth.loginBtn").to_string()
                        };
                        let login_avatar = vm
                            .marketplace_login
                            .chars()
                            .next()
                            .map(|ch| ch.to_uppercase().to_string())
                            .unwrap_or_else(|| "?".to_string());
                        let login_width = (layout::text_width(ui, &login_label, 12.0) + 42.0)
                            .clamp(64.0, 132.0);
                        let search_width =
                            (row_width - close_width - login_width - 20.0).max(120.0);
                        ui.horizontal(|ui| {
                            ui.allocate_ui(egui::vec2(search_width, 30.0), |ui| {
                                egui::Frame::new()
                                    .fill(theme::SURFACE)
                                    .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                    .corner_radius(egui::CornerRadius::same(10))
                                    .inner_margin(egui::Margin::symmetric(8, 5))
                                    .show(ui, |ui| {
                                        ui.set_width((search_width - 16.0).max(1.0));
                                        ui.horizontal(|ui| {
                                            let (icon_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(16.0, 18.0),
                                                egui::Sense::hover(),
                                            );
                                            let icon_center =
                                                icon_rect.center() - egui::vec2(1.5, 1.5);
                                            let icon_stroke = egui::Stroke::new(1.3, theme::INK_3);
                                            ui.painter().circle_stroke(
                                                icon_center,
                                                5.0,
                                                icon_stroke,
                                            );
                                            ui.painter().line_segment(
                                                [
                                                    icon_center + egui::vec2(3.5, 3.5),
                                                    icon_center + egui::vec2(7.0, 7.0),
                                                ],
                                                icon_stroke,
                                            );
                                            ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut vm.marketplace_mine_query,
                                                )
                                                .hint_text(tr_l10n(
                                                    lang,
                                                    "marketplace.myPacks.searchPlaceholder",
                                                ))
                                                .frame(egui::Frame::NONE)
                                                .desired_width((search_width - 40.0).max(48.0)),
                                            );
                                        });
                                    });
                            });
                            ui.add_space(6.0);
                            let (login_rect, login_response) = ui.allocate_exact_size(
                                egui::vec2(login_width, 30.0),
                                egui::Sense::click(),
                            );
                            let login_painter = ui.painter().with_clip_rect(login_rect);
                            login_painter.rect_filled(
                                login_rect,
                                egui::CornerRadius::same(9),
                                if signed_in {
                                    theme::BLUE_SOFT
                                } else {
                                    theme::SURFACE
                                },
                            );
                            login_painter.rect_stroke(
                                login_rect,
                                egui::CornerRadius::same(9),
                                egui::Stroke::new(0.5, theme::LINE_STRONG),
                                egui::StrokeKind::Inside,
                            );
                            let login_badge = egui::pos2(
                                login_rect.left() + 19.0,
                                login_rect.center().y,
                            );
                            login_painter.circle_filled(
                                login_badge,
                                9.0,
                                if signed_in {
                                    egui::Color32::from_rgba_premultiplied(37, 99, 235, 28)
                                } else {
                                    theme::SURFACE_2
                                },
                            );
                            login_painter.text(
                                login_badge,
                                egui::Align2::CENTER_CENTER,
                                &login_avatar,
                                egui::FontId::proportional(10.0),
                                if signed_in { theme::BLUE } else { theme::INK_2 },
                            );
                            login_painter.text(
                                egui::pos2(login_badge.x + 17.0, login_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                &login_label,
                                egui::FontId::proportional(12.0),
                                if signed_in { theme::BLUE } else { theme::INK_2 },
                            );
                            if login_response.clicked() {
                                actions.push(FrontendAction::MarketplaceAuthStart);
                            }
                            ui.add_space(6.0);
                            if ui
                                .add_sized(
                                    [close_width, 30.0],
                                    egui::Button::new("×")
                                        .fill(theme::SURFACE)
                                        .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                        .corner_radius(egui::CornerRadius::same(9)),
                                )
                                .clicked()
                            {
                                actions.push(FrontendAction::MarketplaceCloseMine);
                            }
                        });
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            let summary = if signed_in {
                                let key = if pending_count > 0 {
                                    "marketplace.myPacks.summaryPending"
                                } else {
                                    "marketplace.myPacks.summary"
                                };
                                if pending_count > 0 {
                                    openless_linux_egui::fmt_l10n(
                                        lang,
                                        key,
                                        &[&pack_count, &pending_count],
                                    )
                                } else {
                                    openless_linux_egui::fmt_l10n(lang, key, &[&pack_count])
                                }
                            } else {
                                tr_l10n(lang, "marketplace.myPacks.notLoggedIn").to_string()
                            };
                            ui.label(egui::RichText::new(summary).size(11.5).color(theme::INK_3));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let upload_enabled =
                                        signed_in && !vm.marketplace_upload_submitting;
                                    let upload = ui.add_enabled(
                                        upload_enabled,
                                        egui::Button::new(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "marketplace.upload_btn",
                                            ))
                                            .size(12.0)
                                            .color(if upload_enabled {
                                                egui::Color32::WHITE
                                            } else {
                                                theme::INK_4
                                            }),
                                        )
                                        .fill(if upload_enabled {
                                            theme::BLUE
                                        } else {
                                            theme::SURFACE_2
                                        })
                                        .stroke(egui::Stroke::new(
                                            0.5,
                                            if upload_enabled {
                                                theme::BLUE
                                            } else {
                                                theme::LINE_STRONG
                                            },
                                        ))
                                        .corner_radius(egui::CornerRadius::same(8)),
                                    );
                                    if upload.clicked() {
                                        actions.push(FrontendAction::MarketplaceUploadOpen {
                                            origin_pack_id: None,
                                            target_name: None,
                                        });
                                    }
                                    let refresh_enabled = signed_in && !vm.marketplace_mine_loading;
                                    let refresh = ui.add_enabled(
                                        refresh_enabled,
                                        egui::Button::new(
                                            egui::RichText::new(tr_l10n(lang, "common.refresh"))
                                                .size(12.0)
                                                .color(if refresh_enabled {
                                                    theme::INK_2
                                                } else {
                                                    theme::INK_4
                                                }),
                                        )
                                        .fill(theme::SURFACE)
                                        .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                        .corner_radius(egui::CornerRadius::same(8)),
                                    );
                                    if refresh.clicked() {
                                        actions.push(FrontendAction::MarketplaceMyPacks);
                                    }
                                },
                            );
                        });
                        ui.add_space(12.0);

                        if vm.marketplace_mine_loading {
                            ui.vertical_centered(|ui| {
                                ui.add_space(20.0);
                                ui.label(
                                    egui::RichText::new(tr_l10n(
                                        lang,
                                        "marketplace.myPacks.loadingTitle",
                                    ))
                                    .size(13.0)
                                    .color(theme::INK_3),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(tr_l10n(
                                        lang,
                                        "marketplace.myPacks.loadingHint",
                                    ))
                                    .size(11.5)
                                    .color(theme::INK_4),
                                );
                            });
                        } else if let Some(error) = vm.marketplace_notice.as_deref() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(16.0);
                                ui.label(
                                    egui::RichText::new(tr_l10n(
                                        lang,
                                        "marketplace.myPacks.loadErrorTitle",
                                    ))
                                    .size(13.0)
                                    .color(theme::ERR),
                                );
                                ui.add_space(6.0);
                                ui.label(egui::RichText::new(error).size(11.5).color(theme::INK_4));
                                ui.add_space(10.0);
                                if ui
                                    .button(tr_l10n(lang, "marketplace.myPacks.loadErrorRetry"))
                                    .clicked()
                                {
                                    actions.push(FrontendAction::MarketplaceMyPacks);
                                }
                            });
                        } else if !signed_in {
                            ui.vertical_centered(|ui| {
                                ui.add_space(18.0);
                                ui.label(
                                    egui::RichText::new(tr_l10n(
                                        lang,
                                        "marketplace.myPacks.notLoggedIn",
                                    ))
                                    .size(13.0)
                                    .color(theme::INK_3),
                                );
                            });
                        } else {
                            let query = vm.marketplace_mine_query.trim().to_lowercase();
                            let visible: Vec<_> = vm
                                .marketplace_mine_packs
                                .iter()
                                .filter(|entry| {
                                    if matches!(entry.state.as_str(), "withdrawn" | "superseded") {
                                        return false;
                                    }
                                    query.is_empty()
                                        || entry.pack.name.to_lowercase().contains(&query)
                                        || entry.pack.description.to_lowercase().contains(&query)
                                        || entry
                                            .pack
                                            .tags
                                            .iter()
                                            .any(|tag| tag.to_lowercase().contains(&query))
                                })
                                .collect();
                            if visible.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(20.0);
                                    let empty_key = if query.is_empty() {
                                        "marketplace.myPacks.emptyTitle"
                                    } else {
                                        "marketplace.myPacks.noMatch"
                                    };
                                    ui.label(
                                        egui::RichText::new(tr_l10n(lang, empty_key))
                                            .size(13.0)
                                            .color(theme::INK_3),
                                    );
                                    if query.is_empty() {
                                        ui.add_space(6.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "marketplace.myPacks.emptyHint",
                                            ))
                                            .size(11.5)
                                            .color(theme::INK_4),
                                        );
                                    }
                                });
                            } else {
                                egui::ScrollArea::vertical()
                                    .max_height((size.y - 150.0).max(80.0))
                                    .show(ui, |ui| {
                                        for entry in visible {
                                            let pack = &entry.pack;
                                            egui::Frame::new()
                                                .fill(theme::SURFACE)
                                                .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                                                .corner_radius(egui::CornerRadius::same(12))
                                                .inner_margin(egui::Margin::same(14))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        ui.vertical(|ui| {
                                                            ui.label(
                                                                egui::RichText::new(&pack.name)
                                                                    .size(14.0)
                                                                    .strong()
                                                                    .color(theme::INK),
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(
                                                                    openless_linux_egui::fmt_l10n(
                                                                        lang,
                                                                        "marketplace.myPacks.versionDate",
                                                                        &[&pack.version, &entry.updated_at],
                                                                    ),
                                                                )
                                                                .size(11.0)
                                                                .color(theme::INK_4),
                                                            );
                                                        });
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(egui::Align::Min),
                                                            |ui| {
                                                                egui::Frame::new()
                                                                    .fill(theme::BLUE_SOFT)
                                                                    .stroke(egui::Stroke::new(
                                                                        0.5,
                                                                        theme::LINE_STRONG,
                                                                    ))
                                                                    .corner_radius(egui::CornerRadius::same(7))
                                                                    .inner_margin(egui::Margin::symmetric(7, 3))
                                                                    .show(ui, |ui| {
                                                                        ui.label(
                                                                            egui::RichText::new(
                                                                                market_state_label(
                                                                                    lang,
                                                                                    &entry.state,
                                                                                ),
                                                                            )
                                                                            .size(11.0)
                                                                            .color(theme::INK_3),
                                                                        );
                                                                    });
                                                            },
                                                        );
                                                    });
                                                    if !pack.description.is_empty() {
                                                        ui.add_space(4.0);
                                                        ui.label(
                                                            egui::RichText::new(&pack.description)
                                                                .size(12.0)
                                                                .color(theme::INK_3),
                                                        );
                                                    }
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(&pack.mode)
                                                                .size(11.0)
                                                                .color(theme::INK_4),
                                                        );
                                                        for tag in pack.tags.iter().take(3) {
                                                            ui.label(
                                                                egui::RichText::new(tag)
                                                                    .size(11.0)
                                                                    .color(theme::INK_4),
                                                            );
                                                        }
                                                    });
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(
                                                                openless_linux_egui::fmt_l10n(
                                                                    lang,
                                                                    "marketplace.myPacks.stats",
                                                                    &[&pack.likes, &pack.downloads],
                                                                ),
                                                            )
                                                            .size(11.0)
                                                            .color(theme::INK_4),
                                                        );
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(egui::Align::Center),
                                                            |ui| {
                                                                if ui
                                                                    .button(tr_l10n(
                                                                        lang,
                                                                        "marketplace.myPacks.actions.withdraw",
                                                                    ))
                                                                    .clicked()
                                                                {
                                                                    if let Some(index) = vm
                                                                        .marketplace_mine_packs
                                                                        .iter()
                                                                        .position(|candidate| {
                                                                            candidate.pack.id == pack.id
                                                                        })
                                                                    {
                                                                        actions.push(
                                                                            FrontendAction::MarketplaceWithdrawRequest(
                                                                                index,
                                                                            ),
                                                                        );
                                                                    }
                                                                }
                                                                if ui
                                                                    .button(tr_l10n(
                                                                        lang,
                                                                        "marketplace.myPacks.actions.update",
                                                                    ))
                                                                    .clicked()
                                                                {
                                                                    actions.push(
                                                                        FrontendAction::MarketplaceUploadOpen {
                                                                            origin_pack_id: Some(
                                                                                pack.id.clone(),
                                                                            ),
                                                                            target_name: Some(
                                                                                pack.name.clone(),
                                                                            ),
                                                                        },
                                                                    );
                                                                }
                                                            },
                                                        );
                                                    });
                                                });
                                            ui.add_space(10.0);
                                        }
                                    });
                            }
                        }
                    });
            });
        });
}
