#import <UIKit/UIKit.h>

NS_ASSUME_NONNULL_BEGIN
/// On-demand only: call when the user starts voice input. No cached host identity
/// is returned; nil means the current platform does not expose this getter.
FOUNDATION_EXPORT NSString * _Nullable
OLKeyboardHostBundleIdentifier(UIInputViewController *controller);
/// Resolves only after an explicit voice-input request. Completion runs on the
/// main thread, once, within roughly one second. A new request cancels the old
/// one with nil. Modern UIKit observations are best-effort, not authenticated
/// host identities; unavailable or changing document contexts return nil.
FOUNDATION_EXPORT void OLKeyboardResolveHostBundleIdentifier(
    UIInputViewController *controller,
    void (^completion)(NSString * _Nullable bundleIdentifier));
NS_ASSUME_NONNULL_END
