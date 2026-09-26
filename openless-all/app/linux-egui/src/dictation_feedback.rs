//! 听写终态：给用户看的分类，以及胶囊何时自动收起。
//!
//! Core 的 `mark_dictation_failed` 只把错误码写进 `message`
//! （`format!("{:?}", error.code)`，例如 `InvalidArgument`），那是日志用语：
//! 直接送进胶囊/状态栏既看不懂，也像是宿主崩了。说话时正常、不说话就报这个，
//! 因为空音频在 Core 里就是 `InvalidArgument`（`providers.rs` 的
//! "recording contains no audio"）。
//!
//! 这里把「相位 + message」归一成可本地化的分类，并把 Tauri Host 的收起时序
//! （成功/失败 2 秒、取消立即、进行中不收）固化成纯函数，宿主与单测共用同一份
//! 规则，避免两边各写一套。

use std::time::Duration;

use openless_core::{BackendError, BackendErrorCode, DictationPhase};

/// Tauri `coordinator.rs::CAPSULE_AUTO_HIDE_DELAY_MS`：终态在屏上的停留时长。
pub const CAPSULE_AUTO_HIDE_DELAY_MS: u64 = 2000;

/// Core 在失败终态里塞进 `message` 的错误码名，一律不能当用户文案。
pub fn is_backend_error_code(message: &str) -> bool {
    matches!(
        message.trim(),
        "InvalidArgument"
            | "InvalidState"
            | "Busy"
            | "Cancelled"
            | "PermissionDenied"
            | "Unsupported"
            | "Provider"
            | "Persistence"
            | "Platform"
            | "OutcomeUnknown"
            | "Internal"
    )
}

/// 胶囊这一帧该显示什么；具体文案由宿主用本地化 key 渲染。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsuleOutcome {
    /// 成功：宿主显示「已插入 N」。
    Inserted,
    Cancelled,
    Failed,
    /// 进行中：原样转发 Core 的 message（通常是空串）。
    Progress(String),
}

/// 相位 + message → 胶囊分类。绝不把内部错误码当文案。
pub fn capsule_outcome(phase: DictationPhase, message: Option<&str>) -> CapsuleOutcome {
    let message = message.unwrap_or("").trim();
    match phase {
        DictationPhase::Completed => CapsuleOutcome::Inserted,
        DictationPhase::Cancelled => CapsuleOutcome::Cancelled,
        // Core 的失败 message 永远是错误码名，交给宿主显示本地化文案。
        DictationPhase::Failed => CapsuleOutcome::Failed,
        _ => {
            if message.is_empty() || message == "inserted" || is_backend_error_code(message) {
                CapsuleOutcome::Progress(String::new())
            } else {
                CapsuleOutcome::Progress(message.to_string())
            }
        }
    }
}

/// 终态收起延时：成功/失败 2 秒、取消立即；进行中（含 `Inserting`）不收起。
pub fn capsule_hide_delay(phase: DictationPhase) -> Option<Duration> {
    match phase {
        DictationPhase::Completed | DictationPhase::Failed => {
            Some(Duration::from_millis(CAPSULE_AUTO_HIDE_DELAY_MS))
        }
        DictationPhase::Cancelled => Some(Duration::ZERO),
        _ => None,
    }
}

/// 延时到点时是否还该收起。
///
/// 判据是「这个会话已经不再进行」而不是「快照仍停在终态」：Core 在终态事件之后
/// **立刻** `reset_dictation_session()`（见 `api.rs` 的
/// `mark_dictation_failed(..); reset_dictation_session(..); return Err(..)`），
/// 整个 `DictationStateSnapshot` 被重置成 `Idle` 且 `session_id` 清空。
/// 所以 2 秒后回看快照，`current_session` 是 `None` —— 早先要求
/// `current_session == Some(scheduled)` 的写法让**报错路径永远收不回胶囊**。
///
/// 保留的防护：用户在这 2 秒里又按了录音 → 快照里是**另一个**会话 id → 不收；
/// 同一会话又回到进行中相位（理论上不会，兜底）→ 不收。
pub fn capsule_hide_is_still_current(
    current_session: Option<&str>,
    scheduled_session: &str,
    phase: DictationPhase,
) -> bool {
    match current_session {
        // Core 已经收尾（终态后重置，或根本没有会话语义）→ 正是该收起的时候。
        None => true,
        Some(current) if current == scheduled_session => !phase_shows_capsule(phase),
        // 另一个会话正在进行 → 不能把新胶囊一起关掉。
        Some(_) => false,
    }
}

