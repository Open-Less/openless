use eframe::egui;

#[derive(Clone, Copy)]
pub enum IconName {
    Overview,
    History,
    Vocab,
    Style,
    SelectionAsk,
    Translation,
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
    ChevronDown,
}

/// Draw an icon centred at `center` with the given `color`.
pub fn draw_icon(ui: &egui::Ui, center: egui::Pos2, icon: IconName, color: egui::Color32) {
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.25, color);
    match icon {
        IconName::Overview => {
            let s = 2.0 / 3.0;
            p.add(egui::Shape::line(
                [
                    center + egui::vec2(-9.0 * s, -9.0 * s),
                    center + egui::vec2(-9.0 * s, 9.0 * s),
                    center + egui::vec2(9.0 * s, 9.0 * s),
                ]
                .to_vec(),
                stroke,
            ));
            for (x, top) in [(6.0, 9.0), (1.0, 5.0), (-4.0, 14.0)] {
                p.line_segment(
                    [
                        center + egui::vec2(x * s, 5.0 * s),
                        center + egui::vec2(x * s, (top - 12.0) * s),
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
        IconName::ChevronDown => {
            let s = 0.54;
            let stroke = egui::Stroke::new(1.0, color);
            let pt = |x: f32, y: f32| center + egui::vec2((x - 12.0) * s, (y - 12.0) * s);
            p.add(egui::Shape::line(
                [pt(6.0, 9.0), pt(12.0, 15.0), pt(18.0, 9.0)].to_vec(),
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
        IconName::Translation => {
            p.circle_stroke(center, 6.7, stroke);
            p.line_segment(
                [
                    center + egui::vec2(-6.7, 0.0),
                    center + egui::vec2(6.7, 0.0),
                ],
                stroke,
            );
            for sign in [-1.0, 1.0] {
                p.add(egui::Shape::line(
                    [
                        center + egui::vec2(0.0, -6.7),
                        center + egui::vec2(sign * 2.8, -4.0),
                        center + egui::vec2(sign * 3.3, 0.0),
                        center + egui::vec2(sign * 2.8, 4.0),
                        center + egui::vec2(0.0, 6.7),
                    ]
                    .to_vec(),
                    stroke,
                ));
            }
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
            p.circle_stroke(center, 4.5, stroke);
            for angle in [
                0.0,
                std::f32::consts::FRAC_PI_4,
                std::f32::consts::FRAC_PI_2,
                3.0 * std::f32::consts::FRAC_PI_4,
                std::f32::consts::PI,
                5.0 * std::f32::consts::FRAC_PI_4,
                3.0 * std::f32::consts::FRAC_PI_2,
                7.0 * std::f32::consts::FRAC_PI_4,
            ] {
                let direction = egui::vec2(angle.cos(), angle.sin());
                p.line_segment([center + direction * 5.0, center + direction * 7.0], stroke);
            }
        }
    }
}
