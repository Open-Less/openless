use eframe::egui;

use openless_linux_egui::{fmt_l10n, tr_l10n};

use super::icons::{self, IconName};
use super::theme;
use super::view_model::{FrontendAction, FrontendViewModel, Page};

pub const SIDEBAR_WIDTH: f32 = 188.0;
/// 自绘标题栏高度。对齐 Tauri 的 Linux 自绘标题栏（`LINUX_TITLEBAR_HEIGHT = 36`）。
pub const TITLEBAR_HEIGHT: f32 = 36.0;
/// 页面顶部留白。Tauri 的页面容器是 `padding: 56px 28px ...`（非 mac），
/// 所以「今日概览」这类标题离客户区顶边约 56px。
pub const PAGE_TOP_PADDING: f32 = 56.0;
/// 页面底部留白（Tauri：24px）。
pub const PAGE_BOTTOM_PADDING: f32 = 24.0;
const WINDOW_MARGIN: f32 = 6.0;
const WINDOW_RADIUS: u8 = 14;

// ── Window geometry helpers ─────────────────────────────────────────────────

pub fn window_rect(ctx: &egui::Context) -> egui::Rect {
    ctx.content_rect().shrink(WINDOW_MARGIN)
}

pub fn body_rect(ctx: &egui::Context) -> egui::Rect {
    let window = window_rect(ctx);
    egui::Rect::from_min_max(window.min + egui::vec2(0.0, TITLEBAR_HEIGHT), window.max)
}

// ── App icon ────────────────────────────────────────────────────────────────

