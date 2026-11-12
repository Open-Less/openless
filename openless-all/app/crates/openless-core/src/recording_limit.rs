//! Hard recording-duration cap.
//!
//! Typeless-style behaviour: a dictation may run for at most nine minutes of
//! captured audio. During the final minute the capsule shows a countdown so the
//! user can wrap up, and at the cap Core asks the host for the ordinary stop
//! path (transcribe + polish + insert) instead of dropping the audio.
//!
//! The arithmetic lives here, separate from the backend state machine, so the
//! deadline and the warning window can be tested without a recorder.

use std::time::Duration;

/// Total capture time allowed for one dictation session.
pub const MAX_RECORDING_DURATION: Duration = Duration::from_secs(9 * 60);

/// How long before the cap the capsule starts counting down.
pub const RECORDING_LIMIT_WARNING: Duration = Duration::from_secs(60);

/// `true` once the capture has consumed the whole allowance.
pub fn recording_limit_reached(elapsed_ms: u64) -> bool {
    elapsed_ms >= MAX_RECORDING_DURATION.as_millis() as u64
}

/// Remaining allowance, but only while the countdown should be visible.
///
/// Returns `None` before the warning window opens (nothing to show) and once the
/// cap is reached (the session is being stopped, so a zero countdown would just
/// flash). Callers render `ceil(ms / 1000)` seconds.
pub fn recording_limit_remaining_ms(elapsed_ms: u64) -> Option<u64> {
    if recording_limit_reached(elapsed_ms) {
        return None;
    }
    let warning_at =
        MAX_RECORDING_DURATION.as_millis() as u64 - RECORDING_LIMIT_WARNING.as_millis() as u64;
    (elapsed_ms >= warning_at).then(|| MAX_RECORDING_DURATION.as_millis() as u64 - elapsed_ms)
}

/// Countdown seconds shown to the user, rounding up so the last partial second
/// still reads "1" instead of disappearing a moment early.
pub fn recording_limit_remaining_seconds(elapsed_ms: u64) -> Option<u32> {
    recording_limit_remaining_ms(elapsed_ms)
        .map(|ms| ms.div_ceil(1_000).min(u32::MAX as u64) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_countdown_before_the_final_minute() {
        assert_eq!(recording_limit_remaining_ms(0), None);
        assert_eq!(recording_limit_remaining_ms(8 * 60 * 1_000 - 1), None);
        assert!(!recording_limit_reached(8 * 60 * 1_000 - 1));
    }

    #[test]
    fn countdown_starts_exactly_one_minute_before_the_cap() {
        assert_eq!(recording_limit_remaining_ms(8 * 60 * 1_000), Some(60_000));
        assert_eq!(recording_limit_remaining_seconds(8 * 60 * 1_000), Some(60));
        assert!(!recording_limit_reached(8 * 60 * 1_000));
    }

    #[test]
    fn countdown_rounds_up_and_reaches_one_before_the_cap() {
        // 59.2 s left still reads "60" — the user has a full minute.
        assert_eq!(
            recording_limit_remaining_seconds(8 * 60 * 1_000 + 800),
            Some(60)
        );
        // A partial second left reads "1", never "0".
        assert_eq!(
            recording_limit_remaining_seconds(9 * 60 * 1_000 - 1),
            Some(1)
        );
        assert_eq!(recording_limit_remaining_ms(9 * 60 * 1_000 - 1), Some(1));
    }

    #[test]
    fn the_cap_stops_the_session_and_hides_the_countdown() {
        assert!(recording_limit_reached(9 * 60 * 1_000));
        assert!(recording_limit_reached(9 * 60 * 1_000 + 5_000));
        assert_eq!(recording_limit_remaining_ms(9 * 60 * 1_000), None);
        assert_eq!(recording_limit_remaining_seconds(9 * 60 * 1_000), None);
    }
}
