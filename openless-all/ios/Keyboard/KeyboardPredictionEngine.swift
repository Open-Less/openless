import Foundation
import SQLite3

/// A read-only, bounded-cache statistical dictionary. No document or user words
/// are persisted, and no network/full-access permission is required.
nonisolated final class KeyboardPredictionEngine {
  struct SupplementaryEntry: Sendable, Equatable {
    let input: String
    let text: String
    var reading: String? = nil
  }
  struct EnglishInput: Equatable {
    let prefix: String
    let previousWord: String
    let startsSentence: Bool

    static func parse(_ context: String) -> Self {
      let context = String(context.suffix(160))
      let prefix = String(context.reversed().prefix(while: isWordCharacter).reversed())
      let preceding = String(context.dropLast(prefix.count))
      let trimmed = preceding.trimmingCharacters(in: .whitespaces)
      let previous = KeyboardPredictionEngine.normalized(String(trimmed.reversed().prefix(while: isWordCharacter).reversed()))
      let startsSentence = trimmed.isEmpty || trimmed.last.map { ".!?\n".contains($0) } == true
      return Self(prefix: prefix, previousWord: startsSentence ? "" : previous, startsSentence: startsSentence)
    }

    static func isWordCharacter(_ character: Character) -> Bool {
      character == "'" || character == "’" || character.isLetter && character.unicodeScalars.allSatisfy {
        $0.isASCII || (0x00c0...0x024f).contains($0.value)
          || (0x0300...0x036f).contains($0.value)
      }
    }
  }

  private var database: OpaquePointer?
  private let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
  private var supplementary: [SupplementaryEntry] = []

  private static func normalized(_ text: String) -> String {
    text.replacingOccurrences(of: "’", with: "'")
      .folding(options: [.caseInsensitive, .diacriticInsensitive], locale: Locale(identifier: "en_US"))
  }

  /// Supplementary words stay in memory and are never copied to the user dictionary.
  func setSupplementaryEntries(_ entries: [SupplementaryEntry]) {
    supplementary = entries.prefix(2048).filter {
      !$0.input.isEmpty && !$0.text.isEmpty && $0.input.utf8.count <= 128 && $0.text.utf8.count <= 512
        && !$0.text.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
    }.map { .init(input: Self.normalized($0.input), text: $0.text, reading: $0.reading) }
  }

  static func reading(for phrase: String) -> String? {
    guard !phrase.isEmpty, phrase.count <= 32,
      phrase.unicodeScalars.allSatisfy({ (0x3400...0x9fff).contains($0.value) }),
      let latin = phrase.applyingTransform(.toLatin, reverse: false),
      let plain = latin.decomposedStringWithCanonicalMapping
        .replacingOccurrences(of: "u\u{0308}", with: "v", options: .literal)
        .applyingTransform(.stripCombiningMarks, reverse: false) else { return nil }
    let reading = plain.replacingOccurrences(of: " ", with: "").lowercased()
    return reading.utf8.allSatisfy { (97...122).contains($0) } ? reading : nil
  }

  func supplementaryCandidates(for prefix: String, chinese: Bool = false, limit: Int = 3) -> [String] {
    var key = Self.normalized(prefix.replacingOccurrences(of: " ", with: ""))
    if chinese { key = key.replacingOccurrences(of: "'", with: "") }
    guard !key.isEmpty, key.count <= 64, limit > 0 else { return [] }
    var exact: [String] = [], partial: [String] = []
    for entry in supplementary {
      let input = entry.input
      let reading = chinese ? entry.reading : nil
      if input == key || reading == key { exact.append(entry.text) }
      else if key.count >= 2 && (input.hasPrefix(key) || reading?.hasPrefix(key) == true) {
        partial.append(entry.text)
      }
    }
    var seen = Set<String>()
    return (exact + partial).filter { seen.insert($0).inserted }.prefix(limit).map { $0 }
  }

  init(url: URL?) {
    guard let url else { return }
    if sqlite3_open_v2(url.path, &database, SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX, nil) != SQLITE_OK {
      sqlite3_close(database)
      database = nil
      return
    }
    sqlite3_exec(database, "PRAGMA query_only=ON; PRAGMA cache_size=-512; PRAGMA mmap_size=0;", nil, nil, nil)
  }

  deinit { sqlite3_close(database) }

  func englishCandidates(
    for input: EnglishInput, corrections: [String] = [], completions: [String] = [], limit: Int = 8
  ) -> [String] {
    let prefix = Self.normalized(input.prefix)
    guard limit > 0, prefix.count <= 32 else { return [] }
    var words: [String] = []
    if !prefix.isEmpty { words.append(input.prefix) }
    let personal = supplementaryCandidates(for: input.prefix)
    words += personal
    words += corrections.prefix(2)
    // Explicit next-word predictions disappear at sentence boundaries. Empty
    // editors keep their mode toolbar, instead of inventing a first word.
    if !input.previousWord.isEmpty {
      words += query(
        "SELECT word FROM bigram WHERE context=? AND word>=? AND word<? ORDER BY frequency DESC LIMIT 8",
        [input.previousWord, prefix, prefix + "{"])
    }
    if !prefix.isEmpty {
      words += query(
        "SELECT word FROM english WHERE word>=? AND word<? ORDER BY frequency DESC LIMIT 12",
        [prefix, prefix + "{"])
      words += completions
    }
    var seen = Set<String>()
    return words.compactMap { word -> String? in
      let personalCaseChange = personal.contains(word) && word != input.prefix
        && word.lowercased() == input.prefix.lowercased()
      guard personalCaseChange || seen.insert(word.lowercased()).inserted else { return nil }
      if word == input.prefix { return word }
      if personal.contains(word) { return word }
      if input.prefix.count > 1 && input.prefix == input.prefix.uppercased() {
        return word.uppercased()
      }
      if input.startsSentence || input.prefix.first?.isUppercase == true {
        return word.prefix(1).uppercased() + word.dropFirst()
      }
      // The English first-person pronoun is capitalized even mid-sentence.
      return word == "i" ? "I" : word
    }.prefix(max(0, limit)).map { $0 }
  }

  /// Returns only the untyped suffix, not the whole phrase. Prefer the longest
  /// matching context; back off to shorter Han suffixes when needed.
  func chineseCandidates(before context: String, limit: Int = 8) -> [String] {
    guard limit > 0 else { return [] }
    let tail = String(context.reversed().prefix(while: { character in
      character.unicodeScalars.count == 1 &&
        character.unicodeScalars.allSatisfy { (0x3400...0x9fff).contains($0.value) }
    }).prefix(4).reversed())
    guard !tail.isEmpty else { return [] }
    var seen = Set<String>()
    var result: [String] = []
    for length in stride(from: tail.count, through: 1, by: -1) {
      let words = query(
        "SELECT word FROM chinese WHERE context=? ORDER BY frequency DESC LIMIT 8",
        [String(tail.suffix(length))])
      for word in words where seen.insert(word).inserted {
        result.append(word)
        if result.count >= limit { return result }
      }
    }
    return result
  }

  private func query(_ sql: String, _ arguments: [String]) -> [String] {
    guard let database else { return [] }
    var statement: OpaquePointer?
    guard sqlite3_prepare_v2(database, sql, -1, &statement, nil) == SQLITE_OK else { return [] }
    defer { sqlite3_finalize(statement) }
    for (index, argument) in arguments.enumerated() {
      sqlite3_bind_text(statement, Int32(index + 1), argument, -1, transient)
    }
    var result: [String] = []
    while sqlite3_step(statement) == SQLITE_ROW {
      if let word = sqlite3_column_text(statement, 0) {
        result.append(String(cString: word))
      }
    }
    return result
  }
}
