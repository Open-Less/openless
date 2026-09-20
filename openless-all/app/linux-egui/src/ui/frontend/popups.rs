//! Three auxiliary windows: the dictation capsule, the selection-ask panel and
//! the selection-polish preview.
//!
//! Like the page layer this module is a pure renderer: it reads the popup
//! snapshot and returns at most one action, which the popup host translates into
//! a `PopupToHost` message. Layout, spacing, colours and copy mirror the Tauri
//! windows — `src/components/Capsule.tsx` (classic pill), `src/pages/QaPanel.tsx`
//! (shadcn chat card) and `src/pages/SelectionPolishPreview.tsx`.
//!
//! The chat panel renders in the shadcn zinc palette, which maps onto the theme
//! tokens: white [`theme::SURFACE`], [`theme::INK`] foreground, [`theme::SURFACE_2`]
//! muted fill, [`theme::INK_3`] muted text, [`theme::LINE`] border.

use eframe::egui;

use super::{icons, layout, siri_gl, theme};
use openless_linux_egui::{
    fmt_l10n, tr_l10n, CapsulePopupState, Lang, LessComputerPopupState, PopupChatMessage,
    PreviewPopupState, QaPopupState,
};

/// Result of rendering the selection-polish preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewAction {
    None,
    /// ✕ / 取消 → the host sends `CancelPreview`.
    Cancel,
    /// ✓ 确认并替换 → the host sends `ConfirmPreview` with the edited text.
    Confirm(String),
}

/// Result of rendering the selection-ask panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QaAction {
    None,
    /// ✕ → the host sends `DismissQa`.
    Dismiss,
    /// Enter / 发送 → the host sends `SubmitQa`.
    Submit(String),
    /// 麦克风按钮 → the host sends `ToggleQaRecording`.
    ToggleRecording,
    /// 图钉 → the host sends `SetPinned`（固定后不再自动收起）。
    SetPinned(bool),
    /// 「编辑指令」勾选框 → the host sends `SetEditInstructionMode`.
    SetEditInstructionMode(bool),
    /// 「预览并确认插入」→ the host sends `ApplyEdit`.
    ApplyEdit,
    /// 「保留上一版本」→ the host sends `RevertEdit`.
    RevertEdit,
}

/// Result of rendering the dictation capsule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapsuleAction {
    None,
    /// ✕ → the host cancels the dictation.
    Cancel,
    /// ✓ → the host stops the dictation and inserts.
    Confirm,
}

/// Tauri preview window padding (`padding: 18`).
const PREVIEW_PADDING: f32 = 18.0;
/// The QA card uses `--card-spacing` (14px) for its header/footer gutters.
const CARD_SPACING: f32 = 14.0;
/// Composer row height (Tauri `InputGroup`).
const COMPOSER_HEIGHT: f32 = 40.0;
/// Classic capsule pill metrics (Tauri `CLASSIC_PILL_METRICS`).
const PILL_WIDTH: f32 = 176.0;
const PILL_HEIGHT: f32 = 42.0;
/// Round icon buttons in the capsule / composer.
const ROUND_BUTTON: f32 = 28.0;
/// Tauri `getCapsuleHostMetrics(.., 'classic').bottomInset`。
const CAPSULE_BOTTOM_INSET: f32 = 16.0;
/// Typeless 胶囊（Tauri `CapsuleStyles.css`：176×64、46×46 圆形按钮、11 根波形）。
const TYPELESS_WIDTH: f32 = 176.0;
const TYPELESS_HEIGHT: f32 = 64.0;
const TYPELESS_BUTTON: f32 = 46.0;
/// `.ol-typeless-*` 调色板。
const TYPELESS_BG: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
const TYPELESS_BORDER: egui::Color32 = egui::Color32::from_rgb(0x52, 0x52, 0x5b);
const TYPELESS_INK: egui::Color32 = egui::Color32::from_rgb(0xfa, 0xfa, 0xfa);
const TYPELESS_BUTTON_BG: egui::Color32 = egui::Color32::from_rgb(0x3f, 0x3f, 0x46);
/// 徽章与药丸之间的间距（Tauri `badgeGap`）。
const CAPSULE_BADGE_GAP: f32 = 8.0;

// ── 选区润色预览 ────────────────────────────────────────────────────────────

/// 选区润色预览：标题 + 副标题 + ✕、可编辑结果框、原文摘要、取消 / 确认并替换。
pub fn selection_preview(
    ctx: &egui::Context,
    state: &mut PreviewPopupState,
    first_frame: bool,
    lang: Lang,
) -> PreviewAction {
    let mut action = PreviewAction::None;
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .corner_radius(egui::CornerRadius::same(14))
                .stroke(egui::Stroke::new(0.5, theme::LINE))
                .inner_margin(egui::Margin::same(PREVIEW_PADDING as i8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "selection.polish_preview.title"))
                            .size(16.0)
                            .strong()
                            .color(theme::INK),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "selection.polish_preview.subtitle"))
                            .size(12.0)
                            .color(theme::INK_4),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if icon_button(ui, icons::IconName::Close, theme::INK_3).clicked() {
                        action = PreviewAction::Cancel;
                    }
                });
            });
            ui.add_space(12.0);

            // 可编辑结果框：撑满剩余高度（Tauri `flex: 1; min-height: 150`）。
            let footer_height = 48.0;
            let source_height = if state.source.is_empty() { 0.0 } else { 50.0 };
            let editor_height = (ui.available_height() - footer_height - source_height).max(150.0);
            let width = ui.available_width();
            egui::Frame::new()
                .fill(theme::CONTENT_BG)
                .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                .corner_radius(egui::CornerRadius::same(9))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(width - 24.0, editor_height - 24.0));
                    let response = ui.add_sized(
                        egui::vec2(width - 24.0, editor_height - 24.0),
                        egui::TextEdit::multiline(&mut state.text)
                            .frame(false)
                            .text_color(theme::INK)
                            .font(egui::FontId::proportional(14.0)),
                    );
                    if first_frame {
                        response.request_focus();
                    }
                });

            if !state.source.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{}{}",
                        tr_l10n(lang, "selection.polish_preview.source_prefix"),
                        truncate(&state.source, 200)
                    ))
                    .size(11.0)
                    .color(theme::INK_4),
                );
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let confirm = tr_l10n(lang, "selection.polish_preview.confirm_replace");
                let confirm_width = layout::text_width(ui, confirm, 13.0) + 46.0;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(confirm_width, 34.0), egui::Sense::click());
                let fill = if response.hovered() {
                    theme::BLUE.gamma_multiply(0.9)
                } else {
                    theme::BLUE
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(7), fill);
                icon_text(
                    ui,
                    rect,
                    Some(icons::IconName::Check),
                    confirm,
                    theme::SURFACE,
                );
                if response.clicked() {
                    action = PreviewAction::Confirm(state.text.clone());
                }
                ui.add_space(8.0);
                let cancel = tr_l10n(lang, "selection.polish_preview.cancel");
                let cancel_width = layout::text_width(ui, cancel, 13.0) + 30.0;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(cancel_width, 34.0), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(7),
                    if response.hovered() {
                        theme::SURFACE_2
                    } else {
                        theme::SURFACE
                    },
                );
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(7),
                    egui::Stroke::new(0.5, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );
                icon_text(ui, rect, None, cancel, theme::INK_2);
                if response.clicked() {
                    action = PreviewAction::Cancel;
                }
            });
        });
    action
}

// ── 划词追问 ────────────────────────────────────────────────────────────────

