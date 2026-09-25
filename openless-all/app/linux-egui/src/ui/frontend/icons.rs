use eframe::egui;

use super::theme;

#[derive(Clone, Copy)]
pub enum IconName {
    Overview,
    History,
    Vocab,
    Style,
    SelectionAsk,
    Settings,
    Mic,
    Sparkle,
    Hash,
    Clock,
    Bolt,
    Copy,
    Search,
    Trash,
    Refresh,
    Download,
    Play,
    Stop,
    /// 浮窗用：关闭 ✕、确认 ✓、发送 ↑、空状态对话气泡、用户头像占位。
    Close,
    Check,
    Send,
    /// 划词追问头部的图钉（固定 / 取消固定）。
    Pin,
    Chat,
    Github,
    /// 横向三点（列表/详情行的「…」操作菜单）。
    More,
    ChevronRight,
}

/// Draw an icon centred at `center` with the given `color`.
pub fn draw_icon(ui: &egui::Ui, center: egui::Pos2, icon: IconName, color: egui::Color32) {
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.25, color);
    // 与中心点的相对坐标（各 arm 若需要不同缩放会在内部自行 shadow）。
    let point = |x: f32, y: f32| center + egui::vec2(x, y);
    match icon {
        IconName::Overview => {
            // Tauri 的概览图标是 lucide `ChartNoAxesColumn`：**只有三根柱子、没有坐标轴**。
            // 之前多画了一条 L 形坐标轴，和上游不是同一个图标。
            // lucide 24px 画布：柱子在 x=6/12/18，底线 y=20，顶端 y=14/4/10。
            let s = 20.0 / 24.0;
            for (x, top) in [(6.0_f32, 14.0_f32), (12.0, 4.0), (18.0, 10.0)] {
                p.line_segment(
                    [
                        center + egui::vec2((x - 12.0) * s, 8.0 * s),
                        center + egui::vec2((x - 12.0) * s, (top - 12.0) * s),
                    ],
                    stroke,
                );
            }
        }
        IconName::History | IconName::Clock => {
            p.circle_stroke(center, 6.0, stroke);
            p.line_segment([center, center + egui::vec2(0.0, -3.5)], stroke);
            p.line_segment([center, center + egui::vec2(3.0, 2.0)], stroke);
        }
        IconName::Search => {
            let s = 0.5;
            let stroke = egui::Stroke::new(1.0, color);
            p.circle_stroke(center + egui::vec2(-0.5, -0.5), 4.0, stroke);
            p.line_segment(
                [
                    center + egui::vec2(4.65 * s, 4.65 * s),
                    center + egui::vec2(9.0 * s, 9.0 * s),
                ],
                stroke,
            );
        }
        IconName::Trash => {
            let s = 0.54;
            let stroke = egui::Stroke::new(1.0, color);
            let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            p.line_segment([pt(3.0, 6.0), pt(21.0, 6.0)], stroke);
            p.line_segment([pt(19.0, 6.0), pt(19.0, 20.0)], stroke);
            p.line_segment([pt(19.0, 20.0), pt(17.0, 22.0)], stroke);
            p.line_segment([pt(17.0, 22.0), pt(7.0, 22.0)], stroke);
            p.line_segment([pt(7.0, 22.0), pt(5.0, 20.0)], stroke);
            p.line_segment([pt(5.0, 20.0), pt(5.0, 6.0)], stroke);
            p.line_segment([pt(8.0, 6.0), pt(8.0, 4.0)], stroke);
            p.add(egui::Shape::QuadraticBezier(
                egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [pt(8.0, 4.0), pt(8.0, 2.0), pt(10.0, 2.0)],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ),
            ));
            p.line_segment([pt(10.0, 2.0), pt(14.0, 2.0)], stroke);
            p.add(egui::Shape::QuadraticBezier(
                egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [pt(14.0, 2.0), pt(16.0, 2.0), pt(16.0, 4.0)],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ),
            ));
            p.line_segment([pt(16.0, 4.0), pt(16.0, 6.0)], stroke);
            p.line_segment([pt(10.0, 11.0), pt(10.0, 17.0)], stroke);
            p.line_segment([pt(14.0, 11.0), pt(14.0, 17.0)], stroke);
        }
        IconName::Refresh => {
            let s = 0.54;
            let stroke = egui::Stroke::new(1.0, color);
            let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            let arc = (0..=24)
                .map(|step| {
                    let t = step as f32 / 24.0;
                    let angle = std::f32::consts::PI
                        - t * (std::f32::consts::PI + std::f32::consts::FRAC_PI_2);
                    center + egui::vec2(angle.cos() * 9.0 * s, angle.sin() * 9.0 * s)
                })
                .collect::<Vec<_>>();
            p.add(egui::Shape::line(arc, stroke));
            p.line_segment([pt(3.0, 3.0), pt(3.0, 8.0)], stroke);
            p.line_segment([pt(3.0, 3.0), pt(8.0, 3.0)], stroke);
        }
        IconName::Download => {
            let s = 0.54;
            let stroke = egui::Stroke::new(1.0, color);
            let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            p.add(egui::Shape::line(
                [
                    pt(21.0, 15.0),
                    pt(21.0, 19.0),
                    pt(19.0, 21.0),
                    pt(5.0, 21.0),
                    pt(3.0, 19.0),
                    pt(3.0, 15.0),
                ]
                .to_vec(),
                stroke,
            ));
            p.add(egui::Shape::line(
                [pt(7.0, 10.0), pt(12.0, 15.0), pt(17.0, 10.0)].to_vec(),
                stroke,
            ));
            p.line_segment([pt(12.0, 15.0), pt(12.0, 3.0)], stroke);
        }
        IconName::Play => {
            let s = 0.54;
            let stroke = egui::Stroke::new(1.0, color);
            let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            p.add(egui::Shape::line(
                [pt(5.0, 3.0), pt(19.0, 12.0), pt(5.0, 21.0), pt(5.0, 3.0)].to_vec(),
                stroke,
            ));
        }
        IconName::Stop => {
            let s = 0.54;
            let stroke = egui::Stroke::new(1.0, color);
            let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            p.add(egui::Shape::line(
                [
                    pt(6.0, 6.0),
                    pt(18.0, 6.0),
                    pt(18.0, 18.0),
                    pt(6.0, 18.0),
                    pt(6.0, 6.0),
                ]
                .to_vec(),
                stroke,
            ));
        }
        IconName::Vocab => {
            let s = 2.0 / 3.0;
            let point = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            p.add(egui::Shape::QuadraticBezier(
                egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [point(4.0, 19.5), point(4.0, 17.0), point(6.5, 17.0)],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ),
            ));
            p.add(egui::Shape::line(
                [point(6.5, 17.0), point(20.0, 17.0)].to_vec(),
                stroke,
            ));
            p.add(egui::Shape::QuadraticBezier(
                egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [point(4.0, 19.5), point(4.0, 22.0), point(6.5, 22.0)],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ),
            ));
            p.add(egui::Shape::line(
                [
                    point(6.5, 22.0),
                    point(20.0, 22.0),
                    point(20.0, 4.0),
                    point(6.5, 4.0),
                ]
                .to_vec(),
                stroke,
            ));
            p.add(egui::Shape::QuadraticBezier(
                egui::epaint::QuadraticBezierShape::from_points_stroke(
                    [point(6.5, 4.0), point(4.0, 4.0), point(4.0, 6.5)],
                    false,
                    egui::Color32::TRANSPARENT,
                    stroke,
                ),
            ));
            p.line_segment([point(4.0, 6.5), point(4.0, 19.5)], stroke);
        }
        IconName::Style => {
            let s = 2.0 / 3.0;
            p.line_segment(
                [
                    center + egui::vec2(0.0, -10.0 * s),
                    center + egui::vec2(0.0, 10.0 * s),
                ],
                stroke,
            );
            p.add(egui::Shape::line(
                [
                    center + egui::vec2(5.0 * s, -7.0 * s),
                    center + egui::vec2(-2.5 * s, -7.0 * s),
                    center + egui::vec2(-5.5 * s, -5.0 * s),
                    center + egui::vec2(-5.5 * s, -1.5 * s),
                    center + egui::vec2(-3.5 * s, 1.5 * s),
                    center + egui::vec2(3.0 * s, 1.5 * s),
                    center + egui::vec2(5.0 * s, 3.5 * s),
                    center + egui::vec2(4.0 * s, 6.0 * s),
                    center + egui::vec2(1.0 * s, 7.0 * s),
                    center + egui::vec2(-6.0 * s, 7.0 * s),
                ]
                .to_vec(),
                stroke,
            ));
        }
        IconName::SelectionAsk => {
            p.rect_stroke(
                egui::Rect::from_center_size(
                    center + egui::vec2(0.0, -1.0),
                    egui::vec2(13.0, 10.0),
                ),
                egui::CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            p.line_segment(
                [
                    center + egui::vec2(-2.0, 4.0),
                    center + egui::vec2(-5.0, 7.0),
                ],
                stroke,
            );
        }
        IconName::Mic => {
            p.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(0.0, -2.0), egui::vec2(7.0, 11.0)),
                egui::CornerRadius::same(4),
                stroke,
                egui::StrokeKind::Inside,
            );
            p.line_segment(
                [
                    center + egui::vec2(-4.0, -2.0),
                    center + egui::vec2(-4.0, 1.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(4.0, -2.0),
                    center + egui::vec2(4.0, 1.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-4.0, 1.0),
                    center + egui::vec2(4.0, 1.0),
                ],
                stroke,
            );
            p.line_segment(
                [center + egui::vec2(0.0, 1.0), center + egui::vec2(0.0, 5.0)],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-3.0, 5.0),
                    center + egui::vec2(3.0, 5.0),
                ],
                stroke,
            );
        }
        IconName::Sparkle => {
            p.line_segment(
                [
                    center + egui::vec2(0.0, -7.0),
                    center + egui::vec2(2.5, -2.5),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(2.5, -2.5),
                    center + egui::vec2(7.0, 0.0),
                ],
                stroke,
            );
            p.line_segment(
                [center + egui::vec2(7.0, 0.0), center + egui::vec2(2.5, 2.5)],
                stroke,
            );
            p.line_segment(
                [center + egui::vec2(2.5, 2.5), center + egui::vec2(0.0, 7.0)],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(0.0, 7.0),
                    center + egui::vec2(-2.5, 2.5),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-2.5, 2.5),
                    center + egui::vec2(-7.0, 0.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-7.0, 0.0),
                    center + egui::vec2(-2.5, -2.5),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-2.5, -2.5),
                    center + egui::vec2(0.0, -7.0),
                ],
                stroke,
            );
        }
        IconName::Hash => {
            p.line_segment(
                [
                    center + egui::vec2(-6.0, -3.0),
                    center + egui::vec2(6.0, -3.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-6.0, 3.0),
                    center + egui::vec2(6.0, 3.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-2.0, -7.0),
                    center + egui::vec2(-4.0, 7.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(4.0, -7.0),
                    center + egui::vec2(2.0, 7.0),
                ],
                stroke,
            );
        }
        IconName::Bolt => {
            p.line_segment(
                [
                    center + egui::vec2(1.0, -8.0),
                    center + egui::vec2(-5.0, 1.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-5.0, 1.0),
                    center + egui::vec2(1.0, 1.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(1.0, 1.0),
                    center + egui::vec2(-1.0, 8.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(-1.0, 8.0),
                    center + egui::vec2(6.0, -1.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(6.0, -1.0),
                    center + egui::vec2(1.0, -1.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    center + egui::vec2(1.0, -1.0),
                    center + egui::vec2(1.0, -8.0),
                ],
                stroke,
            );
        }
        IconName::Close => {
            p.line_segment([point(-4.5, -4.5), point(4.5, 4.5)], stroke);
            p.line_segment([point(4.5, -4.5), point(-4.5, 4.5)], stroke);
        }
        IconName::Check => {
            p.add(egui::Shape::line(
                [point(-5.0, 0.5), point(-1.5, 4.0), point(5.0, -4.0)].to_vec(),
                egui::Stroke::new(1.6, color),
            ));
        }
        IconName::Send => {
            p.add(egui::Shape::line(
                [point(0.0, -5.5), point(0.0, 5.5)].to_vec(),
                egui::Stroke::new(1.6, color),
            ));
            p.add(egui::Shape::line(
                [point(-4.0, -1.5), point(0.0, -5.5), point(4.0, -1.5)].to_vec(),
                egui::Stroke::new(1.6, color),
            ));
        }
        IconName::Chat => {
            p.rect_stroke(
                egui::Rect::from_center_size(
                    center + egui::vec2(0.0, -1.0),
                    egui::vec2(18.0, 13.0),
                ),
                egui::CornerRadius::same(4),
                stroke,
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::line(
                [
                    center + egui::vec2(-3.0, 5.5),
                    center + egui::vec2(-1.0, 5.5),
                    center + egui::vec2(-4.0, 8.5),
                ]
                .to_vec(),
                stroke,
            ));
        }
        IconName::Pin => {
            p.circle_stroke(center + egui::vec2(0.0, -3.0), 3.4, stroke);
            p.add(egui::Shape::line(
                [point(-4.6, -6.6), point(4.6, -6.6)].to_vec(),
                stroke,
            ));
            p.add(egui::Shape::line(
                [point(0.0, 0.4), point(0.0, 7.0)].to_vec(),
                stroke,
            ));
        }
        IconName::Github => {
            p.circle_filled(center, 7.0, color.gamma_multiply(0.75));
            p.circle_filled(center + egui::vec2(0.0, 3.0), 3.4, theme::SURFACE_2);
        }
        IconName::More => {
            for offset in [-4.5_f32, 0.0, 4.5] {
                p.circle_filled(center + egui::vec2(offset, 0.0), 1.4, stroke.color);
            }
        }
        IconName::Copy => {
            p.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(1.5, 1.5), egui::vec2(10.0, 12.0)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(-1.5, -2.5), egui::vec2(8.0, 5.0)),
                egui::CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        IconName::Settings => {
            // Lucide Settings: closed gear silhouette and a separate centre opening,
            // rather than the old sun-like circle with eight disconnected rays.
            let points: Vec<_> = (0..32)
                .map(|step| {
                    let angle = step as f32 * std::f32::consts::TAU / 32.0;
                    let radius = if step % 4 == 1 || step % 4 == 2 {
                        7.4
                    } else {
                        5.7
                    };
                    center + egui::vec2(angle.cos(), angle.sin()) * radius
                })
                .collect();
            p.add(egui::Shape::closed_line(points, stroke));
            p.circle_stroke(center, 2.3, stroke);
        }
        IconName::ChevronRight => {
            p.line_segment([point(-2.5, -4.5), point(2.0, 0.0)], stroke);
            p.line_segment([point(2.0, 0.0), point(-2.5, 4.5)], stroke);
        }
    }
}
