import Foundation
import Testing
@testable import OpenLessShared

struct HostVoiceActivationTests {
  private func target(_ revision: UInt64 = 1) -> InputTarget {
    .init(keyboardID: UUID(), documentID: UUID(), revision: revision)
  }
  @Test func legacyRequestDoesNotAuthorizeAutomaticRecording() throws {
    let request = HostVoiceActivation(target: target(), language: .chinese)
    #expect(request.phase == .requested && request.text == nil)
    #expect(!request.permitsAutomaticStart(from: request.activationURL))
    #expect(request.isValid())
    #expect(request.intent.stylePackID == "builtin.raw")
    #expect(request.activationURL.host == "activate")
    #expect(request.activationURL.query?.contains(request.id.uuidString.lowercased()) == true)
    #expect(!request.isValid(at: request.createdAt.addingTimeInterval(121)))
  }
  @Test func onlyFreshMatchingKeyboardTapStartsOnce() throws {
    let now = Date(timeIntervalSince1970: 123456)
    var request = HostVoiceActivation(target: target(), language: .english,
      startOnOpen: true, sourceApplication: "com.apple.mobilesafari", now: now)
    #expect(request.permitsAutomaticStart(from: request.activationURL, at: now))
    #expect(!request.permitsAutomaticStart(from: URL(string: "openless-ios://activate?request=" + UUID().uuidString)!, at: now))
    #expect(!request.permitsAutomaticStart(from: URL(string: "openless-ios://settings?request=" + request.id.uuidString)!, at: now))
    #expect(!request.permitsAutomaticStart(from: URL(string: request.activationURL.absoluteString + "&request=" + request.id.uuidString)!, at: now))
    #expect(!request.permitsAutomaticStart(from: request.activationURL, at: now.addingTimeInterval(121)))
    request.phase = .starting
    #expect(!request.permitsAutomaticStart(from: request.activationURL, at: now))
    request.phase = .cancelled
    #expect(!request.permitsAutomaticStart(from: request.activationURL, at: now))
  }
  @Test func legacyPersistedRequestRemainsManualAfterUpgrade() throws {
    let request = HostVoiceActivation(target: target(), language: .english)
    var json = try #require(JSONSerialization.jsonObject(with: SharedContract.encode(request)) as? [String: Any])
    json.removeValue(forKey: "startOnOpen")
    json.removeValue(forKey: "sourceApplication")
    let old = try SharedContract.decode(HostVoiceActivation.self, from: JSONSerialization.data(withJSONObject: json))
    #expect(old.startOnOpen == nil)
    #expect(!old.permitsAutomaticStart(from: old.activationURL))
  }
  @Test func readyResultBindsCurrentTargetAndCreatesOnlyOneDurableInsertion() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let mailbox = try Mailbox(url: root.appendingPathComponent("ipc.sqlite"))
    var request = HostVoiceActivation(target: target(), language: .english)
    try mailbox.requestHostVoice(request)
    let destination = target(8)
    #expect(throws: (any Error).self) { try mailbox.prepareHostVoiceInsertion(request.id, target: destination) }
    #expect(try mailbox.pendingInsertions().isEmpty)
    request.phase = .ready; request.text = "fixture return"
    try mailbox.updateHostVoice(request)
    #expect(try mailbox.pendingInsertions().isEmpty, "Publishing waits for the keyboard to attach a real current input")
    let first = try mailbox.prepareHostVoiceInsertion(request.id, target: destination)
    let duplicate = try mailbox.prepareHostVoiceInsertion(request.id, target: destination)
    #expect(first.id == duplicate.id)
    #expect(first.text == "fixture return")
    #expect(try mailbox.claimInsertion(first.id, target: destination) != nil)
    try mailbox.acknowledgeInsertion(first.id)
    #expect(try mailbox.claimInsertion(duplicate.id, target: destination) == nil)
    #expect(throws: (any Error).self) { try mailbox.prepareHostVoiceInsertion(request.id, target: self.target()) }
    #expect(throws: (any Error).self) { try mailbox.updateHostVoice(request) }
  }
  @Test func staleOrCancelledRequestsCannotDeliverOrOverwriteLiveRecording() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let mailbox = try Mailbox(url: root.appendingPathComponent("ipc.sqlite"))
    var first = HostVoiceActivation(target: target(), language: .chinese)
    try mailbox.requestHostVoice(first)
    first.phase = .recording
    try mailbox.updateHostVoice(first)
    let replacement = HostVoiceActivation(target: target(), language: .english)
    #expect(throws: (any Error).self) { try mailbox.requestHostVoice(replacement) }
    first.phase = .cancelled; first.text = "must not insert"
    try mailbox.updateHostVoice(first)
    #expect(throws: (any Error).self) { try mailbox.prepareHostVoiceInsertion(first.id, target: self.target()) }
    try mailbox.requestHostVoice(replacement)
    #expect(throws: (any Error).self) { try mailbox.updateHostVoice(first) }
    #expect(try mailbox.pendingInsertions().isEmpty)
  }
  @Test func failedHandshakeCanReplaceDeadHostButNeverAFreshRecording() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let mailbox = try Mailbox(url: root.appendingPathComponent("ipc.sqlite"))
    let now = Date()
    var old = HostVoiceActivation(target: target(), language: .english, now: now)
    try mailbox.requestHostVoice(old)
    old.phase = .recording
    try mailbox.updateHostVoice(old)
    var snapshot = VoiceSnapshot(hostEpoch: UUID())
    snapshot.updatedAt = now
    try mailbox.set(snapshot, for: SharedContract.snapshotKey)
    let replacement = HostVoiceActivation(target: target(), language: .english, startOnOpen: true, now: now)
    #expect(throws: (any Error).self) {
      try mailbox.requestHostVoice(replacement, now: now, replacingUnresponsiveHost: true)
    }
    snapshot.updatedAt = now.addingTimeInterval(-10)
    try mailbox.set(snapshot, for: SharedContract.snapshotKey)
    #expect(throws: (any Error).self) { try mailbox.requestHostVoice(replacement, now: now) }
    try mailbox.requestHostVoice(replacement, now: now, replacingUnresponsiveHost: true)
    #expect(try mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey)?.id == replacement.id)
    #expect(throws: (any Error).self) { try mailbox.updateHostVoice(old) }
  }

}
