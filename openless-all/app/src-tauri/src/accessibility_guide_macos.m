#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>

@interface OLAccessibilityDragView : NSView <NSDraggingSource>
@property(nonatomic, strong) NSURL *applicationURL;
@end

@implementation OLAccessibilityDragView
- (NSView *)hitTest:(NSPoint)point {
    return [super hitTest:point] ? self : nil;
}
- (NSDragOperation)draggingSession:(NSDraggingSession *)session sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
    return NSDragOperationCopy;
}
- (void)mouseDragged:(NSEvent *)event {
    if (!self.applicationURL) return;
    NSDraggingItem *item = [[NSDraggingItem alloc] initWithPasteboardWriter:self.applicationURL];
    NSImage *icon = [[NSWorkspace sharedWorkspace] iconForFile:self.applicationURL.path];
    [item setDraggingFrame:NSMakeRect(0, 0, 48, 48) contents:icon];
    [self beginDraggingSessionWithItems:@[item] event:event source:self];
}
@end

static NSPanel *guidePanel;
static NSTimer *guideTimer;

static NSString *OLGuideText(NSString *zh, NSString *en) {
    return [[[NSLocale preferredLanguages] firstObject] hasPrefix:@"zh"] ? zh : en;
}

static void OLCloseAccessibilityGuide(void) {
    [guideTimer invalidate];
    guideTimer = nil;
    [guidePanel close];
    guidePanel = nil;
}

void openless_show_accessibility_guide(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (AXIsProcessTrusted()) return;
        NSURL *url = [NSBundle mainBundle].bundleURL;
        if (![url.pathExtension.lowercaseString isEqualToString:@"app"]) return;
        if (guidePanel) {
            [guidePanel orderFrontRegardless];
            return;
        }
        NSScreen *screen = NSScreen.mainScreen;
        if (!screen) return;
        NSRect visible = screen.visibleFrame;
        NSRect frame = NSMakeRect(NSMidX(visible) - 210, NSMinY(visible) + 20, 420, 138);
        guidePanel = [[NSPanel alloc] initWithContentRect:frame
            styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
            backing:NSBackingStoreBuffered defer:NO];
        guidePanel.level = NSFloatingWindowLevel;
        guidePanel.floatingPanel = YES;
        guidePanel.hidesOnDeactivate = NO;
        guidePanel.releasedWhenClosed = NO;
        guidePanel.opaque = NO;
        guidePanel.backgroundColor = NSColor.clearColor;
        guidePanel.hasShadow = YES;
        guidePanel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;

        NSView *content = guidePanel.contentView;
        content.wantsLayer = YES;
        content.layer.backgroundColor = NSColor.windowBackgroundColor.CGColor;
        content.layer.cornerRadius = 14;
        content.layer.masksToBounds = YES;
        NSButton *close = [NSButton buttonWithImage:[NSImage imageWithSystemSymbolName:@"xmark" accessibilityDescription:OLGuideText(@"关闭", @"Close")]
            target:guidePanel action:@selector(close)];
        close.frame = NSMakeRect(382, 104, 24, 24);
        close.bordered = NO;
        [content addSubview:close];
        NSTextField *hint = [NSTextField wrappingLabelWithString:OLGuideText(@"将 OpenLess 拖入辅助功能列表，然后打开开关。", @"Drag OpenLess into the Accessibility list, then turn on its switch.")];
        hint.frame = NSMakeRect(18, 88, 352, 36);
        hint.font = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
        [content addSubview:hint];

        OLAccessibilityDragView *drag = [[OLAccessibilityDragView alloc] initWithFrame:NSMakeRect(16, 16, 388, 62)];
        drag.applicationURL = url;
        drag.wantsLayer = YES;
        drag.layer.backgroundColor = NSColor.controlBackgroundColor.CGColor;
        drag.layer.cornerRadius = 10;
        NSImageView *icon = [[NSImageView alloc] initWithFrame:NSMakeRect(12, 9, 44, 44)];
        icon.image = [[NSWorkspace sharedWorkspace] iconForFile:url.path];
        [drag addSubview:icon];
        NSTextField *label = [NSTextField labelWithString:@"OpenLess"];
        label.frame = NSMakeRect(68, 21, 280, 20);
        label.font = [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold];
        [drag addSubview:label];
        [content addSubview:drag];
        [guidePanel orderFrontRegardless];
        guideTimer = [NSTimer scheduledTimerWithTimeInterval:0.5 repeats:YES block:^(NSTimer *timer) {
            if (AXIsProcessTrusted() || !guidePanel.visible) OLCloseAccessibilityGuide();
        }];
    });
}
