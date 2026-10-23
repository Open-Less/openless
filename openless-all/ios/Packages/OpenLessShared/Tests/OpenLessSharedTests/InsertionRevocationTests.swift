import Foundation
import Testing
@testable import OpenLessShared

struct InsertionRevocationTests {
  @Test func revokeNeverReplaysOrErasesAttemptEvidence() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let mailbox = try Mailbox(url: root.appendingPathComponent("mailbox.sqlite"))
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 1)
    for stage in ["pending", "attempting", "submitted"] {
      let item = InsertionRequest(sessionID: UUID(), target: target, text: "fixture")
      try mailbox.storeInsertion(item)
      if stage != "pending" { #expect(try mailbox.claimInsertion(item.id, target: target) != nil) }
      if stage == "submitted" { try mailbox.acknowledgeInsertion(item.id) }
      try mailbox.revokeInsertion(item.id)
      try mailbox.revokeInsertion(item.id)
      let record = try #require(try mailbox.insertion(item.id))
      #expect(record.state == (stage == "pending" ? .targetChanged : stage == "attempting" ? .uncertain : .submitted))
      #expect(record.text == "fixture")
      #expect(try mailbox.claimInsertion(item.id, target: target) == nil)
    }
  }
}
