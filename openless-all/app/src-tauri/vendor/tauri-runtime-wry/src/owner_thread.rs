//! Windows event-loop targets stay on their owner, including their Rc traffic.
//! Transferable handles own metadata only; callbacks cannot borrow through teardown.
use std::{
  any::Any,
  cell::RefCell,
  collections::BTreeMap,
  fmt,
  marker::PhantomData,
  rc::Rc,
  sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
  thread::{self, ThreadId},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
thread_local! {
  static VALUES: RefCell<BTreeMap<u64, Entry>> = const { RefCell::new(BTreeMap::new()) };
}

struct Metadata {
  id: u64,
  owner: ThreadId,
  alive: AtomicBool,
}
struct Entry {
  metadata: Arc<Metadata>,
  value: Rc<dyn Any>,
}
impl Drop for Entry {
  fn drop(&mut self) {
    // Also retire handles when TLS destroys a deliberately forgotten owner.
    self.metadata.alive.store(false, Ordering::Release);
  }
}

pub(crate) struct Owner<T: 'static> {
  handle: Handle<T>,
  // The owner cannot migrate, even when T itself happens to be Send.
  _owner: PhantomData<Rc<T>>,
}
impl<T: 'static> Owner<T> {
  pub(crate) fn new(value: T) -> Self {
    let id = NEXT_ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
      .expect("owner-thread registration IDs exhausted");
    let metadata = Arc::new(Metadata {
      id,
      owner: thread::current().id(),
      alive: AtomicBool::new(true),
    });
    VALUES.with(|values| {
      let previous = values.borrow_mut().insert(id, Entry {
        metadata: Arc::clone(&metadata), value: Rc::new(value),
      });
      assert!(previous.is_none());
    });
    Self { handle: Handle { metadata, _value: PhantomData }, _owner: PhantomData }
  }

  pub(crate) fn handle(&self) -> Handle<T> { self.handle.clone() }
}
impl<T: 'static> Drop for Owner<T> {
  fn drop(&mut self) {
    self.handle.metadata.alive.store(false, Ordering::Release);
    // Owner is !Send. Drop the removed entry after releasing the registry borrow:
    // a target destructor may synchronously invoke another callback.
    let removed = VALUES.try_with(|values| values.borrow_mut().remove(&self.handle.metadata.id));
    drop(removed);
  }
}

pub struct Handle<T: 'static> {
  metadata: Arc<Metadata>,
  _value: PhantomData<fn() -> T>,
}
impl<T: 'static> Clone for Handle<T> {
  fn clone(&self) -> Self {
    Self { metadata: Arc::clone(&self.metadata), _value: PhantomData }
  }
}
impl<T: 'static> fmt::Debug for Handle<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("OwnerThreadHandle").finish_non_exhaustive()
  }
}
impl<T: 'static> Handle<T> {
  pub(crate) fn is_alive(&self) -> bool { self.metadata.alive.load(Ordering::Acquire) }

  pub(crate) fn with<R>(&self, callback: impl FnOnce(&T) -> R) -> Option<R> {
    if thread::current().id() != self.metadata.owner || !self.is_alive() { return None; }
    let value = VALUES.try_with(|values| {
      values.borrow().get(&self.metadata.id).map(|entry| Rc::clone(&entry.value))
    }).ok().flatten()?;
    // This Rc exists only on the owner. End the map borrow before invoking user
    // code, and keep the target alive if that code retires the runtime reentrantly.
    let value = value.downcast_ref::<T>()?;
    Some(callback(value))
  }
}

#[cfg(test)]
#[path = "../tests/owner_thread.rs"]
mod tests;
