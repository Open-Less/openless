import Foundation
import Testing
@testable import OpenLessShared

@Suite struct VocabularyContractTests {
  let entry = ConfirmedVocabularyEntry(id: "core-id", phrase: "量子桥", confirmationGeneration: 1)
  @Test func confirmedSnapshotRoundTripAndMailboxProjection() throws {
    let value = VocabularySnapshot(generation: 2, entries: [entry])
    let data = try JSONEncoder().encode(value)
    #expect(try VocabularySnapshot.decode(data) == value)
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let app = try Mailbox(url: directory.appendingPathComponent("test.sqlite"))
    let keyboard = try Mailbox(url: directory.appendingPathComponent("test.sqlite"))
    try app.set(value, for: VocabularySnapshot.mailboxKey)
    let received = try #require(try keyboard.value(VocabularySnapshot.self, for: VocabularySnapshot.mailboxKey))
    try received.validate()
    #expect(received == value)
    let json = String(decoding: data, as: UTF8.self)
    #expect(!json.contains("note"))
    #expect(!json.contains("fingerprint"))
  }
  @Test func unconfirmedAndDuplicateEntriesAreRejected() throws {
    #expect(throws: (any Error).self) { try VocabularySnapshot(generation: 1, entries: [entry, entry]).validate() }
    let unconfirmed = ConfirmedVocabularyEntry(id: "id", phrase: "量子桥", confirmationGeneration: 0)
    #expect(throws: (any Error).self) { try VocabularySnapshot(generation: 1, entries: [unconfirmed]).validate() }
    #expect(throws: (any Error).self) { try VocabularySnapshot(generation: 0, entries: [entry]).validate() }
  }
  @Test func payloadVersionScopeAndBoundsAreFailClosed() throws {
    let value = VocabularySnapshot(generation: 1, entries: [entry])
    let encoded = String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
    for invalid in [encoded.replacingOccurrences(of: "localOffline", with: "github:123"),
                    encoded.replacingOccurrences(of: "\"schemaVersion\":1", with: "\"schemaVersion\":2")] {
      #expect(throws: (any Error).self) { try VocabularySnapshot.decode(Data(invalid.utf8)) }
    }
    #expect(throws: (any Error).self) { try VocabularySnapshot.decode(Data(repeating: 32, count: 65537)) }
    #expect(!VocabularyValidation.phrase("first\nsecond"))
    #expect(!VocabularyValidation.phrase(String(repeating: "长", count: 65)))
    #expect(!VocabularyValidation.phrase(" word "))
    #expect(VocabularyValidation.phrase("OpenLess")) // Stored but not parsed as Mandarin by Rime.
  }
  @Test func proposalsAreCorrectionOnlyAndExpireWithoutLearning() throws {
    let value = VocabularyProposals(generation: 3, proposals: [
      VocabularyProposal(id: "expired", phrase: "旧词", expiresAtMs: 1000),
      VocabularyProposal(id: "live", phrase: "新词", expiresAtMs: 3000)
    ])
    try value.validate()
    #expect(value.kind == "correction")
    #expect(value.visible(now: Date(timeIntervalSince1970: 2)).map(\.id) == ["live"])
    #expect(throws: (any Error).self) { try VocabularyProposals(generation: 3, proposals: value.proposals + value.proposals).validate() }
  }
  @Test func decisionsRoundTripWithoutVoiceOrInsertionPayload() throws {
    for action in [VocabularyDecision.Action.accept, .reject, .dismiss] {
      let decision = VocabularyDecision(action: action, proposalID: action == .dismiss ? nil : "real-id", expectedGeneration: 3)
      let data = try JSONEncoder().encode(decision)
      #expect(try JSONDecoder().decode(VocabularyDecision.self, from: data) == decision)
      #expect(!String(decoding: data, as: UTF8.self).contains("text"))
    }
  }
}
