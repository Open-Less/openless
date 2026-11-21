import Foundation

public enum KeyboardSizing {
  // Key rows and toolbar dimensions are independent of the system safe area.
  public static let defaultBodyHeight = 208.0
  public static let singleKeyboardBodyHeight = 208.0
  public static let scaleRange = 0.9...1.2
  public static func validScale(_ value: Double) -> Double {
    value.isFinite ? min(scaleRange.upperBound, max(scaleRange.lowerBound, value)) : 1
  }
  public static func bodyHeight(scale: Double, landscape: Bool = false,
    experience: KeyboardExperience = .voiceFocused) -> Double {
    let portrait = experience == .singleKeyboard ? singleKeyboardBodyHeight : defaultBodyHeight
    return (landscape ? 160 : portrait) * validScale(scale)
  }
}

/// Keyboard-only preference. Keep it independent of host preferences so a
/// delayed host settings snapshot cannot overwrite the user's latest style.
public enum KeyboardStyleSelection {
  public static let key = "ol.keyboard.selectedStyleID"
  public static func load(from defaults: UserDefaults = .standard) -> String? {
    normalized(defaults.string(forKey: key))
  }
  public static func save(_ id: String?, to defaults: UserDefaults = .standard) {
    if let id = normalized(id) { defaults.set(id, forKey: key) }
    else { defaults.removeObject(forKey: key) }
  }
  private static func normalized(_ id: String?) -> String? {
    guard let id, !id.isEmpty, id != "builtin.raw" else { return nil }
    return id
  }
}

public enum VoiceWaveform {
  /// Convert real RMS to a speech-visible range. Below -55 dB remains silence;
  /// there is no synthetic oscillation when microphone samples are absent.
  public static func amplitude(rms: Float) -> Double {
    guard rms.isFinite, rms > 0 else { return 0 }
    let decibels = 20 * log10(Double(min(1, rms)))
    return pow(min(1, max(0, (decibels + 55) / 40)), 0.75)
  }
}
