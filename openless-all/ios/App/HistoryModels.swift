import Foundation

struct CoreHistoryEntry: Decodable, Identifiable {
  let id: String
  let createdAt: String
  let finalText: String
  let rawTranscript: String
  let errorCode: String?
  let durationMs: UInt64?
  let mode: String?
  let stylePackId: String?
  let hasAudioRecording: Bool?
  let asrTranscript: String?
  let asrProvider: String?
  let asrModel: String?
  let llmProvider: String?
  let llmModel: String?
  let characterCount: UInt64?
  let isPreview: Bool?

  var displayText: String { finalText.isEmpty ? rawTranscript : finalText }
  var originalText: String { asrTranscript ?? rawTranscript }
  var date: Date? { HistoryFormatting.date(createdAt) }
  var canExportAudio: Bool {
    hasAudioRecording == true && ["recording", "recordingFailed", "transcribeFailed", "emptyTranscript", "cancelled", "interrupted"].contains(errorCode ?? "")
  }
  var statusTitle: String {
    switch errorCode {
    case nil: "已完成"
    case "recording": "未完成录音"
    case "cancelled": "已取消"
    case "transcribeFailed": "识别未完成"
    case "emptyTranscript": "未识别到文字"
    case "polishFailed": "润色未完成，原文已保留"
    case "insertFailed": "文字已保留，输入未完成"
    default: "未完成"
    }
  }
}
struct CoreActivityDay: Decodable, Identifiable {
  let date: String
  let count: UInt64
  let chars: UInt64
  let durationMs: UInt64
  var id: String { date }
}
struct LibraryStyle: Decodable, Identifiable { let id: String; let name: String }
struct ActivityPoint: Identifiable {
  let date: Date
  let count: UInt64
  let chars: UInt64
  let durationMs: UInt64
  var id: Date { date }
}
struct TodayMetrics {
  let characters: UInt64
  let durationMs: UInt64
  let count: Int
  var averageDurationMs: UInt64 { count == 0 ? 0 : durationMs / UInt64(count) }
  init(history: [CoreHistoryEntry], now: Date = .now, calendar: Calendar = .current) {
    let start = calendar.startOfDay(for: now)
    let end = calendar.date(byAdding: .day, value: 1, to: start) ?? now
    let today = history.filter { entry in entry.date.map { $0 >= start && $0 < end } ?? false }
    count = today.count
    characters = HistoryFormatting.sum(today.map { $0.characterCount ?? UInt64($0.finalText.unicodeScalars.count) })
    durationMs = HistoryFormatting.sum(today.map { $0.durationMs ?? 0 })
  }
}
enum HistoryFormatting {
  static func newestFirst(_ entries: [CoreHistoryEntry]) -> [CoreHistoryEntry] {
    // Parse once per row. Equal or absent dates keep their original relative order.
    let dated: [(index: Int, entry: CoreHistoryEntry, date: Date)] = entries.enumerated().map {
      (index: $0.offset, entry: $0.element, date: $0.element.date ?? Date.distantPast)
    }
    let ordered = dated.sorted { lhs, rhs in
      lhs.date == rhs.date ? lhs.index < rhs.index : lhs.date > rhs.date
    }
    return ordered.map { $0.entry }
  }
  private static let iso = ISO8601DateFormatter()
  private static let fractional: ISO8601DateFormatter = {
    let value = ISO8601DateFormatter(); value.formatOptions.insert(.withFractionalSeconds); return value
  }()
  static func date(_ value: String) -> Date? { fractional.date(from: value) ?? iso.date(from: value) }
  static func duration(_ milliseconds: UInt64) -> String {
    let seconds = milliseconds / 1000
    if seconds >= 3600 { return "\(seconds / 3600)小时\((seconds % 3600) / 60)分" }
    if seconds >= 60 { return "\(seconds / 60)分\(seconds % 60)秒" }
    return "\(seconds)秒"
  }
  static func sum(_ values: [UInt64]) -> UInt64 {
    values.reduce(0) { sum, value in let result = sum.addingReportingOverflow(value); return result.overflow ? .max : result.partialValue }
  }
  static func series(_ days: [CoreActivityDay], period: Int, now: Date = .now, calendar: Calendar = .current) -> [ActivityPoint] {
    let byDate = Dictionary(days.map { ($0.date, $0) }, uniquingKeysWith: { _, last in last })
    let formatter = DateFormatter(); formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.calendar = Calendar(identifier: .gregorian); formatter.timeZone = calendar.timeZone
    formatter.dateFormat = "yyyy-MM-dd"
    return (0..<max(0, min(period, 30))).reversed().compactMap { offset in
      guard let date = calendar.date(byAdding: .day, value: -offset, to: calendar.startOfDay(for: now)) else { return nil }
      let day = byDate[formatter.string(from: date)]
      return ActivityPoint(date: date, count: day?.count ?? 0, chars: day?.chars ?? 0, durationMs: day?.durationMs ?? 0)
    }
  }
}
