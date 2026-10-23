#import <Foundation/Foundation.h>

NSObject *PCEMakeProxy(NSInteger type);
NSObject *PCEMakeUnsupportedProxy(void);
NSObject *PCEMakeNativeStateProxy(void);
BOOL PCENativeStateRoundTrips(NSObject *proxy);
NSObject *PCEMakeAdapter(void);
NSInteger PCEControlsStyle(NSObject *adapter);
NSObject *PCEMakeExistingProviderAdapter(void);
NSInteger PCEProviderType(NSObject *adapter, NSObject *proxy);
void PCESetDelegate(NSObject *proxy, NSObject *adapter);
void PCERunAdapterUpdate(NSObject *adapter);
void PCERunProxyRecalculate(NSObject *proxy);
void PCERecalculate(NSObject *proxy, NSObject *adapter);
void PCESetDeferred(NSObject *proxy, BOOL deferred);
void PCECompleteDeferred(NSObject *proxy);
NSInteger PCESendCount(NSObject *proxy);
void PCESetType(NSObject *proxy, NSInteger type);
NSInteger PCEType(NSObject *proxy);
NSInteger PCELastSentType(NSObject *proxy);
