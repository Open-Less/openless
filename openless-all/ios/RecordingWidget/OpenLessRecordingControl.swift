import AppIntents
import Foundation
import OpenLessShared
import SwiftUI
import WidgetKit

struct OpenLessRecordingControl: ControlWidget {
  /// The app can reload this exact kind after publishing a recording transition.
  static let kind = "app.openless.recording"

  var body: some ControlWidgetConfiguration {
    StaticControlConfiguration(kind: Self.kind, provider: RecordingValueProvider()) { recording in
      ControlWidgetToggle(isOn: recording, action: OpenLessRecordingControlIntent()) {
        Label("录音", systemImage: "mic.fill")
      } valueLabel: { isOn in
        if isOn {
          Text("录音中")
        } else {
          Text("已停止")
        }
      }
      .tint(.red)
    }
    .displayName("OpenLess 录音")
    .description("开始或停止录音")
  }
}

/// Read only the shared recording status. Never expose transcript, messages or IDs.
private struct RecordingValueProvider: ControlValueProvider {
  var previewValue: Bool { false }

  func currentValue() async throws -> Bool {
    let mailbox = try SharedContainer.openMailbox()
    guard let snapshot = try mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey) else {
      return false
    }
    return snapshot.isFresh() && snapshot.phase == .recording && snapshot.captureReady
  }
}
