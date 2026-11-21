import Foundation

/// Explicit lookup for UIKit and runtime String values, which SwiftUI does not
/// localize automatically. Pass the app/extension bundle, not Bundle.module.
public struct InterfaceLocalizer {
  public let language: InterfaceLanguage
  private let bundle: Bundle
  private let preferredLanguages: [String]

  public init(language: InterfaceLanguage, bundle: Bundle = .main,
              preferredLanguages: [String] = Locale.preferredLanguages) {
    self.language = language
    self.bundle = bundle
    self.preferredLanguages = preferredLanguages
  }

  public func text(_ key: String) -> String {
    let resource = language.resourceLanguage(preferredLanguages: preferredLanguages)
    guard let path = bundle.path(forResource: resource, ofType: "lproj"),
          let localized = Bundle(path: path) else { return key }
    return localized.localizedString(forKey: key, value: key, table: "Localizable")
  }

  private func format(_ key: String, _ arguments: CVarArg...) -> String {
    String(format: text(key), locale: language.locale, arguments: arguments)
  }

  public func recordCount(_ count: Int) -> String {
    format(count == 1 ? "interface.record.singular.format" : "interface.records.format", count)
  }
  public func recordingSecondsRemaining(_ seconds: Int) -> String {
    format("interface.recording.remaining.format", seconds)
  }
  public func deviceLoginTiming(remaining: Int, interval: Int) -> String {
    format("interface.account.timing.format", remaining, interval)
  }
  public func marketplaceCounts(downloads: Int, likes: Int) -> String {
    format("interface.marketplace.counts.format", downloads, likes)
  }
  public func capabilityStatus(_ status: String) -> String { format("interface.capability.format", status) }
  public func sharedStorageError(_ detail: String) -> String { format("interface.storage.error.format", detail) }
  public func repolishStyle(_ style: String) -> String { format("interface.repolish.format", style) }
  public func copyTitle(_ title: String) -> String { format("interface.copy.format", title) }
  public func temporaryArchiveCleanupError(_ detail: String) -> String {
    format("interface.archive.cleanup.format", detail)
  }
  public func asrDescription(_ provider: String, developerMode: Bool) -> String {
    format(developerMode ? "interface.asr.developer.format" : "interface.asr.local.format", provider)
  }
  public func activityTrendNote(duration: Bool) -> String {
    text(duration ? "interface.trend.duration" : "interface.trend.standard")
  }
  public func duration(seconds: Int) -> String {
    let value = max(0, seconds)
    if value >= 3600 { return format("interface.duration.hoursMinutes.format", value / 3600, value % 3600 / 60) }
    if value >= 60 { return format("interface.duration.minutesSeconds.format", value / 60, value % 60) }
    return format("interface.duration.seconds.format", value)
  }
}