pub fn load_app_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("openless-frontend-app-icon");
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let image = image::load_from_memory(include_bytes!("../../../../public/AppIcon.png"))
        .expect("OpenLess AppIcon.png must be valid")
        .into_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    let texture = ctx.load_texture("openless-app-icon", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

/// Decoded style-pack icon, cached in egui temp storage and keyed by the data
/// URL so a repaint never re-decodes the PNG (Core re-encodes only on change).
pub fn style_pack_icon_texture(
    ctx: &egui::Context,
    pack_id: &str,
    data_url: &str,
) -> Option<egui::TextureHandle> {
    use base64::Engine as _;
    let key = egui::Id::new(("openless-style-icon", pack_id));
    if let Some((cached_url, texture)) =
        ctx.data(|data| data.get_temp::<(String, egui::TextureHandle)>(key))
    {
        if cached_url == data_url {
            return Some(texture);
        }
    }
    let encoded = data_url.split_once(";base64,").map(|(_, value)| value)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let image = image::load_from_memory(&bytes).ok()?.into_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    let texture = ctx.load_texture(
        format!("openless-style-icon-{pack_id}"),
        color,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| {
        data.insert_temp(key, (data_url.to_string(), texture.clone()));
    });
    Some(texture)
}

pub fn paint_app_icon(ui: &egui::Ui, rect: egui::Rect, texture: &egui::TextureHandle) {
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

// ── Window background ───────────────────────────────────────────────────────

pub fn paint_window_background(ctx: &egui::Context) {
    let window = window_rect(ctx);
    let body = body_rect(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("openless-window-background"),
    ));
    // 整张窗口底板：白。内容区就是这块白（Tauri 的 `ol-console-main` 是
    // `--ol-surface`），不要再用灰画 body——否则侧栏/内容的白灰层次正好搞反。
    painter.rect_filled(
        window,
        egui::CornerRadius::same(WINDOW_RADIUS),
        theme::SURFACE,
    );
    let titlebar = egui::Rect::from_min_max(
        window.min,
        egui::pos2(window.max.x, window.min.y + TITLEBAR_HEIGHT),
    );
    painter.rect_filled(
        titlebar,
        egui::CornerRadius {
            nw: WINDOW_RADIUS,
            ne: WINDOW_RADIUS,
            sw: 0,
            se: 0,
        },
        theme::TITLEBAR,
    );
    painter.rect_filled(
        body,
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: WINDOW_RADIUS,
            se: WINDOW_RADIUS,
        },
        theme::SURFACE,
    );
    painter.line_segment(
        [
            egui::pos2(titlebar.left(), titlebar.bottom()),
            egui::pos2(titlebar.right(), titlebar.bottom()),
        ],
        egui::Stroke::new(1.0, theme::LINE),
    );
    painter.rect_stroke(
        window,
        egui::CornerRadius::same(WINDOW_RADIUS),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
}

// ── Titlebar ────────────────────────────────────────────────────────────────

pub fn titlebar(ctx: &egui::Context, actions: &mut Vec<FrontendAction>) {
    let window = window_rect(ctx);
    let titlebar = egui::Rect::from_min_max(
        window.min,
        egui::pos2(window.max.x, window.min.y + TITLEBAR_HEIGHT),
    );

    egui::Area::new(egui::Id::new("openless-titlebar"))
        .order(egui::Order::Middle)
        // The titlebar defines its own drag zone and window-control buttons.
        // Keep the area in the hit-test stack for its children, without adding
        // an area-wide click target that would consume their input.
        .sense(egui::Sense::hover())
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(window.width(), TITLEBAR_HEIGHT));

            let button_width = 40.0;
            let controls_left = titlebar.right() - button_width * 3.0;
            // The drag zone stops before the window controls so a press on the
            // buttons can never be claimed by the titlebar drag target.
            let drag_rect = egui::Rect::from_min_max(
                titlebar.min,
                egui::pos2(controls_left, titlebar.bottom()),
            );
            let drag = ui.interact(
                drag_rect,
                ui.id().with("titlebar-drag"),
                egui::Sense::click_and_drag(),
            );
            // Ask the compositor to move the window on the *press* frame: on
            // Wayland `xdg_toplevel.move` needs the pointer serial from that
            // event, so deferring to `drag_started` silently does nothing.
            let pressed_now =
                drag.is_pointer_button_down_on() && ui.input(|input| input.pointer.any_pressed());
            // `ViewportCommand::StartDrag` 在 egui-winit 里带着 `window.has_focus()`
            // 前置条件（egui-winit-0.33.3/src/lib.rs:1403-1409）：窗口尚未拿到键盘
            // 焦点的那一帧请求会被**整帧丢掉**，而 Wayland 的 `move` 只认按下那一帧
            // 的 serial——一次丢失就等于整个「按住标题栏拖」的手势失效（用户报的
            // 「拖标题栏移不动窗口」）。窗口未聚焦时按帧补发：焦点一翻转，同一帧的
            // 请求就会被放行。`viewport().focused` 就是 winit `has_focus()` 的同源值
            // （egui-winit `update_viewport_info`），所以判断不会偏。
            let focused = ui.input(|input| input.viewport().focused.unwrap_or(false));
            let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
            let holding = drag.is_pointer_button_down_on();
            let wants_drag = drag.drag_started() || pressed_now;
            if wants_drag && !focused {
                // 先把键盘焦点要回来：`StartDrag` 在 egui-winit 里要求 `has_focus()`，
                // 未聚焦时请求会被整帧丢掉（Wayland 的 `move` 只认按下那一帧的
                // serial，丢一次整个手势就废了）。
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if maximized && wants_drag {
                // Wayland 上最大化窗口不接受 move 请求（合成器直接忽略），先还原再拖。
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            } else if wants_drag || (holding && !focused) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                actions.push(FrontendAction::WindowMaximize);
            }

            let texture = load_app_icon(ctx);
            paint_app_icon(
                ui,
                egui::Rect::from_center_size(
                    window.min + egui::vec2(16.0, TITLEBAR_HEIGHT / 2.0),
                    egui::vec2(26.0, 26.0),
                ),
                &texture,
            );
            ui.painter().text(
                window.min + egui::vec2(34.0, TITLEBAR_HEIGHT / 2.0 + 0.5),
                egui::Align2::LEFT_CENTER,
                "OpenLess",
                egui::FontId::proportional(13.0),
                theme::INK_2,
            );

            let close = egui::Rect::from_min_max(
                egui::pos2(titlebar.right() - button_width, titlebar.top()),
                titlebar.right_bottom(),
            );
            let maximize = close.translate(egui::vec2(-button_width, 0.0));
            let minimize = maximize.translate(egui::vec2(-button_width, 0.0));
            let close_response = ui.interact(close, ui.id().with("close"), egui::Sense::click());
            let maximize_response =
                ui.interact(maximize, ui.id().with("maximize"), egui::Sense::click());
            let minimize_response =
                ui.interact(minimize, ui.id().with("minimize"), egui::Sense::click());
            if close_response.clicked() {
                actions.push(FrontendAction::WindowClose);
            }
            if maximize_response.clicked() {
                actions.push(FrontendAction::WindowMaximize);
            }
            if minimize_response.clicked() {
                actions.push(FrontendAction::WindowMinimize);
            }
            for (rect, response) in [
                (minimize, &minimize_response),
                (maximize, &maximize_response),
            ] {
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(6), theme::SURFACE_2);
                }
            }
            if close_response.hovered() {
                let center = close.center();
                let arm = 5.0;
                let stroke = egui::Stroke::new(1.0, theme::ERR);
                ui.painter().line_segment(
                    [
                        center + egui::vec2(-arm, -arm),
                        center + egui::vec2(arm, arm),
                    ],
                    stroke,
                );
                ui.painter().line_segment(
                    [
                        center + egui::vec2(arm, -arm),
                        center + egui::vec2(-arm, arm),
                    ],
                    stroke,
                );
            }

            let stroke = egui::Stroke::new(1.0, theme::INK_3);
            // Minimize: a single horizontal line.
            ui.painter().line_segment(
                [
                    minimize.center() - egui::vec2(5.0, 0.0),
                    minimize.center() + egui::vec2(5.0, 0.0),
                ],
                stroke,
            );
            // Maximize shows a square, restored windows show the two-square glyph.
            let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            let center = maximize.center();
            if maximized {
                let half = 4.0;
                ui.painter().rect_stroke(
                    egui::Rect::from_min_max(
                        center + egui::vec2(-half - 2.0, -half),
                        center + egui::vec2(half - 2.0, half),
                    ),
                    egui::CornerRadius::ZERO,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                ui.painter().rect_stroke(
                    egui::Rect::from_min_max(
                        center + egui::vec2(-half + 2.0, -half + 2.0),
                        center + egui::vec2(half + 2.0, half + 2.0),
                    ),
                    egui::CornerRadius::ZERO,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            } else {
                ui.painter().rect_stroke(
                    egui::Rect::from_center_size(center, egui::vec2(10.0, 10.0)),
                    egui::CornerRadius::ZERO,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
            let close_stroke = egui::Stroke::new(
                1.0,
                if close_response.hovered() {
                    theme::ERR
                } else {
                    theme::INK_3
                },
            );
            ui.painter().line_segment(
                [
                    close.center() - egui::vec2(5.0, 5.0),
                    close.center() + egui::vec2(5.0, 5.0),
                ],
                close_stroke,
            );
            ui.painter().line_segment(
                [
                    close.center() + egui::vec2(5.0, -5.0),
                    close.center() + egui::vec2(-5.0, 5.0),
                ],
                close_stroke,
            );
        });
}

// ── Resize handles ──────────────────────────────────────────────────────────

pub fn resize_handles(ctx: &egui::Context) {
    let window = window_rect(ctx);
    // Keep the draggable titlebar band generous: only a thin strip resizes.
    let edge = 6.0;
    let corner = 18.0;
    let left = window.left();
    let right = window.right();
    let top = window.top();
    let bottom = window.bottom();
    let zones = [
        (
            egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(left + corner, top + corner),
            ),
            egui::ResizeDirection::NorthWest,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - corner, top),
                egui::pos2(right, top + corner),
            ),
            egui::ResizeDirection::NorthEast,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left, bottom - corner),
                egui::pos2(left + corner, bottom),
            ),
            egui::ResizeDirection::SouthWest,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - corner, bottom - corner),
                egui::pos2(right, bottom),
            ),
            egui::ResizeDirection::SouthEast,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left + corner, top),
                egui::pos2(right - corner, top + edge),
            ),
            egui::ResizeDirection::North,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left + corner, bottom - edge),
                egui::pos2(right - corner, bottom),
            ),
            egui::ResizeDirection::South,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(left, top + corner),
                egui::pos2(left + edge, bottom - corner),
            ),
            egui::ResizeDirection::West,
        ),
        (
            egui::Rect::from_min_max(
                egui::pos2(right - edge, top + corner),
                egui::pos2(right, bottom - corner),
            ),
            egui::ResizeDirection::East,
        ),
    ];

    // Each edge gets its own foreground area. A single window-sized Area would
    // become the top hit-test layer for the entire UI, including its transparent
    // interior, and would swallow every button click.
    for (index, (rect, direction)) in zones.into_iter().enumerate() {
        let response = egui::Area::new(egui::Id::new(("openless-resize", index)))
            // 条带要在设置遮罩（Foreground）之上：遮罩盖住了 body 范围内的外圈像素，
            // 否则弹窗打开时拖边缘会被遮罩吃掉、窗口改不了大小（用户报「设置页面下
            // 无法修改窗口大小」）。Tooltip 只是层级，条带本身仍只占外圈 6px/18px。
            .order(egui::Order::Tooltip)
            .fixed_pos(rect.min)
            .default_size(rect.size())
            .sense(egui::Sense::drag())
            .show(ctx, |ui| ui.set_min_size(rect.size()))
            .response;
        // 悬停到拖拽区要换成对应方向的拉伸光标：没有它用户看不出「这里能拉伸」
        // （无边框窗口唯一的提示就是光标形状）。这里直接按指针位置判定：Area 的
        // `hovered()` 依赖上一帧的命中信息，首帧不生效，而光标必须立刻对。
        let hovering = ctx
            .input(|input| input.pointer.latest_pos())
            .is_some_and(|position| rect.contains(position));
        if hovering {
            ctx.set_cursor_icon(resize_cursor(direction));
        }
        let _ = &response;
        if response.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

/// 拖拽方向 → 拉伸光标。
fn resize_cursor(direction: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::ResizeDirection as D;
    match direction {
        D::North | D::South => egui::CursorIcon::ResizeVertical,
        D::East | D::West => egui::CursorIcon::ResizeHorizontal,
        D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
        D::NorthEast | D::SouthWest => egui::CursorIcon::ResizeNeSw,
    }
}

// ── Sidebar ─────────────────────────────────────────────────────────────────

pub fn sidebar(ctx: &egui::Context, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    let body = body_rect(ctx);
    egui::Area::new(egui::Id::new("openless-sidebar"))
        .order(egui::Order::Middle)
        // Navigation rows own their input. A hover-only area preserves their
        // layer while avoiding an invisible area-wide click target.
        .sense(egui::Sense::hover())
        .fixed_pos(body.min)
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(SIDEBAR_WIDTH, body.height()));
            // Constrain the max size too: without it `available_height()` is the
            // whole screen and the pinned settings row lands off-window.
            ui.set_max_size(egui::vec2(SIDEBAR_WIDTH, body.height()));
            ui.set_clip_rect(egui::Rect::from_min_size(
                body.min,
                egui::vec2(SIDEBAR_WIDTH, body.height()),
            ));
            // Paint the exact sidebar rect: `ui.max_rect()` can be the whole
            // screen, which pushed the rounded bottom-left corner off-window.
            let sidebar_rect =
                egui::Rect::from_min_size(body.min, egui::vec2(SIDEBAR_WIDTH, body.height()));
            ui.painter().rect_filled(
                sidebar_rect,
                egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: WINDOW_RADIUS,
                    se: 0,
                },
                theme::SIDEBAR,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(body.left() + SIDEBAR_WIDTH, body.top()),
                    egui::pos2(body.left() + SIDEBAR_WIDTH, body.bottom()),
                ],
                egui::Stroke::new(1.0, theme::LINE),
            );
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 12))
                .show(ui, |ui| {
                    ui.set_width(SIDEBAR_WIDTH - 20.0);
                    // 版本信息行：Tauri 把原「OpenLess」品牌位换成版本信息（BETA 徽章
                    // 与版本号）。侧栏只有 188px，两者同行的结果是把版本号拦腰折断
                    // （「版本 v2.0.0-」/「Beta.2+…」），所以徽章占一行、版本号另起一行。
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        beta_badge(ui, vm.lang);
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(fmt_l10n(vm.lang, "shell.version", &[&vm.version]))
                                .size(12.0)
                                .color(theme::INK_4),
                        );
                    });
                    ui.add_space(12.0);
                    // The nav scrolls when the window is short so the pinned
                    // settings row stays reachable.
                    const PINNED_SETTINGS_HEIGHT: f32 = 46.0;
                    let nav_height = (ui.available_height() - PINNED_SETTINGS_HEIGHT).max(80.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(SIDEBAR_WIDTH - 20.0, nav_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("openless-sidebar-nav")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(SIDEBAR_WIDTH - 20.0);
                                    nav(
                                        ui,
                                        vm,
                                        "nav.overview",
                                        NavTarget::Page(Page::Overview),
                                        IconName::Overview,
                                        actions,
                                    );
                                    nav(
                                        ui,
                                        vm,
                                        "nav.history",
                                        NavTarget::Page(Page::History),
                                        IconName::History,
                                        actions,
                                    );
                                    nav(
                                        ui,
                                        vm,
                                        "nav.vocab",
                                        NavTarget::Page(Page::Vocab),
                                        IconName::Vocab,
                                        actions,
                                    );
                                    ui.add_space(4.0);
                                    group(
                                        ui,
                                        vm,
                                        "nav.group_style",
                                        IconName::Style,
                                        vm.style_open,
                                        FrontendAction::SidebarToggleStyle,
                                        actions,
                                    );
                                    if vm.style_open {
                                        subnav(
                                            ui,
                                            vm,
                                            "nav.polish_mode",
                                            NavTarget::Page(Page::Style),
                                            actions,
                                        );
                                        subnav(
                                            ui,
                                            vm,
                                            "nav.marketplace",
                                            NavTarget::Page(Page::Marketplace),
                                            actions,
                                        );
                                    }
                                    group(
                                        ui,
                                        vm,
                                        "nav.group_tools",
                                        IconName::SelectionAsk,
                                        vm.tools_open,
                                        FrontendAction::SidebarToggleTools,
                                        actions,
                                    );
                                    if vm.tools_open {
                                        subnav(
                                            ui,
                                            vm,
                                            "nav.translation",
                                            NavTarget::Page(Page::Translation),
                                            actions,
                                        );
                                        subnav(
                                            ui,
                                            vm,
                                            "nav.selection_ask",
                                            NavTarget::Page(Page::SelectionAsk),
                                            actions,
                                        );
                                        subnav(
                                            ui,
                                            vm,
                                            "nav.quickNote",
                                            NavTarget::Page(Page::QuickNote),
                                            actions,
                                        );
                                        subnav(
                                            ui,
                                            vm,
                                            "nav.corrections",
                                            NavTarget::Page(Page::Corrections),
                                            actions,
                                        );
                                    }
                                });
                        },
                    );
                    ui.add_space(4.0);
                    nav_with_icon(
                        ui,
                        vm,
                        "nav.settings",
                        NavTarget::Page(Page::Settings),
                        IconName::Settings,
                        actions,
                    );
                });
        });
}

