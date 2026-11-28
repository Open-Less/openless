//! Actions-only real-runtime regression; no application, ASR, or WebView instance.
use std::{sync::{Arc, Barrier, atomic::{AtomicUsize, Ordering}}, thread};
use tauri_runtime::{RunEvent, Runtime, RuntimeHandle};
use tauri_runtime_wry::Wry;

fn assert_retired(handle: <Wry<()> as Runtime<()>>::Handle) {
  assert!(handle.run_on_main_thread(|| panic!("late callback executed")).is_err());
  thread::spawn(move || {
    let clone = handle.clone();
    assert!(clone.run_on_main_thread(|| panic!("late worker callback executed")).is_err());
  }).join().unwrap();
}

fn main() {
  let mode = std::env::args().nth(1).expect("lifecycle mode");
  let mut runtime = Wry::<()>::new(Default::default()).expect("create runtime on owner");
  let late = runtime.handle();
  assert!(late.display_handle().is_ok());
  if mode == "unstarted" {
    drop(runtime);
    assert_retired(late);
    return;
  }
  if mode == "iterations" {
    let barrier = Arc::new(Barrier::new(9));
    let finished = Arc::new(AtomicUsize::new(0));
    let callbacks = Arc::new(AtomicUsize::new(0));
    let owner = thread::current().id();
    let workers: Vec<_> = (0..8).map(|_| {
      let handle = late.clone();
      let barrier = Arc::clone(&barrier);
      let finished = Arc::clone(&finished);
      let callbacks = Arc::clone(&callbacks);
      thread::spawn(move || {
        barrier.wait();
        for _ in 0..100_000 { drop(handle.clone()); }
        handle.run_on_main_thread(move || {
          assert_eq!(thread::current().id(), owner);
          callbacks.fetch_add(1, Ordering::Relaxed);
        }).unwrap();
        finished.fetch_add(1, Ordering::Release);
      })
    }).collect();
    barrier.wait();
    while finished.load(Ordering::Acquire) != 8 || callbacks.load(Ordering::Relaxed) != 8 {
      runtime.run_iteration(|_| {});
    }
    for worker in workers { worker.join().unwrap(); }
    // LoopDestroyed from one iteration must not retire a reusable runtime.
    for _ in 0..3 {
      runtime.run_iteration(|_| {});
      late.run_on_main_thread(|| {}).unwrap();
    }
    drop(runtime);
    assert_retired(late);
    return;
  }
  let handle = late.clone();
  let ready = Arc::new(AtomicUsize::new(0));
  let observed = Arc::clone(&ready);
  let callback = move |event| {
    if matches!(event, RunEvent::Ready) {
      let observed = Arc::clone(&observed);
      handle.run_on_main_thread(move || { observed.fetch_add(1, Ordering::Relaxed); }).unwrap();
    }
  };
  if mode == "return" {
    late.request_exit(17).unwrap();
    assert_eq!(runtime.run_return(callback), 17);
    assert_eq!(ready.load(Ordering::Relaxed), 1);
    assert_retired(late);
  } else if mode == "run" {
    late.request_exit(23).unwrap();
    runtime.run(callback);
    panic!("consuming run returned instead of preserving Tao process-exit semantics");
  } else {
    panic!("unknown lifecycle mode");
  }
}
