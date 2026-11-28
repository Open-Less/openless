use super::*;
use std::{cell::Cell, sync::{Barrier, mpsc}, thread};

struct Payload {
  drops: Rc<Cell<usize>>,
  owner: ThreadId,
}
impl Clone for Payload {
  fn clone(&self) -> Self { panic!("the payload must never be cloned") }
}
impl Drop for Payload {
  fn drop(&mut self) {
    assert_eq!(thread::current().id(), self.owner);
    self.drops.set(self.drops.get() + 1);
  }
}
fn payload(drops: &Rc<Cell<usize>>) -> Payload {
  Payload { drops: Rc::clone(drops), owner: thread::current().id() }
}

#[test]
fn workers_only_clone_metadata_and_cannot_access_the_payload() {
  let drops = Rc::new(Cell::new(0));
  let owner = Owner::new(payload(&drops));
  let handle = owner.handle();
  let barrier = Arc::new(Barrier::new(5));
  let workers: Vec<_> = (0..4).map(|_| {
    let handle = handle.clone();
    let barrier = Arc::clone(&barrier);
    thread::spawn(move || {
      barrier.wait();
      for _ in 0..10_000 {
        let clone = handle.clone();
        assert!(clone.is_alive());
        assert!(clone.with(|_| panic!("off-owner access")).is_none());
      }
    })
  }).collect();
  barrier.wait();
  for worker in workers { worker.join().unwrap(); }
  assert_eq!(drops.get(), 0);
  assert!(handle.with(|_| ()).is_some());
  drop(owner);
  assert_eq!(drops.get(), 1);
  assert!(!handle.is_alive());
  assert!(handle.with(|_| ()).is_none());
}

#[test]
fn owner_destroys_payload_before_the_last_worker_handle() {
  let drops = Rc::new(Cell::new(0));
  let owner = Owner::new(payload(&drops));
  let handle = owner.handle();
  let (tx, rx) = mpsc::channel();
  let worker = thread::spawn(move || {
    rx.recv().unwrap();
    assert!(!handle.is_alive());
    assert!(handle.with(|_| ()).is_none());
    drop(handle.clone());
  });
  drop(owner);
  assert_eq!(drops.get(), 1);
  tx.send(()).unwrap();
  worker.join().unwrap();
  assert_eq!(drops.get(), 1);
}

#[test]
fn nested_lookup_and_owner_retirement_keep_the_active_borrow_alive() {
  let drops = Rc::new(Cell::new(0));
  let mut owner = Some(Owner::new(payload(&drops)));
  let handle = owner.as_ref().unwrap().handle();
  handle.with(|_| {
    assert!(handle.with(|_| ()).is_some());
    drop(owner.take());
    assert!(!handle.is_alive());
    assert!(handle.with(|_| ()).is_none());
    assert_eq!(drops.get(), 0);
  }).unwrap();
  assert_eq!(drops.get(), 1);
}

#[test]
fn payload_destructor_can_reenter_another_registration() {
  struct Reenter(Handle<Cell<usize>>);
  impl Drop for Reenter {
    fn drop(&mut self) { self.0.with(|count| count.set(count.get() + 1)).unwrap(); }
  }
  let other = Owner::new(Cell::new(0));
  let handle = other.handle();
  let owner = Owner::new(Reenter(handle.clone()));
  drop(owner);
  assert_eq!(handle.with(Cell::get), Some(1));
}

#[test]
fn unwind_releases_the_local_keepalive_on_the_owner() {
  let drops = Rc::new(Cell::new(0));
  let mut owner = Some(Owner::new(payload(&drops)));
  let handle = owner.as_ref().unwrap().handle();
  let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    handle.with(|_| { drop(owner.take()); panic!("callback unwind"); });
  }));
  assert!(result.is_err());
  assert_eq!(drops.get(), 1);
  assert!(handle.with(|_| ()).is_none());
}

#[test]
fn stale_ids_do_not_refer_to_a_new_registration() {
  let owner = Owner::new(Cell::new(1));
  let stale = owner.handle();
  let old_id = stale.metadata.id;
  drop(owner);
  let replacement = Owner::new(Cell::new(2));
  assert_ne!(old_id, replacement.handle().metadata.id);
  assert!(stale.with(Cell::get).is_none());
  assert_eq!(replacement.handle().with(Cell::get), Some(2));
}

#[test]
fn tls_teardown_retires_forgotten_owner_metadata() {
  let (tx, rx) = mpsc::channel();
  let worker = thread::spawn(move || {
    let owner = Owner::new(Cell::new(7));
    tx.send(owner.handle()).unwrap();
    std::mem::forget(owner);
  });
  let handle = rx.recv().unwrap();
  worker.join().unwrap();
  assert!(!handle.is_alive());
  assert!(handle.with(Cell::get).is_none());
}
