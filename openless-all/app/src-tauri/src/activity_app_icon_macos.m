#import <AppKit/AppKit.h>
#include <stdint.h>

typedef void (*OpenLessIconReceiver)(void *, const uint8_t *, size_t);

void openless_macos_activity_app_icon(const char *name, const char *bundleId,
                                      void *output, OpenLessIconReceiver receive) {
    @autoreleasepool {
        NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
        NSString *bundle = bundleId ? [NSString stringWithUTF8String:bundleId] : nil;
        NSURL *url = bundle.length ? [workspace URLForApplicationWithBundleIdentifier:bundle] : nil;
        NSImage *icon = nil;
        if (bundle.length) {
            // Running copies can have an icon before Launch Services registers their path.
            for (NSRunningApplication *app in [NSRunningApplication runningApplicationsWithBundleIdentifier:bundle]) {
                if ((icon = app.icon)) break;
            }
        }
        if (!icon && url) icon = [workspace iconForFile:url.path];
        if (!icon && !bundle.length && name) {
            NSString *label = [NSString stringWithUTF8String:name];
            // Older rankings only have a name. Resolve it only when the running app is unambiguous.
            NSRunningApplication *match = nil;
            for (NSRunningApplication *app in workspace.runningApplications) {
                if (![app.localizedName isEqualToString:label]) continue;
                if (match && ![match.bundleURL isEqual:app.bundleURL]) return;
                match = app;
            }
            icon = match.icon;
        }
        if (!icon) return;

        const NSInteger size = 32;
        NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc]
            initWithBitmapDataPlanes:NULL pixelsWide:size pixelsHigh:size bitsPerSample:8
            samplesPerPixel:4 hasAlpha:YES isPlanar:NO colorSpaceName:NSDeviceRGBColorSpace
            bytesPerRow:0 bitsPerPixel:0];
        if (!bitmap) return;
        NSGraphicsContext *context = [NSGraphicsContext graphicsContextWithBitmapImageRep:bitmap];
        if (!context) return;
        [NSGraphicsContext saveGraphicsState];
        [NSGraphicsContext setCurrentContext:context];
        context.imageInterpolation = NSImageInterpolationHigh;
        NSRect rect = NSMakeRect(0, 0, size, size);
        [[NSColor clearColor] set];
        NSRectFillUsingOperation(rect, NSCompositingOperationCopy);
        [icon drawInRect:rect fromRect:NSZeroRect operation:NSCompositingOperationSourceOver fraction:1.0];
        [NSGraphicsContext restoreGraphicsState];
        NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        if (png.length) receive(output, png.bytes, png.length);
    }
}
