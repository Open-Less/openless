import AVFoundation
import Foundation
import OpenLessShared
import Speech

/// System recognizer path for iOS 16 through 18. Require on-device recognition instead
/// of silently sending audio to Apple's servers when local assets are absent.
final class LegacyAppleSpeechService: AppleSpeechBackend {
  private var recognizer: SFSpeechRecognizer?
  private var request: SFSpeechAudioBufferRecognitionRequest?
  private var recognition: SFSpeechRecognitionTask?
  private var completion: CheckedContinuation<Void, Error>?
  private var finalDeadline: Task<Void, Never>?
  private var transcript = ""
  private var lastFailure: String?
  private var finalReceived = false
  private var finishing = false
  private var bytes: UInt64 = 0
  private(set) var sessionID: String?

  static func prepare() throws {
    guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: SpeechRecognitionDefaults.appleLocale)),
      recognizer.supportsOnDeviceRecognition else { throw VoiceFailure.unsupportedLanguage }
    guard recognizer.isAvailable else { throw VoiceFailure.recognition("系统语音识别暂不可用，请稍后重试。") }
  }
  func start(id: String,
    preview: @escaping @MainActor @Sendable (String) -> Void,
    failed: @escaping @MainActor @Sendable (String) -> Void) async throws {
    guard sessionID == nil else { throw ContractError.invalidTransition }
    try Self.prepare()
    guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: SpeechRecognitionDefaults.appleLocale)) else {
      throw VoiceFailure.unsupportedLanguage
    }
    let request = SFSpeechAudioBufferRecognitionRequest()
    request.requiresOnDeviceRecognition = true
    request.shouldReportPartialResults = true
    request.taskHint = .dictation
    self.recognizer = recognizer
    self.request = request
    transcript = ""; bytes = 0; finalReceived = false; finishing = false; lastFailure = nil
    sessionID = id
    recognition = recognizer.recognitionTask(with: request) { @Sendable [weak self] result, error in
      // Only value snapshots cross the legacy callback's unspecified queue.
      let text = result?.bestTranscription.formattedString
      let final = result?.isFinal == true
      let message = error?.localizedDescription
      Task { @MainActor [weak self] in
        guard let self, self.sessionID == id else { return }
        if let text { self.transcript = text; preview(text) }
        if final {
          self.finalReceived = true
          self.resolve()
        } else if let message {
          self.lastFailure = message
          self.resolve(error: VoiceFailure.recognition(message))
          failed(message)
        }
      }
    }
  }
  func consume(_ data: Data, id: String) throws {
    guard sessionID == id, let request else { throw VoiceFailure.cancelled }
    if let lastFailure { throw VoiceFailure.recognition(lastFailure) }
    guard !finalReceived else { throw VoiceFailure.recognition("系统识别会话已结束，请重新开始。") }
    guard !data.isEmpty, data.count % 2 == 0,
      let format = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true),
      let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(data.count / 2)),
      let target = buffer.int16ChannelData?[0] else { throw VoiceFailure.format }
    buffer.frameLength = buffer.frameCapacity
    data.copyBytes(to: UnsafeMutableRawBufferPointer(start: target, count: data.count))
    bytes += UInt64(data.count)
    request.append(buffer)
  }
  func finish(id: String) async throws -> (text: String, duration: UInt64) {
    guard sessionID == id, !finishing else { throw VoiceFailure.cancelled }
    if let lastFailure { throw VoiceFailure.recognition(lastFailure) }
    finishing = true
    request?.endAudio()
    if !finalReceived {
      try await withCheckedThrowingContinuation { continuation in
        completion = continuation
        finalDeadline = Task { [weak self] in
          do { try await Task.sleep(for: .seconds(8)) } catch { return }
          guard let self, self.sessionID == id else { return }
          self.resolve(error: VoiceFailure.recognition("系统识别结束超时，请重试。"))
        }
      }
    }
    guard sessionID == id else { throw VoiceFailure.cancelled }
    let result = (text: transcript, duration: bytes * 1000 / 32_000)
    await cancel(id: id)
    return result
  }
  private func resolve(error: Error? = nil) {
    finalDeadline?.cancel(); finalDeadline = nil
    let waiter = completion; completion = nil
    if let error { waiter?.resume(throwing: error) } else { waiter?.resume() }
  }
  func cancel(id: String? = nil) async {
    if let id, id != sessionID { return }
    sessionID = nil
    resolve(error: VoiceFailure.cancelled)
    recognition?.cancel(); recognition = nil
    request = nil; recognizer = nil
    transcript = ""; bytes = 0; finishing = false; finalReceived = false; lastFailure = nil
  }
}
