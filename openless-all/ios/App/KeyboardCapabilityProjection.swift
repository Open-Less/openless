import Foundation
import OpenLessShared

/// Only public choices cross into the keyboard; never runtime prompts, vaults or endpoints.
struct KeyboardCapabilityProjection: Decodable {
  struct LLM: Decodable {
    let canAttemptPolish: Bool
    let canAttemptHelpWrite: Bool
    let canAttemptTranslation: Bool
  }
  struct Translation: Decodable {
    let targetLanguages: [String]
    let effectiveTargetLanguages: [String]
    let workingLanguages: [String]
  }
  struct Pack: Decodable { let id: String; let name: String; let enabled: Bool }
  struct Library: Decodable { let packs: [Pack] }
  let developerMode: Bool
  let llm: LLM
  let translation: Translation

  func project(styles: Library, revision: UUID) -> KeyboardVoiceCapabilities? {
    let canProcess = developerMode && llm.canAttemptPolish
    let targets = translation.effectiveTargetLanguages.map { KeyboardVoiceOption(id: $0, title: $0) }
    let choices = styles.packs.filter { $0.enabled && ($0.id == "builtin.raw" || canProcess) }
      .map { KeyboardVoiceOption(id: $0.id, title: $0.name) }
    var value = KeyboardVoiceCapabilities(revision: revision,
      sourceLanguage: SpeechRecognitionDefaults.automaticMixedLanguage,
      translationTargets: targets, styles: choices,
      canTranslate: developerMode && llm.canAttemptTranslation && !targets.isEmpty,
      canHelpWrite: developerMode && llm.canAttemptHelpWrite,
      styleCatalog: styles.packs.filter(\.enabled).map { KeyboardVoiceOption(id: $0.id, title: $0.name) })
    value.supportsSpaceVoice = true
    return value.isValid ? value : nil
  }
}
