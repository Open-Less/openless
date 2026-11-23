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
    clipboard.backup_error = false;
    clipboard.backup_error = false;
    clipboard_delivery::copy_with(&mut clipboard, "next explicit copy", || true).unwrap();
    assert_eq!(
        board.lock().unwrap().text.as_deref(),
        Some("next explicit copy")
    );
}

#[test]
fn ordinary_unconfirmed_paste_retains_complete_text_and_new_copy_wins() {
    for confirmed in [false, true] {
        let board = Arc::new(Mutex::new(Board::default()));
        external_copy(&board, "old clipboard");
        let mut clipboard = Clipboard::new(&board);
        let result = clipboard_delivery::insert_with_confirmation(
            &mut clipboard,
            "完整 result\n尾",
            true,
            None,
            || true,
            Some(|| confirmed),
        );
        assert_eq!(
            result,
            if confirmed {
                InsertStatus::Inserted
            } else {
                InsertStatus::PasteSent
            }
        );
        assert_eq!(
            board.lock().unwrap().text.as_deref(),
            Some(if confirmed {
                "old clipboard"
            } else {
                "完整 result\n尾"
            })
        );
        clipboard_delivery::copy_with(&mut clipboard, "manual fallback", || true).unwrap();
        assert_eq!(
            board.lock().unwrap().text.as_deref(),
            Some("manual fallback")
        );
        clipboard_delivery::copy_with(&mut clipboard, "user new copy", || true).unwrap();
        assert_eq!(board.lock().unwrap().text.as_deref(), Some("user new copy"));
    }
}

// Test-only: arboard restores one NSString, not arbitrary pasteboard formats.
struct NativeAcceptanceClipboard<C: ClipboardDelivery> {
    clipboard: C,
    original: String,
    expected_revision: u64,
    owned_revision: Option<u64>,
}

impl<C: ClipboardDelivery> NativeAcceptanceClipboard<C> {
    fn new(
        mut clipboard: C,
        item_types: impl FnOnce() -> Result<Vec<Vec<String>>, String>,
    ) -> Result<Self, String> {
        let revision = clipboard
            .sequence()
            .ok_or("clipboard revision unavailable")?;
        if item_types()? != [vec!["public.utf8-plain-text".to_string()]] {
            return Err("acceptance requires one item with only public.utf8-plain-text; no writes performed".into());
        }
        let original = clipboard.text()?.ok_or("clipboard text unavailable")?;
        if clipboard.sequence() != Some(revision) {
            return Err("clipboard changed during admission; no writes performed".into());
        }
        Ok(Self {
            clipboard,
            original,
            expected_revision: revision,
            owned_revision: None,
        })
    }
    fn restore_if_owned(&mut self) -> Result<bool, String> {
        let Some(revision) = self.owned_revision.take() else {
            return Ok(false);
        };
        if self.clipboard.sequence() != Some(revision) {
            return Ok(false);
        }
        self.clipboard.set_text(&self.original)?;
        Ok(true)
    }
}
impl<C: ClipboardDelivery> ClipboardDelivery for NativeAcceptanceClipboard<C> {
    fn text(&mut self) -> Result<Option<String>, String> {
        self.clipboard.text()
    }
    fn sequence(&self) -> Option<u64> {
        self.clipboard.sequence()
    }
    fn paste(&mut self) -> Result<(), DispatchError> {
        self.clipboard.paste()
    }
    fn wait_for_consumption(&mut self) {
        self.clipboard.wait_for_consumption();
    }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        if self.clipboard.sequence() != Some(self.expected_revision) {
            return Err("clipboard changed outside acceptance; write refused".into());
        }
        self.owned_revision = None;
        self.clipboard.set_text(text)?;
        let revision = self
            .clipboard
            .sequence()
            .ok_or("clipboard write revision unavailable")?;
        if self.clipboard.text()?.as_deref() != Some(text)
            || self.clipboard.sequence() != Some(revision)
        {
            return Err("clipboard write receipt changed; restoration refused".into());
        }
        self.expected_revision = revision;
        self.owned_revision = Some(revision);
        Ok(())
    }
}
impl<C: ClipboardDelivery> Drop for NativeAcceptanceClipboard<C> {
    fn drop(&mut self) {
        if let Err(error) = self.restore_if_owned() {
            eprintln!("acceptance clipboard restoration failed: {error}");
        }
    }
}

