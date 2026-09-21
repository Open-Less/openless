pub mod corrections;
pub mod format;
pub mod history;
pub mod icons;
pub mod layout;
pub mod marketplace;
pub mod overview;
pub mod pages;
pub mod popups;
pub mod selection_ask;
pub mod settings;
pub mod siri_gl;
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
    fn less_computer_rows_follow_the_enable_toggle() {
        // Tauri `CodingAgentSection` shows 后端/权限/模型等配置行 only while the
        // feature is enabled; a disabled section is just the toggle.
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let provider = openless_linux_egui::tr_l10n(zh, "settings.coding_agent.provider");
        for (enabled, expected) in [(false, false), (true, true)] {
            let mut vm = FrontendViewModel {
                lang: zh,
                active_page: Page::Settings,
                settings_open: true,
                settings_section: super::view_model::SettingsSection::Advanced,
                advanced_open: 0,
                ..Default::default()
            };
            vm.settings.less_computer = enabled;
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
            assert_eq!(
                painted.lines().any(|line| line.trim() == provider),
                expected,
                "Less Computer config rows must follow the enable toggle"
            );
        }
    }

    /// 渲染设置页并把这一帧画出的文字按行返回。
    fn painted_settings_lines(
        section: super::view_model::SettingsSection,
        vm: &mut FrontendViewModel,
    ) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut lines = Vec::new();
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            vm.lang = openless_linux_egui::Lang::ZhCn;
            vm.active_page = Page::Settings;
            vm.settings_open = true;
            vm.settings_section = section;
            render(&ctx, vm, &mut actions);
            lines = painted_text(&ctx.end_pass())
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
        }
        lines
    }

    #[test]
    fn shortcut_menu_reveals_record_and_disable() {
        let zh = openless_linux_egui::Lang::ZhCn;
        let record = openless_linux_egui::tr_l10n(zh, "settings.recording.combo_record_btn");
        let disable = openless_linux_egui::tr_l10n(zh, "settings.shortcuts.disable");
        let mut vm = FrontendViewModel {
            shortcut_menu: Some(super::view_model::ShortcutField::Qa),
            ..Default::default()
        };
        let open = painted_settings_lines(super::view_model::SettingsSection::Shortcuts, &mut vm);
        assert!(
            open.iter().any(|line| line == record),
            "the record button must be painted"
        );
        assert!(
            open.iter().any(|line| line == disable),
            "the disable button must be painted"
        );
        // 收起菜单后两个按钮都要消失。
        vm.shortcut_menu = None;
        let closed = painted_settings_lines(super::view_model::SettingsSection::Shortcuts, &mut vm);
        assert!(!closed.iter().any(|line| line == record));
        assert!(!closed.iter().any(|line| line == disable));
    }

    #[test]
    fn shortcut_rows_follow_the_video_order() {
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel {
            dictation_hotkey: "Alt+Z".to_string(),
            ..Default::default()
        };
        let lines = painted_settings_lines(super::view_model::SettingsSection::Shortcuts, &mut vm);
        let index_of = |key: &'static str| {
            let label = openless_linux_egui::tr_l10n(zh, key);
            lines
                .iter()
                .position(|line| line == label)
                .unwrap_or_else(|| panic!("{key} ({label:?}) not painted in {lines:?}"))
        };
        let start = index_of("settings.shortcuts.start_stop");
        let translation = index_of("hotkey.translation");
        let qa = index_of("selection_ask.hotkey_title");
        let switch_style = index_of("settings.shortcuts.switch_style");
        let style_pack = index_of("settings.shortcuts.style_pack_title");
        let open_app = index_of("settings.shortcuts.open_app");
        let cancel = index_of("settings.shortcuts.cancel");
        assert!(
            start < translation
                && translation < qa
                && qa < switch_style
                && switch_style < style_pack
                && style_pack < open_app
                && open_app < cancel,
            "shortcut rows must keep the Tauri order, painted: {lines:?}"
        );
    }

    #[test]
    fn recording_captures_a_bare_modifier_after_release() {
        // egui 没有修饰键的 Key 事件，所以「按住修饰键当热键」只能跨帧判断：
        // 第一帧按住 Ctrl、第二帧松开且期间没有其它键 → 记为 LeftControl。
        use super::view_model::{FrontendAction, ShortcutField};
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
            settings_open: true,
            settings_section: super::view_model::SettingsSection::Shortcuts,
            shortcut_recording: Some(ShortcutField::CodingAgentVoice),
            ..Default::default()
        };
        let held = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let mut captured = Vec::new();
        for modifiers in [held, held, egui::Modifiers::default()] {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                modifiers,
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
            for action in actions {
                if let FrontendAction::ShortcutCaptured(field, primary, modifiers) = action {
                    captured.push((field, primary, modifiers));
                }
            }
        }
        assert_eq!(captured.len(), 1, "exactly one capture after the release");
        assert_eq!(captured[0].0, ShortcutField::CodingAgentVoice);
        assert_eq!(captured[0].1, "LeftControl");
        assert!(captured[0].2.is_empty(), "a bare modifier carries no tags");
    }

    #[test]
    fn recording_ignores_a_modifier_combination_without_a_key() {
        // Ctrl+Shift 同按后松手：不是有效的裸修饰键触发，不能录进去。
        use super::view_model::{FrontendAction, ShortcutField};
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            active_page: Page::Settings,
            settings_open: true,
            settings_section: super::view_model::SettingsSection::Shortcuts,
            shortcut_recording: Some(ShortcutField::Qa),
            ..Default::default()
        };
        let both = egui::Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Default::default()
        };
        let mut captured = Vec::new();
        for modifiers in [both, both, egui::Modifiers::default()] {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                modifiers,
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
            captured.extend(actions.into_iter().filter_map(|action| match action {
                FrontendAction::ShortcutCaptured(..) => Some(()),
                _ => None,
            }));
        }
        assert!(captured.is_empty(), "no capture for a modifier chord");
    }

    #[test]
    fn recording_captures_the_pressed_combination() {
        use super::view_model::{FrontendAction, ShortcutField};
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
            settings_open: true,
            settings_section: super::view_model::SettingsSection::Shortcuts,
            shortcut_recording: Some(ShortcutField::Qa),
            ..Default::default()
        };
        let mut captured = None;
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                events: vec![egui::Event::Key {
                    key: egui::Key::K,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
            for action in actions {
                if let FrontendAction::ShortcutCaptured(field, primary, modifiers) = action {
                    captured = Some((field, primary, modifiers));
                }
            }
        }
        let (field, primary, modifiers) = captured.expect("a captured binding");
        assert_eq!(field, ShortcutField::Qa);
        assert_eq!(primary, "K");
        assert!(modifiers.contains(&"ctrl".to_string()));
        assert!(modifiers.contains(&"shift".to_string()));
    }

    #[test]
    fn style_pack_hotkey_rows_render_their_pack_and_keycaps() {
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel::default();
        vm.style_packs = vec![
            super::view_model::StylePack {
                id: "builtin-polish".into(),
                name: "Polish".into(),
                description: String::new(),
                tags: Vec::new(),
                is_builtin: true,
                enabled: true,
                is_active: true,
                selection_active: false,
            },
            super::view_model::StylePack {
                id: "custom-legal".into(),
                name: "Legal".into(),
                description: String::new(),
                tags: Vec::new(),
                is_builtin: false,
                enabled: false,
                is_active: false,
                selection_active: false,
            },
        ];
        vm.settings.style_pack_hotkeys = vec![super::view_model::StylePackHotkeyRow {
            pack_id: "custom-legal".into(),
            name: "Legal".into(),
            hotkey: "Ctrl+Shift+L".into(),
        }];
        let lines = painted_settings_lines(super::view_model::SettingsSection::Shortcuts, &mut vm);
        // 停用中的风格包在下拉里带「（已停用）」后缀（Tauri `stylePackDisabledSuffix`）。
        let disabled = format!(
            "Legal{}",
            openless_linux_egui::tr_l10n(zh, "settings.shortcuts.style_pack_disabled_suffix")
        );
        assert!(
            lines.iter().any(|line| line == &disabled),
            "disabled pack suffix must be shown, painted: {lines:?}"
        );
        // 键帽逐键渲染。
        assert!(lines.iter().any(|line| line == "Ctrl"), "modifier keycap");
        assert!(lines.iter().any(|line| line == "Shift"), "modifier keycap");
        assert!(lines.iter().any(|line| line == "L"), "primary keycap");
    }

    #[test]
    fn style_pack_add_button_opens_the_draft_row() {
        use super::view_model::SettingsSection;
        let zh = openless_linux_egui::Lang::ZhCn;
        let add = format!(
            "+ {}",
            openless_linux_egui::tr_l10n(zh, "settings.shortcuts.style_pack_add")
        );
        let mut vm = FrontendViewModel::default();
        let closed = painted_settings_lines(SettingsSection::Shortcuts, &mut vm);
        assert!(closed.iter().any(|line| line == &add), "add button shows");
        vm.style_hotkey_draft_open = true;
        let open = painted_settings_lines(SettingsSection::Shortcuts, &mut vm);
        assert!(
            !open.iter().any(|line| line == &add),
            "add button hides while drafting"
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
            id: format!("pack-{name}"),
            enabled: true,
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

    /// 用户报告「缩放窗口时并不是始终居中」：模态卡片此前用 `Area::anchor`
    /// 定位，垂直方向稳定偏下 19.5px，且卡片被内容撑宽 22px（横向偏 11px）。
    /// 现在位置由 `body.center()` 显式算出，两种偏差都必须消失，并且**改变窗口
    /// 尺寸后的第一帧**就要居中（不能靠后续帧收敛）。
    #[test]
    fn the_settings_modal_stays_centred_across_window_resizes() {
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
            settings_open: true,
            ..Default::default()
        };
        for (width, height) in [
            (1240.0, 800.0),
            (900.0, 620.0),
            (1600.0, 1000.0),
            (1100.0, 900.0),
        ] {
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
            // 等一帧让内容成型，下一帧断言（尺寸变化不接受「过渡帧」偏差）。
            for _ in 0..2 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                });
                let mut actions = Vec::new();
                render(&ctx, &mut vm, &mut actions);
                let _ = ctx.end_pass();
            }
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = ctx.end_pass();
            let body = layout::body_rect(&ctx);
            // 卡片矩形现在由叠层自己写入 memory（Area 覆盖整个 body，面积已不等于卡片）。
            let modal = ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new("openless-settings-card-rect"))
                })
                .expect("the settings card rect must be published while the overlay is open");
            assert!(
                (modal.center().x - body.center().x).abs() <= 1.5,
                "modal must be horizontally centred at {width}x{height}: modal={modal:?} body={body:?}"
            );
            assert!(
                (modal.center().y - body.center().y).abs() <= 1.5,
                "modal must be vertically centred at {width}x{height}: modal={modal:?} body={body:?}"
            );
            // 卡片不会被内容撑宽（撑宽就会把居中算歪）。
            let expected_width = (body.width() - 40.0).max(320.0).min(960.0);
            assert!(
                (modal.width() - expected_width).abs() <= 2.0,
                "the content must fit the requested modal width {expected_width}: {modal:?}"
            );
        }
    }

    /// 渲染一帧**指定 vm** 的前端（`frame()` 用的是默认 vm，遮罩类弹窗需要打开状态）。
    fn overlay_frame(
        ctx: &egui::Context,
        vm: &mut FrontendViewModel,
        events: Vec<egui::Event>,
    ) -> Vec<FrontendAction> {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            events,
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(ctx, vm, &mut actions);
        let _ = ctx.end_pass();
        actions
    }

    /// 遮罩类弹窗的结构性回归（用户报「点阴影后阴影上移、设置没法用」）：
    /// 遮罩、点击拦截与卡片必须在**同一个 `Area`**（同一个 LayerId）里，先画遮罩再画卡片。
    /// 各自独立容器时 egui 会在按下后把被点到的那个 `move_to_top`，遮罩一旦被抬起来就会
    /// 盖住卡片。三处弹窗（市场详情 / 风格编辑器 / 历史确认）都必须满足：
    ///  * 卡片落在弹窗自己的图层上、且在**自己的遮罩区**里居中（三处弹窗的遮罩区
    ///    都取自各自的页面 body，历史页的遮罩区是其页面内部分配的 body，故用
    ///    egui memory 里的 Area 矩形作基准，而不是 `layout::body_rect`）；
    ///  * 遮罩上的点也落在弹窗自己的图层（点击不会漏到下方页面）；
    ///  * 点一下遮罩之后，卡片既不移位、也不会被抬起的遮罩盖住。
    fn assert_overlay_keeps_the_card_on_top(
        ctx: &egui::Context,
        vm: &mut FrontendViewModel,
        area_id: &str,
        card_rect_key: &str,
    ) {
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(area_id));
        // Areas 用第一帧建立屏幕矩形，第二帧才可查询。
        for _ in 0..2 {
            overlay_frame(ctx, vm, Vec::new());
        }
        let body = layout::body_rect(ctx);
        // 遮罩区就是弹窗 `Area` 自己的矩形（遮罩 + 点击拦截 + 卡片同属它）。
        let overlay = ctx
            .memory(|mem| mem.area_rect(egui::Id::new(area_id)))
            .unwrap_or_else(|| panic!("{area_id} must exist while the overlay is open"));
        let card = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(card_rect_key)))
            .unwrap_or_else(|| {
                panic!("{card_rect_key} must be published while the overlay is open")
            });
        assert!(card.width() > 0.0 && card.height() > 0.0, "{card:?}");
        assert!(
            body.contains_rect(overlay),
            "the mask {overlay:?} must stay inside the content area {body:?}"
        );
        assert!(
            overlay.contains_rect(card),
            "the card {card:?} must sit inside its mask {overlay:?}"
        );
        assert!(
            (card.center().x - overlay.center().x).abs() <= 1.5,
            "card must be horizontally centred: {card:?} in {overlay:?}"
        );
        assert!(
            (card.center().y - overlay.center().y).abs() <= 1.5,
            "card must be vertically centred: {card:?} in {overlay:?}"
        );
        assert_eq!(
            ctx.layer_id_at(card.center()),
            Some(layer),
            "{area_id}: the card must live in the modal's own layer"
        );
        // 遮罩探针：遮罩顶端内缩 6px（卡片居中，肯定不在卡片上）。
        let mask = egui::pos2(overlay.center().x, overlay.top() + 6.0);
        assert!(
            !card.contains(mask) && overlay.contains(mask),
            "probe {mask:?} must be on the mask, not on the card {card:?}"
        );
        assert_eq!(
            ctx.layer_id_at(mask),
            Some(layer),
            "{area_id}: the mask must swallow input instead of letting it reach the page below"
        );
        // 点一下遮罩：遮罩被抬到卡片之上就会失败。
        overlay_frame(
            ctx,
            vm,
            vec![
                egui::Event::PointerMoved(mask),
                egui::Event::PointerButton {
                    pos: mask,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        overlay_frame(
            ctx,
            vm,
            vec![egui::Event::PointerButton {
                pos: mask,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let after = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new(card_rect_key)))
            .unwrap_or_else(|| panic!("{card_rect_key} must stay published"));
        assert_eq!(
            after, card,
            "{area_id}: clicking the mask must not move the card"
        );
        assert_eq!(
            ctx.layer_id_at(card.center()),
            Some(layer),
            "{area_id}: clicking the mask must not raise it above the card"
        );
    }

    #[test]
    fn marketplace_detail_overlay_is_one_layer() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            active_page: Page::Marketplace,
            // 默认是 `loading = true` / `unsupported = true`（宿主还没送到列表），两者都会
            // 让页面提前 return。
            marketplace_loading: false,
            marketplace_unsupported: false,
            ..Default::default()
        };
        vm.marketplace_packs = vec![super::view_model::MarketplacePack {
            name: "overlay-fixture".to_string(),
            version: "1.0.0".to_string(),
            description: "fixture for the marketplace detail overlay test".to_string(),
            mode: "dictation".to_string(),
            author: "tester".to_string(),
            tags: Vec::new(),
            likes: 1,
            downloads: 2,
            liked: false,
        }];
        vm.marketplace_selected = Some(0);
        assert_overlay_keeps_the_card_on_top(
            &ctx,
            &mut vm,
            "openless-marketplace-detail-modal",
            "openless-marketplace-detail-card-rect",
        );
    }

    #[test]
    fn style_editor_overlay_is_one_layer() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            active_page: Page::Style,
            // 默认是 `unsupported = true`（宿主还没报能力），那样页面会走 unsupported 分支。
            style_unsupported: false,
            style_editor_open: true,
            ..Default::default()
        };
        assert_overlay_keeps_the_card_on_top(
            &ctx,
            &mut vm,
            "openless-style-editor-modal",
            "openless-style-editor-card-rect",
        );
    }

    #[test]
    fn history_confirm_overlay_is_one_layer() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            active_page: Page::History,
            ..Default::default()
        };
        vm.history_confirm = Some(super::view_model::HistoryConfirm::Clear);
        assert_overlay_keeps_the_card_on_top(
            &ctx,
            &mut vm,
            "openless-history-confirm",
            "openless-history-confirm-card-rect",
        );
    }

    /// 无边框窗口的四个拖拽区必须给出对应方向的拉伸光标（否则用户看不出窗口
    /// 能拉伸）。指针放在左边缘中部时应当得到 ResizeHorizontal。
    #[test]
    fn the_window_edges_show_a_resize_cursor_on_hover() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1240.0, 800.0));
        // 拖拽区贴在**窗口**边上，而窗口是屏幕内缩 6px 的圆角表面。
        let window = screen.shrink(6.0);
        let probes = [
            (
                egui::pos2(window.left() + 2.0, window.center().y),
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                egui::pos2(window.center().x, window.bottom() - 2.0),
                egui::CursorIcon::ResizeVertical,
            ),
            (
                egui::pos2(window.right() - 4.0, window.bottom() - 4.0),
                egui::CursorIcon::ResizeNwSe,
            ),
            (
                egui::pos2(window.right() - 4.0, window.top() + 4.0),
                egui::CursorIcon::ResizeNeSw,
            ),
        ];
        for (position, expected) in probes {
            let mut icon = egui::CursorIcon::Default;
            for _ in 0..3 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(position)],
                    ..Default::default()
                });
                layout::resize_handles(&ctx);
                // 光标在 platform_output 里，而 `end_pass` 会把 output 取走，
                // 所以要读返回值而不是 `ctx.output(...)`。
                icon = ctx.end_pass().platform_output.cursor_icon;
            }
            assert_eq!(
                icon, expected,
                "hovering {position:?} must show {expected:?}, got {icon:?}"
            );
        }
    }

    /// 设置页顶部不该再有那一层「设置」标题栏：Tauri 的桌面端左栏顶端是搜索框、
    /// 右栏顶端才是「标题 + 修改后自动保存 + 关闭」。侧栏导航里那一个「设置」仍在。
    #[test]
    fn the_settings_overlay_has_no_separate_title_bar() {
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let settings_label = openless_linux_egui::tr_l10n(zh, "nav.settings");
        let auto_save = openless_linux_egui::tr_l10n(zh, "modal.auto_save_hint");
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
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
        // 只数「整行就是这个标签」的次数：侧栏导航那一个是正常的，多出来的那个
        // 就是被删掉的顶栏标题（子串匹配会把「查找设置分类…」也算进去）。
        let standalone_titles = painted
            .lines()
            .filter(|line| line.trim() == settings_label)
            .count();
        assert_eq!(
            standalone_titles, 1,
            "the overlay must not draw its own settings title (only the sidebar entry may say it): {painted}"
        );
        assert!(
            painted.contains(openless_linux_egui::tr_l10n(zh, "modal.sections.general")),
            "the content pane still owns the section title: {painted}"
        );
        assert!(
            painted.contains(auto_save),
            "the auto-save hint belongs to the content pane's own header row: {painted}"
        );
    }

    /// 录音分区必须真的带上用户点名的几行：可录制的录音快捷键、首选麦克风、
    /// 录音胶囊开关与胶囊样式、录音提示音的试听按钮。
    #[test]
    fn the_recording_section_renders_the_rows_the_user_asked_for() {
        let ctx = egui::Context::default();
        let zh = openless_linux_egui::Lang::ZhCn;
        let mut vm = FrontendViewModel {
            lang: zh,
            active_page: Page::Settings,
            settings_open: true,
            ..Default::default()
        };
        vm.dictation_hotkey = "Alt+A".to_string();
        vm.settings.microphone_options = vec!["USB microphone".to_string()];
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
        for key in [
            "settings.recording.hotkey_label",
            "settings.recording.microphone_label",
            "settings.recording.capsule_label",
            "settings.recording.capsule_style_label",
            "settings.recording.audio_cue_label",
            "settings.recording.audio_cue_preview",
        ] {
            let label = openless_linux_egui::tr_l10n(zh, key);
            assert!(
                painted.contains(label),
                "missing {key} ({label}) in {painted}"
            );
        }
        assert!(
            !painted.contains(openless_linux_egui::tr_l10n(
                zh,
                "settings.recording.microphone_load_error"
            )),
            "no microphone error line may be painted without an error: {painted}"
        );
    }

    /// 回归排查：设置里可展开分组必须「点标题行即可开合」。用户报过「点不开」，
    /// 真机（uinput 点击）与这条无头测试都表明交互是好的 —— 留着防止真回归。
    #[test]
    fn the_collapsible_group_header_toggles_on_click() {
        let ctx = egui::Context::default();
        let mut painted = String::new();
        let mut states = Vec::new();
        for frame in 0..4 {
            let events = if frame == 1 {
                vec![egui::Event::PointerMoved(egui::pos2(120.0, 30.0))]
            } else if frame == 2 {
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(120.0, 30.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }]
            } else if frame == 3 {
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(120.0, 30.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }]
            } else {
                Vec::new()
            };
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            });
            egui::CentralPanel::default().show(&ctx, |ui| {
                super::settings::test_group_toggle(ui);
            });
            painted = painted_text(&ctx.end_pass());
            states.push(painted.contains("GROUPCONTENT"));
        }
        assert!(states[0], "collapsible groups start expanded, like Tauri");
        assert_eq!(
            states[3], false,
            "the group must collapse after its header is clicked: states={states:?}"
        );
    }

    /// 回归排查：按住标题栏必须发出 `ViewportCommand::StartDrag`（窗口能被拖走）。
    /// 用户报过「拖标题栏移不动窗口」，真机 uinput 拖拽实测窗口确实移动
    /// （340,118 → 490,218），这条锁住发出指令那一环。
    #[test]
    fn the_titlebar_press_starts_a_window_drag() {
        let ctx = egui::Context::default();
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1240.0, 800.0));
        let mut started = false;
        for frame in 0..3 {
            let events = if frame == 1 {
                vec![egui::Event::PointerMoved(egui::pos2(600.0, 25.0))]
            } else if frame == 2 {
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(600.0, 25.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }]
            } else {
                Vec::new()
            };
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(window),
                events,
                ..Default::default()
            });
            let mut actions = Vec::new();
            layout::titlebar(&ctx, &mut actions);
            let output = ctx.end_pass();
            for commands in output.viewport_output.values() {
                if commands
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::StartDrag))
                {
                    started = true;
                }
            }
        }
        assert!(
            started,
            "pressing the titlebar must emit ViewportCommand::StartDrag"
        );
    }

    /// 回归排查：`StartDrag` 在 egui-winit 里带着 `window.has_focus()` 前置条件，
    /// 未聚焦时会被丢掉（Wayland 的 `move` 又只认按下那一帧的 serial）。所以窗口
    /// 未聚焦时按住的每一帧都得继续补发；已聚焦且没新按下时不得每帧乱发。
    #[test]
    fn the_titlebar_keeps_asking_while_the_window_is_unfocused() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1240.0, 800.0));
        let press = egui::Event::PointerButton {
            pos: egui::pos2(600.0, 25.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        };

        let run = |focused: Option<bool>, frames: usize| -> Vec<bool> {
            let ctx = egui::Context::default();
            let mut per_frame = Vec::new();
            for frame in 0..frames {
                // 第 1 帧先把指针移到标题栏上，第 2 帧按下，之后保持按住。
                let events = if frame == 1 {
                    vec![egui::Event::PointerMoved(egui::pos2(600.0, 25.0))]
                } else if frame == 2 {
                    vec![press.clone()]
                } else {
                    Vec::new()
                };
                let mut raw = egui::RawInput {
                    screen_rect: Some(window),
                    events,
                    ..Default::default()
                };
                raw.viewports.insert(
                    raw.viewport_id,
                    egui::ViewportInfo {
                        focused,
                        ..Default::default()
                    },
                );
                ctx.begin_pass(raw);
                let mut actions = Vec::new();
                layout::titlebar(&ctx, &mut actions);
                let output = ctx.end_pass();
                per_frame.push(output.viewport_output.values().any(|commands| {
                    commands
                        .commands
                        .iter()
                        .any(|command| matches!(command, egui::ViewportCommand::StartDrag))
                }));
            }
            per_frame
        };

        // 未聚焦：按下帧之后（第 3、4 帧）必须继续补发。
        let unfocused = run(Some(false), 5);
        assert!(unfocused[2], "the press frame must ask for the drag");
        assert!(
            unfocused[3] && unfocused[4],
            "while unfocused the titlebar must keep asking: {unfocused:?}"
        );
        // 已聚焦：只在按下那一帧发一次，不能每帧刷。
        let focused = run(Some(true), 5);
        assert!(focused[2], "the press frame must ask for the drag");
        assert!(
            !focused[3] && !focused[4],
            "a focused window must not re-ask every frame: {focused:?}"
        );
    }
}
