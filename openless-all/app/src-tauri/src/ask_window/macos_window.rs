use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::{
    msg_send,
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::WebviewWindow;

use super::{COMPOSER_HEIGHT, COMPOSER_WIDTH, CONVERSATION_HEIGHT, CONVERSATION_WIDTH, WINDOW_GAP};

const COMPOSER_BOTTOM_PIXELS: f64 = 200.0;
const COMPOSER_CONTENT_INSET_POINTS: f64 = 18.0;
const MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
static SPACE_EPOCH: AtomicU64 = AtomicU64::new(0);

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSWorkspaceActiveSpaceDidChangeNotification: *const AnyObject;
}

pub(super) fn observe_spaces() {
    unsafe {
        let Some(workspace_class) = AnyClass::get("NSWorkspace") else { return };
        let workspace: *mut AnyObject = msg_send![workspace_class, sharedWorkspace];
        let center: *mut AnyObject = msg_send![workspace, notificationCenter];
        let observer = RcBlock::new(|_: *mut AnyObject| {
            SPACE_EPOCH.fetch_add(1, Ordering::AcqRel);
        });
        let _: *mut AnyObject = msg_send![
            center, addObserverForName: NSWorkspaceActiveSpaceDidChangeNotification,
            object: std::ptr::null::<AnyObject>(), queue: std::ptr::null::<AnyObject>(),
            usingBlock: &*observer
        ];
    }
}

pub(super) fn space_epoch() -> u64 {
    SPACE_EPOCH.load(Ordering::Acquire)
}

#[derive(Clone, Copy, Default)]
pub(super) struct ScreenPoint {
    point: Option<NSPoint>,
    epoch: u64,
}

impl ScreenPoint {
    pub(super) fn capture() -> Self {
        let epoch = space_epoch();
        let point = AnyClass::get("NSEvent").map(|class| unsafe { msg_send![class, mouseLocation] });
        Self { point, epoch }
    }

    pub(super) fn is_current(&self) -> bool {
        self.epoch == space_epoch()
    }
}

pub(super) fn position(window: &WebviewWindow, at: ScreenPoint, expanded: bool) -> Result<(), String> {
    let native = window.ns_window().map_err(|error| error.to_string())?.cast::<AnyObject>();
    let class = AnyClass::get("NSScreen").ok_or("NSScreen unavailable")?;
    unsafe {
        let screens: *mut AnyObject = msg_send![class, screens];
        let count: usize = msg_send![screens, count];
        let mut selected: *mut AnyObject = msg_send![class, mainScreen];
        if let Some(point) = at.point {
            for index in 0..count {
                let screen: *mut AnyObject = msg_send![screens, objectAtIndex: index];
                let frame: NSRect = msg_send![screen, frame];
                if point.x >= frame.origin.x && point.x < frame.origin.x + frame.size.width
                    && point.y >= frame.origin.y && point.y < frame.origin.y + frame.size.height
                {
                    selected = screen;
                    break;
                }
            }
        }
        if selected.is_null() {
            return Err("No screen available for Ask.".into());
        }
        let frame: NSRect = msg_send![selected, frame];
        let work: NSRect = msg_send![selected, visibleFrame];
        let scale: f64 = msg_send![selected, backingScaleFactor];
        let (nominal_width, nominal_height, bottom) = if expanded {
            (CONVERSATION_WIDTH, CONVERSATION_HEIGHT, work.origin.y + WINDOW_GAP)
        } else {
            (
                COMPOSER_WIDTH, COMPOSER_HEIGHT,
                (frame.origin.y + COMPOSER_BOTTOM_PIXELS / scale.max(1.0)
                    - COMPOSER_CONTENT_INSET_POINTS).max(work.origin.y),
            )
        };
        let fit = ((work.size.width - WINDOW_GAP * 2.0) / nominal_width)
            .min((work.origin.y + work.size.height - bottom - WINDOW_GAP) / nominal_height)
            .clamp(0.01, 1.0);
        let width = nominal_width * fit;
        let height = nominal_height * fit;
        let ask = NSRect::new(
            NSPoint::new(work.origin.x + (work.size.width - width) / 2.0, bottom),
            NSSize::new(width, height),
        );
        let _: () = msg_send![native, setFrame: ask, display: Bool::NO];
    }
    Ok(())
}

pub(super) fn focus(window: &WebviewWindow) -> Result<(), String> {
    let native = window.ns_window().map_err(|error| error.to_string())?.cast::<AnyObject>();
    let application = AnyClass::get("NSApplication").ok_or("NSApplication unavailable")?;
    unsafe {
        let app: *mut AnyObject = msg_send![application, sharedApplication];
        let _: () = msg_send![native, setCollectionBehavior: MOVE_TO_ACTIVE_SPACE | FULL_SCREEN_AUXILIARY];
        let _: () = msg_send![native, orderFrontRegardless];
        let _: () = msg_send![native, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![app, activateIgnoringOtherApps: Bool::YES];
    }
    Ok(())
}
