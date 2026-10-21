//! Native window behavior, kept out of the shared capsule painter.
use eframe::egui;

pub fn configure_early() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(target_os = "macos")]
    macos::prepare()?;
    Ok(())
}

pub fn configure_window(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    viewport
}

pub fn configure_event_loop(builder: &mut winit::event_loop::EventLoopBuilder<eframe::UserEvent>) {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder
            .with_activation_policy(ActivationPolicy::Accessory)
            .with_activate_ignoring_other_apps(false)
            .with_default_menu(false);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = builder;
}

pub fn configure_native(
    cc: &eframe::CreationContext<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::{
            Foundation::HWND,
            UI::Shell::SetWindowSubclass,
            UI::WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW,
            },
        };
        if let RawWindowHandle::Win32(handle) = cc.window_handle()?.as_raw() {
            let window = HWND(handle.hwnd.get() as *mut _);
            unsafe {
                if !SetWindowSubclass(window, Some(keep_overlay_style), 1, 0).as_bool() {
                    return Err("cannot protect native capsule window styles".into());
                }
                let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    window,
                    GWL_EXSTYLE,
                    style | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
                );
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        macos::restore_initializer();
        if let RawWindowHandle::AppKit(handle) = cc.window_handle()?.as_raw() {
            unsafe { macos::configure(handle.ns_view.as_ptr().cast())? };
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = cc;
    Ok(())
}

#[cfg(windows)]
unsafe extern "system" fn keep_overlay_style(
    window: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    id: usize,
    _: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::{
        Foundation::LRESULT,
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass},
            WindowsAndMessaging::{
                GWL_EXSTYLE, STYLESTRUCT, WM_NCDESTROY, WM_STYLECHANGING, WS_EX_APPWINDOW,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
            },
        },
    };
    if message == WM_STYLECHANGING && wparam.0 as i32 == GWL_EXSTYLE.0 {
        // Winit rewrites extended styles on show/passthrough changes; retain the overlay contract.
        let styles = &mut *(lparam.0 as *mut STYLESTRUCT);
        styles.styleNew =
            (styles.styleNew | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0;
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(keep_overlay_style), id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Imp, Sel},
        sel, ClassType,
    };
    use objc2_app_kit::{NSApplication, NSPanel, NSWindow};
    use objc2_foundation::{MainThreadMarker, NSRect};
    use std::sync::OnceLock;

    type InitWindow =
        unsafe extern "C" fn(*mut AnyObject, Sel, NSRect, usize, usize, Bool) -> *mut AnyObject;

    struct Initializers {
        window: InitWindow,
        panel: InitWindow,
        panel_class: &'static AnyClass,
    }
    static INITIALIZERS: OnceLock<Initializers> = OnceLock::new();

    extern "C" {
        fn object_setClass(object: *mut AnyObject, class: *const AnyClass) -> *const AnyClass;
    }

    unsafe extern "C" fn cannot_focus(_: &AnyObject, _: Sel) -> Bool {
        Bool::NO
    }

    unsafe extern "C" fn init_window(
        window: *mut AnyObject,
        selector: Sel,
        rect: NSRect,
        style: usize,
        backing: usize,
        deferred: Bool,
    ) -> *mut AnyObject {
        let init = INITIALIZERS.get().expect("window initializer installed");
        let original = (&*window).class();
        // Winit 0.30's allocated subclass has no ivars. Convert before either
        // NSWindow initialization or KVO registration, preserving AppKit's
        // internal observers and the delegate's effectiveAppearance observer.
        if original.name() == "WinitWindow"
            && original.instance_size() == NSWindow::class().instance_size()
            && original.instance_size() >= init.panel_class.instance_size()
        {
            object_setClass(window, init.panel_class);
            (init.panel)(window, selector, rect, style | (1 << 7), backing, deferred)
        } else {
            (init.window)(window, selector, rect, style, backing, deferred)
        }
    }

    pub(super) fn prepare() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mtm = MainThreadMarker::new().ok_or("capsule must start on the main thread")?;
        unsafe {
            let app = NSApplication::sharedApplication(mtm);
            let _: Bool = msg_send![&*app, setActivationPolicy: 1isize];
            let mut builder = ClassBuilder::new("OpenLessNativeCapsulePanel", NSPanel::class())
                .ok_or("native panel class already registered")?;
            builder.add_method(
                sel!(canBecomeKeyWindow),
                cannot_focus as unsafe extern "C" fn(_, _) -> _,
            );
            builder.add_method(
                sel!(canBecomeMainWindow),
                cannot_focus as unsafe extern "C" fn(_, _) -> _,
            );
            let selector = sel!(initWithContentRect:styleMask:backing:defer:);
            let window = NSWindow::class()
                .instance_method(selector)
                .ok_or("NSWindow initializer missing")?;
            let panel = NSPanel::class()
                .instance_method(selector)
                .ok_or("NSPanel initializer missing")?;
            INITIALIZERS
                .set(Initializers {
                    window: std::mem::transmute::<Imp, InitWindow>(window.implementation()),
                    panel: std::mem::transmute::<Imp, InitWindow>(panel.implementation()),
                    panel_class: builder.register(),
                })
                .map_err(|_| "native panel initializer already installed")?;
            // Only the disposable helper uses this temporary hook. It is
            // removed as soon as eframe supplies the initialized native view.
            window.set_implementation(std::mem::transmute::<InitWindow, Imp>(init_window));
        }
        Ok(())
    }

    pub(super) fn restore_initializer() {
        if let Some(init) = INITIALIZERS.get() {
            unsafe {
                NSWindow::class()
                    .instance_method(sel!(initWithContentRect:styleMask:backing:defer:))
                    .unwrap()
                    .set_implementation(std::mem::transmute::<InitWindow, Imp>(init.window));
            }
        }
    }

    pub(super) unsafe fn configure(
        view: *mut AnyObject,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let window: *mut AnyObject = msg_send![view, window];
        let panel: Bool = msg_send![window, isKindOfClass: NSPanel::class()];
        if !panel.as_bool() {
            return Err("winit window could not be initialized as a compatible NSPanel".into());
        }
        let _: () = msg_send![window, setLevel: 25isize];
        let _: () = msg_send![window, setCollectionBehavior: (1usize << 0) | (1usize << 8)];
        let _: () = msg_send![window, setHidesOnDeactivate: false];
        Ok(())
    }
}

pub fn place(window: &winit::window::Window, bounds: [i32; 4]) {
    window.set_outer_position(winit::dpi::PhysicalPosition::new(bounds[0], bounds[1]));
    let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
        bounds[2].max(1) as u32,
        bounds[3].max(1) as u32,
    ));
}