/// 划词追问面板：卡片头（标题 + 副行 + ✕）、消息流（空状态 / 对话 / 思考中 /
/// 出错）、底部输入组（选区条 + 输入框 + 麦克风 + 发送）。
pub fn selection_ask(
    ctx: &egui::Context,
    state: &QaPopupState,
    composer: &mut String,
    lang: Lang,
    avatar: Option<&egui::TextureHandle>,
) -> QaAction {
    let mut action = QaAction::None;
    let phase = state.phase.to_ascii_lowercase();
    let recording = phase == "recording";
    // Tauri 在 loading / thinking / awaiting_approval 以及流式增量期间都保持
    // 「思考中」：转圈不停，但已经有流式正文时不再重复显示思考行。
    let thinking = matches!(
        phase.as_str(),
        "loading" | "thinking" | "awaiting_approval" | "answerdelta" | "answer"
    );
    let thinking_row = thinking && state.streaming_answer.is_empty();
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .corner_radius(egui::CornerRadius::same(14))
                .stroke(egui::Stroke::new(0.5, theme::LINE)),
        )
        .show(ctx, |ui| {
            // 首帧只编译不绘制地把三个程序编译好（进程内只排一次），
            // 免得录音/思考的第一帧才发现要编译——那是按热键后「慢一拍」的来源。
            siri_gl::warm_up(ui);
            // ── CardHeader：整条可拖，✕ 在右 ─────────────────────────────
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(CARD_SPACING as i8, 12))
                .show(ui, |ui| {
                    let row = ui
                        .horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(tr_l10n(lang, "qa.title"))
                                        .size(16.0)
                                        .strong()
                                        .color(theme::INK),
                                );
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(tr_l10n(lang, "qa.header_hint"))
                                        .size(12.0)
                                        .color(theme::INK_4),
                                );
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if icon_button(ui, icons::IconName::Close, theme::INK_3)
                                    .on_hover_text(tr_l10n(lang, "qa.close_tooltip"))
                                    .clicked()
                                {
                                    action = QaAction::Dismiss;
                                }
                                // 图钉：固定后宿主不再自动收起（Tauri 的
                                // qa.pinTooltip / qa.unpinTooltip）。
                                let pin_color = if state.pinned {
                                    theme::BLUE
                                } else {
                                    theme::INK_4
                                };
                                let pin_tooltip = if state.pinned {
                                    tr_l10n(lang, "qa.unpin_tooltip")
                                } else {
                                    tr_l10n(lang, "qa.pin_tooltip")
                                };
                                if icon_button(ui, icons::IconName::Pin, pin_color)
                                    .on_hover_text(pin_tooltip)
                                    .clicked()
                                {
                                    action = QaAction::SetPinned(!state.pinned);
                                }
                            });
                        })
                        .response
                        .interact(egui::Sense::drag());
                    if row.drag_started() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                });
            hairline(ui, theme::LINE_SOFT);

            // ── CardContent ──────────────────────────────────────────────
            // 底部高度必须把新增的「编辑指令」勾选框与「保留上一版本 / 预览并确认
            // 插入」按钮算进去，否则线程区会把它们挤出窗口底部。
            let edit_block = if state.edit_apply_available && phase == "idle" {
                (if state.edit_revert_available {
                    38.0
                } else {
                    0.0
                }) + 38.0
            } else {
                0.0
            };
            let footer_height = CARD_SPACING * 2.0 + COMPOSER_HEIGHT + 12.0 + 22.0 + edit_block;
            let content_height = (ui.available_height() - footer_height).max(80.0);
            let has_thread = !state.messages.is_empty()
                || !state.streaming_answer.is_empty()
                || thinking
                || state.error.is_some();
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(CARD_SPACING as i8, 0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if !has_thread {
                        empty_state(ui, lang, content_height);
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("openless-qa-thread")
                            .max_height(content_height)
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                let width = ui.available_width();
                                for message in &state.messages {
                                    message_row(ui, message, width, lang, avatar);
                                    ui.add_space(10.0);
                                }
                                if !state.streaming_answer.is_empty() {
                                    assistant_row(ui, |ui| {
                                        render_markdown(ui, &state.streaming_answer)
                                    });
                                    ui.add_space(10.0);
                                }
                                if thinking_row {
                                    assistant_row(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(tr_l10n(lang, "qa.thinking"))
                                                .size(12.0)
                                                .color(theme::INK_3),
                                        );
                                    });
                                    ui.add_space(10.0);
                                }
                                if let Some(error) = &state.error {
                                    destructive_bubble(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(error).size(14.0).color(theme::ERR),
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(tr_l10n(
                                                lang,
                                                "qa.error_retry_hint",
                                            ))
                                            .size(11.5)
                                            .color(theme::ERR.gamma_multiply(0.7)),
                                        );
                                    });
                                }
                            });
                    }
                });

            // ── CardFooter：选区条 + 输入组 ──────────────────────────────
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(CARD_SPACING as i8, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // 编辑结果：底部出现「保留上一版本 / 预览并确认插入」
                    // （只在轮到 idle 且预览可用时）。
                    if state.edit_apply_available && phase == "idle" {
                        if state.edit_revert_available {
                            if wide_button(ui, tr_l10n(lang, "qa.edit_revert_previous")) {
                                action = QaAction::RevertEdit;
                            }
                            ui.add_space(6.0);
                        }
                        if wide_button_primary(
                            ui,
                            tr_l10n(lang, "qa.edit_apply_replace"),
                            icons::IconName::Check,
                        ) {
                            action = QaAction::ApplyEdit;
                        }
                        ui.add_space(8.0);
                    }
                    if recording {
                        if let Some(selection) = &state.selection_preview {
                            selection_chip(ui, selection, lang);
                            ui.add_space(8.0);
                        }
                    }
                    // 「编辑指令」勾选框（Tauri Composer 左下角，busy 时禁用）。
                    let busy = thinking || recording;
                    let checkbox_label = tr_l10n(lang, "qa.edit_instruction_mode");
                    let (checkbox_rect, checkbox_response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 20.0),
                        if busy {
                            egui::Sense::hover()
                        } else {
                            egui::Sense::click()
                        },
                    );
                    let box_rect = egui::Rect::from_center_size(
                        egui::pos2(checkbox_rect.left() + 7.0, checkbox_rect.center().y),
                        egui::vec2(14.0, 14.0),
                    );
                    let checked = state.edit_instruction_mode;
                    ui.painter().rect_filled(
                        box_rect,
                        egui::CornerRadius::same(3),
                        if checked { theme::INK } else { theme::SURFACE },
                    );
                    ui.painter().rect_stroke(
                        box_rect,
                        egui::CornerRadius::same(3),
                        egui::Stroke::new(0.8, theme::LINE_STRONG),
                        egui::StrokeKind::Inside,
                    );
                    if checked {
                        icons::draw_icon(
                            ui,
                            box_rect.center(),
                            icons::IconName::Check,
                            theme::SURFACE,
                        );
                    }
                    ui.painter().text(
                        egui::pos2(box_rect.right() + 6.0, checkbox_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        checkbox_label,
                        egui::FontId::proportional(11.5),
                        if busy { theme::INK_4 } else { theme::INK_3 },
                    );
                    if checkbox_response.clicked() && !busy {
                        action = QaAction::SetEditInstructionMode(!checked);
                    }
                    ui.add_space(2.0);
                    let width = ui.available_width();
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(width, COMPOSER_HEIGHT),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(12), theme::SURFACE);
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(12),
                        egui::Stroke::new(0.5, theme::LINE_STRONG),
                        egui::StrokeKind::Inside,
                    );
                    // olchat-ring：录音红光 / 思考黑光绕输入组转圈。GPU 路径用
                    // 圆角矩形 SDF 片元着色器（时间/尺寸/圆角/颜色 4 组 uniform），
                    // 驱动拒绝着色器时回落到 CPU 采样版。
                    if recording || thinking {
                        let tint = if recording {
                            color_to_f32(theme::ERR)
                        } else {
                            color_to_f32(theme::INK)
                        };
                        let drive = siri_gl::SiriDrive {
                            level: 0.0,
                            resolved: if recording { 1.0 } else { 0.0 },
                            // 思考态转得更快，和 Tauri 的 state→speed 语义一致。
                            speed: if recording { 1.0 } else { 1.45 },
                            warming: false,
                        };
                        let dt = ui.input(|input| input.stable_dt);
                        let clock = siri_gl::tick(ui.ctx(), "qa-composer-ring", drive, dt);
                        let glow = siri_gl::SiriGlow::ring(
                            clock.time,
                            12.0,
                            if recording { 2.0 } else { 1.6 },
                            if recording { 1.5 } else { 2.1 },
                        )
                        .with_tint(tint);
                        if !siri_gl::paint(ui, rect.expand(3.0), glow) {
                            spinner_ring(ui, rect, if recording { theme::ERR } else { theme::INK });
                        }
                    }
                    let inner = rect.shrink2(egui::vec2(10.0, 6.0));
                    let mic_rect = egui::Rect::from_center_size(
                        egui::pos2(inner.right() - ROUND_BUTTON / 2.0, rect.center().y),
                        egui::vec2(ROUND_BUTTON, ROUND_BUTTON),
                    );
                    let send_rect = egui::Rect::from_center_size(
                        egui::pos2(inner.right() - ROUND_BUTTON * 1.5 - 4.0, rect.center().y),
                        egui::vec2(ROUND_BUTTON, ROUND_BUTTON),
                    );
                    let input_rect = egui::Rect::from_min_max(
                        inner.min,
                        egui::pos2(send_rect.left() - 6.0, inner.bottom()),
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("openless-qa-composer")
                            .max_rect(input_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    child.set_clip_rect(child.clip_rect().intersect(input_rect));
                    let response = child.add(
                        egui::TextEdit::singleline(composer)
                            .id(egui::Id::new("openless-qa-composer-input"))
                            .frame(false)
                            .text_color(theme::INK)
                            .font(egui::FontId::proportional(13.5))
                            .hint_text(tr_l10n(lang, "qa.composer_placeholder"))
                            .desired_width(input_rect.width()),
                    );
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        && !composer.trim().is_empty()
                    {
                        action = QaAction::Submit(std::mem::take(composer));
                    }
                    let mic = ui.interact(
                        mic_rect,
                        ui.id().with("openless-qa-mic"),
                        egui::Sense::click(),
                    );
                    if recording {
                        ui.painter().circle_filled(
                            mic_rect.center(),
                            ROUND_BUTTON / 2.0,
                            theme::ERR,
                        );
                    } else if mic.hovered() {
                        ui.painter().circle_filled(
                            mic_rect.center(),
                            ROUND_BUTTON / 2.0,
                            theme::SURFACE_2,
                        );
                    }
                    icons::draw_icon(
                        ui,
                        mic_rect.center(),
                        if recording {
                            icons::IconName::Stop
                        } else {
                            icons::IconName::Mic
                        },
                        if recording {
                            theme::SURFACE
                        } else {
                            theme::INK_2
                        },
                    );
                    if mic.clicked() && !thinking {
                        action = QaAction::ToggleRecording;
                    }
                    let can_send = !composer.trim().is_empty() && !thinking;
                    let send = ui.interact(
                        send_rect,
                        ui.id().with("openless-qa-send"),
                        egui::Sense::click(),
                    );
                    ui.painter().circle_filled(
                        send_rect.center(),
                        ROUND_BUTTON / 2.0,
                        if can_send {
                            theme::INK
                        } else {
                            theme::SURFACE_2
                        },
                    );
                    icons::draw_icon(
                        ui,
                        send_rect.center(),
                        icons::IconName::Send,
                        if can_send {
                            theme::SURFACE
                        } else {
                            theme::INK_4
                        },
                    );
                    if send.clicked() && can_send {
                        action = QaAction::Submit(std::mem::take(composer));
                    }
                });
        });
    action
}

