import Foundation
import SherpaOnnxC

public enum LocalSpeechError: Error, LocalizedError, Sendable {
  case modelMissing, initializationFailed, invalidAudio, cancelled, resultTooLarge
  public var errorDescription: String? {
    switch self {
    case .modelMissing: "未找到打包的 SenseVoice 模型，请重新构建应用。"
    case .initializationFailed: "SenseVoice 本地识别器初始化失败。"
    case .invalidAudio: "本地语音数据格式不正确。"
    case .cancelled: "识别已取消。"
    case .resultTooLarge: "识别文字过长，请分段录音。"
    }
  }
}

/// Cancellation is visible immediately even while the decoder actor is busy.
/// The native decoder finishes at most the current bounded 20-second chunk.
public final class LocalSpeechCancellation: @unchecked Sendable {
  // Every access is protected by this lock; no mutable state escapes. NSLock
  // also works on iOS 16/17, where Synchronization.Mutex is unavailable.
  private let lock = NSLock()
  private var cancelled = false
  public init() {}
  public func cancel() { lock.withLock { cancelled = true } }
  public func check() throws {
    if lock.withLock({ cancelled }) || Task.isCancelled { throw LocalSpeechError.cancelled }
  }
}

private final class RecognizerHandle {
  let pointer: OpaquePointer
  init(_ pointer: OpaquePointer) { self.pointer = pointer }
  deinit { SherpaOnnxDestroyOfflineRecognizer(pointer) }
}

/// C handles never leave this actor. The extension does not link this module.
public actor SenseVoiceRuntime {
  private var recognizer: RecognizerHandle?
  private var language = ""
  public init() {}

  public func prepare(model: URL, tokens: URL, language: String) throws {
    if recognizer != nil, self.language == language { return }
    guard FileManager.default.fileExists(atPath: model.path),
      FileManager.default.fileExists(atPath: tokens.path)
    else { throw LocalSpeechError.modelMissing }
    recognizer = nil
    let pointer = model.path.withCString { modelPath in
      tokens.path.withCString { tokenPath in
        language.withCString { languageCode in
          "cpu".withCString { provider in
            "greedy_search".withCString { decoding in
              var config = SherpaOnnxOfflineRecognizerConfig()
              config.feat_config.sample_rate = 16_000
              config.feat_config.feature_dim = 80
              config.model_config.sense_voice.model = modelPath
              config.model_config.sense_voice.language = languageCode
              config.model_config.sense_voice.use_itn = 1
              config.model_config.tokens = tokenPath
              config.model_config.num_threads = 2
              config.model_config.provider = provider
              config.decoding_method = decoding
              return SherpaOnnxCreateOfflineRecognizer(&config)
            }
          }
        }
      }
    }
    guard let pointer else { throw LocalSpeechError.initializationFailed }
    recognizer = RecognizerHandle(pointer)
    self.language = language
  }

  public func recognize(pcm: Data, cancellation: LocalSpeechCancellation) throws -> String {
    try cancellation.check()
    guard let recognizer else { throw LocalSpeechError.initializationFailed }
    guard pcm.count % 2 == 0, pcm.count <= 16_000 * 2 * 30 * 60 else { throw LocalSpeechError.invalidAudio }
    let chunkBytes = 16_000 * 2 * 20
    var text = ""
    for offset in stride(from: 0, to: pcm.count, by: chunkBytes) {
      try cancellation.check()
      let length = min(chunkBytes, pcm.count - offset)
      let samples: [Float] = pcm.withUnsafeBytes { bytes in
        (0..<(length / 2)).map {
          Float(Int16(littleEndian: bytes.loadUnaligned(fromByteOffset: offset + $0 * 2, as: Int16.self))) / 32768
        }
      }
      guard let stream = SherpaOnnxCreateOfflineStream(recognizer.pointer) else {
        throw LocalSpeechError.initializationFailed
      }
      defer { SherpaOnnxDestroyOfflineStream(stream) }
      samples.withUnsafeBufferPointer { buffer in
        SherpaOnnxAcceptWaveformOffline(stream, 16_000, buffer.baseAddress, Int32(buffer.count))
      }
      SherpaOnnxDecodeOfflineStream(recognizer.pointer, stream)
      try cancellation.check()
      if let result = SherpaOnnxGetOfflineStreamResult(stream) {
        defer { SherpaOnnxDestroyOfflineRecognizerResult(result) }
        if let value = result.pointee.text {
          let segment = String(cString: value).trimmingCharacters(in: .whitespacesAndNewlines)
          if !text.isEmpty, !segment.isEmpty, language == "en" { text += " " }
          text += segment
        }
      }
      guard text.utf8.count <= 128 * 1024 else { throw LocalSpeechError.resultTooLarge }
    }
    return text
  }
  public func release() { recognizer = nil; language = "" }
}
