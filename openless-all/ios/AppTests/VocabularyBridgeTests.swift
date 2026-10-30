import Foundation
import OpenLessShared
import XCTest
#if canImport(OpenLess)
@testable import OpenLess
#elseif canImport(VocabularyBridgeHarness)
@testable import VocabularyBridgeHarness
#endif

nonisolated final class VocabularyBridgeTests: XCTestCase {
  private func fixture() -> VocabularyProposals {
    VocabularyProposals(generation: 0, proposals: [VocabularyProposal(id: "real-core-id", phrase: "量子桥", expiresAtMs: 4_000_000_000_000)])
  }
  @MainActor func testRefreshOnlyProjectsAndNeverConfirms() async throws {
    var commands: [String] = []
    let candidates = fixture()
    let bridge = VocabularyBridge(control: { command in
      XCTAssertEqual(command["action"] as? String, "vocabularyBridge")
      let request = try XCTUnwrap(command["request"] as? [String: Any])
      let operation = try XCTUnwrap(request["command"] as? String)
      commands.append(operation)
      if operation == "snapshot" { return try JSONEncoder().encode(VocabularySnapshot(generation: 0, entries: [])) }
      return try JSONEncoder().encode(candidates)
    }, mailbox: nil)
    try await bridge.refresh()
    XCTAssertEqual(commands, ["snapshot", "proposals"])
    XCTAssertEqual(bridge.proposals.proposals.first?.phrase, "量子桥")
    XCTAssertTrue(bridge.snapshot.entries.isEmpty)
  }
  @MainActor func testAcceptCarriesRealIDAndGenerationOnlyAfterExplicitDecision() async throws {
    var accepted = false
    let candidates = fixture()
    let bridge = VocabularyBridge(control: { command in
      let request = command["request"] as! [String: Any]
      switch request["command"] as! String {
      case "accept":
        XCTAssertEqual(request["id"] as? String, "real-core-id")
        XCTAssertEqual(request["expectedGeneration"] as? UInt64, 0)
        accepted = true
        return Data("{\"confirmed\":true,\"generation\":1}".utf8)
      case "snapshot": return try JSONEncoder().encode(VocabularySnapshot(generation: accepted ? 1 : 0, entries: accepted ? [ConfirmedVocabularyEntry(id: "core-entry", phrase: "量子桥", confirmationGeneration: 1)] : []))
      default: return try JSONEncoder().encode(accepted ? VocabularyProposals(generation: 1, proposals: []) : candidates)
      }
    }, mailbox: nil)
    try await bridge.refresh()
    XCTAssertFalse(accepted)
    try await bridge.apply(VocabularyDecision(action: .accept, proposalID: "real-core-id", expectedGeneration: 0))
    XCTAssertTrue(accepted)
    XCTAssertEqual(bridge.snapshot.entries.first?.phrase, "量子桥")
  }
  @MainActor func testStaleOrInventedDecisionNeverReachesCore() async throws {
    var requests = 0
    let bridge = VocabularyBridge(control: { _ in requests += 1; return Data() }, mailbox: nil)
    for decision in [VocabularyDecision(action: .accept, proposalID: "invented", expectedGeneration: 0),
                     VocabularyDecision(action: .dismiss, proposalID: nil, expectedGeneration: 12)] {
      do { try await bridge.apply(decision); XCTFail("must reject") } catch {}
    }
    XCTAssertEqual(requests, 0)
  }
  @MainActor func testFailureClearsPreviouslyPublishedSharedProjection() async throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let mailbox = try Mailbox(url: directory.appendingPathComponent("test.sqlite"))
    let previous = VocabularySnapshot(generation: 1, entries: [ConfirmedVocabularyEntry(id: "id", phrase: "旧词", confirmationGeneration: 1)])
    try mailbox.set(previous, for: VocabularySnapshot.mailboxKey)
    let bridge = VocabularyBridge(control: { _ in throw VocabularyValidation.invalid }, mailbox: mailbox)
    do { try await bridge.refresh(); XCTFail("must fail") } catch {}
    XCTAssertEqual(try mailbox.value(VocabularySnapshot.self, for: VocabularySnapshot.mailboxKey)?.entries, [])
    XCTAssertNotNil(bridge.errorMessage)
    XCTAssertFalse(bridge.isBusy)
  }
  @MainActor func testMixedGenerationRefreshIsRejected() async throws {
    let bridge = VocabularyBridge(control: { command in
      let operation = (command["request"] as! [String: Any])["command"] as! String
      if operation == "snapshot" { return try JSONEncoder().encode(VocabularySnapshot(generation: 1, entries: [])) }
      return try JSONEncoder().encode(VocabularyProposals(generation: 2, proposals: []))
    }, mailbox: nil)
    do { try await bridge.refresh(); XCTFail("must fail") } catch {}
    XCTAssertTrue(bridge.snapshot.entries.isEmpty)
  }
}
