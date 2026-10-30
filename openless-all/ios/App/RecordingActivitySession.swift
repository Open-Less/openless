import ActivityKit
import Foundation

@MainActor protocol RecordingActivitySession: AnyObject {
  func didCaptureAudio()
  func finish()
}

@available(iOS 18.0, *)
@MainActor final class SystemRecordingActivitySession: RecordingActivitySession {
  private var activityID: String?
  private var updates: Task<Void, Never>?
  private var lastFrameUpdate = Date.distantPast

  @concurrent nonisolated static func finishOrphans() async {
    for activity in Activity<OpenLessRecordingAttributes>.activities {
      await activity.end(nil, dismissalPolicy: .immediate)
    }
  }

  init(sessionID: UUID, onDismiss: @escaping @MainActor () -> Void) throws {
    guard ActivityAuthorizationInfo().areActivitiesEnabled else { throw SystemRecordingError.activityDisabled }
    let activity = try Activity<OpenLessRecordingAttributes>.request(
      attributes: .init(sessionID: sessionID.uuidString, startedAt: Date()),
      content: ActivityContent(state: .init(recording: false), staleDate: Date().addingTimeInterval(15)),
      pushType: nil)
    let id = activity.id
    activityID = id
    let states = activity.activityStateUpdates
    updates = Task { [weak self] in
      for await state in states {
        guard !Task.isCancelled, self?.activityID == id else { return }
        if state == .dismissed || state == .ended || state == .stale { onDismiss(); return }
      }
    }
  }
  func didCaptureAudio() {
    guard Date().timeIntervalSince(lastFrameUpdate) >= 10, let id = activityID else { return }
    lastFrameUpdate = Date()
    Task { [weak self] in
      guard self?.activityID == id else { return }
      await Self.updateActivity(id: id)
    }
  }
  func finish() {
    guard let id = activityID else { return }
    activityID = nil
    updates?.cancel(); updates = nil
    Task { await Self.endActivity(id: id) }
  }

  // ActivityKit's handles are not Sendable. Resolve each handle on the executor
  // that invokes its async API; only immutable IDs cross the MainActor boundary.
  @concurrent nonisolated private static func updateActivity(id: String) async {
    guard let activity = Activity<OpenLessRecordingAttributes>.activities.first(where: { $0.id == id }) else { return }
    await activity.update(ActivityContent(state: .init(recording: true), staleDate: Date().addingTimeInterval(30)))
  }
  @concurrent nonisolated private static func endActivity(id: String) async {
    guard let activity = Activity<OpenLessRecordingAttributes>.activities.first(where: { $0.id == id }) else { return }
    await activity.end(nil, dismissalPolicy: .immediate)
  }
}

enum SystemRecordingError: Error, LocalizedError {
  case permissionRequired, activityDisabled, unavailable, startFailed
  var errorDescription: String? {
    switch self {
    case .permissionRequired: "请先打开 OpenLess 授权并准备语音。"
    case .activityDisabled: "请允许 OpenLess 实时活动后使用系统录音控制。"
    case .unavailable: "OpenLess 语音服务尚未就绪。"
    case .startFailed: "未能开始录音，请打开 OpenLess 检查。"
    }
  }
}
