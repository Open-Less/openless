//! Geometry policy for the native Ask Anything window. The caller owns platform I/O.

use super::LogicalMonitorFrame;

/// QA records through the shared capsule and keeps its separate answer card open.
pub(super) const QA_WINDOW_WIDTH: f64 = 480.0;
pub(super) const QA_WINDOW_HEIGHT: f64 = 80.0;
// Use logical pixels and a single scale when fitting the work area.
const QA_WINDOW_EXPANDED_WIDTH: f64 = 920.0;
const QA_WINDOW_EXPANDED_HEIGHT: f64 = 800.0;
const QA_REFERENCE_WORK_AREA_WIDTH: f64 = 1440.0;
const QA_REFERENCE_WORK_AREA_HEIGHT: f64 = 900.0;
// Keep the frame usable by the WebView's QA layout gate.
const QA_WINDOW_MIN_WIDTH: f64 = 220.0;
const QA_WINDOW_MIN_EXPANDED_HEIGHT: f64 = 144.0;
/// Reserve room for the independent recording capsule and its context badge.
pub(super) const QA_WINDOW_BOTTOM_INSET: f64 = 96.0;
const QA_WINDOW_EXPANDED_INSET: f64 = 16.0;
const QA_RESULT_BOTTOM_LIFT: f64 = 40.0;

// Preserve a dragged window's centre and baseline; repeated result events must not drift.
fn qa_resize_origin(
    x: f64,
    y: f64,
    old_width: f64,
    old_height: f64,
    width: f64,
    height: f64,
    was_expanded: bool,
    expanded: bool,
) -> (f64, f64) {
    let lift = if expanded { QA_RESULT_BOTTOM_LIFT } else { 0.0 }
        - if was_expanded {
            QA_RESULT_BOTTOM_LIFT
        } else {
            0.0
        };
    (
        x + (old_width - width) / 2.0,
        y + old_height - height - lift,
    )
}

fn qa_expanded_size(area: Option<LogicalMonitorFrame>) -> (f64, f64) {
    let fit = area.map_or(1.0, |frame| {
        let margin = 2.0 * QA_WINDOW_EXPANDED_INSET;
        let available_width = (frame.width - margin).max(1.0);
        let available_height = (frame.height - margin).max(1.0);
        // Scale on both axes so an ultrawide monitor alone does not enlarge the card.
        let preferred = (frame.width / QA_REFERENCE_WORK_AREA_WIDTH)
            .min(frame.height / QA_REFERENCE_WORK_AREA_HEIGHT)
            .max(1.0);
        (available_width / QA_WINDOW_EXPANDED_WIDTH)
            .min(available_height / QA_WINDOW_EXPANDED_HEIGHT)
            .min(preferred)
    });
    (
        QA_WINDOW_EXPANDED_WIDTH * fit,
        QA_WINDOW_EXPANDED_HEIGHT * fit,
    )
}

pub(super) fn resize_qa_frame(
    current: LogicalMonitorFrame,
    work_area: Option<LogicalMonitorFrame>,
    expanded: bool,
) -> Result<LogicalMonitorFrame, &'static str> {
    let was_expanded = current.height > QA_WINDOW_HEIGHT + 1.0;
    let (width, height) = if expanded {
        qa_expanded_size(work_area)
    } else if let Some(frame) = work_area {
        (
            QA_WINDOW_WIDTH.min((frame.width - 32.0).max(240.0)),
            QA_WINDOW_HEIGHT,
        )
    } else {
        (QA_WINDOW_WIDTH, QA_WINDOW_HEIGHT)
    };
    if expanded && (width < QA_WINDOW_MIN_WIDTH || height < QA_WINDOW_MIN_EXPANDED_HEIGHT) {
        return Err("qa_work_area_too_small");
    }
    let (mut x, mut y) = qa_resize_origin(
        current.x,
        current.y,
        current.width,
        current.height,
        width,
        height,
        was_expanded,
        expanded,
    );
    if let Some(frame) = work_area {
        let x_min = frame.x + QA_WINDOW_EXPANDED_INSET;
        let x_max = frame.x + frame.width - width - QA_WINDOW_EXPANDED_INSET;
        x = x.clamp(x_min, x_max.max(x_min));
        let (top_inset, bottom_inset) = if expanded {
            (QA_WINDOW_EXPANDED_INSET, QA_WINDOW_EXPANDED_INSET)
        } else {
            (32.0, QA_WINDOW_BOTTOM_INSET)
        };
        let y_min = frame.y + top_inset;
        let y_max = frame.y + frame.height - height - bottom_inset;
        y = y.clamp(y_min, y_max.max(y_min));
    }
    Ok(LogicalMonitorFrame {
        x,
        y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_qa_fits_the_work_area_with_a_single_scale() {
        let fit = |width, height| {
            qa_expanded_size(Some(LogicalMonitorFrame {
                x: -1600.0,
                y: 20.0,
                width,
                height,
            }))
        };
        assert_eq!(fit(1440.0, 900.0), (920.0, 800.0));
        assert_eq!(fit(2160.0, 1350.0), (1380.0, 1200.0));
        assert_eq!(fit(3840.0, 900.0), (920.0, 800.0));
        for (width, height) in [(800.0, 700.0), (1280.0, 480.0), (3840.0, 2160.0)] {
            let (w, h) = fit(width, height);
            assert!(w <= width - 32.0 + 1e-6);
            assert!(h <= height - 32.0 + 1e-6);
            assert!((w / h - 920.0 / 800.0).abs() < 1e-6);
        }
    }

    #[test]
    fn qa_resize_preserves_centre_baseline_and_user_position() {
        let expanded = qa_resize_origin(-700.0, 700.0, 480.0, 80.0, 920.0, 800.0, false, true);
        assert_eq!(expanded, (-920.0, -60.0));
        assert_eq!(
            qa_resize_origin(expanded.0, expanded.1, 920.0, 800.0, 920.0, 800.0, true, true),
            expanded
        );
        assert_eq!(
            qa_resize_origin(expanded.0, expanded.1, 920.0, 800.0, 480.0, 80.0, true, false),
            (-700.0, 700.0)
        );
    }

    #[test]
    fn rejects_an_expansion_that_cannot_fit_the_work_area() {
        let compact = LogicalMonitorFrame {
            x: 0.0,
            y: 0.0,
            width: 480.0,
            height: 80.0,
        };
        for (width, height) in [(460.0, 170.0), (1280.0, 215.0)] {
            let small = LogicalMonitorFrame {
                width,
                height,
                ..compact
            };
            assert_eq!(
                resize_qa_frame(compact, Some(small), true).unwrap_err(),
                "qa_work_area_too_small"
            );
        }
    }
}
