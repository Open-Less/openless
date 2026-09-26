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

use super::icons::{self, IconName};
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
        Some(IconName::Upload),
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
    let row = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), 46.0));
    let painter = ui.painter().with_clip_rect(inner);

    painter.text(
        egui::pos2(row.left(), row.top() + 11.0),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "style.pack.list_title"),
        egui::FontId::proportional(15.0),
        theme::INK,
    );

    painter.text(
        egui::pos2(row.left(), row.top() + 31.0),
        egui::Align2::LEFT_CENTER,
        tr_l10n(lang, "style.pack.list_desc"),
        egui::FontId::proportional(12.0),
        theme::INK_3,
    );

    // The raw pack is adjacent to the left title, not in the right-hand toolbar.
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
        egui::pos2(
            row.left()
                + layout::text_width(ui, tr_l10n(lang, "style.pack.list_title"), 15.0)
                + 14.0,
            row.top(),
        ),
        egui::vec2(raw_width, 24.0),
    );
    let raw_response = ui.interact(
        raw_rect,
        ui.id().with("style-raw-tab"),
        egui::Sense::click(),
    );
    painter.rect_filled(
        raw_rect,
        egui::CornerRadius::same(12),
        if raw_active || raw_response.hovered() {
            theme::SURFACE_2
        } else {
            theme::SURFACE
        },
    );
    painter.rect_stroke(
        raw_rect,
        egui::CornerRadius::same(12),
        egui::Stroke::new(0.8, theme::LINE),
        egui::StrokeKind::Inside,
    );
    painter.text(
        raw_rect.center(),
        egui::Align2::CENTER_CENTER,
        &raw_label,
        egui::FontId::proportional(12.0),
        if raw_active { theme::INK } else { theme::INK_3 },
    );
    if raw_response.clicked() && !raw_active {
        if let Some(index) = raw_index {
            vm.style_selection_workflow = false;
            actions.push(FrontendAction::StyleActivate(index));
        }
    }

    // Workflow switch + pack counter, right aligned.
    let count = fmt_l10n(lang, "style.pack.list_count", &[&vm.style_packs.len()]);
    let count_size = layout::pill_size(ui, &count);
    let count_rect = egui::Rect::from_min_size(
        egui::pos2(
            row.right() - count_size.x,
            row.center().y - count_size.y / 2.0,
        ),
        count_size,
    );
    layout::paint_pill(&painter, count_rect, &count, PillTone::Outline);

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
    // Icon picker (Tauri `StylePackIconPicker`): a 24px icon button with a
    // pencil badge, plus a reset cross once a custom icon is stored.
    let icon_rect = egui::Rect::from_min_size(inner.min, egui::vec2(24.0, 24.0));
    if let Some(texture) = pack
        .icon_data_url
        .as_deref()
        .and_then(|url| layout::style_pack_icon_texture(ui.ctx(), &pack.id, url))
    {
        ui.painter().image(
            texture.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        let default = match pack.base_mode.as_str() {
            "raw" => IconName::Mic,
            "light" => IconName::Feather,
            "structured" => IconName::Layout,
            _ => IconName::Doc,
        };
        icons::draw_icon(ui, icon_rect.center(), default, theme::INK_2);
    }
    icons::draw_icon(
        ui,
        icon_rect.right_bottom() - egui::vec2(3.0, 3.0),
        IconName::Pencil,
        theme::INK_3,
    );
    if ui
        .interact(
            icon_rect,
            ui.id().with(("style-icon", index)),
            egui::Sense::click(),
        )
        .on_hover_text(fmt_l10n(lang, "style.pack.uploadIcon", &[&pack.name]))
        .clicked()
    {
        actions.push(FrontendAction::StyleChooseIcon(index));
    }
    if pack.icon_data_url.is_some() {
        let reset_rect = egui::Rect::from_min_size(
            egui::pos2(inner.right() - 16.0, inner.top()),
            egui::vec2(16.0, 16.0),
        );
        icons::draw_icon(ui, reset_rect.center(), IconName::Close, theme::INK_3);
        if ui
            .interact(
                reset_rect,
                ui.id().with(("style-icon-reset", index)),
                egui::Sense::click(),
            )
            .on_hover_text(tr_l10n(lang, "style.pack.resetIcon"))
            .clicked()
        {
            actions.push(FrontendAction::StyleResetIcon(index));
        }
    }

    // Name, then the builtin / current badges.
    let mut x = inner.left() + 32.0;
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

/// Style-pack editor: the upstream Beta.2 drawer, not a centred modal.
///
/// Tauri `Style.tsx` renders the editor as a right-hand panel
/// (`top/right/bottom: 16`, `width: min(760px, 100vw - 32px)`) with a fixed
/// header (title, workflow-specific description, close) and one scrolling body
/// holding the pills, the fields, the prompt editors and the footer actions.
const DRAWER_WIDTH: f32 = 760.0;
const DRAWER_INSET: f32 = 16.0;
const FIELD_GAP: f32 = 12.0;
// egui also inserts item spacing between stacked widgets. The vertical gap
// below yields the same form rhythm as Tauri's 16px section gap.
const SECTION_GAP: f32 = 20.0;

fn editor_overlay(
    ctx: &egui::Context,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    if !vm.style_editor_open {
        return;
    }
    let body = layout::body_rect(ctx);
    let body = egui::Rect::from_min_max(
        egui::pos2(body.left() + layout::SIDEBAR_WIDTH, body.top()),
        body.max,
    );
    let width = DRAWER_WIDTH
        .min(body.width() - DRAWER_INSET * 2.0)
        .max(240.0);
    let card_rect = egui::Rect::from_min_max(
        egui::pos2(
            body.right() - DRAWER_INSET - width,
            body.top() + DRAWER_INSET,
        ),
        egui::pos2(body.right() - DRAWER_INSET, body.bottom() - DRAWER_INSET),
    );
    // 卡片实际落点写进 memory，供测试查询（Area 覆盖整个 body，面积已不等于卡片）.
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("openless-style-editor-card-rect"), card_rect)
    });
    // 遮罩、点击拦截与卡片必须同属**一个** `Area`（同一个 LayerId）：各自独立 Area 时，
    // egui 会在按下后把被点到的 Area 抬到同层最上（`move_to_top`），遮罩被抬起就会盖住
    // 卡片；同一图层里先画遮罩、再画卡片在结构上就不可能出现。这里用 `Foreground` 而不是
    // `Tooltip`：弹窗若占 Tooltip，会盖住同层弹出的下拉/菜单（跨 Order 是 Tooltip >
    // Foreground）。
    egui::Area::new(egui::Id::new("openless-style-editor-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(body.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(body.size());
            // Like Settings, sample this frame's GPU-blurred page and apply the
            // macOS overlay tint. Both layers obey the content area's corners.
            let corners = layout::body_corner_radius(ctx);
            if let Some(texture) = crate::ui::backdrop::published(ctx) {
                ui.painter().add(egui::Shape::Rect(
                    egui::epaint::RectShape::filled(body, corners, egui::Color32::WHITE)
                        .with_texture(texture, crate::ui::backdrop::uv_for(ctx, body)),
                ));
            }
            ui.painter().rect_filled(body, corners, theme::OVERLAY);
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
                        ui.set_min_size(card_rect.size());
                        ui.set_max_size(card_rect.size());
                        let inner = egui::Rect::from_min_size(
                            card_rect.min,
                            egui::vec2(card_rect.width(), card_rect.height()),
                        );
                        let header_height = drawer_header(ui, inner, vm, actions);
                        let content = egui::Rect::from_min_max(
                            egui::pos2(inner.left(), inner.top() + header_height),
                            inner.max,
                        );
                        // Header is painted at a fixed rectangle and does not advance
                        // egui's layout cursor. Anchor the scrollable body explicitly
                        // below it; otherwise the form starts under the header and
                        // ends one header-height before the drawer bottom.
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(content)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                            |ui| {
                                ui.set_clip_rect(content.intersect(ui.clip_rect()));
                                drawer_body(ui, content, vm, actions);
                            },
                        );
                    });
            });
        });
}

