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
    OutcomeUnknown(String),
}

impl std::fmt::Display for WindowsImeSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile(message) | Self::Ipc(message) | Self::OutcomeUnknown(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for WindowsImeSessionError {}

impl WindowsImeSessionError {
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, Self::OutcomeUnknown(_))
    }
}

pub fn map_ime_status_to_insert_status(status: ImeSubmitStatus) -> InsertStatus {
    match status {
        ImeSubmitStatus::Committed => InsertStatus::Inserted,
        // The DLL's rejection/failure only proves "not committed"; it never wrote the clipboard.
        // Return Failed so the caller performs the real fallback per user settings; only paths
        // with a successful clipboard write qualify to report CopiedFallback. OutcomeUnknown is
        // still passed separately via Err.
        ImeSubmitStatus::Rejected | ImeSubmitStatus::Failed => InsertStatus::Failed,
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

            // Diagnostic: OpenLess was already the current IME at session start -> the previous
            // session's restore likely failed. Still activate as usual (idempotent);
            // restore_session's sticky-state guard will skip "restore", avoiding hardcoding
            // OpenLess as the original IME (the failure self-sticking from issue #852).
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

        let status = self.ipc.submit_text(request).await.map_err(|error| {
            if error.is_outcome_unknown() {
                WindowsImeSessionError::OutcomeUnknown(error.to_string())
            } else {
                WindowsImeSessionError::Ipc(error.to_string())
            }
        })?;
        if should_fallback_after_ime_result(status) {
            log::warn!(
                "[windows-ime] TSF submit returned {status:?}; falling back to non-TSF insertion"
            );
        }
        Ok(map_ime_status_to_insert_status(status))
    }

    /// Restore the IME from before the session.
    ///
    /// Known limitation: restore is unconditional — an IME the user manually switched to
    /// mid-session is also overwritten with the pre-session snapshot at the end
    /// (`GetActiveProfile` probing is unreliable from a background thread in the OpenLess
    /// process and cannot drive control flow, issue #852).
    pub fn restore_session(&self, prepared: PreparedWindowsImeSession) {
        let saved_profile = prepared.saved_profile.as_ref();
        let openless_was_activated = prepared.openless_was_activated();
        let activation_failed = prepared.activation_failed_with_saved_profile();

        // Diagnostic: log the decision basis + the profile probed as current before restore
        // (does not affect the decision). The issue #852 restore decision relies only on the
        // session's known activation facts, not on this probe.
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

        // Restore flow (sticky guard / retry / diagnostics) lives in windows_ime_restore so it
        // can be unit-tested cross-platform.
        // The outcome only adds a debug diagnostic; success/failure/skip details are already
        // logged inside the flow.
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
    fn rejected_ime_result_never_claims_that_the_clipboard_was_written() {
        for status in [ImeSubmitStatus::Rejected, ImeSubmitStatus::Failed] {
            assert_eq!(
                map_ime_status_to_insert_status(status),
                InsertStatus::Failed
            );
        }
    }

    #[test]
    fn rejected_ime_result_requests_fallback() {
        assert!(should_fallback_after_ime_result(ImeSubmitStatus::Rejected));
        assert!(should_fallback_after_ime_result(ImeSubmitStatus::Failed));
        assert!(!should_fallback_after_ime_result(
            ImeSubmitStatus::Committed
        ));
    }

    #[test]
    fn outcome_unknown_is_distinct_from_a_definite_ipc_failure() {
        assert!(WindowsImeSessionError::OutcomeUnknown("fixture".to_string()).is_outcome_unknown());
        assert!(!WindowsImeSessionError::Ipc("fixture".to_string()).is_outcome_unknown());
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
        // Activated and a pre-session snapshot exists -> restore (the decision no longer relies
        // on profile-current probing, issue #852).
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

        // Never activated (unavailable) -> keep as is.
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
