import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardPresentationTests {
  @Test func experiencePreservesLegacyChoicesAndRestoresOnReopen() throws {
    var preferences = try JSONDecoder().decode(KeyboardPreferences.self,
      from: Data("{\"appearance\":\"typingFirst\",\"spaceHold\":\"cursor\"}".utf8))
    #expect(preferences.experience == .voiceFocused)
    #expect(!preferences.opensVoicePage)
    #expect(!preferences.usesSpaceRecording)
    preferences.experience = .singleKeyboard
    preferences.translationOutputLanguage = "English"
    let reopened = try JSONDecoder().decode(KeyboardPreferences.self, from: JSONEncoder().encode(preferences))
    #expect(reopened.experience == .singleKeyboard)
    #expect(reopened.translationOutputLanguage == "English")
    #expect(reopened.usesSpaceRecording && !reopened.opensVoicePage)
    preferences.experience = .voiceFocused
    #expect(preferences.spaceHold == .cursor)
    #expect(preferences.appearance == .typingFirst)
    #expect(preferences.translationOutputLanguage == "English")
    #expect(KeyboardSizing.bodyHeight(scale: 1, experience: .singleKeyboard) == 208)
    #expect(KeyboardSizing.bodyHeight(scale: 0.9, landscape: true, experience: .singleKeyboard) == 144)
  }
  @Test func singleOutputRequiresAvailableChoiceAndFreezesForRecording() throws {
    func capabilities(targets: [String], enabled: Bool = true) -> KeyboardVoiceCapabilities {
      .init(revision: UUID(), sourceLanguage: "auto", translationTargets: targets.map { .init(id: $0, title: $0) },
        styles: [], canTranslate: enabled, canHelpWrite: false)
    }
    var preferences = KeyboardPreferences()
    preferences.experience = .singleKeyboard
    let available = capabilities(targets: ["English", "日本語"])
    #expect(preferences.translationOutput(in: available) == nil)
    preferences.translationOutputLanguage = "English"
    let frozen = try #require(preferences.translationOutput(in: available))
    preferences.translationOutputLanguage = "日本語"
    var delivery = SpaceVoiceDeliveryState()
    #expect((delivery.choose(.init(.chooseTranslation))) == true)
    #expect((delivery.choose(.init(.translate, targetLanguage: frozen.id))) == true)
    delivery.recognized("你好")
    let work = delivery.takeWork()
    let output = try #require(work)
    #expect(output.0.targetLanguage == "English")
    #expect(delivery.takeWork() == nil)
    #expect(preferences.translationOutput(in: capabilities(targets: ["English"])) == nil)
    #expect(preferences.translationOutput(in: capabilities(targets: ["日本語"], enabled: false)) == nil)
    #expect(preferences.translationOutput(in: nil) == nil)
  }
  @Test func oldPreferencesUseReferenceSizeAndScaleRoundTrips() throws {
    let old = try JSONDecoder().decode(KeyboardPreferences.self, from: Data("{\"version\":1}".utf8))
    #expect(old.keyboardScale == 1)
    #expect(KeyboardSizing.bodyHeight(scale: old.keyboardScale) == 208)
    var saved = old
    saved.keyboardScale = 1.15
    let reopened = try JSONDecoder().decode(KeyboardPreferences.self, from: JSONEncoder().encode(saved))
    #expect(reopened.keyboardScale == saved.keyboardScale)
    #expect(KeyboardSizing.bodyHeight(scale: reopened.keyboardScale) > 208)
    #expect(KeyboardSizing.bodyHeight(scale: 1, landscape: true) == 160)
  }
  @Test func malformedHeightCannotCollapseOrExplodeKeyboard() throws {
    #expect(KeyboardSizing.validScale(.nan) == 1)
    #expect(KeyboardSizing.validScale(.infinity) == 1)
    #expect(KeyboardSizing.validScale(-4) == 0.9)
    #expect(KeyboardSizing.validScale(999) == 1.2)
    let tooLarge = try JSONDecoder().decode(KeyboardPreferences.self,
      from: Data("{\"keyboardScale\":999}".utf8))
    #expect(tooLarge.keyboardScale == 1.2)
  }
  @Test func styleSurvivesNewDefaultsReaderAndExplicitRawSelection() throws {
    let suite = "OpenLess-style-test-" + UUID().uuidString
    let first = try #require(UserDefaults(suiteName: suite))
    defer { first.removePersistentDomain(forName: suite) }
    KeyboardStyleSelection.save("style.polish", to: first)
    let reopened = try #require(UserDefaults(suiteName: suite))
    #expect(KeyboardStyleSelection.load(from: reopened) == "style.polish")
    // Reading unrelated host settings does not mutate the keyboard-only choice.
    _ = try JSONDecoder().decode(KeyboardPreferences.self, from: Data("{}".utf8))
    #expect(KeyboardStyleSelection.load(from: reopened) == "style.polish")
    KeyboardStyleSelection.save(nil, to: reopened)
    #expect(KeyboardStyleSelection.load(from: first) == nil)
    KeyboardStyleSelection.save("builtin.raw", to: reopened)
    #expect(KeyboardStyleSelection.load(from: first) == nil)
  }
  @Test func speechLevelIsVisibleWithoutInventingAudioAtSilence() {
    #expect(VoiceWaveform.amplitude(rms: 0) == 0)
    #expect(VoiceWaveform.amplitude(rms: 0.001) == 0)
    #expect(VoiceWaveform.amplitude(rms: .nan) == 0)
    #expect(VoiceWaveform.amplitude(rms: 0.01) > 0.4)
    #expect(VoiceWaveform.amplitude(rms: 0.1) > VoiceWaveform.amplitude(rms: 0.01))
    #expect(VoiceWaveform.amplitude(rms: 10) == 1)
  }
}
