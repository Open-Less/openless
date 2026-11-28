//! Exercise the production ordered transaction without a real user's clipboard.
mod types { pub use openless_core::shared_types::InsertStatus; }
#[path = "../../src/insertion/clipboard_delivery.rs"]
mod clipboard_delivery;
#[path = "../../src/insertion/material_delivery.rs"]
mod material_delivery;
use clipboard_delivery::{ClipboardDelivery, DispatchError};
use material_delivery::{MaterialClipboard, MaterialTarget, TargetContinuity};
use openless_core::dictation_references::DeliveryPart;
use std::{path::Path, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}};
use types::InsertStatus;
struct Board {
    value: String, revision: u64, sent: Vec<String>, reject_image: bool,
    unknown: Option<usize>, external: bool, target: Arc<AtomicBool>, change_target: bool,
    receipt: Option<(u64, String)>, foreign_after_write: bool, image_not_sent: bool,
    cursor: Arc<Mutex<(u64, u64)>>, reselect_after: Option<usize>,
    activity: Arc<AtomicU64>, navigate_after: Option<usize>,
}
impl Default for Board {
    fn default() -> Self { Self { value: "original".into(), revision: 1, sent: vec![], reject_image: false,
        unknown: None, external: false, target: Arc::new(AtomicBool::new(true)), change_target: false,
        receipt: None, foreign_after_write: false, image_not_sent: false, cursor: Arc::new(Mutex::new((0, 0))), reselect_after: None,
        activity: Arc::new(AtomicU64::new(0)), navigate_after: None } }
}
impl ClipboardDelivery for Board {
    fn text(&mut self) -> Result<Option<String>, String> { Ok(Some(self.value.clone())) }
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.value = text.into(); self.revision += 1; self.receipt = Some((self.revision, self.value.clone()));
        if self.foreign_after_write { self.value = "external B".into(); self.revision += 1; }
        Ok(())
    }
    fn sequence(&self) -> Option<u64> { Some(self.revision) }
    fn owned_sequence(&self) -> Option<u64> { self.receipt.as_ref().map(|r| r.0) }
    fn owns_write(&mut self, receipt: Option<u64>) -> bool { self.receipt.as_ref().is_some_and(|(r, value)| Some(*r) == receipt && *r == self.revision && value == &self.value) }
    fn paste(&mut self) -> Result<(), DispatchError> {
        if self.image_not_sent && self.value.starts_with("image:") { return Err(DispatchError::NotSent); }
        self.sent.push(self.value.clone());
        if self.unknown == Some(self.sent.len()) { Err(DispatchError::OutcomeUnknown) } else { Ok(()) }
    }
    fn wait_for_consumption(&mut self) {
        let mut cursor = self.cursor.lock().unwrap();
        let delta = self.sent.last().map(|text| if text.starts_with("image:") { 1 } else { text.encode_utf16().count() as u64 }).unwrap_or(0);
        *cursor = (cursor.0 + delta, 0);
        if self.reselect_after == Some(self.sent.len()) { *cursor = (999, 5); }
        if self.navigate_after == Some(self.sent.len()) { self.activity.fetch_add(1, Ordering::AcqRel); }
        if self.external { self.value = "new user copy".into(); self.revision += 1; }
        if self.change_target { self.target.store(false, Ordering::Release); }
    }
}
impl MaterialClipboard for Board {
    fn set_image(&mut self, path: &Path) -> Result<(), String> {
        if self.reject_image { return Err("unsupported image".into()); }
        self.value = format!("image:{}", path.display()); self.revision += 1; self.receipt = Some((self.revision, self.value.clone()));
        if self.foreign_after_write { self.value = "external B".into(); self.revision += 1; }
        Ok(())
    }
}
fn parts() -> Vec<DeliveryPart> { vec![DeliveryPart::Text("speech before".into()), DeliveryPart::Image("/owned/copy.png".into()), DeliveryPart::Text("/owned/copy.png speech after".into())] }
struct Gate(Arc<AtomicBool>);
impl MaterialTarget for Gate {
    fn before(&mut self) -> bool { self.0.load(Ordering::Acquire) }
    fn after(&mut self, _: &DeliveryPart) -> bool { self.before() }
}
#[test]
fn native_image_dispatch_always_keeps_following_local_path_without_claiming_consumption() {
    let mut board = Board::default();
    let cursor = board.cursor.clone();
    let continuity = TargetContinuity::new(Some((0, 0)), || true, || Some(*cursor.lock().unwrap()), || Some(0));
    assert_eq!(material_delivery::deliver(&mut board, &parts(), true, continuity), InsertStatus::PasteSent);
    assert_eq!(board.sent, ["speech before", "image:/owned/copy.png", "/owned/copy.png speech after"]);
    assert_eq!(board.value, "original");
    let mut unsupported = Board { reject_image: true, ..Default::default() };
    let gate = Gate(unsupported.target.clone());
    assert_eq!(material_delivery::deliver(&mut unsupported, &parts(), false, gate), InsertStatus::PasteSent);
    assert_eq!(unsupported.sent, ["speech before", "/owned/copy.png speech after"]);
}
#[test]
fn unknown_dispatch_new_user_copy_or_target_change_never_replays_or_writes_remaining_parts() {
    let mut unknown = Board { unknown: Some(2), ..Default::default() };
    let gate = Gate(unknown.target.clone());
    assert_eq!(material_delivery::deliver(&mut unknown, &parts(), true, gate), InsertStatus::PasteSent);
    assert_eq!(unknown.sent.len(), 2);
    let mut external = Board { external: true, ..Default::default() };
    let gate = Gate(external.target.clone());
    material_delivery::deliver(&mut external, &parts(), true, gate);
    assert_eq!(external.sent, ["speech before"]);
    assert_eq!(external.value, "new user copy");
    let mut moved = Board { change_target: true, ..Default::default() };
    let target = moved.target.clone();
    material_delivery::deliver(&mut moved, &parts(), true, Gate(target));
    assert_eq!(moved.sent, ["speech before"]);
}
#[test]
fn write_receipt_never_adopts_external_B_and_definite_image_not_sent_keeps_path() {
    let mut replaced = Board { foreign_after_write: true, ..Default::default() };
    let gate = Gate(replaced.target.clone());
    assert_eq!(material_delivery::deliver(&mut replaced, &parts(), false, gate), InsertStatus::NotRequested);
    assert!(replaced.sent.is_empty()); assert_eq!(replaced.value, "external B");
    let mut image_replaced = Board { foreign_after_write: true, ..Default::default() };
    let gate = Gate(image_replaced.target.clone());
    let image_first = vec![DeliveryPart::Image("/owned/copy.png".into()), DeliveryPart::Text("/owned/copy.png after".into())];
    material_delivery::deliver(&mut image_replaced, &image_first, false, gate);
    assert!(image_replaced.sent.is_empty()); assert_eq!(image_replaced.value, "external B");
    let mut rejected = Board { image_not_sent: true, ..Default::default() };
    let gate = Gate(rejected.target.clone());
    material_delivery::deliver(&mut rejected, &parts(), false, gate);
    assert_eq!(rejected.sent, ["speech before", "/owned/copy.png speech after"]);
}
#[test]
fn same_control_user_reselection_stops_segments_and_copies_only_remaining_text() {
    let mut board = Board { reselect_after: Some(1), ..Default::default() };
    let cursor = board.cursor.clone();
    let target = TargetContinuity::new(Some((0, 0)), || true, || Some(*cursor.lock().unwrap()), || Some(0));
    material_delivery::deliver(&mut board, &parts(), false, target);
    assert_eq!(board.sent, ["speech before"]);
    assert_eq!(board.value, "/owned/copy.png speech after");
    assert!(!board.value.contains("speech before"));
}
#[test]
fn physical_navigation_cannot_masquerade_as_a_one_unit_image_advance() {
    let mut board = Board { navigate_after: Some(2), ..Default::default() };
    let cursor = board.cursor.clone();
    let activity = board.activity.clone();
    let target = TargetContinuity::new(Some((0, 0)), || true, || Some(*cursor.lock().unwrap()), || Some(activity.load(Ordering::Acquire)));
    material_delivery::deliver(&mut board, &parts(), false, target);
    assert_eq!(board.sent, ["speech before", "image:/owned/copy.png"]);
    assert_eq!(board.value, "/owned/copy.png speech after");
}
