#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

/// A one-use return destination captured for the current keyboard activation.
/// Keep it in memory only; never reuse it for another activation request.
@interface OLHostApplicationReturnTarget : NSObject
@property(nonatomic, readonly, copy) NSString *bundleIdentifier;
- (instancetype)init NS_UNAVAILABLE;
+ (instancetype)new NS_UNAVAILABLE;
@end

/// Call on the main thread while handling the keyboard's activation URL.
/// With no expected ID, only an unambiguous, real system source action is used.
FOUNDATION_EXPORT OLHostApplicationReturnTarget * _Nullable
OLHostApplicationCaptureReturnTarget(NSString * _Nullable expectedBundleID);

/// Call on the main thread after capture has actually started. Returns whether
/// the system accepted navigation, not whether the host restored input focus.
FOUNDATION_EXPORT BOOL OLHostApplicationReturn(OLHostApplicationReturnTarget *target);

NS_ASSUME_NONNULL_END
