#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <assert.h>
#include <math.h>
#import "../src/accessibility_guide_macos.m"

int main(int argc, const char **argv) {
    @autoreleasepool {
        NSRect visible = NSMakeRect(0, 0, 1440, 900);
        NSRect settings = NSMakeRect(100, 400, 700, 400);
        NSSize size = NSMakeSize(420, 136);
        NSRect frame = OLGuideFrame(settings, visible, size);
        assert(fabs(NSMidX(frame) - NSMidX(settings)) < .01);
        assert(fabs(NSMaxY(frame) + 8 - NSMinY(settings)) < .01);
        assert(NSContainsRect(visible, frame));
        frame = OLGuideFrame(NSMakeRect(10, 10, 700, 500), visible, size);
        assert(NSContainsRect(visible, frame));
        assert(NSMinY(frame) == 518);
        NSRect secondary = NSMakeRect(-1920, -100, 1920, 1080);
        assert(NSContainsRect(secondary, OLGuideFrame(NSMakeRect(-1800, 250, 800, 700), secondary, size)));
        NSRect converted = OLGuideAppKitRect(CGRectMake(200, 100, 700, 800), 1112);
        assert(converted.origin.y == 212);
        assert(OLGuideStyle == NSWindowStyleMaskBorderless);
        assert((OLGuideStyle & (NSWindowStyleMaskClosable | NSWindowStyleMaskTitled)) == 0);
        assert((OLGuideCollectionBehavior & NSWindowCollectionBehaviorCanJoinAllSpaces) == 0);
        assert((OLGuideCollectionBehavior & NSWindowCollectionBehaviorFullScreenAuxiliary) == 0);
        if (argc > 1) {
            [NSApplication sharedApplication];
            NSApp.activationPolicy = NSApplicationActivationPolicyAccessory;
            __weak NSWindow *closed;
            @autoreleasepool {
                OLAccessibilityGuideController *controller = [[OLAccessibilityGuideController alloc] initWithApplicationURL:[NSURL fileURLWithPath:@"/Applications/OpenLess.app"]];
                closed = controller.window;
                assert([controller.window.contentView isKindOfClass:NSVisualEffectView.class]);
                NSVisualEffectView *material = (NSVisualEffectView *)controller.window.contentView;
                assert(material.maskImage != nil);
                assert(material.maskImage.capInsets.left == 14 && material.maskImage.capInsets.bottom == 14);
                NSBitmapImageRep *maskBitmap = [[NSBitmapImageRep alloc] initWithData:material.maskImage.TIFFRepresentation];
                assert([maskBitmap colorAtX:0 y:0].alphaComponent < .1);
                assert([maskBitmap colorAtX:14 y:14].alphaComponent > .9);
                assert(controller.window.frame.size.width == 420 && controller.window.frame.size.height == 136);
                assert([controller.window standardWindowButton:NSWindowCloseButton] == nil);
                assert(controller.window.contentView.subviews.count == 2);
                OLGuideApplicationView *applicationView = controller.applicationView;
                assert([applicationView acceptsFirstMouse:nil]);
                assert(![applicationView mouseDownCanMoveWindow]);
                id session = [NSObject new];
                assert([applicationView draggingSession:session sourceOperationMaskForDraggingContext:NSDraggingContextOutsideApplication] == NSDragOperationCopy);
                NSPasteboard *pasteboard = [NSPasteboard pasteboardWithUniqueName];
                assert([pasteboard writeObjects:@[[applicationView applicationDraggingItem].item]]);
                NSArray<NSURL *> *urls = [pasteboard readObjectsForClasses:@[NSURL.class] options:@{NSPasteboardURLReadingFileURLsOnlyKey:@YES}];
                assert(urls.count == 1 && [urls.firstObject isEqual:controller.applicationURL]);
                [pasteboard releaseGlobally];
                for (NSAppearanceName name in @[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) {
                    NSApp.appearance = [NSAppearance appearanceNamed:name];
                    [controller applyApplicationAppearance];
                    assert([controller.window.appearance.name isEqualToString:name]);
                    [controller.window.contentView layoutSubtreeIfNeeded];
                    [controller.window.effectiveAppearance performAsCurrentDrawingAppearance:^{
                        NSTextField *label = (NSTextField *)controller.window.contentView.subviews.firstObject;
                        NSColor *text = [label.textColor colorUsingColorSpace:NSColorSpace.sRGBColorSpace];
                        NSColor *background = [NSColor.windowBackgroundColor colorUsingColorSpace:NSColorSpace.sRGBColorSpace];
                        assert(fabs(text.redComponent - background.redComponent) > .5);
                        NSView *view = controller.window.contentView;
                        NSBitmapImageRep *bitmap = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
                        [view cacheDisplayInRect:view.bounds toBitmapImageRep:bitmap];
                        NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
                        NSString *file = [[NSString stringWithUTF8String:argv[1]] stringByAppendingPathComponent:[name stringByAppendingString:@".png"]];
                        assert([png writeToFile:file atomically:YES]);
                    }];
                }
                NSApp.appearance = nil;
                [controller applyApplicationAppearance];
                assert(controller.window.appearance == nil);
                NSTimer *timer = [NSTimer scheduledTimerWithTimeInterval:1 repeats:YES block:^(NSTimer *value) { (void)value; }];
                controller.timer = timer;
                applicationView.dragging = YES;
                [controller close];
                assert(controller.window != nil && controller.closeAfterDrag && timer.valid);
                [applicationView draggingSession:session endedAtPoint:NSZeroPoint operation:NSDragOperationNone];
                [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
                assert(!timer.valid && controller.window == nil && controller.timer == nil);
            }
            assert(closed == nil);
        }
        puts(argc > 1 ? "Accessibility guide: geometry, app appearance, file URL dragging and window/timer cleanup passed" : "Accessibility guide: anchoring, display bounds and window flags passed");
    }
    return 0;
}
