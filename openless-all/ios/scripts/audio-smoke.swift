import AVFoundation
import Foundation

@main struct AudioSmoke {
  static func main() async throws {
    var combinations = 0
    for legacy in [false, true] {
      if !legacy {
        if #unavailable(macOS 27) { continue }
      }
      for rate in [8_000.0, 16_000, 44_100, 48_000] {
        for channels: AVAudioChannelCount in [1, 2] {
          let worker = PCMWorker()
          let reference = ReferencePCMWorker()
          var pcm = Data()
          var expected = Data()
          var frameOffset = 0
          for step in 0..<10 {
            let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: channels, interleaved: false)!
            // Alternate small and large packets to exercise scratch growth,
            // reuse, and a final flush into a previously larger buffer.
            let frames = AVAudioFrameCount(rate * (step % 2 == 0 ? 0.02 : 0.18))
            let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames)!
            buffer.frameLength = frames
            for channel in 0..<Int(channels) {
              for frame in 0..<Int(frames) {
                buffer.floatChannelData![channel][frame] = Float(sin(2 * Double.pi * 440 * Double(frame + frameOffset) / rate) * 0.2)
              }
            }
            let packet: AudioPacket
            if legacy {
              packet = try AudioPacket(copying: buffer)
              // Legacy callbacks may reuse this storage as soon as they return.
              // Overwrite it before conversion to prove independent ownership.
              for channel in 0..<Int(channels) {
                buffer.floatChannelData![channel].update(repeating: 0, count: Int(frames))
              }
            } else if #available(macOS 27, *) {
              packet = AudioPacket(buffer: AVReadOnlyAudioPCMBuffer(copying: buffer))
            } else {
              preconditionFailure("Immutable audio buffers require macOS 27")
            }
            pcm.append(try await worker.convert(packet))
            expected.append(try await reference.convert(packet))
            frameOffset += Int(frames)
          }
          for tail in try await worker.finish() { pcm.append(tail) }
          for tail in try await reference.finish() { expected.append(tail) }
          precondition(pcm == expected, "optimized PCM differs from the original converter: \(rate), \(channels), legacy=\(legacy)")
          precondition(abs(pcm.count / 2 - 16_000) <= 64, "resampling changed duration: \(rate), \(pcm.count)")
          let samples = pcm.withUnsafeBytes { Array($0.bindMemory(to: Int16.self)) }
          let power = samples.map { pow(Double($0) / 32_768, 2) }.reduce(0, +) / Double(samples.count)
          precondition(power > 0.005 && power < 0.1, "invalid PCM amplitude")
          print("PCM \(legacy ? "legacy copy" : "immutable") \(Int(rate))Hz × \(channels) → 16kHz mono: \(samples.count) samples")
          combinations += 1
        }
      }
    }
    print("Audio conversion smoke passed: \(combinations) combinations; no microphone was opened.")
  }
}
