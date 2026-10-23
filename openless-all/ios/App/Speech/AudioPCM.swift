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
