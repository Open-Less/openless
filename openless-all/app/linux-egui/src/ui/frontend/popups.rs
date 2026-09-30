//! Three auxiliary windows: the dictation capsule, the selection-ask panel and
//! the selection-polish preview.
//!
//! Like the page layer this module is a pure renderer: it reads the popup
//! snapshot and returns at most one action, which the popup host translates into
//! a `PopupToHost` message. Layout, spacing, colours and copy mirror the Tauri
//! windows — `src/components/Capsule.tsx` (classic pill) and `src/pages/QaPanel.tsx`
//! (shadcn chat card, which since the preview merge also hosts the read-only
//! polish result mode).
//!
//! The chat panel renders in the shadcn zinc palette, which maps onto the theme
//! tokens: white [`theme::SURFACE`], [`theme::INK`] foreground, [`theme::SURFACE_2`]
//! muted fill, [`theme::INK_3`] muted text, [`theme::LINE`] border.

use eframe::egui;

use super::{capsule_motion, icons, layout, siri_wgpu, theme};
use openless_linux_egui::{
    fmt_l10n, tr_l10n, CapsulePopupState, Lang, LessComputerPopupState, PopupChatMessage,
    QaPolishState, QaPopupState,
};

/// Result of rendering the polish-result mode inside the selection-ask panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolishResultAction {
    None,
    /// ✕ / 取消 → the host sends `CancelPolish`.
    Cancel,
    /// ✓ 确认并替换（即「插入」：把结果写回原选区）→ the host sends `ConfirmPolish`.
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
    /// 润色结果模式：✓ 确认并替换 → 宿主发 `ConfirmPolish`。
    ConfirmPolish(String),
    /// 润色结果模式：取消 → 宿主发 `CancelPolish`。
    CancelPolish,
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
    /// 「要记住这个词吗？」卡片上点了「记住」（Tauri `acceptPendingCorrection`）。
    AcceptSuggestion(String),
    /// 卡片上点了「不用」（Tauri `rejectPendingCorrection`）。
    RejectSuggestion(String),
}

/// 「要记住这个词吗？」卡片的尺寸，与 Tauri `VocabSuggestionCard` 一致：
/// 宽 320、标题区 72、每行 36（`VOCAB_CARD_*`）。
const CARD_WIDTH: f32 = 320.0;
const CARD_CHROME: f32 = 72.0;
const CARD_ROW: f32 = 36.0;
/// 卡片到窗口边缘的留白（Tauri `VOCAB_CARD_EDGE_MARGIN`）。
const CARD_EDGE: f32 = 12.0;
/// 行数多到装不下时标题区至少留这么高，剩下的分给行 —— Tauri 会改窗口大小，我们这个
/// 舞台是固定的 460×180，所以宁可压行高，也不让最后一行露在窗外。
const CARD_CHROME_MIN: f32 = 44.0;
const CARD_BUTTON: f32 = 28.0;
/// 标题占的高度（画的时候用固定值，几何才能是纯函数、测试才点得中按钮）。
const CARD_TITLE_BLOCK: f32 = 24.0;

/// 卡片几何：一列行 + 每行右侧「不用 / 记住」两颗圆钮。
pub(crate) struct VocabCardLayout {
    pub card: egui::Rect,
    pub rows: Vec<VocabCardRow>,
}

pub(crate) struct VocabCardRow {
    pub text: egui::Rect,
    pub accept: egui::Rect,
    pub reject: egui::Rect,
}

/// 按 Tauri `VocabSuggestionCard` 的尺寸算卡片几何。
///
/// 与 Tauri 的唯一差别：Tauri 会把窗口改成卡片大小（320 × 72+36n）并挪到屏幕右下角，
/// 我们的胶囊舞台是固定的 460×180，所以行数多时压缩行高，并把卡片靠右放。
pub(crate) fn vocab_card_layout(stage: egui::Rect, rows: usize) -> VocabCardLayout {
    let count = rows.max(1) as f32;
    let max_height = (stage.height() - 2.0 * CARD_EDGE).max(CARD_CHROME_MIN);
    let mut row_height = CARD_ROW;
    let mut height = CARD_CHROME + CARD_ROW * count;
    if height > max_height {
        row_height = ((max_height - CARD_CHROME_MIN) / count).max(18.0);
        height = (CARD_CHROME_MIN + row_height * count).min(max_height);
    }
    let width = CARD_WIDTH.min(stage.width() - 2.0 * CARD_EDGE).max(1.0);
    let card = egui::Rect::from_min_size(
        egui::pos2(
            stage.right() - CARD_EDGE - width,
            stage.bottom() - CARD_EDGE - height,
        ),
        egui::vec2(width, height),
    );
    let inner = card.shrink(12.0);
    let top = inner.top() + CARD_TITLE_BLOCK;
    let row_height = row_height.min(((inner.bottom() - top) / count).max(1.0));
    // 行被压缩时按钮必须跟着缩：Core 允许一张卡 5 条，硬塞 28px 圆钮会让相邻两行叠在一起。
    let button = CARD_BUTTON.min((row_height - 2.0).max(14.0));
    let rows = (0..rows)
        .map(|index| {
            let row = egui::Rect::from_min_size(
                egui::pos2(inner.left(), top + row_height * index as f32),
                egui::vec2(inner.width(), row_height),
            );
            // 两颗 28 宽圆钮 + 8 间距（Tauri 的 `CardButton` 就是胶囊确认/取消那一对）。
            let accept = egui::Rect::from_center_size(
                egui::pos2(row.right() - button / 2.0, row.center().y),
                egui::vec2(button, button),
            );
            let reject = egui::Rect::from_center_size(
                egui::pos2(accept.left() - 8.0 - button / 2.0, row.center().y),
                egui::vec2(button, button),
            );
            VocabCardRow {
                text: egui::Rect::from_min_max(
                    row.min,
                    egui::pos2(reject.left() - 8.0, row.bottom()),
                ),
                accept,
                reject,
            }
        })
        .collect();
    VocabCardLayout { card, rows }
}

/// Tauri `selection-polish-preview` 面板的边距（`padding: 18`）。
/// **只服务旧的独立预览窗口**的常量已随窗口一起删除；合并后由选区助手
/// 面板的 `CARD_SPACING` 承载。
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
/// Linux egui 让 Typeless 复用 OpenLess 经典药丸的可见 footprint，避免录音、thinking、
/// 完成和错误状态之间发生尺寸跳变；颜色、11 根波形和状态按钮仍保留 Typeless 风格。
const TYPELESS_SCALE: f32 = 1.0;
const TYPELESS_WIDTH: f32 = PILL_WIDTH;
const TYPELESS_HEIGHT: f32 = PILL_HEIGHT;
const TYPELESS_BUTTON: f32 = ROUND_BUTTON;
const TYPELESS_STOP_BUTTON: f32 = 24.0;
const TYPELESS_TEXT_SIZE: f32 = 16.0;
/// `.ol-typeless-*` 调色板。
const TYPELESS_BG: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x1b);
const TYPELESS_BORDER: egui::Color32 = egui::Color32::from_rgb(0x52, 0x52, 0x5b);
const TYPELESS_INK: egui::Color32 = egui::Color32::from_rgb(0xfa, 0xfa, 0xfa);
const TYPELESS_BUTTON_BG: egui::Color32 = egui::Color32::from_rgb(0x3f, 0x3f, 0x46);
/// 徽章与药丸之间的间距（Tauri `badgeGap`）。
const CAPSULE_BADGE_GAP: f32 = 8.0;

