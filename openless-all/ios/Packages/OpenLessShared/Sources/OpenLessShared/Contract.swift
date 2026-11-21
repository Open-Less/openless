import Foundation

public enum KeyboardAppearance: String, Codable, CaseIterable, Sendable {
  case voiceFirst, typingFirst
  public var title: String { self == .voiceFirst ? "语音优先" : "打字优先" }
}
public enum KeyboardExperience: String, Codable, CaseIterable, Sendable {
  case voiceFocused, singleKeyboard
  public var title: String { self == .voiceFocused ? "语音优先" : "单一键盘" }
}
public enum KeyboardLanguage: String, Codable, CaseIterable, Sendable {
  case chinese, english
  public var locale: String { self == .chinese ? "zh-CN" : "en-US" }
  public var title: String { self == .chinese ? "中文全拼" : "English" }
}
public enum SpeechRecognitionDefaults {
  public static let automaticMixedLanguage = "auto"
  public static let appleLocale = "zh-CN"
}
public enum SpaceHoldAction: String, Codable, CaseIterable, Sendable {
  case cursor, recording
  public var title: String { self == .cursor ? "移动光标" : "按住录音" }
}
public struct KeyboardPreferences: Codable, Equatable, Sendable {
  public var version = 1
  public var appearance: KeyboardAppearance = .voiceFirst
  public var language: KeyboardLanguage = .chinese
  public var backgroundKeepAlive = true
  public var spaceHold: SpaceHoldAction = .cursor
  public var haptics = true
  public var keySound = true
  public var keyboardScale = 1.0
  public var experience: KeyboardExperience = .voiceFocused
  public var translationOutputLanguage: String?
  public var usesSpaceRecording: Bool { experience == .singleKeyboard || spaceHold == .recording }
  public var opensVoicePage: Bool { experience == .voiceFocused && appearance == .voiceFirst }

  /// A removed or unavailable choice never falls back to a different language.
  public func translationOutput(in capabilities: KeyboardVoiceCapabilities?) -> KeyboardVoiceOption? {
    guard let capabilities, capabilities.isValid, capabilities.canTranslate,
      let translationOutputLanguage else { return nil }
    return capabilities.translationTargets.first { $0.id == translationOutputLanguage }
  }
  public init() {}
  public init(from decoder: Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    version = try values.decodeIfPresent(Int.self, forKey: .version) ?? 1
    appearance = try values.decodeIfPresent(KeyboardAppearance.self, forKey: .appearance) ?? .voiceFirst
    language = try values.decodeIfPresent(KeyboardLanguage.self, forKey: .language) ?? .chinese
    backgroundKeepAlive = try values.decodeIfPresent(Bool.self, forKey: .backgroundKeepAlive) ?? true
    spaceHold = try values.decodeIfPresent(SpaceHoldAction.self, forKey: .spaceHold) ?? .cursor
    haptics = try values.decodeIfPresent(Bool.self, forKey: .haptics) ?? true
    keySound = try values.decodeIfPresent(Bool.self, forKey: .keySound) ?? true
    keyboardScale = KeyboardSizing.validScale(try values.decodeIfPresent(Double.self, forKey: .keyboardScale) ?? 1)
    experience = try values.decodeIfPresent(KeyboardExperience.self, forKey: .experience) ?? .voiceFocused
    translationOutputLanguage = try values.decodeIfPresent(String.self, forKey: .translationOutputLanguage)
  }
}

public enum VoicePhase: String, Codable, Sendable {
  case inactive, ready, starting, recording, processing, awaitingInsertion, done, cancelled, error
  public var isBusy: Bool {
    [.starting, .recording, .processing, .awaitingInsertion].contains(self)
  }
}
public enum CommandKind: String, Codable, Sendable {
  case ping, prepare, start, stop, cancel, insertionReceipt, confirmGeneratedPreview, vocabularyDecision, finishSpaceTranslation
}
public struct InputTarget: Codable, Equatable, Sendable {
  public let keyboardID: UUID
  public let documentID: UUID
  public let revision: UInt64
  public init(keyboardID: UUID, documentID: UUID, revision: UInt64) {
    self.keyboardID = keyboardID
    self.documentID = documentID
    self.revision = revision
  }
}
public enum InsertionState: String, Codable, Sendable {
  case pending, attempting, submitted, uncertain, targetChanged
}
public struct VoiceCommand: Codable, Equatable, Sendable {
  public var schemaVersion = 2
  public let requestID: UUID
  public let sessionID: UUID?
  public let hostEpoch: UUID?
  public let target: InputTarget
  public let kind: CommandKind
  public let createdAt: Date
  public let deadline: Date
  public let language: KeyboardLanguage
  public var insertionID: UUID?
  public var receipt: InsertionState?
  public var intent: KeyboardVoiceIntent?
  public var capabilityRevision: UUID?
  public var sourceLanguage: String?
  public var previewID: UUID?
  public var vocabularyDecision: VocabularyDecision?
  public var deferredSpaceVoice: Bool?
  public var spaceVoiceFinish: SpaceVoiceFinish?

