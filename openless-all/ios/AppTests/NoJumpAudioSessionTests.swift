import AVFoundation
import Foundation
import Testing
import UIKit
@testable import OpenLess

@MainActor private final class AudioDriverProbe: KeyboardAudioSessionDriver {
  enum Failure: Error { case rejected }
  var calls: [String] = []
  var failCapture = false
  var failRelease = false
  var diagnosticReport: String { calls.joined(separator: ",") }
  func preparePiP() throws { calls.append("preparePiP") }
  func configureCapture() throws {
    calls.append("capture")
    if failCapture { throw Failure.rejected }
  }
  func configurePlayback() throws { calls.append("playback") }
  func release() throws {
    calls.append("release")
    if failRelease { throw Failure.rejected }
  }
}

@MainActor private final class AudioEnvironment {
  let driver = AudioDriverProbe()
  var foreground = true
  var unlocked = true
  var permission = true
  var pip = false
  lazy var audio = NoJumpAudioSession(driver: driver,
    permission: { [unowned self] in self.permission },
    unlocked: { [unowned self] in self.unlocked },
    foreground: { [unowned self] in self.foreground }, observeSystem: false)
  init() { audio.isBackgroundCaptureAvailable = { [unowned self] in self.pip } }
}

/// Tests actual ownership/admission methods with a fake system driver. These
/// establish state and release ordering, not iOS background microphone rights.
@MainActor struct NoJumpAudioSessionTests {
  @Test func idleActivationDoesNotOwnAudioAndPiPAloneAdmitsRepeatedCapture() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = try #require(e.audio.generation)
    #expect(e.driver.calls.isEmpty)
    #expect(!e.audio.ownsAudioSession)
    e.foreground = false
    #expect(!e.audio.admits(generation))
    e.pip = true
    #expect(e.audio.admits(generation))
    for _ in 0..<2 {
      let token = try e.audio.acquireCapture(admission: .keyboard(generation))
      #expect(e.audio.ownsAudioSession)
      e.audio.releaseCapture(token)
      #expect(!e.audio.ownsAudioSession)
      #expect(e.audio.generation == generation)
    }
    #expect(e.driver.calls == ["capture", "release", "capture", "release"])
  }

  @Test func lockUnlockAndForegroundVisitKeepIntentWithoutStartingAudio() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = try #require(e.audio.generation)
    e.pip = true; e.foreground = false
    e.audio.handle(Notification(name: UIApplication.protectedDataWillBecomeUnavailableNotification))
    #expect(e.audio.hasActivationIntent)
    #expect(!e.audio.admits(generation))
    e.audio.handle(Notification(name: UIApplication.protectedDataDidBecomeAvailableNotification))
    #expect(e.audio.admits(generation))
    e.foreground = true; e.audio.foregroundChanged()
    e.foreground = false; e.audio.foregroundChanged()
    #expect(e.audio.admits(generation))
    #expect(e.driver.calls.isEmpty)
  }

  @Test func interruptionStopsCaptureButEndingDoesNotReopenMic() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    e.pip = true
    let generation = try #require(e.audio.generation)
    let token = try e.audio.acquireCapture(admission: .keyboard(generation))
    e.audio.onInterruption = { e.audio.releaseCapture(token) }
    e.audio.handle(Notification(name: AVAudioSession.interruptionNotification,
      userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.began.rawValue]))
    #expect(!e.audio.isReady)
    #expect(!e.audio.ownsAudioSession)
    e.audio.handle(Notification(name: AVAudioSession.interruptionNotification,
      userInfo: [AVAudioSessionInterruptionTypeKey: AVAudioSession.InterruptionType.ended.rawValue]))
    #expect(e.audio.generation == generation)
    #expect(e.driver.calls == ["capture", "release"])
  }

  @Test func routeChangesWhileIdleNeverReclaimAnotherAppsOutput() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    e.foreground = false; e.pip = true
    for reason in [AVAudioSession.RouteChangeReason.newDeviceAvailable, .oldDeviceUnavailable,
      .override, .categoryChange, .noSuitableRouteForCategory] {
      e.audio.handle(Notification(name: AVAudioSession.routeChangeNotification,
        userInfo: [AVAudioSessionRouteChangeReasonKey: reason.rawValue]))
    }
    #expect(e.audio.isReady)
    #expect(e.driver.calls.isEmpty)
  }

  @Test func pipStartupReleaseCannotDeactivateAnOngoingCapture() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    try e.audio.preparePiPStart()
    let token = try e.audio.acquireCapture(admission: .foreground)
    e.audio.settlePiPStart()
    #expect(e.driver.calls == ["preparePiP", "capture"])
    e.audio.releaseCapture(token)
    #expect(e.driver.calls == ["preparePiP", "capture", "release"])
  }

  @Test func failedCaptureReleasesPartialConfigurationAndPreservesPiPIntent() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = e.audio.generation
    e.driver.failCapture = true
    #expect(throws: AudioDriverProbe.Failure.self) {
      _ = try e.audio.acquireCapture(admission: .foreground)
    }
    #expect(e.driver.calls == ["capture", "release"])
    #expect(!e.audio.ownsAudioSession)
    #expect(e.audio.generation == generation)
  }

  @Test func playbackBlocksNewCaptureThenReleasesAndKeepsActivation() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = e.audio.generation
    let token = try e.audio.acquirePlayback()
    #expect(e.audio.generation == nil)
    #expect(throws: VoiceFailure.self) { _ = try e.audio.acquireCapture(admission: .foreground) }
    e.audio.releasePlayback(token)
    #expect(e.audio.generation == generation)
    #expect(e.driver.calls == ["playback", "release"])
  }

  @Test func staleCaptureTokenAndOldGenerationCannotAffectReplacement() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = try #require(e.audio.generation)
    let old = try e.audio.acquireCapture(admission: .foreground)
    e.audio.releaseCapture(old)
    e.audio.deactivate()
    try await e.audio.activate()
    let replacement = try e.audio.acquireCapture(admission: .foreground)
    e.audio.releaseCapture(old)
    e.audio.deactivate(generation: generation)
    #expect(e.audio.ownsAudioSession)
    #expect(e.audio.hasActivationIntent)
    #expect(!e.audio.admits(generation))
    e.audio.releaseCapture(replacement)
    #expect(e.driver.calls == ["capture", "release", "capture", "release"])
  }

  @Test func stoppedPiPOrLostPermissionNeverAdmitsBackgroundCapture() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = try #require(e.audio.generation)
    e.foreground = false; e.pip = false
    #expect(throws: VoiceFailure.self) { _ = try e.audio.acquireCapture(admission: .keyboard(generation)) }
    e.pip = true; e.permission = false
    #expect(throws: VoiceFailure.self) { _ = try e.audio.acquireCapture(admission: .keyboard(generation)) }
    #expect(e.driver.calls.isEmpty)
  }

  @Test func mediaResetAndExplicitStopNeverStartAudioAutomatically() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    let generation = e.audio.generation
    e.audio.handle(Notification(name: AVAudioSession.mediaServicesWereLostNotification))
    #expect(e.audio.generation == nil)
    e.audio.handle(Notification(name: AVAudioSession.mediaServicesWereResetNotification))
    #expect(e.audio.generation == generation)
    e.audio.deactivate()
    e.audio.handle(Notification(name: UIApplication.protectedDataDidBecomeAvailableNotification))
    #expect(!e.audio.hasActivationIntent)
    #expect(e.driver.calls.isEmpty)
  }

  @Test func failedIdleReleaseIsRetriedWithoutReactivation() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    try e.audio.preparePiPStart()
    e.driver.failRelease = true
    e.audio.settlePiPStart()
    #expect(e.audio.ownsAudioSession)
    #expect(!e.audio.isReady)
    e.driver.failRelease = false
    try await Task.sleep(for: .milliseconds(250))
    #expect(!e.audio.ownsAudioSession)
    #expect(e.driver.calls == ["preparePiP", "release", "release"])
  }

  @Test func delayedReleaseCannotDeactivateReplacementCapture() async throws {
    let e = AudioEnvironment()
    try await e.audio.activate()
    try e.audio.preparePiPStart()
    e.driver.failRelease = true
    e.audio.settlePiPStart()
    e.driver.failRelease = false
    let token = try e.audio.acquireCapture(admission: .foreground)
    try await Task.sleep(for: .milliseconds(250))
    #expect(e.audio.ownsAudioSession)
    #expect(e.driver.calls == ["preparePiP", "release", "capture"])
    e.audio.releaseCapture(token)
    #expect(!e.audio.ownsAudioSession)
  }

}
