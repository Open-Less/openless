import AVFoundation
import OpenLessShared

/// The app owns permission requests. Keyboard extensions never request capture.
enum MicrophoneAccess {
  static var isGranted: Bool {
    if #available(iOS 17, *) { return AVAudioApplication.shared.recordPermission == .granted }
    return AVAudioSession.sharedInstance().recordPermission == .granted
  }
  static var isDenied: Bool {
    if #available(iOS 17, *) { return AVAudioApplication.shared.recordPermission == .denied }
    return AVAudioSession.sharedInstance().recordPermission == .denied
  }
  static func request() async -> Bool {
    if #available(iOS 17, *) { return await AVAudioApplication.requestRecordPermission() }
    return await CallbackBridge.value { completion in
      AVAudioSession.sharedInstance().requestRecordPermission(completion)
    }
  }
}