/// Where a sidebar row navigates to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NavTarget {
    Page(Page),
}

fn nav_active(vm: &FrontendViewModel, target: NavTarget) -> bool {
    match target {
        // Settings is an overlay: it highlights while open instead of owning a page.
        NavTarget::Page(Page::Settings) => vm.settings_open,
        NavTarget::Page(page) => vm.active_page == page,
    }
}

fn nav_click(target: NavTarget, actions: &mut Vec<FrontendAction>) {
    match target {
        NavTarget::Page(Page::Settings) => {
            // Keep the current page rendered behind the modal.
            actions.push(FrontendAction::ToggleSettings);
        }
        NavTarget::Page(page) => {
            actions.push(FrontendAction::Navigate(page));
        }
    }
}

fn nav(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    key: &'static str,
    target: NavTarget,
    icon: IconName,
    actions: &mut Vec<FrontendAction>,
) {
    nav_with_icon(ui, vm, key, target, icon, actions);
}

/// Tauri 的 BETA 徽章（`FloatingShell` 版本信息行）：蓝字、透明底、0.5px 蓝描边、
/// 圆角 5、内边距 1×6、字号 10 + 字重 600。区别于其它胶囊的灰/淡蓝实底。
fn beta_badge(ui: &mut egui::Ui, lang: openless_linux_egui::Lang) {
    let label = tr_l10n(lang, "shell.beta_tag");
    let font = theme::medium_font(10.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme::BLUE);
    let padding = egui::vec2(6.0, 1.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + padding * 2.0, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(5),
        egui::Stroke::new(0.8, theme::BLUE_PILL_BORDER),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + padding, galley, theme::BLUE);
}

