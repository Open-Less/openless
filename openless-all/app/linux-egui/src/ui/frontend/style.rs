//! Style (润色模式) page — port of the Tauri `pages/Style.tsx`.
//!
//! One card holds the pack list: a header row with the raw-mode entry, the
//! dictation/selection workflow switch and a pack counter, followed by a grid
//! of style-pack cards plus a "new pack" tile. The pack editor opens as a
//! floating window.
//!
//! The card's highlighted state comes from `StylePack::is_active` only — the
//! pack the host reports as current. The page-local `style_selected` index is
//! used solely for the raw-mode tab, so a stale index can no longer paint a
//! second card as active.

use eframe::egui;
use openless_linux_egui::{fmt_l10n, tr_l10n, Lang};

use super::icons::IconName;
use super::layout::{self, ButtonKind, PillTone};
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, StylePack};

const GAP: f32 = 12.0;
const CARD_PADDING: f32 = 20.0;
const PACK_CARD_HEIGHT: f32 = 232.0;
const PACK_CARD_PADDING: f32 = 16.0;

pub fn page(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let width = (ui.available_width() - 24.0).max(1.0);
    ui.set_min_width(width);
    ui.set_max_width(width);
    let lang = vm.lang;

    if vm.style_unsupported {
        layout::unsupported_page(ui, lang, tr_l10n(lang, "style.title"));
        return;
    }

    header(ui, width, vm, actions);
    ui.add_space(GAP);

    // The list card swallows the remaining height; its grid scrolls inside.
    let card_height = ui.available_height().max(PACK_CARD_HEIGHT + 96.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, card_height), egui::Sense::hover());
    layout::card(ui, rect, CARD_PADDING, |ui, inner| {
        let grid_top = list_header(ui, inner, vm, actions);
        pack_grid(ui, inner, grid_top, vm, actions);
    });

    notice(ui, width, vm);
    editor_overlay(ui.ctx(), vm, actions);
}

// ── Header ──────────────────────────────────────────────────────────────────

fn header(
    ui: &mut egui::Ui,
    width: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let rect = layout::page_header(
        ui,
        width,
        tr_l10n(lang, "style.kicker"),
        tr_l10n(lang, "style.title"),
        Some(tr_l10n(lang, "style.desc")),
    );

    let import = tr_l10n(lang, "style.pack.import_zip");
    let import_width = layout::text_width(ui, import, 12.5) + 46.0;
    let import_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - import_width, rect.top() + 22.0),
        egui::vec2(import_width, 30.0),
    );
    if layout::action_button(
        ui,
        import_rect,
        import,
        Some(IconName::Download),
        ButtonKind::Blue,
    )
    .clicked()
    {
        actions.push(FrontendAction::StyleImport);
    }

    let refresh = tr_l10n(lang, "common.refresh");
    let refresh_width = layout::text_width(ui, refresh, 12.5) + 40.0;
    let refresh_rect = egui::Rect::from_min_size(
        egui::pos2(import_rect.left() - 8.0 - refresh_width, rect.top() + 22.0),
        egui::vec2(refresh_width, 30.0),
    );
    if layout::action_button(
        ui,
        refresh_rect,
        refresh,
        Some(IconName::Refresh),
        ButtonKind::Ghost,
    )
    .clicked()
    {
        vm.style_notice = None;
    }
}

// ── List card ───────────────────────────────────────────────────────────────

