#![allow(dead_code, unused_imports, unused_variables)]
use crate::types::InsertStatus;
use crate::windows_ime_ipc::{ImeSubmitRequest, WindowsImeIpcServer};
use crate::windows_ime_profile::{
    is_openless_profile_snapshot, restore_decision, ImeProfileSnapshot, ProfileRestoreDecision,
    WindowsImeProfileManager,
};
use crate::windows_ime_protocol::ImeSubmitStatus;
use crate::windows_ime_restore::{run_restore_flow, RESTORE_RETRY_DELAY_MS};

#[derive(Debug)]
pub enum WindowsImeSessionError {
    Profile(String),
    Ipc(String),
}

impl std::fmt::Display for WindowsImeSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile(message) | Self::Ipc(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for WindowsImeSessionError {}

pub fn map_ime_status_to_insert_status(status: ImeSubmitStatus) -> InsertStatus {
    match status {
        ImeSubmitStatus::Committed => InsertStatus::Inserted,
        ImeSubmitStatus::Rejected | ImeSubmitStatus::Failed => InsertStatus::CopiedFallback,
    }
}

pub fn should_fallback_after_ime_result(status: ImeSubmitStatus) -> bool {
    !matches!(status, ImeSubmitStatus::Committed)
}

fn describe_snapshot(snapshot: &ImeProfileSnapshot) -> String {
    format!(
        "kind={:?} lang=0x{:04X} clsid={} profile={}",
        snapshot.kind(),
        snapshot.lang_id(),
        snapshot.clsid().unwrap_or("none"),
        snapshot.profile_guid().unwrap_or("none"),
    )
}

#[derive(Debug)]
pub struct PreparedWindowsImeSession {
    saved_profile: Option<ImeProfileSnapshot>,
    openless_activated: bool,
}

impl PreparedWindowsImeSession {
    pub fn unavailable() -> Self {
        Self {
            saved_profile: None,
            openless_activated: false,
        }
    }

    pub fn activation_failed(saved_profile: ImeProfileSnapshot) -> Self {
        Self {
            saved_profile: Some(saved_profile),
            openless_activated: false,
        }
    }

    pub fn is_ready_for_tsf_submit(&self) -> bool {
        self.has_saved_profile() && self.openless_was_activated()
    }

    pub fn has_saved_profile(&self) -> bool {
        self.saved_profile.is_some()
    }

    pub fn openless_was_activated(&self) -> bool {
        self.openless_activated
    }

    pub fn activation_failed_with_saved_profile(&self) -> bool {
        self.has_saved_profile() && !self.openless_was_activated()
    }
}

pub struct WindowsImeSessionController {
    profile_manager: WindowsImeProfileManager,
    ipc: WindowsImeIpcServer,
}

impl WindowsImeSessionController {
    pub fn new() -> Self {
        Self {
            profile_manager: WindowsImeProfileManager::new(),
            ipc: WindowsImeIpcServer::new(),
        }
    }

    pub fn prepare_session(&self) -> PreparedWindowsImeSession {
        #[cfg(target_os = "windows")]
        {
            let saved_profile = match self.profile_manager.capture_active_profile() {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    let error = WindowsImeSessionError::Profile(error.to_string());
                    log::warn!("[windows-ime] capture active profile failed: {error}");
                    return PreparedWindowsImeSession::unavailable();
                }
            };

            // 诊断：会话开始时 OpenLess 已是当前输入法 → 上次会话疑似恢复失败。
            // 此时仍照常激活（幂等），restore_session 的粘滞态防护会跳过"恢复"，
            // 避免把 OpenLess 当原输入法写死（issue #852 的失败状态自粘）。
            if is_openless_profile_snapshot(&saved_profile) {
                log::warn!(
                    "[windows-ime] session began while OpenLess IME was already the active profile — previous session likely failed to restore"
                );
            }

            match self.profile_manager.activate_openless_profile() {
                Ok(()) => PreparedWindowsImeSession {
                    saved_profile: Some(saved_profile),
                    openless_activated: true,
                },
                Err(error) => {
                    let error = WindowsImeSessionError::Profile(error.to_string());
                    log::warn!("[windows-ime] activate OpenLess profile failed: {error}");
                    PreparedWindowsImeSession::activation_failed(saved_profile)
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            PreparedWindowsImeSession::unavailable()
        }
    }

    pub async fn submit_prepared(
        &self,
        prepared: &PreparedWindowsImeSession,
        request: ImeSubmitRequest,
    ) -> Result<InsertStatus, WindowsImeSessionError> {
        if !prepared.is_ready_for_tsf_submit() {
            return Err(WindowsImeSessionError::Ipc(
                "OpenLess IME session is not active".to_string(),
            ));
        }

        let status = self
            .ipc
            .submit_text(request)
            .await
            .map_err(|error| WindowsImeSessionError::Ipc(error.to_string()))?;
        if should_fallback_after_ime_result(status) {
            log::warn!(
                "[windows-ime] TSF submit returned {status:?}; falling back to non-TSF insertion"
            );
        }
        Ok(map_ime_status_to_insert_status(status))
    }

    /// 恢复会话前的输入法。
    ///
    /// 已知限制：恢复是无条件的——会话中途用户手动切走的输入法也会在结束时被
    /// 覆盖为会话前快照（`GetActiveProfile` 探测在 OpenLess 进程后台线程下不可靠，
    /// 不能作为控制流依据，issue #852）。
    pub fn restore_session(&self, prepared: PreparedWindowsImeSession) {
        let saved_profile = prepared.saved_profile.as_ref();
        let openless_was_activated = prepared.openless_was_activated();
        let activation_failed = prepared.activation_failed_with_saved_profile();

        // 诊断：记录决策依据 + 恢复前探测到的当前 profile（不影响决策）。
        // issue #852 的恢复决策只依赖会话已知的激活事实，不依赖该探测结果。
        let active_profile_desc = match self.profile_manager.capture_active_profile() {
            Ok(snapshot) => describe_snapshot(&snapshot),
            Err(error) => format!("unavailable: {error}"),
        };
        let saved_desc = match prepared.saved_profile.as_ref() {
            Some(snapshot) => describe_snapshot(snapshot),
            None => "none".to_string(),
        };
        let decision = restore_decision(saved_profile, openless_was_activated, activation_failed);
        log::info!(
            "[windows-ime] restore decision={decision:?} saved_profile={saved_desc} openless_was_activated={openless_was_activated} activation_failed={activation_failed} active_profile={active_profile_desc}"
        );

        if decision != ProfileRestoreDecision::RestoreSavedProfile {
            return;
        }

        let Some(saved_profile) = saved_profile else {
            return;
        };

        // 恢复流程（粘滞防护/重试/诊断）实现在 windows_ime_restore，可跨平台单测。
        // outcome 仅补一条 debug 诊断；成功/失败/跳过的详情已由流程内部日志输出。
        let outcome = run_restore_flow(
            saved_profile,
            |snapshot| self.profile_manager.restore_profile(snapshot),
            || self.profile_manager.is_openless_profile_active(),
            std::time::Duration::from_millis(RESTORE_RETRY_DELAY_MS),
        );
        log::debug!("[windows-ime] restore outcome: {outcome:?}");
    }
}

impl Default for WindowsImeSessionController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_ime_result_maps_to_inserted() {
        assert_eq!(
            map_ime_status_to_insert_status(ImeSubmitStatus::Committed),
            InsertStatus::Inserted
        );
    }

    #[test]
    fn rejected_ime_result_requests_fallback() {
        assert!(should_fallback_after_ime_result(ImeSubmitStatus::Rejected));
        assert!(should_fallback_after_ime_result(ImeSubmitStatus::Failed));
        assert!(!should_fallback_after_ime_result(
            ImeSubmitStatus::Committed
        ));
    }

    #[tokio::test]
    async fn submit_prepared_reports_unavailable_session() {
        let controller = WindowsImeSessionController::new();
        let result = controller
            .submit_prepared(
                &PreparedWindowsImeSession::unavailable(),
                ImeSubmitRequest {
                    session_id: "session-1".to_string(),
                    text: "hello".to_string(),
                    created_at: "2026-05-01T12:00:00Z".to_string(),
                    target: None,
                },
            )
            .await;

        assert!(
            matches!(result, Err(WindowsImeSessionError::Ipc(message)) if message == "OpenLess IME session is not active")
        );
    }

    #[test]
    fn restore_decision_uses_confirmed_activation_state_only() {
        // 激活成功且有原快照 → 恢复（决策不再依赖 profile-current 探测，issue #852）。
        let activated = PreparedWindowsImeSession {
            saved_profile: Some(ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409)),
            openless_activated: true,
        };
        assert_eq!(
            restore_decision(
                activated.saved_profile.as_ref(),
                activated.openless_was_activated(),
                activated.activation_failed_with_saved_profile(),
            ),
            ProfileRestoreDecision::RestoreSavedProfile
        );

        // 从未激活（unavailable）→ 保持现状。
        let unavailable = PreparedWindowsImeSession::unavailable();
        assert_eq!(
            restore_decision(
                unavailable.saved_profile.as_ref(),
                unavailable.openless_was_activated(),
                unavailable.activation_failed_with_saved_profile(),
            ),
            ProfileRestoreDecision::KeepCurrentProfile
        );
    }

    #[test]
    fn activation_failed_session_keeps_snapshot_but_cannot_submit() {
        let prepared = PreparedWindowsImeSession::activation_failed(
            ImeProfileSnapshot::keyboard_layout(0x0409, 0x0409_0409),
        );

        assert!(prepared.has_saved_profile());
        assert!(!prepared.openless_was_activated());
        assert!(!prepared.is_ready_for_tsf_submit());
        assert!(prepared.activation_failed_with_saved_profile());
    }
}