fn nav_with_icon(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    key: &'static str,
    target: NavTarget,
    icon: IconName,
    actions: &mut Vec<FrontendAction>,
) {
    let label = tr_l10n(vm.lang, key);
    let active = nav_active(vm, target);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    }
    let color = if active { theme::INK } else { theme::INK_3 };
    icons::draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
    ui.painter().text(
        rect.min + egui::vec2(38.0, 16.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    if response.clicked() {
        nav_click(target, actions);
    }
}

fn subnav(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    key: &'static str,
    target: NavTarget,
    actions: &mut Vec<FrontendAction>,
) {
    let label = tr_l10n(vm.lang, key);
    let active = nav_active(vm, target);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 30.0), egui::Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_2);
    }
    ui.painter().text(
        rect.min + egui::vec2(30.0, 15.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        if active { theme::INK } else { theme::INK_3 },
    );
    if response.clicked() {
        nav_click(target, actions);
    }
}

#[allow(clippy::too_many_arguments)]
fn group(
    ui: &mut egui::Ui,
    vm: &mut FrontendViewModel,
    key: &'static str,
    icon: IconName,
    is_open: bool,
    toggle: FrontendAction,
    actions: &mut Vec<FrontendAction>,
) {
    let label = tr_l10n(vm.lang, key);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(SIDEBAR_WIDTH - 20.0, 32.0), egui::Sense::click());
    let color = if response.hovered() {
        theme::INK_2
    } else {
        theme::INK_3
    };
    icons::draw_icon(ui, rect.min + egui::vec2(20.0, 16.0), icon, color);
    ui.painter().text(
        rect.min + egui::vec2(38.0, 16.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    let x = rect.max.x - 18.0;
    let y = rect.center().y;
    if is_open {
        ui.painter().line_segment(
            [egui::pos2(x - 3.0, y - 1.0), egui::pos2(x, y + 2.0)],
            egui::Stroke::new(1.2, color),
        );
        ui.painter().line_segment(
            [egui::pos2(x, y + 2.0), egui::pos2(x + 3.0, y - 1.0)],
            egui::Stroke::new(1.2, color),
        );
    } else {
        ui.painter().line_segment(
            [egui::pos2(x - 1.0, y - 3.0), egui::pos2(x + 2.0, y)],
            egui::Stroke::new(1.2, color),
        );
        ui.painter().line_segment(
            [egui::pos2(x + 2.0, y), egui::pos2(x - 1.0, y + 3.0)],
            egui::Stroke::new(1.2, color),
        );
    }
    if response.clicked() {
        actions.push(toggle);
    }
}

// ── Content panel ───────────────────────────────────────────────────────────

pub fn content_panel(ctx: &egui::Context, add_contents: impl FnOnce(&mut egui::Ui)) {
    let body = body_rect(ctx);
    // 页面留白对齐 Tauri 的页面容器（`padding: 56px 28px 24px`）：标题离标题栏
    // 下沿 56px，左右 28px，底部 24px。
    let content = egui::Rect::from_min_max(
        egui::pos2(
            body.left() + SIDEBAR_WIDTH + 28.0,
            body.top() + PAGE_TOP_PADDING,
        ),
        egui::pos2(body.right() - 2.0, body.bottom() - PAGE_BOTTOM_PADDING),
    );
    egui::Area::new(egui::Id::new("openless-content"))
        .order(egui::Order::Middle)
        // Buttons and text fields inside the panel register their own hit targets.
        .sense(egui::Sense::hover())
        .fixed_pos(content.min)
        .show(ctx, |area| {
            // Area starts each frame with its *previous* measured size. A resize
            // from a large window therefore leaves its root Ui's minimum width
            // and height too large; set_max_size cannot shrink an existing min
            // rect. Build a fresh child with the CURRENT viewport instead of
            // letting yesterday's geometry drive today's scroll/hit testing.
            let mut ui = area.new_child(
                egui::UiBuilder::new()
                    .id_salt("current-content-viewport")
                    .max_rect(content),
            );
            ui.set_clip_rect(content);
            #[cfg(test)]
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("content-available-size"), ui.available_size());
            });
            let scroll = &mut ui.style_mut().spacing.scroll;
            scroll.floating = true;
            scroll.bar_width = 8.0;
            scroll.handle_min_length = 24.0;
            scroll.bar_inner_margin = 0.0;
            scroll.bar_outer_margin = 0.0;
            scroll.foreground_color = false;
            scroll.floating_width = 6.0;
            // 预留滚动条槽位：Tauri 用 `scrollbar-gutter: stable` 常驻 6px，
            // 滚动条不会盖住右边那列文字（之前是 0.0 → 悬浮条压字）。
            scroll.floating_allocated_width = 8.0;
            let visuals = &mut ui.style_mut().visuals.widgets;
            visuals.inactive.corner_radius = egui::CornerRadius::same(6);
            visuals.hovered.corner_radius = egui::CornerRadius::same(6);
            visuals.active.corner_radius = egui::CornerRadius::same(6);
            add_contents(&mut ui);
            // Keep the Area's hit-test bounds aligned to the resized viewport;
            // the child does not advance its parent cursor automatically.
            area.set_min_size(content.size());
        });
}