/// Fixed drawer header. Returns its height so the body can start below it.
fn drawer_header(
    ui: &mut egui::Ui,
    inner: egui::Rect,
    vm: &FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) -> f32 {
    let lang = vm.lang;
    let height = 78.0;
    let rect = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), height));
    let painter = ui.painter().with_clip_rect(rect);
    let text_left = rect.left() + 18.0;
    let text_width = (rect.width() - 36.0 - 34.0).max(80.0);
    painter.text(
        egui::pos2(text_left, rect.top() + 18.0),
        egui::Align2::LEFT_TOP,
        tr_l10n(lang, "style.pack.editorTitle"),
        egui::FontId::proportional(15.0),
        theme::INK,
    );
    // Two literal call sites on purpose: the i18n sync only registers catalog
    // keys it can find as a string literal next to `tr_l10n(…,`.
    let description = if vm.style_selection_workflow {
        tr_l10n(lang, "style.pack.selectionPromptEditorDesc")
    } else {
        tr_l10n(lang, "style.pack.dictationPromptEditorDesc")
    };
    let desc = layout::text_galley(ui, description, theme::INK_3, 12.0, text_width, 3);
    painter.galley(egui::pos2(text_left, rect.top() + 40.0), desc, theme::INK_3);
    // 28px circular close button, top-right.
    let close = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 18.0 - 28.0, rect.top() + 14.0),
        egui::vec2(28.0, 28.0),
    );
    if ui
        .interact(
            close,
            ui.id().with("style-editor-close"),
            egui::Sense::click(),
        )
        .on_hover_text(tr_l10n(lang, "style.pack.closeEditor"))
        .clicked()
    {
        actions.push(FrontendAction::StyleCloseEditor);
    }
    icons::draw_icon(ui, close.center(), IconName::Close, theme::INK_3);
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.bottom()),
            egui::pos2(rect.right(), rect.bottom()),
        ],
        egui::Stroke::new(0.5, theme::LINE),
    );
    height
}

