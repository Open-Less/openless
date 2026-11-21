//! Shape helpers egui lacks: content clipped to a rounded shell (CSS
//! `overflow: hidden` + `border-radius`), inset shadows and wide soft shadows.
use egui::{epaint, pos2, vec2, Color32, Pos2, Rect, Shape};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub pos: Pos2,
    pub uv: Pos2,
}

/// A fully rounded rect, clockwise on screen.
pub fn stadium(rect: Rect) -> Vec<Pos2> {
    let radius = rect.height().min(rect.width()) * 0.5;
    let caps = [
        (pos2(rect.right() - radius, rect.center().y), -0.5),
        (pos2(rect.left() + radius, rect.center().y), 0.5),
    ];
    const SEGMENTS: usize = 32;
    let mut points = Vec::with_capacity(2 * SEGMENTS + 2);
    for (center, start) in caps {
        for step in 0..=SEGMENTS {
            let angle = std::f32::consts::PI * (start + step as f32 / SEGMENTS as f32);
            points.push(center + radius * vec2(angle.cos(), angle.sin()));
        }
    }
    points
}

pub fn circle(center: Pos2, radius: f32) -> Vec<Pos2> {
    const SEGMENTS: usize = 64;
    (0..SEGMENTS)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / SEGMENTS as f32;
            center + radius * vec2(angle.cos(), angle.sin())
        })
        .collect()
}

fn cross(a: egui::Vec2, b: egui::Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Sutherland–Hodgman against a convex `clip` polygon, interpolating UVs.
pub fn clip_convex(subject: &[Vertex], clip: &[Pos2]) -> Vec<Vertex> {
    let area: f32 = (0..clip.len())
        .map(|i| cross(clip[i].to_vec2(), clip[(i + 1) % clip.len()].to_vec2()))
        .sum();
    let orientation = area.signum();
    let mut output = subject.to_vec();
    for i in 0..clip.len() {
        let a = clip[i];
        let edge = clip[(i + 1) % clip.len()] - a;
        if edge.length_sq() < 1e-8 {
            continue;
        }
        let input = std::mem::take(&mut output);
        let Some(&last) = input.last() else {
            break;
        };
        let side = |p: Pos2| orientation * cross(edge, p - a);
        let mut previous = last;
        for current in input {
            let (before, now) = (side(previous.pos), side(current.pos));
            if (before >= 0.0) != (now >= 0.0) {
                let t = before / (before - now);
                output.push(Vertex {
                    pos: previous.pos + (current.pos - previous.pos) * t,
                    uv: previous.uv + (current.uv - previous.uv) * t,
                });
            }
            if now >= 0.0 {
                output.push(current);
            }
            previous = current;
        }
    }
    output
}

/// A filled convex shape clipped to `shell`, anti-aliased by egui.
pub fn fill_clipped(shape: &[Pos2], shell: &[Pos2], fill: Color32) -> Shape {
    let subject: Vec<_> = shape.iter().map(|&pos| Vertex { pos, uv: pos }).collect();
    let points: Vec<_> = clip_convex(&subject, shell)
        .into_iter()
        .map(|vertex| vertex.pos)
        .collect();
    if points.len() < 3 {
        return Shape::Noop;
    }
    Shape::convex_polygon(points, fill, egui::Stroke::NONE)
}

/// A tinted texture quad clipped to `shell`.
pub fn image_clipped(texture: egui::TextureId, rect: Rect, shell: &[Pos2], tint: Color32) -> Shape {
    let quad = [
        (rect.left_top(), pos2(0.0, 0.0)),
        (rect.right_top(), pos2(1.0, 0.0)),
        (rect.right_bottom(), pos2(1.0, 1.0)),
        (rect.left_bottom(), pos2(0.0, 1.0)),
    ]
    .map(|(pos, uv)| Vertex { pos, uv });
    let polygon = clip_convex(&quad, shell);
    if polygon.len() < 3 {
        return Shape::Noop;
    }
    let mut mesh = epaint::Mesh::with_texture(texture);
    for vertex in &polygon {
        mesh.vertices.push(epaint::Vertex {
            pos: vertex.pos,
            uv: vertex.uv,
            color: tint,
        });
    }
    for index in 1..polygon.len() as u32 - 1 {
        mesh.add_triangle(0, index, index + 1);
    }
    Shape::mesh(mesh)
}

/// Standard normal CDF (Abramowitz–Stegun 7.1.26).
fn normal_cdf(x: f32) -> f32 {
    let z = x.abs() / std::f32::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))));
    let erf = 1.0 - poly * (-z * z).exp();
    0.5 * (1.0 + erf.copysign(x))
}

