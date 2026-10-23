// Frozen pre-optimization oracle from f0744e10. Test-only.
import AVFoundation
import Foundation
actor ReferencePCMWorker {
  private var converter: AVAudioConverter?
  func convert(_ packet: AudioPacket) throws -> Data {
    let input = try packet.makeBuffer()
    let inputFormat = input.format
    guard inputFormat.sampleRate > 0, input.frameLength > 0,
      let outputFormat = AVAudioFormat(
        commonFormat: .pcmFormatInt16, sampleRate: 16_000, channels: 1, interleaved: true)
    else {
      throw VoiceFailure.format
    }
    if converter?.inputFormat != inputFormat {
      converter = AVAudioConverter(from: inputFormat, to: outputFormat)
    }
    guard let converter else { throw VoiceFailure.format }
    let capacity = AVAudioFrameCount(
      ceil(Double(input.frameLength) * 16_000 / inputFormat.sampleRate) + 32)
    guard let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: capacity) else {
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
      return input
    }
    if let error { throw error }
    guard status != .error else { throw VoiceFailure.format }
    guard output.frameLength > 0, let bytes = output.int16ChannelData?[0] else { return Data() }
    return Data(bytes: bytes, count: Int(output.frameLength) * 2)
  }
  func finish() throws -> [Data] {
    guard let converter else { return [] }
    var tail: [Data] = []
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
      if output.frameLength > 0, let bytes = output.int16ChannelData?[0] {
        tail.append(Data(bytes: bytes, count: Int(output.frameLength) * 2))
      }
      if status == .endOfStream || output.frameLength == 0 { return tail }
    }
    throw VoiceFailure.format
  }
}
