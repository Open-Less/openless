import AVFoundation
import Foundation

@main struct AudioSmoke {
  static func packet(rate: Double, channels: AVAudioChannelCount, frames: AVAudioFrameCount, offset: Int, legacy: Bool) throws -> AudioPacket {
    let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: channels, interleaved: false)!
    let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
    buffer.frameLength = frames
    for channel in 0..<Int(channels) {
      for frame in 0..<Int(frames) {
        buffer.floatChannelData![channel][frame] = Float(sin(2 * Double.pi * 440 * Double(frame + offset) / rate) * 0.2)
      }
    }
    if legacy {
      let packet = try AudioPacket(copying: buffer)
      // Legacy callbacks reuse their mutable storage after returning.
      for channel in 0..<Int(channels) { buffer.floatChannelData![channel].update(repeating: 0, count: Int(frames)) }
      return packet
    }
    if #available(macOS 27, *) { return AudioPacket(buffer: AVReadOnlyAudioPCMBuffer(copying: buffer)) }
    throw VoiceFailure.format
  }
  static func main() async throws {
    var combinations = 0
    for legacy in [false, true] {
      if !legacy {
        if #unavailable(macOS 27) { continue }
      }
      for rate in [8_000.0, 16_000, 44_100, 48_000] {
        for channels: AVAudioChannelCount in [1, 2] {
          for variablePackets in [false, true] {
          let worker = PCMWorker()
          let reference = ReferencePCMWorker()
          var pcm = Data()
          var expected = Data()
          var frameOffset = 0
          for step in 0..<10 {
            let frames = AVAudioFrameCount(rate * (variablePackets ? (step % 2 == 0 ? 0.02 : 0.18) : 0.1))
            pcm.append(try await worker.convert(packet(rate: rate, channels: channels, frames: frames, offset: frameOffset, legacy: legacy)))
            // The original 100 ms path is the frozen, loss-free oracle.
            // Its variable-packet path loses samples at 44.1 kHz, so compare
            // both new packetizations against the same complete waveform.
            let fixed = AVAudioFrameCount(rate / 10)
            expected.append(try await reference.convert(packet(rate: rate, channels: channels, frames: fixed, offset: step * Int(fixed), legacy: legacy)))
            frameOffset += Int(frames)
          }
          for tail in try await worker.finish() { pcm.append(tail) }
          for tail in try await reference.finish() { expected.append(tail) }
          precondition(pcm == expected, "optimized PCM differs from the complete baseline: \(rate), \(channels), legacy=\(legacy), variable=\(variablePackets)")
          precondition(abs(pcm.count / 2 - 16_000) <= 64, "resampling changed duration: \(rate), \(pcm.count)")
          let samples = pcm.withUnsafeBytes { Array($0.bindMemory(to: Int16.self)) }
          let power = samples.map { pow(Double($0) / 32_768, 2) }.reduce(0, +) / Double(samples.count)
          precondition(power > 0.005 && power < 0.1, "invalid PCM amplitude")
          print("PCM \(legacy ? "legacy copy" : "immutable") \(Int(rate))Hz × \(channels), variable=\(variablePackets) → 16kHz mono: \(samples.count) samples")
          combinations += 1
          }
        }
      }
    }
    print("Audio conversion smoke passed: \(combinations) combinations; no microphone was opened.")
  }
}
