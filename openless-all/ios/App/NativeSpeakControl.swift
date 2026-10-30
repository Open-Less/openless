import OpenLessShared
import SwiftUI
import UIKit

struct NativeSpeakControl: UIViewRepresentable {
  @Environment(\.locale) private var locale
  let phase: VoicePhase
  let level: Float
  let enabled: Bool
  let onAction: (VoiceGesture.Action) -> Void
  func makeUIView(context: Context) -> SpeakButton {
    let button = SpeakButton()
    button.onAction = onAction
    return button
  }
  func updateUIView(_ view: SpeakButton, context: Context) {
    view.onAction = onAction
    view.interfaceLanguage = locale.identifier.hasPrefix("en") ? .english
      : locale.identifier.hasPrefix("zh") ? .simplifiedChinese : .system
    view.configure(phase: phase, level: level, available: enabled)
  }
}
