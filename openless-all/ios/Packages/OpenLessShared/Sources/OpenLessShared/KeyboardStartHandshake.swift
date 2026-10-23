import Foundation

/// One explicit recording gesture, independent of the keyboard view's lifetime.
/// A busy, live host is never evidence that it needs to open in the foreground.
public struct KeyboardStartHandshake: Sendable {
  public static let probeTimeout: TimeInterval = 1.2
  public enum Action: Equatable, Sendable {
    case probe, awaitResponse(TimeInterval), waitForHost, start, openHost
    case expired, targetChanged, hostChanged, unavailable, finished
  }
  public let id: UUID
  public let target: InputTarget
  public let deadline: TimeInterval
  public private(set) var hostEpoch: UUID?
  public private(set) var isFinished = false
  private var probeCount = 0
  private var probeDeadline: TimeInterval?

  public init(id: UUID, target: InputTarget, uptime: TimeInterval) {
    self.id = id
    self.target = target
    deadline = uptime + 20
  }

  public mutating func advance(snapshot: VoiceSnapshot, intent: KeyboardVoiceIntent,
    currentTarget: InputTarget?, date: Date, uptime: TimeInterval) -> Action {
    guard !isFinished else { return .finished }
    guard currentTarget == target else { return finish(.targetChanged) }
    guard uptime < deadline else { return finish(.expired) }
    guard snapshot.isFresh(at: date) else {
      if let probeDeadline, uptime < probeDeadline { return .awaitResponse(probeDeadline - uptime) }
      // Darwin notifications do not wake a suspended process. Re-sending the
      // same probe three times only delayed the necessary foreground handoff.
      guard probeCount < 1 else { return finish(.openHost) }
      probeCount += 1
      probeDeadline = uptime + Self.probeTimeout
      return .probe
    }
    if let hostEpoch, hostEpoch != snapshot.hostEpoch { return finish(.hostChanged) }
    hostEpoch = snapshot.hostEpoch
    probeCount = 0
    probeDeadline = nil

    if let capabilities = snapshot.capabilities, capabilities.isValid, !capabilities.allows(intent) {
      return finish(.unavailable)
    }
    // Processing, cancellation cleanup and configuration changes may all keep
    // canStart false while the independent background session remains usable.
    if snapshot.phase.isBusy || snapshot.backgroundRecovering == true { return .waitForHost }
    guard snapshot.inForeground || snapshot.backgroundReady else { return finish(.openHost) }
    guard snapshot.canStart, let capabilities = snapshot.capabilities, capabilities.isValid else {
      return .waitForHost
    }
    return finish(.start)
  }

  private mutating func finish(_ action: Action) -> Action {
    isFinished = true
    return action
  }
}
