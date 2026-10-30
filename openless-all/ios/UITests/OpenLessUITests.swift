import XCTest
import OpenLessShared
import CoreFoundation

@MainActor
final class OpenLessUITests: XCTestCase {
  override func setUpWithError() throws {
    continueAfterFailure = false
    XCUIDevice.shared.orientation = .portrait
  }

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
    let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
  private func expectValue(_ value: String, on element: XCUIElement) {
    let update = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", value), object: element)
    // Host text and keyboard actions cross process boundaries. This verifies
    // the eventual edit, not simulator responsiveness under competing builds.
    XCTAssertEqual(XCTWaiter.wait(for: [update], timeout: 30), .completed, element.debugDescription)
  }
  private func typePinyin(_ text: String, in app: XCUIApplication, startingWith prefix: String = "") {
    var original = prefix
    for letter in text {
      let key = app.buttons["keyboardKey-" + String(letter)]
      XCTAssertTrue(key.waitForExistence(timeout: 3))
      key.tap()
      original.append(letter)
      let candidates = app.scrollViews["keyboardCandidates"]
      let updated = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == 1 AND value == %@", original), object: candidates)
      XCTAssertEqual(XCTWaiter.wait(for: [updated], timeout: 3), .completed,
        "The single key tap must update composition before the next input. Expected: " + original)
    }
  }

