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

#[cfg(target_os = "macos")]
mod live_ab {
    use super::*;
    use serde_json::{json, Value};
    use std::{path::PathBuf, process::{Child, Command, Stdio}};

    struct Fixture {
        child: Child,
        directory: tempfile::TempDir,
        command: u64,
    }

    impl Fixture {
        async fn launch() -> Self {
            #[link(name = "ApplicationServices", kind = "framework")]
            extern "C" {
                fn AXIsProcessTrusted() -> bool;
            }
            assert!(unsafe { AXIsProcessTrusted() },
                "The native test runner has no Accessibility permission; no fixture was started");
            let binary = PathBuf::from(std::env::var_os("OPENLESS_NATIVE_FIXTURE_BIN")
                .expect("compile scripts/native-live-input-fixture.swift and set OPENLESS_NATIVE_FIXTURE_BIN"));
            let directory = tempfile::Builder::new().prefix("ask-ab-")
                .tempdir_in(binary.parent().expect("absolute fixture binary path")).unwrap();
            let child = Command::new(&binary).arg("--ask-ab").arg(directory.path())
                .stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
            let mut fixture = Self { child, directory, command: 0 };
            fixture.state().await;
            fixture
        }

        async fn state(&mut self) -> Value {
            for _ in 0..100 {
                assert!(self.child.try_wait().unwrap().is_none(), "fixture exited before acknowledgement");
                if let Some(state) = std::fs::read(self.directory.path().join("state.json")).ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .filter(|state| state["commandId"] == self.command)
                {
                    assert_eq!(state["fixture"], "openless-ask-native-ab-v1");
                    assert_eq!(state["pid"], self.child.id());
                    return state;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            panic!("disposable fixture did not acknowledge command {}", self.command);
        }

        async fn send(&mut self, action: &str) -> Value {
            self.command += 1;
            let temporary = self.directory.path().join("command.pending");
            std::fs::write(&temporary, serde_json::to_vec(&json!({
                "id": self.command, "action": action,
            })).unwrap()).unwrap();
            std::fs::rename(temporary, self.directory.path().join("command.json")).unwrap();
            self.state().await
        }

        async fn expect_text(&mut self, a: &str, b: &str) {
            for _ in 0..30 {
                let state = self.state().await;
                if state["a"]["text"] == a && state["b"]["text"] == b {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            let state = self.state().await;
            assert_eq!(state["a"]["text"], a);
            assert_eq!(state["b"]["text"], b);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    async fn original(tracker: &TargetTracker, fixture: &Fixture, scope: u64) -> CapturedTarget {
        for _ in 0..20 {
            let captured = tracker.capture_before_focus(
                Some(fixture.child.id()), scope, ContextAccess { app_metadata: true, surrounding_text: true },
            ).await;
            if captured.selection == ExternalSelection::None && captured.write_target.is_some() {
                return captured;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        panic!("fixture A's UTF-16 caret could not be captured");
    }

    async fn selection(tracker: &TargetTracker, fixture: &Fixture, scope: u64) -> CapturedTarget {
        for _ in 0..20 {
            // The same native capture request as submission, additionally pinned
            // to our child process so even a foreground race cannot bind user text.
            let captured = tracker.capture_for(
                Some(fixture.child.id()), Some(scope),
                ContextAccess { app_metadata: true, surrounding_text: true }, CAPTURE_TIMEOUT,
            ).await;
            if captured.selection == ExternalSelection::Selected("red".into())
                && captured.write_target.is_some()
            {
                return captured;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        panic!("fixture B's second, repeated UTF-16 selection could not be verified");
    }

    #[tokio::test]
    #[ignore = "requires Accessibility permission; launches only the disposable two-window fixture"]
    async fn real_mac_two_editors_keep_a_caret_and_b_selection_independent() {
        let mut fixture = Fixture::launch().await;
        let tracker = TargetTracker::new();
        tracker.start().unwrap();
        const A: &str = "A🙂 before after";
        const B: &str = "B🙂 red red end";

        let a = original(&tracker, &fixture, 1).await;
        assert_eq!(a.context.before.as_deref(), Some("A🙂 before"));
        fixture.send("focusB").await;
        let b = selection(&tracker, &fixture, 1).await;
        assert_eq!(b.context.before.as_deref(), Some("B🙂 red "));
        // A selection question reads B without any mutation.
        fixture.expect_text(A, B).await;
        // Move A's live caret after capture: insertion must restore the saved one.
        fixture.send("moveA").await;
        fixture.send("focusB").await;
        assert_eq!(tracker.deliver(&a.write_target.unwrap(), WriteOperation::Insert, "INSERT".into(),
            Box::new(|| true)).await, WriteOutcome::Written);
        fixture.expect_text("A🙂 beforeINSERT after", B).await;
        eprintln!("native A/B: insertion restored A's captured UTF-16 caret; B unchanged");

        fixture.send("reset").await;
        let _a = original(&tracker, &fixture, 2).await;
        fixture.send("focusB").await;
        let b = selection(&tracker, &fixture, 2).await;
        assert_eq!(tracker.deliver(&b.write_target.unwrap(), WriteOperation::Replace, "EDIT".into(),
            Box::new(|| true)).await, WriteOutcome::Written);
        fixture.expect_text(A, "B🙂 red EDIT end").await;
        eprintln!("native A/B: edit replaced only B's second repeated selection; A unchanged");

        fixture.send("reset").await;
        let a = original(&tracker, &fixture, 3).await;
        fixture.send("focusB").await;
        fixture.send("changeA").await;
        assert_eq!(tracker.deliver(&a.write_target.unwrap(), WriteOperation::Insert, "MUST NOT WRITE".into(),
            Box::new(|| panic!("invalid A must not claim delivery"))).await, WriteOutcome::NotWritten);
        fixture.expect_text("A changed after capture", B).await;

        fixture.send("reset").await;
        let _a = original(&tracker, &fixture, 4).await;
        fixture.send("focusB").await;
        let b = selection(&tracker, &fixture, 4).await;
        fixture.send("moveB").await;
        assert_eq!(tracker.deliver(&b.write_target.unwrap(), WriteOperation::Replace, "MUST NOT WRITE".into(),
            Box::new(|| panic!("changed B range must not claim delivery"))).await, WriteOutcome::NotWritten);
        fixture.expect_text(A, B).await;

        fixture.send("reset").await;
        let a = original(&tracker, &fixture, 5).await;
        fixture.send("focusB").await;
        assert_eq!(tracker.deliver(&a.write_target.unwrap(), WriteOperation::Insert, "CANCELLED".into(),
            Box::new(|| false)).await, WriteOutcome::Declined);
        fixture.expect_text(A, B).await;
        tracker.stop();
        eprintln!("native A/B: changed document, changed same-text range, and revoked claim left both documents untouched");
    }
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
fn context_is_bounded_around_the_exact_utf16_range_and_excludes_the_selection() {
    let before = "🙂".repeat(450);
    let selected = "private selection";
    let after = "文".repeat(450);
    let document: Vec<_> = format!("{before}{selected}{after}").encode_utf16().collect();
    let start = before.encode_utf16().count();
    let baseline = Baseline::new(document, start..start + selected.encode_utf16().count()).unwrap();
    let context = baseline.context();
    assert_eq!(context.before.as_deref(), Some("🙂".repeat(300).as_str()));
    assert_eq!(context.after.as_deref(), Some("文".repeat(300).as_str()));
    assert!(context.source_app.is_none());
    assert!(context.source_url.is_none());
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
