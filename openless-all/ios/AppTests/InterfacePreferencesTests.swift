import Combine
import Foundation
import OpenLessShared
import XCTest
@testable import OpenLess

nonisolated final class InterfacePreferencesTests: XCTestCase {
  @MainActor func testLanguageAppliesImmediatelyAndPersistsWithoutChangingRecognition() throws {
    let name = "OpenLess.InterfacePreferencesTests.\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    defer { defaults.removePersistentDomain(forName: name) }
    let recognition = try SharedContract.encode(KeyboardPreferences())
    defaults.set(recognition, forKey: SharedContract.preferencesKey)
    defaults.set("dark", forKey: "ol.ios.appTheme")
    let model = InterfacePreferences(defaults: defaults)
    var changes = 0
    let subscription = model.objectWillChange.sink { changes += 1 }
    model.language = .english
    XCTAssertGreaterThan(changes, 0)
    XCTAssertEqual(model.locale.identifier, "en")
    XCTAssertEqual(model.theme, .dark)
    XCTAssertEqual(defaults.data(forKey: SharedContract.preferencesKey), recognition)
    XCTAssertEqual(InterfacePreferences(defaults: defaults).language, .english)
    withExtendedLifetime(subscription) {}
  }

  @MainActor func testNewMailboxRecordPublishesOnConnectAndEachChange() throws {
    let name = "OpenLess.InterfacePreferencesTests.\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer {
      defaults.removePersistentDomain(forName: name)
      try? FileManager.default.removeItem(at: directory)
    }
    let model = InterfacePreferences(defaults: defaults)
    let mailbox = try Mailbox(url: directory.appendingPathComponent("preferences.sqlite"))
    var notifications = 0
    model.connect(mailbox: mailbox) { notifications += 1 }
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: true), SharedInterfacePreferences())
    model.theme = .light
    model.language = .simplifiedChinese
    XCTAssertEqual(notifications, 3)
    XCTAssertFalse(model.sharingFailed)
    XCTAssertEqual(mailbox.interfacePreferences(hasFullAccess: true), model.value)
    XCTAssertNil(try mailbox.value(KeyboardPreferences.self, for: SharedContract.preferencesKey))
  }

  @MainActor func testReloadReadsLegacyAppThemeAndSystemSelection() {
    let name = "OpenLess.InterfacePreferencesTests.\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    defer { defaults.removePersistentDomain(forName: name) }
    let model = InterfacePreferences(defaults: defaults)
    XCTAssertEqual(model.value, SharedInterfacePreferences())
    model.language = .english
    defaults.set("dark", forKey: "ol.ios.appTheme")
    model.reload()
    XCTAssertEqual(model.theme, .dark)
    XCTAssertEqual(model.language, .english)
    model.language = .system
    XCTAssertEqual(InterfacePreferences(defaults: defaults).language, .system)
    XCTAssertEqual(InterfacePreferences(defaults: defaults).theme, .dark)
  }
}
