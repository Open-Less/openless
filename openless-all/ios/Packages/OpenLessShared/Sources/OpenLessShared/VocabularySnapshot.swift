import Foundation

/// Only explicitly confirmed phrases, never audio, credentials, userdb or notes.
/// `localOffline` is NOT an authenticated account and is not eligible for sync.
public struct VocabularySnapshot: Codable, Equatable, Sendable {
  public static let mailboxKey = "vocabulary.confirmed.v1"
  public static let maxBytes = 64 * 1024
  public static let maxEntries = 256
  public let schemaVersion: Int
  public let scope: String
  public let generation: UInt64
  public let entries: [ConfirmedVocabularyEntry]

  public init(generation: UInt64, entries: [ConfirmedVocabularyEntry]) {
    schemaVersion = 1
    scope = "localOffline"
    self.generation = generation
    self.entries = entries
  }
  public func validate() throws {
    guard schemaVersion == 1, scope == "localOffline", entries.count <= Self.maxEntries,
      Set(entries.map(\.id)).count == entries.count,
      Set(entries.map(\.phrase)).count == entries.count,
      entries.allSatisfy({ !$0.id.isEmpty && $0.id.utf8.count <= 128
        && VocabularyValidation.phrase($0.phrase)
        && $0.confirmationGeneration > 0 && $0.confirmationGeneration <= generation }),
      try JSONEncoder().encode(self).count <= Self.maxBytes
    else { throw VocabularyValidation.invalid }
  }
  public static func decode(_ data: Data) throws -> Self {
    guard data.count <= maxBytes else { throw VocabularyValidation.invalid }
    let value = try JSONDecoder().decode(Self.self, from: data)
    try value.validate()
    return value
  }
}
public struct ConfirmedVocabularyEntry: Codable, Equatable, Sendable {
  public let id: String
  public let phrase: String
  public let confirmationGeneration: UInt64
  public init(id: String, phrase: String, confirmationGeneration: UInt64) {
    self.id = id
    self.phrase = phrase
    self.confirmationGeneration = confirmationGeneration
  }
}
public enum VocabularyValidation: Error {
  case invalid
  public static func phrase(_ value: String) -> Bool {
    !value.isEmpty && value == value.trimmingCharacters(in: .whitespacesAndNewlines)
      && value.unicodeScalars.count <= 64
      && !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
  }
}