/// Draws the card header (title, raw tab, workflow switch, counter) and returns
/// the y coordinate where the pack grid starts.
fn list_header(
    ui: &mut egui::Ui,
    inner: egui::Rect,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) -> f32 {
    let lang = vm.lang;
    let row = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), 30.0));
    let painter = ui.painter().with_clip_rect(inner);

    painter.text(
        egui::pos2(row.left(), row.center().y),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "style.pack.list_title"),
        egui::FontId::proportional(15.0),
        theme::INK,
    );

    // 原文仅作为右侧单独按钮，不列入风格卡片。
    let raw_index = vm
        .style_packs
        .iter()
        .position(|pack| pack.id == "builtin.raw");
    let raw_active = !vm.style_selection_workflow
        && raw_index.is_some_and(|index| vm.style_packs[index].is_active);
    let raw_label = if raw_active {
        format!(
            "{} · {}",
            tr_l10n(lang, "overview.mode_raw"),
            tr_l10n(lang, "style.pack.current")
        )
    } else {
        tr_l10n(lang, "overview.mode_raw").to_string()
    };
    let raw_width = layout::text_width(ui, &raw_label, 12.0) + 24.0;
    let raw_rect = egui::Rect::from_min_size(
        egui::pos2(row.right() - raw_width, row.center().y - 12.0),
        egui::vec2(raw_width, 24.0),
    );
    let raw_response = ui.interact(
        raw_rect,
        ui.id().with("style-raw-tab"),
        egui::Sense::click(),
    );
    if raw_active {
        painter.rect_filled(raw_rect, egui::CornerRadius::same(6), theme::BLUE);
    } else if raw_response.hovered() {
        painter.rect_filled(raw_rect, egui::CornerRadius::same(6), theme::SURFACE_2);
    }
    painter.text(
        raw_rect.center(),
        egui::Align2::CENTER_CENTER,
        &raw_label,
        egui::FontId::proportional(12.0),
        if raw_active {
            egui::Color32::WHITE
        } else {
            theme::INK_3
        },
    );
    if raw_response.clicked() && !raw_active {
        if let Some(index) = raw_index {
            vm.style_selection_workflow = false;
            actions.push(FrontendAction::StyleActivate(index));
        }
    }

    // Workflow switch + pack counter, right aligned.
    let count = fmt_l10n(
        lang,
        "style.pack.list_count",
        &[&vm
            .style_packs
            .iter()
            .filter(|pack| pack.id != "builtin.raw")
            .count()],
    );
    let count_size = layout::pill_size(ui, &count);
    let count_rect = egui::Rect::from_min_size(
        egui::pos2(
            raw_rect.left() - 8.0 - count_size.x,
            row.center().y - count_size.y / 2.0,
        ),
        count_size,
    );
    layout::paint_pill(&painter, count_rect, &count, PillTone::Gray);

    let options = [
        tr_l10n(lang, "style.pack.dictation_tab"),
        tr_l10n(lang, "style.pack.selection_tab"),
    ];
    let tabs_width = layout::segmented_width(ui, &options);
    let tabs_rect = egui::Rect::from_min_size(
        egui::pos2(count_rect.left() - 10.0 - tabs_width, row.center().y - 13.0),
        egui::vec2(tabs_width, 26.0),
    );
    let selected = usize::from(vm.style_selection_workflow);
    ui.painter().rect_filled(
        tabs_rect.expand(3.0),
        egui::CornerRadius::same(9),
        theme::SURFACE_2,
    );
    if let Some(index) = layout::segmented(ui, tabs_rect, &options, selected) {
        vm.style_selection_workflow = index == 1;
    }

    let separator_y = row.bottom() + 14.0;
    painter.line_segment(
        [
            egui::pos2(inner.left(), separator_y),
            egui::pos2(inner.right(), separator_y),
        ],
        egui::Stroke::new(0.5, theme::LINE),
    );
    separator_y + 14.0
}

