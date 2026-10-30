import Foundation
import Testing
@testable import OpenLessShared

private final class IdentityProvider: NSObject {
    let identifier: NSUUID?
    init(_ identifier: NSUUID?) { self.identifier = identifier }
    @objc var documentIdentifier: NSUUID? { identifier }
}

struct NativeSafetyTests {
    @Test func detachedProxyDoesNotForceBridgeANilUUID() {
        #expect(DocumentIdentity.read(from: IdentityProvider(nil)) == nil)
        #expect(DocumentIdentity.read(from: nil) == nil)
        #expect(DocumentIdentity.read(from: NSObject()) == nil)
    }
    @Test func attachedProxyPreservesItsActualDocument() {
        let identifier = NSUUID()
        #expect(DocumentIdentity.read(from: IdentityProvider(identifier)) == identifier as UUID)
    }
    @Test @MainActor func legacyPermissionReplyMayArriveOffMain() async {
        let value: Bool = await CallbackBridge.value { completion in
            DispatchQueue.global().async {
                completion(!Thread.isMainThread)
            }
        }
        MainActor.preconditionIsolated()
        #expect(value)
    }
}
