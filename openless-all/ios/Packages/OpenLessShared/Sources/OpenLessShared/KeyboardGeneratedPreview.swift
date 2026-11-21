import Foundation

/// Host-owned generated output, not an insertion offer. Confirmation contains only its ID.
public struct KeyboardGeneratedPreview: Codable, Equatable, Sendable {
  public let id: UUID
  public let sessionID: UUID
  public let target: InputTarget
  public let text: String
  public let createdAt: Date
  public let expiresAt: Date
  public init(id: UUID = UUID(), sessionID: UUID, target: InputTarget, text: String,
    createdAt: Date = Date(), expiresAt: Date) {
    self.id = id; self.sessionID = sessionID; self.target = target; self.text = text
    self.createdAt = createdAt; self.expiresAt = expiresAt
  }
  public func isUsable(sessionID: UUID, target: InputTarget, now: Date = Date()) -> Bool {
    self.sessionID == sessionID && self.target == target && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && text.utf8.count <= SharedContract.maxTextBytes && createdAt <= now.addingTimeInterval(2)
      && expiresAt > now && expiresAt > createdAt && expiresAt.timeIntervalSince(createdAt) <= 300
  }
  /// Validate against the session's captured capability, never mutable global settings.
  public func acceptsConfirmation(_ command: VoiceCommand, start: KeyboardVoiceStart,
    epoch: UUID, sessionActive: Bool, now: Date = Date()) -> Bool {
    sessionActive && command.kind == .confirmGeneratedPreview && command.previewID == id
      && command.sessionID == sessionID && command.target == target
      && command.rejection(now: now, epoch: epoch) == nil
      && start.intent.kind == .helpWrite && start.intent.stylePackID == nil && start.capabilityRevision != nil
      && command.intent == start.intent && command.capabilityRevision == start.capabilityRevision
      && command.sourceLanguage == start.sourceLanguage && command.language == start.language
      && isUsable(sessionID: sessionID, target: target, now: now)
  }
  /// A durable offer is accepted only after confirmation of this exact generated output.
  public func matches(_ insertion: InsertionRequest, now: Date = Date()) -> Bool {
    isUsable(sessionID: insertion.sessionID, target: insertion.target, now: now) && insertion.text == text
  }
}
