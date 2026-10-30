import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardInsertionDeliveryTests {
  private let now = Date(timeIntervalSince1970: 1_800_000_000)
  private let epoch = UUID()
  private let oldTarget = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 4)
  private let newTarget = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 0)

  private func withMailbox(_ body: (Mailbox) throws -> Void) throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    try body(Mailbox(url: root.appendingPathComponent("mailbox.sqlite")))
  }

  @Test(arguments: [false, true])
  func reopenedKeyboardReleasesCoreAndLateReceiptCannotAffectNextSession(sameTarget: Bool) throws {
    try withMailbox { (mailbox: Mailbox) throws -> Void in
      var delivery = KeyboardInsertionDelivery()
      let destination = sameTarget ? oldTarget : newTarget
      let oldSession = UUID()
      let oldResult = InsertionRequest(sessionID: oldSession, target: oldTarget,
        text: "Previous app result", now: now)
      #expect(try delivery.offer(oldResult, requestID: "native-old", mailbox: mailbox) == nil)
      #expect(delivery.pendingCount == 1)
      let ping = VoiceCommand(kind: .ping, sessionID: nil, hostEpoch: epoch,
        target: destination, language: .chinese, now: now.addingTimeInterval(1))
      try mailbox.enqueue(ping)
      let queued = try #require(try mailbox.claimCommands(now: now.addingTimeInterval(1)).first)
      let resolutions = try #require(try delivery.observeKeyboardOwnership(queued, sessionID: oldSession,
        startedAt: now, recordingHasEnded: true, mailbox: mailbox,
        now: now.addingTimeInterval(1)))
      #expect(resolutions.count == 1)
      #expect(resolutions.first?.requestID == "native-old")
      #expect(resolutions.first?.submitted == false)
      #expect(delivery.pendingCount == 0, "The old insert no longer keeps Core awaiting a reply.")
      let retained = try #require(try mailbox.pendingInsertions(now: now.addingTimeInterval(1)).first)
      #expect(retained.id == oldResult.id)
      #expect(retained.text == oldResult.text)
      #expect(retained.state == .targetChanged)
      #expect(try mailbox.claimInsertion(oldResult.id, target: destination, now: now) == nil)
      #expect(try mailbox.claimInsertion(oldResult.id, target: oldTarget, now: now) == nil)
      try delivery.finish(sessionID: oldSession, mailbox: mailbox)

      let newSession = UUID()
      let newResult = InsertionRequest(sessionID: newSession, target: destination, text: "New result", now: now)
      #expect(try delivery.offer(newResult, requestID: "native-new", mailbox: mailbox) == nil)
      var lateReceipt = VoiceCommand(kind: .insertionReceipt, sessionID: oldSession,
        hostEpoch: epoch, target: oldTarget, language: .chinese, now: now)
      lateReceipt.insertionID = oldResult.id; lateReceipt.receipt = .submitted
      try mailbox.enqueue(lateReceipt)
      let late = try #require(try mailbox.claimCommands(now: now).first)
      #expect(try delivery.receive(late, mailbox: mailbox) == nil)
      #expect(delivery.pendingCount == 1)
      #expect(try mailbox.claimInsertion(newResult.id, target: destination, now: now) != nil)
      try mailbox.acknowledgeInsertion(newResult.id)
      var receipt = VoiceCommand(kind: .insertionReceipt, sessionID: newSession,
        hostEpoch: epoch, target: destination, language: .chinese, now: now)
      receipt.insertionID = newResult.id; receipt.receipt = .submitted
      let accepted = try #require(try delivery.receive(receipt, mailbox: mailbox))
      #expect(accepted.requestID == "native-new")
      #expect(accepted.submitted)
      #expect(try delivery.receive(receipt, mailbox: mailbox) == nil)
      #expect(delivery.pendingCount == 0)
    }
  }

  @Test(arguments: [false, true])
  func reopenedKeyboardBeforeRecognitionFinishesRetainsResultWithoutWaiting(sameTarget: Bool) throws {
    try withMailbox { (mailbox: Mailbox) throws -> Void in
      var delivery = KeyboardInsertionDelivery()
      let session = UUID()
      let ping = VoiceCommand(kind: .ping, sessionID: nil, hostEpoch: epoch,
        target: sameTarget ? oldTarget : newTarget, language: .chinese, now: now)
      #expect(try delivery.observeKeyboardOwnership(ping, sessionID: session,
        startedAt: now, recordingHasEnded: true, mailbox: mailbox, now: now) == [])
      let result = InsertionRequest(sessionID: session, target: oldTarget, text: "Late result", now: now)
      let rejected = try #require(try delivery.offer(result, requestID: "late-native", mailbox: mailbox))
      #expect(!rejected.submitted)
      #expect(delivery.pendingCount == 0)
      #expect(try mailbox.insertion(result.id)?.state == .targetChanged)
      #expect(try mailbox.pendingInsertions(now: now).first?.text == result.text)
    }
  }

  @Test(arguments: [false, true])
  func ownedSessionHeartbeatPreservesPendingDelivery(sameTarget: Bool) throws {
    try withMailbox { (mailbox: Mailbox) throws -> Void in
      var delivery = KeyboardInsertionDelivery()
      let session = UUID()
      let result = InsertionRequest(sessionID: session, target: oldTarget, text: "Owned result", now: now)
      _ = try delivery.offer(result, requestID: "native-owned", mailbox: mailbox)
      let heartbeat = VoiceCommand(kind: .ping, sessionID: session, hostEpoch: epoch,
        target: sameTarget ? oldTarget : newTarget, language: .chinese, now: now)
      try mailbox.enqueue(heartbeat)
      let queued = try #require(try mailbox.claimCommands(now: now).first)
      #expect(try delivery.observeKeyboardOwnership(queued, sessionID: session,
        startedAt: now, recordingHasEnded: true, mailbox: mailbox, now: now) == nil)
      #expect(delivery.pendingCount == 1)
      #expect(try mailbox.insertion(result.id)?.state == .pending)
      #expect(try mailbox.claimInsertion(result.id, target: oldTarget, now: now) != nil)
      try mailbox.acknowledgeInsertion(result.id)
      var receipt = VoiceCommand(kind: .insertionReceipt, sessionID: session,
        hostEpoch: epoch, target: oldTarget, language: .chinese, now: now)
      receipt.insertionID = result.id; receipt.receipt = .submitted
      #expect(try delivery.receive(receipt, mailbox: mailbox)?.submitted == true)
      #expect(delivery.pendingCount == 0)
    }
  }

  @Test func activeRecordingAndOldProbesDoNotDetachDestination() throws {
    try withMailbox { (mailbox: Mailbox) throws -> Void in
      var delivery = KeyboardInsertionDelivery()
      let session = UUID()
      let ping = VoiceCommand(kind: .ping, sessionID: nil, hostEpoch: epoch,
        target: newTarget, language: .chinese, now: now)
      #expect(try delivery.observeKeyboardOwnership(ping, sessionID: session,
        startedAt: now, recordingHasEnded: false, mailbox: mailbox, now: now) == nil)
      #expect(try delivery.observeKeyboardOwnership(ping, sessionID: session,
        startedAt: now.addingTimeInterval(1), recordingHasEnded: true, mailbox: mailbox,
        now: now.addingTimeInterval(1)) == nil)
      let result = InsertionRequest(sessionID: session, target: oldTarget, text: "Still current", now: now)
      #expect(try delivery.offer(result, requestID: "native-current", mailbox: mailbox) == nil)
      #expect(try mailbox.claimInsertion(result.id, target: oldTarget, now: now) != nil)
    }
  }

  @Test func durableSubmittedWriteWinsOverMissingReceiptDuringDismissal() throws {
    try withMailbox { (mailbox: Mailbox) throws -> Void in
      var delivery = KeyboardInsertionDelivery()
      let session = UUID()
      let result = InsertionRequest(sessionID: session, target: oldTarget, text: "Already written", now: now)
      _ = try delivery.offer(result, requestID: "native", mailbox: mailbox)
      _ = try mailbox.claimInsertion(result.id, target: oldTarget, now: now)
      try mailbox.acknowledgeInsertion(result.id)
      let resolution = try #require(try delivery.detach(sessionID: session, mailbox: mailbox).first)
      #expect(resolution.submitted)
      #expect(try mailbox.insertion(result.id)?.state == .submitted)
      #expect(try mailbox.pendingInsertions(now: now).isEmpty)
    }
  }
}
