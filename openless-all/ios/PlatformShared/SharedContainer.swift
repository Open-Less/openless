import Darwin
import Foundation
import OpenLessShared
import OSLog

enum SharedContainer {
  private static var configuredIdentifier: String {
    Bundle.main.object(forInfoDictionaryKey: "OLAppGroupIdentifier") as? String
      ?? "group.app.openless.ios"
  }
  private static let locationCache = SharedContainerLocationCache()
  private static let logger = Logger(subsystem: "app.openless.ios", category: "shared-container")

  private static func location() -> AppGroupResolution.Location? {
    locationCache.resolve {
      let point = (Bundle.main.object(forInfoDictionaryKey: "NSExtension") as? [String: Any])?["NSExtensionPointIdentifier"] as? String
      let result = AppGroupResolution.resolve(configured: configuredIdentifier,
        bundleIdentifier: Bundle.main.bundleIdentifier, extensionPoint: point,
        containerURL: { FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: $0) })
      if let result {
        logger.info("Shared container resolved: \(result.identifier, privacy: .public)")
      }
      return result
    }
  }

  static var identifier: String { location()?.identifier ?? configuredIdentifier }
  static var commandsNotification: String { identifier + ".commands.v2" }
  static var stateNotification: String { identifier + ".state.v2" }
  static func openMailbox() throws -> Mailbox {
    guard let location = location() else {
      logger.error("No authorized shared container for \(Bundle.main.bundleIdentifier ?? "unknown", privacy: .public)")
      throw ContractError.unavailable
    }
    let store = try Mailbox(url: location.directory.appendingPathComponent("ipc/mailbox.sqlite"))
    try FileManager.default.setAttributes(
      [.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication],
      ofItemAtPath: location.directory.appendingPathComponent("ipc").path)
    return store
  }
  static func post(_ name: String) { notify_post(name) }
}

/// The notify dispatch callback and cancellation are serialized on the main
/// queue; no unmanaged Swift pointer is handed to a cross-thread C callback.
final class MailboxObserver {
  private var token: Int32 = 0
  init(name: String, action: @escaping @MainActor @Sendable () -> Void) {
    notify_register_dispatch(name, &token, .main) { _ in
      MainActor.assumeIsolated { action() }
    }
  }
  deinit { notify_cancel(token) }
}

nonisolated final class SharedContainerLocationCache: @unchecked Sendable {
  private let lock = NSLock()
  private var location: AppGroupResolution.Location?
  func resolve(_ lookup: () -> AppGroupResolution.Location?) -> AppGroupResolution.Location? {
    lock.lock()
    defer { lock.unlock() }
    if let location { return location }
    location = lookup()
    return location
  }
}