/// Grid of pack cards plus the "new pack" tile, scrolling inside the card.
fn pack_grid(
    ui: &mut egui::Ui,
    inner: egui::Rect,
    grid_top: f32,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let grid = egui::Rect::from_min_max(
        egui::pos2(inner.left(), grid_top),
        egui::pos2(inner.right(), inner.bottom()),
    );
    if grid.height() < 8.0 {
        return;
    }
    layout::fixed_ui(ui, grid, ("openless-style-grid",), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("style-packs-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let grid_width = ui.available_width();
                let columns = if grid_width >= 820.0 {
                    3
                } else if grid_width >= 560.0 {
                    2
                } else {
                    1
                };
                let card_width =
                    ((grid_width - GAP * (columns - 1) as f32) / columns as f32).max(1.0);
                let pack_indices: Vec<usize> = vm
                    .style_packs
                    .iter()
                    .enumerate()
                    .filter(|(_, pack)| pack.id != "builtin.raw")
                    .map(|(index, _)| index)
                    .collect();
                let tiles = pack_indices.len() + 1;
                let mut row_start = 0;
                while row_start < tiles {
                    let row_end = (row_start + columns).min(tiles);
                    let (row_rect, _) = ui.allocate_exact_size(
                        egui::vec2(grid_width, PACK_CARD_HEIGHT),
                        egui::Sense::hover(),
                    );
                    for slot in row_start..row_end {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(
                                row_rect.left() + (slot - row_start) as f32 * (card_width + GAP),
                                row_rect.top(),
                            ),
                            egui::vec2(card_width, PACK_CARD_HEIGHT),
                        );
                        if slot == pack_indices.len() {
                            new_pack_tile(ui, rect, lang, actions);
                        } else {
                            let index = pack_indices[slot];
                            let pack = vm.style_packs[index].clone();
                            // The active pack depends on the workflow tab:
                            // dictation/ASR or selection polish.
                            let active = if vm.style_selection_workflow {
                                pack.selection_active
                            } else {
                                pack.is_active
                            };
                            style_pack_card(ui, rect, &pack, index, active, lang, actions);
                        }
                    }
                    ui.add_space(GAP);
                    row_start = row_end;
                }
            });
    });
}

/// One style-pack card. Highlighted only when the host reports it as active.
fn style_pack_card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    pack: &StylePack,
    index: usize,
    // Whether this pack is the active one for the workflow currently shown
    // (dictation/ASR vs selection polish).
    active: bool,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let response = ui.interact(
        rect,
        ui.id().with(("style-pack-card", index)),
        egui::Sense::click(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let (fill, border, border_width) = if active {
        (theme::BLUE_SOFT, theme::BLUE, 1.0)
    } else if response.hovered() {
        (theme::SURFACE_2, theme::LINE, 1.0)
    } else {
        (theme::SURFACE, theme::LINE, 1.0)
    };
    painter.rect_filled(rect, egui::CornerRadius::same(14), fill);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(border_width, border),
        egui::StrokeKind::Inside,
    );

    let inner = rect.shrink(PACK_CARD_PADDING);
    let mut x = inner.left();

    // Name, then the builtin / current badges.
    painter.text(
        egui::pos2(x, inner.top()),
        egui::Align2::LEFT_TOP,
        &pack.name,
        egui::FontId::proportional(14.0),
        theme::INK,
    );
    x += layout::text_width(ui, &pack.name, 14.0) + 8.0;
    for (text, tone) in std::iter::once((
        if pack.is_builtin {
            tr_l10n(lang, "style.pack.builtin")
        } else {
            tr_l10n(lang, "style.pack.imported")
        },
        PillTone::Outline,
    ))
    .chain(active.then_some((tr_l10n(lang, "style.pack.current"), PillTone::Gray)))
    {
        let size = layout::pill_size(ui, text);
        if x + size.x > inner.right() {
            break;
        }
        layout::paint_pill(
            &painter,
            egui::Rect::from_min_size(egui::pos2(x, inner.top() + 2.0), size),
            text,
            tone,
        );
        x += size.x + 6.0;
    }

    // Description.
    let description =
        layout::text_galley(ui, &pack.description, theme::INK_3, 12.0, inner.width(), 4);
    painter.galley(
        egui::pos2(inner.left(), inner.top() + 28.0),
        description.clone(),
        theme::INK_3,
    );

    // Mode / tag pill.
    if let Some(tag) = pack.tags.first() {
        let size = layout::pill_size(ui, tag);
        layout::paint_pill(
            &painter,
            egui::Rect::from_min_size(
                egui::pos2(
                    inner.left(),
                    inner.top() + 28.0 + description.size().y + 10.0,
                ),
                size,
            ),
            tag,
            PillTone::Outline,
        );
    }

    // Actions row.
    let button_height = 28.0;
    let button_y = inner.bottom() - button_height;
    let mut button_x = inner.left();
    let primary = if active {
        tr_l10n(lang, "style.pack.current")
    } else {
        tr_l10n(lang, "style.pack.activate")
    };
    let primary_width = layout::text_width(ui, primary, 11.5) + 24.0;
    let primary_rect = egui::Rect::from_min_size(
        egui::pos2(button_x, button_y),
        egui::vec2(primary_width, button_height),
    );
    button_x += primary_width + 6.0;
    if layout::action_button(
        ui,
        primary_rect,
        primary,
        None,
        if active {
            ButtonKind::Ghost
        } else {
            ButtonKind::Blue
        },
    )
    .clicked()
        && !active
    {
        actions.push(FrontendAction::StyleActivate(index));
    }

    let export = tr_l10n(lang, "style.pack.export_short");
    let export_width = layout::text_width(ui, export, 11.5) + 24.0;
    let export_rect = egui::Rect::from_min_size(
        egui::pos2(button_x, button_y),
        egui::vec2(export_width, button_height),
    );
    button_x += export_width + 6.0;
    if layout::action_button(ui, export_rect, export, None, ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::StyleExport(index));
    }

    let edit = tr_l10n(lang, "style.pack.edit");
    let edit_width = layout::text_width(ui, edit, 11.5) + 24.0;
    let edit_rect = egui::Rect::from_min_size(
        egui::pos2(button_x, button_y),
        egui::vec2(edit_width, button_height),
    );
    if layout::action_button(ui, edit_rect, edit, None, ButtonKind::Ghost).clicked() {
        actions.push(FrontendAction::StyleEdit(index));
    }
}

