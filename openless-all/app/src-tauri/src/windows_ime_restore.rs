#![allow(dead_code, unused_imports, unused_variables)]

use crate::windows_ime_profile::{
    is_openless_profile_snapshot, ImeProfileSnapshot, WindowsImeProfileResult,
};

/// Wait before retrying after `restore_profile` fails (both legacy and modern paths failed).
pub const RESTORE_RETRY_DELAY_MS: u64 = 250;

/// Wait for retry: on a multi-threaded tokio runtime, use `block_in_place` to yield the worker thread so
/// other tasks aren't blocked; in other contexts (current-thread runtime, non-runtime threads), sleep
/// directly to avoid a `block_in_place` panic on a current-thread runtime.
fn sleep_restore_retry(retry_delay: std::time::Duration) {
    let on_multi_thread_runtime = tokio::runtime::Handle::try_current()
        .map(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
        .unwrap_or(false);
    if on_multi_thread_runtime {
        tokio::task::block_in_place(move || std::thread::sleep(retry_delay));
    } else {
        std::thread::sleep(retry_delay);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestoreOutcome {
    /// The saved snapshot is itself OpenLess (previous session likely failed to restore) → skip restore.
    SkippedSticky,
    /// restore_profile returned Ok (first attempt or after retry).
    Verified,
    /// Both restore_profile attempts failed.
    FailedAfterRetry,
}

/// Full restore flow: sticky-state guard → restore → retry on failure.
///
/// Retry decisions come from the `restore_profile` return value (Err only when both legacy and modern
/// fail), never from the `is_openless_active` probe: that probe (`GetActiveProfile`) runs on an OpenLess
/// background thread and can disagree with the target app's thread TSF state (issue #852). It stays as a
/// diagnostic log of whether OpenLess is still active after restore, never as control flow.
/// `restore_profile` / `is_openless_active` are injected so any platform can unit-test this logic
/// (production provides the implementations via `WindowsImeProfileManager`).
///
/// Known limitation: restore is unconditional — even if the user manually switched IMEs mid-session,
/// the pre-session snapshot is restored at the end (the `GetActiveProfile` probe the old version relied
/// on is unreliable on an OpenLess background thread and cannot drive control flow, issue #852).
pub(super) fn run_restore_flow(
    saved_profile: &ImeProfileSnapshot,
    mut restore_profile: impl FnMut(&ImeProfileSnapshot) -> WindowsImeProfileResult<()>,
    mut is_openless_active: impl FnMut() -> WindowsImeProfileResult<bool>,
    retry_delay: std::time::Duration,
) -> RestoreOutcome {
    // Sticky-state guard: the saved profile is itself OpenLess (previous session likely failed to restore)
    // → don't hard-code OpenLess as the original IME; skip restore and leave a diagnostic log.
    if is_openless_profile_snapshot(saved_profile) {
        log::warn!(
            "[windows-ime] saved profile is OpenLess itself — previous session likely failed to restore; skipping restore"
        );
        return RestoreOutcome::SkippedSticky;
    }

    // First restore + one retry on failure: TSF session-level switches fail sporadically; retry once after a
    // short wait. Success is decided by the restore_profile return value; the probe is diagnostic only.
    for attempt in 0..2 {
        if attempt > 0 {
            log::info!("[windows-ime] restore failed; retrying (attempt {attempt})");
            sleep_restore_retry(retry_delay);
        }
        match restore_profile(saved_profile) {
            Ok(()) => {
                log::info!("[windows-ime] restore succeeded (attempt {attempt})");
                log_restore_verification(&mut is_openless_active, attempt);
                return RestoreOutcome::Verified;
            }
            Err(error) => {
                log::warn!(
                    "[windows-ime] restore saved profile failed (attempt {attempt}): {error}"
                );
                log_restore_verification(&mut is_openless_active, attempt);
            }
        }
    }
    log::error!("[windows-ime] restore failed after retry — IME may remain on OpenLess");
    RestoreOutcome::FailedAfterRetry
}

/// Post-restore diagnostic probe (logging only): records whether OpenLess is still the current profile.
///
/// The probe is decoupled from decisions/retries — `GetActiveProfile` runs on an OpenLess background
/// thread and can disagree with the target app's thread TSF state (issue #852), so the result never
/// drives control flow.
fn log_restore_verification(
    is_openless_active: &mut impl FnMut() -> WindowsImeProfileResult<bool>,
    attempt: i32,
) {
    match is_openless_active() {
        Ok(false) => {
            log::info!(
                "[windows-ime] restore verification: OpenLess is no longer the active profile (attempt {attempt})"
            );
        }
        Ok(true) => {
            log::warn!(
                "[windows-ime] restore verification: OpenLess is still the active profile (attempt {attempt})"
            );
        }
        Err(error) => {
            log::warn!(
                "[windows-ime] restore verification check failed (attempt {attempt}): {error}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_ime_profile::{openless_snapshot_for_test, WindowsImeProfileError};

    #[test]
    fn restore_flow_skips_when_saved_profile_is_openless_itself() {
        // Sticky-state guard: saved is OpenLess → skip restore, restore is never called (issue #852).
        let mut restore_calls = 0;
        let outcome = run_restore_flow(
            &openless_snapshot_for_test(),
            |_| {
                restore_calls += 1;
                Ok(())
            },
            || Ok(false),
            std::time::Duration::ZERO,
        );

        assert_eq!(outcome, RestoreOutcome::SkippedSticky);
        assert_eq!(restore_calls, 0);
    }

    #[test]
    fn restore_flow_succeeds_without_retry_when_restore_returns_ok() {
        let mut restore_calls = 0;
        let outcome = run_restore_flow(
            &ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409),
            |_| {
                restore_calls += 1;
                Ok(())
            },
            || Ok(false),
            std::time::Duration::ZERO,
        );

        assert_eq!(outcome, RestoreOutcome::Verified);
        assert_eq!(restore_calls, 1);
    }

    #[test]
    fn restore_flow_succeeds_even_when_probe_still_reports_openless() {
        // A probe still reporting OpenLess does not trigger a retry: success is decided by the restore return value (#852).
        let mut restore_calls = 0;
        let outcome = run_restore_flow(
            &ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409),
            |_| {
                restore_calls += 1;
                Ok(())
            },
            || Ok(true),
            std::time::Duration::ZERO,
        );

        assert_eq!(outcome, RestoreOutcome::Verified);
        assert_eq!(restore_calls, 1);
    }

    #[test]
    fn restore_flow_probe_errors_do_not_affect_outcome() {
        // Probe errors are logged only; they don't affect the restore success verdict.
        let mut restore_calls = 0;
        let outcome = run_restore_flow(
            &ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409),
            |_| {
                restore_calls += 1;
                Ok(())
            },
            || {
                Err(WindowsImeProfileError::WindowsApi(
                    "probe failed".to_string(),
                ))
            },
            std::time::Duration::ZERO,
        );

        assert_eq!(outcome, RestoreOutcome::Verified);
        assert_eq!(restore_calls, 1);
    }

    #[test]
    fn restore_flow_retries_when_restore_fails_then_succeeds() {
        // First restore fails → one retry → success.
        let mut restore_calls = 0;
        let outcome = run_restore_flow(
            &ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409),
            |_| {
                restore_calls += 1;
                if restore_calls == 1 {
                    Err(WindowsImeProfileError::WindowsApi(
                        "transient failure".to_string(),
                    ))
                } else {
                    Ok(())
                }
            },
            || Ok(false),
            std::time::Duration::ZERO,
        );

        assert_eq!(outcome, RestoreOutcome::Verified);
        assert_eq!(restore_calls, 2);
    }

    #[test]
    fn restore_flow_fails_after_two_restore_errors() {
        // Both restores fail → overall failure.
        let mut restore_calls = 0;
        let outcome = run_restore_flow(
            &ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409),
            |_| {
                restore_calls += 1;
                Err(WindowsImeProfileError::WindowsApi(
                    "restore failed".to_string(),
                ))
            },
            || Ok(false),
            std::time::Duration::ZERO,
        );

        assert_eq!(outcome, RestoreOutcome::FailedAfterRetry);
        assert_eq!(restore_calls, 2);
    }
}
