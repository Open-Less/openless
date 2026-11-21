import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardStartHandshakeTests {
  @Test func outputRecoveryWaitsForRealAdmissionWithoutOpeningHost() {
    var handshake = request(), value = ready()
    value.backgroundReady = false; value.backgroundRecovering = true; value.canStart = false
    #expect(value.keyboardAvailability(at: date) == .preparing)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    value.backgroundRecovering = false; value.backgroundReady = true; value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101) == .start)
  }

  @Test func anotherFieldUsesTheSameHostAdmissionForANewGesture() {
    let nextTarget = InputTarget(keyboardID: target.keyboardID, documentID: UUID(), revision: 0)
    var handshake = KeyboardStartHandshake(id: UUID(), target: nextTarget, uptime: 100)
    let value = ready()
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: nextTarget,
      date: date, uptime: 100) == .start)
    #expect(handshake.hostEpoch == value.hostEpoch)
  }

  private let date = Date(timeIntervalSince1970: 1_800_000_000)
  private let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 4)
  private let intent = KeyboardVoiceIntent()
  private func ready() -> VoiceSnapshot {
    var value = VoiceSnapshot(hostEpoch: UUID())
    value.updatedAt = date
    value.inForeground = false; value.backgroundReady = true; value.canStart = true
    value.phase = .ready
    value.capabilities = .init(revision: UUID(), sourceLanguage: "zh-CN", translationTargets: [],
      styles: [.init(id: "builtin.raw", title: "原文")], canTranslate: false, canHelpWrite: false)
    return value
  }
  private func request() -> KeyboardStartHandshake {
    KeyboardStartHandshake(id: UUID(), target: target, uptime: 100)
  }

  @Test func processingAfterDismissalWaitsThenStartsExactlyOnce() {
    var handshake = request(), value = ready()
    value.phase = .processing; value.canStart = false
    for tick in 0..<6 {
      value.updatedAt = date.addingTimeInterval(Double(tick))
      #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
        date: value.updatedAt, uptime: 100 + Double(tick)) == .waitForHost)
    }
    value.phase = .ready; value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: value.updatedAt, uptime: 106) == .start)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: value.updatedAt, uptime: 106) == .finished)
  }

  @Test func terminalPhaseWithHostCleanupStillWaits() {
    for phase in [VoicePhase.done, .cancelled, .error, .inactive] {
      var handshake = request(), value = ready()
      value.phase = phase; value.canStart = false
      #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
        date: date, uptime: 100) == .waitForHost)
    }
  }

  @Test func busyDeadlineIsAbsoluteAndNeverBecomesForegroundJump() {
    var handshake = request(), value = ready()
    value.canStart = false
    for tick in 0..<20 {
      value.updatedAt = date.addingTimeInterval(Double(tick))
      #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
        date: value.updatedAt, uptime: 100 + Double(tick)) == .waitForHost)
    }
    #expect(handshake.deadline == 120)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date.addingTimeInterval(20), uptime: 120) == .expired)
    value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: value.updatedAt, uptime: 121) == .finished)
  }

  @Test func staleReadyAndStaleBusyBothProbeWithoutTrustingOldReadiness() {
    for phase in [VoicePhase.ready, .recording, .processing] {
      var handshake = request(), value = ready()
      value.phase = phase; value.updatedAt = date.addingTimeInterval(-30)
      #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
        date: date, uptime: 100) == .probe)
      let waiting = handshake.advance(snapshot: value, intent: intent, currentTarget: target,
        date: date, uptime: 100.5)
      if case .awaitResponse(let delay) = waiting { #expect(abs(delay - 0.7) < 0.001) }
      else { Issue.record("A repeated read must not send a duplicate probe.") }
      value.updatedAt = date; value.phase = .ready
      #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
        date: date, uptime: 101) == .start)
    }
  }

  @Test func oneBoundedProbeThenOneForegroundHandoff() {
    var handshake = request(), value = ready()
    value.updatedAt = date.addingTimeInterval(-30)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .probe)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101.2) == .openHost)
    value.updatedAt = date
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101.3) == .finished)
  }

  @Test func vanishedOrChangedTargetCannotRevivePendingGesture() {
    for destination in [nil, InputTarget(keyboardID: target.keyboardID, documentID: target.documentID, revision: 5)] {
      var handshake = request()
      #expect(handshake.advance(snapshot: ready(), intent: intent, currentTarget: destination,
        date: date, uptime: 100) == .targetChanged)
      #expect(handshake.advance(snapshot: ready(), intent: intent, currentTarget: target,
        date: date, uptime: 101) == .finished)
    }
  }

  @Test func hostRestartCancelsTheOldPendingGesture() {
    var handshake = request(), value = ready()
    value.canStart = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    value.hostEpoch = UUID(); value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101) == .hostChanged)
  }

  @Test func unsupportedIntentDoesNotJumpOrSilentlyChangeItsMeaning() {
    var handshake = request()
    #expect(handshake.advance(snapshot: ready(), intent: .init(kind: .translation, targetLanguage: "fr"),
      currentTarget: target, date: date, uptime: 100) == .unavailable)
  }

  @Test func onlyMissingBackgroundAdmissionCanOpenAnIdleLiveHost() {
    var handshake = request(), value = ready()
    value.backgroundReady = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .openHost)
    handshake = request(); value.phase = .processing; value.canStart = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    handshake = request(); value.phase = .ready; value.inForeground = true; value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .start)
  }

  @Test func backgroundAdmissionLossAfterBusyWaitRequiresOneHandoff() {
    var handshake = request(), value = ready()
    value.phase = .processing; value.canStart = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    value.phase = .ready; value.canStart = true; value.backgroundReady = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101) == .openHost)
    value.backgroundReady = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 102) == .finished)
  }

  @Test func staleSnapshotCannotReplacePinnedHostEpoch() {
    var handshake = request(), value = ready()
    let pinnedEpoch = value.hostEpoch
    value.canStart = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    value.hostEpoch = UUID(); value.updatedAt = date.addingTimeInterval(-30)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101) == .probe)
    #expect(handshake.hostEpoch == pinnedEpoch)
    value.updatedAt = date; value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 102) == .hostChanged)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 103) == .finished)
  }

  @Test func freshReadyAtAbsoluteDeadlineCannotStartOrJump() {
    var handshake = request()
    #expect(handshake.advance(snapshot: ready(), intent: intent, currentTarget: target,
      date: date, uptime: 120) == .expired)
    #expect(handshake.advance(snapshot: ready(), intent: intent, currentTarget: target,
      date: date, uptime: 120.1) == .finished)
  }

  @Test func capabilityRemovalWhileWaitingTerminatesOriginalIntent() {
    var handshake = request(), value = ready()
    value.phase = .processing; value.canStart = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    value.capabilities = .init(revision: UUID(), sourceLanguage: "zh-CN", translationTargets: [],
      styles: [.init(id: "another.style", title: "Other")], canTranslate: false, canHelpWrite: false)
    value.phase = .ready; value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 101) == .unavailable)
    value = ready()
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 102) == .finished)
  }


  @Test func detachingTargetAfterWaitingCannotReviveOnReattachment() {
    var handshake = request(), value = ready()
    value.phase = .processing; value.canStart = false
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 100) == .waitForHost)
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: nil,
      date: date, uptime: 101) == .targetChanged)
    value.phase = .ready; value.canStart = true
    #expect(handshake.advance(snapshot: value, intent: intent, currentTarget: target,
      date: date, uptime: 102) == .finished)
  }

}
