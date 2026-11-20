#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>

static const NSInteger OLMainLaunchTag = 91260735;

@interface OLMainLaunchCover : NSVisualEffectView
@property(nonatomic, strong) NSView *solid;
@property(nonatomic, weak) WKWebView *readyWebView;
@property(nonatomic) BOOL solidReady;
@property(nonatomic) BOOL revealing;
@end

static void OLRevealHeatmapDetails(WKWebView *webView) {
    [webView evaluateJavaScript:@"document.documentElement.removeAttribute('data-ol-main-content-covered')" completionHandler:nil];
}

@implementation OLMainLaunchCover
- (NSInteger)tag { return OLMainLaunchTag; }

- (void)updateSolidColor {
    NSString *mode = [self.effectiveAppearance bestMatchFromAppearancesWithNames:@[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]];
    NSColor *surface = [mode isEqualToString:NSAppearanceNameDarkAqua]
        ? [NSColor colorWithSRGBRed:28.0/255.0 green:28.0/255.0 blue:31.0/255.0 alpha:1]
        : NSColor.whiteColor;
    self.solid.layer.backgroundColor = surface.CGColor;
}

- (void)viewDidChangeEffectiveAppearance {
    [super viewDidChangeEffectiveAppearance];
    [self updateSolidColor];
}

- (void)revealIfReady {
    WKWebView *webView = self.readyWebView;
    if (!self.superview || !self.solidReady || !webView || self.revealing) return;
    self.revealing = YES;
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion ? 0 : 0.14;
        self.animator.alphaValue = 0;
    } completionHandler:^{
        // A delayed animation must not affect a window closed during the handoff.
        if (!self.superview) return;
        [self removeFromSuperview];
        if (webView.window) OLRevealHeatmapDetails(webView);
    }];
}

- (void)begin {
    BOOL reduceMotion = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    BOOL reduceTransparency = NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceTransparency;
    if (reduceMotion || reduceTransparency) {
        self.solid.alphaValue = 1;
        self.solidReady = YES;
        return;
    }
    __weak OLMainLaunchCover *weakCover = self;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 65 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        OLMainLaunchCover *cover = weakCover;
        if (!cover.superview) return;
        [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
            context.duration = 0.14;
            cover.solid.animator.alphaValue = 1;
        } completionHandler:^{
            cover.solidReady = YES;
            [cover revealIfReady];
        }];
    });
}
@end

void openless_install_main_launch_cover(void *windowHandle) {
    NSWindow *window = (__bridge NSWindow *)windowHandle;
    NSView *content = window.contentView;
    if (!content || [content viewWithTag:OLMainLaunchTag]) return;
    OLMainLaunchCover *cover = [[OLMainLaunchCover alloc] initWithFrame:content.bounds];
    cover.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    cover.material = NSVisualEffectMaterialWindowBackground;
    cover.blendingMode = NSVisualEffectBlendingModeBehindWindow;
    cover.state = NSVisualEffectStateActive;
    cover.solid = [[NSView alloc] initWithFrame:cover.bounds];
    cover.solid.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    cover.solid.wantsLayer = YES;
    cover.solid.alphaValue = 0;
    [cover addSubview:cover.solid];
    [content addSubview:cover positioned:NSWindowAbove relativeTo:nil];
    [cover updateSolidColor];
    [cover begin];
}

void openless_reveal_main_webview(void *webViewHandle) {
    WKWebView *webView = (__bridge WKWebView *)webViewHandle;
    OLMainLaunchCover *cover = [webView.window.contentView viewWithTag:OLMainLaunchTag];
    if (!cover) {
        OLRevealHeatmapDetails(webView);
        return;
    }
    cover.readyWebView = webView;
    [cover revealIfReady];
}

void openless_remove_main_launch_cover(void *windowHandle) {
    NSWindow *window = (__bridge NSWindow *)windowHandle;
    [[window.contentView viewWithTag:OLMainLaunchTag] removeFromSuperview];
}
