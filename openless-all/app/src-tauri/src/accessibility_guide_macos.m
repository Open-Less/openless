#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <math.h>

static const NSWindowStyleMask OLGuideStyle = NSWindowStyleMaskBorderless;
static const NSWindowCollectionBehavior OLGuideCollectionBehavior = NSWindowCollectionBehaviorTransient | NSWindowCollectionBehaviorMoveToActiveSpace;

static NSString *OLGuideText(NSString *zh, NSString *en) {
    return [[[NSLocale preferredLanguages] firstObject] hasPrefix:@"zh"] ? zh : en;
}

// Quartz window bounds use the primary display's top-left origin; AppKit uses bottom-left.
static NSRect OLGuideAppKitRect(CGRect bounds, CGFloat primaryTop) {
    return NSMakeRect(bounds.origin.x, primaryTop - CGRectGetMaxY(bounds), bounds.size.width, bounds.size.height);
}

static NSRect OLGuideFrame(NSRect settings, NSRect visible, NSSize size) {
    CGFloat margin = 8;
    size.width = MIN(size.width, MAX(1, visible.size.width - 2 * margin));
    size.height = MIN(size.height, MAX(1, visible.size.height - 2 * margin));
    CGFloat x = NSMidX(settings) - size.width / 2;
    CGFloat y = NSMinY(settings) - margin - size.height;
    // Keep the helper reachable on small displays, preferring the free space above Settings.
    if (y < NSMinY(visible) + margin && NSMaxY(settings) + margin + size.height <= NSMaxY(visible) - margin) {
        y = NSMaxY(settings) + margin;
    }
    x = MAX(NSMinX(visible) + margin, MIN(x, NSMaxX(visible) - margin - size.width));
    y = MAX(NSMinY(visible) + margin, MIN(y, NSMaxY(visible) - margin - size.height));
    return NSMakeRect(x, y, size.width, size.height);
}

static BOOL OLSettingsWindowFrame(NSRect *frame) {
    NSArray<NSRunningApplication *> *apps = [NSRunningApplication runningApplicationsWithBundleIdentifier:@"com.apple.systempreferences"];
    if (apps.count == 0) return NO;
    // Required metadata (PID, layer and bounds) works without requesting screen recording.
    NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID));
    CGFloat primaryTop = NSScreen.screens.firstObject.frame.size.height;
    for (NSDictionary *window in windows) {
        pid_t pid = [window[(id)kCGWindowOwnerPID] intValue];
        BOOL settings = NO;
        for (NSRunningApplication *app in apps) if (app.processIdentifier == pid) settings = YES;
        if (!settings || [window[(id)kCGWindowLayer] integerValue] != 0) continue;
        CGRect bounds;
        if (!CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)window[(id)kCGWindowBounds], &bounds)) continue;
        if (bounds.size.width < 200 || bounds.size.height < 150) continue;
        *frame = OLGuideAppKitRect(bounds, primaryTop);
        return YES;
    }
    return NO;
}

@protocol OLGuideDragOwner <NSObject>
- (void)guideDragEnded;
@end

@interface OLGuideApplicationView : NSView <NSDraggingSource>
@property(nonatomic, strong) NSURL *applicationURL;
@property(nonatomic, strong) NSImage *applicationIcon;
@property(nonatomic, strong) NSEvent *mouseDownEvent;
@property(nonatomic, weak) id<OLGuideDragOwner> dragOwner;
@property(nonatomic) BOOL dragging;
@end

