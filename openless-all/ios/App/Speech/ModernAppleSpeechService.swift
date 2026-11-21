import AVFoundation
import Foundation
import OpenLessShared
import Speech

@available(iOS 26, *)
final class ModernAppleSpeechService: AppleSpeechBackend {
  private var analyzer: SpeechAnalyzer?
  private var input: AsyncStream<AnalyzerInput>.Continuation?
  private var analysisTask: Task<Void, Error>?
  private var resultsTask: Task<Void, Error>?
  private var transcript = TranscriptAccumulator()
  private var converter: AVAudioConverter?
  private var sourceFormat: AVAudioFormat?
  private var speechFormat: AVAudioFormat?
  private var receivedBytes: UInt64 = 0
  private(set) var sessionID: String?

  private static func module() async throws -> any SpeechModule {
    let requested = Locale(identifier: SpeechRecognitionDefaults.appleLocale)
    if SpeechTranscriber.isAvailable,
      let locale = await SpeechTranscriber.supportedLocale(equivalentTo: requested)
    {
      return SpeechTranscriber(locale: locale, preset: .timeIndexedProgressiveTranscription)
    }
    if let locale = await DictationTranscriber.supportedLocale(equivalentTo: requested) {
      return DictationTranscriber(locale: locale, preset: .progressiveLongDictation)
    }
    throw VoiceFailure.unsupportedLanguage
  }
  static func prepareAssets(allowDownload: Bool) async throws {
    let module = try await module()
    let status = await AssetInventory.status(forModules: [module])
    if status == .installed { return }
    guard status != .unsupported else { throw VoiceFailure.unsupportedLanguage }
    guard allowDownload else { throw VoiceFailure.assetsMissing }
    if let request = try await AssetInventory.assetInstallationRequest(supporting: [module]) {
      try await request.downloadAndInstall()
    }
    guard await AssetInventory.status(forModules: [module]) == .installed else {
      throw VoiceFailure.assetsMissing
    }
  }
  func start(
    id: String,
    preview: @escaping @MainActor @Sendable (String) -> Void,
    failed: @escaping @MainActor @Sendable (String) -> Void
  ) async throws {
    guard sessionID == nil else { throw ContractError.invalidTransition }
    sessionID = id
    do {
      let module = try await Self.module()
      guard await AssetInventory.status(forModules: [module]) == .installed else {
        throw VoiceFailure.assetsMissing
      }
      guard sessionID == id, !Task.isCancelled else { throw VoiceFailure.cancelled }
      guard
        let speechFormat = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [module]),
        let sourceFormat = AVAudioFormat(
          commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true),
        let converter = AVAudioConverter(from: sourceFormat, to: speechFormat)
      else { throw VoiceFailure.format }
      self.speechFormat = speechFormat
      self.sourceFormat = sourceFormat
      self.converter = converter
      let analyzer = SpeechAnalyzer(modules: [module])
      try await analyzer.prepareToAnalyze(in: speechFormat)
      guard sessionID == id, !Task.isCancelled else {
        await analyzer.cancelAndFinishNow()
        throw VoiceFailure.cancelled
      }
      self.analyzer = analyzer
      receivedBytes = 0
      transcript = TranscriptAccumulator()
      let (stream, input) = AsyncStream<AnalyzerInput>.makeStream(
        bufferingPolicy: .bufferingOldest(32))
      self.input = input
      if let transcriber = module as? SpeechTranscriber {
        resultsTask = Task {
          do {
            for try await result in transcriber.results {
              guard self.sessionID == id, !Task.isCancelled else { return }
              self.apply(
                text: String(result.text.characters), range: result.range, final: result.isFinal,
                preview: preview)
            }
          } catch {
            if self.sessionID == id && !Task.isCancelled { failed(error.localizedDescription) }
            throw error
          }
        }
      } else if let transcriber = module as? DictationTranscriber {
        resultsTask = Task {
          do {
            for try await result in transcriber.results {
              guard self.sessionID == id, !Task.isCancelled else { return }
              self.apply(
                text: String(result.text.characters), range: result.range, final: result.isFinal,
                preview: preview)
            }
          } catch {
            if self.sessionID == id && !Task.isCancelled { failed(error.localizedDescription) }
            throw error
          }
        }
      }
      analysisTask = Task {
        do { try await analyzer.start(inputSequence: stream) } catch {
          if self.sessionID == id && !Task.isCancelled { failed(error.localizedDescription) }
          throw error
        }
      }
    } catch {
      if sessionID == id { await cancel(id: id) }
      throw error
    }
  }
  private func apply(
    text: String, range: CMTimeRange, final: Bool,
    preview: @MainActor (String) -> Void
  ) {
    transcript.apply(
      .init(
        start: range.start.seconds, end: CMTimeRangeGetEnd(range).seconds, text: text, final: final)
    )
    preview(transcript.text)
  }
  func consume(_ data: Data, id: String) throws {
    guard sessionID == id, let input, let sourceFormat, let speechFormat, let converter else {
      return
    }
    guard data.count % 2 == 0, !data.isEmpty,
      let source = AVAudioPCMBuffer(
        pcmFormat: sourceFormat, frameCapacity: AVAudioFrameCount(data.count / 2)),
      let pointer = source.int16ChannelData?[0]
    else { throw VoiceFailure.format }
    source.frameLength = source.frameCapacity
    data.copyBytes(to: UnsafeMutableRawBufferPointer(start: pointer, count: data.count))
    let frames = AVAudioFrameCount(
      ceil(Double(source.frameLength) * speechFormat.sampleRate / 16_000) + 32)
    guard let output = AVAudioPCMBuffer(pcmFormat: speechFormat, frameCapacity: frames) else {
      throw VoiceFailure.format
    }
    var supplied = false
    var error: NSError?
    let status = converter.convert(to: output, error: &error) { _, flag in
      if supplied {
        flag.pointee = .noDataNow
        return nil
      }
      supplied = true
      flag.pointee = .haveData
      return source
    }
    if let error { throw error }
    guard status != .error else { throw VoiceFailure.format }
    receivedBytes += UInt64(data.count)
    if output.frameLength > 0, case .dropped = input.yield(AnalyzerInput(buffer: output)) {
      throw VoiceFailure.bufferOverflow
    }
  }
  func finish(id: String) async throws -> (text: String, duration: UInt64) {
    guard sessionID == id, let analyzer else { throw VoiceFailure.cancelled }
    if let converter, let input {
      for _ in 0..<8 {
        guard let output = AVAudioPCMBuffer(pcmFormat: converter.outputFormat, frameCapacity: 2048)
        else { throw VoiceFailure.format }
        var error: NSError?
        let status = converter.convert(to: output, error: &error) { _, flag in
          flag.pointee = .endOfStream
          return nil
        }
        if let error { throw error }
        guard status != .error else { throw VoiceFailure.format }
        if output.frameLength > 0, case .dropped = input.yield(AnalyzerInput(buffer: output)) {
          throw VoiceFailure.bufferOverflow
        }
        if status == .endOfStream || output.frameLength == 0 { break }
      }
    }
    input?.finish()
    input = nil
    try await analysisTask?.value
    try await analyzer.finalizeAndFinishThroughEndOfInput()
    try await resultsTask?.value
    guard sessionID == id else { throw VoiceFailure.cancelled }
    let output = (text: transcript.text, duration: receivedBytes * 1000 / 32_000)
    sessionID = nil
    self.analyzer = nil
    converter = nil
    resultsTask = nil
    analysisTask = nil
    await SpeechModels.endRetention()
    return output
  }
  func cancel(id: String? = nil) async {
    if let id, id != sessionID { return }
    sessionID = nil
    input?.finish()
    input = nil
    resultsTask?.cancel()
    analysisTask?.cancel()
    let previous = analyzer
    analyzer = nil
    resultsTask = nil
    analysisTask = nil
    converter = nil
    await previous?.cancelAndFinishNow()
    if sessionID == nil { await SpeechModels.endRetention() }
  }
}
