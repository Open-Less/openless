import Foundation

/// Core manual-correction suggestions, not an ASR term detector. A UI should
/// Labels these as correction suggestions. Showing or dismissing never adds
/// words or changes text.
public struct VocabularyProposals: Codable, Equatable, Sendable {
  public static let mailboxKey = "vocabulary.proposals.v1"
  public let schemaVersion: Int
  public let scope: String
  public let generation: UInt64
  public let kind: String
  public let proposals: [VocabularyProposal]

  public init(generation: UInt64, proposals: [VocabularyProposal]) {
    schemaVersion = 1
    scope = "localOffline"
    self.generation = generation
    kind = "correction"
    self.proposals = proposals
  }
  public func validate() throws {
    guard schemaVersion == 1, scope == "localOffline", kind == "correction", proposals.count <= 16,
      Set(proposals.map(\.id)).count == proposals.count,
      Set(proposals.map(\.phrase)).count == proposals.count,
      proposals.allSatisfy({ !$0.id.isEmpty && $0.id.utf8.count <= 128
        && VocabularyValidation.phrase($0.phrase) && $0.expiresAtMs > 0 }),
      try JSONEncoder().encode(self).count <= VocabularySnapshot.maxBytes
    else { throw VocabularyValidation.invalid }
  }
  public func visible(now: Date = Date()) -> [VocabularyProposal] {
    proposals.filter { Double($0.expiresAtMs) > now.timeIntervalSince1970 * 1000 }
  }
}
public struct VocabularyProposal: Codable, Equatable, Sendable {
  public let id: String
  public let phrase: String
  public let expiresAtMs: Int64
  public init(id: String, phrase: String, expiresAtMs: Int64) {
    self.id = id
    self.phrase = phrase
    self.expiresAtMs = expiresAtMs
  }
}
/// Typed UI hook for parent IPC integration. No microphone or insertion intent.
public struct VocabularyDecision: Codable, Equatable, Sendable {
  public enum Action: String, Codable, Sendable { case accept, reject, dismiss }
  public let action: Action
  public let proposalID: String?
  public let expectedGeneration: UInt64
  public init(action: Action, proposalID: String?, expectedGeneration: UInt64) {
    self.action = action
    self.proposalID = proposalID
    self.expectedGeneration = expectedGeneration
  }
}