// ── Shared helpers ──────────────────────────────────────────────────────────

/// A stable per-card salt derived from its position, so child widget ids stay
/// unique across the cards on a page without threading a name through.
pub fn card_salt(rect: egui::Rect) -> (i32, i32) {
    (rect.left().round() as i32, rect.top().round() as i32)
}

/// Run `contents` inside a child `Ui` pinned to `rect`, **without** moving the
/// parent cursor.
///
/// `Ui::scope_builder` / `Ui::scope` advance the parent cursor to the child's
/// *used* rect (`scope_dyn` calls `advance_cursor_after_rect`). Cards on these
/// pages are positioned explicitly and mostly paint instead of allocating, so
/// a scope would rewind the cursor and make the next row overlap the card.
/// Use this (or `card_at`) for absolutely positioned content.
pub fn fixed_ui<R>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id_salt)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(child.clip_rect().intersect(rect));
    contents(&mut child)
}

/// Width of `text` at `size` points, measured with the live font atlas.
/// Needed wherever a control is laid out by hand rather than by egui's cursor.
pub fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(size),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Transparent fill, hairline border, ink text (the default toolbar look).
    Ghost,
    /// Filled with the accent blue, white text.
    Blue,
    /// Greyed-out button that swallows clicks (Tauri's 置灰 停用).
    Disabled,
}

