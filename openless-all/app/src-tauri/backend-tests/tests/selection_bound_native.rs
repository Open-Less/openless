//! Production bound transaction with object-addressed reads/writes, not a fake
//! success boolean. A/B deliberately share a window, source and selected range.
#![allow(dead_code)]
#[path = "../../src/selection/bound_native.rs"]
mod bound_native;
mod types {
    pub use openless_core::shared_types::InsertStatus;
}
#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;
#[path = "../../src/selection/direct_delivery.rs"]
mod direct_delivery;
use bound_native::{Binding, Failure, Native, TextRange};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

#[derive(Clone)]
struct Field {
    text: Vec<u16>,
    range: TextRange,
}

struct Desktop {
    fields: RefCell<HashMap<u32, Field>>,
    focus: Cell<u32>,
    permission: Cell<bool>,
    switch_during_gate: Cell<bool>,
    switch_after_submit: Cell<bool>,
    submit_error_after_write: Cell<bool>,
    submit_noop: Cell<bool>,
    reads: RefCell<Vec<u32>>,
    writes: RefCell<Vec<u32>>,
    focus_trace: RefCell<Vec<u32>>,
}
impl Desktop {
    fn fixture() -> (Self, Binding<u32, u32>, String) {
        let original = format!(
            " \t{} UNIQUE_MIDDLE_😀 {}\n ",
            "字".repeat(9000),
            "尾".repeat(9000)
        );
        let field = Field {
            text: format!("前{original}后").encode_utf16().collect(),
            range: TextRange {
                start: 1,
                length: original.encode_utf16().count() as u64,
            },
        };
        let range = field.range;
        (
            Self {
                fields: RefCell::new(HashMap::from([(1, field.clone()), (2, field)])),
                focus: Cell::new(1),
                permission: Cell::new(true),
                switch_during_gate: Cell::new(false),
                switch_after_submit: Cell::new(false),
                submit_error_after_write: Cell::new(false),
                submit_noop: Cell::new(false),
                reads: RefCell::new(Vec::new()),
                writes: RefCell::new(Vec::new()),
                focus_trace: RefCell::new(vec![1]),
            },
            Binding {
                control: 1,
                window: 10,
                range,
            },
            original,
        )
    }
}
impl Native for Desktop {
    type Control = u32;
    type Window = u32;
    fn guard(&self, control: &u32, window: &u32) -> Result<(), Failure> {
        if !self.permission.get() {
            return Err(Failure::Unavailable);
        }
        if *control != 1 || *window != 10 || self.focus.get() != 1 {
            return Err(Failure::TargetChanged);
        }
        if self.switch_during_gate.replace(false) {
            self.focus.set(2);
            self.focus_trace.borrow_mut().push(2);
            // A current-focus supplier at this instant would return B.
            self.focus.set(1);
            self.focus_trace.borrow_mut().push(1);
        }
        Ok(())
    }
    fn selected_range(&self, control: &u32) -> Result<TextRange, Failure> {
        self.reads.borrow_mut().push(*control);
        Ok(self.fields.borrow()[control].range)
    }
    fn text(&self, control: &u32, range: TextRange) -> Result<String, Failure> {
        self.reads.borrow_mut().push(*control);
        let fields = self.fields.borrow();
        let text = fields[control]
            .text
            .get(range.start as usize..(range.start + range.length) as usize)
            .ok_or(Failure::Unavailable)?;
        String::from_utf16(text).map_err(|_| Failure::Unavailable)
    }
    fn writable(&self, control: &u32) -> Result<(), Failure> {
        assert_eq!(*control, 1, "capabilities must belong to the frozen object");
        Ok(())
    }
    fn submit(&self, control: &u32, replacement: &str) -> Result<(), Failure> {
        self.writes.borrow_mut().push(*control);
        if self.submit_noop.get() { return Err(Failure::WriteUnconfirmed); }
        let mut fields = self.fields.borrow_mut();
        let field = fields.get_mut(control).unwrap();
        let replacement: Vec<u16> = replacement.encode_utf16().collect();
        let start = field.range.start as usize;
        let end = start + field.range.length as usize;
        field.text.splice(start..end, replacement.iter().copied());
        field.range = TextRange {
            start: (start + replacement.len()) as u64,
            length: 0,
        };
        if self.switch_after_submit.get() {
            self.focus.set(2);
        }
        if self.submit_error_after_write.get() {
            return Err(Failure::WriteUnconfirmed);
        }
        Ok(())
    }
}

