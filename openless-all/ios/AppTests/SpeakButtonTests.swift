import OpenLessShared
import Testing
@testable import OpenLess

struct SpeakButtonTests {
  @Test @MainActor func unavailableVoiceExplainsWithoutRequestingCapture() {
    let button = SpeakButton()
    var actions: [VoiceGesture.Action] = []
    var explanations = 0
    button.onAction = { actions.append($0) }
    button.onUnavailable = { explanations += 1 }
    button.configure(phase: .inactive, level: 0, available: false)
    #expect(button.accessibilityActivate())
    #expect(explanations == 1)
    #expect(actions.isEmpty)
    button.configure(phase: .processing, level: 0, available: false)
    #expect(!button.accessibilityActivate())
    #expect(explanations == 1)
    #expect(actions.isEmpty)
  }
  @Test @MainActor func startingAndRecordingRemainCancelableWhenReadinessChanges() {
    let button = SpeakButton()
    var actions: [VoiceGesture.Action] = []
    button.onAction = { actions.append($0) }
    button.configure(phase: .ready, level: 0, available: true)
    #expect(button.accessibilityActivate())
    button.configure(phase: .starting, level: 0, available: false)
    #expect(button.accessibilityActivate())
    button.configure(phase: .recording, level: 0.5, available: false)
    #expect(button.accessibilityActivate())
    #expect(actions == [.start, .cancel, .stop])
  }
}
