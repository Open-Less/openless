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
            Page::History => history::page(ui, vm, actions, false),
            Page::QuickNote => history::page(ui, vm, actions, true),
            Page::Vocab => vocab::page(ui, vm, actions),
            page => {
                // Bound the viewport to the current window, not the previous
                // frame's content size: after shrinking a large window the
                // translation guide must remain reachable by scrolling.
                let scroll_output = egui::ScrollArea::vertical()
                    // Each page owns its own scroll position. Sharing one id
                    // with other pages let a prior tab leave Translation at an
                    // unexpected offset after switching or resizing.
                    .id_salt(("openless-main-scroll", page))
                    .max_height(
                        (body.height() - layout::PAGE_TOP_PADDING - layout::PAGE_BOTTOM_PADDING)
                            .max(1.0),
                    )
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match page {
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
                            Page::Overview
                            | Page::History
                            | Page::QuickNote
                            | Page::Style
                            | Page::Vocab
                            | Page::Settings => {
                                // Handled above or via overlay.
                            }
                        }
                        ui.add_space(32.0);
                    });
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("main-scroll-measure"),
                        (
                            scroll_output.content_size.y,
                            scroll_output.inner_rect.height(),
                            scroll_output.state.offset.y,
                        ),
                    )
                });
                #[cfg(not(test))]
                let _ = scroll_output;
            }
        }

        // Settings overlay (rendered on top of everything).
        if vm.settings_open {
            settings::settings_overlay(ctx, vm, actions, body);
        }
    });
}

/// egui 0.36 起 `FullOutput` 里的 `TexturesDelta` 必须被消费，未应用就 drop 会
/// panic（epaint 的 Drop 守卫："Dropped TexturesDelta with N unapplied deltas"）。
/// 生产路径由渲染器消费（`popup_layer.rs` 交给 painter / eframe 自己处理），
/// 无头测试里没有渲染器，这里清掉即可。
#[cfg(test)]
pub(crate) fn run_pass(
    ctx: &egui::Context,
    input: egui::RawInput,
    body: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, body);
    output.textures_delta.clear();
    output
}

