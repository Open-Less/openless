import Foundation
import OpenLessShared
import Testing
@testable import OpenLess

@MainActor struct KeyboardCapabilityProjectionTests {
  @Test func onlyImplementedConfiguredChoicesReachKeyboardAndSourceIsNotUILocale() throws {
    let styles = try JSONDecoder().decode(KeyboardCapabilityProjection.Library.self, from: Data("""
      {"packs":[{"id":"builtin.raw","name":"原文","enabled":true},{"id":"builtin.light","name":"简洁","enabled":true},{"id":"hidden","name":"Disabled","enabled":false}]}
      """.utf8))
    for enabled in [false, true] {
      let source = try JSONDecoder().decode(KeyboardCapabilityProjection.self, from: Data("""
        {"developerMode":\(enabled),"llm":{"canAttemptPolish":\(enabled),"canAttemptHelpWrite":\(enabled),"canAttemptTranslation":\(enabled)},"translation":{"targetLanguages":["English","日本語"],"effectiveTargetLanguages":["日本語"],"workingLanguages":["English"]}}
        """.utf8))
      let revision = UUID()
      let snapshot = try #require(source.project(styles: styles, revision: revision))
      #expect(snapshot.revision == revision)
      #expect(snapshot.sourceLanguage == SpeechRecognitionDefaults.automaticMixedLanguage)
      #expect(snapshot.translationTargets.map(\.id) == ["日本語"])
      #expect(snapshot.styles.map(\.id) == (enabled ? ["builtin.raw", "builtin.light"] : ["builtin.raw"]))
      #expect(snapshot.styleCatalog?.map(\.id) == ["builtin.raw", "builtin.light"])
      #expect(snapshot.allows(KeyboardVoiceIntent(stylePackID: "builtin.light")) == enabled)
      #expect(snapshot.canTranslate == enabled)
      #expect(snapshot.canHelpWrite == enabled)
      #expect(!snapshot.allows(KeyboardVoiceIntent(kind: .helpWrite, stylePackID: "builtin.light")))
    }
  }
}
