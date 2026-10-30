import Foundation
import Testing
@testable import OpenLess

@MainActor
struct HistoryModelsTests {
  @Test func newestFirstUsesInstantsAndKeepsEqualOrUnknownDatesStable() throws {
    let rows = try [
      record(["id": "unknown-first", "createdAt": ""]),
      record(["id": "offset", "createdAt": "2026-10-17T09:00:00+08:00"]),
      record(["id": "newest", "createdAt": "2026-10-17T02:00:00.500Z"]),
      record(["id": "same-instant", "createdAt": "2026-10-17T01:00:00Z"]),
      record(["id": "unknown-second", "createdAt": "invalid"])
    ]
    #expect(HistoryFormatting.newestFirst(rows).map(\.id) ==
      ["newest", "offset", "same-instant", "unknown-first", "unknown-second"])
  }
  private func record(_ fields: [String: Any]) throws -> CoreHistoryEntry {
    var json: [String: Any] = ["id": UUID().uuidString, "createdAt": "2026-10-16T00:30:00Z", "rawTranscript": "原文", "finalText": "你好👩🏽‍💻"]
    json.merge(fields) { _, value in value }
    return try JSONDecoder().decode(CoreHistoryEntry.self, from: JSONSerialization.data(withJSONObject: json))
  }
  @Test func exportRequiresActualAudioAndAnUnfinishedTranscription() throws {
    for code in ["recording", "recordingFailed", "transcribeFailed", "emptyTranscript", "cancelled"] {
      #expect(try record(["hasAudioRecording": true, "errorCode": code]).canExportAudio)
      #expect(try !record(["hasAudioRecording": false, "errorCode": code]).canExportAudio)
    }
    #expect(try !record(["hasAudioRecording": true]).canExportAudio)
    #expect(try !record(["hasAudioRecording": true, "errorCode": "polishFailed"]).canExportAudio)
    #expect(try !record(["hasAudioRecording": true, "errorCode": "insertFailed"]).canExportAudio)
  }
  @Test func todayUsesLocalBoundaryFullCharacterCountAndDesktopAverageMeaning() throws {
    var calendar = Calendar(identifier: .gregorian); calendar.timeZone = TimeZone(secondsFromGMT: -3600)!
    let now = try #require(HistoryFormatting.date("2026-10-16T12:00:00Z"))
    let data = [
      try record(["createdAt": "2026-10-16T00:30:00Z", "durationMs": 9000]),
      try record(["createdAt": "2026-10-16T01:30:00Z", "durationMs": 2000, "characterCount": 1000, "isPreview": true]),
      try record(["createdAt": "2026-10-16T02:30:00Z", "durationMs": 4000, "finalText": "你好👩🏽‍💻"]),
      try record(["createdAt": "2026-10-17T02:30:00Z", "durationMs": 9000])
    ]
    let metrics = TodayMetrics(history: data, now: now, calendar: calendar)
    #expect(metrics.count == 2)
    #expect(metrics.characters == 1000 + UInt64("你好👩🏽‍💻".unicodeScalars.count))
    #expect(metrics.durationMs == 6000)
    #expect(metrics.averageDurationMs == 3000)
  }
  @Test func activityPeriodIsIndependentOfHistoryAndFillsMissingDays() throws {
    let days = try JSONDecoder().decode([CoreActivityDay].self, from: Data("""
      [{"date":"2026-10-10","count":3,"chars":30,"durationMs":6000},
       {"date":"2026-10-16","count":1,"chars":5,"durationMs":1000},
       {"date":"2026-10-17","count":99,"chars":999,"durationMs":999}]
      """.utf8))
    var calendar = Calendar(identifier: .gregorian); calendar.timeZone = TimeZone(secondsFromGMT: 0)!
    let now = try #require(HistoryFormatting.date("2026-10-16T12:00:00Z"))
    let points = HistoryFormatting.series(days, period: 7, now: now, calendar: calendar)
    #expect(points.count == 7)
    #expect(HistoryFormatting.sum(points.map(\.chars)) == 35)
    #expect(points[1].count == 0)
    #expect(HistoryFormatting.series(days, period: 30, now: now, calendar: calendar).count == 30)
    #expect(TodayMetrics(history: [], now: now, calendar: calendar).characters == 0)
    #expect(HistoryFormatting.sum([UInt64.max, 1]) == UInt64.max)
  }
  @Test func originalTextIsNotTakenFromFinalOrTruncatedPreview() throws {
    let entry = try record(["asrTranscript": "真正原始识别", "rawTranscript": "纠正后的识别", "finalText": "润色结果"])
    #expect(entry.originalText == "真正原始识别")
    #expect(entry.displayText == "润色结果")
  }
}
