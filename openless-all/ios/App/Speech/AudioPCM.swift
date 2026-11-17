import AVFoundation
import Foundation

nonisolated enum VoiceFailure: Error, LocalizedError, Sendable {
  case permission, unsupportedLanguage, assetsMissing, bufferOverflow, cancelled, format
  case recognition(String)
  var errorDescription: String? {
    switch self {
    case .permission: "请先允许当前识别方式需要的麦克风和语音权限。"
    case .unsupportedLanguage: "这台设备尚不支持中英混合自动识别。"
    case .assetsMissing: "请先在主应用下载系统语音资源。"
    case .bufferOverflow: "音频处理未能跟上输入，已停止录音，请重试。"
    case .cancelled: "录音已取消。"
    case .format: "当前音频设备格式不可用，请更换输入设备后重试。"
    case .recognition(let message): message
    }
  }
}
nonisolated struct AudioPacket: Sendable {
  private let materialize: @Sendable () throws -> AVAudioPCMBuffer

  @available(iOS 27, macOS 27, *)
  init(buffer: AVReadOnlyAudioPCMBuffer) {
    // The modern immutable buffer remains zero-copy on the real-time thread.
    materialize = { AVAudioPCMBuffer(copying: buffer) }
  }

  init(copying buffer: AVAudioPCMBuffer) throws {
    // Legacy taps reuse their mutable buffer after returning. Snapshot value
    // data before crossing actors; never mark a shared audio buffer Sendable.
    let rate = buffer.format.sampleRate
    let channels = buffer.format.channelCount
    let common = buffer.format.commonFormat.rawValue
    let interleaved = buffer.format.isInterleaved
    let frames = buffer.frameLength
    guard rate > 0, channels > 0, frames > 0 else { throw VoiceFailure.format }
    let source = UnsafeMutableAudioBufferListPointer(buffer.mutableAudioBufferList)
    let chunks = try source.map { item -> Data in
      guard let bytes = item.mData, item.mDataByteSize > 0 else { throw VoiceFailure.format }
      return Data(bytes: bytes, count: Int(item.mDataByteSize))
    }
    materialize = {
      guard let commonFormat = AVAudioCommonFormat(rawValue: common),
        let format = AVAudioFormat(commonFormat: commonFormat, sampleRate: rate, channels: channels, interleaved: interleaved),
        let copy = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames) else { throw VoiceFailure.format }
      copy.frameLength = frames
      let target = UnsafeMutableAudioBufferListPointer(copy.mutableAudioBufferList)
      guard target.count == chunks.count else { throw VoiceFailure.format }
      for (index, bytes) in chunks.enumerated() {
        guard let output = target[index].mData, Int(target[index].mDataByteSize) >= bytes.count else { throw VoiceFailure.format }
        bytes.copyBytes(to: UnsafeMutableRawBufferPointer(start: output, count: bytes.count))
      }
      return copy
    }
  }

  func makeBuffer() throws -> AVAudioPCMBuffer { try materialize() }
}

/// Owns the resampler off the UI and real-time audio callback.
actor PCMWorker {
  private var converter: AVAudioConverter?
  private var scratch: [AVAudioPCMBuffer] = []

  private func outputBuffer(format: AVAudioFormat, capacity: AVAudioFrameCount) throws -> AVAudioPCMBuffer {
    // Converter output capacity affects its pull requests at fractional
    // ratios (e.g. 44.1 -> 16 kHz). Preserve the original capacity
    // exactly, including flush; do not substitute a larger cached buffer.
    if let output = scratch.first(where: { $0.format == format && $0.frameCapacity == capacity }) {
      output.frameLength = 0
      return output
    }
    guard let output = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else { throw VoiceFailure.format }
    // Two entries cover alternating packet sizes without retaining an
    // unbounded set of device callback capacities.
    if scratch.count == 2 { scratch.removeFirst() }
    scratch.append(output)
    return output
  }
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
    var cursor: AVAudioFrameCount = 0
    var invalidInput = false
    var retainedInput: AVAudioPCMBuffer?
    var pcm = Data()
    // A filled output buffer does not imply that the converter has consumed
    // the entire input packet. Drain .haveData before accepting the next
    // packet, otherwise alternating packet sizes can silently lose frames.
    for _ in 0..<8 {
      let output = try outputBuffer(format: outputFormat, capacity: capacity)
      var error: NSError?
      let status = converter.convert(to: output, error: &error) { requested, flag in
        if cursor == input.frameLength {
          flag.pointee = .noDataNow
          return nil
        }
        let frames = min(AVAudioFrameCount(requested), input.frameLength - cursor)
        guard frames > 0 else { invalidInput = true; flag.pointee = .noDataNow; return nil }
        flag.pointee = .haveData
        if cursor == 0 && frames == input.frameLength {
          cursor = input.frameLength
          return input
        }
        // Respect the converter's requested frame count. Returning a whole
        // large packet and marking it supplied loses its unconsumed suffix.
        guard let slice = AVAudioPCMBuffer(pcmFormat: inputFormat, frameCapacity: frames) else {
          invalidInput = true; flag.pointee = .noDataNow; return nil
        }
        slice.frameLength = frames
        let source = UnsafeMutableAudioBufferListPointer(input.mutableAudioBufferList)
        let target = UnsafeMutableAudioBufferListPointer(slice.mutableAudioBufferList)
        let stride = Int(inputFormat.streamDescription.pointee.mBytesPerFrame)
        let offset = Int(cursor) * stride
        let count = Int(frames) * stride
        guard stride > 0, source.count == target.count else { invalidInput = true; flag.pointee = .noDataNow; return nil }
        for index in source.indices {
          guard let from = source[index].mData, let to = target[index].mData,
            offset + count <= Int(source[index].mDataByteSize), count <= Int(target[index].mDataByteSize)
          else { invalidInput = true; flag.pointee = .noDataNow; return nil }
          to.copyMemory(from: from.advanced(by: offset), byteCount: count)
        }
        cursor += frames
        retainedInput = slice
        return slice
      }
      withExtendedLifetime(retainedInput) {}
      guard !invalidInput else { throw VoiceFailure.format }
      if let error { throw error }
      guard status != .error else { throw VoiceFailure.format }
      if output.frameLength > 0, let bytes = output.int16ChannelData?[0] {
        pcm.append(Data(bytes: bytes, count: Int(output.frameLength) * 2))
      }
      if status == .inputRanDry || status == .endOfStream || output.frameLength == 0 {
        guard cursor == input.frameLength else { throw VoiceFailure.format }
        return pcm
      }
    }
    throw VoiceFailure.format
  }
  func finish() throws -> [Data] {
    guard let converter else { return [] }
    var tail: [Data] = []
    for _ in 0..<8 {
      let output = try outputBuffer(format: converter.outputFormat, capacity: 2048)
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
