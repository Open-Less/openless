#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>

static const NSWindowStyleMask OLGuideStyle = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable;
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

@interface OLAccessibilityGuideController : NSObject <NSWindowDelegate>
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic, strong) NSTimer *timer;
@property(nonatomic, strong) NSURL *applicationURL;
@property(nonatomic) NSTimeInterval startedAt;
@property(nonatomic) BOOL shown;
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
    // An ordinary, bounded window avoids nonactivating/all-Spaces WindowServer state.
    _window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 420, 118)
        styleMask:OLGuideStyle backing:NSBackingStoreBuffered defer:NO];
    _window.title = OLGuideText(@"辅助功能授权", @"Accessibility access");
    _window.level = NSFloatingWindowLevel;
    _window.releasedWhenClosed = NO;
    _window.excludedFromWindowsMenu = YES;
    _window.collectionBehavior = OLGuideCollectionBehavior;
    _window.sharingType = NSWindowSharingReadOnly;
    _window.delegate = self;
    _window.backgroundColor = NSColor.windowBackgroundColor;

    NSView *content = _window.contentView;
    NSTextField *hint = [NSTextField wrappingLabelWithString:OLGuideText(
        @"将下方的 OpenLess 图标拖入辅助功能列表，然后打开开关。",
        @"Drag the OpenLess icon below into the Accessibility list, then enable it.")];
    hint.frame = NSMakeRect(16, 64, 388, 40);
    hint.font = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
    hint.textColor = NSColor.labelColor;
    hint.autoresizingMask = NSViewWidthSizable | NSViewMinYMargin;
    [content addSubview:hint];

    // NSPathControl owns file dragging, its tracking loop and drag-image cleanup.
    NSPathControl *path = [[NSPathControl alloc] initWithFrame:NSMakeRect(16, 20, 250, 32)];
    path.pathStyle = NSPathStylePopUp;
    path.URL = url;
    path.editable = NO;
    path.font = [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold];
    path.backgroundColor = NSColor.controlBackgroundColor;
    path.autoresizingMask = NSViewWidthSizable | NSViewMaxYMargin;
    path.accessibilityLabel = @"OpenLess";
    [content addSubview:path];
    NSButton *finder = [NSButton buttonWithTitle:OLGuideText(@"在访达中显示", @"Show in Finder") target:self action:@selector(reveal:)];
    finder.frame = NSMakeRect(278, 20, 126, 32);
    finder.bezelStyle = NSBezelStyleRounded;
    finder.autoresizingMask = NSViewMinXMargin | NSViewMaxYMargin;
    [content addSubview:finder];
    return self;
}
- (void)reveal:(id)sender {
    (void)sender;
    [[NSWorkspace sharedWorkspace] activateFileViewerSelectingURLs:@[self.applicationURL]];
    [self close];
}
- (void)refresh {
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
