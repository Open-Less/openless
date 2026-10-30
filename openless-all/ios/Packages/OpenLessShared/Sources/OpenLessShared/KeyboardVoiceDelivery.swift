import Foundation

public enum KeyboardVoiceDelivery: Equatable, Sendable {
  case reject, preview, insert

  /// Session/epoch/document/revision and durable claim checks remain the caller's responsibility.
  public static func disposition(start: KeyboardVoiceStart, echoedIntent: KeyboardVoiceIntent?,
    confirmedHelpWrite: Bool = false) -> Self {
    if start.intent.kind == .helpWrite && start.intent.stylePackID != nil { return .reject }
    let requiresEcho = start.capabilityRevision != nil || start.intent.kind != .dictation
      || start.intent.stylePackID != nil
    if requiresEcho && echoedIntent != start.intent { return .reject }
    if start.intent.kind == .helpWrite && !confirmedHelpWrite { return .preview }
    return .insert
  }
}
