//! Noninteractive feedback on the existing shared capsule surface.
use openless_core::{
    CapsulePayload, CapsuleState, CapsuleStyle, DictationPhase, VoiceActivationPhase,
    VoiceActivationStateSnapshot,
};

pub(crate) const CAPSULE_AUTO_HIDE_DELAY_MS: u64 = 2000;

pub(crate) fn core_capsule_hide_delay(state: CapsuleState) -> Option<u64> {
    match state {
        CapsuleState::Done | CapsuleState::Error => Some(CAPSULE_AUTO_HIDE_DELAY_MS),
        CapsuleState::Cancelled => Some(0),
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct VoiceActivationFeedback {
    pending: Option<(bool, u64)>,
}

impl VoiceActivationFeedback {
    pub(crate) fn request(&mut self, intent: Option<(bool, u64)>) {
        self.pending = intent;
    }

    pub(crate) fn complete(
        &mut self,
        state: &VoiceActivationStateSnapshot,
        latest: Option<&VoiceActivationStateSnapshot>,
        intent: Option<(bool, u64)>,
    ) -> Option<bool> {
        let (enabled, _) = self.pending?;
        if self.pending != intent {
            self.pending = None;
            return None;
        }
        let latest = latest?;
        use VoiceActivationPhase::*;
        if matches!(
            latest.phase,
            Unavailable | PermissionDenied | MicBusy | Failed
        ) || (enabled
            && matches!(
                latest.phase,
                WakeDetected | HandingOff | ListeningForFinish | Finishing
            ))
        {
            self.pending = None;
            return None;
        }
        if state != latest {
            return None;
        }
        if state.phase == if enabled { ListeningForWake } else { Paused } {
            self.pending = None;
            return Some(enabled);
        }
        None
    }
}

pub(crate) fn can_present_feedback(
    capsule: Option<CapsuleState>,
    dictation: DictationPhase,
    other_owner: bool,
) -> bool {
    !other_owner
        && matches!(
            dictation,
            DictationPhase::Idle
                | DictationPhase::Completed
                | DictationPhase::Cancelled
                | DictationPhase::Failed
        )
        && !matches!(
            capsule,
            Some(CapsuleState::Recording | CapsuleState::Transcribing | CapsuleState::Polishing)
        )
}

pub(crate) fn voice_activation_payload(enabled: bool, style: CapsuleStyle) -> CapsulePayload {
    CapsulePayload {
        session_id: None,
        state: CapsuleState::Done,
        level: 0.0,
        elapsed_ms: 0,
        message: Some(
            if enabled {
                "唤醒监听已开启"
            } else {
                "唤醒监听已暂停"
            }
            .into(),
        ),
        inserted_chars: None,
        translation: false,
        operating: false,
        warming: false,
        capsule_style: style,
        selection_polish: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(phase: VoiceActivationPhase, generation: u64) -> VoiceActivationStateSnapshot {
        VoiceActivationStateSnapshot {
            phase,
            activation_generation: generation,
            session_id: None,
            message: None,
        }
    }

    #[test]
    fn only_a_successful_explicit_click_confirms_once() {
        let mut feedback = VoiceActivationFeedback::default();
        let paused = state(VoiceActivationPhase::Paused, 1);
        assert_eq!(
            feedback.complete(&paused, Some(&paused), Some((false, 1))),
            None
        );
        for enabled in [true, false] {
            let intent = Some((enabled, 2));
            feedback.request(intent);
            let pending = state(
                if enabled {
                    VoiceActivationPhase::Starting
                } else {
                    VoiceActivationPhase::Pausing
                },
                2,
            );
            assert_eq!(feedback.complete(&pending, Some(&pending), intent), None);
            let done = state(
                if enabled {
                    VoiceActivationPhase::ListeningForWake
                } else {
                    VoiceActivationPhase::Paused
                },
                3,
            );
            assert_eq!(feedback.complete(&done, Some(&done), intent), Some(enabled));
            assert_eq!(feedback.complete(&done, Some(&done), intent), None);
            // An automatic rearm without another click cannot repeat the hint.
            assert_eq!(feedback.complete(&pending, Some(&pending), intent), None);
            assert_eq!(feedback.complete(&done, Some(&done), intent), None);
        }
    }

    #[test]
    fn stale_states_and_rapid_intent_changes_do_not_confirm_old_requests() {
        let mut feedback = VoiceActivationFeedback::default();
        feedback.request(Some((true, 2)));
        let listening = state(VoiceActivationPhase::ListeningForWake, 2);
        let pausing = state(VoiceActivationPhase::Pausing, 3);
        assert_eq!(
            feedback.complete(&listening, Some(&pausing), Some((false, 3))),
            None
        );
        feedback.request(Some((false, 3)));
        assert_eq!(
            feedback.complete(&listening, Some(&pausing), Some((false, 3))),
            None
        );
        let paused = state(VoiceActivationPhase::Paused, 4);
        assert_eq!(
            feedback.complete(&paused, Some(&paused), Some((false, 3))),
            Some(false)
        );
        feedback.request(Some((true, 5)));
        assert_eq!(
            feedback.complete(&listening, Some(&paused), Some((true, 5))),
            None
        );
        feedback.request(None); // Resync is not a new user operation.
        assert_eq!(
            feedback.complete(&listening, Some(&listening), Some((true, 5))),
            None
        );
    }

    #[test]
    fn failure_or_wake_handoff_discards_a_delayed_enable_hint() {
        for phase in [
            VoiceActivationPhase::Unavailable,
            VoiceActivationPhase::PermissionDenied,
            VoiceActivationPhase::MicBusy,
            VoiceActivationPhase::Failed,
            VoiceActivationPhase::WakeDetected,
            VoiceActivationPhase::HandingOff,
            VoiceActivationPhase::ListeningForFinish,
            VoiceActivationPhase::Finishing,
        ] {
            let mut feedback = VoiceActivationFeedback::default();
            let intent = Some((true, 1));
            feedback.request(intent);
            let next = state(phase, 2);
            assert_eq!(feedback.complete(&next, Some(&next), intent), None);
            let rearmed = state(VoiceActivationPhase::ListeningForWake, 3);
            assert_eq!(feedback.complete(&rearmed, Some(&rearmed), intent), None);
        }
    }

    #[test]
    fn pause_waits_for_body_finalization_before_confirming() {
        let mut feedback = VoiceActivationFeedback::default();
        let intent = Some((false, 7));
        feedback.request(intent);
        for phase in [
            VoiceActivationPhase::Finishing,
            VoiceActivationPhase::Disabled,
        ] {
            let pending = state(phase, 8);
            assert_eq!(feedback.complete(&pending, Some(&pending), intent), None);
        }
        let paused = state(VoiceActivationPhase::Paused, 9);
        assert_eq!(
            feedback.complete(&paused, Some(&paused), intent),
            Some(false)
        );
    }

    #[test]
    fn active_capsule_owners_always_take_priority() {
        assert!(can_present_feedback(None, DictationPhase::Idle, false));
        for phase in [
            DictationPhase::Starting,
            DictationPhase::Recording,
            DictationPhase::Transcribing,
            DictationPhase::Polishing,
            DictationPhase::Inserting,
        ] {
            assert!(!can_present_feedback(
                Some(CapsuleState::Done),
                phase,
                false
            ));
        }
        for capsule in [
            CapsuleState::Recording,
            CapsuleState::Transcribing,
            CapsuleState::Polishing,
        ] {
            assert!(!can_present_feedback(
                Some(capsule),
                DictationPhase::Idle,
                false
            ));
        }
        assert!(!can_present_feedback(
            Some(CapsuleState::Done),
            DictationPhase::Idle,
            true
        ));
    }

    #[test]
    fn both_capsule_styles_use_done_message_and_the_existing_two_second_hide() {
        for style in [CapsuleStyle::Classic, CapsuleStyle::Siri] {
            for enabled in [true, false] {
                let payload = voice_activation_payload(enabled, style);
                assert_eq!(payload.state, CapsuleState::Done);
                assert_eq!(core_capsule_hide_delay(payload.state), Some(2000));
                assert_eq!(
                    payload.message.as_deref(),
                    Some(if enabled {
                        "唤醒监听已开启"
                    } else {
                        "唤醒监听已暂停"
                    })
                );
                assert_eq!(payload.capsule_style, style);
                assert!(!payload.selection_polish && !payload.warming && !payload.operating);
                assert!(payload.session_id.is_none());
            }
        }
        assert_eq!(core_capsule_hide_delay(CapsuleState::Recording), None);
        assert_eq!(core_capsule_hide_delay(CapsuleState::Error), Some(2000));
        assert_eq!(core_capsule_hide_delay(CapsuleState::Cancelled), Some(0));
    }
}