/// Signed distance to a rounded rect; negative inside.
pub fn rounded_rect_distance(p: Pos2, rect: Rect, radius: f32) -> f32 {
    let radius = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    let q = (p - rect.center()).abs() - (rect.size() * 0.5 - vec2(radius, radius));
    q.max(egui::Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius
}

/// Coverage of CSS `inset 0 <offset_y> <blur> <spread>` at `p` inside the
/// rounded padding box: the blurred hole is approximated from its distance.
pub fn inset_coverage(
    p: Pos2,
    padding_box: Rect,
    radius: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
) -> f32 {
    let hole = padding_box.translate(vec2(0.0, offset_y)).shrink(spread);
    let sigma = (blur * 0.5).max(0.3);
    normal_cdf(rounded_rect_distance(p, hole, (radius - spread).max(0.0)) / sigma)
}

/// An inset shadow as a mesh over the rounded padding box, one row per
/// physical pixel and dense columns near the ends. The outline itself is not
/// anti-aliased: callers keep it under a border.
#[allow(clippy::too_many_arguments)]
pub fn inset_shadow(
    padding_box: Rect,
    radius: f32,
    pixels_per_point: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    color: Color32,
) -> Shape {
    const OFFSETS: [f32; 17] = [
        0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 13.0, 16.0, 20.0, 25.0, 32.0, 40.0,
    ];
    let rect = padding_box;
    let radius = radius.min(rect.height() * 0.5).min(rect.width() * 0.5);
    let inset = |y: f32| {
        let dy = if y < rect.top() + radius {
            rect.top() + radius - y
        } else if y > rect.bottom() - radius {
            y - (rect.bottom() - radius)
        } else {
            0.0
        };
        radius - (radius * radius - dy * dy).max(0.0).sqrt()
    };
    let columns = 2 * OFFSETS.len() as u32 + 1;
    let rows = (rect.height() * pixels_per_point).ceil().max(1.0) as u32;
    let mut mesh = epaint::Mesh::default();
    for row in 0..=rows {
        let y = (rect.top() + row as f32 / pixels_per_point).min(rect.bottom());
        let (left, right) = (rect.left() + inset(y), rect.right() - inset(y));
        let half = ((right - left) * 0.5).max(0.0);
        let xs = OFFSETS
            .iter()
            .map(|offset| left + offset.min(half))
            .chain(std::iter::once(left + half))
            .chain(OFFSETS.iter().rev().map(|offset| right - offset.min(half)));
        for x in xs {
            let p = pos2(x, y);
            let coverage = inset_coverage(p, padding_box, radius, offset_y, blur, spread);
            mesh.colored_vertex(p, color.gamma_multiply(coverage));
        }
        if row > 0 {
            let (above, here) = ((row - 1) * columns, row * columns);
            for column in 0..columns - 1 {
                mesh.add_triangle(above + column, above + column + 1, here + column);
                mesh.add_triangle(above + column + 1, here + column + 1, here + column);
            }
        }
    }
    Shape::mesh(mesh)
}

/// CSS outer `box-shadow` with a blur too wide for egui's feathered shadow
/// (which cannot exceed the box): a separable Gaussian of the spread box.
/// Corners much smaller than the blur wash out, so they are not modelled.
pub fn soft_shadow(rect: Rect, sigma: f32, color: Color32, clip: Rect) -> Shape {
    let area = rect.expand(3.0 * sigma).intersect(clip);
    if !area.is_positive() {
        return Shape::Noop;
    }
    let step = (sigma / 4.0).clamp(1.0, 6.0);
    let columns = (area.width() / step).ceil() as u32 + 1;
    let rows = (area.height() / step).ceil() as u32 + 1;
    let band = |value: f32, low: f32, high: f32| {
        normal_cdf((value - low) / sigma) - normal_cdf((value - high) / sigma)
    };
    let mut mesh = epaint::Mesh::default();
    for row in 0..rows {
        let y = (area.top() + row as f32 * step).min(area.bottom());
        let vertical = band(y, rect.top(), rect.bottom());
        for column in 0..columns {
            let x = (area.left() + column as f32 * step).min(area.right());
            let coverage = vertical * band(x, rect.left(), rect.right());
            mesh.colored_vertex(pos2(x, y), color.gamma_multiply(coverage));
            if row > 0 && column > 0 {
                let here = row * columns + column;
                let above = here - columns;
                mesh.add_triangle(above - 1, above, here - 1);
                mesh.add_triangle(above, here, here - 1);
            }
        }
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(points: &[Pos2]) -> Vec<Vertex> {
        points.iter().map(|&pos| Vertex { pos, uv: pos }).collect()
    }

    #[test]
    fn content_inside_the_shell_is_untouched() {
        let shell = stadium(Rect::from_min_size(pos2(0.0, 0.0), vec2(116.0, 32.0)));
        let button = circle(pos2(16.0, 16.0), 12.0);
        assert_eq!(clip_convex(&plain(&button), &shell).len(), button.len());
    }

    #[test]
    fn a_contracting_shell_cuts_the_buttons_to_a_lens() {
        // Warming: 68px shell, confirm button centred 42px right of the middle.
        let shell = stadium(Rect::from_center_size(pos2(0.0, 0.0), vec2(68.0, 32.0)));
        let clipped = clip_convex(&plain(&circle(pos2(42.0, 0.0), 12.0)), &shell);
        let (min_x, max_x) = clipped
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.pos.x), hi.max(v.pos.x)));
        assert!((min_x - 30.0).abs() < 0.1 && (max_x - 34.0).abs() < 0.1);
        assert!(clipped.iter().all(|v| v.pos.y.abs() < 12.0));
        // Fully outside: nothing left to draw.
        assert!(clip_convex(&plain(&circle(pos2(80.0, 0.0), 12.0)), &shell).is_empty());
    }

    #[test]
    fn texture_coordinates_follow_the_cut() {
        let shell = [pos2(0.0, 0.0), pos2(5.0, 0.0), pos2(5.0, 10.0), pos2(0.0, 10.0)];
        let quad = [
            Vertex { pos: pos2(0.0, 0.0), uv: pos2(0.0, 0.0) },
            Vertex { pos: pos2(10.0, 0.0), uv: pos2(1.0, 0.0) },
            Vertex { pos: pos2(10.0, 10.0), uv: pos2(1.0, 1.0) },
            Vertex { pos: pos2(0.0, 10.0), uv: pos2(0.0, 1.0) },
        ];
        let clipped = clip_convex(&quad, &shell);
        assert!(clipped.iter().all(|v| v.pos.x <= 5.0 + 1e-4));
        assert!(clipped
            .iter()
            .any(|v| (v.pos.x - 5.0).abs() < 1e-4 && (v.uv.x - 0.5).abs() < 1e-4));
    }

    #[test]
    fn translation_inset_shadow_tints_the_top_edge_and_fades_down_the_ends() {
        // `inset 0 4px 7px -3px` inside the 114×30 padding box of the 116px pill.
        let padding = Rect::from_min_size(pos2(1.0, 1.0), vec2(114.0, 30.0));
        let at = |x, y| inset_coverage(pos2(x, y), padding, 15.0, 4.0, 7.0, -3.0);
        assert!((at(58.0, 1.0) - 0.61).abs() < 0.02);
        assert!(at(58.0, 5.0) < 0.25 && at(58.0, 11.0) < 0.01 && at(58.0, 30.0) < 0.02);
        // The rounded ends keep a faint tint, as the browser's blurred hole does.
        assert!((0.15..0.35).contains(&at(1.0, 16.0)));
    }

    #[test]
    fn a_wide_soft_shadow_peaks_below_the_box_and_never_exceeds_its_colour() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 32.0));
        let Shape::Mesh(mesh) = soft_shadow(rect, 25.0, Color32::BLACK, rect.expand(200.0)) else {
            panic!("expected a mesh");
        };
        let peak = mesh.vertices.iter().map(|v| v.color.a()).max().unwrap();
        // A 32px-tall box under a 25px sigma never reaches full strength.
        assert!((110..140).contains(&peak), "{peak}");
    }
}
