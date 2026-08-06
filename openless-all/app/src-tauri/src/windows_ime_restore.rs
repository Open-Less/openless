#![allow(dead_code, unused_imports, unused_variables)]

use crate::windows_ime_profile::{
    is_openless_profile_snapshot, ImeProfileSnapshot, WindowsImeProfileResult,
};

/// `restore_profile` 返回失败（legacy 与现代均失败）后，重试前的等待时长。
pub const RESTORE_RETRY_DELAY_MS: u64 = 250;

/// 等待重试：在多线程 tokio runtime 上执行时用 `block_in_place` 让出工作线程，
/// 避免阻塞 runtime 上其它任务；其它上下文（current-thread runtime、非 runtime
/// 线程）直接 sleep，避免 current-thread runtime 下 `block_in_place` panic。
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
    /// saved 快照本身是 OpenLess（上次会话疑似未恢复）→ 跳过恢复。
    SkippedSticky,
    /// restore_profile 返回 Ok（首次或重试后）。
    Verified,
    /// 两次 restore_profile 均失败。
    FailedAfterRetry,
}

/// 恢复阶段完整流程：粘滞态防护 → 恢复 → 失败重试。
///
/// 重试依据是 `restore_profile` 的返回值（legacy 与现代均失败才为 Err），
/// 不依赖 `is_openless_active` 探测：该探测（`GetActiveProfile`）运行在
/// OpenLess 进程后台线程，与目标 App 线程的 TSF 状态可能不一致（issue #852），
/// 因此只保留为诊断日志，记录恢复后 OpenLess 是否仍激活，不参与控制流。
/// 通过注入 `restore_profile` / `is_openless_active` 让该逻辑可在任意平台被
/// 单元测试覆盖（生产路径由 `WindowsImeProfileManager` 提供实现）。
///
/// 已知限制：恢复是无条件的——即使会话中途用户手动切走了输入法，结束时仍会
/// 恢复到会话前快照（旧版依赖的 `GetActiveProfile` 探测在 OpenLess 进程后台
/// 线程下不可靠，不能作为控制流依据，issue #852）。
pub(super) fn run_restore_flow(
    saved_profile: &ImeProfileSnapshot,
    mut restore_profile: impl FnMut(&ImeProfileSnapshot) -> WindowsImeProfileResult<()>,
    mut is_openless_active: impl FnMut() -> WindowsImeProfileResult<bool>,
    retry_delay: std::time::Duration,
) -> RestoreOutcome {
    // 粘滞态防护：saved 本身就是 OpenLess（上次会话疑似未恢复）→ 不把 OpenLess
    // 当原输入法写死，跳过恢复并留下诊断日志。
    if is_openless_profile_snapshot(saved_profile) {
        log::warn!(
            "[windows-ime] saved profile is OpenLess itself — previous session likely failed to restore; skipping restore"
        );
        return RestoreOutcome::SkippedSticky;
    }

    // 第一次恢复 + 失败重试一次：TSF 会话级切换偶发失败时，短等待后重试一次。
    // 成功与否以 restore_profile 返回值为准；探测仅作诊断日志。
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
    log::error!(
        "[windows-ime] restore failed after retry — IME may remain on OpenLess"
    );
    RestoreOutcome::FailedAfterRetry
}

/// 恢复后的诊断探测（仅日志）：记录 OpenLess 是否仍是当前 profile。
///
/// 该探测与决策/重试解耦——`GetActiveProfile` 运行在 OpenLess 进程后台线程，
/// 与目标 App 线程的 TSF 状态可能不一致（issue #852），结果不可作为控制流依据。
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
        // 粘滞态防护：saved 是 OpenLess → 跳过恢复，restore 不被调用（issue #852）。
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
        // 探测显示 OpenLess 仍激活不触发重试：成功与否以 restore 返回值为准（#852）。
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
        // 探测报错仅记日志，不影响恢复成功判定。
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
        // 首次 restore 失败 → 重试一次 → 成功。
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
        // 两次 restore 都失败 → 整体失败。
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