/// The dashed "add pack" tile closing the grid.
fn new_pack_tile(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    lang: Lang,
    actions: &mut Vec<FrontendAction>,
) {
    let response = ui.interact(
        rect,
        ui.id().with("style-new-pack-tile"),
        egui::Sense::click(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(
        rect,
        egui::CornerRadius::same(14),
        if response.hovered() {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let center = rect.center();
    let stroke = egui::Stroke::new(1.4, theme::INK_4);
    painter.line_segment(
        [
            egui::pos2(center.x - 8.0, center.y - 12.0),
            egui::pos2(center.x + 8.0, center.y - 12.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x, center.y - 20.0),
            egui::pos2(center.x, center.y - 4.0),
        ],
        stroke,
    );
    painter.text(
        egui::pos2(center.x, center.y + 16.0),
        egui::Align2::CENTER_CENTER,
        tr_l10n(lang, "style.pack.add_pack_tile_title"),
        egui::FontId::proportional(13.0),
        theme::INK,
    );
    painter.text(
        egui::pos2(center.x, center.y + 36.0),
        egui::Align2::CENTER_CENTER,
        tr_l10n(lang, "style.pack.add_pack_tile_hint"),
        egui::FontId::proportional(11.5),
        theme::INK_4,
    );
    if response.clicked() {
        actions.push(FrontendAction::StyleNewPack);
    }
}

// ── Notice ──────────────────────────────────────────────────────────────────

fn notice(ui: &mut egui::Ui, width: f32, vm: &mut FrontendViewModel) {
    let Some(text) = vm.style_notice.clone() else {
        return;
    };
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        "✓",
        egui::FontId::proportional(13.0),
        theme::OK,
    );
    painter.text(
        egui::pos2(rect.left() + 18.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &text,
        egui::FontId::proportional(11.5),
        theme::INK_2,
    );
    let dismiss = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + 18.0 + layout::text_width(ui, &text, 11.5) + 10.0,
            rect.top() + 3.0,
        ),
        egui::vec2(20.0, 20.0),
    );
    let response = ui.interact(
        dismiss,
        ui.id().with("style-notice-dismiss"),
        egui::Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(dismiss, egui::CornerRadius::same(6), theme::SURFACE_2);
    }
    painter.text(
        dismiss.center(),
        egui::Align2::CENTER_CENTER,
        "×",
        egui::FontId::proportional(12.0),
        theme::INK_3,
    );
    if response.clicked() {
        vm.style_notice = None;
    }
}

// ── Editor ──────────────────────────────────────────────────────────────────

/// Pack editor window: the style pack's prompt plus save / reset / cancel.
fn editor_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    if !vm.style_editor_open {
        return;
    }
    let lang = vm.lang;
    let name = vm
        .style_packs
        .iter()
        .find(|pack| {
            if vm.style_selection_workflow {
                pack.selection_active
            } else {
                pack.is_active
            }
        })
        .map(|pack| pack.name.clone())
        .unwrap_or_else(|| tr_l10n(lang, "btn.new_style").to_string());

    // In-app overlay: dim the window and float a card above it, exactly like the
    // settings modal. (A free-floating `egui::Window` reads as a second OS window.)
    // Mask the content area and centre the card there, matching the other
    // overlays (settings modal / marketplace detail).
    let body = layout::body_rect(ctx);
    let body = egui::Rect::from_min_max(
        egui::pos2(body.left() + layout::SIDEBAR_WIDTH, body.top()),
        body.max,
    );
    let size = egui::vec2(
        (body.width() - 40.0).max(320.0).min(720.0),
        (body.height() - 40.0).max(240.0).min(560.0),
    );
    // 遮罩、点击拦截与卡片必须同属**一个** `Area`（同一个 LayerId）：各自独立 Area 时，
    // egui 会在按下后把被点到的 Area 抬到同层最上（`move_to_top`），遮罩被抬起就会盖住
    // 卡片；同一图层里先画遮罩、再画卡片在结构上就不可能出现。这里用 `Foreground` 而不是
    // `Tooltip`：弹窗若占 Tooltip，会盖住同层弹出的下拉/菜单（跨 Order 是 Tooltip >
    // Foreground）。
    let card_rect = egui::Rect::from_center_size(body.center(), size);
    // 卡片实际落点写进 memory，供测试查询（Area 现在覆盖整个 body，面积已不等于卡片）。
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("openless-style-editor-card-rect"), card_rect)
    });
    egui::Area::new(egui::Id::new("openless-style-editor-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            ui.painter().rect_filled(
                body,
                egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 14,
                    se: 14,
                },
                theme::OVERLAY,
            );
            // 点击拦截：吃掉 body 上的点击，下方页面既看不到也点不到。
            let _ = ui.allocate_rect(body, egui::Sense::click());
            ui.scope_builder(egui::UiBuilder::new().max_rect(card_rect), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0, theme::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .shadow(egui::Shadow {
                        offset: [0, 12],
                        blur: 28,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(42),
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(size);
                        ui.set_max_size(size);
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(22, 18))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "head.style_pack_editor",
                                            ))
                                            .size(11.0)
                                            .color(theme::INK_4),
                                        );
                                        ui.label(
                                            egui::RichText::new(&name)
                                                .size(18.0)
                                                .strong()
                                                .color(theme::INK),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Min),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("×")
                                                            .size(20.0)
                                                            .color(theme::INK_3),
                                                    )
                                                    .fill(theme::SURFACE_2)
                                                    .stroke(egui::Stroke::new(0.7, theme::LINE))
                                                    .corner_radius(egui::CornerRadius::same(8))
                                                    .min_size(egui::vec2(28.0, 28.0)),
                                                )
                                                .clicked()
                                            {
                                                actions.push(FrontendAction::StyleCloseEditor);
                                            }
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                egui::ScrollArea::vertical()
                                    .id_salt("style-editor-fields")
                                    .max_height((size.y - 122.0).max(120.0))
                                    .show(ui, |ui| {
                                        ui.set_width(size.x - 52.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.fieldName",
                                            ))
                                            .strong(),
                                        );
                                        ui.text_edit_singleline(&mut vm.style_name);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.fieldDescription",
                                            ))
                                            .strong(),
                                        );
                                        ui.text_edit_multiline(&mut vm.style_description);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.fieldTags",
                                            ))
                                            .strong(),
                                        );
                                        ui.add(
                                            egui::TextEdit::singleline(&mut vm.style_tags)
                                                .hint_text(tr_l10n(
                                                    lang,
                                                    "style.pack.fieldTagsPlaceholder",
                                                )),
                                        );
                                        ui.add_space(10.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(lang, "lbl.style_note"))
                                                .size(11.5)
                                                .color(theme::INK_3),
                                        );
                                        ui.add_space(14.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.dictation_prompt_title",
                                            ))
                                            .size(12.0)
                                            .strong(),
                                        );
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.dictation_prompt_hint",
                                            ))
                                            .size(11.0)
                                            .color(theme::INK_4),
                                        );
                                        ui.add_space(6.0);
                                        ui.add_sized(
                                            [ui.available_width(), 115.0],
                                            egui::TextEdit::multiline(&mut vm.style_prompt)
                                                .desired_rows(5),
                                        );
                                        ui.add_space(12.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.selectionPromptTitle",
                                            ))
                                            .strong(),
                                        );
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.selectionPromptHint",
                                            ))
                                            .size(11.0)
                                            .color(theme::INK_4),
                                        );
                                        ui.add_sized(
                                            [ui.available_width(), 100.0],
                                            egui::TextEdit::multiline(
                                                &mut vm.style_selection_prompt,
                                            )
                                            .desired_rows(4),
                                        );
                                        ui.add_space(12.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "style.pack.voiceEditPromptTitle",
                                            ))
                                            .strong(),
                                        );
                                        ui.add_sized(
                                            [ui.available_width(), 100.0],
                                            egui::TextEdit::multiline(
                                                &mut vm.style_voice_edit_prompt,
                                            )
                                            .desired_rows(4),
                                        );
                                        ui.add_space(10.0);
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(tr_l10n(
                                                        lang,
                                                        "style.custom_prompt_save",
                                                    ))
                                                    .fill(theme::BLUE)
                                                    .corner_radius(egui::CornerRadius::same(7)),
                                                )
                                                .clicked()
                                            {
                                                if vm.style_name.trim().is_empty() {
                                                    return;
                                                }
                                                actions.push(FrontendAction::StyleSaveEditor {
                                                    name: vm.style_name.clone(),
                                                    description: vm.style_description.clone(),
                                                    prompt: vm.style_prompt.clone(),
                                                    selection_prompt: vm
                                                        .style_selection_prompt
                                                        .clone(),
                                                    voice_edit_prompt: vm
                                                        .style_voice_edit_prompt
                                                        .clone(),
                                                    tags: vm.style_tags.clone(),
                                                });
                                                vm.style_editor_open = false;
                                            }
                                            if ui
                                                .button(tr_l10n(lang, "btn.reset_builtin"))
                                                .clicked()
                                            {
                                                // No reset action exists yet: clearing the custom
                                                // prompt falls back to the built-in system prompt.
                                                vm.style_prompt.clear();
                                            }
                                            if ui.button(tr_l10n(lang, "common.cancel")).clicked() {
                                                actions.push(FrontendAction::StyleCloseEditor);
                                            }
                                        });
                                    }); // scrollable editor fields
                            });
                    });
            });
        });
}
