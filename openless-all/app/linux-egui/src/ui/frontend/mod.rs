pub mod corrections;
pub mod format;
pub mod history;
pub mod icons;
pub mod layout;
pub mod marketplace;
pub mod overview;
pub mod pages;
pub mod selection_ask;
pub mod settings;
pub mod translation;
pub mod view_model;
pub mod vocab;

use eframe::egui;
use view_model::{FrontendAction, FrontendViewModel, Page};

/// Re-export the theme module from the parent ui module.
pub use super::theme;

/// Render the complete egui frontend for one frame. This is the single entry
/// point called from `OpenLessEguiApp::update`. It replaces the old
/// `shell::titlebar` + `shell::sidebar` + `shell::content_panel` calls.
///
/// The frontend is a pure function of `ctx` and `vm` — it reads display state
/// from the view model and pushes user actions into the `actions` vec. The host
/// drains actions after this call and dispatches them to existing Core/backend
/// methods.
pub fn render(ctx: &egui::Context, vm: &mut FrontendViewModel, actions: &mut Vec<FrontendAction>) {
    // Paint the rounded window surface and body canvas.
    layout::paint_window_background(ctx);

    // Titlebar with window controls.
    layout::titlebar(ctx, actions);

    // Sidebar with navigation.
    layout::sidebar(ctx, vm, actions);

    // Resize handles for borderless window.
    layout::resize_handles(ctx);

    // Content area.
    layout::content_panel(ctx, |ui| {
        let body = layout::body_rect(ctx);

        // The Overview dashboard is a single-screen fixed page: it fills the
        // height the shell gives it and manages its own internal scrolling, so
        // it must not be wrapped in the shared page scroll area.
        if vm.active_page == Page::Overview {
            overview::page(ui, vm, actions);
            return;
        }

        // Style page owns its own layout (a full-height card) and header.
        if vm.active_page == Page::Style {
            pages::style_page(ui, vm, actions);
            ui.add_space(32.0);
            return;
        }

        // History owns its own two-column layout (list and detail scroll
        // independently), so it is not wrapped in the shared scroll area.
        if vm.active_page == Page::History {
            history::page(ui, vm, actions);
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("openless-main-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match vm.active_page {
                    Page::Vocab => {
                        vocab::page(ui, vm, actions);
                    }
                    Page::Marketplace => {
                        marketplace::marketplace_page(ui, vm, actions, body);
                    }
                    Page::SelectionAsk => {
                        selection_ask::page(ui, vm, actions);
                    }
                    Page::Translation => {
                        translation::page(ui, vm, actions);
                    }
                    Page::Corrections => {
                        corrections::page(ui, vm, actions);
                    }
                    Page::Overview | Page::History | Page::Style | Page::Settings => {
                        // Handled above or via overlay.
                    }
                }
                ui.add_space(32.0);
            });

        // Settings overlay (rendered on top of everything).
        if vm.settings_open {
            settings::settings_overlay(ctx, vm, actions, body);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1240.0, 800.0))
    }

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<FrontendAction> {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            events,
            ..Default::default()
        });
        let mut vm = FrontendViewModel::default();
        let mut actions = Vec::new();
        render(ctx, &mut vm, &mut actions);
        let _ = ctx.end_pass();
        actions
    }

    #[test]
    fn overview_page_renders_populated_dashboard_without_panicking() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            overview_loading: false,
            ..Default::default()
        };
        vm.settings.activity_heatmap = true;
        let heatmap = (0..365)
            .map(|index| super::view_model::OverviewHeatmapDay {
                date: format!("2026-{:02}-{:02}", index / 31 + 1, index % 31 + 1),
                count: (index % 4) as u32,
            })
            .collect();
        let activity_daily = (0..30)
            .map(|index| super::view_model::OverviewActivityDay {
                date: format!("2026-01-{:02}", index + 1),
                count: index as u32,
                chars: (index * 12) as u64,
                duration_ms: (index * 900) as u64,
            })
            .collect();
        vm.overview = Some(super::view_model::OverviewSummary {
            asr_provider: "volcengine".into(),
            llm_provider: "ark".into(),
            asr_configured: true,
            llm_configured: true,
            chars_today: 1234,
            segments_today: 7,
            duration_ms_today: 45_000,
            avg_latency_ms: 6_400,
            history_total: 9,
            recent: (0..5)
                .map(|index| super::view_model::OverviewRecentEntry {
                    created_at: "2026-01-15T12:34:00+00:00".into(),
                    final_text: format!("recent item {index}"),
                    raw_transcript: "raw transcript".into(),
                    mode: super::view_model::OverviewMode::Raw,
                    duration_ms: Some(3_100),
                })
                .collect(),
            activity_daily,
            heatmap_year: 2026,
            heatmap,
        });

        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
        }

        // One more pass whose painted text we inspect: this is the end-to-end
        // check that the localized dashboard chrome actually reaches the painter.
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = ctx.end_pass();
        let painted = painted_text(&output);
        // Expected labels are read back from the catalog so the test cannot
        // drift from the keys the page actually uses (and stays free of raw
        // CJK literals, as the localization contract requires).
        for key in [
            "overview.title",
            "overview.refresh",
            "overview.stats_title",
            "overview.metric_chars",
            "overview.metric_duration",
            "overview.metric_avg",
            "overview.metric_total",
            "overview.period_last7",
            "overview.period_last30",
            "overview.metric_count",
            "overview.metric_chars_name",
            "overview.metric_duration_name",
            "overview.recent_title",
            "overview.recent_all",
            "overview.activity_title",
            "overview.mode_raw",
            "nav.overview",
            "nav.history",
            "nav.vocab",
            "nav.group_style",
            "nav.group_tools",
            "nav.translation",
            "nav.selection_ask",
            "nav.corrections",
            "nav.settings",
        ] {
            let expected = openless_linux_egui::tr_l10n(openless_linux_egui::Lang::ZhCn, key);
            assert!(
                painted.contains(expected),
                "expected the overview dashboard to paint {key} ({expected:?})"
            );
        }
    }

    fn painted_text(output: &egui::FullOutput) -> String {
        fn collect(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push_str(text.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = String::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn history_page_renders_populated_state_without_panicking() {
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::History,
            history_loading: false,
            ..Default::default()
        };
        vm.history_entries = vec![
            super::view_model::HistoryEntry {
                id: "a".into(),
                created_at: "2026-01-15T12:34:00+00:00".into(),
                mode: super::view_model::OverviewMode::Raw,
                style_label: "raw".into(),
                raw_transcript: "raw transcript of the first entry".into(),
                final_text: String::new(),
                duration_ms: Some(3_100),
                insert_status: super::view_model::HistoryInsertStatus::Failed,
                has_audio: true,
                asr_provider: Some("zhipu".into()),
                asr_model: Some("glm-asr-2512".into()),
                asr_ms: Some(465),
                llm_provider: None,
                llm_model: None,
                polish_ms: None,
                app_name: Some("OpenLess".into()),
                dictionary_count: Some(2),
            },
            super::view_model::HistoryEntry {
                id: "b".into(),
                created_at: "2026-01-15T11:00:00+00:00".into(),
                mode: super::view_model::OverviewMode::Light,
                style_label: "light".into(),
                raw_transcript: "second raw".into(),
                final_text: "second polished text".into(),
                duration_ms: Some(2_400),
                insert_status: super::view_model::HistoryInsertStatus::Inserted,
                has_audio: false,
                ..Default::default()
            },
        ];

        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
        }
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = ctx.end_pass();
        let painted = painted_text(&output);
        let tr = |key: &'static str| openless_linux_egui::tr_l10n(zh, key);
        for key in [
            "history.title",
            "history.desc",
            "common.refresh",
            "common.clear",
            "history.raw_label",
            "history.play",
            "history.export",
            "history.retranscribe",
            "history.step_asr",
            "history.step_polish",
            "history.step_insert",
            "common.copy",
            "common.delete",
        ] {
            let expected = tr(key);
            assert!(
                painted.contains(expected),
                "expected the history page to paint {key} ({expected:?})"
            );
        }
        // Formatted entries are checked on their substituted form: the first
        // entry has an empty final text but two dictionary hits.
        let chars = openless_linux_egui::fmt_l10n(zh, "history.chars", &[&0]);
        let hits = openless_linux_egui::fmt_l10n(zh, "history.vocab_hits", &[&2]);
        let insert_detail = format!("OpenLess · {chars} · {hits}");
        assert!(
            painted.contains(&insert_detail),
            "expected {insert_detail:?}"
        );
        let placeholder =
            openless_linux_egui::fmt_l10n(zh, "history.search_placeholder", &[&"Ctrl+K"]);
        assert!(painted.contains(&placeholder), "expected {placeholder:?}");
        // The detail panel shows the ASR step and its millisecond timing.
        let ms = openless_linux_egui::fmt_l10n(zh, "dur.ms", &[&465]);
        assert!(painted.contains(&ms), "expected {ms:?}");

        // Confirm dialog renders on top when a destructive action is pending.
        vm.history_confirm = Some(super::view_model::HistoryConfirm::Clear);
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let _ = ctx.end_pass();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = ctx.end_pass();
        let painted = painted_text(&output);
        let confirm_msg = openless_linux_egui::fmt_l10n(
            zh,
            "history.confirm_clear",
            &[&vm.history_entries.len()],
        );
        assert!(painted.contains(&confirm_msg), "expected {confirm_msg:?}");
        assert!(painted.contains(tr("common.cancel")));
        assert!(painted.contains(tr("common.confirm")));
    }

    #[test]
    fn settings_overlay_lists_every_section() {
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        for section in [
            super::view_model::SettingsSection::General,
            super::view_model::SettingsSection::Shortcuts,
            super::view_model::SettingsSection::Appearance,
            super::view_model::SettingsSection::Services,
            super::view_model::SettingsSection::Privacy,
            super::view_model::SettingsSection::Advanced,
            super::view_model::SettingsSection::About,
        ] {
            let mut vm = FrontendViewModel {
                lang: zh,
                active_page: Page::Settings,
                settings_open: true,
                settings_section: section,
                ..Default::default()
            };
            for _ in 0..2 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(viewport()),
                    ..Default::default()
                });
                let mut actions = Vec::new();
                render(&ctx, &mut vm, &mut actions);
                let _ = ctx.end_pass();
            }
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let output = ctx.end_pass();
            let painted = painted_text(&output);
            for key in [
                "modal.sections.general",
                "modal.sections.shortcuts",
                "modal.sections.appearance",
                "modal.sections.services",
                "modal.sections.privacy",
                "modal.sections.advanced",
                "modal.sections.about",
            ] {
                let expected = openless_linux_egui::tr_l10n(zh, key);
                assert!(
                    painted.contains(expected),
                    "settings rail must paint {key} ({expected:?})"
                );
            }
        }
    }

    #[test]
    fn fixed_ui_keeps_the_parent_cursor_in_place() {
        // Regression: `ui.scope_builder` rewinds the parent cursor to the
        // child's used rect, which made each card in a row pull the next row up
        // over itself. `layout::fixed_ui` must not move the parent cursor even
        // when the card body paints instead of allocating.
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        egui::CentralPanel::default().show(&ctx, |ui| {
            ui.allocate_exact_size(egui::vec2(100.0, 10.0), egui::Sense::hover());
            let before = ui.next_widget_position();
            let rect = egui::Rect::from_min_size(before, egui::vec2(240.0, 120.0));
            layout::fixed_ui(ui, rect, "test-card", |ui| {
                ui.label("card body");
            });
            assert_eq!(
                ui.next_widget_position(),
                before,
                "fixed_ui must leave the parent layout cursor untouched"
            );
        });
        let _ = ctx.end_pass();
    }

    #[test]
    fn sidebar_navigation_receives_pointer_clicks_above_window_layers() {
        let ctx = egui::Context::default();

        // Areas use their first pass to establish their screen rectangles.
        frame(&ctx, Vec::new());
        frame(&ctx, Vec::new());

        let pointer = egui::pos2(50.0, 133.0);
        assert_eq!(
            ctx.layer_id_at(pointer),
            Some(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("openless-sidebar"),
            )),
            "the sidebar must be the top input layer at a navigation button"
        );
        frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::PointerButton {
                    pos: pointer,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let actions = frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert!(
            actions
                .iter()
                .any(|action| matches!(action, FrontendAction::Navigate(Page::History))),
            "the foreground resize layer must not consume sidebar clicks"
        );
    }

    #[test]
    fn titlebar_close_control_receives_pointer_clicks() {
        let ctx = egui::Context::default();
        frame(&ctx, Vec::new());
        frame(&ctx, Vec::new());

        let pointer = egui::pos2(1214.0, 20.0);
        frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let actions = frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );

        assert!(
            actions
                .iter()
                .any(|action| matches!(action, FrontendAction::WindowClose)),
            "the titlebar container must not consume the close button click"
        );
    }
}
