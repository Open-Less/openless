import Foundation
import OpenLessShared
import Testing
@testable import OpenLess

@MainActor struct SharedContainerResolutionTests {
  @Test func lookupBeforeFullAccessDoesNotPreventRecoveryAfterGrant() throws {
    let cache = SharedContainerLocationCache()
    var access = false
    var lookups = 0
    let lookup: () -> AppGroupResolution.Location? = {
      lookups += 1
      return AppGroupResolution.resolve(configured: "group.app.openless.ios",
        bundleIdentifier: "app.openless.ios.TESTTEAM01.keyboard", extensionPoint: "com.apple.keyboard-service",
        containerURL: { id in access && id == "group.app.openless.ios.TESTTEAM01" ? URL(fileURLWithPath: "/tmp/shared-test") : nil })
    }
    #expect(cache.resolve(lookup) == nil)
    access = true
    let connected = try #require(cache.resolve(lookup))
    #expect(connected.identifier == "group.app.openless.ios.TESTTEAM01")
    #expect(cache.resolve(lookup) == connected)
    #expect(lookups == 2)
  }
}
