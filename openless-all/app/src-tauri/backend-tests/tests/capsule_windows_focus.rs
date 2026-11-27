//! Windows runtime gate: real STA windows, real UIA and production MTA owner.
//! Requires an interactive Windows test desktop; missing UIA/focus is a failure.
#![cfg(target_os = "windows")]
#![allow(dead_code)]
#[path = "../../src/capsule_style_menu.rs"]
mod capsule_style_menu;
#[path = "../../src/coordinator/style_menu/windows_focus.rs"]
mod windows_focus;

use capsule_style_menu as menu;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::w,
    Win32::{
        Foundation::HWND,
        System::Com::{
            CoGetApartmentType, CoInitializeEx, CoUninitialize, APTTYPE, APTTYPEQUALIFIER,
            APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_STA, COINIT_APARTMENTTHREADED,
        },
        UI::{
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::*,
        },
    },
};

fn pump() {
    unsafe {
        let mut message = MSG::default();
        for _ in 0..64 {
            if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        pump();
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("real Windows UIA/focus runtime did not become ready on this desktop");
}
fn receive<T>(receiver: &mpsc::Receiver<T>) -> T {
    let mut value = None;
    until(|| {
        value = receiver.try_recv().ok();
        value.is_some()
    });
    value.unwrap()
}
fn apartment() -> APTTYPE {
    let (mut kind, mut qualifier) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
    unsafe {
        CoGetApartmentType(&mut kind, &mut qualifier).unwrap();
    }
    kind
}
struct Desktop {
    apartment: APTTYPE,
    window: HWND,
    first: HWND,
    second: HWND,
}
impl Desktop {
    unsafe fn new() -> Self {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        let initial_apartment = apartment();
        // COM distinguishes the process's main STA from other STAs. Both
        // must stay exactly as initialized throughout the UI-side checks.
        assert!(
            initial_apartment == APTTYPE_STA || initial_apartment == APTTYPE_MAINSTA,
            "UI test thread must initialize as STA or MAINSTA, got {initial_apartment:?}"
        );
        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("capsule focus runtime"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            420,
            220,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let edit = |y| {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                10,
                y,
                200,
                30,
                window,
                None,
                None,
                None,
            )
            .unwrap()
        };
        let (first, second) = (edit(10), edit(60));
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(first);
        Self {
            apartment: initial_apartment,
            window,
            first,
            second,
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.window);
            CoUninitialize();
        }
    }
}
fn target(scope: windows_focus::Scope) -> openless_core::DictationStyleTarget {
    openless_core::DictationStyleTarget {
        session_id: scope.session,
        selection: openless_core::DictationStyleSelection::SkipAuto,
        skip_auto_style: true,
        mutable: true,
        style_pack_id: None,
        style_pack_name: None,
        polish_style_pack_id: None,
        available_styles: vec![],
    }
}
fn issue(owner: &windows_focus::Owner, scope: windows_focus::Scope) -> menu::Request {
    let (send, receive_result) = mpsc::channel();
    assert!(owner.submit(scope, move || {
        assert_eq!(
            apartment(),
            APTTYPE_MTA,
            "UIA and request capture share the dedicated MTA"
        );
        send.send(menu::hotkey_toggled(&target(scope), scope.epoch))
            .unwrap();
    }));
    receive(&receive_result).expect("verified unchanged target must produce a request")
}

#[test]
fn real_sta_getter_reserve_delivery_and_late_mta_completion_preserve_focus_ownership() {
    let desktop = unsafe { Desktop::new() };
    assert_eq!(
        unsafe { GetForegroundWindow() },
        desktop.window,
        "test requires the controlled foreground desktop"
    );
    assert_eq!(unsafe { GetFocus() }, desktop.first);
    assert_eq!(apartment(), desktop.apartment);
    let owner = windows_focus::Owner::start().unwrap();
    let scope = windows_focus::Scope {
        session: openless_core::SessionId::new(),
        epoch: 1,
    };
    let live = Arc::new(parking_lot::Mutex::new(scope));
    let source_scope = live.clone();
    let source_owner = owner.clone();
    let epoch = Arc::new(AtomicU64::new(scope.epoch));
    let source_epoch = epoch.clone();
    let revision = owner.clone();
    menu::configure_focus_fenced(
        Arc::new(move || {
            let scope = *source_scope.lock();
            let (serial, process, verified) = source_owner.current(scope)?;
            Some(menu::Context {
                session_id: scope.session,
                style_epoch: scope.epoch,
                mutable: true,
                skip_auto: true,
                picker_enabled: true,
                keyboard_available: verified,
                focus: Some(menu::Focus(serial, process)),
                blocked_keys: [false; 4],
            })
        }),
        Arc::new(|_| true),
        Arc::new(move || source_epoch.load(Ordering::Acquire)),
        Arc::new(move || revision.revision()),
    );
    owner.set_observer(Arc::new(menu::refresh));
    until(|| {
        owner
            .current(scope)
            .is_some_and(|(_, _, verified)| verified)
    });
    let request = issue(&owner, scope);
    // These are the production calls made by STA request delivery, getter,
    // reservation, geometry completion, watch and queued key delivery.
    assert_eq!(menu::pending("capsule"), Some(request.clone()));
    let reserved = menu::reservation("capsule", scope.session, Some(&request.request_id)).unwrap();
    assert!(
        reserved.keyboard_available,
        "real UIA proof must survive the STA handoff"
    );
    menu::commit(reserved.clone(), Some(&request.request_id)).unwrap();
    menu::refresh();
    assert!(menu::owner_current(&reserved));
    assert!(menu::key(menu::Key::Enter, true, false, false));
    assert!(!menu::key(menu::Key::Escape, true, false, true));
    let key = menu::KeyEvent {
        session_id: scope.session,
        menu_id: reserved.menu_id.clone(),
        window_label: "capsule".into(),
        key: menu::Key::Enter,
        repeat: false,
    };
    assert!(menu::is_owner(&key));
    assert_eq!(
        apartment(),
        desktop.apartment,
        "STA must never be reinitialized to MTA"
    );

    unsafe {
        let _ = SetFocus(desktop.second);
    }
    assert!(
        !menu::owner_current(&reserved),
        "HWND focus change rejects immediately, without waiting for UIA"
    );
    assert!(!menu::is_owner(&key));
    assert!(menu::key(menu::Key::Enter, false, false, false));
    assert!(!menu::key(menu::Key::Escape, false, false, false));
    assert!(!menu::key(menu::Key::ArrowDown, true, false, false));
    assert!(!menu::key(menu::Key::ArrowDown, false, false, false));
    assert!(menu::reservation("capsule", scope.session, Some(&request.request_id)).is_err());

    // Hold a real owner job, then replace the Core receipt while it is late.
    // The old COM completion must not authorize that new session/epoch.
    let (entered, began) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    until(|| {
        owner
            .current(scope)
            .is_some_and(|(_, _, verified)| verified)
    });
    assert!(owner.submit(scope, move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    receive(&began);
    let obsolete_ran = Arc::new(AtomicBool::new(false));
    let obsolete_job = obsolete_ran.clone();
    assert!(owner.submit(scope, move || {
        obsolete_job.store(true, Ordering::Release);
        let _ = menu::hotkey_toggled(&target(scope), scope.epoch);
    }));
    unsafe {
        let _ = SetFocus(desktop.first);
    }
    let next = windows_focus::Scope {
        session: openless_core::SessionId::new(),
        epoch: 2,
    };
    *live.lock() = next;
    epoch.store(next.epoch, Ordering::Release);
    assert!(owner.current(next).is_none());
    menu::refresh();
    release.send(()).unwrap();
    until(|| owner.current(next).is_some_and(|(_, _, verified)| verified));
    assert!(
        !obsolete_ran.load(Ordering::Acquire),
        "queued work cannot retarget to another native control"
    );
    assert!(
        menu::pending("capsule").is_none(),
        "fresh proof alone never resurrects a request"
    );
    assert!(!menu::owner_current(&reserved));
    let next_request = issue(&owner, next);
    let next_owner =
        menu::reservation("capsule", next.session, Some(&next_request.request_id)).unwrap();
    menu::commit(next_owner.clone(), Some(&next_request.request_id)).unwrap();
    assert!(!menu::close("capsule", scope.session, &reserved.menu_id));
    assert!(menu::owner_current(&next_owner));
    assert!(menu::close("capsule", next.session, &next_owner.menu_id));
    assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
    assert_eq!(apartment(), desktop.apartment);

    // A blocked/late owner must not grant an unlimited cached keyboard lease.
    let fresh = windows_focus::Scope {
        session: next.session,
        epoch: 3,
    };
    *live.lock() = fresh;
    epoch.store(fresh.epoch, Ordering::Release);
    until(|| {
        owner
            .current(fresh)
            .is_some_and(|(_, _, verified)| verified)
    });
    let fresh_request = issue(&owner, fresh);
    let fresh_owner =
        menu::reservation("capsule", fresh.session, Some(&fresh_request.request_id)).unwrap();
    menu::commit(fresh_owner.clone(), Some(&fresh_request.request_id)).unwrap();
    assert!(menu::key(menu::Key::Enter, true, false, false));
    let (entered, began) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    assert!(owner.submit(fresh, move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    receive(&began);
    std::thread::sleep(Duration::from_millis(180));
    assert!(
        owner.current(fresh).is_none(),
        "stalled UIA proof expires even with unchanged HWND"
    );
    assert!(!menu::owner_current(&fresh_owner));
    assert!(menu::key(menu::Key::Enter, false, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, true, false, false));
    assert!(!menu::key(menu::Key::ArrowUp, false, false, false));
    release.send(()).unwrap();
    until(|| {
        owner
            .current(fresh)
            .is_some_and(|(_, _, verified)| verified)
    });
    assert!(menu::pending("capsule").is_none());
    assert!(!menu::owner_current(&fresh_owner));

    // Expiry also breaks a prepared token when no UI reader observed the gap.
    let prepared_scope = windows_focus::Scope {
        session: fresh.session,
        epoch: 4,
    };
    *live.lock() = prepared_scope;
    epoch.store(prepared_scope.epoch, Ordering::Release);
    until(|| {
        owner
            .current(prepared_scope)
            .is_some_and(|(_, _, verified)| verified)
    });
    let pending = issue(&owner, prepared_scope);
    let prepared =
        menu::reservation("capsule", prepared_scope.session, Some(&pending.request_id)).unwrap();
    let (entered, began) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    assert!(owner.submit(prepared_scope, move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(3)).unwrap();
    }));
    receive(&began);
    std::thread::sleep(Duration::from_millis(180));
    release.send(()).unwrap();
    until(|| {
        owner
            .current(prepared_scope)
            .is_some_and(|(_, _, verified)| verified)
    });
    assert_eq!(
        menu::commit(prepared, Some(&pending.request_id)).unwrap_err(),
        "capsule_style_menu_reservation_stale"
    );
    menu::invalidate();
    menu::configure(Arc::new(|| None), Arc::new(|_| false));
    drop(owner);

    // Real HWND/event ownership still applies when UIA cannot be established.
    // This test-only seam disables UIA creation, not the production native
    // focus capture, event hooks, COM owner runtime or protocol.
    let fallback = windows_focus::Owner::without_uia_for_test().unwrap();
    let source = fallback.clone();
    let fallback_scope = windows_focus::Scope {
        session: openless_core::SessionId::new(),
        epoch: 5,
    };
    let revision = fallback.clone();
    menu::configure_focus_fenced(
        Arc::new(move || {
            let (serial, process, verified) = source.current(fallback_scope)?;
            Some(menu::Context {
                session_id: fallback_scope.session,
                style_epoch: fallback_scope.epoch,
                mutable: true,
                skip_auto: true,
                picker_enabled: true,
                keyboard_available: verified,
                focus: Some(menu::Focus(serial, process)),
                blocked_keys: [false; 4],
            })
        }),
        Arc::new(|_| true),
        Arc::new(move || fallback_scope.epoch),
        Arc::new(move || revision.revision()),
    );
    fallback.set_observer(Arc::new(menu::refresh));
    until(|| {
        fallback
            .current(fallback_scope)
            .is_some_and(|(_, _, verified)| !verified)
    });
    let request = issue(&fallback, fallback_scope);
    let mouse =
        menu::reservation("capsule", fallback_scope.session, Some(&request.request_id)).unwrap();
    assert!(!mouse.keyboard_available);
    menu::commit(mouse.clone(), Some(&request.request_id)).unwrap();
    assert!(!menu::key(menu::Key::Enter, true, false, false));
    unsafe {
        let _ = SetFocus(desktop.second);
    }
    assert!(!menu::owner_current(&mouse));
    assert!(!menu::key(menu::Key::Enter, false, false, false));
    assert_eq!(apartment(), desktop.apartment);
    menu::invalidate();
    menu::configure(Arc::new(|| None), Arc::new(|_| false));
}
