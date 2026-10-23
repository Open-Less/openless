import XCTest

/// Screenshot tour used by the iOS UI redesign. It only navigates and captures;
/// it never changes user data. Output directory/prefix come from
/// TEST_RUNNER_AUDIT_DIR / TEST_RUNNER_AUDIT_PREFIX (set by xcodebuild).
@MainActor
final class UIAuditTests: XCTestCase {
  private var dir: String { ProcessInfo.processInfo.environment["AUDIT_DIR"] ?? "/tmp/openless-audit" }
  private var prefix: String { ProcessInfo.processInfo.environment["AUDIT_PREFIX"] ?? "audit" }
  override func setUpWithError() throws {
    continueAfterFailure = true
    XCUIDevice.shared.orientation = .portrait
    try? FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
  }
  private func shot(_ name: String, wait: TimeInterval = 0.8) {
    Thread.sleep(forTimeInterval: wait)
    let png = XCUIScreen.main.screenshot().pngRepresentation
    try? png.write(to: URL(fileURLWithPath: "\(dir)/\(prefix)-\(name).png"))
  }
  private func dockTab(_ app: XCUIApplication, _ title: String) { app.tabBars["primaryDock"].buttons[title].tap() }

  func testTour() {
    let app = XCUIApplication()
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
    shot("home", wait: 2)
    app.swipeUp(); shot("home-scrolled")
    app.swipeUp(); shot("home-bottom")
    app.swipeDown(); app.swipeDown()
    if app.buttons["keyboardSetup"].exists { app.buttons["keyboardSetup"].tap(); shot("keyboard-install"); app.navigationBars.buttons.firstMatch.tap() }
    if app.buttons["openKeyboardTrial"].exists || { app.swipeUp(); return app.buttons["openKeyboardTrial"].exists }() {
      app.buttons["openKeyboardTrial"].tap(); shot("keyboard-trial")
      app.navigationBars.buttons.firstMatch.tap()
    }
    dockTab(app, "历史"); shot("history")
    dockTab(app, "词典"); shot("dictionary")
    dockTab(app, "设置"); shot("settings")
    app.swipeUp(); shot("settings-scrolled")
    app.swipeDown()
    if app.buttons["openAccountSettings"].exists { app.buttons["openAccountSettings"].tap(); shot("settings-account"); app.navigationBars.buttons.firstMatch.tap() }
    if app.buttons["openInterfacePreferences"].exists { app.buttons["openInterfacePreferences"].tap(); shot("settings-interface"); app.navigationBars.buttons.firstMatch.tap() }
    if app.buttons["openHelp"].exists || { app.swipeUp(); return app.buttons["openHelp"].exists }() {
      app.buttons["openHelp"].tap(); shot("settings-help"); app.navigationBars.buttons.firstMatch.tap()
    }
    app.tabBars["styleDock"].buttons["dockStyles"].tap()
    shot("styles-library", wait: 1.5)
    let detail = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "styleDetail-")).firstMatch
    if detail.exists { detail.tap(); shot("style-detail"); app.navigationBars.buttons.firstMatch.tap() }
    app.tabBars["styleDock"].buttons["dockStyle-market"].tap(); shot("styles-market", wait: 2)
    app.tabBars["styleDock"].buttons["dockStyle-translation"].tap(); shot("styles-translation", wait: 1.2)
    if app.buttons["addTranslationTarget"].exists { app.buttons["addTranslationTarget"].tap(); shot("translation-picker"); }
  }

  // MARK: - Keyboard tour (copied helpers from OpenLessUITests)
  private func launch() -> XCUIApplication {
    let app = XCUIApplication()
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
    return app
  }
  private func show(_ element: XCUIElement, in app: XCUIApplication, attempts: Int = 8) {
    for _ in 0..<attempts {
      if element.exists && element.isHittable {
        let frame = element.frame
        let tabBar = app.tabBars["primaryDock"]
        let bottom = tabBar.exists ? tabBar.frame.minY - 8 : app.frame.maxY - 8
        if frame.minY >= 95 && frame.maxY <= bottom { return }
      }
      app.swipeUp()
    }
    XCTAssertTrue(element.exists, "Missing element after scrolling: \(element)\n\(app.debugDescription)")
  }
  private func capture(_ app: XCUIApplication, name: String) {
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 3))
    // App-scoped screenshots in the iOS 27 simulator can be cropped using
    // stale portrait bounds after rotation. Capture the actual display.
    shot(name)
  }
  private func expectValue(_ value: String, on element: XCUIElement) {
    let update = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", value), object: element)
    XCTAssertEqual(XCTWaiter.wait(for: [update], timeout: 3), .completed, element.debugDescription)
  }
  private func openEditor(_ app: XCUIApplication) -> XCUIElement {
    let link = app.buttons["openKeyboardTrial"]
    show(link, in: app)
    link.tap()
    XCTAssertTrue(app.navigationBars["试用键盘"].waitForExistence(timeout: 5))
    let editor = app.textViews["keyboardTestEditor"]
    XCTAssertTrue(editor.waitForExistence(timeout: 5))
    editor.tap()
    return editor
  }
  private func selectOpenLess(_ app: XCUIApplication) {
    if app.buttons["keyboardModePinyin"].waitForExistence(timeout: 3) { return }
    for attempt in 0..<4 {
      let cont = app.buttons.matching(NSPredicate(format: "label IN %@", ["继续", "Continue"])).firstMatch
      if cont.exists { cont.tap(); Thread.sleep(forTimeInterval: 0.6) }
      let globe = app.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.965))
      globe.press(forDuration: 1.3)
      shot("kb-globe-menu-\(attempt)", wait: 0.5)
      let item = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR label BEGINSWITH %@", "OpenLess", "OpenLess")).firstMatch
      if item.waitForExistence(timeout: 2) { item.tap() }
      if app.buttons["keyboardModePinyin"].waitForExistence(timeout: 4) { return }
    }
    XCTFail("OpenLess keyboard not reachable")
  }
  private func installKeyboard(in app: XCUIApplication, fullAccess: Bool = false) {
    let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
    settings.launch()
    let add = settings.buttons.matching(NSPredicate(format: "label BEGINSWITH %@ OR label BEGINSWITH %@", "添加新键盘", "Add New Keyboard")).firstMatch
    if !add.exists {
      for _ in 0..<6 {
        if settings.navigationBars["设置"].exists || settings.navigationBars["Settings"].exists { break }
        settings.navigationBars.buttons.firstMatch.tap()
      }
      for _ in 0..<4 { settings.swipeDown() }
      for labels in [["通用", "General"], ["键盘", "Keyboard"], ["键盘", "Keyboards"]] {
        let destination = settings.navigationBars.matching(NSPredicate(format: "identifier IN %@ OR label IN %@", labels, labels)).firstMatch
        for attempt in 0..<3 {
          let chinese = settings.cells.containing(.staticText, identifier: labels[0]).firstMatch
          let english = settings.cells.containing(.staticText, identifier: labels[1]).firstMatch
          let row = chinese.exists ? chinese : english
          if !row.exists || !row.isHittable { show(row, in: settings) }
          row.tap()
          if destination.waitForExistence(timeout: 4) { break }
          if attempt < 2 { settings.navigationBars.buttons.firstMatch.tap() }
        }
        XCTAssertTrue(destination.exists, settings.debugDescription)
      }
    }
    let installed = settings.staticTexts.matching(NSPredicate(format: "label BEGINSWITH %@", "OpenLess")).firstMatch
    if !installed.exists {
      XCTAssertTrue(add.waitForExistence(timeout: 5))
      add.tap()
      let openless = settings.staticTexts["OpenLess"].firstMatch
      show(openless, in: settings)
      openless.tap()
    }
    // Normalize persisted simulator preferences, not a physical device. A
    // previous manual run must not silently turn this into a Full Access test.
    if !add.exists { settings.navigationBars.buttons.firstMatch.tap() }
    let keyboard = settings.cells.containing(.staticText, identifier: "OpenLess").firstMatch
    let prefix = settings.cells.containing(.staticText, identifier: "OpenLess — OpenLess").firstMatch
    let row = keyboard.exists ? keyboard : prefix
    XCTAssertTrue(row.waitForExistence(timeout: 5), settings.debugDescription)
    row.tap()
    let access = settings.switches.firstMatch
    XCTAssertTrue(access.waitForExistence(timeout: 5), settings.debugDescription)
    let desiredAccess = fullAccess ? "1" : "0"
    if access.value as? String != desiredAccess {
      // Preferences exposes the whole labelled row as a switch in iOS 27;
      // its centre is the label, not the actual toggle at the trailing edge.
      access.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
      if fullAccess, settings.alerts.firstMatch.waitForExistence(timeout: 2) {
        let allow = settings.alerts.buttons.matching(NSPredicate(format: "label IN %@", ["允许", "Allow"])).firstMatch
        XCTAssertTrue(allow.exists, settings.debugDescription)
        allow.tap()
      }
      let update = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", desiredAccess), object: access)
      XCTAssertEqual(XCTWaiter.wait(for: [update], timeout: 4), .completed)
    }
    XCTAssertEqual(access.value as? String, desiredAccess)
    _ = 0
    app.terminate()
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
  }
  func testKeyboardTour() {
    let app = launch()
    // keyboard already enabled in the audit simulator
    _ = openEditor(app)
    shot("kb-editor-focused", wait: 2)
    selectOpenLess(app)
    app.buttons["keyboardModeVoice"].tap(); shot("kb-voice", wait: 1.2)
    app.buttons["keyboardModePinyin"].tap(); shot("kb-pinyin", wait: 1.0)
    for letter in ["n", "i", "h", "a", "o"] { app.buttons[letter].firstMatch.tap() }
    shot("kb-pinyin-candidates", wait: 1.0)
    app.buttons["你好"].tap()
    app.buttons["keyboardModeEnglish"].tap(); shot("kb-english", wait: 1.0)
    app.buttons["keyboardModeVoice"].tap()
    let speak = app.buttons["speakButton"].firstMatch
    if speak.exists { speak.tap(); shot("kb-voice-tapped", wait: 1.5) }
  }

  func testProbeKeyboardSettings() {
    let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
    settings.launch()
    shot("probe-0", wait: 1.5)
    for labels in [["通用", "General"], ["键盘", "Keyboard"], ["键盘", "Keyboards"]] {
      let cell = settings.cells.containing(.staticText, identifier: labels[0]).firstMatch
      let en = settings.cells.containing(.staticText, identifier: labels[1]).firstMatch
      let row = cell.exists ? cell : en
      if !row.exists { for _ in 0..<4 { settings.swipeUp() } }
      if row.exists { row.tap() }
      shot("probe-\(labels[1])", wait: 1.2)
    }
  }
}
