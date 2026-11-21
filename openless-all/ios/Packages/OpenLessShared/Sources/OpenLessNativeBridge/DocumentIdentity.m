#import "OpenLessNativeBridge.h"

@protocol OLDocumentIdentityProvider <NSObject>
@property(nonatomic, readonly, nullable) NSUUID *documentIdentifier;
@end

NSUUID * _Nullable OLReadDocumentIdentifier(id _Nullable proxy) {
    if (![proxy respondsToSelector:@selector(documentIdentifier)]) return nil;
    id identifier = [(id<OLDocumentIdentityProvider>)proxy documentIdentifier];
    return [identifier isKindOfClass:NSUUID.class] ? identifier : nil;
}