/// 空状态：居中图标 + 标题 + 说明（Tauri `<Empty>`）。
fn empty_state(ui: &mut egui::Ui, lang: Lang, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let center = rect.center();
    icons::draw_icon(
        ui,
        egui::pos2(center.x, center.y - 48.0),
        icons::IconName::Chat,
        theme::INK_4,
    );
    ui.painter().text(
        egui::pos2(center.x, center.y - 14.0),
        egui::Align2::CENTER_CENTER,
        tr_l10n(lang, "qa.empty_title"),
        egui::FontId::proportional(14.0),
        theme::INK,
    );
    let galley = layout::text_galley(
        ui,
        tr_l10n(lang, "qa.empty_desc"),
        theme::INK_4,
        12.0,
        (rect.width() - 48.0).min(300.0),
        4,
    );
    ui.painter().galley(
        egui::pos2(center.x - galley.rect.width() / 2.0, center.y + 6.0),
        galley,
        theme::INK_4,
    );
}

/// 一条对话消息：用户右侧深色气泡（带选区引用块）+ 头像；助手左侧头像 + Markdown。
fn message_row(
    ui: &mut egui::Ui,
    message: &PopupChatMessage,
    width: f32,
    lang: Lang,
    avatar: Option<&egui::TextureHandle>,
) {
    if message.role.eq_ignore_ascii_case("user") {
        let selection = message
            .selection_text
            .as_deref()
            .map(|text| truncate(text, 120))
            .filter(|text| !text.is_empty() && *text != message.content);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            user_avatar(ui, avatar);
            ui.add_space(8.0);
            let max_width = (width - 56.0).max(120.0) * 0.8;
            ui.allocate_ui_with_layout(
                egui::vec2(max_width, 0.0),
                egui::Layout::top_down(egui::Align::Max),
                |ui| {
                    if let Some(selection) = selection {
                        bubble(ui, theme::SURFACE_2, theme::INK_3, |ui| {
                            ui.label(
                                egui::RichText::new(format!("“{selection}”"))
                                    .size(12.0)
                                    .italics()
                                    .color(theme::INK_3),
                            );
                        });
                        ui.add_space(4.0);
                    }
                    bubble(ui, theme::INK, theme::SURFACE, |ui| {
                        ui.label(
                            egui::RichText::new(&message.content)
                                .size(14.0)
                                .color(theme::SURFACE),
                        );
                    });
                },
            );
        });
        let _ = lang;
        return;
    }
    assistant_row(ui, |ui| render_markdown(ui, &message.content));
}

/// 助手行：深色思考头像 + 内容（内容由调用方渲染在头像右侧）。
fn assistant_row(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ai_avatar(ui);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 4.0).max(80.0));
            contents(ui);
        });
    });
}

/// 一个聊天气泡：`rounded-3xl` = 24px，padding 12/10。
fn bubble(
    ui: &mut egui::Ui,
    fill: egui::Color32,
    ink: egui::Color32,
    contents: impl FnOnce(&mut egui::Ui),
) {
    let _ = ink;
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(18))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, contents);
}

/// 出错气泡（Tauri `variant="destructive"`）：红底红字。
fn destructive_bubble(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::DANGER_SOFT)
        .corner_radius(egui::CornerRadius::same(18))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, contents);
}

/// 用户头像：已登录 GitHub 时画真实头像（`github.com/{login}.png`，圆形裁切），
/// 未登录 / 取图失败回落 GitHub 图标（Tauri `UserAvatar`）。
fn user_avatar(ui: &mut egui::Ui, avatar: Option<&egui::TextureHandle>) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ROUND_BUTTON + 4.0, ROUND_BUTTON + 4.0),
        egui::Sense::hover(),
    );
    let radius = (ROUND_BUTTON + 4.0) / 2.0;
    match avatar {
        Some(texture) => {
            textured_circle(ui, rect.center(), radius, texture);
        }
        None => {
            ui.painter()
                .circle_filled(rect.center(), radius, theme::SURFACE_2);
            icons::draw_icon(ui, rect.center(), icons::IconName::Github, theme::INK_2);
        }
    }
}

/// 把一张方形贴图画成圆形：以中心为扇形顶点、UV 按圆周比例展开。
fn textured_circle(ui: &egui::Ui, center: egui::Pos2, radius: f32, texture: &egui::TextureHandle) {
    const SEGMENTS: usize = 48;
    let mut mesh = egui::Mesh::with_texture(texture.id());
    mesh.vertices.push(egui::epaint::Vertex {
        pos: center,
        uv: egui::pos2(0.5, 0.5),
        color: egui::Color32::WHITE,
    });
    for index in 0..=SEGMENTS {
        let angle = index as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let pos = center + egui::vec2(angle.cos(), angle.sin()) * radius;
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::pos2(0.5 + angle.cos() * 0.5, 0.5 + angle.sin() * 0.5),
            color: egui::Color32::WHITE,
        });
        if index > 0 {
            mesh.indices
                .extend_from_slice(&[0, index as u32, index as u32 + 1]);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// 输入区上方的整宽次级按钮（Tauri `Button variant="outline"`）。
fn wide_button(ui: &mut egui::Ui, label: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let fill = if response.hovered() {
        theme::SURFACE_2
    } else {
        theme::SURFACE
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(0.5, theme::LINE),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        theme::INK_2,
    );
    response.clicked()
}

/// 输入区上方的整宽主按钮（Tauri `Button`，带 ✓）。
fn wide_button_primary(ui: &mut egui::Ui, label: &str, icon: icons::IconName) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let fill = if response.hovered() {
        theme::INK_2
    } else {
        theme::INK
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    icon_text(ui, rect, Some(icon), label, theme::SURFACE);
    response.clicked()
}

/// 助手头像：深色圆底 + 旋转的思考光点（Tauri 的 `OrbAvatar`）。
fn ai_avatar(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ROUND_BUTTON + 4.0, ROUND_BUTTON + 4.0),
        egui::Sense::hover(),
    );
    let radius = (ROUND_BUTTON + 4.0) / 2.0;
    ui.painter()
        .circle_filled(rect.center(), radius, theme::INK);
    let time = ui.input(|input| input.time) as f32;
    let mut previous: Option<egui::Pos2> = None;
    for step in 0..14 {
        let angle = time * 1.6 + step as f32 * std::f32::consts::TAU / 14.0;
        let alpha = (30.0 + 225.0 * (step as f32 / 13.0)).min(255.0) as u8;
        let point = rect.center() + egui::vec2(angle.cos(), angle.sin()) * (radius * 0.42);
        if let Some(previous) = previous {
            ui.painter().line_segment(
                [previous, point],
                egui::Stroke::new(
                    2.0,
                    egui::Color32::from_rgba_unmultiplied(150, 185, 255, alpha),
                ),
            );
        }
        previous = Some(point);
    }
    ui.painter().circle_filled(
        rect.center(),
        2.6,
        egui::Color32::from_rgba_unmultiplied(150, 185, 255, 235),
    );
}

