import ActivityKit
import AppIntents
import Foundation
import SwiftUI
import WidgetKit

struct OpenLessRecordingLiveActivity: Widget {
  var body: some WidgetConfiguration {
    ActivityConfiguration(for: OpenLessRecordingAttributes.self) { context in
      VStack(alignment: .leading, spacing: 12) {
        HStack(spacing: 10) {
          Label("OpenLess", systemImage: "mic.fill").font(.headline)
          Spacer(minLength: 8)
          RecordingElapsedTime(context: context)
        }
        HStack(spacing: 12) {
          RecordingStateLabel(context: context).font(.subheadline)
          Spacer(minLength: 8)
          RecordingStopButton(sessionID: context.attributes.sessionID)
          RecordingSettingsLink()
        }
      }
      .padding(16)
      .foregroundStyle(.white)
      .activityBackgroundTint(.black.opacity(0.9))
      .activitySystemActionForegroundColor(.white)
    } dynamicIsland: { context in
      DynamicIsland {
        DynamicIslandExpandedRegion(.leading) {
          Label("OpenLess", systemImage: "mic.fill").font(.headline)
        }
        DynamicIslandExpandedRegion(.trailing) {
          RecordingElapsedTime(context: context)
        }
        DynamicIslandExpandedRegion(.bottom) {
          HStack(spacing: 12) {
            RecordingStateLabel(context: context).font(.caption)
            Spacer(minLength: 8)
            RecordingStopButton(sessionID: context.attributes.sessionID)
            RecordingSettingsLink()
          }
        }
      } compactLeading: {
        Image(systemName: "mic.fill")
          .foregroundStyle(context.state.recording && !context.isStale ? .red : .secondary)
          .accessibilityLabel("OpenLess")
      } compactTrailing: {
        RecordingElapsedTime(context: context)
          .font(.caption2)
          .lineLimit(1)
          .minimumScaleFactor(0.6)
          .frame(width: 56)
      } minimal: {
        Image(systemName: "mic.fill")
          .foregroundStyle(context.state.recording && !context.isStale ? .red : .secondary)
          .accessibilityLabel("OpenLess")
      }
      .keylineTint(.red)
    }
  }
}

private struct RecordingElapsedTime: View {
  let context: ActivityViewContext<OpenLessRecordingAttributes>

  var body: some View {
    if context.state.recording && !context.isStale {
      Text(context.attributes.startedAt, style: .timer)
        .monospacedDigit()
    } else {
      Text("—")
        .accessibilityLabel(context.isStale ? Text("连接已暂停") : Text("准备中…"))
    }
  }
}

private struct RecordingStateLabel: View {
  let context: ActivityViewContext<OpenLessRecordingAttributes>

  var body: some View {
    if context.isStale {
      Text("连接已暂停")
    } else if context.state.recording {
      Text("录音中")
    } else {
      Text("准备中…")
    }
  }
}

private struct RecordingStopButton: View {
  let sessionID: String
  var body: some View {
    // Keep cancellation reachable during preparation and stale presentation, too.
    Button(intent: OpenLessRecordingControlIntent(value: false, sessionID: sessionID)) {
      Label("停止", systemImage: "stop.fill")
        .frame(minWidth: 44, minHeight: 44)
    }
    .buttonStyle(.borderedProminent)
    .tint(.red)
  }
}

private struct RecordingSettingsLink: View {
  var body: some View {
    // Fixed app-owned URL. A Link explicitly brings the app to the foreground.
    if let settingsURL = URL(string: "openless-ios://settings") {
      Link(destination: settingsURL) {
        Image(systemName: "gearshape")
          .frame(minWidth: 44, minHeight: 44)
      }
      .accessibilityLabel("打开设置")
    }
  }
}