/// 同上，用于 `begin_pass` / `end_pass` 形态的测试。
#[cfg(test)]
pub(crate) fn end_pass(ctx: &egui::Context) -> egui::FullOutput {
    let mut output = ctx.end_pass();
    output.textures_delta.clear();
    output
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
        let _ = crate::ui::frontend::end_pass(&ctx);
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
            let _ = crate::ui::frontend::end_pass(&ctx);
        }

        // One more pass whose painted text we inspect: this is the end-to-end
        // check that the localized dashboard chrome actually reaches the painter.
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = crate::ui::frontend::end_pass(&ctx);
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
        let heatmap: egui::Rect = ctx.data(|data| {
            data.get_temp(egui::Id::new("openless-overview-heatmap-rect"))
                .expect("populated overview should show the heatmap")
        });
        let content_bottom = layout::body_rect(&ctx).bottom() - layout::PAGE_BOTTOM_PADDING;
        assert!(heatmap.bottom() <= content_bottom + 0.5,
            "heatmap must not be obscured by the white bottom gutter: {heatmap:?}, content bottom={content_bottom}");

        // With a pending provider the cards need more than the old fixed 600px
        // cutoff. The yearly activity must remain reachable at this height.
        vm.overview.as_mut().unwrap().asr_configured = false;
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 800.0),
            )),
            ..Default::default()
        });
        render(&ctx, &mut vm, &mut Vec::new());
        let _ = end_pass(&ctx);
        let (content, visible, _): (f32, f32, f32) = ctx.data(|data| {
            data.get_temp(egui::Id::new("overview-scroll-measure"))
                .expect("provider cards plus heatmap must enable page scrolling")
        });
        assert!(
            content > visible,
            "overview did not offer scrolling: {content}/{visible}"
        );
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
                quick_note: false,
                error_code: None,
                created_at: "2026-01-15T12:34:00+00:00".into(),
                mode: super::view_model::OverviewMode::Raw,
                style_label: "raw".into(),
                raw_transcript: "raw transcript of the first entry".into(),
                final_text: String::new(),
                duration_ms: Some(3_100),
                has_audio: true,
                asr_provider: Some("zhipu".into()),
                asr_model: Some("glm-asr-2512".into()),
                asr_ms: Some(465),
                llm_provider: None,
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
            let _ = crate::ui::frontend::end_pass(&ctx);
        }
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = crate::ui::frontend::end_pass(&ctx);
        let painted = painted_text(&output);
        let tr = |key: &'static str| openless_linux_egui::tr_l10n(zh, key);
        for key in [
            "history.title",
            "history.desc",
            "common.refresh",
            "common.clear",
            "history.step_asr",
            // 润色信息现在挂在润色卡片胶囊上（「润色 · 风格」）。
            "history.step_polish",
            // 默认只看润色结果：原文靠这个开关展开。
            "history.show_raw",
            "common.copy",
        ] {
            let expected = tr(key);
            assert!(
                painted.contains(expected),
                "expected the history page to paint {key} ({expected:?})"
            );
        }
        // 菜单默认收起：导出/重新转录/删除都在「…」浮层里，不该直接出现在页面上。
        for key in [
            "history.play",
            "history.export",
            "history.retranscribe",
            "common.delete",
        ] {
            let unexpected = tr(key);
            assert!(
                !painted.contains(unexpected),
                "the collapsed action menu must not paint {key} ({unexpected:?})"
            );
        }
        // 步骤区按上游只保留识别/润色两步，插入行（字数/热词）已经不在详情区。
        assert!(
            !painted.contains(tr("history.step_insert")),
            "the insert step row must be gone"
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
        let _ = crate::ui::frontend::end_pass(&ctx);
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = crate::ui::frontend::end_pass(&ctx);
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
            let _ = crate::ui::frontend::end_pass(&ctx);
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
            let _ = crate::ui::frontend::end_pass(&ctx);
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
            let _ = crate::ui::frontend::end_pass(&ctx);
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
            painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
            painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
            painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
                painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
                let _ = crate::ui::frontend::end_pass(&ctx);
            }
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let output = crate::ui::frontend::end_pass(&ctx);
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
            let output = crate::ui::frontend::end_pass(&ctx);
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
                painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
            lines = painted_text(&crate::ui::frontend::end_pass(&ctx))
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
                ..Default::default()
            });
            ctx.input_mut(|i| i.modifiers = modifiers);
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = crate::ui::frontend::end_pass(&ctx);
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
                ..Default::default()
            });
            ctx.input_mut(|i| i.modifiers = modifiers);
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = crate::ui::frontend::end_pass(&ctx);
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
            let _ = crate::ui::frontend::end_pass(&ctx);
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
                icon_path: None,
                icon_data_url: None,
                base_mode: "light".into(),
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
                icon_path: None,
                icon_data_url: None,
                base_mode: "formal".into(),
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
                painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
                let output = crate::ui::frontend::end_pass(&ctx);
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
                output = Some(crate::ui::frontend::end_pass(&ctx));
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

    /// 设置页遮罩用的是离屏模糊背板，而不是一层暗色：发布的纹理必须真的落到弹窗图层
    /// 上，否则用户看到的还是旧的黑遮罩。
    #[test]
    fn the_settings_overlay_paints_the_published_blurred_backdrop() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            settings_open: true,
            ..Default::default()
        };
        // Area 首帧只建立屏幕矩形，而且 `Area::fade_in` 会把刚出现的图层整个淡入：
        // 淡入没走完时 painter 记下的是 `Shape::Noop`，看不到背板。所以推时间跑几帧。
        for frame in 0..4 {
            crate::ui::backdrop::publish(&ctx, Some(egui::TextureId::User(9)));
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 640.0),
                )),
                time: Some(frame as f64 * 0.1),
                ..Default::default()
            });
            render(&ctx, &mut vm, &mut Vec::new());
            let layer = egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("openless-settings-modal"),
            );
            let backdrop_corners = ctx.graphics(|graphics| {
                graphics.get(layer).and_then(|list| {
                    list.all_entries().find_map(|entry| match &entry.shape {
                        // 模糊背板是「带纹理的矩形」，只有这样圆角才跟着内容区走
                        // （用整块的 `image` 会是直角，底部两角顶出窗口圆角）。
                        egui::Shape::Rect(rect)
                            if rect.fill_texture_id() == egui::TextureId::User(9) =>
                        {
                            Some(rect.corner_radius)
                        }
                        _ => None,
                    })
                })
            });
            if frame == 3 {
                let corners = backdrop_corners.expect("the modal layer must sample the backdrop");
                assert_eq!(
                    corners,
                    layout::body_corner_radius(&ctx),
                    "the blurred backdrop must use the content corners"
                );
                // macOS 一致：模糊之上还有一层 `--ol-overlay-bg` 压暗。
                let paints_tint = ctx.graphics(|graphics| {
                    graphics.get(layer).is_some_and(|list| {
                        list.all_entries().any(|entry| match &entry.shape {
                            egui::Shape::Rect(rect) => rect.fill == theme::OVERLAY,
                            _ => false,
                        })
                    })
                });
                assert!(paints_tint, "the modal layer must tint above the blur");
            }
            // 卡片矩形依然要写进 memory：遮罩换了材质，弹窗几何不能跟着变。
            assert!(ctx
                .data(|data| data
                    .get_temp::<egui::Rect>(egui::Id::new("openless-settings-card-rect")))
                .is_some());
            let _ = end_pass(&ctx);
        }
    }

    #[test]
    fn shrinking_without_clicking_rebuilds_the_content_viewport_immediately() {
        for page in [
            Page::Overview,
            Page::History,
            Page::QuickNote,
            Page::Translation,
        ] {
            let ctx = egui::Context::default();
            let mut vm = FrontendViewModel {
                active_page: page,
                ..Default::default()
            };
            for (width, height) in [(1500.0, 950.0), (960.0, 640.0)] {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, height),
                    )),
                    ..Default::default()
                });
                render(&ctx, &mut vm, &mut Vec::new());
                let available: egui::Vec2 = ctx.data(|data| {
                    data.get_temp(egui::Id::new("content-available-size"))
                        .unwrap()
                });
                let expected = layout::body_rect(&ctx).size()
                    - egui::vec2(
                        layout::SIDEBAR_WIDTH + 30.0,
                        layout::PAGE_TOP_PADDING + layout::PAGE_BOTTOM_PADDING,
                    );
                assert!((available.x - expected.x).abs() < 1.0,
                    "{page:?} still uses old width after resize: {available:?}, expected {expected:?}");
                assert!((available.y - expected.y).abs() < 1.0,
                    "{page:?} still uses old height after resize: {available:?}, expected {expected:?}");
                let _ = end_pass(&ctx);
            }
        }
    }

    #[test]
    fn history_and_quick_note_remain_side_by_side_at_the_window_minimum() {
        for page in [Page::History, Page::QuickNote] {
            let ctx = egui::Context::default();
            let mut vm = FrontendViewModel {
                active_page: page,
                ..Default::default()
            };
            for size in [egui::vec2(1300.0, 835.0), egui::vec2(960.0, 640.0)] {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                });
                render(&ctx, &mut vm, &mut Vec::new());
                let _ = end_pass(&ctx);
            }
            let (list, detail): (egui::Rect, egui::Rect) = ctx.data(|data| {
                data.get_temp(egui::Id::new("history-column-rects"))
                    .unwrap()
            });
            assert!(
                detail.left() >= list.right(),
                "{page:?} stacked at minimum: {list:?} / {detail:?}"
            );
            assert!(
                detail.right() <= layout::body_rect(&ctx).right(),
                "{page:?} detail clipped: {detail:?}"
            );
            let visible_bottom = layout::body_rect(&ctx).bottom() - layout::PAGE_BOTTOM_PADDING;
            assert!(
                detail.bottom() <= visible_bottom + 1.0,
                "{page:?} detail extends under bottom gutter: {detail:?}, bottom={visible_bottom}"
            );
        }
    }

    #[test]
    fn history_and_quick_note_scroll_both_columns_at_minimum_window_height() {
        for page in [Page::History, Page::QuickNote] {
            for (pointer, measure) in [
                (egui::pos2(360.0, 400.0), "history-list-scroll-measure"),
                (egui::pos2(670.0, 400.0), "history-detail-scroll-measure"),
            ] {
                let ctx = egui::Context::default();
                let mut vm = FrontendViewModel {
                    active_page: page,
                    quick_note_shortcut_hidden: true,
                    history_loading: false,
                    history_entries: (0..25)
                        .map(|n| super::view_model::HistoryEntry {
                            id: n.to_string(),
                            quick_note: page == Page::QuickNote,
                            created_at: "2026-01-15T12:34:00+00:00".into(),
                            raw_transcript: "long sentence for scroll testing ".repeat(90),
                            final_text: "final paragraph ".repeat(60),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                };
                for frame in 0..6 {
                    let size = if frame == 0 {
                        egui::vec2(1300.0, 835.0)
                    } else {
                        egui::vec2(960.0, 640.0)
                    };
                    let mut events = vec![egui::Event::PointerMoved(pointer)];
                    if frame >= 2 {
                        events.push(egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -180.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        });
                    }
                    ctx.begin_pass(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    });
                    render(&ctx, &mut vm, &mut Vec::new());
                    let _ = end_pass(&ctx);
                }
                let (content, visible, offset): (f32, f32, f32) =
                    ctx.data(|data| data.get_temp(egui::Id::new(measure)).unwrap());
                assert!(
                    content > visible + 10.0,
                    "{page:?} {measure} no scroll extent: {content}/{visible}"
                );
                assert!(
                    offset > 10.0,
                    "{page:?} {measure} did not scroll: {content}/{visible}, offset={offset}"
                );
            }
        }
    }

    /// 复现「滚不动，点一下页面才行」：窗口被合成器拖动/缩放后，egui 手里的指针
    /// 坐标停在按下那一刻（Wayland 交互式移动/缩放期间客户端不再收到 motion），
    /// 用户其实是在页面上滚轮，egui 却以为指针还在标题栏上 —— `ScrollArea` 的
    /// `rect_contains_pointer` 判定失败，滚轮整帧被丢掉。
    #[test]
    fn a_stale_pointer_must_not_swallow_the_page_wheel() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            active_page: Page::Translation,
            translation_unsupported: false,
            ..Default::default()
        };
        for frame in 0..12 {
            let mut events = Vec::new();
            if frame < 2 {
                events.push(egui::Event::PointerMoved(egui::pos2(550.0, 390.0)));
            }
            if frame == 2 {
                // 按标题栏拖窗：app 在按下那一帧就把指针交给合成器（StartDrag）。
                events.push(egui::Event::PointerMoved(egui::pos2(400.0, 18.0)));
                events.push(egui::Event::PointerButton {
                    pos: egui::pos2(400.0, 18.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            if frame >= 3 {
                // 拖动结束后客户端再没收到指针事件，用户就地滚滚轮。
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -180.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            frame_with_pointer_routing(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(viewport()),
                    events,
                    time: Some(frame as f64 / 10.0),
                    ..Default::default()
                },
                &mut vm,
            );
        }
        let (content, visible, offset): (f32, f32, f32) = ctx.data(|data| {
            data.get_temp(egui::Id::new("main-scroll-measure"))
                .expect("the translation page must have a scroll container")
        });
        assert!(
            content > visible,
            "the page must be scrollable: {content} / {visible}"
        );
        assert!(
            offset > 10.0,
            "a stale pointer swallowed the wheel: {content} / {visible}, offset={offset}"
        );
    }

    /// 跑一帧，并且像 `eframe::App::raw_input_hook` 那样先修一次指针状态
    /// （测试里没有 eframe，只能手动走同一条路径，否则修的东西测不到）。
    fn frame_with_pointer_routing(
        ctx: &egui::Context,
        mut raw_input: egui::RawInput,
        vm: &mut FrontendViewModel,
    ) {
        layout::route_pointer_before_pass(ctx, &mut raw_input);
        ctx.begin_pass(raw_input);
        render(ctx, vm, &mut Vec::new());
        let _ = end_pass(ctx);
    }

    /// 用户报的复现路径：打开概览页 → 从右下角把窗口拖到最小 → 不再动鼠标就地滚动。
    /// 四角的拉伸条是 app 自己画的（`BeginResize`），按下那一帧就把这次手势交给了
    /// 合成器：释放回不来、缩放期间也没有 motion，egui 于是留着「正在拖拉伸条」的状态，
    /// `ScrollArea` 就不吃滚轮了。
    #[test]
    fn resizing_from_the_corner_does_not_leave_the_page_unscrollable() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            active_page: Page::Overview,
            overview_loading: false,
            ..Default::default()
        };
        vm.settings.activity_heatmap = true;
        vm.overview = Some(super::view_model::OverviewSummary {
            asr_provider: "asr".into(),
            llm_provider: "llm".into(),
            asr_configured: true,
            llm_configured: true,
            chars_today: 42,
            segments_today: 3,
            duration_ms_today: 12_000,
            avg_latency_ms: 1200,
            history_total: 20,
            recent: Vec::new(),
            activity_daily: Vec::new(),
            heatmap_year: 2026,
            heatmap: (0..365)
                .map(|day| super::view_model::OverviewHeatmapDay {
                    date: format!("2026-01-{day}"),
                    count: 1,
                })
                .collect(),
        });
        let large = egui::vec2(1240.0, 800.0);
        let small = egui::vec2(960.0, 640.0);
        // 右下角拉伸条：`window_rect` 是客户区内缩 6px，角上 18px 属于 SouthEast。
        let grip = egui::pos2(large.x - 15.0, large.y - 15.0);
        for frame in 0..18 {
            let mut events = Vec::new();
            if frame < 3 {
                events.push(egui::Event::PointerMoved(grip));
            }
            if frame == 3 {
                events.push(egui::Event::PointerButton {
                    pos: grip,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            if frame >= 4 {
                // 用户缩完窗口就地滚：合成器既不还释放，也不再发 motion。
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -180.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let size = if frame >= 4 { small } else { large };
            frame_with_pointer_routing(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    time: Some(frame as f64 / 10.0),
                    ..Default::default()
                },
                &mut vm,
            );
        }
        let (content, visible, offset): (f32, f32, f32) = ctx.data(|data| {
            data.get_temp(egui::Id::new("overview-scroll-measure"))
                .expect("the overview must offer page scrolling at the minimum size")
        });
        assert!(
            content > visible,
            "overview did not offer scrolling: {content}/{visible}"
        );
        assert!(
            offset > 10.0,
            "a corner resize swallowed the wheel: {content}/{visible}, offset={offset}"
        );
    }

    #[test]
    fn pointer_routing_only_guesses_when_the_pointer_is_not_in_the_body() {
        let ctx = egui::Context::default();
        // 指针在主体里：滚轮照旧走 egui 自己的命中判定，不要猜。
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            events: vec![egui::Event::PointerMoved(egui::pos2(600.0, 400.0))],
            ..Default::default()
        });
        let _ = end_pass(&ctx);
        let mut raw = egui::RawInput {
            screen_rect: Some(viewport()),
            events: vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        layout::route_pointer_before_pass(&ctx, &mut raw);
        assert!(
            !raw.events
                .iter()
                .any(|event| matches!(event, egui::Event::PointerMoved(_))),
            "a pointer inside the body must be left alone"
        );

        // 指针停在标题栏上（合成器拖窗之后的典型状态）：滚轮证明指针在窗口里，
        // 补一个主体内的坐标，滚轮才有人接。
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            events: vec![egui::Event::PointerMoved(egui::pos2(400.0, 18.0))],
            ..Default::default()
        });
        let _ = end_pass(&ctx);
        let mut raw = egui::RawInput {
            screen_rect: Some(viewport()),
            events: vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        layout::route_pointer_before_pass(&ctx, &mut raw);
        let guessed = raw.events.iter().find_map(|event| match event {
            egui::Event::PointerMoved(pos) => Some(*pos),
            _ => None,
        });
        let body = layout::body_rect(&ctx);
        assert!(
            guessed.is_some_and(|pos| body.contains(pos)),
            "a stale pointer must be moved into the body: {guessed:?}"
        );
    }

    #[test]
    fn an_orphaned_press_is_released_once_the_window_manager_keeps_it() {
        let ctx = egui::Context::default();
        // 按下标题栏：app 会把这次手势交给合成器。
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            time: Some(1.0),
            events: vec![
                egui::Event::PointerMoved(egui::pos2(400.0, 18.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(400.0, 18.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        });
        layout::note_window_gesture_handoff(&ctx);
        let _ = end_pass(&ctx);
        assert!(ctx.input(|input| input.pointer.any_down()));

        // 刚交出去就补释放会把「按住标题栏」也打断，所以先等一会儿。
        let mut early = egui::RawInput {
            screen_rect: Some(viewport()),
            time: Some(1.1),
            ..Default::default()
        };
        layout::route_pointer_before_pass(&ctx, &mut early);
        assert!(!early
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerButton { pressed: false, .. })));

        // 合成器一直没把释放还回来：这次手势已经不属于 egui，补上释放。
        // （`raw_input_hook` 读的是上一帧的 `input().time`，所以先把时钟推过去。）
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            time: Some(2.0),
            ..Default::default()
        });
        let _ = end_pass(&ctx);
        let mut late = egui::RawInput {
            screen_rect: Some(viewport()),
            time: Some(2.1),
            ..Default::default()
        };
        layout::route_pointer_before_pass(&ctx, &mut late);
        assert!(late
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::PointerButton { pressed: false, .. })));
    }

    #[test]
    fn overview_and_translation_scroll_at_minimum_window_height() {
        for page in [Page::Overview, Page::Translation] {
            let ctx = egui::Context::default();
            let mut vm = FrontendViewModel {
                active_page: page,
                translation_unsupported: false,
                overview_loading: false,
                ..Default::default()
            };
            if page == Page::Overview {
                vm.settings.activity_heatmap = true;
                vm.overview = Some(super::view_model::OverviewSummary {
                    asr_provider: "asr".into(),
                    llm_provider: "llm".into(),
                    asr_configured: true,
                    llm_configured: true,
                    chars_today: 42,
                    segments_today: 3,
                    duration_ms_today: 12_000,
                    avg_latency_ms: 1200,
                    history_total: 20,
                    recent: Vec::new(),
                    activity_daily: Vec::new(),
                    heatmap_year: 2026,
                    heatmap: (0..365)
                        .map(|day| super::view_model::OverviewHeatmapDay {
                            date: format!("2026-01-{day}"),
                            count: 1,
                        })
                        .collect(),
                });
            }
            let large = egui::vec2(1300.0, 835.0);
            let small = egui::vec2(960.0, 640.0);
            for frame in 0..6 {
                let size = if frame == 0 { large } else { small };
                let events = if frame >= 2 {
                    vec![
                        egui::Event::PointerMoved(egui::pos2(550.0, 390.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -180.0),
                            phase: egui::TouchPhase::Move,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    vec![egui::Event::PointerMoved(egui::pos2(550.0, 390.0))]
                };
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                });
                render(&ctx, &mut vm, &mut Vec::new());
                let _ = end_pass(&ctx);
                if frame == 5 {
                    let id = if page == Page::Overview {
                        "overview-scroll-measure"
                    } else {
                        "main-scroll-measure"
                    };
                    let (content, visible, offset): (f32, f32, f32) = ctx.data(|data| {
                        data.get_temp(egui::Id::new(id))
                            .expect("scroll container must exist")
                    });
                    assert!(
                        content > visible + 10.0,
                        "{page:?} has no scroll extent: {content} / {visible}"
                    );
                    assert!(
                        offset > 10.0,
                        "{page:?} did not scroll: {content} / {visible}, offset={offset}"
                    );
                }
            }
        }
    }

    #[test]
    fn translation_scroll_position_is_not_shared_with_other_pages() {
        fn paint(ctx: &egui::Context, vm: &mut FrontendViewModel, events: Vec<egui::Event>) -> f32 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(960.0, 640.0),
                )),
                events,
                ..Default::default()
            });
            render(ctx, vm, &mut Vec::new());
            let _ = end_pass(ctx);
            ctx.data(|data| {
                data.get_temp::<(f32, f32, f32)>(egui::Id::new("main-scroll-measure"))
                    .unwrap()
                    .2
            })
        }
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            active_page: Page::Translation,
            translation_unsupported: false,
            ..Default::default()
        };
        paint(
            &ctx,
            &mut vm,
            vec![egui::Event::PointerMoved(egui::pos2(530.0, 360.0))],
        );
        let mut scrolled = 0.0;
        for _ in 0..3 {
            scrolled = paint(
                &ctx,
                &mut vm,
                vec![
                    egui::Event::PointerMoved(egui::pos2(530.0, 360.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -130.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(scrolled > 1.0);
        vm.active_page = Page::Marketplace;
        paint(&ctx, &mut vm, Vec::new());
        let other = paint(&ctx, &mut vm, Vec::new());
        assert!(
            other < 1.0,
            "marketplace inherited translation scroll offset: {other}"
        );
        vm.active_page = Page::Translation;
        let restored = paint(&ctx, &mut vm, Vec::new());
        assert!(
            restored > 10.0,
            "translation lost its scroll position after switching pages: {scrolled} -> {restored}"
        );
    }

    #[test]
    fn translation_page_stays_inside_its_viewport_and_scrolls_after_a_shrink() {
        // Regression for "翻译页缩放卡顿、使用方法被遮挡且无法滚动": the page pinned
        // only `set_min_width`, so after shrinking the window egui kept laying the
        // two columns (and the four-column guide) out at the previous, larger
        // width — a growing-width feedback loop that pushed the guide out of the
        // scrollable area. The page must fit the viewport it is given at every
        // size, and the guide must stay reachable by scrolling.
        use openless_linux_egui::Lang;
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: Lang::ZhCn,
            active_page: Page::Translation,
            translation_unsupported: false,
            ..Default::default()
        };

        let mut scroll_needed = false;
        for (index, screen) in [
            egui::vec2(1400.0, 900.0),
            egui::vec2(1080.0, 700.0),
            egui::vec2(900.0, 520.0),
        ]
        .into_iter()
        .enumerate()
        {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                ..Default::default()
            });
            let viewport = egui::Rect::from_min_size(
                egui::pos2(220.0, 56.0),
                egui::vec2((screen.x - 250.0).max(80.0), (screen.y - 80.0).max(80.0)),
            );
            let mut measured = None;
            egui::Area::new(egui::Id::new(("translation-layout", index)))
                .fixed_pos(viewport.min)
                .show(&ctx, |ui| {
                    ui.set_min_size(viewport.size());
                    ui.set_max_size(viewport.size());
                    let output = egui::ScrollArea::vertical()
                        .id_salt("translation-layout-scroll")
                        .max_height(viewport.height())
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            translation::page(ui, &mut vm, &mut Vec::new());
                            ui.add_space(32.0);
                        });
                    measured = Some((output.content_size, output.inner_rect));
                });
            let (content, inner) = measured.expect("the page rendered into the area");
            let badge: egui::Rect = ctx.data(|data| {
                data.get_temp(egui::Id::new("translation-style-badge"))
                    .expect("translation style has a badge")
            });
            assert!(
                badge.width() <= 180.0 && badge.height() <= 24.0,
                "badge stretched: {badge:?}"
            );
            assert!(
                badge.right() <= viewport.right() && badge.left() >= viewport.left(),
                "badge must remain inside the resized viewport: {badge:?} / {viewport:?}"
            );
            assert!(
                content.x <= inner.width() + 1.0,
                "the page must not exceed its viewport: content {content:?}, inner {inner:?}"
            );
            if content.y > inner.height() {
                scroll_needed = true;
            }
        }
        assert!(
            scroll_needed,
            "at the smallest window the page must be taller than the viewport so the \
             usage guide stays reachable by scrolling"
        );
    }

    #[test]
    fn quick_note_header_has_refresh_but_no_start_or_finish_recording_button() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            quick_note_recording: true,
            ..Default::default()
        };
        let output = run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            },
            |ui| history::page(ui, &mut vm, &mut Vec::new(), true),
        );
        let text = painted_text(&output);
        assert!(text.contains(openless_linux_egui::tr_l10n(vm.lang, "common.refresh")));
        assert!(!text.contains(openless_linux_egui::tr_l10n(vm.lang, "quickNote.start")));
        assert!(!text.contains(openless_linux_egui::tr_l10n(vm.lang, "quickNote.finish")));
        assert!(!text.contains(openless_linux_egui::tr_l10n(vm.lang, "common.clear")));
    }

    #[test]
    fn fixed_ui_keeps_the_parent_cursor_in_place() {
        // Regression: `ui.scope_builder` rewinds the parent cursor to the
        // child's used rect, which made each card in a row pull the next row up
        // over itself. `layout::fixed_ui` must not move the parent cursor even
        // when the card body paints instead of allocating.
        let ctx = egui::Context::default();
        let _ = crate::ui::frontend::run_pass(
            &ctx,
            egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            },
            |ui| {
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
            },
        );
    }

    #[test]
    fn shell_paints_a_gray_sidebar_over_white_content_and_a_tinted_titlebar() {
        let ctx = egui::Context::default();
        frame(&ctx, Vec::new());
        frame(&ctx, Vec::new());
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut vm = FrontendViewModel::default();
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = crate::ui::frontend::end_pass(&ctx);

        let mut fills: Vec<(egui::Rect, egui::Color32)> = Vec::new();
        for clipped in &output.shapes {
            collect_rect_fills(&clipped.shape, &mut fills);
        }
        let window = layout::window_rect(&ctx);
        let body = layout::body_rect(&ctx);
        let sidebar_rect =
            egui::Rect::from_min_size(body.min, egui::vec2(layout::SIDEBAR_WIDTH, body.height()));
        let titlebar_rect = egui::Rect::from_min_max(
            window.min,
            egui::pos2(window.max.x, window.min.y + layout::TITLEBAR_HEIGHT),
        );
        // 入场动画会把 UI 层的颜色整体乘一个 alpha（存的是预乘色），所以比「源色」的
        // RGB 而不是比字面值。
        let painted_with = |rect: egui::Rect, color: egui::Color32| {
            let [want_r, want_g, want_b, _] = color.to_srgba_unmultiplied();
            fills.iter().any(|(painted, fill)| {
                if *painted != rect {
                    return false;
                }
                let [r, g, b, _] = fill.to_srgba_unmultiplied();
                // 反预乘会有一两点取整误差，所以给 ±3 的容差。
                (r as i32 - want_r as i32).abs() <= 3
                    && (g as i32 - want_g as i32).abs() <= 3
                    && (b as i32 - want_b as i32).abs() <= 3
            })
        };
        // 侧栏（左列）= 灰底；内容区底板 = 白底。
        // 这两个搞反就是用户报的「左右两侧的底色搞翻了」。
        assert!(
            painted_with(sidebar_rect, theme::SIDEBAR),
            "the sidebar column must be painted with the gray sidebar surface"
        );
        assert!(
            painted_with(body, theme::SURFACE),
            "the body must be painted white (content area), not canvas gray"
        );
        // 自绘标题栏比纯白更灰（Tauri 的 Linux 标题栏不是纯白）。
        assert!(
            painted_with(titlebar_rect, theme::TITLEBAR),
            "the titlebar strip must use the tinted titlebar surface"
        );
        assert_ne!(theme::TITLEBAR, theme::SURFACE);
        assert_ne!(theme::SIDEBAR, theme::SURFACE);
    }

    fn collect_rect_fills(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32)>) {
        match shape {
            egui::Shape::Rect(rect) => out.push((rect.rect, rect.fill)),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_rect_fills(shape, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn sidebar_navigation_receives_pointer_clicks_above_window_layers() {
        let ctx = egui::Context::default();

        // Areas use their first pass to establish their screen rectangles.
        frame(&ctx, Vec::new());
        frame(&ctx, Vec::new());

        // 侧栏顶部现在是两行版本信息（BETA 徽章 + 版本号），导航行的实际 y 会随
        // 字体度量漂移，所以扫一列而不是写死一个坐标：只要有一个位置能点出导航，
        // 就说明最上层的 resize 层没吃掉侧栏的点击。
        let mut navigated = false;
        let mut y = 80.0;
        while y <= 320.0 && !navigated {
            let pointer = egui::pos2(50.0, y);
            if y == 80.0 {
                assert_eq!(
                    ctx.layer_id_at(pointer),
                    Some(egui::LayerId::new(
                        egui::Order::Middle,
                        egui::Id::new("openless-sidebar"),
                    )),
                    "the sidebar must be the top input layer over its own column"
                );
            }
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
            navigated = actions
                .iter()
                .any(|action| matches!(action, FrontendAction::Navigate(_)));
            y += 4.0;
        }
        assert!(
            navigated,
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
            icon_path: None,
            icon_data_url: None,
            base_mode: "light".into(),
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
            let _ = crate::ui::frontend::end_pass(&ctx);
        }
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(viewport()),
            ..Default::default()
        });
        let mut actions = Vec::new();
        render(&ctx, &mut vm, &mut actions);
        let output = crate::ui::frontend::end_pass(&ctx);
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
                let _ = crate::ui::frontend::end_pass(&ctx);
            }
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut vm, &mut actions);
            let _ = crate::ui::frontend::end_pass(&ctx);
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
        let _ = crate::ui::frontend::end_pass(&ctx);
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
    /// 弹窗卡片相对遮罩区的位置。上游 Beta.2 里市场详情/历史确认是居中的
    /// `Modal`，风格编辑器是贴右边的抽屉（`top/right/bottom: 16`，
    /// `width: min(760px, 100vw - 32px)`）。
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum CardPlacement {
        Centred,
        RightDocked,
    }

    fn assert_overlay_keeps_the_card_on_top(
        ctx: &egui::Context,
        vm: &mut FrontendViewModel,
        area_id: &str,
        card_rect_key: &str,
        placement: CardPlacement,
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
        match placement {
            CardPlacement::Centred => {
                assert!(
                    (card.center().x - overlay.center().x).abs() <= 1.5,
                    "card must be horizontally centred: {card:?} in {overlay:?}"
                );
                assert!(
                    (card.center().y - overlay.center().y).abs() <= 1.5,
                    "card must be vertically centred: {card:?} in {overlay:?}"
                );
            }
            CardPlacement::RightDocked => {
                assert!(
                    (card.right() - (overlay.right() - 16.0)).abs() <= 1.5,
                    "the drawer must dock 16px from the right edge: {card:?} in {overlay:?}"
                );
                assert!(
                    (card.left() - (overlay.right() - 16.0 - 760.0)).abs() <= 1.5,
                    "the drawer must be min(760px, …) wide: {card:?} in {overlay:?}"
                );
                assert!(
                    (card.top() - (overlay.top() + 16.0)).abs() <= 1.5
                        && (card.bottom() - (overlay.bottom() - 16.0)).abs() <= 1.5,
                    "the drawer must keep a 16px inset vertically: {card:?} in {overlay:?}"
                );
            }
        }
        assert_eq!(
            ctx.layer_id_at(card.center()),
            Some(layer),
            "{area_id}: the card must live in the modal's own layer"
        );
        // 遮罩探针：选一个肯定落在遮罩上、不落在卡片上的点。
        let mask = match placement {
            CardPlacement::Centred => egui::pos2(overlay.center().x, overlay.top() + 6.0),
            // 抽屉贴右，遮罩左缘 20px 处一定在遮罩上。
            CardPlacement::RightDocked => egui::pos2(overlay.left() + 20.0, overlay.center().y),
        };
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
            id: "overlay-fixture-id".to_string(),
            name: "overlay-fixture".to_string(),
            version: "1.0.0".to_string(),
            description: "fixture for the marketplace detail overlay test".to_string(),
            mode: "dictation".to_string(),
            author: "tester".to_string(),
            origin_author_login: None,
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
            CardPlacement::Centred,
        );
    }

    #[test]
    fn marketplace_mine_modal_matches_the_tauri_header_geometry() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            lang: openless_linux_egui::Lang::ZhCn,
            active_page: Page::Marketplace,
            marketplace_loading: false,
            marketplace_unsupported: false,
            marketplace_mine_open: true,
            marketplace_mine_loading: false,
            marketplace_signed_in: false,
            ..Default::default()
        };
        for _ in 0..2 {
            overlay_frame(&ctx, &mut vm, Vec::new());
        }
        let body = layout::body_rect(&ctx);
        let card = ctx
            .data(|data| {
                data.get_temp::<egui::Rect>(egui::Id::new("openless-marketplace-mine-card-rect"))
            })
            .expect("the my-packs card rect must be published while open");
        assert!(body.contains_rect(card), "card={card:?} body={body:?}");
        assert!((card.width() - 560.0).abs() <= 2.0);
        assert!((card.height() - 300.0).abs() <= 2.0);
        assert_eq!(
            ctx.layer_id_at(card.center()),
            Some(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("openless-marketplace-mine-modal"),
            ))
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
            CardPlacement::RightDocked,
        );
    }

    #[test]
    fn style_editor_uses_the_published_live_blur_under_its_tint() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            active_page: Page::Style,
            style_unsupported: false,
            style_editor_open: true,
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            crate::ui::backdrop::publish(&ctx, Some(egui::TextureId::User(42)));
            render(&ctx, &mut vm, &mut Vec::new());
            output = Some(end_pass(&ctx));
        }
        let output = output.unwrap();
        let body = layout::body_rect(&ctx);
        fn collect_textured(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::TextureId)>) {
            match shape {
                egui::Shape::Rect(rect) => {
                    if let Some(brush) = &rect.brush {
                        out.push((rect.rect, brush.fill_texture_id));
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_textured(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut textured = Vec::new();
        for clipped in &output.shapes {
            collect_textured(&clipped.shape, &mut textured);
        }
        assert!(
            textured.iter().any(|(rect, texture)| {
                *texture == egui::TextureId::User(42)
                    && *rect
                        == egui::Rect::from_min_max(
                            egui::pos2(body.left() + layout::SIDEBAR_WIDTH, body.top()),
                            body.max,
                        )
            }),
            "style editor should draw the current-frame blurred backdrop: {textured:?}"
        );
    }

    #[test]
    fn style_editor_form_starts_below_header_and_inside_drawer() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            active_page: Page::Style,
            style_unsupported: false,
            style_editor_open: true,
            style_name: "GEOMETRY_NAME_SENTINEL".into(),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            render(&ctx, &mut vm, &mut Vec::new());
            output = Some(end_pass(&ctx));
        }
        let output = output.unwrap();
        let card: egui::Rect = ctx.data(|data| {
            data.get_temp(egui::Id::new("openless-style-editor-card-rect"))
                .unwrap()
        });
        let scroll: egui::Rect = ctx.data(|data| {
            data.get_temp(egui::Id::new("style-editor-scroll-rect"))
                .unwrap()
        });
        assert!(
            scroll.top() >= card.top() + 78.0 - 1.0,
            "form overlaps the fixed header: {scroll:?}, {card:?}"
        );
        assert!(
            scroll.bottom() <= card.bottom() + 1.0,
            "form extends beyond drawer: {scroll:?}, {card:?}"
        );
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(text) => out.push((text.galley.text().to_string(), text.pos)),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut text);
        }
        let label = |key| openless_linux_egui::tr_l10n(vm.lang, key);
        let name = label("style.pack.fieldName");
        let description = label("style.pack.fieldDescription");
        let prompt = label("style.pack.dictation_prompt_title");
        let targets = [
            name,
            label("style.pack.fieldAuthor"),
            label("style.pack.fieldVersion"),
            label("style.pack.fieldTags"),
            description,
            label("style.pack.fieldModel"),
            prompt,
            "GEOMETRY_NAME_SENTINEL",
        ];
        let positions: Vec<_> = text
            .iter()
            .filter(|(s, _)| targets.contains(&s.as_str()))
            .collect();
        assert!(positions.len() >= 7, "missing editor fields: {positions:?}");
        assert!(
            positions
                .iter()
                .all(|(_, p)| p.x >= card.left() + 16.0 && p.y >= scroll.top()),
            "fields escape inset or overlap header: {positions:?}; {card:?}, {scroll:?}"
        );
        let position = |label: &str| positions.iter().find(|(s, _)| s == label).unwrap().1;
        assert!(position(name).y < scroll.top() + 120.0);
        assert!((position(name).y - position(label("style.pack.fieldAuthor")).y).abs() < 2.0);
        assert!((position(name).y - position(label("style.pack.fieldVersion")).y).abs() < 2.0);
        assert!(position(label("style.pack.fieldTags")).y > position(name).y + 40.0);
        assert!(position(description).y < scroll.top() + 290.0);
        assert!(
            position(prompt).y < scroll.bottom(),
            "prompt should be visible without scrolling past a blank header"
        );
        fn collect_inputs(shape: &egui::Shape, borders: &mut Vec<(egui::Rect, egui::Stroke)>) {
            match shape {
                egui::Shape::Rect(rect)
                    if rect.rect.width() > 180.0 && rect.rect.height() >= 30.0 =>
                {
                    borders.push((rect.rect, rect.stroke));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_inputs(shape, borders);
                    }
                }
                _ => {}
            }
        }
        let mut borders = Vec::new();
        for clipped in &output.shapes {
            collect_inputs(&clipped.shape, &mut borders);
        }
        let editor_borders: Vec<_> = borders
            .iter()
            .filter(|(rect, stroke)| {
                rect.left() >= card.left() + 16.0
                    && rect.width() < card.width() - 20.0
                    && card.contains(rect.center())
                    && stroke.width >= 0.5
                    && stroke.color.a() >= 50
            })
            .collect();
        assert!(
            editor_borders.len() >= 7,
            "editor inputs and textareas need visible outlines, got {borders:?}"
        );
        assert!(
            editor_borders
                .iter()
                .all(|(rect, _)| rect.right() <= card.right() + 1.0),
            "editor content must keep a right inset: {editor_borders:?}, card={card:?}"
        );
    }

    #[test]
    fn style_editor_long_text_scrolls_within_fixed_height_inputs() {
        fn measure(selection: bool, long: bool) -> (f32, Vec<(f32, f32, f32)>) {
            let ctx = egui::Context::default();
            let paragraphs = (0..100)
                .map(|i| format!("paragraph {i}: some long text to wrap across the field width\n"))
                .collect::<String>();
            let mut vm = FrontendViewModel {
                active_page: Page::Style,
                style_unsupported: false,
                style_editor_open: true,
                style_selection_workflow: selection,
                style_description: if long {
                    paragraphs.clone()
                } else {
                    String::new()
                },
                style_prompt: if long {
                    paragraphs.clone()
                } else {
                    String::new()
                },
                style_selection_prompt: if long {
                    paragraphs.clone()
                } else {
                    String::new()
                },
                style_voice_edit_prompt: if long { paragraphs } else { String::new() },
                ..Default::default()
            };
            for _ in 0..2 {
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(viewport()),
                    ..Default::default()
                });
                render(&ctx, &mut vm, &mut Vec::new());
                let _ = end_pass(&ctx);
            }
            let content = ctx.data(|data| {
                data.get_temp::<f32>(egui::Id::new("style-editor-scroll-content-height"))
                    .unwrap()
            });
            let fields = if selection {
                ["description", "selection-prompt", "voice-edit-prompt"].to_vec()
            } else {
                ["description", "dictation-prompt"].to_vec()
            };
            let measures = fields
                .iter()
                .map(|id| {
                    ctx.data(|data| {
                        data.get_temp::<(f32, f32, f32)>(egui::Id::new((
                            "style-editor-textarea-measure",
                            id,
                        )))
                        .unwrap()
                    })
                })
                .collect();
            (content, measures)
        }
        for selection in [false, true] {
            let (empty_height, empty) = measure(selection, false);
            let (long_height, long) = measure(selection, true);
            assert!(
                (long_height - empty_height).abs() < 10.0,
                "long text stretched the drawer: {empty_height} -> {long_height}"
            );
            let expected = if selection {
                vec![64.0, 128.0, 128.0]
            } else {
                vec![64.0, 188.0]
            };
            for ((empty_field, long_field), expected_height) in
                empty.iter().zip(&long).zip(expected)
            {
                assert!(
                    (long_field.1 - expected_height).abs() < 2.0,
                    "wrong fixed field viewport: {long_field:?}"
                );
                assert!((empty_field.1 - long_field.1).abs() < 1.0);
                assert!(
                    long_field.0 > long_field.1 + 100.0,
                    "long text must remain scrollable inside input: {long_field:?}"
                );
            }
        }
    }

    #[test]
    fn style_editor_wheel_over_long_prompt_scrolls_input_not_drawer() {
        let ctx = egui::Context::default();
        let mut vm = FrontendViewModel {
            active_page: Page::Style,
            style_unsupported: false,
            style_editor_open: true,
            style_prompt: "long prompt line\n".repeat(100),
            ..Default::default()
        };
        for frame in 0..6 {
            let rect: egui::Rect = ctx
                .data(|data| {
                    data.get_temp(egui::Id::new((
                        "style-editor-textarea-rect",
                        "dictation-prompt",
                    )))
                })
                .unwrap_or(egui::Rect::from_min_size(
                    egui::pos2(750.0, 650.0),
                    egui::vec2(200.0, 100.0),
                ));
            let events = if frame >= 2 {
                vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -100.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else {
                Vec::new()
            };
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                events,
                ..Default::default()
            });
            render(&ctx, &mut vm, &mut Vec::new());
            let _ = end_pass(&ctx);
        }
        let (_, visible, offset): (f32, f32, f32) = ctx.data(|data| {
            data.get_temp(egui::Id::new((
                "style-editor-textarea-measure",
                "dictation-prompt",
            )))
            .unwrap()
        });
        assert!(
            offset > 20.0,
            "wheel did not scroll the bounded prompt: visible={visible}, offset={offset}"
        );
        let outer: f32 = ctx.data(|data| {
            data.get_temp(egui::Id::new("style-editor-scroll-offset"))
                .unwrap()
        });
        assert!(
            outer < 2.0,
            "wheel over the prompt scrolled the drawer: {outer}"
        );
    }

    /// Editor parity: upstream renders a right-hand drawer whose body follows the
    /// workflow switch (dictation prompt vs. the two selection prompts) and whose
    /// destructive button is "reset built-in" or "delete imported" depending on
    /// the pack kind. Both must reach Core, so they are distinct actions.
    #[test]
    fn style_editor_drawer_follows_the_workflow_and_the_pack_kind() {
        let zh = openless_linux_egui::Lang::ZhCn;
        let ctx = egui::Context::default();

        let mut dictation = FrontendViewModel {
            lang: zh,
            active_page: Page::Style,
            style_unsupported: false,
            style_editor_open: true,
            style_editor_id: "builtin-light".to_string(),
            style_editor_builtin: true,
            style_editor_dirty: true,
            ..Default::default()
        };
        let mut lines = Vec::new();
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut dictation, &mut actions);
            lines = painted_text(&crate::ui::frontend::end_pass(&ctx))
                .lines()
                .map(|line| line.trim().to_string())
                .collect();
        }
        let has = |key: &'static str| {
            let label = openless_linux_egui::tr_l10n(zh, key);
            assert!(
                lines.iter().any(|line| line == label),
                "{key} ({label:?}) must be painted in {lines:?}"
            );
        };
        has("style.pack.editorTitle");
        has("style.pack.dictationPromptEditorDesc");
        has("style.pack.dictation_prompt_title");
        has("style.pack.resetBuiltin");
        has("style.pack.unsaved");
        assert!(
            !lines.iter().any(|line| {
                line == openless_linux_egui::tr_l10n(zh, "style.pack.voiceEditPromptTitle")
            }),
            "the dictation workflow must not show the selection-voice prompt"
        );
        assert!(
            !lines.iter().any(|line| {
                line == openless_linux_egui::tr_l10n(zh, "style.pack.deleteImported")
            }),
            "a built-in pack is reset, never deleted"
        );

        // Same drawer, selection workflow, imported pack.
        let mut selection = FrontendViewModel {
            lang: zh,
            active_page: Page::Style,
            style_unsupported: false,
            style_editor_open: true,
            style_editor_id: "custom-legal".to_string(),
            style_editor_builtin: false,
            style_selection_workflow: true,
            ..Default::default()
        };
        let mut selection_lines = Vec::new();
        for frame in 0..6 {
            let events = if frame >= 2 {
                vec![
                    egui::Event::PointerMoved(egui::pos2(1000.0, 550.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -220.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else {
                Vec::new()
            };
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(viewport()),
                events,
                ..Default::default()
            });
            let mut actions = Vec::new();
            render(&ctx, &mut selection, &mut actions);
            selection_lines.extend(
                painted_text(&crate::ui::frontend::end_pass(&ctx))
                    .lines()
                    .map(|line| line.trim().to_string()),
            );
        }
        lines = selection_lines;
        let has_selection = |key: &'static str| {
            let label = openless_linux_egui::tr_l10n(zh, key);
            assert!(
                lines.iter().any(|line| line == label),
                "{key} ({label:?}) must be painted in {lines:?}"
            );
        };
        has_selection("style.pack.selectionPromptEditorDesc");
        has_selection("style.pack.selectionPromptTitle");
        has_selection("style.pack.voiceEditPromptTitle");
        has_selection("style.pack.deleteImported");
        assert!(
            !lines.iter().any(|line| {
                line == openless_linux_egui::tr_l10n(zh, "style.pack.resetBuiltin")
            }),
            "an imported pack is deleted, never reset"
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
            CardPlacement::Centred,
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
                icon = crate::ui::frontend::end_pass(&ctx)
                    .platform_output
                    .cursor_icon;
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
            painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
            painted = painted_text(&crate::ui::frontend::end_pass(&ctx));
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
        let mut painted;
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
            let output = crate::ui::frontend::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    super::settings::test_group_toggle(ui);
                },
            );
            painted = painted_text(&output);
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
            let output = crate::ui::frontend::end_pass(&ctx);
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
                let output = crate::ui::frontend::end_pass(&ctx);
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