  private func restoreKeyboardToolbar(in app: XCUIApplication) {
    let close = app.buttons["dismissKeyboardPredictions"]
    if close.exists && close.isHittable {
      XCTAssertGreaterThanOrEqual(close.frame.width, 44)
      XCTAssertGreaterThanOrEqual(close.frame.height, 44)
      close.tap()
    }
  }
  private func selectKeyboardMode(_ mode: String, in app: XCUIApplication) {
    restoreKeyboardToolbar(in: app)
    app.buttons[mode].tap()
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
  private func keyboardModeEntry(in app: XCUIApplication) -> XCUIElement {
    app.buttons.matching(NSPredicate(format: "identifier IN %@",
      ["keyboardModePinyin", "keyboardLanguageToggle"])).firstMatch
  }
  private func selectOpenLess(_ app: XCUIApplication) {
    if keyboardModeEntry(in: app).waitForExistence(timeout: 3) { return }
    // iOS can cycle only recently used keyboards on a short tap. Prefer the
    // explicit input-method menu; retain cycling for runtimes that dismiss the
    // menu when a synthetic long press lifts.
    for _ in 0..<6 {
      // Fresh simulators show either the QuickPath or keyboard-switching
      // tutorial. The latter may stay English even in a Chinese test run.
      let tutorial = app.staticTexts.matching(NSPredicate(format:
        "label IN %@", ["滑动手指将字母拼成词以快速键入。", "Quickly Change Keyboards",
                       "快速切换键盘", "Slide your finger across the letters to type."])).firstMatch
      if tutorial.exists {
        app.buttons.matching(NSPredicate(format: "label IN %@", ["继续", "Continue"])).firstMatch.tap()
      }
      let next = app.buttons.matching(NSPredicate(format: "label IN %@", ["Next keyboard", "下一个键盘", "下一键盘", "切换输入法"])).firstMatch
      guard next.waitForExistence(timeout: 5) else {
        capture(app, name: "keyboard-not-visible-after-editor-focus")
        XCTFail(app.debugDescription); return
      }
      next.press(forDuration: 1)
      let openless = app.descendants(matching: .any).matching(NSPredicate(
        format: "label IN %@", ["OpenLess", "OpenLess — OpenLess"])).firstMatch
      if openless.waitForExistence(timeout: 1), openless.isHittable {
        openless.tap()
        if keyboardModeEntry(in: app).waitForExistence(timeout: 8) {
          capture(app, name: "selected-extension-surface")
          return
        }
      }
      next.tap()
      if keyboardModeEntry(in: app).waitForExistence(timeout: 5) {
        capture(app, name: "selected-extension-surface")
        return
      }
    }
    XCTFail(app.debugDescription)
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
    // The freshly installed extension first appears under the containing app's
    // identifier. Wait for the actual extension row before opening its settings.
    let keyboard = settings.cells["app.openless.ios.keyboard"]
    if !keyboard.exists {
      let addPage = settings.navigationBars.matching(NSPredicate(format:
        "identifier IN %@ OR label IN %@", ["添加新键盘", "Add New Keyboard"], ["添加新键盘", "Add New Keyboard"])).firstMatch
      if !addPage.exists {
        XCTAssertTrue(add.waitForExistence(timeout: 8))
        add.tap()
      }
      let installRow = settings.cells["app.openless.ios"]
      for _ in 0..<3 {
        guard !keyboard.exists else { break }
        for _ in 0..<10 {
          if installRow.exists && installRow.isHittable && installRow.frame.midY < settings.frame.maxY - 160 { break }
          settings.swipeUp()
        }
        XCTAssertTrue(installRow.exists && installRow.isHittable, settings.debugDescription)
        installRow.tap()
        if keyboard.waitForExistence(timeout: 12) { break }
        guard addPage.exists else { break }
      }
    }
    // Normalize persisted simulator preferences, not a physical device. A
    // previous manual run must not silently turn this into a Full Access test.
    XCTAssertTrue(keyboard.waitForExistence(timeout: 8), settings.debugDescription)
    let ready = XCTNSPredicateExpectation(
      predicate: NSPredicate(format: "hittable == true AND enabled == true"), object: keyboard)
    XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 5), .completed, settings.debugDescription)
    let access = settings.switches.firstMatch
    for _ in 0..<3 {
      keyboard.tap()
      if access.waitForExistence(timeout: 8) { break }
      // A freshly registered extension can leave Preferences on this list
      // after the first tap. Retry only this idempotent navigation, never input.
      guard add.exists, keyboard.exists, keyboard.isHittable else { break }
    }
    XCTAssertTrue(access.exists, settings.debugDescription)
    let desiredAccess = fullAccess ? "1" : "0"
    if access.value as? String != desiredAccess {
      // Prefer the nested switch over the enclosing accessibility row.
      if let toggle = settings.switches.allElementsBoundByIndex.first(where: { $0.frame.width < 100 && $0.isHittable }) {
        toggle.tap()
      } else { access.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap() }
      if fullAccess, settings.alerts.firstMatch.waitForExistence(timeout: 2) {
        let allow = settings.alerts.buttons.matching(NSPredicate(format: "label IN %@", ["允许", "Allow"])).firstMatch
        XCTAssertTrue(allow.exists, settings.debugDescription)
        allow.tap()
      }
      let update = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", desiredAccess), object: access)
      XCTAssertEqual(XCTWaiter.wait(for: [update], timeout: 8), .completed, settings.debugDescription)
    }
    XCTAssertEqual(access.value as? String, desiredAccess)
    capture(settings, name: fullAccess ? "keyboard-full-access-on" : "keyboard-full-access-off")
    app.terminate()
    app.launch()
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
  }

  func testDeveloperModeReloadStartsWithoutLLMAndCanExit() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let version = app.buttons["appVersion"]
    show(version, in: app)
    version.tap(); version.tap(); version.tap()
    let unlock = app.alerts["开启开发者模式？"]
    XCTAssertTrue(unlock.waitForExistence(timeout: 3))
    unlock.buttons["重载并开启"].tap()
    XCTAssertTrue(app.buttons["prepareSpeech"].waitForExistence(timeout: 10))
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let developer = app.buttons["openDeveloperSettings"]
    show(developer, in: app)
    developer.tap()
    let pipeline = app.segmentedControls["pipelinePicker"]
    XCTAssertTrue(pipeline.waitForExistence(timeout: 10), app.debugDescription)
    XCTAssertEqual(app.switches["enablePolish"].value as? String, "0")
    XCTAssertTrue(app.buttons["addProvider-asr"].exists)
    XCTAssertTrue(app.buttons["addProvider-llm"].exists)
    XCTAssertFalse(app.staticTexts["developerError"].exists, app.debugDescription)
    capture(app, name: "developer-raw-defaults-real-core")
    app.buttons["savePipeline"].tap()
    let saved = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: app.buttons["savePipeline"])
    XCTAssertEqual(XCTWaiter.wait(for: [saved], timeout: 5), .completed)
    XCTAssertFalse(app.staticTexts["developerError"].exists, app.debugDescription)
    let exit = app.buttons["exitDeveloperMode"]
    show(exit, in: app)
    exit.tap()
    XCTAssertTrue(app.buttons["prepareSpeech"].waitForExistence(timeout: 10))
    app.tabBars["primaryDock"].buttons["设置"].tap()
    XCTAssertFalse(app.buttons["openDeveloperSettings"].exists)
    capture(app, name: "developer-exit-reloaded-standard-mode")
  }

  func testFirstLaunchShowsHonestCapabilityState() {
    let app = launch()
    XCTAssertTrue(app.staticTexts["OpenLess"].waitForExistence(timeout: 10))
    capture(app, name: "home-redesigned")
    let prepare = app.buttons["prepareSpeech"]
    show(prepare, in: app)
    XCTAssertTrue(prepare.isEnabled, "Real Core must initialize before preparation")
    show(app.staticTexts["keyboardVoiceBoundary"], in: app)
    app.tabBars["primaryDock"].buttons["设置"].tap()
    XCTAssertTrue(app.navigationBars["设置"].waitForExistence(timeout: 5))
    XCTAssertFalse(app.buttons["收集接口诊断（不启动小窗）"].exists, "Diagnostics belong on their own page")
    capture(app, name: "settings-redesigned")
  }
  func testHomeAndSettingsTextClippingAudit() throws {
    guard #available(iOS 17, *) else { throw XCTSkip("System accessibility audit requires iOS 17; layout remains native on iOS 16") }
    let app = launch()
    let prepare = app.buttons["prepareSpeech"]
    show(prepare, in: app)
    try app.performAccessibilityAudit(for: .textClipped)
    capture(app, name: "home-preparation-readable")
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let picker = app.buttons["appearancePicker"]
    show(picker, in: app)
    try app.performAccessibilityAudit(for: .textClipped)
    capture(app, name: "settings-values-readable")
  }

  func testHomeScrollingDoesNotPrepareOrRecord() {
    let app = launch()
    let prepare = app.buttons["prepareSpeech"]
    XCTAssertTrue(prepare.waitForExistence(timeout: 5))
    app.scrollViews.firstMatch.swipeUp()
    app.scrollViews.firstMatch.swipeDown()
    XCTAssertEqual(app.alerts.count, 0, "Scrolling must not request speech resources")
    XCTAssertTrue(prepare.isEnabled, "A scroll cannot begin preparation")
    XCTAssertFalse(app.staticTexts["正在聆听"].exists)
  }

  func testHistoryManagementRequiresConfirmation() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["历史"].tap()
    XCTAssertTrue(app.navigationBars["历史"].waitForExistence(timeout: 5))
    let first = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "historyEntry-")).firstMatch
    let empty = app.staticTexts["还没有识别记录"]
    let ready = NSPredicate { _, _ in empty.exists || first.exists }
    XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: ready, object: app)], timeout: 10), .completed)
    let hadRecords = first.exists
    app.buttons["historyActions"].tap()
    let clear = app.buttons["清空历史"]
    XCTAssertTrue(clear.waitForExistence(timeout: 3))
    XCTAssertEqual(clear.isEnabled, hadRecords)
    if hadRecords {
      clear.tap()
      XCTAssertTrue(app.staticTexts["清空本机识别历史及其录音？"].waitForExistence(timeout: 3))
      app.buttons["取消"].tap()
      XCTAssertTrue(first.exists, "Cancelling must preserve actual history")
    }
  }
  func testProductLibraryNavigationDoesNotMutateUserData() {
    let app = launch()
    XCTAssertTrue(app.tabBars["primaryDock"].buttons["概览"].exists)
    let records = app.staticTexts["overviewRecords"]
    show(records, in: app)
    XCTAssertTrue(records.exists)
    let period = app.segmentedControls["overviewPeriod"]
    show(period, in: app)
    period.buttons["近 30 天"].tap()
    XCTAssertTrue(app.staticTexts["overviewPeriodTotal"].exists)
    capture(app, name: "product-library-overview-30-days")
    app.tabBars["primaryDock"].buttons["历史"].tap()
    let menu = app.buttons["historyActions"]
    XCTAssertTrue(menu.waitForExistence(timeout: 5))
    menu.tap()
    XCTAssertTrue(app.buttons["刷新"].waitForExistence(timeout: 3))
    app.buttons["刷新"].tap()
    app.tabBars["primaryDock"].buttons["词典"].tap()
    let add = app.buttons["addVocabulary"]
    XCTAssertTrue(add.waitForExistence(timeout: 5))
    add.tap()
    XCTAssertTrue(app.textFields["vocabularyPhrase"].waitForExistence(timeout: 3))
    capture(app, name: "product-library-add-word-no-keyboard-required")
    app.buttons["取消"].tap()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let theme = app.buttons["openInterfacePreferences"]
    show(theme, in: app)
    XCTAssertTrue(theme.exists)
    let help = app.buttons["openHelp"]
    show(help, in: app)
    help.tap()
    XCTAssertTrue(app.navigationBars["帮助中心"].waitForExistence(timeout: 3))
    app.navigationBars.buttons.firstMatch.tap()
    let notes = app.buttons["openReleaseNotes"]
    show(notes, in: app)
    notes.tap()
    XCTAssertTrue(app.navigationBars["版本说明"].waitForExistence(timeout: 3))
    capture(app, name: "product-library-release-notes")
  }

  func testStyleAndAccountPagesExposeActualLocalState() {
    let app = launch()
    app.tabBars["styleDock"].buttons["dockStyles"].tap()
    let sections = app.tabBars["styleDock"].buttons["dockStyle-library"]
    XCTAssertTrue(sections.waitForExistence(timeout: 5))
    app.tabBars["styleDock"].buttons["dockStyle-library"].tap()
    XCTAssertTrue(app.navigationBars["我的风格"].waitForExistence(timeout: 5))
    XCTAssertTrue(app.buttons["styleDetail-builtin.raw"].waitForExistence(timeout: 10))
    XCTAssertFalse(app.staticTexts["stylesError"].exists)
    capture(app, name: "style-library-local-state")
    app.tabBars["styleDock"].buttons["dockStyle-market"].tap()
    XCTAssertTrue(app.navigationBars["风格市场"].waitForExistence(timeout: 5))
    XCTAssertTrue(app.textFields["styleMarketQuery"].exists)
    XCTAssertFalse(app.buttons["styleMarketSearch"].exists)
    app.tabBars["styleDock"].buttons["dockStyle-translation"].tap()
    XCTAssertTrue(app.navigationBars["翻译目标"].waitForExistence(timeout: 5))
    let add = app.buttons["addTranslationTarget"]
    XCTAssertTrue(add.exists)
    add.tap()
    XCTAssertTrue(app.navigationBars["添加目标语言"].waitForExistence(timeout: 5))
    app.buttons["取消"].tap()
    app.tabBars["styleDock"].buttons["dockStyle-library"].tap()
    app.tabBars["primaryDock"].buttons["词典"].tap()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let account = app.buttons["openAccountSettings"]
    show(account, in: app)
    account.tap()
    XCTAssertTrue(app.navigationBars["我的主页"].waitForExistence(timeout: 5))
    let ready = NSPredicate { _, _ in
      app.staticTexts["accountSignedOut"].exists || app.staticTexts["accountUnverified"].exists || app.staticTexts["accountVerifiedLogin"].exists
    }
    XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: ready, object: app)], timeout: 10), .completed)
    XCTAssertFalse(app.staticTexts["accountUserCode"].exists, "Opening settings must not begin OAuth")
    XCTAssertFalse(app.staticTexts["accountError"].exists)
    capture(app, name: "account-local-status-no-login-request")
  }

  func testDictionaryReferenceFiltersAndCancelPreserveData() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["词典"].tap()
    for source in ["learned", "manual", "all"] {
      let filter = app.buttons["vocabularySource-\(source)"]
      XCTAssertTrue(filter.waitForExistence(timeout: 5)); filter.tap()
    }
    capture(app, name: "dictionary-video-reference-page")
    app.buttons["addVocabulary"].tap()
    XCTAssertTrue(app.textFields["vocabularyPhrase"].waitForExistence(timeout: 5))
    XCTAssertFalse(app.buttons["saveVocabulary"].isEnabled)
    capture(app, name: "dictionary-compact-modal-empty")
    app.buttons["cancelVocabulary"].tap()
    XCTAssertTrue(app.buttons["addVocabulary"].waitForExistence(timeout: 5))
    XCTAssertFalse(app.textFields["vocabularyPhrase"].exists)
  }

  func testTrialEditorShowsSoftwareKeyboardAfterRefocus() {
    let app = launch()
    let editor = openEditor(app)
    let visible = NSPredicate { _, _ in
      app.keyboards.firstMatch.exists || app.buttons["keyboardModeVoice"].exists
    }
    for _ in 0..<3 {
      let shown = XCTNSPredicateExpectation(predicate: visible, object: app)
      XCTAssertEqual(XCTWaiter.wait(for: [shown], timeout: 6), .completed, app.debugDescription)
      XCTAssertLessThan(editor.frame.height, app.frame.height * 0.65, "The software keyboard must occupy the editor's safe area")
      app.buttons["dismissTrialKeyboard"].tap()
      editor.tap()
    }
    let reopened = XCTNSPredicateExpectation(predicate: visible, object: app)
    XCTAssertEqual(XCTWaiter.wait(for: [reopened], timeout: 6), .completed, app.debugDescription)
    capture(app, name: "software-keyboard-after-restart")
    XCTAssertEqual(app.alerts.count, 0)
  }

  func testTypingDoesNotAskForMicrophone() {
    let app = launch()
    let editor = openEditor(app)
    if app.buttons["keyboardModeEnglish"].exists { selectKeyboardMode("keyboardModeEnglish", in: app) }
    editor.typeText("OpenLess 123")
    XCTAssertTrue((editor.value as? String)?.contains("OpenLess 123") == true)
    XCTAssertEqual(app.alerts.count, 0)
    XCTAssertFalse(app.tabBars["primaryDock"].buttons["设置"].exists, "No tab bar should overlap the editor")
    capture(app, name: "native-editor-no-tab-overlap")
    app.buttons["dismissTrialKeyboard"].tap()
    app.navigationBars.buttons.firstMatch.tap()
    XCTAssertTrue(app.tabBars["primaryDock"].buttons["设置"].waitForExistence(timeout: 5))
    app.tabBars["primaryDock"].buttons["设置"].tap()
    XCTAssertTrue(app.navigationBars["设置"].waitForExistence(timeout: 5))
  }
  func testKeyboardFullAccessAndConsistentModeHeight() {
    let app = launch()
    installKeyboard(in: app, fullAccess: true)
    verifyKeyboardStylesAndComposition(in: app)
  }

  /// For a dedicated QA device whose real keyboard/Full Access setup was already verified.
  func testKeyboardStylesAndCompositionWithInstalledKeyboard() {
    verifyKeyboardStylesAndComposition(in: launch())
  }

  func testTypingCandidateHeaderWithInstalledKeyboard() {
    verifyKeyboardStylesAndComposition(in: launch(), checkStyles: false)
  }

  private func verifyKeyboardStylesAndComposition(in app: XCUIApplication, checkStyles: Bool = true) {
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeVoice", in: app)
    let top = app.buttons["keyboardModeVoice"].frame.minY
    let picker = app.buttons["keyboardStylePicker"]
    XCTAssertTrue(picker.isHittable, app.debugDescription)
    XCTAssertGreaterThan(picker.frame.width, 60)
    if checkStyles {
      picker.tap()
      XCTAssertTrue(app.buttons["原文"].firstMatch.waitForExistence(timeout: 3), app.debugDescription)
      XCTAssertEqual(app.buttons.matching(NSPredicate(format: "label == %@", "原文")).count, 1)
      let light = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "轻度润色")).firstMatch
      XCTAssertTrue(light.waitForExistence(timeout: 3), app.debugDescription)
      capture(app, name: "build3-style-catalog-with-setup-state")
      light.tap()
      expectValue("轻度润色", on: picker)
      picker.tap()
      app.buttons["原文"].firstMatch.tap()
    }
    capture(app, name: "feedback-voice-transparent-mark-style-button")
    for mode in ["keyboardModeEnglish", "keyboardModePinyin", "keyboardModeVoice"] {
      selectKeyboardMode(mode, in: app)
      XCTAssertEqual(app.buttons["keyboardModeVoice"].frame.minY, top, accuracy: 1, "Mode switches must not grow the keyboard")
      XCTAssertLessThanOrEqual(editor.frame.maxY, top)
      if mode != "keyboardModeVoice" {
        XCTAssertLessThanOrEqual(app.buttons["keyboardReturn"].frame.maxY, top + 282.5,
          "The complete bottom key row must fit inside the fixed content viewport")
        if mode == "keyboardModeEnglish" {
          XCTAssertTrue(picker.isHittable, "Typing mode must also expose the style button before composition")
          app.buttons["keyboardKey-o"].tap(); app.buttons["keyboardKey-k"].tap()
          XCTAssertTrue((editor.value as? String)?.lowercased().contains("ok") == true)
          XCTAssertFalse(app.buttons["keyboardBrand"].isHittable)
          XCTAssertFalse(app.buttons["keyboardModeEnglish"].isHittable)
          let candidates = app.scrollViews["keyboardCandidates"]
          XCTAssertTrue(candidates.exists, app.debugDescription)
          XCTAssertEqual(candidates.frame.minY, top, accuracy: 1)
          capture(app, name: "build3-english-header-candidates")
          app.buttons["keyboardSpace"].tap()
          restoreKeyboardToolbar(in: app)
          XCTAssertTrue(app.buttons["keyboardBrand"].waitForExistence(timeout: 3))
        } else {
          for letter in ["n", "i", "h", "a", "o"] { app.buttons[letter].firstMatch.tap() }
          XCTAssertTrue(app.buttons["你好"].waitForExistence(timeout: 3))
          XCTAssertFalse(app.buttons["keyboardBrand"].isHittable)
          XCTAssertFalse(app.buttons["keyboardModePinyin"].isHittable)
          capture(app, name: "build3-pinyin-header-candidates")
          app.buttons["你好"].tap()
          restoreKeyboardToolbar(in: app)
          XCTAssertTrue(app.buttons["keyboardBrand"].waitForExistence(timeout: 3))
          XCTAssertTrue((editor.value as? String)?.contains("你好") == true)
        }
      }
      capture(app, name: "feedback-height-" + mode)
    }
    app.buttons["speakButton"].firstMatch.tap()
    XCTAssertFalse(app.staticTexts["设置 → OpenLess 键盘 → 允许完全访问\n仅语音需要此权限"].exists, app.debugDescription)
    XCTAssertFalse(app.staticTexts["语音需要“允许完全访问”\n基础打字不受影响"].exists, app.debugDescription)
    capture(app, name: "feedback-full-access-record-action")
  }

  func testKeyboardExtensionInputsChineseAndEnglishWithoutFullAccess() {
    let app = launch()
    installKeyboard(in: app)
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeVoice", in: app)
    capture(app, name: "voice-panel-official-brand")
    app.buttons["speakButton"].firstMatch.tap()
    XCTAssertTrue(app.staticTexts["设置 → OpenLess 键盘 → 允许完全访问\n仅语音需要此权限"].waitForExistence(timeout: 3), app.debugDescription)
    XCTAssertEqual(app.alerts.count, 0)
    selectKeyboardMode("keyboardModePinyin", in: app)
    XCTAssertLessThanOrEqual(editor.frame.maxY, app.buttons["keyboardModeVoice"].frame.minY)
    let q = app.buttons["q"].firstMatch
    let a = app.buttons["a"].firstMatch
    XCTAssertGreaterThan(a.frame.minX, q.frame.minX + 10, "The home row must be staggered")
    XCTAssertEqual(a.frame.width, q.frame.width, accuracy: 1, "Letter widths must not grow on shorter rows")
    for letter in ["n", "i", "h", "a", "o"] { app.buttons[letter].firstMatch.tap() }
    XCTAssertTrue(app.buttons["你好"].waitForExistence(timeout: 4), app.debugDescription)
    capture(app, name: "pinyin-candidates-staggered-rows")
    app.buttons["你好"].tap()
    XCTAssertTrue((editor.value as? String)?.contains("你好") == true)
    selectKeyboardMode("keyboardModeEnglish", in: app)
    for letter in ["h", "e", "l", "l", "o"] { app.buttons[letter].firstMatch.tap() }
    XCTAssertTrue((editor.value as? String)?.lowercased().contains("你好hello") == true, app.debugDescription)
    XCTAssertGreaterThan(app.buttons["keyboardSpace"].frame.width, app.buttons["keyboardReturn"].frame.width * 1.4)
    capture(app, name: "english-keyboard-redesigned")
    app.buttons["keyboardSpace"].tap()
    selectKeyboardMode("keyboardModePinyin", in: app)
    for _ in 0..<5 {
      // Reopening honors voice-first preferences; enter typing before dismissing.
      selectKeyboardMode("keyboardModePinyin", in: app)
      app.buttons["收起键盘"].tap()
      editor.tap()
      guard app.buttons["keyboardModeVoice"].waitForExistence(timeout: 8) else {
        capture(app, name: "keyboard-reopen-failure")
        XCTFail(app.debugDescription)
        return
      }
    }
    let systemDictation = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] %@ OR label CONTAINS %@", "dictat", "听写"))
    // iOS can retain a disabled, visually hidden dictation element in AX.
    // Its count is not a visibility test; also retain the full-screen proof.
    for button in systemDictation.allElementsBoundByIndex {
      XCTAssertFalse(button.isHittable, app.debugDescription)
      XCTAssertFalse(button.isEnabled)
    }
    capture(app, name: "keyboard-bottom-after-reopen")
    app.buttons["dismissTrialKeyboard"].tap()
    app.navigationBars.buttons.firstMatch.tap()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    XCTAssertTrue(app.navigationBars["设置"].waitForExistence(timeout: 5))
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 3))
  }

  func testOfflineNextWordAndChineseContinuation() {
    let app = launch()
    installKeyboard(in: app)
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeEnglish", in: app)
    for letter in ["t", "h", "a", "n", "k"] { app.buttons["keyboardKey-" + letter].tap() }
    app.buttons["keyboardSpace"].tap()
    let you = app.scrollViews["keyboardCandidates"].buttons["you"]
    XCTAssertTrue(you.waitForExistence(timeout: 3), app.debugDescription)
    capture(app, name: "offline-english-next-word-after-space")
    you.tap()
    XCTAssertTrue((editor.value as? String)?.lowercased().contains("thank you ") == true, app.debugDescription)
    selectKeyboardMode("keyboardModePinyin", in: app)
    for letter in ["n", "i", "h", "a", "o"] { app.buttons[letter].firstMatch.tap() }
    app.scrollViews["keyboardCandidates"].buttons["你好"].tap()
    let continuation = app.scrollViews["keyboardCandidates"].buttons["吗"]
    XCTAssertTrue(continuation.waitForExistence(timeout: 3), app.debugDescription)
    capture(app, name: "offline-chinese-continuation-after-commit")
    continuation.tap()
    XCTAssertTrue((editor.value as? String)?.contains("你好吗") == true, app.debugDescription)
    XCTAssertFalse((editor.value as? String)?.contains("你好你好吗") == true)
    selectKeyboardMode("keyboardModeVoice", in: app)
    XCTAssertTrue(app.buttons["speakButton"].firstMatch.isHittable)
  }
  func testSymbolsReturnAndCompositionSwitch() {
    let app = launch()
    installKeyboard(in: app)
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModePinyin", in: app)
    typePinyin("openless", in: app)
    XCTAssertFalse(app.buttons["keyboardRawInputCandidate"].exists,
      "Keep Chinese candidates; original English is confirmed with Return.")
    app.buttons["keyboardReturn"].tap()
    expectValue("openless", on: editor)
    app.buttons["keyboardReturn"].tap()
    expectValue("openless\n", on: editor)
    typePinyin("nihao", in: app)
    XCTAssertTrue(app.buttons["你好"].waitForExistence(timeout: 3))
    app.buttons["你好"].tap()
    expectValue("openless\n你好", on: editor)
    selectKeyboardMode("keyboardModeVoice", in: app)
    let at = app.buttons["keyboardAt"]
    XCTAssertTrue(at.waitForExistence(timeout: 3))
    XCTAssertTrue(at.isHittable)
    XCTAssertGreaterThanOrEqual(at.frame.width, 44)
    at.tap()
    expectValue("openless\n你好@", on: editor)
    capture(app, name: "build11-reference-voice-at")
    selectKeyboardMode("keyboardModePinyin", in: app)
    capture(app, name: "build11-reference-pinyin")
  }

  func testSingleKeyboardExperiencePersistsAndKeepsTypingOnDeniedHold() {
    var app = launch()
    installKeyboard(in: app, fullAccess: true)
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let picker = app.buttons["keyboardExperiencePicker"]
    show(picker, in: app); picker.tap()
    app.buttons["单一键盘"].firstMatch.tap()
    XCTAssertTrue(app.buttons["keyboardOutputLanguagePicker"].exists)
    XCTAssertFalse(app.buttons["spaceHoldPicker"].exists)
    app.terminate(); app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    show(app.buttons["keyboardExperiencePicker"], in: app)
    XCTAssertTrue(app.buttons["keyboardOutputLanguagePicker"].exists)
    app.tabBars["primaryDock"].buttons["概览"].tap()
    let editor = openEditor(app)
    selectOpenLess(app)
    let toggle = app.buttons["keyboardLanguageToggle"]
    XCTAssertTrue(toggle.waitForExistence(timeout: 10))
    XCTAssertFalse(app.buttons["keyboardModeVoice"].exists)
    XCTAssertFalse(app.buttons["speakButton"].exists)
    if toggle.value as? String != "English" { toggle.tap() }
    for key in "ssqw" { app.buttons["keyboardKey-" + String(key)].tap() }
    let typed = editor.value as? String ?? ""
    XCTAssertEqual(typed.lowercased(), "ssqw")
    let space = app.buttons["keyboardSpace"]
    XCTAssertGreaterThan(space.frame.width, 85)
    XCTAssertLessThanOrEqual(space.frame.height, 60)
    space.tap(); expectValue(typed + " ", on: editor)
    space.press(forDuration: 0.5)
    expectValue(typed + " ", on: editor)
    XCTAssertTrue(toggle.exists)
    XCTAssertFalse(app.buttons["speakButton"].exists)
    capture(app, name: "single-keyboard-english")
    toggle.tap()
    XCTAssertEqual(toggle.value as? String, "中文全拼")
    capture(app, name: "single-keyboard-chinese")
    app.buttons["keyboardNumbers"].tap()
    XCTAssertTrue(app.buttons["keyboardLetters"].waitForExistence(timeout: 3))
    app.buttons["keyboardLetters"].tap()
    app.terminate(); app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let restored = app.buttons["keyboardExperiencePicker"]
    show(restored, in: app); restored.tap()
    app.buttons["语音优先"].firstMatch.tap()
    XCTAssertTrue(app.buttons["spaceHoldPicker"].exists)
    app.tabBars["primaryDock"].buttons["概览"].tap()
    _ = openEditor(app)
    selectOpenLess(app)
    restoreKeyboardToolbar(in: app)
    XCTAssertTrue(app.buttons["keyboardModeVoice"].waitForExistence(timeout: 5))
  }

  /// Synthetic service responses exercise real extension touches and durable delivery.
  /// Serve Tests/Fixtures/voice-gesture.html on loopback and open it in the system browser.
  func testHeldVoiceLifecycleAcrossInputTranslationSendAndCancel() throws {
    #if !targetEnvironment(simulator)
    throw XCTSkip("The protocol fixture is restricted to the dedicated simulator.")
    #else
    let main = launch()
    installKeyboard(in: main, fullAccess: true)
    main.terminate()
    let mailbox = try voiceMailbox()
    let original = try mailbox.value(KeyboardPreferences.self, for: SharedContract.preferencesKey)
    var preferences = KeyboardPreferences()
    preferences.experience = .singleKeyboard
    preferences.language = .english
    preferences.translationOutputLanguage = "English"
    try mailbox.set(preferences, for: SharedContract.preferencesKey)
    let host = HeldVoiceFixtureHost(mailbox: mailbox)
    host.start()
    defer {
      host.stop()
      if let original { try? mailbox.set(original, for: SharedContract.preferencesKey) }
    }
    let safari = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
    safari.terminate(); safari.activate()
    let editor = safari.textViews["Voice gesture test input"]
    XCTAssertTrue(editor.waitForExistence(timeout: 15), safari.debugDescription)
    editor.tap()
    selectOpenLess(safari)
    let space = safari.buttons["keyboardSpace"]
    let overlay = safari.otherElements["spaceVoiceOverlay"]
    func waitForHost(_ predicate: @escaping () -> Bool) {
      let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in predicate() }, object: nil)
      XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 8), .completed)
    }
    func hold(_ action: SpaceVoiceGesture.Action) {
      restoreKeyboardToolbar(in: safari)
      XCTAssertTrue(space.waitForExistence(timeout: 5))
      let start = space.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
      if action == .input { start.press(forDuration: 1.5); return }
      let top = safari.buttons["keyboardBrand"].frame.minY - 6
      let height = space.frame.maxY + 11 - top
      let x = safari.frame.width * (action == .cancel ? 1.0 / 6 : action == .translate ? 0.5 : 5.0 / 6)
      let point = safari.coordinate(withNormalizedOffset: .zero)
        .withOffset(CGVector(dx: x, dy: top + height * 0.46 + 34))
      start.press(forDuration: 1.5, thenDragTo: point)
    }
    func assertProcessing() {
      XCTAssertTrue(overlay.waitForExistence(timeout: 5))
      let processing = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", "processing"), object: overlay)
      XCTAssertEqual(XCTWaiter.wait(for: [processing], timeout: 5), .completed)
      XCTAssertEqual(safari.staticTexts.matching(identifier: "thinking").count, 1)
      XCTAssertFalse(safari.staticTexts["正在聆听"].exists)
      XCTAssertFalse(safari.buttons["keyboardKey-q"].exists)
      XCTAssertTrue(safari.buttons["spaceVoice.cancelProcessing"].exists)
    }
    func assertReady() {
      XCTAssertTrue(space.waitForExistence(timeout: 8), safari.debugDescription)
      XCTAssertFalse(overlay.exists)
      XCTAssertFalse(safari.staticTexts["thinking"].exists)
      XCTAssertTrue(safari.buttons["keyboardLanguageToggle"].exists)
    }

    hold(.input)
    waitForHost { host.finish == .input }
    assertProcessing()
    host.publishLateRecording()
    Thread.sleep(forTimeInterval: 0.5)
    assertProcessing()
    capture(safari, name: "held-voice-single-processing-surface")
    host.complete("fixture input")
    expectValue("fixture input", on: editor)
    assertReady()
    host.repeatDelivery()
    Thread.sleep(forTimeInterval: 0.3)
    expectValue("fixture input", on: editor)

    host.reset()
    hold(.translate)
    waitForHost { host.finish == .translate }
    XCTAssertEqual(host.outputLanguage, "English")
    assertProcessing()
    host.complete(" translated")
    expectValue("fixture input translated", on: editor)
    assertReady()

    host.reset()
    hold(.input)
    assertProcessing()
    safari.buttons["spaceVoice.cancelProcessing"].tap()
    waitForHost { host.cancelled }
    assertReady()
    expectValue("fixture input translated", on: editor)

    host.reset()
    hold(.cancel)
    waitForHost { host.cancelled }
    assertReady()
    expectValue("fixture input translated", on: editor)

    host.reset()
    hold(.send)
    waitForHost { host.finish == .send }
    assertProcessing()
    host.complete(" sent")
    expectValue("fixture input translated sent", on: editor)
    XCTAssertTrue(safari.staticTexts["Submitted: 1"].waitForExistence(timeout: 5))
    assertReady()
    safari.buttons["keyboardKey-z"].tap()
    expectValue("fixture input translated sentz", on: editor)
    XCTAssertTrue(host.failures.isEmpty, host.failures.joined(separator: ", "))
    capture(safari, name: "held-voice-returned-to-typing")
    #endif
  }

  func testSystemStyleBottomRowAndCandidateExpansion() {
    let app = launch()
    installKeyboard(in: app, fullAccess: true)
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let picker = app.buttons["keyboardExperiencePicker"]
    show(picker, in: app); picker.tap()
    app.buttons["单一键盘"].firstMatch.tap()
    app.tabBars["primaryDock"].buttons["概览"].tap()
    let editor = openEditor(app)
    selectOpenLess(app)
    let language = app.buttons["keyboardLanguageToggle"]
    XCTAssertTrue(language.waitForExistence(timeout: 8))
    let numbers = app.buttons["keyboardNumbers"]
    let space = app.buttons["keyboardSpace"]
    let primary = app.buttons["keyboardReturn"]
    XCTAssertLessThan(numbers.frame.midX, language.frame.midX)
    XCTAssertLessThan(language.frame.maxX, space.frame.minX + 4)
    XCTAssertLessThan(space.frame.maxX, primary.frame.minX + 4)
    XCTAssertEqual(language.frame.width, numbers.frame.width, accuracy: 2)
    XCTAssertGreaterThan(space.frame.width, primary.frame.width * 1.8)
    if language.value as? String == "English" { language.tap() }
    capture(app, name: "system-style-bottom-row-chinese")
    typePinyin("nihao", in: app)
    let expand = app.buttons["keyboardExpandCandidates"]
    XCTAssertTrue(expand.waitForExistence(timeout: 5))
    expand.tap()
    XCTAssertTrue(app.scrollViews["keyboardExpandedCandidates"].waitForExistence(timeout: 5))
    XCTAssertFalse(app.buttons["keyboardKey-q"].exists)
    capture(app, name: "expanded-chinese-candidates")
    let chosen = app.buttons["expandedCandidate-0"]
    XCTAssertTrue(chosen.waitForExistence(timeout: 5))
    let word = chosen.label
    chosen.tap()
    expectValue(word, on: editor)
    XCTAssertFalse(app.scrollViews["keyboardExpandedCandidates"].exists)
    XCTAssertTrue(app.buttons["keyboardKey-q"].exists)
    language.tap()
    XCTAssertEqual(language.value as? String, "English")
    app.buttons["keyboardKey-h"].tap()
    app.buttons["keyboardKey-e"].tap()
    let englishExpand = app.buttons["keyboardExpandCandidates"]
    XCTAssertTrue(englishExpand.waitForExistence(timeout: 5))
    englishExpand.tap()
    let englishCandidate = app.buttons["expandedCandidate-1"]
    XCTAssertTrue(englishCandidate.waitForExistence(timeout: 5))
    let completion = englishCandidate.label
    englishCandidate.tap()
    expectValue(word + completion + " ", on: editor)
    XCTAssertTrue(language.exists)
    capture(app, name: "system-style-bottom-row-english")
  }

  func testSingleKeyboardCompactLayout() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let picker = app.buttons["keyboardExperiencePicker"]
    show(picker, in: app); picker.tap()
    app.buttons["单一键盘"].firstMatch.tap()
    app.tabBars["primaryDock"].buttons["概览"].tap()
    _ = openEditor(app)
    selectOpenLess(app)
    XCTAssertTrue(app.buttons["keyboardLanguageToggle"].waitForExistence(timeout: 5))
    XCTAssertTrue(app.buttons["keyboardStylePicker"].exists)
    XCTAssertTrue(app.buttons["keyboardOutputLanguage"].exists)
    XCTAssertGreaterThanOrEqual(app.buttons["keyboardInlineVoice"].frame.width, 44)
    XCTAssertFalse(app.staticTexts["keyboardTypingHint"].exists)
    let space = app.buttons["keyboardSpace"]
    let brand = app.buttons["keyboardBrand"]
    XCTAssertLessThan(space.frame.maxY - brand.frame.minY, 265)
    XCTAssertGreaterThan(space.frame.width, 85)
    capture(app, name: "single-keyboard-final-portrait")
    XCUIDevice.shared.orientation = .landscapeLeft
    defer { XCUIDevice.shared.orientation = .portrait }
    XCTAssertTrue(space.waitForExistence(timeout: 5))
    XCTAssertLessThan(space.frame.maxY - brand.frame.minY, 225)
    capture(app, name: "single-keyboard-final-landscape")
  }

  func testConfiguredSpaceKeyTouch() {
    let app = launch()
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeEnglish", in: app)
    let space = app.buttons["keyboardSpace"]
    XCTAssertTrue(space.label.contains("按住说话"), space.debugDescription)
    XCTAssertGreaterThan(space.frame.width, 80)
    XCTAssertGreaterThan(space.frame.height, 30)
    capture(app, name: "space-native-frame")
    XCTAssertTrue(space.isHittable, space.debugDescription)
    app.buttons["keyboardKey-x"].tap()
    let typed = editor.value as? String ?? ""
    space.tap()
    expectValue(typed + " ", on: editor)
    space.press(forDuration: 0.5)
    XCTAssertTrue(app.buttons["speakButton"].waitForExistence(timeout: 5))
    expectValue(typed + " ", on: editor)
    capture(app, name: "space-touch-verified")
  }

  func testSpaceVoiceShortTapAndRapidTyping() {
    let app = launch()
    installKeyboard(in: app, fullAccess: true)
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let picker = app.buttons["spaceHoldPicker"]
    show(picker, in: app)
    picker.tap()
    app.buttons["按住录音"].firstMatch.tap()
    app.tabBars["primaryDock"].buttons["概览"].tap()
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeEnglish", in: app)
    // Consecutive identical/adjacent keys must produce one character per tap.
    for key in "ssddqwwe" { app.buttons["keyboardKey-" + String(key)].tap() }
    let typed = editor.value as? String ?? ""
    XCTAssertEqual(typed.lowercased(), "ssddqwwe")
    app.buttons["keyboardSpace"].tap()
    expectValue(typed + " ", on: editor)
    app.buttons["keyboardSpace"].press(forDuration: 0.5)
    XCTAssertTrue(app.buttons["speakButton"].waitForExistence(timeout: 5))
    expectValue(typed + " ", on: editor)
    XCTAssertFalse(app.otherElements["spaceVoiceOverlay"].exists,
      "A denied hold does not fabricate an active recording or insert a trailing space.")
    capture(app, name: "space-voice-needs-activation")
  }

  func testVoiceKeyboardReferenceLayout() {
    let app = launch()
    _ = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeVoice", in: app)
    let at = app.buttons["keyboardAt"]
    XCTAssertTrue(at.waitForExistence(timeout: 5))
    let primary = app.buttons["keyboardVoiceReturn"]
    XCTAssertLessThan(abs(primary.frame.midX - app.frame.midX), 8,
      "The primary action stays centered between the two bottom edit controls.")
    capture(app, name: "build11-final-voice-layout")
    selectKeyboardMode("keyboardModePinyin", in: app)
    capture(app, name: "build11-final-pinyin-layout")
  }

  func testKeyboardHeightPreferenceSurvivesRelaunch() {
    var app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    let slider = app.sliders["keyboardHeightSlider"]
    show(slider, in: app)
    slider.adjust(toNormalizedSliderPosition: 0)
    XCTAssertTrue(app.staticTexts["90%"].waitForExistence(timeout: 3))
    app.terminate()
    app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    show(app.sliders["keyboardHeightSlider"], in: app)
    XCTAssertTrue(app.staticTexts["90%"].waitForExistence(timeout: 3))
    app.buttons["resetKeyboardHeight"].tap()
    XCTAssertTrue(app.staticTexts["100%"].waitForExistence(timeout: 3))
    capture(app, name: "build11-height-preference")
  }
  func testKeyboardLandscapeAndRepeatedModeChanges() {
    let app = launch()
    installKeyboard(in: app)
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeEnglish", in: app)
    XCUIDevice.shared.orientation = .landscapeLeft
    defer { XCUIDevice.shared.orientation = .portrait }
    let wide = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in app.frame.width > app.frame.height }, object: app)
    XCTAssertEqual(XCTWaiter.wait(for: [wide], timeout: 5), .completed)
    for _ in 0..<6 {
      selectKeyboardMode("keyboardModeVoice", in: app)
      selectKeyboardMode("keyboardModeEnglish", in: app)
    }
    for letter in ["h", "i"] { app.buttons[letter].firstMatch.tap() }
    XCTAssertEqual(editor.value as? String, "hi")
    XCTAssertGreaterThan(editor.frame.height, 25, "Landscape editor must retain a usable line")
    XCTAssertLessThanOrEqual(editor.frame.maxY, app.buttons["keyboardModeVoice"].frame.minY)
    capture(app, name: "landscape-staggered-keyboard")
    selectKeyboardMode("keyboardModeVoice", in: app)
    capture(app, name: "landscape-voice-panel")
  }

  func testCapsLockCursorAndVoiceSwipe() {
    let app = launch()
    installKeyboard(in: app)
    let editor = openEditor(app)
    selectOpenLess(app)
    selectKeyboardMode("keyboardModeEnglish", in: app)
    let shift = app.buttons["切换大小写"]
    shift.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).doubleTap()
    expectValue("大写锁定", on: shift)
    app.buttons["keyboardKey-a"].tap()
    app.buttons["keyboardKey-b"].tap()
    XCTAssertEqual(editor.value as? String, "AB")
    shift.tap()
    expectValue("小写", on: shift)
    app.buttons["keyboardKey-c"].tap()
    let space = app.buttons["keyboardSpace"]
    let start = space.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
    // KeyboardKit's standard long-press delay is 0.5s, not the old custom 0.35s.
    start.press(forDuration: 0.65, thenDragTo: start.withOffset(CGVector(dx: -32, dy: 0)))
    app.buttons["keyboardKey-x"].tap()
    let text = editor.value as? String ?? ""
    XCTAssertEqual(text.count, 4, "Cursor mode must not insert an accidental space")
    XCTAssertNotEqual(text, "ABcx", "Holding space must actually move the insertion point")
    XCTAssertTrue(text.contains("x"))
    capture(app, name: "caps-lock-and-space-cursor")
    selectKeyboardMode("keyboardModeVoice", in: app)
    let y = app.buttons["keyboardModeVoice"].frame.maxY + 24
    let from = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: app.frame.width * 0.85, dy: y))
    let to = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: app.frame.width * 0.15, dy: y))
    from.press(forDuration: 0.05, thenDragTo: to)
    XCTAssertTrue(app.buttons["keyboardKey-q"].waitForExistence(timeout: 3), "Swipe the voice background, not just its toolbar")
    XCTAssertTrue(app.buttons["keyboardModeEnglish"].isSelected)
    XCTAssertEqual(app.alerts.count, 0)
    capture(app, name: "voice-swipe-to-typing")
  }

  func testPermissionCallbackDoesNotCrashOrStartRecording() {
    let app = launch()
    let prepare = app.buttons["prepareSpeech"]
    show(prepare, in: app)
    prepare.tap()
    let system = XCUIApplication(bundleIdentifier: "com.apple.springboard")
    var deniedNewPrompt = false
    for _ in 0..<3 {
      if system.alerts.firstMatch.waitForExistence(timeout: 1) {
        let deny = system.alerts.buttons.matching(NSPredicate(format: "label IN %@", ["不允许", "不允許", "Don't Allow"])).firstMatch
        if deny.exists { deny.tap(); deniedNewPrompt = true }
      }
      if app.alerts.buttons["暂不下载"].exists { app.alerts.buttons["暂不下载"].tap() }
    }
    XCTAssertTrue(app.wait(for: .runningForeground, timeout: 5))
    XCTAssertTrue(app.staticTexts["OpenLess"].exists)
    if deniedNewPrompt {
      XCTAssertTrue(app.buttons["openSpeechSettings"].exists, "Denied authorization must offer a settings route")
    }
    XCTAssertFalse(app.staticTexts["正在录音"].exists)
    capture(app, name: "permission-callback-survived")
  }
  func testConsumerSettingsHideEngineeringDiagnostics() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["设置"].tap()
    show(app.buttons["appVersion"], in: app)
    XCTAssertFalse(app.buttons["openDiagnostics"].exists)
    XCTAssertFalse(app.buttons["收集接口诊断（不启动小窗）"].exists)
    XCTAssertEqual(app.alerts.count, 0)
  }

  func testDockRoundTripsAndStyleCardsStayResponsive() {
    let app = launch()
    for index in 0..<12 {
      app.tabBars["styleDock"].buttons["dockStyles"].tap()
      let library = app.tabBars["styleDock"].buttons["dockStyle-library"]
      XCTAssertTrue(library.waitForExistence(timeout: 3))
      library.tap()
      let detail = app.buttons["styleDetail-builtin.raw"]
      XCTAssertTrue(detail.waitForExistence(timeout: 5))
      XCTAssertFalse(app.buttons["styleManage-builtin.raw"].exists)
      let use = app.buttons["styleUse-builtin.raw"]
      XCTAssertTrue(use.exists)
      XCTAssertTrue(["使用此风格", "当前使用"].contains(use.label))
      if index == 0 { capture(app, name: "consumer-style-cards") }
      detail.tap()
      XCTAssertTrue(app.navigationBars["风格详情"].waitForExistence(timeout: 3))
      XCTAssertTrue(app.buttons["styleActivate-builtin.raw"].exists)
      app.navigationBars.buttons.firstMatch.tap()
      app.tabBars["styleDock"].buttons["dockStyle-translation"].tap()
      XCTAssertTrue(app.buttons["addTranslationTarget"].waitForExistence(timeout: 3))
      app.tabBars["primaryDock"].buttons["词典"].tap()
      XCTAssertTrue(app.buttons["searchVocabulary"].waitForExistence(timeout: 3))
      XCTAssertTrue(app.tabBars["primaryDock"].buttons["概览"].exists)
      XCTAssertTrue(app.tabBars["primaryDock"].buttons["历史"].exists)
      XCTAssertTrue(app.tabBars["primaryDock"].buttons["设置"].exists)
      XCTAssertTrue(app.tabBars["primaryDock"].buttons["词典"].isSelected)
    }
    capture(app, name: "consumer-dock-restored-dictionary")
    app.tabBars["primaryDock"].buttons["设置"].tap()
    XCTAssertTrue(app.buttons["openAccountSettings"].waitForExistence(timeout: 3))
    capture(app, name: "consumer-settings-profile")
  }
  func testNativeDockPressDragAndBottomActions() {
    let app = launch()
    let home = app.tabBars["primaryDock"].buttons["概览"]
    let settings = app.tabBars["primaryDock"].buttons["设置"]
    XCTAssertTrue(home.waitForExistence(timeout: 5))
    home.press(forDuration: 0.6, thenDragTo: settings)
    XCTAssertTrue(app.navigationBars["设置"].waitForExistence(timeout: 4))
    let version = app.buttons["appVersion"]
    show(version, in: app)
    XCTAssertTrue(version.isHittable)
    XCTAssertLessThan(version.frame.maxY, settings.frame.minY, "Version must be fully above the dock")
    version.tap(); version.tap(); version.tap()
    XCTAssertTrue(app.alerts.firstMatch.waitForExistence(timeout: 3))
    XCTAssertTrue(["开启开发者模式？", "退出开发者模式？"].contains(app.alerts.firstMatch.label))
    app.alerts.buttons["取消"].tap()
    capture(app, name: "settings-bottom-fully-reachable")
    app.tabBars["styleDock"].buttons["dockStyles"].tap()
    let mine = app.tabBars["styleDock"].buttons["dockStyle-library"]
    let translation = app.tabBars["styleDock"].buttons["dockStyle-translation"]
    XCTAssertTrue(mine.waitForExistence(timeout: 5))
    capture(app, name: "style-dock-first-expansion")
    mine.press(forDuration: 0.6, thenDragTo: translation)
    XCTAssertTrue(app.navigationBars["翻译目标"].waitForExistence(timeout: 4))
    translation.press(forDuration: 0.6, thenDragTo: mine)
    XCTAssertTrue(app.navigationBars["我的风格"].waitForExistence(timeout: 4))
    app.buttons["styleDetail-builtin.raw"].tap()
    let use = app.buttons["styleActivate-builtin.raw"]
    XCTAssertTrue(use.waitForExistence(timeout: 5))
    XCTAssertLessThan(use.frame.maxY, mine.frame.minY, "Style action must never sit under the dock")
    capture(app, name: "style-action-above-dock")
    app.tabBars["primaryDock"].buttons["词典"].tap()
    app.tabBars["primaryDock"].buttons["历史"].tap()
    capture(app, name: "main-dock-restored-titles")
  }

  func testStyleCanBeSelectedDirectlyOnItsCard() throws {
    let app = launch()
    app.tabBars["styleDock"].buttons["dockStyles"].tap()
    let cards = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "styleUse-"))
    let current = cards.matching(NSPredicate(format: "label == %@", "当前使用")).firstMatch
    XCTAssertTrue(current.waitForExistence(timeout: 6))
    let originalID = current.identifier
    let target = try XCTUnwrap(cards.allElementsBoundByIndex.first { $0.isHittable && $0.isEnabled })
    let targetID = target.identifier
    target.tap()
    let changed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "当前使用"), object: app.buttons[targetID])
    let result = XCTWaiter.wait(for: [changed], timeout: 8)
    capture(app, name: "style-card-direct-selection")
    let original = app.buttons[originalID]
    if original.exists && original.isEnabled { original.tap() }
    let restored = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "当前使用"), object: original)
    XCTAssertEqual(XCTWaiter.wait(for: [restored], timeout: 8), .completed)
    XCTAssertEqual(result, .completed)
  }

  func testSearchSurfacesFilterAndDictionaryCollapses() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["dockTab-3"].tap()
    XCTAssertTrue(app.buttons["vocabularySource-learned"].waitForExistence(timeout: 5))
    app.buttons["searchVocabulary"].firstMatch.tap()
    let dictionary = app.textFields["vocabularySearch"]
    XCTAssertTrue(dictionary.waitForExistence(timeout: 5))
    dictionary.typeText("zzzznoentry")
    XCTAssertTrue(app.staticTexts["没有匹配的词条"].waitForExistence(timeout: 5))
    capture(app, name: "dictionary-native-search-single-surface")
    app.buttons["searchVocabulary"].firstMatch.tap()
    XCTAssertFalse(dictionary.exists)
    XCTAssertTrue(app.buttons["vocabularySource-learned"].exists)
    capture(app, name: "dictionary-reference-filters-restored")
    app.tabBars["styleDock"].buttons["dockStyles"].tap()
    let styles = app.textFields["installedStyleQuery"]
    XCTAssertTrue(styles.waitForExistence(timeout: 5))
    styles.tap(); styles.typeText("zzzznostyle")
    XCTAssertTrue(app.staticTexts["没有匹配的风格，试试其他关键词。"].waitForExistence(timeout: 5))
    XCTAssertFalse(app.buttons["styleDetail-builtin.raw"].exists)
    styles.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 11) + "\n")
    XCTAssertTrue(app.buttons["styleDetail-builtin.raw"].waitForExistence(timeout: 5))
    capture(app, name: "styles-native-search-single-surface")
    app.tabBars["primaryDock"].buttons.matching(NSPredicate(
      format: "identifier IN %@", ["dockDictionary", "dockTab-3"])).firstMatch.tap()
    app.tabBars["primaryDock"].buttons["历史"].firstMatch.tap()
    let history = app.textFields["historySearch"]
    XCTAssertTrue(history.waitForExistence(timeout: 5))
    history.tap(); history.typeText("zzzznohistory\n")
    XCTAssertTrue(app.staticTexts["没有匹配的记录"].waitForExistence(timeout: 5))
    app.switches["historyDateFilter"].tap()
    XCTAssertEqual(app.datePickers.count, 2)
    capture(app, name: "history-text-and-date-search")
    app.tabBars["primaryDock"].buttons["设置"].firstMatch.tap()
    let settings = app.textFields["settingsSearch"]
    XCTAssertTrue(settings.waitForExistence(timeout: 5))
    settings.tap(); settings.typeText("privacy\n")
    XCTAssertTrue(app.buttons["openHelp"].waitForExistence(timeout: 5))
    XCTAssertFalse(app.buttons["openAccountSettings"].exists)
    capture(app, name: "settings-search-results")
  }

  /// Synthetic completed transcript, real shared mailbox and real keyboard.
  /// No test-only behavior is compiled into the application or extension.
  func testReturnedVoiceAutomaticallyInsertsIntoCurrentInputExactlyOnce() throws {
    try verifyAutomaticInsertion(install: true)
  }
  func testAutoInsertionWithInstalledKeyboard() throws {
    try verifyAutomaticInsertion(install: false)
  }
  private func verifyAutomaticInsertion(install: Bool) throws {
    let main = XCUIApplication()
    if install {
      main.launch()
      installKeyboard(in: main, fullAccess: true)
    }
    main.terminate()
    let app = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
    // Xcode may reinstall the extension while Safari retains a remote keyboard
    // view from the previous run. Start a fresh host connection for this test.
    app.terminate()
    app.activate()
    let editor = app.textViews["Routing test input"]
    XCTAssertTrue(editor.waitForExistence(timeout: 10))
    app.buttons["Place test cursor"].tap()
    XCTAssertTrue(app.staticTexts["Cursor: 7-7"].waitForExistence(timeout: 5))
    selectOpenLess(app)
    // XCTest's prebuilt Simulator runner does not inherit the test bundle's
    // simulated App Group entitlement. The harness supplies the installed
    // application's real shared-container path via simctl for this fixture.
    let group: URL
    #if targetEnvironment(simulator)
    if let path = ProcessInfo.processInfo.environment["OPENLESS_TEST_SHARED_CONTAINER"], !path.isEmpty {
      group = URL(fileURLWithPath: path, isDirectory: true)
    } else {
      group = try XCTUnwrap(FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.app.openless.ios"))
    }
    #else
    group = try XCTUnwrap(FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.app.openless.ios"))
    #endif
    let mailbox = try Mailbox(url: group.appendingPathComponent("ipc/mailbox.sqlite"))
    let oldTarget = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 999)
    let text = "自动输入验证"
    let expected = "prefix " + text + "suffix"
    var result = HostVoiceActivation(target: oldTarget, language: .chinese, startOnOpen: true)
    try mailbox.requestHostVoice(result, replacingUnresponsiveHost: true)
    result.phase = .ready; result.text = text
    try mailbox.updateHostVoice(result)
    CFNotificationCenterPostNotification(CFNotificationCenterGetDarwinNotifyCenter(), CFNotificationName(rawValue: "group.app.openless.ios.state.v2" as CFString), nil, nil, true)
    expectValue(expected, on: editor)
    XCTAssertFalse(app.buttons["确认插入这里"].exists)
    XCTAssertFalse(app.staticTexts["输入位置已变化，请确认插入当前输入框。"].exists)
    let consumed = try XCTUnwrap(mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey))
    XCTAssertEqual(consumed.phase, .consumed)
    let insertionID = try XCTUnwrap(consumed.insertionID)
    XCTAssertEqual(try mailbox.insertion(insertionID)?.state, .submitted)
    for _ in 0..<3 { CFNotificationCenterPostNotification(CFNotificationCenterGetDarwinNotifyCenter(), CFNotificationName(rawValue: "group.app.openless.ios.state.v2" as CFString), nil, nil, true) }
    app.buttons.matching(NSPredicate(format: "label IN %@", ["完成", "Done"])).firstMatch.tap()
    editor.tap()
    selectOpenLess(app)
    expectValue(expected, on: editor)
    capture(app, name: "voice-result-auto-insert-no-confirmation-no-replay")
    app.buttons.matching(NSPredicate(format: "label IN %@", ["完成", "Done"])).firstMatch.tap()
    var delayed = HostVoiceActivation(target: nil, language: .chinese, startOnOpen: true)
    try mailbox.requestHostVoice(delayed)
    delayed.phase = .ready; delayed.text = "再次输入"
    try mailbox.updateHostVoice(delayed)
    CFNotificationCenterPostNotification(CFNotificationCenterGetDarwinNotifyCenter(), CFNotificationName(rawValue: "group.app.openless.ios.state.v2" as CFString), nil, nil, true)
    Thread.sleep(forTimeInterval: 0.5)
    XCTAssertEqual(try mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey)?.phase, .ready)
    expectValue(expected, on: editor)
    editor.tap()
    selectOpenLess(app)
    let inserted = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value CONTAINS %@", "再次输入"), object: editor)
    XCTAssertEqual(XCTWaiter.wait(for: [inserted], timeout: 5), .completed)
    let finalText = try XCTUnwrap(editor.value as? String)
    XCTAssertEqual(finalText.components(separatedBy: "再次输入").count, 2)
    XCTAssertEqual(finalText.replacingOccurrences(of: "再次输入", with: ""), expected)
    capture(app, name: "voice-result-waits-for-input-and-inserts-on-reattach")
  }

  /// Real keyboard -> cold host -> automatic capture -> original Safari field.
  /// The local textarea fixture is served at localhost:8765 with microphone
  /// permission granted on this test simulator before launch.
  func testKeyboardColdVoiceAutomaticallyReturnsAndKeepsInputPosition() {
    verifyColdVoiceReturn(install: true)
  }
  func testColdVoiceReturnWithInstalledKeyboard() {
    verifyColdVoiceReturn(install: false)
  }
  /// Requires the installed OpenLess keyboard with Full Access, microphone
  /// permission and prepared ASR assets. Safari must already display the real
  /// localhost:8765 textarea fixture. No application/extension test hooks are used.
  /// Simulator runners without App Group access also need OPENLESS_TEST_SHARED_CONTAINER.
  func testKeyboardVoiceSurvivesDismissalAndReopening() {
    executionTimeAllowance = 300
    let app = launch()
    defer { app.terminate() }
    app.tabBars["primaryDock"].buttons["概览"].tap()
    let prepare = app.buttons["prepareSpeech"]
    XCTAssertTrue(prepare.waitForExistence(timeout: 15))
    let enabled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: prepare)
    XCTAssertEqual(XCTWaiter.wait(for: [enabled], timeout: 20), .completed)
    // Preparation arms the independent output session without starting capture.
    // All recordings below originate in the real Safari keyboard extension.
    prepare.tap()
    let session = app.buttons["toggleKeyboardVoice"]
    XCTAssertTrue(session.waitForExistence(timeout: 60), app.debugDescription)
    let ready = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS %@", "结束键盘语音会话"), object: session)
    XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 10), .completed, app.debugDescription)
    capture(app, name: "keyboard-reopening-output-session-armed")

    let safari = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
    // Discard a remote keyboard view retained across an extension reinstall.
    safari.terminate()
    safari.activate()
    XCTAssertTrue(safari.wait(for: .runningForeground, timeout: 10))
    let editor = safari.textViews["Routing test input"]
    XCTAssertTrue(editor.waitForExistence(timeout: 15), safari.debugDescription)
    // Safari can restore DOM focus without presenting the software keyboard.
    // A real field tap establishes UIKit focus before positioning the caret.
    editor.tap()
    safari.buttons["Place test cursor"].tap()
    XCTAssertTrue(safari.staticTexts["Cursor: 7-7"].waitForExistence(timeout: 5), safari.debugDescription)
    selectOpenLess(safari)
    selectKeyboardMode("keyboardModeVoice", in: safari)
    XCTAssertTrue(safari.buttons["speakButton"].waitForExistence(timeout: 10), safari.debugDescription)
    do { try verifyVoiceReopening(in: safari, host: app, editor: editor) }
    catch { XCTFail("Keyboard reopening regression failed: \(error)") }
  }
  private func verifyColdVoiceReturn(install: Bool) {
    let app = install ? launch() : XCUIApplication()
    if install { installKeyboard(in: app, fullAccess: true) }
    app.terminate()
    let safari = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
    safari.terminate()
    safari.activate()
    let editor = safari.textViews["Routing test input"]
    XCTAssertTrue(editor.waitForExistence(timeout: 15), safari.debugDescription)
    editor.tap()
    safari.buttons["Place test cursor"].tap()
    XCTAssertTrue(safari.staticTexts["Cursor: 7-7"].waitForExistence(timeout: 5), safari.debugDescription)
    selectOpenLess(safari)
    selectKeyboardMode("keyboardModeVoice", in: safari)
    let speak = safari.buttons["speakButton"]
    XCTAssertTrue(speak.waitForExistence(timeout: 8))
    XCTAssertTrue(safari.staticTexts["Cursor: 7-7"].exists, "The starting caret must be known before the handoff.")
    speak.tap()
    let recording = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "结束录音"), object: speak)
    XCTAssertEqual(XCTWaiter.wait(for: [recording], timeout: 90), .completed,
      "One keyboard tap must start capture and return without a second start tap.\n" + safari.debugDescription)
    guard let mailbox = try? voiceMailbox(),
      let started = try? mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey),
      let sessionID = started.sessionID else {
      XCTFail("The cold recording must publish a real shared session.")
      return
    }
    // The remote keyboard can receive real PCM and update its label before the
    // foreground return animation finishes. Require the completed handoff too.
    var handoffStates: [String] = []
    let returned = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
      let sourceState = safari.state
      let hostState = app.state
      let snapshot = try? mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey)
      handoffStates.append("Safari=\(sourceState.rawValue) OpenLess=\(hostState.rawValue) fresh=\(snapshot?.isFresh() == true) foreground=\(snapshot?.inForeground == true) capture=\(snapshot?.captureReady == true)")
      return sourceState == .runningForeground && hostState == .runningBackground &&
        snapshot?.isFresh() == true && snapshot?.sessionID == sessionID &&
        snapshot?.inForeground == false && snapshot?.captureReady == true
    }, object: nil)
    XCTAssertEqual(XCTWaiter.wait(for: [returned], timeout: 5), .completed,
      "The source app must finish returning while the same recording runs in the background.\n" +
        handoffStates.joined(separator: "\n"))
    XCTAssertEqual(safari.state, .runningForeground)
    XCTAssertEqual(app.state, .runningBackground)
    capture(safari, name: "cold-voice-auto-return-original-field")
    Thread.sleep(forTimeInterval: 1)
    let stable = try? mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey)
    XCTAssertTrue(stable?.isFresh() == true && stable?.sessionID == sessionID &&
      stable?.captureReady == true && stable?.inForeground == false,
      "The same real recording must remain active after the return animation.")
    safari.buttons["取消录音"].tap()
    for attempt in 1...2 {
      let idle = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "点击说话"), object: speak)
      XCTAssertEqual(XCTWaiter.wait(for: [idle], timeout: 20), .completed, safari.debugDescription)
      Thread.sleep(forTimeInterval: 4)
      speak.tap()
      let active = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "结束录音"), object: speak)
      XCTAssertEqual(XCTWaiter.wait(for: [active], timeout: 10), .completed, safari.debugDescription)
      for _ in 0..<3 {
        XCTAssertEqual(safari.state, .runningForeground)
        XCTAssertNotEqual(app.state, .runningForeground)
        Thread.sleep(forTimeInterval: 1)
      }
      capture(safari, name: "warm-voice-no-jump-\(attempt)")
      safari.buttons["取消录音"].tap()
    }
    let idle = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "点击说话"), object: speak)
    XCTAssertEqual(XCTWaiter.wait(for: [idle], timeout: 20), .completed)
    // Do not tap the textarea again: typing must use the preserved caret.
    selectKeyboardMode("keyboardModeEnglish", in: safari)
    safari.buttons["keyboardKey-x"].tap()
    expectValue("prefix xsuffix", on: editor)
    capture(safari, name: "auto-return-caret-preserved")
    app.terminate()
  }

  private func voiceMailbox() throws -> Mailbox {
    let group: URL
    #if targetEnvironment(simulator)
    if let path = ProcessInfo.processInfo.environment["OPENLESS_TEST_SHARED_CONTAINER"], !path.isEmpty {
      group = URL(fileURLWithPath: path, isDirectory: true)
    } else {
      group = try XCTUnwrap(FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.app.openless.ios"))
    }
    #else
    group = try XCTUnwrap(FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "group.app.openless.ios"))
    #endif
    return try Mailbox(url: group.appendingPathComponent("ipc/mailbox.sqlite"))
  }
  private func verifyVoiceReopening(in safari: XCUIApplication, host app: XCUIApplication,
    editor: XCUIElement) throws {
    let mailbox = try voiceMailbox()
    let speak = safari.buttons["speakButton"]

    func assertForeground() {
      XCTAssertEqual(safari.state, .runningForeground, "Reopening must keep Safari in front.")
      XCTAssertNotEqual(app.state, .runningForeground, "A warm keyboard recording must not reopen OpenLess.")
    }
    func waitFor(_ label: String, timeout: TimeInterval = 30) {
      let deadline = ProcessInfo.processInfo.systemUptime + timeout
      while ProcessInfo.processInfo.systemUptime < deadline {
        assertForeground()
        if speak.exists && speak.label == label { return }
        Thread.sleep(forTimeInterval: 0.25)
      }
      XCTFail("Expected voice control '\(label)' after reopening.\n" + safari.debugDescription)
    }
    func waitWhileHidden(_ seconds: Int) {
      for _ in 0..<seconds {
        assertForeground()
        XCTAssertFalse(speak.exists, "The keyboard must remain dismissed during the idle interval.")
        Thread.sleep(forTimeInterval: 1)
      }
      assertForeground()
    }
    func dismissKeyboard() {
      let dismiss = safari.buttons.matching(NSPredicate(format: "label IN %@", ["完成", "Done"])).firstMatch
      XCTAssertTrue(dismiss.waitForExistence(timeout: 3), safari.debugDescription)
      XCTAssertTrue(dismiss.isHittable, safari.debugDescription)
      dismiss.tap()
      let hidden = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: speak)
      XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 5), .completed, safari.debugDescription)
      assertForeground()
    }
    func reopenKeyboard() {
      assertForeground()
      editor.tap()
      selectOpenLess(safari)
      selectKeyboardMode("keyboardModeVoice", in: safari)
      XCTAssertTrue(speak.waitForExistence(timeout: 8), safari.debugDescription)
      assertForeground()
    }
    func startWarmRecording(_ checkpoint: String) throws -> UUID {
      speak.tap()
      waitFor("结束录音")
      let snapshot = try XCTUnwrap(mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey))
      XCTAssertTrue(snapshot.isFresh())
      XCTAssertTrue(snapshot.captureReady, "The recording label must correspond to real captured audio.")
      XCTAssertEqual(snapshot.phase, .recording)
      XCTAssertFalse(snapshot.inForeground)
      let session = try XCTUnwrap(snapshot.sessionID)
      capture(safari, name: checkpoint)
      return session
    }

    waitFor("点击说话")
    var lastSession: UUID?
    for mode in ["keyboardModeEnglish", "keyboardModePinyin"] {
      selectKeyboardMode(mode, in: safari)
      selectKeyboardMode("keyboardModeVoice", in: safari)
      let recognitionDeadline = ProcessInfo.processInfo.systemUptime + 30
      var recognitionReady = false
      while ProcessInfo.processInfo.systemUptime < recognitionDeadline {
        assertForeground()
        if let snapshot = try mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey),
          snapshot.isFresh(), snapshot.canStart, snapshot.backgroundReady,
          snapshot.capabilities?.sourceLanguage == SpeechRecognitionDefaults.automaticMixedLanguage {
          recognitionReady = true
          break
        }
        Thread.sleep(forTimeInterval: 0.25)
      }
      XCTAssertTrue(recognitionReady, "Changing the keyboard layout must keep automatic mixed-language ASR ready.")
      let session = try startWarmRecording("keyboard-recording-after-layout-\(mode)")
      XCTAssertNotEqual(session, lastSession)
      lastSession = session
      safari.buttons["取消录音"].tap()
      waitFor("点击说话")
    }
    for seconds in [5, 30] {
      dismissKeyboard()
      waitWhileHidden(seconds)
      reopenKeyboard()
      // Tap immediately after reattachment; readiness must be recovered by the
      // explicit request even when the displayed snapshot has not caught up.
      let session = try startWarmRecording("keyboard-recording-after-hidden-\(seconds)s")
      XCTAssertNotEqual(session, lastSession)
      lastSession = session
      safari.buttons["取消录音"].tap()
      waitFor("点击说话")
    }

    // This direct keyboard session must stop when its input destination hides.
    // Read only the real published state; never synthesize recognition output.
    let hiddenSession = try startWarmRecording("keyboard-recording-before-dismissal")
    dismissKeyboard()
    let stopDeadline = ProcessInfo.processInfo.systemUptime + 30
    var stopped = false
    while ProcessInfo.processInfo.systemUptime < stopDeadline {
      assertForeground()
      XCTAssertFalse(speak.exists)
      if let snapshot = try mailbox.value(VoiceSnapshot.self, for: SharedContract.snapshotKey),
        snapshot.isFresh(), snapshot.sessionID == hiddenSession, !snapshot.captureReady,
        [.processing, .done, .cancelled, .error].contains(snapshot.phase) {
        stopped = true
        break
      }
      Thread.sleep(forTimeInterval: 0.25)
    }
    XCTAssertTrue(stopped, "Dismissing a direct keyboard recording must stop microphone capture.")
    capture(safari, name: "keyboard-hidden-after-capture-stopped")
    reopenKeyboard()
    // The previous ASR request may still be processing. A new tap waits for it
    // without a foreground handoff, then starts a distinct real recording.
    let resumedSession = try startWarmRecording("keyboard-recording-after-active-dismissal")
    XCTAssertNotEqual(resumedSession, hiddenSession)
    safari.buttons["取消录音"].tap()
    waitFor("点击说话")
  }

  /// Uses the existing simulator's enabled keyboard/mic grant and the local
  /// textarea fixture served at localhost:8765. No settings are reset.
  func testKeyboardVoiceStartsTwiceWithoutForegroundJump() {
    let app = launch()
    app.tabBars["primaryDock"].buttons["概览"].tap()
    let prepare = app.buttons["prepareSpeech"]
    XCTAssertTrue(prepare.waitForExistence(timeout: 15))
    let enabled = XCTNSPredicateExpectation(predicate: NSPredicate(format: "enabled == true"), object: prepare)
    XCTAssertEqual(XCTWaiter.wait(for: [enabled], timeout: 20), .completed)
    prepare.tap()
    let session = app.buttons["toggleKeyboardVoice"]
    XCTAssertTrue(session.waitForExistence(timeout: 60), app.debugDescription)
    let ready = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS %@", "结束键盘语音会话"), object: session)
    XCTAssertEqual(XCTWaiter.wait(for: [ready], timeout: 10), .completed, app.debugDescription)
    capture(app, name: "keyboard-session-armed-without-recording")
    let safari = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
    safari.activate()
    XCTAssertTrue(safari.wait(for: .runningForeground, timeout: 10))
    let editor = safari.textViews["Routing test input"]
    XCTAssertTrue(editor.waitForExistence(timeout: 15), safari.debugDescription)
    editor.tap()
    selectOpenLess(safari)
    selectKeyboardMode("keyboardModeVoice", in: safari)
    let speak = safari.buttons["speakButton"]
    XCTAssertTrue(speak.waitForExistence(timeout: 10), safari.debugDescription)
    for index in 1...2 {
      let idle = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "点击说话"), object: speak)
      XCTAssertEqual(XCTWaiter.wait(for: [idle], timeout: 15), .completed, safari.debugDescription)
      speak.tap()
      let recording = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "结束录音"), object: speak)
      XCTAssertEqual(XCTWaiter.wait(for: [recording], timeout: 8), .completed, safari.debugDescription)
      for _ in 0..<3 {
        XCTAssertEqual(safari.state, .runningForeground)
        XCTAssertNotEqual(app.state, .runningForeground)
        Thread.sleep(forTimeInterval: 1)
      }
      capture(safari, name: "keyboard-background-recording-\(index)")
      speak.tap()
      let stopped = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "点击说话"), object: speak)
      XCTAssertEqual(XCTWaiter.wait(for: [stopped], timeout: 30), .completed, safari.debugDescription)
      XCTAssertEqual(safari.state, .runningForeground)
      XCTAssertNotEqual(app.state, .runningForeground)
      capture(safari, name: "keyboard-background-stopped-\(index)")
      Thread.sleep(forTimeInterval: 5)
    }
    app.activate()
    XCTAssertTrue(session.waitForExistence(timeout: 10))
    session.tap()
    let ended = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label == %@", "启用键盘语音"), object: session)
    XCTAssertEqual(XCTWaiter.wait(for: [ended], timeout: 5), .completed)
  }

}