/// A bordered button drawn into an exact rectangle, with an optional leading
/// icon. Used by the pages that position their cards explicitly.
pub fn action_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: &str,
    icon: Option<IconName>,
    kind: ButtonKind,
) -> egui::Response {
    let id = ui.id().with((
        "openless-action-button",
        label,
        rect.left().round() as i32,
        rect.top().round() as i32,
    ));
    let response = ui.interact(rect, id, egui::Sense::click());
    let (fill, stroke, ink) = match kind {
        ButtonKind::Ghost => (
            if response.hovered() {
                theme::SURFACE_2
            } else {
                egui::Color32::TRANSPARENT
            },
            Some(egui::Stroke::new(0.8, theme::LINE)),
            theme::INK_2,
        ),
        ButtonKind::Blue => (
            if response.hovered() {
                theme::BLUE.linear_multiply(0.92)
            } else {
                theme::BLUE
            },
            None,
            egui::Color32::WHITE,
        ),
        ButtonKind::Disabled => (
            egui::Color32::TRANSPARENT,
            Some(egui::Stroke::new(0.5, theme::LINE_SOFT)),
            theme::INK_4.linear_multiply(0.6),
        ),
    };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(8), fill);
    if let Some(stroke) = stroke {
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(8),
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    let label_width = text_width(ui, label, 12.5);
    let icon_space = if icon.is_some() { 19.0 } else { 0.0 };
    let mut x = rect.center().x - (label_width + icon_space) / 2.0;
    if let Some(icon) = icon {
        // 只有图标的按钮（例如历史详情的「…」菜单）把图标居中。
        let icon_center = if label.is_empty() {
            rect.center()
        } else {
            egui::pos2(x + 6.5, rect.center().y)
        };
        icons::draw_icon(ui, icon_center, icon, ink);
        x += icon_space;
    }
    painter.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        ink,
    );
    response
}

/// Draw the standard page header (uppercase kicker, title, optional desc) and
/// return the row so the caller can place right-aligned actions on it.
pub fn page_header(
    ui: &mut egui::Ui,
    width: f32,
    kicker: &str,
    title: &str,
    desc: Option<&str>,
) -> egui::Rect {
    let height = if desc.is_some() { 84.0 } else { 60.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.top() + 2.0),
        egui::Align2::LEFT_TOP,
        kicker,
        egui::FontId::proportional(11.0),
        theme::INK_4,
    );
    painter.text(
        egui::pos2(rect.left(), rect.top() + 18.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(26.0),
        theme::INK,
    );
    if let Some(desc) = desc {
        painter.text(
            egui::pos2(rect.left(), rect.top() + 56.0),
            egui::Align2::LEFT_TOP,
            desc,
            egui::FontId::proportional(13.0),
            theme::INK_3,
        );
    }
    rect
}

