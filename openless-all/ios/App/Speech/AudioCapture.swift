import AVFoundation
import Foundation
import OpenLessShared

final class AudioCapture {
  private var engine: AVAudioEngine?
  private var continuation: AsyncThrowingStream<AudioPacket, Error>.Continuation?
  private var pump: Task<Void, Never>?
  private var audioSessionToken: UUID?
  private(set) var isRunning = false
  private(set) var sessionID: String?

  func start(
    sessionID: String, admission: NoJumpAudioSession.CaptureAdmission,
    feed: @escaping @Sendable (Data) -> Void,
    failed: @escaping @MainActor @Sendable (String) -> Void
  ) throws {
    guard engine == nil else { throw ContractError.invalidTransition }
    guard MicrophoneAccess.isGranted else {
      throw VoiceFailure.permission
    }
    self.sessionID = sessionID
    do {
      audioSessionToken = try NoJumpAudioSession.shared.acquireCapture(admission: admission)
      let engine = AVAudioEngine()
      let input = engine.inputNode
      let format = input.outputFormat(forBus: 0)
      guard format.sampleRate > 0, format.channelCount > 0 else { throw VoiceFailure.format }
      let (stream, continuation) = AsyncThrowingStream<AudioPacket, Error>.makeStream(
        bufferingPolicy: .bufferingOldest(32))
      self.continuation = continuation
      let bufferSize = AVAudioFrameCount(ceil(format.sampleRate * 0.1))
      if #available(iOS 27, *) {
        try input.installAudioTap(onBus: 0, bufferSize: bufferSize, format: format) { @Sendable buffer, _ in
          if case .dropped = continuation.yield(AudioPacket(buffer: buffer)) {
            continuation.finish(throwing: VoiceFailure.bufferOverflow)
          }
        }
      } else {
        input.installTap(onBus: 0, bufferSize: bufferSize, format: format) { @Sendable buffer, _ in
          do {
            if case .dropped = continuation.yield(try AudioPacket(copying: buffer)) {
              continuation.finish(throwing: VoiceFailure.bufferOverflow)
            }
          } catch { continuation.finish(throwing: error) }
        }
      }
      let worker = PCMWorker()
      pump = Task.detached {
        do {
          for try await packet in stream {
            try Task.checkCancellation()
            let data = try await worker.convert(packet)
            if !data.isEmpty { feed(data) }
          }
          for data in try await worker.finish() { feed(data) }
        } catch is CancellationError {
        } catch { await failed(error.localizedDescription) }
      }
      self.engine = engine
      engine.prepare()
      try engine.start()
      isRunning = true
    } catch {
      stopHardware()
      NSLog("OPENLESS_AUDIO_CAPTURE_START_FAILED session=%@ error=%@ %@",
        sessionID, error.localizedDescription, NoJumpAudioSession.shared.diagnosticReport)
      throw error
    }
  }
  /// Synchronous safety stop; never waits for ASR, Core or a network response.
  func stopHardware(sessionID: String? = nil) {
    if let sessionID, self.sessionID != sessionID { return }
    if let engine {
      engine.inputNode.removeTap(onBus: 0)
      engine.stop()
    }
    engine = nil
    isRunning = false
    self.sessionID = nil
    continuation?.finish()
    continuation = nil
    if let token = audioSessionToken {
      audioSessionToken = nil
      NoJumpAudioSession.shared.releaseCapture(token)
    }
  }
  func stopAndDrain(sessionID: String? = nil) async {
    if let sessionID, let current = self.sessionID, current != sessionID { return }
    stopHardware(sessionID: sessionID)
    let pending = pump
    pump = nil
    await pending?.value
    // Drain PCM callbacks enqueued before the worker completed.
    await withCheckedContinuation { continuation in
      DispatchQueue.main.async { continuation.resume() }
    }
  }
}
