import Foundation
import XCTest
@testable import OpenLessShared

final class InterfaceLocalizerTests: XCTestCase {
  private func withLocalizers(_ body: (InterfaceLocalizer, InterfaceLocalizer, Bundle) throws -> Void) throws {
    var root = URL(fileURLWithPath: #filePath)
    for _ in 0..<5 { root.deleteLastPathComponent() }
    let resources = root.appendingPathComponent("Resources")
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString).bundle")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    for language in ["en", "zh-Hans"] {
      try FileManager.default.copyItem(at: resources.appendingPathComponent("\(language).lproj"),
                                       to: directory.appendingPathComponent("\(language).lproj"))
    }
    let info = ["CFBundleIdentifier": "app.openless.localization-test", "CFBundleDevelopmentRegion": "en"]
    try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
      .write(to: directory.appendingPathComponent("Info.plist"))
    let bundle = try XCTUnwrap(Bundle(url: directory))
    try body(InterfaceLocalizer(language: .english, bundle: bundle),
             InterfaceLocalizer(language: .simplifiedChinese, bundle: bundle), bundle)
  }

  func testRealResourceLookupExplicitAndSystemLanguage() throws {
    try withLocalizers { en, zh, bundle in
      XCTAssertEqual(en.text("设置"), "Settings")
      XCTAssertEqual(zh.text("设置"), "设置")
      XCTAssertEqual(en.text("unknown.key"), "unknown.key")
      let system = InterfaceLocalizer(language: .system, bundle: bundle, preferredLanguages: ["zh-CN"])
      XCTAssertEqual(system.text("跟随系统"), "跟随系统")
    }
  }

  func testAllDynamicHelpersUseRealLocalizedFormatsAndDoNotInterpretContent() throws {
    try withLocalizers { en, zh, _ in
      XCTAssertEqual(en.recordCount(1), "1 record")
      XCTAssertEqual(en.recordCount(2), "2 records")
      XCTAssertEqual(zh.recordCount(2), "2 段记录")
      XCTAssertEqual(en.deviceLoginTiming(remaining: 30, interval: 5), "About 30 seconds remaining; checks at least 5 seconds apart.")
      XCTAssertEqual(zh.deviceLoginTiming(remaining: 30, interval: 5), "剩余约 30 秒；检查间隔至少 5 秒。")
      XCTAssertEqual(en.marketplaceCounts(downloads: 12, likes: 3), "Downloads 12 · Likes 3")
      XCTAssertEqual(en.recordingSecondsRemaining(9), "9 seconds remaining; recording stops at the limit")
      XCTAssertEqual(en.capabilityStatus("future"), "Core capability status: future")
      XCTAssertEqual(en.sharedStorageError("read"), "Shared storage unavailable (read)")
      XCTAssertEqual(en.repolishStyle("%@ 原文"), "Re-polish · %@ 原文")
      XCTAssertEqual(en.copyTitle("Original recognition text"), "Copy Original recognition text")
      XCTAssertEqual(en.temporaryArchiveCleanupError("disk"), "Temporary ZIP cleanup failed: disk")
      XCTAssertEqual(en.asrDescription("SenseVoice", developerMode: false), "SenseVoice. Standard mode uses local ASR only, without LLM post-processing.")
      XCTAssertTrue(en.asrDescription("Custom", developerMode: true).contains("You configure"))
      XCTAssertTrue(en.activityTrendNote(duration: true).contains("Chart unit: minutes."))
      XCTAssertFalse(en.activityTrendNote(duration: false).contains("Chart unit"))
      XCTAssertEqual(en.duration(seconds: 3661), "1h 1m")
      XCTAssertEqual(en.duration(seconds: 61), "1m 1s")
      XCTAssertEqual(en.duration(seconds: -1), "0s")
      XCTAssertEqual(zh.duration(seconds: 3661), "1小时1分")
      XCTAssertEqual(zh.duration(seconds: 61), "1分1秒")
      XCTAssertEqual(zh.duration(seconds: 1), "1秒")
    }
  }
}
