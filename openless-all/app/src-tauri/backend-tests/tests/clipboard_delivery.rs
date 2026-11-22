//! Controlled regression of the production clipboard-to-paste boundary, without OS I/O.
mod types {
    pub use openless_core::shared_types::InsertStatus;
}

#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;

use clipboard_delivery::{ClipboardDelivery, DispatchError};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use types::InsertStatus;

#[derive(Default)]
struct Board {
    text: Option<String>,
    revision: u64,
    dispatched: Vec<String>,
    consumed: Vec<String>,
}

struct Clipboard {
    board: Arc<Mutex<Board>>,
    at_consumption: Option<mpsc::Sender<()>>,
    allow_consumption: Option<mpsc::Receiver<()>>,
    during_consumption: Option<Box<dyn FnOnce() + Send>>,
    dispatch_error: Option<DispatchError>,
    backup_error: bool,
}

impl Clipboard {
    fn new(board: &Arc<Mutex<Board>>) -> Self {
        Self {
            board: Arc::clone(board),
            at_consumption: None,
            allow_consumption: None,
            during_consumption: None,
            dispatch_error: None,
            backup_error: false,
        }
    }
}

impl ClipboardDelivery for Clipboard {
    fn text(&mut self) -> Result<Option<String>, String> {
        if self.backup_error {
            Err("clipboard occupied".into())
        } else {
            Ok(self.board.lock().unwrap().text.clone())
        }
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        external_copy(&self.board, text);
        Ok(())
    }

    fn sequence(&self) -> Option<u64> {
        Some(self.board.lock().unwrap().revision)
    }

