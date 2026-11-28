//! Exercise the production ordered transaction without a real user's clipboard.
mod types { pub use openless_core::shared_types::InsertStatus; }
#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;
#[path = "../../src/insertion/material_delivery.rs"]
mod material_delivery;
use clipboard_delivery::{ClipboardDelivery, DispatchError};
use material_delivery::MaterialClipboard;
use openless_core::dictation_references::DeliveryPart;
use std::{path::Path, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use types::InsertStatus;
struct Board {
    value: String, revision: u64, sent: Vec<String>, reject_image: bool,
    unknown: Option<usize>, external: bool, target: Arc<AtomicBool>, change_target: bool,
}
impl Default for Board {
    fn default() -> Self { Self { value: "original".into(), revision: 1, sent: vec![], reject_image: false,
        unknown: None, external: false, target: Arc::new(AtomicBool::new(true)), change_target: false } }
}
impl ClipboardDelivery for Board {
    fn text(&mut self) -> Result<Option<String>, String> { Ok(Some(self.value.clone())) }
    fn set_text(&mut self, text: &str) -> Result<(), String> { self.value = text.into(); self.revision += 1; Ok(()) }
    fn sequence(&self) -> Option<u64> { Some(self.revision) }
    fn paste(&mut self) -> Result<(), DispatchError> {
        self.sent.push(self.value.clone());
        if self.unknown == Some(self.sent.len()) { Err(DispatchError::OutcomeUnknown) } else { Ok(()) }
    }
    fn wait_for_consumption(&mut self) {
        if self.external { self.value = "new user copy".into(); self.revision += 1; }
        if self.change_target { self.target.store(false, Ordering::Release); }
    }
}
impl MaterialClipboard for Board {
    fn set_image(&mut self, path: &Path) -> Result<(), String> {
        if self.reject_image { return Err("unsupported image".into()); }
        self.value = format!("image:{}", path.display()); self.revision += 1; Ok(())
    }
}
fn parts() -> Vec<DeliveryPart> { vec![DeliveryPart::Text("speech before".into()), DeliveryPart::Image("/owned/copy.png".into()), DeliveryPart::Text("/owned/copy.png speech after".into())] }
#[test]
fn native_image_dispatch_always_keeps_following_local_path_without_claiming_consumption() {
    let mut board = Board::default();
    assert_eq!(material_delivery::deliver(&mut board, &parts(), true, |_| true), InsertStatus::PasteSent);
    assert_eq!(board.sent, ["speech before", "image:/owned/copy.png", "/owned/copy.png speech after"]);
    assert_eq!(board.value, "original");
    let mut unsupported = Board { reject_image: true, ..Default::default() };
    assert_eq!(material_delivery::deliver(&mut unsupported, &parts(), false, |_| true), InsertStatus::PasteSent);
    assert_eq!(unsupported.sent, ["speech before", "/owned/copy.png speech after"]);
}
#[test]
fn unknown_dispatch_new_user_copy_or_target_change_never_replays_or_writes_remaining_parts() {
    let mut unknown = Board { unknown: Some(2), ..Default::default() };
    assert_eq!(material_delivery::deliver(&mut unknown, &parts(), true, |_| true), InsertStatus::PasteSent);
    assert_eq!(unknown.sent.len(), 2);
    let mut external = Board { external: true, ..Default::default() };
    material_delivery::deliver(&mut external, &parts(), true, |_| true);
    assert_eq!(external.sent, ["speech before"]);
    assert_eq!(external.value, "new user copy");
    let mut moved = Board { change_target: true, ..Default::default() };
    let target = moved.target.clone();
    material_delivery::deliver(&mut moved, &parts(), true, |_| target.load(Ordering::Acquire));
    assert_eq!(moved.sent, ["speech before"]);
}