// ── 润色结果模式（选区助手面板内的第二套 UI） ──────────────────────────────

/// 润色结果：标题 + 副标题 + ✕、**只读**结果框、原文摘要、取消 / 确认并替换。
///
/// 视觉与 Tauri 选区助手面板（`src/pages/QaPanel.tsx`）一致 —— 原先那个独立
/// 预览窗口已下线，两侧现在都把润色结果画在选区助手面板里（宿主侧的
/// `HostAction::ShowSelectionPreview` 落到这里）。按用户要求，
/// 结果**只读**（不做就地编辑）；「确认并替换」就是把结果写回原选区的「插入」。
pub fn polish_result_mode(
    ui: &mut egui::Ui,
    state: &QaPolishState,
    lang: Lang,
) -> PolishResultAction {
    let mut action = PolishResultAction::None;
    // 与选区助手面板（`selection_ask`）共用同一套外壳：
    //   CardHeader  = 14px 横向 / 12px 纵向留白 + 底部 hairline（整条可拖）
    //   CardContent = 左右 14px 留白
    //   CardFooter  = 顶部 hairline + 14px / 12px 留白
    // 润色结果只把中间换成「只读结果 + 原文摘要」，头尾的尺寸、间距、分隔线与
    // 选区助手完全一致 —— 不再是自己一套排版（那看着就像旧的独立预览窗口）。
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(CARD_SPACING as i8, 12))
        .show(ui, |ui| {
            let row = ui
                .horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "selection.polish_preview.title"))
                                .size(16.0)
                                .strong()
                                .color(theme::INK),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(tr_l10n(lang, "selection.polish_preview.subtitle"))
                                .size(12.0)
                                .color(theme::INK_4),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if icon_button(ui, icons::IconName::Close, theme::INK_3)
                            .on_hover_text(tr_l10n(lang, "selection.polish_preview.cancel"))
                            .clicked()
                        {
                            action = PolishResultAction::Cancel;
                        }
                    });
                })
                .response
                .interact(egui::Sense::drag());
            if row.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                // 弹窗也是「按下即交给合成器」，同样会丢掉这次手势的释放。
                super::layout::note_window_gesture_handoff(ui.ctx());
            }
        });
    hairline(ui, theme::LINE_SOFT);

    // 底部固定高度：与选区助手一样是「14px / 12px 留白 + 34px 按钮」。
    let footer_height = CARD_SPACING * 2.0 + 34.0;
    let source_height = if state.source.is_empty() { 0.0 } else { 48.0 };
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(CARD_SPACING as i8, 0))
        .show(ui, |ui| {
            ui.add_space(CARD_SPACING);
            // 只读结果框：撑满剩余高度（Tauri `flex: 1; min-height: 150`），
            // 文本可选可滚动，但没有光标、不会回到宿主。
            let editor_height = (ui.available_height() - footer_height - source_height).max(150.0);
            let width = ui.available_width();
            egui::Frame::new()
                .fill(theme::CONTENT_BG)
                .stroke(egui::Stroke::new(0.5, theme::LINE_STRONG))
                .corner_radius(egui::CornerRadius::same(9))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(width - 24.0, editor_height - 24.0));
                    egui::ScrollArea::vertical()
                        .id_salt("openless-polish-result")
                        .auto_shrink([false, false])
                        // 上限卡在算好的框高上：`set_min_size` 不限制上限，
                        // 不限的话滚动区会把下面的原文摘要顶出可视区。
                        .max_height((editor_height - 24.0).max(60.0))
                        .show(ui, |ui| {
                            ui.set_min_width(width - 24.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&state.text)
                                        .size(14.0)
                                        .color(theme::INK),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
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
        });

    hairline(ui, theme::LINE_SOFT);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(CARD_SPACING as i8, 12))
        .show(ui, |ui| {
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
                    action = PolishResultAction::Confirm(state.text.clone());
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
                    action = PolishResultAction::Cancel;
                }
            });
        });
    action
}

// ── 划词追问 ────────────────────────────────────────────────────────────────

