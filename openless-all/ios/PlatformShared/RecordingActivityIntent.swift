import ActivityKit
import AppIntents
import Foundation
import OpenLessShared

@available(iOS 16.1, *)
nonisolated struct OpenLessRecordingAttributes: ActivityAttributes {
  nonisolated struct ContentState: Codable, Hashable { var recording: Bool }
  let sessionID: String
  let startedAt: Date
}

/// System controls/Live Activities provide the broker context. An ordinary
/// keyboard Button(intent:) was measured executing locally and is NOT used.
@available(iOS 18.0, *)
struct OpenLessRecordingControlIntent: SetValueIntent, AudioRecordingIntent, LiveActivityIntent {
  static let title: LocalizedStringResource = "OpenLess 录音开关"
  static var openAppWhenRun: Bool { false }
  @available(iOS 26.0, *) static var supportedModes: IntentModes { .background }
  @available(iOS 27.0, *) static var allowedExecutionTargets: IntentExecutionTargets { .main }
  @Parameter(title: "录音") var value: Bool
  @Parameter(title: "录音会话") var sessionID: String?
  init() { value = false; sessionID = nil }
  init(value: Bool, sessionID: String? = nil) { self.value = value; self.sessionID = sessionID }
  @MainActor func perform() async throws -> IntentResultContainer<Never, Never, Never, Never> {
    #if OPENLESS_HOST_APP
    let session = sessionID.flatMap(UUID.init(uuidString:))
    if sessionID != nil && session == nil { throw RecordingIntentError.invalidSession }
    try await AppController.shared.setSystemRecording(value, sessionID: session)
    return .result()
    #else
    // Never instantiate an audio engine, Core or credential store in an extension.
    throw RecordingIntentError.wrongProcess
    #endif
  }
}

private enum RecordingIntentError: Error { case wrongProcess, invalidSession }
