import AVFoundation
import Foundation
import LocalSpeech
import Testing

private final class SpeechFixtureBundle: NSObject {}
struct SenseVoiceTests {
  @Test func cancellationIsObservedBeforeModelAccess() async {
    let runtime = SenseVoiceRuntime()
    let token = LocalSpeechCancellation()
    token.cancel()
    do {
      _ = try await runtime.recognize(pcm: Data(), cancellation: token)
      Issue.record("A cancelled session must not decode")
    } catch let error as LocalSpeechError {
      if case .cancelled = error {} else { Issue.record("Unexpected error: \(error)") }
    } catch { Issue.record("Unexpected error: \(error)") }
  }

  @Test @MainActor func packagedModelRecognizesPublicMandarinFixtureWithoutMicrophone() async throws {
    let model = try #require(Bundle.main.url(forResource: "model.int8", withExtension: "onnx", subdirectory: "SenseVoice"))
    let tokens = try #require(Bundle.main.url(forResource: "tokens", withExtension: "txt", subdirectory: "SenseVoice"))
    let fixture = try #require(Bundle(for: SpeechFixtureBundle.self).url(forResource: "sensevoice-zh", withExtension: "wav"))
    let file = try AVAudioFile(forReading: fixture, commonFormat: .pcmFormatInt16, interleaved: true)
    #expect(file.processingFormat.sampleRate == 16_000)
    #expect(file.processingFormat.channelCount == 1)
    let buffer = try #require(AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: UInt32(file.length)))
    try file.read(into: buffer)
    let samples = try #require(buffer.int16ChannelData)
    let pcm = Data(bytes: samples[0], count: Int(buffer.frameLength) * 2)
    let runtime = SenseVoiceRuntime()
    try await runtime.prepare(model: model, tokens: tokens, language: "zh")
    let text = try await runtime.recognize(pcm: pcm, cancellation: LocalSpeechCancellation())
    // The upstream fixture is public speech without an accompanying gold
    // transcript. This is a real-model integration smoke, not an accuracy benchmark.
    #expect(!text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    #expect(text.range(of: "\\p{Han}", options: .regularExpression) != nil, "Actual local result: \(text)")
    await runtime.release()
  }
}