/// Paint the standard card background (white, hairline border, 14pt radius).
pub fn paint_card(painter: &egui::Painter, rect: egui::Rect) {
    // Tauri `Card`: --ol-r-lg 圆角 + 0.5px --ol-line 边框 + --ol-shadow-sm。
    painter.rect_filled(rect, egui::CornerRadius::same(14), theme::SURFACE);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(14),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
}

/// A card with padded contents that never moves the parent layout cursor.
pub fn card(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    padding: f32,
    contents: impl FnOnce(&mut egui::Ui, egui::Rect),
) {
    paint_card(ui.painter(), rect);
    let inner = rect.shrink(padding);
    fixed_ui(ui, inner, card_salt(rect), |ui| contents(ui, inner));
}

/// iOS-style switch painted into an exact rectangle.
pub fn toggle(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    on: bool,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(id_salt), egui::Sense::click());
    // Tauri `Toggle`: 开启 --ol-blue，关闭 --ol-toggle-off-bg。
    let track = if on { theme::BLUE } else { theme::TOGGLE_OFF };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), track);
    let knob_x = if on {
        rect.right() - 10.0
    } else {
        rect.left() + 10.0
    };
    ui.painter().circle_filled(
        egui::pos2(knob_x, rect.center().y),
        8.0,
        egui::Color32::WHITE,
    );
    response
}