    fn paste(&mut self) -> Result<(), DispatchError> {
        if matches!(self.dispatch_error, Some(DispatchError::NotSent)) {
            return Err(self.dispatch_error.take().unwrap());
        }
        let mut board = self.board.lock().unwrap();
        let text = board.text.clone().unwrap_or_default();
        board.dispatched.push(text);
        match self.dispatch_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn wait_for_consumption(&mut self) {
        if let Some(sender) = self.at_consumption.take() {
            sender.send(()).unwrap();
        }
        if let Some(receiver) = self.allow_consumption.take() {
            receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        {
            let mut board = self.board.lock().unwrap();
            let text = board.text.clone().unwrap_or_default();
            board.consumed.push(text);
        }
        if let Some(action) = self.during_consumption.take() {
            action();
        }
    }
}

fn external_copy(board: &Arc<Mutex<Board>>, text: &str) {
    let mut board = board.lock().unwrap();
    board.text = Some(text.into());
    board.revision += 1;
}

#[test]
fn original_protection_and_explicit_copy_cannot_replace_a_pending_paste() {
    for restore in [false, true] {
        let board = Arc::new(Mutex::new(Board::default()));
        external_copy(&board, "user clipboard");
        let mut a = Clipboard::new(&board);
        let (prepared_tx, prepared_rx) = mpsc::channel();
        let (dispatch_tx, dispatch_rx) = mpsc::channel();
        let (at_consumption_tx, at_consumption_rx) = mpsc::channel();
        let (consume_tx, consume_rx) = mpsc::channel();
        a.at_consumption = Some(at_consumption_tx);
        a.allow_consumption = Some(consume_rx);
        let delivery = std::thread::spawn(move || {
            let mut prepared = false;
            clipboard_delivery::insert_with(&mut a, "result A", restore, None, || {
                if prepared {
                    prepared_tx.send(()).unwrap();
                    dispatch_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                prepared = true;
                true
            })
        });
        prepared_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut b = Clipboard::new(&board);
        let (started_tx, started_rx) = mpsc::channel();
        let (copied_tx, copied_rx) = mpsc::channel();
        let protection = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            // Both protect_original and explicit Copy use this production entry.
            clipboard_delivery::copy_with(&mut b, "  original B\n", || true).unwrap();
            copied_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let copied_before_dispatch = copied_rx.recv_timeout(Duration::from_millis(50)).is_ok();
        dispatch_tx.send(()).unwrap();
        at_consumption_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let copied_before_consumption =
            copied_before_dispatch || copied_rx.recv_timeout(Duration::from_millis(50)).is_ok();
        consume_tx.send(()).unwrap();
        assert_eq!(delivery.join().unwrap(), InsertStatus::PasteSent);
        protection.join().unwrap();
        let board = board.lock().unwrap();
        // Before the fix this is ["  original B\n"], despite an unchanged target.
        assert_eq!(board.dispatched, ["result A"]);
        assert_eq!(board.consumed, ["result A"]);
        assert!(!copied_before_consumption);
        assert_eq!(board.text.as_deref(), Some("  original B\n"));
    }
}

#[test]
fn selection_success_restores_complete_original_and_next_borrow_never_restores_result_a() {
    let board = Arc::new(Mutex::new(Board::default()));
    let mut clipboard = Clipboard::new(&board);
    let original = format!(" \n{}\n  ", "original 中段 ".repeat(5000));
    let revision = clipboard_delivery::copy_with(&mut clipboard, &original, || true).unwrap();
    assert_eq!(
        clipboard_delivery::insert_with(&mut clipboard, "result A", true, revision, || true),
        InsertStatus::PasteSent,
    );
    assert_eq!(
        board.lock().unwrap().text.as_deref(),
        Some(original.as_str())
    );
    assert_eq!(
        clipboard_delivery::insert_with(&mut clipboard, "result B", true, None, || true),
        InsertStatus::PasteSent,
    );
    let board = board.lock().unwrap();
    assert_eq!(board.dispatched, ["result A", "result B"]);
    assert_eq!(board.consumed, ["result A", "result B"]);
    assert_eq!(board.text.as_deref(), Some(original.as_str()));
}

#[test]
fn external_copy_during_validation_prevents_dispatch_even_with_identical_text() {
    let board = Arc::new(Mutex::new(Board::default()));
    external_copy(&board, "original");
    let mut clipboard = Clipboard::new(&board);
    let mut validations = 0;
    let status = clipboard_delivery::insert_with(&mut clipboard, "result A", true, None, || {
        validations += 1;
        if validations == 2 {
            external_copy(&board, "result A");
        }
        true
    });
    assert_eq!(status, InsertStatus::NotRequested);
    let board = board.lock().unwrap();
    assert!(board.dispatched.is_empty());
    assert_eq!(board.text.as_deref(), Some("result A"));
    assert_eq!(board.revision, 3);
}

#[test]
fn external_copy_during_consumption_beats_restore_and_conditional_result_fallback() {
    for new_copy in [Some("new user copy"), Some("result A"), None] {
        let board = Arc::new(Mutex::new(Board::default()));
        let mut clipboard = Clipboard::new(&board);
        let revision = clipboard_delivery::copy_with(&mut clipboard, "original", || true).unwrap();
        let external = Arc::clone(&board);
        clipboard.during_consumption = Some(Box::new(move || {
            let mut board = external.lock().unwrap();
            // None models a newly copied image with no text format.
            board.text = new_copy.map(str::to_string);
            board.revision += 1;
        }));
        assert_eq!(
            clipboard_delivery::insert_with(&mut clipboard, "result A", true, revision, || true),
            InsertStatus::PasteSent,
        );
        assert!(!clipboard_delivery::copy_if_current_with(
            &mut clipboard,
            "late fallback",
            revision,
            || true,
        )
        .unwrap());
        let board = board.lock().unwrap();
        assert_eq!(board.dispatched, ["result A"]);
        assert_eq!(board.consumed, ["result A"]);
        assert_eq!(board.text.as_deref(), new_copy);
        assert_eq!(board.revision, 3);
    }
}

#[test]
fn newer_copy_before_selection_delivery_is_neither_borrowed_nor_replaced_by_fallback() {
    let board = Arc::new(Mutex::new(Board::default()));
    let mut clipboard = Clipboard::new(&board);
    let revision = clipboard_delivery::copy_with(&mut clipboard, "original", || true).unwrap();
    external_copy(&board, "original");
    assert_eq!(
        clipboard_delivery::insert_with(&mut clipboard, "result", true, revision, || true),
        InsertStatus::NotRequested,
    );
    assert!(
        !clipboard_delivery::copy_if_current_with(&mut clipboard, "result", revision, || true,)
            .unwrap()
    );
    let board = board.lock().unwrap();
    assert!(board.dispatched.is_empty());
    assert_eq!(board.text.as_deref(), Some("original"));
    assert_eq!(board.revision, 2);
}

#[test]
fn confirmed_no_dispatch_allows_result_fallback_but_uncertain_dispatch_never_does() {
    for uncertain in [false, true] {
        let board = Arc::new(Mutex::new(Board::default()));
        let mut clipboard = Clipboard::new(&board);
        let revision = clipboard_delivery::copy_with(&mut clipboard, "original", || true).unwrap();
        clipboard.dispatch_error = Some(if uncertain {
            DispatchError::OutcomeUnknown
        } else {
            DispatchError::NotSent
        });
        let status =
            clipboard_delivery::insert_with(&mut clipboard, "result", true, revision, || true);
        let board = board.lock().unwrap();
        if uncertain {
            assert_eq!(status, InsertStatus::PasteSent);
            assert_eq!(board.dispatched, ["result"]);
            assert_eq!(board.consumed, ["result"]);
            assert_eq!(board.text.as_deref(), Some("original"));
        } else {
            assert_eq!(status, InsertStatus::CopiedFallback);
            assert!(board.dispatched.is_empty());
            assert!(board.consumed.is_empty());
            assert_eq!(board.text.as_deref(), Some("result"));
        }
    }
}

#[test]
fn waiting_workflow_rechecks_cancellation_and_original_target_before_any_write() {
    for changed in ["control", "position", "original text", "cancelled"] {
        let board = Arc::new(Mutex::new(Board::default()));
        external_copy(&board, "original");
        let gate = clipboard_delivery::lock();
        assert!(clipboard_delivery::try_lock().is_none());
        let captured = (7u64, (5u64, 8u64), " original ".to_string());
        let current = Arc::new(Mutex::new((captured.clone(), false)));
        let check = Arc::clone(&current);
        let mut clipboard = Clipboard::new(&board);
        let (started_tx, started_rx) = mpsc::channel();
        let waiting = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            // Controlled target adapter: the native caller supplies the same four checks.
            clipboard_delivery::insert_with(&mut clipboard, "result", true, Some(1), || {
                let current = check.lock().unwrap();
                !current.1 && current.0 == captured
            })
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        {
            let mut current = current.lock().unwrap();
            match changed {
                "control" => current.0 .0 += 1,
                "position" => current.0 .1 .0 += 1,
                "original text" => current.0 .2 = " modified ".into(),
                "cancelled" => current.1 = true,
                _ => unreachable!(),
            }
        }
        drop(gate);
        assert_eq!(waiting.join().unwrap(), InsertStatus::NotRequested);
        assert_eq!(board.lock().unwrap().revision, 1);
        assert!(board.lock().unwrap().dispatched.is_empty());
        let mut next = Clipboard::new(&board);
        if changed == "cancelled" {
            assert!(
                clipboard_delivery::copy_with(&mut next, "cancelled original", || false).is_err()
            );
            assert!(!clipboard_delivery::copy_if_current_with(
                &mut next,
                "cancelled fallback",
                Some(1),
                || false,
            )
            .unwrap());
        }
        assert_eq!(
            clipboard_delivery::insert_with(&mut next, "next result", true, Some(1), || true),
            InsertStatus::PasteSent,
        );
    }
}

#[test]
fn backup_error_releases_delivery_gate_without_writing_and_next_copy_works() {
    let board = Arc::new(Mutex::new(Board::default()));
    external_copy(&board, "original");
    let mut clipboard = Clipboard::new(&board);
    clipboard.backup_error = true;
    assert_eq!(
        clipboard_delivery::insert_with(&mut clipboard, "result", true, Some(1), || true),
        InsertStatus::Failed,
    );
    assert_eq!(board.lock().unwrap().revision, 1);
    assert!(board.lock().unwrap().dispatched.is_empty());
    clipboard_delivery::copy_with(&mut clipboard, "next explicit copy", || true).unwrap();
    assert_eq!(
        board.lock().unwrap().text.as_deref(),
        Some("next explicit copy")
    );
}