/// 录音时的选区上下文条（Tauri `SelectionChip`）。
fn selection_chip(ui: &mut egui::Ui, text: &str, lang: Lang) {
    egui::Frame::new()
        .fill(theme::SURFACE_2)
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(tr_l10n(lang, "qa.selection_preview"))
                        .size(11.5)
                        .color(theme::INK_3),
                );
                ui.label(
                    egui::RichText::new(truncate(text, 60))
                        .size(11.5)
                        .color(theme::INK_2),
                );
            });
        });
}

/// 输入组外圈：Tauri 用 conic-gradient 假 border，这里按圆角矩形周长采样做出
/// 同样的「转圈高光」（egui 没有锥形渐变）。
fn spinner_ring(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let time = ui.input(|input| input.time) as f32;
    let points = rounded_rect_points(rect.expand(2.0), 10.0, 64);
    let head = (time * 1.1).rem_euclid(1.0);
    for (index, window) in points.windows(2).enumerate() {
        let phase = index as f32 / points.len() as f32;
        let distance = (phase - head).rem_euclid(1.0);
        let intensity = if distance < 0.22 {
            1.0 - distance / 0.22
        } else {
            0.0
        };
        let alpha = (38.0 + intensity * 217.0).min(255.0) as u8;
        ui.painter().line_segment(
            [window[0], window[1]],
            egui::Stroke::new(
                2.0,
                egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
            ),
        );
    }
}

/// 圆角矩形的周长采样点（顺时针，从右下角弧开始）。
fn rounded_rect_points(rect: egui::Rect, radius: f32, segments: usize) -> Vec<egui::Pos2> {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (rect.right() - radius, rect.bottom() - radius, 0.0_f32),
        (
            rect.left() + radius,
            rect.bottom() - radius,
            std::f32::consts::FRAC_PI_2,
        ),
        (
            rect.left() + radius,
            rect.top() + radius,
            std::f32::consts::PI,
        ),
        (
            rect.right() - radius,
            rect.top() + radius,
            3.0 * std::f32::consts::FRAC_PI_2,
        ),
    ];
    let per_corner = (segments / 4).max(2);
    let mut points = Vec::with_capacity(per_corner * 4);
    for (center_x, center_y, start) in corners {
        for step in 0..=per_corner {
            let angle = start + std::f32::consts::FRAC_PI_2 * (step as f32 / per_corner as f32);
            points.push(egui::pos2(
                center_x + radius * angle.cos(),
                center_y + radius * angle.sin(),
            ));
        }
    }
    points
}

/// egui color → the shader's `uTint` (linear 0..1, gamma-space value is fine
/// here because the glow is additive on a translucent window).
fn color_to_f32(color: egui::Color32) -> [f32; 3] {
    [
        f32::from(color.r()) / 255.0,
        f32::from(color.g()) / 255.0,
        f32::from(color.b()) / 255.0,
    ]
}

fn hairline(ui: &mut egui::Ui, color: egui::Color32) {
    let rect = ui
        .allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover())
        .0;
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(0.5, color),
    );
}

// ── 录音胶囊 ────────────────────────────────────────────────────────────────

/// 药丸上方的「正在翻译」徽章（Tauri `ClassicCapsule` 的 `capsule.translating`）：
/// 蓝点 + 蓝字、圆角胶囊、`--ol-capsule-badge-bg` 底、`--ol-capsule-badge-border` 边。
fn translating_badge(ui: &mut egui::Ui, pill: egui::Rect, lang: Lang) {
    let label = tr_l10n(lang, "capsule.translating");
    let text_width = layout::text_width(ui, label, 10.5);
    let width = text_width + 5.0 + 5.0 + 20.0;
    let height = 19.0;
    let rect = egui::Rect::from_center_size(
        egui::pos2(
            pill.center().x,
            pill.top() - CAPSULE_BADGE_GAP - height / 2.0,
        ),
        egui::vec2(width, height),
    );
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        egui::CornerRadius::same((height / 2.0) as u8),
        theme::CAPSULE_BADGE_BG,
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same((height / 2.0) as u8),
        egui::Stroke::new(0.5, theme::CAPSULE_BADGE_BORDER),
        egui::StrokeKind::Inside,
    );
    let dot = egui::pos2(rect.left() + 10.0, rect.center().y);
    painter.circle_filled(dot, 2.5, theme::BLUE);
    painter.text(
        egui::pos2(dot.x + 5.0 + 2.5, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(10.5),
        theme::BLUE,
    );
}

/// 录音胶囊：经典药丸（Tauri `ClassicPill`）—— 左 ✕、中间状态、右 ✓。
pub fn dictation_capsule(
    ctx: &egui::Context,
    state: &CapsulePopupState,
    lang: Lang,
) -> CapsuleAction {
    let mut action = CapsuleAction::None;
    let phase = state.phase.to_ascii_lowercase();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            // 胶囊进程的首帧预热（同 QA 面板；录音环与 Siri 波都是 GPU 路径）。
            siri_gl::warm_up(ui);
            // Tauri `capsuleStyle`：siri = 流光药丸（GPU 波/环），classic = 经典药丸 +
            // 五根音量条，typeless = 176×64 深色胶囊 + 11 根波形。
            let style = state.style.as_str();
            let typeless = style == "typeless";
            // 未知/空值走 siri（默认样式），只有显式选择 classic 才关掉 GPU 光效。
            let use_gpu = !typeless && style != "classic";
            let (pill_width, pill_height, button, bar_count) = if typeless {
                (TYPELESS_WIDTH, TYPELESS_HEIGHT, TYPELESS_BUTTON, 11)
            } else {
                (PILL_WIDTH, PILL_HEIGHT, ROUND_BUTTON, 5)
            };
            let (pill_bg, pill_border, pill_ink) = if typeless {
                (TYPELESS_BG, TYPELESS_BORDER, TYPELESS_INK)
            } else {
                (theme::SURFACE, theme::LINE, theme::INK_2)
            };
            // Tauri 经典药丸宿主：窗口高 100，药丸水平居中、距底 16，徽章再上移 8。
            let available = ui.available_rect_before_wrap();
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    available.center().x - pill_width / 2.0,
                    available.bottom() - CAPSULE_BOTTOM_INSET - pill_height,
                ),
                egui::vec2(pill_width, pill_height),
            );
            let _ = ui.allocate_rect(rect, egui::Sense::hover());
            if state.translation_active {
                translating_badge(ui, rect, lang);
            }
            // Tauri 的经典药丸只有「1px 中性描边」+「随音量轻微放大」两件事
            // （Capsule.tsx 的 ClassicPill：border 1px var(--ol-capsule-pill-border)、
            // transform scale(1 + ambient * 0.018)），**没有**任何外圈扫光/描边颜色变化。
            // 所以这里不再把录音相位画成红圈（那是本仓自己加的，用户报「有一个红边」）；
            // 运动感只保留药丸中心的音量波形。
            let ambient = if phase == "recording" {
                state.audio_level.unwrap_or(0.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let pill =
                egui::Rect::from_center_size(rect.center(), rect.size() * (1.0 + ambient * 0.018));
            ui.painter().rect_filled(
                pill,
                egui::CornerRadius::same((pill_height / 2.0) as u8),
                pill_bg,
            );
            ui.painter().rect_stroke(
                pill,
                egui::CornerRadius::same((pill_height / 2.0) as u8),
                egui::Stroke::new(1.0, pill_border),
                egui::StrokeKind::Inside,
            );
            let inset = if typeless { 7.0 } else { 8.0 };
            let cancel_rect = egui::Rect::from_center_size(
                egui::pos2(rect.left() + inset + button / 2.0, rect.center().y),
                egui::vec2(button, button),
            );
            let cancel = ui.interact(
                cancel_rect,
                ui.id().with("openless-capsule-cancel"),
                egui::Sense::click(),
            );
            let cancel_fill = if typeless {
                TYPELESS_BUTTON_BG
            } else {
                theme::SURFACE_2
            };
            round_button(
                ui,
                cancel_rect,
                icons::IconName::Close,
                cancel.hovered(),
                cancel_fill,
                pill_ink,
            );
            if cancel.clicked() {
                action = CapsuleAction::Cancel;
            }
            let confirm_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - inset - button / 2.0, rect.center().y),
                egui::vec2(button, button),
            );
            let confirm = ui.interact(
                confirm_rect,
                ui.id().with("openless-capsule-confirm"),
                egui::Sense::click(),
            );
            // Tauri `.ol-typeless-confirm-bg: #fafafa` + 深色勾。
            let (confirm_fill, confirm_ink) = if typeless {
                (TYPELESS_INK, TYPELESS_BG)
            } else {
                (theme::SURFACE_2, theme::INK_2)
            };
            round_button(
                ui,
                confirm_rect,
                icons::IconName::Check,
                confirm.hovered(),
                confirm_fill,
                confirm_ink,
            );
            if confirm.clicked() {
                action = CapsuleAction::Confirm;
            }
            let center = egui::Rect::from_min_max(
                egui::pos2(cancel_rect.right() + 4.0, rect.top() + 4.0),
                egui::pos2(confirm_rect.left() - 4.0, rect.bottom() - 4.0),
            );
            let processing = matches!(
                phase.as_str(),
                "starting" | "transcribing" | "polishing" | "inserting"
            );
            if phase == "recording" {
                // Siri 声波（Tauri `SiriGL` wave 模式）：GPU 路径用真实电平驱动，
                // 失败时回落到经典五根音量条。
                let drive = siri_gl::SiriDrive {
                    level: state.audio_level.unwrap_or_default(),
                    resolved: 1.0,
                    speed: 1.0,
                    warming: state.audio_level.is_none(),
                };
                let dt = ui.input(|input| input.stable_dt);
                let clock = siri_gl::tick(ui.ctx(), "capsule-siri-wave", drive, dt);
                let glow = siri_gl::SiriGlow::wave(clock.time, clock.level, clock.resolved);
                if !use_gpu || !siri_gl::paint(ui, center, glow) {
                    audio_bars(
                        ui,
                        center,
                        state.audio_level.unwrap_or_default(),
                        bar_count,
                        pill_ink,
                    );
                }
            } else if processing {
                // 思考中：Siri 流体圆点（orb），从 wave 收拢的光点化开成环。
                let drive = siri_gl::SiriDrive {
                    level: 0.0,
                    resolved: 0.0,
                    speed: 1.3,
                    warming: false,
                };
                let dt = ui.input(|input| input.stable_dt);
                let clock = siri_gl::tick(ui.ctx(), "capsule-siri-orb", drive, dt);
                // 0.3s 全聚圆心接住 wave 收拢的光点，再缓缓散开成环。
                let gather = (1.0 - (clock.time / 0.9).clamp(0.0, 1.0)).clamp(0.0, 1.0);
                let glow = siri_gl::SiriGlow::orb(clock.time, gather);
                if !use_gpu || !siri_gl::paint(ui, center, glow) {
                    ui.painter().text(
                        center.center(),
                        egui::Align2::CENTER_CENTER,
                        tr_l10n(lang, "capsule.thinking"),
                        egui::FontId::proportional(17.0),
                        theme::INK,
                    );
                }
            } else if state.text.is_empty() {
                let label = if processing {
                    tr_l10n(lang, "capsule.thinking")
                } else if phase == "cancelled" {
                    tr_l10n(lang, "capsule.cancelled")
                } else if phase == "failed" {
                    tr_l10n(lang, "capsule.error")
                } else {
                    tr_l10n(lang, "capsule.thinking")
                };
                let size = if processing { 17.0 } else { 11.0 };
                ui.painter().text(
                    center.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(size),
                    if phase == "failed" {
                        theme::ERR
                    } else if typeless {
                        TYPELESS_INK
                    } else {
                        theme::INK
                    },
                );
            } else {
                // 11px/500 单行居中，超长省略（Tauri `getCapsuleMessageLayout`）。
                let galley =
                    layout::text_galley(ui, &state.text, pill_ink, 11.0, center.width(), 1);
                ui.painter().galley(
                    egui::pos2(
                        center.center().x - galley.rect.width() / 2.0,
                        center.center().y - galley.rect.height() / 2.0,
                    ),
                    galley,
                    pill_ink,
                );
            }
        });
    action
}