/// 划词追问面板：卡片头（标题 + 副行 + ✕）、消息流（空状态 / 对话 / 思考中 /
/// 出错）、底部输入组（选区条 + 输入框 + 麦克风 + 发送）。
pub fn selection_ask(
    root_ui: &mut egui::Ui,
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
        .show(root_ui, |ui| {
            // 首帧只编译不绘制地把三个程序编译好（进程内只排一次），
            // 免得录音/思考的第一帧才发现要编译——那是按热键后「慢一拍」的来源。
            // ── 润色结果模式：同一个面板，第二套 UI（原独立预览窗口的同一套视觉）。
            if let Some(polish) = state.polish.as_ref() {
                action = match polish_result_mode(ui, polish, lang) {
                    PolishResultAction::None => QaAction::None,
                    PolishResultAction::Cancel => QaAction::CancelPolish,
                    PolishResultAction::Confirm(text) => QaAction::ConfirmPolish(text),
                };
                return;
            }
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
                        super::layout::note_window_gesture_handoff(ui.ctx());
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
            let recording_selection_height = if recording && state.selection_preview.is_some() {
                28.0
            } else {
                0.0
            };
            let footer_height = CARD_SPACING * 2.0
                + COMPOSER_HEIGHT
                + 12.0
                + 22.0
                + recording_selection_height
                + edit_block;
            // Never force an 80px thread area: on short work areas that
            // minimum overlaps the fixed composer and clips its bottom edge.
            let content_height = (ui.available_height() - footer_height).max(0.0);
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
                        let drive = siri_wgpu::SiriDrive {
                            level: 0.0,
                            resolved: if recording { 1.0 } else { 0.0 },
                            // 思考态转得更快，和 Tauri 的 state→speed 语义一致。
                            speed: if recording { 1.0 } else { 1.45 },
                            ..Default::default()
                        };
                        let dt = ui.input(|input| input.stable_dt);
                        let clock = siri_wgpu::tick(ui.ctx(), "qa-composer-ring", drive, dt);
                        let effect = siri_wgpu::SiriEffect::ring(
                            clock.time,
                            12.0,
                            if recording { 2.0 } else { 1.6 },
                        )
                        .with_tint(tint);
                        if !siri_wgpu::paint(ui, rect.expand(3.0), effect) {
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
                            .frame(egui::Frame::NONE)
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
/// here because the effect is additive on a translucent window).
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
fn translating_badge(ui: &mut egui::Ui, pill: egui::Rect, lang: Lang, scale: f32) {
    let label = tr_l10n(lang, "capsule.translating");
    let text_width = layout::text_width(ui, label, 10.5 * scale);
    let width = text_width + (5.0 + 5.0 + 20.0) * scale;
    let height = 19.0 * scale;
    let gap = CAPSULE_BADGE_GAP * scale;
    let rect = egui::Rect::from_center_size(
        egui::pos2(pill.center().x, pill.top() - gap - height / 2.0),
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
    let dot = egui::pos2(rect.left() + 10.0 * scale, rect.center().y);
    painter.circle_filled(dot, 2.5 * scale, theme::BLUE);
    painter.text(
        egui::pos2(dot.x + (5.0 + 2.5) * scale, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(10.5 * scale),
        theme::BLUE,
    );
}

/// 录音胶囊：经典药丸（Tauri `ClassicPill`）—— 左 ✕、中间状态、右 ✓。
///
/// `warmup_ms` 是学出来的「预备 → 就绪」平均耗时，驱动光条的预测式展开（Tauri `warmupMs`
/// prop；那边存在 localStorage，这边由宿主量、存在 UI 状态文档里）。
pub fn dictation_capsule(
    root_ui: &mut egui::Ui,
    state: &CapsulePopupState,
    lang: Lang,
    warmup_ms: f32,
) -> CapsuleAction {
    let mut action = CapsuleAction::None;
    let phase = state.phase.to_ascii_lowercase();
    // 「要记住这个词吗？」卡片与录音胶囊共用一个窗口：有候选时整窗只画这张卡。
    if !state.suggestions.is_empty() {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(root_ui, |ui| {
                action = capsule_suggestion_card(ui, state, lang);
            });
        return action;
    }
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(root_ui, |ui| {
            // 胶囊进程的首帧预热（同 QA 面板；录音环与 Siri 波都是 GPU 路径）。
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
            let siri = use_gpu;
            let (pill_bg, pill_border, pill_ink) = if typeless {
                (TYPELESS_BG, TYPELESS_BORDER, TYPELESS_INK)
            } else if siri {
                (
                    egui::Color32::TRANSPARENT,
                    egui::Color32::TRANSPARENT,
                    theme::INK_2,
                )
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
                translating_badge(ui, rect, lang, if typeless { TYPELESS_SCALE } else { 1.0 });
            }
            // The capsule footprint is invariant across recording, thinking and
            // terminal frames. The old volume scale made the pill visibly jump
            // smaller when thinking ended, just before the hide deadline.
            // Motion belongs to the centre effect, never to the host geometry.
            let pill = rect;
            if !siri {
                ui.painter().rect_filled(
                    pill,
                    egui::CornerRadius::same((pill_height / 2.0) as u8),
                    pill_bg,
                );
                ui.painter().rect_stroke(
                    pill,
                    egui::CornerRadius::same((pill_height / 2.0) as u8),
                    egui::Stroke::new(if typeless { 0.5 } else { 1.0 }, pill_border),
                    egui::StrokeKind::Inside,
                );
            }
            let processing = matches!(
                phase.as_str(),
                "starting" | "transcribing" | "polishing" | "inserting"
            );
            // 终态：光点做「六点合回一颗圆」的收尾，转速回落 1.0（Tauri `VoiceOrbStage`
            // 的 `merging` / `speed`）。
            let terminal = matches!(
                phase.as_str(),
                "completed" | "done" | "inserted" | "cancelled" | "failed" | "error"
            );
            // 相位时序（扫光周期、wave 淡出）与「新会话复位」：Tauri 那边靠 CSS / 组件
            // 重新挂载，egui 没这两样，节奏都在 `capsule_motion` 里算并有单测。
            let now = ui.input(|input| input.time);
            let motion_id = egui::Id::new("openless-capsule-motion");
            let mut motion = ui.ctx().data_mut(|data| {
                data.get_temp::<capsule_motion::CapsuleMotion>(motion_id)
                    .unwrap_or_default()
            });
            let new_session = motion.update(&phase, now);
            let wave_opacity = motion.wave_opacity(now);
            let shine_cycle = motion.shine_cycle_seconds(now);
            ui.ctx()
                .data_mut(|data| data.insert_temp(motion_id, motion));
            if new_session {
                // 两个窗口进程都常驻：新会话必须显式复位，否则上一轮的光条是展开的、
                // 圆点环也已经散开了，入场动画就没了（Tauri 每次重新挂载天然是新的）。
                siri_wgpu::reset(ui.ctx(), "capsule-siri-wave");
                siri_wgpu::reset(ui.ctx(), "capsule-siri-orb");
            }
            // Siri is a clean, transparent listening indicator. Classic keeps
            // both actions visible; Typeless follows Tauri and shows the two
            // large actions only while recording, then only its small stop
            // action while processing.
            let center = if siri {
                // Siri is a full 460×180 light stage, not a wave squeezed
                // inside the classic 176×42 pill. All phases share this host.
                available
            } else if typeless && phase != "recording" {
                if processing {
                    let stop_rect = egui::Rect::from_center_size(
                        egui::pos2(
                            rect.right() - 7.0 - TYPELESS_STOP_BUTTON / 2.0,
                            rect.center().y,
                        ),
                        egui::vec2(TYPELESS_STOP_BUTTON, TYPELESS_STOP_BUTTON),
                    );
                    let stop = ui.interact(
                        stop_rect,
                        ui.id().with("openless-typeless-stop"),
                        egui::Sense::click(),
                    );
                    round_button(
                        ui,
                        stop_rect,
                        icons::IconName::Close,
                        stop.hovered(),
                        TYPELESS_BUTTON_BG,
                        TYPELESS_INK,
                        (0.0, 13.0 * TYPELESS_SCALE),
                    );
                    if stop.clicked() {
                        action = CapsuleAction::Cancel;
                    }
                }
                rect.shrink(4.0)
            } else {
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
                    (
                        if typeless { 0.0 } else { 0.8 },
                        if typeless {
                            24.0 * TYPELESS_SCALE
                        } else {
                            13.0
                        },
                    ),
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
                    (
                        if typeless { 0.0 } else { 0.8 },
                        if typeless {
                            24.0 * TYPELESS_SCALE
                        } else {
                            13.0
                        },
                    ),
                );
                if confirm.clicked() {
                    action = CapsuleAction::Confirm;
                }
                egui::Rect::from_min_max(
                    egui::pos2(cancel_rect.right() + 4.0, rect.top() + 4.0),
                    egui::pos2(confirm_rect.left() - 4.0, rect.bottom() - 4.0),
                )
            };
            if phase == "recording" {
                // Siri capsules are transparent overlays. Queue the spectral
                // ribbon through the shared WGPU callback used by both eframe
                // windows and the native layer-shell surface.
                let drive = siri_wgpu::SiriDrive {
                    level: state.audio_level.unwrap_or_default(),
                    resolved: 1.0,
                    speed: 1.0,
                    warming: state.audio_level.is_none(),
                    warmup_ms,
                    merging: false,
                };
                let dt = ui.input(|input| input.stable_dt);
                let clock = siri_wgpu::tick(ui.ctx(), "capsule-siri-wave", drive, dt);
                if siri {
                    let _ = siri_wgpu::paint(
                        ui,
                        center,
                        siri_wgpu::SiriEffect::wave(clock.time, clock.level),
                    );
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(16));
                } else {
                    audio_bars(
                        ui,
                        center,
                        state.audio_level.unwrap_or_default(),
                        bar_count,
                        pill_ink,
                    );
                }
            } else {
                // 思考态 / 终态：Siri 舞台画光点（终态是「六点合回一颗圆」的收尾），
                // 经典 / typeless 只画文案。
                if siri {
                    // wave → orb 的交叉淡出：切态后 0.55s 内保持可见，再用 0.6s 淡出，
                    // 期间波形继续向圆心收拢（Tauri `opacity .6s ease-out .55s`）。
                    if let Some(opacity) = wave_opacity {
                        let fade_drive = siri_wgpu::SiriDrive {
                            level: state.audio_level.unwrap_or_default(),
                            resolved: 0.0,
                            speed: 1.0,
                            warming: false,
                            warmup_ms,
                            merging: false,
                        };
                        let dt = ui.input(|input| input.stable_dt);
                        let clock = siri_wgpu::tick(ui.ctx(), "capsule-siri-wave", fade_drive, dt);
                        let _ = siri_wgpu::paint(
                            ui,
                            center,
                            siri_wgpu::SiriEffect::wave(clock.time, clock.level)
                                .with_opacity(opacity),
                        );
                    }
                    if processing || terminal {
                        // 思考中 1.5 速转；终态回落到 1.0 并让六点合并成中央一颗圆
                        //（Tauri `VoiceOrbStage` 的 speed / merging）。
                        let drive = siri_wgpu::SiriDrive {
                            level: 0.0,
                            resolved: 0.0,
                            speed: if processing { 1.5 } else { 1.0 },
                            warming: false,
                            warmup_ms,
                            merging: terminal,
                        };
                        let dt = ui.input(|input| input.stable_dt);
                        // 聚拢度 / 出场 hold 由时钟算（Tauri `GATHER_HOLD_S` + `merging`）。
                        let clock = siri_wgpu::tick(ui.ctx(), "capsule-siri-orb", drive, dt);
                        let effect = siri_wgpu::SiriEffect::orb(clock.time, clock.gather);
                        let _ = siri_wgpu::paint(ui, center, effect);
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(16));
                    // Siri 舞台只有出错时给文字（Tauri `VoiceOrbStage` —— 没有「思考中」
                    // 文案，也没有「已插入 N 字」，反馈全靠光效）。
                    if matches!(phase.as_str(), "failed" | "error") {
                        paint_siri_error(ui, center, state, lang);
                    }
                } else if processing {
                    // 经典 / typeless 的「思考中」：文案 + 扫光（Tauri `cap-shine`）。
                    paint_thinking_label(
                        ui,
                        center,
                        tr_l10n(lang, "capsule.thinking"),
                        egui::FontId::proportional(if typeless {
                            TYPELESS_TEXT_SIZE
                        } else {
                            17.0
                        }),
                        if typeless { TYPELESS_INK } else { theme::INK },
                        shine_cycle,
                        now,
                    );
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
                    let size = if typeless {
                        TYPELESS_TEXT_SIZE
                    } else if processing || matches!(phase.as_str(), "completed" | "done" | "idle")
                    {
                        // A terminal snapshot can briefly have no result text. Keep
                        // its thinking placeholder at the processing size until the
                        // popup is actually dismissed; never render a tiny final
                        // "thinking" frame.
                        17.0
                    } else {
                        11.0
                    };
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
                    let galley = layout::text_galley(
                        ui,
                        &state.text,
                        pill_ink,
                        if typeless { TYPELESS_TEXT_SIZE } else { 11.0 },
                        center.width().min(84.0),
                        1,
                    );
                    ui.painter().galley(
                        egui::pos2(
                            center.center().x - galley.rect.width() / 2.0,
                            center.center().y - galley.rect.height() / 2.0,
                        ),
                        galley,
                        pill_ink,
                    );
                }
            }
        });
    action
}

/// Siri 舞台的错误提示（Tauri `VoiceOrbStage` 的 `errorGlowTextStyle`）：底部居中的
/// 红字药丸；文案优先用宿主给的 `message`（例如选区润色的「未选中内容」），缺省才是
/// 通用的「出现错误」。
pub fn paint_siri_error(ui: &egui::Ui, center: egui::Rect, state: &CapsulePopupState, lang: Lang) {
    let text = if state.text.trim().is_empty() {
        tr_l10n(lang, "capsule.error").to_string()
    } else {
        state.text.clone()
    };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = center.width().clamp(1.0, 400.0);
    job.wrap.max_rows = 2;
    job.append(
        &text,
        0.0,
        egui::text::TextFormat {
            font_id: theme::medium_font(12.0),
            color: theme::ERR,
            ..Default::default()
        },
    );
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let rect = egui::Rect::from_center_size(
        egui::pos2(
            center.center().x,
            center.bottom() - 24.0 - galley.rect.height() / 2.0,
        ),
        galley.rect.size() + egui::vec2(24.0, 12.0),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12), theme::SURFACE_2);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(12),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.rect.width() / 2.0,
            rect.center().y - galley.rect.height() / 2.0,
        ),
        galley,
        theme::ERR,
    );
}

/// 经典药丸「思考中」文案 + 扫光（Tauri `cap-shine`）：把同一段文字在一条移动的高光带内
/// 用蓝色重绘一遍。带子由窄到宽分三层（越宽越淡），扫过字面 —— 等价于 Tauri 用
/// `background-clip: text` 做的高光；进入流式的头 2 秒走快周期，之后回落稳态。
fn paint_thinking_label(
    ui: &egui::Ui,
    center: egui::Rect,
    text: &str,
    font: egui::FontId,
    ink: egui::Color32,
    cycle_seconds: f64,
    now: f64,
) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), ink);
    let rect = egui::Rect::from_center_size(center.center(), galley.size());
    painter.galley(rect.min, galley, ink);

    let cycle = cycle_seconds.max(0.05);
    let progress = ((now % cycle) / cycle).clamp(0.0, 1.0) as f32;
    let span = (rect.width() * 0.5).max(8.0);
    let band_left = rect.left() - span + progress * (rect.width() + span);
    for (width, alpha) in [(span * 0.35, 235_u8), (span * 0.7, 130), (span, 60)] {
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() - 2.0),
            egui::pos2(rect.left() + width, rect.bottom() + 2.0),
        )
        .translate(egui::vec2(band_left - rect.left(), 0.0));
        let clip = band.intersect(rect);
        if clip.width() <= 0.0 {
            continue;
        }
        let shine = egui::Color32::from_rgba_unmultiplied(
            theme::BLUE.r(),
            theme::BLUE.g(),
            theme::BLUE.b(),
            alpha,
        );
        let shine_galley = painter.layout_no_wrap(text.to_string(), font.clone(), shine);
        painter
            .with_clip_rect(clip)
            .galley(rect.min, shine_galley, shine);
    }
}

