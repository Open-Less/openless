//! Construct disposable Tauri cards as NSPanels before AppKit installs KVO.
//!
//! Tao 0.37.1 allocates TaoWindow (NSWindow + one `focusable` ivar). Replacing
//! its class after initialization loses AppKit's observation state and aborts
//! when the card is destroyed. Keep Tao's layout/sendEvent implementation, but
//! run NSPanel's initializer before the window, delegate or WebView is set up.
use objc2::{
    msg_send,
    runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Imp, Sel},
    sel, ClassType,
};
use objc2_app_kit::{NSPanel, NSWindow};
use objc2_foundation::NSRect;
use std::{cell::Cell, marker::PhantomData, rc::Rc, sync::OnceLock};

type InitWindow =
    unsafe extern "C" fn(*mut AnyObject, Sel, NSRect, usize, usize, Bool) -> *mut AnyObject;
type SendEvent = unsafe extern "C" fn(&AnyObject, Sel, *mut AnyObject);

struct Initializers {
    window: InitWindow,
    panel: InitWindow,
}
static INITIALIZERS: OnceLock<Initializers> = OnceLock::new();
static PANEL_CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
thread_local! {
    static CONVERT_NEXT: Cell<bool> = const { Cell::new(false) };
}

extern "C" {
    fn object_setClass(object: *mut AnyObject, class: *const AnyClass) -> *const AnyClass;
}

unsafe extern "C" fn cannot_focus(_: &AnyObject, _: Sel) -> Bool {
    Bool::NO
}

unsafe fn panel_class(tao: &'static AnyClass) -> Option<&'static AnyClass> {
    *PANEL_CLASS.get_or_init(|| {
        // Fail closed if an SDK/Tao upgrade changes the allocation contract.
        let ivars = tao.instance_variables();
        if tao.superclass() != Some(NSWindow::class())
            || NSPanel::class().instance_size() != NSWindow::class().instance_size()
            || ivars.len() != 1
            || ivars[0].name() != "focusable"
        {
            return None;
        }
        let event = tao.instance_method(sel!(sendEvent:))?;
        let mut builder = ClassBuilder::new("OpenLessDisposableCardPanel", NSPanel::class())?;
        builder.add_ivar::<Bool>("focusable");
        builder.add_method(
            sel!(canBecomeKeyWindow),
            cannot_focus as unsafe extern "C" fn(_, _) -> _,
        );
        builder.add_method(
            sel!(canBecomeMainWindow),
            cannot_focus as unsafe extern "C" fn(_, _) -> _,
        );
        builder.add_method(
            sel!(sendEvent:),
            std::mem::transmute::<Imp, SendEvent>(event.implementation())
                as unsafe extern "C" fn(_, _, _),
        );
        let panel = builder.register();
        let focusable = panel.instance_variable("focusable")?;
        if panel.instance_size() != tao.instance_size()
            || focusable.offset() != ivars[0].offset()
            || focusable.type_encoding() != ivars[0].type_encoding()
        {
            return None;
        }
        Some(panel)
    })
}

unsafe extern "C" fn init_window(
    window: *mut AnyObject,
    selector: Sel,
    rect: NSRect,
    style: usize,
    backing: usize,
    deferred: Bool,
) -> *mut AnyObject {
    let init = INITIALIZERS.get().expect("card initializer installed");
    let original = (&*window).class();
    if original.name() == "TaoWindow" && CONVERT_NEXT.replace(false) {
        if let Some(panel) = panel_class(original) {
            // This is an allocated, uninitialized object. Never change its
            // class again after NSPanel/AppKit have installed their observers.
            object_setClass(window, panel);
            return (init.panel)(window, selector, rect, style | (1 << 7), backing, deferred);
        }
    }
    (init.window)(window, selector, rect, style, backing, deferred)
}

/// Main-thread-only scoped hook. Restore it on success, build error or unwind;
/// other window classes always use the original initializer while it is active.
pub(super) struct CardInitializer(PhantomData<Rc<()>>);

impl CardInitializer {
    pub(super) fn install() -> tauri::Result<Self> {
        if unsafe { libc::pthread_main_np() } != 1 {
            return Err(error("card creation requires the main thread"));
        }
        unsafe {
            let selector = sel!(initWithContentRect:styleMask:backing:defer:);
            let window = NSWindow::class()
                .instance_method(selector)
                .ok_or_else(|| error("NSWindow initializer missing"))?;
            let panel = NSPanel::class()
                .instance_method(selector)
                .ok_or_else(|| error("NSPanel initializer missing"))?;
            let init = INITIALIZERS.get_or_init(|| Initializers {
                window: std::mem::transmute::<Imp, InitWindow>(window.implementation()),
                panel: std::mem::transmute::<Imp, InitWindow>(panel.implementation()),
            });
            if window.implementation() as usize != init.window as usize {
                return Err(error(
                    "NSWindow initializer changed or card build is reentrant",
                ));
            }
            window.set_implementation(std::mem::transmute::<InitWindow, Imp>(init_window));
            CONVERT_NEXT.set(true);
        }
        Ok(Self(PhantomData))
    }
}

impl Drop for CardInitializer {
    fn drop(&mut self) {
        CONVERT_NEXT.set(false);
        unsafe {
            NSWindow::class()
                .instance_method(sel!(initWithContentRect:styleMask:backing:defer:))
                .expect("NSWindow initializer exists")
                .set_implementation(std::mem::transmute::<InitWindow, Imp>(
                    INITIALIZERS
                        .get()
                        .expect("card initializer installed")
                        .window,
                ));
        }
    }
}

pub(super) fn configure<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    unsafe {
        let raw = window.ns_window()? as *mut AnyObject;
        let panel: Bool = msg_send![raw, isKindOfClass: NSPanel::class()];
        if !panel.as_bool() {
            return Err(error(
                "TaoWindow layout is incompatible with a disposable NSPanel",
            ));
        }
        let _: () = msg_send![raw, setLevel: 25isize];
        let _: () = msg_send![raw, setCollectionBehavior: (1usize << 0) | (1usize << 8)];
        let _: () = msg_send![raw, setHidesOnDeactivate: false];
    }
    Ok(())
}

fn error(message: &str) -> tauri::Error {
    tauri::Error::Anyhow(anyhow::anyhow!(message.to_owned()))
}
