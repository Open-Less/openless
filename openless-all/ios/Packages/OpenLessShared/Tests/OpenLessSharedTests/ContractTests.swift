import Foundation
import Testing

@testable import OpenLessShared

struct GestureTests {
  @Test func tapStartsWithoutWaitingForHoldThreshold() {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: .ready)
    #expect(gesture.tick(at: 0.299) == nil)
    #expect(gesture.up(phase: .ready) == .start)
    #expect(gesture.tick(at: 1) == nil)
    #expect(gesture.up(phase: .recording) == nil)
  }
  @Test func holdOnlyStartsOnceAndReleaseStops() {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: .ready)
    #expect(gesture.tick(at: 0.3) == .start)
    #expect(gesture.tick(at: 1) == nil)
    #expect(gesture.up(phase: .recording) == .stop)
  }
  @Test func releaseWhileStartingCancelsLateMicrophone() {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: .ready)
    _ = gesture.tick(at: 0.3)
    #expect(gesture.up(phase: .starting) == .cancel)
  }
  @Test func leavingTargetCancelsWithoutTap() {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: .ready)
    #expect(gesture.up(phase: .ready, inside: false) == nil)
    gesture.down(at: 0, phase: .ready)
    _ = gesture.tick(at: 0.3)
    #expect(gesture.up(phase: .recording, inside: false) == .cancel)
  }
  @Test(arguments: [VoicePhase.processing, .awaitingInsertion, .inactive])
  func busyPressDoesNotStartOrCancel(phase: VoicePhase) {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: phase)
    #expect(gesture.tick(at: 1) == nil)
    #expect(gesture.up(phase: phase) == nil)
  }
  @Test func cancellationCannotEmitAnotherAction() {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: .ready)
    _ = gesture.tick(at: 0.3)
    #expect(gesture.cancel() == .cancel)
    #expect(gesture.up(phase: .recording) == nil)
  }
  @Test func tapDuringRecordingStops() {
    var gesture = VoiceGesture()
    gesture.down(at: 0, phase: .recording)
    #expect(gesture.up(phase: .recording) == .stop)
  }
}

struct ContractTests {
  let now = Date(timeIntervalSince1970: 1_000)
  @Test func staleEpochAndDeadlinesAreRejected() {
    let epoch = UUID()
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 1)
    let command = VoiceCommand(
      kind: .start, sessionID: UUID(), hostEpoch: epoch,
      target: target, language: .chinese, now: now)
    #expect(command.rejection(now: now, epoch: epoch) == nil)
    #expect(command.rejection(now: now, epoch: UUID()) != nil)
    #expect(command.rejection(now: now.addingTimeInterval(11), epoch: epoch) != nil)
  }
  @Test func snapshotCannotOutliveTheHost() {
    var snapshot = VoiceSnapshot(hostEpoch: UUID())
    snapshot.updatedAt = now
    snapshot.canStart = true
    #expect(snapshot.isFresh(at: now))
    #expect(!snapshot.isFresh(at: now.addingTimeInterval(5)))
  }
  @Test func preferenceRoundTrip() throws {
    var prefs = KeyboardPreferences()
    prefs.appearance = .typingFirst
    prefs.spaceHold = .recording
    #expect(
      try SharedContract.decode(KeyboardPreferences.self, from: SharedContract.encode(prefs))
        == prefs)
  }
  @Test func oversizedPayloadIsRejected() {
    #expect(throws: ContractError.tooLarge) {
      try SharedContract.encode(String(repeating: "a", count: SharedContract.maxPayloadBytes))
    }
  }
  @Test func noUnverifiedKeepAliveIsEnabled() throws {
    #expect(
      !KeepAliveGate.allows(
        systemBuild: "27A", deviceModel: "iPhone17,1", records: [], experimentEnabled: true))
    let evidence = try JSONDecoder().decode(
      KeepAliveEvidence.self,
      from: Data(
        """
        {"systemBuild":"27A","deviceModel":"iPhone17,1","backgroundRestartPassed":true,"idleMicrophoneReleased":true,"noVisibleSurface":false,"evidenceID":"test"}
        """.utf8))
    #expect(
      !KeepAliveGate.allows(
        systemBuild: "27A", deviceModel: "iPhone17,1", records: [evidence], experimentEnabled: true)
    )
  }
  @Test func transcriptRevisionsDoNotRepeatWords() {
    var transcript = TranscriptAccumulator()
    transcript.apply(.init(start: 0, end: 1, text: "你", final: false))
    transcript.apply(.init(start: 0, end: 2, text: "你好。", final: true))
    transcript.apply(.init(start: 0, end: 1, text: "错", final: false))
    transcript.apply(.init(start: 2, end: 4, text: "再见。", final: true))
    #expect(transcript.text == "你好。再见。")
  }
}