fn drawer_body(
    ui: &mut egui::Ui,
    content: egui::Rect,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let field_width = (content.width() - 36.0 - FIELD_GAP) / 2.0;
    let scroll = egui::ScrollArea::vertical()
        .id_salt("style-editor-fields")
        .max_height(content.height().max(120.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Match the Tauri drawer's 18px scroll-body padding. A width
            // constraint alone leaves every field hard against the card edge.
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(18, 0))
                .show(ui, |ui| {
                    ui.set_width((content.width() - 36.0).max(120.0));
                    ui.spacing_mut().item_spacing.x = FIELD_GAP;
                    ui.add_space(18.0);
                    pills_row(ui, content, vm, actions);
                    ui.add_space(SECTION_GAP);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(field_width);
                            field_label(ui, tr_l10n(lang, "style.pack.fieldName"));
                            ui.add_sized(
                                [ui.available_width(), 38.0],
                                editor_input(&mut vm.style_name),
                            );
                        });
                        ui.vertical(|ui| {
                            ui.set_width(field_width);
                            field_label(ui, tr_l10n(lang, "style.pack.fieldAuthor"));
                            ui.add_sized(
                                [ui.available_width(), 38.0],
                                editor_input(&mut vm.style_author)
                                    .hint_text(tr_l10n(lang, "style.pack.fieldAuthorPlaceholder")),
                            );
                        });
                    });
                    ui.add_space(SECTION_GAP);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(field_width);
                            field_label(ui, tr_l10n(lang, "style.pack.fieldVersion"));
                            ui.add_sized(
                                [ui.available_width(), 38.0],
                                editor_input(&mut vm.style_version),
                            );
                        });
                        ui.vertical(|ui| {
                            ui.set_width(field_width);
                            field_label(ui, tr_l10n(lang, "style.pack.fieldTags"));
                            ui.add_sized(
                                [ui.available_width(), 38.0],
                                editor_input(&mut vm.style_tags)
                                    .hint_text(tr_l10n(lang, "style.pack.fieldTagsPlaceholder")),
                            );
                        });
                    });
                    ui.add_space(SECTION_GAP);
                    field_label(ui, tr_l10n(lang, "style.pack.fieldDescription"));
                    editor_textarea(ui, &mut vm.style_description, 86.0, "description");
                    ui.add_space(SECTION_GAP);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(field_width);
                            field_label(ui, tr_l10n(lang, "style.pack.fieldModel"));
                            ui.add_sized(
                                [ui.available_width(), 38.0],
                                editor_input(&mut vm.style_model)
                                    .hint_text(tr_l10n(lang, "style.pack.fieldModelPlaceholder")),
                            );
                            ui.label(
                                egui::RichText::new(tr_l10n(lang, "style.pack.fieldModelHint"))
                                    .size(11.5)
                                    .color(theme::INK_4),
                            );
                        });
                        ui.vertical(|ui| {
                            ui.set_width(field_width);
                            field_label(ui, tr_l10n(lang, "style.pack.fieldCompatibility"));
                            ui.add_sized(
                                [ui.available_width(), 38.0],
                                editor_input(&mut vm.style_compatible_version).hint_text(tr_l10n(
                                    lang,
                                    "style.pack.fieldCompatibilityPlaceholder",
                                )),
                            );
                        });
                    });
                    ui.add_space(SECTION_GAP + 4.0);

                    // The two workflows read different prompt slots from one pack, so the
                    // drawer only edits the one the page is currently showing.
                    if vm.style_selection_workflow {
                        prompt_field(
                            ui,
                            tr_l10n(lang, "style.pack.selectionPromptTitle"),
                            Some(tr_l10n(lang, "style.pack.selectionPromptHint")),
                            &mut vm.style_selection_prompt,
                            150.0,
                            "selection-prompt",
                        );
                        ui.add_space(SECTION_GAP);
                        prompt_field(
                            ui,
                            tr_l10n(lang, "style.pack.voiceEditPromptTitle"),
                            None,
                            &mut vm.style_voice_edit_prompt,
                            150.0,
                            "voice-edit-prompt",
                        );
                    } else {
                        prompt_field(
                            ui,
                            tr_l10n(lang, "style.pack.dictation_prompt_title"),
                            Some(tr_l10n(lang, "style.pack.dictation_prompt_hint")),
                            &mut vm.style_prompt,
                            210.0,
                            "dictation-prompt",
                        );
                    }

                    // The runtime card belongs to the dictation workflow: it shows which
                    // directives Core actually assembles into the prompt right now.
                    if !vm.style_selection_workflow {
                        ui.add_space(SECTION_GAP + 4.0);
                        runtime_card(ui, vm);
                    }

                    ui.add_space(18.0);
                    drawer_footer(ui, vm, actions);
                    ui.add_space(18.0);
                });
        });
    #[cfg(test)]
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new("style-editor-scroll-rect"), scroll.inner_rect);
        data.insert_temp(
            egui::Id::new("style-editor-scroll-content-height"),
            scroll.content_size.y,
        );
        data.insert_temp(
            egui::Id::new("style-editor-scroll-offset"),
            scroll.state.offset.y,
        );
    });
    #[cfg(not(test))]
    let _ = scroll;
}

