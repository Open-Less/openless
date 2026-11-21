import Foundation
import Testing

@testable import OpenLessShared

struct LeaseTests {
  @Test func deadKeyboardCannotHoldTheMicrophoneIndefinitely() {
    let start = Date(timeIntervalSince1970: 100)
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 1)
    let lease = KeyboardLease(sessionID: UUID(), target: target, now: start)
    #expect(!lease.expired(at: start.addingTimeInterval(3)))
    #expect(lease.expired(at: start.addingTimeInterval(5)))
  }
  @Test func onlyTheSameSessionAndDocumentCanRenew() {
    let start = Date(timeIntervalSince1970: 100)
    let id = UUID()
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 1)
    var lease = KeyboardLease(sessionID: id, target: target, now: start)
    let time = start.addingTimeInterval(3)
    let foreign = VoiceCommand(
      kind: .ping, sessionID: UUID(), hostEpoch: nil, target: target, language: .chinese, now: time)
    let foreignRenewed = lease.observe(foreign, now: time)
    #expect(!foreignRenewed)
    #expect(lease.expired(at: start.addingTimeInterval(5)))
    let valid = VoiceCommand(
      kind: .ping, sessionID: id, hostEpoch: nil, target: target, language: .chinese, now: time)
    let renewed = lease.observe(valid, now: time)
    #expect(renewed)
    #expect(!lease.expired(at: start.addingTimeInterval(5)))
    #expect(lease.expired(at: start.addingTimeInterval(8)))
  }
  @Test func staleHeartbeatDoesNotRenew() {
    let start = Date(timeIntervalSince1970: 100)
    let id = UUID()
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 1)
    var lease = KeyboardLease(sessionID: id, target: target, now: start)
    let old = VoiceCommand(
      kind: .ping, sessionID: id, hostEpoch: nil, target: target, language: .english, now: start)
    let renewed = lease.observe(old, now: start.addingTimeInterval(12))
    #expect(!renewed)
  }
}
