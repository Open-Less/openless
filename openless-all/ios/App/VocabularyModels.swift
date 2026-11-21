import Foundation

/// Source is Core's classification, not a claim about which device/person added a word.
enum VocabularySource: String, CaseIterable, Identifiable {
  case all, learned, manual, unknown
  var id: String { rawValue }
  static let filters: [Self] = [.all, .learned, .manual]
  var title: String {
    switch self {
    case .all: "所有"
    case .learned: "自动添加"
    case .manual: "手动添加"
    case .unknown: "来源未知"
    }
  }
  var symbol: String {
    switch self {
    case .all: "tray"
    case .learned: "sparkles"
    case .manual: "pencil.tip"
    case .unknown: "questionmark.circle"
    }
  }
}

struct CoreVocabularyEntry: Decodable, Identifiable {
  let id: String
  let phrase: String
  let note: String?
  let enabled: Bool
  let hits: UInt64
  let createdAt: String
  private let source: String?

  // Keep this exact fallback aligned with Core shared_types::LEARNED_VOCAB_NOTE.
  static let learnedNote = "从手改中自动收集"
  var classifiedSource: VocabularySource {
    if let source {
      return [.learned, .manual].first { $0.rawValue == source } ?? .unknown
    }
    // Core currently classifies all non-marker entries (including legacy/imports) as manual.
    return note == Self.learnedNote ? .learned : .manual
  }
}

struct VocabularySelection: Equatable {
  var query = ""
  var source: VocabularySource = .all

  func request(offset: Int) -> [String: Any] {
    ["action": "vocabulary", "query": query, "source": source.rawValue, "offset": offset]
  }
}

enum VocabularyInput {
  static func validationMessage(_ phrase: String) -> String? {
    if phrase.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "请输入单词。" }
    if phrase.contains(where: { $0.isNewline }) { return "词条不能包含换行。" }
    if phrase.utf8.count > 512 { return "词条不能超过 512 字节，请缩短后再试。" }
    return nil
  }
  static func saveRequest(_ phrase: String, entryID: String?) -> [String: Any] {
    var request: [String: Any] = [
      "action": entryID == nil ? "addVocabulary" : "updateVocabulary",
      "phrase": phrase.trimmingCharacters(in: .whitespacesAndNewlines)
    ]
    if let entryID { request["id"] = entryID }
    return request
  }
}
