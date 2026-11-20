use super::*;
use std::cell::Cell;

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires Accessibility permission and the disposable native-live-input-fixture app"]
async fn real_mac_editor_replaces_only_the_verified_fixture_selection() {
    let pid: i32 = std::env::var("OPENLESS_NATIVE_FIXTURE_PID")
        .expect("provide the disposable native-live-input-fixture process ID")
        .parse()
        .unwrap();
    let tracker = TargetTracker::new();
    tracker.start().unwrap();
    let mut captured = None;
    for _ in 0..12 {
        if crate::selection::current_front_app_pid() != Some(pid) {
            tokio::time::sleep(Duration::from_millis(80)).await;
            continue;
        }
        let target = tracker.capture().await;
        if crate::selection::current_front_app_pid() == Some(pid)
            && target.selection == ExternalSelection::Selected("原文".into())
        {
            if let Some(write_target) = target.write_target {
                captured = Some(write_target);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    let target = captured.expect("the original disposable selection must be writable");
    assert_eq!(crate::selection::current_front_app_pid(), Some(pid));
    let outcome = tracker
        .deliver(
            &target,
            WriteOperation::Replace,
            "Ask fixture result".into(),
            Box::new(move || crate::selection::current_front_app_pid() == Some(pid)),
        )
        .await;
    tracker.stop();
    assert_eq!(outcome, WriteOutcome::Written);
}

#[test]
fn own_focus_preserves_bound_editor_but_an_unreadable_external_visit_clears_it() {
    let now = Instant::now();
    let mut latest = Latest::new(now);
    latest.refresh(Foreground::External(1), 1, now, false, |_| Some("first"));
    latest.refresh(Foreground::Own, 1, now, true, |_| panic!("do not bind Ask"));
    assert_eq!(latest.bound, Some("first"));

    latest.refresh(Foreground::External(2), 2, now, false, |_| None);
    latest.refresh(Foreground::Own, 2, now, true, |_| panic!("do not bind Ask"));
    assert_eq!(latest.bound, None);
}

#[test]
fn latest_external_observation_replaces_another_control_in_the_same_app() {
    let now = Instant::now();
    let mut latest = Latest::new(now);
    latest.refresh(Foreground::External(1), 1, now, false, |_| Some("first"));
    latest.refresh(Foreground::External(1), 2, now, false, |_| Some("second"));
    latest.refresh(Foreground::Own, 2, now, true, |_| panic!("do not bind Ask"));
    assert_eq!(latest.bound, Some("second"));
}

#[test]
fn unsampled_external_transition_invalidates_older_focus_and_submit_forces_refresh() {
    let now = Instant::now();
    let mut latest = Latest::new(now);
    latest.refresh(Foreground::External(1), 1, now, false, |_| Some("old"));
    latest.refresh(Foreground::Own, 2, now, true, |_| panic!("do not bind Ask"));
    assert_eq!(latest.bound, None);
    latest.refresh(Foreground::External(1), 3, now, false, |_| Some("observed"));
    latest.refresh(Foreground::External(1), 3, now, true, |_| Some("at send"));
    assert_eq!(latest.bound, Some("at send"));
}

#[test]
fn replacement_uses_original_utf16_offsets_not_the_first_matching_text() {
    let doc: Vec<_> = "🙂red\r\nred".encode_utf16().collect();
    let baseline = Baseline::new(doc.clone(), 7..10).unwrap();
    assert!(!baseline.permits(WriteOperation::Insert));
    assert!(baseline.permits(WriteOperation::Replace));
    let expected = baseline.expected(&[b'X' as u16]).unwrap();
    assert_eq!(String::from_utf16(&expected).unwrap(), "🙂red\r\nX");
    assert!(Baseline::new(doc.clone(), 1..1).is_none());
    assert!(Baseline::new(doc.clone(), 7..11).is_none());
    assert!(Baseline::new(doc.clone(), 2..5).unwrap() != baseline);
    let mut changed_away_from_range = doc;
    changed_away_from_range[2] = b'X' as u16;
    assert!(Baseline::new(changed_away_from_range, 7..10).unwrap() != baseline);
}

#[test]
fn insertion_requires_a_collapsed_position_and_preserves_everything_around_it() {
    let doc: Vec<_> = "🙂\nword".encode_utf16().collect();
    let caret = Baseline::new(doc, 2..2).unwrap();
    assert!(caret.permits(WriteOperation::Insert));
    assert!(!caret.permits(WriteOperation::Replace));
    let generated: Vec<_> = "❄".encode_utf16().collect();
    assert_eq!(
        String::from_utf16(&caret.expected(&generated).unwrap()).unwrap(),
        "🙂❄\nword"
    );
}

#[test]
fn selection_keeps_whitespace_and_rejects_overlong_or_malformed_data() {
    assert_eq!(selection_from_units(&[]), Some(ExternalSelection::None));
    assert_eq!(
        selection_from_units(&" \r\n🙂 ".encode_utf16().collect::<Vec<_>>()),
        Some(ExternalSelection::Selected(" \r\n🙂 ".into()))
    );
    assert_eq!(selection_from_units(&[0xdc00]), None);
    assert_eq!(
        selection_from_units(&vec![b'x' as u16; MAX_SELECTION_SCALARS + 1]),
        None
    );
}

#[test]
fn abandoned_and_stopped_attempts_cannot_run_the_authorization_callback() {
    let phase = DeliveryPhase::default();
    let running = AtomicBool::new(true);
    assert_eq!(phase.abandon(), WriteOutcome::NotWritten);
    assert_eq!(
        phase.claim(&running, Box::new(|| panic!("expired"))),
        Err(WriteOutcome::NotWritten)
    );
    let phase = DeliveryPhase::default();
    running.store(false, Ordering::Release);
    assert_eq!(
        phase.claim(&running, Box::new(|| panic!("stopped"))),
        Err(WriteOutcome::NotWritten)
    );
}

#[test]
fn declined_claim_does_not_write_and_a_claimed_timeout_is_unknown() {
    let running = AtomicBool::new(true);
    let phase = DeliveryPhase::default();
    assert_eq!(
        phase.claim(&running, Box::new(|| false)),
        Err(WriteOutcome::Declined)
    );
    assert_eq!(phase.abandon(), WriteOutcome::Declined);

    let phase = DeliveryPhase::default();
    assert_eq!(phase.claim(&running, Box::new(|| true)), Ok(()));
    assert_eq!(
        phase.claim(&running, Box::new(|| panic!("double claim"))),
        Err(WriteOutcome::NotWritten)
    );
    assert_eq!(phase.abandon(), WriteOutcome::Unknown);
}

#[test]
fn timeout_during_authorization_does_not_start_a_late_write() {
    let phase = Arc::new(DeliveryPhase::default());
    let timing = phase.clone();
    let outcome = Cell::new(None);
    let active = AtomicBool::new(true);
    assert_eq!(
        phase.claim(
            &active,
            Box::new(move || {
                outcome.set(Some(timing.abandon()));
                assert_eq!(outcome.take(), Some(WriteOutcome::NotWritten));
                true
            }),
        ),
        Err(WriteOutcome::NotWritten)
    );
}