  public init(
    kind: CommandKind, sessionID: UUID?, hostEpoch: UUID?,
    target: InputTarget, language: KeyboardLanguage, now: Date = Date()
  ) {
    requestID = UUID()
    self.kind = kind
    self.sessionID = sessionID
    self.hostEpoch = hostEpoch
    self.target = target
    self.language = language
    createdAt = now
    deadline = now.addingTimeInterval(10)
  }
  public func rejection(now: Date, epoch: UUID) -> String? {
    if schemaVersion != 2 { return "不支持的消息版本" }
    if deadline < now || createdAt > now.addingTimeInterval(2) { return "请求已过期" }
    if kind != .ping && hostEpoch != epoch { return "主应用会话已更新，请重新连接" }
    if kind != .ping && sessionID == nil { return "缺少录音会话标识" }
    if kind == .confirmGeneratedPreview && previewID == nil { return "缺少帮写预览标识" }
    return nil
  }
}
public struct InsertionRequest: Codable, Equatable, Sendable {
  public let id: UUID
  public let sessionID: UUID
  public let target: InputTarget
  public let text: String
  public let createdAt: Date
  public var state: InsertionState
  public init(sessionID: UUID, target: InputTarget, text: String, now: Date = Date()) {
    id = UUID()
    self.sessionID = sessionID
    self.target = target
    self.text = text
    createdAt = now
    state = .pending
  }
}
public struct VoiceSnapshot: Codable, Equatable, Sendable {
  public var schemaVersion = 2
  public var hostEpoch: UUID
  public var sessionID: UUID?
  public var phase: VoicePhase = .inactive
  public var captureReady = false
  public var level: Float = 0
  public var elapsedMilliseconds: UInt64 = 0
  public var preview = ""
  public var message = "请先打开 OpenLess"
  public var updatedAt = Date()
  public var canStart = false
  public var backgroundReady = false
  /// A bounded output repair is in progress; this never grants capture admission.
  /// Optional to decode snapshots written by earlier app versions.
  public var backgroundRecovering: Bool?
  public var inForeground = true
  public var insertion: InsertionRequest?
  public var capabilities: KeyboardVoiceCapabilities?
  public var activeIntent: KeyboardVoiceIntent?
  public var generatedPreview: KeyboardGeneratedPreview?
  public init(hostEpoch: UUID) { self.hostEpoch = hostEpoch }

  /// Public choices may offer an explicit foreground handoff after a liveness
  /// lease expires. The host must revalidate them before beginning a session;
  /// this never grants permission to start, insert, or resume a stale session.
  public var configuredVoiceCapabilities: KeyboardVoiceCapabilities? {
    guard schemaVersion == 2, let capabilities, capabilities.isValid else { return nil }
    return capabilities
  }

  public func isFresh(at date: Date = Date()) -> Bool {
    let age = date.timeIntervalSince(updatedAt)
    return schemaVersion == 2 && age >= -2 && age <= 4
  }
}
public enum SharedContract {
  public static let maxPayloadBytes = 256 * 1024
  public static let maxTextBytes = 128 * 1024
  public static let maxRecordingSeconds = 30 * 60
  public static let preferencesKey = "keyboard.preferences.v1"
  public static let snapshotKey = "voice.snapshot.v2"
  public static func encode<T: Encodable>(_ value: T) throws -> Data {
    let data = try JSONEncoder().encode(value)
    guard data.count <= maxPayloadBytes else { throw ContractError.tooLarge }
    return data
  }
  public static func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
    guard data.count <= maxPayloadBytes else { throw ContractError.tooLarge }
    return try JSONDecoder().decode(type, from: data)
  }
}
public enum ContractError: Error, LocalizedError, Equatable {
  case tooLarge, unavailable, invalidTransition
  case database(String)
  public var errorDescription: String? {
    switch self {
    case .tooLarge: "内容超过安全上限"
    case .unavailable: "共享容器不可用，请启用键盘完整访问"
    case .invalidTransition: "操作已失效，请重试"
    case .database(let message): "共享存储不可用（\(message)）"
    }
  }
}
