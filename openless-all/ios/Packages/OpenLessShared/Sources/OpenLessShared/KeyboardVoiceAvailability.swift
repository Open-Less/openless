import Foundation

/// A fresh host is not necessarily busy: it can be waiting for activation or
/// refreshing its recognizer/configuration. UI and start admission must not
/// treat all three situations as an active recording.
public enum KeyboardVoiceAvailability: Equatable, Sendable {
  case unresponsive, needsActivation, preparing, busy, ready
}

extension VoiceSnapshot {
  public func keyboardAvailability(at date: Date = Date()) -> KeyboardVoiceAvailability {
    guard isFresh(at: date) else { return .unresponsive }
    if phase.isBusy { return .busy }
    if backgroundRecovering == true { return .preparing }
    guard inForeground || backgroundReady else { return .needsActivation }
    guard canStart, capabilities?.isValid == true else { return .preparing }
    return .ready
  }
}