/// 28×28 圆形按钮（Tauri `CircleButton`）。
fn round_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    icon: icons::IconName,
    hovered: bool,
    fill: egui::Color32,
    ink: egui::Color32,
) {
    ui.painter().circle_filled(
        rect.center(),
        rect.width() / 2.0,
        if hovered {
            fill.gamma_multiply(1.06)
        } else {
            fill
        },
    );
    ui.painter().circle_stroke(
        rect.center(),
        rect.width() / 2.0,
        egui::Stroke::new(0.8, theme::LINE),
    );
    icons::draw_icon(ui, rect.center(), icon, ink);
}

/// 音量条：Tauri `AudioBars`（5 根 3px 竖条，包络 0.55/0.85/1/0.85/0.55，
/// 过静音门限后按 0.42 次幂提亮）。
fn audio_bars(ui: &egui::Ui, rect: egui::Rect, level: f32, bar_count: usize, ink: egui::Color32) {
    const CLASSIC_ENVELOPE: [f32; 5] = [0.55, 0.85, 1.0, 0.85, 0.55];
    // Tauri `WAVE_ENVELOPE`（Typeless 的 11 根）。
    const TYPELESS_ENVELOPE: [f32; 11] = [
        0.28, 0.44, 0.63, 0.82, 0.96, 1.0, 0.96, 0.82, 0.63, 0.44, 0.28,
    ];
    let envelope: &[f32] = if bar_count > CLASSIC_ENVELOPE.len() {
        &TYPELESS_ENVELOPE
    } else {
        &CLASSIC_ENVELOPE
    };
    const BASE: f32 = 2.0;
    let max = if bar_count > CLASSIC_ENVELOPE.len() {
        26.0
    } else {
        24.0
    };
    let voice = level.clamp(0.0, 1.0);
    let gated = ((voice - 0.012) / (0.34 - 0.012)).clamp(0.0, 1.0);
    let eased = gated * gated * (3.0 - 2.0 * gated);
    let visual = eased.powf(0.42);
    let bar_width = 3.0;
    let gap = 3.0;
    let total = envelope.len() as f32 * bar_width + (envelope.len() - 1) as f32 * gap;
    let mut x = rect.center().x - total / 2.0;
    for envelope in envelope {
        let height = BASE + (max - BASE) * visual * envelope;
        ui.painter().rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(x + bar_width / 2.0, rect.center().y),
                egui::vec2(bar_width, height),
            ),
            egui::CornerRadius::same(2),
            ink,
        );
        x += bar_width + gap;
    }
}

// ── 共享小件 ────────────────────────────────────────────────────────────────

/// 30×30 无底色图标按钮（Tauri `size-icon-sm` ghost）。
fn icon_button(ui: &mut egui::Ui, icon: icons::IconName, color: egui::Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), theme::SURFACE_2);
    }
    icons::draw_icon(ui, rect.center(), icon, color);
    response
}

/// 在矩形内居中画「[图标] 文字」。
fn icon_text(
    ui: &egui::Ui,
    rect: egui::Rect,
    icon: Option<icons::IconName>,
    text: &str,
    color: egui::Color32,
) {
    let text_width = layout::text_width(ui, text, 13.0);
    let icon_width = if icon.is_some() { 16.0 } else { 0.0 };
    let gap = if icon.is_some() { 6.0 } else { 0.0 };
    let start = rect.center().x - (text_width + gap + icon_width) / 2.0;
    if let Some(icon) = icon {
        icons::draw_icon(
            ui,
            egui::pos2(start + icon_width / 2.0, rect.center().y),
            icon,
            color,
        );
    }
    ui.painter().text(
        egui::pos2(start + icon_width + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(13.0),
        color,
    );
}

/// 极简 Markdown：标题 / 列表 / 代码块 / `**粗体**` / `*斜体*` / `` `等宽` ``。
/// 覆盖 Tauri `AssistantMarkdown` 会产出的块级结构；不做表格与引用块。
pub fn render_markdown(ui: &mut egui::Ui, markdown: &str) {
    let mut code = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_code {
                code_block(ui, code.trim_end());
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(6.0);
            continue;
        }
        let (text, size, strong, bullet) = if let Some(value) = trimmed.strip_prefix("### ") {
            (value, 14.0, true, false)
        } else if let Some(value) = trimmed.strip_prefix("## ") {
            (value, 15.0, true, false)
        } else if let Some(value) = trimmed.strip_prefix("# ") {
            (value, 16.0, true, false)
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            (&trimmed[2..], 14.0, false, true)
        } else {
            (trimmed, 14.0, false, false)
        };
        if bullet {
            ui.horizontal_top(|ui| {
                ui.add_space(2.0);
                ui.label(egui::RichText::new("•").size(size).color(theme::INK_3));
                ui.label(inline_job(ui, text, size, strong));
            });
        } else {
            ui.label(inline_job(ui, text, size, strong));
        }
    }
    if !code.is_empty() {
        code_block(ui, code.trim_end());
    }
}

fn inline_job(ui: &egui::Ui, text: &str, size: f32, strong: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width().max(40.0);
    append_inline(&mut job, text, size, strong, false, false);
    job
}