/// Kind / mode / active / unsaved pills plus export and activate.
fn pills_row(
    ui: &mut egui::Ui,
    content: egui::Rect,
    vm: &mut FrontendViewModel,
    actions: &mut Vec<FrontendAction>,
) {
    let lang = vm.lang;
    let index = vm
        .style_packs
        .iter()
        .position(|pack| pack.id == vm.style_editor_id);
    let row = egui::Rect::from_min_size(
        egui::pos2(content.left() + 18.0, ui.cursor().top()),
        egui::vec2(content.width() - 36.0, 24.0),
    );
    ui.allocate_rect(row, egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(row);
    let mut x = row.left();
    let pill = |text: &str, tone: PillTone, x: &mut f32| {
        let size = layout::pill_size(ui, text);
        if *x + size.x > row.right() - 200.0 {
            return;
        }
        layout::paint_pill(
            &painter,
            egui::Rect::from_min_size(egui::pos2(*x, row.center().y - size.y / 2.0), size),
            text,
            tone,
        );
        *x += size.x + 6.0;
    };
    pill(
        tr_l10n(
            lang,
            if vm.style_editor_builtin {
                "style.pack.builtin"
            } else {
                "style.pack.imported"
            },
        ),
        if vm.style_editor_builtin {
            PillTone::Outline
        } else {
            PillTone::Blue
        },
        &mut x,
    );
    if !vm.style_editor_mode.is_empty() {
        let mode = vm.style_editor_mode.clone();
        pill(&mode, PillTone::Gray, &mut x);
    }
    if vm.style_editor_active {
        pill(tr_l10n(lang, "style.pack.active"), PillTone::Blue, &mut x);
    }
    if vm.style_editor_dirty {
        pill(
            tr_l10n(lang, "style.pack.unsaved"),
            PillTone::Outline,
            &mut x,
        );
    }

    // Right side: export ZIP + activate (both reuse the list-row actions).
    let activate = if vm.style_editor_active {
        tr_l10n(lang, "style.pack.active")
    } else {
        tr_l10n(lang, "style.pack.activate")
    };
    let activate_width = layout::text_width(ui, activate, 11.5) + 30.0;
    let activate_rect = egui::Rect::from_min_size(
        egui::pos2(row.right() - activate_width, row.center().y - 14.0),
        egui::vec2(activate_width, 28.0),
    );
    let export = tr_l10n(lang, "style.pack.exportZip");
    let export_width = layout::text_width(ui, export, 11.5) + 42.0;
    let export_rect = egui::Rect::from_min_size(
        egui::pos2(
            activate_rect.left() - 8.0 - export_width,
            row.center().y - 14.0,
        ),
        egui::vec2(export_width, 28.0),
    );
    if layout::action_button(
        ui,
        export_rect,
        export,
        Some(IconName::Download),
        ButtonKind::Ghost,
    )
    .clicked()
    {
        if let Some(index) = index {
            actions.push(FrontendAction::StyleExport(index));
        }
    }
    if layout::action_button(
        ui,
        activate_rect,
        activate,
        None,
        if vm.style_editor_active {
            ButtonKind::Ghost
        } else {
            ButtonKind::Blue
        },
    )
    .clicked()
        && !vm.style_editor_active
    {
        if let Some(index) = index {
            actions.push(FrontendAction::StyleActivate(index));
        }
    }
    ui.add_space(6.0);
}

fn editor_input(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .font(egui::FontId::proportional(12.5))
        .margin(egui::Margin::symmetric(11, 9))
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                .corner_radius(egui::CornerRadius::same(10)),
        )
}

