import Foundation

/// A durable keyboard handoff. Only a fresh user-initiated request plus its matching URL
/// may start automatically; restoring an old mailbox entry never starts recording.
public struct HostVoiceActivation: Codable, Equatable, Identifiable, Sendable {
  public enum Phase: String, Codable, Sendable { case requested, starting, recording, processing, ready, consumed, cancelled, failed }
  public static let mailboxKey = "voice.activation.v1"
  public let id: UUID
  public let originalTarget: InputTarget?
  public let language: KeyboardLanguage
  public let intent: KeyboardVoiceIntent
  public let startOnOpen: Bool?
  public let sourceApplication: String?
  public let createdAt: Date
  public var expiresAt: Date
  public var phase: Phase = .requested
  public var text: String?
  public var message: String?
  public var insertionID: UUID?

  public init(target: InputTarget?, language: KeyboardLanguage, intent: KeyboardVoiceIntent = .init(), startOnOpen: Bool = false, sourceApplication: String? = nil, now: Date = Date()) {
    id = UUID(); originalTarget = target; self.language = language; self.intent = intent
    self.startOnOpen = startOnOpen; self.sourceApplication = sourceApplication
    createdAt = now; expiresAt = now.addingTimeInterval(120)
  }
  public func isValid(at now: Date = Date()) -> Bool {
    createdAt <= now.addingTimeInterval(2) && expiresAt > now && expiresAt > createdAt
      && expiresAt.timeIntervalSince(createdAt) <= Double(SharedContract.maxRecordingSeconds + 300)
      && (text?.utf8.count ?? 0) <= SharedContract.maxTextBytes
  }
  public func permitsAutomaticStart(from url: URL, at now: Date = Date()) -> Bool {
    guard startOnOpen == true, phase == .requested, isValid(at: now),
      url.scheme == "openless-ios", url.host == "activate",
      let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
      items.count == 1, items[0].name == "request",
      let value = items[0].value, UUID(uuidString: value) == id else { return false }
    return true
  }
  public var activationURL: URL {
    // UUID is generated locally; no transcript or app identity enters the URL.
    URL(string: "openless-ios://activate?request=" + id.uuidString.lowercased())!
  }
}