#[test]
fn native_acceptance_guard_refuses_non_restorable_types_without_writes() {
    for types in [
        vec![],
        vec![vec!["public.utf8-plain-text", "public.html"]],
        vec![vec!["public.utf8-plain-text", "public.rtf"]],
        vec![vec!["public.utf8-plain-text", "public.file-url"]],
        vec![vec!["public.utf16-plain-text"]],
        vec![
            vec!["public.utf8-plain-text"],
            vec!["public.utf8-plain-text"],
        ],
    ] {
        let board = Arc::new(Mutex::new(Board::default()));
        external_copy(
            &board,
            "readable text does not prove all formats are restorable",
        );
        let result = NativeAcceptanceClipboard::new(Clipboard::new(&board), || {
            Ok(types
                .into_iter()
                .map(|item| item.into_iter().map(str::to_string).collect())
                .collect())
        });
        assert!(result.is_err());
        assert_eq!(board.lock().unwrap().revision, 1);
    }
}
#[test]
fn native_acceptance_guard_restores_only_its_last_owned_plain_text_revision() {
    for external in [false, true] {
        let board = Arc::new(Mutex::new(Board::default()));
        external_copy(&board, " original\n完整 ");
        {
            let mut guarded = NativeAcceptanceClipboard::new(Clipboard::new(&board), || {
                Ok(vec![vec!["public.utf8-plain-text".into()]])
            })
            .unwrap();
            guarded.set_text("probe 1").unwrap();
            guarded.set_text("probe 2").unwrap();
            assert_eq!(guarded.owned_revision, Some(3));
            if external {
                // Even a user's new copy of identical text revokes test ownership.
                external_copy(&board, "probe 2");
                assert!(guarded.set_text("must not overwrite a newer copy").is_err());
            }
        }
        let board = board.lock().unwrap();
        assert_eq!(board.revision, 4);
        assert_eq!(
            board.text.as_deref(),
            Some(if external {
                "probe 2"
            } else {
                " original\n完整 "
            })
        );
    }
}
#[test]
fn native_acceptance_guard_refuses_changes_during_admission_without_writes() {
    let board = Arc::new(Mutex::new(Board::default()));
    external_copy(&board, "original");
    let result = NativeAcceptanceClipboard::new(Clipboard::new(&board), || {
        external_copy(&board, "external copy during metadata read");
        Ok(vec![vec!["public.utf8-plain-text".into()]])
    });
    assert!(result.is_err());
    assert_eq!(board.lock().unwrap().revision, 2);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "explicit local clipboard acceptance; never runs in CI"]
fn native_clipboard_fallback_is_readable_and_not_restored_after_an_unconfirmed_paste() {
    struct Native {
        clipboard: arboard::Clipboard,
    }
    impl ClipboardDelivery for Native {
        fn text(&mut self) -> Result<Option<String>, String> {
            self.clipboard
                .get_text()
                .map(Some)
                .map_err(|e| e.to_string())
        }
        fn set_text(&mut self, text: &str) -> Result<(), String> {
            self.clipboard
                .set_text(text.to_string())
                .map_err(|e| e.to_string())
        }
        fn sequence(&self) -> Option<u64> {
            unsafe {
                use objc2::{
                    msg_send,
                    runtime::{AnyClass, AnyObject},
                };
                let board: *mut AnyObject =
                    msg_send![AnyClass::get("NSPasteboard").unwrap(), generalPasteboard];
                let value: isize = msg_send![board, changeCount];
                Some(value as u64)
            }
        }
        fn paste(&mut self) -> Result<(), DispatchError> {
            Err(DispatchError::NotSent)
        }
        fn wait_for_consumption(&mut self) {}
    }
    // Read all declared types before accepting arboard's text-only backup.
    fn item_types() -> Result<Vec<Vec<String>>, String> {
        unsafe {
            use objc2::{
                msg_send,
                runtime::{AnyClass, AnyObject},
            };
            let board: *mut AnyObject =
                msg_send![AnyClass::get("NSPasteboard").unwrap(), generalPasteboard];
            if board.is_null() {
                return Err("pasteboard unavailable; no writes performed".into());
            }
            let items: *mut AnyObject = msg_send![board, pasteboardItems];
            if items.is_null() {
                return Err("pasteboard items unavailable; no writes performed".into());
            }
            let count: usize = msg_send![items, count];
            if count != 1 {
                return Err("acceptance requires exactly one item; no writes performed".into());
            }
            let item: *mut AnyObject = msg_send![items, objectAtIndex: 0usize];
            let types: *mut AnyObject = msg_send![item, types];
            let count: usize = msg_send![types, count];
            if count != 1 {
                return Err(
                    "acceptance refuses multiple representations; no writes performed".into(),
                );
            }
            let name: *mut AnyObject = msg_send![types, objectAtIndex: 0usize];
            let name: *const std::ffi::c_char = msg_send![name, UTF8String];
            if name.is_null() {
                return Err("pasteboard type unavailable; no writes performed".into());
            }
            let name = std::ffi::CStr::from_ptr(name)
                .to_str()
                .map_err(|_| "invalid pasteboard type")?;
            Ok(vec![vec![name.to_string()]])
        }
    }
    let native = Native {
        clipboard: arboard::Clipboard::new().unwrap(),
    };
    let mut native = NativeAcceptanceClipboard::new(native, item_types)
        .expect("native acceptance refused unsupported or changing clipboard before any write");
    let expected = "OpenLess native receipt acceptance\n完整结果尾";
    assert_eq!(
        clipboard_delivery::insert_with_confirmation(
            &mut native,
            expected,
            true,
            None,
            || true,
            Some(|| false)
        ),
        InsertStatus::CopiedFallback
    );
    assert_eq!(native.text().unwrap().unwrap(), expected);
    let receipt = clipboard_delivery::copy_with(&mut native, expected, || true).unwrap();
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(native.text().unwrap().unwrap(), expected);
    assert_eq!(native.sequence(), receipt);
    // External copies are simulated above, never impersonated on the user's board.
    assert!(native.restore_if_owned().unwrap());
}

#[test]
fn a_successful_set_without_retained_text_is_not_a_copy_receipt() {
    struct Lost;
    impl ClipboardDelivery for Lost {
        fn text(&mut self) -> Result<Option<String>, String> {
            Ok(Some("old".into()))
        }
        fn set_text(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn sequence(&self) -> Option<u64> {
            Some(1)
        }
        fn paste(&mut self) -> Result<(), DispatchError> {
            panic!("copy never pastes")
        }
        fn wait_for_consumption(&mut self) {
            panic!("copy never dispatches")
        }
    }
    assert!(clipboard_delivery::copy_with(&mut Lost, "complete result", || true).is_err());
}