/// 快照里会话已经消失、而宿主从未安排收起时，该补收的会话 id。
///
/// Core 只有**部分**错误路径会先 `mark_dictation_failed`（发布终态事件）再 reset；
/// 另一些（例如转写阶段的空音频）**只 reset 不发布**，宿主就永远等不到终态，
/// 药丸会一直贴在屏幕上。这里按快照自身判断「会话已经没了」，补一次收起。
///
/// 返回 `Some(session)` = 该为这个会话安排收起；`None` = 什么都不用做。
pub fn capsule_needs_fallback_dismissal(
    capsule_session: Option<&str>,
    live_session: Option<&str>,
    already_scheduled: Option<&str>,
) -> Option<String> {
    let capsule_session = capsule_session?;
    // 同一会话仍在跑：等它自己的终态。
    if live_session == Some(capsule_session) {
        return None;
    }
    // 已经为它安排过收起（事件路径已经处理）：别重复计时。
    if already_scheduled == Some(capsule_session) {
        return None;
    }
    Some(capsule_session.to_string())
}

/// 这个相位是否需要胶囊在屏幕上：只有进行中的相位才该按需拉起弹窗。
///
/// 终态不再拉起——否则一个迟到的终态事件会把刚刚自动收起的药丸又喊回来；
/// `Idle` 也不拉（它没有内容可显示）。
pub fn phase_shows_capsule(phase: DictationPhase) -> bool {
    matches!(
        phase,
        DictationPhase::Starting
            | DictationPhase::Recording
            | DictationPhase::Transcribing
            | DictationPhase::Polishing
            | DictationPhase::Inserting
    )
}

/// 停止/取消听写时「本来就可能发生」的错误，不该弹成失败：
/// `InvalidArgument` = 没录到音频（没说话、麦克风没出声）；
/// `InvalidState`/`Busy` = 会话已经收尾（连点两次、自动停止与手动停止撞车）；
/// `Cancelled` = 用户自己取消。
pub fn is_expected_stop_error(code: BackendErrorCode) -> bool {
    matches!(
        code,
        BackendErrorCode::InvalidArgument
            | BackendErrorCode::InvalidState
            | BackendErrorCode::Busy
            | BackendErrorCode::Cancelled
    )
}