/// Multiline TextEdit grows with its content even when passed to `add_sized`:
/// that size is a *minimum*, not a height cap. Keep the border fixed and let
/// long text scroll inside it, independently of the drawer's own scroll area.
fn editor_textarea(ui: &mut egui::Ui, text: &mut String, height: f32, id: &'static str) {
    let width = ui.available_width();
    let inner_height = height - 22.0; // 11px top and bottom, matching textareaStyle
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(12, 11))
        .show(ui, |ui| {
            ui.set_width((width - 24.0).max(1.0));
            let scroll = egui::ScrollArea::vertical()
                .id_salt(("style-editor-textarea", id))
                .max_height(inner_height)
                .min_scrolled_height(inner_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .font(egui::FontId::proportional(12.5))
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .desired_width(ui.available_width()),
                    );
                });
            #[cfg(test)]
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new(("style-editor-textarea-measure", id)),
                    (
                        scroll.content_size.y,
                        scroll.inner_rect.height(),
                        scroll.state.offset.y,
                    ),
                );
                data.insert_temp(
                    egui::Id::new(("style-editor-textarea-rect", id)),
                    scroll.inner_rect,
                )
            });
            #[cfg(not(test))]
            let _ = scroll;
        });
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(theme::INK),
    );
    ui.add_space(8.0);
}

