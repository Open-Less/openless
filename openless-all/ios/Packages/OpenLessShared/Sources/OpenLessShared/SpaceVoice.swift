import Foundation

/// A stop request is terminal for capture, even before the host acknowledges it.
public struct KeyboardVoiceProgress: Sendable {
  public static let processingTimeout: TimeInterval = 180
  private var stoppedSession: UUID?
  private var stoppedAt: Date?
  public init() {}
  public mutating func stopped(session: UUID, now: Date = Date()) {
    guard stoppedSession != session else { return }
    stoppedSession = session
    stoppedAt = now
  }
  public func phase(_ reported: VoicePhase, session: UUID?) -> VoicePhase {
    guard let session, session == stoppedSession else { return reported }
    return reported == .starting || reported == .recording ? .processing : reported
  }
  public func expired(session: UUID, now: Date = Date()) -> Bool {
    guard session == stoppedSession, let stoppedAt else { return false }
    return now.timeIntervalSince(stoppedAt) >= Self.processingTimeout
  }
  public mutating func reset() { stoppedSession = nil; stoppedAt = nil }
}

/// The recording surface and compact status strip share one phase projection.
public enum KeyboardVoicePresentation: Sendable, Equatable {
  case idle, preparing, recording, processing
  public init(phase: VoicePhase, hasSession: Bool, captureReady: Bool) {
    guard hasSession else { self = .idle; return }
    switch phase {
    case .starting: self = .preparing
    case .recording: self = captureReady ? .recording : .preparing
    case .processing, .awaitingInsertion: self = .processing
    default: self = .idle
    }
  }
  public var title: String {
    switch self {
    case .idle: ""
    case .preparing: "正在准备录音"
    case .recording: "正在聆听"
    case .processing: "thinking"
    }
  }
}

/// A terminal choice for one held recording.
public struct SpaceVoiceFinish: Codable, Equatable, Sendable {
  public enum Kind: String, Codable, Sendable { case input, send, chooseTranslation, translate }
  public let kind: Kind
  public let targetLanguage: String?
  public init(_ kind: Kind, targetLanguage: String? = nil) {
    self.kind = kind; self.targetLanguage = targetLanguage
  }
  public var isValid: Bool {
    if kind != .translate { return targetLanguage == nil }
    guard let targetLanguage else { return false }
    return !targetLanguage.isEmpty && targetLanguage.utf8.count <= 128
      && !targetLanguage.contains(where: { $0.isNewline || $0.asciiValue.map { $0 < 32 } == true })
  }
}

/// Holds recognition and language selection until one terminal action is ready.
public struct SpaceVoiceDeliveryState: Sendable {
  public private(set) var decision: SpaceVoiceFinish?
  public private(set) var text: String?
  private var consumed = false
  public init() {}
  @discardableResult public mutating func choose(_ choice: SpaceVoiceFinish) -> Bool {
    guard choice.isValid, !consumed else { return false }
    guard decision == nil || (decision?.kind == .chooseTranslation && choice.kind == .translate) else { return false }
    decision = choice
    return true
  }
  public mutating func recognized(_ text: String) {
    guard self.text == nil, !consumed else { return }
    self.text = text
  }
  public mutating func takeWork() -> (SpaceVoiceFinish, String)? {
    guard !consumed, let decision, decision.kind != .chooseTranslation, let text else { return nil }
    consumed = true
    return (decision, text)
  }
}

/// Authorizes one send action for one session and input target.
public struct KeyboardSubmitIntent: Sendable {
  public let sessionID: UUID
  public let target: InputTarget
  private var consumed = false
  public init(sessionID: UUID, target: InputTarget) { self.sessionID = sessionID; self.target = target }
  public mutating func consume(for insertion: InsertionRequest, current: InputTarget?, supportsSend: Bool) -> Bool {
    guard !consumed else { return false }
    consumed = true
    return supportsSend && current == target && insertion.target == target && insertion.sessionID == sessionID
      && !insertion.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }
}

/// Resolves the terminal action from the visible overlay and release location.
public struct SpaceVoiceGesture: Sendable {
  public enum Action: Sendable, Equatable { case input, cancel, translate, send }
  public private(set) var active = false
  public private(set) var selection: Action = .input
  public init() {}
  public mutating func begin() { active = true; selection = .input }
  public mutating func move(hit: Action?, inRestArea: Bool) {
    guard active else { return }
    selection = hit ?? (inRestArea ? .input : .cancel)
  }
  public mutating func release(hit: Action?, inRestArea: Bool) -> Action? {
    guard active else { return nil }
    move(hit: hit, inRestArea: inRestArea)
    let action = selection
    cancel()
    return action
  }
  public mutating func cancel() { active = false; selection = .input }
}