/// 圆形按钮（classic 为 28×28；Typeless 按 Tauri 的 zoom 比例缩放）。
/// 确认式词库学习的确认入口（Tauri `VocabSuggestionCard`）。
///
/// 用户手改一个词之后 Core 把它攒成候选（`pending_corrections`）：「自动收集」在真机
/// 上大约五条错四条，所以每一条都要人过一眼 —— 接受写进词库（词库页的「确认收集」
/// 分段），拒绝只是丢掉，不留黑名单。
fn capsule_suggestion_card(
    ui: &mut egui::Ui,
    state: &CapsulePopupState,
    lang: Lang,
) -> CapsuleAction {
    let mut action = CapsuleAction::None;
    let stage = ui.max_rect();
    let layout = vocab_card_layout(stage, state.suggestions.len());
    let card = layout.card;
    let painter = ui.painter().with_clip_rect(card.intersect(stage));
    painter.rect_filled(card, egui::CornerRadius::same(16), theme::SURFACE);
    painter.rect_stroke(
        card,
        egui::CornerRadius::same(16),
        egui::Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let inner = card.shrink(12.0);
    let title = layout::text_galley(
        ui,
        tr_l10n(lang, "vocabCard.title"),
        theme::INK_3,
        11.0,
        inner.width(),
        1,
    );
    painter.galley(inner.min, title, theme::INK_3);
    for (index, suggestion) in state.suggestions.iter().enumerate() {
        let Some(row) = layout.rows.get(index) else {
            break;
        };
        let text_rect = row.text;
        let (reject_rect, accept_rect) = (row.reject, row.accept);
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = text_rect.width();
        job.wrap.max_rows = 1;
        let mono = egui::FontId::new(12.5, egui::FontFamily::Monospace);
        // 改前（暗）→ 改后（亮）：一眼能看出这次手改把什么改成了什么。
        job.append(
            &suggestion.pattern,
            0.0,
            egui::text::TextFormat {
                font_id: mono.clone(),
                color: theme::INK_4,
                ..Default::default()
            },
        );
        job.append(
            " \u{2192} ",
            0.0,
            egui::text::TextFormat {
                font_id: mono.clone(),
                color: theme::INK_4,
                ..Default::default()
            },
        );
        job.append(
            &suggestion.replacement,
            0.0,
            egui::text::TextFormat {
                font_id: mono,
                color: theme::INK,
                ..Default::default()
            },
        );
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        painter.with_clip_rect(text_rect).galley(
            egui::pos2(
                text_rect.left(),
                text_rect.center().y - galley.rect.height() / 2.0,
            ),
            galley,
            theme::INK,
        );
        let reject = ui.interact(
            reject_rect,
            ui.id().with(("openless-vocab-reject", index)),
            egui::Sense::click(),
        );
        let icon = (reject_rect.width() * 0.46).clamp(9.0, 13.0);
        round_button(
            ui,
            reject_rect,
            icons::IconName::Close,
            reject.hovered(),
            theme::SURFACE_2,
            theme::INK_2,
            (0.8, icon),
        );
        if reject.clicked() {
            action = CapsuleAction::RejectSuggestion(suggestion.id.clone());
        }
        let accept = ui.interact(
            accept_rect,
            ui.id().with(("openless-vocab-accept", index)),
            egui::Sense::click(),
        );
        round_button(
            ui,
            accept_rect,
            icons::IconName::Check,
            accept.hovered(),
            theme::BLUE_SOFT,
            theme::BLUE,
            (0.8, icon),
        );
        if accept.clicked() {
            action = CapsuleAction::AcceptSuggestion(suggestion.id.clone());
        }
    }
    action
}

fn round_button(
    ui: &egui::Ui,
    rect: egui::Rect,
    icon: icons::IconName,
    hovered: bool,
    fill: egui::Color32,
    ink: egui::Color32,
    button_metrics: (f32, f32),
) {
    let (stroke_width, icon_size) = button_metrics;
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
        egui::Stroke::new(stroke_width, theme::LINE),
    );
    icons::draw_icon_sized(ui, rect.center(), icon, ink, icon_size);
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
    let scale = if bar_count > CLASSIC_ENVELOPE.len() {
        TYPELESS_SCALE
    } else {
        1.0
    };
    let base = 2.0 * scale;
    let max = if bar_count > CLASSIC_ENVELOPE.len() {
        28.0 * scale
    } else {
        24.0
    };
    let voice = level.clamp(0.0, 1.0);
    let gated = ((voice - 0.012) / (0.34 - 0.012)).clamp(0.0, 1.0);
    let eased = gated * gated * (3.0 - 2.0 * gated);
    let visual = eased.powf(0.42);
    let bar_width = 3.0 * scale;
    let gap = 3.0 * scale;
    let total = envelope.len() as f32 * bar_width + (envelope.len() - 1) as f32 * gap;
    let mut x = rect.center().x - total / 2.0;
    for envelope in envelope {
        let height = base + (max - base) * visual * envelope;
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
    root_ui: &mut egui::Ui,
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
        .show(root_ui, |ui| {
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
                    .frame(egui::Frame::NONE)
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
    fn run(size: egui::Vec2, mut render: impl FnMut(&mut egui::Ui) -> String) -> String {
        // Every popup test renders the same frontend as the GPU-state tests, so
        // they share the process-global effect flags and must not run in parallel.
        let ctx = egui::Context::default();
        let mut painted = String::new();
        for _ in 0..2 {
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| {
                    let _ = render(ui);
                },
            );
            painted = painted_text(&output);
        }
        painted
    }

    /// 渲染胶囊并把结果动作交出来；`click` 给一个坐标时会合成一次点击（按下+抬起
    /// 同一帧，和真实点击等价）。
    fn run_capsule(
        state: &CapsulePopupState,
        size: egui::Vec2,
        click: Option<egui::Pos2>,
    ) -> (CapsuleAction, String) {
        let ctx = egui::Context::default();
        let mut action = CapsuleAction::None;
        let mut painted = String::new();
        for frame in 0..2 {
            let mut events = Vec::new();
            if frame == 1 {
                if let Some(pos) = click {
                    for pressed in [true, false] {
                        events.push(egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                }
            }
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| {
                    action = dictation_capsule(ui, state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                },
            );
            painted = painted_text(&output);
        }
        (action, painted)
    }

    /// 「要记住这个词吗？」卡片：有候选时整窗只画卡片（没有药丸），点「记住」把候选 id
    /// 发回宿主，点「不用」发拒绝 —— 这就是确认式词库学习的确认入口（Tauri
    /// `VocabSuggestionCard` + `PendingCorrection`）。
    #[test]
    fn the_vocab_card_asks_before_remembering() {
        let state = CapsulePopupState {
            phase: String::new(),
            suggestions: vec![
                openless_linux_egui::CapsuleSuggestion {
                    id: "first".to_string(),
                    pattern: "banana".to_string(),
                    replacement: "bananas".to_string(),
                },
                openless_linux_egui::CapsuleSuggestion {
                    id: "second".to_string(),
                    pattern: "teh".to_string(),
                    replacement: "the".to_string(),
                },
            ],
            ..Default::default()
        };
        let size = egui::vec2(460.0, 180.0);
        let (action, painted) = run_capsule(&state, size, None);
        assert!(
            matches!(action, CapsuleAction::None),
            "rendering alone must not resolve anything"
        );
        assert!(
            painted.contains(tr_l10n(Lang::ZhCn, "vocabCard.title")),
            "the card must ask the question: {painted}"
        );
        for text in ["banana", "bananas", "teh", "the"] {
            assert!(painted.contains(text), "row text {text} missing: {painted}");
        }
        // 每行两颗圆钮都在（图标画的是 Check / Close）。
        assert_eq!(
            painted
                .matches(tr_l10n(Lang::ZhCn, "vocabCard.title"))
                .count(),
            1,
            "the title is painted once"
        );
        let layout = vocab_card_layout(
            egui::Rect::from_min_size(egui::Pos2::ZERO, size),
            state.suggestions.len(),
        );
        assert_eq!(layout.rows.len(), 2, "one row per suggestion");
        // 两行不重叠：第二行在第一行下面。
        assert!(
            layout.rows[0].accept.bottom() <= layout.rows[1].accept.top(),
            "rows must not overlap"
        );
        let (action, _) = run_capsule(&state, size, Some(layout.rows[0].accept.center()));
        assert!(
            matches!(&action, CapsuleAction::AcceptSuggestion(id) if id == "first"),
            "accepting the first row must carry its id, got {action:?}"
        );
        let (action, _) = run_capsule(&state, size, Some(layout.rows[1].reject.center()));
        assert!(
            matches!(&action, CapsuleAction::RejectSuggestion(id) if id == "second"),
            "rejecting the second row must carry its id, got {action:?}"
        );
    }

    /// 行数拉满（Core 上限 5 条）时卡片仍然整张落在舞台里，不露在窗外。
    #[test]
    fn a_full_vocab_card_still_fits_the_capsule_stage() {
        let stage = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(460.0, 180.0));
        let layout = vocab_card_layout(stage, 5);
        assert_eq!(layout.rows.len(), 5, "every candidate gets a row");
        assert!(
            stage.contains_rect(layout.card),
            "the card must stay inside the stage: {:?}",
            layout.card
        );
        for (index, row) in layout.rows.iter().enumerate() {
            assert!(
                layout.card.contains_rect(row.accept) && layout.card.contains_rect(row.reject),
                "row {index} buttons must stay inside the card"
            );
            assert!(
                row.text.right() <= row.reject.left(),
                "row {index} text must not overlap the buttons"
            );
        }
        assert!(
            layout
                .rows
                .windows(2)
                .all(|pair| pair[0].accept.bottom() <= pair[1].accept.top()),
            "rows must not overlap at the cap"
        );
        // 压缩后按钮也不能叠上去。
        assert!(
            layout.rows.iter().all(|row| row.accept.height() >= 14.0),
            "buttons must stay tappable when the rows are compressed"
        );
    }

    /// 与 `run` 同款流程，但把最后一帧的 `FullOutput` 交出来（要按形状断言时用）。
    fn run_output(
        size: egui::Vec2,
        mut render: impl FnMut(&mut egui::Ui) -> String,
    ) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let mut last = None;
        for _ in 0..2 {
            last = Some(crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| {
                    let _ = render(ui);
                },
            ));
        }
        last.expect("at least one pass")
    }

    /// 面板画出的分隔线（`hairline` = 一条 `LINE_SOFT` 细线），返回 (y, x0, x1)。
    fn hairlines(output: &egui::FullOutput) -> Vec<(f32, f32, f32)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(f32, f32, f32)>) {
            match shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == theme::LINE_SOFT && stroke.width <= 1.0 =>
                {
                    out.push((points[0].y, points[0].x, points[1].x));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// 润色结果模式必须沿用选区助手面板的外壳：同一套头/尾分隔线。
    ///
    /// 回归背景：合并之后 egui 侧曾自己一套排版（没有分隔线、没有 14px 内容
    /// 边距），看着就像已经下线的独立预览窗口。
    #[test]
    fn the_polish_result_reuses_the_selection_ask_chrome() {
        let polish = QaPopupState {
            phase: "idle".to_string(),
            polish: Some(QaPolishState {
                text: "polished text".to_string(),
                source: "source paragraph".to_string(),
            }),
            ..Default::default()
        };
        let ask = QaPopupState {
            phase: "idle".to_string(),
            ..Default::default()
        };
        let mut polish_composer = String::new();
        let mut ask_composer = String::new();
        let polish_out = run_output(egui::vec2(420.0, 540.0), |ctx| {
            selection_ask(ctx, &polish, &mut polish_composer, Lang::ZhCn, None);
            String::new()
        });
        let ask_out = run_output(egui::vec2(420.0, 540.0), |ctx| {
            selection_ask(ctx, &ask, &mut ask_composer, Lang::ZhCn, None);
            String::new()
        });
        let polish_lines = hairlines(&polish_out);
        let ask_lines = hairlines(&ask_out);
        assert!(
            polish_lines.len() >= 2,
            "the polish mode must paint header + footer separators, got {:?}",
            polish_lines
        );
        // 第一条（头部下方那条）必须与选区助手面板的位置与宽度一致。
        let (py, px0, px1) = polish_lines[0];
        let (ay, ax0, ax1) = ask_lines[0];
        assert!(
            (py - ay).abs() < 0.5 && (px0 - ax0).abs() < 0.5 && (px1 - ax1).abs() < 0.5,
            "the header separator must match the selection-ask panel: polish {:?} vs ask {:?}",
            polish_lines[0],
            ask_lines[0]
        );
    }

    /// 合并后：润色结果就画在**选区助手面板**里（同一弹窗的第二套 UI），
    /// 原独立预览窗口的文案与动作必须一模一样地出现。
    #[test]
    fn the_polish_result_renders_inside_the_ask_panel() {
        let state = QaPopupState {
            phase: "idle".to_string(),
            polish: Some(QaPolishState {
                text: "polished text".to_string(),
                source: "source paragraph".to_string(),
            }),
            ..Default::default()
        };
        let mut composer = String::new();
        let painted = run(egui::vec2(420.0, 540.0), |ctx| {
            let action = selection_ask(ctx, &state, &mut composer, Lang::ZhCn, None);
            assert_eq!(action, QaAction::None);
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
                "the polish mode must paint {expected:?} inside the ask panel\n{painted}"
            );
        }
    }

    /// 润色结果**只读**：签名拿的是 `&QaPolishState`（根本改不动），框里必须
    /// 把结果原文画出来——「确认并替换」（即用户说的「插入」）写回的就是这段原文。
    #[test]
    fn the_polish_result_is_read_only_and_paints_the_text() {
        let state = QaPolishState {
            text: "polished result text".to_string(),
            source: "source paragraph".to_string(),
        };
        let painted = run(egui::vec2(420.0, 540.0), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert_eq!(
                    polish_result_mode(ui, &state, Lang::ZhCn),
                    PolishResultAction::None
                );
            });
            String::new()
        });
        assert!(
            has(&painted, "polished result text"),
            "the read-only result box must paint the polished text\n{painted}"
        );
        assert!(
            has(&painted, "source paragraph"),
            "the source summary must stay visible\n{painted}"
        );
        assert_eq!(state.text, "polished result text");
    }

    /// 润色结果模式下，面板的其余部分（提问对话、输入框、麦克风）不得出现：
    /// 同一个面板的两套 UI 不能同时画。
    #[test]
    fn the_polish_result_mode_replaces_the_ask_conversation() {
        let state = QaPopupState {
            phase: "idle".to_string(),
            polish: Some(QaPolishState {
                text: "polished text".to_string(),
                source: String::new(),
            }),
            ..Default::default()
        };
        let mut composer = String::new();
        let painted = run(egui::vec2(420.0, 540.0), |ctx| {
            selection_ask(ctx, &state, &mut composer, Lang::ZhCn, None);
            String::new()
        });
        assert!(
            !has(&painted, tr_l10n(Lang::ZhCn, "qa.composer_placeholder")),
            "the ask composer must not be painted in polish mode\n{painted}"
        );
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
            polish: None,
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

    /// Recording and thinking each queue exactly one WGPU centre effect. Terminal
    /// states must not leave a callback behind while the popup waits to dismiss.
    #[test]
    fn capsule_queues_only_the_active_wgpu_centre_effect() {
        let frame = |state: CapsulePopupState| {
            let ctx = egui::Context::default();
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(200.0, 100.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let _ = dictation_capsule(ui, &state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                },
            );
            let callbacks = output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
                .count();
            (callbacks,)
        };
        for (label, state, wants_centre) in [
            (
                "recording = one WGPU wave, no perimeter ring",
                CapsulePopupState {
                    phase: "recording".into(),
                    audio_level: Some(0.2),
                    ..Default::default()
                },
                true,
            ),
            (
                "thinking = one WGPU orb, no perimeter ring",
                CapsulePopupState {
                    phase: "transcribing".into(),
                    ..Default::default()
                },
                true,
            ),
            (
                "terminal Siri capsule keeps the orb for the merge-out",
                CapsulePopupState {
                    phase: "inserted".into(),
                    text: "hello".into(),
                    ..Default::default()
                },
                true,
            ),
            (
                "classic capsule never paints a WGPU effect",
                CapsulePopupState {
                    phase: "polishing".into(),
                    style: "classic".into(),
                    suggestions: Vec::new(),
                    ..Default::default()
                },
                false,
            ),
        ] {
            let (callbacks,) = frame(state);
            assert_eq!(callbacks, usize::from(wants_centre), "{label}");
        }
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
            dictation_capsule(ctx, &idle, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
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
            dictation_capsule(ctx, &translating, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
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
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(520.0, 520.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    selection_ask(ui, &state, &mut composer, Lang::ZhCn, Some(&texture));
                },
            );
            painted = painted_text(&output);
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

    /// 文本的声明颜色（扫光靠重绘另一份 galley 实现，所以要看 Text 的 section 颜色）。
    fn painted_text_colors(shape: &egui::Shape, out: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Text(text) => {
                for section in &text.galley.job.sections {
                    out.push(section.format.color);
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    painted_text_colors(shape, out);
                }
            }
            _ => {}
        }
    }

    /// 偏蓝 = 扫光那一遍（INK 是中性灰，TYPELESS 是白）。
    fn is_bluish(color: egui::Color32) -> bool {
        color.b() > color.r().saturating_add(24) && color.b() > color.g()
    }

    /// 经典药丸的「思考中」扫光（Tauri `cap-shine`）：高光带扫过字面时，文字会被蓝色
    /// 重绘一遍；Siri 舞台没有这段文字，自然也不会有。
    #[test]
    fn the_classic_thinking_label_sweeps_a_shine_band() {
        let collect = |time: f64, style: &str| {
            let ctx = egui::Context::default();
            let state = CapsulePopupState {
                phase: "Polishing".into(),
                style: style.into(),
                suggestions: Vec::new(),
                ..Default::default()
            };
            let mut colors = Vec::new();
            // 两帧：第一帧确定相位起点，第二帧才是被测的那一帧。
            for frame in [0.0, time] {
                let output = crate::ui::frontend::run_pass(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(200.0, 60.0),
                        )),
                        time: Some(frame),
                        ..Default::default()
                    },
                    |ui| {
                        let _ =
                            dictation_capsule(ui, &state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                    },
                );
                colors.clear();
                for clipped in &output.shapes {
                    painted_text_colors(&clipped.shape, &mut colors);
                }
            }
            colors
        };
        // 高速 burst 周期 0.9s：t=0.45 时高光带正扫到字面中部。
        assert!(
            collect(0.45, "classic").iter().copied().any(is_bluish),
            "the sweep must repaint the label in blue"
        );
        // 慢速阶段（>2s）同样扫，只是周期变长。
        assert!(
            collect(2.7, "classic").iter().copied().any(is_bluish),
            "the sweep must keep running after the initial burst"
        );
        // Siri 舞台没有这段文案。
        assert!(
            !collect(0.45, "siri").iter().copied().any(is_bluish),
            "the Siri stage has no caption to sweep"
        );
    }

    /// Siri 舞台出错时给的是宿主的具体文案（Tauri `message || t('capsule.error')`），
    /// 不是笼统的「出现错误」。
    #[test]
    fn the_siri_stage_prefers_the_host_error_message() {
        let message = tr_l10n(Lang::ZhCn, "capsule.selectionPolish.noSelection");
        let failed = CapsulePopupState {
            phase: "Failed".to_string(),
            text: message.to_string(),
            style: "siri".to_string(),
            suggestions: Vec::new(),
            ..Default::default()
        };
        let painted = run(egui::vec2(460.0, 180.0), |ctx| {
            dictation_capsule(ctx, &failed, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(has(&painted, message), "{painted}");
        assert!(
            !has(&painted, tr_l10n(Lang::ZhCn, "capsule.error")),
            "the generic error text must not replace the host message: {painted}"
        );
        // 没有具体文案时才是通用文案。
        let bare = CapsulePopupState {
            text: String::new(),
            ..failed.clone()
        };
        let painted = run(egui::vec2(460.0, 180.0), |ctx| {
            dictation_capsule(ctx, &bare, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "capsule.error")),
            "{painted}"
        );
    }

    /// wave → orb 的交叉淡出（Tauri `opacity .6s ease-out .55s`）：刚切到思考态时波形
    /// 还在（两个回调），淡出之后只剩圆点环。
    #[test]
    fn the_wave_cross_fades_into_the_orb_after_recording() {
        let ctx = egui::Context::default();
        let stage = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(460.0, 180.0));
        let render = |time: f64, phase: &str| {
            let state = CapsulePopupState {
                phase: phase.into(),
                audio_level: Some(0.4),
                style: "siri".into(),
                suggestions: Vec::new(),
                ..Default::default()
            };
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(stage),
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    let _ = dictation_capsule(ui, &state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                },
            );
            output
                .shapes
                .iter()
                .filter(|clipped| matches!(clipped.shape, egui::Shape::Callback(_)))
                .count()
        };
        assert_eq!(
            render(0.0, "recording"),
            1,
            "recording paints the wave only"
        );
        assert_eq!(
            render(0.2, "polishing"),
            2,
            "right after the switch the wave must still be fading over the orb"
        );
        assert_eq!(
            render(2.0, "polishing"),
            1,
            "once the fade is over only the orb remains"
        );
    }

    /// Render one capsule frame and collect the colours, the GPU callback count
    /// and the number of centre effect strokes (the CPU wave lines).
    fn capsule_frame(state: &CapsulePopupState) -> (Vec<egui::Color32>, usize, usize) {
        let ctx = egui::Context::default();
        let mut colors = Vec::new();
        let mut callbacks = 0;
        let mut effect = 0;
        for _ in 0..2 {
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(200.0, 100.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let _ = dictation_capsule(ui, state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                },
            );
            colors.clear();
            callbacks = 0;
            effect = 0;
            for clipped in &output.shapes {
                painted_colors(&clipped.shape, &mut colors);
                count_centre_effect(&clipped.shape, &mut effect);
                if matches!(clipped.shape, egui::Shape::Callback(_)) {
                    callbacks += 1;
                }
            }
        }
        (colors, callbacks, effect)
    }

    /// Classic fallback primitives still expose a centre visual for tests; Siri
    /// states are represented by one WGPU callback instead.
    fn count_centre_effect(shape: &egui::Shape, out: &mut usize) {
        match shape {
            egui::Shape::Path(path) if path.points.len() >= 8 => *out += 1,
            egui::Shape::Circle(_) => *out += 1,
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    count_centre_effect(shape, out);
                }
            }
            _ => {}
        }
    }

    /// Whether a colour reads as the OpenLess error red (the old ring tint).
    fn is_reddish(color: egui::Color32) -> bool {
        color.a() > 40 && color.r() > 150 && color.g() < 110 && color.b() < 110
    }

    #[test]
    fn recording_capsule_paints_no_coloured_outline() {
        // Tauri 的经典药丸只有 1px 中性描边（Capsule.tsx：border 1px
        // var(--ol-capsule-pill-border)）；录音音量只驱动中心波形。
        // 外圈红/黑扫光是本仓自己加的，用户报「语音输入弹窗有一个红边」——
        // 这条测试锁死它不许回来。
        for phase in ["Recording", "Transcribing", "Polishing"] {
            let state = CapsulePopupState {
                phase: phase.to_string(),
                text: String::new(),
                audio_level: Some(0.6),
                translation_active: false,
                style: "classic".to_string(),
                suggestions: Vec::new(),
            };
            let (colors, _, _) = capsule_frame(&state);
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
            suggestions: Vec::new(),
        };
        let (colors, callbacks, effect) = capsule_frame(&state);
        // 音量竖条是 3px 宽的小圆角矩形：数一下细长条形的填充个数。
        let fills = colors.iter().filter(|color| color.a() > 0).count();
        assert!(
            callbacks > 0 || fills >= 6 || effect > 0,
            "recording capsule must keep the centre visual \
             (callbacks={callbacks}, fills={fills}, effect={effect})"
        );
    }

    /// 胶囊在 layer-shell 表面上**只有指针输入**（层表面拿不到键盘与输入法），
    /// 所以「两个圆钮必须可点」是它唯一能用的交互。这条测试把指针按到 ✕ / ✓
    /// 的圆心，必须分别返回 Cancel / Confirm。
    #[test]
    fn the_capsule_buttons_report_cancel_and_confirm() {
        let size = egui::vec2(460.0, 180.0);
        // 与渲染同源的几何：药丸 176×42，水平居中、距底 16；圆钮 28、内缩 8。
        let pill_left = size.x / 2.0 - PILL_WIDTH / 2.0;
        let pill_right = pill_left + PILL_WIDTH;
        let centre_y = size.y - CAPSULE_BOTTOM_INSET - PILL_HEIGHT / 2.0;
        let cancel = egui::pos2(pill_left + 8.0 + ROUND_BUTTON / 2.0, centre_y);
        let confirm = egui::pos2(pill_right - 8.0 - ROUND_BUTTON / 2.0, centre_y);
        for (position, expected) in [
            (cancel, CapsuleAction::Cancel),
            (confirm, CapsuleAction::Confirm),
        ] {
            let ctx = egui::Context::default();
            let state = CapsulePopupState {
                phase: "Recording".to_string(),
                audio_level: Some(0.4),
                style: "classic".to_string(),
                suggestions: Vec::new(),
                ..Default::default()
            };
            let mut action = CapsuleAction::None;
            // 三帧：第一帧布局，第二帧按下，第三帧松开（egui 的点击需要成对事件）。
            for events in [
                vec![egui::Event::PointerMoved(position)],
                vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            ] {
                let _ = crate::ui::frontend::run_pass(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        action =
                            dictation_capsule(ui, &state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                    },
                );
            }
            assert_eq!(
                action, expected,
                "clicking {position:?} must report {expected:?}"
            );
        }
    }

    /// 经典药丸的「思考中」文案必须本地化并在每一个处理相位都在；Siri 舞台反过来 ——
    /// 它没有文案（Tauri `VoiceOrbStage` 只有 error 才给字），反馈全靠光效。
    #[test]
    fn classic_processing_caption_is_localized_and_siri_has_none() {
        for lang in [Lang::ZhCn, Lang::En] {
            for phase in ["starting", "transcribing", "polishing", "inserting"] {
                let classic = CapsulePopupState {
                    phase: phase.into(),
                    style: "classic".into(),
                    suggestions: Vec::new(),
                    ..Default::default()
                };
                let painted = run(egui::vec2(200.0, 60.0), |ui| {
                    dictation_capsule(ui, &classic, lang, siri_wgpu::DEFAULT_WARMUP_MS);
                    String::new()
                });
                assert!(
                    has(&painted, tr_l10n(lang, "capsule.thinking")),
                    "{phase} must show the localized processing caption: {painted}"
                );
                let siri = CapsulePopupState {
                    style: "siri".into(),
                    suggestions: Vec::new(),
                    ..classic.clone()
                };
                let painted = run(egui::vec2(460.0, 180.0), |ui| {
                    dictation_capsule(ui, &siri, lang, siri_wgpu::DEFAULT_WARMUP_MS);
                    String::new()
                });
                assert!(
                    !has(&painted, tr_l10n(lang, "capsule.thinking")),
                    "the Siri stage must not paint a caption (Tauri parity): {painted}"
                );
            }
        }
    }

    #[test]
    fn siri_recording_and_thinking_use_the_full_stage() {
        for phase in ["recording", "transcribing"] {
            let ctx = egui::Context::default();
            let stage = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(460.0, 180.0));
            let state = CapsulePopupState {
                phase: phase.into(),
                style: "siri".into(),
                suggestions: Vec::new(),
                ..Default::default()
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(stage),
                    ..Default::default()
                },
                |ui| {
                    dictation_capsule(ui, &state, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
                },
            );
            assert!(
                output.shapes.iter().any(|shape| matches!(
                    &shape.shape, egui::Shape::Callback(callback) if callback.rect == stage
                )),
                "{phase} must not shrink the Siri callback into the classic pill"
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn capsule_paints_state_specific_content() {
        let recording = CapsulePopupState {
            phase: "Recording".to_string(),
            text: String::new(),
            audio_level: Some(0.4),
            translation_active: false,
            style: "siri".to_string(),
            suggestions: Vec::new(),
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &recording, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(
            !painted.contains(tr_l10n(Lang::ZhCn, "capsule.thinking")),
            "recording capsule shows level bars, not the thinking label: {painted}"
        );

        // classic 样式：中心走 Tauri 的竖条/文字（`use_gpu` 为假）。
        let transcribing = CapsulePopupState {
            phase: "Transcribing".to_string(),
            style: "classic".to_string(),
            suggestions: Vec::new(),
            ..Default::default()
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &transcribing, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "capsule.thinking")),
            "{painted}"
        );

        // Siri 舞台没有文案：光点换乘「收尾合并」，字都交给光效（Tauri `VoiceOrbStage`）。
        let transcribing_siri = CapsulePopupState {
            phase: "Transcribing".to_string(),
            style: "siri".to_string(),
            suggestions: Vec::new(),
            ..Default::default()
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(
                ctx,
                &transcribing_siri,
                Lang::ZhCn,
                siri_wgpu::DEFAULT_WARMUP_MS,
            );
            String::new()
        });
        assert!(
            !has(&painted, tr_l10n(Lang::ZhCn, "capsule.thinking")),
            "siri transcribing must not paint a caption: {painted}"
        );

        // 「已插入 N 字」是经典药丸的文案；Siri 舞台不画它。
        let done = CapsulePopupState {
            phase: "Completed".to_string(),
            text: inserted_message(Lang::ZhCn, 12),
            audio_level: None,
            translation_active: false,
            style: "siri".to_string(),
            suggestions: Vec::new(),
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &done, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(
            !has(&painted, "12"),
            "the Siri stage never paints the inserted-chars text: {painted}"
        );
        let done_classic = CapsulePopupState {
            style: "classic".to_string(),
            suggestions: Vec::new(),
            ..done.clone()
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &done_classic, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(has(&painted, "12"), "{painted}");

        let failed = CapsulePopupState {
            phase: "Failed".to_string(),
            text: String::new(),
            audio_level: None,
            translation_active: false,
            style: "siri".to_string(),
            suggestions: Vec::new(),
        };
        let painted = run(egui::vec2(200.0, 60.0), |ctx| {
            dictation_capsule(ctx, &failed, Lang::ZhCn, siri_wgpu::DEFAULT_WARMUP_MS);
            String::new()
        });
        assert!(
            has(&painted, tr_l10n(Lang::ZhCn, "capsule.error")),
            "{painted}"
        );
    }
}
