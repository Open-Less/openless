import Foundation
import Testing
@testable import OpenLessShared

struct AppGroupResolutionTests {
  private let configured = "group.app.openless.ios"
  private let signedHost = "app.openless.ios.TESTTEAM01"

  @Test func resignedHostKeyboardAndControlUseTheSameMailbox() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let group = "group." + signedHost
    let roles: [(String, String?)] = [(signedHost, nil),
      (signedHost + ".keyboard", "com.apple.keyboard-service"),
      (signedHost + ".recording", "com.apple.widgetkit-extension")]
    let locations = try roles.map { id, point in
      try #require(AppGroupResolution.resolve(configured: configured, bundleIdentifier: id,
        extensionPoint: point, containerURL: { $0 == group ? directory : nil }))
    }
    #expect(locations.allSatisfy { $0.identifier == group && $0.directory == directory })
    let host = try Mailbox(url: locations[0].directory.appendingPathComponent("ipc/mailbox.sqlite"))
    try host.set("connected", for: "connection-probe")
    for location in locations.dropFirst() {
      let reader = try Mailbox(url: location.directory.appendingPathComponent("ipc/mailbox.sqlite"))
      #expect(try reader.value(String.self, for: "connection-probe") == "connected")
    }
    #expect(Set(locations.map { $0.identifier + ".commands.v2" }).count == 1)
    #expect(Set(locations.map { $0.identifier + ".state.v2" }).count == 1)
  }

  @Test func explicitAuthorizedGroupWinsForNormalXcodeSigning() {
    let location = AppGroupResolution.resolve(configured: "group.company.shared", bundleIdentifier: signedHost,
      extensionPoint: nil, containerURL: { URL(fileURLWithPath: "/tmp/" + $0) })
    #expect(location?.identifier == "group.company.shared")
    #expect(AppGroupResolution.candidates(configured: configured, bundleIdentifier: "app.openless.ios.keyboard",
      extensionPoint: "com.apple.keyboard-service") == [configured])
  }

  @Test func guessedNameNeverBypassesAnAbsentEntitlement() {
    let location = AppGroupResolution.resolve(configured: configured, bundleIdentifier: signedHost,
      extensionPoint: nil, containerURL: { _ in nil })
    #expect(location == nil)
  }

  @Test func unknownOrUnexpectedExtensionIDsDoNotChooseAnUnrelatedGroup() {
    for (id, point) in [(signedHost + ".other", "com.apple.keyboard-service"),
      (signedHost + ".keyboard", "unrecognized.extension")] {
      #expect(AppGroupResolution.candidates(configured: configured, bundleIdentifier: id,
        extensionPoint: point) == [configured])
    }
  }

  @Test func absentBundleIdentifierRetainsConfiguredGroup() {
    #expect(AppGroupResolution.candidates(configured: configured, bundleIdentifier: nil,
      extensionPoint: nil) == [configured])
  }
}
