import Foundation
import XCTest
@testable import OpenLessShared

final class InterfacePreferenceContractTests: XCTestCase {
  private func withDefaults(_ body: (UserDefaults) throws -> Void) rethrows {
    let name = "OpenLess.InterfacePreferenceContractTests.\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    defer { defaults.removePersistentDomain(forName: name) }
    try body(defaults)
  }

  func testDefaultsAndInvalidStoredValuesFollowSystem() {
    withDefaults { defaults in
      let store = InterfacePreferenceStore(defaults: defaults)
      XCTAssertEqual(store.load(), SharedInterfacePreferences())
      defaults.set("future", forKey: InterfacePreferenceStore.languageKey)
      defaults.set("future", forKey: InterfacePreferenceStore.themeKey)
      XCTAssertEqual(store.load(), SharedInterfacePreferences())
    }
  }

  func testExistingThemeIsPreservedAndLanguagePersistsIndependently() throws {
    try withDefaults { defaults in
      defaults.set("dark", forKey: "ol.ios.appTheme")
      var recognition = KeyboardPreferences()
      recognition.language = .english
      let before = try SharedContract.encode(recognition)
      defaults.set(before, forKey: SharedContract.preferencesKey)
      let store = InterfacePreferenceStore(defaults: defaults)
      XCTAssertEqual(store.load().theme, .dark)
      store.save(SharedInterfacePreferences(language: .simplifiedChinese, theme: store.load().theme))
      XCTAssertEqual(InterfacePreferenceStore(defaults: defaults).load(),
                     SharedInterfacePreferences(language: .simplifiedChinese, theme: .dark))
      XCTAssertEqual(defaults.data(forKey: SharedContract.preferencesKey), before)
      XCTAssertEqual(try SharedContract.decode(KeyboardPreferences.self, from: before).language, .english)
    }
  }

  func testNewWireRecordIsBoundedAndBackwardCompatible() throws {
    let cases = ["{}", "{\"theme\":\"unknown\",\"language\":\"fr\"}",
                 "{\"version\":2,\"theme\":\"dark\",\"language\":\"en\"}"]
    for json in cases {
      XCTAssertEqual(try SharedContract.decode(SharedInterfacePreferences.self, from: Data(json.utf8)),
                     SharedInterfacePreferences())
    }
    let oldWire = Data("{\"version\":1,\"appearance\":\"voiceFirst\",\"language\":\"chinese\",\"spaceHold\":\"cursor\",\"haptics\":true}".utf8)
    XCTAssertEqual(try SharedContract.decode(KeyboardPreferences.self, from: oldWire), KeyboardPreferences())
    let original = SharedInterfacePreferences(language: .english, theme: .light)
    XCTAssertEqual(try SharedContract.decode(SharedInterfacePreferences.self, from: SharedContract.encode(original)), original)
    XCTAssertLessThan(try SharedContract.encode(original).count, 128)
    XCTAssertNotEqual(SharedInterfacePreferences.mailboxKey, SharedContract.preferencesKey)
  }

  func testKeyboardFallbackDoesNotReuseCachedPreferenceWithoutFullAccess() {
    let chosen = SharedInterfacePreferences(language: .english, theme: .dark)
    XCTAssertEqual(SharedInterfacePreferences.keyboardPreferences(shared: chosen, hasFullAccess: true), chosen)
    XCTAssertEqual(SharedInterfacePreferences.keyboardPreferences(shared: chosen, hasFullAccess: false), SharedInterfacePreferences())
    XCTAssertEqual(SharedInterfacePreferences.keyboardPreferences(shared: nil, hasFullAccess: true), SharedInterfacePreferences())
  }

  func testMailboxMissingCorruptFutureAndDeniedRecordsFallBackToSystem() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let mailbox = try Mailbox(url: directory.appendingPathComponent("interface.sqlite"))
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: true), SharedInterfacePreferences())
    let chosen = SharedInterfacePreferences(language: .simplifiedChinese, theme: .light)
    try mailbox.set(chosen, for: SharedInterfacePreferences.mailboxKey)
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: true), chosen)
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: false), SharedInterfacePreferences())
    try mailbox.set("bad record", for: SharedInterfacePreferences.mailboxKey)
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: true), SharedInterfacePreferences())
    try mailbox.set(["version": 99], for: SharedInterfacePreferences.mailboxKey)
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: true), SharedInterfacePreferences())
    XCTAssertNil(try mailbox.value(KeyboardPreferences.self, for: SharedContract.preferencesKey))
  }

  func testLanguageLocaleAndSupportedResourceResolutionAreIndependentOfASR() {
    XCTAssertEqual(InterfaceLanguage.simplifiedChinese.locale.identifier, "zh-Hans")
    XCTAssertEqual(InterfaceLanguage.english.locale.identifier, "en")
    XCTAssertEqual(InterfaceLanguage.system.resourceLanguage(preferredLanguages: ["de-DE", "en-GB"]), "en")
    XCTAssertEqual(InterfaceLanguage.system.resourceLanguage(preferredLanguages: ["zh-CN", "en-US"]), "zh-Hans")
    XCTAssertEqual(InterfaceLanguage.system.resourceLanguage(preferredLanguages: ["fr-FR"]), "en")
    XCTAssertEqual(InterfaceLanguage.english.resourceLanguage(preferredLanguages: ["zh-CN"]), "en")
    XCTAssertEqual(KeyboardLanguage.chinese.locale, "zh-CN")
  }
}