#if targetEnvironment(simulator)
/// A protocol peer confined to tests; it never accesses a microphone or a model.
private final class HeldVoiceFixtureHost: @unchecked Sendable {
  private let mailbox: Mailbox
  private let queue = DispatchQueue(label: "OpenLess.HeldVoiceFixture")
  private var timer: DispatchSourceTimer?
  private var snapshot = VoiceSnapshot(hostEpoch: UUID())
  private var target: InputTarget?
  private var choice: SpaceVoiceFinish?
  private var wasCancelled = false
  private var errors: [String] = []
  private var lastInsertion: InsertionRequest?
  private let startedAt = Date()
  init(mailbox: Mailbox) {
    self.mailbox = mailbox
    var capabilities = KeyboardVoiceCapabilities(revision: UUID(), sourceLanguage: "auto",
      translationTargets: [.init(id: "English", title: "English")],
      styles: [.init(id: "builtin.raw", title: "Raw")], canTranslate: true, canHelpWrite: false)
    capabilities.supportsSpaceVoice = true
    snapshot.capabilities = capabilities
    snapshot.inForeground = false
    snapshot.backgroundReady = true
    snapshot.canStart = true
    snapshot.phase = .ready
  }
  var finish: SpaceVoiceFinish.Kind? { queue.sync { choice?.kind } }
  var outputLanguage: String? { queue.sync { choice?.targetLanguage } }
  var cancelled: Bool { queue.sync { wasCancelled } }
  var failures: [String] { queue.sync { errors } }
  func start() {
    queue.sync {
      publish()
      let timer = DispatchSource.makeTimerSource(queue: queue)
      timer.schedule(deadline: .now(), repeating: .milliseconds(50))
      timer.setEventHandler { [weak self] in self?.tick() }
      self.timer = timer
      timer.resume()
    }
  }
  func stop() {
    queue.sync {
      timer?.cancel(); timer = nil
      snapshot.phase = .inactive; snapshot.canStart = false
      snapshot.captureReady = false; snapshot.insertion = nil
      publish()
    }
  }
  func reset() {
    queue.sync {
      choice = nil; wasCancelled = false; lastInsertion = nil; target = nil
      snapshot.sessionID = nil; snapshot.phase = .ready
      snapshot.captureReady = false; snapshot.canStart = true; snapshot.insertion = nil
      publish()
    }
  }
  func publishLateRecording() {
    queue.sync {
      snapshot.phase = .recording; snapshot.captureReady = true
      publish()
    }
  }
  func complete(_ text: String) {
    queue.sync {
      guard let session = snapshot.sessionID, let target, !wasCancelled else {
        errors.append("Missing active delivery target"); return
      }
      do {
        let insertion = InsertionRequest(sessionID: session, target: target, text: text)
        try mailbox.storeInsertion(insertion)
        lastInsertion = insertion
        snapshot.insertion = insertion; snapshot.phase = .awaitingInsertion
        snapshot.captureReady = false
        publish()
      } catch { errors.append("Could not store fixture insertion") }
    }
  }
  func repeatDelivery() {
    queue.sync {
      snapshot.insertion = lastInsertion; snapshot.phase = .awaitingInsertion
      publish()
    }
  }
  private func tick() {
    do {
      for command in try mailbox.claimCommands() where command.createdAt >= startedAt {
        switch command.kind {
        case .start:
          guard command.deferredSpaceVoice == true else { errors.append("Missing deferred delivery"); continue }
          target = command.target; snapshot.sessionID = command.sessionID
          snapshot.activeIntent = command.intent
          snapshot.phase = .recording; snapshot.captureReady = true
          snapshot.canStart = false; snapshot.level = 0.04
        case .stop:
          guard command.sessionID == snapshot.sessionID else { continue }
          choice = command.spaceVoiceFinish
          snapshot.phase = .processing; snapshot.captureReady = false; snapshot.level = 0
        case .finishSpaceTranslation:
          guard command.sessionID == snapshot.sessionID else { continue }
          choice = command.spaceVoiceFinish
        case .cancel:
          guard command.sessionID == snapshot.sessionID else { continue }
          wasCancelled = true
          snapshot.phase = .cancelled; snapshot.captureReady = false; snapshot.canStart = true
          snapshot.insertion = nil
        case .insertionReceipt:
          guard command.sessionID == snapshot.sessionID else { continue }
          snapshot.phase = .done; snapshot.canStart = true; snapshot.insertion = nil
        default: break
        }
      }
    } catch { errors.append("Could not claim fixture commands") }
    publish()
  }
  private func publish() {
    snapshot.updatedAt = Date()
    do { try mailbox.set(snapshot, for: SharedContract.snapshotKey) }
    catch { errors.append("Could not publish fixture snapshot") }
    CFNotificationCenterPostNotification(CFNotificationCenterGetDarwinNotifyCenter(),
      CFNotificationName(rawValue: "group.app.openless.ios.state.v2" as CFString), nil, nil, true)
  }
}
#endif