@implementation OLGuideApplicationView
- (instancetype)initWithFrame:(NSRect)frame applicationURL:(NSURL *)url {
    self = [super initWithFrame:frame];
    if (!self) return nil;
    _applicationURL = url;
    _applicationIcon = [NSWorkspace.sharedWorkspace iconForFile:url.path];
    self.accessibilityElement = YES;
    self.accessibilityRole = NSAccessibilityImageRole;
    self.accessibilityLabel = @"OpenLess";
    self.accessibilityHelp = OLGuideText(@"将此应用拖入上方的授权列表。", @"Drag this application into the access list above.");
    return self;
}
- (BOOL)acceptsFirstMouse:(NSEvent *)event { (void)event; return YES; }
- (BOOL)mouseDownCanMoveWindow { return NO; }
- (void)resetCursorRects { [self addCursorRect:self.bounds cursor:NSCursor.openHandCursor]; }
- (void)drawRect:(NSRect)dirtyRect {
    (void)dirtyRect;
    [self.applicationIcon drawInRect:NSMakeRect(0, 6, 40, 40)];
    [@"OpenLess" drawAtPoint:NSMakePoint(56, 13) withAttributes:@{
        NSFontAttributeName: [NSFont systemFontOfSize:20 weight:NSFontWeightSemibold],
        NSForegroundColorAttributeName: NSColor.labelColor
    }];
}
- (NSDraggingItem *)applicationDraggingItem {
    // NSURL writes the native file URL flavor expected by Finder and System Settings.
    NSDraggingItem *item = [[NSDraggingItem alloc] initWithPasteboardWriter:self.applicationURL];
    [item setDraggingFrame:NSMakeRect(0, 6, 40, 40) contents:self.applicationIcon];
    return item;
}
- (void)mouseDown:(NSEvent *)event { self.mouseDownEvent = event; }
- (void)mouseUp:(NSEvent *)event { (void)event; self.mouseDownEvent = nil; }
- (void)mouseDragged:(NSEvent *)event {
    if (!self.mouseDownEvent || self.dragging) return;
    NSPoint start = self.mouseDownEvent.locationInWindow;
    NSPoint current = event.locationInWindow;
    if (hypot(current.x - start.x, current.y - start.y) < 3) return;
    self.dragging = YES;
    NSEvent *mouseDown = self.mouseDownEvent;
    self.mouseDownEvent = nil;
    NSDraggingSession *session = [self beginDraggingSessionWithItems:@[[self applicationDraggingItem]] event:mouseDown source:self];
    if (!session) { self.dragging = NO; return; }
    session.animatesToStartingPositionsOnCancelOrFail = YES;
}
- (NSDragOperation)draggingSession:(NSDraggingSession *)session sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
    (void)session; (void)context;
    // The application itself must never be moved out of Applications.
    return NSDragOperationCopy;
}
- (BOOL)ignoreModifierKeysForDraggingSession:(NSDraggingSession *)session { (void)session; return YES; }
- (void)draggingSession:(NSDraggingSession *)session endedAtPoint:(NSPoint)point operation:(NSDragOperation)operation {
    (void)session; (void)point; (void)operation;
    self.dragging = NO;
    self.mouseDownEvent = nil;
    [self.dragOwner guideDragEnded];
}
@end

@interface OLAccessibilityGuideController : NSObject <NSWindowDelegate, OLGuideDragOwner>
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic, strong) NSTimer *timer;
@property(nonatomic, strong) NSURL *applicationURL;
@property(nonatomic) NSTimeInterval startedAt;
@property(nonatomic) BOOL shown;
@property(nonatomic) BOOL closeAfterDrag;
@property(nonatomic, strong) OLGuideApplicationView *applicationView;
- (void)applyApplicationAppearance;
- (void)close;
- (void)refresh;
@end

static OLAccessibilityGuideController *guideController;

