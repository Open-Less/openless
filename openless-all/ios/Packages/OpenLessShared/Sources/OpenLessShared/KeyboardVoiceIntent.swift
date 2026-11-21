import Foundation

/// Public request metadata only. Never contains a prompt, selection, provider key or diagnostics.
public struct KeyboardVoiceIntent: Codable, Equatable, Sendable {
  public enum Kind: String, Codable, Sendable { case dictation, translation, helpWrite }
  public let kind: Kind
  public let targetLanguage: String?
  public let stylePackID: String?
  public init(kind: Kind = .dictation, targetLanguage: String? = nil, stylePackID: String? = nil) {
    self.kind = kind
    self.targetLanguage = targetLanguage
    // Current keyboard producers mean Raw, not the App's globally selected style.
    // Synthesized decoding still preserves a legacy missing optional field.
    self.stylePackID = kind == .dictation ? (stylePackID ?? "builtin.raw") : stylePackID
  }
}

public struct KeyboardVoiceOption: Codable, Equatable, Sendable {
  public let id: String
  public let title: String
  public init(id: String, title: String) { self.id = id; self.title = title }
}

/// An admission capability, not proof that a microphone/background service is ready.
/// The host must reject starts against a changed revision, never reinterpret their intent.
public struct KeyboardVoiceCapabilities: Codable, Equatable, Sendable {
  public var version = 1
  public var supportsSpaceVoice: Bool?
  public let revision: UUID
  public let sourceLanguage: String
  public let translationTargets: [KeyboardVoiceOption]
  public let styles: [KeyboardVoiceOption]
  /// Display-only enabled packs, including ones that still need model setup.
  /// Only `styles` grants admission; legacy snapshots have no separate catalog.
  public let styleCatalog: [KeyboardVoiceOption]?
  public let canTranslate: Bool
  public let canHelpWrite: Bool
  public init(revision: UUID, sourceLanguage: String, translationTargets: [KeyboardVoiceOption],
    styles: [KeyboardVoiceOption], canTranslate: Bool, canHelpWrite: Bool,
    styleCatalog: [KeyboardVoiceOption]? = nil) {
    self.revision = revision
    self.sourceLanguage = sourceLanguage
    self.translationTargets = translationTargets
    self.styles = styles
    self.styleCatalog = styleCatalog
    self.canTranslate = canTranslate
    self.canHelpWrite = canHelpWrite
  }
  /// Transport bounds, not a product limit on configured languages. Oversize snapshots fail closed.
  public var isValid: Bool {
    version == 1 && !sourceLanguage.isEmpty && sourceLanguage.utf8.count <= 64
      && translationTargets.count <= 256 && styles.count <= 256
      && Self.valid(translationTargets) && Self.valid(styles)
      && (styleCatalog.map { $0.count <= 256 && Self.valid($0) } ?? true)
  }
  private static func valid(_ options: [KeyboardVoiceOption]) -> Bool {
    Set(options.map(\.id)).count == options.count && options.allSatisfy {
      !$0.id.isEmpty && $0.id.utf8.count <= 128 && !$0.title.isEmpty && $0.title.utf8.count <= 160
        && !$0.id.contains(where: { $0.isNewline }) && !$0.title.contains(where: { $0.isNewline })
    }
  }
  public func allows(_ intent: KeyboardVoiceIntent) -> Bool {
    guard isValid else { return false }
    if let style = intent.stylePackID, !styles.contains(where: { $0.id == style }) { return false }
    switch intent.kind {
    case .dictation: return intent.targetLanguage == nil
    case .translation:
      return canTranslate && translationTargets.contains { $0.id == intent.targetLanguage }
    case .helpWrite:
      return canHelpWrite && intent.targetLanguage == nil && intent.stylePackID == nil
    }
  }
}

/// Immutable keyboard-side capture; the host must freeze the matching Core configuration at admission.
public struct KeyboardVoiceStart: Equatable, Sendable {
  public let intent: KeyboardVoiceIntent
  public let capabilityRevision: UUID?
  public let sourceLanguage: String?
  public let language: KeyboardLanguage
  public init(intent: KeyboardVoiceIntent, capabilities: KeyboardVoiceCapabilities?, language: KeyboardLanguage) {
    self.intent = intent
    self.capabilityRevision = capabilities?.revision
    self.sourceLanguage = capabilities?.sourceLanguage
    self.language = language
  }
  /// Current producers never downgrade an explicit user intent to legacy defaults.
  public func attachMetadata(to command: inout VoiceCommand) -> Bool {
    guard command.kind == .start || command.kind == .confirmGeneratedPreview,
      let capabilityRevision, let sourceLanguage, !sourceLanguage.isEmpty else { return false }
    command.intent = intent
    command.capabilityRevision = capabilityRevision
    command.sourceLanguage = sourceLanguage
    return true
  }
}

/// Selection is entered via the language affordance or a speech-press takeover.
/// Release without a hit cancels; a release emits one frozen target, never a speech stop.
public struct TranslationSelectionGesture: Sendable {
  public private(set) var isSelecting = false
  public private(set) var selectedID: String?
  public private(set) var targets: [KeyboardVoiceOption] = []
  public init() {}
  public mutating func begin(targets: [KeyboardVoiceOption]) {
    self.targets = targets; selectedID = nil; isSelecting = !targets.isEmpty
  }
  public mutating func move(hitID: String?) {
    guard isSelecting else { return }
    selectedID = targets.contains(where: { $0.id == hitID }) ? hitID : nil
  }
  public mutating func release() -> String? {
    let result = isSelecting ? selectedID : nil
    cancel()
    return result
  }
  public mutating func cancel() { isSelecting = false; selectedID = nil; targets = [] }
}
