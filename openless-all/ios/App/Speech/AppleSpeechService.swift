import Foundation
import OpenLessShared
import Speech

@MainActor
protocol AppleSpeechBackend: AnyObject {
  var sessionID: String? { get }
  func start(id: String,
    preview: @escaping @MainActor @Sendable (String) -> Void,
    failed: @escaping @MainActor @Sendable (String) -> Void) async throws
  func consume(_ data: Data, id: String) throws
  func finish(id: String) async throws -> (text: String, duration: UInt64)
  func cancel(id: String?) async
}

/// Apple is an explicitly selected developer provider. Normal mode keeps
/// SenseVoice on every supported OS; this is never an automatic cloud fallback.
final class AppleSpeechService {
  private var backend: (any AppleSpeechBackend)?

  static func microphonePermission() async -> Bool { await MicrophoneAccess.request() }
  static func permissions() async -> Bool {
    guard await microphonePermission() else { return false }
    return await speechAuthorization() == .authorized
  }
  static func speechAuthorization() async -> SFSpeechRecognizerAuthorizationStatus {
    await CallbackBridge.value { completion in
      SFSpeechRecognizer.requestAuthorization(completion)
    }
  }
  static func prepareAssets(allowDownload: Bool) async throws {
    if #available(iOS 26, *) {
      try await ModernAppleSpeechService.prepareAssets(allowDownload: allowDownload)
    } else {
      try LegacyAppleSpeechService.prepare()
    }
  }
  func start(id: String,
    preview: @escaping @MainActor @Sendable (String) -> Void,
    failed: @escaping @MainActor @Sendable (String) -> Void) async throws {
    guard backend == nil else { throw ContractError.invalidTransition }
    let current: any AppleSpeechBackend
    if #available(iOS 26, *) { current = ModernAppleSpeechService() }
    else { current = LegacyAppleSpeechService() }
    backend = current
    do { try await current.start(id: id, preview: preview, failed: failed) }
    catch {
      if backend === current { backend = nil }
      await current.cancel(id: id)
      throw error
    }
  }
  func consume(_ data: Data, id: String) throws {
    guard let backend else { throw VoiceFailure.cancelled }
    try backend.consume(data, id: id)
  }
  func finish(id: String) async throws -> (text: String, duration: UInt64) {
    guard let current = backend else { throw VoiceFailure.cancelled }
    do {
      let result = try await current.finish(id: id)
      if backend === current { backend = nil }
      return result
    } catch {
      if backend === current { backend = nil }
      await current.cancel(id: id)
      throw error
    }
  }
  func cancel(id: String? = nil) async {
    guard let current = backend, id == nil || current.sessionID == id else { return }
    backend = nil
    await current.cancel(id: id)
  }
}
