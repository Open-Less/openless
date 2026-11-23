//! 其余平台的选区捕获占位。

use crate::selection::CaptureOutcome;

pub(crate) fn capture_via_simulated_copy() -> CaptureOutcome {
    CaptureOutcome::Unsupported
}