fn prompt_field(
    ui: &mut egui::Ui,
    title: &str,
    hint: Option<&str>,
    value: &mut String,
    height: f32,
    id: &'static str,
) {
    field_label(ui, title);
    if let Some(hint) = hint {
        ui.label(egui::RichText::new(hint).size(11.0).color(theme::INK_4));
        ui.add_space(4.0);
    }
    editor_textarea(ui, value, height, id);
}

/// Dictation directives preview. Rendering only reads the DTO Core built, so the
/// UI can never disagree with the prompt the pipeline actually sends.
fn runtime_card(ui: &mut egui::Ui, vm: &FrontendViewModel) {
    let lang = vm.lang;
    let Some(runtime) = vm.style_runtime.as_ref() else {
        return;
    };
    egui::Frame::new()
        .fill(theme::SURFACE_2)
        .stroke(egui::Stroke::new(0.5, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width((ui.available_width() - 2.0).max(80.0));
            ui.label(
                egui::RichText::new(tr_l10n(lang, "style.pack.runtimeTitle"))
                    .size(13.0)
                    .strong()
                    .color(theme::INK),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(tr_l10n(lang, "style.pack.runtimeDesc"))
                    .size(11.5)
                    .color(theme::INK_4),
            );
            ui.add_space(12.0);
            runtime_row(
                ui,
                tr_l10n(lang, "style.pack.runtimeContextTitle"),
                tr_l10n(lang, "style.pack.runtimeContextDesc"),
                runtime.context_active,
                tr_l10n(lang, "style.pack.runtimeContextEmpty"),
                lang,
            );
            ui.add_space(8.0);
            runtime_row(
                ui,
                tr_l10n(lang, "style.pack.runtimeHotwordTitle"),
                tr_l10n(lang, "style.pack.runtimeHotwordDesc"),
                runtime.hotword_active,
                tr_l10n(lang, "style.pack.runtimeHotwordEmpty"),
                lang,
            );
            ui.add_space(8.0);
            runtime_row(
                ui,
                tr_l10n(lang, "style.pack.runtimeHistoryTitle"),
                tr_l10n(lang, "style.pack.runtimeHistoryDesc"),
                runtime.history_active,
                tr_l10n(lang, "style.pack.runtimeHistoryEmpty"),
                lang,
            );
            if runtime.omits_front_app {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "style.pack.runtimePreviewOmittedFrontApp"))
                        .size(11.5)
                        .color(theme::INK_4),
                );
            }
        });
}

