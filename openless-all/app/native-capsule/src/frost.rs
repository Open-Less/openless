//! Platform glass is bounded to the animated pill; the rest of the host stays transparent.
use eframe::egui;

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq)]
struct SurfaceState {
    surface: Option<(egui::Rect, f32)>,
    dark: bool,
    pixels_per_point: f32,
    scale: f64,
}

pub struct Backdrop {
    #[cfg(target_os = "macos")]
    effect: Option<objc2::rc::Retained<objc2_app_kit::NSVisualEffectView>>,
    #[cfg(target_os = "macos")]
    last: std::cell::Cell<Option<SurfaceState>>,
}

impl Backdrop {
    pub fn new(window: &winit::window::Window) -> Self {
        #[cfg(target_os = "macos")]
        return Self {
            effect: unsafe { macos::create(window) },
            last: std::cell::Cell::new(None),
        };
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            Self {}
        }
    }

    pub fn update(
        &self,
        window: &winit::window::Window,
        surface: Option<(egui::Rect, f32)>,
        dark: bool,
        pixels_per_point: f32,
    ) {
        #[cfg(target_os = "macos")]
        if let Some(effect) = &self.effect {
            let state = SurfaceState {
                surface,
                dark,
                pixels_per_point,
                scale: window.scale_factor(),
            };
            if self.last.replace(Some(state)) == Some(state) {
                return;
            }
            unsafe { macos::update(effect, window, surface, dark, pixels_per_point) };
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (window, surface, dark, pixels_per_point);
    }
}

#[cfg(target_os = "macos")]
impl Drop for Backdrop {
    fn drop(&mut self) {
        if let Some(effect) = &self.effect {
            unsafe { effect.removeFromSuperview() };
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
        sel, ClassType,
    };
    use objc2_app_kit::{
        NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::sync::OnceLock;

    unsafe extern "C" fn ignore_pointer(_: &AnyObject, _: Sel, _: NSPoint) -> *mut AnyObject {
        std::ptr::null_mut()
    }

    unsafe fn host(window: &winit::window::Window) -> Option<&NSView> {
        match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::AppKit(handle) => Some(&*handle.ns_view.as_ptr().cast::<NSView>()),
            _ => None,
        }
    }

    pub(super) unsafe fn create(
        window: &winit::window::Window,
    ) -> Option<Retained<NSVisualEffectView>> {
        let view = host(window)?;
        let parent = view.superview()?;
        static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
        let class = CLASS.get_or_init(|| {
            let mut builder =
                ClassBuilder::new("OpenLessCapsuleFrost", NSVisualEffectView::class())
                    .expect("capsule frost class is registered once");
            builder.add_method(
                sel!(hitTest:),
                ignore_pointer as unsafe extern "C" fn(_, _, _) -> _,
            );
            builder.register()
        });
        let effect: *mut AnyObject = msg_send![*class, alloc];
        let effect: *mut NSVisualEffectView = msg_send![effect, initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))];
        let effect = Retained::from_raw(effect)?;
        effect.setMaterial(NSVisualEffectMaterial::HUDWindow);
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect.setState(NSVisualEffectState::Active);
        effect.setWantsLayer(true);
        effect.setHidden(true);
        let layer: *mut AnyObject = msg_send![&*effect, layer];
        let _: () = msg_send![layer, setCornerRadius: 16.0f64];
        let _: () = msg_send![layer, setMasksToBounds: true];
        // A child of the OpenGL view would cover its waveform and controls.
        parent.addSubview_positioned_relativeTo(
            &effect,
            NSWindowOrderingMode::NSWindowBelow,
            Some(view),
        );
        Some(effect)
    }

    pub(super) unsafe fn update(
        effect: &NSVisualEffectView,
        window: &winit::window::Window,
        surface: Option<(egui::Rect, f32)>,
        dark: bool,
        pixels_per_point: f32,
    ) {
        let Some((rect, opacity)) = surface.filter(|(_, opacity)| *opacity > 0.001) else {
            effect.setHidden(true);
            return;
        };
        let Some(view) = host(window) else {
            return;
        };
        let scale = pixels_per_point as f64 / window.scale_factor();
        let bounds = view.bounds();
        let y = if view.isFlipped() {
            rect.top() as f64 * scale
        } else {
            bounds.size.height - rect.bottom() as f64 * scale
        };
        let rect = NSRect::new(
            NSPoint::new(rect.left() as f64 * scale, y),
            NSSize::new(rect.width() as f64 * scale, rect.height() as f64 * scale),
        );
        effect.setFrame(view.convertRect_toView(rect, effect.superview().as_deref()));
        let name = NSString::from_str(if dark {
            "NSAppearanceNameDarkAqua"
        } else {
            "NSAppearanceNameAqua"
        });
        let appearance: *mut AnyObject =
            msg_send![AnyClass::get("NSAppearance").unwrap(), appearanceNamed: &*name];
        let _: () = msg_send![effect, setAppearance: appearance];
        let layer: *mut AnyObject = msg_send![effect, layer];
        let _: () = msg_send![layer, setCornerRadius: 16.0 * scale];
        effect.setAlphaValue(opacity as f64);
        effect.setHidden(false);
    }
}