/// 行内样式：`**粗体**`、`*斜体*`、`` `等宽` ``。
fn append_inline(
    job: &mut egui::text::LayoutJob,
    text: &str,
    size: f32,
    strong: bool,
    italics: bool,
    monospace: bool,
) {
    let mut rest = text;
    while !rest.is_empty() {
        let mut matched = false;
        for (open, close, next_strong, next_italics, next_monospace) in [
            ("**", "**", true, italics, monospace),
            ("`", "`", strong, italics, true),
            ("*", "*", strong, true, monospace),
            ("_", "_", strong, true, monospace),
        ] {
            if let Some(after_open) = rest.strip_prefix(open) {
                if let Some(end) = after_open.find(close) {
                    append_span(
                        job,
                        &after_open[..end],
                        size,
                        next_strong,
                        next_italics,
                        next_monospace,
                    );
                    rest = &after_open[end + close.len()..];
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            continue;
        }
        let next = ["**", "*", "`", "_"]
            .iter()
            .filter_map(|marker| rest.find(marker))
            .min()
            .unwrap_or(rest.len());
        let length = if next == 0 {
            rest.chars().next().map(char::len_utf8).unwrap_or(0)
        } else {
            next
        };
        append_span(job, &rest[..length], size, strong, italics, monospace);
        rest = &rest[length..];
    }
}

fn append_span(
    job: &mut egui::text::LayoutJob,
    text: &str,
    size: f32,
    strong: bool,
    italics: bool,
    monospace: bool,
) {
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(
                size,
                if monospace {
                    egui::FontFamily::Monospace
                } else {
                    egui::FontFamily::Proportional
                },
            ),
            color: if strong { theme::INK } else { theme::INK_2 },
            background: if monospace {
                theme::SURFACE_2
            } else {
                egui::Color32::TRANSPARENT
            },
            italics,
            ..Default::default()
        },
    );
}

fn code_block(ui: &mut egui::Ui, code: &str) {
    egui::Frame::new()
        .fill(theme::SURFACE_2)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(code)
                    .monospace()
                    .size(12.5)
                    .color(theme::INK_2),
            );
        });
}

fn truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let mut out: String = chars[..max].iter().collect();
    out.push('…');
    out
}