#[test]
fn a_to_b_to_a_can_only_operate_on_frozen_a_even_when_b_is_identical() {
    let (desktop, binding, original) = Desktop::fixture();
    let b = desktop.fields.borrow()[&2].text.clone();
    desktop.switch_during_gate.set(true);
    bound_native::replace(&desktop, &binding, &original, "替换😀").unwrap();
    assert_eq!(*desktop.focus_trace.borrow(), [1, 2, 1]);
    assert_eq!(*desktop.writes.borrow(), [1]);
    assert!(desktop.reads.borrow().iter().all(|object| *object == 1));
    assert_eq!(desktop.fields.borrow()[&2].text, b);
    assert_eq!(
        String::from_utf16(&desktop.fields.borrow()[&1].text).unwrap(),
        "前替换😀后"
    );
}

#[test]
fn another_control_or_window_or_permission_cannot_authorize_a_write() {
    for case in 0..4 {
        let (desktop, mut binding, original) = Desktop::fixture();
        match case {
            0 => binding.control = 2,
            1 => binding.window = 20,
            2 => desktop.focus.set(2),
            _ => desktop.permission.set(false),
        }
        assert!(bound_native::replace(&desktop, &binding, &original, "result").is_err());
        assert!(desktop.writes.borrow().is_empty());
    }
}

#[test]
fn original_range_and_complete_text_must_both_match_before_submission() {
    for change_range in [false, true] {
        let (desktop, binding, original) = Desktop::fixture();
        let mut fields = desktop.fields.borrow_mut();
        if change_range {
            fields.get_mut(&1).unwrap().range.start += 1;
        } else {
            fields.get_mut(&1).unwrap().text[5000] = '改' as u16;
        }
        drop(fields);
        assert_eq!(
            bound_native::replace(&desktop, &binding, &original, "result"),
            Err(Failure::SelectionChanged)
        );
        assert!(desktop.writes.borrow().is_empty());
    }
}

#[test]
fn submission_failure_is_unknown_and_never_retried_or_read_from_new_focus() {
    for setter_error in [false, true] {
        let (desktop, binding, original) = Desktop::fixture();
        desktop.switch_after_submit.set(true);
        desktop.submit_error_after_write.set(setter_error);
        assert_eq!(
            bound_native::replace(&desktop, &binding, &original, "already written"),
            Err(Failure::WriteUnconfirmed)
        );
        assert_eq!(*desktop.writes.borrow(), [1]);
        assert!(desktop.reads.borrow().iter().all(|object| *object == 1));
        assert_eq!(
            String::from_utf16(&desktop.fields.borrow()[&1].text).unwrap(),
            "前already written后"
        );
        assert!(String::from_utf16(&desktop.fields.borrow()[&2].text)
            .unwrap()
            .contains(&original));
    }
}

#[test]
fn unexpected_cf_type_and_non_range_axvalue_never_reach_the_range_decoder() {
    for (is_ax_value, is_range) in [(false, true), (true, false), (true, true)] {
        let kind_calls = Cell::new(0);
        let decode_calls = Cell::new(0);
        let result = bound_native::checked_range(
            || is_ax_value,
            || {
                kind_calls.set(kind_calls.get() + 1);
                is_range
            },
            || {
                decode_calls.set(decode_calls.get() + 1);
                Some((5, 3))
            },
        );
        assert_eq!(kind_calls.get(), usize::from(is_ax_value));
        assert_eq!(decode_calls.get(), usize::from(is_ax_value && is_range));
        assert_eq!(result.is_some(), is_ax_value && is_range);
    }
}

struct RecoveryClipboard<'a> {
    desktop: &'a Desktop,
    text: String,
    revision: u64,
    writes: Vec<String>,
    pastes: usize,
    accept_paste: bool,
}

impl clipboard_delivery::ClipboardDelivery for RecoveryClipboard<'_> {
    fn text(&mut self) -> Result<Option<String>, String> { Ok(Some(self.text.clone())) }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.text = text.into();
        self.revision += 1;
        self.writes.push(text.into());
        Ok(())
    }
    fn sequence(&self) -> Option<u64> { Some(self.revision) }
    fn paste(&mut self) -> Result<(), clipboard_delivery::DispatchError> {
        self.pastes += 1;
        if self.accept_paste {
            self.desktop.submit_noop.set(false);
            self.desktop.submit(&1, &self.text).unwrap();
        }
        Ok(())
    }
    fn wait_for_consumption(&mut self) {}
}

fn recover(
    desktop: &Desktop,
    binding: &Binding<u32, u32>,
    original: &str,
    clipboard: &RefCell<RecoveryClipboard<'_>>,
) -> Result<openless_core::InsertOutcome, openless_core::BackendError> {
    use openless_core::{BackendError, BackendErrorCode, InsertOutcome};
    direct_delivery::deliver(
        || direct_delivery::native_result(
            bound_native::replace(desktop, binding, original, "replacement😀")
                .map_err(|failure| failure.message().into()),
        ),
        |error| direct_delivery::recover_with_paste(
            error,
            || bound_native::confirm_replacement(desktop, binding, "replacement😀").is_ok(),
            || clipboard_delivery::insert_with_confirmation(
                &mut *clipboard.borrow_mut(), "replacement😀", false, Some(1),
                || bound_native::verify(desktop, binding, original).is_ok(),
                Some(|| bound_native::confirm_replacement(desktop, binding, "replacement😀").is_ok()),
            ),
            || {
                let copied = clipboard_delivery::copy_if_current_with(
                    &mut *clipboard.borrow_mut(), "replacement😀", Some(1), || true,
                ).unwrap();
                if copied { Ok(InsertOutcome::CopiedFallback) }
                else { Err(BackendError::new(BackendErrorCode::InvalidState, "selectionResultInHistory")) }
            },
        ),
    )
}

