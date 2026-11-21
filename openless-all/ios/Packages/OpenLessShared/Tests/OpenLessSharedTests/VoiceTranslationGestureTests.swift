import Foundation
import Testing
@testable import OpenLessShared

struct VoiceTranslationGestureTests {
  @Test func ordinaryTapAndHoldRemainDictation() {
    var selection = VoiceTranslationGesture(), voice = VoiceGesture()
    selection.down(at: 0, allowed: true); voice.down(at: 0, phase: .ready)
    #expect(selection.move(at: 0.1, horizontal: 0, vertical: 0) == false)
    #expect(selection.end() == false)
    #expect(voice.up(phase: .ready) == .start)
    selection.down(at: 1, allowed: true); voice.down(at: 1, phase: .ready)
    #expect(voice.tick(at: 1.31) == .start)
    #expect(selection.move(at: 2, horizontal: 0, vertical: 0) == false)
    #expect(selection.end() == false)
    #expect(voice.up(phase: .recording) == .stop)
  }

  @Test func upwardTakeoverCancelsProvisionalCaptureExactlyOnce() {
    var selection = VoiceTranslationGesture(), voice = VoiceGesture()
    selection.down(at: 0, allowed: true); voice.down(at: 0, phase: .ready)
    #expect(voice.tick(at: 0.31) == .start)
    #expect(selection.move(at: 0.5, horizontal: 8, vertical: -24) == true)
    #expect(voice.cancel() == .cancel)
    #expect(selection.move(at: 0.6, horizontal: 0, vertical: -80) == false)
    #expect(selection.end() == true)
    #expect(selection.end() == false)
    #expect(voice.up(phase: .recording) == nil)
    #expect(voice.cancel() == nil)
  }

  @Test func earlySlideWaitsForHoldAndHorizontalOrDownwardDragsNeverSelect() {
    var selection = VoiceTranslationGesture()
    selection.down(at: 0, allowed: true)
    #expect(selection.move(at: 0.1, horizontal: 0, vertical: -40) == false)
    #expect(selection.move(at: 0.4, horizontal: 40, vertical: -24) == false)
    #expect(selection.move(at: 0.4, horizontal: 0, vertical: 40) == false)
    #expect(selection.move(at: 0.4, horizontal: 0, vertical: -23) == false)
    #expect(selection.move(at: 0.4, horizontal: 0, vertical: -40) == true)
  }

  @Test func interruptedOrInitiallyBusyPressCannotChooseALanguage() {
    var selection = VoiceTranslationGesture()
    selection.down(at: 0, allowed: false)
    #expect(selection.move(at: 1, horizontal: 0, vertical: -40) == false)
    selection.down(at: 2, allowed: true)
    selection.end()
    #expect(selection.move(at: 3, horizontal: 0, vertical: -40) == false)
  }

  @Test func selectedTargetDoesNotLeakIntoTheNextDictationRequest() throws {
    let translation = KeyboardVoiceIntent(kind: .translation, targetLanguage: "English")
    let dictation = KeyboardVoiceIntent()
    #expect(translation.targetLanguage == "English")
    let encoded = try JSONSerialization.jsonObject(with: SharedContract.encode(dictation)) as? [String: Any]
    #expect(encoded?["kind"] as? String == "dictation")
    #expect(encoded?["targetLanguage"] == nil)
    #expect(dictation.targetLanguage == nil)
  }
}