/// 格式化「已插入 N」文案（宿主在 Completed 阶段缺少 Core message 时使用）。
pub fn inserted_message(lang: Lang, chars: usize) -> String {
    fmt_l10n(lang, "capsule.inserted", &[&chars])
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_linux_egui::PopupChatMessage;

    fn painted_text(output: &egui::FullOutput) -> String {
        let mut text = String::new();
        for clipped in output.shapes.iter() {
            collect(&clipped.shape, &mut text);
        }
        text
    }

    fn collect(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                for row in &text.galley.rows {
                    for glyph in &row.glyphs {
                        if glyph.chr != '\0' {
                            out.push(glyph.chr);
                        }
                    }
                    out.push('\n');
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, out);
                }
            }
            _ => {}
        }
    }

    /// Glyphs are collected row by row, so wrapped copy contains newlines.
    /// Compare whitespace-insensitively.
    fn flat(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// Whether the painted output contains `needle`, ignoring line wrapping.
    fn has(painted: &str, needle: &str) -> bool {
        flat(painted).contains(&flat(needle))
    }

    /// Render one popup for two frames (egui sizes some widgets lazily) and
    /// return everything it painted.
    fn run(size: egui::Vec2, mut render: impl FnMut(&egui::Context) -> String) -> String {
        // Every popup test renders the same frontend as the GPU-state tests, so
        // they share the process-global glow flags and must not run in parallel.
        let _guard = super::siri_gl::gpu_state_guard();
        let ctx = egui::Context::default();
        let mut painted = String::new();
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            });
            let _ = render(&ctx);
            painted = painted_text(&ctx.end_pass());
        }
        painted
    }

    #[test]
    fn polish_preview_paints_header_source_and_actions() {
        let mut state = PreviewPopupState {
            text: "polished text".to_string(),
            source: "source paragraph".to_string(),
        };
        let painted = run(egui::vec2(480.0, 300.0), |ctx| {
            let action = selection_preview(ctx, &mut state, false, Lang::ZhCn);
            assert_eq!(action, PreviewAction::None);
            String::new()
        });
        for expected in [
            tr_l10n(Lang::ZhCn, "selection.polish_preview.title"),
            tr_l10n(Lang::ZhCn, "selection.polish_preview.subtitle"),
            tr_l10n(Lang::ZhCn, "selection.polish_preview.confirm_replace"),
            tr_l10n(Lang::ZhCn, "selection.polish_preview.cancel"),
            tr_l10n(Lang::ZhCn, "selection.polish_preview.source_prefix"),
        ] {
            assert!(
                has(&painted, expected),
                "preview must paint {expected:?}\n{painted}"
            );
        }
    }

    #[test]
    fn polish_preview_confirm_returns_edited_text() {
        let ctx = egui::Context::default();
        let mut state = PreviewPopupState {
            text: "edited result".to_string(),
            source: String::new(),
        };
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(480.0, 300.0),
            )),
            ..Default::default()
        });
        // 直接走一次渲染，确认没有输入时不会误报动作。
        let action = selection_preview(&ctx, &mut state, true, Lang::ZhCn);
        let _ = ctx.end_pass();
        assert_eq!(action, PreviewAction::None);
        assert!(state.text == "edited result");
    }

    #[test]
    fn ask_panel_paints_empty_state_then_thread() {
        let empty = QaPopupState {
            phase: "idle".to_string(),
            ..Default::default()
        };
        let mut composer = String::new();
        let painted = run(egui::vec2(520.0, 520.0), |ctx| {
            selection_ask(ctx, &empty, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        for expected in [
            tr_l10n(Lang::ZhCn, "qa.title"),
            tr_l10n(Lang::ZhCn, "qa.header_hint"),
            tr_l10n(Lang::ZhCn, "qa.empty_title"),
            tr_l10n(Lang::ZhCn, "qa.empty_desc"),
            tr_l10n(Lang::ZhCn, "qa.composer_placeholder"),
        ] {
            assert!(
                has(&painted, expected),
                "empty ask panel must paint {expected:?}\n{painted}"
            );
        }

        let thread = QaPopupState {
            phase: "thinking".to_string(),
            messages: vec![
                PopupChatMessage {
                    role: "user".to_string(),
                    content: "how should I read this?".to_string(),
                    selection_text: Some("selected source".to_string()),
                },
                PopupChatMessage {
                    role: "assistant".to_string(),
                    content: "**key point** here.".to_string(),
                    selection_text: None,
                },
            ],
            selection_preview: Some("selected source".to_string()),
            streaming_answer: String::new(),
            error: Some("network error".to_string()),
            edit_instruction_mode: false,
            edit_apply_available: false,
            edit_revert_available: false,
            pinned: false,
            viewer_login: String::new(),
        };
        let mut composer = String::new();
        let painted = run(egui::vec2(520.0, 520.0), |ctx| {
            selection_ask(ctx, &thread, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        assert!(has(&painted, "how should I read this?"), "{painted}");
        assert!(has(&painted, "selected source"), "{painted}");
        assert!(has(&painted, "network error"), "{painted}");
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "qa.thinking")),
            "{painted}"
        );
    }

    #[test]
    fn ask_panel_recording_shows_selection_chip_and_ring() {
        let state = QaPopupState {
            phase: "recording".to_string(),
            selection_preview: Some("selection shown while recording".to_string()),
            ..Default::default()
        };
        let mut composer = String::new();
        let painted = run(egui::vec2(520.0, 520.0), |ctx| {
            selection_ask(ctx, &state, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "qa.selection_preview")),
            "{painted}"
        );
        assert!(
            has(&painted, "selection shown while recording"),
            "{painted}"
        );
    }

    /// 录音/思考只排队**药丸中心**的 GPU 视觉（录音=声波，思考=流体圆点），
    /// 终态一个都没有。
    ///
    /// 以前这里是 2：录音还会多排一个**外圈红扫光**、思考多一个黑扫光。Tauri 的
    /// 经典药丸只有 1px 中性描边（Capsule.tsx 的 `border: 1px
    /// var(--ol-capsule-pill-border)`），没有外圈扫光——用户报「语音输入弹窗有一个
    /// 红边」就是它。所以数字固定成 1/1/0，谁再把外圈加回来这里就会红。
    #[test]
    fn capsule_queues_the_gpu_glow_per_state() {
        // The GPU state is process-global; take the shared test guard.
        let _guard = super::siri_gl::gpu_state_guard();
        super::siri_gl::seed_gpu_ready_for_tests();
        let callbacks = |state: CapsulePopupState| {
            let ctx = egui::Context::default();
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            });
            let _ = dictation_capsule(&ctx, &state, Lang::ZhCn);
            let output = ctx.end_pass();
            output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
                .count()
        };
        assert_eq!(
            callbacks(CapsulePopupState {
                phase: "recording".into(),
                audio_level: Some(0.2),
                ..Default::default()
            }),
            1,
            "recording = siri wave only, no perimeter ring"
        );
        assert_eq!(
            callbacks(CapsulePopupState {
                phase: "transcribing".into(),
                ..Default::default()
            }),
            1,
            "thinking = orb only, no perimeter ring"
        );
        assert_eq!(
            callbacks(CapsulePopupState {
                phase: "inserted".into(),
                text: "hello".into(),
                ..Default::default()
            }),
            0,
            "terminal capsule paints no glow"
        );
    }

    #[test]
    fn capsule_shows_the_translating_badge_only_when_translating() {
        let badge = tr_l10n(Lang::ZhCn, "capsule.translating");
        let idle = CapsulePopupState {
            phase: "Recording".to_string(),
            audio_level: Some(0.3),
            translation_active: false,
            ..Default::default()
        };
        let painted = run(egui::vec2(200.0, 100.0), |ctx| {
            dictation_capsule(ctx, &idle, Lang::ZhCn);
            String::new()
        });
        assert!(
            !has(&painted, badge),
            "badge must stay hidden while translating is off: {painted}"
        );

        let translating = CapsulePopupState {
            translation_active: true,
            ..idle
        };
        let painted = run(egui::vec2(200.0, 100.0), |ctx| {
            dictation_capsule(ctx, &translating, Lang::ZhCn);
            String::new()
        });
        assert!(has(&painted, badge), "{painted}");
    }

    #[test]
    fn qa_panel_shows_edit_affordances_only_when_the_host_reports_them() {
        let hidden = QaPopupState {
            phase: "idle".to_string(),
            ..Default::default()
        };
        let mut composer = String::new();
        let painted = run(egui::vec2(520.0, 520.0), |ctx| {
            selection_ask(ctx, &hidden, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        assert!(!has(&painted, tr_l10n(Lang::ZhCn, "qa.edit_apply_replace")));
        assert!(!has(
            &painted,
            tr_l10n(Lang::ZhCn, "qa.edit_revert_previous")
        ));

        let ready = QaPopupState {
            phase: "idle".to_string(),
            edit_apply_available: true,
            edit_revert_available: true,
            ..Default::default()
        };
        let painted = run(egui::vec2(520.0, 520.0), |ctx| {
            selection_ask(ctx, &ready, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "qa.edit_apply_replace")),
            "{painted}"
        );
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "qa.edit_revert_previous")),
            "{painted}"
        );
        // 「编辑指令」勾选框常驻在输入组左下角。
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "qa.edit_instruction_mode")),
            "{painted}"
        );

        // 只有可回退时才出现「保留上一版本」。
        let apply_only = QaPopupState {
            phase: "idle".to_string(),
            edit_apply_available: true,
            ..Default::default()
        };
        let painted = run(egui::vec2(520.0, 520.0), |ctx| {
            selection_ask(ctx, &apply_only, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "qa.edit_apply_replace")),
            "{painted}"
        );
        assert!(!has(
            &painted,
            tr_l10n(Lang::ZhCn, "qa.edit_revert_previous")
        ));
    }

    #[test]
    fn qa_panel_paints_the_pin_affordance_in_both_states() {
        // 图钉是无文字的图标按钮：这里断言两种状态都能整帧渲染（含 tooltip 绑定），
        // 动作本身由 popup.rs 的协议测试覆盖。
        for pinned in [false, true] {
            let state = QaPopupState {
                phase: "idle".to_string(),
                pinned,
                ..Default::default()
            };
            let mut composer = String::new();
            let painted = run(egui::vec2(520.0, 520.0), |ctx| {
                selection_ask(ctx, &state, &mut composer, Lang::ZhCn, None);
                String::new()
            });
            assert!(
                has(&painted, tr_l10n(Lang::ZhCn, "qa.empty_title")),
                "pinned={pinned}\n{painted}"
            );
        }
    }

    #[test]
    fn qa_panel_renders_the_github_avatar_texture_when_present() {
        let ctx = egui::Context::default();
        let image = egui::ColorImage::new([2, 2], vec![egui::Color32::RED; 4]);
        let texture = ctx.load_texture("test-avatar", image, egui::TextureOptions::LINEAR);
        let state = QaPopupState {
            phase: "idle".to_string(),
            messages: vec![PopupChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                selection_text: None,
            }],
            ..Default::default()
        };
        let mut composer = String::new();
        let mut painted = String::new();
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(520.0, 520.0),
                )),
                ..Default::default()
            });
            selection_ask(&ctx, &state, &mut composer, Lang::ZhCn, Some(&texture));
            painted = painted_text(&ctx.end_pass());
        }
        assert!(has(&painted, "hello"), "{painted}");
    }

    /// Every fill / stroke colour the frame painted, so a test can assert the
    /// classic pill never grows a coloured outline again.
    fn painted_colors(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Rect(rect) => {
                out.push(rect.fill);
                out.push(rect.stroke.color);
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    painted_colors(shape, out);
                }
            }
            _ => {}
        }
    }

    /// Render one capsule frame and collect the colours plus callback count.
    fn capsule_frame(state: &CapsulePopupState) -> (Vec<egui::Color32>, usize) {
        let _guard = super::siri_gl::gpu_state_guard();
        let ctx = egui::Context::default();
        let mut colors = Vec::new();
        let mut callbacks = 0;
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            });
            let _ = dictation_capsule(&ctx, state, Lang::ZhCn);
            let output = ctx.end_pass();
            colors.clear();
            callbacks = 0;
            for clipped in &output.shapes {
                painted_colors(&clipped.shape, &mut colors);
                if matches!(clipped.shape, egui::Shape::Callback(_)) {
                    callbacks += 1;
                }
            }
        }
        (colors, callbacks)
    }

    /// Whether a colour reads as the OpenLess error red (the old ring tint).
    fn is_reddish(color: egui::Color32) -> bool {
        color.a() > 40 && color.r() > 150 && color.g() < 110 && color.b() < 110
    }

    #[test]
    fn recording_capsule_paints_no_coloured_outline() {
        // Tauri 的经典药丸只有 1px 中性描边（Capsule.tsx：border 1px
        // var(--ol-capsule-pill-border)），录音时只把药丸随音量放大 1.8%。
        // 外圈红/黑扫光是本仓自己加的，用户报「语音输入弹窗有一个红边」——
        // 这条测试锁死它不许回来。
        for phase in ["Recording", "Transcribing", "Polishing"] {
            let state = CapsulePopupState {
                phase: phase.to_string(),
                text: String::new(),
                audio_level: Some(0.6),
                translation_active: false,
                style: "classic".to_string(),
            };
            let (colors, _) = capsule_frame(&state);
            let reddish: Vec<_> = colors
                .iter()
                .copied()
                .filter(|color| is_reddish(*color))
                .collect();
            assert!(
                reddish.is_empty(),
                "{phase} capsule must not paint a red outline, found {reddish:?}"
            );
        }
    }

    #[test]
    fn recording_capsule_keeps_its_centre_visual() {
        // 去掉外圈之后，录音相位的运动感来自药丸中心（GPU 波形，失败时回退成
        // Tauri 的 5 根音量竖条）——两者至少有一个必须在。
        let state = CapsulePopupState {
            phase: "Recording".to_string(),
            text: String::new(),
            audio_level: Some(0.6),
            translation_active: false,
            style: "siri".to_string(),
        };
        let (colors, callbacks) = capsule_frame(&state);
        // 音量竖条是 3px 宽的小圆角矩形：数一下细长条形的填充个数。
        let fills = colors.iter().filter(|color| color.a() > 0).count();
        assert!(
            callbacks > 0 || fills >= 6,
            "recording capsule must keep the centre visual (callbacks={callbacks}, fills={fills})"
        );
    }

    #[test]
    fn capsule_paints_state_specific_content() {
        let recording = CapsulePopupState {
            phase: "Recording".to_string(),
            text: String::new(),
            audio_level: Some(0.4),
            translation_active: false,
            style: "siri".to_string(),
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &recording, Lang::ZhCn);
            String::new()
        });
        assert!(
            !painted.contains(tr_l10n(Lang::ZhCn, "capsule.thinking")),
            "recording capsule shows level bars, not the thinking label: {painted}"
        );

        let transcribing = CapsulePopupState {
            phase: "Transcribing".to_string(),
            ..Default::default()
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &transcribing, Lang::ZhCn);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "capsule.thinking")),
            "{painted}"
        );

        let done = CapsulePopupState {
            phase: "Completed".to_string(),
            text: inserted_message(Lang::ZhCn, 12),
            audio_level: None,
            translation_active: false,
            style: "siri".to_string(),
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &done, Lang::ZhCn);
            String::new()
        });
        assert!(has(&painted, "12"), "{painted}");

        let failed = CapsulePopupState {
            phase: "Failed".to_string(),
            text: String::new(),
            audio_level: None,
            translation_active: false,
            style: "siri".to_string(),
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &failed, Lang::ZhCn);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "capsule.error")),
            "{painted}"
        );
    }
}