/// 归一化一次停止/取消听写的结果：预期内的错误当作「没有结果」而不是失败。
///
/// 真正的失败（网络、鉴权、持久化…）仍然原样向上报。
pub fn normalize_stop_result<T>(
    result: Result<T, BackendError>,
) -> Result<Option<T>, BackendError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if is_expected_stop_error(error.code) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_timing_matches_the_tauri_host_contract() {
        assert_eq!(
            capsule_hide_delay(DictationPhase::Completed),
            Some(Duration::from_millis(CAPSULE_AUTO_HIDE_DELAY_MS))
        );
        assert_eq!(
            capsule_hide_delay(DictationPhase::Failed),
            Some(Duration::from_millis(CAPSULE_AUTO_HIDE_DELAY_MS))
        );
        assert_eq!(
            capsule_hide_delay(DictationPhase::Cancelled),
            Some(Duration::ZERO)
        );
        for phase in [
            DictationPhase::Idle,
            DictationPhase::Starting,
            DictationPhase::Recording,
            DictationPhase::Transcribing,
            DictationPhase::Polishing,
            DictationPhase::Inserting,
        ] {
            assert_eq!(capsule_hide_delay(phase), None, "{phase:?} must stay up");
        }
    }

    #[test]
    fn silence_failure_is_a_normal_terminal_state() {
        // 没说话 → Core 报 InvalidArgument（空音频）：算终态、2 秒后收起。
        let phase = DictationPhase::Failed;
        assert_eq!(
            capsule_outcome(phase, Some("InvalidArgument")),
            CapsuleOutcome::Failed
        );
        assert_eq!(
            capsule_hide_delay(phase),
            Some(Duration::from_millis(CAPSULE_AUTO_HIDE_DELAY_MS))
        );
        assert!(is_expected_stop_error(BackendErrorCode::InvalidArgument));
    }

    #[test]
    fn backend_error_codes_never_become_capsule_copy() {
        for code in [
            "InvalidArgument",
            "InvalidState",
            "Busy",
            "Cancelled",
            "PermissionDenied",
            "Unsupported",
            "Provider",
            "Persistence",
            "Platform",
            "OutcomeUnknown",
            "Internal",
        ] {
            assert!(is_backend_error_code(code), "{code} is an internal code");
            assert_eq!(
                capsule_outcome(DictationPhase::Transcribing, Some(code)),
                CapsuleOutcome::Progress(String::new()),
                "{code} must not reach the capsule"
            );
        }
        assert!(!is_backend_error_code("12 characters"));
        assert!(!is_backend_error_code(""));
    }

    #[test]
    fn progress_messages_still_pass_through() {
        assert_eq!(
            capsule_outcome(DictationPhase::Transcribing, Some("12 characters")),
            CapsuleOutcome::Progress("12 characters".to_string())
        );
        // Core 成功时塞的内部词同样不显示。
        assert_eq!(
            capsule_outcome(DictationPhase::Polishing, Some("inserted")),
            CapsuleOutcome::Progress(String::new())
        );
        assert_eq!(
            capsule_outcome(DictationPhase::Recording, None),
            CapsuleOutcome::Progress(String::new())
        );
    }

    #[test]
    fn a_new_session_cancels_the_pending_dismissal() {
        assert!(capsule_hide_is_still_current(
            Some("s1"),
            "s1",
            DictationPhase::Completed
        ));
        // 另一个会话（用户已经又按了录音）→ 不能收起新胶囊。
        assert!(!capsule_hide_is_still_current(
            Some("s2"),
            "s1",
            DictationPhase::Completed
        ));
        // 同一会话又回到进行中相位 → 也不收。
        assert!(!capsule_hide_is_still_current(
            Some("s1"),
            "s1",
            DictationPhase::Recording
        ));
    }

    #[test]
    fn a_session_that_vanishes_without_a_terminal_event_still_hides_the_capsule() {
        // Core 有些错误路径只 reset、不发布终态事件（转写阶段空音频就是），
        // 宿主必须自己发现「胶囊的会话已经不在快照里」并补一次收起。
        assert_eq!(
            capsule_needs_fallback_dismissal(Some("s1"), None, None),
            Some("s1".to_string())
        );
        // 会话仍在跑 → 等它自己的终态。
        assert_eq!(
            capsule_needs_fallback_dismissal(Some("s1"), Some("s1"), None),
            None
        );
        // 事件路径已经安排过 → 不重复计时。
        assert_eq!(
            capsule_needs_fallback_dismissal(Some("s1"), None, Some("s1")),
            None
        );
        // 新会话顶掉了旧会话（旧胶囊复用同一进程）→ 旧会话该收。
        assert_eq!(
            capsule_needs_fallback_dismissal(Some("s1"), Some("s2"), None),
            Some("s1".to_string())
        );
        // 根本没有胶囊在屏上 → 什么都不做。
        assert_eq!(capsule_needs_fallback_dismissal(None, None, None), None);
    }

    #[test]
    fn a_vanished_session_hides_the_capsule_through_the_fallback_path() {
        // 兜底链路的端到端判据：会话消失（无终态事件）→ 取兜底会话 → 按失败终态
        // 的时长 → 到点时判据为真 → 收起。
        let session = capsule_needs_fallback_dismissal(Some("s1"), None, None)
            .expect("a vanished session must be picked up");
        assert_eq!(
            capsule_hide_delay(DictationPhase::Failed),
            Some(Duration::from_millis(CAPSULE_AUTO_HIDE_DELAY_MS))
        );
        assert!(capsule_hide_is_still_current(
            None,
            &session,
            DictationPhase::Idle
        ));
    }

    #[test]
    fn the_failure_path_ends_with_a_dismissal() {
        // 串起报错路径的四环（不看实现，看行为）：
        // 1) 失败是终态 → 2 秒后收起；
        // 2) 这 2 秒里 Core 已经把快照 reset 成 Idle、session_id 清空；
        // 3) 到点时的判据必须为真 → 真的收起；
        // 4) 但若这 2 秒里用户又按了录音（新会话）→ 不收，新胶囊活着。
        assert_eq!(
            capsule_hide_delay(DictationPhase::Failed),
            Some(Duration::from_millis(CAPSULE_AUTO_HIDE_DELAY_MS))
        );
        assert!(capsule_hide_is_still_current(
            None,
            "session",
            DictationPhase::Idle
        ));
        assert!(!capsule_hide_is_still_current(
            Some("next"),
            "session",
            DictationPhase::Starting
        ));
    }

    #[test]
    fn the_core_reset_after_a_terminal_phase_still_dismisses_the_capsule() {
        // 真实链路：Core 的失败路径是
        // `mark_dictation_failed(..); reset_dictation_session(..); return Err(..)`，
        // 后者把整个快照重置成 Idle 并清空 session_id。2 秒后回看快照只剩
        // `None` —— 这正是「报错弹窗收不回」的原因，必须仍然收起。
        for phase in [
            DictationPhase::Idle,
            DictationPhase::Completed,
            DictationPhase::Failed,
            DictationPhase::Cancelled,
        ] {
            assert!(
                capsule_hide_is_still_current(None, "s1", phase),
                "a finished session ({phase:?}) must still hide the capsule"
            );
            // 同一 id 但相位已经落回 Idle：同样属于「不再进行」。
            assert!(capsule_hide_is_still_current(Some("s1"), "s1", phase));
        }
    }

    #[test]
    fn only_in_flight_phases_spawn_a_capsule() {
        for phase in [
            DictationPhase::Starting,
            DictationPhase::Recording,
            DictationPhase::Transcribing,
            DictationPhase::Polishing,
            DictationPhase::Inserting,
        ] {
            assert!(
                phase_shows_capsule(phase),
                "{phase:?} must be able to spawn"
            );
        }
        for phase in [
            DictationPhase::Idle,
            DictationPhase::Completed,
            DictationPhase::Cancelled,
            DictationPhase::Failed,
        ] {
            assert!(!phase_shows_capsule(phase), "{phase:?} must not re-spawn");
        }
    }

    #[test]
    fn expected_stop_errors_do_not_become_failures() {
        for code in [
            BackendErrorCode::InvalidArgument,
            BackendErrorCode::InvalidState,
            BackendErrorCode::Busy,
            BackendErrorCode::Cancelled,
        ] {
            assert!(
                normalize_stop_result::<()>(Err(BackendError::new(code, "boom")))
                    .unwrap()
                    .is_none(),
                "{code:?} is an expected stop outcome"
            );
        }
        let real = normalize_stop_result::<()>(Err(BackendError::new(
            BackendErrorCode::Provider,
            "upstream 500",
        )));
        assert_eq!(real.unwrap_err().code, BackendErrorCode::Provider);
        assert_eq!(
            normalize_stop_result(Ok(7)).unwrap(),
            Some(7),
            "successful stops keep their value"
        );
    }
}
