import Foundation
import OpenLessNativeBridge

public enum DocumentIdentity {
    /// Call on the UI thread when passing a UIKit proxy. Nil means detached,
    /// not a new random identity and not permission to write elsewhere.
    public static func read(from proxy: AnyObject?) -> UUID? {
        OLReadDocumentIdentifier(proxy)
    }
}

public enum CallbackBridge {
    /// Explicitly Sendable at BOTH callback boundaries: a legacy ObjC API
    /// is allowed to reply off-main even when registered from a MainActor UI.
    public static func value<T: Sendable>(
        register: @Sendable (@escaping @Sendable (T) -> Void) -> Void
    ) async -> T {
        await withCheckedContinuation { continuation in
            let completion: @Sendable (T) -> Void = { value in
                continuation.resume(returning: value)
            }
            register(completion)
        }
    }
}
