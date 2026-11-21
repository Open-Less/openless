//! Geometry policy for the native Ask Anything window. The caller owns platform I/O.

use super::LogicalMonitorFrame;

/// QA records through the shared capsule and keeps its separate answer card open.
pub(super) const QA_WINDOW_WIDTH: f64 = 480.0;
pub(super) const QA_WINDOW_HEIGHT: f64 = 80.0;
// 1950 × 830 at 2× display scale. Keep a shared scale when fitting smaller work areas.
const QA_WINDOW_EXPANDED_WIDTH: f64 = 975.0;
const QA_WINDOW_EXPANDED_HEIGHT: f64 = 415.0;
const QA_WINDOW_MIN_EXPANDED_HEIGHT: f64 = 144.0;
/// Reserve room for the independent recording capsule and its context badge.
pub(super) const QA_WINDOW_BOTTOM_INSET: f64 = 96.0;
/// Expanded answers sit higher so the lower composer does not crowd the recording capsule.
const QA_WINDOW_EXPANDED_BOTTOM_INSET: f64 = 132.0;
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
        let available_width = (frame.width - 32.0).max(1.0);
        let available_height = (frame.height - 32.0 - QA_WINDOW_EXPANDED_BOTTOM_INSET).max(1.0);
        (available_width / QA_WINDOW_EXPANDED_WIDTH)
            .min(available_height / QA_WINDOW_EXPANDED_HEIGHT)
            .min(1.0)
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
    if expanded && height < QA_WINDOW_MIN_EXPANDED_HEIGHT {
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
        x = x.clamp(
            frame.x + 16.0,
            (frame.x + frame.width - width - 16.0).max(frame.x + 16.0),
        );
        let bottom_inset = if expanded {
            QA_WINDOW_EXPANDED_BOTTOM_INSET
        } else {
            QA_WINDOW_BOTTOM_INSET
        };
        y = y.clamp(
            frame.y + 32.0,
            (frame.y + frame.height - height - bottom_inset).max(frame.y + 32.0),
        );
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
        assert_eq!(fit(1440.0, 900.0), (975.0, 415.0));
        for (width, height) in [(800.0, 700.0), (1280.0, 480.0)] {
            let (w, h) = fit(width, height);
            assert!(w <= width - 32.0 + 1e-6);
            assert!(h <= height - 164.0 + 1e-6);
            assert!((w / h - 1950.0 / 830.0).abs() < 1e-6);
        }
    }

    #[test]
    fn qa_resize_preserves_centre_baseline_and_user_position() {
        let expanded = qa_resize_origin(-700.0, 700.0, 480.0, 80.0, 975.0, 415.0, false, true);
        assert_eq!(expanded, (-947.5, 325.0));
        assert_eq!(
            qa_resize_origin(expanded.0, expanded.1, 975.0, 415.0, 975.0, 415.0, true, true),
            expanded
        );
        assert_eq!(
            qa_resize_origin(expanded.0, expanded.1, 975.0, 415.0, 480.0, 80.0, true, false),
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
        let small = LogicalMonitorFrame {
            width: 460.0,
            height: 250.0,
            ..compact
        };
        assert_eq!(
            resize_qa_frame(compact, Some(small), true).unwrap_err(),
            "qa_work_area_too_small"
        );
    }
}