@implementation OLAccessibilityGuideController
- (instancetype)initWithApplicationURL:(NSURL *)url {
    self = [super init];
    if (!self) return nil;
    _applicationURL = url;
    _startedAt = NSProcessInfo.processInfo.systemUptime;
    // Keep one bounded window without nonactivating or all-Spaces panel state.
    _window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 420, 136)
        styleMask:OLGuideStyle backing:NSBackingStoreBuffered defer:NO];
    _window.title = OLGuideText(@"OpenLess 授权引导", @"OpenLess access guide");
    _window.level = NSFloatingWindowLevel;
    _window.releasedWhenClosed = NO;
    _window.excludedFromWindowsMenu = YES;
    _window.collectionBehavior = OLGuideCollectionBehavior;
    _window.sharingType = NSWindowSharingReadOnly;
    _window.delegate = self;
    _window.opaque = NO;
    _window.backgroundColor = NSColor.clearColor;
    _window.hasShadow = YES;
    _window.movable = NO;
    NSVisualEffectView *content = [[NSVisualEffectView alloc] initWithFrame:NSMakeRect(0, 0, 420, 136)];
    content.material = NSVisualEffectMaterialPopover;
    content.blendingMode = NSVisualEffectBlendingModeBehindWindow;
    content.state = NSVisualEffectStateActive;
    content.wantsLayer = YES;
    content.layer.cornerRadius = 14;
    content.layer.masksToBounds = YES;
    NSImage *mask = [NSImage imageWithSize:NSMakeSize(29, 29) flipped:NO drawingHandler:^BOOL(NSRect rect) {
        [NSColor.blackColor setFill];
        [[NSBezierPath bezierPathWithRoundedRect:rect xRadius:14 yRadius:14] fill];
        return YES;
    }];
    mask.capInsets = NSEdgeInsetsMake(14, 14, 14, 14);
    mask.resizingMode = NSImageResizingModeStretch;
    content.maskImage = mask;
    _window.contentView = content;
    [_window invalidateShadow];
    [self applyApplicationAppearance];
    NSTextField *hint = [NSTextField wrappingLabelWithString:OLGuideText(
        @"将下方的 OpenLess 拖入上方列表，按系统提示完成授权。",
        @"Drag OpenLess into the list above, then follow the system prompt.")];
    hint.frame = NSMakeRect(20, 84, 380, 36);
    hint.font = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
    hint.textColor = NSColor.labelColor;
    [content addSubview:hint];
    self.applicationView = [[OLGuideApplicationView alloc] initWithFrame:NSMakeRect(20, 18, 380, 52) applicationURL:url];
    self.applicationView.dragOwner = self;
    [content addSubview:self.applicationView];
    return self;
}
- (void)applyApplicationAppearance {
    // Tauri maps the app's explicit Light/Dark preference to NSApp.appearance.
    // Nil retains the app's System preference; do not force the desktop appearance.
    self.window.appearance = NSApp.appearance;
}
- (void)guideDragEnded {
    // AppKit still owns the drag callback stack. Defer teardown to the next turn.
    __weak OLAccessibilityGuideController *weakSelf = self;
    dispatch_async(dispatch_get_main_queue(), ^{
        OLAccessibilityGuideController *controller = weakSelf;
        if (controller.closeAfterDrag) [controller close];
        else [controller refresh];
    });
}
- (void)refresh {
    [self applyApplicationAppearance];
    if (self.applicationView.dragging) return;
    if (AXIsProcessTrusted()) { [self close]; return; }
    NSRect settings;
    if (!OLSettingsWindowFrame(&settings)) {
        if (self.shown || NSProcessInfo.processInfo.systemUptime - self.startedAt > 10) [self close];
        return;
    }
    NSString *front = NSWorkspace.sharedWorkspace.frontmostApplication.bundleIdentifier;
    if (self.shown && ![front isEqualToString:@"com.apple.systempreferences"] && ![front isEqualToString:NSBundle.mainBundle.bundleIdentifier]) {
        [self close]; return;
    }
    NSScreen *target = nil;
    CGFloat largest = 0;
    for (NSScreen *screen in NSScreen.screens) {
        NSRect overlap = NSIntersectionRect(screen.frame, settings);
        CGFloat area = overlap.size.width * overlap.size.height;
        if (area > largest) { largest = area; target = screen; }
    }
    if (!target) { [self close]; return; }
    [self.window setFrame:OLGuideFrame(settings, target.visibleFrame, self.window.frame.size) display:YES];
    if (!self.shown) { [self.window orderFront:nil]; self.shown = YES; }
}
- (void)start {
    __weak OLAccessibilityGuideController *weakSelf = self;
    // Default mode defers teardown until AppKit has completed an active file drag.
    self.timer = [NSTimer scheduledTimerWithTimeInterval:0.25 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        [weakSelf refresh];
    }];
    [self refresh];
}
- (void)close {
    if (self.applicationView.dragging) { self.closeAfterDrag = YES; return; }
    self.applicationView.dragOwner = nil;
    self.applicationView = nil;
    [self.timer invalidate]; self.timer = nil;
    NSWindow *window = self.window;
    window.delegate = nil;
    [window orderOut:nil];
    [window close];
    self.window = nil;
    if (guideController == self) guideController = nil;
}
- (void)windowWillClose:(NSNotification *)notification {
    if (notification.object != self.window) return;
    [self.timer invalidate]; self.timer = nil;
    self.window.delegate = nil;
    self.applicationView.dragOwner = nil;
    self.applicationView = nil;
    self.window = nil;
    if (guideController == self) guideController = nil;
}
@end

void openless_show_accessibility_guide(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (AXIsProcessTrusted()) { [guideController close]; return; }
        NSURL *url = NSBundle.mainBundle.bundleURL;
        if (![url.pathExtension.lowercaseString isEqualToString:@"app"]) return;
        if (guideController) { [guideController refresh]; return; }
        guideController = [[OLAccessibilityGuideController alloc] initWithApplicationURL:url];
        [guideController start];
    });
}