/// One directive row: title + description on the left, active/inactive pill right.
fn runtime_row(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    active: bool,
    inactive_hint: &str,
    lang: Lang,
) {
    let (row, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(row);
    let pill_text = if active {
        tr_l10n(lang, "style.pack.runtimeActive")
    } else {
        tr_l10n(lang, "style.pack.runtimeInactive")
    };
    let pill_size = layout::pill_size(ui, pill_text);
    let text_width = (row.width() - pill_size.x - 12.0).max(60.0);
    let title_galley = layout::text_galley(ui, title, theme::INK, 12.0, text_width, 1);
    painter.galley(
        egui::pos2(row.left(), row.top() + 2.0),
        title_galley,
        theme::INK,
    );
    let hint = if active { detail } else { inactive_hint };
    let hint_galley = layout::text_galley(ui, hint, theme::INK_4, 11.0, text_width, 1);
    painter.galley(
        egui::pos2(row.left(), row.top() + 18.0),
        hint_galley,
        theme::INK_4,
    );
    layout::paint_pill(
        &painter,
        egui::Rect::from_min_size(
            egui::pos2(
                row.right() - pill_size.x,
                row.center().y - pill_size.y / 2.0,
            ),
            pill_size,
        ),
        pill_text,
        if active {
            PillTone::Blue
        } else {
            PillTone::Gray
        },
    );
}

/// Save / revert on the left, reset-or-delete on the right.
fn drawer_footer(ui: &mut egui::Ui, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let lang = vm.lang;
    let row_width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(egui::vec2(row_width, 32.0), egui::Sense::hover());
    let save = tr_l10n(lang, "style.pack.save");
    let save_width = layout::text_width(ui, save, 11.5) + 40.0;
    let save_rect = egui::Rect::from_min_size(row.min, egui::vec2(save_width, 30.0));
    // Upstream only enables Save while the draft differs from the stored pack.
    if vm.style_editor_dirty
        && layout::action_button(ui, save_rect, save, Some(IconName::Check), ButtonKind::Blue)
            .clicked()
    {
        if !vm.style_name.trim().is_empty() {
            actions.push(FrontendAction::StyleSaveEditor {
                name: vm.style_name.clone(),
                description: vm.style_description.clone(),
                prompt: vm.style_prompt.clone(),
                selection_prompt: vm.style_selection_prompt.clone(),
                voice_edit_prompt: vm.style_voice_edit_prompt.clone(),
                tags: vm.style_tags.clone(),
                author: vm.style_author.clone(),
                version: vm.style_version.clone(),
                model: vm.style_model.clone(),
                compatible_version: vm.style_compatible_version.clone(),
            });
        }
    } else if !vm.style_editor_dirty {
        // Still paint the disabled button so the layout does not jump.
        layout::action_button(
            ui,
            save_rect,
            save,
            Some(IconName::Check),
            ButtonKind::Disabled,
        );
    }

    let revert = tr_l10n(lang, "style.pack.revert");
    let revert_width = layout::text_width(ui, revert, 11.5) + 40.0;
    let revert_rect = egui::Rect::from_min_size(
        egui::pos2(save_rect.right() + 8.0, row.top()),
        egui::vec2(revert_width, 30.0),
    );
    let kind = if vm.style_editor_dirty {
        ButtonKind::Ghost
    } else {
        ButtonKind::Disabled
    };
    if layout::action_button(ui, revert_rect, revert, Some(IconName::Refresh), kind).clicked()
        && vm.style_editor_dirty
    {
        actions.push(FrontendAction::StyleRevertDraft);
    }

    // Built-in packs reset to Core's shipped prompt; imported ones are deleted.
    let secondary = if vm.style_editor_builtin {
        tr_l10n(lang, "style.pack.resetBuiltin")
    } else {
        tr_l10n(lang, "style.pack.deleteImported")
    };
    let secondary_width = layout::text_width(ui, secondary, 11.5) + 42.0;
    let secondary_rect = egui::Rect::from_min_size(
        egui::pos2(row.right() - secondary_width, row.top()),
        egui::vec2(secondary_width, 30.0),
    );
    if layout::action_button(
        ui,
        secondary_rect,
        secondary,
        Some(if vm.style_editor_builtin {
            IconName::Refresh
        } else {
            IconName::Trash
        }),
        ButtonKind::Ghost,
    )
    .clicked()
    {
        actions.push(if vm.style_editor_builtin {
            FrontendAction::StyleResetBuiltin
        } else {
            FrontendAction::StyleDeleteImported
        });
    }
}
