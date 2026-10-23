import Foundation

/// Presentation only. Never use this value to construct ASR or translation requests.
public enum InterfaceLanguage: String, CaseIterable, Codable, Sendable {
  case system, simplifiedChinese = "zh-Hans", english = "en"

  public var locale: Locale {
    switch self {
    case .system: .autoupdatingCurrent
    case .simplifiedChinese: Locale(identifier: "zh-Hans")
    case .english: Locale(identifier: "en")
    }
  }

  public func resourceLanguage(preferredLanguages: [String] = Locale.preferredLanguages) -> String {
    guard self == .system else { return rawValue }
    // Match only the two shipped UI languages, not the recognition language catalog.
    for identifier in preferredLanguages {
      if identifier.lowercased().hasPrefix("zh") { return "zh-Hans" }
      if identifier.lowercased().hasPrefix("en") { return "en" }
    }
    return "en"
  }
}

/// Same raw values as the existing ol.ios.appTheme preference.
public enum InterfaceTheme: String, CaseIterable, Codable, Sendable {
  case system, light, dark
}

/// Separate, bounded mailbox record: old KeyboardPreferences readers remain unchanged.
public struct SharedInterfacePreferences: Codable, Equatable, Sendable {
  public static let mailboxKey = "interface.preferences.v1"
  public let version: Int
  public let language: InterfaceLanguage
  public let theme: InterfaceTheme

  public init(language: InterfaceLanguage = .system, theme: InterfaceTheme = .system) {
    version = 1
    self.language = language
    self.theme = theme
  }

  private enum CodingKeys: String, CodingKey { case version, language, theme }
  public init(from decoder: any Decoder) throws {
    let values = try decoder.container(keyedBy: CodingKeys.self)
    let version = try values.decodeIfPresent(Int.self, forKey: .version) ?? 1
    guard version == 1 else {
      self.init()
      return
    }
    self.init(
      language: InterfaceLanguage(rawValue: try values.decodeIfPresent(String.self, forKey: .language) ?? "") ?? .system,
      theme: InterfaceTheme(rawValue: try values.decodeIfPresent(String.self, forKey: .theme) ?? "") ?? .system)
  }

  public static func keyboardPreferences(
    shared: SharedInterfacePreferences?, hasFullAccess: Bool
  ) -> SharedInterfacePreferences {
    hasFullAccess ? (shared ?? Self()) : Self()
  }
}

/// App-private storage. Does not read or write keyboard/recognition preferences.
public struct InterfacePreferenceStore {
  public static let languageKey = "ol.ios.interfaceLanguage"
  public static let themeKey = "ol.ios.appTheme"
  private let defaults: UserDefaults

  public init(defaults: UserDefaults) { self.defaults = defaults }

  public func load() -> SharedInterfacePreferences {
    SharedInterfacePreferences(
      language: InterfaceLanguage(rawValue: defaults.string(forKey: Self.languageKey) ?? "") ?? .system,
      theme: InterfaceTheme(rawValue: defaults.string(forKey: Self.themeKey) ?? "") ?? .system)
  }

  public func save(_ preferences: SharedInterfacePreferences) {
    defaults.set(preferences.language.rawValue, forKey: Self.languageKey)
    defaults.set(preferences.theme.rawValue, forKey: Self.themeKey)
  }
}

extension Mailbox {
  /// Call only after checking Full Access; denial must not open/read shared storage.
  public func interfacePreferences(hasFullAccess: Bool) -> SharedInterfacePreferences {
    guard hasFullAccess else { return SharedInterfacePreferences() }
    let saved = try? value(SharedInterfacePreferences.self, for: SharedInterfacePreferences.mailboxKey)
    return SharedInterfacePreferences.keyboardPreferences(shared: saved, hasFullAccess: true)
  }
}
