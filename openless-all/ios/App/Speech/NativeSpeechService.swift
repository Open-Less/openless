import Foundation
import LocalSpeech
import OpenLessShared

/// Owns only native recognizers. Cloud ASR consumes the same PCM inside Core.
final class NativeSpeechService {
  private let apple = AppleSpeechService()
  private let local = SenseVoiceRuntime()
  private nonisolated let pcmBuffer = NativePCMBuffer()
  private final class Session {
    let id: String
    let provider: String
    let cancellation = LocalSpeechCancellation()
    init(id: String, provider: String) { self.id = id; self.provider = provider }
  }
  private var session: Session?

  func prepare(provider: String, allowDownload: Bool) async throws {
    if provider == "apple-speech" {
      guard await AppleSpeechService.permissions() else { throw VoiceFailure.permission }
      try await AppleSpeechService.prepareAssets(allowDownload: allowDownload)
      return
    }
    guard await AppleSpeechService.microphonePermission() else { throw VoiceFailure.permission }
    if provider == "sensevoice" || provider == "ios-sensevoice" {
      guard let model = Bundle.main.url(forResource: "model.int8", withExtension: "onnx", subdirectory: "SenseVoice"),
        let tokens = Bundle.main.url(forResource: "tokens", withExtension: "txt", subdirectory: "SenseVoice")
      else { throw LocalSpeechError.modelMissing }
      try await local.prepare(model: model, tokens: tokens,
        language: SpeechRecognitionDefaults.automaticMixedLanguage)
    }
    // Other provider types are Core-managed cloud/Omni; no Apple resource is
    // required and no cloud call is made merely by preparing microphone access.
  }
  func start(
    id: String, provider: String,
    preview: @escaping @MainActor @Sendable (String) -> Void,
    failed: @escaping @MainActor @Sendable (String) -> Void
  ) async throws {
    guard session == nil else { throw ContractError.invalidTransition }
    guard provider == "sensevoice" || provider == "apple-speech" else { throw VoiceFailure.unsupportedLanguage }
    let current = Session(id: id, provider: provider)
    session = current
    do {
      try pcmBuffer.begin(id: id, forwardsToMain: provider == "apple-speech")
      if provider == "apple-speech" {
        try await apple.start(id: id, preview: preview, failed: failed)
      } else {
        try await prepare(provider: provider, allowDownload: false)
      }
      guard session === current, !Task.isCancelled else { throw VoiceFailure.cancelled }
    } catch {
      pcmBuffer.cancel(id: id)
      if session === current { session = nil }
      throw error
    }
  }
  nonisolated func consumeFromCore(_ pcm: Data, id: String) -> NativeAudioDisposition {
    pcmBuffer.append(pcm, id: id)
  }
  func consume(_ pcm: Data, id: String) throws {
    guard let current = session, current.id == id else { throw VoiceFailure.cancelled }
    if current.provider == "apple-speech" { try apple.consume(pcm, id: id); return }
    if pcmBuffer.append(pcm, id: id) == .overflow {
      throw LocalSpeechError.invalidAudio
    }
  }
  func finish(id: String) async throws -> (text: String, duration: UInt64) {
    guard let current = session, current.id == id else { throw VoiceFailure.cancelled }
    do {
      let pcm = try pcmBuffer.finish(id: id)
      if current.provider == "apple-speech" {
        let result = try await apple.finish(id: id)
        guard session === current, !Task.isCancelled else { throw VoiceFailure.cancelled }
        session = nil
        return result
      }
      let text = try await local.recognize(pcm: pcm, cancellation: current.cancellation)
      guard session === current, !Task.isCancelled else { throw VoiceFailure.cancelled }
      session = nil
      return (text, UInt64(pcm.count) * 1000 / 32_000)
    } catch {
      pcmBuffer.cancel(id: id)
      if session === current { session = nil }
      throw error
    }
  }
  func cancel(id: String? = nil) async {
    if let id, id != session?.id { return }
    pcmBuffer.cancel(id: id)
    session?.cancellation.cancel()
    session = nil
    await apple.cancel(id: id)
  }
  func release() async {
    await cancel()
    await local.release()
  }
}
