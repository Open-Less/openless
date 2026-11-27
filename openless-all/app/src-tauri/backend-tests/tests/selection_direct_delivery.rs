//! Exercise the production selection handoff and clipboard ownership transaction.
#![allow(dead_code)]
mod types {
    pub use openless_core::shared_types::InsertStatus;
}
#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;
#[path = "../../src/selection/direct_delivery.rs"]
mod direct_delivery;
#[path = "../../src/selection/target_position.rs"]
mod target_position;

use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    text: Option<String>,
    revision: u64,
    writes: Vec<String>,
}
struct Clipboard(Rc<RefCell<State>>);
impl clipboard_delivery::ClipboardDelivery for Clipboard {
    fn text(&mut self) -> Result<Option<String>, String> {
        Ok(self.0.borrow().text.clone())
    }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        state.text = Some(text.into());
        state.revision += 1;
        state.writes.push(text.into());
        Ok(())
    }
    fn sequence(&self) -> Option<u64> {
        Some(self.0.borrow().revision)
    }
    fn paste(&mut self) -> Result<(), clipboard_delivery::DispatchError> {
        panic!("selection delivery must never simulate paste")
    }
    fn wait_for_consumption(&mut self) {
        panic!("direct delivery must never wait for paste consumption")
    }
}

#[test]
fn successful_native_write_never_copies_the_result_even_temporarily() {
    let state = Rc::new(RefCell::new(State::default()));
    let mut clipboard = Clipboard(state.clone());
    let original = format!(
        " \t{} UNIQUE_MIDDLE {}\n ",
        "字".repeat(9000),
        "尾".repeat(9000)
    );
    let revision = clipboard_delivery::copy_with(&mut clipboard, &original, || true).unwrap();
    let result = direct_delivery::deliver(
        || Ok::<_, String>("native readback confirmed"),
        |_| {
            clipboard_delivery::copy_if_current_with(&mut clipboard, "result", revision, || true)?;
            Ok("copied fallback")
        },
    )
    .unwrap();
    assert_eq!(result, "native readback confirmed");
    assert_eq!(state.borrow().writes, [original]);
}

#[test]
fn native_failure_is_confirmed_before_copying_the_result_once() {
    let state = Rc::new(RefCell::new(State::default()));
    let mut clipboard = Clipboard(state.clone());
    let original = " \toriginal\n ";
    let revision = clipboard_delivery::copy_with(&mut clipboard, original, || true).unwrap();
    let attempted = RefCell::new(false);
    direct_delivery::deliver(
        || {
            assert_eq!(state.borrow().writes, [original]);
            *attempted.borrow_mut() = true;
            Err::<(), _>("original control changed".to_string())
        },
        |_| {
            assert!(*attempted.borrow());
            assert!(clipboard_delivery::copy_if_current_with(
                &mut clipboard,
                "result",
                revision,
                || true,
            )?);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(state.borrow().writes, [original, "result"]);
}

#[test]
fn user_copy_and_cancellation_each_prevent_result_clipboard_overwrite() {
    for user_copied in [false, true] {
        let state = Rc::new(RefCell::new(State::default()));
        let mut clipboard = Clipboard(state.clone());
        let revision = clipboard_delivery::copy_with(&mut clipboard, "original", || true).unwrap();
        if user_copied {
            clipboard_delivery::copy_with(&mut clipboard, "user-owned", || true).unwrap();
        }
        let copied = direct_delivery::deliver(
            || Err::<bool, _>("cannot safely deliver".to_string()),
            |_| {
                clipboard_delivery::copy_if_current_with(&mut clipboard, "result", revision, || {
                    user_copied
                })
            },
        )
        .unwrap();
        assert!(!copied);
        assert!(!state.borrow().writes.iter().any(|text| text == "result"));
    }
}
