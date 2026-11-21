import Foundation

/// A re-signer can change the containing app ID and grant group.<new app ID>
/// without rewriting custom Info.plist keys. Only the OS may authorize a candidate.
public enum AppGroupResolution {
  public static func candidates(configured: String, bundleIdentifier: String?,
                                extensionPoint: String?) -> [String] {
    guard let bundleIdentifier, !bundleIdentifier.isEmpty else { return [configured] }
    let hostIdentifier: String
    switch extensionPoint {
    case nil:
      hostIdentifier = bundleIdentifier
    case "com.apple.keyboard-service":
      guard bundleIdentifier.hasSuffix(".keyboard") else { return [configured] }
      hostIdentifier = String(bundleIdentifier.dropLast(".keyboard".count))
    case "com.apple.widgetkit-extension":
      guard bundleIdentifier.hasSuffix(".recording") else { return [configured] }
      hostIdentifier = String(bundleIdentifier.dropLast(".recording".count))
    default:
      return [configured]
    }
    guard !hostIdentifier.isEmpty else { return [configured] }
    let resigned = "group." + hostIdentifier
    return resigned == configured ? [configured] : [configured, resigned]
  }

  public static func resolve(configured: String, bundleIdentifier: String?, extensionPoint: String?,
                             containerURL: (String) -> URL?) -> Location? {
    for identifier in candidates(configured: configured, bundleIdentifier: bundleIdentifier,
                                 extensionPoint: extensionPoint) {
      if let directory = containerURL(identifier) { return Location(identifier: identifier, directory: directory) }
    }
    return nil
  }

  public struct Location: Equatable, Sendable {
    public let identifier: String
    public let directory: URL
  }
}
