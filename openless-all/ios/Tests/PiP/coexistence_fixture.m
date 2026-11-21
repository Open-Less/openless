#import "coexistence_fixture.h"
#import <dlfcn.h>

@interface NSObject (PCENativeState)
- (NSDictionary *)diffFromPlaybackState:(id)state;
- (void)updatePlaybackStateWithDiff:(NSDictionary *)diff;
@end

@interface PCETestState : NSObject
@property(nonatomic) NSInteger contentType;
@end
@implementation PCETestState
@end

@interface PCETestProxy : NSObject
@property(nonatomic, weak) NSObject *delegate;
@property(nonatomic, strong) PCETestState *playbackState;
@property(nonatomic) NSInteger lastSentType;
@property(nonatomic) NSInteger sendCount;
@property(nonatomic) BOOL deferred;
@property(nonatomic, strong) NSMutableArray *pending;
- (void)updatePlaybackStateUsingBlock:(void (^)(id))block;
- (void)_updatePlaybackStateContentTypeIfNeeded;
@end
@implementation PCETestProxy
- (void)_updatePlaybackStateContentTypeIfNeeded {
    self.playbackState.contentType = PCEProviderType(self.delegate, self);
}
- (void)updatePlaybackStateUsingBlock:(void (^)(id))block {
    if (self.deferred) { [self.pending addObject:[block copy]]; return; }
    block(self.playbackState);
    self.lastSentType = self.playbackState.contentType;
    self.sendCount++;
}
@end

@interface PCETestAdapter : NSObject
@property(nonatomic, weak) PCETestProxy *proxy;
- (void)_updateProxyPlaybackState;
- (NSInteger)_proxyControlsStyle;
@end
@implementation PCETestAdapter
- (void)_updateProxyPlaybackState { self.proxy.playbackState.contentType = 4; }
- (NSInteger)_proxyControlsStyle { return 2; }
@end
@interface PCEExistingProviderAdapter : NSObject
- (NSInteger)pictureInPictureProxyContentType:(id)proxy;
@end
@implementation PCEExistingProviderAdapter
- (NSInteger)pictureInPictureProxyContentType:(id)proxy { return 4; }
@end

NSObject *PCEMakeProxy(NSInteger type) {
    PCETestProxy *proxy = [PCETestProxy new];
    proxy.playbackState = [PCETestState new];
    proxy.playbackState.contentType = type;
    proxy.lastSentType = -1;
    proxy.pending = [NSMutableArray new];
    return proxy;
}
NSObject *PCEMakeUnsupportedProxy(void) { return [NSObject new]; }
NSObject *PCEMakeNativeStateProxy(void) {
    dlopen("/System/Library/PrivateFrameworks/Pegasus.framework/Pegasus", RTLD_NOW);
    Class stateClass = NSClassFromString(@"PGPlaybackState");
    if (!stateClass) return nil;
    PCETestProxy *proxy = (PCETestProxy *)PCEMakeProxy(4);
    proxy.playbackState = [[stateClass alloc] init];
    proxy.playbackState.contentType = 4;
    return proxy;
}
BOOL PCENativeStateRoundTrips(NSObject *proxy) {
    id state = ((PCETestProxy *)proxy).playbackState;
    PCETestState *baseline = [state copy];
    baseline.contentType = 4;
    // Pegasus transports a Foundation dictionary diff, not an archived PGPlaybackState.
    NSDictionary *diff = [baseline diffFromPlaybackState:state];
    fprintf(stderr, "native diff=%s\n", [[diff description] UTF8String]);
    NSError *error = nil;
    NSData *data = [NSKeyedArchiver archivedDataWithRootObject:diff requiringSecureCoding:YES error:&error];
    if (!data || error) { fprintf(stderr, "native encode error: %s\n", [[error description] UTF8String]); return NO; }
    NSSet *classes = [NSSet setWithObjects:NSDictionary.class, NSString.class, NSNumber.class, nil];
    NSDictionary *decoded = [NSKeyedUnarchiver unarchivedObjectOfClasses:classes fromData:data error:&error];
    if (!decoded || error) return NO;
    [baseline updatePlaybackStateWithDiff:decoded];
    fprintf(stderr, "native diff round-trip: keys=%lu, before=%ld, after=%ld\n", (unsigned long)diff.count, (long)[state contentType], (long)baseline.contentType);
    return diff.count == 1 && baseline.contentType == [state contentType];
}
NSObject *PCEMakeAdapter(void) { return [PCETestAdapter new]; }
NSInteger PCEControlsStyle(NSObject *adapter) { return [(PCETestAdapter *)adapter _proxyControlsStyle]; }
NSObject *PCEMakeExistingProviderAdapter(void) { return [PCEExistingProviderAdapter new]; }
NSInteger PCEProviderType(NSObject *adapter, NSObject *proxy) {
    if (![adapter respondsToSelector:@selector(pictureInPictureProxyContentType:)]) return PCEType(proxy);
    return [(PCEExistingProviderAdapter *)adapter pictureInPictureProxyContentType:proxy];
}
void PCESetDelegate(NSObject *proxy, NSObject *adapter) {
    ((PCETestProxy *)proxy).delegate = adapter;
    if ([adapter isKindOfClass:PCETestAdapter.class]) ((PCETestAdapter *)adapter).proxy = (PCETestProxy *)proxy;
}
void PCERunAdapterUpdate(NSObject *adapter) { [(PCETestAdapter *)adapter _updateProxyPlaybackState]; }
void PCERunProxyRecalculate(NSObject *proxy) { [(PCETestProxy *)proxy _updatePlaybackStateContentTypeIfNeeded]; }
void PCERecalculate(NSObject *proxy, NSObject *adapter) {
    [(PCETestProxy *)proxy updatePlaybackStateUsingBlock:^(id state) {
        [state setContentType:PCEProviderType(adapter, proxy)];
    }];
}
void PCESetDeferred(NSObject *proxy, BOOL deferred) { ((PCETestProxy *)proxy).deferred = deferred; }
void PCECompleteDeferred(NSObject *proxy) {
    PCETestProxy *target = (PCETestProxy *)proxy;
    NSArray *pending = [target.pending copy];
    [target.pending removeAllObjects];
    for (void (^block)(id) in pending) {
        block(target.playbackState);
        target.lastSentType = target.playbackState.contentType;
        target.sendCount++;
    }
}
NSInteger PCESendCount(NSObject *proxy) { return ((PCETestProxy *)proxy).sendCount; }
void PCESetType(NSObject *proxy, NSInteger type) { ((PCETestProxy *)proxy).playbackState.contentType = type; }
NSInteger PCEType(NSObject *proxy) { return ((PCETestProxy *)proxy).playbackState.contentType; }
NSInteger PCELastSentType(NSObject *proxy) { return ((PCETestProxy *)proxy).lastSentType; }
