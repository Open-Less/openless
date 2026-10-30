#import <Foundation/Foundation.h>
NS_ASSUME_NONNULL_BEGIN
/// UIKit declares UITextDocumentProxy.documentIdentifier nonnull, but a
/// detached remote keyboard proxy can actually return nil. Keep the ObjC
/// boundary nullable so Swift never invokes UUID's unconditional bridge.
FOUNDATION_EXPORT NSUUID * _Nullable OLReadDocumentIdentifier(id _Nullable proxy);
NS_ASSUME_NONNULL_END