/// Lay out text into a galley with a width/row limit. Used by pages that paint
/// their content at explicit positions instead of with the layout cursor.
pub fn text_galley(
    ui: &egui::Ui,
    text: &str,
    color: egui::Color32,
    size: f32,
    max_width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = max_rows;
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// Width of a segmented control for `options`.
pub fn segmented_width(ui: &egui::Ui, options: &[&str]) -> f32 {
    let mut width = 4.0;
    for (index, option) in options.iter().enumerate() {
        if index > 0 {
            width += 2.0;
        }
        width += text_width(ui, option, 12.0) + 18.0;
    }
    width
}

/// A segmented button group (the Tauri `ol-seg` control). Returns the index the
/// user clicked. Every segment is a real button, not a text label.
pub fn segmented(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    options: &[&str],
    selected: usize,
) -> Option<usize> {
    let painter = ui.painter().with_clip_rect(rect);
    // Tauri track: rgba(0,0,0,0.04) with a 2px inset; the active chip is a white
    // surface with a hairline + soft shadow, the label stays ink-colored.
    painter.rect_filled(rect, egui::CornerRadius::same(8), theme::SEGMENTED_TRACK);
    let mut x = rect.left() + 2.0;
    let mut clicked = None;
    for (index, option) in options.iter().enumerate() {
        let width = text_width(ui, option, 12.0) + 18.0;
        let option_rect = egui::Rect::from_min_size(
            egui::pos2(x, rect.top() + 2.0),
            egui::vec2(width, rect.height() - 4.0),
        );
        let id = ui.id().with((
            "openless-segment",
            index,
            rect.left().round() as i32,
            rect.top().round() as i32,
        ));
        let response = ui.interact(option_rect, id, egui::Sense::click());
        let is_selected = index == selected;
        if is_selected {
            // Tauri `--ol-segmented-active-shadow` = `0 1px 2px rgba(0,0,0,0.06), 0 0 0 0.5px rgba(0,0,0,0.06)`：
            // 选中片是白底、无描边，靠向下 1px 的浅投影 + 0.5px 细环从轨道上"浮"起来。
            painter.rect_filled(
                option_rect.expand(1.0).translate(egui::vec2(0.0, 1.0)),
                egui::CornerRadius::same(7),
                theme::SEGMENTED_ACTIVE_SHADOW,
            );
            painter.rect_filled(
                option_rect,
                egui::CornerRadius::same(6),
                theme::SEGMENTED_ACTIVE_BG,
            );
            painter.rect_stroke(
                option_rect,
                egui::CornerRadius::same(6),
                egui::Stroke::new(0.5, theme::SEGMENTED_ACTIVE_RING),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(
                option_rect,
                egui::CornerRadius::same(6),
                egui::Color32::from_white_alpha(140),
            );
        }
        painter.text(
            option_rect.center(),
            egui::Align2::CENTER_CENTER,
            option,
            theme::medium_font(12.0),
            if is_selected {
                theme::INK
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
/// A segmented button group (the Tauri `ol-seg` control). Returns the index the
/// user clicked. Every segment is a real button, not a text label.
/// Tauri `inputStyle`（`src/pages/settings/shared.tsx:243-256`）：高度 32、字号 13.5、
/// 左右 padding 10、radius 8、底色 `--ol-select-trigger-bg`（= `--ol-control-solid`）、
/// 最大宽度 360。所有设置页的文本输入都走这里，不再逐处写尺寸。
pub const INPUT_HEIGHT: f32 = 32.0;
pub const INPUT_FONT_SIZE: f32 = 13.5;
pub const INPUT_MAX_WIDTH: f32 = 360.0;

/// A single-line text input styled like the Tauri settings rows.
///
/// `password` maps to Tauri's `type="password"`（密钥/令牌字段）。
pub fn text_input(
    ui: &mut egui::Ui,
    value: &mut String,
    id: egui::Id,
    hint: &str,
    width: f32,
    password: bool,
) -> egui::Response {
    let width = width.clamp(80.0, INPUT_MAX_WIDTH);
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, INPUT_HEIGHT), egui::Sense::hover());
    ui.painter()
        .rect_filled(outer, egui::CornerRadius::same(8), theme::SURFACE);
    ui.painter().rect_stroke(
        outer,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.5, theme::LINE_STRONG),
        egui::StrokeKind::Inside,
    );
    // padding 0/10：左右缩进 10，纵向留 1 抵消 0.5px 描边，文字由 vertical_align 居中。
    let inner = outer.shrink2(egui::vec2(10.0, 1.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let mut edit = egui::TextEdit::singleline(value)
        .id(id)
        .hint_text(hint)
        .font(egui::FontId::proportional(INPUT_FONT_SIZE))
        .text_color(theme::INK)
        .frame(egui::Frame::NONE)
        .desired_width(inner.width())
        .vertical_align(egui::Align::Center);
    if password {
        edit = edit.password(true);
    }
    child.add(edit)
}

/// A labelled section block: title plus optional smaller description line.
pub fn section_title(ui: &mut egui::Ui, width: f32, title: &str, desc: Option<&str>) {
    let height = if desc.is_some() { 40.0 } else { 20.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.text(
        egui::pos2(rect.left(), rect.top()),
        egui::Align2::LEFT_TOP,
        title,
        // Tauri `SectionTitle`: 14 / 600。
        egui::FontId::proportional(14.0),
        theme::INK,
    );
    if let Some(desc) = desc {
        painter.text(
            egui::pos2(rect.left(), rect.top() + 20.0),
            egui::Align2::LEFT_TOP,
            desc,
            egui::FontId::proportional(11.5),
            theme::INK_4,
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillTone {
    /// Transparent fill with a hairline border (used for "raw").
    Outline,
    /// Neutral `SURFACE_2` fill.
    Gray,
    /// Accent-tinted fill.
    Blue,
    /// Derived marketplace packs.
    Green,
}

/// Natural size of a small pill for `text`.
pub fn pill_size(ui: &egui::Ui, text: &str) -> egui::Vec2 {
    egui::vec2(text_width(ui, text, 10.5) + 16.0, 18.0)
}

/// Paint a small rounded pill into an exact rectangle.
pub fn paint_pill(painter: &egui::Painter, rect: egui::Rect, text: &str, tone: PillTone) {
    let (fill, border, color) = match tone {
        PillTone::Outline => (egui::Color32::TRANSPARENT, Some(theme::LINE), theme::INK_3),
        PillTone::Gray => (theme::SURFACE_2, None, theme::INK_3),
        PillTone::Blue => (theme::BLUE_SOFT, None, theme::BLUE),
        PillTone::Green => (egui::Color32::from_rgb(235, 250, 239), None, theme::OK),
    };
    painter.rect_filled(rect, egui::CornerRadius::same(9), fill);
    if let Some(border) = border {
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(9),
            egui::Stroke::new(0.7, border),
            egui::StrokeKind::Inside,
        );
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(10.5),
        color,
    );
}

pub fn unsupported_page(ui: &mut egui::Ui, lang: openless_linux_egui::Lang, title: &str) {
    ui.add_space(28.0);
    if !title.is_empty() {
        ui.label(
            egui::RichText::new(title)
                .size(28.0)
                .strong()
                .color(theme::INK),
        );
        ui.add_space(22.0);
    }
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(28))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(openless_linux_egui::tr_l10n(
                        lang,
                        "common.unsupported_title",
                    ))
                    .size(13.0)
                    .color(theme::INK_3),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(openless_linux_egui::tr_l10n(
                        lang,
                        "common.unsupported_hint",
                    ))
                    .size(11.0)
                    .color(theme::INK_4),
                );
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_metrics_match_the_tauri_input_style() {
        // Tauri `inputStyle`（settings/shared.tsx:243-256）：height 32 / fontSize 13.5 /
        // padding 0 10 / maxWidth 360。这些取值被设置页所有文本输入共用，改动必须是有意的。
        assert_eq!(INPUT_HEIGHT, 32.0);
        assert_eq!(INPUT_FONT_SIZE, 13.5);
        assert_eq!(INPUT_MAX_WIDTH, 360.0);
    }

    #[test]
    fn the_selected_segment_uses_the_tauri_shadow_tokens() {
        // Tauri `--ol-segmented-active-shadow` 的两段：细环 + 向下 1px 的浅投影。
        // 选中片没有描边（border: 0），所以环不能等于普通的 --ol-line。
        assert_ne!(theme::SEGMENTED_ACTIVE_RING, theme::LINE);
        assert_eq!(theme::SEGMENTED_ACTIVE_BG, theme::SURFACE);
        assert!(theme::SEGMENTED_ACTIVE_SHADOW.a() > 0);
        assert!(theme::SEGMENTED_ACTIVE_SHADOW.a() < theme::SEGMENTED_ACTIVE_RING.a());
    }
}
