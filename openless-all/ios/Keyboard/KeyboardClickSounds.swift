import KeyboardKit
import UIKit

/// Enables the system input click only while this keyboard is on screen.
final class KeyboardAudioInputView: UIInputView, UIInputViewAudioFeedback {
  nonisolated var enableInputClicksWhenVisible: Bool { true }
}

@MainActor
enum KeyboardClickSounds {
  static func play() { UIDevice.current.playInputClick() }
}

/// Key gestures share the public input-click path with the custom text controls.
nonisolated final class SystemKeyboardFeedbackService: Feedback.StandardFeedbackService {
  override func triggerAudioFeedback(_ feedback: Feedback.Audio) {
    guard feedback != .none else { return }
    MainActor.assumeIsolated { KeyboardClickSounds.play() }
  }
}
