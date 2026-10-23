import AVFoundation
import Combine

final class HistoryAudioPlayer: NSObject, ObservableObject, AVAudioPlayerDelegate {
  @Published private(set) var playing = false
  @Published private(set) var error: String?
  private var player: AVAudioPlayer?
  private var audioSessionToken: UUID?

  func toggle(_ url: URL) {
    if playing { stop(); return }
    do {
      audioSessionToken = try NoJumpAudioSession.shared.acquirePlayback()
      let player = try AVAudioPlayer(contentsOf: url)
      player.delegate = self
      guard player.prepareToPlay(), player.play() else { throw VoiceFailure.cancelled }
      self.player = player; playing = true; error = nil
    } catch { self.error = "无法播放这段录音，请检查文件是否仍然存在。"; stop() }
  }
  func stop() {
    player?.stop(); player = nil; playing = false
    if let token = audioSessionToken {
      audioSessionToken = nil
      NoJumpAudioSession.shared.releasePlayback(token)
    }
  }
  nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
    let id = ObjectIdentifier(player)
    Task { @MainActor [weak self] in
      guard let self, self.player.map(ObjectIdentifier.init) == id else { return }
      self.stop()
    }
  }
  nonisolated func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: (any Error)?) {
    let id = ObjectIdentifier(player)
    Task { @MainActor [weak self] in
      guard let self, self.player.map(ObjectIdentifier.init) == id else { return }
      self.error = "录音文件无法解码。"; self.stop()
    }
  }
}