// ── Less Computer 面板 ──────────────────────────────────────────────────────

/// Less Computer 浮窗的动作（宿主转成 `PopupToHost` 消息）。
pub enum LessComputerAction {
    None,
    /// ✕ → 只收起面板（已完成的一轮保留）。
    Dismiss,
    /// Esc / 停止 → 取消当前这一轮。
    Cancel,
    /// 输入框回车 / 发送。
    Submit(String),
    /// 阻塞命令的批准或拒绝。
    Approve {
        token: String,
        approved: bool,
    },
}

/// Less Computer 语音 Agent 浮窗（Tauri `LessComputerPanel.tsx`）。
///
/// 面板只呈现宿主推来的事件序列（`LessComputerPopupState::entries`），不解释产品意图：
/// 用户指令是右对齐气泡、工具调用与上下文压缩是行内标记、助手正文走 markdown，
/// 阻塞命令在输入框上方给出批准 / 拒绝。
pub fn less_computer(
    ctx: &egui::Context,
    state: &LessComputerPopupState,
    composer: &mut String,
    lang: Lang,
) -> LessComputerAction {
    let mut action = LessComputerAction::None;
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .corner_radius(egui::CornerRadius::same(14))
                .stroke(egui::Stroke::new(0.5, theme::LINE))
                .inner_margin(egui::Margin::same(CARD_SPACING as i8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(tr_l10n(lang, "less_computer.title"))
                            .size(16.0)
                            .strong()
                            .color(theme::INK),
                    );
                    ui.add_space(3.0);
                    // 运行中显示「执行中…」，否则是那句「想让电脑做什么？」的副标题。
                    let subtitle = if state.working {
                        tr_l10n(lang, "less_computer.working")
                    } else {
                        tr_l10n(lang, "less_computer.subtitle")
                    };
                    ui.label(egui::RichText::new(subtitle).size(12.0).color(theme::INK_4));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if icon_button(ui, icons::IconName::Close, theme::INK_3).clicked() {
                        action = LessComputerAction::Dismiss;
                    }
                });
            });
            ui.add_space(10.0);

            // 审批卡的实际高度随命令/警告文字行数变化（警告会换行到两行），预留值取
            // “单行标题 + 等宽命令 + 两行警告 + 按钮行 + 内边距”。取小了会把
            // 底部输入框挤出卡片外（越出窗口下缘），这是实测发现的。
            let approval_height = if state.approval.is_some() { 158.0 } else { 0.0 };
            let list_height =
                (ui.available_height() - COMPOSER_HEIGHT - approval_height - 12.0).max(120.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), list_height), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("openless-less-computer")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if state.entries.is_empty() && !state.working {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(tr_l10n(lang, "less_computer.subtitle"))
                                        .size(13.0)
                                        .color(theme::INK_4),
                                );
                            });
                        }
                        for entry in &state.entries {
                            match entry.kind.as_str() {
                                "user" => user_bubble(ui, &entry.text),
                                "assistant" => {
                                    ui.add_space(2.0);
                                    render_markdown(ui, &entry.text);
                                    ui.add_space(2.0);
                                }
                                "tool" | "note" => marker_row(ui, &entry.text, theme::INK_3),
                                "compaction" => compaction_marker(ui, &entry.text),
                                "error" => {
                                    ui.label(
                                        egui::RichText::new(&entry.text)
                                            .size(12.5)
                                            .color(theme::ERR),
                                    );
                                }
                                _ => marker_row(ui, &entry.text, theme::INK_3),
                            }
                        }
                        if state.working {
                            ui.add_space(2.0);
                            marker_row(ui, tr_l10n(lang, "less_computer.working"), theme::INK_3);
                        }
                    });
            });

            if let Some(approval) = &state.approval {
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(theme::WARN_SOFT)
                    .stroke(egui::Stroke::new(0.5, theme::WARN))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "less_computer.approval_title"))
                                .size(12.5)
                                .strong()
                                .color(theme::INK),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&approval.command)
                                .font(egui::FontId::monospace(11.5))
                                .color(theme::INK_2),
                        );
                        if !approval.reason.is_empty() {
                            ui.add_space(3.0);
                            ui.label(
                                egui::RichText::new(&approval.reason)
                                    .size(11.0)
                                    .color(theme::INK_3),
                            );
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if small_action_button(ui, tr_l10n(lang, "less_computer.approve"), true)
                                .clicked()
                            {
                                action = LessComputerAction::Approve {
                                    token: approval.token.clone(),
                                    approved: true,
                                };
                            }
                            if small_action_button(ui, tr_l10n(lang, "less_computer.deny"), false)
                                .clicked()
                            {
                                action = LessComputerAction::Approve {
                                    token: approval.token.clone(),
                                    approved: false,
                                };
                            }
                        });
                    });
            }

            ui.add_space(6.0);
            let width = ui.available_width();
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, COMPOSER_HEIGHT), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(12), theme::SURFACE);
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(12),
                egui::Stroke::new(0.5, theme::LINE_STRONG),
                egui::StrokeKind::Inside,
            );
            let send_rect = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 20.0, rect.center().y),
                egui::vec2(28.0, 28.0),
            );
            let send = ui
                .interact(
                    send_rect,
                    egui::Id::new("less-computer-send"),
                    egui::Sense::click(),
                )
                .on_hover_text(tr_l10n(lang, "less_computer.send"));
            if !composer.trim().is_empty() {
                ui.painter().rect_filled(
                    send_rect,
                    egui::CornerRadius::same(14),
                    if send.hovered() {
                        theme::INK_2
                    } else {
                        theme::INK
                    },
                );
                icons::draw_icon(
                    ui,
                    send_rect.center(),
                    icons::IconName::Send,
                    theme::SURFACE,
                );
            }
            let text_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 12.0, rect.top()),
                egui::pos2(send_rect.left() - 6.0, rect.bottom()),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("less-computer-composer")
                    .max_rect(text_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let response = child.add(
                egui::TextEdit::singleline(composer)
                    .id(egui::Id::new("less-computer-composer-input"))
                    .hint_text(tr_l10n(lang, "less_computer.input_placeholder"))
                    .font(egui::FontId::proportional(13.5))
                    .text_color(theme::INK)
                    .frame(false)
                    .desired_width(text_rect.width())
                    .vertical_align(egui::Align::Center),
            );
            let submitted =
                response.lost_focus() && child.input(|input| input.key_pressed(egui::Key::Enter));
            if submitted || send.clicked() {
                let text = composer.trim().to_string();
                if !text.is_empty() {
                    action = LessComputerAction::Submit(text);
                }
            }
            // Esc 取消当前这一轮（Tauri 面板的 Esc 语义）。
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                action = LessComputerAction::Cancel;
            }
        });
    action
}

/// 右对齐的用户指令气泡（Tauri `Bubble align="end"`）。
fn user_bubble(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let max = (ui.available_width() * 0.82).max(80.0);
        ui.set_max_width(max);
        egui::Frame::new()
            .fill(theme::BLUE_SOFT)
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(9, 6))
            .show(ui, |ui| {
                ui.set_max_width(max - 18.0);
                ui.label(egui::RichText::new(text).size(12.5).color(theme::INK));
            });
    });
    ui.add_space(2.0);
}

/// 行内标记：小圆点 + 辅助色文字（Tauri `Marker`）。
fn marker_row(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 2.0, color);
        ui.label(egui::RichText::new(text).size(11.5).color(color));
    });
}

/// 上下文压缩标记（Tauri `Marker variant="separator"`）。
fn compaction_marker(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 16.0), egui::Sense::hover());
        ui.painter().line_segment(
            [rect.left_center(), rect.right_center()],
            egui::Stroke::new(0.5, theme::LINE_SOFT),
        );
        let galley = ui.painter().layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(11.0),
            theme::INK_4,
        );
        let center = rect.center();
        ui.painter().rect_filled(
            egui::Rect::from_center_size(center, galley.size() + egui::vec2(10.0, 0.0)),
            egui::CornerRadius::same(7),
            theme::SURFACE,
        );
        ui.painter().galley(
            egui::pos2(
                center.x - galley.rect.width() / 2.0,
                center.y - galley.rect.height() / 2.0,
            ),
            galley,
            theme::INK_4,
        );
    });
    ui.add_space(2.0);
}

/// 批准 / 拒绝按钮（Tauri `Button`）。
fn small_action_button(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let text = egui::RichText::new(label).size(11.5);
    let button = if primary {
        egui::Button::new(text.color(theme::SURFACE)).fill(theme::INK)
    } else {
        egui::Button::new(text.color(theme::INK_2))
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
    };
    ui.add(
        button
            .corner_radius(egui::CornerRadius::same(7))
            .min_size(egui::vec2(56.0, 26.0)),
    )
}
