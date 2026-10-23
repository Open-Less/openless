import Foundation

/// Only an active keyboard recording requires heartbeats. Idle keyboards do
/// not keep either process polling or the microphone alive.
public struct KeyboardLease: Sendable {
  public let sessionID: UUID
  public let target: InputTarget
  private var lastSeen: Date
  public init(sessionID: UUID, target: InputTarget, now: Date = Date()) {
    self.sessionID = sessionID
    self.target = target
    lastSeen = now
  }
  @discardableResult
  public mutating func observe(_ command: VoiceCommand, now: Date = Date()) -> Bool {
    guard command.kind == .ping, command.sessionID == sessionID, command.target == target,
      command.deadline >= now, command.createdAt <= now.addingTimeInterval(2)
    else { return false }
    lastSeen = now
    return true
  }
  public func expired(at now: Date = Date()) -> Bool {
    now.timeIntervalSince(lastSeen) > 4 || now.timeIntervalSince(lastSeen) < -2
  }
}
