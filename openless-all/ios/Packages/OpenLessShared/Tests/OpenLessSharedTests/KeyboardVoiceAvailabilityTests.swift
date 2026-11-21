import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardVoiceAvailabilityTests {
  private let now = Date(timeIntervalSince1970: 1_800_000_000)
  private func snapshot() -> VoiceSnapshot {
    var value = VoiceSnapshot(hostEpoch: UUID())
    value.updatedAt = now
    value.inForeground = false
    value.phase = .ready
    value.capabilities = .init(revision: UUID(), sourceLanguage: "zh-CN",
      translationTargets: [], styles: [.init(id: "builtin.raw", title: "Raw")],
      canTranslate: false, canHelpWrite: false)
    return value
  }

  @Test func idleHostWithoutAudioAdmissionIsNotBusy() {
    let value = snapshot()
    #expect(value.keyboardAvailability(at: now) == .needsActivation)
  }

  @Test func languageRefreshKeepsAdmissionAndBecomesReadyInPlace() {
    var value = snapshot()
    value.backgroundReady = true
    #expect(value.keyboardAvailability(at: now) == .preparing)
    value.capabilities = nil
    #expect(value.keyboardAvailability(at: now) == .preparing)
    value = snapshot()
    value.backgroundReady = true
    value.canStart = true
    #expect(value.keyboardAvailability(at: now) == .ready)
  }

  @Test func processingRemainsBusyEvenAfterOutputAdmissionEnds() {
    var value = snapshot()
    value.phase = .processing
    #expect(value.keyboardAvailability(at: now) == .busy)
    value.updatedAt = now.addingTimeInterval(-5)
    #expect(value.keyboardAvailability(at: now) == .unresponsive)
  }
}