#[test]
fn unconfirmed_ax_noop_recovers_with_one_guarded_paste_and_no_post_success_copy() {
    let (desktop, binding, original) = Desktop::fixture();
    desktop.submit_noop.set(true);
    let clipboard = RefCell::new(RecoveryClipboard {
        desktop: &desktop, text: original.clone(), revision: 1,
        writes: Vec::new(), pastes: 0, accept_paste: true,
    });
    assert_eq!(recover(&desktop, &binding, &original, &clipboard).unwrap(), openless_core::InsertOutcome::Inserted);
    let clipboard = clipboard.borrow();
    assert_eq!(clipboard.pastes, 1);
    assert_eq!(clipboard.writes, ["replacement😀"]);
    assert_eq!(String::from_utf16(&desktop.fields.borrow()[&1].text).unwrap(), "前replacement😀后");
    assert!(String::from_utf16(&desktop.fields.borrow()[&2].text).unwrap().contains(&original));
}

#[test]
fn ax_already_wrote_despite_setter_error_is_confirmed_without_duplicate_paste_or_copy() {
    let (desktop, binding, original) = Desktop::fixture();
    desktop.submit_error_after_write.set(true);
    let clipboard = RefCell::new(RecoveryClipboard {
        desktop: &desktop, text: original.clone(), revision: 1,
        writes: Vec::new(), pastes: 0, accept_paste: true,
    });
    assert_eq!(recover(&desktop, &binding, &original, &clipboard).unwrap(), openless_core::InsertOutcome::Inserted);
    assert_eq!(clipboard.borrow().pastes, 0);
    assert!(clipboard.borrow().writes.is_empty());
    assert_eq!(*desktop.writes.borrow(), [1]);
}

#[test]
fn changed_focus_or_window_copies_result_without_native_write_or_paste() {
    for changed_window in [false, true] {
        let (desktop, mut binding, original) = Desktop::fixture();
        if changed_window { binding.window = 20; } else { desktop.focus.set(2); }
        let clipboard = RefCell::new(RecoveryClipboard {
            desktop: &desktop, text: original.clone(), revision: 1,
            writes: Vec::new(), pastes: 0, accept_paste: true,
        });
        assert_eq!(recover(&desktop, &binding, &original, &clipboard).unwrap(), openless_core::InsertOutcome::CopiedFallback);
        assert!(desktop.writes.borrow().is_empty());
        assert_eq!(clipboard.borrow().pastes, 0);
        assert_eq!(clipboard.borrow().writes, ["replacement😀"]);
    }
}

#[test]
fn unconfirmed_paste_retains_result_reports_unknown_and_never_retries() {
    let (desktop, binding, original) = Desktop::fixture();
    desktop.submit_noop.set(true);
    let clipboard = RefCell::new(RecoveryClipboard {
        desktop: &desktop, text: original.clone(), revision: 1,
        writes: Vec::new(), pastes: 0, accept_paste: false,
    });
    let error = recover(&desktop, &binding, &original, &clipboard).unwrap_err();
    assert_eq!(error.code, openless_core::BackendErrorCode::OutcomeUnknown);
    assert_eq!(error.message, "selectionPolishWriteUnconfirmed");
    assert_eq!(error.details.unwrap()["result_copied"], true);
    assert_eq!(clipboard.borrow().pastes, 1);
    assert_eq!(clipboard.borrow().writes, ["replacement😀"]);
    assert_eq!(*desktop.writes.borrow(), [1], "only the original AX attempt was made");
}

#[test]
fn recovery_respects_a_newer_user_clipboard_revision() {
    let (desktop, binding, original) = Desktop::fixture();
    desktop.submit_noop.set(true);
    let clipboard = RefCell::new(RecoveryClipboard {
        desktop: &desktop, text: "user copy".into(), revision: 2,
        writes: Vec::new(), pastes: 0, accept_paste: true,
    });
    let error = recover(&desktop, &binding, &original, &clipboard).unwrap_err();
    assert_eq!(error.code, openless_core::BackendErrorCode::OutcomeUnknown);
    assert_eq!(error.details.unwrap()["result_copied"], false);
    assert_eq!(clipboard.borrow().text, "user copy");
    assert_eq!(clipboard.borrow().pastes, 0);
    assert!(clipboard.borrow().writes.is_empty());
}
