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
pub mod style;
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
        // it must not be wrapped in the shared page scroll area. Same for the
        // style page (full-height card) and history (two independent columns).
        //
        // These are *branching* arms rather than early returns: the settings
        // overlay below has to be painted on every page, and an early return
        // used to skip it (设置按钮在概览/风格/历史页点了没反应).
        match vm.active_page {
            Page::Overview => overview::page(ui, vm, actions),
            Page::Style => style::page(ui, vm, actions),
            Page::History => history::page(ui, vm, actions),
            page => {
                egui::ScrollArea::vertical()
                    .id_salt("openless-main-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match page {
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
            }
        }

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
    fn text_inputs_keep_and_show_what_the_user_types() {
        // Two separate regressions live here:
        //  * the marketplace search bound a local clone, so the host never wrote
        //    the field back and every keystroke vanished on the next frame;
        //  * the settings text rows were re-hydrated from preferences every
        //    frame, so editing them snapped back to the stored value.
        let zh = openless_linux_egui::Lang::ZhCn;

        // 1) marketplace search
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Marketplace,
            ..Default::default()
        };
        let id = egui::Id::new("openless-marketplace-search");
        // warm up: egui needs a frame before the widget exists / accepts focus
        for _ in 0..3 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
        }
        for step in ["a", "b", "c"] {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                events: vec![egui::Event::Text(step.into())],
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
        }
        assert_eq!(
            vm.marketplace_query, "abc",
            "the marketplace search field must keep typed characters"
        );

        // 2) settings text row (历史条数上限 lives in 权限与数据 → 数据存储)
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
            settings_open: true,
            settings_section: super::view_model::SettingsSection::Privacy,
            ..Default::default()
        };
        let label =
            openless_linux_egui::tr_l10n(zh, "settings.recording.history_max_entries_label");
        let id = egui::Id::new(("openless-settings-text", label));
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
        }
        let mut painted = String::new();
        for step in ["7", "7"] {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                events: vec![egui::Event::Text(step.into())],
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            painted = painted_text(&ctx.end_pass());
        }
        assert_eq!(
            vm.settings.history_max_entries, "77",
            "settings text rows must keep typed characters"
        );
        assert!(
            painted.contains("77"),
            "the typed value must actually be painted"
        );

        // 3) 添加渠道表单里的名称输入框（AI 服务与模型 → 语音识别）
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
            settings_open: true,
            settings_section: super::view_model::SettingsSection::Services,
            services_view: 1,
            channel_form_open: true,
            ..Default::default()
        };
        let id = egui::Id::new("openless-settings-channel-name");
        let mut painted = String::new();
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            painted = painted_text(&ctx.end_pass());
        }
        for step in ["m", "y"] {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                events: vec![egui::Event::Text(step.into())],
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            painted = painted_text(&ctx.end_pass());
        }
        assert_eq!(
            vm.channel_form_name, "my",
            "the add-channel form must accept typed characters"
        );
        assert!(
            painted.contains("my"),
            "the add-channel form must paint what was typed"
        );
    }

    #[test]
    fn settings_overlay_opens_from_every_page() {
        // Regression: Overview / Style / History returned early from `render`, so
        // the settings overlay at the end of the function never ran and the
        // 设置 button did nothing on those pages.
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let rail_general = openless_linux_egui::tr_l10n(zh, "modal.sections.general");
        for page in [Page::Overview, Page::Style, Page::History, Page::Vocab] {
            let mut vm = FrontendViewModel {
                lang: zh,
                active_page: page,
                settings_open: true,
                ..Default::default()
            };
            let mut painted = String::new();
            for _ in 0..2 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(viewport()),
                    ..Default::default()
                });
                let mut actions = Vec::new();
                render(&ctx, &mut vm, &mut actions);
                painted = painted_text(&ctx.end_pass());
            }
            assert!(
                painted.contains(rail_general),
                "the settings overlay must render on {page:?} too"
            );
        }
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
            // The shortcuts section renders key caps for the live bindings.
            vm.dictation_hotkey = "Ctrl+Shift+Z".to_string();
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
            // Section blurb under the title, mirroring the Tauri modal.
            let desc_key = match section {
                super::view_model::SettingsSection::General => "modal.descriptions.general",
                super::view_model::SettingsSection::Shortcuts => "modal.descriptions.shortcuts",
                super::view_model::SettingsSection::Services => "modal.descriptions.services",
                super::view_model::SettingsSection::Appearance => "modal.descriptions.appearance",
                super::view_model::SettingsSection::Privacy => "modal.descriptions.privacy",
                super::view_model::SettingsSection::Advanced => "modal.descriptions.advanced",
                super::view_model::SettingsSection::About => "modal.descriptions.about",
            };
            let desc = openless_linux_egui::tr_l10n(zh, desc_key);
            assert!(
                painted.contains(desc),
                "settings section blurb must paint {desc_key} ({desc:?})"
            );
            if section == super::view_model::SettingsSection::Shortcuts {
                // Key caps: one painted chip per key in the binding.
                for cap in ["Ctrl", "Shift", "Z"] {
                    assert!(
                        painted.contains(cap),
                        "shortcut rows must paint the {cap} key cap"
                    );
                }
            }
        }
    }

    #[test]
    fn sidebar_settings_row_stays_reachable_in_a_short_window() {
        // Regression: the sidebar painted against `ui.max_rect()` (the whole
        // screen), so the rounded bottom-left corner and the pinned settings row
        // both landed off-window in a short window.
        let small = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 520.0));
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            ..Default::default()
        };
        let mut painted: Vec<(String, egui::Rect)> = Vec::new();
        for _ in 0..3 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(small),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let output = ctx.end_pass();
            painted.clear();
            for clipped in &output.shapes {
                if let egui::Shape::Text(text) = &clipped.shape {
                    painted.push((
                        text.galley.text().to_string(),
                        text.visual_bounding_rect().intersect(clipped.clip_rect),
                    ));
                }
            }
        }
        let settings =
            openless_linux_egui::tr_l10n(openless_linux_egui::Lang::ZhCn, "nav.settings");
        let (_, rect) = painted
            .iter()
            .find(|(text, _)| text == settings)
            .expect("the sidebar must paint the settings row");
        assert!(
            rect.height() > 0.0 && rect.bottom() <= small.bottom(),
            "the settings row must be visible inside the window: {rect:?}"
        );
    }

    #[test]
    fn ai_service_tabs_follow_the_host_capabilities() {
        // Tauri gates the local-model view on `supports_local_asr`; the Linux
        // host reports false, so the tab (and its "not supported" card) must
        // disappear instead of being permanently visible.
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let models = openless_linux_egui::tr_l10n(zh, "modal.service_views.models");
        for (supported, expected) in [(false, false), (true, true)] {
            let mut vm = FrontendViewModel {
                lang: zh,
                active_page: Page::Settings,
                settings_open: true,
                settings_section: super::view_model::SettingsSection::Services,
                supports_local_asr: supported,
                ..Default::default()
            };
            let mut painted = String::new();
            for _ in 0..2 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(viewport()),
                    ..Default::default()
                });
                let mut actions = Vec::new();
                render(&ctx, &mut vm, &mut actions);
                painted = painted_text(&ctx.end_pass());
            }
            // Compare whole painted lines: the section description also mentions
            // 「本地模型」, so a substring check would always match.
            assert_eq!(
                painted.lines().any(|line| line.trim() == models),
                expected,
                "local-model tab visibility must follow supports_local_asr"
            );
        }
    }

    #[test]
    fn empty_library_pages_render_their_empty_state_not_unsupported() {
        // Regression: the library pages only cleared `*_unsupported` when the
        // store was non-empty, so an empty dictionary/correction store rendered
        // the "not wired up yet" placeholder instead of the empty state.
        let zh = openless_linux_egui::Lang::ZhCn;
        let unsupported = openless_linux_egui::tr_l10n(zh, "common.unsupported_title");
        for (label, page, empty_key) in [
            ("vocab", Page::Vocab, "vocab.empty"),
            ("corrections", Page::Corrections, "vocab.corrections_empty"),
        ] {
            let ctx = egui::Context::default();
            let mut vm = FrontendViewModel {
                lang: zh,
                active_page: page,
                // What the host reports once the (empty) library has loaded.
                vocab_unsupported: false,
                ..Default::default()
            };
            let mut painted = String::new();
            for _ in 0..3 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(viewport()),
                    ..Default::default()
                });
                let mut actions = Vec::new();
                render(&ctx, &mut vm, &mut actions);
                let output = ctx.end_pass();
                painted = painted_text(&output);
            }
            assert!(
                !painted.contains(unsupported),
                "{label} must not show the unsupported placeholder for an empty store"
            );
            let empty = openless_linux_egui::tr_l10n(zh, empty_key);
            assert!(
                painted.contains(empty),
                "{label} must show its empty-state hint ({empty:?})"
            );
        }
    }

    #[test]
    fn overlays_stay_inside_a_small_window() {
        // Regression: the style editor used to force a minimum card height, so a
        // long prompt pushed the button row past the window edge. Every overlay
        // must stay inside the viewport at a small window size.
        let small = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0));
        let long_prompt = "line\n".repeat(120);

        let cases: [(&str, FrontendViewModel); 2] = [
            (
                "style editor",
                FrontendViewModel {
                    lang: openless_linux_egui::Lang::ZhCn,
                    active_page: Page::Style,
                    style_editor_open: true,
                    style_prompt: long_prompt.clone(),
                    ..Default::default()
                },
            ),
            (
                "settings overlay",
                FrontendViewModel {
                    lang: openless_linux_egui::Lang::ZhCn,
                    active_page: Page::Settings,
                    settings_open: true,
                    ..Default::default()
                },
            ),
        ];

        for (label, mut vm) in cases {
            let ctx = egui::Context::default();
            let mut output = None;
            for _ in 0..3 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(small),
                    ..Default::default()
                });
                let mut actions = Vec::new();
                render(&ctx, &mut vm, &mut actions);
                output = Some(ctx.end_pass());
            }
            let output = output.expect("a frame was rendered");
            for clipped in &output.shapes {
                // Only the part inside the shape's clip rect is actually drawn.
                let bounds = clipped
                    .shape
                    .visual_bounding_rect()
                    .intersect(clipped.clip_rect);
                if !bounds.is_finite() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
                    continue;
                }
                assert!(
                    bounds.bottom() <= small.bottom() + 2.0,
                    "{label} painted below the window: {bounds:?} (window {small:?})"
                );
                assert!(
                    bounds.right() <= small.right() + 2.0,
                    "{label} painted right of the window: {bounds:?} (window {small:?})"
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

    #[test]
    fn style_page_marks_only_the_active_pack_as_current() {
        // Regression: the page used to treat its page-local `style_selected`
        // index as "active" as well, so a stale index painted a second card in
        // the active style. Only the pack the host reports as active may say
        // "current" — one badge plus one primary button.
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let pack = |name: &str, is_active: bool| super::view_model::StylePack {
            name: name.to_string(),
            description: "sample description".to_string(),
            tags: vec!["light".to_string()],
            is_builtin: true,
            is_active,
            selection_active: false,
        };
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Style,
            style_unsupported: false,
            ..Default::default()
        };
        vm.style_packs = vec![
            pack("first", false),
            pack("second", true),
            pack("third", false),
        ];
        vm.style_selected = 0;

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

        let current = openless_linux_egui::tr_l10n(zh, "style.pack.current");
        let activate = openless_linux_egui::tr_l10n(zh, "style.pack.activate");
        assert_eq!(
            painted.matches(current).count(),
            2,
            "exactly one pack (badge + primary button) may read as current"
        );
        assert_eq!(
            painted.matches(activate).count(),
            2,
            "the two other packs offer an activate button"
        );
        assert!(
            painted.contains("first") && painted.contains("second") && painted.contains("third")
        );
    }
}