struct MailboxTests {
  func database() throws -> (Mailbox, URL) {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    return (try Mailbox(url: directory.appendingPathComponent("mailbox.sqlite")), directory)
  }
  @Test func commandsClaimOnlyOnceAcrossTwoConnections() throws {
    let (mailbox, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let other = try Mailbox(url: directory.appendingPathComponent("mailbox.sqlite"))
    let command = VoiceCommand(
      kind: .ping, sessionID: nil, hostEpoch: nil,
      target: .init(keyboardID: UUID(), documentID: UUID(), revision: 0), language: .english)
    try mailbox.enqueue(command)
    try mailbox.enqueue(command)
    #expect(try other.claimCommands() == [command])
    #expect(try mailbox.claimCommands().isEmpty)
  }
  @Test func changedDocumentNeverReceivesSpeech() throws {
    let (mailbox, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let original = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 0)
    let insertion = InsertionRequest(sessionID: UUID(), target: original, text: "不会写错")
    try mailbox.storeInsertion(insertion)
    let changed = InputTarget(keyboardID: original.keyboardID, documentID: UUID(), revision: 0)
    #expect(try mailbox.claimInsertion(insertion.id, target: changed) == nil)
    #expect(try mailbox.insertion(insertion.id)?.state == .targetChanged)
  }
  @Test func crashBetweenWriteAndReceiptDoesNotReplay() throws {
    let (mailbox, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 0)
    let insertion = InsertionRequest(sessionID: UUID(), target: target, text: "只提交一次")
    try mailbox.storeInsertion(insertion)
    #expect(try mailbox.claimInsertion(insertion.id, target: target) != nil)
    try mailbox.recoverUncertainInsertions()
    #expect(try mailbox.insertion(insertion.id)?.state == .uncertain)
    #expect(try mailbox.claimInsertion(insertion.id, target: target) == nil)
    try mailbox.storeInsertion(insertion)
    #expect(try mailbox.claimInsertion(insertion.id, target: target) == nil)
  }
  @Test func receiptRequiresAnAttempt() throws {
    let (mailbox, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 0)
    let insertion = InsertionRequest(sessionID: UUID(), target: target, text: "提交")
    try mailbox.storeInsertion(insertion)
    #expect(throws: ContractError.invalidTransition) {
      try mailbox.acknowledgeInsertion(insertion.id)
    }
    _ = try mailbox.claimInsertion(insertion.id, target: target)
    try mailbox.acknowledgeInsertion(insertion.id)
    #expect(try mailbox.insertion(insertion.id)?.state == .submitted)
    #expect(try mailbox.claimInsertion(insertion.id, target: target) == nil)
  }
  @Test func settingsAreReadableAcrossProcesses() throws {
    let (mailbox, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let other = try Mailbox(url: directory.appendingPathComponent("mailbox.sqlite"))
    var prefs = KeyboardPreferences()
    prefs.spaceHold = .recording
    try mailbox.set(prefs, for: SharedContract.preferencesKey)
    #expect(try other.value(KeyboardPreferences.self, for: SharedContract.preferencesKey) == prefs)
  }
}
