import Foundation

/// Tracks native Core requests while the keyboard owns the final write. Leaving
/// the destination releases those requests, but retains the text and durable
/// attempt evidence in the mailbox.
public struct KeyboardInsertionDelivery: Sendable {
  public struct Resolution: Equatable, Sendable {
    public let requestID: String
    public let submitted: Bool
  }
  private struct Waiting: Sendable {
    let requestID: String
    let sessionID: UUID
    let target: InputTarget
  }
  private var waiting: [UUID: Waiting] = [:]
  private var detachedSessions: Set<UUID> = []
  public init() {}
  public var pendingCount: Int { waiting.count }

  /// A result can arrive after dismissal was already observed. Persist it for
  /// manual recovery and resolve Core immediately instead of waiting 30 seconds
  /// for an extension that no longer owns this document.
  public mutating func offer(_ insertion: InsertionRequest, requestID: String,
    mailbox: Mailbox) throws -> Resolution? {
    try mailbox.storeInsertion(insertion)
    if detachedSessions.contains(insertion.sessionID) {
      try mailbox.revokeInsertion(insertion.id)
      return Resolution(requestID: requestID, submitted: false)
    }
    waiting[insertion.id] = Waiting(requestID: requestID,
      sessionID: insertion.sessionID, target: insertion.target)
    return nil
  }

  public mutating func receive(_ command: VoiceCommand, mailbox: Mailbox) throws -> Resolution? {
    guard command.kind == .insertionReceipt, let id = command.insertionID,
      let pending = waiting[id], pending.sessionID == command.sessionID,
      pending.target == command.target, let record = try mailbox.insertion(id)
    else { return nil }
    waiting[id] = nil
    return Resolution(requestID: pending.requestID,
      submitted: record.state == .submitted && command.receipt == .submitted)
  }

  /// Returns each native reply at most once, even if the old keyboard's delayed
  /// acknowledgement arrives after another keyboard has started a new session.
  public mutating func detach(sessionID: UUID, mailbox: Mailbox) throws -> [Resolution] {
    detachedSessions.insert(sessionID)
    var resolutions: [Resolution] = []
    for (id, pending) in waiting.filter({ $0.value.sessionID == sessionID }) {
      try mailbox.revokeInsertion(id)
      let record = try mailbox.insertion(id)
      waiting[id] = nil
      resolutions.append(Resolution(requestID: pending.requestID, submitted: record?.state == .submitted))
    }
    return resolutions
  }

  /// A current idle probe means the attached keyboard owns no recording. The
  /// document can remain identical after dismissal/reopening, so target equality
  /// does not restore ownership. A keyboard that still owns a recording or its
  /// delivery sends the session ID instead. Queued probes predating the session
  /// cannot revoke its future result.
  public mutating func observeKeyboardOwnership(_ command: VoiceCommand, sessionID: UUID,
    startedAt: Date, recordingHasEnded: Bool,
    mailbox: Mailbox, now: Date = Date()) throws -> [Resolution]? {
    guard command.schemaVersion == 2, command.kind == .ping, command.sessionID == nil,
      command.createdAt >= startedAt,
      command.createdAt <= now.addingTimeInterval(2), command.deadline >= now,
      recordingHasEnded else { return nil }
    return try detach(sessionID: sessionID, mailbox: mailbox)
  }

  public mutating func finish(sessionID: UUID, mailbox: Mailbox) throws {
    _ = try detach(sessionID: sessionID, mailbox: mailbox)
    detachedSessions.remove(sessionID)
  }
}
